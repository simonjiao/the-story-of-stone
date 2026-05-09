use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, net::SocketAddr, sync::Arc, time::Duration};
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
}

#[derive(Debug, Parser, Clone)]
struct PrintConfigArgs {
    #[arg(long, env = "GLOBAL_ROUTER_ROUTES_JSON")]
    routes_json: Option<String>,
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
}

#[derive(Debug, Clone, Serialize)]
struct RouteView {
    model: String,
    name: Option<String>,
    upstream_model: String,
    base_url: String,
    requires_bridge: bool,
    has_api_key_env: bool,
    timeout_seconds: u64,
}

#[derive(Debug, Clone)]
struct Route {
    model: String,
    name: Option<String>,
    base_url: String,
    upstream_model: String,
    requires_bridge: bool,
    api_key: Option<String>,
    timeout: Duration,
}

#[derive(Clone)]
struct AppState {
    routes: Arc<BTreeMap<String, Route>>,
    client: reqwest::Client,
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
            let routes = load_routes(args.routes_json.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&route_views(&routes))?);
            Ok(())
        }
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    let routes = load_routes(args.routes_json.as_deref())?;
    let route_count = routes.len();
    let state = Arc::new(AppState {
        routes: Arc::new(routes),
        client: reqwest::Client::new(),
    });
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(bind = %args.bind, route_count, "global router listening");
    axum::serve(listener, app).await?;
    Ok(())
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
        }],
    };

    if configs.is_empty() {
        return Err(anyhow!("global-router route allowlist is empty"));
    }

    let mut routes = BTreeMap::new();
    for config in configs {
        let model = config.model.trim();
        if model.is_empty() {
            return Err(anyhow!("route model must not be empty"));
        }
        if routes.contains_key(model) {
            return Err(anyhow!("duplicate visible model id: {model}"));
        }
        let base_url = config.base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(anyhow!("route {model} base_url must not be empty"));
        }
        let upstream_model = config
            .upstream_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(model)
            .to_string();
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
            model.to_string(),
            Route {
                model: model.to_string(),
                name: config.name,
                base_url,
                upstream_model,
                requires_bridge: config.requires_bridge,
                api_key,
                timeout: Duration::from_secs(config.timeout_seconds.unwrap_or(120)),
            },
        );
    }
    Ok(routes)
}

async fn healthz(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "routes": route_views(&state.routes),
    }))
}

async fn models(State(state): State<Arc<AppState>>) -> Json<Value> {
    let data = state
        .routes
        .values()
        .map(|route| {
            json!({
                "id": route.model,
                "object": "model",
                "owned_by": "global-router",
                "name": route.name.as_deref().unwrap_or(&route.model),
                "requires_bridge": route.requires_bridge,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "object": "list",
        "data": data,
    }))
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Response {
    let trace_id = format!("gr-{}", uuid::Uuid::now_v7().simple());
    let Some(model) = payload.get("model").and_then(Value::as_str) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing_model",
            "request.model is required",
            &trace_id,
        );
    };
    let Some(route) = state.routes.get(model) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "model_not_allowed",
            "model is not in global-router allowlist",
            &trace_id,
        );
    };

    if route.requires_bridge && payload.get("agent_bridge_context").is_none() {
        return error_response(
            StatusCode::FORBIDDEN,
            "agent_bridge_context_required",
            "this model requires agent_bridge_context",
            &trace_id,
        );
    }
    if !route.requires_bridge {
        remove_agent_bridge_context(&mut payload);
    }
    payload["model"] = json!(route.upstream_model);

    match forward_chat(&state.client, route, payload, &headers, &trace_id).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%trace_id, model = %route.model, error = %error, "route forward failed");
            error_response(
                StatusCode::BAD_GATEWAY,
                "route_forward_failed",
                &error.to_string(),
                &trace_id,
            )
        }
    }
}

async fn forward_chat(
    client: &reqwest::Client,
    route: &Route,
    payload: Value,
    inbound_headers: &HeaderMap,
    trace_id: &str,
) -> Result<Response> {
    let mut request = client
        .post(format!("{}/chat/completions", route.base_url))
        .timeout(route.timeout)
        .header("x-global-router-trace-id", trace_id)
        .json(&payload);
    if let Some(api_key) = &route.api_key {
        request = request.header(AUTHORIZATION, format!("Bearer {api_key}"));
    } else if let Some(value) = inbound_headers.get(header::AUTHORIZATION) {
        request = request.header(AUTHORIZATION, value.clone());
    }
    let upstream = request.send().await?;
    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| header::HeaderValue::from_static("application/json"));

    let stream = upstream.bytes_stream().map(|chunk| {
        chunk
            .map(Bytes::from)
            .map_err(|error| std::io::Error::other(error.to_string()))
    });
    let mut response = Body::from_stream(stream).into_response();
    *response.status_mut() = status;
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        "x-global-router-trace-id",
        header::HeaderValue::from_str(trace_id)?,
    );
    Ok(response)
}

fn remove_agent_bridge_context(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("agent_bridge_context");
    }
}

fn error_response(status: StatusCode, code: &str, message: &str, trace_id: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "type": code,
                "message": message,
            },
            "trace_id": trace_id,
        })),
    )
        .into_response()
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
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_route_is_tonglingyu() {
        let routes = load_routes(None).unwrap();
        let route = routes.get("tonglingyu").unwrap();
        assert_eq!(route.upstream_model, "tonglingyu");
        assert!(!route.requires_bridge);
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
            "requires_bridge":true
          }
        ]"#;
        let routes = load_routes(Some(json)).unwrap();
        let route = routes.get("other/default").unwrap();
        assert_eq!(route.upstream_model, "default");
        assert!(route.requires_bridge);
    }
}
