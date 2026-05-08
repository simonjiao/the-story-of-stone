use crate::{
    AgentInstanceStatus, AgentRequestStatus, AgentRunStatus, AgentSessionStatus, HealthStatus,
    MessageRole, RequestType, RiskLevel, SideEffectMode, TriggerType,
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
    pub side_effect_mode: Option<SideEffectMode>,
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
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunInput {
    pub session_id: Option<String>,
    pub trigger_type: TriggerType,
    pub idempotency_key: Option<String>,
    pub target_resource: Option<String>,
    pub risk_level: Option<RiskLevel>,
    pub side_effect_mode: Option<SideEffectMode>,
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
