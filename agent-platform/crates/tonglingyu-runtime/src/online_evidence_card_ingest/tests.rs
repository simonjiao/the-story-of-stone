use super::*;
use rusqlite::Connection;
use serde_json::json;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    crate::init_runtime_schema(&conn).expect("runtime schema");
    crate::init_knowledge_base_schema(&conn).expect("knowledge base schema");
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

fn relation_candidate(
    request: &OnlineEvidenceCardUpdateRequestRecord,
    block_id: &str,
    text: &str,
    source_hash: &str,
) -> StageCandidateInput {
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame)
        .expect("frame");
    stage_candidate_from_frame(
        request,
        Some(&frame),
        sample_card(block_id, text),
        source_hash.to_string(),
    )
    .expect("stage candidate")
    .expect("relation candidate")
}

fn set_candidate_entities(input: &mut StageCandidateInput, subject: &str, object: &str) {
    input.entities = canonical_json_value(&json!([
        {"role": "subject", "canonical": subject, "aliases": []},
        {"role": "object", "canonical": object, "aliases": []}
    ]));
    input.entities_key = stable_hash(&input.entities).expect("entities key");
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
fn update_requests_and_stats_are_queryable() {
    let conn = test_conn();
    let request = relation_request(&conn);

    let requests =
        list_online_evidence_card_update_requests_for_trace(&conn, &request.trace_id, 10)
            .expect("requests list");
    let stats = online_evidence_card_ingest_stats(&conn).expect("ingest stats");

    assert_eq!(
        requests[0]["update_request_id"],
        json!(request.update_request_id)
    );
    assert_eq!(requests[0]["status"], json!("queued"));
    assert_eq!(stats["update_requests"]["by_status"]["queued"], json!(1));
    assert_eq!(stats["raw_candidate_count"], json!(0));
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
fn same_claim_from_distinct_spans_merges_without_strength_upgrade() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-claim-a", "A 服侍 B。", "source-hash-a");
    let second = relation_candidate(
        &request,
        "block-claim-b",
        "旁证文字说 A 曾经服侍 B。",
        "source-hash-b",
    );

    let staged = stage_evidence_card_candidate(&conn, first).expect("first staged");
    assert_eq!(staged.status, "staged");
    let merged = stage_evidence_card_candidate(&conn, second).expect("second merged");

    assert_eq!(merged.status, "merged");
    assert_eq!(merged.supporting_spans.len(), 2);
    assert_eq!(merged.evidence_strength, "direct");
    let staged_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM canonical_staged_cards", [], |row| {
            row.get(0)
        })
        .expect("staged count");
    assert_eq!(staged_count, 1);
}

#[test]
fn overlap_claim_merge_keeps_more_complete_canonical_span() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-overlap", "A 服侍 B。", "source-hash-a");
    let second = relation_candidate(
        &request,
        "block-overlap",
        "某段较完整的上下文写明：A 服侍 B。",
        "source-hash-a",
    );

    stage_evidence_card_candidate(&conn, first).expect("first staged");
    let merged = stage_evidence_card_candidate(&conn, second).expect("merged");

    assert_eq!(merged.status, "merged");
    assert_eq!(merged.supporting_spans.len(), 2);
    assert!(merged.evidence.text.contains("较完整的上下文"));
}

#[test]
fn promoted_claim_supersedes_later_candidate_without_duplicate_card() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-promoted-a", "A 服侍 B。", "source-hash-a");
    let staged = stage_evidence_card_candidate(&conn, first).expect("first staged");
    let promoted = validate_and_promote_staged_card(&conn, &staged.staged_card_id)
        .expect("promote")
        .expect("promoted");
    let second = relation_candidate(
        &request,
        "block-promoted-b",
        "另一处材料也写 A 服侍 B。",
        "source-hash-b",
    );
    let superseded = stage_evidence_card_candidate(&conn, second).expect("superseded");

    assert_eq!(superseded.status, "superseded_by_promoted");
    assert_eq!(
        superseded.promoted_evidence_id,
        promoted.promoted_evidence_id
    );
    let promoted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM evidence_cards WHERE package_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("promoted count");
    assert_eq!(promoted_count, 1);
    let events =
        list_online_evidence_card_events_for_trace(&conn, &request.trace_id, 100).expect("events");
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "staged_card_superseded_by_promoted")
    );
}

#[test]
fn promoted_card_is_queryable_without_staged_or_raw_candidates() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame)
        .expect("frame");
    let candidate = stage_candidate_from_frame(
        &request,
        Some(&frame),
        sample_card("block-promoted-search", "A 服侍 B。"),
        "source-hash-a".to_string(),
    )
    .expect("stage candidate")
    .expect("relation candidate");
    let staged = stage_evidence_card_candidate(&conn, candidate).expect("staged");
    let promoted = validate_and_promote_staged_card(&conn, &staged.staged_card_id)
        .expect("promote")
        .expect("promoted");

    let cards = crate::search_evidence(&conn, "服侍", 5, &[]).expect("search evidence");
    assert!(
        cards
            .iter()
            .any(|card| Some(&card.evidence_id) == promoted.promoted_evidence_id.as_ref())
    );
}

#[test]
fn staged_and_raw_candidates_do_not_enter_search_until_promoted() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let staged_candidate =
        relation_candidate(&request, "block-staged-only", "A 服侍 B。", "source-hash-a");
    stage_evidence_card_candidate(&conn, staged_candidate).expect("staged");
    insert_raw_candidate_for_card(
        &conn,
        &request,
        &sample_card("block-raw-only", "A 服侍 B。"),
        "source-hash-b",
        "rule_gap_no_supported_card_assertion",
    )
    .expect("raw candidate");

    let cards = crate::search_evidence(&conn, "服侍", 5, &[]).expect("search evidence");
    assert!(cards.is_empty());
}

#[test]
fn same_span_multiple_slots_remain_distinct_claim_cards() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-multi-slot", "A 服侍 B。", "source-hash-a");
    let mut second =
        relation_candidate(&request, "block-multi-slot", "A 服侍 B。", "source-hash-a");
    second.slot_id = "assist".to_string();

    let first = stage_evidence_card_candidate(&conn, first).expect("first staged");
    let second = stage_evidence_card_candidate(&conn, second).expect("second staged");

    assert_ne!(first.claim_key, second.claim_key);
    assert_eq!(first.exact_span_key, second.exact_span_key);
    let staged_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM canonical_staged_cards", [], |row| {
            row.get(0)
        })
        .expect("staged count");
    assert_eq!(staged_count, 2);
}

#[test]
fn different_source_scopes_remain_separate_claim_cards() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-scope-a", "A 服侍 B。", "source-hash-a");
    let mut second = relation_candidate(&request, "block-scope-b", "A 服侍 B。", "source-hash-b");
    second.source_scope = "alternate_source_scope".to_string();

    let first = stage_evidence_card_candidate(&conn, first).expect("first staged");
    let second = stage_evidence_card_candidate(&conn, second).expect("second staged");

    assert_eq!(first.status, "staged");
    assert_eq!(second.status, "staged");
    assert_ne!(first.claim_key, second.claim_key);
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
fn claim_dimension_conflicts_are_table_driven() {
    for (field, value) in [
        ("polarity", "refutes"),
        ("modality", "indirect_commentary_hint"),
        ("evidence_strength", "clue"),
        ("rules_version", "different-rules-version"),
    ] {
        let conn = test_conn();
        let request = relation_request(&conn);
        let first =
            relation_candidate(&request, "block-dimension-a", "A 服侍 B。", "source-hash-a");
        stage_evidence_card_candidate(&conn, first).expect("first staged");
        let mut second =
            relation_candidate(&request, "block-dimension-b", "A 服侍 B。", "source-hash-b");
        match field {
            "polarity" => second.polarity = value.to_string(),
            "modality" => second.modality = value.to_string(),
            "evidence_strength" => second.evidence_strength = value.to_string(),
            "rules_version" => second.rules_version = value.to_string(),
            _ => unreachable!("covered table field"),
        }
        let conflict = stage_evidence_card_candidate(&conn, second).expect("conflict");
        assert_eq!(conflict.status, "conflicted", "field={field}");
    }
}

#[test]
fn role_conflict_blocks_reversed_entity_roles() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-role-a", "A 服侍 B。", "source-hash-a");
    stage_evidence_card_candidate(&conn, first).expect("first staged");
    let mut second = relation_candidate(&request, "block-role-b", "A 服侍 B。", "source-hash-b");
    set_candidate_entities(&mut second, "B", "A");

    let conflict = stage_evidence_card_candidate(&conn, second).expect("role conflict");

    assert_eq!(conflict.status, "conflicted");
    let events =
        list_online_evidence_card_events_for_trace(&conn, &request.trace_id, 100).expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "staged_card_conflicted" && event["reason_code"] == "role_conflict"
    }));
}

#[test]
fn entity_resolution_gap_blocks_promotion_and_survives_duplicate_merge() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let mut first = relation_candidate(&request, "block-entity-gap", "A 服侍 B。", "source-hash-a");
    set_candidate_entities(&mut first, "A", "");

    let needs_disambiguation =
        stage_evidence_card_candidate(&conn, first).expect("needs disambiguation");
    assert_eq!(needs_disambiguation.status, "needs_disambiguation");
    let promoted = validate_and_promote_staged_card(&conn, &needs_disambiguation.staged_card_id)
        .expect("validate skipped");
    assert_eq!(promoted.expect("record").status, "needs_disambiguation");

    let mut duplicate =
        relation_candidate(&request, "block-entity-gap", "A 服侍 B。", "source-hash-a");
    set_candidate_entities(&mut duplicate, "A", "");
    let merged = stage_evidence_card_candidate(&conn, duplicate).expect("duplicate merge");
    assert_eq!(merged.status, "needs_disambiguation");
}

#[test]
fn source_hash_conflict_blocks_direct_claim_merge() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = relation_candidate(&request, "block-source-hash", "A 服侍 B。", "source-hash-a");
    stage_evidence_card_candidate(&conn, first).expect("first staged");
    let second = relation_candidate(&request, "block-source-hash", "A 服侍 B。", "source-hash-b");

    let conflict = stage_evidence_card_candidate(&conn, second).expect("source hash conflict");

    assert_eq!(conflict.status, "conflicted");
    let events =
        list_online_evidence_card_events_for_trace(&conn, &request.trace_id, 100).expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "staged_card_conflicted"
            && event["reason_code"] == "source_hash_conflict"
    }));
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
