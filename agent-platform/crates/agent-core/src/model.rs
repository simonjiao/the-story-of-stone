use crate::{AgentCoreError, CoreResult, new_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fmt, str::FromStr};
use time::OffsetDateTime;

macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident => $value:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = AgentCoreError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    other => Err(AgentCoreError::InvalidEnum {
                        kind: stringify!($name),
                        value: other.to_string(),
                    }),
                }
            }
        }
    };
}

string_enum! {
    pub enum RoleName {
        SystemAdmin => "system_admin",
        AgentAdmin => "agent_admin",
        ResourceOwner => "resource_owner",
        ResourceMaintainer => "resource_maintainer",
        Operator => "operator",
        Viewer => "viewer",
    }
}

string_enum! {
    pub enum AgentTemplateStatus {
        Active => "active",
        Disabled => "disabled",
    }
}

string_enum! {
    pub enum AgentRequestStatus {
        Requested => "requested",
        Parsed => "parsed",
        PolicyChecked => "policy_checked",
        Denied => "denied",
        ApprovalRequired => "approval_required",
        Approved => "approved",
        Provisioning => "provisioning",
        Enqueued => "enqueued",
        Fulfilled => "fulfilled",
        Cancelled => "cancelled",
        Expired => "expired",
        Failed => "failed",
    }
}

string_enum! {
    pub enum AgentInstanceStatus {
        Provisioning => "provisioning",
        Running => "running",
        Paused => "paused",
        Terminated => "terminated",
        Failed => "failed",
    }
}

string_enum! {
    pub enum AgentSessionStatus {
        Created => "created",
        Active => "active",
        Closing => "closing",
        Closed => "closed",
        Expired => "expired",
        Failed => "failed",
    }
}

string_enum! {
    pub enum AgentRunStatus {
        Queued => "queued",
        Claimed => "claimed",
        ContextBuilt => "context_built",
        PolicyChecked => "policy_checked",
        Executing => "executing",
        Validating => "validating",
        AwaitingApproval => "awaiting_approval",
        ApplyingSideEffects => "applying_side_effects",
        Completed => "completed",
        Failed => "failed",
        Cancelled => "cancelled",
        TimedOut => "timed_out",
        DeadLetter => "dead_letter",
    }
}

string_enum! {
    pub enum ApprovalStatus {
        Pending => "pending",
        Approved => "approved",
        Denied => "denied",
        Cancelled => "cancelled",
        Expired => "expired",
    }
}

string_enum! {
    pub enum ObserverRunStatus {
        Scheduled => "scheduled",
        AdminRequested => "admin_requested",
        SnapshotCollected => "snapshot_collected",
        Evaluated => "evaluated",
        Reported => "reported",
        Failed => "failed",
    }
}

string_enum! {
    pub enum HealthStatus {
        Healthy => "healthy",
        Degraded => "degraded",
        Unhealthy => "unhealthy",
        Unknown => "unknown",
    }
}

string_enum! {
    pub enum RiskLevel {
        Low => "low",
        Medium => "medium",
        High => "high",
        Critical => "critical",
    }
}

string_enum! {
    pub enum SideEffectMode {
        Deny => "deny",
        ReadOnly => "read_only",
        ApprovalRequired => "approval_required",
        Authorized => "authorized",
    }
}

string_enum! {
    pub enum RequestType {
        CreateAgent => "create_agent",
        ChangeAgent => "change_agent",
        ResumeAgent => "resume_agent",
        CreateRun => "create_run",
        CreateSession => "create_session",
        CreateChildSession => "create_child_session",
    }
}

string_enum! {
    pub enum TriggerType {
        Manual => "manual",
        Scheduled => "scheduled",
        Webhook => "webhook",
        SessionMessage => "session_message",
        AdminManual => "admin_manual",
    }
}

string_enum! {
    pub enum MessageRole {
        User => "user",
        Assistant => "assistant",
        System => "system",
        Tool => "tool",
    }
}

string_enum! {
    pub enum AuditDecision {
        Allowed => "allowed",
        Denied => "denied",
        ApprovalRequired => "approval_required",
        Conflict => "conflict",
        NotFoundOrForbidden => "not_found_or_forbidden",
        Completed => "completed",
        Failed => "failed",
    }
}

pub const AGENT_TYPE_BACKGROUND_WORKER: &str = "background_worker";
pub const AGENT_TYPE_OBSERVER: &str = "observer_agent";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub role: RoleName,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
}

impl RoleAssignment {
    pub fn global(role: RoleName) -> Self {
        Self {
            role,
            resource_type: None,
            resource_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub user_id: String,
    pub service_id: String,
    pub service_allowed_actions: Vec<String>,
    pub roles: Vec<RoleAssignment>,
    pub resource_allowlist: Vec<String>,
    pub trace_id: String,
}

impl AuthContext {
    pub fn has_role(&self, role: RoleName) -> bool {
        self.roles.iter().any(|assignment| assignment.role == role)
    }

    pub fn has_any_role(&self, roles: &[RoleName]) -> bool {
        roles.iter().copied().any(|role| self.has_role(role))
    }

    pub fn service_allows(&self, action: &str) -> bool {
        self.service_allowed_actions.iter().any(|allowed| {
            allowed == "*" || allowed == action || action.starts_with(allowed.trim_end_matches('*'))
        })
    }

    pub fn user_can_access_resource(&self, resource: &ResourceRef) -> bool {
        if self.has_any_role(&[
            RoleName::SystemAdmin,
            RoleName::AgentAdmin,
            RoleName::ResourceOwner,
            RoleName::ResourceMaintainer,
        ]) {
            return true;
        }

        if self
            .resource_allowlist
            .iter()
            .any(|allowed| allowed == "*" || allowed == &resource.raw)
        {
            return true;
        }

        self.roles.iter().any(|assignment| {
            assignment
                .resource_type
                .as_deref()
                .is_none_or(|resource_type| resource_type == resource.resource_type)
                && assignment
                    .resource_id
                    .as_deref()
                    .is_none_or(|resource_id| resource_id == resource.resource_id)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub raw: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl ResourceRef {
    pub fn parse(raw: impl Into<String>) -> CoreResult<Self> {
        let raw = raw.into();
        let stripped = raw.strip_prefix("resource:").unwrap_or(&raw);
        let Some((resource_type, resource_id)) = stripped.split_once('/') else {
            return Err(AgentCoreError::InvalidResourceRef(raw));
        };
        if resource_type.is_empty() || resource_id.is_empty() {
            return Err(AgentCoreError::InvalidResourceRef(raw));
        }
        let resource_type = resource_type.to_string();
        let resource_id = resource_id.to_string();
        Ok(Self {
            raw,
            resource_type,
            resource_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub agent_type: String,
    pub display_name: String,
    pub allowed_triggers: Value,
    pub allowed_actions: Value,
    pub default_constraints: Value,
    pub status: AgentTemplateStatus,
    pub created_at: OffsetDateTime,
}

impl AgentTemplate {
    pub fn background_worker(now: OffsetDateTime) -> Self {
        Self {
            agent_type: AGENT_TYPE_BACKGROUND_WORKER.to_string(),
            display_name: "通用后台执行 Agent".to_string(),
            allowed_triggers: json!(["manual", "scheduled", "webhook", "session_message"]),
            allowed_actions: json!(["analyze", "prepare_change", "run_checks"]),
            default_constraints: json!({
                "default_side_effect_mode": "approval_required",
                "max_items_per_run": 8,
                "max_runtime_seconds": 1800,
                "max_concurrent_runs_per_agent": 1,
                "max_concurrent_runs_per_resource": 1,
                "max_active_agents_per_user": 10,
                "max_active_agents_per_resource": 3,
                "max_active_agents_per_user_resource": 1,
                "max_session_depth": 1,
                "max_child_sessions_per_parent": 3,
                "active_child_sessions_per_parent": 2,
                "protected_scopes": ["secrets", "credentials", "production", "protected_branch"]
            }),
            status: AgentTemplateStatus::Active,
            created_at: now,
        }
    }

    pub fn observer(now: OffsetDateTime) -> Self {
        Self {
            agent_type: AGENT_TYPE_OBSERVER.to_string(),
            display_name: "系统观察 Agent".to_string(),
            allowed_triggers: json!(["scheduled", "admin_manual"]),
            allowed_actions: json!(["read_status_snapshot", "write_observer_report"]),
            default_constraints: json!({
                "default_side_effect_mode": "deny",
                "max_concurrent_observer_runs": 1,
                "readable_scopes": [
                    "status_summary",
                    "audit_summary",
                    "worker_heartbeat_summary",
                    "lock_summary",
                    "error_metrics"
                ],
                "forbidden_scopes": [
                    "secrets",
                    "credentials",
                    "full_prompt",
                    "full_context",
                    "raw_internal_logs"
                ]
            }),
            status: AgentTemplateStatus::Active,
            created_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub id: String,
    pub idempotency_key: Option<String>,
    pub requested_by_user: String,
    pub requested_by_service: String,
    pub request_type: RequestType,
    pub agent_type: Option<String>,
    pub target_resource: Option<String>,
    pub intent_text: Option<String>,
    pub structured_payload: Value,
    pub status: AgentRequestStatus,
    pub denial_reason: Option<String>,
    pub approval_id: Option<String>,
    pub result_agent_id: Option<String>,
    pub result_run_id: Option<String>,
    pub trace_id: String,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl AgentRequest {
    pub fn new(
        auth: &AuthContext,
        request_type: RequestType,
        agent_type: Option<String>,
        target_resource: Option<String>,
        intent_text: Option<String>,
        structured_payload: Value,
        idempotency_key: Option<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: new_id("req"),
            idempotency_key,
            requested_by_user: auth.user_id.clone(),
            requested_by_service: auth.service_id.clone(),
            request_type,
            agent_type,
            target_resource,
            intent_text,
            structured_payload,
            status: AgentRequestStatus::Requested,
            denial_reason: None,
            approval_id: None,
            result_agent_id: None,
            result_run_id: None,
            trace_id: auth.trace_id.clone(),
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub request_id: String,
    pub requested_by_user: String,
    pub approver_user: Option<String>,
    pub status: ApprovalStatus,
    pub risk_level: Option<RiskLevel>,
    pub reason: Option<String>,
    pub decision_reason: Option<String>,
    pub created_at: OffsetDateTime,
    pub decided_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    pub id: String,
    pub agent_type: String,
    pub hermes_profile: String,
    pub owner_user: String,
    pub target_resource: String,
    pub core_constraints_hash: String,
    pub status: AgentInstanceStatus,
    pub display_name: Option<String>,
    pub config: Value,
    pub trace_id: String,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl AgentInstance {
    pub fn new(
        owner_user: impl Into<String>,
        agent_type: impl Into<String>,
        target_resource: impl Into<String>,
        core_constraints_hash: impl Into<String>,
        config: Value,
        trace_id: impl Into<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        let agent_type = agent_type.into();
        Self {
            id: new_id("agent"),
            hermes_profile: format!("{agent_type}:agent-platform-minimal"),
            agent_type,
            owner_user: owner_user.into(),
            target_resource: target_resource.into(),
            core_constraints_hash: core_constraints_hash.into(),
            status: AgentInstanceStatus::Running,
            display_name: None,
            config,
            trace_id: trace_id.into(),
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub idempotency_key: Option<String>,
    pub agent_id: String,
    pub owner_user: String,
    pub source_conversation_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub created_by_session_id: Option<String>,
    pub status: AgentSessionStatus,
    pub depth: i32,
    pub resource_scope: Value,
    pub context_summary: Option<String>,
    pub trace_id: String,
    pub version: i64,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl AgentSession {
    pub fn new(
        agent_id: impl Into<String>,
        owner_user: impl Into<String>,
        resource_scope: Value,
        trace_id: impl Into<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: new_id("sess"),
            idempotency_key: None,
            agent_id: agent_id.into(),
            owner_user: owner_user.into(),
            source_conversation_id: None,
            parent_session_id: None,
            created_by_session_id: None,
            status: AgentSessionStatus::Active,
            depth: 0,
            resource_scope,
            context_summary: None,
            trace_id: trace_id.into(),
            version: 0,
            expires_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionMessage {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub role: MessageRole,
    pub content_ref: Option<String>,
    pub content_summary: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: String,
    pub created_at: OffsetDateTime,
}

impl AgentSessionMessage {
    pub fn new(
        session_id: impl Into<String>,
        sequence: i64,
        role: MessageRole,
        content_summary: Option<String>,
        run_id: Option<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id("msg"),
            session_id: session_id.into(),
            sequence,
            role,
            content_ref: None,
            content_summary,
            run_id,
            trace_id: trace_id.into(),
            created_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub idempotency_key: Option<String>,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub trigger_type: TriggerType,
    pub target_resource: String,
    pub run_status: AgentRunStatus,
    pub risk_level: RiskLevel,
    pub side_effect_mode: SideEffectMode,
    pub lease_owner: Option<String>,
    pub lease_until: Option<OffsetDateTime>,
    pub next_retry_at: Option<OffsetDateTime>,
    pub retry_count: i32,
    pub result_summary: Option<String>,
    pub result_ref: Option<String>,
    pub trace_id: String,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub claimed_at: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
}

impl AgentRun {
    pub fn new(
        agent_id: impl Into<String>,
        session_id: Option<String>,
        trigger_type: TriggerType,
        target_resource: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id("run"),
            idempotency_key: None,
            agent_id: agent_id.into(),
            session_id,
            trigger_type,
            target_resource: target_resource.into(),
            run_status: AgentRunStatus::Queued,
            risk_level: RiskLevel::Low,
            side_effect_mode: SideEffectMode::ReadOnly,
            lease_owner: None,
            lease_until: None,
            next_retry_at: None,
            retry_count: 0,
            result_summary: None,
            result_ref: None,
            trace_id: trace_id.into(),
            version: 0,
            created_at: OffsetDateTime::now_utc(),
            claimed_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunStep {
    pub id: String,
    pub run_id: String,
    pub step_name: String,
    pub status: String,
    pub summary: Option<String>,
    pub started_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLock {
    pub id: String,
    pub resource_type: String,
    pub resource_id: String,
    pub lock_scope: String,
    pub holder_run_id: String,
    pub lease_until: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGrant {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub constraints: Value,
    pub granted_by: Option<String>,
    pub created_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverReport {
    pub id: String,
    pub observer_run_id: String,
    pub health_status: HealthStatus,
    pub risk_level: Option<RiskLevel>,
    pub summary: String,
    pub findings: Value,
    pub recommendations: Value,
    pub evidence_refs: Value,
    pub trace_id: String,
    pub created_at: OffsetDateTime,
}

impl ObserverReport {
    pub fn new(
        observer_run_id: impl Into<String>,
        health_status: HealthStatus,
        risk_level: Option<RiskLevel>,
        summary: impl Into<String>,
        findings: Value,
        recommendations: Value,
        evidence_refs: Value,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id("obsr"),
            observer_run_id: observer_run_id.into(),
            health_status,
            risk_level,
            summary: summary.into(),
            findings,
            recommendations,
            evidence_refs,
            trace_id: trace_id.into(),
            created_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub actor_user: Option<String>,
    pub actor_service: Option<String>,
    pub action: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub decision: Option<AuditDecision>,
    pub reason: Option<String>,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub approval_id: Option<String>,
    pub observer_report_id: Option<String>,
    pub trace_id: String,
    pub created_at: OffsetDateTime,
}

impl AuditLog {
    pub fn new(
        auth: Option<&AuthContext>,
        action: impl Into<String>,
        decision: AuditDecision,
        reason: Option<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        let trace_id = trace_id.into();
        Self {
            id: new_id("audit"),
            actor_user: auth.map(|ctx| ctx.user_id.clone()),
            actor_service: auth.map(|ctx| ctx.service_id.clone()),
            action: action.into(),
            resource_type: None,
            resource_id: None,
            decision: Some(decision),
            reason,
            request_id: None,
            session_id: None,
            run_id: None,
            approval_id: None,
            observer_report_id: None,
            trace_id,
            created_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverSnapshot {
    pub collected_at: OffsetDateTime,
    pub agent_counts: Value,
    pub session_counts: Value,
    pub run_counts: Value,
    pub lock_summary: Value,
    pub audit_summary: Value,
    pub worker_summary: Value,
}

pub fn validate_request_transition(
    from: AgentRequestStatus,
    to: AgentRequestStatus,
) -> CoreResult<()> {
    use AgentRequestStatus::*;
    let ok = matches!(
        (from, to),
        (Requested, Parsed)
            | (Parsed, PolicyChecked)
            | (PolicyChecked, Denied | ApprovalRequired | Approved)
            | (Approved, Provisioning | Enqueued)
            | (Provisioning, Fulfilled)
            | (Enqueued, Fulfilled)
            | (ApprovalRequired, Approved | Denied | Cancelled | Expired)
            | (
                Requested | Parsed | PolicyChecked | Approved | Provisioning | Enqueued,
                Cancelled | Expired | Failed
            )
    );
    if ok {
        Ok(())
    } else {
        Err(AgentCoreError::InvalidTransition {
            entity: "agent_request",
            from: from.to_string(),
            to: to.to_string(),
        })
    }
}

pub fn validate_run_transition(from: AgentRunStatus, to: AgentRunStatus) -> CoreResult<()> {
    use AgentRunStatus::*;
    let ok = matches!(
        (from, to),
        (Queued, Claimed | Cancelled | TimedOut)
            | (Claimed, ContextBuilt | Failed | TimedOut)
            | (ContextBuilt, PolicyChecked | Failed | TimedOut)
            | (
                PolicyChecked,
                Executing | AwaitingApproval | Failed | TimedOut
            )
            | (Executing, Validating | Failed | TimedOut)
            | (
                Validating,
                ApplyingSideEffects | Completed | Failed | TimedOut
            )
            | (ApplyingSideEffects, Completed | Failed | TimedOut)
            | (AwaitingApproval, PolicyChecked | Cancelled | Failed)
            | (Failed, Queued | DeadLetter)
            | (TimedOut, Queued | DeadLetter)
            | (DeadLetter, Queued | Cancelled)
    );
    if ok {
        Ok(())
    } else {
        Err(AgentCoreError::InvalidTransition {
            entity: "agent_run",
            from: from.to_string(),
            to: to.to_string(),
        })
    }
}

pub fn validate_session_transition(
    from: AgentSessionStatus,
    to: AgentSessionStatus,
) -> CoreResult<()> {
    use AgentSessionStatus::*;
    let ok = matches!(
        (from, to),
        (Created, Active | Failed | Expired)
            | (Active, Closing | Closed | Failed | Expired)
            | (Closing, Closed | Failed)
    );
    if ok {
        Ok(())
    } else {
        Err(AgentCoreError::InvalidTransition {
            entity: "agent_session",
            from: from.to_string(),
            to: to.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_ref_parses_documented_format() {
        let resource = ResourceRef::parse("resource:team/project-alpha").unwrap();
        assert_eq!(resource.resource_type, "team");
        assert_eq!(resource.resource_id, "project-alpha");
    }

    #[test]
    fn child_run_does_not_skip_required_state_machine_steps() {
        assert!(validate_run_transition(AgentRunStatus::Queued, AgentRunStatus::Claimed).is_ok());
        assert!(
            validate_run_transition(AgentRunStatus::Queued, AgentRunStatus::Completed).is_err()
        );
    }
}
