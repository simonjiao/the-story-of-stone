#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -d "${SCRIPT_DIR}/../open-webui/functions" ]]; then
  DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
else
  DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)/deploy"
fi

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

FUNCTION_FILE="${FUNCTION_FILE:-${DEPLOY_DIR}/open-webui/functions/tonglingyu_gateway_admin_action.py}"
FUNCTION_ID="${FUNCTION_ID:-tonglingyu_gateway_admin}"
FUNCTION_NAME="${FUNCTION_NAME:-Tonglingyu Gateway Admin}"
TARGET_MODEL="${TONGLINGYU_GATEWAY_ADMIN_ACTION_TARGET_MODEL:-${TONGLINGYU_MODEL_ID:-tonglingyu}}"
TARGET_MODELS="${TONGLINGYU_GATEWAY_ADMIN_ACTION_TARGET_MODELS:-${TARGET_MODEL}}"
GATEWAY_BASE_URL="${TONGLINGYU_GATEWAY_ADMIN_BASE_URL:-http://tonglingyu-gateway:8090}"
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

if [[ -z "${TONGLINGYU_ADMIN_API_KEY:-}" ]]; then
  echo "TONGLINGYU_ADMIN_API_KEY is required" >&2
  exit 1
fi

python3 - "$BASE_URL" "$ADMIN_TOKEN" "$FUNCTION_FILE" "$FUNCTION_ID" "$FUNCTION_NAME" "$TARGET_MODEL" "$TARGET_MODELS" "$GATEWAY_BASE_URL" <<'PY'
import json
import os
import sys
import urllib.error
import urllib.request

(
    base_url,
    token,
    function_file,
    function_id,
    function_name,
    target_model,
    target_models,
    gateway_base_url,
) = sys.argv[1:9]
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
    "type": "action",
    "content": content,
    "meta": {
        "description": "Read-only Tonglingyu Gateway admin entry for Open WebUI admins.",
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
        "GATEWAY_BASE_URL": gateway_base_url,
        "GATEWAY_ADMIN_API_KEY": os.environ["TONGLINGYU_ADMIN_API_KEY"],
        "TARGET_MODEL": target_model,
        "TARGET_MODELS": target_models,
        "REQUEST_TIMEOUT_SECONDS": int(
            os.environ.get("TONGLINGYU_GATEWAY_ADMIN_ACTION_TIMEOUT", "15")
        ),
        "RESPONSE_MAX_CHARS": int(
            os.environ.get("TONGLINGYU_GATEWAY_ADMIN_ACTION_MAX_CHARS", "6000")
        ),
    },
)

print(json.dumps(
    {
        "function_id": function_id,
        "action": action,
        "target_model": target_model,
        "target_models": target_models,
        "gateway_base_url": gateway_base_url,
    }
))
PY
