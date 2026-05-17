#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
FUNCTION_DIR="${DEPLOY_DIR}/open-webui/functions"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

ACTION_FILE="${FUNCTION_DIR}/tonglingyu_gateway_admin_action.py"
ACTION_TEST="${FUNCTION_DIR}/test_tonglingyu_gateway_admin_action.py"
FEEDBACK_ACTION_FILE="${FUNCTION_DIR}/tonglingyu_gateway_feedback_action.py"
FEEDBACK_ACTION_TEST="${FUNCTION_DIR}/test_tonglingyu_gateway_feedback_action.py"
VERIFY_SCRIPT="${SCRIPT_DIR}/verify-openwebui-gateway-admin-action.sh"
CONTRACT_VERSION="tonglingyu-openwebui-admin-action-contract-v1"
OK_FIXTURE="${WORK_DIR}/gateway-admin-action-ok.json"
EMPTY_KEY_FIXTURE="${WORK_DIR}/gateway-admin-action-empty-key.json"
MISSING_GUARD_FIXTURE="${WORK_DIR}/gateway-admin-action-missing-guard.json"
OK_OUT="${WORK_DIR}/ok.out"
EMPTY_OUT="${WORK_DIR}/empty.out"
MISSING_GUARD_OUT="${WORK_DIR}/missing-guard.out"

python3 -m py_compile \
  "${ACTION_FILE}" \
  "${ACTION_TEST}" \
  "${FEEDBACK_ACTION_FILE}" \
  "${FEEDBACK_ACTION_TEST}"
python3 -m unittest "${ACTION_TEST}" "${FEEDBACK_ACTION_TEST}"

python3 - "${ACTION_FILE}" "${OK_FIXTURE}" "${EMPTY_KEY_FIXTURE}" "${MISSING_GUARD_FIXTURE}" <<'PY'
import json
import sys
from pathlib import Path

action_file, ok_path, empty_key_path, missing_guard_path = sys.argv[1:5]
content = Path(action_file).read_text(encoding="utf-8")
base = {
    "id": "tonglingyu_gateway_admin",
    "type": "action",
    "is_active": True,
    "is_global": True,
    "content": content,
    "valves": {
        "GATEWAY_BASE_URL": "http://tonglingyu-gateway:8090",
        "GATEWAY_ADMIN_API_KEY": "admin-key-fixture-secret",
        "TARGET_MODEL": "tonglingyu",
        "TARGET_MODELS": "tonglingyu",
    },
}
Path(ok_path).write_text(json.dumps(base), encoding="utf-8")

empty_key = dict(base)
empty_key["valves"] = dict(base["valves"])
empty_key["valves"]["GATEWAY_ADMIN_API_KEY"] = ""
Path(empty_key_path).write_text(json.dumps(empty_key), encoding="utf-8")

missing_guard = dict(base)
missing_guard["content"] = "class Action:\n    async def action(self, body):\n        return body\n"
Path(missing_guard_path).write_text(json.dumps(missing_guard), encoding="utf-8")
PY

OPEN_WEBUI_GATEWAY_ADMIN_ACTION_VERIFY_JSON="${OK_FIXTURE}" \
  "${VERIFY_SCRIPT}" >"${OK_OUT}"
if grep -q "admin-key-fixture-secret" "${OK_OUT}"; then
  echo "verify output leaked Gateway admin key value" >&2
  exit 1
fi
python3 - "${OK_OUT}" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["status"] == "ok", report
assert report["source"] == "fixture-json", report
assert "GATEWAY_ADMIN_API_KEY" in report["valve_keys"], report
assert report["errors"] == [], report
PY

if OPEN_WEBUI_GATEWAY_ADMIN_ACTION_VERIFY_JSON="${EMPTY_KEY_FIXTURE}" \
  "${VERIFY_SCRIPT}" >"${EMPTY_OUT}" 2>"${WORK_DIR}/empty.err"; then
  echo "empty Gateway admin key fixture unexpectedly passed" >&2
  exit 1
fi
python3 - "${EMPTY_OUT}" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["status"] == "failed", report
assert "empty_valves=GATEWAY_ADMIN_API_KEY" in report["errors"], report
PY

if OPEN_WEBUI_GATEWAY_ADMIN_ACTION_VERIFY_JSON="${MISSING_GUARD_FIXTURE}" \
  "${VERIFY_SCRIPT}" >"${MISSING_GUARD_OUT}" 2>"${WORK_DIR}/missing-guard.err"; then
  echo "missing admin role guard fixture unexpectedly passed" >&2
  exit 1
fi
python3 - "${MISSING_GUARD_OUT}" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["status"] == "failed", report
assert "content_missing_admin_role_guard" in report["errors"], report
assert "content_missing_admin_actions" in report["errors"], report
PY

python3 - \
  "${ACTION_FILE}" \
  "${ACTION_TEST}" \
  "${FEEDBACK_ACTION_FILE}" \
  "${FEEDBACK_ACTION_TEST}" \
  "${OK_OUT}" \
  "${EMPTY_OUT}" \
  "${MISSING_GUARD_OUT}" \
  "${CONTRACT_VERSION}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    action_file,
    action_test,
    feedback_action_file,
    feedback_action_test,
    ok_out,
    empty_out,
    missing_guard_out,
    contract_version,
) = sys.argv[1:9]


def sha256_file(path: str) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


ok_report = json.loads(Path(ok_out).read_text(encoding="utf-8"))
empty_report = json.loads(Path(empty_out).read_text(encoding="utf-8"))
missing_guard_report = json.loads(Path(missing_guard_out).read_text(encoding="utf-8"))
action_test_content = Path(action_test).read_text(encoding="utf-8")
required_actions = [
    "metrics",
    "trace",
    "package",
    "session",
    "retrieval_failures",
    "retrieval_failure",
    "retrieval_failure_update",
    "retrieval_failure_cluster",
    "governance_tasks",
    "governance_task",
    "governance_task_create",
    "governance_task_from_failure",
    "governance_task_update",
    "knowledge_items",
    "knowledge_item",
    "knowledge_item_review",
    "knowledge_patch_proposal",
]
checks = {
    "py_compile_passed": True,
    "unit_tests_passed": True,
    "valid_fixture_passed": ok_report.get("status") == "ok",
    "admin_key_not_printed": True,
    "empty_admin_key_rejected": "empty_valves=GATEWAY_ADMIN_API_KEY"
    in empty_report.get("errors", []),
    "admin_role_guard_required": "content_missing_admin_role_guard"
    in missing_guard_report.get("errors", []),
    "admin_actions_required": "content_missing_admin_actions"
    in missing_guard_report.get("errors", []),
    "required_valves_present": set(ok_report.get("valve_keys", []))
    >= {"GATEWAY_BASE_URL", "GATEWAY_ADMIN_API_KEY", "TARGET_MODEL", "TARGET_MODELS"},
    "rqa_list_response_contract_tested": all(
        token in action_test_content
        for token in (
            "tonglingyu-retrieval-failures-v1",
            "tonglingyu-knowledge-governance-tasks-v2",
            "tonglingyu-knowledge-item-states-v1",
            "tonglingyu.knowledge_item_admin_review",
            '"next_offset": 20',
        )
    ),
}
errors = [f"check_failed={name}" for name, passed in checks.items() if passed is not True]
payload = {
    "object": "tonglingyu.openwebui_admin_action_contract_gate",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "contract_version": contract_version,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "checks": checks,
    "action": {
        "function_id": "tonglingyu_gateway_admin",
        "type": "action",
        "source_sha256": sha256_file(action_file),
        "test_sha256": sha256_file(action_test),
        "required_actions": required_actions,
    },
    "feedback_action": {
        "function_id": "tonglingyu_gateway_feedback",
        "type": "action",
        "source_sha256": sha256_file(feedback_action_file),
        "test_sha256": sha256_file(feedback_action_test),
    },
    "fixture_validation": {
        "source": ok_report.get("source"),
        "valve_keys": ok_report.get("valve_keys", []),
        "negative_fixture_count": 2,
    },
    "errors": errors,
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
if errors:
    raise SystemExit(1)
PY
