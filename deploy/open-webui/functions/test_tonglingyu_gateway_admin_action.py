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

    def test_non_admin_is_denied_and_reports_gateway_audit(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"recorded":true}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {"model": "tonglingyu"},
                    __user__={"id": "user-1", "role": "user"},
                    __id__="metrics",
                )
            )

        self.assertIn("requires Open WebUI admin role", result["content"])
        request = urlopen.call_args.args[0]
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/access-denials",
        )
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.get_header("Authorization"), "Bearer admin-key")
        self.assertEqual(request.get_header("X-tonglingyu-subject"), "user-1")
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(payload["action"], "metrics")
        self.assertEqual(payload["denial"], "role_denied")

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
        self.assertEqual(request.get_header("X-tonglingyu-subject"), "admin-1")
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
            return_value=FakeResponse(
                json.dumps(
                    {
                        "object": "tonglingyu.retrieval_failure_admin_list",
                        "list": {
                            "object": "tonglingyu.retrieval_failure_list",
                            "schema_version": "tonglingyu-retrieval-failures-v1",
                            "limit": 20,
                            "offset": 0,
                            "next_offset": 20,
                            "items": [],
                        },
                    }
                )
            ),
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
        self.assertIn("tonglingyu-retrieval-failures-v1", result["content"])
        self.assertIn('"next_offset": 20', result["content"])

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
        self.assertEqual(request.get_header("X-tonglingyu-subject"), "admin-1")
        self.assertIn("tonglingyu.retrieval_failure_admin_update", result["content"])

    def test_governance_tasks_list_supports_bounded_filters(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse(
                json.dumps(
                    {
                        "object": "tonglingyu.governance_task_admin_list",
                        "list": {
                            "object": "tonglingyu.knowledge_governance_task_list",
                            "schema_version": "tonglingyu-knowledge-governance-tasks-v2",
                            "limit": 20,
                            "offset": 0,
                            "next_offset": 20,
                            "items": [],
                        },
                    }
                )
            ),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "status": "open",
                        "task_type": "expected_evidence_fix",
                        "priority": "p0",
                        "source_failure_id": "rf-1",
                        "source_entity_type": "retrieval_failure",
                        "source_entity_id": "rf-1",
                        "limit": 20,
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="governance_tasks",
                )
            )

        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/governance/tasks?status=open&task_type=expected_evidence_fix&priority=p0&source_failure_id=rf-1&source_entity_type=retrieval_failure&source_entity_id=rf-1&limit=20",
        )
        self.assertIn("tonglingyu.governance_task_admin_list", result["content"])
        self.assertIn("tonglingyu-knowledge-governance-tasks-v2", result["content"])
        self.assertIn('"next_offset": 20', result["content"])

    def test_retrieval_failure_cluster_uses_post_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.retrieval_failure_cluster_admin_result"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "status": "open",
                        "failure_type": "quality_report_not_passed",
                        "min_cluster_size": 2,
                        "limit": 20,
                        "create_tasks": True,
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="retrieval_failure_cluster",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/retrieval-failures/cluster",
        )
        self.assertEqual(payload["human_review_status"], "open")
        self.assertEqual(payload["failure_type"], "quality_report_not_passed")
        self.assertEqual(payload["min_cluster_size"], 2)
        self.assertEqual(payload["limit"], 20)
        self.assertTrue(payload["create_tasks"])
        self.assertIn("tonglingyu.retrieval_failure_cluster_admin_result", result["content"])

    def test_governance_task_manual_create_uses_post_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.governance_task_admin_create"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "source_entity_type": "trace",
                        "source_entity_id": "trace-1",
                        "task_type": "expert_review",
                        "priority": "p0",
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="governance_task_create",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/governance/tasks",
        )
        self.assertEqual(payload["source_entity_type"], "trace")
        self.assertEqual(payload["source_entity_id"], "trace-1")
        self.assertIn("tonglingyu.governance_task_admin_create", result["content"])

    def test_governance_task_create_from_failure_uses_post_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.governance_task_admin_create"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "failure_id": "rf-1",
                        "task_type": "expected_evidence_fix",
                        "priority": "p0",
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="governance_task_from_failure",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/retrieval-failures/rf-1/governance-task",
        )
        self.assertEqual(payload["task_type"], "expected_evidence_fix")
        self.assertEqual(payload["priority"], "p0")
        self.assertIn("tonglingyu.governance_task_admin_create", result["content"])

    def test_knowledge_patch_proposal_uses_post_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse(
                '{"object":"tonglingyu.knowledge_patch_proposal_admin_create"}'
            ),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "proposal_type": "alias",
                        "trace_id": "trace-1",
                        "package_id": "pkg-1",
                        "source_ref": "package:pkg-1",
                        "payload": {
                            "alias": "灵玉",
                            "target_ref": "person:baoyu",
                        },
                        "priority": "p1",
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="knowledge_patch_proposal",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/governance/proposals",
        )
        self.assertEqual(payload["proposal_type"], "alias")
        self.assertEqual(payload["payload"]["target_ref"], "person:baoyu")
        self.assertIn(
            "tonglingyu.knowledge_patch_proposal_admin_create", result["content"]
        )

    def test_knowledge_items_list_supports_filters(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse(
                json.dumps(
                    {
                        "object": "tonglingyu.knowledge_item_admin_list",
                        "list": {
                            "schema_version": "tonglingyu-knowledge-item-states-v1",
                            "items": [],
                        },
                    }
                )
            ),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "kind": "alias",
                        "state": "system_calibrated",
                        "limit": 20,
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="knowledge_items",
                )
            )

        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/knowledge/items?kind=alias&state=system_calibrated&limit=20",
        )
        self.assertIn("tonglingyu.knowledge_item_admin_list", result["content"])

    def test_knowledge_item_read_uses_get(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.knowledge_item_admin_read"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "messages": [{"content": "item_id: ki-alias-1"}],
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="knowledge_item",
                )
            )

        self.assertEqual(
            urlopen.call_args.args[0].full_url,
            "http://tonglingyu-gateway:8090/v1/admin/knowledge/items/ki-alias-1",
        )
        self.assertIn("tonglingyu.knowledge_item_admin_read", result["content"])

    def test_knowledge_item_review_uses_post_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse(
                '{"object":"tonglingyu.knowledge_item_admin_review"}'
            ),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "item_id": "ki-alias-1",
                        "task_id": "kgt-1",
                        "decision": "accept",
                        "trace_id": "trace-1",
                        "reviewer": "admin-1",
                        "review_note": "accepted",
                        "evidence_ref": "source://review-note/001",
                        "if_match_state_version": 2,
                        "if_match_task_updated_at": "2026-05-17T00:00:00Z",
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="knowledge_item_review",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/knowledge/items/ki-alias-1/review",
        )
        self.assertEqual(payload["decision"], "accept")
        self.assertEqual(payload["task_id"], "kgt-1")
        self.assertEqual(payload["if_match_state_version"], 2)
        self.assertEqual(request.get_header("X-tonglingyu-subject"), "admin-1")
        self.assertIn("tonglingyu.knowledge_item_admin_review", result["content"])

    def test_governance_task_update_uses_patch_json(self) -> None:
        action = self.action_with_key()
        with patch(
            "tonglingyu_gateway_admin_action.urllib.request.urlopen",
            return_value=FakeResponse('{"object":"tonglingyu.governance_task_admin_update"}'),
        ) as urlopen:
            result = asyncio.run(
                action.action(
                    {
                        "model": "tonglingyu",
                        "task_id": "kgt-1",
                        "status": "accepted",
                        "reviewer": "admin-1",
                        "review_note": "accepted",
                        "evidence_ref": "source://review-note/001",
                        "if_match_updated_at": "2026-05-15T00:00:00Z",
                    },
                    __user__={"id": "admin-1", "role": "admin"},
                    __id__="governance_task_update",
                )
            )

        request = urlopen.call_args.args[0]
        payload = json.loads(request.data.decode("utf-8"))
        self.assertEqual(request.get_method(), "PATCH")
        self.assertEqual(
            request.full_url,
            "http://tonglingyu-gateway:8090/v1/admin/governance/tasks/kgt-1",
        )
        self.assertEqual(payload["status"], "accepted")
        self.assertEqual(payload["evidence_ref"], "source://review-note/001")
        self.assertIn("tonglingyu.governance_task_admin_update", result["content"])

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
