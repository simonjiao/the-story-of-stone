#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

PASS_CMD="${WORK_DIR}/gate-pass.sh"
FAIL_CMD="${WORK_DIR}/gate-fail.sh"
BROWSER_NO_VALIDATION_CMD="${WORK_DIR}/browser-gate-no-validation.sh"
BROWSER_EVIDENCE_JSON="${WORK_DIR}/browser-review-evidence.json"
MISSING_ARTIFACT_EVIDENCE_JSON="${WORK_DIR}/missing-artifact-browser-review-evidence.json"
MISMATCH_PUBLIC_URL_EVIDENCE_JSON="${WORK_DIR}/mismatch-public-url-browser-review-evidence.json"
STALE_BROWSER_EVIDENCE_JSON="${WORK_DIR}/stale-browser-review-evidence.json"
GENERATED_BROWSER_EVIDENCE_JSON="${WORK_DIR}/generated-browser-review-evidence.json"
REVIEWED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

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

browser_evidence_stdout="${WORK_DIR}/browser-evidence.stdout"
"${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" \
  "${BROWSER_EVIDENCE_JSON}" >"${browser_evidence_stdout}"
python3 - "${browser_evidence_stdout}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
if report["status"] != "ok":
    raise SystemExit(report)
if len(report.get("evidence_sha256", "")) != 64:
    raise SystemExit(report)
local_refs = [
    item for item in report.get("validated_evidence_refs", [])
    if item.get("kind") == "local_file"
]
if len(local_refs) != 2:
    raise SystemExit(report)
if any(len(item.get("sha256", "")) != 64 for item in local_refs):
    raise SystemExit(report)
if report["secret_values_printed"] is not False:
    raise SystemExit(report)
PY

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
assert_report "${missing_validation_report}" '"openwebui_browser_review_validation" in report["required_failures"]'
assert_report "${missing_validation_report}" 'report["browser_review_acknowledged"] is False'
assert_report "${missing_validation_report}" '"Open WebUI browser-side review validation summary was missing" in report["release_blockers"]'

env_file="${WORK_DIR}/release-readiness.env"
env_file_report="${WORK_DIR}/env-file-report.json"
cat >"${env_file}" <<EOF
TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true
TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}
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
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE="${BROWSER_EVIDENCE_JSON}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${conditions_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${conditions_report}" 'report["release_conditions_met"] is True'
assert_report "${conditions_report}" 'report["production_release_ready"] is False'
assert_report "${conditions_report}" 'report["status"] == "passed_with_gate_command_overrides"'
assert_report "${conditions_report}" 'report["exit_policy"] == "summary_only"'
assert_report "${conditions_report}" 'report["browser_review_ref"] == "mock-browser-review"'
assert_report "${conditions_report}" 'report["browser_review_evidence"].endswith("browser-review-evidence.json")'
assert_report "${conditions_report}" 'len(report["browser_review_validation"]["evidence_sha256"]) == 64'
assert_report "${conditions_report}" 'len([item for item in report["browser_review_validation"]["validated_evidence_refs"] if item["kind"] == "local_file"]) == 2'
assert_report "${conditions_report}" '"gate command overrides were used" in report["release_blockers"]'

failed_report="${WORK_DIR}/live-failed-gate.json"
if env \
  TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true \
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}" \
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
