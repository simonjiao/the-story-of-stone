#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
MODE="${1:---check}"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"

case "${MODE}" in
  --check | --apply) ;;
  *)
    echo "usage: $0 [--check|--apply]" >&2
    exit 2
    ;;
esac

ENV_FILE="$(deploy_env_file_path)"
if [[ -z "${ENV_FILE}" ]]; then
  ENV_FILE="${DEPLOY_DIR}/.env"
fi
if [[ ! -f "${ENV_FILE}" ]]; then
  echo "deploy env file not found: ${ENV_FILE}" >&2
  exit 1
fi

python3 - "${MODE}" "${ENV_FILE}" <<'PY'
import json
import re
import secrets
import shutil
import stat
import sys
from datetime import datetime, timezone
from pathlib import Path

mode, env_path_raw = sys.argv[1:3]
env_path = Path(env_path_raw)
lines = env_path.read_text(encoding="utf-8").splitlines()
line_re = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)=(.*)$")

positions = {}
values = {}
for index, line in enumerate(lines):
    match = line_re.match(line)
    if not match:
        continue
    key, raw_value = match.groups()
    positions[key] = index
    values[key] = raw_value.strip()


def clean(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        return value[1:-1]
    return value


def token(prefix: str) -> str:
    return f"{prefix}_{secrets.token_urlsafe(32)}"


def split_provider_keys(value: str) -> list[str]:
    return [item.strip() for item in clean(value).replace(",", ";").split(";")]


def split_base_urls(value: str) -> list[str]:
    return [item.strip() for item in clean(value).split(";") if item.strip()]


def set_value(key: str, value: str) -> None:
    if key in positions:
        lines[positions[key]] = f"{key}={value}"
    else:
        positions[key] = len(lines)
        lines.append(f"{key}={value}")
    values[key] = value


planned_changes = []
gateway_key = clean(values.get("TONGLINGYU_GATEWAY_API_KEY", ""))
admin_key = clean(values.get("TONGLINGYU_ADMIN_API_KEY", ""))
if not gateway_key:
    gateway_key = token("tlyg")
    planned_changes.append("TONGLINGYU_GATEWAY_API_KEY")
if not admin_key:
    admin_key = token("tlya")
    planned_changes.append("TONGLINGYU_ADMIN_API_KEY")
if gateway_key == admin_key:
    raise SystemExit("TONGLINGYU_GATEWAY_API_KEY and TONGLINGYU_ADMIN_API_KEY must not match")

allow_admin = clean(values.get("TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY", ""))
if allow_admin.lower() not in {"false", "0", "no", "off"}:
    planned_changes.append("TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY")

base_urls = split_base_urls(values.get("OPEN_WEBUI_OPENAI_API_BASE_URLS", ""))
provider_keys = split_provider_keys(values.get("OPEN_WEBUI_OPENAI_API_KEYS", ""))
entry_count = max(len(base_urls), len(provider_keys), 1)
provider_keys.extend([""] * (entry_count - len(provider_keys)))
if admin_key in {item for item in provider_keys if item}:
    raise SystemExit("OPEN_WEBUI_OPENAI_API_KEYS must not contain TONGLINGYU_ADMIN_API_KEY")
if not provider_keys or provider_keys[0] != gateway_key:
    planned_changes.append("OPEN_WEBUI_OPENAI_API_KEYS")
provider_keys[0] = gateway_key
provider_value = ";".join(provider_keys[:entry_count])

status = "needs_update" if planned_changes else "ok"
backup_path = ""
if mode == "--apply" and planned_changes:
    backup_path = f"{env_path}.pre-tonglingyu-gateway-env.{datetime.now(timezone.utc).strftime('%Y%m%d-%H%M%S')}"
    shutil.copy2(env_path, backup_path)
    Path(backup_path).chmod(stat.S_IRUSR | stat.S_IWUSR)
    set_value("TONGLINGYU_GATEWAY_API_KEY", gateway_key)
    set_value("TONGLINGYU_ADMIN_API_KEY", admin_key)
    set_value("TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY", "false")
    set_value("OPEN_WEBUI_OPENAI_API_KEYS", provider_value)
    env_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    env_path.chmod(stat.S_IRUSR | stat.S_IWUSR)
    status = "updated"

print(json.dumps(
    {
        "status": status,
        "mode": mode.removeprefix("--"),
        "env_file": str(env_path),
        "changed_keys": sorted(set(planned_changes)),
        "backup_created": bool(backup_path),
        "provider_key_entries": entry_count,
        "secret_values_printed": False,
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
