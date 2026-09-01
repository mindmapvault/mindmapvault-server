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

Exposed ports (bound to `127.0.0.1` — see the note below):

- `9000` on the host maps to Garage S3 API `3900`
- `3901` on the host maps to Garage RPC `3901`
- `3903` on the host maps to Garage admin API `3903`

Port binding note: the compose file binds the Postgres and Garage host ports to
`127.0.0.1` on purpose. Docker's published ports are inserted ahead of
ufw-style host firewalls, so a plain `5432:5432` mapping exposes the database
to the network even when the firewall says otherwise. Only `8090` is meant to
be reachable from outside, ideally behind a reverse proxy. If browsers on
other machines must reach presigned URLs directly, put the S3 endpoint behind
the same reverse proxy and point `S3_PUBLIC_ENDPOINT` at that address instead
of republishing port `9000`.

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

### Instance Settings

Four controls decide how much a stranger who finds your server can do with it.
They live in the database, not in the environment, and you change them in the
admin console at `/admin/` under **Settings**. Changes apply immediately; there
is nothing to restart.

The console has four screens: **Status**, **People** (accounts and invite
codes), **Settings** (the table below), and **Maintenance** (the cleanup job,
what to back up, and a log of every change made from the console).

**Status** answers "is it working and is anything about to go wrong":

- **Disk** — free space on the filesystem the server container sits on. Under a
  normal Docker setup that is the host filesystem the Postgres and object
  storage volumes live on, so it is the space that actually runs out. If you
  bind those volumes to a different disk, set `STATUS_DISK_PATH` to a path on
  that one. The page warns at 80% full and again at 92%.
- **Dependencies** — whether Postgres and the object store answer, how long
  they take, the PostgreSQL version, the size of the database on disk, and how
  many objects the bucket holds and their total size.
- **Totals** — accounts, vaults, bytes stored, uptime, and the server's memory
  use where the platform reports it.
- **Warnings** about the current configuration: sign-ups open with no storage
  limit, a reverse proxy whose client addresses are being ignored, a short
  admin token, a failed cleanup run.

The bucket total is normally larger than the accounts add up to, and the page
says so: every vault keeps a number of older versions so people can roll back,
and revoked shares sit there until the daily cleanup removes them.

| Setting | Default | What it does |
|---|---|---|
| Allow new sign-ups | on | Off refuses `POST /api/auth/register` and hides the sign-up form. Existing accounts are unaffected. |
| Storage per account | unlimited | Refuses an upload that would take the account past the cap. |
| Largest single file | unlimited | Refuses one file above the size, before it is uploaded. |
| Auth requests per address per minute | 30 | Applies to sign-in, sign-up and the salt lookup. 0 turns it off. |
| Failed sign-ins before lockout | 10 | Per username, then locked for the lockout length. 0 turns it off. |
| Lockout length | 15 minutes | How long that lasts. |

The environment variables `REGISTRATION_ENABLED`, `USER_STORAGE_LIMIT_BYTES`,
`MAX_ATTACHMENT_SIZE_BYTES` and `TRUST_PROXY_HEADERS` **seed** these values the
first time the server starts against an empty database, so a new deployment can
come up already closed. Once the settings row exists they are ignored — the
admin console is the authority, and there is no second place to look when the
two disagree. The effective values are printed at startup.

Two things worth knowing:

- **The storage cap counts what the database records**: attachments, share
  copies and their files. Map blobs live in object storage and their sizes are
  not recorded, so they fall outside the total. The version history of a map is
  bounded separately by that map's version limit.
- **The failed-sign-in lockout can be aimed at a user.** Anyone who knows a
  username can keep it locked by failing sign-ins on purpose. That is why the
  lockout expires on its own and why it is a separate switch from the
  per-address limit — leave the address limit on and set the lockout to 0 if
  that trade is not one you want.

### Client Addresses Behind A Proxy

Both throttles count per client address. Behind a reverse proxy, every request
arrives from the proxy, so without help the whole instance shares one
allowance and one busy user can throttle everyone.

`X-Forwarded-For` carries the real address, but it is a request header — anyone
can write anything in it, and trusting it on a directly-exposed server hands an
attacker an unlimited supply of identities. So it is off by default, and you
turn it on once your proxy is in front:

1. Make sure the proxy **sets** `X-Forwarded-For` rather than passing through
   whatever the client sent. (`proxy_set_header X-Forwarded-For $remote_addr;`
   in nginx — note `$remote_addr`, not `$proxy_add_x_forwarded_for`, which
   appends to a client-supplied value.)
2. Turn on **Read the client address from X-Forwarded-For** in the admin
   console.

The Settings screen shows the address your own request was attributed to. If
that reads as your proxy's address rather than your own, the setting is wrong
for this deployment.

### Inviting People When Sign-Ups Are Closed

Turning sign-ups off does not lock you out of adding people. Go to **People** in
the admin console, create an invite code, and send the person the link — the
code is filled in for them.

There is no "create a user" button, and there cannot be one. Registration is
zero-knowledge: the browser derives the account's keys from the password and
sends only public material, so the password never reaches the server. Nobody
but the new user can make their account. An invite is the server's way of
saying "this one person may".

A code is single-use, optionally expires, and can be revoked before it is used.
A sign-up that fails for another reason — a username already taken — hands the
code back rather than burning it. While sign-ups are open, codes are neither
required nor spent.

For the same reason, **you cannot reset anyone's password.** Their password is
what decrypts their vaults. If they lose it, that data is gone; the only path
forward is deleting the account and starting again.

### Protecting `/admin/`

The admin console authenticates with a single static bearer token
(`ADMIN_API_TOKEN`), which the browser keeps in `sessionStorage`. The token is
compared in constant time, so guessing it gains nothing from response timing,
but it does not rotate and there is no second factor.

Treat `/admin/` as an internal surface: restrict it at the proxy layer by
source address, or behind your own authentication, rather than leaving it
reachable from the public internet with only the token in front of it.

## Published Images

> Release notes link to [mindmapvault.com/homelab](https://www.mindmapvault.com/homelab/),
> which is the evergreen page for self-hosters. Keep that link in the notes for
> each release; write a blog post only when a release has something worth
> explaining, and link it from the hub rather than tying a post to every version.


Every release publishes the same image to two registries, with identical
digests. Use whichever you prefer:

| Registry | Image |
| --- | --- |
| Docker Hub | `kornelko2/mindmapvault-server` |
| GHCR | `ghcr.io/mindmapvault/mindmapvault-server` |

Docker Hub is the default: it is what `docker-compose.yml` and the guided
installer in `scripts/publish_dockerhub/` pull. GHCR needs no account to pull
and is convenient if you already authenticate to GitHub.

Both are multi-arch, `linux/amd64` and `linux/arm64`, each built on a runner of
its own architecture rather than under emulation.

### What The Tags Mean

| Tag | Written when | Use it for |
| --- | --- | --- |
| `vX.Y.Z` | a `v*` git tag is pushed | production — pin this |
| `latest` | a `v*` git tag is pushed | trying it out; tracks the newest **release** |
| `sha-<commit>` | any push to `main` | reproducing a specific commit |

`latest` deliberately does not follow `main`. A push to the default branch
publishes only its `sha-` tag, so `latest` always means "newest release" rather
than "newest commit".

### How Publishing Works

`.github/workflows/build-server-image.yml` builds each architecture on a native
runner and pushes it to GHCR by digest, then a second job joins the digests
into one multi-arch manifest and pushes that manifest to both registries. The
architectures are therefore built and uploaded once, not once per registry, and
the two registries cannot drift apart. The job then inspects every tag it wrote
and fails if one is missing an architecture.

Docker Hub publishing needs three repository secrets:

| Secret | Purpose |
| --- | --- |
| `DOCKERHUB_NAMESPACE` | the user or org the image lives under |
| `DOCKERHUB_USERNAME` | account used to log in |
| `DOCKERHUB_TOKEN` | an access token with write permission |

If `DOCKERHUB_NAMESPACE` is unset — as in a fork — the workflow logs a notice
and publishes to GHCR alone rather than failing.

### Republishing A Release

To publish an existing release to a registry it did not reach, run the workflow
manually and give it the release tag:

```bash
gh workflow run build-server-image.yml --ref main -f release_tag=v0.3.33
```

The tag has to match the version in `backend/Cargo.toml` on the ref you run
from; the job fails rather than publish one version's code under another's tag.
This exists so a release can be pushed to a newly-wired registry without moving
the git tag.

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

The compose file defaults to the Docker Hub image. To pin a version, or to use
GHCR instead, set `SERVER_IMAGE` before startup:

```bash
SERVER_IMAGE='kornelko2/mindmapvault-server:v0.3.33' docker compose up -d postgres garage server
SERVER_IMAGE='ghcr.io/mindmapvault/mindmapvault-server:v0.3.33' docker compose up -d postgres garage server
```

See [Published Images](#published-images) for what the tags mean.

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