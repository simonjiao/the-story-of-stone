use agent_core::{
    AgentCoreError, AgentSessionMessage, CoreResult, ErrorCode, MessageRole, RuntimeClient,
    RuntimeOutput, RuntimeRunInput, RuntimeSessionInput, SideEffectMode, runtime_failure,
};
use async_trait::async_trait;
use serde_json::{Value, json};

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

pub fn runtime_error(error: impl std::fmt::Display) -> agent_core::AgentCoreError {
    runtime_failure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{AgentRun, TriggerType, new_trace_id};

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
                context: None,
                snapshot: None,
            })
            .await;
        assert!(result.is_err());
    }
}
