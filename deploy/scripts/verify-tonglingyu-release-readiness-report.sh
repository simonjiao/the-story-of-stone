#!/usr/bin/env bash
set -euo pipefail

REPORT_PATH="${1:-${TONGLINGYU_RELEASE_REPORT_PATH:-}}"

python3 - "${REPORT_PATH}" <<'PY'
import json
import sys
from pathlib import Path

report_path = sys.argv[1].strip()
errors = []
report = {}
live_gate_names = [
    "model_upstream_network",
    "strict_gateway",
    "openwebui_function",
    "openwebui_admin_action",
]
required_browser_review_items = [
    "ordinary_user_model_visibility",
    "streaming_chat_ux",
    "admin_audit_visibility",
    "persisted_provider_settings",
]
allowed_report_statuses = {
    "failed",
    "passed",
    "passed_with_failed_optional_gates",
    "passed_with_gate_command_overrides",
    "passed_with_skipped_gates",
    "passed_in_summary_only_mode",
}
allowed_gate_statuses = {"passed", "failed", "skipped"}
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


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


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
    add_if(gate.get("status") not in allowed_gate_statuses, f"gate_{index}_status_invalid")
    add_if(not isinstance(gate.get("required"), bool), f"gate_{index}_required_must_be_bool")

gates_by_name = {
    gate.get("name"): gate
    for gate in gates
    if isinstance(gate, dict) and isinstance(gate.get("name"), str)
}

required_failures = report.get("required_failures") if isinstance(report.get("required_failures"), list) else []
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
browser_review_ref = report.get("browser_review_ref")
browser_review_evidence = report.get("browser_review_evidence")
browser_review_validation = report.get("browser_review_validation")

add_if(
    production_ready and not release_conditions_met,
    "production_ready_requires_release_conditions_met",
)
add_if(production_ready and not require_live, "production_ready_requires_live")
add_if(production_ready and summary_only, "production_ready_forbids_summary_only")
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

browser_gate = gates_by_name.get("openwebui_browser_review")
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
    validation_errors = browser_review_validation.get("errors")
    evidence_sha256 = browser_review_validation.get("evidence_sha256")
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
        not (
            isinstance(evidence_sha256, str)
            and len(evidence_sha256) == 64
            and all(char in hex_digits for char in evidence_sha256.lower())
        ),
        "browser_review_validation_evidence_sha256_invalid",
    )
    add_if(
        not isinstance(checked_items, list),
        "browser_review_validation_checked_items_must_be_array",
    )
    if isinstance(checked_items, list):
        for item in required_browser_review_items:
            add_if(
                item not in checked_items,
                f"browser_review_validation_missing_checked_item={item}",
            )
    add_if(
        not isinstance(validated_evidence_refs, list),
        "browser_review_validation_refs_must_be_array",
    )
elif browser_review_validation is not None:
    errors.append("browser_review_validation_must_be_object")

emit("ok" if not errors else "failed")
if errors:
    raise SystemExit(1)
PY
