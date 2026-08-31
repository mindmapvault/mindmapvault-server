# Release 0.3.32 — Encrypted Share Links

**Date:** 2026-08-31

## Overview

Share links work. The web app has shipped a share UI since 0.3.27 — a share tab
in the editor, a `/shared/:id` page, an API client calling
`/api/mindmaps/{id}/shares…` — against a backend that had no share routes at
all. Every share action failed and every link led to a dead page. This release
adds the backend, the ways to reach it, and the recipient experience.

A share is a second, independently-keyed copy of a vault, encrypted in the
browser under a passphrase the server never sees. The server stores ciphertext
and hands it to whoever holds the link. Recipients need no account.

## Sharing

**Creating one.** The desktop canvas toolbar gained **Vault files** (F7) and
**Share exports** (F8) buttons — until now the share panel had no desktop entry
point whatsoever — and every vault in the lobby has a share action in both the
card and table views.

**The dialog** is organised into what is shared, the passphrase, and access.
The passphrase and its confirmation are masked behind one Show/Hide toggle;
pressing **Generate** produces roughly 100 bits from an alphabet without
look-alike characters and reveals it, because a generated passphrase you cannot
read is one you cannot pass on. Anything under 12 characters is refused.

**Expiry** is a number of days or **never**. Never-expiring links say plainly
that they live until revoked.

**Receiving one.** The recipient opens the link with no account, enters the
passphrase, and sees the map drawn on a canvas — the same renderer that draws
your vault previews — with the outline one click away and any included files
available decrypted. A wrong passphrase now says so.

**Revoking** deletes the encrypted copy rather than only flagging the row, and
a daily sweep clears shares past their expiry. Before this, a revoked share's
ciphertext would have stayed in object storage indefinitely, still counted
against the owner's storage.

**An overview** in the lobby summarises both directions and filters the vault
list when clicked: *I'm sharing* counts active shares across your vaults;
*Shared with me* counts vaults you imported from someone's share. That second
one is as honest as the model allows — a share has no recipient, so an
imported copy is the only durable record that something was shared with you.

## Fixed

- **Shares carried the vault's own preview thumbnails.** With "include files"
  on, one code path fell back to the raw attachment list, which still contains
  the internal `__vault_preview_*` images, and handed them to recipients.
- **A wrong share passphrase gave no feedback.** A failed AES-GCM decryption
  throws `OperationError`, an `Error` whose `message` is empty, so the error
  banner — which renders only a non-empty message — stayed hidden and the page
  simply sat there.

## Changed

- **File extensions match the product name.** Share exports are `.mmvshare`
  (was `.cmvshare`); the desktop vault export is `.mmv` (was `.cmvault`). The
  `cmv`/`cryptmind` prefixes were left from the project's former name. Stored
  crypto format identifiers are deliberately unchanged, so existing shares and
  attachments keep working.
- **Share links are built from the request host**, honouring
  `X-Forwarded-Host` / `-Proto`, so an instance behind a reverse proxy emits
  its public URL.
- **Release images are now built for `linux/amd64` and `linux/arm64`**, so a
  Raspberry Pi or an Ampere VPS can run the server. Each architecture is built
  on a runner of that architecture and the two are joined into one multi-arch
  tag; emulating arm64 on an x86 runner made the Rust build take the better
  part of an hour. Version tags and `latest` are applied on release tags rather
  than on every push to `main`, so `latest` means "newest release" and the
  published image carries its real version.

## Upgrading

```bash
docker pull ghcr.io/mindmapvault/mindmapvault-server:v0.3.32
```

The share tables are created on startup by the existing schema step, so there
is no migration to run and no configuration to change.

One note for anyone who had the previous version open in a browser: the app
registers a service worker, so a reload is needed before the new UI appears.
