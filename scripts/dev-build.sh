#!/usr/bin/env bash
# dev-build.sh
#
# Build a local Docker image from the current source tree and replace the
# running mindmapvault-server container with it.  Postgres and Garage are
# left untouched.
#
# The server is wired to a dedicated dev database (mmvdev / mmvdev) inside
# the already-running Postgres container so the production credentials in
# .env.deploy are never needed by the server container itself.
#
# Usage (from anywhere inside the repo, in WSL):
#   bash scripts/dev-build.sh             # build + restart server
#   bash scripts/dev-build.sh --no-cache  # force full Docker rebuild
#   bash scripts/dev-build.sh --down      # bring everything down cleanly

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="mindmapvault-server:local"
NO_CACHE=""
DO_DOWN=false

# ── Dev database credentials (only for local dev) ────────────────────────────
DEV_DB_USER="mmvdev"
DEV_DB_PASS="mmvdev"
DEV_DB_NAME="mmvdev"
PG_CONTAINER="mindmapvault-server-postgres"

for arg in "$@"; do
  case "$arg" in
    --no-cache) NO_CACHE="--no-cache" ;;
    --down)     DO_DOWN=true ;;
    *) echo "Unknown argument: $arg" >&2; exit 1 ;;
  esac
done

ENV_FILE="$REPO_ROOT/.env.deploy"
if [[ ! -f "$ENV_FILE" ]]; then
  echo "ERROR: $ENV_FILE not found. Create it from .env.deploy.example first." >&2
  exit 1
fi

# ── Ensure .env symlink exists so bare 'docker compose' commands work ─────────
ENV_LINK="$REPO_ROOT/.env"
if [[ ! -e "$ENV_LINK" ]]; then
  ln -sf "$ENV_FILE" "$ENV_LINK"
  echo "==> Created .env -> .env.deploy symlink (bare 'docker compose' commands will now work)"
fi

# ── --down: tear everything down and exit ─────────────────────────────────────
if $DO_DOWN; then
  echo "==> Bringing stack down..."
  cd "$REPO_ROOT"
  docker compose \
    -f docker-compose.yml \
    -f docker-compose.dev.yml \
    down
  echo "==> Done."
  exit 0
fi

echo "==> Repo root  : $REPO_ROOT"
echo "==> Image tag  : $IMAGE"
[[ -n "$NO_CACHE" ]] && echo "==> Cache      : disabled"
echo ""

# ── 1. Build ──────────────────────────────────────────────────────────────────
echo "==> Building Docker image..."
docker build \
  $NO_CACHE \
  --file "$REPO_ROOT/backend/Dockerfile" \
  --tag  "$IMAGE" \
  "$REPO_ROOT"

echo ""
echo "==> Build complete: $IMAGE"
echo ""

# ── 2. Ensure dev database and user exist in the running Postgres ─────────────
# docker exec -u postgres uses unix socket (peer auth) — no password needed.
echo "==> Ensuring dev database '$DEV_DB_NAME' and user '$DEV_DB_USER' in $PG_CONTAINER..."

docker exec -u postgres "$PG_CONTAINER" psql -v ON_ERROR_STOP=0 -U postgres <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '${DEV_DB_USER}') THEN
    CREATE USER ${DEV_DB_USER} WITH PASSWORD '${DEV_DB_PASS}';
    RAISE NOTICE 'Created user ${DEV_DB_USER}';
  ELSE
    ALTER USER ${DEV_DB_USER} WITH PASSWORD '${DEV_DB_PASS}';
    RAISE NOTICE 'User ${DEV_DB_USER} already exists — password reset';
  END IF;
END
\$\$;
SQL

docker exec -u postgres "$PG_CONTAINER" \
  psql -U postgres -tc "SELECT 1 FROM pg_database WHERE datname='${DEV_DB_NAME}'" \
  | grep -q 1 \
  || docker exec -u postgres "$PG_CONTAINER" \
       createdb -U postgres -O "${DEV_DB_USER}" "${DEV_DB_NAME}"

docker exec -u postgres "$PG_CONTAINER" \
  psql -U postgres -d "${DEV_DB_NAME}" \
  -c "GRANT ALL PRIVILEGES ON DATABASE ${DEV_DB_NAME} TO ${DEV_DB_USER};"

echo "==> Dev database ready."
echo ""

# ── 3. Hot-swap the server container ─────────────────────────────────────────
echo "==> Restarting mindmapvault-server container (dev DB: $DEV_DB_NAME)..."
cd "$REPO_ROOT"
SERVER_IMAGE="$IMAGE" \
  docker compose \
    -f docker-compose.yml \
    -f docker-compose.dev.yml \
    up -d --no-deps --force-recreate server

echo ""
echo "==> Done."
echo "    DB user  : $DEV_DB_USER"
echo "    DB name  : $DEV_DB_NAME"
echo "    Password : $DEV_DB_PASS"
echo ""
echo "==> Tailing logs (Ctrl-C to stop)..."
echo ""
docker logs -f --tail 60 mindmapvault-server
