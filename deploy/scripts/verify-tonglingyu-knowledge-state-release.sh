#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

RUN_ID="${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_RUN_ID:-local-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
ARTIFACT_ROOT="${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/knowledge-state-release}"
ARTIFACT_DIR="${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
REPORT_PATH="${1:-${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_REPORT_PATH:-${TONGLINGYU_RELEASE_REPORT_PATH:-${ARTIFACT_DIR}/release-readiness.json}}}"
VALIDATION_PATH="${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_VALIDATION_PATH:-${ARTIFACT_DIR}/release-readiness-validation.json}"
SUMMARY_PATH="${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_SUMMARY_PATH:-${ARTIFACT_DIR}/knowledge-state-release-summary.json}"
READINESS_STDOUT="${ARTIFACT_DIR}/release-readiness.stdout"
READINESS_STDERR="${ARTIFACT_DIR}/release-readiness.stderr"
VALIDATOR_STDERR="${ARTIFACT_DIR}/release-readiness-validation.stderr"
RUN_READINESS="${TONGLINGYU_KNOWLEDGE_STATE_RELEASE_RUN_READINESS:-true}"

mkdir -p "${ARTIFACT_DIR}"
mkdir -p "$(dirname -- "${REPORT_PATH}")"
mkdir -p "$(dirname -- "${VALIDATION_PATH}")"
mkdir -p "$(dirname -- "${SUMMARY_PATH}")"

if [[ "${RUN_READINESS}" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
  readiness_status="failed"
  if env "TONGLINGYU_RELEASE_REPORT_PATH=${REPORT_PATH}" \
    "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" \
    >"${READINESS_STDOUT}" 2>"${READINESS_STDERR}"; then
    readiness_status="passed"
  fi
elif [[ ! -f "${REPORT_PATH}" ]]; then
  echo "release report not found: ${REPORT_PATH}" >&2
  exit 1
else
  readiness_status="not_run"
fi

"${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${REPORT_PATH}" >"${VALIDATION_PATH}" 2>"${VALIDATOR_STDERR}"

python3 - "${REPORT_PATH}" "${VALIDATION_PATH}" "${SUMMARY_PATH}" \
  "${RUN_ID}" "${READINESS_STDOUT}" "${READINESS_STDERR}" \
  "${VALIDATOR_STDERR}" "${readiness_status}" <<'PY'
import json
import os
import sys
from pathlib import Path

(
    report_path_raw,
    validation_path_raw,
    summary_path_raw,
    run_id,
    readiness_stdout_raw,
    readiness_stderr_raw,
    validator_stderr_raw,
    readiness_status,
) = sys.argv[1:9]
report_path = Path(report_path_raw)
validation_path = Path(validation_path_raw)
summary_path = Path(summary_path_raw)
errors = []


def load_json(path, label):
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        errors.append(f"{label}_unreadable")
        return {"_error": str(exc)}


def is_sha256(value):
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(ch in "0123456789abcdef" for ch in value)
    )


report = load_json(report_path, "release_report")
validation = load_json(validation_path, "release_report_validation")
if validation.get("status") != "ok":
    errors.append("saved_report_validator_failed")
if readiness_status == "failed":
    errors.append("release_readiness_failed")

manifest = report.get("release_manifest")
if not isinstance(manifest, dict):
    errors.append("release_manifest_missing")
    manifest = {}
knowledge_state = manifest.get("knowledge_state")
if not isinstance(knowledge_state, dict):
    errors.append("release_manifest_knowledge_state_missing")
    knowledge_state = {}

required_knowledge_state_fields = [
    "state_summary_sha256",
    "runtime_policy_version",
    "calibration_job_summary",
    "kb_diff_report_id",
    "kb_diff_report_sha256",
    "kb_diff_sha256",
    "eval_diff_sha256",
    "eval_impact_sha256",
]
for field in required_knowledge_state_fields:
    value = knowledge_state.get(field)
    if value in (None, "", [], {}):
        errors.append(f"knowledge_state_{field}_missing")
for field in [
    "state_summary_sha256",
    "kb_diff_report_sha256",
    "kb_diff_sha256",
    "eval_diff_sha256",
    "eval_impact_sha256",
]:
    if not is_sha256(knowledge_state.get(field)):
        errors.append(f"knowledge_state_{field}_invalid")
if not isinstance(knowledge_state.get("calibration_job_summary"), dict):
    errors.append("knowledge_state_calibration_job_summary_invalid")
elif knowledge_state["calibration_job_summary"].get("failed_or_retry_waiting") not in (0, None):
    errors.append("knowledge_state_calibration_jobs_failed_or_retry_waiting")

registry = report.get("release_artifact_registry")
if not isinstance(registry, dict):
    errors.append("release_artifact_registry_missing")
    registry = {}
entries = {
    entry.get("name"): entry
    for entry in registry.get("entries") or []
    if isinstance(entry, dict)
}
for name in [
    "knowledge_state_summary",
    "kb_version_diff_report",
    "knowledge_state_eval_impact",
]:
    entry = entries.get(name)
    if not isinstance(entry, dict):
        errors.append(f"artifact_registry_{name}_missing")
        continue
    if entry.get("source_gate") != "retrieval_quality":
        errors.append(f"artifact_registry_{name}_source_gate_invalid")
    if entry.get("required_for_production") is not True:
        errors.append(f"artifact_registry_{name}_not_required")
    if not is_sha256(entry.get("digest_sha256")):
        errors.append(f"artifact_registry_{name}_digest_invalid")

require_live = str(os.environ.get("TONGLINGYU_RELEASE_REQUIRE_LIVE", "")).lower()
if require_live in {"1", "true", "yes", "on"} and report.get("production_release_ready") is not True:
    errors.append("live_release_not_production_ready")

summary = {
    "object": "tonglingyu.knowledge_state_release_gate",
    "schema_version": 1,
    "status": "passed" if not errors else "failed",
    "run_id": run_id,
    "report_path": str(report_path),
    "validation_path": str(validation_path),
    "readiness_stdout_path": readiness_stdout_raw,
    "readiness_stderr_path": readiness_stderr_raw,
    "release_readiness_status": readiness_status,
    "validator_stderr_path": validator_stderr_raw,
    "production_release_ready": bool(report.get("production_release_ready")),
    "release_manifest_digest": report.get("release_manifest_digest"),
    "release_artifact_registry_digest": report.get("release_artifact_registry_digest"),
    "knowledge_state": {
        "state_summary_sha256": knowledge_state.get("state_summary_sha256"),
        "kb_diff_report_sha256": knowledge_state.get("kb_diff_report_sha256"),
        "eval_impact_sha256": knowledge_state.get("eval_impact_sha256"),
        "runtime_policy_version": knowledge_state.get("runtime_policy_version"),
    },
    "saved_report_validator_status": validation.get("status"),
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
