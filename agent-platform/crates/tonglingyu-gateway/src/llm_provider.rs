use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::llm_modes::LlmMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderRequest {
    pub capability: String,
    pub mode: LlmMode,
    pub schema_name: String,
    pub schema_version: String,
    pub timeout_ms: u64,
    pub input_json: Value,
    pub projection_digest: String,
    pub trace_ref: String,
    pub replay_anchor: String,
    #[serde(default)]
    pub repair_attempt: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderResponse {
    pub raw_response_sha256: String,
    pub parsed_json: Value,
    pub usage: Value,
    pub latency_ms: u64,
    pub provider_model: String,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderError {
    Timeout,
    RateLimited,
    AuthError,
    ProviderUnavailable,
    SchemaInvalid,
    SchemaRepairFailed,
    SafetyRefusal,
    BudgetExceeded,
    ProfileMissing,
    ProjectionDigestMismatch,
}

impl LlmProviderError {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::RateLimited => "rate_limited",
            Self::AuthError => "auth_error",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::SchemaInvalid => "schema_invalid",
            Self::SchemaRepairFailed => "schema_repair_failed",
            Self::SafetyRefusal => "safety_refusal",
            Self::BudgetExceeded => "budget_exceeded",
            Self::ProfileMissing => "profile_missing",
            Self::ProjectionDigestMismatch => "projection_digest_mismatch",
        }
    }
}

pub trait LlmProviderClient {
    fn complete_json(
        &mut self,
        request: LlmProviderRequest,
    ) -> Result<LlmProviderResponse, LlmProviderError>;
}

#[derive(Debug, Clone)]
pub struct FakeLlmProvider {
    responses: VecDeque<Result<Value, LlmProviderError>>,
}

impl FakeLlmProvider {
    pub fn new(responses: Vec<Result<Value, LlmProviderError>>) -> Self {
        Self {
            responses: responses.into(),
        }
    }
}

impl LlmProviderClient for FakeLlmProvider {
    fn complete_json(
        &mut self,
        request: LlmProviderRequest,
    ) -> Result<LlmProviderResponse, LlmProviderError> {
        match self.responses.pop_front() {
            Some(Ok(parsed_json)) => Ok(LlmProviderResponse {
                raw_response_sha256: "sha256:fake-provider-response".to_string(),
                parsed_json,
                usage: json!({"input_tokens": 0, "output_tokens": 0}),
                latency_ms: 0,
                provider_model: "fake-llm-provider".to_string(),
                finish_reason: if request.repair_attempt > 0 {
                    "schema_repaired".to_string()
                } else {
                    "stop".to_string()
                },
            }),
            Some(Err(err)) => Err(err),
            None => Err(LlmProviderError::ProviderUnavailable),
        }
    }
}

pub fn complete_json_with_schema_repair<C, F>(
    client: &mut C,
    request: LlmProviderRequest,
    mut validate: F,
) -> Result<LlmProviderResponse, LlmProviderError>
where
    C: LlmProviderClient,
    F: FnMut(&Value) -> bool,
{
    let response = client.complete_json(request.clone())?;
    if validate(&response.parsed_json) {
        return Ok(response);
    }
    let mut repair_request = request;
    repair_request.repair_attempt = repair_request.repair_attempt.saturating_add(1);
    repair_request.input_json = json!({
        "schema_name": repair_request.schema_name,
        "schema_version": repair_request.schema_version,
        "schema_error_summary": "provider response did not satisfy target schema",
        "original_projection_digest": repair_request.projection_digest,
    });
    let repaired = client.complete_json(repair_request)?;
    if validate(&repaired.parsed_json) {
        Ok(repaired)
    } else {
        Err(LlmProviderError::SchemaRepairFailed)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn request() -> LlmProviderRequest {
        LlmProviderRequest {
            capability: "question_resolver".to_string(),
            mode: LlmMode::Shadow,
            schema_name: "tonglingyu-question-resolver".to_string(),
            schema_version: "tonglingyu-question-resolver-v1".to_string(),
            timeout_ms: 1500,
            input_json: json!({"question": "她后来怎么样？"}),
            projection_digest: "sha256:projection".to_string(),
            trace_ref: "trace://test".to_string(),
            replay_anchor: "fixture://test".to_string(),
            repair_attempt: 0,
        }
    }

    #[test]
    fn fake_provider_supports_schema_repair_once() {
        let mut provider = FakeLlmProvider::new(vec![
            Ok(json!({"bad": true})),
            Ok(json!({"schema_version": "ok"})),
        ]);

        let response = complete_json_with_schema_repair(&mut provider, request(), |value| {
            value.get("schema_version").and_then(Value::as_str) == Some("ok")
        })
        .expect("repair succeeds");

        assert_eq!(response.finish_reason, "schema_repaired");
    }

    #[test]
    fn fake_provider_reports_repair_failure() {
        let mut provider = FakeLlmProvider::new(vec![
            Ok(json!({"bad": true})),
            Ok(json!({"still_bad": true})),
        ]);

        let error = complete_json_with_schema_repair(&mut provider, request(), |value| {
            value.get("schema_version").and_then(Value::as_str) == Some("ok")
        })
        .expect_err("repair fails");

        assert_eq!(error, LlmProviderError::SchemaRepairFailed);
    }
}
