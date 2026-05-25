# MindMapVault Server v0.3.26

Initial tagged release of the self-hosted MindMapVault server.

## Highlights

- Zero-knowledge-compatible server architecture: vault content is encrypted client-side before upload and the server stores ciphertext only.
- Packaged web UI: the authenticated app is served from `/` and the admin surface from `/admin/` in the same Docker image.
- Encrypted blob versioning: the server tracks encrypted vault versions without exposing plaintext content.
- Self-hosted storage: PostgreSQL plus S3-compatible object storage, including MinIO and RustFS-style deployments.
- Single-container runtime: one image runs the API, the web UI, and the admin surface together.
- AGPL-3.0-or-later licensing.

## What This Release Is

This is the first stable GitHub release line for MindMapVault Server.

It is intended for self-hosters who want full infrastructure ownership without sending plaintext mind-map content or encryption keys to the server.

## What This Release Is Not

- It is not the enterprise governance edition.
- It does not add team governance, SSO, or audit controls.
- It does not change the client-side encryption boundary.

## Install

```bash
docker pull ghcr.io/mindmapvault/mindmapvault-server:latest
docker compose up -d
```

After startup, the default endpoints are:

- `http://localhost:8090` for the server
- `127.0.0.1:5432` for PostgreSQL
- `127.0.0.1:9000` for S3-compatible storage

## Validation

- `cargo check --manifest-path backend/Cargo.toml`
- `cargo test --manifest-path backend/Cargo.toml`
- `node scripts/check_no_committed_secrets.mjs`

## Notes For Operators

- The release image is built from the repository root and ships both the authenticated app and admin UI.
- Keep the release tag aligned with the `backend`, `frontend_app`, and desktop metadata version numbers.
- If you publish this release publicly, create the Git tag first and then create the GitHub release from that tag.