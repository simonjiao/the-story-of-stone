#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::response_events::{
    ResponseEvent, ResponseEventType, ResponseStatus, ResponseVisibility,
};
use crate::run_manager::RunIdentity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseStateRecord {
    pub(crate) run_id: String,
    pub(crate) response_id: String,
    pub(crate) session_id: String,
    pub(crate) trace_id: String,
    pub(crate) tenant_id: String,
    pub(crate) subject: String,
    pub(crate) user_id: Option<String>,
    pub(crate) status: ResponseStatus,
    pub(crate) sequence: u64,
    pub(crate) last_event_id: Option<String>,
    pub(crate) package_id: Option<String>,
    pub(crate) final_response_ref: Option<String>,
    pub(crate) cancel_requested: bool,
    pub(crate) requires_action_count: u64,
    pub(crate) callback_policy_ref: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) completed_at: Option<OffsetDateTime>,
}

impl ResponseStateRecord {
    fn new(identity: &RunIdentity) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            run_id: identity.run_id.clone(),
            response_id: identity.response_id.clone(),
            session_id: identity.session_id.clone(),
            trace_id: identity.trace_id.clone(),
            tenant_id: identity.owner_scope.tenant_id.clone(),
            subject: identity.owner_scope.subject.clone(),
            user_id: identity.owner_scope.user_id.clone(),
            status: identity.initial_status.clone(),
            sequence: 0,
            last_event_id: None,
            package_id: None,
            final_response_ref: None,
            cancel_requested: false,
            requires_action_count: 0,
            callback_policy_ref: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AppendResponseEventInput {
    pub(crate) response_id: String,
    pub(crate) event_type: ResponseEventType,
    pub(crate) payload: Value,
    pub(crate) status_update: Option<ResponseStatus>,
    pub(crate) visibility: Option<ResponseVisibility>,
    pub(crate) package_id: Option<String>,
    pub(crate) final_response_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResponseStoreError {
    DuplicateResponseId(String),
    DuplicateRunId(String),
    DuplicateIdempotencyKey {
        response_id: String,
    },
    UnknownResponseId(String),
    UnknownRunId(String),
    InvalidStatusTransition {
        response_id: String,
        current: ResponseStatus,
        next: ResponseStatus,
    },
}

pub(crate) trait ResponseEventStore {
    fn create_response(
        &mut self,
        identity: &RunIdentity,
    ) -> Result<ResponseStateRecord, ResponseStoreError>;

    fn response_id_for_run(&self, run_id: &str) -> Result<String, ResponseStoreError>;

    fn state(&self, response_id: &str) -> Result<ResponseStateRecord, ResponseStoreError>;

    fn append_event(
        &mut self,
        input: AppendResponseEventInput,
    ) -> Result<(ResponseStateRecord, ResponseEvent), ResponseStoreError>;

    fn read_after(
        &self,
        response_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Vec<ResponseEvent>, ResponseStoreError>;
}

#[derive(Debug, Default)]
pub(crate) struct InMemoryResponseEventStore {
    states: BTreeMap<String, ResponseStateRecord>,
    run_to_response: BTreeMap<String, String>,
    idempotency_to_response: BTreeMap<String, String>,
    events: BTreeMap<String, Vec<ResponseEvent>>,
}

impl ResponseEventStore for InMemoryResponseEventStore {
    fn create_response(
        &mut self,
        identity: &RunIdentity,
    ) -> Result<ResponseStateRecord, ResponseStoreError> {
        if self.states.contains_key(&identity.response_id) {
            return Err(ResponseStoreError::DuplicateResponseId(
                identity.response_id.clone(),
            ));
        }
        if self.run_to_response.contains_key(&identity.run_id) {
            return Err(ResponseStoreError::DuplicateRunId(identity.run_id.clone()));
        }
        if let Some(key) = identity_idempotency_key(identity) {
            if let Some(response_id) = self.idempotency_to_response.get(&key) {
                return Err(ResponseStoreError::DuplicateIdempotencyKey {
                    response_id: response_id.clone(),
                });
            }
        }
        let state = ResponseStateRecord::new(identity);
        self.run_to_response
            .insert(identity.run_id.clone(), identity.response_id.clone());
        if let Some(key) = identity_idempotency_key(identity) {
            self.idempotency_to_response
                .insert(key, identity.response_id.clone());
        }
        self.events.insert(identity.response_id.clone(), Vec::new());
        self.states
            .insert(identity.response_id.clone(), state.clone());
        Ok(state)
    }

    fn response_id_for_run(&self, run_id: &str) -> Result<String, ResponseStoreError> {
        self.run_to_response
            .get(run_id)
            .cloned()
            .ok_or_else(|| ResponseStoreError::UnknownRunId(run_id.to_string()))
    }

    fn state(&self, response_id: &str) -> Result<ResponseStateRecord, ResponseStoreError> {
        self.states
            .get(response_id)
            .cloned()
            .ok_or_else(|| ResponseStoreError::UnknownResponseId(response_id.to_string()))
    }

    fn append_event(
        &mut self,
        input: AppendResponseEventInput,
    ) -> Result<(ResponseStateRecord, ResponseEvent), ResponseStoreError> {
        let mut state = self.state(&input.response_id)?;
        if let Some(next) = &input.status_update {
            if !state.status.can_transition_to(next) {
                return Err(ResponseStoreError::InvalidStatusTransition {
                    response_id: input.response_id,
                    current: state.status,
                    next: next.clone(),
                });
            }
        }

        let next_sequence = state.sequence + 1;
        let mut event = ResponseEvent::new(
            state.run_id.clone(),
            state.response_id.clone(),
            state.session_id.clone(),
            state.trace_id.clone(),
            next_sequence,
            input.event_type,
            input.payload,
        );
        if let Some(visibility) = input.visibility {
            event.visibility = visibility;
        }

        state.sequence = next_sequence;
        state.last_event_id = Some(event.event_id.clone());
        state.updated_at = OffsetDateTime::now_utc();
        if let Some(next) = input.status_update {
            state.status = next;
            if state.status.is_terminal() {
                state.completed_at = Some(state.updated_at);
            }
        }
        if let Some(package_id) = input.package_id {
            state.package_id = Some(package_id);
        }
        if let Some(final_response_ref) = input.final_response_ref {
            state.final_response_ref = Some(final_response_ref);
        }
        if event.event_type == ResponseEventType::ResponseRequiresAction {
            state.requires_action_count += 1;
        }
        if state.status == ResponseStatus::Canceling {
            state.cancel_requested = true;
        }

        self.events
            .entry(state.response_id.clone())
            .or_default()
            .push(event.clone());
        self.states.insert(state.response_id.clone(), state.clone());
        Ok((state, event))
    }

    fn read_after(
        &self,
        response_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Vec<ResponseEvent>, ResponseStoreError> {
        if !self.states.contains_key(response_id) {
            return Err(ResponseStoreError::UnknownResponseId(
                response_id.to_string(),
            ));
        }
        let after_sequence = after_sequence.unwrap_or(0);
        Ok(self
            .events
            .get(response_id)
            .into_iter()
            .flatten()
            .filter(|event| event.sequence > after_sequence)
            .cloned()
            .collect())
    }
}

fn identity_idempotency_key(identity: &RunIdentity) -> Option<String> {
    let idempotency_key = identity.idempotency_key.as_ref()?;
    Some(format!(
        "{}:{}:{}",
        identity.owner_scope.tenant_id, identity.owner_scope.subject, idempotency_key
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::run_manager::{RunApiType, RunNormalizationInput, normalize_run};

    fn identity() -> RunIdentity {
        normalize_run(RunNormalizationInput {
            api_type: RunApiType::Responses,
            model: "tonglingyu".to_string(),
            session_id: Some("session-store-test".to_string()),
            auth_subject: "subject-store-test".to_string(),
            tenant_id: "tenant-store-test".to_string(),
            user_id: None,
            idempotency_key: Some("idem-store-test".to_string()),
            metadata: json!({"client": "store-test"}),
            request: json!({"model": "tonglingyu", "input": "问题"}),
            stream: false,
            background: false,
        })
        .expect("identity")
    }

    #[test]
    fn create_response_binds_run_id_to_response_id() {
        let identity = identity();
        let mut store = InMemoryResponseEventStore::default();

        let state = store.create_response(&identity).expect("state");

        assert_eq!(state.status, ResponseStatus::Queued);
        assert_eq!(
            store
                .response_id_for_run(&identity.run_id)
                .expect("mapping"),
            identity.response_id
        );
    }

    #[test]
    fn append_event_increments_sequence_and_updates_state_atomically() {
        let identity = identity();
        let mut store = InMemoryResponseEventStore::default();
        store.create_response(&identity).expect("state");

        let (state, event) = store
            .append_event(AppendResponseEventInput {
                response_id: identity.response_id.clone(),
                event_type: ResponseEventType::ResponseStatus,
                payload: json!({"status": "in_progress"}),
                status_update: Some(ResponseStatus::InProgress),
                visibility: None,
                package_id: None,
                final_response_ref: None,
            })
            .expect("append");

        assert_eq!(event.sequence, 1);
        assert_eq!(state.sequence, 1);
        assert_eq!(state.status, ResponseStatus::InProgress);
        assert_eq!(
            state.last_event_id.as_deref(),
            Some(event.event_id.as_str())
        );
    }

    #[test]
    fn terminal_state_cannot_be_overwritten() {
        let identity = identity();
        let mut store = InMemoryResponseEventStore::default();
        store.create_response(&identity).expect("state");
        store
            .append_event(AppendResponseEventInput {
                response_id: identity.response_id.clone(),
                event_type: ResponseEventType::ResponseStatus,
                payload: json!({"status": "in_progress"}),
                status_update: Some(ResponseStatus::InProgress),
                visibility: None,
                package_id: None,
                final_response_ref: None,
            })
            .expect("start");
        store
            .append_event(AppendResponseEventInput {
                response_id: identity.response_id.clone(),
                event_type: ResponseEventType::ResponseCompleted,
                payload: json!({"status": "completed"}),
                status_update: Some(ResponseStatus::Completed),
                visibility: None,
                package_id: None,
                final_response_ref: Some("final-journal-ref".to_string()),
            })
            .expect("complete");

        let error = store
            .append_event(AppendResponseEventInput {
                response_id: identity.response_id.clone(),
                event_type: ResponseEventType::ResponseStatus,
                payload: json!({"status": "in_progress"}),
                status_update: Some(ResponseStatus::InProgress),
                visibility: None,
                package_id: None,
                final_response_ref: None,
            })
            .expect_err("invalid transition");

        assert!(matches!(
            error,
            ResponseStoreError::InvalidStatusTransition {
                current: ResponseStatus::Completed,
                next: ResponseStatus::InProgress,
                ..
            }
        ));
    }

    #[test]
    fn read_after_replays_only_later_events() {
        let identity = identity();
        let mut store = InMemoryResponseEventStore::default();
        store.create_response(&identity).expect("state");
        for piece in ["一", "二", "三"] {
            store
                .append_event(AppendResponseEventInput {
                    response_id: identity.response_id.clone(),
                    event_type: ResponseEventType::OutputTextDelta,
                    payload: json!({"text": piece}),
                    status_update: Some(ResponseStatus::InProgress),
                    visibility: None,
                    package_id: None,
                    final_response_ref: None,
                })
                .expect("append");
        }

        let events = store
            .read_after(&identity.response_id, Some(1))
            .expect("events");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 2);
        assert_eq!(events[1].sequence, 3);
    }

    #[test]
    fn requires_action_updates_state_counter() {
        let identity = identity();
        let mut store = InMemoryResponseEventStore::default();
        store.create_response(&identity).expect("state");
        store
            .append_event(AppendResponseEventInput {
                response_id: identity.response_id.clone(),
                event_type: ResponseEventType::ResponseStatus,
                payload: json!({"status": "in_progress"}),
                status_update: Some(ResponseStatus::InProgress),
                visibility: None,
                package_id: None,
                final_response_ref: None,
            })
            .expect("start");

        let (state, event) = store
            .append_event(AppendResponseEventInput {
                response_id: identity.response_id.clone(),
                event_type: ResponseEventType::ResponseRequiresAction,
                payload: json!({"action_id": "act_1", "action_type": "human_approval"}),
                status_update: Some(ResponseStatus::RequiresAction),
                visibility: None,
                package_id: None,
                final_response_ref: None,
            })
            .expect("append");

        assert_eq!(event.visibility, ResponseVisibility::Public);
        assert_eq!(state.status, ResponseStatus::RequiresAction);
        assert_eq!(state.requires_action_count, 1);
    }

    #[test]
    fn duplicate_idempotency_key_returns_existing_response_id() {
        let first = identity();
        let mut second = identity();
        second.run_id = format!("run_{}", uuid::Uuid::now_v7().simple());
        second.response_id = format!("resp_{}", uuid::Uuid::now_v7().simple());
        let mut store = InMemoryResponseEventStore::default();
        store.create_response(&first).expect("first");

        let error = store
            .create_response(&second)
            .expect_err("duplicate idempotency key");

        assert_eq!(
            error,
            ResponseStoreError::DuplicateIdempotencyKey {
                response_id: first.response_id
            }
        );
    }
}
