ALTER TABLE agent_sessions
ADD COLUMN IF NOT EXISTS idempotency_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS ux_agent_sessions_idempotency
ON agent_sessions(owner_user, agent_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;
