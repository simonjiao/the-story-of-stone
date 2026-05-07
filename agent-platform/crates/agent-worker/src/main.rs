use agent_worker::{Worker, idle_heartbeat, minimal_runtime, observer_tick, store_from_env};
use clap::{Parser, Subcommand};
use std::time::Duration;

#[derive(Debug, Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Worker {
        #[arg(long, env = "AGENT_WORKER_ID", default_value = "worker-local")]
        worker_id: String,
        #[arg(long, default_value_t = false)]
        once: bool,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
    Observer {
        #[arg(long, default_value_t = false)]
        once: bool,
        #[arg(long, default_value_t = 30000)]
        interval_ms: u64,
    },
    Heartbeat {
        #[arg(long, env = "AGENT_WORKER_ID", default_value = "worker-local")]
        worker_id: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let args = Args::parse();
    let store = store_from_env().await?;
    match args.command {
        Command::Worker {
            worker_id,
            once,
            interval_ms,
        } => {
            let worker = Worker::new(store, minimal_runtime(), worker_id);
            loop {
                worker.tick().await?;
                if once {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
            }
        }
        Command::Observer { once, interval_ms } => loop {
            let trace_id = agent_core::new_trace_id();
            observer_tick(store.clone(), &trace_id).await?;
            if once {
                break;
            }
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        },
        Command::Heartbeat { worker_id } => {
            let trace_id = agent_core::new_trace_id();
            idle_heartbeat(store, &worker_id, &trace_id).await?;
        }
    }
    Ok(())
}
