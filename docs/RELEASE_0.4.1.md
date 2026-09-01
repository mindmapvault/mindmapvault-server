# Release 0.4.1 — Notes You Can Actually Write In

**Date:** 2026-09-01

## Overview

Two things: the note editor becomes a real writing surface, and upgrades
start reaching the people on your server.

The second one is the reason this is worth deploying promptly. Until now a
new release could be installed, running, and completely invisible — every
browser that had opened the app kept serving itself the build it first
loaded. If you upgraded to 0.4.0 and someone told you the password form
still was not there, this is why.

## The note editor

Notes were a split screen: a textarea on the left, a rendered preview on the
right, and a Save button. They are now a single surface with an Obsidian-style
live preview — Markdown renders as you type, and the raw syntax reappears only
on the line the caret is on, so it stays editable without a second pane.

- Headings, bold, italic, strikethrough, quotes, inline and fenced code, links.
- Task checkboxes you can click; clicking one rewrites the source.
- Images rendered inline, **including your attachments** — a note referring to
  `attachment://…` shows the decrypted file, in place.
- Enter continues a list or quote and clears the marker on an empty item, Tab
  indents the selected lines, `Ctrl`/`Cmd` + B, I and K do the obvious.

What is stored is still plain Markdown, byte for byte what you typed. Nothing
is converted to HTML on the way in, which is what keeps notes portable and
keeps them the same shape the FOSS and desktop apps write.

**Notes autosave.** The Save button is gone, and so is the way it lost work:
closing the dialog or pressing Escape now writes what is on screen. The header
says Saving… / Saved so you can see it happen.

The dialog is also one screen instead of a stack. Labels and attachments moved
behind a **Details** disclosure, collapsed by default — which is also where
Delete note now lives — so the writing surface gets the window. `Ctrl`/`Cmd` + E
switches between Write and Read.

CodeMirror is loaded only when a note is first opened, as its own chunk, so
nobody who never opens a note pays for it.

## Upgrades now reach the browser

This was two bugs stacked on each other, and either one alone was enough to
pin a user to an old release.

**The app never asked to be updated.** The service worker is built in "prompt"
mode: a new one installs and then waits for the page to tell it to take over.
The registration passed no callback, so nothing ever told it. The new version
sat there, downloaded and waiting, indefinitely. The app now notices and
offers a **Reload** — a prompt rather than an automatic reload, because
reloading discards anything unsaved in the editor.

**The entry points were served with no cache policy at all.** `index.html`,
`/admin/` and `sw.js` went out without `Cache-Control`, so a browser was free
to reuse them for as long as it liked — and a cached `index.html` goes on
requesting the *previous* release's asset filenames no matter what the server
now holds. They are now served `no-cache`. Vite hashes the filenames under
`/assets/`, so those keep a one-year `immutable` and cost nothing.

Four checks in `tests/endpoints/release-smoke.mjs` now hold this in place. The
full path — old worker in charge, new build deployed, prompt appears, reload
lands on the new build — was verified end to end against a built app.

**One-off note for this upgrade:** browsers that cached the old `index.html`
under the previous no-policy build may still need one hard reload to pick up
0.4.1. From 0.4.1 onwards they will not.

## Also fixed

- The vault-files dialog closes on **Escape**, like every other dialog in the
  editor. A pending delete or revoke confirmation takes the Escape first, so it
  never closes out from under a question it just asked.
- `MobileMindMapEditor.tsx` is gone: 861 lines nothing imported, which still
  had to be kept in step with the real editor every time the toolbar changed.
  Its unreachable "Download .mmv" button goes with it. The touch editor is the
  main editor's own mobile branch and is unaffected.
- The sign-in page reported **APP v0.3.33** on a v0.4.0 server —
  `frontend_app/package.json` was missed in the 0.4.0 version bump.

## Upgrading

```
cd ~/mindmapvault-deploy && bash setup.sh   # choose update
```

Or pull `kornelko2/mindmapvault-server:v0.4.1` /
`ghcr.io/mindmapvault/mindmapvault-server:v0.4.1` — same digests, amd64 and
arm64.

No database migration, no configuration change, and unlike 0.4.0 this one does
not sign anyone out.

Self-hosting guide and what's currently shipping:
[mindmapvault.com/homelab](https://www.mindmapvault.com/homelab/).
