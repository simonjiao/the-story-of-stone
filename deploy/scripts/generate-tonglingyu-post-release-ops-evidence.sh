#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_deploy_env_file_or_local

RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:?set TONGLINGYU_RELEASE_REPORT_PATH}"
OPERATOR="${TONGLINGYU_POST_RELEASE_OPS_OPERATOR:-${TONGLINGYU_RELEASE_OPERATOR:-}}"
ENVIRONMENT="${TONGLINGYU_POST_RELEASE_OPS_ENVIRONMENT:-${TONGLINGYU_RELEASE_ENVIRONMENT:-}}"
ARTIFACT_DIR="${TONGLINGYU_POST_RELEASE_OPS_ARTIFACT_DIR:-}"
WINDOW_MINUTES="${TONGLINGYU_POST_RELEASE_OPS_WINDOW_MINUTES:-${TONGLINGYU_RELEASE_POST_RELEASE_WINDOW_MINUTES:-60}}"
SAMPLE_INTERVAL_SECONDS="${TONGLINGYU_POST_RELEASE_OPS_SAMPLE_INTERVAL_SECONDS:-300}"
ENV_PATH="${TONGLINGYU_POST_RELEASE_OPS_ENV_PATH:-}"
COMPOSE_PROBE_SERVICE="${TONGLINGYU_POST_RELEASE_OPS_PROBE_SERVICE:-open-webui}"

if [[ -z "${OPERATOR// }" ]]; then
  echo "TONGLINGYU_RELEASE_OPERATOR is required for post-release ops evidence" >&2
  exit 1
fi
if [[ -z "${ENVIRONMENT// }" ]]; then
  echo "TONGLINGYU_RELEASE_ENVIRONMENT is required for post-release ops evidence" >&2
  exit 1
fi

RELEASE_REPORT_ABS="${RELEASE_REPORT_PATH}"
if [[ "${RELEASE_REPORT_ABS}" != /* ]]; then
  RELEASE_REPORT_ABS="${DEPLOY_DIR}/${RELEASE_REPORT_ABS}"
fi
if [[ ! -f "${RELEASE_REPORT_ABS}" ]]; then
  echo "release report not found: ${RELEASE_REPORT_ABS}" >&2
  exit 1
fi
if [[ -z "${ARTIFACT_DIR// }" ]]; then
  ARTIFACT_DIR="$(dirname -- "${RELEASE_REPORT_ABS}")/post-release-ops"
fi
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${DEPLOY_DIR}/${ARTIFACT_DIR}"
fi
if [[ -z "${ENV_PATH// }" ]]; then
  ENV_PATH="${ARTIFACT_DIR}/post-release-ops.env"
fi
if [[ "${ENV_PATH}" != /* ]]; then
  ENV_PATH="${DEPLOY_DIR}/${ENV_PATH}"
fi
mkdir -p "${ARTIFACT_DIR}" "$(dirname -- "${ENV_PATH}")"
cd "${DEPLOY_DIR}"

python3 - "${RELEASE_REPORT_ABS}" "${ARTIFACT_DIR}" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

report_path = Path(sys.argv[1])
artifact_dir = Path(sys.argv[2])
report = json.loads(report_path.read_text(encoding="utf-8"))
gates = report.get("gates") if isinstance(report.get("gates"), list) else []
gates_by_name = {
    gate.get("name"): gate
    for gate in gates
    if isinstance(gate, dict) and isinstance(gate.get("name"), str)
}


def gate_json(name):
    gate = gates_by_name.get(name)
    if not isinstance(gate, dict):
        raise SystemExit(f"missing gate: {name}")
    for raw in reversed(gate.get("stdout_tail") or []):
        if not isinstance(raw, str):
            continue
        try:
            value = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            return gate, value
    raise SystemExit(f"missing JSON stdout for gate: {name}")


live_gate_names = [
    "model_upstream_network",
    "strict_gateway",
    "openwebui_function",
    "openwebui_admin_action",
]
live_gates = {}
for name in live_gate_names:
    gate, value = gate_json(name)
    if gate.get("status") != "passed":
        raise SystemExit(f"live gate did not pass: {name}")
    live_gates[name] = value

_, admin_action = gate_json("openwebui_admin_action")
if admin_action.get("status") != "ok":
    raise SystemExit("openwebui admin action evidence did not pass")

_, restore = gate_json("rqa_backup_restore_drill")
if restore.get("status") != "ok":
    raise SystemExit("restore drill evidence did not pass")

_, ops_gate = gate_json("release_ops_readiness")
alert_policy = ops_gate.get("alert_policy")
if not isinstance(alert_policy, dict) or not alert_policy.get("low_cardinality_labels_only"):
    raise SystemExit("alert policy evidence missing or invalid")

artifacts = {
    "live-gate-evidence.json": {
        "object": "tonglingyu.post_release_live_gate_evidence",
        "schema_version": 1,
        "status": "ok",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "release_report_path": str(report_path),
        "live_gates": live_gates,
        "secret_values_printed": False,
    },
    "admin-action-evidence.json": {
        "object": "tonglingyu.post_release_admin_action_evidence",
        "schema_version": 1,
        "status": "ok",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "release_report_path": str(report_path),
        "admin_action": admin_action,
        "secret_values_printed": False,
    },
    "rollback-evidence.json": {
        "object": "tonglingyu.post_release_rollback_evidence",
        "schema_version": 1,
        "status": "ok",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "release_report_path": str(report_path),
        "backup": restore.get("backup") or {},
        "restore": restore.get("restore") or {},
        "checks": restore.get("checks") or {},
        "secret_values_printed": False,
    },
    "rto-rpo-evidence.json": {
        "object": "tonglingyu.post_release_rto_rpo_evidence",
        "schema_version": 1,
        "status": "ok",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "release_report_path": str(report_path),
        "rto": restore.get("rto") or {},
        "rpo": restore.get("rpo") or {},
        "secret_values_printed": False,
    },
    "alert-policy-evidence.json": {
        "object": "tonglingyu.post_release_alert_policy_evidence",
        "schema_version": 1,
        "status": "ok",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "release_report_path": str(report_path),
        "alert_policy": alert_policy,
        "secret_values_printed": False,
    },
}
for filename, payload in artifacts.items():
    (artifact_dir / filename).write_text(
        json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n",
        encoding="utf-8",
    )
PY

sample_log="${ARTIFACT_DIR}/post-release-monitor-samples.jsonl"
sample_summary="${ARTIFACT_DIR}/post-release-monitor-samples-summary.json"
monitor_evidence="${ARTIFACT_DIR}/post-release-monitor-evidence.json"
started_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
window_seconds="$((WINDOW_MINUTES * 60))"
deadline_epoch="$(( $(date -u '+%s') + window_seconds ))"
sample_index=0
: >"${sample_log}"

append_sample() {
  local status="$1"
  local detail_path="$2"
  local error_path="$3"
  python3 - "${sample_log}" "${sample_index}" "${status}" "${detail_path}" "${error_path}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

sample_log = Path(sys.argv[1])
sample_index = int(sys.argv[2])
status = sys.argv[3]
detail_path = Path(sys.argv[4])
error_path = Path(sys.argv[5])


def read_json(path):
    if not path.is_file():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def sha256(path):
    if not path.is_file():
        return ""
    return hashlib.sha256(path.read_bytes()).hexdigest()


error_text = ""
if error_path.is_file():
    error_text = error_path.read_text(encoding="utf-8", errors="replace")[-2000:]
payload = {
    "object": "tonglingyu.post_release_monitor_sample",
    "schema_version": 1,
    "sample_index": sample_index,
    "sampled_at": datetime.now(timezone.utc).isoformat(),
    "status": status,
    "checks": read_json(detail_path),
    "stderr_tail_sha256": hashlib.sha256(error_text.encode("utf-8")).hexdigest(),
    "secret_values_printed": False,
}
if status != "ok":
    payload["error_tail_sha256"] = sha256(error_path)
with sample_log.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n")
PY
}

while :; do
  sample_detail="$(mktemp)"
  sample_error="$(mktemp)"
  if docker compose exec -T -e TLY_ADMIN_KEY="${TONGLINGYU_ADMIN_API_KEY}" \
    "${COMPOSE_PROBE_SERVICE}" sh -lc '
set -eu
test -n "${TLY_ADMIN_KEY}"
curl -fsS -o /dev/null http://tonglingyu-gateway:8090/healthz
curl -fsS -H "Authorization: Bearer ${TLY_ADMIN_KEY}" \
  -o /tmp/tonglingyu-post-release-admin-metrics.json \
  http://tonglingyu-gateway:8090/v1/admin/metrics
python3 - <<'"'"'PY'"'"'
import json
from pathlib import Path

metrics = json.loads(Path("/tmp/tonglingyu-post-release-admin-metrics.json").read_text())
print(json.dumps({
    "gateway_health": True,
    "admin_metrics": True,
    "admin_metrics_key_count": len(metrics.keys()) if isinstance(metrics, dict) else 0,
}, ensure_ascii=True, sort_keys=True))
PY
' >"${sample_detail}" 2>"${sample_error}"; then
    append_sample "ok" "${sample_detail}" "${sample_error}"
  else
    append_sample "failed" "${sample_detail}" "${sample_error}"
  fi
  rm -f "${sample_detail}" "${sample_error}"
  sample_index=$((sample_index + 1))
  now_epoch="$(date -u '+%s')"
  if (( now_epoch >= deadline_epoch )); then
    break
  fi
  sleep_for="${SAMPLE_INTERVAL_SECONDS}"
  remaining="$((deadline_epoch - now_epoch))"
  if (( remaining < sleep_for )); then
    sleep_for="${remaining}"
  fi
  if (( sleep_for > 0 )); then
    sleep "${sleep_for}"
  fi
done
finished_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

python3 - "${sample_log}" "${sample_summary}" "${started_at}" "${finished_at}" \
  "${WINDOW_MINUTES}" <<'PY'
import json
import sys
from pathlib import Path

sample_log = Path(sys.argv[1])
sample_summary = Path(sys.argv[2])
started_at = sys.argv[3]
finished_at = sys.argv[4]
window_minutes = int(sys.argv[5])
samples = [
    json.loads(line)
    for line in sample_log.read_text(encoding="utf-8").splitlines()
    if line.strip()
]
failed = [item for item in samples if item.get("status") != "ok"]
payload = {
    "object": "tonglingyu.post_release_monitor_sample_summary",
    "schema_version": 1,
    "status": "ok" if not failed else "failed",
    "started_at": started_at,
    "finished_at": finished_at,
    "window_minutes": window_minutes,
    "sample_count": len(samples),
    "failed_sample_count": len(failed),
    "secret_values_printed": False,
}
sample_summary.write_text(
    json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if not failed else 1)
PY

TONGLINGYU_POST_RELEASE_MONITOR_REPORT_PATH="${monitor_evidence}" \
TONGLINGYU_POST_RELEASE_MONITOR_OPERATOR="${OPERATOR}" \
TONGLINGYU_POST_RELEASE_MONITOR_ENVIRONMENT="${ENVIRONMENT}" \
TONGLINGYU_POST_RELEASE_MONITOR_RELEASE_REPORT_PATH="${RELEASE_REPORT_ABS}" \
TONGLINGYU_POST_RELEASE_MONITOR_REF="${sample_log}" \
TONGLINGYU_POST_RELEASE_MONITOR_LIVE_GATE_REF="${ARTIFACT_DIR}/live-gate-evidence.json" \
TONGLINGYU_POST_RELEASE_MONITOR_ADMIN_ACTION_REF="${ARTIFACT_DIR}/admin-action-evidence.json" \
TONGLINGYU_POST_RELEASE_MONITOR_STARTED_AT="${started_at}" \
TONGLINGYU_POST_RELEASE_MONITOR_FINISHED_AT="${finished_at}" \
TONGLINGYU_POST_RELEASE_MONITOR_CONCLUSION=passed \
  "${SCRIPT_DIR}/verify-tonglingyu-post-release-monitor.sh" >/dev/null

python3 - "${ENV_PATH}" "${OPERATOR}" "${ARTIFACT_DIR}" "${sample_log}" \
  "${monitor_evidence}" "${WINDOW_MINUTES}" <<'PY'
import shlex
import sys
from pathlib import Path

env_path = Path(sys.argv[1])
operator = sys.argv[2]
artifact_dir = Path(sys.argv[3])
sample_log = Path(sys.argv[4])
monitor_evidence = Path(sys.argv[5])
window_minutes = sys.argv[6]
values = {
    "TONGLINGYU_RELEASE_OPERATOR": operator,
    "TONGLINGYU_RELEASE_ROLLBACK_EVIDENCE_REF": str(artifact_dir / "rollback-evidence.json"),
    "TONGLINGYU_RELEASE_RTO_RPO_EVIDENCE_REF": str(artifact_dir / "rto-rpo-evidence.json"),
    "TONGLINGYU_RELEASE_ALERT_EVIDENCE_REF": str(artifact_dir / "alert-policy-evidence.json"),
    "TONGLINGYU_RELEASE_POST_RELEASE_MONITOR_REF": str(sample_log),
    "TONGLINGYU_RELEASE_POST_RELEASE_MONITOR_EVIDENCE": str(monitor_evidence),
    "TONGLINGYU_RELEASE_POST_RELEASE_LIVE_GATE_REF": str(artifact_dir / "live-gate-evidence.json"),
    "TONGLINGYU_RELEASE_POST_RELEASE_ADMIN_ACTION_REF": str(artifact_dir / "admin-action-evidence.json"),
    "TONGLINGYU_RELEASE_POST_RELEASE_CONCLUSION": "passed",
    "TONGLINGYU_RELEASE_POST_RELEASE_WINDOW_MINUTES": window_minutes,
}
env_path.write_text(
    "".join(f"export {key}={shlex.quote(value)}\n" for key, value in values.items()),
    encoding="utf-8",
)
PY

python3 - "${ARTIFACT_DIR}" "${ENV_PATH}" "${monitor_evidence}" "${sample_summary}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

artifact_dir = Path(sys.argv[1])
env_path = Path(sys.argv[2])
monitor_evidence = Path(sys.argv[3])
sample_summary = Path(sys.argv[4])


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else ""


payload = {
    "object": "tonglingyu.post_release_ops_evidence_generation",
    "schema_version": 1,
    "status": "ok",
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "artifact_dir": str(artifact_dir),
    "env_path": str(env_path),
    "post_release_monitor_evidence": str(monitor_evidence),
    "post_release_monitor_evidence_sha256": sha256(monitor_evidence),
    "sample_summary": str(sample_summary),
    "sample_summary_sha256": sha256(sample_summary),
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
PY
