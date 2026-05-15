#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
REPORT_PATH="${TONGLINGYU_RQA_RELEASE_AUTOMATION_REPORT_PATH:-}"
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
GIT_COMMIT="$(
  git -C "${REPO_DIR}" rev-parse HEAD 2>/dev/null || printf 'unknown'
)"
RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:-${WORK_DIR}/release-readiness.json}"
VALIDATION_REPORT_PATH="${WORK_DIR}/release-readiness-validation.json"
CONTRACT_STDOUT="${WORK_DIR}/contract-smoke.stdout"
CONTRACT_STDERR="${WORK_DIR}/contract-smoke.stderr"
READINESS_STDOUT="${WORK_DIR}/release-readiness.stdout"
READINESS_STDERR="${WORK_DIR}/release-readiness.stderr"
VALIDATOR_STDOUT="${WORK_DIR}/saved-report-validator.stdout"
VALIDATOR_STDERR="${WORK_DIR}/saved-report-validator.stderr"

contract_status="failed"
if "${SCRIPT_DIR}/test-tonglingyu-release-readiness-contract.sh" \
  >"${CONTRACT_STDOUT}" 2>"${CONTRACT_STDERR}"; then
  contract_status="passed"
fi

readiness_status="failed"
if env "TONGLINGYU_RELEASE_REPORT_PATH=${RELEASE_REPORT_PATH}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" \
  >"${READINESS_STDOUT}" 2>"${READINESS_STDERR}"; then
  readiness_status="passed"
fi

validator_status="not_run"
if [[ -f "${RELEASE_REPORT_PATH}" ]]; then
  validator_status="failed"
  if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
    "${RELEASE_REPORT_PATH}" >"${VALIDATOR_STDOUT}" 2>"${VALIDATOR_STDERR}"; then
    validator_status="passed"
  fi
fi

python3 - "${REPORT_PATH}" "${RUN_ID}" "${GIT_COMMIT}" \
  "${contract_status}" "${readiness_status}" "${validator_status}" \
  "${RELEASE_REPORT_PATH}" "${VALIDATION_REPORT_PATH}" \
  "${CONTRACT_STDOUT}" "${CONTRACT_STDERR}" \
  "${READINESS_STDOUT}" "${READINESS_STDERR}" \
  "${VALIDATOR_STDOUT}" "${VALIDATOR_STDERR}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    run_id,
    git_commit,
    contract_status,
    readiness_status,
    validator_status,
    release_report_path_raw,
    validation_report_path_raw,
    contract_stdout_raw,
    contract_stderr_raw,
    readiness_stdout_raw,
    readiness_stderr_raw,
    validator_stdout_raw,
    validator_stderr_raw,
) = sys.argv[1:15]


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


release_report = load_json(release_report_path_raw)
validator_report = None
if validator_status == "passed":
    validator_lines = tail(validator_stdout_raw, 1)
    if validator_lines:
        try:
            validator_report = json.loads(validator_lines[-1])
        except json.JSONDecodeError:
            validator_report = None

production_ready = (
    contract_status == "passed"
    and readiness_status == "passed"
    and validator_status == "passed"
    and isinstance(release_report, dict)
    and release_report.get("production_release_ready") is True
    and isinstance(validator_report, dict)
    and validator_report.get("status") == "ok"
)
errors = []
if contract_status != "passed":
    errors.append("contract_smoke_failed")
if readiness_status != "passed":
    errors.append("release_readiness_failed")
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

payload = {
    "object": "tonglingyu.rqa_release_automation",
    "schema_version": 1,
    "status": "ok" if production_ready else "failed",
    "automation_policy_version": "tonglingyu-rqa-release-automation-v1",
    "production_ready": production_ready,
    "run_id": run_id,
    "git_commit": git_commit,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "checks": {
        "contract_smoke": contract_status,
        "release_readiness": readiness_status,
        "saved_report_validator": validator_status,
    },
    "artifacts": {
        "release_report_path": str(Path(release_report_path_raw)),
        "release_report_sha256": file_sha256(release_report_path_raw),
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
        "readiness_stdout_sha256": file_sha256(readiness_stdout_raw),
    },
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
