use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex, RwLock},
};
use tower_http::trace::TraceLayer;

#[derive(Debug, Parser)]
#[command(name = "global-router")]
#[command(about = "OpenAI-compatible model allowlist and gateway router")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve(ServeArgs),
    PrintConfig(PrintConfigArgs),
}

#[derive(Debug, Parser, Clone)]
struct ServeArgs {
    #[arg(long, env = "GLOBAL_ROUTER_BIND", default_value = "127.0.0.1:8099")]
    bind: SocketAddr,
    #[arg(long, env = "GLOBAL_ROUTER_ROUTES_JSON")]
    routes_json: Option<String>,
    #[arg(long, env = "GLOBAL_ROUTER_ROUTES_FILE")]
    routes_file: Option<PathBuf>,
    #[arg(long, env = "GLOBAL_ROUTER_INBOUND_API_KEYS")]
    inbound_api_keys: Option<String>,
    #[arg(long, env = "GLOBAL_ROUTER_ADMIN_API_KEY")]
    admin_api_key: Option<String>,
    #[arg(long, env = "GLOBAL_ROUTER_BRIDGE_SECRET")]
    bridge_secret: Option<String>,
    #[arg(long, env = "GLOBAL_ROUTER_BRIDGE_ISSUER")]
    bridge_issuer: Option<String>,
    #[arg(long, env = "GLOBAL_ROUTER_BRIDGE_MAX_CLOCK_SKEW_SECONDS")]
    bridge_max_clock_skew_seconds: Option<i64>,
    #[arg(long, env = "GLOBAL_ROUTER_AUDIT_LOG_PATH")]
    audit_log_path: Option<PathBuf>,
}

#[derive(Debug, Parser, Clone)]
struct PrintConfigArgs {
    #[arg(long, env = "GLOBAL_ROUTER_ROUTES_JSON")]
    routes_json: Option<String>,
    #[arg(long, env = "GLOBAL_ROUTER_ROUTES_FILE")]
    routes_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct RouteConfig {
    model: String,
    #[serde(default)]
    name: Option<String>,
    base_url: String,
    #[serde(default)]
    upstream_model: Option<String>,
    #[serde(default)]
    requires_bridge: bool,
    #[serde(default)]
    api_key_env: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    discover_models: bool,
    #[serde(default)]
    allowed_user_roles: Vec<String>,
    #[serde(default)]
    allowed_subjects: Vec<String>,
    #[serde(default)]
    failure_threshold: Option<u32>,
    #[serde(default)]
    circuit_breaker_seconds: Option<u64>,
    #[serde(default)]
    fallback_model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RouteView {
    model: String,
    name: Option<String>,
    upstream_model: Option<String>,
    base_url: String,
    requires_bridge: bool,
    has_api_key_env: bool,
    timeout_seconds: u64,
    discover_models: bool,
    allowed_user_roles: Vec<String>,
    has_subject_allowlist: bool,
    failure_threshold: u32,
    circuit_breaker_seconds: u64,
    fallback_model: Option<String>,
}

#[derive(Debug, Clone)]
struct Route {
    model: String,
    name: Option<String>,
    base_url: String,
    upstream_model: Option<String>,
    requires_bridge: bool,
    api_key: Option<String>,
    timeout: Duration,
    discover_models: bool,
    allowed_user_roles: Vec<String>,
    allowed_subjects: Vec<String>,
    failure_threshold: u32,
    circuit_breaker: Duration,
    fallback_model: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedRoute {
    route: Route,
    visible_model: String,
    upstream_model: String,
}

#[derive(Debug, Clone)]
struct ConfigSource {
    routes_json: Option<String>,
    routes_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct SecurityConfig {
    inbound_api_keys: Vec<String>,
    admin_api_key: Option<String>,
    bridge: BridgeSecurityConfig,
}

#[derive(Debug, Clone)]
struct BridgeSecurityConfig {
    secret: Option<String>,
    issuer: String,
    max_clock_skew_seconds: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct RouteHealth {
    consecutive_failures: u32,
    circuit_open_until_ms: Option<u128>,
    last_success_ms: Option<u128>,
    last_failure_ms: Option<u128>,
    last_status: Option<u16>,
    last_error: Option<String>,
}

#[derive(Clone)]
struct AuditSink {
    path: Option<PathBuf>,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Serialize)]
struct AuditEvent {
    timestamp_ms: u128,
    trace_id: String,
    action: String,
    decision: String,
    model: Option<String>,
    route_model: Option<String>,
    upstream_model: Option<String>,
    subject: Option<String>,
    user_role: Option<String>,
    status: u16,
    reason: Option<String>,
    upstream_status: Option<u16>,
}

#[derive(Clone)]
struct AppState {
    routes: Arc<RwLock<BTreeMap<String, Route>>>,
    health: Arc<RwLock<BTreeMap<String, RouteHealth>>>,
    bridge_nonces: Arc<RwLock<BTreeMap<String, u128>>>,
    client: reqwest::Client,
    config_source: ConfigSource,
    security: SecurityConfig,
    audit: AuditSink,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
}

enum ForwardOutcome {
    Success(Response),
    UpstreamError {
        status: StatusCode,
        message: String,
        response: Response,
    },
}

type RouterResult<T> = Result<T, Box<Response>>;

struct FallbackAttempt<'a> {
    headers: &'a HeaderMap,
    fallback: ResolvedRoute,
    payload: Value,
    original_model: &'a str,
    bridge: Option<&'a TrustedBridgeContext>,
    reason: &'a str,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    match Args::parse().command {
        Command::Serve(args) => serve(args).await,
        Command::PrintConfig(args) => {
            let source = ConfigSource {
                routes_json: args.routes_json,
                routes_file: args.routes_file,
            };
            let routes = load_routes_from_source(&source)?;
            println!("{}", serde_json::to_string_pretty(&route_views(&routes))?);
            Ok(())
        }
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    let source = ConfigSource {
        routes_json: args.routes_json.clone(),
        routes_file: args.routes_file.clone(),
    };
    let routes = load_routes_from_source(&source)?;
    let route_count = routes.len();
    let state = Arc::new(AppState {
        routes: Arc::new(RwLock::new(routes)),
        health: Arc::new(RwLock::new(BTreeMap::new())),
        bridge_nonces: Arc::new(RwLock::new(BTreeMap::new())),
        client: reqwest::Client::new(),
        config_source: source,
        security: SecurityConfig::from_args(&args),
        audit: AuditSink {
            path: args.audit_log_path,
            lock: Arc::new(Mutex::new(())),
        },
    });
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/router/health", get(router_health))
        .route("/v1/router/routes", get(router_routes))
        .route("/v1/router/reload", post(router_reload))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(bind = %args.bind, route_count, "global router listening");
    axum::serve(listener, app).await?;
    Ok(())
}

impl SecurityConfig {
    fn from_args(args: &ServeArgs) -> Self {
        let mut inbound_api_keys = parse_csv(args.inbound_api_keys.as_deref());
        if inbound_api_keys.is_empty() {
            inbound_api_keys = parse_csv(env::var("GLOBAL_ROUTER_INBOUND_API_KEY").ok().as_deref());
        }
        let bridge_secret = clean_option(args.bridge_secret.clone())
            .or_else(|| clean_option(env::var("AGENT_BRIDGE_SECRET").ok()));
        let bridge_issuer = clean_option(args.bridge_issuer.clone())
            .or_else(|| clean_option(env::var("AGENT_BRIDGE_ISSUER").ok()))
            .unwrap_or_else(|| "open-webui".to_string());
        let bridge_max_clock_skew_seconds = args
            .bridge_max_clock_skew_seconds
            .or_else(|| {
                env::var("AGENT_BRIDGE_MAX_CLOCK_SKEW_SECONDS")
                    .ok()
                    .and_then(|value| value.parse::<i64>().ok())
            })
            .unwrap_or(300);
        Self {
            inbound_api_keys,
            admin_api_key: clean_option(args.admin_api_key.clone()),
            bridge: BridgeSecurityConfig {
                secret: bridge_secret,
                issuer: bridge_issuer,
                max_clock_skew_seconds: bridge_max_clock_skew_seconds,
            },
        }
    }
}

fn load_routes_from_source(source: &ConfigSource) -> Result<BTreeMap<String, Route>> {
    let routes_json = match &source.routes_file {
        Some(path) => Some(
            fs::read_to_string(path)
                .with_context(|| format!("read global-router routes file {}", path.display()))?,
        ),
        None => source.routes_json.clone(),
    };
    load_routes(routes_json.as_deref())
}

fn load_routes(routes_json: Option<&str>) -> Result<BTreeMap<String, Route>> {
    let configs = match routes_json.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => serde_json::from_str::<Vec<RouteConfig>>(value)
            .context("parse GLOBAL_ROUTER_ROUTES_JSON")?,
        None => vec![RouteConfig {
            model: "tonglingyu".to_string(),
            name: Some("通灵玉".to_string()),
            base_url: "http://tonglingyu-gateway:8090/v1".to_string(),
            upstream_model: Some("tonglingyu".to_string()),
            requires_bridge: false,
            api_key_env: None,
            timeout_seconds: Some(120),
            discover_models: false,
            allowed_user_roles: Vec::new(),
            allowed_subjects: Vec::new(),
            failure_threshold: Some(3),
            circuit_breaker_seconds: Some(30),
            fallback_model: None,
        }],
    };

    if configs.is_empty() {
        return Err(anyhow!("global-router route allowlist is empty"));
    }

    let mut routes = BTreeMap::new();
    for config in configs {
        let model = config.model.trim().trim_matches('/').to_string();
        if model.is_empty() {
            return Err(anyhow!("route model must not be empty"));
        }
        if routes.contains_key(&model) {
            return Err(anyhow!("duplicate visible model id: {model}"));
        }
        let base_url = config.base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(anyhow!("route {model} base_url must not be empty"));
        }
        let upstream_model = clean_option(config.upstream_model);
        let api_key = match config
            .api_key_env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(name) => {
                let value = env::var(name)
                    .with_context(|| format!("route {model} api_key_env {name} is not set"))?;
                let value = value.trim().to_string();
                if value.is_empty() {
                    return Err(anyhow!("route {model} api_key_env {name} is empty"));
                }
                Some(value)
            }
            None => None,
        };
        routes.insert(
            model.clone(),
            Route {
                model,
                name: config.name,
                base_url,
                upstream_model,
                requires_bridge: config.requires_bridge,
                api_key,
                timeout: Duration::from_secs(config.timeout_seconds.unwrap_or(120)),
                discover_models: config.discover_models,
                allowed_user_roles: normalize_list(config.allowed_user_roles),
                allowed_subjects: normalize_list(config.allowed_subjects),
                failure_threshold: config.failure_threshold.unwrap_or(3).max(1),
                circuit_breaker: Duration::from_secs(config.circuit_breaker_seconds.unwrap_or(30)),
                fallback_model: clean_option(config.fallback_model),
            },
        );
    }
    Ok(routes)
}

async fn healthz(State(state): State<Arc<AppState>>) -> Json<Value> {
    let routes = state.routes.read().await;
    let health = state.health.read().await;
    let unhealthy_routes = routes
        .values()
        .filter(|route| route_is_circuit_open(route, health.get(&route.model)))
        .count();
    Json(json!({
        "status": "ok",
        "route_count": routes.len(),
        "unhealthy_routes": unhealthy_routes,
    }))
}

async fn models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let trace_id = new_trace_id();
    if let Err(response) = authorize_inbound(&state.security, &headers, &trace_id) {
        state
            .audit(AuditEvent::new(
                &trace_id,
                "models",
                "denied",
                StatusCode::UNAUTHORIZED,
            ))
            .await;
        return *response;
    }

    let routes = state.routes.read().await.clone();
    let mut data = Vec::new();
    let mut seen = BTreeSet::new();
    for route in routes.values() {
        if route.discover_models {
            match discover_upstream_models(&state.client, route, &headers).await {
                Ok(models) => {
                    for item in models {
                        push_model_once(&mut data, &mut seen, item);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        route = %route.model,
                        error = %error,
                        "upstream model discovery failed"
                    );
                }
            }
        } else {
            push_model_once(
                &mut data,
                &mut seen,
                route_model_json(route, &route.model, route.name.as_deref()),
            );
        }
    }
    Json(json!({
        "object": "list",
        "data": data,
    }))
    .into_response()
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Response {
    let trace_id = new_trace_id();
    if let Err(response) = authorize_inbound(&state.security, &headers, &trace_id) {
        state
            .audit(AuditEvent::new(
                &trace_id,
                "chat.completions",
                "denied",
                StatusCode::UNAUTHORIZED,
            ))
            .await;
        return *response;
    }

    let Some(model) = payload.get("model").and_then(Value::as_str).map(str::trim) else {
        let response = error_response(
            StatusCode::BAD_REQUEST,
            "missing_model",
            "request.model is required",
            &trace_id,
        );
        state
            .audit(AuditEvent::new(
                &trace_id,
                "chat.completions",
                "denied",
                StatusCode::BAD_REQUEST,
            ))
            .await;
        return response;
    };
    let model = model.to_string();
    let Some(mut resolved) = state.resolve_model(&model).await else {
        let response = error_response(
            StatusCode::NOT_FOUND,
            "model_not_allowed",
            "model is not in global-router allowlist",
            &trace_id,
        );
        state
            .audit(
                AuditEvent::new(
                    &trace_id,
                    "chat.completions",
                    "denied",
                    StatusCode::NOT_FOUND,
                )
                .with_model(&model),
            )
            .await;
        return response;
    };

    let bridge = match prepare_bridge_context(&state, &mut payload, &resolved, &trace_id).await {
        Ok(bridge) => bridge,
        Err(response) => {
            state
                .audit(
                    AuditEvent::new(&trace_id, "chat.completions", "denied", response.status())
                        .with_model(&model)
                        .with_route(&resolved),
                )
                .await;
            return *response;
        }
    };
    if let Err(response) = authorize_route(&resolved.route, bridge.as_ref(), &trace_id) {
        state
            .audit(
                AuditEvent::new(&trace_id, "chat.completions", "denied", response.status())
                    .with_model(&model)
                    .with_route(&resolved)
                    .with_bridge(bridge.as_ref()),
            )
            .await;
        return *response;
    }

    if route_is_open_by_state(&state, &resolved.route).await {
        match select_fallback(&state, &resolved, bridge.as_ref()).await {
            Some(fallback) => {
                tracing::warn!(
                    %trace_id,
                    model = %resolved.visible_model,
                    fallback_model = %fallback.visible_model,
                    "route circuit is open; using fallback"
                );
                state
                    .audit(
                        AuditEvent::new(
                            &trace_id,
                            "chat.completions.fallback",
                            "allowed",
                            StatusCode::OK,
                        )
                        .with_model(&model)
                        .with_route(&fallback)
                        .with_bridge(bridge.as_ref())
                        .with_reason("primary_circuit_open"),
                    )
                    .await;
                resolved = fallback;
            }
            None => {
                let response = error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "route_circuit_open",
                    "route is temporarily unavailable",
                    &trace_id,
                );
                state
                    .audit(
                        AuditEvent::new(
                            &trace_id,
                            "chat.completions",
                            "denied",
                            StatusCode::SERVICE_UNAVAILABLE,
                        )
                        .with_model(&model)
                        .with_route(&resolved)
                        .with_bridge(bridge.as_ref())
                        .with_reason("route_circuit_open"),
                    )
                    .await;
                return response;
            }
        }
    }

    payload["model"] = json!(resolved.upstream_model);
    match forward_chat(
        &state.client,
        &resolved,
        payload.clone(),
        &headers,
        &trace_id,
    )
    .await
    {
        Ok(ForwardOutcome::Success(response)) => {
            state.record_success(&resolved.route, StatusCode::OK).await;
            state
                .audit(
                    AuditEvent::new(
                        &trace_id,
                        "chat.completions",
                        "completed",
                        response.status(),
                    )
                    .with_model(&model)
                    .with_route(&resolved)
                    .with_bridge(bridge.as_ref()),
                )
                .await;
            response
        }
        Ok(ForwardOutcome::UpstreamError {
            status,
            message,
            response,
        }) => {
            if status.is_server_error() {
                state
                    .record_failure(&resolved.route, status, &message)
                    .await;
                if let Some(fallback) = select_fallback(&state, &resolved, bridge.as_ref()).await {
                    let mut fallback_payload = payload;
                    fallback_payload["model"] = json!(fallback.upstream_model);
                    return forward_fallback(
                        &state,
                        &trace_id,
                        FallbackAttempt {
                            headers: &headers,
                            fallback,
                            payload: fallback_payload,
                            original_model: &model,
                            bridge: bridge.as_ref(),
                            reason: "upstream_server_error",
                        },
                    )
                    .await;
                }
            }
            state
                .audit(
                    AuditEvent::new(&trace_id, "chat.completions", "upstream_error", status)
                        .with_model(&model)
                        .with_route(&resolved)
                        .with_bridge(bridge.as_ref())
                        .with_upstream_status(status)
                        .with_reason(&message),
                )
                .await;
            response
        }
        Err(error) => {
            let message = sanitize_error_message(&error.to_string());
            tracing::warn!(%trace_id, model = %resolved.route.model, error = %message, "route forward failed");
            state
                .record_failure(&resolved.route, StatusCode::BAD_GATEWAY, &message)
                .await;
            if let Some(fallback) = select_fallback(&state, &resolved, bridge.as_ref()).await {
                let mut fallback_payload = payload;
                fallback_payload["model"] = json!(fallback.upstream_model);
                return forward_fallback(
                    &state,
                    &trace_id,
                    FallbackAttempt {
                        headers: &headers,
                        fallback,
                        payload: fallback_payload,
                        original_model: &model,
                        bridge: bridge.as_ref(),
                        reason: "route_forward_failed",
                    },
                )
                .await;
            }
            let response = error_response(
                StatusCode::BAD_GATEWAY,
                "route_forward_failed",
                &message,
                &trace_id,
            );
            state
                .audit(
                    AuditEvent::new(
                        &trace_id,
                        "chat.completions",
                        "failed",
                        StatusCode::BAD_GATEWAY,
                    )
                    .with_model(&model)
                    .with_route(&resolved)
                    .with_bridge(bridge.as_ref())
                    .with_reason(&message),
                )
                .await;
            response
        }
    }
}

async fn forward_fallback(
    state: &AppState,
    trace_id: &str,
    attempt: FallbackAttempt<'_>,
) -> Response {
    match forward_chat(
        &state.client,
        &attempt.fallback,
        attempt.payload,
        attempt.headers,
        trace_id,
    )
    .await
    {
        Ok(ForwardOutcome::Success(response)) => {
            state
                .record_success(&attempt.fallback.route, StatusCode::OK)
                .await;
            state
                .audit(
                    AuditEvent::new(
                        trace_id,
                        "chat.completions.fallback",
                        "completed",
                        response.status(),
                    )
                    .with_model(attempt.original_model)
                    .with_route(&attempt.fallback)
                    .with_bridge(attempt.bridge)
                    .with_reason(attempt.reason),
                )
                .await;
            response
        }
        Ok(ForwardOutcome::UpstreamError {
            status,
            message,
            response,
        }) => {
            if status.is_server_error() {
                state
                    .record_failure(&attempt.fallback.route, status, &message)
                    .await;
            }
            state
                .audit(
                    AuditEvent::new(
                        trace_id,
                        "chat.completions.fallback",
                        "upstream_error",
                        status,
                    )
                    .with_model(attempt.original_model)
                    .with_route(&attempt.fallback)
                    .with_bridge(attempt.bridge)
                    .with_upstream_status(status)
                    .with_reason(&message),
                )
                .await;
            response
        }
        Err(error) => {
            let message = sanitize_error_message(&error.to_string());
            state
                .record_failure(&attempt.fallback.route, StatusCode::BAD_GATEWAY, &message)
                .await;
            let response = error_response(
                StatusCode::BAD_GATEWAY,
                "fallback_route_forward_failed",
                &message,
                trace_id,
            );
            state
                .audit(
                    AuditEvent::new(
                        trace_id,
                        "chat.completions.fallback",
                        "failed",
                        StatusCode::BAD_GATEWAY,
                    )
                    .with_model(attempt.original_model)
                    .with_route(&attempt.fallback)
                    .with_bridge(attempt.bridge)
                    .with_reason(&message),
                )
                .await;
            response
        }
    }
}

async fn router_health(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let trace_id = new_trace_id();
    if let Err(response) = authorize_admin(&state.security, &headers, &trace_id) {
        return *response;
    }
    let routes = state.routes.read().await;
    let health = state.health.read().await;
    let data = routes
        .values()
        .map(|route| {
            let item = health.get(&route.model).cloned().unwrap_or_default();
            json!({
                "model": route.model,
                "status": if route_is_circuit_open(route, Some(&item)) { "circuit_open" } else { "ok" },
                "health": item,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "object": "list",
        "data": data,
    }))
    .into_response()
}

async fn router_routes(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let trace_id = new_trace_id();
    if let Err(response) = authorize_admin(&state.security, &headers, &trace_id) {
        return *response;
    }
    let routes = state.routes.read().await;
    Json(json!({
        "object": "list",
        "data": route_views(&routes),
    }))
    .into_response()
}

async fn router_reload(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let trace_id = new_trace_id();
    if let Err(response) = authorize_admin(&state.security, &headers, &trace_id) {
        return *response;
    }
    match load_routes_from_source(&state.config_source) {
        Ok(routes) => {
            let route_count = routes.len();
            *state.routes.write().await = routes;
            state.health.write().await.clear();
            state
                .audit(AuditEvent::new(
                    &trace_id,
                    "router.reload",
                    "completed",
                    StatusCode::OK,
                ))
                .await;
            Json(json!({
                "status": "ok",
                "route_count": route_count,
                "trace_id": trace_id,
            }))
            .into_response()
        }
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "route_reload_failed",
            &sanitize_error_message(&error.to_string()),
            &trace_id,
        ),
    }
}

impl AppState {
    async fn resolve_model(&self, model: &str) -> Option<ResolvedRoute> {
        let routes = self.routes.read().await;
        resolve_route(&routes, model)
    }

    async fn record_success(&self, route: &Route, status: StatusCode) {
        let mut health = self.health.write().await;
        let item = health.entry(route.model.clone()).or_default();
        item.consecutive_failures = 0;
        item.circuit_open_until_ms = None;
        item.last_success_ms = Some(now_ms());
        item.last_status = Some(status.as_u16());
        item.last_error = None;
    }

    async fn record_failure(&self, route: &Route, status: StatusCode, reason: &str) {
        let mut health = self.health.write().await;
        let item = health.entry(route.model.clone()).or_default();
        item.consecutive_failures = item.consecutive_failures.saturating_add(1);
        item.last_failure_ms = Some(now_ms());
        item.last_status = Some(status.as_u16());
        item.last_error = Some(sanitize_error_message(reason));
        if item.consecutive_failures >= route.failure_threshold {
            item.circuit_open_until_ms = Some(now_ms() + route.circuit_breaker.as_millis().max(1));
        }
    }

    async fn claim_bridge_nonce(
        &self,
        context: &AgentBridgeContext,
        trace_id: &str,
    ) -> RouterResult<()> {
        let now = now_ms();
        let max_age_ms = self.security.bridge.max_clock_skew_seconds.max(1) as u128 * 1000;
        let expires_at = now + max_age_ms;
        let key = format!(
            "{}:{}:{}:{}",
            context.issuer, context.subject, context.chat_id, context.nonce
        );
        let mut nonces = self.bridge_nonces.write().await;
        nonces.retain(|_, expires_at| *expires_at > now);
        if nonces.contains_key(&key) {
            return Err(Box::new(error_response(
                StatusCode::FORBIDDEN,
                "agent_bridge_context_replayed",
                "agent_bridge_context nonce has already been used",
                trace_id,
            )));
        }
        nonces.insert(key, expires_at);
        Ok(())
    }

    async fn audit(&self, event: AuditEvent) {
        if let Err(error) = self.audit.write(event).await {
            tracing::warn!(error = %error, "failed to write global-router audit event");
        }
    }
}

impl AuditSink {
    async fn write(&self, event: AuditEvent) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _guard = self.lock.lock().await;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .with_context(|| format!("open audit log {}", path.display()))?;
        file.write_all(serde_json::to_string(&event)?.as_bytes())
            .await?;
        file.write_all(b"\n").await?;
        Ok(())
    }
}

impl AuditEvent {
    fn new(trace_id: &str, action: &str, decision: &str, status: StatusCode) -> Self {
        Self {
            timestamp_ms: now_ms(),
            trace_id: trace_id.to_string(),
            action: action.to_string(),
            decision: decision.to_string(),
            model: None,
            route_model: None,
            upstream_model: None,
            subject: None,
            user_role: None,
            status: status.as_u16(),
            reason: None,
            upstream_status: None,
        }
    }

    fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    fn with_route(mut self, route: &ResolvedRoute) -> Self {
        self.route_model = Some(route.route.model.clone());
        self.upstream_model = Some(route.upstream_model.clone());
        self
    }

    fn with_bridge(mut self, bridge: Option<&TrustedBridgeContext>) -> Self {
        if let Some(bridge) = bridge {
            self.subject = Some(bridge.subject.clone());
            self.user_role = Some(bridge.user_role.clone());
        }
        self
    }

    fn with_reason(mut self, reason: &str) -> Self {
        self.reason = Some(sanitize_error_message(reason));
        self
    }

    fn with_upstream_status(mut self, status: StatusCode) -> Self {
        self.upstream_status = Some(status.as_u16());
        self
    }
}

fn resolve_route(routes: &BTreeMap<String, Route>, model: &str) -> Option<ResolvedRoute> {
    if let Some(route) = routes.get(model) {
        if route.discover_models && route.upstream_model.is_none() {
            return None;
        }
        return Some(ResolvedRoute {
            route: route.clone(),
            visible_model: model.to_string(),
            upstream_model: route
                .upstream_model
                .clone()
                .unwrap_or_else(|| route.model.clone()),
        });
    }
    routes.values().find_map(|route| {
        if !route.discover_models {
            return None;
        }
        let prefix = format!("{}/", route.model);
        let upstream_model = model.strip_prefix(&prefix)?.trim();
        if upstream_model.is_empty() {
            return None;
        }
        Some(ResolvedRoute {
            route: route.clone(),
            visible_model: model.to_string(),
            upstream_model: upstream_model.to_string(),
        })
    })
}

async fn select_fallback(
    state: &AppState,
    resolved: &ResolvedRoute,
    bridge: Option<&TrustedBridgeContext>,
) -> Option<ResolvedRoute> {
    let fallback_model = resolved.route.fallback_model.as_deref()?;
    if fallback_model == resolved.visible_model {
        return None;
    }
    let fallback = state.resolve_model(fallback_model).await?;
    if fallback.route.requires_bridge {
        return None;
    }
    if !route_policy_allows(&fallback.route, bridge) {
        return None;
    }
    if route_is_open_by_state(state, &fallback.route).await {
        return None;
    }
    Some(fallback)
}

async fn route_is_open_by_state(state: &AppState, route: &Route) -> bool {
    let health = state.health.read().await;
    route_is_circuit_open(route, health.get(&route.model))
}

fn route_is_circuit_open(route: &Route, health: Option<&RouteHealth>) -> bool {
    let Some(until) = health.and_then(|health| health.circuit_open_until_ms) else {
        return false;
    };
    let open = now_ms() < until;
    open && route.circuit_breaker.as_millis() > 0
}

async fn prepare_bridge_context(
    state: &AppState,
    payload: &mut Value,
    resolved: &ResolvedRoute,
    trace_id: &str,
) -> RouterResult<Option<TrustedBridgeContext>> {
    if !resolved.route.requires_bridge {
        remove_agent_bridge_context(payload);
        return Ok(None);
    }
    let Some(value) = payload.get("agent_bridge_context") else {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "agent_bridge_context_required",
            "this model requires agent_bridge_context",
            trace_id,
        )));
    };
    let context = serde_json::from_value::<AgentBridgeContext>(value.clone()).map_err(|_| {
        Box::new(error_response(
            StatusCode::FORBIDDEN,
            "invalid_agent_bridge_context",
            "agent_bridge_context is invalid",
            trace_id,
        ))
    })?;
    let trusted = verify_bridge_context(
        &state.security.bridge,
        &context,
        &resolved.visible_model,
        trace_id,
    )
    .map_err(|code| {
        Box::new(error_response(
            StatusCode::FORBIDDEN,
            code,
            "agent_bridge_context is not trusted",
            trace_id,
        ))
    })?;
    state.claim_bridge_nonce(&context, trace_id).await?;
    remove_agent_bridge_context(payload);
    Ok(Some(trusted))
}

fn verify_bridge_context(
    config: &BridgeSecurityConfig,
    context: &AgentBridgeContext,
    visible_model: &str,
    _trace_id: &str,
) -> Result<TrustedBridgeContext, &'static str> {
    let Some(secret) = &config.secret else {
        return Err("bridge_secret_not_configured");
    };
    if context.version != 1
        || context.issuer != config.issuer
        || !context.subject.starts_with("openwebui:")
        || context.chat_id.trim().is_empty()
        || context.model != visible_model
        || context.nonce.trim().is_empty()
        || context.signature.trim().is_empty()
    {
        return Err("invalid_agent_bridge_context");
    }
    let now = unix_timestamp();
    if (now - context.issued_at).abs() > config.max_clock_skew_seconds {
        return Err("expired_agent_bridge_context");
    }
    let expected = bridge_signature(secret, context).map_err(|_| "invalid_agent_bridge_context")?;
    if !constant_time_eq(expected.as_bytes(), context.signature.as_bytes()) {
        return Err("invalid_agent_bridge_signature");
    }
    Ok(TrustedBridgeContext {
        subject: context.subject.clone(),
        user_role: context
            .user_role
            .clone()
            .unwrap_or_else(|| "user".to_string()),
    })
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

fn authorize_route(
    route: &Route,
    bridge: Option<&TrustedBridgeContext>,
    trace_id: &str,
) -> RouterResult<()> {
    if route_policy_allows(route, bridge) {
        return Ok(());
    }
    let Some(bridge) = bridge else {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "route_identity_required",
            "this route requires a trusted user identity",
            trace_id,
        )));
    };
    if !route_subject_allows(route, &bridge.subject) {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "route_subject_forbidden",
            "subject is not allowed to use this route",
            trace_id,
        )));
    }
    if !route_role_allows(route, &bridge.user_role) {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "route_role_forbidden",
            "role is not allowed to use this route",
            trace_id,
        )));
    }
    Ok(())
}

fn route_policy_allows(route: &Route, bridge: Option<&TrustedBridgeContext>) -> bool {
    if route.allowed_user_roles.is_empty() && route.allowed_subjects.is_empty() {
        return true;
    }
    let Some(bridge) = bridge else {
        return false;
    };
    route_subject_allows(route, &bridge.subject) && route_role_allows(route, &bridge.user_role)
}

fn route_subject_allows(route: &Route, subject: &str) -> bool {
    route.allowed_subjects.is_empty()
        || route
            .allowed_subjects
            .iter()
            .any(|allowed| allowed == subject)
}

fn route_role_allows(route: &Route, user_role: &str) -> bool {
    route.allowed_user_roles.is_empty()
        || route
            .allowed_user_roles
            .iter()
            .any(|allowed| allowed == user_role)
}

fn authorize_inbound(
    security: &SecurityConfig,
    headers: &HeaderMap,
    trace_id: &str,
) -> RouterResult<()> {
    if security.inbound_api_keys.is_empty() {
        return Ok(());
    }
    let Some(token) = bearer_token(headers) else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "router_auth_required",
            "global-router inbound authorization is required",
            trace_id,
        )));
    };
    if security
        .inbound_api_keys
        .iter()
        .any(|expected| constant_time_eq(expected.as_bytes(), token.as_bytes()))
    {
        return Ok(());
    }
    Err(Box::new(error_response(
        StatusCode::UNAUTHORIZED,
        "router_auth_invalid",
        "global-router inbound authorization is invalid",
        trace_id,
    )))
}

fn authorize_admin(
    security: &SecurityConfig,
    headers: &HeaderMap,
    trace_id: &str,
) -> RouterResult<()> {
    let Some(expected) = &security.admin_api_key else {
        return Err(Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "router_admin_auth_not_configured",
            "global-router admin API key is not configured",
            trace_id,
        )));
    };
    let Some(token) = bearer_token(headers) else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "router_admin_auth_required",
            "global-router admin authorization is required",
            trace_id,
        )));
    };
    if constant_time_eq(expected.as_bytes(), token.as_bytes()) {
        return Ok(());
    }
    Err(Box::new(error_response(
        StatusCode::UNAUTHORIZED,
        "router_admin_auth_invalid",
        "global-router admin authorization is invalid",
        trace_id,
    )))
}

async fn discover_upstream_models(
    client: &reqwest::Client,
    route: &Route,
    inbound_headers: &HeaderMap,
) -> Result<Vec<Value>> {
    let mut request = client
        .get(format!("{}/models", route.base_url))
        .timeout(route.timeout);
    request = apply_upstream_auth(request, route, inbound_headers);
    let upstream = request.send().await?;
    let status = upstream.status();
    if !status.is_success() {
        return Err(anyhow!("upstream /models returned HTTP {status}"));
    }
    let payload = upstream.json::<Value>().await?;
    let data = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("upstream /models response missing data array"))?;
    let mut models = Vec::new();
    for item in data {
        let Some(upstream_id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        if upstream_id.trim().is_empty() {
            continue;
        }
        let visible_id = format!("{}/{}", route.model, upstream_id.trim());
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(upstream_id)
            .to_string();
        models.push(route_model_json(route, &visible_id, Some(&name)));
    }
    Ok(models)
}

fn route_model_json(route: &Route, visible_id: &str, name: Option<&str>) -> Value {
    json!({
        "id": visible_id,
        "object": "model",
        "owned_by": "global-router",
        "name": name.unwrap_or(visible_id),
        "requires_bridge": route.requires_bridge,
    })
}

fn push_model_once(data: &mut Vec<Value>, seen: &mut BTreeSet<String>, item: Value) {
    let Some(id) = item.get("id").and_then(Value::as_str) else {
        return;
    };
    if seen.insert(id.to_string()) {
        data.push(item);
    }
}

async fn forward_chat(
    client: &reqwest::Client,
    resolved: &ResolvedRoute,
    payload: Value,
    inbound_headers: &HeaderMap,
    trace_id: &str,
) -> Result<ForwardOutcome> {
    let mut request = client
        .post(format!("{}/chat/completions", resolved.route.base_url))
        .timeout(resolved.route.timeout)
        .header("x-global-router-trace-id", trace_id)
        .json(&payload);
    request = apply_upstream_auth(request, &resolved.route, inbound_headers);

    let upstream = request.send().await?;
    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| header::HeaderValue::from_static("application/json"));

    if !status.is_success() {
        let message = upstream_error_message(status, upstream).await;
        let response = error_response(status, "upstream_error", &message, trace_id);
        return Ok(ForwardOutcome::UpstreamError {
            status,
            message,
            response,
        });
    }

    let stream = upstream
        .bytes_stream()
        .map(|chunk| chunk.map_err(|error| std::io::Error::other(error.to_string())));
    let mut response = Body::from_stream(stream).into_response();
    *response.status_mut() = status;
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        "x-global-router-trace-id",
        header::HeaderValue::from_str(trace_id)?,
    );
    Ok(ForwardOutcome::Success(response))
}

fn apply_upstream_auth(
    mut request: reqwest::RequestBuilder,
    route: &Route,
    inbound_headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    if let Some(api_key) = &route.api_key {
        request = request.header(AUTHORIZATION, format!("Bearer {api_key}"));
    } else if let Some(value) = inbound_headers.get(header::AUTHORIZATION) {
        request = request.header(AUTHORIZATION, value.clone());
    }
    request
}

async fn upstream_error_message(status: StatusCode, response: reqwest::Response) -> String {
    let text = response.text().await.unwrap_or_default();
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
        {
            return sanitize_error_message(message);
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return sanitize_error_message(message);
        }
    }
    if text.trim().is_empty() {
        format!("upstream returned HTTP {status}")
    } else {
        sanitize_error_message(&text)
    }
}

fn remove_agent_bridge_context(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("agent_bridge_context");
    }
}

fn error_response(status: StatusCode, code: &str, message: &str, trace_id: &str) -> Response {
    let mut response = (
        status,
        Json(json!({
            "error": {
                "type": code,
                "message": message,
            },
            "trace_id": trace_id,
        })),
    )
        .into_response();
    if let Ok(value) = header::HeaderValue::from_str(trace_id) {
        response
            .headers_mut()
            .insert("x-global-router-trace-id", value);
    }
    response
}

fn route_views(routes: &BTreeMap<String, Route>) -> Vec<RouteView> {
    routes
        .values()
        .map(|route| RouteView {
            model: route.model.clone(),
            name: route.name.clone(),
            upstream_model: route.upstream_model.clone(),
            base_url: route.base_url.clone(),
            requires_bridge: route.requires_bridge,
            has_api_key_env: route.api_key.is_some(),
            timeout_seconds: route.timeout.as_secs(),
            discover_models: route.discover_models,
            allowed_user_roles: route.allowed_user_roles.clone(),
            has_subject_allowlist: !route.allowed_subjects.is_empty(),
            failure_threshold: route.failure_threshold,
            circuit_breaker_seconds: route.circuit_breaker.as_secs(),
            fallback_model: route.fallback_model.clone(),
        })
        .collect()
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_csv(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn clean_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn sanitize_error_message(message: &str) -> String {
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > 300 {
        format!("{}...", compact.chars().take(300).collect::<String>())
    } else if compact.is_empty() {
        "upstream request failed".to_string()
    } else {
        compact
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn new_trace_id() -> String {
    format!("gr-{}", uuid::Uuid::now_v7().simple())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_route_is_tonglingyu() {
        let routes = load_routes(None).unwrap();
        let route = routes.get("tonglingyu").unwrap();
        assert_eq!(route.upstream_model.as_deref(), Some("tonglingyu"));
        assert!(!route.requires_bridge);
        assert_eq!(route.failure_threshold, 3);
    }

    #[test]
    fn rejects_duplicate_visible_model_ids() {
        let json = r#"[
          {"model":"same","base_url":"http://a/v1"},
          {"model":"same","base_url":"http://b/v1"}
        ]"#;
        let error = load_routes(Some(json)).unwrap_err().to_string();
        assert!(error.contains("duplicate visible model id"));
    }

    #[test]
    fn route_can_require_bridge_and_rewrite_model() {
        let json = r#"[
          {
            "model":"other/default",
            "base_url":"http://other-gateway:8090/v1",
            "upstream_model":"default",
            "requires_bridge":true,
            "allowed_user_roles":["admin"],
            "fallback_model":"tonglingyu"
          },
          {
            "model":"tonglingyu",
            "base_url":"http://tonglingyu-gateway:8090/v1"
          }
        ]"#;
        let routes = load_routes(Some(json)).unwrap();
        let route = routes.get("other/default").unwrap();
        assert_eq!(route.upstream_model.as_deref(), Some("default"));
        assert!(route.requires_bridge);
        assert_eq!(route.allowed_user_roles, vec!["admin"]);
        assert_eq!(route.fallback_model.as_deref(), Some("tonglingyu"));
    }

    #[test]
    fn resolves_dynamic_discovered_model_namespace() {
        let json = r#"[
          {
            "model":"other",
            "base_url":"http://other-gateway:8090/v1",
            "discover_models":true
          }
        ]"#;
        let routes = load_routes(Some(json)).unwrap();
        let resolved = resolve_route(&routes, "other/default").unwrap();
        assert_eq!(resolved.route.model, "other");
        assert_eq!(resolved.visible_model, "other/default");
        assert_eq!(resolved.upstream_model, "default");
        assert!(resolve_route(&routes, "other").is_none());
    }

    #[test]
    fn bridge_signature_matches_filter_canonical_payload() {
        let mut context = bridge_context("other/default");
        context.issued_at = 1_700_000_000;
        context.nonce = "nonce-1".to_string();
        context.signature.clear();
        assert_eq!(
            bridge_signature("bridge-secret", &context).unwrap(),
            "12b6f7f74c5f2ac6576db4c9462aa21da4807b592aa17c23adce42c95330d464"
        );
    }

    #[test]
    fn verifies_bridge_context_for_visible_model() {
        let config = BridgeSecurityConfig {
            secret: Some("bridge-secret".to_string()),
            issuer: "open-webui".to_string(),
            max_clock_skew_seconds: 10_000_000_000,
        };
        let mut context = bridge_context("other/default");
        context.signature = bridge_signature("bridge-secret", &context).unwrap();
        let trusted = verify_bridge_context(&config, &context, "other/default", "trace").unwrap();
        assert_eq!(trusted.subject, "openwebui:user-1");
        assert_eq!(trusted.user_role, "admin");
    }

    #[test]
    fn rejects_wrong_bridge_model() {
        let config = BridgeSecurityConfig {
            secret: Some("bridge-secret".to_string()),
            issuer: "open-webui".to_string(),
            max_clock_skew_seconds: 10_000_000_000,
        };
        let mut context = bridge_context("other/default");
        context.signature = bridge_signature("bridge-secret", &context).unwrap();
        assert_eq!(
            verify_bridge_context(&config, &context, "other/private", "trace").unwrap_err(),
            "invalid_agent_bridge_context"
        );
    }

    #[test]
    fn route_rbac_requires_trusted_role() {
        let route = Route {
            model: "private".to_string(),
            name: None,
            base_url: "http://backend/v1".to_string(),
            upstream_model: None,
            requires_bridge: true,
            api_key: None,
            timeout: Duration::from_secs(120),
            discover_models: false,
            allowed_user_roles: vec!["admin".to_string()],
            allowed_subjects: Vec::new(),
            failure_threshold: 3,
            circuit_breaker: Duration::from_secs(30),
            fallback_model: None,
        };
        let viewer = TrustedBridgeContext {
            subject: "openwebui:user-1".to_string(),
            user_role: "viewer".to_string(),
        };
        assert!(authorize_route(&route, Some(&viewer), "trace").is_err());
        let admin = TrustedBridgeContext {
            subject: "openwebui:user-1".to_string(),
            user_role: "admin".to_string(),
        };
        assert!(authorize_route(&route, Some(&admin), "trace").is_ok());
    }

    #[tokio::test]
    async fn bridge_context_is_stripped_and_nonce_cannot_replay() {
        let state = test_state();
        let route = Route {
            model: "other/default".to_string(),
            name: None,
            base_url: "http://other-gateway:8090/v1".to_string(),
            upstream_model: Some("default".to_string()),
            requires_bridge: true,
            api_key: None,
            timeout: Duration::from_secs(120),
            discover_models: false,
            allowed_user_roles: Vec::new(),
            allowed_subjects: Vec::new(),
            failure_threshold: 3,
            circuit_breaker: Duration::from_secs(30),
            fallback_model: None,
        };
        let resolved = ResolvedRoute {
            route,
            visible_model: "other/default".to_string(),
            upstream_model: "default".to_string(),
        };
        let mut context = bridge_context("other/default");
        context.signature = bridge_signature("bridge-secret", &context).unwrap();
        let mut payload = json!({
            "model": "other/default",
            "messages": [],
            "agent_bridge_context": context.clone(),
        });

        let trusted = prepare_bridge_context(&state, &mut payload, &resolved, "trace")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(trusted.subject, "openwebui:user-1");
        assert!(payload.get("agent_bridge_context").is_none());

        let mut replay_payload = json!({
            "model": "other/default",
            "messages": [],
            "agent_bridge_context": context,
        });
        let response = prepare_bridge_context(&state, &mut replay_payload, &resolved, "trace")
            .await
            .unwrap_err();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    fn test_state() -> AppState {
        AppState {
            routes: Arc::new(RwLock::new(BTreeMap::new())),
            health: Arc::new(RwLock::new(BTreeMap::new())),
            bridge_nonces: Arc::new(RwLock::new(BTreeMap::new())),
            client: reqwest::Client::new(),
            config_source: ConfigSource {
                routes_json: None,
                routes_file: None,
            },
            security: SecurityConfig {
                inbound_api_keys: Vec::new(),
                admin_api_key: None,
                bridge: BridgeSecurityConfig {
                    secret: Some("bridge-secret".to_string()),
                    issuer: "open-webui".to_string(),
                    max_clock_skew_seconds: 10_000_000_000,
                },
            },
            audit: AuditSink {
                path: None,
                lock: Arc::new(Mutex::new(())),
            },
        }
    }

    fn bridge_context(model: &str) -> AgentBridgeContext {
        AgentBridgeContext {
            version: 1,
            issuer: "open-webui".to_string(),
            subject: "openwebui:user-1".to_string(),
            user_role: Some("admin".to_string()),
            chat_id: "chat-1".to_string(),
            session_id: Some("session-1".to_string()),
            message_id: Some("message-1".to_string()),
            model: model.to_string(),
            issued_at: unix_timestamp(),
            nonce: "nonce".to_string(),
            signature: String::new(),
        }
    }
}
