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

HERMES_DATA_DIR="$(read_env_value HERMES_DATA_DIR)"
LOCAL_OPENAI_BASE_URL="$(read_env_value LOCAL_OPENAI_BASE_URL)"
LOCAL_OPENAI_MODEL="$(read_env_value LOCAL_OPENAI_MODEL)"
LOCAL_OPENAI_API_KEY="$(read_env_value LOCAL_OPENAI_API_KEY)"

: "${HERMES_DATA_DIR:?set HERMES_DATA_DIR in .env}"
: "${LOCAL_OPENAI_BASE_URL:?set LOCAL_OPENAI_BASE_URL in .env}"
: "${LOCAL_OPENAI_MODEL:?set LOCAL_OPENAI_MODEL in .env}"
: "${LOCAL_OPENAI_API_KEY:?set LOCAL_OPENAI_API_KEY in .env}"

mkdir -p "${HERMES_DATA_DIR}"

cat > "${HERMES_DATA_DIR}/config.yaml" <<EOF
model:
  provider: custom
  model: ${LOCAL_OPENAI_MODEL}
  base_url: ${LOCAL_OPENAI_BASE_URL}
  api_key: ${LOCAL_OPENAI_API_KEY}
EOF

echo "Wrote ${HERMES_DATA_DIR}/config.yaml"
