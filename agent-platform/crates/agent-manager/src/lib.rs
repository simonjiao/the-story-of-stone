mod control_services;
mod external_action_services;
mod http_support;
mod lifecycle_services;
mod observer_services;
mod request_services;
mod run_admin_services;
mod telemetry_support;

use agent_core::{
    AgentBridgeBinding, AgentBridgeBindingSummary, AgentCoreError, AgentGrant, AgentInstance,
    AgentRequestInput, AgentRequestResponse, AgentRequestStatus, AgentRun, AgentSession,
    AgentSessionMessage, AppendMessageInput, ApprovalDecisionInput, ApprovalStatus, AuditDecision,
    AuditLog, AuthContext, ClaimOpenWebUiBridgeNonceInput, CoreResult, CreateChildSessionInput,
    CreateGrantInput, CreateRunInput, CreateSessionInput, DenyDecisionInput, EmptyResponse,
    ErrorCode, ExternalActionPlanApplyInput, ExternalActionPlanApplyResponse,
    ExternalActionPlanCompensateInput, ExternalActionPlanCompensateResponse,
    ExternalActionPlanDryRunInput, ExternalActionPlanDryRunResponse, ObserverReport,
    ObserverReportDiscussionInput, Page, RoleName, RunAdminDecisionInput, RunSummary,
    SystemStatusSessionInput, UpdateOpenWebUiBridgeRunInput, UpsertOpenWebUiBridgeBindingInput,
    WebhookTriggerInput, actions, assess_observer_snapshot, new_id,
};
use agent_store::{AgentStore, MemoryAgentStore, PgAgentStore};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get, post, put},
};
pub use http_support::{ApiError, ManagerConfig, extract_auth};
use http_support::{ensure_admin, ensure_operator_or_admin, ensure_service_allows};
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

#[derive(Debug, Deserialize)]
struct BridgeBindingQuery {
    model: Option<String>,
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
        .route("/v1/my-runs/{run_id}", get(my_run))
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
            "/v1/admin/observer/system-session",
            post(admin_observer_system_session),
        )
        .route(
            "/v1/admin/observer/reports/{report_id}",
            get(admin_observer_report),
        )
        .route(
            "/v1/admin/observer/reports/{report_id}/discussions",
            post(admin_observer_report_discussion),
        )
        .route("/v1/admin/observer/runs", post(admin_observer_run))
        .route(
            "/v1/admin/runs/{run_id}/external-action-plans/dry-run",
            post(admin_external_action_dry_run),
        )
        .route(
            "/v1/admin/runs/{run_id}/external-action-plans/{plan_id}/apply",
            post(admin_external_action_apply),
        )
        .route(
            "/v1/admin/runs/{run_id}/external-action-plans/{plan_id}/compensate",
            post(admin_external_action_compensate),
        )
        .route("/v1/internal/webhooks/{connector}", post(internal_webhook))
        .route(
            "/v1/internal/open-webui-bridge/nonces",
            post(internal_claim_open_webui_bridge_nonce),
        )
        .route(
            "/v1/internal/open-webui-bridge/bindings/{chat_id}",
            get(internal_open_webui_bridge_binding),
        )
        .route(
            "/v1/internal/open-webui-bridge/bindings",
            put(internal_upsert_open_webui_bridge_binding),
        )
        .route(
            "/v1/internal/open-webui-bridge/bindings/{chat_id}/close",
            post(internal_close_open_webui_bridge_binding),
        )
        .route(
            "/v1/internal/open-webui-bridge/bindings/{binding_id}/run",
            post(internal_update_open_webui_bridge_run),
        )
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

async fn audit_bridge_binding(
    state: &AppState,
    auth: &AuthContext,
    action: &str,
    binding: &AgentBridgeBinding,
    run_id: Option<&str>,
) {
    let mut audit_log = AuditLog::new(
        Some(auth),
        action,
        AuditDecision::Allowed,
        Some(format!(
            "chat_id={} model={} session_id={}",
            binding.open_webui_chat_id, binding.model, binding.agent_session_id
        )),
        auth.trace_id.clone(),
    );
    audit_log.resource_type = Some("open_webui_bridge_binding".to_string());
    audit_log.resource_id = Some(binding.id.clone());
    audit_log.session_id = Some(binding.agent_session_id.clone());
    audit_log.run_id = run_id.map(ToString::to_string);
    let _ = state.store.append_audit(audit_log).await;
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

async fn my_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Result<Json<RunSummary>, ApiError> {
    let auth = auth(&headers, &state)?;
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
    let agent = state
        .store
        .get_agent(&run.agent_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    if agent.owner_user != auth.user_id
        && !auth.has_any_role(&[RoleName::SystemAdmin, RoleName::AgentAdmin])
    {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::NotFound, "not found"),
            auth.trace_id,
        ));
    }
    Ok(Json(run_summary(&run)))
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

fn bridge_binding_summary(binding: &AgentBridgeBinding) -> AgentBridgeBindingSummary {
    AgentBridgeBindingSummary {
        binding_id: binding.id.clone(),
        open_webui_subject: binding.open_webui_subject.clone(),
        open_webui_chat_id: binding.open_webui_chat_id.clone(),
        open_webui_session_id: binding.open_webui_session_id.clone(),
        model: binding.model.clone(),
        agent_id: binding.agent_id.clone(),
        agent_session_id: binding.agent_session_id.clone(),
        status: binding.status,
        last_message_id: binding.last_message_id.clone(),
        last_run_id: binding.last_run_id.clone(),
        trace_id: binding.trace_id.clone(),
        created_at: binding.created_at,
        updated_at: binding.updated_at,
        closed_at: binding.closed_at,
    }
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
    let current = state
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
    if current.owner_user != auth.user_id
        && !auth.has_any_role(&[RoleName::SystemAdmin, RoleName::AgentAdmin])
    {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::NotFound, "not found"),
            auth.trace_id,
        ));
    }
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

async fn admin_observer_report_discussion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(report_id): Path<String>,
    Json(input): Json<ObserverReportDiscussionInput>,
) -> Result<Json<agent_core::ObserverReportDiscussionResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_operator_or_admin(&auth, actions::ADMIN_OBSERVER_DISCUSS)?;
    observer_services::create_report_discussion(&state.store, &auth, report_id, input)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn admin_observer_system_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SystemStatusSessionInput>,
) -> Result<Json<agent_core::SystemStatusSessionResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_operator_or_admin(&auth, actions::ADMIN_OBSERVER_DISCUSS)?;
    observer_services::create_system_status_session(&state.store, &auth, input)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
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

async fn admin_external_action_dry_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<ExternalActionPlanDryRunInput>,
) -> Result<Json<ExternalActionPlanDryRunResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_EXTERNAL_ACTION_DRY_RUN)?;
    external_action_services::dry_run_external_action_plan(&state.store, &auth, run_id, input)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn admin_external_action_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((run_id, plan_id)): Path<(String, String)>,
    Json(input): Json<ExternalActionPlanApplyInput>,
) -> Result<Json<ExternalActionPlanApplyResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_EXTERNAL_ACTION_APPLY)?;
    external_action_services::apply_external_action_plan(
        &state.store,
        &auth,
        run_id,
        plan_id,
        input,
    )
    .await
    .map(Json)
    .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn admin_external_action_compensate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((run_id, plan_id)): Path<(String, String)>,
    Json(input): Json<ExternalActionPlanCompensateInput>,
) -> Result<Json<ExternalActionPlanCompensateResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_admin(&auth, actions::ADMIN_EXTERNAL_ACTION_COMPENSATE)?;
    external_action_services::compensate_external_action_plan(
        &state.store,
        &auth,
        run_id,
        plan_id,
        input,
    )
    .await
    .map(Json)
    .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

async fn create_observer_report_from_snapshot(
    state: &AppState,
    trace_id: &str,
) -> CoreResult<ObserverReport> {
    let snapshot = state.store.collect_observer_snapshot(trace_id).await?;
    let assessment = assess_observer_snapshot(&snapshot);
    let report = ObserverReport::new(
        new_id("observer_run"),
        assessment.health_status,
        Some(assessment.risk_level),
        assessment.summary,
        assessment.findings,
        assessment.recommendations,
        assessment.evidence_refs,
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

async fn internal_open_webui_bridge_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<BridgeBindingQuery>,
) -> Result<Json<AgentBridgeBindingSummary>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_READ)?;
    let model = query.model.unwrap_or_else(|| "hermes-agent".to_string());
    let binding = state
        .store
        .get_open_webui_bridge_binding(&auth.user_id, &chat_id, &model)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    Ok(Json(bridge_binding_summary(&binding)))
}

async fn internal_claim_open_webui_bridge_nonce(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ClaimOpenWebUiBridgeNonceInput>,
) -> Result<Json<EmptyResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_OPEN_WEBUI_BRIDGE_NONCE)?;
    let response = state
        .store
        .claim_open_webui_bridge_nonce(
            &auth.user_id,
            &input.open_webui_chat_id,
            &input.model,
            &input.nonce,
            input.issued_at,
            &auth.trace_id,
        )
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(response))
}

async fn internal_upsert_open_webui_bridge_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpsertOpenWebUiBridgeBindingInput>,
) -> Result<Json<AgentBridgeBindingSummary>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_UPSERT)?;
    let mut binding = AgentBridgeBinding::new(
        auth.user_id.clone(),
        input.open_webui_chat_id,
        input.open_webui_session_id,
        input.model,
        input.agent_id,
        input.agent_session_id,
        auth.trace_id.clone(),
    );
    binding.last_message_id = input.last_message_id;
    let binding = state
        .store
        .upsert_open_webui_bridge_binding(binding)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit_bridge_binding(
        &state,
        &auth,
        actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_UPSERT,
        &binding,
        None,
    )
    .await;
    Ok(Json(bridge_binding_summary(&binding)))
}

async fn internal_close_open_webui_bridge_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(chat_id): Path<String>,
    Query(query): Query<BridgeBindingQuery>,
) -> Result<Json<EmptyResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_CLOSE)?;
    let model = query.model.unwrap_or_else(|| "hermes-agent".to_string());
    let existing = state
        .store
        .get_open_webui_bridge_binding(&auth.user_id, &chat_id, &model)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    let response = state
        .store
        .close_open_webui_bridge_binding(&auth.user_id, &chat_id, &model, &auth.trace_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    if let Some(binding) = existing {
        audit_bridge_binding(
            &state,
            &auth,
            actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_CLOSE,
            &binding,
            None,
        )
        .await;
    }
    Ok(Json(response))
}

async fn internal_update_open_webui_bridge_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(binding_id): Path<String>,
    Json(input): Json<UpdateOpenWebUiBridgeRunInput>,
) -> Result<Json<AgentBridgeBindingSummary>, ApiError> {
    let auth = auth(&headers, &state)?;
    ensure_service_allows(&auth, actions::INTERNAL_OPEN_WEBUI_BRIDGE_RUN_UPDATE)?;
    let binding = state
        .store
        .update_open_webui_bridge_run(
            &auth.user_id,
            &binding_id,
            input.message_id.as_deref(),
            &input.run_id,
            &auth.trace_id,
        )
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit_bridge_binding(
        &state,
        &auth,
        actions::INTERNAL_OPEN_WEBUI_BRIDGE_RUN_UPDATE,
        &binding,
        Some(input.run_id.as_str()),
    )
    .await;
    Ok(Json(bridge_binding_summary(&binding)))
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
        AGENT_TYPE_BACKGROUND_WORKER, AGENT_TYPE_OBSERVER, AgentRunStatus, AgentSession,
        ApprovalRequest, ApprovalStatus, CredentialLease, CredentialLeaseRequest,
        CredentialLeaseStatus, CredentialProvider, ErrorCode, ExternalActionMode,
        ExternalActionPlanStatus, HealthStatus, MessageRole, ObserverReport, RequestType,
        ResourceLock, RiskLevel, RoleAssignment, TriggerType, WriteConnector,
        WriteConnectorCompensateInput, WriteConnectorCompensateOutput, WriteConnectorDryRunInput,
        WriteConnectorDryRunOutput, WriteConnectorExecuteInput, WriteConnectorExecuteOutput,
        new_id, new_trace_id,
    };
    use std::{sync::Mutex, time::Duration};
    use time::OffsetDateTime;
    use tokio::net::TcpListener;

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
                external_action_mode: Some(ExternalActionMode::ReadOnly),
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
                external_action_mode: Some(ExternalActionMode::ReadOnly),
            },
        )
        .await
        .unwrap()
    }

    async fn approved_external_action_approval(state: &AppState, auth: &AuthContext) -> String {
        let approval = ApprovalRequest {
            id: new_id("approval"),
            request_id: new_id("req"),
            requested_by_user: auth.user_id.clone(),
            approver_user: Some("approver-1".to_string()),
            status: ApprovalStatus::Approved,
            risk_level: Some(RiskLevel::Low),
            reason: Some("external action dry-run approval".to_string()),
            decision_reason: Some("test approval".to_string()),
            created_at: OffsetDateTime::now_utc(),
            decided_at: Some(OffsetDateTime::now_utc()),
        };
        state.store.create_approval(approval).await.unwrap().id
    }

    async fn spawn_server(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
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
                external_action_mode: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.status, AgentRequestStatus::ApprovalRequired);
        assert!(response.approval_id.is_some());
    }

    #[tokio::test]
    async fn approved_create_agent_keeps_requester_as_owner() {
        let state = test_state().await;
        let requester = test_auth(RoleName::ResourceMaintainer);
        let response = request_services::submit_request(
            &state.store,
            &requester,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create approved background worker".to_string()),
                structured_payload: json!({"mode": "approved-owner"}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                external_action_mode: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(response.status, AgentRequestStatus::ApprovalRequired);

        let Json(approved) = admin_approve(
            State(state.clone()),
            HeaderMap::new(),
            Path(response.request_id),
            Json(ApprovalDecisionInput {
                reason: Some("test approval".to_string()),
            }),
        )
        .await
        .unwrap();

        let agent_id = approved.agent_id.unwrap();
        let agent = state.store.get_agent(&agent_id).await.unwrap().unwrap();
        assert_eq!(agent.owner_user, requester.user_id);
        let requester_agents = state
            .store
            .list_agents(Some(&requester.user_id), 10)
            .await
            .unwrap();
        assert!(
            requester_agents
                .iter()
                .any(|agent| agent.agent_id == agent_id)
        );
        let approver_agents = state.store.list_agents(Some("dev-user"), 10).await.unwrap();
        assert!(
            !approver_agents
                .iter()
                .any(|agent| agent.agent_id == agent_id)
        );
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
                external_action_mode: Some(ExternalActionMode::ReadOnly),
            },
        )
        .await
        .unwrap();

        assert_eq!(response.status, AgentRequestStatus::Fulfilled);
        assert!(response.agent_id.is_some());
    }

    #[tokio::test]
    async fn bridge_source_creates_reusable_agent_with_distinct_chat_sessions() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let first = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create bridge worker".to_string()),
                structured_payload: json!({
                    "mode": "bridge-reuse",
                    "bridge_source": {
                        "kind": "open_webui",
                        "chat_id": "chat-1",
                        "session_id": "ow-session-1",
                        "message_id": "msg-1",
                        "model": "hermes-agent"
                    }
                }),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::ReadOnly),
            },
        )
        .await
        .unwrap();
        let second = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create bridge worker in another chat".to_string()),
                structured_payload: json!({
                    "mode": "bridge-reuse",
                    "bridge_source": {
                        "kind": "open_webui",
                        "chat_id": "chat-2",
                        "session_id": "ow-session-2",
                        "message_id": "msg-2",
                        "model": "hermes-agent"
                    }
                }),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::ReadOnly),
            },
        )
        .await
        .unwrap();

        assert_eq!(first.status, AgentRequestStatus::Fulfilled);
        assert_eq!(first.agent_id, second.agent_id);

        let first_binding = state
            .store
            .get_open_webui_bridge_binding(&auth.user_id, "chat-1", "hermes-agent")
            .await
            .unwrap()
            .unwrap();
        let second_binding = state
            .store
            .get_open_webui_bridge_binding(&auth.user_id, "chat-2", "hermes-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first_binding.agent_id, second_binding.agent_id);
        assert_ne!(
            first_binding.agent_session_id,
            second_binding.agent_session_id
        );
        assert_eq!(first_binding.last_message_id.as_deref(), Some("msg-1"));
    }

    fn bridge_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-agent-service", "agent-orchestrator".parse().unwrap());
        headers.insert("x-agent-user", "openwebui:user-1".parse().unwrap());
        headers.insert("x-agent-roles", "viewer".parse().unwrap());
        headers.insert(
            "x-agent-allowed-actions",
            "internal:open_webui_bridge:*".parse().unwrap(),
        );
        headers.insert("x-agent-trace-id", new_trace_id().parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn bridge_nonce_claim_rejects_replay_through_manager() {
        let state = test_state().await;
        let input = ClaimOpenWebUiBridgeNonceInput {
            open_webui_chat_id: "chat-1".to_string(),
            model: "hermes-agent".to_string(),
            nonce: "nonce-1".to_string(),
            issued_at: OffsetDateTime::now_utc().unix_timestamp(),
        };
        let _ = internal_claim_open_webui_bridge_nonce(
            State(state.clone()),
            bridge_headers(),
            Json(input.clone()),
        )
        .await
        .unwrap();

        let replay =
            internal_claim_open_webui_bridge_nonce(State(state), bridge_headers(), Json(input))
                .await;
        assert!(replay.is_err());
    }

    #[tokio::test]
    async fn bridge_binding_lifecycle_is_audited() {
        let state = test_state().await;
        let Json(binding) = internal_upsert_open_webui_bridge_binding(
            State(state.clone()),
            bridge_headers(),
            Json(UpsertOpenWebUiBridgeBindingInput {
                open_webui_chat_id: "chat-1".to_string(),
                open_webui_session_id: Some("ow-session-1".to_string()),
                model: "hermes-agent".to_string(),
                agent_id: "agent-1".to_string(),
                agent_session_id: "sess-1".to_string(),
                last_message_id: Some("msg-1".to_string()),
            }),
        )
        .await
        .unwrap();

        let _ = internal_update_open_webui_bridge_run(
            State(state.clone()),
            bridge_headers(),
            Path(binding.binding_id.clone()),
            Json(UpdateOpenWebUiBridgeRunInput {
                message_id: Some("msg-2".to_string()),
                run_id: "run-1".to_string(),
            }),
        )
        .await
        .unwrap();
        let _ = internal_close_open_webui_bridge_binding(
            State(state.clone()),
            bridge_headers(),
            Path("chat-1".to_string()),
            Query(BridgeBindingQuery {
                model: Some("hermes-agent".to_string()),
            }),
        )
        .await
        .unwrap();

        let audits = state.store.list_audit(10).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_UPSERT
                && audit.resource_id.as_deref() == Some(binding.binding_id.as_str())
        }));
        assert!(audits.iter().any(|audit| {
            audit.action == actions::INTERNAL_OPEN_WEBUI_BRIDGE_RUN_UPDATE
                && audit.run_id.as_deref() == Some("run-1")
        }));
        assert!(audits.iter().any(|audit| {
            audit.action == actions::INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_CLOSE
                && audit.session_id.as_deref() == Some("sess-1")
        }));
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
                constraints: json!({"external_action_mode": "read_only"}),
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
    async fn observer_report_discussion_creates_redacted_session_for_operator() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let agent = request_services::submit_request(
            &state.store,
            &auth,
            AgentRequestInput {
                request_type: RequestType::CreateAgent,
                agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
                target_resource: Some("resource:team/project-alpha".to_string()),
                intent_text: Some("create discussion worker".to_string()),
                structured_payload: json!({"mode": "observer-discussion"}),
                idempotency_key: None,
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::ReadOnly),
            },
        )
        .await
        .unwrap();
        let report = create_observer_report_from_snapshot(&state, &new_trace_id())
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("x-agent-roles", "operator".parse().unwrap());
        let Json(response) = admin_observer_report_discussion(
            State(state.clone()),
            headers,
            Path(report.id.clone()),
            Json(ObserverReportDiscussionInput {
                agent_id: agent.agent_id.unwrap(),
                initial_message: "what should we do next?".to_string(),
                idempotency_key: Some("discussion-1".to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.report_id, report.id);
        assert_eq!(response.session.owner_user, "dev-user");
        assert!(
            response
                .session
                .context_summary
                .as_deref()
                .unwrap_or_default()
                .contains("Observer report discussion context")
        );
        assert_eq!(response.first_message.role, MessageRole::User);
        let audits = state.store.list_audit(20).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_OBSERVER_DISCUSS
                && audit.observer_report_id.as_deref() == Some(report.id.as_str())
                && audit.session_id.as_deref() == Some(response.session.id.as_str())
        }));
    }

    #[tokio::test]
    async fn observer_system_session_creates_dedicated_status_agent_with_redacted_report() {
        let state = test_state().await;
        let report = state
            .store
            .create_observer_report(ObserverReport::new(
                new_id("observer_run"),
                HealthStatus::Healthy,
                Some(RiskLevel::Low),
                "system is healthy",
                json!({"run_counts": {"completed": 3}}),
                json!([{"priority": "low", "recommendation": "continue observing"}]),
                json!({
                    "safe": "visible",
                    "nested": {"credential_token": "secret-value"}
                }),
                new_trace_id(),
            ))
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("x-agent-roles", "operator".parse().unwrap());
        let Json(response) = admin_observer_system_session(
            State(state.clone()),
            headers,
            Json(SystemStatusSessionInput {
                report_id: None,
                initial_message: Some("give me the deep status".to_string()),
                idempotency_key: Some("system-status".to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.report_id, report.id);
        assert_eq!(response.agent.agent_type, AGENT_TYPE_OBSERVER);
        assert_eq!(
            response.agent.display_name.as_deref(),
            Some("System Observer")
        );
        assert_eq!(
            response.session.source_conversation_id.as_deref(),
            Some("system_observer:status")
        );
        assert_eq!(response.report_message.role, MessageRole::System);
        let packet = response.report_message.content_summary.unwrap();
        assert!(packet.contains("system is healthy"));
        assert!(packet.contains("\"credential_token\":\"redacted\""));
        assert!(!packet.contains("secret-value"));
        assert_eq!(response.first_message.role, MessageRole::User);

        let audits = state.store.list_audit(20).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_OBSERVER_DISCUSS
                && audit.observer_report_id.as_deref() == Some(report.id.as_str())
                && audit.session_id.as_deref() == Some(response.session.id.as_str())
        }));
    }

    #[tokio::test]
    async fn p1_external_action_dry_run_creates_plan_and_noop_credential_lease() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "p1-external-action-dry-run").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;

        let Json(response) = admin_external_action_dry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(run.id.clone()),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: Some(approval_id.clone()),
                input_summary: Some("would comment on an issue".to_string()),
                input_ref: Some("payload://dry-run/1".to_string()),
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            response.dry_run_status,
            ExternalActionPlanStatus::DryRunReady
        );
        assert!(response.credential_lease.is_some());
        assert_eq!(
            response.plan.approval_id.as_deref(),
            Some(approval_id.as_str())
        );
        assert_eq!(response.plan.run_id, run.id);
        assert!(
            response
                .plan
                .result_ref
                .as_deref()
                .unwrap()
                .starts_with("noop://")
        );
        let plans = state
            .store
            .list_external_action_plans_by_run(&response.plan.run_id)
            .await
            .unwrap();
        assert_eq!(plans.len(), 1);
    }

    #[derive(Debug, Clone)]
    struct TestCredentialProvider;

    #[async_trait::async_trait]
    impl CredentialProvider for TestCredentialProvider {
        async fn dry_run_lease(
            &self,
            request: CredentialLeaseRequest,
        ) -> CoreResult<CredentialLease> {
            Ok(CredentialLease::dry_run(
                request.external_action_plan_id,
                request.credential_scope,
                request.trace_id,
            ))
        }

        async fn active_lease(
            &self,
            request: CredentialLeaseRequest,
        ) -> CoreResult<CredentialLease> {
            Ok(CredentialLease::active(
                request.external_action_plan_id,
                request.credential_scope,
                "vault://leases/test-external-action",
                60,
                request.trace_id,
            ))
        }
    }

    #[derive(Debug, Clone)]
    struct RecordingWriteConnector {
        provider_ref: Arc<Mutex<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl WriteConnector for RecordingWriteConnector {
        async fn dry_run(
            &self,
            input: WriteConnectorDryRunInput,
        ) -> CoreResult<WriteConnectorDryRunOutput> {
            Ok(WriteConnectorDryRunOutput {
                accepted: true,
                status: "dry_run_ready".to_string(),
                result_ref: Some(format!("test://dry-run/{}", input.plan.id)),
                metadata: json!({}),
            })
        }

        async fn execute(
            &self,
            input: WriteConnectorExecuteInput,
        ) -> CoreResult<WriteConnectorExecuteOutput> {
            *self.provider_ref.lock().unwrap() = input.credential_provider_ref.clone();
            Ok(WriteConnectorExecuteOutput {
                accepted: true,
                status: "applied".to_string(),
                result_ref: Some(format!("write://action-executions/{}", input.plan.id)),
                compensation_ref: Some(format!("compensate://action-executions/{}", input.plan.id)),
                error_code: None,
                metadata: json!({"attempted": true}),
            })
        }

        async fn compensate(
            &self,
            input: WriteConnectorCompensateInput,
        ) -> CoreResult<WriteConnectorCompensateOutput> {
            Ok(WriteConnectorCompensateOutput {
                accepted: true,
                status: "compensated".to_string(),
                result_ref: Some(format!(
                    "compensate-result://action-executions/{}",
                    input.plan.id
                )),
                error_code: None,
                metadata: json!({"compensated": true}),
            })
        }
    }

    #[derive(Debug, Clone)]
    struct InvalidWriteConnector;

    #[async_trait::async_trait]
    impl WriteConnector for InvalidWriteConnector {
        async fn dry_run(
            &self,
            input: WriteConnectorDryRunInput,
        ) -> CoreResult<WriteConnectorDryRunOutput> {
            Ok(WriteConnectorDryRunOutput {
                accepted: true,
                status: "dry_run_ready".to_string(),
                result_ref: Some(format!("test://dry-run/{}", input.plan.id)),
                metadata: json!({}),
            })
        }

        async fn execute(
            &self,
            input: WriteConnectorExecuteInput,
        ) -> CoreResult<WriteConnectorExecuteOutput> {
            Ok(WriteConnectorExecuteOutput {
                accepted: true,
                status: "applied".to_string(),
                result_ref: Some(format!("write://action-executions/{}", input.plan.id)),
                compensation_ref: None,
                error_code: None,
                metadata: json!({"invalid": "missing compensation_ref"}),
            })
        }

        async fn compensate(
            &self,
            input: WriteConnectorCompensateInput,
        ) -> CoreResult<WriteConnectorCompensateOutput> {
            Ok(WriteConnectorCompensateOutput {
                accepted: true,
                status: "compensated".to_string(),
                result_ref: Some(format!(
                    "compensate-result://action-executions/{}",
                    input.plan.id
                )),
                error_code: None,
                metadata: json!({}),
            })
        }
    }

    #[derive(Debug, Clone)]
    struct FailingWriteConnector {
        attempts: Arc<Mutex<u32>>,
    }

    #[async_trait::async_trait]
    impl WriteConnector for FailingWriteConnector {
        async fn dry_run(
            &self,
            input: WriteConnectorDryRunInput,
        ) -> CoreResult<WriteConnectorDryRunOutput> {
            Ok(WriteConnectorDryRunOutput {
                accepted: true,
                status: "dry_run_ready".to_string(),
                result_ref: Some(format!("test://dry-run/{}", input.plan.id)),
                metadata: json!({}),
            })
        }

        async fn execute(
            &self,
            _input: WriteConnectorExecuteInput,
        ) -> CoreResult<WriteConnectorExecuteOutput> {
            *self.attempts.lock().unwrap() += 1;
            Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "connector execution failed",
            ))
        }

        async fn compensate(
            &self,
            _input: WriteConnectorCompensateInput,
        ) -> CoreResult<WriteConnectorCompensateOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "connector compensation failed",
            ))
        }
    }

    #[tokio::test]
    async fn external_action_apply_uses_active_credential_lock_and_audit() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "external-action-apply").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;
        let Json(dry_run) = admin_external_action_dry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(run.id.clone()),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: Some(approval_id),
                input_summary: Some("comment on an issue".to_string()),
                input_ref: Some("payload://apply/1".to_string()),
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();
        let provider_ref = Arc::new(Mutex::new(None));
        let connector = RecordingWriteConnector {
            provider_ref: provider_ref.clone(),
        };

        let response = external_action_services::apply_external_action_plan_with_adapters(
            &state.store,
            &auth,
            run.id.clone(),
            dry_run.plan.id.clone(),
            ExternalActionPlanApplyInput {
                payload: json!({"body": "approved external write"}),
            },
            &TestCredentialProvider,
            &connector,
            external_action_services::ExternalActionApplyConfig {
                lock_lease: Duration::from_secs(30),
                max_attempts: 1,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.apply_status, ExternalActionPlanStatus::Applied);
        assert_eq!(
            response.credential_lease.status,
            CredentialLeaseStatus::Active
        );
        assert_eq!(
            response.credential_lease.provider_ref.as_deref(),
            Some("vault://leases/test-external-action")
        );
        assert_eq!(
            *provider_ref.lock().unwrap(),
            Some("vault://leases/test-external-action".to_string())
        );
        assert!(
            response
                .plan
                .result_ref
                .as_deref()
                .unwrap()
                .starts_with("write://action-executions/")
        );
        assert!(
            response
                .plan
                .compensation_ref
                .as_deref()
                .unwrap()
                .starts_with("compensate://action-executions/")
        );
        let lock = state
            .store
            .active_resource_lock("team", "project-alpha", "external_action")
            .await
            .unwrap();
        assert!(lock.is_none());
        let audits = state.store.list_audit(20).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_EXTERNAL_ACTION_APPLY
                && audit.decision == Some(AuditDecision::Completed)
                && audit.resource_id.as_deref() == Some(response.plan.id.as_str())
                && audit.reason.as_deref().is_some_and(|reason| {
                    reason.contains(&format!("lock_id={}", response.resource_lock.id))
                })
        }));

        let compensated = external_action_services::compensate_external_action_plan_with_connector(
            &state.store,
            &auth,
            run.id.clone(),
            response.plan.id.clone(),
            ExternalActionPlanCompensateInput {
                reason: Some("rollback test".to_string()),
                payload: json!({"mode": "test"}),
            },
            &connector,
        )
        .await
        .unwrap();
        assert_eq!(
            compensated.compensate_status,
            ExternalActionPlanStatus::Compensated
        );
        assert!(
            compensated
                .compensation_result_ref
                .as_deref()
                .unwrap()
                .starts_with("compensate-result://action-executions/")
        );
        let lock = state
            .store
            .active_resource_lock("team", "project-alpha", "external_action")
            .await
            .unwrap();
        assert!(lock.is_none());
        let audits = state.store.list_audit(20).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_EXTERNAL_ACTION_COMPENSATE
                && audit.decision == Some(AuditDecision::Completed)
                && audit.resource_id.as_deref() == Some(response.plan.id.as_str())
                && audit
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("lock_id="))
        }));
    }

    #[tokio::test]
    async fn external_action_apply_smokes_real_http_adapter_target() {
        let target_log = std::env::temp_dir().join(format!(
            "agent-platform-manager-action-gateway-smoke-{}.jsonl",
            new_trace_id()
        ));
        let adapter_url = spawn_server(
            agent_runtime::action_gateway_router(agent_runtime::ActionGatewayConfig {
                target_log_path: target_log.clone(),
                api_key: Some("secret-token".to_string()),
                lease_ttl_seconds: 60,
                connector: "action-journal".to_string(),
                allowed_credential_scopes: vec!["agent-platform:action-gateway-smoke".to_string()],
            })
            .unwrap(),
        )
        .await;
        let credential_provider = agent_runtime::HttpCredentialProvider::new(
            agent_runtime::HttpCredentialProviderConfig {
                base_url: adapter_url.clone(),
                api_key: Some("secret-token".to_string()),
                timeout: Duration::from_secs(2),
                lease_ttl_seconds: 300,
            },
        )
        .unwrap();
        let write_connector =
            agent_runtime::HttpWriteConnector::new(agent_runtime::HttpWriteConnectorConfig {
                base_url: adapter_url,
                api_key: Some("secret-token".to_string()),
                timeout: Duration::from_secs(2),
            })
            .unwrap();
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "external-action-http-adapter-smoke").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;
        let Json(dry_run) = admin_external_action_dry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(run.id.clone()),
            Json(ExternalActionPlanDryRunInput {
                connector: "action-journal".to_string(),
                action: "target.write".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("agent-platform:action-gateway-smoke".to_string()),
                approval_id: Some(approval_id),
                input_summary: Some("local smoke target write".to_string()),
                input_ref: Some("payload://external-action-http-smoke".to_string()),
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();

        let response = external_action_services::apply_external_action_plan_with_adapters(
            &state.store,
            &auth,
            run.id,
            dry_run.plan.id.clone(),
            ExternalActionPlanApplyInput {
                payload: json!({"message": "external action manager HTTP adapter smoke"}),
            },
            &credential_provider,
            &write_connector,
            external_action_services::ExternalActionApplyConfig {
                lock_lease: Duration::from_secs(30),
                max_attempts: 1,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.apply_status, ExternalActionPlanStatus::Applied);
        assert!(
            response
                .plan
                .result_ref
                .as_deref()
                .unwrap()
                .starts_with("action-journal-target://")
        );
        assert!(
            response
                .plan
                .compensation_ref
                .as_deref()
                .unwrap()
                .starts_with("action-journal-compensation://")
        );
        let log = std::fs::read_to_string(&target_log).unwrap();
        assert_eq!(
            log.matches("\"event_type\":\"credential_lease_issued\"")
                .count(),
            1
        );
        assert_eq!(log.matches("\"event_type\":\"action_executed\"").count(), 1);
        assert!(log.contains("\"idempotency_key\""));
        let _ = std::fs::remove_file(target_log);
    }

    #[tokio::test]
    async fn external_action_apply_rejects_invalid_connector_result() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "external-action-invalid-result").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;
        let Json(dry_run) = admin_external_action_dry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(run.id.clone()),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: Some(approval_id),
                input_summary: Some("comment on an issue".to_string()),
                input_ref: Some("payload://apply/invalid".to_string()),
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();

        let result = external_action_services::apply_external_action_plan_with_adapters(
            &state.store,
            &auth,
            run.id.clone(),
            dry_run.plan.id.clone(),
            ExternalActionPlanApplyInput {
                payload: json!({"body": "approved external write"}),
            },
            &TestCredentialProvider,
            &InvalidWriteConnector,
            external_action_services::ExternalActionApplyConfig {
                lock_lease: Duration::from_secs(30),
                max_attempts: 1,
            },
        )
        .await;

        assert!(result.is_err());
        let plan = state
            .store
            .get_external_action_plan(&dry_run.plan.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(plan.status, ExternalActionPlanStatus::Failed);
        assert_eq!(plan.error_code.as_deref(), Some("connector_invalid_result"));
        let lock = state
            .store
            .active_resource_lock("team", "project-alpha", "external_action")
            .await
            .unwrap();
        assert!(lock.is_none());
    }

    #[tokio::test]
    async fn external_action_apply_dead_letters_run_after_connector_retries() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run =
            create_read_only_run(&state, &auth, "external-action-connector-dead-letter").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;
        let Json(dry_run) = admin_external_action_dry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(run.id.clone()),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: Some(approval_id),
                input_summary: Some("comment on an issue".to_string()),
                input_ref: Some("payload://apply/dead-letter".to_string()),
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();
        let attempts = Arc::new(Mutex::new(0));
        let connector = FailingWriteConnector {
            attempts: attempts.clone(),
        };

        let result = external_action_services::apply_external_action_plan_with_adapters(
            &state.store,
            &auth,
            run.id.clone(),
            dry_run.plan.id.clone(),
            ExternalActionPlanApplyInput {
                payload: json!({"body": "approved external write"}),
            },
            &TestCredentialProvider,
            &connector,
            external_action_services::ExternalActionApplyConfig {
                lock_lease: Duration::from_secs(30),
                max_attempts: 2,
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(*attempts.lock().unwrap(), 2);
        let plan = state
            .store
            .get_external_action_plan(&dry_run.plan.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(plan.status, ExternalActionPlanStatus::Failed);
        assert_eq!(plan.error_code.as_deref(), Some("connector_dead_letter"));
        let run = state.store.get_run(&run.id).await.unwrap().unwrap();
        assert_eq!(run.run_status, AgentRunStatus::DeadLetter);
    }

    #[tokio::test]
    async fn external_action_apply_records_lock_conflict_from_precheck() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "external-action-apply-lock-conflict").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;
        let Json(dry_run) = admin_external_action_dry_run(
            State(state.clone()),
            HeaderMap::new(),
            Path(run.id.clone()),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: Some(approval_id),
                input_summary: Some("comment on an issue".to_string()),
                input_ref: Some("payload://apply/locked".to_string()),
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();
        state
            .store
            .acquire_resource_lock(
                ResourceLock {
                    id: new_id("lock"),
                    resource_type: "team".to_string(),
                    resource_id: "project-alpha".to_string(),
                    lock_scope: "external_action".to_string(),
                    holder_run_id: "run-other".to_string(),
                    lease_until: OffsetDateTime::now_utc(),
                    created_at: OffsetDateTime::now_utc(),
                },
                Duration::from_secs(30),
            )
            .await
            .unwrap();

        let result = external_action_services::apply_external_action_plan_with_adapters(
            &state.store,
            &auth,
            run.id.clone(),
            dry_run.plan.id.clone(),
            ExternalActionPlanApplyInput {
                payload: json!({"body": "approved external write"}),
            },
            &TestCredentialProvider,
            &RecordingWriteConnector {
                provider_ref: Arc::new(Mutex::new(None)),
            },
            external_action_services::ExternalActionApplyConfig {
                lock_lease: Duration::from_secs(30),
                max_attempts: 1,
            },
        )
        .await;

        assert!(result.is_err());
        let plan = state
            .store
            .get_external_action_plan(&dry_run.plan.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(plan.status, ExternalActionPlanStatus::Failed);
        assert_eq!(plan.error_code.as_deref(), Some("resource_locked"));
        let audits = state.store.list_audit(20).await.unwrap();
        assert!(audits.iter().any(|audit| {
            audit.action == actions::ADMIN_EXTERNAL_ACTION_APPLY
                && audit.decision == Some(AuditDecision::Conflict)
                && audit.resource_id.as_deref() == Some(plan.id.as_str())
        }));
    }

    #[tokio::test]
    async fn p1_external_action_dry_run_rejects_missing_approval() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "p1-external-action-no-approval").await;

        let Json(response) = admin_external_action_dry_run(
            State(state),
            HeaderMap::new(),
            Path(run.id),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: None,
                input_summary: Some("would comment on an issue".to_string()),
                input_ref: None,
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            response.dry_run_status,
            ExternalActionPlanStatus::DryRunRejected
        );
        assert_eq!(
            response.plan.error_code.as_deref(),
            Some("approval_required")
        );
        assert!(response.credential_lease.is_none());
    }

    #[tokio::test]
    async fn p1_external_action_dry_run_rejects_active_resource_lock() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let run = create_read_only_run(&state, &auth, "p1-external-action-locked").await;
        let approval_id = approved_external_action_approval(&state, &auth).await;
        state
            .store
            .acquire_resource_lock(
                ResourceLock {
                    id: new_id("lock"),
                    resource_type: "team".to_string(),
                    resource_id: "project-alpha".to_string(),
                    lock_scope: "external_action".to_string(),
                    holder_run_id: "run-other".to_string(),
                    lease_until: OffsetDateTime::now_utc(),
                    created_at: OffsetDateTime::now_utc(),
                },
                Duration::from_secs(30),
            )
            .await
            .unwrap();

        let Json(response) = admin_external_action_dry_run(
            State(state),
            HeaderMap::new(),
            Path(run.id),
            Json(ExternalActionPlanDryRunInput {
                connector: "github".to_string(),
                action: "issue.comment".to_string(),
                resource_ref: "resource:team/project-alpha".to_string(),
                credential_scope: Some("github:issues:write".to_string()),
                approval_id: Some(approval_id),
                input_summary: Some("would comment on an issue".to_string()),
                input_ref: None,
                risk_level: Some(RiskLevel::Low),
                external_action_mode: Some(ExternalActionMode::Authorized),
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            response.dry_run_status,
            ExternalActionPlanStatus::DryRunRejected
        );
        assert_eq!(response.plan.error_code.as_deref(), Some("resource_locked"));
        assert!(response.credential_lease.is_none());
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
                external_action_mode: Some(ExternalActionMode::ReadOnly),
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
        assert_eq!(run.external_action_mode, ExternalActionMode::ReadOnly);
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
                external_action_mode: Some(ExternalActionMode::ReadOnly),
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
            external_action_mode: Some(ExternalActionMode::ReadOnly),
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
                external_message_id: None,
                run_id: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(message.sequence, 1);
        assert_eq!(message.content_summary.as_deref(), Some("runtime response"));
    }
}
