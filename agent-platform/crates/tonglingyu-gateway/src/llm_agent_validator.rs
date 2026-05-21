use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    conversation_state::{
        ConversationStateSummary, ConversationStateValidationContext,
        validate_conversation_state_summary,
    },
    llm_agent_contracts::{
        CONVERSATION_STATE_WRITER_PROFILE_ID, LLM_AGENT_VALIDATOR_SCHEMA_VERSION,
        LlmAgentRequestEnvelope, QUESTION_NORMALIZER_PROFILE_ID, QuestionNormalizerAgentOutput,
    },
    llm_contracts::{
        CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION, LLM_RESOLVER_ALLOWED_CONTEXT_REFS,
        LLM_RESOLVER_ALLOWED_TRIGGERS, LLM_RESOLVER_FORBIDDEN_FIELDS,
        QUESTION_RESOLVER_SCHEMA_VERSION,
    },
    llm_modes::LlmMode,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentValidationDecision {
    Accepted,
    Clarify,
    Rejected,
}

#[derive(Debug, Clone)]
pub(crate) struct SealedQuestionResolution {
    output: QuestionNormalizerAgentOutput,
}

impl SealedQuestionResolution {
    pub(crate) fn resolved_question(&self) -> &str {
        &self.output.resolved_question
    }

    pub(crate) fn referent_bindings(&self) -> &[String] {
        &self.output.referent_bindings
    }

    pub(crate) fn used_context_refs(&self) -> &[String] {
        &self.output.used_context_refs
    }

    pub(crate) fn confidence(&self) -> f64 {
        self.output.confidence
    }

    pub(crate) fn needs_clarification(&self) -> bool {
        self.output.needs_clarification
    }

    pub(crate) fn clarification_question(&self) -> Option<&str> {
        self.output.clarification_question.as_deref()
    }

    pub(crate) fn unsupported_reason(&self) -> Option<&str> {
        self.output.unsupported_reason.as_deref()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct QuestionNormalizerValidationDecision {
    accepted: Option<SealedQuestionResolution>,
    audit: Value,
    errors: Vec<String>,
}

impl QuestionNormalizerValidationDecision {
    pub(crate) fn accepted_resolution(&self) -> Option<&SealedQuestionResolution> {
        self.accepted.as_ref()
    }

    pub(crate) fn contract_accepted(&self) -> bool {
        self.audit
            .get("contract_accepted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub(crate) fn errors(&self) -> &[String] {
        &self.errors
    }

    pub(crate) fn audit_json(&self) -> Value {
        self.audit.clone()
    }

    pub(crate) fn with_repair_metadata(
        mut self,
        repair_attempted: bool,
        first_error_digest: Option<String>,
    ) -> Self {
        if let Some(object) = self.audit.as_object_mut() {
            object.insert(
                "schema_repair_attempted".to_string(),
                json!(repair_attempted),
            );
            object.insert(
                "pre_repair_error_digest".to_string(),
                first_error_digest.map(Value::String).unwrap_or(Value::Null),
            );
        }
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SealedConversationStateSummary {
    summary: ConversationStateSummary,
}

impl SealedConversationStateSummary {
    pub(crate) fn summary(&self) -> &ConversationStateSummary {
        &self.summary
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConversationStateValidationDecision {
    accepted: Option<SealedConversationStateSummary>,
    audit: Value,
    errors: Vec<String>,
}

impl ConversationStateValidationDecision {
    pub(crate) fn accepted_summary(&self) -> Option<&SealedConversationStateSummary> {
        self.accepted.as_ref()
    }

    pub(crate) fn contract_accepted(&self) -> bool {
        self.audit
            .get("contract_accepted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub(crate) fn errors(&self) -> &[String] {
        &self.errors
    }

    pub(crate) fn audit_json(&self) -> Value {
        self.audit.clone()
    }

    pub(crate) fn with_repair_metadata(
        mut self,
        repair_attempted: bool,
        first_error_digest: Option<String>,
    ) -> Self {
        if let Some(object) = self.audit.as_object_mut() {
            object.insert(
                "schema_repair_attempted".to_string(),
                json!(repair_attempted),
            );
            object.insert(
                "pre_repair_error_digest".to_string(),
                first_error_digest.map(Value::String).unwrap_or(Value::Null),
            );
        }
        self
    }
}

pub(crate) fn validate_question_normalizer_runtime_output(
    mode: LlmMode,
    trigger: &str,
    envelope: &LlmAgentRequestEnvelope,
    raw_output: &str,
    output_ref: Option<&str>,
    allowed_referents: &[String],
) -> QuestionNormalizerValidationDecision {
    let raw_output_sha256 = hash_text(raw_output);
    let parsed = parse_agent_json(raw_output);
    let (value, local_json_extraction_applied) = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            return rejected_question_decision(
                mode,
                trigger,
                envelope,
                output_ref,
                raw_output_sha256,
                vec![error],
                false,
                false,
            );
        }
    };

    let mut errors = Vec::new();
    let mut schema_repaired_locally = local_json_extraction_applied;
    reject_forbidden_fields("$", &value, &mut errors);
    if !LLM_RESOLVER_ALLOWED_TRIGGERS.contains(&trigger) {
        errors.push(format!("trigger_not_allowed: {trigger}"));
    }
    let parsed_output = match serde_json::from_value::<QuestionNormalizerAgentOutput>(value.clone())
    {
        Ok(output) => output,
        Err(error) => {
            errors.push(format!("schema_deserialize_failed: {error}"));
            return rejected_question_decision(
                mode,
                trigger,
                envelope,
                output_ref,
                raw_output_sha256,
                errors,
                false,
                schema_repaired_locally,
            );
        }
    };
    if parsed_output.schema_version != QUESTION_RESOLVER_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version_mismatch: {}",
            parsed_output.schema_version
        ));
    }
    if parsed_output.resolved_question.trim().is_empty() {
        errors.push("resolved_question_empty".to_string());
    }
    if !(0.0..=1.0).contains(&parsed_output.confidence) {
        errors.push("confidence_out_of_range".to_string());
    }
    if parsed_output.referent_bindings.len() > 4 {
        errors.push("referent_bindings_too_many_items".to_string());
    }
    let allowed_referents = allowed_referents
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for binding in &parsed_output.referent_bindings {
        if binding.trim().is_empty() {
            errors.push("referent_binding_empty".to_string());
        }
        if binding.chars().count() > 80 {
            errors.push("referent_binding_too_long".to_string());
        }
        if !allowed_referents.is_empty() && !allowed_referents.contains(binding.as_str()) {
            errors.push(format!(
                "referent_not_in_projection: {}",
                hash_text(binding)
            ));
        }
    }
    for context_ref in &parsed_output.used_context_refs {
        if !LLM_RESOLVER_ALLOWED_CONTEXT_REFS.contains(&context_ref.as_str()) {
            errors.push(format!("unknown_context_ref: {context_ref}"));
        }
        if context_ref == "conversation_state_summary" {
            errors.push("conversation_state_summary_used_as_resolver_context".to_string());
        }
    }
    if parsed_output.needs_clarification
        && parsed_output
            .clarification_question
            .as_deref()
            .is_none_or(str::is_empty)
    {
        errors.push("clarification_question_required".to_string());
    }

    let contract_accepted = errors.is_empty();
    let decision = if !contract_accepted {
        AgentValidationDecision::Rejected
    } else if parsed_output.confidence >= 0.75 && !parsed_output.needs_clarification {
        AgentValidationDecision::Accepted
    } else if parsed_output.confidence >= 0.45 {
        AgentValidationDecision::Clarify
    } else {
        AgentValidationDecision::Rejected
    };
    let accepted_for_main = mode == LlmMode::Enforced
        && matches!(
            decision,
            AgentValidationDecision::Accepted | AgentValidationDecision::Clarify
        );
    let accepted = accepted_for_main.then_some(SealedQuestionResolution {
        output: parsed_output,
    });
    if !contract_accepted {
        schema_repaired_locally = false;
    }
    QuestionNormalizerValidationDecision {
        audit: question_audit(
            mode,
            trigger,
            envelope,
            output_ref,
            raw_output_sha256,
            &errors,
            contract_accepted,
            accepted_for_main,
            &decision,
            schema_repaired_locally,
        ),
        accepted,
        errors,
    }
}

pub(crate) fn question_normalizer_runtime_error_decision(
    mode: LlmMode,
    trigger: &str,
    envelope: &LlmAgentRequestEnvelope,
    error: impl Into<String>,
) -> QuestionNormalizerValidationDecision {
    rejected_question_decision(
        mode,
        trigger,
        envelope,
        None,
        hash_text("runtime_error_without_output"),
        vec![format!("runtime_error: {}", error.into())],
        false,
        false,
    )
}

pub(crate) fn validate_conversation_state_runtime_output(
    mode: LlmMode,
    envelope: &LlmAgentRequestEnvelope,
    raw_output: &str,
    output_ref: Option<&str>,
    validation_context: &ConversationStateValidationContext<'_>,
) -> ConversationStateValidationDecision {
    let raw_output_sha256 = hash_text(raw_output);
    let parsed = parse_agent_json(raw_output);
    let (value, local_json_extraction_applied) = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            return rejected_conversation_state_decision(
                mode,
                envelope,
                output_ref,
                raw_output_sha256,
                vec![error],
                false,
                false,
            );
        }
    };
    let mut errors = Vec::new();
    reject_forbidden_fields("$", &value, &mut errors);
    reject_unknown_conversation_state_fields(&value, &mut errors);
    let summary = match serde_json::from_value::<ConversationStateSummary>(value.clone()) {
        Ok(summary) => summary,
        Err(error) => {
            errors.push(format!("schema_deserialize_failed: {error}"));
            return rejected_conversation_state_decision(
                mode,
                envelope,
                output_ref,
                raw_output_sha256,
                errors,
                false,
                local_json_extraction_applied,
            );
        }
    };
    if summary.schema_version != CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version_mismatch: {}",
            summary.schema_version
        ));
    }
    let validation = validate_conversation_state_summary(&summary, validation_context);
    errors.extend(validation.errors);

    let contract_accepted = errors.is_empty();
    let decision = if contract_accepted {
        AgentValidationDecision::Accepted
    } else {
        AgentValidationDecision::Rejected
    };
    let accepted_for_projection =
        mode == LlmMode::Enforced && decision == AgentValidationDecision::Accepted;
    let accepted = accepted_for_projection.then_some(SealedConversationStateSummary { summary });
    ConversationStateValidationDecision {
        audit: conversation_state_audit(
            mode,
            envelope,
            output_ref,
            raw_output_sha256,
            &errors,
            contract_accepted,
            accepted_for_projection,
            &decision,
            local_json_extraction_applied,
        ),
        accepted,
        errors,
    }
}

pub(crate) fn conversation_state_runtime_error_decision(
    mode: LlmMode,
    envelope: &LlmAgentRequestEnvelope,
    error: impl Into<String>,
) -> ConversationStateValidationDecision {
    rejected_conversation_state_decision(
        mode,
        envelope,
        None,
        hash_text("runtime_error_without_output"),
        vec![format!("runtime_error: {}", error.into())],
        false,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn rejected_question_decision(
    mode: LlmMode,
    trigger: &str,
    envelope: &LlmAgentRequestEnvelope,
    output_ref: Option<&str>,
    raw_output_sha256: String,
    errors: Vec<String>,
    contract_accepted: bool,
    schema_repaired_locally: bool,
) -> QuestionNormalizerValidationDecision {
    let decision = AgentValidationDecision::Rejected;
    QuestionNormalizerValidationDecision {
        audit: question_audit(
            mode,
            trigger,
            envelope,
            output_ref,
            raw_output_sha256,
            &errors,
            contract_accepted,
            false,
            &decision,
            schema_repaired_locally,
        ),
        accepted: None,
        errors,
    }
}

fn rejected_conversation_state_decision(
    mode: LlmMode,
    envelope: &LlmAgentRequestEnvelope,
    output_ref: Option<&str>,
    raw_output_sha256: String,
    errors: Vec<String>,
    contract_accepted: bool,
    schema_repaired_locally: bool,
) -> ConversationStateValidationDecision {
    let decision = AgentValidationDecision::Rejected;
    ConversationStateValidationDecision {
        audit: conversation_state_audit(
            mode,
            envelope,
            output_ref,
            raw_output_sha256,
            &errors,
            contract_accepted,
            false,
            &decision,
            schema_repaired_locally,
        ),
        accepted: None,
        errors,
    }
}

#[allow(clippy::too_many_arguments)]
fn question_audit(
    mode: LlmMode,
    trigger: &str,
    envelope: &LlmAgentRequestEnvelope,
    output_ref: Option<&str>,
    raw_output_sha256: String,
    errors: &[String],
    contract_accepted: bool,
    accepted_for_main: bool,
    decision: &AgentValidationDecision,
    local_json_extraction_applied: bool,
) -> Value {
    json!({
        "schema_version": LLM_AGENT_VALIDATOR_SCHEMA_VERSION,
        "validator": LLM_AGENT_VALIDATOR_SCHEMA_VERSION,
        "profile_id": QUESTION_NORMALIZER_PROFILE_ID,
        "agent_request_id": &envelope.agent_request_id,
        "agent_request_schema_version": &envelope.schema_version,
        "mode": mode.as_str(),
        "trigger": trigger,
        "decision": decision,
        "provider_called": true,
        "contract_accepted": contract_accepted,
        "accepted_for_main": accepted_for_main,
        "input_digest": &envelope.input_digest,
        "projection_ref": &envelope.projection_ref,
        "raw_output_sha256": raw_output_sha256,
        "raw_output_embedded": false,
        "output_ref": output_ref,
        "runtime_adapter": runtime_adapter_from_output_ref(output_ref),
        "errors": errors,
        "schema_repair_attempted": false,
        "local_json_extraction_applied": local_json_extraction_applied,
    })
}

#[allow(clippy::too_many_arguments)]
fn conversation_state_audit(
    mode: LlmMode,
    envelope: &LlmAgentRequestEnvelope,
    output_ref: Option<&str>,
    raw_output_sha256: String,
    errors: &[String],
    contract_accepted: bool,
    accepted_for_projection: bool,
    decision: &AgentValidationDecision,
    local_json_extraction_applied: bool,
) -> Value {
    json!({
        "schema_version": LLM_AGENT_VALIDATOR_SCHEMA_VERSION,
        "validator": LLM_AGENT_VALIDATOR_SCHEMA_VERSION,
        "profile_id": CONVERSATION_STATE_WRITER_PROFILE_ID,
        "agent_request_id": &envelope.agent_request_id,
        "agent_request_schema_version": &envelope.schema_version,
        "mode": mode.as_str(),
        "decision": decision,
        "provider_called": true,
        "contract_accepted": contract_accepted,
        "accepted_for_projection": accepted_for_projection,
        "input_digest": &envelope.input_digest,
        "projection_ref": &envelope.projection_ref,
        "raw_output_sha256": raw_output_sha256,
        "raw_output_embedded": false,
        "output_ref": output_ref,
        "runtime_adapter": runtime_adapter_from_output_ref(output_ref),
        "errors": errors,
        "schema_repair_attempted": false,
        "local_json_extraction_applied": local_json_extraction_applied,
    })
}

fn runtime_adapter_from_output_ref(output_ref: Option<&str>) -> &'static str {
    match output_ref.unwrap_or_default() {
        value if value.starts_with("openai-compatible-network://") => "openai-compatible-network",
        value if value.starts_with("hermes://") => "hermes",
        value if value.starts_with("result://") => "minimal",
        _ => "unknown",
    }
}

fn parse_agent_json(raw_output: &str) -> Result<(Value, bool), String> {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return Err("schema_parse_failed: empty_output".to_string());
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if value.is_object() {
            return Ok((value, false));
        }
        return Err("schema_parse_failed: output_not_object".to_string());
    }
    let unfenced = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim);
    if let Some(unfenced) = unfenced
        && let Ok(value) = serde_json::from_str::<Value>(unfenced)
        && value.is_object()
    {
        return Ok((value, true));
    }
    let Some(start) = trimmed.find('{') else {
        return Err("schema_parse_failed: object_start_not_found".to_string());
    };
    let Some(end) = trimmed.rfind('}') else {
        return Err("schema_parse_failed: object_end_not_found".to_string());
    };
    if end <= start {
        return Err("schema_parse_failed: object_bounds_invalid".to_string());
    }
    let candidate = &trimmed[start..=end];
    serde_json::from_str::<Value>(candidate)
        .map_err(|error| format!("schema_parse_failed: {error}"))
        .and_then(|value| {
            if value.is_object() {
                Ok((value, true))
            } else {
                Err("schema_parse_failed: output_not_object".to_string())
            }
        })
}

fn reject_forbidden_fields(path: &str, value: &Value, errors: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let child_path = format!("{path}.{key}");
                if LLM_RESOLVER_FORBIDDEN_FIELDS
                    .iter()
                    .any(|forbidden| key == forbidden)
                    || key == "conversation_state_summary"
                    || key == "raw_prompt"
                    || key == "raw_response"
                    || key == "raw_llm_payload"
                    || key == "tool_payload"
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

fn reject_unknown_conversation_state_fields(value: &Value, errors: &mut Vec<String>) {
    let Some(map) = value.as_object() else {
        errors.push("conversation_state_output_not_object".to_string());
        return;
    };
    let allowed = [
        "object",
        "schema_version",
        "current_topic",
        "active_entities",
        "open_questions",
        "last_answer_boundaries",
        "evidence_package_refs",
        "reviewer_warnings",
        "memory_allowed_as_evidence",
        "summary_confidence",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    for key in map.keys() {
        if !allowed.contains(key.as_str()) {
            errors.push(format!("unexpected_conversation_state_field: {key}"));
        }
    }
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub(crate) fn error_digest(errors: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(errors).unwrap_or_default());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        conversation_state::{
            CONVERSATION_STATE_SUMMARY_OBJECT, ConversationStateInput,
            conversation_state_validation_context,
        },
        llm_agent_contracts::{
            CONVERSATION_STATE_WRITER_AGENT_TYPE, LLM_AGENT_REQUEST_SCHEMA_VERSION,
        },
    };

    fn envelope(profile_id: &str) -> LlmAgentRequestEnvelope {
        LlmAgentRequestEnvelope {
            schema_version: LLM_AGENT_REQUEST_SCHEMA_VERSION.to_string(),
            agent_request_id: "req-test".to_string(),
            request_type: "create_run".to_string(),
            agent_type: CONVERSATION_STATE_WRITER_AGENT_TYPE.to_string(),
            requested_by_service: "test".to_string(),
            requested_by_user: "test".to_string(),
            status: "parsed".to_string(),
            profile_id: profile_id.to_string(),
            mode: "enforced".to_string(),
            trace_id: "trace-test".to_string(),
            user_session_id: "user-session-test".to_string(),
            interaction_context_id: "interaction-context-test".to_string(),
            projection_ref: "llm-agent-input://test".to_string(),
            input_digest: "sha256:test".to_string(),
            timeout_ms: 1500,
            requested_tools: Vec::new(),
            structured_payload: json!({}),
        }
    }

    #[test]
    fn question_validator_accepts_only_sealed_valid_output() {
        let output = json!({
            "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
            "resolved_question": "晴雯后来怎么样？",
            "referent_bindings": ["晴雯"],
            "used_context_refs": ["current_question", "session_summary"],
            "confidence": 0.92,
            "needs_clarification": false,
            "clarification_question": null,
            "unsupported_reason": null
        });
        let decision = validate_question_normalizer_runtime_output(
            LlmMode::Enforced,
            "prior_subject_needed",
            &envelope(QUESTION_NORMALIZER_PROFILE_ID),
            &output.to_string(),
            Some("hermes://profiles/test"),
            &["晴雯".to_string()],
        );

        assert!(decision.contract_accepted());
        assert_eq!(decision.audit_json()["accepted_for_main"], json!(true));
        assert_eq!(
            decision
                .accepted_resolution()
                .expect("sealed output")
                .resolved_question(),
            "晴雯后来怎么样？"
        );
        assert_eq!(decision.audit_json()["raw_output_embedded"], json!(false));
    }

    #[test]
    fn question_validator_rejects_control_fields() {
        let output = json!({
            "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
            "resolved_question": "晴雯后来怎么样？",
            "referent_bindings": ["晴雯"],
            "used_context_refs": ["current_question"],
            "confidence": 0.92,
            "needs_clarification": false,
            "clarification_question": null,
            "unsupported_reason": null,
            "allowed_tools": ["tonglingyu.memory.write"]
        });
        let decision = validate_question_normalizer_runtime_output(
            LlmMode::Enforced,
            "prior_subject_needed",
            &envelope(QUESTION_NORMALIZER_PROFILE_ID),
            &output.to_string(),
            None,
            &["晴雯".to_string()],
        );

        assert!(!decision.contract_accepted());
        assert!(decision.accepted_resolution().is_none());
        assert!(
            decision
                .errors()
                .iter()
                .any(|error| error.contains("forbidden_field"))
        );
    }

    #[test]
    fn conversation_state_validator_rejects_memory_as_evidence() {
        let messages = [];
        let evidence_refs = [];
        let input = ConversationStateInput {
            current_question: "晴雯后来怎么样？",
            recent_messages: &messages,
            session_summary: "最近讨论对象：晴雯",
            last_public_answer_boundary: None,
            evidence_package_refs: &evidence_refs,
            reviewer_warnings: &[],
        };
        let validation_context = conversation_state_validation_context(&input, &["晴雯"], &[]);
        let output = json!({
            "object": CONVERSATION_STATE_SUMMARY_OBJECT,
            "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
            "current_topic": "晴雯相关问题",
            "active_entities": ["晴雯"],
            "open_questions": ["晴雯后来怎么样？"],
            "last_answer_boundaries": [],
            "evidence_package_refs": [],
            "reviewer_warnings": [],
            "memory_allowed_as_evidence": true,
            "summary_confidence": 0.9
        });
        let decision = validate_conversation_state_runtime_output(
            LlmMode::Enforced,
            &envelope(CONVERSATION_STATE_WRITER_PROFILE_ID),
            &output.to_string(),
            None,
            &validation_context,
        );

        assert!(!decision.contract_accepted());
        assert!(decision.accepted_summary().is_none());
        assert!(
            decision
                .errors()
                .iter()
                .any(|error| error.contains("memory_allowed_as_evidence_true"))
        );
    }
}
