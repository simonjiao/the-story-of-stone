use super::*;
use crate::context_governance::RESOLVER_SCHEMA_VERSION;
use crate::eval_command::{
    EVAL_NOT_APPLICABLE_COVERAGE_SMOKE, EXPECTED_TLY_INSCRIPTION_BLOCKS, EvalCase,
    EvalQualityAccumulator, eval_allows_non_production_quality_issue, eval_expected_block_ids,
    eval_expected_evidence_not_applicable_reason, eval_failure_quality_report,
    eval_quality_summary, expected_refs_hit_at,
};
use agent_core::{
    AgentCoreError, CoreResult, ErrorCode, RuntimeOutput, RuntimeProfileInput, RuntimeRunInput,
    RuntimeSessionInput,
};
use tonglingyu_runtime::{
    DOMAIN_RETRIEVAL_PLAN_SCHEMA_VERSION, KnowledgeItemCreateInput, KnowledgeItemStateUpdateInput,
    RetrievalFailureListInput, RetrievalFailureView, ReviewRecord, domain_common_retrieval_plan,
};

fn test_env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    let values = pairs
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    move |name| values.get(name).map(|value| (*value).to_string())
}

fn test_internal_profiles() -> InternalProfiles {
    InternalProfiles {
        main: "honglou-main".to_string(),
        text: "honglou-text".to_string(),
        reviewer: "honglou-reviewer".to_string(),
    }
}

#[test]
fn public_completion_strips_cached_runtime_stream_events() {
    let value = completion_value(
        "tonglingyu",
        "测试回答".to_string(),
        None,
        Some("session-test"),
    );
    let cached = cache_completion_value(
        &value,
        &[RuntimeWorkflowStreamEvent {
            sequence: 0,
            event_type: "content_delta".to_string(),
            profile: "honglou-main".to_string(),
            trace_id: "trace-test".to_string(),
            content_delta: Some("测试回答".to_string()),
            output_ref: None,
            package_id: None,
            metadata: json!({}),
        }],
    );

    assert!(cached.get("_runtime_stream_events").is_some());
    assert!(cached_runtime_stream_events(&cached).is_some());
    let public = public_completion_value(&cached);
    assert!(public.get("_runtime_stream_events").is_none());
    assert!(public.get("_stream_source").is_none());
    assert!(public.get("session_id").is_none());
    assert!(public.get("trace_id").is_none());
    assert!(public.get("evidence_package_id").is_none());
    assert!(public.get("review").is_none());
}

#[test]
fn latest_agent_runtime_summary_uses_last_summary_event() {
    let events = vec![
        json!({
            "event_type": "agent_runtime_profile_execution_summarized",
            "payload": {
                "profile_execution_status": "minimal_envelope_only",
                "tool_result_count": 0,
            },
        }),
        json!({
            "event_type": "agent_runtime_profile_step_executed",
            "payload": {"operation": "draft_answer"},
        }),
        json!({
            "event_type": "agent_runtime_profile_execution_summarized",
            "payload": {
                "profile_execution_status": "openai_compatible_draft_with_local_governance",
                "tool_result_count": 1,
            },
        }),
    ];

    let summary = latest_agent_runtime_summary(&events);

    assert_eq!(
        summary["profile_execution_status"],
        "openai_compatible_draft_with_local_governance"
    );
    assert_eq!(summary["tool_result_count"], json!(1));
    assert!(latest_agent_runtime_summary(&[]).is_null());
}

#[test]
fn llm_agent_runtime_requires_role_provider_config_over_legacy_envs() {
    let env = test_env(&[
        ("AGENT_RUNTIME_OPENAI_BASE_URL", "https://api.deepseek.com"),
        ("AGENT_RUNTIME_OPENAI_MODEL", "deepseek-v4-flash"),
        ("AGENT_RUNTIME_OPENAI_API_KEY", "sk-test"),
        ("OPENAI_BASE_URL", "https://legacy.invalid/v1"),
    ]);

    let error = match build_llm_agent_runtime_from_source(&env) {
        Ok(_) => panic!("legacy runtime env must not configure LLM agent routing"),
        Err(error) => error.to_string(),
    };

    assert!(error.contains(QUESTION_NORMALIZER_PROVIDER_ENV));
}

#[test]
fn llm_agent_runtime_builds_deepseek_provider_profile_without_secret_summary() {
    let env = test_env(&[
        (QUESTION_NORMALIZER_PROVIDER_ENV, "deepseek_flash"),
        (CONVERSATION_STATE_PROVIDER_ENV, "deepseek_flash"),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_BACKEND",
            "openai-compatible-network",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_BASE_URL",
            "https://api.deepseek.com",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_MODEL",
            "deepseek-v4-flash",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_API_KEY_ENV",
            "DEEPSEEK_API_KEY",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_API_FAMILY",
            "deepseek-chat",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_THINKING_TYPE",
            "disabled",
        ),
        ("DEEPSEEK_API_KEY", "sk-test-secret"),
    ]);

    let (_client, mode, config) =
        build_llm_agent_runtime_from_source(&env).expect("deepseek provider config builds");
    let serialized = serde_json::to_string(&config).expect("provider config serializes");

    assert_eq!(mode, "provider-profile");
    assert_eq!(
        config["provider_profiles"][0]["backend"],
        json!("openai-compatible-network")
    );
    assert_eq!(
        config["provider_profiles"][0]["base_url_host"],
        json!("api.deepseek.com")
    );
    assert!(!serialized.contains("sk-test-secret"));
    assert_eq!(config["secret_values_printed"], json!(false));
}

#[test]
fn workflow_agent_runtime_config_requires_role_providers_over_legacy_mode() {
    let env = test_env(&[("TONGLINGYU_AGENT_RUNTIME_MODE", "hermes")]);

    let error = build_workflow_agent_runtime_config_from_source(&test_internal_profiles(), &env)
        .expect_err("legacy runtime mode must not configure gateway workflow routing")
        .to_string();

    assert!(error.contains(TEXT_PROVIDER_ENV));
}

#[test]
fn workflow_agent_runtime_config_rejects_hermes_provider_backend() {
    let env = test_env(&[
        (TEXT_PROVIDER_ENV, "hermes_tooling"),
        (PACKAGE_PROVIDER_ENV, "hermes_tooling"),
        (DRAFT_PROVIDER_ENV, "hermes_tooling"),
        (REVIEW_PROVIDER_ENV, "hermes_tooling"),
        (
            "TONGLINGYU_AGENT_PROVIDER_HERMES_TOOLING_BACKEND",
            "hermes-agent",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_HERMES_TOOLING_BASE_URL",
            "http://hermes:8642/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_HERMES_TOOLING_MODEL",
            "hermes-agent",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_HERMES_TOOLING_API_KEY_ENV",
            "HERMES_TOOLING_API_KEY",
        ),
        ("HERMES_TOOLING_API_KEY", "hermes-test-secret"),
    ]);

    let error = build_workflow_agent_runtime_config_from_source(&test_internal_profiles(), &env)
        .expect_err("workflow runtime must reject Hermes provider backend")
        .to_string();

    assert!(error.contains("openai-compatible-network"));
    assert!(error.contains("hermes-agent"));
    assert!(!error.contains("hermes-test-secret"));
}

#[test]
fn workflow_agent_runtime_config_rejects_unsupported_provider_backend() {
    let env = test_env(&[
        (TEXT_PROVIDER_ENV, "legacy_workflow"),
        (PACKAGE_PROVIDER_ENV, "legacy_workflow"),
        (DRAFT_PROVIDER_ENV, "legacy_workflow"),
        (REVIEW_PROVIDER_ENV, "legacy_workflow"),
        (
            "TONGLINGYU_AGENT_PROVIDER_LEGACY_WORKFLOW_BACKEND",
            "legacy-provider",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_LEGACY_WORKFLOW_BASE_URL",
            "https://provider.invalid/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_LEGACY_WORKFLOW_MODEL",
            "legacy-model",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_LEGACY_WORKFLOW_API_KEY_ENV",
            "LEGACY_PROVIDER_API_KEY",
        ),
        ("LEGACY_PROVIDER_API_KEY", "legacy-test-secret"),
    ]);

    let error = build_workflow_agent_runtime_config_from_source(&test_internal_profiles(), &env)
        .expect_err("workflow runtime must reject non-openai-compatible provider backend")
        .to_string();

    assert!(error.contains("openai-compatible-network"));
    assert!(error.contains("legacy-provider"));
    assert!(!error.contains("legacy-test-secret"));
}

#[test]
fn workflow_agent_runtime_config_builds_openai_compatible_provider_profile_without_secret_summary()
{
    let env = test_env(&[
        (TEXT_PROVIDER_ENV, "openai_profile"),
        (PACKAGE_PROVIDER_ENV, "openai_profile"),
        (DRAFT_PROVIDER_ENV, "openai_profile"),
        (REVIEW_PROVIDER_ENV, "openai_profile"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BACKEND",
            "openai-compatible-network",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BASE_URL",
            "http://sub2api:8080/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MODEL",
            "gpt-5.4-mini",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_KEY_ENV",
            "OPENAI_COMPATIBLE_API_KEY",
        ),
        ("OPENAI_COMPATIBLE_API_KEY", "openai-compatible-test-secret"),
    ]);

    let config = build_workflow_agent_runtime_config_from_source(&test_internal_profiles(), &env)
        .expect("workflow openai-compatible provider config builds");
    let serialized = serde_json::to_string(&config).expect("provider config serializes");

    assert_eq!(config["mode"], json!("openai-compatible-network"));
    assert_eq!(
        config["provider_profiles"][0]["backend"],
        json!("openai-compatible-network")
    );
    assert_eq!(
        config["provider_profiles"][0]["base_url_host"],
        json!("sub2api")
    );
    assert!(!serialized.contains("openai-compatible-test-secret"));
    assert_eq!(config["secret_values_printed"], json!(false));
}

fn eval_case_fixture(id: &'static str) -> EvalCase {
    let expected_block_ids = match id {
        "tly-inscription" => EXPECTED_TLY_INSCRIPTION_BLOCKS,
        _ => &[],
    };
    EvalCase {
        id,
        question: "通灵玉是什么？",
        expected_review_status: "passed",
        limit: None,
        min_cards: 1,
        max_cards: None,
        required_evidence_type: Some("base_text"),
        required_text_any: &[],
        required_issue_any: &[],
        expected_evidence_ids: &[],
        expected_block_ids,
        expected_evidence_not_applicable_reason: if expected_block_ids.is_empty() {
            Some(EVAL_NOT_APPLICABLE_COVERAGE_SMOKE)
        } else {
            None
        },
    }
}

fn eval_test_card(block_id: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: format!("ev-{block_id}"),
        evidence_type: "base_text".to_string(),
        source_id: "hongloumeng-wikisource-120".to_string(),
        source_title: "紅樓夢/第008回".to_string(),
        source_url: "https://example.test/source".to_string(),
        revision_id: None,
        block_id: block_id.to_string(),
        text: "莫失莫忘，一除邪祟。".to_string(),
        support_scope: "test".to_string(),
        unsupported_scope: "test".to_string(),
        evidence_level: "primary".to_string(),
        confidence: "high".to_string(),
        verification_status: "verified".to_string(),
    }
}

#[test]
fn rqa_restore_canary_creates_closed_live_refs_without_open_p0() {
    let db_path = temp_gateway_db_path("restore-canary");
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let package = runtime_store
        .create_package(
            "trace-restore-canary-test",
            "通灵玉正面文字在哪里？",
            vec![eval_test_card("block-restore-canary")],
        )
        .expect("package creates");

    let args = RqaRestoreCanaryArgs {
        db: db_path.clone(),
        package_id: Some(package.package_id.clone()),
        reviewer: "restore-drill".to_string(),
        review_note: "closed restore drill canary".to_string(),
    };
    let report = rqa_restore_canary_command(&args).expect("restore canary runs");

    assert_eq!(report["status"], json!("ok"));
    assert_eq!(report["refs"]["trace_id"], json!(package.trace_id));
    assert_eq!(report["refs"]["package_id"], json!(package.package_id));
    assert_eq!(
        report["checks"]["failure_type"],
        json!("restore_drill_canary")
    );
    assert_eq!(report["checks"]["failure_status"], json!("resolved"));
    assert_eq!(report["checks"]["task_status"], json!("closed"));
    assert_eq!(report["checks"]["task_priority"], json!("p1"));
    assert_eq!(report["checks"]["open_p0_retrieval_failures"], json!(0));
    assert_eq!(report["checks"]["open_p0_governance_tasks"], json!(0));

    let rerun = rqa_restore_canary_command(&args).expect("restore canary reruns");
    assert_eq!(rerun["refs"]["failure_id"], report["refs"]["failure_id"]);
    assert_eq!(rerun["refs"]["task_id"], report["refs"]["task_id"]);

    let conn = open_db(&db_path).expect("db opens");
    let canary_events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'rqa_restore_canary_recorded'",
            [],
            |row| row.get(0),
        )
        .expect("audit count");
    assert_eq!(canary_events, 2);
    let failure_status: String = conn
        .query_row(
            "SELECT human_review_status FROM retrieval_failures WHERE failure_id = ?1",
            params![report["refs"]["failure_id"].as_str().expect("failure id")],
            |row| row.get(0),
        )
        .expect("failure status");
    assert_eq!(failure_status, "resolved");
    let task_status: String = conn
        .query_row(
            "SELECT status FROM knowledge_governance_tasks WHERE task_id = ?1",
            params![report["refs"]["task_id"].as_str().expect("task id")],
            |row| row.get(0),
        )
        .expect("task status");
    assert_eq!(task_status, "closed");
    remove_sqlite_file_set(&db_path);
}

fn temp_gateway_db_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{label}-{}.db", new_trace_id()))
}

struct TestLlmAgentRuntime;

#[async_trait::async_trait]
impl RuntimeClient for TestLlmAgentRuntime {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "test llm agent runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "test llm agent runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let payload = input
            .messages
            .get(1)
            .and_then(|message| serde_json::from_str::<Value>(&message.content).ok())
            .unwrap_or_else(|| json!({}));
        let result = match input.profile_id.as_str() {
            QUESTION_NORMALIZER_PROFILE_ID => test_question_normalizer_output(&payload),
            _ => json!({}),
        };
        Ok(RuntimeOutput {
            result_summary: result.to_string(),
            result_ref: Some(format!("test-llm-agent://{}", input.profile_id)),
            messages: Vec::new(),
            metadata: json!({"test_llm_agent": true}),
        })
    }
}

fn test_question_normalizer_output(payload: &Value) -> Value {
    let input_context = &payload["input_context"];
    let current_question = input_context["current_question"]
        .as_str()
        .unwrap_or_default();
    let referent = input_context["allowed_referents"]
        .as_array()
        .and_then(|items| items.iter().find_map(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            infer_test_subject(
                input_context["prior_session_summary_for_context_only"]
                    .as_str()
                    .unwrap_or_default(),
            )
        });
    let Some(referent) = referent else {
        return json!({
            "schema_version": RESOLVER_SCHEMA_VERSION,
            "resolved_question": current_question,
            "referent_bindings": [],
            "used_context_refs": ["current_question"],
            "confidence": 0.5,
            "needs_clarification": true,
            "clarification_question": "请明确你想问哪位人物？",
            "unsupported_reason": "unresolved_referent"
        });
    };
    let resolved_question = if current_question.contains('她') {
        current_question.replacen('她', &referent, 1)
    } else if current_question.contains('他') {
        current_question.replacen('他', &referent, 1)
    } else {
        current_question.to_string()
    };
    json!({
        "schema_version": RESOLVER_SCHEMA_VERSION,
        "resolved_question": resolved_question,
        "referent_bindings": [referent],
        "used_context_refs": ["current_question", "session_summary"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null,
        "unsupported_reason": null
    })
}

fn infer_test_subject(text: &str) -> Option<String> {
    crate::context_rules::latest_subject_in_text(text)
        .ok()
        .flatten()
}

#[test]
fn runtime_schema_migrate_command_applies_additive_migrations() {
    let db_path = temp_gateway_db_path("tonglingyu-runtime-schema-migrate");
    remove_sqlite_file_set(&db_path);
    let report = runtime_schema_migrate_command(&RuntimeSchemaMigrateArgs {
        db: db_path.clone(),
    })
    .expect("runtime schema migrate");

    assert_eq!(
        report["object"],
        json!("tonglingyu.runtime_schema_migration_apply")
    );
    assert_eq!(report["status"], json!("ok"));
    assert_eq!(report["pending_after"], json!(0));
    assert_eq!(report["will_rebuild_knowledge_base"], json!(false));
    assert_eq!(report["will_delete_runtime_data"], json!(false));
    assert_eq!(report["secret_values_printed"], json!(false));
    assert!(
        report["pending_before"].as_u64().unwrap_or_default() > 0,
        "{report}"
    );
    assert_eq!(report["after"]["pending_migrations"], json!([]), "{report}");
    remove_sqlite_file_set(&db_path);
}

fn test_app_state(db_path: PathBuf) -> AppState {
    AppState {
        db: db_path.clone(),
        runtime_store: TonglingyuRuntimeStore::new(db_path),
        response_store: Arc::new(Mutex::new(ResponseStoreBackend::InMemory(
            InMemoryResponseEventStore::default(),
        ))),
        response_jobs: Arc::new(Mutex::new(ResponseJobQueueBackend::InMemory(
            InMemoryResponseJobQueue::default(),
        ))),
        response_job_max_attempts: 3,
        response_sync_wait_secs: 2,
        realtime_max_buffer_chars: 4000,
        model_id: DEFAULT_MODEL_ID.to_string(),
        model_name: DEFAULT_MODEL_NAME.to_string(),
        upstream_base_url: None,
        upstream_api_key: None,
        upstream_model: DEFAULT_MODEL_ID.to_string(),
        upstream_timeout_secs: 30,
        max_evidence: 8,
        retriever: RetrieverHttpClient::new("http://127.0.0.1:1", 1)
            .expect("test retriever client"),
        retriever_base_url: "http://127.0.0.1:1".to_string(),
        retriever_timeout_secs: 1,
        retriever_rerank: false,
        gateway_api_keys: vec!["gateway-key".to_string()],
        admin_api_keys: vec!["admin-key".to_string()],
        allow_admin_with_gateway_key: false,
        max_messages: 20,
        max_question_chars: 2000,
        max_body_bytes: 1024 * 1024,
        rate_limit_per_minute: 120,
        rate_limiter: Arc::new(GatewayRateLimiter::new(120, Duration::from_secs(60))),
        admin_rate_limiter: Arc::new(GatewayRateLimiter::new(120, Duration::from_secs(60))),
        retention_days: 30,
        online_evidence_card_worker_enabled: true,
        online_evidence_card_worker_interval_secs: 30,
        online_evidence_card_worker_batch_size: 20,
        online_evidence_card_worker_retrieval_limit: 12,
        profiles: InternalProfiles {
            main: "honglou-main".to_string(),
            text: "honglou-text".to_string(),
            reviewer: "honglou-reviewer".to_string(),
        },
        agent_runtime: Arc::new(MinimalRuntimeClient::default()),
        agent_runtime_mode: TonglingyuAgentRuntimeMode::Minimal,
        llm_agent_runtime: Arc::new(TestLlmAgentRuntime),
        llm_agent_runtime_mode: "minimal-test".to_string(),
        llm_agent_provider_profiles: json!({
            "object": "test.llm_agent_provider_profile_config"
        }),
        workflow_agent_provider_profiles: json!({
            "object": "test.workflow_agent_provider_profile_config"
        }),
        started_at: now_rfc3339(),
    }
}

fn seed_eval_retrieval_failure(db_path: &Path, trace_id: &str) -> String {
    let runtime_store = TonglingyuRuntimeStore::new(db_path.to_path_buf());
    let case = eval_case_fixture("rqa-admin-failure");
    let package = EvidencePackage {
        package_id: "pkg-rqa-admin-failure".to_string(),
        trace_id: trace_id.to_string(),
        question: case.question.to_string(),
        cards: Vec::new(),
        claims: vec!["证据不足，不能给出确定结论。".to_string()],
        claim_evidence_map: Vec::new(),
        knowledge_state_summary: Default::default(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "needs_revision".to_string(),
            severity: "high".to_string(),
            issues: vec!["当前没有可追溯证据。".to_string()],
            summary: "reviewer requires evidence".to_string(),
        },
    };
    let quality_report =
        eval_failure_quality_report(None, &case, &package, &["forced eval failure".to_string()]);
    runtime_store
        .create_retrieval_failure(RetrievalFailureCreateInput {
            trace_id: trace_id.to_string(),
            package_id: Some(package.package_id),
            question: case.question.to_string(),
            quality_report,
            selected_evidence_ids: Vec::new(),
            expected_evidence_ids: Vec::new(),
            agent_diagnosis: Some("eval_case_failed:forced eval failure".to_string()),
            proposed_fix: Some("inspect_eval_case_quality_details".to_string()),
        })
        .expect("seed retrieval failure")
        .failure_id
}

fn admin_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer admin-key".parse().unwrap());
    headers.insert("x-tonglingyu-subject", "admin-1".parse().unwrap());
    headers
}

fn gateway_headers(user_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer gateway-key".parse().unwrap());
    headers.insert("x-tonglingyu-subject", user_id.parse().unwrap());
    headers.insert("x-open-webui-user-id", user_id.parse().unwrap());
    headers
}

fn seed_owned_gateway_package(db_path: &Path, user_id: &str) -> EvidencePackage {
    let runtime_store = TonglingyuRuntimeStore::new(db_path.to_path_buf());
    let question = "通灵玉回答是否有证据？";
    let package = runtime_store
        .create_package(
            "trace-user-feedback-test",
            question,
            vec![eval_test_card("block-user-feedback-test")],
        )
        .expect("package creates");
    let conn = open_db(db_path).expect("gateway db opens");
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: question.to_string(),
    }];
    let scoped_context = create_context_for_request(
        &conn,
        ContextRequestInput {
            trace_id: &package.trace_id,
            model_id: DEFAULT_MODEL_ID,
            external_user_ref: user_id,
            external_session_id: "chat-user-feedback-test",
            external_message_id: "message-user-feedback-test",
            question,
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
    )
    .expect("scoped context creates");
    let response = completion_value(
        DEFAULT_MODEL_ID,
        "测试回答".to_string(),
        Some(&package),
        Some(&scoped_context.user_session_id),
    );
    append_final_response(
        &conn,
        FinalResponseJournalInput {
            trace_id: &package.trace_id,
            user_session_id: &scoped_context.user_session_id,
            interaction_context_id: &scoped_context.interaction_context_id,
            context_pack_id: &scoped_context.context_pack_id,
            external_message_id: "message-user-feedback-test",
            package_id: Some(&package.package_id),
            response: &response,
        },
    )
    .expect("final response journal stores");
    package
}

fn seed_runtime_chat_source(db_path: &Path) {
    let conn = open_db(db_path).expect("gateway db opens");
    tonglingyu_runtime::init_runtime_schema(&conn).expect("runtime schema");
    tonglingyu_runtime::init_knowledge_base_schema(&conn).expect("kb schema");
    conn.execute(
            r#"
            INSERT INTO sources (
                source_id, source_category, format, title, work, edition, language,
                source_url, api_url, fetched_at, license, license_url,
                license_source_url, attribution, usage_boundary, notes,
                snapshot_contract_json, source_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            "#,
            params![
                "quality-source",
                "base_material",
                "mediawiki",
                "质量测试红楼梦 source",
                "红楼梦",
                "测试底本；仅用于 Gateway scoped context 单元测试",
                "zh",
                "https://example.test/source",
                "https://example.test/api",
                "2026-05-18T00:00:00Z",
                "CC-BY-SA-4.0",
                "https://creativecommons.org/licenses/by-sa/4.0/",
                "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "Wikisource contributors",
                "可作为正文或版本对照证据候选；不声明完成学术校勘。",
                "测试 source snapshot",
                serde_json::to_string(&json!({
                    "license": "CC-BY-SA-4.0",
                    "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                    "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                    "attribution": "Wikisource contributors",
                    "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
                }))
                .expect("snapshot contract json"),
                "hash-quality-source",
            ],
        )
        .expect("insert source");
    let source_title = "质量测试红楼梦/第六十六回";
    let text = "尤三姐最后自刎，以明心迹；此处仅作 scoped context 单元测试证据。";
    conn.execute(
        r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        params![
            "quality-block-yousanjie",
            "quality-source",
            "quality-section-066",
            source_title,
            tonglingyu_runtime::normalize_for_search(source_title),
            "https://example.test/source/66",
            1_i64,
            1_i64,
            "paragraph",
            Option::<String>::None,
            text,
            tonglingyu_runtime::normalize_for_search(text),
            "base_text",
            66_i64,
        ],
    )
    .expect("insert block");
}

#[test]
fn prune_gateway_and_runtime_data_preserves_active_rqa_gateway_rows() {
    let db_path = temp_gateway_db_path("gateway-prune-rqa-protect");
    let old = "2020-01-01T00:00:00Z";
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let active_package = runtime_store
        .create_package(
            "trace-gateway-prune-active",
            "active gateway retention question",
            vec![eval_test_card("block-gateway-prune-active")],
        )
        .expect("active package creates");
    let runtime_conn = runtime_store.open_connection().expect("runtime db opens");
    for table in ["evidence_packages", "evidence_cards", "review_records"] {
        runtime_conn
            .execute(
                &format!("UPDATE {table} SET created_at = ?1 WHERE package_id = ?2"),
                params![old, &active_package.package_id],
            )
            .expect("runtime package rows old");
    }
    runtime_conn
        .execute(
            "UPDATE audit_events SET created_at = ?1 WHERE trace_id = ?2",
            params![old, &active_package.trace_id],
        )
        .expect("runtime audit rows old");
    runtime_conn
        .execute(
            r#"
                INSERT INTO retrieval_failures (
                    failure_id, trace_id, package_id, question_sha256,
                    question_char_count, question_summary, kb_schema_version,
                    kb_version_id, failure_type, redacted_query_terms_json,
                    required_evidence_types_json, actual_evidence_types_json,
                    expected_evidence_ids_json, selected_evidence_ids_json,
                    missing_evidence_types_json, quality_issues_json,
                    agent_diagnosis, proposed_fix, human_review_status, reviewer,
                    review_note, created_at, updated_at, resolved_at
                ) VALUES (
                    'rf-gateway-prune-active', ?1, ?2, ?3, 33,
                    'sha256:gateway-prune-active', ?4, NULL,
                    'expected_evidence_missing', '[]', '["base_text"]', '[]',
                    '["ev-missing"]', '[]', '["base_text"]',
                    '["expected_evidence_missing"]', NULL,
                    'protect gateway rows while RQA failure is open',
                    'open', NULL, NULL, ?5, ?5, NULL
                )
                "#,
            params![
                &active_package.trace_id,
                &active_package.package_id,
                hash_text("active gateway retention question"),
                KNOWLEDGE_BASE_SCHEMA_VERSION,
                old,
            ],
        )
        .expect("active retrieval failure inserts");
    drop(runtime_conn);

    let conn = open_db(&db_path).expect("gateway db opens");
    seed_gateway_retention_row(
        &conn,
        "active",
        &active_package.trace_id,
        Some(&active_package.package_id),
        "active gateway retention question",
        old,
    );
    seed_gateway_retention_row(
        &conn,
        "expired",
        "trace-gateway-prune-expired",
        Some("pkg-gateway-prune-expired"),
        "expired gateway retention question",
        old,
    );
    drop(conn);

    let dry_run = prune_gateway_and_runtime_data(&db_path, 1, true).expect("gateway dry run prune");
    assert_eq!(dry_run["counts"]["gateway_message_candidates"], json!(2));
    assert_eq!(dry_run["counts"]["gateway_messages"], json!(1));
    assert_eq!(dry_run["counts"]["protected_gateway_messages"], json!(1));
    assert_eq!(dry_run["counts"]["workflow_state_candidates"], json!(2));
    assert_eq!(dry_run["counts"]["workflow_states"], json!(1));
    assert_eq!(dry_run["counts"]["protected_workflow_states"], json!(1));
    assert_eq!(dry_run["counts"]["gateway_sessions"], json!(1));
    assert_eq!(dry_run["counts"]["protected_gateway_sessions"], json!(1));

    let report = prune_gateway_and_runtime_data(&db_path, 1, false).expect("gateway prune");
    assert_eq!(report["counts"]["gateway_messages"], json!(1));
    assert_eq!(report["counts"]["workflow_states"], json!(1));
    assert_eq!(report["counts"]["gateway_sessions"], json!(1));
    assert_eq!(report["counts"]["gateway_tombstones"], json!(3));
    let conn = open_db(&db_path).expect("gateway db reopens");
    assert_eq!(
        gateway_row_count(&conn, "gateway_messages", "message_id", "msg-active"),
        1
    );
    assert_eq!(
        gateway_row_count(&conn, "gateway_messages", "message_id", "msg-expired"),
        0
    );
    assert_eq!(
        gateway_row_count(&conn, "workflow_states", "state_id", "state-active"),
        1
    );
    assert_eq!(
        gateway_row_count(&conn, "workflow_states", "state_id", "state-expired"),
        0
    );
    assert_eq!(
        gateway_row_count(&conn, "gateway_sessions", "session_id", "session-active"),
        1
    );
    assert_eq!(
        gateway_row_count(&conn, "gateway_sessions", "session_id", "session-expired"),
        0
    );
    let gateway_tombstones: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rqa_lifecycle_tombstones WHERE object_type LIKE 'gateway_%' OR object_type = 'workflow_state_batch'",
                [],
                |row| row.get(0),
            )
            .expect("gateway tombstone count");
    assert_eq!(gateway_tombstones, 3);
    let tombstone_payloads = load_gateway_tombstone_payloads(&conn);
    assert!(tombstone_payloads.iter().all(|payload| {
        !payload.contains("active gateway retention question")
            && !payload.contains("expired gateway retention question")
    }));
    drop(conn);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

fn seed_gateway_retention_row(
    conn: &Connection,
    suffix: &str,
    trace_id: &str,
    package_id: Option<&str>,
    question: &str,
    created_at: &str,
) {
    conn.execute(
            "INSERT INTO gateway_sessions (session_id, user_ref, chat_ref, model_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                format!("session-{suffix}"),
                format!("user-{suffix}"),
                format!("chat-{suffix}"),
                DEFAULT_MODEL_ID,
                created_at,
            ],
        )
        .expect("gateway session inserts");
    conn.execute(
            "INSERT INTO gateway_messages (message_id, session_id, external_message_id, trace_id, package_id, request_hash, question, response_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                format!("msg-{suffix}"),
                format!("session-{suffix}"),
                format!("external-{suffix}"),
                trace_id,
                package_id,
                hash_text(question),
                question,
                json!({"object": "chat.completion", "id": format!("cmpl-{suffix}")}).to_string(),
                created_at,
            ],
        )
        .expect("gateway message inserts");
    conn.execute(
            "INSERT INTO workflow_states (state_id, trace_id, session_id, package_id, state, status, detail_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                format!("state-{suffix}"),
                trace_id,
                format!("session-{suffix}"),
                package_id,
                "rqa_retention_test",
                "completed",
                json!({"suffix": suffix}).to_string(),
                created_at,
            ],
        )
        .expect("workflow state inserts");
}

fn gateway_row_count(conn: &Connection, table: &str, id_column: &str, id: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {id_column} = ?1"),
        params![id],
        |row| row.get(0),
    )
    .expect("gateway row count")
}

fn load_gateway_tombstone_payloads(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT payload_json FROM rqa_lifecycle_tombstones ORDER BY created_at")
        .expect("prepare tombstone payloads")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query tombstone payloads")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("collect tombstone payloads")
}

fn audit_event_count(db_path: &Path, event_type: &str) -> i64 {
    let conn = open_db(db_path).expect("db opens");
    count_where(&conn, "audit_events", "event_type = ?1", event_type).expect("audit count")
}

fn latest_audit_event_payload(db_path: &Path, event_type: &str) -> Value {
    let conn = open_db(db_path).expect("db opens");
    let payload: String = conn
            .query_row(
                "SELECT payload_json FROM audit_events WHERE event_type = ?1 ORDER BY created_at DESC, event_id DESC LIMIT 1",
                params![event_type],
                |row| row.get(0),
            )
            .expect("audit payload exists");
    serde_json::from_str(&payload).expect("audit payload json")
}

async fn response_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body reads");
    String::from_utf8(bytes.to_vec()).expect("response body is utf-8")
}

async fn response_json(response: Response) -> Value {
    serde_json::from_str(&response_text(response).await).expect("response json")
}

#[tokio::test]
async fn responses_and_runs_share_identity_status_and_replay_events() {
    let db_path = temp_gateway_db_path("gateway-response-run-projection");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut headers = gateway_headers("user-response-run");
    headers.insert("x-tonglingyu-tenant-id", "tenant-a".parse().unwrap());
    headers.insert("idempotency-key", "idem-response-run".parse().unwrap());
    let payload = json!({
        "model": DEFAULT_MODEL_ID,
        "input": "分析这批材料并给出结论",
        "background": true,
        "metadata": {"mode": "rag"}
    });

    let created =
        create_response_endpoint(State(state.clone()), headers.clone(), Json(payload.clone()))
            .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    assert_eq!(created["object"], json!("response"));
    assert_eq!(created["status"], json!("queued"));
    let response_id = created["response_id"].as_str().unwrap().to_string();
    let run_id = created["run_id"].as_str().unwrap().to_string();

    let idempotent =
        create_response_endpoint(State(state.clone()), headers.clone(), Json(payload)).await;
    assert_eq!(idempotent.status(), StatusCode::OK);
    let idempotent = response_json(idempotent).await;
    assert_eq!(idempotent["response_id"], json!(response_id));
    assert_eq!(idempotent["run_id"], json!(run_id));

    let run_status = get_run_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath(run_id.clone()),
    )
    .await;
    assert_eq!(run_status.status(), StatusCode::OK);
    let run_status = response_json(run_status).await;
    assert_eq!(run_status["object"], json!("run"));
    assert_eq!(run_status["id"], json!(run_id));
    assert_eq!(run_status["response_id"], json!(response_id));
    assert_eq!(run_status["status"], json!("queued"));

    let events = response_events_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath(response_id.clone()),
        Query(ResponseEventsQuery { after: None }),
    )
    .await;
    assert_eq!(events.status(), StatusCode::OK);
    let events = response_text(events).await;
    assert!(events.contains("id: 1\n"));
    assert!(events.contains("event: response.created"));
    assert!(events.contains(&response_id));
    assert!(!events.contains("trace_id"));
    assert!(!events.contains("[DONE]"));

    let mut resume_headers = headers.clone();
    resume_headers.insert("last-event-id", "1".parse().unwrap());
    let resumed = run_events_endpoint(
        State(state),
        resume_headers,
        AxumPath(run_id),
        Query(ResponseEventsQuery { after: None }),
    )
    .await;
    assert_eq!(resumed.status(), StatusCode::OK);
    assert_eq!(response_text(resumed).await, "");

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn response_create_enqueues_job_only_for_new_identity() {
    let db_path = temp_gateway_db_path("gateway-response-job-enqueue");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut headers = gateway_headers("enqueue-user");
    headers.insert("idempotency-key", "enqueue-once".parse().unwrap());
    let payload = json!({
        "model": DEFAULT_MODEL_ID,
        "input": "分析这批材料",
        "background": true,
        "metadata": {"client_request_id": "enqueue-once"}
    });

    let created =
        create_response_endpoint(State(state.clone()), headers.clone(), Json(payload.clone()))
            .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    let idempotent = create_response_endpoint(State(state.clone()), headers, Json(payload)).await;
    assert_eq!(idempotent.status(), StatusCode::OK);
    let idempotent = response_json(idempotent).await;
    assert_eq!(created["response_id"], idempotent["response_id"]);

    let first = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("first job")
    };
    assert_eq!(json!(first.job.response_id), created["response_id"]);
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue.complete(first).expect("complete");
        assert!(queue.claim_next("test-worker", 0).expect("claim").is_none());
    }
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn response_worker_completes_empty_input_from_queued_job() {
    let db_path = temp_gateway_db_path("gateway-response-worker-empty");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("worker-empty-user");
    let payload = json!({
        "model": DEFAULT_MODEL_ID,
        "input": "",
        "background": true
    });

    let created =
        create_response_endpoint(State(state.clone()), headers.clone(), Json(payload)).await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    let response_id = created["response_id"]
        .as_str()
        .expect("response id")
        .to_string();
    let lease = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("queued job")
    };

    execute_response_job(state.clone(), lease.job.clone())
        .await
        .expect("worker execution");
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue.complete(lease).expect("complete");
    }

    let final_state = get_response_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath(response_id.clone()),
    )
    .await;
    assert_eq!(final_state.status(), StatusCode::OK);
    let final_state = response_json(final_state).await;
    assert_eq!(final_state["status"], json!("completed"));

    let events = response_events_endpoint(
        State(state),
        headers,
        AxumPath(response_id),
        Query(ResponseEventsQuery { after: None }),
    )
    .await;
    let events = response_text(events).await;
    assert!(events.contains("event: output_text.delta"));
    assert!(events.contains("event: response.completed"));
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn response_workflow_event_sink_maps_runtime_events_and_cancel_signal() {
    let db_path = temp_gateway_db_path("gateway-response-workflow-event-sink");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("workflow-sink-user");
    let created = create_response_endpoint(
        State(state.clone()),
        headers.clone(),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "input": "",
            "background": true
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let lease = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("queued job")
    };
    let job = lease.job.clone();
    let sink = ResponseWorkflowEventSink {
        state: state.clone(),
        job: job.clone(),
    };
    let started_event = RuntimeWorkflowStreamEvent {
        sequence: 0,
        event_type: "started".to_string(),
        profile: "honglou-main".to_string(),
        trace_id: job.trace_id.clone(),
        content_delta: None,
        output_ref: None,
        package_id: Some("pkg-workflow-sink".to_string()),
        metadata: json!({"context_pack_id": "ctx-secret"}),
    };
    sink.emit(started_event.clone())
        .await
        .expect("started emit");
    sink.emit(RuntimeWorkflowStreamEvent {
        sequence: 1,
        event_type: "content_delta".to_string(),
        profile: "honglou-main".to_string(),
        trace_id: job.trace_id.clone(),
        content_delta: Some("公开增量".to_string()),
        output_ref: None,
        package_id: Some("pkg-workflow-sink".to_string()),
        metadata: json!({"trace_id": "trace-forged"}),
    })
    .await
    .expect("delta emit");
    sink.emit(RuntimeWorkflowStreamEvent {
        sequence: 2,
        event_type: "final_output".to_string(),
        profile: "honglou-main".to_string(),
        trace_id: job.trace_id.clone(),
        content_delta: None,
        output_ref: Some("output-secret".to_string()),
        package_id: Some("pkg-workflow-sink".to_string()),
        metadata: json!({}),
    })
    .await
    .expect("final emit");

    let events = response_events_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath(job.response_id.clone()),
        Query(ResponseEventsQuery { after: None }),
    )
    .await;
    assert_eq!(events.status(), StatusCode::OK);
    let events = response_text(events).await;
    assert!(events.contains("event: response.status"));
    assert!(events.contains("event: output_text.delta"));
    assert!(events.contains("公开增量"));
    assert!(!events.contains("ctx-secret"));
    assert!(!events.contains("trace-forged"));
    assert!(!events.contains(&job.trace_id));

    let canceled = cancel_response_endpoint(
        State(state.clone()),
        headers,
        AxumPath(job.response_id.clone()),
    )
    .await;
    assert_eq!(canceled.status(), StatusCode::OK);
    assert!(sink.cancel_requested().await.expect("cancel check"));

    let mut missing_job = job.clone();
    missing_job.response_id = "resp_missing_for_sink".to_string();
    let missing_sink = ResponseWorkflowEventSink {
        state: state.clone(),
        job: missing_job,
    };
    assert!(missing_sink.emit(started_event).await.is_err());
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue.complete(lease).expect("complete");
    }
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn responses_stream_true_reads_same_response_events() {
    let db_path = temp_gateway_db_path("gateway-response-stream-bridge");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("response-stream-user");
    let payload = json!({
        "model": DEFAULT_MODEL_ID,
        "input": "",
        "stream": true
    });

    let response =
        create_response_endpoint(State(state.clone()), headers.clone(), Json(payload)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let lease = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("queued response job")
    };
    execute_response_job(state.clone(), lease.job.clone())
        .await
        .expect("worker execution");
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue.complete(lease).expect("complete");
    }

    let body = response_text(response).await;
    assert!(body.contains("event: response.created"));
    assert!(body.contains("event: output_text.delta"));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("请提出一个《红楼梦》相关问题。"));
    assert!(body.contains("data: [DONE]"));
    assert!(!body.contains("trace_id"));
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn response_background_false_waits_until_worker_terminal_state() {
    let db_path = temp_gateway_db_path("gateway-response-sync-wait");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("response-sync-user");
    let payload = json!({
        "model": DEFAULT_MODEL_ID,
        "input": "",
        "background": false
    });

    let create_future =
        create_response_endpoint(State(state.clone()), headers.clone(), Json(payload));
    let worker_future = async {
        for _ in 0..40 {
            let lease = {
                let mut queue = state.response_jobs.lock().expect("queue");
                queue.claim_next("test-worker", 0).expect("claim")
            };
            if let Some(lease) = lease {
                execute_response_job(state.clone(), lease.job.clone())
                    .await
                    .expect("worker execution");
                let mut queue = state.response_jobs.lock().expect("queue");
                queue.complete(lease).expect("complete");
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("response job was not queued before sync wait timeout");
    };
    let (created, _) = tokio::join!(create_future, worker_future);

    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    assert_eq!(created["object"], json!("response"));
    assert_eq!(created["status"], json!("completed"));
    assert!(created["completed_at"].as_i64().is_some());
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn chat_stream_bridges_to_response_job_events() {
    let db_path = temp_gateway_db_path("gateway-chat-stream-bridge");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("chat-stream-user");
    let payload = json!({
        "model": DEFAULT_MODEL_ID,
        "stream": true,
        "messages": [
            {"role": "user", "content": ""}
        ]
    });

    let response = chat_completions(State(state.clone()), headers, Json(payload)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let lease = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("queued chat job")
    };
    execute_response_job(state.clone(), lease.job.clone())
        .await
        .expect("worker execution");
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue.complete(lease).expect("complete");
    }

    let body = response_text(response).await;
    assert!(body.contains("\"object\":\"chat.completion.chunk\""));
    assert!(body.contains("\"delta\":{\"role\":\"assistant\"}"));
    assert!(body.contains("请提出一个《红楼梦》相关问题。"));
    assert!(body.contains("\"tonglingyu_event\""));
    assert!(body.contains("\"type\":\"response.status\""));
    assert!(body.contains("data: [DONE]"));
    assert!(!body.contains("trace_id"));
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn response_owner_scope_requires_tenant_and_subject() {
    let db_path = temp_gateway_db_path("gateway-response-owner");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut owner_headers = gateway_headers("owner-user");
    owner_headers.insert("x-tonglingyu-tenant-id", "tenant-a".parse().unwrap());
    let created = create_response_endpoint(
        State(state.clone()),
        owner_headers.clone(),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "input": "分析材料",
            "background": true
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    let response_id = created["response_id"].as_str().unwrap().to_string();

    let mut wrong_tenant_headers = gateway_headers("owner-user");
    wrong_tenant_headers.insert("x-tonglingyu-tenant-id", "tenant-b".parse().unwrap());
    let wrong_tenant = get_response_endpoint(
        State(state.clone()),
        wrong_tenant_headers,
        AxumPath(response_id.clone()),
    )
    .await;
    assert_eq!(wrong_tenant.status(), StatusCode::NOT_FOUND);

    let mut wrong_subject_headers = gateway_headers("other-user");
    wrong_subject_headers.insert("x-tonglingyu-tenant-id", "tenant-a".parse().unwrap());
    let wrong_subject = get_response_endpoint(
        State(state.clone()),
        wrong_subject_headers,
        AxumPath(response_id.clone()),
    )
    .await;
    assert_eq!(wrong_subject.status(), StatusCode::NOT_FOUND);

    let owner = get_response_endpoint(State(state), owner_headers, AxumPath(response_id)).await;
    assert_eq!(owner.status(), StatusCode::OK);

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn run_cancel_writes_terminal_events_and_rejects_late_action() {
    let db_path = temp_gateway_db_path("gateway-run-cancel");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("cancel-user");
    let created = create_run_endpoint(
        State(state.clone()),
        headers.clone(),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "input": "执行长时间研究任务",
            "background": true
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    let run_id = created["run_id"].as_str().unwrap().to_string();

    let canceled = cancel_run_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath(run_id.clone()),
    )
    .await;
    assert_eq!(canceled.status(), StatusCode::OK);
    let canceled = response_json(canceled).await;
    assert_eq!(canceled["object"], json!("run"));
    assert_eq!(canceled["status"], json!("canceled"));
    assert_eq!(canceled["cancel_requested"], json!(true));

    let events = run_events_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath(run_id.clone()),
        Query(ResponseEventsQuery { after: None }),
    )
    .await;
    assert_eq!(events.status(), StatusCode::OK);
    let events = response_text(events).await;
    assert!(events.contains("event: response.created"));
    assert!(events.contains("event: response.status"));
    assert!(events.contains("event: response.canceled"));
    assert!(events.contains("data: [DONE]"));
    assert!(!events.contains("trace_id"));

    let action = submit_run_action_endpoint(
        State(state),
        headers,
        AxumPath((run_id, "approval-test".to_string())),
        Json(json!({})),
    )
    .await;
    assert_eq!(action.status(), StatusCode::CONFLICT);
    let action = response_json(action).await;
    assert_eq!(action["error"]["code"], json!("run_terminal"));

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn run_action_submit_restores_waiting_run_idempotently() {
    let db_path = temp_gateway_db_path("gateway-run-action-submit");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut headers = gateway_headers("action-user");
    headers.insert("idempotency-key", "submit-action-once".parse().unwrap());
    let created = create_run_endpoint(
        State(state.clone()),
        headers.clone(),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "input": "等待人工审批",
            "background": true
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    let run_id = created["run_id"].as_str().unwrap().to_string();
    let response_id = created["response_id"].as_str().unwrap().to_string();

    append_response_event_for_response_id(
        &state,
        &response_id,
        ResponseEventType::ResponseStatus,
        json!({"status": "in_progress"}),
        Some(ResponseStatus::InProgress),
        None,
        None,
        None,
    )
    .expect("start response");
    append_response_event_for_response_id(
        &state,
        &response_id,
        ResponseEventType::ResponseRequiresAction,
        json!({
            "action_id": "approval-1",
            "action_type": "human_approval",
            "expires_at": OffsetDateTime::now_utc().unix_timestamp() + 3600,
        }),
        Some(ResponseStatus::RequiresAction),
        None,
        None,
        None,
    )
    .expect("require action");

    let submitted = submit_run_action_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath((run_id.clone(), "approval-1".to_string())),
        Json(json!({"approved": true})),
    )
    .await;
    assert_eq!(submitted.status(), StatusCode::OK);
    let submitted = response_json(submitted).await;
    assert_eq!(submitted["status"], json!("in_progress"));
    assert_eq!(submitted["requires_action_count"], json!(0));

    let duplicate = submit_run_action_endpoint(
        State(state.clone()),
        headers.clone(),
        AxumPath((run_id.clone(), "approval-1".to_string())),
        Json(json!({"approved": true})),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::OK);
    let duplicate = response_json(duplicate).await;
    assert_eq!(duplicate["status"], json!("in_progress"));

    let events = run_events_endpoint(
        State(state),
        headers,
        AxumPath(run_id),
        Query(ResponseEventsQuery { after: None }),
    )
    .await;
    assert_eq!(events.status(), StatusCode::OK);
    let events = response_text(events).await;
    assert!(events.contains("event: response.requires_action"));
    assert_eq!(events.matches("\"action_status\":\"submitted\"").count(), 1);
    assert!(!events.contains("trace_id"));

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn realtime_rejects_audio_sequence_replay_and_forbidden_fields() {
    let mut session = RealtimeSessionState::new(8);
    let audio_event: RealtimeClientEvent = serde_json::from_value(json!({
        "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
        "event_id": "cli-audio",
        "type": "input_audio.delta",
        "sequence": 1,
        "payload": {}
    }))
    .expect("audio event");
    let audio_error = realtime_validate_client_event(&session, &audio_event).unwrap_err();
    assert_eq!(audio_error.code, "audio_frame_rejected");

    let valid_event: RealtimeClientEvent = serde_json::from_value(json!({
        "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
        "event_id": "cli-1",
        "type": "input_text.delta",
        "sequence": 1,
        "payload": {"text": "宝玉"}
    }))
    .expect("valid event");
    realtime_validate_client_event(&session, &valid_event).expect("valid");
    realtime_record_client_event(&mut session, &valid_event);

    let replay_event: RealtimeClientEvent = serde_json::from_value(json!({
        "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
        "event_id": "cli-2",
        "type": "input_text.delta",
        "sequence": 1,
        "payload": {"text": "宝玉"}
    }))
    .expect("replay event");
    let replay_error = realtime_validate_client_event(&session, &replay_event).unwrap_err();
    assert_eq!(replay_error.code, "sequence_regressed");

    let duplicate_event: RealtimeClientEvent = serde_json::from_value(json!({
        "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
        "event_id": "cli-1",
        "type": "input_text.delta",
        "sequence": 2,
        "payload": {"text": "宝玉"}
    }))
    .expect("duplicate event");
    let duplicate_error = realtime_validate_client_event(&session, &duplicate_event).unwrap_err();
    assert_eq!(duplicate_error.code, "duplicate_event_id");

    let forbidden_event: RealtimeClientEvent = serde_json::from_value(json!({
        "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
        "event_id": "cli-3",
        "type": "input_text.commit",
        "sequence": 3,
        "payload": {"text": "宝玉", "trace_id": "client-forged"}
    }))
    .expect("forbidden event");
    let forbidden_error = realtime_validate_client_event(&session, &forbidden_event).unwrap_err();
    assert_eq!(forbidden_error.code, "forbidden_control_fields");

    let oversized_event: RealtimeClientEvent = serde_json::from_value(json!({
        "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
        "event_id": "cli-4",
        "type": "input_text.commit",
        "sequence": 4,
        "payload": {"text": "一二三四五六七八九"}
    }))
    .expect("oversized event");
    let oversized_error = realtime_validate_client_event(&session, &oversized_event).unwrap_err();
    assert_eq!(oversized_error.code, "input_too_large");
}

#[tokio::test]
async fn realtime_commit_create_resume_and_cancel_share_response_events() {
    let db_path = temp_gateway_db_path("gateway-realtime-create");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("realtime-user");
    let mut session = RealtimeSessionState::new(4000);
    session.session_id = "session-realtime-test".to_string();

    let delta_frames = realtime_handle_text_frame(
        state.clone(),
        &headers,
        "realtime-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-delta-1",
            "type": "input_text.delta",
            "sequence": 1,
            "payload": {"text": "宝玉的玉"}
        })
        .to_string(),
    )
    .await;
    assert!(delta_frames.join("\n").contains("input_text.delta.ack"));
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        assert!(queue.claim_next("test-worker", 0).expect("claim").is_none());
    }

    let commit_frames = realtime_handle_text_frame(
        state.clone(),
        &headers,
        "realtime-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-commit-1",
            "type": "input_text.commit",
            "sequence": 2,
            "payload": {"text": ""}
        })
        .to_string(),
    )
    .await;
    assert!(commit_frames.join("\n").contains("input_text.committed"));

    let create_frames = realtime_handle_text_frame(
        state.clone(),
        &headers,
        "realtime-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-create-1",
            "type": "response.create",
            "sequence": 3,
            "payload": {"text": ""}
        })
        .to_string(),
    )
    .await;
    let create_body = create_frames.join("\n");
    assert!(create_body.contains("\"type\":\"response.created\""));
    assert!(!create_body.contains("trace_id"));

    let lease = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("queued realtime job")
    };
    let response_id = lease.job.response_id.clone();
    execute_response_job(state.clone(), lease.job.clone())
        .await
        .expect("worker execution");
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue.complete(lease).expect("complete");
    }

    let fanout_frames = realtime_collect_subscription_frames(state.clone(), &mut session);
    let fanout_body = fanout_frames.join("\n");
    assert!(fanout_body.contains("\"type\":\"output_text.delta\""));
    assert!(fanout_body.contains("\"type\":\"response.completed\""));
    assert!(fanout_body.contains("请提出一个《红楼梦》相关问题。"));
    assert!(!fanout_body.contains("trace_id"));

    let resume_frames = realtime_handle_text_frame(
        state.clone(),
        &headers,
        "realtime-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-resume-1",
            "type": "response.resume",
            "response_id": response_id,
            "sequence": 4,
            "payload": {"after": 1}
        })
        .to_string(),
    )
    .await;
    let resume_body = resume_frames.join("\n");
    assert!(!resume_body.contains("\"type\":\"response.created\""));
    assert!(resume_body.contains("\"type\":\"response.completed\""));

    let cancel_create_frames = realtime_handle_text_frame(
        state.clone(),
        &headers,
        "realtime-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-create-2",
            "type": "response.create",
            "sequence": 5,
            "payload": {"text": "宝玉的玉第一次怎么来的？"}
        })
        .to_string(),
    )
    .await;
    assert!(
        cancel_create_frames
            .join("\n")
            .contains("\"type\":\"response.created\"")
    );
    let cancel_lease = {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .claim_next("test-worker", 0)
            .expect("claim")
            .expect("queued cancel job")
    };
    let cancel_response_id = cancel_lease.job.response_id.clone();
    {
        let mut queue = state.response_jobs.lock().expect("queue");
        queue
            .complete(cancel_lease)
            .expect("discard queued cancel job");
    }

    let cancel_frames = realtime_handle_text_frame(
        state,
        &headers,
        "realtime-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-cancel-1",
            "type": "response.cancel",
            "response_id": cancel_response_id,
            "sequence": 6,
            "payload": {}
        })
        .to_string(),
    )
    .await;
    let cancel_body = cancel_frames.join("\n");
    assert!(cancel_body.contains("\"type\":\"response.status\""));
    assert!(cancel_body.contains("\"type\":\"response.canceled\""));

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn realtime_action_submit_restores_waiting_response() {
    let db_path = temp_gateway_db_path("gateway-realtime-action");
    let state = Arc::new(test_app_state(db_path.clone()));
    let headers = gateway_headers("realtime-action-user");
    let created = create_run_endpoint(
        State(state.clone()),
        headers.clone(),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "input": "等待实时审批",
            "background": true
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_json(created).await;
    let response_id = created["response_id"].as_str().unwrap().to_string();

    append_response_event_for_response_id(
        &state,
        &response_id,
        ResponseEventType::ResponseStatus,
        json!({"status": "in_progress"}),
        Some(ResponseStatus::InProgress),
        None,
        None,
        None,
    )
    .expect("start response");
    append_response_event_for_response_id(
        &state,
        &response_id,
        ResponseEventType::ResponseRequiresAction,
        json!({
            "action_id": "approval-ws-1",
            "action_type": "human_approval",
            "expires_at": OffsetDateTime::now_utc().unix_timestamp() + 3600,
        }),
        Some(ResponseStatus::RequiresAction),
        None,
        None,
        None,
    )
    .expect("require action");

    let mut session = RealtimeSessionState::new(4000);
    session.session_id = "session-realtime-action-test".to_string();
    let frames = realtime_handle_text_frame(
        state,
        &headers,
        "realtime-action-user",
        &mut session,
        &json!({
            "schema_version": REALTIME_CLIENT_EVENT_SCHEMA_VERSION,
            "event_id": "cli-action-1",
            "type": "response.action.submit",
            "response_id": response_id,
            "sequence": 1,
            "payload": {
                "action_id": "approval-ws-1",
                "approved": true,
                "idempotency_key": "approval-ws-1-once"
            }
        })
        .to_string(),
    )
    .await;

    let body = frames.join("\n");
    assert!(body.contains("\"type\":\"response.requires_action\""));
    assert!(body.contains("\"action_status\":\"submitted\""));
    assert!(!body.contains("trace_id"));
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn response_create_rejects_metadata_scope_override_before_state_creation() {
    let db_path = temp_gateway_db_path("gateway-response-metadata-override");
    let state = Arc::new(test_app_state(db_path.clone()));
    let response = create_response_endpoint(
        State(state),
        gateway_headers("metadata-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "input": "分析材料",
            "metadata": {
                "tenant_id": "tenant-override"
            }
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = response_json(response).await;
    assert_eq!(response["error"]["code"], json!("metadata_overrides_auth"));

    remove_sqlite_file_set(&db_path);
}

#[test]
fn common_recall_person_plan_targets_entity_route_with_vector_required() {
    let domain_plan =
        domain_common_retrieval_plan("person", "介绍尤三姐").expect("domain common retrieval plan");
    let plan = RetrieverSearchPlan::for_domain_plan(&domain_plan, 8, false);

    assert_eq!(plan.routes, vec!["entity", "event", "bm25", "vector"]);
    assert_eq!(plan.raw_plan["domain_retrieval_profile"], json!("person"));
    assert_eq!(
        plan.raw_plan["route_policy"],
        json!("person_lookup_entity_first_with_event_context")
    );
    assert_eq!(plan.route_weights["entity"], 1.4);
    assert!(plan.routes.contains(&"vector".to_string()));
    assert!(
        plan.queries["entity"]
            .iter()
            .any(|query| query.contains("人物"))
    );
    assert_eq!(
        plan.filters.as_ref().expect("filters")["entity_types"],
        json!(["person"])
    );
}

#[test]
fn common_recall_judgement_plan_uses_poem_route_with_structured_terms() {
    let domain_plan = domain_common_retrieval_plan("judgement_poem", "宝钗判词")
        .expect("domain common retrieval plan");
    let plan = RetrieverSearchPlan::for_domain_plan(&domain_plan, 6, true);

    assert_eq!(plan.routes.first().map(String::as_str), Some("poem"));
    assert!(plan.routes.contains(&"commentary".to_string()));
    assert!(plan.routes.contains(&"vector".to_string()));
    assert_eq!(
        plan.raw_plan["domain_retrieval_profile"],
        json!("judgement_poem")
    );
    assert!(
        plan.queries["poem"]
            .iter()
            .any(|query| query.contains("金陵十二钗"))
    );
    assert!(
        plan.structured_terms
            .contains(&"entity_subtype:judgement".to_string())
    );
    assert!(plan.expansion_terms.contains(&"判词".to_string()));
    assert_eq!(
        plan.filters.as_ref().expect("filters")["entity_subtypes"],
        json!(["judgement"])
    );
}

#[test]
fn query_planner_adapter_preserves_planner_shape_and_selects_judgement_recall() {
    let query_plan = query_plan_from_gateway_policy(
        "宝钗判词",
        "poem_or_judgement",
        &["base_text".to_string()],
        None,
    )
    .expect("query plan");
    let search_plan = RetrieverSearchPlan::for_domain_plan(&query_plan, 6, false);

    assert_eq!(
        query_plan.schema_version,
        DOMAIN_RETRIEVAL_PLAN_SCHEMA_VERSION
    );
    assert_eq!(query_plan.retrieval_profile, "judgement_poem");
    assert!(
        query_plan
            .recall_hints
            .contains(&"judgement_poem".to_string())
    );
    assert_eq!(
        search_plan.raw_plan["domain_retrieval_profile"],
        json!("judgement_poem")
    );
    assert_eq!(
        search_plan.raw_plan["domain_plan"]["schema_version"],
        json!(DOMAIN_RETRIEVAL_PLAN_SCHEMA_VERSION)
    );
    assert!(
        search_plan
            .retrieval_routes
            .contains(&"poem_lookup".to_string())
    );
    assert_eq!(search_plan.routes.first().map(String::as_str), Some("poem"));
    assert!(search_plan.routes.contains(&"vector".to_string()));
    assert!(
        search_plan
            .structured_terms
            .contains(&"entity_subtype:judgement".to_string())
    );
}

#[test]
fn query_planner_xiangyun_fate_evidence_uses_runtime_catalog_judgement_recall() {
    let query = "关于史湘云的结局，脂批中的证据呢？";
    let frame = json!({
        "schema_version": "tonglingyu.question_frame.v2",
        "frame_id": "qf_primary",
        "original_question": query,
        "normalized_question": query,
        "task": "extract_evidence",
        "slots": {
            "subject": {"type": "character", "name": "史湘云", "aliases": [], "source": "current_question"},
            "event": {"type": "fate", "trigger": "结局"},
            "evidence_focus": "character_fate",
            "source_scope": {"work": "hongloumeng", "range": "pre_80_base_text_and_commentary"}
        },
        "answer_target": {"type": "evidence_list"},
        "evidence_contract": {
            "required_types": ["commentary"],
            "supporting_types": ["base_text"],
            "min_answer_basis": 1,
            "require_claim_evidence_map": true,
            "allow_navigation_hint_as_answer_basis": false,
            "citation_granularity": "chapter_or_span",
            "unsupported_behavior": "fail_closed"
        },
        "subquestions": [],
        "clarification": {"needed": false, "open_slots": [], "question": null}
    });
    let query_plan = query_plan_from_gateway_policy(
        query,
        "evidence",
        &["base_text".to_string(), "commentary".to_string()],
        Some(&frame),
    )
    .expect("query plan");
    let search_plan = RetrieverSearchPlan::for_domain_plan(&query_plan, 8, false);

    assert_eq!(query_plan.primary_intent, "commentary_lookup");
    assert_eq!(query_plan.retrieval_profile, "judgement_poem");
    assert!(
        query_plan
            .recall_hints
            .contains(&"judgement_poem".to_string())
    );
    assert_eq!(
        search_plan.raw_plan["domain_retrieval_profile"],
        json!("judgement_poem")
    );
    assert!(search_plan.routes.contains(&"poem".to_string()));
    assert!(search_plan.routes.contains(&"commentary".to_string()));
    assert_eq!(
        search_plan.filters.as_ref().expect("filters")["entity_subtypes"],
        json!(["judgement"])
    );
    assert!(search_plan.expansion_terms.contains(&"樂中悲".to_string()));
    assert!(
        search_plan
            .retrieval_routes
            .contains(&"poem_lookup".to_string())
    );
    assert!(
        search_plan
            .retrieval_routes
            .contains(&"commentary_lookup".to_string())
    );
}

#[test]
fn query_planner_adapter_maps_person_intro_to_person_recall() {
    let query_plan =
        query_plan_from_gateway_policy("介绍尤三姐", "base_text", &["base_text".to_string()], None)
            .expect("query plan");
    let search_plan = RetrieverSearchPlan::for_domain_plan(&query_plan, 8, false);

    assert_eq!(query_plan.retrieval_profile, "person");
    assert_eq!(
        search_plan.raw_plan["domain_retrieval_profile"],
        json!("person")
    );
    assert_eq!(
        search_plan.routes.first().map(String::as_str),
        Some("entity")
    );
    assert!(
        search_plan
            .retrieval_routes
            .contains(&"entity_lookup".to_string())
    );
}

#[test]
fn query_planner_count_loss_question_uses_event_recall_terms() {
    let query = "通灵宝玉丢了几次？";
    let frame = crate::question_frame::build_question_frame(query).expect("question frame");
    let frame_value = serde_json::to_value(&frame).expect("v2 frame json");
    let query_plan = query_plan_from_gateway_policy(
        query,
        "base_text",
        &["base_text".to_string()],
        Some(&frame_value),
    )
    .expect("query plan");
    let search_plan = RetrieverSearchPlan::for_domain_plan(&query_plan, 8, false);

    assert_eq!(query_plan.primary_intent, "event_lookup");
    assert_eq!(query_plan.retrieval_profile, "event");
    assert!(
        search_plan
            .retrieval_routes
            .contains(&"event_lookup".to_string())
    );
    assert!(
        search_plan
            .structured_terms
            .contains(&"event_type:loss_or_theft".to_string())
    );
    assert!(
        search_plan
            .keyword_queries
            .iter()
            .any(|query| query.contains("良儿偷玉"))
    );
    assert!(
        search_plan
            .keyword_queries
            .iter()
            .any(|query| query.contains("扫雪拾玉"))
    );
    for queries in search_plan.queries.values() {
        assert!(queries.len() <= 8);
    }
}

#[test]
fn eval_quality_summary_fails_closed_without_expected_denominator() {
    let quality = EvalQualityAccumulator {
        total_cases: 1,
        quality_report_cases: 1,
        classified_cases: 1,
        expected_evidence_cases: 0,
        forbidden_conclusion_cases: 1,
        forbidden_conclusion_avoided: 1,
        reviewer_status_matched: 1,
        ..EvalQualityAccumulator::default()
    };

    let summary = eval_quality_summary(&quality);

    assert_eq!(summary["status"], json!("failed"));
    assert!(
        summary["blockers"]
            .as_array()
            .is_some_and(|items| { items.contains(&json!("expected_evidence_denominator_zero")) })
    );
}

#[test]
fn eval_quality_summary_passes_annotated_thresholds() {
    let mut quality = EvalQualityAccumulator {
        total_cases: 1,
        quality_report_cases: 1,
        quality_report_production_ready_required_cases: 1,
        quality_report_production_ready_cases: 1,
        classified_cases: 1,
        expected_evidence_cases: 1,
        expected_hit_at_1: 1,
        expected_hit_at_3: 1,
        expected_hit_at_8: 1,
        required_type_cases: 1,
        required_type_passed: 1,
        exact_term_total: 1,
        exact_term_passed: 1,
        source_boundary_confirmation_cases: 1,
        source_boundary_confirmation_avoided: 1,
        forbidden_conclusion_cases: 1,
        forbidden_conclusion_avoided: 1,
        reviewer_status_matched: 1,
        ..EvalQualityAccumulator::default()
    };
    quality
        .source_ids
        .insert("hongloumeng-wikisource-120".to_string());
    quality.edition_labels.insert("紅樓夢/第008回".to_string());

    let summary = eval_quality_summary(&quality);

    assert_eq!(summary["status"], json!("passed"));
    assert_eq!(summary["expected_evidence_hit_at_8"]["ratio"], json!(1.0));
    assert_eq!(summary["source_diversity"]["count"], json!(1));
}

#[test]
fn eval_quality_summary_fails_closed_on_knowledge_state_rejection() {
    let quality = EvalQualityAccumulator {
        total_cases: 1,
        quality_report_cases: 1,
        quality_report_production_ready_required_cases: 1,
        quality_report_production_ready_cases: 1,
        classified_cases: 1,
        expected_evidence_cases: 1,
        expected_hit_at_1: 1,
        expected_hit_at_3: 1,
        expected_hit_at_8: 1,
        required_type_cases: 1,
        required_type_passed: 1,
        exact_term_total: 1,
        exact_term_passed: 1,
        source_boundary_confirmation_cases: 1,
        source_boundary_confirmation_avoided: 1,
        forbidden_conclusion_cases: 1,
        forbidden_conclusion_avoided: 1,
        reviewer_status_matched: 1,
        knowledge_state_runtime_policy_rejected_count: 1,
        knowledge_state_system_calibrated_rejected_count: 1,
        knowledge_state_reviewer_downgrade_cases: 1,
        ..EvalQualityAccumulator::default()
    };

    let summary = eval_quality_summary(&quality);

    assert_eq!(summary["status"], json!("failed"));
    assert_eq!(
        summary["knowledge_state_quality"]["runtime_policy_rejected_count"],
        json!(1)
    );
    assert!(summary["blockers"].as_array().is_some_and(|items| {
        items.contains(&json!("knowledge_state_runtime_policy_rejected"))
            && items.contains(&json!("knowledge_state_reviewer_downgrade"))
    }));
}

#[test]
fn eval_cli_defaults_to_snapshot_copy() {
    let args = Args::try_parse_from(["tonglingyu-gateway", "eval"]).expect("parse eval args");
    let Command::Eval(eval_args) = args.command else {
        panic!("expected eval command");
    };

    assert!(!eval_args.allow_db_mutation);
}

#[test]
fn eval_cli_requires_explicit_db_mutation_opt_in() {
    let args = Args::try_parse_from(["tonglingyu-gateway", "eval", "--allow-db-mutation"])
        .expect("parse eval args");
    let Command::Eval(eval_args) = args.command else {
        panic!("expected eval command");
    };

    assert!(eval_args.allow_db_mutation);
}

#[test]
fn eval_allows_expected_downgrade_quality_issues_only_for_negative_cases() {
    let mut negative = eval_case_fixture("unsupported-modern-topic");
    negative.expected_review_status = "needs_revision";
    negative.required_evidence_type = None;
    negative.min_cards = 0;

    assert!(eval_allows_non_production_quality_issue(
        &negative,
        "no_evidence_selected"
    ));
    assert!(eval_allows_non_production_quality_issue(
        &negative,
        "missing_required_evidence_type:base_text"
    ));
    assert!(!eval_allows_non_production_quality_issue(
        &negative,
        "source_usage_metadata_incomplete:source:missing_license_metadata"
    ));

    let positive = eval_case_fixture("baoyu-alias-retrieval");
    assert!(!eval_allows_non_production_quality_issue(
        &positive,
        "no_evidence_selected"
    ));
}

#[test]
fn eval_case_classification_marks_unannotated_cases_not_applicable() {
    let annotated = eval_case_fixture("tly-inscription");
    let unannotated = eval_case_fixture("baoyu-alias-retrieval");

    assert!(!eval_expected_block_ids(&annotated).is_empty());
    assert!(eval_expected_evidence_not_applicable_reason(&annotated).is_none());
    assert_eq!(
        eval_expected_evidence_not_applicable_reason(&unannotated),
        Some("coverage_smoke_without_stable_expected_block")
    );
}

#[test]
fn eval_expected_hit_requires_all_expected_refs() {
    let case = eval_case_fixture("tly-inscription");
    let partial_cards = vec![eval_test_card(EXPECTED_TLY_INSCRIPTION_BLOCKS[0])];
    let full_cards = vec![
        eval_test_card(EXPECTED_TLY_INSCRIPTION_BLOCKS[0]),
        eval_test_card(EXPECTED_TLY_INSCRIPTION_BLOCKS[1]),
    ];

    assert!(!expected_refs_hit_at(&case, &partial_cards, 8));
    assert!(expected_refs_hit_at(&case, &full_cards, 8));
}

#[test]
fn eval_failure_record_uses_retrieval_failures_api() {
    let db_path = temp_gateway_db_path("tonglingyu-gateway-eval-failure");
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let case = eval_case_fixture("eval-failure-test");
    let package = EvidencePackage {
        package_id: "pkg-eval-failure-test".to_string(),
        trace_id: "trace-eval-failure-test".to_string(),
        question: case.question.to_string(),
        cards: Vec::new(),
        claims: vec!["证据不足，不能给出确定结论。".to_string()],
        claim_evidence_map: Vec::new(),
        knowledge_state_summary: Default::default(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "needs_revision".to_string(),
            severity: "high".to_string(),
            issues: vec!["当前没有可追溯证据。".to_string()],
            summary: "reviewer requires evidence".to_string(),
        },
    };
    let quality_report =
        eval_failure_quality_report(None, &case, &package, &["forced eval failure".to_string()]);

    runtime_store
        .create_retrieval_failure(RetrievalFailureCreateInput {
            trace_id: package.trace_id.clone(),
            package_id: Some(package.package_id.clone()),
            question: case.question.to_string(),
            quality_report,
            selected_evidence_ids: Vec::new(),
            expected_evidence_ids: Vec::new(),
            agent_diagnosis: Some("eval_case_failed:forced eval failure".to_string()),
            proposed_fix: Some("inspect_eval_case_quality_details".to_string()),
        })
        .expect("eval failure writes retrieval failure");
    let failures = runtime_store
        .list_retrieval_failures(RetrievalFailureListInput {
            human_review_status: Some("open".to_string()),
            failure_type: Some("quality_report_not_passed".to_string()),
            limit: 10,
            offset: 0,
            view: RetrievalFailureView::AdminDetail,
        })
        .expect("list retrieval failures");

    assert_eq!(failures.items.len(), 1);
    assert_eq!(
        failures.items[0]["trace_id"],
        json!("trace-eval-failure-test")
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[test]
fn admin_trace_includes_retrieval_quality_summary_and_failure_ids() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-trace-rqa");
    let trace_id = "trace-admin-rqa-test";
    let failure_id = seed_eval_retrieval_failure(&db_path, trace_id);

    let trace = load_trace(&db_path, trace_id)
        .expect("trace loads")
        .expect("trace exists");

    assert_eq!(
        trace["retrieval_quality_summary"]["schema_version"],
        RETRIEVAL_FAILURE_SCHEMA_VERSION
    );
    assert_eq!(
        trace["retrieval_quality_summary"]["failure_count"],
        json!(1)
    );
    assert_eq!(
        trace["retrieval_quality_summary"]["open_failure_count"],
        json!(1)
    );
    assert_eq!(trace["retrieval_failure_ids"], json!([failure_id]));
    assert_eq!(trace["retrieval_failures"][0]["view"], "admin_detail");
    assert_eq!(trace["governance_tasks"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        trace["governance_tasks"][0]["source_failure_id"],
        json!(failure_id)
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[test]
fn admin_trace_includes_online_evidence_card_ingest_state() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-trace-online-card");
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let conn = runtime_store.open_connection().expect("runtime db opens");
    let trace_id = "trace-admin-online-card-test";
    let request = tonglingyu_runtime::create_online_evidence_card_update_request(
        &conn,
        OnlineEvidenceCardUpdateRequestInput {
            trace_id: trace_id.to_string(),
            session_id: Some("session-online-card-test".to_string()),
            resolved_question: "A 与 B 的关系".to_string(),
            question_frame: Some(json!({
                "intent": "relation_query",
                "canonical_question": "A 与 B 的关系",
                "subject": {"canonical": "A", "aliases": []},
                "predicate": {
                    "id": "relation",
                    "label": "关系",
                    "aliases": ["关系"],
                    "evidence_terms": ["关系"]
                },
                "object": {"canonical": "B", "aliases": []},
                "required_evidence_types": ["base_text"]
            })),
            coverage_gap_reason: "package_coverage_partial".to_string(),
            source_scope_policy: json!({"scope": "test"}),
            recall_advice_ref: None,
        },
    )
    .expect("online evidence card update request created");

    let trace = load_trace(&db_path, trace_id)
        .expect("trace loads")
        .expect("trace exists");
    assert_eq!(
        trace["online_evidence_card_ingest"]["update_requests"][0]["update_request_id"],
        json!(request.update_request_id)
    );
    assert_eq!(
        trace["online_evidence_card_ingest"]["update_requests"][0]["status"],
        json!("queued")
    );
    assert_eq!(
        trace["online_evidence_card_ingest"]["jobs"][0]["update_request_id"],
        json!(request.update_request_id)
    );
    assert_eq!(
        trace["online_evidence_card_ingest"]["jobs"][0]["status"],
        json!("queued")
    );
    assert_eq!(
        trace["online_learning_candidates"]["candidate_ids"]["evidence"]["update_request"],
        json!([request.update_request_id])
    );
    assert_eq!(
        trace["online_learning_candidates"]["evidence_candidates"]["update_requests"][0]["status"],
        json!("queued")
    );
    assert!(trace["audit_events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["event_type"] == "online_evidence_card_update_requested")
    }));
    remove_sqlite_file_set(&db_path);
}

#[test]
fn admin_trace_includes_online_learning_promotion_manifests() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-trace-promotion-manifest");
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let conn = runtime_store.open_connection().expect("runtime db opens");
    let trace_id = "trace-admin-promotion-manifest-test";
    runtime_store
        .online_learning_prompt_candidates_for_trace(trace_id, 1)
        .expect("prompt candidate schema initialized");
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO online_learning_prompt_candidates (
            candidate_id, target_profile, operation, failure_pattern,
            proposed_change_summary, expected_regression_cases_json, risk_level,
            status, observation_count, first_observed_at, last_observed_at,
            created_at, updated_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'staged', 1, ?8, ?8, ?8, ?8, ?9)",
        params![
            "prompt-candidate-a",
            "honglou-main",
            "draft_answer",
            "prompt_boundary_issue",
            "clarify evidence-anchor output contract",
            serde_json::to_string(&json!([{
                "suite_ref": "conversation_cases.small20",
                "case_id": "small20_evidence_followup_shixiangyun_commentary"
            }]))
            .expect("regression json"),
            "medium",
            &now,
            tonglingyu_runtime::ONLINE_LEARNING_PROMPT_CANDIDATE_SCHEMA_VERSION,
        ],
    )
    .expect("prompt candidate inserts");
    conn.execute(
        "INSERT INTO online_learning_prompt_candidate_observations (
            observation_id, candidate_id, trace_id, package_id, step_id,
            source_ref_json, review_decision_json, observed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            "prompt-candidate-observation-a",
            "prompt-candidate-a",
            trace_id,
            "pkg-admin-prompt-candidate-test",
            "step-draft-answer",
            serde_json::to_string(&json!({
                "trace_id": trace_id,
                "profile": "honglou-main",
                "operation": "draft_answer"
            }))
            .expect("source ref json"),
            serde_json::to_string(&json!({
                "status": "needs_revision",
                "failure_pattern": "prompt_boundary_issue"
            }))
            .expect("review decision json"),
            &now,
            tonglingyu_runtime::ONLINE_LEARNING_PROMPT_CANDIDATE_SCHEMA_VERSION,
        ],
    )
    .expect("prompt candidate observation inserts");
    let manifest = tonglingyu_runtime::record_online_learning_promotion_manifest(
        &conn,
        tonglingyu_runtime::PromotionManifestInput {
            artifact_kind: "prompt".to_string(),
            candidate_ids: vec!["prompt-candidate-a".to_string()],
            source_trace_refs: vec![trace_id.to_string()],
            source_span_refs: json!([]),
            rule_diff_refs: json!([{
                "catalog_name": "runtime_prompt_catalog",
                "target_ref": "answer_draft_contract",
                "before_sha256": "before",
                "after_sha256": "after"
            }]),
            merge_conflict_decision: json!({"status": "passed", "decision": "promote"}),
            target_ref: "runtime_prompt_catalog:answer_draft_contract".to_string(),
            expected_version_bump: "0.35.0".to_string(),
            regression_cases: json!([{
                "suite_ref": "conversation_cases.small20",
                "status": "passed",
                "case_count": 20,
                "passed_count": 20,
                "failed_count": 0,
                "skipped_count": 0
            }]),
            reviewer_policy: json!({
                "actor": "test-admin",
                "policy": "manual_review_plus_regression"
            }),
            dry_run_result: json!({"status": "passed", "errors": []}),
            rollback_ref: json!({
                "kind": "catalog_sha",
                "before_sha256": "before"
            }),
        },
    )
    .expect("promotion manifest");

    let trace = load_trace(&db_path, trace_id)
        .expect("trace loads")
        .expect("trace exists");
    assert_eq!(
        trace["online_learning_candidates"]["prompt_candidates"][0]["candidate_id"],
        json!("prompt-candidate-a")
    );
    assert_eq!(
        trace["online_learning_candidates"]["candidate_ids"]["prompt"],
        json!(["prompt-candidate-a"])
    );
    assert_eq!(
        trace["online_learning_candidates"]["promotion_manifests"][0]["promotion_batch_id"],
        manifest["promotion_batch_id"]
    );
    assert_eq!(
        trace["online_learning_candidates"]["promotion_manifests"][0]["artifact_kind"],
        json!("prompt")
    );
    remove_sqlite_file_set(&db_path);
}

#[test]
fn admin_trace_includes_deterministic_rule_candidate_observations() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-trace-deterministic-rule-candidate");
    let conn = open_db(&db_path).expect("db opens");
    let trace_id = "trace-admin-deterministic-rule-candidate";
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: "紫鹃照管过史湘云吗？".to_string(),
    }];
    create_context_for_request(
        &conn,
        ContextRequestInput {
            trace_id,
            model_id: DEFAULT_MODEL_ID,
            external_user_ref: "deterministic-rule-candidate-user",
            external_session_id: "deterministic-rule-candidate-chat",
            external_message_id: "deterministic-rule-candidate-message",
            question: "紫鹃照管过史湘云吗？",
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
    )
    .expect("context creates deterministic rule candidate");
    drop(conn);

    let trace = load_trace(&db_path, trace_id)
        .expect("trace loads")
        .expect("trace exists");
    assert_eq!(
        trace["online_learning_candidates"]["rule_candidates"][0]["candidate_type"],
        json!("predicate_alias")
    );
    assert_eq!(
        trace["online_learning_candidates"]["rule_candidates"][0]["primary_term"],
        json!("照管")
    );
    assert_eq!(
        trace["online_learning_candidates"]["rule_candidates"][0]["validator_audit"]["profile_id"],
        json!("deterministic_question_frame")
    );
    assert_eq!(
        trace["online_learning_candidates"]["candidate_ids"]["rule"]
            .as_array()
            .expect("rule candidate ids")
            .len(),
        1
    );
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn admin_can_run_online_evidence_card_worker_once() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-online-card-worker");
    seed_runtime_chat_source(&db_path);
    let state = Arc::new(test_app_state(db_path.clone()));
    let conn = state
        .runtime_store
        .open_connection()
        .expect("runtime db opens");
    tonglingyu_runtime::create_online_evidence_card_update_request(
        &conn,
        OnlineEvidenceCardUpdateRequestInput {
            trace_id: "trace-admin-online-card-worker-test".to_string(),
            session_id: None,
            resolved_question: "尤三姐最后".to_string(),
            question_frame: None,
            coverage_gap_reason: "package_coverage_partial".to_string(),
            source_scope_policy: json!({"scope": "test"}),
            recall_advice_ref: None,
        },
    )
    .expect("online evidence card update request created");

    let response = online_evidence_card_worker_run_endpoint(
        State(state),
        admin_headers(),
        Json(OnlineEvidenceCardWorkerRunRequest {
            actor: None,
            limit: Some(5),
            retrieval_limit: Some(5),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_str(&response_text(response).await).expect("worker response json");
    assert_eq!(body["processed_count"], json!(1));
    assert_eq!(body["failed_count"], json!(0));
    let stats = TonglingyuRuntimeStore::new(db_path.clone())
        .online_evidence_card_ingest_stats()
        .expect("ingest stats");
    assert_eq!(stats["update_requests"]["by_status"]["completed"], json!(1));
    assert_eq!(stats["jobs"]["by_status"]["completed"], json!(1));
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn admin_answer_quality_observation_endpoints_record_trace_linked_route() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-answer-quality");
    remove_sqlite_file_set(&db_path);
    let state = Arc::new(test_app_state(db_path.clone()));

    let create_response = create_answer_quality_observation_endpoint(
        State(state.clone()),
        admin_headers(),
        Json(AnswerQualityObservationRequest {
            actor: None,
            source_entity_type: "eval_miss".to_string(),
            source_entity_id: "conversation-eval:run-quality:case-question-frame".to_string(),
            run_id: Some("run-quality".to_string()),
            case_id: Some("case-question-frame".to_string()),
            trace_id: Some("trace-answer-quality".to_string()),
            package_id: Some("pkg-answer-quality".to_string()),
            severity: "medium".to_string(),
            failure_tags: vec!["question_frame_mismatch:object".to_string()],
            user_visible_issue: Some("追问对象绑定错误。".to_string()),
            runtime_issue: Some("question_frame.object mismatch".to_string()),
            suggested_owner: Some("question_resolution".to_string()),
            suggested_action: Some("检查结构化问句理解规则。".to_string()),
        }),
    )
    .await;
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_body: Value =
        serde_json::from_str(&response_text(create_response).await).expect("create json");
    let observation_id = create_body["observation"]["observation_id"]
        .as_str()
        .expect("observation id")
        .to_string();
    assert_eq!(
        create_body["observation"]["action_route"],
        json!("context_rule_candidate")
    );
    assert_eq!(
        create_body["observation"]["active_path_visible"],
        json!(false)
    );
    let action_item_id = create_body["observation"]["action_items"][0]["action_item_id"]
        .as_str()
        .expect("action item id")
        .to_string();
    assert_eq!(
        create_body["observation"]["action_items"][0]["action_type"],
        json!("review_context_rule_candidate_gap")
    );
    assert_eq!(
        create_body["observation"]["action_items"][0]["status"],
        json!("queued")
    );

    let mut list_params = BTreeMap::new();
    list_params.insert(
        "action_route".to_string(),
        "context_rule_candidate".to_string(),
    );
    let list_response = answer_quality_observations_endpoint(
        State(state.clone()),
        admin_headers(),
        Query(list_params),
    )
    .await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body: Value =
        serde_json::from_str(&response_text(list_response).await).expect("list json");
    assert_eq!(list_body["items"].as_array().expect("items").len(), 1);
    assert_eq!(list_body["active_path_visible"], json!(false));

    let read_response = answer_quality_observation_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(observation_id),
    )
    .await;
    assert_eq!(read_response.status(), StatusCode::OK);
    let read_body: Value =
        serde_json::from_str(&response_text(read_response).await).expect("read json");
    assert_eq!(
        read_body["observation"]["events"][0]["event_type"],
        json!("observed")
    );
    assert_eq!(
        read_body["observation"]["action_items"][0]["action_item_id"],
        json!(action_item_id)
    );

    let mut action_list_params = BTreeMap::new();
    action_list_params.insert("status".to_string(), "queued".to_string());
    action_list_params.insert(
        "action_type".to_string(),
        "review_context_rule_candidate_gap".to_string(),
    );
    let action_list_response = answer_quality_actions_endpoint(
        State(state.clone()),
        admin_headers(),
        Query(action_list_params),
    )
    .await;
    assert_eq!(action_list_response.status(), StatusCode::OK);
    let action_list_body: Value =
        serde_json::from_str(&response_text(action_list_response).await).expect("action list json");
    assert_eq!(
        action_list_body["items"][0]["action_item_id"],
        json!(action_item_id)
    );
    assert_eq!(action_list_body["active_path_visible"], json!(false));

    let action_read_response = answer_quality_action_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(action_item_id.clone()),
    )
    .await;
    assert_eq!(action_read_response.status(), StatusCode::OK);
    let action_read_body: Value =
        serde_json::from_str(&response_text(action_read_response).await).expect("action read json");
    assert_eq!(
        action_read_body["action_item"]["events"][0]["event_type"],
        json!("queued")
    );

    let transition_response = transition_answer_quality_action_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(action_item_id.clone()),
        Json(AnswerQualityActionTransitionRequest {
            actor: Some("quality-reviewer".to_string()),
            action: "link".to_string(),
            reason: "linked to staged context rule candidate".to_string(),
            linked_entity_type: Some("rule_candidate".to_string()),
            linked_entity_id: Some("rule-candidate-admin-test".to_string()),
        }),
    )
    .await;
    assert_eq!(transition_response.status(), StatusCode::OK);
    let transition_body: Value =
        serde_json::from_str(&response_text(transition_response).await).expect("transition json");
    assert_eq!(transition_body["action_item"]["status"], json!("linked"));
    assert_eq!(
        transition_body["action_item"]["linked_entity_type"],
        json!("rule_candidate")
    );

    let trace_response = trace_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath("trace-answer-quality".to_string()),
    )
    .await;
    assert_eq!(trace_response.status(), StatusCode::OK);
    let trace_body: Value =
        serde_json::from_str(&response_text(trace_response).await).expect("trace json");
    assert_eq!(
        trace_body["answer_quality_observations"][0]["case_id"],
        json!("case-question-frame")
    );
    assert_eq!(
        trace_body["answer_quality_actions"][0]["action_item_id"],
        json!(action_item_id)
    );
    let conn = open_db(&db_path).expect("open db for quality counts");
    assert_eq!(
        table_count(&conn, "answer_quality_observations").expect("observation count"),
        1
    );
    assert_eq!(
        table_count(&conn, "answer_quality_action_items").expect("action count"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "answer_quality_observation_admin_create"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "answer_quality_observation_admin_list"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "answer_quality_observation_admin_read"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "answer_quality_action_admin_list"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "answer_quality_action_admin_read"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "answer_quality_action_admin_transition"),
        1
    );
    remove_sqlite_file_set(&db_path);
}

#[test]
fn metrics_include_bounded_retrieval_failure_counts() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-metrics-rqa");
    let trace_id = "trace-admin-metrics-test";
    let case = eval_case_fixture("rqa-admin-failure");
    let failure_id = seed_eval_retrieval_failure(&db_path, trace_id);
    let conn = open_db(&db_path).expect("gateway db opens");
    tonglingyu_runtime::init_knowledge_base_schema(&conn).expect("kb schema exists");
    let state = test_app_state(db_path.clone());

    let metrics = load_metrics(&state).expect("metrics load");
    let prometheus = load_prometheus_metrics(&state).expect("prometheus metrics load");
    let metrics_text = serde_json::to_string(&metrics).expect("metrics serializes");

    assert_eq!(metrics["counts"]["retrieval_failures"], json!(1));
    assert_eq!(metrics["counts"]["governance_tasks"], json!(1));
    assert_eq!(
        metrics["rqa"]["schema_version"],
        RETRIEVAL_FAILURE_SCHEMA_VERSION
    );
    assert_eq!(
        metrics["runtime_rule_catalogs"]["object"],
        json!("tonglingyu.runtime_rule_catalog_metadata")
    );
    assert!(
        metrics["runtime_rule_catalogs"]["query_expansions"]["catalog_version"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
    );
    assert!(
        metrics["runtime_rule_catalogs"]["answer_rules"]["catalog_version"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
    );
    assert_eq!(
        metrics["rqa"]["retrieval_failures"]["by_status"]["open"],
        json!(1)
    );
    assert_eq!(
        metrics["rqa"]["retrieval_failures"]["by_type"]["quality_report_not_passed"],
        json!(1)
    );
    assert_eq!(
        metrics["rqa"]["governance_tasks"]["by_status"]["open"],
        json!(1)
    );
    assert_eq!(
        metrics["rqa"]["governance_tasks"]["by_priority"]["p0"],
        json!(1)
    );
    assert!(prometheus.contains("tonglingyu_retrieval_failures_total 1"));
    assert!(prometheus.contains("tonglingyu_governance_tasks_total 1"));
    assert!(
        prometheus.contains("tonglingyu_retrieval_failures_by_status_total{status=\"open\"} 1")
    );
    assert!(prometheus.contains(
        "tonglingyu_retrieval_failures_by_type_total{failure_type=\"quality_report_not_passed\"} 1"
    ));
    assert!(prometheus.contains("tonglingyu_governance_tasks_by_status_total{status=\"open\"} 1"));
    assert!(prometheus.contains("tonglingyu_gateway_info{agent_runtime_mode="));
    assert!(prometheus.contains("rate_limit_per_minute=\"120\""));
    assert!(prometheus.contains("max_body_bytes=\"1048576\""));
    assert!(!prometheus.contains("main_profile="));
    assert!(!prometheus.contains("reviewer_profile="));
    for leaked_value in [
        trace_id,
        "pkg-rqa-admin-failure",
        failure_id.as_str(),
        case.question,
    ] {
        assert!(!metrics_text.contains(leaked_value));
        assert!(!prometheus.contains(leaked_value));
    }
    for forbidden_label in ["trace_id=", "package_id=", "question=", "query=", "user="] {
        assert!(!prometheus.contains(forbidden_label));
    }
    assert!(!metrics_text.contains("\"trace_id\""));
    assert!(!metrics_text.contains("\"package_id\""));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[test]
fn retrieval_failure_list_filter_rejects_unknown_fields() {
    let mut params = BTreeMap::new();
    params.insert("trace_id".to_string(), "trace-leak-attempt".to_string());

    let error = retrieval_failure_list_input(&params).expect_err("unknown filter must fail closed");

    assert!(
        error
            .to_string()
            .contains("unsupported retrieval failure filter")
    );
}

#[test]
fn retrieval_failure_update_detects_stale_updated_at() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-cas");
    let failure_id = seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-cas");
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let failure = runtime_store
        .read_retrieval_failure(&failure_id, RetrievalFailureView::AdminDetail)
        .expect("failure reads")
        .expect("failure exists");
    let updated_at = failure["updated_at"]
        .as_str()
        .expect("updated_at is present")
        .to_string();

    runtime_store
        .update_retrieval_failure_status_checked(
            &failure_id,
            "in_review",
            Some("admin-1"),
            Some("reviewing"),
            Some(&updated_at),
        )
        .expect("first update succeeds")
        .expect("failure updated");
    let stale = runtime_store
        .update_retrieval_failure_status_checked(
            &failure_id,
            "resolved",
            Some("admin-1"),
            Some("fixed"),
            Some(&updated_at),
        )
        .expect_err("stale update must conflict");

    assert!(stale.to_string().contains("update conflict"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_endpoint_denies_unauthorized_and_audits() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-auth-denial");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response =
        retrieval_failures_endpoint(State(state), HeaderMap::new(), Query(BTreeMap::new())).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(audit_event_count(&db_path, "rqa_admin_access_denied"), 1);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_endpoint_denies_gateway_key_subject_and_audits() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-user-denial");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer gateway-key".parse().unwrap());
    headers.insert("x-tonglingyu-subject", "user-1".parse().unwrap());

    let response = retrieval_failures_endpoint(State(state), headers, Query(BTreeMap::new())).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(audit_event_count(&db_path, "rqa_admin_access_denied"), 1);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_endpoint_rate_limit_denial_is_audited() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-rate-limit");
    let mut state = test_app_state(db_path.clone());
    state.admin_rate_limiter = Arc::new(GatewayRateLimiter::new(1, Duration::from_secs(60)));
    let state = Arc::new(state);
    let headers = admin_headers();

    let first = retrieval_failures_endpoint(
        State(state.clone()),
        headers.clone(),
        Query(BTreeMap::new()),
    )
    .await;
    let second = retrieval_failures_endpoint(State(state), headers, Query(BTreeMap::new())).await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(audit_event_count(&db_path, "rqa_admin_access_denied"), 1);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn admin_access_denial_endpoint_records_role_denial_audit() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-role-denial");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut headers = admin_headers();
    headers.insert("x-tonglingyu-subject", "user-1".parse().unwrap());

    let response = admin_access_denial_endpoint(
        State(state),
        headers,
        Json(AdminAccessDenialRequest {
            action: Some("metrics".to_string()),
            denial: "role_denied".to_string(),
            model: Some(DEFAULT_MODEL_ID.to_string()),
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(audit_event_count(&db_path, "rqa_admin_access_denied"), 1);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn memory_admin_endpoints_collect_transition_and_enable_read_with_policy() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-memory");
    let state = Arc::new(test_app_state(db_path.clone()));

    let conn = open_db(&db_path).expect("db opens");
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: "我喜欢回答里多引用原文。".to_string(),
    }];
    let context = create_context_for_request(
        &conn,
        ContextRequestInput {
            trace_id: "trace-admin-memory",
            model_id: DEFAULT_MODEL_ID,
            external_user_ref: "memory-user",
            external_session_id: "memory-chat",
            external_message_id: "memory-message-1",
            question: "我喜欢回答里多引用原文。",
            messages: &messages,
            history_over_limit: false,
            max_messages: 40,
        },
    )
    .expect("context created");
    append_final_response(
        &conn,
        FinalResponseJournalInput {
            trace_id: "trace-admin-memory",
            user_session_id: &context.user_session_id,
            interaction_context_id: &context.interaction_context_id,
            context_pack_id: &context.context_pack_id,
            external_message_id: "memory-message-1",
            package_id: Some("pkg-admin-memory"),
            response: &json!({"status": "ok"}),
        },
    )
    .expect("final response journal");

    let collector = memory_collector_run_endpoint(
        State(state.clone()),
        admin_headers(),
        Json(MemoryCollectorRunRequest {
            trigger: Some("admin_manual".to_string()),
            limit: Some(20),
            dry_run: Some(false),
            trace_id: Some("trace-admin-memory".to_string()),
            llm_extraction_probe: Some(json!({
                "schema_version": "scoped-memory-llm-filter-v1",
                "is_long_term_memory": true,
                "is_temporary_instruction": false,
                "is_quoted_or_third_party": false,
                "has_contradiction": false,
                "scope_type": "user_private",
                "candidate_type": "retrieval_preference",
                "confidence": 0.84,
                "sensitivity": "low",
                "risk_flags": [],
                "ttl_hint": "180d",
                "exclusion_flags": [],
            })),
        }),
    )
    .await;
    assert_eq!(collector.status(), StatusCode::OK);
    let collector_body: Value =
        serde_json::from_str(&response_text(collector).await).expect("collector json");
    assert_eq!(collector_body["candidate_count"], json!(1));
    assert_eq!(
        collector_body["llm_extraction_probe_validation"]["status"],
        json!("pending")
    );

    let mut list_params = BTreeMap::new();
    list_params.insert("status".to_string(), "pending".to_string());
    let list =
        memory_candidates_endpoint(State(state.clone()), admin_headers(), Query(list_params)).await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body: Value =
        serde_json::from_str(&response_text(list).await).expect("candidate list json");
    let candidate_id = list_body["items"][0]["candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();

    let approve = memory_candidate_transition_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.clone()),
        Json(MemoryCandidateTransitionRequest {
            action: "approve".to_string(),
            reason: Some("admin approved".to_string()),
            candidate_type: None,
            sensitivity: None,
            merge_target_candidate_id: None,
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(approve.status(), StatusCode::OK);
    let promote = memory_candidate_transition_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id),
        Json(MemoryCandidateTransitionRequest {
            action: "promote".to_string(),
            reason: Some("admin promoted for card lifecycle test".to_string()),
            candidate_type: None,
            sensitivity: None,
            merge_target_candidate_id: None,
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(promote.status(), StatusCode::OK);

    let mut card_params = BTreeMap::new();
    card_params.insert("status".to_string(), "active".to_string());
    let cards =
        memory_cards_endpoint(State(state.clone()), admin_headers(), Query(card_params)).await;
    assert_eq!(cards.status(), StatusCode::OK);
    let cards_body: Value =
        serde_json::from_str(&response_text(cards).await).expect("memory card list json");
    assert_eq!(cards_body["items"][0]["read_enabled"], json!(false));
    let memory_card_id = cards_body["items"][0]["memory_card_id"]
        .as_str()
        .expect("memory card id")
        .to_string();

    let enable = memory_card_transition_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(memory_card_id.clone()),
        Json(MemoryCardTransitionRequest {
            action: "enable_read".to_string(),
            reason: Some("manual review approved read enablement".to_string()),
        }),
    )
    .await;
    assert_eq!(enable.status(), StatusCode::OK);
    let enable_body: Value =
        serde_json::from_str(&response_text(enable).await).expect("enable json");
    assert_eq!(enable_body["memory_card"]["read_enabled"], json!(true));
    assert_eq!(enable_body["read_path_enabled"], json!(true));

    let read_context = create_context_for_request(
        &conn,
        ContextRequestInput {
            trace_id: "trace-admin-memory-read-enabled",
            model_id: DEFAULT_MODEL_ID,
            external_user_ref: "memory-user",
            external_session_id: "memory-chat",
            external_message_id: "memory-message-2",
            question: "介绍贾宝玉",
            messages: &[ContextMessage {
                role: "user".to_string(),
                content: "介绍贾宝玉".to_string(),
            }],
            history_over_limit: false,
            max_messages: 40,
        },
    )
    .expect("context reads manual enabled memory");
    assert_eq!(
        read_context.context_pack["memory_read_refs"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let unauthorized = memory_cards_endpoint(
        State(state.clone()),
        gateway_headers("memory-user"),
        Query(BTreeMap::new()),
    )
    .await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    conn.execute(
        "DELETE FROM memory_policy_decisions WHERE memory_card_id = ?1",
        params![memory_card_id],
    )
    .expect("remove read policy decision");
    let fail_closed = create_context_for_request(
        &conn,
        ContextRequestInput {
            trace_id: "trace-admin-memory-fail-closed",
            model_id: DEFAULT_MODEL_ID,
            external_user_ref: "memory-user",
            external_session_id: "memory-chat",
            external_message_id: "memory-message-2",
            question: "介绍贾宝玉",
            messages: &[ContextMessage {
                role: "user".to_string(),
                content: "介绍贾宝玉".to_string(),
            }],
            history_over_limit: false,
            max_messages: 40,
        },
    )
    .expect_err("read_enabled cards must fail closed");
    assert!(fail_closed.to_string().contains("without policy decision"));
    assert_eq!(audit_event_count(&db_path, "memory_collector_admin_run"), 1);
    assert_eq!(
        audit_event_count(&db_path, "memory_candidate_admin_transition"),
        2
    );
    assert_eq!(
        audit_event_count(&db_path, "memory_card_admin_transition"),
        1
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_read_not_found_is_redacted() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-not-found");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = retrieval_failure_endpoint(
        State(state),
        admin_headers(),
        AxumPath("rf-does-not-exist".to_string()),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_admin_read"),
        1
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_read_success_writes_access_audit() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-read-audit");
    let failure_id = seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-read-audit");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response =
        retrieval_failure_endpoint(State(state), admin_headers(), AxumPath(failure_id)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_admin_read"),
        1
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_update_repeated_payload_is_idempotent() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-update-idempotent");
    let failure_id = seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-idempotent");
    let state = Arc::new(test_app_state(db_path.clone()));

    let first = update_retrieval_failure_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(failure_id.clone()),
        Json(RetrievalFailureUpdateRequest {
            human_review_status: "in_review".to_string(),
            reviewer: Some("admin-1".to_string()),
            review_note: Some("reviewing".to_string()),
            if_match_updated_at: None,
        }),
    )
    .await;
    let second = update_retrieval_failure_endpoint(
        State(state),
        admin_headers(),
        AxumPath(failure_id),
        Json(RetrievalFailureUpdateRequest {
            human_review_status: "in_review".to_string(),
            reviewer: Some("admin-1".to_string()),
            review_note: Some("reviewing".to_string()),
            if_match_updated_at: None,
        }),
    )
    .await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_status_updated"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_admin_update"),
        2
    );
    let runtime_update_payload =
        latest_audit_event_payload(&db_path, "retrieval_failure_status_updated");
    assert_eq!(runtime_update_payload["previous_status"], "open");
    assert_eq!(runtime_update_payload["new_status"], "in_review");
    assert_eq!(
        runtime_update_payload["status_history"]["previous_status"],
        "open"
    );
    assert_eq!(
        runtime_update_payload["status_history"]["new_status"],
        "in_review"
    );
    assert!(
        runtime_update_payload["status_history"]["reason_sha256"]
            .as_str()
            .is_some()
    );
    assert!(
        runtime_update_payload["status_history"]["timestamp"]
            .as_str()
            .is_some()
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_update_denies_gateway_key_subject_and_audits() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-update-user-denial");
    let failure_id = seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-update-denial");
    let state = Arc::new(test_app_state(db_path.clone()));
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer gateway-key".parse().unwrap());
    headers.insert("x-tonglingyu-subject", "user-1".parse().unwrap());

    let response = update_retrieval_failure_endpoint(
        State(state),
        headers,
        AxumPath(failure_id),
        Json(RetrievalFailureUpdateRequest {
            human_review_status: "resolved".to_string(),
            reviewer: Some("user-1".to_string()),
            review_note: Some("should be denied".to_string()),
            if_match_updated_at: None,
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(audit_event_count(&db_path, "rqa_admin_access_denied"), 1);
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_admin_update"),
        0
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn retrieval_failure_update_conflict_writes_access_audit() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-update-conflict");
    let failure_id = seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-conflict");
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let failure = runtime_store
        .read_retrieval_failure(&failure_id, RetrievalFailureView::AdminDetail)
        .expect("failure reads")
        .expect("failure exists");
    let stale_updated_at = failure["updated_at"]
        .as_str()
        .expect("updated_at is present")
        .to_string();
    runtime_store
        .update_retrieval_failure_status_checked(
            &failure_id,
            "in_review",
            Some("admin-1"),
            Some("reviewing"),
            Some(&stale_updated_at),
        )
        .expect("first update succeeds");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = update_retrieval_failure_endpoint(
        State(state),
        admin_headers(),
        AxumPath(failure_id),
        Json(RetrievalFailureUpdateRequest {
            human_review_status: "resolved".to_string(),
            reviewer: Some("admin-1".to_string()),
            review_note: Some("fixed".to_string()),
            if_match_updated_at: Some(stale_updated_at),
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_admin_update"),
        1
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn governance_task_endpoints_create_list_update_and_audit() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-governance-task");
    let failure_id = seed_eval_retrieval_failure(&db_path, "trace-admin-governance-task");
    let state = Arc::new(test_app_state(db_path.clone()));

    let create = create_governance_task_from_failure_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(failure_id.clone()),
        Json(GovernanceTaskCreateRequest {
            task_type: None,
            priority: None,
            proposed_fix: None,
            agent_cluster_key: None,
        }),
    )
    .await;
    assert_eq!(create.status(), StatusCode::OK);

    let mut params = BTreeMap::new();
    params.insert("status".to_string(), "open".to_string());
    params.insert("source_failure_id".to_string(), failure_id.clone());
    let list =
        governance_tasks_endpoint(State(state.clone()), admin_headers(), Query(params)).await;
    assert_eq!(list.status(), StatusCode::OK);

    let create_trace = create_governance_task_endpoint(
        State(state.clone()),
        admin_headers(),
        Json(GovernanceTaskManualCreateRequest {
            source_entity_type: "trace".to_string(),
            source_entity_id: "trace-admin-governance-task".to_string(),
            trace_id: None,
            package_id: None,
            task_type: Some("expert_review".to_string()),
            priority: Some("p0".to_string()),
            proposed_fix: Some("request expert review".to_string()),
            agent_cluster_key: None,
        }),
    )
    .await;
    assert_eq!(create_trace.status(), StatusCode::OK);

    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let tasks = runtime_store
        .list_governance_tasks(KnowledgeGovernanceTaskListInput {
            status: Some("open".to_string()),
            task_type: None,
            priority: Some("p0".to_string()),
            source_failure_id: Some(failure_id),
            source_entity_type: None,
            source_entity_id: None,
            limit: 10,
            offset: 0,
        })
        .expect("list governance tasks");
    let task_id = tasks.items[0]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    let updated_at = tasks.items[0]["updated_at"]
        .as_str()
        .expect("updated_at")
        .to_string();
    let update = update_governance_task_endpoint(
        State(state),
        admin_headers(),
        AxumPath(task_id),
        Json(GovernanceTaskUpdateRequest {
            status: "accepted".to_string(),
            reviewer: Some("admin-1".to_string()),
            review_note: Some("accepted for source patch".to_string()),
            evidence_ref: Some("source://review-note/001".to_string()),
            if_match_updated_at: Some(updated_at),
        }),
    )
    .await;

    assert_eq!(update.status(), StatusCode::OK);
    assert_eq!(
        audit_event_count(&db_path, "governance_task_admin_create"),
        2
    );
    assert_eq!(audit_event_count(&db_path, "governance_task_admin_list"), 1);
    assert_eq!(
        audit_event_count(&db_path, "governance_task_admin_update"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "governance_task_status_updated"),
        1
    );
    let runtime_update_payload =
        latest_audit_event_payload(&db_path, "governance_task_status_updated");
    assert_eq!(runtime_update_payload["previous_status"], "open");
    assert_eq!(runtime_update_payload["new_status"], "accepted");
    assert_eq!(
        runtime_update_payload["status_history"]["previous_status"],
        "open"
    );
    assert_eq!(
        runtime_update_payload["status_history"]["new_status"],
        "accepted"
    );
    assert!(
        runtime_update_payload["status_history"]["reason_sha256"]
            .as_str()
            .is_some()
    );
    assert!(
        runtime_update_payload["status_history"]["timestamp"]
            .as_str()
            .is_some()
    );
    let admin_update_payload = latest_audit_event_payload(&db_path, "governance_task_admin_update");
    assert_eq!(admin_update_payload["actor"], "admin-1");
    assert_eq!(
        admin_update_payload["payload"]["status_history"]["previous_status"],
        "open"
    );
    assert_eq!(
        admin_update_payload["payload"]["status_history"]["new_status"],
        "accepted"
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn knowledge_patch_proposal_endpoint_creates_review_task_without_fact_mutation() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-knowledge-patch-proposal");
    let package = seed_owned_gateway_package(&db_path, "user-1");
    let conn = open_db(&db_path).expect("db opens");
    tonglingyu_runtime::init_knowledge_base_schema(&conn).expect("kb schema");
    let alias_count_before = table_count(&conn, "aliases").expect("alias count before");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = create_knowledge_patch_proposal_endpoint(
        State(state),
        admin_headers(),
        Json(KnowledgePatchProposalCreateRequest {
            proposal_type: "alias".to_string(),
            trace_id: Some(package.trace_id.clone()),
            package_id: Some(package.package_id.clone()),
            source_ref: Some(format!("package:{}", package.package_id)),
            payload: json!({
                "alias": "灵玉",
                "target_ref": "person:baoyu",
                "rationale": "admin proposed alias must be human reviewed",
            }),
            priority: Some("p1".to_string()),
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_str(&response_text(response).await).expect("proposal response json");
    assert_eq!(
        body["object"],
        json!("tonglingyu.knowledge_patch_proposal_admin_create")
    );
    assert_eq!(
        body["schema_version"],
        json!(KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION)
    );
    assert_eq!(body["result"]["direct_fact_mutation"], json!(false));
    assert_eq!(body["result"]["proposal"]["proposal_type"], json!("alias"));
    assert_eq!(
        body["result"]["task"]["source_entity_type"],
        json!("knowledge_patch_proposal")
    );
    assert_eq!(
        body["result"]["task"]["task_type"],
        json!("alias_term_review")
    );
    assert_eq!(
        table_count(&conn, "aliases").expect("alias count after"),
        alias_count_before
    );
    assert_eq!(
        audit_event_count(&db_path, "knowledge_patch_proposal_admin_create"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "knowledge_patch_proposal_created"),
        1
    );
    assert_eq!(audit_event_count(&db_path, "governance_task_created"), 1);
    let runtime_audit_payload: String = conn
            .query_row(
                "SELECT payload_json FROM audit_events WHERE event_type = 'knowledge_patch_proposal_created' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("runtime proposal audit payload");
    assert!(!runtime_audit_payload.contains("灵玉"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn knowledge_item_admin_endpoints_list_read_and_audit_state_boundary() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-knowledge-item");
    let state = Arc::new(test_app_state(db_path.clone()));
    let created = state
        .runtime_store
        .create_knowledge_item(KnowledgeItemCreateInput {
            kind: KnowledgeItemKind::Alias,
            initial_state: KnowledgeState::Candidate,
            source_refs: vec!["source://wikisource/chapter/admin-item".to_string()],
            evidence_refs: vec!["block://wikisource/admin-item".to_string()],
            payload: json!({
                "alias": "stone",
                "person_id": "p-baoyu",
                "scope": "admin endpoint test",
            }),
            schema_version: None,
            trace_id: "trace-admin-knowledge-item".to_string(),
            actor: "system-calibration".to_string(),
            reason: "candidate created for admin endpoint test".to_string(),
        })
        .expect("knowledge item creates");
    let updated = state
        .runtime_store
        .update_knowledge_item_state(
            &created.item_id,
            KnowledgeItemStateUpdateInput {
                new_state: KnowledgeState::SystemCalibrated,
                trace_id: "trace-admin-knowledge-item".to_string(),
                actor: "calibration-runner".to_string(),
                reason: "evidence judge passed for admin endpoint test".to_string(),
                evidence_refs: vec!["block://wikisource/admin-item".to_string()],
                expected_state_version: created.state_version,
            },
        )
        .expect("knowledge item state updates")
        .expect("knowledge item exists");

    let mut params = BTreeMap::new();
    params.insert("kind".to_string(), "alias".to_string());
    params.insert("state".to_string(), "system_calibrated".to_string());
    let list_response =
        knowledge_items_endpoint(State(state.clone()), admin_headers(), Query(params)).await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body: Value =
        serde_json::from_str(&response_text(list_response).await).expect("list response json");
    assert_eq!(
        list_body["object"],
        json!("tonglingyu.knowledge_item_admin_list")
    );
    assert_eq!(
        list_body["schema_version"],
        json!(KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION)
    );
    assert_eq!(
        list_body["list"]["items"][0]["item_id"],
        json!(updated.item_id)
    );
    assert_eq!(list_body["list"]["items"][0]["kind"], json!("alias"));
    assert_eq!(
        list_body["list"]["items"][0]["state"],
        json!("system_calibrated")
    );
    assert_eq!(list_body["list"]["items"][0]["state_version"], json!(2));

    let read_response = knowledge_item_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(updated.item_id.clone()),
    )
    .await;
    assert_eq!(read_response.status(), StatusCode::OK);
    let read_body: Value =
        serde_json::from_str(&response_text(read_response).await).expect("read response json");
    assert_eq!(
        read_body["object"],
        json!("tonglingyu.knowledge_item_admin_read")
    );
    assert_eq!(read_body["item"]["state"], json!("system_calibrated"));
    assert_eq!(read_body["item"]["payload"]["alias"], json!("stone"));

    let mut invalid_params = BTreeMap::new();
    invalid_params.insert("state".to_string(), "accepted".to_string());
    let invalid_response =
        knowledge_items_endpoint(State(state), admin_headers(), Query(invalid_params)).await;
    assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(audit_event_count(&db_path, "knowledge_item_admin_list"), 1);
    assert_eq!(audit_event_count(&db_path, "knowledge_item_admin_read"), 1);
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn rule_candidate_preflight_endpoint_marks_staged_candidate_ready_without_active_mutation() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rule-candidate-preflight");
    let state = Arc::new(test_app_state(db_path.clone()));

    let conn = open_db(&db_path).expect("db opens");
    let messages = vec![ContextMessage {
        role: "user".to_string(),
        content: "史湘云的结局呢？".to_string(),
    }];
    let context = create_context_for_request(
        &conn,
        ContextRequestInput {
            trace_id: "trace-rule-candidate-preflight",
            model_id: DEFAULT_MODEL_ID,
            external_user_ref: "rule-candidate-preflight-user",
            external_session_id: "rule-candidate-preflight-chat",
            external_message_id: "rule-candidate-preflight-message",
            question: "史湘云的结局呢？",
            messages: &messages,
            history_over_limit: false,
            max_messages: 20,
        },
    )
    .expect("context creates");
    let now = now_rfc3339();
    let candidate_id = "rule-candidate-admin-preflight-test";
    let candidate_hash = format!("{}:{}", "entity_alias", hash_text("枕霞客"));
    conn.execute(
        "INSERT INTO rule_candidates (
            candidate_id, candidate_ref, candidate_hash, candidate_type, term_key, primary_term,
            status, conflict_status, active_rule_refs_json, observation_count, created_by,
            first_observed_at, last_observed_at, created_at, updated_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'staged', 'none', ?7, 1, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            candidate_id,
            "rule-candidate://tonglingyu/rule-candidate-admin-preflight-test",
            candidate_hash,
            "entity_alias",
            "枕霞客",
            "枕霞客",
            serde_json::to_string(&json!([])).expect("refs json"),
            "test_fixture",
            &now,
            &now,
            &now,
            &now,
            crate::rule_candidates::RULE_CANDIDATE_SCHEMA_VERSION,
        ],
    )
    .expect("rule candidate inserts");
    conn.execute(
        "INSERT INTO rule_candidate_observations (
            observation_id, candidate_id, trace_id, user_session_id, interaction_context_id,
            context_pack_id, external_message_id, source_question, resolved_question, reason,
            question_frame_json, validator_audit_json, observed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            "rule-candidate-observation-admin-preflight-test",
            candidate_id,
            "trace-rule-candidate-preflight",
            &context.user_session_id,
            &context.interaction_context_id,
            &context.context_pack_id,
            "rule-candidate-preflight-message",
            "史湘云的结局呢？",
            &context.resolved_question,
            "用户表达显示它可能是史湘云别名",
            serde_json::to_string(&context.context_pack["resolver"]["question_frame"])
                .expect("frame json"),
            serde_json::to_string(&json!({
                "schema_version": "test",
                "contract_accepted": true,
                "accepted_for_main": true,
                "rule_candidate_count": 1,
            }))
            .expect("audit json"),
            &now,
            crate::rule_candidates::RULE_CANDIDATE_SCHEMA_VERSION,
        ],
    )
    .expect("rule candidate observation inserts");

    let missing_evidence_response = rule_candidate_preflight_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.to_string()),
        Json(RuleCandidatePreflightRequest { actor: None }),
    )
    .await;
    assert_eq!(missing_evidence_response.status(), StatusCode::OK);
    let missing_evidence_body: Value =
        serde_json::from_str(&response_text(missing_evidence_response).await)
            .expect("missing evidence preflight response json");
    assert_eq!(missing_evidence_body["status"], json!("failed"));
    assert!(
        missing_evidence_body["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|error| error == "regression_evidence_check_failed")
    );

    let review_run_response = rule_candidate_review_run_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.to_string()),
        Json(RuleCandidateReviewRunRequest {
            actor: None,
            reopen_reason: Some("补充回归证据后重新进入 review-run".to_string()),
            suite_ref: "conversation_cases.small20".to_string(),
            report_ref: "eval://admin-rule-candidate-preflight/small20".to_string(),
            report_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            case_count: 20,
            passed_count: 20,
            failed_count: 0,
            skipped_count: Some(0),
            notes: Some("候选规则通过 small20 回归验证".to_string()),
        }),
    )
    .await;
    assert_eq!(review_run_response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_str(&response_text(review_run_response).await).expect("review-run json");
    assert_eq!(
        body["object"],
        json!("tonglingyu.rule_candidate_review_run")
    );
    assert_eq!(body["status"], json!("passed"));
    assert_eq!(body["active_path_visible"], json!(false));
    assert!(
        body["transition_run_id"]
            .as_str()
            .expect("transition run id")
            .starts_with("rule-candidate-transition-")
    );
    assert!(
        body["regression_evidence_id"]
            .as_str()
            .expect("regression evidence id")
            .starts_with("rule-candidate-regression-")
    );
    assert!(
        body["preflight_run_id"]
            .as_str()
            .expect("preflight run id")
            .starts_with("rule-candidate-preflight-")
    );
    assert_eq!(body["steps"][0]["name"], json!("review_boundary"));
    assert_eq!(body["steps"][1]["name"], json!("regression_evidence"));
    assert_eq!(body["steps"][2]["name"], json!("preflight"));
    assert_eq!(body["steps"][0]["action"], json!("reopen"));
    assert_eq!(
        body["steps"][2]["result"]["candidate_status"],
        json!("ready_for_review")
    );
    assert_eq!(
        crate::context_rules::latest_subject_in_text("枕霞客的结局")
            .expect("active ontology lookup"),
        None
    );
    let status: String = conn
        .query_row(
            "SELECT status FROM rule_candidates WHERE candidate_id = ?1",
            params![candidate_id],
            |row| row.get(0),
        )
        .expect("candidate status");
    assert_eq!(status, "ready_for_review");

    let mut list_params = BTreeMap::new();
    list_params.insert("status".to_string(), "ready_for_review".to_string());
    list_params.insert("candidate_type".to_string(), "entity_alias".to_string());
    let list_response =
        rule_candidates_endpoint(State(state.clone()), admin_headers(), Query(list_params)).await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body: Value =
        serde_json::from_str(&response_text(list_response).await).expect("list response json");
    assert_eq!(list_body["object"], json!("tonglingyu.rule_candidate_list"));
    assert_eq!(list_body["items"].as_array().expect("items").len(), 1);
    assert_eq!(list_body["items"][0]["candidate_id"], json!(candidate_id));
    assert_eq!(list_body["active_path_visible"], json!(false));

    let read_response = rule_candidate_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.to_string()),
    )
    .await;
    assert_eq!(read_response.status(), StatusCode::OK);
    let read_body: Value =
        serde_json::from_str(&response_text(read_response).await).expect("read response json");
    assert_eq!(
        read_body["object"],
        json!("tonglingyu.rule_candidate_review_packet")
    );
    assert_eq!(read_body["candidate"]["status"], json!("ready_for_review"));
    assert_eq!(
        read_body["observations"][0]["validator_audit"]["rule_candidate_count"],
        json!(1)
    );
    assert!(
        read_body["observations"][0]["validator_audit"]
            .get("rule_candidates")
            .is_none()
    );
    assert_eq!(
        read_body["regression_evidence"][0]["status"],
        json!("passed")
    );
    assert_eq!(
        read_body["regression_evidence"][0]["suite_ref"],
        json!("conversation_cases.small20")
    );
    assert_eq!(read_body["preflight_runs"][0]["status"], json!("passed"));
    assert_eq!(read_body["review_runs"][0]["status"], json!("passed"));
    assert_eq!(
        read_body["review_runs"][0]["transition_run_id"],
        body["transition_run_id"]
    );
    assert_eq!(
        read_body["review_runs"][0]["regression_evidence_id"],
        body["regression_evidence_id"]
    );
    assert_eq!(
        read_body["review_runs"][0]["preflight_run_id"],
        body["preflight_run_id"]
    );
    assert_eq!(
        read_body["promotion_runs"].as_array().expect("runs").len(),
        0
    );
    assert_eq!(
        read_body["transition_runs"].as_array().expect("runs").len(),
        1
    );
    assert_eq!(read_body["active_path_visible"], json!(false));

    let reject_response = rule_candidate_transition_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.to_string()),
        Json(RuleCandidateTransitionRequest {
            actor: None,
            action: "reject".to_string(),
            reason: "人工复核认为不应进入通用规则".to_string(),
        }),
    )
    .await;
    assert_eq!(reject_response.status(), StatusCode::OK);
    let reject_body: Value =
        serde_json::from_str(&response_text(reject_response).await).expect("reject response json");
    assert_eq!(reject_body["from_status"], json!("ready_for_review"));
    assert_eq!(reject_body["candidate_status"], json!("rejected"));
    assert_eq!(reject_body["active_path_visible"], json!(false));

    let reopen_response = rule_candidate_transition_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.to_string()),
        Json(RuleCandidateTransitionRequest {
            actor: None,
            action: "reopen".to_string(),
            reason: "补充评测后重新进入候选队列".to_string(),
        }),
    )
    .await;
    assert_eq!(reopen_response.status(), StatusCode::OK);
    let reopen_body: Value =
        serde_json::from_str(&response_text(reopen_response).await).expect("reopen response json");
    assert_eq!(reopen_body["from_status"], json!("rejected"));
    assert_eq!(reopen_body["candidate_status"], json!("staged"));
    assert_eq!(reopen_body["review_state_reset"], json!(true));

    let preflight_status_after_reopen: Option<String> = conn
        .query_row(
            "SELECT preflight_status FROM rule_candidates WHERE candidate_id = ?1",
            params![candidate_id],
            |row| row.get(0),
        )
        .expect("candidate preflight status");
    assert_eq!(preflight_status_after_reopen, None);

    let read_after_transition_response = rule_candidate_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(candidate_id.to_string()),
    )
    .await;
    assert_eq!(read_after_transition_response.status(), StatusCode::OK);
    let read_after_transition_body: Value =
        serde_json::from_str(&response_text(read_after_transition_response).await)
            .expect("transition read response json");
    assert_eq!(
        read_after_transition_body["transition_runs"]
            .as_array()
            .expect("transition runs")
            .len(),
        3
    );
    assert_eq!(
        read_after_transition_body["transition_runs"][0]["action"],
        json!("reopen")
    );
    assert_eq!(
        read_after_transition_body["transition_runs"][1]["action"],
        json!("reject")
    );
    assert_eq!(
        read_after_transition_body["transition_runs"][2]["action"],
        json!("reopen")
    );

    let trace = load_trace(&db_path, "trace-rule-candidate-preflight")
        .expect("trace loads")
        .expect("trace exists");
    assert_eq!(
        trace["online_learning_candidates"]["rule_candidates"][0]["candidate_id"],
        json!(candidate_id)
    );
    assert_eq!(
        trace["online_learning_candidates"]["candidate_ids"]["rule"],
        json!([candidate_id])
    );
    assert_eq!(
        trace["scoped_context"]["rule_candidate_observations"][0]["candidate_id"],
        json!(candidate_id)
    );

    let mut invalid_params = BTreeMap::new();
    invalid_params.insert("trace_id".to_string(), "trace".to_string());
    let invalid_response =
        rule_candidates_endpoint(State(state), admin_headers(), Query(invalid_params)).await;
    assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(audit_event_count(&db_path, "rule_candidate_preflight"), 1);
    assert_eq!(
        audit_event_count(&db_path, "rule_candidate_regression_evidence"),
        0
    );
    assert_eq!(audit_event_count(&db_path, "rule_candidate_review_run"), 1);
    assert_eq!(audit_event_count(&db_path, "rule_candidate_admin_list"), 1);
    assert_eq!(audit_event_count(&db_path, "rule_candidate_admin_read"), 2);
    assert_eq!(
        audit_event_count(&db_path, "rule_candidate_admin_transition"),
        2
    );
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn knowledge_item_admin_review_requires_admin_task_and_records_boundaries() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-knowledge-item-review");
    let state = Arc::new(test_app_state(db_path.clone()));
    let created = state
        .runtime_store
        .create_knowledge_item(KnowledgeItemCreateInput {
            kind: KnowledgeItemKind::Alias,
            initial_state: KnowledgeState::Candidate,
            source_refs: vec!["source://wikisource/chapter/admin-review".to_string()],
            evidence_refs: vec!["block://wikisource/admin-review".to_string()],
            payload: json!({
                "alias": "stone",
                "scope": "admin review endpoint test",
            }),
            schema_version: None,
            trace_id: "trace-admin-knowledge-item-review".to_string(),
            actor: "system-calibration".to_string(),
            reason: "candidate created for admin review endpoint test".to_string(),
        })
        .expect("knowledge item creates");

    let task_response = create_governance_task_endpoint(
        State(state.clone()),
        admin_headers(),
        Json(GovernanceTaskManualCreateRequest {
            source_entity_type: "knowledge_item".to_string(),
            source_entity_id: created.item_id.clone(),
            trace_id: Some("trace-admin-knowledge-item-review".to_string()),
            package_id: None,
            task_type: Some("expert_review".to_string()),
            priority: Some("p0".to_string()),
            proposed_fix: Some("review knowledge item before human marking".to_string()),
            agent_cluster_key: None,
        }),
    )
    .await;
    assert_eq!(task_response.status(), StatusCode::OK);
    let task_body: Value =
        serde_json::from_str(&response_text(task_response).await).expect("task response json");
    assert_eq!(
        task_body["task"]["source_entity_type"],
        json!("knowledge_item")
    );
    let task_id = task_body["task"]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    let task_updated_at = task_body["task"]["updated_at"]
        .as_str()
        .expect("task updated_at")
        .to_string();

    let ordinary = review_knowledge_item_endpoint(
        State(state.clone()),
        gateway_headers("user-1"),
        AxumPath(created.item_id.clone()),
        Json(KnowledgeItemHumanReviewRequest {
            task_id: task_id.clone(),
            decision: "accept".to_string(),
            trace_id: "trace-admin-knowledge-item-review".to_string(),
            reviewer: "admin-1".to_string(),
            review_note: "accepted".to_string(),
            evidence_ref: "source://review-note/admin-review".to_string(),
            if_match_state_version: created.state_version,
            if_match_task_updated_at: Some(task_updated_at.clone()),
        }),
    )
    .await;
    assert_eq!(ordinary.status(), StatusCode::UNAUTHORIZED);

    let review = review_knowledge_item_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(created.item_id.clone()),
        Json(KnowledgeItemHumanReviewRequest {
            task_id: task_id.clone(),
            decision: "accept".to_string(),
            trace_id: "trace-admin-knowledge-item-review".to_string(),
            reviewer: "admin-1".to_string(),
            review_note: "accepted for human marked boundary".to_string(),
            evidence_ref: "source://review-note/admin-review".to_string(),
            if_match_state_version: created.state_version,
            if_match_task_updated_at: Some(task_updated_at),
        }),
    )
    .await;
    assert_eq!(review.status(), StatusCode::OK);
    let review_body: Value =
        serde_json::from_str(&response_text(review).await).expect("review response json");
    assert_eq!(
        review_body["object"],
        json!("tonglingyu.knowledge_item_admin_review")
    );
    assert_eq!(
        review_body["schema_version"],
        json!(KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION)
    );
    assert_eq!(
        review_body["result"]["item"]["state"],
        json!("human_marked")
    );
    assert_eq!(review_body["result"]["task"]["status"], json!("accepted"));
    assert_eq!(review_body["result"]["kb_rebuild_required"], json!(true));
    assert_eq!(review_body["result"]["eval_diff_required"], json!(true));
    assert_eq!(review_body["result"]["release_gate_required"], json!(true));

    let retry = review_knowledge_item_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(created.item_id.clone()),
        Json(KnowledgeItemHumanReviewRequest {
            task_id,
            decision: "accept".to_string(),
            trace_id: "trace-admin-knowledge-item-review".to_string(),
            reviewer: "admin-1".to_string(),
            review_note: "accepted for human marked boundary".to_string(),
            evidence_ref: "source://review-note/admin-review".to_string(),
            if_match_state_version: created.state_version,
            if_match_task_updated_at: Some("stale-task-updated-at".to_string()),
        }),
    )
    .await;
    assert_eq!(retry.status(), StatusCode::OK);
    let conn = open_db(&db_path).expect("db opens");
    let history_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_item_state_history WHERE item_id = ?1",
            params![created.item_id],
            |row| row.get(0),
        )
        .expect("history count");
    assert_eq!(history_count, 2);
    assert_eq!(
        audit_event_count(&db_path, "knowledge_item_admin_review"),
        2
    );
    assert_eq!(
        audit_event_count(&db_path, "knowledge_item_human_reviewed"),
        1
    );
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn knowledge_item_review_conflict_does_not_update_task_or_item() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-knowledge-item-review-conflict");
    let state = Arc::new(test_app_state(db_path.clone()));
    let created = state
        .runtime_store
        .create_knowledge_item(KnowledgeItemCreateInput {
            kind: KnowledgeItemKind::Term,
            initial_state: KnowledgeState::Candidate,
            source_refs: vec!["source://wikisource/chapter/admin-review-conflict".to_string()],
            evidence_refs: vec!["block://wikisource/admin-review-conflict".to_string()],
            payload: json!({
                "term": "stone",
                "scope": "admin review conflict test",
            }),
            schema_version: None,
            trace_id: "trace-admin-knowledge-item-review-conflict".to_string(),
            actor: "system-calibration".to_string(),
            reason: "candidate created for admin review conflict test".to_string(),
        })
        .expect("knowledge item creates");
    let task_response = create_governance_task_endpoint(
        State(state.clone()),
        admin_headers(),
        Json(GovernanceTaskManualCreateRequest {
            source_entity_type: "knowledge_item".to_string(),
            source_entity_id: created.item_id.clone(),
            trace_id: Some("trace-admin-knowledge-item-review-conflict".to_string()),
            package_id: None,
            task_type: Some("expert_review".to_string()),
            priority: Some("p0".to_string()),
            proposed_fix: Some("review knowledge item before rejection".to_string()),
            agent_cluster_key: None,
        }),
    )
    .await;
    assert_eq!(task_response.status(), StatusCode::OK);
    let task_body: Value =
        serde_json::from_str(&response_text(task_response).await).expect("task response json");
    let task_id = task_body["task"]["task_id"]
        .as_str()
        .expect("task id")
        .to_string();
    let task_updated_at = task_body["task"]["updated_at"]
        .as_str()
        .expect("task updated_at")
        .to_string();

    let conflict = review_knowledge_item_endpoint(
        State(state.clone()),
        admin_headers(),
        AxumPath(created.item_id.clone()),
        Json(KnowledgeItemHumanReviewRequest {
            task_id: task_id.clone(),
            decision: "reject".to_string(),
            trace_id: "trace-admin-knowledge-item-review-conflict".to_string(),
            reviewer: "admin-1".to_string(),
            review_note: "reject with stale item state".to_string(),
            evidence_ref: "source://review-note/admin-review-conflict".to_string(),
            if_match_state_version: created.state_version + 1,
            if_match_task_updated_at: Some(task_updated_at),
        }),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let item = state
        .runtime_store
        .read_knowledge_item(&created.item_id)
        .expect("item reads")
        .expect("item exists");
    assert_eq!(item.state, KnowledgeState::Candidate);
    let task = state
        .runtime_store
        .read_governance_task(&task_id)
        .expect("task reads")
        .expect("task exists");
    assert_eq!(task["status"], json!("open"));
    assert_eq!(
        audit_event_count(&db_path, "knowledge_item_admin_review"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "knowledge_item_human_reviewed"),
        0
    );
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn retrieval_failure_cluster_endpoint_creates_proposed_fix_task() {
    let db_path = temp_gateway_db_path("tonglingyu-admin-rqa-cluster");
    seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-cluster-1");
    seed_eval_retrieval_failure(&db_path, "trace-admin-rqa-cluster-2");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = cluster_retrieval_failures_endpoint(
        State(state),
        admin_headers(),
        Json(RetrievalFailureClusterRequest {
            human_review_status: Some("open".to_string()),
            failure_type: Some("quality_report_not_passed".to_string()),
            min_cluster_size: Some(2),
            limit: Some(20),
            create_tasks: Some(true),
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_str(&response_text(response).await).expect("cluster response json");
    assert_eq!(
        body["schema_version"],
        json!(RETRIEVAL_FAILURE_CLUSTER_SCHEMA_VERSION)
    );
    assert_eq!(body["result"]["cluster_count"], json!(1));
    assert_eq!(body["result"]["task_count"], json!(1));
    assert_eq!(
        body["result"]["clusters"][0]["direct_fact_mutation"],
        json!(false)
    );
    assert_eq!(
        body["result"]["clusters"][0]["task"]["source_entity_type"],
        json!("retrieval_failure_cluster")
    );
    assert!(
        body["result"]["clusters"][0]["task"]["proposed_fix"]
            .as_str()
            .is_some_and(|value| value.contains("agent_cluster_proposed_fix"))
    );
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failure_admin_cluster"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "retrieval_failures_clustered"),
        1
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn user_feedback_endpoint_queues_governance_task_without_fact_mutation() {
    let db_path = temp_gateway_db_path("tonglingyu-user-feedback");
    let package = seed_owned_gateway_package(&db_path, "user-1");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = user_feedback_endpoint(
        State(state.clone()),
        gateway_headers("user-1"),
        Json(UserFeedbackRequest {
            trace_id: None,
            package_id: Some(package.package_id.clone()),
            feedback_type: Some("missing_evidence".to_string()),
            feedback_text: "这条回答缺少直接证据，请专家复核。".to_string(),
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_str(&response_text(response).await).expect("feedback response json");
    assert_eq!(body["object"], json!("tonglingyu.user_feedback"));
    assert_eq!(body["direct_fact_mutation"], json!(false));
    assert_eq!(body["task"]["source_entity_type"], json!("user_feedback"));
    assert_eq!(body["task"]["task_type"], json!("expert_review"));
    assert_eq!(body["task"]["priority"], json!("p1"));

    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let tasks = runtime_store
        .list_governance_tasks(KnowledgeGovernanceTaskListInput {
            status: Some("open".to_string()),
            task_type: Some("expert_review".to_string()),
            priority: Some("p1".to_string()),
            source_failure_id: None,
            source_entity_type: Some("user_feedback".to_string()),
            source_entity_id: Some(
                body["task"]["source_entity_id"]
                    .as_str()
                    .expect("feedback source id")
                    .to_string(),
            ),
            limit: 10,
            offset: 0,
        })
        .expect("list user feedback governance tasks");
    assert_eq!(tasks.items.len(), 1);
    assert_eq!(tasks.items[0]["package_id"], json!(package.package_id));
    assert_eq!(tasks.items[0]["source_failure_id"], Value::Null);
    assert!(
        tasks.items[0]["proposed_fix"]
            .as_str()
            .is_some_and(|value| value.contains("user_feedback_type=missing_evidence"))
    );
    assert_eq!(audit_event_count(&db_path, "user_feedback_received"), 1);
    assert_eq!(audit_event_count(&db_path, "governance_task_created"), 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn user_feedback_endpoint_rejects_unowned_package() {
    let db_path = temp_gateway_db_path("tonglingyu-user-feedback-unowned");
    let package = seed_owned_gateway_package(&db_path, "owner-1");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = user_feedback_endpoint(
        State(state),
        gateway_headers("user-2"),
        Json(UserFeedbackRequest {
            trace_id: None,
            package_id: Some(package.package_id),
            feedback_type: Some("wrong_answer".to_string()),
            feedback_text: "这条回答可能有问题。".to_string(),
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(audit_event_count(&db_path, "user_feedback_received"), 0);
    let runtime_store = TonglingyuRuntimeStore::new(db_path.clone());
    let tasks = runtime_store
        .list_governance_tasks(KnowledgeGovernanceTaskListInput {
            status: None,
            task_type: None,
            priority: None,
            source_failure_id: None,
            source_entity_type: Some("user_feedback".to_string()),
            source_entity_id: None,
            limit: 10,
            offset: 0,
        })
        .expect("list user feedback governance tasks");
    assert!(tasks.items.is_empty());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[test]
fn openwebui_metadata_task_detection_is_narrow() {
    let title_prompt = r#"### Task:
Generate a concise, 3-5 word title with an emoji summarizing the chat history.

### Output:
JSON format: { "title": "your concise title here" }

### Chat History:
<chat_history>
USER: 通灵玉是什么？
</chat_history>"#;
    let tags_prompt = r#"### Task:
Generate 1-3 broad tags categorizing the main themes of the chat history.

### Output:
JSON format: { "tags": ["tag1", "tag2", "tag3"] }

### Chat History:
<chat_history>
USER: 通灵玉是什么？
</chat_history>"#;
    let follow_ups_prompt = r#"### Task:
Suggest 3-5 relevant follow-up questions or prompts that the user might naturally ask next in this conversation as a **user**, based on the chat history, to help continue or deepen the discussion.

### Output:
JSON format: { "follow_ups": ["Question 1?", "Question 2?", "Question 3?"] }

### Chat History:
<chat_history>
USER: 通灵玉是什么？
</chat_history>"#;

    assert_eq!(
        detect_openwebui_metadata_task(title_prompt),
        Some(OpenWebUiMetadataTask::Title)
    );
    assert_eq!(
        detect_openwebui_metadata_task(tags_prompt),
        Some(OpenWebUiMetadataTask::Tags)
    );
    assert_eq!(
        detect_openwebui_metadata_task(follow_ups_prompt),
        Some(OpenWebUiMetadataTask::FollowUps)
    );
    assert_eq!(detect_openwebui_metadata_task("通灵玉是什么？"), None);
}

#[tokio::test]
async fn openwebui_metadata_request_does_not_mutate_rqa_governance() {
    let db_path = temp_gateway_db_path("tonglingyu-openwebui-metadata");
    let state = Arc::new(test_app_state(db_path.clone()));
    let metadata_prompt = r#"### Task:
Generate a concise, 3-5 word title with an emoji summarizing the chat history.
### Guidelines:
- The output must be a single, raw JSON object, without any markdown code fences.
### Output:
JSON format: { "title": "your concise title here" }
### Chat History:
<chat_history>
USER: Please answer briefly: what evidence appears when Lin Daiyu first arrives in chapter 3?
ASSISTANT: 证据不足或需要降级：未命中可追溯证据，必须返回证据不足。
</chat_history>"#;

    let response = chat_completions(
        State(state),
        gateway_headers("openwebui-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "messages": [{"role": "user", "content": metadata_prompt}],
            "metadata": {
                "user_id": "openwebui-user",
                "chat_id": "openwebui-chat",
                "message_id": "openwebui-title-message",
            },
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&response_text(response).await).expect("response json");
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .expect("metadata content");
    let metadata_json: Value = serde_json::from_str(content).expect("metadata content is json");
    assert_eq!(metadata_json["title"], json!("通灵玉证据复核"));
    assert_eq!(body["model"], json!(DEFAULT_MODEL_ID));
    assert!(body.get("trace_id").is_none());
    assert!(body.get("evidence_package_id").is_none());
    assert!(body.get("review").is_none());
    assert!(body.get("session_id").is_none());

    let conn = open_db(&db_path).expect("db opens");
    tonglingyu_runtime::init_runtime_schema(&conn).expect("runtime schema");
    assert_eq!(
        table_count(&conn, "retrieval_failures").expect("failure count"),
        0
    );
    assert_eq!(
        table_count(&conn, "knowledge_governance_tasks").expect("task count"),
        0
    );
    assert_eq!(
        table_count(&conn, "evidence_packages").expect("package count"),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM session_journal WHERE entry_type = 'metadata_prompt'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("metadata journal count"),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM session_journal WHERE entry_type = 'final_response'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("metadata final response journal count"),
        1
    );
    assert_eq!(
        table_count(&conn, "context_packs").expect("context pack count"),
        1
    );
    assert_eq!(
        table_count(&conn, "context_projections").expect("context projection count"),
        3
    );
    assert_eq!(
        table_count(&conn, "gateway_messages").expect("legacy gateway message count"),
        0
    );
    assert_eq!(
        audit_event_count(&db_path, "openwebui_metadata_request_handled"),
        1
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openwebui_follow_ups_request_does_not_mutate_rqa_governance() {
    let db_path = temp_gateway_db_path("tonglingyu-openwebui-follow-ups");
    let state = Arc::new(test_app_state(db_path.clone()));
    let metadata_prompt = r#"### Task:
Suggest 3-5 relevant follow-up questions or prompts that the user might naturally ask next in this conversation as a **user**, based on the chat history, to help continue or deepen the discussion.
### Guidelines:
- Response must be a JSON object with a "follow_ups" key containing an array of strings, no extra text or formatting.
### Output:
JSON format: { "follow_ups": ["Question 1?", "Question 2?", "Question 3?"] }
### Chat History:
<chat_history>
USER: 请简要说明第三回林黛玉初进荣国府时当前证据状态。
ASSISTANT: 当前证据状态较为有限，但已有正文材料可直接支持部分文本事实。
</chat_history>"#;

    let response = chat_completions(
        State(state),
        gateway_headers("openwebui-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "messages": [{"role": "user", "content": metadata_prompt}],
            "metadata": {
                "user_id": "openwebui-user",
                "chat_id": "openwebui-chat",
                "message_id": "openwebui-follow-ups-message",
            },
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&response_text(response).await).expect("response json");
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .expect("metadata content");
    let metadata_json: Value = serde_json::from_str(content).expect("metadata content is json");
    assert!(metadata_json["follow_ups"].is_array());
    assert_eq!(body["model"], json!(DEFAULT_MODEL_ID));
    assert!(body.get("trace_id").is_none());
    assert!(body.get("evidence_package_id").is_none());
    assert!(body.get("review").is_none());
    assert!(body.get("session_id").is_none());

    let conn = open_db(&db_path).expect("db opens");
    tonglingyu_runtime::init_runtime_schema(&conn).expect("runtime schema");
    assert_eq!(
        table_count(&conn, "retrieval_failures").expect("failure count"),
        0
    );
    assert_eq!(
        table_count(&conn, "knowledge_governance_tasks").expect("task count"),
        0
    );
    assert_eq!(
        table_count(&conn, "evidence_packages").expect("package count"),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM session_journal WHERE entry_type = 'metadata_prompt'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("metadata journal count"),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM session_journal WHERE entry_type = 'final_response'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("metadata final response journal count"),
        1
    );
    assert_eq!(
        table_count(&conn, "context_packs").expect("context pack count"),
        1
    );
    assert_eq!(
        table_count(&conn, "context_projections").expect("context projection count"),
        3
    );
    assert_eq!(
        table_count(&conn, "gateway_messages").expect("legacy gateway message count"),
        0
    );
    assert_eq!(
        audit_event_count(&db_path, "openwebui_metadata_request_handled"),
        1
    );

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn chat_completion_accepts_long_openwebui_history() {
    let db_path = temp_gateway_db_path("tonglingyu-long-history");
    let state = Arc::new(test_app_state(db_path.clone()));
    let max_messages = state.max_messages;
    let metadata_prompt = r#"### Task:
Generate a concise, 3-5 word title with an emoji summarizing the chat history.
### Guidelines:
- The output must be a single, raw JSON object, without any markdown code fences.
### Output:
JSON format: { "title": "your concise title here" }
### Chat History:
<chat_history>
USER: 介绍尤三姐
</chat_history>"#;
    let mut messages = Vec::new();
    for index in 0..max_messages {
        messages.push(json!({
            "role": "user",
            "content": format!("历史消息 {index}"),
        }));
    }
    messages.push(json!({"role": "user", "content": metadata_prompt}));

    let response = chat_completions(
        State(state),
        gateway_headers("openwebui-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "messages": messages,
            "metadata": {
                "user_id": "openwebui-user",
                "chat_id": "openwebui-chat",
                "message_id": "openwebui-long-history-message",
            },
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&response_text(response).await).expect("response json");
    assert_eq!(body["model"], json!(DEFAULT_MODEL_ID));
    assert!(body.get("trace_id").is_none());
    assert!(body.get("evidence_package_id").is_none());

    let conn = open_db(&db_path).expect("db opens");
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM workflow_states WHERE state = 'Message History Truncated'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("history truncation state count"),
        1
    );
    assert_eq!(audit_event_count(&db_path, "message_history_truncated"), 1);
    let session_summary = conn
        .query_row(
            "SELECT session_summary FROM context_packs ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("context pack session summary");
    assert!(session_summary.contains("历史消息"));
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM session_journal WHERE entry_type = 'metadata_prompt'",
            [],
            |row| row.get(0),
        )
        .expect("metadata prompt journal metadata");
    let metadata: Value = serde_json::from_str(&metadata_json).expect("journal metadata json");
    assert_eq!(metadata["history_over_limit"], json!(true));
    assert_eq!(metadata["max_messages"], json!(max_messages));

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn chat_completion_fails_closed_when_retriever_is_unavailable() {
    let db_path = temp_gateway_db_path("tonglingyu-scoped-context-follow-up");
    seed_runtime_chat_source(&db_path);
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = chat_completions(
        State(state.clone()),
        gateway_headers("scoped-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "messages": [{"role": "user", "content": "介绍尤三姐"}],
            "metadata": {
                "user_id": "scoped-user",
                "chat_id": "scoped-chat",
                "message_id": "scoped-message-1",
            },
        })),
    )
    .await;
    let status = response.status();
    let text = response_text(response).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{text}");
    let body: Value = serde_json::from_str(&text).expect("response json");
    assert_eq!(body["error"]["code"], json!("retriever_failed"));

    let conn = open_db(&db_path).expect("db opens");
    let (trace_id, resolved_question): (String, String) = conn
        .query_row(
            "SELECT trace_id, resolved_question FROM context_packs ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("latest context pack");
    assert_eq!(resolved_question, "介绍尤三姐");
    assert_eq!(
        conn.query_row(
            "SELECT status FROM workflow_states WHERE state = 'Failed with Controlled Response' ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("failure state"),
        "retriever_failed"
    );
    assert_eq!(
        audit_event_count(&db_path, "retriever_http_failed"),
        1,
        "retriever failures must be audited instead of falling back"
    );
    let retrieval_plan = latest_audit_event_payload(&db_path, "retrieval_plan_created");
    assert_eq!(
        retrieval_plan["retriever_query_plan"]["schema_version"],
        json!(DOMAIN_RETRIEVAL_PLAN_SCHEMA_VERSION)
    );
    assert_eq!(retrieval_plan["domain_retrieval_profile"], json!("person"));
    assert!(
        load_trace(&db_path, &trace_id)
            .expect("trace loads")
            .is_some()
    );

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn common_recall_endpoint_fails_closed_when_retriever_is_unavailable() {
    let db_path = temp_gateway_db_path("tonglingyu-common-recall-retriever-unavailable");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = common_recall_endpoint(
        State(state),
        gateway_headers("recall-user"),
        AxumPath("judgement".to_string()),
        Json(CommonRecallRequest {
            query: "宝钗判词".to_string(),
            session_id: Some("recall-session".to_string()),
            limit: Some(5),
            rerank: Some(false),
            trace_level: Some("route".to_string()),
            trace_doc_limit: Some(5),
            include_raw: Some(false),
            metadata: Some(json!({"caller": "test"})),
        }),
    )
    .await;

    let status = response.status();
    let text = response_text(response).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{text}");
    let body: Value = serde_json::from_str(&text).expect("response json");
    assert_eq!(body["error"]["code"], json!("retriever_failed"));
    assert_eq!(
        audit_event_count(&db_path, "retriever_common_recall_requested"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "retriever_common_recall_failed"),
        1
    );
    assert_eq!(
        audit_event_count(&db_path, "retriever_common_recall_completed"),
        0
    );
    let failed = latest_audit_event_payload(&db_path, "retriever_common_recall_failed");
    assert_eq!(failed["domain_retrieval_profile"], json!("judgement_poem"));
    assert_eq!(failed["fallback_used"], json!(false));

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn chat_completion_fails_closed_when_referent_is_unresolved() {
    let db_path = temp_gateway_db_path("tonglingyu-scoped-context-unresolved");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = chat_completions(
        State(state),
        gateway_headers("scoped-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "messages": [{"role": "user", "content": "她最后怎么样？"}],
            "metadata": {
                "user_id": "scoped-user",
                "chat_id": "new-scoped-chat",
                "message_id": "unresolved-message-1",
            },
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&response_text(response).await).expect("response json");
    assert!(
        body["choices"][0]["message"]["content"]
            .as_str()
            .expect("assistant content")
            .contains("请明确")
    );
    assert!(body.get("evidence_package_id").is_none());
    assert!(body.get("context_pack_id").is_none());
    let conn = open_db(&db_path).expect("db opens");
    assert_eq!(
        table_count(&conn, "evidence_packages").expect("package count"),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM workflow_states WHERE status = 'clarification_required'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("clarification state count"),
        1
    );

    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn forbidden_control_fields_audit_llm_provider_not_called() {
    let db_path = temp_gateway_db_path("tonglingyu-llm-agent-provider-not-called");
    let state = Arc::new(test_app_state(db_path.clone()));

    let response = chat_completions(
        State(state),
        gateway_headers("provider-not-called-user"),
        Json(json!({
            "model": DEFAULT_MODEL_ID,
            "messages": [{"role": "user", "content": "通灵玉是什么？"}],
            "metadata": {
                "user_id": "provider-not-called-user",
                "chat_id": "provider-not-called-chat",
                "message_id": "provider-not-called-message",
            },
            "extra_body": {
                "context_pack_id": "forged-context-pack"
            },
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        audit_event_count(&db_path, "llm_agent_provider_not_called"),
        1
    );
    let payload = latest_audit_event_payload(&db_path, "llm_agent_provider_not_called");
    assert_eq!(payload["provider_called"], json!(false));
    assert_eq!(
        payload["profiles_not_called"],
        json!([
            QUESTION_NORMALIZER_PROFILE_ID,
            CONVERSATION_STATE_WRITER_PROFILE_ID
        ])
    );
    assert_eq!(payload["raw_agent_output_embedded"], json!(false));
    assert!(payload["forbidden_fields_sha256"].as_str().is_some());

    remove_sqlite_file_set(&db_path);
}

#[test]
fn public_completion_does_not_expose_rqa_internal_fields() {
    let package = EvidencePackage {
        package_id: "pkg-public-rqa-test".to_string(),
        trace_id: "trace-public-rqa-test".to_string(),
        question: "通灵玉是什么？".to_string(),
        cards: vec![eval_test_card("block-public-rqa-test")],
        claims: vec!["通灵玉回答必须受证据包约束。".to_string()],
        claim_evidence_map: Vec::new(),
        knowledge_state_summary: Default::default(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer passed".to_string(),
        },
    };
    let mut value = completion_value(
        DEFAULT_MODEL_ID,
        "测试回答".to_string(),
        Some(&package),
        Some("session-public-rqa-test"),
    );
    value["context_pack_id"] = json!("context-pack-public-rqa-test");
    value["context_pack_ref"] = json!("context-pack://tonglingyu/public-rqa/test");
    value["context_projection_id"] = json!("context-projection-public-rqa-test");
    value["context_projection_ref"] = json!("context-projection://tonglingyu/public-rqa/test");
    value["context_projections"] = json!([{"consumer_name": "honglou-main"}]);
    value["interaction_context_id"] = json!("interaction-context-public-rqa-test");
    value["session_journal"] = json!([{"entry_type": "user_message"}]);
    value["memory_read_refs"] = json!(["memory:forbidden"]);
    value["memory_read_ref_digest"] = json!("memory-read-ref-digest");
    value["memory_read_policy_digest"] = json!("memory-read-policy-digest");
    value["memory_summaries"] = json!([{"summary": "internal memory"}]);
    value["memory_policy_digest"] = json!("memory-policy-digest");
    value["memory_usage_summary"] = json!({"read_ref_count": 1});
    value["memory_candidate_id"] = json!("memory-candidate-public-rqa-test");
    value["memory_card_id"] = json!("memory-card-public-rqa-test");
    value["memory_policy_decision_id"] = json!("memory-policy-decision-public-rqa-test");
    value["memory_policy_decision_ref"] =
        json!("memory-policy-decision://tonglingyu/public-rqa/test");
    value["llm_extraction"] = json!({"summary": "internal"});
    value["llm_filter"] = json!({"schema_version": "scoped-memory-llm-filter-v1"});
    value["rule_filter"] = json!({"schema_version": "scoped-memory-rule-filter-v1"});
    value["read_enabled"] = json!(true);

    let rendered =
        serde_json::to_string(&public_completion_value(&value)).expect("completion serializes");

    assert!(!rendered.contains("retrieval_failures"));
    assert!(!rendered.contains("retrieval_quality_summary"));
    assert!(!rendered.contains("quality_report"));
    assert!(!rendered.contains("trace-public-rqa-test"));
    assert!(!rendered.contains("pkg-public-rqa-test"));
    assert!(!rendered.contains("reviewer"));
    assert!(!rendered.contains("session-public-rqa-test"));
    assert!(!rendered.contains("context_pack_id"));
    assert!(!rendered.contains("context_pack_ref"));
    assert!(!rendered.contains("context_projection_id"));
    assert!(!rendered.contains("context_projection_ref"));
    assert!(!rendered.contains("context_projections"));
    assert!(!rendered.contains("interaction_context_id"));
    assert!(!rendered.contains("session_journal"));
    assert!(!rendered.contains("memory_read_refs"));
    assert!(!rendered.contains("memory_read_ref_digest"));
    assert!(!rendered.contains("memory_read_policy_digest"));
    assert!(!rendered.contains("memory_summaries"));
    assert!(!rendered.contains("memory_policy_digest"));
    assert!(!rendered.contains("memory_usage_summary"));
    assert!(!rendered.contains("memory_candidate_id"));
    assert!(!rendered.contains("memory_card_id"));
    assert!(!rendered.contains("memory_policy_decision_id"));
    assert!(!rendered.contains("memory_policy_decision_ref"));
    assert!(!rendered.contains("llm_extraction"));
    assert!(!rendered.contains("llm_filter"));
    assert!(!rendered.contains("rule_filter"));
    assert!(!rendered.contains("read_enabled"));
}

#[test]
fn public_completion_blocks_knowledge_state_labels_in_answer_content() {
    let value = completion_value(
        DEFAULT_MODEL_ID,
        "internal state: system_calibrated runtime_usable human_marked knowledge_item_refs"
            .to_string(),
        None,
        Some("session-public-knowledge-state-test"),
    );

    let rendered =
        serde_json::to_string(&public_completion_value(&value)).expect("completion serializes");

    for forbidden in [
        "system_calibrated",
        "runtime_usable",
        "human_marked",
        "knowledge_item_refs",
        "session-public-knowledge-state-test",
    ] {
        assert!(!rendered.contains(forbidden));
    }
    assert!(rendered.contains("公开输出检查"));
}

#[tokio::test]
async fn streaming_completion_does_not_expose_rqa_internal_fields() {
    let package = EvidencePackage {
        package_id: "pkg-public-rqa-stream-test".to_string(),
        trace_id: "trace-public-rqa-stream-test".to_string(),
        question: "通灵玉是什么？".to_string(),
        cards: vec![eval_test_card("block-public-rqa-stream-test")],
        claims: vec!["通灵玉回答必须受证据包约束。".to_string()],
        claim_evidence_map: Vec::new(),
        knowledge_state_summary: Default::default(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer passed".to_string(),
        },
    };
    let value = completion_value(
        DEFAULT_MODEL_ID,
        "测试回答".to_string(),
        Some(&package),
        Some("session-public-rqa-stream-test"),
    );

    let rendered = response_text(streaming_response_from_completion_value(&value)).await;

    assert!(!rendered.contains("retrieval_failures"));
    assert!(!rendered.contains("retrieval_quality_summary"));
    assert!(!rendered.contains("quality_report"));
    assert!(!rendered.contains("trace-public-rqa-stream-test"));
    assert!(!rendered.contains("pkg-public-rqa-stream-test"));
    assert!(!rendered.contains("reviewer"));
    assert!(!rendered.contains("session-public-rqa-stream-test"));
}

#[tokio::test]
async fn streaming_completion_blocks_knowledge_state_labels_in_deltas() {
    let value = completion_value(
        DEFAULT_MODEL_ID,
        "fallback contains no internal label".to_string(),
        None,
        Some("session-public-knowledge-state-stream-test"),
    );
    let response = streaming_response_from_runtime_events(
        DEFAULT_MODEL_ID,
        &value,
        &[RuntimeWorkflowStreamEvent {
            sequence: 1,
            event_type: "content_delta".to_string(),
            profile: "honglou-main".to_string(),
            trace_id: "trace-public-knowledge-state-stream-test".to_string(),
            content_delta: Some("leaked runtime_usable knowledge_item_refs".to_string()),
            output_ref: None,
            package_id: None,
            metadata: json!({"state": "system_calibrated"}),
        }],
    );

    let rendered = response_text(response).await;

    for forbidden in [
        "system_calibrated",
        "runtime_usable",
        "human_marked",
        "knowledge_item_refs",
        "trace-public-knowledge-state-stream-test",
        "session-public-knowledge-state-stream-test",
    ] {
        assert!(!rendered.contains(forbidden));
    }
    assert!(rendered.contains("fallback contains no internal label"));
}

#[test]
fn forbidden_control_fields_rejects_runtime_and_admin_trace_controls() {
    let mut fields = forbidden_control_fields(&json!({
        "model": "tonglingyu",
        "agent_runtime_summary": {"status": "forged"},
        "metadata": {
            "runtime_step_plan": [],
            "admin_trace": {"trace_id": "forged"},
            "interaction_context_id": "forged-context",
            "runtime_adapter": "forged-runtime",
            "session_journal": [{"content": "forged"}],
            "memory_candidate_id": "forged-candidate",
            "nested": {"agent_runtime": {"mode": "forged"}},
            "message_id": "open-webui-message",
        },
        "extra_body": {
            "allowed_tools": ["tonglingyu.text.search"],
            "context_pack_id": "forged-pack",
            "context_projection_digest": "forged-digest",
            "context_projection_ref": "forged-projection",
            "forbidden_tools": ["tonglingyu.commentary.search"],
            "llm_extraction": {"promotion": "forged"},
            "memory_card_id": "forged-card",
            "memory_read_policy_digest": "forged-read-policy",
            "memory_read_ref_digest": "forged-read-ref-digest",
            "memory_read_refs": ["memory-summary://forged"],
            "memory_read_scopes": ["user_private:any"],
            "read_enabled": true,
            "tool_policy_digest": "forged-tool-policy",
            "layers": [{"runtime_step_outputs": []}],
        },
        "messages": [{"role": "user", "content": "通灵玉是什么？"}],
    }));
    fields.sort();

    assert_eq!(
        fields,
        vec![
            "agent_runtime_summary",
            "extra_body.allowed_tools",
            "extra_body.context_pack_id",
            "extra_body.context_projection_digest",
            "extra_body.context_projection_ref",
            "extra_body.forbidden_tools",
            "extra_body.layers[0].runtime_step_outputs",
            "extra_body.llm_extraction",
            "extra_body.memory_card_id",
            "extra_body.memory_read_policy_digest",
            "extra_body.memory_read_ref_digest",
            "extra_body.memory_read_refs",
            "extra_body.memory_read_scopes",
            "extra_body.read_enabled",
            "extra_body.tool_policy_digest",
            "metadata.admin_trace",
            "metadata.interaction_context_id",
            "metadata.memory_candidate_id",
            "metadata.nested.agent_runtime",
            "metadata.runtime_adapter",
            "metadata.runtime_step_plan",
            "metadata.session_journal",
        ]
    );
}

#[test]
fn forbidden_control_fields_allows_openwebui_identity_metadata() {
    let fields = forbidden_control_fields(&json!({
        "model": "tonglingyu",
        "metadata": {
            "user_id": "user-a",
            "chat_id": "chat-a",
            "message_id": "message-a",
        },
        "messages": [{"role": "user", "content": "通灵玉是什么？"}],
    }));

    assert!(fields.is_empty());
}

#[test]
fn gateway_rate_limiter_rejects_after_subject_budget() {
    let limiter = GatewayRateLimiter::new(2, Duration::from_secs(60));

    let first = limiter.check("subject-a");
    let second = limiter.check("subject-a");
    let third = limiter.check("subject-a");
    let other_subject = limiter.check("subject-b");

    assert!(first.allowed);
    assert_eq!(first.remaining, 1);
    assert!(second.allowed);
    assert_eq!(second.remaining, 0);
    assert!(!third.allowed);
    assert_eq!(third.limit, 2);
    assert!(third.retry_after_secs >= 1);
    assert!(other_subject.allowed);
}

#[test]
fn gateway_rate_limiter_can_be_disabled() {
    let limiter = GatewayRateLimiter::new(0, Duration::from_secs(60));

    for _ in 0..10 {
        let decision = limiter.check("subject-a");
        assert!(decision.allowed);
        assert_eq!(decision.limit, 0);
    }
}

#[test]
fn configured_keys_deduplicates_trims_and_splits_rotation_keys() {
    let keys = configured_keys(
        Some(" gateway-a ".to_string()),
        Some("gateway-b, gateway-a, ,gateway-c".to_string()),
    );

    assert_eq!(keys, ["gateway-a", "gateway-b", "gateway-c"]);
}

#[test]
fn rejects_overlapping_gateway_and_admin_keys() {
    let err = validate_admin_key_isolation(
        &["gateway-a".to_string(), "shared".to_string()],
        &["admin-a".to_string(), "shared".to_string()],
        false,
    )
    .expect_err("overlapping gateway/admin keys must be rejected");

    assert!(
        err.to_string()
            .contains("admin API keys must not overlap gateway API keys")
    );
    assert!(!err.to_string().contains("shared"));
}

#[test]
fn rejects_admin_gateway_fallback_when_admin_keys_are_configured() {
    let err =
        validate_admin_key_isolation(&["gateway-a".to_string()], &["admin-a".to_string()], true)
            .expect_err("admin fallback must not coexist with admin keys");

    assert!(
        err.to_string()
            .contains("requires empty admin API key configuration")
    );
    assert!(!err.to_string().contains("admin-a"));
}

#[test]
fn allows_gateway_fallback_only_without_admin_keys() {
    validate_admin_key_isolation(&["gateway-a".to_string()], &[], true)
        .expect("local gateway-key admin fallback should remain available without admin keys");
}

#[test]
fn gateway_does_not_reown_runtime_domain_or_kb_functions() {
    let main_source = include_str!("main.rs");
    for function_name in [
        "init_knowledge_base_schema",
        "load_source_snapshot",
        "seed_aliases",
        "extract_terms",
        "query_blocks_like",
        "query_blocks_exact_text",
        "evidence_card_from_block",
        "create_evidence_package",
        "load_evidence_package",
        "claims_from_cards",
        "review",
        "local_answer",
        "enforce_review",
    ] {
        let forbidden = format!("fn {function_name}(");
        assert!(
            !main_source.contains(&forbidden),
            "Gateway must not re-own runtime domain function {function_name}"
        );
    }
    for forbidden in [
        format!("struct Source{}", "Metadata"),
        format!("struct Block{}", "Record"),
        format!("CREATE VIRTUAL TABLE IF NOT EXISTS {}", "blocks_fts"),
        format!("INSERT INTO {}", "blocks_fts"),
        format!("SELECT package_id FROM {}", "evidence_packages"),
        format!("DELETE FROM {}", "evidence_packages"),
        format!("INSERT INTO {}", "audit_events"),
        format!("SELECT COUNT(*) FROM {}", "sources"),
    ] {
        assert!(
            !main_source.contains(&forbidden),
            "Gateway must not re-own Runtime KB/source snapshot code: {forbidden}"
        );
    }
}
