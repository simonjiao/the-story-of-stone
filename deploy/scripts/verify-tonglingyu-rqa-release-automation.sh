#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
WORK_DIR="$(mktemp -d)"
KEEP_WORK_DIR="${TONGLINGYU_RQA_RELEASE_AUTOMATION_KEEP_WORK_DIR:-false}"

cleanup() {
  if [[ "${KEEP_WORK_DIR}" == "true" ]]; then
    printf 'release_automation_work_dir=%s\n' "${WORK_DIR}" >&2
  else
    rm -rf "${WORK_DIR}"
  fi
}
trap cleanup EXIT

RUN_ID="${TONGLINGYU_RQA_RELEASE_RUN_ID:-}"
if [[ -z "${RUN_ID}" ]]; then
  RUN_ID="local-$(date -u +%Y%m%dT%H%M%SZ)-$$"
fi
ARTIFACT_ROOT="${TONGLINGYU_RQA_RELEASE_AUTOMATION_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/release-artifacts}"
if [[ "${ARTIFACT_ROOT}" != /* ]]; then
  ARTIFACT_ROOT="${REPO_DIR}/${ARTIFACT_ROOT}"
fi
ARTIFACT_DIR="${TONGLINGYU_RQA_RELEASE_AUTOMATION_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${REPO_DIR}/${ARTIFACT_DIR}"
fi
REPORT_PATH="${TONGLINGYU_RQA_RELEASE_AUTOMATION_REPORT_PATH:-${ARTIFACT_DIR}/release-automation.json}"
GIT_COMMIT="${TONGLINGYU_RELEASE_GIT_COMMIT:-}"
if [[ -z "${GIT_COMMIT}" ]]; then
  GIT_COMMIT="$(
    git -C "${REPO_DIR}" rev-parse HEAD 2>/dev/null || printf 'unknown'
  )"
fi
RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:-${ARTIFACT_DIR}/release-readiness.json}"
VALIDATION_REPORT_PATH="${TONGLINGYU_RQA_RELEASE_VALIDATION_REPORT_PATH:-${ARTIFACT_DIR}/release-readiness-validation.json}"
CONTRACT_STDOUT="${WORK_DIR}/contract-smoke.stdout"
CONTRACT_STDERR="${WORK_DIR}/contract-smoke.stderr"
LIVE_CAPACITY_STDOUT="${WORK_DIR}/live-capacity-load-smoke.stdout"
LIVE_CAPACITY_STDERR="${WORK_DIR}/live-capacity-load-smoke.stderr"
READINESS_STDOUT="${WORK_DIR}/release-readiness.stdout"
READINESS_STDERR="${WORK_DIR}/release-readiness.stderr"
POST_RELEASE_OPS_STDOUT="${WORK_DIR}/post-release-ops.stdout"
POST_RELEASE_OPS_STDERR="${WORK_DIR}/post-release-ops.stderr"
VALIDATOR_STDOUT="${WORK_DIR}/saved-report-validator.stdout"
VALIDATOR_STDERR="${WORK_DIR}/saved-report-validator.stderr"
mkdir -p "${ARTIFACT_DIR}"
for artifact_path in "${REPORT_PATH}" "${RELEASE_REPORT_PATH}" "${VALIDATION_REPORT_PATH}"; do
  mkdir -p "$(dirname -- "${artifact_path}")"
done

contract_status="failed"
if "${SCRIPT_DIR}/test-tonglingyu-release-readiness-contract.sh" \
  >"${CONTRACT_STDOUT}" 2>"${CONTRACT_STDERR}"; then
  contract_status="passed"
fi

live_capacity_status="not_run"
live_capacity_report_path="${ARTIFACT_DIR}/rqa-live-capacity-load-smoke.json"
live_capacity_artifact_dir="${ARTIFACT_DIR}/live-capacity-load"
if [[ "${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]] \
  && [[ "${TONGLINGYU_RQA_RELEASE_GENERATE_LIVE_CAPACITY_EVIDENCE:-true}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
  live_capacity_status="failed"
  if env \
    "TONGLINGYU_RQA_LIVE_CAPACITY_ARTIFACT_DIR=${live_capacity_artifact_dir}" \
    "TONGLINGYU_RQA_LIVE_CAPACITY_REPORT_PATH=${live_capacity_report_path}" \
    "${SCRIPT_DIR}/verify-tonglingyu-rqa-live-capacity-load-smoke.sh" \
    >"${LIVE_CAPACITY_STDOUT}" 2>"${LIVE_CAPACITY_STDERR}"; then
    live_capacity_status="passed"
  fi
  live_capacity_env="${live_capacity_artifact_dir}/live-capacity-load.env"
  if [[ -f "${live_capacity_env}" ]]; then
    # shellcheck disable=SC1090
    . "${live_capacity_env}"
  fi
fi

readiness_status="failed"
if env "TONGLINGYU_RELEASE_REPORT_PATH=${RELEASE_REPORT_PATH}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" \
  >"${READINESS_STDOUT}" 2>"${READINESS_STDERR}"; then
  readiness_status="passed"
fi

post_release_ops_status="not_run"
post_release_ops_env_path="${ARTIFACT_DIR}/post-release-ops/post-release-ops.env"
if [[ "${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]] \
  && [[ "${TONGLINGYU_RQA_RELEASE_GENERATE_POST_RELEASE_OPS_EVIDENCE:-true}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
  post_release_ops_status="failed"
  if env \
    "TONGLINGYU_POST_RELEASE_OPS_ARTIFACT_DIR=${ARTIFACT_DIR}/post-release-ops" \
    "TONGLINGYU_POST_RELEASE_OPS_ENV_PATH=${post_release_ops_env_path}" \
    "${SCRIPT_DIR}/generate-tonglingyu-post-release-ops-evidence.sh" \
    >"${POST_RELEASE_OPS_STDOUT}" 2>"${POST_RELEASE_OPS_STDERR}"; then
    post_release_ops_status="passed"
    if [[ -f "${post_release_ops_env_path}" ]]; then
      # shellcheck disable=SC1090
      . "${post_release_ops_env_path}"
      readiness_status="failed"
      second_preflight_backup="${ARTIFACT_DIR}/pre-migration-backup-post-release-ops.db"
      second_restore_drill_dir="${ARTIFACT_DIR}/restore-drill-post-release-ops"
      if env "TONGLINGYU_RELEASE_REPORT_PATH=${RELEASE_REPORT_PATH}" \
        "TONGLINGYU_RQA_MIGRATION_PREFLIGHT_BACKUP_PATH=${second_preflight_backup}" \
        "TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_DIR=${second_restore_drill_dir}" \
        "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" \
        >"${READINESS_STDOUT}" 2>"${READINESS_STDERR}"; then
        readiness_status="passed"
      fi
    fi
  fi
fi

validator_status="not_run"
if [[ -f "${RELEASE_REPORT_PATH}" ]]; then
  validator_status="failed"
  if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
    "${RELEASE_REPORT_PATH}" >"${VALIDATOR_STDOUT}" 2>"${VALIDATOR_STDERR}"; then
    validator_status="passed"
  fi
fi

python3 - "${REPORT_PATH}" "${ARTIFACT_DIR}" "${WORK_DIR}" \
  "${RUN_ID}" "${GIT_COMMIT}" \
  "${contract_status}" "${live_capacity_status}" "${readiness_status}" \
  "${post_release_ops_status}" "${validator_status}" \
  "${RELEASE_REPORT_PATH}" "${VALIDATION_REPORT_PATH}" \
  "${CONTRACT_STDOUT}" "${CONTRACT_STDERR}" \
  "${LIVE_CAPACITY_STDOUT}" "${LIVE_CAPACITY_STDERR}" "${live_capacity_report_path}" \
  "${READINESS_STDOUT}" "${READINESS_STDERR}" \
  "${POST_RELEASE_OPS_STDOUT}" "${POST_RELEASE_OPS_STDERR}" "${post_release_ops_env_path}" \
  "${VALIDATOR_STDOUT}" "${VALIDATOR_STDERR}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    artifact_dir_raw,
    work_dir_raw,
    run_id,
    git_commit,
    contract_status,
    live_capacity_status,
    readiness_status,
    post_release_ops_status,
    validator_status,
    release_report_path_raw,
    validation_report_path_raw,
    contract_stdout_raw,
    contract_stderr_raw,
    live_capacity_stdout_raw,
    live_capacity_stderr_raw,
    live_capacity_report_path_raw,
    readiness_stdout_raw,
    readiness_stderr_raw,
    post_release_ops_stdout_raw,
    post_release_ops_stderr_raw,
    post_release_ops_env_path_raw,
    validator_stdout_raw,
    validator_stderr_raw,
) = sys.argv[1:25]


def tail(path_raw, limit=20):
    path = Path(path_raw)
    if not path.is_file():
        return []
    return path.read_text(encoding="utf-8", errors="replace").splitlines()[-limit:]


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


def write_json(path_raw, value):
    if not path_raw or not isinstance(value, dict):
        return False
    path = Path(path_raw)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=True, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return True


def path_is_outside_work_dir(path_raw):
    if not path_raw:
        return False
    path = Path(path_raw)
    if not path.is_file():
        return False
    try:
        resolved = path.resolve()
        work_dir = Path(work_dir_raw).resolve()
    except OSError:
        return False
    try:
        resolved.relative_to(work_dir)
        return False
    except ValueError:
        return True


def target_is_outside_work_dir(path_raw):
    if not path_raw:
        return False
    try:
        resolved = Path(path_raw).resolve()
        work_dir = Path(work_dir_raw).resolve()
    except OSError:
        return False
    try:
        resolved.relative_to(work_dir)
        return False
    except ValueError:
        return True


release_report = load_json(release_report_path_raw)
validator_report = None
validator_lines = tail(validator_stdout_raw, 1)
if validator_lines:
    try:
        validator_report = json.loads(validator_lines[-1])
    except json.JSONDecodeError:
        validator_report = None
validation_report_written = write_json(validation_report_path_raw, validator_report)
release_report_sha256 = file_sha256(release_report_path_raw)
validation_report_sha256 = file_sha256(validation_report_path_raw)
release_report_persistent = path_is_outside_work_dir(release_report_path_raw)
validation_report_persistent = path_is_outside_work_dir(validation_report_path_raw)
automation_report_persistent = target_is_outside_work_dir(report_path_raw)

production_ready = (
    contract_status == "passed"
    and live_capacity_status != "failed"
    and readiness_status == "passed"
    and validator_status == "passed"
    and isinstance(release_report, dict)
    and release_report.get("production_release_ready") is True
    and isinstance(validator_report, dict)
    and validator_report.get("status") == "ok"
    and bool(release_report_sha256)
    and bool(validation_report_sha256)
    and release_report_persistent
    and validation_report_persistent
    and automation_report_persistent
)
errors = []
if contract_status != "passed":
    errors.append("contract_smoke_failed")
if readiness_status != "passed":
    errors.append("release_readiness_failed")
if post_release_ops_status == "failed":
    errors.append("post_release_ops_evidence_failed")
if live_capacity_status == "failed":
    errors.append("live_capacity_load_smoke_failed")
if validator_status != "passed":
    errors.append("saved_report_validator_failed")
if isinstance(release_report, dict):
    if release_report.get("production_release_ready") is not True:
        errors.append("release_report_not_production_ready")
    for blocker in release_report.get("release_blockers") or []:
        if isinstance(blocker, str):
            errors.append(f"release_blocker={blocker}")
else:
    errors.append("release_report_missing_or_invalid")
if not release_report_sha256:
    errors.append("release_report_artifact_missing")
if not validation_report_written or not validation_report_sha256:
    errors.append("validation_report_artifact_missing")
if not release_report_persistent:
    errors.append("release_report_artifact_not_persistent")
if not validation_report_persistent:
    errors.append("validation_report_artifact_not_persistent")
if not automation_report_persistent:
    errors.append("automation_report_artifact_not_persistent")

payload = {
    "object": "tonglingyu.rqa_release_automation",
    "schema_version": 1,
    "status": "ok" if production_ready else "failed",
    "automation_policy_version": "tonglingyu-rqa-release-automation-v1",
    "production_ready": production_ready,
    "run_id": run_id,
    "git_commit": git_commit,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "artifact_dir": str(Path(artifact_dir_raw)),
    "checks": {
        "contract_smoke": contract_status,
        "live_capacity_load_smoke": live_capacity_status,
        "post_release_ops_evidence": post_release_ops_status,
        "release_readiness": readiness_status,
        "saved_report_validator": validator_status,
    },
    "artifacts": {
        "automation_report_path": str(Path(report_path_raw)) if report_path_raw else "",
        "artifact_dir": str(Path(artifact_dir_raw)),
        "release_report_path": str(Path(release_report_path_raw)),
        "release_report_sha256": release_report_sha256,
        "validation_report_path": str(Path(validation_report_path_raw)),
        "validation_report_sha256": validation_report_sha256,
        "release_manifest_digest": (
            release_report.get("release_manifest_digest")
            if isinstance(release_report, dict)
            else ""
        ),
        "release_artifact_registry_digest": (
            release_report.get("release_artifact_registry_digest")
            if isinstance(release_report, dict)
            else ""
        ),
        "release_artifact_registry_entry_count": (
            len(
                (
                    (
                        release_report.get("release_artifact_registry")
                        if isinstance(release_report, dict)
                        else {}
                    )
                    or {}
                ).get("entries") or []
            )
        ),
        "validator_stdout_sha256": file_sha256(validator_stdout_raw),
        "contract_stdout_sha256": file_sha256(contract_stdout_raw),
        "live_capacity_stdout_sha256": file_sha256(live_capacity_stdout_raw),
        "live_capacity_report_path": str(Path(live_capacity_report_path_raw)),
        "live_capacity_report_sha256": file_sha256(live_capacity_report_path_raw),
        "post_release_ops_stdout_sha256": file_sha256(post_release_ops_stdout_raw),
        "post_release_ops_env_path": str(Path(post_release_ops_env_path_raw)),
        "post_release_ops_env_sha256": file_sha256(post_release_ops_env_path_raw),
        "readiness_stdout_sha256": file_sha256(readiness_stdout_raw),
        "artifact_persistence": {
            "release_report_persistent": release_report_persistent,
            "validation_report_persistent": validation_report_persistent,
            "automation_report_persistent": automation_report_persistent,
            "validation_report_written": validation_report_written,
        },
    },
    "validator_summary": validator_report if isinstance(validator_report, dict) else {},
    "gate_summary": {
        "required_failures": (
            release_report.get("required_failures")
            if isinstance(release_report, dict)
            else []
        ),
        "skipped_live_gates": (
            release_report.get("skipped_live_gates")
            if isinstance(release_report, dict)
            else []
        ),
        "release_blockers": (
            release_report.get("release_blockers")
            if isinstance(release_report, dict)
            else []
        ),
    },
    "tails": {
        "contract_stderr": tail(contract_stderr_raw),
        "live_capacity_stderr": tail(live_capacity_stderr_raw),
        "post_release_ops_stderr": tail(post_release_ops_stderr_raw),
        "readiness_stderr": tail(readiness_stderr_raw),
        "validator_stderr": tail(validator_stderr_raw),
    },
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path_raw:
    report_path = Path(report_path_raw)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
raise SystemExit(0 if production_ready else 1)
PY
