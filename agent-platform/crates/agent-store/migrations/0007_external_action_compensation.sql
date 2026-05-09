ALTER TABLE external_action_plans
ADD COLUMN IF NOT EXISTS compensation_ref TEXT;

CREATE INDEX IF NOT EXISTS ix_external_action_plans_error_code
ON external_action_plans(error_code, created_at DESC);
