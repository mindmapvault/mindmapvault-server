#!/usr/bin/env bash

set -euo pipefail

RAW_BASE_URL="https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main"
DEFAULT_SERVER_IMAGE="kornelko2/mindmapvault-server:latest"
ENV_FILE=".env.deploy"
ENV_EXAMPLE_FILE=".env.deploy.example"
COMPOSE_FILE="docker-compose.yml"
GARAGE_FILE="garage.toml"

print_step() {
  echo
  echo "[$1] $2"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "ERROR: Missing required command: $1" >&2
    exit 1
  fi
}

compose_cmd() {
  if docker compose version >/dev/null 2>&1; then
    echo "docker compose"
    return
  fi
  if command -v docker-compose >/dev/null 2>&1; then
    echo "docker-compose"
    return
  fi
  echo ""
}

random_from_charset() {
  local length="$1"
  local charset="$2"
  local output=""
  local remaining
  local chunk

  while [[ "${#output}" -lt "${length}" ]]; do
    remaining="$((length - ${#output}))"
    chunk="$(LC_ALL=C tr -dc "${charset}" </dev/urandom | head -c "${remaining}" || true)"
    output+="${chunk}"
  done

  echo "${output}"
}

random_alnum() {
  local length="$1"
  random_from_charset "${length}" 'A-Za-z0-9'
}

random_uppernum() {
  local length="$1"
  random_from_charset "${length}" 'A-Z0-9'
}

upsert_env_value() {
  local key="$1"
  local value="$2"
  local file="$3"
  local tmp

  tmp="$(mktemp)"
  awk -v key="${key}" -v value="${value}" '
    BEGIN { found = 0 }
    $0 ~ ("^" key "=") {
      print key "=" value
      found = 1
      next
    }
    { print }
    END {
      if (!found) {
        print key "=" value
      }
    }
  ' "${file}" >"${tmp}"
  mv "${tmp}" "${file}"
}

ask_with_default() {
  local prompt="$1"
  local default_value="$2"
  local answer

  read -r -p "${prompt} [${default_value}]: " answer
  if [[ -z "${answer}" ]]; then
    echo "${default_value}"
  else
    echo "${answer}"
  fi
}

ask_secret_or_generate() {
  local name="$1"
  local generated_value="$2"
  local answer

  read -r -p "${name} (press Enter to auto-generate): " answer
  if [[ -z "${answer}" ]]; then
    echo "${generated_value}"
  else
    echo "${answer}"
  fi
}

download_file() {
  local source_url="$1"
  local target_path="$2"

  curl -fsSL "${source_url}" -o "${target_path}"
}

echo "MindMapVault Server setup"
echo "This script prepares docker-compose.yml, garage.toml, and ${ENV_FILE} in: $(pwd)"

print_step "1/6" "Checking prerequisites"
require_command curl
require_command docker

COMPOSE_CMD="$(compose_cmd)"
if [[ -z "${COMPOSE_CMD}" ]]; then
  echo "ERROR: Docker Compose not found. Install Docker Compose plugin or docker-compose." >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "ERROR: Docker daemon is not reachable. Start Docker and retry." >&2
  exit 1
fi

print_step "2/6" "Downloading deployment files"
download_file "${RAW_BASE_URL}/scripts/publish_dockerhub/${COMPOSE_FILE}" "${COMPOSE_FILE}"
download_file "${RAW_BASE_URL}/scripts/publish_dockerhub/${ENV_EXAMPLE_FILE}" "${ENV_EXAMPLE_FILE}"
download_file "${RAW_BASE_URL}/docker/${GARAGE_FILE}" "${GARAGE_FILE}"

if [[ ! -f "${ENV_FILE}" ]]; then
  cp "${ENV_EXAMPLE_FILE}" "${ENV_FILE}"
  chmod 600 "${ENV_FILE}" 2>/dev/null || true
fi

print_step "3/6" "Configuring image and required secrets"
SERVER_IMAGE="$(ask_with_default "SERVER_IMAGE" "${DEFAULT_SERVER_IMAGE}")"
POSTGRES_PASSWORD="$(ask_secret_or_generate "POSTGRES_PASSWORD" "$(random_alnum 40)")"
S3_ACCESS_KEY="$(ask_secret_or_generate "S3_ACCESS_KEY" "$(random_uppernum 20)")"
S3_SECRET_KEY="$(ask_secret_or_generate "S3_SECRET_KEY" "$(random_alnum 48)")"
JWT_SECRET="$(ask_secret_or_generate "JWT_SECRET" "$(random_alnum 64)")"
ADMIN_API_TOKEN="$(ask_secret_or_generate "ADMIN_API_TOKEN" "$(random_alnum 64)")"

upsert_env_value "SERVER_IMAGE" "${SERVER_IMAGE}" "${ENV_FILE}"
upsert_env_value "POSTGRES_PASSWORD" "${POSTGRES_PASSWORD}" "${ENV_FILE}"
upsert_env_value "S3_ACCESS_KEY" "${S3_ACCESS_KEY}" "${ENV_FILE}"
upsert_env_value "S3_SECRET_KEY" "${S3_SECRET_KEY}" "${ENV_FILE}"
upsert_env_value "JWT_SECRET" "${JWT_SECRET}" "${ENV_FILE}"
upsert_env_value "ADMIN_API_TOKEN" "${ADMIN_API_TOKEN}" "${ENV_FILE}"

print_step "4/6" "Optional runtime overrides"
GARAGE_BUCKET="$(ask_with_default "GARAGE_BUCKET" "mindmapvault")"
S3_BUCKET="$(ask_with_default "S3_BUCKET" "mindmapvault")"
S3_BASE_URL="$(ask_with_default "S3_BASE_URL" "http://localhost:9000")"
S3_REGION="$(ask_with_default "S3_REGION" "garage")"
JWT_ACCESS_EXPIRY_SECS="$(ask_with_default "JWT_ACCESS_EXPIRY_SECS" "900")"
JWT_REFRESH_EXPIRY_SECS="$(ask_with_default "JWT_REFRESH_EXPIRY_SECS" "2592000")"
DEFAULT_CORS="http://localhost:5173,http://localhost:1420,http://tauri.localhost,https://tauri.localhost"
CORS_ALLOWED_ORIGINS="$(ask_with_default "CORS_ALLOWED_ORIGINS" "${DEFAULT_CORS}")"
RUST_LOG="$(ask_with_default "RUST_LOG" "backend=debug,tower_http=info")"

upsert_env_value "GARAGE_BUCKET" "${GARAGE_BUCKET}" "${ENV_FILE}"
upsert_env_value "S3_BUCKET" "${S3_BUCKET}" "${ENV_FILE}"
upsert_env_value "S3_BASE_URL" "${S3_BASE_URL}" "${ENV_FILE}"
upsert_env_value "S3_REGION" "${S3_REGION}" "${ENV_FILE}"
upsert_env_value "JWT_ACCESS_EXPIRY_SECS" "${JWT_ACCESS_EXPIRY_SECS}" "${ENV_FILE}"
upsert_env_value "JWT_REFRESH_EXPIRY_SECS" "${JWT_REFRESH_EXPIRY_SECS}" "${ENV_FILE}"
upsert_env_value "CORS_ALLOWED_ORIGINS" "${CORS_ALLOWED_ORIGINS}" "${ENV_FILE}"
upsert_env_value "RUST_LOG" "${RUST_LOG}" "${ENV_FILE}"

print_step "5/6" "Configuration saved"
echo "Saved deployment config to ${ENV_FILE}"
echo "File permissions were set to 600 when supported."

print_step "6/6" "Start containers"
read -r -p "Start MindMapVault Server now? [Y/n]: " start_now
if [[ -z "${start_now}" || "${start_now}" =~ ^[Yy]$ ]]; then
  ${COMPOSE_CMD} --env-file "${ENV_FILE}" up -d
  echo
  echo "MindMapVault Server is starting."
  echo "App:   http://localhost:8090/"
  echo "Admin: http://localhost:8090/admin/"
  echo
  echo "To stop: ${COMPOSE_CMD} --env-file ${ENV_FILE} down"
else
  echo
  echo "Setup complete. Start later with:"
  echo "${COMPOSE_CMD} --env-file ${ENV_FILE} up -d"
fi
