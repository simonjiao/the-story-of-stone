"""
title: Tonglingyu Products
author: Tonglingyu
version: 1.0.0
required_open_webui_version: 0.6.0
"""

import asyncio
import json
import re
import urllib.error
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


class ProductActionError(Exception):
    pass


class Action:
    actions = [
        {"id": "start_writing", "name": "开始写作"},
        {"id": "view_product_run", "name": "查看产品任务"},
        {"id": "confirm_product_action", "name": "确认当前步骤"},
        {"id": "reject_product_action", "name": "拒绝当前步骤"},
        {"id": "cancel_product_run", "name": "取消产品任务"},
        {"id": "open_product_artifact", "name": "打开产物"},
        {"id": "start_illustrated_book", "name": "制作图文书"},
    ]

    class Valves(BaseModel):
        GATEWAY_BASE_URL: str = Field(default="http://tonglingyu-gateway:8090")
        GATEWAY_API_KEY: str = Field(default="")
        TARGET_MODEL: str = Field(default="tonglingyu")
        REQUEST_TIMEOUT_SECONDS: int = Field(default=30)
        priority: int = Field(default=5)

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
        del __model__
        action_id = (__id__ or "view_product_run").strip()
        key = str(self.valves.GATEWAY_API_KEY or "").strip()
        if not key:
            return _message("Tonglingyu Gateway key is not configured.")
        if action_id == "start_illustrated_book":
            return _message("图文书产品尚未在 Studio capabilities 中发布，当前不会降级为普通对话。")
        try:
            await _emit_status(__event_emitter__, "in_progress", "正在处理产品操作")
            if action_id == "start_writing":
                result = await self._start_writing(body, __user__, __event_call__)
            elif action_id == "view_product_run":
                run_id = await _resolve_id(body, "run_id", "Run ID", __event_call__)
                result = await _gateway_json(self.valves, key, "GET", f"/v1/runs/{run_id}", None, __user__)
            elif action_id in {"confirm_product_action", "reject_product_action"}:
                run_id = await _resolve_id(body, "run_id", "Run ID", __event_call__)
                remote_action_id = await _resolve_id(body, "action_id", "Action ID", __event_call__)
                decision = "accept" if action_id == "confirm_product_action" else "reject"
                result = await _gateway_json(
                    self.valves, key, "POST", f"/v1/runs/{run_id}/actions/{remote_action_id}",
                    {"decision": decision, "payload": {}, "idempotency_key": f"{remote_action_id}:{decision}"}, __user__,
                )
            elif action_id == "cancel_product_run":
                run_id = await _resolve_id(body, "run_id", "Run ID", __event_call__)
                result = await _gateway_json(self.valves, key, "POST", f"/v1/runs/{run_id}/cancel", {}, __user__)
            elif action_id == "open_product_artifact":
                run_id = await _resolve_id(body, "run_id", "Run ID", __event_call__)
                artifact_id = await _resolve_id(body, "artifact_id", "Artifact ID", __event_call__)
                result = await _gateway_json(self.valves, key, "POST", f"/v1/runs/{run_id}/artifacts/{artifact_id}/open", {}, __user__)
            else:
                raise ProductActionError(f"Unsupported product action: {action_id}")
            await _emit_status(__event_emitter__, "complete", "产品操作已提交")
            return _result_message(action_id, result)
        except ProductActionError as error:
            await _emit_status(__event_emitter__, "error", str(error))
            return _message(str(error))
        except Exception as error:
            await _emit_status(__event_emitter__, "error", "产品操作失败")
            return _message(f"Product action failed: {error}")

    async def _start_writing(self, body: dict, user: Optional[Any], event_call: Optional[Any]) -> Any:
        chat_id = _first_non_empty(_deep_get(body, "chat_id"), body.get("chat_id"))
        assistant_message_id = _first_non_empty(
            body.get("id"), _deep_get(body, "assistant_message_id"), _deep_get(body, "message_id")
        )
        if not chat_id:
            raise ProductActionError("Open WebUI chat id is required to start a writing product.")
        if not assistant_message_id:
            raise ProductActionError("The selected assistant message id is required for automatic updates.")
        prompt = _first_non_empty(_deep_get(body, "writing_prompt"), _deep_get(body, "product_prompt"))
        if not prompt:
            prompt = await _prompt_text(event_call, "写作要求", "例如：写一篇关于晴雯的人物小传")
        headers = {
            "X-Tonglingyu-Product-Id": "writing-assistant",
            "X-Tonglingyu-Chat-Id": chat_id,
            "X-Tonglingyu-Message-Id": assistant_message_id,
        }
        return await _gateway_json(
            self.valves,
            str(self.valves.GATEWAY_API_KEY),
            "POST",
            "/v1/runs",
            {
                "model": self.valves.TARGET_MODEL,
                "input": prompt,
                "background": True,
                "idempotency_key": assistant_message_id,
            },
            user,
            headers,
        )


async def _gateway_json(
    valves: Action.Valves,
    api_key: str,
    method: str,
    path: str,
    payload: Optional[dict],
    user: Optional[Any],
    extra_headers: Optional[dict[str, str]] = None,
) -> Any:
    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
        "X-Tonglingyu-Subject": _user_subject(user),
    }
    user_id = _user_id(user)
    if user_id:
        headers["X-Tonglingyu-User-Id"] = user_id
    headers.update(extra_headers or {})
    request = urllib.request.Request(
        f"{str(valves.GATEWAY_BASE_URL).rstrip('/')}{path}",
        data=json.dumps(payload).encode("utf-8") if payload is not None else None,
        method=method,
        headers=headers,
    )
    try:
        with urllib.request.urlopen(request, timeout=int(valves.REQUEST_TIMEOUT_SECONDS or 30)) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise ProductActionError(f"Gateway returned HTTP {error.code}: {detail}") from error


async def _resolve_id(body: dict, key: str, title: str, event_call: Optional[Any]) -> str:
    value = _first_non_empty(_deep_get(body, key), _extract_id(_body_text(body), key))
    return value or await _prompt_text(event_call, title, key)


def _extract_id(text: str, key: str) -> str:
    labels = {"run_id": "Run ID", "action_id": "Action ID", "artifact_id": "Artifact ID"}
    match = re.search(rf"(?:{re.escape(labels.get(key, key))}|{re.escape(key)})\s*[:=]\s*([A-Za-z0-9_.:-]+)", text, re.IGNORECASE)
    return match.group(1) if match else ""


async def _prompt_text(event_call: Optional[Any], title: str, placeholder: str) -> str:
    if not event_call:
        raise ProductActionError(f"{title} is required.")
    result = event_call({"type": "input", "data": {"title": title, "placeholder": placeholder}})
    if asyncio.iscoroutine(result):
        result = await result
    value = str(result or "").strip()
    if not value:
        raise ProductActionError(f"{title} is required.")
    return value


async def _emit_status(emitter: Optional[Any], status: str, description: str) -> None:
    if not emitter:
        return
    result = emitter({"type": "status", "data": {"status": status, "description": description, "done": status in {"complete", "error"}}})
    if asyncio.iscoroutine(result):
        await result


def _result_message(action_id: str, result: Any) -> dict:
    if action_id == "start_writing" and isinstance(result, dict):
        return _message(f"写作任务已启动。\n\nRun ID: {result.get('id') or result.get('run_id') or 'unknown'}\n\n任务完成后会自动更新当前助手消息。")
    if action_id == "open_product_artifact" and isinstance(result, dict) and result.get("url"):
        return _message(f"[打开 Studio 产物]({result['url']})")
    return _message(json.dumps(result, ensure_ascii=False, indent=2)[:4000])


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
    values = [str(body.get("content") or "")]
    for message in body.get("messages") or []:
        if isinstance(message, dict):
            values.append(str(message.get("content") or ""))
    return "\n".join(values)


def _first_non_empty(*values: Any) -> str:
    for value in values:
        text = str(value or "").strip()
        if text:
            return text
    return ""


def _user_id(user: Optional[Any]) -> str:
    return _first_non_empty(user.get("id") if isinstance(user, dict) else None)


def _user_subject(user: Optional[Any]) -> str:
    if isinstance(user, dict):
        return _first_non_empty(user.get("id"), user.get("email"), "open-webui")
    return "open-webui"


def _message(content: str) -> dict:
    return {"type": "message", "content": content}
