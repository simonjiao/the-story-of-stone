use super::*;
use crate::EvidenceCard;
use serde_json::json;

fn card(source_id: &str, source_title: &str, evidence_type: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: "ev-test".to_string(),
        evidence_type: evidence_type.to_string(),
        source_id: source_id.to_string(),
        source_title: source_title.to_string(),
        source_url: "https://example.test".to_string(),
        revision_id: Some(1),
        block_id: "block-test".to_string(),
        text: "测试材料".to_string(),
        support_scope: "test".to_string(),
        unsupported_scope: "test".to_string(),
        evidence_level: "test".to_string(),
        confidence: "high".to_string(),
        verification_status: "source_snapshot_ready".to_string(),
    }
}

#[test]
fn default_catalog_builds_pre80_text_and_commentary_search_request() {
    let request = default_full_text_search_requests(
        "人物进府时多大了",
        Some(&json!({
            "canonical_question": "人物进府时多大了",
            "subject": {"canonical": "人物", "aliases": ["此人"]},
            "predicate": {"label": "年龄", "evidence_terms": ["几岁", "年纪"]},
            "required_evidence_types": ["base_text", "commentary"]
        })),
        &json!({"later_forty_allowed": false}),
    )
    .expect("search request")
    .remove(0);

    assert_eq!(
        request.schema_version,
        FULL_TEXT_SEARCH_REQUEST_SCHEMA_VERSION
    );
    assert_eq!(request.request_source, "local_question_frame");
    assert_eq!(
        request.corpus_ids,
        vec!["cheng_120_base_text", "zhi_pre80_base_text_commentary"]
    );
    assert_eq!(
        request.source_layers,
        vec!["base_text_pre_80", "commentary", "version_note"]
    );
    assert_eq!(request.chapter_start, Some(1));
    assert_eq!(request.chapter_end, Some(80));
    assert!(request.search_terms.iter().any(|term| term == "人物"));
    assert!(request.search_terms.iter().any(|term| term == "几岁"));
}

#[test]
fn default_search_request_includes_query_expansion_terms() {
    let request = default_full_text_search_requests(
        "林黛玉结局如何",
        Some(&json!({
            "intent": "character_fate_query",
            "canonical_question": "林黛玉结局如何",
            "subject": {"canonical": "林黛玉", "aliases": ["黛玉", "林姑娘"]},
            "predicate": null,
            "required_evidence_types": ["base_text", "commentary"]
        })),
        &json!({"later_forty_allowed": false}),
    )
    .expect("search request")
    .remove(0);

    assert!(
        request
            .search_terms
            .iter()
            .any(|term| term == "玉帶林中掛" || term == "玉带林中挂")
    );
    assert!(request.search_terms.iter().any(|term| term == "枉凝眉"));
}

#[test]
fn explicit_later_forty_scope_switches_to_later_forty_layers() {
    let request = default_full_text_search_requests(
        "按后四十回说明人物结局",
        None,
        &json!({"later_forty_allowed": true}),
    )
    .expect("search request")
    .remove(0);

    assert_eq!(request.corpus_ids, vec!["cheng_120_base_text"]);
    assert_eq!(
        request.source_layers,
        vec!["base_text_later_40", "version_note"]
    );
    assert_eq!(request.chapter_start, Some(81));
    assert_eq!(request.chapter_end, Some(120));
}

#[test]
fn upstream_repair_request_uses_scope_defaults_for_unknown_corpus_and_layer() {
    let request = full_text_search_requests_from_retrieval_repair_queries(
        "林黛玉进贾府时多大了",
        Some(&json!({
            "canonical_question": "林黛玉进贾府时多大了",
            "subject": {"canonical": "林黛玉", "aliases": ["黛玉"]},
            "predicate": {"label": "年龄", "evidence_terms": ["年纪", "几岁"]},
            "required_evidence_types": ["base_text", "commentary"]
        })),
        &json!({"later_forty_allowed": false}),
        &[json!({
            "query_text": "林黛玉 初进 贾府 年纪 第三回",
            "search_terms": ["林黛玉", "进贾府", "年纪", "第三回"],
            "corpus_ids": ["honglou-main"],
            "source_layers": ["base_text"],
            "chapter_range": {"start": 3, "end": 3},
            "reason": "repair missing age evidence"
        })],
    )
    .expect("repair request")
    .remove(0);

    assert_eq!(request.request_source, "upstream_retrieval_repair");
    assert_eq!(
        request.corpus_ids,
        vec!["cheng_120_base_text", "zhi_pre80_base_text_commentary"]
    );
    assert_eq!(
        request.source_layers,
        vec!["base_text_pre_80", "commentary", "version_note"]
    );
    assert_eq!(request.chapter_start, Some(3));
    assert_eq!(request.chapter_end, Some(3));
}

#[test]
fn upstream_repair_request_cannot_expand_default_scope_into_later_forty() {
    let request = full_text_search_requests_from_retrieval_repair_queries(
        "默认范围问题",
        None,
        &json!({"later_forty_allowed": false}),
        &[json!({
            "query_text": "后四十回材料",
            "search_terms": ["后四十回材料"],
            "corpus_ids": ["cheng_120_base_text"],
            "source_layers": ["base_text_later_40"],
            "chapter_range": {"start": 94, "end": 95}
        })],
    )
    .expect("repair request")
    .remove(0);

    assert_eq!(
        request.source_layers,
        vec!["base_text_pre_80", "commentary", "version_note"]
    );
    assert_eq!(request.chapter_start, Some(1));
    assert_eq!(request.chapter_end, Some(80));
}

#[test]
fn card_match_respects_corpus_layer_and_chapter_scope() {
    let request = default_full_text_search_requests(
        "默认范围问题",
        None,
        &json!({"later_forty_allowed": false}),
    )
    .expect("search request")
    .remove(0);
    let pre80 = card("hongloumeng-wikisource-120", "紅樓夢/第003回", "base_text");
    let commentary = card(
        "shitouji-wikisource-zhiyanzhai",
        "脂硯齋重評石頭記/第005回",
        "commentary",
    );
    let later40 = card("hongloumeng-wikisource-120", "紅樓夢/第095回", "base_text");

    assert!(card_matches_full_text_search_request(&pre80, &request).expect("pre80 match"));
    assert!(card_matches_full_text_search_request(&commentary, &request).expect("commentary"));
    assert!(!card_matches_full_text_search_request(&later40, &request).expect("later40"));
}
