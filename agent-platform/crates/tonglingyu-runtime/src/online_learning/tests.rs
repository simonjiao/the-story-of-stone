use super::*;
use crate::{ClaimKnowledgeItemRef, RuntimeWorkflowOutput, RuntimeWorkflowStepReport};
use rusqlite::Connection;
use serde_json::json;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    crate::init_runtime_schema(&conn).expect("runtime schema");
    crate::init_knowledge_base_schema(&conn).expect("knowledge base schema");
    conn
}

fn sample_card(evidence_id: &str, verification_status: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: evidence_id.to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source-a".to_string(),
        source_title: "紅樓夢/第003回".to_string(),
        source_url: "https://example.test/source-a".to_string(),
        revision_id: Some(1),
        block_id: "block-a".to_string(),
        text: "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string(),
        support_scope: "supports source-bound claim".to_string(),
        unsupported_scope: "does not support unrelated claims".to_string(),
        evidence_level: "source_snapshot".to_string(),
        confidence: "medium".to_string(),
        verification_status: verification_status.to_string(),
    }
}

fn claim_map(evidence_id: &str) -> ClaimEvidenceMap {
    ClaimEvidenceMap {
        claim_index: 0,
        claim: "黛玉进府时尚属幼年".to_string(),
        evidence_ids: vec![evidence_id.to_string()],
        knowledge_item_refs: Vec::<ClaimKnowledgeItemRef>::new(),
        forbidden_conclusions: vec!["不能推出精确生日".to_string()],
    }
}

fn review() -> ReviewRecord {
    ReviewRecord {
        status: "passed".to_string(),
        severity: "none".to_string(),
        issues: Vec::new(),
        summary: "passed".to_string(),
    }
}

fn workflow_with_rejected_draft(package: crate::EvidencePackage) -> RuntimeWorkflowOutput {
    RuntimeWorkflowOutput {
        trace_id: "trace-online-learning-llm-assets".to_string(),
        question: "林黛玉进贾府时多大了".to_string(),
        package,
        draft_answer: "本地草稿".to_string(),
        final_answer: "本地回答".to_string(),
        answer_source: "agent_runtime_openai_compatible_profile_rejected_by_local_governance"
            .to_string(),
        agent_runtime_summary: json!({"mode": "openai_compatible"}),
        steps: vec![RuntimeWorkflowStepReport {
            step_id: "step-draft".to_string(),
            profile: "answer-drafter".to_string(),
            profile_contract_version: "test-contract".to_string(),
            operation: "draft_answer".to_string(),
            status: "ok".to_string(),
            required: true,
            allowed_tools: Vec::new(),
            tool_calls: Vec::new(),
            input_ref: None,
            output_ref: "workflow://trace-online-learning-llm-assets/step-draft".to_string(),
            duration_ms: 42,
            trace_id: "trace-online-learning-llm-assets".to_string(),
            output: json!({
                "agent_runtime_draft_consumed": false,
                "agent_runtime_draft_rejected_reason": "draft_claim_exceeds_evidence_boundary",
                "agent_runtime_result_format": "json",
                "agent_runtime_coverage_status": "partial",
                "agent_runtime_retrieval_repair_recommended": true,
                "agent_runtime_retrieval_repair_query_count": 1,
                "agent_runtime_retrieval_repair_queries": [{
                    "query_text": "林黛玉 初进 贾府 年纪 第三回",
                    "search_terms": ["林黛玉", "进贾府", "年纪", "第三回"],
                    "corpus_ids": ["honglou-main"],
                    "source_layers": ["base_text"],
                    "chapter_range": {"start": 3, "end": 3},
                    "required_evidence_types": ["base_text", "commentary"],
                    "reason": "repair missing age evidence"
                }],
            }),
            agent_runtime: Some(json!({
                "status": "executed",
                "result_ref": "agent-runtime://result/step-draft",
                "provider_request_sha256": "provider-request-sha",
                "content_source": "agent-runtime-openai-compatible-profile-rejected"
            })),
        }],
        stream_events: Vec::new(),
    }
}

#[test]
fn source_snapshot_card_becomes_request_scoped_evidence_binding() {
    let conn = test_conn();
    conn.execute(
        r#"
        INSERT INTO sources (
            source_id, source_category, format, title, work, edition, language,
            source_url, api_url, fetched_at, license, license_url,
            license_source_url, attribution, usage_boundary, notes,
            snapshot_contract_json, source_hash
        ) VALUES (
            'source-a', 'base_material', 'mediawiki', 'Source A', '红楼梦',
            '120回', 'zh', 'https://example.test/source-a', NULL,
            '2026-01-01T00:00:00Z', 'test-license', NULL, NULL,
            'test attribution', 'test usage boundary', 'test notes', '{}',
            'source-hash-a'
        )
        "#,
        [],
    )
    .expect("insert source");
    let card = sample_card(
        "ev-source-snapshot",
        "source_snapshot_ready_not_scholarly_collated",
    );
    let summary = build_online_learning_trace_summary(
        &conn,
        "pkg-a",
        "trace-a",
        std::slice::from_ref(&card),
        &[claim_map(&card.evidence_id)],
        &json!({"later_forty_allowed": false}),
        &review(),
    )
    .expect("trace summary");

    assert_eq!(summary.schema_version, ONLINE_LEARNING_TRACE_SCHEMA_VERSION);
    assert_eq!(summary.tiered_evidence_bindings.len(), 1);
    let binding = &summary.tiered_evidence_bindings[0];
    assert_eq!(binding.evidence_tier, "request_scoped_evidence");
    assert_eq!(binding.answer_use, "request_bound_basis");
    assert_eq!(binding.source_hash, "source-hash-a");
    assert_eq!(
        binding.source_span_ref["source_hash_status"],
        json!("source_snapshot_hash")
    );
    assert_eq!(
        binding.evidence_gate["request_scoped_evidence_ready"],
        json!(true)
    );
    assert_eq!(binding.evidence_gate["status"], json!("passed"));
    assert_eq!(binding.evidence_gate["missing_required_fields"], json!([]));
    assert_eq!(
        binding.evidence_gate["decision_evidence_tier"],
        json!("request_scoped_evidence")
    );
    assert!(binding.admin_only);
    assert_eq!(binding.review_status, "passed");
    assert_eq!(
        binding.claim_binding["forbidden_conclusions"],
        json!(["不能推出精确生日"])
    );
}

#[test]
fn source_snapshot_card_without_source_hash_stays_raw_full_text_hit() {
    let conn = test_conn();
    let card = sample_card(
        "ev-source-snapshot-without-hash",
        "source_snapshot_ready_not_scholarly_collated",
    );
    let summary = build_online_learning_trace_summary(
        &conn,
        "pkg-raw-hit",
        "trace-raw-hit",
        std::slice::from_ref(&card),
        &[claim_map(&card.evidence_id)],
        &json!({"later_forty_allowed": false}),
        &review(),
    )
    .expect("trace summary");

    let binding = &summary.tiered_evidence_bindings[0];
    assert_eq!(binding.evidence_tier, "request_raw_full_text_hit");
    assert_eq!(binding.answer_use, "supplemental_only");
    assert_eq!(
        binding.source_span_ref["source_hash_status"],
        json!("derived_fallback_hash")
    );
    assert_eq!(
        binding.evidence_gate["request_scoped_evidence_ready"],
        json!(false)
    );
    assert_eq!(binding.evidence_gate["status"], json!("downgraded"));
    assert_eq!(
        binding.evidence_gate["decision_evidence_tier"],
        json!("request_raw_full_text_hit")
    );
    assert!(
        binding.evidence_gate["missing_required_fields"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some("source_hash")))
    );
}

#[test]
fn promoted_card_keeps_stable_evidence_tier() {
    let conn = test_conn();
    let card = sample_card("evc-promoted-card", "online_promoted_source_backed");
    let summary = build_online_learning_trace_summary(
        &conn,
        "pkg-promoted",
        "trace-promoted",
        std::slice::from_ref(&card),
        &[claim_map(&card.evidence_id)],
        &json!({"later_forty_allowed": false}),
        &review(),
    )
    .expect("trace summary");

    let binding = &summary.tiered_evidence_bindings[0];
    assert_eq!(binding.evidence_tier, "promoted_evidence_card");
    assert_eq!(binding.answer_use, "stable_basis");
}

#[test]
fn records_candidate_refs_for_online_evidence_update_request() {
    let conn = test_conn();
    let update_request = crate::create_online_evidence_card_update_request(
        &conn,
        crate::OnlineEvidenceCardUpdateRequestInput {
            trace_id: "trace-online-learning-candidates".to_string(),
            session_id: Some("session-a".to_string()),
            resolved_question: "林黛玉进贾府时多大了".to_string(),
            question_frame: Some(json!({
                "intent": "attribute_at_event",
                "canonical_question": "林黛玉进贾府时多大了",
                "subject": {"canonical": "林黛玉", "aliases": ["黛玉"]},
                "predicate": {
                    "id": "age",
                    "label": "年龄",
                    "aliases": ["多大"],
                    "evidence_terms": ["年方", "岁"]
                },
                "object": null,
                "required_evidence_types": ["base_text", "commentary"]
            })),
            coverage_gap_reason: "package_coverage_gap:review:insufficient".to_string(),
            source_scope_policy: json!({"later_forty_allowed": false}),
            recall_advice_ref: None,
        },
    )
    .expect("update request");
    let card = sample_card(
        "ev-source-snapshot",
        "source_snapshot_ready_not_scholarly_collated",
    );
    let trace_summary = build_online_learning_trace_summary(
        &conn,
        "pkg-online-learning-candidates",
        "trace-online-learning-candidates",
        std::slice::from_ref(&card),
        &[claim_map(&card.evidence_id)],
        &json!({"later_forty_allowed": false}),
        &review(),
    )
    .expect("trace summary");

    let payload = record_online_learning_candidate_refs(
        &conn,
        "trace-online-learning-candidates",
        Some(&trace_summary),
        Some(&update_request),
    )
    .expect("candidate refs")
    .expect("payload");

    assert_eq!(
        payload["candidate_ids"]["evidence"]["update_request_id"],
        json!(update_request.update_request_id)
    );
    assert!(
        payload["candidate_ids"]["evidence"]["search_request_ids"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(
        payload["candidate_ids"]["evidence"]["worker_job_ids"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    let events = crate::runtime_audit_events_for_trace(&conn, "trace-online-learning-candidates")
        .expect("audit events");
    assert!(
        events.iter().any(|event| {
            event["event_type"] == json!("online_learning_candidate_refs_recorded")
        })
    );
}

#[test]
fn records_llm_search_advice_and_prompt_candidate_assets() {
    let conn = test_conn();
    let card = sample_card(
        "ev-source-snapshot",
        "source_snapshot_ready_not_scholarly_collated",
    );
    let package = crate::create_evidence_package(
        &conn,
        "trace-online-learning-llm-assets",
        "林黛玉进贾府时多大了",
        vec![card],
    )
    .expect("package");
    let workflow = workflow_with_rejected_draft(package);
    let repair_queries = workflow.steps[0]
        .output
        .get("agent_runtime_retrieval_repair_queries")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("repair queries");
    let inserted = crate::add_online_evidence_retrieval_repair_search_requests(
        &conn,
        &workflow.trace_id,
        &workflow.question,
        workflow.package.question_frame.clone(),
        json!({"later_forty_allowed": false}),
        &repair_queries,
    )
    .expect("search requests");

    let payload = record_agent_runtime_online_learning_assets(
        &conn,
        &workflow,
        "openai_compatible",
        inserted,
    )
    .expect("llm assets")
    .expect("payload");

    assert_eq!(
        payload["llm_semantic_parse_ref"],
        json!("agent-runtime://result/step-draft")
    );
    assert_eq!(
        payload["failure_patterns"],
        json!(["draft_rejected:draft_claim_exceeds_evidence_boundary"])
    );
    assert_eq!(payload["retrieval_repair_search_request_count"], json!(1));
    assert!(
        payload["candidate_ids"]["prompt"]
            .as_array()
            .is_some_and(|items| items.len() == 1)
    );
    assert!(
        payload["persisted_full_text_search_request_ids"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    let prompt_candidates = list_online_learning_prompt_candidates_for_trace(
        &conn,
        "trace-online-learning-llm-assets",
        10,
    )
    .expect("prompt candidates");
    assert_eq!(prompt_candidates.len(), 1);
    assert_eq!(
        prompt_candidates[0]["failure_pattern"],
        json!("draft_rejected:draft_claim_exceeds_evidence_boundary")
    );
    assert_eq!(prompt_candidates[0]["status"], json!("staged"));
    let events = crate::runtime_audit_events_for_trace(&conn, "trace-online-learning-llm-assets")
        .expect("events");
    assert!(
        events
            .iter()
            .any(|event| { event["event_type"] == json!("online_learning_llm_assets_recorded") })
    );
}

#[test]
fn records_review_observation_prompt_candidate_assets() {
    let conn = test_conn();
    let card = sample_card(
        "ev-source-snapshot-review-prompt",
        "source_snapshot_ready_not_scholarly_collated",
    );
    let package = crate::create_evidence_package(
        &conn,
        "trace-online-learning-review-prompt",
        "林黛玉进贾府时多大了",
        vec![card],
    )
    .expect("package");
    let mut workflow = workflow_with_rejected_draft(package);
    workflow.trace_id = "trace-online-learning-review-prompt".to_string();
    workflow.steps.push(RuntimeWorkflowStepReport {
        step_id: "step-review".to_string(),
        profile: "honglou-reviewer".to_string(),
        profile_contract_version: "test-contract".to_string(),
        operation: "review_answer".to_string(),
        status: "ok".to_string(),
        required: true,
        allowed_tools: Vec::new(),
        tool_calls: Vec::new(),
        input_ref: None,
        output_ref: "workflow://trace-online-learning-review-prompt/step-review".to_string(),
        duration_ms: 12,
        trace_id: "trace-online-learning-review-prompt".to_string(),
        output: json!({
            "agent_runtime_review_rejected_reason": "unsupported_json_review",
        }),
        agent_runtime: Some(json!({
            "status": "executed",
            "result_ref": "agent-runtime://result/step-review",
            "provider_request_sha256": "provider-request-review-sha",
            "content_source": "agent-runtime-openai-compatible-review-rejected"
        })),
    });

    let payload =
        record_agent_runtime_online_learning_assets(&conn, &workflow, "openai_compatible", 0)
            .expect("llm assets")
            .expect("payload");

    assert!(
        payload["failure_patterns"]
            .as_array()
            .is_some_and(|patterns| patterns.iter().any(|pattern| pattern.as_str()
                == Some("review_observation_rejected:unsupported_json_review")))
    );
    let prompt_candidates = list_online_learning_prompt_candidates_for_trace(
        &conn,
        "trace-online-learning-review-prompt",
        10,
    )
    .expect("prompt candidates");
    assert!(
        prompt_candidates
            .iter()
            .any(|candidate| candidate["operation"] == json!("review_answer"))
    );
}

#[test]
fn records_oversized_prompt_failure_candidate_from_runtime_rejection() {
    let conn = test_conn();
    let card = sample_card(
        "ev-source-snapshot-oversized-prompt",
        "source_snapshot_ready_not_scholarly_collated",
    );
    let package = crate::create_evidence_package(
        &conn,
        "trace-online-learning-oversized-prompt",
        "林黛玉进贾府时多大了",
        vec![card],
    )
    .expect("package");
    let mut workflow = workflow_with_rejected_draft(package);
    workflow.trace_id = "trace-online-learning-oversized-prompt".to_string();
    workflow.steps[0].agent_runtime = None;
    let error = anyhow::anyhow!(
        "runtime profile message exceeded safety budget: step_id=step-draft operation=draft_answer bytes=9000 limit=8192"
    );

    let candidate_id = record_agent_runtime_prompt_failure_candidate(
        &conn,
        &workflow,
        "agent_runtime_step_execution",
        &error,
    )
    .expect("prompt failure candidate")
    .expect("candidate id");

    assert!(candidate_id.starts_with("prompt-candidate-"));
    let prompt_candidates = list_online_learning_prompt_candidates_for_trace(
        &conn,
        "trace-online-learning-oversized-prompt",
        10,
    )
    .expect("prompt candidates");
    assert_eq!(prompt_candidates.len(), 1);
    assert_eq!(
        prompt_candidates[0]["failure_pattern"],
        json!("oversized_prompt:agent_runtime_step_execution")
    );
    assert_eq!(
        prompt_candidates[0]["observation"]["source_ref"]["error_sha256"]
            .as_str()
            .map(str::len),
        Some(64)
    );
}
