# Release 0.3.31 — Upload, Logging and Export Fixes

**Date:** 2026-08-31

## Overview

A maintenance release. No new features — it fixes things that were broken or
too noisy in 0.3.30, and takes two dependency updates for published advisories.

Two of these are worth upgrading for on their own: attachments larger than
about 2 MB could not be uploaded at all, and PNG/PDF exports produced an empty
page.

## Fixed

**Uploads over ~2 MB failed with a bare `413`.** The attachment `init` call
accepted the declared size, so nothing warned the user, and only the upload
that followed failed. Two body limits were stacked: axum's 2 MB default, which
the upload routes never overrode, and a global 10 MiB ceiling that clipped
anything set above it. Both upload routes now carry a limit derived from the
largest per-plan attachment cap, and the global ceiling clears it.

**PNG and PDF exports came out empty** apart from the background and
watermark. The canvas `<svg>` is sized by CSS and carries no `width`/`height`
attributes; serialized into a standalone image none of that CSS applies, so it
rasterized at the SVG default of 300×150 and the map was cropped away. The
export now stamps the measured size before serializing.

**Export filenames picked up the day of the month as a version.** A date
fallback label (`v 6. 8. 2026`) matched an unanchored pattern and exported
`MyMap` as `MyMap-v6.png`. Only a real sequential label (`v12`) contributes a
filename token now.

**Markdown import turned flat lists into descending chains** — each sibling
item became a child of the one before it.

**`VITE_BACKEND_URL` in `.env` was ignored** by both dev proxies, which read
`process.env` where Vite does not put `.env` values.

## Changed

**Postgres and Garage ports are bound to `127.0.0.1` in `docker-compose.yml`.**
Docker's published ports are inserted ahead of ufw-style host firewalls, so the
previous mappings exposed the database and object store to the network even
when the firewall said otherwise. Only `8090` is meant to be reachable from
outside. See `docs/DEPLOYMENT.md` for how to serve presigned URLs through a
reverse proxy instead of republishing port `9000`.

**Logs no longer carry usernames or raw database errors.** PostgreSQL errors
are logged by error code only — the full message can contain column values from
constraint violations. Registration, login and account deletion log the user
UUID instead of the username. JWT failures return `invalid or expired token`
to the client rather than the library's own error text.

**Every request gets an ID**, echoed back as `X-Request-ID` and attached to
each log line for that request. HTTP method, path, status and latency are now
logged at INFO; the compose default `RUST_LOG` drops from `backend=debug` to
`backend=info` to match.

**The built-in CORS default no longer lists hosted domains.** The compiled-in
fallback covers localhost development and the desktop origins only. Set
`CORS_ALLOWED_ORIGINS` for anything else, as the compose file already does.

## Security

- `h2` updated to 0.4.19 for RUSTSEC-2026-0258 (unbounded empty DATA frames).
- The admin console's `nanoid` override raised to 3.3.18 for
  GHSA-2v37-7h3g-55p8; the previous pin sat one patch below the fix.

## Upgrading

```bash
docker pull ghcr.io/mindmapvault/mindmapvault-server:v0.3.31
```

No database migration and no configuration change is required. Two things to
be aware of:

- If you relied on reaching Postgres or the S3 endpoint from another machine
  via the compose port mappings, that no longer works by default — bind them
  deliberately or put them behind a proxy.
- If you run the binary without setting `CORS_ALLOWED_ORIGINS` and depended on
  the old built-in list, set the variable explicitly.
