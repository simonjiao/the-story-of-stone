#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

REPORT_PATH="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_REPORT_PATH:-}"
SOURCE_DB_PATH_RAW="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_DB_PATH:-${TONGLINGYU_RQA_DB_PATH:-}}"
SOURCE_ROOT="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_SOURCE_ROOT:-${REPO_DIR}/resources/sources/wiki}"
GATEWAY_BIN="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_GATEWAY_BIN:-${REPO_DIR}/agent-platform/target/debug/tonglingyu-gateway}"
REQUIRE_LIVE="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_REQUIRE_LIVE:-${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}}"
BACKUP_PATH_RAW="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_BACKUP_PATH:-}"
BUILD_TIMEOUT_SECONDS="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_BUILD_TIMEOUT_SECONDS:-300}"
KB_BUILD_TIMEOUT_SECONDS="${TONGLINGYU_RQA_MIGRATION_PREFLIGHT_KB_BUILD_TIMEOUT_SECONDS:-180}"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

now_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

run_with_timeout() {
  local timeout_seconds="$1"
  shift
  python3 - "${timeout_seconds}" "$@" <<'PY'
import subprocess
import sys

timeout_seconds = float(sys.argv[1])
command = sys.argv[2:]
try:
    completed = subprocess.run(command, timeout=timeout_seconds)
except subprocess.TimeoutExpired:
    raise SystemExit(124)
raise SystemExit(completed.returncode)
PY
}

resolve_path() {
  local input_path="$1"
  python3 - "${input_path}" "${REPO_DIR}" <<'PY'
import sys
from pathlib import Path

raw, repo_dir = sys.argv[1:3]
path = Path(raw)
if not path.is_absolute():
    path = Path(repo_dir) / path
print(path.resolve())
PY
}

emit_failure() {
  local error_code="$1"
  local started_ms="${2:-0}"
  local finished_ms
  finished_ms="$(now_ms)"
  python3 - "${error_code}" "${started_ms}" "${finished_ms}" "${REPORT_PATH}" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

error_code, started_raw, finished_raw, report_path = sys.argv[1:5]
started_ms = int(started_raw or "0")
finished_ms = int(finished_raw)
started_at = None
if started_ms > 0:
    started_at = datetime.fromtimestamp(started_ms / 1000, timezone.utc).isoformat()
finished_at = datetime.fromtimestamp(finished_ms / 1000, timezone.utc).isoformat()
payload = {
    "object": "tonglingyu.rqa_migration_preflight_gate",
    "schema_version": 1,
    "status": "failed",
    "migration_preflight_passed": False,
    "policy_version": "tonglingyu-rqa-migration-preflight-v1",
    "started_at": started_at,
    "finished_at": finished_at,
    "errors": [error_code],
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path:
    path = Path(report_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(encoded + "\n", encoding="utf-8")
PY
  exit 1
}

STARTED_MS="$(now_ms)"

if is_true "${REQUIRE_LIVE}" && [[ -z "${BACKUP_PATH_RAW//[[:space:]]/}" ]]; then
  emit_failure "live_backup_path_missing" "${STARTED_MS}"
fi

if ! (
  cd "${REPO_DIR}/agent-platform"
  run_with_timeout \
    "${BUILD_TIMEOUT_SECONDS}" \
    cargo build --quiet -p tonglingyu-gateway
); then
  emit_failure "gateway_build_failed" "${STARTED_MS}"
fi

if [[ ! -x "${GATEWAY_BIN}" ]]; then
  emit_failure "gateway_binary_missing" "${STARTED_MS}"
fi

SOURCE_MODE="existing_db"
if [[ -n "${SOURCE_DB_PATH_RAW//[[:space:]]/}" ]]; then
  DB_PATH="$(resolve_path "${SOURCE_DB_PATH_RAW}")"
else
  DB_PATH="${REPO_DIR}/data/tonglingyu/tonglingyu.db"
fi

if [[ ! -f "${DB_PATH}" ]]; then
  if is_true "${REQUIRE_LIVE}"; then
    emit_failure "live_db_missing" "${STARTED_MS}"
  fi
  SOURCE_MODE="fixture_built"
  DB_PATH="${WORK_DIR}/migration-preflight.db"
  if ! run_with_timeout "${KB_BUILD_TIMEOUT_SECONDS}" "${GATEWAY_BIN}" build-kb \
    --db "${DB_PATH}" \
    --source-root "${SOURCE_ROOT}" \
    --rebuild \
    --skip-diff-eval \
    >"${WORK_DIR}/build-kb.stdout" \
    2>"${WORK_DIR}/build-kb.stderr"; then
    emit_failure "fixture_kb_build_failed" "${STARTED_MS}"
  fi
fi

if [[ -n "${BACKUP_PATH_RAW//[[:space:]]/}" ]]; then
  BACKUP_PATH="$(resolve_path "${BACKUP_PATH_RAW}")"
else
  BACKUP_PATH="${WORK_DIR}/migration-preflight-backup.db"
fi
mkdir -p "$(dirname -- "${BACKUP_PATH}")"
if [[ -e "${BACKUP_PATH}" ]]; then
  emit_failure "backup_path_already_exists" "${STARTED_MS}"
fi

BACKUP_STARTED_MS="$(now_ms)"
if ! python3 - "${DB_PATH}" "${BACKUP_PATH}" <<'PY'
import sqlite3
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
backup_path = Path(sys.argv[2])
backup_path.parent.mkdir(parents=True, exist_ok=True)
source = sqlite3.connect(f"file:{source_path}?mode=ro", uri=True)
target = sqlite3.connect(backup_path)
try:
    source.backup(target)
finally:
    target.close()
    source.close()
PY
then
  emit_failure "sqlite_backup_failed" "${STARTED_MS}"
fi
BACKUP_FINISHED_MS="$(now_ms)"

PREFLIGHT_STARTED_MS="$(now_ms)"
if ! "${GATEWAY_BIN}" runtime-schema-preflight \
  --db "${DB_PATH}" \
  >"${WORK_DIR}/schema-preflight.json" \
  2>"${WORK_DIR}/schema-preflight.stderr"; then
  emit_failure "schema_preflight_failed" "${STARTED_MS}"
fi
PREFLIGHT_FINISHED_MS="$(now_ms)"

if ! python3 - \
  "${DB_PATH}" \
  "${BACKUP_PATH}" \
  "${WORK_DIR}/schema-preflight.json" \
  "${REPORT_PATH}" \
  "${STARTED_MS}" \
  "${BACKUP_STARTED_MS}" \
  "${BACKUP_FINISHED_MS}" \
  "${PREFLIGHT_STARTED_MS}" \
  "${PREFLIGHT_FINISHED_MS}" \
  "${SOURCE_MODE}" \
  "${REQUIRE_LIVE}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    db_raw,
    backup_raw,
    preflight_raw,
    report_raw,
    started_raw,
    backup_started_raw,
    backup_finished_raw,
    preflight_started_raw,
    preflight_finished_raw,
    source_mode,
    require_live_raw,
) = sys.argv[1:12]


def as_dt(ms_raw):
    return datetime.fromtimestamp(int(ms_raw) / 1000, timezone.utc).isoformat()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_digest(value):
    encoded = json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    return sha256_bytes(encoded.encode("utf-8"))


def path_digest(path):
    return sha256_bytes(str(Path(path).resolve()).encode("utf-8"))


def is_true(value):
    return value in {"1", "true", "TRUE", "yes", "YES", "on", "ON"}


db_path = Path(db_raw).resolve()
backup_path = Path(backup_raw).resolve()
preflight_path = Path(preflight_raw)
with preflight_path.open("r", encoding="utf-8") as handle:
    preflight = json.load(handle)

required = preflight.get("required_migrations")
applied = preflight.get("applied_migrations")
pending = preflight.get("pending_migrations")
if not isinstance(required, list):
    required = []
if not isinstance(applied, list):
    applied = []
if not isinstance(pending, list):
    pending = []

checks = {
    "backup_created": backup_path.is_file() and backup_path.stat().st_size > 0,
    "backup_before_preflight": int(backup_finished_raw) <= int(preflight_started_raw),
    "backup_path_recorded": bool(str(backup_path)),
    "schema_preflight_ran": preflight.get("object")
    == "tonglingyu.runtime_schema_migration_preflight",
    "no_runtime_data_rebuild": preflight.get("will_rebuild_knowledge_base") is False,
    "no_runtime_data_delete": preflight.get("will_delete_runtime_data") is False,
    "no_secret_values": preflight.get("contains_secret_values") is False,
}
passed = all(checks.values())
status = "ok" if passed else "failed"
started_ms = int(started_raw)
finished_ms = int(preflight_finished_raw)
source_db_sha256 = sha256_file(db_path)
backup_sha256 = sha256_file(backup_path) if backup_path.is_file() else ""
payload = {
    "object": "tonglingyu.rqa_migration_preflight_gate",
    "schema_version": 1,
    "status": status,
    "migration_preflight_passed": passed,
    "policy_version": "tonglingyu-rqa-migration-preflight-v1",
    "mode": "live" if is_true(require_live_raw) else "preflight",
    "require_live": is_true(require_live_raw),
    "source_mode": source_mode,
    "started_at": as_dt(started_raw),
    "finished_at": as_dt(preflight_finished_raw),
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "duration_ms": max(0, finished_ms - started_ms),
    "db": {
        "path": str(db_path),
        "path_sha256": path_digest(db_path),
        "source_db_sha256": source_db_sha256,
        "size_bytes": db_path.stat().st_size,
    },
    "backup": {
        "artifact_path": str(backup_path),
        "artifact_path_sha256": path_digest(backup_path),
        "artifact_sha256": backup_sha256,
        "source_db_sha256": source_db_sha256,
        "size_bytes": backup_path.stat().st_size if backup_path.is_file() else 0,
        "started_at": as_dt(backup_started_raw),
        "finished_at": as_dt(backup_finished_raw),
        "before_preflight": int(backup_finished_raw) <= int(preflight_started_raw),
    },
    "migration_preflight": preflight,
    "migration_preflight_sha256": canonical_digest(preflight),
    "migration_counts": {
        "required": len(required),
        "applied": len(applied),
        "pending": len(pending),
    },
    "checks": checks,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_raw:
    report_path = Path(report_raw)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
if not passed:
    raise SystemExit(1)
PY
then
  emit_failure "migration_preflight_checks_failed" "${STARTED_MS}"
fi
