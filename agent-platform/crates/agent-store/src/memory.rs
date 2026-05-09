use crate::{AgentStore, store_error};
use agent_core::{
    AgentBridgeBinding, AgentBridgeBindingStatus, AgentCoreError, AgentGrant, AgentInstance,
    AgentInstanceStatus, AgentRequest, AgentRequestStatus, AgentRun, AgentRunStatus, AgentSession,
    AgentSessionMessage, AgentSummary, AgentTemplate, AppendMessageInput, ApprovalRequest,
    ApprovalStatus, AuditLog, CoreResult, CredentialLease, EmptyResponse, ErrorCode, MemoryStore,
    ObserverReport, ObserverReportSummary, ObserverSnapshot, ObserverSnapshotStore, ResourceLock,
    RunClaim, RunQueue, RunSummary, RuntimeOutput, SessionContext, SessionSummary, SideEffectPlan,
    validate_run_transition, validate_session_transition,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};
use time::OffsetDateTime;

#[derive(Debug, Default)]
struct Inner {
    templates: HashMap<String, AgentTemplate>,
    requests: HashMap<String, AgentRequest>,
    approvals: HashMap<String, ApprovalRequest>,
    agents: HashMap<String, AgentInstance>,
    sessions: HashMap<String, AgentSession>,
    open_webui_bridge_bindings: HashMap<String, AgentBridgeBinding>,
    open_webui_bridge_nonces: HashMap<(String, String, String, String), OffsetDateTime>,
    messages: HashMap<String, Vec<AgentSessionMessage>>,
    runs: HashMap<String, AgentRun>,
    audits: Vec<AuditLog>,
    reports: HashMap<String, ObserverReport>,
    side_effect_plans: HashMap<String, SideEffectPlan>,
    credential_leases: HashMap<String, CredentialLease>,
    grants: HashMap<String, AgentGrant>,
    locks: HashMap<(String, String, String), ResourceLock>,
    worker_heartbeats: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryAgentStore {
    inner: Arc<RwLock<Inner>>,
}

impl MemoryAgentStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn read(&self) -> CoreResult<std::sync::RwLockReadGuard<'_, Inner>> {
        self.inner.read().map_err(store_error)
    }

    fn write(&self) -> CoreResult<std::sync::RwLockWriteGuard<'_, Inner>> {
        self.inner.write().map_err(store_error)
    }
}

fn lease_until(lease: Duration) -> OffsetDateTime {
    OffsetDateTime::now_utc()
        + time::Duration::try_from(lease).unwrap_or_else(|_| time::Duration::seconds(30))
}

fn retry_backoff(retry_count: i32) -> time::Duration {
    match retry_count {
        count if count <= 1 => time::Duration::seconds(30),
        2 => time::Duration::seconds(120),
        _ => time::Duration::seconds(300),
    }
}

fn agent_summary(inner: &Inner, agent: &AgentInstance) -> AgentSummary {
    let active_session_count = inner
        .sessions
        .values()
        .filter(|session| {
            session.agent_id == agent.id
                && matches!(session.status, agent_core::AgentSessionStatus::Active)
        })
        .count() as i64;
    let last_run = inner
        .runs
        .values()
        .filter(|run| run.agent_id == agent.id)
        .max_by_key(|run| run.created_at);
    AgentSummary {
        agent_id: agent.id.clone(),
        agent_type: agent.agent_type.clone(),
        display_name: agent.display_name.clone(),
        target_resource: agent.target_resource.clone(),
        status: agent.status,
        allowed_actions: json!(["analyze", "prepare_change", "run_checks"]),
        active_session_count,
        last_run_status: last_run.map(|run| run.run_status),
        last_run_at: last_run.map(|run| run.created_at),
        trace_id: agent.trace_id.clone(),
    }
}

fn session_summary(session: &AgentSession) -> SessionSummary {
    SessionSummary {
        session_id: session.id.clone(),
        agent_id: session.agent_id.clone(),
        status: session.status,
        parent_session_id: session.parent_session_id.clone(),
        depth: session.depth,
        created_at: session.created_at,
        updated_at: session.updated_at,
        context_summary: session.context_summary.clone(),
        trace_id: session.trace_id.clone(),
    }
}

fn run_summary(run: &AgentRun) -> RunSummary {
    RunSummary {
        run_id: run.id.clone(),
        agent_id: run.agent_id.clone(),
        session_id: run.session_id.clone(),
        trigger_type: run.trigger_type,
        target_resource: run.target_resource.clone(),
        run_status: run.run_status,
        risk_level: run.risk_level,
        result_summary: run.result_summary.clone(),
        result_ref: run.result_ref.clone(),
        next_retry_at: run.next_retry_at,
        created_at: run.created_at,
        finished_at: run.finished_at,
        trace_id: run.trace_id.clone(),
    }
}

#[async_trait]
impl AgentStore for MemoryAgentStore {
    async fn bootstrap(&self) -> CoreResult<()> {
        let now = OffsetDateTime::now_utc();
        let mut inner = self.write()?;
        inner.templates.insert(
            agent_core::AGENT_TYPE_BACKGROUND_WORKER.to_string(),
            AgentTemplate::background_worker(now),
        );
        inner.templates.insert(
            agent_core::AGENT_TYPE_OBSERVER.to_string(),
            AgentTemplate::observer(now),
        );
        Ok(())
    }

    async fn upsert_template(&self, template: AgentTemplate) -> CoreResult<AgentTemplate> {
        self.write()?
            .templates
            .insert(template.agent_type.clone(), template.clone());
        Ok(template)
    }

    async fn get_template(&self, agent_type: &str) -> CoreResult<Option<AgentTemplate>> {
        Ok(self.read()?.templates.get(agent_type).cloned())
    }

    async fn create_agent_request(&self, request: AgentRequest) -> CoreResult<AgentRequest> {
        self.write()?
            .requests
            .insert(request.id.clone(), request.clone());
        Ok(request)
    }

    async fn get_agent_request(&self, request_id: &str) -> CoreResult<Option<AgentRequest>> {
        Ok(self.read()?.requests.get(request_id).cloned())
    }

    async fn find_agent_request_by_idempotency(
        &self,
        user_id: &str,
        service_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentRequest>> {
        Ok(self
            .read()?
            .requests
            .values()
            .find(|request| {
                request.requested_by_user == user_id
                    && request.requested_by_service == service_id
                    && request.idempotency_key.as_deref() == Some(idempotency_key)
            })
            .cloned())
    }

    async fn list_agent_requests(
        &self,
        user_id: Option<&str>,
        statuses: &[AgentRequestStatus],
        limit: i64,
    ) -> CoreResult<Vec<AgentRequest>> {
        let mut items: Vec<_> = self
            .read()?
            .requests
            .values()
            .filter(|request| user_id.is_none_or(|user| request.requested_by_user == user))
            .filter(|request| statuses.is_empty() || statuses.contains(&request.status))
            .cloned()
            .collect();
        items.sort_by_key(|request| std::cmp::Reverse(request.created_at));
        items.truncate(limit as usize);
        Ok(items)
    }

    async fn update_agent_request(&self, request: AgentRequest) -> CoreResult<AgentRequest> {
        self.write()?
            .requests
            .insert(request.id.clone(), request.clone());
        Ok(request)
    }

    async fn create_approval(&self, approval: ApprovalRequest) -> CoreResult<ApprovalRequest> {
        self.write()?
            .approvals
            .insert(approval.id.clone(), approval.clone());
        Ok(approval)
    }

    async fn get_approval(&self, approval_id: &str) -> CoreResult<Option<ApprovalRequest>> {
        Ok(self.read()?.approvals.get(approval_id).cloned())
    }

    async fn get_approval_by_request(
        &self,
        request_id: &str,
    ) -> CoreResult<Option<ApprovalRequest>> {
        Ok(self
            .read()?
            .approvals
            .values()
            .find(|approval| approval.request_id == request_id)
            .cloned())
    }

    async fn decide_approval(
        &self,
        approval_id: &str,
        approver_user: &str,
        status: ApprovalStatus,
        reason: Option<String>,
    ) -> CoreResult<ApprovalRequest> {
        let mut inner = self.write()?;
        let approval = inner
            .approvals
            .get_mut(approval_id)
            .ok_or_else(|| store_error("approval not found"))?;
        approval.status = status;
        approval.approver_user = Some(approver_user.to_string());
        approval.decision_reason = reason;
        approval.decided_at = Some(OffsetDateTime::now_utc());
        Ok(approval.clone())
    }

    async fn create_agent_instance(&self, agent: AgentInstance) -> CoreResult<AgentInstance> {
        self.write()?.agents.insert(agent.id.clone(), agent.clone());
        Ok(agent)
    }

    async fn find_reusable_agent(
        &self,
        owner_user: &str,
        agent_type: &str,
        target_resource: &str,
        core_constraints_hash: &str,
    ) -> CoreResult<Option<AgentInstance>> {
        Ok(self
            .read()?
            .agents
            .values()
            .find(|agent| {
                agent.owner_user == owner_user
                    && agent.agent_type == agent_type
                    && agent.target_resource == target_resource
                    && agent.core_constraints_hash == core_constraints_hash
                    && matches!(
                        agent.status,
                        AgentInstanceStatus::Provisioning
                            | AgentInstanceStatus::Running
                            | AgentInstanceStatus::Paused
                            | AgentInstanceStatus::Failed
                    )
            })
            .cloned())
    }

    async fn get_agent(&self, agent_id: &str) -> CoreResult<Option<AgentInstance>> {
        Ok(self.read()?.agents.get(agent_id).cloned())
    }

    async fn list_agents(
        &self,
        user_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<AgentSummary>> {
        let inner = self.read()?;
        let mut items: Vec<_> = inner
            .agents
            .values()
            .filter(|agent| user_id.is_none_or(|user| agent.owner_user == user))
            .map(|agent| agent_summary(&inner, agent))
            .collect();
        items.sort_by_key(|agent| {
            std::cmp::Reverse(agent.last_run_at.unwrap_or(
                agent_core::AgentTemplate::background_worker(OffsetDateTime::now_utc()).created_at,
            ))
        });
        items.truncate(limit as usize);
        Ok(items)
    }

    async fn update_agent_status(
        &self,
        agent_id: &str,
        status: AgentInstanceStatus,
        trace_id: &str,
    ) -> CoreResult<AgentInstance> {
        let mut inner = self.write()?;
        let agent = inner
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| store_error("agent not found"))?;
        agent.status = status;
        agent.trace_id = trace_id.to_string();
        agent.version += 1;
        agent.updated_at = OffsetDateTime::now_utc();
        Ok(agent.clone())
    }

    async fn create_session(&self, session: AgentSession) -> CoreResult<AgentSession> {
        self.write()?
            .sessions
            .insert(session.id.clone(), session.clone());
        Ok(session)
    }

    async fn get_session(&self, session_id: &str) -> CoreResult<Option<AgentSession>> {
        Ok(self.read()?.sessions.get(session_id).cloned())
    }

    async fn find_session_by_idempotency(
        &self,
        owner_user: &str,
        agent_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentSession>> {
        Ok(self
            .read()?
            .sessions
            .values()
            .find(|session| {
                session.owner_user == owner_user
                    && session.agent_id == agent_id
                    && session.idempotency_key.as_deref() == Some(idempotency_key)
            })
            .cloned())
    }

    async fn list_sessions(
        &self,
        user_id: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<SessionSummary>> {
        let mut items: Vec<_> = self
            .read()?
            .sessions
            .values()
            .filter(|session| user_id.is_none_or(|user| session.owner_user == user))
            .filter(|session| agent_id.is_none_or(|agent| session.agent_id == agent))
            .map(session_summary)
            .collect();
        items.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        items.truncate(limit as usize);
        Ok(items)
    }

    async fn list_child_sessions(
        &self,
        parent_session_id: &str,
    ) -> CoreResult<Vec<SessionSummary>> {
        Ok(self
            .read()?
            .sessions
            .values()
            .filter(|session| session.parent_session_id.as_deref() == Some(parent_session_id))
            .map(session_summary)
            .collect())
    }

    async fn close_session(&self, session_id: &str, trace_id: &str) -> CoreResult<AgentSession> {
        let mut inner = self.write()?;
        let session = inner
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| store_error("session not found"))?;
        validate_session_transition(session.status, agent_core::AgentSessionStatus::Closed)?;
        session.status = agent_core::AgentSessionStatus::Closed;
        session.trace_id = trace_id.to_string();
        session.updated_at = OffsetDateTime::now_utc();
        session.version += 1;
        Ok(session.clone())
    }

    async fn next_message_sequence(&self, session_id: &str) -> CoreResult<i64> {
        Ok(self
            .read()?
            .messages
            .get(session_id)
            .map(|messages| messages.len() as i64 + 1)
            .unwrap_or(1))
    }

    async fn get_open_webui_bridge_binding(
        &self,
        open_webui_subject: &str,
        open_webui_chat_id: &str,
        model: &str,
    ) -> CoreResult<Option<AgentBridgeBinding>> {
        Ok(self
            .read()?
            .open_webui_bridge_bindings
            .values()
            .find(|binding| {
                binding.open_webui_subject == open_webui_subject
                    && binding.open_webui_chat_id == open_webui_chat_id
                    && binding.model == model
                    && binding.status == AgentBridgeBindingStatus::Active
            })
            .cloned())
    }

    async fn upsert_open_webui_bridge_binding(
        &self,
        mut binding: AgentBridgeBinding,
    ) -> CoreResult<AgentBridgeBinding> {
        let mut inner = self.write()?;
        if let Some(existing) = inner
            .open_webui_bridge_bindings
            .values_mut()
            .find(|existing| {
                existing.open_webui_subject == binding.open_webui_subject
                    && existing.open_webui_chat_id == binding.open_webui_chat_id
                    && existing.model == binding.model
                    && existing.status == AgentBridgeBindingStatus::Active
            })
        {
            existing.open_webui_session_id = binding.open_webui_session_id.take();
            existing.agent_id = binding.agent_id;
            existing.agent_session_id = binding.agent_session_id;
            existing.last_message_id = binding.last_message_id;
            existing.trace_id = binding.trace_id;
            existing.version += 1;
            existing.updated_at = OffsetDateTime::now_utc();
            return Ok(existing.clone());
        }
        inner
            .open_webui_bridge_bindings
            .insert(binding.id.clone(), binding.clone());
        Ok(binding)
    }

    async fn close_open_webui_bridge_binding(
        &self,
        open_webui_subject: &str,
        open_webui_chat_id: &str,
        model: &str,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse> {
        let mut inner = self.write()?;
        if let Some(binding) = inner
            .open_webui_bridge_bindings
            .values_mut()
            .find(|binding| {
                binding.open_webui_subject == open_webui_subject
                    && binding.open_webui_chat_id == open_webui_chat_id
                    && binding.model == model
                    && binding.status == AgentBridgeBindingStatus::Active
            })
        {
            binding.status = AgentBridgeBindingStatus::Closed;
            binding.trace_id = trace_id.to_string();
            binding.closed_at = Some(OffsetDateTime::now_utc());
            binding.updated_at = OffsetDateTime::now_utc();
            binding.version += 1;
        }
        Ok(EmptyResponse {
            status: "closed".to_string(),
            trace_id: trace_id.to_string(),
        })
    }

    async fn update_open_webui_bridge_run(
        &self,
        open_webui_subject: &str,
        binding_id: &str,
        message_id: Option<&str>,
        run_id: &str,
        trace_id: &str,
    ) -> CoreResult<AgentBridgeBinding> {
        let mut inner = self.write()?;
        let binding = inner
            .open_webui_bridge_bindings
            .get_mut(binding_id)
            .ok_or_else(|| store_error("bridge binding not found"))?;
        if binding.open_webui_subject != open_webui_subject
            || binding.status != AgentBridgeBindingStatus::Active
        {
            return Err(store_error("bridge binding not found"));
        }
        binding.last_message_id = message_id.map(ToString::to_string);
        binding.last_run_id = Some(run_id.to_string());
        binding.trace_id = trace_id.to_string();
        binding.updated_at = OffsetDateTime::now_utc();
        binding.version += 1;
        Ok(binding.clone())
    }

    async fn claim_open_webui_bridge_nonce(
        &self,
        open_webui_subject: &str,
        open_webui_chat_id: &str,
        model: &str,
        nonce: &str,
        issued_at: i64,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse> {
        if open_webui_subject.trim().is_empty()
            || open_webui_chat_id.trim().is_empty()
            || model.trim().is_empty()
            || nonce.trim().is_empty()
        {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "bridge nonce fields must not be empty",
            ));
        }
        let issued_at = OffsetDateTime::from_unix_timestamp(issued_at).map_err(store_error)?;
        let key = (
            open_webui_subject.to_string(),
            open_webui_chat_id.to_string(),
            model.to_string(),
            nonce.to_string(),
        );
        let mut inner = self.write()?;
        if inner.open_webui_bridge_nonces.contains_key(&key) {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "bridge nonce replay",
            ));
        }
        inner.open_webui_bridge_nonces.insert(key, issued_at);
        Ok(EmptyResponse {
            status: "claimed".to_string(),
            trace_id: trace_id.to_string(),
        })
    }

    async fn create_run(&self, run: AgentRun) -> CoreResult<AgentRun> {
        self.write()?.runs.insert(run.id.clone(), run.clone());
        Ok(run)
    }

    async fn get_run(&self, run_id: &str) -> CoreResult<Option<AgentRun>> {
        Ok(self.read()?.runs.get(run_id).cloned())
    }

    async fn find_run_by_idempotency(
        &self,
        agent_id: &str,
        idempotency_key: &str,
    ) -> CoreResult<Option<AgentRun>> {
        Ok(self
            .read()?
            .runs
            .values()
            .find(|run| {
                run.agent_id == agent_id && run.idempotency_key.as_deref() == Some(idempotency_key)
            })
            .cloned())
    }

    async fn list_runs(
        &self,
        user_id: Option<&str>,
        agent_id: Option<&str>,
        limit: i64,
    ) -> CoreResult<Vec<RunSummary>> {
        let inner = self.read()?;
        let mut items: Vec<_> = inner
            .runs
            .values()
            .filter(|run| agent_id.is_none_or(|agent| run.agent_id == agent))
            .filter(|run| {
                user_id.is_none_or(|user| {
                    inner
                        .agents
                        .get(&run.agent_id)
                        .is_some_and(|agent| agent.owner_user == user)
                })
            })
            .map(run_summary)
            .collect();
        items.sort_by_key(|run| std::cmp::Reverse(run.created_at));
        items.truncate(limit as usize);
        Ok(items)
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        status: AgentRunStatus,
        trace_id: &str,
    ) -> CoreResult<AgentRun> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        validate_run_transition(run.run_status, status)?;
        run.run_status = status;
        run.trace_id = trace_id.to_string();
        run.next_retry_at = None;
        run.version += 1;
        if matches!(
            status,
            AgentRunStatus::Completed
                | AgentRunStatus::Failed
                | AgentRunStatus::TimedOut
                | AgentRunStatus::DeadLetter
                | AgentRunStatus::Cancelled
        ) {
            run.finished_at = Some(OffsetDateTime::now_utc());
        }
        Ok(run.clone())
    }

    async fn retry_run(&self, run_id: &str, reason: &str, trace_id: &str) -> CoreResult<AgentRun> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        validate_run_transition(run.run_status, AgentRunStatus::Queued)?;
        run.run_status = AgentRunStatus::Queued;
        run.retry_count = 0;
        run.result_summary = Some(reason.to_string());
        run.lease_owner = None;
        run.lease_until = None;
        run.next_retry_at = None;
        run.finished_at = None;
        run.trace_id = trace_id.to_string();
        run.version += 1;
        Ok(run.clone())
    }

    async fn terminate_run(
        &self,
        run_id: &str,
        reason: &str,
        trace_id: &str,
    ) -> CoreResult<AgentRun> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        validate_run_transition(run.run_status, AgentRunStatus::Cancelled)?;
        run.run_status = AgentRunStatus::Cancelled;
        run.result_summary = Some(reason.to_string());
        run.lease_owner = None;
        run.lease_until = None;
        run.next_retry_at = None;
        run.finished_at = Some(OffsetDateTime::now_utc());
        run.trace_id = trace_id.to_string();
        run.version += 1;
        Ok(run.clone())
    }

    async fn append_audit(&self, audit: AuditLog) -> CoreResult<AuditLog> {
        self.write()?.audits.push(audit.clone());
        Ok(audit)
    }

    async fn list_audit(&self, limit: i64) -> CoreResult<Vec<AuditLog>> {
        let mut items = self.read()?.audits.clone();
        items.sort_by_key(|audit| std::cmp::Reverse(audit.created_at));
        items.truncate(limit as usize);
        Ok(items)
    }

    async fn create_observer_report(&self, report: ObserverReport) -> CoreResult<ObserverReport> {
        self.write()?
            .reports
            .insert(report.id.clone(), report.clone());
        Ok(report)
    }

    async fn list_observer_reports(&self, limit: i64) -> CoreResult<Vec<ObserverReportSummary>> {
        let mut items: Vec<_> = self
            .read()?
            .reports
            .values()
            .map(|report| ObserverReportSummary {
                report_id: report.id.clone(),
                observer_run_id: report.observer_run_id.clone(),
                health_status: report.health_status,
                risk_level: report.risk_level,
                summary: report.summary.clone(),
                created_at: report.created_at,
                trace_id: report.trace_id.clone(),
            })
            .collect();
        items.sort_by_key(|report| std::cmp::Reverse(report.created_at));
        items.truncate(limit as usize);
        Ok(items)
    }

    async fn get_observer_report(&self, report_id: &str) -> CoreResult<Option<ObserverReport>> {
        Ok(self.read()?.reports.get(report_id).cloned())
    }

    async fn create_side_effect_plan(&self, plan: SideEffectPlan) -> CoreResult<SideEffectPlan> {
        self.write()?
            .side_effect_plans
            .insert(plan.id.clone(), plan.clone());
        Ok(plan)
    }

    async fn list_side_effect_plans_by_run(&self, run_id: &str) -> CoreResult<Vec<SideEffectPlan>> {
        let mut items: Vec<_> = self
            .read()?
            .side_effect_plans
            .values()
            .filter(|plan| plan.run_id == run_id)
            .cloned()
            .collect();
        items.sort_by_key(|plan| std::cmp::Reverse(plan.created_at));
        Ok(items)
    }

    async fn create_credential_lease(&self, lease: CredentialLease) -> CoreResult<CredentialLease> {
        self.write()?
            .credential_leases
            .insert(lease.id.clone(), lease.clone());
        Ok(lease)
    }

    async fn list_credential_leases_by_plan(
        &self,
        plan_id: &str,
    ) -> CoreResult<Vec<CredentialLease>> {
        let mut items: Vec<_> = self
            .read()?
            .credential_leases
            .values()
            .filter(|lease| lease.side_effect_plan_id == plan_id)
            .cloned()
            .collect();
        items.sort_by_key(|lease| std::cmp::Reverse(lease.created_at));
        Ok(items)
    }

    async fn create_grant(&self, grant: AgentGrant) -> CoreResult<AgentGrant> {
        self.write()?.grants.insert(grant.id.clone(), grant.clone());
        Ok(grant)
    }

    async fn acquire_resource_lock(
        &self,
        mut lock: ResourceLock,
        lease: Duration,
    ) -> CoreResult<ResourceLock> {
        let mut inner = self.write()?;
        let key = (
            lock.resource_type.clone(),
            lock.resource_id.clone(),
            lock.lock_scope.clone(),
        );
        let now = OffsetDateTime::now_utc();
        if let Some(existing) = inner.locks.get(&key)
            && existing.lease_until > now
            && existing.holder_run_id != lock.holder_run_id
        {
            return Err(agent_core::AgentCoreError::coded(
                agent_core::ErrorCode::Conflict,
                "resource lock is already held",
            ));
        }
        lock.lease_until = lease_until(lease);
        inner.locks.insert(key, lock.clone());
        Ok(lock)
    }

    async fn active_resource_lock(
        &self,
        resource_type: &str,
        resource_id: &str,
        lock_scope: &str,
    ) -> CoreResult<Option<ResourceLock>> {
        let key = (
            resource_type.to_string(),
            resource_id.to_string(),
            lock_scope.to_string(),
        );
        let now = OffsetDateTime::now_utc();
        Ok(self
            .read()?
            .locks
            .get(&key)
            .filter(|lock| lock.lease_until > now)
            .cloned())
    }

    async fn release_resource_lock(&self, run_id: &str) -> CoreResult<EmptyResponse> {
        let mut inner = self.write()?;
        inner
            .locks
            .retain(|_, lock| lock.holder_run_id.as_str() != run_id);
        Ok(EmptyResponse {
            status: "released".to_string(),
            trace_id: agent_core::new_trace_id(),
        })
    }

    async fn record_worker_heartbeat(
        &self,
        worker_id: &str,
        current_run_id: Option<&str>,
        status: &str,
        trace_id: &str,
    ) -> CoreResult<EmptyResponse> {
        self.write()?.worker_heartbeats.insert(
            worker_id.to_string(),
            json!({
                "worker_id": worker_id,
                "current_run_id": current_run_id,
                "status": status,
                "trace_id": trace_id,
                "last_seen_at": OffsetDateTime::now_utc(),
            }),
        );
        Ok(EmptyResponse {
            status: "recorded".to_string(),
            trace_id: trace_id.to_string(),
        })
    }
}

#[async_trait]
impl MemoryStore for MemoryAgentStore {
    async fn append_message(
        &self,
        message: AgentSessionMessage,
    ) -> CoreResult<AgentSessionMessage> {
        let mut inner = self.write()?;
        let messages = inner
            .messages
            .entry(message.session_id.clone())
            .or_default();
        if let Some(external_message_id) = message.external_message_id.as_deref()
            && let Some(existing) = messages.iter().find(|existing| {
                existing.external_message_id.as_deref() == Some(external_message_id)
            })
        {
            return Ok(existing.clone());
        }
        messages.push(message.clone());
        if let Some(session) = inner.sessions.get_mut(&message.session_id) {
            session.updated_at = OffsetDateTime::now_utc();
            session.trace_id = message.trace_id.clone();
            session.version += 1;
        }
        Ok(message)
    }

    async fn session_context(
        &self,
        session_id: &str,
        trace_id: &str,
    ) -> CoreResult<SessionContext> {
        let inner = self.read()?;
        let session = inner
            .sessions
            .get(session_id)
            .ok_or_else(|| store_error("session not found"))?;
        let mut recent_messages: Vec<_> = inner
            .messages
            .get(session_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .rev()
            .take(30)
            .map(|message| AppendMessageInput {
                role: message.role,
                content_summary: message.content_summary.unwrap_or_default(),
                content_ref: message.content_ref,
                external_message_id: message.external_message_id,
                run_id: message.run_id,
            })
            .collect();
        recent_messages.reverse();
        Ok(SessionContext {
            session_id: session.id.clone(),
            agent_id: session.agent_id.clone(),
            context_summary: session.context_summary.clone(),
            recent_messages,
            resource_scope: session.resource_scope.clone(),
            trace_id: trace_id.to_string(),
        })
    }

    async fn write_summary(
        &self,
        session_id: &str,
        summary: &str,
        trace_id: &str,
    ) -> CoreResult<()> {
        let mut inner = self.write()?;
        let session = inner
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| store_error("session not found"))?;
        session.context_summary = Some(summary.to_string());
        session.trace_id = trace_id.to_string();
        session.updated_at = OffsetDateTime::now_utc();
        session.version += 1;
        Ok(())
    }

    async fn write_result_ref(
        &self,
        run_id: &str,
        result_summary: &str,
        result_ref: Option<&str>,
        trace_id: &str,
    ) -> CoreResult<()> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        run.result_summary = Some(result_summary.to_string());
        run.result_ref = result_ref.map(ToString::to_string);
        run.trace_id = trace_id.to_string();
        run.version += 1;
        Ok(())
    }
}

#[async_trait]
impl RunQueue for MemoryAgentStore {
    async fn enqueue_run(&self, run: AgentRun) -> CoreResult<AgentRun> {
        self.create_run(run).await
    }

    async fn claim_next_run(
        &self,
        worker_id: &str,
        lease: Duration,
    ) -> CoreResult<Option<RunClaim>> {
        let mut inner = self.write()?;
        let now = OffsetDateTime::now_utc();
        let Some(run) = inner
            .runs
            .values_mut()
            .filter(|run| run.run_status == AgentRunStatus::Queued)
            .filter(|run| run.next_retry_at.is_none_or(|retry_at| retry_at <= now))
            .min_by_key(|run| (run.next_retry_at.unwrap_or(run.created_at), run.created_at))
        else {
            return Ok(None);
        };
        validate_run_transition(run.run_status, AgentRunStatus::Claimed)?;
        run.run_status = AgentRunStatus::Claimed;
        run.lease_owner = Some(worker_id.to_string());
        run.lease_until = Some(lease_until(lease));
        run.next_retry_at = None;
        run.claimed_at = Some(now);
        run.version += 1;
        Ok(Some(RunClaim {
            run: run.clone(),
            lease_owner: worker_id.to_string(),
            lease_seconds: lease.as_secs() as i64,
        }))
    }

    async fn heartbeat_run(
        &self,
        run_id: &str,
        worker_id: &str,
        lease: Duration,
    ) -> CoreResult<()> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        if run.lease_owner.as_deref() != Some(worker_id) {
            return Err(agent_core::AgentCoreError::coded(
                agent_core::ErrorCode::Conflict,
                "lease owner mismatch",
            ));
        }
        run.lease_until = Some(lease_until(lease));
        run.version += 1;
        Ok(())
    }

    async fn finish_run(&self, run_id: &str, output: RuntimeOutput) -> CoreResult<AgentRun> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        run.run_status = AgentRunStatus::Completed;
        run.result_summary = Some(output.result_summary);
        run.result_ref = output.result_ref;
        run.finished_at = Some(OffsetDateTime::now_utc());
        run.lease_owner = None;
        run.lease_until = None;
        run.next_retry_at = None;
        run.version += 1;
        Ok(run.clone())
    }

    async fn fail_or_retry_run(
        &self,
        run_id: &str,
        reason: &str,
        max_retries: i32,
    ) -> CoreResult<AgentRun> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        if run.retry_count >= max_retries {
            run.run_status = AgentRunStatus::DeadLetter;
            run.finished_at = Some(OffsetDateTime::now_utc());
            run.next_retry_at = None;
        } else {
            run.run_status = AgentRunStatus::Queued;
            run.retry_count += 1;
            run.next_retry_at = Some(OffsetDateTime::now_utc() + retry_backoff(run.retry_count));
        }
        run.result_summary = Some(reason.to_string());
        run.lease_owner = None;
        run.lease_until = None;
        run.version += 1;
        Ok(run.clone())
    }

    async fn dead_letter_run(&self, run_id: &str, reason: &str) -> CoreResult<AgentRun> {
        let mut inner = self.write()?;
        let run = inner
            .runs
            .get_mut(run_id)
            .ok_or_else(|| store_error("run not found"))?;
        run.run_status = AgentRunStatus::DeadLetter;
        run.result_summary = Some(reason.to_string());
        run.finished_at = Some(OffsetDateTime::now_utc());
        run.lease_owner = None;
        run.lease_until = None;
        run.next_retry_at = None;
        run.version += 1;
        Ok(run.clone())
    }

    async fn sweep_expired_leases(&self, max_retries: i32) -> CoreResult<Vec<RunSummary>> {
        let mut inner = self.write()?;
        let now = OffsetDateTime::now_utc();
        let mut swept = Vec::new();
        for run in inner.runs.values_mut() {
            if matches!(
                run.run_status,
                AgentRunStatus::Claimed
                    | AgentRunStatus::ContextBuilt
                    | AgentRunStatus::PolicyChecked
                    | AgentRunStatus::Executing
                    | AgentRunStatus::Validating
                    | AgentRunStatus::ApplyingSideEffects
            ) && run.lease_until.is_some_and(|lease| lease < now)
            {
                if run.retry_count >= max_retries {
                    run.run_status = AgentRunStatus::DeadLetter;
                    run.finished_at = Some(now);
                    run.next_retry_at = None;
                } else {
                    run.run_status = AgentRunStatus::Queued;
                    run.retry_count += 1;
                    run.next_retry_at = Some(now + retry_backoff(run.retry_count));
                }
                run.lease_owner = None;
                run.lease_until = None;
                run.result_summary = Some("lease expired".to_string());
                run.version += 1;
                swept.push(run_summary(run));
            }
        }
        Ok(swept)
    }
}

#[async_trait]
impl ObserverSnapshotStore for MemoryAgentStore {
    async fn collect_observer_snapshot(&self, _trace_id: &str) -> CoreResult<ObserverSnapshot> {
        let inner = self.read()?;
        let count_by = |values: Vec<String>| -> Value {
            let mut counts = serde_json::Map::new();
            for value in values {
                let current = counts.get(&value).and_then(Value::as_i64).unwrap_or(0);
                counts.insert(value, json!(current + 1));
            }
            Value::Object(counts)
        };
        let completed_latencies_ms: Vec<i128> = inner
            .runs
            .values()
            .filter_map(|run| run.finished_at.zip(run.claimed_at))
            .map(|(finished_at, claimed_at)| (finished_at - claimed_at).whole_milliseconds())
            .collect();
        let avg_runtime_ms = if completed_latencies_ms.is_empty() {
            0.0
        } else {
            completed_latencies_ms.iter().sum::<i128>() as f64 / completed_latencies_ms.len() as f64
        };
        let max_context_messages = inner
            .messages
            .values()
            .map(Vec::len)
            .max()
            .unwrap_or_default();
        Ok(ObserverSnapshot {
            collected_at: OffsetDateTime::now_utc(),
            agent_counts: count_by(
                inner
                    .agents
                    .values()
                    .map(|agent| agent.status.to_string())
                    .collect(),
            ),
            session_counts: count_by(
                inner
                    .sessions
                    .values()
                    .map(|session| session.status.to_string())
                    .collect(),
            ),
            run_counts: count_by(
                inner
                    .runs
                    .values()
                    .map(|run| run.run_status.to_string())
                    .collect(),
            ),
            runtime_summary: json!({
                "retrying_runs": inner.runs.values().filter(|run| run.retry_count > 0).count(),
                "max_retry_count": inner.runs.values().map(|run| run.retry_count).max().unwrap_or_default(),
                "timed_out_runs": inner.runs.values().filter(|run| run.run_status == AgentRunStatus::TimedOut).count(),
                "avg_completed_runtime_ms": avg_runtime_ms,
                "max_context_messages": max_context_messages,
                "side_effect_plan_counts": count_by(
                    inner
                        .side_effect_plans
                        .values()
                        .map(|plan| plan.status.to_string())
                        .collect(),
                ),
            }),
            lock_summary: json!({
                "active_locks": inner.locks.len(),
                "locks": inner.locks.values().map(|lock| {
                    json!({
                        "resource_type": lock.resource_type,
                        "resource_id": lock.resource_id,
                        "lock_scope": lock.lock_scope,
                        "holder_run_id": lock.holder_run_id,
                        "lease_until": lock.lease_until,
                    })
                }).collect::<Vec<_>>()
            }),
            audit_summary: json!({
                "audit_count": inner.audits.len(),
                "recent_decisions": inner.audits.iter().rev().take(20).map(|audit| {
                    json!({
                        "action": audit.action,
                        "decision": audit.decision.map(|decision| decision.to_string()),
                        "trace_id": audit.trace_id,
                    })
                }).collect::<Vec<_>>()
            }),
            worker_summary: json!({
                "workers": inner.worker_heartbeats.values().cloned().collect::<Vec<_>>(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{AgentBridgeBinding, AgentRun, TriggerType, new_trace_id};

    #[tokio::test]
    async fn claim_uses_single_lease_owner() {
        let store = MemoryAgentStore::new();
        let run = AgentRun::new(
            "agent-1",
            None,
            TriggerType::Manual,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        store.enqueue_run(run).await.unwrap();

        let first = store
            .claim_next_run("worker-1", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(first.is_some());

        let second = store
            .claim_next_run("worker-2", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn observer_snapshot_is_aggregate_only() {
        let store = MemoryAgentStore::new();
        store.bootstrap().await.unwrap();
        let snapshot = store.collect_observer_snapshot("trace-test").await.unwrap();
        assert_eq!(snapshot.lock_summary["active_locks"], json!(0));
        assert!(snapshot.audit_summary.get("recent_decisions").is_some());
        let _ = agent_core::HealthStatus::Healthy;
    }

    #[tokio::test]
    async fn retry_failure_defers_next_claim() {
        let store = MemoryAgentStore::new();
        let run = AgentRun::new(
            "agent-1",
            None,
            TriggerType::Manual,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        let run = store.enqueue_run(run).await.unwrap();
        let now = OffsetDateTime::now_utc();

        let claim = store
            .claim_next_run("worker-1", Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.run.id, run.id);

        let retry = store
            .fail_or_retry_run(&run.id, "runtime failed", 3)
            .await
            .unwrap();
        assert_eq!(retry.run_status, AgentRunStatus::Queued);
        assert_eq!(retry.retry_count, 1);
        assert!(retry.next_retry_at.is_some_and(|retry_at| retry_at > now));

        let deferred_claim = store
            .claim_next_run("worker-2", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(deferred_claim.is_none());
    }

    #[tokio::test]
    async fn admin_retry_requeues_dead_letter_run_immediately() {
        let store = MemoryAgentStore::new();
        let run = AgentRun::new(
            "agent-1",
            None,
            TriggerType::Manual,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        let run = store.enqueue_run(run).await.unwrap();
        let dead_letter = store
            .dead_letter_run(&run.id, "runtime exhausted")
            .await
            .unwrap();
        assert_eq!(dead_letter.run_status, AgentRunStatus::DeadLetter);

        let manual_retry = store
            .retry_run(&run.id, "admin retry", "trace-admin")
            .await
            .unwrap();
        assert_eq!(manual_retry.run_status, AgentRunStatus::Queued);
        assert_eq!(manual_retry.retry_count, 0);
        assert!(manual_retry.next_retry_at.is_none());

        let immediate_claim = store
            .claim_next_run("worker-3", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(immediate_claim.is_some());
    }

    #[tokio::test]
    async fn bridge_binding_is_user_chat_model_scoped_and_reopenable() {
        let store = MemoryAgentStore::new();
        let trace_id = new_trace_id();
        let binding = AgentBridgeBinding::new(
            "openwebui:user-1",
            "chat-1",
            Some("ow-session-1".to_string()),
            "hermes-agent",
            "agent-1",
            "sess-1",
            trace_id.clone(),
        );
        let binding = store
            .upsert_open_webui_bridge_binding(binding)
            .await
            .unwrap();

        let found = store
            .get_open_webui_bridge_binding("openwebui:user-1", "chat-1", "hermes-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, binding.id);

        let other_user = store
            .get_open_webui_bridge_binding("openwebui:user-2", "chat-1", "hermes-agent")
            .await
            .unwrap();
        assert!(other_user.is_none());

        store
            .update_open_webui_bridge_run(
                "openwebui:user-1",
                &binding.id,
                Some("msg-1"),
                "run-1",
                &trace_id,
            )
            .await
            .unwrap();
        let updated = store
            .get_open_webui_bridge_binding("openwebui:user-1", "chat-1", "hermes-agent")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.last_run_id.as_deref(), Some("run-1"));

        store
            .close_open_webui_bridge_binding(
                "openwebui:user-1",
                "chat-1",
                "hermes-agent",
                &trace_id,
            )
            .await
            .unwrap();
        assert!(
            store
                .get_open_webui_bridge_binding("openwebui:user-1", "chat-1", "hermes-agent")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .update_open_webui_bridge_run(
                    "openwebui:user-1",
                    &binding.id,
                    Some("msg-closed"),
                    "run-closed",
                    &trace_id,
                )
                .await
                .is_err()
        );

        let reopened = AgentBridgeBinding::new(
            "openwebui:user-1",
            "chat-1",
            None,
            "hermes-agent",
            "agent-1",
            "sess-2",
            trace_id,
        );
        let reopened = store
            .upsert_open_webui_bridge_binding(reopened)
            .await
            .unwrap();
        assert_eq!(reopened.agent_session_id, "sess-2");
    }

    #[tokio::test]
    async fn bridge_nonce_claim_rejects_replay() {
        let store = MemoryAgentStore::new();
        let trace_id = new_trace_id();
        store
            .claim_open_webui_bridge_nonce(
                "openwebui:user-1",
                "chat-1",
                "hermes-agent",
                "nonce-1",
                OffsetDateTime::now_utc().unix_timestamp(),
                &trace_id,
            )
            .await
            .unwrap();

        let replay = store
            .claim_open_webui_bridge_nonce(
                "openwebui:user-1",
                "chat-1",
                "hermes-agent",
                "nonce-1",
                OffsetDateTime::now_utc().unix_timestamp(),
                &trace_id,
            )
            .await;
        assert!(replay.is_err());
    }

    #[tokio::test]
    async fn append_message_dedupes_external_message_id() {
        let store = MemoryAgentStore::new();
        let trace_id = new_trace_id();
        let mut first = AgentSessionMessage::new(
            "sess-1",
            1,
            agent_core::MessageRole::User,
            Some("hello".to_string()),
            None,
            trace_id.clone(),
        );
        first.external_message_id = Some("openwebui:msg-1".to_string());
        let first = store.append_message(first).await.unwrap();

        let mut duplicate = AgentSessionMessage::new(
            "sess-1",
            2,
            agent_core::MessageRole::User,
            Some("hello again".to_string()),
            None,
            trace_id,
        );
        duplicate.external_message_id = Some("openwebui:msg-1".to_string());
        let duplicate = store.append_message(duplicate).await.unwrap();

        assert_eq!(duplicate.id, first.id);
        assert_eq!(
            store.read().unwrap().messages.get("sess-1").map(Vec::len),
            Some(1)
        );
    }
}
