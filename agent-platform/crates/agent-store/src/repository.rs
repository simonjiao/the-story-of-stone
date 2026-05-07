use agent_core::{
    AgentCoreError, AgentGrant, AgentInstance, AgentInstanceStatus, AgentRequest,
    AgentRequestStatus, AgentRun, AgentRunStatus, AgentSession, AgentSummary, AgentTemplate,
    ApprovalRequest, ApprovalStatus, AuditLog, CoreResult, EmptyResponse, ObserverReport,
    ObserverReportSummary, ResourceLock, RunSummary, SessionSummary,
};
use async_trait::async_trait;
use std::time::Duration;

pub fn store_error(error: impl std::fmt::Display) -> AgentCoreError {
    AgentCoreError::coded(agent_core::ErrorCode::InternalError, error.to_string())
}

#[async_trait]
pub trait AgentStore:
    agent_core::MemoryStore + agent_core::RunQueue + agent_core::ObserverSnapshotStore + Send + Sync
{
    async fn bootstrap(&self) -> CoreResult<()>;

    async fn upsert_template(&self, template: AgentTemplate) -> CoreResult<AgentTemplate>;
    async fn get_template(&self, agent_type: &str) -> CoreResult<Option<AgentTemplate>>;

    async fn create_agent_request(&self, request: AgentRequest) -> CoreResult<AgentRequest>;
    async fn get_agent_request(&self, request_id: &str) -> CoreResult<Option<AgentRequest>>;
    async fn find_agent_request_by_idempotency(
        &self,
        user_id: &str,
        service_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentRequest>>;
    async fn list_agent_requests(
        &self,
        user_id: Option<&str>,
        statuses: &[AgentRequestStatus],
        limit: i64,
    ) -> CoreResult<Vec<AgentRequest>>;
    async fn update_agent_request(&self, request: AgentRequest) -> CoreResult<AgentRequest>;

    async fn create_approval(&self, approval: ApprovalRequest) -> CoreResult<ApprovalRequest>;
    async fn get_approval(&self, approval_id: &str) -> CoreResult<Option<ApprovalRequest>>;
    async fn get_approval_by_request(
        &self,
        request_id: &str,
    ) -> CoreResult<Option<ApprovalRequest>>;
    async fn decide_approval(
        &self,
        approval_id: &str,
        approver_user: &str,
        status: ApprovalStatus,
        reason: Option<String>,
    ) -> CoreResult<ApprovalRequest>;

    async fn create_agent_instance(&self, agent: AgentInstance) -> CoreResult<AgentInstance>;
    async fn find_reusable_agent(
        &self,
        owner_user: &str,
        agent_type: &str,
        target_resource: &str,
        core_constraints_hash: &str,
    ) -> CoreResult<Option<AgentInstance>>;
    async fn get_agent(&self, agent_id: &str) -> CoreResult<Option<AgentInstance>>;
    async fn list_agents(&self, user_id: Option<&str>, limit: i64)
    -> CoreResult<Vec<AgentSummary>>;
    async fn update_agent_status(
        &self,
        agent_id: &str,
        status: AgentInstanceStatus,
        trace_id: &str,
    ) -> CoreResult<AgentInstance>;

    async fn create_session(&self, session: AgentSession) -> CoreResult<AgentSession>;
    async fn get_session(&self, session_id: &str) -> CoreResult<Option<AgentSession>>;
    async fn list_sessions(
        &self,
        user_id: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<SessionSummary>>;
    async fn list_child_sessions(&self, parent_session_id: &str)
    -> CoreResult<Vec<SessionSummary>>;
    async fn close_session(&self, session_id: &str, trace_id: &str) -> CoreResult<AgentSession>;
    async fn next_message_sequence(&self, session_id: &str) -> CoreResult<i64>;

    async fn create_run(&self, run: AgentRun) -> CoreResult<AgentRun>;
    async fn get_run(&self, run_id: &str) -> CoreResult<Option<AgentRun>>;
    async fn list_runs(
        &self,
        user_id: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<RunSummary>>;
    async fn update_run_status(
        &self,
        run_id: &str,
        status: AgentRunStatus,
        trace_id: &str,
    ) -> CoreResult<AgentRun>;

    async fn append_audit(&self, audit: AuditLog) -> CoreResult<AuditLog>;
    async fn list_audit(&self, limit: i64) -> CoreResult<Vec<AuditLog>>;

    async fn create_observer_report(&self, report: ObserverReport) -> CoreResult<ObserverReport>;
    async fn list_observer_reports(&self, limit: i64) -> CoreResult<Vec<ObserverReportSummary>>;
    async fn get_observer_report(&self, report_id: &str) -> CoreResult<Option<ObserverReport>>;

    async fn create_grant(&self, grant: AgentGrant) -> CoreResult<AgentGrant>;

    async fn acquire_resource_lock(
        &self,
        lock: ResourceLock,
        lease: Duration,
    ) -> CoreResult<ResourceLock>;
    async fn release_resource_lock(&self, run_id: &str) -> CoreResult<EmptyResponse>;
    async fn record_worker_heartbeat(
        &self,
        worker_id: &str,
        current_run_id: Option<&str>,
        status: &str,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse>;
}
