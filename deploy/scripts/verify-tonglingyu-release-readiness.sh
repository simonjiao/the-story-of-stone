#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
WORK_DIR="$(mktemp -d)"
RESULTS_JSONL="${WORK_DIR}/results.jsonl"
READY_STATUS="${WORK_DIR}/production-ready.status"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:-}"
RQA_EVAL_REPORT_OUTPUT_PATH="${TONGLINGYU_RQA_EVAL_REPORT_OUTPUT_PATH:-}"
GATE_CMD_OVERRIDES_USED="false"
if [[ -n "${TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_QUALITY_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_SECURITY_SCAN_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD:-}" ]] \
  || [[ -n "${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_CMD:-}" ]]; then
  GATE_CMD_OVERRIDES_USED="true"
fi
RUNTIME_CONFIG_CMD="${TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD:-${SCRIPT_DIR}/verify-tonglingyu-runtime-config.sh}"
RQA_QUALITY_CMD="${TONGLINGYU_RELEASE_RQA_QUALITY_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-quality-gate.sh}"
RQA_RESTORE_DRILL_CMD="${TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-backup-restore-drill.sh}"
RQA_PERFORMANCE_CMD="${TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-performance-budget.sh}"
RQA_API_CONTRACT_CMD="${TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD:-${SCRIPT_DIR}/verify-tonglingyu-rqa-api-contract.sh}"
SECURITY_SCAN_CMD="${TONGLINGYU_RELEASE_SECURITY_SCAN_CMD:-${SCRIPT_DIR}/verify-tonglingyu-release-security.sh}"
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

failed=0
run_gate "runtime_config" "true" "${RUNTIME_CONFIG_CMD}" || failed=1
if [[ -n "${RQA_EVAL_REPORT_OUTPUT_PATH}" ]]; then
  run_gate "retrieval_quality" "true" env \
    "TONGLINGYU_RQA_EVAL_REPORT_OUTPUT_PATH=${RQA_EVAL_REPORT_OUTPUT_PATH}" \
    "${RQA_QUALITY_CMD}" || failed=1
else
  run_gate "retrieval_quality" "true" "${RQA_QUALITY_CMD}" || failed=1
fi
run_gate "rqa_backup_restore_drill" "true" env \
  "TONGLINGYU_RQA_RESTORE_DRILL_REQUIRE_LIVE=${require_live}" \
  "${RQA_RESTORE_DRILL_CMD}" || failed=1
run_gate "rqa_performance_budget" "true" "${RQA_PERFORMANCE_CMD}" || failed=1
run_gate "rqa_api_contract" "true" "${RQA_API_CONTRACT_CMD}" || failed=1
run_gate "security_scan" "true" "${SECURITY_SCAN_CMD}" || failed=1

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

python3 - "${RESULTS_JSONL}" "${REPORT_PATH}" "${READY_STATUS}" "${require_live}" "${summary_only}" "${browser_review_ref}" "${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE:-}" "${GATE_CMD_OVERRIDES_USED}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    results_path,
    report_path,
    ready_status_path,
    require_live_raw,
    summary_only_raw,
    browser_review_ref,
    browser_review_evidence,
    gate_cmd_overrides_raw,
) = sys.argv[1:9]
require_live = require_live_raw == "true"
summary_only = summary_only_raw == "true"
browser_review_ref = browser_review_ref.strip()
browser_review_evidence = browser_review_evidence.strip()
gate_cmd_overrides_used = gate_cmd_overrides_raw == "true"
with open(results_path, "r", encoding="utf-8") as handle:
    gates = [json.loads(line) for line in handle if line.strip()]

gates_by_name = {gate["name"]: gate for gate in gates}
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
release_conditions_met = (
    require_live
    and not required_failures
    and not skipped_live_gates
    and browser_review_acknowledged
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
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "secret_values_printed": False,
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
