# Deployment Guide

This document explains how to run the recommended Docker Compose stack for MindMapVault Server, what each container does, which prerequisites are required, and how to verify, upgrade, back up, and troubleshoot the deployment.

## Scope

The repository-level `docker-compose.yml` is intended for:

- local development
- single-node evaluation
- small self-hosted deployments where Docker Compose is an acceptable operational model

It is not a high-availability orchestration setup. If you need multi-node scheduling, external secret management, or managed TLS and ingress, keep the same service boundaries but move the stack to your orchestration platform of choice.

## Recommended Stack

The supported default stack in this repository is:

- `server` - the packaged MindMapVault Server image that serves the API, the end-user app at `/`, and the admin UI at `/admin/`
- `postgres` - the SQL database used by the backend for account and metadata storage
- `garage` - the S3-compatible object store used for encrypted blobs and versioned object writes

Recommended status by service:

- `server`: required
- `postgres`: required
- `garage`: required for the default compose flow

The repository currently documents and tests the Compose path with PostgreSQL and Garage. If you substitute other services, treat that as a custom deployment and validate it yourself before relying on it.

## Prerequisites

Before starting the stack, make sure you have:

- Docker Engine with the Docker Compose plugin installed and working via `docker compose`
- enough local permissions to create containers, bind ports, and create named Docker volumes
- at least 4 GB of available RAM for a comfortable local run
- free host ports: `8090`, `5432`, `9000`, `3901`, and `3903`
- a writable working copy of this repository

Recommended host environments:

- Linux for straightforward Docker usage
- Windows with Docker Desktop and WSL 2 when running from a Windows checkout

## Service Reference

### `server`

Purpose:

- runs the Rust backend
- serves the packaged app UI and admin UI
- connects to PostgreSQL and Garage

Exposed port:

- `8090` on the host maps to `8090` in the container

Persistence:

- stateless container by default
- durable state lives in PostgreSQL and Garage volumes

### `postgres`

Purpose:

- stores users, encrypted metadata references, auth state, and other SQL-backed backend data

Exposed port:

- `5432` on the host maps to `5432` in the container

Persistence:

- Docker named volume `postgres-data`

### `garage`

Purpose:

- stores encrypted blob objects through an S3-compatible API

Exposed ports:

- `9000` on the host maps to Garage S3 API `3900`
- `3901` on the host maps to Garage RPC `3901`
- `3903` on the host maps to Garage admin API `3903`

Persistence:

- Docker named volume `garage-meta`
- Docker named volume `garage-data`

## Configuration Files

The main files involved in a Compose deployment are:

- `docker-compose.yml` - service definitions, ports, dependencies, and environment defaults
- `.env.example` - example environment variables for source runs or direct `docker run --env-file` usage
- `docker/garage.toml` - Garage server configuration mounted into the Garage container

For the default local Compose workflow, the most important settings already live in `docker-compose.yml` under the `server` service.

Use `.env.example` when you want to:

- run the backend directly from source
- create a custom env file for `docker run --env-file`
- keep your overrides outside the compose file

## Quick Start

From the repository root:

```powershell
docker compose up -d postgres garage server
docker compose ps
```

Expected endpoints after startup:

- app and API: `http://localhost:8090`
- health check target: `http://localhost:8090/health`
- Garage S3 API: `http://127.0.0.1:9000`
- Garage admin API: `http://127.0.0.1:3903`
- PostgreSQL: `127.0.0.1:5432`

Verify the stack:

```powershell
docker compose ps
curl http://127.0.0.1:8090/health
```

When the stack is healthy:

- `postgres` and `garage` should report healthy status
- `server` should be running
- `GET /health` should return success
- opening `http://localhost:8090/login` in a browser should render the app instead of a 404 page

## Configuration Notes

### Database

The compose stack pins the backend to PostgreSQL with:

```text
DB_ENGINE=postgres
POSTGRES_DSN=postgresql://postgres:postgres@postgres:5432/mindmapvault
```

Do not point the `server` container at `127.0.0.1` for the database inside Compose. Use the service name `postgres` so container-to-container networking works correctly.

### Object Storage

The compose stack points the backend at Garage with:

```text
S3_ENDPOINT=http://garage:3900
S3_PUBLIC_ENDPOINT=http://localhost:9000
S3_BUCKET=mindmapvault
```

Important behavior notes:

- the backend expects an S3-compatible object store
- the public endpoint is what generated presigned URLs target from the browser
- the current repository compose flow assumes Garage is the object store

### Secrets And Tokens

The default compose file includes placeholder values for local use:

- `JWT_SECRET`
- `ADMIN_API_TOKEN`
- Garage access keys

For anything beyond a throwaway local setup, replace them with strong unique values before exposing the stack to other users or networks.

Never commit real secrets into the repository.

### CORS

The default `CORS_ALLOWED_ORIGINS` value is tuned for local browser and Tauri development.

If you deploy behind a custom domain or reverse proxy, update this value so it matches the actual browser origin that will call the API.

## Persistence And Backups

The compose stack stores durable data in named volumes:

- `postgres-data`
- `garage-meta`
- `garage-data`

If you delete these volumes, you delete the stored database and object data.

Back up at least:

- the PostgreSQL data volume or a logical SQL dump
- the Garage metadata volume
- the Garage object data volume
- any external env file or secret material used by your deployment

For upgrades and recovery, keep PostgreSQL data and Garage data backed up together so metadata and object storage stay aligned.

## Internet Exposure And Reverse Proxying

For local development, the host port mappings in `docker-compose.yml` are enough.

For internet-facing deployments:

- place a reverse proxy in front of the `server` container
- terminate TLS at the reverse proxy or your ingress layer
- expose only the application entrypoint publicly unless you intentionally need direct access to PostgreSQL or Garage
- keep database and object storage ports private whenever possible

In most cases, only the `server` service should be reachable from the public internet.

## Upgrade Procedure

For a normal image upgrade:

1. Back up PostgreSQL and Garage data first.
2. Pull or build the new `server` image.
3. Stop the running stack.
4. Start the stack again with the updated image.
5. Verify `docker compose ps` and `GET /health`.
6. Perform a login and a basic vault create or read smoke test.

Example:

```powershell
docker compose pull
docker compose up -d
docker compose ps
curl http://127.0.0.1:8090/health
```

If you use the published image from GitHub Container Registry, set `SERVER_IMAGE` before startup:

```powershell
$env:SERVER_IMAGE = 'ghcr.io/mindmapvault/mindmapvault-server:latest'
docker compose up -d postgres garage server
```

If Docker Hub publishing is configured for the repository, you can use the Docker Hub image name instead:

```powershell
$env:SERVER_IMAGE = 'docker.io/<dockerhub-namespace>/mindmapvault-server:0.3.26'
docker compose up -d postgres garage server
```

On tag pushes like `v0.3.26`, the publish workflow emits version tags like `0.3.26` in addition to the original Git ref tag and `latest` on the default branch.

## Common Operations

Start the stack:

```powershell
docker compose up -d postgres garage server
```

Stop the stack but keep data:

```powershell
docker compose down
```

Stop the stack and delete volumes:

```powershell
docker compose down -v
```

View logs:

```powershell
docker compose logs -f server
docker compose logs -f postgres
docker compose logs -f garage
```

Check service status:

```powershell
docker compose ps
```

## Troubleshooting

### `server` does not start

Check:

- `docker compose ps`
- `docker compose logs server`
- whether `postgres` and `garage` reached healthy state first

### Browser uploads fail

Check:

- `S3_PUBLIC_ENDPOINT` points to a browser-reachable address
- `garage` is running and reachable on host port `9000`
- the browser can reach the presigned URL target

### The app loads but API calls fail from the browser

Check:

- `CORS_ALLOWED_ORIGINS`
- whether the browser origin matches the configured value
- whether the reverse proxy preserves the expected host and scheme

### Data disappeared after restart

Check:

- whether the named Docker volumes still exist
- whether `docker compose down -v` was run
- whether the stack was started from the same repository and project context

## Source-Based Backend Runs

If you run the backend directly from source instead of through the packaged image:

1. Copy `.env.example` to `backend/.env`.
2. Start PostgreSQL and Garage with Compose.
3. Run the backend from `backend/` so `dotenv` picks up `backend/.env`.

Example:

```powershell
Copy-Item .env.example backend/.env
docker compose up -d postgres garage
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server/backend && cargo run'
```

## Recommended User Path

If you are evaluating or self-hosting this repository for the first time, start with this sequence:

1. Use the repository `docker-compose.yml` unchanged.
2. Run `postgres`, `garage`, and `server` together.
3. Verify `/health` and `/login`.
4. Only after the baseline works, start replacing defaults such as image tag, JWT secret, CORS origins, or reverse proxy settings.

That keeps the initial setup small, debuggable, and aligned with the path this repository actually documents and validates.