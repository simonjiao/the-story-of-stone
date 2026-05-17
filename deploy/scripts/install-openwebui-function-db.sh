#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -d "${SCRIPT_DIR}/../open-webui/functions" ]]; then
  DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
else
  DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)/deploy"
fi

COMPOSE_SERVICE="${OPEN_WEBUI_COMPOSE_SERVICE:-open-webui}"
CONTAINER_PATH="/tmp/agent_identity_bridge_filter.py"
FUNCTION_FILE="${FUNCTION_FILE:-${DEPLOY_DIR}/open-webui/functions/agent_identity_bridge_filter.py}"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_deploy_env_file_or_local

if [[ -z "${AGENT_BRIDGE_SECRET:-}" ]]; then
  echo "AGENT_BRIDGE_SECRET is required" >&2
  exit 1
fi

docker compose cp "${FUNCTION_FILE}" "${COMPOSE_SERVICE}:${CONTAINER_PATH}" >/dev/null
docker compose exec -T \
  -e AGENT_BRIDGE_SECRET \
  -e AGENT_BRIDGE_ISSUER \
  -e AGENT_BRIDGE_TARGET_MODEL \
  -e AGENT_BRIDGE_TARGET_MODELS \
  "${COMPOSE_SERVICE}" \
  python3 - <<'PY'
from pathlib import Path
import json
import os
import sqlite3
import time

content = Path("/tmp/agent_identity_bridge_filter.py").read_text()
conn = sqlite3.connect("/app/backend/data/webui.db")
cur = conn.cursor()
admin = cur.execute(
    "select id from user where role = ? order by created_at limit 1",
    ("admin",),
).fetchone()
if not admin:
    raise SystemExit("no admin user found")

now = int(time.time())
meta = json.dumps(
    {"description": "Injects signed Open WebUI identity context for Tonglingyu Agent requests."},
    ensure_ascii=False,
)
valves = json.dumps(
    {
        "AGENT_BRIDGE_SECRET": os.environ["AGENT_BRIDGE_SECRET"],
        "AGENT_BRIDGE_ISSUER": os.environ.get("AGENT_BRIDGE_ISSUER", "open-webui"),
        "TARGET_MODEL": os.environ.get("AGENT_BRIDGE_TARGET_MODEL", "tonglingyu"),
        "TARGET_MODELS": os.environ.get(
            "AGENT_BRIDGE_TARGET_MODELS",
            os.environ.get("AGENT_BRIDGE_TARGET_MODEL", "tonglingyu"),
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
        "agent_identity_bridge",
        admin[0],
        "Agent Identity Bridge",
        "filter",
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
print("function_upserted=agent_identity_bridge")
PY

docker compose restart "${COMPOSE_SERVICE}" >/dev/null
echo "open-webui_restarted=true"
