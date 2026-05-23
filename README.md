# MindMapVault Server

MindMapVault Server is the technical backend component for the MindMapVault platform.

Purpose:
- provide sync for multi-device clients
- provide secure sharing workflows
- enable future collaboration features
- expose a clear and auditable API contract

Positioning:
- backend service, not the commercial edition itself
- optional add-on for the community, not a paywall over FOSS

Privacy boundary:
- designed for zero-knowledge-compatible workflows
- server must not require plaintext map payloads for standard operations

License:
- AGPL-3.0-or-later (see LICENSE)

Repository status:
- private during initial structuring

## Code Ownership And Packaging

This repository is the community server baseline.

- `backend/` owns the API contract and the community runtime image.
- `frontend_app/` is the end-user web client served by the backend at `/` in the packaged server image.
- `frontend_admin/` is the community admin surface served by the backend at `/admin/` in the packaged server image.
- hosted billing, Cloudflare deployment wiring, and other SaaS-only operations belong in `mindmapvault-saas`, not here.
- enterprise governance, SSO, audit, and compliance overlays belong in `mindmapvault-enterprise-server`, not here.

See `docs/SURFACE_OWNERSHIP.md` for the local rule set and `mindmapvault-www/docs/internal/PRODUCT_SURFACE_OWNERSHIP.md` for the canonical cross-repo split.

## Local Configuration

For container-based local development, `docker-compose.yml` is the primary configuration file.
Edit the values in the `server` service there to change host, database, S3, JWT, CORS, and logging settings.

The backend still auto-loads a local `.env` file when you run it directly from `backend/`, but that file is now only a fallback for source-based runs.

1. Use `docker compose up -d postgres garage server` for the default local stack.
2. Edit `docker-compose.yml` directly if you want to change backend settings for the compose stack.
3. If you run the backend from source, create or edit `backend/.env` with only the values you need for that direct run.

Example source-based setup:

```powershell
Copy-Item .env.example backend/.env
```
The `.gitignore` intentionally ignores both `.env` and `backend/.env` so local credentials do not get committed.

## Clone And Build

If you are starting from scratch, clone the repository first and then build the server image:

```powershell
git clone https://github.com/mindmapvault/mindmapvault-server
cd mindmapvault-server
docker build -f backend/Dockerfile -t mindmapvault-server:local .
```

The same build works from WSL:

```powershell
wsl.exe -d Ubuntu bash -lc 'git clone https://github.com/mindmapvault/mindmapvault-server && cd mindmapvault-server && docker build -f backend/Dockerfile -t mindmapvault-server:local .'
```

## Tests

Repository test helpers live under `tests/`.

- `tests/performance/load-test.mjs` runs a JavaScript load test against the backend and defaults to 200 concurrent users.

Run it after starting the local stack:

```powershell
node tests/performance/load-test.mjs --users 200 --concurrency 200 --no-cleanup
```

The script registers test users, logs them in, reads account data, updates settings, exercises notifications, creates and edits a vault, and then reads it back. Add `--cleanup` if you want it to attempt vault and profile deletion after the run.

## Local Dependencies

MindMapVault Server expects two infrastructure services:

- PostgreSQL for application data
- an S3-compatible object store for encrypted blob storage

The repository includes `docker-compose.yml` to start both services and an initialization flow that creates the `mindmapvault` bucket. Garage is supported as the local S3-compatible backend, but it does not implement bucket versioning, so the server skips that step at startup instead of failing on `PutBucketVersioning`.

The compose file is also the main place to edit the backend runtime settings for the local container stack.

Start only the infrastructure services:

```powershell
docker compose up -d postgres garage server
```

What you get:

- PostgreSQL on `127.0.0.1:5432`
- Garage S3 API on `127.0.0.1:9000`
- Garage admin API on `127.0.0.1:3903`

Default local values from the compose stack:

- PostgreSQL database: `mindmapvault`
- PostgreSQL user: `postgres`
- PostgreSQL password: `postgres`

- S3 access key: `garage-access-key`
- S3 secret key: `garage-secret-key`

Change the inline values in `docker-compose.yml` before using the stack in a shared or long-lived environment.

## Running The Server Locally

### Source-Based Backend Run

1. Start PostgreSQL and the S3-compatible storage backend:

```powershell
docker compose up -d postgres garage server
```

2. Copy the env template to `backend/.env`:

```powershell
Copy-Item .env.example backend/.env
```

3. Run the backend from WSL or Linux from the `backend/` directory so `dotenv` picks up `backend/.env`:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server/backend && cargo run'
```

### Containerized Run

1. Build the single-image Server container:

```powershell
docker build -f backend/Dockerfile -t mindmapvault-server:local .
```

If you want to run the same build step from WSL, use:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server && docker build -f backend/Dockerfile -t mindmapvault-server:local .'
```

2. Start the local stack with Docker Compose:

```powershell
docker compose up -d postgres garage server
```

If you prefer to launch it from WSL, use:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server && docker compose up -d postgres garage server'
```

3. If you want a direct source-based backend run instead of the container image, copy the env template to `backend/.env`:

```powershell
Copy-Item .env.example backend/.env
```

The compose stack starts PostgreSQL, Garage, bucket initialization, and the packaged Server image. The default compose `server` service uses `mindmapvault-server:local`, but you can override it with a published image by setting `SERVER_IMAGE`.

## Docker Image

The community Server repository publishes a single Docker image that contains:

- the Rust backend
- the built `frontend_app` bundle served at `/`
- the built `frontend_admin` bundle served at `/admin/`

Build locally from the repository root:

```powershell
docker build -f backend/Dockerfile -t mindmapvault-server:local .
```

Run locally with your env file:

```powershell
docker run --env-file .env -p 8090:8090 mindmapvault-server:local
```

If you prefer the compose-based local stack, the same image is used by `docker compose up -d`.

## GitHub Actions Image Publishing

The workflow in `.github/workflows/build-server-image.yml` builds the same single-image Server package.

It uses the current common GitHub Actions versions for container builds, including `actions/checkout@v4` and `docker/build-push-action@v6`.

- pull requests: validate the Docker build without pushing
- pushes to `main`: publish `latest`, `main`, and `sha-<commit>` tags to GHCR
- version tags like `v1.2.3`: publish semver tags to GHCR
- manual runs: allow on-demand rebuilds from the Actions tab

Default published image name:

```text
ghcr.io/<github-owner>/mindmapvault-server
```

To use the published image, create a repository or environment-specific `.env` file from `.env.example` and run the container with `--env-file`.
