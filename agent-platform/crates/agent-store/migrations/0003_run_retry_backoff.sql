ALTER TABLE agent_runs
ADD COLUMN IF NOT EXISTS next_retry_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS ix_agent_runs_retry_ready
ON agent_runs(run_status, next_retry_at, created_at);
