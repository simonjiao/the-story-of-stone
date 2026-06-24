import asyncio
import json
import pathlib
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from tonglingyu_gateway_feedback_action import Action


class FakeResponse:
    status = 200

    def __init__(self, body: str) -> None:
        self.body = body.encode("utf-8")

    def __enter__(self) -> "FakeResponse":
        return self

    def __exit__(self, *args: object) -> None:
        return None

    def read(self) -> bytes:
        return self.body


class TonglingyuGatewayFeedbackActionTest(unittest.TestCase):
    def action_with_key(self) -> Action:
        action = Action()
        action.valves.GATEWAY_API_KEY = "gateway-key"
        return action

    def test_user_feedback_posts_bounded_gateway_payload(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_feedback_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.user_feedback"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "package_id": "pkg-1",
                        "feedback_type": "missing_evidence",
                        "feedback_text": "Needs expert review.",
                        "alias": "ignored-fact-field",
                    },
                    __user__={"id": "user-1", "role": "user"},
                    __id__="feedback",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/feedback",
        )
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.get_header("Authorization"), "Bearer gateway-key")
        self.assertEqual(request.get_header("X-tonglingyu-subject"), "user-1")
        self.assertEqual(
            payload,
            {
                "feedback_text": "Needs expert review.",
                "feedback_type": "missing_evidence",
                "package_id": "pkg-1",
            },
        )
        self.assertIn("tonglingyu.user_feedback", result["content"])

    def test_trace_id_can_be_extracted_from_message_content(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_feedback_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.user_feedback"}'),
        ) as urlopen:
            asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "messages": [{"content": "trace_id: trace-1"}],
                        "feedback_text": "Please review this answer.",
                    },
                    __user__={"id": "user-1", "role": "user"},
                    __id__="feedback",
                )
            )

        payload = json.loads(urlopen.call_args.args[0].data.decode("utf-8"))
        self.assertEqual(payload["trace_id"], "trace-1")
        self.assertNotIn("package_id", payload)

    def test_prompts_for_missing_feedback_text(self) -> None:
        action = self.action_with_key()
        prompts = []

        async def event_call(payload: dict) -> str:
            prompts.append(payload)
            return "Prompted feedback text"

        with patch(
            "tonglingyu_gateway_feedback_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.user_feedback"}'),
        ) as urlopen:
            asyncio.run(
                action.action(
                    {"model": "tonglingyu", "package_id": "pkg-1"},
                    __user__={"id": "user-1", "role": "user"},
                    __event_call__=event_call,
                    __id__="feedback",
                )
            )

        self.assertEqual(prompts[0]["data"]["title"], "Feedback")
        payload = json.loads(urlopen.call_args.args[0].data.decode("utf-8"))
        self.assertEqual(payload["feedback_text"], "Prompted feedback text")

    def test_missing_gateway_key_is_denied_without_gateway_call(self) -> None:
        action = Action()
        with patch("tonglingyu_gateway_feedback_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu", "package_id": "pkg-1", "feedback_text": "x"},
                    __user__={"id": "user-1", "role": "user"},
                    __id__="feedback",
                )
            )

        self.assertIn("Gateway key is not configured", result["content"])
        urlopen.assert_not_called()

    def test_target_model_guard_skips_non_tonglingyu_model(self) -> None:
        action = self.action_with_key()
        with patch("tonglingyu_gateway_feedback_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "other", "package_id": "pkg-1", "feedback_text": "x"},
                    __user__={"id": "user-1", "role": "user"},
                    __id__="feedback",
                )
            )

        self.assertIn("not enabled", result["content"])
        urlopen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
