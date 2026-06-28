use super::super::{
    ContextMessage, ContextRequestInput, latest_public_answer_boundary_from_current_window,
    resolve_question,
};
use super::{
    FakeRuntimeClient, create_context_for_request_with_agent_runtime_and_modes, file_conn,
    remove_file_db, temp_context_db_path,
};
use crate::llm_modes::LlmMode;

#[test]
fn resolver_fills_open_relation_object_for_yes_no_entity_followup() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "紫鵑服侍过谁？".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "紫鹃是服侍黛玉的。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "史大姑娘算吗？".to_string(),
        },
    ];

    let resolved = resolve_question("史大姑娘算吗？", &messages, None, None).expect("resolves");

    assert_eq!(resolved.resolved_question, "紫鹃服侍过史湘云吗？");
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.referent_bindings, vec!["紫鹃", "史湘云"]);
    assert_eq!(resolved.question_frame.intent, "relation_query");
    assert_eq!(
        resolved
            .question_frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
}

#[test]
fn resolver_binds_pronoun_relation_subject_before_current_object_entity() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "袭人主要服侍谁？".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "袭人主要在宝玉身边服侍。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "她之前服侍过老太太吗？".to_string(),
        },
    ];

    let resolved =
        resolve_question("她之前服侍过老太太吗？", &messages, None, None).expect("resolves");

    assert_eq!(resolved.resolved_question, "袭人之前服侍过老太太吗？");
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.question_frame.intent, "relation_query");
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("袭人")
    );
    assert_eq!(
        resolved
            .question_frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("贾母")
    );
}

#[test]
fn resolver_fills_open_subject_relation_slot_for_entity_followup() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "谁服侍过史湘云？".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "需要检索关系证据。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "袭人呢？".to_string(),
        },
    ];

    let resolved = resolve_question("袭人呢？", &messages, None, None).expect("resolves");

    assert_eq!(resolved.resolved_question, "袭人服侍过史湘云吗？");
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.referent_bindings, vec!["袭人", "史湘云"]);
    assert_eq!(resolved.question_frame.open_slot.as_deref(), None);
}

#[test]
fn resolver_binds_evidence_followup_to_current_window_anchor() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "史湘云的结局".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "先列可检索证据。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "脂批中的证据呢？".to_string(),
        },
    ];

    let resolved = resolve_question("脂批中的证据呢？", &messages, None, None).expect("resolves");

    assert_eq!(
        resolved.resolved_question,
        "关于史湘云的结局，脂批中的证据呢？"
    );
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.question_frame.intent, "evidence_query");
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
}

#[test]
fn resolver_treats_named_death_chapter_question_as_character_fate() {
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: "秦钟是哪一回死的".to_string(),
    }];

    let resolved = resolve_question("秦钟是哪一回死的", &messages, None, None).expect("resolves");

    assert_eq!(resolved.resolved_question, "秦钟是哪一回死的");
    assert_eq!(resolved.question_frame.intent, "character_fate_query");
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("秦钟")
    );
    assert_eq!(resolved.question_frame.needs_clarification, false);
}

#[test]
fn resolver_binds_scope_followup_to_current_window_anchor() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "史湘云的结局".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "先回答默认范围。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "后四十回呢？".to_string(),
        },
    ];

    let resolved = resolve_question("后四十回呢？", &messages, None, None).expect("resolves");

    assert_eq!(resolved.resolved_question, "关于史湘云的结局，后四十回呢？");
    assert_eq!(resolved.question_frame.source_scope, "later_40_base_text");
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
}

#[test]
fn resolver_binds_attribute_compare_followup_to_prior_topic_subject() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "林黛玉进贾府时几岁了".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "当前证据不足以确定林黛玉进贾府时的年龄。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "和贾宝玉相比，谁的年龄大".to_string(),
        },
    ];

    let resolved =
        resolve_question("和贾宝玉相比，谁的年龄大", &messages, None, None).expect("resolves");

    assert_eq!(
        resolved.resolved_question,
        "林黛玉和贾宝玉相比，谁的年龄更大？"
    );
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.question_frame.intent, "attribute_compare");
    assert_eq!(resolved.question_frame.open_slot.as_deref(), None);
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(
        resolved
            .question_frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("贾宝玉")
    );
}

#[test]
fn resolver_binds_contextual_analysis_to_prior_topic_question() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "林黛玉进贾府多大了".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "当前证据支持有限推算。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "结合前文进行分析下".to_string(),
        },
    ];

    let resolved = resolve_question("结合前文进行分析下", &messages, None, None).expect("resolves");

    assert_eq!(
        resolved.resolved_question,
        "关于林黛玉进贾府多大了，结合前文进行分析下"
    );
    assert_eq!(resolved.strategy, "deterministic_contextual_continuation");
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.question_frame.intent, "attribute_at_event");
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
}

#[test]
fn resolver_binds_reasoning_followup_to_prior_topic_question() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "林黛玉进贾府时多大了".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "当前证据支持有限推算。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "你的推理逻辑是什么".to_string(),
        },
    ];

    let resolved = resolve_question("你的推理逻辑是什么", &messages, None, None).expect("resolves");

    assert_eq!(
        resolved.resolved_question,
        "关于林黛玉进贾府时多大了，你的推理逻辑是什么"
    );
    assert_eq!(resolved.strategy, "deterministic_contextual_continuation");
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
    assert_eq!(resolved.question_frame.intent, "attribute_at_event");
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(
        resolved
            .question_frame
            .predicate
            .as_ref()
            .map(|predicate| predicate.id.as_str()),
        Some("age")
    );
}

#[test]
fn resolver_skips_contextual_analysis_when_binding_later_attribute_compare() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "林黛玉进贾府多大了".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "当前证据支持有限推算。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "结合前文进行分析下".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "仍然围绕林黛玉进贾府年龄分析。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "和贾宝玉相比，谁的年龄大".to_string(),
        },
    ];

    let resolved =
        resolve_question("和贾宝玉相比，谁的年龄大", &messages, None, None).expect("resolves");

    assert_eq!(
        resolved.resolved_question,
        "林黛玉和贾宝玉相比，谁的年龄更大？"
    );
    assert_eq!(
        resolved.strategy,
        "deterministic_attribute_compare_followup"
    );
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(
        resolved
            .question_frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("贾宝玉")
    );
}

#[test]
fn current_window_public_answer_boundary_overrides_stale_journal_topic() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "林黛玉进贾府时几岁了".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "当前证据只围绕林黛玉进贾府年龄问题，不能扩展为其他人物定论。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "和贾宝玉相比，谁的年龄大".to_string(),
        },
    ];

    let boundary =
        latest_public_answer_boundary_from_current_window(&messages, "和贾宝玉相比，谁的年龄大")
            .expect("boundary");

    assert!(boundary.contains("林黛玉"));
    assert!(!boundary.contains("通灵宝玉"));
}

#[test]
fn resolver_binds_scope_followup_to_latest_topic_not_prior_evidence_followup() {
    let messages = vec![
        ContextMessage {
            role: "user".to_string(),
            content: "史湘云的结局".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "先回答默认范围。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "脂批中的证据呢？".to_string(),
        },
        ContextMessage {
            role: "assistant".to_string(),
            content: "脂批有相关判词证据。".to_string(),
        },
        ContextMessage {
            role: "user".to_string(),
            content: "后四十回呢？".to_string(),
        },
    ];

    let resolved = resolve_question("后四十回呢？", &messages, None, None).expect("resolves");

    assert_eq!(resolved.resolved_question, "关于史湘云的结局，后四十回呢？");
    assert_eq!(resolved.question_frame.source_scope, "later_40_base_text");
    assert_eq!(
        resolved
            .question_frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert!(
        !resolved
            .question_frame
            .required_evidence_types
            .contains(&"commentary".to_string())
    );
    assert_eq!(resolved.used_context_refs, vec!["current_window"]);
}

#[test]
fn resolver_clarifies_scope_followup_without_anchor() {
    let resolved = resolve_question("后四十回呢？", &[], None, None).expect("resolves");

    assert!(resolved.needs_clarification);
    assert_eq!(
        resolved.unsupported_reason.as_deref(),
        Some("unresolved_scope_or_evidence_followup")
    );
}

#[test]
fn resolver_clarifies_implicit_missing_relation_subject() {
    let resolved = resolve_question("服侍过史湘云吗？", &[], None, None).expect("resolves");

    assert!(resolved.needs_clarification);
    assert_eq!(
        resolved.unsupported_reason.as_deref(),
        Some("question_frame_needs_clarification")
    );
    assert_eq!(
        resolved.question_frame.open_slot.as_deref(),
        Some("subject")
    );
}

#[test]
fn resolver_clarifies_unknown_relation_predicate_without_subject_fallback() {
    let resolved = resolve_question("紫鹃照管过史湘云吗？", &[], None, None).expect("resolves");

    assert!(resolved.needs_clarification);
    assert_eq!(
        resolved.unsupported_reason.as_deref(),
        Some("unknown_relation_predicate")
    );
    assert_eq!(resolved.question_frame.intent, "unknown_relation_predicate");
    assert!(resolved.question_frame.predicate.is_none());
    assert_eq!(resolved.referent_bindings, vec!["紫鹃", "史湘云"]);
}

#[tokio::test]
async fn unknown_relation_predicate_does_not_call_question_normalizer_as_oracle() {
    let db_path = temp_context_db_path("unknown-relation-no-question-agent");
    let conn = file_conn(&db_path);
    drop(conn);
    let runtime = FakeRuntimeClient::new(Vec::new());
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: "紫鹃照管过史湘云吗？".to_string(),
    }];

    let context = create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-unknown-relation-no-question-agent",
            model_id: "tonglingyu",
            external_user_ref: "user-unknown-relation",
            external_session_id: "session-unknown-relation",
            external_message_id: "message-unknown-relation",
            question: "紫鹃照管过史湘云吗？",
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    assert!(context.needs_clarification);
    assert_eq!(
        context.unsupported_reason.as_deref(),
        Some("unknown_relation_predicate")
    );
    assert_eq!(runtime.profile_inputs().len(), 0);
    remove_file_db(&db_path);
}

#[tokio::test]
async fn explicit_current_entity_with_demonstrative_does_not_call_question_normalizer() {
    let db_path = temp_context_db_path("explicit-entity-demonstrative-no-question-agent");
    let conn = file_conn(&db_path);
    drop(conn);
    let runtime = FakeRuntimeClient::new(Vec::new());
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: "说说袭人这个人物".to_string(),
    }];

    let context = create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-explicit-entity-demonstrative",
            model_id: "tonglingyu",
            external_user_ref: "user-explicit-entity",
            external_session_id: "session-explicit-entity",
            external_message_id: "message-explicit-entity",
            question: "说说袭人这个人物",
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    assert!(!context.needs_clarification);
    assert_eq!(context.resolved_question, "说说袭人这个人物");
    assert_eq!(runtime.profile_inputs().len(), 0);
    remove_file_db(&db_path);
}
