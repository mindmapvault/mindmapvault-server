# Release 0.5.2 — Rotate Your Secrets

**Date:** 2026-09-05

## Read this first

The guided installer (`scripts/publish_dockerhub/setup.sh`) could leave an
instance running on the secrets published in this repository. If you installed
with it and pressed Enter through the prompts, **your `JWT_SECRET` and
`ADMIN_API_TOKEN` are values anyone can read on GitHub.** A JWT secret that is
public lets anyone mint a session token for any account on your server.

Check now, before upgrading:

```bash
grep -E '^(JWT_SECRET|ADMIN_API_TOKEN|POSTGRES_PASSWORD|S3_ACCESS_KEY|S3_SECRET_KEY)=' .env.deploy
```

Any value starting `replace_with_` is a published secret. See
[Rotating a compromised install](#rotating-a-compromised-install) below.

**This version refuses to start on those values.** That is deliberate: an
instance running with a public JWT secret is not secured, and a warning in a log
nobody reads is not a defence. If your server stops after the upgrade, it is
telling you it was never protected.

## What went wrong

Step 2 of the installer copies `.env.deploy.example` to `.env.deploy` before it
asks anything. Step 4 then read those values back as "existing" secrets and
offered them as the default: `Enter=keep current, gen=generate new`. On a fresh
install, "keep current" meant keeping the template's placeholder. The random
value the script generates on the same line was computed and discarded.

The bug only shows on installs where the operator accepted the defaults. If you
typed `gen` or pasted your own values, you were never affected.

## Rotating a compromised install

In order, and expect to be signed out:

1. **`JWT_SECRET`** — replace with 64 random characters
   (`head -c 48 /dev/urandom | base64`). Every existing session and refresh
   token stops working; everyone signs in again. Do this first.
2. **`ADMIN_API_TOKEN`** — replace the same way. Nothing else depends on it.
3. **`POSTGRES_PASSWORD`** — change it in the database as well as the file:
   `docker compose exec postgres psql -U postgres -c "ALTER USER postgres PASSWORD '<new>';"`
   then update `.env.deploy` and restart.
4. **`S3_ACCESS_KEY` / `S3_SECRET_KEY`** — these are the Garage credentials in
   the same file. Changing them means re-keying the bucket; if your Garage is
   bound to localhost as the shipped Compose file does, the exposure is limited
   to anyone who already had host access. Plan it rather than rushing it.

Vault contents are not at risk from any of this. They are encrypted client-side
with keys derived from each account's password, which the server never holds.
What a leaked `JWT_SECRET` grants is the ability to *authenticate as* an
account — and the app then asks for that account's password before it can
decrypt anything.

## Also fixed in the installer

**An update no longer discards your Compose edits.** Every run re-downloaded
`docker-compose.yml` and `garage.toml` over whatever was on disk, so a changed
port, a TLS front end or a memory limit vanished on each update with nothing to
restore from. Modified files are now copied to `.bak` first, and only when they
actually differ from what we published.

## New: link a node to another vault

A node can now point at another vault. Right-click a node and choose **Link to
Vault…**, or use the **Link** button in the toolbar's Content group, or press
**Ctrl+K** (**⌘K** on a Mac) — the key FreeMind uses for hyperlinks and the one
macOS uses everywhere else. Pick from a searchable list of your vaults; the node
grows a strip along its bottom naming the target, and clicking it opens that
vault.

The vault's title is stored on the node next to its id. Titles are encrypted, so
a node holding only an id could not draw its own strip offline, inside a share,
or once the target vault is gone. The trade is that renaming a vault leaves the
old title on nodes that link to it.

## Other fixes

- **Edits made while a file uploads are no longer discarded.** Six code paths
  rebuilt the map from a copy taken *before* the upload started, so anything you
  typed while a file was in flight was thrown away when it finished.
- **Nodes with a vault link are no longer 18px too tall.** The layout reserved a
  footer strip for a field nothing has ever drawn.
- **An export with a blank title is no longer named `.md`.** A title of only
  spaces produced a file with an extension and no name.
- **One corrupt local label list no longer empties your vault list.** In local
  mode a single malformed `localStorage` entry failed the whole page; it now
  costs that one vault its labels.

## Under the hood

No behaviour change, but the reason the fixes above were found at all.

`MindMapEditor.tsx` went from 3,794 lines to 3,380, `VaultsPage.tsx` from 2,259
to 1,408, and the Postgres store from one 1,943-line file to six. The node's
measurement arithmetic existed in three places and had drifted between them —
which is why whitespace-only notes and externally-held attachments each stole
18px from a node's text, and why vault thumbnails put labels where the editor
did not. It exists once now, in `packages/mindmap-core`, with tests.

The frontend had no tests at the start of this work and has **136**; the backend
has 52. The endpoint coverage suite, which aborted on its first case because it
was copied from another repo, now runs 46 checks clean.

Plans and reasoning: `docs/MINDMAP_EDITOR_REFACTORING_PLAN.md`,
`docs/VAULTS_PAGE_REFACTORING_PLAN.md`, `docs/BACKEND_SQL_REFACTORING_PLAN.md`,
and the rules they produced in `CLAUDE.md`.

## Upgrading

```bash
docker compose --env-file .env.deploy pull
docker compose --env-file .env.deploy up -d
```

No migration. If the server does not come back, read its logs — it is almost
certainly the placeholder-secret refusal described at the top.
