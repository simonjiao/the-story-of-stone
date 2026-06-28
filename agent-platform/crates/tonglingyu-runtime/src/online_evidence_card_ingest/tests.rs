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

fn attribute_age_request(conn: &Connection) -> OnlineEvidenceCardUpdateRequestRecord {
    create_online_evidence_card_update_request(
        conn,
        OnlineEvidenceCardUpdateRequestInput {
            trace_id: "trace-online-attribute-age-test".to_string(),
            session_id: Some("session-attribute-age".to_string()),
            resolved_question: "林黛玉进贾府时多大了".to_string(),
            question_frame: Some(json!({
                "intent": "attribute_at_event",
                "canonical_question": "林黛玉进贾府时多大了",
                "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
                "predicate": {
                    "id": "age",
                    "label": "年龄",
                    "aliases": ["年龄", "几岁", "多大"],
                    "evidence_terms": ["岁", "歲", "年纪", "年紀", "年方"]
                },
                "object": null,
                "required_evidence_types": ["base_text", "commentary"]
            })),
            coverage_gap_reason: "coverage_partial".to_string(),
            source_scope_policy: json!({
                "later_forty_allowed": false,
                "allowed_source_layers": ["base_text_pre_80", "commentary", "version_note"]
            }),
            recall_advice_ref: None,
        },
    )
    .expect("attribute request created")
}

fn job_for_request(
    conn: &Connection,
    request: &OnlineEvidenceCardUpdateRequestRecord,
) -> CardIngestJobRecord {
    load_card_ingest_job_by_request(conn, &request.update_request_id)
        .expect("load job")
        .expect("job exists")
}

fn insert_source_block(conn: &Connection, block_id: &str, text: &str) {
    insert_source_block_with_hash(
        conn,
        "hongloumeng-wikisource-120",
        "source-hash-runtime-test",
        block_id,
        text,
    );
}

fn insert_source_block_with_hash(
    conn: &Connection,
    source_id: &str,
    source_hash: &str,
    block_id: &str,
    text: &str,
) {
    conn.execute(
        r#"
        INSERT OR IGNORE INTO sources (
            source_id, source_category, format, title, work, edition, language,
            source_url, api_url, fetched_at, license, license_url,
            license_source_url, attribution, usage_boundary, notes,
            snapshot_contract_json, source_hash
        ) VALUES (
            ?1, 'base_material', 'mediawiki',
            '红楼梦 维基文库 120回', '红楼梦', '120回', 'zh',
            'https://example.test/source', NULL, '2026-01-01T00:00:00Z',
            'CC-BY-SA-4.0', 'https://example.test/license', NULL,
            'test attribution', 'test usage boundary', 'test notes',
            '{}', ?2
        )
        "#,
        rusqlite::params![source_id, source_hash],
    )
    .expect("insert source");
    let normalized_text = crate::normalize_text(text);
    conn.execute(
        r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, ?2, 'section-test',
                  '紅樓夢/第003回', '红楼梦/第003回',
                  'https://example.test/block', 1, 1, 'paragraph', NULL,
                  ?3, ?4, 'base_text', 3)
        "#,
        rusqlite::params![block_id, source_id, text, normalized_text],
    )
    .expect("insert block");
    conn.execute(
        "INSERT INTO blocks_fts (block_id, source_id, source_title, text, normalized_text) VALUES (?1, ?2, '紅樓夢/第003回', ?3, ?4)",
        rusqlite::params![block_id, source_id, text, normalized_text],
    )
    .expect("insert block fts");
}

#[test]
fn creates_update_request_idempotently() {
    let conn = test_conn();
    let first = relation_request(&conn);
    let second = relation_request(&conn);

    assert_eq!(first.update_request_id, second.update_request_id);
    assert_eq!(second.status, "queued");
    let jobs = list_online_evidence_card_jobs_for_trace(&conn, &first.trace_id, 10)
        .expect("jobs for trace");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, "queued");
    let search_requests =
        list_online_evidence_card_search_requests_for_trace(&conn, &first.trace_id, 10)
            .expect("search requests");
    assert_eq!(search_requests.len(), 1);
    assert_eq!(search_requests[0].status, "queued");
    assert_eq!(search_requests[0].request_source, "local_question_frame");
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
    assert_eq!(stats["search_requests"]["by_status"]["queued"], json!(1));
    assert_eq!(stats["jobs"]["by_status"]["queued"], json!(1));
    assert_eq!(stats["raw_candidate_count"], json!(0));
}

#[test]
fn worker_executes_persisted_full_text_search_request_before_staging() {
    let conn = test_conn();
    let request = relation_request(&conn);
    insert_source_block(&conn, "block-full-text-supported", "A 服侍 B。");

    let report = run_online_evidence_card_worker_once(
        &conn,
        OnlineEvidenceCardWorkerRunInput {
            actor: "worker-a".to_string(),
            limit: 10,
            retrieval_limit: 5,
        },
    )
    .expect("worker run");

    assert_eq!(report.processed_count, 1);
    assert_eq!(report.search_request_count, 1);
    assert_eq!(report.staged_count, 1);
    assert_eq!(report.promoted_count, 1);
    let search_requests =
        list_online_evidence_card_search_requests_for_trace(&conn, &request.trace_id, 10)
            .expect("search requests");
    assert_eq!(search_requests[0].status, "completed");
    assert_eq!(search_requests[0].result_count, 1);
    let events =
        list_online_evidence_card_events_for_trace(&conn, &request.trace_id, 100).expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "online_evidence_search_request_completed"
            && event["payload"]["result_count"] == json!(1)
    }));
}

#[test]
fn worker_promotes_attribute_age_card_from_full_text_search() {
    let conn = test_conn();
    let request = attribute_age_request(&conn);
    insert_source_block(
        &conn,
        "block-attribute-age",
        "今只有嫡妻賈氏，生得一女，乳名黛玉，年方五歲。",
    );

    let report = run_online_evidence_card_worker_once(
        &conn,
        OnlineEvidenceCardWorkerRunInput {
            actor: "worker-a".to_string(),
            limit: 10,
            retrieval_limit: 5,
        },
    )
    .expect("worker run");

    assert_eq!(report.processed_count, 1);
    assert_eq!(report.staged_count, 1);
    assert_eq!(report.promoted_count, 1);
    assert_eq!(report.raw_candidate_count, 0);
    let staged = list_online_evidence_card_staged_for_trace(&conn, &request.trace_id, 10)
        .expect("staged cards");
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].slot_id, "age");
    assert_eq!(staged[0].status, "promoted");
    assert!(staged[0].entities.to_string().contains("5岁"));

    let cards = crate::search_evidence(&conn, "黛玉 年方五歲", 5, &[])
        .expect("search promoted age evidence");
    assert!(cards.iter().any(|card| {
        card.verification_status == "online_promoted_source_backed"
            && card.text.contains("年方五歲")
    }));
}

#[test]
fn job_lease_heartbeat_and_expired_reconcile() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let jobs = lease_card_ingest_jobs(&conn, "worker-a", 1).expect("lease job");

    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, "processing");
    assert_eq!(jobs[0].leased_by.as_deref(), Some("worker-a"));
    assert_eq!(jobs[0].attempt_count, 1);
    assert!(heartbeat_card_ingest_job(&conn, &jobs[0].job_id, "worker-a").expect("heartbeat"));

    conn.execute(
        "UPDATE card_ingest_jobs SET lease_until = '1970-01-01T00:00:00Z' WHERE job_id = ?1",
        [&jobs[0].job_id],
    )
    .expect("expire lease");
    let repaired = reconcile_card_ingest_jobs(&conn).expect("reconcile");
    let recovered = job_for_request(&conn, &request);

    assert_eq!(repaired, 1);
    assert_eq!(recovered.status, "queued");
    assert_eq!(recovered.stage, "lease_expired");
    assert!(recovered.leased_by.is_none());
    let request_after =
        load_update_request(&conn, &request.update_request_id).expect("request reload");
    assert_eq!(request_after.expect("request").status, "queued");
}

#[test]
fn job_failure_retries_then_dead_letters() {
    let conn = test_conn();
    let request = relation_request(&conn);

    for expected_attempt in 1..=CARD_INGEST_JOB_MAX_ATTEMPTS {
        let leased = lease_card_ingest_jobs(&conn, "worker-a", 1).expect("lease");
        assert_eq!(leased.len(), 1);
        assert_eq!(leased[0].attempt_count, expected_attempt);
        fail_card_ingest_job(&conn, &leased[0], "synthetic worker failure").expect("fail job");
        let failed = job_for_request(&conn, &request);
        if expected_attempt < CARD_INGEST_JOB_MAX_ATTEMPTS {
            assert_eq!(failed.status, "retry_wait");
            assert!(
                failed
                    .last_error
                    .as_deref()
                    .is_some_and(|error| error.contains("synthetic worker failure"))
            );
            conn.execute(
                "UPDATE card_ingest_jobs SET next_run_at = '1970-01-01T00:00:00Z' WHERE job_id = ?1",
                [&failed.job_id],
            )
            .expect("make retry ready");
            reconcile_card_ingest_jobs(&conn).expect("retry reconcile");
            assert_eq!(job_for_request(&conn, &request).status, "queued");
        } else {
            assert_eq!(failed.status, "dead_letter");
            let request_after =
                load_update_request(&conn, &request.update_request_id).expect("request reload");
            assert_eq!(request_after.expect("request").status, "failed");
        }
    }
}

#[test]
fn reconciler_recovers_promote_failed_retry_job() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let job = job_for_request(&conn, &request);
    conn.execute(
        r#"
        UPDATE card_ingest_jobs
        SET status = 'retry_wait',
            stage = 'promote_failed',
            attempt_count = 1,
            next_run_at = '1970-01-01T00:00:00Z',
            last_error = 'promote failed'
        WHERE job_id = ?1
        "#,
        [&job.job_id],
    )
    .expect("set promote failed retry");

    let repaired = reconcile_card_ingest_jobs(&conn).expect("reconcile");
    let recovered = job_for_request(&conn, &request);

    assert_eq!(repaired, 1);
    assert_eq!(recovered.status, "queued");
    assert_eq!(recovered.stage, "retry_ready");
}

#[test]
fn reconciler_recreates_missing_job_for_queued_request() {
    let conn = test_conn();
    let request = relation_request(&conn);
    conn.execute(
        "DELETE FROM card_ingest_jobs WHERE update_request_id = ?1",
        [&request.update_request_id],
    )
    .expect("delete job");

    let repaired = reconcile_card_ingest_jobs(&conn).expect("reconcile");

    assert_eq!(repaired, 1);
    assert_eq!(job_for_request(&conn, &request).status, "queued");
}

#[test]
fn completed_job_replay_does_not_reprocess_request() {
    let conn = test_conn();
    relation_request(&conn);
    let first = run_online_evidence_card_worker_once(
        &conn,
        OnlineEvidenceCardWorkerRunInput {
            actor: "worker-a".to_string(),
            limit: 10,
            retrieval_limit: 5,
        },
    )
    .expect("first worker run");
    let second = run_online_evidence_card_worker_once(
        &conn,
        OnlineEvidenceCardWorkerRunInput {
            actor: "worker-a".to_string(),
            limit: 10,
            retrieval_limit: 5,
        },
    )
    .expect("second worker run");

    assert_eq!(first.processed_count, 1);
    assert_eq!(second.processed_count, 0);
    let stats = online_evidence_card_ingest_stats(&conn).expect("stats");
    assert_eq!(stats["jobs"]["by_status"]["completed"], json!(1));
    let events = list_online_evidence_card_events_for_trace(&conn, "trace-online-card-test", 100)
        .expect("events");
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "card_ingest_job_leased")
    );
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "card_ingest_job_completed")
    );
}

#[test]
fn retrieval_repair_search_requests_requeue_completed_job() {
    let conn = test_conn();
    let request = relation_request(&conn);
    let first = run_online_evidence_card_worker_once(
        &conn,
        OnlineEvidenceCardWorkerRunInput {
            actor: "worker-a".to_string(),
            limit: 10,
            retrieval_limit: 5,
        },
    )
    .expect("first worker run");
    assert_eq!(first.processed_count, 1);
    assert_eq!(job_for_request(&conn, &request).status, "completed");

    insert_source_block(&conn, "block-repair-supported", "A 服侍 B。");
    let inserted = add_online_evidence_retrieval_repair_search_requests(
        &conn,
        &request.trace_id,
        &request.resolved_question,
        request.question_frame.clone(),
        request.source_scope_policy.clone(),
        &[json!({
            "query_text": "A 服侍 B",
            "search_terms": ["A", "服侍", "B"],
            "corpus_ids": ["honglou-main"],
            "source_layers": ["base_text"],
            "chapter_range": {"start": 3, "end": 3},
            "reason": "repair local coverage"
        })],
    )
    .expect("add repair request");
    assert_eq!(inserted, 1);
    let requeued = job_for_request(&conn, &request);
    assert_eq!(requeued.status, "queued");
    assert_eq!(requeued.stage, "new_search_requests_queued");
    assert_eq!(requeued.attempt_count, 0);

    let report = run_online_evidence_card_worker_once(
        &conn,
        OnlineEvidenceCardWorkerRunInput {
            actor: "worker-a".to_string(),
            limit: 10,
            retrieval_limit: 5,
        },
    )
    .expect("second worker run");

    assert_eq!(report.processed_count, 1);
    assert_eq!(report.search_request_count, 1);
    assert_eq!(report.staged_count, 1);
    assert_eq!(report.promoted_count, 1);
    let search_requests =
        list_online_evidence_card_search_requests_for_trace(&conn, &request.trace_id, 10)
            .expect("search requests");
    assert_eq!(search_requests.len(), 2);
    assert_eq!(search_requests[0].status, "completed");
    assert_eq!(
        search_requests[1].request_source,
        "upstream_retrieval_repair"
    );
    assert_eq!(search_requests[1].status, "completed");
    assert_eq!(
        search_requests[1].request["corpus_ids"],
        json!(["cheng_120_base_text", "zhi_pre80_base_text_commentary"])
    );
    assert_eq!(search_requests[1].result_count, 1);
    let events =
        list_online_evidence_card_events_for_trace(&conn, &request.trace_id, 100).expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "card_ingest_job_requeued"
            && event["reason_code"] == "new_search_requests_queued"
    }));
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

    let manifests =
        crate::list_online_learning_promotion_manifests_for_trace(&conn, &request.trace_id, 10)
            .expect("promotion manifests");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0]["artifact_kind"], json!("evidence"));
    assert_eq!(manifests[0]["status"], json!("passed"));
    assert_eq!(
        manifests[0]["candidate_ids"],
        json!([staged.staged_card_id])
    );
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
fn staged_card_rehydrates_from_source_span_before_package_use() {
    let conn = test_conn();
    let request = relation_request(&conn);
    insert_source_block_with_hash(
        &conn,
        "source-a",
        "source-hash-a",
        "block-staged-rehydrate",
        "A 服侍 B。",
    );
    let staged_candidate = relation_candidate(
        &request,
        "block-staged-rehydrate",
        "A 服侍 B。",
        "source-hash-a",
    );
    let staged = stage_evidence_card_candidate(&conn, staged_candidate).expect("staged");
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame)
        .expect("frame");

    let hits = rehydrate_staged_source_hits_for_frame(
        &conn,
        "trace-rehydrate-later-request",
        Some(&frame),
        5,
    )
    .expect("rehydrated hits");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].staged_card_id, staged.staged_card_id);
    assert_eq!(hits[0].source_hash, "source-hash-a");
    assert_eq!(hits[0].source_span_ref["status"], json!("hash_verified"));
    assert_eq!(
        hits[0].card.verification_status,
        "rehydrated_staged_source_backed"
    );
    let package = crate::create_evidence_package(
        &conn,
        "trace-rehydrate-later-request",
        "A 是否服侍 B",
        hits.into_iter().map(|hit| hit.card).collect(),
    )
    .expect("package");
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_tier,
        "request_scoped_evidence"
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].source_hash,
        "source-hash-a"
    );
}

#[test]
fn staged_card_rehydration_rejects_source_hash_mismatch() {
    let conn = test_conn();
    let request = relation_request(&conn);
    insert_source_block_with_hash(
        &conn,
        "source-a",
        "source-hash-current",
        "block-staged-hash-mismatch",
        "A 服侍 B。",
    );
    let staged_candidate = relation_candidate(
        &request,
        "block-staged-hash-mismatch",
        "A 服侍 B。",
        "source-hash-staged",
    );
    let staged = stage_evidence_card_candidate(&conn, staged_candidate).expect("staged");
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame)
        .expect("frame");

    let hits = rehydrate_staged_source_hits_for_frame(
        &conn,
        "trace-rehydrate-hash-mismatch",
        Some(&frame),
        5,
    )
    .expect("rehydrated hits");

    assert!(hits.is_empty());
    let events =
        list_online_evidence_card_events_for_trace(&conn, "trace-rehydrate-hash-mismatch", 20)
            .expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"].as_str() == Some("staged_card_rehydration_rejected")
            && event["staged_card_id"].as_str() == Some(staged.staged_card_id.as_str())
            && event["reason_code"].as_str() == Some("source_hash_mismatch")
    }));
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
