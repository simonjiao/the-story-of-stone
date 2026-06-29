import asyncio
import pathlib
import re
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from agent_identity_bridge_filter import (
    FORBIDDEN_GATEWAY_CONTROL_FIELDS,
    RECURSIVE_GATEWAY_CONTROL_CONTAINERS,
    SHALLOW_GATEWAY_CONTROL_CONTAINERS,
    Filter,
    _signature,
)


class AgentIdentityBridgeFilterTest(unittest.TestCase):
    def test_signature_is_stable_for_canonical_payload(self) -> None:
        context = {
            "version": 1,
            "issuer": "open-webui",
            "subject": "openwebui:user-1",
            "user_role": "user",
            "chat_id": "chat-1",
            "session_id": "session-1",
            "message_id": "message-1",
            "model": "tonglingyu",
            "issued_at": 1778220000,
            "nonce": "nonce-1",
        }
        self.assertEqual(
            _signature("bridge-secret", context),
            "c2b5b51c2e432b504341b9098fc8e5103710e445ec6ff099871b5cabdcb15e03",
        )

    def test_control_field_filter_tracks_gateway_forbidden_fields(self) -> None:
        repo_root = pathlib.Path(__file__).resolve().parents[2]
        gateway_main = repo_root / "agent-platform/crates/tonglingyu-gateway/src/main.rs"
        source = gateway_main.read_text(encoding="utf-8")
        forbidden_match = re.search(
            r"fn forbidden_control_fields\(payload: &Value\).*?const FORBIDDEN: &\[&str\] = &\[(.*?)\];",
            source,
            re.S,
        )
        nested_match = re.search(
            r"const NESTED_OBJECTS: &\[&str\] = &\[(.*?)\];",
            source,
            re.S,
        )
        shallow_match = re.search(
            r"const SHALLOW_NESTED_OBJECTS: &\[&str\] = &\[(.*?)\];",
            source,
            re.S,
        )
        self.assertIsNotNone(forbidden_match)
        self.assertIsNotNone(nested_match)
        self.assertIsNotNone(shallow_match)

        gateway_forbidden = set(re.findall(r'"([^"]+)"', forbidden_match.group(1)))
        gateway_nested = set(re.findall(r'"([^"]+)"', nested_match.group(1)))
        gateway_shallow = set(re.findall(r'"([^"]+)"', shallow_match.group(1)))
        self.assertEqual(FORBIDDEN_GATEWAY_CONTROL_FIELDS, gateway_forbidden)
        self.assertEqual(RECURSIVE_GATEWAY_CONTROL_CONTAINERS, gateway_nested)
        self.assertEqual(SHALLOW_GATEWAY_CONTROL_CONTAINERS, gateway_shallow)

    def test_inlet_injects_signed_context_for_target_model(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        body = {"model": "tonglingyu", "messages": []}
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "user"},
                __metadata__={
                    "chat_id": "chat-1",
                    "session_id": "session-1",
                    "message_id": "message-1",
                },
            )
        )

        context = result["agent_bridge_context"]
        self.assertEqual(context["subject"], "openwebui:user-1")
        self.assertEqual(context["chat_id"], "chat-1")
        self.assertTrue(context["signature"])

    def test_inlet_accepts_target_models_list(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        filt.valves.TARGET_MODEL = "legacy-agent"
        filt.valves.TARGET_MODELS = "hermes-agent,tonglingyu"
        body = {"model": "tonglingyu", "messages": []}
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "user"},
                __metadata__={
                    "chat_id": "chat-1",
                    "session_id": "session-1",
                    "message_id": "message-1",
                },
            )
        )

        context = result["agent_bridge_context"]
        self.assertEqual(context["model"], "tonglingyu")
        self.assertEqual(context["subject"], "openwebui:user-1")

    def test_inlet_skips_non_target_model(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        filt.valves.TARGET_MODEL = "legacy-agent"
        filt.valves.TARGET_MODELS = "legacy-agent"
        body = {"model": "tonglingyu", "messages": []}
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "user"},
                __metadata__={"chat_id": "chat-1"},
            )
        )

        self.assertNotIn("agent_bridge_context", result)

    def test_inlet_prefers_user_message_id_for_dedupe(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        body = {"model": "tonglingyu", "messages": []}
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "user"},
                __metadata__={
                    "chat_id": "chat-1",
                    "session_id": "session-1",
                    "message_id": "assistant-placeholder-1",
                    "user_message_id": "user-message-1",
                },
            )
        )

        context = result["agent_bridge_context"]
        self.assertEqual(context["message_id"], "user-message-1")

    def test_inlet_accepts_body_level_metadata_fallbacks(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        body = {
            "model": "tonglingyu",
            "messages": [],
            "chat_id": "chat-from-body",
            "session_id": "session-from-body",
            "user_message_id": "message-from-body",
        }
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "admin"},
            )
        )

        context = result["agent_bridge_context"]
        self.assertEqual(context["chat_id"], "chat-from-body")
        self.assertEqual(context["session_id"], "session-from-body")
        self.assertEqual(context["message_id"], "message-from-body")
        self.assertEqual(context["user_role"], "admin")

    def test_inlet_strips_gateway_control_fields_for_target_model(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        body = {
            "model": "tonglingyu",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function"}],
            "tool_choice": "auto",
            "parallel_tool_calls": True,
            "metadata": {
                "chat_id": "chat-1",
                "trace_id": "external-trace",
                "nested": {"package_id": "external-package"},
            },
            "user": {"name": "reader", "trace_id": "external-user-trace"},
        }
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "user"},
                __metadata__={
                    "chat_id": "chat-1",
                    "session_id": "session-1",
                    "message_id": "message-1",
                },
            )
        )

        self.assertNotIn("tools", result)
        self.assertNotIn("tool_choice", result)
        self.assertNotIn("parallel_tool_calls", result)
        self.assertNotIn("trace_id", result["metadata"])
        self.assertNotIn("package_id", result["metadata"]["nested"])
        self.assertNotIn("trace_id", result["user"])
        self.assertEqual(result["user"]["name"], "reader")
        self.assertIn("agent_bridge_context", result)

    def test_inlet_leaves_non_target_model_control_fields_untouched(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        filt.valves.TARGET_MODEL = "tonglingyu"
        filt.valves.TARGET_MODELS = "tonglingyu"
        body = {
            "model": "other-model",
            "messages": [],
            "tools": [{"type": "function"}],
            "metadata": {"trace_id": "external-trace"},
        }
        result = asyncio.run(
            filt.inlet(
                body,
                __user__={"id": "user-1", "role": "user"},
                __metadata__={"chat_id": "chat-1"},
            )
        )

        self.assertIn("tools", result)
        self.assertIn("trace_id", result["metadata"])
        self.assertNotIn("agent_bridge_context", result)


if __name__ == "__main__":
    unittest.main()
