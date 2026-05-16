#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
WORK_DIR="$(mktemp -d)"
KEEP_WORK_DIR="${TONGLINGYU_RQA_RESTORE_DRILL_KEEP_WORK_DIR:-false}"

SERVER_PID=""
cleanup() {
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  if is_true "${KEEP_WORK_DIR}"; then
    printf 'restore_drill_work_dir=%s\n' "${WORK_DIR}" >&2
  else
    rm -rf "${WORK_DIR}"
  fi
}
trap cleanup EXIT

REPORT_PATH="${TONGLINGYU_RQA_RESTORE_DRILL_REPORT_PATH:-}"
SOURCE_DB_PATH="${TONGLINGYU_RQA_RESTORE_DRILL_DB_PATH:-${TONGLINGYU_RQA_DB_PATH:-${REPO_DIR}/data/tonglingyu/tonglingyu.db}}"
SOURCE_ROOT="${TONGLINGYU_RQA_RESTORE_DRILL_SOURCE_ROOT:-${REPO_DIR}/resources/sources/wiki}"
RTO_TARGET_SECONDS="${TONGLINGYU_RQA_RESTORE_DRILL_RTO_SECONDS:-900}"
RPO_TARGET_SECONDS="${TONGLINGYU_RQA_RESTORE_DRILL_RPO_SECONDS:-3600}"
OPERATOR="${TONGLINGYU_RQA_RESTORE_DRILL_OPERATOR:-local-drill}"
ENVIRONMENT="${TONGLINGYU_RQA_RESTORE_DRILL_ENVIRONMENT:-local}"
EVAL_LIMIT="${TONGLINGYU_RQA_EVAL_LIMIT:-8}"
REQUIRE_LIVE="${TONGLINGYU_RQA_RESTORE_DRILL_REQUIRE_LIVE:-false}"
UPSTREAM_MODEL="${TONGLINGYU_UPSTREAM_MODEL:-${AGENT_RUNTIME_HERMES_MODEL:-hermes-agent}}"
ARTIFACT_ROOT="${TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/restore-drills}"
ARTIFACT_RUN_ID="${TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_RUN_ID:-$(date -u +"%Y%m%dT%H%M%SZ")-$$}"
ARTIFACT_DIR_OVERRIDE="${TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_DIR:-}"

TRACE_ID="${TONGLINGYU_RQA_RESTORE_DRILL_TRACE_ID:-}"
PACKAGE_ID="${TONGLINGYU_RQA_RESTORE_DRILL_PACKAGE_ID:-}"
FAILURE_ID="${TONGLINGYU_RQA_RESTORE_DRILL_FAILURE_ID:-}"
TASK_ID="${TONGLINGYU_RQA_RESTORE_DRILL_TASK_ID:-}"

GATEWAY_BIN="${TONGLINGYU_RQA_RESTORE_DRILL_GATEWAY_BIN:-${REPO_DIR}/agent-platform/target/debug/tonglingyu-gateway}"
SKIP_BUILD="${TONGLINGYU_RQA_RESTORE_DRILL_SKIP_BUILD:-false}"
DOCKER_FALLBACK="${TONGLINGYU_RQA_RESTORE_DRILL_DOCKER_FALLBACK:-true}"
DOCKER_SERVICE="${TONGLINGYU_RQA_RESTORE_DRILL_DOCKER_SERVICE:-tonglingyu-gateway}"
CONTAINER_DB_PATH="${TONGLINGYU_RQA_RESTORE_DRILL_CONTAINER_DB_PATH:-${TONGLINGYU_DB_PATH:-/data/tonglingyu.db}}"
CONTAINER_GATEWAY_BIN="${TONGLINGYU_RQA_RESTORE_DRILL_CONTAINER_GATEWAY_BIN:-tonglingyu-gateway}"
RESTORED_DB="${WORK_DIR}/restored.db"
META_JSON="${WORK_DIR}/restore-drill-meta.json"
BACKUP_EXECUTION_MODE="host"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

if [[ -n "${ARTIFACT_DIR_OVERRIDE}" ]]; then
  RESTORE_ARTIFACT_DIR="${ARTIFACT_DIR_OVERRIDE}"
elif is_true "${REQUIRE_LIVE}"; then
  RESTORE_ARTIFACT_DIR="${ARTIFACT_ROOT}/${ARTIFACT_RUN_ID}"
else
  RESTORE_ARTIFACT_DIR="${WORK_DIR}"
fi
mkdir -p "${RESTORE_ARTIFACT_DIR}"
RESTORE_ARTIFACT_DIR="$(cd -- "${RESTORE_ARTIFACT_DIR}" && pwd)"
BACKUP_DB="${RESTORE_ARTIFACT_DIR}/backup.db"
CONTAINER_BACKUP_DB="/tmp/tonglingyu-restore-drill-${ARTIFACT_RUN_ID}.db"

now_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
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
    "object": "tonglingyu.rqa_backup_restore_drill",
    "schema_version": 1,
    "status": "failed",
    "drill_result": "failed",
    "errors": [error_code],
    "started_at": started_at,
    "finished_at": finished_at,
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

refs_provided="false"
if [[ -n "${TRACE_ID}" || -n "${PACKAGE_ID}" || -n "${FAILURE_ID}" || -n "${TASK_ID}" ]]; then
  if [[ -z "${TRACE_ID}" || -z "${PACKAGE_ID}" || -z "${FAILURE_ID}" || -z "${TASK_ID}" ]]; then
    emit_failure "restore_reference_set_incomplete" "${STARTED_MS}"
  fi
  refs_provided="true"
fi

if is_true "${REQUIRE_LIVE}" && [[ "${refs_provided}" != "true" ]]; then
  emit_failure "live_restore_reference_missing" "${STARTED_MS}"
fi

if ! is_true "${SKIP_BUILD}"; then
  if ! (
    cd "${REPO_DIR}/agent-platform"
    cargo build --quiet -p tonglingyu-gateway
  ); then
    emit_failure "gateway_build_failed" "${STARTED_MS}"
  fi
fi

if [[ ! -x "${GATEWAY_BIN}" ]]; then
  emit_failure "gateway_binary_missing" "${STARTED_MS}"
fi

SOURCE_MODE="existing_refs"
PRIMARY_DB="${SOURCE_DB_PATH}"

if [[ "${refs_provided}" != "true" ]]; then
  SOURCE_MODE="fixture"
  PRIMARY_DB="${WORK_DIR}/primary.db"
  if ! "${GATEWAY_BIN}" build-kb \
    --db "${PRIMARY_DB}" \
    --source-root "${SOURCE_ROOT}" \
    --rebuild \
    --skip-diff-eval \
    >"${WORK_DIR}/build-kb.stdout" \
    2>"${WORK_DIR}/build-kb.stderr"; then
    emit_failure "fixture_kb_build_failed" "${STARTED_MS}"
  fi
  if ! "${GATEWAY_BIN}" query \
    --db "${PRIMARY_DB}" \
    --limit 8 \
    "通灵玉正面文字在哪里？" \
    >"${WORK_DIR}/fixture-package.json" \
    2>"${WORK_DIR}/fixture-query.stderr"; then
    emit_failure "fixture_package_query_failed" "${STARTED_MS}"
  fi
  if ! python3 - "${PRIMARY_DB}" "${WORK_DIR}/fixture-package.json" "${META_JSON}" <<'PY'
import hashlib
import json
import sqlite3
import sys
from datetime import datetime, timezone

db_path, package_path, meta_path = sys.argv[1:4]
with open(package_path, "r", encoding="utf-8") as handle:
    package = json.load(handle)
trace_id = package.get("trace_id")
package_id = package.get("package_id")
if not trace_id or not package_id:
    raise SystemExit("fixture package missing trace_id or package_id")

now = datetime.now(timezone.utc).isoformat()
seed_text = "restore drill resolved retrieval sample"
question_sha256 = hashlib.sha256(seed_text.encode("utf-8")).hexdigest()
failure_id = f"rf-restore-drill-{question_sha256[:16]}"
task_id = f"kgt-restore-drill-{question_sha256[16:32]}"

conn = sqlite3.connect(db_path)
conn.execute(
    """
    INSERT INTO retrieval_failures (
        failure_id, trace_id, package_id, question_sha256, question_char_count,
        question_summary, kb_schema_version, kb_version_id, failure_type,
        redacted_query_terms_json, required_evidence_types_json,
        actual_evidence_types_json, expected_evidence_ids_json,
        selected_evidence_ids_json, missing_evidence_types_json,
        quality_issues_json, agent_diagnosis, proposed_fix, human_review_status,
        reviewer, review_note, created_at, updated_at, resolved_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """,
    (
        failure_id,
        trace_id,
        package_id,
        question_sha256,
        len(seed_text),
        f"sha256:{question_sha256[:12]}",
        "tonglingyu-v1-sqlite-fts",
        None,
        "expected_evidence_missing",
        json.dumps(["restore", "drill"], ensure_ascii=True),
        json.dumps(["base_text"], ensure_ascii=True),
        json.dumps(["base_text"], ensure_ascii=True),
        json.dumps([], ensure_ascii=True),
        json.dumps([], ensure_ascii=True),
        json.dumps([], ensure_ascii=True),
        json.dumps(["restore_drill_resolved_failure"], ensure_ascii=True),
        "restore drill fixture was seeded as resolved",
        "no production knowledge change required",
        "resolved",
        "restore-drill",
        "resolved restore drill fixture",
        now,
        now,
        now,
    ),
)
conn.execute(
    """
    INSERT INTO knowledge_governance_tasks (
        task_id, source_failure_id, source_entity_type, source_entity_id,
        trace_id, package_id, task_type, status, priority, agent_cluster_key,
        proposed_fix, reviewer, review_note, evidence_ref, created_at,
        updated_at, accepted_at, closed_at
    ) VALUES (?, ?, 'retrieval_failure', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """,
    (
        task_id,
        failure_id,
        failure_id,
        trace_id,
        package_id,
        "expected_evidence_fix",
        "closed",
        "p1",
        f"restore-drill:{question_sha256[:16]}",
        "restore drill governance fixture closed after verification",
        "restore-drill",
        "closed restore drill fixture",
        f"package:{package_id}",
        now,
        now,
        None,
        now,
    ),
)
conn.commit()
conn.close()
meta = {
    "trace_id": trace_id,
    "package_id": package_id,
    "failure_id": failure_id,
    "task_id": task_id,
}
with open(meta_path, "w", encoding="utf-8") as handle:
    json.dump(meta, handle, sort_keys=True)
    handle.write("\n")
PY
  then
    emit_failure "fixture_rqa_seed_failed" "${STARTED_MS}"
  fi
else
  if [[ ! -f "${PRIMARY_DB}" ]]; then
    emit_failure "source_db_not_found" "${STARTED_MS}"
  fi
  python3 - "${META_JSON}" "${TRACE_ID}" "${PACKAGE_ID}" "${FAILURE_ID}" "${TASK_ID}" <<'PY'
import json
import sys

meta_path, trace_id, package_id, failure_id, task_id = sys.argv[1:6]
with open(meta_path, "w", encoding="utf-8") as handle:
    json.dump(
        {
            "trace_id": trace_id,
            "package_id": package_id,
            "failure_id": failure_id,
            "task_id": task_id,
        },
        handle,
        sort_keys=True,
    )
    handle.write("\n")
PY
fi

read -r TRACE_ID PACKAGE_ID FAILURE_ID TASK_ID < <(
  python3 - "${META_JSON}" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    meta = json.load(handle)
print(meta["trace_id"], meta["package_id"], meta["failure_id"], meta["task_id"])
PY
)

BACKUP_STARTED_MS="$(now_ms)"
backup_primary_db() {
  "${GATEWAY_BIN}" backup-db \
    --db "${PRIMARY_DB}" \
    --output "${BACKUP_DB}" \
    >"${WORK_DIR}/backup.stdout" \
    2>"${WORK_DIR}/backup.stderr"
}

backup_primary_db_with_docker() {
  BACKUP_EXECUTION_MODE="docker"
  docker compose exec -T "${DOCKER_SERVICE}" \
    "${CONTAINER_GATEWAY_BIN}" backup-db \
    --db "${CONTAINER_DB_PATH}" \
    --output "${CONTAINER_BACKUP_DB}" \
    >"${WORK_DIR}/backup.stdout" \
    2>"${WORK_DIR}/backup.stderr"
  docker compose cp "${DOCKER_SERVICE}:${CONTAINER_BACKUP_DB}" "${BACKUP_DB}" \
    >>"${WORK_DIR}/backup.stdout" \
    2>>"${WORK_DIR}/backup.stderr"
  docker compose exec -T "${DOCKER_SERVICE}" rm -f "${CONTAINER_BACKUP_DB}" \
    >>"${WORK_DIR}/backup.stdout" \
    2>>"${WORK_DIR}/backup.stderr" || true
}

if ! backup_primary_db; then
  if ! is_true "${DOCKER_FALLBACK}"; then
    emit_failure "backup_command_failed" "${STARTED_MS}"
  fi
  if ! backup_primary_db_with_docker; then
    emit_failure "backup_command_failed" "${STARTED_MS}"
  fi
fi
BACKUP_FINISHED_MS="$(now_ms)"

RESTORE_STARTED_MS="$(now_ms)"
cp "${BACKUP_DB}" "${RESTORED_DB}"
chmod 0600 "${RESTORED_DB}"

if ! python3 - "${RESTORED_DB}" <<'PY'
import sqlite3
import sys

conn = sqlite3.connect(sys.argv[1])
value = conn.execute("PRAGMA integrity_check").fetchone()[0]
conn.close()
if value != "ok":
    raise SystemExit(value)
PY
then
  emit_failure "restored_db_integrity_check_failed" "${STARTED_MS}"
fi

RESTORE_PORT="$(
  python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"
GATEWAY_KEY="restore-drill-gateway-${RESTORE_PORT}"
ADMIN_KEY="restore-drill-admin-${RESTORE_PORT}"

TONGLINGYU_AGENT_RUNTIME_MODE=minimal \
TONGLINGYU_GATEWAY_API_KEY="${GATEWAY_KEY}" \
TONGLINGYU_ADMIN_API_KEY="${ADMIN_KEY}" \
TONGLINGYU_RATE_LIMIT_PER_MINUTE=0 \
"${GATEWAY_BIN}" serve \
  --db "${RESTORED_DB}" \
  --bind "127.0.0.1:${RESTORE_PORT}" \
  >"${WORK_DIR}/gateway.stdout" \
  2>"${WORK_DIR}/gateway.stderr" &
SERVER_PID="$!"

health_ok="false"
for _ in $(seq 1 100); do
  if curl -fsS "http://127.0.0.1:${RESTORE_PORT}/healthz" \
    >"${WORK_DIR}/health.json" \
    2>"${WORK_DIR}/health.stderr"; then
    health_ok="true"
    break
  fi
  if ! kill -0 "${SERVER_PID}" 2>/dev/null; then
    emit_failure "restored_gateway_exited" "${STARTED_MS}"
  fi
  sleep 0.1
done
if [[ "${health_ok}" != "true" ]]; then
  emit_failure "restored_gateway_health_failed" "${STARTED_MS}"
fi

if ! python3 - "${META_JSON}" "${RESTORED_DB}" <<'PY'
import json
import sqlite3
import sys

meta_path, db_path = sys.argv[1:3]
with open(meta_path, "r", encoding="utf-8") as handle:
    meta = json.load(handle)
conn = sqlite3.connect(db_path)
checks = {
    "package": (
        "SELECT COUNT(*) FROM evidence_packages WHERE package_id = ? AND trace_id = ?",
        (meta["package_id"], meta["trace_id"]),
    ),
    "failure": (
        "SELECT COUNT(*) FROM retrieval_failures WHERE failure_id = ? AND trace_id = ?",
        (meta["failure_id"], meta["trace_id"]),
    ),
    "task": (
        "SELECT COUNT(*) FROM knowledge_governance_tasks WHERE task_id = ? AND trace_id = ?",
        (meta["task_id"], meta["trace_id"]),
    ),
}
missing = []
for name, (sql, params) in checks.items():
    if conn.execute(sql, params).fetchone()[0] != 1:
        missing.append(name)
conn.close()
if missing:
    raise SystemExit(",".join(missing))
PY
then
  emit_failure "restored_rqa_reference_missing" "${STARTED_MS}"
fi

ADMIN_HEADER=(-H "Authorization: Bearer ${ADMIN_KEY}")
if ! curl -fsS "${ADMIN_HEADER[@]}" \
  "http://127.0.0.1:${RESTORE_PORT}/v1/admin/traces/${TRACE_ID}" \
  >"${WORK_DIR}/admin-trace.json" \
  2>"${WORK_DIR}/admin-trace.stderr"; then
  emit_failure "admin_trace_read_failed" "${STARTED_MS}"
fi
if ! curl -fsS "${ADMIN_HEADER[@]}" \
  "http://127.0.0.1:${RESTORE_PORT}/v1/admin/retrieval-failures/${FAILURE_ID}" \
  >"${WORK_DIR}/admin-failure.json" \
  2>"${WORK_DIR}/admin-failure.stderr"; then
  emit_failure "admin_retrieval_failure_read_failed" "${STARTED_MS}"
fi
if ! curl -fsS "${ADMIN_HEADER[@]}" \
  "http://127.0.0.1:${RESTORE_PORT}/v1/admin/governance/tasks/${TASK_ID}" \
  >"${WORK_DIR}/admin-task.json" \
  2>"${WORK_DIR}/admin-task.stderr"; then
  emit_failure "admin_governance_task_read_failed" "${STARTED_MS}"
fi
if ! curl -fsS "${ADMIN_HEADER[@]}" \
  "http://127.0.0.1:${RESTORE_PORT}/v1/admin/packages/${PACKAGE_ID}" \
  >"${WORK_DIR}/admin-package.json" \
  2>"${WORK_DIR}/admin-package.stderr"; then
  emit_failure "admin_package_read_failed" "${STARTED_MS}"
fi
if ! "${GATEWAY_BIN}" replay-package \
  --db "${RESTORED_DB}" \
  "${PACKAGE_ID}" \
  >"${WORK_DIR}/package-replay.json" \
  2>"${WORK_DIR}/package-replay.stderr"; then
  emit_failure "package_replay_failed" "${STARTED_MS}"
fi

if ! python3 - "${META_JSON}" \
  "${WORK_DIR}/admin-trace.json" \
  "${WORK_DIR}/admin-failure.json" \
  "${WORK_DIR}/admin-task.json" \
  "${WORK_DIR}/admin-package.json" \
  "${WORK_DIR}/package-replay.json" <<'PY'
import json
import sys

meta_path, trace_path, failure_path, task_path, package_path, replay_path = sys.argv[1:7]
with open(meta_path, "r", encoding="utf-8") as handle:
    meta = json.load(handle)

def load(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)

trace = load(trace_path)
failure = load(failure_path).get("failure", {})
task = load(task_path).get("task", {})
package = load(package_path)
replay = load(replay_path)
checks = [
    trace.get("object") == "tonglingyu.trace",
    meta["failure_id"] in trace.get("retrieval_failure_ids", []),
    meta["task_id"] in trace.get("governance_task_ids", []),
    failure.get("failure_id") == meta["failure_id"],
    task.get("task_id") == meta["task_id"],
    package.get("object") == "tonglingyu.package_audit",
    package.get("package_id") == meta["package_id"],
    isinstance(replay, dict)
    and isinstance(replay.get("package"), dict)
    and replay["package"].get("package_id") == meta["package_id"],
]
if not all(checks):
    raise SystemExit("restored admin or replay payload did not match expected references")
PY
then
  emit_failure "restored_payload_validation_failed" "${STARTED_MS}"
fi

RESTORE_EVAL_DB="${WORK_DIR}/restore-eval-input.db"
RESTORE_EVAL_REPORT="${WORK_DIR}/restore-rqa-eval-report.json"
cp "${RESTORED_DB}" "${RESTORE_EVAL_DB}"
if ! "${GATEWAY_BIN}" eval \
  --db "${RESTORE_EVAL_DB}" \
  --limit "${EVAL_LIMIT}" \
  --report "${RESTORE_EVAL_REPORT}" \
  >"${WORK_DIR}/restore-eval.stdout" \
  2>"${WORK_DIR}/restore-eval.stderr"; then
  emit_failure "restore_eval_report_generation_failed" "${STARTED_MS}"
fi

if ! env \
  "TONGLINGYU_UPSTREAM_MODEL=${UPSTREAM_MODEL}" \
  "TONGLINGYU_RQA_DB_PATH=${RESTORED_DB}" \
  "TONGLINGYU_RQA_EVAL_REPORT_PATH=${RESTORE_EVAL_REPORT}" \
  "${SCRIPT_DIR}/verify-tonglingyu-rqa-quality-gate.sh" \
  >"${WORK_DIR}/restore-rqa-quality-gate.json" \
  2>"${WORK_DIR}/restore-rqa-quality-gate.stderr"; then
  emit_failure "restore_rqa_quality_gate_failed" "${STARTED_MS}"
fi

NESTED_PASS="${WORK_DIR}/nested-gate-pass.sh"
python3 - "${NESTED_PASS}" <<'PY'
import stat
import sys
from pathlib import Path

path = Path(sys.argv[1])
path.write_text(
    "#!/usr/bin/env bash\n"
    "set -euo pipefail\n"
    "echo '{\"status\":\"ok\",\"source\":\"restore-drill-nested-mock\",\"secret_values_printed\":false}'\n",
    encoding="utf-8",
)
path.chmod(path.stat().st_mode | stat.S_IXUSR)
PY

NESTED_GIT_COMMIT="${TONGLINGYU_RELEASE_GIT_COMMIT:-}"
if [[ -z "${NESTED_GIT_COMMIT}" ]]; then
  NESTED_GIT_COMMIT="$(git -C "${REPO_DIR}" rev-parse HEAD 2>/dev/null || true)"
fi
if [[ -z "${NESTED_GIT_COMMIT}" ]]; then
  NESTED_GIT_COMMIT="0000000000000000000000000000000000000000"
fi
NESTED_GIT_TRACKED_DIRTY="${TONGLINGYU_RELEASE_GIT_TRACKED_DIRTY:-false}"

if ! env \
  "TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true" \
  "TONGLINGYU_RELEASE_SUMMARY_ONLY=true" \
  "TONGLINGYU_RELEASE_REQUIRE_LIVE=false" \
  "TONGLINGYU_RELEASE_GIT_COMMIT=${NESTED_GIT_COMMIT}" \
  "TONGLINGYU_RELEASE_GIT_TRACKED_DIRTY=${NESTED_GIT_TRACKED_DIRTY}" \
  "TONGLINGYU_UPSTREAM_MODEL=${UPSTREAM_MODEL}" \
  "TONGLINGYU_RQA_DB_PATH=${RESTORED_DB}" \
  "TONGLINGYU_RQA_EVAL_REPORT_PATH=${RESTORE_EVAL_REPORT}" \
  "TONGLINGYU_RELEASE_REPORT_PATH=${WORK_DIR}/restore-release-readiness-report.json" \
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_OPS_READINESS_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_RQA_INCIDENT_CAPACITY_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD=${NESTED_PASS}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD=${NESTED_PASS}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" \
  >"${WORK_DIR}/restore-release-readiness.stdout" \
  2>"${WORK_DIR}/restore-release-readiness.stderr"; then
  emit_failure "restore_release_report_generation_failed" "${STARTED_MS}"
fi

if ! "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${WORK_DIR}/restore-release-readiness-report.json" \
  >"${WORK_DIR}/restore-release-report-validator.json" \
  2>"${WORK_DIR}/restore-release-report-validator.stderr"; then
  emit_failure "restore_release_report_validator_failed" "${STARTED_MS}"
fi

FINISHED_MS="$(now_ms)"

if ! python3 - "${REPORT_PATH}" "${META_JSON}" "${SOURCE_MODE}" "${PRIMARY_DB}" \
  "${BACKUP_DB}" "${RESTORED_DB}" "${WORK_DIR}/restore-rqa-quality-gate.json" \
  "${WORK_DIR}/restore-release-readiness-report.json" \
  "${WORK_DIR}/restore-release-report-validator.json" \
  "${STARTED_MS}" "${BACKUP_STARTED_MS}" "${BACKUP_FINISHED_MS}" \
  "${RESTORE_STARTED_MS}" "${FINISHED_MS}" "${RTO_TARGET_SECONDS}" \
  "${RPO_TARGET_SECONDS}" "${OPERATOR}" "${ENVIRONMENT}" "${BACKUP_EXECUTION_MODE}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path,
    meta_path,
    source_mode,
    primary_db,
    backup_db,
    restored_db,
    rqa_gate_path,
    release_report_path,
    validator_path,
    started_raw,
    backup_started_raw,
    backup_finished_raw,
    restore_started_raw,
    finished_raw,
    rto_target_raw,
    rpo_target_raw,
    operator,
    environment,
    backup_execution_mode,
) = sys.argv[1:20]

started_ms = int(started_raw)
backup_started_ms = int(backup_started_raw)
backup_finished_ms = int(backup_finished_raw)
restore_started_ms = int(restore_started_raw)
finished_ms = int(finished_raw)
rto_target = int(rto_target_raw)
rpo_target = int(rpo_target_raw)

def iso(ms):
    return datetime.fromtimestamp(ms / 1000, timezone.utc).isoformat()

def file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def hash_text(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()

with open(meta_path, "r", encoding="utf-8") as handle:
    meta = json.load(handle)
with open(rqa_gate_path, "r", encoding="utf-8") as handle:
    rqa_gate = json.load(handle)
with open(validator_path, "r", encoding="utf-8") as handle:
    validator = json.load(handle)

backup_path = str(Path(backup_db).resolve())
artifact_dir = str(Path(backup_db).resolve().parent)
rto_actual = (finished_ms - restore_started_ms) / 1000
rpo_actual = (finished_ms - backup_finished_ms) / 1000
rto_met = rto_actual <= rto_target
rpo_met = rpo_actual <= rpo_target
checks = {
    "admin_trace_readable": True,
    "retrieval_failure_readable": True,
    "governance_task_readable": True,
    "admin_package_readable": True,
    "package_replay_readable": True,
    "rqa_quality_gate_reran": rqa_gate.get("status") == "ok",
    "saved_report_validator_reran": validator.get("status") == "ok",
}
result_ok = rto_met and rpo_met and all(checks.values())
payload = {
    "object": "tonglingyu.rqa_backup_restore_drill",
    "schema_version": 1,
    "status": "ok" if result_ok else "failed",
    "drill_result": "passed" if result_ok else "failed",
    "source_mode": source_mode,
    "policy_version": "tonglingyu-rqa-backup-restore-drill-v1",
    "artifact_dir": artifact_dir,
    "artifact_dir_sha256": hash_text(artifact_dir),
    "started_at": iso(started_ms),
    "finished_at": iso(finished_ms),
    "duration_ms": finished_ms - started_ms,
    "environment": environment,
    "operator": operator,
    "rto": {
        "target_seconds": rto_target,
        "actual_seconds": rto_actual,
        "met": rto_met,
    },
    "rpo": {
        "target_seconds": rpo_target,
        "actual_seconds": rpo_actual,
        "met": rpo_met,
    },
    "backup": {
        "execution_mode": backup_execution_mode,
        "started_at": iso(backup_started_ms),
        "finished_at": iso(backup_finished_ms),
        "artifact_path": backup_path,
        "artifact_path_sha256": hash_text(backup_path),
        "artifact_sha256": file_sha256(backup_db),
        "size_bytes": Path(backup_db).stat().st_size,
        "source_db_sha256": file_sha256(primary_db),
    },
    "restore": {
        "started_at": iso(restore_started_ms),
        "finished_at": iso(finished_ms),
        "db_integrity_check": "ok",
        "restored_db_sha256": file_sha256(restored_db),
        "schema_migrations_verified": True,
    },
    "checks": checks,
    "refs": {
        "trace_sha256": hash_text(meta["trace_id"]),
        "package_sha256": hash_text(meta["package_id"]),
        "failure_sha256": hash_text(meta["failure_id"]),
        "governance_task_sha256": hash_text(meta["task_id"]),
    },
    "artifacts": {
        "rqa_quality_gate_sha256": file_sha256(rqa_gate_path),
        "saved_release_report_sha256": file_sha256(release_report_path),
        "saved_report_validator_sha256": file_sha256(validator_path),
    },
    "errors": [] if result_ok else ["rto_or_rpo_or_post_restore_check_failed"],
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path:
    path = Path(report_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(encoded + "\n", encoding="utf-8")
if not result_ok:
    raise SystemExit(1)
PY
then
  exit 1
fi
