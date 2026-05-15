#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

HEALTH_JSON="${TONGLINGYU_GATEWAY_VERIFY_HEALTH_JSON:-${WORK_DIR}/health.json}"
MODELS_JSON="${TONGLINGYU_GATEWAY_VERIFY_MODELS_JSON:-${WORK_DIR}/models.json}"
METRICS_JSON="${TONGLINGYU_GATEWAY_VERIFY_METRICS_JSON:-${WORK_DIR}/metrics.json}"
PROMETHEUS_TXT="${TONGLINGYU_GATEWAY_VERIFY_PROMETHEUS_TXT:-${WORK_DIR}/metrics.prom}"
CHAT_JSON="${TONGLINGYU_GATEWAY_VERIFY_CHAT_JSON:-${WORK_DIR}/chat.json}"
STREAM_TXT="${TONGLINGYU_GATEWAY_VERIFY_STREAM_TXT:-${WORK_DIR}/chat-stream.txt}"
TRACE_JSON="${TONGLINGYU_GATEWAY_VERIFY_TRACE_JSON:-${WORK_DIR}/trace.json}"
STREAM_TRACE_JSON="${TONGLINGYU_GATEWAY_VERIFY_STREAM_TRACE_JSON:-${WORK_DIR}/stream-trace.json}"
RUNNING_IMAGES_JSON="${TONGLINGYU_GATEWAY_VERIFY_RUNNING_IMAGES_JSON:-${WORK_DIR}/running-images.json}"
VERIFY_RUN_ID="${TONGLINGYU_GATEWAY_VERIFY_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$}"

cd "${DEPLOY_DIR}"

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_HEALTH_JSON:-}" ]]; then
  docker compose exec -T tonglingyu-gateway \
    curl -fsS http://127.0.0.1:8090/healthz >"${HEALTH_JSON}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_MODELS_JSON:-}" ]]; then
  docker compose exec -T open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS -H "Authorization: Bearer ${key}" http://tonglingyu-gateway:8090/v1/models
' >"${MODELS_JSON}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_METRICS_JSON:-}" ]]; then
  docker compose exec -T tonglingyu-gateway sh -lc '
test -n "${TONGLINGYU_ADMIN_API_KEY}"
curl -fsS -H "Authorization: Bearer ${TONGLINGYU_ADMIN_API_KEY}" http://127.0.0.1:8090/v1/admin/metrics
' >"${METRICS_JSON}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_PROMETHEUS_TXT:-}" ]]; then
  docker compose exec -T tonglingyu-gateway sh -lc '
test -n "${TONGLINGYU_ADMIN_API_KEY}"
curl -fsS -H "Authorization: Bearer ${TONGLINGYU_ADMIN_API_KEY}" http://127.0.0.1:8090/v1/admin/metrics/prometheus
' >"${PROMETHEUS_TXT}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_CHAT_JSON:-}" ]]; then
  docker compose exec -T open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: release-gate" \
  -H "x-tonglingyu-chat-id: strict-gateway-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: strict-gateway-runtime-tool-smoke-${VERIFY_RUN_ID}" \
  --data "{\"model\":\"tonglingyu\",\"messages\":[{\"role\":\"user\",\"content\":\"通灵玉是什么？\"}]}" \
  http://tonglingyu-gateway:8090/v1/chat/completions
' >"${CHAT_JSON}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_STREAM_TXT:-}" ]]; then
  docker compose exec -T open-webui sh -lc '
key="${OPENAI_API_KEYS%%;*}"
test -n "${key}"
curl -fsS \
  -H "Authorization: Bearer ${key}" \
  -H "content-type: application/json" \
  -H "x-tonglingyu-user-id: release-gate" \
  -H "x-tonglingyu-chat-id: strict-gateway-${VERIFY_RUN_ID}" \
  -H "x-tonglingyu-message-id: strict-gateway-runtime-stream-smoke-${VERIFY_RUN_ID}" \
  --data "{\"model\":\"tonglingyu\",\"stream\":true,\"messages\":[{\"role\":\"user\",\"content\":\"通灵玉是什么？\"}]}" \
  http://tonglingyu-gateway:8090/v1/chat/completions
' >"${STREAM_TXT}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_TRACE_JSON:-}" ]]; then
  TRACE_ID="$(python3 - "${CHAT_JSON}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    value = json.load(handle)
trace_id = value.get("trace_id")
if not trace_id:
    raise SystemExit("chat response missing trace_id")
print(trace_id)
PY
)"
  docker compose exec -T tonglingyu-gateway sh -lc '
test -n "${TONGLINGYU_ADMIN_API_KEY}"
curl -fsS -H "Authorization: Bearer ${TONGLINGYU_ADMIN_API_KEY}" "http://127.0.0.1:8090/v1/admin/traces/'"${TRACE_ID}"'"
' >"${TRACE_JSON}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_STREAM_TRACE_JSON:-}" ]]; then
  STREAM_TRACE_ID="$(python3 - "${STREAM_TXT}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    for raw_line in handle:
        line = raw_line.strip()
        if not line.startswith("data:"):
            continue
        payload = line[len("data:"):].strip()
        if not payload or payload == "[DONE]":
            continue
        value = json.loads(payload)
        trace_id = value.get("trace_id")
        if trace_id:
            print(trace_id)
            raise SystemExit(0)
raise SystemExit("stream response missing trace_id")
PY
)"
  docker compose exec -T tonglingyu-gateway sh -lc '
test -n "${TONGLINGYU_ADMIN_API_KEY}"
curl -fsS -H "Authorization: Bearer ${TONGLINGYU_ADMIN_API_KEY}" "http://127.0.0.1:8090/v1/admin/traces/'"${STREAM_TRACE_ID}"'"
' >"${STREAM_TRACE_JSON}"
fi

if [[ -z "${TONGLINGYU_GATEWAY_VERIFY_RUNNING_IMAGES_JSON:-}" ]]; then
  python3 - "${DEPLOY_DIR}" "${RUNNING_IMAGES_JSON}" <<'PY'
import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

deploy_dir = Path(sys.argv[1])
target_path = Path(sys.argv[2])


def run_json(args):
    completed = subprocess.run(
        args,
        cwd=deploy_dir,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def sha256_text(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


container_ids_raw = subprocess.run(
    ["docker", "compose", "ps", "-q"],
    cwd=deploy_dir,
    check=True,
    capture_output=True,
    text=True,
).stdout
container_ids = [
    line.strip()
    for line in container_ids_raw.splitlines()
    if line.strip()
]
containers = run_json(["docker", "inspect", *container_ids]) if container_ids else []
images = []
for container in containers:
    labels = (container.get("Config") or {}).get("Labels") or {}
    service = str(labels.get("com.docker.compose.service") or "")
    configured_image = str((container.get("Config") or {}).get("Image") or "")
    image_id = str(container.get("Image") or "")
    image_info = run_json(["docker", "image", "inspect", image_id])[0] if image_id else {}
    repo_digests = sorted(
        str(item)
        for item in (image_info.get("RepoDigests") or [])
        if str(item).strip()
    )
    images.append({
        "service": service,
        "configured_image": configured_image,
        "image_id": image_id,
        "image_id_sha256": sha256_text(image_id),
        "repo_digests": repo_digests,
        "repo_digests_sha256": sha256_text("\n".join(repo_digests) + "\n"),
        "container_id_sha256": sha256_text(str(container.get("Id") or "")),
    })

payload = {
    "object": "tonglingyu.running_image_inventory",
    "schema_version": 1,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "image_count": len(images),
    "images": sorted(images, key=lambda item: item["service"]),
    "secret_values_printed": False,
}
target_path.write_text(
    json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
fi

python3 - "${HEALTH_JSON}" "${MODELS_JSON}" "${METRICS_JSON}" "${PROMETHEUS_TXT}" \
  "${CHAT_JSON}" "${STREAM_TXT}" "${TRACE_JSON}" "${STREAM_TRACE_JSON}" \
  "${RUNNING_IMAGES_JSON}" "${REPO_DIR}" <<'PY'
import hashlib
import json
import os
import sys
from pathlib import Path

(
    health_path,
    models_path,
    metrics_path,
    prometheus_path,
    chat_path,
    stream_path,
    trace_path,
    stream_trace_path,
    running_images_path,
    repo_dir_raw,
) = sys.argv[1:11]
repo_dir = Path(repo_dir_raw)
with open(health_path, "r", encoding="utf-8") as handle:
    health = json.load(handle)
with open(models_path, "r", encoding="utf-8") as handle:
    models = json.load(handle)
with open(metrics_path, "r", encoding="utf-8") as handle:
    metrics = json.load(handle)
with open(prometheus_path, "r", encoding="utf-8") as handle:
    prometheus = handle.read()
with open(chat_path, "r", encoding="utf-8") as handle:
    chat = json.load(handle)
with open(stream_path, "r", encoding="utf-8") as handle:
    stream = handle.read()
with open(trace_path, "r", encoding="utf-8") as handle:
    trace = json.load(handle)
with open(stream_trace_path, "r", encoding="utf-8") as handle:
    stream_trace = json.load(handle)
with open(running_images_path, "r", encoding="utf-8") as handle:
    running_images = json.load(handle)

errors = []
forbidden_public_chat_keys = {
    "_runtime_stream_events",
    "_stream_source",
    "agent_runtime",
    "agent_runtime_plan_gate",
    "agent_runtime_summary",
    "audit_events",
    "internal_trace",
    "runtime_step_outputs",
    "runtime_step_plan",
    "workflow_states",
}


def forbidden_public_chat_paths(value, prefix="$"):
    paths = []
    if isinstance(value, dict):
        for key, child in value.items():
            field = f"{prefix}.{key}"
            if key in forbidden_public_chat_keys:
                paths.append(field)
                continue
            paths.extend(forbidden_public_chat_paths(child, field))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            paths.extend(forbidden_public_chat_paths(child, f"{prefix}[{index}]"))
    return paths


def parse_stream_events(stream_text):
    events = []
    done_seen = False
    for line_number, raw_line in enumerate(stream_text.splitlines(), start=1):
        line = raw_line.strip()
        if not line or line.startswith(":"):
            continue
        if line.startswith(("event:", "id:", "retry:")):
            continue
        if not line.startswith("data:"):
            errors.append(f"stream line {line_number} must be an SSE data line")
            continue
        payload = line[len("data:"):].strip()
        if payload == "[DONE]":
            done_seen = True
            continue
        if not payload:
            errors.append(f"stream line {line_number} must not have an empty data payload")
            continue
        try:
            events.append(json.loads(payload))
        except json.JSONDecodeError as exc:
            errors.append(
                f"stream line {line_number} data must be JSON or [DONE]: {exc.msg}"
            )
    return events, done_seen


def unique_event_values(events, key):
    return {
        event.get(key)
        for event in events
        if isinstance(event, dict) and event.get(key)
    }


def stream_event_has_content_delta(event):
    for choice in event.get("choices") or []:
        if not isinstance(choice, dict):
            continue
        delta = choice.get("delta") or {}
        if isinstance(delta, dict) and delta.get("content"):
            return True
    return False


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def optional_file_sha256(path):
    try:
        return file_sha256(path)
    except OSError:
        errors.append(f"policy file missing: {path.name}")
        return ""


def canonical_digest(value):
    encoded = json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    return sha256_bytes(encoded.encode("utf-8"))


def is_sha256(value):
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(char in "0123456789abcdef" for char in value.lower())
    )


if running_images.get("object") != "tonglingyu.running_image_inventory":
    errors.append("running image inventory object invalid")
if running_images.get("secret_values_printed") is not False:
    errors.append("running image inventory must not print secret values")
running_image_items = running_images.get("images")
if not isinstance(running_image_items, list) or not running_image_items:
    errors.append("running image inventory must include running containers")
else:
    running_services = {
        item.get("service")
        for item in running_image_items
        if isinstance(item, dict) and item.get("service")
    }
    for required_service in ("tonglingyu-gateway", "open-webui"):
        if required_service not in running_services:
            errors.append(f"running image inventory missing {required_service}")
    for index, item in enumerate(running_image_items):
        if not isinstance(item, dict):
            errors.append(f"running image inventory item {index} must be object")
            continue
        image_id = str(item.get("image_id") or "")
        if not image_id.startswith("sha256:") or not is_sha256(image_id.removeprefix("sha256:")):
            errors.append(f"running image inventory item {index} image_id must be sha256")
        if not item.get("configured_image"):
            errors.append(f"running image inventory item {index} configured_image missing")
        if not isinstance(item.get("repo_digests"), list):
            errors.append(f"running image inventory item {index} repo_digests must be array")
        if not is_sha256(item.get("image_id_sha256")):
            errors.append(f"running image inventory item {index} image_id_sha256 invalid")
        if not is_sha256(item.get("repo_digests_sha256")):
            errors.append(f"running image inventory item {index} repo_digests_sha256 invalid")


def validate_trace_summary_surface(trace_value, expected_trace_id, expected_package_id, label):
    if trace_value.get("trace_id") != expected_trace_id:
        errors.append(f"{label} trace_id must match response trace_id")
    event_types = {
        item.get("event_type")
        for item in trace_value.get("audit_events") or []
        if isinstance(item, dict)
    }
    for event_type in [
        "agent_runtime_profile_step_executed",
        "agent_runtime_profile_evidence_observed",
        "agent_runtime_profile_package_observed",
        "agent_runtime_profile_review_observed",
    ]:
        if event_type not in event_types:
            errors.append(f"{label} must include {event_type}")
    runtime_step_events = [
        item
        for item in trace_value.get("audit_events") or []
        if item.get("event_type") == "agent_runtime_profile_step_executed"
    ]
    runtime_summary_events = [
        item
        for item in trace_value.get("audit_events") or []
        if item.get("event_type") == "agent_runtime_profile_execution_summarized"
    ]
    if not runtime_summary_events:
        errors.append(f"{label} must include agent_runtime_profile_execution_summarized")
        return
    runtime_summary = runtime_summary_events[-1].get("payload") or {}
    if trace_value.get("agent_runtime_summary") != runtime_summary:
        errors.append(f"{label} agent_runtime_summary must match latest summary event")
    if runtime_summary.get("mode") != "hermes":
        errors.append(f"{label} runtime summary mode must be hermes")
    if (
        runtime_summary.get("profile_execution_status")
        != "hermes_profile_observed_with_local_governance"
    ):
        errors.append(
            f"{label} runtime summary profile_execution_status must be "
            "hermes_profile_observed_with_local_governance"
        )
    if runtime_summary.get("hermes_content_execution_complete") is not True:
        errors.append(f"{label} runtime summary hermes_content_execution_complete must be true")
    if runtime_summary.get("local_governance_enforced") is not True:
        errors.append(f"{label} runtime summary local_governance_enforced must be true")
    if int(runtime_summary.get("tool_result_count") or 0) <= 0:
        errors.append(f"{label} runtime summary tool_result_count must be positive")
    if int(runtime_summary.get("tool_audit_event_count") or 0) <= 0:
        errors.append(f"{label} runtime summary tool_audit_event_count must be positive")
    if int(runtime_summary.get("tool_audit_event_count") or 0) < int(runtime_summary.get("tool_result_count") or 0):
        errors.append(f"{label} runtime summary tool_audit_event_count must cover runtime tool results")
    operations = {
        ((item.get("payload") or {}).get("operation"))
        for item in runtime_step_events
    }
    for operation in [
        "text_evidence_search",
        "evidence_package_create",
        "draft_answer",
        "review_answer",
    ]:
        if operation not in operations:
            errors.append(f"{label} must include runtime operation {operation}")
    for item in runtime_step_events:
        payload = item.get("payload") or {}
        agent_runtime = payload.get("agent_runtime") or {}
        operation = payload.get("operation")
        if agent_runtime.get("client") != "hermes":
            errors.append(f"{label} runtime step {operation} must use hermes client")
        if agent_runtime.get("status") != "executed":
            errors.append(f"{label} runtime step {operation} must be executed")
        if int(agent_runtime.get("tool_result_count") or 0) <= 0:
            errors.append(f"{label} runtime step {operation} must include tool results")
        if int(agent_runtime.get("tool_audit_event_count") or 0) <= 0:
            errors.append(f"{label} runtime step {operation} must include tool audit events")
        tool_results = agent_runtime.get("tool_results") or []
        tool_audit_events = agent_runtime.get("tool_audit_events") or []
        if len(tool_audit_events) < len(tool_results):
            errors.append(f"{label} runtime step {operation} tool audit events must cover tool results")
        for result in tool_results:
            if not isinstance(result, dict):
                errors.append(f"{label} runtime step {operation} tool result must be an object")
                continue
            tool_name = result.get("tool_name")
            output_ref = result.get("output_ref")
            matching_result_audit = any(
                isinstance(event, dict)
                and event.get("event") == "runtime_tool_result"
                and event.get("tool_name") == tool_name
                and event.get("output_ref") == output_ref
                for event in tool_audit_events
            )
            if not matching_result_audit:
                errors.append(
                    f"{label} runtime step {operation} tool {tool_name} result must have matching audit event"
                )
            if not output_ref:
                errors.append(f"{label} runtime step {operation} tool {tool_name} must include output_ref")
                continue
            if expected_trace_id and not str(output_ref).startswith(f"runtime://tonglingyu/{expected_trace_id}/"):
                errors.append(f"{label} runtime step {operation} tool {tool_name} output_ref must bind to trace")
            if tool_name in {
                "tonglingyu.evidence.package.create",
                "tonglingyu.evidence.package.read",
                "tonglingyu.evidence.package.replay",
            } and expected_trace_id and expected_package_id:
                expected_ref = f"runtime://tonglingyu/{expected_trace_id}/packages/{expected_package_id}"
                if output_ref != expected_ref:
                    errors.append(f"{label} runtime step {operation} tool {tool_name} output_ref must bind to package")


if health.get("status") != "ok":
    errors.append("health.status must be ok")
if health.get("model") != "tonglingyu":
    errors.append("health.model must be tonglingyu")
if (health.get("agent_runtime") or {}).get("mode") != "hermes":
    errors.append("health.agent_runtime.mode must be hermes")
if int(health.get("sources") or 0) <= 0:
    errors.append("health.sources must be positive")
if int(health.get("blocks") or 0) <= 0:
    errors.append("health.blocks must be positive")

model_ids = [
    item.get("id")
    for item in models.get("data") or []
    if isinstance(item, dict) and item.get("id")
]
if model_ids != ["tonglingyu"]:
    errors.append("models.data must expose only tonglingyu")
if any(str(model_id).startswith("honglou-") for model_id in model_ids):
    errors.append("models.data must not expose internal honglou-* profiles")

dependencies = metrics.get("dependencies") or {}
security = metrics.get("security") or {}
limits = metrics.get("limits") or {}
if metrics.get("object") != "tonglingyu.gateway_metrics":
    errors.append("metrics.object must be tonglingyu.gateway_metrics")
if (dependencies.get("agent_runtime") or {}).get("mode") != "hermes":
    errors.append("metrics.dependencies.agent_runtime.mode must be hermes")
if dependencies.get("upstream") != "configured":
    errors.append("metrics.dependencies.upstream must be configured")
if int(security.get("gateway_key_count") or 0) <= 0:
    errors.append("metrics.security.gateway_key_count must be positive")
if int(security.get("admin_key_count") or 0) <= 0:
    errors.append("metrics.security.admin_key_count must be positive")
if security.get("admin_key_isolated") is not True:
    errors.append("metrics.security.admin_key_isolated must be true")
if security.get("rate_limit_disabled") is True:
    errors.append("metrics.security.rate_limit_disabled must be false")
if int(limits.get("max_body_bytes") or 0) <= 0:
    errors.append("metrics.limits.max_body_bytes must be positive")
rqa_metrics = metrics.get("rqa") or {}
retrieval_failure_metrics = rqa_metrics.get("retrieval_failures") or {}
if rqa_metrics.get("schema_version") != "tonglingyu-retrieval-failures-v1":
    errors.append("metrics.rqa.schema_version must be tonglingyu-retrieval-failures-v1")
if not isinstance(retrieval_failure_metrics.get("by_status"), dict):
    errors.append("metrics.rqa.retrieval_failures.by_status must be an object")
if not isinstance(retrieval_failure_metrics.get("by_type"), dict):
    errors.append("metrics.rqa.retrieval_failures.by_type must be an object")


def sensitive_metric_paths(value, prefix="$"):
    hits = []
    if isinstance(value, dict):
        for key, child in value.items():
            normalized = str(key).lower()
            child_prefix = f"{prefix}.{key}"
            if normalized in {
                "query",
                "question",
                "raw_query",
                "raw_question",
                "prompt",
                "trace_id",
                "trace_ids",
                "package_id",
                "package_ids",
                "session_id",
                "session_ids",
                "user_id",
                "user_ids",
            }:
                hits.append(child_prefix)
            hits.extend(sensitive_metric_paths(child, child_prefix))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            hits.extend(sensitive_metric_paths(child, f"{prefix}[{index}]"))
    return hits


metrics_sensitive_paths = sensitive_metric_paths(metrics)
prometheus_forbidden_needles = [
    "query",
    "question",
    "raw_query",
    "raw_question",
    "trace_id",
    "package_id",
    "session_id",
    "user_id",
    "x-api-key",
    "authorization",
    "bearer ",
]
prometheus_sensitive_tokens = [
    needle
    for needle in prometheus_forbidden_needles
    if needle in prometheus.lower()
]
known_secret_values = [
    value.strip()
    for value in (
        os.environ.get("TONGLINGYU_ADMIN_API_KEY", ""),
        os.environ.get("TONGLINGYU_GATEWAY_API_KEY", ""),
        os.environ.get("OPENAI_API_KEY", ""),
        os.environ.get("AGENT_BRIDGE_SECRET", ""),
    )
    if value and len(value.strip()) >= 8
]
metrics_encoded = json.dumps(metrics, ensure_ascii=True, sort_keys=True)
secret_values_in_metrics = any(secret in metrics_encoded for secret in known_secret_values)
secret_values_in_prometheus = any(secret in prometheus for secret in known_secret_values)
metrics_privacy = {
    "object": "tonglingyu.strict_gateway_metrics_privacy",
    "schema_version": 1,
    "json_metrics_sensitive_paths": metrics_sensitive_paths,
    "json_metrics_sensitive_paths_sha256": canonical_digest(metrics_sensitive_paths),
    "prometheus_sensitive_tokens": prometheus_sensitive_tokens,
    "prometheus_sensitive_tokens_sha256": canonical_digest(prometheus_sensitive_tokens),
    "json_metrics_secret_values_present": secret_values_in_metrics,
    "prometheus_secret_values_present": secret_values_in_prometheus,
    "secret_values_printed": False,
}
if metrics_sensitive_paths:
    errors.append("metrics JSON must not expose query, trace, package, session, or user identifiers")
if prometheus_sensitive_tokens:
    errors.append("prometheus metrics must not expose query, trace, package, session, user, or auth labels")
if secret_values_in_metrics:
    errors.append("metrics JSON must not expose secret values")
if secret_values_in_prometheus:
    errors.append("prometheus metrics must not expose secret values")

runtime_policy_digest = optional_file_sha256(
    repo_dir / "agent-platform" / "crates" / "tonglingyu-runtime" / "src" / "lib.rs"
)
gateway_policy_digest = optional_file_sha256(
    repo_dir / "agent-platform" / "crates" / "tonglingyu-gateway" / "src" / "main.rs"
)
model_upstream_id = (
    os.environ.get("TONGLINGYU_UPSTREAM_MODEL")
    or os.environ.get("AGENT_RUNTIME_HERMES_MODEL")
    or ""
).strip()
if not model_upstream_id:
    errors.append("strict gateway model upstream id missing")
behavior_config = {
    "agent_runtime_mode_env": "TONGLINGYU_AGENT_RUNTIME_MODE",
    "decoding_parameters_summary": {
        "source": "gateway_runtime_config",
        "upstream_timeout_secs_env": "TONGLINGYU_UPSTREAM_TIMEOUT_SECS",
    },
    "profile_contract": "tonglingyu-runtime-profile-contract-v1",
    "runtime_profile_digest": runtime_policy_digest,
    "prompt_digest": runtime_policy_digest,
    "tool_policy": "read_only_runtime_tools",
    "tool_policy_digest": runtime_policy_digest,
    "reviewer_policy": "local_reviewer_enforced",
    "reviewer_policy_digest": runtime_policy_digest,
    "gateway_policy_digest": gateway_policy_digest,
    "model_upstream_id": model_upstream_id,
    "model_upstream_bound_by_gate": "model_upstream_network",
    "decoding_parameters_source": "gateway_runtime_config",
}
behavior_config["behavior_config_digest"] = canonical_digest(behavior_config)

if 'agent_runtime_mode="hermes"' not in prometheus:
    errors.append("prometheus tonglingyu_gateway_info must include agent_runtime_mode=hermes")
if 'agent_runtime_mode="minimal"' in prometheus:
    errors.append("prometheus tonglingyu_gateway_info must not report minimal runtime mode")
if "tonglingyu_retrieval_failures_total" not in prometheus:
    errors.append("prometheus must expose bounded retrieval failure totals")

chat_trace_id = chat.get("trace_id")
chat_package_id = chat.get("evidence_package_id")
if not chat_trace_id:
    errors.append("chat response must include trace_id")
if not chat_package_id:
    errors.append("chat response must include evidence_package_id")
for forbidden_chat_path in forbidden_public_chat_paths(chat):
    errors.append(f"chat response must not expose {forbidden_chat_path}")
stream_events, stream_done_seen = parse_stream_events(stream)
if not stream_done_seen:
    errors.append("stream response must include data: [DONE]")
if not stream_events:
    errors.append("stream response must include JSON data chunks")
stream_trace_ids = unique_event_values(stream_events, "trace_id")
stream_package_ids = unique_event_values(stream_events, "evidence_package_id")
stream_session_ids = unique_event_values(stream_events, "session_id")
stream_trace_id = next(iter(stream_trace_ids), None)
stream_package_id = next(iter(stream_package_ids), None)
if len(stream_trace_ids) != 1:
    errors.append("stream response must carry exactly one trace_id across chunks")
if len(stream_package_ids) != 1:
    errors.append("stream response must carry exactly one evidence_package_id across chunks")
if len(stream_session_ids) > 1:
    errors.append("stream response must not mix session_id values across chunks")
if not any(
    isinstance(event, dict) and event.get("evidence_package_id")
    for event in stream_events
):
    errors.append("stream response must include evidence_package_id")
if not any(
    isinstance(event, dict)
    and (
        event.get("runtime_workflow")
        or event.get("stream_source") == "runtime_workflow"
    )
    for event in stream_events
):
    errors.append("stream response must identify runtime_workflow source")
content_delta_events = [
    event
    for event in stream_events
    if isinstance(event, dict)
    and (event.get("runtime_event") or {}).get("event_type") == "content_delta"
]
if not content_delta_events:
    errors.append("stream response must include runtime content_delta chunks")
for content_event in content_delta_events:
    if not stream_event_has_content_delta(content_event):
        errors.append("stream runtime content_delta chunk must carry assistant content")
for index, stream_event in enumerate(stream_events):
    for forbidden_stream_path in forbidden_public_chat_paths(stream_event, f"$[{index}]"):
        errors.append(f"stream response must not expose {forbidden_stream_path}")
if stream_trace_id and stream_package_id:
    validate_trace_summary_surface(
        stream_trace,
        stream_trace_id,
        stream_package_id,
        "stream admin trace",
    )
if trace.get("trace_id") != chat_trace_id:
    errors.append("admin trace must match chat trace_id")
trace_quality_summary = trace.get("retrieval_quality_summary") or {}
if trace_quality_summary.get("schema_version") != "tonglingyu-retrieval-failures-v1":
    errors.append("admin trace must expose RQA retrieval quality summary")
if not isinstance(trace.get("retrieval_failure_ids"), list):
    errors.append("admin trace retrieval_failure_ids must be a list")
event_types = {
    item.get("event_type")
    for item in trace.get("audit_events") or []
    if isinstance(item, dict)
}
for event_type in [
    "agent_runtime_profile_step_executed",
    "agent_runtime_profile_evidence_observed",
    "agent_runtime_profile_package_observed",
    "agent_runtime_profile_review_observed",
]:
    if event_type not in event_types:
        errors.append(f"admin trace must include {event_type}")

runtime_step_events = [
    item
    for item in trace.get("audit_events") or []
    if item.get("event_type") == "agent_runtime_profile_step_executed"
]
runtime_summary_events = [
    item
    for item in trace.get("audit_events") or []
    if item.get("event_type") == "agent_runtime_profile_execution_summarized"
]
if not runtime_summary_events:
    errors.append("admin trace must include agent_runtime_profile_execution_summarized")
else:
    runtime_summary = runtime_summary_events[-1].get("payload") or {}
    trace_runtime_summary = trace.get("agent_runtime_summary") or {}
    if trace_runtime_summary != runtime_summary:
        errors.append("admin trace agent_runtime_summary must match latest runtime summary event")
    if runtime_summary.get("mode") != "hermes":
        errors.append("runtime summary mode must be hermes")
    if (
        runtime_summary.get("profile_execution_status")
        != "hermes_profile_observed_with_local_governance"
    ):
        errors.append(
            "runtime summary profile_execution_status must be "
            "hermes_profile_observed_with_local_governance"
        )
    if runtime_summary.get("hermes_content_execution_complete") is not True:
        errors.append("runtime summary hermes_content_execution_complete must be true")
    if runtime_summary.get("local_governance_enforced") is not True:
        errors.append("runtime summary local_governance_enforced must be true")
    if int(runtime_summary.get("tool_result_count") or 0) <= 0:
        errors.append("runtime summary tool_result_count must be positive")
    if int(runtime_summary.get("tool_audit_event_count") or 0) <= 0:
        errors.append("runtime summary tool_audit_event_count must be positive")
    if int(runtime_summary.get("profile_step_count") or 0) != len(runtime_step_events):
        errors.append("runtime summary profile_step_count must match runtime step events")
    if int(runtime_summary.get("executed_profile_step_count") or 0) != len(runtime_step_events):
        errors.append("runtime summary executed_profile_step_count must match runtime step events")
    step_tool_result_count = 0
    step_tool_audit_event_count = 0
    for item in runtime_step_events:
        agent_runtime = (item.get("payload") or {}).get("agent_runtime") or {}
        step_tool_result_count += int(agent_runtime.get("tool_result_count") or 0)
        step_tool_audit_event_count += int(agent_runtime.get("tool_audit_event_count") or 0)
    if int(runtime_summary.get("tool_result_count") or 0) != step_tool_result_count:
        errors.append("runtime summary tool_result_count must match runtime step tool results")
    if int(runtime_summary.get("tool_audit_event_count") or 0) != step_tool_audit_event_count:
        errors.append("runtime summary tool_audit_event_count must match runtime step tool audit events")
    if int(runtime_summary.get("tool_audit_event_count") or 0) < int(runtime_summary.get("tool_result_count") or 0):
        errors.append("runtime summary tool_audit_event_count must cover runtime tool results")
operations = {
    ((item.get("payload") or {}).get("operation"))
    for item in runtime_step_events
}
for operation in [
    "text_evidence_search",
    "evidence_package_create",
    "draft_answer",
    "review_answer",
]:
    if operation not in operations:
        errors.append(f"admin trace must include runtime operation {operation}")
for item in runtime_step_events:
    payload = item.get("payload") or {}
    agent_runtime = payload.get("agent_runtime") or {}
    operation = payload.get("operation")
    if agent_runtime.get("client") != "hermes":
        errors.append(f"runtime step {operation} must use hermes client")
    if agent_runtime.get("status") != "executed":
        errors.append(f"runtime step {operation} must be executed")
    if int(agent_runtime.get("tool_result_count") or 0) <= 0:
        errors.append(f"runtime step {operation} must include tool results")
    if int(agent_runtime.get("tool_audit_event_count") or 0) <= 0:
        errors.append(f"runtime step {operation} must include tool audit events")
    tool_results = agent_runtime.get("tool_results") or []
    tool_audit_events = agent_runtime.get("tool_audit_events") or []
    if not any(isinstance(result, dict) and result.get("tool_name") for result in tool_results):
        errors.append(f"runtime step {operation} must include tool_name in tool results")
    if len(tool_audit_events) < len(tool_results):
        errors.append(f"runtime step {operation} tool audit events must cover tool results")
    for result in tool_results:
        if not isinstance(result, dict):
            errors.append(f"runtime step {operation} tool result must be an object")
            continue
        tool_name = result.get("tool_name")
        output_ref = result.get("output_ref")
        matching_result_audit = any(
            isinstance(event, dict)
            and event.get("event") == "runtime_tool_result"
            and event.get("tool_name") == tool_name
            and event.get("output_ref") == output_ref
            for event in tool_audit_events
        )
        if not matching_result_audit:
            errors.append(
                f"runtime step {operation} tool {tool_name} result must have matching audit event"
            )
        if not output_ref:
            errors.append(f"runtime step {operation} tool {tool_name} must include output_ref")
            continue
        if chat_trace_id and not str(output_ref).startswith(f"runtime://tonglingyu/{chat_trace_id}/"):
            errors.append(f"runtime step {operation} tool {tool_name} output_ref must bind to trace")
        if tool_name in {
            "tonglingyu.text.search",
            "tonglingyu.commentary.search",
        } and chat_trace_id:
            expected_prefix = f"runtime://tonglingyu/{chat_trace_id}/evidence/"
            if not str(output_ref).startswith(expected_prefix):
                errors.append(
                    f"runtime step {operation} tool {tool_name} output_ref must bind to evidence set"
                )
        if tool_name in {
            "tonglingyu.evidence.package.create",
            "tonglingyu.evidence.package.read",
            "tonglingyu.evidence.package.replay",
        } and chat_trace_id and chat_package_id:
            expected_ref = f"runtime://tonglingyu/{chat_trace_id}/packages/{chat_package_id}"
            if output_ref != expected_ref:
                errors.append(f"runtime step {operation} tool {tool_name} output_ref must bind to package")
    if operation in {"text_evidence_search", "commentary_evidence_search"}:
        evidence_observation = agent_runtime.get("evidence_observation") or {}
        if evidence_observation.get("matches_runtime_evidence") is not True:
            errors.append(f"runtime step {operation} evidence observation must match local evidence")
        if evidence_observation.get("local_evidence_enforced") is not True:
            errors.append(f"runtime step {operation} must enforce local evidence")
    if operation == "evidence_package_create":
        package_observation = agent_runtime.get("package_observation") or {}
        if package_observation.get("matches_runtime_package") is not True:
            errors.append(f"runtime step {operation} package observation must match local package")
        if package_observation.get("local_package_enforced") is not True:
            errors.append(f"runtime step {operation} must enforce local package")
    if operation == "draft_answer":
        content_application = agent_runtime.get("content_application") or {}
        if content_application.get("draft_consumed") is not True:
            errors.append(f"runtime step {operation} must consume Hermes draft output")
        if content_application.get("local_reviewer_enforced") is not True:
            errors.append(f"runtime step {operation} must enforce local reviewer")
    if operation == "review_answer":
        review_observation = agent_runtime.get("review_observation") or {}
        if review_observation.get("local_reviewer_enforced") is not True:
            errors.append(f"runtime step {operation} must enforce local reviewer")

trace_runtime_summary_for_binding = (
    trace.get("agent_runtime_summary")
    if isinstance(trace.get("agent_runtime_summary"), dict)
    else {}
)
stream_runtime_summary_for_binding = (
    stream_trace.get("agent_runtime_summary")
    if isinstance(stream_trace.get("agent_runtime_summary"), dict)
    else {}
)
behavior_config_binding = {
    "object": "tonglingyu.strict_gateway_behavior_config_binding",
    "schema_version": 1,
    "policy_version": "tonglingyu-behavior-config-binding-v1",
    "behavior_config_digest": behavior_config.get("behavior_config_digest"),
    "behavior_config_sha256": canonical_digest(behavior_config),
    "admin_trace_id": chat_trace_id,
    "stream_trace_id": stream_trace_id,
    "admin_trace_runtime_summary": trace_runtime_summary_for_binding,
    "admin_trace_runtime_summary_sha256": (
        canonical_digest(trace_runtime_summary_for_binding)
        if trace_runtime_summary_for_binding
        else ""
    ),
    "stream_trace_runtime_summary": stream_runtime_summary_for_binding,
    "stream_trace_runtime_summary_sha256": (
        canonical_digest(stream_runtime_summary_for_binding)
        if stream_runtime_summary_for_binding
        else ""
    ),
    "agent_runtime_mode": trace_runtime_summary_for_binding.get("mode"),
    "profile_execution_status": trace_runtime_summary_for_binding.get(
        "profile_execution_status"
    ),
    "hermes_content_execution_complete": trace_runtime_summary_for_binding.get(
        "hermes_content_execution_complete"
    ),
    "local_governance_enforced": trace_runtime_summary_for_binding.get(
        "local_governance_enforced"
    ),
    "profile_step_count": trace_runtime_summary_for_binding.get("profile_step_count"),
    "executed_profile_step_count": trace_runtime_summary_for_binding.get(
        "executed_profile_step_count"
    ),
    "tool_result_count": trace_runtime_summary_for_binding.get("tool_result_count"),
    "tool_audit_event_count": trace_runtime_summary_for_binding.get(
        "tool_audit_event_count"
    ),
    "secret_values_printed": False,
}
if behavior_config_binding["behavior_config_sha256"] != canonical_digest(behavior_config):
    errors.append("behavior config binding digest must match behavior_config")
for label, summary in (
    ("admin trace", trace_runtime_summary_for_binding),
    ("stream admin trace", stream_runtime_summary_for_binding),
):
    if not summary:
        errors.append(f"{label} runtime summary missing for behavior config binding")
        continue
    if summary.get("mode") != "hermes":
        errors.append(f"{label} behavior binding runtime mode must be hermes")
    if (
        summary.get("profile_execution_status")
        != "hermes_profile_observed_with_local_governance"
    ):
        errors.append(
            f"{label} behavior binding profile execution status must be hermes complete"
        )
    if summary.get("hermes_content_execution_complete") is not True:
        errors.append(f"{label} behavior binding content execution must be complete")
    if summary.get("local_governance_enforced") is not True:
        errors.append(f"{label} behavior binding local governance must be enforced")
    if int(summary.get("tool_result_count") or 0) <= 0:
        errors.append(f"{label} behavior binding tool result count must be positive")
    if int(summary.get("tool_audit_event_count") or 0) < int(
        summary.get("tool_result_count") or 0
    ):
        errors.append(f"{label} behavior binding tool audit count must cover results")

if errors:
    for error in errors:
        print(f"strict_gateway_error={error}", file=sys.stderr)
    sys.exit(1)

print(json.dumps(
    {
        "status": "ok",
        "checked_surfaces": [
            "tonglingyu-gateway:/healthz",
            "open-webui->tonglingyu-gateway:/v1/models",
            "tonglingyu-gateway:/v1/admin/metrics",
            "tonglingyu-gateway:/v1/admin/metrics/prometheus",
            "open-webui->tonglingyu-gateway:/v1/chat/completions",
            "open-webui->tonglingyu-gateway:/v1/chat/completions stream",
            "tonglingyu-gateway:/v1/admin/traces/{trace_id}",
            "tonglingyu-gateway:/v1/admin/traces/{stream_trace_id}",
        ],
        "model_ids": model_ids,
        "agent_runtime_mode": "hermes",
        "admin_key_isolated": True,
        "trace_id": chat_trace_id,
        "evidence_package_id": chat_package_id,
        "stream_trace_id": stream_trace_id,
        "stream_evidence_package_id": stream_package_id,
        "behavior_config": behavior_config,
        "behavior_config_binding": behavior_config_binding,
        "metrics_privacy": metrics_privacy,
        "running_images": running_images,
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
