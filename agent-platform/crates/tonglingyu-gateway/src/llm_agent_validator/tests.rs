use serde_json::json;

use super::*;
use crate::{
    llm_agent_contracts::{CONVERSATION_STATE_WRITER_AGENT_TYPE, LLM_AGENT_REQUEST_SCHEMA_VERSION},
    question_frame,
};

fn envelope(profile_id: &str) -> LlmAgentRequestEnvelope {
    LlmAgentRequestEnvelope {
        schema_version: LLM_AGENT_REQUEST_SCHEMA_VERSION.to_string(),
        agent_request_id: "req-candidate-test".to_string(),
        request_type: "create_run".to_string(),
        agent_type: CONVERSATION_STATE_WRITER_AGENT_TYPE.to_string(),
        requested_by_service: "test".to_string(),
        requested_by_user: "test".to_string(),
        status: "parsed".to_string(),
        profile_id: profile_id.to_string(),
        mode: "enforced".to_string(),
        trace_id: "trace-candidate-test".to_string(),
        user_session_id: "user-session-candidate-test".to_string(),
        interaction_context_id: "interaction-context-candidate-test".to_string(),
        projection_ref: "llm-agent-input://candidate-test".to_string(),
        input_digest: "sha256:candidate-test".to_string(),
        timeout_ms: 1500,
        requested_tools: Vec::new(),
        structured_payload: json!({}),
    }
}

#[test]
fn question_validator_accepts_valid_frame_candidate_and_rule_candidates() {
    let resolved_question = "紫鹃服侍过史湘云吗？";
    let frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    let output = json!({
        "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": ["紫鹃", "史湘云"],
        "used_context_refs": ["current_question"],
        "confidence": 0.92,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": frame,
        "rule_candidates": [{
            "candidate_type": "entity_alias",
            "term": "枕霞客",
            "reason": "用户表达显示它可能是史湘云别名"
        }]
    });

    let decision = validate_question_normalizer_runtime_output(
        LlmMode::Enforced,
        "prior_subject_needed",
        &envelope(QUESTION_NORMALIZER_PROFILE_ID),
        &output.to_string(),
        Some("openai-compatible-network://profiles/test"),
        None,
        &["紫鹃".to_string(), "史湘云".to_string()],
    );

    assert!(decision.contract_accepted());
    assert_eq!(decision.audit_json()["rule_candidate_count"], json!(1));
    assert_eq!(
        decision.audit_json()["rule_candidates"][0]["term"],
        json!("枕霞客")
    );
    assert_eq!(
        decision
            .accepted_resolution()
            .and_then(|sealed| sealed.question_frame())
            .and_then(|frame| frame.object.as_ref())
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
}

#[test]
fn question_validator_rejects_frame_candidate_outside_active_ontology() {
    let resolved_question = "紫鹃服侍过史湘云吗？";
    let mut frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    frame["object"]["canonical"] = json!("不存在的人物");
    let output = json!({
        "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.92,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": frame,
        "rule_candidates": []
    });

    let decision = validate_question_normalizer_runtime_output(
        LlmMode::Enforced,
        "prior_subject_needed",
        &envelope(QUESTION_NORMALIZER_PROFILE_ID),
        &output.to_string(),
        None,
        None,
        &[],
    );

    assert!(!decision.contract_accepted());
    assert!(decision.accepted_resolution().is_none());
    assert!(
        decision
            .errors()
            .iter()
            .any(|error| error.contains("question_frame_candidate_unknown_entity"))
    );
    assert_eq!(decision.audit_json()["rule_candidate_count"], json!(0));
}

#[test]
fn question_validator_rejects_frame_candidate_with_answer_fields() {
    let resolved_question = "紫鹃服侍过史湘云吗？";
    let mut frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    frame["answer"] = json!("紫鹃服侍过史湘云");
    let output = json!({
        "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.92,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": frame,
        "rule_candidates": []
    });

    let decision = validate_question_normalizer_runtime_output(
        LlmMode::Enforced,
        "prior_subject_needed",
        &envelope(QUESTION_NORMALIZER_PROFILE_ID),
        &output.to_string(),
        None,
        None,
        &[],
    );

    assert!(!decision.contract_accepted());
    assert!(decision.accepted_resolution().is_none());
    assert!(
        decision
            .errors()
            .iter()
            .any(|error| error.contains("question_frame_candidate_deserialize_failed"))
    );
}

#[test]
fn question_validator_preserves_clarification_boundary_for_low_confidence_candidate() {
    let output = json!({
        "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
        "resolved_question": "她的结局呢？",
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.2,
        "needs_clarification": true,
        "clarification_question": "请明确你问的是哪位人物的结局。",
        "unsupported_reason": "unresolved_referent",
        "rule_candidates": [{
            "candidate_type": "clarification_pattern",
            "term": "请明确你问的是哪位人物",
            "reason": "低置信指代题需要普通澄清"
        }]
    });

    let decision = validate_question_normalizer_runtime_output(
        LlmMode::Enforced,
        "unresolved_referent",
        &envelope(QUESTION_NORMALIZER_PROFILE_ID),
        &output.to_string(),
        None,
        None,
        &[],
    );

    let sealed = decision.accepted_resolution().expect("clarify sealed");
    assert!(decision.contract_accepted());
    assert!(sealed.needs_clarification());
    assert_eq!(
        sealed.clarification_question(),
        Some("请明确你问的是哪位人物的结局。")
    );
    assert_eq!(decision.audit_json()["decision"], json!("clarify"));
}

#[test]
fn question_validator_ignores_invalid_frame_candidate_when_clarifying() {
    let resolved_question = "她后来怎么样？";
    let mut frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    frame["source_scope"] = json!("later_40_base_text");
    let output = json!({
        "schema_version": QUESTION_RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.2,
        "needs_clarification": true,
        "clarification_question": "请明确你问的是哪位人物的结局。",
        "unsupported_reason": "unresolved_referent",
        "question_frame_candidate": frame,
        "rule_candidates": []
    });

    let decision = validate_question_normalizer_runtime_output(
        LlmMode::Enforced,
        "unresolved_referent",
        &envelope(QUESTION_NORMALIZER_PROFILE_ID),
        &output.to_string(),
        Some("openai-compatible-network://profiles/test"),
        None,
        &[],
    );

    let sealed = decision.accepted_resolution().expect("clarify sealed");
    assert!(decision.contract_accepted());
    assert!(sealed.needs_clarification());
    assert!(decision.errors().is_empty());
    assert_eq!(
        decision.audit_json()["question_frame_candidate_present"],
        json!(true)
    );
    assert_eq!(
        decision.audit_json()["question_frame_candidate_accepted"],
        json!(false)
    );
    assert!(
        decision.audit_json()["question_frame_candidate_errors"][0]
            .as_str()
            .expect("candidate error")
            .contains("question_frame_candidate_source_scope_mismatch")
    );
}
