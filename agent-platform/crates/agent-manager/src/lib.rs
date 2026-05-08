mod control_services;
mod http_support;
mod lifecycle_services;
mod request_services;
mod run_admin_services;
mod telemetry_support;

use agent_core::{
    AgentCoreError, AgentGrant, AgentInstance, AgentRequestInput, AgentRequestResponse,
    AgentRequestStatus, AgentRun, AgentSession, AgentSessionMessage, AppendMessageInput,
    ApprovalDecisionInput, ApprovalStatus, AuditDecision, AuditLog, AuthContext, CoreResult,
    CreateChildSessionInput, CreateGrantInput, CreateRunInput, CreateSessionInput,
    DenyDecisionInput, EmptyResponse, ErrorCode, HealthStatus, ObserverReport, Page, RiskLevel,
    RoleName, RunAdminDecisionInput, RunSummary, WebhookTriggerInput, actions, new_id,
};
use agent_store::{AgentStore, MemoryAgentStore, PgAgentStore};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get, post},
};
pub use http_support::{ApiError, ManagerConfig, extract_auth};
use http_support::{ensure_admin, ensure_service_allows};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tower_http::trace::TraceLayer;
use tracing::Instrument;

pub type StoreRef = Arc<dyn AgentStore>;

#[derive(Clone)]
pub struct AppState {
    pub store: StoreRef,
    pub config: Arc<ManagerConfig>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    limit: Option<i64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/my-agents", get(my_agents))
        .route("/v1/my-runs", get(my_runs))
        .route("/v1/my-sessions", get(my_sessions))
        .route("/v1/agent-requests", post(create_agent_request))
        .route("/v1/agent-requests/{request_id}", get(get_agent_request))
        .route(
            "/v1/agent-requests/{request_id}/cancel",
            post(cancel_agent_request),
        )
        .route("/v1/my-agents/{agent_id}/runs", post(create_run))
        .route("/v1/my-agents/{agent_id}/sessions", post(create_session))
        .route("/v1/agent-sessions/{session_id}", get(get_session))
        .route(
            "/v1/agent-sessions/{session_id}/messages",
            post(append_message),
        )
        .route(
            "/v1/agent-sessions/{session_id}/child-sessions",
            post(create_child_session),
        )
        .route(
            "/v1/agent-sessions/{session_id}/children",
            get(list_children),
        )
        .route("/v1/agent-sessions/{session_id}/close", post(close_session))
        .route("/v1/admin/requests", get(admin_requests))
        .route(
            "/v1/admin/requests/{request_id}/approve",
            post(admin_approve),
        )
        .route("/v1/admin/requests/{request_id}/deny", post(admin_deny))
        .route("/v1/admin/agents", get(admin_agents))
        .route("/v1/admin/agents/{agent_id}/pause", post(admin_pause_agent))
        .route(
            "/v1/admin/agents/{agent_id}/resume",
            post(admin_resume_agent),
        )
        .route("/v1/admin/agents/{agent_id}", delete(admin_delete_agent))
        .route("/v1/admin/audit", get(admin_audit))
        .route("/v1/admin/runs", get(admin_runs))
        .route("/v1/admin/runs/{run_id}", get(admin_run))
        .route("/v1/admin/runs/{run_id}/retry", post(admin_retry_run))
        .route(
            "/v1/admin/runs/{run_id}/terminate",
            post(admin_terminate_run),
        )
        .route("/v1/admin/grants", post(admin_create_grant))
        .route("/v1/admin/observer/reports", get(admin_observer_reports))
        .route(
            "/v1/admin/observer/reports/{report_id}",
            get(admin_observer_report),
        )
        .route("/v1/admin/observer/runs", post(admin_observer_run))
        .route("/v1/internal/webhooks/{connector}", post(internal_webhook))
        .route("/v1/internal/runs", post(internal_create_run))
        .route("/v1/internal/runs/claim", post(internal_claim_run))
        .route(
            "/v1/internal/runs/{run_id}/heartbeat",
            post(internal_heartbeat_run),
        )
        .route(
            "/v1/internal/runs/{run_id}/finish",
            post(internal_finish_run),
        )
        .route(
            "/v1/internal/runs/{run_id}/dead-letter",
            post(internal_dead_letter_run),
        )
        .route(
            "/v1/internal/sessions/{session_id}/messages",
            post(internal_append_message),
        )
        .route(
            "/v1/internal/sessions/{session_id}/context",
            get(internal_session_context),
        )
        .route(
            "/v1/internal/memory/summaries",
            post(internal_memory_summary),
        )
        .route("/v1/internal/observer/tick", post(internal_observer_tick))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

pub async fn build_state_from_env() -> CoreResult<AppState> {
    let config = Arc::new(ManagerConfig::from_env());
    let store: StoreRef = if let Ok(database_url) = std::env::var("DATABASE_URL") {
        let pg = PgAgentStore::connect(&database_url, 10).await?;
        pg.bootstrap().await?;
        Arc::new(pg)
    } else {
        let memory = MemoryAgentStore::new();
        memory.bootstrap().await?;
        Arc::new(memory)
    };
    Ok(AppState { store, config })
}

pub async fn serve(bind: SocketAddr) -> anyhow::Result<()> {
    let state = build_state_from_env().await?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "agent-manager listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

fn auth(headers: &HeaderMap, state: &AppState) -> Result<AuthContext, ApiError> {
    extract_auth(headers, &state.config)
}

async fn audit(
    state: &AppState,
    auth: Option<&AuthContext>,
    action: &str,
    decision: AuditDecision,
    reason: Option<String>,
    trace_id: &str,
) {
    let _ = state
        .store
        .append_audit(AuditLog::new(
            auth,
            action,
            decision,
            reason,
            trace_id.to_string(),
        ))
        .await;
}

async fn create_agent_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AgentRequestInput>,
) -> Result<Json<AgentRequestResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    let span = tracing::info_span!(
        "manager.create_agent_request",
        trace_id = %auth.trace_id,
        user_id = %auth.user_id,
        service_id = %auth.service_id,
        request_type = %input.request_type
    );
    request_services::submit_request(&state.store, &auth, input)
        .instrument(span)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        .map(Json)
}

async fn get_agent_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<AgentRequestResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    let request = state
        .store
        .get_agent_request(&request_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    if request.requested_by_user != auth.user_id
        && !auth.has_any_role(&[RoleName::SystemAdmin, RoleName::AgentAdmin])
    {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::NotFound, "not found"),
            auth.trace_id,
        ));
    }
    Ok(Json(request_services::request_response(&request)))
}

async fn cancel_agent_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<AgentRequestResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    let mut request = state
        .store
        .get_agent_request(&request_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    if request.requested_by_user != auth.user_id {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::NotFound, "not found"),
            auth.trace_id,
        ));
    }
    request.status = AgentRequestStatus::Cancelled;
    request = state
        .store
        .update_agent_request(request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        "request:cancel",
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(request_services::request_response(&request)))
}

async fn my_agents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    let items = state
        .store
        .list_agents(Some(&auth.user_id), query.limit.unwrap_or(50))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn my_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    let items = state
        .store
        .list_runs(Some(&auth.user_id), None, query.limit.unwrap_or(50))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn my_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    let items = state
        .store
        .list_sessions(Some(&auth.user_id), None, query.limit.unwrap_or(50))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(input): Json<CreateSessionInput>,
) -> Result<Json<AgentSession>, ApiError> {
    let auth = auth(&headers, &state)?;
    lifecycle_services::create_session(&state.store, &auth, agent_id, input)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        .map(Json)
}

async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<SessionEnvelope>, ApiError> {
    let auth = auth(&headers, &state)?;
    let session = state
        .store
        .get_session(&session_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    if session.owner_user != auth.user_id
        && !auth.has_any_role(&[RoleName::SystemAdmin, RoleName::AgentAdmin])
    {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::NotFound, "not found"),
            auth.trace_id,
        ));
    }
    let context = state
        .store
        .session_context(&session_id, &auth.trace_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(SessionEnvelope { session, context }))
}

#[derive(Debug, Serialize)]
struct SessionEnvelope {
    session: AgentSession,
    context: agent_core::SessionContext,
}

async fn append_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(input): Json<AppendMessageInput>,
) -> Result<Json<AgentSessionMessage>, ApiError> {
    let auth = auth(&headers, &state)?;
    control_services::append_message_to_session(
        &state.store,
        &auth,
        &session_id,
        input,
        true,
        actions::SESSION_APPEND_MESSAGE,
    )
    .await
    .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
    .map(Json)
}

async fn create_child_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(parent_session_id): Path<String>,
    Json(input): Json<CreateChildSessionInput>,
) -> Result<Json<AgentSession>, ApiError> {
    let auth = auth(&headers, &state)?;
    lifecycle_services::create_child_session(&state.store, &auth, parent_session_id, input)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        .map(Json)
}

async fn list_children(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    let items = state
        .store
        .list_child_sessions(&session_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn close_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<AgentSession>, ApiError> {
    let auth = auth(&headers, &state)?;
    let session = state
        .store
        .close_session(&session_id, &auth.trace_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(session))
}

async fn create_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(input): Json<CreateRunInput>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    lifecycle_services::create_run(&state.store, &auth, agent_id, input)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        .map(Json)
}

async fn admin_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_APPROVE)?;
    let items = state
        .store
        .list_agent_requests(None, &[], query.limit.unwrap_or(100))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn admin_approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
    Json(input): Json<ApprovalDecisionInput>,
) -> Result<Json<AgentRequestResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_APPROVE)?;
    let mut request = state
        .store
        .get_agent_request(&request_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    let approval = state
        .store
        .get_approval_by_request(&request_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "approval not found"),
                auth.trace_id.clone(),
            )
        })?;
    state
        .store
        .decide_approval(
            &approval.id,
            &auth.user_id,
            ApprovalStatus::Approved,
            input.reason,
        )
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    request.status = AgentRequestStatus::Approved;
    request = state
        .store
        .update_agent_request(request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::ADMIN_APPROVE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    let response = request_services::fulfill_request(&state.store, &auth, request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(response))
}

async fn admin_deny(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
    Json(input): Json<DenyDecisionInput>,
) -> Result<Json<AgentRequestResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_DENY)?;
    let mut request = state
        .store
        .get_agent_request(&request_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    if let Some(approval) = state
        .store
        .get_approval_by_request(&request_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
    {
        state
            .store
            .decide_approval(
                &approval.id,
                &auth.user_id,
                ApprovalStatus::Denied,
                input.reason.clone(),
            )
            .await
            .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    }
    request.status = AgentRequestStatus::Denied;
    request.denial_reason = input.reason;
    request = state
        .store
        .update_agent_request(request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::ADMIN_DENY,
        AuditDecision::Denied,
        request.denial_reason.clone(),
        &auth.trace_id,
    )
    .await;
    Ok(Json(request_services::request_response(&request)))
}

async fn admin_agents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_AGENT_PAUSE)?;
    let items = state
        .store
        .list_agents(None, query.limit.unwrap_or(100))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn admin_pause_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentInstance>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_AGENT_PAUSE)?;
    let agent = state
        .store
        .update_agent_status(
            &agent_id,
            agent_core::AgentInstanceStatus::Paused,
            &auth.trace_id,
        )
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::ADMIN_AGENT_PAUSE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(agent))
}

async fn admin_resume_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentInstance>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_AGENT_RESUME)?;
    let agent = state
        .store
        .update_agent_status(
            &agent_id,
            agent_core::AgentInstanceStatus::Running,
            &auth.trace_id,
        )
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::ADMIN_AGENT_RESUME,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(agent))
}

async fn admin_delete_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentInstance>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_AGENT_PAUSE)?;
    let agent = state
        .store
        .update_agent_status(
            &agent_id,
            agent_core::AgentInstanceStatus::Terminated,
            &auth.trace_id,
        )
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        "admin:agent_terminate",
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(agent))
}

async fn admin_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_AUDIT_READ)?;
    let items = state
        .store
        .list_audit(query.limit.unwrap_or(100))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn admin_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_RUN_READ)?;
    let items: Vec<RunSummary> = state
        .store
        .list_runs(None, None, query.limit.unwrap_or(100))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn admin_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_RUN_READ)?;
    let run = state
        .store
        .get_run(&run_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    Ok(Json(run))
}

async fn admin_retry_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RunAdminDecisionInput>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_RUN_RETRY)?;
    run_admin_services::retry_dead_letter_run(&state.store, &auth, &run_id, input.reason)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        .map(Json)
}

async fn admin_terminate_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RunAdminDecisionInput>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_RUN_TERMINATE)?;
    run_admin_services::terminate_dead_letter_run(&state.store, &auth, &run_id, input.reason)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        .map(Json)
}

async fn admin_create_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateGrantInput>,
) -> Result<Json<AgentGrant>, ApiError> {
    let auth = auth(&headers, &state)?;
    tracing::info!(
        trace_id = %auth.trace_id,
        user_id = %auth.user_id,
        subject_type = %input.subject_type,
        action = %input.action,
        resource_type = %input.resource_type,
        "manager admin create grant"
    );
    telemetry_support::record_request_metric(actions::ADMIN_GRANT_CREATE, &auth.service_id);
    ensure_admin(&auth, actions::ADMIN_GRANT_CREATE)?;
    let grant = control_services::create_grant(&state.store, &auth, input)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(grant))
}

async fn admin_observer_reports(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_OBSERVER_READ)?;
    let items = state
        .store
        .list_observer_reports(query.limit.unwrap_or(50))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!(Page { items })))
}

async fn admin_observer_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(report_id): Path<String>,
) -> Result<Json<ObserverReport>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_OBSERVER_READ)?;
    let report = state
        .store
        .get_observer_report(&report_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    Ok(Json(report))
}

async fn admin_observer_run(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ObserverReport>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_OBSERVER_READ)?;
    create_observer_report_from_snapshot(&state, &auth.trace_id)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn create_observer_report_from_snapshot(
    state: &AppState,
    trace_id: &str,
) -> CoreResult<ObserverReport> {
    let snapshot = state.store.collect_observer_snapshot(trace_id).await?;
    let dead_letters = snapshot
        .run_counts
        .get("dead_letter")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let failed = snapshot
        .run_counts
        .get("failed")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let health = if dead_letters > 0 || failed > 5 {
        HealthStatus::Degraded
    } else {
        HealthStatus::Healthy
    };
    let report = ObserverReport::new(
        new_id("observer_run"),
        health,
        if dead_letters > 0 {
            Some(RiskLevel::Medium)
        } else {
            Some(RiskLevel::Low)
        },
        format!(
            "Observer snapshot collected at {}. dead_letter={}, failed={}.",
            snapshot.collected_at, dead_letters, failed
        ),
        json!({
            "run_counts": snapshot.run_counts,
            "agent_counts": snapshot.agent_counts,
            "session_counts": snapshot.session_counts,
        }),
        json!([
            {
                "priority": if dead_letters > 0 { "medium" } else { "low" },
                "recommendation": "Review dead_letter and timeout trends through agentctl audit before changing policy."
            }
        ]),
        json!({
            "lock_summary": snapshot.lock_summary,
            "audit_summary": snapshot.audit_summary,
            "worker_summary": snapshot.worker_summary,
        }),
        trace_id.to_string(),
    );
    let report = state.store.create_observer_report(report).await?;
    let mut audit_log = AuditLog::new(
        None,
        actions::INTERNAL_OBSERVER_TICK,
        AuditDecision::Completed,
        None,
        trace_id.to_string(),
    );
    audit_log.observer_report_id = Some(report.id.clone());
    let _ = state.store.append_audit(audit_log).await;
    Ok(report)
}

async fn internal_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(connector): Path<String>,
    Json(input): Json<WebhookTriggerInput>,
) -> Result<Json<control_services::WebhookAcceptedResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    tracing::info!(
        trace_id = %auth.trace_id,
        service_id = %auth.service_id,
        connector = %connector,
        event_type = %input.event_type,
        "manager internal webhook"
    );
    telemetry_support::record_request_metric(actions::INTERNAL_WEBHOOK, &auth.service_id);
    ensure_service_allows(&auth, actions::INTERNAL_WEBHOOK)?;
    let response = control_services::accept_webhook(&state.store, &auth, connector, input)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    tracing::info!(
        trace_id = %auth.trace_id,
        connector = %response.connector,
        event_type = %response.event_type,
        run_count = response.run_ids.len(),
        "webhook normalized to controlled runs"
    );
    Ok(Json(response))
}

async fn internal_create_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(run): Json<AgentRun>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_RUN_CREATE)?;
    let run = control_services::create_internal_run(&state.store, &auth, run)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(run))
}

async fn internal_claim_run(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_RUN_CLAIM)?;
    let claim = state
        .store
        .claim_next_run(&auth.service_id, Duration::from_secs(30))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(json!({ "claim": claim, "trace_id": auth.trace_id })))
}

async fn internal_heartbeat_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Result<Json<EmptyResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_RUN_HEARTBEAT)?;
    state
        .store
        .heartbeat_run(&run_id, &auth.service_id, Duration::from_secs(30))
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    state
        .store
        .record_worker_heartbeat(&auth.service_id, Some(&run_id), "heartbeat", &auth.trace_id)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn internal_finish_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(output): Json<agent_core::RuntimeOutput>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_RUN_FINISH)?;
    let run = state
        .store
        .finish_run(&run_id, output)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(run))
}

async fn internal_dead_letter_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_RUN_DEAD_LETTER)?;
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("dead letter requested");
    state
        .store
        .dead_letter_run(&run_id, reason)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn internal_append_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(input): Json<AppendMessageInput>,
) -> Result<Json<AgentSessionMessage>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_SESSION_APPEND_MESSAGE)?;
    control_services::append_message_to_session(
        &state.store,
        &auth,
        &session_id,
        input,
        false,
        actions::INTERNAL_SESSION_APPEND_MESSAGE,
    )
    .await
    .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
    .map(Json)
}

async fn internal_session_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<agent_core::SessionContext>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_SESSION_CONTEXT)?;
    state
        .store
        .session_context(&session_id, &auth.trace_id)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn internal_memory_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<EmptyResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_MEMORY_SUMMARY)?;
    let session_id = body
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::Conflict, "session_id required"),
                auth.trace_id.clone(),
            )
        })?;
    let summary = body.get("summary").and_then(Value::as_str).ok_or_else(|| {
        ApiError::from_core(
            AgentCoreError::coded(ErrorCode::Conflict, "summary required"),
            auth.trace_id.clone(),
        )
    })?;
    state
        .store
        .write_summary(session_id, summary, &auth.trace_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(EmptyResponse {
        status: "ok".to_string(),
        trace_id: auth.trace_id,
    }))
}

async fn internal_observer_tick(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ObserverReport>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_OBSERVER_TICK)?;
    create_observer_report_from_snapshot(&state, &auth.trace_id)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        AGENT_TYPE_BACKGROUND_WORKER, AgentRunStatus, AgentSession, MessageRole, RequestType,
        RoleAssignment, SideEffectMode, TriggerType, new_trace_id,
    };
    use time::OffsetDateTime;

    async fn test_state() -> AppState {
        let store = MemoryAgentStore::new();
        store.bootstrap().await.unwrap();
        AppState {
            store: Arc::new(store),
            config: Arc::new(ManagerConfig {
                jwt_secret: None,
                allow_dev_headers: true,
                default_service_actions: vec!["*".to_string()],
            }),
        }
    }

    fn test_auth(role: RoleName) -> AuthContext {
        AuthContext {
            user_id: "user-1".to_string(),
            service_id: "orchestrator".to_string(),
            service_allowed_actions: vec!["*".to_string()],
            roles: vec![RoleAssignment::global(role)],
            resource_allowlist: vec!["resource:team/project-alpha".to_string()],
            trace_id: new_trace_id(),
        }
    }

    async fn create_read_only_run(state: &AppState, auth: &AuthContext, label: &str) -> AgentRun {
        let agent = request_services::submit_request(
            &state.store,
            auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some(format!("create {label} worker")),
                structured_payload: json!({"mode": label}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                side_effect_mode: Some(SideEffectMode::ReadOnly),
            },
        )
        .await
        .unwrap();
        lifecycle_services::create_run(
            &state.store,
            auth,
            agent.agent_id.unwrap(),
            CreateRunInput {
                session_id: None,
                trigger_type: TriggerType::Manual,
                idempotency_key: None,
                target_resource: Some("resource:team/project-alpha".to_string()),
                risk_level: Some(RiskLevel::Low),
                side_effect_mode: Some(SideEffectMode::ReadOnly),
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_agent_defaults_to_approval_required() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let response = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create background worker".to_string()),
                structured_payload: json!({}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                side_effect_mode: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.status, AgentRequestStatus::ApprovalRequired);
        assert!(response.approval_id.is_some());
    }

    #[tokio::test]
    async fn read_only_create_agent_is_fulfilled() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let response = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create read only background worker".to_string()),
                structured_payload: json!({"mode": "read_only"}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                side_effect_mode: Some(SideEffectMode::ReadOnly),
            },
        )
        .await
        .unwrap();

        assert_eq!(response.status, AgentRequestStatus::Fulfilled);
        assert!(response.agent_id.is_some());
    }

    #[tokio::test]
    async fn admin_grant_endpoint_persists_and_audits_grant() {
        let state = test_state().await;
        let Json(grant) = admin_create_grant(
            State(state.clone()),
            HeaderMap::new(),
            Json(CreateGrantInput {
                subject_type: "user".to_string(),
                subject_id: "maintainer-1".to_string(),
                action: "run:create".to_string(),
                resource_type: "team".to_string(),
                resource_id: "project-alpha".to_string(),
                constraints: json!({"side_effect_mode": "read_only"}),
                expires_at: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(grant.subject_id, "maintainer-1");
        assert_eq!(grant.granted_by.as_deref(), Some("dev-user"));
        let audits = state.store.list_audit(10).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_GRANT_CREATE
                && audit.decision == Some(AuditDecision::Allowed)
        }));
    }

    #[tokio::test]
    async fn admin_dead_letter_retry_and_terminate_are_audited() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);

        let retry_run = create_read_only_run(&state, &auth, "dead-letter-retry").await;
        let dead_letter = state
            .store
            .dead_letter_run(&retry_run.id, "runtime exhausted")
            .await
            .unwrap();
        assert_eq!(dead_letter.run_status, AgentRunStatus::DeadLetter);

        let Json(inspected) = admin_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(dead_letter.id.clone()),
        )
        .await
        .unwrap();
        assert_eq!(inspected.run_status, AgentRunStatus::DeadLetter);

        let Json(retried) = admin_retry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(dead_letter.id.clone()),
            Json(RunAdminDecisionInput {
                reason: Some("retry after operator fix".to_string()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(retried.run_status, AgentRunStatus::Queued);
        assert_eq!(retried.retry_count, 0);
        assert!(retried.next_retry_at.is_none());

        let terminate_run = create_read_only_run(&state, &auth, "dead-letter-terminate").await;
        let terminate_dead_letter = state
            .store
            .dead_letter_run(&terminate_run.id, "runtime exhausted")
            .await
            .unwrap();
        let Json(terminated) = admin_terminate_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(terminate_dead_letter.id),
            Json(RunAdminDecisionInput {
                reason: Some("operator stop".to_string()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(terminated.run_status, AgentRunStatus::Cancelled);
        assert!(terminated.finished_at.is_some());

        let audits = state.store.list_audit(20).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_RUN_RETRY
                && audit.run_id.as_deref() == Some(retry_run.id.as_str())
                && audit.decision == Some(AuditDecision::Allowed)
        }));
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_RUN_TERMINATE
                && audit.run_id.as_deref() == Some(terminate_run.id.as_str())
                && audit.decision == Some(AuditDecision::Allowed)
        }));
    }

    #[tokio::test]
    async fn webhook_creates_idempotent_read_only_runs_for_matching_agents() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let agent = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create webhook worker".to_string()),
                structured_payload: json!({"mode": "webhook"}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                side_effect_mode: Some(SideEffectMode::ReadOnly),
            },
        )
        .await
        .unwrap();
        assert!(agent.agent_id.is_some());

        let input = WebhookTriggerInput {
            trigger_type: "webhook".to_string(),
            connector: "github".to_string(),
            event_type: "issue.updated".to_string(),
            resource: "resource:team/project-alpha".to_string(),
            dedupe_key: "github-issue-1-42".to_string(),
            payload_ref: "memory://webhook/github-issue-1-42".to_string(),
            received_at: OffsetDateTime::now_utc(),
        };
        let Json(first) = internal_webhook(
            State(state.clone()),
            HeaderMap::new(),
            Path("github".to_string()),
            Json(input.clone()),
        )
        .await
        .unwrap();
        let Json(second) = internal_webhook(
            State(state.clone()),
            HeaderMap::new(),
            Path("github".to_string()),
            Json(input),
        )
        .await
        .unwrap();

        assert_eq!(first.run_ids.len(), 1);
        assert_eq!(second.run_ids, first.run_ids);
        let run = state
            .store
            .get_run(&first.run_ids[0])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.trigger_type, TriggerType::Webhook);
        assert_eq!(run.side_effect_mode, SideEffectMode::ReadOnly);
        assert_eq!(run.idempotency_key.as_deref(), Some("github-issue-1-42"));
    }

    #[tokio::test]
    async fn user_session_and_run_create_are_idempotent() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let agent = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create idempotent worker".to_string()),
                structured_payload: json!({"mode": "idempotency"}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                side_effect_mode: Some(SideEffectMode::ReadOnly),
            },
        )
        .await
        .unwrap();
        let agent_id = agent.agent_id.unwrap();

        let session_input = CreateSessionInput {
            source_conversation_id: Some("conv-idempotent".to_string()),
            resource_scope: json!({}),
            idempotency_key: Some("session-key-1".to_string()),
        };
        let first_session = lifecycle_services::create_session(
            &state.store,
            &auth,
            agent_id.clone(),
            session_input.clone(),
        )
        .await
        .unwrap();
        let second_session = lifecycle_services::create_session(
            &state.store,
            &auth,
            agent_id.clone(),
            session_input,
        )
        .await
        .unwrap();
        assert_eq!(second_session.id, first_session.id);
        assert_eq!(
            first_session.idempotency_key.as_deref(),
            Some("session-key-1")
        );

        let run_input = CreateRunInput {
            session_id: Some(first_session.id),
            trigger_type: TriggerType::Manual,
            idempotency_key: Some("run-key-1".to_string()),
            target_resource: Some("resource:team/project-alpha".to_string()),
            risk_level: Some(RiskLevel::Low),
            side_effect_mode: Some(SideEffectMode::ReadOnly),
        };
        let first_run = lifecycle_services::create_run(
            &state.store,
            &auth,
            agent_id.clone(),
            run_input.clone(),
        )
        .await
        .unwrap();
        let second_run = lifecycle_services::create_run(&state.store, &auth, agent_id, run_input)
            .await
            .unwrap();
        assert_eq!(second_run.id, first_run.id);
        assert_eq!(first_run.idempotency_key.as_deref(), Some("run-key-1"));
    }

    #[tokio::test]
    async fn internal_append_message_uses_service_authorization_not_owner_identity() {
        let state = test_state().await;
        let session = state
            .store
            .create_session(AgentSession::new(
                "agent-1",
                "session-owner",
                json!({"resource": "resource:team/project-alpha"}),
                "trace-test",
            ))
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-agent-user", "worker-user".parse().unwrap());
        headers.insert(
            "x-agent-allowed-actions",
            actions::INTERNAL_SESSION_APPEND_MESSAGE.parse().unwrap(),
        );

        let Json(message) = internal_append_message(
            State(state),
            headers,
            Path(session.id),
            Json(AppendMessageInput {
                role: MessageRole::Assistant,
                content_summary: "runtime response".to_string(),
                content_ref: None,
                run_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(message.sequence, 1);
        assert_eq!(message.content_summary.as_deref(), Some("runtime response"));
    }
}
