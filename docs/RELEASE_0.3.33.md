# Release 0.3.33 — A Console For The Person Running The Server

**Date:** 2026-08-31

## Overview

A self-hosted instance had no way to say no. Registration was always open,
storage was unlimited for every account, and the sign-in form could be hit as
fast as anyone liked. The hosted product answers those with plan limits and a
bot check; this one had neither, so a server reachable from the internet could
be scripted into arbitrary storage consumption by anyone who found it.

This release adds those controls — and rebuilds the admin console around the
person who actually runs one of these servers, which is usually one person with
a box in a cupboard and a handful of friends on it.

## The console

Four screens, in the order the questions get asked.

**Status** — free space on the disk, whether Postgres and the object store are
answering and how quickly, the PostgreSQL version and the database's size, how
many files the bucket holds and how much they weigh, uptime and memory. Plus
warnings about the setup itself: the disk past 80% and again past 92%, sign-ups
open with no storage limit, a proxy in front whose client addresses are being
ignored, an admin token short enough to worry about.

The disk figure is measured on the filesystem the container sits on, which
under a normal Docker setup is the one the database and object-storage volumes
are on — the one that actually runs out. `STATUS_DISK_PATH` points it elsewhere
if yours are on another disk.

The bucket usually holds more than the accounts add up to, and the page says
why rather than leaving it looking like a leak: every vault keeps a set number
of older versions so people can roll back, and revoked shares sit there until
the daily cleanup.

**People** — every account, what it is using, and the invite codes you hand
out. Locking and deleting are here, described in terms of what they actually
do to someone's data.

**Settings** — the rules below.

**Maintenance** — run the expired-share cleanup on demand, what to back up and
why the three volumes have to be backed up together, and a log of every change
made from the console.

The whole thing is styled as an operations tool rather than a product page: one
neutral surface colour, one accent, flat panels, type at reading size, and
colour used only for state that means something. Every coloured state is also
written out in words, so the page still reads in greyscale.

It follows your operating system's light or dark setting, and the Auto / Light
/ Dark control in the header overrides that if you would rather pick. The
choice is remembered per browser, so a shared machine does not impose one
operator's preference on the next.

Gone: subscription plans, Stripe customer and subscription fields, "paid users"
and "active subscriptions" counters, manual plan overrides, and a matrix of
access grants across `shared_plaintext`, `realtime_collaboration` and `kanban`
surfaces. None of it did anything on this build — the Stripe fields were never
written, and no route read the access grants for authorization. The endpoints
behind them are removed too, rather than left to mislead the next reader.

## Invites

Closing sign-ups used to leave no way to add anyone. There is no "create user"
button and there cannot be one: registration is zero-knowledge, so the browser
derives the account's keys from the password and the server never sees it.
Nobody but the new user can make their account.

So the console generates invite codes instead — `MMV-XXXX-XXXX-XXXX-XXXX`, from
an alphabet with no look-alike characters, because these get read down a phone.
A code is single-use, optionally expires, and can be revoked before use. Send
the link and the code is filled in for the person following it; typing it by
hand works too, in any casing, with or without the dashes.

A sign-up that fails for another reason — a username already taken — hands the
code back rather than burning it. While sign-ups are open, codes are neither
required nor spent.

## Added

**Sign-ups can be turned off.** Registration is refused and the app hides the
sign-up form and the "create one" link, so the door is visibly shut rather than
opening onto a refusal after a minute of key derivation. Existing accounts are
unaffected.

**A per-account storage limit.** An upload that would take the account past it
is refused at the point the client asks to upload, before the bytes are sent,
with the usage and the limit in the response. It counts what the database
records — attachments, share copies and the files inside them. Map blobs live
in object storage and their sizes are not recorded, so they are outside the
total; a map's version history is bounded separately by that map's version
limit.

**A largest-single-file limit**, checked the same way.

**Two sign-in throttles.** A per-address limit on sign-in, sign-up and the salt
lookup (30 a minute by default), and a lockout after repeated failed sign-ins
against one username (10, then 15 minutes). Both return `429` with
`Retry-After`. Both can be switched off.

The lockout can be aimed at a user — anyone who knows a username can keep it
locked by failing sign-ins on purpose — which is why it expires on its own and
is a separate switch from the per-address limit.

Wrong usernames are counted the same as wrong passwords. Skipping them would
make a wrong username measurably cheaper than a wrong password, which is a way
to ask the server which accounts exist.

**Client addresses behind a proxy.** Both throttles count per address, and
behind a reverse proxy every request arrives from the proxy. `X-Forwarded-For`
carries the real address but anyone can write anything in it, so it is trusted
only when the operator says so. The Settings screen shows which address your
own request was attributed to, so a misconfiguration is visible rather than
silent. See `docs/DEPLOYMENT.md`.

**Environment seeds.** `REGISTRATION_ENABLED`, `USER_STORAGE_LIMIT_BYTES`,
`MAX_ATTACHMENT_SIZE_BYTES` and `TRUST_PROXY_HEADERS` set the initial values
when the server first starts against an empty database, so a new deployment can
come up already closed. They are ignored once the settings row exists — the
admin console is the single authority, rather than two places that can disagree.
The effective values are logged at startup, with a warning when registration is
open and no storage limit is set.

Every change is written to the admin audit trail with the before and after
values.

## Fixed

- **The admin console never loaded.** It expected a `feedback` array in the
  overview response — a hosted-product surface this backend has no endpoints
  for — and threw on `undefined.filter` immediately after signing in. Nobody
  running this build had a working console.
- **The admin console called the wrong server.** Without a build-time
  `VITE_ADMIN_API_BASE` it fell back to `127.0.0.1:8090` and then to the hosted
  SaaS API, so on any self-hosted install reached by its real hostname the
  console was asking a domain the operator does not run. It now defaults to the
  same origin, which is where this image serves both the console and the API.
- **Storage was reported against a plan that does not exist here.**
  `/api/auth/storage` returned the hosted free tier's 25 MB cap, so a
  self-hosted user with more than that was told they were over a limit nothing
  enforced. Both storage endpoints now report the instance's own limit, or
  effectively boundless when none is set.
- **The admin token was compared with `!=`**, which returns as soon as it finds
  a difference. It is now compared in constant time.
- **The app's service worker was swallowing `/admin/`.** Its navigation
  fallback had no denylist and its scope is the whole origin, so once a browser
  had opened the app, every later navigation — the admin console included — was
  answered with the app's own shell. The operator got the app's sign-in page
  instead of the console, in exactly the browser they use most. `/admin/`,
  `/api/`, `/share/` and `/health` are now excluded.

## Upgrading

```bash
docker pull ghcr.io/mindmapvault/mindmapvault-server:v0.3.33
```

The settings table is created on startup by the existing schema step, so there
is no migration to run. Defaults preserve current behaviour exactly:
registration open, no storage limits. The two throttles start on — 30 auth
requests per address per minute and a lockout after 10 failed sign-ins — which
is well clear of normal use, and both can be turned off in the admin console.

Worth doing once after upgrading: open `/admin/`, read the warnings on
**Status**, and if this server is reachable from the internet, close sign-ups
in **Settings** and add the people you want with invite codes from **People**.
While you are in Settings, check the address your request was attributed to and
turn on the reverse-proxy option if it shows your proxy rather than your own
machine.

One thing this release does not change, and cannot: **there is no password
reset.** Each person's password is what decrypts their vaults and it never
reaches the server. If someone loses it, that data is gone — deleting the
account and starting again is the only way forward.
