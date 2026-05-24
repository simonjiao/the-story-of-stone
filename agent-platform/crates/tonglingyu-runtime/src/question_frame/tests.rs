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
fn frame_search_query_expands_entity_aliases() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");

    let query = frame_search_query("紫鹃在《红楼梦》里是什么样的人？", Some(&frame));

    assert!(query.contains("紫鹃"));
    assert!(query.contains("紫鵑"));
    assert!(query.contains("鹦哥"));
    assert!(query.contains("鸚哥"));
}

#[test]
fn frame_focus_terms_collects_entity_aliases_without_question_noise() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");

    let terms = frame_focus_terms(Some(&frame));

    assert!(terms.contains(&"紫鹃".to_string()));
    assert!(terms.contains(&"紫鵑".to_string()));
    assert!(terms.contains(&"鸚哥".to_string()));
    assert!(!terms.contains(&"红楼梦".to_string()));
}

#[test]
fn entity_intro_answer_rejects_unfocused_evidence_list_template() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第001回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "從此空空道人因空見色，由色生情，改《石頭記》為《情僧錄》。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    let answer = question_frame_answer(Some(&frame), &cards).expect("entity answer");

    assert!(answer.contains("紫鹃"));
    assert!(answer.contains("没有命中"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
}

#[test]
fn entity_intro_answer_uses_focused_evidence_card() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第003回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "賈母見雪雁甚小，便將自己身邊的一個二等丫頭，名喚鸚哥者與了黛玉。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    let answer = question_frame_answer(Some(&frame), &cards).expect("entity answer");

    assert!(answer.contains("紫鹃"));
    assert!(answer.contains("紅樓夢/第003回"));
    assert!(answer.contains("鸚哥"));
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
    assert!(answer.contains("没有直接证据"));
    assert!(answer.contains("不能确认"));
    assert!(answer.contains("紫鹃服侍过史湘云"));
}

#[test]
fn relation_direct_answer_uses_same_block_relation_support() {
    let frame = relation_frame();
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第三回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "紫鵑伏侍史湘雲，日夜不離。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    assert!(relation_review_issues(Some(&frame), &cards).is_empty());
    let answer = relation_direct_answer(Some(&frame), &cards).expect("direct answer");
    assert!(answer.contains("可以确认"));
    assert!(answer.contains("紫鹃服侍过史湘云"));
}
