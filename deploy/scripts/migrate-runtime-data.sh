#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RUNTIME_DIR="${TONGLINGYU_RUNTIME_DIR:-${HOME}/tonglingyu-home-runtime}"
OLD_DATA_DIR="${DEPLOY_DIR}/data"
NEW_DATA_DIR="${RUNTIME_DIR}/data"

mkdir -p \
	  "${NEW_DATA_DIR}/hermes" \
	  "${NEW_DATA_DIR}/open-webui" \
	  "${NEW_DATA_DIR}/tonglingyu" \
	  "${RUNTIME_DIR}/backups"

if [[ ! -d "${OLD_DATA_DIR}" ]]; then
  echo "No legacy data directory at ${OLD_DATA_DIR}"
  exit 0
fi

cd "${DEPLOY_DIR}"
docker compose stop
docker run --rm \
  -v "${OLD_DATA_DIR}:/from:ro" \
  -v "${NEW_DATA_DIR}:/to" \
  busybox:1.36 \
  sh -c 'cp -a /from/. /to/'

backup_dir="${RUNTIME_DIR}/backups/legacy-deploy-data.$(date +%Y%m%d-%H%M%S)"
mv "${OLD_DATA_DIR}" "${backup_dir}"
echo "Moved ${OLD_DATA_DIR} to ${backup_dir}"
echo "Runtime data is now under ${NEW_DATA_DIR}"
