#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  cat <<'EOF'
usage: record-openwebui-browser-review-evidence.sh [output-json]

Records a passed Open WebUI browser-side release review after the human review
has completed. Required environment:

  TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF
  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEWER
  TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL or PUBLIC_WEBUI_URL or OPEN_WEBUI_BASE_URL
  TONGLINGYU_BROWSER_REVIEW_ORDINARY_USER_MODEL_VISIBILITY_REF
  TONGLINGYU_BROWSER_REVIEW_STREAMING_CHAT_UX_REF
  TONGLINGYU_BROWSER_REVIEW_ADMIN_AUDIT_VISIBILITY_REF
  TONGLINGYU_BROWSER_REVIEW_PERSISTED_PROVIDER_SETTINGS_REF
  TONGLINGYU_RELEASE_OPENWEBUI_PROVIDER_SETTINGS_MATCHED=true

Optional:

  TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE
  TONGLINGYU_BROWSER_REVIEW_REVIEWED_AT
  TONGLINGYU_BROWSER_REVIEW_EVIDENCE_ROOT
  TONGLINGYU_BROWSER_REVIEW_EVIDENCE_OVERWRITE=true

Local screenshot/file evidence refs must exist under the evidence JSON
directory, or under TONGLINGYU_BROWSER_REVIEW_EVIDENCE_ROOT when it is set.
EOF
  exit 0
fi

OUTPUT_PATH="${1:-${TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE:-openwebui-browser-review.json}}"

python3 - "${OUTPUT_PATH}" <<'PY'
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

output_path = Path(sys.argv[1])


def is_true(value) -> bool:
    return str(value or "").strip().lower() in {"1", "true", "yes", "on"}


def env_first(*names: str) -> str:
    for name in names:
        value = os.environ.get(name, "").strip()
        if value:
            return value
    return ""


def required(name: str, errors: list[str]) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        errors.append(f"missing_{name}")
    return value


errors: list[str] = []
if not is_true(os.environ.get("TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW")):
    errors.append("browser_review_ack_must_be_true")

review_ref = required("TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF", errors)
reviewer = required("TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEWER", errors)
public_url = env_first(
    "TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL",
    "PUBLIC_WEBUI_URL",
    "OPEN_WEBUI_BASE_URL",
)
if not public_url:
    errors.append("missing_TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL")

ordinary_ref = required(
    "TONGLINGYU_BROWSER_REVIEW_ORDINARY_USER_MODEL_VISIBILITY_REF",
    errors,
)
streaming_ref = required("TONGLINGYU_BROWSER_REVIEW_STREAMING_CHAT_UX_REF", errors)
admin_ref = required("TONGLINGYU_BROWSER_REVIEW_ADMIN_AUDIT_VISIBILITY_REF", errors)
provider_ref = required(
    "TONGLINGYU_BROWSER_REVIEW_PERSISTED_PROVIDER_SETTINGS_REF",
    errors,
)
if not is_true(os.environ.get("TONGLINGYU_RELEASE_OPENWEBUI_PROVIDER_SETTINGS_MATCHED")):
    errors.append("provider_settings_matched_must_be_true")

reviewed_at = os.environ.get("TONGLINGYU_BROWSER_REVIEW_REVIEWED_AT", "").strip()
if not reviewed_at:
    reviewed_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    reviewed_at = reviewed_at.replace("+00:00", "Z")

if output_path.exists() and not is_true(
    os.environ.get("TONGLINGYU_BROWSER_REVIEW_EVIDENCE_OVERWRITE")
):
    errors.append("output_path_exists_set_TONGLINGYU_BROWSER_REVIEW_EVIDENCE_OVERWRITE")

if errors:
    print(
        json.dumps(
            {
                "object": "tonglingyu.openwebui_browser_review_record",
                "status": "failed",
                "evidence_path": str(output_path),
                "errors": errors,
                "secret_values_printed": False,
            },
            ensure_ascii=True,
            sort_keys=True,
        )
    )
    raise SystemExit(1)

evidence = {
    "object": "tonglingyu.openwebui_browser_review",
    "status": "passed",
    "review_ref": review_ref,
    "reviewed_at": reviewed_at,
    "reviewer": reviewer,
    "public_webui_url": public_url,
    "checks": {
        "ordinary_user_model_visibility": {
            "status": "passed",
            "evidence_ref": ordinary_ref,
        },
        "streaming_chat_ux": {
            "status": "passed",
            "evidence_ref": streaming_ref,
        },
        "admin_audit_visibility": {
            "status": "passed",
            "evidence_ref": admin_ref,
        },
        "persisted_provider_settings": {
            "status": "passed",
            "evidence_ref": provider_ref,
            "matched_rendered_env": True,
        },
    },
}

output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(
    json.dumps(evidence, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY

"${SCRIPT_DIR}/verify-openwebui-browser-review-evidence.sh" "${OUTPUT_PATH}"
