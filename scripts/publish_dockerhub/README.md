# MindMapVault Server on Docker Hub

Self‑host MindMapVault Server with a single command. No build step required.

- Repository: https://github.com/mindmapvault/mindmapvault-server
- Project page: https://www.mindmapvault.com/
- Download & product overview: https://www.mindmapvault.com/download/

Docker Hub image: `kornelko2/mindmapvault-server`

---

## Run with one command

Run this in a new empty folder. It guides you step by step, generates strong secrets for required values, saves them into `.env.deploy`, and can start the stack immediately.

```bash
curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/scripts/publish_dockerhub/setup.sh | bash
```

What the setup script does:

- downloads `docker-compose.yml`, `.env.deploy.example`, and `garage.toml`
- creates or updates `.env.deploy`
- prompts for values one by one in interactive mode
- auto-generates secure defaults for required secrets when you press Enter
- uses colored output with explicit progress and waiting status messages
- supports both `install` and `update` mode
- in update mode can pull and refresh all three services (`server`, `postgres`, `garage`)
- optionally runs deployment and post-start health waiting checks

For upgrades, run the same command again and choose `update` mode.
The script preserves existing env values by default and lets you override or rotate values when needed.

Manual fallback (if you prefer editing the env file yourself):

```bash
curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/scripts/publish_dockerhub/docker-compose.yml -o docker-compose.yml \
	&& curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/scripts/publish_dockerhub/.env.deploy.example -o .env.deploy \
	&& curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/docker/garage.toml -o garage.toml \
	&& nano .env.deploy \
	&& docker compose --env-file .env.deploy up -d
```

Once the containers are healthy (first start takes ~15 seconds):

- App: http://localhost:8090/
- Admin: http://localhost:8090/admin/

To stop the stack:

```bash
docker compose --env-file .env.deploy down
```

---

## Folder file structure

This folder is the recommended local deployment workspace:

```text
scripts/publish_dockerhub/
	README.md              # deployment and publishing guide (this file)
	setup.sh               # interactive one-command installer
	docker-compose.yml     # runtime stack: server + postgres + garage
	.env.deploy.example    # deployment env template (copy to .env.deploy)
	.env.deploy            # deployment secrets (local, do not commit)
	.env                   # publish script credentials only (local, do not commit)
	publish-to-dockerhub.sh  # mirrors GHCR tags to Docker Hub
```

For proper deployment, keep `.env` and `.env.deploy` separate:

- `.env` only for Docker Hub publishing credentials.
- `.env.deploy` only for runtime/deployment secrets used by Docker Compose.

## Recommended quick start (with secrets file)

```bash
cd scripts/publish_dockerhub
cp .env.deploy.example .env.deploy
# edit .env.deploy and set strong secret values
docker compose --env-file .env.deploy up -d
```

Open:

- App: `http://localhost:8090/`
- Admin: `http://localhost:8090/admin/`

## Required containers (run in combo)

MindMapVault Server needs three services together:

- `server` (MindMapVault API + app/admin web surfaces)
- `postgres` (account and metadata persistence)
- `garage` (S3-compatible encrypted blob storage)

The compose example starts all three and wires health checks and dependencies.

## Image tags and pinning

Recommended production practice is to pin a version tag instead of `latest`.

```bash
cd scripts/publish_dockerhub
SERVER_IMAGE=kornelko2/mindmapvault-server:1.2.3 \
docker compose --env-file .env.deploy up -d
```

## Required secret setup

Before first run, copy `scripts/publish_dockerhub/.env.deploy.example` to `scripts/publish_dockerhub/.env.deploy` and set these required values:

- `POSTGRES_PASSWORD`
- `S3_ACCESS_KEY`
- `S3_SECRET_KEY`
- `JWT_SECRET`
- `ADMIN_API_TOKEN`

Example:

```dotenv
POSTGRES_PASSWORD=replace-with-strong-db-password
S3_ACCESS_KEY=replace-with-strong-s3-access-key
S3_SECRET_KEY=replace-with-strong-s3-secret-key
JWT_SECRET=replace-with-long-random-jwt-secret
ADMIN_API_TOKEN=replace-with-long-random-admin-token
```

The compose file enforces these values and will fail fast if they are missing.

Important:

- `.env` is reserved for `publish-to-dockerhub.sh` credentials only.
- Runtime/deployment secrets should be stored in `.env.deploy`.

## Complete `.env.deploy.example` content

Use this exact template as your deployment env file source:

```dotenv
# Docker Hub deployment environment variables
# Copy to .env.deploy and replace all placeholder values.

# Optional image pin. Leave latest for local testing.
SERVER_IMAGE=kornelko2/mindmapvault-server:latest

# Required secrets
POSTGRES_PASSWORD=replace-with-strong-db-password
S3_ACCESS_KEY=replace-with-strong-s3-access-key
S3_SECRET_KEY=replace-with-strong-s3-secret-key
JWT_SECRET=replace-with-long-random-jwt-secret
ADMIN_API_TOKEN=replace-with-long-random-admin-token

# Optional overrides
GARAGE_BUCKET=mindmapvault
S3_BUCKET=mindmapvault
S3_BASE_URL=http://localhost:9000
S3_REGION=garage
JWT_ACCESS_EXPIRY_SECS=900
JWT_REFRESH_EXPIRY_SECS=2592000
CORS_ALLOWED_ORIGINS=http://localhost:5173,http://localhost:1420,http://tauri.localhost,https://tauri.localhost
RUST_LOG=backend=debug,tower_http=info
```

## Complete `docker-compose.yml` content

Use this exact stack file for runtime deployment:

```yaml
services:
	postgres:
		image: postgres:16-bookworm
		container_name: mindmapvault-server-postgres
		restart: unless-stopped
		environment:
			POSTGRES_DB: mindmapvault
			POSTGRES_USER: postgres
			POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?Set POSTGRES_PASSWORD in scripts/publish_dockerhub/.env.deploy}
		ports:
			- "5432:5432"
		volumes:
			- postgres-data:/var/lib/postgresql/data
		healthcheck:
			test: ["CMD-SHELL", "pg_isready -U postgres -d mindmapvault"]
			interval: 10s
			timeout: 5s
			retries: 10

	garage:
		image: dxflrs/garage:v2.3.0
		container_name: mindmapvault-server-garage
		restart: unless-stopped
		command: /garage server --single-node --default-bucket
		environment:
			GARAGE_DEFAULT_ACCESS_KEY: ${S3_ACCESS_KEY:?Set S3_ACCESS_KEY in scripts/publish_dockerhub/.env.deploy}
			GARAGE_DEFAULT_SECRET_KEY: ${S3_SECRET_KEY:?Set S3_SECRET_KEY in scripts/publish_dockerhub/.env.deploy}
			GARAGE_DEFAULT_BUCKET: ${GARAGE_BUCKET:-mindmapvault}
		ports:
			- "9000:3900"
			- "3901:3901"
			- "3903:3903"
		volumes:
			- ../../docker/garage.toml:/etc/garage.toml:ro
			- garage-meta:/var/lib/garage/meta
			- garage-data:/var/lib/garage/data
		healthcheck:
			test: ["CMD", "/garage", "-c", "/etc/garage.toml", "status"]
			interval: 10s
			timeout: 5s
			retries: 10

	server:
		image: ${SERVER_IMAGE:-kornelko2/mindmapvault-server:latest}
		container_name: mindmapvault-server
		restart: unless-stopped
		depends_on:
			postgres:
				condition: service_healthy
			garage:
				condition: service_healthy
		environment:
			HOST: 0.0.0.0
			PORT: 8090
			DB_ENGINE: postgres

			POSTGRES_USER: postgres
			POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?Set POSTGRES_PASSWORD in scripts/publish_dockerhub/.env.deploy}
			POSTGRES_DB: mindmapvault
			POSTGRES_PORT: 5432
			POSTGRES_DSN: postgresql://postgres:${POSTGRES_PASSWORD:?Set POSTGRES_PASSWORD in scripts/publish_dockerhub/.env.deploy}@postgres:5432/mindmapvault

			S3_ENDPOINT: http://garage:3900
			S3_PUBLIC_ENDPOINT: http://localhost:9000
			S3_ACCESS_KEY: ${S3_ACCESS_KEY:?Set S3_ACCESS_KEY in scripts/publish_dockerhub/.env.deploy}
			S3_SECRET_KEY: ${S3_SECRET_KEY:?Set S3_SECRET_KEY in scripts/publish_dockerhub/.env.deploy}
			S3_BUCKET: ${S3_BUCKET:-mindmapvault}
			S3_BASE_URL: ${S3_BASE_URL:-http://localhost:9000}
			S3_REGION: ${S3_REGION:-garage}
			S3_PRESIGN_EXPIRY_SECS: ${S3_PRESIGN_EXPIRY_SECS:-3600}

			JWT_SECRET: ${JWT_SECRET:?Set JWT_SECRET in scripts/publish_dockerhub/.env.deploy}
			JWT_ACCESS_EXPIRY_SECS: ${JWT_ACCESS_EXPIRY_SECS:-900}
			JWT_REFRESH_EXPIRY_SECS: ${JWT_REFRESH_EXPIRY_SECS:-2592000}
			ADMIN_API_TOKEN: ${ADMIN_API_TOKEN:?Set ADMIN_API_TOKEN in scripts/publish_dockerhub/.env.deploy}

			CORS_ALLOWED_ORIGINS: ${CORS_ALLOWED_ORIGINS:-http://localhost:5173,http://localhost:1420,http://tauri.localhost,https://tauri.localhost}
			RUST_LOG: ${RUST_LOG:-backend=debug,tower_http=info}
		ports:
			- "8090:8090"

volumes:
	postgres-data:
	garage-meta:
	garage-data:
```

## Runtime configuration

Most important variables used by the compose file:

- `SERVER_IMAGE` sets which Docker Hub tag to run
- `JWT_SECRET` must be unique per environment
- `ADMIN_API_TOKEN` must be unique per environment
- `S3_ACCESS_KEY` and `S3_SECRET_KEY` are used by both Garage and the server
- `POSTGRES_PASSWORD` is required for both Postgres and server DB connection

For production-like deployments, set strong secrets and avoid default sample values.

## Data persistence and backups

The stack persists data in named volumes:

- `postgres-data`
- `garage-meta`
- `garage-data`

Back up these volumes before upgrades and periodically during normal operations.

## Networking and exposure

Default host ports in the example:

- `8090` for the server
- `9000` for the Garage S3 endpoint

Be careful when exposing database or storage ports outside trusted networks.

## Upgrade workflow

1. Back up persistent volumes.
2. Pull the new image tag.
3. Restart with the pinned `SERVER_IMAGE` tag.
4. Verify health and app access.

Example:

```bash
cd scripts/publish_dockerhub
docker pull kornelko2/mindmapvault-server:1.2.4
SERVER_IMAGE=kornelko2/mindmapvault-server:1.2.4 \
docker compose --env-file .env.deploy up -d
```

## Changelog and release notes

Best practice is to review release notes before pulling a new tag.

- Internal engineering changelog: `CHANGELOG.md`
- Customer-facing changelog: `CUSTOMER_CHANGELOG.md`

Use these files to decide when to roll forward and when to hold a version.

## Troubleshooting

- Check container status:

```bash
cd scripts/publish_dockerhub
docker compose --env-file .env.deploy ps
```

- Check server logs:

```bash
cd scripts/publish_dockerhub
docker compose --env-file .env.deploy logs -f server
```

- Validate rendered compose config:

```bash
cd scripts/publish_dockerhub
docker compose --env-file .env.deploy config
```

## Maintainer publishing helper

Maintainers can publish Docker Hub tags from GHCR with:

```bash
./scripts/publish_dockerhub/publish-to-dockerhub.sh 1.2.3
```

## Feature requests, bug reports, and contributions

If you want to request a feature, report a bug, or contribute a fix, please use the GitHub repository:

- Report bugs: https://github.com/mindmapvault/mindmapvault-server/issues/new
- Request features: https://github.com/mindmapvault/mindmapvault-server/issues/new
- Contribute code: https://github.com/mindmapvault/mindmapvault-server/pulls

Please include clear reproduction steps, expected behavior, and environment details when opening an issue.
