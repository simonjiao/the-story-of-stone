use super::*;

#[test]
fn unknown_relation_predicate_uses_external_markers() {
    let parsed = parse_unknown_relation_predicate_question("紫鹃照管过史湘云吗？")
        .expect("unknown relation parse")
        .expect("unknown relation predicate detected");

    assert_eq!(parsed.subject.as_deref(), Some("紫鹃"));
    assert_eq!(parsed.object.as_deref(), Some("史湘云"));
    assert_eq!(parsed.open_slot, None);
    assert_eq!(parsed.predicate_candidate_term.as_deref(), Some("照管"));
    assert!(parsed.clarification_question.contains("紫鹃"));
    assert!(parsed.clarification_question.contains("史湘云"));
}

#[test]
fn unknown_relation_predicate_does_not_stage_generic_interrogative_as_alias() {
    let parsed = parse_unknown_relation_predicate_question("紫鹃和史湘云是什么关系？")
        .expect("unknown relation parse")
        .expect("unknown relation predicate detected");

    assert_eq!(parsed.predicate_candidate_term, None);
}
