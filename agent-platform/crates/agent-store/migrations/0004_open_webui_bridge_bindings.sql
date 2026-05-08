CREATE TABLE IF NOT EXISTS open_webui_bridge_bindings (
    id TEXT PRIMARY KEY,
    open_webui_subject TEXT NOT NULL,
    open_webui_chat_id TEXT NOT NULL,
    open_webui_session_id TEXT,
    model TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    agent_session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    last_message_id TEXT,
    last_run_id TEXT,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    closed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_open_webui_bridge_active_chat
ON open_webui_bridge_bindings(open_webui_subject, open_webui_chat_id, model)
WHERE status = 'active';

CREATE INDEX IF NOT EXISTS ix_open_webui_bridge_session
ON open_webui_bridge_bindings(agent_session_id, status);
