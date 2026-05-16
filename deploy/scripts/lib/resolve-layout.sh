#!/usr/bin/env bash

resolve_tonglingyu_layout() {
  local script_dir="$1"
  local script_parent
  script_parent="$(cd -- "${script_dir}/.." && pwd)"

  DEPLOY_DIR="${script_parent}"
  if [[ -d "${script_parent}/agent-platform" || -d "${script_parent}/resources" ]]; then
    REPO_DIR="${script_parent}"
  else
    REPO_DIR="$(cd -- "${script_dir}/../.." && pwd)"
  fi
}
