CREATE TABLE IF NOT EXISTS side_effect_plans (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    connector TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_ref TEXT NOT NULL,
    risk_level TEXT NOT NULL,
    side_effect_mode TEXT NOT NULL,
    approval_id TEXT,
    credential_scope TEXT,
    input_summary TEXT,
    input_ref TEXT,
    result_ref TEXT,
    status TEXT NOT NULL,
    error_code TEXT,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_side_effect_plans_run
ON side_effect_plans(run_id, created_at DESC);

CREATE INDEX IF NOT EXISTS ix_side_effect_plans_status
ON side_effect_plans(status, created_at DESC);

CREATE TABLE IF NOT EXISTS credential_leases (
    id TEXT PRIMARY KEY,
    side_effect_plan_id TEXT NOT NULL,
    credential_scope TEXT NOT NULL,
    provider_ref TEXT,
    status TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    trace_id TEXT NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_credential_leases_plan
ON credential_leases(side_effect_plan_id, created_at DESC);

CREATE INDEX IF NOT EXISTS ix_credential_leases_status
ON credential_leases(status, expires_at);
