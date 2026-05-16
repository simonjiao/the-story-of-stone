#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

for leaked_env_name in \
  "${!TONGLINGYU_RELEASE_@}" \
  "${!TONGLINGYU_RQA_@}" \
  "${!TONGLINGYU_BROWSER_REVIEW_@}" \
  "${!MODEL_UPSTREAM_PROBE_@}" \
  TONGLINGYU_DEPLOY_ENV_FILE \
  PUBLIC_WEBUI_URL \
  OPEN_WEBUI_BASE_URL; do
  unset "${leaked_env_name}"
done

PASS_CMD="${WORK_DIR}/gate-pass.sh"
FAIL_CMD="${WORK_DIR}/gate-fail.sh"
BROWSER_NO_VALIDATION_CMD="${WORK_DIR}/browser-gate-no-validation.sh"
MODEL_UPSTREAM_FAKE_DOCKER="${WORK_DIR}/model-upstream-fake-docker.sh"
MODEL_UPSTREAM_FAKE_COUNTER="${WORK_DIR}/model-upstream-fake-counter"
FAKE_TRIVY_DIR="${WORK_DIR}/fake-trivy-bin"
SECURITY_DEPENDENCY_SCAN_JSON="${WORK_DIR}/security-dependency-scan.json"
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
TAMPERED_RELEASE_MANIFEST_REPORT="${WORK_DIR}/tampered-release-manifest-report.json"
TAMPERED_RELEASE_MANIFEST_DIGEST_REPORT="${WORK_DIR}/tampered-release-manifest-digest-report.json"
TAMPERED_RELEASE_CONTEXT_REPORT="${WORK_DIR}/tampered-release-context-report.json"
TAMPERED_RELEASE_CONTEXT_VALIDITY_REPORT="${WORK_DIR}/tampered-release-context-validity-report.json"
TAMPERED_RUNTIME_IDENTITY_REPORT="${WORK_DIR}/tampered-runtime-identity-report.json"
TAMPERED_RUNTIME_IDENTITY_IMAGES_REPORT="${WORK_DIR}/tampered-runtime-identity-images-report.json"
TAMPERED_BEHAVIOR_BINDING_REPORT="${WORK_DIR}/tampered-behavior-binding-report.json"
TAMPERED_STRICT_GATEWAY_METRICS_PRIVACY_REPORT="${WORK_DIR}/tampered-strict-gateway-metrics-privacy-report.json"
TAMPERED_ARTIFACT_REGISTRY_REPORT="${WORK_DIR}/tampered-artifact-registry-report.json"
TAMPERED_ARTIFACT_REGISTRY_DIGEST_REPORT="${WORK_DIR}/tampered-artifact-registry-digest-report.json"
TAMPERED_LIVE_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-live-gate-stdout-report.json"
TAMPERED_RQA_MIGRATION_PREFLIGHT_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-migration-preflight-stdout-report.json"
TAMPERED_RQA_MIGRATION_PREFLIGHT_BACKUP_REPORT="${WORK_DIR}/tampered-rqa-migration-preflight-backup-report.json"
TAMPERED_RQA_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-gate-stdout-report.json"
TAMPERED_RQA_RESTORE_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-restore-gate-stdout-report.json"
TAMPERED_RQA_RESTORE_FIXTURE_REPORT="${WORK_DIR}/tampered-rqa-restore-fixture-report.json"
TAMPERED_RQA_RESTORE_RTO_REPORT="${WORK_DIR}/tampered-rqa-restore-rto-report.json"
TAMPERED_RQA_RESTORE_BACKUP_ARTIFACT_REPORT="${WORK_DIR}/tampered-rqa-restore-backup-artifact-report.json"
TAMPERED_SECURITY_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-security-gate-stdout-report.json"
TAMPERED_SECURITY_GATE_RISK_REPORT="${WORK_DIR}/tampered-security-gate-risk-report.json"
TAMPERED_SECURITY_GATE_SCRIPT_REPORT="${WORK_DIR}/tampered-security-gate-script-report.json"
TAMPERED_SECURITY_GATE_IMAGE_INVENTORY_REPORT="${WORK_DIR}/tampered-security-gate-image-inventory-report.json"
TAMPERED_SECURITY_GATE_IMAGE_RAW_REPORTS_REPORT="${WORK_DIR}/tampered-security-gate-image-raw-reports-report.json"
TAMPERED_RELEASE_OPS_STDOUT_REPORT="${WORK_DIR}/tampered-release-ops-stdout-report.json"
TAMPERED_RELEASE_OPS_MONITOR_REPORT="${WORK_DIR}/tampered-release-ops-monitor-report.json"
TAMPERED_RELEASE_OPS_ALERT_REPORT="${WORK_DIR}/tampered-release-ops-alert-report.json"
TAMPERED_RQA_INCIDENT_CAPACITY_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-incident-capacity-stdout-report.json"
TAMPERED_RQA_INCIDENT_CAPACITY_EMERGENCY_REPORT="${WORK_DIR}/tampered-rqa-incident-capacity-emergency-report.json"
TAMPERED_RQA_INCIDENT_CAPACITY_EVIDENCE_REPORT="${WORK_DIR}/tampered-rqa-incident-capacity-evidence-report.json"
TAMPERED_RQA_PERFORMANCE_GATE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-performance-gate-stdout-report.json"
TAMPERED_RQA_PERFORMANCE_BUDGET_REPORT="${WORK_DIR}/tampered-rqa-performance-budget-report.json"
TAMPERED_RQA_PERFORMANCE_CHECK_REPORT="${WORK_DIR}/tampered-rqa-performance-check-report.json"
TAMPERED_RQA_API_CONTRACT_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-api-contract-stdout-report.json"
TAMPERED_RQA_API_CONTRACT_CHECK_REPORT="${WORK_DIR}/tampered-rqa-api-contract-check-report.json"
TAMPERED_RQA_API_CONTRACT_STATUS_REPORT="${WORK_DIR}/tampered-rqa-api-contract-status-report.json"
TAMPERED_RQA_API_CONTRACT_POLICY_REPORT="${WORK_DIR}/tampered-rqa-api-contract-policy-report.json"
TAMPERED_RQA_USER_LIFECYCLE_STDOUT_REPORT="${WORK_DIR}/tampered-rqa-user-lifecycle-stdout-report.json"
TAMPERED_RQA_USER_LIFECYCLE_CHECK_REPORT="${WORK_DIR}/tampered-rqa-user-lifecycle-check-report.json"
TAMPERED_RQA_USER_LIFECYCLE_ACTION_REPORT="${WORK_DIR}/tampered-rqa-user-lifecycle-action-report.json"
TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_STDOUT_REPORT="${WORK_DIR}/tampered-openwebui-admin-action-contract-stdout-report.json"
TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_CHECK_REPORT="${WORK_DIR}/tampered-openwebui-admin-action-contract-check-report.json"
TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_ACTION_REPORT="${WORK_DIR}/tampered-openwebui-admin-action-contract-action-report.json"
TAMPERED_OPENWEBUI_ADMIN_ACTION_LIVE_REPORT="${WORK_DIR}/tampered-openwebui-admin-action-live-report.json"
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
mkdir -p "${FAKE_TRIVY_DIR}"
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

cat >"${SECURITY_DEPENDENCY_SCAN_JSON}" <<'JSON'
{
  "critical_count": 0,
  "high_count": 0,
  "object": "tonglingyu.security_scan_result",
  "scan_type": "dependency",
  "scanner": "cargo-audit",
  "secret_values_printed": false,
  "status": "passed"
}
JSON

cat >"${FAKE_TRIVY_DIR}/trivy" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != "image" ]]; then
  echo "unexpected fake trivy invocation: $*" >&2
  exit 2
fi
cat <<'JSON'
{
  "Results": [
    {
      "Target": "fake-image",
      "Vulnerabilities": [
        {
          "Severity": "HIGH",
          "VulnerabilityID": "CVE-FAKE-HIGH"
        }
      ]
    }
  ]
}
JSON
SH
chmod +x "${FAKE_TRIVY_DIR}/trivy"

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
  "TONGLINGYU_RELEASE_ENVIRONMENT=contract-live"
  "TONGLINGYU_RELEASE_TARGET=contract-target"
  "TONGLINGYU_RELEASE_GIT_COMMIT=1111111111111111111111111111111111111111"
  "TONGLINGYU_RELEASE_GIT_TRACKED_DIRTY=false"
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_MIGRATION_PREFLIGHT_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_QUALITY_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_OPS_READINESS_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_RQA_INCIDENT_CAPACITY_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_MODEL_UPSTREAM_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_STRICT_GATEWAY_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_OPENWEBUI_FUNCTION_CMD=${PASS_CMD}"
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CMD=${PASS_CMD}"
)
security_digest_env=(
  "AGENT_PLATFORM_IMAGE_REF=registry.invalid/hermes-agent-platform@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  "TONGLINGYU_GATEWAY_IMAGE_REF=registry.invalid/tonglingyu-gateway@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  "HERMES_IMAGE_REF=registry.invalid/hermes@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
  "OPEN_WEBUI_IMAGE_REF=registry.invalid/open-webui@sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
  "CLOUDFLARED_IMAGE_REF=registry.invalid/cloudflared@sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
  "AGENT_PLATFORM_POSTGRES_IMAGE_REF=registry.invalid/postgres@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
)

trivy_high_stdout="${WORK_DIR}/trivy-high-security.stdout"
trivy_high_artifact_dir="${WORK_DIR}/trivy-high-raw-reports"
if env \
  "${security_digest_env[@]}" \
  "PATH=${FAKE_TRIVY_DIR}:${PATH}" \
  "TONGLINGYU_RELEASE_SECURITY_DEPENDENCY_SCAN_PATH=${SECURITY_DEPENDENCY_SCAN_JSON}" \
  "TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_PATH=" \
  "TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_DIR=${trivy_high_artifact_dir}" \
  "TONGLINGYU_RELEASE_SECURITY_RUN_TRIVY=true" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-security.sh" >"${trivy_high_stdout}"; then
  echo "security gate must fail when real Trivy JSON contains high findings" >&2
  exit 1
fi
assert_report "${trivy_high_stdout}" 'report["status"] == "failed"'
assert_report "${trivy_high_stdout}" 'report["image_scan"]["high_count"] == report["image_scan"]["image_count"]'
assert_report "${trivy_high_stdout}" 'report["image_scan"]["scanned_report_count"] == report["image_scan"]["image_count"]'
assert_report "${trivy_high_stdout}" 'report["image_scan"]["raw_reports_persistent"] is True'
assert_report "${trivy_high_stdout}" 'report["image_scan"]["raw_report_artifact_dir"].endswith("trivy-high-raw-reports")'
assert_report "${trivy_high_stdout}" 'len(report["image_scan"]["raw_report_paths"]) == report["image_scan"]["image_count"]'
assert_report "${trivy_high_stdout}" '"image_high_findings_present" in report["errors"]'
assert_report "${trivy_high_stdout}" 'report["image_scan"]["owned_high_count"] == 2'

third_party_high_artifact_dir="${WORK_DIR}/third-party-high-raw-reports"
third_party_high_scan_json="${WORK_DIR}/third-party-high-image-scan.json"
python3 - "${third_party_high_scan_json}" "${third_party_high_artifact_dir}" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

target = Path(sys.argv[1])
artifact_dir = Path(sys.argv[2]).resolve()
artifact_dir.mkdir(parents=True, exist_ok=True)
image_refs = [
    "registry.invalid/hermes-agent-platform@sha256:" + "a" * 64,
    "registry.invalid/tonglingyu-gateway@sha256:" + "b" * 64,
    "registry.invalid/hermes@sha256:" + "c" * 64,
    "registry.invalid/open-webui@sha256:" + "d" * 64,
    "registry.invalid/cloudflared@sha256:" + "e" * 64,
    "registry.invalid/postgres@sha256:" + "f" * 64,
]
raw_report_paths = []
report_digests = []
for index, image_ref in enumerate(image_refs):
    report_path = artifact_dir / (
        "trivy-" + hashlib.sha256(image_ref.encode("utf-8")).hexdigest() + ".json"
    )
    vulnerabilities = []
    if index >= 2:
        vulnerabilities = [
            {"Severity": "HIGH", "VulnerabilityID": f"CVE-THIRD-PARTY-{index}"}
        ]
    report_path.write_text(
        json.dumps(
            {
                "Results": [
                    {
                        "Target": image_ref,
                        "Vulnerabilities": vulnerabilities,
                    }
                ]
            },
            ensure_ascii=True,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    raw_report_paths.append(str(report_path))
    report_digests.append(hashlib.sha256(report_path.read_bytes()).hexdigest())
target.write_text(
    json.dumps(
        {
            "critical_count": 0,
            "failed_image_count": 0,
            "high_count": 4,
            "object": "tonglingyu.security_scan_result",
            "raw_report_artifact_dir": str(artifact_dir),
            "raw_report_paths": raw_report_paths,
            "raw_report_paths_sha256": hashlib.sha256(
                ("\n".join(raw_report_paths) + "\n").encode("utf-8")
            ).hexdigest(),
            "raw_reports_persistent": True,
            "scan_run_id": "third-party-high",
            "scan_type": "image",
            "scanned_image_count": len(image_refs),
            "scanned_image_refs_sha256": hashlib.sha256(
                ("\n".join(image_refs) + "\n").encode("utf-8")
            ).hexdigest(),
            "scanned_report_count": len(report_digests),
            "scanned_reports_sha256": hashlib.sha256(
                ("\n".join(sorted(report_digests)) + "\n").encode("utf-8")
            ).hexdigest(),
            "scanner": "trivy",
            "secret_values_printed": False,
            "status": "failed",
        },
        ensure_ascii=True,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY
third_party_high_stdout="${WORK_DIR}/third-party-high-security.stdout"
env \
  "${security_digest_env[@]}" \
  "TONGLINGYU_RELEASE_SECURITY_DEPENDENCY_SCAN_PATH=${SECURITY_DEPENDENCY_SCAN_JSON}" \
  "TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_PATH=${third_party_high_scan_json}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-security.sh" >"${third_party_high_stdout}"
assert_report "${third_party_high_stdout}" 'report["status"] == "ok"'
assert_report "${third_party_high_stdout}" 'report["security_scan_passed"] is True'
assert_report "${third_party_high_stdout}" 'report["image_scan"]["status"] == "passed"'
assert_report "${third_party_high_stdout}" 'report["image_scan"]["owned_high_count"] == 0'
assert_report "${third_party_high_stdout}" 'report["image_scan"]["third_party_high_count"] == 4'
assert_report "${third_party_high_stdout}" '"third_party_image_high_findings_present" in report["nonblocking_errors"]'
assert_report "${third_party_high_stdout}" '"image_high_findings_present" not in report["errors"]'

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
TONGLINGYU_RELEASE_ENVIRONMENT=contract-live
TONGLINGYU_RELEASE_TARGET=contract-target
TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_MIGRATION_PREFLIGHT_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_QUALITY_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_OPS_READINESS_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_RQA_INCIDENT_CAPACITY_CMD=${PASS_CMD}
TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD=${PASS_CMD}
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
assert_report "${conditions_report}" 'report["release_conditions_met"] is False'
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
assert_report "${conditions_report}" '"live running image inventory was not captured" in report["release_blockers"]'
assert_report "${conditions_report}" '"live migration preflight was not captured" in report["release_blockers"]'
assert_report "${conditions_report}" '"pending migrations must be zero for live release" in report["release_blockers"]'
assert_report "${conditions_report}" '"tracked worktree must be clean for live release" not in report["release_blockers"]'
assert_report "${conditions_report}" 'report["release_runtime_identity"]["git"]["tracked_dirty"] is False'

dirty_worktree_report="${WORK_DIR}/live-dirty-worktree.json"
env "${common_env[@]}" \
  TONGLINGYU_RELEASE_GIT_TRACKED_DIRTY=true \
  TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_SUMMARY_ONLY=true \
  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF=mock-browser-review \
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL=https://example.invalid \
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE="${BROWSER_EVIDENCE_JSON}" \
  TONGLINGYU_RELEASE_REPORT_PATH="${dirty_worktree_report}" \
  "${SCRIPT_DIR}/verify-tonglingyu-release-readiness.sh" >/dev/null
assert_report "${dirty_worktree_report}" 'report["release_conditions_met"] is False'
assert_report "${dirty_worktree_report}" 'report["release_runtime_identity"]["git"]["tracked_dirty"] is True'
assert_report "${dirty_worktree_report}" '"tracked worktree must be clean for live release" in report["release_blockers"]'

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
  "${SYNTHETIC_RQA_EVAL_REPORT}" "${SYNTHETIC_RQA_DB}" \
  "${SCRIPT_DIR}/../runbooks/tonglingyu-rqa-release-runbook.md" <<'PY'
import hashlib
import json
import sqlite3
import sys
from pathlib import Path

source, target, eval_report_path, db_path, runbook_path = sys.argv[1:6]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["production_release_ready"] = True
report["gate_command_overrides_used"] = False
report["summary_only"] = False
report["exit_policy"] = "production_release_ready"
report["status"] = "passed"
report["release_blockers"] = []
report["release_conditions_met"] = True
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


def digest_json(value):
    return hashlib.sha256(
        json.dumps(
            value,
            ensure_ascii=True,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()


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
behavior_config["behavior_config_digest"] = digest_json(behavior_config)
runtime_summary = {
    "answer_source": "agent-runtime-hermes-draft-with-local-reviewer",
    "content_used_for_final_answer": True,
    "draft_consumed": True,
    "evidence_matches_local": True,
    "evidence_observation_count": 1,
    "executed_profile_step_count": 4,
    "hermes_content_execution_complete": True,
    "local_governance_enforced": True,
    "mode": "hermes",
    "package_matches_local": True,
    "profile_execution_status": "hermes_profile_observed_with_local_governance",
    "profile_step_count": 4,
    "review_local_enforced": True,
    "tool_audit_event_count": 4,
    "tool_result_count": 4,
}
behavior_config_binding = {
    "object": "tonglingyu.strict_gateway_behavior_config_binding",
    "schema_version": 1,
    "policy_version": "tonglingyu-behavior-config-binding-v1",
    "behavior_config_digest": behavior_config["behavior_config_digest"],
    "behavior_config_sha256": digest_json(behavior_config),
    "admin_trace_id": "tly-chat",
    "stream_trace_id": "tly-stream",
    "admin_trace_runtime_summary": runtime_summary,
    "admin_trace_runtime_summary_sha256": digest_json(runtime_summary),
    "stream_trace_runtime_summary": runtime_summary,
    "stream_trace_runtime_summary_sha256": digest_json(runtime_summary),
    "agent_runtime_mode": "hermes",
    "profile_execution_status": "hermes_profile_observed_with_local_governance",
    "hermes_content_execution_complete": True,
    "local_governance_enforced": True,
    "profile_step_count": 4,
    "executed_profile_step_count": 4,
    "tool_result_count": 4,
    "tool_audit_event_count": 4,
    "secret_values_printed": False,
}
runbook_sha256 = hashlib.sha256(Path(runbook_path).read_bytes()).hexdigest()
post_release_monitor_evidence_path = Path(str(target) + ".post-release-monitor.json")
post_release_monitor_evidence = {
    "checks": {
        "admin_action_or_api_evidence_ref_valid": True,
        "conclusion_passed": True,
        "live_gate_statuses_passed": True,
        "monitor_window_at_least_60_minutes": True,
        "operator_environment_recorded": True,
        "release_report_exists": True,
        "release_report_requires_live": True,
    },
    "conclusion": "passed",
    "environment": "production",
    "errors": [],
    "evidence_refs": {
        "admin_action_or_api_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:live-admin-action-query",
            "valid": True,
        },
        "live_gate_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:live-release-gate",
            "valid": True,
        },
        "monitor_ref": {
            "kind": "artifact",
            "ref": "artifact:post-release-monitor",
            "valid": True,
        },
    },
    "finished_at": "2026-05-15T01:00:00+00:00",
    "generated_at": "2026-05-15T01:00:01+00:00",
    "monitor_policy_version": "tonglingyu-post-release-monitor-v1",
    "object": "tonglingyu.post_release_monitor",
    "operator": "release-reviewer",
    "release_report": {
        "failed_live_gates": [],
        "generated_at": "2026-05-15T00:00:00+00:00",
        "live_gate_statuses": {
            "model_upstream_network": "passed",
            "openwebui_admin_action": "passed",
            "openwebui_function": "passed",
            "strict_gateway": "passed",
        },
        "missing_live_gates": [],
        "path": target,
        "production_release_ready": True,
        "require_live": True,
        "sha256": "8" * 64,
    },
    "schema_version": 1,
    "secret_values_printed": False,
    "started_at": "2026-05-15T00:00:00+00:00",
    "status": "ok",
    "window_minutes": 60,
}
post_release_monitor_evidence_path.write_text(
    json.dumps(post_release_monitor_evidence, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
post_release_monitor_evidence_sha256 = hashlib.sha256(
    post_release_monitor_evidence_path.read_bytes(),
).hexdigest()
capacity_load_evidence_path = Path(str(target) + ".capacity-load.json")
capacity_load_evidence = {
    "budget_results": {
        "admin_read_p95_ms": True,
        "metrics_read_p95_ms": True,
        "release_gate_ms": True,
        "rqa_write_p95_ms": True,
    },
    "capacity_load_policy_version": "tonglingyu-rqa-capacity-load-evidence-v1",
    "checks": {
        "admin_read_budget_passed": True,
        "metrics_read_budget_passed": True,
        "operator_environment_recorded": True,
        "release_gate_budget_passed": True,
        "representative_capacity_covered": True,
        "rqa_write_budget_passed": True,
        "window_at_least_minimum": True,
    },
    "environment": "production",
    "errors": [],
    "evidence_refs": {
        "audit_history_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:audit-history",
            "valid": True,
        },
        "capacity_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:capacity-smoke",
            "valid": True,
        },
        "incident_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:incident-response",
            "valid": True,
        },
        "load_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:load-soak",
            "valid": True,
        },
    },
    "finished_at": "2026-05-15T00:10:00+00:00",
    "generated_at": "2026-05-15T00:10:01+00:00",
    "load_budgets_ms": {
        "admin_read_p95_ms": 2000,
        "metrics_read_p95_ms": 2000,
        "release_gate_ms": 90000,
        "rqa_write_p95_ms": 10000,
    },
    "load_measurements": {
        "admin_read_p95_ms": 80,
        "metrics_read_p95_ms": 60,
        "release_gate_ms": 3000,
        "rqa_write_p95_ms": 900,
    },
    "min_window_minutes": 10,
    "object": "tonglingyu.rqa_capacity_load_evidence",
    "operator": "release-reviewer",
    "representative_counts": {
        "admin_list_page_count": 2,
        "eval_report_count": 1,
        "failure_count": 1,
    },
    "schema_version": 1,
    "secret_values_printed": False,
    "started_at": "2026-05-15T00:00:00+00:00",
    "status": "ok",
    "window_minutes": 10,
}
capacity_load_evidence_path.write_text(
    json.dumps(capacity_load_evidence, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
capacity_load_evidence_sha256 = hashlib.sha256(
    capacity_load_evidence_path.read_bytes(),
).hexdigest()
incident_audit_evidence_path = Path(str(target) + ".incident-audit.json")
incident_audit_evidence = {
    "audit_history": {
        "audit_history_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:audit-history",
            "valid": True,
        },
        "audit_tombstone_count": 1,
        "hard_delete_open_records_forbidden": True,
        "required_fields": [
            "actor",
            "reason_sha256",
            "previous_status",
            "new_status",
            "timestamp",
        ],
        "status_history_actor_count": 1,
        "status_history_event_count": 2,
    },
    "checks": {
        "conclusion_passed": True,
        "incident_response_refs_valid": True,
        "operator_environment_recorded": True,
        "recovery_validation_present": True,
        "rto_rpo_breach_escalation_present": True,
        "status_history_actor_present": True,
        "status_history_events_present": True,
    },
    "duration_minutes": 30,
    "environment": "production",
    "errors": [],
    "finished_at": "2026-05-15T00:30:00+00:00",
    "generated_at": "2026-05-15T00:30:01+00:00",
    "incident_audit_policy_version": "tonglingyu-rqa-incident-audit-evidence-v1",
    "incident_drill": {
        "conclusion": "passed",
        "first_response_ref": {
            "kind": "artifact",
            "ref": "artifact:incident-first-response",
            "valid": True,
        },
        "incident_evidence_ref": {
            "kind": "artifact",
            "ref": "artifact:incident-response",
            "valid": True,
        },
        "mitigation_ref": {
            "kind": "artifact",
            "ref": "artifact:incident-mitigation",
            "valid": True,
        },
        "owner": "rqa-oncall",
        "recovery_validation_ref": {
            "kind": "artifact",
            "ref": "artifact:incident-recovery-validation",
            "valid": True,
        },
        "rollback_ref": {
            "kind": "artifact",
            "ref": "artifact:incident-rollback",
            "valid": True,
        },
        "rto_rpo_breach_escalation_ref": {
            "kind": "artifact",
            "ref": "artifact:rto-rpo-breach-escalation",
            "valid": True,
        },
        "severity": "sev2",
    },
    "object": "tonglingyu.rqa_incident_audit_evidence",
    "operator": "release-reviewer",
    "schema_version": 1,
    "secret_values_printed": False,
    "started_at": "2026-05-15T00:00:00+00:00",
    "status": "ok",
}
incident_audit_evidence_path.write_text(
    json.dumps(incident_audit_evidence, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
incident_audit_evidence_sha256 = hashlib.sha256(
    incident_audit_evidence_path.read_bytes(),
).hexdigest()
image_refs = [
    "sha256:" + "a" * 64,
    "sha256:" + "b" * 64,
    "registry.invalid/hermes@sha256:" + "c" * 64,
    "registry.invalid/open-webui@sha256:" + "d" * 64,
    "registry.invalid/cloudflared@sha256:" + "e" * 64,
    "registry.invalid/postgres@sha256:" + "f" * 64,
]
image_refs_sha256 = hashlib.sha256(
    ("\n".join(image_refs) + "\n").encode("utf-8")
).hexdigest()
owned_image_refs = image_refs[:2]
third_party_image_refs = image_refs[2:]
owned_image_refs_sha256 = hashlib.sha256(
    ("\n".join(owned_image_refs) + "\n").encode("utf-8")
).hexdigest()
third_party_image_refs_sha256 = hashlib.sha256(
    ("\n".join(third_party_image_refs) + "\n").encode("utf-8")
).hexdigest()
image_ownership = [
    {"owner_type": "owned", "ref": image_refs[0]},
    {"owner_type": "owned", "ref": image_refs[1]},
    {"owner_type": "third_party", "ref": image_refs[2]},
    {"owner_type": "third_party", "ref": image_refs[3]},
    {"owner_type": "third_party", "ref": image_refs[4]},
    {"owner_type": "third_party", "ref": image_refs[5]},
]
image_scan_artifact_dir = Path(str(target) + ".image-scan-reports").resolve()
image_scan_artifact_dir.mkdir(parents=True, exist_ok=True)
image_raw_report_paths = []
image_report_digests = []
for image_ref in image_refs:
    report_path = image_scan_artifact_dir / (
        "trivy-" + hashlib.sha256(image_ref.encode("utf-8")).hexdigest() + ".json"
    )
    report_path.write_text(
        json.dumps(
            {
                "Results": [
                    {
                        "Target": image_ref,
                        "Vulnerabilities": [],
                    },
                ],
            },
            ensure_ascii=True,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    image_raw_report_paths.append(str(report_path))
    image_report_digests.append(hashlib.sha256(report_path.read_bytes()).hexdigest())
image_raw_report_paths_sha256 = hashlib.sha256(
    ("\n".join(image_raw_report_paths) + "\n").encode("utf-8")
).hexdigest()
image_scanned_reports_sha256 = hashlib.sha256(
    ("\n".join(sorted(image_report_digests)) + "\n").encode("utf-8")
).hexdigest()
restore_artifact_dir = Path(str(target) + ".restore-artifacts").resolve()
restore_artifact_dir.mkdir(parents=True, exist_ok=True)
restore_backup_path = restore_artifact_dir / "backup.db"
restore_backup_path.write_bytes(b"synthetic restore drill backup evidence\n")
restore_artifact_dir_sha256 = hashlib.sha256(
    str(restore_artifact_dir).encode("utf-8")
).hexdigest()
restore_backup_path_sha256 = hashlib.sha256(
    str(restore_backup_path).encode("utf-8")
).hexdigest()
restore_backup_sha256 = hashlib.sha256(restore_backup_path.read_bytes()).hexdigest()
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
    "rqa_migration_preflight": {
        "backup": {
            "artifact_path": "/tmp/tonglingyu-migration-preflight-backup.db",
            "artifact_path_sha256": "1" * 64,
            "artifact_sha256": "2" * 64,
            "before_preflight": True,
            "finished_at": "2026-05-15T00:00:02+00:00",
            "size_bytes": 4096,
            "source_db_sha256": "3" * 64,
            "started_at": "2026-05-15T00:00:01+00:00",
        },
        "checks": {
            "backup_before_preflight": True,
            "backup_created": True,
            "backup_path_recorded": True,
            "no_runtime_data_delete": True,
            "no_runtime_data_rebuild": True,
            "no_secret_values": True,
            "schema_preflight_ran": True,
        },
        "db": {
            "path": "/var/lib/tonglingyu/tonglingyu.db",
            "path_sha256": "4" * 64,
            "size_bytes": 8192,
            "source_db_sha256": "3" * 64,
        },
        "duration_ms": 3000,
        "finished_at": "2026-05-15T00:00:03+00:00",
        "generated_at": "2026-05-15T00:00:03+00:00",
        "migration_counts": {
            "applied": 3,
            "pending": 0,
            "required": 3,
        },
        "migration_preflight": {
            "applied_migrations": [
                "tonglingyu-runtime-schema-v1",
                "tonglingyu-retrieval-failures-v1",
                "tonglingyu-retrieval-failure-dedupe-v1",
            ],
            "contains_secret_values": False,
            "object": "tonglingyu.runtime_schema_migration_preflight",
            "pending_migrations": [],
            "required_migrations": [
                "tonglingyu-runtime-schema-v1",
                "tonglingyu-retrieval-failures-v1",
                "tonglingyu-retrieval-failure-dedupe-v1",
            ],
            "will_delete_runtime_data": False,
            "will_rebuild_knowledge_base": False,
        },
        "migration_preflight_passed": True,
        "mode": "live",
        "object": "tonglingyu.rqa_migration_preflight_gate",
        "policy_version": "tonglingyu-rqa-migration-preflight-v1",
        "require_live": True,
        "schema_version": 1,
        "secret_values_printed": False,
        "source_mode": "existing_db",
        "started_at": "2026-05-15T00:00:00+00:00",
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
        "artifact_dir": str(restore_artifact_dir),
        "artifact_dir_sha256": restore_artifact_dir_sha256,
        "artifacts": {
            "rqa_quality_gate_sha256": "b" * 64,
            "saved_release_report_sha256": "c" * 64,
            "saved_report_validator_sha256": "d" * 64,
        },
        "backup": {
            "artifact_path": str(restore_backup_path),
            "artifact_path_sha256": restore_backup_path_sha256,
            "artifact_sha256": restore_backup_sha256,
            "finished_at": "2026-05-15T00:00:02+00:00",
            "size_bytes": restore_backup_path.stat().st_size,
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
            "governance_task_stable_sort": True,
            "governance_task_unknown_filter_rejected": True,
            "old_client_governance_task_list_compatible": True,
            "old_client_governance_task_read_compatible": True,
            "retrieval_failure_invalid_status_rejected": True,
            "retrieval_failure_list_pagination": True,
            "retrieval_failure_list_schema": True,
            "retrieval_failure_max_page_clamped": True,
            "retrieval_failure_read_schema": True,
            "retrieval_failure_storage_minimized": True,
            "retrieval_failure_stable_sort": True,
            "retrieval_failure_unknown_filter_rejected": True,
            "old_client_retrieval_failure_list_compatible": True,
            "old_client_retrieval_failure_read_compatible": True,
            "additive_response_fields_tolerated": True,
            "unknown_mutation_fields_rejected": True,
            "schema_versions_stable": True,
            "json_metrics_schema": True,
            "json_metrics_excludes_raw_identifiers": True,
            "prometheus_metrics_excludes_raw_identifiers": True,
            "prometheus_label_set_bounded": True,
            "admin_detail_excludes_sensitive_patterns": True,
        },
        "compatibility_policy": {
            "policy_version": "tonglingyu-rqa-api-compatibility-v1",
            "query_unknown_fields": "reject",
            "request_unknown_fields": "reject",
            "response_unknown_fields": "ignore_additive_fields",
            "schema_versions": {
                "governance_task_list": "tonglingyu-knowledge-governance-tasks-v2",
                "governance_task_read": "tonglingyu-knowledge-governance-tasks-v2",
                "retrieval_failure_list": "tonglingyu-retrieval-failures-v1",
                "retrieval_failure_read": "tonglingyu-retrieval-failures-v1",
            },
            "unknown_request_statuses": {
                "governance_task_create_from_failure": 422,
                "governance_task_manual_create": 422,
                "governance_task_update": 422,
                "knowledge_patch_proposal_create": 422,
                "retrieval_failure_cluster": 422,
                "retrieval_failure_update": 422,
            },
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
            "blocking_critical_count": 0,
            "blocking_high_count": 0,
            "critical_count": 0,
            "digest_missing_count": 0,
            "failed_image_count": 0,
            "high_count": 0,
            "image_count": 6,
            "image_finding_summary": [
                {
                    "critical_count": 0,
                    "high_count": 0,
                    "image_ref_sha256": hashlib.sha256(image_ref.encode("utf-8")).hexdigest(),
                    "owner_type": item["owner_type"],
                }
                for image_ref, item in zip(image_refs, image_ownership)
            ],
            "image_ownership": image_ownership,
            "image_policy_version": "tonglingyu-image-ownership-v1",
            "image_refs": image_refs,
            "image_refs_sha256": image_refs_sha256,
            "mutable_tag_count": 0,
            "owned_critical_count": 0,
            "owned_high_count": 0,
            "owned_image_count": 2,
            "owned_image_refs_sha256": owned_image_refs_sha256,
            "report_sha256": "b" * 64,
            "scanner": "trivy",
            "scanned_image_count": 6,
            "scanned_image_refs_sha256": image_refs_sha256,
            "scanned_report_count": 6,
            "scanned_reports_sha256": image_scanned_reports_sha256,
            "third_party_critical_count": 0,
            "third_party_findings_non_blocking": True,
            "third_party_high_count": 0,
            "third_party_image_count": 4,
            "third_party_image_refs_sha256": third_party_image_refs_sha256,
            "raw_reports_persistent": True,
            "raw_report_artifact_dir": str(image_scan_artifact_dir),
            "raw_report_paths": image_raw_report_paths,
            "raw_report_paths_sha256": image_raw_report_paths_sha256,
            "scan_run_id": "synthetic-trivy-run",
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
    "release_ops_readiness": {
        "alert_policy": {
            "conditions": {
                "admin_api_5xx_rate": {
                    "labels": ["status"],
                    "metric": "tonglingyu_admin_api_5xx_total",
                    "owner": "rqa-oncall",
                    "severity": "page",
                    "threshold": "> 0 for 5m",
                },
                "admin_api_latency_p95": {
                    "labels": ["status"],
                    "metric": "tonglingyu_admin_api_latency_ms",
                    "owner": "rqa-oncall",
                    "severity": "ticket",
                    "threshold": "p95 > 2000ms for 10m",
                },
                "open_p0_governance_task": {
                    "labels": ["status", "task_type", "priority"],
                    "metric": "tonglingyu_governance_tasks_total",
                    "owner": "rqa-oncall",
                    "severity": "page",
                    "threshold": "open P0 > 0",
                },
                "open_p0_retrieval_failure": {
                    "labels": ["status", "failure_type"],
                    "metric": "tonglingyu_retrieval_failures_total",
                    "owner": "rqa-oncall",
                    "severity": "page",
                    "threshold": "open P0 > 0",
                },
                "openwebui_admin_action_failure": {
                    "labels": ["status"],
                    "metric": "tonglingyu_openwebui_admin_action_failures_total",
                    "owner": "rqa-oncall",
                    "severity": "ticket",
                    "threshold": "> 0",
                },
                "release_gate_failure": {
                    "labels": ["status"],
                    "metric": "tonglingyu_release_gate_failures_total",
                    "owner": "release-oncall",
                    "severity": "page",
                    "threshold": "> 0",
                },
                "rqa_quality_gate_failure": {
                    "labels": ["status"],
                    "metric": "tonglingyu_rqa_quality_gate_failures_total",
                    "owner": "rqa-oncall",
                    "severity": "page",
                    "threshold": "> 0",
                },
                "rqa_write_failure_rate": {
                    "labels": ["status", "failure_type"],
                    "metric": "tonglingyu_rqa_write_failures_total",
                    "owner": "rqa-oncall",
                    "severity": "page",
                    "threshold": "> 0 for 5m",
                },
            },
            "evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:release-alert-config",
                "valid": True,
            },
            "low_cardinality_labels_only": True,
            "missing_conditions": [],
            "policy_version": "tonglingyu-rqa-release-alerts-v1",
            "required_conditions": [
                "rqa_write_failure_rate",
                "admin_api_5xx_rate",
                "admin_api_latency_p95",
                "open_p0_retrieval_failure",
                "open_p0_governance_task",
                "rqa_quality_gate_failure",
                "release_gate_failure",
                "openwebui_admin_action_failure",
            ],
        },
        "checks": {
            "alert_labels_low_cardinality": True,
            "alerts_defined": True,
            "db_restore_or_additive_downgrade_defined": True,
            "non_production_marker_required": True,
            "post_release_monitor_defined": True,
            "post_rollback_release_readiness_required": True,
            "release_report_reproduction_defined": True,
            "rollback_steps_defined": True,
            "runbook_exists": True,
            "runbook_sections_complete": True,
        },
        "errors": [],
        "evidence": {
            "alert_evidence_ref": "artifact:release-alert-config",
            "post_release_monitor_evidence_sha256": post_release_monitor_evidence_sha256,
            "post_release_monitor_ref": "artifact:post-release-monitor",
            "production_evidence_complete": True,
            "rollback_evidence_ref": "artifact:rollback-drill",
            "rto_rpo_evidence_ref": "artifact:rto-rpo-drill",
        },
        "generated_at": "2026-05-15T00:00:10+00:00",
        "mode": "live",
        "object": "tonglingyu.release_ops_readiness_gate",
        "ops_policy_version": "tonglingyu-rqa-release-ops-v1",
        "post_release_monitor": {
            "admin_action_or_api_evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:live-admin-action-query",
                "valid": True,
            },
            "conclusion": "passed",
            "environment": "production",
            "evidence_errors": [],
            "evidence_path": str(post_release_monitor_evidence_path),
            "evidence_sha256": post_release_monitor_evidence_sha256,
            "evidence_validated": True,
            "live_gate_evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:live-release-gate",
                "valid": True,
            },
            "monitor_ref": {
                "kind": "artifact",
                "ref": "artifact:post-release-monitor",
                "valid": True,
            },
            "operator": "release-reviewer",
            "release_report_path": target,
            "required": True,
            "requires_admin_action_or_api_evidence": True,
            "requires_live_gate_evidence": True,
            "window_minutes": 60,
        },
        "release_ops_ready": True,
        "reproduction": {
            "required_inputs": [
                "git_commit",
                "image_digest",
                "config_digest",
                "source_snapshot_digest",
                "kb_build_hash",
                "security_scan_summary",
                "runtime_profile_digest",
                "prompt_digest",
                "tool_policy_digest",
            ],
            "runbook_ref": "runbook:tonglingyu-rqa-release-runbook#release-report-reproduction",
        },
        "require_live": True,
        "rollback": {
            "db_restore_or_additive_downgrade_defined": True,
            "evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:rollback-drill",
                "valid": True,
            },
            "non_production_marker_required": True,
            "post_rollback_release_readiness_required": True,
        },
        "rto_rpo": {
            "evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:rto-rpo-drill",
                "valid": True,
            },
            "rpo_target_seconds": 3600,
            "rto_target_seconds": 900,
        },
        "runbook": {
            "missing_sections": [],
            "path": runbook_path,
            "ref": "runbook:tonglingyu-rqa-release-runbook",
            "required_sections": [
                "release_flow",
                "migration_preflight",
                "backup",
                "deploy",
                "live_gate",
                "saved_report_validation",
                "rollback_image_config",
                "db_restore_or_additive_downgrade",
                "rto_rpo_restore",
                "alert_policy",
                "incident_response",
                "post_release_monitor",
                "release_report_reproduction",
            ],
            "sha256": runbook_sha256,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "status": "ok",
    },
    "rqa_incident_capacity": {
        "audit_history": {
            "audit_history_evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:audit-history",
                "valid": True,
            },
            "hard_delete_open_records_forbidden": True,
            "required_fields": [
                "actor",
                "reason_sha256",
                "previous_status",
                "new_status",
                "timestamp",
            ],
            "status_history_required": True,
        },
        "capacity_policy": {
            "capacity_evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:capacity-smoke",
                "valid": True,
            },
            "load_evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:load-soak",
                "valid": True,
            },
            "load_measurements": {
                "admin_read_p95_ms": 80,
                "metrics_read_p95_ms": 60,
                "release_gate_ms": 3000,
                "rqa_write_p95_ms": 900,
            },
            "max_in_memory_queue_items": 0,
            "representative_counts": {
                "admin_list_page_count": 2,
                "eval_report_count": 1,
                "failure_count": 1,
            },
            "retry_duplicate_record_forbidden": True,
            "retry_idempotency_required": True,
            "write_queue_policy": "synchronous_write_no_unbounded_queue",
        },
        "capacity_load_evidence": {
            "errors": [],
            "path": str(capacity_load_evidence_path),
            "sha256": capacity_load_evidence_sha256,
            "validated": True,
        },
        "incident_audit_evidence": {
            "errors": [],
            "path": str(incident_audit_evidence_path),
            "sha256": incident_audit_evidence_sha256,
            "validated": True,
        },
        "checks": {
            "audit_history_live_evidence_required": True,
            "capacity_load_evidence_validated": True,
            "capacity_live_evidence_required": True,
            "emergency_flags_fail_closed": True,
            "hard_delete_open_records_forbidden": True,
            "incident_audit_evidence_validated": True,
            "incident_runbook_defined": True,
            "load_live_evidence_required": True,
            "no_unbounded_queue": True,
            "public_degraded_response_defined": True,
            "retry_idempotency_defined": True,
            "status_history_audit_defined": True,
        },
        "emergency_state": {
            "degraded_mode": False,
            "emergency_disabled": False,
            "non_production_required": False,
            "persistence_degraded": False,
            "production_allowed": True,
        },
        "errors": [],
        "evidence": {
            "audit_history_evidence_ref": "artifact:audit-history",
            "capacity_load_evidence_sha256": capacity_load_evidence_sha256,
            "capacity_evidence_complete": True,
            "capacity_evidence_ref": "artifact:capacity-smoke",
            "incident_evidence_ref": "artifact:incident-response",
            "incident_audit_evidence_sha256": incident_audit_evidence_sha256,
            "load_evidence_ref": "artifact:load-soak",
        },
        "generated_at": "2026-05-15T00:00:11+00:00",
        "incident_capacity_ready": True,
        "incident_runbook": {
            "incident_evidence_ref": {
                "kind": "artifact",
                "ref": "artifact:incident-response",
                "valid": True,
            },
            "path": runbook_path,
            "ref": "runbook:tonglingyu-rqa-release-runbook#incident-response",
            "rto_rpo_breach_escalation_defined": True,
            "severity_owner_first_response_defined": True,
            "sha256": runbook_sha256,
        },
        "mode": "live",
        "object": "tonglingyu.rqa_incident_capacity_gate",
        "policy_version": "tonglingyu-rqa-incident-capacity-v1",
        "public_degraded_response": {
            "full_success_forbidden": True,
            "stable_status_required": True,
            "trace_id_required": True,
        },
        "require_live": True,
        "schema_version": 1,
        "secret_values_printed": False,
        "status": "ok",
    },
    "openwebui_admin_action_contract": {
        "action": {
            "function_id": "tonglingyu_gateway_admin",
            "required_actions": [
                "metrics",
                "trace",
                "package",
                "session",
                "retrieval_failures",
                "retrieval_failure",
                "retrieval_failure_update",
                "retrieval_failure_cluster",
                "governance_tasks",
                "governance_task",
                "governance_task_create",
                "governance_task_from_failure",
                "governance_task_update",
                "knowledge_patch_proposal",
            ],
            "source_sha256": "1" * 64,
            "test_sha256": "2" * 64,
            "type": "action",
        },
        "checks": {
            "admin_actions_required": True,
            "admin_key_not_printed": True,
            "admin_role_guard_required": True,
            "empty_admin_key_rejected": True,
            "py_compile_passed": True,
            "required_valves_present": True,
            "rqa_list_response_contract_tested": True,
            "unit_tests_passed": True,
            "valid_fixture_passed": True,
        },
        "contract_version": "tonglingyu-openwebui-admin-action-contract-v1",
        "errors": [],
        "feedback_action": {
            "function_id": "tonglingyu_gateway_feedback",
            "source_sha256": "3" * 64,
            "test_sha256": "4" * 64,
            "type": "action",
        },
        "fixture_validation": {
            "negative_fixture_count": 2,
            "source": "fixture-json",
            "valve_keys": [
                "GATEWAY_ADMIN_API_KEY",
                "GATEWAY_BASE_URL",
                "TARGET_MODEL",
                "TARGET_MODELS",
            ],
        },
        "generated_at": "2026-05-15T00:00:09+00:00",
        "object": "tonglingyu.openwebui_admin_action_contract_gate",
        "schema_version": 1,
        "secret_values_printed": False,
        "status": "ok",
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
        "behavior_config_binding": behavior_config_binding,
        "checked_surfaces": ["tonglingyu-gateway:/healthz"],
        "metrics_privacy": {
            "object": "tonglingyu.strict_gateway_metrics_privacy",
            "schema_version": 1,
            "json_metrics_sensitive_paths": [],
            "json_metrics_sensitive_paths_sha256": digest_json([]),
            "prometheus_sensitive_tokens": [],
            "prometheus_sensitive_tokens_sha256": digest_json([]),
            "json_metrics_secret_values_present": False,
            "prometheus_secret_values_present": False,
            "secret_values_printed": False,
        },
        "model_ids": ["tonglingyu"],
        "running_images": {
            "generated_at": "2026-05-15T00:00:10+00:00",
            "image_count": 2,
            "images": [
                {
                    "configured_image": "registry.invalid/open-webui@sha256:" + "d" * 64,
                    "container_id_sha256": "6" * 64,
                    "image_id": "sha256:" + "d" * 64,
                    "image_id_sha256": "7" * 64,
                    "repo_digests": [
                        "registry.invalid/open-webui@sha256:" + "d" * 64,
                    ],
                    "repo_digests_sha256": "8" * 64,
                    "service": "open-webui",
                },
                {
                    "configured_image": "registry.invalid/tonglingyu-gateway@sha256:" + "b" * 64,
                    "container_id_sha256": "9" * 64,
                    "image_id": "sha256:" + "b" * 64,
                    "image_id_sha256": "a" * 64,
                    "repo_digests": [
                        "registry.invalid/tonglingyu-gateway@sha256:" + "b" * 64,
                    ],
                    "repo_digests_sha256": "c" * 64,
                    "service": "tonglingyu-gateway",
                },
            ],
            "object": "tonglingyu.running_image_inventory",
            "schema_version": 1,
            "secret_values_printed": False,
        },
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
        "checks": {
            "active_global_action": True,
            "admin_key_not_printed": True,
            "admin_role_denial_defined": True,
            "admin_role_guard_required": True,
            "required_valves_present": True,
            "rqa_admin_actions_present": True,
            "rqa_admin_api_paths_present": True,
            "target_models_bound": True,
        },
        "function_id": "tonglingyu_gateway_admin",
        "is_active": True,
        "is_global": True,
        "object": "tonglingyu.openwebui_admin_action_live_gate",
        "permission_boundary": {
            "admin_key_valve_bound": True,
            "admin_role_denial_defined": True,
            "admin_role_guard_required": True,
            "required_actions": [
                "metrics",
                "trace",
                "package",
                "session",
                "retrieval_failures",
                "retrieval_failure",
                "retrieval_failure_update",
                "retrieval_failure_cluster",
                "governance_tasks",
                "governance_task",
                "governance_task_create",
                "governance_task_from_failure",
                "governance_task_update",
                "knowledge_patch_proposal",
            ],
            "required_api_paths": [
                "/v1/admin/metrics",
                "/v1/admin/traces/",
                "/v1/admin/packages/",
                "/v1/admin/sessions/",
                "/v1/admin/retrieval-failures",
                "/v1/admin/governance/tasks",
                "/v1/admin/governance/proposals",
            ],
            "target_models_bound": True,
        },
        "schema_version": 1,
        "secret_values_printed": False,
        "source": "admin-api",
        "status": "ok",
        "type": "action",
        "valve_keys": [
            "GATEWAY_ADMIN_API_KEY",
            "GATEWAY_BASE_URL",
            "TARGET_MODEL",
            "TARGET_MODELS",
        ],
    },
}
gate_stdout["rqa_migration_preflight"]["migration_preflight_sha256"] = hashlib.sha256(
    json.dumps(
        gate_stdout["rqa_migration_preflight"]["migration_preflight"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
for gate in report["gates"]:
    if gate.get("name") in gate_stdout:
        gate["stdout_tail"] = [json.dumps(gate_stdout[gate["name"]], sort_keys=True)]


def canonical_digest(value):
    return hashlib.sha256(
        json.dumps(
            value,
            ensure_ascii=True,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()


runtime_config = gate_stdout["runtime_config"]
migration_gate = gate_stdout["rqa_migration_preflight"]
rqa_gate = gate_stdout["retrieval_quality"]
security_gate = gate_stdout["security_scan"]
behavior = rqa_gate["behavior_config"]
image_scan = security_gate["image_scan"]
kb_version = rqa_gate["kb_version"]
migration_backup = migration_gate["backup"]
migration_db = migration_gate["db"]
migration_counts = migration_gate["migration_counts"]
release_manifest = {
    "object": "tonglingyu.release_manifest",
    "schema_version": 1,
    "git": {
        "commit": "f" * 40,
        "tracked_dirty": False,
    },
    "runtime_config": {
        "config_digest": canonical_digest(runtime_config),
        "config_mode": runtime_config.get("config_mode"),
        "checked_policy_fields": runtime_config.get("checked_policy_fields"),
        "checked_services": runtime_config.get("checked_services"),
    },
    "rqa": {
        "rqa_schema_version": rqa_gate["rqa_schema_version"],
        "eval_suite_version": rqa_gate["eval_suite_version"],
        "eval_run_id": rqa_gate["eval_run_id"],
        "eval_report_sha256": rqa_gate["eval_report_sha256"],
        "source_snapshot_digest": rqa_gate["source_snapshot_digest"],
        "kb_build_hash": rqa_gate["kb_build_hash"],
        "kb_version": {
            "version_id": kb_version.get("version_id"),
            "schema_version": kb_version.get("schema_version"),
            "source_count": kb_version.get("source_count"),
            "block_count": kb_version.get("block_count"),
            "built_at": kb_version.get("built_at"),
        },
        "source_license_summary_digest": canonical_digest(
            rqa_gate["source_license_summary"]
        ),
    },
    "migration": {
        "applied_migration_count": migration_counts["applied"],
        "backup_artifact_path": migration_backup["artifact_path"],
        "backup_artifact_path_sha256": migration_backup["artifact_path_sha256"],
        "backup_artifact_sha256": migration_backup["artifact_sha256"],
        "migration_preflight_sha256": migration_gate["migration_preflight_sha256"],
        "mode": migration_gate["mode"],
        "pending_migration_count": migration_counts["pending"],
        "policy_version": migration_gate["policy_version"],
        "require_live": migration_gate["require_live"],
        "required_migration_count": migration_counts["required"],
        "source_db_sha256": migration_db["source_db_sha256"],
        "source_mode": migration_gate["source_mode"],
    },
    "behavior_config": {
        "behavior_config_digest": behavior["behavior_config_digest"],
        "runtime_profile_digest": behavior["runtime_profile_digest"],
        "prompt_digest": behavior["prompt_digest"],
        "tool_policy_digest": behavior["tool_policy_digest"],
        "reviewer_policy_digest": behavior["reviewer_policy_digest"],
        "gateway_policy_digest": behavior["gateway_policy_digest"],
        "model_upstream_id": behavior["model_upstream_id"],
        "model_upstream_bound_by_gate": behavior["model_upstream_bound_by_gate"],
        "decoding_parameters_source": behavior["decoding_parameters_source"],
        "decoding_parameters_summary": behavior["decoding_parameters_summary"],
    },
    "security": {
        "dependency_scan_sha256": security_gate["dependency_scan"]["report_sha256"],
        "image_count": image_scan["image_count"],
        "image_refs": image_scan["image_refs"],
        "image_refs_sha256": image_scan["image_refs_sha256"],
        "digest_missing_count": image_scan["digest_missing_count"],
        "mutable_tag_count": image_scan["mutable_tag_count"],
        "scanned_image_count": image_scan["scanned_image_count"],
        "scanned_image_refs_sha256": image_scan["scanned_image_refs_sha256"],
        "scanned_report_count": image_scan["scanned_report_count"],
        "scanned_reports_sha256": image_scan["scanned_reports_sha256"],
        "raw_reports_persistent": image_scan["raw_reports_persistent"],
        "raw_report_artifact_dir": image_scan["raw_report_artifact_dir"],
        "raw_report_paths_sha256": image_scan["raw_report_paths_sha256"],
    },
}
report["release_manifest"] = release_manifest
report["release_manifest_digest"] = canonical_digest(release_manifest)
strict_gate = gate_stdout["strict_gateway"]
release_runtime_identity = {
    "object": "tonglingyu.release_runtime_identity",
    "schema_version": 1,
    "policy_version": "tonglingyu-release-runtime-identity-v1",
    "require_live": True,
    "git": dict(release_manifest["git"]),
    "image_inventory": {
        "source_gate": "security_scan",
        "image_count": image_scan["image_count"],
        "image_refs": image_scan["image_refs"],
        "image_refs_sha256": image_scan["image_refs_sha256"],
        "digest_missing_count": image_scan["digest_missing_count"],
        "mutable_tag_count": image_scan["mutable_tag_count"],
        "scanned_image_count": image_scan["scanned_image_count"],
        "scanned_report_count": image_scan["scanned_report_count"],
        "scanned_reports_sha256": image_scan["scanned_reports_sha256"],
        "raw_reports_persistent": image_scan["raw_reports_persistent"],
        "raw_report_artifact_dir": image_scan["raw_report_artifact_dir"],
        "raw_report_paths_sha256": image_scan["raw_report_paths_sha256"],
    },
    "running_images": {
        "source_gate": "strict_gateway",
        "inventory": strict_gate["running_images"],
        "inventory_sha256": canonical_digest(strict_gate["running_images"]),
        "image_count": len(strict_gate["running_images"]["images"]),
    },
    "migration": {
        "source_gate": "rqa_migration_preflight",
        "policy_version": release_manifest["migration"]["policy_version"],
        "mode": release_manifest["migration"]["mode"],
        "source_mode": release_manifest["migration"]["source_mode"],
        "source_db_sha256": release_manifest["migration"]["source_db_sha256"],
        "preflight_sha256": release_manifest["migration"]["migration_preflight_sha256"],
        "backup_artifact_sha256": release_manifest["migration"]["backup_artifact_sha256"],
        "required_migration_count": release_manifest["migration"]["required_migration_count"],
        "applied_migration_count": release_manifest["migration"]["applied_migration_count"],
        "pending_migration_count": release_manifest["migration"]["pending_migration_count"],
    },
    "valid": True,
    "errors": [],
    "secret_values_printed": False,
}
report["release_runtime_identity"] = release_runtime_identity
report["release_runtime_identity_digest"] = canonical_digest(release_runtime_identity)


def artifact_entry(
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
    return {
        "name": name,
        "artifact_type": artifact_type,
        "digest_sha256": digest,
        "source_gate": source_gate,
        "ref": ref,
        "path": path,
        "retention_class": retention_class,
        "required_for_production": required_for_production,
    }


registry_entries = [
    artifact_entry(
        "release_manifest",
        "inline_json",
        report["release_manifest_digest"],
        "release_readiness",
        ref="release_manifest",
        retention_class="release_manifest",
    ),
    artifact_entry(
        "release_context",
        "inline_json",
        report["release_context_digest"],
        "release_readiness",
        ref=report["release_context"]["environment"],
        retention_class="release_manifest",
    ),
    artifact_entry(
        "release_runtime_identity",
        "inline_json",
        report["release_runtime_identity_digest"],
        "release_readiness",
        ref=release_manifest["git"]["commit"],
        retention_class="release_manifest",
    ),
    artifact_entry(
        "runtime_config",
        "gate_stdout",
        release_manifest["runtime_config"]["config_digest"],
        "runtime_config",
        ref="runtime_config",
    ),
    artifact_entry(
        "rqa_eval_report",
        "local_file",
        release_manifest["rqa"]["eval_report_sha256"],
        "retrieval_quality",
        ref=release_manifest["rqa"]["eval_run_id"],
        path=rqa_gate["eval_report_path"],
    ),
    artifact_entry(
        "source_license_summary",
        "inline_json",
        release_manifest["rqa"]["source_license_summary_digest"],
        "retrieval_quality",
        ref=release_manifest["rqa"]["source_snapshot_digest"],
    ),
    artifact_entry(
        "migration_preflight",
        "inline_json",
        release_manifest["migration"]["migration_preflight_sha256"],
        "rqa_migration_preflight",
        ref=release_manifest["migration"]["policy_version"],
    ),
    artifact_entry(
        "migration_backup",
        "sqlite_backup",
        release_manifest["migration"]["backup_artifact_sha256"],
        "rqa_migration_preflight",
        ref=release_manifest["migration"]["source_db_sha256"],
        path=release_manifest["migration"]["backup_artifact_path"],
    ),
    artifact_entry(
        "behavior_config",
        "inline_json",
        release_manifest["behavior_config"]["behavior_config_digest"],
        "retrieval_quality",
        ref=release_manifest["behavior_config"]["model_upstream_id"],
    ),
    artifact_entry(
        "dependency_scan",
        "scan_report",
        release_manifest["security"]["dependency_scan_sha256"],
        "security_scan",
        ref="cargo-audit",
    ),
    artifact_entry(
        "image_inventory",
        "inline_json",
        release_manifest["security"]["image_refs_sha256"],
        "security_scan",
        ref=f"images:{release_manifest['security']['image_count']}",
    ),
    artifact_entry(
        "image_scan_reports",
        "scan_report_collection",
        release_manifest["security"]["scanned_reports_sha256"],
        "security_scan",
        ref=release_manifest["security"]["raw_report_paths_sha256"],
        path=release_manifest["security"]["raw_report_artifact_dir"],
    ),
]
browser_validation = report.get("browser_review_validation")
if isinstance(browser_validation, dict):
    registry_entries.append(artifact_entry(
        "browser_review_evidence",
        "local_file",
        browser_validation["evidence_sha256"],
        "openwebui_browser_review",
        ref=report["browser_review_ref"],
        path=report["browser_review_evidence"],
    ))
release_artifact_registry = {
    "object": "tonglingyu.release_artifact_registry",
    "schema_version": 1,
    "policy_version": "tonglingyu-release-artifact-registry-v1",
    "generated_at": "2026-05-15T00:00:11+00:00",
    "retention_days": 365,
    "legal_hold_supported": True,
    "entries": registry_entries,
    "secret_values_printed": False,
}
report["release_artifact_registry"] = release_artifact_registry
report["release_artifact_registry_digest"] = canonical_digest(release_artifact_registry)
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
"${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${SYNTHETIC_READY_REPORT}" >/dev/null

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_MANIFEST_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["release_manifest"]["rqa"]["source_snapshot_digest"] = "0" * 64
report["release_manifest_digest"] = hashlib.sha256(
    json.dumps(
        report["release_manifest"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_manifest_stdout="${WORK_DIR}/tampered-release-manifest.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_MANIFEST_REPORT}" >"${tampered_release_manifest_stdout}"; then
  echo "production-ready reports must bind release manifest to RQA source snapshot" >&2
  exit 1
fi
assert_report "${tampered_release_manifest_stdout}" \
  '"release_manifest_rqa_source_snapshot_digest_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_MANIFEST_DIGEST_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["release_manifest_digest"] = "0" * 64
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_manifest_digest_stdout="${WORK_DIR}/tampered-release-manifest-digest.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_MANIFEST_DIGEST_REPORT}" \
  >"${tampered_release_manifest_digest_stdout}"; then
  echo "production-ready reports must bind release manifest digest" >&2
  exit 1
fi
assert_report "${tampered_release_manifest_digest_stdout}" \
  '"release_manifest_digest_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_ARTIFACT_REGISTRY_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
registry = report["release_artifact_registry"]
registry["entries"] = [
    entry
    for entry in registry["entries"]
    if entry.get("name") != "rqa_eval_report"
]
report["release_artifact_registry_digest"] = hashlib.sha256(
    json.dumps(
        registry,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_artifact_registry_stdout="${WORK_DIR}/tampered-artifact-registry.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_ARTIFACT_REGISTRY_REPORT}" >"${tampered_artifact_registry_stdout}"; then
  echo "production-ready reports must keep RQA eval artifact in the registry" >&2
  exit 1
fi
assert_report "${tampered_artifact_registry_stdout}" \
  '"production_ready_missing_artifact_registry_entry=rqa_eval_report" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_ARTIFACT_REGISTRY_DIGEST_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["release_artifact_registry_digest"] = "0" * 64
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_artifact_registry_digest_stdout="${WORK_DIR}/tampered-artifact-registry-digest.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_ARTIFACT_REGISTRY_DIGEST_REPORT}" \
  >"${tampered_artifact_registry_digest_stdout}"; then
  echo "production-ready reports must bind release artifact registry digest" >&2
  exit 1
fi
assert_report "${tampered_artifact_registry_digest_stdout}" \
  '"release_artifact_registry_digest_mismatch" in report["errors"]'

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
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["generated_at"] = "2000-01-01T00:00:00Z"
report["release_context"]["generated_at"] = "2000-01-01T00:00:00Z"
report["release_context"]["valid_until"] = "2000-01-02T00:00:00Z"
context_digest = hashlib.sha256(
    json.dumps(
        report["release_context"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
report["release_context_digest"] = context_digest
for entry in report["release_artifact_registry"]["entries"]:
    if entry.get("name") == "release_context":
        entry["digest_sha256"] = context_digest
report["release_artifact_registry_digest"] = hashlib.sha256(
    json.dumps(
        report["release_artifact_registry"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
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

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_CONTEXT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report.pop("release_context", None)
report.pop("release_context_digest", None)
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_context_stdout="${WORK_DIR}/tampered-release-context.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_CONTEXT_REPORT}" >"${tampered_release_context_stdout}"; then
  echo "production-ready reports must bind release context" >&2
  exit 1
fi
assert_report "${tampered_release_context_stdout}" \
  '"release_context_missing" in report["errors"]'
assert_report "${tampered_release_context_stdout}" \
  '"production_ready_requires_valid_release_context" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_CONTEXT_VALIDITY_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report["release_context"]["valid_until"] = report["release_context"]["generated_at"]
context_digest = hashlib.sha256(
    json.dumps(
        report["release_context"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
report["release_context_digest"] = context_digest
for entry in report["release_artifact_registry"]["entries"]:
    if entry.get("name") == "release_context":
        entry["digest_sha256"] = context_digest
report["release_artifact_registry_digest"] = hashlib.sha256(
    json.dumps(
        report["release_artifact_registry"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_context_validity_stdout="${WORK_DIR}/tampered-release-context-validity.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_CONTEXT_VALIDITY_REPORT}" \
  >"${tampered_release_context_validity_stdout}"; then
  echo "production-ready reports must bind a future validity window" >&2
  exit 1
fi
assert_report "${tampered_release_context_validity_stdout}" \
  '"release_context_valid_until_not_after_generated_at" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RUNTIME_IDENTITY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
report.pop("release_runtime_identity", None)
report.pop("release_runtime_identity_digest", None)
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_runtime_identity_stdout="${WORK_DIR}/tampered-runtime-identity.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RUNTIME_IDENTITY_REPORT}" >"${tampered_runtime_identity_stdout}"; then
  echo "production-ready reports must bind runtime identity" >&2
  exit 1
fi
assert_report "${tampered_runtime_identity_stdout}" \
  '"release_runtime_identity_missing" in report["errors"]'
assert_report "${tampered_runtime_identity_stdout}" \
  '"production_ready_requires_valid_runtime_identity" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RUNTIME_IDENTITY_IMAGES_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
identity = report["release_runtime_identity"]
identity["running_images"]["inventory"]["images"] = []
identity["running_images"]["inventory"]["image_count"] = 0
identity["running_images"]["inventory_sha256"] = hashlib.sha256(
    json.dumps(
        identity["running_images"]["inventory"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
identity["running_images"]["image_count"] = 0
report["release_runtime_identity_digest"] = hashlib.sha256(
    json.dumps(
        identity,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
for entry in report["release_artifact_registry"]["entries"]:
    if entry.get("name") == "release_runtime_identity":
        entry["digest_sha256"] = report["release_runtime_identity_digest"]
report["release_artifact_registry_digest"] = hashlib.sha256(
    json.dumps(
        report["release_artifact_registry"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_runtime_identity_images_stdout="${WORK_DIR}/tampered-runtime-identity-images.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RUNTIME_IDENTITY_IMAGES_REPORT}" \
  >"${tampered_runtime_identity_images_stdout}"; then
  echo "production-ready reports must bind running image identity" >&2
  exit 1
fi
assert_report "${tampered_runtime_identity_images_stdout}" \
  '"release_runtime_identity_errors_mismatch" in report["errors"]'
assert_report "${tampered_runtime_identity_images_stdout}" \
  '"production_ready_requires_valid_runtime_identity" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_BEHAVIOR_BINDING_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") != "strict_gateway":
        continue
    gate_json = json.loads(gate["stdout_tail"][-1])
    gate_json["behavior_config_binding"]["behavior_config_digest"] = "0" * 64
    gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_behavior_binding_stdout="${WORK_DIR}/tampered-behavior-binding.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_BEHAVIOR_BINDING_REPORT}" >"${tampered_behavior_binding_stdout}"; then
  echo "production-ready reports must bind behavior config to admin trace summary" >&2
  exit 1
fi
assert_report "${tampered_behavior_binding_stdout}" \
  '"strict_gateway_behavior_config_binding_behavior_config_digest_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_STRICT_GATEWAY_METRICS_PRIVACY_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") != "strict_gateway":
        continue
    gate_json = json.loads(gate["stdout_tail"][-1])
    metrics_privacy = gate_json["metrics_privacy"]
    metrics_privacy["prometheus_sensitive_tokens"] = ["trace_id"]
    metrics_privacy["prometheus_sensitive_tokens_sha256"] = hashlib.sha256(
        json.dumps(
            metrics_privacy["prometheus_sensitive_tokens"],
            ensure_ascii=True,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()
    gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_strict_gateway_metrics_privacy_stdout="${WORK_DIR}/tampered-strict-gateway-metrics-privacy.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_STRICT_GATEWAY_METRICS_PRIVACY_REPORT}" \
  >"${tampered_strict_gateway_metrics_privacy_stdout}"; then
  echo "production-ready reports must reject strict Gateway metrics privacy leaks" >&2
  exit 1
fi
assert_report "${tampered_strict_gateway_metrics_privacy_stdout}" \
  '"strict_gateway_metrics_privacy_prometheus_sensitive_tokens_present" in report["errors"]'

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

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_MIGRATION_PREFLIGHT_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_migration_preflight":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_migration_preflight_stdout="${WORK_DIR}/tampered-rqa-migration-preflight-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_MIGRATION_PREFLIGHT_STDOUT_REPORT}" \
  >"${tampered_rqa_migration_preflight_stdout}"; then
  echo "production-ready reports must bind migration preflight status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_migration_preflight_stdout}" \
  '"rqa_migration_preflight_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_MIGRATION_PREFLIGHT_BACKUP_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_migration_preflight":
        gate_json = json.loads(gate["stdout_tail"][-1])
        gate_json["backup"]["artifact_path"] = ""
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
report["release_manifest"]["migration"]["backup_artifact_path"] = ""
report["release_manifest_digest"] = hashlib.sha256(
    json.dumps(
        report["release_manifest"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
for entry in report["release_artifact_registry"]["entries"]:
    if entry.get("name") == "migration_backup":
        entry["path"] = ""
report["release_artifact_registry_digest"] = hashlib.sha256(
    json.dumps(
        report["release_artifact_registry"],
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
).hexdigest()
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_migration_preflight_backup_stdout="${WORK_DIR}/tampered-rqa-migration-preflight-backup.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_MIGRATION_PREFLIGHT_BACKUP_REPORT}" \
  >"${tampered_rqa_migration_preflight_backup_stdout}"; then
  echo "production-ready reports must bind migration backup path evidence" >&2
  exit 1
fi
assert_report "${tampered_rqa_migration_preflight_backup_stdout}" \
  '"rqa_migration_preflight_backup_path_missing" in report["errors"]'

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

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_RESTORE_BACKUP_ARTIFACT_REPORT}" <<'PY'
import hashlib
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_backup_restore_drill":
        gate_json = json.loads(gate["stdout_tail"][0])
        missing_path = "/tmp/tonglingyu-missing-restore-backup.db"
        gate_json["backup"]["artifact_path"] = missing_path
        gate_json["backup"]["artifact_path_sha256"] = hashlib.sha256(
            missing_path.encode("utf-8")
        ).hexdigest()
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_restore_backup_artifact_stdout="${WORK_DIR}/tampered-rqa-restore-backup-artifact.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_RESTORE_BACKUP_ARTIFACT_REPORT}" >"${tampered_rqa_restore_backup_artifact_stdout}"; then
  echo "production-ready reports must reject missing RQA restore backup artifacts" >&2
  exit 1
fi
assert_report "${tampered_rqa_restore_backup_artifact_stdout}" \
  '"production_ready_rqa_restore_backup_artifact_missing" in report["errors"]'

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

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_SECURITY_GATE_IMAGE_INVENTORY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "security_scan":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["image_scan"]["scanned_image_refs_sha256"] = "0" * 64
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_security_gate_image_inventory_stdout="${WORK_DIR}/tampered-security-gate-image-inventory.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_SECURITY_GATE_IMAGE_INVENTORY_REPORT}" >"${tampered_security_gate_image_inventory_stdout}"; then
  echo "production-ready reports must reject image scans not bound to release image refs" >&2
  exit 1
fi
assert_report "${tampered_security_gate_image_inventory_stdout}" \
  '"security_scan_image_inventory_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_SECURITY_GATE_IMAGE_RAW_REPORTS_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "security_scan":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["image_scan"]["raw_reports_persistent"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_security_gate_image_raw_reports_stdout="${WORK_DIR}/tampered-security-gate-image-raw-reports.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_SECURITY_GATE_IMAGE_RAW_REPORTS_REPORT}" >"${tampered_security_gate_image_raw_reports_stdout}"; then
  echo "production-ready reports must reject image scans without persistent raw reports" >&2
  exit 1
fi
assert_report "${tampered_security_gate_image_raw_reports_stdout}" \
  '"security_scan_image_raw_reports_not_persistent" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_OPS_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "release_ops_readiness":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_ops_stdout="${WORK_DIR}/tampered-release-ops-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_OPS_STDOUT_REPORT}" >"${tampered_release_ops_stdout}"; then
  echo "production-ready reports must bind release ops status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_release_ops_stdout}" \
  '"release_ops_readiness_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_OPS_MONITOR_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "release_ops_readiness":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["evidence"]["production_evidence_complete"] = False
        gate_json["post_release_monitor"]["live_gate_evidence_ref"] = {
            "kind": "",
            "ref": "",
            "valid": False,
        }
        gate_json["post_release_monitor"]["evidence_validated"] = False
        gate_json["post_release_monitor"]["evidence_errors"] = [
            "post_release_monitor_evidence_failed_live_gates_present"
        ]
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_ops_monitor_stdout="${WORK_DIR}/tampered-release-ops-monitor.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_OPS_MONITOR_REPORT}" >"${tampered_release_ops_monitor_stdout}"; then
  echo "production-ready reports must reject missing post-release monitor evidence" >&2
  exit 1
fi
assert_report "${tampered_release_ops_monitor_stdout}" \
  '"release_ops_production_evidence_incomplete" in report["errors"]'
assert_report "${tampered_release_ops_monitor_stdout}" \
  '"release_ops_post_release_live_gate_ref_missing" in report["errors"]'
assert_report "${tampered_release_ops_monitor_stdout}" \
  '"release_ops_post_release_evidence_not_validated" in report["errors"]'
assert_report "${tampered_release_ops_monitor_stdout}" \
  '"release_ops_post_release_evidence_errors_present" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RELEASE_OPS_ALERT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "release_ops_readiness":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["alert_policy"]["conditions"]["rqa_write_failure_rate"]["labels"].append("trace_id")
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_release_ops_alert_stdout="${WORK_DIR}/tampered-release-ops-alert.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RELEASE_OPS_ALERT_REPORT}" >"${tampered_release_ops_alert_stdout}"; then
  echo "production-ready reports must reject high-cardinality alert labels" >&2
  exit 1
fi
assert_report "${tampered_release_ops_alert_stdout}" \
  '"release_ops_alert_rqa_write_failure_rate_forbidden_label=trace_id" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_INCIDENT_CAPACITY_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_incident_capacity":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_incident_capacity_stdout="${WORK_DIR}/tampered-rqa-incident-capacity-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_INCIDENT_CAPACITY_STDOUT_REPORT}" >"${tampered_rqa_incident_capacity_stdout}"; then
  echo "production-ready reports must bind incident/capacity status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_rqa_incident_capacity_stdout}" \
  '"rqa_incident_capacity_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_INCIDENT_CAPACITY_EMERGENCY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_incident_capacity":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["emergency_state"]["emergency_disabled"] = True
        gate_json["emergency_state"]["production_allowed"] = False
        gate_json["emergency_state"]["non_production_required"] = True
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_incident_capacity_emergency_stdout="${WORK_DIR}/tampered-rqa-incident-capacity-emergency.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_INCIDENT_CAPACITY_EMERGENCY_REPORT}" >"${tampered_rqa_incident_capacity_emergency_stdout}"; then
  echo "production-ready reports must reject emergency-disabled RQA state" >&2
  exit 1
fi
assert_report "${tampered_rqa_incident_capacity_emergency_stdout}" \
  '"rqa_incident_capacity_emergency_disabled" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_emergency_stdout}" \
  '"rqa_incident_capacity_production_not_allowed" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_INCIDENT_CAPACITY_EVIDENCE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_incident_capacity":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["evidence"]["capacity_evidence_complete"] = False
        gate_json["capacity_policy"]["representative_counts"]["admin_list_page_count"] = 1
        gate_json["capacity_policy"]["capacity_evidence_ref"] = {
            "kind": "",
            "ref": "",
            "valid": False,
        }
        gate_json["capacity_load_evidence"]["validated"] = False
        gate_json["capacity_load_evidence"]["errors"] = [
            "capacity_load_evidence_admin_list_page_count_mismatch"
        ]
        gate_json["incident_audit_evidence"]["validated"] = False
        gate_json["incident_audit_evidence"]["errors"] = [
            "incident_audit_evidence_status_history_event_count_invalid"
        ]
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_incident_capacity_evidence_stdout="${WORK_DIR}/tampered-rqa-incident-capacity-evidence.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_INCIDENT_CAPACITY_EVIDENCE_REPORT}" >"${tampered_rqa_incident_capacity_evidence_stdout}"; then
  echo "production-ready reports must reject missing capacity evidence" >&2
  exit 1
fi
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_evidence_incomplete" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_capacity_evidence_ref_missing" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_admin_list_page_count_below_minimum" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_capacity_load_evidence_not_validated" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_capacity_load_evidence_errors_present" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_incident_audit_evidence_not_validated" in report["errors"]'
assert_report "${tampered_rqa_incident_capacity_evidence_stdout}" \
  '"rqa_incident_capacity_incident_audit_evidence_errors_present" in report["errors"]'

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
        gate_json["checks"]["admin_detail_excludes_sensitive_patterns"] = False
        gate_json["checks"]["prometheus_label_set_bounded"] = False
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
assert_report "${tampered_rqa_api_contract_check_stdout}" \
  '"rqa_api_contract_check_failed=admin_detail_excludes_sensitive_patterns" in report["errors"]'
assert_report "${tampered_rqa_api_contract_check_stdout}" \
  '"rqa_api_contract_check_failed=prometheus_label_set_bounded" in report["errors"]'

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

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_RQA_API_CONTRACT_POLICY_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "rqa_api_contract":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["compatibility_policy"]["request_unknown_fields"] = "ignore"
        gate_json["compatibility_policy"]["unknown_request_statuses"]["governance_task_update"] = 200
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_rqa_api_contract_policy_stdout="${WORK_DIR}/tampered-rqa-api-contract-policy.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_RQA_API_CONTRACT_POLICY_REPORT}" >"${tampered_rqa_api_contract_policy_stdout}"; then
  echo "production-ready reports must reject incomplete RQA API compatibility policies" >&2
  exit 1
fi
assert_report "${tampered_rqa_api_contract_policy_stdout}" \
  '"rqa_api_contract_request_unknown_fields_policy_invalid" in report["errors"]'
assert_report "${tampered_rqa_api_contract_policy_stdout}" \
  '"rqa_api_contract_governance_task_update_unknown_request_status_invalid" in report["errors"]'

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

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_STDOUT_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "openwebui_admin_action_contract":
        gate["stdout_tail"] = []
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_openwebui_admin_action_contract_stdout="${WORK_DIR}/tampered-openwebui-admin-action-contract-stdout.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_STDOUT_REPORT}" >"${tampered_openwebui_admin_action_contract_stdout}"; then
  echo "production-ready reports must bind Open WebUI admin Action contract status to gate stdout" >&2
  exit 1
fi
assert_report "${tampered_openwebui_admin_action_contract_stdout}" \
  '"openwebui_admin_action_contract_stdout_success_json_missing" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_CHECK_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "openwebui_admin_action_contract":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["checks"]["admin_role_guard_required"] = False
        gate_json["checks"]["rqa_list_response_contract_tested"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_openwebui_admin_action_contract_check_stdout="${WORK_DIR}/tampered-openwebui-admin-action-contract-check.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_CHECK_REPORT}" >"${tampered_openwebui_admin_action_contract_check_stdout}"; then
  echo "production-ready reports must reject failed Open WebUI admin Action contract checks" >&2
  exit 1
fi
assert_report "${tampered_openwebui_admin_action_contract_check_stdout}" \
  '"openwebui_admin_action_contract_check_failed=admin_role_guard_required" in report["errors"]'
assert_report "${tampered_openwebui_admin_action_contract_check_stdout}" \
  '"openwebui_admin_action_contract_check_failed=rqa_list_response_contract_tested" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_ACTION_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "openwebui_admin_action_contract":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["action"]["required_actions"].remove("knowledge_patch_proposal")
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_openwebui_admin_action_contract_action_stdout="${WORK_DIR}/tampered-openwebui-admin-action-contract-action.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_OPENWEBUI_ADMIN_ACTION_CONTRACT_ACTION_REPORT}" >"${tampered_openwebui_admin_action_contract_action_stdout}"; then
  echo "production-ready reports must reject missing Open WebUI admin Action coverage" >&2
  exit 1
fi
assert_report "${tampered_openwebui_admin_action_contract_action_stdout}" \
  '"openwebui_admin_action_contract_required_actions_mismatch" in report["errors"]'

python3 - "${SYNTHETIC_READY_REPORT}" "${TAMPERED_OPENWEBUI_ADMIN_ACTION_LIVE_REPORT}" <<'PY'
import json
import sys

source, target = sys.argv[1:3]
with open(source, encoding="utf-8") as handle:
    report = json.load(handle)
for gate in report["gates"]:
    if gate.get("name") == "openwebui_admin_action":
        gate_json = json.loads(gate["stdout_tail"][0])
        gate_json["checks"]["admin_role_guard_required"] = False
        gate_json["permission_boundary"]["admin_role_guard_required"] = False
        gate["stdout_tail"] = [json.dumps(gate_json, sort_keys=True)]
with open(target, "w", encoding="utf-8") as handle:
    json.dump(report, handle)
PY
tampered_openwebui_admin_action_live_stdout="${WORK_DIR}/tampered-openwebui-admin-action-live.stdout"
if "${SCRIPT_DIR}/verify-tonglingyu-release-readiness-report.sh" \
  "${TAMPERED_OPENWEBUI_ADMIN_ACTION_LIVE_REPORT}" >"${tampered_openwebui_admin_action_live_stdout}"; then
  echo "production-ready reports must reject weak live Open WebUI admin Action permission boundary" >&2
  exit 1
fi
assert_report "${tampered_openwebui_admin_action_live_stdout}" \
  '"openwebui_admin_action_check_failed=admin_role_guard_required" in report["errors"]'
assert_report "${tampered_openwebui_admin_action_live_stdout}" \
  '"openwebui_admin_action_admin_role_guard_missing" in report["errors"]'

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
  TONGLINGYU_RELEASE_ENVIRONMENT=contract-live \
  TONGLINGYU_RELEASE_TARGET=contract-target \
  "TONGLINGYU_RELEASE_RUNTIME_CONFIG_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_MIGRATION_PREFLIGHT_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_QUALITY_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_RESTORE_DRILL_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_PERFORMANCE_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_API_CONTRACT_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_USER_LIFECYCLE_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_SECURITY_SCAN_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_OPS_READINESS_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_RQA_INCIDENT_CAPACITY_CMD=${PASS_CMD}" \
  "TONGLINGYU_RELEASE_OPENWEBUI_ADMIN_ACTION_CONTRACT_CMD=${PASS_CMD}" \
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
