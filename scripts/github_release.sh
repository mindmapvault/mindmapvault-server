#!/usr/bin/env bash
# Creates or updates the GitHub release for a version tag, so every release
# page names the server image that was published for it.
#
# A new release gets the version's section of CHANGELOG.md as its notes,
# titled from the section's first line ("v0.6.7 — Copy, Cut And Paste Nodes").
# Either way the notes end with a "Server image" section: the pull commands
# and the digest. An existing release keeps its title and its own notes; only
# that section is added, or replaced if it is already there.
#
# Called by .github/workflows/build-server-image.yml after a release image is
# published. Can be run by hand for a tag whose release is missing:
#
#   scripts/github_release.sh v0.6.7 sha256:… ghcr.io/mindmapvault/mindmapvault-server docker.io/kornelko2/mindmapvault-server
#
# Needs gh (authenticated) and python3. The Docker Hub image is optional.

set -euo pipefail

tag="${1:?usage: github_release.sh <tag> <digest> <ghcr-image> [dockerhub-image]}"
digest="${2:?missing image digest}"
ghcr="${3:?missing GHCR image}"
dockerhub="${4:-}"
version="${tag#v}"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
notes="$(mktemp)"
trap 'rm -f "$notes"' EXIT

# The CHANGELOG section, without its own "## [x.y.z]" heading.
section="$(awk -v v="$version" '
  $0 ~ "^## \\[" v "\\]" { found = 1; next }
  found && /^## \[/ { exit }
  found { print }
' "$repo_root/CHANGELOG.md")"

# Title from the first sentence of the section's opening paragraph (which may
# wrap over several lines), in the releases' Title Case.
summary="$(printf '%s\n' "$section" | awk '
  !started && NF && !/^[#-]/ { started = 1 }
  started && !NF { exit }
  started { printf "%s ", $0 }
' | sed -E 's/([.!?])( .*)?$//; s/ +$//')"
title="$tag"
if [ -n "$summary" ]; then
  title="$tag — $(python3 -c 'import sys; print(" ".join(w[:1].upper() + w[1:] for w in sys.argv[1].split()))' "$summary")"
fi

image_block() {
  echo "### Server image"
  echo
  echo '```'
  echo "docker pull $ghcr:$tag"
  if [ -n "$dockerhub" ]; then
    echo "docker pull $dockerhub:$tag"
  fi
  echo '```'
  echo
  echo "Digest \`$digest\`, for \`linux/amd64\` and \`linux/arm64\`. \`latest\` points at the newest release."
}

if gh release view "$tag" >/dev/null 2>&1; then
  {
    # The release's own notes, without a Server image section from an earlier run.
    gh release view "$tag" --json body --jq .body | awk '/^### Server image$/ { exit } { print }' | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'
    echo
    image_block
  } > "$notes"
  gh release edit "$tag" --notes-file "$notes"
  echo "Updated release $tag with its server image."
else
  if [ -z "$(printf '%s' "$section" | tr -d '[:space:]')" ]; then
    echo "CHANGELOG.md has no section for $version." >&2
    exit 1
  fi
  {
    printf '%s\n' "$section" | sed -e '/./,$!d'
    echo
    image_block
  } > "$notes"
  # Only the newest version may be Latest. GitHub would otherwise give the
  # label to whichever release was created last, so filling in an old release
  # would take it from the current one.
  newest="$(gh api '/repos/{owner}/{repo}/tags?per_page=100' --jq '.[].name' | grep -E '^v[0-9]' | sort -V | tail -n1)"
  latest=false
  [ "$tag" = "$newest" ] && latest=true
  gh release create "$tag" --verify-tag --title "$title" --notes-file "$notes" --latest="$latest"
  echo "Created release $tag: $title"
fi
