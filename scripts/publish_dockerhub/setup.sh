#!/usr/bin/env bash

set -euo pipefail

RAW_BASE_URL="https://raw.githubusercontent.com/mindmapvault/mindmapvault-server/main"
DEFAULT_SERVER_IMAGE="kornelko2/mindmapvault-server:latest"
ENV_FILE=".env.deploy"
ENV_EXAMPLE_FILE=".env.deploy.example"
COMPOSE_FILE="docker-compose.yml"
GARAGE_FILE="garage.toml"

if [[ -t 1 ]] && command -v tput >/dev/null 2>&1 && [[ "$(tput colors 2>/dev/null || echo 0)" -ge 8 ]]; then
  COLOR_RESET="$(tput sgr0)"
  COLOR_BOLD="$(tput bold)"
  COLOR_BLUE="$(tput setaf 4)"
  COLOR_GREEN="$(tput setaf 2)"
  COLOR_YELLOW="$(tput setaf 3)"
  COLOR_RED="$(tput setaf 1)"
  COLOR_CYAN="$(tput setaf 6)"
else
  COLOR_RESET=""
  COLOR_BOLD=""
  COLOR_BLUE=""
  COLOR_GREEN=""
  COLOR_YELLOW=""
  COLOR_RED=""
  COLOR_CYAN=""
fi

draw_line() {
  printf "%s\n" "────────────────────────────────────────────────────────────"
  if [[ "${MMV_PROMPT_STDIN:-0}" != "1" && ! -t 1 && -w /dev/tty ]]; then
    printf "%s\n" "────────────────────────────────────────────────────────────" >/dev/tty
  fi
}

show_banner() {
  echo ""
  echo "${COLOR_CYAN}${COLOR_BOLD}╔══════════════════════════════════════════════════════════╗${COLOR_RESET}"
  echo "${COLOR_CYAN}${COLOR_BOLD}║               MindMapVault Server Installer             ║${COLOR_RESET}"
  echo "${COLOR_CYAN}${COLOR_BOLD}║                 Guided Deploy Wizard                    ║${COLOR_RESET}"
  echo "${COLOR_CYAN}${COLOR_BOLD}╚══════════════════════════════════════════════════════════╝${COLOR_RESET}"

  if [[ "${MMV_PROMPT_STDIN:-0}" != "1" && ! -t 1 && -w /dev/tty ]]; then
    echo "" >/dev/tty
    echo "${COLOR_CYAN}${COLOR_BOLD}╔══════════════════════════════════════════════════════════╗${COLOR_RESET}" >/dev/tty
    echo "${COLOR_CYAN}${COLOR_BOLD}║               MindMapVault Server Installer             ║${COLOR_RESET}" >/dev/tty
    echo "${COLOR_CYAN}${COLOR_BOLD}║                 Guided Deploy Wizard                    ║${COLOR_RESET}" >/dev/tty
    echo "${COLOR_CYAN}${COLOR_BOLD}╚══════════════════════════════════════════════════════════╝${COLOR_RESET}" >/dev/tty
  fi
}

log_step() {
  echo
  echo "${COLOR_BLUE}${COLOR_BOLD}▶ Step $1${COLOR_RESET}  $2"
  draw_line
}

log_info() {
  echo "${COLOR_CYAN}ℹ${COLOR_RESET}  $1"
}

log_success() {
  echo "${COLOR_GREEN}✓${COLOR_RESET}  $1"
}

log_warn() {
  echo "${COLOR_YELLOW}⚠${COLOR_RESET}  $1"
}

log_error() {
  echo "${COLOR_RED}✖${COLOR_RESET}  $1" >&2
}

prompt_input() {
  local prompt="$1"
  local answer=""

  if [[ "${MMV_PROMPT_STDIN:-0}" == "1" ]]; then
    read -r -p "${prompt}" answer || answer=""
  elif [[ -r /dev/tty ]]; then
    printf "%s" "${prompt}" >/dev/tty
    IFS= read -r answer </dev/tty || answer=""
  else
    read -r -p "${prompt}" answer || answer=""
  fi

  echo "${answer}"
}

ask_yes_no() {
  local prompt="$1"
  local default_choice="$2"
  local answer=""
  local suffix="[y/N]"
  local normalized_default="n"

  case "${default_choice}" in
    Y|y|yes|YES)
      suffix="[Y/n]"
      normalized_default="y"
      ;;
    *)
      suffix="[y/N]"
      normalized_default="n"
      ;;
  esac

  answer="$(prompt_input "${prompt} ${suffix}: ")"
  answer="${answer,,}"
  if [[ -z "${answer}" ]]; then
    answer="${normalized_default}"
  fi

  if [[ "${answer}" =~ ^y(es)?$ ]]; then
    echo "yes"
  else
    echo "no"
  fi
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log_error "Missing required command: $1"
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

get_env_value() {
  local key="$1"
  local file="$2"

  if [[ ! -f "${file}" ]]; then
    echo ""
    return
  fi

  awk -F '=' -v key="${key}" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "${file}" | tr -d '\r'
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

  answer="$(prompt_input "${prompt} [${default_value}]: ")"
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

  answer="$(prompt_input "${name}: Enter=auto-generate, or paste your own value: ")"
  if [[ -z "${answer}" ]]; then
    echo "${generated_value}"
  else
    echo "${answer}"
  fi
}

ask_secret_with_existing() {
  local name="$1"
  local generated_value="$2"
  local existing_value="$3"
  local answer=""

  if [[ -n "${existing_value}" ]]; then
    answer="$(prompt_input "${name}: Enter=keep current, gen=generate new, or paste new value: ")"
    if [[ -z "${answer}" ]]; then
      echo "${existing_value}"
    elif [[ "${answer,,}" == "gen" ]]; then
      echo "${generated_value}"
    else
      echo "${answer}"
    fi
    return
  fi

  echo "$(ask_secret_or_generate "${name}" "${generated_value}")"
}

download_file() {
  local source_url="$1"
  local target_path="$2"

  curl -fsSL "${source_url}" -o "${target_path}"
}

wait_for_container_state() {
  local container_name="$1"
  local expected_state="$2"
  local timeout_secs="$3"
  local elapsed=0
  local state=""

  log_info "Waiting for ${container_name} to become ${expected_state} (timeout ${timeout_secs}s)"
  while (( elapsed < timeout_secs )); do
    state="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "${container_name}" 2>/dev/null || true)"
    if [[ "${state}" == "${expected_state}" ]]; then
      log_success "${container_name} is ${expected_state}"
      return 0
    fi

    printf "." >&2
    sleep 2
    elapsed=$((elapsed + 2))
  done

  echo >&2
  log_warn "${container_name} did not reach '${expected_state}'. Last observed state: ${state:-unknown}"
  return 1
}

load_default() {
  local key="$1"
  local fallback="$2"
  local value

  value="$(get_env_value "${key}" "${ENV_FILE}")"
  if [[ -n "${value}" ]]; then
    echo "${value}"
  else
    echo "${fallback}"
  fi
}

show_banner
echo "This script prepares docker-compose.yml, garage.toml, and ${ENV_FILE} in: $(pwd)"

ENV_FILE_EXISTED_AT_START=0
if [[ -f "${ENV_FILE}" ]]; then
  ENV_FILE_EXISTED_AT_START=1
fi

# Show this warning only when we detect the actual MindMapVault source checkout root.
if [[ ( -d .git || -f .git ) && -f "backend/src/main.rs" && -d "scripts/publish_dockerhub" ]]; then
  log_warn "Detected MindMapVault source repository root."
  log_warn "Best practice: keep deployment secrets/config outside the source checkout."
  continue_here="$(ask_yes_no "Use this source folder for deployment files anyway (advanced)?" "N")"
  if [[ "${continue_here}" != "yes" ]]; then
    echo ""
    log_info "Aborted by user."
    echo "Try: mkdir -p ~/mindmapvault-deploy && cd ~/mindmapvault-deploy"
    exit 1
  fi
fi

log_step "1/7" "Checking prerequisites"
require_command curl
require_command docker

COMPOSE_CMD="$(compose_cmd)"
if [[ -z "${COMPOSE_CMD}" ]]; then
  log_error "Docker Compose not found. Install Docker Compose plugin or docker-compose."
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  log_error "Docker daemon is not reachable. Start Docker and retry."
  exit 1
fi
log_success "Docker and Compose are available"

log_step "2/7" "Downloading deployment files"
log_info "Downloading ${COMPOSE_FILE}"
download_file "${RAW_BASE_URL}/scripts/publish_dockerhub/${COMPOSE_FILE}" "${COMPOSE_FILE}"
log_info "Downloading ${ENV_EXAMPLE_FILE}"
download_file "${RAW_BASE_URL}/scripts/publish_dockerhub/${ENV_EXAMPLE_FILE}" "${ENV_EXAMPLE_FILE}"
log_info "Downloading ${GARAGE_FILE}"
download_file "${RAW_BASE_URL}/docker/${GARAGE_FILE}" "${GARAGE_FILE}"
log_success "Deployment files downloaded"

if [[ ! -f "${ENV_FILE}" ]]; then
  cp "${ENV_EXAMPLE_FILE}" "${ENV_FILE}"
  chmod 600 "${ENV_FILE}" 2>/dev/null || true
  log_info "Created ${ENV_FILE} from template"
else
  log_info "Reusing existing ${ENV_FILE}"
fi

log_step "3/7" "Choose setup mode"
MODE_DEFAULT="install"
if [[ "${ENV_FILE_EXISTED_AT_START}" -eq 1 ]]; then
  MODE_DEFAULT="update"
fi
MODE_CHOICE="$(prompt_input "Mode [install/update] (${MODE_DEFAULT}): ")"
MODE_CHOICE="${MODE_CHOICE,,}"
if [[ -z "${MODE_CHOICE}" ]]; then
  MODE_CHOICE="${MODE_DEFAULT}"
fi
if [[ "${MODE_CHOICE}" != "install" && "${MODE_CHOICE}" != "update" ]]; then
  log_warn "Unknown mode '${MODE_CHOICE}', defaulting to ${MODE_DEFAULT}."
  MODE_CHOICE="${MODE_DEFAULT}"
fi
log_success "Selected mode: ${MODE_CHOICE}"

log_step "4/7" "Configuring image and required secrets"
SERVER_IMAGE="$(ask_with_default "SERVER_IMAGE" "$(load_default "SERVER_IMAGE" "${DEFAULT_SERVER_IMAGE}")")"

EXISTING_POSTGRES_PASSWORD="$(get_env_value "POSTGRES_PASSWORD" "${ENV_FILE}")"
EXISTING_S3_ACCESS_KEY="$(get_env_value "S3_ACCESS_KEY" "${ENV_FILE}")"
EXISTING_S3_SECRET_KEY="$(get_env_value "S3_SECRET_KEY" "${ENV_FILE}")"
EXISTING_JWT_SECRET="$(get_env_value "JWT_SECRET" "${ENV_FILE}")"
EXISTING_ADMIN_API_TOKEN="$(get_env_value "ADMIN_API_TOKEN" "${ENV_FILE}")"

POSTGRES_PASSWORD="$(ask_secret_with_existing "POSTGRES_PASSWORD" "$(random_alnum 40)" "${EXISTING_POSTGRES_PASSWORD}")"
S3_ACCESS_KEY="$(ask_secret_with_existing "S3_ACCESS_KEY" "$(random_uppernum 20)" "${EXISTING_S3_ACCESS_KEY}")"
S3_SECRET_KEY="$(ask_secret_with_existing "S3_SECRET_KEY" "$(random_alnum 48)" "${EXISTING_S3_SECRET_KEY}")"
JWT_SECRET="$(ask_secret_with_existing "JWT_SECRET" "$(random_alnum 64)" "${EXISTING_JWT_SECRET}")"
ADMIN_API_TOKEN="$(ask_secret_with_existing "ADMIN_API_TOKEN" "$(random_alnum 64)" "${EXISTING_ADMIN_API_TOKEN}")"

upsert_env_value "SERVER_IMAGE" "${SERVER_IMAGE}" "${ENV_FILE}"
upsert_env_value "POSTGRES_PASSWORD" "${POSTGRES_PASSWORD}" "${ENV_FILE}"
upsert_env_value "S3_ACCESS_KEY" "${S3_ACCESS_KEY}" "${ENV_FILE}"
upsert_env_value "S3_SECRET_KEY" "${S3_SECRET_KEY}" "${ENV_FILE}"
upsert_env_value "JWT_SECRET" "${JWT_SECRET}" "${ENV_FILE}"
upsert_env_value "ADMIN_API_TOKEN" "${ADMIN_API_TOKEN}" "${ENV_FILE}"

log_step "5/7" "Optional runtime overrides"
GARAGE_BUCKET="$(ask_with_default "GARAGE_BUCKET" "$(load_default "GARAGE_BUCKET" "mindmapvault")")"
S3_BUCKET="$(ask_with_default "S3_BUCKET" "$(load_default "S3_BUCKET" "mindmapvault")")"
S3_BASE_URL="$(ask_with_default "S3_BASE_URL" "$(load_default "S3_BASE_URL" "http://localhost:9000")")"
S3_REGION="$(ask_with_default "S3_REGION" "$(load_default "S3_REGION" "garage")")"
JWT_ACCESS_EXPIRY_SECS="$(ask_with_default "JWT_ACCESS_EXPIRY_SECS" "$(load_default "JWT_ACCESS_EXPIRY_SECS" "900")")"
JWT_REFRESH_EXPIRY_SECS="$(ask_with_default "JWT_REFRESH_EXPIRY_SECS" "$(load_default "JWT_REFRESH_EXPIRY_SECS" "2592000")")"
DEFAULT_CORS="http://localhost:5173,http://localhost:1420,http://tauri.localhost,https://tauri.localhost"
CORS_ALLOWED_ORIGINS="$(ask_with_default "CORS_ALLOWED_ORIGINS" "$(load_default "CORS_ALLOWED_ORIGINS" "${DEFAULT_CORS}")")"
RUST_LOG="$(ask_with_default "RUST_LOG" "$(load_default "RUST_LOG" "backend=debug,tower_http=info")")"

upsert_env_value "GARAGE_BUCKET" "${GARAGE_BUCKET}" "${ENV_FILE}"
upsert_env_value "S3_BUCKET" "${S3_BUCKET}" "${ENV_FILE}"
upsert_env_value "S3_BASE_URL" "${S3_BASE_URL}" "${ENV_FILE}"
upsert_env_value "S3_REGION" "${S3_REGION}" "${ENV_FILE}"
upsert_env_value "JWT_ACCESS_EXPIRY_SECS" "${JWT_ACCESS_EXPIRY_SECS}" "${ENV_FILE}"
upsert_env_value "JWT_REFRESH_EXPIRY_SECS" "${JWT_REFRESH_EXPIRY_SECS}" "${ENV_FILE}"
upsert_env_value "CORS_ALLOWED_ORIGINS" "${CORS_ALLOWED_ORIGINS}" "${ENV_FILE}"
upsert_env_value "RUST_LOG" "${RUST_LOG}" "${ENV_FILE}"

log_step "6/7" "Configuration saved"
log_success "Saved deployment config to ${ENV_FILE}"
log_info "File permissions were set to 600 when supported"

log_step "7/7" "Deploy or update containers"
deploy_now="$(ask_yes_no "Deploy now?" "Y")"
if [[ "${deploy_now}" == "yes" ]]; then
  update_images_default="N"
  if [[ "${MODE_CHOICE}" == "update" ]]; then
    update_images_default="Y"
  fi
  pull_choice="$(ask_yes_no "Pull latest images first (server/postgres/garage)?" "${update_images_default}")"

  if [[ "${pull_choice}" == "yes" ]]; then
    log_info "Pulling latest images. This can take a while on slower networks."
    ${COMPOSE_CMD} --env-file "${ENV_FILE}" pull server postgres garage
    log_success "Image pull completed"
  else
    log_info "Skipping image pull"
  fi

  log_info "Applying compose changes and (re)starting containers"
  ${COMPOSE_CMD} --env-file "${ENV_FILE}" up -d --remove-orphans

  wait_for_container_state "mindmapvault-server-postgres" "healthy" 120 || true
  wait_for_container_state "mindmapvault-server-garage" "healthy" 120 || true
  wait_for_container_state "mindmapvault-server" "running" 120 || true

  echo
  log_success "MindMapVault Server deployment command finished"
  echo "App:   http://localhost:8090/"
  echo "Admin: http://localhost:8090/admin/"
  echo
  echo "To stop: ${COMPOSE_CMD} --env-file ${ENV_FILE} down"
  echo "To update later: run this setup script again and choose mode 'update'."
else
  echo
  log_success "Setup complete"
  echo "Start later with:"
  echo "${COMPOSE_CMD} --env-file ${ENV_FILE} up -d"
fi
