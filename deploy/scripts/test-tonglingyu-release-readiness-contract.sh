#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

PASS_CMD="${WORK_DIR}/gate-pass.sh"
FAIL_CMD="${WORK_DIR}/gate-fail.sh"

cat >"${PASS_CMD}" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
echo '{"status":"ok","source":"mock-gate"}'
SH
chmod +x "${PASS_CMD}"

cat >"${FAIL_CMD}" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
echo 'mock gate failed' >&2
exit 42
SH
chmod +x "${FAIL_CMD}"

assert_report() {
  local report_path="$1"
  local expression="$2"
  python3 - "${report_path}" "${expression}" <<'PY'
import json
import sys

report_path, expression = sys.argv[1:3]
with open(report_path, "r", encoding="utf-8") as handle:
    report = json.load(handle)
if not eval(expression, {"report": report}):
    raise SystemExit(f"assertion failed: {expression}\nreport={json.dumps(report, sort_keys=True)}")
PY
}

common_env=(
  "TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true"
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD=${PASS_CMD}"
)

override_guard_stderr="${WORK_DIR}/override-guard.stderr"
if env \
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null 2>"${override_guard_stderr}"; then
  echo "gate command overrides must require explicit test opt-in" >&2
  exit 1
fi
if ! grep -q "Production release readiness cannot be proven" "${override_guard_stderr}"; then
  echo "override guard did not explain production readiness boundary" >&2
  exit 1
fi

default_report="${WORK_DIR}/default-not-ready.json"
if env "${common_env[@]}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${default_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null; then
  echo "default non-live release readiness must fail" >&2
  exit 1
fi
assert_report "${default_report}" 'report["production_release_ready"] is False'
assert_report "${default_report}" 'report["status"] == "passed_with_skipped_gates"'
assert_report "${default_report}" 'report["exit_policy"] == "production_release_ready"'
assert_report "${default_report}" 'report["gate_command_overrides_used"] is True'
assert_report "${default_report}" '"openwebui_admin_action" in report["skipped_live_gates"]'

optional_report="${WORK_DIR}/summary-optional-failure.json"
env "${common_env[@]}" \
  TONGLINGYU_RELEASE_SUMMARY_ONLY=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_REPORT_PATH="${optional_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${optional_report}" 'report["status"] == "passed_with_failed_optional_gates"'
assert_report "${optional_report}" 'report["optional_failures"] == ["openwebui_browser_review"]'
assert_report "${optional_report}" 'report["browser_review_acknowledged"] is False'

conditions_report="${WORK_DIR}/live-conditions-met.json"
env "${common_env[@]}" \
  TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_SUMMARY_ONLY=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_REPORT_PATH="${conditions_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${conditions_report}" 'report["release_conditions_met"] is True'
assert_report "${conditions_report}" 'report["production_release_ready"] is False'
assert_report "${conditions_report}" 'report["status"] == "passed_with_gate_command_overrides"'
assert_report "${conditions_report}" 'report["exit_policy"] == "summary_only"'
assert_report "${conditions_report}" 'report["browser_review_ref"] == "mock-browser-review"'
assert_report "${conditions_report}" '"gate command overrides were used" in report["release_blockers"]'

failed_report="${WORK_DIR}/live-failed-gate.json"
if env \
  TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true \
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD=${FAIL_CMD}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD=${PASS_CMD}" \
  TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_REPORT_PATH="${failed_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null; then
  echo "live release readiness must fail when strict Gateway gate fails" >&2
  exit 1
fi
assert_report "${failed_report}" 'report["production_release_ready"] is False'
assert_report "${failed_report}" '"strict_gateway" in report["required_failures"]'
assert_report "${failed_report}" 'report["status"] == "failed"'

echo "tonglingyu release readiness contract passed"
