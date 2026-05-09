#!/usr/bin/env bash
set -euo pipefail

MANAGER_URL="${AGENT_MANAGER_URL:-http://127.0.0.1:8088}"
ADMIN_USER="${AGENTCTL_USER:-external-action-contract-admin}"
ADMIN_SERVICE="${AGENTCTL_SERVICE:-external-action-contract-smoke}"
ADMIN_ROLES="${AGENTCTL_ROLES:-system_admin}"
VIEWER_USER="${EXTERNAL_ACTION_VIEWER_USER:-external-action-contract-viewer}"

required_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 2
  fi
}

required_env EXTERNAL_ACTION_CONNECTOR
required_env EXTERNAL_ACTION_NAME
required_env EXTERNAL_ACTION_RESOURCE_REF
required_env EXTERNAL_ACTION_CREDENTIAL_SCOPE

EXTERNAL_ACTION_RISK_LEVEL="${EXTERNAL_ACTION_RISK_LEVEL:-low}"
EXTERNAL_ACTION_MODE="${EXTERNAL_ACTION_MODE:-authorized}"
if [[ -z "${EXTERNAL_ACTION_PAYLOAD_JSON:-}" ]]; then
  EXTERNAL_ACTION_PAYLOAD_JSON="{}"
fi
if [[ -z "${EXTERNAL_ACTION_COMPENSATE_PAYLOAD_JSON:-}" ]]; then
  EXTERNAL_ACTION_COMPENSATE_PAYLOAD_JSON="{}"
fi
EXTERNAL_ACTION_COMPENSATE_REASON="${EXTERNAL_ACTION_COMPENSATE_REASON:-external action contract smoke compensation}"
SMOKE_KEY="external-action-contract-$(date +%s)"
export EXTERNAL_ACTION_CONNECTOR
export EXTERNAL_ACTION_NAME
export EXTERNAL_ACTION_RESOURCE_REF
export EXTERNAL_ACTION_CREDENTIAL_SCOPE
export EXTERNAL_ACTION_RISK_LEVEL
export EXTERNAL_ACTION_MODE
export EXTERNAL_ACTION_PAYLOAD_JSON
export EXTERNAL_ACTION_COMPENSATE_PAYLOAD_JSON
export EXTERNAL_ACTION_COMPENSATE_REASON
export SMOKE_KEY

json_get() {
  local path="$1"
  python3 -c '
import json, sys
value = json.load(sys.stdin)
for part in sys.argv[1].split("."):
    value = value[part]
print(value)
' "$path"
}

manager_post_as() {
  local user="$1"
  local roles="$2"
  local path="$3"
  local body="$4"
  curl -fsS \
    -H "content-type: application/json" \
    -H "x-agent-user: ${user}" \
    -H "x-agent-service: ${ADMIN_SERVICE}" \
    -H "x-agent-roles: ${roles}" \
    -X POST \
    -d "$body" \
    "${MANAGER_URL}${path}"
}

manager_post() {
  manager_post_as "${ADMIN_USER}" "${ADMIN_ROLES}" "$1" "$2"
}

viewer_post() {
  manager_post_as "${VIEWER_USER}" "viewer" "$1" "$2"
}

CREATE_AGENT_BODY="$(python3 - <<'PY'
import json, os
print(json.dumps({
    "request_type": "create_agent",
    "agent_type": "background_worker",
    "target_resource": os.environ["EXTERNAL_ACTION_RESOURCE_REF"],
    "intent_text": "external action contract smoke agent",
    "structured_payload": {"purpose": "external-action-contract-smoke"},
    "idempotency_key": os.environ["SMOKE_KEY"] + "-agent",
    "risk_level": os.environ["EXTERNAL_ACTION_RISK_LEVEL"],
    "external_action_mode": "read_only",
}))
PY
)"
CREATE_AGENT_RESPONSE="$(manager_post "/v1/agent-requests" "${CREATE_AGENT_BODY}")"
AGENT_ID="$(printf '%s' "${CREATE_AGENT_RESPONSE}" | json_get "agent_id")"

RUN_BODY="$(python3 - <<'PY'
import json, os
print(json.dumps({
    "trigger_type": "manual",
    "idempotency_key": os.environ["SMOKE_KEY"] + "-run",
    "target_resource": os.environ["EXTERNAL_ACTION_RESOURCE_REF"],
    "risk_level": os.environ["EXTERNAL_ACTION_RISK_LEVEL"],
    "external_action_mode": "read_only",
}))
PY
)"
RUN_RESPONSE="$(manager_post "/v1/my-agents/${AGENT_ID}/runs" "${RUN_BODY}")"
RUN_ID="$(printf '%s' "${RUN_RESPONSE}" | json_get "id")"

APPROVAL_BODY="$(python3 - <<'PY'
import json, os
print(json.dumps({
    "request_type": "change_agent",
    "agent_type": "background_worker",
    "target_resource": os.environ["EXTERNAL_ACTION_RESOURCE_REF"],
    "intent_text": "Approve external action contract smoke",
    "structured_payload": {"purpose": "external-action-contract-approval"},
    "idempotency_key": os.environ["SMOKE_KEY"] + "-approval",
    "risk_level": os.environ["EXTERNAL_ACTION_RISK_LEVEL"],
    "external_action_mode": "approval_required",
}))
PY
)"
APPROVAL_RESPONSE="$(viewer_post "/v1/agent-requests" "${APPROVAL_BODY}")"
APPROVAL_REQUEST_ID="$(printf '%s' "${APPROVAL_RESPONSE}" | json_get "request_id")"
APPROVAL_ID="$(printf '%s' "${APPROVAL_RESPONSE}" | json_get "approval_id")"
export APPROVAL_ID

manager_post "/v1/admin/requests/${APPROVAL_REQUEST_ID}/approve" \
  '{"reason":"external action contract smoke approval"}' >/dev/null

DRY_RUN_BODY="$(python3 - <<'PY'
import json, os
print(json.dumps({
    "connector": os.environ["EXTERNAL_ACTION_CONNECTOR"],
    "action": os.environ["EXTERNAL_ACTION_NAME"],
    "resource_ref": os.environ["EXTERNAL_ACTION_RESOURCE_REF"],
    "credential_scope": os.environ["EXTERNAL_ACTION_CREDENTIAL_SCOPE"],
    "approval_id": os.environ["APPROVAL_ID"],
    "input_summary": "external action contract smoke",
    "risk_level": os.environ["EXTERNAL_ACTION_RISK_LEVEL"],
    "external_action_mode": os.environ["EXTERNAL_ACTION_MODE"],
}))
PY
)"
DRY_RUN_RESPONSE="$(manager_post "/v1/admin/runs/${RUN_ID}/external-action-plans/dry-run" "${DRY_RUN_BODY}")"
PLAN_ID="$(printf '%s' "${DRY_RUN_RESPONSE}" | json_get "plan.id")"
DRY_RUN_STATUS="$(printf '%s' "${DRY_RUN_RESPONSE}" | json_get "dry_run_status")"
if [[ "${DRY_RUN_STATUS}" != "dry_run_ready" ]]; then
  echo "unexpected dry_run_status=${DRY_RUN_STATUS}" >&2
  exit 1
fi

APPLY_BODY="$(python3 - <<'PY'
import json, os, sys
payload = json.loads(os.environ["EXTERNAL_ACTION_PAYLOAD_JSON"])
print(json.dumps({"payload": payload}))
PY
)"
APPLY_RESPONSE="$(manager_post "/v1/admin/runs/${RUN_ID}/external-action-plans/${PLAN_ID}/apply" "${APPLY_BODY}")"
APPLY_STATUS="$(printf '%s' "${APPLY_RESPONSE}" | json_get "apply_status")"
RESULT_REF="$(printf '%s' "${APPLY_RESPONSE}" | json_get "plan.result_ref")"
COMPENSATION_REF="$(printf '%s' "${APPLY_RESPONSE}" | json_get "plan.compensation_ref")"
if [[ "${APPLY_STATUS}" != "applied" ]]; then
  echo "unexpected apply_status=${APPLY_STATUS}" >&2
  exit 1
fi
if [[ -z "${RESULT_REF}" || -z "${COMPENSATION_REF}" ]]; then
  echo "apply response is missing result_ref or compensation_ref" >&2
  exit 1
fi

COMPENSATE_BODY="$(python3 - <<'PY'
import json, os
payload = json.loads(os.environ["EXTERNAL_ACTION_COMPENSATE_PAYLOAD_JSON"])
print(json.dumps({
    "reason": os.environ["EXTERNAL_ACTION_COMPENSATE_REASON"],
    "payload": payload,
}))
PY
)"
COMPENSATE_RESPONSE="$(manager_post "/v1/admin/runs/${RUN_ID}/external-action-plans/${PLAN_ID}/compensate" "${COMPENSATE_BODY}")"
COMPENSATE_STATUS="$(printf '%s' "${COMPENSATE_RESPONSE}" | json_get "compensate_status")"
COMPENSATION_RESULT_REF="$(printf '%s' "${COMPENSATE_RESPONSE}" | json_get "compensation_result_ref")"
if [[ "${COMPENSATE_STATUS}" != "compensated" ]]; then
  echo "unexpected compensate_status=${COMPENSATE_STATUS}" >&2
  exit 1
fi
if [[ -z "${COMPENSATION_RESULT_REF}" ]]; then
  echo "compensate response is missing compensation_result_ref" >&2
  exit 1
fi

echo "external action contract smoke passed"
echo "manager_url=${MANAGER_URL}"
echo "run_id=${RUN_ID}"
echo "plan_id=${PLAN_ID}"
echo "dry_run_status=${DRY_RUN_STATUS}"
echo "apply_status=${APPLY_STATUS}"
echo "result_ref=${RESULT_REF}"
echo "compensation_ref=${COMPENSATION_REF}"
echo "compensate_status=${COMPENSATE_STATUS}"
echo "compensation_result_ref=${COMPENSATION_RESULT_REF}"
