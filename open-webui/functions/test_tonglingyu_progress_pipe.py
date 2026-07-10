import asyncio
import json
import pathlib
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from tonglingyu_progress_pipe import Pipe, _gateway_headers, _gateway_payload


class FakeStreamResponse:
    def __init__(self, lines: list[str]) -> None:
        self.lines = lines

    async def __aenter__(self) -> "FakeStreamResponse":
        return self

    async def __aexit__(self, *args: object) -> None:
        return None

    def raise_for_status(self) -> None:
        return None

    async def aiter_lines(self):
        for line in self.lines:
            yield line


class FakeAsyncClient:
    last_request = None

    def __init__(self, *args, **kwargs) -> None:
        self.args = args
        self.kwargs = kwargs

    async def __aenter__(self) -> "FakeAsyncClient":
        return self

    async def __aexit__(self, *args: object) -> None:
        return None

    def stream(self, method: str, url: str, headers: dict, json: dict):
        FakeAsyncClient.last_request = {
            "method": method,
            "url": url,
            "headers": headers,
            "json": json,
        }
        chunks = [
            {
                "choices": [{"delta": {}, "finish_reason": None}],
                "tonglingyu_event": {
                    "type": "response.status",
                    "payload": {"status": "in_progress"},
                },
            },
            {
                "choices": [{"delta": {}, "finish_reason": None}],
                "tonglingyu_event": {
                    "type": "evidence.searching",
                    "payload": {},
                },
            },
            {
                "choices": [{"delta": {}, "finish_reason": None}],
                "tonglingyu_event": {
                    "type": "evidence.found",
                    "payload": {"evidence_count": 14},
                },
            },
            {
                "choices": [{"delta": {"content": "最终"}, "finish_reason": None}],
            },
            {
                "choices": [{"delta": {"content": "答案"}, "finish_reason": None}],
                "tonglingyu_event": {
                    "type": "response.completed",
                    "payload": {},
                },
            },
        ]
        lines = [f"data: {json_module.dumps(chunk, ensure_ascii=False)}" for chunk in chunks]
        lines.append("data: [DONE]")
        return FakeStreamResponse(lines)


class FakeJsonResponse:
    def raise_for_status(self) -> None:
        return None

    def json(self) -> dict:
        return {"id": "run-slash-1", "status": "queued"}


class FakeProductAsyncClient:
    last_request = None

    def __init__(self, *args, **kwargs) -> None:
        pass

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        return None

    async def request(self, method: str, url: str, headers: dict, json: dict):
        FakeProductAsyncClient.last_request = {"method": method, "url": url, "headers": headers, "json": json}
        return FakeJsonResponse()


json_module = json


class TonglingyuProgressPipeTest(unittest.TestCase):
    def test_writing_slash_command_creates_background_product_run_for_assistant_placeholder(self) -> None:
        pipe = Pipe()
        pipe.valves.GATEWAY_API_KEY = "gateway-key"
        with patch("tonglingyu_progress_pipe.httpx.AsyncClient", FakeProductAsyncClient):
            answer = asyncio.run(pipe.pipe(
                {"messages": [{"role": "user", "content": "/写作 写一篇晴雯小传"}]},
                __user__={"id": "user-1"},
                __metadata__={"chat_id": "chat-1", "message_id": "assistant-placeholder-1", "user_message_id": "user-message-1"},
            ))
        request = FakeProductAsyncClient.last_request
        self.assertEqual(request["url"], "http://tonglingyu-gateway:8090/v1/runs")
        self.assertEqual(request["headers"]["X-Tonglingyu-Product-Id"], "writing-assistant")
        self.assertEqual(request["headers"]["X-Tonglingyu-Message-Id"], "assistant-placeholder-1")
        self.assertEqual(request["json"]["input"], "写一篇晴雯小传")
        self.assertTrue(request["json"]["background"])
        self.assertIn("Run ID: run-slash-1", answer)

    def test_knowledge_slash_command_is_stripped_before_gateway_stream(self) -> None:
        payload = _gateway_payload(
            {"messages": [{"role": "user", "content": "/问答 晴雯是第几回死的"}]},
            "tonglingyu",
            None,
        )
        self.assertEqual(payload["messages"][-1]["content"], "晴雯是第几回死的")

    def test_gateway_payload_forces_upstream_model_and_safe_metadata(self) -> None:
        payload = _gateway_payload(
            {
                "model": "tonglingyu_progress",
                "messages": [{"role": "user", "content": "晴雯是第几回死的"}],
                "metadata": {
                    "chat_id": "chat-1",
                    "trace_id": "forbidden",
                    "message_id": "msg-1",
                },
                "tools": [],
            },
            "tonglingyu",
            {"user_id": "user-1"},
        )

        self.assertEqual(payload["model"], "tonglingyu")
        self.assertTrue(payload["stream"])
        self.assertEqual(payload["metadata"]["chat_id"], "chat-1")
        self.assertEqual(payload["metadata"]["message_id"], "msg-1")
        self.assertEqual(payload["metadata"]["user_id"], "user-1")
        self.assertNotIn("trace_id", payload["metadata"])
        self.assertNotIn("tools", payload)

    def test_gateway_headers_include_identity_and_api_key(self) -> None:
        headers = _gateway_headers(
            {"metadata": {"chat_id": "chat-1", "message_id": "msg-1"}},
            "gateway-key",
            {"id": "user-1"},
            None,
        )

        self.assertEqual(headers["Authorization"], "Bearer gateway-key")
        self.assertEqual(headers["X-Tonglingyu-User-Id"], "user-1")
        self.assertEqual(headers["X-Tonglingyu-Chat-Id"], "chat-1")
        self.assertEqual(headers["X-Tonglingyu-Message-Id"], "msg-1")

    def test_pipe_emits_replace_embeds_and_returns_only_final_answer(self) -> None:
        pipe = Pipe()
        pipe.valves.GATEWAY_API_KEY = "gateway-key"
        pipe.valves.MIN_EMBED_INTERVAL_MS = 0
        events = []

        async def event_emitter(event: dict) -> None:
            events.append(event)

        with patch("tonglingyu_progress_pipe.httpx.AsyncClient", FakeAsyncClient):
            answer = asyncio.run(
                pipe.pipe(
                    {
                        "model": "tonglingyu_progress",
                        "messages": [{"role": "user", "content": "晴雯是第几回死的"}],
                        "metadata": {
                            "chat_id": "chat-1",
                            "message_id": "msg-1",
                        },
                    },
                    __event_emitter__=event_emitter,
                    __user__={"id": "user-1"},
                )
            )

        self.assertEqual(answer, "最终答案")
        self.assertEqual(
            FakeAsyncClient.last_request["url"],
            "http://tonglingyu-gateway:8090/v1/chat/completions",
        )
        self.assertEqual(FakeAsyncClient.last_request["json"]["model"], "tonglingyu")
        self.assertGreaterEqual(len(events), 3)
        self.assertEqual(events[0]["type"], "embeds")
        self.assertTrue(events[0]["data"]["replace"])
        self.assertIn("已收到问题", events[0]["data"]["embeds"][0])
        self.assertTrue(
            any("已找到 14 条候选证据" in event["data"]["embeds"][0] for event in events[:-1])
        )
        self.assertEqual(events[-1], {"type": "embeds", "data": {"embeds": [], "replace": True}})


if __name__ == "__main__":
    unittest.main()
