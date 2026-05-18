#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${ROOT}/.." && pwd)"
CARGO_BIN="${CARGO:-cargo}"
RUN_ID="${TONGLINGYU_CALIBRATION_JOB_SMOKE_RUN_ID:-local-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
ARTIFACT_ROOT="${TONGLINGYU_CALIBRATION_JOB_SMOKE_ARTIFACT_ROOT:-${REPO_ROOT}/data/tonglingyu/calibration-job-smoke}"
ARTIFACT_DIR="${TONGLINGYU_CALIBRATION_JOB_SMOKE_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
SUMMARY_PATH="${TONGLINGYU_CALIBRATION_JOB_SMOKE_SUMMARY_PATH:-${ARTIFACT_DIR}/calibration-job-smoke-summary.json}"
JOB_STDOUT="${ARTIFACT_DIR}/calibration-job.stdout"
JOB_STDERR="${ARTIFACT_DIR}/calibration-job.stderr"
FAIL_CLOSED_STDOUT="${ARTIFACT_DIR}/calibration-fail-closed.stdout"
FAIL_CLOSED_STDERR="${ARTIFACT_DIR}/calibration-fail-closed.stderr"

mkdir -p "${ARTIFACT_DIR}"

job_status="failed"
if (
  cd "${ROOT}"
  "${CARGO_BIN}" test -p tonglingyu-runtime \
    knowledge_calibration_job_model_is_idempotent_leased_and_audited \
    -- --exact
) >"${JOB_STDOUT}" 2>"${JOB_STDERR}"; then
  job_status="passed"
fi

fail_closed_status="failed"
if (
  cd "${ROOT}"
  "${CARGO_BIN}" test -p tonglingyu-runtime \
    knowledge_calibration_llm_fake_output_is_report_only_and_privacy_checked \
    -- --exact
) >"${FAIL_CLOSED_STDOUT}" 2>"${FAIL_CLOSED_STDERR}"; then
  fail_closed_status="passed"
fi

python3 - "${SUMMARY_PATH}" "${RUN_ID}" "${JOB_STDOUT}" "${JOB_STDERR}" \
  "${FAIL_CLOSED_STDOUT}" "${FAIL_CLOSED_STDERR}" \
  "${job_status}" "${fail_closed_status}" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

(
    summary_path_raw,
    run_id,
    job_stdout_raw,
    job_stderr_raw,
    fail_stdout_raw,
    fail_stderr_raw,
    job_status,
    fail_closed_status,
) = sys.argv[1:9]
summary_path = Path(summary_path_raw)


def file_sha256(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


errors = []
if job_status != "passed":
    errors.append("calibration_job_lease_retry_audit_test_failed")
if fail_closed_status != "passed":
    errors.append("calibration_llm_fail_closed_test_failed")

summary = {
    "object": "tonglingyu.calibration_job_smoke",
    "schema_version": 1,
    "status": "passed" if not errors else "failed",
    "run_id": run_id,
    "checks": {
        "lease_retry_audit": {
            "status": job_status,
            "stdout_path": job_stdout_raw,
            "stdout_sha256": file_sha256(job_stdout_raw),
            "stderr_path": job_stderr_raw,
            "stderr_sha256": file_sha256(job_stderr_raw),
        },
        "llm_config_fail_closed": {
            "status": fail_closed_status,
            "stdout_path": fail_stdout_raw,
            "stdout_sha256": file_sha256(fail_stdout_raw),
            "stderr_path": fail_stderr_raw,
            "stderr_sha256": file_sha256(fail_stderr_raw),
        },
    },
    "secret_values_printed": False,
    "errors": errors,
}
summary_path.parent.mkdir(parents=True, exist_ok=True)
with summary_path.open("w", encoding="utf-8") as handle:
    json.dump(summary, handle, ensure_ascii=True, sort_keys=True)
    handle.write("\n")
print(json.dumps(summary, ensure_ascii=True, sort_keys=True))
if errors:
    raise SystemExit(1)
PY
