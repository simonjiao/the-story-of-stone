use clap::Parser;
use std::net::SocketAddr;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, env = "AGENT_MANAGER_BIND", default_value = "127.0.0.1:8088")]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let args = Args::parse();
    agent_manager::serve(args.bind).await
}
