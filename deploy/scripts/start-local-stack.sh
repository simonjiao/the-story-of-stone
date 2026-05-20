#!/usr/bin/env bash
set -euo pipefail

PROJECT_VERSION_FALLBACK="0.1.13"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"

BUILD_GATEWAY="true"
PULL_IMAGES="false"
DETACH="true"

project_version() {
  if [[ -f "${REPO_DIR}/VERSION" ]]; then
    local version
    version="$(tr -d '[:space:]' <"${REPO_DIR}/VERSION")"
    printf '%s\n' "${version}"
  else
    printf '%s\n' "${PROJECT_VERSION_FALLBACK}"
  fi
}

usage() {
  cat <<'EOF'
Usage: deploy/scripts/start-local-stack.sh [--no-build] [--pull] [--foreground] [--version]

Starts the local Tonglingyu compose stack without changing the project version.
Set TONGLINGYU_DEPLOY_ENV_FILE to use an env file outside deploy/.env.
EOF
}

while (($#)); do
  case "$1" in
    --no-build)
      BUILD_GATEWAY="false"
      shift
      ;;
    --pull)
      PULL_IMAGES="true"
      shift
      ;;
    --foreground)
      DETACH="false"
      shift
      ;;
    --version)
      project_version
      exit 0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

cd "${DEPLOY_DIR}"
load_deploy_env_file_or_local
export TONGLINGYU_VERSION="$(project_version)"

if [[ "${PULL_IMAGES}" == "true" ]]; then
  docker compose pull
fi

if [[ "${BUILD_GATEWAY}" == "true" ]]; then
  docker compose build tonglingyu-gateway
fi

if [[ "${DETACH}" == "true" ]]; then
  docker compose up -d
  docker compose ps
else
  docker compose up
fi
