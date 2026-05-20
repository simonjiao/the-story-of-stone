#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

FUNCTION_FILE="${FUNCTION_FILE:-${OPEN_WEBUI_FUNCTION_DIR}/agent_identity_bridge_filter.py}"
FUNCTION_ID="${FUNCTION_ID:-agent_identity_bridge}"
FUNCTION_NAME="${FUNCTION_NAME:-Agent Identity Bridge}"
TARGET_MODEL="${AGENT_BRIDGE_TARGET_MODEL:-tonglingyu}"
TARGET_MODELS="${AGENT_BRIDGE_TARGET_MODELS:-${TARGET_MODEL}}"
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

if [[ -z "${AGENT_BRIDGE_SECRET:-}" ]]; then
  echo "AGENT_BRIDGE_SECRET is required" >&2
  exit 1
fi

python3 - "$BASE_URL" "$ADMIN_TOKEN" "$FUNCTION_FILE" "$FUNCTION_ID" "$FUNCTION_NAME" "$TARGET_MODEL" "$TARGET_MODELS" <<'PY'
import json
import os
import sys
import urllib.error
import urllib.request

base_url, token, function_file, function_id, function_name, target_model, target_models = sys.argv[1:8]
base_url = base_url.rstrip("/")
with open(function_file, "r", encoding="utf-8") as handle:
    content = handle.read()


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        f"{base_url}{path}",
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            raw = response.read().decode("utf-8")
            try:
                parsed = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                parsed = {"raw": raw}
            return response.status, parsed
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8", errors="replace")
        if error.code == 404:
            return error.code, None
        raise SystemExit(f"Open WebUI API {method} {path} failed: HTTP {error.code}: {raw[:500]}")


payload = {
    "id": function_id,
    "name": function_name,
    "type": "filter",
    "content": content,
    "meta": {
        "description": "Injects signed Open WebUI identity context for Tonglingyu Agent requests.",
    },
    "is_active": True,
    "is_global": True,
}

api_prefix = "/api/v1/functions"

status, _ = request("GET", f"{api_prefix}/id/{function_id}")
if status == 404:
    request("POST", f"{api_prefix}/create", payload)
    action = "created"
else:
    request("POST", f"{api_prefix}/id/{function_id}/update", payload)
    action = "updated"

request(
    "POST",
    f"{api_prefix}/id/{function_id}/valves/update",
    {
        "AGENT_BRIDGE_SECRET": os.environ["AGENT_BRIDGE_SECRET"],
        "AGENT_BRIDGE_ISSUER": os.environ.get("AGENT_BRIDGE_ISSUER", "open-webui"),
        "TARGET_MODEL": target_model,
        "TARGET_MODELS": target_models,
    },
)

print(json.dumps(
    {
        "function_id": function_id,
        "action": action,
        "target_model": target_model,
        "target_models": target_models,
    }
))
PY
