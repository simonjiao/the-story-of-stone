#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

RUNBOOK_PATH="${TONGLINGYU_RELEASE_OPS_RUNBOOK_PATH:-${DEPLOY_DIR}/runbooks/tonglingyu-rqa-release-runbook.md}"
REPORT_PATH="${TONGLINGYU_RELEASE_OPS_REPORT_PATH:-}"
REQUIRE_LIVE="${TONGLINGYU_RELEASE_OPS_REQUIRE_LIVE:-${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}}"
OPERATOR="${TONGLINGYU_RELEASE_OPS_OPERATOR:-${TONGLINGYU_RELEASE_OPERATOR:-}}"
ENVIRONMENT="${TONGLINGYU_RELEASE_OPS_ENVIRONMENT:-${TONGLINGYU_RELEASE_ENVIRONMENT:-}}"
RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:-}"
ROLLBACK_EVIDENCE_REF="${TONGLINGYU_RELEASE_ROLLBACK_EVIDENCE_REF:-}"
RTO_RPO_EVIDENCE_REF="${TONGLINGYU_RELEASE_RTO_RPO_EVIDENCE_REF:-}"
ALERT_EVIDENCE_REF="${TONGLINGYU_RELEASE_ALERT_EVIDENCE_REF:-}"
POST_RELEASE_MONITOR_REF="${TONGLINGYU_RELEASE_POST_RELEASE_MONITOR_REF:-}"
POST_RELEASE_MONITOR_EVIDENCE="${TONGLINGYU_RELEASE_POST_RELEASE_MONITOR_EVIDENCE:-}"
POST_RELEASE_LIVE_GATE_REF="${TONGLINGYU_RELEASE_POST_RELEASE_LIVE_GATE_REF:-}"
POST_RELEASE_ADMIN_ACTION_REF="${TONGLINGYU_RELEASE_POST_RELEASE_ADMIN_ACTION_REF:-}"
POST_RELEASE_CONCLUSION="${TONGLINGYU_RELEASE_POST_RELEASE_CONCLUSION:-}"
POST_RELEASE_WINDOW_MINUTES="${TONGLINGYU_RELEASE_POST_RELEASE_WINDOW_MINUTES:-60}"
RTO_TARGET_SECONDS="${TONGLINGYU_RELEASE_RTO_TARGET_SECONDS:-900}"
RPO_TARGET_SECONDS="${TONGLINGYU_RELEASE_RPO_TARGET_SECONDS:-3600}"

python3 - "${RUNBOOK_PATH}" "${REPORT_PATH}" "${REQUIRE_LIVE}" "${OPERATOR}" \
  "${ENVIRONMENT}" "${RELEASE_REPORT_PATH}" "${ROLLBACK_EVIDENCE_REF}" \
  "${RTO_RPO_EVIDENCE_REF}" "${ALERT_EVIDENCE_REF}" \
  "${POST_RELEASE_MONITOR_REF}" "${POST_RELEASE_MONITOR_EVIDENCE}" \
  "${POST_RELEASE_LIVE_GATE_REF}" "${POST_RELEASE_ADMIN_ACTION_REF}" \
  "${POST_RELEASE_CONCLUSION}" "${POST_RELEASE_WINDOW_MINUTES}" "${RTO_TARGET_SECONDS}" \
  "${RPO_TARGET_SECONDS}" <<'PY'
import hashlib
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

(
    runbook_path_raw,
    report_path_raw,
    require_live_raw,
    operator,
    environment,
    release_report_path,
    rollback_evidence_ref,
    rto_rpo_evidence_ref,
    alert_evidence_ref,
    post_release_monitor_ref,
    post_release_monitor_evidence_raw,
    post_release_live_gate_ref,
    post_release_admin_action_ref,
    post_release_conclusion,
    post_release_window_minutes_raw,
    rto_target_seconds_raw,
    rpo_target_seconds_raw,
) = sys.argv[1:18]

errors = []
runbook_path = Path(runbook_path_raw)
if not runbook_path.is_absolute():
    runbook_path = Path.cwd() / runbook_path


def is_true(value):
    return str(value).strip().lower() in {"1", "true", "yes", "on"}


def now_iso():
    return datetime.now(timezone.utc).isoformat()


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json_file(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def positive_int(value, default):
    try:
        parsed = int(str(value).strip())
    except ValueError:
        errors.append(f"{default}_value_invalid")
        return default
    if parsed <= 0:
        errors.append(f"{default}_value_invalid")
        return default
    return parsed


def ref_kind(value):
    value = str(value or "").strip()
    if not value:
        return ""
    parsed = urlparse(value)
    if parsed.scheme in {"http", "https"} and parsed.netloc:
        return "url"
    if value.startswith("runbook:"):
        return "runbook"
    if value.startswith("artifact:"):
        return "artifact"
    if value.startswith("file:"):
        return "file"
    if value.startswith("/"):
        return "local_file"
    return ""


def ref_valid(value):
    value = str(value or "").strip()
    if not value:
        return False
    lowered = value.lower()
    secret_needles = (
        "api_key",
        "apikey",
        "authorization:",
        "bearer ",
        "password",
        "secret" + "=",
        "sk-",
        "token" + "=",
    )
    if any(needle in lowered for needle in secret_needles):
        return False
    return bool(ref_kind(value))


def checked_ref(value):
    return {
        "ref": str(value or "").strip(),
        "kind": ref_kind(value),
        "valid": ref_valid(value),
    }


def resolve_evidence_path(path_raw):
    value = str(path_raw or "").strip()
    if not value:
        return None
    path = Path(value)
    if path.is_absolute():
        return path
    return Path.cwd() / path


def required_ref(name, value):
    if not ref_valid(value):
        errors.append(f"{name}_missing_or_invalid")


required_sections = [
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
]
required_alerts = [
    "rqa_write_failure_rate",
    "admin_api_5xx_rate",
    "admin_api_latency_p95",
    "open_p0_retrieval_failure",
    "open_p0_governance_task",
    "rqa_quality_gate_failure",
    "release_gate_failure",
    "openwebui_admin_action_failure",
]
allowed_labels = {
    "status",
    "failure_type",
    "task_type",
    "priority",
    "event_type",
    "agent_runtime_mode",
    "rate_limit_per_minute",
    "max_body_bytes",
}
forbidden_labels = {
    "query",
    "question",
    "trace",
    "trace_id",
    "package",
    "package_id",
    "user",
    "user_id",
    "session",
    "session_id",
    "message",
    "message_id",
}
alert_policy = {
    "policy_version": "tonglingyu-rqa-release-alerts-v1",
    "low_cardinality_labels_only": True,
    "conditions": {
        "rqa_write_failure_rate": {
            "severity": "page",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_rqa_write_failures_total",
            "threshold": "> 0 for 5m",
            "labels": ["status", "failure_type"],
        },
        "admin_api_5xx_rate": {
            "severity": "page",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_admin_api_5xx_total",
            "threshold": "> 0 for 5m",
            "labels": ["status"],
        },
        "admin_api_latency_p95": {
            "severity": "ticket",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_admin_api_latency_ms",
            "threshold": "p95 > 2000ms for 10m",
            "labels": ["status"],
        },
        "open_p0_retrieval_failure": {
            "severity": "page",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_retrieval_failures_total",
            "threshold": "open P0 > 0",
            "labels": ["status", "failure_type"],
        },
        "open_p0_governance_task": {
            "severity": "page",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_governance_tasks_total",
            "threshold": "open P0 > 0",
            "labels": ["status", "task_type", "priority"],
        },
        "rqa_quality_gate_failure": {
            "severity": "page",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_rqa_quality_gate_failures_total",
            "threshold": "> 0",
            "labels": ["status"],
        },
        "release_gate_failure": {
            "severity": "page",
            "owner": "release-oncall",
            "metric": "tonglingyu_release_gate_failures_total",
            "threshold": "> 0",
            "labels": ["status"],
        },
        "openwebui_admin_action_failure": {
            "severity": "ticket",
            "owner": "rqa-oncall",
            "metric": "tonglingyu_openwebui_admin_action_failures_total",
            "threshold": "> 0",
            "labels": ["status"],
        },
    },
}

runbook_text = ""
runbook_sha256 = ""
if not runbook_path.is_file():
    errors.append("runbook_not_found")
else:
    runbook_text = runbook_path.read_text(encoding="utf-8")
    runbook_sha256 = file_sha256(runbook_path)

missing_sections = [
    section
    for section in required_sections
    if f"tonglingyu:release-runbook:{section}" not in runbook_text
]
missing_alerts = [
    alert
    for alert in required_alerts
    if f"tonglingyu:alert:{alert}" not in runbook_text
]
if missing_sections:
    errors.append("runbook_sections_missing=" + ",".join(missing_sections))
if missing_alerts:
    errors.append("alert_conditions_missing=" + ",".join(missing_alerts))

bad_alert_labels = {}
for alert_name, alert in alert_policy["conditions"].items():
    labels = alert.get("labels") or []
    bad = [
        label
        for label in labels
        if label not in allowed_labels or label in forbidden_labels
    ]
    if bad:
        bad_alert_labels[alert_name] = bad
if bad_alert_labels:
    errors.append("alert_labels_not_low_cardinality")

require_live = is_true(require_live_raw)
post_release_window_minutes = positive_int(
    post_release_window_minutes_raw,
    "post_release_window_minutes",
)
rto_target_seconds = positive_int(rto_target_seconds_raw, "rto_target_seconds")
rpo_target_seconds = positive_int(rpo_target_seconds_raw, "rpo_target_seconds")

production_refs = {
    "rollback_evidence_ref": checked_ref(rollback_evidence_ref),
    "rto_rpo_evidence_ref": checked_ref(rto_rpo_evidence_ref),
    "alert_evidence_ref": checked_ref(alert_evidence_ref),
    "post_release_monitor_ref": checked_ref(post_release_monitor_ref),
    "post_release_live_gate_ref": checked_ref(post_release_live_gate_ref),
    "post_release_admin_action_ref": checked_ref(post_release_admin_action_ref),
}
post_release_monitor_evidence_path = resolve_evidence_path(
    post_release_monitor_evidence_raw,
)
post_release_monitor_evidence = None
post_release_monitor_evidence_sha256 = ""
post_release_monitor_evidence_valid = False
post_release_monitor_evidence_errors = []
if post_release_monitor_evidence_path is not None:
    if not post_release_monitor_evidence_path.is_file():
        post_release_monitor_evidence_errors.append("post_release_monitor_evidence_not_found")
    else:
        post_release_monitor_evidence_sha256 = file_sha256(
            post_release_monitor_evidence_path,
        )
        post_release_monitor_evidence = load_json_file(post_release_monitor_evidence_path)
        if not isinstance(post_release_monitor_evidence, dict):
            post_release_monitor_evidence_errors.append(
                "post_release_monitor_evidence_json_invalid",
            )
if isinstance(post_release_monitor_evidence, dict):
    if post_release_monitor_evidence.get("object") != "tonglingyu.post_release_monitor":
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_object_invalid",
        )
    if post_release_monitor_evidence.get("schema_version") != 1:
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_schema_version_invalid",
        )
    if post_release_monitor_evidence.get("status") != "ok":
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_status_invalid",
        )
    if post_release_monitor_evidence.get("secret_values_printed") is not False:
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_secret_values_printed",
        )
    if post_release_monitor_evidence.get("operator") != operator.strip():
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_operator_mismatch",
        )
    if post_release_monitor_evidence.get("environment") != environment.strip():
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_environment_mismatch",
        )
    if post_release_monitor_evidence.get("conclusion") != post_release_conclusion.strip():
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_conclusion_mismatch",
        )
    if post_release_monitor_evidence.get("window_minutes") != post_release_window_minutes:
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_window_mismatch",
        )
    release_report = post_release_monitor_evidence.get("release_report")
    if not isinstance(release_report, dict):
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_release_report_missing",
        )
    else:
        if release_report.get("path") != release_report_path.strip():
            post_release_monitor_evidence_errors.append(
                "post_release_monitor_evidence_release_report_path_mismatch",
            )
        if release_report.get("require_live") is not True:
            post_release_monitor_evidence_errors.append(
                "post_release_monitor_evidence_release_report_not_live",
            )
        failed_live_gates = release_report.get("failed_live_gates")
        missing_live_gates = release_report.get("missing_live_gates")
        if failed_live_gates not in ([], None):
            post_release_monitor_evidence_errors.append(
                "post_release_monitor_evidence_failed_live_gates_present",
            )
        if missing_live_gates not in ([], None):
            post_release_monitor_evidence_errors.append(
                "post_release_monitor_evidence_missing_live_gates_present",
            )
    evidence_refs = post_release_monitor_evidence.get("evidence_refs")
    if not isinstance(evidence_refs, dict):
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_refs_missing",
        )
    else:
        expected_refs = {
            "monitor_ref": production_refs["post_release_monitor_ref"],
            "live_gate_evidence_ref": production_refs["post_release_live_gate_ref"],
            "admin_action_or_api_evidence_ref": production_refs[
                "post_release_admin_action_ref"
            ],
        }
        for field, expected_ref in expected_refs.items():
            actual_ref = evidence_refs.get(field)
            if not isinstance(actual_ref, dict):
                post_release_monitor_evidence_errors.append(
                    f"post_release_monitor_evidence_{field}_missing",
                )
                continue
            if actual_ref.get("ref") != expected_ref["ref"]:
                post_release_monitor_evidence_errors.append(
                    f"post_release_monitor_evidence_{field}_mismatch",
                )
            if actual_ref.get("valid") is not True:
                post_release_monitor_evidence_errors.append(
                    f"post_release_monitor_evidence_{field}_invalid",
                )
    checks_from_monitor = post_release_monitor_evidence.get("checks")
    if not isinstance(checks_from_monitor, dict):
        post_release_monitor_evidence_errors.append(
            "post_release_monitor_evidence_checks_missing",
        )
    else:
        for check in (
            "release_report_exists",
            "release_report_requires_live",
            "live_gate_statuses_passed",
            "admin_action_or_api_evidence_ref_valid",
            "monitor_window_at_least_60_minutes",
            "operator_environment_recorded",
            "conclusion_passed",
        ):
            if checks_from_monitor.get(check) is not True:
                post_release_monitor_evidence_errors.append(
                    f"post_release_monitor_evidence_check_failed={check}",
                )
post_release_monitor_evidence_valid = (
    post_release_monitor_evidence_path is not None
    and isinstance(post_release_monitor_evidence, dict)
    and not post_release_monitor_evidence_errors
)

if require_live:
    for field, ref in production_refs.items():
        required_ref(field, ref["ref"])
    if not operator.strip():
        errors.append("operator_missing")
    if not environment.strip():
        errors.append("environment_missing")
    if not release_report_path.strip():
        errors.append("release_report_path_missing")
    if post_release_conclusion.strip() != "passed":
        errors.append("post_release_conclusion_not_passed")
    if post_release_window_minutes < 60:
        errors.append("post_release_window_too_short")
    if post_release_monitor_evidence_path is None:
        errors.append("post_release_monitor_evidence_missing")
    if not post_release_monitor_evidence_valid:
        errors.extend(post_release_monitor_evidence_errors)

production_evidence_complete = (
    bool(operator.strip())
    and bool(environment.strip())
    and bool(release_report_path.strip())
    and post_release_conclusion.strip() == "passed"
    and post_release_window_minutes >= 60
    and all(item["valid"] for item in production_refs.values())
    and post_release_monitor_evidence_valid
)
if not require_live:
    production_evidence_complete = False

checks = {
    "runbook_exists": runbook_path.is_file(),
    "runbook_sections_complete": not missing_sections,
    "rollback_steps_defined": "rollback_image_config" not in missing_sections,
    "db_restore_or_additive_downgrade_defined": (
        "db_restore_or_additive_downgrade" not in missing_sections
    ),
    "post_rollback_release_readiness_required": True,
    "non_production_marker_required": True,
    "alerts_defined": not missing_alerts,
    "alert_labels_low_cardinality": not bad_alert_labels,
    "post_release_monitor_defined": "post_release_monitor" not in missing_sections,
    "release_report_reproduction_defined": (
        "release_report_reproduction" not in missing_sections
    ),
}
release_ops_ready = not errors

payload = {
    "object": "tonglingyu.release_ops_readiness_gate",
    "schema_version": 1,
    "status": "ok" if release_ops_ready else "failed",
    "release_ops_ready": release_ops_ready,
    "ops_policy_version": "tonglingyu-rqa-release-ops-v1",
    "mode": "live" if require_live else "preflight",
    "require_live": require_live,
    "generated_at": now_iso(),
    "runbook": {
        "path": str(runbook_path),
        "sha256": runbook_sha256,
        "required_sections": required_sections,
        "missing_sections": missing_sections,
        "ref": "runbook:tonglingyu-rqa-release-runbook",
    },
    "checks": checks,
    "rollback": {
        "evidence_ref": production_refs["rollback_evidence_ref"],
        "post_rollback_release_readiness_required": True,
        "non_production_marker_required": True,
        "db_restore_or_additive_downgrade_defined": checks[
            "db_restore_or_additive_downgrade_defined"
        ],
    },
    "rto_rpo": {
        "rto_target_seconds": rto_target_seconds,
        "rpo_target_seconds": rpo_target_seconds,
        "evidence_ref": production_refs["rto_rpo_evidence_ref"],
    },
    "alert_policy": {
        **alert_policy,
        "required_conditions": required_alerts,
        "missing_conditions": missing_alerts,
        "evidence_ref": production_refs["alert_evidence_ref"],
    },
    "post_release_monitor": {
        "required": True,
        "window_minutes": post_release_window_minutes,
        "operator": operator.strip(),
        "environment": environment.strip(),
        "release_report_path": release_report_path.strip(),
        "monitor_ref": production_refs["post_release_monitor_ref"],
        "evidence_path": (
            str(post_release_monitor_evidence_path)
            if post_release_monitor_evidence_path is not None
            else ""
        ),
        "evidence_sha256": post_release_monitor_evidence_sha256,
        "evidence_validated": post_release_monitor_evidence_valid,
        "evidence_errors": post_release_monitor_evidence_errors,
        "live_gate_evidence_ref": production_refs["post_release_live_gate_ref"],
        "admin_action_or_api_evidence_ref": production_refs[
            "post_release_admin_action_ref"
        ],
        "requires_live_gate_evidence": True,
        "requires_admin_action_or_api_evidence": True,
        "conclusion": post_release_conclusion.strip() or "pending_live_release",
    },
    "reproduction": {
        "runbook_ref": "runbook:tonglingyu-rqa-release-runbook#release-report-reproduction",
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
    },
    "evidence": {
        "production_evidence_complete": production_evidence_complete,
        "rollback_evidence_ref": production_refs["rollback_evidence_ref"]["ref"],
        "rto_rpo_evidence_ref": production_refs["rto_rpo_evidence_ref"]["ref"],
        "alert_evidence_ref": production_refs["alert_evidence_ref"]["ref"],
        "post_release_monitor_ref": production_refs["post_release_monitor_ref"]["ref"],
        "post_release_monitor_evidence_sha256": post_release_monitor_evidence_sha256,
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
if not release_ops_ready:
    raise SystemExit(1)
PY
