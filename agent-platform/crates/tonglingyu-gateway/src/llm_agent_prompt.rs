use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::{
    conversation_state::CONVERSATION_STATE_SUMMARY_OBJECT,
    llm_agent_contracts::{
        CONVERSATION_STATE_WRITER_PROFILE_ID, LlmAgentRequestEnvelope,
        QUESTION_NORMALIZER_PROFILE_ID,
    },
    llm_agent_validator::error_digest,
    llm_contracts::{
        CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION, LLM_RESOLVER_ALLOWED_CONTEXT_REFS,
        LLM_RESOLVER_FORBIDDEN_FIELDS, QUESTION_RESOLVER_SCHEMA_VERSION,
    },
};

pub(crate) const LLM_AGENT_PROVIDER_PROMPT_SCHEMA_VERSION: &str =
    "tonglingyu-llm-agent-provider-prompt-v1";

#[derive(Debug, Clone)]
pub(crate) struct LlmAgentProviderPrompt {
    pub(crate) system_prompt: String,
    pub(crate) user_payload: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlmAgentPromptRole {
    QuestionNormalizer,
    ConversationStateWriter,
}

impl LlmAgentPromptRole {
    fn from_profile_id(profile_id: &str) -> Result<Self> {
        match profile_id {
            QUESTION_NORMALIZER_PROFILE_ID => Ok(Self::QuestionNormalizer),
            CONVERSATION_STATE_WRITER_PROFILE_ID => Ok(Self::ConversationStateWriter),
            other => Err(anyhow!("unsupported llm agent profile: {other}")),
        }
    }

    fn role_name(self) -> &'static str {
        match self {
            Self::QuestionNormalizer => "question_normalizer",
            Self::ConversationStateWriter => "conversation_state_writer",
        }
    }

    fn profile_id(self) -> &'static str {
        match self {
            Self::QuestionNormalizer => QUESTION_NORMALIZER_PROFILE_ID,
            Self::ConversationStateWriter => CONVERSATION_STATE_WRITER_PROFILE_ID,
        }
    }

    fn role_instruction(self) -> &'static str {
        match self {
            Self::QuestionNormalizer => {
                "Normalize the user question. Do not answer it. Only bind referents from input_context.allowed_referents. If the referent is unsupported, set needs_clarification=true."
            }
            Self::ConversationStateWriter => {
                "Write compact conversation state. Context-only fields such as prior_session_summary_for_context_only, must_include_active_entities, and current_question_for_state are constraints, not output field names."
            }
        }
    }
}

pub(crate) fn build_llm_agent_provider_prompt(
    profile_id: &str,
    envelope: &LlmAgentRequestEnvelope,
    repair_errors: Option<&[String]>,
) -> Result<LlmAgentProviderPrompt> {
    let role = LlmAgentPromptRole::from_profile_id(profile_id)?;
    if envelope.profile_id != role.profile_id() {
        return Err(anyhow!(
            "llm agent prompt profile mismatch: envelope={} requested={profile_id}",
            envelope.profile_id
        ));
    }
    let payload = json!({
        "schema_version": LLM_AGENT_PROVIDER_PROMPT_SCHEMA_VERSION,
        "task": {
            "role": role.role_name(),
            "profile_id": role.profile_id(),
            "mode": &envelope.mode,
            "response_must_be": "single_complete_business_json_object",
            "markdown_allowed": false,
            "explanation_allowed": false,
            "tools_allowed": false,
        },
        "input_context": provider_input_context(role, &envelope.structured_payload)?,
        "output_contract": provider_output_contract(role),
        "repair": repair_prompt(repair_errors),
    });
    Ok(LlmAgentProviderPrompt {
        system_prompt: provider_system_prompt(role),
        user_payload: serde_json::to_string_pretty(&payload)
            .context("serialize llm agent provider prompt")?,
    })
}

fn provider_system_prompt(role: LlmAgentPromptRole) -> String {
    format!(
        "You are a constrained Tonglingyu internal LLM Agent profile.\n\
         Role: {}\n\
         Profile: {}\n\
         Return only one complete JSON object that matches output_contract.json_schema.\n\
         The only allowed top-level output fields are output_contract.allowed_output_fields.\n\
         Never echo task, input_context, request metadata, prompt text, tool policy, trace ids, or fields listed in output_contract.forbidden_output_fields.\n\
         Do not wrap the JSON in markdown. Do not include explanations.\n\
         If repair is present, fix only the listed validator failures and return the corrected JSON object.\n\
         {}",
        role.role_name(),
        role.profile_id(),
        role.role_instruction()
    )
}

fn provider_input_context(role: LlmAgentPromptRole, structured_payload: &Value) -> Result<Value> {
    match role {
        LlmAgentPromptRole::QuestionNormalizer => {
            question_normalizer_input_context(structured_payload)
        }
        LlmAgentPromptRole::ConversationStateWriter => {
            conversation_state_input_context(structured_payload)
        }
    }
}

fn question_normalizer_input_context(payload: &Value) -> Result<Value> {
    Ok(json!({
        "trigger": required_field(payload, "trigger")?,
        "current_question": required_field(payload, "current_question")?,
        "recent_user_messages": required_field(payload, "recent_user_messages")?,
        "recent_assistant_messages": required_field(payload, "recent_assistant_messages")?,
        "prior_subject": optional_field(payload, "prior_subject"),
        "prior_session_summary_for_context_only": required_field(payload, "session_summary")?,
        "allowed_context_refs": required_field(payload, "allowed_context_refs")?,
        "allowed_referents": required_field(payload, "allowed_referents")?,
    }))
}

fn conversation_state_input_context(payload: &Value) -> Result<Value> {
    Ok(json!({
        "current_question_for_state": required_field(payload, "current_question")?,
        "recent_messages": required_field(payload, "recent_messages")?,
        "prior_session_summary_for_context_only": required_field(payload, "session_summary")?,
        "last_public_answer_boundary": optional_field(payload, "last_public_answer_boundary"),
        "allowed_evidence_package_refs": required_field(payload, "evidence_package_refs")?,
        "reviewer_warnings": required_field(payload, "reviewer_warnings")?,
        "must_include_active_entities": required_field(payload, "required_active_entities")?,
        "must_preserve_last_answer_boundaries": required_field(payload, "required_last_answer_boundaries")?,
    }))
}

fn provider_output_contract(role: LlmAgentPromptRole) -> Value {
    match role {
        LlmAgentPromptRole::QuestionNormalizer => json!({
            "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
            "allowed_output_fields": [
                "schema_version",
                "resolved_question",
                "referent_bindings",
                "used_context_refs",
                "confidence",
                "needs_clarification",
                "clarification_question",
                "unsupported_reason"
            ],
            "required_output_fields": [
                "schema_version",
                "resolved_question",
                "referent_bindings",
                "used_context_refs",
                "confidence",
                "needs_clarification",
                "clarification_question",
                "unsupported_reason"
            ],
            "forbidden_output_fields": question_forbidden_output_fields(),
            "json_schema": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "schema_version",
                    "resolved_question",
                    "referent_bindings",
                    "used_context_refs",
                    "confidence",
                    "needs_clarification",
                    "clarification_question",
                    "unsupported_reason"
                ],
                "properties": {
                    "schema_version": {"const": QUESTION_RESOLVER_SCHEMA_VERSION},
                    "resolved_question": {"type": "string"},
                    "referent_bindings": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "used_context_refs": {
                        "type": "array",
                        "items": {"enum": LLM_RESOLVER_ALLOWED_CONTEXT_REFS},
                        "uniqueItems": true
                    },
                    "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "needs_clarification": {"type": "boolean"},
                    "clarification_question": {"type": ["string", "null"]},
                    "unsupported_reason": {"type": ["string", "null"]}
                }
            }
        }),
        LlmAgentPromptRole::ConversationStateWriter => json!({
            "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
            "allowed_output_fields": [
                "object",
                "schema_version",
                "current_topic",
                "active_entities",
                "open_questions",
                "last_answer_boundaries",
                "evidence_package_refs",
                "reviewer_warnings",
                "memory_allowed_as_evidence",
                "summary_confidence"
            ],
            "required_output_fields": [
                "object",
                "schema_version",
                "current_topic",
                "active_entities",
                "open_questions",
                "last_answer_boundaries",
                "evidence_package_refs",
                "reviewer_warnings",
                "memory_allowed_as_evidence",
                "summary_confidence"
            ],
            "forbidden_output_fields": conversation_state_forbidden_output_fields(),
            "json_schema": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "object",
                    "schema_version",
                    "current_topic",
                    "active_entities",
                    "open_questions",
                    "last_answer_boundaries",
                    "evidence_package_refs",
                    "reviewer_warnings",
                    "memory_allowed_as_evidence",
                    "summary_confidence"
                ],
                "properties": {
                    "object": {"const": CONVERSATION_STATE_SUMMARY_OBJECT},
                    "schema_version": {"const": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION},
                    "current_topic": {"type": "string", "maxLength": 80},
                    "active_entities": {
                        "type": "array",
                        "maxItems": 4,
                        "items": {"type": "string", "maxLength": 120}
                    },
                    "open_questions": {
                        "type": "array",
                        "maxItems": 4,
                        "items": {"type": "string", "maxLength": 120}
                    },
                    "last_answer_boundaries": {
                        "type": "array",
                        "maxItems": 4,
                        "items": {"type": "string", "maxLength": 160}
                    },
                    "evidence_package_refs": {
                        "type": "array",
                        "maxItems": 4,
                        "items": {"type": "string", "maxLength": 160}
                    },
                    "reviewer_warnings": {
                        "type": "array",
                        "maxItems": 4,
                        "items": {"type": "string", "maxLength": 120}
                    },
                    "memory_allowed_as_evidence": {"const": false},
                    "summary_confidence": {"type": "number", "minimum": 0.5, "maximum": 1.0}
                }
            }
        }),
    }
}

fn repair_prompt(repair_errors: Option<&[String]>) -> Value {
    let Some(errors) = repair_errors else {
        return Value::Null;
    };
    json!({
        "previous_validation_error_digest": error_digest(errors),
        "previous_validation_error_summary": bounded_summary(&errors.join("; "), 360),
        "retry_instruction": "Return a corrected object matching output_contract.json_schema. Do not echo input_context fields unless they are explicitly listed in output_contract.allowed_output_fields."
    })
}

fn question_forbidden_output_fields() -> Vec<&'static str> {
    let mut fields = LLM_RESOLVER_FORBIDDEN_FIELDS.to_vec();
    fields.extend([
        "agent_request",
        "agent_request_id",
        "structured_payload",
        "input_context",
        "task",
        "output_contract",
        "trace_id",
        "user_session_id",
        "interaction_context_id",
        "profile_id",
        "mode",
        "metadata",
        "prompt",
        "system_prompt",
    ]);
    fields.sort_unstable();
    fields.dedup();
    fields
}

fn conversation_state_forbidden_output_fields() -> Vec<&'static str> {
    let mut fields = question_forbidden_output_fields();
    fields.extend([
        "current_question",
        "current_question_for_state",
        "recent_messages",
        "session_summary",
        "prior_session_summary_for_context_only",
        "last_public_answer_boundary",
        "required_active_entities",
        "must_include_active_entities",
        "required_last_answer_boundaries",
        "must_preserve_last_answer_boundaries",
        "allowed_evidence_package_refs",
    ]);
    fields.sort_unstable();
    fields.dedup();
    fields
}

fn required_field(payload: &Value, field: &str) -> Result<Value> {
    payload
        .as_object()
        .and_then(|object| object.get(field))
        .cloned()
        .ok_or_else(|| anyhow!("llm agent structured payload missing field: {field}"))
}

fn optional_field(payload: &Value, field: &str) -> Value {
    payload
        .as_object()
        .and_then(|object| object.get(field))
        .cloned()
        .unwrap_or(Value::Null)
}

fn bounded_summary(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut output = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        output.push_str("...");
    }
    output
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::llm_agent_contracts::{
        AgentContextMessage, CONVERSATION_STATE_WRITER_AGENT_TYPE,
        ConversationStateWriterAgentInput, LLM_AGENT_REQUEST_SCHEMA_VERSION,
        QUESTION_NORMALIZER_AGENT_TYPE, QuestionNormalizerAgentInput,
    };

    #[test]
    fn direct_agent_roles_get_distinct_system_prompts() {
        let question_prompt = build_llm_agent_provider_prompt(
            QUESTION_NORMALIZER_PROFILE_ID,
            &question_envelope(),
            None,
        )
        .expect("question prompt");
        let state_prompt = build_llm_agent_provider_prompt(
            CONVERSATION_STATE_WRITER_PROFILE_ID,
            &conversation_state_envelope(),
            None,
        )
        .expect("state prompt");

        assert_ne!(question_prompt.system_prompt, state_prompt.system_prompt);
        assert!(
            question_prompt
                .system_prompt
                .contains("Role: question_normalizer")
        );
        assert!(question_prompt.system_prompt.contains("Do not answer it"));
        assert!(
            state_prompt
                .system_prompt
                .contains("Role: conversation_state_writer")
        );
        assert!(state_prompt.system_prompt.contains("Context-only fields"));
    }

    #[test]
    fn conversation_state_prompt_has_strict_schema_without_raw_envelope() {
        let envelope = conversation_state_envelope();
        let prompt =
            build_llm_agent_provider_prompt(CONVERSATION_STATE_WRITER_PROFILE_ID, &envelope, None)
                .expect("prompt");
        let payload: Value = serde_json::from_str(&prompt.user_payload).expect("payload json");

        assert!(payload.get("agent_request").is_none());
        assert!(payload.get("structured_payload").is_none());
        assert!(!prompt.user_payload.contains("trace-conversation-state"));
        assert!(!prompt.user_payload.contains("interaction-context-test"));
        assert_eq!(payload["task"]["role"], json!("conversation_state_writer"));
        assert_eq!(
            payload["output_contract"]["json_schema"]["additionalProperties"],
            json!(false)
        );
        assert!(contains_string(
            &payload["output_contract"]["allowed_output_fields"],
            "current_topic"
        ));
        for forbidden in [
            "session_summary",
            "required_active_entities",
            "current_question",
            "agent_request_id",
            "trace_id",
        ] {
            assert!(
                contains_string(
                    &payload["output_contract"]["forbidden_output_fields"],
                    forbidden
                ),
                "{forbidden} should be forbidden"
            );
        }
        assert!(payload["input_context"].get("session_summary").is_none());
        assert!(
            payload["input_context"]
                .get("required_active_entities")
                .is_none()
        );
        assert_eq!(
            payload["input_context"]["prior_session_summary_for_context_only"],
            json!("最近讨论对象：晴雯")
        );
        assert_eq!(
            payload["input_context"]["must_include_active_entities"],
            json!(["晴雯"])
        );
    }

    #[test]
    fn repair_prompt_keeps_role_schema_and_omits_envelope() {
        let envelope = conversation_state_envelope();
        let errors = vec![
            "unexpected_conversation_state_field: session_summary".to_string(),
            "schema_deserialize_failed: missing field `current_topic`".to_string(),
        ];
        let prompt = build_llm_agent_provider_prompt(
            CONVERSATION_STATE_WRITER_PROFILE_ID,
            &envelope,
            Some(&errors),
        )
        .expect("repair prompt");
        let payload: Value = serde_json::from_str(&prompt.user_payload).expect("payload json");

        assert!(
            payload["repair"]["previous_validation_error_digest"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert!(
            payload["repair"]["previous_validation_error_summary"]
                .as_str()
                .is_some_and(|value| value.contains("session_summary"))
        );
        assert!(payload.get("agent_request").is_none());
        assert_eq!(
            payload["output_contract"]["json_schema"]["properties"]["object"]["const"],
            json!(CONVERSATION_STATE_SUMMARY_OBJECT)
        );
    }

    #[test]
    fn question_prompt_exposes_allowed_refs_and_forbids_answer_fields() {
        let envelope = question_envelope();
        let prompt =
            build_llm_agent_provider_prompt(QUESTION_NORMALIZER_PROFILE_ID, &envelope, None)
                .expect("prompt");
        let payload: Value = serde_json::from_str(&prompt.user_payload).expect("payload json");

        assert!(payload.get("agent_request").is_none());
        assert_eq!(payload["task"]["role"], json!("question_normalizer"));
        assert_eq!(
            payload["output_contract"]["json_schema"]["properties"]["schema_version"]["const"],
            json!(QUESTION_RESOLVER_SCHEMA_VERSION)
        );
        assert!(contains_string(
            &payload["output_contract"]["forbidden_output_fields"],
            "answer"
        ));
        assert_eq!(
            payload["input_context"]["prior_session_summary_for_context_only"],
            json!("最近讨论对象：晴雯")
        );
        assert!(payload["input_context"].get("session_summary").is_none());
        assert!(contains_string(
            &payload["input_context"]["allowed_context_refs"],
            "session_summary"
        ));
    }

    fn conversation_state_envelope() -> LlmAgentRequestEnvelope {
        let input = ConversationStateWriterAgentInput::new(
            "她后来怎么样？",
            vec![AgentContextMessage {
                role: "user".to_string(),
                content: "介绍晴雯".to_string(),
            }],
            "最近讨论对象：晴雯",
            Some("上一轮只确认晴雯判词位置".to_string()),
            vec!["package:pkg-test".to_string()],
            Vec::new(),
            vec!["晴雯".to_string()],
            vec!["上一轮只确认晴雯判词位置".to_string()],
        );
        LlmAgentRequestEnvelope {
            schema_version: LLM_AGENT_REQUEST_SCHEMA_VERSION.to_string(),
            agent_request_id: "req-test".to_string(),
            request_type: "create_run".to_string(),
            agent_type: CONVERSATION_STATE_WRITER_AGENT_TYPE.to_string(),
            requested_by_service: "tonglingyu-gateway".to_string(),
            requested_by_user: "gateway-context-governance".to_string(),
            status: "parsed".to_string(),
            profile_id: CONVERSATION_STATE_WRITER_PROFILE_ID.to_string(),
            mode: "enforced".to_string(),
            trace_id: "trace-conversation-state".to_string(),
            user_session_id: "user-session-test".to_string(),
            interaction_context_id: "interaction-context-test".to_string(),
            projection_ref: "llm-agent-input://test".to_string(),
            input_digest: "sha256:test".to_string(),
            timeout_ms: 1500,
            requested_tools: Vec::new(),
            structured_payload: json!(input),
        }
    }

    fn question_envelope() -> LlmAgentRequestEnvelope {
        let input = QuestionNormalizerAgentInput::new(
            "prior_subject_needed",
            "她后来怎么样？",
            vec!["介绍晴雯".to_string()],
            vec!["晴雯是重要人物。".to_string()],
            Some("晴雯".to_string()),
            "最近讨论对象：晴雯",
            vec!["晴雯".to_string()],
        );
        LlmAgentRequestEnvelope {
            schema_version: LLM_AGENT_REQUEST_SCHEMA_VERSION.to_string(),
            agent_request_id: "req-question-test".to_string(),
            request_type: "create_run".to_string(),
            agent_type: QUESTION_NORMALIZER_AGENT_TYPE.to_string(),
            requested_by_service: "tonglingyu-gateway".to_string(),
            requested_by_user: "gateway-context-governance".to_string(),
            status: "parsed".to_string(),
            profile_id: QUESTION_NORMALIZER_PROFILE_ID.to_string(),
            mode: "enforced".to_string(),
            trace_id: "trace-question".to_string(),
            user_session_id: "user-session-question".to_string(),
            interaction_context_id: "interaction-context-question".to_string(),
            projection_ref: "llm-agent-input://question".to_string(),
            input_digest: "sha256:question".to_string(),
            timeout_ms: 1500,
            requested_tools: Vec::new(),
            structured_payload: json!(input),
        }
    }

    fn contains_string(value: &Value, expected: &str) -> bool {
        value.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str().is_some_and(|value| value == expected))
        })
    }
}
