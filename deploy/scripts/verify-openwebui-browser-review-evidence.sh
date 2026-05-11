#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

EVIDENCE_PATH="${1:-${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE:-}}"
EXPECTED_REVIEW_REF="${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF:-}"

python3 - "${EVIDENCE_PATH}" "${EXPECTED_REVIEW_REF}" <<'PY'
import json
import os
import re
import sys
from datetime import datetime
from pathlib import Path
from urllib.parse import urlparse

evidence_path = sys.argv[1].strip()
expected_review_ref = sys.argv[2].strip()
evidence_root = os.environ.get("TONGLINGYU_BROWSER_REVIEW_EVIDENCE_ROOT", "").strip()
required_checks = [
    "ordinary_user_model_visibility",
    "streaming_chat_ux",
    "admin_audit_visibility",
    "persisted_provider_settings",
]
secret_key_terms = [
    "api_key",
    "api_keys",
    "authorization",
    "bearer",
    "credential",
    "password",
    "secret",
    "token",
]
secret_value_needles = [
    "api-key=",
    "api_key=",
    "apikey=",
    "authorization:",
    "bearer ",
    "ghp_",
    "github_pat_",
    "password=",
    "secret=",
    "sk-",
    "token=",
    "xoxb-",
]
check_allowed_ref_kinds = {
    "ordinary_user_model_visibility": {"local_file", "url"},
    "streaming_chat_ux": {"local_file", "url"},
    "admin_audit_visibility": {"trace", "local_file", "url"},
    "persisted_provider_settings": {"runbook", "local_file", "url"},
}
trace_ref_re = re.compile(r"^trace:tly-[A-Za-z0-9_-]+$")
errors = []
evidence = {}


def report(status):
    print(
        json.dumps(
            {
                "object": "tonglingyu.openwebui_browser_review_gate",
                "status": status,
                "evidence_path": evidence_path,
                "review_ref": evidence.get("review_ref", "") if isinstance(evidence, dict) else "",
                "expected_review_ref_bound": bool(expected_review_ref),
                "checked_items": required_checks,
                "errors": errors,
                "secret_values_printed": False,
            },
            ensure_ascii=True,
            sort_keys=True,
        )
    )


def nonempty(value):
    return isinstance(value, str) and value.strip()


def has_timezone(value):
    if not isinstance(value, str):
        return False
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return False
    return parsed.tzinfo is not None and parsed.tzinfo.utcoffset(parsed) is not None


def secret_key_paths(value, prefix="$"):
    paths = []
    if isinstance(value, dict):
        for key, child in value.items():
            key_text = str(key)
            key_lower = key_text.lower()
            child_path = f"{prefix}.{key_text}"
            if any(term in key_lower for term in secret_key_terms):
                paths.append(child_path)
                continue
            paths.extend(secret_key_paths(child, child_path))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            paths.extend(secret_key_paths(child, f"{prefix}[{index}]"))
    return paths


def secret_value_paths(value, prefix="$"):
    paths = []
    if isinstance(value, dict):
        for key, child in value.items():
            paths.extend(secret_value_paths(child, f"{prefix}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            paths.extend(secret_value_paths(child, f"{prefix}[{index}]"))
    elif isinstance(value, str):
        lowered = value.lower()
        if any(needle in lowered for needle in secret_value_needles):
            paths.append(prefix)
    return paths


def safe_relative_path(value):
    candidate = Path(value)
    if candidate.is_absolute():
        return None
    if any(part in {"", ".", ".."} for part in candidate.parts):
        return None
    return candidate


def validate_local_ref(check_name, ref_path):
    relative = safe_relative_path(ref_path)
    if relative is None:
        errors.append(f"{check_name}_evidence_ref_file_path_must_be_relative")
        return "local_file"
    base = Path(evidence_root) if evidence_root else path.parent
    resolved = base / relative
    if not resolved.is_file():
        errors.append(f"{check_name}_evidence_ref_file_not_found")
    return "local_file"


def validate_evidence_ref(check_name, value):
    ref = value.strip()
    if len(ref) > 512:
        errors.append(f"{check_name}_evidence_ref_too_long")
        return "invalid"
    if any(char in ref for char in "\r\n\t"):
        errors.append(f"{check_name}_evidence_ref_contains_control_char")
        return "invalid"

    parsed = urlparse(ref)
    kind = "invalid"
    if parsed.scheme in {"http", "https"}:
        if parsed.scheme != "https" or not parsed.netloc:
            errors.append(f"{check_name}_evidence_ref_url_must_be_https")
        kind = "url"
    elif parsed.scheme == "trace":
        if not trace_ref_re.match(ref):
            errors.append(f"{check_name}_evidence_ref_trace_invalid")
        kind = "trace"
    elif parsed.scheme == "runbook":
        if not (parsed.netloc or parsed.path):
            errors.append(f"{check_name}_evidence_ref_runbook_empty")
        kind = "runbook"
    elif parsed.scheme == "file":
        if parsed.netloc:
            errors.append(f"{check_name}_evidence_ref_file_must_be_relative")
        kind = validate_local_ref(check_name, parsed.path)
    elif parsed.scheme:
        errors.append(f"{check_name}_evidence_ref_scheme_unsupported")
    else:
        kind = validate_local_ref(check_name, ref)

    allowed = check_allowed_ref_kinds[check_name]
    if kind not in allowed:
        errors.append(f"{check_name}_evidence_ref_kind_invalid")
    return kind


if not evidence_path:
    errors.append("evidence_path_missing")
    report("failed")
    raise SystemExit(1)

path = Path(evidence_path)
if not path.is_file():
    errors.append("evidence_path_not_found")
    report("failed")
    raise SystemExit(1)

try:
    evidence = json.loads(path.read_text(encoding="utf-8"))
except json.JSONDecodeError as exc:
    errors.append(f"evidence_json_invalid={exc.msg}")
    report("failed")
    raise SystemExit(1)

if not isinstance(evidence, dict):
    errors.append("evidence_must_be_object")
elif evidence.get("object") != "tonglingyu.openwebui_browser_review":
    errors.append("object_must_be_tonglingyu.openwebui_browser_review")

if evidence.get("status") != "passed":
    errors.append("status_must_be_passed")

review_ref = evidence.get("review_ref")
if not nonempty(review_ref):
    errors.append("review_ref_missing")
elif expected_review_ref and review_ref.strip() != expected_review_ref:
    errors.append("review_ref_mismatch")

reviewed_at = evidence.get("reviewed_at")
if not nonempty(reviewed_at):
    errors.append("reviewed_at_missing")
elif not has_timezone(reviewed_at):
    errors.append("reviewed_at_must_be_iso8601_with_timezone")
if not nonempty(evidence.get("reviewer")):
    errors.append("reviewer_missing")

public_webui_url = evidence.get("public_webui_url")
if not nonempty(public_webui_url):
    errors.append("public_webui_url_missing")
else:
    parsed_url = urlparse(public_webui_url.strip())
    if parsed_url.scheme != "https" or not parsed_url.netloc:
        errors.append("public_webui_url_must_be_https_url")

secret_paths = secret_key_paths(evidence)
if secret_paths:
    errors.append("secret_like_fields_present=" + ",".join(secret_paths[:8]))
secret_value_hits = secret_value_paths(evidence)
if secret_value_hits:
    errors.append("secret_like_values_present=" + ",".join(secret_value_hits[:8]))

checks = evidence.get("checks") or {}
if not isinstance(checks, dict):
    errors.append("checks_must_be_object")
    checks = {}

for check_name in required_checks:
    check = checks.get(check_name)
    if not isinstance(check, dict):
        errors.append(f"{check_name}_missing")
        continue
    if check.get("status") != "passed":
        errors.append(f"{check_name}_status_must_be_passed")
    evidence_ref = check.get("evidence_ref")
    if not nonempty(evidence_ref):
        errors.append(f"{check_name}_evidence_ref_missing")
    else:
        validate_evidence_ref(check_name, evidence_ref)

provider_check = checks.get("persisted_provider_settings")
if isinstance(provider_check, dict):
    matched = provider_check.get("matched_rendered_env")
    if matched is not True:
        errors.append("persisted_provider_settings_matched_rendered_env_must_be_true")

report("ok" if not errors else "failed")
if errors:
    raise SystemExit(1)
PY
