"""
title: Tonglingyu Gateway Admin
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


class GatewayAdminError(Exception):
    pass


class Action:
    actions = [
        {"id": "metrics", "name": "Gateway metrics"},
        {"id": "trace", "name": "Gateway trace"},
        {"id": "package", "name": "Evidence package audit"},
        {"id": "session", "name": "Gateway session"},
        {"id": "retrieval_failures", "name": "RQA retrieval failures"},
        {"id": "retrieval_failure", "name": "RQA retrieval failure"},
        {"id": "retrieval_failure_update", "name": "Update RQA failure status"},
    ]

    class Valves(BaseModel):
        GATEWAY_BASE_URL: str = Field(default="http://tonglingyu-gateway:8090")
        GATEWAY_ADMIN_API_KEY: str = Field(default="")
        TARGET_MODEL: str = Field(default="tonglingyu")
        TARGET_MODELS: str = Field(default="tonglingyu")
        REQUEST_TIMEOUT_SECONDS: int = Field(default=15)
        RESPONSE_MAX_CHARS: int = Field(default=6000)
        priority: int = Field(default=20)

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
        if _user_role(__user__) != "admin":
            return _message("Tonglingyu Gateway admin access requires Open WebUI admin role.")

        model = _request_model(body, __model__)
        target_models = _target_models(self.valves.TARGET_MODELS, self.valves.TARGET_MODEL)
        if model and model not in target_models:
            return _message(f"Tonglingyu Gateway admin action is not enabled for model {model!r}.")

        admin_key = str(self.valves.GATEWAY_ADMIN_API_KEY or "").strip()
        if not admin_key:
            return _message("Tonglingyu Gateway admin key is not configured.")

        action_id = (__id__ or "metrics").strip() or "metrics"
        try:
            if action_id == "metrics":
                result = await _gateway_get(
                    self.valves.GATEWAY_BASE_URL,
                    admin_key,
                    "/v1/admin/metrics",
                    self.valves.REQUEST_TIMEOUT_SECONDS,
                )
                return _json_message("Gateway metrics", result, self.valves.RESPONSE_MAX_CHARS)
            if action_id == "trace":
                trace_id = await _resolve_identifier(
                    body,
                    __event_call__,
                    "trace_id",
                    "Trace ID",
                    r"\btrace[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
                )
                return await self._lookup_json(
                    "Gateway trace",
                    f"/v1/admin/traces/{urllib.parse.quote(trace_id, safe='')}",
                )
            if action_id == "package":
                package_id = await _resolve_identifier(
                    body,
                    __event_call__,
                    "package_id",
                    "Package ID",
                    r"\bpackage[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
                )
                return await self._lookup_json(
                    "Evidence package audit",
                    f"/v1/admin/packages/{urllib.parse.quote(package_id, safe='')}",
                )
            if action_id == "session":
                session_id = await _resolve_identifier(
                    body,
                    __event_call__,
                    "session_id",
                    "Session ID",
                    r"\bsession[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
                )
                return await self._lookup_json(
                    "Gateway session",
                    f"/v1/admin/sessions/{urllib.parse.quote(session_id, safe='')}",
                )
            if action_id == "retrieval_failures":
                return await self._lookup_json(
                    "RQA retrieval failures",
                    f"/v1/admin/retrieval-failures{_retrieval_failure_query(body)}",
                )
            if action_id == "retrieval_failure":
                failure_id = await _resolve_identifier(
                    body,
                    __event_call__,
                    "failure_id",
                    "Failure ID",
                    r"\bfailure[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
                )
                return await self._lookup_json(
                    "RQA retrieval failure",
                    f"/v1/admin/retrieval-failures/{urllib.parse.quote(failure_id, safe='')}",
                )
            if action_id == "retrieval_failure_update":
                failure_id = await _resolve_identifier(
                    body,
                    __event_call__,
                    "failure_id",
                    "Failure ID",
                    r"\bfailure[_ -]?id\b\s*[:=]\s*([A-Za-z0-9_.:-]+)",
                )
                status = str(
                    _deep_get(body, "human_review_status")
                    or _deep_get(body, "status")
                    or ""
                ).strip()
                if not status:
                    raise GatewayAdminError("Human review status is required.")
                result = await _gateway_patch_json(
                    self.valves.GATEWAY_BASE_URL,
                    admin_key,
                    f"/v1/admin/retrieval-failures/{urllib.parse.quote(failure_id, safe='')}",
                    {
                        "human_review_status": status,
                        "reviewer": _deep_get(body, "reviewer"),
                        "review_note": _deep_get(body, "review_note"),
                        "if_match_updated_at": _deep_get(body, "if_match_updated_at"),
                    },
                    self.valves.REQUEST_TIMEOUT_SECONDS,
                )
                return _json_message(
                    "RQA retrieval failure update",
                    result,
                    self.valves.RESPONSE_MAX_CHARS,
                )
        except GatewayAdminError as error:
            await _emit_status(__event_emitter__, "error", str(error))
            return _message(str(error))

        return _message(f"Unsupported Tonglingyu Gateway admin action: {action_id}")

    async def _lookup_json(self, title: str, path: str) -> dict:
        result = await _gateway_get(
            self.valves.GATEWAY_BASE_URL,
            str(self.valves.GATEWAY_ADMIN_API_KEY or "").strip(),
            path,
            self.valves.REQUEST_TIMEOUT_SECONDS,
        )
        return _json_message(title, result, self.valves.RESPONSE_MAX_CHARS)


def _message(content: str) -> dict:
    return {"content": content}


def _json_message(title: str, payload: Any, max_chars: int) -> dict:
    content = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True)
    if len(content) > max_chars:
        content = (
            content[: max(0, max_chars - 80)]
            + "\n... truncated by Open WebUI action ..."
        )
    return {"content": f"### {title}\n\n```json\n{content}\n```"}


async def _gateway_get(base_url: str, admin_key: str, path: str, timeout_seconds: int) -> Any:
    return await asyncio.to_thread(
        _gateway_get_blocking,
        base_url,
        admin_key,
        path,
        timeout_seconds,
    )


async def _gateway_patch_json(
    base_url: str,
    admin_key: str,
    path: str,
    payload: dict,
    timeout_seconds: int,
) -> Any:
    return await asyncio.to_thread(
        _gateway_json_blocking,
        base_url,
        admin_key,
        path,
        "PATCH",
        payload,
        timeout_seconds,
    )


def _gateway_get_blocking(base_url: str, admin_key: str, path: str, timeout_seconds: int) -> Any:
    return _gateway_json_blocking(base_url, admin_key, path, "GET", None, timeout_seconds)


def _gateway_json_blocking(
    base_url: str,
    admin_key: str,
    path: str,
    method: str,
    payload: Optional[dict],
    timeout_seconds: int,
) -> Any:
    if not str(base_url or "").strip():
        raise GatewayAdminError("Tonglingyu Gateway base URL is not configured.")
    url = f"{str(base_url).rstrip('/')}{path}"
    data = None
    headers = {"Authorization": f"Bearer {admin_key}"}
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(
        url,
        data=data,
        headers=headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=max(1, int(timeout_seconds))) as response:
            raw = response.read().decode("utf-8", errors="replace")
            if not raw:
                return {"status": response.status}
            try:
                return json.loads(raw)
            except json.JSONDecodeError:
                return {"status": response.status, "raw": raw}
    except urllib.error.HTTPError as error:
        raise GatewayAdminError(
            f"Tonglingyu Gateway admin request failed: HTTP {error.code}"
        ) from None
    except (TimeoutError, OSError, urllib.error.URLError):
        raise GatewayAdminError("Tonglingyu Gateway admin request failed: network error") from None


def _retrieval_failure_query(body: dict) -> str:
    params = {}
    for key in ("human_review_status", "status", "failure_type", "limit", "offset"):
        value = _deep_get(body, key)
        if value is not None and str(value).strip():
            params[key] = str(value).strip()
    if not params:
        return ""
    return "?" + urllib.parse.urlencode(params)


async def _resolve_identifier(
    body: dict,
    event_call: Optional[Any],
    key: str,
    title: str,
    pattern: str,
) -> str:
    value = _extract_identifier(body, key, pattern)
    if not value and event_call:
        value = await _prompt_identifier(event_call, title)
    value = str(value or "").strip()
    if not value:
        raise GatewayAdminError(f"{title} is required.")
    return value


async def _prompt_identifier(event_call: Any, title: str) -> str:
    prompt = {
        "type": "input",
        "data": {
            "title": title,
            "message": f"Enter {title}",
            "placeholder": title,
        },
    }
    value = event_call(prompt)
    if hasattr(value, "__await__"):
        value = await value
    if isinstance(value, dict):
        value = value.get("value") or value.get("content") or value.get("text") or ""
    return str(value or "").strip()


def _extract_identifier(body: dict, key: str, pattern: str) -> str:
    direct = _deep_get(body, key)
    if direct:
        return str(direct).strip()
    content = "\n".join(_message_texts(body))
    match = re.search(pattern, content, flags=re.IGNORECASE)
    if match:
        return match.group(1).strip()
    return ""


def _deep_get(value: Any, key: str) -> Any:
    if isinstance(value, dict):
        if value.get(key):
            return value.get(key)
        for child in value.values():
            found = _deep_get(child, key)
            if found:
                return found
    if isinstance(value, list):
        for child in value:
            found = _deep_get(child, key)
            if found:
                return found
    return None


def _message_texts(body: Any) -> list[str]:
    texts: list[str] = []
    if isinstance(body, dict):
        for key in ("content", "text", "message"):
            value = body.get(key)
            if isinstance(value, str):
                texts.append(value)
        for value in body.values():
            texts.extend(_message_texts(value))
    elif isinstance(body, list):
        for item in body:
            texts.extend(_message_texts(item))
    return texts


async def _emit_status(event_emitter: Optional[Any], status: str, description: str) -> None:
    if not event_emitter:
        return
    event = {
        "type": "status",
        "data": {"status": status, "description": description, "done": True},
    }
    result = event_emitter(event)
    if hasattr(result, "__await__"):
        await result


def _request_model(body: dict, model: Optional[Any]) -> str:
    return str(
        _get(body, "model")
        or _get(_get(body, "metadata"), "model")
        or _get(model, "id")
        or ""
    ).strip()


def _user_role(user: Optional[Any]) -> str:
    return str(_get(user, "role") or "user").strip().lower() or "user"


def _get(value: Any, key: str) -> Any:
    if value is None:
        return None
    if isinstance(value, dict):
        return value.get(key)
    return getattr(value, key, None)


def _target_models(target_models: str, target_model: str) -> set[str]:
    models: set[str] = set()
    for value in (target_models, target_model):
        for item in str(value or "").split(","):
            item = item.strip()
            if item:
                models.add(item)
    return models
