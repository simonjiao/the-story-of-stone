#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tonglingyu_runtime::RuntimeWorkflowStreamEvent;

pub(crate) const RESPONSE_EVENT_SCHEMA_VERSION: &str = "tonglingyu.response_event.v1";

const FORBIDDEN_PUBLIC_KEYS: &[&str] = &[
    "trace_id",
    "review",
    "context_pack_id",
    "context_pack_ref",
    "context_projection_id",
    "context_projection_ref",
    "context_projection",
    "context_projections",
    "memory_card_id",
    "memory_card_ref",
    "memory_cards",
    "memory_candidate_id",
    "memory_candidate_ref",
    "memory_read_refs",
    "memory_read_policy_digest",
    "profile",
    "runtime_step_plan",
    "agent_runtime_plan_gate",
    "agent_runtime",
    "input_ref",
    "output_ref",
    "allowed_tools",
    "tool_calls",
    "tool_policy",
    "tool_policy_digest",
    "output_contract_digest",
    "raw_provider_response",
    "raw_prompt",
    "raw_memory",
    "provider_request",
    "provider_response",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseVisibility {
    Public,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseStatus {
    Queued,
    InProgress,
    Retrieving,
    Composing,
    Reviewing,
    RequiresAction,
    Canceling,
    Completed,
    Failed,
    Canceled,
    Timeout,
    Expired,
}

impl ResponseStatus {
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::Timeout | Self::Expired
        )
    }

    pub(crate) fn can_transition_to(&self, next: &Self) -> bool {
        if self == next {
            return true;
        }
        if self.is_terminal() {
            return false;
        }
        match (self, next) {
            (Self::Queued, Self::InProgress | Self::Canceling | Self::Failed | Self::Timeout) => {
                true
            }
            (
                Self::InProgress,
                Self::Retrieving
                | Self::Composing
                | Self::Reviewing
                | Self::RequiresAction
                | Self::Canceling
                | Self::Completed
                | Self::Failed
                | Self::Timeout,
            ) => true,
            (
                Self::Retrieving,
                Self::Composing
                | Self::Reviewing
                | Self::InProgress
                | Self::RequiresAction
                | Self::Canceling
                | Self::Completed
                | Self::Failed
                | Self::Timeout,
            ) => true,
            (
                Self::Composing,
                Self::Reviewing
                | Self::InProgress
                | Self::RequiresAction
                | Self::Canceling
                | Self::Completed
                | Self::Failed
                | Self::Timeout,
            ) => true,
            (
                Self::Reviewing,
                Self::InProgress
                | Self::RequiresAction
                | Self::Canceling
                | Self::Completed
                | Self::Failed
                | Self::Timeout,
            ) => true,
            (
                Self::RequiresAction,
                Self::InProgress | Self::Canceling | Self::Failed | Self::Timeout | Self::Expired,
            ) => true,
            (Self::Canceling, Self::Canceled | Self::Completed | Self::Failed) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ResponseEventType {
    #[serde(rename = "response.created")]
    ResponseCreated,
    #[serde(rename = "response.status")]
    ResponseStatus,
    #[serde(rename = "evidence.searching")]
    EvidenceSearching,
    #[serde(rename = "evidence.found")]
    EvidenceFound,
    #[serde(rename = "review.started")]
    ReviewStarted,
    #[serde(rename = "review.completed")]
    ReviewCompleted,
    #[serde(rename = "output_text.delta")]
    OutputTextDelta,
    #[serde(rename = "output_text.done")]
    OutputTextDone,
    #[serde(rename = "artifact.updated")]
    ArtifactUpdated,
    #[serde(rename = "response.requires_action")]
    ResponseRequiresAction,
    #[serde(rename = "response.completed")]
    ResponseCompleted,
    #[serde(rename = "response.failed")]
    ResponseFailed,
    #[serde(rename = "response.canceled")]
    ResponseCanceled,
    #[serde(rename = "runtime.plan.created")]
    RuntimePlanCreated,
    #[serde(rename = "runtime.profile.started")]
    RuntimeProfileStarted,
    #[serde(rename = "runtime.profile.completed")]
    RuntimeProfileCompleted,
    #[serde(rename = "runtime.tool.summary")]
    RuntimeToolSummary,
    #[serde(rename = "context.pack.created")]
    ContextPackCreated,
    #[serde(rename = "context.projection.created")]
    ContextProjectionCreated,
    #[serde(rename = "audit.linked")]
    AuditLinked,
    #[serde(rename = "dedupe.hit")]
    DedupeHit,
    #[serde(rename = "worker.retry_scheduled")]
    WorkerRetryScheduled,
    #[serde(rename = "worker.dead_lettered")]
    WorkerDeadLettered,
}

impl ResponseEventType {
    pub(crate) fn default_visibility(&self) -> ResponseVisibility {
        match self {
            Self::RuntimePlanCreated
            | Self::RuntimeProfileStarted
            | Self::RuntimeProfileCompleted
            | Self::RuntimeToolSummary
            | Self::ContextPackCreated
            | Self::ContextProjectionCreated
            | Self::AuditLinked
            | Self::DedupeHit
            | Self::WorkerRetryScheduled
            | Self::WorkerDeadLettered => ResponseVisibility::Admin,
            _ => ResponseVisibility::Public,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseEvent {
    pub(crate) schema_version: String,
    pub(crate) event_id: String,
    pub(crate) run_id: String,
    pub(crate) response_id: String,
    pub(crate) session_id: String,
    pub(crate) trace_id: String,
    pub(crate) sequence: u64,
    #[serde(rename = "type")]
    pub(crate) event_type: ResponseEventType,
    pub(crate) visibility: ResponseVisibility,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    pub(crate) payload: Value,
}

impl ResponseEvent {
    pub(crate) fn new(
        run_id: impl Into<String>,
        response_id: impl Into<String>,
        session_id: impl Into<String>,
        trace_id: impl Into<String>,
        sequence: u64,
        event_type: ResponseEventType,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: RESPONSE_EVENT_SCHEMA_VERSION.to_string(),
            event_id: format!("evt_{}", uuid::Uuid::now_v7().simple()),
            run_id: run_id.into(),
            response_id: response_id.into(),
            session_id: session_id.into(),
            trace_id: trace_id.into(),
            sequence,
            visibility: event_type.default_visibility(),
            event_type,
            created_at: OffsetDateTime::now_utc(),
            payload,
        }
    }

    pub(crate) fn public_projection(&self) -> Option<Value> {
        if self.visibility != ResponseVisibility::Public {
            return None;
        }
        Some(json!({
            "schema_version": self.schema_version,
            "event_id": self.event_id,
            "run_id": self.run_id,
            "response_id": self.response_id,
            "session_id": self.session_id,
            "sequence": self.sequence,
            "type": self.event_type,
            "created_at": self.created_at,
            "payload": sanitize_public_payload(&self.payload),
        }))
    }
}

pub(crate) fn response_event_from_runtime_stream_event(
    run_id: &str,
    response_id: &str,
    session_id: &str,
    event: &RuntimeWorkflowStreamEvent,
) -> ResponseEvent {
    let (event_type, payload) = match event.event_type.as_str() {
        "started" => (
            ResponseEventType::ResponseStatus,
            json!({
                "status": "in_progress",
                "package_id": &event.package_id,
                "source": "runtime_workflow",
            }),
        ),
        "step_completed" => (
            ResponseEventType::RuntimeProfileCompleted,
            json!({
                "profile": &event.profile,
                "output_ref": &event.output_ref,
                "package_id": &event.package_id,
                "metadata": &event.metadata,
            }),
        ),
        "content_delta" => (
            ResponseEventType::OutputTextDelta,
            json!({
                "text": event.content_delta.as_deref().unwrap_or_default(),
            }),
        ),
        "final_output" => (
            ResponseEventType::OutputTextDone,
            json!({
                "package_id": &event.package_id,
            }),
        ),
        other => (
            ResponseEventType::RuntimeToolSummary,
            json!({
                "runtime_event_type": other,
                "profile": &event.profile,
                "output_ref": &event.output_ref,
                "package_id": &event.package_id,
                "metadata": &event.metadata,
            }),
        ),
    };
    ResponseEvent::new(
        run_id,
        response_id,
        session_id,
        event.trace_id.clone(),
        event.sequence + 1,
        event_type,
        payload,
    )
}

pub(crate) fn sanitize_public_payload(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sanitized = Map::new();
            for (key, value) in map {
                if is_forbidden_public_key(key) {
                    continue;
                }
                sanitized.insert(key.clone(), sanitize_public_payload(value));
            }
            Value::Object(sanitized)
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(sanitize_public_payload)
                .collect::<Vec<_>>(),
        ),
        _ => value.clone(),
    }
}

fn is_forbidden_public_key(key: &str) -> bool {
    FORBIDDEN_PUBLIC_KEYS
        .iter()
        .any(|forbidden| key.eq_ignore_ascii_case(forbidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_state_cannot_transition_back_to_running() {
        assert!(!ResponseStatus::Completed.can_transition_to(&ResponseStatus::InProgress));
        assert!(!ResponseStatus::Failed.can_transition_to(&ResponseStatus::Queued));
    }

    #[test]
    fn canceling_can_only_finish_with_terminal_state() {
        assert!(ResponseStatus::Canceling.can_transition_to(&ResponseStatus::Canceled));
        assert!(ResponseStatus::Canceling.can_transition_to(&ResponseStatus::Completed));
        assert!(ResponseStatus::Canceling.can_transition_to(&ResponseStatus::Failed));
        assert!(!ResponseStatus::Canceling.can_transition_to(&ResponseStatus::Retrieving));
    }

    #[test]
    fn streaming_response_status_can_advance_through_workflow_phases() {
        assert!(ResponseStatus::Queued.can_transition_to(&ResponseStatus::InProgress));
        assert!(ResponseStatus::InProgress.can_transition_to(&ResponseStatus::Retrieving));
        assert!(ResponseStatus::Retrieving.can_transition_to(&ResponseStatus::Composing));
        assert!(ResponseStatus::Composing.can_transition_to(&ResponseStatus::Reviewing));
        assert!(ResponseStatus::Reviewing.can_transition_to(&ResponseStatus::Completed));
        assert!(!ResponseStatus::Composing.can_transition_to(&ResponseStatus::Retrieving));
        assert!(!ResponseStatus::Reviewing.can_transition_to(&ResponseStatus::Composing));
    }

    #[test]
    fn public_projection_removes_admin_only_fields_recursively() {
        let event = ResponseEvent::new(
            "run_test",
            "resp_test",
            "session_test",
            "trace_secret",
            1,
            ResponseEventType::OutputTextDelta,
            json!({
                "text": "公开文本",
                "trace_id": "trace_secret",
                "nested": {
                    "context_pack_id": "ctx_secret",
                    "safe": true,
                    "items": [{"memory_card_id": "mem_secret", "count": 1}]
                }
            }),
        );

        let public = event.public_projection().expect("public event");

        assert!(public.get("trace_id").is_none());
        assert_eq!(public["payload"]["text"], json!("公开文本"));
        assert!(public["payload"].get("trace_id").is_none());
        assert!(public["payload"]["nested"].get("context_pack_id").is_none());
        assert_eq!(public["payload"]["nested"]["safe"], json!(true));
        assert!(
            public["payload"]["nested"]["items"][0]
                .get("memory_card_id")
                .is_none()
        );
    }

    #[test]
    fn admin_events_have_no_public_projection() {
        let event = ResponseEvent::new(
            "run_test",
            "resp_test",
            "session_test",
            "trace_secret",
            1,
            ResponseEventType::RuntimePlanCreated,
            json!({"runtime_step_plan": "secret"}),
        );

        assert_eq!(event.visibility, ResponseVisibility::Admin);
        assert!(event.public_projection().is_none());
    }

    #[test]
    fn unknown_event_type_is_rejected_by_schema() {
        let value = json!({
            "schema_version": RESPONSE_EVENT_SCHEMA_VERSION,
            "event_id": "evt_test",
            "run_id": "run_test",
            "response_id": "resp_test",
            "session_id": "session_test",
            "trace_id": "trace_test",
            "sequence": 1,
            "type": "runtime.unregistered",
            "visibility": "public",
            "created_at": "2026-06-29T00:00:00Z",
            "payload": {}
        });

        assert!(serde_json::from_value::<ResponseEvent>(value).is_err());
    }

    #[test]
    fn runtime_content_delta_projects_to_public_response_event() {
        let runtime_event = RuntimeWorkflowStreamEvent {
            sequence: 7,
            event_type: "content_delta".to_string(),
            profile: "honglou-main".to_string(),
            trace_id: "trace-runtime".to_string(),
            content_delta: Some("公开增量".to_string()),
            output_ref: Some("secret-output-ref".to_string()),
            package_id: Some("pkg_public".to_string()),
            metadata: json!({
                "context_pack_id": "ctx-secret",
                "agent_runtime": {"client": "secret"},
                "safe": true
            }),
        };

        let event = response_event_from_runtime_stream_event(
            "run_runtime",
            "resp_runtime",
            "session_runtime",
            &runtime_event,
        );
        let public = event.public_projection().expect("public runtime delta");

        assert_eq!(event.sequence, 8);
        assert_eq!(event.event_type, ResponseEventType::OutputTextDelta);
        assert_eq!(public["payload"]["text"], json!("公开增量"));
        assert!(public["payload"].get("output_ref").is_none());
        assert!(public["payload"].get("agent_runtime").is_none());
    }

    #[test]
    fn runtime_step_completed_stays_admin_only() {
        let runtime_event = RuntimeWorkflowStreamEvent {
            sequence: 2,
            event_type: "step_completed".to_string(),
            profile: "honglou-reviewer".to_string(),
            trace_id: "trace-runtime".to_string(),
            content_delta: None,
            output_ref: Some("output-secret".to_string()),
            package_id: Some("pkg_public".to_string()),
            metadata: json!({"allowed_tools": ["tonglingyu.text.search"]}),
        };

        let event = response_event_from_runtime_stream_event(
            "run_runtime",
            "resp_runtime",
            "session_runtime",
            &runtime_event,
        );

        assert_eq!(event.event_type, ResponseEventType::RuntimeProfileCompleted);
        assert_eq!(event.visibility, ResponseVisibility::Admin);
        assert!(event.public_projection().is_none());
    }
}
