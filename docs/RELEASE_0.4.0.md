# Release 0.4.0 — Changing Your Password Works Now

**Date:** 2026-09-01

## Overview

Changing your password on a self-hosted MindMapVault has been deliberately
disabled for months, with a guard in the code that said why: the old flow
re-encrypted your keys and your vault titles but never re-wrapped the keys
protecting your attachments — which are derived from the password. One
password change and every attachment on the account became permanently
unreadable, silently. Rather than ship that, the button was turned off.

This release turns it back on, properly. It also brings the first piece of
the app's new settings hub, which is where the password form lives. Together
they close the last gap in the account lifecycle: a self-hosted account can
now be created (openly or by invite), administered, have its password
changed, and be deleted — all without anyone but its owner ever holding a
key to it.

The version jump from 0.3.x marks that milestone.

## Changing your password

Open the gear in the toolbar → **Account** → **Password**. What happens next
runs entirely in your browser:

1. Your current password is verified by unwrapping your keys locally — the
   server is not consulted and never sees either password.
2. Everything the password protects is re-encrypted: both private keys,
   every vault and board title, every vault note, and the wrapped file key
   of **every attachment**, including uploads that never finished. Vault
   contents, version history and shares are untouched — they are keyed to
   your key-pair or to a share passphrase, neither of which changes.
3. The whole result is committed in **one all-or-nothing transaction**. There
   is no state where half your data is under the old key. If anything is
   wrong — wrong password, a vault or attachment missing from the bundle —
   the server refuses and nothing changes.

Attachments stored before v0.3.22 used an older key-wrapping format; a
password change upgrades them to the current one as it re-wraps them.

Two behaviours are worth knowing before you use it:

**Every other signed-in device is signed out.** A session on your phone still
derives keys from the old password; if it were allowed to keep writing, its
saves would be encrypted under keys that no longer exist. So sessions now
carry the key generation they were issued under, and a session from before
the change is refused on any write and on token refresh. It signs back in
with the new password and carries on.

**An interruption tells you the truth.** If the connection dies at the wrong
moment, the app does not guess: it checks which Argon2 salt the server hands
out — a password change always mints a fresh one — and reports definitively
whether the new password took. You are never left trying both.

## What this release does *not* do

- **It does not rotate your key-pair.** Vault contents are sealed to your
  key-pair, which a password change leaves alone. If you believe the
  key-pair itself is compromised, a password change does not heal that.
- **It is not a password reset.** The server still holds nothing that can
  recreate your keys. If a password is lost, that account's data is gone —
  the console says so where you would look for the reset button.

## The settings hub

The theme popover had grown into a scrolling column of unrelated controls.
It is now a proper settings surface — **Account**, **Appearance**, **Help** —
opened from the same gear. Account carries your optional profile fields
(name and email, stored unencrypted and visible to your admin — the app says
so next to the fields), auto-logout, the password form above, and account
deletion. Appearance is the theme, accent colour and autosave. No billing
anywhere, because this product has none.

## Under the hood, for the curious

The rotation transaction runs on a dedicated database connection with the
completeness checks inside it, under a row lock — the previous
implementation's transaction ran on the connection every request shares,
which is not the isolation the word "transaction" suggests. The full design,
failure analysis and guarantees are in `docs/PASSWORD_ROTATION.md`.

Proven by three suites: the server contract (51 checks — coverage,
atomicity, stale sessions, the salt probe), the real client crypto in a
browser (legacy-wrap upgrade included), and a three-account matrix — 60
attachments, accounts that did not rotate shown byte-identical while others
did, a forged cross-account bundle refused, two accounts rotating in
parallel, and a decrypt-everything sweep under the new passwords only.

## Upgrading

```
cd ~/mindmapvault-deploy && bash setup.sh   # choose update
```

Or pull `kornelko2/mindmapvault-server:v0.4.0` /
`ghcr.io/mindmapvault/mindmapvault-server:v0.4.0` — same digests, amd64 and
arm64.

**After the upgrade, every signed-in session re-authenticates once**: older
tokens predate the key-generation claim and are refused on refresh. That is
the fail-closed choice, made deliberately.

Self-hosting guide and what's currently shipping:
[mindmapvault.com/homelab](https://www.mindmapvault.com/homelab/).
