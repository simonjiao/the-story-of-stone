import asyncio
import io
import json
import pathlib
import sys
import unittest
import urllib.error
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from tonglingyu_gateway_admin_action import Action


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


class TonglingyuGatewayAdminActionTest(unittest.TestCase):
    def action_with_key(self) -> Action:
        action = Action()
        action.valves.GATEWAY_ADMIN_API_KEY = "admin-key"
        return action

    def test_non_admin_is_denied_without_gateway_call(self) -> None:
        action = self.action_with_key()
        with patch("tonglingyu_gateway_admin_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu"},
                    __user__={"id": "user-1", "role": "user"},
                    __id__="metrics",
                )
            )

        self.assertIn("requires Open WebUI admin role", result["content"])
        urlopen.assert_not_called()

    def test_missing_admin_key_is_denied_without_gateway_call(self) -> None:
        action = Action()
        with patch("tonglingyu_gateway_admin_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu"},
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="metrics",
                )
            )

        self.assertIn("admin key is not configured", result["content"])
        urlopen.assert_not_called()

    def test_metrics_calls_gateway_admin_endpoint(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.gateway_metrics"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu"},
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="metrics",
                )
            )

        request = urlopen.call_args.args[0]
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/metrics",
        )
        self.assertEqual(request.get_header("Authorization"), "Bearer admin-key")
        self.assertIn("tonglingyu.gateway_metrics", result["content"])

    def test_trace_action_prompts_for_trace_id(self) -> None:
        action = self.action_with_key()
        prompts = []

        async def event_call(payload: dict) -> str:
            prompts.append(payload)
            return "trace-1"

        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.trace"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu", "messages": []},
                    __user__={"id": "admin-1", "role": "admin"},
                    __event_call__=event_call,
                    __id__="trace",
                )
            )

        self.assertEqual(prompts[0]["data"]["title"], "Trace ID")
        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/traces/trace-1",
        )
        self.assertIn("tonglingyu.trace", result["content"])

    def test_package_id_can_be_extracted_from_message_content(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.package_audit"}'),
        ) as urlopen:
            asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "messages": [{"content": "package_id: pkg-1"}],
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="package",
                )
            )

        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/packages/pkg-1",
        )

    def test_retrieval_failures_list_supports_bounded_filters(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.retrieval_failure_admin_list"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "status": "open",
                        "failure_type": "quality_report_not_passed",
                        "limit": 20,
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="retrieval_failures",
                )
            )

        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/retrieval-failures?status=open&failure_type=quality_report_not_passed&limit=20",
        )
        self.assertIn("tonglingyu.retrieval_failure_admin_list", result["content"])

    def test_retrieval_failure_id_can_be_extracted_from_message_content(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.retrieval_failure_admin_read"}'),
        ) as urlopen:
            asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "messages": [{"content": "failure_id: rf-1"}],
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="retrieval_failure",
                )
            )

        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/retrieval-failures/rf-1",
        )

    def test_retrieval_failure_update_uses_patch_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.retrieval_failure_admin_update"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "failure_id": "rf-1",
                        "human_review_status": "resolved",
                        "reviewer": "admin-1",
                        "review_note": "fixed",
                        "if_match_updated_at": "2026-05-15T00:00:00Z",
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="retrieval_failure_update",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "PATCH")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/retrieval-failures/rf-1",
        )
        self.assertEqual(payload["human_review_status"], "resolved")
        self.assertEqual(payload["if_match_updated_at"], "2026-05-15T00:00:00Z")
        self.assertIn("tonglingyu.retrieval_failure_admin_update", result["content"])

    def test_target_model_guard_skips_non_tonglingyu_model(self) -> None:
        action = self.action_with_key()
        with patch("tonglingyu_gateway_admin_action.urllib.request.urlopen") as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "hermes-agent"},
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="metrics",
                )
            )

        self.assertIn("not enabled for model", result["content"])
        urlopen.assert_not_called()

    def test_gateway_http_error_is_sanitized(self) -> None:
        action = self.action_with_key()
        error = urllib.error.HTTPError(
            "http://tonglingyu-gateway:8090/v1/admin/metrics",
            401,
            "unauthorized",
            {},
            io.BytesIO(b"secret raw body"),
        )
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            side_effect=error,
        ):
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu"},
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="metrics",
                )
            )

        self.assertIn("HTTP 401", result["content"])
        self.assertNotIn("admin-key", result["content"])
        self.assertNotIn("secret raw body", result["content"])


if __name__ == "__main__":
    unittest.main()
