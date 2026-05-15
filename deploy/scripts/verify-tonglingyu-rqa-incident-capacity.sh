#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

REPORT_PATH="${TONGLINGYU_RQA_INCIDENT_CAPACITY_REPORT_PATH:-}"
RUNBOOK_PATH="${TONGLINGYU_RQA_INCIDENT_RUNBOOK_PATH:-${REPO_DIR}/deploy/runbooks/tonglingyu-rqa-release-runbook.md}"
REQUIRE_LIVE="${TONGLINGYU_RQA_INCIDENT_CAPACITY_REQUIRE_LIVE:-${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}}"
EMERGENCY_DISABLED="${TONGLINGYU_RQA_EMERGENCY_DISABLED:-false}"
DEGRADED_MODE="${TONGLINGYU_RQA_DEGRADED_MODE:-false}"
PERSISTENCE_DEGRADED="${TONGLINGYU_RQA_PERSISTENCE_DEGRADED:-false}"
CAPACITY_EVIDENCE_REF="${TONGLINGYU_RQA_CAPACITY_EVIDENCE_REF:-}"
LOAD_EVIDENCE_REF="${TONGLINGYU_RQA_LOAD_EVIDENCE_REF:-}"
AUDIT_HISTORY_EVIDENCE_REF="${TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF:-}"
INCIDENT_EVIDENCE_REF="${TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF:-}"
CAPACITY_EVAL_REPORT_COUNT="${TONGLINGYU_RQA_CAPACITY_EVAL_REPORT_COUNT:-0}"
CAPACITY_FAILURE_COUNT="${TONGLINGYU_RQA_CAPACITY_FAILURE_COUNT:-0}"
CAPACITY_ADMIN_LIST_PAGE_COUNT="${TONGLINGYU_RQA_CAPACITY_ADMIN_LIST_PAGE_COUNT:-0}"
LOAD_RQA_WRITE_P95_MS="${TONGLINGYU_RQA_LOAD_RQA_WRITE_P95_MS:-0}"
LOAD_ADMIN_READ_P95_MS="${TONGLINGYU_RQA_LOAD_ADMIN_READ_P95_MS:-0}"
LOAD_RELEASE_GATE_MS="${TONGLINGYU_RQA_LOAD_RELEASE_GATE_MS:-0}"

python3 - "${REPO_DIR}" "${RUNBOOK_PATH}" "${REPORT_PATH}" "${REQUIRE_LIVE}" \
  "${EMERGENCY_DISABLED}" "${DEGRADED_MODE}" "${PERSISTENCE_DEGRADED}" \
  "${CAPACITY_EVIDENCE_REF}" "${LOAD_EVIDENCE_REF}" \
  "${AUDIT_HISTORY_EVIDENCE_REF}" "${INCIDENT_EVIDENCE_REF}" \
  "${CAPACITY_EVAL_REPORT_COUNT}" "${CAPACITY_FAILURE_COUNT}" \
  "${CAPACITY_ADMIN_LIST_PAGE_COUNT}" "${LOAD_RQA_WRITE_P95_MS}" \
  "${LOAD_ADMIN_READ_P95_MS}" "${LOAD_RELEASE_GATE_MS}" <<'PY'
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

(
    repo_dir_raw,
    runbook_path_raw,
    report_path_raw,
    require_live_raw,
    emergency_disabled_raw,
    degraded_mode_raw,
    persistence_degraded_raw,
    capacity_evidence_ref,
    load_evidence_ref,
    audit_history_evidence_ref,
    incident_evidence_ref,
    capacity_eval_report_count_raw,
    capacity_failure_count_raw,
    capacity_admin_list_page_count_raw,
    load_rqa_write_p95_ms_raw,
    load_admin_read_p95_ms_raw,
    load_release_gate_ms_raw,
) = sys.argv[1:18]

repo_dir = Path(repo_dir_raw)
runbook_path = Path(runbook_path_raw)
if not runbook_path.is_absolute():
    runbook_path = repo_dir / runbook_path
errors = []


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


def positive_number(value):
    try:
        parsed = float(str(value).strip())
    except ValueError:
        return None
    if parsed <= 0:
        return None
    return parsed


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


def read_text(relative):
    return (repo_dir / relative).read_text(encoding="utf-8", errors="replace")


gateway_source = read_text("agent-platform/crates/tonglingyu-gateway/src/main.rs")
runtime_source = read_text("agent-platform/crates/tonglingyu-runtime/src/lib.rs")
combined_source = gateway_source + "\n" + runtime_source
unbounded_queue_patterns = [
    r"\bunbounded_channel\b",
    r"\bmpsc::unbounded\b",
    r"\bVecDeque\b",
]
unbounded_queue_hits = [
    pattern
    for pattern in unbounded_queue_patterns
    if re.search(pattern, combined_source)
]
hard_delete_patterns = [
    r"DELETE\s+FROM\s+retrieval_failures",
    r"DELETE\s+FROM\s+knowledge_governance_tasks",
]
hard_delete_hits = [
    pattern
    for pattern in hard_delete_patterns
    if re.search(pattern, combined_source, re.IGNORECASE)
]
retry_idempotency_defined = (
    "load_retrieval_failure_by_dedupe" in runtime_source
    and "retrieval_failure_update_repeated_payload_is_idempotent" in gateway_source
)
status_history_audit_defined = (
    '"previous_status"' in combined_source
    and '"new_status"' in combined_source
    and '"status_history"' in combined_source
    and '"reason_sha256"' in combined_source
)

runbook_text = ""
runbook_sha256 = ""
if not runbook_path.is_file():
    errors.append("incident_runbook_not_found")
else:
    runbook_text = runbook_path.read_text(encoding="utf-8")
    runbook_sha256 = file_sha256(runbook_path)
incident_runbook_defined = "tonglingyu:release-runbook:incident_response" in runbook_text
if not incident_runbook_defined:
    errors.append("incident_runbook_section_missing")

emergency_disabled = is_true(emergency_disabled_raw)
degraded_mode = is_true(degraded_mode_raw)
persistence_degraded = is_true(persistence_degraded_raw)
if emergency_disabled:
    errors.append("rqa_emergency_disabled_requires_non_production")
if degraded_mode:
    errors.append("rqa_degraded_mode_requires_non_production")
if persistence_degraded:
    errors.append("rqa_persistence_degraded_requires_non_production")
if unbounded_queue_hits:
    errors.append("unbounded_queue_pattern_present")
if hard_delete_hits:
    errors.append("hard_delete_open_record_pattern_present")
if not retry_idempotency_defined:
    errors.append("retry_idempotency_not_proven")
if not status_history_audit_defined:
    errors.append("status_history_audit_not_proven")

require_live = is_true(require_live_raw)
capacity_eval_report_count = non_negative_int(capacity_eval_report_count_raw)
capacity_failure_count = non_negative_int(capacity_failure_count_raw)
capacity_admin_list_page_count = non_negative_int(capacity_admin_list_page_count_raw)
load_rqa_write_p95_ms = positive_number(load_rqa_write_p95_ms_raw)
load_admin_read_p95_ms = positive_number(load_admin_read_p95_ms_raw)
load_release_gate_ms = positive_number(load_release_gate_ms_raw)
capacity_ref = checked_ref(capacity_evidence_ref)
load_ref = checked_ref(load_evidence_ref)
audit_ref = checked_ref(audit_history_evidence_ref)
incident_ref = checked_ref(incident_evidence_ref)

if require_live:
    for name, ref in (
        ("capacity_evidence_ref", capacity_ref),
        ("load_evidence_ref", load_ref),
        ("audit_history_evidence_ref", audit_ref),
        ("incident_evidence_ref", incident_ref),
    ):
        if not ref["valid"]:
            errors.append(f"{name}_missing_or_invalid")
    if capacity_eval_report_count is None or capacity_eval_report_count < 1:
        errors.append("capacity_eval_report_count_invalid")
    if capacity_failure_count is None or capacity_failure_count < 1:
        errors.append("capacity_failure_count_invalid")
    if capacity_admin_list_page_count is None or capacity_admin_list_page_count < 2:
        errors.append("capacity_admin_list_page_count_invalid")
    if load_rqa_write_p95_ms is None:
        errors.append("load_rqa_write_p95_ms_invalid")
    if load_admin_read_p95_ms is None:
        errors.append("load_admin_read_p95_ms_invalid")
    if load_release_gate_ms is None:
        errors.append("load_release_gate_ms_invalid")

capacity_evidence_complete = (
    require_live
    and capacity_ref["valid"]
    and load_ref["valid"]
    and audit_ref["valid"]
    and incident_ref["valid"]
    and capacity_eval_report_count is not None
    and capacity_eval_report_count >= 1
    and capacity_failure_count is not None
    and capacity_failure_count >= 1
    and capacity_admin_list_page_count is not None
    and capacity_admin_list_page_count >= 2
    and load_rqa_write_p95_ms is not None
    and load_admin_read_p95_ms is not None
    and load_release_gate_ms is not None
    and not emergency_disabled
    and not degraded_mode
    and not persistence_degraded
)

checks = {
    "emergency_flags_fail_closed": not emergency_disabled
    and not degraded_mode
    and not persistence_degraded,
    "public_degraded_response_defined": True,
    "no_unbounded_queue": not unbounded_queue_hits,
    "retry_idempotency_defined": retry_idempotency_defined,
    "status_history_audit_defined": status_history_audit_defined,
    "hard_delete_open_records_forbidden": not hard_delete_hits,
    "incident_runbook_defined": incident_runbook_defined,
    "capacity_live_evidence_required": True,
    "load_live_evidence_required": True,
    "audit_history_live_evidence_required": True,
}
incident_capacity_ready = not errors

payload = {
    "object": "tonglingyu.rqa_incident_capacity_gate",
    "schema_version": 1,
    "status": "ok" if incident_capacity_ready else "failed",
    "incident_capacity_ready": incident_capacity_ready,
    "policy_version": "tonglingyu-rqa-incident-capacity-v1",
    "mode": "live" if require_live else "preflight",
    "require_live": require_live,
    "generated_at": now_iso(),
    "emergency_state": {
        "emergency_disabled": emergency_disabled,
        "degraded_mode": degraded_mode,
        "persistence_degraded": persistence_degraded,
        "production_allowed": not (
            emergency_disabled or degraded_mode or persistence_degraded
        ),
        "non_production_required": (
            emergency_disabled or degraded_mode or persistence_degraded
        ),
    },
    "public_degraded_response": {
        "stable_status_required": True,
        "trace_id_required": True,
        "full_success_forbidden": True,
    },
    "capacity_policy": {
        "write_queue_policy": "synchronous_write_no_unbounded_queue",
        "max_in_memory_queue_items": 0,
        "retry_idempotency_required": True,
        "retry_duplicate_record_forbidden": True,
        "capacity_evidence_ref": capacity_ref,
        "load_evidence_ref": load_ref,
        "representative_counts": {
            "eval_report_count": capacity_eval_report_count or 0,
            "failure_count": capacity_failure_count or 0,
            "admin_list_page_count": capacity_admin_list_page_count or 0,
        },
        "load_measurements": {
            "rqa_write_p95_ms": load_rqa_write_p95_ms or 0,
            "admin_read_p95_ms": load_admin_read_p95_ms or 0,
            "release_gate_ms": load_release_gate_ms or 0,
        },
    },
    "audit_history": {
        "status_history_required": True,
        "required_fields": [
            "actor",
            "reason_sha256",
            "previous_status",
            "new_status",
            "timestamp",
        ],
        "hard_delete_open_records_forbidden": True,
        "audit_history_evidence_ref": audit_ref,
    },
    "incident_runbook": {
        "path": str(runbook_path),
        "sha256": runbook_sha256,
        "ref": "runbook:tonglingyu-rqa-release-runbook#incident-response",
        "severity_owner_first_response_defined": incident_runbook_defined,
        "rto_rpo_breach_escalation_defined": incident_runbook_defined,
        "incident_evidence_ref": incident_ref,
    },
    "checks": checks,
    "evidence": {
        "capacity_evidence_complete": capacity_evidence_complete,
        "capacity_evidence_ref": capacity_ref["ref"],
        "load_evidence_ref": load_ref["ref"],
        "audit_history_evidence_ref": audit_ref["ref"],
        "incident_evidence_ref": incident_ref["ref"],
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
if not incident_capacity_ready:
    raise SystemExit(1)
PY
