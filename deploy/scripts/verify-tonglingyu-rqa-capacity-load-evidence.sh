#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

REPORT_PATH="${TONGLINGYU_RQA_CAPACITY_LOAD_REPORT_PATH:-}"
OPERATOR="${TONGLINGYU_RQA_CAPACITY_LOAD_OPERATOR:-${TONGLINGYU_RELEASE_OPERATOR:-}}"
ENVIRONMENT="${TONGLINGYU_RQA_CAPACITY_LOAD_ENVIRONMENT:-${TONGLINGYU_RELEASE_ENVIRONMENT:-}}"
STARTED_AT="${TONGLINGYU_RQA_CAPACITY_LOAD_STARTED_AT:-}"
FINISHED_AT="${TONGLINGYU_RQA_CAPACITY_LOAD_FINISHED_AT:-}"
CAPACITY_EVIDENCE_REF="${TONGLINGYU_RQA_CAPACITY_EVIDENCE_REF:-}"
LOAD_EVIDENCE_REF="${TONGLINGYU_RQA_LOAD_EVIDENCE_REF:-}"
AUDIT_HISTORY_EVIDENCE_REF="${TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF:-}"
INCIDENT_EVIDENCE_REF="${TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF:-}"
CAPACITY_EVAL_REPORT_COUNT="${TONGLINGYU_RQA_CAPACITY_EVAL_REPORT_COUNT:-0}"
CAPACITY_FAILURE_COUNT="${TONGLINGYU_RQA_CAPACITY_FAILURE_COUNT:-0}"
CAPACITY_ADMIN_LIST_PAGE_COUNT="${TONGLINGYU_RQA_CAPACITY_ADMIN_LIST_PAGE_COUNT:-0}"
LOAD_RQA_WRITE_P95_MS="${TONGLINGYU_RQA_LOAD_RQA_WRITE_P95_MS:-0}"
LOAD_ADMIN_READ_P95_MS="${TONGLINGYU_RQA_LOAD_ADMIN_READ_P95_MS:-0}"
LOAD_METRICS_READ_P95_MS="${TONGLINGYU_RQA_LOAD_METRICS_READ_P95_MS:-0}"
LOAD_RELEASE_GATE_MS="${TONGLINGYU_RQA_LOAD_RELEASE_GATE_MS:-0}"
MIN_WINDOW_MINUTES="${TONGLINGYU_RQA_CAPACITY_LOAD_MIN_WINDOW_MINUTES:-10}"
BUDGET_RQA_WRITE_MS="${TONGLINGYU_RQA_CAPACITY_LOAD_BUDGET_RQA_WRITE_MS:-10000}"
BUDGET_ADMIN_READ_MS="${TONGLINGYU_RQA_CAPACITY_LOAD_BUDGET_ADMIN_READ_MS:-2000}"
BUDGET_METRICS_READ_MS="${TONGLINGYU_RQA_CAPACITY_LOAD_BUDGET_METRICS_READ_MS:-2000}"
BUDGET_RELEASE_GATE_MS="${TONGLINGYU_RQA_CAPACITY_LOAD_BUDGET_RELEASE_GATE_MS:-90000}"

python3 - "${REPO_DIR}" "${REPORT_PATH}" "${OPERATOR}" "${ENVIRONMENT}" \
  "${STARTED_AT}" "${FINISHED_AT}" "${CAPACITY_EVIDENCE_REF}" \
  "${LOAD_EVIDENCE_REF}" "${AUDIT_HISTORY_EVIDENCE_REF}" \
  "${INCIDENT_EVIDENCE_REF}" "${CAPACITY_EVAL_REPORT_COUNT}" \
  "${CAPACITY_FAILURE_COUNT}" "${CAPACITY_ADMIN_LIST_PAGE_COUNT}" \
  "${LOAD_RQA_WRITE_P95_MS}" "${LOAD_ADMIN_READ_P95_MS}" \
  "${LOAD_METRICS_READ_P95_MS}" "${LOAD_RELEASE_GATE_MS}" \
  "${MIN_WINDOW_MINUTES}" "${BUDGET_RQA_WRITE_MS}" "${BUDGET_ADMIN_READ_MS}" \
  "${BUDGET_METRICS_READ_MS}" "${BUDGET_RELEASE_GATE_MS}" <<'PY'
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
    capacity_evidence_ref,
    load_evidence_ref,
    audit_history_evidence_ref,
    incident_evidence_ref,
    capacity_eval_report_count_raw,
    capacity_failure_count_raw,
    capacity_admin_list_page_count_raw,
    load_rqa_write_p95_ms_raw,
    load_admin_read_p95_ms_raw,
    load_metrics_read_p95_ms_raw,
    load_release_gate_ms_raw,
    min_window_minutes_raw,
    budget_rqa_write_ms_raw,
    budget_admin_read_ms_raw,
    budget_metrics_read_ms_raw,
    budget_release_gate_ms_raw,
) = sys.argv[1:23]

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


def require_ref(name, value):
    if not ref_valid(value):
        errors.append(f"{name}_missing_or_invalid")


started_at = parse_timestamp(started_at_raw)
finished_at = parse_timestamp(finished_at_raw)
if started_at is None:
    errors.append("started_at_missing_or_invalid")
if finished_at is None:
    errors.append("finished_at_missing_or_invalid")
window_minutes = 0
if started_at is not None and finished_at is not None:
    if finished_at <= started_at:
        errors.append("finished_at_not_after_started_at")
    window_minutes = int((finished_at - started_at).total_seconds() // 60)

min_window_minutes = non_negative_int(min_window_minutes_raw)
if min_window_minutes is None or min_window_minutes <= 0:
    min_window_minutes = 10
    errors.append("min_window_minutes_invalid")
if window_minutes < min_window_minutes:
    errors.append("capacity_load_window_too_short")

counts = {
    "eval_report_count": non_negative_int(capacity_eval_report_count_raw),
    "failure_count": non_negative_int(capacity_failure_count_raw),
    "admin_list_page_count": non_negative_int(capacity_admin_list_page_count_raw),
}
minimums = {
    "eval_report_count": 1,
    "failure_count": 1,
    "admin_list_page_count": 2,
}
for field, minimum in minimums.items():
    if counts[field] is None or counts[field] < minimum:
        errors.append(f"{field}_below_minimum")

measurements = {
    "rqa_write_p95_ms": positive_number(load_rqa_write_p95_ms_raw),
    "admin_read_p95_ms": positive_number(load_admin_read_p95_ms_raw),
    "metrics_read_p95_ms": positive_number(load_metrics_read_p95_ms_raw),
    "release_gate_ms": positive_number(load_release_gate_ms_raw),
}
budgets = {
    "rqa_write_p95_ms": positive_number(budget_rqa_write_ms_raw),
    "admin_read_p95_ms": positive_number(budget_admin_read_ms_raw),
    "metrics_read_p95_ms": positive_number(budget_metrics_read_ms_raw),
    "release_gate_ms": positive_number(budget_release_gate_ms_raw),
}
budget_results = {}
for field, actual in measurements.items():
    budget = budgets[field]
    if actual is None:
        errors.append(f"{field}_invalid")
        budget_results[field] = False
    elif budget is None:
        errors.append(f"{field}_budget_invalid")
        budget_results[field] = False
    else:
        budget_results[field] = actual <= budget
        if actual > budget:
            errors.append(f"{field}_budget_exceeded")

for name, value in (
    ("capacity_evidence_ref", capacity_evidence_ref),
    ("load_evidence_ref", load_evidence_ref),
    ("audit_history_evidence_ref", audit_history_evidence_ref),
    ("incident_evidence_ref", incident_evidence_ref),
):
    require_ref(name, value)
if not operator.strip():
    errors.append("operator_missing")
if not environment.strip():
    errors.append("environment_missing")

payload = {
    "object": "tonglingyu.rqa_capacity_load_evidence",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "capacity_load_policy_version": "tonglingyu-rqa-capacity-load-evidence-v1",
    "generated_at": now_iso(),
    "operator": operator.strip(),
    "environment": environment.strip(),
    "started_at": started_at.isoformat() if started_at else "",
    "finished_at": finished_at.isoformat() if finished_at else "",
    "window_minutes": window_minutes,
    "min_window_minutes": min_window_minutes,
    "representative_counts": {key: value or 0 for key, value in counts.items()},
    "load_budgets_ms": {key: value or 0 for key, value in budgets.items()},
    "load_measurements": {key: value or 0 for key, value in measurements.items()},
    "budget_results": budget_results,
    "evidence_refs": {
        "capacity_evidence_ref": checked_ref(capacity_evidence_ref),
        "load_evidence_ref": checked_ref(load_evidence_ref),
        "audit_history_evidence_ref": checked_ref(audit_history_evidence_ref),
        "incident_evidence_ref": checked_ref(incident_evidence_ref),
    },
    "checks": {
        "representative_capacity_covered": all(
            counts[field] is not None and counts[field] >= minimum
            for field, minimum in minimums.items()
        ),
        "rqa_write_budget_passed": budget_results.get("rqa_write_p95_ms") is True,
        "admin_read_budget_passed": budget_results.get("admin_read_p95_ms") is True,
        "metrics_read_budget_passed": budget_results.get("metrics_read_p95_ms") is True,
        "release_gate_budget_passed": budget_results.get("release_gate_ms") is True,
        "operator_environment_recorded": bool(operator.strip() and environment.strip()),
        "window_at_least_minimum": window_minutes >= min_window_minutes,
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
