#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

FUNCTION_ID="${FUNCTION_ID:-tonglingyu_gateway_admin}"
BASE_URL="${OPEN_WEBUI_BASE_URL:-${PUBLIC_WEBUI_URL:-}}"
ADMIN_TOKEN="${OPEN_WEBUI_ADMIN_TOKEN:-}"
COMPOSE_SERVICE="${OPEN_WEBUI_COMPOSE_SERVICE:-open-webui}"
FIXTURE_JSON="${OPEN_WEBUI_GATEWAY_ADMIN_ACTION_VERIFY_JSON:-}"

if [[ -n "${FIXTURE_JSON}" ]]; then
  python3 - "$FUNCTION_ID" "$FIXTURE_JSON" <<'PY'
import json
import sys

function_id, fixture_path = sys.argv[1:3]
with open(fixture_path, "r", encoding="utf-8") as handle:
    fixture = json.load(handle)
function = fixture.get("function") or fixture
valves = function.get("valves") or {}
if isinstance(valves, str):
    try:
        valves = json.loads(valves)
    except json.JSONDecodeError:
        valves = {}
required_valves = [
    "GATEWAY_BASE_URL",
    "GATEWAY_ADMIN_API_KEY",
    "TARGET_MODEL",
    "TARGET_MODELS",
]
valve_keys = sorted(str(key) for key in valves.keys())
content = function.get("content") or ""
missing = [key for key in required_valves if key not in valves]
empty = [
    key
    for key in required_valves
    if key in valves and not str(valves.get(key) or "").strip()
]
errors = []
if function.get("id", function_id) != function_id:
    errors.append(f"id={function.get('id')!r}")
if function.get("type") != "action":
    errors.append(f"type={function.get('type')!r}")
if not bool(function.get("is_active")):
    errors.append("is_active=false")
if not bool(function.get("is_global")):
    errors.append("is_global=false")
if "class Action" not in content or "GATEWAY_ADMIN_API_KEY" not in content:
    errors.append("content_missing_gateway_admin_action")
if "__user__" not in content or "role" not in content or "admin" not in content:
    errors.append("content_missing_admin_role_guard")
if "actions =" not in content or "/v1/admin/metrics" not in content:
    errors.append("content_missing_admin_actions")
if missing:
    errors.append("missing_valves=" + ",".join(missing))
if empty:
    errors.append("empty_valves=" + ",".join(empty))

print(
    json.dumps(
        {
            "function_id": function_id,
            "source": "fixture-json",
            "type": function.get("type"),
            "is_active": bool(function.get("is_active")),
            "is_global": bool(function.get("is_global")),
            "valve_keys": valve_keys,
            "status": "ok" if not errors else "failed",
            "errors": errors,
        },
        ensure_ascii=False,
    )
)
if errors:
    raise SystemExit(1)
PY
  exit 0
fi

if [[ -n "${ADMIN_TOKEN}" && -z "${BASE_URL}" ]]; then
  echo "OPEN_WEBUI_BASE_URL or PUBLIC_WEBUI_URL is required" >&2
  exit 1
fi

if [[ -z "${ADMIN_TOKEN}" ]]; then
  if [[ ! -f "docker-compose.yml" && ! -f "compose.yml" ]]; then
    echo "OPEN_WEBUI_ADMIN_TOKEN is required outside a compose deploy directory" >&2
    exit 1
  fi
  docker compose exec -T "${COMPOSE_SERVICE}" python3 - "$FUNCTION_ID" <<'PY'
import json
import sqlite3
import sys

function_id = sys.argv[1]
conn = sqlite3.connect("/app/backend/data/webui.db")
conn.row_factory = sqlite3.Row
row = conn.execute(
    "select id, type, content, valves, is_active, is_global from function where id = ?",
    (function_id,),
).fetchone()
if row is None:
    raise SystemExit(f"function {function_id!r} not found")

valves = row["valves"] or "{}"
try:
    valves = json.loads(valves)
except json.JSONDecodeError:
    valves = {}
valve_keys = sorted(str(key) for key in valves.keys())
content = row["content"] or ""
missing = [
    key
    for key in ["GATEWAY_BASE_URL", "GATEWAY_ADMIN_API_KEY", "TARGET_MODEL", "TARGET_MODELS"]
    if key not in valves
]
empty = [
    key
    for key in ["GATEWAY_BASE_URL", "GATEWAY_ADMIN_API_KEY", "TARGET_MODEL", "TARGET_MODELS"]
    if key in valves and not str(valves.get(key) or "").strip()
]
errors = []
if row["type"] != "action":
    errors.append(f"type={row['type']!r}")
if not bool(row["is_active"]):
    errors.append("is_active=false")
if not bool(row["is_global"]):
    errors.append("is_global=false")
if "class Action" not in content or "GATEWAY_ADMIN_API_KEY" not in content:
    errors.append("content_missing_gateway_admin_action")
if "__user__" not in content or "role" not in content or "admin" not in content:
    errors.append("content_missing_admin_role_guard")
if "actions =" not in content or "/v1/admin/metrics" not in content:
    errors.append("content_missing_admin_actions")
if missing:
    errors.append("missing_valves=" + ",".join(missing))
if empty:
    errors.append("empty_valves=" + ",".join(empty))

print(
    json.dumps(
        {
            "function_id": function_id,
            "source": "compose-db",
            "type": row["type"],
            "is_active": bool(row["is_active"]),
            "is_global": bool(row["is_global"]),
            "valve_keys": valve_keys,
            "status": "ok" if not errors else "failed",
            "errors": errors,
        },
        ensure_ascii=False,
    )
)
if errors:
    raise SystemExit(1)
PY
  exit 0
fi

python3 - "$BASE_URL" "$ADMIN_TOKEN" "$FUNCTION_ID" <<'PY'
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

base_url, token, function_id = sys.argv[1:4]
base_url = base_url.rstrip("/")


def request(path: str):
    req = urllib.request.Request(
        f"{base_url}{path}",
        headers={"Authorization": f"Bearer {token}"},
        method="GET",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            raw = response.read().decode("utf-8")
            return response.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8", errors="replace")
        raise SystemExit(f"Open WebUI API GET {path} failed: HTTP {error.code}: {raw[:500]}")


encoded_id = urllib.parse.quote(function_id, safe="")
status, function = request(f"/api/v1/functions/id/{encoded_id}")
if status != 200:
    raise SystemExit(f"function lookup failed: HTTP {status}")

export = {}
try:
    _, export = request(f"/api/v1/functions/id/{encoded_id}/export?include_valves=true")
except SystemExit:
    export = {}

function_type = function.get("type")
is_active = bool(function.get("is_active"))
is_global = bool(function.get("is_global"))
content = function.get("content") or ""
valves = export.get("valves") or function.get("valves") or {}
if isinstance(valves, str):
    try:
        valves = json.loads(valves)
    except json.JSONDecodeError:
        valves = {}
valve_keys = sorted(str(key) for key in valves.keys())

missing = [
    key
    for key in ["GATEWAY_BASE_URL", "GATEWAY_ADMIN_API_KEY", "TARGET_MODEL", "TARGET_MODELS"]
    if key not in valves
]
empty = [
    key
    for key in ["GATEWAY_BASE_URL", "GATEWAY_ADMIN_API_KEY", "TARGET_MODEL", "TARGET_MODELS"]
    if key in valves and not str(valves.get(key) or "").strip()
]
errors = []
if function_type != "action":
    errors.append(f"type={function_type!r}")
if not is_active:
    errors.append("is_active=false")
if not is_global:
    errors.append("is_global=false")
if "class Action" not in content or "GATEWAY_ADMIN_API_KEY" not in content:
    errors.append("content_missing_gateway_admin_action")
if "__user__" not in content or "role" not in content or "admin" not in content:
    errors.append("content_missing_admin_role_guard")
if "actions =" not in content or "/v1/admin/metrics" not in content:
    errors.append("content_missing_admin_actions")
if missing:
    errors.append("missing_valves=" + ",".join(missing))
if empty:
    errors.append("empty_valves=" + ",".join(empty))

print(
    json.dumps(
        {
            "function_id": function_id,
            "source": "admin-api",
            "type": function_type,
            "is_active": is_active,
            "is_global": is_global,
            "valve_keys": valve_keys,
            "status": "ok" if not errors else "failed",
            "errors": errors,
        },
        ensure_ascii=False,
    )
)
if errors:
    raise SystemExit(1)
PY
