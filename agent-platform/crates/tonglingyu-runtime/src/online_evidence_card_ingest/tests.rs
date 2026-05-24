use super::*;
use rusqlite::Connection;
use serde_json::json;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    crate::init_runtime_schema(&conn).expect("runtime schema");
    conn
}

fn sample_card(block_id: &str, text: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: format!("ev-{block_id}"),
        evidence_type: "base_text".to_string(),
        source_id: "source-a".to_string(),
        source_title: "Source A".to_string(),
        source_url: "https://example.test/source-a".to_string(),
        revision_id: Some(1),
        block_id: block_id.to_string(),
        text: text.to_string(),
        support_scope: "supports direct local source span".to_string(),
        unsupported_scope: "does not support unrelated claims".to_string(),
        evidence_level: "source_snapshot".to_string(),
        confidence: "medium".to_string(),
        verification_status: "source_snapshot_ready".to_string(),
    }
}

fn relation_request(conn: &Connection) -> OnlineEvidenceCardUpdateRequestRecord {
    create_online_evidence_card_update_request(
        conn,
        OnlineEvidenceCardUpdateRequestInput {
            trace_id: "trace-online-card-test".to_string(),
            session_id: Some("session-a".to_string()),
            resolved_question: "A 是否服侍 B".to_string(),
            question_frame: Some(json!({
                "intent": "relation_query",
                "canonical_question": "A 是否服侍 B",
                "subject": {"canonical": "A", "aliases": []},
                "predicate": {
                    "id": "serve",
                    "label": "服侍",
                    "aliases": ["服侍"],
                    "evidence_terms": ["服侍"]
                },
                "object": {"canonical": "B", "aliases": []},
                "required_evidence_types": ["base_text"]
            })),
            coverage_gap_reason: "coverage_partial".to_string(),
            source_scope_policy: json!({"scope": "test"}),
            recall_advice_ref: None,
        },
    )
    .expect("request created")
}

#[test]
fn creates_update_request_idempotently() {
    let conn = test_conn();
    let first = relation_request(&conn);
    let second = relation_request(&conn);

    assert_eq!(first.update_request_id, second.update_request_id);
    assert_eq!(second.status, "queued");
}

#[test]
fn stages_validates_and_promotes_supported_relation_card() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let card = sample_card("block-1", "A 曾经服侍 B，众人皆知。");
    let candidate = stage_candidate_from_frame(
        &request,
        request
            .question_frame
            .as_ref()
            .and_then(question_frame::parse_runtime_question_frame)
            .as_ref(),
        card,
        "source-hash-a".to_string(),
    )
    .expect("stage candidate")
    .expect("relation candidate");

    let staged = stage_evidence_card_candidate(&conn, candidate).expect("staged");
    assert_eq!(staged.status, "staged");

    let promoted = validate_and_promote_staged_card(&conn, &staged.staged_card_id)
        .expect("promote")
        .expect("promoted record");
    assert_eq!(promoted.status, "promoted");
    assert!(promoted.promoted_evidence_id.is_some());

    let promoted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM evidence_cards WHERE package_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("promoted count");
    assert_eq!(promoted_count, 1);
}

#[test]
fn repeated_candidate_merges_without_duplicate_promoted_card() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame)
        .expect("frame");
    let card = sample_card("block-1", "A 服侍 B。");

    for _ in 0..2 {
        let candidate = stage_candidate_from_frame(
            &request,
            Some(&frame),
            card.clone(),
            "source-hash-a".to_string(),
        )
        .expect("stage candidate")
        .expect("relation candidate");
        let staged = stage_evidence_card_candidate(&conn, candidate).expect("staged");
        validate_and_promote_staged_card(&conn, &staged.staged_card_id).expect("promote");
    }

    let staged_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM canonical_staged_cards", [], |row| {
            row.get(0)
        })
        .expect("staged count");
    let promoted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM evidence_cards WHERE package_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("promoted count");
    assert_eq!(staged_count, 1);
    assert_eq!(promoted_count, 1);
}

#[test]
fn conflicting_claim_dimension_blocks_promotion() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame)
        .expect("frame");
    let first = stage_candidate_from_frame(
        &request,
        Some(&frame),
        sample_card("block-1", "A 服侍 B。"),
        "source-hash-a".to_string(),
    )
    .expect("first")
    .expect("first candidate");
    let staged = stage_evidence_card_candidate(&conn, first).expect("staged");
    validate_and_promote_staged_card(&conn, &staged.staged_card_id).expect("promote");

    let mut conflicting = stage_candidate_from_frame(
        &request,
        Some(&frame),
        sample_card("block-2", "A 服侍 B。"),
        "source-hash-a".to_string(),
    )
    .expect("conflicting")
    .expect("conflicting candidate");
    conflicting.modality = "indirect_commentary_hint".to_string();
    let conflict = stage_evidence_card_candidate(&conn, conflicting).expect("conflict");

    assert_eq!(conflict.status, "conflicted");
    let promoted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM canonical_staged_cards WHERE status = 'promoted'",
            [],
            |row| row.get(0),
        )
        .expect("promoted staged count");
    assert_eq!(promoted_count, 1);
}

#[test]
fn rule_gap_records_raw_candidate_without_package_card() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let card = sample_card("block-raw", "A 与 B 同在一处。");
    insert_raw_candidate_for_card(
        &conn,
        &request,
        &card,
        "source-hash-a",
        "rule_gap_no_supported_card_assertion",
    )
    .expect("raw candidate");

    let raw = list_online_evidence_card_raw_candidates_for_trace(&conn, &request.trace_id, 10)
        .expect("raw candidates");
    let staged = list_online_evidence_card_staged_for_trace(&conn, &request.trace_id, 10)
        .expect("staged cards");
    assert_eq!(raw.len(), 1);
    assert!(staged.is_empty());
}
