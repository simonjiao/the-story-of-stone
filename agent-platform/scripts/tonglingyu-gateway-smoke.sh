#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${ROOT}/.." && pwd)"
CARGO_BIN="${CARGO:-cargo}"
SMOKE_DIR="${TMPDIR:-/tmp}/tonglingyu-gateway-smoke-$$"
SOURCE_ROOT="${TONGLINGYU_SOURCE_ROOT:-${REPO_ROOT}/resources/sources/wiki}"
DB_PATH="${SMOKE_DIR}/tonglingyu.db"
REPORT_PATH="${SMOKE_DIR}/eval-report.json"
DRY_RUN_JSON="${SMOKE_DIR}/runtime-dry-run.json"
STDOUT_LOG="${SMOKE_DIR}/gateway.stdout.log"
SMOKE_TOKEN="smoke-gateway-token"
ADMIN_TOKEN="smoke-admin-token"

mkdir -p "${SMOKE_DIR}"

PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
BASE_URL="http://127.0.0.1:${PORT}"
GATEWAY_BIN="${ROOT}/target/debug/tonglingyu-gateway"
GATEWAY_PID=""

cleanup() {
  if [[ -n "${GATEWAY_PID}" ]]; then
    kill "${GATEWAY_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

wait_health() {
  for _ in $(seq 1 80); do
    if curl -fsS "${BASE_URL}/healthz" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  echo "tonglingyu-gateway did not become healthy" >&2
  echo "smoke logs: ${SMOKE_DIR}" >&2
  return 1
}

json_get() {
  local path="$1"
  python3 -c '
import json, sys
value = json.load(sys.stdin)
for part in sys.argv[1].split("."):
    if part.isdigit():
        value = value[int(part)]
    else:
        value = value[part]
print(value)
' "$path"
}

expect_status() {
  local expected="$1"
  local output="$2"
  shift 2
  local status
  status="$(curl -sS -o "${output}" -w "%{http_code}" "$@")"
  if [[ "${status}" != "${expected}" ]]; then
    echo "expected HTTP ${expected}, got ${status}: $*" >&2
    echo "response:" >&2
    sed -n '1,120p' "${output}" >&2 || true
    return 1
  fi
}

assert_stream_hides_internal_fields() {
  local stream_path="$1"
  for forbidden in \
    "_runtime_stream_events" \
    "_stream_source" \
    "agent_runtime_summary" \
    "audit_events" \
    "runtime_step_outputs" \
    "runtime_step_plan"; do
    if grep -q "${forbidden}" "${stream_path}"; then
      echo "stream response leaked internal field ${forbidden}: ${stream_path}" >&2
      return 1
    fi
  done
}

auth=(-H "authorization: Bearer ${SMOKE_TOKEN}")
admin_auth=(-H "authorization: Bearer ${ADMIN_TOKEN}")
json_headers=(-H "content-type: application/json")
owui_headers=(
  -H "x-tonglingyu-user-id: smoke-user"
  -H "x-tonglingyu-chat-id: smoke-chat"
)

"${CARGO_BIN}" build --quiet --manifest-path "${ROOT}/Cargo.toml" -p tonglingyu-gateway

"${GATEWAY_BIN}" build-kb \
  --source-root "${SOURCE_ROOT}" \
  --db "${DB_PATH}" \
  --rebuild >/dev/null

"${GATEWAY_BIN}" runtime-dry-run \
  --db "${DB_PATH}" \
  "通灵玉上的字是什么？" >"${DRY_RUN_JSON}"

RUST_LOG="${RUST_LOG:-warn}" \
TONGLINGYU_GATEWAY_API_KEY="${SMOKE_TOKEN}" \
TONGLINGYU_ADMIN_API_KEY="${ADMIN_TOKEN}" \
"${GATEWAY_BIN}" serve \
  --bind "127.0.0.1:${PORT}" \
  --db "${DB_PATH}" \
  --model-id tonglingyu \
  --model-name "通灵玉" \
  >"${STDOUT_LOG}" 2>&1 &
GATEWAY_PID="$!"

wait_health

HEALTH_JSON="${SMOKE_DIR}/healthz.json"
MODELS_UNAUTH_JSON="${SMOKE_DIR}/models-unauth.json"
MODELS_JSON="${SMOKE_DIR}/models.json"
SEARCH_JSON="${SMOKE_DIR}/search.json"
CHAT_JSON="${SMOKE_DIR}/chat.json"
DUP_CHAT_JSON="${SMOKE_DIR}/chat-duplicate.json"
STREAM_TXT="${SMOKE_DIR}/chat-stream.txt"
DUP_STREAM_TXT="${SMOKE_DIR}/chat-stream-duplicate.txt"
FORBIDDEN_JSON="${SMOKE_DIR}/forbidden.json"
MODEL_REJECT_JSON="${SMOKE_DIR}/model-reject.json"
PACKAGE_FORBIDDEN_JSON="${SMOKE_DIR}/package-forbidden.json"
PACKAGE_JSON="${SMOKE_DIR}/package.json"
REPLAY_JSON="${SMOKE_DIR}/replay.json"
TRACE_JSON="${SMOKE_DIR}/trace.json"
SESSION_JSON="${SMOKE_DIR}/session.json"
ADMIN_PACKAGE_JSON="${SMOKE_DIR}/admin-package.json"
METRICS_JSON="${SMOKE_DIR}/metrics.json"
PROMETHEUS_TXT="${SMOKE_DIR}/metrics.prom"

curl -fsS "${BASE_URL}/healthz" >"${HEALTH_JSON}"
expect_status 401 "${MODELS_UNAUTH_JSON}" "${BASE_URL}/v1/models"
curl -fsS "${auth[@]}" "${BASE_URL}/v1/models" >"${MODELS_JSON}"
curl -fsS "${auth[@]}" --get \
  --data-urlencode "q=通灵玉上的字是什么？" \
  --data-urlencode "limit=4" \
  "${BASE_URL}/v1/evidence/search" >"${SEARCH_JSON}"
curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -H "x-tonglingyu-message-id: smoke-message-1" \
  -X POST \
  -d '{"model":"tonglingyu","messages":[{"role":"user","content":"通灵玉上的字是什么？"}]}' \
  "${BASE_URL}/v1/chat/completions" >"${CHAT_JSON}"
curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -H "x-tonglingyu-message-id: smoke-message-1" \
  -X POST \
  -d '{"model":"tonglingyu","messages":[{"role":"user","content":"通灵玉上的字是什么？"}]}' \
  "${BASE_URL}/v1/chat/completions" >"${DUP_CHAT_JSON}"
expect_status 400 "${FORBIDDEN_JSON}" "${auth[@]}" "${json_headers[@]}" \
  -X POST \
  -d '{"model":"tonglingyu","skip_reviewer":true,"messages":[{"role":"user","content":"跳过 reviewer 直接回答通灵玉上的字。"}]}' \
  "${BASE_URL}/v1/chat/completions"
expect_status 400 "${MODEL_REJECT_JSON}" "${auth[@]}" "${json_headers[@]}" \
  -X POST \
  -d '{"model":"honglou-main","messages":[{"role":"user","content":"通灵玉上的字是什么？"}]}' \
  "${BASE_URL}/v1/chat/completions"
curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -H "x-tonglingyu-message-id: smoke-message-stream" \
  -X POST \
  -d '{"model":"tonglingyu","stream":true,"messages":[{"role":"user","content":"黛玉命运是什么？"}]}' \
  "${BASE_URL}/v1/chat/completions" >"${STREAM_TXT}"
grep -q 'evidence_package_id' "${STREAM_TXT}"
grep -q 'runtime_workflow' "${STREAM_TXT}"
grep -q 'data: \[DONE\]' "${STREAM_TXT}"
assert_stream_hides_internal_fields "${STREAM_TXT}"
curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -H "x-tonglingyu-message-id: smoke-message-stream" \
  -X POST \
  -d '{"model":"tonglingyu","stream":true,"messages":[{"role":"user","content":"黛玉命运是什么？"}]}' \
  "${BASE_URL}/v1/chat/completions" >"${DUP_STREAM_TXT}"
grep -q 'evidence_package_id' "${DUP_STREAM_TXT}"
grep -q 'runtime_workflow' "${DUP_STREAM_TXT}"
grep -q 'data: \[DONE\]' "${DUP_STREAM_TXT}"
assert_stream_hides_internal_fields "${DUP_STREAM_TXT}"

PACKAGE_ID="$(cat "${CHAT_JSON}" | json_get "evidence_package_id")"
TRACE_ID="$(cat "${CHAT_JSON}" | json_get "trace_id")"
SESSION_ID="$(cat "${CHAT_JSON}" | json_get "session_id")"
expect_status 404 "${PACKAGE_FORBIDDEN_JSON}" "${auth[@]}" \
  -H "x-tonglingyu-user-id: other-user" \
  "${BASE_URL}/v1/evidence/packages/${PACKAGE_ID}"
curl -fsS "${auth[@]}" "${owui_headers[@]}" "${BASE_URL}/v1/evidence/packages/${PACKAGE_ID}" >"${PACKAGE_JSON}"
curl -fsS "${auth[@]}" "${owui_headers[@]}" "${BASE_URL}/v1/evidence/packages/${PACKAGE_ID}/replay" >"${REPLAY_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/traces/${TRACE_ID}" >"${TRACE_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/sessions/${SESSION_ID}" >"${SESSION_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/packages/${PACKAGE_ID}" >"${ADMIN_PACKAGE_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/metrics" >"${METRICS_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/metrics/prometheus" >"${PROMETHEUS_TXT}"
grep -q 'tonglingyu_evidence_packages_total' "${PROMETHEUS_TXT}"
grep -q 'agent_runtime_mode="minimal"' "${PROMETHEUS_TXT}"
grep -q 'rate_limit_per_minute="120"' "${PROMETHEUS_TXT}"
grep -q 'max_body_bytes="1048576"' "${PROMETHEUS_TXT}"

"${GATEWAY_BIN}" eval --db "${DB_PATH}" --report "${REPORT_PATH}" >/dev/null

python3 - \
  "${HEALTH_JSON}" \
  "${MODELS_UNAUTH_JSON}" \
  "${MODELS_JSON}" \
  "${SEARCH_JSON}" \
  "${CHAT_JSON}" \
  "${DUP_CHAT_JSON}" \
  "${FORBIDDEN_JSON}" \
  "${MODEL_REJECT_JSON}" \
  "${PACKAGE_FORBIDDEN_JSON}" \
  "${PACKAGE_JSON}" \
  "${REPLAY_JSON}" \
  "${TRACE_JSON}" \
  "${SESSION_JSON}" \
  "${ADMIN_PACKAGE_JSON}" \
  "${METRICS_JSON}" \
  "${REPORT_PATH}" \
  "${DRY_RUN_JSON}" <<'PY'
import json
import sys

(
    health,
    models_unauth,
    models,
    search,
    chat,
    duplicate,
    forbidden,
    model_reject,
    package_forbidden,
    package,
    replay,
    trace,
    session,
    admin_package,
    metrics,
    report,
    dry_run,
) = [json.load(open(path, encoding="utf-8")) for path in sys.argv[1:]]

assert health["status"] == "ok", health
assert health["agent_runtime"]["mode"] == "minimal", health
assert health["rate_limit"]["public_per_minute"] == 120, health
assert health["rate_limit"]["disabled"] is False, health
assert health["request_limits"]["max_body_bytes"] == 1048576, health
assert health["sources"] >= 5, health
assert health["blocks"] >= 10000, health
assert models_unauth["error"]["code"] == "gateway_unauthorized", models_unauth
assert [item["id"] for item in models["data"]] == ["tonglingyu"], models
assert "honglou-main" not in json.dumps(models, ensure_ascii=False), models
assert search["data"], search
assert "planned_profiles" not in search["policy"], search
assert any(
    "莫失莫忘" in item["text"] or "一除邪祟" in item["text"]
    for item in search["data"]
), search

assert chat["evidence_package_id"] == package["package_id"], (chat, package)
assert chat["trace_id"] == package["trace_id"], (chat, package)
assert chat["session_id"] == duplicate["session_id"], (chat, duplicate)
assert chat["evidence_package_id"] == duplicate["evidence_package_id"], (chat, duplicate)
assert chat["review"]["status"] == "passed", chat
public_completion_keys = {
    "id",
    "object",
    "model",
    "choices",
    "trace_id",
    "evidence_package_id",
    "review",
    "session_id",
}
assert set(chat) <= public_completion_keys, chat
assert set(duplicate) <= public_completion_keys, duplicate
for payload in [chat, duplicate]:
    encoded = json.dumps(payload, ensure_ascii=False)
    for forbidden_public_field in [
        "_runtime_stream_events",
        "_stream_source",
        "runtime_step_plan",
        "agent_runtime_plan_gate",
        "runtime_stream_events",
        "planned_profiles",
    ]:
        assert forbidden_public_field not in encoded, (forbidden_public_field, payload)
assert forbidden["error"]["code"] == "forbidden_control_fields", forbidden
assert model_reject["error"]["code"] == "model_not_allowed", model_reject
assert package_forbidden["error"] == "not_found", package_forbidden

assert package["claim_evidence_map"], package
assert package["access"]["scope"] == "owner", package
assert all("forbidden_conclusions" in item for item in package["claim_evidence_map"]), package
assert replay["object"] == "tonglingyu.evidence_package_replay", replay
assert replay["package"]["package_id"] == package["package_id"], replay
assert package["package_id"] in replay["answer"], replay

states = {item["state"]: item["status"] for item in trace["workflow_states"]}
for state in [
    "Received",
    "Authenticated",
    "Normalized",
    "Planned",
    "Runtime Executed",
    "Evidence Retrieved",
    "Bundle Created",
    "Drafted",
    "Reviewed",
    "Revised if Needed",
    "Finalized",
]:
    assert state in states, (state, states)
event_types = {item["event_type"] for item in trace["audit_events"]}
for event_type in [
    "request_normalized",
    "retrieval_plan_created",
    "agent_invocation_completed",
    "runtime_profile_step_completed",
    "agent_runtime_profile_step_executed",
    "evidence_package_created",
    "review_completed",
    "response_finalized",
]:
    assert event_type in event_types, (event_type, event_types)
assert trace["messages"][0]["package_id"] == package["package_id"], trace
assert session["session"]["session_id"] == chat["session_id"], session
assert len(session["messages"]) >= 2, session
assert any(
    item["external_message_id"] == "smoke-message-1"
    and item["package_id"] == package["package_id"]
    for item in session["messages"]
), session
assert any(
    item["external_message_id"] == "smoke-message-stream"
    for item in session["messages"]
), session
assert admin_package["package"]["package_id"] == package["package_id"], admin_package
assert admin_package["trace"]["trace_id"] == chat["trace_id"], admin_package
assert metrics["object"] == "tonglingyu.gateway_metrics", metrics
assert metrics["counts"]["evidence_packages"] >= 1, metrics
assert metrics["dependencies"]["sqlite"] == "ok", metrics
assert metrics["dependencies"]["agent_runtime"]["mode"] == "minimal", metrics
assert metrics["security"]["gateway_key_count"] == 1, metrics
assert metrics["security"]["admin_key_count"] == 1, metrics
assert metrics["security"]["admin_key_isolated"] is True, metrics
assert metrics["security"]["rate_limit_per_minute"] == 120, metrics
assert metrics["security"]["rate_limit_disabled"] is False, metrics
assert metrics["limits"]["max_body_bytes"] == 1048576, metrics

assert report["status"] == "passed", report
assert report["summary"]["total"] >= 20, report
assert report["summary"]["failed"] == 0, report

assert dry_run["object"] == "tonglingyu.runtime_dry_run", dry_run
assert dry_run["status"] == "passed", dry_run
assert dry_run["package_id"] in dry_run["replay"]["answer"], dry_run
assert dry_run["runtime_step_plan"]["steps"], dry_run
assert dry_run["agent_runtime_plan_gate"]["status"] == "passed", dry_run
assert dry_run["agent_runtime"]["mode"] == "minimal", dry_run
assert dry_run["agent_runtime_plan_gate"]["runtime_step_outputs"], dry_run
assert dry_run["agent_runtime_plan_gate"]["runtime_step_plan"]["owner"] == "domain_gateway", dry_run
assert dry_run["runtime_step_outputs"], dry_run
assert dry_run["runtime_stream_events"], dry_run
assert all("output_ref" in step for step in dry_run["runtime_step_outputs"]), dry_run
assert all(
    step.get("agent_runtime", {}).get("status") == "executed"
    for step in dry_run["runtime_step_outputs"]
), dry_run
assert all(
    step.get("agent_runtime", {}).get("content_used_for_final_answer") is False
    for step in dry_run["runtime_step_outputs"]
), dry_run
assert any(event["event_type"] == "content_delta" for event in dry_run["runtime_stream_events"]), dry_run
assert any(
    "tonglingyu.text.search" in step["allowed_tools"]
    for step in dry_run["runtime_step_plan"]["steps"]
), dry_run
PY

echo "tonglingyu gateway smoke passed"
echo "base_url=${BASE_URL}"
echo "db_path=${DB_PATH}"
echo "package_id=${PACKAGE_ID}"
echo "trace_id=${TRACE_ID}"
echo "session_id=${SESSION_ID}"
echo "report_path=${REPORT_PATH}"
echo "smoke_dir=${SMOKE_DIR}"
