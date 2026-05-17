#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
FRONT_RUNTIME_DIR="${HUIXIANGDOU_HOME_RUNTIME_DIR:-${HOME}/huixiangdou-home-runtime}"
TONGLINGYU_RUNTIME_DIR="${TONGLINGYU_RUNTIME_DIR:-${HOME}/tonglingyu-home-runtime}"
OLD_DATA_DIR="${DEPLOY_DIR}/data"
FRONT_DATA_DIR="${FRONT_RUNTIME_DIR}/data"
TONGLINGYU_DATA_ROOT="${TONGLINGYU_RUNTIME_DIR}/data"

mkdir -p \
  "${FRONT_DATA_DIR}/open-webui" \
  "${TONGLINGYU_DATA_ROOT}/hermes" \
  "${TONGLINGYU_DATA_ROOT}/tonglingyu" \
  "${FRONT_RUNTIME_DIR}/backups" \
  "${TONGLINGYU_RUNTIME_DIR}/backups"

if [[ ! -d "${OLD_DATA_DIR}" ]]; then
  echo "No legacy data directory at ${OLD_DATA_DIR}"
  exit 0
fi

cd "${DEPLOY_DIR}"
docker compose stop
docker run --rm \
  -v "${OLD_DATA_DIR}:/from:ro" \
  -v "${FRONT_DATA_DIR}:/front" \
  -v "${TONGLINGYU_DATA_ROOT}:/tonglingyu" \
  busybox:1.36 \
  sh -c '
    set -eu
    copy_dir() {
      name="$1"
      target="$2"
      if [ -e "/from/${name}" ]; then
        mkdir -p "${target}"
        cp -a "/from/${name}" "${target}/"
      fi
    }
    copy_dir open-webui /front
    copy_dir hermes /tonglingyu
    copy_dir tonglingyu /tonglingyu
  '

backup_dir="${TONGLINGYU_RUNTIME_DIR}/backups/legacy-deploy-data.$(date +%Y%m%d-%H%M%S)"
mv "${OLD_DATA_DIR}" "${backup_dir}"
echo "Moved ${OLD_DATA_DIR} to ${backup_dir}"
echo "Open WebUI runtime data is now under ${FRONT_DATA_DIR}/open-webui"
echo "Tonglingyu runtime data is now under ${TONGLINGYU_DATA_ROOT}"
