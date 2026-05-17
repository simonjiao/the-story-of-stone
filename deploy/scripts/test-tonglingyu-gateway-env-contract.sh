#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

ENV_FILE="${WORK_DIR}/deploy.env"
CHECK_REPORT="${WORK_DIR}/check.json"
APPLY_REPORT="${WORK_DIR}/apply.json"
SECOND_APPLY_REPORT="${WORK_DIR}/second-apply.json"
BAD_ENV_FILE="${WORK_DIR}/bad.env"
QUOTED_ENV_FILE="${WORK_DIR}/quoted.env"
QUOTED_REPORT="${WORK_DIR}/quoted-apply.json"
QUOTE_ONLY_ENV_FILE="${WORK_DIR}/quote-only.env"
QUOTE_ONLY_REPORT="${WORK_DIR}/quote-only-apply.json"

cat >"${ENV_FILE}" <<'EOF'
HERMES_API_KEY=hermes-fixture-key
OPEN_WEBUI_OPENAI_API_BASE_URLS=http://tonglingyu-gateway:8090/v1;http://agent-orchestrator:8080/v1
OPEN_WEBUI_OPENAI_API_KEYS=legacy-gateway-key;agent-orchestrator-key
TONGLINGYU_MODEL_ID=tonglingyu
EOF

TONGLINGYU_DEPLOY_ENV_FILE="${ENV_FILE}" \
  "${SCRIPT_DIR}/ensure-tonglingyu-gateway-env.sh" --check >"${CHECK_REPORT}"
python3 - "${CHECK_REPORT}" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["status"] == "needs_update", report
assert "OPEN_WEBUI_OPENAI_API_BASE_URLS" in report["changed_keys"], report
assert "OPEN_WEBUI_OPENAI_API_KEYS" in report["changed_keys"], report
assert "TONGLINGYU_GATEWAY_API_KEY" in report["changed_keys"], report
assert "TONGLINGYU_ADMIN_API_KEY" in report["changed_keys"], report
assert report["secret_values_printed"] is False, report
PY

TONGLINGYU_DEPLOY_ENV_FILE="${ENV_FILE}" \
  "${SCRIPT_DIR}/ensure-tonglingyu-gateway-env.sh" --apply >"${APPLY_REPORT}"
python3 - "${ENV_FILE}" "${APPLY_REPORT}" <<'PY'
import json
import sys
from pathlib import Path

env_path, report_path = sys.argv[1:3]
report = json.load(open(report_path, encoding="utf-8"))
assert report["status"] == "updated", report
assert report["backup_created"] is True, report
data = {}
for line in Path(env_path).read_text(encoding="utf-8").splitlines():
    if "=" in line and not line.lstrip().startswith("#"):
        key, value = line.split("=", 1)
        data[key] = value
gateway = data["TONGLINGYU_GATEWAY_API_KEY"]
admin = data["TONGLINGYU_ADMIN_API_KEY"]
provider_keys = data["OPEN_WEBUI_OPENAI_API_KEYS"].split(";")
assert gateway, data
assert admin, data
assert gateway != admin, data
assert data["OPEN_WEBUI_OPENAI_API_BASE_URLS"] == "http://tonglingyu-gateway:8090/v1", data
assert provider_keys == [gateway], data
assert admin not in provider_keys, data
assert data["TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY"] == "false", data
output = Path(report_path).read_text(encoding="utf-8")
assert gateway not in output, report
assert admin not in output, report
PY

TONGLINGYU_DEPLOY_ENV_FILE="${ENV_FILE}" \
  "${SCRIPT_DIR}/ensure-tonglingyu-gateway-env.sh" --apply >"${SECOND_APPLY_REPORT}"
python3 - "${SECOND_APPLY_REPORT}" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
assert report["status"] == "ok", report
assert report["changed_keys"] == [], report
assert report["backup_created"] is False, report
PY

cat >"${QUOTED_ENV_FILE}" <<'EOF'
HERMES_API_KEY=hermes-fixture-key
OPEN_WEBUI_OPENAI_API_BASE_URLS="http://tonglingyu-gateway:8090/v1;http://agent-orchestrator:8080/v1"
OPEN_WEBUI_OPENAI_API_KEYS=legacy-gateway-key;agent-orchestrator-key"
TONGLINGYU_GATEWAY_API_KEY=tlyg_fixture_gateway
TONGLINGYU_ADMIN_API_KEY=tlya_fixture_admin
TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY=true
EOF
TONGLINGYU_DEPLOY_ENV_FILE="${QUOTED_ENV_FILE}" \
  "${SCRIPT_DIR}/ensure-tonglingyu-gateway-env.sh" --apply >"${QUOTED_REPORT}"
python3 - "${QUOTED_ENV_FILE}" "${QUOTED_REPORT}" <<'PY'
import json
import sys
from pathlib import Path

env_path, report_path = sys.argv[1:3]
report = json.load(open(report_path, encoding="utf-8"))
assert report["status"] == "updated", report
data = {}
for line in Path(env_path).read_text(encoding="utf-8").splitlines():
    if "=" in line and not line.lstrip().startswith("#"):
        key, value = line.split("=", 1)
        data[key] = value
provider_keys = data["OPEN_WEBUI_OPENAI_API_KEYS"].split(";")
assert data["OPEN_WEBUI_OPENAI_API_BASE_URLS"] == "http://tonglingyu-gateway:8090/v1", data
assert provider_keys == ["tlyg_fixture_gateway"], data
assert not data["OPEN_WEBUI_OPENAI_API_KEYS"].startswith('"'), data
assert not data["OPEN_WEBUI_OPENAI_API_KEYS"].endswith('"'), data
assert data["TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY"] == "false", data
PY

cat >"${QUOTE_ONLY_ENV_FILE}" <<'EOF'
HERMES_API_KEY=hermes-fixture-key
OPEN_WEBUI_OPENAI_API_BASE_URLS=http://tonglingyu-gateway:8090/v1;http://agent-orchestrator:8080/v1
OPEN_WEBUI_OPENAI_API_KEYS=tlyg_fixture_gateway;agent-orchestrator-key"
TONGLINGYU_GATEWAY_API_KEY=tlyg_fixture_gateway
TONGLINGYU_ADMIN_API_KEY=tlya_fixture_admin
TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY=false
EOF
TONGLINGYU_DEPLOY_ENV_FILE="${QUOTE_ONLY_ENV_FILE}" \
  "${SCRIPT_DIR}/ensure-tonglingyu-gateway-env.sh" --apply >"${QUOTE_ONLY_REPORT}"
python3 - "${QUOTE_ONLY_ENV_FILE}" "${QUOTE_ONLY_REPORT}" <<'PY'
import json
import sys
from pathlib import Path

env_path, report_path = sys.argv[1:3]
report = json.load(open(report_path, encoding="utf-8"))
assert report["status"] == "updated", report
line = next(
    line for line in Path(env_path).read_text(encoding="utf-8").splitlines()
    if line.startswith("OPEN_WEBUI_OPENAI_API_KEYS=")
)
assert line == "OPEN_WEBUI_OPENAI_API_KEYS=tlyg_fixture_gateway", line
PY

cat >"${BAD_ENV_FILE}" <<'EOF'
TONGLINGYU_GATEWAY_API_KEY=same-key
TONGLINGYU_ADMIN_API_KEY=same-key
OPEN_WEBUI_OPENAI_API_KEYS=same-key
EOF
if TONGLINGYU_DEPLOY_ENV_FILE="${BAD_ENV_FILE}" \
  "${SCRIPT_DIR}/ensure-tonglingyu-gateway-env.sh" --check >"${WORK_DIR}/bad.out" 2>"${WORK_DIR}/bad.err"; then
  echo "overlapping Gateway/admin env unexpectedly passed" >&2
  exit 1
fi
grep -q "must not match" "${WORK_DIR}/bad.err"

echo "tonglingyu gateway env contract passed"
