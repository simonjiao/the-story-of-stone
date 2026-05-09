use agent_runtime::{ActionGatewayConfig, action_gateway_router};
use clap::Parser;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

#[derive(Debug, Parser)]
struct Args {
    #[arg(
        long,
        env = "AGENT_ACTION_GATEWAY_BIND",
        default_value = "127.0.0.1:8091"
    )]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let args = Args::parse();
    let app =
        action_gateway_router(ActionGatewayConfig::from_env())?.layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(%args.bind, "agent-action-gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}
