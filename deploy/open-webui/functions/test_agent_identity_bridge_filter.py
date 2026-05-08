import asyncio
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from agent_identity_bridge_filter import Filter, _signature


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
            "model": "hermes-agent",
            "issued_at": 1778220000,
            "nonce": "nonce-1",
        }
        self.assertEqual(
            _signature("bridge-secret", context),
            "6185debba03afb3b99ac20a9ff87d93757940034dc9b3ccef7c83247004fbb10",
        )

    def test_inlet_injects_signed_context_for_target_model(self) -> None:
        filt = Filter()
        filt.valves.AGENT_BRIDGE_SECRET = "bridge-secret"
        body = {"model": "hermes-agent", "messages": []}
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


if __name__ == "__main__":
    unittest.main()
