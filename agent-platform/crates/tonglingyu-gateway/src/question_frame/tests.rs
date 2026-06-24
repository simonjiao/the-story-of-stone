use super::*;

#[test]
fn relation_question_frame_uses_external_predicate_and_subject_ontology() {
    let frame = build_question_frame("紫鹃服侍过史湘云吗？").expect("frame");

    assert_eq!(frame.intent, "relation_query");
    assert_eq!(frame.canonical_question, "紫鹃服侍过史湘云吗？");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("紫鹃")
    );
    assert_eq!(
        frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert_eq!(
        frame
            .predicate
            .as_ref()
            .map(|predicate| predicate.id.as_str()),
        Some("serve")
    );
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "base_text")
    );
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "commentary")
    );
}

#[test]
fn unknown_relation_predicate_requires_clarification() {
    let frame = build_question_frame("紫鹃照管过史湘云吗？").expect("frame");

    assert_eq!(frame.intent, "unknown_relation_predicate");
    assert_eq!(frame.canonical_question, "紫鹃照管过史湘云吗？");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("紫鹃")
    );
    assert_eq!(
        frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert!(frame.predicate.is_none());
    assert!(frame.needs_clarification);
    assert!(
        frame
            .clarification_question
            .as_deref()
            .is_some_and(|question| question.contains("哪一种关系"))
    );
}

#[test]
fn evidence_terms_do_not_define_relation_intent() {
    let frame = build_question_frame("紫鹃是丫鬟吗？").expect("frame");

    assert_eq!(frame.intent, "entity_query");
    assert!(frame.predicate.is_none());
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("紫鹃")
    );
}

#[test]
fn relation_entity_followup_inherits_open_relation_slot() {
    let resolved =
        resolve_relation_entity_followup("那史湘云呢？", "紫鹃服侍过谁？", "current_window")
            .expect("followup result")
            .expect("followup resolves");

    assert_eq!(resolved.0, "紫鹃服侍过史湘云吗？");
    assert_eq!(resolved.2, "current_window");
    assert_eq!(
        resolved
            .1
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("紫鹃")
    );
    assert_eq!(
        resolved
            .1
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
}

#[test]
fn relation_question_frame_opens_subject_slot_when_placeholder_precedes_predicate() {
    let frame = build_question_frame("谁服侍过史湘云？").expect("frame");

    assert_eq!(frame.intent, "relation_query");
    assert_eq!(frame.canonical_question, "谁服侍过史湘云？");
    assert!(frame.subject.is_none());
    assert_eq!(
        frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert_eq!(frame.open_slot.as_deref(), Some("subject"));
    assert!(!frame.needs_clarification);
}

#[test]
fn relation_entity_followup_fills_open_subject_from_prior_relation_slot() {
    let resolved =
        resolve_relation_entity_followup("袭人呢？", "谁服侍过史湘云？", "current_window")
            .expect("followup result")
            .expect("followup resolves");

    assert_eq!(resolved.0, "袭人服侍过史湘云吗？");
    assert_eq!(
        resolved
            .1
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("袭人")
    );
    assert_eq!(
        resolved
            .1
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert_eq!(
        resolved
            .1
            .context_binding
            .as_ref()
            .map(|binding| binding.binding_reason.as_str()),
        Some("filled_prior_open_subject_relation_slot")
    );
}

#[test]
fn relation_entity_followup_fills_open_object_from_yes_no_entity_turn() {
    let resolved =
        resolve_relation_entity_followup("史大姑娘算吗？", "紫鹃服侍过谁？", "current_window")
            .expect("followup result")
            .expect("followup resolves");

    assert_eq!(resolved.0, "紫鹃服侍过史湘云吗？");
    assert_eq!(
        resolved
            .1
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("紫鹃")
    );
    assert_eq!(
        resolved
            .1
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
}

#[test]
fn relation_entity_followup_uses_external_suffix_and_marker_terms() {
    let suffix_resolved =
        resolve_relation_entity_followup("史大姑娘呢？", "紫鹃服侍过谁？", "current_window")
            .expect("suffix followup result")
            .expect("suffix followup resolves");
    let marker_resolved =
        resolve_relation_entity_followup("史大姑娘算不算？", "紫鹃服侍过谁？", "current_window")
            .expect("marker followup result")
            .expect("marker followup resolves");

    assert_eq!(suffix_resolved.0, "紫鹃服侍过史湘云吗？");
    assert_eq!(marker_resolved.0, "紫鹃服侍过史湘云吗？");
}

#[test]
fn relation_entity_followup_allows_only_open_object_residual_terms() {
    let resolved =
        resolve_relation_entity_followup("史大姑娘也可以吗？", "紫鹃服侍过谁？", "current_window")
            .expect("followup result")
            .expect("followup resolves");

    assert_eq!(resolved.0, "紫鹃服侍过史湘云吗？");
}

#[test]
fn relation_entity_followup_does_not_capture_new_entity_property_question() {
    let resolved =
        resolve_relation_entity_followup("史大姑娘漂亮吗？", "紫鹃服侍过谁？", "current_window")
            .expect("followup result");

    assert!(resolved.is_none());
}

#[test]
fn relation_entity_followup_does_not_capture_standalone_entity_question() {
    let resolved =
        resolve_relation_entity_followup("那史湘云是谁？", "紫鹃服侍过谁？", "current_window")
            .expect("followup result");

    assert!(resolved.is_none());
}

#[test]
fn evidence_followup_frame_preserves_subject_and_commentary_scope() {
    let frame = build_question_frame("关于史湘云的结局，脂批中的证据呢？").expect("frame");

    assert_eq!(frame.intent, "evidence_query");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert_eq!(frame.source_scope, "pre_80_base_text_and_commentary");
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "commentary")
    );
}

#[test]
fn character_fate_question_uses_dedicated_intent_and_default_scope() {
    let frame = build_question_frame("林黛玉结局如何").expect("frame");

    assert_eq!(frame.intent, "character_fate_query");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(frame.source_scope, "pre_80_base_text_and_commentary");
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "base_text")
    );
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "commentary")
    );
    assert!(!frame.needs_clarification);
}

#[test]
fn character_fate_question_without_subject_requires_clarification() {
    let frame = build_question_frame("结局如何").expect("frame");

    assert_eq!(frame.intent, "character_fate_query");
    assert!(frame.subject.is_none());
    assert_eq!(frame.open_slot.as_deref(), Some("subject"));
    assert!(frame.needs_clarification);
    assert!(
        frame
            .clarification_question
            .as_deref()
            .is_some_and(|question| question.contains("哪位人物"))
    );
}

#[test]
fn chapter_location_question_uses_dedicated_intent_not_entity_intro() {
    let frame = build_question_frame("林黛玉葬花是在那一回？").expect("frame");

    assert_eq!(frame.intent, "chapter_location_query");
    assert_eq!(frame.canonical_question, "林黛玉葬花是在那一回？");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert!(frame.predicate.is_none());
    assert!(frame.object.is_none());
    assert_eq!(frame.open_slot.as_deref(), None);
    assert!(!frame.needs_clarification);
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "base_text")
    );
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "commentary")
    );
}

#[test]
fn chapter_location_question_without_event_requires_clarification() {
    let frame = build_question_frame("林黛玉是在那一回？").expect("frame");

    assert_eq!(frame.intent, "chapter_location_query");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(frame.open_slot.as_deref(), Some("event"));
    assert!(frame.needs_clarification);
    assert!(
        frame
            .clarification_question
            .as_deref()
            .is_some_and(|question| question.contains("具体情节"))
    );
}

#[test]
fn source_scope_phrase_updates_explicit_later_forty_scope() {
    let frame = build_question_frame("关于史湘云的结局，后四十回呢？").expect("frame");

    assert_eq!(frame.intent, "character_fate_query");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("史湘云")
    );
    assert_eq!(frame.source_scope, "later_40_base_text");
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "base_text")
    );
    assert!(
        frame
            .required_evidence_types
            .iter()
            .all(|item| item != "commentary")
    );
}

#[test]
fn count_question_frame_uses_external_count_terms() {
    let frame = build_question_frame("通灵宝玉丢了几次？").expect("frame");

    assert_eq!(frame.intent, "count_query");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("通灵宝玉")
    );
    assert_eq!(frame.source_scope, "pre_80_base_text_and_commentary");
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "base_text")
    );
}

#[test]
fn attribute_at_event_frame_uses_external_age_rules() {
    let frame = build_question_frame("林黛玉进贾府时几岁了").expect("frame");

    assert_eq!(frame.intent, "attribute_at_event");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(
        frame
            .predicate
            .as_ref()
            .map(|predicate| predicate.id.as_str()),
        Some("age")
    );
    assert!(!frame.needs_clarification);
    assert!(
        frame
            .required_evidence_types
            .iter()
            .any(|item| item == "base_text")
    );
}

#[test]
fn attribute_at_event_frame_accepts_duoda_event_wording() {
    let frame = build_question_frame("林黛玉进贾府多大了").expect("frame");

    assert_eq!(frame.intent, "attribute_at_event");
    assert_eq!(
        frame
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(
        frame
            .predicate
            .as_ref()
            .map(|predicate| predicate.id.as_str()),
        Some("age")
    );
}

#[test]
fn attribute_compare_frame_opens_subject_for_context_target() {
    let frame = build_question_frame("和贾宝玉相比，谁的年龄大").expect("frame");

    assert_eq!(frame.intent, "attribute_compare");
    assert!(frame.subject.is_none());
    assert_eq!(
        frame
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("贾宝玉")
    );
    assert_eq!(frame.open_slot.as_deref(), Some("subject"));
    assert!(frame.needs_clarification);
    assert_ne!(frame.intent, "unknown_relation_predicate");
}

#[test]
fn attribute_compare_followup_fills_prior_topic_subject() {
    let resolved = resolve_attribute_compare_followup(
        "和贾宝玉相比，谁的年龄大",
        "林黛玉进贾府时几岁了",
        "current_window",
    )
    .expect("followup result")
    .expect("followup resolves");

    assert_eq!(resolved.0, "林黛玉和贾宝玉相比，谁的年龄更大？");
    assert_eq!(
        resolved
            .1
            .subject
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("林黛玉")
    );
    assert_eq!(
        resolved
            .1
            .object
            .as_ref()
            .map(|entity| entity.canonical.as_str()),
        Some("贾宝玉")
    );
    assert_eq!(resolved.1.open_slot.as_deref(), None);
    assert_eq!(
        resolved
            .1
            .context_binding
            .as_ref()
            .map(|binding| binding.binding_reason.as_str()),
        Some("filled_prior_attribute_compare_subject")
    );
}
