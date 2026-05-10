use agent_core::{
    AgentCoreError, AgentSessionMessage, ConnectorClient, ConnectorSnapshot, CoreResult,
    CredentialLease, CredentialLeaseRequest, CredentialProvider, ErrorCode, ExternalActionMode,
    MessageRole, ProfileContract, RuntimeClient, RuntimeOutput, RuntimeProfileInput,
    RuntimeProfileMessage, RuntimeRunInput, RuntimeSessionInput, RuntimeStreamEvent,
    RuntimeStreamEventType, RuntimeToolCall, RuntimeToolExecutor, RuntimeToolResult,
    RuntimeToolSpec, WriteConnector, WriteConnectorCompensateInput, WriteConnectorCompensateOutput,
    WriteConnectorDryRunInput, WriteConnectorDryRunOutput, WriteConnectorExecuteInput,
    WriteConnectorExecuteOutput, metric_names, new_id, runtime_failure, runtime_stream_error_event,
    validate_json_schema_value,
};
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode as HttpStatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
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

const DEFAULT_TOOL_RESULT_INLINE_BYTES: usize = 8192;

#[derive(Debug, Clone, Default)]
pub struct RuntimeProfileRegistry {
    contracts: Arc<BTreeMap<String, ProfileContract>>,
}

impl RuntimeProfileRegistry {
    pub fn new(contracts: impl IntoIterator<Item = ProfileContract>) -> Self {
        Self {
            contracts: Arc::new(
                contracts
                    .into_iter()
                    .map(|contract| (contract.profile_id.clone(), contract))
                    .collect(),
            ),
        }
    }

    pub fn get(&self, profile_id: &str) -> Option<ProfileContract> {
        self.contracts.get(profile_id).cloned()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DenyRuntimeToolExecutor;

#[async_trait]
impl RuntimeToolExecutor for DenyRuntimeToolExecutor {
    async fn execute_tool(
        &self,
        _call: RuntimeToolCall,
        _spec: RuntimeToolSpec,
    ) -> CoreResult<RuntimeToolResult> {
        Err(AgentCoreError::coded(
            ErrorCode::Forbidden,
            "runtime tool executor is not configured",
        ))
    }
}

#[async_trait]
pub trait RuntimeAuditSink: Send + Sync {
    async fn append_runtime_event(&self, event: Value) -> CoreResult<()>;
}

#[derive(Debug, Clone, Default)]
pub struct NoopRuntimeAuditSink;

#[async_trait]
impl RuntimeAuditSink for NoopRuntimeAuditSink {
    async fn append_runtime_event(&self, _event: Value) -> CoreResult<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct JsonlRuntimeAuditSink {
    path: PathBuf,
    file_lock: Arc<Mutex<()>>,
}

impl JsonlRuntimeAuditSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            file_lock: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait]
impl RuntimeAuditSink for JsonlRuntimeAuditSink {
    async fn append_runtime_event(&self, event: Value) -> CoreResult<()> {
        let _guard = self.file_lock.lock().await;
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| runtime_failure("runtime audit directory is not writable"))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|_| runtime_failure("runtime audit log is not writable"))?;
        let encoded = serde_json::to_vec(&event)
            .map_err(|_| runtime_failure("runtime audit event was not serializable"))?;
        file.write_all(&encoded)
            .await
            .map_err(|_| runtime_failure("runtime audit log write failed"))?;
        file.write_all(b"\n")
            .await
            .map_err(|_| runtime_failure("runtime audit log write failed"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct StaticRuntimeToolExecutor {
    outputs: Arc<BTreeMap<String, Value>>,
}

impl StaticRuntimeToolExecutor {
    pub fn new(outputs: impl IntoIterator<Item = (String, Value)>) -> Self {
        Self {
            outputs: Arc::new(outputs.into_iter().collect()),
        }
    }
}

#[async_trait]
impl RuntimeToolExecutor for StaticRuntimeToolExecutor {
    async fn execute_tool(
        &self,
        call: RuntimeToolCall,
        _spec: RuntimeToolSpec,
    ) -> CoreResult<RuntimeToolResult> {
        let output = self.outputs.get(&call.tool_name).cloned().ok_or_else(|| {
            AgentCoreError::coded(ErrorCode::NotFound, "runtime tool was not registered")
        })?;
        Ok(RuntimeToolResult {
            call_id: call.call_id,
            profile_id: call.profile_id,
            tool_name: call.tool_name,
            output_ref: Some(format!("runtime://tool-results/{}", new_id("rttoolout"))),
            output,
            metadata: json!({
                "runtime_tool_executor": "static",
                "trace_id": call.trace_id,
            }),
        })
    }
}

#[derive(Debug, Clone)]
pub struct MinimalRuntimeClient {
    profile: String,
    registry: RuntimeProfileRegistry,
}

impl MinimalRuntimeClient {
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            registry: RuntimeProfileRegistry::default(),
        }
    }

    pub fn with_profile_registry(mut self, registry: RuntimeProfileRegistry) -> Self {
        self.registry = registry;
        self
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
        let runtime_profile = runtime_profile_for_run(&input, &self.profile);
        let contract = input
            .profile_contract
            .clone()
            .or_else(|| self.registry.get(&runtime_profile));
        validate_contract_input(
            contract.as_ref(),
            &runtime_run_contract_payload(&input, &runtime_profile),
            &input.requested_tools,
            &runtime_profile,
        )?;
        let context_size = input
            .context
            .as_ref()
            .map(|context| context.recent_messages.len())
            .unwrap_or(0);
        let snapshot_summary = input
            .snapshot
            .as_ref()
            .map(summarize_json)
            .unwrap_or_else(|| "no snapshot".to_string());
        let result_summary = format!(
            "Minimal Runtime profile={} executed run {} for {} with {} recent messages; snapshot={}.",
            runtime_profile,
            input.run.id,
            input.run.target_resource,
            context_size,
            snapshot_summary
        );
        finalize_runtime_output(
            RuntimeOutput {
                result_summary,
                result_ref: Some(Self::result_ref("agent-runs", &input.run.id)),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": runtime_profile,
                    "trace_id": input.trace_id,
                    "external_action_mode": input.run.external_action_mode,
                    "read_only": true,
                }),
            },
            contract.as_ref(),
            &input.requested_tools,
            input.runtime_step.as_ref(),
        )
    }

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        let runtime_profile = runtime_profile_for_session(&input, &self.profile);
        let contract = input
            .profile_contract
            .clone()
            .or_else(|| self.registry.get(&runtime_profile));
        validate_contract_input(
            contract.as_ref(),
            &runtime_session_contract_payload(&input, &runtime_profile),
            &input.requested_tools,
            &runtime_profile,
        )?;
        let user_summary = input.message.content_summary.clone().unwrap_or_default();
        let response = format!(
            "Minimal Runtime profile={} received session {} message: {}",
            runtime_profile, input.session_id, user_summary
        );
        let assistant_message = AgentSessionMessage::new(
            input.session_id.clone(),
            input.message.sequence + 1,
            MessageRole::Assistant,
            Some(response.clone()),
            input.message.run_id.clone(),
            input.trace_id.clone(),
        );
        finalize_runtime_output(
            RuntimeOutput {
                result_summary: response,
                result_ref: Some(Self::result_ref("agent-sessions", &input.session_id)),
                messages: vec![assistant_message],
                metadata: json!({
                    "runtime_profile": runtime_profile,
                    "trace_id": input.trace_id,
                    "read_only": true,
                }),
            },
            contract.as_ref(),
            &input.requested_tools,
            input.runtime_step.as_ref(),
        )
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let contract = input
            .profile_contract
            .clone()
            .or_else(|| self.registry.get(&input.profile_id));
        validate_contract_input(
            contract.as_ref(),
            &runtime_profile_contract_payload(&input),
            &input.requested_tools,
            &input.profile_id,
        )?;
        let content = input
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        finalize_runtime_output(
            RuntimeOutput {
                result_summary: format!(
                    "Minimal Runtime profile={} received profile step: {}",
                    input.profile_id, content
                ),
                result_ref: Some(Self::result_ref("runtime-profiles", &input.profile_id)),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "read_only": true,
                }),
            },
            contract.as_ref(),
            &input.requested_tools,
            input.runtime_step.as_ref(),
        )
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

#[derive(Clone)]
pub struct HermesRuntimeClient {
    config: HermesRuntimeConfig,
    client: reqwest::Client,
    registry: RuntimeProfileRegistry,
    tool_executor: Arc<dyn RuntimeToolExecutor>,
    audit_sink: Arc<dyn RuntimeAuditSink>,
    max_tool_rounds: usize,
    max_tool_result_inline_bytes: usize,
}

impl HermesRuntimeClient {
    pub fn new(config: HermesRuntimeConfig) -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(runtime_error)?;
        Ok(Self {
            config,
            client,
            registry: RuntimeProfileRegistry::default(),
            tool_executor: Arc::new(DenyRuntimeToolExecutor),
            audit_sink: Arc::new(NoopRuntimeAuditSink),
            max_tool_rounds: 4,
            max_tool_result_inline_bytes: DEFAULT_TOOL_RESULT_INLINE_BYTES,
        })
    }

    pub fn from_env() -> CoreResult<Self> {
        let mut client = Self::new(HermesRuntimeConfig::from_env())?;
        if let Some(config) = HttpRuntimeToolExecutorConfig::from_env() {
            client.tool_executor = Arc::new(HttpRuntimeToolExecutor::new(config)?);
        }
        if let Ok(path) = std::env::var("AGENT_RUNTIME_AUDIT_LOG")
            && !path.trim().is_empty()
        {
            client.audit_sink = Arc::new(JsonlRuntimeAuditSink::new(path));
        }
        client.max_tool_result_inline_bytes = env_u64(
            "AGENT_RUNTIME_TOOL_RESULT_INLINE_BYTES",
            DEFAULT_TOOL_RESULT_INLINE_BYTES as u64,
        ) as usize;
        Ok(client)
    }

    pub fn with_profile_registry(mut self, registry: RuntimeProfileRegistry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_tool_executor(mut self, executor: Arc<dyn RuntimeToolExecutor>) -> Self {
        self.tool_executor = executor;
        self
    }

    pub fn with_audit_sink(mut self, audit_sink: Arc<dyn RuntimeAuditSink>) -> Self {
        self.audit_sink = audit_sink;
        self
    }

    pub fn with_max_tool_rounds(mut self, max_tool_rounds: usize) -> Self {
        self.max_tool_rounds = max_tool_rounds;
        self
    }

    pub fn with_max_tool_result_inline_bytes(mut self, bytes: usize) -> Self {
        self.max_tool_result_inline_bytes = bytes;
        self
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
        let (message, metadata) = self
            .chat_completion_message(messages, trace_id, runtime_profile, Vec::new())
            .await?;
        let content = message_content(message)?;
        Ok((content, metadata))
    }

    async fn chat_completion_message(
        &self,
        messages: Vec<HermesChatMessage>,
        trace_id: &str,
        runtime_profile: &str,
        tools: Vec<HermesToolDefinition>,
    ) -> CoreResult<(HermesMessage, Value)> {
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
            tools,
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
        let message = body
            .choices
            .first()
            .map(|choice| choice.message.clone())
            .ok_or_else(|| {
                AgentCoreError::coded(
                    ErrorCode::InternalError,
                    "Hermes Runtime response did not include assistant message",
                )
            })?;
        let elapsed = started.elapsed().as_secs_f64();
        metrics::counter!(metric_names::RUNTIME_CALL_TOTAL, "runtime" => "hermes").increment(1);
        metrics::histogram!(metric_names::RUNTIME_DURATION_SECONDS, "runtime" => "hermes")
            .record(elapsed);
        Ok((
            message,
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

    async fn profile_chat_completion(
        &self,
        mut messages: Vec<HermesChatMessage>,
        trace_id: &str,
        runtime_profile: &str,
        contract: Option<&ProfileContract>,
        requested_tools: &[String],
    ) -> CoreResult<(String, Value)> {
        let tools = tool_definitions(contract, requested_tools);
        let started = Instant::now();
        let mut tool_results = Vec::new();
        let mut tool_audit_events = Vec::new();
        for round in 0..=self.max_tool_rounds {
            ensure_profile_budget(started, contract)?;
            let (message, mut metadata) = self
                .chat_completion_message(messages.clone(), trace_id, runtime_profile, tools.clone())
                .await?;
            ensure_profile_budget(started, contract)?;
            if !message.tool_calls.is_empty() {
                if round >= self.max_tool_rounds {
                    return Err(AgentCoreError::coded(
                        ErrorCode::Conflict,
                        "runtime profile exceeded maximum tool rounds",
                    ));
                }
                let tool_calls = message.tool_calls.clone();
                messages.push(HermesChatMessage::assistant_tool_calls(tool_calls.clone()));
                for tool_call in tool_calls {
                    ensure_profile_budget(started, contract)?;
                    let call_event =
                        runtime_tool_call_audit_event(runtime_profile, trace_id, &tool_call);
                    self.audit_sink
                        .append_runtime_event(call_event.clone())
                        .await?;
                    tool_audit_events.push(call_event);
                    let tool_call_id = tool_call.id.clone();
                    let tool_name = tool_call.function.name.clone();
                    let (result, output_schema) = match self
                        .execute_runtime_tool_call(
                            contract,
                            requested_tools,
                            runtime_profile,
                            trace_id,
                            tool_call,
                        )
                        .await
                    {
                        Ok(result) => result,
                        Err(error) => {
                            let failure_event = runtime_tool_error_audit_event(
                                runtime_profile,
                                trace_id,
                                &tool_call_id,
                                &tool_name,
                                &error,
                            );
                            self.audit_sink
                                .append_runtime_event(failure_event.clone())
                                .await?;
                            tool_audit_events.push(failure_event);
                            return Err(error);
                        }
                    };
                    ensure_profile_budget(started, contract)?;
                    let result_summary = runtime_tool_result_summary(&result, &output_schema);
                    let result_event = runtime_tool_result_audit_event(&result, &output_schema);
                    self.audit_sink
                        .append_runtime_event(result_event.clone())
                        .await?;
                    tool_audit_events.push(result_event);
                    messages.push(HermesChatMessage::tool_result(
                        result.call_id.clone(),
                        runtime_tool_result_message(&result, self.max_tool_result_inline_bytes)?,
                    ));
                    tool_results.push(result_summary);
                }
                continue;
            }

            let content = message_content(message)?;
            metadata["tool_rounds"] = json!(round);
            metadata["tool_results"] = json!(tool_results);
            metadata["tool_audit_events"] = json!(tool_audit_events);
            return Ok((content, metadata));
        }
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime profile did not produce a final answer",
        ))
    }

    async fn execute_runtime_tool_call(
        &self,
        contract: Option<&ProfileContract>,
        requested_tools: &[String],
        runtime_profile: &str,
        trace_id: &str,
        tool_call: HermesToolCall,
    ) -> CoreResult<(RuntimeToolResult, Value)> {
        let Some(contract) = contract else {
            return Err(AgentCoreError::coded(
                ErrorCode::Forbidden,
                "runtime profile requested a tool without a profile contract",
            ));
        };
        let spec = contract
            .tool_policy
            .validate_tool_call(&tool_call.function.name, requested_tools)?;
        let arguments = parse_tool_arguments(&tool_call.function.arguments)?;
        validate_json_schema_value(&spec.input_schema, &arguments)?;
        let call_id = tool_call.id.clone();
        let tool_name = tool_call.function.name.clone();
        let call = RuntimeToolCall {
            call_id: tool_call.id,
            profile_id: runtime_profile.to_string(),
            tool_name: tool_call.function.name,
            arguments,
            trace_id: trace_id.to_string(),
            metadata: json!({
                "runtime": "hermes",
                "tool_call_type": tool_call.kind,
            }),
        };
        let mut result = self.tool_executor.execute_tool(call, spec.clone()).await?;
        result.call_id = call_id;
        result.profile_id = runtime_profile.to_string();
        result.tool_name = tool_name;
        if !result.metadata.is_object() {
            result.metadata = json!({});
        }
        result.metadata["trace_id"] = json!(trace_id);
        if result.output_ref.is_none() {
            result.output_ref = Some(format!("runtime://tool-results/{}", result.call_id));
        }
        if spec.output_ref_required && result.output_ref.is_none() {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "runtime tool result did not include output_ref",
            ));
        }
        validate_json_schema_value(&spec.output_schema, &result.output)?;
        Ok((result, spec.output_schema))
    }

    async fn chat_completion_stream(
        &self,
        messages: Vec<HermesChatMessage>,
        trace_id: &str,
        runtime_profile: &str,
    ) -> CoreResult<(String, Value, Vec<RuntimeStreamEvent>)> {
        let started = Instant::now();
        let model = self.config.model_for_profile(runtime_profile);
        let request = HermesChatCompletionRequest {
            model: model.clone(),
            messages,
            stream: true,
            metadata: json!({
                "trace_id": trace_id,
                "runtime_profile": runtime_profile,
                "agent_platform_phase": "runtime-streaming",
                "read_only": true,
            }),
            tools: Vec::new(),
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
            return Err(AgentCoreError::coded(
                if status == ReqwestStatusCode::TOO_MANY_REQUESTS {
                    ErrorCode::RateLimited
                } else {
                    ErrorCode::InternalError
                },
                format!("Hermes Runtime stream returned HTTP {}", status.as_u16()),
            ));
        }

        let mut events = vec![RuntimeStreamEvent {
            sequence: 0,
            event_type: RuntimeStreamEventType::Started,
            profile_id: runtime_profile.to_string(),
            trace_id: trace_id.to_string(),
            run_id: None,
            session_id: None,
            schema_version: None,
            content_delta: None,
            output: None,
            error_code: None,
            metadata: json!({
                "runtime": "hermes",
                "hermes_model": model,
            }),
        }];
        let mut content = String::new();
        let mut sequence = 1;
        let mut pending = String::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest_error)?;
            pending.push_str(&String::from_utf8_lossy(&chunk));
            process_sse_lines(
                &mut pending,
                runtime_profile,
                trace_id,
                &model,
                &mut sequence,
                &mut content,
                &mut events,
                false,
            )?;
        }
        process_sse_lines(
            &mut pending,
            runtime_profile,
            trace_id,
            &model,
            &mut sequence,
            &mut content,
            &mut events,
            true,
        )?;
        let content = content.trim().to_string();
        if content.is_empty() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "Hermes Runtime stream did not include assistant content",
            ));
        }
        let elapsed = started.elapsed().as_secs_f64();
        metrics::counter!(metric_names::RUNTIME_CALL_TOTAL, "runtime" => "hermes_stream")
            .increment(1);
        metrics::histogram!(metric_names::RUNTIME_DURATION_SECONDS, "runtime" => "hermes_stream")
            .record(elapsed);
        Ok((
            content,
            json!({
                "runtime": "hermes",
                "runtime_profile": runtime_profile,
                "hermes_model": model,
                "trace_id": trace_id,
                "read_only": true,
                "streaming": true,
                "duration_ms": (elapsed * 1000.0).round() as i64,
            }),
            events,
        ))
    }
}

#[async_trait]
impl RuntimeClient for HermesRuntimeClient {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        self.ensure_read_only_runtime(input.run.external_action_mode)?;
        let runtime_profile = runtime_profile_for_run(&input, "agent-platform-hermes");
        let contract = input
            .profile_contract
            .clone()
            .or_else(|| self.registry.get(&runtime_profile));
        validate_contract_input(
            contract.as_ref(),
            &runtime_run_contract_payload(&input, &runtime_profile),
            &input.requested_tools,
            &runtime_profile,
        )?;
        let messages = run_messages(&input, &runtime_profile);
        let (content, metadata) = self
            .chat_completion(messages, &input.trace_id, &runtime_profile)
            .await?;
        finalize_runtime_output(
            RuntimeOutput {
                result_summary: content,
                result_ref: Some(format!("hermes://runs/{}", input.run.id)),
                messages: Vec::new(),
                metadata,
            },
            contract.as_ref(),
            &input.requested_tools,
            input.runtime_step.as_ref(),
        )
    }

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        let runtime_profile = runtime_profile_for_session(&input, &input.agent_id);
        let contract = input
            .profile_contract
            .clone()
            .or_else(|| self.registry.get(&runtime_profile));
        validate_contract_input(
            contract.as_ref(),
            &runtime_session_contract_payload(&input, &runtime_profile),
            &input.requested_tools,
            &runtime_profile,
        )?;
        let messages = session_messages(&input, &runtime_profile);
        let (content, metadata) = self
            .chat_completion(messages, &input.trace_id, &runtime_profile)
            .await?;
        let assistant_message = AgentSessionMessage::new(
            input.session_id.clone(),
            input.message.sequence + 1,
            MessageRole::Assistant,
            Some(content.clone()),
            input.message.run_id.clone(),
            input.trace_id.clone(),
        );
        finalize_runtime_output(
            RuntimeOutput {
                result_summary: content,
                result_ref: Some(format!("hermes://sessions/{}", input.session_id)),
                messages: vec![assistant_message],
                metadata,
            },
            contract.as_ref(),
            &input.requested_tools,
            input.runtime_step.as_ref(),
        )
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let contract = input
            .profile_contract
            .clone()
            .or_else(|| self.registry.get(&input.profile_id));
        validate_contract_input(
            contract.as_ref(),
            &runtime_profile_contract_payload(&input),
            &input.requested_tools,
            &input.profile_id,
        )?;
        let mut effective_contract = contract.clone();
        let mut requested_tools = input.requested_tools.clone();
        if let (Some(contract), Some(step)) =
            (effective_contract.as_mut(), input.runtime_step.as_ref())
        {
            if step.tool_policy.has_rules() {
                requested_tools = step
                    .tool_policy
                    .effective_tools_for_request(&input.requested_tools);
                step.tool_policy
                    .validate_requested_tools(&requested_tools)?;
                contract.tool_policy = step.tool_policy.clone();
            }
            if !json_schema_is_empty(&step.output_contract) {
                contract.output_schema = step.output_contract.clone();
            }
        }
        let messages = input
            .messages
            .iter()
            .map(runtime_profile_message_to_hermes)
            .collect();
        let (content, metadata) = self
            .profile_chat_completion(
                messages,
                &input.trace_id,
                &input.profile_id,
                effective_contract.as_ref(),
                &requested_tools,
            )
            .await?;
        finalize_runtime_output(
            RuntimeOutput {
                result_summary: content,
                result_ref: Some(format!(
                    "hermes://profiles/{}/{}",
                    input.profile_id, input.trace_id
                )),
                messages: Vec::new(),
                metadata,
            },
            effective_contract.as_ref(),
            &requested_tools,
            input.runtime_step.as_ref(),
        )
    }

    async fn stream_run(&self, input: RuntimeRunInput) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let runtime_profile = runtime_profile_for_run(&input, "agent-platform-hermes");
        let trace_id = input.trace_id.clone();
        let run_id = input.run.id.clone();
        let error_schema_version = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.version.version.clone());
        let error_profile = runtime_profile.clone();
        let error_trace_id = trace_id.clone();
        let result: CoreResult<Vec<RuntimeStreamEvent>> = async {
            self.ensure_read_only_runtime(input.run.external_action_mode)?;
            let contract = input
                .profile_contract
                .clone()
                .or_else(|| self.registry.get(&runtime_profile));
            validate_contract_input(
                contract.as_ref(),
                &runtime_run_contract_payload(&input, &runtime_profile),
                &input.requested_tools,
                &runtime_profile,
            )?;
            let messages = run_messages(&input, &runtime_profile);
            let (content, metadata, mut events) = self
                .chat_completion_stream(messages, &trace_id, &runtime_profile)
                .await?;
            let output = finalize_runtime_output(
                RuntimeOutput {
                    result_summary: content,
                    result_ref: Some(format!("hermes://runs/{}", input.run.id)),
                    messages: Vec::new(),
                    metadata,
                },
                contract.as_ref(),
                &input.requested_tools,
                input.runtime_step.as_ref(),
            )?;
            let schema_version = contract
                .as_ref()
                .map(|contract| contract.version.version.as_str());
            push_schema_partial_event(&mut events, &runtime_profile, &trace_id, &output);
            push_final_output_event(&mut events, runtime_profile, trace_id, output);
            annotate_stream_events(&mut events, Some(&run_id), None, schema_version);
            Ok(events)
        }
        .await;
        Ok(result.unwrap_or_else(|error| {
            let mut event = runtime_stream_error_event(0, error_profile, error_trace_id, &error);
            event.run_id = Some(run_id);
            event.schema_version = error_schema_version;
            vec![event]
        }))
    }

    async fn stream_session_message(
        &self,
        input: RuntimeSessionInput,
    ) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let runtime_profile = runtime_profile_for_session(&input, &input.agent_id);
        let trace_id = input.trace_id.clone();
        let session_id = input.session_id.clone();
        let error_schema_version = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.version.version.clone());
        let error_profile = runtime_profile.clone();
        let error_trace_id = trace_id.clone();
        let result: CoreResult<Vec<RuntimeStreamEvent>> = async {
            let contract = input
                .profile_contract
                .clone()
                .or_else(|| self.registry.get(&runtime_profile));
            validate_contract_input(
                contract.as_ref(),
                &runtime_session_contract_payload(&input, &runtime_profile),
                &input.requested_tools,
                &runtime_profile,
            )?;
            let messages = session_messages(&input, &runtime_profile);
            let (content, metadata, mut events) = self
                .chat_completion_stream(messages, &trace_id, &runtime_profile)
                .await?;
            let assistant_message = AgentSessionMessage::new(
                input.session_id.clone(),
                input.message.sequence + 1,
                MessageRole::Assistant,
                Some(content.clone()),
                input.message.run_id.clone(),
                trace_id.clone(),
            );
            let output = finalize_runtime_output(
                RuntimeOutput {
                    result_summary: content,
                    result_ref: Some(format!("hermes://sessions/{}", input.session_id)),
                    messages: vec![assistant_message],
                    metadata,
                },
                contract.as_ref(),
                &input.requested_tools,
                input.runtime_step.as_ref(),
            )?;
            let schema_version = contract
                .as_ref()
                .map(|contract| contract.version.version.as_str());
            push_schema_partial_event(&mut events, &runtime_profile, &trace_id, &output);
            push_final_output_event(&mut events, runtime_profile, trace_id, output);
            annotate_stream_events(&mut events, None, Some(&session_id), schema_version);
            Ok(events)
        }
        .await;
        Ok(result.unwrap_or_else(|error| {
            let mut event = runtime_stream_error_event(0, error_profile, error_trace_id, &error);
            event.session_id = Some(session_id);
            event.schema_version = error_schema_version;
            vec![event]
        }))
    }

    async fn stream_profile_step(
        &self,
        input: RuntimeProfileInput,
    ) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let error_profile = input.profile_id.clone();
        let error_trace_id = input.trace_id.clone();
        let error_schema_version = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.version.version.clone());
        let result: CoreResult<Vec<RuntimeStreamEvent>> = async {
            let contract = input
                .profile_contract
                .clone()
                .or_else(|| self.registry.get(&input.profile_id));
            validate_contract_input(
                contract.as_ref(),
                &runtime_profile_contract_payload(&input),
                &input.requested_tools,
                &input.profile_id,
            )?;
            if contract.as_ref().is_some_and(|contract| {
                !contract
                    .tool_policy
                    .effective_tools_for_request(&input.requested_tools)
                    .is_empty()
            }) {
                let profile_id = input.profile_id.clone();
                let trace_id = input.trace_id.clone();
                let output = self.execute_profile_step(input).await?;
                let mut events = vec![RuntimeStreamEvent {
                    sequence: 0,
                    event_type: RuntimeStreamEventType::Started,
                    profile_id: profile_id.clone(),
                    trace_id: trace_id.clone(),
                    run_id: None,
                    session_id: None,
                    schema_version: None,
                    content_delta: None,
                    output: None,
                    error_code: None,
                    metadata: json!({"runtime": "hermes", "tool_loop": true}),
                }];
                push_tool_progress_events(&mut events, &profile_id, &trace_id, &output);
                push_schema_partial_event(&mut events, &profile_id, &trace_id, &output);
                push_final_output_event(&mut events, profile_id, trace_id, output);
                let schema_version = contract
                    .as_ref()
                    .map(|contract| contract.version.version.as_str());
                annotate_stream_events(&mut events, None, None, schema_version);
                return Ok(events);
            }
            let messages = input
                .messages
                .iter()
                .map(runtime_profile_message_to_hermes)
                .collect();
            let started = Instant::now();
            ensure_profile_budget(started, contract.as_ref())?;
            let (content, metadata, mut events) = self
                .chat_completion_stream(messages, &input.trace_id, &input.profile_id)
                .await?;
            ensure_profile_budget(started, contract.as_ref())?;
            let output = finalize_runtime_output(
                RuntimeOutput {
                    result_summary: content,
                    result_ref: Some(format!(
                        "hermes://profiles/{}/{}",
                        input.profile_id, input.trace_id
                    )),
                    messages: Vec::new(),
                    metadata,
                },
                contract.as_ref(),
                &input.requested_tools,
                input.runtime_step.as_ref(),
            )?;
            let schema_version = contract
                .as_ref()
                .map(|contract| contract.version.version.as_str());
            push_schema_partial_event(&mut events, &input.profile_id, &input.trace_id, &output);
            push_final_output_event(&mut events, input.profile_id, input.trace_id, output);
            annotate_stream_events(&mut events, None, None, schema_version);
            Ok(events)
        }
        .await;
        Ok(result.unwrap_or_else(|error| {
            let mut event = runtime_stream_error_event(0, error_profile, error_trace_id, &error);
            event.schema_version = error_schema_version;
            vec![event]
        }))
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

#[derive(Debug, Clone)]
pub struct HttpRuntimeToolExecutorConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

impl HttpRuntimeToolExecutorConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("AGENT_RUNTIME_TOOL_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        Some(Self {
            base_url: trim_base_url(base_url),
            api_key: std::env::var("AGENT_RUNTIME_TOOL_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
            timeout: Duration::from_secs(env_u64("AGENT_RUNTIME_TOOL_TIMEOUT_SECONDS", 10)),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HttpRuntimeToolExecutor {
    config: HttpRuntimeToolExecutorConfig,
    client: reqwest::Client,
    tool_call_url: Url,
}

impl HttpRuntimeToolExecutor {
    pub fn new(config: HttpRuntimeToolExecutorConfig) -> CoreResult<Self> {
        let tool_call_url = Url::parse(&format!("{}/tool-calls", config.base_url))
            .map_err(|_| runtime_failure("invalid runtime tool executor URL"))?;
        Ok(Self {
            config,
            client: reqwest::Client::new(),
            tool_call_url,
        })
    }
}

#[async_trait]
impl RuntimeToolExecutor for HttpRuntimeToolExecutor {
    async fn execute_tool(
        &self,
        call: RuntimeToolCall,
        _spec: RuntimeToolSpec,
    ) -> CoreResult<RuntimeToolResult> {
        let response = post_json(
            &self.client,
            self.tool_call_url.clone(),
            self.config.api_key.as_deref(),
            &call.trace_id,
            self.config.timeout,
            &call,
        )
        .await?;
        if !response.status().is_success() {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                format!(
                    "runtime tool executor returned HTTP {}",
                    response.status().as_u16()
                ),
            ));
        }
        response.json::<RuntimeToolResult>().await.map_err(|_| {
            AgentCoreError::coded(
                ErrorCode::InternalError,
                "runtime tool executor response was malformed",
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

fn runtime_profile_for_run(input: &RuntimeRunInput, fallback: &str) -> String {
    input
        .profile_contract
        .as_ref()
        .map(|contract| contract.profile_id.clone())
        .or_else(|| {
            input
                .agent
                .as_ref()
                .map(|agent| agent.hermes_profile.clone())
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn runtime_profile_for_session(input: &RuntimeSessionInput, fallback: &str) -> String {
    input
        .profile_contract
        .as_ref()
        .map(|contract| contract.profile_id.clone())
        .or_else(|| {
            input
                .agent
                .as_ref()
                .map(|agent| agent.hermes_profile.clone())
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn validate_contract_input(
    contract: Option<&ProfileContract>,
    input: &Value,
    requested_tools: &[String],
    runtime_profile: &str,
) -> CoreResult<()> {
    let Some(contract) = contract else {
        return Ok(());
    };
    if contract.profile_id != runtime_profile {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime profile contract does not match requested profile",
        ));
    }
    contract
        .tool_policy
        .validate_requested_tools(requested_tools)?;
    validate_contract_context_budget(contract, input)?;
    validate_contract_safety_policy(contract, input)?;
    validate_json_schema_value(&contract.input_schema, input)
}

fn validate_contract_context_budget(contract: &ProfileContract, input: &Value) -> CoreResult<()> {
    let Some(max_context_messages) = contract.max_context_messages else {
        return Ok(());
    };
    if runtime_context_message_count(input) > max_context_messages {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime profile exceeded max_context_messages",
        ));
    }
    Ok(())
}

fn runtime_context_message_count(input: &Value) -> usize {
    let profile_messages = input
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let context_messages = input
        .get("context")
        .and_then(|context| context.get("recent_messages"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let current_message = usize::from(input.get("message").is_some());
    profile_messages + context_messages + current_message
}

fn validate_contract_safety_policy(contract: &ProfileContract, input: &Value) -> CoreResult<()> {
    if json_schema_is_empty(&contract.safety_policy) || contract.safety_policy.is_null() {
        return Ok(());
    }
    let Some(policy) = contract.safety_policy.as_object() else {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime profile safety policy was invalid",
        ));
    };
    for field in policy.keys() {
        if !matches!(field.as_str(), "deny_message_roles" | "max_message_bytes") {
            return Err(invalid_safety_policy(field));
        }
    }
    if let Some(denied_roles) = policy.get("deny_message_roles") {
        let denied_roles = denied_roles
            .as_array()
            .ok_or_else(|| invalid_safety_policy("deny_message_roles"))?;
        for denied_role in denied_roles {
            let denied_role = denied_role
                .as_str()
                .ok_or_else(|| invalid_safety_policy("deny_message_roles"))?;
            if runtime_message_values(input).into_iter().any(|message| {
                message
                    .get("role")
                    .and_then(Value::as_str)
                    .is_some_and(|role| role == denied_role)
            }) {
                return Err(AgentCoreError::coded(
                    ErrorCode::Conflict,
                    "runtime profile safety policy rejected message role",
                ));
            }
        }
    }
    if let Some(max_message_bytes) = policy.get("max_message_bytes") {
        let max_message_bytes = max_message_bytes
            .as_u64()
            .ok_or_else(|| invalid_safety_policy("max_message_bytes"))?
            as usize;
        if runtime_message_values(input)
            .into_iter()
            .filter_map(runtime_message_content)
            .any(|content| content.len() > max_message_bytes)
        {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "runtime profile safety policy rejected oversized message",
            ));
        }
    }
    Ok(())
}

fn invalid_safety_policy(field: &str) -> AgentCoreError {
    AgentCoreError::coded(
        ErrorCode::Conflict,
        format!("runtime profile safety policy field {field} was invalid"),
    )
}

fn runtime_message_values(input: &Value) -> Vec<&Value> {
    let mut messages = Vec::new();
    if let Some(profile_messages) = input.get("messages").and_then(Value::as_array) {
        messages.extend(profile_messages);
    }
    if let Some(current_message) = input.get("message") {
        messages.push(current_message);
    }
    if let Some(context_messages) = input
        .get("context")
        .and_then(|context| context.get("recent_messages"))
        .and_then(Value::as_array)
    {
        messages.extend(context_messages);
    }
    messages
}

fn runtime_message_content(message: &Value) -> Option<&str> {
    message
        .get("content")
        .or_else(|| message.get("content_summary"))
        .and_then(Value::as_str)
}

fn finalize_runtime_output(
    mut output: RuntimeOutput,
    contract: Option<&ProfileContract>,
    requested_tools: &[String],
    runtime_step: Option<&agent_core::RuntimeStep>,
) -> CoreResult<RuntimeOutput> {
    if let Some(contract) = contract {
        if !output.metadata.is_object() {
            output.metadata = json!({});
        }
        output.metadata["profile_id"] = json!(&contract.profile_id);
        output.metadata["schema_version"] = json!(&contract.version.version);
        let tool_policy = runtime_step
            .filter(|step| step.tool_policy.has_rules())
            .map(|step| &step.tool_policy)
            .unwrap_or(&contract.tool_policy);
        output.metadata["effective_tool_set"] =
            json!(tool_policy.effective_tools_for_request(requested_tools));
        output.metadata["requested_tools"] = json!(requested_tools);
        if let Some(step) = runtime_step {
            let mut step_value = serde_json::to_value(step)
                .map_err(|_| runtime_failure("runtime step metadata was not serializable"))?;
            step_value["status"] = json!("completed");
            output.metadata["runtime_step"] = step_value;
        }
        let value = serde_json::to_value(&output)
            .map_err(|_| runtime_failure("runtime output was not serializable"))?;
        let output_schema = runtime_step
            .filter(|step| !json_schema_is_empty(&step.output_contract))
            .map(|step| &step.output_contract)
            .unwrap_or(&contract.output_schema);
        validate_json_schema_value(output_schema, &value)?;
    }
    Ok(output)
}

fn json_schema_is_empty(schema: &Value) -> bool {
    schema.as_object().is_none_or(serde_json::Map::is_empty)
}

fn push_tool_progress_events(
    events: &mut Vec<RuntimeStreamEvent>,
    profile_id: &str,
    trace_id: &str,
    output: &RuntimeOutput,
) {
    let Some(tool_events) = output
        .metadata
        .get("tool_audit_events")
        .and_then(Value::as_array)
    else {
        return;
    };
    for tool_event in tool_events {
        events.push(RuntimeStreamEvent::tool_progress(
            events.len() as u64,
            profile_id,
            trace_id,
            json!({
                "runtime": "hermes",
                "tool_event": tool_event,
            }),
        ));
    }
}

fn push_schema_partial_event(
    events: &mut Vec<RuntimeStreamEvent>,
    profile_id: &str,
    trace_id: &str,
    output: &RuntimeOutput,
) {
    events.push(RuntimeStreamEvent::schema_partial(
        events.len() as u64,
        profile_id,
        trace_id,
        json!({
            "schema_validated": true,
            "profile_id": output.metadata.get("profile_id"),
            "schema_version": output.metadata.get("schema_version"),
            "effective_tool_set": output.metadata.get("effective_tool_set"),
            "runtime_step": output.metadata.get("runtime_step"),
            "result_ref": &output.result_ref,
        }),
    ));
}

fn push_final_output_event(
    events: &mut Vec<RuntimeStreamEvent>,
    profile_id: impl Into<String>,
    trace_id: impl Into<String>,
    output: RuntimeOutput,
) {
    let mut event = RuntimeStreamEvent::final_output(profile_id, trace_id, output);
    event.sequence = events.len() as u64;
    events.push(event);
}

fn annotate_stream_events(
    events: &mut [RuntimeStreamEvent],
    run_id: Option<&str>,
    session_id: Option<&str>,
    schema_version: Option<&str>,
) {
    for event in events {
        if event.run_id.is_none() {
            event.run_id = run_id.map(ToString::to_string);
        }
        if event.session_id.is_none() {
            event.session_id = session_id.map(ToString::to_string);
        }
        if event.schema_version.is_none() {
            event.schema_version = schema_version.map(ToString::to_string);
        }
    }
}

fn runtime_run_contract_payload(input: &RuntimeRunInput, runtime_profile: &str) -> Value {
    json!({
        "kind": "run",
        "profile_id": runtime_profile,
        "run": &input.run,
        "agent": &input.agent,
        "context": &input.context,
        "snapshot": &input.snapshot,
        "runtime_step": &input.runtime_step,
        "requested_tools": &input.requested_tools,
        "trace_id": &input.trace_id,
    })
}

fn runtime_session_contract_payload(input: &RuntimeSessionInput, runtime_profile: &str) -> Value {
    json!({
        "kind": "session_message",
        "profile_id": runtime_profile,
        "session_id": &input.session_id,
        "agent_id": &input.agent_id,
        "agent": &input.agent,
        "message": &input.message,
        "context": &input.context,
        "snapshot": &input.snapshot,
        "runtime_step": &input.runtime_step,
        "requested_tools": &input.requested_tools,
        "trace_id": &input.trace_id,
    })
}

fn runtime_profile_contract_payload(input: &RuntimeProfileInput) -> Value {
    json!({
        "kind": "profile_step",
        "profile_id": &input.profile_id,
        "messages": &input.messages,
        "metadata": &input.metadata,
        "runtime_step": &input.runtime_step,
        "requested_tools": &input.requested_tools,
        "trace_id": &input.trace_id,
    })
}

fn runtime_profile_message_to_hermes(message: &RuntimeProfileMessage) -> HermesChatMessage {
    HermesChatMessage::new(message.role.clone(), message.content.clone())
}

fn message_content(message: HermesMessage) -> CoreResult<String> {
    message
        .content
        .as_deref()
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::InternalError,
                "Hermes Runtime response did not include assistant content",
            )
        })
}

fn ensure_profile_budget(started: Instant, contract: Option<&ProfileContract>) -> CoreResult<()> {
    let Some(max_runtime_seconds) = contract.and_then(|contract| contract.max_runtime_seconds)
    else {
        return Ok(());
    };
    if started.elapsed() > Duration::from_secs(max_runtime_seconds) {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime profile exceeded max_runtime_seconds",
        ));
    }
    Ok(())
}

fn tool_definitions(
    contract: Option<&ProfileContract>,
    requested_tools: &[String],
) -> Vec<HermesToolDefinition> {
    contract
        .map(|contract| {
            contract
                .tool_policy
                .effective_tool_specs_for_request(requested_tools)
                .into_iter()
                .map(|spec| HermesToolDefinition {
                    kind: "function".to_string(),
                    function: HermesToolFunctionDefinition {
                        name: spec.name,
                        description: spec.description,
                        parameters: spec.input_schema,
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_tool_arguments(arguments: &str) -> CoreResult<Value> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(arguments).map_err(|_| {
        AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime tool call arguments were malformed",
        )
    })
}

fn runtime_tool_call_audit_event(
    runtime_profile: &str,
    trace_id: &str,
    tool_call: &HermesToolCall,
) -> Value {
    json!({
        "event": "runtime_tool_call",
        "call_id": &tool_call.id,
        "profile_id": runtime_profile,
        "tool_name": &tool_call.function.name,
        "trace_id": trace_id,
    })
}

fn runtime_tool_result_audit_event(result: &RuntimeToolResult, output_schema: &Value) -> Value {
    let mut value = runtime_tool_result_summary(result, output_schema);
    value["event"] = json!("runtime_tool_result");
    value
}

fn runtime_tool_error_audit_event(
    runtime_profile: &str,
    trace_id: &str,
    call_id: &str,
    tool_name: &str,
    error: &AgentCoreError,
) -> Value {
    json!({
        "event": "runtime_tool_error",
        "call_id": call_id,
        "profile_id": runtime_profile,
        "tool_name": tool_name,
        "trace_id": trace_id,
        "error_code": error.code().as_str(),
    })
}

fn runtime_tool_result_summary(result: &RuntimeToolResult, output_schema: &Value) -> Value {
    json!({
        "call_id": &result.call_id,
        "profile_id": &result.profile_id,
        "tool_name": &result.tool_name,
        "output_schema": output_schema,
        "output_ref": &result.output_ref,
        "output_summary": summarize_json(&result.output),
        "trace_id": result.metadata.get("trace_id").and_then(Value::as_str),
    })
}

fn runtime_tool_result_message(
    result: &RuntimeToolResult,
    inline_limit: usize,
) -> CoreResult<String> {
    let output_bytes = serde_json::to_vec(&result.output)
        .map_err(|_| runtime_failure("runtime tool result was not serializable"))?
        .len();
    let content = if output_bytes <= inline_limit {
        json!({
            "call_id": &result.call_id,
            "tool_name": &result.tool_name,
            "output_ref": &result.output_ref,
            "output": &result.output,
        })
    } else {
        json!({
            "call_id": &result.call_id,
            "tool_name": &result.tool_name,
            "output_ref": &result.output_ref,
            "output_summary": summarize_json(&result.output),
            "output_omitted": true,
        })
    };
    serde_json::to_string(&content)
        .map_err(|_| runtime_failure("runtime tool result was not serializable"))
}

#[allow(clippy::too_many_arguments)]
fn process_sse_lines(
    pending: &mut String,
    runtime_profile: &str,
    trace_id: &str,
    model: &str,
    sequence: &mut u64,
    content: &mut String,
    events: &mut Vec<RuntimeStreamEvent>,
    flush: bool,
) -> CoreResult<()> {
    loop {
        let Some(index) = pending.find('\n') else {
            break;
        };
        let line = pending.drain(..=index).collect::<String>();
        process_sse_line(
            line.trim(),
            runtime_profile,
            trace_id,
            model,
            sequence,
            content,
            events,
        )?;
    }
    if flush && !pending.trim().is_empty() {
        let line = std::mem::take(pending);
        process_sse_line(
            line.trim(),
            runtime_profile,
            trace_id,
            model,
            sequence,
            content,
            events,
        )?;
    }
    Ok(())
}

fn process_sse_line(
    line: &str,
    runtime_profile: &str,
    trace_id: &str,
    model: &str,
    sequence: &mut u64,
    content: &mut String,
    events: &mut Vec<RuntimeStreamEvent>,
) -> CoreResult<()> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(());
    };
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let value = serde_json::from_str::<Value>(data)
        .map_err(|_| runtime_failure("Hermes Runtime stream event was malformed"))?;
    let Some(delta) = value
        .pointer("/choices/0/delta/content")
        .or_else(|| value.pointer("/choices/0/message/content"))
        .and_then(Value::as_str)
        .filter(|delta| !delta.is_empty())
    else {
        return Ok(());
    };
    content.push_str(delta);
    events.push(RuntimeStreamEvent {
        sequence: *sequence,
        event_type: RuntimeStreamEventType::Delta,
        profile_id: runtime_profile.to_string(),
        trace_id: trace_id.to_string(),
        run_id: None,
        session_id: None,
        schema_version: None,
        content_delta: Some(delta.to_string()),
        output: None,
        error_code: None,
        metadata: json!({
            "runtime": "hermes",
            "hermes_model": model,
        }),
    });
    *sequence += 1;
    Ok(())
}

fn summarize_json(value: &Value) -> String {
    match value {
        Value::Object(map) => format!("object_keys_len:{}", map.len()),
        Value::Array(items) => format!("array_len:{}", items.len()),
        Value::String(value) => format!("string_len:{}", value.chars().count()),
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "number".to_string(),
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tools: Vec<HermesToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesChatMessage {
    role: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<HermesToolCall>,
}

impl HermesChatMessage {
    fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    fn assistant_tool_calls(tool_calls: Vec<HermesToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: String::new(),
            tool_call_id: None,
            tool_calls,
        }
    }

    fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_call_id: Some(call_id.into()),
            tool_calls: Vec::new(),
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<HermesToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesToolCall {
    id: String,
    #[serde(default, rename = "type")]
    kind: String,
    function: HermesToolFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesToolFunctionCall {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
struct HermesToolDefinition {
    #[serde(rename = "type")]
    kind: String,
    function: HermesToolFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct HermesToolFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

fn run_messages(input: &RuntimeRunInput, runtime_profile: &str) -> Vec<HermesChatMessage> {
    let mut messages = vec![HermesChatMessage::new(
        "system",
        format!(
            "You are executing an Agent Platform P1 read-only run. Runtime profile: {runtime_profile}. Never perform external writes or request write credentials."
        ),
    )];
    if let Some(context) = &input.context {
        if let Some(summary) = &context.context_summary {
            messages.push(HermesChatMessage::new(
                "system",
                format!("Session context summary: {summary}"),
            ));
        }
        for message in &context.recent_messages {
            messages.push(HermesChatMessage::new(
                message.role.to_string(),
                message.content_summary.clone(),
            ));
        }
    }
    if let Some(snapshot) = &input.snapshot {
        messages.push(HermesChatMessage::new(
            "system",
            format!(
                "Read-only connector snapshot summary: {}",
                summarize_json(snapshot)
            ),
        ));
    }
    messages.push(HermesChatMessage::new(
        "user",
        format!(
            "Run {} trigger={} target_resource={} risk={} external_action_mode={}. Provide a concise read-only result.",
            input.run.id,
            input.run.trigger_type,
            input.run.target_resource,
            input.run.risk_level,
            input.run.external_action_mode
        ),
    ));
    messages
}

fn session_messages(input: &RuntimeSessionInput, runtime_profile: &str) -> Vec<HermesChatMessage> {
    let mut messages = vec![HermesChatMessage::new(
        "system",
        format!(
            "You are in an Agent Platform P1 read-only session. Runtime profile: {runtime_profile}. Do not request or use write credentials."
        ),
    )];
    if let Some(summary) = &input.context.context_summary {
        messages.push(HermesChatMessage::new(
            "system",
            format!("Session context summary: {summary}"),
        ));
    }
    if let Some(snapshot) = &input.snapshot {
        messages.push(HermesChatMessage::new(
            "system",
            format!(
                "Read-only connector snapshot summary: {}",
                summarize_json(snapshot)
            ),
        ));
    }
    for message in &input.context.recent_messages {
        if message.role == input.message.role
            && Some(message.content_summary.as_str()) == input.message.content_summary.as_deref()
            && message.external_message_id == input.message.external_message_id
        {
            continue;
        }
        messages.push(HermesChatMessage::new(
            message.role.to_string(),
            message.content_summary.clone(),
        ));
    }
    messages.push(HermesChatMessage::new(
        input.message.role.to_string(),
        input.message.content_summary.clone().unwrap_or_default(),
    ));
    messages
}

pub fn runtime_error(error: impl std::fmt::Display) -> agent_core::AgentCoreError {
    runtime_failure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        AgentInstance, AgentRun, CredentialLeaseStatus, ExternalActionPlan, ProfileContract,
        RiskLevel, RuntimeProfileInput, RuntimeProfileMessage, RuntimeStep,
        RuntimeStepFailurePolicy, RuntimeStepPlan, RuntimeStepPlanInput, RuntimeStepPlanOwner,
        RuntimeToolCapability, RuntimeToolPolicy, RuntimeToolSpec, TriggerType, new_trace_id,
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

    type SeenWriteConnectorInput = (Arc<Mutex<Option<String>>>, Arc<Mutex<Option<Value>>>);

    #[test]
    fn summarize_json_omits_values_and_object_keys() {
        assert_eq!(
            summarize_json(&json!({"SECRET_TOOL_OUTPUT": "SECRET_TOOL_VALUE"})),
            "object_keys_len:1"
        );
        assert_eq!(
            summarize_json(&json!("SECRET_TOOL_OUTPUT")),
            "string_len:18"
        );
        assert_eq!(summarize_json(&json!(42)), "number");
        assert_eq!(summarize_json(&json!(true)), "bool");
    }

    #[derive(Debug, Default)]
    struct RawProfileRuntimeClient;

    #[async_trait]
    impl RuntimeClient for RawProfileRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "raw profile runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "raw profile runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            Ok(RuntimeOutput {
                result_summary: format!("raw profile step {}", input.profile_id),
                result_ref: Some(format!("result://raw/{}", input.profile_id)),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                }),
            })
        }
    }

    #[derive(Debug, Default)]
    struct LeakyMetadataToolExecutor;

    #[async_trait]
    impl RuntimeToolExecutor for LeakyMetadataToolExecutor {
        async fn execute_tool(
            &self,
            call: RuntimeToolCall,
            _spec: RuntimeToolSpec,
        ) -> CoreResult<RuntimeToolResult> {
            Ok(RuntimeToolResult {
                call_id: "spoofed-call".to_string(),
                profile_id: "spoofed-profile".to_string(),
                tool_name: "spoofed-tool".to_string(),
                output_ref: Some("runtime://tool-results/leaky".to_string()),
                output: json!({"SECRET_TOOL_OUTPUT": "SECRET_TOOL_VALUE"}),
                metadata: json!({
                    "trace_id": call.trace_id,
                    "secret": "SECRET_TOOL_METADATA",
                    "payload": {"raw": "SECRET_TOOL_PAYLOAD"}
                }),
            })
        }
    }

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
            profile_contract: None,
            runtime_step: None,
            requested_tools: Vec::new(),
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
                profile_contract: None,
                runtime_step: None,
                requested_tools: Vec::new(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn minimal_runtime_profile_step_validates_contract_and_streams_final_event() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.input_schema = json!({
            "type": "object",
            "required": ["kind", "profile_id", "messages"],
            "properties": {
                "kind": {"type": "string"},
                "profile_id": {"type": "string"},
                "messages": {"type": "array", "minItems": 1}
            }
        });
        contract.output_schema = json!({
            "type": "object",
            "required": ["result_summary", "metadata"],
            "properties": {
                "result_summary": {"type": "string"},
                "metadata": {
                    "type": "object",
                    "required": ["profile_id", "schema_version", "effective_tool_set"],
                    "properties": {
                        "profile_id": {"type": "string"},
                        "schema_version": {"type": "string"},
                        "effective_tool_set": {"type": "array"}
                    }
                }
            }
        });
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.read".to_string()]);
        let runtime = MinimalRuntimeClient::default();
        let input = RuntimeProfileInput {
            profile_id: "test-profile".to_string(),
            messages: vec![RuntimeProfileMessage::new("user", "hello")],
            metadata: json!({}),
            profile_contract: Some(contract),
            runtime_step: None,
            requested_tools: vec!["tool.read".to_string()],
            trace_id: new_trace_id(),
        };

        let output = runtime.execute_profile_step(input.clone()).await.unwrap();
        assert_eq!(output.metadata["profile_id"], "test-profile");
        assert_eq!(output.metadata["schema_version"], "v1");
        assert_eq!(output.metadata["effective_tool_set"][0], "tool.read");

        let events = runtime.stream_profile_step(input).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.as_str(), "final");
        assert_eq!(events[0].schema_version.as_deref(), Some("v1"));
        assert!(events[0].output.is_some());
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_denied_profile_tool() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.read".to_string()]);
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "hello")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["direct_external_write".to_string()],
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Forbidden,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_unallowed_profile_tool() {
        let contract = ProfileContract::new("test-profile", "v1");
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "hello")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.read".to_string()],
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Forbidden,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_invalid_profile_input_schema() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.input_schema = json!({
            "type": "object",
            "required": ["missing_required_field"]
        });
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "hello")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_profile_context_over_budget() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.max_context_messages = Some(1);
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![
                    RuntimeProfileMessage::new("system", "contract"),
                    RuntimeProfileMessage::new("user", "hello"),
                ],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_profile_safety_denied_role() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.safety_policy = json!({
            "deny_message_roles": ["system"]
        });
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![
                    RuntimeProfileMessage::new("system", "do not allow this role"),
                    RuntimeProfileMessage::new("user", "hello"),
                ],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_profile_safety_oversized_message() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.safety_policy = json!({
            "max_message_bytes": 4
        });
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "hello")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn minimal_runtime_rejects_profile_safety_unknown_field() {
        let mut contract = ProfileContract::new("test-profile", "v1");
        contract.safety_policy = json!({
            "future_policy": true
        });
        let runtime = MinimalRuntimeClient::default();
        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "test-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "hello")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
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
    async fn hermes_runtime_streams_delta_and_final_output() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["stream"], true);
                (
                    StatusCode::OK,
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/event-stream; charset=utf-8",
                    )],
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                )
            }),
        );
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let events = runtime
            .stream_profile_step(RuntimeProfileInput {
                profile_id: "honglou-main".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "question")],
                metadata: json!({}),
                profile_contract: None,
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(events[0].event_type.as_str(), "started");
        assert_eq!(events[1].content_delta.as_deref(), Some("hello"));
        assert_eq!(events[2].content_delta.as_deref(), Some(" world"));
        assert_eq!(events[3].event_type.as_str(), "schema_partial");
        assert_eq!(events[4].event_type.as_str(), "final");
        let output = events[4].output.as_ref().unwrap();
        assert_eq!(output.result_summary, "hello world");
        assert_eq!(output.metadata["streaming"], true);
    }

    #[tokio::test]
    async fn hermes_runtime_stream_run_sets_run_id_on_events() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["stream"], true);
                (
                    StatusCode::OK,
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/event-stream; charset=utf-8",
                    )],
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"run\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\" ok\"}}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                )
            }),
        );
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let input = hermes_run_input(new_trace_id());
        let run_id = input.run.id.clone();

        let events = runtime.stream_run(input).await.unwrap();

        assert!(
            events
                .iter()
                .all(|event| event.run_id.as_deref() == Some(run_id.as_str()))
        );
        assert!(events.iter().all(|event| event.session_id.is_none()));
        assert_eq!(events.last().unwrap().event_type.as_str(), "final");
        assert_eq!(
            events
                .last()
                .unwrap()
                .output
                .as_ref()
                .unwrap()
                .result_summary,
            "run ok"
        );
    }

    #[tokio::test]
    async fn hermes_runtime_streams_safe_error_event() {
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
        let events = runtime
            .stream_profile_step(RuntimeProfileInput {
                profile_id: "error-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "SECRET_PROMPT")],
                metadata: json!({}),
                profile_contract: None,
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.as_str(), "error");
        assert_eq!(events[0].error_code.as_deref(), Some("internal_error"));
        assert!(events[0].output.is_none());
        let encoded = serde_json::to_string(&events[0]).unwrap();
        assert!(!encoded.contains("SECRET_PROMPT"));
        assert!(!encoded.contains("upstream"));
    }

    #[tokio::test]
    async fn runtime_executes_multi_step_plan_with_output_refs() {
        let runtime = MinimalRuntimeClient::default();
        let contract_a = ProfileContract::new("profile-a", "v1");
        let contract_b = ProfileContract::new("profile-b", "v1");
        let step_a = RuntimeStep::new("profile-a", "v1", json!({"name": "first"}));
        let mut step_b = RuntimeStep::new("profile-b", "v1", json!({"name": "second"}));
        step_b.depends_on = vec![step_a.step_id.clone()];
        let trace_id = new_trace_id();
        let plan = RuntimeStepPlan::new(trace_id.clone(), vec![step_a.clone(), step_b]);

        let output = runtime
            .execute_profile_step_plan(RuntimeStepPlanInput {
                plan,
                messages: vec![RuntimeProfileMessage::new("user", "initial input")],
                metadata: json!({"purpose": "test"}),
                profile_contracts: vec![contract_a, contract_b],
                requested_tools_by_profile: BTreeMap::new(),
                trace_id,
            })
            .await
            .unwrap();

        assert!(output.result_summary.contains("runtime_step_input_ref"));
        assert!(output.result_summary.contains(&step_a.step_id));
        assert_eq!(output.metadata["runtime_step_plan"]["status"], "completed");
        assert_eq!(
            output.metadata["runtime_step_outputs"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(
            output.metadata["runtime_step_outputs"][0]["output_ref"]
                .as_str()
                .unwrap()
                .starts_with("result://runtime-profiles/profile-a")
        );
    }

    #[tokio::test]
    async fn runtime_step_plan_helper_materializes_step_contracts() {
        let runtime = MinimalRuntimeClient::default();
        let mut contract = ProfileContract::new("profile-a", "v1");
        contract.output_schema = json!({
            "type": "object",
            "required": ["result_summary"]
        });
        contract.tool_policy =
            RuntimeToolPolicy::read_only(vec!["tool.alpha".to_string(), "tool.beta".to_string()]);
        let trace_id = new_trace_id();
        let mut plan = RuntimeStepPlan::for_profile_contracts(
            trace_id.clone(),
            RuntimeStepPlanOwner::Manager,
            vec![contract.clone()],
            json!({"source": "manager"}),
        );
        plan.steps[0].tool_policy = RuntimeToolPolicy::read_only(vec!["tool.alpha".to_string()]);

        let output = runtime
            .execute_profile_step_plan(RuntimeStepPlanInput {
                plan,
                messages: vec![RuntimeProfileMessage::new("user", "step scoped")],
                metadata: json!({}),
                profile_contracts: vec![contract],
                requested_tools_by_profile: BTreeMap::from([(
                    "profile-a".to_string(),
                    vec!["tool.alpha".to_string(), "tool.beta".to_string()],
                )]),
                trace_id,
            })
            .await
            .unwrap();

        assert_eq!(output.metadata["effective_tool_set"][0], "tool.alpha");
        assert_eq!(
            output.metadata["runtime_step_plan"]["steps"][0]["tool_policy"]["allowed_tools"][0],
            "tool.alpha"
        );
        assert_eq!(
            output.metadata["runtime_step_plan"]["steps"][0]["output_contract"]["required"][0],
            "result_summary"
        );
    }

    #[tokio::test]
    async fn runtime_step_plan_validates_step_output_contract() {
        let runtime = MinimalRuntimeClient::default();
        let contract = ProfileContract::new("profile-a", "v1");
        let mut step = RuntimeStep::new("profile-a", "v1", json!({}));
        step.output_contract = json!({
            "type": "object",
            "required": ["metadata"],
            "properties": {
                "metadata": {
                    "type": "object",
                    "required": ["missing_field"]
                }
            }
        });
        let trace_id = new_trace_id();
        let result = runtime
            .execute_profile_step_plan(RuntimeStepPlanInput {
                plan: RuntimeStepPlan::new(trace_id.clone(), vec![step]),
                messages: vec![RuntimeProfileMessage::new("user", "bad output")],
                metadata: json!({}),
                profile_contracts: vec![contract],
                requested_tools_by_profile: BTreeMap::new(),
                trace_id,
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn runtime_step_plan_applies_fallback_to_executor_output_contract_failure() {
        let runtime = RawProfileRuntimeClient;
        let bad_contract = ProfileContract::new("bad-profile", "v1");
        let good_contract = ProfileContract::new("good-profile", "v1");
        let mut bad_step = RuntimeStep::new("bad-profile", "v1", json!({}));
        bad_step.output_contract = json!({
            "type": "object",
            "required": ["metadata"],
            "properties": {
                "metadata": {
                    "type": "object",
                    "required": ["missing_field"]
                }
            }
        });
        bad_step.fallback_policy = RuntimeStepFailurePolicy::Continue;
        let good_step = RuntimeStep::new("good-profile", "v1", json!({}));
        let trace_id = new_trace_id();

        let output = runtime
            .execute_profile_step_plan(RuntimeStepPlanInput {
                plan: RuntimeStepPlan::new(trace_id.clone(), vec![bad_step, good_step]),
                messages: vec![RuntimeProfileMessage::new("user", "continue")],
                metadata: json!({}),
                profile_contracts: vec![bad_contract, good_contract],
                requested_tools_by_profile: BTreeMap::new(),
                trace_id,
            })
            .await
            .unwrap();

        assert_eq!(output.metadata["runtime_step_plan"]["status"], "failed");
        assert_eq!(
            output.metadata["runtime_step_outputs"][0]["error_code"],
            "step_output_invalid"
        );
        assert_eq!(
            output.metadata["runtime_step_outputs"][1]["status"],
            "completed"
        );
    }

    #[tokio::test]
    async fn runtime_step_plan_continues_optional_failed_step() {
        let runtime = MinimalRuntimeClient::default();
        let mut bad_contract = ProfileContract::new("bad-profile", "v1");
        bad_contract.output_schema = json!({
            "type": "object",
            "required": ["metadata"],
            "properties": {
                "metadata": {
                    "type": "object",
                    "required": ["never_present"]
                }
            }
        });
        let good_contract = ProfileContract::new("good-profile", "v1");
        let mut bad_step = RuntimeStep::new("bad-profile", "v1", json!({}));
        bad_step.fallback_policy = RuntimeStepFailurePolicy::Continue;
        let good_step = RuntimeStep::new("good-profile", "v1", json!({}));
        let trace_id = new_trace_id();
        let plan = RuntimeStepPlan::new(trace_id.clone(), vec![bad_step, good_step]);

        let output = runtime
            .execute_profile_step_plan(RuntimeStepPlanInput {
                plan,
                messages: vec![RuntimeProfileMessage::new("user", "continue")],
                metadata: json!({}),
                profile_contracts: vec![bad_contract, good_contract],
                requested_tools_by_profile: BTreeMap::new(),
                trace_id,
            })
            .await
            .unwrap();

        assert_eq!(output.metadata["runtime_step_plan"]["status"], "failed");
        assert_eq!(
            output.metadata["runtime_step_outputs"][0]["status"],
            "failed"
        );
        assert_eq!(
            output.metadata["runtime_step_outputs"][1]["status"],
            "completed"
        );
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
    async fn hermes_runtime_executes_authorized_profile_tool_call() {
        let calls = Arc::new(Mutex::new(0usize));
        let app = Router::new()
            .route(
                "/chat/completions",
                post(
                    |State(calls): State<Arc<Mutex<usize>>>,
                     Json(body): Json<Value>| async move {
                        let round = {
                            let mut calls = calls.lock().unwrap();
                            *calls += 1;
                            *calls
                        };
                        if round == 1 {
                            assert_eq!(
                                body["tools"][0]["function"]["name"],
                                "tonglingyu.text.search"
                            );
                            Json(json!({
                                "choices": [
                                    {
                                        "message": {
                                            "role": "assistant",
                                            "tool_calls": [
                                                {
                                                    "id": "call-1",
                                                    "type": "function",
                                                    "function": {
                                                        "name": "tonglingyu.text.search",
                                                        "arguments": "{\"query\":\"通灵玉\"}"
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                ]
                            }))
                        } else {
                            assert!(body["messages"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|message| message["role"] == "tool"
                                    && message["tool_call_id"] == "call-1"));
                            Json(json!({
                                "choices": [
                                    {"message": {"role": "assistant", "content": "tool grounded answer"}}
                                ]
                            }))
                        }
                    },
                ),
            )
            .with_state(calls.clone());
        let mut contract = ProfileContract::new("honglou-text", "v1");
        contract.tool_policy =
            RuntimeToolPolicy::read_only(vec!["tonglingyu.text.search".to_string()]);
        let output_schema = json!({
            "type": "object",
            "required": ["cards"],
            "properties": {"cards": {"type": "array"}}
        });
        contract.tool_policy.tool_specs = vec![RuntimeToolSpec {
            name: "tonglingyu.text.search".to_string(),
            description: "Search Tonglingyu source text".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["query"],
                "properties": {"query": {"type": "string"}}
            }),
            output_schema: output_schema.clone(),
            output_ref_required: true,
            capability: RuntimeToolCapability::ReadOnly,
        }];
        let executor = StaticRuntimeToolExecutor::new([(
            "tonglingyu.text.search".to_string(),
            json!({"cards": [{"evidence_id": "ev-1", "text": "通灵玉"}]}),
        )]);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(executor));

        let output = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "honglou-text".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "通灵玉是什么？")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tonglingyu.text.search".to_string()],
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(output.result_summary, "tool grounded answer");
        assert_eq!(output.metadata["tool_rounds"], 1);
        assert_eq!(
            output.metadata["tool_results"][0]["tool_name"],
            "tonglingyu.text.search"
        );
        assert_eq!(
            output.metadata["tool_results"][0]["output_schema"],
            output_schema
        );
        assert!(output.metadata["tool_results"][0].get("output").is_none());
        assert_eq!(
            output.metadata["tool_audit_events"][0]["event"],
            "runtime_tool_call"
        );
        assert_eq!(
            output.metadata["tool_audit_events"][1]["event"],
            "runtime_tool_result"
        );
        assert_eq!(*calls.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn hermes_runtime_streams_tool_progress_and_schema_partial_events() {
        let calls = Arc::new(Mutex::new(0usize));
        let app = Router::new()
            .route(
                "/chat/completions",
                post(
                    |State(calls): State<Arc<Mutex<usize>>>, Json(_body): Json<Value>| async move {
                        let mut calls_guard = calls.lock().unwrap();
                        *calls_guard += 1;
                        if *calls_guard == 1 {
                            Json(json!({
                                "choices": [
                                    {
                                        "message": {
                                            "role": "assistant",
                                            "tool_calls": [
                                                {
                                                    "id": "call-stream",
                                                    "type": "function",
                                                    "function": {
                                                        "name": "tool.read",
                                                        "arguments": "{}"
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                ]
                            }))
                        } else {
                            Json(json!({
                                "choices": [
                                    {"message": {"role": "assistant", "content": "done"}}
                                ]
                            }))
                        }
                    },
                ),
            )
            .with_state(calls);
        let mut contract = ProfileContract::new("tool-profile", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.read".to_string()]);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(StaticRuntimeToolExecutor::new([(
            "tool.read".to_string(),
            json!({"ok": true}),
        )])));

        let events = runtime
            .stream_profile_step(RuntimeProfileInput {
                profile_id: "tool-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "stream tool")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.read".to_string()],
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(events[0].event_type.as_str(), "started");
        assert_eq!(events[1].event_type.as_str(), "tool_progress");
        assert_eq!(
            events[1].metadata["tool_event"]["event"],
            "runtime_tool_call"
        );
        assert_eq!(events[2].event_type.as_str(), "tool_progress");
        assert_eq!(
            events[2].metadata["tool_event"]["event"],
            "runtime_tool_result"
        );
        assert_eq!(events[3].event_type.as_str(), "schema_partial");
        assert_eq!(events[4].event_type.as_str(), "final");
        assert!(
            events
                .iter()
                .all(|event| event.schema_version.as_deref() == Some("v1"))
        );
    }

    #[tokio::test]
    async fn hermes_runtime_writes_tool_events_to_jsonl_audit_sink() {
        let calls = Arc::new(Mutex::new(0usize));
        let app = Router::new()
            .route(
                "/chat/completions",
                post(
                    |State(calls): State<Arc<Mutex<usize>>>, Json(_body): Json<Value>| async move {
                        let mut calls_guard = calls.lock().unwrap();
                        *calls_guard += 1;
                        if *calls_guard == 1 {
                            Json(json!({
                                "choices": [
                                    {
                                        "message": {
                                            "role": "assistant",
                                            "tool_calls": [
                                                {
                                                    "id": "call-audit",
                                                    "type": "function",
                                                    "function": {
                                                        "name": "tool.read",
                                                        "arguments": "{}"
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                ]
                            }))
                        } else {
                            Json(json!({
                                "choices": [
                                    {"message": {"role": "assistant", "content": "audited"}}
                                ]
                            }))
                        }
                    },
                ),
            )
            .with_state(calls);
        let mut contract = ProfileContract::new("audit-profile", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.read".to_string()]);
        let audit_path = std::env::temp_dir().join(format!("{}.jsonl", new_id("rtaudit")));
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(StaticRuntimeToolExecutor::new([(
            "tool.read".to_string(),
            json!({"ok": true}),
        )])))
        .with_audit_sink(Arc::new(JsonlRuntimeAuditSink::new(&audit_path)));

        runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "audit-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "audit")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.read".to_string()],
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        let log = tokio::fs::read_to_string(&audit_path).await.unwrap();
        assert_eq!(log.matches("runtime_tool_call").count(), 1);
        assert_eq!(log.matches("runtime_tool_result").count(), 1);
        assert!(log.contains("\"output_schema\""));
        assert!(!log.contains("\"output\":"));
        let _ = tokio::fs::remove_file(audit_path).await;
    }

    #[tokio::test]
    async fn hermes_runtime_exposes_only_requested_profile_tools() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                let tools = body["tools"].as_array().unwrap();
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0]["function"]["name"], "tool.alpha");
                Json(json!({
                    "choices": [
                        {"message": {"role": "assistant", "content": "scoped tools"}}
                    ]
                }))
            }),
        );
        let mut contract = ProfileContract::new("scoped-profile", "v1");
        contract.tool_policy =
            RuntimeToolPolicy::read_only(vec!["tool.alpha".to_string(), "tool.beta".to_string()]);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let output = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "scoped-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "scoped")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.alpha".to_string()],
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(output.result_summary, "scoped tools");
        assert_eq!(output.metadata["effective_tool_set"][0], "tool.alpha");
        assert_eq!(
            output.metadata["effective_tool_set"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn hermes_runtime_rejects_write_capability_tool_scope() {
        let mut contract = ProfileContract::new("write-profile", "v1");
        contract.tool_policy.allowed_tools = vec!["tool.write".to_string()];
        contract.tool_policy.tool_specs = vec![RuntimeToolSpec::write("tool.write")];
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "write-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "write")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.write".to_string()],
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Forbidden,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn hermes_runtime_omits_large_tool_payload_from_model_and_metadata() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                let has_tool_result = body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| message["role"] == "tool")
                    .cloned();
                if let Some(message) = has_tool_result {
                    let content: Value =
                        serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
                    assert_eq!(content["output_omitted"], true);
                    assert!(content.get("output").is_none());
                    Json(json!({
                        "choices": [
                            {"message": {"role": "assistant", "content": "large result summarized"}}
                        ]
                    }))
                } else {
                    Json(json!({
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "tool_calls": [
                                        {
                                            "id": "call-large",
                                            "type": "function",
                                            "function": {
                                                "name": "tool.large",
                                                "arguments": "{}"
                                            }
                                        }
                                    ]
                                }
                            }
                        ]
                    }))
                }
            }),
        );
        let mut contract = ProfileContract::new("large-profile", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.large".to_string()]);
        let executor = StaticRuntimeToolExecutor::new([(
            "tool.large".to_string(),
            json!({"text": "x".repeat(128)}),
        )]);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(executor))
        .with_max_tool_result_inline_bytes(16);

        let output = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "large-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "use large tool")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.large".to_string()],
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(output.result_summary, "large result summarized");
        assert!(output.metadata["tool_results"][0].get("output").is_none());
        assert!(
            output.metadata["tool_results"][0]["output_ref"]
                .as_str()
                .unwrap()
                .starts_with("runtime://tool-results/")
        );
    }

    #[tokio::test]
    async fn hermes_runtime_omits_tool_metadata_payload_from_metadata_and_audit() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(body): Json<Value>| async move {
                let has_tool_result = body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["role"] == "tool");
                if has_tool_result {
                    Json(json!({
                        "choices": [
                            {"message": {"role": "assistant", "content": "metadata summarized"}}
                        ]
                    }))
                } else {
                    Json(json!({
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "tool_calls": [
                                        {
                                            "id": "call-metadata",
                                            "type": "function",
                                            "function": {
                                                "name": "tool.metadata",
                                                "arguments": "{}"
                                            }
                                        }
                                    ]
                                }
                            }
                        ]
                    }))
                }
            }),
        );
        let mut contract = ProfileContract::new("metadata-profile", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.metadata".to_string()]);
        let audit_path = std::env::temp_dir().join(format!("{}.jsonl", new_id("rtaudit")));
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(LeakyMetadataToolExecutor))
        .with_audit_sink(Arc::new(JsonlRuntimeAuditSink::new(&audit_path)));

        let output = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "metadata-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "use metadata tool")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.metadata".to_string()],
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        let encoded_metadata = serde_json::to_string(&output.metadata).unwrap();
        assert!(!encoded_metadata.contains("SECRET_TOOL_METADATA"));
        assert!(!encoded_metadata.contains("SECRET_TOOL_PAYLOAD"));
        assert!(!encoded_metadata.contains("SECRET_TOOL_OUTPUT"));
        assert!(!encoded_metadata.contains("SECRET_TOOL_VALUE"));
        assert_eq!(
            output.metadata["tool_results"][0]["output_ref"],
            "runtime://tool-results/leaky"
        );
        assert_eq!(
            output.metadata["tool_results"][0]["output_summary"],
            "object_keys_len:1"
        );
        assert_eq!(
            output.metadata["tool_results"][0]["output_schema"],
            json!({"type": "object"})
        );
        assert_eq!(
            output.metadata["tool_results"][0]["call_id"],
            "call-metadata"
        );
        assert_eq!(
            output.metadata["tool_results"][0]["profile_id"],
            "metadata-profile"
        );
        assert_eq!(
            output.metadata["tool_results"][0]["tool_name"],
            "tool.metadata"
        );
        assert_eq!(
            output.metadata["tool_results"][0]["trace_id"].is_string(),
            true
        );
        assert!(output.metadata["tool_results"][0].get("metadata").is_none());

        let log = tokio::fs::read_to_string(&audit_path).await.unwrap();
        assert_eq!(log.matches("runtime_tool_result").count(), 1);
        assert!(!log.contains("spoofed-call"));
        assert!(!log.contains("spoofed-profile"));
        assert!(!log.contains("spoofed-tool"));
        assert!(!log.contains("SECRET_TOOL_METADATA"));
        assert!(!log.contains("SECRET_TOOL_PAYLOAD"));
        assert!(!log.contains("SECRET_TOOL_OUTPUT"));
        assert!(!log.contains("SECRET_TOOL_VALUE"));
        let _ = tokio::fs::remove_file(audit_path).await;
    }

    #[tokio::test]
    async fn hermes_runtime_rejects_excessive_tool_rounds() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(_body): Json<Value>| async move {
                Json(json!({
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "tool_calls": [
                                    {
                                        "id": "call-loop",
                                        "type": "function",
                                        "function": {
                                            "name": "tool.read",
                                            "arguments": "{}"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }))
            }),
        );
        let mut contract = ProfileContract::new("loop-profile", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.read".to_string()]);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(StaticRuntimeToolExecutor::new([(
            "tool.read".to_string(),
            json!({"ok": true}),
        )])))
        .with_max_tool_rounds(1);

        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "loop-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "loop")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: vec!["tool.read".to_string()],
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn hermes_runtime_rejects_expired_profile_budget() {
        let mut contract = ProfileContract::new("budget-profile", "v1");
        contract.max_runtime_seconds = Some(0);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "budget-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "budget")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Conflict,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn hermes_runtime_streams_safe_error_for_expired_profile_budget() {
        let mut contract = ProfileContract::new("budget-profile", "v1");
        contract.max_runtime_seconds = Some(0);
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();

        let events = runtime
            .stream_profile_step(RuntimeProfileInput {
                profile_id: "budget-profile".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "SECRET_BUDGET")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.as_str(), "error");
        assert_eq!(events[0].error_code.as_deref(), Some("conflict"));
        assert!(events[0].output.is_none());
        let encoded = serde_json::to_string(&events[0]).unwrap();
        assert!(!encoded.contains("SECRET_BUDGET"));
    }

    #[tokio::test]
    async fn hermes_runtime_rejects_unauthorized_profile_tool_call() {
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(_body): Json<Value>| async move {
                Json(json!({
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "tool_calls": [
                                    {
                                        "id": "call-1",
                                        "type": "function",
                                        "function": {
                                            "name": "direct_external_write",
                                            "arguments": "{}"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }))
            }),
        );
        let mut contract = ProfileContract::new("honglou-main", "v1");
        contract.tool_policy = RuntimeToolPolicy::read_only(vec!["tool.read".to_string()]);
        let audit_path = std::env::temp_dir().join(format!("{}.jsonl", new_id("rtaudit")));
        let runtime = HermesRuntimeClient::new(HermesRuntimeConfig {
            base_url: spawn_server(app).await,
            api_key: None,
            model: "hermes-agent".to_string(),
            profile_models: BTreeMap::new(),
            timeout: Duration::from_secs(2),
        })
        .unwrap()
        .with_tool_executor(Arc::new(StaticRuntimeToolExecutor::default()))
        .with_audit_sink(Arc::new(JsonlRuntimeAuditSink::new(&audit_path)));

        let result = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: "honglou-main".to_string(),
                messages: vec![RuntimeProfileMessage::new("user", "write externally")],
                metadata: json!({}),
                profile_contract: Some(contract),
                runtime_step: None,
                requested_tools: Vec::new(),
                trace_id: new_trace_id(),
            })
            .await;

        assert!(matches!(
            result.unwrap_err(),
            AgentCoreError::Coded {
                code: ErrorCode::Forbidden,
                ..
            }
        ));

        let log = tokio::fs::read_to_string(&audit_path).await.unwrap();
        assert_eq!(log.matches("runtime_tool_call").count(), 1);
        assert_eq!(log.matches("runtime_tool_error").count(), 1);
        assert!(!log.contains("\"arguments\""));
        let _ = tokio::fs::remove_file(audit_path).await;
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
                    |State((seen_provider_ref, seen_payload)): State<SeenWriteConnectorInput>,
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
