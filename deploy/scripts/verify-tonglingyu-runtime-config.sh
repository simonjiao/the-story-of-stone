#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
CONFIG_JSON="$(mktemp)"
trap 'rm -f "${CONFIG_JSON}"' EXIT

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
if [[ -n "$(deploy_env_file_path)" ]]; then
  load_optional_deploy_env_file
elif [[ -f "${DEPLOY_DIR}/.env" ]]; then
  source_deploy_env_file "${DEPLOY_DIR}/.env"
fi

cd "${DEPLOY_DIR}"

if command -v docker >/dev/null 2>&1; then
  if ! docker compose config --format json >"${CONFIG_JSON}"; then
    echo "compose_config=failed" >&2
    echo "hint=set the required deploy/.env variables or export dummy values for dry-run verification" >&2
    exit 1
  fi
elif [[ "${TONGLINGYU_RUNTIME_CONFIG_REQUIRE_DOCKER:-false}" == "true" ]]; then
  echo "compose_config=failed" >&2
  echo "hint=docker is required for live runtime config verification" >&2
  exit 1
else
  python3 - "${DEPLOY_DIR}/docker-compose.yml" "${CONFIG_JSON}" <<'PY'
import json
import os
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    print("pyyaml_missing_for_static_compose_parse", file=sys.stderr)
    raise SystemExit(1)

compose_path = Path(sys.argv[1])
target_path = Path(sys.argv[2])


def find_expr_end(value, start):
    index = start + 2
    depth = 1
    while index < len(value):
        if value.startswith("${", index):
            depth += 1
            index += 2
            continue
        if value[index] == "}":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return -1


def resolve_expr(expr):
    if ":-" in expr:
        name, default = expr.split(":-", 1)
        raw = os.environ.get(name)
        return interpolate(default) if raw is None or raw == "" else raw
    if ":?" in expr:
        name, _message = expr.split(":?", 1)
        raw = os.environ.get(name)
        return "" if raw is None or raw == "" else raw
    raw = os.environ.get(expr)
    return "" if raw is None else raw


def interpolate(value):
    text = str(value)
    result = []
    index = 0
    while index < len(text):
        start = text.find("${", index)
        if start < 0:
            result.append(text[index:])
            break
        result.append(text[index:start])
        end = find_expr_end(text, start)
        if end < 0:
            result.append(text[start:])
            break
        result.append(resolve_expr(text[start + 2:end]))
        index = end + 1
    return "".join(result)


raw = yaml.safe_load(compose_path.read_text(encoding="utf-8")) or {}
services = {}
for name, service in (raw.get("services") or {}).items():
    environment = service.get("environment") or {}
    if isinstance(environment, dict):
        resolved = {
            str(key): interpolate("" if value is None else value)
            for key, value in environment.items()
        }
    elif isinstance(environment, list):
        resolved = {}
        for item in environment:
            text = interpolate(item)
            if "=" in text:
                key, value = text.split("=", 1)
                resolved[key] = value
    else:
        resolved = {}
    services[str(name)] = {
        "container_name": interpolate(service.get("container_name", "")),
        "environment": resolved,
    }

target_path.write_text(
    json.dumps(
        {
            "_config_mode": "static_compose_parse",
            "services": services,
        },
        ensure_ascii=True,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY
fi

python3 - "${CONFIG_JSON}" <<'PY'
import json
import sys

config_path = sys.argv[1]
with open(config_path, "r", encoding="utf-8") as handle:
    config = json.load(handle)

errors = []
services = config.get("services") or {}
config_blob = json.dumps(config, ensure_ascii=True).lower()
for forbidden_spelling in ["tonglignyu"]:
    if forbidden_spelling in config_blob:
        errors.append(f"spelling_forbidden={forbidden_spelling}")


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
    for item in provider_key_list(value_text):
        keys.add(item)
    return keys


def provider_key_list(value_text):
    keys = []
    for item in str(value_text or "").replace(",", ";").split(";"):
        candidate = item.strip()
        if candidate:
            keys.append(candidate)
    return keys


def require_false(env, key, service_name):
    actual = value(env, key).lower()
    if actual not in {"false", "0", "no", "off"}:
        errors.append(f"{service_name}.{key} must be false/0/no/off for production verification")


def require_positive_int(env, key, service_name):
    actual = value(env, key)
    try:
        parsed = int(actual)
    except ValueError:
        errors.append(f"{service_name}.{key} must be a positive integer")
        return
    if parsed <= 0:
        errors.append(f"{service_name}.{key} must be a positive integer")


gateway_env = env_map("tonglingyu-gateway")
open_webui_env = env_map("open-webui")
hermes_env = env_map("hermes")

for forbidden_service in [
    "agent-platform-postgres",
    "agent-action-gateway",
    "agent-manager",
    "agent-orchestrator",
    "agent-worker",
    "agent-observer",
    "global-router",
]:
    if forbidden_service in services:
        errors.append(f"service_forbidden={forbidden_service}")

for service_name in ["hermes", "open-webui", "tonglingyu-gateway", "cloudflared"]:
    service = services.get(service_name) or {}
    container_name = str(service.get("container_name") or "").strip()
    if container_name and not container_name.startswith("tonglingyu-"):
        errors.append(f"{service_name}.container_name must start with tonglingyu-")

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

open_webui_provider_key_list = provider_key_list(value(open_webui_env, "OPENAI_API_KEYS"))
open_webui_provider_key_set = set(open_webui_provider_key_list)
if gateway_api_key and open_webui_provider_key_list != [gateway_api_key]:
    errors.append("open-webui.OPENAI_API_KEYS must contain only TONGLINGYU_GATEWAY_API_KEY")
if admin_key_set.intersection(open_webui_provider_key_set):
    errors.append("open-webui.OPENAI_API_KEYS must not contain TONGLINGYU_ADMIN_API_KEY(S)")

open_webui_admin_api_key = value(open_webui_env, "TONGLINGYU_ADMIN_API_KEY")
if open_webui_admin_api_key and open_webui_admin_api_key != admin_api_key:
    errors.append("open-webui.TONGLINGYU_ADMIN_API_KEY must match tonglingyu-gateway.TONGLINGYU_ADMIN_API_KEY")

if value(open_webui_env, "DEFAULT_MODELS") != "tonglingyu":
    errors.append("open-webui.DEFAULT_MODELS must be tonglingyu")
base_urls = [item.strip() for item in value(open_webui_env, "OPENAI_API_BASE_URLS").split(";") if item.strip()]
if base_urls != ["http://tonglingyu-gateway:8090/v1"]:
    errors.append("open-webui.OPENAI_API_BASE_URLS must be exactly http://tonglingyu-gateway:8090/v1")

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
require_positive_int(
    gateway_env,
    "TONGLINGYU_AGENT_RUNTIME_PROFILE_MAX_SECONDS",
    "tonglingyu-gateway",
)

if errors:
    for error in errors:
        print(f"config_error={error}", file=sys.stderr)
    sys.exit(1)

print(json.dumps(
    {
        "status": "ok",
        "config_mode": config.get("_config_mode", "docker_compose_config"),
        "checked_services": [
            "hermes",
            "open-webui",
            "tonglingyu-gateway",
            "cloudflared",
        ],
        "forbidden_services_absent": [
            "agent-platform-postgres",
            "agent-action-gateway",
            "agent-manager",
            "agent-orchestrator",
            "agent-worker",
            "agent-observer",
            "global-router",
        ],
        "checked_secret_fields": [
            "HERMES_API_KEY/API_SERVER_KEY",
            "TONGLINGYU_GATEWAY_API_KEY(S)",
            "TONGLINGYU_ADMIN_API_KEY(S)",
            "OPENAI_API_KEYS",
            "tonglingyu-gateway.AGENT_RUNTIME_HERMES_API_KEY",
        ],
        "checked_policy_fields": [
            "DEFAULT_MODELS",
            "OPENAI_API_BASE_URLS",
            "forbidden_spellings",
            "TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY",
            "TONGLINGYU_AGENT_RUNTIME_MODE",
            "TONGLINGYU_AGENT_RUNTIME_PROFILE_MAX_SECONDS",
            "container_name_prefix",
        ],
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
