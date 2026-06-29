"""
title: Agent Identity Bridge
author: Hermes Home
version: 1.0.0
required_open_webui_version: 0.6.0
"""

import hashlib
import hmac
import json
import secrets
import time
from typing import Any, Optional

try:
    from pydantic import BaseModel, Field
except ModuleNotFoundError:
    class BaseModel:
        def __init__(self, **kwargs: Any) -> None:
            annotations = getattr(self, "__annotations__", {})
            for name in annotations:
                setattr(self, name, kwargs.get(name, getattr(self.__class__, name, None)))

    def Field(default: Any = None, **_: Any) -> Any:
        return default


FORBIDDEN_GATEWAY_CONTROL_FIELDS = {
    "agent",
    "agent_id",
    "agent_profile",
    "agent_runtime",
    "agent_runtime_plan_gate",
    "agent_runtime_summary",
    "profile",
    "internal_agent",
    "honglou_agent",
    "runtime_profile",
    "runtime_step_outputs",
    "runtime_step_plan",
    "reviewer",
    "skip_reviewer",
    "disable_reviewer",
    "allowed_tools",
    "required_evidence_types",
    "trace_id",
    "package_id",
    "evidence_package_id",
    "admin_trace",
    "audit_events",
    "internal_trace",
    "runtime_tools_used",
    "workflow_states",
    "workflow_state",
    "tools",
    "tool_choice",
    "functions",
    "function_call",
    "parallel_tool_calls",
    "system_prompt",
    "instructions",
    "profile_config",
    "internal_config",
    "interaction_context_id",
    "context_pack_id",
    "context_pack_ref",
    "context_projection",
    "context_projection_id",
    "context_projection_ref",
    "context_projection_digest",
    "consumer_type",
    "consumer_name",
    "runtime_adapter",
    "context_scope_binding",
    "scope_id",
    "scope_graph",
    "memory_read_scopes",
    "memory_read_refs",
    "memory_read_ref_digest",
    "memory_read_policy_digest",
    "memory_write_scopes",
    "memory_scope",
    "memory_summaries",
    "memory_policy",
    "memory_policy_digest",
    "memory_usage_summary",
    "memory_candidate",
    "memory_candidate_id",
    "memory_candidate_ref",
    "memory_candidates",
    "memory_card",
    "memory_card_id",
    "memory_card_ref",
    "memory_cards",
    "memory_policy_decision",
    "memory_policy_decision_id",
    "memory_policy_decision_ref",
    "memory_policy_decisions",
    "memory_transition_audit",
    "memory_collector",
    "llm_extraction",
    "llm_filter",
    "rule_filter",
    "read_enabled",
    "forbidden_tools",
    "tool_policy_digest",
    "output_contract_digest",
    "session_journal",
}
RECURSIVE_GATEWAY_CONTROL_CONTAINERS = {
    "metadata",
    "extra_body",
    "options",
    "parameters",
    "config",
}
SHALLOW_GATEWAY_CONTROL_CONTAINERS = {"user"}


class Filter:
    class Valves(BaseModel):
        AGENT_BRIDGE_SECRET: str = Field(default="")
        AGENT_BRIDGE_ISSUER: str = Field(default="open-webui")
        TARGET_MODEL: str = Field(default="tonglingyu")
        TARGET_MODELS: str = Field(default="tonglingyu")

    def __init__(self) -> None:
        self.valves = self.Valves()

    async def inlet(
        self,
        body: dict,
        __user__: Optional[Any] = None,
        __metadata__: Optional[dict] = None,
        __model__: Optional[Any] = None,
    ) -> dict:
        model = str(body.get("model") or _get(__model__, "id") or "")
        if model not in _target_models(self.valves.TARGET_MODELS, self.valves.TARGET_MODEL):
            return body

        _strip_gateway_control_fields(body)

        secret = self.valves.AGENT_BRIDGE_SECRET
        user_id = str(_get(__user__, "id") or "").strip()
        chat_id = str(
            _get(__metadata__, "chat_id")
            or _get(__metadata__, "conversation_id")
            or _get(body.get("metadata"), "chat_id")
            or _get(body.get("metadata"), "conversation_id")
            or body.get("chat_id")
            or body.get("conversation_id")
            or ""
        ).strip()
        if not secret or not user_id or not chat_id:
            return body

        session_id = str(
            _get(__metadata__, "session_id")
            or _get(body.get("metadata"), "session_id")
            or body.get("session_id")
            or ""
        ).strip()
        message_id = str(
            _get(__metadata__, "user_message_id")
            or _get(__metadata__, "message_id")
            or _get(body.get("metadata"), "user_message_id")
            or _get(body.get("metadata"), "message_id")
            or body.get("user_message_id")
            or body.get("message_id")
            or ""
        ).strip()
        user_role = str(_get(__user__, "role") or "user").strip() or "user"
        context = {
            "version": 1,
            "issuer": self.valves.AGENT_BRIDGE_ISSUER,
            "subject": f"openwebui:{user_id}",
            "user_role": user_role,
            "chat_id": chat_id,
            "session_id": session_id,
            "message_id": message_id,
            "model": model,
            "issued_at": int(time.time()),
            "nonce": secrets.token_urlsafe(18),
        }
        context["signature"] = _signature(secret, context)
        body["agent_bridge_context"] = context
        return body


def _strip_gateway_control_fields(body: dict) -> None:
    for key in list(body.keys()):
        if key in FORBIDDEN_GATEWAY_CONTROL_FIELDS:
            body.pop(key, None)

    for key in RECURSIVE_GATEWAY_CONTROL_CONTAINERS:
        _strip_control_fields_recursive(body.get(key))

    for key in SHALLOW_GATEWAY_CONTROL_CONTAINERS:
        value = body.get(key)
        if isinstance(value, dict):
            for child_key in list(value.keys()):
                if child_key in FORBIDDEN_GATEWAY_CONTROL_FIELDS:
                    value.pop(child_key, None)


def _strip_control_fields_recursive(value: Any) -> None:
    if isinstance(value, dict):
        for key in list(value.keys()):
            if key in FORBIDDEN_GATEWAY_CONTROL_FIELDS:
                value.pop(key, None)
                continue
            _strip_control_fields_recursive(value.get(key))
    elif isinstance(value, list):
        for item in value:
            _strip_control_fields_recursive(item)


def _get(value: Any, key: str) -> Any:
    if value is None:
        return None
    if isinstance(value, dict):
        return value.get(key)
    return getattr(value, key, None)


def _target_models(target_models: str, target_model: str) -> set[str]:
    values = [target_models, target_model]
    models: set[str] = set()
    for value in values:
        for item in str(value or "").split(","):
            item = item.strip()
            if item:
                models.add(item)
    return models


def _signature(secret: str, context: dict) -> str:
    payload = {
        "version": context.get("version", 1),
        "issuer": context.get("issuer", ""),
        "subject": context.get("subject", ""),
        "user_role": context.get("user_role", ""),
        "chat_id": context.get("chat_id", ""),
        "session_id": context.get("session_id", ""),
        "message_id": context.get("message_id", ""),
        "model": context.get("model", ""),
        "issued_at": context.get("issued_at", 0),
        "nonce": context.get("nonce", ""),
    }
    canonical = json.dumps(
        payload,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    )
    return hmac.new(
        secret.encode("utf-8"),
        canonical.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()
