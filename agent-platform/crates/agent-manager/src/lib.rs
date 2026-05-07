use agent_core::{
    AGENT_TYPE_BACKGROUND_WORKER, AgentCoreError, AgentInstance, AgentRequest, AgentRequestInput,
    AgentRequestResponse, AgentRequestStatus, AgentRun, AgentSession, AgentSessionMessage,
    AppendMessageInput, ApprovalDecisionInput, ApprovalRequest, ApprovalStatus, AuditDecision,
    AuditLog, AuthContext, CoreResult, CreateChildSessionInput, CreateRunInput, CreateSessionInput,
    DenyDecisionInput, EmptyResponse, ErrorCode, HealthStatus, ObserverReport, Page, PolicyContext,
    PolicyDecision, RequestType, ResourceRef, RiskLevel, RoleAssignment, RoleName, SafeError,
    SideEffectMode, TriggerType, actions, new_id, new_trace_id, request_action,
};
use agent_store::{AgentStore, MemoryAgentStore, PgAgentStore};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use time::OffsetDateTime;
use tower_http::trace::TraceLayer;

pub type StoreRef = Arc<dyn AgentStore>;

#[derive(Clone)]
pub struct AppState {
    pub store: StoreRef,
    pub config: Arc<ManagerConfig>,
}

#[derive(Debug, Clone)]
pub struct ManagerConfig {
    pub jwt_secret: Option<SecretString>,
    pub allow_dev_headers: bool,
    pub default_service_actions: Vec<String>,
}

impl ManagerConfig {
    pub fn from_env() -> Self {
        Self {
            jwt_secret: std::env::var("AGENT_JWT_SECRET")
                .ok()
                .filter(|value| !value.is_empty())
                .map(SecretString::from),
            allow_dev_headers: std::env::var("AGENT_ALLOW_DEV_HEADERS")
                .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
                .unwrap_or(true),
            default_service_actions: vec![
                "request:*".to_string(),
                "session:*".to_string(),
                "run:*".to_string(),
                "admin:*".to_string(),
                "internal:*".to_string(),
            ],
        }
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: SafeError,
}

impl ApiError {
    fn from_core(error: AgentCoreError, trace_id: impl Into<String>) -> Self {
        let code = error.code();
        let status = match code {
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::ApprovalRequired => StatusCode::ACCEPTED,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            body: SafeError::new(code, trace_id),
        }
    }

    fn unauthorized(trace_id: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: SafeError::new(ErrorCode::Unauthorized, trace_id),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceJwtClaims {
    sub: String,
    service_name: Option<String>,
    allowed_actions: Vec<String>,
    exp: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserJwtClaims {
    sub: String,
    roles: Vec<String>,
    resource_allowlist: Vec<String>,
    exp: usize,
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn parse_roles(value: &str) -> Vec<RoleAssignment> {
    value
        .split(',')
        .filter_map(|role| role.trim().parse::<RoleName>().ok())
        .map(RoleAssignment::global)
        .collect()
}

fn parse_csv(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn bearer(value: &str) -> Option<&str> {
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

pub fn extract_auth(headers: &HeaderMap, config: &ManagerConfig) -> Result<AuthContext, ApiError> {
    let trace_id = header_value(headers, "x-agent-trace-id").unwrap_or_else(new_trace_id);

    if let Some(secret) = &config.jwt_secret {
        let service_token = header_value(headers, "authorization")
            .and_then(|value| bearer(&value).map(ToString::to_string))
            .ok_or_else(|| ApiError::unauthorized(trace_id.clone()))?;
        let user_token = header_value(headers, "x-agent-user-token")
            .and_then(|value| bearer(&value).map(ToString::to_string).or(Some(value)))
            .ok_or_else(|| ApiError::unauthorized(trace_id.clone()))?;
        let service = decode::<ServiceJwtClaims>(
            &service_token,
            &DecodingKey::from_secret(secret.expose_secret().as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| ApiError::unauthorized(trace_id.clone()))?
        .claims;
        let user = decode::<UserJwtClaims>(
            &user_token,
            &DecodingKey::from_secret(secret.expose_secret().as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| ApiError::unauthorized(trace_id.clone()))?
        .claims;
        return Ok(AuthContext {
            user_id: user.sub,
            service_id: service.sub,
            service_allowed_actions: service.allowed_actions,
            roles: user
                .roles
                .into_iter()
                .filter_map(|role| role.parse::<RoleName>().ok())
                .map(RoleAssignment::global)
                .collect(),
            resource_allowlist: user.resource_allowlist,
            trace_id,
        });
    }

    if config.allow_dev_headers {
        let service_id = header_value(headers, "x-agent-service")
            .unwrap_or_else(|| "dev-orchestrator".to_string());
        let user_id =
            header_value(headers, "x-agent-user").unwrap_or_else(|| "dev-user".to_string());
        let roles = parse_roles(
            &header_value(headers, "x-agent-roles").unwrap_or_else(|| "system_admin".to_string()),
        );
        let allowed = parse_csv(header_value(headers, "x-agent-allowed-actions"));
        return Ok(AuthContext {
            user_id,
            service_id,
            service_allowed_actions: if allowed.is_empty() {
                config.default_service_actions.clone()
            } else {
                allowed
            },
            roles,
            resource_allowlist: parse_csv(header_value(headers, "x-agent-resource-allowlist"))
                .into_iter()
                .chain(std::iter::once("*".to_string()))
                .collect(),
            trace_id,
        });
    }

    Err(ApiError::unauthorized(trace_id))
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
        .route("/v1/admin/grants", post(admin_create_grant))
        .route("/v1/admin/observer/reports", get(admin_observer_reports))
        .route(
            "/v1/admin/observer/reports/{report_id}",
            get(admin_observer_report),
        )
        .route("/v1/admin/observer/runs", post(admin_observer_run))
        .route("/v1/internal/webhooks/{connector}", post(internal_webhook))
        .route("/v1/internal/runs", post(internal_create_run))
        .route("/v1/internal/runs/{run_id}/claim", post(internal_claim_run))
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

fn core_hash(payload: &Value) -> String {
    let mut hasher = Sha256::new();
    let encoded = serde_json::to_vec(payload).unwrap_or_default();
    hasher.update(encoded);
    format!("{:x}", hasher.finalize())
}

fn auth(headers: &HeaderMap, state: &AppState) -> Result<AuthContext, ApiError> {
    extract_auth(headers, &state.config)
}

fn policy_ctx(
    action: &str,
    request_type: Option<RequestType>,
    agent_type: Option<String>,
    target_resource: Option<String>,
    risk_level: RiskLevel,
    side_effect_mode: SideEffectMode,
) -> CoreResult<PolicyContext> {
    Ok(PolicyContext {
        action: action.to_string(),
        request_type,
        agent_type,
        resource: target_resource.map(ResourceRef::parse).transpose()?,
        risk_level,
        side_effect_mode,
        resource_attributes: Value::Null,
        observer_mode: false,
    })
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

async fn submit_request(
    state: &AppState,
    auth: &AuthContext,
    input: AgentRequestInput,
) -> Result<AgentRequestResponse, ApiError> {
    if let Some(key) = &input.idempotency_key
        && let Some(existing) = state
            .store
            .find_agent_request_by_idempotency(&auth.user_id, &auth.service_id, key)
            .await
            .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
    {
        return Ok(request_response(&existing));
    }

    let mut request = AgentRequest::new(
        auth,
        input.request_type,
        input.agent_type.clone(),
        input.target_resource.clone(),
        input.intent_text.clone(),
        input.structured_payload.clone(),
        input.idempotency_key,
    );
    request = state
        .store
        .create_agent_request(request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    request.status = AgentRequestStatus::Parsed;
    request = state
        .store
        .update_agent_request(request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;

    let risk_level = input.risk_level.unwrap_or(RiskLevel::Low);
    let side_effect_mode = input.side_effect_mode.unwrap_or_else(|| {
        if input.request_type == RequestType::CreateAgent {
            SideEffectMode::ApprovalRequired
        } else {
            SideEffectMode::ReadOnly
        }
    });
    let action = request_action(input.request_type);
    let policy = policy_ctx(
        action,
        Some(input.request_type),
        input.agent_type.clone(),
        input.target_resource.clone(),
        risk_level,
        side_effect_mode,
    )
    .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    let decision = agent_core::DefaultPolicy::authorize(auth, &policy);
    request.status = AgentRequestStatus::PolicyChecked;
    request = state
        .store
        .update_agent_request(request)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;

    match decision {
        PolicyDecision::Denied { reason } => {
            request.status = AgentRequestStatus::Denied;
            request.denial_reason = Some(reason.clone());
            request = state
                .store
                .update_agent_request(request)
                .await
                .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
            audit(
                state,
                Some(auth),
                action,
                AuditDecision::Denied,
                Some(reason),
                &auth.trace_id,
            )
            .await;
            Ok(request_response(&request))
        }
        PolicyDecision::ApprovalRequired { reason } => {
            let approval = ApprovalRequest {
                id: new_id("approval"),
                request_id: request.id.clone(),
                requested_by_user: auth.user_id.clone(),
                approver_user: None,
                status: ApprovalStatus::Pending,
                risk_level: Some(risk_level),
                reason: Some(reason.clone()),
                decision_reason: None,
                created_at: OffsetDateTime::now_utc(),
                decided_at: None,
            };
            let approval = state
                .store
                .create_approval(approval)
                .await
                .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
            request.status = AgentRequestStatus::ApprovalRequired;
            request.approval_id = Some(approval.id.clone());
            request = state
                .store
                .update_agent_request(request)
                .await
                .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
            audit(
                state,
                Some(auth),
                action,
                AuditDecision::ApprovalRequired,
                Some(reason),
                &auth.trace_id,
            )
            .await;
            Ok(request_response(&request))
        }
        PolicyDecision::Allowed => {
            audit(
                state,
                Some(auth),
                action,
                AuditDecision::Allowed,
                None,
                &auth.trace_id,
            )
            .await;
            fulfill_request(state, auth, request)
                .await
                .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))
        }
    }
}

async fn fulfill_request(
    state: &AppState,
    auth: &AuthContext,
    mut request: AgentRequest,
) -> CoreResult<AgentRequestResponse> {
    match request.request_type {
        RequestType::CreateAgent | RequestType::ChangeAgent | RequestType::ResumeAgent => {
            let agent_type = request
                .agent_type
                .clone()
                .unwrap_or_else(|| AGENT_TYPE_BACKGROUND_WORKER.to_string());
            let target_resource = request.target_resource.clone().ok_or_else(|| {
                AgentCoreError::coded(ErrorCode::Conflict, "target_resource required")
            })?;
            let hash = core_hash(&request.structured_payload);
            if let Some(existing) = state
                .store
                .find_reusable_agent(&auth.user_id, &agent_type, &target_resource, &hash)
                .await?
            {
                request.status = AgentRequestStatus::Fulfilled;
                request.result_agent_id = Some(existing.id);
                let request = state.store.update_agent_request(request).await?;
                return Ok(request_response(&request));
            }
            request.status = AgentRequestStatus::Provisioning;
            request = state.store.update_agent_request(request).await?;
            let mut agent = AgentInstance::new(
                auth.user_id.clone(),
                agent_type,
                target_resource,
                hash,
                request.structured_payload.clone(),
                auth.trace_id.clone(),
            );
            agent.display_name = request
                .intent_text
                .clone()
                .or_else(|| Some("P0 background worker".to_string()));
            let agent = state.store.create_agent_instance(agent).await?;
            request.status = AgentRequestStatus::Fulfilled;
            request.result_agent_id = Some(agent.id);
            let request = state.store.update_agent_request(request).await?;
            Ok(request_response(&request))
        }
        RequestType::CreateRun => {
            let payload_agent_id = request
                .structured_payload
                .get("agent_id")
                .and_then(Value::as_str)
                .ok_or_else(|| AgentCoreError::coded(ErrorCode::Conflict, "agent_id required"))?;
            let agent = state
                .store
                .get_agent(payload_agent_id)
                .await?
                .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "agent not found"))?;
            let run = AgentRun::new(
                agent.id,
                None,
                TriggerType::Manual,
                agent.target_resource,
                auth.trace_id.clone(),
            );
            let run = state.store.create_run(run).await?;
            request.status = AgentRequestStatus::Fulfilled;
            request.result_run_id = Some(run.id);
            let request = state.store.update_agent_request(request).await?;
            Ok(request_response(&request))
        }
        RequestType::CreateSession | RequestType::CreateChildSession => {
            request.status = AgentRequestStatus::Fulfilled;
            let request = state.store.update_agent_request(request).await?;
            Ok(request_response(&request))
        }
    }
}

fn request_response(request: &AgentRequest) -> AgentRequestResponse {
    let message = match request.status {
        AgentRequestStatus::ApprovalRequired => "该请求需要资源负责人审批。",
        AgentRequestStatus::Denied => "该请求已被策略拒绝。",
        AgentRequestStatus::Fulfilled => "请求已完成。",
        AgentRequestStatus::Cancelled => "请求已取消。",
        _ => "请求已记录。",
    };
    AgentRequestResponse {
        request_id: request.id.clone(),
        status: request.status,
        message: message.to_string(),
        approval_id: request.approval_id.clone(),
        agent_id: request.result_agent_id.clone(),
        run_id: request.result_run_id.clone(),
        trace_id: request.trace_id.clone(),
    }
}

fn ensure_admin(auth: &AuthContext, action: &str) -> Result<(), ApiError> {
    let ctx = PolicyContext {
        action: action.to_string(),
        request_type: None,
        agent_type: None,
        resource: None,
        risk_level: RiskLevel::Low,
        side_effect_mode: SideEffectMode::Deny,
        resource_attributes: Value::Null,
        observer_mode: false,
    };
    match agent_core::DefaultPolicy::authorize(auth, &ctx) {
        PolicyDecision::Allowed => Ok(()),
        PolicyDecision::Denied { reason } | PolicyDecision::ApprovalRequired { reason } => {
            Err(ApiError::from_core(
                AgentCoreError::coded(ErrorCode::Forbidden, reason),
                auth.trace_id.clone(),
            ))
        }
    }
}

async fn create_agent_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AgentRequestInput>,
) -> Result<Json<AgentRequestResponse>, ApiError> {
    let auth = auth(&headers, &state)?;
    submit_request(&state, &auth, input).await.map(Json)
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
    Ok(Json(request_response(&request)))
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
    Ok(Json(request_response(&request)))
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
    let agent = state
        .store
        .get_agent(&agent_id)
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
    let mut session = AgentSession::new(
        agent.id,
        auth.user_id.clone(),
        if input.resource_scope.is_null() {
            json!({"resource": agent.target_resource})
        } else {
            input.resource_scope
        },
        auth.trace_id.clone(),
    );
    session.source_conversation_id = input.source_conversation_id;
    let session = state
        .store
        .create_session(session)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::SESSION_CREATE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(session))
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
    if session.owner_user != auth.user_id {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::NotFound, "not found"),
            auth.trace_id,
        ));
    }
    let sequence = state
        .store
        .next_message_sequence(&session_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    let mut message = AgentSessionMessage::new(
        session_id,
        sequence,
        input.role,
        Some(input.content_summary),
        input.run_id,
        auth.trace_id.clone(),
    );
    message.content_ref = input.content_ref;
    let message = state
        .store
        .append_message(message)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::SESSION_APPEND_MESSAGE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(message))
}

async fn create_child_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(parent_session_id): Path<String>,
    Json(input): Json<CreateChildSessionInput>,
) -> Result<Json<AgentSession>, ApiError> {
    let auth = auth(&headers, &state)?;
    let parent = state
        .store
        .get_session(&parent_session_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?
        .ok_or_else(|| {
            ApiError::from_core(
                AgentCoreError::coded(ErrorCode::NotFound, "not found"),
                auth.trace_id.clone(),
            )
        })?;
    if parent.owner_user != auth.user_id || parent.depth >= 1 {
        return Err(ApiError::from_core(
            AgentCoreError::coded(
                ErrorCode::Conflict,
                "child session depth or owner constraint failed",
            ),
            auth.trace_id,
        ));
    }
    let children = state
        .store
        .list_child_sessions(&parent_session_id)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    let active_children = children
        .iter()
        .filter(|child| child.status == agent_core::AgentSessionStatus::Active)
        .count();
    if children.len() >= 3 || active_children >= 2 {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::Conflict, "child session budget exceeded"),
            auth.trace_id,
        ));
    }
    let mut child = AgentSession::new(
        input.agent_id.unwrap_or(parent.agent_id.clone()),
        auth.user_id.clone(),
        if input.resource_scope.is_null() {
            parent.resource_scope.clone()
        } else {
            input.resource_scope
        },
        auth.trace_id.clone(),
    );
    child.parent_session_id = Some(parent_session_id.clone());
    child.created_by_session_id = Some(parent_session_id);
    child.depth = parent.depth + 1;
    child.context_summary = input.context_summary.or(parent.context_summary);
    let child = state
        .store
        .create_session(child)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        "session:create_child",
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(child))
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
    let agent = state
        .store
        .get_agent(&agent_id)
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
    let mut run = AgentRun::new(
        agent.id,
        input.session_id,
        input.trigger_type,
        input.target_resource.unwrap_or(agent.target_resource),
        auth.trace_id.clone(),
    );
    run.idempotency_key = input.idempotency_key;
    run.risk_level = input.risk_level.unwrap_or(RiskLevel::Low);
    run.side_effect_mode = input.side_effect_mode.unwrap_or(SideEffectMode::ReadOnly);
    let policy = policy_ctx(
        actions::RUN_CREATE,
        Some(RequestType::CreateRun),
        Some(agent.agent_type),
        Some(run.target_resource.clone()),
        run.risk_level,
        run.side_effect_mode,
    )
    .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    match agent_core::DefaultPolicy::authorize(&auth, &policy) {
        PolicyDecision::Allowed => {}
        PolicyDecision::Denied { reason } | PolicyDecision::ApprovalRequired { reason } => {
            audit(
                &state,
                Some(&auth),
                actions::RUN_CREATE,
                AuditDecision::Denied,
                Some(reason.clone()),
                &auth.trace_id,
            )
            .await;
            return Err(ApiError::from_core(
                AgentCoreError::coded(ErrorCode::Forbidden, reason),
                auth.trace_id,
            ));
        }
    }
    let run = state
        .store
        .create_run(run)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    audit(
        &state,
        Some(&auth),
        actions::RUN_CREATE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(Json(run))
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
    let response = fulfill_request(&state, &auth, request)
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
    Ok(Json(request_response(&request)))
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

async fn admin_create_grant(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(_body): Json<Value>,
) -> Result<Json<EmptyResponse>, ApiError> {
    let trace_id = header_value(&headers, "x-agent-trace-id").unwrap_or_else(new_trace_id);
    Err(ApiError::from_core(
        AgentCoreError::coded(
            ErrorCode::Forbidden,
            "P0 keeps grant creation endpoint present but denies ad-hoc grants until policy schema is explicit",
        ),
        trace_id,
    ))
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
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
    if !auth.service_allows(actions::INTERNAL_WEBHOOK) {
        return Err(ApiError::from_core(
            AgentCoreError::coded(
                ErrorCode::Forbidden,
                "internal webhook service claim required",
            ),
            auth.trace_id,
        ));
    }
    audit(
        &state,
        Some(&auth),
        actions::INTERNAL_WEBHOOK,
        AuditDecision::Allowed,
        Some(format!("normalized connector webhook: {connector}")),
        &auth.trace_id,
    )
    .await;
    Ok(Json(json!({
        "status": "accepted",
        "connector": connector,
        "dedupe_key": body.get("dedupe_key"),
        "trace_id": auth.trace_id,
    })))
}

async fn internal_create_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut run): Json<AgentRun>,
) -> Result<Json<AgentRun>, ApiError> {
    let auth = auth(&headers, &state)?;
    if !auth.service_allows("internal:runs") && !auth.service_allows(actions::RUN_CREATE) {
        return Err(ApiError::from_core(
            AgentCoreError::coded(ErrorCode::Forbidden, "internal run service claim required"),
            auth.trace_id,
        ));
    }
    run.trace_id = auth.trace_id.clone();
    let run = state
        .store
        .create_run(run)
        .await
        .map_err(|error| ApiError::from_core(error, auth.trace_id.clone()))?;
    Ok(Json(run))
}

async fn internal_claim_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(_run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let auth = auth(&headers, &state)?;
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
    append_message(State(state), headers, Path(session_id), Json(input)).await
}

async fn internal_session_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<agent_core::SessionContext>, ApiError> {
    let auth = auth(&headers, &state)?;
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
    create_observer_report_from_snapshot(&state, &auth.trace_id)
        .await
        .map(Json)
        .map_err(|error| ApiError::from_core(error, auth.trace_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{AGENT_TYPE_BACKGROUND_WORKER, RoleAssignment};

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

    #[tokio::test]
    async fn create_agent_defaults_to_approval_required() {
        let state = test_state().await;
        let auth = test_auth(RoleName::ResourceMaintainer);
        let response = submit_request(
            &state,
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
        let response = submit_request(
            &state,
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
}
