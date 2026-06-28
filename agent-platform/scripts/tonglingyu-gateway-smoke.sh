#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="${CARGO:-cargo}"
SMOKE_DIR="${TMPDIR:-/tmp}/tonglingyu-gateway-smoke-$$"
SEED_DB="${TONGLINGYU_SMOKE_DB_PATH:-}"
SMOKE_RETRIEVER_BASE_URL="${TONGLINGYU_SMOKE_RETRIEVER_BASE_URL:-}"
DB_PATH="${SMOKE_DIR}/tonglingyu.db"
STDOUT_LOG="${SMOKE_DIR}/gateway.stdout.log"
HEALTHCHECK_JSON="${SMOKE_DIR}/healthcheck.json"
HEALTH_JSON="${SMOKE_DIR}/healthz.json"
MODELS_UNAUTH_JSON="${SMOKE_DIR}/models-unauth.json"
MODELS_JSON="${SMOKE_DIR}/models.json"
METRICS_JSON="${SMOKE_DIR}/metrics.json"
PROMETHEUS_TXT="${SMOKE_DIR}/metrics.prom"
MIGRATE_JSON="${SMOKE_DIR}/runtime-schema-migrate.json"
RESPONSE_JSON="${SMOKE_DIR}/response.json"
RESPONSE_STATUS_JSON="${SMOKE_DIR}/response-status.json"
RESPONSE_EVENTS_TXT="${SMOKE_DIR}/response-events.txt"
RUN_JSON="${SMOKE_DIR}/run.json"
RUN_CANCEL_JSON="${SMOKE_DIR}/run-cancel.json"
RUN_EVENTS_TXT="${SMOKE_DIR}/run-events.txt"
FORBIDDEN_RESPONSE_JSON="${SMOKE_DIR}/response-forbidden.json"
RETRIEVER_CALLS_JSONL="${SMOKE_DIR}/retriever-calls.jsonl"
SEARCH_JSON="${SMOKE_DIR}/search.json"
CHAT_JSON="${SMOKE_DIR}/chat.json"
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
RETRIEVER_PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
BASE_URL="http://127.0.0.1:${PORT}"
RETRIEVER_BASE_URL="${SMOKE_RETRIEVER_BASE_URL}"
GATEWAY_BIN="${ROOT}/target/debug/tonglingyu-gateway"
GATEWAY_PID=""
RETRIEVER_PID=""

cleanup() {
  if [[ -n "${GATEWAY_PID}" ]]; then
    kill "${GATEWAY_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RETRIEVER_PID}" ]]; then
    kill "${RETRIEVER_PID}" >/dev/null 2>&1 || true
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

wait_retriever() {
  for _ in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:${RETRIEVER_PORT}/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  echo "smoke retriever health stub did not become healthy" >&2
  echo "smoke logs: ${SMOKE_DIR}" >&2
  return 1
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

auth=(-H "authorization: Bearer ${SMOKE_TOKEN}")
admin_auth=(-H "authorization: Bearer ${ADMIN_TOKEN}")
json_headers=(-H "content-type: application/json")
owui_headers=(
  -H "x-tonglingyu-user-id: smoke-user"
  -H "x-tonglingyu-chat-id: smoke-chat"
)

"${CARGO_BIN}" build --quiet --manifest-path "${ROOT}/Cargo.toml" -p tonglingyu-gateway

if [[ -n "${SEED_DB}" ]]; then
  cp "${SEED_DB}" "${DB_PATH}"
fi

"${GATEWAY_BIN}" runtime-schema-migrate --db "${DB_PATH}" >"${MIGRATE_JSON}"

if [[ -z "${RETRIEVER_BASE_URL}" ]]; then
  RETRIEVER_BASE_URL="http://127.0.0.1:${RETRIEVER_PORT}"
  : >"${RETRIEVER_CALLS_JSONL}"
  python3 - "${RETRIEVER_PORT}" "${RETRIEVER_CALLS_JSONL}" <<'PY' >/dev/null 2>&1 &
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

CALLS_PATH = sys.argv[2]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, _format, *args):
        return

    def record_request(self, method):
        with open(CALLS_PATH, "a", encoding="utf-8") as handle:
            handle.write(json.dumps({"method": method, "path": self.path}) + "\n")

    def do_GET(self):
        self.record_request("GET")
        service = {
            "schema_version": "tonglingyu.agent_retriever.service.v1",
            "name": "smoke-retriever-health",
            "version": "0",
            "repo_root": "smoke",
            "index_path": "smoke",
            "env_file": "",
        }
        if self.path == "/health":
            payload = {
                "ok": True,
                "schema_version": "tonglingyu.agent_retriever.service_health.v1",
                "record_type": "agent_retriever_service_health",
                "service": service,
                "status": "ok",
                "ready": True,
                "required_failed": [],
                "components": {},
            }
        elif self.path == "/metadata":
            payload = {
                "ok": True,
                "schema_version": "tonglingyu.agent_retriever.service_metadata.v1",
                "record_type": "agent_retriever_service_metadata",
                "service": service,
                "contracts": {
                    "search_plan_schema": "tonglingyu.agent_retriever.search_plan.v1",
                    "retrieve_options_schema": "tonglingyu.agent_retriever.retrieve_options.v1",
                    "evidence_pack_schema": "tonglingyu.agent_retriever.evidence_pack.v1",
                    "retrieve_response_schema": "tonglingyu.agent_retriever.retrieve_response.v1",
                    "error_response_schema": "tonglingyu.agent_retriever.error_response.v1",
                },
                "capabilities": {
                    "routes": [
                        "bm25",
                        "vector",
                        "entity",
                        "event",
                        "poem",
                        "commentary",
                    ],
                    "required_routes": ["vector"],
                },
                "adapter_guidance": {
                    "stable_input": "SearchPlan + RetrieveOptions",
                },
            }
        else:
            self.send_response(404)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"ok":false,"error":"not_found"}')
            return
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        self.record_request("POST")
        self.send_response(501)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(
            b'{"ok":false,"error":"smoke_retriever_stub_does_not_mock_retrieve"}'
        )


HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY
  RETRIEVER_PID="$!"
  wait_retriever
fi

RUST_LOG="${RUST_LOG:-warn}" \
TONGLINGYU_GATEWAY_API_KEY="${SMOKE_TOKEN}" \
TONGLINGYU_ADMIN_API_KEY="${ADMIN_TOKEN}" \
TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER=smoke_profile \
TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER=smoke_profile \
TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER=smoke_profile \
TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER=smoke_profile \
TONGLINGYU_AGENT_ROLE_QUESTION_NORMALIZER_PROVIDER=smoke_profile \
TONGLINGYU_AGENT_ROLE_CONVERSATION_STATE_PROVIDER=smoke_profile \
TONGLINGYU_AGENT_PROVIDER_SMOKE_PROFILE_BACKEND=openai-compatible-network \
TONGLINGYU_AGENT_PROVIDER_SMOKE_PROFILE_BASE_URL=http://127.0.0.1:9/v1 \
TONGLINGYU_AGENT_PROVIDER_SMOKE_PROFILE_MODEL=smoke-model \
TONGLINGYU_AGENT_PROVIDER_SMOKE_PROFILE_API_KEY_ENV=TONGLINGYU_SMOKE_AGENT_API_KEY \
TONGLINGYU_SMOKE_AGENT_API_KEY=smoke-agent-token \
TONGLINGYU_RETRIEVER_BASE_URL="${RETRIEVER_BASE_URL}" \
TONGLINGYU_ONLINE_EVIDENCE_CARD_WORKER_ENABLED=false \
TONGLINGYU_RESPONSE_WORKER_ENABLED=false \
"${GATEWAY_BIN}" serve \
  --bind "127.0.0.1:${PORT}" \
  --db "${DB_PATH}" \
  --model-id tonglingyu \
  --model-name "通灵玉" \
  >"${STDOUT_LOG}" 2>&1 &
GATEWAY_PID="$!"

wait_health

"${GATEWAY_BIN}" healthcheck --url "${BASE_URL}/healthz" >"${HEALTHCHECK_JSON}"
curl -fsS "${BASE_URL}/healthz" >"${HEALTH_JSON}"
expect_status 401 "${MODELS_UNAUTH_JSON}" "${BASE_URL}/v1/models"
curl -fsS "${auth[@]}" "${BASE_URL}/v1/models" >"${MODELS_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/metrics" >"${METRICS_JSON}"
curl -fsS "${admin_auth[@]}" "${BASE_URL}/v1/admin/metrics/prometheus" >"${PROMETHEUS_TXT}"

curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -X POST \
  -d '{"model":"tonglingyu","input":"smoke response","background":true,"metadata":{"mode":"smoke"}}' \
  "${BASE_URL}/v1/responses" >"${RESPONSE_JSON}"
response_id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8"))["id"])' "${RESPONSE_JSON}")"
curl -fsS "${auth[@]}" "${owui_headers[@]}" \
  "${BASE_URL}/v1/responses/${response_id}" >"${RESPONSE_STATUS_JSON}"
curl -fsS "${auth[@]}" "${owui_headers[@]}" \
  "${BASE_URL}/v1/responses/${response_id}/events" >"${RESPONSE_EVENTS_TXT}"

curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -X POST \
  -d '{"model":"tonglingyu","input":"smoke cancel","background":true}' \
  "${BASE_URL}/v1/runs" >"${RUN_JSON}"
run_id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8"))["id"])' "${RUN_JSON}")"
curl -fsS "${auth[@]}" "${owui_headers[@]}" \
  -X POST \
  "${BASE_URL}/v1/runs/${run_id}/cancel" >"${RUN_CANCEL_JSON}"
curl -fsS "${auth[@]}" "${owui_headers[@]}" \
  "${BASE_URL}/v1/runs/${run_id}/events" >"${RUN_EVENTS_TXT}"

expect_status 400 "${FORBIDDEN_RESPONSE_JSON}" \
  "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
  -X POST \
  -d '{"model":"tonglingyu","input":"smoke forbidden","background":true,"profile":"forged"}' \
  "${BASE_URL}/v1/responses"

if [[ -n "${SEED_DB}" ]]; then
  curl -fsS "${auth[@]}" --get \
    --data-urlencode "q=通灵玉上的字是什么？" \
    --data-urlencode "limit=4" \
    "${BASE_URL}/v1/evidence/search" >"${SEARCH_JSON}"
  if [[ -n "${SMOKE_RETRIEVER_BASE_URL}" ]]; then
    curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
      -H "x-tonglingyu-message-id: smoke-message-1" \
      -X POST \
      -d '{"model":"tonglingyu","messages":[{"role":"user","content":"通灵玉上的字是什么？"}]}' \
      "${BASE_URL}/v1/chat/completions" >"${CHAT_JSON}"
  else
    printf '{"skipped":true,"reason":"TONGLINGYU_SMOKE_RETRIEVER_BASE_URL not set"}\n' >"${CHAT_JSON}"
  fi
else
  printf '{"skipped":true,"reason":"TONGLINGYU_SMOKE_DB_PATH not set"}\n' >"${SEARCH_JSON}"
  printf '{"skipped":true,"reason":"TONGLINGYU_SMOKE_DB_PATH not set"}\n' >"${CHAT_JSON}"
fi

if [[ -z "${SMOKE_RETRIEVER_BASE_URL}" ]] && grep -q '"/retrieve"' "${RETRIEVER_CALLS_JSONL}"; then
  echo "default smoke retriever stub received /retrieve; provide TONGLINGYU_SMOKE_RETRIEVER_BASE_URL for real RQA chat smoke" >&2
  echo "retriever calls: ${RETRIEVER_CALLS_JSONL}" >&2
  exit 1
fi

python3 - \
  "${MIGRATE_JSON}" \
  "${HEALTHCHECK_JSON}" \
  "${HEALTH_JSON}" \
  "${MODELS_UNAUTH_JSON}" \
  "${MODELS_JSON}" \
  "${METRICS_JSON}" \
  "${PROMETHEUS_TXT}" \
  "${RESPONSE_JSON}" \
  "${RESPONSE_STATUS_JSON}" \
  "${RESPONSE_EVENTS_TXT}" \
  "${RUN_JSON}" \
  "${RUN_CANCEL_JSON}" \
  "${RUN_EVENTS_TXT}" \
  "${FORBIDDEN_RESPONSE_JSON}" \
  "${SEARCH_JSON}" \
  "${CHAT_JSON}" \
  "${SEED_DB}" <<'PY'
import json
import sys

(
    migrate_path,
    healthcheck_path,
    health_path,
    unauth_path,
    models_path,
    metrics_path,
    prometheus_path,
    response_path,
    response_status_path,
    response_events_path,
    run_path,
    run_cancel_path,
    run_events_path,
    forbidden_response_path,
    search_path,
    chat_path,
    seed_db,
) = sys.argv[1:]

with open(migrate_path, encoding="utf-8") as handle:
    migrate = json.load(handle)
with open(healthcheck_path, encoding="utf-8") as handle:
    healthcheck = json.load(handle)
with open(health_path, encoding="utf-8") as handle:
    health = json.load(handle)
with open(unauth_path, encoding="utf-8") as handle:
    unauth = json.load(handle)
with open(models_path, encoding="utf-8") as handle:
    models = json.load(handle)
with open(metrics_path, encoding="utf-8") as handle:
    metrics = json.load(handle)
with open(prometheus_path, encoding="utf-8") as handle:
    prometheus = handle.read()
with open(response_path, encoding="utf-8") as handle:
    response = json.load(handle)
with open(response_status_path, encoding="utf-8") as handle:
    response_status = json.load(handle)
with open(response_events_path, encoding="utf-8") as handle:
    response_events = handle.read()
with open(run_path, encoding="utf-8") as handle:
    run = json.load(handle)
with open(run_cancel_path, encoding="utf-8") as handle:
    run_cancel = json.load(handle)
with open(run_events_path, encoding="utf-8") as handle:
    run_events = handle.read()
with open(forbidden_response_path, encoding="utf-8") as handle:
    forbidden_response = json.load(handle)
with open(search_path, encoding="utf-8") as handle:
    search = json.load(handle)
with open(chat_path, encoding="utf-8") as handle:
    chat = json.load(handle)

assert migrate["status"] == "ok", migrate
assert migrate["will_rebuild_knowledge_base"] is False, migrate
assert healthcheck["status"] == "ok", healthcheck
assert health["status"] == "ok", health
assert health["model"] == "tonglingyu", health
assert health["online_evidence_card_ingest"]["worker_enabled"] is False, health
assert health["response_store"]["status"] == "ok", health
assert health["response_store"]["mode"] == "in_memory", health
assert health["response_jobs"]["status"] == "ok", health
assert health["response_jobs"]["mode"] == "in_memory", health
assert health["sources"] >= 0, health
assert health["blocks"] >= 0, health
assert unauth["error"]["code"] == "gateway_unauthorized", unauth
assert [item["id"] for item in models["data"]] == ["tonglingyu"], models
assert metrics["object"] == "tonglingyu.gateway_metrics", metrics
assert metrics["dependencies"]["sqlite"] == "ok", metrics
assert metrics["dependencies"]["response_store"]["status"] == "ok", metrics
assert metrics["dependencies"]["response_jobs"]["status"] == "ok", metrics
assert metrics["security"]["gateway_key_count"] == 1, metrics
assert metrics["security"]["admin_key_count"] == 1, metrics
assert metrics["security"]["admin_key_isolated"] is True, metrics
assert metrics["counts"]["sources"] == health["sources"], (metrics, health)
assert metrics["counts"]["blocks"] == health["blocks"], (metrics, health)
assert "tonglingyu_gateway_info" in prometheus, prometheus
assert "tonglingyu_sources_total" in prometheus, prometheus
assert "tonglingyu_response_store_up" in prometheus, prometheus
assert "tonglingyu_response_jobs_up" in prometheus, prometheus
for forbidden_label in ["trace_id=", "package_id=", "question=", "query=", "user=", "session_id="]:
    assert forbidden_label not in prometheus, forbidden_label

assert response["object"] == "response", response
assert response["status"] == "queued", response
assert response["run_id"].startswith("run_"), response
assert response["response_id"] == response["id"], response
assert response["events_url"] == f"/v1/responses/{response['id']}/events", response
assert response["cancel_requested"] is False, response
assert response_status["id"] == response["id"], response_status
assert response_status["status"] == "queued", response_status
assert "id: 1" in response_events, response_events
assert "event: response.created" in response_events, response_events
assert "trace_id" not in response_events, response_events

assert run["object"] == "run", run
assert run["status"] == "queued", run
assert run["id"] == run["run_id"], run
assert run["response_id"].startswith("resp_"), run
assert run["events_url"] == f"/v1/runs/{run['id']}/events", run
assert run_cancel["object"] == "run", run_cancel
assert run_cancel["id"] == run["id"], run_cancel
assert run_cancel["status"] == "canceled", run_cancel
assert run_cancel["cancel_requested"] is True, run_cancel
assert "id: 1" in run_events, run_events
assert "event: response.created" in run_events, run_events
assert "event: response.status" in run_events, run_events
assert "event: response.canceled" in run_events, run_events
assert "data: [DONE]" in run_events, run_events
assert "trace_id" not in run_events, run_events

assert forbidden_response["error"]["code"] == "forbidden_control_fields", forbidden_response

if seed_db:
    assert search["object"] == "list", search
    if chat.get("skipped"):
        assert chat["reason"] == "TONGLINGYU_SMOKE_RETRIEVER_BASE_URL not set", chat
    else:
        assert "choices" in chat, chat
else:
    assert search["skipped"] is True, search
    assert chat["skipped"] is True, chat
PY

echo "tonglingyu gateway smoke passed"
echo "base_url=${BASE_URL}"
echo "db_path=${DB_PATH}"
echo "smoke_dir=${SMOKE_DIR}"
