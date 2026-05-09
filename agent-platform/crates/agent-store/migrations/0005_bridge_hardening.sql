ALTER TABLE agent_session_messages
ADD COLUMN IF NOT EXISTS external_message_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS ux_session_messages_external_message
ON agent_session_messages(session_id, external_message_id)
WHERE external_message_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS open_webui_bridge_nonces (
    id TEXT PRIMARY KEY,
    open_webui_subject TEXT NOT NULL,
    open_webui_chat_id TEXT NOT NULL,
    model TEXT NOT NULL,
    nonce TEXT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    trace_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_open_webui_bridge_nonce
ON open_webui_bridge_nonces(open_webui_subject, open_webui_chat_id, model, nonce);

CREATE INDEX IF NOT EXISTS ix_open_webui_bridge_nonce_created
ON open_webui_bridge_nonces(created_at);
