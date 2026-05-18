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
HEALTH_JSON="${TONGLINGYU_SCOPED_CONTEXT_HEALTH_JSON:-${WORK_DIR}/health.json}"
FIRST_PAYLOAD="${WORK_DIR}/first-payload.json"
SECOND_PAYLOAD="${WORK_DIR}/second-payload.json"
LONG_PAYLOAD="${WORK_DIR}/long-payload.json"
FIRST_JSON="${TONGLINGYU_SCOPED_CONTEXT_FIRST_JSON:-${WORK_DIR}/first.json}"
SECOND_JSON="${TONGLINGYU_SCOPED_CONTEXT_SECOND_JSON:-${WORK_DIR}/second.json}"
LONG_JSON="${TONGLINGYU_SCOPED_CONTEXT_LONG_JSON:-${WORK_DIR}/long.json}"
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
  -H "x-tonglingyu-user-id: scoped-context-live" \
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
  -H "x-tonglingyu-user-id: scoped-context-live" \
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
  -H "x-tonglingyu-user-id: scoped-context-live" \
  -H "x-tonglingyu-chat-id: scoped-context-live-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: scoped-context-live-long-${VERIFY_RUN_ID}" \
  --data-binary @- \
  http://tonglingyu-gateway:8090/v1/chat/completions
' <"${LONG_PAYLOAD}" >"${LONG_JSON}"

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

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/traces/'"${SECOND_TRACE_ID}"'"
' >"${SECOND_TRACE_JSON}"

docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY:-}" open-webui sh -lc '
test -n "${TLY_ADMIN_KEY}"
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" "http://tonglingyu-gateway:8090/v1/admin/traces/'"${LONG_TRACE_ID}"'"
' >"${LONG_TRACE_JSON}"

python3 - "${HEALTH_JSON}" "${FIRST_JSON}" "${SECOND_JSON}" "${LONG_JSON}" \
  "${SECOND_REF_JSON}" "${LONG_REF_JSON}" "${SECOND_TRACE_JSON}" "${LONG_TRACE_JSON}" \
  "${MAX_MESSAGES}" "${RUN_ID}" <<'PY'
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
    second_ref_path,
    long_ref_path,
    second_trace_path,
    long_trace_path,
    max_messages_raw,
    run_id,
) = sys.argv[1:11]


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
second_ref = load(second_ref_path)
long_ref = load(long_ref_path)
second_trace = load(second_trace_path)
long_trace = load(long_trace_path)
max_messages = int(max_messages_raw)
errors = []

for name, response in [("first", first), ("second", second), ("long", long_response)]:
    if response.get("object") != "chat.completion":
        errors.append(f"{name}_response_object_invalid")
    public_text = rendered(response)
    for forbidden in [
        "context_pack_id",
        "interaction_context_id",
        "user_session_id",
        "session_journal",
        "memory_read_refs",
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
    if pack.get("memory_read_refs") != []:
        errors.append("second_memory_read_refs_must_be_empty")
    for view in pack.get("profile_views") or []:
        if view.get("profile_name") in {
            "honglou-text",
            "honglou-commentary",
            "honglou-reviewer",
        } and view.get("session_summary") is not None:
            errors.append(f"{view.get('profile_name')}_must_not_get_session_summary")

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
    "public_response_redacted": not any("public_response_leaks" in error for error in errors),
    "profile_history_isolation": not any("must_not_get_session_summary" in error for error in errors),
    "journal_raw_content_hidden": not any("admin_trace_exposes_journal_content" in error for error in errors),
    "no_memory_phase1": not any("memory" in error for error in errors),
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
    "second_context_pack_id_sha256": hashlib.sha256(
        str(second_ref.get("context_pack_id") or "").encode("utf-8")
    ).hexdigest(),
    "artifact_sha256": {
        "health": file_sha256(health_path),
        "first_response": file_sha256(first_path),
        "second_response": file_sha256(second_path),
        "long_response": file_sha256(long_path),
        "second_trace": file_sha256(second_trace_path),
        "long_trace": file_sha256(long_trace_path),
    },
    "checks": checks,
    "errors": errors,
    "production_ready_proven": False,
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if not errors else 1)
PY
