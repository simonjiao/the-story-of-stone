import asyncio
import json
import pathlib
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from tonglingyu_product_action import Action


class FakeResponse:
    def __init__(self, body: dict) -> None:
        self.body = json.dumps(body).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return None

    def read(self) -> bytes:
        return self.body


class TonglingyuProductActionTest(unittest.TestCase):
    def configured(self) -> Action:
        action = Action()
        action.valves.GATEWAY_API_KEY = "gateway-key"
        return action

    def test_start_writing_uses_selected_assistant_message_for_persistent_delivery(self) -> None:
        action = self.configured()
        with patch("tonglingyu_product_action.urllib.request.urlopen", return_value=FakeResponse({"id": "run-1"})) as urlopen:
            result = asyncio.run(action.action(
                {"id": "assistant-1", "chat_id": "chat-1", "writing_prompt": "写晴雯", "user_message_id": "user-message-1"},
                __user__={"id": "user-1"}, __id__="start_writing",
            ))
        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_header("X-tonglingyu-product-id"), "writing-assistant")
        self.assertEqual(request.get_header("X-tonglingyu-chat-id"), "chat-1")
        self.assertEqual(request.get_header("X-tonglingyu-message-id"), "assistant-1")
        self.assertEqual(request.get_header("X-tonglingyu-user-id"), "user-1")
        self.assertTrue(payload["background"])
        self.assertEqual(payload["idempotency_key"], "assistant-1")
        self.assertIn("Run ID: run-1", result["content"])

    def test_confirm_action_posts_structured_decision(self) -> None:
        action = self.configured()
        with patch("tonglingyu_product_action.urllib.request.urlopen", return_value=FakeResponse({"status": "in_progress"})) as urlopen:
            asyncio.run(action.action(
                {"content": "Run ID: run-1\nAction ID: action-1"},
                __user__={"id": "user-1"}, __id__="confirm_product_action",
            ))
        request = urlopen.call_args.args[0]
        self.assertTrue(request.full_url.endswith("/v1/runs/run-1/actions/action-1"))
        self.assertEqual(json.loads(request.data)["decision"], "accept")

    def test_start_refuses_missing_assistant_message_id(self) -> None:
        action = self.configured()
        with patch("tonglingyu_product_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(action.action(
                {"chat_id": "chat-1", "writing_prompt": "写晴雯"},
                __user__={"id": "user-1"}, __id__="start_writing",
            ))
        self.assertIn("assistant message id", result["content"])
        urlopen.assert_not_called()

    def test_illustrated_book_is_explicitly_unavailable(self) -> None:
        action = self.configured()
        with patch("tonglingyu_product_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(action.action({}, __id__="start_illustrated_book"))
        self.assertIn("尚未", result["content"])
        urlopen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
