use crate::{
    AgentCoreError, AgentInstance, AgentRun, AgentSessionMessage, CoreResult, CredentialLease,
    ErrorCode, ExternalActionMode, ExternalActionPlan, ObserverSnapshot, ProfileContract,
    RiskLevel, RunSummary, RuntimeStep, RuntimeStepFailurePolicy, RuntimeStepPlan,
    RuntimeStepStatus, RuntimeStreamEventType, RuntimeToolCall, RuntimeToolResult, RuntimeToolSpec,
    SessionContext,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

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
pub struct RuntimeStepPlanInput {
    pub plan: RuntimeStepPlan,
    pub messages: Vec<RuntimeProfileMessage>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub profile_contracts: Vec<ProfileContract>,
    #[serde(default)]
    pub requested_tools_by_profile: BTreeMap<String, Vec<String>>,
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
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub schema_version: Option<String>,
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
            run_id: None,
            session_id: None,
            schema_version: None,
            content_delta: None,
            output: Some(output),
            error_code: None,
            metadata: serde_json::json!({}),
        }
    }

    pub fn tool_progress(
        sequence: u64,
        profile_id: impl Into<String>,
        trace_id: impl Into<String>,
        metadata: Value,
    ) -> Self {
        Self {
            sequence,
            event_type: RuntimeStreamEventType::ToolProgress,
            profile_id: profile_id.into(),
            trace_id: trace_id.into(),
            run_id: None,
            session_id: None,
            schema_version: None,
            content_delta: None,
            output: None,
            error_code: None,
            metadata,
        }
    }

    pub fn schema_partial(
        sequence: u64,
        profile_id: impl Into<String>,
        trace_id: impl Into<String>,
        metadata: Value,
    ) -> Self {
        Self {
            sequence,
            event_type: RuntimeStreamEventType::SchemaPartial,
            profile_id: profile_id.into(),
            trace_id: trace_id.into(),
            run_id: None,
            session_id: None,
            schema_version: None,
            content_delta: None,
            output: None,
            error_code: None,
            metadata,
        }
    }

    pub fn error(
        sequence: u64,
        profile_id: impl Into<String>,
        trace_id: impl Into<String>,
        code: ErrorCode,
    ) -> Self {
        Self {
            sequence,
            event_type: RuntimeStreamEventType::Error,
            profile_id: profile_id.into(),
            trace_id: trace_id.into(),
            run_id: None,
            session_id: None,
            schema_version: None,
            content_delta: None,
            output: None,
            error_code: Some(code.as_str().to_string()),
            metadata: json!({
                "safe_message": code.safe_message(),
            }),
        }
    }
}

pub fn runtime_stream_error_event(
    sequence: u64,
    profile_id: impl Into<String>,
    trace_id: impl Into<String>,
    error: &AgentCoreError,
) -> RuntimeStreamEvent {
    RuntimeStreamEvent::error(sequence, profile_id, trace_id, error.code())
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

    async fn execute_profile_step_plan(
        &self,
        input: RuntimeStepPlanInput,
    ) -> CoreResult<RuntimeOutput> {
        execute_runtime_step_plan(self, input).await
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
        let run_id = input.run.id.clone();
        let schema_version = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.version.version.clone());
        match self.execute_run(input).await {
            Ok(output) => {
                let mut event = RuntimeStreamEvent::final_output(profile_id, trace_id, output);
                event.run_id = Some(run_id);
                event.schema_version = schema_version;
                Ok(vec![event])
            }
            Err(error) => {
                let mut event = runtime_stream_error_event(0, profile_id, trace_id, &error);
                event.run_id = Some(run_id);
                event.schema_version = schema_version;
                Ok(vec![event])
            }
        }
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
        let session_id = input.session_id.clone();
        let schema_version = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.version.version.clone());
        match self.send_session_message(input).await {
            Ok(output) => {
                let mut event = RuntimeStreamEvent::final_output(profile_id, trace_id, output);
                event.session_id = Some(session_id);
                event.schema_version = schema_version;
                Ok(vec![event])
            }
            Err(error) => {
                let mut event = runtime_stream_error_event(0, profile_id, trace_id, &error);
                event.session_id = Some(session_id);
                event.schema_version = schema_version;
                Ok(vec![event])
            }
        }
    }

    async fn stream_profile_step(
        &self,
        input: RuntimeProfileInput,
    ) -> CoreResult<Vec<RuntimeStreamEvent>> {
        let profile_id = input.profile_id.clone();
        let trace_id = input.trace_id.clone();
        let schema_version = input
            .profile_contract
            .as_ref()
            .map(|contract| contract.version.version.clone());
        match self.execute_profile_step(input).await {
            Ok(output) => {
                let mut event = RuntimeStreamEvent::final_output(profile_id, trace_id, output);
                event.schema_version = schema_version;
                Ok(vec![event])
            }
            Err(error) => {
                let mut event = runtime_stream_error_event(0, profile_id, trace_id, &error);
                event.schema_version = schema_version;
                Ok(vec![event])
            }
        }
    }
}

async fn execute_runtime_step_plan<C>(
    client: &C,
    mut input: RuntimeStepPlanInput,
) -> CoreResult<RuntimeOutput>
where
    C: RuntimeClient + ?Sized,
{
    if input.plan.steps.is_empty() {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime step plan has no steps",
        ));
    }
    if input.plan.trace_id != input.trace_id {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime step plan trace_id mismatch",
        ));
    }

    let mut seen_step_ids = BTreeSet::new();
    for step in &input.plan.steps {
        if step.step_id.trim().is_empty() || step.profile_id.trim().is_empty() {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "runtime step plan contains an invalid step",
            ));
        }
        if !seen_step_ids.insert(step.step_id.clone()) {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "runtime step plan contains duplicate step ids",
            ));
        }
    }

    let contracts = input
        .profile_contracts
        .into_iter()
        .map(|contract| (contract.profile_id.clone(), contract))
        .collect::<BTreeMap<_, _>>();
    let mut output_refs = BTreeMap::<String, String>::new();
    let mut completed_steps = Vec::new();
    let mut step_summaries = Vec::new();
    let mut last_output = None;
    let mut had_failed_continued_step = false;

    for mut step in input.plan.steps.clone() {
        let mut dependency_refs = Vec::new();
        let mut dependency_error = None;
        for dependency in &step.depends_on {
            match output_refs.get(dependency) {
                Some(output_ref) => dependency_refs.push(json!({
                    "step_id": dependency,
                    "output_ref": output_ref,
                })),
                None => {
                    dependency_error = Some(AgentCoreError::coded(
                        ErrorCode::Conflict,
                        "runtime step dependency was not completed",
                    ));
                    break;
                }
            }
        }
        if let Some(error) = dependency_error {
            record_runtime_step_failure(
                &mut step,
                &mut completed_steps,
                &mut step_summaries,
                "dependency_not_completed",
            );
            if step.fallback_policy == RuntimeStepFailurePolicy::Continue {
                had_failed_continued_step = true;
                continue;
            }
            return Err(error);
        }
        let contract = contracts.get(&step.profile_id).cloned().ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::Conflict,
                "runtime step profile contract was not provided",
            )
        })?;
        if step.contract_version != contract.version.version {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "runtime step contract version mismatch",
            ));
        }

        let step_has_tool_policy = step.tool_policy.has_rules();
        let step_has_output_contract = !json_schema_is_empty(&step.output_contract);
        let mut step_contract = contract.clone();
        if step_has_tool_policy {
            step_contract.tool_policy = step.tool_policy.clone();
        } else {
            step.tool_policy = contract.tool_policy.clone();
        }
        if step_has_output_contract {
            step_contract.output_schema = step.output_contract.clone();
        } else {
            step.output_contract = contract.output_schema.clone();
        }

        step.status = RuntimeStepStatus::Executing;
        step.input_ref = dependency_refs
            .first()
            .and_then(|value| value.get("output_ref"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let mut step_messages = input.messages.clone();
        for dependency_ref in &dependency_refs {
            let encoded = serde_json::to_string(dependency_ref).map_err(|_| {
                AgentCoreError::coded(
                    ErrorCode::InternalError,
                    "runtime step dependency ref was not serializable",
                )
            })?;
            step_messages.push(RuntimeProfileMessage::new(
                "system",
                format!("runtime_step_input_ref {encoded}"),
            ));
        }
        let profile_requested_tools = input
            .requested_tools_by_profile
            .get(&step.profile_id)
            .cloned()
            .unwrap_or_default();
        contract
            .tool_policy
            .validate_requested_tools(&profile_requested_tools)?;
        let requested_tools = if step_has_tool_policy {
            step_contract
                .tool_policy
                .effective_tools_for_request(&profile_requested_tools)
        } else {
            profile_requested_tools
        };
        step_contract
            .tool_policy
            .validate_requested_tools(&requested_tools)?;
        let step_metadata = json!({
            "plan_id": &input.plan.plan_id,
            "plan_owner": input.plan.owner,
            "step_metadata": &step.metadata,
            "dependency_output_refs": dependency_refs,
            "plan_metadata": &input.metadata,
        });
        let result = client
            .execute_profile_step(RuntimeProfileInput {
                profile_id: step.profile_id.clone(),
                messages: step_messages,
                metadata: step_metadata,
                profile_contract: Some(step_contract),
                runtime_step: Some(step.clone()),
                requested_tools,
                trace_id: input.trace_id.clone(),
            })
            .await;

        match result {
            Ok(output) => {
                if let Err(error) = validate_runtime_step_output(&step.output_contract, &output) {
                    record_runtime_step_failure(
                        &mut step,
                        &mut completed_steps,
                        &mut step_summaries,
                        "step_output_invalid",
                    );
                    if step.fallback_policy == RuntimeStepFailurePolicy::Continue {
                        had_failed_continued_step = true;
                        continue;
                    }
                    return Err(error);
                }
                let Some(output_ref) = output.result_ref.clone() else {
                    let error = AgentCoreError::coded(
                        ErrorCode::Conflict,
                        "runtime step did not produce output_ref",
                    );
                    record_runtime_step_failure(
                        &mut step,
                        &mut completed_steps,
                        &mut step_summaries,
                        "step_missing_output_ref",
                    );
                    if step.fallback_policy == RuntimeStepFailurePolicy::Continue {
                        had_failed_continued_step = true;
                        continue;
                    }
                    return Err(error);
                };
                step.status = RuntimeStepStatus::Completed;
                step.output_ref = Some(output_ref.clone());
                output_refs.insert(step.step_id.clone(), output_ref.clone());
                step_summaries.push(json!({
                    "step_id": &step.step_id,
                    "profile_id": &step.profile_id,
                    "status": step.status,
                    "output_ref": &output_ref,
                }));
                completed_steps.push(step);
                last_output = Some(output);
            }
            Err(error) => {
                record_runtime_step_failure(
                    &mut step,
                    &mut completed_steps,
                    &mut step_summaries,
                    "step_failed",
                );
                if step.fallback_policy == RuntimeStepFailurePolicy::Continue {
                    had_failed_continued_step = true;
                    continue;
                }
                return Err(error);
            }
        }
    }

    let Some(mut output) = last_output else {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "runtime step plan produced no successful output",
        ));
    };
    input.plan.status = if had_failed_continued_step {
        RuntimeStepStatus::Failed
    } else {
        RuntimeStepStatus::Completed
    };
    input.plan.steps = completed_steps;
    if !output.metadata.is_object() {
        output.metadata = json!({});
    }
    output.metadata["runtime_step_plan"] = serde_json::to_value(&input.plan).map_err(|_| {
        AgentCoreError::coded(
            ErrorCode::InternalError,
            "runtime step plan metadata was not serializable",
        )
    })?;
    output.metadata["runtime_step_outputs"] = json!(step_summaries);
    Ok(output)
}

fn record_runtime_step_failure(
    step: &mut RuntimeStep,
    completed_steps: &mut Vec<RuntimeStep>,
    step_summaries: &mut Vec<Value>,
    error_code: &str,
) {
    step.status = RuntimeStepStatus::Failed;
    step_summaries.push(json!({
        "step_id": &step.step_id,
        "profile_id": &step.profile_id,
        "status": step.status,
        "error_code": error_code,
    }));
    completed_steps.push(step.clone());
}

fn json_schema_is_empty(schema: &Value) -> bool {
    schema.as_object().is_none_or(serde_json::Map::is_empty)
}

fn validate_runtime_step_output(schema: &Value, output: &RuntimeOutput) -> CoreResult<()> {
    if json_schema_is_empty(schema) {
        return Ok(());
    }
    let value = serde_json::to_value(output).map_err(|_| {
        AgentCoreError::coded(
            ErrorCode::InternalError,
            "runtime step output was not serializable",
        )
    })?;
    validate_json_schema_value(schema, &value)
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

fn schema_validation_error(reason: &'static str) -> AgentCoreError {
    AgentCoreError::coded(
        ErrorCode::Conflict,
        format!("schema validation failed: {reason}"),
    )
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
            return Err(schema_validation_error("type mismatch"));
        }
    }

    if let Some(values) = schema_object.get("enum").and_then(Value::as_array)
        && !values.iter().any(|item| item == value)
    {
        return Err(schema_validation_error("enum mismatch"));
    }

    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        let object = value
            .as_object()
            .ok_or_else(|| schema_validation_error("type mismatch"))?;
        for field in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(field) {
                return Err(schema_validation_error("missing required field"));
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
                    return Err(schema_validation_error("unexpected property"));
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
        return Err(schema_validation_error("minItems"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_validation_error_omits_controlled_field_names_and_values() {
        let error = validate_json_schema_value(
            &json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            &json!({"SECRET_UNEXPECTED_FIELD": "SECRET_UNEXPECTED_VALUE"}),
        )
        .unwrap_err();

        let encoded = error.to_string();
        assert!(encoded.contains("unexpected property"));
        assert!(!encoded.contains("SECRET_UNEXPECTED_FIELD"));
        assert!(!encoded.contains("SECRET_UNEXPECTED_VALUE"));

        let error = validate_json_schema_value(
            &json!({
                "type": "object",
                "required": ["SECRET_REQUIRED_FIELD"],
                "properties": {
                    "SECRET_PATH": {"type": "string"}
                }
            }),
            &json!({"SECRET_PATH": 42}),
        )
        .unwrap_err();

        let encoded = error.to_string();
        assert!(!encoded.contains("SECRET_REQUIRED_FIELD"));
        assert!(!encoded.contains("SECRET_PATH"));
        assert!(!encoded.contains("42"));
    }
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
