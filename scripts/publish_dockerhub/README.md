# MindMapVault Server on Docker Hub

Self-host MindMapVault Server with a guided installer. No local build step required.

- Repository: https://github.com/mindmapvault/mindmapvault-server
- Project page: https://www.mindmapvault.com/
- Download overview: https://www.mindmapvault.com/download/
- Docker Hub image: `kornelko2/mindmapvault-server`

---

## Beginner install (recommended)

Run this in a dedicated deployment folder (not in a source code checkout):

```bash
mkdir -p ~/mindmapvault-deploy
cd ~/mindmapvault-deploy
curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/scripts/publish_dockerhub/setup.sh -o setup.sh
bash setup.sh
```

What the installer does:

- downloads `docker-compose.yml`, `.env.deploy.example`, and `garage.toml`
- creates or updates `.env.deploy`
- guides you step by step (`install` or `update` mode)
- generates strong secret defaults when needed
- can pull images and deploy all services
- waits for service readiness and shows progress in terminal

After a successful run:

- App: http://localhost:8090/
- Admin: http://localhost:8090/admin/

Stop the stack:

```bash
docker compose --env-file .env.deploy down
```

---

## Prompt guide

Key prompts explained:

- `Mode [install/update]`
  - `install` for first deployment
  - `update` for upgrading or changing config
- Secret prompt when a value already exists
  - press Enter = keep current value
  - type `gen` = generate a fresh secure value
  - paste text = set your own value
- Secret prompt when value is missing
  - press Enter = auto-generate
  - paste text = set your own value

---

## Upgrade flow

Use the same installer again:

```bash
cd ~/mindmapvault-deploy
bash setup.sh
```

Choose `update` mode when prompted.

If you want to pin a specific image tag, set it when asked for `SERVER_IMAGE`, for example:

```text
kornelko2/mindmapvault-server:1.2.4
```

---

## If something looks wrong

If you see an odd `SERVER_IMAGE` prompt or a broken env value, reset and rerun:

```bash
cd ~/mindmapvault-deploy
rm -f .env.deploy
bash setup.sh
```

Useful checks:

```bash
docker compose --env-file .env.deploy ps
docker compose --env-file .env.deploy logs -f server
docker compose --env-file .env.deploy config
```

---

## Advanced manual setup (without setup.sh)

If you prefer manual setup:

```bash
curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/scripts/publish_dockerhub/docker-compose.yml -o docker-compose.yml
curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/scripts/publish_dockerhub/.env.deploy.example -o .env.deploy
curl -fsSL https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main/docker/garage.toml -o garage.toml
nano .env.deploy
docker compose --env-file .env.deploy up -d
```

Source-of-truth files:

- `scripts/publish_dockerhub/docker-compose.yml`
- `scripts/publish_dockerhub/.env.deploy.example`
- `docker/garage.toml`

---

## Data and security notes

- Runtime secrets belong in `.env.deploy`.
- Do not commit `.env.deploy` to git.
- Persistent volumes are:
  - `postgres-data`
  - `garage-meta`
  - `garage-data`
- Back up volumes before upgrades.

---

## Maintainer publishing helper

Maintainers can mirror GHCR tags to Docker Hub with:

```bash
./scripts/publish_dockerhub/publish-to-dockerhub.sh 1.2.3
```

---

## Issues and contributions

- Report bugs: https://github.com/mindmapvault/mindmapvault-server/issues/new
- Request features: https://github.com/mindmapvault/mindmapvault-server/issues/new
- Contribute code: https://github.com/mindmapvault/mindmapvault-server/pulls
