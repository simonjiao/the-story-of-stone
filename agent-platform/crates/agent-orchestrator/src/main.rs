use agent_core::{
    AgentRequestInput, AgentRequestResponse, AppendMessageInput, MessageRole, RequestType,
    RiskLevel, SafeError, SideEffectMode, new_trace_id,
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
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, RwLock},
};
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
}

#[derive(Clone)]
struct AppState {
    manager_url: String,
    upstream_base_url: String,
    upstream_api_key: Option<String>,
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
    Json(payload): Json<Value>,
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
    match bound_session(&state, &request, &trace_id) {
        Ok(Some(session_id)) => control_response(
            request.model.as_deref(),
            request.stream.unwrap_or(false),
            append_session_message(&state, &headers, &session_id, &last_user, &trace_id).await,
            &trace_id,
        ),
        Ok(None) if looks_like_agent_request(&last_user) => control_response(
            request.model.as_deref(),
            request.stream.unwrap_or(false),
            submit_agent_request(&state, &headers, &last_user, &trace_id).await,
            &trace_id,
        ),
        Ok(None) => passthrough_chat_completion(&state, &headers, payload, &trace_id).await,
        Err(error) => control_response(
            request.model.as_deref(),
            request.stream.unwrap_or(false),
            Err(error),
            &trace_id,
        ),
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
    payload: Value,
    trace_id: &str,
) -> Response {
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

async fn submit_agent_request(
    state: &AppState,
    headers: &HeaderMap,
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
            }
        }),
        idempotency_key: None,
        risk_level: Some(RiskLevel::Low),
        side_effect_mode: Some(SideEffectMode::ApprovalRequired),
    };
    let response = state
        .client
        .post(format!("{}/v1/agent-requests", state.manager_url))
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
    let response = response
        .json::<AgentRequestResponse>()
        .await
        .map_err(|_| SafeError::new(agent_core::ErrorCode::InternalError, trace_id.to_string()))?;
    Ok(format!(
        "{} request_id={} status={} trace_id={}",
        response.message, response.request_id, response.status, response.trace_id
    ))
}

async fn append_session_message(
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
    use super::looks_like_agent_request;

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
USER: 创建 agent resource:team/default 进行 UI P0 smoke
ASSISTANT: 该请求需要资源负责人审批。
</chat_history>"#;

        assert!(!looks_like_agent_request(prompt));
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
