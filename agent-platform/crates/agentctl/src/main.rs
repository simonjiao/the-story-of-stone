use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(name = "agentctl")]
#[command(about = "Agent Manager admin CLI")]
struct Cli {
    #[arg(
        long,
        env = "AGENT_MANAGER_URL",
        default_value = "http://127.0.0.1:8088"
    )]
    manager_url: String,

    #[arg(long, env = "AGENTCTL_USER", default_value = "admin")]
    user: String,

    #[arg(long, env = "AGENTCTL_SERVICE", default_value = "agentctl")]
    service: String,

    #[arg(long, env = "AGENTCTL_ROLES", default_value = "system_admin")]
    roles: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Requests(RequestsCommand),
    Agents(AgentsCommand),
    Runs(RunsCommand),
    Audit(AuditCommand),
    Observer(ObserverCommand),
}

#[derive(Debug, Args)]
struct RequestsCommand {
    #[command(subcommand)]
    command: RequestsSubcommand,
}

#[derive(Debug, Subcommand)]
enum RequestsSubcommand {
    List {
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    Approve {
        request_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Deny {
        request_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Args)]
struct AgentsCommand {
    #[command(subcommand)]
    command: AgentsSubcommand,
}

#[derive(Debug, Subcommand)]
enum AgentsSubcommand {
    List {
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    Pause {
        agent_id: String,
    },
    Resume {
        agent_id: String,
    },
}

#[derive(Debug, Args)]
struct RunsCommand {
    #[command(subcommand)]
    command: RunsSubcommand,
}

#[derive(Debug, Subcommand)]
enum RunsSubcommand {
    List {
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    #[command(alias = "inspect")]
    Show { run_id: String },
    Retry {
        run_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Terminate {
        run_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    DryRunExternalAction {
        run_id: String,
        #[arg(long)]
        connector: String,
        #[arg(long)]
        action: String,
        #[arg(long)]
        resource_ref: String,
        #[arg(long)]
        credential_scope: Option<String>,
        #[arg(long)]
        approval_id: Option<String>,
        #[arg(long)]
        input_summary: Option<String>,
        #[arg(long)]
        risk_level: Option<String>,
        #[arg(long)]
        external_action_mode: Option<String>,
    },
    ApplyExternalAction {
        run_id: String,
        plan_id: String,
        #[arg(long)]
        payload_json: Option<String>,
    },
}

#[derive(Debug, Args)]
struct AuditCommand {
    #[arg(long, default_value_t = 100)]
    limit: i64,
}

#[derive(Debug, Args)]
struct ObserverCommand {
    #[command(subcommand)]
    command: ObserverSubcommand,
}

#[derive(Debug, Subcommand)]
enum ObserverSubcommand {
    Reports {
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    Show {
        report_id: String,
    },
    Discuss {
        report_id: String,
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        message: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    SystemSession {
        #[arg(long)]
        report_id: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Run,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    let headers = headers(&cli)?;
    let manager_url = cli.manager_url.clone();
    let value = match cli.command {
        Command::Requests(command) => match command.command {
            RequestsSubcommand::List { limit } => {
                get(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/requests?limit={limit}"),
                )
                .await?
            }
            RequestsSubcommand::Approve { request_id, reason } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/requests/{request_id}/approve"),
                    json!({ "reason": reason }),
                )
                .await?
            }
            RequestsSubcommand::Deny { request_id, reason } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/requests/{request_id}/deny"),
                    json!({ "reason": reason }),
                )
                .await?
            }
        },
        Command::Agents(command) => match command.command {
            AgentsSubcommand::List { limit } => {
                get(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/agents?limit={limit}"),
                )
                .await?
            }
            AgentsSubcommand::Pause { agent_id } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/agents/{agent_id}/pause"),
                    json!({}),
                )
                .await?
            }
            AgentsSubcommand::Resume { agent_id } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/agents/{agent_id}/resume"),
                    json!({}),
                )
                .await?
            }
        },
        Command::Runs(command) => match command.command {
            RunsSubcommand::List { limit } => {
                get(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/runs?limit={limit}"),
                )
                .await?
            }
            RunsSubcommand::Show { run_id } => {
                get(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/runs/{run_id}"),
                )
                .await?
            }
            RunsSubcommand::Retry { run_id, reason } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/runs/{run_id}/retry"),
                    json!({ "reason": reason }),
                )
                .await?
            }
            RunsSubcommand::Terminate { run_id, reason } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/runs/{run_id}/terminate"),
                    json!({ "reason": reason }),
                )
                .await?
            }
            RunsSubcommand::DryRunExternalAction {
                run_id,
                connector,
                action,
                resource_ref,
                credential_scope,
                approval_id,
                input_summary,
                risk_level,
                external_action_mode,
            } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/runs/{run_id}/external-action-plans/dry-run"),
                    json!({
                        "connector": connector,
                        "action": action,
                        "resource_ref": resource_ref,
                        "credential_scope": credential_scope,
                        "approval_id": approval_id,
                        "input_summary": input_summary,
                        "risk_level": risk_level,
                        "external_action_mode": external_action_mode,
                    }),
                )
                .await?
            }
            RunsSubcommand::ApplyExternalAction {
                run_id,
                plan_id,
                payload_json,
            } => {
                let payload = match payload_json {
                    Some(value) => serde_json::from_str::<Value>(&value)
                        .context("payload-json must be valid JSON")?,
                    None => json!({}),
                };
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/runs/{run_id}/external-action-plans/{plan_id}/apply"),
                    json!({ "payload": payload }),
                )
                .await?
            }
        },
        Command::Audit(command) => {
            get(
                &client,
                &manager_url,
                &headers,
                &format!("/v1/admin/audit?limit={}", command.limit),
            )
            .await?
        }
        Command::Observer(command) => match command.command {
            ObserverSubcommand::Reports { limit } => {
                get(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/observer/reports?limit={limit}"),
                )
                .await?
            }
            ObserverSubcommand::Show { report_id } => {
                get(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/observer/reports/{report_id}"),
                )
                .await?
            }
            ObserverSubcommand::Discuss {
                report_id,
                agent_id,
                message,
                idempotency_key,
            } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    &format!("/v1/admin/observer/reports/{report_id}/discussions"),
                    json!({
                        "agent_id": agent_id,
                        "initial_message": message,
                        "idempotency_key": idempotency_key,
                    }),
                )
                .await?
            }
            ObserverSubcommand::SystemSession {
                report_id,
                message,
                idempotency_key,
            } => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    "/v1/admin/observer/system-session",
                    json!({
                        "report_id": report_id,
                        "initial_message": message,
                        "idempotency_key": idempotency_key,
                    }),
                )
                .await?
            }
            ObserverSubcommand::Run => {
                post(
                    &client,
                    &manager_url,
                    &headers,
                    "/v1/admin/observer/runs",
                    json!({}),
                )
                .await?
            }
        },
    };

    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn headers(cli: &Cli) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-agent-user"),
        HeaderValue::from_str(&cli.user)?,
    );
    headers.insert(
        HeaderName::from_static("x-agent-service"),
        HeaderValue::from_str(&cli.service)?,
    );
    headers.insert(
        HeaderName::from_static("x-agent-roles"),
        HeaderValue::from_str(&cli.roles)?,
    );
    headers.insert(
        HeaderName::from_static("x-agent-allowed-actions"),
        HeaderValue::from_static("*"),
    );
    headers.insert(
        HeaderName::from_static("x-agent-resource-allowlist"),
        HeaderValue::from_static("*"),
    );
    Ok(headers)
}

async fn get(
    client: &reqwest::Client,
    manager_url: &str,
    headers: &HeaderMap,
    path: &str,
) -> anyhow::Result<Value> {
    let response = client
        .get(format!("{}{}", manager_url, path))
        .headers(headers.clone())
        .send()
        .await
        .context("request failed")?;
    response_json(response).await
}

async fn post(
    client: &reqwest::Client,
    manager_url: &str,
    headers: &HeaderMap,
    path: &str,
    body: Value,
) -> anyhow::Result<Value> {
    let response = client
        .post(format!("{}{}", manager_url, path))
        .headers(headers.clone())
        .json(&body)
        .send()
        .await
        .context("request failed")?;
    response_json(response).await
}

async fn response_json(response: reqwest::Response) -> anyhow::Result<Value> {
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .context("invalid JSON response")?;
    if !status.is_success() {
        bail!(
            "manager returned {status}: {}",
            serde_json::to_string_pretty(&value)?
        );
    }
    Ok(value)
}
