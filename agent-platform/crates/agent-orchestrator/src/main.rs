use agent_core::{
    AgentBridgeBindingSummary, AgentRequestInput, AgentRequestResponse, AgentRunStatus,
    AppendMessageInput, CreateRunInput, ErrorCode, MessageRole, RequestType, RiskLevel, RunSummary,
    SafeError, SideEffectMode, TriggerType, UpdateOpenWebUiBridgeRunInput, new_trace_id,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use clap::Parser;
use futures_util::stream;
use hmac::{Hmac, Mac};
use jsonwebtoken::{EncodingKey, Header, encode};
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, HashMap},
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};
use time::OffsetDateTime;
use tower_http::trace::TraceLayer;

#[derive(Debug, Parser)]
struct Args {
    #[arg(
        long,
        env = "AGENT_ORCHESTRATOR_BIND",
        default_value = "127.0.0.1:8080"
    )]
    bind: SocketAddr,
    #[arg(
        long,
        env = "AGENT_MANAGER_URL",
        default_value = "http://127.0.0.1:8088"
    )]
    manager_url: String,
    #[arg(
        long,
        env = "AGENT_ORCHESTRATOR_UPSTREAM_BASE_URL",
        default_value = "http://hermes:8642/v1"
    )]
    upstream_base_url: String,
    #[arg(long, env = "AGENT_ORCHESTRATOR_UPSTREAM_API_KEY")]
    upstream_api_key: Option<String>,
    #[arg(long, env = "AGENT_BRIDGE_SECRET")]
    agent_bridge_secret: Option<String>,
    #[arg(long, env = "AGENT_BRIDGE_ISSUER", default_value = "open-webui")]
    agent_bridge_issuer: String,
    #[arg(
        long,
        env = "AGENT_BRIDGE_MAX_CLOCK_SKEW_SECONDS",
        default_value_t = 300
    )]
    agent_bridge_max_clock_skew_seconds: i64,
    #[arg(
        long,
        env = "AGENT_BRIDGE_REQUIRED_FOR_CONTROL",
        default_value_t = true
    )]
    agent_bridge_required_for_control: bool,
    #[arg(
        long,
        env = "AGENT_BRIDGE_RESOURCE_ALLOWLIST",
        default_value = "resource:team/default"
    )]
    agent_bridge_resource_allowlist: String,
    #[arg(long, env = "AGENT_BRIDGE_USER_ROLE", default_value = "viewer")]
    agent_bridge_user_role: String,
    #[arg(
        long,
        env = "AGENT_BRIDGE_ADMIN_ROLE_MAPPING",
        default_value = "disabled"
    )]
    agent_bridge_admin_role_mapping: String,
    #[arg(long, env = "AGENT_JWT_SECRET")]
    agent_jwt_secret: Option<String>,
    #[arg(
        long,
        env = "AGENT_MANAGER_SERVICE_ID",
        default_value = "agent-orchestrator"
    )]
    agent_manager_service_id: String,
    #[arg(long, env = "AGENT_MANAGER_JWT_TTL_SECONDS", default_value_t = 300)]
    agent_manager_jwt_ttl_seconds: i64,
    #[arg(
        long,
        env = "AGENT_BRIDGE_RUN_WAIT_TIMEOUT_SECONDS",
        default_value_t = 20
    )]
    agent_bridge_run_wait_timeout_seconds: u64,
    #[arg(long, env = "AGENT_BRIDGE_RUN_POLL_INTERVAL_MS", default_value_t = 500)]
    agent_bridge_run_poll_interval_ms: u64,
}

#[derive(Clone)]
struct AppState {
    manager_url: String,
    upstream_base_url: String,
    upstream_api_key: Option<String>,
    bridge: BridgeConfig,
    manager_auth: ManagerAuthConfig,
    client: reqwest::Client,
    bindings: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
    metadata: Option<Value>,
    user: Option<String>,
    agent_bridge_context: Option<AgentBridgeContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct BindingInput {
    session_id: String,
}

#[derive(Clone)]
struct BridgeConfig {
    secret: Option<String>,
    issuer: String,
    max_clock_skew_seconds: i64,
    required_for_control: bool,
    resource_allowlist: Vec<String>,
    user_role: String,
    admin_role_mapping: String,
    run_wait_timeout: Duration,
    run_poll_interval: Duration,
}

#[derive(Clone)]
struct ManagerAuthConfig {
    jwt_secret: Option<String>,
    service_id: String,
    jwt_ttl_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentBridgeContext {
    version: u32,
    issuer: String,
    subject: String,
    #[serde(default)]
    user_role: Option<String>,
    chat_id: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    model: String,
    issued_at: i64,
    nonce: String,
    signature: String,
}

#[derive(Debug, Clone)]
struct TrustedBridgeContext {
    subject: String,
    user_role: String,
    chat_id: String,
    session_id: Option<String>,
    message_id: Option<String>,
    model: String,
    nonce: String,
}

#[derive(Debug, Serialize)]
struct ServiceJwtClaims {
    sub: String,
    service_name: Option<String>,
    allowed_actions: Vec<String>,
    exp: usize,
}

#[derive(Debug, Serialize)]
struct UserJwtClaims {
    sub: String,
    roles: Vec<String>,
    resource_allowlist: Vec<String>,
    exp: usize,
}

impl AppState {
    fn bind_session(
        &self,
        conversation_id: &str,
        session_id: &str,
        trace_id: &str,
    ) -> Result<(), SafeError> {
        let mut bindings = self
            .bindings
            .write()
            .map_err(|_| internal_error(trace_id))?;
        bindings.insert(conversation_id.to_string(), session_id.to_string());
        Ok(())
    }

    fn bound_session_by_conversation(
        &self,
        conversation_id: &str,
        trace_id: &str,
    ) -> Result<Option<String>, SafeError> {
        let bindings = self.bindings.read().map_err(|_| internal_error(trace_id))?;
        Ok(bindings.get(conversation_id).cloned())
    }
}

fn internal_error(trace_id: &str) -> SafeError {
    SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let args = Args::parse();
    let state = AppState {
        manager_url: args.manager_url,
        upstream_base_url: args.upstream_base_url.trim_end_matches('/').to_string(),
        upstream_api_key: args.upstream_api_key,
        bridge: BridgeConfig {
            secret: args.agent_bridge_secret.filter(|value| !value.is_empty()),
            issuer: args.agent_bridge_issuer,
            max_clock_skew_seconds: args.agent_bridge_max_clock_skew_seconds,
            required_for_control: args.agent_bridge_required_for_control,
            resource_allowlist: parse_csv(&args.agent_bridge_resource_allowlist),
            user_role: args.agent_bridge_user_role,
            admin_role_mapping: args.agent_bridge_admin_role_mapping,
            run_wait_timeout: Duration::from_secs(args.agent_bridge_run_wait_timeout_seconds),
            run_poll_interval: Duration::from_millis(args.agent_bridge_run_poll_interval_ms),
        },
        manager_auth: ManagerAuthConfig {
            jwt_secret: args.agent_jwt_secret.filter(|value| !value.is_empty()),
            service_id: args.agent_manager_service_id,
            jwt_ttl_seconds: args.agent_manager_jwt_ttl_seconds,
        },
        client: reqwest::Client::new(),
        bindings: Arc::new(RwLock::new(HashMap::new())),
    };
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route(
            "/v1/orchestrator/bindings/{conversation_id}",
            post(bind_session),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(%args.bind, "agent-orchestrator listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": "hermes-agent",
                "object": "model",
                "owned_by": "agent-orchestrator"
            }
        ]
    }))
}

async fn bind_session(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    Json(input): Json<BindingInput>,
) -> Response {
    let trace_id = new_trace_id();
    match state.bind_session(&conversation_id, &input.session_id, &trace_id) {
        Ok(()) => Json(json!({
            "status": "bound",
            "conversation_id": conversation_id,
            "session_id": input.session_id,
            "trace_id": trace_id,
        }))
        .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response(),
    }
}

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Response {
    let trace_id = header_value(&headers, "x-agent-trace-id").unwrap_or_else(new_trace_id);
    let request = match serde_json::from_value::<ChatCompletionRequest>(payload.clone()) {
        Ok(request) => request,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SafeError::new(
                    agent_core::ErrorCode::InternalError,
                    trace_id.to_string(),
                )),
            )
                .into_response();
        }
    };
    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    let stream = request.stream.unwrap_or(false);
    let model = request.model.as_deref();
    let bridge = match request
        .agent_bridge_context
        .as_ref()
        .map(|context| verify_bridge_context(&state, context, &trace_id))
        .transpose()
    {
        Ok(bridge) => bridge,
        Err(error)
            if looks_like_agent_request(&last_user) || looks_like_session_close(&last_user) =>
        {
            return control_response(model, stream, Err(error), &trace_id);
        }
        Err(_) => None,
    };

    if looks_like_agent_request(&last_user) {
        let bridge = match require_bridge(bridge.as_ref(), &state, &trace_id) {
            Ok(bridge) => bridge,
            Err(error) => return control_response(model, stream, Err(error), &trace_id),
        };
        return control_response(
            model,
            stream,
            submit_agent_request(&state, bridge, &last_user, &trace_id).await,
            &trace_id,
        );
    }

    if looks_like_session_close(&last_user) {
        let bridge = match require_bridge(bridge.as_ref(), &state, &trace_id) {
            Ok(bridge) => bridge,
            Err(error) => return control_response(model, stream, Err(error), &trace_id),
        };
        return control_response(
            model,
            stream,
            close_bridge_session(&state, bridge, &trace_id).await,
            &trace_id,
        );
    }

    if let Some(bridge) = bridge.as_ref() {
        match load_bridge_binding(&state, bridge, &trace_id).await {
            Ok(Some(binding)) => {
                return control_response(
                    model,
                    stream,
                    append_session_message_and_run(&state, bridge, &binding, &last_user, &trace_id)
                        .await,
                    &trace_id,
                );
            }
            Ok(None) => {}
            Err(error) => return control_response(model, stream, Err(error), &trace_id),
        }
    }

    match bound_session(&state, &request, &trace_id) {
        Ok(Some(session_id)) if !state.bridge.required_for_control => control_response(
            model,
            stream,
            append_session_message_legacy(&state, &headers, &session_id, &last_user, &trace_id)
                .await,
            &trace_id,
        ),
        Ok(_) => {
            strip_bridge_context(&mut payload);
            passthrough_chat_completion(&state, &headers, payload, &trace_id).await
        }
        Err(error) => control_response(model, stream, Err(error), &trace_id),
    }
}

fn control_response(
    model: Option<&str>,
    stream: bool,
    result: Result<String, SafeError>,
    trace_id: &str,
) -> Response {
    let content = match result {
        Ok(content) => content,
        Err(error) => serde_json::to_string(&error).unwrap_or_else(|_| {
            format!(
                r#"{{"error":"internal_error","message":"内部错误，请使用 trace_id 排查。","trace_id":"{}"}}"#,
                trace_id
            )
        }),
    };

    if stream {
        streaming_response(model.unwrap_or("hermes-agent"), content)
    } else {
        completion_response(model.unwrap_or("hermes-agent"), content)
    }
}

async fn passthrough_chat_completion(
    state: &AppState,
    headers: &HeaderMap,
    mut payload: Value,
    trace_id: &str,
) -> Response {
    strip_bridge_context(&mut payload);
    let mut request = state
        .client
        .post(format!("{}/chat/completions", state.upstream_base_url))
        .header("x-agent-trace-id", trace_id)
        .json(&payload);

    if let Some(api_key) = &state.upstream_api_key {
        request = request.bearer_auth(api_key);
    } else if let Some(authorization) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, authorization.clone());
    }

    match request.send().await {
        Ok(upstream) => upstream_response(upstream).await,
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(SafeError::new(
                agent_core::ErrorCode::InternalError,
                trace_id.to_string(),
            )),
        )
            .into_response(),
    }
}

async fn upstream_response(upstream: reqwest::Response) -> Response {
    let status = upstream.status();
    let mut response = Response::builder().status(status);
    for name in [header::CONTENT_TYPE, header::CACHE_CONTROL] {
        if let Some(value) = upstream.headers().get(&name) {
            response = response.header(name, value);
        }
    }
    let body = Body::from_stream(upstream.bytes_stream());
    response.body(body).unwrap_or_else(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "upstream_response_failed"})),
        )
            .into_response()
    })
}

fn completion_response(model: &str, content: String) -> Response {
    Json(json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::now_v7().simple()),
        "object": "chat.completion",
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": "stop"
            }
        ]
    }))
    .into_response()
}

fn streaming_response(model: &str, content: String) -> Response {
    let chunk = json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::now_v7().simple()),
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [
            {
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": null
            }
        ]
    })
    .to_string();
    let done = json!({
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    })
    .to_string();
    let events = stream::iter(vec![
        Ok::<Event, Infallible>(Event::default().data(chunk)),
        Ok(Event::default().data(done)),
        Ok(Event::default().data("[DONE]")),
    ]);
    Sse::new(events).into_response()
}

fn looks_like_agent_request(content: &str) -> bool {
    let direct_content = direct_user_control_text(content);
    let lowered = direct_content.to_lowercase();
    lowered.contains("agent")
        && (direct_content.contains("启动")
            || direct_content.contains("创建")
            || direct_content.contains("常驻")
            || lowered.contains("create"))
}

fn looks_like_session_close(content: &str) -> bool {
    let direct_content = direct_user_control_text(content);
    direct_content.contains("agent")
        && (direct_content.contains("结束")
            || direct_content.contains("关闭")
            || direct_content.contains("退出"))
        && direct_content.contains("session")
}

fn direct_user_control_text(content: &str) -> &str {
    content
        .split("### Chat History:")
        .next()
        .unwrap_or(content)
        .split("<chat_history>")
        .next()
        .unwrap_or(content)
        .trim()
}

fn bound_session(
    state: &AppState,
    request: &ChatCompletionRequest,
    trace_id: &str,
) -> Result<Option<String>, SafeError> {
    if let Some(session_id) = request
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent_session_id"))
        .and_then(Value::as_str)
    {
        return Ok(Some(session_id.to_string()));
    }
    let conversation_id = request
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("conversation_id"))
        .and_then(Value::as_str)
        .or(request.user.as_deref());
    match conversation_id {
        Some(conversation_id) => state.bound_session_by_conversation(conversation_id, trace_id),
        None => Ok(None),
    }
}

fn verify_bridge_context(
    state: &AppState,
    context: &AgentBridgeContext,
    trace_id: &str,
) -> Result<TrustedBridgeContext, SafeError> {
    let Some(secret) = &state.bridge.secret else {
        return Err(SafeError::new(
            ErrorCode::Unauthorized,
            trace_id.to_string(),
        ));
    };
    if context.version != 1
        || context.issuer != state.bridge.issuer
        || !context.subject.starts_with("openwebui:")
        || context.chat_id.trim().is_empty()
        || context.model != "hermes-agent"
        || context.nonce.trim().is_empty()
    {
        return Err(SafeError::new(
            ErrorCode::Unauthorized,
            trace_id.to_string(),
        ));
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    if (now - context.issued_at).abs() > state.bridge.max_clock_skew_seconds {
        return Err(SafeError::new(
            ErrorCode::Unauthorized,
            trace_id.to_string(),
        ));
    }
    let expected = bridge_signature(secret, context)
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if !constant_time_eq(expected.as_bytes(), context.signature.as_bytes()) {
        return Err(SafeError::new(
            ErrorCode::Unauthorized,
            trace_id.to_string(),
        ));
    }
    Ok(TrustedBridgeContext {
        subject: context.subject.clone(),
        user_role: context
            .user_role
            .clone()
            .unwrap_or_else(|| "user".to_string()),
        chat_id: context.chat_id.clone(),
        session_id: context.session_id.clone(),
        message_id: context.message_id.clone(),
        model: context.model.clone(),
        nonce: context.nonce.clone(),
    })
}

fn require_bridge<'a>(
    bridge: Option<&'a TrustedBridgeContext>,
    state: &AppState,
    trace_id: &str,
) -> Result<&'a TrustedBridgeContext, SafeError> {
    if !state.bridge.required_for_control {
        return bridge.ok_or_else(|| SafeError::new(ErrorCode::Unauthorized, trace_id.to_string()));
    }
    bridge.ok_or_else(|| SafeError::new(ErrorCode::Unauthorized, trace_id.to_string()))
}

fn bridge_signature(secret: &str, context: &AgentBridgeContext) -> Result<String, String> {
    type HmacSha256 = Hmac<Sha256>;
    let payload = bridge_signing_payload(context)?;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|error| error.to_string())?;
    mac.update(payload.as_bytes());
    Ok(hex_encode(&mac.finalize().into_bytes()))
}

fn bridge_signing_payload(context: &AgentBridgeContext) -> Result<String, String> {
    let mut payload = BTreeMap::new();
    payload.insert("version".to_string(), json!(context.version));
    payload.insert("issuer".to_string(), json!(context.issuer.clone()));
    payload.insert("subject".to_string(), json!(context.subject.clone()));
    payload.insert(
        "user_role".to_string(),
        json!(context.user_role.clone().unwrap_or_default()),
    );
    payload.insert("chat_id".to_string(), json!(context.chat_id.clone()));
    payload.insert(
        "session_id".to_string(),
        json!(context.session_id.clone().unwrap_or_default()),
    );
    payload.insert(
        "message_id".to_string(),
        json!(context.message_id.clone().unwrap_or_default()),
    );
    payload.insert("model".to_string(), json!(context.model.clone()));
    payload.insert("issued_at".to_string(), json!(context.issued_at));
    payload.insert("nonce".to_string(), json!(context.nonce.clone()));
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

fn manager_headers(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    trace_id: &str,
) -> Result<ReqHeaderMap, SafeError> {
    let mut headers = ReqHeaderMap::new();
    if let Some(secret) = &state.manager_auth.jwt_secret {
        let exp = (OffsetDateTime::now_utc()
            + time::Duration::seconds(state.manager_auth.jwt_ttl_seconds))
        .unix_timestamp() as usize;
        let service = ServiceJwtClaims {
            sub: state.manager_auth.service_id.clone(),
            service_name: Some("agent-orchestrator".to_string()),
            allowed_actions: vec![
                "request:*".to_string(),
                "session:*".to_string(),
                "run:*".to_string(),
                "internal:*".to_string(),
            ],
            exp,
        };
        let user = UserJwtClaims {
            sub: bridge.subject.clone(),
            roles: vec![mapped_agent_role(state, bridge)],
            resource_allowlist: state.bridge.resource_allowlist.clone(),
            exp,
        };
        let key = EncodingKey::from_secret(secret.as_bytes());
        let service_token = encode(&Header::default(), &service, &key)
            .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
        let user_token = encode(&Header::default(), &user, &key)
            .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {service_token}"))
                .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-agent-user-token"),
            HeaderValue::from_str(&user_token)
                .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?,
        );
    } else {
        headers.insert(
            HeaderName::from_static("x-agent-service"),
            HeaderValue::from_str(&state.manager_auth.service_id)
                .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-agent-user"),
            HeaderValue::from_str(&bridge.subject)
                .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-agent-roles"),
            HeaderValue::from_str(&mapped_agent_role(state, bridge))
                .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-agent-resource-allowlist"),
            HeaderValue::from_str(&state.bridge.resource_allowlist.join(","))
                .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-agent-allowed-actions"),
            HeaderValue::from_static("request:*,session:*,run:*,internal:*"),
        );
    }
    headers.insert(
        HeaderName::from_static("x-agent-trace-id"),
        HeaderValue::from_str(trace_id)
            .unwrap_or_else(|_| HeaderValue::from_static("trace_invalid")),
    );
    Ok(headers)
}

fn mapped_agent_role(state: &AppState, bridge: &TrustedBridgeContext) -> String {
    if bridge.user_role == "admin" && state.bridge.admin_role_mapping == "agent_admin" {
        "agent_admin".to_string()
    } else {
        state.bridge.user_role.clone()
    }
}

fn bridge_idempotency_key(bridge: &TrustedBridgeContext, kind: &str) -> String {
    let message_key = bridge.message_id.as_deref().unwrap_or(&bridge.nonce);
    format!(
        "openwebui:{}:{}:{}:{}",
        bridge.subject, bridge.chat_id, kind, message_key
    )
}

async fn submit_agent_request(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    content: &str,
    trace_id: &str,
) -> Result<String, SafeError> {
    let body = AgentRequestInput {
        request_type: RequestType::CreateAgent,
        agent_type: Some("background_worker".to_string()),
        target_resource: extract_resource(content)
            .or_else(|| Some("resource:team/default".to_string())),
        intent_text: Some(content.to_string()),
        structured_payload: json!({
            "constraints": {
                "trigger_mode": "manual",
                "allowed_actions": ["analyze", "prepare_change", "run_checks"],
                "require_approval_for_side_effects": true
            },
            "bridge_source": {
                "kind": "open_webui",
                "chat_id": bridge.chat_id.clone(),
                "session_id": bridge.session_id.clone(),
                "message_id": bridge.message_id.clone(),
                "model": bridge.model.clone()
            }
        }),
        idempotency_key: Some(bridge_idempotency_key(bridge, "request")),
        risk_level: Some(RiskLevel::Low),
        side_effect_mode: Some(SideEffectMode::ApprovalRequired),
    };
    let response = state
        .client
        .post(format!("{}/v1/agent-requests", state.manager_url))
        .headers(manager_headers(state, bridge, trace_id)?)
        .json(&body)
        .send()
        .await
        .map_err(|_| SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string()))?;
    if !response.status().is_success() {
        return Err(response.json::<SafeError>().await.unwrap_or_else(|_| {
            SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string())
        }));
    }
    let response = response
        .json::<AgentRequestResponse>()
        .await
        .map_err(|_| SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string()))?;
    let binding = if response.agent_id.is_some() {
        load_bridge_binding(state, bridge, trace_id)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let mut summary = format!(
        "{} request_id={} status={} trace_id={}",
        response.message, response.request_id, response.status, response.trace_id
    );
    if let Some(approval_id) = response.approval_id {
        summary.push_str(&format!(" approval_id={approval_id}"));
    }
    if let Some(agent_id) = response.agent_id {
        summary.push_str(&format!(" agent_id={agent_id}"));
    }
    if let Some(binding) = binding {
        summary.push_str(&format!(
            " session_id={} binding_id={}",
            binding.agent_session_id, binding.binding_id
        ));
    }
    Ok(summary)
}

async fn append_session_message_legacy(
    state: &AppState,
    headers: &HeaderMap,
    session_id: &str,
    content: &str,
    trace_id: &str,
) -> Result<String, SafeError> {
    let body = AppendMessageInput {
        role: MessageRole::User,
        content_summary: content.to_string(),
        content_ref: None,
        run_id: None,
    };
    let response = state
        .client
        .post(format!(
            "{}/v1/agent-sessions/{}/messages",
            state.manager_url, session_id
        ))
        .headers(forward_headers(headers, trace_id))
        .json(&body)
        .send()
        .await
        .map_err(|_| SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string()))?;
    if !response.status().is_success() {
        return Err(response.json::<SafeError>().await.unwrap_or_else(|_| {
            SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string())
        }));
    }
    Ok(format!(
        "session {} message appended trace_id={}",
        session_id, trace_id
    ))
}

async fn append_session_message_and_run(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    binding: &AgentBridgeBindingSummary,
    content: &str,
    trace_id: &str,
) -> Result<String, SafeError> {
    let message = AppendMessageInput {
        role: MessageRole::User,
        content_summary: content.to_string(),
        content_ref: None,
        run_id: None,
    };
    let response = state
        .client
        .post(format!(
            "{}/v1/agent-sessions/{}/messages",
            state.manager_url, binding.agent_session_id
        ))
        .headers(manager_headers(state, bridge, trace_id)?)
        .json(&message)
        .send()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if !response.status().is_success() {
        return Err(response
            .json::<SafeError>()
            .await
            .unwrap_or_else(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string())));
    }

    let run_input = CreateRunInput {
        session_id: Some(binding.agent_session_id.clone()),
        trigger_type: TriggerType::SessionMessage,
        idempotency_key: Some(bridge_idempotency_key(bridge, "run")),
        target_resource: None,
        risk_level: Some(RiskLevel::Low),
        side_effect_mode: Some(SideEffectMode::ReadOnly),
    };
    let response = state
        .client
        .post(format!(
            "{}/v1/my-agents/{}/runs",
            state.manager_url, binding.agent_id
        ))
        .headers(manager_headers(state, bridge, trace_id)?)
        .json(&run_input)
        .send()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if !response.status().is_success() {
        return Err(response
            .json::<SafeError>()
            .await
            .unwrap_or_else(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string())));
    }
    let run = response
        .json::<agent_core::AgentRun>()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    update_bridge_run(state, bridge, &binding.binding_id, &run.id, trace_id).await?;
    wait_for_run(state, bridge, &run.id, trace_id).await
}

async fn close_bridge_session(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    trace_id: &str,
) -> Result<String, SafeError> {
    let Some(binding) = load_bridge_binding(state, bridge, trace_id).await? else {
        return Ok(format!(
            "当前 Open WebUI chat 没有 active agent session。trace_id={trace_id}"
        ));
    };
    let response = state
        .client
        .post(format!(
            "{}/v1/agent-sessions/{}/close",
            state.manager_url, binding.agent_session_id
        ))
        .headers(manager_headers(state, bridge, trace_id)?)
        .send()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if !response.status().is_success() {
        return Err(response
            .json::<SafeError>()
            .await
            .unwrap_or_else(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string())));
    }
    close_bridge_binding(state, bridge, trace_id).await?;
    Ok(format!(
        "agent session closed session_id={} trace_id={trace_id}",
        binding.agent_session_id
    ))
}

async fn load_bridge_binding(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    trace_id: &str,
) -> Result<Option<AgentBridgeBindingSummary>, SafeError> {
    let response = state
        .client
        .get(format!(
            "{}/v1/internal/open-webui-bridge/bindings/{}?model={}",
            state.manager_url,
            path_segment_escape(&bridge.chat_id),
            path_segment_escape(&bridge.model)
        ))
        .headers(manager_headers(state, bridge, trace_id)?)
        .send()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(response
            .json::<SafeError>()
            .await
            .unwrap_or_else(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string())));
    }
    response
        .json::<AgentBridgeBindingSummary>()
        .await
        .map(Some)
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))
}

async fn update_bridge_run(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    binding_id: &str,
    run_id: &str,
    trace_id: &str,
) -> Result<(), SafeError> {
    let input = UpdateOpenWebUiBridgeRunInput {
        message_id: bridge.message_id.clone(),
        run_id: run_id.to_string(),
    };
    let response = state
        .client
        .post(format!(
            "{}/v1/internal/open-webui-bridge/bindings/{}/run",
            state.manager_url,
            path_segment_escape(binding_id)
        ))
        .headers(manager_headers(state, bridge, trace_id)?)
        .json(&input)
        .send()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response
            .json::<SafeError>()
            .await
            .unwrap_or_else(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string())))
    }
}

async fn close_bridge_binding(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    trace_id: &str,
) -> Result<(), SafeError> {
    let response = state
        .client
        .post(format!(
            "{}/v1/internal/open-webui-bridge/bindings/{}/close?model={}",
            state.manager_url,
            path_segment_escape(&bridge.chat_id),
            path_segment_escape(&bridge.model)
        ))
        .headers(manager_headers(state, bridge, trace_id)?)
        .send()
        .await
        .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response
            .json::<SafeError>()
            .await
            .unwrap_or_else(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string())))
    }
}

async fn wait_for_run(
    state: &AppState,
    bridge: &TrustedBridgeContext,
    run_id: &str,
    trace_id: &str,
) -> Result<String, SafeError> {
    let started = tokio::time::Instant::now();
    loop {
        let response = state
            .client
            .get(format!(
                "{}/v1/my-runs/{}",
                state.manager_url,
                path_segment_escape(run_id)
            ))
            .headers(manager_headers(state, bridge, trace_id)?)
            .send()
            .await
            .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
        if !response.status().is_success() {
            return Err(response.json::<SafeError>().await.unwrap_or_else(|_| {
                SafeError::new(ErrorCode::InternalError, trace_id.to_string())
            }));
        }
        let run = response
            .json::<RunSummary>()
            .await
            .map_err(|_| SafeError::new(ErrorCode::InternalError, trace_id.to_string()))?;
        match run.run_status {
            AgentRunStatus::Completed => {
                return Ok(format!(
                    "{} run_id={} session_id={} trace_id={}",
                    run.result_summary
                        .unwrap_or_else(|| "run completed".to_string()),
                    run.run_id,
                    run.session_id.unwrap_or_default(),
                    run.trace_id
                ));
            }
            AgentRunStatus::Failed
            | AgentRunStatus::Cancelled
            | AgentRunStatus::TimedOut
            | AgentRunStatus::DeadLetter => {
                return Ok(format!(
                    "run_id={} status={} trace_id={}",
                    run.run_id, run.run_status, run.trace_id
                ));
            }
            _ if started.elapsed() >= state.bridge.run_wait_timeout => {
                return Ok(format!(
                    "run_id={} status={} trace_id={} still running",
                    run.run_id, run.run_status, run.trace_id
                ));
            }
            _ => tokio::time::sleep(state.bridge.run_poll_interval).await,
        }
    }
}

fn extract_resource(content: &str) -> Option<String> {
    content
        .split_whitespace()
        .find(|part| part.starts_with("resource:"))
        .map(|part| {
            part.trim_matches(|c: char| c == '。' || c == ',' || c == '.')
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(secret: Option<&str>) -> AppState {
        AppState {
            manager_url: "http://manager".to_string(),
            upstream_base_url: "http://hermes/v1".to_string(),
            upstream_api_key: None,
            bridge: BridgeConfig {
                secret: secret.map(ToString::to_string),
                issuer: "open-webui".to_string(),
                max_clock_skew_seconds: 300,
                required_for_control: true,
                resource_allowlist: vec!["resource:team/default".to_string()],
                user_role: "viewer".to_string(),
                admin_role_mapping: "disabled".to_string(),
                run_wait_timeout: Duration::from_secs(1),
                run_poll_interval: Duration::from_millis(10),
            },
            manager_auth: ManagerAuthConfig {
                jwt_secret: Some("manager-secret".to_string()),
                service_id: "agent-orchestrator".to_string(),
                jwt_ttl_seconds: 300,
            },
            client: reqwest::Client::new(),
            bindings: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn signed_context(secret: &str) -> AgentBridgeContext {
        let mut context = AgentBridgeContext {
            version: 1,
            issuer: "open-webui".to_string(),
            subject: "openwebui:user-1".to_string(),
            user_role: Some("user".to_string()),
            chat_id: "chat-1".to_string(),
            session_id: Some("browser-session-1".to_string()),
            message_id: Some("message-1".to_string()),
            model: "hermes-agent".to_string(),
            issued_at: OffsetDateTime::now_utc().unix_timestamp(),
            nonce: "nonce-1".to_string(),
            signature: String::new(),
        };
        context.signature = bridge_signature(secret, &context).unwrap();
        context
    }

    #[test]
    fn detects_direct_agent_create_request() {
        assert!(looks_like_agent_request(
            "创建 agent resource:team/default 进行 smoke test"
        ));
    }

    #[test]
    fn ignores_open_webui_followup_prompt_chat_history() {
        let prompt = r#"### Task:
Suggest 3-5 relevant follow-up questions or prompts.
### Output:
JSON format: { "follow_ups": ["Question 1?"] }
### Chat History:
<chat_history>
USER: 创建 agent resource:team/default 进行 UI Agent Platform smoke
ASSISTANT: 该请求需要资源负责人审批。
</chat_history>"#;

        assert!(!looks_like_agent_request(prompt));
    }

    #[test]
    fn verifies_signed_bridge_context() {
        let state = test_state(Some("bridge-secret"));
        let context = signed_context("bridge-secret");
        let verified = verify_bridge_context(&state, &context, "trace-test").unwrap();
        assert_eq!(verified.subject, "openwebui:user-1");
        assert_eq!(verified.chat_id, "chat-1");
    }

    #[test]
    fn bridge_signature_matches_filter_canonical_payload() {
        let mut context = signed_context("bridge-secret");
        context.issued_at = 1778220000;
        context.nonce = "nonce-1".to_string();
        context.session_id = Some("session-1".to_string());
        context.signature.clear();
        assert_eq!(
            bridge_signature("bridge-secret", &context).unwrap(),
            "6185debba03afb3b99ac20a9ff87d93757940034dc9b3ccef7c83247004fbb10"
        );
    }

    #[test]
    fn rejects_wrong_bridge_signature() {
        let state = test_state(Some("bridge-secret"));
        let mut context = signed_context("bridge-secret");
        context.chat_id = "chat-2".to_string();
        assert!(verify_bridge_context(&state, &context, "trace-test").is_err());
    }

    #[test]
    fn rejects_expired_bridge_context() {
        let state = test_state(Some("bridge-secret"));
        let mut context = signed_context("bridge-secret");
        context.issued_at -= 600;
        context.signature = bridge_signature("bridge-secret", &context).unwrap();
        assert!(verify_bridge_context(&state, &context, "trace-test").is_err());
    }

    #[test]
    fn strips_bridge_context_before_passthrough() {
        let mut payload = json!({
            "model": "hermes-agent",
            "messages": [{"role": "user", "content": "hello"}],
            "agent_bridge_context": {"secret": "do-not-forward"}
        });
        strip_bridge_context(&mut payload);
        assert!(payload.get("agent_bridge_context").is_none());
    }
}

fn forward_headers(headers: &HeaderMap, trace_id: &str) -> ReqHeaderMap {
    let mut forwarded = ReqHeaderMap::new();
    for name in [
        "authorization",
        "x-agent-user-token",
        "x-agent-service",
        "x-agent-user",
        "x-agent-roles",
        "x-agent-allowed-actions",
        "x-agent-resource-allowlist",
    ] {
        if let Some(value) = headers.get(name)
            && let Ok(header_name) = HeaderName::from_bytes(name.as_bytes())
        {
            forwarded.insert(header_name, value.clone());
        }
    }
    forwarded.insert(
        HeaderName::from_static("x-agent-trace-id"),
        HeaderValue::from_str(trace_id)
            .unwrap_or_else(|_| HeaderValue::from_static("trace_invalid")),
    );
    forwarded
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn strip_bridge_context(payload: &mut Value) {
    if let Some(object) = payload.as_object_mut() {
        object.remove("agent_bridge_context");
    }
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

fn path_segment_escape(value: &str) -> String {
    let mut escaped = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            escaped.push(byte as char);
        } else {
            escaped.push('%');
            escaped.push_str(&format!("{byte:02X}"));
        }
    }
    escaped
}
