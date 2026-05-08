#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
ENV_FILE="${DEPLOY_DIR}/.env"
BACKUP_ROOT="${ENV_BACKUP_ROOT:-${HOME}/OneDrive/backup}"
BACKUP_DIR="${ENV_BACKUP_DIR:-${BACKUP_ROOT}/the-story-of-stone/deploy-env}"
BACKUP_PREFIX="deploy.env.bak"

usage() {
  cat <<EOF
Usage:
  $0 backup
  $0 list
  $0 restore latest
  $0 restore <backup-file>

Environment:
  ENV_BACKUP_ROOT  Backup root directory. Default: ${HOME}/OneDrive/backup
  ENV_BACKUP_DIR   Exact backup directory. Default: \$ENV_BACKUP_ROOT/the-story-of-stone/deploy-env
EOF
}

timestamp() {
  date +%Y%m%d-%H%M%S
}

ensure_env_file() {
  if [[ ! -f "${ENV_FILE}" ]]; then
    echo "Missing deploy .env at ${ENV_FILE}" >&2
    exit 1
  fi
}

ensure_backup_dir() {
  mkdir -p "${BACKUP_DIR}"
}

unique_backup_path() {
  local prefix="$1"
  local ts
  local candidate
  local n

  ts="$(timestamp)"
  candidate="${BACKUP_DIR}/${prefix}.${ts}"
  n=1
  while [[ -e "${candidate}" ]]; do
    candidate="${BACKUP_DIR}/${prefix}.${ts}.${n}"
    n=$((n + 1))
  done
  printf '%s' "${candidate}"
}

copy_env_to_backup() {
  local prefix="$1"
  local backup_file

  ensure_env_file
  ensure_backup_dir
  backup_file="$(unique_backup_path "${prefix}")"
  cp -p "${ENV_FILE}" "${backup_file}"
  chmod 600 "${backup_file}"
  printf '%s\n' "${backup_file}"
}

latest_backup() {
  local backups
  shopt -s nullglob
  backups=("${BACKUP_DIR}/${BACKUP_PREFIX}".*)
  shopt -u nullglob

  if (( ${#backups[@]} == 0 )); then
    return 1
  fi

  printf '%s\n' "${backups[@]}" | sort | tail -n 1
}

resolve_backup_file() {
  local arg="$1"
  local backup_file

  if [[ "${arg}" == "latest" ]]; then
    if ! backup_file="$(latest_backup)"; then
      echo "No ${BACKUP_PREFIX} backups found under ${BACKUP_DIR}" >&2
      exit 1
    fi
  elif [[ "${arg}" == /* ]]; then
    backup_file="${arg}"
  else
    backup_file="${BACKUP_DIR}/${arg}"
  fi

  if [[ ! -f "${backup_file}" ]]; then
    echo "Backup file not found: ${backup_file}" >&2
    exit 1
  fi

  printf '%s' "${backup_file}"
}

backup_env() {
  local backup_file
  backup_file="$(copy_env_to_backup "${BACKUP_PREFIX}")"
  echo "Backed up deploy/.env to ${backup_file}"
}

list_backups() {
  ensure_backup_dir

  shopt -s nullglob
  local files=("${BACKUP_DIR}"/deploy.env.*)
  shopt -u nullglob

  if (( ${#files[@]} == 0 )); then
    echo "No deploy.env backups found under ${BACKUP_DIR}"
    return
  fi

  printf '%s\n' "${files[@]}" | sort -r
}

restore_env() {
  local requested="${1:-}"
  local backup_file
  local current_backup

  if [[ -z "${requested}" ]]; then
    echo "restore requires 'latest' or a backup file path" >&2
    usage >&2
    exit 1
  fi

  backup_file="$(resolve_backup_file "${requested}")"
  current_backup="$(copy_env_to_backup "deploy.env.pre-restore")"
  cp -p "${backup_file}" "${ENV_FILE}"
  chmod 600 "${ENV_FILE}"

  echo "Saved current deploy/.env before restore to ${current_backup}"
  echo "Restored deploy/.env from ${backup_file}"
}

main() {
  local action="${1:-}"
  shift || true

  case "${action}" in
    backup)
      backup_env
      ;;
    list)
      list_backups
      ;;
    restore)
      restore_env "$@"
      ;;
    -h|--help|help|"")
      usage
      ;;
    *)
      echo "Unknown action: ${action}" >&2
      usage >&2
      exit 1
      ;;
  esac
}

main "$@"
