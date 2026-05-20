use agent_core::{ProfileContract, RuntimeToolPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::llm_contracts::{
    CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION, LLM_RESOLVER_ALLOWED_CONTEXT_REFS,
    LLM_RESOLVER_FORBIDDEN_FIELDS, QUESTION_RESOLVER_SCHEMA_VERSION,
};

pub(crate) const LLM_AGENT_REQUEST_SCHEMA_VERSION: &str = "tonglingyu-llm-agent-request-v1";
pub(crate) const LLM_AGENT_VALIDATOR_SCHEMA_VERSION: &str = "tonglingyu-llm-agent-validator-v1";
pub(crate) const QUESTION_NORMALIZER_PROFILE_ID: &str = "tonglingyu-question-normalizer";
pub(crate) const CONVERSATION_STATE_WRITER_PROFILE_ID: &str =
    "tonglingyu-conversation-state-writer";
pub(crate) const QUESTION_NORMALIZER_AGENT_TYPE: &str = "tonglingyu_question_normalizer";
pub(crate) const CONVERSATION_STATE_WRITER_AGENT_TYPE: &str =
    "tonglingyu_conversation_state_writer";
pub(crate) const QUESTION_NORMALIZER_TIMEOUT_MS: u64 = 1_500;
pub(crate) const CONVERSATION_STATE_WRITER_TIMEOUT_MS: u64 = 1_500;

pub(crate) const QUESTION_NORMALIZER_SYSTEM_PROMPT: &str = r#"You are a Tonglingyu question-normalization runtime profile.
Return exactly one JSON object matching schema_version tonglingyu-question-resolver-v1.
You may only rewrite the user's question and bind referents from the provided bounded context.
Do not answer the question. Do not add facts. Do not change tool policy, ACLs, memory policy, evidence package IDs, or reviewer decisions.
If the referent is not supported by the provided context, return needs_clarification=true."#;

pub(crate) const CONVERSATION_STATE_WRITER_SYSTEM_PROMPT: &str = r#"You are a Tonglingyu conversation-state runtime profile.
Return exactly one JSON object representing tonglingyu.conversation_state_summary.
Use only the provided bounded conversation context and public-answer boundary.
Do not introduce evidence claims. Do not use memory as evidence. Do not emit trace IDs, context IDs, raw prompts, tool payloads, ACLs, or policy controls."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LlmAgentRequestEnvelope {
    pub(crate) schema_version: String,
    pub(crate) agent_request_id: String,
    pub(crate) request_type: String,
    pub(crate) agent_type: String,
    pub(crate) requested_by_service: String,
    pub(crate) requested_by_user: String,
    pub(crate) status: String,
    pub(crate) profile_id: String,
    pub(crate) mode: String,
    pub(crate) trace_id: String,
    pub(crate) user_session_id: String,
    pub(crate) interaction_context_id: String,
    pub(crate) projection_ref: String,
    pub(crate) input_digest: String,
    pub(crate) timeout_ms: u64,
    pub(crate) requested_tools: Vec<String>,
    pub(crate) structured_payload: Value,
}

impl LlmAgentRequestEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        agent_request_id: impl Into<String>,
        agent_type: impl Into<String>,
        profile_id: impl Into<String>,
        mode: impl Into<String>,
        trace_id: impl Into<String>,
        user_session_id: impl Into<String>,
        interaction_context_id: impl Into<String>,
        projection_ref: impl Into<String>,
        input_digest: impl Into<String>,
        timeout_ms: u64,
        structured_payload: Value,
    ) -> Self {
        Self {
            schema_version: LLM_AGENT_REQUEST_SCHEMA_VERSION.to_string(),
            agent_request_id: agent_request_id.into(),
            request_type: "create_run".to_string(),
            agent_type: agent_type.into(),
            requested_by_service: "tonglingyu-gateway".to_string(),
            requested_by_user: "gateway-context-governance".to_string(),
            status: "parsed".to_string(),
            profile_id: profile_id.into(),
            mode: mode.into(),
            trace_id: trace_id.into(),
            user_session_id: user_session_id.into(),
            interaction_context_id: interaction_context_id.into(),
            projection_ref: projection_ref.into(),
            input_digest: input_digest.into(),
            timeout_ms,
            requested_tools: Vec::new(),
            structured_payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentContextMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct QuestionNormalizerAgentInput {
    pub(crate) schema_version: String,
    pub(crate) trigger: String,
    pub(crate) current_question: String,
    pub(crate) recent_user_messages: Vec<String>,
    pub(crate) recent_assistant_messages: Vec<String>,
    pub(crate) prior_subject: Option<String>,
    pub(crate) session_summary: String,
    pub(crate) allowed_context_refs: Vec<String>,
    pub(crate) allowed_referents: Vec<String>,
    pub(crate) forbidden_output_fields: Vec<String>,
}

impl QuestionNormalizerAgentInput {
    pub(crate) fn new(
        trigger: impl Into<String>,
        current_question: impl Into<String>,
        recent_user_messages: Vec<String>,
        recent_assistant_messages: Vec<String>,
        prior_subject: Option<String>,
        session_summary: impl Into<String>,
        allowed_referents: Vec<String>,
    ) -> Self {
        Self {
            schema_version: QUESTION_RESOLVER_SCHEMA_VERSION.to_string(),
            trigger: trigger.into(),
            current_question: current_question.into(),
            recent_user_messages,
            recent_assistant_messages,
            prior_subject,
            session_summary: session_summary.into(),
            allowed_context_refs: LLM_RESOLVER_ALLOWED_CONTEXT_REFS
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
            allowed_referents,
            forbidden_output_fields: LLM_RESOLVER_FORBIDDEN_FIELDS
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionNormalizerAgentOutput {
    pub(crate) schema_version: String,
    pub(crate) resolved_question: String,
    #[serde(default)]
    pub(crate) referent_bindings: Vec<String>,
    #[serde(default)]
    pub(crate) used_context_refs: Vec<String>,
    pub(crate) confidence: f64,
    pub(crate) needs_clarification: bool,
    pub(crate) clarification_question: Option<String>,
    pub(crate) unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConversationStateWriterAgentInput {
    pub(crate) schema_version: String,
    pub(crate) current_question: String,
    pub(crate) recent_messages: Vec<AgentContextMessage>,
    pub(crate) session_summary: String,
    pub(crate) last_public_answer_boundary: Option<String>,
    pub(crate) evidence_package_refs: Vec<String>,
    pub(crate) reviewer_warnings: Vec<String>,
    pub(crate) required_active_entities: Vec<String>,
    pub(crate) required_last_answer_boundaries: Vec<String>,
    pub(crate) forbidden_output_fields: Vec<String>,
}

impl ConversationStateWriterAgentInput {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        current_question: impl Into<String>,
        recent_messages: Vec<AgentContextMessage>,
        session_summary: impl Into<String>,
        last_public_answer_boundary: Option<String>,
        evidence_package_refs: Vec<String>,
        reviewer_warnings: Vec<String>,
        required_active_entities: Vec<String>,
        required_last_answer_boundaries: Vec<String>,
    ) -> Self {
        Self {
            schema_version: CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION.to_string(),
            current_question: current_question.into(),
            recent_messages,
            session_summary: session_summary.into(),
            last_public_answer_boundary,
            evidence_package_refs,
            reviewer_warnings,
            required_active_entities,
            required_last_answer_boundaries,
            forbidden_output_fields: LLM_RESOLVER_FORBIDDEN_FIELDS
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
        }
    }
}

pub(crate) fn tonglingyu_llm_agent_profile_contracts() -> Vec<ProfileContract> {
    vec![
        question_normalizer_profile_contract(),
        conversation_state_writer_profile_contract(),
    ]
}

pub(crate) fn question_normalizer_profile_contract() -> ProfileContract {
    read_only_profile_contract(
        QUESTION_NORMALIZER_PROFILE_ID,
        QUESTION_RESOLVER_SCHEMA_VERSION,
        QUESTION_NORMALIZER_TIMEOUT_MS,
    )
}

pub(crate) fn conversation_state_writer_profile_contract() -> ProfileContract {
    read_only_profile_contract(
        CONVERSATION_STATE_WRITER_PROFILE_ID,
        CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
        CONVERSATION_STATE_WRITER_TIMEOUT_MS,
    )
}

fn read_only_profile_contract(profile_id: &str, version: &str, timeout_ms: u64) -> ProfileContract {
    let mut contract = ProfileContract::new(profile_id, version);
    contract.input_schema = json!({
        "type": "object",
        "required": ["kind", "profile_id", "messages", "trace_id"],
        "properties": {
            "kind": {"enum": ["profile_step"]},
            "profile_id": {"enum": [profile_id]},
            "messages": {
                "type": "array",
                "minItems": 2,
                "items": {
                    "type": "object",
                    "required": ["role", "content"],
                    "properties": {
                        "role": {"type": "string"},
                        "content": {"type": "string"}
                    }
                }
            },
            "requested_tools": {"type": "array"},
            "trace_id": {"type": "string"}
        }
    });
    contract.output_schema = json!({
        "type": "object",
        "required": ["result_summary"],
        "properties": {
            "result_summary": {"type": "string"},
            "result_ref": {"type": "string"},
            "metadata": {"type": "object"}
        }
    });
    contract.tool_policy = RuntimeToolPolicy {
        allowed_tools: Vec::new(),
        denied_tools: vec![
            "external_action.apply".to_string(),
            "external_action.compensate".to_string(),
            "direct_external_write".to_string(),
            "tonglingyu.evidence.package.create".to_string(),
            "tonglingyu.memory.write".to_string(),
        ],
        tool_specs: Vec::new(),
    };
    contract.max_context_messages = Some(3);
    contract.max_runtime_seconds = Some((timeout_ms / 1_000).max(1));
    contract.safety_policy = json!({
        "deny_message_roles": ["tool"],
        "max_message_bytes": 32768
    });
    contract
}

#[cfg(test)]
mod tests {
    use agent_core::{RuntimeClient, RuntimeProfileInput, RuntimeProfileMessage};
    use agent_runtime::MinimalRuntimeClient;
    use serde_json::json;

    use super::tonglingyu_llm_agent_profile_contracts;

    #[tokio::test]
    async fn llm_agent_profile_contracts_pass_runtime_safety_gate() {
        let runtime = MinimalRuntimeClient::default();
        for contract in tonglingyu_llm_agent_profile_contracts() {
            let profile_id = contract.profile_id.clone();
            let result = runtime
                .execute_profile_step(RuntimeProfileInput {
                    profile_id: profile_id.clone(),
                    messages: vec![
                        RuntimeProfileMessage::new("system", "return JSON only"),
                        RuntimeProfileMessage::new("user", "{}"),
                    ],
                    metadata: json!({}),
                    profile_contract: Some(contract),
                    runtime_step: None,
                    requested_tools: Vec::new(),
                    trace_id: "trace-llm-agent-contract-safety".to_string(),
                })
                .await;

            assert!(result.is_ok(), "{profile_id} rejected: {result:?}");
        }
    }
}
