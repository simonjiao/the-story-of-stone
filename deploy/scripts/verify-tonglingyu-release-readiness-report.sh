#!/usr/bin/env bash
set -euo pipefail

REPORT_PATH="${1:-${TONGLINGYU_RELEASE_REPORT_PATH:-}}"

python3 - "${REPORT_PATH}" <<'PY'
import json
import hashlib
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

report_path = sys.argv[1].strip()
errors = []
report = {}
report_max_age_hours_raw = os.environ.get(
    "TONGLINGYU_RELEASE_REPORT_MAX_AGE_HOURS",
    "24",
).strip()
browser_review_evidence_root = os.environ.get(
    "TONGLINGYU_BROWSER_REVIEW_EVIDENCE_ROOT",
    "",
).strip()
live_gate_names = [
    "model_upstream_network",
    "strict_gateway",
    "openwebui_function",
    "openwebui_admin_action",
]
required_gate_names = [
    "runtime_config",
    "retrieval_quality",
    *live_gate_names,
    "openwebui_browser_review",
]
allowed_gate_names = set(required_gate_names)
manual_browser_checks = [
    "Open WebUI browser-side ordinary-user model visibility",
    "Open WebUI browser-side admin audit entry visibility",
    "Open WebUI streaming chat UX against the live public endpoint",
    "Human confirmation that existing Open WebUI webui.db persisted settings match env-rendered provider settings",
]
required_browser_review_items = [
    "ordinary_user_model_visibility",
    "streaming_chat_ux",
    "admin_audit_visibility",
    "persisted_provider_settings",
]
required_browser_review_item_set = set(required_browser_review_items)
browser_review_allowed_ref_kinds = {
    "ordinary_user_model_visibility": {"local_file", "url"},
    "streaming_chat_ux": {"local_file", "url"},
    "admin_audit_visibility": {"trace", "local_file", "url"},
    "persisted_provider_settings": {"runbook", "local_file", "url"},
}
allowed_report_statuses = {
    "failed",
    "passed",
    "passed_with_failed_optional_gates",
    "passed_with_gate_command_overrides",
    "passed_with_skipped_gates",
    "passed_in_summary_only_mode",
}
allowed_gate_statuses = {"passed", "failed", "skipped"}
max_tail_lines = 20
max_tail_line_chars = 16384
secret_value_needles = [
    "api-key=",
    "api_key=",
    "apikey=",
    "authorization:",
    "bearer ",
    "ghp_",
    "github_pat_",
    "password=",
    "secret=",
    "sk-",
    "token=",
    "x-api-key:",
    "xoxb-",
]
gate_stdout_requirements = {
    "runtime_config": {
        "required_fields": [
            "checked_policy_fields",
            "checked_secret_fields",
            "checked_services",
        ],
    },
    "retrieval_quality": {
        "object": "tonglingyu.rqa_quality_gate",
        "required_fields": [
            "behavior_config",
            "effective_thresholds",
            "eval_report_sha256",
            "eval_run_id",
            "eval_suite_version",
            "kb_build_hash",
            "kb_version",
            "quality_gate_passed",
            "quality_summary",
            "rqa_schema_version",
            "source_license_summary",
            "source_snapshot_digest",
        ],
    },
    "model_upstream_network": {
        "object": "tonglingyu.model_upstream_network_gate",
        "required_fields": [
            "probe_count",
            "probes",
        ],
    },
    "strict_gateway": {
        "exact": {
            "agent_runtime_mode": "hermes",
        },
        "required_fields": [
            "checked_surfaces",
            "model_ids",
            "stream_trace_id",
            "trace_id",
        ],
    },
    "openwebui_function": {
        "exact": {
            "function_id": "agent_identity_bridge",
            "type": "filter",
        },
    },
    "openwebui_admin_action": {
        "exact": {
            "function_id": "tonglingyu_gateway_admin",
            "type": "action",
        },
    },
}
hex_digits = set("0123456789abcdef")


def emit(status):
    print(
        json.dumps(
            {
                "object": "tonglingyu.release_readiness_report_validation",
                "schema_version": 1,
                "status": status,
                "report_path": report_path,
                "production_release_ready": bool(report.get("production_release_ready")),
                "errors": errors,
                "secret_values_printed": False,
            },
            ensure_ascii=True,
            sort_keys=True,
        )
    )


def add_if(condition, error):
    if condition:
        errors.append(error)


def add_mismatch(field, expected, actual):
    if expected != actual:
        errors.append(f"{field}_mismatch")


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def is_sha256(value):
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(char in hex_digits for char in value.lower())
    )


def resolve_artifact_path(value):
    artifact_path = Path(value)
    if artifact_path.is_absolute():
        return artifact_path
    return path.parent / artifact_path


def safe_relative_path(value):
    candidate = Path(value)
    if candidate.is_absolute():
        return None
    if any(part in {"", ".", ".."} for part in candidate.parts):
        return None
    return candidate


def resolve_browser_evidence_ref(ref, evidence_path):
    relative = safe_relative_path(ref)
    if relative is None:
        return None
    base = (
        Path(browser_review_evidence_root)
        if browser_review_evidence_root
        else evidence_path.parent
    )
    return base / relative


def file_sha256(file_path):
    digest = hashlib.sha256()
    with file_path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def secret_value_paths(value, prefix="$"):
    paths = []
    if isinstance(value, dict):
        for key, child in value.items():
            paths.extend(secret_value_paths(child, f"{prefix}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            paths.extend(secret_value_paths(child, f"{prefix}[{index}]"))
    elif isinstance(value, str):
        lowered = value.lower()
        if any(needle in lowered for needle in secret_value_needles):
            paths.append(prefix)
    return paths


def validate_gate_tail(name, gate, field):
    tail = gate.get(field)
    if not isinstance(tail, list):
        errors.append(f"{name}_{field}_must_be_array")
        return
    if len(tail) > max_tail_lines:
        errors.append(f"{name}_{field}_too_many_lines")
    for index, line in enumerate(tail):
        if not isinstance(line, str):
            errors.append(f"{name}_{field}_{index}_must_be_string")
            continue
        if len(line) > max_tail_line_chars:
            errors.append(f"{name}_{field}_{index}_too_long")
        if "\r" in line or "\n" in line:
            errors.append(f"{name}_{field}_{index}_contains_newline")


def parse_timestamp(value):
    if not isinstance(value, str):
        return None
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None or parsed.tzinfo.utcoffset(parsed) is None:
        return None
    return parsed.astimezone(timezone.utc)


def success_json_from_gate_stdout(gate, expected_object=None):
    if not isinstance(gate, dict):
        return None
    stdout_tail = gate.get("stdout_tail")
    if not isinstance(stdout_tail, list):
        return None
    for line in reversed(stdout_tail):
        if not isinstance(line, str):
            continue
        try:
            candidate = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(candidate, dict) or candidate.get("status") != "ok":
            continue
        if expected_object and candidate.get("object") != expected_object:
            continue
        return candidate
    return None


def browser_validation_from_gate(gate):
    return success_json_from_gate_stdout(
        gate,
        "tonglingyu.openwebui_browser_review_gate",
    )


def ratio_is_one(value):
    return isinstance(value, dict) and value.get("ratio") == 1.0


def nonempty_dict(value):
    return isinstance(value, dict) and bool(value)


def validate_gate_stdout(name, gate, requirement):
    if not isinstance(gate, dict) or gate.get("status") != "passed":
        return
    gate_json = success_json_from_gate_stdout(gate, requirement.get("object"))
    if gate_json is None:
        errors.append(f"{name}_stdout_success_json_missing")
        return
    for key, expected in (requirement.get("exact") or {}).items():
        if gate_json.get(key) != expected:
            errors.append(f"{name}_stdout_{key}_mismatch")
    for field in requirement.get("required_fields") or []:
        if field not in gate_json:
            errors.append(f"{name}_stdout_{field}_missing")
    if gate_json.get("secret_values_printed") is True:
        errors.append(f"{name}_stdout_secret_values_printed_must_not_be_true")


def validate_non_override_gate_stdout():
    if gate_overrides_used:
        return
    for name, requirement in gate_stdout_requirements.items():
        validate_gate_stdout(name, gates_by_name.get(name), requirement)


def validate_production_gate_stdout():
    if not production_ready:
        return
    for name, requirement in gate_stdout_requirements.items():
        gate = gates_by_name.get(name)
        if isinstance(gate, dict) and gate.get("status") == "passed":
            validate_gate_stdout(name, gate, requirement)
        else:
            errors.append(f"production_ready_requires_{name}_stdout_success_json")


def validate_retrieval_quality_gate_stdout():
    gate_json = success_json_from_gate_stdout(
        gates_by_name.get("retrieval_quality"),
        "tonglingyu.rqa_quality_gate",
    )
    if gate_json is None:
        return
    if gate_json.get("quality_gate_passed") is not True:
        errors.append("retrieval_quality_gate_not_passed")
    if gate_json.get("errors") not in ([], None):
        errors.append("retrieval_quality_gate_errors_present")
    if gate_json.get("secret_values_printed") is not False:
        errors.append("retrieval_quality_secret_values_printed_must_be_false")
    if gate_json.get("rqa_schema_version") != "tonglingyu-retrieval-failures-v1":
        errors.append("retrieval_quality_rqa_schema_version_invalid")
    if not nonempty(gate_json.get("eval_suite_version")):
        errors.append("retrieval_quality_eval_suite_version_missing")
    if not nonempty(gate_json.get("eval_run_id")):
        errors.append("retrieval_quality_eval_run_id_missing")
    for field in ("source_snapshot_digest", "kb_build_hash", "eval_report_sha256"):
        if not is_sha256(gate_json.get(field)):
            errors.append(f"retrieval_quality_{field}_invalid")
    thresholds = gate_json.get("effective_thresholds")
    if not isinstance(thresholds, dict):
        errors.append("retrieval_quality_effective_thresholds_missing")
    else:
        expected_thresholds = {
            "quality_report_coverage": 1.0,
            "quality_report_production_ready": 1.0,
            "eval_case_classification": 1.0,
            "expected_evidence_denominator_min": 1,
            "expected_evidence_hit_at_8": 1.0,
            "required_type_coverage": 1.0,
            "exact_term_coverage": 1.0,
            "forbidden_conclusion_avoided": 1.0,
            "reviewer_status_matched": 1.0,
            "open_p0_retrieval_failures": 0,
        }
        for key, expected in expected_thresholds.items():
            if thresholds.get(key) != expected:
                errors.append(f"retrieval_quality_threshold_{key}_mismatch")
    quality = gate_json.get("quality_summary")
    if not isinstance(quality, dict):
        errors.append("retrieval_quality_summary_missing")
    else:
        if quality.get("status") != "passed":
            errors.append("retrieval_quality_summary_status_not_passed")
        if quality.get("blockers") not in ([], None):
            errors.append("retrieval_quality_summary_blockers_present")
        for field in (
            "quality_report_coverage",
            "quality_report_production_ready",
            "eval_case_classification",
            "expected_evidence_hit_at_8",
            "required_type_coverage",
            "exact_term_coverage",
            "forbidden_conclusion_avoided",
            "reviewer_status_matched",
        ):
            if not ratio_is_one(quality.get(field)):
                errors.append(f"retrieval_quality_{field}_below_threshold")
        if quality.get("eval_failure_records") != 0:
            errors.append("retrieval_quality_eval_failure_records_not_zero")
        denominator = quality.get("expected_evidence_denominator")
        if not isinstance(denominator, int) or denominator < 1:
            errors.append("retrieval_quality_expected_evidence_denominator_invalid")
        source_boundary = quality.get("source_coverage_boundary")
        if not isinstance(source_boundary, dict):
            errors.append("retrieval_quality_source_coverage_boundary_missing")
        else:
            if source_boundary.get("source_snapshot_status") != "wikisource_source_snapshot":
                errors.append("retrieval_quality_source_snapshot_status_invalid")
            for field in (
                "facsimile_review_status",
                "authoritative_edition_review_status",
                "expert_collation_status",
            ):
                if source_boundary.get(field) != "not_reviewed":
                    errors.append(f"retrieval_quality_{field}_unexpected")
    kb_version = gate_json.get("kb_version")
    if not nonempty_dict(kb_version):
        errors.append("retrieval_quality_kb_version_missing")
    else:
        for field in ("version_id", "source_count", "block_count", "schema_version", "built_at"):
            if field not in kb_version:
                errors.append(f"retrieval_quality_kb_version_{field}_missing")
    source_license = gate_json.get("source_license_summary")
    if not isinstance(source_license, dict):
        errors.append("retrieval_quality_source_license_summary_missing")
    else:
        if not isinstance(source_license.get("source_count"), int) or source_license.get("source_count") < 1:
            errors.append("retrieval_quality_source_license_source_count_invalid")
        if source_license.get("missing_metadata") not in ([], None):
            errors.append("retrieval_quality_source_license_missing_metadata")
        sources = source_license.get("sources")
        if not isinstance(sources, list) or not sources:
            errors.append("retrieval_quality_source_license_sources_missing")
    if gate_json.get("open_p0_retrieval_failures") != 0:
        errors.append("retrieval_quality_open_p0_retrieval_failures_not_zero")
    if production_ready and gate_json.get("eval_report_generated_by_gate") is not True:
        errors.append("retrieval_quality_eval_report_must_be_generated_by_gate")
    behavior_config = gate_json.get("behavior_config")
    if not isinstance(behavior_config, dict):
        errors.append("retrieval_quality_behavior_config_missing")
    else:
        for field in (
            "agent_runtime_mode_env",
            "runtime_profile_digest",
            "prompt_digest",
            "profile_contract",
            "tool_policy",
            "tool_policy_digest",
            "reviewer_policy",
            "reviewer_policy_digest",
            "gateway_policy_digest",
            "model_upstream_id",
            "model_upstream_bound_by_gate",
            "decoding_parameters_source",
        ):
            if not nonempty(behavior_config.get(field)):
                errors.append(f"retrieval_quality_behavior_config_{field}_missing")
        for field in (
            "runtime_profile_digest",
            "prompt_digest",
            "tool_policy_digest",
            "reviewer_policy_digest",
            "gateway_policy_digest",
        ):
            if not is_sha256(behavior_config.get(field)):
                errors.append(f"retrieval_quality_behavior_config_{field}_invalid")
        if not isinstance(behavior_config.get("decoding_parameters_summary"), dict):
            errors.append("retrieval_quality_behavior_config_decoding_parameters_summary_missing")


if not report_path:
    errors.append("report_path_missing")
    emit("failed")
    raise SystemExit(1)

path = Path(report_path)
if not path.is_file():
    errors.append("report_path_not_found")
    emit("failed")
    raise SystemExit(1)

try:
    report = json.loads(path.read_text(encoding="utf-8"))
except json.JSONDecodeError as exc:
    errors.append(f"report_json_invalid={exc.msg}")
    emit("failed")
    raise SystemExit(1)

add_if(report.get("object") != "tonglingyu.release_readiness_report", "object_invalid")
add_if(report.get("schema_version") != 1, "schema_version_invalid")
add_if(report.get("status") not in allowed_report_statuses, "status_invalid")
add_if(
    not isinstance(report.get("production_release_ready"), bool),
    "production_release_ready_must_be_bool",
)
add_if(
    not isinstance(report.get("release_conditions_met"), bool),
    "release_conditions_met_must_be_bool",
)
add_if(not isinstance(report.get("require_live"), bool), "require_live_must_be_bool")
add_if(not isinstance(report.get("summary_only"), bool), "summary_only_must_be_bool")
add_if(
    not isinstance(report.get("gate_command_overrides_used"), bool),
    "gate_command_overrides_used_must_be_bool",
)
add_if(
    not isinstance(report.get("browser_review_acknowledged"), bool),
    "browser_review_acknowledged_must_be_bool",
)
add_if(
    report.get("exit_policy") not in {"production_release_ready", "summary_only"},
    "exit_policy_invalid",
)
add_if(
    report.get("secret_values_printed") is not False,
    "secret_values_printed_must_be_false",
)
secret_value_hits = secret_value_paths(report)
if secret_value_hits:
    errors.append("secret_like_values_present=" + ",".join(secret_value_hits[:8]))

required_lists = [
    "gates",
    "required_failures",
    "optional_failures",
    "skipped_live_gates",
    "failed_live_gates",
    "release_blockers",
    "remaining_manual_checks",
]
for field in required_lists:
    add_if(not isinstance(report.get(field), list), f"{field}_must_be_array")

gates = report.get("gates") if isinstance(report.get("gates"), list) else []
seen_gate_names = set()
for index, gate in enumerate(gates):
    if not isinstance(gate, dict):
        errors.append(f"gate_{index}_must_be_object")
        continue
    name = gate.get("name")
    if not nonempty(name):
        errors.append(f"gate_{index}_name_missing")
    elif name in seen_gate_names:
        errors.append(f"duplicate_gate_name={name}")
    else:
        seen_gate_names.add(name)
    if nonempty(name) and name not in allowed_gate_names:
        errors.append(f"unexpected_gate_name={name}")
    add_if(gate.get("status") not in allowed_gate_statuses, f"gate_{index}_status_invalid")
    add_if(not isinstance(gate.get("required"), bool), f"gate_{index}_required_must_be_bool")
    if nonempty(name):
        validate_gate_tail(name, gate, "stdout_tail")
        validate_gate_tail(name, gate, "stderr_tail")

gates_by_name = {
    gate.get("name"): gate
    for gate in gates
    if isinstance(gate, dict) and isinstance(gate.get("name"), str)
}
for name in required_gate_names:
    add_if(name not in gates_by_name, f"missing_gate={name}")

required_failures = report.get("required_failures") if isinstance(report.get("required_failures"), list) else []
optional_failures = report.get("optional_failures") if isinstance(report.get("optional_failures"), list) else []
skipped_live_gates = report.get("skipped_live_gates") if isinstance(report.get("skipped_live_gates"), list) else []
failed_live_gates = report.get("failed_live_gates") if isinstance(report.get("failed_live_gates"), list) else []
release_blockers = report.get("release_blockers") if isinstance(report.get("release_blockers"), list) else []
manual_checks = report.get("remaining_manual_checks") if isinstance(report.get("remaining_manual_checks"), list) else []

production_ready = report.get("production_release_ready") is True
release_conditions_met = report.get("release_conditions_met") is True
browser_review_acknowledged = report.get("browser_review_acknowledged") is True
gate_overrides_used = report.get("gate_command_overrides_used") is True
require_live = report.get("require_live") is True
summary_only = report.get("summary_only") is True
exit_policy = report.get("exit_policy")
browser_review_ref = report.get("browser_review_ref")
browser_review_evidence = report.get("browser_review_evidence")
browser_review_validation = report.get("browser_review_validation")
generated_at = report.get("generated_at")

generated_at_dt = None
if not nonempty(generated_at):
    errors.append("generated_at_missing")
else:
    generated_at_dt = parse_timestamp(generated_at)
    if generated_at_dt is None:
        errors.append("generated_at_must_be_iso8601_with_timezone")
    else:
        now = datetime.now(timezone.utc)
        future_skew_seconds = (generated_at_dt - now).total_seconds()
        if future_skew_seconds > 300:
            errors.append("generated_at_must_not_be_in_future")

computed_required_failures = [
    gate["name"]
    for gate in gates
    if isinstance(gate, dict)
    and gate.get("required") is True
    and gate.get("status") != "passed"
]
computed_optional_failures = [
    gate["name"]
    for gate in gates
    if isinstance(gate, dict)
    and gate.get("required") is False
    and gate.get("status") == "failed"
]
computed_skipped_gate_names = [
    gate["name"]
    for gate in gates
    if isinstance(gate, dict)
    and isinstance(gate.get("name"), str)
    and gate.get("status") == "skipped"
]
computed_skipped_live_gates = [
    name
    for name in live_gate_names
    if (gates_by_name.get(name) or {}).get("status") == "skipped"
]
computed_failed_live_gates = [
    name
    for name in live_gate_names
    if (gates_by_name.get(name) or {}).get("status") == "failed"
]
browser_gate = gates_by_name.get("openwebui_browser_review")
browser_gate_validation = browser_validation_from_gate(browser_gate)
browser_gate_passed = (
    isinstance(browser_gate, dict)
    and browser_gate.get("name") == "openwebui_browser_review"
    and browser_gate.get("status") == "passed"
)
browser_validation_present = isinstance(browser_review_validation, dict)
computed_browser_review_acknowledged = browser_gate_passed and browser_validation_present
browser_validation_missing = browser_gate_passed and not browser_validation_present
if browser_validation_missing:
    if isinstance(browser_gate, dict) and browser_gate.get("required") is True:
        computed_required_failures.append("openwebui_browser_review_validation")
    else:
        computed_optional_failures.append("openwebui_browser_review_validation")

computed_status = "failed" if computed_required_failures else "passed"
if computed_status == "passed" and computed_optional_failures:
    computed_status = "passed_with_failed_optional_gates"
elif computed_status == "passed" and computed_skipped_gate_names:
    computed_status = "passed_with_skipped_gates"
elif computed_status == "passed" and gate_overrides_used:
    computed_status = "passed_with_gate_command_overrides"
elif computed_status == "passed" and summary_only:
    computed_status = "passed_in_summary_only_mode"

computed_manual_checks = [] if computed_browser_review_acknowledged else manual_browser_checks
computed_release_blockers = []
if not require_live:
    computed_release_blockers.append("live release mode was not required")
for name in computed_required_failures:
    computed_release_blockers.append(f"required gate did not pass: {name}")
for name in computed_skipped_live_gates:
    computed_release_blockers.append(f"live gate was skipped: {name}")
for name in computed_failed_live_gates:
    if name not in computed_required_failures:
        computed_release_blockers.append(f"live gate failed: {name}")
if browser_validation_missing:
    computed_release_blockers.append("Open WebUI browser-side review validation summary was missing")
if not computed_browser_review_acknowledged:
    computed_release_blockers.append("Open WebUI browser-side review was not acknowledged")
if summary_only:
    computed_release_blockers.append("summary-only mode was used")
computed_release_conditions_met = (
    require_live
    and not computed_required_failures
    and not computed_skipped_live_gates
    and computed_browser_review_acknowledged
)
if gate_overrides_used:
    computed_release_blockers.append("gate command overrides were used")
computed_production_ready = (
    computed_release_conditions_met and not gate_overrides_used and not summary_only
)
computed_exit_policy = "summary_only" if summary_only else "production_release_ready"

add_mismatch("required_failures", computed_required_failures, required_failures)
add_mismatch("optional_failures", computed_optional_failures, optional_failures)
add_mismatch("skipped_live_gates", computed_skipped_live_gates, skipped_live_gates)
add_mismatch("failed_live_gates", computed_failed_live_gates, failed_live_gates)
add_mismatch("status", computed_status, report.get("status"))
add_mismatch(
    "browser_review_acknowledged",
    computed_browser_review_acknowledged,
    browser_review_acknowledged,
)
add_mismatch("remaining_manual_checks", computed_manual_checks, manual_checks)
add_mismatch("release_blockers", computed_release_blockers, release_blockers)
add_mismatch(
    "release_conditions_met",
    computed_release_conditions_met,
    release_conditions_met,
)
add_mismatch("production_release_ready", computed_production_ready, production_ready)
add_mismatch("exit_policy", computed_exit_policy, exit_policy)
validate_non_override_gate_stdout()
validate_production_gate_stdout()
validate_retrieval_quality_gate_stdout()

add_if(
    production_ready and not release_conditions_met,
    "production_ready_requires_release_conditions_met",
)
add_if(production_ready and not require_live, "production_ready_requires_live")
add_if(production_ready and summary_only, "production_ready_forbids_summary_only")
if production_ready:
    try:
        report_max_age_hours = float(report_max_age_hours_raw)
    except ValueError:
        report_max_age_hours = -1.0
    if report_max_age_hours <= 0:
        errors.append("release_report_max_age_hours_must_be_positive")
    elif generated_at_dt is not None:
        age_seconds = (datetime.now(timezone.utc) - generated_at_dt).total_seconds()
        if age_seconds > report_max_age_hours * 3600:
            errors.append("production_ready_report_too_old")
add_if(
    production_ready and report.get("status") != "passed",
    "production_ready_requires_passed_status",
)
add_if(production_ready and gate_overrides_used, "production_ready_forbids_gate_overrides")
add_if(production_ready and required_failures, "production_ready_forbids_required_failures")
add_if(production_ready and skipped_live_gates, "production_ready_forbids_skipped_live_gates")
add_if(production_ready and failed_live_gates, "production_ready_forbids_failed_live_gates")
add_if(production_ready and release_blockers, "production_ready_forbids_release_blockers")
add_if(production_ready and manual_checks, "production_ready_forbids_remaining_manual_checks")
add_if(production_ready and not browser_review_acknowledged, "production_ready_requires_browser_review")
add_if(
    production_ready and not isinstance(browser_review_validation, dict),
    "production_ready_requires_browser_review_validation",
)
add_if(
    production_ready and not nonempty(browser_review_ref),
    "production_ready_requires_browser_review_ref",
)
add_if(
    production_ready and not nonempty(browser_review_evidence),
    "production_ready_requires_browser_review_evidence",
)

for name in live_gate_names:
    gate = gates_by_name.get(name)
    add_if(production_ready and not isinstance(gate, dict), f"production_ready_missing_{name}")
    if isinstance(gate, dict):
        add_if(
            production_ready and gate.get("status") != "passed",
            f"production_ready_requires_{name}_passed",
        )
        add_if(
            production_ready and gate.get("required") is not True,
            f"production_ready_requires_{name}_required",
        )

if production_ready:
    add_if(not isinstance(browser_gate, dict), "production_ready_missing_openwebui_browser_review")
    if isinstance(browser_gate, dict):
        add_if(
            browser_gate.get("status") != "passed",
            "production_ready_requires_openwebui_browser_review_passed",
        )
        add_if(
            browser_gate.get("required") is not True,
            "production_ready_requires_openwebui_browser_review_required",
        )

if browser_review_acknowledged and not isinstance(browser_review_validation, dict):
    errors.append("browser_review_ack_requires_validation")

if isinstance(browser_review_validation, dict):
    if browser_gate_validation is None:
        errors.append("browser_review_validation_stdout_missing")
    elif browser_review_validation != browser_gate_validation:
        errors.append("browser_review_validation_stdout_mismatch")
    validation_errors = browser_review_validation.get("errors")
    evidence_sha256 = browser_review_validation.get("evidence_sha256")
    validation_reviewed_at = browser_review_validation.get("reviewed_at")
    validation_reviewer = browser_review_validation.get("reviewer")
    validation_public_webui_url = browser_review_validation.get("public_webui_url")
    checked_items = browser_review_validation.get("checked_items")
    validated_evidence_refs = browser_review_validation.get("validated_evidence_refs")
    add_if(
        browser_review_validation.get("object") != "tonglingyu.openwebui_browser_review_gate",
        "browser_review_validation_object_invalid",
    )
    add_if(
        browser_review_validation.get("status") != "ok",
        "browser_review_validation_status_invalid",
    )
    add_if(
        not isinstance(validation_errors, list),
        "browser_review_validation_errors_must_be_array",
    )
    add_if(
        isinstance(validation_errors, list) and bool(validation_errors),
        "browser_review_validation_errors_present",
    )
    add_if(
        browser_review_validation.get("secret_values_printed") is not False,
        "browser_review_validation_secret_values_printed_must_be_false",
    )
    add_if(
        production_ready
        and browser_review_validation.get("expected_review_ref_bound") is not True,
        "production_ready_requires_browser_review_ref_bound",
    )
    add_if(
        production_ready
        and browser_review_validation.get("expected_public_url_bound") is not True,
        "production_ready_requires_browser_review_public_url_bound",
    )
    add_if(
        not nonempty(browser_review_ref),
        "browser_review_validation_requires_review_ref",
    )
    add_if(
        not nonempty(browser_review_evidence),
        "browser_review_validation_requires_evidence",
    )
    add_if(
        nonempty(browser_review_ref)
        and browser_review_validation.get("review_ref") != browser_review_ref,
        "browser_review_validation_review_ref_mismatch",
    )
    add_if(
        nonempty(browser_review_evidence)
        and browser_review_validation.get("evidence_path") != browser_review_evidence,
        "browser_review_validation_evidence_path_mismatch",
    )
    add_if(
        nonempty(browser_review_evidence)
        and not Path(browser_review_evidence).is_absolute(),
        "browser_review_evidence_path_must_be_absolute",
    )
    add_if(
        nonempty(browser_review_validation.get("evidence_path"))
        and not Path(browser_review_validation.get("evidence_path")).is_absolute(),
        "browser_review_validation_evidence_path_must_be_absolute",
    )
    add_if(
        not nonempty(validation_reviewer),
        "browser_review_validation_reviewer_missing",
    )
    if not nonempty(validation_reviewed_at):
        errors.append("browser_review_validation_reviewed_at_missing")
    else:
        validation_reviewed_at_dt = parse_timestamp(validation_reviewed_at)
        if validation_reviewed_at_dt is None:
            errors.append("browser_review_validation_reviewed_at_invalid")
        elif generated_at_dt is not None:
            future_skew_seconds = (
                validation_reviewed_at_dt - generated_at_dt
            ).total_seconds()
            if future_skew_seconds > 300:
                errors.append("browser_review_validation_reviewed_at_after_report")
    if not nonempty(validation_public_webui_url):
        errors.append("browser_review_validation_public_webui_url_missing")
    else:
        validation_public_url = urlparse(validation_public_webui_url.strip())
        if validation_public_url.scheme != "https" or not validation_public_url.netloc:
            errors.append("browser_review_validation_public_webui_url_invalid")
    add_if(
        not is_sha256(evidence_sha256),
        "browser_review_validation_evidence_sha256_invalid",
    )
    resolved_evidence_path = None
    if nonempty(browser_review_evidence):
        resolved_evidence_path = resolve_artifact_path(browser_review_evidence)
        if not resolved_evidence_path.is_file():
            errors.append("browser_review_evidence_file_not_found")
        elif is_sha256(evidence_sha256):
            actual_evidence_sha256 = file_sha256(resolved_evidence_path)
            add_if(
                actual_evidence_sha256 != evidence_sha256,
                "browser_review_evidence_sha256_mismatch",
            )
    add_if(
        not isinstance(checked_items, list),
        "browser_review_validation_checked_items_must_be_array",
    )
    if isinstance(checked_items, list):
        seen_checked_items = set()
        for item in checked_items:
            if item not in required_browser_review_item_set:
                errors.append(f"browser_review_validation_unexpected_checked_item={item}")
            elif item in seen_checked_items:
                errors.append(f"browser_review_validation_duplicate_checked_item={item}")
            else:
                seen_checked_items.add(item)
        for item in required_browser_review_items:
            add_if(
                item not in seen_checked_items,
                f"browser_review_validation_missing_checked_item={item}",
            )
    add_if(
        not isinstance(validated_evidence_refs, list),
        "browser_review_validation_refs_must_be_array",
    )
    if isinstance(validated_evidence_refs, list):
        seen_ref_checks = set()
        for index, ref_record in enumerate(validated_evidence_refs):
            if not isinstance(ref_record, dict):
                errors.append(f"browser_review_validation_ref_{index}_must_be_object")
                continue
            check_name = ref_record.get("check")
            kind = ref_record.get("kind")
            ref = ref_record.get("ref")
            if check_name not in required_browser_review_items:
                errors.append(f"browser_review_validation_ref_{index}_check_invalid")
            elif check_name in seen_ref_checks:
                errors.append(f"browser_review_validation_ref_duplicate={check_name}")
            else:
                seen_ref_checks.add(check_name)
            allowed_kinds = browser_review_allowed_ref_kinds.get(check_name, set())
            if kind not in allowed_kinds:
                errors.append(f"browser_review_validation_ref_{index}_kind_invalid")
            if not nonempty(ref):
                errors.append(f"browser_review_validation_ref_{index}_ref_missing")
            elif any(char in ref for char in "\r\n\t"):
                errors.append(f"browser_review_validation_ref_{index}_ref_contains_control_char")
            sha256 = ref_record.get("sha256")
            if kind == "local_file":
                add_if(
                    not is_sha256(sha256),
                    f"browser_review_validation_ref_{index}_sha256_invalid",
                )
                resolved_ref_path = (
                    resolve_browser_evidence_ref(ref, resolved_evidence_path)
                    if nonempty(ref) and resolved_evidence_path is not None
                    else None
                )
                if resolved_ref_path is None:
                    errors.append(f"browser_review_validation_ref_{index}_file_path_invalid")
                elif not resolved_ref_path.is_file():
                    errors.append(f"browser_review_validation_ref_{index}_file_not_found")
                elif is_sha256(sha256):
                    actual_ref_sha256 = file_sha256(resolved_ref_path)
                    add_if(
                        actual_ref_sha256 != sha256,
                        f"browser_review_validation_ref_{index}_sha256_mismatch",
                    )
            elif sha256 is not None:
                add_if(
                    not is_sha256(sha256),
                    f"browser_review_validation_ref_{index}_sha256_invalid",
                )
        for item in required_browser_review_items:
            add_if(
                item not in seen_ref_checks,
                f"browser_review_validation_missing_ref={item}",
            )
elif browser_review_validation is not None:
    errors.append("browser_review_validation_must_be_object")

emit("ok" if not errors else "failed")
if errors:
    raise SystemExit(1)
PY
