#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
CONFIG_JSON="$(mktemp)"
trap 'rm -f "${CONFIG_JSON}"' EXIT

cd "${DEPLOY_DIR}"

if ! docker compose config --format json >"${CONFIG_JSON}"; then
  echo "compose_config=failed" >&2
  echo "hint=set the required deploy/.env variables or export dummy values for dry-run verification" >&2
  exit 1
fi

python3 - "${CONFIG_JSON}" <<'PY'
import json
import sys

config_path = sys.argv[1]
with open(config_path, "r", encoding="utf-8") as handle:
    config = json.load(handle)

errors = []
services = config.get("services") or {}


def env_map(service_name):
    service = services.get(service_name)
    if not service:
        errors.append(f"service_missing={service_name}")
        return {}
    raw_env = service.get("environment") or {}
    if isinstance(raw_env, dict):
        return {
            str(key): "" if value is None else str(value)
            for key, value in raw_env.items()
        }
    if isinstance(raw_env, list):
        result = {}
        for item in raw_env:
            text = str(item)
            if "=" in text:
                key, value = text.split("=", 1)
                result[key] = value
        return result
    errors.append(f"environment_invalid={service_name}")
    return {}


def value(env, key):
    return str(env.get(key) or "").strip()


def key_set(primary, additional):
    keys = set()
    for item in [primary, additional]:
        for part in str(item or "").split(","):
            candidate = part.strip()
            if candidate:
                keys.add(candidate)
    return keys


def provider_key_set(value_text):
    keys = set()
    for item in str(value_text or "").replace(",", ";").split(";"):
        candidate = item.strip()
        if candidate:
            keys.add(candidate)
    return keys


def require_false(env, key, service_name):
    actual = value(env, key).lower()
    if actual not in {"false", "0", "no", "off"}:
        errors.append(f"{service_name}.{key} must be false/0/no/off for production verification")


gateway_env = env_map("tonglingyu-gateway")
open_webui_env = env_map("open-webui")
hermes_env = env_map("hermes")
worker_env = env_map("agent-worker")
orchestrator_env = env_map("agent-orchestrator")

gateway_api_key = value(gateway_env, "TONGLINGYU_GATEWAY_API_KEY")
gateway_api_keys = value(gateway_env, "TONGLINGYU_GATEWAY_API_KEYS")
admin_api_key = value(gateway_env, "TONGLINGYU_ADMIN_API_KEY")
admin_api_keys = value(gateway_env, "TONGLINGYU_ADMIN_API_KEYS")
gateway_key_set = key_set(gateway_api_key, gateway_api_keys)
admin_key_set = key_set(admin_api_key, admin_api_keys)

if not gateway_key_set:
    errors.append("tonglingyu-gateway.TONGLINGYU_GATEWAY_API_KEY(S) must be configured")
if not admin_key_set:
    errors.append("tonglingyu-gateway.TONGLINGYU_ADMIN_API_KEY(S) must be configured")
if gateway_key_set.intersection(admin_key_set):
    errors.append("TONGLINGYU gateway and admin API key sets must not overlap")

require_false(gateway_env, "TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY", "tonglingyu-gateway")

open_webui_provider_keys = provider_key_set(value(open_webui_env, "OPENAI_API_KEYS"))
if gateway_api_key and gateway_api_key not in open_webui_provider_keys:
    errors.append("open-webui.OPENAI_API_KEYS must include TONGLINGYU_GATEWAY_API_KEY")
if admin_key_set.intersection(open_webui_provider_keys):
    errors.append("open-webui.OPENAI_API_KEYS must not contain TONGLINGYU_ADMIN_API_KEY(S)")

open_webui_admin_api_key = value(open_webui_env, "TONGLINGYU_ADMIN_API_KEY")
if open_webui_admin_api_key and open_webui_admin_api_key != admin_api_key:
    errors.append("open-webui.TONGLINGYU_ADMIN_API_KEY must match tonglingyu-gateway.TONGLINGYU_ADMIN_API_KEY")

if value(open_webui_env, "DEFAULT_MODELS") != "tonglingyu":
    errors.append("open-webui.DEFAULT_MODELS must be tonglingyu")
if "http://tonglingyu-gateway:8090/v1" not in value(open_webui_env, "OPENAI_API_BASE_URLS").split(";"):
    errors.append("open-webui.OPENAI_API_BASE_URLS must include http://tonglingyu-gateway:8090/v1")

hermes_api_key = value(hermes_env, "API_SERVER_KEY")
if not hermes_api_key:
    errors.append("hermes.API_SERVER_KEY must be configured")
if value(gateway_env, "TONGLINGYU_UPSTREAM_BASE_URL") != "http://hermes:8642/v1":
    errors.append("tonglingyu-gateway.TONGLINGYU_UPSTREAM_BASE_URL must be http://hermes:8642/v1")
if hermes_api_key and value(gateway_env, "TONGLINGYU_UPSTREAM_API_KEY") != hermes_api_key:
    errors.append("tonglingyu-gateway.TONGLINGYU_UPSTREAM_API_KEY must match hermes.API_SERVER_KEY")
if value(gateway_env, "TONGLINGYU_AGENT_RUNTIME_MODE") != "hermes":
    errors.append("tonglingyu-gateway.TONGLINGYU_AGENT_RUNTIME_MODE must be hermes")
if value(gateway_env, "AGENT_RUNTIME_HERMES_BASE_URL") != "http://hermes:8642/v1":
    errors.append("tonglingyu-gateway.AGENT_RUNTIME_HERMES_BASE_URL must be http://hermes:8642/v1")
if value(gateway_env, "AGENT_RUNTIME_HERMES_MODEL") != value(gateway_env, "TONGLINGYU_UPSTREAM_MODEL"):
    errors.append("tonglingyu-gateway.AGENT_RUNTIME_HERMES_MODEL must match TONGLINGYU_UPSTREAM_MODEL")
if hermes_api_key and value(gateway_env, "AGENT_RUNTIME_HERMES_API_KEY") != hermes_api_key:
    errors.append("tonglingyu-gateway.AGENT_RUNTIME_HERMES_API_KEY must match hermes.API_SERVER_KEY")

if value(worker_env, "AGENT_RUNTIME_MODE") != "hermes":
    errors.append("agent-worker.AGENT_RUNTIME_MODE must be hermes")
if value(worker_env, "AGENT_RUNTIME_HERMES_BASE_URL") != "http://hermes:8642/v1":
    errors.append("agent-worker.AGENT_RUNTIME_HERMES_BASE_URL must be http://hermes:8642/v1")
if hermes_api_key and value(worker_env, "AGENT_RUNTIME_HERMES_API_KEY") != hermes_api_key:
    errors.append("agent-worker.AGENT_RUNTIME_HERMES_API_KEY must match hermes.API_SERVER_KEY")

if value(orchestrator_env, "AGENT_ORCHESTRATOR_UPSTREAM_BASE_URL") != "http://hermes:8642/v1":
    errors.append("agent-orchestrator.AGENT_ORCHESTRATOR_UPSTREAM_BASE_URL must be http://hermes:8642/v1")
if hermes_api_key and value(orchestrator_env, "AGENT_ORCHESTRATOR_UPSTREAM_API_KEY") != hermes_api_key:
    errors.append("agent-orchestrator.AGENT_ORCHESTRATOR_UPSTREAM_API_KEY must match hermes.API_SERVER_KEY")

if errors:
    for error in errors:
        print(f"config_error={error}", file=sys.stderr)
    sys.exit(1)

print(json.dumps(
    {
        "status": "ok",
        "checked_services": [
            "hermes",
            "open-webui",
            "tonglingyu-gateway",
            "agent-orchestrator",
            "agent-worker",
        ],
        "checked_secret_fields": [
            "HERMES_API_KEY/API_SERVER_KEY",
            "TONGLINGYU_GATEWAY_API_KEY(S)",
            "TONGLINGYU_ADMIN_API_KEY(S)",
            "OPENAI_API_KEYS",
            "tonglingyu-gateway.AGENT_RUNTIME_HERMES_API_KEY",
            "AGENT_RUNTIME_HERMES_API_KEY",
        ],
        "checked_policy_fields": [
            "DEFAULT_MODELS",
            "OPENAI_API_BASE_URLS",
            "TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY",
            "TONGLINGYU_AGENT_RUNTIME_MODE",
            "AGENT_RUNTIME_MODE",
        ],
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
