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
