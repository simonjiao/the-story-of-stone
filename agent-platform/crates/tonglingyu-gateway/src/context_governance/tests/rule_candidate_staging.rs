use rusqlite::Connection;
use serde_json::{Value, json};

use super::{
    ContextMessage, ContextRequestInput, FakeRuntimeClient,
    create_context_for_request_with_agent_runtime_and_modes, file_conn, load_trace_context,
    read_rule_candidate_for_review, remove_file_db, run_rule_candidate_preflight_check,
    run_rule_candidate_promotion, temp_context_db_path,
};
use crate::{
    context_governance::RESOLVER_SCHEMA_VERSION,
    context_rules,
    llm_contracts::CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
    llm_modes::LlmMode,
    question_frame,
    rule_candidates::{
        RuleCandidatePromotionInput, RuleCandidatePromotionPaths,
        RuleCandidateRegressionEvidenceInput, record_rule_candidate_regression_evidence,
    },
};
use tonglingyu_runtime::RuntimeRuleCandidatePromotionPaths;

#[tokio::test]
async fn accepted_question_normalizer_rule_candidates_are_staged_not_active() {
    let db_path = temp_context_db_path("rule-candidate-staging");
    let conn = file_conn(&db_path);
    drop(conn);
    let resolved_question = "史湘云的结局呢？";
    let question_frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    let runtime = FakeRuntimeClient::new(vec![json!({
        "schema_version": RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": ["史湘云"],
        "used_context_refs": ["current_question"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": question_frame,
        "rule_candidates": [
            {
                "candidate_type": "entity_alias",
                "term": "枕霞客",
                "reason": "用户表达显示它可能是史湘云别名"
            },
            {
                "candidate_type": "entity_alias",
                "term": "史湘云",
                "reason": "重复出现的人物称呼需要确认是否已在 active ontology"
            }
        ]
    })]);
    let messages = [ContextMessage {
        role: "user".to_string(),
        content: "她的结局呢？".to_string(),
    }];

    let context = create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-rule-candidate-staging",
            model_id: "tonglingyu",
            external_user_ref: "rule-candidate-user",
            external_session_id: "rule-candidate-session",
            external_message_id: "rule-candidate-message",
            question: "她的结局呢？",
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    assert_eq!(context.resolved_question, resolved_question);
    assert_eq!(
        context.context_pack["resolver"]["question_frame"]["subject"]["canonical"],
        json!("史湘云")
    );

    let conn = Connection::open(&db_path).expect("db opens");
    let trace = load_trace_context(&conn, "trace-rule-candidate-staging").expect("trace context");
    let observations = trace["rule_candidate_observations"]
        .as_array()
        .expect("candidate observations");
    assert_eq!(observations.len(), 2);
    assert_observation_status(observations, "枕霞客", "staged", "none");
    assert_observation_status(
        observations,
        "史湘云",
        "blocked_active_duplicate",
        "active_duplicate",
    );
    assert_eq!(
        context_rules::latest_subject_in_text("枕霞客的结局").expect("active ontology lookup"),
        None
    );
    let staged_candidate_id = candidate_id_for_term(observations, "枕霞客");
    record_rule_candidate_regression_evidence(
        &conn,
        RuleCandidateRegressionEvidenceInput {
            candidate_id: &staged_candidate_id,
            actor: "test-admin",
            suite_ref: "conversation_cases.small20",
            report_ref: "eval://rule-candidate-staging/small20",
            report_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            case_count: 20,
            passed_count: 20,
            failed_count: 0,
            skipped_count: 0,
            notes: Some("staged alias candidate regression evidence"),
        },
    )
    .expect("regression evidence");
    let preflight = run_rule_candidate_preflight_check(&conn, &staged_candidate_id, "test-admin")
        .expect("preflight");
    assert_eq!(preflight["status"], json!("passed"));
    assert_eq!(preflight["candidate_status"], json!("ready_for_review"));
    assert_eq!(preflight["active_path_visible"], json!(false));
    let ontology_path = std::env::temp_dir().join(format!(
        "subject-ontology-promotion-{}.json",
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::write(
        &ontology_path,
        include_str!("../../../resources/subject_ontology.json"),
    )
    .expect("subject ontology test catalog");
    let promotion = run_rule_candidate_promotion(
        &conn,
        &staged_candidate_id,
        RuleCandidatePromotionInput {
            actor: "test-admin".to_string(),
            reason: "admin reviewed alias candidate against source evidence".to_string(),
            target_ref: Some("subject:史湘云".to_string()),
            catalog_version: Some("test-online-promotion".to_string()),
            paths: RuleCandidatePromotionPaths {
                subject_ontology_path: Some(ontology_path.clone()),
                ..RuleCandidatePromotionPaths::default()
            },
        },
    )
    .expect("promotion");
    assert_eq!(promotion["status"], json!("passed"));
    assert_eq!(promotion["candidate_status"], json!("promoted"));
    assert_eq!(promotion["active_path_visible"], json!(true));
    assert!(
        promotion["promotion_batch_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("promotion-batch-"))
    );
    assert_eq!(
        promotion["promotion_manifest"]["artifact_kind"],
        json!("rule")
    );
    assert_eq!(promotion["promotion_manifest"]["status"], json!("passed"));
    let promoted_catalog: Value =
        serde_json::from_str(&std::fs::read_to_string(&ontology_path).expect("promoted catalog"))
            .expect("promoted catalog json");
    assert!(
        promoted_catalog["subjects"]
            .as_array()
            .expect("subjects")
            .iter()
            .find(|subject| subject["canonical"] == json!("史湘云"))
            .and_then(|subject| subject["aliases"].as_array())
            .expect("aliases")
            .iter()
            .any(|alias| alias.as_str() == Some("枕霞客"))
    );
    std::fs::remove_file(&ontology_path).ok();

    let duplicate_candidate_id = candidate_id_for_term(observations, "史湘云");
    let duplicate_preflight =
        run_rule_candidate_preflight_check(&conn, &duplicate_candidate_id, "test-admin")
            .expect("duplicate preflight");
    assert_eq!(duplicate_preflight["status"], json!("failed"));
    assert!(
        duplicate_preflight["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|error| error == "schema_or_status_check_failed")
    );
    remove_file_db(&db_path);
}

#[tokio::test]
async fn rejected_conversation_state_hallucinated_entity_does_not_stage_rule_candidates() {
    let db_path = temp_context_db_path("rule-candidate-conversation-state-reject");
    let conn = file_conn(&db_path);
    drop(conn);
    let invalid_state = json!({
        "object": crate::conversation_state::CONVERSATION_STATE_SUMMARY_OBJECT,
        "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
        "current_topic": "林黛玉相关问题",
        "active_entities": ["林黛玉"],
        "open_questions": ["晴雯后来怎么样？"],
        "last_answer_boundaries": [],
        "evidence_package_refs": [],
        "reviewer_warnings": [],
        "memory_allowed_as_evidence": false,
        "summary_confidence": 0.9
    });
    let runtime = FakeRuntimeClient::new(vec![invalid_state.clone(), invalid_state]);
    let messages = [ContextMessage {
        role: "user".to_string(),
        content: "晴雯后来怎么样？".to_string(),
    }];

    let context = create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-rule-candidate-conversation-state-reject",
            model_id: "tonglingyu",
            external_user_ref: "rule-candidate-state-user",
            external_session_id: "rule-candidate-state-session",
            external_message_id: "rule-candidate-state-message",
            question: "晴雯后来怎么样？",
            messages: &messages,
            history_over_limit: true,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Disabled,
        LlmMode::Enforced,
    )
    .await
    .expect("deterministic frame still creates context");

    assert_eq!(
        context.context_pack["llm_agent_context_path"]["conversation_state_agent"]["contract_accepted"],
        json!(false)
    );
    let conn = Connection::open(&db_path).expect("db opens");
    let trace = load_trace_context(&conn, "trace-rule-candidate-conversation-state-reject")
        .expect("trace context");
    assert_eq!(
        trace["rule_candidate_observations"]
            .as_array()
            .expect("candidate observations")
            .len(),
        0
    );
    let candidate_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rule_candidates", [], |row| row.get(0))
        .expect("candidate count");
    let observation_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM rule_candidate_observations",
            [],
            |row| row.get(0),
        )
        .expect("observation count");
    assert_eq!(candidate_count, 0);
    assert_eq!(observation_count, 0);
    remove_file_db(&db_path);
}

#[tokio::test]
async fn unknown_relation_predicate_stages_reviewable_predicate_alias_candidate() {
    let db_path = temp_context_db_path("rule-candidate-unknown-relation-predicate");
    let conn = file_conn(&db_path);
    drop(conn);
    let runtime = FakeRuntimeClient::new(Vec::new());
    let messages = [ContextMessage {
        role: "user".to_string(),
        content: "紫鹃照管过史湘云吗？".to_string(),
    }];

    let context = create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-rule-candidate-unknown-relation-predicate",
            model_id: "tonglingyu",
            external_user_ref: "rule-candidate-unknown-user",
            external_session_id: "rule-candidate-unknown-session",
            external_message_id: "rule-candidate-unknown-message",
            question: "紫鹃照管过史湘云吗？",
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    assert!(context.needs_clarification);
    assert_eq!(
        context.unsupported_reason.as_deref(),
        Some("unknown_relation_predicate")
    );
    assert_eq!(runtime.profile_inputs().len(), 0);

    let conn = Connection::open(&db_path).expect("db opens");
    let trace = load_trace_context(&conn, "trace-rule-candidate-unknown-relation-predicate")
        .expect("trace context");
    let observations = trace["rule_candidate_observations"]
        .as_array()
        .expect("candidate observations");
    assert_eq!(observations.len(), 1);
    let observation = &observations[0];
    assert_eq!(observation["candidate_type"], json!("predicate_alias"));
    assert_eq!(observation["primary_term"], json!("照管"));
    assert_eq!(observation["status"], json!("staged"));
    assert_eq!(observation["conflict_status"], json!("none"));
    assert_eq!(
        observation["validator_audit"]["profile_id"],
        json!("deterministic_question_frame")
    );
    assert_eq!(
        observation["validator_audit"]["contract_accepted"],
        json!(true)
    );
    assert!(
        context_rules::predicate_in_text("照管过史湘云吗？")
            .expect("active predicate lookup")
            .is_none()
    );
    remove_file_db(&db_path);
}

#[tokio::test]
async fn runtime_rule_candidate_promotes_answer_rule_catalog() {
    let db_path = temp_context_db_path("runtime-rule-candidate-answer-promotion");
    let conn = file_conn(&db_path);
    drop(conn);
    let resolved_question = "脂批中的凭据呢？";
    let question_frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    let runtime = FakeRuntimeClient::new(vec![json!({
        "schema_version": RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.88,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": question_frame,
        "rule_candidates": [
            {
                "candidate_type": "answer_evidence_request_term",
                "term": "凭据",
                "reason": "用户把凭据当作证据请求词使用，应进入 answer rule review"
            }
        ]
    })]);
    let messages = [ContextMessage {
        role: "user".to_string(),
        content: resolved_question.to_string(),
    }];

    create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-runtime-rule-candidate-answer-promotion",
            model_id: "tonglingyu",
            external_user_ref: "runtime-rule-candidate-user",
            external_session_id: "runtime-rule-candidate-session",
            external_message_id: "runtime-rule-candidate-message",
            question: resolved_question,
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    let conn = Connection::open(&db_path).expect("db opens");
    let trace = load_trace_context(&conn, "trace-runtime-rule-candidate-answer-promotion")
        .expect("trace context");
    let observations = trace["rule_candidate_observations"]
        .as_array()
        .expect("candidate observations");
    assert_eq!(observations.len(), 1);
    assert_observation_status_for_question(
        observations,
        "凭据",
        "staged",
        "none",
        resolved_question,
        resolved_question,
    );
    let candidate_id = candidate_id_for_term(observations, "凭据");

    let packet = read_rule_candidate_for_review(&conn, &candidate_id)
        .expect("review packet")
        .expect("candidate packet");
    assert_eq!(
        packet["promotion_target"]["catalog_name"],
        json!("answer_rules")
    );
    assert_eq!(
        packet["promotion_target"]["target_ref_pattern"],
        json!("answer_rules:answer_requirements.evidence_request_terms")
    );

    record_rule_candidate_regression_evidence(
        &conn,
        RuleCandidateRegressionEvidenceInput {
            candidate_id: &candidate_id,
            actor: "test-admin",
            suite_ref: "conversation_cases.small20",
            report_ref: "eval://runtime-rule-candidate-answer/small20",
            report_sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            case_count: 20,
            passed_count: 20,
            failed_count: 0,
            skipped_count: 0,
            notes: Some("runtime answer rule candidate regression evidence"),
        },
    )
    .expect("regression evidence");
    let preflight =
        run_rule_candidate_preflight_check(&conn, &candidate_id, "test-admin").expect("preflight");
    assert_eq!(preflight["status"], json!("passed"));

    let answer_rules_path = std::env::temp_dir().join(format!(
        "answer-rules-promotion-{}.json",
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::write(
        &answer_rules_path,
        include_str!("../../../../tonglingyu-runtime/resources/answer_rules.json"),
    )
    .expect("answer rules test catalog");
    let promotion = run_rule_candidate_promotion(
        &conn,
        &candidate_id,
        RuleCandidatePromotionInput {
            actor: "test-admin".to_string(),
            reason: "admin reviewed evidence request term".to_string(),
            target_ref: Some("answer_rules:answer_requirements.evidence_request_terms".to_string()),
            catalog_version: Some("test-runtime-answer-promotion".to_string()),
            paths: RuleCandidatePromotionPaths {
                runtime_paths: RuntimeRuleCandidatePromotionPaths {
                    answer_rules_path: Some(answer_rules_path.clone()),
                    ..RuntimeRuleCandidatePromotionPaths::default()
                },
                ..RuleCandidatePromotionPaths::default()
            },
        },
    )
    .expect("promotion");
    assert_eq!(promotion["status"], json!("passed"));
    assert_eq!(promotion["catalog_name"], json!("answer_rules"));
    assert_eq!(promotion["active_path_visible"], json!(true));
    assert_eq!(
        promotion["promotion_manifest"]["rule_diff_refs"][0]["catalog_name"],
        json!("answer_rules")
    );

    let promoted_catalog: Value =
        serde_json::from_str(&std::fs::read_to_string(&answer_rules_path).expect("catalog"))
            .expect("catalog json");
    assert!(
        promoted_catalog["answer_requirements"]["evidence_request_terms"]
            .as_array()
            .expect("terms")
            .iter()
            .any(|term| term.as_str() == Some("凭据"))
    );
    std::fs::remove_file(&answer_rules_path).ok();
    remove_file_db(&db_path);
}

#[tokio::test]
async fn runtime_rule_candidate_promotion_requires_target_ref() {
    let db_path = temp_context_db_path("runtime-rule-candidate-target-required");
    let conn = file_conn(&db_path);
    drop(conn);
    let resolved_question = "脂批中的凭据呢？";
    let question_frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    let runtime = FakeRuntimeClient::new(vec![json!({
        "schema_version": RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.88,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": question_frame,
        "rule_candidates": [
            {
                "candidate_type": "answer_evidence_request_term",
                "term": "凭据",
                "reason": "用户把凭据当作证据请求词使用，应进入 answer rule review"
            }
        ]
    })]);
    let messages = [ContextMessage {
        role: "user".to_string(),
        content: resolved_question.to_string(),
    }];

    create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-runtime-rule-candidate-target-required",
            model_id: "tonglingyu",
            external_user_ref: "runtime-rule-candidate-target-user",
            external_session_id: "runtime-rule-candidate-target-session",
            external_message_id: "runtime-rule-candidate-target-message",
            question: resolved_question,
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    let conn = Connection::open(&db_path).expect("db opens");
    let trace = load_trace_context(&conn, "trace-runtime-rule-candidate-target-required")
        .expect("trace context");
    let observations = trace["rule_candidate_observations"]
        .as_array()
        .expect("candidate observations");
    let candidate_id = candidate_id_for_term(observations, "凭据");
    record_rule_candidate_regression_evidence(
        &conn,
        RuleCandidateRegressionEvidenceInput {
            candidate_id: &candidate_id,
            actor: "test-admin",
            suite_ref: "conversation_cases.small20",
            report_ref: "eval://runtime-rule-candidate-target/small20",
            report_sha256: "123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0",
            case_count: 20,
            passed_count: 20,
            failed_count: 0,
            skipped_count: 0,
            notes: Some("runtime answer rule candidate regression evidence"),
        },
    )
    .expect("regression evidence");
    run_rule_candidate_preflight_check(&conn, &candidate_id, "test-admin").expect("preflight");

    let answer_rules_path = std::env::temp_dir().join(format!(
        "answer-rules-target-required-{}.json",
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::write(
        &answer_rules_path,
        include_str!("../../../../tonglingyu-runtime/resources/answer_rules.json"),
    )
    .expect("answer rules test catalog");
    let promotion = run_rule_candidate_promotion(
        &conn,
        &candidate_id,
        RuleCandidatePromotionInput {
            actor: "test-admin".to_string(),
            reason: "admin reviewed evidence request term".to_string(),
            target_ref: None,
            catalog_version: Some("test-runtime-answer-target-required".to_string()),
            paths: RuleCandidatePromotionPaths {
                runtime_paths: RuntimeRuleCandidatePromotionPaths {
                    answer_rules_path: Some(answer_rules_path.clone()),
                    ..RuntimeRuleCandidatePromotionPaths::default()
                },
                ..RuleCandidatePromotionPaths::default()
            },
        },
    )
    .expect("promotion failure is recorded");
    assert_eq!(promotion["status"], json!("failed"));
    assert!(
        promotion["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|error| error
                .as_str()
                .is_some_and(|text| text.contains("target_ref")))
    );

    std::fs::remove_file(&answer_rules_path).ok();
    remove_file_db(&db_path);
}

#[tokio::test]
async fn runtime_rule_candidate_active_duplicate_is_blocked() {
    let db_path = temp_context_db_path("runtime-rule-candidate-active-duplicate");
    let conn = file_conn(&db_path);
    drop(conn);
    let resolved_question = "脂批中的证据呢？";
    let question_frame = question_frame::build_question_frame(resolved_question)
        .expect("question frame")
        .audit_json();
    let runtime = FakeRuntimeClient::new(vec![json!({
        "schema_version": RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [],
        "used_context_refs": ["current_question"],
        "confidence": 0.88,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null,
        "question_frame_candidate": question_frame,
        "rule_candidates": [
            {
                "candidate_type": "answer_evidence_request_term",
                "term": "证据",
                "reason": "重复建议现有证据请求词"
            }
        ]
    })]);
    let messages = [ContextMessage {
        role: "user".to_string(),
        content: resolved_question.to_string(),
    }];

    create_context_for_request_with_agent_runtime_and_modes(
        &db_path,
        ContextRequestInput {
            trace_id: "trace-runtime-rule-candidate-active-duplicate",
            model_id: "tonglingyu",
            external_user_ref: "runtime-rule-candidate-dup-user",
            external_session_id: "runtime-rule-candidate-dup-session",
            external_message_id: "runtime-rule-candidate-dup-message",
            question: resolved_question,
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
        &runtime,
        LlmMode::Enforced,
        LlmMode::Disabled,
    )
    .await
    .expect("context");

    let conn = Connection::open(&db_path).expect("db opens");
    let trace = load_trace_context(&conn, "trace-runtime-rule-candidate-active-duplicate")
        .expect("trace context");
    let observations = trace["rule_candidate_observations"]
        .as_array()
        .expect("candidate observations");
    assert_eq!(observations.len(), 1);
    assert_observation_status_for_question(
        observations,
        "证据",
        "blocked_active_duplicate",
        "active_duplicate",
        resolved_question,
        resolved_question,
    );
    assert!(
        observations[0]["active_rule_refs"]
            .as_array()
            .expect("active refs")
            .iter()
            .any(|item| item["rule_ref"]
                == json!("answer_rules.answer_requirements.evidence_request_terms"))
    );
    let candidate_id = candidate_id_for_term(observations, "证据");
    let preflight = run_rule_candidate_preflight_check(&conn, &candidate_id, "test-admin")
        .expect("duplicate preflight");
    assert_eq!(preflight["status"], json!("failed"));

    remove_file_db(&db_path);
}

fn candidate_id_for_term(observations: &[Value], term: &str) -> String {
    observations
        .iter()
        .find(|item| item["primary_term"] == json!(term))
        .and_then(|item| item["candidate_id"].as_str())
        .expect("candidate id by term")
        .to_string()
}

fn assert_observation_status(
    observations: &[Value],
    term: &str,
    status: &str,
    conflict_status: &str,
) {
    assert_observation_status_for_question(
        observations,
        term,
        status,
        conflict_status,
        "她的结局呢？",
        "史湘云的结局呢？",
    );
}

fn assert_observation_status_for_question(
    observations: &[Value],
    term: &str,
    status: &str,
    conflict_status: &str,
    source_question: &str,
    resolved_question: &str,
) {
    let observation = observations
        .iter()
        .find(|item| item["primary_term"] == json!(term))
        .expect("observation by term");
    assert_eq!(observation["status"], json!(status));
    assert_eq!(observation["conflict_status"], json!(conflict_status));
    assert_eq!(observation["source_question"], json!(source_question));
    assert_eq!(observation["resolved_question"], json!(resolved_question));
    assert_eq!(
        observation["validator_audit"]["contract_accepted"],
        json!(true)
    );
}
