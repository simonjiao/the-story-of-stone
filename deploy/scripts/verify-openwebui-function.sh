#!/usr/bin/env bash
set -euo pipefail

FUNCTION_ID="${FUNCTION_ID:-agent_identity_bridge}"
BASE_URL="${OPEN_WEBUI_BASE_URL:-${PUBLIC_WEBUI_URL:-}}"
ADMIN_TOKEN="${OPEN_WEBUI_ADMIN_TOKEN:-}"

if [[ -z "${BASE_URL}" ]]; then
  echo "OPEN_WEBUI_BASE_URL or PUBLIC_WEBUI_URL is required" >&2
  exit 1
fi

if [[ -z "${ADMIN_TOKEN}" ]]; then
  echo "OPEN_WEBUI_ADMIN_TOKEN is required" >&2
  exit 1
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
    for key in ["AGENT_BRIDGE_SECRET", "AGENT_BRIDGE_ISSUER", "TARGET_MODEL"]
    if key not in valves
]
errors = []
if function_type != "filter":
    errors.append(f"type={function_type!r}")
if not is_active:
    errors.append("is_active=false")
if not is_global:
    errors.append("is_global=false")
if "class Filter" not in content or "agent_bridge_context" not in content:
    errors.append("content_missing_bridge_filter")
if missing:
    errors.append("missing_valves=" + ",".join(missing))

print(
    json.dumps(
        {
            "function_id": function_id,
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
