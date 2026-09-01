# Release 0.5.0 — Pictures On The Map

**Date:** 2026-09-02

## Overview

A mind map with a photo on it says something a node full of text does not.
Until now a picture could only be *attached* to a node — stored, listed, and
opened in a dialog. This release puts it on the node, on the canvas, where you
can see it.

The version moves to 0.5 because this is the first release that changes what a
map looks like rather than what the server does with it. It also carries two
fixes for bugs that could lose work, both found while building the feature.

## Pictures on nodes

Four ways to add one, so it fits whatever you were already doing:

- Right-click a node → **Add Image…**
- **Alt+K** — the same shortcut FreeMind uses for "insert image"
- Drop an image file onto a node
- **Ctrl+V** with a picture on the clipboard

The thumbnail is drawn at the picture's own aspect ratio inside a 64×64 box. A
16:9 photo comes out 64×36, a portrait 36×64, a square 64×64. Nothing is
cropped and nothing is letterboxed, because the bitmap is encoded at exactly
the size it will be drawn. An extreme panorama is the one exception: beyond 3:1
the excess is trimmed from the centre, because a 10:1 shot scaled to fit would
otherwise be an unreadable six-pixel sliver.

Clicking the picture opens the full-resolution original.

**Where the thumbnail lives matters.** It is stored inside the map itself, not
in object storage. Three things follow from that, all of them good:

- It inherits the vault's encryption for free. The server holds it as part of
  the same ciphertext it already could not read.
- It renders with **no request at all** — on the canvas, in the vault-list
  preview, and on a shared link. Opening a shared map with pictures on it is
  now faster than it was without them, because nothing has to be fetched to
  see them.
- A picture whose original has been deleted **still renders**. Only
  click-through to full size stops working, and it says so. Restore a map
  version from before an original was deleted and it still looks right.

The cost is bounded on purpose: each thumbnail is WebP with a hard 8 KB
ceiling, re-encoded at lower quality if it would exceed it. A 4000×3000 photo
comes out around 2 KB. Ten pictures on a map add roughly 15 KB per saved
version.

The full-resolution original is kept as an ordinary encrypted attachment, so it
appears in the vault-files dialog and can be downloaded or deleted like any
other file.

**Exports:** PNG and PDF include the pictures. The text formats — Markdown,
FreeMind, Freeplane, XMind, WiseMapping — drop them, as they have no way to
carry an inline image.

## Two fixes worth reading

**An attachment upload discarded unsaved edits.** When freshly uploaded
attachment references came back, they were fed into the editor through the same
prop the editor uses to *load a document* — so it reset its whole working tree,
undo history included, and threw away everything changed since the vault was
opened. Including, for an upload, the change being made at that moment. This
was live before this release and is fixed now.

**Duplicating a node shared its files with the original.** Duplicate gave the
copy new node ids but left the attachment ids alone, so both nodes pointed at
one stored file. Deleting either node's file silently broke the other. The
duplicate now gets its own copy of every file in the subtree, made by copying
the ciphertext inside object storage — nothing is uploaded, and the server
still decrypts nothing.

## Also in this release

- **A reveal toggle on the sign-in and sign-up password fields.** This password
  is the encryption key, nobody can reset it, and a typo is only discovered at
  the next sign-in. Being able to check what was typed is the cheapest guard
  against locking yourself out of your own vaults.

## For operators

Nothing to do beyond the usual upgrade. No database migration, no configuration
change, no forced sign-out.

One new endpoint,
`POST /api/mindmaps/{id}/attachments/{attachment_id}/copy`, used when a node is
duplicated. It copies stored ciphertext within the same vault, counts against
the storage quota like any upload, and prefers the object store's own
server-side copy — falling back to a read-and-write through the backend on
stores that do not implement it.

Storage note: thumbnails ride in the map blob, which is retained per version.
If an account of yours is near its limit and its owner adds pictures to a map
they save often, the lever is the retained version count, not the picture size.

## Upgrading

```
cd ~/mindmapvault-deploy && bash setup.sh   # choose update
```

Or pull `kornelko2/mindmapvault-server:v0.5.0` /
`ghcr.io/mindmapvault/mindmapvault-server:v0.5.0` — same digests, amd64 and
arm64.

Self-hosting guide and what's currently shipping:
[mindmapvault.com/homelab](https://www.mindmapvault.com/homelab/).
