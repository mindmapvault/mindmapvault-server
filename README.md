# MindMapVault Server

MindMapVault Server is the community backend package for the MindMapVault platform.

It provides the server-side runtime that supports multi-device sync, secure sharing workflows, and the hosted API contract used by the web client and admin surface.

This repository is intended to be readable, auditable, and deployable as a single packaged server image.

## Why This Project

MindMapVault Server exists as a practical backend for the community edition.

What this repository provides:

- a Rust API backend for encrypted vault sync and metadata management
- a packaged `frontend_app` bundle served at `/` in the server image
- a packaged `frontend_admin` bundle served at `/admin/` in the server image
- a Docker-based local stack for PostgreSQL, Garage, and the server container
- a clear API surface that stays compatible with the web client

What this repository does not own:

- SaaS billing, Cloudflare deployment wiring, or other hosted-commercial operations
- enterprise governance, SSO, audit, or compliance overlays
- a local-only desktop runtime; that belongs in the FOSS desktop repository

## Privacy Boundary

This project is designed for zero-knowledge-compatible workflows.

Key boundary rules:

- server operations must not require plaintext map payloads for normal use
- vault content remains encrypted client-side before upload
- object storage contains encrypted blobs, not decrypted user data
- logs should not expose plaintext notes, keys, or secrets

Important boundary: this is a backend service, not an anonymity system. Password hygiene, endpoint protection, and safe export handling still matter.

## Architecture Overview

Core components:

1. `backend/` - Rust API, auth, storage, and route handlers
2. `frontend_app/` - React + TypeScript web client served by the packaged server
3. `frontend_admin/` - community admin surface served at `/admin/`
4. `docker-compose.yml` - local PostgreSQL, Garage, and server stack
5. `tests/` - load tests and regression helpers

High-level flow:

1. The user opens the web client and authenticates against the backend.
2. The client encrypts vault content locally.
3. The backend stores encrypted metadata and encrypted blobs.
4. The admin surface stays separate from the end-user app but is served from the same packaged image.

Related technical notes:

- `docs/SURFACE_OWNERSHIP.md`
- `mindmapvault-www/docs/internal/PRODUCT_SURFACE_OWNERSHIP.md`

## Code Ownership And Packaging

This repository is the community server baseline.

- `backend/` owns the API contract and the community runtime image.
- `frontend_app/` is the end-user web client served at `/` in the packaged server image.
- `frontend_admin/` is the community admin surface served at `/admin/` in the packaged server image.
- hosted billing, Cloudflare deployment wiring, and other SaaS-only operations belong in `mindmapvault-saas`, not here.
- enterprise governance, SSO, audit, and compliance overlays belong in `mindmapvault-enterprise-server`, not here.

## Getting Started

Prerequisites:

- Docker
- Docker Compose
- WSL 2 if you want to use the native Windows workflow described below

For the full container setup guide, prerequisites, service reference, persistence model, upgrade flow, and troubleshooting notes, see `docs/DEPLOYMENT.md`.

Start the local stack:

```powershell
docker compose up -d postgres garage server
```

What you get:

- PostgreSQL on `127.0.0.1:5432`
- Garage S3 API on `127.0.0.1:9000`
- Garage admin API on `127.0.0.1:3903`
- the packaged server on `http://localhost:8090`

This README keeps the quick-start short. The recommended operator path and container guidance live in `docs/DEPLOYMENT.md`.

If you run the backend directly from source, copy the env template to `backend/.env` first:

```powershell
Copy-Item .env.example backend/.env
```

Then run from the `backend/` directory so `dotenv` picks up that file:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server/backend && cargo run'
```

## Build And Run

Build the single-image server package from the repository root:

```powershell
docker build -f backend/Dockerfile -t mindmapvault-server:local .
```

Run the image directly with an env file:

```powershell
docker run --env-file .env -p 8090:8090 mindmapvault-server:local
```

The same image is used by the compose-based local stack.

## Validation

Useful repository checks:

```powershell
cargo check --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml
node scripts/check_no_committed_secrets.mjs
```

Load test helper:

```powershell
node tests/performance/load-test.mjs --users 200 --concurrency 200 --no-cleanup
```

The load test registers users, signs in, exercises account settings, creates and edits a vault, and reads it back. Use `--cleanup` if you want it to delete the test accounts afterward.

## Release Outputs

Typical outputs include:

- a packaged Docker image for the community server
- a packaged web app bundle for the end-user client
- a packaged admin surface for operators

Build workflow configuration lives in `.github/workflows/build-server-image.yml`.

The workflow builds the same single-image server package, validates pull requests without pushing, and publishes tags from `main` and versioned releases.

Published image:

```text
ghcr.io/mindmapvault/mindmapvault-server:latest
```

If Docker Hub publishing is configured in GitHub Actions, the same image is also published as:

```text
docker.io/<dockerhub-namespace>/mindmapvault-server:latest
docker.io/<dockerhub-namespace>/mindmapvault-server:0.3.24
```

Pull it directly with:

```bash
docker pull ghcr.io/mindmapvault/mindmapvault-server:latest
```

Container registry pushes publish image manifests and layers only. The repository README and `docs/DEPLOYMENT.md` stay in GitHub; they are not embedded into the Docker Hub repository description automatically by this workflow.

## Local Configuration

For container-based local development, `docker-compose.yml` is the primary configuration file.

Edit the `server` service values there to change host, database, S3, JWT, CORS, and logging settings.

For service-by-service explanations, recommended container choices, prerequisites, internet exposure notes, backups, and upgrades, use `docs/DEPLOYMENT.md`.

The backend still auto-loads `backend/.env` when you run it directly from source, but that file is only a fallback for source-based runs.

The compose file also creates the `mindmapvault` bucket during startup. Garage does not implement bucket versioning, so the server skips that step instead of failing on `PutBucketVersioning`.

## Contributing

Please keep changes focused and reviewable.

- preserve encrypted-data boundaries
- keep backend and frontend contracts aligned
- document user-visible changes in the changelog
- run the validation steps relevant to the touched surface

## License

MindMapVault Server is released under the AGPL-3.0-or-later license. See `LICENSE` for details.