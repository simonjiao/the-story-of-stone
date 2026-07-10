#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const GATEWAY_CAPABILITIES_SCHEMA_VERSION: &str =
    "story-of-stone.gateway-capabilities.v1";
pub(crate) const PRODUCT_RUN_SCHEMA_VERSION: &str = "story-of-stone.product-run.v1";
pub(crate) const PRODUCT_RUN_EVENT_SCHEMA_VERSION: &str = "story-of-stone.product-run-event.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GatewayCapabilities {
    pub(crate) schema_version: String,
    pub(crate) service: String,
    pub(crate) protocol_versions: Vec<String>,
    pub(crate) products: Vec<ProductCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductCapability {
    pub(crate) id: String,
    pub(crate) actions: bool,
    pub(crate) artifacts: Vec<String>,
    pub(crate) stream: ProductStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductStream {
    Sse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunCreateRequest {
    pub(crate) schema_version: String,
    pub(crate) request_id: String,
    pub(crate) external_message_id: String,
    pub(crate) trace_id: String,
    pub(crate) product_id: String,
    pub(crate) identity: ProductRunIdentity,
    pub(crate) input: ProductRunInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunIdentity {
    pub(crate) issuer: String,
    pub(crate) user_ref: String,
    pub(crate) chat_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunInput {
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) article_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) section_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) replace_existing: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunRecord {
    pub(crate) id: String,
    pub(crate) schema_version: String,
    pub(crate) request_id: String,
    pub(crate) external_message_id: String,
    pub(crate) trace_id: String,
    pub(crate) product_id: String,
    pub(crate) identity: ProductRunIdentity,
    pub(crate) workflow_run_id: String,
    pub(crate) status: ProductRunStatus,
    pub(crate) last_sequence: u64,
    pub(crate) pending_action: Option<ProductRunAction>,
    pub(crate) artifacts: Vec<ProductArtifactRef>,
    pub(crate) error: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductRunStatus {
    Queued,
    Running,
    RequiresAction,
    Completed,
    Failed,
    Canceling,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunAction {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) action_type: String,
    pub(crate) title: String,
    pub(crate) options: Vec<ProductRunActionOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunActionOption {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductArtifactRef {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunEvent {
    pub(crate) schema_version: String,
    pub(crate) event_id: String,
    pub(crate) sequence: u64,
    pub(crate) run_id: String,
    pub(crate) product_id: String,
    #[serde(rename = "type")]
    pub(crate) event_type: ProductRunEventType,
    pub(crate) created_at: String,
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProductRunEventType {
    #[serde(rename = "run.started")]
    RunStarted,
    #[serde(rename = "run.status")]
    RunStatus,
    #[serde(rename = "artifact.updated")]
    ArtifactUpdated,
    #[serde(rename = "run.requires_action")]
    RunRequiresAction,
    #[serde(rename = "run.resumed")]
    RunResumed,
    #[serde(rename = "run.completed")]
    RunCompleted,
    #[serde(rename = "run.failed")]
    RunFailed,
    #[serde(rename = "run.canceled")]
    RunCanceled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductRunActionSubmission {
    pub(crate) decision: ProductRunActionDecision,
    #[serde(default)]
    pub(crate) payload: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductRunActionDecision {
    Accept,
    Reject,
}

pub(crate) fn validate_capabilities(
    value: GatewayCapabilities,
) -> Result<GatewayCapabilities, String> {
    require_exact(
        &value.schema_version,
        GATEWAY_CAPABILITIES_SCHEMA_VERSION,
        "capabilities schema",
    )?;
    require_exact(
        &value.service,
        "story-of-stone-studio",
        "capabilities service",
    )?;
    if !value
        .protocol_versions
        .iter()
        .any(|version| version == PRODUCT_RUN_SCHEMA_VERSION)
        || !value
            .protocol_versions
            .iter()
            .any(|version| version == PRODUCT_RUN_EVENT_SCHEMA_VERSION)
    {
        return Err("Studio does not declare the required Product Run protocols".to_string());
    }
    if value
        .products
        .iter()
        .any(|product| product.id.trim().is_empty())
    {
        return Err("Studio capabilities contain an empty product id".to_string());
    }
    Ok(value)
}

pub(crate) fn validate_create_request(value: &ProductRunCreateRequest) -> Result<(), String> {
    require_exact(
        &value.schema_version,
        PRODUCT_RUN_SCHEMA_VERSION,
        "product run schema",
    )?;
    for (label, field) in [
        ("request_id", value.request_id.as_str()),
        ("external_message_id", value.external_message_id.as_str()),
        ("trace_id", value.trace_id.as_str()),
        ("product_id", value.product_id.as_str()),
        ("identity.user_ref", value.identity.user_ref.as_str()),
        ("identity.chat_ref", value.identity.chat_ref.as_str()),
        ("input.message", value.input.message.as_str()),
    ] {
        if field.trim().is_empty() {
            return Err(format!("{label} must not be empty"));
        }
    }
    require_exact(
        &value.identity.issuer,
        "tonglingyu-gateway",
        "identity issuer",
    )
}

pub(crate) fn validate_event(value: &ProductRunEvent, after_sequence: u64) -> Result<(), String> {
    require_exact(
        &value.schema_version,
        PRODUCT_RUN_EVENT_SCHEMA_VERSION,
        "product run event schema",
    )?;
    if value.event_id.trim().is_empty()
        || value.run_id.trim().is_empty()
        || value.product_id.trim().is_empty()
    {
        return Err("product run event ids must not be empty".to_string());
    }
    if value.sequence == 0 || value.sequence <= after_sequence {
        return Err(format!(
            "product run event sequence {} is not after {after_sequence}",
            value.sequence
        ));
    }
    Ok(())
}

fn require_exact(value: &str, expected: &str, label: &str) -> Result<(), String> {
    if value == expected {
        Ok(())
    } else {
        Err(format!("unsupported {label}: {value}"))
    }
}

#[cfg(test)]
mod tests;
