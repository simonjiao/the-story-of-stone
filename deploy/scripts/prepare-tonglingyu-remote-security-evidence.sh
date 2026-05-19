#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"

REMOTE_HOST="${TONGLINGYU_REMOTE_RELEASE_HOST:-hhost}"
REMOTE_PROJECT_DIR="${TONGLINGYU_REMOTE_RELEASE_PROJECT_DIR:-}"
REMOTE_PROJECT_ARG="${REMOTE_PROJECT_DIR:-__DEFAULT_REMOTE_PROJECT_DIR__}"
REMOTE_ARTIFACT_DIR="${TONGLINGYU_REMOTE_SECURITY_REMOTE_ARTIFACT_DIR:?set TONGLINGYU_REMOTE_SECURITY_REMOTE_ARTIFACT_DIR}"
RUN_ID="${TONGLINGYU_REMOTE_SECURITY_RUN_ID:-security-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
ARTIFACT_ROOT="${TONGLINGYU_REMOTE_SECURITY_ARTIFACT_ROOT:-${REPO_DIR}/data/tonglingyu/remote-security-evidence}"
if [[ "${ARTIFACT_ROOT}" != /* ]]; then
  ARTIFACT_ROOT="${REPO_DIR}/${ARTIFACT_ROOT}"
fi
ARTIFACT_DIR="${TONGLINGYU_REMOTE_SECURITY_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
if [[ "${ARTIFACT_DIR}" != /* ]]; then
  ARTIFACT_DIR="${REPO_DIR}/${ARTIFACT_DIR}"
fi
REPORT_PATH="${TONGLINGYU_REMOTE_SECURITY_REPORT_PATH:-${ARTIFACT_DIR}/remote-security-evidence.json}"

DEPENDENCY_SCAN_LOCAL="${ARTIFACT_DIR}/dependency-scan.json"
DEPENDENCY_SCAN_STDERR="${ARTIFACT_DIR}/cargo-audit.stderr"
IMAGE_REFS_LOCAL="${ARTIFACT_DIR}/image-refs.txt"
IMAGE_REPORT_DIR_LOCAL="${ARTIFACT_DIR}/image-reports"
IMAGE_SCAN_LOCAL="${ARTIFACT_DIR}/image-scan.json"
TRIVY_STDERR_DIR="${ARTIFACT_DIR}/trivy-stderr"
TRIVY_TAR_DIR="${ARTIFACT_DIR}/image-tars"
DOCKER_CONFIG_DIR="${ARTIFACT_DIR}/docker-config"

REMOTE_SECURITY_DIR="${REMOTE_ARTIFACT_DIR%/}/security-evidence"
REMOTE_DEPENDENCY_SCAN="${REMOTE_SECURITY_DIR}/dependency-scan.json"
REMOTE_IMAGE_SCAN="${REMOTE_SECURITY_DIR}/image-scan.json"
REMOTE_IMAGE_REPORT_DIR="${REMOTE_SECURITY_DIR}/image-reports"

mkdir -p "${ARTIFACT_DIR}" "${IMAGE_REPORT_DIR_LOCAL}" "${TRIVY_STDERR_DIR}" \
  "${TRIVY_TAR_DIR}" "${DOCKER_CONFIG_DIR}"

REMOTE_PROJECT_RESOLVED="$(
  ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
    'sh -s' -- "${REMOTE_PROJECT_ARG}" <<'REMOTE'
set -eu
project_dir_arg="$1"
if [ "${project_dir_arg}" != "__DEFAULT_REMOTE_PROJECT_DIR__" ]; then
  project_dir="${project_dir_arg}"
else
  project_dir="${DEPLOY_NODE_PROJECT_DIR:-$HOME/tonglingyu-home-deploy}"
fi
printf '%s\n' "${project_dir}"
REMOTE
)"

dependency_scan_status="missing"
if command -v cargo-audit >/dev/null 2>&1; then
  dependency_scan_status="passed"
  if ! (
    cd "${REPO_DIR}/agent-platform"
    cargo audit --json >"${DEPENDENCY_SCAN_LOCAL}" 2>"${DEPENDENCY_SCAN_STDERR}"
  ); then
    dependency_scan_status="failed"
    if [[ ! -s "${DEPENDENCY_SCAN_LOCAL}" ]]; then
      printf '{"object":"tonglingyu.security_scan_result","scan_type":"dependency","status":"failed","scanner":"cargo-audit","critical_count":1,"high_count":0,"secret_values_printed":false}\n' \
        >"${DEPENDENCY_SCAN_LOCAL}"
    fi
  fi
fi

ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  'sh -s' -- "${REMOTE_PROJECT_RESOLVED}" >"${IMAGE_REFS_LOCAL}" <<'REMOTE'
set -eu
project_dir="$1"
python3 - "${project_dir}" <<'PY'
import os
import re
import sys
from pathlib import Path

project_dir = Path(sys.argv[1])
env_path = project_dir / ".env"
compose_path = project_dir / "docker-compose.yml"
env = {}
if env_path.is_file():
    for raw_line in env_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        value = value.strip().strip('"').strip("'")
        env[key.strip()] = value


def resolve_compose_var(token):
    name = token
    default = ""
    if ":-" in token:
        name, default = token.split(":-", 1)
    value = env.get(name) or os.environ.get(name) or default
    return resolve_compose_image_ref(value)


def resolve_compose_image_ref(raw_ref):
    value = str(raw_ref or "").strip().strip('"').strip("'")
    pattern = re.compile(r"\$\{([^{}]+)\}")
    previous = None
    while previous != value:
        previous = value
        value = pattern.sub(lambda match: resolve_compose_var(match.group(1)), value)
    return value.strip()


for raw_line in compose_path.read_text(encoding="utf-8").splitlines():
    match = re.match(r"\s*image:\s+(.+?)\s*$", raw_line)
    if match:
        print(resolve_compose_image_ref(match.group(1)))
PY
REMOTE

trivy_status="missing"
failed_image_count=0
if command -v trivy >/dev/null 2>&1; then
  trivy_status="passed"
  while IFS= read -r image_ref; do
    [[ -n "${image_ref}" ]] || continue
    image_hash="$(
      python3 - "${image_ref}" <<'PY'
import hashlib
import sys

print(hashlib.sha256(sys.argv[1].encode("utf-8")).hexdigest())
PY
    )"
    raw_report="${IMAGE_REPORT_DIR_LOCAL}/trivy-${image_hash}.json"
    stderr_path="${TRIVY_STDERR_DIR}/trivy-${image_hash}.stderr"
    if [[ "${image_ref}" =~ ^sha256:[0-9a-f]{64}$ || "${image_ref}" =~ (^|/)tonglingyu-gateway(:|@|$) ]]; then
      image_tar="${TRIVY_TAR_DIR}/image-${image_hash}.tar"
      if ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
        'sh -s' -- "${image_ref}" >"${image_tar}" 2>"${stderr_path}.docker-save" <<'REMOTE'
set -eu
docker save "$1"
REMOTE
      then
        if ! DOCKER_CONFIG="${DOCKER_CONFIG_DIR}" trivy image --quiet --format json \
          --input "${image_tar}" >"${raw_report}" 2>"${stderr_path}"; then
          trivy_status="failed"
          failed_image_count=$((failed_image_count + 1))
        fi
      else
        trivy_status="failed"
        failed_image_count=$((failed_image_count + 1))
      fi
    elif ! DOCKER_CONFIG="${DOCKER_CONFIG_DIR}" trivy image --quiet --format json \
      "${image_ref}" >"${raw_report}" 2>"${stderr_path}"; then
      trivy_status="failed"
      failed_image_count=$((failed_image_count + 1))
    fi
  done <"${IMAGE_REFS_LOCAL}"
fi

python3 - "${IMAGE_SCAN_LOCAL}" "${trivy_status}" "${failed_image_count}" \
  "${IMAGE_REFS_LOCAL}" "${IMAGE_REPORT_DIR_LOCAL}" "${REMOTE_IMAGE_REPORT_DIR}" \
  "${RUN_ID}" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

(
    target_raw,
    status,
    failed_image_count_raw,
    image_refs_path_raw,
    local_report_dir_raw,
    remote_report_dir_raw,
    scan_run_id,
) = sys.argv[1:8]

image_refs_raw = Path(image_refs_path_raw).read_text(encoding="utf-8")
image_refs = [line for line in image_refs_raw.splitlines() if line.strip()]
local_report_dir = Path(local_report_dir_raw)
remote_report_dir = remote_report_dir_raw.rstrip("/")
failed_image_count = int(failed_image_count_raw)
critical_count = 0
high_count = 0
report_digests = []
raw_report_paths = []
for image_ref in image_refs:
    image_hash = hashlib.sha256(image_ref.encode("utf-8")).hexdigest()
    local_report = local_report_dir / f"trivy-{image_hash}.json"
    raw_report_paths.append(f"{remote_report_dir}/trivy-{image_hash}.json")
    if not local_report.is_file():
        status = "failed"
        failed_image_count += 1
        continue
    raw_report = local_report.read_bytes()
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
Path(target_raw).write_text(
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
            "raw_report_artifact_dir": remote_report_dir,
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

ssh -o BatchMode=yes -o ConnectTimeout=10 "${REMOTE_HOST}" \
  "mkdir -p '${REMOTE_SECURITY_DIR}' '${REMOTE_IMAGE_REPORT_DIR}'"
if [[ -f "${DEPENDENCY_SCAN_LOCAL}" ]]; then
  rsync -a "${DEPENDENCY_SCAN_LOCAL}" "${REMOTE_HOST}:${REMOTE_DEPENDENCY_SCAN}"
fi
rsync -a "${IMAGE_SCAN_LOCAL}" "${REMOTE_HOST}:${REMOTE_IMAGE_SCAN}"
rsync -a "${IMAGE_REPORT_DIR_LOCAL}/" "${REMOTE_HOST}:${REMOTE_IMAGE_REPORT_DIR}/"

python3 - "${REPORT_PATH}" "${RUN_ID}" "${REMOTE_HOST}" "${REMOTE_PROJECT_RESOLVED}" \
  "${REMOTE_SECURITY_DIR}" "${DEPENDENCY_SCAN_LOCAL}" "${DEPENDENCY_SCAN_STDERR}" \
  "${IMAGE_REFS_LOCAL}" "${IMAGE_SCAN_LOCAL}" "${IMAGE_REPORT_DIR_LOCAL}" \
  "${REMOTE_DEPENDENCY_SCAN}" "${REMOTE_IMAGE_SCAN}" "${REMOTE_IMAGE_REPORT_DIR}" \
  "${dependency_scan_status}" "${trivy_status}" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    report_path_raw,
    run_id,
    remote_host,
    remote_project_dir,
    remote_security_dir,
    dependency_scan_local_raw,
    dependency_stderr_raw,
    image_refs_local_raw,
    image_scan_local_raw,
    image_report_dir_local_raw,
    remote_dependency_scan,
    remote_image_scan,
    remote_image_report_dir,
    dependency_scan_status,
    trivy_status,
) = sys.argv[1:16]


def file_sha256(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path_raw):
    path = Path(path_raw)
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


image_scan = load_json(image_scan_local_raw) or {}
image_refs = Path(image_refs_local_raw).read_text(encoding="utf-8").splitlines()
report_dir = Path(image_report_dir_local_raw)
raw_report_count = len([path for path in report_dir.glob("trivy-*.json") if path.is_file()])
payload = {
    "object": "tonglingyu.remote_security_evidence",
    "schema_version": 1,
    "status": "ok" if dependency_scan_status != "missing" and trivy_status != "missing" else "failed",
    "run_id": run_id,
    "remote_host": remote_host,
    "remote_project_dir": remote_project_dir,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "dependency_scan": {
        "status": dependency_scan_status,
        "local_path": dependency_scan_local_raw if Path(dependency_scan_local_raw).is_file() else "",
        "local_sha256": file_sha256(dependency_scan_local_raw),
        "remote_path": remote_dependency_scan if Path(dependency_scan_local_raw).is_file() else "",
        "stderr_path": dependency_stderr_raw if Path(dependency_stderr_raw).is_file() else "",
    },
    "image_scan": {
        "status": image_scan.get("status") or trivy_status,
        "local_path": image_scan_local_raw,
        "local_sha256": file_sha256(image_scan_local_raw),
        "remote_path": remote_image_scan,
        "remote_report_dir": remote_image_report_dir,
        "image_ref_count": len([line for line in image_refs if line.strip()]),
        "raw_report_count": raw_report_count,
        "critical_count": image_scan.get("critical_count"),
        "high_count": image_scan.get("high_count"),
        "failed_image_count": image_scan.get("failed_image_count"),
    },
    "remote_security_dir": remote_security_dir,
    "secret_values_printed": False,
}
Path(report_path_raw).write_text(
    json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
print(json.dumps(payload, ensure_ascii=True, sort_keys=True))
raise SystemExit(0 if payload["status"] == "ok" else 1)
PY
