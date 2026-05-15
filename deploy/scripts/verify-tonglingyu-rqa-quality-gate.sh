#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

DB_PATH="${TONGLINGYU_RQA_DB_PATH:-${REPO_DIR}/data/tonglingyu/tonglingyu.db}"
EVAL_LIMIT="${TONGLINGYU_RQA_EVAL_LIMIT:-8}"
EVAL_REPORT_PATH="${TONGLINGYU_RQA_EVAL_REPORT_PATH:-}"
EVAL_REPORT_OUTPUT_PATH="${TONGLINGYU_RQA_EVAL_REPORT_OUTPUT_PATH:-}"
GENERATED_REPORT="false"

if [[ -z "${EVAL_REPORT_PATH}" ]]; then
  if [[ -n "${EVAL_REPORT_OUTPUT_PATH}" ]]; then
    EVAL_REPORT_PATH="${EVAL_REPORT_OUTPUT_PATH}"
  else
    EVAL_REPORT_PATH="${WORK_DIR}/tonglingyu-rqa-eval-report.json"
  fi
  GENERATED_REPORT="true"
  if ! (
    cd "${REPO_DIR}/agent-platform"
    cargo run -p tonglingyu-gateway -- eval \
      --db "${DB_PATH}" \
      --limit "${EVAL_LIMIT}" \
      --report "${EVAL_REPORT_PATH}" \
      >"${WORK_DIR}/eval.stdout" \
      2>"${WORK_DIR}/eval.stderr"
  ); then
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
    "open_p0_retrieval_failures": open_retrieval_failures,
    "open_p0_governance_tasks": open_governance_tasks,
    "behavior_config": behavior_config,
    "secret_values_printed": False,
}
print(json.dumps(gate, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if not errors else 1)
PY
