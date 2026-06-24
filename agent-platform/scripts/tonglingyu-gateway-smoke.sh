#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="${CARGO:-cargo}"
SMOKE_DIR="${TMPDIR:-/tmp}/tonglingyu-gateway-smoke-$$"
SEED_DB="${TONGLINGYU_SMOKE_DB_PATH:-}"
DB_PATH="${SMOKE_DIR}/tonglingyu.db"
STDOUT_LOG="${SMOKE_DIR}/gateway.stdout.log"
HEALTHCHECK_JSON="${SMOKE_DIR}/healthcheck.json"
HEALTH_JSON="${SMOKE_DIR}/healthz.json"
MODELS_UNAUTH_JSON="${SMOKE_DIR}/models-unauth.json"
MODELS_JSON="${SMOKE_DIR}/models.json"
METRICS_JSON="${SMOKE_DIR}/metrics.json"
PROMETHEUS_TXT="${SMOKE_DIR}/metrics.prom"
MIGRATE_JSON="${SMOKE_DIR}/runtime-schema-migrate.json"
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
TONGLINGYU_ONLINE_EVIDENCE_CARD_WORKER_ENABLED=false \
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

if [[ -n "${SEED_DB}" ]]; then
  curl -fsS "${auth[@]}" --get \
    --data-urlencode "q=通灵玉上的字是什么？" \
    --data-urlencode "limit=4" \
    "${BASE_URL}/v1/evidence/search" >"${SEARCH_JSON}"
  curl -fsS "${auth[@]}" "${json_headers[@]}" "${owui_headers[@]}" \
    -H "x-tonglingyu-message-id: smoke-message-1" \
    -X POST \
    -d '{"model":"tonglingyu","messages":[{"role":"user","content":"通灵玉上的字是什么？"}]}' \
    "${BASE_URL}/v1/chat/completions" >"${CHAT_JSON}"
else
  printf '{"skipped":true,"reason":"TONGLINGYU_SMOKE_DB_PATH not set"}\n' >"${SEARCH_JSON}"
  printf '{"skipped":true,"reason":"TONGLINGYU_SMOKE_DB_PATH not set"}\n' >"${CHAT_JSON}"
fi

python3 - \
  "${MIGRATE_JSON}" \
  "${HEALTHCHECK_JSON}" \
  "${HEALTH_JSON}" \
  "${MODELS_UNAUTH_JSON}" \
  "${MODELS_JSON}" \
  "${METRICS_JSON}" \
  "${PROMETHEUS_TXT}" \
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
assert health["sources"] >= 0, health
assert health["blocks"] >= 0, health
assert unauth["error"]["code"] == "gateway_unauthorized", unauth
assert [item["id"] for item in models["data"]] == ["tonglingyu"], models
assert metrics["object"] == "tonglingyu.gateway_metrics", metrics
assert metrics["dependencies"]["sqlite"] == "ok", metrics
assert metrics["security"]["gateway_key_count"] == 1, metrics
assert metrics["security"]["admin_key_count"] == 1, metrics
assert metrics["security"]["admin_key_isolated"] is True, metrics
assert metrics["counts"]["sources"] == health["sources"], (metrics, health)
assert metrics["counts"]["blocks"] == health["blocks"], (metrics, health)
assert "tonglingyu_gateway_info" in prometheus, prometheus
assert "tonglingyu_sources_total" in prometheus, prometheus
for forbidden_label in ["trace_id=", "package_id=", "question=", "query=", "user=", "session_id="]:
    assert forbidden_label not in prometheus, forbidden_label

if seed_db:
    assert search["object"] == "list", search
    assert "choices" in chat, chat
else:
    assert search["skipped"] is True, search
    assert chat["skipped"] is True, chat
PY

echo "tonglingyu gateway smoke passed"
echo "base_url=${BASE_URL}"
echo "db_path=${DB_PATH}"
echo "smoke_dir=${SMOKE_DIR}"
