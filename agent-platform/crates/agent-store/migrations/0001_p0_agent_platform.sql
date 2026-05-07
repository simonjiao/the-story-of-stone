CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    display_name TEXT,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS roles (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS service_accounts (
    id TEXT PRIMARY KEY,
    service_name TEXT NOT NULL,
    status TEXT NOT NULL,
    allowed_actions JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS resource_bindings (
    id TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    owner_user TEXT NOT NULL,
    attributes JSONB NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_templates (
    agent_type TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    allowed_triggers JSONB NOT NULL,
    allowed_actions JSONB NOT NULL,
    default_constraints JSONB NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_instances (
    id TEXT PRIMARY KEY,
    agent_type TEXT NOT NULL,
    hermes_profile TEXT NOT NULL,
    owner_user TEXT NOT NULL,
    target_resource TEXT NOT NULL,
    core_constraints_hash TEXT NOT NULL,
    status TEXT NOT NULL,
    display_name TEXT,
    config JSONB NOT NULL,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_agent_instances_reuse_key
ON agent_instances(owner_user, agent_type, target_resource, core_constraints_hash)
WHERE status IN ('provisioning', 'running', 'paused', 'failed');

CREATE TABLE IF NOT EXISTS agent_policies (
    id TEXT PRIMARY KEY,
    agent_id TEXT,
    agent_type TEXT,
    policy JSONB NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_requests (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT,
    requested_by_user TEXT NOT NULL,
    requested_by_service TEXT NOT NULL,
    request_type TEXT NOT NULL,
    agent_type TEXT,
    target_resource TEXT,
    intent_text TEXT,
    structured_payload JSONB NOT NULL,
    status TEXT NOT NULL,
    denial_reason TEXT,
    approval_id TEXT,
    result_agent_id TEXT,
    result_run_id TEXT,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_agent_requests_idempotency
ON agent_requests(requested_by_user, requested_by_service, idempotency_key)
WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS approval_requests (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    requested_by_user TEXT NOT NULL,
    approver_user TEXT,
    status TEXT NOT NULL,
    risk_level TEXT,
    reason TEXT,
    decision_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    decided_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS agent_sessions (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    owner_user TEXT NOT NULL,
    source_conversation_id TEXT,
    parent_session_id TEXT,
    created_by_session_id TEXT,
    status TEXT NOT NULL,
    depth INT NOT NULL DEFAULT 0,
    resource_scope JSONB NOT NULL,
    context_summary TEXT,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_session_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    role TEXT NOT NULL,
    content_ref TEXT,
    content_summary TEXT,
    run_id TEXT,
    trace_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_session_messages_sequence
ON agent_session_messages(session_id, sequence);

CREATE TABLE IF NOT EXISTS agent_runs (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT,
    agent_id TEXT NOT NULL,
    session_id TEXT,
    trigger_type TEXT NOT NULL,
    target_resource TEXT NOT NULL,
    run_status TEXT NOT NULL,
    risk_level TEXT NOT NULL,
    side_effect_mode TEXT NOT NULL,
    lease_owner TEXT,
    lease_until TIMESTAMPTZ,
    retry_count INT NOT NULL DEFAULT 0,
    result_summary TEXT,
    result_ref TEXT,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    claimed_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_agent_runs_idempotency
ON agent_runs(agent_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS ix_agent_runs_claimable
ON agent_runs(run_status, lease_until, created_at);

CREATE TABLE IF NOT EXISTS agent_run_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_name TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS resource_locks (
    id TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    lock_scope TEXT NOT NULL,
    holder_run_id TEXT NOT NULL,
    lease_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_resource_locks_active
ON resource_locks(resource_type, resource_id, lock_scope);

CREATE TABLE IF NOT EXISTS agent_grants (
    id TEXT PRIMARY KEY,
    subject_type TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    constraints JSONB NOT NULL,
    granted_by TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS worker_heartbeats (
    worker_id TEXT PRIMARY KEY,
    current_run_id TEXT,
    status TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS observer_reports (
    id TEXT PRIMARY KEY,
    observer_run_id TEXT NOT NULL,
    health_status TEXT NOT NULL,
    risk_level TEXT,
    summary TEXT NOT NULL,
    findings JSONB NOT NULL,
    recommendations JSONB NOT NULL,
    evidence_refs JSONB NOT NULL,
    trace_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id TEXT PRIMARY KEY,
    actor_user TEXT,
    actor_service TEXT,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    decision TEXT,
    reason TEXT,
    request_id TEXT,
    session_id TEXT,
    run_id TEXT,
    approval_id TEXT,
    observer_report_id TEXT,
    trace_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

INSERT INTO agent_templates (
    agent_type,
    display_name,
    allowed_triggers,
    allowed_actions,
    default_constraints,
    status,
    created_at
) VALUES
(
    'background_worker',
    '通用后台执行 Agent',
    '["manual","scheduled","webhook","session_message"]'::jsonb,
    '["analyze","prepare_change","run_checks"]'::jsonb,
    '{
      "default_side_effect_mode": "approval_required",
      "max_items_per_run": 8,
      "max_runtime_seconds": 1800,
      "max_concurrent_runs_per_agent": 1,
      "max_concurrent_runs_per_resource": 1,
      "max_active_agents_per_user": 10,
      "max_active_agents_per_resource": 3,
      "max_active_agents_per_user_resource": 1,
      "max_session_depth": 1,
      "max_child_sessions_per_parent": 3,
      "active_child_sessions_per_parent": 2,
      "protected_scopes": ["secrets", "credentials", "production", "protected_branch"]
    }'::jsonb,
    'active',
    now()
),
(
    'observer_agent',
    '系统观察 Agent',
    '["scheduled","admin_manual"]'::jsonb,
    '["read_status_snapshot","write_observer_report"]'::jsonb,
    '{
      "default_side_effect_mode": "deny",
      "max_concurrent_observer_runs": 1,
      "readable_scopes": ["status_summary","audit_summary","worker_heartbeat_summary","lock_summary","error_metrics"],
      "forbidden_scopes": ["secrets","credentials","full_prompt","full_context","raw_internal_logs"]
    }'::jsonb,
    'active',
    now()
)
ON CONFLICT (agent_type) DO UPDATE SET
    display_name = EXCLUDED.display_name,
    allowed_triggers = EXCLUDED.allowed_triggers,
    allowed_actions = EXCLUDED.allowed_actions,
    default_constraints = EXCLUDED.default_constraints,
    status = EXCLUDED.status;
