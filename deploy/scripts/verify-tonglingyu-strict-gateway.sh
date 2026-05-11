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

python3 - "${HEALTH_JSON}" "${MODELS_JSON}" "${METRICS_JSON}" "${PROMETHEUS_TXT}" <<'PY'
import json
import sys

health_path, models_path, metrics_path, prometheus_path = sys.argv[1:5]
with open(health_path, "r", encoding="utf-8") as handle:
    health = json.load(handle)
with open(models_path, "r", encoding="utf-8") as handle:
    models = json.load(handle)
with open(metrics_path, "r", encoding="utf-8") as handle:
    metrics = json.load(handle)
with open(prometheus_path, "r", encoding="utf-8") as handle:
    prometheus = handle.read()

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
        ],
        "model_ids": model_ids,
        "agent_runtime_mode": "hermes",
        "admin_key_isolated": True,
    },
    ensure_ascii=True,
    sort_keys=True,
))
PY
