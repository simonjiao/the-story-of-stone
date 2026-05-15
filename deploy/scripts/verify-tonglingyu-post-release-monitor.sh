#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

REPORT_PATH="${TONGLINGYU_POST_RELEASE_MONITOR_REPORT_PATH:-}"
OPERATOR="${TONGLINGYU_POST_RELEASE_MONITOR_OPERATOR:-${TONGLINGYU_RELEASE_OPERATOR:-}}"
ENVIRONMENT="${TONGLINGYU_POST_RELEASE_MONITOR_ENVIRONMENT:-${TONGLINGYU_RELEASE_ENVIRONMENT:-}}"
RELEASE_REPORT_PATH="${TONGLINGYU_POST_RELEASE_MONITOR_RELEASE_REPORT_PATH:-${TONGLINGYU_RELEASE_REPORT_PATH:-}}"
MONITOR_REF="${TONGLINGYU_POST_RELEASE_MONITOR_REF:-${TONGLINGYU_RELEASE_POST_RELEASE_MONITOR_REF:-}}"
LIVE_GATE_REF="${TONGLINGYU_POST_RELEASE_MONITOR_LIVE_GATE_REF:-${TONGLINGYU_RELEASE_POST_RELEASE_LIVE_GATE_REF:-}}"
ADMIN_ACTION_REF="${TONGLINGYU_POST_RELEASE_MONITOR_ADMIN_ACTION_REF:-${TONGLINGYU_RELEASE_POST_RELEASE_ADMIN_ACTION_REF:-}}"
STARTED_AT="${TONGLINGYU_POST_RELEASE_MONITOR_STARTED_AT:-}"
FINISHED_AT="${TONGLINGYU_POST_RELEASE_MONITOR_FINISHED_AT:-}"
CONCLUSION="${TONGLINGYU_POST_RELEASE_MONITOR_CONCLUSION:-${TONGLINGYU_RELEASE_POST_RELEASE_CONCLUSION:-}}"

python3 - "${REPO_DIR}" "${REPORT_PATH}" "${OPERATOR}" "${ENVIRONMENT}" \
  "${RELEASE_REPORT_PATH}" "${MONITOR_REF}" "${LIVE_GATE_REF}" \
  "${ADMIN_ACTION_REF}" "${STARTED_AT}" "${FINISHED_AT}" "${CONCLUSION}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

(
    repo_dir_raw,
    report_path_raw,
    operator,
    environment,
    release_report_path_raw,
    monitor_ref,
    live_gate_ref,
    admin_action_ref,
    started_at_raw,
    finished_at_raw,
    conclusion,
) = sys.argv[1:12]

repo_dir = Path(repo_dir_raw)
errors = []
live_gate_names = {
    "model_upstream_network",
    "strict_gateway",
    "openwebui_function",
    "openwebui_admin_action",
}
secret_needles = (
    "api_key",
    "apikey",
    "authorization:",
    "bearer ",
    "password",
    "secret" + "=",
    "sk-",
    "token" + "=",
)


def now_iso():
    return datetime.now(timezone.utc).isoformat()


def parse_timestamp(value):
    if not isinstance(value, str) or not value.strip():
        return None
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.astimezone(timezone.utc)


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_path(path_raw):
    value = str(path_raw or "").strip()
    if not value:
        return None
    path = Path(value)
    if path.is_absolute():
        return path
    return repo_dir / path


def ref_kind(value):
    value = str(value or "").strip()
    if not value:
        return ""
    parsed = urlparse(value)
    if parsed.scheme in {"http", "https"} and parsed.netloc:
        return "url"
    if value.startswith("runbook:"):
        return "runbook"
    if value.startswith("artifact:"):
        return "artifact"
    if value.startswith("file:"):
        return "file"
    if value.startswith("/"):
        return "local_file"
    return ""


def ref_valid(value):
    value = str(value or "").strip()
    if not value:
        return False
    lowered = value.lower()
    if any(needle in lowered for needle in secret_needles):
        return False
    return bool(ref_kind(value))


def checked_ref(value):
    return {
        "ref": str(value or "").strip(),
        "kind": ref_kind(value),
        "valid": ref_valid(value),
    }


release_report_path = resolve_path(release_report_path_raw)
release_report = {}
release_report_sha256 = ""
if release_report_path is None:
    errors.append("release_report_path_missing")
elif not release_report_path.is_file():
    errors.append("release_report_not_found")
else:
    release_report_sha256 = file_sha256(release_report_path)
    try:
        release_report = json.loads(release_report_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        errors.append("release_report_json_invalid")
        release_report = {}

if release_report:
    if release_report.get("object") != "tonglingyu.release_readiness_report":
        errors.append("release_report_object_invalid")
    if release_report.get("schema_version") != 1:
        errors.append("release_report_schema_version_invalid")
    if release_report.get("require_live") is not True:
        errors.append("release_report_not_live")

gates = release_report.get("gates") if isinstance(release_report.get("gates"), list) else []
gate_statuses = {
    gate.get("name"): gate.get("status")
    for gate in gates
    if isinstance(gate, dict) and isinstance(gate.get("name"), str)
}
missing_live_gates = sorted(live_gate_names - set(gate_statuses))
failed_live_gates = sorted(
    name for name in live_gate_names if gate_statuses.get(name) != "passed"
)
if missing_live_gates:
    errors.append("release_report_live_gates_missing=" + ",".join(missing_live_gates))
if failed_live_gates:
    errors.append("release_report_live_gates_not_passed=" + ",".join(failed_live_gates))

started_at = parse_timestamp(started_at_raw)
finished_at = parse_timestamp(finished_at_raw)
if started_at is None:
    errors.append("started_at_missing_or_invalid")
if finished_at is None:
    errors.append("finished_at_missing_or_invalid")
window_minutes = 0
if started_at is not None and finished_at is not None:
    if finished_at <= started_at:
        errors.append("finished_at_not_after_started_at")
    window_minutes = int((finished_at - started_at).total_seconds() // 60)
    if window_minutes < 60:
        errors.append("monitor_window_too_short")

monitor_ref_checked = checked_ref(monitor_ref)
live_gate_ref_checked = checked_ref(live_gate_ref)
admin_action_ref_checked = checked_ref(admin_action_ref)
for name, ref in (
    ("monitor_ref", monitor_ref_checked),
    ("live_gate_evidence_ref", live_gate_ref_checked),
    ("admin_action_or_api_evidence_ref", admin_action_ref_checked),
):
    if not ref["valid"]:
        errors.append(f"{name}_missing_or_invalid")
if not operator.strip():
    errors.append("operator_missing")
if not environment.strip():
    errors.append("environment_missing")
if conclusion.strip() != "passed":
    errors.append("conclusion_not_passed")

payload = {
    "object": "tonglingyu.post_release_monitor",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "monitor_policy_version": "tonglingyu-post-release-monitor-v1",
    "generated_at": now_iso(),
    "operator": operator.strip(),
    "environment": environment.strip(),
    "started_at": started_at.isoformat() if started_at else "",
    "finished_at": finished_at.isoformat() if finished_at else "",
    "window_minutes": window_minutes,
    "conclusion": conclusion.strip() or "pending",
    "release_report": {
        "path": str(release_report_path) if release_report_path else "",
        "sha256": release_report_sha256,
        "require_live": release_report.get("require_live") is True,
        "production_release_ready": release_report.get("production_release_ready") is True,
        "generated_at": release_report.get("generated_at") or "",
        "live_gate_statuses": gate_statuses,
        "missing_live_gates": missing_live_gates,
        "failed_live_gates": failed_live_gates,
    },
    "evidence_refs": {
        "monitor_ref": monitor_ref_checked,
        "live_gate_evidence_ref": live_gate_ref_checked,
        "admin_action_or_api_evidence_ref": admin_action_ref_checked,
    },
    "checks": {
        "release_report_exists": bool(release_report_sha256),
        "release_report_requires_live": release_report.get("require_live") is True,
        "live_gate_statuses_passed": not missing_live_gates and not failed_live_gates,
        "admin_action_or_api_evidence_ref_valid": admin_action_ref_checked["valid"],
        "monitor_window_at_least_60_minutes": window_minutes >= 60,
        "operator_environment_recorded": bool(operator.strip() and environment.strip()),
        "conclusion_passed": conclusion.strip() == "passed",
    },
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path_raw:
    report_path = resolve_path(report_path_raw)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
if errors:
    raise SystemExit(1)
PY
