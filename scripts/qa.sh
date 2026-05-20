#!/usr/bin/env bash
set -euo pipefail

PROJECT_VERSION_FALLBACK="0.1.13"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
MODE="quick"

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
Usage: scripts/qa.sh [--quick|--full|--version]

--quick    Version, uv lock, Python compile/unittest, shell syntax, cargo fmt.
--full     Quick gates plus cargo clippy, cargo test, and compose render.
EOF
}

while (($#)); do
  case "$1" in
    --quick)
      MODE="quick"
      shift
      ;;
    --full)
      MODE="full"
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

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    printf 'BLOCKED missing required command: %s\n' "${cmd}" >&2
    exit 1
  fi
}

run() {
  printf '+ %s\n' "$*"
  "$@"
}

require_cmd uv
require_cmd cargo
require_cmd find

run uv run --no-sync python "${REPO_DIR}/scripts/version.py" check
run uv lock --project "${REPO_DIR}" --check
run uv run --no-sync python -m py_compile \
  "${REPO_DIR}/scripts/bilibili_hlm_pipeline.py" \
  "${REPO_DIR}/scripts/extract_epub.py" \
  "${REPO_DIR}/scripts/download_wikisource.py" \
  "${REPO_DIR}/scripts/validate_source_snapshots.py" \
  "${REPO_DIR}/scripts/version.py"
run uv run --no-sync python -m compileall -q \
  "${REPO_DIR}/scripts" \
  "${REPO_DIR}/open-webui/functions" \
  "${REPO_DIR}/tests"
run uv run --no-sync python -m unittest discover \
  -s "${REPO_DIR}/tests" \
  -p 'test_*.py'
run uv run --no-sync python -m unittest discover \
  -s "${REPO_DIR}/open-webui/functions" \
  -p 'test_*.py'

while IFS= read -r -d '' script_path; do
  run bash -n "${script_path}"
done < <(
  find "${REPO_DIR}/scripts" "${REPO_DIR}/deploy/scripts" "${REPO_DIR}/agent-platform/scripts" \
    -type f \
    -name '*.sh' \
    -print0
)

if command -v shellcheck >/dev/null 2>&1; then
  if [[ "${TONGLINGYU_QA_SHELLCHECK:-false}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
    while IFS= read -r -d '' script_path; do
      run shellcheck "${script_path}"
    done < <(
      find "${REPO_DIR}/scripts" "${REPO_DIR}/deploy/scripts" "${REPO_DIR}/agent-platform/scripts" \
        -type f \
        -name '*.sh' \
        -print0
    )
  else
    printf 'SKIP shellcheck set TONGLINGYU_QA_SHELLCHECK=true to enable\n'
  fi
else
  printf 'SKIP shellcheck command not found\n'
fi

run cargo fmt --manifest-path "${REPO_DIR}/agent-platform/Cargo.toml" --all --check

if [[ "${MODE}" == "full" ]]; then
  printf '+ cargo metadata --manifest-path %s --format-version 1 >/dev/null\n' \
    "${REPO_DIR}/agent-platform/Cargo.toml"
  cargo metadata \
    --manifest-path "${REPO_DIR}/agent-platform/Cargo.toml" \
    --format-version 1 \
    >/dev/null
  run cargo clippy \
    --manifest-path "${REPO_DIR}/agent-platform/Cargo.toml" \
    --workspace \
    --all-targets \
    -- \
    -D warnings
  run cargo test \
    --manifest-path "${REPO_DIR}/agent-platform/Cargo.toml" \
    --workspace
  if command -v docker >/dev/null 2>&1; then
    (
      cd "${REPO_DIR}/deploy"
      run docker compose config --quiet
    )
  else
    printf 'SKIP docker compose config command docker not found\n'
  fi
fi
