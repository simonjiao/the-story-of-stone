#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

REMOTE_HOST="${TONGLINGYU_REMOTE_RELEASE_HOST:-hhost}"
REMOTE_PROJECT_DIR="${TONGLINGYU_REMOTE_RELEASE_PROJECT_DIR:-}"
REMOTE_PROJECT_ARG="${REMOTE_PROJECT_DIR:-__DEFAULT_REMOTE_PROJECT_DIR__}"
RUN_ID="${TONGLINGYU_REMOTE_LIVE_GATE_RUN_ID:-remote-live-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
ARTIFACT_ROOT="${TONGLINGYU_REMOTE_LIVE_GATE_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/remote-live-gates}"
if [[ "${ARTIFACT_ROOT}" != /* ]]; then
  ARTIFACT_ROOT="${REPO_DIR}/${ARTIFACT_ROOT}"
fi
ARTIFACT_DIR="${TONGLINGYU_REMOTE_LIVE_GATE_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${REPO_DIR}/${ARTIFACT_DIR}"
fi
REPORT_PATH="${TONGLINGYU_REMOTE_LIVE_GATE_REPORT_PATH:-${ARTIFACT_DIR}/remote-live-gates.json}"
RESULTS_JSONL="${ARTIFACT_DIR}/remote-live-gates.jsonl"

mkdir -p "${ARTIFACT_DIR}"
: >"${RESULTS_JSONL}"

run_remote_gate() {
  local name="$1"
  local script_name="$2"
  local expected_object="$3"
  local stdout_path="${ARTIFACT_DIR}/${name}.stdout"
  local stderr_path="${ARTIFACT_DIR}/${name}.stderr"
  local gate_json_path="${ARTIFACT_DIR}/${name}.json"
  local exit_code=0

  set +e
  ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
    'sh -s' -- "${REMOTE_PROJECT_ARG}" "${script_name}" <<'REMOTE' \
    >"${stdout_path}" 2>"${stderr_path}"
set -eu
project_dir_arg="$1"
script_name="$2"
if [ "${project_dir_arg}" != "__DEFAULT_REMOTE_PROJECT_DIR__" ]; then
  project_dir="${project_dir_arg}"
else
  project_dir="${DEPLOY_NODE_PROJECT_DIR:-$HOME/tonglingyu-home-deploy}"
fi
cd "${project_dir}"
if [ ! -x "scripts/${script_name}" ]; then
  printf 'remote script missing: scripts/%s\n' "${script_name}" >&2
  exit 127
fi
"./scripts/${script_name}"
REMOTE
  exit_code=$?
  set -e

  python3 - "${RESULTS_JSONL}" "${name}" "${script_name}" "${expected_object}" \
    "${exit_code}" "${stdout_path}" "${stderr_path}" "${gate_json_path}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    results_path_raw,
    name,
    script_name,
    expected_object,
    exit_code_raw,
    stdout_path_raw,
    stderr_path_raw,
    gate_json_path_raw,
) = sys.argv[1:9]
exit_code = int(exit_code_raw)
stdout_path = Path(stdout_path_raw)
stderr_path = Path(stderr_path_raw)
gate_json_path = Path(gate_json_path_raw)
secret_value_needles = (
    "api-key=",
    "api_key=",
    "apikey=",
    "authorization:",
    "bearer ",
    "password" + "=",
    "secret" + "=",
    "sk-",
    "token" + "=",
)


def file_sha256(path):
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def lines(path):
    if not path.is_file():
        return []
    return path.read_text(encoding="utf-8", errors="replace").splitlines()


def has_secret_value(text):
    lowered = text.lower()
    return any(needle in lowered for needle in secret_value_needles)


stdout_lines = lines(stdout_path)
stderr_lines = lines(stderr_path)
gate_json = None
for line in reversed(stdout_lines):
    try:
        candidate = json.loads(line)
    except json.JSONDecodeError:
        continue
    if isinstance(candidate, dict):
        gate_json = candidate
        break

errors = []
if exit_code != 0:
    errors.append(f"remote_exit_code={exit_code}")
if gate_json is None:
    errors.append("gate_json_missing")
else:
    gate_json_path.write_text(
        json.dumps(gate_json, ensure_ascii=True, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    if gate_json.get("status") != "ok":
        errors.append("gate_status_not_ok")
    if expected_object and gate_json.get("object") != expected_object:
        errors.append("gate_object_mismatch")
combined_text = "\n".join(stdout_lines + stderr_lines)
if has_secret_value(combined_text):
    errors.append("secret_like_value_in_gate_output")

result = {
    "name": name,
    "script": f"scripts/{script_name}",
    "status": "passed" if not errors else "failed",
    "exit_code": exit_code,
    "expected_object": expected_object,
    "gate_object": gate_json.get("object", "") if isinstance(gate_json, dict) else "",
    "gate_status": gate_json.get("status", "") if isinstance(gate_json, dict) else "",
    "stdout_path": str(stdout_path),
    "stderr_path": str(stderr_path),
    "stdout_sha256": file_sha256(stdout_path),
    "stderr_sha256": file_sha256(stderr_path),
    "gate_json_path": str(gate_json_path) if gate_json_path.is_file() else "",
    "gate_json_sha256": file_sha256(gate_json_path),
    "errors": errors,
    "generated_at": datetime.now(timezone.utc).isoformat(),
}
with Path(results_path_raw).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(result, ensure_ascii=True, sort_keys=True) + "\n")
PY
}

run_remote_gate "model_upstream_network" \
  "verify-model-upstream-network.sh" \
  "tonglingyu.model_upstream_network_gate"
run_remote_gate "openwebui_function" \
  "verify-openwebui-function.sh" \
  ""
run_remote_gate "openwebui_admin_action" \
  "verify-openwebui-gateway-admin-action.sh" \
  ""
run_remote_gate "strict_gateway" \
  "verify-tonglingyu-strict-gateway.sh" \
  ""
run_remote_gate "scoped_context" \
  "verify-tonglingyu-scoped-context-live.sh" \
  "tonglingyu.scoped_context_live_gate"

REMOTE_CAPABILITIES_PATH="${ARTIFACT_DIR}/remote-capabilities.json"
set +e
ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  'sh -s' -- "${REMOTE_PROJECT_ARG}" <<'REMOTE' \
  >"${REMOTE_CAPABILITIES_PATH}.tmp" 2>"${ARTIFACT_DIR}/remote-capabilities.stderr"
set -eu
project_dir_arg="$1"
if [ "${project_dir_arg}" != "__DEFAULT_REMOTE_PROJECT_DIR__" ]; then
  project_dir="${project_dir_arg}"
else
  project_dir="${DEPLOY_NODE_PROJECT_DIR:-$HOME/tonglingyu-home-deploy}"
fi
cd "${project_dir}"
python3 - <<'PY'
import json
from pathlib import Path

required_scripts = [
    "verify-tonglingyu-rqa-release-automation.sh",
    "verify-tonglingyu-rqa-capacity-load-smoke.sh",
    "verify-tonglingyu-rqa-live-capacity-load-smoke.sh",
    "verify-tonglingyu-rqa-incident-capacity.sh",
    "verify-tonglingyu-rqa-backup-restore-drill.sh",
    "remediate-tonglingyu-kb-source-metadata.sh",
    "verify-tonglingyu-release-ops-readiness.sh",
    "verify-tonglingyu-post-release-monitor.sh",
    "verify-tonglingyu-release-security.sh",
    "verify-tonglingyu-scoped-context-live.sh",
]
scripts_dir = Path("scripts")
present = {
    script: (scripts_dir / script).is_file()
    for script in required_scripts
}
print(json.dumps(
    {
        "object": "tonglingyu.remote_release_capabilities",
        "status": "ok",
        "scripts_present": present,
        "missing_scripts": [script for script, exists in present.items() if not exists],
        "secret_values_printed": False,
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
REMOTE
capabilities_exit=$?
set -e
if [[ "${capabilities_exit}" -eq 0 ]]; then
  python3 - "${REMOTE_CAPABILITIES_PATH}.tmp" "${REMOTE_CAPABILITIES_PATH}" <<'PY'
import json
import sys
from pathlib import Path

source = Path(sys.argv[1])
target = Path(sys.argv[2])
for line in reversed(source.read_text(encoding="utf-8", errors="replace").splitlines()):
    try:
        payload = json.loads(line)
    except json.JSONDecodeError:
        continue
    if isinstance(payload, dict):
        target.write_text(json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n", encoding="utf-8")
        break
PY
else
  printf '{"object":"tonglingyu.remote_release_capabilities","status":"failed","scripts_present":{},"missing_scripts":[],"secret_values_printed":false}\n' \
    >"${REMOTE_CAPABILITIES_PATH}"
fi
rm -f "${REMOTE_CAPABILITIES_PATH}.tmp"

python3 - "${REPORT_PATH}" "${RESULTS_JSONL}" "${REMOTE_CAPABILITIES_PATH}" \
  "${REMOTE_HOST}" "${REMOTE_PROJECT_DIR}" "${ARTIFACT_DIR}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    results_path_raw,
    capabilities_path_raw,
    remote_host,
    remote_project_dir,
    artifact_dir_raw,
) = sys.argv[1:7]


def file_sha256(path):
    path = Path(path)
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


results_path = Path(results_path_raw)
capabilities_path = Path(capabilities_path_raw)
gate_results = [
    json.loads(line)
    for line in results_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
]
capabilities = json.loads(capabilities_path.read_text(encoding="utf-8"))
missing_scripts = capabilities.get("missing_scripts") or []
gate_failures = [
    result["name"]
    for result in gate_results
    if result.get("status") != "passed"
]
payload = {
    "object": "tonglingyu.remote_live_gates",
    "schema_version": 1,
    "status": "ok" if not gate_failures else "failed",
    "remote_host": remote_host,
    "remote_project_dir_bound": bool(remote_project_dir.strip()),
    "artifact_dir": artifact_dir_raw,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "gate_results": gate_results,
    "remote_capabilities": {
        "path": str(capabilities_path),
        "sha256": file_sha256(capabilities_path),
        "missing_scripts": missing_scripts,
    },
    "checks": {
        "model_upstream_network_passed": "model_upstream_network" not in gate_failures,
        "openwebui_function_passed": "openwebui_function" not in gate_failures,
        "openwebui_admin_action_passed": "openwebui_admin_action" not in gate_failures,
        "strict_gateway_passed": "strict_gateway" not in gate_failures,
        "scoped_context_passed": "scoped_context" not in gate_failures,
        "latest_rqa_release_automation_present": not missing_scripts,
    },
    "production_ready_proven": False,
    "production_ready_blockers": [
        "remote_latest_rqa_release_automation_missing"
    ] if missing_scripts else [
        "full_live_release_automation_not_run"
    ],
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
report_path = Path(report_path_raw)
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(encoded + "\n", encoding="utf-8")
print(encoded)
if gate_failures:
    raise SystemExit(1)
PY
