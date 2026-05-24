use super::*;
use serde_json::json;

fn relation_frame() -> RuntimeQuestionFrame {
    serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "紫鹃服侍过史湘云吗？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "湘云"]},
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("frame")
}

#[test]
fn relation_search_query_binds_subject_predicate_and_object_terms() {
    let frame = relation_frame();
    let query = relation_search_query("紫鹃服侍过史湘云吗？", Some(&frame));

    assert!(query.contains("紫鹃"));
    assert!(query.contains("服侍"));
    assert!(query.contains("丫鬟"));
    assert!(query.contains("史湘云"));
}

#[test]
fn relation_review_requires_direct_relation_support_for_yes_no_relation() {
    let frame = relation_frame();
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "source title".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "史湘云偶填柳絮词。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    assert_eq!(
        relation_review_issues(Some(&frame), &cards),
        vec!["relation_predicate_evidence_missing"]
    );
    let answer = relation_boundary_answer(Some(&frame), &cards).expect("boundary answer");
    assert!(answer.contains("未见直接证据"));
    assert!(answer.contains("紫鹃服侍过史湘云"));
}
