#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"

ENV_FILE="${WORK_DIR}/target.env"
LOCAL_DIR="${WORK_DIR}/local"
mkdir -p "${LOCAL_DIR}"

cat >"${ENV_FILE}" <<'EOF'
DEPLOY_ENV_CONTRACT_VALUE=loaded-from-explicit-env
DEPLOY_ENV_CONTRACT_SECRET=do-not-print-me
EOF

TONGLINGYU_DEPLOY_ENV_FILE="${ENV_FILE}" load_optional_deploy_env_file
test "${DEPLOY_ENV_CONTRACT_VALUE}" = "loaded-from-explicit-env"

unset DEPLOY_ENV_CONTRACT_VALUE DEPLOY_ENV_CONTRACT_SECRET
cat >"${LOCAL_DIR}/.env" <<'EOF'
DEPLOY_ENV_CONTRACT_VALUE=loaded-from-local-env
DEPLOY_ENV_CONTRACT_SECRET=do-not-print-me-either
EOF
(
  cd "${LOCAL_DIR}"
  load_deploy_env_file_or_local
  test "${DEPLOY_ENV_CONTRACT_VALUE}" = "loaded-from-local-env"
)

missing_err="${WORK_DIR}/missing.err"
if TONGLINGYU_DEPLOY_ENV_FILE="${WORK_DIR}/missing.env" \
  load_optional_deploy_env_file >"${WORK_DIR}/missing.out" 2>"${missing_err}"; then
  echo "missing deploy env file unexpectedly loaded" >&2
  exit 1
fi
grep -q "deploy env file not found" "${missing_err}"
if grep -q "do-not-print" "${missing_err}"; then
  echo "deploy env helper leaked env value" >&2
  exit 1
fi

echo "deploy env file contract passed"
