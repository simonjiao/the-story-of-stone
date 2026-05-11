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

add_if(
    production_ready and not release_conditions_met,
    "production_ready_requires_release_conditions_met",
)
add_if(production_ready and not require_live, "production_ready_requires_live")
add_if(production_ready and gate_overrides_used, "production_ready_forbids_gate_overrides")
add_if(production_ready and required_failures, "production_ready_forbids_required_failures")
add_if(production_ready and skipped_live_gates, "production_ready_forbids_skipped_live_gates")
add_if(production_ready and failed_live_gates, "production_ready_forbids_failed_live_gates")
add_if(production_ready and release_blockers, "production_ready_forbids_release_blockers")
add_if(production_ready and manual_checks, "production_ready_forbids_remaining_manual_checks")
add_if(production_ready and not browser_review_acknowledged, "production_ready_requires_browser_review")
add_if(
    production_ready and not isinstance(report.get("browser_review_validation"), dict),
    "production_ready_requires_browser_review_validation",
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

emit("ok" if not errors else "failed")
if errors:
    raise SystemExit(1)
PY
