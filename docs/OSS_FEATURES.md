# MindMapVault Server OSS Feature Status

This document lists the public community-server features in MindMapVault Server.

It is split into:
- features already shipped in the current public release line
- public-facing UI and product-surface items that are part of the OSS server baseline
- explicit out-of-scope items that are not part of this server edition

This OSS server line is online-only and is intentionally limited to backend plus web UI surfaces.
Public sync/offline client features, public collaboration APIs, and Swagger-style API documentation are not part of this repository.

## Done

| Feature | Description | Status |
|---|---|---|
| Zero-knowledge backend | Vault content is encrypted client-side before upload, and the server stores ciphertext only. | Done |
| Packaged web UI | The user app is served at `/`, and the admin surface is served at `/admin/` from the same Docker image. | Done |
| PostgreSQL + S3-compatible storage | The server works with PostgreSQL plus MinIO, RustFS, or any S3-compatible endpoint. | Done |
| Encrypted blob versioning | The server tracks encrypted versions of each vault without exposing plaintext content. | Done |
| Single Docker image runtime | One container runs the API, the web UI, and the admin surface together. | Done |
| Public deployment tooling | Docker Hub publishing, guided install scripts, and deployment docs are available for self-hosters. | Done |
| Public release documentation | Release notes, deployment guidance, and validation notes are published with the repository. | Done |
| AGPL-3 licensing | The server is released under AGPL-3.0-or-later for public self-hosting and source availability. | Done |

## UI and public product surface

These are part of the OSS server experience and should remain publicly visible in the repo documentation.

| Feature | Description | Status |
|---|---|---|
| End-user web app | The main user interface is served from `/` in the packaged image. | Done |
| Admin surface | The admin UI is served from `/admin/` in the packaged image. | Done |
| Local deployment UX | The `scripts/publish_dockerhub/setup.sh` installer and `docker-compose.yml` provide the operator flow for local and self-hosted setups. | Done |
| Versioned release tags | Published images follow the `v<version>` release-tag convention for public releases. | Done |

## Explicitly out of scope for this OSS edition

These capabilities remain reserved for other product lines and are not planned for the OSS server repository:

| Feature | Description | Status |
|---|---|---|
| Cloud encrypted sync backend | Managed cloud sync infrastructure for multi-device hosted storage. | Out of scope |
| Managed real-time collaboration | Hosted live collaboration infrastructure and relay services. | Out of scope |
| Billing and subscriptions | Tenant billing, subscription, and payment operations. | Out of scope |
| Production hosted control plane | Managed deployment orchestration and hosted operator surface. | Out of scope |
| Admin and analytics surfaces | Enterprise governance, audit, and analytics overlays. | Out of scope |

## Related docs

- [Surface ownership](SURFACE_OWNERSHIP.md)
- [Deployment guide](DEPLOYMENT.md)
- [Current release notes](RELEASE_0.3.27.md)
