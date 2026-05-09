use crate::{
    AgentCoreError, AgentInstance, AgentRun, AgentSessionMessage, CoreResult, CredentialLease,
    ExternalActionMode, ExternalActionPlan, ObserverSnapshot, RiskLevel, RunSummary,
    SessionContext,
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

#[async_trait]
pub trait RuntimeClient: Send + Sync {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput>;

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput>;
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
