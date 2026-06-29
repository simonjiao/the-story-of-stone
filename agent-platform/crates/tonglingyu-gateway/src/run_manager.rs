#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::response_events::ResponseStatus;

pub(crate) const RUN_MANAGER_CONTRACT_VERSION: &str = "tonglingyu.gateway.run_manager.v1";

const FORBIDDEN_CONTROL_FIELDS: &[&str] = &[
    "profile",
    "profiles",
    "agent",
    "agents",
    "tool_policy",
    "tool_policy_digest",
    "reviewer",
    "skip_review",
    "context_pack",
    "context_projection",
    "memory_card",
    "memory_read_refs",
    "runtime_adapter",
    "trace_id",
    "evidence_package_override",
    "callback_url",
    "webhook_url",
    "callback_secret",
    "run_store_override",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunApiType {
    ChatCompletions,
    Responses,
    Run,
    RealtimeWs,
}

#[derive(Debug, Clone)]
pub(crate) struct RunNormalizationInput {
    pub(crate) api_type: RunApiType,
    pub(crate) model: String,
    pub(crate) session_id: Option<String>,
    pub(crate) auth_subject: String,
    pub(crate) tenant_id: String,
    pub(crate) user_id: Option<String>,
    pub(crate) idempotency_key: Option<String>,
    pub(crate) metadata: Value,
    pub(crate) request: Value,
    pub(crate) stream: bool,
    pub(crate) background: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunIdentity {
    pub(crate) contract_version: String,
    pub(crate) run_id: String,
    pub(crate) response_id: String,
    pub(crate) chat_completion_id: Option<String>,
    pub(crate) session_id: String,
    pub(crate) trace_id: String,
    pub(crate) owner_scope: RunOwnerScope,
    pub(crate) api_type: RunApiType,
    pub(crate) model: String,
    pub(crate) idempotency_key: Option<String>,
    pub(crate) initial_status: ResponseStatus,
    pub(crate) request_digest: String,
    pub(crate) stream: bool,
    pub(crate) background: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunOwnerScope {
    pub(crate) tenant_id: String,
    pub(crate) subject: String,
    pub(crate) user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunNormalizationError {
    ForbiddenControlFields(Vec<String>),
    MetadataOverridesAuth(Vec<String>),
    EmptyAuthSubject,
    EmptyTenant,
    EmptyModel,
}

impl RunNormalizationError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::ForbiddenControlFields(_) => "forbidden_control_fields",
            Self::MetadataOverridesAuth(_) => "metadata_overrides_auth",
            Self::EmptyAuthSubject => "empty_auth_subject",
            Self::EmptyTenant => "empty_tenant",
            Self::EmptyModel => "empty_model",
        }
    }
}

pub(crate) fn normalize_run(
    input: RunNormalizationInput,
) -> Result<RunIdentity, RunNormalizationError> {
    if input.auth_subject.trim().is_empty() {
        return Err(RunNormalizationError::EmptyAuthSubject);
    }
    if input.tenant_id.trim().is_empty() {
        return Err(RunNormalizationError::EmptyTenant);
    }
    if input.model.trim().is_empty() {
        return Err(RunNormalizationError::EmptyModel);
    }

    let forbidden = forbidden_control_fields(&input.request);
    if !forbidden.is_empty() {
        return Err(RunNormalizationError::ForbiddenControlFields(forbidden));
    }
    let metadata_overrides = metadata_auth_overrides(&input.metadata);
    if !metadata_overrides.is_empty() {
        return Err(RunNormalizationError::MetadataOverridesAuth(
            metadata_overrides,
        ));
    }

    let run_id = format!("run_{}", uuid::Uuid::now_v7().simple());
    let response_id = format!("resp_{}", uuid::Uuid::now_v7().simple());
    let chat_completion_id = if input.api_type == RunApiType::ChatCompletions {
        Some(format!("chatcmpl_{}", uuid::Uuid::now_v7().simple()))
    } else {
        None
    };
    let session_id = input
        .session_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("session_{}", uuid::Uuid::now_v7().simple()));
    let trace_id = format!("tly-{}", uuid::Uuid::now_v7().simple());
    let request_digest = digest_value(&input.request);

    Ok(RunIdentity {
        contract_version: RUN_MANAGER_CONTRACT_VERSION.to_string(),
        run_id,
        response_id,
        chat_completion_id,
        session_id,
        trace_id,
        owner_scope: RunOwnerScope {
            tenant_id: input.tenant_id,
            subject: input.auth_subject,
            user_id: input.user_id,
        },
        api_type: input.api_type,
        model: input.model,
        idempotency_key: input.idempotency_key,
        initial_status: ResponseStatus::Queued,
        request_digest,
        stream: input.stream,
        background: input.background,
    })
}

pub(crate) fn identity_mapping(identity: &RunIdentity) -> Value {
    json!({
        "contract_version": identity.contract_version,
        "run_id": identity.run_id,
        "response_id": identity.response_id,
        "chat_completion_id": identity.chat_completion_id,
        "session_id": identity.session_id,
        "trace_id": identity.trace_id,
        "api_type": identity.api_type,
        "model": identity.model,
        "owner_scope": identity.owner_scope,
        "initial_status": identity.initial_status,
        "request_digest": identity.request_digest,
        "stream": identity.stream,
        "background": identity.background,
    })
}

fn forbidden_control_fields(value: &Value) -> Vec<String> {
    let mut fields = Vec::new();
    collect_forbidden_control_fields(value, "", &mut fields);
    fields.sort();
    fields.dedup();
    fields
}

fn collect_forbidden_control_fields(value: &Value, path: &str, fields: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let next_path = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };
                if is_forbidden_control_key(key) {
                    fields.push(next_path.clone());
                }
                collect_forbidden_control_fields(value, &next_path, fields);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let next_path = if path.is_empty() {
                    index.to_string()
                } else {
                    format!("{path}.{index}")
                };
                collect_forbidden_control_fields(value, &next_path, fields);
            }
        }
        _ => {}
    }
}

fn is_forbidden_control_key(key: &str) -> bool {
    FORBIDDEN_CONTROL_FIELDS
        .iter()
        .any(|forbidden| key.eq_ignore_ascii_case(forbidden))
}

fn metadata_auth_overrides(metadata: &Value) -> Vec<String> {
    let mut fields = Vec::new();
    if metadata.get("tenant_id").is_some() {
        fields.push("metadata.tenant_id".to_string());
    }
    if metadata.get("thread_id").is_some() {
        fields.push("metadata.thread_id".to_string());
    }
    if metadata.get("callback_url").is_some() {
        fields.push("metadata.callback_url".to_string());
    }
    fields
}

fn digest_value(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input(request: Value) -> RunNormalizationInput {
        RunNormalizationInput {
            api_type: RunApiType::Responses,
            model: "tonglingyu".to_string(),
            session_id: Some("session-existing".to_string()),
            auth_subject: "subject-user".to_string(),
            tenant_id: "tenant-auth".to_string(),
            user_id: Some("user-auth".to_string()),
            idempotency_key: Some("message-1".to_string()),
            metadata: json!({"client": "test"}),
            request,
            stream: false,
            background: false,
        }
    }

    #[test]
    fn normalize_run_binds_all_external_ids_to_one_identity() {
        let identity = normalize_run(base_input(json!({
            "model": "tonglingyu",
            "input": "问题"
        })))
        .expect("identity");

        assert!(identity.run_id.starts_with("run_"));
        assert!(identity.response_id.starts_with("resp_"));
        assert!(identity.chat_completion_id.is_none());
        assert_eq!(identity.session_id, "session-existing");
        assert_eq!(identity.owner_scope.tenant_id, "tenant-auth");
        assert_eq!(identity.initial_status, ResponseStatus::Queued);
        assert_eq!(identity.contract_version, RUN_MANAGER_CONTRACT_VERSION);
        assert_eq!(identity.idempotency_key.as_deref(), Some("message-1"));
    }

    #[test]
    fn chat_completions_gets_chat_completion_projection_id() {
        let mut input = base_input(json!({"model": "tonglingyu", "messages": []}));
        input.api_type = RunApiType::ChatCompletions;

        let identity = normalize_run(input).expect("identity");

        assert!(identity.chat_completion_id.is_some());
        assert!(
            identity
                .chat_completion_id
                .as_deref()
                .unwrap_or_default()
                .starts_with("chatcmpl_")
        );
    }

    #[test]
    fn forbidden_control_fields_fail_closed_before_workflow_queue() {
        let error = normalize_run(base_input(json!({
            "model": "tonglingyu",
            "input": "问题",
            "metadata": {"nested": {"skip_review": true}}
        })))
        .expect_err("forbidden control field");

        assert_eq!(error.code(), "forbidden_control_fields");
        assert_eq!(
            error,
            RunNormalizationError::ForbiddenControlFields(vec![
                "metadata.nested.skip_review".to_string()
            ])
        );
    }

    #[test]
    fn metadata_cannot_override_authenticated_scope_or_callback_policy() {
        let mut input = base_input(json!({"model": "tonglingyu", "input": "问题"}));
        input.metadata = json!({
            "tenant_id": "tenant-attacker",
            "thread_id": "thread-attacker",
            "callback_url": "https://attacker.invalid/webhook"
        });

        let error = normalize_run(input).expect_err("metadata override");

        assert_eq!(error.code(), "metadata_overrides_auth");
        assert_eq!(
            error,
            RunNormalizationError::MetadataOverridesAuth(vec![
                "metadata.tenant_id".to_string(),
                "metadata.thread_id".to_string(),
                "metadata.callback_url".to_string(),
            ])
        );
    }

    #[test]
    fn identity_mapping_keeps_run_response_and_chat_ids_together() {
        let mut input = base_input(json!({"model": "tonglingyu", "messages": []}));
        input.api_type = RunApiType::ChatCompletions;
        let identity = normalize_run(input).expect("identity");

        let mapping = identity_mapping(&identity);

        assert_eq!(mapping["run_id"], json!(identity.run_id));
        assert_eq!(mapping["response_id"], json!(identity.response_id));
        assert_eq!(
            mapping["chat_completion_id"],
            json!(identity.chat_completion_id)
        );
        assert_eq!(mapping["initial_status"], json!("queued"));
    }
}
