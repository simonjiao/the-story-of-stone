use agent_core::{RuntimeRunInput, RuntimeSessionInput};
use agent_runtime::MinimalRuntimeClient;
use axum::{
    Json, Router,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use clap::Parser;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, env = "AGENT_RUNTIME_BIND", default_value = "127.0.0.1:8090")]
    bind: SocketAddr,

    #[arg(
        long,
        env = "AGENT_RUNTIME_PROFILE",
        default_value = "agent-platform-minimal"
    )]
    profile: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let args = Args::parse();
    let runtime = MinimalRuntimeClient::new(args.profile);
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/v1/runtime/runs",
            post({
                let runtime = runtime.clone();
                move |Json(input): Json<RuntimeRunInput>| {
                    let runtime = runtime.clone();
                    async move {
                        match agent_core::RuntimeClient::execute_run(&runtime, input).await {
                            Ok(output) => {
                                (StatusCode::OK, Json(serde_json::json!(output))).into_response()
                            }
                            Err(error) => (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({"error": error.to_string()})),
                            )
                                .into_response(),
                        }
                    }
                }
            }),
        )
        .route(
            "/v1/runtime/session-messages",
            post({
                let runtime = runtime.clone();
                move |Json(input): Json<RuntimeSessionInput>| {
                    let runtime = runtime.clone();
                    async move {
                        match agent_core::RuntimeClient::send_session_message(&runtime, input).await
                        {
                            Ok(output) => {
                                (StatusCode::OK, Json(serde_json::json!(output))).into_response()
                            }
                            Err(error) => (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({"error": error.to_string()})),
                            )
                                .into_response(),
                        }
                    }
                }
            }),
        )
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(%args.bind, "agent-runtime listening");
    axum::serve(listener, app).await?;
    Ok(())
}
