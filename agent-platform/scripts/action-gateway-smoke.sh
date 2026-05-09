#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="${CARGO:-cargo}"
SMOKE_DIR="${TMPDIR:-/tmp}/agent-platform-action-gateway-smoke-$$"
mkdir -p "$SMOKE_DIR"

MANAGER_PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
GATEWAY_PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"

MANAGER_URL="http://127.0.0.1:${MANAGER_PORT}"
GATEWAY_URL="http://127.0.0.1:${GATEWAY_PORT}"
TARGET_LOG="${SMOKE_DIR}/actions.jsonl"
GATEWAY_STDOUT="${SMOKE_DIR}/gateway.stdout.log"
MANAGER_STDOUT="${SMOKE_DIR}/manager.stdout.log"
SMOKE_TOKEN="action-gateway-smoke-token"

GATEWAY_PID=""
MANAGER_PID=""

cleanup() {
  if [[ -n "${MANAGER_PID}" ]]; then
    kill "${MANAGER_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${GATEWAY_PID}" ]]; then
    kill "${GATEWAY_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

wait_health() {
  local url="$1"
  local name="$2"
  for _ in $(seq 1 80); do
    if curl -fsS "${url}/healthz" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  echo "${name} did not become healthy" >&2
  echo "smoke logs: ${SMOKE_DIR}" >&2
  return 1
}

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

manager_post() {
  local path="$1"
  local body="$2"
  curl -fsS \
    -H "content-type: application/json" \
    -H "x-agent-user: action-gateway-smoke-admin" \
    -H "x-agent-service: action-gateway-smoke" \
    -H "x-agent-roles: system_admin" \
    -X POST \
    -d "$body" \
    "${MANAGER_URL}${path}"
}

viewer_post() {
  local path="$1"
  local body="$2"
  curl -fsS \
    -H "content-type: application/json" \
    -H "x-agent-user: action-gateway-smoke-viewer" \
    -H "x-agent-service: action-gateway-smoke" \
    -H "x-agent-roles: viewer" \
    -X POST \
    -d "$body" \
    "${MANAGER_URL}${path}"
}

"${CARGO_BIN}" build --quiet --manifest-path "${ROOT}/Cargo.toml" --bins

AGENT_ACTION_GATEWAY_TARGET_LOG="${TARGET_LOG}" \
AGENT_ACTION_GATEWAY_API_KEY="${SMOKE_TOKEN}" \
AGENT_ACTION_GATEWAY_ALLOWED_SCOPES="agent-platform:action-gateway-smoke" \
AGENT_ACTION_GATEWAY_CONNECTOR="action-journal" \
AGENT_ACTION_GATEWAY_LEASE_TTL_SECONDS="300" \
RUST_LOG="${RUST_LOG:-warn}" \
"${ROOT}/target/debug/agent-action-gateway" --bind "127.0.0.1:${GATEWAY_PORT}" \
  >"${GATEWAY_STDOUT}" 2>&1 &
GATEWAY_PID="$!"

env -u DATABASE_URL \
AGENT_ALLOW_DEV_HEADERS="true" \
AGENT_CREDENTIAL_PROVIDER_BASE_URL="${GATEWAY_URL}" \
AGENT_CREDENTIAL_PROVIDER_API_KEY="${SMOKE_TOKEN}" \
AGENT_CREDENTIAL_PROVIDER_TIMEOUT_SECONDS="3" \
AGENT_CREDENTIAL_LEASE_TTL_SECONDS="300" \
AGENT_WRITE_CONNECTOR_BASE_URL="${GATEWAY_URL}" \
AGENT_WRITE_CONNECTOR_API_KEY="${SMOKE_TOKEN}" \
AGENT_WRITE_CONNECTOR_TIMEOUT_SECONDS="3" \
AGENT_WRITE_CONNECTOR_MAX_ATTEMPTS="2" \
AGENT_EXTERNAL_ACTION_LOCK_LEASE_SECONDS="30" \
RUST_LOG="${RUST_LOG:-warn}" \
"${ROOT}/target/debug/agent-manager" --bind "127.0.0.1:${MANAGER_PORT}" \
  >"${MANAGER_STDOUT}" 2>&1 &
MANAGER_PID="$!"

wait_health "${GATEWAY_URL}" "agent-action-gateway"
wait_health "${MANAGER_URL}" "agent-manager"

CREATE_AGENT_RESPONSE="$(manager_post "/v1/agent-requests" '{
  "request_type": "create_agent",
  "agent_type": "background_worker",
  "target_resource": "resource:team/action-gateway-smoke",
  "intent_text": "external action smoke agent",
  "structured_payload": {"purpose": "action-gateway-smoke"},
  "idempotency_key": "action-gateway-smoke-agent",
  "risk_level": "low",
  "external_action_mode": "read_only"
}')"
AGENT_ID="$(printf '%s' "${CREATE_AGENT_RESPONSE}" | json_get "agent_id")"

RUN_RESPONSE="$(manager_post "/v1/my-agents/${AGENT_ID}/runs" '{
  "trigger_type": "manual",
  "idempotency_key": "action-gateway-smoke-run",
  "target_resource": "resource:team/action-gateway-smoke",
  "risk_level": "low",
  "external_action_mode": "read_only"
}')"
RUN_ID="$(printf '%s' "${RUN_RESPONSE}" | json_get "id")"

APPROVAL_RESPONSE="$(viewer_post "/v1/agent-requests" '{
  "request_type": "change_agent",
  "agent_type": "background_worker",
  "target_resource": "resource:team/action-gateway-smoke",
  "intent_text": "Approve low-risk action-gateway smoke write",
  "structured_payload": {"purpose": "action-gateway-approval"},
  "idempotency_key": "action-gateway-smoke-approval-request",
  "risk_level": "low",
  "external_action_mode": "approval_required"
}')"
APPROVAL_REQUEST_ID="$(printf '%s' "${APPROVAL_RESPONSE}" | json_get "request_id")"
APPROVAL_ID="$(printf '%s' "${APPROVAL_RESPONSE}" | json_get "approval_id")"

manager_post "/v1/admin/requests/${APPROVAL_REQUEST_ID}/approve" '{
  "reason": "external action smoke approval"
}' >/dev/null

DRY_RUN_RESPONSE="$(manager_post "/v1/admin/runs/${RUN_ID}/external-action-plans/dry-run" "{
  \"connector\": \"action-journal\",
  \"action\": \"target.write\",
  \"resource_ref\": \"resource:team/action-gateway-smoke\",
  \"credential_scope\": \"agent-platform:action-gateway-smoke\",
  \"approval_id\": \"${APPROVAL_ID}\",
  \"input_summary\": \"low-risk external action smoke write\",
  \"risk_level\": \"low\",
  \"external_action_mode\": \"authorized\"
}")"
PLAN_ID="$(printf '%s' "${DRY_RUN_RESPONSE}" | json_get "plan.id")"
DRY_RUN_STATUS="$(printf '%s' "${DRY_RUN_RESPONSE}" | json_get "dry_run_status")"

APPLY_RESPONSE="$(manager_post "/v1/admin/runs/${RUN_ID}/external-action-plans/${PLAN_ID}/apply" '{
  "payload": {
    "message": "external action smoke write",
    "target": "local-jsonl"
  }
}')"
APPLY_STATUS="$(printf '%s' "${APPLY_RESPONSE}" | json_get "apply_status")"
RESULT_REF="$(printf '%s' "${APPLY_RESPONSE}" | json_get "plan.result_ref")"
COMPENSATION_REF="$(printf '%s' "${APPLY_RESPONSE}" | json_get "plan.compensation_ref")"

COMPENSATION_RESPONSE="$(curl -fsS \
  -H "content-type: application/json" \
  -H "authorization: Bearer ${SMOKE_TOKEN}" \
  -X POST \
  -d "{
    \"compensation_ref\": \"${COMPENSATION_REF}\",
    \"reason\": \"external action smoke compensation verification\",
    \"trace_id\": \"action-gateway-smoke\"
  }" \
  "${GATEWAY_URL}/action-executions/compensate")"
COMPENSATION_STATUS="$(printf '%s' "${COMPENSATION_RESPONSE}" | json_get "status")"

python3 - "${TARGET_LOG}" "${PLAN_ID}" <<'PY'
import json
import sys

path, plan_id = sys.argv[1], sys.argv[2]
events = []
with open(path, "r", encoding="utf-8") as handle:
    for line in handle:
        line = line.strip()
        if line:
            events.append(json.loads(line))

executed = [
    event for event in events
    if event.get("event_type") == "action_executed"
    and event.get("plan_id") == plan_id
]
compensated = [
    event for event in events
    if event.get("event_type") == "action_compensated"
]
leases = [
    event for event in events
    if event.get("event_type") == "credential_lease_issued"
]
if len(executed) != 1 or len(compensated) != 1 or len(leases) != 1:
    raise SystemExit(
        f"expected one lease, one execution, one compensation; "
        f"got leases={len(leases)} executed={len(executed)} compensated={len(compensated)}"
    )
if not executed[0].get("result_ref") or not executed[0].get("compensation_ref"):
    raise SystemExit("execution event is missing result_ref or compensation_ref")
PY

echo "external action smoke passed"
echo "manager_url=${MANAGER_URL}"
echo "gateway_url=${GATEWAY_URL}"
echo "target_log=${TARGET_LOG}"
echo "dry_run_status=${DRY_RUN_STATUS}"
echo "apply_status=${APPLY_STATUS}"
echo "result_ref=${RESULT_REF}"
echo "compensation_ref=${COMPENSATION_REF}"
echo "compensation_status=${COMPENSATION_STATUS}"
