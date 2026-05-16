#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

REMOTE_HOST="${TONGLINGYU_REMOTE_RELEASE_HOST:-hhost}"
REMOTE_PROJECT_DIR="${TONGLINGYU_REMOTE_RELEASE_PROJECT_DIR:-}"
REMOTE_PROJECT_ARG="${REMOTE_PROJECT_DIR:-__DEFAULT_REMOTE_PROJECT_DIR__}"
RUN_ID="${TONGLINGYU_REMOTE_RELEASE_AUTOMATION_RUN_ID:-remote-release-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
RELEASE_ENVIRONMENT="${TONGLINGYU_REMOTE_RELEASE_ENVIRONMENT:-hhost}"
RELEASE_TARGET="${TONGLINGYU_REMOTE_RELEASE_TARGET:-tonglingyu-rqa}"
RELEASE_OPERATOR="${TONGLINGYU_REMOTE_RELEASE_OPERATOR:-codex-release-automation}"
ARTIFACT_ROOT="${TONGLINGYU_REMOTE_RELEASE_AUTOMATION_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/remote-release-automation}"
if [[ "${ARTIFACT_ROOT}" != /* ]]; then
  ARTIFACT_ROOT="${REPO_DIR}/${ARTIFACT_ROOT}"
fi
ARTIFACT_DIR="${TONGLINGYU_REMOTE_RELEASE_AUTOMATION_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${REPO_DIR}/${ARTIFACT_DIR}"
fi
REPORT_PATH="${TONGLINGYU_REMOTE_RELEASE_AUTOMATION_REPORT_PATH:-${ARTIFACT_DIR}/remote-release-automation.json}"
REMOTE_RUN_INFO="${ARTIFACT_DIR}/remote-run-info.json"
REMOTE_STDOUT="${ARTIFACT_DIR}/remote-release-automation.stdout"
REMOTE_STDERR="${ARTIFACT_DIR}/remote-release-automation.stderr"
REMOTE_ARTIFACT_COPY_DIR="${ARTIFACT_DIR}/remote-artifacts"
LOCAL_GIT_COMMIT="$(
  git -C "${REPO_DIR}" rev-parse HEAD 2>/dev/null || printf ''
)"
LOCAL_GIT_TRACKED_DIRTY="false"
if [[ -n "$(git -C "${REPO_DIR}" status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
  LOCAL_GIT_TRACKED_DIRTY="true"
fi

mkdir -p "${ARTIFACT_DIR}" "${REMOTE_ARTIFACT_COPY_DIR}"

set +e
ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  'sh -s' -- "${REMOTE_PROJECT_ARG}" "${RUN_ID}" <<'REMOTE' \
  >"${REMOTE_RUN_INFO}.tmp" 2>"${ARTIFACT_DIR}/remote-run-info.stderr"
set -eu
project_dir_arg="$1"
run_id="$2"
if [ "${project_dir_arg}" != "__DEFAULT_REMOTE_PROJECT_DIR__" ]; then
  project_dir="${project_dir_arg}"
else
  project_dir="${DEPLOY_NODE_PROJECT_DIR:-$HOME/hermes-home-deploy}"
fi
cd "${project_dir}"
if [ ! -f .env ]; then
  printf '{"object":"tonglingyu.remote_release_run_info","status":"failed","error":"deploy_env_missing","secret_values_printed":false}\n'
  exit 1
fi
if [ ! -f .tonglingyu-release-tool-env ]; then
  printf '{"object":"tonglingyu.remote_release_run_info","status":"failed","error":"tool_env_missing","secret_values_printed":false}\n'
  exit 1
fi
set -a
. ./.env >/dev/null 2>&1
set +a
python3 - "${project_dir}" "${run_id}" <<'PY'
import json
import os
import sqlite3
import sys
from pathlib import Path

project_dir = Path(sys.argv[1])
run_id = sys.argv[2]
data_dir_raw = os.environ.get("TONGLINGYU_DATA_DIR") or "./data/tonglingyu"
container_db_path = os.environ.get("TONGLINGYU_DB_PATH") or "/data/tonglingyu.db"
data_dir = Path(data_dir_raw)
if not data_dir.is_absolute():
    data_dir = (project_dir / data_dir).resolve()
host_db = data_dir / Path(container_db_path).name
remote_artifact_dir = project_dir / "data" / "tonglingyu" / "release-artifacts" / run_id
remote_artifact_dir.mkdir(parents=True, exist_ok=True)
restore_refs_env = remote_artifact_dir / "restore-refs.env"
open_p0 = None
restore_ref_available = False
restore_ref_shapes = {}
if host_db.is_file():
    conn = sqlite3.connect(str(host_db))
    conn.row_factory = sqlite3.Row
    try:
        open_failures = conn.execute(
            "select count(*) as c from retrieval_failures "
            "where human_review_status in ('open','in_review')"
        ).fetchone()["c"]
        open_tasks = conn.execute(
            "select count(*) as c from knowledge_governance_tasks "
            "where status in ('open','in_review')"
        ).fetchone()["c"]
        open_p0 = {
            "retrieval_failures": open_failures,
            "governance_tasks": open_tasks,
        }
        row = conn.execute(
            """
            select rf.trace_id, rf.package_id, rf.failure_id, kgt.task_id
            from retrieval_failures rf
            join knowledge_governance_tasks kgt
              on kgt.source_failure_id = rf.failure_id
            where rf.trace_id is not null
              and rf.package_id is not null
              and rf.failure_id is not null
              and kgt.task_id is not null
            order by rf.created_at desc
            limit 1
            """
        ).fetchone()
        if row:
            import shlex

            restore_ref_available = True
            restore_ref_shapes = {key: bool(row[key]) for key in row.keys()}
            restore_refs_env.write_text(
                "\n".join(
                    [
                        "export TONGLINGYU_RQA_RESTORE_DRILL_TRACE_ID="
                        + shlex.quote(row["trace_id"]),
                        "export TONGLINGYU_RQA_RESTORE_DRILL_PACKAGE_ID="
                        + shlex.quote(row["package_id"]),
                        "export TONGLINGYU_RQA_RESTORE_DRILL_FAILURE_ID="
                        + shlex.quote(row["failure_id"]),
                        "export TONGLINGYU_RQA_RESTORE_DRILL_TASK_ID="
                        + shlex.quote(row["task_id"]),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
    finally:
        conn.close()
payload = {
    "object": "tonglingyu.remote_release_run_info",
    "schema_version": 1,
    "status": "ok" if host_db.is_file() else "failed",
    "project_dir": str(project_dir),
    "remote_artifact_dir": str(remote_artifact_dir),
    "host_db_path": str(host_db),
    "host_db_exists": host_db.is_file(),
    "open_p0": open_p0,
    "restore_ref_available": restore_ref_available,
    "restore_ref_shapes": restore_ref_shapes,
    "restore_refs_env_path": str(restore_refs_env) if restore_refs_env.is_file() else "",
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if host_db.is_file() else 1)
PY
REMOTE
run_info_exit=$?
set -e
if [[ "${run_info_exit}" -ne 0 ]]; then
  cp "${REMOTE_RUN_INFO}.tmp" "${REMOTE_RUN_INFO}" 2>/dev/null || true
  python3 - "${REPORT_PATH}" "${REMOTE_RUN_INFO}" "${ARTIFACT_DIR}/remote-run-info.stderr" \
    "${REMOTE_HOST}" "${RUN_ID}" "${run_info_exit}" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

report_path, run_info_path, stderr_path, remote_host, run_id, exit_raw = sys.argv[1:7]
payload = {
    "object": "tonglingyu.remote_release_automation",
    "schema_version": 1,
    "status": "failed",
    "production_ready_proven": False,
    "run_id": run_id,
    "remote_host": remote_host,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "errors": [f"remote_run_info_exit={exit_raw}"],
    "artifacts": {
        "remote_run_info_path": run_info_path,
        "remote_run_info_stderr_path": stderr_path,
    },
    "secret_values_printed": False,
}
Path(report_path).write_text(
    json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
PY
  exit 1
fi
mv "${REMOTE_RUN_INFO}.tmp" "${REMOTE_RUN_INFO}"

remote_artifact_dir="$(
  python3 - "${REMOTE_RUN_INFO}" <<'PY'
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload.get("remote_artifact_dir") or "")
PY
)"
if [[ -z "${remote_artifact_dir}" ]]; then
  echo "remote artifact dir missing from run info" >&2
  exit 1
fi
remote_project_dir="$(
  python3 - "${REMOTE_RUN_INFO}" <<'PY'
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload.get("project_dir") or "")
PY
)"

SECURITY_EVIDENCE_REPORT="${ARTIFACT_DIR}/remote-security-evidence.json"
SECURITY_EVIDENCE_STDERR="${ARTIFACT_DIR}/remote-security-evidence.stderr"
security_dependency_scan_remote=""
security_image_scan_remote=""
set +e
TONGLINGYU_REMOTE_SECURITY_REMOTE_ARTIFACT_DIR="${remote_artifact_dir}" \
TONGLINGYU_REMOTE_SECURITY_ARTIFACT_DIR="${ARTIFACT_DIR}/security-evidence" \
TONGLINGYU_REMOTE_SECURITY_REPORT_PATH="${SECURITY_EVIDENCE_REPORT}" \
TONGLINGYU_REMOTE_SECURITY_RUN_ID="${RUN_ID}" \
  "${SCRIPT_DIR}/prepare-tonglingyu-remote-security-evidence.sh" \
  >"${SECURITY_EVIDENCE_REPORT}.tmp" 2>"${SECURITY_EVIDENCE_STDERR}"
security_evidence_exit=$?
set -e
if [[ -s "${SECURITY_EVIDENCE_REPORT}.tmp" ]]; then
  mv "${SECURITY_EVIDENCE_REPORT}.tmp" "${SECURITY_EVIDENCE_REPORT}"
fi
if [[ -f "${SECURITY_EVIDENCE_REPORT}" ]]; then
  security_dependency_scan_remote="$(
    python3 - "${SECURITY_EVIDENCE_REPORT}" <<'PY'
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print((payload.get("dependency_scan") or {}).get("remote_path") or "")
PY
  )"
  security_image_scan_remote="$(
    python3 - "${SECURITY_EVIDENCE_REPORT}" <<'PY'
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print((payload.get("image_scan") or {}).get("remote_path") or "")
PY
  )"
fi

set +e
ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  'sh -s' -- "${REMOTE_PROJECT_ARG}" "${RUN_ID}" "${RELEASE_ENVIRONMENT}" \
  "${RELEASE_TARGET}" "${RELEASE_OPERATOR}" "${LOCAL_GIT_COMMIT}" "${LOCAL_GIT_TRACKED_DIRTY}" \
  "${security_dependency_scan_remote}" "${security_image_scan_remote}" <<'REMOTE' \
  >"${REMOTE_STDOUT}" 2>"${REMOTE_STDERR}"
set -eu
project_dir_arg="$1"
run_id="$2"
release_environment="$3"
release_target="$4"
release_operator="$5"
source_git_commit="$6"
source_git_tracked_dirty="$7"
security_dependency_scan_path="$8"
security_image_scan_path="$9"
if [ "${project_dir_arg}" != "__DEFAULT_REMOTE_PROJECT_DIR__" ]; then
  project_dir="${project_dir_arg}"
else
  project_dir="${DEPLOY_NODE_PROJECT_DIR:-$HOME/hermes-home-deploy}"
fi
cd "${project_dir}"
. ./.tonglingyu-release-tool-env
set -a
. ./.env >/dev/null 2>&1
set +a
python3 - "${project_dir}" "${run_id}" >"${project_dir}/data/tonglingyu/release-artifacts/${run_id}/remote-live-env.json" <<'PY'
import json
import os
import sys
from pathlib import Path

project_dir = Path(sys.argv[1])
run_id = sys.argv[2]
data_dir_raw = os.environ.get("TONGLINGYU_DATA_DIR") or "./data/tonglingyu"
container_db_path = os.environ.get("TONGLINGYU_DB_PATH") or "/data/tonglingyu.db"
data_dir = Path(data_dir_raw)
if not data_dir.is_absolute():
    data_dir = (project_dir / data_dir).resolve()
host_db = data_dir / Path(container_db_path).name
remote_artifact_dir = project_dir / "data" / "tonglingyu" / "release-artifacts" / run_id
payload = {
    "object": "tonglingyu.remote_live_release_env",
    "status": "ok",
    "host_db_path": str(host_db),
    "remote_artifact_dir": str(remote_artifact_dir),
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
PY
remote_artifact_dir="${project_dir}/data/tonglingyu/release-artifacts/${run_id}"
host_db="$(
  python3 - "${remote_artifact_dir}/remote-live-env.json" <<'PY'
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")).get("host_db_path") or "")
PY
)"
if [ -f "${remote_artifact_dir}/restore-refs.env" ]; then
  . "${remote_artifact_dir}/restore-refs.env"
fi
export TONGLINGYU_DEPLOY_ENV_FILE="${project_dir}/.env"
export TONGLINGYU_RQA_RELEASE_RUN_ID="${run_id}"
export TONGLINGYU_RQA_RELEASE_AUTOMATION_ARTIFACT_DIR="${remote_artifact_dir}"
export TONGLINGYU_RQA_RELEASE_AUTOMATION_REPORT_PATH="${remote_artifact_dir}/release-automation.json"
export TONGLINGYU_RELEASE_REPORT_PATH="${remote_artifact_dir}/release-readiness.json"
export TONGLINGYU_RQA_RELEASE_VALIDATION_REPORT_PATH="${remote_artifact_dir}/release-readiness-validation.json"
export TONGLINGYU_RELEASE_REQUIRE_LIVE=true
export TONGLINGYU_RELEASE_ENVIRONMENT="${release_environment}"
export TONGLINGYU_RELEASE_TARGET="${release_target}"
export TONGLINGYU_RELEASE_OPERATOR="${release_operator}"
export TONGLINGYU_RELEASE_GIT_COMMIT="${source_git_commit}"
export TONGLINGYU_RELEASE_GIT_TRACKED_DIRTY="${source_git_tracked_dirty}"
export TONGLINGYU_RQA_DB_PATH="${host_db}"
export TONGLINGYU_RQA_MIGRATION_PREFLIGHT_DB_PATH="${host_db}"
export TONGLINGYU_RQA_MIGRATION_PREFLIGHT_BACKUP_PATH="${remote_artifact_dir}/pre-migration-backup.db"
export TONGLINGYU_RQA_RESTORE_DRILL_DB_PATH="${host_db}"
export TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_DIR="${remote_artifact_dir}/restore-drill"
export TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_DIR="${remote_artifact_dir}/security-image-scans"
export TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_RUN_ID="${run_id}"
if [ -n "${security_dependency_scan_path}" ]; then
  export TONGLINGYU_RELEASE_SECURITY_DEPENDENCY_SCAN_PATH="${security_dependency_scan_path}"
fi
if [ -n "${security_image_scan_path}" ]; then
  export TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_PATH="${security_image_scan_path}"
fi
if command -v trivy >/dev/null 2>&1; then
  export TONGLINGYU_RELEASE_SECURITY_RUN_TRIVY=true
fi
"${project_dir}/scripts/verify-tonglingyu-rqa-release-automation.sh"
REMOTE
remote_exit=$?
set -e

RSYNC_STDERR="${ARTIFACT_DIR}/remote-artifact-rsync.stderr"
if ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  "test -d '${remote_artifact_dir}'" >/dev/null 2>&1; then
  rsync -a "${REMOTE_HOST}:${remote_artifact_dir}/" "${REMOTE_ARTIFACT_COPY_DIR}/" \
    2>"${RSYNC_STDERR}" || true
fi

python3 - "${REPORT_PATH}" "${REMOTE_RUN_INFO}" "${REMOTE_STDOUT}" "${REMOTE_STDERR}" \
  "${REMOTE_ARTIFACT_COPY_DIR}" "${REMOTE_HOST}" "${remote_artifact_dir}" \
  "${RUN_ID}" "${remote_exit}" "${RSYNC_STDERR}" "${security_evidence_exit}" \
  "${SECURITY_EVIDENCE_REPORT}" "${SECURITY_EVIDENCE_STDERR}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    run_info_path_raw,
    stdout_path_raw,
    stderr_path_raw,
    artifact_copy_dir_raw,
    remote_host,
    remote_artifact_dir,
    run_id,
    remote_exit_raw,
    rsync_stderr_raw,
    security_evidence_exit_raw,
    security_evidence_report_raw,
    security_evidence_stderr_raw,
) = sys.argv[1:14]


def file_sha256(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


def last_json_line(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return None
    for line in reversed(path.read_text(encoding="utf-8", errors="replace").splitlines()):
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            return value
    return None


def tail(path_raw, limit=20):
    path = Path(path_raw)
    if not path.is_file():
        return []
    return path.read_text(encoding="utf-8", errors="replace").splitlines()[-limit:]


secret_needles = (
    "api-key=",
    "api_key=",
    "apikey=",
    "authorization:",
    "bearer ",
    "password" + "=",
    "secret" + "=",
    "sk-",
    "token" + "=",
)
combined_text = "\n".join(tail(stdout_path_raw, 200) + tail(stderr_path_raw, 200))
secret_values_printed = any(needle in combined_text.lower() for needle in secret_needles)

artifact_copy_dir = Path(artifact_copy_dir_raw)
automation_report = load_json(artifact_copy_dir / "release-automation.json")
release_report = load_json(artifact_copy_dir / "release-readiness.json")
validator_report = load_json(artifact_copy_dir / "release-readiness-validation.json")
if automation_report is None:
    automation_report = last_json_line(stdout_path_raw)
run_info = load_json(run_info_path_raw) or {}
remote_exit = int(remote_exit_raw)
errors = []
if remote_exit != 0:
    errors.append(f"remote_release_automation_exit={remote_exit}")
if not isinstance(automation_report, dict):
    errors.append("automation_report_missing")
if not artifact_copy_dir.is_dir() or not any(artifact_copy_dir.iterdir()):
    errors.append("remote_artifacts_not_copied")
if secret_values_printed:
    errors.append("secret_like_value_in_output")
production_ready = (
    remote_exit == 0
    and isinstance(automation_report, dict)
    and automation_report.get("production_ready") is True
    and isinstance(release_report, dict)
    and release_report.get("production_release_ready") is True
    and isinstance(validator_report, dict)
    and validator_report.get("status") == "ok"
    and not secret_values_printed
)
payload = {
    "object": "tonglingyu.remote_release_automation",
    "schema_version": 1,
    "status": "ok" if production_ready else "failed",
    "production_ready_proven": production_ready,
    "run_id": run_id,
    "remote_host": remote_host,
    "remote_artifact_dir": remote_artifact_dir,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "remote_run_info": {
        "path": str(Path(run_info_path_raw)),
        "sha256": file_sha256(run_info_path_raw),
        "open_p0": run_info.get("open_p0"),
        "restore_ref_available": run_info.get("restore_ref_available"),
        "host_db_exists": run_info.get("host_db_exists"),
    },
    "checks": {
        "remote_exit": remote_exit,
        "security_evidence_exit": int(security_evidence_exit_raw),
        "automation_report_present": isinstance(automation_report, dict),
        "release_report_present": isinstance(release_report, dict),
        "validator_report_present": isinstance(validator_report, dict),
        "remote_artifacts_copied": artifact_copy_dir.is_dir() and any(artifact_copy_dir.iterdir()),
        "secret_values_printed": secret_values_printed,
    },
    "gate_summary": (
        automation_report.get("gate_summary")
        if isinstance(automation_report, dict)
        else {}
    ),
    "automation_errors": (
        automation_report.get("errors")
        if isinstance(automation_report, dict)
        else []
    ),
    "release_blockers": (
        release_report.get("release_blockers")
        if isinstance(release_report, dict)
        else []
    ),
    "artifacts": {
        "local_artifact_dir": str(Path(report_path_raw).parent),
        "remote_artifact_copy_dir": str(artifact_copy_dir),
        "remote_stdout_path": stdout_path_raw,
        "remote_stdout_sha256": file_sha256(stdout_path_raw),
        "remote_stderr_path": stderr_path_raw,
        "remote_stderr_sha256": file_sha256(stderr_path_raw),
        "release_automation_path": str(artifact_copy_dir / "release-automation.json"),
        "release_automation_sha256": file_sha256(artifact_copy_dir / "release-automation.json"),
        "release_readiness_path": str(artifact_copy_dir / "release-readiness.json"),
        "release_readiness_sha256": file_sha256(artifact_copy_dir / "release-readiness.json"),
        "validator_path": str(artifact_copy_dir / "release-readiness-validation.json"),
        "validator_sha256": file_sha256(artifact_copy_dir / "release-readiness-validation.json"),
        "rsync_stderr_path": rsync_stderr_raw,
        "security_evidence_path": security_evidence_report_raw,
        "security_evidence_sha256": file_sha256(security_evidence_report_raw),
        "security_evidence_stderr_path": security_evidence_stderr_raw,
        "security_evidence_stderr_sha256": file_sha256(security_evidence_stderr_raw),
    },
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
Path(report_path_raw).write_text(encoded + "\n", encoding="utf-8")
print(encoded)
raise SystemExit(0 if production_ready else 1)
PY
