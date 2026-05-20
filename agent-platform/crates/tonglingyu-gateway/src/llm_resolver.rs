use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    llm_contracts::{
        LLM_RESOLVER_ALLOWED_CONTEXT_REFS, LLM_RESOLVER_ALLOWED_TRIGGERS,
        LLM_RESOLVER_FORBIDDEN_FIELDS, LLM_RESOLVER_FORBIDDEN_TRIGGERS,
        QUESTION_RESOLVER_SCHEMA_VERSION,
    },
    llm_modes::LlmMode,
    llm_provider::{LlmProviderClient, LlmProviderRequest, complete_json_with_schema_repair},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResolverLlmOutput {
    pub schema_version: String,
    pub resolved_question: String,
    #[serde(default)]
    pub referent_bindings: Vec<String>,
    #[serde(default)]
    pub used_context_refs: Vec<String>,
    pub confidence: f64,
    pub needs_clarification: bool,
    pub clarification_question: Option<String>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResolverContractDecision {
    Accept,
    Clarify,
    FailClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolverContractEvaluation {
    pub accepted: bool,
    pub decision: ResolverContractDecision,
    pub can_call_llm: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResolverRunReport {
    pub mode: LlmMode,
    pub trigger: String,
    pub provider_called: bool,
    pub accepted_for_main: bool,
    pub contract_accepted: bool,
    pub decision: ResolverContractDecision,
    pub error_type: Option<String>,
    pub replay_anchor: String,
}

pub fn llm_resolver_can_run_for_trigger(trigger: &str) -> bool {
    LLM_RESOLVER_ALLOWED_TRIGGERS.contains(&trigger)
}

pub fn known_llm_resolver_trigger(trigger: &str) -> bool {
    LLM_RESOLVER_ALLOWED_TRIGGERS.contains(&trigger)
        || LLM_RESOLVER_FORBIDDEN_TRIGGERS.contains(&trigger)
}

pub fn evaluate_resolver_with_provider<C: LlmProviderClient>(
    mode: LlmMode,
    trigger: &str,
    input_json: Value,
    replay_anchor: &str,
    provider: &mut C,
) -> LlmResolverRunReport {
    if mode == LlmMode::Disabled {
        return LlmResolverRunReport {
            mode,
            trigger: trigger.to_string(),
            provider_called: false,
            accepted_for_main: false,
            contract_accepted: false,
            decision: ResolverContractDecision::FailClosed,
            error_type: None,
            replay_anchor: replay_anchor.to_string(),
        };
    }
    if !llm_resolver_can_run_for_trigger(trigger) {
        return LlmResolverRunReport {
            mode,
            trigger: trigger.to_string(),
            provider_called: false,
            accepted_for_main: false,
            contract_accepted: false,
            decision: ResolverContractDecision::FailClosed,
            error_type: Some("trigger_not_allowed".to_string()),
            replay_anchor: replay_anchor.to_string(),
        };
    }

    let request = LlmProviderRequest {
        capability: "question_resolver".to_string(),
        mode,
        schema_name: "tonglingyu-question-resolver".to_string(),
        schema_version: QUESTION_RESOLVER_SCHEMA_VERSION.to_string(),
        timeout_ms: 1500,
        input_json: input_json.clone(),
        projection_digest: digest_value(&input_json),
        trace_ref: "trace://llm-resolver".to_string(),
        replay_anchor: replay_anchor.to_string(),
        repair_attempt: 0,
    };
    match complete_json_with_schema_repair(provider, request, |value| {
        evaluate_resolver_contract(trigger, value).errors.is_empty()
    }) {
        Ok(response) => {
            let evaluation = evaluate_resolver_contract(trigger, &response.parsed_json);
            let accepted_for_main = mode == LlmMode::Enforced
                && evaluation.decision == ResolverContractDecision::Accept;
            LlmResolverRunReport {
                mode,
                trigger: trigger.to_string(),
                provider_called: true,
                accepted_for_main,
                contract_accepted: evaluation.accepted,
                decision: evaluation.decision,
                error_type: None,
                replay_anchor: replay_anchor.to_string(),
            }
        }
        Err(error) => LlmResolverRunReport {
            mode,
            trigger: trigger.to_string(),
            provider_called: true,
            accepted_for_main: false,
            contract_accepted: false,
            decision: ResolverContractDecision::FailClosed,
            error_type: Some(error.as_str().to_string()),
            replay_anchor: replay_anchor.to_string(),
        },
    }
}

pub fn evaluate_resolver_contract(trigger: &str, output: &Value) -> ResolverContractEvaluation {
    let can_call_llm = llm_resolver_can_run_for_trigger(trigger);
    let mut errors = Vec::new();

    if !known_llm_resolver_trigger(trigger) {
        errors.push(format!("unknown trigger: {trigger}"));
    }
    if !can_call_llm {
        errors.push(format!("llm resolver not allowed for trigger: {trigger}"));
    }
    reject_forbidden_fields("$", output, &mut errors);

    let parsed = serde_json::from_value::<QuestionResolverLlmOutput>(output.clone());
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            errors.push(format!("schema_parse_failed: {err}"));
            return ResolverContractEvaluation {
                accepted: false,
                decision: ResolverContractDecision::FailClosed,
                can_call_llm,
                errors,
            };
        }
    };

    if parsed.schema_version != QUESTION_RESOLVER_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version_mismatch: {}",
            parsed.schema_version
        ));
    }
    if parsed.resolved_question.trim().is_empty() {
        errors.push("resolved_question_empty".to_string());
    }
    if !(0.0..=1.0).contains(&parsed.confidence) {
        errors.push("confidence_out_of_range".to_string());
    }
    for context_ref in &parsed.used_context_refs {
        if !LLM_RESOLVER_ALLOWED_CONTEXT_REFS.contains(&context_ref.as_str()) {
            errors.push(format!("unknown_context_ref: {context_ref}"));
        }
    }
    if parsed.needs_clarification
        && parsed
            .clarification_question
            .as_deref()
            .is_none_or(str::is_empty)
    {
        errors.push("clarification_question_required".to_string());
    }

    let decision = if !errors.is_empty() {
        ResolverContractDecision::FailClosed
    } else if parsed.confidence >= 0.75 && !parsed.needs_clarification {
        ResolverContractDecision::Accept
    } else if parsed.confidence >= 0.45 {
        ResolverContractDecision::Clarify
    } else {
        ResolverContractDecision::FailClosed
    };

    ResolverContractEvaluation {
        accepted: decision == ResolverContractDecision::Accept,
        decision,
        can_call_llm,
        errors,
    }
}

fn digest_value(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(value).unwrap_or_default());
    format!("sha256:{:x}", hasher.finalize())
}

fn reject_forbidden_fields(path: &str, value: &Value, errors: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let child_path = format!("{path}.{key}");
                if LLM_RESOLVER_FORBIDDEN_FIELDS
                    .iter()
                    .any(|forbidden| key == forbidden)
                {
                    errors.push(format!("forbidden_field: {child_path}"));
                }
                reject_forbidden_fields(&child_path, item, errors);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_forbidden_fields(&format!("{path}[{index}]"), item, errors);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::llm_provider::{FakeLlmProvider, LlmProviderError};
    use serde_json::json;

    use super::*;

    fn valid_output(confidence: f64) -> Value {
        json!({
            "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
            "resolved_question": "晴雯后来怎么样？",
            "referent_bindings": ["晴雯"],
            "used_context_refs": ["current_question", "session_summary"],
            "confidence": confidence,
            "needs_clarification": false,
            "clarification_question": null,
            "unsupported_reason": null
        })
    }

    #[test]
    fn resolver_contract_accepts_valid_allowed_trigger() {
        let evaluation = evaluate_resolver_contract("unresolved_referent", &valid_output(0.91));

        assert!(evaluation.accepted);
        assert_eq!(evaluation.decision, ResolverContractDecision::Accept);
        assert!(evaluation.errors.is_empty());
    }

    #[test]
    fn resolver_contract_fails_closed_for_unknown_context_ref() {
        let mut output = valid_output(0.91);
        output["used_context_refs"] = json!(["raw_memory"]);
        let evaluation = evaluate_resolver_contract("unresolved_referent", &output);

        assert!(!evaluation.accepted);
        assert_eq!(evaluation.decision, ResolverContractDecision::FailClosed);
        assert!(
            evaluation
                .errors
                .iter()
                .any(|error| error.contains("unknown_context_ref"))
        );
    }

    #[test]
    fn resolver_contract_rejects_forbidden_trigger_even_with_valid_output() {
        let evaluation =
            evaluate_resolver_contract("prompt_injection_detected", &valid_output(0.91));

        assert!(!evaluation.accepted);
        assert!(!evaluation.can_call_llm);
        assert_eq!(evaluation.decision, ResolverContractDecision::FailClosed);
    }

    #[test]
    fn resolver_provider_shadow_does_not_accept_for_main() {
        let mut provider = FakeLlmProvider::new(vec![Ok(valid_output(0.91))]);

        let report = evaluate_resolver_with_provider(
            LlmMode::Shadow,
            "unresolved_referent",
            json!({"question": "她后来怎么样？"}),
            "fixture://resolver-shadow",
            &mut provider,
        );

        assert!(report.provider_called);
        assert!(report.contract_accepted);
        assert!(!report.accepted_for_main);
    }

    #[test]
    fn resolver_provider_enforced_accepts_valid_contract_for_main() {
        let mut provider = FakeLlmProvider::new(vec![Ok(valid_output(0.91))]);

        let report = evaluate_resolver_with_provider(
            LlmMode::Enforced,
            "unresolved_referent",
            json!({"question": "她后来怎么样？"}),
            "fixture://resolver-enforced",
            &mut provider,
        );

        assert!(report.accepted_for_main);
    }

    #[test]
    fn resolver_provider_failure_fails_closed() {
        let mut provider = FakeLlmProvider::new(vec![Err(LlmProviderError::Timeout)]);

        let report = evaluate_resolver_with_provider(
            LlmMode::Enforced,
            "unresolved_referent",
            json!({"question": "她后来怎么样？"}),
            "fixture://resolver-timeout",
            &mut provider,
        );

        assert!(!report.accepted_for_main);
        assert_eq!(report.error_type.as_deref(), Some("timeout"));
    }
}
