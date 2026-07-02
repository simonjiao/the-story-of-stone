"""
title: Tonglingyu Progress Pipe
author: Tonglingyu
version: 1.0.0
required_open_webui_version: 0.6.0
"""

import html
import json
import time
from typing import Any, AsyncIterator, Optional

import httpx

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


SAFE_METADATA_KEYS = {
    "user_id",
    "user",
    "username",
    "chat_id",
    "conversation_id",
    "session_id",
    "user_message_id",
    "message_id",
    "request_id",
}


class Pipe:
    class Valves(BaseModel):
        GATEWAY_BASE_URL: str = Field(default="http://tonglingyu-gateway:8090")
        GATEWAY_API_KEY: str = Field(default="")
        UPSTREAM_MODEL: str = Field(default="tonglingyu")
        REQUEST_TIMEOUT_SECONDS: int = Field(default=180)
        CONNECT_TIMEOUT_SECONDS: int = Field(default=5)
        MAX_PROGRESS_LINES: int = Field(default=8)
        EMBED_TITLE: str = Field(default="通灵玉正在处理")
        CLEAR_EMBEDS_ON_DONE: bool = Field(default=True)
        MIN_EMBED_INTERVAL_MS: int = Field(default=150)

    def __init__(self) -> None:
        self.valves = self.Valves()

    async def pipe(
        self,
        body: dict,
        __event_emitter__: Optional[Any] = None,
        __user__: Optional[Any] = None,
        __metadata__: Optional[dict] = None,
        __model__: Optional[Any] = None,
    ) -> str:
        del __model__
        state = ProgressState(
            title=self.valves.EMBED_TITLE,
            max_lines=max(1, int(self.valves.MAX_PROGRESS_LINES or 8)),
            lines=["已收到问题"],
        )
        await _emit_progress_embed(__event_emitter__, state)

        final_answer: list[str] = []
        last_embed_emit = 0.0
        payload = _gateway_payload(body, self.valves.UPSTREAM_MODEL, __metadata__)
        headers = _gateway_headers(body, self.valves.GATEWAY_API_KEY, __user__, __metadata__)
        url = self.valves.GATEWAY_BASE_URL.rstrip("/") + "/v1/chat/completions"
        timeout = httpx.Timeout(
            float(self.valves.REQUEST_TIMEOUT_SECONDS or 180),
            connect=float(self.valves.CONNECT_TIMEOUT_SECONDS or 5),
        )

        try:
            async with httpx.AsyncClient(timeout=timeout) as client:
                async with client.stream(
                    "POST",
                    url,
                    headers=headers,
                    json=payload,
                ) as response:
                    response.raise_for_status()
                    async for chunk in _iter_openai_sse_chunks(response):
                        event = chunk.get("tonglingyu_event")
                        if isinstance(event, dict):
                            changed = state.apply_event(event)
                            now = time.monotonic()
                            elapsed_ms = (now - last_embed_emit) * 1000.0
                            if changed and elapsed_ms >= self.valves.MIN_EMBED_INTERVAL_MS:
                                await _emit_progress_embed(__event_emitter__, state)
                                last_embed_emit = now

                        delta = _content_delta(chunk)
                        if delta:
                            final_answer.append(delta)
        except Exception as error:
            state.phase = "failed"
            state.add_line(f"处理失败：{error}")
            await _emit_progress_embed(__event_emitter__, state)
            raise
        finally:
            if self.valves.CLEAR_EMBEDS_ON_DONE:
                await _clear_progress_embed(__event_emitter__)

        answer = "".join(final_answer).strip()
        if not answer:
            raise RuntimeError("gateway stream completed without final answer content")
        return answer


class ProgressState:
    def __init__(self, title: str, max_lines: int, lines: Optional[list[str]] = None) -> None:
        self.title = title
        self.max_lines = max_lines
        self.phase = "starting"
        self.lines = list(lines or [])

    def add_line(self, line: str) -> bool:
        line = str(line or "").strip()
        if not line:
            return False
        if self.lines and self.lines[-1] == line:
            return False
        self.lines.append(line)
        if len(self.lines) > self.max_lines:
            self.lines = self.lines[-self.max_lines :]
        return True

    def apply_event(self, event: dict) -> bool:
        event_type = str(event.get("type") or "")
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        line: Optional[str] = None

        if event_type == "response.status":
            status = str(payload.get("status") or "in_progress")
            self.phase = "normalizing" if status == "in_progress" else status
            line = "正在解析问题和准备检索"
        elif event_type == "evidence.searching":
            self.phase = "retrieving"
            line = "正在检索证据"
        elif event_type == "evidence.found":
            self.phase = "retrieving"
            count = payload.get("evidence_count") or payload.get("count")
            line = f"已找到 {count} 条候选证据" if count else "已找到候选证据"
        elif event_type == "review.started":
            self.phase = "reviewing"
            line = "正在复核答案"
        elif event_type == "review.completed":
            self.phase = "reviewed"
            status = payload.get("status") or payload.get("review_status")
            line = f"答案复核完成：{status}" if status else "答案复核完成"
        elif event_type == "response.completed":
            self.phase = "completed"
            line = "回答生成完成"
        elif event_type == "response.failed":
            self.phase = "failed"
            line = "回答生成失败"
        elif event_type == "response.canceled":
            self.phase = "canceled"
            line = "回答已取消"

        return self.add_line(line or "")


def _gateway_payload(body: dict, upstream_model: str, metadata: Optional[dict]) -> dict:
    messages = body.get("messages") or []
    safe_metadata = _safe_metadata(body.get("metadata"))
    safe_metadata.update(_safe_metadata(metadata))
    return {
        "model": upstream_model,
        "messages": messages,
        "stream": True,
        "metadata": safe_metadata,
    }


def _gateway_headers(
    body: dict,
    api_key: str,
    user: Optional[Any],
    metadata: Optional[dict],
) -> dict[str, str]:
    if not api_key:
        raise RuntimeError("GATEWAY_API_KEY is required for Tonglingyu Progress Pipe")

    merged_metadata = {}
    merged_metadata.update(_safe_metadata(body.get("metadata")))
    merged_metadata.update(_safe_metadata(metadata))
    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }
    user_id = _first_non_empty(
        _get(user, "id"),
        merged_metadata.get("user_id"),
        merged_metadata.get("user"),
        merged_metadata.get("username"),
    )
    chat_id = _first_non_empty(
        merged_metadata.get("chat_id"),
        merged_metadata.get("conversation_id"),
        merged_metadata.get("session_id"),
    )
    message_id = _first_non_empty(
        merged_metadata.get("user_message_id"),
        merged_metadata.get("message_id"),
        merged_metadata.get("request_id"),
    )
    if user_id:
        headers["X-Tonglingyu-User-Id"] = user_id
    if chat_id:
        headers["X-Tonglingyu-Chat-Id"] = chat_id
    if message_id:
        headers["X-Tonglingyu-Message-Id"] = message_id
    return headers


def _safe_metadata(value: Any) -> dict:
    if not isinstance(value, dict):
        return {}
    safe = {}
    for key in SAFE_METADATA_KEYS:
        item = value.get(key)
        if item is not None and str(item).strip():
            safe[key] = item
    return safe


async def _iter_openai_sse_chunks(response: Any) -> AsyncIterator[dict]:
    async for line in response.aiter_lines():
        line = line.strip()
        if not line or not line.startswith("data:"):
            continue
        data = line.removeprefix("data:").strip()
        if data == "[DONE]":
            break
        try:
            parsed = json.loads(data)
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict):
            yield parsed


def _content_delta(chunk: dict) -> str:
    choices = chunk.get("choices")
    if not isinstance(choices, list) or not choices:
        return ""
    first = choices[0] if isinstance(choices[0], dict) else {}
    delta = first.get("delta") if isinstance(first.get("delta"), dict) else {}
    return str(delta.get("content") or "")


async def _emit_progress_embed(event_emitter: Optional[Any], state: ProgressState) -> None:
    if not event_emitter:
        return
    result = event_emitter(
        {
            "type": "embeds",
            "data": {
                "embeds": [_render_progress_html(state)],
                "replace": True,
            },
        }
    )
    if hasattr(result, "__await__"):
        await result


async def _clear_progress_embed(event_emitter: Optional[Any]) -> None:
    if not event_emitter:
        return
    result = event_emitter(
        {
            "type": "embeds",
            "data": {
                "embeds": [],
                "replace": True,
            },
        }
    )
    if hasattr(result, "__await__"):
        await result


def _render_progress_html(state: ProgressState) -> str:
    escaped_title = html.escape(state.title)
    escaped_phase = html.escape(state.phase)
    items = "\n".join(
        f"<li>{html.escape(line)}</li>"
        for line in state.lines[-state.max_lines :]
    )
    return f"""<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <style>
    body {{
      margin: 0;
      font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      color: #1f2937;
      background: transparent;
    }}
    .box {{
      box-sizing: border-box;
      border: 1px solid #d1d5db;
      border-radius: 8px;
      background: #f9fafb;
      padding: 12px 14px;
      width: 100%;
    }}
    .title {{
      display: flex;
      justify-content: space-between;
      gap: 12px;
      font-size: 13px;
      font-weight: 650;
      margin-bottom: 8px;
    }}
    .phase {{
      color: #4b5563;
      font-weight: 500;
    }}
    ol {{
      margin: 0;
      padding-left: 20px;
      font-size: 13px;
      line-height: 1.55;
    }}
  </style>
</head>
<body>
  <div class="box">
    <div class="title">
      <span>{escaped_title}</span>
      <span class="phase">{escaped_phase}</span>
    </div>
    <ol>{items}</ol>
  </div>
  <script>
    const height = Math.max(document.body.scrollHeight, document.documentElement.scrollHeight);
    window.parent.postMessage({{ type: "resize", height }}, "*");
  </script>
</body>
</html>"""


def _first_non_empty(*values: Any) -> str:
    for value in values:
        if value is None:
            continue
        text = str(value).strip()
        if text:
            return text
    return ""


def _get(value: Any, key: str) -> Any:
    if value is None:
        return None
    if isinstance(value, dict):
        return value.get(key)
    return getattr(value, key, None)
