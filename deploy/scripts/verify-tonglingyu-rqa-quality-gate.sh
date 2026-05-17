#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

DB_PATH="${TONGLINGYU_RQA_DB_PATH:-${REPO_DIR}/data/tonglingyu/tonglingyu.db}"
EVAL_LIMIT="${TONGLINGYU_RQA_EVAL_LIMIT:-8}"
EVAL_REPORT_PATH="${TONGLINGYU_RQA_EVAL_REPORT_PATH:-}"
EVAL_REPORT_OUTPUT_PATH="${TONGLINGYU_RQA_EVAL_REPORT_OUTPUT_PATH:-}"
GATEWAY_BIN="${TONGLINGYU_RQA_QUALITY_GATEWAY_BIN:-${TONGLINGYU_RQA_GATEWAY_BIN:-}}"
GENERATED_REPORT="false"
EVAL_DB_PATH="${DB_PATH}"

if [[ -z "${EVAL_REPORT_PATH}" ]]; then
  if [[ -n "${EVAL_REPORT_OUTPUT_PATH}" ]]; then
    EVAL_REPORT_PATH="${EVAL_REPORT_OUTPUT_PATH}"
  else
    EVAL_REPORT_PATH="${WORK_DIR}/tonglingyu-rqa-eval-report.json"
  fi
  GENERATED_REPORT="true"
  EVAL_DB_PATH="${WORK_DIR}/tonglingyu-rqa-eval-input.db"
  if ! python3 - "${DB_PATH}" "${EVAL_DB_PATH}" <<'PY'
import sqlite3
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
target_path = Path(sys.argv[2])
if not source_path.is_file():
    raise SystemExit("source_db_missing")
target_path.parent.mkdir(parents=True, exist_ok=True)
source = sqlite3.connect(str(source_path))
target = sqlite3.connect(str(target_path))
try:
    source.backup(target)
finally:
    target.close()
    source.close()
PY
  then
    python3 - <<'PY'
import json

print(json.dumps({
    "object": "tonglingyu.rqa_quality_gate",
    "schema_version": 1,
    "status": "failed",
    "errors": ["eval_db_snapshot_failed"],
    "secret_values_printed": False,
}, ensure_ascii=True, sort_keys=True))
PY
    exit 1
  fi
  if [[ -n "${GATEWAY_BIN}" && -x "${GATEWAY_BIN}" ]]; then
    if ! "${GATEWAY_BIN}" eval \
      --db "${EVAL_DB_PATH}" \
      --limit "${EVAL_LIMIT}" \
      --report "${EVAL_REPORT_PATH}" \
      >"${WORK_DIR}/eval.stdout" \
      2>"${WORK_DIR}/eval.stderr"; then
      eval_failed="true"
    else
      eval_failed="false"
    fi
  else
    if ! (
      cd "${REPO_DIR}/agent-platform"
      cargo run -p tonglingyu-gateway -- eval \
        --db "${EVAL_DB_PATH}" \
        --limit "${EVAL_LIMIT}" \
        --report "${EVAL_REPORT_PATH}" \
        >"${WORK_DIR}/eval.stdout" \
        2>"${WORK_DIR}/eval.stderr"
    ); then
      eval_failed="true"
    else
      eval_failed="false"
    fi
  fi
  if [[ "${eval_failed}" == "true" ]]; then
    python3 - <<'PY'
import json

print(json.dumps({
    "object": "tonglingyu.rqa_quality_gate",
    "schema_version": 1,
    "status": "failed",
    "errors": ["eval_command_failed"],
    "secret_values_printed": False,
}, ensure_ascii=True, sort_keys=True))
PY
    exit 1
  fi
fi

python3 - "${DB_PATH}" "${EVAL_REPORT_PATH}" "${EVAL_LIMIT}" "${GENERATED_REPORT}" "${REPO_DIR}" <<'PY'
import hashlib
import json
import os
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

db_path_raw, eval_report_path_raw, eval_limit_raw, generated_report_raw, repo_dir_raw = sys.argv[1:6]
db_path = Path(db_path_raw)
eval_report_path = Path(eval_report_path_raw)
repo_dir = Path(repo_dir_raw)
errors = []
production_default_thresholds = {
    "quality_report_coverage": 1.0,
    "quality_report_production_ready": 1.0,
    "eval_case_classification": 1.0,
    "expected_evidence_denominator_min": 1,
    "expected_evidence_hit_at_8": 1.0,
    "required_type_coverage": 1.0,
    "exact_term_coverage": 1.0,
    "source_boundary_confirmation_avoided": 1.0,
    "forbidden_conclusion_avoided": 1.0,
    "reviewer_status_matched": 1.0,
    "open_p0_retrieval_failures": 0,
    "open_p0_governance_tasks": 0,
}
threshold_env = {
    "quality_report_coverage": ("TONGLINGYU_RQA_THRESHOLD_QUALITY_REPORT_COVERAGE", float),
    "quality_report_production_ready": ("TONGLINGYU_RQA_THRESHOLD_QUALITY_REPORT_PRODUCTION_READY", float),
    "eval_case_classification": ("TONGLINGYU_RQA_THRESHOLD_EVAL_CASE_CLASSIFICATION", float),
    "expected_evidence_denominator_min": ("TONGLINGYU_RQA_THRESHOLD_EXPECTED_EVIDENCE_DENOMINATOR_MIN", int),
    "expected_evidence_hit_at_8": ("TONGLINGYU_RQA_THRESHOLD_EXPECTED_EVIDENCE_HIT_AT_8", float),
    "required_type_coverage": ("TONGLINGYU_RQA_THRESHOLD_REQUIRED_TYPE_COVERAGE", float),
    "exact_term_coverage": ("TONGLINGYU_RQA_THRESHOLD_EXACT_TERM_COVERAGE", float),
    "source_boundary_confirmation_avoided": ("TONGLINGYU_RQA_THRESHOLD_SOURCE_BOUNDARY_CONFIRMATION_AVOIDED", float),
    "forbidden_conclusion_avoided": ("TONGLINGYU_RQA_THRESHOLD_FORBIDDEN_CONCLUSION_AVOIDED", float),
    "reviewer_status_matched": ("TONGLINGYU_RQA_THRESHOLD_REVIEWER_STATUS_MATCHED", float),
    "open_p0_retrieval_failures": ("TONGLINGYU_RQA_THRESHOLD_OPEN_P0_RETRIEVAL_FAILURES", int),
    "open_p0_governance_tasks": ("TONGLINGYU_RQA_THRESHOLD_OPEN_P0_GOVERNANCE_TASKS", int),
}


def parse_thresholds():
    thresholds = dict(production_default_thresholds)
    overrides = []
    invalid_overrides = []
    less_strict_overrides = []
    for key, (env_name, parser) in threshold_env.items():
        raw_value = os.environ.get(env_name)
        if raw_value is None or not raw_value.strip():
            continue
        raw_value = raw_value.strip()
        try:
            parsed = parser(raw_value)
        except ValueError:
            invalid_overrides.append({"key": key, "env": env_name, "value": raw_value})
            continue
        if parser is float:
            parsed = float(parsed)
            if parsed < 0.0 or parsed > 1.0:
                invalid_overrides.append({"key": key, "env": env_name, "value": raw_value})
                continue
        elif parsed < 0:
            invalid_overrides.append({"key": key, "env": env_name, "value": raw_value})
            continue
        thresholds[key] = parsed
        overrides.append({"key": key, "env": env_name, "value": parsed})
        default = production_default_thresholds[key]
        if key in ("open_p0_retrieval_failures", "open_p0_governance_tasks"):
            if parsed > default:
                less_strict_overrides.append(key)
        elif parsed < default:
            less_strict_overrides.append(key)
    config = {
        "source": "environment_overrides" if overrides else "production_defaults",
        "override_env_prefix": "TONGLINGYU_RQA_THRESHOLD_",
        "overrides": overrides,
        "invalid_overrides": invalid_overrides,
        "less_strict_overrides": less_strict_overrides,
        "production_ready_thresholds_enforced": not invalid_overrides and not less_strict_overrides,
    }
    return thresholds, config


thresholds, threshold_config = parse_thresholds()
if threshold_config["invalid_overrides"]:
    errors.append("threshold_config_invalid")
if threshold_config["less_strict_overrides"]:
    errors.append("thresholds_below_production_defaults")


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def optional_file_sha256(path):
    try:
        return file_sha256(path)
    except OSError:
        errors.append(f"policy_file_missing={path.name}")
        return ""


def canonical_digest(value):
    encoded = json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    return sha256_bytes(encoded.encode("utf-8"))


def table_exists(conn, name):
    return conn.execute(
        """
        SELECT 1
        FROM sqlite_master
        WHERE type = 'table' AND name = ?
        LIMIT 1
        """,
        (name,),
    ).fetchone() is not None


def load_json_value(raw, default):
    if raw is None:
        return default
    try:
        return json.loads(raw)
    except (TypeError, json.JSONDecodeError):
        return default


def state_count_defaults():
    return {
        "source_snapshot": 0,
        "candidate": 0,
        "system_calibrated": 0,
        "runtime_usable": 0,
        "human_marked": 0,
        "rejected": 0,
        "deprecated": 0,
    }


def build_knowledge_state_db_summary(conn):
    required_tables = [
        "knowledge_items",
        "knowledge_item_state_history",
        "knowledge_calibration_reports",
        "knowledge_calibration_jobs",
        "kb_version_diff_reports",
    ]
    missing_tables = [name for name in required_tables if not table_exists(conn, name)]
    for name in missing_tables:
        errors.append(f"knowledge_state_table_missing={name}")
    states = state_count_defaults()
    by_kind = {}
    runtime_policy_version = "tonglingyu-knowledge-runtime-policy-v1"
    runtime_policy_promotion_summary = {
        "object": "tonglingyu.knowledge_runtime_policy_promotion_summary",
        "policy_version": runtime_policy_version,
        "runtime_usable_count": 0,
        "human_marked_count": 0,
        "by_kind": {},
        "release_run_refs": [],
    }
    if "knowledge_items" not in missing_tables:
        for row in conn.execute("SELECT state, COUNT(*) FROM knowledge_items GROUP BY state"):
            states[str(row[0])] = int(row[1])
        for row in conn.execute(
            """
            SELECT kind, state, COUNT(*)
            FROM knowledge_items
            GROUP BY kind, state
            ORDER BY kind, state
            """
        ):
            by_kind.setdefault(str(row[0]), {})[str(row[1])] = int(row[2])
        release_run_refs = set()
        for row in conn.execute(
            """
            SELECT kind, state, payload_json
            FROM knowledge_items
            WHERE state IN ('runtime_usable', 'human_marked')
            ORDER BY kind, state
            """
        ):
            kind = str(row[0])
            state = str(row[1])
            payload = load_json_value(row[2], {})
            runtime_policy_promotion_summary["by_kind"].setdefault(kind, {})
            runtime_policy_promotion_summary["by_kind"][kind][state] = (
                runtime_policy_promotion_summary["by_kind"][kind].get(state, 0) + 1
            )
            if state == "runtime_usable":
                runtime_policy_promotion_summary["runtime_usable_count"] += 1
                release_run_id = (payload.get("runtime_policy") or {}).get("release_run_id")
                if isinstance(release_run_id, str) and release_run_id.strip():
                    release_run_refs.add("sha256:" + sha256_bytes(release_run_id.encode("utf-8")))
            if state == "human_marked":
                runtime_policy_promotion_summary["human_marked_count"] += 1
        runtime_policy_promotion_summary["release_run_refs"] = sorted(release_run_refs)
    report_summary = {
        "total": 0,
        "by_decision": {},
        "latest_report_ref": None,
        "latest_report_hash": None,
    }
    if "knowledge_calibration_reports" not in missing_tables:
        for row in conn.execute(
            """
            SELECT decision, COUNT(*)
            FROM knowledge_calibration_reports
            GROUP BY decision
            """
        ):
            report_summary["by_decision"][str(row[0])] = int(row[1])
            report_summary["total"] += int(row[1])
        latest_report = conn.execute(
            """
            SELECT report_ref, report_hash
            FROM knowledge_calibration_reports
            ORDER BY created_at DESC, report_id DESC
            LIMIT 1
            """
        ).fetchone()
        if latest_report is not None:
            report_summary["latest_report_ref"] = latest_report[0]
            report_summary["latest_report_hash"] = latest_report[1]
    calibration_job_summary = {
        "object": "tonglingyu.knowledge_calibration_job_summary",
        "total": 0,
        "by_status": {},
        "latest_run_id": None,
        "latest_input_artifact_digest": None,
        "latest_output_report_digest": None,
        "config_digests": [],
        "failed_task_summary": [],
        "failed_or_retry_waiting": 0,
    }
    if "knowledge_calibration_jobs" not in missing_tables:
        for row in conn.execute(
            """
            SELECT status, COUNT(*)
            FROM knowledge_calibration_jobs
            GROUP BY status
            """
        ):
            calibration_job_summary["by_status"][str(row[0])] = int(row[1])
            calibration_job_summary["total"] += int(row[1])
        latest_job = conn.execute(
            """
            SELECT job_id, input_digest, config_digest, report_id
            FROM knowledge_calibration_jobs
            ORDER BY updated_at DESC, job_id DESC
            LIMIT 1
            """
        ).fetchone()
        if latest_job is not None:
            calibration_job_summary["latest_run_id"] = latest_job[0]
            calibration_job_summary["latest_input_artifact_digest"] = latest_job[1]
            if latest_job[2]:
                calibration_job_summary["config_digests"].append(latest_job[2])
            if latest_job[3] and "knowledge_calibration_reports" not in missing_tables:
                report_hash = conn.execute(
                    """
                    SELECT report_hash
                    FROM knowledge_calibration_reports
                    WHERE report_id = ?
                    """,
                    (latest_job[3],),
                ).fetchone()
                if report_hash is not None:
                    calibration_job_summary["latest_output_report_digest"] = report_hash[0]
        for row in conn.execute(
            """
            SELECT job_id, status, input_kind, method, last_error_sha256
            FROM knowledge_calibration_jobs
            WHERE status IN ('failed', 'retry_waiting')
            ORDER BY updated_at DESC, job_id DESC
            LIMIT 20
            """
        ):
            calibration_job_summary["failed_task_summary"].append({
                "job_id": row[0],
                "status": row[1],
                "input_kind": row[2],
                "method": row[3],
                "last_error_sha256": row[4],
            })
        calibration_job_summary["failed_or_retry_waiting"] = len(
            calibration_job_summary["failed_task_summary"]
        )
    per_kind_coverage_matrix = []
    for kind, counts in sorted(by_kind.items()):
        total = sum(int(value) for value in counts.values())
        runtime_or_human = int(counts.get("runtime_usable", 0)) + int(
            counts.get("human_marked", 0)
        )
        per_kind_coverage_matrix.append({
            "kind": kind,
            "state_counts": counts,
            "total": total,
            "runtime_or_human_usable": runtime_or_human,
            "runtime_or_human_ratio": (runtime_or_human / total) if total else None,
        })
    unresolved_gaps = {
        "candidate_or_source_snapshot": states["candidate"] + states["source_snapshot"],
        "system_calibrated_not_runtime_usable": states["system_calibrated"],
        "rejected_or_deprecated": states["rejected"] + states["deprecated"],
        "calibration_failed_or_retry_waiting": calibration_job_summary["failed_or_retry_waiting"],
    }
    state_counts = {
        "object": "tonglingyu.knowledge_state_counts",
        "states": states,
        "runtime_usable_count": states["runtime_usable"],
        "human_marked_count": states["human_marked"],
        "system_calibrated_count": states["system_calibrated"],
        "rejected_or_deprecated_count": states["rejected"] + states["deprecated"],
        "candidate_or_source_snapshot_count": states["candidate"] + states["source_snapshot"],
        "total_count": sum(states.values()),
    }
    summary = {
        "object": "tonglingyu.knowledge_state_release_summary",
        "schema_version": "tonglingyu-knowledge-item-state-v1",
        "runtime_policy_version": runtime_policy_version,
        "state_counts": state_counts,
        "by_kind": by_kind,
        "calibration_report_summary": report_summary,
        "calibration_job_summary": calibration_job_summary,
        "runtime_policy_promotion_summary": runtime_policy_promotion_summary,
        "per_kind_coverage_matrix": per_kind_coverage_matrix,
        "unresolved_gaps": unresolved_gaps,
    }
    summary["summary_sha256"] = canonical_digest({
        "state_counts": state_counts,
        "by_kind": by_kind,
        "calibration_report_summary": report_summary,
        "calibration_job_summary": calibration_job_summary,
        "runtime_policy_promotion_summary": runtime_policy_promotion_summary,
        "per_kind_coverage_matrix": per_kind_coverage_matrix,
        "unresolved_gaps": unresolved_gaps,
    })
    return summary


def latest_kb_diff_report_summary(conn):
    if not table_exists(conn, "kb_version_diff_reports"):
        errors.append("kb_diff_reports_table_missing")
        return {}, ""
    row = conn.execute(
        """
        SELECT report_id, schema_version, before_version_id, after_version_id,
               source_root, before_summary_json, after_summary_json, diff_json,
               eval_before_summary_json, eval_after_summary_json, eval_diff_json,
               created_at, updated_at
        FROM kb_version_diff_reports
        ORDER BY created_at DESC, report_id DESC
        LIMIT 1
        """
    ).fetchone()
    if row is None:
        errors.append("kb_diff_report_missing")
        return {}, ""
    report = {
        "object": "tonglingyu.kb_version_diff_report",
        "report_id": row[0],
        "schema_version": row[1],
        "before_version_id": row[2],
        "after_version_id": row[3],
        "source_root": row[4],
        "before_summary": load_json_value(row[5], None),
        "after_summary": load_json_value(row[6], {}),
        "diff": load_json_value(row[7], {}),
        "eval_before_summary": load_json_value(row[8], None),
        "eval_after_summary": load_json_value(row[9], {}),
        "eval_diff": load_json_value(row[10], {}),
        "created_at": row[11],
        "updated_at": row[12],
    }
    report_digest = canonical_digest(report)
    diff = report.get("diff") if isinstance(report.get("diff"), dict) else {}
    eval_diff = report.get("eval_diff") if isinstance(report.get("eval_diff"), dict) else {}
    knowledge_state_diff = diff.get("knowledge_state")
    if not isinstance(knowledge_state_diff, dict):
        knowledge_state_diff = {
            "object": "tonglingyu.knowledge_state_kb_diff",
            "schema_version": "tonglingyu-knowledge-item-state-v1",
            "runtime_policy_version": "tonglingyu-knowledge-runtime-policy-v1",
            "state_counts": {},
            "by_kind": {},
            "state_change_refs": [],
            "runtime_policy_promotion_summary": {},
            "calibration_job_summary": {},
            "unresolved_gaps": [],
            "status": "unchanged",
        }
    summary = {
        "object": "tonglingyu.kb_version_diff_release_ref",
        "report_id": report["report_id"],
        "schema_version": report["schema_version"],
        "before_version_id": report["before_version_id"],
        "after_version_id": report["after_version_id"],
        "created_at": report["created_at"],
        "updated_at": report["updated_at"],
        "report_sha256": report_digest,
        "diff_sha256": canonical_digest(diff),
        "eval_diff_sha256": canonical_digest(eval_diff),
        "knowledge_state_diff": knowledge_state_diff,
        "eval_diff": eval_diff,
    }
    return summary, report_digest


def ratio(summary, field):
    value = summary.get(field)
    if not isinstance(value, dict):
        errors.append(f"{field}_missing")
        return None
    result = value.get("ratio")
    if not isinstance(result, (int, float)):
        errors.append(f"{field}_ratio_missing")
        return None
    return float(result)


def count_value(summary, field, key):
    value = summary.get(field)
    if not isinstance(value, dict):
        errors.append(f"{field}_missing")
        return None
    result = value.get(key)
    if not isinstance(result, int):
        errors.append(f"{field}_{key}_missing")
        return None
    return result


def add_if(condition, error):
    if condition:
        errors.append(error)


report = {}
if not eval_report_path.is_file():
    errors.append("eval_report_not_found")
else:
    try:
        report = json.loads(eval_report_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        errors.append("eval_report_json_invalid")

summary = report.get("quality_summary") if isinstance(report, dict) else None
if not isinstance(summary, dict):
    errors.append("quality_summary_missing")
    summary = {}

add_if(report.get("object") != "tonglingyu.eval_report", "eval_report_object_invalid")
add_if(report.get("status") != "passed", "eval_report_status_not_passed")
add_if(summary.get("status") != "passed", "quality_summary_status_not_passed")
add_if(bool(summary.get("blockers")), "quality_summary_blockers_present")
add_if(summary.get("eval_failure_records") != 0, "eval_failure_records_not_zero")

if ratio(summary, "quality_report_coverage") != thresholds["quality_report_coverage"]:
    errors.append("quality_report_coverage_below_threshold")
if ratio(summary, "quality_report_production_ready") != thresholds["quality_report_production_ready"]:
    errors.append("quality_report_production_ready_below_threshold")
if ratio(summary, "eval_case_classification") != thresholds["eval_case_classification"]:
    errors.append("eval_case_classification_below_threshold")
if ratio(summary, "expected_evidence_hit_at_8") != thresholds["expected_evidence_hit_at_8"]:
    errors.append("expected_evidence_hit_at_8_below_threshold")
if ratio(summary, "required_type_coverage") != thresholds["required_type_coverage"]:
    errors.append("required_type_coverage_below_threshold")
exact_total = count_value(summary, "exact_term_coverage", "total")
exact_ratio = ratio(summary, "exact_term_coverage")
if exact_total and exact_ratio != thresholds["exact_term_coverage"]:
    errors.append("exact_term_coverage_below_threshold")
if ratio(summary, "source_boundary_confirmation_avoided") != thresholds["source_boundary_confirmation_avoided"]:
    errors.append("source_boundary_confirmation_avoided_below_threshold")
if ratio(summary, "forbidden_conclusion_avoided") != thresholds["forbidden_conclusion_avoided"]:
    errors.append("forbidden_conclusion_avoided_below_threshold")
if ratio(summary, "reviewer_status_matched") != thresholds["reviewer_status_matched"]:
    errors.append("reviewer_status_matched_below_threshold")
expected_denominator = summary.get("expected_evidence_denominator")
if not isinstance(expected_denominator, int) or expected_denominator < thresholds["expected_evidence_denominator_min"]:
    errors.append("expected_evidence_denominator_below_threshold")
knowledge_state_quality = summary.get("knowledge_state_quality")
if not isinstance(knowledge_state_quality, dict):
    errors.append("knowledge_state_quality_missing")
    knowledge_state_quality = {}
add_if(
    knowledge_state_quality.get("runtime_policy_rejected_count") != 0,
    "knowledge_state_runtime_policy_rejected",
)
add_if(
    knowledge_state_quality.get("rejected_or_deprecated_selected_count") != 0,
    "knowledge_state_rejected_or_deprecated_selected",
)
add_if(
    knowledge_state_quality.get("reviewer_downgrade_cases") != 0,
    "knowledge_state_reviewer_downgrade",
)
add_if(
    knowledge_state_quality.get("forbidden_failure_cases") != 0,
    "knowledge_state_forbidden_failure",
)

source_coverage_boundary = summary.get("source_coverage_boundary") or {}
if not isinstance(source_coverage_boundary, dict):
    source_coverage_boundary = {}
add_if(
    source_coverage_boundary.get("source_snapshot_status") != "wikisource_source_snapshot",
    "source_snapshot_status_invalid",
)
for field in (
    "facsimile_review_status",
    "authoritative_edition_review_status",
    "expert_collation_status",
):
    add_if(source_coverage_boundary.get(field) != "not_reviewed", f"{field}_unexpected")

db_summary = {
    "db_path": str(db_path),
    "kb_version": None,
    "source_count": 0,
    "block_count": 0,
}
source_license_summary = {
    "source_count": 0,
    "sources": [],
    "missing_metadata": [],
}
open_retrieval_failures = None
open_governance_tasks = None
source_snapshot_digest = ""
kb_build_hash = ""
knowledge_state_summary = {
    "object": "tonglingyu.knowledge_state_release_summary",
    "schema_version": "tonglingyu-knowledge-item-state-v1",
    "runtime_policy_version": "tonglingyu-knowledge-runtime-policy-v1",
    "state_counts": {
        "object": "tonglingyu.knowledge_state_counts",
        "states": state_count_defaults(),
        "runtime_usable_count": 0,
        "human_marked_count": 0,
        "system_calibrated_count": 0,
        "rejected_or_deprecated_count": 0,
        "candidate_or_source_snapshot_count": 0,
        "total_count": 0,
    },
    "by_kind": {},
    "calibration_report_summary": {},
    "calibration_job_summary": {},
    "runtime_policy_promotion_summary": {},
    "per_kind_coverage_matrix": [],
    "unresolved_gaps": {},
    "summary_sha256": "",
}
kb_diff_report = {}
kb_diff_report_sha256 = ""
if not db_path.is_file():
    errors.append("db_not_found")
else:
    try:
        conn = sqlite3.connect(str(db_path))
        conn.row_factory = sqlite3.Row
        kb_version = conn.execute(
            """
            SELECT version_id, source_root, source_count, block_count, schema_version, built_at
            FROM kb_version
            ORDER BY built_at DESC, version_id DESC
            LIMIT 1
            """
        ).fetchone()
        if kb_version is None:
            errors.append("kb_version_missing")
        else:
            db_summary["kb_version"] = dict(kb_version)
            db_summary["source_count"] = int(kb_version["source_count"])
            db_summary["block_count"] = int(kb_version["block_count"])
        source_rows = [
            dict(row)
            for row in conn.execute(
                """
                SELECT source_id, source_hash, license, license_url, license_source_url,
                       attribution, usage_boundary
                FROM sources
                ORDER BY source_id
                """
            )
        ]
        source_license_summary["source_count"] = len(source_rows)
        if not source_rows:
            errors.append("sources_missing")
        for source in source_rows:
            missing = [
                field
                for field in (
                    "license",
                    "license_url",
                    "license_source_url",
                    "attribution",
                    "usage_boundary",
                )
                if not str(source.get(field) or "").strip()
            ]
            if missing:
                source_license_summary["missing_metadata"].append({
                    "source_id": source.get("source_id"),
                    "missing": missing,
                })
            source_license_summary["sources"].append({
                "source_id": source.get("source_id"),
                "source_hash": source.get("source_hash"),
                "license": source.get("license"),
                "license_url": source.get("license_url"),
                "license_source_url": source.get("license_source_url"),
                "attribution": source.get("attribution"),
                "usage_boundary_sha256": sha256_bytes(
                    str(source.get("usage_boundary") or "").encode("utf-8")
                ),
            })
        if source_license_summary["missing_metadata"]:
            errors.append("source_usage_metadata_incomplete")
        source_snapshot_digest = canonical_digest(source_license_summary["sources"])
        kb_build_hash = canonical_digest({
            "kb_version": db_summary["kb_version"],
            "source_snapshot_digest": source_snapshot_digest,
        })
        source_ids = {source["source_id"] for source in source_rows}
        eval_source_ids = set(
            (summary.get("source_diversity") or {}).get("source_ids") or []
        )
        if eval_source_ids and not eval_source_ids.issubset(source_ids):
            errors.append("eval_source_ids_not_in_current_kb")
        open_retrieval_failures = conn.execute(
            """
            SELECT COUNT(*)
            FROM retrieval_failures
            WHERE human_review_status IN ('open', 'in_review')
            """
        ).fetchone()[0]
        if open_retrieval_failures != thresholds["open_p0_retrieval_failures"]:
            errors.append("open_retrieval_failures_present")
        governance_tasks_table = conn.execute(
            """
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = 'knowledge_governance_tasks'
            LIMIT 1
            """
        ).fetchone()
        if governance_tasks_table is None:
            errors.append("governance_tasks_table_missing")
        else:
            open_governance_tasks = conn.execute(
                """
                SELECT COUNT(*)
                FROM knowledge_governance_tasks
                WHERE priority = 'p0'
                  AND status IN ('open', 'in_review', 'accepted')
                """
            ).fetchone()[0]
            if open_governance_tasks != thresholds["open_p0_governance_tasks"]:
                errors.append("open_governance_tasks_present")
        knowledge_state_summary = build_knowledge_state_db_summary(conn)
        kb_diff_report, kb_diff_report_sha256 = latest_kb_diff_report_summary(conn)
        calibration_jobs = knowledge_state_summary.get("calibration_job_summary") or {}
        if calibration_jobs.get("failed_or_retry_waiting") != 0:
            errors.append("knowledge_calibration_jobs_failed_or_retry_waiting")
    except sqlite3.Error:
        errors.append("db_query_failed")

quality_summary_public = {
    "status": summary.get("status"),
    "blockers": summary.get("blockers"),
    "quality_report_coverage": summary.get("quality_report_coverage"),
    "quality_report_production_ready": summary.get("quality_report_production_ready"),
    "eval_case_classification": summary.get("eval_case_classification"),
    "expected_evidence_denominator": summary.get("expected_evidence_denominator"),
    "expected_evidence_hit_at_8": summary.get("expected_evidence_hit_at_8"),
    "required_type_coverage": summary.get("required_type_coverage"),
    "exact_term_coverage": summary.get("exact_term_coverage"),
    "source_boundary_confirmation_avoided": summary.get("source_boundary_confirmation_avoided"),
    "forbidden_conclusion_avoided": summary.get("forbidden_conclusion_avoided"),
    "reviewer_status_matched": summary.get("reviewer_status_matched"),
    "knowledge_state_quality": knowledge_state_quality,
    "eval_failure_records": summary.get("eval_failure_records"),
    "source_coverage_boundary": source_coverage_boundary,
    "source_diversity": {
        "count": (summary.get("source_diversity") or {}).get("count"),
        "source_ids": (summary.get("source_diversity") or {}).get("source_ids") or [],
    },
}
eval_report_sha256 = file_sha256(eval_report_path) if eval_report_path.is_file() else ""
eval_run_id = f"rqa-eval-{eval_report_sha256[:16]}" if eval_report_sha256 else ""
eval_report_resolved_path = str(eval_report_path.expanduser().resolve())
runtime_policy_digest = optional_file_sha256(
    repo_dir / "agent-platform" / "crates" / "tonglingyu-runtime" / "src" / "lib.rs"
)
gateway_policy_digest = optional_file_sha256(
    repo_dir / "agent-platform" / "crates" / "tonglingyu-gateway" / "src" / "main.rs"
)
model_upstream_id = (
    os.environ.get("TONGLINGYU_UPSTREAM_MODEL")
    or os.environ.get("AGENT_RUNTIME_HERMES_MODEL")
    or ""
).strip()
if not model_upstream_id:
    errors.append("model_upstream_id_missing")
behavior_config = {
    "agent_runtime_mode_env": "TONGLINGYU_AGENT_RUNTIME_MODE",
    "decoding_parameters_summary": {
        "source": "gateway_runtime_config",
        "upstream_timeout_secs_env": "TONGLINGYU_UPSTREAM_TIMEOUT_SECS",
    },
    "profile_contract": "tonglingyu-runtime-profile-contract-v1",
    "runtime_profile_digest": runtime_policy_digest,
    "prompt_digest": runtime_policy_digest,
    "tool_policy": "read_only_runtime_tools",
    "tool_policy_digest": runtime_policy_digest,
    "reviewer_policy": "local_reviewer_enforced",
    "reviewer_policy_digest": runtime_policy_digest,
    "gateway_policy_digest": gateway_policy_digest,
    "model_upstream_id": model_upstream_id,
    "model_upstream_bound_by_gate": "model_upstream_network",
    "decoding_parameters_source": "gateway_runtime_config",
}
behavior_config["behavior_config_digest"] = canonical_digest(behavior_config)
eval_impact = {
    "object": "tonglingyu.knowledge_state_eval_impact",
    "eval_run_id": eval_run_id,
    "quality_summary_status": summary.get("status"),
    "knowledge_state_quality": knowledge_state_quality,
    "kb_diff_eval_diff": kb_diff_report.get("eval_diff") if isinstance(kb_diff_report, dict) else {},
}
gate = {
    "object": "tonglingyu.rqa_quality_gate",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "quality_gate_passed": not errors,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "errors": errors,
    "production_default_thresholds": production_default_thresholds,
    "effective_thresholds": thresholds,
    "threshold_config": threshold_config,
    "rqa_schema_version": "tonglingyu-retrieval-failures-v1",
    "eval_suite_version": summary.get("schema_version"),
    "eval_run_id": eval_run_id,
    "eval_report_sha256": eval_report_sha256,
    "eval_report_path": eval_report_resolved_path,
    "eval_report_generated_by_gate": generated_report_raw == "true",
    "eval_report_db_source": (
        "snapshot_copy" if generated_report_raw == "true" else "provided_report"
    ),
    "live_db_mutated_by_eval": False,
    "eval_limit": int(eval_limit_raw),
    "source_snapshot_digest": source_snapshot_digest,
    "kb_build_hash": kb_build_hash,
    "kb_version": db_summary["kb_version"],
    "db_summary": {
        "source_count": db_summary["source_count"],
        "block_count": db_summary["block_count"],
    },
    "source_license_summary": source_license_summary,
    "quality_summary": quality_summary_public,
    "knowledge_state_summary": knowledge_state_summary,
    "knowledge_state_summary_sha256": knowledge_state_summary.get("summary_sha256"),
    "kb_diff_report": kb_diff_report,
    "kb_diff_report_sha256": kb_diff_report_sha256,
    "eval_impact": eval_impact,
    "runtime_policy_promotion_summary": knowledge_state_summary.get("runtime_policy_promotion_summary"),
    "per_kind_coverage_matrix": knowledge_state_summary.get("per_kind_coverage_matrix"),
    "calibration_job_summary": knowledge_state_summary.get("calibration_job_summary"),
    "unresolved_calibration_gaps": knowledge_state_summary.get("unresolved_gaps"),
    "open_p0_retrieval_failures": open_retrieval_failures,
    "open_p0_governance_tasks": open_governance_tasks,
    "behavior_config": behavior_config,
    "secret_values_printed": False,
}
print(json.dumps(gate, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if not errors else 1)
PY
