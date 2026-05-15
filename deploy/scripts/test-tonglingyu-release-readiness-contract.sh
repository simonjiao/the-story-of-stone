#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

PASS_CMD="${WORK_DIR}/gate-pass.sh"
FAIL_CMD="${WORK_DIR}/gate-fail.sh"
BROWSER_NO_VALIDATION_CMD="${WORK_DIR}/browser-gate-no-validation.sh"
MODEL_UPSTREAM_FAKE_DOCKER="${WORK_DIR}/model-upstream-fake-docker.sh"
MODEL_UPSTREAM_FAKE_COUNTER="${WORK_DIR}/model-upstream-fake-counter"
BROWSER_EVIDENCE_JSON="${WORK_DIR}/browser-review-evidence.json"
MISSING_ARTIFACT_EVIDENCE_JSON="${WORK_DIR}/missing-artifact-browser-review-evidence.json"
MISMATCH_PUBLIC_URL_EVIDENCE_JSON="${WORK_DIR}/mismatch-public-url-browser-review-evidence.json"
EXTRA_CHECK_EVIDENCE_JSON="${WORK_DIR}/extra-check-browser-review-evidence.json"
STALE_BROWSER_EVIDENCE_JSON="${WORK_DIR}/stale-browser-review-evidence.json"
GENERATED_BROWSER_EVIDENCE_JSON="${WORK_DIR}/generated-browser-review-evidence.json"
BROWSER_PREFLIGHT_EVIDENCE_JSON="${WORK_DIR}/preflight-browser-review-evidence.json"
TAMPERED_READY_REPORT="${WORK_DIR}/tampered-ready-report.json"
TAMPERED_DERIVED_REPORT="${WORK_DIR}/tampered-derived-report.json"
TAMPERED_EXIT_POLICY_REPORT="${WORK_DIR}/tampered-exit-policy-report.json"
TAMPERED_TAIL_SHAPE_REPORT="${WORK_DIR}/tampered-tail-shape-report.json"
TAMPERED_MISSING_GENERATED_REPORT="${WORK_DIR}/tampered-missing-generated-report.json"
TAMPERED_MISSING_GATE_REPORT="${WORK_DIR}/tampered-missing-gate-report.json"
TAMPERED_EXTRA_GATE_REPORT="${WORK_DIR}/tampered-extra-gate-report.json"
TAMPERED_SECRET_REPORT="${WORK_DIR}/tampered-secret-report.json"
SYNTHETIC_READY_REPORT="${WORK_DIR}/synthetic-ready-report.json"
TAMPERED_STALE_READY_REPORT="${WORK_DIR}/tampered-stale-ready-report.json"
TAMPERED_PRODUCTION_FLAG_REPORT="${WORK_DIR}/tampered-production-flag-report.json"
TAMPERED_LIVE_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-live-gate-stdout-report.json"
TAMPERED_RQA_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-gate-stdout-report.json"
TAMPERED_RQA_RESTORE_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-restore-gate-stdout-report.json"
TAMPERED_RQA_RESTORE_FIXTURE_REPORT="${WORK_DIR}/tampered-rqa-restore-fixture-report.json"
TAMPERED_RQA_RESTORE_RTO_REPORT="${WORK_DIR}/tampered-rqa-restore-rto-report.json"
TAMPERED_SECURITY_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-security-gate-stdout-report.json"
TAMPERED_SECURITY_GATE_RISK_REPORT="${WORK_DIR}/tampered-security-gate-risk-report.json"
TAMPERED_SECURITY_GATE_SCRIPT_REPORT="${WORK_DIR}/tampered-security-gate-script-report.json"
TAMPERED_RQA_PERFORMANCE_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-performance-gate-stdout-report.json"
TAMPERED_RQA_PERFORMANCE_BUDGET_REPORT="${WORK_DIR}/tampered-rqa-performance-budget-report.json"
TAMPERED_RQA_PERFORMANCE_CHECK_REPORT="${WORK_DIR}/tampered-rqa-performance-check-report.json"
TAMPERED_RQA_API_CONTRACT_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-api-contract-stdout-report.json"
TAMPERED_RQA_API_CONTRACT_CHECK_REPORT="${WORK_DIR}/tampered-rqa-api-contract-check-report.json"
TAMPERED_RQA_API_CONTRACT_STATUS_REPORT="${WORK_DIR}/tampered-rqa-api-contract-status-report.json"
TAMPERED_RQA_USER_LIFECYCLE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-user-lifecycle-stdout-report.json"
TAMPERED_RQA_USER_LIFECYCLE_CHECK_REPORT="${WORK_DIR}/tampered-rqa-user-lifecycle-check-report.json"
TAMPERED_RQA_USER_LIFECYCLE_ACTION_REPORT="${WORK_DIR}/tampered-rqa-user-lifecycle-action-report.json"
TAMPERED_RQA_GATE_THRESHOLD_REPORT="${WORK_DIR}/tampered-rqa-gate-threshold-report.json"
TAMPERED_RQA_GATE_OPEN_P0_REPORT="${WORK_DIR}/tampered-rqa-gate-open-p0-report.json"
TAMPERED_RQA_GATE_SUMMARY_REPORT="${WORK_DIR}/tampered-rqa-gate-summary-report.json"
TAMPERED_RQA_GATE_MISSING_EVAL_REPORT="${WORK_DIR}/tampered-rqa-gate-missing-eval-report.json"
TAMPERED_BEHAVIOR_CONFIG_REPORT="${WORK_DIR}/tampered-behavior-config-report.json"
TAMPERED_PRIVACY_REPORT="${WORK_DIR}/tampered-privacy-report.json"
TAMPERED_BROWSER_STDOUT_REPORT="${WORK_DIR}/tampered-browser-stdout-report.json"
TAMPERED_BROWSER_BINDING_REPORT="${WORK_DIR}/tampered-browser-binding-report.json"
TAMPERED_BROWSER_VALIDATION_REPORT="${WORK_DIR}/tampered-browser-validation-report.json"
TAMPERED_BROWSER_POINTERS_REPORT="${WORK_DIR}/tampered-browser-pointers-report.json"
TAMPERED_BROWSER_RELATIVE_EVIDENCE_REPORT="${WORK_DIR}/tampered-browser-relative-evidence-report.json"
TAMPERED_BROWSER_CHECKED_ITEMS_REPORT="${WORK_DIR}/tampered-browser-checked-items-report.json"
TAMPERED_BROWSER_EVIDENCE_HASH_REPORT="${WORK_DIR}/tampered-browser-evidence-hash-report.json"
TAMPERED_BROWSER_LOCAL_REF_HASH_REPORT="${WORK_DIR}/tampered-browser-local-ref-hash-report.json"
REVIEWED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
SYNTHETIC_RQA_EVAL_REPORT="${WORK_DIR}/synthetic-rqa-eval-report.json"
SYNTHETIC_RQA_DB="${WORK_DIR}/synthetic-rqa.db"

mkdir -p "${WORK_DIR}/screenshots"
: >"${WORK_DIR}/screenshots/models.png"
: >"${WORK_DIR}/screenshots/streaming.png"

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

cat >"${BROWSER_NO_VALIDATION_CMD}" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
echo '{"status":"ok","source":"mock-browser-without-validation"}'
SH
chmod +x "${BROWSER_NO_VALIDATION_CMD}"

cat >"${MODEL_UPSTREAM_FAKE_DOCKER}" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "ps" ]]; then
  echo "sub2api"
  exit 0
fi

if [[ "${1:-}" == "exec" ]]; then
  script="${*: -1}"
  if [[ "${script}" == getent\ hosts* ]]; then
    echo "203.0.113.10"
    exit 0
  fi
  : "${MODEL_UPSTREAM_FAKE_COUNTER:?}"
  count=0
  if [[ -f "${MODEL_UPSTREAM_FAKE_COUNTER}" ]]; then
    read -r count <"${MODEL_UPSTREAM_FAKE_COUNTER}"
  fi
  count=$((count + 1))
  echo "${count}" >"${MODEL_UPSTREAM_FAKE_COUNTER}"
  if [[ "${count}" -eq 1 ]]; then
    echo "http=000 connect=0.010 tls=0.000 total=0.020"
    echo "curl: (35) TLS connect error" >&2
    exit 35
  fi
  echo "http=401 connect=0.011 tls=0.200 total=0.300"
  exit 0
fi

echo "unexpected fake docker invocation: $*" >&2
exit 127
SH
chmod +x "${MODEL_UPSTREAM_FAKE_DOCKER}"

cat >"${BROWSER_EVIDENCE_JSON}" <<JSON
{
  "object": "tonglingyu.openwebui_browser_review",
  "status": "passed",
  "review_ref": "mock-browser-review",
  "reviewed_at": "${REVIEWED_AT}",
  "reviewer": "release-reviewer",
  "public_webui_url": "https://example.invalid",
  "checks": {
    "ordinary_user_model_visibility": {
      "status": "passed",
      "evidence_ref": "screenshots/models.png"
    },
    "streaming_chat_ux": {
      "status": "passed",
      "evidence_ref": "screenshots/streaming.png"
    },
    "admin_audit_visibility": {
      "status": "passed",
      "evidence_ref": "trace:tly-123"
    },
    "persisted_provider_settings": {
      "status": "passed",
      "evidence_ref": "runbook:provider-settings",
      "matched_rendered_env": true
    }
  }
}
JSON

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
  "TONGLINGYU_RELEASE_RQA_QUALITY_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD=${PASS_CMD}"
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

model_upstream_retry_stdout="${WORK_DIR}/model-upstream-retry.stdout"
env \
  MODEL_UPSTREAM_PROBE_DOCKER_BIN="${MODEL_UPSTREAM_FAKE_DOCKER}" \
  MODEL_UPSTREAM_FAKE_COUNTER="${MODEL_UPSTREAM_FAKE_COUNTER}" \
  MODEL_UPSTREAM_PROBE_ATTEMPTS=2 \
  MODEL_UPSTREAM_PROBE_RETRY_DELAY_SECONDS=0 \
  MODEL_UPSTREAM_PROBE_URLS=https://api.openai.test/v1/models \
  "${SCRIPT_DIR}/verify-model-upstream-network.sh" >"${model_upstream_retry_stdout}"
assert_report "${model_upstream_retry_stdout}" 'report["status"] == "ok"'
assert_report "${model_upstream_retry_stdout}" 'report["max_attempts"] == 2'
assert_report "${model_upstream_retry_stdout}" 'report["probes"][0]["attempt_count"] == 2'
assert_report "${model_upstream_retry_stdout}" \
  'report["probes"][0]["attempts"][0]["status"] == "failed"'
assert_report "${model_upstream_retry_stdout}" \
  'report["probes"][0]["attempts"][1]["status"] == "ok"'

browser_evidence_stdout="${WORK_DIR}/browser-evidence.stdout"
"${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${BROWSER_EVIDENCE_JSON}" >"${browser_evidence_stdout}"
python3 - "${browser_evidence_stdout}" "${REVIEWED_AT}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
reviewed_at = sys.argv[2]
if report["status"] != "ok":
    raise SystemExit(report)
if len(report.get("evidence_sha256", "")) != 64:
    raise SystemExit(report)
if report.get("reviewed_at") != reviewed_at:
    raise SystemExit(report)
if report.get("reviewer") != "release-reviewer":
    raise SystemExit(report)
if report.get("public_webui_url") != "https://example.invalid":
    raise SystemExit(report)
local_refs = [
    item for item in report.get("validated_evidence_refs", [])
    if item.get("kind") == "local_file"
]
validated_ref_checks = {
    item.get("check")
    for item in report.get("validated_evidence_refs", [])
}
expected_ref_checks = {
    "ordinary_user_model_visibility",
    "streaming_chat_ux",
    "admin_audit_visibility",
    "persisted_provider_settings",
}
if validated_ref_checks != expected_ref_checks:
    raise SystemExit(report)
if len(local_refs) != 2:
    raise SystemExit(report)
if any(len(item.get("sha256", "")) != 64 for item in local_refs):
    raise SystemExit(report)
if report["secret_values_printed"] is not False:
    raise SystemExit(report)
PY

browser_evidence_relative_stdout="${WORK_DIR}/browser-evidence-relative.stdout"
(
  cd "${WORK_DIR}"
  "${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
    browser-review-evidence.json >"${browser_evidence_relative_stdout}"
)
assert_report "${browser_evidence_relative_stdout}" 'report["status"] == "ok"'
assert_report "${browser_evidence_relative_stdout}" 'report["evidence_path"].startswith("/")'
assert_report "${browser_evidence_relative_stdout}" 'report["evidence_path"].endswith("/browser-review-evidence.json")'

browser_evidence_mismatch_stdout="${WORK_DIR}/browser-evidence-mismatch.stdout"
if env TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=other-review \
  "${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${BROWSER_EVIDENCE_JSON}" >"${browser_evidence_mismatch_stdout}"; then
  echo "browser review evidence must be bound to the release review ref" >&2
  exit 1
fi
python3 - "${browser_evidence_mismatch_stdout}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
if report["status"] != "failed":
    raise SystemExit(report)
if "review_ref_mismatch" not in report["errors"]:
    raise SystemExit(report)
PY

python3 - "${BROWSER_EVIDENCE_JSON}" "${MISSING_ARTIFACT_EVIDENCE_JSON}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["checks"]["ordinary_user_model_visibility"]["evidence_ref"] = "screenshots/missing.png"
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
browser_evidence_missing_artifact_stdout="${WORK_DIR}/browser-evidence-missing-artifact.stdout"
if "${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${MISSING_ARTIFACT_EVIDENCE_JSON}" >"${browser_evidence_missing_artifact_stdout}"; then
  echo "browser review screenshot evidence refs must point to existing artifacts" >&2
  exit 1
fi
assert_report "${browser_evidence_missing_artifact_stdout}" \
  '"ordinary_user_model_visibility_evidence_ref_file_not_found" in report["errors"]'

cp "${BROWSER_EVIDENCE_JSON}" "${MISMATCH_PUBLIC_URL_EVIDENCE_JSON}"
browser_evidence_public_url_mismatch_stdout="${WORK_DIR}/browser-evidence-public-url-mismatch.stdout"
if env TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://other.invalid \
  "${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${MISMATCH_PUBLIC_URL_EVIDENCE_JSON}" >"${browser_evidence_public_url_mismatch_stdout}"; then
  echo "browser review evidence must be bound to the release public URL" >&2
  exit 1
fi
assert_report "${browser_evidence_public_url_mismatch_stdout}" \
  '"public_webui_url_mismatch" in report["errors"]'

python3 - "${BROWSER_EVIDENCE_JSON}" "${EXTRA_CHECK_EVIDENCE_JSON}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["checks"]["phantom_browser_check"] = {
    "status": "passed",
    "evidence_ref": "runbook:phantom",
}
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
browser_evidence_extra_check_stdout="${WORK_DIR}/browser-evidence-extra-check.stdout"
if "${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${EXTRA_CHECK_EVIDENCE_JSON}" >"${browser_evidence_extra_check_stdout}"; then
  echo "browser review evidence must reject non-canonical checks" >&2
  exit 1
fi
assert_report "${browser_evidence_extra_check_stdout}" \
  '"unexpected_check=phantom_browser_check" in report["errors"]'

python3 - "${BROWSER_EVIDENCE_JSON}" "${STALE_BROWSER_EVIDENCE_JSON}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["reviewed_at"] = "2000-01-01T00:00:00Z"
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
browser_evidence_stale_stdout="${WORK_DIR}/browser-evidence-stale.stdout"
if "${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${STALE_BROWSER_EVIDENCE_JSON}" >"${browser_evidence_stale_stdout}"; then
  echo "browser review evidence must be recent" >&2
  exit 1
fi
assert_report "${browser_evidence_stale_stdout}" \
  '"reviewed_at_too_old" in report["errors"]'

browser_record_missing_ack_stdout="${WORK_DIR}/browser-record-missing-ack.stdout"
if env \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEWER=release-reviewer \
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://example.invalid \
  TONGLINGYU_BROWSER_REVIEW_ORDINARY_USER_MODEL_VISIBILITY_REF=screenshots/models.png \
  TONGLINGYU_BROWSER_REVIEW_STREAMING_CHAT_UX_REF=screenshots/streaming.png \
  TONGLINGYU_BROWSER_REVIEW_ADMIN_AUDIT_VISIBILITY_REF=trace:tly-123 \
  TONGLINGYU_BROWSER_REVIEW_PERSISTED_PROVIDER_SETTINGS_REF=runbook:provider-settings \
  TONGLINGYU_RELEASE_OPENWEBUI_PROVIDER_SETTINGS_MATCHED=true \
  "${SCRIPT_DIR}/record-openwebui-browser-review-evidence.sh" \
  "${GENERATED_BROWSER_EVIDENCE_JSON}" >"${browser_record_missing_ack_stdout}"; then
  echo "browser review evidence recorder must require explicit ACK" >&2
  exit 1
fi
assert_report "${browser_record_missing_ack_stdout}" \
  '"browser_review_ack_must_be_true" in report["errors"]'

browser_preflight_missing_ack_stdout="${WORK_DIR}/browser-preflight-missing-ack.stdout"
if env \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEWER=release-reviewer \
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://example.invalid \
  TONGLINGYU_BROWSER_REVIEW_ORDINARY_USER_MODEL_VISIBILITY_REF=screenshots/models.png \
  TONGLINGYU_BROWSER_REVIEW_STREAMING_CHAT_UX_REF=screenshots/streaming.png \
  TONGLINGYU_BROWSER_REVIEW_ADMIN_AUDIT_VISIBILITY_REF=trace:tly-123 \
  TONGLINGYU_BROWSER_REVIEW_PERSISTED_PROVIDER_SETTINGS_REF=runbook:provider-settings \
  TONGLINGYU_RELEASE_OPENWEBUI_PROVIDER_SETTINGS_MATCHED=true \
  "${SCRIPT_DIR}/record-openwebui-browser-review-evidence.sh" \
  --preflight "${BROWSER_PREFLIGHT_EVIDENCE_JSON}" >"${browser_preflight_missing_ack_stdout}"; then
  echo "browser review evidence recorder preflight must require explicit ACK" >&2
  exit 1
fi
assert_report "${browser_preflight_missing_ack_stdout}" \
  'report["object"] == "tonglingyu.openwebui_browser_review_record_preflight"'
assert_report "${browser_preflight_missing_ack_stdout}" \
  '"browser_review_ack_must_be_true" in report["errors"]'
assert_report "${browser_preflight_missing_ack_stdout}" \
  'report["will_write"] is False'

browser_preflight_stdout="${WORK_DIR}/browser-preflight.stdout"
env \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEWER=release-reviewer \
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://example.invalid \
  TONGLINGYU_BROWSER_REVIEW_ORDINARY_USER_MODEL_VISIBILITY_REF=screenshots/models.png \
  TONGLINGYU_BROWSER_REVIEW_STREAMING_CHAT_UX_REF=screenshots/streaming.png \
  TONGLINGYU_BROWSER_REVIEW_ADMIN_AUDIT_VISIBILITY_REF=trace:tly-123 \
  TONGLINGYU_BROWSER_REVIEW_PERSISTED_PROVIDER_SETTINGS_REF=runbook:provider-settings \
  TONGLINGYU_RELEASE_OPENWEBUI_PROVIDER_SETTINGS_MATCHED=true \
  "${SCRIPT_DIR}/record-openwebui-browser-review-evidence.sh" \
  --preflight "${BROWSER_PREFLIGHT_EVIDENCE_JSON}" >"${browser_preflight_stdout}"
assert_report "${browser_preflight_stdout}" 'report["status"] == "ok"'
assert_report "${browser_preflight_stdout}" 'report["will_write"] is False'
assert_report "${browser_preflight_stdout}" \
  'report["required_env_present"]["TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW"] is True'
assert_report "${browser_preflight_stdout}" \
  'report["public_url_source"] == "TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL"'
if [[ -e "${BROWSER_PREFLIGHT_EVIDENCE_JSON}" ]]; then
  echo "browser review evidence recorder preflight must not write evidence JSON" >&2
  exit 1
fi

browser_record_stdout="${WORK_DIR}/browser-record.stdout"
env \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEWER=release-reviewer \
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://example.invalid \
  TONGLINGYU_BROWSER_REVIEW_ORDINARY_USER_MODEL_VISIBILITY_REF=screenshots/models.png \
  TONGLINGYU_BROWSER_REVIEW_STREAMING_CHAT_UX_REF=screenshots/streaming.png \
  TONGLINGYU_BROWSER_REVIEW_ADMIN_AUDIT_VISIBILITY_REF=trace:tly-123 \
  TONGLINGYU_BROWSER_REVIEW_PERSISTED_PROVIDER_SETTINGS_REF=runbook:provider-settings \
  TONGLINGYU_RELEASE_OPENWEBUI_PROVIDER_SETTINGS_MATCHED=true \
  "${SCRIPT_DIR}/record-openwebui-browser-review-evidence.sh" \
  "${GENERATED_BROWSER_EVIDENCE_JSON}" >"${browser_record_stdout}"
assert_report "${browser_record_stdout}" 'report["status"] == "ok"'
assert_report "${browser_record_stdout}" 'report["review_ref"] == "mock-browser-review"'
assert_report "${GENERATED_BROWSER_EVIDENCE_JSON}" \
  'report["checks"]["persisted_provider_settings"]["matched_rendered_env"] is True'

default_report="${WORK_DIR}/default-not-ready.json"
if env "${common_env[@]}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${default_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null; then
  echo "default non-live release readiness must fail" >&2
  exit 1
fi
assert_report "${default_report}" 'report["production_release_ready"] is False'
assert_report "${default_report}" 'report["object"] == "tonglingyu.release_readiness_report"'
assert_report "${default_report}" 'report["schema_version"] == 1'
assert_report "${default_report}" 'report["status"] == "passed_with_skipped_gates"'
assert_report "${default_report}" 'report["exit_policy"] == "production_release_ready"'
assert_report "${default_report}" 'report["gate_command_overrides_used"] is True'
assert_report "${default_report}" 'report["secret_values_printed"] is False'
assert_report "${default_report}" '"openwebui_admin_action" in report["skipped_live_gates"]'
"${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" "${default_report}" >/dev/null

python3 - "${default_report}" "${TAMPERED_READY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["production_release_ready"] = True
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_validation_stdout="${WORK_DIR}/tampered-ready-validation.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_READY_REPORT}" >"${tampered_validation_stdout}"; then
  echo "tampered production-ready reports must fail validation" >&2
  exit 1
fi
assert_report "${tampered_validation_stdout}" 'report["status"] == "failed"'
assert_report "${tampered_validation_stdout}" \
  '"production_ready_requires_release_conditions_met" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_DERIVED_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["status"] = "passed"
report["skipped_live_gates"] = []
report["release_blockers"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_derived_stdout="${WORK_DIR}/tampered-derived-validation.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_DERIVED_REPORT}" >"${tampered_derived_stdout}"; then
  echo "tampered derived release readiness fields must fail validation" >&2
  exit 1
fi
assert_report "${tampered_derived_stdout}" \
  '"status_mismatch" in report["errors"]'
assert_report "${tampered_derived_stdout}" \
  '"skipped_live_gates_mismatch" in report["errors"]'
assert_report "${tampered_derived_stdout}" \
  '"release_blockers_mismatch" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_EXIT_POLICY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["exit_policy"] = "summary_only"
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_exit_policy_stdout="${WORK_DIR}/tampered-exit-policy-validation.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_EXIT_POLICY_REPORT}" >"${tampered_exit_policy_stdout}"; then
  echo "saved release reports must keep exit policy derived" >&2
  exit 1
fi
assert_report "${tampered_exit_policy_stdout}" \
  '"exit_policy_mismatch" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_TAIL_SHAPE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["gates"][0]["stdout_tail"] = ["ok"] * 21
report["gates"][0]["stderr_tail"] = [123]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_tail_shape_stdout="${WORK_DIR}/tampered-tail-shape-validation.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_TAIL_SHAPE_REPORT}" >"${tampered_tail_shape_stdout}"; then
  echo "saved release reports must keep bounded string gate tails" >&2
  exit 1
fi
assert_report "${tampered_tail_shape_stdout}" \
  '"runtime_config_stdout_tail_too_many_lines" in report["errors"]'
assert_report "${tampered_tail_shape_stdout}" \
  '"runtime_config_stderr_tail_0_must_be_string" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_MISSING_GENERATED_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report.pop("generated_at", None)
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_missing_generated_stdout="${WORK_DIR}/tampered-missing-generated.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_MISSING_GENERATED_REPORT}" >"${tampered_missing_generated_stdout}"; then
  echo "saved release reports must include generated_at" >&2
  exit 1
fi
assert_report "${tampered_missing_generated_stdout}" \
  '"generated_at_missing" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_MISSING_GATE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["gates"] = [
    gate
    for gate in report["gates"]
    if gate.get("name") != "openwebui_browser_review"
]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_missing_gate_stdout="${WORK_DIR}/tampered-missing-gate.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_MISSING_GATE_REPORT}" >"${tampered_missing_gate_stdout}"; then
  echo "saved release reports must include every canonical release gate" >&2
  exit 1
fi
assert_report "${tampered_missing_gate_stdout}" \
  '"missing_gate=openwebui_browser_review" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_EXTRA_GATE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["gates"].append({
    "name": "security_audit",
    "status": "passed",
    "required": True,
    "reason": "",
    "stdout_tail": [],
    "stderr_tail": [],
})
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_extra_gate_stdout="${WORK_DIR}/tampered-extra-gate.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_EXTRA_GATE_REPORT}" >"${tampered_extra_gate_stdout}"; then
  echo "saved release reports must reject non-canonical release gates" >&2
  exit 1
fi
assert_report "${tampered_extra_gate_stdout}" \
  '"unexpected_gate_name=security_audit" in report["errors"]'

python3 - "${default_report}" "${TAMPERED_SECRET_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["gates"][0]["stdout_tail"].append("authorization: Bearer sk-release-leak")
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_secret_stdout="${WORK_DIR}/tampered-secret-validation.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_SECRET_REPORT}" >"${tampered_secret_stdout}"; then
  echo "saved release reports must reject secret-like leaked values" >&2
  exit 1
fi
assert_report "${tampered_secret_stdout}" \
  'any(error.startswith("secret_like_values_present=") for error in report["errors"])'

optional_report="${WORK_DIR}/summary-optional-failure.json"
env "${common_env[@]}" \
  TONGLINGYU_RELEASE_SUMMARY_ONLY=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_REPORT_PATH="${optional_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${optional_report}" 'report["status"] == "passed_with_failed_optional_gates"'
assert_report "${optional_report}" 'report["optional_failures"] == ["openwebui_browser_review"]'
assert_report "${optional_report}" 'report["browser_review_acknowledged"] is False'

optional_missing_validation_report="${WORK_DIR}/browser-optional-missing-validation.json"
env "${common_env[@]}" \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_CMD="${BROWSER_NO_VALIDATION_CMD}" \
  TONGLINGYU_RELEASE_SUMMARY_ONLY=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE="${BROWSER_EVIDENCE_JSON}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${optional_missing_validation_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${optional_missing_validation_report}" \
  'report["status"] == "passed_with_failed_optional_gates"'
assert_report "${optional_missing_validation_report}" \
  '"openwebui_browser_review_validation" in report["optional_failures"]'
assert_report "${optional_missing_validation_report}" \
  '"openwebui_browser_review_validation" not in report["required_failures"]'
assert_report "${optional_missing_validation_report}" \
  'report["browser_review_acknowledged"] is False'

missing_validation_report="${WORK_DIR}/browser-missing-validation.json"
if env "${common_env[@]}" \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_CMD="${BROWSER_NO_VALIDATION_CMD}" \
  TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE="${BROWSER_EVIDENCE_JSON}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${missing_validation_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null; then
  echo "browser review gate must fail when validation summary is missing" >&2
  exit 1
fi
assert_report "${missing_validation_report}" 'report["status"] == "failed"'
assert_report "${missing_validation_report}" 'report["object"] == "tonglingyu.release_readiness_report"'
assert_report "${missing_validation_report}" 'report["schema_version"] == 1'
assert_report "${missing_validation_report}" '"openwebui_browser_review_validation" in report["required_failures"]'
assert_report "${missing_validation_report}" 'report["browser_review_acknowledged"] is False'
assert_report "${missing_validation_report}" '"Open WebUI browser-side review validation summary was missing" in report["release_blockers"]'

env_file="${WORK_DIR}/release-readiness.env"
env_file_report="${WORK_DIR}/env-file-report.json"
cat >"${env_file}" <<EOF
TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true
TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_QUALITY_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_SUMMARY_ONLY=true
TONGLINGYU_RELEASE_REPORT_PATH=${env_file_report}
EOF
TONGLINGYU_DEPLOY_ENV_FILE="${env_file}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
test -s "${env_file_report}"
assert_report "${env_file_report}" 'report["summary_only"] is True'
assert_report "${env_file_report}" 'report["gate_command_overrides_used"] is True'

conditions_report="${WORK_DIR}/live-conditions-met.json"
env "${common_env[@]}" \
  TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_SUMMARY_ONLY=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://example.invalid \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE="${BROWSER_EVIDENCE_JSON}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${conditions_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${conditions_report}" 'report["release_conditions_met"] is True'
assert_report "${conditions_report}" 'report["object"] == "tonglingyu.release_readiness_report"'
assert_report "${conditions_report}" 'report["schema_version"] == 1'
assert_report "${conditions_report}" 'report["production_release_ready"] is False'
assert_report "${conditions_report}" 'report["status"] == "passed_with_gate_command_overrides"'
assert_report "${conditions_report}" 'report["exit_policy"] == "summary_only"'
assert_report "${conditions_report}" 'report["browser_review_ref"] == "mock-browser-review"'
assert_report "${conditions_report}" 'report["browser_review_evidence"].endswith("browser-review-evidence.json")'
assert_report "${conditions_report}" 'report["browser_review_evidence"].startswith("/")'
assert_report "${conditions_report}" 'report["browser_review_evidence"] == report["browser_review_validation"]["evidence_path"]'
assert_report "${conditions_report}" 'report["browser_review_validation"]["expected_review_ref_bound"] is True'
assert_report "${conditions_report}" 'report["browser_review_validation"]["expected_public_url_bound"] is True'
assert_report "${conditions_report}" 'report["browser_review_validation"]["reviewed_at"] == "'"${REVIEWED_AT}"'"'
assert_report "${conditions_report}" 'report["browser_review_validation"]["reviewer"] == "release-reviewer"'
assert_report "${conditions_report}" 'report["browser_review_validation"]["public_webui_url"] == "https://example.invalid"'
assert_report "${conditions_report}" 'len(report["browser_review_validation"]["evidence_sha256"]) == 64'
assert_report "${conditions_report}" 'len([item for item in report["browser_review_validation"]["validated_evidence_refs"] if item["kind"] == "local_file"]) == 2'
assert_report "${conditions_report}" '"gate command overrides were used" in report["release_blockers"]'

python3 - "${conditions_report}" "${TAMPERED_BROWSER_POINTERS_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["browser_review_ref"] = ""
report["browser_review_evidence"] = ""
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_pointers_stdout="${WORK_DIR}/tampered-browser-pointers.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_POINTERS_REPORT}" >"${tampered_browser_pointers_stdout}"; then
  echo "saved browser validation must keep top-level evidence pointers" >&2
  exit 1
fi
assert_report "${tampered_browser_pointers_stdout}" \
  '"browser_review_validation_requires_review_ref" in report["errors"]'
assert_report "${tampered_browser_pointers_stdout}" \
  '"browser_review_validation_requires_evidence" in report["errors"]'

python3 - "${conditions_report}" "${TAMPERED_BROWSER_RELATIVE_EVIDENCE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
validation = dict(report["browser_review_validation"])
validation["evidence_path"] = "browser-review-evidence.json"
report["browser_review_evidence"] = "browser-review-evidence.json"
report["browser_review_validation"] = validation
for gate in report["gates"]:
    if gate.get("name") == "openwebui_browser_review":
        gate["stdout_tail"] = [json.dumps(validation, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_relative_evidence_stdout="${WORK_DIR}/tampered-browser-relative-evidence.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_RELATIVE_EVIDENCE_REPORT}" >"${tampered_browser_relative_evidence_stdout}"; then
  echo "saved browser validation must reject relative evidence paths" >&2
  exit 1
fi
assert_report "${tampered_browser_relative_evidence_stdout}" \
  '"browser_review_evidence_path_must_be_absolute" in report["errors"]'
assert_report "${tampered_browser_relative_evidence_stdout}" \
  '"browser_review_validation_evidence_path_must_be_absolute" in report["errors"]'

python3 - "${conditions_report}" "${SYNTHETIC_READY_REPORT}" \
  "${SYNTHETIC_RQA_EVAL_REPORT}" "${SYNTHETIC_RQA_DB}" <<'PY'
import hashlib
import json
import sqlite3
import sys

source, target, eval_report_path, db_path = sys.argv[1:5]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["production_release_ready"] = True
report["gate_command_overrides_used"] = False
report["summary_only"] = False
report["exit_policy"] = "production_release_ready"
report["status"] = "passed"
report["release_blockers"] = []
quality_summary = {
    "blockers": [],
    "eval_case_classification": {"passed": 2, "ratio": 1.0, "total": 2},
    "eval_failure_records": 0,
    "exact_term_coverage": {"passed": 1, "ratio": 1.0, "total": 1},
    "expected_evidence_denominator": 1,
    "expected_evidence_hit_at_1": {"passed": 1, "ratio": 1.0, "total": 1},
    "expected_evidence_hit_at_3": {"passed": 1, "ratio": 1.0, "total": 1},
    "expected_evidence_hit_at_8": {"passed": 1, "ratio": 1.0, "total": 1},
    "forbidden_conclusion_avoided": {"passed": 2, "ratio": 1.0, "total": 2},
    "quality_report_coverage": {"passed": 2, "ratio": 1.0, "total": 2},
    "quality_report_production_ready": {"passed": 1, "ratio": 1.0, "total": 1},
    "required_type_coverage": {"passed": 2, "ratio": 1.0, "total": 2},
    "reviewer_status_matched": {"passed": 2, "ratio": 1.0, "total": 2},
    "source_boundary_confirmation_avoided": {"passed": 1, "ratio": 1.0, "total": 1},
    "schema_version": "tonglingyu-eval-quality-v1",
    "source_coverage_boundary": {
        "authoritative_edition_review_status": "not_reviewed",
        "expert_collation_status": "not_reviewed",
        "facsimile_review_status": "not_reviewed",
        "source_snapshot_status": "wikisource_source_snapshot",
    },
    "source_diversity": {
        "boundary": "wikisource_source_snapshot_only_not_facsimile_or_authoritative_collation",
        "count": 1,
        "source_ids": ["hongloumeng-wikisource-120"],
    },
    "status": "passed",
}
eval_report = {
    "object": "tonglingyu.eval_report",
    "status": "passed",
    "summary": {"failed": 0, "passed": 2, "total": 2},
    "quality_summary": quality_summary,
    "cases": [
        {
            "block_ids": ["synthetic-block"],
            "card_count": 1,
            "evidence_ids": ["synthetic-evidence"],
            "expected_review_status": "passed",
            "failures": [],
            "forbidden_conclusion_count": 0,
            "id": "synthetic-ready-case",
            "package_id": "pkg-synthetic",
            "passed": True,
            "quality": {
                "classification": {
                    "classification": "expected_evidence",
                    "expected_block_ids": ["synthetic-block"],
                    "expected_evidence_ids": ["synthetic-evidence"],
                },
                "edition_labels": ["synthetic-edition"],
                "exact_term_coverage": {"passed": 1, "total": 1},
                "expected_evidence_hit_at_1": True,
                "expected_evidence_hit_at_3": True,
                "expected_evidence_hit_at_8": True,
                "quality_report_count": 1,
                "quality_report_production_ready_required": True,
                "quality_report_unallowed_non_production_issues": [],
                "required_type_required": True,
                "required_type_passed": True,
                "source_boundary_confirmation_required": False,
                "source_boundary_confirmation_avoided": False,
                "source_coverage_boundary": "wikisource_source_snapshot_only_not_facsimile_or_authoritative_collation",
                "source_ids": ["hongloumeng-wikisource-120"],
            },
            "question": "synthetic release eval case",
            "required_evidence_type": "base_text",
            "review_severity": "none",
            "review_status": "passed",
            "trace_id": "eval-synthetic",
        },
        {
            "block_ids": ["synthetic-block"],
            "card_count": 1,
            "evidence_ids": ["synthetic-evidence"],
            "expected_review_status": "needs_revision",
            "failures": [],
            "forbidden_conclusion_count": 0,
            "id": "synthetic-source-boundary-case",
            "package_id": "pkg-synthetic-boundary",
            "passed": True,
            "quality": {
                "classification": {
                    "classification": "not_applicable",
                    "reason": "source_boundary_requires_facsimile_authoritative_or_expert_review",
                },
                "edition_labels": ["synthetic-edition"],
                "exact_term_coverage": {"passed": 0, "total": 0},
                "expected_evidence_hit_at_1": False,
                "expected_evidence_hit_at_3": False,
                "expected_evidence_hit_at_8": False,
                "quality_report_count": 1,
                "quality_report_production_ready_required": False,
                "quality_report_unallowed_non_production_issues": [],
                "required_type_required": True,
                "required_type_passed": True,
                "source_boundary_confirmation_required": True,
                "source_boundary_confirmation_avoided": True,
                "source_coverage_boundary": "wikisource_source_snapshot_only_not_facsimile_or_authoritative_collation",
                "source_ids": ["hongloumeng-wikisource-120"],
            },
            "question": "synthetic source boundary confirmation case",
            "required_evidence_type": "base_text",
            "review_severity": "needs_revision",
            "review_status": "needs_revision",
            "trace_id": "eval-synthetic-boundary",
        },
    ],
}
with open(eval_report_path, "w", encoding="utf-8") as handle:
    json.dump(eval_report, handle, sort_keys=True)
    handle.write("\n")
with open(eval_report_path, "rb") as handle:
    eval_report_sha256 = hashlib.sha256(handle.read()).hexdigest()
conn = sqlite3.connect(db_path)
conn.executescript(
    """
    CREATE TABLE kb_version (
        version_id TEXT,
        source_root TEXT,
        source_count INTEGER,
        block_count INTEGER,
        schema_version TEXT,
        built_at TEXT
    );
    CREATE TABLE sources (
        source_id TEXT,
        source_hash TEXT,
        license TEXT,
        license_url TEXT,
        license_source_url TEXT,
        attribution TEXT,
        usage_boundary TEXT
    );
    CREATE TABLE retrieval_failures (
        human_review_status TEXT
    );
    CREATE TABLE knowledge_governance_tasks (
        status TEXT,
        priority TEXT
    );
    """
)
conn.execute(
    "INSERT INTO kb_version VALUES (?, ?, ?, ?, ?, ?)",
    (
        "kb-synthetic",
        "resources/sources/wiki",
        1,
        1,
        "tonglingyu-kb-v1",
        "2026-05-15T00:00:00Z",
    ),
)
conn.execute(
    "INSERT INTO sources VALUES (?, ?, ?, ?, ?, ?, ?)",
    (
        "hongloumeng-wikisource-120",
        "3" * 64,
        "CC-BY-SA-4.0",
        "https://creativecommons.org/licenses/by-sa/4.0/",
        "https://wikisource.org/wiki/Wikisource:Copyright_policy",
        "Wikisource contributors",
        "synthetic usage boundary",
    ),
)
conn.commit()
conn.close()
behavior_config = {
    "agent_runtime_mode_env": "TONGLINGYU_AGENT_RUNTIME_MODE",
    "decoding_parameters_source": "gateway_runtime_config",
    "decoding_parameters_summary": {
        "source": "gateway_runtime_config",
        "upstream_timeout_secs_env": "TONGLINGYU_UPSTREAM_TIMEOUT_SECS",
    },
    "gateway_policy_digest": "6" * 64,
    "model_upstream_id": "gpt-synthetic",
    "model_upstream_bound_by_gate": "model_upstream_network",
    "profile_contract": "tonglingyu-runtime-profile-contract-v1",
    "prompt_digest": "7" * 64,
    "reviewer_policy": "local_reviewer_enforced",
    "reviewer_policy_digest": "8" * 64,
    "runtime_profile_digest": "9" * 64,
    "tool_policy": "read_only_runtime_tools",
    "tool_policy_digest": "a" * 64,
}
behavior_config["behavior_config_digest"] = hashlib.sha256(
    json.dumps(
        behavior_config,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
production_default_thresholds = {
    "eval_case_classification": 1.0,
    "exact_term_coverage": 1.0,
    "expected_evidence_denominator_min": 1,
    "expected_evidence_hit_at_8": 1.0,
    "forbidden_conclusion_avoided": 1.0,
    "open_p0_retrieval_failures": 0,
    "open_p0_governance_tasks": 0,
    "quality_report_coverage": 1.0,
    "quality_report_production_ready": 1.0,
    "required_type_coverage": 1.0,
    "reviewer_status_matched": 1.0,
    "source_boundary_confirmation_avoided": 1.0,
}
gate_stdout = {
    "runtime_config": {
        "checked_policy_fields": ["TONGLINGYU_AGENT_RUNTIME_MODE"],
        "checked_secret_fields": ["TONGLINGYU_GATEWAY_API_KEY(S)"],
        "checked_services": ["tonglingyu-gateway"],
        "status": "ok",
    },
    "retrieval_quality": {
        "behavior_config": behavior_config,
        "effective_thresholds": production_default_thresholds,
        "errors": [],
        "eval_report_generated_by_gate": True,
        "eval_report_path": eval_report_path,
        "eval_report_sha256": eval_report_sha256,
        "eval_run_id": f"rqa-eval-{eval_report_sha256[:16]}",
        "eval_suite_version": "tonglingyu-eval-quality-v1",
        "kb_build_hash": "2" * 64,
        "kb_version": {
            "block_count": 10419,
            "built_at": "2026-05-15T00:00:00Z",
            "schema_version": "tonglingyu-kb-v1",
            "source_count": 5,
            "source_root": "resources/sources/wiki",
            "version_id": "kb-synthetic",
        },
        "object": "tonglingyu.rqa_quality_gate",
        "open_p0_governance_tasks": 0,
        "open_p0_retrieval_failures": 0,
        "production_default_thresholds": production_default_thresholds,
        "quality_gate_passed": True,
        "quality_summary": {
            "blockers": quality_summary["blockers"],
            "eval_case_classification": quality_summary["eval_case_classification"],
            "eval_failure_records": quality_summary["eval_failure_records"],
            "exact_term_coverage": quality_summary["exact_term_coverage"],
            "expected_evidence_denominator": quality_summary["expected_evidence_denominator"],
            "expected_evidence_hit_at_8": quality_summary["expected_evidence_hit_at_8"],
            "forbidden_conclusion_avoided": quality_summary["forbidden_conclusion_avoided"],
            "quality_report_coverage": quality_summary["quality_report_coverage"],
            "quality_report_production_ready": quality_summary["quality_report_production_ready"],
            "required_type_coverage": quality_summary["required_type_coverage"],
            "reviewer_status_matched": quality_summary["reviewer_status_matched"],
            "source_boundary_confirmation_avoided": quality_summary["source_boundary_confirmation_avoided"],
            "source_coverage_boundary": quality_summary["source_coverage_boundary"],
            "source_diversity": quality_summary["source_diversity"],
            "status": quality_summary["status"],
        },
        "rqa_schema_version": "tonglingyu-retrieval-failures-v1",
        "schema_version": 1,
        "secret_values_printed": False,
        "source_license_summary": {
            "missing_metadata": [],
            "source_count": 1,
            "sources": [{
                "attribution": "Wikisource contributors",
                "license": "CC-BY-SA-4.0",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "source_hash": "3" * 64,
                "source_id": "hongloumeng-wikisource-120",
                "usage_boundary_sha256": "4" * 64,
            }],
        },
        "source_snapshot_digest": "5" * 64,
        "status": "ok",
        "threshold_config": {
            "invalid_overrides": [],
            "less_strict_overrides": [],
            "override_env_prefix": "TONGLINGYU_RQA_THRESHOLD_",
            "overrides": [],
            "production_ready_thresholds_enforced": True,
            "source": "production_defaults",
        },
    },
    "rqa_backup_restore_drill": {
        "artifacts": {
            "rqa_quality_gate_sha256": "b" * 64,
            "saved_release_report_sha256": "c" * 64,
            "saved_report_validator_sha256": "d" * 64,
        },
        "backup": {
            "artifact_sha256": "e" * 64,
            "finished_at": "2026-05-15T00:00:02+00:00",
            "size_bytes": 4096,
            "source_db_sha256": "f" * 64,
            "started_at": "2026-05-15T00:00:01+00:00",
        },
        "checks": {
            "admin_package_readable": True,
            "admin_trace_readable": True,
            "governance_task_readable": True,
            "package_replay_readable": True,
            "retrieval_failure_readable": True,
            "rqa_quality_gate_reran": True,
            "saved_report_validator_reran": True,
        },
        "drill_result": "passed",
        "duration_ms": 4000,
        "environment": "production",
        "errors": [],
        "finished_at": "2026-05-15T00:00:04+00:00",
        "object": "tonglingyu.rqa_backup_restore_drill",
        "operator": "release-reviewer",
        "policy_version": "tonglingyu-rqa-backup-restore-drill-v1",
        "refs": {
            "failure_sha256": "1" * 64,
            "governance_task_sha256": "2" * 64,
            "package_sha256": "3" * 64,
            "trace_sha256": "4" * 64,
        },
        "restore": {
            "db_integrity_check": "ok",
            "finished_at": "2026-05-15T00:00:04+00:00",
            "restored_db_sha256": "5" * 64,
            "schema_migrations_verified": True,
            "started_at": "2026-05-15T00:00:02+00:00",
        },
        "rpo": {
            "actual_seconds": 2.0,
            "met": True,
            "target_seconds": 3600,
        },
        "rto": {
            "actual_seconds": 2.0,
            "met": True,
            "target_seconds": 900,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "source_mode": "existing_refs",
        "started_at": "2026-05-15T00:00:00+00:00",
        "status": "ok",
    },
    "rqa_performance_budget": {
        "budget_policy_version": "tonglingyu-rqa-performance-budget-v1",
        "budget_results": {
            "admin_failure_list_ms": {
                "actual_ms": 80,
                "budget_ms": 2000,
                "met": True,
            },
            "admin_governance_task_list_ms": {
                "actual_ms": 90,
                "budget_ms": 2000,
                "met": True,
            },
            "admin_status_update_ms": {
                "actual_ms": 150,
                "budget_ms": 3000,
                "met": True,
            },
            "admin_trace_read_ms": {
                "actual_ms": 70,
                "budget_ms": 2000,
                "met": True,
            },
            "rqa_quality_gate_ms": {
                "actual_ms": 3000,
                "budget_ms": 90000,
                "met": True,
            },
            "rqa_write_ms": {
                "actual_ms": 900,
                "budget_ms": 10000,
                "met": True,
            },
        },
        "budgets": {
            "admin_failure_list_ms": 2000,
            "admin_governance_task_list_ms": 2000,
            "admin_status_update_ms": 3000,
            "admin_trace_read_ms": 2000,
            "rqa_quality_gate_ms": 90000,
            "rqa_write_ms": 10000,
        },
        "checks": {
            "admin_lists_readable": True,
            "admin_status_updates_closed_open_p0": True,
            "admin_trace_readable": True,
            "rqa_quality_gate_reran": True,
            "rqa_write_created_failure": True,
            "rqa_write_created_governance_task": True,
        },
        "errors": [],
        "generated_at": "2026-05-15T00:00:06+00:00",
        "measurements": {
            "admin_failure_list_ms": 80,
            "admin_governance_task_list_ms": 90,
            "admin_status_update_ms": 150,
            "admin_trace_read_ms": 70,
            "rqa_quality_gate_ms": 3000,
            "rqa_write_ms": 900,
        },
        "object": "tonglingyu.rqa_performance_budget_gate",
        "performance_budget_passed": True,
        "refs": {
            "failure_sha256": "5" * 64,
            "governance_task_sha256": "6" * 64,
            "package_sha256": "7" * 64,
            "trace_sha256": "8" * 64,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "status": "ok",
        "timeouts_seconds": {
            "curl_connect": 3.0,
            "curl_max_time": 15.0,
            "eval": 180.0,
            "gateway_build": 300.0,
            "kb_build": 180.0,
            "rqa_quality_gate": 180.0,
        },
    },
    "rqa_api_contract": {
        "api_contract_passed": True,
        "checks": {
            "admin_payload_excludes_raw_prompts": True,
            "governance_task_invalid_priority_rejected": True,
            "governance_task_invalid_status_rejected": True,
            "governance_task_list_pagination": True,
            "governance_task_list_schema": True,
            "governance_task_max_page_clamped": True,
            "governance_task_read_schema": True,
            "governance_task_unknown_filter_rejected": True,
            "retrieval_failure_invalid_status_rejected": True,
            "retrieval_failure_list_pagination": True,
            "retrieval_failure_list_schema": True,
            "retrieval_failure_max_page_clamped": True,
            "retrieval_failure_read_schema": True,
            "retrieval_failure_unknown_filter_rejected": True,
        },
        "contract_version": "tonglingyu-rqa-api-contract-v1",
        "errors": [],
        "generated_at": "2026-05-15T00:00:07+00:00",
        "negative_statuses": {
            "governance_task_invalid_priority": 400,
            "governance_task_invalid_status": 400,
            "governance_task_unknown_filter": 400,
            "retrieval_failure_invalid_status": 400,
            "retrieval_failure_unknown_filter": 400,
        },
        "object": "tonglingyu.rqa_api_contract_gate",
        "pagination": {
            "governance_tasks": {
                "effective_limit": 1,
                "max_limit": 100,
                "next_offset": 1,
                "offset": 0,
                "requested_limit": 1,
            },
            "retrieval_failures": {
                "effective_limit": 1,
                "max_limit": 100,
                "next_offset": 1,
                "offset": 0,
                "requested_limit": 1,
            },
        },
        "refs": {
            "failure_sha256": "9" * 64,
            "governance_task_sha256": "a" * 64,
            "package_sha256": "b" * 64,
            "trace_sha256": "c" * 64,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "status": "ok",
    },
    "rqa_user_lifecycle": {
        "action_reports": {
            "anonymize": {
                "action": "anonymize",
                "counts": {"message_count": 2, "session_count": 1},
                "source_text_included": False,
                "response_body_included": False,
                "secret_values_printed": False,
                "status": "ok",
            },
            "blocked_anonymize": {
                "action": "anonymize",
                "counts": {"active_legal_hold_count": 1, "message_count": 2, "session_count": 1},
                "source_text_included": False,
                "response_body_included": False,
                "secret_values_printed": False,
                "status": "blocked",
            },
            "export": {
                "action": "export",
                "counts": {"message_count": 2, "session_count": 1},
                "source_text_included": False,
                "response_body_included": False,
                "secret_values_printed": False,
                "status": "ok",
            },
            "legal_hold": {
                "action": "legal_hold",
                "counts": {"active_legal_hold_count": 1, "message_count": 2, "session_count": 1},
                "source_text_included": False,
                "response_body_included": False,
                "secret_values_printed": False,
                "status": "ok",
            },
            "release_hold": {
                "action": "release_legal_hold",
                "counts": {"active_legal_hold_count": 0, "message_count": 2, "session_count": 1},
                "source_text_included": False,
                "response_body_included": False,
                "secret_values_printed": False,
                "status": "ok",
            },
        },
        "checks": {
            "anonymize_completed": True,
            "export_audited_and_redacted": True,
            "export_manifest_redacted": True,
            "legal_hold_blocks_anonymize": True,
            "legal_hold_can_be_released": True,
            "lifecycle_audit_events_recorded": True,
            "raw_user_values_removed": True,
            "rqa_traceability_preserved": True,
            "tombstones_recorded": True,
        },
        "contract_version": "tonglingyu-rqa-user-lifecycle-contract-v1",
        "errors": [],
        "generated_at": "2026-05-15T00:00:08+00:00",
        "lifecycle_policy_version": "tonglingyu-rqa-lifecycle-v1",
        "object": "tonglingyu.rqa_user_lifecycle_gate",
        "refs": {
            "package_sha256": "d" * 64,
            "subject_sha256": "e" * 64,
            "trace_sha256": "f" * 64,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "status": "ok",
        "user_lifecycle_passed": True,
    },
    "security_scan": {
        "accepted_error_count": 0,
        "dependency_scan": {
            "critical_count": 0,
            "high_count": 0,
            "report_sha256": "a" * 64,
            "scanner": "cargo-audit",
            "status": "passed",
        },
        "errors": [],
        "generated_at": "2026-05-15T00:00:05+00:00",
        "image_scan": {
            "critical_count": 0,
            "digest_missing_count": 0,
            "high_count": 0,
            "image_count": 6,
            "mutable_tag_count": 0,
            "report_sha256": "b" * 64,
            "scanner": "trivy",
            "status": "passed",
        },
        "object": "tonglingyu.release_security_gate",
        "release_script_scan": {
            "finding_count": 0,
            "finding_types": [],
            "scanned_file_count": 18,
            "scanner": "tonglingyu-release-script-static-policy-v1",
            "status": "passed",
        },
        "risk_acceptance": {
            "accepted_findings": [],
            "present": False,
        },
        "risk_conclusion": "no_unaccepted_findings",
        "scan_coverage": {
            "dependency_scan": True,
            "image_scan": True,
            "release_script_scan": True,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "security_scan_passed": True,
        "status": "ok",
        "unaccepted_error_count": 0,
    },
    "model_upstream_network": {
        "errors": [],
        "object": "tonglingyu.model_upstream_network_gate",
        "probe_count": 1,
        "probes": [],
        "secret_values_printed": False,
        "status": "ok",
    },
    "strict_gateway": {
        "agent_runtime_mode": "hermes",
        "behavior_config": behavior_config,
        "checked_surfaces": ["tonglingyu-gateway:/healthz"],
        "model_ids": ["tonglingyu"],
        "status": "ok",
        "stream_trace_id": "tly-stream",
        "trace_id": "tly-chat",
    },
    "openwebui_function": {
        "function_id": "agent_identity_bridge",
        "status": "ok",
        "type": "filter",
    },
    "openwebui_admin_action": {
        "function_id": "tonglingyu_gateway_admin",
        "status": "ok",
        "type": "action",
    },
}
for gate in report["gates"]:
    if gate.get("name") in gate_stdout:
        gate["stdout_tail"] = [json.dumps(gate_stdout[gate["name"]], sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
"${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${SYNTHETIC_READY_REPORT}" >/dev/null

rqa_gate_default_stdout="${WORK_DIR}/rqa-gate-default-thresholds.stdout"
env \
  TONGLINGYU_UPSTREAM_MODEL=gpt-synthetic \
  TONGLINGYU_RQA_DB_PATH="${SYNTHETIC_RQA_DB}" \
  TONGLINGYU_RQA_EVAL_REPORT_PATH="${SYNTHETIC_RQA_EVAL_REPORT}" \
  "${SCRIPT_DIR}/verify-tonglingyu-rqa-quality-gate.sh" >"${rqa_gate_default_stdout}"
assert_report "${rqa_gate_default_stdout}" 'report["status"] == "ok"'
assert_report "${rqa_gate_default_stdout}" \
  'report["threshold_config"]["source"] == "production_defaults"'
assert_report "${rqa_gate_default_stdout}" \
  'report["threshold_config"]["production_ready_thresholds_enforced"] is True'

rqa_gate_low_threshold_stdout="${WORK_DIR}/rqa-gate-low-threshold.stdout"
if env \
  TONGLINGYU_UPSTREAM_MODEL=gpt-synthetic \
  TONGLINGYU_RQA_DB_PATH="${SYNTHETIC_RQA_DB}" \
  TONGLINGYU_RQA_EVAL_REPORT_PATH="${SYNTHETIC_RQA_EVAL_REPORT}" \
  TONGLINGYU_RQA_THRESHOLD_EXPECTED_EVIDENCE_HIT_AT_8=0.8 \
  "${SCRIPT_DIR}/verify-tonglingyu-rqa-quality-gate.sh" \
  >"${rqa_gate_low_threshold_stdout}"; then
  echo "RQA quality gate must fail closed when thresholds are below production defaults" >&2
  exit 1
fi
assert_report "${rqa_gate_low_threshold_stdout}" \
  '"thresholds_below_production_defaults" in report["errors"]'
assert_report "${rqa_gate_low_threshold_stdout}" \
  '"expected_evidence_hit_at_8" in report["threshold_config"]["less_strict_overrides"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_STALE_READY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["generated_at"] = "2000-01-01T00:00:00Z"
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_stale_ready_stdout="${WORK_DIR}/tampered-stale-ready.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_STALE_READY_REPORT}" >"${tampered_stale_ready_stdout}"; then
  echo "production-ready release reports must be recent" >&2
  exit 1
fi
assert_report "${tampered_stale_ready_stdout}" \
  '"production_ready_report_too_old" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_LIVE_GATE_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "strict_gateway":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_live_gate_stdout="${WORK_DIR}/tampered-live-gate-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_LIVE_GATE_STDOUT_REPORT}" >"${tampered_live_gate_stdout}"; then
  echo "production-ready reports must bind live gate status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_live_gate_stdout}" \
  '"strict_gateway_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_GATE_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "retrieval_quality":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_gate_stdout="${WORK_DIR}/tampered-rqa-gate-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_GATE_STDOUT_REPORT}" >"${tampered_rqa_gate_stdout}"; then
  echo "production-ready reports must bind RQA quality gate status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_gate_stdout}" \
  '"retrieval_quality_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_RESTORE_GATE_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_backup_restore_drill":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_restore_gate_stdout="${WORK_DIR}/tampered-rqa-restore-gate-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_RESTORE_GATE_STDOUT_REPORT}" >"${tampered_rqa_restore_gate_stdout}"; then
  echo "production-ready reports must bind RQA restore drill status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_restore_gate_stdout}" \
  '"rqa_backup_restore_drill_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_RESTORE_FIXTURE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_backup_restore_drill":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["source_mode"] = "fixture"
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_restore_fixture_stdout="${WORK_DIR}/tampered-rqa-restore-fixture.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_RESTORE_FIXTURE_REPORT}" >"${tampered_rqa_restore_fixture_stdout}"; then
  echo "production-ready reports must reject fixture-only RQA restore drill evidence" >&2
  exit 1
fi
assert_report "${tampered_rqa_restore_fixture_stdout}" \
  '"production_ready_requires_live_rqa_restore_drill_refs" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_RESTORE_RTO_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_backup_restore_drill":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["rto"]["met"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_restore_rto_stdout="${WORK_DIR}/tampered-rqa-restore-rto.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_RESTORE_RTO_REPORT}" >"${tampered_rqa_restore_rto_stdout}"; then
  echo "production-ready reports must reject unmet RQA restore RTO" >&2
  exit 1
fi
assert_report "${tampered_rqa_restore_rto_stdout}" \
  '"rqa_backup_restore_drill_rto_not_met" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_SECURITY_GATE_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "security_scan":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_security_gate_stdout="${WORK_DIR}/tampered-security-gate-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_SECURITY_GATE_STDOUT_REPORT}" >"${tampered_security_gate_stdout}"; then
  echo "production-ready reports must bind security scan status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_security_gate_stdout}" \
  '"security_scan_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_SECURITY_GATE_RISK_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "security_scan":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["scan_coverage"]["image_scan"] = False
        gate_json["image_scan"]["status"] = "missing"
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_security_gate_risk_stdout="${WORK_DIR}/tampered-security-gate-risk.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_SECURITY_GATE_RISK_REPORT}" >"${tampered_security_gate_risk_stdout}"; then
  echo "production-ready reports must reject missing security scans without risk acceptance" >&2
  exit 1
fi
assert_report "${tampered_security_gate_risk_stdout}" \
  '"security_scan_without_risk_acceptance_requires_image_scan" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_SECURITY_GATE_SCRIPT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "security_scan":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["release_script_scan"]["status"] = "failed"
        gate_json["release_script_scan"]["finding_count"] = 1
        gate_json["release_script_scan"]["finding_types"] = ["curl_pipe_shell"]
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_security_gate_script_stdout="${WORK_DIR}/tampered-security-gate-script.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_SECURITY_GATE_SCRIPT_REPORT}" >"${tampered_security_gate_script_stdout}"; then
  echo "production-ready reports must reject release script security findings" >&2
  exit 1
fi
assert_report "${tampered_security_gate_script_stdout}" \
  '"security_scan_release_scripts_not_passed" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_PERFORMANCE_GATE_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_performance_budget":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_performance_gate_stdout="${WORK_DIR}/tampered-rqa-performance-gate-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_PERFORMANCE_GATE_STDOUT_REPORT}" >"${tampered_rqa_performance_gate_stdout}"; then
  echo "production-ready reports must bind RQA performance gate status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_performance_gate_stdout}" \
  '"rqa_performance_budget_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_PERFORMANCE_BUDGET_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_performance_budget":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["measurements"]["rqa_write_ms"] = 10001
        gate_json["budget_results"]["rqa_write_ms"]["actual_ms"] = 10001
        gate_json["budget_results"]["rqa_write_ms"]["met"] = False
        gate_json["performance_budget_passed"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_performance_budget_stdout="${WORK_DIR}/tampered-rqa-performance-budget.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_PERFORMANCE_BUDGET_REPORT}" >"${tampered_rqa_performance_budget_stdout}"; then
  echo "production-ready reports must reject exceeded RQA performance budgets" >&2
  exit 1
fi
assert_report "${tampered_rqa_performance_budget_stdout}" \
  '"rqa_performance_budget_not_passed" in report["errors"]'
assert_report "${tampered_rqa_performance_budget_stdout}" \
  '"rqa_performance_budget_rqa_write_ms_not_met" in report["errors"]'
assert_report "${tampered_rqa_performance_budget_stdout}" \
  '"rqa_performance_budget_rqa_write_ms_exceeded" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_PERFORMANCE_CHECK_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_performance_budget":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["checks"]["admin_status_updates_closed_open_p0"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_performance_check_stdout="${WORK_DIR}/tampered-rqa-performance-check.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_PERFORMANCE_CHECK_REPORT}" >"${tampered_rqa_performance_check_stdout}"; then
  echo "production-ready reports must reject incomplete RQA performance checks" >&2
  exit 1
fi
assert_report "${tampered_rqa_performance_check_stdout}" \
  '"rqa_performance_budget_check_failed=admin_status_updates_closed_open_p0" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_API_CONTRACT_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_api_contract":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_api_contract_stdout="${WORK_DIR}/tampered-rqa-api-contract-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_API_CONTRACT_STDOUT_REPORT}" >"${tampered_rqa_api_contract_stdout}"; then
  echo "production-ready reports must bind RQA API contract gate status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_api_contract_stdout}" \
  '"rqa_api_contract_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_API_CONTRACT_CHECK_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_api_contract":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["checks"]["admin_payload_excludes_raw_prompts"] = False
        gate_json["api_contract_passed"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_api_contract_check_stdout="${WORK_DIR}/tampered-rqa-api-contract-check.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_API_CONTRACT_CHECK_REPORT}" >"${tampered_rqa_api_contract_check_stdout}"; then
  echo "production-ready reports must reject failed RQA API contract checks" >&2
  exit 1
fi
assert_report "${tampered_rqa_api_contract_check_stdout}" \
  '"rqa_api_contract_not_passed" in report["errors"]'
assert_report "${tampered_rqa_api_contract_check_stdout}" \
  '"rqa_api_contract_check_failed=admin_payload_excludes_raw_prompts" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_API_CONTRACT_STATUS_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_api_contract":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["negative_statuses"]["retrieval_failure_invalid_status"] = 500
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_api_contract_status_stdout="${WORK_DIR}/tampered-rqa-api-contract-status.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_API_CONTRACT_STATUS_REPORT}" >"${tampered_rqa_api_contract_status_stdout}"; then
  echo "production-ready reports must reject non-400 RQA API contract negative checks" >&2
  exit 1
fi
assert_report "${tampered_rqa_api_contract_status_stdout}" \
  '"rqa_api_contract_retrieval_failure_invalid_status_status_invalid" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_USER_LIFECYCLE_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_user_lifecycle":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_user_lifecycle_stdout="${WORK_DIR}/tampered-rqa-user-lifecycle-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_USER_LIFECYCLE_STDOUT_REPORT}" >"${tampered_rqa_user_lifecycle_stdout}"; then
  echo "production-ready reports must bind RQA user lifecycle gate status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_user_lifecycle_stdout}" \
  '"rqa_user_lifecycle_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_USER_LIFECYCLE_CHECK_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_user_lifecycle":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["checks"]["raw_user_values_removed"] = False
        gate_json["user_lifecycle_passed"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_user_lifecycle_check_stdout="${WORK_DIR}/tampered-rqa-user-lifecycle-check.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_USER_LIFECYCLE_CHECK_REPORT}" >"${tampered_rqa_user_lifecycle_check_stdout}"; then
  echo "production-ready reports must reject failed RQA user lifecycle checks" >&2
  exit 1
fi
assert_report "${tampered_rqa_user_lifecycle_check_stdout}" \
  '"rqa_user_lifecycle_not_passed" in report["errors"]'
assert_report "${tampered_rqa_user_lifecycle_check_stdout}" \
  '"rqa_user_lifecycle_check_failed=raw_user_values_removed" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_USER_LIFECYCLE_ACTION_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_user_lifecycle":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["action_reports"]["blocked_anonymize"]["status"] = "ok"
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_user_lifecycle_action_stdout="${WORK_DIR}/tampered-rqa-user-lifecycle-action.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_USER_LIFECYCLE_ACTION_REPORT}" >"${tampered_rqa_user_lifecycle_action_stdout}"; then
  echo "production-ready reports must reject user lifecycle action status drift" >&2
  exit 1
fi
assert_report "${tampered_rqa_user_lifecycle_action_stdout}" \
  '"rqa_user_lifecycle_blocked_anonymize_status_invalid" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_GATE_THRESHOLD_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "retrieval_quality":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["effective_thresholds"]["expected_evidence_hit_at_8"] = 0.8
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_gate_threshold_stdout="${WORK_DIR}/tampered-rqa-gate-threshold.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_GATE_THRESHOLD_REPORT}" >"${tampered_rqa_gate_threshold_stdout}"; then
  echo "production-ready reports must reject lowered RQA quality thresholds" >&2
  exit 1
fi
assert_report "${tampered_rqa_gate_threshold_stdout}" \
  '"retrieval_quality_threshold_expected_evidence_hit_at_8_below_production_default" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_GATE_OPEN_P0_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "retrieval_quality":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["open_p0_retrieval_failures"] = 1
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_gate_open_p0_stdout="${WORK_DIR}/tampered-rqa-gate-open-p0.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_GATE_OPEN_P0_REPORT}" >"${tampered_rqa_gate_open_p0_stdout}"; then
  echo "production-ready reports must reject open RQA retrieval failures" >&2
  exit 1
fi
assert_report "${tampered_rqa_gate_open_p0_stdout}" \
  '"retrieval_quality_open_p0_retrieval_failures_not_zero" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${WORK_DIR}/tampered-rqa-gate-open-governance.json" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "retrieval_quality":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["open_p0_governance_tasks"] = 1
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_gate_open_governance_stdout="${WORK_DIR}/tampered-rqa-gate-open-governance.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${WORK_DIR}/tampered-rqa-gate-open-governance.json" >"${tampered_rqa_gate_open_governance_stdout}"; then
  echo "production-ready reports must reject open RQA governance tasks" >&2
  exit 1
fi
assert_report "${tampered_rqa_gate_open_governance_stdout}" \
  '"retrieval_quality_open_p0_governance_tasks_not_zero" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_GATE_SUMMARY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "retrieval_quality":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["quality_summary"]["expected_evidence_denominator"] = 2
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_gate_summary_stdout="${WORK_DIR}/tampered-rqa-gate-summary.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_GATE_SUMMARY_REPORT}" >"${tampered_rqa_gate_summary_stdout}"; then
  echo "production-ready reports must bind RQA gate summary to the eval artifact" >&2
  exit 1
fi
assert_report "${tampered_rqa_gate_summary_stdout}" \
  '"retrieval_quality_eval_report_expected_evidence_denominator_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_GATE_MISSING_EVAL_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "retrieval_quality":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["eval_report_path"] = "/tmp/tonglingyu-rqa-missing-eval-report.json"
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_gate_missing_eval_stdout="${WORK_DIR}/tampered-rqa-gate-missing-eval.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_GATE_MISSING_EVAL_REPORT}" \
  >"${tampered_rqa_gate_missing_eval_stdout}"; then
  echo "production-ready reports must keep the RQA eval artifact readable" >&2
  exit 1
fi
assert_report "${tampered_rqa_gate_missing_eval_stdout}" \
  '"retrieval_quality_eval_report_file_not_found" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BEHAVIOR_CONFIG_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "strict_gateway":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["behavior_config"]["model_upstream_id"] = "other-model"
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_behavior_config_stdout="${WORK_DIR}/tampered-behavior-config.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BEHAVIOR_CONFIG_REPORT}" >"${tampered_behavior_config_stdout}"; then
  echo "RQA eval behavior config must match strict live gate behavior config" >&2
  exit 1
fi
assert_report "${tampered_behavior_config_stdout}" \
  '"strict_gateway_behavior_config_digest_mismatch" in report["errors"]'
assert_report "${tampered_behavior_config_stdout}" \
  '"retrieval_quality_behavior_config_strict_gateway_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_PRIVACY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["rqa_debug_leak"] = {
    "question": "raw user question must not appear in release report",
    "trace_ids": ["trace-a", "trace-b"],
}
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_privacy_stdout="${WORK_DIR}/tampered-privacy.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_PRIVACY_REPORT}" >"${tampered_privacy_stdout}"; then
  echo "release reports must reject raw questions and high-cardinality id lists" >&2
  exit 1
fi
assert_report "${tampered_privacy_stdout}" \
  'any(error.startswith("privacy_sensitive_fields_present=") for error in report["errors"])'
assert_report "${tampered_privacy_stdout}" \
  'any(error.startswith("high_cardinality_fields_present=") for error in report["errors"])'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_PRODUCTION_FLAG_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["production_release_ready"] = False
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_production_flag_stdout="${WORK_DIR}/tampered-production-flag.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_PRODUCTION_FLAG_REPORT}" >"${tampered_production_flag_stdout}"; then
  echo "saved release reports must keep production-ready flag derived" >&2
  exit 1
fi
assert_report "${tampered_production_flag_stdout}" \
  '"production_release_ready_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BROWSER_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "openwebui_browser_review":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_stdout_stdout="${WORK_DIR}/tampered-browser-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_STDOUT_REPORT}" >"${tampered_browser_stdout_stdout}"; then
  echo "saved browser validation must be backed by browser gate stdout" >&2
  exit 1
fi
assert_report "${tampered_browser_stdout_stdout}" \
  '"browser_review_validation_stdout_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BROWSER_BINDING_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
validation = dict(report["browser_review_validation"])
validation["expected_review_ref_bound"] = False
validation["expected_public_url_bound"] = False
report["browser_review_validation"] = validation
for gate in report["gates"]:
    if gate.get("name") == "openwebui_browser_review":
        gate["stdout_tail"] = [json.dumps(validation, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_binding_stdout="${WORK_DIR}/tampered-browser-binding.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_BINDING_REPORT}" >"${tampered_browser_binding_stdout}"; then
  echo "production-ready browser validation must bind release ref and public URL" >&2
  exit 1
fi
assert_report "${tampered_browser_binding_stdout}" \
  '"production_ready_requires_browser_review_ref_bound" in report["errors"]'
assert_report "${tampered_browser_binding_stdout}" \
  '"production_ready_requires_browser_review_public_url_bound" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BROWSER_VALIDATION_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["browser_review_validation"] = {
    "object": "tonglingyu.openwebui_browser_review_gate",
    "status": "ok",
    "evidence_path": report["browser_review_evidence"],
    "evidence_sha256": "",
    "review_ref": "other-review",
    "checked_items": [],
    "validated_evidence_refs": [],
    "errors": [],
    "secret_values_printed": False,
}
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_validation_stdout="${WORK_DIR}/tampered-browser-validation.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_VALIDATION_REPORT}" >"${tampered_browser_validation_stdout}"; then
  echo "production-ready reports must validate browser review verifier metadata" >&2
  exit 1
fi
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_review_ref_mismatch" in report["errors"]'
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_evidence_sha256_invalid" in report["errors"]'
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_stdout_mismatch" in report["errors"]'
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_missing_ref=ordinary_user_model_visibility" in report["errors"]'
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_reviewed_at_missing" in report["errors"]'
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_reviewer_missing" in report["errors"]'
assert_report "${tampered_browser_validation_stdout}" \
  '"browser_review_validation_public_webui_url_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BROWSER_CHECKED_ITEMS_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
validation = dict(report["browser_review_validation"])
validation["checked_items"] = list(validation["checked_items"]) + ["phantom_browser_check"]
report["browser_review_validation"] = validation
for gate in report["gates"]:
    if gate.get("name") == "openwebui_browser_review":
        gate["stdout_tail"] = [json.dumps(validation, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_checked_items_stdout="${WORK_DIR}/tampered-browser-checked-items.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_CHECKED_ITEMS_REPORT}" >"${tampered_browser_checked_items_stdout}"; then
  echo "saved browser validation must reject non-canonical checked items" >&2
  exit 1
fi
assert_report "${tampered_browser_checked_items_stdout}" \
  '"browser_review_validation_unexpected_checked_item=phantom_browser_check" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BROWSER_EVIDENCE_HASH_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
validation = dict(report["browser_review_validation"])
validation["evidence_sha256"] = "0" * 64
report["browser_review_validation"] = validation
for gate in report["gates"]:
    if gate.get("name") == "openwebui_browser_review":
        gate["stdout_tail"] = [json.dumps(validation, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_evidence_hash_stdout="${WORK_DIR}/tampered-browser-evidence-hash.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_EVIDENCE_HASH_REPORT}" >"${tampered_browser_evidence_hash_stdout}"; then
  echo "saved browser validation must match the evidence file digest" >&2
  exit 1
fi
assert_report "${tampered_browser_evidence_hash_stdout}" \
  '"browser_review_evidence_sha256_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BROWSER_LOCAL_REF_HASH_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
validation = dict(report["browser_review_validation"])
validation["validated_evidence_refs"] = [
    dict(item)
    for item in validation["validated_evidence_refs"]
]
for item in validation["validated_evidence_refs"]:
    if item.get("kind") == "local_file":
        item["sha256"] = "0" * 64
        break
report["browser_review_validation"] = validation
for gate in report["gates"]:
    if gate.get("name") == "openwebui_browser_review":
        gate["stdout_tail"] = [json.dumps(validation, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_browser_local_ref_hash_stdout="${WORK_DIR}/tampered-browser-local-ref-hash.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BROWSER_LOCAL_REF_HASH_REPORT}" >"${tampered_browser_local_ref_hash_stdout}"; then
  echo "saved browser validation must match local evidence file digests" >&2
  exit 1
fi
assert_report "${tampered_browser_local_ref_hash_stdout}" \
  'any(error.endswith("_sha256_mismatch") for error in report["errors"])'

failed_report="${WORK_DIR}/live-failed-gate.json"
if env \
  TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true \
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_QUALITY_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD=${FAIL_CMD}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD=${PASS_CMD}" \
  TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE="${BROWSER_EVIDENCE_JSON}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${failed_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null; then
  echo "live release readiness must fail when strict Gateway gate fails" >&2
  exit 1
fi
assert_report "${failed_report}" 'report["production_release_ready"] is False'
assert_report "${failed_report}" '"strict_gateway" in report["required_failures"]'
assert_report "${failed_report}" 'report["status"] == "failed"'

echo "tonglingyu release readiness contract passed"
