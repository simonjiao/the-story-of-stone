#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

HEALTH_JSON="${TONGLINGYU_GATEWAY_VERIFY_HEALTH_JSON:-${WORK_DIR}/health.json}"
MODELS_JSON="${TONGLINGYU_GATEWAY_VERIFY_MODELS_JSON:-${WORK_DIR}/models.json}"
METRICS_JSON="${TONGLINGYU_GATEWAY_VERIFY_METRICS_JSON:-${WORK_DIR}/metrics.json}"
PROMETHEUS_TXT="${TONGLINGYU_GATEWAY_VERIFY_PROMETHEUS_TXT:-${WORK_DIR}/metrics.prom}"
CHAT_JSON="${TONGLINGYU_GATEWAY_VERIFY_CHAT_JSON:-${WORK_DIR}/chat.json}"
TRACE_JSON="${TONGLINGYU_GATEWAY_VERIFY_TRACE_JSON:-${WORK_DIR}/trace.json}"

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
  -H "x-tonglingyu-chat-id: strict-gateway" \
  -H "x-tonglingyu-message-id: strict-gateway-runtime-tool-smoke" \
  --data "{\"model\":\"tonglingyu\",\"messages\":[{\"role\":\"user\",\"content\":\"通灵玉是什么？\"}]}" \
  http://tonglingyu-gateway:8090/v1/chat/completions
' >"${CHAT_JSON}"
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

python3 - "${HEALTH_JSON}" "${MODELS_JSON}" "${METRICS_JSON}" "${PROMETHEUS_TXT}" \
  "${CHAT_JSON}" "${TRACE_JSON}" <<'PY'
import json
import sys

health_path, models_path, metrics_path, prometheus_path, chat_path, trace_path = sys.argv[1:7]
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
with open(trace_path, "r", encoding="utf-8") as handle:
    trace = json.load(handle)

errors = []

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

if 'agent_runtime_mode="hermes"' not in prometheus:
    errors.append("prometheus tonglingyu_gateway_info must include agent_runtime_mode=hermes")
if 'agent_runtime_mode="minimal"' in prometheus:
    errors.append("prometheus tonglingyu_gateway_info must not report minimal runtime mode")

chat_trace_id = chat.get("trace_id")
chat_package_id = chat.get("evidence_package_id")
if not chat_trace_id:
    errors.append("chat response must include trace_id")
if not chat_package_id:
    errors.append("chat response must include evidence_package_id")
for forbidden_chat_key in [
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
]:
    if forbidden_chat_key in chat:
        errors.append(f"chat response must not expose {forbidden_chat_key}")
if trace.get("trace_id") != chat_trace_id:
    errors.append("admin trace must match chat trace_id")
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
            "tonglingyu-gateway:/v1/admin/traces/{trace_id}",
        ],
        "model_ids": model_ids,
        "agent_runtime_mode": "hermes",
        "admin_key_isolated": True,
        "trace_id": chat_trace_id,
        "evidence_package_id": chat_package_id,
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
