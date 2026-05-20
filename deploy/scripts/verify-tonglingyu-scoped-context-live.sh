#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_deploy_env_file_or_local

RUN_ID="${TONGLINGYU_SCOPED_CONTEXT_LIVE_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
SCOPED_USER_ID="scoped-context-live-${RUN_ID}"
HEALTH_JSON="${TONGLINGYU_SCOPED_CONTEXT_HEALTH_JSON:-${WORK_DIR}/health.json}"
FIRST_PAYLOAD="${WORK_DIR}/first-payload.json"
SECOND_PAYLOAD="${WORK_DIR}/second-payload.json"
LONG_PAYLOAD="${WORK_DIR}/long-payload.json"
MEMORY_PAYLOAD="${WORK_DIR}/memory-payload.json"
FIRST_JSON="${TONGLINGYU_SCOPED_CONTEXT_FIRST_JSON:-${WORK_DIR}/first.json}"
SECOND_JSON="${TONGLINGYU_SCOPED_CONTEXT_SECOND_JSON:-${WORK_DIR}/second.json}"
LONG_JSON="${TONGLINGYU_SCOPED_CONTEXT_LONG_JSON:-${WORK_DIR}/long.json}"
MEMORY_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_JSON:-${WORK_DIR}/memory.json}"
MEMORY_REF_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_REF_JSON:-${WORK_DIR}/memory-ref.json}"
MEMORY_READ_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_READ_JSON:-${WORK_DIR}/memory-read.json}"
MEMORY_READ_REF_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_READ_REF_JSON:-${WORK_DIR}/memory-read-ref.json}"
MEMORY_READ_TRACE_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_READ_TRACE_JSON:-${WORK_DIR}/memory-read-trace.json}"
MEMORY_COLLECTOR_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_COLLECTOR_JSON:-${WORK_DIR}/memory-collector.json}"
MEMORY_CANDIDATES_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_CANDIDATES_JSON:-${WORK_DIR}/memory-candidates.json}"
MEMORY_CARDS_JSON="${TONGLINGYU_SCOPED_CONTEXT_MEMORY_CARDS_JSON:-${WORK_DIR}/memory-cards.json}"
SECOND_REF_JSON="${TONGLINGYU_SCOPED_CONTEXT_SECOND_REF_JSON:-${WORK_DIR}/second-ref.json}"
LONG_REF_JSON="${TONGLINGYU_SCOPED_CONTEXT_LONG_REF_JSON:-${WORK_DIR}/long-ref.json}"
SECOND_TRACE_JSON="${TONGLINGYU_SCOPED_CONTEXT_SECOND_TRACE_JSON:-${WORK_DIR}/second-trace.json}"
LONG_TRACE_JSON="${TONGLINGYU_SCOPED_CONTEXT_LONG_TRACE_JSON:-${WORK_DIR}/long-trace.json}"
METRICS_JSON="${TONGLINGYU_SCOPED_CONTEXT_METRICS_JSON:-${WORK_DIR}/metrics.json}"

cd "${DEPLOY_DIR}"

docker compose exec -T open-webui \
  curl -fsS http://tonglingyu-gateway:8090/healthz >"${HEALTH_JSON}"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" http://tonglingyu-gateway:8090/v1/admin/metrics
' >"${METRICS_JSON}"

MAX_MESSAGES="$(
  python3 - "${METRICS_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    metrics = json.load(handle)
value = metrics.get("limits", {}).get("max_messages", 20)
print(int(value or 20))
PY
)"

python3 - "${FIRST_PAYLOAD}" <<'PY'
import json
import sys
payload = {
    "model": "tonglingyu",
    "messages": [{"role": "user", "content": "介绍贾宝玉"}],
}
with open(sys.argv[1], "w", encoding="utf-8") as handle:
    json.dump(payload, handle, ensure_ascii=False)
PY

docker compose exec -T -e VERIFY_RUN_ID="${RUN_ID}" open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: '"${SCOPED_USER_ID}"'" \
  -H "x-tonglingyu-chat-id: scoped-context-live-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: scoped-context-live-first-${VERIFY_RUN_ID}" \
  --data-binary @- \
  http://tonglingyu-gateway:8090/v1/chat/completions
' <"${FIRST_PAYLOAD}" >"${FIRST_JSON}"

python3 - "${SECOND_PAYLOAD}" <<'PY'
import json
import sys
payload = {
    "model": "tonglingyu",
    "messages": [{"role": "user", "content": "他是谁？"}],
}
with open(sys.argv[1], "w", encoding="utf-8") as handle:
    json.dump(payload, handle, ensure_ascii=False)
PY

docker compose exec -T -e VERIFY_RUN_ID="${RUN_ID}" open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: '"${SCOPED_USER_ID}"'" \
  -H "x-tonglingyu-chat-id: scoped-context-live-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: scoped-context-live-second-${VERIFY_RUN_ID}" \
  --data-binary @- \
  http://tonglingyu-gateway:8090/v1/chat/completions
' <"${SECOND_PAYLOAD}" >"${SECOND_JSON}"

python3 - "${LONG_PAYLOAD}" "${MAX_MESSAGES}" <<'PY'
import json
import sys
path = sys.argv[1]
max_messages = int(sys.argv[2])
messages = [
    {"role": "user", "content": f"历史消息 {index}"}
    for index in range(max_messages + 1)
]
messages.append({
    "role": "user",
    "content": """### Task:
Generate a concise, 3-5 word title with an emoji summarizing the chat history.
### Guidelines:
- The output must be a single, raw JSON object, without any markdown code fences.
### Output:
JSON format: { "title": "your concise title here" }
### Chat History:
<chat_history>
USER: 介绍贾宝玉
</chat_history>""",
})
payload = {"model": "tonglingyu", "messages": messages}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle, ensure_ascii=False)
PY

docker compose exec -T -e VERIFY_RUN_ID="${RUN_ID}" open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: '"${SCOPED_USER_ID}"'" \
  -H "x-tonglingyu-chat-id: scoped-context-live-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: scoped-context-live-long-${VERIFY_RUN_ID}" \
  --data-binary @- \
  http://tonglingyu-gateway:8090/v1/chat/completions
' <"${LONG_PAYLOAD}" >"${LONG_JSON}"

python3 - "${MEMORY_PAYLOAD}" <<'PY'
import json
import sys
payload = {
    "model": "tonglingyu",
    "messages": [
        {
            "role": "user",
            "content": "以后回答《红楼梦》问题时，请用简体中文短句总结。现在介绍贾宝玉。",
        }
    ],
}
with open(sys.argv[1], "w", encoding="utf-8") as handle:
    json.dump(payload, handle, ensure_ascii=False)
PY

docker compose exec -T -e VERIFY_RUN_ID="${RUN_ID}" open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: '"${SCOPED_USER_ID}"'" \
  -H "x-tonglingyu-chat-id: scoped-context-live-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: scoped-context-live-memory-${VERIFY_RUN_ID}" \
  --data-binary @- \
  http://tonglingyu-gateway:8090/v1/chat/completions
' <"${MEMORY_PAYLOAD}" >"${MEMORY_JSON}"

resolve_scoped_message_ref() {
  local external_message_id="$1"
  python3 - "${DEPLOY_DIR}" "${external_message_id}" <<'PY'
import json
import os
import sqlite3
import sys
from pathlib import Path

deploy_dir = Path(sys.argv[1])
external_message_id = sys.argv[2]
container_db_path = os.environ.get("TONGLINGYU_DB_PATH", "/data/tonglingyu.db").strip() or "/data/tonglingyu.db"
data_dir = Path(os.environ.get("TONGLINGYU_DATA_DIR", "./data/tonglingyu"))
if not data_dir.is_absolute():
    data_dir = deploy_dir / data_dir

candidates = []
if container_db_path.startswith("/data/"):
    candidates.append(data_dir / container_db_path.removeprefix("/data/"))
else:
    raw = Path(container_db_path)
    candidates.append(raw if raw.is_absolute() else deploy_dir / raw)
candidates.append(deploy_dir / "data" / "tonglingyu" / "tonglingyu.db")

seen = set()
for candidate in candidates:
    candidate = candidate.resolve()
    if candidate in seen:
        continue
    seen.add(candidate)
    if not candidate.exists():
        continue
    conn = sqlite3.connect(f"file:{candidate}?mode=ro", uri=True)
    row = conn.execute(
        """
        SELECT trace_id, package_id, user_session_id, interaction_context_id, context_pack_id
        FROM session_journal
        WHERE external_message_id = ?
          AND entry_type = 'final_response'
        ORDER BY created_at DESC, journal_id DESC
        LIMIT 1
        """,
        (external_message_id,),
    ).fetchone()
    conn.close()
    if row:
        print(json.dumps({
            "trace_id": row[0],
            "package_id": row[1],
            "user_session_id": row[2],
            "interaction_context_id": row[3],
            "context_pack_id": row[4],
            "external_message_id": external_message_id,
            "source": "session_journal",
        }, ensure_ascii=True, sort_keys=True))
        raise SystemExit(0)

raise SystemExit(f"session journal ref not found for external_message_id={external_message_id}")
PY
}

resolve_scoped_message_ref "scoped-context-live-second-${RUN_ID}" >"${SECOND_REF_JSON}"
resolve_scoped_message_ref "scoped-context-live-long-${RUN_ID}" >"${LONG_REF_JSON}"
resolve_scoped_message_ref "scoped-context-live-memory-${RUN_ID}" >"${MEMORY_REF_JSON}"

SECOND_TRACE_ID="$(
  python3 - "${SECOND_REF_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    print(json.load(handle)["trace_id"])
PY
)"
LONG_TRACE_ID="$(
  python3 - "${LONG_REF_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    print(json.load(handle)["trace_id"])
PY
)"
MEMORY_TRACE_ID="$(
  python3 - "${MEMORY_REF_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    print(json.load(handle)["trace_id"])
PY
)"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/traces/'"${SECOND_TRACE_ID}"'"
' >"${SECOND_TRACE_JSON}"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/traces/'"${LONG_TRACE_ID}"'"
' >"${LONG_TRACE_JSON}"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS \
  -H "Authorization: Bearer ${TLY_ADMIN_KEY}" \
  -H "content-type: application/json" \
  --data-binary "{\"trigger\":\"admin_manual\",\"limit\":100,\"dry_run\":false,\"trace_id\":\"'"${MEMORY_TRACE_ID}"'\",\"llm_extraction_probe\":{\"schema_version\":\"scoped-memory-llm-filter-v1\",\"is_long_term_memory\":true,\"is_temporary_instruction\":false,\"is_quoted_or_third_party\":false,\"has_contradiction\":false,\"scope_type\":\"user_private\",\"candidate_type\":\"language_preference\",\"confidence\":0.86,\"sensitivity\":\"low\",\"risk_flags\":[],\"ttl_hint\":\"180d\",\"exclusion_flags\":[]}}" \
  http://tonglingyu-gateway:8090/v1/admin/memory/collector/run
' >"${MEMORY_COLLECTOR_JSON}"

MEMORY_CANDIDATE_ID="$(
  python3 - "${MEMORY_COLLECTOR_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
items = payload.get("candidates") or []
if not items:
    raise SystemExit("memory collector produced no candidates")
print(items[0]["candidate_id"])
PY
)"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/memory/candidates?status=approved&limit=20"
' >"${MEMORY_CANDIDATES_JSON}"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/memory/cards?status=active&limit=20"
' >"${MEMORY_CARDS_JSON}"

docker compose exec -T -e VERIFY_RUN_ID="${RUN_ID}" open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: '"${SCOPED_USER_ID}"'" \
  -H "x-tonglingyu-chat-id: scoped-context-live-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: scoped-context-live-memory-read-${VERIFY_RUN_ID}" \
  -d "{\"model\":\"tonglingyu\",\"messages\":[{\"role\":\"user\",\"content\":\"介绍林黛玉。\"}]}" \
  http://tonglingyu-gateway:8090/v1/chat/completions
' >"${MEMORY_READ_JSON}"
resolve_scoped_message_ref "scoped-context-live-memory-read-${RUN_ID}" >"${MEMORY_READ_REF_JSON}"
MEMORY_READ_TRACE_ID="$(
  python3 - "${MEMORY_READ_REF_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    print(json.load(handle)["trace_id"])
PY
)"
docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/traces/'"${MEMORY_READ_TRACE_ID}"'"
' >"${MEMORY_READ_TRACE_JSON}"

python3 - "${HEALTH_JSON}" "${FIRST_JSON}" "${SECOND_JSON}" "${LONG_JSON}" \
  "${MEMORY_JSON}" "${SECOND_REF_JSON}" "${LONG_REF_JSON}" "${MEMORY_REF_JSON}" \
  "${MEMORY_READ_JSON}" "${MEMORY_READ_REF_JSON}" "${MEMORY_READ_TRACE_JSON}" \
  "${SECOND_TRACE_JSON}" "${LONG_TRACE_JSON}" "${MEMORY_COLLECTOR_JSON}" \
  "${MEMORY_CANDIDATES_JSON}" "${MEMORY_CARDS_JSON}" "${MAX_MESSAGES}" "${RUN_ID}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    health_path,
    first_path,
    second_path,
    long_path,
    memory_path,
    second_ref_path,
    long_ref_path,
    memory_ref_path,
    memory_read_path,
    memory_read_ref_path,
    memory_read_trace_path,
    second_trace_path,
    long_trace_path,
    memory_collector_path,
    memory_candidates_path,
    memory_cards_path,
    max_messages_raw,
    run_id,
) = sys.argv[1:19]


def load(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def file_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def rendered(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


health = load(health_path)
first = load(first_path)
second = load(second_path)
long_response = load(long_path)
memory_response = load(memory_path)
second_ref = load(second_ref_path)
long_ref = load(long_ref_path)
memory_ref = load(memory_ref_path)
memory_read_response = load(memory_read_path)
memory_read_ref = load(memory_read_ref_path)
memory_read_trace = load(memory_read_trace_path)
second_trace = load(second_trace_path)
long_trace = load(long_trace_path)
memory_collector = load(memory_collector_path)
memory_candidates = load(memory_candidates_path)
memory_cards = load(memory_cards_path)
max_messages = int(max_messages_raw)
errors = []

for name, response in [
    ("first", first),
    ("second", second),
    ("long", long_response),
    ("memory", memory_response),
    ("memory_read", memory_read_response),
]:
    if response.get("object") != "chat.completion":
        errors.append(f"{name}_response_object_invalid")
    public_text = rendered(response)
    for forbidden in [
        "context_pack_id",
        "context_pack_ref",
        "context_projection",
        "context_projection_id",
        "context_projection_ref",
        "context_projection_digest",
        "context_projections",
        "consumer_type",
        "consumer_name",
        "runtime_adapter",
        "forbidden_tools",
        "tool_policy_digest",
        "output_contract_digest",
        "interaction_context_id",
        "user_session_id",
        "session_journal",
        "memory_read_refs",
        "memory_read_ref_digest",
        "memory_read_policy_digest",
        "memory_summaries",
        "memory_policy",
        "memory_candidate",
        "memory_candidate_id",
        "memory_card",
        "memory_card_id",
        "memory_policy_decision",
        "llm_extraction",
        "llm_filter",
        "rule_filter",
        "read_enabled",
        "trace_id",
        "evidence_package_id",
        "review",
    ]:
        if forbidden in public_text:
            errors.append(f"{name}_public_response_leaks_{forbidden}")

if health.get("status") not in {"ok", "healthy"}:
    errors.append("gateway_health_not_ok")

second_context = second_trace.get("scoped_context") or {}
second_packs = second_context.get("context_packs") or []
if not second_packs:
    errors.append("second_context_pack_missing")
else:
    pack = second_packs[-1]
    if pack.get("resolved_question") != "贾宝玉是谁？":
        errors.append("second_resolved_question_unexpected")
    if not pack.get("memory_read_ref_digest"):
        errors.append("second_memory_read_ref_digest_missing")
    if pack.get("memory_read_refs") != []:
        errors.append("second_memory_read_refs_must_be_empty")
    for view in pack.get("profile_views") or []:
        if view.get("profile_name") in {
            "honglou-text",
            "honglou-commentary",
            "honglou-reviewer",
        } and view.get("session_summary") is not None:
            errors.append(f"{view.get('profile_name')}_must_not_get_session_summary")


def validate_context_projections(trace_name, trace):
    context = trace.get("scoped_context") or {}
    packs = context.get("context_packs") or []
    projections = context.get("context_projections") or []
    if len(projections) < 4:
        errors.append(f"{trace_name}_context_projections_missing")
        return []
    pack_refs = {
        pack.get("context_pack_ref")
        for pack in packs
        if pack.get("context_pack_ref")
    }
    consumers = {projection.get("consumer_name") for projection in projections}
    for required in [
        "honglou-main",
        "honglou-text",
        "honglou-commentary",
        "honglou-reviewer",
    ]:
        if required not in consumers:
            errors.append(f"{trace_name}_context_projection_missing_{required}")
    for projection in projections:
        consumer = projection.get("consumer_name") or "unknown"
        if "projection_payload" in projection:
            errors.append(f"{trace_name}_{consumer}_projection_payload_exposed")
        if projection.get("consumer_type") != "runtime_profile":
            errors.append(f"{trace_name}_{consumer}_consumer_type_invalid")
        if projection.get("runtime_adapter") != "tonglingyu-runtime-adapter-v1":
            errors.append(f"{trace_name}_{consumer}_runtime_adapter_invalid")
        if projection.get("schema_version") != "tonglingyu-context-projection-v1":
            errors.append(f"{trace_name}_{consumer}_projection_schema_invalid")
        if not str(projection.get("context_projection_ref") or "").startswith(
            "context-projection://tonglingyu/"
        ):
            errors.append(f"{trace_name}_{consumer}_projection_ref_invalid")
        if pack_refs and projection.get("context_pack_ref") not in pack_refs:
            errors.append(f"{trace_name}_{consumer}_projection_pack_ref_unbound")
        for digest_field in [
            "digest",
            "tool_policy_digest",
            "output_contract_digest",
            "projection_payload_sha256",
        ]:
            if not projection.get(digest_field):
                errors.append(f"{trace_name}_{consumer}_{digest_field}_missing")
        summary = projection.get("projection_payload_summary")
        if not isinstance(summary, dict):
            errors.append(f"{trace_name}_{consumer}_projection_summary_missing")
            summary = {}
        if not summary.get("memory_read_ref_digest"):
            errors.append(f"{trace_name}_{consumer}_memory_read_ref_digest_missing")
        if consumer in {
            "honglou-text",
            "honglou-commentary",
            "honglou-reviewer",
        } and summary.get("has_session_summary") is not False:
            errors.append(f"{trace_name}_{consumer}_projection_session_summary_leak")
    main_projection = next(
        (item for item in projections if item.get("consumer_name") == "honglou-main"),
        {},
    )
    main_tools = set(main_projection.get("allowed_tools") or [])
    if "tonglingyu.evidence.package.create" not in main_tools:
        errors.append(f"{trace_name}_main_projection_package_create_missing")
    if "tonglingyu.evidence.package.read" not in main_tools:
        errors.append(f"{trace_name}_main_projection_package_read_missing")
    projection_journal = [
        entry
        for entry in context.get("session_journal") or []
        if entry.get("entry_type") == "context_projection"
    ]
    if len(projection_journal) < len(projections):
        errors.append(f"{trace_name}_context_projection_journal_missing")
    return projections


def validate_runtime_projection_binding(trace_name, trace, projections):
    if not projections:
        return
    projections_by_ref = {
        projection.get("context_projection_ref"): projection
        for projection in projections
        if projection.get("context_projection_ref")
    }
    profile_step_events = [
        item
        for item in trace.get("audit_events") or []
        if item.get("event_type") == "runtime_profile_step_completed"
    ]
    if not profile_step_events:
        errors.append(f"{trace_name}_runtime_profile_step_audit_missing")
    for event in profile_step_events:
        payload = event.get("payload") or {}
        profile = payload.get("profile") or "unknown"
        contract = payload.get("context_projection") or {}
        projection_ref = contract.get("context_projection_ref")
        projection = projections_by_ref.get(projection_ref)
        if projection is None:
            errors.append(f"{trace_name}_{profile}_runtime_step_projection_unbound")
            continue
        if contract.get("consumer_name") != profile:
            errors.append(f"{trace_name}_{profile}_runtime_step_consumer_mismatch")
        allowed = set(projection.get("allowed_tools") or [])
        for tool in payload.get("allowed_tools") or []:
            if tool not in allowed:
                errors.append(f"{trace_name}_{profile}_runtime_step_tool_outside_projection")
    agent_step_events = [
        item
        for item in trace.get("audit_events") or []
        if item.get("event_type") == "agent_runtime_profile_step_executed"
    ]
    if not agent_step_events:
        errors.append(f"{trace_name}_agent_runtime_step_audit_missing")
    for event in agent_step_events:
        payload = event.get("payload") or {}
        profile = payload.get("profile") or "unknown"
        agent_runtime = payload.get("agent_runtime") or {}
        runtime_step = agent_runtime.get("runtime_step") or {}
        context_contract = runtime_step.get("metadata", {}).get("context_contract") or {}
        projection_ref = (
            context_contract
            .get("context_projection", {})
            .get("context_projection_ref")
        )
        if projection_ref not in projections_by_ref:
            errors.append(f"{trace_name}_{profile}_agent_runtime_projection_unbound")


second_projections = validate_context_projections("second", second_trace)
long_projections = validate_context_projections("long", long_trace)
validate_runtime_projection_binding("second", second_trace, second_projections)

if not memory_ref.get("trace_id"):
    errors.append("memory_trace_ref_missing")
if memory_collector.get("object") != "tonglingyu.memory_collector_run":
    errors.append("memory_collector_object_invalid")
if memory_collector.get("status") != "ok":
    errors.append("memory_collector_status_not_ok")
if int(memory_collector.get("candidate_count") or 0) < 1:
    errors.append("memory_collector_candidate_missing")
if int(memory_collector.get("auto_enabled_count") or 0) < 1:
    errors.append("memory_collector_auto_enabled_missing")
probe = memory_collector.get("llm_extraction_probe_validation") or {}
if probe.get("status") != "pending":
    errors.append("memory_llm_probe_not_pending")
boundary = memory_collector.get("llm_boundary") or {}
if boundary.get("allowed") is not True or boundary.get("used") is not False:
    errors.append("memory_llm_boundary_invalid")
policy = memory_collector.get("memory_policy") or {}
if policy.get("policy_version") != "scoped-memory-policy-v1":
    errors.append("memory_policy_version_invalid")
candidates = memory_collector.get("candidates") or []
candidate_id = candidates[0].get("candidate_id") if candidates else ""
candidate_list_items = memory_candidates.get("items") or []
candidate = next(
    (item for item in candidate_list_items if item.get("candidate_id") == candidate_id),
    candidates[0] if candidates else {},
)
if not candidate:
    errors.append("memory_candidate_not_listed")
else:
    if candidate.get("source_entry_type") != "user_message":
        errors.append("memory_candidate_source_invalid")
    if candidate.get("scope_type") != "user_private":
        errors.append("memory_candidate_scope_invalid")
    if not str(candidate.get("scope_ref") or "").startswith("user_private:sha256:"):
        errors.append("memory_candidate_scope_ref_invalid")
    if candidate.get("status") != "approved":
        errors.append("memory_candidate_not_auto_approved")
    if candidate.get("candidate_type") != "language_preference":
        errors.append("memory_candidate_type_invalid")
    extraction = candidate.get("llm_extraction") or {}
    participation = extraction.get("llm_participation") or {}
    if participation.get("allowed") is not True or participation.get("used") is not False:
        errors.append("memory_candidate_llm_boundary_invalid")
    if "trace_id" not in candidate or "journal_id" not in candidate or "context_pack_id" not in candidate:
        errors.append("memory_candidate_traceability_missing")
cards = memory_cards.get("items") or []
card = next((item for item in cards if item.get("source_candidate_id") == candidate_id), {})
if not card:
    errors.append("memory_card_not_created")
else:
    if card.get("status") != "active":
        errors.append("memory_card_status_invalid")
    if card.get("read_enabled") is not True:
        errors.append("memory_card_not_read_enabled")
    acl = card.get("acl") or {}
    if acl.get("policy_version") != "scoped-memory-policy-v1":
        errors.append("memory_card_acl_policy_missing")
if not memory_read_ref.get("trace_id"):
    errors.append("memory_read_trace_ref_missing")
memory_read_context = memory_read_trace.get("scoped_context") or {}
memory_read_packs = memory_read_context.get("context_packs") or []
if not any(pack.get("memory_read_refs") for pack in memory_read_packs):
    errors.append("memory_read_context_refs_missing")
memory_read_projections = memory_read_context.get("context_projections") or []
if not any(
    item.get("consumer_name") == "honglou-main"
    and (item.get("projection_payload_summary") or {}).get("memory_read_ref_count", 0) >= 1
    for item in memory_read_projections
):
    errors.append("memory_read_main_projection_missing")
if any(
    item.get("consumer_name") == "honglou-reviewer"
    and (item.get("projection_payload_summary") or {}).get("memory_summary_count", 0) > 0
    for item in memory_read_projections
):
    errors.append("memory_read_reviewer_content_leak")

for trace_name, trace in [("second", second_trace), ("long", long_trace)]:
    context = trace.get("scoped_context") or {}
    if context.get("raw_content_included") is not False:
        errors.append(f"{trace_name}_raw_content_flag_invalid")
    trace_text = rendered(trace)
    if '"content":' in trace_text:
        errors.append(f"{trace_name}_admin_trace_exposes_journal_content")
    if '"entry_type":"memory_candidate_created"' in trace_text:
        errors.append(f"{trace_name}_created_memory_candidate")
    if "memory_card" in trace_text:
        errors.append(f"{trace_name}_mentions_memory_card")
    if '"projection_payload":' in trace_text:
        errors.append(f"{trace_name}_admin_trace_exposes_projection_payload")

long_context = long_trace.get("scoped_context") or {}
long_packs = long_context.get("context_packs") or []
if not long_packs:
    errors.append("long_context_pack_missing")
else:
    summary = str(long_packs[-1].get("session_summary") or "")
    if "历史消息" not in summary:
        errors.append("long_session_summary_missing_history")

long_journal = long_context.get("session_journal") or []
metadata_entries = [
    entry for entry in long_journal if entry.get("entry_type") == "metadata_prompt"
]
if not metadata_entries:
    errors.append("long_metadata_prompt_journal_missing")
else:
    metadata = metadata_entries[-1].get("metadata") or {}
    if metadata.get("history_over_limit") is not True:
        errors.append("long_history_over_limit_not_recorded")
    if metadata.get("max_messages") != max_messages:
        errors.append("long_max_messages_not_recorded")

checks = {
    "multi_turn_resolved_question": "second_resolved_question_unexpected" not in errors,
    "long_history_session_summary": "long_session_summary_missing_history" not in errors,
    "context_pack_trace_replay": bool(second_packs and long_packs),
    "context_projection_trace_replay": bool(second_projections and long_projections),
    "runtime_projection_binding": not any(
        "projection_unbound" in error
        or "projection_pack_ref_unbound" in error
        or "runtime_step_consumer_mismatch" in error
        or "runtime_step_tool_outside_projection" in error
        for error in errors
    ),
    "public_response_redacted": not any("public_response_leaks" in error for error in errors),
    "profile_history_isolation": not any(
        "must_not_get_session_summary" in error
        or "projection_session_summary_leak" in error
        for error in errors
    ),
    "journal_raw_content_hidden": not any(
        "admin_trace_exposes_journal_content" in error
        or "admin_trace_exposes_projection_payload" in error
        for error in errors
    ),
    "memory_scoped_policy_gate": not any(
        error.startswith("memory_") for error in errors
    ),
    "memory_read_path_enabled": not any(
        error
        in {
            "memory_card_not_read_enabled",
            "memory_read_context_refs_missing",
            "memory_read_main_projection_missing",
            "memory_read_reviewer_content_leak",
        }
        for error in errors
    ),
}
payload = {
    "object": "tonglingyu.scoped_context_live_gate",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "run_id": run_id,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "max_messages": max_messages,
    "second_trace_id": second_ref.get("trace_id"),
    "long_trace_id": long_ref.get("trace_id"),
    "memory_trace_id": memory_ref.get("trace_id"),
    "memory_read_trace_id": memory_read_ref.get("trace_id"),
    "second_context_pack_id_sha256": hashlib.sha256(
        str(second_ref.get("context_pack_id") or "").encode("utf-8")
    ).hexdigest(),
    "artifact_sha256": {
        "health": file_sha256(health_path),
        "first_response": file_sha256(first_path),
        "second_response": file_sha256(second_path),
        "long_response": file_sha256(long_path),
        "memory_response": file_sha256(memory_path),
        "memory_read_response": file_sha256(memory_read_path),
        "second_trace": file_sha256(second_trace_path),
        "long_trace": file_sha256(long_trace_path),
        "memory_read_trace": file_sha256(memory_read_trace_path),
        "memory_collector": file_sha256(memory_collector_path),
        "memory_candidates": file_sha256(memory_candidates_path),
        "memory_cards": file_sha256(memory_cards_path),
    },
    "checks": checks,
    "errors": errors,
    "production_ready_proven": False,
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if not errors else 1)
PY
