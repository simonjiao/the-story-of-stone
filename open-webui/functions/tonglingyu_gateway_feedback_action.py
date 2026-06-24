"""
title: Tonglingyu Feedback
author: Hermes Home
version: 1.0.0
required_open_webui_version: 0.6.0
"""

import asyncio
import json
import re
import urllib.error
import urllib.parse
import urllib.request
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


class FeedbackActionError(Exception):
    pass


class Action:
    actions = [{"id": "feedback", "name": "Send Tonglingyu feedback"}]

    class Valves(BaseModel):
        GATEWAY_BASE_URL: str = Field(default="http://tonglingyu-gateway:8090")
        GATEWAY_API_KEY: str = Field(default="")
        TARGET_MODEL: str = Field(default="tonglingyu")
        TARGET_MODELS: str = Field(default="tonglingyu")
        REQUEST_TIMEOUT_SECONDS: int = Field(default=15)
        RESPONSE_MAX_CHARS: int = Field(default=3000)
        priority: int = Field(default=10)

    def __init__(self) -> None:
        self.valves = self.Valves()

    async def action(
        self,
        body: dict,
        __user__: Optional[Any] = None,
        __event_call__: Optional[Any] = None,
        __event_emitter__: Optional[Any] = None,
        __id__: Optional[str] = None,
        __model__: Optional[Any] = None,
    ) -> dict:
        del __event_emitter__
        action_id = (__id__ or "feedback").strip() or "feedback"
        if action_id != "feedback":
            return _message(f"Unsupported Tonglingyu feedback action {action_id!r}.")

        model = _request_model(body, __model__)
        target_models = _target_models(self.valves.TARGET_MODELS, self.valves.TARGET_MODEL)
        if model and model not in target_models:
            return _message(f"Tonglingyu feedback action is not enabled for model {model!r}.")

        gateway_key = str(self.valves.GATEWAY_API_KEY or "").strip()
        if not gateway_key:
            return _message("Tonglingyu Gateway key is not configured.")

        try:
            package_id = _resolve_optional_identifier(
                body,
                "package_id",
                r"\bpackage[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
            )
            trace_id = _resolve_optional_identifier(
                body,
                "trace_id",
                r"\btrace[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
            )
            if not package_id and not trace_id:
                package_id = await _prompt_text(__event_call__, "Package ID", "package_id")
            feedback_text = str(
                _deep_get(body, "feedback_text") or _deep_get(body, "feedback") or ""
            ).strip()
            if not feedback_text:
                feedback_text = await _prompt_text(
                    __event_call__,
                    "Feedback",
                    "Describe what needs expert review.",
                    multiline=True,
                )
            feedback_type = str(_deep_get(body, "feedback_type") or "other").strip() or "other"
            payload = {
                "feedback_text": feedback_text,
                "feedback_type": feedback_type,
            }
            if package_id:
                payload["package_id"] = package_id
            if trace_id:
                payload["trace_id"] = trace_id
            result = await _gateway_post_json(
                self.valves.GATEWAY_BASE_URL,
                gateway_key,
                "/v1/feedback",
                payload,
                self.valves.REQUEST_TIMEOUT_SECONDS,
                _user_subject(__user__),
            )
            return _json_message(
                "Tonglingyu feedback queued",
                result,
                self.valves.RESPONSE_MAX_CHARS,
            )
        except FeedbackActionError as error:
            return _message(str(error))
        except Exception as error:
            return _message(f"Tonglingyu feedback request failed: {error}")


async def _gateway_post_json(
    base_url: str,
    api_key: str,
    path: str,
    payload: dict,
    timeout: int,
    subject: str,
) -> Any:
    url = f"{base_url.rstrip('/')}{path}"
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        method="POST",
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
            "X-tonglingyu-subject": subject,
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise FeedbackActionError(
            f"Gateway feedback request failed with HTTP {error.code}: {detail}"
        ) from error


def _request_model(body: dict, model: Optional[Any]) -> str:
    if isinstance(model, str):
        return model
    if isinstance(model, dict):
        return str(model.get("id") or model.get("name") or "")
    return str(body.get("model") or "")


def _target_models(target_models: str, fallback: str) -> set[str]:
    values = [item.strip() for item in str(target_models or fallback).split(",")]
    return {value for value in values if value}


def _user_subject(user: Optional[Any]) -> str:
    if isinstance(user, dict):
        return str(user.get("id") or user.get("email") or "open-webui")
    return "open-webui"


def _resolve_optional_identifier(body: dict, key: str, pattern: str) -> str:
    value = _deep_get(body, key)
    if value:
        return str(value).strip()
    text = _body_text(body)
    match = re.search(pattern, text, flags=re.IGNORECASE)
    if match:
        return match.group(1).strip()
    return ""


async def _prompt_text(
    event_call: Optional[Any],
    title: str,
    placeholder: str,
    multiline: bool = False,
) -> str:
    if not event_call:
        raise FeedbackActionError(f"{title} is required.")
    result = event_call(
        {
            "type": "input",
            "data": {
                "title": title,
                "placeholder": placeholder,
                "multiline": multiline,
            },
        }
    )
    if asyncio.iscoroutine(result):
        result = await result
    value = str(result or "").strip()
    if not value:
        raise FeedbackActionError(f"{title} is required.")
    return value


def _deep_get(value: Any, key: str) -> Any:
    if not isinstance(value, dict):
        return None
    if key in value:
        return value[key]
    for nested_name in ("metadata", "extra", "data"):
        nested = value.get(nested_name)
        if isinstance(nested, dict) and key in nested:
            return nested[key]
    return None


def _body_text(body: dict) -> str:
    chunks: list[str] = []
    for message in body.get("messages") or []:
        content = message.get("content") if isinstance(message, dict) else None
        if isinstance(content, str):
            chunks.append(content)
        elif isinstance(content, list):
            for part in content:
                if isinstance(part, dict) and isinstance(part.get("text"), str):
                    chunks.append(part["text"])
    return "\n".join(chunks)


def _json_message(title: str, value: Any, max_chars: int) -> dict:
    data = json.dumps(value, ensure_ascii=False, indent=2)
    if len(data) > max_chars:
        data = f"{data[:max_chars]}\n... truncated ..."
    return _message(f"{title}\n\n```json\n{data}\n```")


def _message(content: str) -> dict:
    return {"type": "message", "content": content}
