# MindMapVault Server

MindMapVault Server is the self-hosted, open-source server with full MindMapVault UI. 

It gives you full infrastructure ownership: your own server, your own storage, no third-party cloud, and the same zero-knowledge encrypted core as the desktop app.

## What It Is

- **Zero-knowledge backend** — vault content is encrypted client-side before upload; the server stores only ciphertext
- **Web UI included** — the end-user app is served at `/` and the admin surface at `/admin/` from the same Docker image
- **PostgreSQL + S3-compatible storage** — works with MinIO, RustFS, or any S3-compatible endpoint
- **Encrypted blob versioning** — the server tracks encrypted versions of each vault without ever seeing plaintext
- **Single Docker image** — one container runs the API, the web UI, and the admin surface together
- **AGPL-3 licensed**

What this server does not include: team management, enterprise governance, SSO, or audit controls. Those belong to the enterprise edition.

## Quick Start

Pull the published image:

```bash
docker pull ghcr.io/mindmapvault/mindmapvault-server:latest
```

Or start the full local stack (PostgreSQL + S3 + server) with Docker Compose:

```bash
docker compose up -d
```

What you get:

- PostgreSQL on `127.0.0.1:5432`
- S3-compatible storage on `127.0.0.1:9000`
- MindMapVault Server on `http://localhost:8090`

**For the full deployment guide** — environment variables, volume mounts, storage setup, upgrades, and production configuration — see [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md).

## Privacy Boundary

This project is designed for zero-knowledge-compatible workflows.

- vault content stays encrypted client-side before upload
- object storage contains encrypted blobs, not decrypted user data
- server operations do not require plaintext map payloads for normal use
- logs must not expose plaintext notes, keys, or secrets

This is a backend service, not an anonymity system. Password hygiene, endpoint protection, and safe export handling still apply.

## Repository Layout

```
backend/          Rust API, auth, storage, route handlers
frontend_app/     React web client (served at / in the packaged image)
frontend_admin/   Admin surface (served at /admin/ in the packaged image)
docker-compose.yml  Local stack: PostgreSQL, S3, server
docs/DEPLOYMENT.md  Full operator guide
tests/            Load tests and regression helpers
```

## Build From Source

Build the single-image package from the repository root:

```bash
docker build -f backend/Dockerfile -t mindmapvault-server:local .
```

Run it with an env file:

```bash
docker run --env-file .env -p 8090:8090 mindmapvault-server:local
```

## Validation

```bash
cargo check --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml
node scripts/check_no_committed_secrets.mjs
```

## Published Image

```text
ghcr.io/mindmapvault/mindmapvault-server:latest
```

Images are built and published automatically via GitHub Actions on pushes to `main` and on version tags. The workflow configuration lives in `.github/workflows/build-server-image.yml`.

## Contributing

- preserve encrypted-data boundaries
- keep backend and frontend contracts aligned
- document user-visible changes in the changelog
- run the validation steps relevant to the touched surface

## License

MindMapVault Server is released under the AGPL-3.0-or-later license. See `LICENSE` for details.
