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
        ApplyingExternalActions => "applying_external_actions",
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
    pub enum ExternalActionMode {
        Deny => "deny",
        ReadOnly => "read_only",
        ApprovalRequired => "approval_required",
        Authorized => "authorized",
    }
}

string_enum! {
    pub enum ExternalActionPlanStatus {
        Draft => "draft",
        DryRunReady => "dry_run_ready",
        DryRunRejected => "dry_run_rejected",
        Applied => "applied",
        Compensated => "compensated",
        Failed => "failed",
    }
}

string_enum! {
    pub enum CredentialLeaseStatus {
        DryRun => "dry_run",
        Active => "active",
        Revoked => "revoked",
        Expired => "expired",
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

string_enum! {
    pub enum AgentBridgeBindingStatus {
        Active => "active",
        Closed => "closed",
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
                "default_external_action_mode": "approval_required",
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
                "default_external_action_mode": "deny",
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
        let hermes_profile = config
            .get("hermes_profile")
            .or_else(|| config.pointer("/runtime/hermes_profile"))
            .or_else(|| config.get("runtime_profile"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|profile| !profile.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("{agent_type}:agent-platform-minimal"));
        Self {
            id: new_id("agent"),
            hermes_profile,
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
    pub external_message_id: Option<String>,
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
            external_message_id: None,
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
    pub external_action_mode: ExternalActionMode,
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
            external_action_mode: ExternalActionMode::ReadOnly,
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
pub struct ExternalActionPlan {
    pub id: String,
    pub run_id: String,
    pub connector: String,
    pub action: String,
    pub resource_ref: String,
    pub risk_level: RiskLevel,
    pub external_action_mode: ExternalActionMode,
    pub approval_id: Option<String>,
    pub credential_scope: Option<String>,
    pub input_summary: Option<String>,
    pub input_ref: Option<String>,
    pub result_ref: Option<String>,
    pub compensation_ref: Option<String>,
    pub compensation_result_ref: Option<String>,
    pub status: ExternalActionPlanStatus,
    pub error_code: Option<String>,
    pub trace_id: String,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl ExternalActionPlan {
    pub fn new(
        run_id: impl Into<String>,
        connector: impl Into<String>,
        action: impl Into<String>,
        resource_ref: impl Into<String>,
        risk_level: RiskLevel,
        external_action_mode: ExternalActionMode,
        trace_id: impl Into<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: new_id("eaplan"),
            run_id: run_id.into(),
            connector: connector.into(),
            action: action.into(),
            resource_ref: resource_ref.into(),
            risk_level,
            external_action_mode,
            approval_id: None,
            credential_scope: None,
            input_summary: None,
            input_ref: None,
            result_ref: None,
            compensation_ref: None,
            compensation_result_ref: None,
            status: ExternalActionPlanStatus::Draft,
            error_code: None,
            trace_id: trace_id.into(),
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialLease {
    pub id: String,
    pub external_action_plan_id: String,
    pub credential_scope: String,
    pub provider_ref: Option<String>,
    pub status: CredentialLeaseStatus,
    pub expires_at: Option<OffsetDateTime>,
    pub trace_id: String,
    pub revoked_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

impl CredentialLease {
    pub fn dry_run(
        external_action_plan_id: impl Into<String>,
        credential_scope: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id("credlease"),
            external_action_plan_id: external_action_plan_id.into(),
            credential_scope: credential_scope.into(),
            provider_ref: None,
            status: CredentialLeaseStatus::DryRun,
            expires_at: None,
            trace_id: trace_id.into(),
            revoked_at: None,
            created_at: OffsetDateTime::now_utc(),
        }
    }

    pub fn active(
        external_action_plan_id: impl Into<String>,
        credential_scope: impl Into<String>,
        provider_ref: impl Into<String>,
        ttl_seconds: i64,
        trace_id: impl Into<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: new_id("credlease"),
            external_action_plan_id: external_action_plan_id.into(),
            credential_scope: credential_scope.into(),
            provider_ref: Some(provider_ref.into()),
            status: CredentialLeaseStatus::Active,
            expires_at: Some(now + time::Duration::seconds(ttl_seconds.max(1))),
            trace_id: trace_id.into(),
            revoked_at: None,
            created_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBridgeBinding {
    pub id: String,
    pub open_webui_subject: String,
    pub open_webui_chat_id: String,
    pub open_webui_session_id: Option<String>,
    pub model: String,
    pub agent_id: String,
    pub agent_session_id: String,
    pub status: AgentBridgeBindingStatus,
    pub last_message_id: Option<String>,
    pub last_run_id: Option<String>,
    pub trace_id: String,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub closed_at: Option<OffsetDateTime>,
}

impl AgentBridgeBinding {
    pub fn new(
        open_webui_subject: impl Into<String>,
        open_webui_chat_id: impl Into<String>,
        open_webui_session_id: Option<String>,
        model: impl Into<String>,
        agent_id: impl Into<String>,
        agent_session_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: new_id("bridge"),
            open_webui_subject: open_webui_subject.into(),
            open_webui_chat_id: open_webui_chat_id.into(),
            open_webui_session_id,
            model: model.into(),
            agent_id: agent_id.into(),
            agent_session_id: agent_session_id.into(),
            status: AgentBridgeBindingStatus::Active,
            last_message_id: None,
            last_run_id: None,
            trace_id: trace_id.into(),
            version: 0,
            created_at: now,
            updated_at: now,
            closed_at: None,
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
    pub runtime_summary: Value,
    pub lock_summary: Value,
    pub audit_summary: Value,
    pub worker_summary: Value,
}

#[derive(Debug, Clone)]
pub struct ObserverSnapshotAssessment {
    pub health_status: HealthStatus,
    pub risk_level: RiskLevel,
    pub summary: String,
    pub findings: Value,
    pub recommendations: Value,
    pub evidence_refs: Value,
}

pub fn assess_observer_snapshot(snapshot: &ObserverSnapshot) -> ObserverSnapshotAssessment {
    let dead_letters = json_i64(&snapshot.run_counts, "dead_letter");
    let failed = json_i64(&snapshot.run_counts, "failed");
    let timed_out = std::cmp::max(
        json_i64(&snapshot.run_counts, "timed_out"),
        json_i64(&snapshot.runtime_summary, "timed_out_runs"),
    );
    let retrying = json_i64(&snapshot.runtime_summary, "retrying_runs");
    let max_retry = json_i64(&snapshot.runtime_summary, "max_retry_count");
    let avg_runtime_ms = json_f64(&snapshot.runtime_summary, "avg_completed_runtime_ms");
    let max_context_messages = json_i64(&snapshot.runtime_summary, "max_context_messages");
    let dry_run_rejected = snapshot
        .runtime_summary
        .get("external_action_plan_counts")
        .map(|counts| json_i64(counts, "dry_run_rejected"))
        .unwrap_or(0);
    let external_action_applied = snapshot
        .runtime_summary
        .get("external_action_plan_counts")
        .map(|counts| json_i64(counts, "applied"))
        .unwrap_or(0);
    let external_action_compensated = snapshot
        .runtime_summary
        .get("external_action_plan_counts")
        .map(|counts| json_i64(counts, "compensated"))
        .unwrap_or(0);
    let external_action_failed = snapshot
        .runtime_summary
        .get("external_action_plan_counts")
        .map(|counts| json_i64(counts, "failed"))
        .unwrap_or(0);
    let external_action_errors = snapshot
        .runtime_summary
        .get("external_action_plan_error_counts")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let approval_bypass_attempts = json_i64(&external_action_errors, "approval_required")
        + json_i64(&external_action_errors, "approval_not_found")
        + json_i64(&external_action_errors, "approval_not_approved:denied");
    let external_action_lock_conflicts = json_i64(&external_action_errors, "resource_locked");
    let abnormal_external_action_results =
        json_i64(&external_action_errors, "connector_invalid_result")
            + json_i64(&external_action_errors, "connector_dead_letter");
    let active_locks = json_i64(&snapshot.lock_summary, "active_locks");
    let failed_audits = snapshot
        .audit_summary
        .get("recent_decisions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item.get("decision").and_then(Value::as_str),
                        Some("failed" | "denied" | "conflict")
                    )
                })
                .count() as i64
        })
        .unwrap_or(0);

    let mut signals = Vec::new();
    push_count_signal(
        &mut signals,
        "runtime_dead_letter",
        dead_letters,
        1,
        5,
        "dead_letter runs indicate exhausted retries or unrecoverable runtime failures",
    );
    push_count_signal(
        &mut signals,
        "runtime_failed",
        failed,
        5,
        20,
        "failed runs indicate runtime, policy or connector failures that did not complete",
    );
    push_count_signal(
        &mut signals,
        "runtime_timeout",
        timed_out,
        1,
        5,
        "timed_out runs indicate runtime latency or stuck execution",
    );
    push_count_signal(
        &mut signals,
        "runtime_retry_pressure",
        retrying + max_retry,
        3,
        10,
        "retry pressure indicates unstable runtime or worker execution",
    );
    push_f64_signal(
        &mut signals,
        "runtime_latency",
        avg_runtime_ms,
        30_000.0,
        120_000.0,
        "completed runtime latency is above the P1 review threshold",
    );
    push_count_signal(
        &mut signals,
        "context_growth",
        max_context_messages,
        100,
        300,
        "session context is growing enough to affect quality and cost",
    );
    push_count_signal(
        &mut signals,
        "external_action_dry_run_rejection",
        dry_run_rejected,
        1,
        10,
        "External action readiness dry-runs are being rejected and need operator review",
    );
    push_count_signal(
        &mut signals,
        "external_action_apply_failure",
        external_action_failed,
        1,
        5,
        "External action writes are failing and need connector, credential or approval review",
    );
    push_count_signal(
        &mut signals,
        "external_action_approval_bypass_attempt",
        approval_bypass_attempts,
        1,
        5,
        "external-action apply attempts are missing or using invalid approvals",
    );
    push_count_signal(
        &mut signals,
        "external_action_lock_conflict",
        external_action_lock_conflicts,
        1,
        5,
        "external-action apply attempts are colliding on resource locks",
    );
    push_count_signal(
        &mut signals,
        "external_action_abnormal_write_result",
        abnormal_external_action_results,
        1,
        5,
        "write connector results are invalid or exhausted retry/dead-letter handling",
    );
    push_count_signal(
        &mut signals,
        "resource_lock_pressure",
        active_locks,
        5,
        20,
        "active resource locks may indicate long-running or stuck external-action preparation",
    );
    push_count_signal(
        &mut signals,
        "audit_failure_decisions",
        failed_audits,
        3,
        10,
        "recent audit decisions include failed, denied or conflict outcomes",
    );

    let risk_level = highest_signal_risk(&signals);
    let health_status = match risk_level {
        RiskLevel::Critical => HealthStatus::Unhealthy,
        RiskLevel::High | RiskLevel::Medium => HealthStatus::Degraded,
        RiskLevel::Low => HealthStatus::Healthy,
    };
    let summary = format!(
        "Observer snapshot collected at {}. signals={}, dead_letter={}, failed={}, timed_out={}, retrying={}, avg_runtime_ms={:.0}, max_context_messages={}, dry_run_rejected={}, external_action_applied={}, external_action_compensated={}, external_action_failed={}, approval_bypass_attempts={}, external_action_lock_conflicts={}, abnormal_external_action_results={}.",
        snapshot.collected_at,
        signals.len(),
        dead_letters,
        failed,
        timed_out,
        retrying,
        avg_runtime_ms,
        max_context_messages,
        dry_run_rejected,
        external_action_applied,
        external_action_compensated,
        external_action_failed,
        approval_bypass_attempts,
        external_action_lock_conflicts,
        abnormal_external_action_results
    );
    let recommendations = if signals.is_empty() {
        json!([
            {
                "priority": "low",
                "recommendation": "Continue normal P1 smoke and audit review; no runtime quality risk crossed the current threshold."
            }
        ])
    } else {
        Value::Array(
            signals
                .iter()
                .map(|signal| {
                    json!({
                        "priority": signal.get("severity").and_then(Value::as_str).unwrap_or("medium"),
                        "category": signal.get("category").and_then(Value::as_str).unwrap_or("runtime_quality"),
                        "recommendation": "Inspect the linked run, audit, worker heartbeat and connector summaries before any control-plane change."
                    })
                })
                .collect(),
        )
    };

    ObserverSnapshotAssessment {
        health_status,
        risk_level,
        summary,
        findings: json!({
            "run_counts": snapshot.run_counts.clone(),
            "agent_counts": snapshot.agent_counts.clone(),
            "session_counts": snapshot.session_counts.clone(),
            "runtime_summary": snapshot.runtime_summary.clone(),
            "quality_signals": signals,
            "risk_taxonomy": {
                "runtime_dead_letter": dead_letters,
                "runtime_failed": failed,
                "runtime_timeout": timed_out,
                "runtime_retry_pressure": retrying,
                "max_retry_count": max_retry,
                "runtime_latency_ms": avg_runtime_ms,
                "context_growth_messages": max_context_messages,
                "external_action_dry_run_rejection": dry_run_rejected,
                "external_action_applied": external_action_applied,
                "external_action_compensated": external_action_compensated,
                "external_action_apply_failure": external_action_failed,
                "external_action_error_counts": external_action_errors,
                "external_action_approval_bypass_attempt": approval_bypass_attempts,
                "external_action_lock_conflict": external_action_lock_conflicts,
                "external_action_abnormal_write_result": abnormal_external_action_results,
                "resource_lock_pressure": active_locks,
                "audit_failure_decisions": failed_audits
            }
        }),
        recommendations,
        evidence_refs: json!({
            "lock_summary": snapshot.lock_summary.clone(),
            "audit_summary": snapshot.audit_summary.clone(),
            "worker_summary": snapshot.worker_summary.clone(),
        }),
    }
}

fn json_i64(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn json_f64(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn push_count_signal(
    signals: &mut Vec<Value>,
    category: &str,
    value: i64,
    medium_threshold: i64,
    high_threshold: i64,
    summary: &str,
) {
    if value < medium_threshold {
        return;
    }
    let severity = if value >= high_threshold {
        "high"
    } else {
        "medium"
    };
    signals.push(json!({
        "category": category,
        "severity": severity,
        "value": value,
        "summary": summary,
    }));
}

fn push_f64_signal(
    signals: &mut Vec<Value>,
    category: &str,
    value: f64,
    medium_threshold: f64,
    high_threshold: f64,
    summary: &str,
) {
    if value < medium_threshold {
        return;
    }
    let severity = if value >= high_threshold {
        "high"
    } else {
        "medium"
    };
    signals.push(json!({
        "category": category,
        "severity": severity,
        "value": value,
        "summary": summary,
    }));
}

fn highest_signal_risk(signals: &[Value]) -> RiskLevel {
    let high = signals.iter().any(|signal| {
        signal
            .get("severity")
            .and_then(Value::as_str)
            .is_some_and(|severity| severity == "high")
    });
    if high {
        RiskLevel::High
    } else if signals.is_empty() {
        RiskLevel::Low
    } else {
        RiskLevel::Medium
    }
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
                ApplyingExternalActions | Completed | Failed | TimedOut
            )
            | (ApplyingExternalActions, Completed | Failed | TimedOut)
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

    #[test]
    fn agent_instance_uses_configured_hermes_profile() {
        let agent = AgentInstance::new(
            "user-1",
            "background_worker",
            "resource:team/project-alpha",
            "hash",
            json!({"runtime": {"hermes_profile": "background_worker:analysis"}}),
            "trace-test",
        );

        assert_eq!(agent.hermes_profile, "background_worker:analysis");
    }

    #[test]
    fn observer_assessment_flags_runtime_quality_signals() {
        let snapshot = ObserverSnapshot {
            collected_at: OffsetDateTime::now_utc(),
            agent_counts: json!({"running": 1}),
            session_counts: json!({"active": 1}),
            run_counts: json!({"dead_letter": 1, "failed": 6}),
            runtime_summary: json!({
                "retrying_runs": 3,
                "max_retry_count": 3,
                "timed_out_runs": 1,
                "avg_completed_runtime_ms": 31_000.0,
                "max_context_messages": 101,
                "external_action_plan_counts": {"dry_run_rejected": 1}
            }),
            lock_summary: json!({"active_locks": 0}),
            audit_summary: json!({"recent_decisions": []}),
            worker_summary: json!({"workers": []}),
        };

        let assessment = assess_observer_snapshot(&snapshot);

        assert_eq!(assessment.health_status, HealthStatus::Degraded);
        assert_eq!(assessment.risk_level, RiskLevel::Medium);
        assert!(
            assessment
                .findings
                .get("quality_signals")
                .and_then(Value::as_array)
                .is_some_and(|signals| signals.len() >= 4)
        );
    }
}
