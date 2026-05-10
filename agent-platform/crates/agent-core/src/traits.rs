use crate::{
    AgentCoreError, AgentInstance, AgentRun, AgentSessionMessage, CoreResult, CredentialLease,
    ErrorCode, ExternalActionMode, ExternalActionPlan, ObserverSnapshot, ProfileContract,
    RiskLevel, RunSummary, RuntimeStep, RuntimeStreamEventType, RuntimeToolCall, RuntimeToolResult,
    RuntimeToolSpec, SessionContext,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRunInput {
    pub run: AgentRun,
    #[serde(default)]
    pub agent: Option<AgentInstance>,
    pub context: Option<SessionContext>,
    pub snapshot: Option<Value>,
    #[serde(default)]
    pub profile_contract: Option<ProfileContract>,
    #[serde(default)]
    pub runtime_step: Option<RuntimeStep>,
    #[serde(default)]
    pub requested_tools: Vec<String>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSessionInput {
    pub session_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub agent: Option<AgentInstance>,
    pub message: AgentSessionMessage,
    pub context: SessionContext,
    #[serde(default)]
    pub snapshot: Option<Value>,
    #[serde(default)]
    pub profile_contract: Option<ProfileContract>,
    #[serde(default)]
    pub runtime_step: Option<RuntimeStep>,
    #[serde(default)]
    pub requested_tools: Vec<String>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfileMessage {
    pub role: String,
    pub content: String,
}

impl RuntimeProfileMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfileInput {
    pub profile_id: String,
    pub messages: Vec<RuntimeProfileMessage>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub profile_contract: Option<ProfileContract>,
    #[serde(default)]
    pub runtime_step: Option<RuntimeStep>,
    #[serde(default)]
    pub requested_tools: Vec<String>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOutput {
    pub result_summary: String,
    pub result_ref: Option<String>,
    #[serde(default)]
    pub messages: Vec<AgentSessionMessage>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStreamEvent {
    pub sequence: u64,
    pub event_type: RuntimeStreamEventType,
    pub profile_id: String,
    pub trace_id: String,
    pub content_delta: Option<String>,
    pub output: Option<RuntimeOutput>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

impl RuntimeStreamEvent {
    pub fn final_output(
        profile_id: impl Into<String>,
        trace_id: impl Into<String>,
        output: RuntimeOutput,
    ) -> Self {
        Self {
            sequence: 0,
            event_type: RuntimeStreamEventType::Final,
            profile_id: profile_id.into(),
            trace_id: trace_id.into(),
            content_delta: None,
            output: Some(output),
            error_code: None,
            metadata: serde_json::json!({}),
        }
    }
}

#[async_trait]
pub trait RuntimeClient: Send + Sync {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput>;

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput>;

    async fn execute_profile_step(&self, _input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime client does not support direct profile steps",
        ))
    }

    async fn stream_run(&self, input: RuntimeRunInput) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let profile_id = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.profile_id.clone())
            .or_else(|| {
                input
                    .agent
                    .as_ref()
                    .map(|agent| agent.hermes_profile.clone())
            })
            .unwrap_or_else(|| "agent-platform-runtime".to_string());
        let trace_id = input.trace_id.clone();
        let output = self.execute_run(input).await?;
        Ok(vec![RuntimeStreamEvent::final_output(
            profile_id, trace_id, output,
        )])
    }

    async fn stream_session_message(
        &self,
        input: RuntimeSessionInput,
    ) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let profile_id = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.profile_id.clone())
            .or_else(|| {
                input
                    .agent
                    .as_ref()
                    .map(|agent| agent.hermes_profile.clone())
            })
            .unwrap_or_else(|| input.agent_id.clone());
        let trace_id = input.trace_id.clone();
        let output = self.send_session_message(input).await?;
        Ok(vec![RuntimeStreamEvent::final_output(
            profile_id, trace_id, output,
        )])
    }

    async fn stream_profile_step(
        &self,
        input: RuntimeProfileInput,
    ) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let profile_id = input.profile_id.clone();
        let trace_id = input.trace_id.clone();
        let output = self.execute_profile_step(input).await?;
        Ok(vec![RuntimeStreamEvent::final_output(
            profile_id, trace_id, output,
        )])
    }
}

#[async_trait]
pub trait RuntimeToolExecutor: Send + Sync {
    async fn execute_tool(
        &self,
        call: RuntimeToolCall,
        spec: RuntimeToolSpec,
    ) -> CoreResult<RuntimeToolResult>;
}

pub fn validate_json_schema_value(schema: &Value, value: &Value) -> CoreResult<()> {
    validate_json_schema_at(schema, value, "$")
}

fn validate_json_schema_at(schema: &Value, value: &Value, path: &str) -> CoreResult<()> {
    let Some(schema_object) = schema.as_object() else {
        return Ok(());
    };
    if schema_object.is_empty() {
        return Ok(());
    }

    if let Some(expected) = schema_object.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "null" => value.is_null(),
            _ => true,
        };
        if !matches {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                format!("schema validation failed at {path}: expected {expected}"),
            ));
        }
    }

    if let Some(values) = schema_object.get("enum").and_then(Value::as_array)
        && !values.iter().any(|item| item == value)
    {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            format!("schema validation failed at {path}: enum mismatch"),
        ));
    }

    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        let object = value.as_object().ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::Conflict,
                format!("schema validation failed at {path}: expected object"),
            )
        })?;
        for field in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(field) {
                return Err(AgentCoreError::coded(
                    ErrorCode::Conflict,
                    format!("schema validation failed at {path}: missing {field}"),
                ));
            }
        }
    }

    if let (Some(properties), Some(object)) = (
        schema_object.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        for (field, property_schema) in properties {
            if let Some(field_value) = object.get(field) {
                validate_json_schema_at(property_schema, field_value, &format!("{path}.{field}"))?;
            }
        }
        if schema_object
            .get("additionalProperties")
            .and_then(Value::as_bool)
            == Some(false)
        {
            for field in object.keys() {
                if !properties.contains_key(field) {
                    return Err(AgentCoreError::coded(
                        ErrorCode::Conflict,
                        format!("schema validation failed at {path}: unexpected {field}"),
                    ));
                }
            }
        }
    }

    if let (Some(item_schema), Some(items)) = (schema_object.get("items"), value.as_array()) {
        for (index, item) in items.iter().enumerate() {
            validate_json_schema_at(item_schema, item, &format!("{path}[{index}]"))?;
        }
    }

    if let Some(min_items) = schema_object.get("minItems").and_then(Value::as_u64)
        && value
            .as_array()
            .map(|items| items.len())
            .unwrap_or_default()
            < min_items as usize
    {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            format!("schema validation failed at {path}: minItems"),
        ));
    }

    Ok(())
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append_message(&self, message: AgentSessionMessage)
    -> CoreResult<AgentSessionMessage>;
    async fn session_context(&self, session_id: &str, trace_id: &str)
    -> CoreResult<SessionContext>;
    async fn write_summary(
        &self,
        session_id: &str,
        summary: &str,
        trace_id: &str,
    ) -> CoreResult<()>;
    async fn write_result_ref(
        &self,
        run_id: &str,
        result_summary: &str,
        result_ref: Option<&str>,
        trace_id: &str,
    ) -> CoreResult<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorSnapshot {
    pub connector: String,
    pub resource: String,
    pub payload_ref: String,
    pub summary: Value,
}

#[async_trait]
pub trait ConnectorClient: Send + Sync {
    async fn read_only_snapshot(
        &self,
        connector: &str,
        resource: &str,
        trace_id: &str,
    ) -> CoreResult<ConnectorSnapshot>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLeaseRequest {
    pub external_action_plan_id: String,
    pub credential_scope: String,
    pub trace_id: String,
}

#[async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn dry_run_lease(&self, request: CredentialLeaseRequest) -> CoreResult<CredentialLease>;
    async fn active_lease(&self, request: CredentialLeaseRequest) -> CoreResult<CredentialLease>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConnectorDryRunInput {
    pub plan: ExternalActionPlan,
    #[serde(default)]
    pub payload: Value,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConnectorDryRunOutput {
    pub accepted: bool,
    pub status: String,
    pub result_ref: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConnectorExecuteInput {
    pub plan: ExternalActionPlan,
    pub idempotency_key: String,
    pub credential_provider_ref: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConnectorExecuteOutput {
    pub accepted: bool,
    pub status: String,
    pub result_ref: Option<String>,
    pub compensation_ref: Option<String>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConnectorCompensateInput {
    pub plan: ExternalActionPlan,
    pub compensation_ref: String,
    pub reason: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConnectorCompensateOutput {
    pub accepted: bool,
    pub status: String,
    pub result_ref: Option<String>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[async_trait]
pub trait WriteConnector: Send + Sync {
    async fn dry_run(
        &self,
        input: WriteConnectorDryRunInput,
    ) -> CoreResult<WriteConnectorDryRunOutput>;
    async fn execute(
        &self,
        input: WriteConnectorExecuteInput,
    ) -> CoreResult<WriteConnectorExecuteOutput>;
    async fn compensate(
        &self,
        input: WriteConnectorCompensateInput,
    ) -> CoreResult<WriteConnectorCompensateOutput>;
}

pub fn external_action_requires_credential(mode: ExternalActionMode, risk: RiskLevel) -> bool {
    matches!(
        (mode, risk),
        (ExternalActionMode::Authorized, _)
            | (ExternalActionMode::ApprovalRequired, _)
            | (_, RiskLevel::High | RiskLevel::Critical)
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunClaim {
    pub run: AgentRun,
    pub lease_owner: String,
    pub lease_seconds: i64,
}

#[async_trait]
pub trait RunQueue: Send + Sync {
    async fn enqueue_run(&self, run: AgentRun) -> CoreResult<AgentRun>;
    async fn claim_next_run(
        &self,
        worker_id: &str,
        lease: Duration,
    ) -> CoreResult<Option<RunClaim>>;
    async fn heartbeat_run(&self, run_id: &str, worker_id: &str, lease: Duration)
    -> CoreResult<()>;
    async fn finish_run(&self, run_id: &str, output: RuntimeOutput) -> CoreResult<AgentRun>;
    async fn fail_or_retry_run(
        &self,
        run_id: &str,
        reason: &str,
        max_retries: i32,
    ) -> CoreResult<AgentRun>;
    async fn dead_letter_run(&self, run_id: &str, reason: &str) -> CoreResult<AgentRun>;
    async fn sweep_expired_leases(&self, max_retries: i32) -> CoreResult<Vec<RunSummary>>;
}

#[async_trait]
pub trait ObserverSnapshotStore: Send + Sync {
    async fn collect_observer_snapshot(&self, trace_id: &str) -> CoreResult<ObserverSnapshot>;
}

pub fn runtime_failure(reason: impl Into<String>) -> AgentCoreError {
    AgentCoreError::coded(crate::ErrorCode::InternalError, reason)
}
