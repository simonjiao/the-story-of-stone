#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

REPORT_PATH="${TONGLINGYU_RQA_INCIDENT_AUDIT_REPORT_PATH:-}"
OPERATOR="${TONGLINGYU_RQA_INCIDENT_AUDIT_OPERATOR:-${TONGLINGYU_RELEASE_OPERATOR:-}}"
ENVIRONMENT="${TONGLINGYU_RQA_INCIDENT_AUDIT_ENVIRONMENT:-${TONGLINGYU_RELEASE_ENVIRONMENT:-}}"
STARTED_AT="${TONGLINGYU_RQA_INCIDENT_AUDIT_STARTED_AT:-}"
FINISHED_AT="${TONGLINGYU_RQA_INCIDENT_AUDIT_FINISHED_AT:-}"
AUDIT_HISTORY_EVIDENCE_REF="${TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF:-}"
INCIDENT_EVIDENCE_REF="${TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF:-}"
STATUS_HISTORY_EVENT_COUNT="${TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_EVENT_COUNT:-0}"
STATUS_HISTORY_ACTOR_COUNT="${TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_ACTOR_COUNT:-0}"
AUDIT_TOMBSTONE_COUNT="${TONGLINGYU_RQA_AUDIT_TOMBSTONE_COUNT:-0}"
INCIDENT_SEVERITY="${TONGLINGYU_RQA_INCIDENT_SEVERITY:-}"
INCIDENT_OWNER="${TONGLINGYU_RQA_INCIDENT_OWNER:-}"
FIRST_RESPONSE_REF="${TONGLINGYU_RQA_INCIDENT_FIRST_RESPONSE_REF:-}"
MITIGATION_REF="${TONGLINGYU_RQA_INCIDENT_MITIGATION_REF:-}"
ROLLBACK_REF="${TONGLINGYU_RQA_INCIDENT_ROLLBACK_REF:-}"
RECOVERY_VALIDATION_REF="${TONGLINGYU_RQA_INCIDENT_RECOVERY_VALIDATION_REF:-}"
RTO_RPO_BREACH_ESCALATION_REF="${TONGLINGYU_RQA_INCIDENT_RTO_RPO_BREACH_ESCALATION_REF:-}"
INCIDENT_CONCLUSION="${TONGLINGYU_RQA_INCIDENT_CONCLUSION:-}"

python3 - "${REPO_DIR}" "${REPORT_PATH}" "${OPERATOR}" "${ENVIRONMENT}" \
  "${STARTED_AT}" "${FINISHED_AT}" "${AUDIT_HISTORY_EVIDENCE_REF}" \
  "${INCIDENT_EVIDENCE_REF}" "${STATUS_HISTORY_EVENT_COUNT}" \
  "${STATUS_HISTORY_ACTOR_COUNT}" "${AUDIT_TOMBSTONE_COUNT}" \
  "${INCIDENT_SEVERITY}" "${INCIDENT_OWNER}" "${FIRST_RESPONSE_REF}" \
  "${MITIGATION_REF}" "${ROLLBACK_REF}" "${RECOVERY_VALIDATION_REF}" \
  "${RTO_RPO_BREACH_ESCALATION_REF}" "${INCIDENT_CONCLUSION}" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

(
    repo_dir_raw,
    report_path_raw,
    operator,
    environment,
    started_at_raw,
    finished_at_raw,
    audit_history_evidence_ref,
    incident_evidence_ref,
    status_history_event_count_raw,
    status_history_actor_count_raw,
    audit_tombstone_count_raw,
    incident_severity,
    incident_owner,
    first_response_ref,
    mitigation_ref,
    rollback_ref,
    recovery_validation_ref,
    rto_rpo_breach_escalation_ref,
    incident_conclusion,
) = sys.argv[1:20]

repo_dir = Path(repo_dir_raw)
errors = []
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
allowed_severities = {"sev0", "sev1", "sev2", "sev3"}


def now_iso():
    return datetime.now(timezone.utc).isoformat()


def parse_timestamp(value):
    if not isinstance(value, str) or not value.strip():
        return None
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.astimezone(timezone.utc)


def non_negative_int(value):
    try:
        parsed = int(str(value).strip())
    except ValueError:
        return None
    if parsed < 0:
        return None
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
    if any(needle in lowered for needle in secret_needles):
        return False
    return bool(ref_kind(value))


def checked_ref(value):
    return {
        "ref": str(value or "").strip(),
        "kind": ref_kind(value),
        "valid": ref_valid(value),
    }


def resolve_report_path(path_raw):
    value = str(path_raw or "").strip()
    if not value:
        return None
    path = Path(value)
    if path.is_absolute():
        return path
    return repo_dir / path


started_at = parse_timestamp(started_at_raw)
finished_at = parse_timestamp(finished_at_raw)
if started_at is None:
    errors.append("started_at_missing_or_invalid")
if finished_at is None:
    errors.append("finished_at_missing_or_invalid")
duration_minutes = 0
if started_at is not None and finished_at is not None:
    if finished_at <= started_at:
        errors.append("finished_at_not_after_started_at")
    duration_minutes = int((finished_at - started_at).total_seconds() // 60)

status_history_event_count = non_negative_int(status_history_event_count_raw)
status_history_actor_count = non_negative_int(status_history_actor_count_raw)
audit_tombstone_count = non_negative_int(audit_tombstone_count_raw)
if status_history_event_count is None or status_history_event_count < 1:
    errors.append("status_history_event_count_below_minimum")
if status_history_actor_count is None or status_history_actor_count < 1:
    errors.append("status_history_actor_count_below_minimum")
if audit_tombstone_count is None:
    errors.append("audit_tombstone_count_invalid")

if not operator.strip():
    errors.append("operator_missing")
if not environment.strip():
    errors.append("environment_missing")
if incident_severity.strip().lower() not in allowed_severities:
    errors.append("incident_severity_invalid")
if not incident_owner.strip():
    errors.append("incident_owner_missing")
if incident_conclusion.strip() != "passed":
    errors.append("incident_conclusion_not_passed")

refs = {
    "audit_history_evidence_ref": checked_ref(audit_history_evidence_ref),
    "incident_evidence_ref": checked_ref(incident_evidence_ref),
    "first_response_ref": checked_ref(first_response_ref),
    "mitigation_ref": checked_ref(mitigation_ref),
    "rollback_ref": checked_ref(rollback_ref),
    "recovery_validation_ref": checked_ref(recovery_validation_ref),
    "rto_rpo_breach_escalation_ref": checked_ref(rto_rpo_breach_escalation_ref),
}
for name, value in refs.items():
    if not value["valid"]:
        errors.append(f"{name}_missing_or_invalid")

payload = {
    "object": "tonglingyu.rqa_incident_audit_evidence",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "incident_audit_policy_version": "tonglingyu-rqa-incident-audit-evidence-v1",
    "generated_at": now_iso(),
    "operator": operator.strip(),
    "environment": environment.strip(),
    "started_at": started_at.isoformat() if started_at else "",
    "finished_at": finished_at.isoformat() if finished_at else "",
    "duration_minutes": duration_minutes,
    "audit_history": {
        "status_history_event_count": status_history_event_count or 0,
        "status_history_actor_count": status_history_actor_count or 0,
        "audit_tombstone_count": audit_tombstone_count or 0,
        "required_fields": [
            "actor",
            "reason_sha256",
            "previous_status",
            "new_status",
            "timestamp",
        ],
        "hard_delete_open_records_forbidden": True,
        "audit_history_evidence_ref": refs["audit_history_evidence_ref"],
    },
    "incident_drill": {
        "severity": incident_severity.strip().lower(),
        "owner": incident_owner.strip(),
        "incident_evidence_ref": refs["incident_evidence_ref"],
        "first_response_ref": refs["first_response_ref"],
        "mitigation_ref": refs["mitigation_ref"],
        "rollback_ref": refs["rollback_ref"],
        "recovery_validation_ref": refs["recovery_validation_ref"],
        "rto_rpo_breach_escalation_ref": refs["rto_rpo_breach_escalation_ref"],
        "conclusion": incident_conclusion.strip() or "pending",
    },
    "checks": {
        "status_history_events_present": (
            status_history_event_count is not None and status_history_event_count >= 1
        ),
        "status_history_actor_present": (
            status_history_actor_count is not None and status_history_actor_count >= 1
        ),
        "incident_response_refs_valid": all(ref["valid"] for ref in refs.values()),
        "recovery_validation_present": refs["recovery_validation_ref"]["valid"],
        "rto_rpo_breach_escalation_present": refs[
            "rto_rpo_breach_escalation_ref"
        ]["valid"],
        "operator_environment_recorded": bool(operator.strip() and environment.strip()),
        "conclusion_passed": incident_conclusion.strip() == "passed",
    },
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path_raw:
    report_path = resolve_report_path(report_path_raw)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
if errors:
    raise SystemExit(1)
PY
