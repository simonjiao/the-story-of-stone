#![allow(dead_code)]

use std::collections::BTreeMap;

use redis::{Commands, streams::StreamRangeReply};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    StateConflict {
        response_id: String,
    },
    BackendUnavailable(String),
    CorruptState(String),
    CorruptEvent(String),
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
    control_events: BTreeMap<String, Vec<Value>>,
    action_events: BTreeMap<String, Vec<Value>>,
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
        let previous_status = state.status.clone();
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
            if previous_status == ResponseStatus::RequiresAction
                && next == ResponseStatus::InProgress
                && state.requires_action_count > 0
            {
                state.requires_action_count -= 1;
            }
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

#[derive(Debug, Clone)]
pub(crate) struct ResponseStoreConfig {
    pub(crate) redis_url: Option<String>,
    pub(crate) redis_required: bool,
    pub(crate) stream_prefix: String,
    pub(crate) event_maxlen: usize,
    pub(crate) event_ttl_secs: u64,
}

impl Default for ResponseStoreConfig {
    fn default() -> Self {
        Self {
            redis_url: None,
            redis_required: false,
            stream_prefix: "tonglingyu".to_string(),
            event_maxlen: 2000,
            event_ttl_secs: 86_400,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponseStoreHealth {
    pub(crate) mode: &'static str,
    pub(crate) required: bool,
    pub(crate) prefix: String,
    pub(crate) event_maxlen: usize,
    pub(crate) event_ttl_secs: u64,
    pub(crate) status: &'static str,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) enum ResponseStoreBackend {
    InMemory(InMemoryResponseEventStore),
    Redis(RedisResponseEventStore),
}

impl ResponseStoreBackend {
    pub(crate) fn from_config(config: ResponseStoreConfig) -> Result<Self, ResponseStoreError> {
        let redis_url = config.redis_url.clone().unwrap_or_default();
        let redis_url = redis_url.trim().to_string();
        if redis_url.is_empty() {
            if config.redis_required {
                return Err(ResponseStoreError::BackendUnavailable(
                    "TONGLINGYU_REDIS_URL is required when TONGLINGYU_REDIS_REQUIRED=true"
                        .to_string(),
                ));
            }
            return Ok(Self::InMemory(InMemoryResponseEventStore::default()));
        }
        let store = RedisResponseEventStore::new(&redis_url, config)?;
        store.ping()?;
        Ok(Self::Redis(store))
    }

    pub(crate) fn health_snapshot(&self) -> ResponseStoreHealth {
        match self {
            Self::InMemory(_) => ResponseStoreHealth {
                mode: "in_memory",
                required: false,
                prefix: "local".to_string(),
                event_maxlen: 0,
                event_ttl_secs: 0,
                status: "ok",
                error: None,
            },
            Self::Redis(store) => match store.ping() {
                Ok(()) => ResponseStoreHealth {
                    mode: "redis",
                    required: store.redis_required,
                    prefix: store.prefix.clone(),
                    event_maxlen: store.event_maxlen,
                    event_ttl_secs: store.event_ttl_secs,
                    status: "ok",
                    error: None,
                },
                Err(error) => ResponseStoreHealth {
                    mode: "redis",
                    required: store.redis_required,
                    prefix: store.prefix.clone(),
                    event_maxlen: store.event_maxlen,
                    event_ttl_secs: store.event_ttl_secs,
                    status: "unavailable",
                    error: Some(format!("{error:?}")),
                },
            },
        }
    }

    pub(crate) fn append_control_event(
        &mut self,
        response_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<String, ResponseStoreError> {
        match self {
            Self::InMemory(store) => store.append_control_event(response_id, event_type, payload),
            Self::Redis(store) => store.append_control_event(response_id, event_type, payload),
        }
    }

    pub(crate) fn append_action_event(
        &mut self,
        response_id: &str,
        action_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<String, ResponseStoreError> {
        match self {
            Self::InMemory(store) => {
                store.append_action_event(response_id, action_id, event_type, payload)
            }
            Self::Redis(store) => {
                store.append_action_event(response_id, action_id, event_type, payload)
            }
        }
    }
}

impl ResponseEventStore for ResponseStoreBackend {
    fn create_response(
        &mut self,
        identity: &RunIdentity,
    ) -> Result<ResponseStateRecord, ResponseStoreError> {
        match self {
            Self::InMemory(store) => store.create_response(identity),
            Self::Redis(store) => store.create_response(identity),
        }
    }

    fn response_id_for_run(&self, run_id: &str) -> Result<String, ResponseStoreError> {
        match self {
            Self::InMemory(store) => store.response_id_for_run(run_id),
            Self::Redis(store) => store.response_id_for_run(run_id),
        }
    }

    fn state(&self, response_id: &str) -> Result<ResponseStateRecord, ResponseStoreError> {
        match self {
            Self::InMemory(store) => store.state(response_id),
            Self::Redis(store) => store.state(response_id),
        }
    }

    fn append_event(
        &mut self,
        input: AppendResponseEventInput,
    ) -> Result<(ResponseStateRecord, ResponseEvent), ResponseStoreError> {
        match self {
            Self::InMemory(store) => store.append_event(input),
            Self::Redis(store) => store.append_event(input),
        }
    }

    fn read_after(
        &self,
        response_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Vec<ResponseEvent>, ResponseStoreError> {
        match self {
            Self::InMemory(store) => store.read_after(response_id, after_sequence),
            Self::Redis(store) => store.read_after(response_id, after_sequence),
        }
    }
}

impl InMemoryResponseEventStore {
    pub(crate) fn append_control_event(
        &mut self,
        response_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<String, ResponseStoreError> {
        self.state(response_id)?;
        let event_id = format!("ctrl_{}", uuid::Uuid::now_v7().simple());
        self.control_events
            .entry(response_id.to_string())
            .or_default()
            .push(json!({
                "event_id": event_id,
                "event_type": event_type,
                "payload": payload,
                "created_at": OffsetDateTime::now_utc(),
            }));
        Ok(event_id)
    }

    pub(crate) fn append_action_event(
        &mut self,
        response_id: &str,
        action_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<String, ResponseStoreError> {
        self.state(response_id)?;
        let event_id = format!("act_{}", uuid::Uuid::now_v7().simple());
        self.action_events
            .entry(response_id.to_string())
            .or_default()
            .push(json!({
                "event_id": event_id,
                "action_id": action_id,
                "event_type": event_type,
                "payload": payload,
                "created_at": OffsetDateTime::now_utc(),
            }));
        Ok(event_id)
    }
}

#[derive(Debug)]
pub(crate) struct RedisResponseEventStore {
    client: redis::Client,
    prefix: String,
    redis_required: bool,
    event_maxlen: usize,
    event_ttl_secs: u64,
}

impl RedisResponseEventStore {
    pub(crate) fn new(
        redis_url: &str,
        config: ResponseStoreConfig,
    ) -> Result<Self, ResponseStoreError> {
        let client =
            redis::Client::open(redis_url).map_err(|error| redis_backend_error(error, "open"))?;
        Ok(Self {
            client,
            prefix: sanitize_prefix(&config.stream_prefix),
            redis_required: config.redis_required,
            event_maxlen: config.event_maxlen.max(1),
            event_ttl_secs: config.event_ttl_secs,
        })
    }

    fn connection(&self) -> Result<redis::Connection, ResponseStoreError> {
        self.client
            .get_connection()
            .map_err(|error| redis_backend_error(error, "connect"))
    }

    fn ping(&self) -> Result<(), ResponseStoreError> {
        let mut connection = self.connection()?;
        let pong: String = redis::cmd("PING")
            .query(&mut connection)
            .map_err(|error| redis_backend_error(error, "ping"))?;
        if pong.eq_ignore_ascii_case("PONG") {
            Ok(())
        } else {
            Err(ResponseStoreError::BackendUnavailable(format!(
                "redis ping returned {pong}"
            )))
        }
    }

    fn response_state_key(&self, response_id: &str) -> String {
        format!("{}:response:{response_id}:state", self.prefix)
    }

    fn response_events_key(&self, response_id: &str) -> String {
        format!("{}:response:{response_id}:events", self.prefix)
    }

    fn response_control_key(&self, response_id: &str) -> String {
        format!("{}:response:{response_id}:control", self.prefix)
    }

    fn response_actions_key(&self, response_id: &str) -> String {
        format!("{}:response:{response_id}:actions", self.prefix)
    }

    fn run_response_key(&self, run_id: &str) -> String {
        format!("{}:run:{run_id}:response_id", self.prefix)
    }

    fn idempotency_key(&self, identity: &RunIdentity) -> Option<String> {
        identity_idempotency_key(identity).map(|key| format!("{}:idempotency:{key}", self.prefix))
    }

    fn state_from_json(&self, value: &str) -> Result<ResponseStateRecord, ResponseStoreError> {
        serde_json::from_str(value)
            .map_err(|error| ResponseStoreError::CorruptState(error.to_string()))
    }

    fn append_control_event(
        &mut self,
        response_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<String, ResponseStoreError> {
        self.state(response_id)?;
        let mut connection = self.connection()?;
        let event_id = format!("ctrl_{}", uuid::Uuid::now_v7().simple());
        let payload_json = serde_json::to_string(&json!({
            "event_id": event_id,
            "event_type": event_type,
            "payload": payload,
            "created_at": OffsetDateTime::now_utc(),
        }))
        .map_err(|error| ResponseStoreError::CorruptEvent(error.to_string()))?;
        let stream_id: String = redis::cmd("XADD")
            .arg(self.response_control_key(response_id))
            .arg("MAXLEN")
            .arg("~")
            .arg(self.event_maxlen)
            .arg("*")
            .arg("event_json")
            .arg(payload_json)
            .query(&mut connection)
            .map_err(|error| redis_backend_error(error, "xadd-control"))?;
        self.expire_key(&mut connection, &self.response_control_key(response_id))?;
        Ok(stream_id)
    }

    fn append_action_event(
        &mut self,
        response_id: &str,
        action_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<String, ResponseStoreError> {
        self.state(response_id)?;
        let mut connection = self.connection()?;
        let event_id = format!("act_{}", uuid::Uuid::now_v7().simple());
        let payload_json = serde_json::to_string(&json!({
            "event_id": event_id,
            "action_id": action_id,
            "event_type": event_type,
            "payload": payload,
            "created_at": OffsetDateTime::now_utc(),
        }))
        .map_err(|error| ResponseStoreError::CorruptEvent(error.to_string()))?;
        let stream_id: String = redis::cmd("XADD")
            .arg(self.response_actions_key(response_id))
            .arg("MAXLEN")
            .arg("~")
            .arg(self.event_maxlen)
            .arg("*")
            .arg("event_json")
            .arg(payload_json)
            .query(&mut connection)
            .map_err(|error| redis_backend_error(error, "xadd-action"))?;
        self.expire_key(&mut connection, &self.response_actions_key(response_id))?;
        Ok(stream_id)
    }

    fn expire_key(
        &self,
        connection: &mut redis::Connection,
        key: &str,
    ) -> Result<(), ResponseStoreError> {
        if self.event_ttl_secs == 0 {
            return Ok(());
        }
        let _: () = redis::cmd("EXPIRE")
            .arg(key)
            .arg(self.event_ttl_secs)
            .query(connection)
            .map_err(|error| redis_backend_error(error, "expire"))?;
        Ok(())
    }
}

impl ResponseEventStore for RedisResponseEventStore {
    fn create_response(
        &mut self,
        identity: &RunIdentity,
    ) -> Result<ResponseStateRecord, ResponseStoreError> {
        let state = ResponseStateRecord::new(identity);
        let state_json = serde_json::to_string(&state)
            .map_err(|error| ResponseStoreError::CorruptState(error.to_string()))?;
        let idempotency_key = self
            .idempotency_key(identity)
            .unwrap_or_else(|| format!("{}:idempotency:none", self.prefix));
        let has_idempotency = if identity.idempotency_key.is_some() {
            "1"
        } else {
            "0"
        };
        let mut connection = self.connection()?;
        let reply: Vec<String> = redis::Script::new(
            r#"
local state_key = KEYS[1]
local run_key = KEYS[2]
local idempotency_key = KEYS[3]
local response_id = ARGV[1]
local run_id = ARGV[2]
local state_json = ARGV[3]
local has_idempotency = ARGV[4]
local ttl_secs = tonumber(ARGV[5])
if has_idempotency == "1" then
  local existing = redis.call("GET", idempotency_key)
  if existing then
    return {"duplicate_idempotency", existing}
  end
end
if redis.call("EXISTS", state_key) == 1 then
  return {"duplicate_response", response_id}
end
if redis.call("EXISTS", run_key) == 1 then
  return {"duplicate_run", run_id}
end
redis.call("SET", state_key, state_json)
redis.call("SET", run_key, response_id)
if has_idempotency == "1" then
  redis.call("SET", idempotency_key, response_id)
end
if ttl_secs and ttl_secs > 0 then
  redis.call("EXPIRE", state_key, ttl_secs)
  redis.call("EXPIRE", run_key, ttl_secs)
  if has_idempotency == "1" then
    redis.call("EXPIRE", idempotency_key, ttl_secs)
  end
end
return {"ok", state_json}
"#,
        )
        .key(self.response_state_key(&identity.response_id))
        .key(self.run_response_key(&identity.run_id))
        .key(idempotency_key)
        .arg(&identity.response_id)
        .arg(&identity.run_id)
        .arg(state_json)
        .arg(has_idempotency)
        .arg(self.event_ttl_secs)
        .invoke(&mut connection)
        .map_err(|error| redis_backend_error(error, "create-response"))?;
        match redis_script_status(&reply).as_deref() {
            Some("ok") => Ok(state),
            Some("duplicate_idempotency") => Err(ResponseStoreError::DuplicateIdempotencyKey {
                response_id: reply.get(1).cloned().unwrap_or_default(),
            }),
            Some("duplicate_response") => Err(ResponseStoreError::DuplicateResponseId(
                identity.response_id.clone(),
            )),
            Some("duplicate_run") => {
                Err(ResponseStoreError::DuplicateRunId(identity.run_id.clone()))
            }
            _ => Err(ResponseStoreError::BackendUnavailable(
                "unexpected redis create_response reply".to_string(),
            )),
        }
    }

    fn response_id_for_run(&self, run_id: &str) -> Result<String, ResponseStoreError> {
        let mut connection = self.connection()?;
        let response_id: Option<String> = connection
            .get(self.run_response_key(run_id))
            .map_err(|error| redis_backend_error(error, "get-run-response"))?;
        response_id.ok_or_else(|| ResponseStoreError::UnknownRunId(run_id.to_string()))
    }

    fn state(&self, response_id: &str) -> Result<ResponseStateRecord, ResponseStoreError> {
        let mut connection = self.connection()?;
        let state_json: Option<String> = connection
            .get(self.response_state_key(response_id))
            .map_err(|error| redis_backend_error(error, "get-state"))?;
        let state_json =
            state_json.ok_or_else(|| ResponseStoreError::UnknownResponseId(response_id.into()))?;
        self.state_from_json(&state_json)
    }

    fn append_event(
        &mut self,
        input: AppendResponseEventInput,
    ) -> Result<(ResponseStateRecord, ResponseEvent), ResponseStoreError> {
        let mut state = self.state(&input.response_id)?;
        let expected_sequence = state.sequence;
        let expected_status = serde_json::to_string(&state.status)
            .map_err(|error| ResponseStoreError::CorruptState(error.to_string()))?
            .trim_matches('"')
            .to_string();
        let previous_status = state.status.clone();
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
            if previous_status == ResponseStatus::RequiresAction
                && next == ResponseStatus::InProgress
                && state.requires_action_count > 0
            {
                state.requires_action_count -= 1;
            }
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

        let state_json = serde_json::to_string(&state)
            .map_err(|error| ResponseStoreError::CorruptState(error.to_string()))?;
        let event_json = serde_json::to_string(&event)
            .map_err(|error| ResponseStoreError::CorruptEvent(error.to_string()))?;
        let event_type_json = serde_json::to_string(&event.event_type)
            .map_err(|error| ResponseStoreError::CorruptEvent(error.to_string()))?
            .trim_matches('"')
            .to_string();
        let visibility_json = serde_json::to_string(&event.visibility)
            .map_err(|error| ResponseStoreError::CorruptEvent(error.to_string()))?
            .trim_matches('"')
            .to_string();
        let mut connection = self.connection()?;
        let reply: Vec<String> = redis::Script::new(
            r#"
local state_key = KEYS[1]
local events_key = KEYS[2]
local expected_sequence = ARGV[1]
local expected_status = ARGV[2]
local next_state_json = ARGV[3]
local event_json = ARGV[4]
local maxlen = tonumber(ARGV[5])
local ttl_secs = tonumber(ARGV[6])
local event_sequence = ARGV[7]
local event_type = ARGV[8]
local visibility = ARGV[9]
local current_json = redis.call("GET", state_key)
if not current_json then
  return {"unknown_response", ""}
end
local current = cjson.decode(current_json)
if tostring(current["sequence"]) ~= expected_sequence or current["status"] ~= expected_status then
  return {"state_conflict", current_json}
end
local stream_id = redis.call(
  "XADD", events_key, "MAXLEN", "~", maxlen, "*",
  "event_json", event_json,
  "sequence", event_sequence,
  "event_type", event_type,
  "visibility", visibility
)
redis.call("SET", state_key, next_state_json)
if ttl_secs and ttl_secs > 0 then
  redis.call("EXPIRE", state_key, ttl_secs)
  redis.call("EXPIRE", events_key, ttl_secs)
end
return {"ok", stream_id}
"#,
        )
        .key(self.response_state_key(&state.response_id))
        .key(self.response_events_key(&state.response_id))
        .arg(expected_sequence.to_string())
        .arg(expected_status)
        .arg(state_json)
        .arg(event_json)
        .arg(self.event_maxlen)
        .arg(self.event_ttl_secs)
        .arg(event.sequence.to_string())
        .arg(event_type_json)
        .arg(visibility_json)
        .invoke(&mut connection)
        .map_err(|error| redis_backend_error(error, "append-event"))?;
        match redis_script_status(&reply).as_deref() {
            Some("ok") => Ok((state, event)),
            Some("unknown_response") => Err(ResponseStoreError::UnknownResponseId(
                input.response_id.clone(),
            )),
            Some("state_conflict") => Err(ResponseStoreError::StateConflict {
                response_id: input.response_id.clone(),
            }),
            _ => Err(ResponseStoreError::BackendUnavailable(
                "unexpected redis append_event reply".to_string(),
            )),
        }
    }

    fn read_after(
        &self,
        response_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Vec<ResponseEvent>, ResponseStoreError> {
        self.state(response_id)?;
        let mut connection = self.connection()?;
        let reply: StreamRangeReply = connection
            .xrange_all(self.response_events_key(response_id))
            .map_err(|error| redis_backend_error(error, "xrange-events"))?;
        let after_sequence = after_sequence.unwrap_or(0);
        let mut events = Vec::new();
        for stream_id in reply.ids {
            let event_json: Option<String> = stream_id.get("event_json");
            let Some(event_json) = event_json else {
                return Err(ResponseStoreError::CorruptEvent(
                    "redis stream event_json field is missing".to_string(),
                ));
            };
            let event: ResponseEvent = serde_json::from_str(&event_json)
                .map_err(|error| ResponseStoreError::CorruptEvent(error.to_string()))?;
            if event.sequence > after_sequence {
                events.push(event);
            }
        }
        events.sort_by_key(|event| event.sequence);
        Ok(events)
    }
}

fn redis_script_status(reply: &[String]) -> Option<String> {
    reply.first().cloned()
}

fn redis_backend_error(error: redis::RedisError, operation: &str) -> ResponseStoreError {
    ResponseStoreError::BackendUnavailable(format!("redis {operation} failed: {error}"))
}

fn sanitize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return "tonglingyu".to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
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

    #[test]
    fn redis_required_without_url_fails_closed() {
        let error = ResponseStoreBackend::from_config(ResponseStoreConfig {
            redis_url: None,
            redis_required: true,
            ..ResponseStoreConfig::default()
        })
        .expect_err("required redis should fail without url");

        assert!(matches!(error, ResponseStoreError::BackendUnavailable(_)));
    }

    #[test]
    fn missing_redis_url_uses_memory_only_when_not_required() {
        let store = ResponseStoreBackend::from_config(ResponseStoreConfig {
            redis_url: Some("   ".to_string()),
            redis_required: false,
            ..ResponseStoreConfig::default()
        })
        .expect("memory store");

        let health = store.health_snapshot();
        assert_eq!(health.mode, "in_memory");
        assert_eq!(health.status, "ok");
    }

    #[test]
    fn control_and_action_events_require_known_response() {
        let identity = identity();
        let mut store = InMemoryResponseEventStore::default();
        store.create_response(&identity).expect("state");

        let control_id = store
            .append_control_event(
                &identity.response_id,
                "cancel_requested",
                json!({"reason": "test"}),
            )
            .expect("control event");
        let action_id = store
            .append_action_event(
                &identity.response_id,
                "act_test",
                "action_submit_rejected",
                json!({"reason": "not_waiting"}),
            )
            .expect("action event");

        assert!(control_id.starts_with("ctrl_"));
        assert!(action_id.starts_with("act_"));
        assert!(matches!(
            store.append_control_event("resp_missing", "cancel_requested", json!({})),
            Err(ResponseStoreError::UnknownResponseId(_))
        ));
        assert!(matches!(
            store.append_action_event("resp_missing", "act_missing", "submit", json!({})),
            Err(ResponseStoreError::UnknownResponseId(_))
        ));
    }

    #[test]
    fn redis_prefix_is_sanitized_for_key_material() {
        assert_eq!(sanitize_prefix(" tly prod "), "tly_prod");
        assert_eq!(sanitize_prefix(""), "tonglingyu");
        assert_eq!(sanitize_prefix("tly:prod-1"), "tly:prod-1");
    }
}
