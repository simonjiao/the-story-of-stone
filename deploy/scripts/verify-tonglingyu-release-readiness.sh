#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
WORK_DIR="$(mktemp -d)"
RESULTS_JSONL="${WORK_DIR}/results.jsonl"
REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:-}"
trap 'rm -rf "${WORK_DIR}"' EXIT

cd "${DEPLOY_DIR}"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

append_result() {
  local name="$1"
  local status="$2"
  local required="$3"
  local reason="$4"
  local stdout_path="${5:-}"
  local stderr_path="${6:-}"
  python3 - "${RESULTS_JSONL}" "${name}" "${status}" "${required}" "${reason}" \
    "${stdout_path}" "${stderr_path}" <<'PY'
import json
import sys

results_path, name, status, required, reason, stdout_path, stderr_path = sys.argv[1:8]


def tail(path):
    if not path:
        return []
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            return [line.rstrip("\n") for line in handle.readlines()[-20:]]
    except FileNotFoundError:
        return []


with open(results_path, "a", encoding="utf-8") as handle:
    handle.write(json.dumps(
        {
            "name": name,
            "status": status,
            "required": required == "true",
            "reason": reason,
            "stdout_tail": tail(stdout_path),
            "stderr_tail": tail(stderr_path),
        },
        ensure_ascii=True,
        sort_keys=True,
    ))
    handle.write("\n")
PY
}

run_gate() {
  local name="$1"
  local required="$2"
  shift 2
  local stdout_path="${WORK_DIR}/${name}.stdout"
  local stderr_path="${WORK_DIR}/${name}.stderr"
  if "$@" >"${stdout_path}" 2>"${stderr_path}"; then
    append_result "${name}" "passed" "${required}" "" "${stdout_path}" "${stderr_path}"
    return 0
  fi
  append_result "${name}" "failed" "${required}" "" "${stdout_path}" "${stderr_path}"
  if [[ "${required}" == "true" ]]; then
    return 1
  fi
  return 0
}

skip_gate() {
  append_result "$1" "skipped" "$2" "$3"
}

failed=0
run_gate "runtime_config" "true" "${SCRIPT_DIR}/verify-tonglingyu-runtime-config.sh" || failed=1

require_live="false"
if is_true "${TONGLINGYU_RELEASE_REQUIRE_LIVE:-false}"; then
  require_live="true"
fi

verify_strict_gateway="false"
if [[ "${require_live}" == "true" ]] || is_true "${TONGLINGYU_RELEASE_VERIFY_STRICT_GATEWAY:-false}"; then
  verify_strict_gateway="true"
fi

if [[ "${verify_strict_gateway}" == "true" ]]; then
  run_gate "strict_gateway" "true" \
    "${SCRIPT_DIR}/verify-tonglingyu-strict-gateway.sh" || failed=1
else
  skip_gate "strict_gateway" "false" \
    "set TONGLINGYU_RELEASE_VERIFY_STRICT_GATEWAY=true or TONGLINGYU_RELEASE_REQUIRE_LIVE=true"
fi

verify_openwebui_function="false"
if [[ "${require_live}" == "true" ]] || is_true "${TONGLINGYU_RELEASE_VERIFY_OPENWEBUI_FUNCTION:-false}"; then
  verify_openwebui_function="true"
fi

if [[ "${verify_openwebui_function}" == "true" ]]; then
  run_gate "openwebui_function" "true" \
    "${SCRIPT_DIR}/verify-openwebui-function.sh" || failed=1
else
  skip_gate "openwebui_function" "false" \
    "set TONGLINGYU_RELEASE_VERIFY_OPENWEBUI_FUNCTION=true or TONGLINGYU_RELEASE_REQUIRE_LIVE=true"
fi

python3 - "${RESULTS_JSONL}" "${REPORT_PATH}" <<'PY'
import json
import sys
from datetime import datetime, timezone

results_path, report_path = sys.argv[1:3]
with open(results_path, "r", encoding="utf-8") as handle:
    gates = [json.loads(line) for line in handle if line.strip()]

required_failures = [
    gate["name"]
    for gate in gates
    if gate["required"] and gate["status"] != "passed"
]
skipped = [gate["name"] for gate in gates if gate["status"] == "skipped"]
status = "failed" if required_failures else "passed"
if status == "passed" and skipped:
    status = "passed_with_skipped_gates"

report = {
    "status": status,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "gates": gates,
    "required_failures": required_failures,
    "remaining_manual_checks": [
        "Open WebUI browser-side ordinary-user model visibility",
        "Open WebUI browser-side admin audit entry visibility",
        "Open WebUI streaming chat UX against the live public endpoint",
        "Human confirmation that existing Open WebUI webui.db persisted settings match env-rendered provider settings",
    ],
}
encoded = json.dumps(report, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path:
    with open(report_path, "w", encoding="utf-8") as handle:
        handle.write(encoded)
        handle.write("\n")
PY

if [[ "${failed}" -ne 0 ]]; then
  exit 1
fi
