use agent_core::{
    AgentCoreError, AgentSessionMessage, ConnectorClient, ConnectorSnapshot, CoreResult,
    CredentialLease, CredentialLeaseRequest, CredentialProvider, ErrorCode, ExternalActionMode,
    MessageRole, RuntimeClient, RuntimeOutput, RuntimeRunInput, RuntimeSessionInput,
    WriteConnector, WriteConnectorCompensateInput, WriteConnectorCompensateOutput,
    WriteConnectorDryRunInput, WriteConnectorDryRunOutput, WriteConnectorExecuteInput,
    WriteConnectorExecuteOutput, metric_names, new_id, runtime_failure,
};
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode as HttpStatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use reqwest::{StatusCode as ReqwestStatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::Mutex};

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

    fn ensure_read_only_runtime(&self, external_action_mode: ExternalActionMode) -> CoreResult<()> {
        if matches!(external_action_mode, ExternalActionMode::Authorized) {
            return Err(AgentCoreError::coded(
                ErrorCode::Forbidden,
                "Minimal Runtime refuses authorized external actions",
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
        self.ensure_read_only_runtime(input.run.external_action_mode)?;
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
                "external_action_mode": input.run.external_action_mode,
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

    fn ensure_read_only_runtime(&self, external_action_mode: ExternalActionMode) -> CoreResult<()> {
        if matches!(external_action_mode, ExternalActionMode::Authorized) {
            return Err(AgentCoreError::coded(
                ErrorCode::Forbidden,
                "Hermes Runtime refuses authorized external actions in P1",
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
            if status == ReqwestStatusCode::TOO_MANY_REQUESTS {
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
        self.ensure_read_only_runtime(input.run.external_action_mode)?;
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
            request.external_action_plan_id,
            request.credential_scope,
            request.trace_id,
        ))
    }

    async fn active_lease(&self, _request: CredentialLeaseRequest) -> CoreResult<CredentialLease> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "no-op credential provider cannot issue active credential leases",
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

    async fn execute(
        &self,
        _input: WriteConnectorExecuteInput,
    ) -> CoreResult<WriteConnectorExecuteOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "no-op write connector cannot execute external actions",
        ))
    }

    async fn compensate(
        &self,
        _input: WriteConnectorCompensateInput,
    ) -> CoreResult<WriteConnectorCompensateOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "no-op write connector cannot compensate external actions",
        ))
    }
}

#[derive(Debug, Clone)]
pub struct HttpCredentialProviderConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
    pub lease_ttl_seconds: i64,
}

impl HttpCredentialProviderConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("AGENT_CREDENTIAL_PROVIDER_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        Some(Self {
            base_url: trim_base_url(base_url),
            api_key: std::env::var("AGENT_CREDENTIAL_PROVIDER_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
            timeout: Duration::from_secs(env_u64("AGENT_CREDENTIAL_PROVIDER_TIMEOUT_SECONDS", 10)),
            lease_ttl_seconds: env_i64("AGENT_CREDENTIAL_LEASE_TTL_SECONDS", 300),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HttpCredentialProvider {
    config: HttpCredentialProviderConfig,
    client: reqwest::Client,
    lease_url: Url,
}

impl HttpCredentialProvider {
    pub fn new(config: HttpCredentialProviderConfig) -> CoreResult<Self> {
        let lease_url = Url::parse(&format!("{}/credential-leases", config.base_url))
            .map_err(|_| runtime_failure("invalid credential provider URL"))?;
        Ok(Self {
            config,
            client: reqwest::Client::new(),
            lease_url,
        })
    }
}

#[async_trait]
impl CredentialProvider for HttpCredentialProvider {
    async fn dry_run_lease(&self, request: CredentialLeaseRequest) -> CoreResult<CredentialLease> {
        Ok(CredentialLease::dry_run(
            request.external_action_plan_id,
            request.credential_scope,
            request.trace_id,
        ))
    }

    async fn active_lease(&self, request: CredentialLeaseRequest) -> CoreResult<CredentialLease> {
        let response = post_json(
            &self.client,
            self.lease_url.clone(),
            self.config.api_key.as_deref(),
            &request.trace_id,
            self.config.timeout,
            &request,
        )
        .await?;
        if !response.status().is_success() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "credential provider refused active lease",
            ));
        }
        let body = response
            .json::<HttpCredentialLeaseResponse>()
            .await
            .map_err(|_| runtime_failure("credential provider response was malformed"))?;
        if body.provider_ref.trim().is_empty() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "credential provider returned an empty provider_ref",
            ));
        }
        Ok(CredentialLease::active(
            request.external_action_plan_id,
            request.credential_scope,
            body.provider_ref,
            body.expires_in_seconds
                .unwrap_or(self.config.lease_ttl_seconds)
                .max(1),
            request.trace_id,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct HttpWriteConnectorConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

impl HttpWriteConnectorConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("AGENT_WRITE_CONNECTOR_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        Some(Self {
            base_url: trim_base_url(base_url),
            api_key: std::env::var("AGENT_WRITE_CONNECTOR_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
            timeout: Duration::from_secs(env_u64("AGENT_WRITE_CONNECTOR_TIMEOUT_SECONDS", 30)),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HttpWriteConnector {
    config: HttpWriteConnectorConfig,
    client: reqwest::Client,
    dry_run_url: Url,
    execute_url: Url,
    compensate_url: Url,
}

impl HttpWriteConnector {
    pub fn new(config: HttpWriteConnectorConfig) -> CoreResult<Self> {
        let dry_run_url = Url::parse(&format!("{}/action-executions/dry-run", config.base_url))
            .map_err(|_| runtime_failure("invalid write connector dry-run URL"))?;
        let execute_url = Url::parse(&format!("{}/action-executions/execute", config.base_url))
            .map_err(|_| runtime_failure("invalid write connector execute URL"))?;
        let compensate_url =
            Url::parse(&format!("{}/action-executions/compensate", config.base_url))
                .map_err(|_| runtime_failure("invalid write connector compensate URL"))?;
        Ok(Self {
            config,
            client: reqwest::Client::new(),
            dry_run_url,
            execute_url,
            compensate_url,
        })
    }
}

#[async_trait]
impl WriteConnector for HttpWriteConnector {
    async fn dry_run(
        &self,
        input: WriteConnectorDryRunInput,
    ) -> CoreResult<WriteConnectorDryRunOutput> {
        let response = post_json(
            &self.client,
            self.dry_run_url.clone(),
            self.config.api_key.as_deref(),
            &input.trace_id,
            self.config.timeout,
            &input,
        )
        .await?;
        if !response.status().is_success() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "write connector dry-run failed",
            ));
        }
        response
            .json::<WriteConnectorDryRunOutput>()
            .await
            .map_err(|_| runtime_failure("write connector dry-run response was malformed"))
    }

    async fn execute(
        &self,
        input: WriteConnectorExecuteInput,
    ) -> CoreResult<WriteConnectorExecuteOutput> {
        let response = post_json(
            &self.client,
            self.execute_url.clone(),
            self.config.api_key.as_deref(),
            &input.trace_id,
            self.config.timeout,
            &input,
        )
        .await?;
        if !response.status().is_success() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "write connector execute failed",
            ));
        }
        response
            .json::<WriteConnectorExecuteOutput>()
            .await
            .map_err(|_| runtime_failure("write connector execute response was malformed"))
    }

    async fn compensate(
        &self,
        input: WriteConnectorCompensateInput,
    ) -> CoreResult<WriteConnectorCompensateOutput> {
        let response = post_json(
            &self.client,
            self.compensate_url.clone(),
            self.config.api_key.as_deref(),
            &input.trace_id,
            self.config.timeout,
            &input,
        )
        .await?;
        if !response.status().is_success() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "write connector compensate failed",
            ));
        }
        response
            .json::<WriteConnectorCompensateOutput>()
            .await
            .map_err(|_| runtime_failure("write connector compensate response was malformed"))
    }
}

#[derive(Debug, Deserialize)]
struct HttpCredentialLeaseResponse {
    provider_ref: String,
    expires_in_seconds: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ActionGatewayConfig {
    pub target_log_path: PathBuf,
    pub api_key: Option<String>,
    pub lease_ttl_seconds: i64,
    pub connector: String,
    pub allowed_credential_scopes: Vec<String>,
}

impl ActionGatewayConfig {
    pub fn from_env() -> Self {
        Self {
            target_log_path: std::env::var("AGENT_ACTION_GATEWAY_TARGET_LOG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/tmp/agent-platform-actions.jsonl")),
            api_key: std::env::var("AGENT_ACTION_GATEWAY_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
            lease_ttl_seconds: env_i64("AGENT_ACTION_GATEWAY_LEASE_TTL_SECONDS", 300),
            connector: std::env::var("AGENT_ACTION_GATEWAY_CONNECTOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "action-journal".to_string()),
            allowed_credential_scopes: std::env::var("AGENT_ACTION_GATEWAY_ALLOWED_SCOPES")
                .ok()
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

#[derive(Clone)]
struct ActionGatewayState {
    config: ActionGatewayConfig,
    executions: Arc<Mutex<BTreeMap<String, WriteConnectorExecuteOutput>>>,
    compensations: Arc<Mutex<BTreeMap<String, WriteConnectorCompensateOutput>>>,
    file_lock: Arc<Mutex<()>>,
}

impl ActionGatewayState {
    fn new(config: ActionGatewayConfig) -> CoreResult<Self> {
        let (executions, compensations) = load_action_gateway_log(&config.target_log_path)?;
        Ok(Self {
            config,
            executions: Arc::new(Mutex::new(executions)),
            compensations: Arc::new(Mutex::new(compensations)),
            file_lock: Arc::new(Mutex::new(())),
        })
    }

    fn authorize(&self, headers: &HeaderMap) -> Result<(), ActionGatewayError> {
        let Some(expected) = &self.config.api_key else {
            return Ok(());
        };
        let received = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        if received == Some(expected.as_str()) {
            Ok(())
        } else {
            Err(ActionGatewayError::unauthorized())
        }
    }

    fn credential_scope_allowed(&self, scope: &str) -> bool {
        self.config.allowed_credential_scopes.is_empty()
            || self
                .config
                .allowed_credential_scopes
                .iter()
                .any(|allowed| allowed == scope)
    }

    async fn append_event(&self, event: Value) -> CoreResult<()> {
        let _guard = self.file_lock.lock().await;
        if let Some(parent) = self.config.target_log_path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| runtime_failure("action gateway target directory is not writable"))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.target_log_path)
            .await
            .map_err(|_| runtime_failure("action gateway target log is not writable"))?;
        let encoded = serde_json::to_vec(&event)
            .map_err(|_| runtime_failure("action gateway target event was not serializable"))?;
        file.write_all(&encoded)
            .await
            .map_err(|_| runtime_failure("action gateway target log write failed"))?;
        file.write_all(b"\n")
            .await
            .map_err(|_| runtime_failure("action gateway target log write failed"))?;
        Ok(())
    }
}

#[derive(Debug)]
struct ActionGatewayError {
    status: HttpStatusCode,
    code: &'static str,
    message: &'static str,
}

impl ActionGatewayError {
    fn unauthorized() -> Self {
        Self {
            status: HttpStatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "adapter authentication failed",
        }
    }

    fn forbidden(code: &'static str, message: &'static str) -> Self {
        Self {
            status: HttpStatusCode::FORBIDDEN,
            code,
            message,
        }
    }

    fn conflict(code: &'static str, message: &'static str) -> Self {
        Self {
            status: HttpStatusCode::CONFLICT,
            code,
            message,
        }
    }

    fn internal() -> Self {
        Self {
            status: HttpStatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "action gateway failed",
        }
    }
}

fn action_gateway_error_response(error: ActionGatewayError) -> Response {
    (
        error.status,
        Json(json!({
            "error": error.code,
            "message": error.message,
        })),
    )
        .into_response()
}

pub fn action_gateway_router(config: ActionGatewayConfig) -> CoreResult<Router> {
    let state = ActionGatewayState::new(config)?;
    Ok(Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/credential-leases", post(local_credential_lease))
        .route("/action-executions/dry-run", post(gateway_action_dry_run))
        .route("/action-executions/execute", post(gateway_action_execute))
        .route(
            "/action-executions/compensate",
            post(gateway_action_compensate),
        )
        .with_state(state))
}

async fn local_credential_lease(
    State(state): State<ActionGatewayState>,
    headers: HeaderMap,
    Json(request): Json<CredentialLeaseRequest>,
) -> Response {
    match local_credential_lease_inner(state, headers, request).await {
        Ok(value) => (HttpStatusCode::OK, Json(value)).into_response(),
        Err(error) => action_gateway_error_response(error),
    }
}

async fn local_credential_lease_inner(
    state: ActionGatewayState,
    headers: HeaderMap,
    request: CredentialLeaseRequest,
) -> Result<Value, ActionGatewayError> {
    state.authorize(&headers)?;
    if request.external_action_plan_id.trim().is_empty()
        || request.credential_scope.trim().is_empty()
    {
        return Err(ActionGatewayError::conflict(
            "invalid_credential_request",
            "external_action_plan_id and credential_scope are required",
        ));
    }
    if !state.credential_scope_allowed(&request.credential_scope) {
        return Err(ActionGatewayError::forbidden(
            "credential_scope_not_allowed",
            "credential scope is not allowed by the adapter",
        ));
    }
    let lease_id = new_id("gatewaylease");
    let provider_ref = format!(
        "action-journal-credential://leases/{lease_id}/plans/{}",
        request.external_action_plan_id
    );
    state
        .append_event(json!({
            "event_type": "credential_lease_issued",
            "lease_id": lease_id,
            "external_action_plan_id": request.external_action_plan_id,
            "credential_scope": request.credential_scope,
            "provider_ref": provider_ref,
            "expires_in_seconds": state.config.lease_ttl_seconds.max(1),
            "trace_id": request.trace_id,
        }))
        .await
        .map_err(|_| ActionGatewayError::internal())?;
    Ok(json!({
        "provider_ref": provider_ref,
        "expires_in_seconds": state.config.lease_ttl_seconds.max(1),
    }))
}

async fn gateway_action_dry_run(
    State(state): State<ActionGatewayState>,
    headers: HeaderMap,
    Json(input): Json<WriteConnectorDryRunInput>,
) -> Response {
    match gateway_action_dry_run_inner(state, headers, input).await {
        Ok(value) => (HttpStatusCode::OK, Json(value)).into_response(),
        Err(error) => action_gateway_error_response(error),
    }
}

async fn gateway_action_dry_run_inner(
    state: ActionGatewayState,
    headers: HeaderMap,
    input: WriteConnectorDryRunInput,
) -> Result<WriteConnectorDryRunOutput, ActionGatewayError> {
    state.authorize(&headers)?;
    let accepted = input.plan.connector == state.config.connector;
    Ok(WriteConnectorDryRunOutput {
        accepted,
        status: if accepted {
            "dry_run_ready".to_string()
        } else {
            "rejected".to_string()
        },
        result_ref: accepted.then(|| format!("action-journal-dry-run://{}", input.plan.id)),
        metadata: json!({
            "connector": state.config.connector,
            "trace_id": input.trace_id,
            "target": "local-jsonl",
        }),
    })
}

async fn gateway_action_execute(
    State(state): State<ActionGatewayState>,
    headers: HeaderMap,
    Json(input): Json<WriteConnectorExecuteInput>,
) -> Response {
    match gateway_action_execute_inner(state, headers, input).await {
        Ok(output) => (HttpStatusCode::OK, Json(output)).into_response(),
        Err(error) => action_gateway_error_response(error),
    }
}

async fn gateway_action_execute_inner(
    state: ActionGatewayState,
    headers: HeaderMap,
    input: WriteConnectorExecuteInput,
) -> Result<WriteConnectorExecuteOutput, ActionGatewayError> {
    state.authorize(&headers)?;
    if input.idempotency_key.trim().is_empty() {
        return Err(ActionGatewayError::conflict(
            "idempotency_key_required",
            "idempotency_key is required",
        ));
    }
    let mut executions = state.executions.lock().await;
    if let Some(output) = executions.get(&input.idempotency_key).cloned() {
        return Ok(output);
    }

    if input.plan.connector != state.config.connector {
        return Ok(rejected_write_output(
            "connector_mismatch",
            input.trace_id,
            json!({"expected_connector": state.config.connector}),
        ));
    }
    let provider_ref = input.credential_provider_ref.as_deref().unwrap_or_default();
    if !provider_ref.starts_with("action-journal-credential://")
        || !provider_ref.contains(&input.plan.id)
    {
        return Ok(rejected_write_output(
            "credential_provider_ref_invalid",
            input.trace_id,
            json!({"provider_ref_valid": false}),
        ));
    }

    let event_id = new_id("gatewaytarget");
    let result_ref = format!("action-journal-target://events/{event_id}");
    let compensation_ref = format!(
        "action-journal-compensation://plans/{}/events/{event_id}",
        input.plan.id
    );
    let output = WriteConnectorExecuteOutput {
        accepted: true,
        status: "applied".to_string(),
        result_ref: Some(result_ref.clone()),
        compensation_ref: Some(compensation_ref.clone()),
        error_code: None,
        metadata: json!({
            "adapter": "action_gateway",
            "connector": state.config.connector,
            "target_event_id": event_id,
            "idempotency_key": input.idempotency_key,
            "trace_id": input.trace_id,
        }),
    };
    state
        .append_event(json!({
            "event_type": "action_executed",
            "event_id": event_id,
            "idempotency_key": input.idempotency_key,
            "plan_id": input.plan.id,
            "run_id": input.plan.run_id,
            "connector": input.plan.connector,
            "action": input.plan.action,
            "resource_ref": input.plan.resource_ref,
            "credential_provider_ref": provider_ref,
            "payload": input.payload,
            "result_ref": result_ref,
            "compensation_ref": compensation_ref,
            "trace_id": input.trace_id,
            "output": output,
        }))
        .await
        .map_err(|_| ActionGatewayError::internal())?;
    executions.insert(input.idempotency_key, output.clone());
    Ok(output)
}

fn rejected_write_output(
    error_code: &'static str,
    trace_id: String,
    metadata: Value,
) -> WriteConnectorExecuteOutput {
    WriteConnectorExecuteOutput {
        accepted: false,
        status: "rejected".to_string(),
        result_ref: None,
        compensation_ref: None,
        error_code: Some(error_code.to_string()),
        metadata: json!({
            "trace_id": trace_id,
            "details": metadata,
        }),
    }
}

async fn gateway_action_compensate(
    State(state): State<ActionGatewayState>,
    headers: HeaderMap,
    Json(input): Json<WriteConnectorCompensateInput>,
) -> Response {
    match gateway_action_compensate_inner(state, headers, input).await {
        Ok(output) => (HttpStatusCode::OK, Json(output)).into_response(),
        Err(error) => action_gateway_error_response(error),
    }
}

async fn gateway_action_compensate_inner(
    state: ActionGatewayState,
    headers: HeaderMap,
    input: WriteConnectorCompensateInput,
) -> Result<WriteConnectorCompensateOutput, ActionGatewayError> {
    state.authorize(&headers)?;
    if !input
        .compensation_ref
        .starts_with("action-journal-compensation://")
    {
        return Err(ActionGatewayError::conflict(
            "compensation_ref_invalid",
            "compensation_ref is not owned by the action gateway",
        ));
    }
    let mut compensations = state.compensations.lock().await;
    if let Some(output) = compensations.get(&input.compensation_ref).cloned() {
        return Ok(output);
    }
    let known_compensation_ref =
        state.executions.lock().await.values().any(|output| {
            output.compensation_ref.as_deref() == Some(input.compensation_ref.as_str())
        });
    if !known_compensation_ref {
        return Err(ActionGatewayError::conflict(
            "compensation_ref_not_found",
            "compensation_ref does not match a action gateway execution",
        ));
    }
    let compensation_id = new_id("gatewaycomp");
    let result_ref = format!("action-journal-compensation-result://events/{compensation_id}");
    let output = WriteConnectorCompensateOutput {
        accepted: true,
        status: "compensated".to_string(),
        result_ref: Some(result_ref.clone()),
        error_code: None,
        metadata: json!({
            "adapter": "action_gateway",
            "compensation_id": compensation_id,
            "trace_id": input.trace_id,
        }),
    };
    state
        .append_event(json!({
            "event_type": "action_compensated",
            "compensation_id": compensation_id,
            "compensation_ref": input.compensation_ref,
            "plan_id": input.plan.id,
            "run_id": input.plan.run_id,
            "reason": input.reason,
            "payload": input.payload,
            "result_ref": result_ref,
            "trace_id": input.trace_id,
            "output": output,
        }))
        .await
        .map_err(|_| ActionGatewayError::internal())?;
    compensations.insert(input.compensation_ref, output.clone());
    Ok(output)
}

fn load_action_gateway_log(
    path: &PathBuf,
) -> CoreResult<(
    BTreeMap<String, WriteConnectorExecuteOutput>,
    BTreeMap<String, WriteConnectorCompensateOutput>,
)> {
    if !path.exists() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }
    let mut executions = BTreeMap::new();
    let mut compensations = BTreeMap::new();
    let content = std::fs::read_to_string(path)
        .map_err(|_| runtime_failure("action gateway target log is not readable"))?;
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line)
            .map_err(|_| runtime_failure("action gateway target log is malformed"))?;
        match event.get("event_type").and_then(Value::as_str) {
            Some("action_executed") => {
                let Some(key) = event.get("idempotency_key").and_then(Value::as_str) else {
                    return Err(runtime_failure(
                        "action gateway execution event is missing idempotency_key",
                    ));
                };
                let output = serde_json::from_value::<WriteConnectorExecuteOutput>(
                    event.get("output").cloned().unwrap_or(Value::Null),
                )
                .map_err(|_| runtime_failure("action gateway execution output is malformed"))?;
                executions.insert(key.to_string(), output);
            }
            Some("action_compensated") => {
                let Some(compensation_ref) = event.get("compensation_ref").and_then(Value::as_str)
                else {
                    return Err(runtime_failure(
                        "action gateway compensation event is missing compensation_ref",
                    ));
                };
                let output = serde_json::from_value::<WriteConnectorCompensateOutput>(
                    event.get("output").cloned().unwrap_or(Value::Null),
                )
                .map_err(|_| runtime_failure("action gateway compensation output is malformed"))?;
                compensations.insert(compensation_ref.to_string(), output);
            }
            _ => {}
        }
    }
    Ok((executions, compensations))
}

async fn post_json<T: Serialize + ?Sized>(
    client: &reqwest::Client,
    url: Url,
    api_key: Option<&str>,
    trace_id: &str,
    timeout: Duration,
    body: &T,
) -> CoreResult<reqwest::Response> {
    let mut request = client
        .post(url)
        .timeout(timeout)
        .header("x-agent-trace-id", trace_id)
        .json(body);
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }
    request.send().await.map_err(map_external_connector_error)
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

fn map_external_connector_error(error: reqwest::Error) -> AgentCoreError {
    if error.is_timeout() {
        return AgentCoreError::coded(ErrorCode::InternalError, "external connector timed out");
    }
    AgentCoreError::coded(
        ErrorCode::InternalError,
        "external connector request failed",
    )
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
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
            "Run {} trigger={} target_resource={} risk={} external_action_mode={}. Provide a concise read-only result.",
            input.run.id,
            input.run.trigger_type,
            input.run.target_resource,
            input.run.risk_level,
            input.run.external_action_mode
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
    use agent_core::{
        AgentInstance, AgentRun, CredentialLeaseStatus, ExternalActionPlan, RiskLevel, TriggerType,
        new_trace_id,
    };
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
    async fn minimal_runtime_refuses_authorized_external_actions() {
        let runtime = MinimalRuntimeClient::default();
        let mut run = AgentRun::new(
            "agent-1",
            None,
            TriggerType::Manual,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        run.external_action_mode = ExternalActionMode::Authorized;
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

    #[tokio::test]
    async fn http_credential_provider_returns_active_opaque_lease() {
        let seen_trace: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/credential-leases",
                post(
                    |State(seen_trace): State<Arc<Mutex<Option<String>>>>,
                     headers: HeaderMap,
                     Json(request): Json<CredentialLeaseRequest>| async move {
                        *seen_trace.lock().unwrap() = headers
                            .get("x-agent-trace-id")
                            .and_then(|value| value.to_str().ok())
                            .map(ToString::to_string);
                        Json(json!({
                            "provider_ref": format!("vault://leases/{}", request.external_action_plan_id),
                            "expires_in_seconds": 60
                        }))
                    },
                ),
            )
            .with_state(seen_trace.clone());
        let trace_id = new_trace_id();
        let provider = HttpCredentialProvider::new(HttpCredentialProviderConfig {
            base_url: spawn_server(app).await,
            api_key: Some("secret-token".to_string()),
            timeout: Duration::from_secs(2),
            lease_ttl_seconds: 300,
        })
        .unwrap();

        let lease = provider
            .active_lease(CredentialLeaseRequest {
                external_action_plan_id: "eaplan-1".to_string(),
                credential_scope: "github:issues:write".to_string(),
                trace_id: trace_id.clone(),
            })
            .await
            .unwrap();

        assert_eq!(lease.status, CredentialLeaseStatus::Active);
        assert_eq!(
            lease.provider_ref.as_deref(),
            Some("vault://leases/eaplan-1")
        );
        assert!(lease.expires_at.is_some());
        assert_eq!(*seen_trace.lock().unwrap(), Some(trace_id));
    }

    #[tokio::test]
    async fn http_write_connector_executes_with_provider_ref_and_payload() {
        let seen_provider_ref: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let seen_payload: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/action-executions/execute",
                post(
                    |State((seen_provider_ref, seen_payload)): State<(
                        Arc<Mutex<Option<String>>>,
                        Arc<Mutex<Option<Value>>>,
                    )>,
                     Json(input): Json<WriteConnectorExecuteInput>| async move {
                        *seen_provider_ref.lock().unwrap() = input.credential_provider_ref.clone();
                        *seen_payload.lock().unwrap() = Some(input.payload.clone());
                        Json(WriteConnectorExecuteOutput {
                            accepted: true,
                            status: "applied".to_string(),
                            result_ref: Some(format!("write://result/{}", input.plan.id)),
                            compensation_ref: Some(format!(
                                "compensate://result/{}",
                                input.plan.id
                            )),
                            error_code: None,
                            metadata: json!({"connector": input.plan.connector}),
                        })
                    },
                ),
            )
            .with_state((seen_provider_ref.clone(), seen_payload.clone()));
        let trace_id = new_trace_id();
        let connector = HttpWriteConnector::new(HttpWriteConnectorConfig {
            base_url: spawn_server(app).await,
            api_key: Some("secret-token".to_string()),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let plan = ExternalActionPlan::new(
            "run-1",
            "github",
            "issue.comment",
            "resource:team/project-alpha",
            RiskLevel::Low,
            ExternalActionMode::Authorized,
            trace_id.clone(),
        );

        let output = connector
            .execute(WriteConnectorExecuteInput {
                plan: plan.clone(),
                idempotency_key: plan.id.clone(),
                credential_provider_ref: Some("vault://leases/eaplan-1".to_string()),
                payload: json!({"body": "approved comment"}),
                trace_id,
            })
            .await
            .unwrap();

        assert!(output.accepted);
        assert_eq!(
            output.result_ref,
            Some(format!("write://result/{}", plan.id))
        );
        assert_eq!(
            *seen_provider_ref.lock().unwrap(),
            Some("vault://leases/eaplan-1".to_string())
        );
        assert_eq!(
            seen_payload.lock().unwrap().as_ref().unwrap()["body"],
            "approved comment"
        );
    }

    #[tokio::test]
    async fn action_gateway_executes_idempotently_and_compensates() {
        let target_log = std::env::temp_dir().join(format!(
            "agent-platform-action-gateway-{}.jsonl",
            new_trace_id()
        ));
        let app = action_gateway_router(ActionGatewayConfig {
            target_log_path: target_log.clone(),
            api_key: Some("secret-token".to_string()),
            lease_ttl_seconds: 60,
            connector: "action-journal".to_string(),
            allowed_credential_scopes: vec!["agent-platform:action-gateway-smoke".to_string()],
        })
        .unwrap();
        let base_url = spawn_server(app).await;
        let trace_id = new_trace_id();
        let provider = HttpCredentialProvider::new(HttpCredentialProviderConfig {
            base_url: base_url.clone(),
            api_key: Some("secret-token".to_string()),
            timeout: Duration::from_secs(2),
            lease_ttl_seconds: 300,
        })
        .unwrap();
        let connector = HttpWriteConnector::new(HttpWriteConnectorConfig {
            base_url: base_url.clone(),
            api_key: Some("secret-token".to_string()),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let plan = ExternalActionPlan::new(
            "run-1",
            "action-journal",
            "target.write",
            "resource:team/action-gateway-smoke",
            RiskLevel::Low,
            ExternalActionMode::Authorized,
            trace_id.clone(),
        );
        let lease = provider
            .active_lease(CredentialLeaseRequest {
                external_action_plan_id: plan.id.clone(),
                credential_scope: "agent-platform:action-gateway-smoke".to_string(),
                trace_id: trace_id.clone(),
            })
            .await
            .unwrap();
        let input = WriteConnectorExecuteInput {
            plan: plan.clone(),
            idempotency_key: plan.id.clone(),
            credential_provider_ref: lease.provider_ref.clone(),
            payload: json!({"message": "external action smoke"}),
            trace_id: trace_id.clone(),
        };

        let first = connector.execute(input.clone()).await.unwrap();
        let second = connector.execute(input).await.unwrap();

        assert!(first.accepted);
        assert_eq!(first.status, "applied");
        assert_eq!(first.result_ref, second.result_ref);
        assert_eq!(first.compensation_ref, second.compensation_ref);
        let compensation_ref = first.compensation_ref.clone().unwrap();
        let client = reqwest::Client::new();
        let invalid_compensation = client
            .post(format!("{base_url}/action-executions/compensate"))
            .bearer_auth("secret-token")
            .json(&WriteConnectorCompensateInput {
                plan: plan.clone(),
                compensation_ref: "action-journal-compensation://unknown".to_string(),
                reason: Some("test compensation".to_string()),
                payload: json!({}),
                trace_id: trace_id.clone(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_compensation.status().as_u16(), 409);

        let compensation = client
            .post(format!("{base_url}/action-executions/compensate"))
            .bearer_auth("secret-token")
            .json(&WriteConnectorCompensateInput {
                plan,
                compensation_ref,
                reason: Some("test compensation".to_string()),
                payload: json!({}),
                trace_id,
            })
            .send()
            .await
            .unwrap()
            .json::<WriteConnectorCompensateOutput>()
            .await
            .unwrap();

        assert_eq!(compensation.status, "compensated");
        let log = std::fs::read_to_string(&target_log).unwrap();
        assert_eq!(
            log.matches("\"event_type\":\"credential_lease_issued\"")
                .count(),
            1
        );
        assert_eq!(log.matches("\"event_type\":\"action_executed\"").count(), 1);
        assert_eq!(
            log.matches("\"event_type\":\"action_compensated\"").count(),
            1
        );
        let _ = std::fs::remove_file(target_log);
    }
}
