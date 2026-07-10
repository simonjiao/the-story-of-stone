#![allow(dead_code)]

use std::collections::BTreeMap;

use redis::Commands;
use serde::{Deserialize, Serialize};

pub(crate) const PRODUCT_BINDING_SCHEMA_VERSION: &str = "tonglingyu.product_run_binding.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductBindingStatus {
    Queued,
    Running,
    RequiresAction,
    Canceling,
    Completed,
    Failed,
    Canceled,
}

impl ProductBindingStatus {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProductRunBinding {
    pub(crate) schema_version: String,
    pub(crate) response_id: String,
    pub(crate) run_id: String,
    pub(crate) product_id: String,
    pub(crate) executor: String,
    pub(crate) remote_run_id: Option<String>,
    pub(crate) remote_last_sequence: u64,
    pub(crate) pending_remote_action_id: Option<String>,
    pub(crate) openwebui_chat_id: String,
    pub(crate) openwebui_assistant_message_id: String,
    pub(crate) status: ProductBindingStatus,
    pub(crate) version: u64,
}

impl ProductRunBinding {
    pub(crate) fn new(
        response_id: impl Into<String>,
        run_id: impl Into<String>,
        product_id: impl Into<String>,
        chat_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: PRODUCT_BINDING_SCHEMA_VERSION.to_string(),
            response_id: response_id.into(),
            run_id: run_id.into(),
            product_id: product_id.into(),
            executor: "story-of-stone-studio".to_string(),
            remote_run_id: None,
            remote_last_sequence: 0,
            pending_remote_action_id: None,
            openwebui_chat_id: chat_id.into(),
            openwebui_assistant_message_id: message_id.into(),
            status: ProductBindingStatus::Queued,
            version: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProductBindingStoreError {
    BackendUnavailable(String),
    CorruptBinding(String),
    UnknownResponse(String),
    BindingConflict(String),
    ActiveChatConflict(String),
    SequenceRegression { current: u64, next: u64 },
}

#[derive(Debug, Default)]
pub(crate) struct InMemoryProductBindingStore {
    bindings: BTreeMap<String, ProductRunBinding>,
    active_chats: BTreeMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct RedisProductBindingStore {
    client: redis::Client,
    prefix: String,
    ttl_secs: u64,
}

#[derive(Debug)]
pub(crate) enum ProductBindingStoreBackend {
    InMemory(InMemoryProductBindingStore),
    Redis(RedisProductBindingStore),
}

impl ProductBindingStoreBackend {
    pub(crate) fn from_config(
        redis_url: Option<&str>,
        prefix: &str,
        ttl_secs: u64,
    ) -> Result<Self, ProductBindingStoreError> {
        let redis_url = redis_url.unwrap_or_default().trim();
        if redis_url.is_empty() {
            return Ok(Self::InMemory(InMemoryProductBindingStore::default()));
        }
        let client = redis::Client::open(redis_url)
            .map_err(|error| ProductBindingStoreError::BackendUnavailable(error.to_string()))?;
        let store = RedisProductBindingStore {
            client,
            prefix: sanitize_prefix(prefix),
            ttl_secs,
        };
        store.ping()?;
        Ok(Self::Redis(store))
    }

    pub(crate) fn is_durable(&self) -> bool {
        matches!(self, Self::Redis(_))
    }

    pub(crate) fn create(
        &mut self,
        binding: ProductRunBinding,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        match self {
            Self::InMemory(store) => store.create(binding),
            Self::Redis(store) => store.create(binding),
        }
    }

    pub(crate) fn get(
        &self,
        response_id: &str,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        match self {
            Self::InMemory(store) => store.get(response_id),
            Self::Redis(store) => store.get(response_id),
        }
    }

    pub(crate) fn save(
        &mut self,
        binding: ProductRunBinding,
        expected_version: u64,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        match self {
            Self::InMemory(store) => store.save(binding, expected_version),
            Self::Redis(store) => store.save(binding, expected_version),
        }
    }
}

impl InMemoryProductBindingStore {
    fn create(
        &mut self,
        binding: ProductRunBinding,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        if let Some(existing) = self.bindings.get(&binding.response_id) {
            return if same_identity(existing, &binding) {
                Ok(existing.clone())
            } else {
                Err(ProductBindingStoreError::BindingConflict(
                    binding.response_id,
                ))
            };
        }
        if let Some(response_id) = self.active_chats.get(&binding.openwebui_chat_id) {
            return Err(ProductBindingStoreError::ActiveChatConflict(
                response_id.clone(),
            ));
        }
        self.active_chats.insert(
            binding.openwebui_chat_id.clone(),
            binding.response_id.clone(),
        );
        self.bindings
            .insert(binding.response_id.clone(), binding.clone());
        Ok(binding)
    }

    fn get(&self, response_id: &str) -> Result<ProductRunBinding, ProductBindingStoreError> {
        self.bindings
            .get(response_id)
            .cloned()
            .ok_or_else(|| ProductBindingStoreError::UnknownResponse(response_id.to_string()))
    }

    fn save(
        &mut self,
        mut binding: ProductRunBinding,
        expected_version: u64,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        let current = self.get(&binding.response_id)?;
        validate_update(&current, &binding, expected_version)?;
        binding.version = expected_version + 1;
        if binding.status.is_terminal() {
            self.active_chats.remove(&binding.openwebui_chat_id);
        }
        self.bindings
            .insert(binding.response_id.clone(), binding.clone());
        Ok(binding)
    }
}

impl RedisProductBindingStore {
    fn ping(&self) -> Result<(), ProductBindingStoreError> {
        let mut connection = self.connection()?;
        let pong: String = redis::cmd("PING")
            .query(&mut connection)
            .map_err(backend_error)?;
        if pong.eq_ignore_ascii_case("PONG") {
            Ok(())
        } else {
            Err(ProductBindingStoreError::BackendUnavailable(format!(
                "redis ping returned {pong}"
            )))
        }
    }

    fn create(
        &self,
        binding: ProductRunBinding,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        let mut connection = self.connection()?;
        let binding_json = serde_json::to_string(&binding).map_err(corrupt_error)?;
        let reply: Vec<String> = redis::Script::new(
            r#"
local binding_key = KEYS[1]
local chat_key = KEYS[2]
local binding_json = ARGV[1]
local response_id = ARGV[2]
local ttl_secs = tonumber(ARGV[3])
local existing = redis.call("GET", binding_key)
if existing then return {"existing", existing} end
local active = redis.call("GET", chat_key)
if active then return {"active_chat", active} end
redis.call("SET", binding_key, binding_json)
redis.call("SET", chat_key, response_id)
if ttl_secs and ttl_secs > 0 then
  redis.call("EXPIRE", binding_key, ttl_secs)
  redis.call("EXPIRE", chat_key, ttl_secs)
end
return {"ok", binding_json}
"#,
        )
        .key(self.binding_key(&binding.response_id))
        .key(self.chat_key(&binding.openwebui_chat_id))
        .arg(&binding_json)
        .arg(&binding.response_id)
        .arg(self.ttl_secs)
        .invoke(&mut connection)
        .map_err(backend_error)?;
        match reply.first().map(String::as_str) {
            Some("ok") => Ok(binding),
            Some("existing") => {
                let existing = parse_binding(reply.get(1).map(String::as_str).unwrap_or_default())?;
                if same_identity(&existing, &binding) {
                    Ok(existing)
                } else {
                    Err(ProductBindingStoreError::BindingConflict(
                        binding.response_id,
                    ))
                }
            }
            Some("active_chat") => Err(ProductBindingStoreError::ActiveChatConflict(
                reply.get(1).cloned().unwrap_or_default(),
            )),
            _ => Err(ProductBindingStoreError::BackendUnavailable(
                "unexpected Redis product binding create reply".to_string(),
            )),
        }
    }

    fn get(&self, response_id: &str) -> Result<ProductRunBinding, ProductBindingStoreError> {
        let mut connection = self.connection()?;
        let value: Option<String> = connection
            .get(self.binding_key(response_id))
            .map_err(backend_error)?;
        value
            .as_deref()
            .map(parse_binding)
            .transpose()?
            .ok_or_else(|| ProductBindingStoreError::UnknownResponse(response_id.to_string()))
    }

    fn save(
        &self,
        mut binding: ProductRunBinding,
        expected_version: u64,
    ) -> Result<ProductRunBinding, ProductBindingStoreError> {
        let current = self.get(&binding.response_id)?;
        validate_update(&current, &binding, expected_version)?;
        binding.version = expected_version + 1;
        let mut connection = self.connection()?;
        let binding_json = serde_json::to_string(&binding).map_err(corrupt_error)?;
        let terminal = if binding.status.is_terminal() {
            "1"
        } else {
            "0"
        };
        let reply: Vec<String> = redis::Script::new(
            r#"
local binding_key = KEYS[1]
local chat_key = KEYS[2]
local expected_version = tonumber(ARGV[1])
local binding_json = ARGV[2]
local terminal = ARGV[3]
local ttl_secs = tonumber(ARGV[4])
local current_json = redis.call("GET", binding_key)
if not current_json then return {"missing", ""} end
local current = cjson.decode(current_json)
if tonumber(current["version"]) ~= expected_version then return {"conflict", current_json} end
redis.call("SET", binding_key, binding_json)
if terminal == "1" then redis.call("DEL", chat_key) end
if ttl_secs and ttl_secs > 0 then redis.call("EXPIRE", binding_key, ttl_secs) end
return {"ok", binding_json}
"#,
        )
        .key(self.binding_key(&binding.response_id))
        .key(self.chat_key(&binding.openwebui_chat_id))
        .arg(expected_version)
        .arg(&binding_json)
        .arg(terminal)
        .arg(self.ttl_secs)
        .invoke(&mut connection)
        .map_err(backend_error)?;
        match reply.first().map(String::as_str) {
            Some("ok") => Ok(binding),
            Some("missing") => Err(ProductBindingStoreError::UnknownResponse(
                binding.response_id,
            )),
            Some("conflict") => Err(ProductBindingStoreError::BindingConflict(
                binding.response_id,
            )),
            _ => Err(ProductBindingStoreError::BackendUnavailable(
                "unexpected Redis product binding save reply".to_string(),
            )),
        }
    }

    fn connection(&self) -> Result<redis::Connection, ProductBindingStoreError> {
        self.client.get_connection().map_err(backend_error)
    }

    fn binding_key(&self, response_id: &str) -> String {
        format!("{}:product-binding:{response_id}", self.prefix)
    }

    fn chat_key(&self, chat_id: &str) -> String {
        format!("{}:product-chat:{chat_id}:active", self.prefix)
    }
}

fn validate_update(
    current: &ProductRunBinding,
    next: &ProductRunBinding,
    expected_version: u64,
) -> Result<(), ProductBindingStoreError> {
    if current.version != expected_version || !same_identity(current, next) {
        return Err(ProductBindingStoreError::BindingConflict(
            current.response_id.clone(),
        ));
    }
    if next.remote_last_sequence < current.remote_last_sequence {
        return Err(ProductBindingStoreError::SequenceRegression {
            current: current.remote_last_sequence,
            next: next.remote_last_sequence,
        });
    }
    if current.remote_run_id.is_some() && next.remote_run_id != current.remote_run_id {
        return Err(ProductBindingStoreError::BindingConflict(
            current.response_id.clone(),
        ));
    }
    Ok(())
}

fn same_identity(left: &ProductRunBinding, right: &ProductRunBinding) -> bool {
    left.response_id == right.response_id
        && left.run_id == right.run_id
        && left.product_id == right.product_id
        && left.openwebui_chat_id == right.openwebui_chat_id
        && left.openwebui_assistant_message_id == right.openwebui_assistant_message_id
}

fn sanitize_prefix(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "tonglingyu".to_string()
    } else {
        value
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
}

fn parse_binding(value: &str) -> Result<ProductRunBinding, ProductBindingStoreError> {
    serde_json::from_str(value).map_err(corrupt_error)
}

fn backend_error(error: redis::RedisError) -> ProductBindingStoreError {
    ProductBindingStoreError::BackendUnavailable(error.to_string())
}

fn corrupt_error(error: serde_json::Error) -> ProductBindingStoreError {
    ProductBindingStoreError::CorruptBinding(error.to_string())
}

#[cfg(test)]
mod tests;
