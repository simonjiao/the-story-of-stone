#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

DEPENDENCY_SCAN_PATH="${TONGLINGYU_RELEASE_SECURITY_DEPENDENCY_SCAN_PATH:-}"
IMAGE_SCAN_PATH="${TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_PATH:-}"
RISK_ACCEPTANCE_PATH="${TONGLINGYU_RELEASE_SECURITY_RISK_ACCEPTANCE_PATH:-}"
RUN_TRIVY="${TONGLINGYU_RELEASE_SECURITY_RUN_TRIVY:-false}"
REPORT_PATH="${TONGLINGYU_RELEASE_SECURITY_REPORT_PATH:-}"
IMAGE_SCAN_ARTIFACT_ROOT="${TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/security-image-scans}"
IMAGE_SCAN_ARTIFACT_RUN_ID="${TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_RUN_ID:-$(date -u +"%Y%m%dT%H%M%SZ")-$$}"
IMAGE_SCAN_ARTIFACT_DIR="${TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_DIR:-${IMAGE_SCAN_ARTIFACT_ROOT}/${IMAGE_SCAN_ARTIFACT_RUN_ID}}"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

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
  mkdir -p "${IMAGE_SCAN_ARTIFACT_DIR}"
  python3 - "${REPO_DIR}" "${DEPLOY_DIR}" "${WORK_DIR}/images.txt" <<'PY'
import re
import sys
import os
from pathlib import Path

repo_dir = Path(sys.argv[1])
deploy_dir = Path(sys.argv[2])
target = Path(sys.argv[3])
compose = deploy_dir / "docker-compose.yml"
if not compose.is_file():
    compose = repo_dir / "deploy" / "docker-compose.yml"
images = []


def resolve_compose_var(token):
    name = token
    default = ""
    if ":-" in token:
        name, default = token.split(":-", 1)
    value = os.environ.get(name)
    if value is None or value == "":
        value = default
    return resolve_compose_image_ref(value)


def resolve_compose_image_ref(raw_ref):
    value = str(raw_ref or "").strip().strip('"').strip("'")
    pattern = re.compile(r"\$\{([^{}]+)\}")
    previous = None
    while previous != value:
        previous = value
        value = pattern.sub(lambda match: resolve_compose_var(match.group(1)), value)
    return value.strip()


for raw_line in compose.read_text(encoding="utf-8").splitlines():
    match = re.match(r"\s*image:\s+(.+?)\s*$", raw_line)
    if match:
        images.append(resolve_compose_image_ref(match.group(1)))
target.write_text("\n".join(images) + "\n", encoding="utf-8")
PY
  trivy_status="passed"
  failed_image_count=0
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
      >"${IMAGE_SCAN_ARTIFACT_DIR}/trivy-${image_hash}.json" \
      2>"${WORK_DIR}/trivy-${image_hash}.stderr"; then
      trivy_status="failed"
      failed_image_count=$((failed_image_count + 1))
    fi
  done <"${WORK_DIR}/images.txt"
  python3 - "${IMAGE_SCAN_REPORT}" "${trivy_status}" "${failed_image_count}" \
    "${WORK_DIR}/images.txt" "${IMAGE_SCAN_ARTIFACT_DIR}" \
    "${IMAGE_SCAN_ARTIFACT_RUN_ID}" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

(
    target,
    status,
    failed_image_count,
    image_refs_path,
    artifact_dir_raw,
    scan_run_id,
) = sys.argv[1:7]
image_refs_raw = Path(image_refs_path).read_text(encoding="utf-8")
image_refs = [line for line in image_refs_raw.splitlines() if line.strip()]
artifact_dir = Path(artifact_dir_raw).resolve()
critical_count = 0
high_count = 0
failed_image_count = int(failed_image_count)
report_digests = []
raw_report_paths = []
for image_ref in image_refs:
    image_hash = hashlib.sha256(image_ref.encode("utf-8")).hexdigest()
    report_path = artifact_dir / f"trivy-{image_hash}.json"
    raw_report_paths.append(str(report_path))
    if not report_path.is_file():
        status = "failed"
        failed_image_count += 1
        continue
    raw_report = report_path.read_bytes()
    report_digests.append(hashlib.sha256(raw_report).hexdigest())
    try:
        report = json.loads(raw_report.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        status = "failed"
        failed_image_count += 1
        continue
    for result in report.get("Results") or []:
        for vulnerability in result.get("Vulnerabilities") or []:
            severity = str(vulnerability.get("Severity") or "").upper()
            if severity == "CRITICAL":
                critical_count += 1
            elif severity == "HIGH":
                high_count += 1
if critical_count > 0 or high_count > 0 or failed_image_count > 0:
    status = "failed"
Path(target).write_text(
    json.dumps(
        {
            "object": "tonglingyu.security_scan_result",
            "scan_type": "image",
            "status": status,
            "scanner": "trivy",
            "critical_count": critical_count,
            "high_count": high_count,
            "failed_image_count": failed_image_count,
            "scanned_image_count": len(image_refs),
            "scanned_image_refs_sha256": hashlib.sha256(
                image_refs_raw.encode("utf-8")
            ).hexdigest(),
            "scanned_report_count": len(report_digests),
            "scanned_reports_sha256": hashlib.sha256(
                ("\n".join(sorted(report_digests)) + "\n").encode("utf-8")
            ).hexdigest(),
            "raw_reports_persistent": True,
            "raw_report_artifact_dir": str(artifact_dir),
            "raw_report_paths": raw_report_paths,
            "raw_report_paths_sha256": hashlib.sha256(
                ("\n".join(raw_report_paths) + "\n").encode("utf-8")
            ).hexdigest(),
            "scan_run_id": scan_run_id,
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

python3 - "${REPO_DIR}" "${DEPLOY_DIR}" "${DEPENDENCY_SCAN_REPORT}" \
  "${IMAGE_SCAN_REPORT}" "${RISK_ACCEPTANCE_PATH}" "${REPORT_PATH}" <<'PY'
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
    deploy_dir_raw,
    dependency_scan_path_raw,
    image_scan_path_raw,
    risk_acceptance_path_raw,
    report_path_raw,
) = sys.argv[1:7]

repo_dir = Path(repo_dir_raw)
deploy_dir = Path(deploy_dir_raw)
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
    if scan_type == "image":
        result["failed_image_count"] = value.get("failed_image_count")
        result["scanned_image_count"] = value.get("scanned_image_count")
        result["scanned_image_refs_sha256"] = str(
            value.get("scanned_image_refs_sha256") or ""
        )
        result["scanned_report_count"] = value.get("scanned_report_count")
        result["scanned_reports_sha256"] = str(
            value.get("scanned_reports_sha256") or ""
        )
        result["raw_reports_persistent"] = value.get("raw_reports_persistent")
        result["raw_report_artifact_dir"] = str(
            value.get("raw_report_artifact_dir") or ""
        )
        result["raw_report_paths"] = value.get("raw_report_paths")
        result["raw_report_paths_sha256"] = str(
            value.get("raw_report_paths_sha256") or ""
        )
        result["scan_run_id"] = str(value.get("scan_run_id") or "")
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
    try:
        output = subprocess.check_output(
            command,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        return [repo_dir / line for line in output.splitlines() if line.strip()]
    except (OSError, subprocess.CalledProcessError):
        files = []
        for raw in paths:
            candidates = [repo_dir / raw]
            if raw.startswith("deploy/"):
                candidates.append(deploy_dir / raw.removeprefix("deploy/"))
            for candidate in candidates:
                if candidate.is_file():
                    files.append(candidate)
                    break
                if candidate.is_dir():
                    files.extend(
                        path
                        for path in sorted(candidate.rglob("*"))
                        if path.is_file()
                        and ".git" not in path.parts
                        and "target" not in path.parts
                    )
                    break
        unique = []
        seen = set()
        for path in files:
            resolved = path.resolve()
            if resolved not in seen:
                seen.add(resolved)
                unique.append(path)
        return unique


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


def resolve_compose_var(token):
    name = token
    default = ""
    if ":-" in token:
        name, default = token.split(":-", 1)
    value = os.environ.get(name)
    if value is None or value == "":
        value = default
    return resolve_compose_image_ref(value)


def resolve_compose_image_ref(raw_ref):
    value = str(raw_ref or "").strip().strip('"').strip("'")
    pattern = re.compile(r"\$\{([^{}]+)\}")
    previous = None
    while previous != value:
        previous = value
        value = pattern.sub(lambda match: resolve_compose_var(match.group(1)), value)
    return value.strip()


def compose_image_policy():
    compose_path = deploy_dir / "docker-compose.yml"
    if not compose_path.is_file():
        compose_path = repo_dir / "deploy" / "docker-compose.yml"
    text = compose_path.read_text(encoding="utf-8")
    image_items = []
    for raw_line in text.splitlines():
        match = re.match(r"\s*image:\s+(.+?)\s*$", raw_line)
        if match:
            raw_ref = match.group(1)
            resolved_ref = resolve_compose_image_ref(raw_ref)
            image_items.append(
                {
                    "ref": resolved_ref,
                    "owner_type": classify_image_owner(raw_ref, resolved_ref),
                }
            )
    refs = [item["ref"] for item in image_items]
    owned_refs = [item["ref"] for item in image_items if item["owner_type"] == "owned"]
    third_party_refs = [
        item["ref"] for item in image_items if item["owner_type"] == "third_party"
    ]
    encoded_refs = ("\n".join(refs) + "\n") if refs else ""
    mutable = []
    digest_missing = []
    owned_digest_missing = []
    third_party_digest_missing = []
    for item in image_items:
        ref = item["ref"]
        if not image_ref_is_immutable(ref):
            digest_missing.append(ref)
            if item["owner_type"] == "owned":
                owned_digest_missing.append(ref)
            else:
                third_party_digest_missing.append(ref)
        if re.search(r"(:|\:-)(latest|main)([}:]|$)", ref):
            mutable.append(ref)
    return {
        "image_count": len(refs),
        "image_refs": refs,
        "image_refs_sha256": hashlib.sha256(
            encoded_refs.encode("utf-8")
        ).hexdigest(),
        "image_ownership": image_items,
        "owned_image_count": len(owned_refs),
        "owned_image_refs_sha256": hashlib.sha256(
            (("\n".join(owned_refs) + "\n") if owned_refs else "").encode("utf-8")
        ).hexdigest(),
        "third_party_image_count": len(third_party_refs),
        "third_party_image_refs_sha256": hashlib.sha256(
            (
                ("\n".join(third_party_refs) + "\n") if third_party_refs else ""
            ).encode("utf-8")
        ).hexdigest(),
        "mutable_tag_count": len(mutable),
        "digest_missing_count": len(third_party_digest_missing),
        "owned_digest_missing_count": len(owned_digest_missing),
        "third_party_digest_missing_count": len(third_party_digest_missing),
    }


def classify_image_owner(raw_ref, resolved_ref):
    owned_env_vars = {
        "TONGLINGYU_GATEWAY_IMAGE_REF",
    }
    raw = str(raw_ref or "")
    resolved = str(resolved_ref or "")
    if any(name in raw for name in owned_env_vars):
        return "owned"
    if re.search(r"(^|/)(tonglingyu-gateway)(:|@|$)", resolved):
        return "owned"
    return "third_party"


def image_ref_is_immutable(ref):
    return "@sha256:" in ref or re.fullmatch(r"sha256:[0-9a-f]{64}", ref) is not None


def count_trivy_high_critical(report_path):
    report = load_json_file(report_path)
    critical_count = 0
    high_count = 0
    for result in report.get("Results") or []:
        for vulnerability in result.get("Vulnerabilities") or []:
            severity = str(vulnerability.get("Severity") or "").upper()
            if severity == "CRITICAL":
                critical_count += 1
            elif severity == "HIGH":
                high_count += 1
    return critical_count, high_count


def apply_image_blocking_policy(image_scan, image_policy):
    policy_errors = []
    nonblocking_errors = []
    ownership = image_policy.get("image_ownership")
    raw_report_paths = image_scan.get("raw_report_paths")
    if not isinstance(ownership, list) or not isinstance(raw_report_paths, list):
        image_scan.update(
            {
                "image_policy_version": "tonglingyu-image-ownership-v1",
                "blocking_critical_count": None,
                "blocking_high_count": None,
                "owned_critical_count": None,
                "owned_high_count": None,
                "third_party_critical_count": None,
                "third_party_high_count": None,
                "third_party_findings_non_blocking": True,
            }
        )
        return ["image_ownership_classification_missing"], nonblocking_errors
    if len(ownership) != len(raw_report_paths):
        policy_errors.append("image_ownership_report_count_mismatch")

    owned_critical_count = 0
    owned_high_count = 0
    third_party_critical_count = 0
    third_party_high_count = 0
    total_critical_count = 0
    total_high_count = 0
    scan_failure_count = 0
    findings = []

    for item, raw_report_path in zip(ownership, raw_report_paths):
        if not isinstance(item, dict):
            policy_errors.append("image_ownership_entry_invalid")
            continue
        owner_type = item.get("owner_type")
        image_ref = str(item.get("ref") or "")
        if owner_type not in {"owned", "third_party"}:
            policy_errors.append("image_owner_type_invalid")
            owner_type = "third_party"
        candidate = Path(str(raw_report_path or ""))
        if not candidate.is_file():
            policy_errors.append("image_ownership_raw_report_missing")
            scan_failure_count += 1
            continue
        try:
            critical_count, high_count = count_trivy_high_critical(candidate)
        except (OSError, json.JSONDecodeError):
            policy_errors.append("image_ownership_raw_report_invalid")
            scan_failure_count += 1
            continue
        total_critical_count += critical_count
        total_high_count += high_count
        if owner_type == "owned":
            owned_critical_count += critical_count
            owned_high_count += high_count
        else:
            third_party_critical_count += critical_count
            third_party_high_count += high_count
        findings.append(
            {
                "image_ref_sha256": hashlib.sha256(image_ref.encode("utf-8")).hexdigest(),
                "owner_type": owner_type,
                "critical_count": critical_count,
                "high_count": high_count,
            }
        )

    if owned_critical_count > 0:
        policy_errors.append("image_owned_critical_findings_present")
    if owned_high_count > 0:
        policy_errors.append("image_owned_high_findings_present")
    if third_party_critical_count > 0:
        nonblocking_errors.append("third_party_image_critical_findings_present")
    if third_party_high_count > 0:
        nonblocking_errors.append("third_party_image_high_findings_present")

    scanner_failed_image_count = image_scan.get("failed_image_count")
    if not isinstance(scanner_failed_image_count, int):
        scanner_failed_image_count = 0
    blocking_failed = bool(policy_errors)
    image_scan.update(
        {
            "status": "failed" if blocking_failed else "passed",
            "critical_count": total_critical_count,
            "high_count": total_high_count,
            "failed_image_count": scan_failure_count,
            "scanner_failed_image_count": scanner_failed_image_count,
            "image_policy_version": "tonglingyu-image-ownership-v1",
            "blocking_critical_count": owned_critical_count,
            "blocking_high_count": owned_high_count,
            "owned_critical_count": owned_critical_count,
            "owned_high_count": owned_high_count,
            "third_party_critical_count": third_party_critical_count,
            "third_party_high_count": third_party_high_count,
            "third_party_findings_non_blocking": True,
            "image_finding_summary": findings,
        }
    )
    return policy_errors, nonblocking_errors


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
image_policy_errors, image_nonblocking_errors = apply_image_blocking_policy(
    image_scan,
    image_policy,
)
image_errors.extend(image_policy_errors)
if image_scan["status"] == "passed":
    image_errors = [
        error
        for error in image_errors
        if error
        not in {
            "image_scan_not_passed",
            "image_critical_findings_present",
            "image_high_findings_present",
        }
    ]
if image_scan["status"] == "passed":
    failed_image_count = image_scan.get("failed_image_count")
    if not isinstance(failed_image_count, int) or failed_image_count < 0:
        image_errors.append("image_scan_failed_image_count_invalid")
    elif failed_image_count > 0:
        image_errors.append("image_scan_failed_images_present")
    scanned_image_count = image_scan.get("scanned_image_count")
    if not isinstance(scanned_image_count, int) or scanned_image_count < 0:
        image_errors.append("image_scan_scanned_image_count_invalid")
    elif scanned_image_count != image_policy["image_count"]:
        image_errors.append("image_scan_image_count_mismatch")
    scanned_image_refs_sha256 = image_scan.get("scanned_image_refs_sha256")
    if not isinstance(scanned_image_refs_sha256, str) or not re.fullmatch(
        r"[0-9a-f]{64}", scanned_image_refs_sha256
    ):
        image_errors.append("image_scan_inventory_digest_invalid")
    elif scanned_image_refs_sha256 != image_policy["image_refs_sha256"]:
        image_errors.append("image_scan_inventory_mismatch")
    scanned_report_count = image_scan.get("scanned_report_count")
    if not isinstance(scanned_report_count, int) or scanned_report_count < 0:
        image_errors.append("image_scan_report_count_invalid")
    elif scanned_report_count != image_policy["image_count"]:
        image_errors.append("image_scan_report_count_mismatch")
    scanned_reports_sha256 = image_scan.get("scanned_reports_sha256")
    if not isinstance(scanned_reports_sha256, str) or not re.fullmatch(
        r"[0-9a-f]{64}", scanned_reports_sha256
    ):
        image_errors.append("image_scan_reports_digest_invalid")
    raw_report_artifact_dir = image_scan.get("raw_report_artifact_dir")
    if image_scan.get("raw_reports_persistent") is not True:
        image_errors.append("image_scan_raw_reports_not_persistent")
    if not isinstance(raw_report_artifact_dir, str) or not raw_report_artifact_dir:
        image_errors.append("image_scan_raw_report_artifact_dir_missing")
    else:
        raw_report_artifact_path = Path(raw_report_artifact_dir)
        if not raw_report_artifact_path.is_absolute():
            image_errors.append("image_scan_raw_report_artifact_dir_not_absolute")
        elif not raw_report_artifact_path.is_dir():
            image_errors.append("image_scan_raw_report_artifact_dir_missing")
    raw_report_paths = image_scan.get("raw_report_paths")
    raw_report_paths_sha256 = image_scan.get("raw_report_paths_sha256")
    if not isinstance(raw_report_paths, list) or not all(
        isinstance(item, str) and item for item in raw_report_paths
    ):
        image_errors.append("image_scan_raw_report_paths_invalid")
        raw_report_paths = []
    elif (
        isinstance(scanned_report_count, int)
        and scanned_report_count >= 0
        and len(raw_report_paths) != scanned_report_count
    ):
        image_errors.append("image_scan_raw_report_paths_count_mismatch")
    if not isinstance(raw_report_paths_sha256, str) or not re.fullmatch(
        r"[0-9a-f]{64}", raw_report_paths_sha256
    ):
        image_errors.append("image_scan_raw_report_paths_digest_invalid")
    elif raw_report_paths_sha256 != hashlib.sha256(
        ("\n".join(raw_report_paths) + "\n").encode("utf-8")
    ).hexdigest():
        image_errors.append("image_scan_raw_report_paths_digest_mismatch")
    raw_report_digests = []
    for raw_report_path in raw_report_paths:
        candidate = Path(raw_report_path)
        if not candidate.is_absolute():
            image_errors.append("image_scan_raw_report_path_not_absolute")
            continue
        if not candidate.is_file():
            image_errors.append("image_scan_raw_report_path_missing")
            continue
        raw_report_digests.append(file_sha256(candidate))
    if (
        raw_report_paths
        and len(raw_report_digests) == len(raw_report_paths)
        and isinstance(scanned_reports_sha256, str)
        and re.fullmatch(r"[0-9a-f]{64}", scanned_reports_sha256)
        and scanned_reports_sha256
        != hashlib.sha256(
            ("\n".join(sorted(raw_report_digests)) + "\n").encode("utf-8")
        ).hexdigest()
    ):
        image_errors.append("image_scan_raw_reports_digest_mismatch")

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
    "nonblocking_error_count": len(image_nonblocking_errors),
    "nonblocking_errors": image_nonblocking_errors,
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
