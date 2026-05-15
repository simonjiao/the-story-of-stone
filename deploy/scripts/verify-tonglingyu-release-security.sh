#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

DEPENDENCY_SCAN_PATH="${TONGLINGYU_RELEASE_SECURITY_DEPENDENCY_SCAN_PATH:-}"
IMAGE_SCAN_PATH="${TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_PATH:-}"
RISK_ACCEPTANCE_PATH="${TONGLINGYU_RELEASE_SECURITY_RISK_ACCEPTANCE_PATH:-}"
RUN_TRIVY="${TONGLINGYU_RELEASE_SECURITY_RUN_TRIVY:-false}"
REPORT_PATH="${TONGLINGYU_RELEASE_SECURITY_REPORT_PATH:-}"

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

DEPENDENCY_SCAN_REPORT="${WORK_DIR}/dependency-scan.json"
IMAGE_SCAN_REPORT="${WORK_DIR}/image-scan.json"

if [[ -n "${DEPENDENCY_SCAN_PATH}" ]]; then
  cp "${DEPENDENCY_SCAN_PATH}" "${DEPENDENCY_SCAN_REPORT}"
elif cargo audit --version >/dev/null 2>&1; then
  if ! (
    cd "${REPO_DIR}/agent-platform"
    cargo audit --json >"${DEPENDENCY_SCAN_REPORT}" 2>"${WORK_DIR}/cargo-audit.stderr"
  ); then
    printf '{"object":"tonglingyu.security_scan_result","scan_type":"dependency","status":"failed","scanner":"cargo-audit","critical_count":1,"high_count":0,"secret_values_printed":false}\n' >"${DEPENDENCY_SCAN_REPORT}"
  fi
fi

if [[ -n "${IMAGE_SCAN_PATH}" ]]; then
  cp "${IMAGE_SCAN_PATH}" "${IMAGE_SCAN_REPORT}"
elif is_true "${RUN_TRIVY}" && command -v trivy >/dev/null 2>&1; then
  python3 - "${REPO_DIR}" "${WORK_DIR}/images.txt" <<'PY'
import re
import sys
from pathlib import Path

repo_dir = Path(sys.argv[1])
target = Path(sys.argv[2])
compose = repo_dir / "deploy" / "docker-compose.yml"
images = []
for raw_line in compose.read_text(encoding="utf-8").splitlines():
    match = re.match(r"\s*image:\s+(.+?)\s*$", raw_line)
    if match:
        images.append(match.group(1).strip().strip('"').strip("'"))
target.write_text("\n".join(images) + "\n", encoding="utf-8")
PY
  trivy_status="passed"
  critical_count=0
  high_count=0
  while IFS= read -r image_ref; do
    [[ -n "${image_ref}" ]] || continue
    image_hash="$(
      python3 - "${image_ref}" <<'PY'
import hashlib
import sys

print(hashlib.sha256(sys.argv[1].encode("utf-8")).hexdigest())
PY
    )"
    if ! trivy image --quiet --format json "${image_ref}" \
      >"${WORK_DIR}/trivy-${image_hash}.json" \
      2>"${WORK_DIR}/trivy-${image_hash}.stderr"; then
      trivy_status="failed"
      critical_count=$((critical_count + 1))
    fi
  done <"${WORK_DIR}/images.txt"
  python3 - "${IMAGE_SCAN_REPORT}" "${trivy_status}" "${critical_count}" "${high_count}" <<'PY'
import json
import sys

target, status, critical_count, high_count = sys.argv[1:5]
Path = __import__("pathlib").Path
Path(target).write_text(
    json.dumps(
        {
            "object": "tonglingyu.security_scan_result",
            "scan_type": "image",
            "status": status,
            "scanner": "trivy",
            "critical_count": int(critical_count),
            "high_count": int(high_count),
            "secret_values_printed": False,
        },
        ensure_ascii=True,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY
fi

python3 - "${REPO_DIR}" "${DEPENDENCY_SCAN_REPORT}" "${IMAGE_SCAN_REPORT}" \
  "${RISK_ACCEPTANCE_PATH}" "${REPORT_PATH}" <<'PY'
import hashlib
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    repo_dir_raw,
    dependency_scan_path_raw,
    image_scan_path_raw,
    risk_acceptance_path_raw,
    report_path_raw,
) = sys.argv[1:6]

repo_dir = Path(repo_dir_raw)
dependency_scan_path = Path(dependency_scan_path_raw)
image_scan_path = Path(image_scan_path_raw)
risk_acceptance_path = Path(risk_acceptance_path_raw) if risk_acceptance_path_raw else None
report_path = Path(report_path_raw) if report_path_raw else None
errors = []


def now_iso():
    return datetime.now(timezone.utc).isoformat()


def parse_timestamp(value):
    if not isinstance(value, str):
        return None
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None or parsed.tzinfo.utcoffset(parsed) is None:
        return None
    return parsed.astimezone(timezone.utc)


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json_file(path):
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def safe_scan_result(path, scan_type):
    if not path.is_file():
        return {
            "status": "missing",
            "scanner": "",
            "critical_count": None,
            "high_count": None,
            "report_sha256": "",
        }, [f"{scan_type}_scan_missing"]
    try:
        value = load_json_file(path)
    except (OSError, json.JSONDecodeError):
        return {
            "status": "failed",
            "scanner": "",
            "critical_count": None,
            "high_count": None,
            "report_sha256": file_sha256(path) if path.is_file() else "",
        }, [f"{scan_type}_scan_report_invalid"]
    if scan_type == "dependency" and isinstance(value.get("vulnerabilities"), dict):
        vulnerabilities = value["vulnerabilities"].get("list") or []
        finding_count = len(vulnerabilities) if isinstance(vulnerabilities, list) else 1
        value = {
            "status": "passed" if finding_count == 0 else "failed",
            "scanner": "cargo-audit",
            "critical_count": finding_count,
            "high_count": 0,
            "secret_values_printed": False,
        }
    elif scan_type == "image" and isinstance(value.get("Results"), list):
        critical_count = 0
        high_count = 0
        for result in value.get("Results") or []:
            for vulnerability in result.get("Vulnerabilities") or []:
                severity = str(vulnerability.get("Severity") or "").upper()
                if severity == "CRITICAL":
                    critical_count += 1
                elif severity == "HIGH":
                    high_count += 1
        value = {
            "status": "passed" if critical_count == 0 and high_count == 0 else "failed",
            "scanner": "trivy",
            "critical_count": critical_count,
            "high_count": high_count,
            "secret_values_printed": False,
        }
    status = value.get("status")
    critical_count = value.get("critical_count", 0)
    high_count = value.get("high_count", 0)
    result = {
        "status": status if isinstance(status, str) else "failed",
        "scanner": str(value.get("scanner") or ""),
        "critical_count": critical_count if isinstance(critical_count, int) else None,
        "high_count": high_count if isinstance(high_count, int) else None,
        "report_sha256": file_sha256(path),
    }
    result_errors = []
    if value.get("secret_values_printed") is True:
        result_errors.append(f"{scan_type}_scan_secret_values_printed")
    if result["status"] != "passed":
        result_errors.append(f"{scan_type}_scan_not_passed")
    if not isinstance(critical_count, int) or critical_count < 0:
        result_errors.append(f"{scan_type}_critical_count_invalid")
    elif critical_count > 0:
        result_errors.append(f"{scan_type}_critical_findings_present")
    if not isinstance(high_count, int) or high_count < 0:
        result_errors.append(f"{scan_type}_high_count_invalid")
    elif high_count > 0:
        result_errors.append(f"{scan_type}_high_findings_present")
    if not result["scanner"]:
        result_errors.append(f"{scan_type}_scanner_missing")
    return result, result_errors


def git_tracked_files(paths):
    command = ["git", "-C", str(repo_dir), "ls-files", *paths]
    output = subprocess.check_output(command, text=True)
    return [repo_dir / line for line in output.splitlines() if line.strip()]


secret_patterns = {
    "hardcoded_openai_key": re.compile(r"sk-[A-Za-z0-9][A-Za-z0-9_-]{18,}"),
    "hardcoded_github_token": re.compile(r"(ghp_|github_pat_)[A-Za-z0-9_]{20,}"),
    "hardcoded_slack_token": re.compile(r"xox[baprs]-[A-Za-z0-9-]{20,}"),
    "private_key_material": re.compile(r"BEGIN (RSA |EC |OPENSSH |)PRIVATE KEY"),
    "literal_password_assignment": re.compile(
        r"(?i)\b(password|secret|token|api[_-]?key)\s*[:=]\s*['\"][^'$\"{][^'\"]{8,}"
    ),
}
dangerous_patterns = {
    "curl_pipe_shell": re.compile(r"curl\b.*\|\s*(ba)?sh\b"),
    "privileged_container": re.compile(r"\bprivileged:\s*true\b"),
    "docker_sock_mount": re.compile(r"/var/run/docker\.sock"),
    "world_writable_chmod": re.compile(r"chmod\s+777\b"),
}


def scan_release_scripts():
    files = git_tracked_files(
        [
            "deploy/scripts",
            "deploy/open-webui/functions",
            "deploy/docker-compose.yml",
            "agent-platform/Dockerfile",
            "agent-platform/crates/tonglingyu-gateway/Dockerfile",
        ]
    )
    finding_types = set()
    for path in files:
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            finding_types.add("tracked_release_file_unreadable")
            continue
        for name, pattern in {**secret_patterns, **dangerous_patterns}.items():
            if pattern.search(text):
                finding_types.add(name)
    return {
        "status": "passed" if not finding_types else "failed",
        "scanner": "tonglingyu-release-script-static-policy-v1",
        "scanned_file_count": len(files),
        "finding_count": len(finding_types),
        "finding_types": sorted(finding_types),
    }


def compose_image_policy():
    compose_path = repo_dir / "deploy" / "docker-compose.yml"
    text = compose_path.read_text(encoding="utf-8")
    refs = []
    for raw_line in text.splitlines():
        match = re.match(r"\s*image:\s+(.+?)\s*$", raw_line)
        if match:
            refs.append(match.group(1).strip().strip('"').strip("'"))
    mutable = []
    digest_missing = []
    for ref in refs:
        if "@sha256:" not in ref:
            digest_missing.append(ref)
        if re.search(r"(:|\:-)(latest|main)([}:]|$)", ref):
            mutable.append(ref)
    return {
        "image_count": len(refs),
        "mutable_tag_count": len(mutable),
        "digest_missing_count": len(digest_missing),
    }


def load_risk_acceptance():
    if risk_acceptance_path is None:
        return {
            "present": False,
            "accepted_findings": [],
        }, []
    try:
        value = load_json_file(risk_acceptance_path)
    except (OSError, json.JSONDecodeError):
        return {
            "present": True,
            "accepted_findings": [],
        }, ["risk_acceptance_invalid"]
    accepted_findings = value.get("accepted_findings")
    if not isinstance(accepted_findings, list) or not all(
        isinstance(item, str) and item for item in accepted_findings
    ):
        accepted_findings = []
    result = {
        "present": True,
        "accepted_risk_id": str(value.get("accepted_risk_id") or ""),
        "risk_owner": str(value.get("risk_owner") or ""),
        "approved_at": str(value.get("approved_at") or ""),
        "expires_at": str(value.get("expires_at") or ""),
        "accepted_findings": sorted(set(accepted_findings)),
        "report_sha256": file_sha256(risk_acceptance_path),
    }
    result_errors = []
    if value.get("object") != "tonglingyu.release_security_risk_acceptance":
        result_errors.append("risk_acceptance_object_invalid")
    if value.get("status") != "approved":
        result_errors.append("risk_acceptance_not_approved")
    if not result["accepted_risk_id"]:
        result_errors.append("risk_acceptance_id_missing")
    if not result["risk_owner"]:
        result_errors.append("risk_acceptance_owner_missing")
    approved_at = parse_timestamp(result["approved_at"])
    expires_at = parse_timestamp(result["expires_at"])
    if approved_at is None:
        result_errors.append("risk_acceptance_approved_at_invalid")
    if expires_at is None:
        result_errors.append("risk_acceptance_expires_at_invalid")
    elif expires_at <= datetime.now(timezone.utc):
        result_errors.append("risk_acceptance_expired")
    if not accepted_findings:
        result_errors.append("risk_acceptance_findings_missing")
    return result, result_errors


dependency_scan, dependency_errors = safe_scan_result(dependency_scan_path, "dependency")
image_scan, image_errors = safe_scan_result(image_scan_path, "image")
release_script_scan = scan_release_scripts()
script_errors = []
if release_script_scan["status"] != "passed":
    script_errors.append("release_script_scan_not_passed")

image_policy = compose_image_policy()
if image_policy["image_count"] <= 0:
    image_errors.append("image_inventory_empty")
if image_policy["mutable_tag_count"] > 0:
    image_errors.append("mutable_image_tags_present")
if image_policy["digest_missing_count"] > 0:
    image_errors.append("image_digest_missing")
image_scan.update(image_policy)

risk_acceptance, risk_errors = load_risk_acceptance()
coverable_errors = dependency_errors + image_errors + script_errors
accepted_findings = set(risk_acceptance.get("accepted_findings") or [])
unaccepted_errors = [
    error
    for error in coverable_errors
    if error not in accepted_findings
]
unaccepted_errors = risk_errors + unaccepted_errors
accepted_error_count = len(coverable_errors) - (len(unaccepted_errors) - len(risk_errors))
security_scan_passed = not unaccepted_errors

payload = {
    "object": "tonglingyu.release_security_gate",
    "schema_version": 1,
    "status": "ok" if security_scan_passed else "failed",
    "security_scan_passed": security_scan_passed,
    "generated_at": now_iso(),
    "dependency_scan": dependency_scan,
    "image_scan": image_scan,
    "release_script_scan": release_script_scan,
    "scan_coverage": {
        "dependency_scan": dependency_scan["status"] != "missing",
        "image_scan": image_scan["status"] != "missing",
        "release_script_scan": True,
    },
    "risk_acceptance": risk_acceptance,
    "risk_conclusion": (
        "no_unaccepted_findings"
        if security_scan_passed
        else "unaccepted_findings_present"
    ),
    "accepted_error_count": accepted_error_count,
    "unaccepted_error_count": len(unaccepted_errors),
    "errors": unaccepted_errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path:
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
if not security_scan_passed:
    raise SystemExit(1)
PY
