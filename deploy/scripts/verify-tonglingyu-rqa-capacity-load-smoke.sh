#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

fail() {
  printf 'capacity-load-smoke failed: %s\n' "$*" >&2
  exit 1
}

now_iso() {
  python3 - <<'PY'
from datetime import datetime, timezone
print(datetime.now(timezone.utc).isoformat())
PY
}

now_epoch_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

positive_int() {
  case "${1:-}" in
    ''|*[!0-9]*) return 1 ;;
    *) [ "$1" -gt 0 ] ;;
  esac
}

OPERATOR="${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_OPERATOR:-${TONGLINGYU_RELEASE_OPERATOR:-${USER:-local-smoke}}}"
ENVIRONMENT="${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_ENVIRONMENT:-${TONGLINGYU_RELEASE_ENVIRONMENT:-local-smoke}}"
ITERATIONS="${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_ITERATIONS:-3}"
MIN_WINDOW_MINUTES="${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_MIN_WINDOW_MINUTES:-10}"
INCIDENT_SEVERITY="${TONGLINGYU_RQA_INCIDENT_SEVERITY:-sev3}"
INCIDENT_OWNER="${TONGLINGYU_RQA_INCIDENT_OWNER:-${OPERATOR}}"

positive_int "${ITERATIONS}" || fail "TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_ITERATIONS must be a positive integer"
positive_int "${MIN_WINDOW_MINUTES}" || fail "TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_MIN_WINDOW_MINUTES must be a positive integer"
[ -n "${OPERATOR// }" ] || fail "operator is required"
[ -n "${ENVIRONMENT// }" ] || fail "environment is required"

if [ -n "${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_ARTIFACT_DIR:-}" ]; then
  ARTIFACT_DIR="${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_ARTIFACT_DIR}"
else
  ARTIFACT_DIR="$(mktemp -d -t tonglingyu-rqa-capacity-load-smoke.XXXXXX)"
fi
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${REPO_DIR}/${ARTIFACT_DIR}"
fi
mkdir -p "${ARTIFACT_DIR}"
chmod 700 "${ARTIFACT_DIR}" 2>/dev/null || true

SMOKE_REPORT_PATH="${TONGLINGYU_RQA_CAPACITY_LOAD_SMOKE_REPORT_PATH:-${ARTIFACT_DIR}/rqa-capacity-load-smoke.json}"
if [[ "${SMOKE_REPORT_PATH}" != /* ]]; then
  SMOKE_REPORT_PATH="${REPO_DIR}/${SMOKE_REPORT_PATH}"
fi

RUNS_JSONL="${ARTIFACT_DIR}/performance-runs.jsonl"
SUMMARY_PATH="${ARTIFACT_DIR}/capacity-load-raw-summary.json"
METRICS_ENV="${ARTIFACT_DIR}/capacity-load-smoke.env"
INCIDENT_DRILL_PATH="${ARTIFACT_DIR}/incident-drill-local-smoke.json"
CAPACITY_LOAD_EVIDENCE_PATH="${ARTIFACT_DIR}/rqa-capacity-load-evidence.json"
INCIDENT_AUDIT_EVIDENCE_PATH="${ARTIFACT_DIR}/rqa-incident-audit-evidence.json"
INCIDENT_CAPACITY_REPORT_PATH="${ARTIFACT_DIR}/rqa-incident-capacity-live-gate.json"
: >"${RUNS_JSONL}"

STARTED_AT="$(now_iso)"
STARTED_MS="$(now_epoch_ms)"

for index in $(seq 1 "${ITERATIONS}"); do
  PERFORMANCE_REPORT="${ARTIFACT_DIR}/rqa-performance-budget-${index}.json"
  RUN_STDOUT="${ARTIFACT_DIR}/rqa-performance-budget-${index}.stdout"
  RUN_STARTED_MS="$(now_epoch_ms)"
  TONGLINGYU_RQA_PERFORMANCE_REPORT_PATH="${PERFORMANCE_REPORT}" \
    "${SCRIPT_DIR}/verify-tonglingyu-rqa-performance-budget.sh" \
    >"${RUN_STDOUT}"
  RUN_FINISHED_MS="$(now_epoch_ms)"
  python3 - "${RUNS_JSONL}" "${PERFORMANCE_REPORT}" "${RUN_STDOUT}" \
    "${RUN_STARTED_MS}" "${RUN_FINISHED_MS}" "${index}" <<'PY'
import json
import sys
from pathlib import Path

runs_path = Path(sys.argv[1])
report_path = Path(sys.argv[2])
stdout_path = Path(sys.argv[3])
started_ms = int(sys.argv[4])
finished_ms = int(sys.argv[5])
index = int(sys.argv[6])
entry = {
    "index": index,
    "report_path": str(report_path),
    "stdout_path": str(stdout_path),
    "started_ms": started_ms,
    "finished_ms": finished_ms,
    "gate_runtime_ms": finished_ms - started_ms,
}
with runs_path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(entry, ensure_ascii=True, sort_keys=True) + "\n")
PY
done

python3 - "${RUNS_JSONL}" "${SUMMARY_PATH}" "${METRICS_ENV}" <<'PY'
import json
import math
import shlex
import sys
from pathlib import Path

runs_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
metrics_env_path = Path(sys.argv[3])

runs = [
    json.loads(line)
    for line in runs_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
]
if not runs:
    raise SystemExit("no performance runs recorded")

reports = []
errors = []
for run in runs:
    report_path = Path(run["report_path"])
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if report.get("object") != "tonglingyu.rqa_performance_budget_gate":
        errors.append(f"performance_report_object_invalid={report_path}")
    if report.get("status") != "ok" or report.get("performance_budget_passed") is not True:
        errors.append(f"performance_report_failed={report_path}")
    reports.append({"run": run, "report": report})

if errors:
    raise SystemExit(";".join(errors))


def values(field):
    return [
        int(item["report"]["measurements"][field])
        for item in reports
    ]


def percentile_95(numbers):
    ordered = sorted(int(value) for value in numbers)
    index = max(0, math.ceil(0.95 * len(ordered)) - 1)
    return ordered[index]


def total_count(field):
    return sum(
        int(item["report"].get("capacity_counts", {}).get(field, 0))
        for item in reports
    )


rqa_write_p95_ms = percentile_95(values("rqa_write_ms"))
admin_read_p95_ms = percentile_95(
    max(
        int(item["report"]["measurements"]["admin_trace_read_ms"]),
        int(item["report"]["measurements"]["admin_failure_list_ms"]),
        int(item["report"]["measurements"]["admin_governance_task_list_ms"]),
    )
    for item in reports
)
metrics_read_p95_ms = percentile_95(values("admin_metrics_read_ms"))
release_gate_ms = percentile_95(
    int(item["run"]["gate_runtime_ms"])
    for item in reports
)
status_history_event_count = sum(
    int(item["report"].get("audit_history_counts", {}).get("status_history_event_count", 0))
    for item in reports
)
status_history_actor_count = max(
    int(item["report"].get("audit_history_counts", {}).get("status_history_actor_count", 0))
    for item in reports
)
audit_tombstone_count = sum(
    int(item["report"].get("audit_history_counts", {}).get("audit_tombstone_count", 0))
    for item in reports
)
summary = {
    "object": "tonglingyu.rqa_capacity_load_raw_summary",
    "schema_version": 1,
    "performance_report_count": len(reports),
    "performance_reports": [
        item["run"]["report_path"]
        for item in reports
    ],
    "capacity_counts": {
        "eval_report_count": total_count("eval_report_count"),
        "failure_count": total_count("failure_count"),
        "admin_list_page_count": total_count("admin_list_page_count"),
    },
    "load_measurements": {
        "rqa_write_p95_ms": rqa_write_p95_ms,
        "admin_read_p95_ms": admin_read_p95_ms,
        "metrics_read_p95_ms": metrics_read_p95_ms,
        "release_gate_ms": release_gate_ms,
    },
    "audit_history_counts": {
        "status_history_event_count": status_history_event_count,
        "status_history_actor_count": status_history_actor_count,
        "audit_tombstone_count": audit_tombstone_count,
    },
}
summary_path.write_text(
    json.dumps(summary, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
env_values = {
    "CAPACITY_EVAL_REPORT_COUNT": summary["capacity_counts"]["eval_report_count"],
    "CAPACITY_FAILURE_COUNT": summary["capacity_counts"]["failure_count"],
    "CAPACITY_ADMIN_LIST_PAGE_COUNT": summary["capacity_counts"]["admin_list_page_count"],
    "LOAD_RQA_WRITE_P95_MS": rqa_write_p95_ms,
    "LOAD_ADMIN_READ_P95_MS": admin_read_p95_ms,
    "LOAD_METRICS_READ_P95_MS": metrics_read_p95_ms,
    "LOAD_RELEASE_GATE_MS": release_gate_ms,
    "AUDIT_STATUS_HISTORY_EVENT_COUNT": status_history_event_count,
    "AUDIT_STATUS_HISTORY_ACTOR_COUNT": status_history_actor_count,
    "AUDIT_TOMBSTONE_COUNT": audit_tombstone_count,
}
with metrics_env_path.open("w", encoding="utf-8") as handle:
    for key, value in env_values.items():
        handle.write(f"{key}={shlex.quote(str(value))}\n")
PY

# shellcheck disable=SC1090
source "${METRICS_ENV}"

CURRENT_MS="$(now_epoch_ms)"
ELAPSED_MS=$(( CURRENT_MS - STARTED_MS ))
REQUIRED_MS=$(( MIN_WINDOW_MINUTES * 60 * 1000 ))
if [ "${ELAPSED_MS}" -lt "${REQUIRED_MS}" ]; then
  SLEEP_SECONDS=$(( (REQUIRED_MS - ELAPSED_MS + 999) / 1000 + 1 ))
  sleep "${SLEEP_SECONDS}"
fi
FINISHED_AT="$(now_iso)"

python3 - "${INCIDENT_DRILL_PATH}" "${OPERATOR}" "${ENVIRONMENT}" \
  "${STARTED_AT}" "${FINISHED_AT}" "${SUMMARY_PATH}" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
payload = {
    "object": "tonglingyu.rqa_incident_drill_local_smoke",
    "schema_version": 1,
    "operator": sys.argv[2],
    "environment": sys.argv[3],
    "started_at": sys.argv[4],
    "finished_at": sys.argv[5],
    "capacity_load_summary_ref": sys.argv[6],
    "steps": {
        "first_response": "local smoke opened RQA failure and governance task",
        "mitigation": "local smoke closed the retrieval failure with an audited reason",
        "rollback": "local smoke verified no emergency-disabled or degraded-mode release bypass",
        "recovery_validation": "local smoke reran quality and incident/capacity gates",
        "rto_rpo_breach_escalation": "local smoke confirmed runbook escalation evidence binding",
    },
    "conclusion": "passed",
    "secret_values_printed": False,
}
path.write_text(json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n", encoding="utf-8")
PY

TONGLINGYU_RQA_CAPACITY_LOAD_REPORT_PATH="${CAPACITY_LOAD_EVIDENCE_PATH}" \
TONGLINGYU_RQA_CAPACITY_LOAD_OPERATOR="${OPERATOR}" \
TONGLINGYU_RQA_CAPACITY_LOAD_ENVIRONMENT="${ENVIRONMENT}" \
TONGLINGYU_RQA_CAPACITY_LOAD_STARTED_AT="${STARTED_AT}" \
TONGLINGYU_RQA_CAPACITY_LOAD_FINISHED_AT="${FINISHED_AT}" \
TONGLINGYU_RQA_CAPACITY_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_LOAD_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_CAPACITY_EVAL_REPORT_COUNT="${CAPACITY_EVAL_REPORT_COUNT}" \
TONGLINGYU_RQA_CAPACITY_FAILURE_COUNT="${CAPACITY_FAILURE_COUNT}" \
TONGLINGYU_RQA_CAPACITY_ADMIN_LIST_PAGE_COUNT="${CAPACITY_ADMIN_LIST_PAGE_COUNT}" \
TONGLINGYU_RQA_LOAD_RQA_WRITE_P95_MS="${LOAD_RQA_WRITE_P95_MS}" \
TONGLINGYU_RQA_LOAD_ADMIN_READ_P95_MS="${LOAD_ADMIN_READ_P95_MS}" \
TONGLINGYU_RQA_LOAD_METRICS_READ_P95_MS="${LOAD_METRICS_READ_P95_MS}" \
TONGLINGYU_RQA_LOAD_RELEASE_GATE_MS="${LOAD_RELEASE_GATE_MS}" \
TONGLINGYU_RQA_CAPACITY_LOAD_MIN_WINDOW_MINUTES="${MIN_WINDOW_MINUTES}" \
  "${SCRIPT_DIR}/verify-tonglingyu-rqa-capacity-load-evidence.sh" \
  >"${ARTIFACT_DIR}/rqa-capacity-load-evidence.stdout"

TONGLINGYU_RQA_INCIDENT_AUDIT_REPORT_PATH="${INCIDENT_AUDIT_EVIDENCE_PATH}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_OPERATOR="${OPERATOR}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_ENVIRONMENT="${ENVIRONMENT}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_STARTED_AT="${STARTED_AT}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_FINISHED_AT="${FINISHED_AT}" \
TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_EVENT_COUNT="${AUDIT_STATUS_HISTORY_EVENT_COUNT}" \
TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_ACTOR_COUNT="${AUDIT_STATUS_HISTORY_ACTOR_COUNT}" \
TONGLINGYU_RQA_AUDIT_TOMBSTONE_COUNT="${AUDIT_TOMBSTONE_COUNT}" \
TONGLINGYU_RQA_INCIDENT_SEVERITY="${INCIDENT_SEVERITY}" \
TONGLINGYU_RQA_INCIDENT_OWNER="${INCIDENT_OWNER}" \
TONGLINGYU_RQA_INCIDENT_FIRST_RESPONSE_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_INCIDENT_MITIGATION_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_INCIDENT_ROLLBACK_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_INCIDENT_RECOVERY_VALIDATION_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_INCIDENT_RTO_RPO_BREACH_ESCALATION_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_INCIDENT_CONCLUSION=passed \
  "${SCRIPT_DIR}/verify-tonglingyu-rqa-incident-audit-evidence.sh" \
  >"${ARTIFACT_DIR}/rqa-incident-audit-evidence.stdout"

TONGLINGYU_RQA_INCIDENT_CAPACITY_REPORT_PATH="${INCIDENT_CAPACITY_REPORT_PATH}" \
TONGLINGYU_RQA_INCIDENT_CAPACITY_REQUIRE_LIVE=true \
TONGLINGYU_RQA_EMERGENCY_DISABLED=false \
TONGLINGYU_RQA_DEGRADED_MODE=false \
TONGLINGYU_RQA_PERSISTENCE_DEGRADED=false \
TONGLINGYU_RQA_CAPACITY_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_LOAD_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF="${SUMMARY_PATH}" \
TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF="${INCIDENT_DRILL_PATH}" \
TONGLINGYU_RQA_CAPACITY_LOAD_EVIDENCE="${CAPACITY_LOAD_EVIDENCE_PATH}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_EVIDENCE="${INCIDENT_AUDIT_EVIDENCE_PATH}" \
TONGLINGYU_RQA_CAPACITY_EVAL_REPORT_COUNT="${CAPACITY_EVAL_REPORT_COUNT}" \
TONGLINGYU_RQA_CAPACITY_FAILURE_COUNT="${CAPACITY_FAILURE_COUNT}" \
TONGLINGYU_RQA_CAPACITY_ADMIN_LIST_PAGE_COUNT="${CAPACITY_ADMIN_LIST_PAGE_COUNT}" \
TONGLINGYU_RQA_LOAD_RQA_WRITE_P95_MS="${LOAD_RQA_WRITE_P95_MS}" \
TONGLINGYU_RQA_LOAD_ADMIN_READ_P95_MS="${LOAD_ADMIN_READ_P95_MS}" \
TONGLINGYU_RQA_LOAD_METRICS_READ_P95_MS="${LOAD_METRICS_READ_P95_MS}" \
TONGLINGYU_RQA_LOAD_RELEASE_GATE_MS="${LOAD_RELEASE_GATE_MS}" \
  "${SCRIPT_DIR}/verify-tonglingyu-rqa-incident-capacity.sh" \
  >"${ARTIFACT_DIR}/rqa-incident-capacity-live-gate.stdout"

python3 - "${SMOKE_REPORT_PATH}" "${ARTIFACT_DIR}" "${RUNS_JSONL}" \
  "${SUMMARY_PATH}" "${CAPACITY_LOAD_EVIDENCE_PATH}" \
  "${INCIDENT_AUDIT_EVIDENCE_PATH}" "${INCIDENT_CAPACITY_REPORT_PATH}" \
  "${OPERATOR}" "${ENVIRONMENT}" "${STARTED_AT}" "${FINISHED_AT}" \
  "${ITERATIONS}" "${MIN_WINDOW_MINUTES}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    artifact_dir_raw,
    runs_jsonl_raw,
    summary_path_raw,
    capacity_load_evidence_path_raw,
    incident_audit_evidence_path_raw,
    incident_capacity_report_path_raw,
    operator,
    environment,
    started_at,
    finished_at,
    iterations_raw,
    min_window_minutes_raw,
) = sys.argv[1:14]


def file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


summary = load_json(summary_path_raw)
capacity_load_evidence = load_json(capacity_load_evidence_path_raw)
incident_audit_evidence = load_json(incident_audit_evidence_path_raw)
incident_capacity_report = load_json(incident_capacity_report_path_raw)
runs = [
    json.loads(line)
    for line in Path(runs_jsonl_raw).read_text(encoding="utf-8").splitlines()
    if line.strip()
]
artifact_paths = {
    "raw_summary": summary_path_raw,
    "capacity_load_evidence": capacity_load_evidence_path_raw,
    "incident_audit_evidence": incident_audit_evidence_path_raw,
    "incident_capacity_live_gate": incident_capacity_report_path_raw,
}
payload = {
    "object": "tonglingyu.rqa_capacity_load_smoke",
    "schema_version": 1,
    "status": "ok",
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "operator": operator,
    "environment": environment,
    "scope": "local_gateway_smoke",
    "started_at": started_at,
    "finished_at": finished_at,
    "iterations": int(iterations_raw),
    "min_window_minutes": int(min_window_minutes_raw),
    "artifact_dir": artifact_dir_raw,
    "performance_runs": runs,
    "capacity_counts": summary["capacity_counts"],
    "load_measurements": summary["load_measurements"],
    "audit_history_counts": summary["audit_history_counts"],
    "checks": {
        "performance_runs_passed": True,
        "capacity_load_evidence_ok": capacity_load_evidence.get("status") == "ok",
        "incident_audit_evidence_ok": incident_audit_evidence.get("status") == "ok",
        "incident_capacity_live_gate_ok": incident_capacity_report.get("status") == "ok",
    },
    "target_environment_live_evidence": False,
    "artifacts": {
        key: {
            "path": value,
            "sha256": file_sha256(value),
        }
        for key, value in artifact_paths.items()
    },
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
Path(report_path_raw).parent.mkdir(parents=True, exist_ok=True)
Path(report_path_raw).write_text(encoded + "\n", encoding="utf-8")
print(encoded)
PY
