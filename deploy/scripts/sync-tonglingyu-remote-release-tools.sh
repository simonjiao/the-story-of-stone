#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

REMOTE_HOST="${TONGLINGYU_REMOTE_RELEASE_HOST:-hhost}"
REMOTE_PROJECT_DIR="${TONGLINGYU_REMOTE_RELEASE_PROJECT_DIR:-}"
REMOTE_PROJECT_ARG="${REMOTE_PROJECT_DIR:-__DEFAULT_REMOTE_PROJECT_DIR__}"
RUN_ID="${TONGLINGYU_REMOTE_RELEASE_TOOLS_RUN_ID:-remote-tools-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
ARTIFACT_ROOT="${TONGLINGYU_REMOTE_RELEASE_TOOLS_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/remote-release-tools}"
if [[ "${ARTIFACT_ROOT}" != /* ]]; then
  ARTIFACT_ROOT="${REPO_DIR}/${ARTIFACT_ROOT}"
fi
ARTIFACT_DIR="${TONGLINGYU_REMOTE_RELEASE_TOOLS_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${REPO_DIR}/${ARTIFACT_DIR}"
fi
REPORT_PATH="${TONGLINGYU_REMOTE_RELEASE_TOOLS_REPORT_PATH:-${ARTIFACT_DIR}/remote-release-tools-sync.json}"
INCLUDE_COMPOSE="${TONGLINGYU_REMOTE_RELEASE_TOOLS_INCLUDE_COMPOSE:-false}"

mkdir -p "${ARTIFACT_DIR}"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

REMOTE_PROJECT_RESOLVED="$(
  ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
    'sh -s' -- "${REMOTE_PROJECT_ARG}" <<'REMOTE'
set -eu
project_dir_arg="$1"
if [ "${project_dir_arg}" != "__DEFAULT_REMOTE_PROJECT_DIR__" ]; then
  project_dir="${project_dir_arg}"
else
  project_dir="${DEPLOY_NODE_PROJECT_DIR:-$HOME/hermes-home-deploy}"
fi
mkdir -p "${project_dir}/scripts" "${project_dir}/runbooks" \
  "${project_dir}/agent-platform" "${project_dir}/resources"
printf '%s\n' "${project_dir}"
REMOTE
)"

rsync_log="${ARTIFACT_DIR}/rsync.log"
: >"${rsync_log}"

rsync_path() {
  local source_path="$1"
  local remote_subdir="$2"
  shift 2
  if [[ ! -e "${source_path}" ]]; then
    return 0
  fi
  rsync -a "$@" "${source_path}" "${REMOTE_HOST}:${REMOTE_PROJECT_RESOLVED}/${remote_subdir}/" \
    >>"${rsync_log}" 2>&1
}

rsync_path "${REPO_DIR}/deploy/scripts/" "scripts"
rsync_path "${REPO_DIR}/deploy/runbooks/" "runbooks"
rsync_path "${REPO_DIR}/deploy/open-webui/" "open-webui"
rsync_path "${REPO_DIR}/agent-platform/" "agent-platform" \
  --exclude target --exclude .git --exclude .direnv
rsync_path "${REPO_DIR}/resources/" "resources"
if is_true "${INCLUDE_COMPOSE}"; then
  rsync -a "${REPO_DIR}/deploy/docker-compose.yml" \
    "${REMOTE_HOST}:${REMOTE_PROJECT_RESOLVED}/docker-compose.yml" \
    >>"${rsync_log}" 2>&1
fi

REMOTE_PREP_STDOUT="${ARTIFACT_DIR}/remote-prepare.stdout"
REMOTE_PREP_STDERR="${ARTIFACT_DIR}/remote-prepare.stderr"
set +e
ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  'sh -s' -- "${REMOTE_PROJECT_RESOLVED}" <<'REMOTE' \
  >"${REMOTE_PREP_STDOUT}" 2>"${REMOTE_PREP_STDERR}"
set -eu
project_dir="$1"
cd "${project_dir}"
mkdir -p agent-platform/target/debug data/tonglingyu/remote-release-tools
tmp_bin="$(mktemp)"
docker compose cp tonglingyu-gateway:/usr/local/bin/tonglingyu-gateway "${tmp_bin}" >/dev/null
mv "${tmp_bin}" agent-platform/target/debug/tonglingyu-gateway
chmod 755 agent-platform/target/debug/tonglingyu-gateway
cat > .tonglingyu-release-tool-env <<EOF
export TONGLINGYU_DEPLOY_ENV_FILE='${project_dir}/.env'
export TONGLINGYU_RQA_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_QUALITY_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_MIGRATION_PREFLIGHT_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_MIGRATION_PREFLIGHT_SKIP_BUILD=true
export TONGLINGYU_RQA_PERFORMANCE_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_PERFORMANCE_SKIP_BUILD=true
export TONGLINGYU_RQA_API_CONTRACT_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_API_CONTRACT_SKIP_BUILD=true
export TONGLINGYU_RQA_USER_LIFECYCLE_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_USER_LIFECYCLE_SKIP_BUILD=true
export TONGLINGYU_RQA_RESTORE_DRILL_GATEWAY_BIN='${project_dir}/agent-platform/target/debug/tonglingyu-gateway'
export TONGLINGYU_RQA_RESTORE_DRILL_SKIP_BUILD=true
export TONGLINGYU_RQA_MIGRATION_PREFLIGHT_SOURCE_ROOT='${project_dir}/resources/sources/wiki'
export TONGLINGYU_RQA_PERFORMANCE_SOURCE_ROOT='${project_dir}/resources/sources/wiki'
export TONGLINGYU_RQA_API_CONTRACT_SOURCE_ROOT='${project_dir}/resources/sources/wiki'
export TONGLINGYU_RQA_USER_LIFECYCLE_SOURCE_ROOT='${project_dir}/resources/sources/wiki'
export TONGLINGYU_RQA_RESTORE_DRILL_SOURCE_ROOT='${project_dir}/resources/sources/wiki'
EOF
python3 - <<'PY'
import hashlib
import json
from pathlib import Path

project_dir = Path.cwd()
bin_path = project_dir / "agent-platform" / "target" / "debug" / "tonglingyu-gateway"
env_path = project_dir / ".tonglingyu-release-tool-env"
digest = hashlib.sha256(bin_path.read_bytes()).hexdigest()
print(json.dumps(
    {
        "object": "tonglingyu.remote_release_tools_prepare",
        "status": "ok",
        "gateway_bin": str(bin_path),
        "gateway_bin_sha256": digest,
        "tool_env_path": str(env_path),
        "secret_values_printed": False,
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
REMOTE
prepare_exit=$?
set -e

REMOTE_VERIFY_STDOUT="${ARTIFACT_DIR}/remote-verify.stdout"
REMOTE_VERIFY_STDERR="${ARTIFACT_DIR}/remote-verify.stderr"
set +e
ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  'sh -s' -- "${REMOTE_PROJECT_RESOLVED}" <<'REMOTE' \
  >"${REMOTE_VERIFY_STDOUT}" 2>"${REMOTE_VERIFY_STDERR}"
set -eu
project_dir="$1"
cd "${project_dir}"
required_scripts='
verify-tonglingyu-rqa-release-automation.sh
verify-tonglingyu-rqa-capacity-load-smoke.sh
verify-tonglingyu-rqa-incident-capacity.sh
verify-tonglingyu-rqa-backup-restore-drill.sh
remediate-tonglingyu-kb-source-metadata.sh
verify-tonglingyu-release-ops-readiness.sh
verify-tonglingyu-post-release-monitor.sh
verify-tonglingyu-release-security.sh
verify-tonglingyu-release-readiness.sh
verify-tonglingyu-release-readiness-report.sh
'
missing=""
for script in ${required_scripts}; do
  if [ ! -f "scripts/${script}" ]; then
    missing="${missing} ${script}"
  fi
done
bash -n scripts/verify-tonglingyu-rqa-release-automation.sh
bash -n scripts/verify-tonglingyu-release-readiness.sh
bash -n scripts/verify-tonglingyu-rqa-performance-budget.sh
bash -n scripts/verify-tonglingyu-rqa-backup-restore-drill.sh
bash -n scripts/verify-tonglingyu-rqa-quality-gate.sh
. ./.tonglingyu-release-tool-env
test -x "${TONGLINGYU_RQA_GATEWAY_BIN}"
python3 - "${missing}" <<'PY'
import json
import sys

missing = [item for item in sys.argv[1].split() if item]
print(json.dumps(
    {
        "object": "tonglingyu.remote_release_tools_verify",
        "status": "ok" if not missing else "failed",
        "missing_scripts": missing,
        "gateway_bin_executable": True,
        "tool_env_present": True,
        "secret_values_printed": False,
    },
    ensure_ascii=True,
    sort_keys=True,
))
raise SystemExit(0 if not missing else 1)
PY
REMOTE
verify_exit=$?
set -e

python3 - "${REPORT_PATH}" "${REMOTE_HOST}" "${REMOTE_PROJECT_RESOLVED}" \
  "${ARTIFACT_DIR}" "${rsync_log}" "${REMOTE_PREP_STDOUT}" "${REMOTE_PREP_STDERR}" \
  "${prepare_exit}" "${REMOTE_VERIFY_STDOUT}" "${REMOTE_VERIFY_STDERR}" "${verify_exit}" \
  "${INCLUDE_COMPOSE}" "${REPO_DIR}" <<'PY'
import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    remote_host,
    remote_project_dir,
    artifact_dir_raw,
    rsync_log_raw,
    prepare_stdout_raw,
    prepare_stderr_raw,
    prepare_exit_raw,
    verify_stdout_raw,
    verify_stderr_raw,
    verify_exit_raw,
    include_compose_raw,
    repo_dir_raw,
) = sys.argv[1:14]


def file_sha256(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tail(path_raw, limit=20):
    path = Path(path_raw)
    if not path.is_file():
        return []
    return path.read_text(encoding="utf-8", errors="replace").splitlines()[-limit:]


def last_json(path_raw):
    for line in reversed(tail(path_raw, 200)):
        try:
            candidate = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(candidate, dict):
            return candidate
    return None


def git_output(args):
    try:
        return subprocess.run(
            ["git", "-C", repo_dir_raw, *args],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return ""


prepare_exit = int(prepare_exit_raw)
verify_exit = int(verify_exit_raw)
prepare_json = last_json(prepare_stdout_raw)
verify_json = last_json(verify_stdout_raw)
errors = []
if prepare_exit != 0:
    errors.append(f"remote_prepare_exit={prepare_exit}")
if verify_exit != 0:
    errors.append(f"remote_verify_exit={verify_exit}")
if not isinstance(prepare_json, dict) or prepare_json.get("status") != "ok":
    errors.append("remote_prepare_json_not_ok")
if not isinstance(verify_json, dict) or verify_json.get("status") != "ok":
    errors.append("remote_verify_json_not_ok")
missing_scripts = (
    verify_json.get("missing_scripts")
    if isinstance(verify_json, dict) and isinstance(verify_json.get("missing_scripts"), list)
    else []
)
if missing_scripts:
    errors.append("remote_required_scripts_missing")

payload = {
    "object": "tonglingyu.remote_release_tools_sync",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "remote_host": remote_host,
    "remote_project_dir": remote_project_dir,
    "include_compose": include_compose_raw.lower() in {"1", "true", "yes", "on"},
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "git": {
        "commit": git_output(["rev-parse", "HEAD"]),
        "tracked_dirty": bool(git_output(["status", "--porcelain", "--untracked-files=no"])),
    },
    "artifacts": {
        "artifact_dir": artifact_dir_raw,
        "rsync_log": rsync_log_raw,
        "rsync_log_sha256": file_sha256(rsync_log_raw),
        "remote_prepare_stdout": prepare_stdout_raw,
        "remote_prepare_stdout_sha256": file_sha256(prepare_stdout_raw),
        "remote_prepare_stderr": prepare_stderr_raw,
        "remote_prepare_stderr_sha256": file_sha256(prepare_stderr_raw),
        "remote_verify_stdout": verify_stdout_raw,
        "remote_verify_stdout_sha256": file_sha256(verify_stdout_raw),
        "remote_verify_stderr": verify_stderr_raw,
        "remote_verify_stderr_sha256": file_sha256(verify_stderr_raw),
    },
    "remote_prepare": prepare_json or {},
    "remote_verify": verify_json or {},
    "checks": {
        "scripts_synced": not missing_scripts,
        "gateway_binary_prepared": (
            isinstance(prepare_json, dict)
            and bool(prepare_json.get("gateway_bin_sha256"))
        ),
        "tool_env_present": (
            isinstance(verify_json, dict)
            and verify_json.get("tool_env_present") is True
        ),
        "gateway_bin_executable": (
            isinstance(verify_json, dict)
            and verify_json.get("gateway_bin_executable") is True
        ),
        "deploy_env_not_synced": True,
    },
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
report_path = Path(report_path_raw)
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(encoded + "\n", encoding="utf-8")
print(encoded)
if errors:
    raise SystemExit(1)
PY
