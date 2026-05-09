use crate::{
    AgentBridgeBindingStatus, AgentInstanceStatus, AgentRequestStatus, AgentRunStatus,
    AgentSession, AgentSessionMessage, AgentSessionStatus, CredentialLease, ExternalActionMode,
    ExternalActionPlan, ExternalActionPlanStatus, HealthStatus, MessageRole, RequestType,
    ResourceLock, RiskLevel, TriggerType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequestInput {
    pub request_type: RequestType,
    pub agent_type: Option<String>,
    pub target_resource: Option<String>,
    pub intent_text: Option<String>,
    #[serde(default)]
    pub structured_payload: Value,
    pub idempotency_key: Option<String>,
    pub risk_level: Option<RiskLevel>,
    pub external_action_mode: Option<ExternalActionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequestResponse {
    pub request_id: String,
    pub status: AgentRequestStatus,
    pub message: String,
    pub approval_id: Option<String>,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecisionInput {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenyDecisionInput {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionInput {
    pub source_conversation_id: Option<String>,
    #[serde(default)]
    pub resource_scope: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChildSessionInput {
    pub agent_id: Option<String>,
    #[serde(default)]
    pub resource_scope: Value,
    pub context_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendMessageInput {
    pub role: MessageRole,
    pub content_summary: String,
    pub content_ref: Option<String>,
    #[serde(default)]
    pub external_message_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunInput {
    pub session_id: Option<String>,
    pub trigger_type: TriggerType,
    pub idempotency_key: Option<String>,
    pub target_resource: Option<String>,
    pub risk_level: Option<RiskLevel>,
    pub external_action_mode: Option<ExternalActionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub agent_type: String,
    pub display_name: Option<String>,
    pub target_resource: String,
    pub status: AgentInstanceStatus,
    pub allowed_actions: Value,
    pub active_session_count: i64,
    pub last_run_status: Option<AgentRunStatus>,
    pub last_run_at: Option<OffsetDateTime>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub agent_id: String,
    pub status: AgentSessionStatus,
    pub parent_session_id: Option<String>,
    pub depth: i32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub context_summary: Option<String>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub trigger_type: TriggerType,
    pub target_resource: String,
    pub run_status: AgentRunStatus,
    pub risk_level: RiskLevel,
    pub result_summary: Option<String>,
    pub result_ref: Option<String>,
    pub next_retry_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBridgeBindingSummary {
    pub binding_id: String,
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
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub closed_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertOpenWebUiBridgeBindingInput {
    pub open_webui_chat_id: String,
    pub open_webui_session_id: Option<String>,
    pub model: String,
    pub agent_id: String,
    pub agent_session_id: String,
    pub last_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOpenWebUiBridgeRunInput {
    pub message_id: Option<String>,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimOpenWebUiBridgeNonceInput {
    pub open_webui_chat_id: String,
    pub model: String,
    pub nonce: String,
    pub issued_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAdminDecisionInput {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverReportSummary {
    pub report_id: String,
    pub observer_run_id: String,
    pub health_status: HealthStatus,
    pub risk_level: Option<RiskLevel>,
    pub summary: String,
    pub created_at: OffsetDateTime,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverReportDiscussionInput {
    pub agent_id: String,
    pub initial_message: String,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverReportDiscussionResponse {
    pub report_id: String,
    pub session: AgentSession,
    pub first_message: AgentSessionMessage,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatusSessionInput {
    pub report_id: Option<String>,
    pub initial_message: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatusSessionResponse {
    pub report_id: String,
    pub agent: crate::AgentInstance,
    pub session: AgentSession,
    pub report_message: AgentSessionMessage,
    pub first_message: AgentSessionMessage,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalActionPlanDryRunInput {
    pub connector: String,
    pub action: String,
    pub resource_ref: String,
    pub credential_scope: Option<String>,
    pub approval_id: Option<String>,
    pub input_summary: Option<String>,
    pub input_ref: Option<String>,
    pub risk_level: Option<RiskLevel>,
    pub external_action_mode: Option<ExternalActionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalActionPlanDryRunResponse {
    pub plan: ExternalActionPlan,
    pub credential_lease: Option<CredentialLease>,
    pub dry_run_status: ExternalActionPlanStatus,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalActionPlanApplyInput {
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalActionPlanApplyResponse {
    pub plan: ExternalActionPlan,
    pub credential_lease: CredentialLease,
    pub resource_lock: ResourceLock,
    pub apply_status: ExternalActionPlanStatus,
    #[serde(default)]
    pub connector_metadata: Value,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalActionPlanCompensateInput {
    pub reason: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalActionPlanCompensateResponse {
    pub plan: ExternalActionPlan,
    pub compensate_status: ExternalActionPlanStatus,
    pub compensation_result_ref: Option<String>,
    #[serde(default)]
    pub connector_metadata: Value,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub session_id: String,
    pub agent_id: String,
    pub context_summary: Option<String>,
    pub recent_messages: Vec<AppendMessageInput>,
    pub resource_scope: Value,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTriggerInput {
    pub trigger_type: String,
    pub connector: String,
    pub event_type: String,
    pub resource: String,
    pub dedupe_key: String,
    pub payload_ref: String,
    #[serde(with = "time::serde::rfc3339")]
    pub received_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGrantInput {
    pub subject_type: String,
    pub subject_id: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyResponse {
    pub status: String,
    pub trace_id: String,
}
