#!/usr/bin/env bash
set -euo pipefail

PROJECT_VERSION_FALLBACK="0.1.6"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

QA_MODE="${TONGLINGYU_DEPLOY_QA_MODE:-quick}"
SKIP_QA="${TONGLINGYU_DEPLOY_SKIP_QA:-false}"

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
Usage: deploy/scripts/deploy-versioned-stack.sh [--version]

Bumps the patch version, runs QA, then builds and starts the compose stack.
Set TONGLINGYU_DEPLOY_QA_MODE=full for release-grade local gates.
EOF
}

case "${1:-}" in
  --version)
    project_version
    exit 0
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  "")
    ;;
  *)
    printf 'unknown argument: %s\n' "$1" >&2
    usage >&2
    exit 2
    ;;
esac

"${SCRIPT_DIR}/bump-deploy-version.sh"
if [[ ! "${SKIP_QA}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
  "${REPO_DIR}/scripts/qa.sh" "--${QA_MODE}"
fi

cd "${DEPLOY_DIR}"
docker compose build tonglingyu-gateway
docker compose pull
docker compose up -d
docker compose ps
