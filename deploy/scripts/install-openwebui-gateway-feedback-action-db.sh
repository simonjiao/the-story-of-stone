#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -d "${SCRIPT_DIR}/../open-webui/functions" ]]; then
  DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
else
  DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)/deploy"
fi

COMPOSE_SERVICE="${OPEN_WEBUI_COMPOSE_SERVICE:-open-webui}"
CONTAINER_PATH="/tmp/tonglingyu_gateway_feedback_action.py"
FUNCTION_FILE="${FUNCTION_FILE:-${DEPLOY_DIR}/open-webui/functions/tonglingyu_gateway_feedback_action.py}"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_deploy_env_file_or_local

if [[ -z "${TONGLINGYU_GATEWAY_API_KEY:-}" ]]; then
  echo "TONGLINGYU_GATEWAY_API_KEY is required" >&2
  exit 1
fi

docker compose cp "${FUNCTION_FILE}" "${COMPOSE_SERVICE}:${CONTAINER_PATH}" >/dev/null
docker compose exec -T \
  -e TONGLINGYU_GATEWAY_API_KEY \
  -e TONGLINGYU_GATEWAY_ADMIN_BASE_URL \
  -e TONGLINGYU_GATEWAY_FEEDBACK_ACTION_TARGET_MODEL \
  -e TONGLINGYU_GATEWAY_FEEDBACK_ACTION_TARGET_MODELS \
  -e TONGLINGYU_GATEWAY_FEEDBACK_ACTION_TIMEOUT \
  -e TONGLINGYU_GATEWAY_FEEDBACK_ACTION_MAX_CHARS \
  -e TONGLINGYU_MODEL_ID \
  "${COMPOSE_SERVICE}" \
  python3 - <<'PY'
from pathlib import Path
import json
import os
import sqlite3
import time

content = Path("/tmp/tonglingyu_gateway_feedback_action.py").read_text()
conn = sqlite3.connect("/app/backend/data/webui.db")
cur = conn.cursor()
admin = cur.execute(
    "select id from user where role = ? order by created_at limit 1",
    ("admin",),
).fetchone()
if not admin:
    raise SystemExit("no admin user found")

target_model = os.environ.get(
    "TONGLINGYU_GATEWAY_FEEDBACK_ACTION_TARGET_MODEL",
    os.environ.get("TONGLINGYU_MODEL_ID", "tonglingyu"),
)
target_models = os.environ.get(
    "TONGLINGYU_GATEWAY_FEEDBACK_ACTION_TARGET_MODELS",
    target_model,
)
now = int(time.time())
meta = json.dumps(
    {"description": "Queues Tonglingyu Gateway feedback from Open WebUI."},
    ensure_ascii=False,
)
valves = json.dumps(
    {
        "GATEWAY_BASE_URL": os.environ.get(
            "TONGLINGYU_GATEWAY_ADMIN_BASE_URL",
            "http://tonglingyu-gateway:8090",
        ),
        "GATEWAY_API_KEY": os.environ["TONGLINGYU_GATEWAY_API_KEY"],
        "TARGET_MODEL": target_model,
        "TARGET_MODELS": target_models,
        "REQUEST_TIMEOUT_SECONDS": int(
            os.environ.get("TONGLINGYU_GATEWAY_FEEDBACK_ACTION_TIMEOUT", "15")
        ),
        "RESPONSE_MAX_CHARS": int(
            os.environ.get("TONGLINGYU_GATEWAY_FEEDBACK_ACTION_MAX_CHARS", "3000")
        ),
    },
    ensure_ascii=False,
)
cur.execute(
    """
    insert into function(id, user_id, name, type, content, meta, created_at, updated_at, valves, is_active, is_global)
    values(?,?,?,?,?,?,?,?,?,?,?)
    on conflict(id) do update set
      user_id=excluded.user_id,
      name=excluded.name,
      type=excluded.type,
      content=excluded.content,
      meta=excluded.meta,
      updated_at=excluded.updated_at,
      valves=excluded.valves,
      is_active=excluded.is_active,
      is_global=excluded.is_global
    """,
    (
        "tonglingyu_gateway_feedback",
        admin[0],
        "Tonglingyu Feedback",
        "action",
        content,
        meta,
        now,
        now,
        valves,
        1,
        1,
    ),
)
conn.commit()
print("function_upserted=tonglingyu_gateway_feedback")
PY

docker compose restart "${COMPOSE_SERVICE}" >/dev/null
echo "open-webui_restarted=true"
