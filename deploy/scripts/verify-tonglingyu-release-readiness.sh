#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
WORK_DIR="$(mktemp -d)"
RESULTS_JSONL="${WORK_DIR}/results.jsonl"
READY_STATUS="${WORK_DIR}/production-ready.status"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:-}"
RQA_EVAL_REPORT_OUTPUT_PATH="${TONGLINGYU_RQA_EVAL_REPORT_OUTPUT_PATH:-}"
RQA_UPSTREAM_MODEL="${TONGLINGYU_UPSTREAM_MODEL:-${AGENT_RUNTIME_HERMES_MODEL:-hermes-agent}}"
GATE_CMD_OVERRIDES_USED="false"
if [[ -n "${TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_MIGRATION_PREFLIGHT_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_QUALITY_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_SECURITY_SCAN_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPS_READINESS_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_INCIDENT_CAPACITY_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_CMD:-}" ]]; then
  GATE_CMD_OVERRIDES_USED="true"
fi
RUNTIME_CONFIG_CMD="${TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD:-${SCRIPT_DIR}/verify-tonglingyu-runtime-config.sh}"
RQA_MIGRATION_PREFLIGHT_CMD="${TONGLINGYU_RELEASE_RQA_MIGRATION_PREFLIGHT_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-migration-preflight.sh}"
RQA_QUALITY_CMD="${TONGLINGYU_RELEASE_RQA_QUALITY_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-quality-gate.sh}"
RQA_RESTORE_DRILL_CMD="${TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-backup-restore-drill.sh}"
RQA_PERFORMANCE_CMD="${TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-performance-budget.sh}"
RQA_API_CONTRACT_CMD="${TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-api-contract.sh}"
RQA_USER_LIFECYCLE_CMD="${TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-user-lifecycle.sh}"
SECURITY_SCAN_CMD="${TONGLINGYU_RELEASE_SECURITY_SCAN_CMD:-${SCRIPT_DIR}/verify-tonglingyu-release-security.sh}"
OPS_READINESS_CMD="${TONGLINGYU_RELEASE_OPS_READINESS_CMD:-${SCRIPT_DIR}/verify-tonglingyu-release-ops-readiness.sh}"
RQA_INCIDENT_CAPACITY_CMD="${TONGLINGYU_RELEASE_RQA_INCIDENT_CAPACITY_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-incident-capacity.sh}"
OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD="${TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD:-${SCRIPT_DIR}/test-openwebui-gateway-admin-action-contract.sh}"
MODEL_UPSTREAM_CMD="${TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD:-${SCRIPT_DIR}/verify-model-upstream-network.sh}"
STRICT_GATEWAY_CMD="${TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD:-${SCRIPT_DIR}/verify-tonglingyu-strict-gateway.sh}"
OPENWEBUI_FUNCTION_CMD="${TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD:-${SCRIPT_DIR}/verify-openwebui-function.sh}"
OPENWEBUI_ADMIN_ACTION_CMD="${TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD:-${SCRIPT_DIR}/verify-openwebui-gateway-admin-action.sh}"
OPENWEBUI_BROWSER_REVIEW_CMD="${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_CMD:-${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh}"
trap 'rm -rf "${WORK_DIR}"' EXIT

if [[ -z "${TONGLINGYU_RELEASE_RQA_QUALITY_CMD:-}" ]] \
  && [[ -z "${TONGLINGYU_RQA_EVAL_REPORT_PATH:-}" ]] \
  && [[ -z "${RQA_EVAL_REPORT_OUTPUT_PATH}" ]] \
  && [[ -n "${REPORT_PATH}" ]]; then
  RQA_EVAL_REPORT_OUTPUT_PATH="$(
    python3 - "${REPORT_PATH}" "${DEPLOY_DIR}" <<'PY'
import sys
from pathlib import Path

report_path = Path(sys.argv[1])
deploy_dir = Path(sys.argv[2])
if not report_path.is_absolute():
    report_path = deploy_dir / report_path
print(str(Path(str(report_path) + ".rqa-eval.json")))
PY
  )"
fi

cd "${DEPLOY_DIR}"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

if [[ "${GATE_CMD_OVERRIDES_USED}" == "true" ]] \
  && ! is_true "${TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE:-false}"; then
  cat >&2 <<'EOF'
release readiness gate command overrides require
TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true and are for local contract tests
only. Production release readiness cannot be proven with overridden gate
commands.
EOF
  exit 2
fi

append_result() {
  local name="$1"
  local status="$2"
  local required="$3"
  local reason="$4"
  local stdout_path="${5:-}"
  local stderr_path="${6:-}"
  python3 - "${RESULTS_JSONL}" "${name}" "${status}" "${required}" "${reason}" \
    "${stdout_path}" "${stderr_path}" <<'PY'
import json
import sys

results_path, name, status, required, reason, stdout_path, stderr_path = sys.argv[1:8]


def tail(path):
    if not path:
        return []
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            return [line.rstrip("\n") for line in handle.readlines()[-20:]]
    except FileNotFoundError:
        return []


with open(results_path, "a", encoding="utf-8") as handle:
    handle.write(json.dumps(
        {
            "name": name,
            "status": status,
            "required": required == "true",
            "reason": reason,
            "stdout_tail": tail(stdout_path),
            "stderr_tail": tail(stderr_path),
        },
        ensure_ascii=True,
        sort_keys=True,
    ))
    handle.write("\n")
PY
}

run_gate() {
  local name="$1"
  local required="$2"
  shift 2
  local stdout_path="${WORK_DIR}/${name}.stdout"
  local stderr_path="${WORK_DIR}/${name}.stderr"
  if "$@" >"${stdout_path}" 2>"${stderr_path}"; then
    append_result "${name}" "passed" "${required}" "" "${stdout_path}" "${stderr_path}"
    return 0
  fi
  append_result "${name}" "failed" "${required}" "" "${stdout_path}" "${stderr_path}"
  if [[ "${required}" == "true" ]]; then
    return 1
  fi
  return 0
}

skip_gate() {
  append_result "$1" "skipped" "$2" "$3"
}

require_live="false"
if is_true "${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}"; then
  require_live="true"
fi
summary_only="false"
if is_true "${TONGLINGYU_RELEASE_SUMMARY_ONLY:-false}"; then
  summary_only="true"
fi
browser_review_ref="${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF:-}"
release_environment="${TONGLINGYU_RELEASE_ENVIRONMENT:-}"
release_target="${TONGLINGYU_RELEASE_TARGET:-}"
release_report_validity_hours="${TONGLINGYU_RELEASE_REPORT_VALIDITY_HOURS:-24}"

failed=0
run_gate "runtime_config" "true" env \
  "TONGLINGYU_RUNTIME_CONFIG_REQUIRE_DOCKER=${require_live}" \
  "${RUNTIME_CONFIG_CMD}" || failed=1
run_gate "rqa_migration_preflight" "true" env \
  "TONGLINGYU_RQA_MIGRATION_PREFLIGHT_REQUIRE_LIVE=${require_live}" \
  "${RQA_MIGRATION_PREFLIGHT_CMD}" || failed=1
if [[ -n "${RQA_EVAL_REPORT_OUTPUT_PATH}" ]]; then
  run_gate "retrieval_quality" "true" env \
    "TONGLINGYU_UPSTREAM_MODEL=${RQA_UPSTREAM_MODEL}" \
    "TONGLINGYU_RQA_EVAL_REPORT_OUTPUT_PATH=${RQA_EVAL_REPORT_OUTPUT_PATH}" \
    "${RQA_QUALITY_CMD}" || failed=1
else
  run_gate "retrieval_quality" "true" env \
    "TONGLINGYU_UPSTREAM_MODEL=${RQA_UPSTREAM_MODEL}" \
    "${RQA_QUALITY_CMD}" || failed=1
fi
run_gate "rqa_backup_restore_drill" "true" env \
  "TONGLINGYU_RQA_RESTORE_DRILL_REQUIRE_LIVE=${require_live}" \
  "${RQA_RESTORE_DRILL_CMD}" || failed=1
run_gate "rqa_performance_budget" "true" "${RQA_PERFORMANCE_CMD}" || failed=1
run_gate "rqa_api_contract" "true" "${RQA_API_CONTRACT_CMD}" || failed=1
run_gate "rqa_user_lifecycle" "true" "${RQA_USER_LIFECYCLE_CMD}" || failed=1
run_gate "security_scan" "true" "${SECURITY_SCAN_CMD}" || failed=1
run_gate "release_ops_readiness" "true" env \
  "TONGLINGYU_RELEASE_OPS_REQUIRE_LIVE=${require_live}" \
  "${OPS_READINESS_CMD}" || failed=1
run_gate "rqa_incident_capacity" "true" env \
  "TONGLINGYU_RQA_INCIDENT_CAPACITY_REQUIRE_LIVE=${require_live}" \
  "${RQA_INCIDENT_CAPACITY_CMD}" || failed=1
run_gate "openwebui_admin_action_contract" "true" \
  "${OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD}" || failed=1

verify_strict_gateway="false"
if [[ "${require_live}" == "true" ]] || is_true "${TONGLINGYU_RELEASE_VERIFY_STRICT_GATEWAY:-false}"; then
  verify_strict_gateway="true"
fi

verify_model_upstream="false"
if [[ "${require_live}" == "true" ]] || is_true "${TONGLINGYU_RELEASE_VERIFY_MODEL_UPSTREAM:-false}"; then
  verify_model_upstream="true"
fi

if [[ "${verify_model_upstream}" == "true" ]]; then
  run_gate "model_upstream_network" "true" \
    "${MODEL_UPSTREAM_CMD}" || failed=1
else
  skip_gate "model_upstream_network" "false" \
    "set TONGLINGYU_RELEASE_VERIFY_MODEL_UPSTREAM=true or TONGLINGYU_RELEASE_REQUIRE_LIVE=true"
fi

if [[ "${verify_strict_gateway}" == "true" ]]; then
  run_gate "strict_gateway" "true" \
    "${STRICT_GATEWAY_CMD}" || failed=1
else
  skip_gate "strict_gateway" "false" \
    "set TONGLINGYU_RELEASE_VERIFY_STRICT_GATEWAY=true or TONGLINGYU_RELEASE_REQUIRE_LIVE=true"
fi

verify_openwebui_function="false"
if [[ "${require_live}" == "true" ]] || is_true "${TONGLINGYU_RELEASE_VERIFY_OPENWEBUI_FUNCTION:-false}"; then
  verify_openwebui_function="true"
fi

if [[ "${verify_openwebui_function}" == "true" ]]; then
  run_gate "openwebui_function" "true" \
    "${OPENWEBUI_FUNCTION_CMD}" || failed=1
else
  skip_gate "openwebui_function" "false" \
    "set TONGLINGYU_RELEASE_VERIFY_OPENWEBUI_FUNCTION=true or TONGLINGYU_RELEASE_REQUIRE_LIVE=true"
fi

verify_openwebui_admin_action="false"
if [[ "${require_live}" == "true" ]] || is_true "${TONGLINGYU_RELEASE_VERIFY_OPENWEBUI_ADMIN_ACTION:-false}"; then
  verify_openwebui_admin_action="true"
fi

if [[ "${verify_openwebui_admin_action}" == "true" ]]; then
  run_gate "openwebui_admin_action" "true" \
    "${OPENWEBUI_ADMIN_ACTION_CMD}" || failed=1
else
  skip_gate "openwebui_admin_action" "false" \
    "set TONGLINGYU_RELEASE_VERIFY_OPENWEBUI_ADMIN_ACTION=true or TONGLINGYU_RELEASE_REQUIRE_LIVE=true"
fi

if is_true "${TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW:-false}"; then
  if [[ -z "${browser_review_ref//[[:space:]]/}" ]]; then
    append_result "openwebui_browser_review" "failed" "${require_live}" \
      "set TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF with the browser review evidence reference"
    if [[ "${require_live}" == "true" ]]; then
      failed=1
    fi
  elif [[ -z "${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE:-}" ]]; then
    append_result "openwebui_browser_review" "failed" "${require_live}" \
      "set TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE to a validated browser review JSON report"
    if [[ "${require_live}" == "true" ]]; then
      failed=1
    fi
  else
    run_gate "openwebui_browser_review" "${require_live}" \
      "${OPENWEBUI_BROWSER_REVIEW_CMD}" || failed=1
  fi
elif [[ "${require_live}" == "true" ]]; then
  append_result "openwebui_browser_review" "failed" "true" \
    "set TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true, TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF, and TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE after browser-side review"
  failed=1
else
  skip_gate "openwebui_browser_review" "false" \
    "set TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true, TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF, and TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE after browser-side review"
fi

python3 - "${RESULTS_JSONL}" "${REPORT_PATH}" "${READY_STATUS}" \
  "${require_live}" "${summary_only}" "${browser_review_ref}" \
  "${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE:-}" \
  "${GATE_CMD_OVERRIDES_USED}" "${REPO_DIR}" "${release_environment}" \
  "${release_target}" "${release_report_validity_hours}" <<'PY'
import json
import hashlib
import os
import subprocess
import sys
from datetime import datetime, timedelta, timezone

(
    results_path,
    report_path,
    ready_status_path,
    require_live_raw,
    summary_only_raw,
    browser_review_ref,
    browser_review_evidence,
    gate_cmd_overrides_raw,
    repo_dir,
    release_environment_raw,
    release_target_raw,
    release_report_validity_hours_raw,
) = sys.argv[1:13]
require_live = require_live_raw == "true"
summary_only = summary_only_raw == "true"
browser_review_ref = browser_review_ref.strip()
browser_review_evidence = browser_review_evidence.strip()
gate_cmd_overrides_used = gate_cmd_overrides_raw == "true"
release_generated_at = datetime.now(timezone.utc)
release_environment_raw = release_environment_raw.strip()
release_environment = release_environment_raw or ("live" if require_live else "local")
release_target = release_target_raw.strip()
release_context_errors = []
try:
    release_report_validity_hours = float(release_report_validity_hours_raw)
except ValueError:
    release_report_validity_hours = 0.0
if release_report_validity_hours <= 0:
    release_context_errors.append("release report validity hours must be positive")
if require_live and not release_environment_raw:
    release_context_errors.append("live release environment was not provided")
if require_live and release_environment.lower() in {"local", "preflight", "test", "fixture"}:
    release_context_errors.append("live release environment must identify target environment")
release_valid_until = release_generated_at + timedelta(
    hours=max(release_report_validity_hours, 0.0),
)
release_context = {
    "object": "tonglingyu.release_context",
    "schema_version": 1,
    "policy_version": "tonglingyu-release-context-v1",
    "environment": release_environment,
    "environment_explicit": bool(release_environment_raw),
    "target": release_target,
    "require_live": require_live,
    "generated_at": release_generated_at.isoformat(),
    "valid_until": release_valid_until.isoformat(),
    "validity_hours": release_report_validity_hours,
    "context_source": "env",
    "valid": not release_context_errors,
    "errors": release_context_errors,
    "secret_values_printed": False,
}
with open(results_path, "r", encoding="utf-8") as handle:
    gates = [json.loads(line) for line in handle if line.strip()]

gates_by_name = {gate["name"]: gate for gate in gates}


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def canonical_digest(value):
    encoded = json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    return sha256_bytes(encoded.encode("utf-8"))


def success_json_from_gate_stdout(name, expected_object=None):
    gate = gates_by_name.get(name) or {}
    for line in reversed(gate.get("stdout_tail") or []):
        try:
            candidate = json.loads(line)
        except (TypeError, json.JSONDecodeError):
            continue
        if not isinstance(candidate, dict) or candidate.get("status") != "ok":
            continue
        if expected_object and candidate.get("object") != expected_object:
            continue
        return candidate
    return None


def git_output(args):
    try:
        completed = subprocess.run(
            ["git", "-C", repo_dir, *args],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return ""
    return completed.stdout.strip()


git_commit_override = os.environ.get("TONGLINGYU_RELEASE_GIT_COMMIT", "").strip()
git_tracked_dirty_override = os.environ.get(
    "TONGLINGYU_RELEASE_GIT_TRACKED_DIRTY",
    "",
).strip().lower()


def git_commit():
    return git_commit_override or git_output(["rev-parse", "HEAD"])


def git_tracked_dirty():
    if git_tracked_dirty_override in {"1", "true", "yes", "on"}:
        return True
    if git_tracked_dirty_override in {"0", "false", "no", "off"}:
        return False
    return bool(git_output(["status", "--porcelain", "--untracked-files=no"]))


def build_release_manifest():
    runtime_config = success_json_from_gate_stdout("runtime_config") or {}
    rqa_gate = success_json_from_gate_stdout(
        "retrieval_quality",
        "tonglingyu.rqa_quality_gate",
    ) or {}
    migration_gate = success_json_from_gate_stdout(
        "rqa_migration_preflight",
        "tonglingyu.rqa_migration_preflight_gate",
    ) or {}
    security_gate = success_json_from_gate_stdout(
        "security_scan",
        "tonglingyu.release_security_gate",
    ) or {}
    dependency_scan = security_gate.get("dependency_scan") or {}
    image_scan = security_gate.get("image_scan") or {}
    behavior_config = rqa_gate.get("behavior_config") or {}
    kb_version = rqa_gate.get("kb_version") or {}
    source_license = rqa_gate.get("source_license_summary") or {}
    knowledge_state = rqa_gate.get("knowledge_state_summary") or {}
    kb_diff_report = rqa_gate.get("kb_diff_report") or {}
    eval_impact = rqa_gate.get("eval_impact") or {}
    migration_preflight = migration_gate.get("migration_preflight") or {}
    migration_counts = migration_gate.get("migration_counts") or {}
    migration_backup = migration_gate.get("backup") or {}
    migration_db = migration_gate.get("db") or {}
    manifest = {
        "object": "tonglingyu.release_manifest",
        "schema_version": 1,
        "git": {
            "commit": git_commit(),
            "tracked_dirty": git_tracked_dirty(),
        },
        "runtime_config": {
            "config_digest": canonical_digest(runtime_config) if runtime_config else "",
            "config_mode": runtime_config.get("config_mode"),
            "checked_policy_fields": runtime_config.get("checked_policy_fields"),
            "checked_services": runtime_config.get("checked_services"),
        },
        "rqa": {
            "rqa_schema_version": rqa_gate.get("rqa_schema_version"),
            "eval_suite_version": rqa_gate.get("eval_suite_version"),
            "eval_run_id": rqa_gate.get("eval_run_id"),
            "eval_report_sha256": rqa_gate.get("eval_report_sha256"),
            "source_snapshot_digest": rqa_gate.get("source_snapshot_digest"),
            "kb_build_hash": rqa_gate.get("kb_build_hash"),
            "kb_version": {
                "version_id": kb_version.get("version_id"),
                "schema_version": kb_version.get("schema_version"),
                "source_count": kb_version.get("source_count"),
                "block_count": kb_version.get("block_count"),
                "built_at": kb_version.get("built_at"),
            },
            "source_license_summary_digest": (
                canonical_digest(source_license) if source_license else ""
            ),
        },
        "knowledge_state": {
            "state_summary_sha256": rqa_gate.get("knowledge_state_summary_sha256"),
            "runtime_policy_version": knowledge_state.get("runtime_policy_version"),
            "state_counts": knowledge_state.get("state_counts"),
            "per_kind_coverage_matrix": rqa_gate.get("per_kind_coverage_matrix"),
            "calibration_job_summary": rqa_gate.get("calibration_job_summary"),
            "runtime_policy_promotion_summary": rqa_gate.get(
                "runtime_policy_promotion_summary"
            ),
            "unresolved_calibration_gaps": rqa_gate.get("unresolved_calibration_gaps"),
            "kb_diff_report_id": kb_diff_report.get("report_id"),
            "kb_diff_report_sha256": rqa_gate.get("kb_diff_report_sha256"),
            "kb_diff_sha256": kb_diff_report.get("diff_sha256"),
            "eval_diff_sha256": kb_diff_report.get("eval_diff_sha256"),
            "eval_impact_sha256": canonical_digest(eval_impact) if eval_impact else "",
            "open_p0_governance_tasks": rqa_gate.get("open_p0_governance_tasks"),
        },
        "migration": {
            "policy_version": migration_gate.get("policy_version"),
            "mode": migration_gate.get("mode"),
            "require_live": migration_gate.get("require_live"),
            "source_mode": migration_gate.get("source_mode"),
            "source_db_sha256": migration_db.get("source_db_sha256"),
            "backup_artifact_path": migration_backup.get("artifact_path"),
            "backup_artifact_path_sha256": migration_backup.get("artifact_path_sha256"),
            "backup_artifact_sha256": migration_backup.get("artifact_sha256"),
            "migration_preflight_sha256": (
                canonical_digest(migration_preflight) if migration_preflight else ""
            ),
            "required_migration_count": migration_counts.get("required"),
            "applied_migration_count": migration_counts.get("applied"),
            "pending_migration_count": migration_counts.get("pending"),
        },
        "behavior_config": {
            "behavior_config_digest": behavior_config.get("behavior_config_digest"),
            "runtime_profile_digest": behavior_config.get("runtime_profile_digest"),
            "prompt_digest": behavior_config.get("prompt_digest"),
            "tool_policy_digest": behavior_config.get("tool_policy_digest"),
            "reviewer_policy_digest": behavior_config.get("reviewer_policy_digest"),
            "gateway_policy_digest": behavior_config.get("gateway_policy_digest"),
            "model_upstream_id": behavior_config.get("model_upstream_id"),
            "model_upstream_bound_by_gate": behavior_config.get("model_upstream_bound_by_gate"),
            "decoding_parameters_source": behavior_config.get("decoding_parameters_source"),
            "decoding_parameters_summary": behavior_config.get("decoding_parameters_summary"),
        },
        "security": {
            "dependency_scan_sha256": dependency_scan.get("report_sha256"),
            "image_count": image_scan.get("image_count"),
            "image_refs": image_scan.get("image_refs"),
            "image_refs_sha256": image_scan.get("image_refs_sha256"),
            "digest_missing_count": image_scan.get("digest_missing_count"),
            "mutable_tag_count": image_scan.get("mutable_tag_count"),
            "scanned_image_count": image_scan.get("scanned_image_count"),
            "scanned_image_refs_sha256": image_scan.get("scanned_image_refs_sha256"),
            "scanned_report_count": image_scan.get("scanned_report_count"),
            "scanned_reports_sha256": image_scan.get("scanned_reports_sha256"),
            "raw_reports_persistent": image_scan.get("raw_reports_persistent"),
            "raw_report_artifact_dir": image_scan.get("raw_report_artifact_dir"),
            "raw_report_paths_sha256": image_scan.get("raw_report_paths_sha256"),
        },
    }
    return manifest


release_manifest = build_release_manifest()
release_manifest_digest = canonical_digest(release_manifest)
release_context_digest = canonical_digest(release_context)


def build_release_runtime_identity():
    security_gate = success_json_from_gate_stdout(
        "security_scan",
        "tonglingyu.release_security_gate",
    ) or {}
    strict_gate = success_json_from_gate_stdout("strict_gateway") or {}
    migration_gate = success_json_from_gate_stdout(
        "rqa_migration_preflight",
        "tonglingyu.rqa_migration_preflight_gate",
    ) or {}
    image_scan = security_gate.get("image_scan") or {}
    running_images = strict_gate.get("running_images") or {}
    running_image_items = (
        running_images.get("images")
        if isinstance(running_images.get("images"), list)
        else []
    )
    migration_preflight = migration_gate.get("migration_preflight") or {}
    migration_counts = migration_gate.get("migration_counts") or {}
    identity_errors = []
    if require_live and not running_image_items:
        identity_errors.append("live running image inventory was not captured")
    if require_live and migration_gate.get("mode") != "live":
        identity_errors.append("live migration preflight was not captured")
    if require_live and migration_counts.get("pending") != 0:
        identity_errors.append("pending migrations must be zero for live release")
    if require_live and git_tracked_dirty():
        identity_errors.append("tracked worktree must be clean for live release")
    return {
        "object": "tonglingyu.release_runtime_identity",
        "schema_version": 1,
        "policy_version": "tonglingyu-release-runtime-identity-v1",
        "require_live": require_live,
        "git": {
            "commit": git_commit(),
            "tracked_dirty": git_tracked_dirty(),
        },
        "image_inventory": {
            "source_gate": "security_scan",
            "image_count": image_scan.get("image_count"),
            "image_refs": image_scan.get("image_refs"),
            "image_refs_sha256": image_scan.get("image_refs_sha256"),
            "digest_missing_count": image_scan.get("digest_missing_count"),
            "mutable_tag_count": image_scan.get("mutable_tag_count"),
            "scanned_image_count": image_scan.get("scanned_image_count"),
            "scanned_report_count": image_scan.get("scanned_report_count"),
            "scanned_reports_sha256": image_scan.get("scanned_reports_sha256"),
            "raw_reports_persistent": image_scan.get("raw_reports_persistent"),
            "raw_report_artifact_dir": image_scan.get("raw_report_artifact_dir"),
            "raw_report_paths_sha256": image_scan.get("raw_report_paths_sha256"),
        },
        "running_images": {
            "source_gate": "strict_gateway",
            "inventory": running_images,
            "inventory_sha256": canonical_digest(running_images) if running_images else "",
            "image_count": len(running_image_items),
        },
        "migration": {
            "source_gate": "rqa_migration_preflight",
            "policy_version": migration_gate.get("policy_version"),
            "mode": migration_gate.get("mode"),
            "source_mode": migration_gate.get("source_mode"),
            "source_db_sha256": (migration_gate.get("db") or {}).get("source_db_sha256"),
            "preflight_sha256": (
                canonical_digest(migration_preflight) if migration_preflight else ""
            ),
            "backup_artifact_sha256": (migration_gate.get("backup") or {}).get("artifact_sha256"),
            "required_migration_count": migration_counts.get("required"),
            "applied_migration_count": migration_counts.get("applied"),
            "pending_migration_count": migration_counts.get("pending"),
        },
        "valid": not identity_errors,
        "errors": identity_errors,
        "secret_values_printed": False,
    }


release_runtime_identity = build_release_runtime_identity()
release_runtime_identity_digest = canonical_digest(release_runtime_identity)
browser_review_validation = None
browser_review_gate = gates_by_name.get("openwebui_browser_review") or {}
for line in reversed(browser_review_gate.get("stdout_tail") or []):
    try:
        candidate = json.loads(line)
    except json.JSONDecodeError:
        continue
    if (
        candidate.get("object") == "tonglingyu.openwebui_browser_review_gate"
        and candidate.get("status") == "ok"
    ):
        browser_review_validation = candidate
        break

browser_review_gate_passed = (
    browser_review_gate.get("name") == "openwebui_browser_review"
    and browser_review_gate.get("status") == "passed"
)
browser_review_validation_missing = (
    browser_review_gate_passed and browser_review_validation is None
)
verified_browser_review_evidence = browser_review_evidence
if isinstance(browser_review_validation, dict):
    validation_evidence_path = browser_review_validation.get("evidence_path")
    if isinstance(validation_evidence_path, str) and validation_evidence_path.strip():
        verified_browser_review_evidence = validation_evidence_path.strip()


def build_release_artifact_registry():
    rqa_gate = success_json_from_gate_stdout(
        "retrieval_quality",
        "tonglingyu.rqa_quality_gate",
    ) or {}
    manifest_rqa = release_manifest.get("rqa") or {}
    manifest_knowledge_state = release_manifest.get("knowledge_state") or {}
    manifest_runtime = release_manifest.get("runtime_config") or {}
    manifest_migration = release_manifest.get("migration") or {}
    manifest_behavior = release_manifest.get("behavior_config") or {}
    manifest_security = release_manifest.get("security") or {}
    entries = []

    def add_entry(
        name,
        artifact_type,
        digest,
        source_gate,
        *,
        ref="",
        path="",
        retention_class="release_evidence",
        required_for_production=True,
    ):
        entries.append({
            "name": name,
            "artifact_type": artifact_type,
            "digest_sha256": digest or "",
            "source_gate": source_gate,
            "ref": ref or "",
            "path": path or "",
            "retention_class": retention_class,
            "required_for_production": required_for_production,
        })

    add_entry(
        "release_manifest",
        "inline_json",
        release_manifest_digest,
        "release_readiness",
        ref="release_manifest",
        retention_class="release_manifest",
    )
    add_entry(
        "release_context",
        "inline_json",
        release_context_digest,
        "release_readiness",
        ref=release_context.get("environment"),
        retention_class="release_manifest",
    )
    add_entry(
        "release_runtime_identity",
        "inline_json",
        release_runtime_identity_digest,
        "release_readiness",
        ref=release_runtime_identity.get("git", {}).get("commit"),
        retention_class="release_manifest",
    )
    add_entry(
        "runtime_config",
        "gate_stdout",
        manifest_runtime.get("config_digest"),
        "runtime_config",
        ref="runtime_config",
    )
    add_entry(
        "rqa_eval_report",
        "local_file",
        manifest_rqa.get("eval_report_sha256"),
        "retrieval_quality",
        ref=manifest_rqa.get("eval_run_id"),
        path=rqa_gate.get("eval_report_path") or "",
    )
    add_entry(
        "source_license_summary",
        "inline_json",
        manifest_rqa.get("source_license_summary_digest"),
        "retrieval_quality",
        ref=manifest_rqa.get("source_snapshot_digest"),
    )
    add_entry(
        "knowledge_state_summary",
        "inline_json",
        manifest_knowledge_state.get("state_summary_sha256"),
        "retrieval_quality",
        ref=manifest_knowledge_state.get("runtime_policy_version"),
    )
    add_entry(
        "kb_version_diff_report",
        "inline_json",
        manifest_knowledge_state.get("kb_diff_report_sha256"),
        "retrieval_quality",
        ref=manifest_knowledge_state.get("kb_diff_report_id"),
    )
    add_entry(
        "knowledge_state_eval_impact",
        "inline_json",
        manifest_knowledge_state.get("eval_impact_sha256"),
        "retrieval_quality",
        ref=manifest_rqa.get("eval_run_id"),
    )
    add_entry(
        "migration_preflight",
        "inline_json",
        manifest_migration.get("migration_preflight_sha256"),
        "rqa_migration_preflight",
        ref=manifest_migration.get("policy_version"),
    )
    add_entry(
        "migration_backup",
        "sqlite_backup",
        manifest_migration.get("backup_artifact_sha256"),
        "rqa_migration_preflight",
        ref=manifest_migration.get("source_db_sha256"),
        path=manifest_migration.get("backup_artifact_path") or "",
    )
    add_entry(
        "behavior_config",
        "inline_json",
        manifest_behavior.get("behavior_config_digest"),
        "retrieval_quality",
        ref=manifest_behavior.get("model_upstream_id"),
    )
    add_entry(
        "dependency_scan",
        "scan_report",
        manifest_security.get("dependency_scan_sha256"),
        "security_scan",
        ref="cargo-audit",
    )
    add_entry(
        "image_inventory",
        "inline_json",
        manifest_security.get("image_refs_sha256"),
        "security_scan",
        ref=f"images:{manifest_security.get('image_count') or 0}",
    )
    add_entry(
        "image_scan_reports",
        "scan_report_collection",
        manifest_security.get("scanned_reports_sha256"),
        "security_scan",
        ref=manifest_security.get("raw_report_paths_sha256")
        or f"reports:{manifest_security.get('scanned_report_count') or 0}",
        path=manifest_security.get("raw_report_artifact_dir") or "",
    )
    if isinstance(browser_review_validation, dict):
        add_entry(
            "browser_review_evidence",
            "local_file",
            browser_review_validation.get("evidence_sha256"),
            "openwebui_browser_review",
            ref=browser_review_ref,
            path=verified_browser_review_evidence,
        )

    return {
        "object": "tonglingyu.release_artifact_registry",
        "schema_version": 1,
        "policy_version": "tonglingyu-release-artifact-registry-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "retention_days": 365,
        "legal_hold_supported": True,
        "entries": entries,
        "secret_values_printed": False,
    }


release_artifact_registry = build_release_artifact_registry()
release_artifact_registry_digest = canonical_digest(release_artifact_registry)

live_gate_names = [
    "model_upstream_network",
    "strict_gateway",
    "openwebui_function",
    "openwebui_admin_action",
]
required_failures = [
    gate["name"]
    for gate in gates
    if gate["required"] and gate["status"] != "passed"
]
optional_failures = [
    gate["name"]
    for gate in gates
    if not gate["required"] and gate["status"] == "failed"
]
if browser_review_validation_missing:
    if browser_review_gate.get("required"):
        required_failures.append("openwebui_browser_review_validation")
    else:
        optional_failures.append("openwebui_browser_review_validation")
skipped = [gate["name"] for gate in gates if gate["status"] == "skipped"]
skipped_live_gates = [
    name
    for name in live_gate_names
    if (gates_by_name.get(name) or {}).get("status") == "skipped"
]
failed_live_gates = [
    name
    for name in live_gate_names
    if (gates_by_name.get(name) or {}).get("status") == "failed"
]
status = "failed" if required_failures else "passed"
if status == "passed" and optional_failures:
    status = "passed_with_failed_optional_gates"
elif status == "passed" and skipped:
    status = "passed_with_skipped_gates"
elif status == "passed" and gate_cmd_overrides_used:
    status = "passed_with_gate_command_overrides"
elif status == "passed" and summary_only:
    status = "passed_in_summary_only_mode"
browser_review_acknowledged = (
    browser_review_gate_passed and browser_review_validation is not None
)
manual_checks = [] if browser_review_acknowledged else [
    "Open WebUI browser-side ordinary-user model visibility",
    "Open WebUI browser-side admin audit entry visibility",
    "Open WebUI streaming chat UX against the live public endpoint",
    "Human confirmation that existing Open WebUI webui.db persisted settings match env-rendered provider settings",
]
release_blockers = []
if not require_live:
    release_blockers.append("live release mode was not required")
for name in required_failures:
    release_blockers.append(f"required gate did not pass: {name}")
for name in skipped_live_gates:
    release_blockers.append(f"live gate was skipped: {name}")
for name in failed_live_gates:
    if name not in required_failures:
        release_blockers.append(f"live gate failed: {name}")
if browser_review_validation_missing:
    release_blockers.append("Open WebUI browser-side review validation summary was missing")
if not browser_review_acknowledged:
    release_blockers.append("Open WebUI browser-side review was not acknowledged")
if summary_only:
    release_blockers.append("summary-only mode was used")
for error in release_context_errors:
    release_blockers.append(error)
for error in release_runtime_identity.get("errors") or []:
    if require_live:
        release_blockers.append(error)
release_conditions_met = (
    require_live
    and not required_failures
    and not skipped_live_gates
    and browser_review_acknowledged
    and release_context["valid"]
    and release_runtime_identity["valid"]
)
if gate_cmd_overrides_used:
    release_blockers.append("gate command overrides were used")
production_release_ready = (
    release_conditions_met and not gate_cmd_overrides_used and not summary_only
)

report = {
    "object": "tonglingyu.release_readiness_report",
    "schema_version": 1,
    "status": status,
    "production_release_ready": production_release_ready,
    "release_conditions_met": release_conditions_met,
    "require_live": require_live,
    "summary_only": summary_only,
    "exit_policy": "summary_only" if summary_only else "production_release_ready",
    "gate_command_overrides_used": gate_cmd_overrides_used,
    "browser_review_acknowledged": browser_review_acknowledged,
    "browser_review_ref": browser_review_ref,
    "browser_review_evidence": verified_browser_review_evidence,
    "browser_review_validation": browser_review_validation,
    "generated_at": release_context["generated_at"],
    "release_context": release_context,
    "release_context_digest": release_context_digest,
    "release_runtime_identity": release_runtime_identity,
    "release_runtime_identity_digest": release_runtime_identity_digest,
    "secret_values_printed": False,
    "release_manifest": release_manifest,
    "release_manifest_digest": release_manifest_digest,
    "release_artifact_registry": release_artifact_registry,
    "release_artifact_registry_digest": release_artifact_registry_digest,
    "gates": gates,
    "required_failures": required_failures,
    "optional_failures": optional_failures,
    "skipped_live_gates": skipped_live_gates,
    "failed_live_gates": failed_live_gates,
    "release_blockers": release_blockers,
    "remaining_manual_checks": manual_checks,
}
encoded = json.dumps(report, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path:
    with open(report_path, "w", encoding="utf-8") as handle:
        handle.write(encoded)
        handle.write("\n")
with open(ready_status_path, "w", encoding="utf-8") as handle:
    handle.write("true\n" if production_release_ready else "false\n")
PY

if [[ "${failed}" -ne 0 ]]; then
  exit 1
fi
if [[ "${summary_only}" != "true" ]] && [[ "$(cat "${READY_STATUS}")" != "true" ]]; then
  exit 1
fi
