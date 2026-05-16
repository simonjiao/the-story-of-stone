use crate::{AgentStore, store_error};
use agent_core::{
    AgentBridgeBinding, AgentCoreError, AgentGrant, AgentInstance, AgentInstanceStatus,
    AgentRequest, AgentRequestStatus, AgentRun, AgentRunStatus, AgentSession, AgentSessionMessage,
    AgentSummary, AgentTemplate, AppendMessageInput, ApprovalRequest, ApprovalStatus, AuditLog,
    CoreResult, CredentialLease, EmptyResponse, ErrorCode, ExternalActionPlan, MemoryStore,
    ObserverReport, ObserverReportSummary, ObserverSnapshot, ObserverSnapshotStore, ResourceLock,
    RunClaim, RunQueue, RunSummary, RuntimeOutput, SessionContext, SessionSummary, new_id,
    validate_run_transition, validate_session_transition,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx_core::{query::query, query_scalar::query_scalar, row::Row};
use sqlx_postgres::{PgPool, PgPoolOptions, PgRow};
use std::{env, path::PathBuf, str::FromStr, time::Duration};
use time::OffsetDateTime;

const MIGRATIONS_DIR_ENV: &str = "AGENT_STORE_MIGRATIONS_DIR";
const CONTAINER_MIGRATIONS_DIR: &str = "/usr/local/share/agent-platform/migrations";
const LOCAL_MIGRATIONS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");

#[derive(Debug, Clone)]
pub struct PgAgentStore {
    pool: PgPool,
}

impl PgAgentStore {
    pub async fn connect(database_url: &str, max_connections: u32) -> CoreResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await
            .map_err(store_error)?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> CoreResult<()> {
        let migrations_dir = resolve_migrations_dir()?;
        let migrator = sqlx_core::migrate::Migrator::new(migrations_dir.as_path())
            .await
            .map_err(store_error)?;
        migrator.run(&self.pool).await.map_err(store_error)
    }
}

fn resolve_migrations_dir() -> CoreResult<PathBuf> {
    if let Ok(value) = env::var(MIGRATIONS_DIR_ENV) {
        let path = PathBuf::from(value);
        if path.is_dir() {
            return Ok(path);
        }
        return Err(store_error(format!(
            "{MIGRATIONS_DIR_ENV} does not point to a migration directory: {}",
            path.display()
        )));
    }

    for candidate in [
        PathBuf::from(CONTAINER_MIGRATIONS_DIR),
        PathBuf::from(LOCAL_MIGRATIONS_DIR),
        PathBuf::from("crates/agent-store/migrations"),
        PathBuf::from("migrations"),
    ] {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err(store_error(format!(
        "agent-store migrations directory not found; set {MIGRATIONS_DIR_ENV}"
    )))
}

fn parse<T>(value: String) -> CoreResult<T>
where
    T: FromStr<Err = agent_core::AgentCoreError>,
{
    value.parse()
}

fn lease_until(lease: Duration) -> OffsetDateTime {
    OffsetDateTime::now_utc()
        + time::Duration::try_from(lease).unwrap_or_else(|_| time::Duration::seconds(30))
}

fn retry_backoff(retry_count: i32) -> time::Duration {
    match retry_count {
        count if count <= 1 => time::Duration::seconds(30),
        2 => time::Duration::seconds(120),
        _ => time::Duration::seconds(300),
    }
}

fn map_template(row: &PgRow) -> CoreResult<AgentTemplate> {
    Ok(AgentTemplate {
        agent_type: row.try_get("agent_type").map_err(store_error)?,
        display_name: row.try_get("display_name").map_err(store_error)?,
        allowed_triggers: row.try_get("allowed_triggers").map_err(store_error)?,
        allowed_actions: row.try_get("allowed_actions").map_err(store_error)?,
        default_constraints: row.try_get("default_constraints").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
    })
}

fn map_request(row: &PgRow) -> CoreResult<AgentRequest> {
    Ok(AgentRequest {
        id: row.try_get("id").map_err(store_error)?,
        idempotency_key: row.try_get("idempotency_key").map_err(store_error)?,
        requested_by_user: row.try_get("requested_by_user").map_err(store_error)?,
        requested_by_service: row.try_get("requested_by_service").map_err(store_error)?,
        request_type: parse(
            row.try_get::<String, _>("request_type")
                .map_err(store_error)?,
        )?,
        agent_type: row.try_get("agent_type").map_err(store_error)?,
        target_resource: row.try_get("target_resource").map_err(store_error)?,
        intent_text: row.try_get("intent_text").map_err(store_error)?,
        structured_payload: row.try_get("structured_payload").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        denial_reason: row.try_get("denial_reason").map_err(store_error)?,
        approval_id: row.try_get("approval_id").map_err(store_error)?,
        result_agent_id: row.try_get("result_agent_id").map_err(store_error)?,
        result_run_id: row.try_get("result_run_id").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        version: row.try_get("version").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        updated_at: row.try_get("updated_at").map_err(store_error)?,
    })
}

fn map_approval(row: &PgRow) -> CoreResult<ApprovalRequest> {
    Ok(ApprovalRequest {
        id: row.try_get("id").map_err(store_error)?,
        request_id: row.try_get("request_id").map_err(store_error)?,
        requested_by_user: row.try_get("requested_by_user").map_err(store_error)?,
        approver_user: row.try_get("approver_user").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        risk_level: row
            .try_get::<Option<String>, _>("risk_level")
            .map_err(store_error)?
            .map(parse)
            .transpose()?,
        reason: row.try_get("reason").map_err(store_error)?,
        decision_reason: row.try_get("decision_reason").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        decided_at: row.try_get("decided_at").map_err(store_error)?,
    })
}

fn map_agent(row: &PgRow) -> CoreResult<AgentInstance> {
    Ok(AgentInstance {
        id: row.try_get("id").map_err(store_error)?,
        agent_type: row.try_get("agent_type").map_err(store_error)?,
        hermes_profile: row.try_get("hermes_profile").map_err(store_error)?,
        owner_user: row.try_get("owner_user").map_err(store_error)?,
        target_resource: row.try_get("target_resource").map_err(store_error)?,
        core_constraints_hash: row.try_get("core_constraints_hash").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        display_name: row.try_get("display_name").map_err(store_error)?,
        config: row.try_get("config").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        version: row.try_get("version").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        updated_at: row.try_get("updated_at").map_err(store_error)?,
    })
}

fn map_session(row: &PgRow) -> CoreResult<AgentSession> {
    Ok(AgentSession {
        id: row.try_get("id").map_err(store_error)?,
        idempotency_key: row.try_get("idempotency_key").map_err(store_error)?,
        agent_id: row.try_get("agent_id").map_err(store_error)?,
        owner_user: row.try_get("owner_user").map_err(store_error)?,
        source_conversation_id: row.try_get("source_conversation_id").map_err(store_error)?,
        parent_session_id: row.try_get("parent_session_id").map_err(store_error)?,
        created_by_session_id: row.try_get("created_by_session_id").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        depth: row.try_get("depth").map_err(store_error)?,
        resource_scope: row.try_get("resource_scope").map_err(store_error)?,
        context_summary: row.try_get("context_summary").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        version: row.try_get("version").map_err(store_error)?,
        expires_at: row.try_get("expires_at").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        updated_at: row.try_get("updated_at").map_err(store_error)?,
    })
}

fn map_message(row: &PgRow) -> CoreResult<AgentSessionMessage> {
    Ok(AgentSessionMessage {
        id: row.try_get("id").map_err(store_error)?,
        session_id: row.try_get("session_id").map_err(store_error)?,
        sequence: row.try_get("sequence").map_err(store_error)?,
        role: parse(row.try_get::<String, _>("role").map_err(store_error)?)?,
        content_ref: row.try_get("content_ref").map_err(store_error)?,
        content_summary: row.try_get("content_summary").map_err(store_error)?,
        external_message_id: row.try_get("external_message_id").map_err(store_error)?,
        run_id: row.try_get("run_id").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
    })
}

fn map_run(row: &PgRow) -> CoreResult<AgentRun> {
    Ok(AgentRun {
        id: row.try_get("id").map_err(store_error)?,
        idempotency_key: row.try_get("idempotency_key").map_err(store_error)?,
        agent_id: row.try_get("agent_id").map_err(store_error)?,
        session_id: row.try_get("session_id").map_err(store_error)?,
        trigger_type: parse(
            row.try_get::<String, _>("trigger_type")
                .map_err(store_error)?,
        )?,
        target_resource: row.try_get("target_resource").map_err(store_error)?,
        run_status: parse(
            row.try_get::<String, _>("run_status")
                .map_err(store_error)?,
        )?,
        risk_level: parse(
            row.try_get::<String, _>("risk_level")
                .map_err(store_error)?,
        )?,
        external_action_mode: parse(
            row.try_get::<String, _>("external_action_mode")
                .map_err(store_error)?,
        )?,
        lease_owner: row.try_get("lease_owner").map_err(store_error)?,
        lease_until: row.try_get("lease_until").map_err(store_error)?,
        next_retry_at: row.try_get("next_retry_at").map_err(store_error)?,
        retry_count: row.try_get("retry_count").map_err(store_error)?,
        result_summary: row.try_get("result_summary").map_err(store_error)?,
        result_ref: row.try_get("result_ref").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        version: row.try_get("version").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        claimed_at: row.try_get("claimed_at").map_err(store_error)?,
        finished_at: row.try_get("finished_at").map_err(store_error)?,
    })
}

fn map_bridge_binding(row: &PgRow) -> CoreResult<AgentBridgeBinding> {
    Ok(AgentBridgeBinding {
        id: row.try_get("id").map_err(store_error)?,
        open_webui_subject: row.try_get("open_webui_subject").map_err(store_error)?,
        open_webui_chat_id: row.try_get("open_webui_chat_id").map_err(store_error)?,
        open_webui_session_id: row.try_get("open_webui_session_id").map_err(store_error)?,
        model: row.try_get("model").map_err(store_error)?,
        agent_id: row.try_get("agent_id").map_err(store_error)?,
        agent_session_id: row.try_get("agent_session_id").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        last_message_id: row.try_get("last_message_id").map_err(store_error)?,
        last_run_id: row.try_get("last_run_id").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        version: row.try_get("version").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        updated_at: row.try_get("updated_at").map_err(store_error)?,
        closed_at: row.try_get("closed_at").map_err(store_error)?,
    })
}

fn map_audit(row: &PgRow) -> CoreResult<AuditLog> {
    Ok(AuditLog {
        id: row.try_get("id").map_err(store_error)?,
        actor_user: row.try_get("actor_user").map_err(store_error)?,
        actor_service: row.try_get("actor_service").map_err(store_error)?,
        action: row.try_get("action").map_err(store_error)?,
        resource_type: row.try_get("resource_type").map_err(store_error)?,
        resource_id: row.try_get("resource_id").map_err(store_error)?,
        decision: row
            .try_get::<Option<String>, _>("decision")
            .map_err(store_error)?
            .map(parse)
            .transpose()?,
        reason: row.try_get("reason").map_err(store_error)?,
        request_id: row.try_get("request_id").map_err(store_error)?,
        session_id: row.try_get("session_id").map_err(store_error)?,
        run_id: row.try_get("run_id").map_err(store_error)?,
        approval_id: row.try_get("approval_id").map_err(store_error)?,
        observer_report_id: row.try_get("observer_report_id").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
    })
}

fn map_report(row: &PgRow) -> CoreResult<ObserverReport> {
    Ok(ObserverReport {
        id: row.try_get("id").map_err(store_error)?,
        observer_run_id: row.try_get("observer_run_id").map_err(store_error)?,
        health_status: parse(
            row.try_get::<String, _>("health_status")
                .map_err(store_error)?,
        )?,
        risk_level: row
            .try_get::<Option<String>, _>("risk_level")
            .map_err(store_error)?
            .map(parse)
            .transpose()?,
        summary: row.try_get("summary").map_err(store_error)?,
        findings: row.try_get("findings").map_err(store_error)?,
        recommendations: row.try_get("recommendations").map_err(store_error)?,
        evidence_refs: row.try_get("evidence_refs").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
    })
}

fn map_external_action_plan(row: &PgRow) -> CoreResult<ExternalActionPlan> {
    Ok(ExternalActionPlan {
        id: row.try_get("id").map_err(store_error)?,
        run_id: row.try_get("run_id").map_err(store_error)?,
        connector: row.try_get("connector").map_err(store_error)?,
        action: row.try_get("action").map_err(store_error)?,
        resource_ref: row.try_get("resource_ref").map_err(store_error)?,
        risk_level: parse(
            row.try_get::<String, _>("risk_level")
                .map_err(store_error)?,
        )?,
        external_action_mode: parse(
            row.try_get::<String, _>("external_action_mode")
                .map_err(store_error)?,
        )?,
        approval_id: row.try_get("approval_id").map_err(store_error)?,
        credential_scope: row.try_get("credential_scope").map_err(store_error)?,
        input_summary: row.try_get("input_summary").map_err(store_error)?,
        input_ref: row.try_get("input_ref").map_err(store_error)?,
        result_ref: row.try_get("result_ref").map_err(store_error)?,
        compensation_ref: row.try_get("compensation_ref").map_err(store_error)?,
        compensation_result_ref: row
            .try_get("compensation_result_ref")
            .map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        error_code: row.try_get("error_code").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        version: row.try_get("version").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
        updated_at: row.try_get("updated_at").map_err(store_error)?,
    })
}

fn map_credential_lease(row: &PgRow) -> CoreResult<CredentialLease> {
    Ok(CredentialLease {
        id: row.try_get("id").map_err(store_error)?,
        external_action_plan_id: row
            .try_get("external_action_plan_id")
            .map_err(store_error)?,
        credential_scope: row.try_get("credential_scope").map_err(store_error)?,
        provider_ref: row.try_get("provider_ref").map_err(store_error)?,
        status: parse(row.try_get::<String, _>("status").map_err(store_error)?)?,
        expires_at: row.try_get("expires_at").map_err(store_error)?,
        trace_id: row.try_get("trace_id").map_err(store_error)?,
        revoked_at: row.try_get("revoked_at").map_err(store_error)?,
        created_at: row.try_get("created_at").map_err(store_error)?,
    })
}

fn run_summary(run: &AgentRun) -> RunSummary {
    RunSummary {
        run_id: run.id.clone(),
        agent_id: run.agent_id.clone(),
        session_id: run.session_id.clone(),
        trigger_type: run.trigger_type,
        target_resource: run.target_resource.clone(),
        run_status: run.run_status,
        risk_level: run.risk_level,
        result_summary: run.result_summary.clone(),
        result_ref: run.result_ref.clone(),
        next_retry_at: run.next_retry_at,
        created_at: run.created_at,
        finished_at: run.finished_at,
        trace_id: run.trace_id.clone(),
    }
}

fn session_summary(session: &AgentSession) -> SessionSummary {
    SessionSummary {
        session_id: session.id.clone(),
        agent_id: session.agent_id.clone(),
        status: session.status,
        parent_session_id: session.parent_session_id.clone(),
        depth: session.depth,
        created_at: session.created_at,
        updated_at: session.updated_at,
        context_summary: session.context_summary.clone(),
        trace_id: session.trace_id.clone(),
    }
}

#[async_trait]
impl AgentStore for PgAgentStore {
    async fn bootstrap(&self) -> CoreResult<()> {
        self.migrate().await
    }

    async fn upsert_template(&self, template: AgentTemplate) -> CoreResult<AgentTemplate> {
        let row = query(
            r#"
            INSERT INTO agent_templates (
                agent_type, display_name, allowed_triggers, allowed_actions,
                default_constraints, status, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7)
            ON CONFLICT (agent_type) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                allowed_triggers = EXCLUDED.allowed_triggers,
                allowed_actions = EXCLUDED.allowed_actions,
                default_constraints = EXCLUDED.default_constraints,
                status = EXCLUDED.status
            RETURNING *
            "#,
        )
        .bind(&template.agent_type)
        .bind(&template.display_name)
        .bind(&template.allowed_triggers)
        .bind(&template.allowed_actions)
        .bind(&template.default_constraints)
        .bind(template.status.to_string())
        .bind(template.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_template(&row)
    }

    async fn get_template(&self, agent_type: &str) -> CoreResult<Option<AgentTemplate>> {
        let row = query("SELECT * FROM agent_templates WHERE agent_type = $1")
            .bind(agent_type)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_template).transpose()
    }

    async fn create_agent_request(&self, request: AgentRequest) -> CoreResult<AgentRequest> {
        let row = query(
            r#"
            INSERT INTO agent_requests (
                id, idempotency_key, requested_by_user, requested_by_service, request_type,
                agent_type, target_resource, intent_text, structured_payload, status,
                denial_reason, approval_id, result_agent_id, result_run_id, trace_id,
                version, created_at, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
            RETURNING *
            "#,
        )
        .bind(&request.id)
        .bind(&request.idempotency_key)
        .bind(&request.requested_by_user)
        .bind(&request.requested_by_service)
        .bind(request.request_type.to_string())
        .bind(&request.agent_type)
        .bind(&request.target_resource)
        .bind(&request.intent_text)
        .bind(&request.structured_payload)
        .bind(request.status.to_string())
        .bind(&request.denial_reason)
        .bind(&request.approval_id)
        .bind(&request.result_agent_id)
        .bind(&request.result_run_id)
        .bind(&request.trace_id)
        .bind(request.version)
        .bind(request.created_at)
        .bind(request.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_request(&row)
    }

    async fn get_agent_request(&self, request_id: &str) -> CoreResult<Option<AgentRequest>> {
        let row = query("SELECT * FROM agent_requests WHERE id = $1")
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_request).transpose()
    }

    async fn find_agent_request_by_idempotency(
        &self,
        user_id: &str,
        service_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentRequest>> {
        let row = query(
            r#"
            SELECT * FROM agent_requests
            WHERE requested_by_user = $1
              AND requested_by_service = $2
              AND idempotency_key = $3
            "#,
        )
        .bind(user_id)
        .bind(service_id)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.as_ref().map(map_request).transpose()
    }

    async fn list_agent_requests(
        &self,
        user_id: Option<&str>,
        statuses: &[AgentRequestStatus],
        limit: i64,
    ) -> CoreResult<Vec<AgentRequest>> {
        let rows = query(
            r#"
            SELECT * FROM agent_requests
            WHERE ($1::text IS NULL OR requested_by_user = $1)
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let mut requests = rows
            .iter()
            .map(map_request)
            .collect::<CoreResult<Vec<_>>>()?;
        if !statuses.is_empty() {
            requests.retain(|request| statuses.contains(&request.status));
        }
        Ok(requests)
    }

    async fn update_agent_request(&self, request: AgentRequest) -> CoreResult<AgentRequest> {
        let row = query(
            r#"
            UPDATE agent_requests SET
                idempotency_key = $2,
                requested_by_user = $3,
                requested_by_service = $4,
                request_type = $5,
                agent_type = $6,
                target_resource = $7,
                intent_text = $8,
                structured_payload = $9,
                status = $10,
                denial_reason = $11,
                approval_id = $12,
                result_agent_id = $13,
                result_run_id = $14,
                trace_id = $15,
                version = version + 1,
                updated_at = $16
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(&request.id)
        .bind(&request.idempotency_key)
        .bind(&request.requested_by_user)
        .bind(&request.requested_by_service)
        .bind(request.request_type.to_string())
        .bind(&request.agent_type)
        .bind(&request.target_resource)
        .bind(&request.intent_text)
        .bind(&request.structured_payload)
        .bind(request.status.to_string())
        .bind(&request.denial_reason)
        .bind(&request.approval_id)
        .bind(&request.result_agent_id)
        .bind(&request.result_run_id)
        .bind(&request.trace_id)
        .bind(OffsetDateTime::now_utc())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_request(&row)
    }

    async fn create_approval(&self, approval: ApprovalRequest) -> CoreResult<ApprovalRequest> {
        let row = query(
            r#"
            INSERT INTO approval_requests (
                id, request_id, requested_by_user, approver_user, status, risk_level,
                reason, decision_reason, created_at, decided_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            RETURNING *
            "#,
        )
        .bind(&approval.id)
        .bind(&approval.request_id)
        .bind(&approval.requested_by_user)
        .bind(&approval.approver_user)
        .bind(approval.status.to_string())
        .bind(approval.risk_level.map(|risk| risk.to_string()))
        .bind(&approval.reason)
        .bind(&approval.decision_reason)
        .bind(approval.created_at)
        .bind(approval.decided_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_approval(&row)
    }

    async fn get_approval(&self, approval_id: &str) -> CoreResult<Option<ApprovalRequest>> {
        let row = query("SELECT * FROM approval_requests WHERE id = $1")
            .bind(approval_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_approval).transpose()
    }

    async fn get_approval_by_request(
        &self,
        request_id: &str,
    ) -> CoreResult<Option<ApprovalRequest>> {
        let row = query("SELECT * FROM approval_requests WHERE request_id = $1 ORDER BY created_at DESC LIMIT 1")
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_approval).transpose()
    }

    async fn decide_approval(
        &self,
        approval_id: &str,
        approver_user: &str,
        status: ApprovalStatus,
        reason: Option<String>,
    ) -> CoreResult<ApprovalRequest> {
        let row = query(
            r#"
            UPDATE approval_requests SET
                approver_user = $2,
                status = $3,
                decision_reason = $4,
                decided_at = $5
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(approval_id)
        .bind(approver_user)
        .bind(status.to_string())
        .bind(reason)
        .bind(OffsetDateTime::now_utc())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_approval(&row)
    }

    async fn create_agent_instance(&self, agent: AgentInstance) -> CoreResult<AgentInstance> {
        let row = query(
            r#"
            INSERT INTO agent_instances (
                id, agent_type, hermes_profile, owner_user, target_resource,
                core_constraints_hash, status, display_name, config, trace_id,
                version, created_at, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            RETURNING *
            "#,
        )
        .bind(&agent.id)
        .bind(&agent.agent_type)
        .bind(&agent.hermes_profile)
        .bind(&agent.owner_user)
        .bind(&agent.target_resource)
        .bind(&agent.core_constraints_hash)
        .bind(agent.status.to_string())
        .bind(&agent.display_name)
        .bind(&agent.config)
        .bind(&agent.trace_id)
        .bind(agent.version)
        .bind(agent.created_at)
        .bind(agent.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_agent(&row)
    }

    async fn find_reusable_agent(
        &self,
        owner_user: &str,
        agent_type: &str,
        target_resource: &str,
        core_constraints_hash: &str,
    ) -> CoreResult<Option<AgentInstance>> {
        let row = query(
            r#"
            SELECT * FROM agent_instances
            WHERE owner_user = $1
              AND agent_type = $2
              AND target_resource = $3
              AND core_constraints_hash = $4
              AND status IN ('provisioning','running','paused','failed')
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .bind(owner_user)
        .bind(agent_type)
        .bind(target_resource)
        .bind(core_constraints_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.as_ref().map(map_agent).transpose()
    }

    async fn get_agent(&self, agent_id: &str) -> CoreResult<Option<AgentInstance>> {
        let row = query("SELECT * FROM agent_instances WHERE id = $1")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_agent).transpose()
    }

    async fn list_agents(
        &self,
        user_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<AgentSummary>> {
        let rows = query(
            r#"
            SELECT * FROM agent_instances
            WHERE ($1::text IS NULL OR owner_user = $1)
            ORDER BY updated_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let mut summaries = Vec::new();
        for row in rows {
            let agent = map_agent(&row)?;
            let active_session_count: i64 = query_scalar(
                "SELECT COUNT(*) FROM agent_sessions WHERE agent_id = $1 AND status = 'active'",
            )
            .bind(&agent.id)
            .fetch_one(&self.pool)
            .await
            .map_err(store_error)?;
            let last_run_row = query(
                "SELECT * FROM agent_runs WHERE agent_id = $1 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(&agent.id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
            let last_run = last_run_row.as_ref().map(map_run).transpose()?;
            summaries.push(AgentSummary {
                agent_id: agent.id,
                agent_type: agent.agent_type,
                display_name: agent.display_name,
                target_resource: agent.target_resource,
                status: agent.status,
                allowed_actions: json!(["analyze", "prepare_change", "run_checks"]),
                active_session_count,
                last_run_status: last_run.as_ref().map(|run| run.run_status),
                last_run_at: last_run.as_ref().map(|run| run.created_at),
                trace_id: agent.trace_id,
            });
        }
        Ok(summaries)
    }

    async fn update_agent_status(
        &self,
        agent_id: &str,
        status: AgentInstanceStatus,
        trace_id: &str,
    ) -> CoreResult<AgentInstance> {
        let row = query(
            r#"
            UPDATE agent_instances
            SET status = $2, trace_id = $3, version = version + 1, updated_at = $4
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(agent_id)
        .bind(status.to_string())
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_agent(&row)
    }

    async fn create_session(&self, session: AgentSession) -> CoreResult<AgentSession> {
        let row = query(
            r#"
            INSERT INTO agent_sessions (
                id, idempotency_key, agent_id, owner_user, source_conversation_id, parent_session_id,
                created_by_session_id, status, depth, resource_scope, context_summary,
                trace_id, version, expires_at, created_at, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
            RETURNING *
            "#,
        )
        .bind(&session.id)
        .bind(&session.idempotency_key)
        .bind(&session.agent_id)
        .bind(&session.owner_user)
        .bind(&session.source_conversation_id)
        .bind(&session.parent_session_id)
        .bind(&session.created_by_session_id)
        .bind(session.status.to_string())
        .bind(session.depth)
        .bind(&session.resource_scope)
        .bind(&session.context_summary)
        .bind(&session.trace_id)
        .bind(session.version)
        .bind(session.expires_at)
        .bind(session.created_at)
        .bind(session.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_session(&row)
    }

    async fn get_session(&self, session_id: &str) -> CoreResult<Option<AgentSession>> {
        let row = query("SELECT * FROM agent_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_session).transpose()
    }

    async fn find_session_by_idempotency(
        &self,
        owner_user: &str,
        agent_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentSession>> {
        let row = query(
            r#"
            SELECT * FROM agent_sessions
            WHERE owner_user = $1
              AND agent_id = $2
              AND idempotency_key = $3
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .bind(owner_user)
        .bind(agent_id)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.as_ref().map(map_session).transpose()
    }

    async fn list_sessions(
        &self,
        user_id: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<SessionSummary>> {
        let rows = query(
            r#"
            SELECT * FROM agent_sessions
            WHERE ($1::text IS NULL OR owner_user = $1)
              AND ($2::text IS NULL OR agent_id = $2)
            ORDER BY updated_at DESC
            LIMIT $3
            "#,
        )
        .bind(user_id)
        .bind(agent_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.iter()
            .map(map_session)
            .map(|r| r.map(|s| session_summary(&s)))
            .collect()
    }

    async fn list_child_sessions(
        &self,
        parent_session_id: &str,
    ) -> CoreResult<Vec<SessionSummary>> {
        let rows = query(
            "SELECT * FROM agent_sessions WHERE parent_session_id = $1 ORDER BY created_at ASC",
        )
        .bind(parent_session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.iter()
            .map(map_session)
            .map(|r| r.map(|s| session_summary(&s)))
            .collect()
    }

    async fn close_session(&self, session_id: &str, trace_id: &str) -> CoreResult<AgentSession> {
        let current = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| store_error("session not found"))?;
        validate_session_transition(current.status, agent_core::AgentSessionStatus::Closed)?;
        let row = query(
            r#"
            UPDATE agent_sessions
            SET status = 'closed', trace_id = $2, version = version + 1, updated_at = $3
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(session_id)
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_session(&row)
    }

    async fn next_message_sequence(&self, session_id: &str) -> CoreResult<i64> {
        let sequence: i64 = query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM agent_session_messages WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(sequence)
    }

    async fn get_open_webui_bridge_binding(
        &self,
        open_webui_subject: &str,
        open_webui_chat_id: &str,
        model: &str,
    ) -> CoreResult<Option<AgentBridgeBinding>> {
        let row = query(
            r#"
            SELECT * FROM open_webui_bridge_bindings
            WHERE open_webui_subject = $1
              AND open_webui_chat_id = $2
              AND model = $3
              AND status = 'active'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(open_webui_subject)
        .bind(open_webui_chat_id)
        .bind(model)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.as_ref().map(map_bridge_binding).transpose()
    }

    async fn upsert_open_webui_bridge_binding(
        &self,
        binding: AgentBridgeBinding,
    ) -> CoreResult<AgentBridgeBinding> {
        let row = query(
            r#"
            INSERT INTO open_webui_bridge_bindings (
                id, open_webui_subject, open_webui_chat_id, open_webui_session_id,
                model, agent_id, agent_session_id, status, last_message_id, last_run_id,
                trace_id, version, created_at, updated_at, closed_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
            ON CONFLICT (open_webui_subject, open_webui_chat_id, model)
                WHERE status = 'active'
            DO UPDATE SET
                open_webui_session_id = EXCLUDED.open_webui_session_id,
                agent_id = EXCLUDED.agent_id,
                agent_session_id = EXCLUDED.agent_session_id,
                last_message_id = EXCLUDED.last_message_id,
                trace_id = EXCLUDED.trace_id,
                version = open_webui_bridge_bindings.version + 1,
                updated_at = EXCLUDED.updated_at
            RETURNING *
            "#,
        )
        .bind(&binding.id)
        .bind(&binding.open_webui_subject)
        .bind(&binding.open_webui_chat_id)
        .bind(&binding.open_webui_session_id)
        .bind(&binding.model)
        .bind(&binding.agent_id)
        .bind(&binding.agent_session_id)
        .bind(binding.status.to_string())
        .bind(&binding.last_message_id)
        .bind(&binding.last_run_id)
        .bind(&binding.trace_id)
        .bind(binding.version)
        .bind(binding.created_at)
        .bind(OffsetDateTime::now_utc())
        .bind(binding.closed_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_bridge_binding(&row)
    }

    async fn close_open_webui_bridge_binding(
        &self,
        open_webui_subject: &str,
        open_webui_chat_id: &str,
        model: &str,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse> {
        query(
            r#"
            UPDATE open_webui_bridge_bindings
            SET status = 'closed',
                trace_id = $4,
                version = version + 1,
                updated_at = $5,
                closed_at = $5
            WHERE open_webui_subject = $1
              AND open_webui_chat_id = $2
              AND model = $3
              AND status = 'active'
            "#,
        )
        .bind(open_webui_subject)
        .bind(open_webui_chat_id)
        .bind(model)
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(EmptyResponse {
            status: "closed".to_string(),
            trace_id: trace_id.to_string(),
        })
    }

    async fn update_open_webui_bridge_run(
        &self,
        open_webui_subject: &str,
        binding_id: &str,
        message_id: Option<&str>,
        run_id: &str,
        trace_id: &str,
    ) -> CoreResult<AgentBridgeBinding> {
        let row = query(
            r#"
            UPDATE open_webui_bridge_bindings
            SET last_message_id = $2,
                last_run_id = $3,
                trace_id = $4,
                version = version + 1,
                updated_at = $5
            WHERE id = $1 AND open_webui_subject = $6 AND status = 'active'
            RETURNING *
            "#,
        )
        .bind(binding_id)
        .bind(message_id)
        .bind(run_id)
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .bind(open_webui_subject)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_bridge_binding(&row)
    }

    async fn claim_open_webui_bridge_nonce(
        &self,
        open_webui_subject: &str,
        open_webui_chat_id: &str,
        model: &str,
        nonce: &str,
        issued_at: i64,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse> {
        if open_webui_subject.trim().is_empty()
            || open_webui_chat_id.trim().is_empty()
            || model.trim().is_empty()
            || nonce.trim().is_empty()
        {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "bridge nonce fields must not be empty",
            ));
        }
        let issued_at = OffsetDateTime::from_unix_timestamp(issued_at).map_err(store_error)?;
        let inserted = query_scalar::<_, String>(
            r#"
            INSERT INTO open_webui_bridge_nonces (
                id, open_webui_subject, open_webui_chat_id, model, nonce,
                issued_at, trace_id, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            ON CONFLICT (open_webui_subject, open_webui_chat_id, model, nonce)
            DO NOTHING
            RETURNING id
            "#,
        )
        .bind(new_id("bridge_nonce"))
        .bind(open_webui_subject)
        .bind(open_webui_chat_id)
        .bind(model)
        .bind(nonce)
        .bind(issued_at)
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        if inserted.is_none() {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "bridge nonce replay",
            ));
        }
        Ok(EmptyResponse {
            status: "claimed".to_string(),
            trace_id: trace_id.to_string(),
        })
    }

    async fn create_run(&self, run: AgentRun) -> CoreResult<AgentRun> {
        let row = query(
            r#"
            INSERT INTO agent_runs (
                id, idempotency_key, agent_id, session_id, trigger_type, target_resource,
                run_status, risk_level, external_action_mode, lease_owner, lease_until, next_retry_at,
                retry_count, result_summary, result_ref, trace_id, version, created_at,
                claimed_at, finished_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)
            RETURNING *
            "#,
        )
        .bind(&run.id)
        .bind(&run.idempotency_key)
        .bind(&run.agent_id)
        .bind(&run.session_id)
        .bind(run.trigger_type.to_string())
        .bind(&run.target_resource)
        .bind(run.run_status.to_string())
        .bind(run.risk_level.to_string())
        .bind(run.external_action_mode.to_string())
        .bind(&run.lease_owner)
        .bind(run.lease_until)
        .bind(run.next_retry_at)
        .bind(run.retry_count)
        .bind(&run.result_summary)
        .bind(&run.result_ref)
        .bind(&run.trace_id)
        .bind(run.version)
        .bind(run.created_at)
        .bind(run.claimed_at)
        .bind(run.finished_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_run(&row)
    }

    async fn get_run(&self, run_id: &str) -> CoreResult<Option<AgentRun>> {
        let row = query("SELECT * FROM agent_runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_run).transpose()
    }

    async fn find_run_by_idempotency(
        &self,
        agent_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentRun>> {
        let row = query(
            r#"
            SELECT * FROM agent_runs
            WHERE agent_id = $1 AND idempotency_key = $2
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .bind(agent_id)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.as_ref().map(map_run).transpose()
    }

    async fn list_runs(
        &self,
        user_id: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<RunSummary>> {
        let rows = query(
            r#"
            SELECT r.* FROM agent_runs r
            JOIN agent_instances a ON a.id = r.agent_id
            WHERE ($1::text IS NULL OR a.owner_user = $1)
              AND ($2::text IS NULL OR r.agent_id = $2)
            ORDER BY r.created_at DESC
            LIMIT $3
            "#,
        )
        .bind(user_id)
        .bind(agent_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.iter()
            .map(map_run)
            .map(|r| r.map(|run| run_summary(&run)))
            .collect()
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        status: AgentRunStatus,
        trace_id: &str,
    ) -> CoreResult<AgentRun> {
        let current = self
            .get_run(run_id)
            .await?
            .ok_or_else(|| store_error("run not found"))?;
        validate_run_transition(current.run_status, status)?;
        let finished_at = if matches!(
            status,
            AgentRunStatus::Completed
                | AgentRunStatus::Failed
                | AgentRunStatus::TimedOut
                | AgentRunStatus::DeadLetter
                | AgentRunStatus::Cancelled
        ) {
            Some(OffsetDateTime::now_utc())
        } else {
            current.finished_at
        };
        let row = query(
            r#"
            UPDATE agent_runs
            SET run_status = $2,
                trace_id = $3,
                finished_at = $4,
                next_retry_at = NULL,
                version = version + 1
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(run_id)
        .bind(status.to_string())
        .bind(trace_id)
        .bind(finished_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_run(&row)
    }

    async fn retry_run(&self, run_id: &str, reason: &str, trace_id: &str) -> CoreResult<AgentRun> {
        let current = self
            .get_run(run_id)
            .await?
            .ok_or_else(|| store_error("run not found"))?;
        validate_run_transition(current.run_status, AgentRunStatus::Queued)?;
        let row = query(
            r#"
            UPDATE agent_runs SET
                run_status = 'queued',
                retry_count = 0,
                result_summary = $2,
                lease_owner = NULL,
                lease_until = NULL,
                next_retry_at = NULL,
                finished_at = NULL,
                trace_id = $3,
                version = version + 1
            WHERE id = $1 AND run_status = 'dead_letter'
            RETURNING *
            "#,
        )
        .bind(run_id)
        .bind(reason)
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        let Some(row) = row else {
            return Err(agent_core::AgentCoreError::coded(
                agent_core::ErrorCode::Conflict,
                "run status changed before retry",
            ));
        };
        map_run(&row)
    }

    async fn terminate_run(
        &self,
        run_id: &str,
        reason: &str,
        trace_id: &str,
    ) -> CoreResult<AgentRun> {
        let current = self
            .get_run(run_id)
            .await?
            .ok_or_else(|| store_error("run not found"))?;
        validate_run_transition(current.run_status, AgentRunStatus::Cancelled)?;
        let row = query(
            r#"
            UPDATE agent_runs SET
                run_status = 'cancelled',
                result_summary = $2,
                lease_owner = NULL,
                lease_until = NULL,
                next_retry_at = NULL,
                finished_at = $3,
                trace_id = $4,
                version = version + 1
            WHERE id = $1 AND run_status = 'dead_letter'
            RETURNING *
            "#,
        )
        .bind(run_id)
        .bind(reason)
        .bind(OffsetDateTime::now_utc())
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        let Some(row) = row else {
            return Err(agent_core::AgentCoreError::coded(
                agent_core::ErrorCode::Conflict,
                "run status changed before termination",
            ));
        };
        map_run(&row)
    }

    async fn append_audit(&self, audit: AuditLog) -> CoreResult<AuditLog> {
        let row = query(
            r#"
            INSERT INTO audit_logs (
                id, actor_user, actor_service, action, resource_type, resource_id,
                decision, reason, request_id, session_id, run_id, approval_id,
                observer_report_id, trace_id, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
            RETURNING *
            "#,
        )
        .bind(&audit.id)
        .bind(&audit.actor_user)
        .bind(&audit.actor_service)
        .bind(&audit.action)
        .bind(&audit.resource_type)
        .bind(&audit.resource_id)
        .bind(audit.decision.map(|decision| decision.to_string()))
        .bind(&audit.reason)
        .bind(&audit.request_id)
        .bind(&audit.session_id)
        .bind(&audit.run_id)
        .bind(&audit.approval_id)
        .bind(&audit.observer_report_id)
        .bind(&audit.trace_id)
        .bind(audit.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_audit(&row)
    }

    async fn list_audit(&self, limit: i64) -> CoreResult<Vec<AuditLog>> {
        let rows = query("SELECT * FROM audit_logs ORDER BY created_at DESC LIMIT $1")
            .bind(limit.max(1))
            .fetch_all(&self.pool)
            .await
            .map_err(store_error)?;
        rows.iter().map(map_audit).collect()
    }

    async fn create_observer_report(&self, report: ObserverReport) -> CoreResult<ObserverReport> {
        let row = query(
            r#"
            INSERT INTO observer_reports (
                id, observer_run_id, health_status, risk_level, summary, findings,
                recommendations, evidence_refs, trace_id, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            RETURNING *
            "#,
        )
        .bind(&report.id)
        .bind(&report.observer_run_id)
        .bind(report.health_status.to_string())
        .bind(report.risk_level.map(|risk| risk.to_string()))
        .bind(&report.summary)
        .bind(&report.findings)
        .bind(&report.recommendations)
        .bind(&report.evidence_refs)
        .bind(&report.trace_id)
        .bind(report.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_report(&row)
    }

    async fn list_observer_reports(&self, limit: i64) -> CoreResult<Vec<ObserverReportSummary>> {
        let rows = query("SELECT * FROM observer_reports ORDER BY created_at DESC LIMIT $1")
            .bind(limit.max(1))
            .fetch_all(&self.pool)
            .await
            .map_err(store_error)?;
        let mut items = Vec::new();
        for row in rows {
            let report = map_report(&row)?;
            items.push(ObserverReportSummary {
                report_id: report.id,
                observer_run_id: report.observer_run_id,
                health_status: report.health_status,
                risk_level: report.risk_level,
                summary: report.summary,
                created_at: report.created_at,
                trace_id: report.trace_id,
            });
        }
        Ok(items)
    }

    async fn get_observer_report(&self, report_id: &str) -> CoreResult<Option<ObserverReport>> {
        let row = query("SELECT * FROM observer_reports WHERE id = $1")
            .bind(report_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_report).transpose()
    }

    async fn create_external_action_plan(
        &self,
        plan: ExternalActionPlan,
    ) -> CoreResult<ExternalActionPlan> {
        let row = query(
            r#"
            INSERT INTO external_action_plans (
                id, run_id, connector, action, resource_ref, risk_level,
                external_action_mode, approval_id, credential_scope, input_summary,
                input_ref, result_ref, compensation_ref, compensation_result_ref, status, error_code,
                trace_id, version, created_at, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)
            RETURNING *
            "#,
        )
        .bind(&plan.id)
        .bind(&plan.run_id)
        .bind(&plan.connector)
        .bind(&plan.action)
        .bind(&plan.resource_ref)
        .bind(plan.risk_level.to_string())
        .bind(plan.external_action_mode.to_string())
        .bind(&plan.approval_id)
        .bind(&plan.credential_scope)
        .bind(&plan.input_summary)
        .bind(&plan.input_ref)
        .bind(&plan.result_ref)
        .bind(&plan.compensation_ref)
        .bind(&plan.compensation_result_ref)
        .bind(plan.status.to_string())
        .bind(&plan.error_code)
        .bind(&plan.trace_id)
        .bind(plan.version)
        .bind(plan.created_at)
        .bind(plan.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_external_action_plan(&row)
    }

    async fn get_external_action_plan(
        &self,
        plan_id: &str,
    ) -> CoreResult<Option<ExternalActionPlan>> {
        let row = query("SELECT * FROM external_action_plans WHERE id = $1")
            .bind(plan_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        row.as_ref().map(map_external_action_plan).transpose()
    }

    async fn list_external_action_plans_by_run(
        &self,
        run_id: &str,
    ) -> CoreResult<Vec<ExternalActionPlan>> {
        let rows =
            query("SELECT * FROM external_action_plans WHERE run_id = $1 ORDER BY created_at DESC")
                .bind(run_id)
                .fetch_all(&self.pool)
                .await
                .map_err(store_error)?;
        rows.iter().map(map_external_action_plan).collect()
    }

    async fn update_external_action_plan_status(
        &self,
        plan_id: &str,
        status: agent_core::ExternalActionPlanStatus,
        result_ref: Option<&str>,
        compensation_ref: Option<&str>,
        error_code: Option<&str>,
        trace_id: &str,
    ) -> CoreResult<ExternalActionPlan> {
        let row = query(
            r#"
            UPDATE external_action_plans
            SET status = $2,
                result_ref = $3,
                compensation_ref = $4,
                error_code = $5,
                trace_id = $6,
                version = version + 1,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(plan_id)
        .bind(status.to_string())
        .bind(result_ref)
        .bind(compensation_ref)
        .bind(error_code)
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or_else(|| {
            agent_core::AgentCoreError::coded(agent_core::ErrorCode::NotFound, "not found")
        })?;
        map_external_action_plan(&row)
    }

    async fn record_external_action_compensation(
        &self,
        plan_id: &str,
        compensation_result_ref: &str,
        trace_id: &str,
    ) -> CoreResult<ExternalActionPlan> {
        let row = query(
            r#"
            UPDATE external_action_plans
            SET status = 'compensated',
                compensation_result_ref = $2,
                error_code = NULL,
                trace_id = $3,
                version = version + 1,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(plan_id)
        .bind(compensation_result_ref)
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or_else(|| {
            agent_core::AgentCoreError::coded(agent_core::ErrorCode::NotFound, "not found")
        })?;
        map_external_action_plan(&row)
    }

    async fn create_credential_lease(&self, lease: CredentialLease) -> CoreResult<CredentialLease> {
        let row = query(
            r#"
            INSERT INTO credential_leases (
                id, external_action_plan_id, credential_scope, provider_ref, status,
                expires_at, trace_id, revoked_at, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            RETURNING *
            "#,
        )
        .bind(&lease.id)
        .bind(&lease.external_action_plan_id)
        .bind(&lease.credential_scope)
        .bind(&lease.provider_ref)
        .bind(lease.status.to_string())
        .bind(lease.expires_at)
        .bind(&lease.trace_id)
        .bind(lease.revoked_at)
        .bind(lease.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_credential_lease(&row)
    }

    async fn list_credential_leases_by_plan(
        &self,
        plan_id: &str,
    ) -> CoreResult<Vec<CredentialLease>> {
        let rows = query(
            "SELECT * FROM credential_leases WHERE external_action_plan_id = $1 ORDER BY created_at DESC",
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.iter().map(map_credential_lease).collect()
    }

    async fn create_grant(&self, grant: AgentGrant) -> CoreResult<AgentGrant> {
        query(
            r#"
            INSERT INTO agent_grants (
                id, subject_type, subject_id, action, resource_type, resource_id,
                constraints, granted_by, created_at, expires_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            "#,
        )
        .bind(&grant.id)
        .bind(&grant.subject_type)
        .bind(&grant.subject_id)
        .bind(&grant.action)
        .bind(&grant.resource_type)
        .bind(&grant.resource_id)
        .bind(&grant.constraints)
        .bind(&grant.granted_by)
        .bind(grant.created_at)
        .bind(grant.expires_at)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(grant)
    }

    async fn acquire_resource_lock(
        &self,
        mut lock: ResourceLock,
        lease: Duration,
    ) -> CoreResult<ResourceLock> {
        lock.lease_until = lease_until(lease);
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        query(
            r#"
            DELETE FROM resource_locks
            WHERE resource_type = $1 AND resource_id = $2 AND lock_scope = $3
              AND (lease_until < $4 OR holder_run_id = $5)
            "#,
        )
        .bind(&lock.resource_type)
        .bind(&lock.resource_id)
        .bind(&lock.lock_scope)
        .bind(OffsetDateTime::now_utc())
        .bind(&lock.holder_run_id)
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
        let inserted = query(
            r#"
            INSERT INTO resource_locks (
                id, resource_type, resource_id, lock_scope, holder_run_id, lease_until, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7)
            ON CONFLICT DO NOTHING
            RETURNING *
            "#,
        )
        .bind(&lock.id)
        .bind(&lock.resource_type)
        .bind(&lock.resource_id)
        .bind(&lock.lock_scope)
        .bind(&lock.holder_run_id)
        .bind(lock.lease_until)
        .bind(lock.created_at)
        .fetch_optional(&mut *tx)
        .await
        .map_err(store_error)?;
        tx.commit().await.map_err(store_error)?;
        if inserted.is_none() {
            return Err(agent_core::AgentCoreError::coded(
                agent_core::ErrorCode::Conflict,
                "resource lock is already held",
            ));
        }
        Ok(lock)
    }

    async fn active_resource_lock(
        &self,
        resource_type: &str,
        resource_id: &str,
        lock_scope: &str,
    ) -> CoreResult<Option<ResourceLock>> {
        let row = query(
            r#"
            SELECT *
            FROM resource_locks
            WHERE resource_type = $1
              AND resource_id = $2
              AND lock_scope = $3
              AND lease_until > $4
            ORDER BY lease_until DESC
            LIMIT 1
            "#,
        )
        .bind(resource_type)
        .bind(resource_id)
        .bind(lock_scope)
        .bind(OffsetDateTime::now_utc())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.map(|row| {
            Ok(ResourceLock {
                id: row.try_get("id").map_err(store_error)?,
                resource_type: row.try_get("resource_type").map_err(store_error)?,
                resource_id: row.try_get("resource_id").map_err(store_error)?,
                lock_scope: row.try_get("lock_scope").map_err(store_error)?,
                holder_run_id: row.try_get("holder_run_id").map_err(store_error)?,
                lease_until: row.try_get("lease_until").map_err(store_error)?,
                created_at: row.try_get("created_at").map_err(store_error)?,
            })
        })
        .transpose()
    }

    async fn release_resource_lock(&self, run_id: &str) -> CoreResult<EmptyResponse> {
        query("DELETE FROM resource_locks WHERE holder_run_id = $1")
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(EmptyResponse {
            status: "released".to_string(),
            trace_id: agent_core::new_trace_id(),
        })
    }

    async fn record_worker_heartbeat(
        &self,
        worker_id: &str,
        current_run_id: Option<&str>,
        status: &str,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse> {
        query(
            r#"
            INSERT INTO worker_heartbeats (worker_id, current_run_id, status, trace_id, last_seen_at)
            VALUES ($1,$2,$3,$4,$5)
            ON CONFLICT (worker_id) DO UPDATE SET
                current_run_id = EXCLUDED.current_run_id,
                status = EXCLUDED.status,
                trace_id = EXCLUDED.trace_id,
                last_seen_at = EXCLUDED.last_seen_at
            "#,
        )
        .bind(worker_id)
        .bind(current_run_id)
        .bind(status)
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(EmptyResponse {
            status: "recorded".to_string(),
            trace_id: trace_id.to_string(),
        })
    }
}

#[async_trait]
impl MemoryStore for PgAgentStore {
    async fn append_message(
        &self,
        message: AgentSessionMessage,
    ) -> CoreResult<AgentSessionMessage> {
        let row = query(
            r#"
            INSERT INTO agent_session_messages (
                id, session_id, sequence, role, content_ref, content_summary,
                external_message_id, run_id, trace_id, created_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            ON CONFLICT (session_id, external_message_id)
                WHERE external_message_id IS NOT NULL
            DO UPDATE SET
                trace_id = agent_session_messages.trace_id
            RETURNING *
            "#,
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(message.sequence)
        .bind(message.role.to_string())
        .bind(&message.content_ref)
        .bind(&message.content_summary)
        .bind(&message.external_message_id)
        .bind(&message.run_id)
        .bind(&message.trace_id)
        .bind(message.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        query(
            "UPDATE agent_sessions SET updated_at = $2, trace_id = $3, version = version + 1 WHERE id = $1",
        )
        .bind(&message.session_id)
        .bind(OffsetDateTime::now_utc())
        .bind(&message.trace_id)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        map_message(&row)
    }

    async fn session_context(
        &self,
        session_id: &str,
        trace_id: &str,
    ) -> CoreResult<SessionContext> {
        let session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| store_error("session not found"))?;
        let rows = query(
            r#"
            SELECT * FROM agent_session_messages
            WHERE session_id = $1
            ORDER BY sequence DESC
            LIMIT 30
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let mut messages = rows
            .iter()
            .map(map_message)
            .map(|message| {
                message.map(|message| AppendMessageInput {
                    role: message.role,
                    content_summary: message.content_summary.unwrap_or_default(),
                    content_ref: message.content_ref,
                    external_message_id: message.external_message_id,
                    run_id: message.run_id,
                })
            })
            .collect::<CoreResult<Vec<_>>>()?;
        messages.reverse();
        Ok(SessionContext {
            session_id: session.id,
            agent_id: session.agent_id,
            context_summary: session.context_summary,
            recent_messages: messages,
            resource_scope: session.resource_scope,
            trace_id: trace_id.to_string(),
        })
    }

    async fn write_summary(
        &self,
        session_id: &str,
        summary: &str,
        trace_id: &str,
    ) -> CoreResult<()> {
        query(
            r#"
            UPDATE agent_sessions
            SET context_summary = $2, trace_id = $3, version = version + 1, updated_at = $4
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .bind(summary)
        .bind(trace_id)
        .bind(OffsetDateTime::now_utc())
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }

    async fn write_result_ref(
        &self,
        run_id: &str,
        result_summary: &str,
        result_ref: Option<&str>,
        trace_id: &str,
    ) -> CoreResult<()> {
        query(
            r#"
            UPDATE agent_runs
            SET result_summary = $2, result_ref = $3, trace_id = $4, version = version + 1
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(result_summary)
        .bind(result_ref)
        .bind(trace_id)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }
}

#[async_trait]
impl RunQueue for PgAgentStore {
    async fn enqueue_run(&self, run: AgentRun) -> CoreResult<AgentRun> {
        self.create_run(run).await
    }

    async fn claim_next_run(
        &self,
        worker_id: &str,
        lease: Duration,
    ) -> CoreResult<Option<RunClaim>> {
        let lease_until = lease_until(lease);
        let now = OffsetDateTime::now_utc();
        let row = query(
            r#"
            WITH candidate AS (
                SELECT id FROM agent_runs
                WHERE run_status = 'queued'
                  AND (next_retry_at IS NULL OR next_retry_at <= $3)
                ORDER BY COALESCE(next_retry_at, created_at), created_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE agent_runs SET
                run_status = 'claimed',
                lease_owner = $1,
                lease_until = $2,
                claimed_at = $3,
                next_retry_at = NULL,
                version = version + 1
            WHERE id = (SELECT id FROM candidate)
            RETURNING *
            "#,
        )
        .bind(worker_id)
        .bind(lease_until)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        row.as_ref().map(map_run).transpose().map(|run| {
            run.map(|run| RunClaim {
                run,
                lease_owner: worker_id.to_string(),
                lease_seconds: lease.as_secs() as i64,
            })
        })
    }

    async fn heartbeat_run(
        &self,
        run_id: &str,
        worker_id: &str,
        lease: Duration,
    ) -> CoreResult<()> {
        let row = query(
            r#"
            UPDATE agent_runs
            SET lease_until = $3, version = version + 1
            WHERE id = $1 AND lease_owner = $2
            RETURNING id
            "#,
        )
        .bind(run_id)
        .bind(worker_id)
        .bind(lease_until(lease))
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        if row.is_none() {
            return Err(agent_core::AgentCoreError::coded(
                agent_core::ErrorCode::Conflict,
                "lease owner mismatch",
            ));
        }
        Ok(())
    }

    async fn finish_run(&self, run_id: &str, output: RuntimeOutput) -> CoreResult<AgentRun> {
        let row = query(
            r#"
            UPDATE agent_runs SET
                run_status = 'completed',
                result_summary = $2,
                result_ref = $3,
                lease_owner = NULL,
                lease_until = NULL,
                next_retry_at = NULL,
                finished_at = $4,
                version = version + 1
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(run_id)
        .bind(output.result_summary)
        .bind(output.result_ref)
        .bind(OffsetDateTime::now_utc())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_run(&row)
    }

    async fn fail_or_retry_run(
        &self,
        run_id: &str,
        reason: &str,
        max_retries: i32,
    ) -> CoreResult<AgentRun> {
        let current = self
            .get_run(run_id)
            .await?
            .ok_or_else(|| store_error("run not found"))?;
        let next_status = if current.retry_count >= max_retries {
            AgentRunStatus::DeadLetter
        } else {
            AgentRunStatus::Queued
        };
        let next_retry = if next_status == AgentRunStatus::Queued {
            current.retry_count + 1
        } else {
            current.retry_count
        };
        let next_retry_at = if next_status == AgentRunStatus::Queued {
            Some(OffsetDateTime::now_utc() + retry_backoff(next_retry))
        } else {
            None
        };
        let finished_at = if next_status == AgentRunStatus::DeadLetter {
            Some(OffsetDateTime::now_utc())
        } else {
            None
        };
        let row = query(
            r#"
            UPDATE agent_runs SET
                run_status = $2,
                retry_count = $3,
                result_summary = $4,
                lease_owner = NULL,
                lease_until = NULL,
                next_retry_at = $5,
                finished_at = $6,
                version = version + 1
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(run_id)
        .bind(next_status.to_string())
        .bind(next_retry)
        .bind(reason)
        .bind(next_retry_at)
        .bind(finished_at)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_run(&row)
    }

    async fn dead_letter_run(&self, run_id: &str, reason: &str) -> CoreResult<AgentRun> {
        let row = query(
            r#"
            UPDATE agent_runs SET
                run_status = 'dead_letter',
                result_summary = $2,
                lease_owner = NULL,
                lease_until = NULL,
                next_retry_at = NULL,
                finished_at = $3,
                version = version + 1
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(run_id)
        .bind(reason)
        .bind(OffsetDateTime::now_utc())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        map_run(&row)
    }

    async fn sweep_expired_leases(&self, max_retries: i32) -> CoreResult<Vec<RunSummary>> {
        let rows = query(
            r#"
            SELECT * FROM agent_runs
            WHERE run_status IN ('claimed','context_built','policy_checked','executing','validating','applying_external_actions')
              AND lease_until IS NOT NULL
              AND lease_until < $1
            ORDER BY lease_until ASC
            LIMIT 100
            "#,
        )
        .bind(OffsetDateTime::now_utc())
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let mut swept = Vec::new();
        for row in rows {
            let run = map_run(&row)?;
            let updated = self
                .fail_or_retry_run(&run.id, "lease expired", max_retries)
                .await?;
            swept.push(run_summary(&updated));
        }
        Ok(swept)
    }
}

#[async_trait]
impl ObserverSnapshotStore for PgAgentStore {
    async fn collect_observer_snapshot(&self, _trace_id: &str) -> CoreResult<ObserverSnapshot> {
        async fn counts(pool: &PgPool, sql: &str) -> CoreResult<Value> {
            let rows = query(sql).fetch_all(pool).await.map_err(store_error)?;
            let mut map = serde_json::Map::new();
            for row in rows {
                let status: String = row.try_get("status").map_err(store_error)?;
                let count: i64 = row.try_get("count").map_err(store_error)?;
                map.insert(status, json!(count));
            }
            Ok(Value::Object(map))
        }

        let agent_counts = counts(
            &self.pool,
            "SELECT status, COUNT(*) AS count FROM agent_instances GROUP BY status",
        )
        .await?;
        let session_counts = counts(
            &self.pool,
            "SELECT status, COUNT(*) AS count FROM agent_sessions GROUP BY status",
        )
        .await?;
        let run_counts = counts(
            &self.pool,
            "SELECT run_status AS status, COUNT(*) AS count FROM agent_runs GROUP BY run_status",
        )
        .await?;
        let runtime_row = query(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE retry_count > 0) AS retrying_runs,
                COALESCE(MAX(retry_count), 0)::BIGINT AS max_retry_count,
                COUNT(*) FILTER (WHERE run_status = 'timed_out') AS timed_out_runs,
                COALESCE(AVG(EXTRACT(EPOCH FROM (finished_at - claimed_at)) * 1000), 0)::DOUBLE PRECISION AS avg_completed_runtime_ms
            FROM agent_runs
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        let context_row = query(
            r#"
            SELECT COALESCE(MAX(message_count), 0)::BIGINT AS max_context_messages
            FROM (
                SELECT session_id, COUNT(*) AS message_count
                FROM agent_session_messages
                GROUP BY session_id
            ) counts
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        let external_action_plan_counts = counts(
            &self.pool,
            "SELECT status, COUNT(*) AS count FROM external_action_plans GROUP BY status",
        )
        .await?;
        let external_action_plan_error_counts = counts(
            &self.pool,
            "SELECT error_code AS status, COUNT(*) AS count FROM external_action_plans WHERE error_code IS NOT NULL GROUP BY error_code",
        )
        .await?;
        let lock_rows = query(
            r#"
            SELECT resource_type, resource_id, lock_scope, holder_run_id, lease_until
            FROM resource_locks
            ORDER BY lease_until ASC
            LIMIT 100
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let locks = lock_rows
            .iter()
            .map(|row| {
                Ok(json!({
                    "resource_type": row.try_get::<String, _>("resource_type").map_err(store_error)?,
                    "resource_id": row.try_get::<String, _>("resource_id").map_err(store_error)?,
                    "lock_scope": row.try_get::<String, _>("lock_scope").map_err(store_error)?,
                    "holder_run_id": row.try_get::<String, _>("holder_run_id").map_err(store_error)?,
                    "lease_until": row.try_get::<OffsetDateTime, _>("lease_until").map_err(store_error)?,
                }))
            })
            .collect::<CoreResult<Vec<_>>>()?;
        let audit_rows = query(
            r#"
            SELECT action, decision, trace_id, created_at
            FROM audit_logs
            ORDER BY created_at DESC
            LIMIT 50
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let audits = audit_rows
            .iter()
            .map(|row| {
                Ok(json!({
                    "action": row.try_get::<String, _>("action").map_err(store_error)?,
                    "decision": row.try_get::<Option<String>, _>("decision").map_err(store_error)?,
                    "trace_id": row.try_get::<String, _>("trace_id").map_err(store_error)?,
                    "created_at": row.try_get::<OffsetDateTime, _>("created_at").map_err(store_error)?,
                }))
            })
            .collect::<CoreResult<Vec<_>>>()?;
        let worker_rows = query(
            r#"
            SELECT worker_id, current_run_id, status, trace_id, last_seen_at
            FROM worker_heartbeats
            ORDER BY last_seen_at DESC
            LIMIT 100
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let workers = worker_rows
            .iter()
            .map(|row| {
                Ok(json!({
                    "worker_id": row.try_get::<String, _>("worker_id").map_err(store_error)?,
                    "current_run_id": row.try_get::<Option<String>, _>("current_run_id").map_err(store_error)?,
                    "status": row.try_get::<String, _>("status").map_err(store_error)?,
                    "trace_id": row.try_get::<String, _>("trace_id").map_err(store_error)?,
                    "last_seen_at": row.try_get::<OffsetDateTime, _>("last_seen_at").map_err(store_error)?,
                }))
            })
            .collect::<CoreResult<Vec<_>>>()?;
        Ok(ObserverSnapshot {
            collected_at: OffsetDateTime::now_utc(),
            agent_counts,
            session_counts,
            run_counts,
            runtime_summary: json!({
                "retrying_runs": runtime_row.try_get::<i64, _>("retrying_runs").map_err(store_error)?,
                "max_retry_count": runtime_row.try_get::<i64, _>("max_retry_count").map_err(store_error)?,
                "timed_out_runs": runtime_row.try_get::<i64, _>("timed_out_runs").map_err(store_error)?,
                "avg_completed_runtime_ms": runtime_row.try_get::<f64, _>("avg_completed_runtime_ms").map_err(store_error)?,
                "max_context_messages": context_row.try_get::<i64, _>("max_context_messages").map_err(store_error)?,
                "external_action_plan_counts": external_action_plan_counts,
                "external_action_plan_error_counts": external_action_plan_error_counts,
            }),
            lock_summary: json!({
                "active_locks": locks.len(),
                "locks": locks,
            }),
            audit_summary: json!({
                "recent_decisions": audits,
            }),
            worker_summary: json!({
                "workers": workers,
            }),
        })
    }
}
