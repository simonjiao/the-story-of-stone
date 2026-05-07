#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${ROOT_DIR}/.env"

if [[ ! -f "${ENV_FILE}" ]]; then
  echo "Missing .env at ${ENV_FILE}" >&2
  exit 1
fi

read_env_value() {
  local key="$1"
  local line
  line="$(grep -E "^${key}=" "${ENV_FILE}" | tail -n 1 || true)"
  if [[ -z "${line}" ]]; then
    return 1
  fi
  local value="${line#*=}"
  value="${value%$'\r'}"
  value="${value%\"}"
  value="${value#\"}"
  value="${value%\'}"
  value="${value#\'}"
  printf '%s' "${value}"
}

read_env_value_or_default() {
  local key="$1"
  local default="$2"
  read_env_value "${key}" || printf '%s' "${default}"
}

HERMES_DATA_DIR="$(read_env_value HERMES_DATA_DIR)"
LOCAL_OPENAI_BASE_URL="$(read_env_value LOCAL_OPENAI_BASE_URL)"
LOCAL_OPENAI_MODEL="$(read_env_value LOCAL_OPENAI_MODEL)"
LOCAL_OPENAI_API_KEY="$(read_env_value LOCAL_OPENAI_API_KEY)"
HERMES_MEMORY_ENABLED="$(read_env_value_or_default HERMES_MEMORY_ENABLED false)"
HERMES_USER_PROFILE_ENABLED="$(read_env_value_or_default HERMES_USER_PROFILE_ENABLED false)"

: "${HERMES_DATA_DIR:?set HERMES_DATA_DIR in .env}"
: "${LOCAL_OPENAI_BASE_URL:?set LOCAL_OPENAI_BASE_URL in .env}"
: "${LOCAL_OPENAI_MODEL:?set LOCAL_OPENAI_MODEL in .env}"
: "${LOCAL_OPENAI_API_KEY:?set LOCAL_OPENAI_API_KEY in .env}"

case "${HERMES_MEMORY_ENABLED}" in
  true|false) ;;
  *)
    echo "HERMES_MEMORY_ENABLED must be true or false" >&2
    exit 1
    ;;
esac

case "${HERMES_USER_PROFILE_ENABLED}" in
  true|false) ;;
  *)
    echo "HERMES_USER_PROFILE_ENABLED must be true or false" >&2
    exit 1
    ;;
esac

mkdir -p "${HERMES_DATA_DIR}"

CONFIG_FILE="${HERMES_DATA_DIR}/config.yaml"
if [[ -f "${CONFIG_FILE}" ]]; then
  BACKUP_FILE="${CONFIG_FILE}.bak.$(date +%Y%m%d-%H%M%S)"
  cp "${CONFIG_FILE}" "${BACKUP_FILE}"
  echo "Backed up ${CONFIG_FILE} to ${BACKUP_FILE}"
fi

cat > "${CONFIG_FILE}" <<EOF
model:
  provider: custom
  model: ${LOCAL_OPENAI_MODEL}
  base_url: ${LOCAL_OPENAI_BASE_URL}
  api_key: ${LOCAL_OPENAI_API_KEY}
memory:
  memory_enabled: ${HERMES_MEMORY_ENABLED}
  user_profile_enabled: ${HERMES_USER_PROFILE_ENABLED}
EOF

echo "Wrote ${CONFIG_FILE}"
