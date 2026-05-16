#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

SOURCE_DB_PATH="${TONGLINGYU_RQA_RESTORE_CANARY_DB_PATH:-${TONGLINGYU_RQA_DB_PATH:-${REPO_DIR}/data/tonglingyu/tonglingyu.db}}"
GATEWAY_BIN="${TONGLINGYU_RQA_RESTORE_CANARY_GATEWAY_BIN:-${TONGLINGYU_RQA_GATEWAY_BIN:-${REPO_DIR}/agent-platform/target/debug/tonglingyu-gateway}}"
SKIP_BUILD="${TONGLINGYU_RQA_RESTORE_CANARY_SKIP_BUILD:-false}"
REVIEWER="${TONGLINGYU_RQA_RESTORE_CANARY_REVIEWER:-restore-drill}"
REVIEW_NOTE="${TONGLINGYU_RQA_RESTORE_CANARY_REVIEW_NOTE:-closed restore drill canary}"
PACKAGE_ID="${TONGLINGYU_RQA_RESTORE_CANARY_PACKAGE_ID:-}"
ARTIFACT_ROOT="${TONGLINGYU_RQA_RESTORE_CANARY_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/restore-canaries}"
RUN_ID="${TONGLINGYU_RQA_RESTORE_CANARY_RUN_ID:-$(date -u +"%Y%m%dT%H%M%SZ")-$$}"
ARTIFACT_DIR="${TONGLINGYU_RQA_RESTORE_CANARY_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
REPORT_PATH="${TONGLINGYU_RQA_RESTORE_CANARY_REPORT_PATH:-${ARTIFACT_DIR}/restore-canary-prepare.json}"

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

emit_report() {
  local status="$1"
  local error_code="${2:-}"
  python3 - "${REPORT_PATH}" "${status}" "${error_code}" "${ARTIFACT_DIR}" \
    "${BACKUP_DB}" "${CANARY_STDOUT}" "${CANARY_STDERR}" "${RESTORE_REFS_ENV}" <<'PY'
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
) = sys.argv[1:9]

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
  if ! (
    cd "${REPO_DIR}/agent-platform"
    cargo build --quiet -p tonglingyu-gateway
  ); then
    emit_report "failed" "gateway_build_failed"
    exit 1
  fi
fi

if [[ ! -x "${GATEWAY_BIN}" ]]; then
  emit_report "failed" "gateway_binary_missing"
  exit 1
fi

if ! "${GATEWAY_BIN}" backup-db \
  --db "${SOURCE_DB_PATH}" \
  --output "${BACKUP_DB}" \
  >"${BACKUP_STDOUT}" 2>"${BACKUP_STDERR}"; then
  emit_report "failed" "backup_command_failed"
  exit 1
fi

canary_cmd=(
  "${GATEWAY_BIN}"
  rqa-restore-canary
  --db "${SOURCE_DB_PATH}"
  --reviewer "${REVIEWER}"
  --review-note "${REVIEW_NOTE}"
)
if [[ -n "${PACKAGE_ID}" ]]; then
  canary_cmd+=(--package-id "${PACKAGE_ID}")
fi

if ! "${canary_cmd[@]}" >"${CANARY_STDOUT}" 2>"${CANARY_STDERR}"; then
  emit_report "failed" "restore_canary_command_failed"
  exit 1
fi

emit_report "ok"
