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

The backend reads environment variables directly and also auto-loads a local `.env` file when present in its working directory.

1. Copy `.env.example` to `backend/.env` for local source-based backend runs.
2. Fill in the values for `SQL_DSN`, `MINIO_*`, `JWT_SECRET`, and `ADMIN_API_TOKEN`.
3. Keep `POSTGRES_DSN` empty unless you need it for backward-compatible local setups.

Example:

```powershell
Copy-Item .env.example backend/.env
```

If you want to run the packaged container directly, you can reuse the same values in a root-level `.env` file:

```powershell
Copy-Item .env.example .env
```

The packaged container can then use that file:

```powershell
docker run --env-file .env -p 8090:8090 ghcr.io/<owner>/mindmapvault-server:latest
```

The new `.gitignore` intentionally ignores both `.env` and `backend/.env` so local credentials do not get committed.

## Local Dependencies

MindMapVault Server expects two infrastructure services:

- PostgreSQL for application data
- MinIO for encrypted blob storage

The repository includes `docker-compose.yml` to start both services and an initialization helper that creates the `mindmapvault` bucket and enables versioning.

Start only the infrastructure services:

```powershell
docker compose up -d postgres minio minio-init
```

What you get:

- PostgreSQL on `127.0.0.1:5432`
- MinIO S3 API on `127.0.0.1:9000`
- MinIO console on `127.0.0.1:9001`

Default local credentials from the compose stack:

- PostgreSQL database: `mindmapvault`
- PostgreSQL user: `postgres`
- PostgreSQL password: `postgres`
- MinIO access key: `minioadmin`
- MinIO secret key: `minioadmin`

These values already match `.env.example`.

## Running The Server Locally

### Source-Based Backend Run

1. Start PostgreSQL and MinIO:

```powershell
docker compose up -d postgres minio minio-init
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

2. Copy the env template to `backend/.env` for compose:

```powershell
Copy-Item .env.example backend/.env
```

3. Start the full local stack:

```powershell
docker compose up -d
```

The compose stack starts PostgreSQL, MinIO, bucket initialization, and the packaged Server image. The default compose `server` service uses `mindmapvault-server:local`, but you can override it with a published image by setting `SERVER_IMAGE`.

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

- pull requests: validate the Docker build without pushing
- pushes to `main`: publish `latest`, `main`, and `sha-<commit>` tags to GHCR
- version tags like `v1.2.3`: publish semver tags to GHCR
- manual runs: allow on-demand rebuilds from the Actions tab

Default published image name:

```text
ghcr.io/<github-owner>/mindmapvault-server
```

To use the published image, create a repository or environment-specific `.env` file from `.env.example` and run the container with `--env-file`.
