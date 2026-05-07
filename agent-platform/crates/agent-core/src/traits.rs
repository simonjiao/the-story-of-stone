use crate::{
    AgentCoreError, AgentRun, AgentSessionMessage, CoreResult, ObserverSnapshot, RunSummary,
    SessionContext,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRunInput {
    pub run: AgentRun,
    pub context: Option<SessionContext>,
    pub snapshot: Option<Value>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSessionInput {
    pub session_id: String,
    pub agent_id: String,
    pub message: AgentSessionMessage,
    pub context: SessionContext,
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
