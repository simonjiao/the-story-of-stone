#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

DEPLOY_ENV_FILE_PATH="${TONGLINGYU_DEPLOY_ENV_FILE:-${DEPLOY_ENV_FILE:-}}"
if [[ -z "${DEPLOY_ENV_FILE_PATH}" && -f "${REPO_DIR}/.env" ]]; then
  DEPLOY_ENV_FILE_PATH="${REPO_DIR}/.env"
fi
if [[ -n "${DEPLOY_ENV_FILE_PATH}" && -f "${DEPLOY_ENV_FILE_PATH}" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "${DEPLOY_ENV_FILE_PATH}" >/dev/null 2>&1
  set +a
fi

DEFAULT_SOURCE_DB_PATH="$(
  python3 - "${REPO_DIR}" <<'PY'
import os
import sys
from pathlib import Path

repo_dir = Path(sys.argv[1])
data_dir = Path(os.environ.get("TONGLINGYU_DATA_DIR") or "data/tonglingyu")
if not data_dir.is_absolute():
    data_dir = (repo_dir / data_dir).resolve()
db_path = Path(os.environ.get("TONGLINGYU_DB_PATH") or "tonglingyu.db")
if db_path.is_absolute():
    print(data_dir / db_path.name)
else:
    print((repo_dir / db_path).resolve())
PY
)"
DEFAULT_ARTIFACT_ROOT="$(dirname "${DEFAULT_SOURCE_DB_PATH}")/restore-canaries"

SOURCE_DB_PATH="${TONGLINGYU_RQA_RESTORE_CANARY_DB_PATH:-${TONGLINGYU_RQA_DB_PATH:-${DEFAULT_SOURCE_DB_PATH}}}"
GATEWAY_BIN="${TONGLINGYU_RQA_RESTORE_CANARY_GATEWAY_BIN:-${TONGLINGYU_RQA_GATEWAY_BIN:-${REPO_DIR}/agent-platform/target/debug/tonglingyu-gateway}}"
SKIP_BUILD="${TONGLINGYU_RQA_RESTORE_CANARY_SKIP_BUILD:-false}"
REVIEWER="${TONGLINGYU_RQA_RESTORE_CANARY_REVIEWER:-restore-drill}"
REVIEW_NOTE="${TONGLINGYU_RQA_RESTORE_CANARY_REVIEW_NOTE:-closed restore drill canary}"
PACKAGE_ID="${TONGLINGYU_RQA_RESTORE_CANARY_PACKAGE_ID:-}"
ARTIFACT_ROOT="${TONGLINGYU_RQA_RESTORE_CANARY_ARTIFACT_ROOT:-${DEFAULT_ARTIFACT_ROOT}}"
RUN_ID="${TONGLINGYU_RQA_RESTORE_CANARY_RUN_ID:-$(date -u +"%Y%m%dT%H%M%SZ")-$$}"
ARTIFACT_DIR="${TONGLINGYU_RQA_RESTORE_CANARY_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
REPORT_PATH="${TONGLINGYU_RQA_RESTORE_CANARY_REPORT_PATH:-${ARTIFACT_DIR}/restore-canary-prepare.json}"
DOCKER_FALLBACK="${TONGLINGYU_RQA_RESTORE_CANARY_DOCKER_FALLBACK:-true}"
DOCKER_SERVICE="${TONGLINGYU_RQA_RESTORE_CANARY_DOCKER_SERVICE:-tonglingyu-gateway}"
CONTAINER_DB_PATH="${TONGLINGYU_RQA_RESTORE_CANARY_CONTAINER_DB_PATH:-${TONGLINGYU_DB_PATH:-/data/tonglingyu.db}}"
CONTAINER_GATEWAY_BIN="${TONGLINGYU_RQA_RESTORE_CANARY_CONTAINER_GATEWAY_BIN:-tonglingyu-gateway}"
CONTAINER_ARTIFACT_DIR="${TONGLINGYU_RQA_RESTORE_CANARY_CONTAINER_ARTIFACT_DIR:-/data/restore-canaries/${RUN_ID}}"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

mkdir -p "${ARTIFACT_DIR}"
ARTIFACT_DIR="$(cd -- "${ARTIFACT_DIR}" && pwd)"
BACKUP_DB="${ARTIFACT_DIR}/live-db-before-restore-canary.db"
CANARY_STDOUT="${ARTIFACT_DIR}/restore-canary.stdout"
CANARY_STDERR="${ARTIFACT_DIR}/restore-canary.stderr"
BACKUP_STDOUT="${ARTIFACT_DIR}/backup.stdout"
BACKUP_STDERR="${ARTIFACT_DIR}/backup.stderr"
RESTORE_REFS_ENV="${ARTIFACT_DIR}/restore-refs.env"
EXECUTION_MODE="host"

emit_report() {
  local status="$1"
  local error_code="${2:-}"
  python3 - "${REPORT_PATH}" "${status}" "${error_code}" "${ARTIFACT_DIR}" \
    "${BACKUP_DB}" "${CANARY_STDOUT}" "${CANARY_STDERR}" "${RESTORE_REFS_ENV}" \
    "${EXECUTION_MODE}" <<'PY'
import hashlib
import json
import shlex
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    status,
    error_code,
    artifact_dir_raw,
    backup_db_raw,
    canary_stdout_raw,
    canary_stderr_raw,
    refs_env_raw,
    execution_mode,
) = sys.argv[1:10]

def file_sha256(path):
    path = Path(path)
    if not path.is_file():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def parse_canary(path):
    path = Path(path)
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8").strip()
    if not text:
        return None
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None

artifact_dir = str(Path(artifact_dir_raw).resolve())
backup_db = Path(backup_db_raw)
canary = parse_canary(canary_stdout_raw)
if status == "ok" and canary:
    refs = canary.get("refs") or {}
    refs_env = Path(refs_env_raw)
    refs_env.write_text(
        "\n".join(
            [
                "export TONGLINGYU_RQA_RESTORE_DRILL_TRACE_ID="
                + shlex.quote(str(refs.get("trace_id", ""))),
                "export TONGLINGYU_RQA_RESTORE_DRILL_PACKAGE_ID="
                + shlex.quote(str(refs.get("package_id", ""))),
                "export TONGLINGYU_RQA_RESTORE_DRILL_FAILURE_ID="
                + shlex.quote(str(refs.get("failure_id", ""))),
                "export TONGLINGYU_RQA_RESTORE_DRILL_TASK_ID="
                + shlex.quote(str(refs.get("task_id", ""))),
            ]
        )
        + "\n",
        encoding="utf-8",
    )

payload = {
    "object": "tonglingyu.rqa_restore_canary_prepare",
    "schema_version": 1,
    "status": status,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "artifact_dir": artifact_dir,
    "execution_mode": execution_mode,
    "backup": {
        "artifact_path": str(backup_db.resolve()) if backup_db.is_file() else "",
        "artifact_sha256": file_sha256(backup_db),
        "size_bytes": backup_db.stat().st_size if backup_db.is_file() else None,
    },
    "canary": canary,
    "restore_refs_env_path": str(Path(refs_env_raw).resolve())
    if Path(refs_env_raw).is_file()
    else "",
    "errors": [] if status == "ok" else [error_code or "restore_canary_prepare_failed"],
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
path = Path(report_path_raw)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(encoded + "\n", encoding="utf-8")
PY
}

if [[ ! -f "${SOURCE_DB_PATH}" ]]; then
  emit_report "failed" "source_db_not_found"
  exit 1
fi

if ! is_true "${SKIP_BUILD}"; then
  if command -v cargo >/dev/null 2>&1; then
    if ! (
      cd "${REPO_DIR}/agent-platform"
      cargo build --quiet -p tonglingyu-gateway
    ); then
      emit_report "failed" "gateway_build_failed"
      exit 1
    fi
  elif [[ ! -x "${GATEWAY_BIN}" ]]; then
    emit_report "failed" "gateway_build_failed"
    exit 1
  fi
fi

if [[ ! -x "${GATEWAY_BIN}" ]]; then
  emit_report "failed" "gateway_binary_missing"
  exit 1
fi

run_host_canary() {
  "${GATEWAY_BIN}" backup-db \
    --db "${SOURCE_DB_PATH}" \
    --output "${BACKUP_DB}" \
    >"${BACKUP_STDOUT}" 2>"${BACKUP_STDERR}"
  local canary_cmd=(
    "${GATEWAY_BIN}"
    rqa-restore-canary
    --db "${SOURCE_DB_PATH}"
    --reviewer "${REVIEWER}"
    --review-note "${REVIEW_NOTE}"
  )
  if [[ -n "${PACKAGE_ID}" ]]; then
    canary_cmd+=(--package-id "${PACKAGE_ID}")
  fi
  "${canary_cmd[@]}" >"${CANARY_STDOUT}" 2>"${CANARY_STDERR}"
}

run_docker_canary() {
  EXECUTION_MODE="docker"
  docker compose exec -T "${DOCKER_SERVICE}" sh -lc \
    'mkdir -p "$1" && "$2" backup-db --db "$3" --output "$1/live-db-before-restore-canary.db"' \
    sh "${CONTAINER_ARTIFACT_DIR}" "${CONTAINER_GATEWAY_BIN}" "${CONTAINER_DB_PATH}" \
    >"${BACKUP_STDOUT}" 2>"${BACKUP_STDERR}"
  local canary_cmd=(
    docker compose exec -T "${DOCKER_SERVICE}"
    "${CONTAINER_GATEWAY_BIN}"
    rqa-restore-canary
    --db "${CONTAINER_DB_PATH}"
    --reviewer "${REVIEWER}"
    --review-note "${REVIEW_NOTE}"
  )
  if [[ -n "${PACKAGE_ID}" ]]; then
    canary_cmd+=(--package-id "${PACKAGE_ID}")
  fi
  "${canary_cmd[@]}" >"${CANARY_STDOUT}" 2>"${CANARY_STDERR}"
}

if ! run_host_canary; then
  if ! is_true "${DOCKER_FALLBACK}"; then
    emit_report "failed" "host_restore_canary_command_failed"
    exit 1
  fi
  if ! run_docker_canary; then
    emit_report "failed" "docker_restore_canary_command_failed"
    exit 1
  fi
fi

emit_report "ok"
