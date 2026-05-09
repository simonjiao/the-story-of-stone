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


class Filter:
    class Valves(BaseModel):
        AGENT_BRIDGE_SECRET: str = Field(default="")
        AGENT_BRIDGE_ISSUER: str = Field(default="open-webui")
        TARGET_MODEL: str = Field(default="hermes-agent")

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
        if model != self.valves.TARGET_MODEL:
            return body

        secret = self.valves.AGENT_BRIDGE_SECRET
        user_id = str(_get(__user__, "id") or "").strip()
        chat_id = str(
            _get(__metadata__, "chat_id")
            or _get(__metadata__, "conversation_id")
            or _get(body.get("metadata"), "chat_id")
            or _get(body.get("metadata"), "conversation_id")
            or ""
        ).strip()
        if not secret or not user_id or not chat_id:
            return body

        session_id = str(
            _get(__metadata__, "session_id")
            or _get(body.get("metadata"), "session_id")
            or ""
        ).strip()
        message_id = str(
            _get(__metadata__, "user_message_id")
            or _get(__metadata__, "message_id")
            or _get(body.get("metadata"), "user_message_id")
            or _get(body.get("metadata"), "message_id")
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


def _get(value: Any, key: str) -> Any:
    if value is None:
        return None
    if isinstance(value, dict):
        return value.get(key)
    return getattr(value, key, None)


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
