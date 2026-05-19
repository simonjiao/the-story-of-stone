#!/usr/bin/env bash
set -euo pipefail

PROJECT_VERSION_FALLBACK="0.1.12"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

project_version() {
  if [[ -f "${REPO_DIR}/VERSION" ]]; then
    local version
    version="$(tr -d '[:space:]' <"${REPO_DIR}/VERSION")"
    printf '%s\n' "${version}"
  else
    printf '%s\n' "${PROJECT_VERSION_FALLBACK}"
  fi
}

if [[ "${1:-}" == "--version" ]]; then
  project_version
  exit 0
fi

if (($#)); then
  printf 'usage: deploy/scripts/bump-deploy-version.sh [--version]\n' >&2
  exit 2
fi

if ! command -v uv >/dev/null 2>&1; then
  printf 'BLOCKED missing required command: uv\n' >&2
  exit 1
fi

new_version="$(
  uv run --no-sync python "${REPO_DIR}/scripts/version.py" bump patch
)"
(
  cd "${REPO_DIR}"
  uv lock
)
(
  cd "${REPO_DIR}/agent-platform"
  cargo metadata --format-version 1 >/dev/null
)
uv run --no-sync python "${REPO_DIR}/scripts/version.py" check >/dev/null
printf 'deploy_version=%s\n' "${new_version}"
