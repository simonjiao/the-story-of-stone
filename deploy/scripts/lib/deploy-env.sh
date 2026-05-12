#!/usr/bin/env bash

deploy_env_file_path() {
  printf '%s' "${TONGLINGYU_DEPLOY_ENV_FILE:-${DEPLOY_ENV_FILE:-}}"
}

source_deploy_env_file() {
  local env_file="$1"
  if [[ -z "${env_file}" ]]; then
    echo "deploy env file path is empty" >&2
    return 1
  fi
  if [[ ! -f "${env_file}" ]]; then
    echo "deploy env file not found: ${env_file}" >&2
    return 1
  fi

  local restore_nounset="false"
  case "$-" in
    *u*)
      restore_nounset="true"
      set +u
      ;;
  esac
  set -a
  # shellcheck source=/dev/null
  . "${env_file}"
  set +a
  if [[ "${restore_nounset}" == "true" ]]; then
    set -u
  fi
}

load_optional_deploy_env_file() {
  local env_file
  env_file="$(deploy_env_file_path)"
  if [[ -z "${env_file}" ]]; then
    return 0
  fi
  source_deploy_env_file "${env_file}"
}

load_deploy_env_file_or_local() {
  local env_file
  env_file="$(deploy_env_file_path)"
  if [[ -z "${env_file}" && -f ".env" ]]; then
    env_file=".env"
  fi
  if [[ -z "${env_file}" ]]; then
    echo "run this script from the deploy directory that contains .env or set TONGLINGYU_DEPLOY_ENV_FILE" >&2
    return 1
  fi
  source_deploy_env_file "${env_file}"
}
