use agent_core::{
    AgentCoreError, AgentSessionMessage, ConnectorClient, ConnectorSnapshot, CoreResult,
    CredentialLease, CredentialLeaseRequest, CredentialProvider, ErrorCode, MessageRole,
    RuntimeClient, RuntimeOutput, RuntimeRunInput, RuntimeSessionInput, SideEffectMode,
    WriteConnector, WriteConnectorDryRunInput, WriteConnectorDryRunOutput, metric_names,
    runtime_failure,
};
use async_trait::async_trait;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct MinimalRuntimeClient {
    profile: String,
}

impl MinimalRuntimeClient {
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
        }
    }

    fn ensure_read_only_runtime(&self, side_effect_mode: SideEffectMode) -> CoreResult<()> {
        if matches!(side_effect_mode, SideEffectMode::Authorized) {
            return Err(AgentCoreError::coded(
                ErrorCode::Forbidden,
                "Minimal Runtime refuses authorized side effects",
            ));
        }
        Ok(())
    }

    fn result_ref(kind: &str, id: &str) -> String {
        format!("result://{kind}/{id}")
    }
}

impl Default for MinimalRuntimeClient {
    fn default() -> Self {
        Self::new("agent-platform-minimal")
    }
}

#[async_trait]
impl RuntimeClient for MinimalRuntimeClient {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        self.ensure_read_only_runtime(input.run.side_effect_mode)?;
        let context_size = input
            .context
            .as_ref()
            .map(|context| context.recent_messages.len())
            .unwrap_or(0);
        let snapshot_summary = input
            .snapshot
            .as_ref()
            .map(|snapshot| summarize_json(snapshot))
            .unwrap_or_else(|| "no snapshot".to_string());
        let result_summary = format!(
            "Minimal Runtime profile={} executed run {} for {} with {} recent messages; snapshot={}.",
            self.profile, input.run.id, input.run.target_resource, context_size, snapshot_summary
        );
        Ok(RuntimeOutput {
            result_summary,
            result_ref: Some(Self::result_ref("agent-runs", &input.run.id)),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": self.profile,
                "trace_id": input.trace_id,
                "side_effect_mode": input.run.side_effect_mode,
                "read_only": true,
            }),
        })
    }

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        let user_summary = input.message.content_summary.clone().unwrap_or_default();
        let response = format!(
            "Minimal Runtime profile={} received session {} message: {}",
            self.profile, input.session_id, user_summary
        );
        let assistant_message = AgentSessionMessage::new(
            input.session_id.clone(),
            input.message.sequence + 1,
            MessageRole::Assistant,
            Some(response.clone()),
            input.message.run_id.clone(),
            input.trace_id.clone(),
        );
        Ok(RuntimeOutput {
            result_summary: response,
            result_ref: Some(Self::result_ref("agent-sessions", &input.session_id)),
            messages: vec![assistant_message],
            metadata: json!({
                "runtime_profile": self.profile,
                "trace_id": input.trace_id,
                "read_only": true,
            }),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HermesRuntimeConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub profile_models: BTreeMap<String, String>,
    pub timeout: Duration,
}

impl HermesRuntimeConfig {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: trim_base_url(base_url.into()),
            api_key: None,
            model: model.into(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("AGENT_RUNTIME_HERMES_BASE_URL")
            .or_else(|_| std::env::var("HERMES_RUNTIME_BASE_URL"))
            .unwrap_or_else(|_| "http://hermes:8642/v1".to_string());
        let model = std::env::var("AGENT_RUNTIME_HERMES_MODEL")
            .or_else(|_| std::env::var("HERMES_API_SERVER_MODEL_NAME"))
            .unwrap_or_else(|_| "hermes-agent".to_string());
        let timeout = std::env::var("AGENT_RUNTIME_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(30));
        Self {
            base_url: trim_base_url(base_url),
            api_key: std::env::var("AGENT_RUNTIME_HERMES_API_KEY")
                .or_else(|_| std::env::var("HERMES_API_KEY"))
                .ok()
                .filter(|value| !value.is_empty()),
            model,
            profile_models: std::env::var("AGENT_RUNTIME_HERMES_PROFILE_MODELS")
                .ok()
                .map(|value| parse_profile_models(&value))
                .unwrap_or_default(),
            timeout,
        }
    }

    pub fn model_for_profile(&self, runtime_profile: &str) -> String {
        self.profile_models
            .get(runtime_profile)
            .or_else(|| {
                runtime_profile
                    .rsplit_once(':')
                    .and_then(|(_, suffix)| self.profile_models.get(suffix))
            })
            .cloned()
            .unwrap_or_else(|| self.model.clone())
    }
}

#[derive(Debug, Clone)]
pub struct HermesRuntimeClient {
    config: HermesRuntimeConfig,
    client: reqwest::Client,
}

impl HermesRuntimeClient {
    pub fn new(config: HermesRuntimeConfig) -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(runtime_error)?;
        Ok(Self { config, client })
    }

    pub fn from_env() -> CoreResult<Self> {
        Self::new(HermesRuntimeConfig::from_env())
    }

    fn ensure_read_only_runtime(&self, side_effect_mode: SideEffectMode) -> CoreResult<()> {
        if matches!(side_effect_mode, SideEffectMode::Authorized) {
            return Err(AgentCoreError::coded(
                ErrorCode::Forbidden,
                "Hermes Runtime refuses authorized side effects in P1",
            ));
        }
        Ok(())
    }

    async fn chat_completion(
        &self,
        messages: Vec<HermesChatMessage>,
        trace_id: &str,
        runtime_profile: &str,
    ) -> CoreResult<(String, Value)> {
        let started = Instant::now();
        let model = self.config.model_for_profile(runtime_profile);
        let request = HermesChatCompletionRequest {
            model: model.clone(),
            messages,
            stream: false,
            metadata: json!({
                "trace_id": trace_id,
                "runtime_profile": runtime_profile,
                "agent_platform_phase": "p1",
                "read_only": true,
            }),
        };
        let url = chat_url(&self.config.base_url)?;
        let mut builder = self
            .client
            .post(url)
            .header("x-agent-trace-id", trace_id)
            .json(&request);
        if let Some(api_key) = &self.config.api_key {
            builder = builder.bearer_auth(api_key);
        }
        let response = builder.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            if status == StatusCode::TOO_MANY_REQUESTS {
                return Err(AgentCoreError::coded(
                    ErrorCode::RateLimited,
                    "Hermes Runtime rate limited request",
                ));
            }
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                format!("Hermes Runtime returned HTTP {}", status.as_u16()),
            ));
        }
        let body = response
            .json::<HermesChatCompletionResponse>()
            .await
            .map_err(|_| {
                AgentCoreError::coded(
                    ErrorCode::InternalError,
                    "Hermes Runtime response was malformed",
                )
            })?;
        let content = body
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(str::trim)
            .filter(|content| !content.is_empty())
            .ok_or_else(|| {
                AgentCoreError::coded(
                    ErrorCode::InternalError,
                    "Hermes Runtime response did not include assistant content",
                )
            })?
            .to_string();
        let elapsed = started.elapsed().as_secs_f64();
        metrics::counter!(metric_names::RUNTIME_CALL_TOTAL, "runtime" => "hermes").increment(1);
        metrics::histogram!(metric_names::RUNTIME_DURATION_SECONDS, "runtime" => "hermes")
            .record(elapsed);
        Ok((
            content,
            json!({
                "runtime": "hermes",
                "runtime_profile": runtime_profile,
                "hermes_model": model,
                "trace_id": trace_id,
                "read_only": true,
                "duration_ms": (elapsed * 1000.0).round() as i64,
            }),
        ))
    }
}

#[async_trait]
impl RuntimeClient for HermesRuntimeClient {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        self.ensure_read_only_runtime(input.run.side_effect_mode)?;
        let runtime_profile = input
            .agent
            .as_ref()
            .map(|agent| agent.hermes_profile.as_str())
            .unwrap_or("agent-platform-hermes");
        let messages = run_messages(&input, runtime_profile);
        let (content, metadata) = self
            .chat_completion(messages, &input.trace_id, runtime_profile)
            .await?;
        Ok(RuntimeOutput {
            result_summary: content,
            result_ref: Some(format!("hermes://runs/{}", input.run.id)),
            messages: Vec::new(),
            metadata,
        })
    }

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        let runtime_profile = input
            .agent
            .as_ref()
            .map(|agent| agent.hermes_profile.as_str())
            .unwrap_or(input.agent_id.as_str());
        let messages = session_messages(&input, runtime_profile);
        let (content, metadata) = self
            .chat_completion(messages, &input.trace_id, runtime_profile)
            .await?;
        let assistant_message = AgentSessionMessage::new(
            input.session_id.clone(),
            input.message.sequence + 1,
            MessageRole::Assistant,
            Some(content.clone()),
            input.message.run_id.clone(),
            input.trace_id.clone(),
        );
        Ok(RuntimeOutput {
            result_summary: content,
            result_ref: Some(format!("hermes://sessions/{}", input.session_id)),
            messages: vec![assistant_message],
            metadata,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct LocalReadOnlyConnector;

#[async_trait]
impl agent_core::ConnectorClient for LocalReadOnlyConnector {
    async fn read_only_snapshot(
        &self,
        connector: &str,
        resource: &str,
        trace_id: &str,
    ) -> CoreResult<agent_core::ConnectorSnapshot> {
        Ok(agent_core::ConnectorSnapshot {
            connector: connector.to_string(),
            resource: resource.to_string(),
            payload_ref: format!("snapshot://{connector}/{resource}"),
            summary: json!({
                "resource": resource,
                "connector": connector,
                "mode": "read_only",
                "trace_id": trace_id,
            }),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HttpReadOnlyConnectorConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

impl HttpReadOnlyConnectorConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("AGENT_READ_ONLY_CONNECTOR_BASE_URL")
            .ok()
            .filter(|value| !value.is_empty())?;
        let timeout = std::env::var("AGENT_READ_ONLY_CONNECTOR_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(10));
        Some(Self {
            base_url: trim_base_url(base_url),
            api_key: std::env::var("AGENT_READ_ONLY_CONNECTOR_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
            timeout,
        })
    }
}

#[derive(Debug, Clone)]
pub struct HttpReadOnlyConnector {
    config: HttpReadOnlyConnectorConfig,
    client: reqwest::Client,
}

impl HttpReadOnlyConnector {
    pub fn new(config: HttpReadOnlyConnectorConfig) -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(runtime_error)?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl ConnectorClient for HttpReadOnlyConnector {
    async fn read_only_snapshot(
        &self,
        connector: &str,
        resource: &str,
        trace_id: &str,
    ) -> CoreResult<ConnectorSnapshot> {
        let mut url = Url::parse(&format!("{}/snapshots", self.config.base_url))
            .map_err(|_| runtime_failure("invalid read-only connector URL"))?;
        url.query_pairs_mut()
            .append_pair("connector", connector)
            .append_pair("resource", resource);
        let mut request = self.client.get(url).header("x-agent-trace-id", trace_id);
        if let Some(api_key) = &self.config.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await.map_err(map_reqwest_error)?;
        if !response.status().is_success() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                format!(
                    "read-only connector returned HTTP {}",
                    response.status().as_u16()
                ),
            ));
        }
        metrics::counter!(
            metric_names::CONNECTOR_SNAPSHOT_TOTAL,
            "connector" => connector.to_string()
        )
        .increment(1);
        response.json::<ConnectorSnapshot>().await.map_err(|_| {
            AgentCoreError::coded(
                ErrorCode::InternalError,
                "read-only connector response was malformed",
            )
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopCredentialProvider;

#[async_trait]
impl CredentialProvider for NoopCredentialProvider {
    async fn dry_run_lease(&self, request: CredentialLeaseRequest) -> CoreResult<CredentialLease> {
        Ok(CredentialLease::dry_run(
            request.side_effect_plan_id,
            request.credential_scope,
            request.trace_id,
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopWriteConnector;

#[async_trait]
impl WriteConnector for NoopWriteConnector {
    async fn dry_run(
        &self,
        input: WriteConnectorDryRunInput,
    ) -> CoreResult<WriteConnectorDryRunOutput> {
        Ok(WriteConnectorDryRunOutput {
            accepted: true,
            status: "dry_run_ready".to_string(),
            result_ref: Some(format!("noop://write-connector/dry-run/{}", input.plan.id)),
            metadata: json!({
                "trace_id": input.trace_id,
                "connector": input.plan.connector,
                "action": input.plan.action,
                "readiness_only": true,
            }),
        })
    }
}

fn summarize_json(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "object_keys:{}",
            map.keys().cloned().collect::<Vec<_>>().join(",")
        ),
        Value::Array(items) => format!("array_len:{}", items.len()),
        Value::String(value) => value.chars().take(120).collect(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn trim_base_url(value: String) -> String {
    value.trim_end_matches('/').to_string()
}

fn parse_profile_models(value: &str) -> BTreeMap<String, String> {
    if let Ok(parsed) = serde_json::from_str::<BTreeMap<String, String>>(value) {
        return parsed
            .into_iter()
            .filter(|(profile, model)| !profile.trim().is_empty() && !model.trim().is_empty())
            .map(|(profile, model)| (profile.trim().to_string(), model.trim().to_string()))
            .collect();
    }

    value
        .split(',')
        .filter_map(|item| item.split_once('='))
        .map(|(profile, model)| (profile.trim(), model.trim()))
        .filter(|(profile, model)| !profile.is_empty() && !model.is_empty())
        .map(|(profile, model)| (profile.to_string(), model.to_string()))
        .collect()
}

fn chat_url(base_url: &str) -> CoreResult<Url> {
    Url::parse(&format!("{base_url}/chat/completions"))
        .map_err(|_| runtime_failure("invalid Hermes Runtime URL"))
}

fn map_reqwest_error(error: reqwest::Error) -> AgentCoreError {
    if error.is_timeout() {
        metrics::counter!(metric_names::RUNTIME_TIMEOUT_TOTAL, "runtime" => "hermes").increment(1);
        return AgentCoreError::coded(ErrorCode::InternalError, "Hermes Runtime timed out");
    }
    AgentCoreError::coded(ErrorCode::InternalError, "Hermes Runtime request failed")
}

#[derive(Debug, Clone, Serialize)]
struct HermesChatCompletionRequest {
    model: String,
    messages: Vec<HermesChatMessage>,
    stream: bool,
    metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
struct HermesChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct HermesChatCompletionResponse {
    #[serde(default)]
    choices: Vec<HermesChoice>,
}

#[derive(Debug, Deserialize)]
struct HermesChoice {
    message: HermesMessage,
}

#[derive(Debug, Deserialize)]
struct HermesMessage {
    content: Option<String>,
}

fn run_messages(input: &RuntimeRunInput, runtime_profile: &str) -> Vec<HermesChatMessage> {
    let mut messages = vec![HermesChatMessage {
        role: "system".to_string(),
        content: format!(
            "You are executing an Agent Platform P1 read-only run. Runtime profile: {runtime_profile}. Never perform external writes or request write credentials."
        ),
    }];
    if let Some(context) = &input.context {
        if let Some(summary) = &context.context_summary {
            messages.push(HermesChatMessage {
                role: "system".to_string(),
                content: format!("Session context summary: {summary}"),
            });
        }
        for message in &context.recent_messages {
            messages.push(HermesChatMessage {
                role: message.role.to_string(),
                content: message.content_summary.clone(),
            });
        }
    }
    if let Some(snapshot) = &input.snapshot {
        messages.push(HermesChatMessage {
            role: "system".to_string(),
            content: format!(
                "Read-only connector snapshot summary: {}",
                summarize_json(snapshot)
            ),
        });
    }
    messages.push(HermesChatMessage {
        role: "user".to_string(),
        content: format!(
            "Run {} trigger={} target_resource={} risk={} side_effect_mode={}. Provide a concise read-only result.",
            input.run.id,
            input.run.trigger_type,
            input.run.target_resource,
            input.run.risk_level,
            input.run.side_effect_mode
        ),
    });
    messages
}

fn session_messages(input: &RuntimeSessionInput, runtime_profile: &str) -> Vec<HermesChatMessage> {
    let mut messages = vec![HermesChatMessage {
        role: "system".to_string(),
        content: format!(
            "You are in an Agent Platform P1 read-only session. Runtime profile: {runtime_profile}. Do not request or use write credentials."
        ),
    }];
    if let Some(summary) = &input.context.context_summary {
        messages.push(HermesChatMessage {
            role: "system".to_string(),
            content: format!("Session context summary: {summary}"),
        });
    }
    if let Some(snapshot) = &input.snapshot {
        messages.push(HermesChatMessage {
            role: "system".to_string(),
            content: format!(
                "Read-only connector snapshot summary: {}",
                summarize_json(snapshot)
            ),
        });
    }
    for message in &input.context.recent_messages {
        if message.role == input.message.role
            && Some(message.content_summary.as_str()) == input.message.content_summary.as_deref()
            && message.external_message_id == input.message.external_message_id
        {
            continue;
        }
        messages.push(HermesChatMessage {
            role: message.role.to_string(),
            content: message.content_summary.clone(),
        });
    }
    messages.push(HermesChatMessage {
        role: input.message.role.to_string(),
        content: input.message.content_summary.clone().unwrap_or_default(),
    });
    messages
}

pub fn runtime_error(error: impl std::fmt::Display) -> agent_core::AgentCoreError {
    runtime_failure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{AgentInstance, AgentRun, TriggerType, new_trace_id};
    use axum::{
        Json, Router,
        extract::{Query, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    use tokio::net::TcpListener;

    fn hermes_run_input(trace_id: String) -> RuntimeRunInput {
        RuntimeRunInput {
            trace_id: trace_id.clone(),
            run: AgentRun::new(
                "agent-1",
                None,
                TriggerType::Manual,
                "resource:team/project-alpha",
                trace_id,
            ),
            agent: None,
            context: None,
            snapshot: None,
        }
    }

    async fn spawn_server(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn minimal_runtime_refuses_authorized_side_effects() {
        let runtime = MinimalRuntimeClient::default();
        let mut run = AgentRun::new(
            "agent-1",
            None,
            TriggerType::Manual,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        run.side_effect_mode = SideEffectMode::Authorized;
        let result = runtime
            .execute_run(RuntimeRunInput {
                trace_id: run.trace_id.clone(),
                run,
                agent: None,
                context: None,
                snapshot: None,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn hermes_runtime_success_passes_trace_id_and_metadata() {
        let seen_trace: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/chat/completions",
                post(
                    |State(seen_trace): State<Arc<Mutex<Option<String>>>>,
                     headers: HeaderMap,
                     Json(body): Json<Value>| async move {
                        *seen_trace.lock().unwrap() = headers
                            .get("x-agent-trace-id")
                            .and_then(|value| value.to_str().ok())
                            .map(ToString::to_string);
                        assert_eq!(body["model"], "hermes-agent");
                        Json(json!({
                            "choices": [
                                {"message": {"role": "assistant", "content": "runtime ok"}}
                            ]
                        }))
                    },
                ),
            )
            .with_state(seen_trace.clone());
        let base_url = spawn_server(app).await;
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url,
            api_key: Some("test-key".to_string()),
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let trace_id = new_trace_id();
        let output = runtime
            .execute_run(hermes_run_input(trace_id.clone()))
            .await
            .unwrap();

        assert_eq!(output.result_summary, "runtime ok");
        assert_eq!(output.metadata["runtime"], "hermes");
        assert_eq!(*seen_trace.lock().unwrap(), Some(trace_id));
    }

    #[tokio::test]
    async fn hermes_runtime_routes_model_by_agent_profile() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["model"], "profile-model");
                Json(json!({
                    "choices": [
                        {"message": {"role": "assistant", "content": "profile routed"}}
                    ]
                }))
            }),
        );
        let mut profile_models = BTreeMap::new();
        profile_models.insert(
            "background_worker:analysis".to_string(),
            "profile-model".to_string(),
        );
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "default-model".to_string(),
            profile_models,
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let trace_id = new_trace_id();
        let mut input = hermes_run_input(trace_id);
        let mut agent = AgentInstance::new(
            "user-1",
            "background_worker",
            "resource:team/project-alpha",
            "hash",
            json!({"hermes_profile": "background_worker:analysis"}),
            new_trace_id(),
        );
        agent.id = input.run.agent_id.clone();
        input.agent = Some(agent);

        let output = runtime.execute_run(input).await.unwrap();

        assert_eq!(output.result_summary, "profile routed");
        assert_eq!(output.metadata["hermes_model"], "profile-model");
    }

    #[tokio::test]
    async fn hermes_runtime_maps_5xx_to_safe_internal_error() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async { (StatusCode::BAD_GATEWAY, Json(json!({"error": "upstream"}))) }),
        );
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let result = runtime.execute_run(hermes_run_input(new_trace_id())).await;
        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::InternalError,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn hermes_runtime_rejects_malformed_response() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async { Json(json!({"choices": []})) }),
        );
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let result = runtime.execute_run(hermes_run_input(new_trace_id())).await;
        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::InternalError,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn hermes_runtime_maps_timeout_without_leaking_prompt() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Json(json!({
                    "choices": [
                        {"message": {"role": "assistant", "content": "too late"}}
                    ]
                }))
            }),
        );
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_millis(10),
        })
        .unwrap();
        let result = runtime.execute_run(hermes_run_input(new_trace_id())).await;
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            AgentCoreError::Coded {
                code: ErrorCode::InternalError,
                ..
            }
        ));
        assert!(!error.to_string().contains("resource:team/project-alpha"));
    }

    #[tokio::test]
    async fn http_read_only_connector_passes_trace_and_parses_snapshot() {
        let seen_trace: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/snapshots",
                get(
                    |State(seen_trace): State<Arc<Mutex<Option<String>>>>,
                     headers: HeaderMap,
                     Query(query): Query<BTreeMap<String, String>>| async move {
                        *seen_trace.lock().unwrap() = headers
                            .get("x-agent-trace-id")
                            .and_then(|value| value.to_str().ok())
                            .map(ToString::to_string);
                        Json(ConnectorSnapshot {
                            connector: query.get("connector").cloned().unwrap_or_default(),
                            resource: query.get("resource").cloned().unwrap_or_default(),
                            payload_ref: "snapshot://http/test".to_string(),
                            summary: json!({"mode": "read_only", "source": "http"}),
                        })
                    },
                ),
            )
            .with_state(seen_trace.clone());
        let trace_id = new_trace_id();
        let connector = HttpReadOnlyConnector::new(HttpReadOnlyConnectorConfig {
            base_url: spawn_server(app).await,
            api_key: Some("secret-token".to_string()),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let snapshot = connector
            .read_only_snapshot("github", "resource:team/project-alpha", &trace_id)
            .await
            .unwrap();

        assert_eq!(snapshot.connector, "github");
        assert_eq!(snapshot.summary["mode"], "read_only");
        assert_eq!(*seen_trace.lock().unwrap(), Some(trace_id));
    }

    #[tokio::test]
    async fn http_read_only_connector_error_does_not_leak_secret_or_resource() {
        let app = Router::new().route(
            "/snapshots",
            get(|| async {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": "secret-token"})),
                )
            }),
        );
        let connector = HttpReadOnlyConnector::new(HttpReadOnlyConnectorConfig {
            base_url: spawn_server(app).await,
            api_key: Some("secret-token".to_string()),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let error = connector
            .read_only_snapshot("github", "resource:team/project-alpha", &new_trace_id())
            .await
            .unwrap_err();

        assert!(!error.to_string().contains("secret-token"));
        assert!(!error.to_string().contains("project-alpha"));
    }
}
