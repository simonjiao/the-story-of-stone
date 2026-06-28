use super::*;
use agent_core::{RuntimeOutput, RuntimeRunInput, RuntimeSessionInput};

fn test_env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    let values = pairs
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    move |name| values.get(name).map(|value| (*value).to_string())
}

#[derive(Debug, Default)]
struct DraftRuntimeClient;

#[derive(Debug, Default)]
struct NoToolRuntimeClient;

#[derive(Debug, Default)]
struct PartialCoverageDraftRuntimeClient;

#[derive(Debug, Default)]
struct InvalidEvidenceRefDraftRuntimeClient;

#[derive(Debug, Default)]
struct EvidenceBoundaryDraftRuntimeClient;

#[derive(Debug, Default)]
struct ProviderRequestRuntimeClient;

#[derive(Debug, Default)]
struct BadOutputRefRuntimeClient;

#[derive(Debug, Default)]
struct IncompleteHermesContentRuntimeClient;

#[derive(Debug, Default)]
struct MissingToolAuditRuntimeClient;

#[derive(Debug, Default)]
struct WrongEvidenceOutputRefRuntimeClient;

#[derive(Debug, Default)]
struct FailingProfileRuntimeClient;

#[derive(Debug, Default)]
struct DiagnosticFailingProfileRuntimeClient;

#[derive(Debug, Default)]
struct TimeoutProfileRuntimeClient;

#[derive(Debug, Default)]
struct FlakyProfileRuntimeClient {
    failures: std::sync::atomic::AtomicUsize,
}

#[derive(Debug, Default)]
struct DraftRepairRuntimeClient;

#[derive(Debug, Default)]
struct OpenObjectDraftRepairRuntimeClient;

#[derive(Debug, Default)]
struct CalibrationJudgeRuntimeClient;

#[test]
fn workflow_agent_runtime_mode_rejects_hermes_provider_backend() {
    let env = test_env(&[
        ("TONGLINGYU_AGENT_RUNTIME_MODE", "openai-compatible-network"),
        ("TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER", "hermes_tooling"),
        ("TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER", "hermes_tooling"),
        ("TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER", "hermes_tooling"),
        ("TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER", "hermes_tooling"),
        (
            "TONGLINGYU_AGENT_PROVIDER_HERMES_TOOLING_BACKEND",
            "hermes-agent",
        ),
    ]);

    let error = workflow_agent_runtime_mode_from_role_provider_source(&env)
        .expect_err("workflow role provider backend must reject Hermes")
        .to_string();

    assert!(error.contains("openai-compatible-network"));
    assert!(error.contains("hermes-agent"));
}

#[test]
fn workflow_agent_runtime_mode_accepts_openai_compatible_provider_backend() {
    let env = test_env(&[
        ("TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER", "openai_profile"),
        ("TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER", "openai_profile"),
        ("TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER", "openai_profile"),
        ("TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER", "openai_profile"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BACKEND",
            "openai-compatible-network",
        ),
    ]);

    let mode = workflow_agent_runtime_mode_from_role_provider_source(&env)
        .expect("openai-compatible provider backend parses");

    assert_eq!(
        mode,
        Some(TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork)
    );
}

#[test]
fn openai_compatible_provider_profile_defaults_to_deepseek_chat_family() {
    let env = test_env(&[
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BACKEND",
            "openai-compatible-network",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BASE_URL",
            "http://provider.local/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MODEL",
            "provider-model",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_KEY_ENV",
            "OPENAI_COMPATIBLE_API_KEY",
        ),
        ("OPENAI_COMPATIBLE_API_KEY", "provider-key"),
    ]);

    let config = openai_compatible_config_from_provider_profile_source(
        "openai_profile",
        &["honglou-text", "honglou-main"],
        &env,
    )
    .expect("provider profile config parses");

    assert_eq!(config.api_family.as_str(), "deepseek-chat");
    assert_eq!(config.thinking_type, None);
    assert_eq!(
        config
            .profile_models
            .get("honglou-text")
            .map(String::as_str),
        Some("provider-model")
    );
}

#[test]
fn openai_compatible_provider_profile_applies_profile_tuning() {
    let env = test_env(&[
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BACKEND",
            "openai-compatible-network",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BASE_URL",
            "http://provider.local/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MODEL",
            "provider-model",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_KEY_ENV",
            "OPENAI_COMPATIBLE_API_KEY",
        ),
        ("OPENAI_COMPATIBLE_API_KEY", "provider-key"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_CONNECT_TIMEOUT_MS",
            "800",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_READ_TIMEOUT_MS",
            "7000",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_TOTAL_DEADLINE_MS",
            "9000",
        ),
        ("TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MAX_TOKENS", "384"),
        ("TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MAX_ATTEMPTS", "1"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_RETRY_BASE_DELAY_MS",
            "25",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_UNHEALTHY_WINDOW_MS",
            "0",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MAX_CONCURRENCY",
            "3",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_FAMILY",
            "deepseek-chat",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_THINKING_TYPE",
            "disabled",
        ),
    ]);

    let config = openai_compatible_config_from_provider_profile_source(
        "openai_profile",
        &["honglou-text", "honglou-main"],
        &env,
    )
    .expect("provider profile config parses");

    assert_eq!(config.connect_timeout.as_millis(), 800);
    assert_eq!(config.read_timeout.as_millis(), 7000);
    assert_eq!(config.total_deadline.as_millis(), 9000);
    assert_eq!(config.max_tokens, 384);
    assert_eq!(config.max_attempts, 1);
    assert_eq!(config.retry_base_delay.as_millis(), 25);
    assert_eq!(config.unhealthy_window.as_millis(), 0);
    assert_eq!(config.max_concurrency, 3);
    assert_eq!(config.api_family.as_str(), "deepseek-chat");
    assert_eq!(
        config.thinking_type.map(|value| value.as_str()),
        Some("disabled")
    );
}

#[test]
fn openai_compatible_provider_profile_supports_openai_responses_family() {
    let env = test_env(&[
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BACKEND",
            "openai-compatible-network",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BASE_URL",
            "http://provider.local/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MODEL",
            "provider-model",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_KEY_ENV",
            "OPENAI_COMPATIBLE_API_KEY",
        ),
        ("OPENAI_COMPATIBLE_API_KEY", "provider-key"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_FAMILY",
            "openai-responses",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_REASONING_EFFORT",
            "medium",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_REASONING_SUMMARY",
            "auto",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_TEXT_VERBOSITY",
            "low",
        ),
        ("TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_STORE", "false"),
    ]);

    let config = openai_compatible_config_from_provider_profile_source(
        "openai_profile",
        &["honglou-text"],
        &env,
    )
    .expect("provider profile config parses");

    assert_eq!(config.api_family.as_str(), "openai-responses");
    assert_eq!(config.reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(config.reasoning_summary.as_deref(), Some("auto"));
    assert_eq!(config.text_verbosity.as_deref(), Some("low"));
    assert_eq!(config.store, Some(false));

    let invalid_env = test_env(&[
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BASE_URL",
            "http://provider.local/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MODEL",
            "provider-model",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_KEY_ENV",
            "OPENAI_COMPATIBLE_API_KEY",
        ),
        ("OPENAI_COMPATIBLE_API_KEY", "provider-key"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_FAMILY",
            "openai-responses",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_THINKING_TYPE",
            "disabled",
        ),
    ]);

    let error = openai_compatible_config_from_provider_profile_source(
        "openai_profile",
        &["honglou-text"],
        &invalid_env,
    )
    .expect_err("OpenAI Responses must reject DeepSeek thinking fields")
    .to_string();

    assert!(error.contains("THINKING_TYPE"));

    let invalid_verbosity_env = test_env(&[
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_BASE_URL",
            "http://provider.local/v1",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_MODEL",
            "provider-model",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_KEY_ENV",
            "OPENAI_COMPATIBLE_API_KEY",
        ),
        ("OPENAI_COMPATIBLE_API_KEY", "provider-key"),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_API_FAMILY",
            "openai-responses",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_OPENAI_PROFILE_TEXT_VERBOSITY",
            "verbose",
        ),
    ]);

    let error = openai_compatible_config_from_provider_profile_source(
        "openai_profile",
        &["honglou-text"],
        &invalid_verbosity_env,
    )
    .expect_err("OpenAI Responses must reject invalid text verbosity")
    .to_string();

    assert!(error.contains("TEXT_VERBOSITY"));
}

#[test]
fn workflow_agent_runtime_mode_rejects_partial_role_provider_config() {
    let env = test_env(&[("TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER", "hermes_tooling")]);

    let error = workflow_agent_runtime_mode_from_role_provider_source(&env)
        .expect_err("partial workflow role provider config must fail closed")
        .to_string();

    assert!(error.contains("TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER"));
}

#[test]
fn workflow_agent_runtime_mode_rejects_mixed_provider_profiles() {
    let env = test_env(&[
        ("TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER", "hermes_tooling"),
        ("TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER", "hermes_tooling"),
        ("TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER", "deepseek_flash"),
        ("TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER", "hermes_tooling"),
        (
            "TONGLINGYU_AGENT_PROVIDER_HERMES_TOOLING_BACKEND",
            "hermes-agent",
        ),
        (
            "TONGLINGYU_AGENT_PROVIDER_DEEPSEEK_FLASH_BACKEND",
            "openai-compatible-network",
        ),
    ]);

    let error = workflow_agent_runtime_mode_from_role_provider_source(&env)
        .expect_err("mixed workflow providers are unsupported until per-step routing")
        .to_string();

    assert!(error.contains("workflow agent role providers must use one provider profile"));
}

#[test]
fn workflow_agent_runtime_mode_rejects_unsupported_provider_backend() {
    let env = test_env(&[
        ("TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER", "legacy_workflow"),
        ("TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER", "legacy_workflow"),
        ("TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER", "legacy_workflow"),
        ("TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER", "legacy_workflow"),
        (
            "TONGLINGYU_AGENT_PROVIDER_LEGACY_WORKFLOW_BACKEND",
            "legacy-provider",
        ),
    ]);

    let error = workflow_agent_runtime_mode_from_role_provider_source(&env)
        .expect_err("workflow role provider backend must reject unsupported provider")
        .to_string();

    assert!(error.contains("openai-compatible-network"));
    assert!(error.contains("legacy-provider"));
}

#[derive(Debug)]
struct SlowDraftRuntimeClient {
    active: Arc<std::sync::atomic::AtomicUsize>,
    max_active: Arc<std::sync::atomic::AtomicUsize>,
}

impl SlowDraftRuntimeClient {
    fn new(
        active: Arc<std::sync::atomic::AtomicUsize>,
        max_active: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self { active, max_active }
    }
}

fn test_runtime_context(
    trace_id: &str,
    question: &str,
    profiles: &RuntimeWorkflowProfiles,
) -> RuntimeContextContract {
    let interaction_context_id = format!("test-interaction-context-{trace_id}");
    let context_pack_ref = format!("context-pack://tonglingyu/{trace_id}/test");
    let pack_payload = json!({
        "trace_id": trace_id,
        "interaction_context_id": &interaction_context_id,
        "resolved_question": question,
        "schema_version": RUNTIME_CONTEXT_PACK_SCHEMA_VERSION,
        "source": "runtime-test",
    });
    RuntimeContextContract {
        trace_id: trace_id.to_string(),
        interaction_context_id,
        context_pack_ref: context_pack_ref.clone(),
        context_pack_schema_version: RUNTIME_CONTEXT_PACK_SCHEMA_VERSION.to_string(),
        context_pack_digest: hash_json(&pack_payload),
        projections: vec![
            test_runtime_projection(
                trace_id,
                &context_pack_ref,
                &profiles.text,
                question,
                None,
                vec!["tonglingyu.text.search".to_string()],
            ),
            test_runtime_projection(
                trace_id,
                &context_pack_ref,
                &profiles.main,
                question,
                Some("test session summary".to_string()),
                vec![
                    "tonglingyu.evidence.package.create".to_string(),
                    "tonglingyu.evidence.package.read".to_string(),
                ],
            ),
            test_runtime_projection(
                trace_id,
                &context_pack_ref,
                &profiles.reviewer,
                question,
                None,
                vec!["tonglingyu.evidence.package.read".to_string()],
            ),
        ],
    }
}

fn test_runtime_projection(
    trace_id: &str,
    context_pack_ref: &str,
    consumer_name: &str,
    question: &str,
    session_summary: Option<String>,
    allowed_tools: Vec<String>,
) -> RuntimeContextProjection {
    let context_projection_id = format!("test-context-projection-{trace_id}-{consumer_name}");
    let context_projection_ref =
        format!("context-projection://tonglingyu/{trace_id}/{consumer_name}");
    let forbidden_tools = Vec::<String>::new();
    let projection_payload = json!({
        "object": "tonglingyu.context_projection_payload",
        "visible_question": question,
        "session_summary": session_summary,
        "forbidden_context": ["complete_user_history", "unauthorized_memory"],
        "memory_read_refs": [],
        "consumer_name": consumer_name,
    });
    let output_contract = json!({
        "object": "tonglingyu.test_runtime_projection",
        "must_return_output_ref": true,
    });
    let tool_policy_digest = hash_json(&json!({
        "allowed_tools": &allowed_tools,
        "forbidden_tools": &forbidden_tools,
    }));
    let output_contract_digest = hash_json(&output_contract);
    let unsigned_projection = json!({
        "context_projection_id": &context_projection_id,
        "context_projection_ref": &context_projection_ref,
        "context_pack_ref": context_pack_ref,
        "consumer_type": RUNTIME_CONTEXT_CONSUMER_TYPE,
        "consumer_name": consumer_name,
        "runtime_adapter": TONGLINGYU_RUNTIME_ADAPTER,
        "projection_payload": &projection_payload,
        "allowed_tools": &allowed_tools,
        "forbidden_tools": &forbidden_tools,
        "output_contract": &output_contract,
        "tool_policy_digest": &tool_policy_digest,
        "output_contract_digest": &output_contract_digest,
        "schema_version": RUNTIME_CONTEXT_PROJECTION_SCHEMA_VERSION,
    });
    RuntimeContextProjection {
        context_projection_id,
        context_projection_ref,
        context_pack_ref: context_pack_ref.to_string(),
        context_projection_schema_version: RUNTIME_CONTEXT_PROJECTION_SCHEMA_VERSION.to_string(),
        context_projection_digest: hash_json(&unsigned_projection),
        consumer_type: RUNTIME_CONTEXT_CONSUMER_TYPE.to_string(),
        consumer_name: consumer_name.to_string(),
        runtime_adapter: TONGLINGYU_RUNTIME_ADAPTER.to_string(),
        projection_payload,
        allowed_tools,
        forbidden_tools,
        output_contract,
        tool_policy_digest,
        output_contract_digest,
    }
}

fn test_workflow_input(
    trace_id: &str,
    question: &str,
    limit: usize,
    required_evidence_types: Vec<String>,
) -> RuntimeWorkflowInput {
    let profiles = RuntimeWorkflowProfiles::default();
    RuntimeWorkflowInput {
        trace_id: trace_id.to_string(),
        question: question.to_string(),
        limit,
        required_evidence_types,
        retrieved_evidence: None,
        context: test_runtime_context(trace_id, question, &profiles),
        profiles,
    }
}

fn test_workflow_input_with_question_frame(
    trace_id: &str,
    question: &str,
    limit: usize,
    required_evidence_types: Vec<String>,
    question_frame: Value,
) -> RuntimeWorkflowInput {
    let mut input = test_workflow_input(trace_id, question, limit, required_evidence_types);
    for projection in &mut input.context.projections {
        projection.projection_payload["question_frame"] = question_frame.clone();
        projection.context_projection_digest = hash_json(&projection.digest_value());
    }
    input
}

fn relation_question_frame_value() -> Value {
    json!({
        "schema_version": "tonglingyu.question_frame.v1",
        "intent": "relation_query",
        "canonical_question": "紫鹃服侍过史湘云吗？",
        "subject": {
            "canonical": "紫鹃",
            "aliases": ["紫鹃", "紫鵑", "鹦哥"]
        },
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候"],
            "evidence_terms": ["丫鬟", "丫头", "跟着"]
        },
        "object": {
            "canonical": "史湘云",
            "aliases": ["史湘云", "史湘雲", "湘云"]
        },
        "source_scope": "pre_80_base_text_and_commentary",
        "required_evidence_types": ["base_text"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null
    })
}

fn xiren_jiamu_relation_question_frame_value() -> Value {
    json!({
        "schema_version": "tonglingyu.question_frame.v1",
        "intent": "relation_query",
        "canonical_question": "袭人服侍过贾母吗？",
        "subject": {
            "canonical": "袭人",
            "aliases": ["袭人", "襲人", "珍珠"]
        },
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候"],
            "evidence_terms": ["丫鬟", "丫头", "跟着"]
        },
        "object": {
            "canonical": "贾母",
            "aliases": ["贾母", "賈母", "老太太"]
        },
        "source_scope": "pre_80_base_text_and_commentary",
        "required_evidence_types": ["base_text"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null
    })
}

fn xiren_open_object_relation_question_frame_value() -> Value {
    json!({
        "schema_version": "tonglingyu.question_frame.v1",
        "intent": "relation_query",
        "canonical_question": "袭人服侍过谁？",
        "subject": {
            "canonical": "袭人",
            "aliases": ["袭人", "襲人", "珍珠"]
        },
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头", "跟着"]
        },
        "object": null,
        "source_scope": "pre_80_base_text_and_commentary",
        "required_evidence_types": ["base_text"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null
    })
}

fn zijuan_entity_question_frame_value() -> Value {
    json!({
        "schema_version": "tonglingyu.question_frame.v1",
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {
            "canonical": "紫鹃",
            "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]
        },
        "predicate": null,
        "object": null,
        "source_scope": "pre_80_base_text_and_commentary",
        "required_evidence_types": ["base_text"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null
    })
}

fn lindaiyu_age_question_frame_value() -> Value {
    json!({
        "schema_version": "tonglingyu.question_frame.v1",
        "intent": "attribute_at_event",
        "canonical_question": "林黛玉进贾府时多大了",
        "subject": {
            "canonical": "林黛玉",
            "aliases": ["林黛玉", "黛玉", "林姑娘"]
        },
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大"],
            "evidence_terms": ["岁", "歲", "年纪", "年紀", "年方"]
        },
        "object": null,
        "source_scope": "pre_80_base_text_and_commentary",
        "required_evidence_types": ["base_text"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null
    })
}

fn lindaiyu_character_fate_question_frame_value() -> Value {
    json!({
        "schema_version": "tonglingyu.question_frame.v1",
        "intent": "character_fate_query",
        "canonical_question": "林黛玉结局如何",
        "subject": {
            "canonical": "林黛玉",
            "aliases": ["林黛玉", "黛玉", "林姑娘"]
        },
        "predicate": null,
        "object": null,
        "source_scope": "pre_80_base_text_and_commentary",
        "required_evidence_types": ["base_text"],
        "confidence": 0.91,
        "needs_clarification": false,
        "clarification_question": null
    })
}

#[async_trait]
impl RuntimeClient for CalibrationJudgeRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "calibration judge runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "calibration judge runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        if input.profile_id != KNOWLEDGE_CALIBRATION_PROFILE_ID {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "unexpected calibration profile",
            ));
        }
        if !input.requested_tools.is_empty() {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "calibration profile must not request tools",
            ));
        }
        let config_digest = input
            .metadata
            .get("llm_config")
            .and_then(|value| value.get("config_digest"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(RuntimeOutput {
            result_summary: serde_json::to_string(&json!({
                "llm_evidence_judge": {
                    "decision": "system_calibrated",
                    "confidence": 0.93,
                    "evidence_refs": ["block://wikisource/llm-judge"],
                    "source_boundary": {
                        "source_id": "wikisource",
                        "usage_boundary": "source snapshot evidence only",
                        "config_digest": config_digest,
                    },
                    "quality_issues": [],
                    "forbidden_conclusion_detected": false
                }
            }))
            .expect("calibration output serializes"),
            result_ref: Some("result://calibration-judge".to_string()),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": input.profile_id,
                "trace_id": input.trace_id,
                "tool_results": [],
                "tool_audit_events": [],
            }),
        })
    }
}

#[async_trait]
impl RuntimeClient for DraftRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "draft runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "draft runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let operation = input
            .runtime_step
            .as_ref()
            .and_then(|step| step.metadata.get("operation"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let tool_rounds = if input.requested_tools.is_empty() {
            0
        } else {
            1
        };
        let package_id = package_id_from_step_message(&message);
        let public_draft = public_draft_from_step_message(&message);
        let tool_results = if input.requested_tools.is_empty() {
            json!([])
        } else {
            Value::Array(
                input
                    .requested_tools
                    .iter()
                    .enumerate()
                    .map(|(index, tool_name)| {
                        let output_ref = if matches!(
                            tool_name.as_str(),
                            "tonglingyu.evidence.package.create"
                                | "tonglingyu.evidence.package.read"
                                | "tonglingyu.evidence.package.replay"
                        ) {
                            package_id
                                .as_ref()
                                .map(|package_id| {
                                    format!(
                                        "runtime://tonglingyu/{}/packages/{package_id}",
                                        input.trace_id
                                    )
                                })
                                .unwrap_or_else(|| {
                                    format!(
                                        "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                        input.trace_id
                                    )
                                })
                        } else if matches!(
                            tool_name.as_str(),
                            "tonglingyu.text.search" | "tonglingyu.commentary.search"
                        ) {
                            evidence_set_ref_from_step_message(&message).unwrap_or_else(|| {
                                evidence_set_output_ref(
                                    &input.trace_id,
                                    &evidence_ids_from_step_message(&message),
                                )
                            })
                        } else {
                            format!(
                                "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                input.trace_id
                            )
                        };
                        json!({
                            "call_id": format!("call-runtime-{operation}-{index}"),
                            "profile_id": input.profile_id,
                            "tool_name": tool_name,
                            "output_ref": output_ref,
                        })
                    })
                    .collect(),
            )
        };
        let tool_audit_events = if input.requested_tools.is_empty() {
            json!([])
        } else {
            Value::Array(
                input
                    .requested_tools
                    .iter()
                    .map(|tool_name| {
                        json!({
                            "event": "runtime_tool_result",
                            "tool_name": tool_name,
                            "trace_id": input.trace_id,
                        })
                    })
                    .collect(),
            )
        };
        Ok(RuntimeOutput {
                result_summary: match operation.as_str() {
                    "text_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "evidence_refs": evidence_ids_from_step_message(&message),
                            "evidence_analysis": "Hermes observed text evidence refs",
                            "unsupported_scope": "observation only; local runtime evidence is enforced",
                        }
                    }))
                    .expect("text evidence output serializes"),
                    "commentary_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "commentary_refs": evidence_ids_from_step_message(&message),
                            "commentary_analysis": "Hermes observed commentary evidence refs",
                            "scope_notes": "commentary is first-class evidence within the default pre-80 scope",
                        }
                    }))
                    .expect("commentary evidence output serializes"),
                    "draft_answer" => upstream_bundle_summary_with_policy(
                        source_scope_policy_from_step_message(&message),
                        &package_id_from_step_message(&message)
                            .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                        &package_id_from_step_message(&message)
                            .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                        &public_draft,
                        &public_draft,
                        evidence_ids_from_step_message(&message),
                    ),
                    "evidence_package_create" => serde_json::to_string(&json!({
                        "package_observation": {
                            "package_id": package_id_from_step_message(&message)
                                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                            "summary": "Hermes observed runtime package ref",
                        }
                    }))
                    .expect("package output serializes"),
                    "review_answer" => serde_json::to_string(&json!({
                        "review_observation": {
                            "review_status": "passed",
                            "severity": "none",
                            "issues": [],
                            "required_revisions": [],
                        }
                    }))
                    .expect("review output serializes"),
                    _ => format!("Hermes full workflow step {operation}. context={message}"),
                },
                result_ref: Some(format!(
                    "result://draft-runtime/{}/{}",
                    input.profile_id, operation
                )),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "operation": operation,
                    "tool_rounds": tool_rounds,
                    "tool_results": tool_results,
                    "tool_audit_events": tool_audit_events,
                }),
            })
    }
}

#[async_trait]
impl RuntimeClient for NoToolRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "no-tool runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "no-tool runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let operation = input
            .runtime_step
            .as_ref()
            .and_then(|step| step.metadata.get("operation"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let public_draft = public_draft_from_step_message(&message);
        Ok(RuntimeOutput {
                result_summary: match operation.as_str() {
                    "text_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "evidence_refs": evidence_ids_from_step_message(&message),
                            "evidence_analysis": "Hermes observed text evidence refs without model tool calls",
                            "unsupported_scope": "observation only; local runtime evidence is enforced",
                        }
                    }))
                    .expect("text evidence output serializes"),
                    "commentary_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "commentary_refs": evidence_ids_from_step_message(&message),
                            "commentary_analysis": "Hermes observed commentary evidence refs without model tool calls",
                            "scope_notes": "commentary is first-class evidence within the default pre-80 scope",
                        }
                    }))
                    .expect("commentary evidence output serializes"),
                    "draft_answer" => upstream_bundle_summary_with_policy(
                        source_scope_policy_from_step_message(&message),
                        &package_id_from_step_message(&message)
                            .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                        &package_id_from_step_message(&message)
                            .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                        &public_draft,
                        &public_draft,
                        evidence_ids_from_step_message(&message),
                    ),
                    "evidence_package_create" => serde_json::to_string(&json!({
                        "package_observation": {
                            "package_id": package_id_from_step_message(&message)
                                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                            "summary": "Hermes observed runtime package ref without model tool calls",
                        }
                    }))
                    .expect("package output serializes"),
                    "review_answer" => serde_json::to_string(&json!({
                        "review_observation": {
                            "review_status": "passed",
                            "severity": "none",
                            "issues": [],
                            "required_revisions": [],
                        }
                    }))
                    .expect("review output serializes"),
                    _ => format!("Hermes full workflow step {operation}. context={message}"),
                },
                result_ref: Some(format!("result://no-tool-runtime/{}", input.profile_id)),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "latency_ms": 250,
                    "tool_rounds": 0,
                    "tool_results": [],
                    "tool_audit_events": [],
                }),
            })
    }
}

#[async_trait]
impl RuntimeClient for PartialCoverageDraftRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "partial-coverage runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "partial-coverage runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let operation = input
            .runtime_step
            .as_ref()
            .and_then(|step| step.metadata.get("operation"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let mut output = NoToolRuntimeClient.execute_profile_step(input).await?;
        if operation == "draft_answer" {
            let package_id = package_id_from_step_message(&message)
                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string());
            output.result_summary = serde_json::to_string(&json!({
                "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
                "package_id": package_id,
                "source_scope_policy": source_scope_policy_from_step_message(&message),
                "draft_candidate": {
                    "draft_answer": "上游认为当前包不足，交由本地治理边界处理。",
                    "package_id": package_id,
                    "claim_statements": [{
                        "text": "上游 coverage 未通过时，本地治理仍保留包边界。",
                        "evidence_refs": evidence_ids_from_step_message(&message),
                    }],
                },
                "coverage_assessment": {
                    "status": "insufficient",
                    "missing_in_scope_slots": ["上游未能完成覆盖判断。"],
                    "out_of_scope_slots": [],
                },
                "evidence_hints": [],
                "retrieval_repair": {
                    "recommended": false,
                    "queries": [],
                },
                "out_of_scope_hints": [],
            }))
            .expect("partial coverage output serializes");
        }
        Ok(output)
    }
}

#[async_trait]
impl RuntimeClient for InvalidEvidenceRefDraftRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "invalid-evidence-ref runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "invalid-evidence-ref runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let operation = input
            .runtime_step
            .as_ref()
            .and_then(|step| step.metadata.get("operation"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let mut output = NoToolRuntimeClient.execute_profile_step(input).await?;
        if operation == "draft_answer" {
            let package_id = package_id_from_step_message(&message)
                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string());
            output.result_summary = upstream_bundle_summary_with_policy(
                source_scope_policy_from_step_message(&message),
                &package_id,
                &package_id,
                "上游草稿引用了不在本地证据包内的证据，必须由本地治理拒收。",
                "上游草稿的证据引用必须绑定当前 package。",
                vec!["ev-outside-package".to_string()],
            );
        }
        Ok(output)
    }
}

#[async_trait]
impl RuntimeClient for EvidenceBoundaryDraftRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "evidence-boundary runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "evidence-boundary runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let operation = input
            .runtime_step
            .as_ref()
            .and_then(|step| step.metadata.get("operation"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let mut output = NoToolRuntimeClient.execute_profile_step(input).await?;
        if operation == "draft_answer" {
            let package_id = package_id_from_step_message(&message)
                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string());
            output.result_summary = upstream_bundle_summary_with_policy(
                source_scope_policy_from_step_message(&message),
                &package_id,
                &package_id,
                "通灵玉是贾府亲戚一支的象征，这个说法必须被本地证据边界拒收。",
                "上游草稿包含本地证据没有支撑的亲戚判断。",
                evidence_ids_from_step_message(&message),
            );
        }
        Ok(output)
    }
}

#[async_trait]
impl RuntimeClient for ProviderRequestRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "provider-request runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "provider-request runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let provider_request = json!({
            "schema_version": "openai-compatible-provider-request-v1",
            "runtime_adapter": "openai-compatible-network",
            "trace_id": &input.trace_id,
            "profile_id": &input.profile_id,
            "model": "provider-request-test-model",
            "messages": &input.messages,
            "message_count": input.messages.len(),
            "stream": false,
            "response_format": {"type": "json_object"},
            "authorization_header_embedded": false,
            "api_key_embedded": false,
            "secret_values_printed": false,
        });
        let mut output = NoToolRuntimeClient.execute_profile_step(input).await?;
        output.metadata["provider_request_sha256"] =
            json!(format!("sha256:{}", hash_json(&provider_request)));
        output.metadata["provider_request_embedded"] = json!(true);
        output.metadata["provider_request"] = provider_request;
        Ok(output)
    }
}

#[async_trait]
impl RuntimeClient for BadOutputRefRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "bad-output-ref runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "bad-output-ref runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let tool_results = Value::Array(
            input
                .requested_tools
                .iter()
                .enumerate()
                .map(|(index, tool_name)| {
                    json!({
                        "call_id": format!("call-bad-output-ref-{index}"),
                        "profile_id": input.profile_id,
                        "tool_name": tool_name,
                        "output_ref": format!("runtime://tool-results/{index}"),
                    })
                })
                .collect(),
        );
        Ok(RuntimeOutput {
            result_summary: "{}".to_string(),
            result_ref: Some(format!(
                "result://bad-output-ref-runtime/{}",
                input.profile_id
            )),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": input.profile_id,
                "trace_id": input.trace_id,
                "tool_rounds": 1,
                "tool_results": tool_results,
                "tool_audit_events": [],
            }),
        })
    }
}

#[async_trait]
impl RuntimeClient for IncompleteHermesContentRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "incomplete-hermes-content runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "incomplete-hermes-content runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let operation = input
            .runtime_step
            .as_ref()
            .and_then(|step| step.metadata.get("operation"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let package_id = package_id_from_step_message(&message);
        let tool_results = Value::Array(
            input
                .requested_tools
                .iter()
                .enumerate()
                .map(|(index, tool_name)| {
                    let output_ref = if matches!(
                        tool_name.as_str(),
                        "tonglingyu.text.search" | "tonglingyu.commentary.search"
                    ) {
                        evidence_set_ref_from_step_message(&message).unwrap_or_else(|| {
                            evidence_set_output_ref(
                                &input.trace_id,
                                &evidence_ids_from_step_message(&message),
                            )
                        })
                    } else if matches!(
                        tool_name.as_str(),
                        "tonglingyu.evidence.package.create"
                            | "tonglingyu.evidence.package.read"
                            | "tonglingyu.evidence.package.replay"
                    ) {
                        package_id
                            .as_ref()
                            .map(|package_id| {
                                format!(
                                    "runtime://tonglingyu/{}/packages/{package_id}",
                                    input.trace_id
                                )
                            })
                            .unwrap_or_else(|| {
                                format!(
                                    "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                    input.trace_id
                                )
                            })
                    } else {
                        format!(
                            "runtime://tonglingyu/{}/tools/{operation}/{index}",
                            input.trace_id
                        )
                    };
                    json!({
                        "call_id": format!("call-incomplete-hermes-{operation}-{index}"),
                        "profile_id": input.profile_id,
                        "tool_name": tool_name,
                        "output_ref": output_ref,
                    })
                })
                .collect(),
        );
        Ok(RuntimeOutput {
            result_summary: "{}".to_string(),
            result_ref: Some(format!(
                "result://incomplete-hermes-content/{}",
                input.profile_id
            )),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": input.profile_id,
                "trace_id": input.trace_id,
                "operation": operation,
                "tool_rounds": if input.requested_tools.is_empty() { 0 } else { 1 },
                "tool_results": tool_results,
                "tool_audit_events": [],
            }),
        })
    }
}

#[async_trait]
impl RuntimeClient for MissingToolAuditRuntimeClient {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        DraftRuntimeClient.execute_run(input).await
    }

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        DraftRuntimeClient.send_session_message(input).await
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let mut output = DraftRuntimeClient.execute_profile_step(input).await?;
        output.metadata["tool_audit_events"] = json!([]);
        Ok(output)
    }
}

#[async_trait]
impl RuntimeClient for WrongEvidenceOutputRefRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "wrong-evidence-output-ref runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "wrong-evidence-output-ref runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let tool_results = Value::Array(
            input
                .requested_tools
                .iter()
                .enumerate()
                .map(|(index, tool_name)| {
                    let output_ref = if matches!(
                        tool_name.as_str(),
                        "tonglingyu.text.search" | "tonglingyu.commentary.search"
                    ) {
                        format!("runtime://tonglingyu/{}/evidence/wrong-set", input.trace_id)
                    } else {
                        format!("runtime://tonglingyu/{}/tools/{index}", input.trace_id)
                    };
                    json!({
                        "call_id": format!("call-wrong-evidence-output-ref-{index}"),
                        "profile_id": input.profile_id,
                        "tool_name": tool_name,
                        "output_ref": output_ref,
                    })
                })
                .collect(),
        );
        Ok(RuntimeOutput {
            result_summary: "{}".to_string(),
            result_ref: Some(format!(
                "result://wrong-evidence-output-ref-runtime/{}",
                input.profile_id
            )),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": input.profile_id,
                "trace_id": input.trace_id,
                "tool_rounds": 1,
                "tool_results": tool_results,
                "tool_audit_events": [],
            }),
        })
    }
}

#[async_trait]
impl RuntimeClient for FailingProfileRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "failing-profile runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "failing-profile runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::InternalError,
            format!("profile {} backend unavailable", input.profile_id),
        ))
    }
}

#[async_trait]
impl RuntimeClient for DiagnosticFailingProfileRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "diagnostic-failing-profile runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "diagnostic-failing-profile runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded_with_diagnostic(
            ErrorCode::InternalError,
            format!(
                "OpenAI-compatible Runtime response did not include assistant content for {} (provider_empty_content)",
                input.profile_id
            ),
            json!({
                "schema_version": "openai-compatible-provider-diagnostic-v1",
                "error_type": "provider_empty_content",
                "attempt": 2,
                "retryable": true,
                "status_code": 200,
                "provider_model": "direct-test-model",
                "choice_count": 1,
                "content_present": true,
                "content_len": 0,
                "raw_response_body_embedded": false,
                "raw_content_embedded": false,
                "secret_values_printed": false,
            }),
        ))
    }
}

#[async_trait]
impl RuntimeClient for TimeoutProfileRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "timeout-profile runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "timeout-profile runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, _input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::InternalError,
            "Hermes Runtime timed out",
        ))
    }
}

#[async_trait]
impl RuntimeClient for FlakyProfileRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "flaky-profile runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "flaky-profile runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        use std::sync::atomic::Ordering;

        if self.failures.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                "transient profile failure",
            ));
        }
        DraftRuntimeClient.execute_profile_step(input).await
    }
}

#[async_trait]
impl RuntimeClient for DraftRepairRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "draft-repair runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "draft-repair runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        if !message.contains("draft_repair_context_json") {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "draft-repair runtime requires repair context",
            ));
        }
        if !message.contains("\"direct_count\":2")
            || !message.contains("良儿偷玉")
            || !message.contains("凤姐扫雪拾玉")
            || !message.contains("甄宝玉送玉")
        {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "draft-repair runtime requires structured slot count context",
            ));
        }
        let package_id =
            package_id_from_step_message(&message).unwrap_or_else(|| "pkg-missing".to_string());
        Ok(RuntimeOutput {
            result_summary: upstream_bundle_summary_with_policy(
                source_scope_policy_from_step_message(&message),
                &package_id,
                &package_id,
                "按当前材料的默认范围，可以说有两处明确失玉证据：第五十二回良儿偷玉、脂批第二十三回称凤姐扫雪拾玉；另有脂批第十八回伏甄宝玉送玉，属于疑似流转线索。",
                "默认范围内的正文和脂批证据支持两处明确失玉证据，并保留一条疑似流转线索。",
                evidence_ids_from_step_message(&message),
            ),
            result_ref: Some(format!(
                "result://draft-repair-runtime/{}",
                input.profile_id
            )),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": input.profile_id,
                "trace_id": input.trace_id,
                "operation": "draft_answer",
                "tool_rounds": 0,
                "tool_results": [],
                "tool_audit_events": [],
            }),
        })
    }
}

#[async_trait]
impl RuntimeClient for OpenObjectDraftRepairRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "open-object draft-repair runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "open-object draft-repair runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let message = input
            .messages
            .first()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        if !message.contains("question_frame_answer_requirements")
            || !message.contains("史大姑娘")
            || !message.contains("draft_missing_open_relation_object")
        {
            return Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "open-object draft repair requires structured relation object context",
            ));
        }
        let package_id =
            package_id_from_step_message(&message).unwrap_or_else(|| "pkg-missing".to_string());
        Ok(RuntimeOutput {
            result_summary: upstream_bundle_summary_with_policy(
                source_scope_policy_from_step_message(&message),
                &package_id,
                &package_id,
                "据第3回与第19回，袭人先后服侍过贾母、史大姑娘和宝玉：第3回称她本是贾母之婢，后与了宝玉；第19回又有“先伏侍了史大姑娘几年，如今又伏侍了你几年”。",
                "第3回和第19回材料共同支持袭人的服侍对象清单。",
                evidence_ids_from_step_message(&message),
            ),
            result_ref: Some(format!(
                "result://open-object-draft-repair-runtime/{}",
                input.profile_id
            )),
            messages: Vec::new(),
            metadata: json!({
                "runtime_profile": input.profile_id,
                "trace_id": input.trace_id,
                "operation": "draft_answer",
                "tool_rounds": 0,
                "tool_results": [],
                "tool_audit_events": [],
            }),
        })
    }
}

#[async_trait]
impl RuntimeClient for SlowDraftRuntimeClient {
    async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        DraftRuntimeClient.execute_run(input).await
    }

    async fn send_session_message(&self, input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        DraftRuntimeClient.send_session_message(input).await
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        use std::sync::atomic::Ordering;

        let runtime_step = input.runtime_step.clone();
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let mut output = DraftRuntimeClient.execute_profile_step(input).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if let (Ok(output), Some(runtime_step)) = (&mut output, runtime_step) {
            output.metadata["runtime_step"] = json!(runtime_step);
            output.metadata["runtime_step"]["status"] = json!("completed");
        }
        output
    }
}

fn step_output_from_message(message: &str) -> Option<Value> {
    message.lines().find_map(|line| {
        let value = line.strip_prefix("step_output_json: ")?;
        serde_json::from_str::<Value>(value).ok()
    })
}

fn package_id_from_step_message(message: &str) -> Option<String> {
    step_output_from_message(message)?
        .get("package_id")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn source_scope_policy_from_step_message(message: &str) -> Value {
    step_output_from_message(message)
        .and_then(|value| value.get("source_scope_policy").cloned())
        .unwrap_or_else(|| json!(source_scope_policy_for_question("")))
}

fn evidence_set_ref_from_step_message(message: &str) -> Option<String> {
    step_output_from_message(message)?
        .get("evidence_set_ref")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn evidence_ids_from_step_message(message: &str) -> Vec<String> {
    step_output_from_message(message)
        .and_then(|value| {
            value
                .get("evidence_ids")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_default()
}

fn public_draft_from_step_message(message: &str) -> String {
    let source_and_text = step_output_from_message(message)
        .and_then(|value| {
            let first = value.get("evidence_brief")?.as_array()?.first()?;
            let source_title = first
                .get("source_title")
                .and_then(Value::as_str)
                .map(|value| trim_text(value, 32))
                .unwrap_or_else(|| "当前材料".to_string());
            let text = first
                .get("text")
                .and_then(Value::as_str)
                .map(|value| trim_text(value, 48))
                .unwrap_or_else(|| "原文片段".to_string());
            Some((source_title, text))
        })
        .unwrap_or_else(|| ("当前材料".to_string(), "原文片段".to_string()));
    format!(
        "根据{}“{}”，可以在当前材料范围内作答；结论不外推到未给出的版本。",
        source_and_text.0, source_and_text.1
    )
}

fn upstream_bundle_summary(
    question: &str,
    package_id: &str,
    draft_answer: &str,
    claim_text: &str,
    evidence_refs: Vec<String>,
) -> String {
    upstream_bundle_summary_with_policy(
        json!(source_scope_policy_for_question(question)),
        package_id,
        package_id,
        draft_answer,
        claim_text,
        evidence_refs,
    )
}

fn upstream_bundle_summary_with_candidate_package(
    question: &str,
    bundle_package_id: &str,
    candidate_package_id: &str,
    draft_answer: &str,
    claim_text: &str,
    evidence_refs: Vec<String>,
) -> String {
    upstream_bundle_summary_with_policy(
        json!(source_scope_policy_for_question(question)),
        bundle_package_id,
        candidate_package_id,
        draft_answer,
        claim_text,
        evidence_refs,
    )
}

fn upstream_bundle_summary_with_policy(
    source_scope_policy: Value,
    bundle_package_id: &str,
    candidate_package_id: &str,
    draft_answer: &str,
    claim_text: &str,
    evidence_refs: Vec<String>,
) -> String {
    serde_json::to_string(&json!({
        "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
        "package_id": bundle_package_id,
        "source_scope_policy": source_scope_policy,
        "draft_candidate": {
            "draft_answer": draft_answer,
            "package_id": candidate_package_id,
            "claim_statements": [{
                "text": claim_text,
                "evidence_refs": evidence_refs,
            }],
        },
        "coverage_assessment": {
            "status": "passed",
            "missing_in_scope_slots": [],
            "out_of_scope_slots": [],
        },
        "evidence_hints": [],
        "retrieval_repair": {
            "recommended": false,
            "queries": [],
        },
        "out_of_scope_hints": [],
    }))
    .expect("upstream bundle serializes")
}

fn sample_card(evidence_type: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: format!("ev-test-{evidence_type}"),
        evidence_type: evidence_type.to_string(),
        source_id: "test-source".to_string(),
        source_title: "test-title".to_string(),
        source_url: "https://example.test/source".to_string(),
        revision_id: Some(1),
        block_id: format!("block-test-{evidence_type}"),
        text: "脂批：测试证据".to_string(),
        support_scope: "测试支持范围".to_string(),
        unsupported_scope: "测试不支持范围".to_string(),
        evidence_level: "测试层级".to_string(),
        confidence: "medium".to_string(),
        verification_status: "test".to_string(),
    }
}

fn test_retrieved_evidence(
    question: &str,
    cards: Vec<EvidenceCard>,
) -> RuntimeWorkflowRetrievedEvidence {
    RuntimeWorkflowRetrievedEvidence {
        schema_version: RUNTIME_WORKFLOW_RETRIEVED_EVIDENCE_SCHEMA_VERSION.to_string(),
        source: "knownledge_http_retriever".to_string(),
        cards,
        retrieval: json!({
            "schema_version": RUNTIME_WORKFLOW_RETRIEVED_EVIDENCE_SCHEMA_VERSION,
            "request": {
                "search_plan": {
                    "schema_version": "tonglingyu.agent_retriever.search_plan.v1",
                    "query": question,
                    "routes": ["bm25", "vector", "entity", "event", "poem", "commentary"],
                },
                "retrieve_options": {
                    "schema_version": "tonglingyu.agent_retriever.retrieve_options.v1",
                    "trace_level": "route",
                },
            },
            "request_response_trace": {
                "transport": "http",
                "tool_name": "tonglingyu.agent_retriever.retrieve_http",
                "method": "POST",
                "path": "/retrieve",
                "request": {
                    "search_plan": {
                        "query": question,
                    },
                },
                "response": {
                    "schema_version": "tonglingyu.agent_retriever.retrieve_response.v1",
                },
            },
            "raw_retrieve_response": {
                "schema_version": "tonglingyu.agent_retriever.retrieve_response.v1",
                "record_type": "agent_retriever_retrieve_response",
            },
            "evidence_pack": {
                "schema_version": "tonglingyu.agent_retriever.evidence_pack.v1",
            },
            "diagnostics": {
                "sufficiency": {
                    "doc_count": 1,
                },
            },
        }),
    }
}

fn insert_test_source_hash(conn: &Connection, source_id: &str, source_hash: &str) {
    conn.execute(
        r#"
        INSERT OR REPLACE INTO sources (
            source_id, source_category, format, title, work, edition, language,
            source_url, api_url, fetched_at, license, license_url,
            license_source_url, attribution, usage_boundary, notes,
            snapshot_contract_json, source_hash
        ) VALUES (
            ?1, 'base_material', 'mediawiki', 'Test Source', '红楼梦',
            '120回', 'zh', 'https://example.test/source', NULL,
            '2026-01-01T00:00:00Z', 'test-license', NULL, NULL,
            'test attribution', 'test usage boundary', 'test notes', '{}',
            ?2
        )
        "#,
        rusqlite::params![source_id, source_hash],
    )
    .expect("insert test source hash");
}

fn runtime_policy_test_card(marker: &str) -> EvidenceCard {
    let mut card = sample_card("base_text");
    card.evidence_id = format!("ev-runtime-policy-{marker}");
    card.source_id = format!("wikisource/chapter/{marker}");
    card.source_title = format!("Wikisource chapter {marker}");
    card.block_id = format!("wikisource/{marker}");
    card.text = format!("sample knowledge item {marker}");
    card.evidence_level = "正文直接".to_string();
    card.confidence = "high".to_string();
    card.verification_status = "source_snapshot".to_string();
    card
}

#[test]
fn package_snapshots_promoted_cards_with_package_scoped_ids() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let mut promoted = sample_card("base_text");
    promoted.evidence_id = "evc-promoted-global-card".to_string();
    promoted.block_id = "block-promoted-global-card".to_string();
    promoted.text = "林黛玉年方五歲，進榮國府前後可據此作有限推算。".to_string();
    promoted.verification_status = "online_promoted_source_backed".to_string();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO evidence_cards (evidence_id, package_id, evidence_type, source_id, block_id, support_scope, unsupported_scope, evidence_level, confidence, verification_status, evidence_json, created_at) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            &promoted.evidence_id,
            &promoted.evidence_type,
            &promoted.source_id,
            &promoted.block_id,
            &promoted.support_scope,
            &promoted.unsupported_scope,
            &promoted.evidence_level,
            &promoted.confidence,
            &promoted.verification_status,
            serde_json::to_string(&promoted).expect("promoted evidence serializes"),
            &now,
        ],
    )
    .expect("insert promoted card");

    let package = create_evidence_package(
        &conn,
        "trace-package-promoted-card",
        "林黛玉进贾府时多大了",
        vec![promoted.clone()],
    )
    .expect("package accepts promoted card snapshot");

    assert_ne!(package.cards[0].evidence_id, promoted.evidence_id);
    assert!(package.cards[0].evidence_id.starts_with("evpkg-"));
    let loaded = load_evidence_package_from_conn(&conn, &package.package_id)
        .expect("load package")
        .expect("package exists");
    assert_eq!(loaded.cards[0].evidence_id, package.cards[0].evidence_id);
    let evidence_card_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM evidence_cards", [], |row| row.get(0))
        .expect("count evidence cards");
    assert_eq!(evidence_card_count, 2);
}

#[test]
fn package_records_admin_only_online_learning_trace() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-source-snapshot-online-learning".to_string();
    card.text = "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string();
    card.verification_status = "source_snapshot_ready_not_scholarly_collated".to_string();

    let package = create_evidence_package(
        &conn,
        "trace-package-online-learning",
        "林黛玉进贾府时多大了",
        vec![card],
    )
    .expect("package");

    assert_eq!(package.tiered_evidence_bindings.len(), 1);
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_tier,
        "request_raw_full_text_hit"
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].answer_use,
        "supplemental_only"
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_gate["status"],
        json!("downgraded")
    );
    assert!(
        package
            .online_learning_trace
            .as_ref()
            .is_some_and(|trace| trace.online_learning_trace_id.starts_with("olt-"))
    );
    let events = runtime_audit_events_for_trace(&conn, "trace-package-online-learning")
        .expect("audit events");
    let online_learning_event = events
        .iter()
        .find(|event| event["event_type"] == json!("online_learning_trace_recorded"))
        .expect("online learning trace event");
    assert_eq!(
        online_learning_event["payload"]["tiered_evidence_bindings"][0]["evidence_tier"],
        json!("request_raw_full_text_hit")
    );
    let public_package =
        serde_json::to_string(&package_json(&package)).expect("package json serializes");
    assert!(!public_package.contains("tiered_evidence_bindings"));
    assert!(!public_package.contains("request_scoped_evidence"));
    assert!(!public_package.contains("request_raw_full_text_hit"));
    assert!(!public_package.contains("online_learning_trace"));
}

#[test]
fn local_answer_keeps_raw_full_text_hit_as_supplemental_only() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-source-snapshot-raw-only-answer".to_string();
    card.text = "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string();
    card.verification_status = "source_snapshot_ready_not_scholarly_collated".to_string();

    let package = create_evidence_package(
        &conn,
        "trace-raw-only-answer",
        "林黛玉进贾府时多大了",
        vec![card],
    )
    .expect("package");

    assert_eq!(package.review.status, "needs_revision");
    assert!(
        package
            .review
            .issues
            .iter()
            .any(|issue| issue.contains("补充线索"))
    );
    let answer = local_answer("林黛玉进贾府时多大了", &package);
    assert!(answer.contains("补充线索"));
    assert!(answer.contains("还不能直接支撑结论"));
    assert!(!answer.contains("request_raw_full_text_hit"));
}

#[test]
fn local_answer_renders_request_scoped_evidence_with_bounded_public_wording() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    insert_test_source_hash(&conn, "test-source", "source-hash-local-answer");
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-source-snapshot-request-bound-answer".to_string();
    card.text = "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string();
    card.verification_status = "source_snapshot_ready_not_scholarly_collated".to_string();

    let package = create_evidence_package(
        &conn,
        "trace-request-bound-answer",
        "请说明文本证据",
        vec![card],
    )
    .expect("package");

    assert_eq!(
        package.tiered_evidence_bindings[0].answer_use,
        "request_bound_basis"
    );
    assert!(
        !package.tiered_evidence_bindings[0]
            .source_scope_policy_sha256
            .is_empty()
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_gate["status"],
        json!("passed")
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_gate["decision_answer_use"],
        json!("request_bound_basis")
    );
    assert_eq!(package.review.status, "passed");
    let answer = local_answer("请说明文本证据", &package);
    assert!(answer.starts_with("就本次检索到的可追溯材料看"));
    assert!(!answer.contains("request_scoped_evidence"));
    assert!(!answer.contains("request_bound_basis"));
}

#[test]
fn local_answer_renders_promoted_evidence_as_registered_public_basis() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    insert_test_source_hash(&conn, "test-source", "source-hash-promoted-answer");
    let mut card = sample_card("base_text");
    card.evidence_id = "evc-promoted-public-wording-answer".to_string();
    card.text = "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string();
    card.verification_status = "online_promoted_source_backed".to_string();

    let package = create_evidence_package(
        &conn,
        "trace-promoted-public-wording",
        "请说明文本证据",
        vec![card],
    )
    .expect("package");

    assert_eq!(
        package.tiered_evidence_bindings[0].answer_use,
        "stable_basis"
    );
    assert!(
        !package.tiered_evidence_bindings[0]
            .source_scope_policy_sha256
            .is_empty()
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_gate["status"],
        json!("passed")
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_gate["decision_answer_use"],
        json!("stable_basis")
    );
    assert_eq!(package.review.status, "passed");
    let answer = local_answer("请说明文本证据", &package);
    assert!(answer.starts_with("基于当前已登记资料"));
    assert!(!answer.contains("promoted_evidence_card"));
    assert!(!answer.contains("stable_basis"));
}

#[test]
fn reviewer_downgrades_answer_basis_when_tier_gate_is_incomplete() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let mut card = sample_card("base_text");
    card.evidence_id = "evc-promoted-without-source-hash".to_string();
    card.text = "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string();
    card.verification_status = "online_promoted_source_backed".to_string();

    let package = create_evidence_package(
        &conn,
        "trace-promoted-without-source-hash",
        "请说明文本证据",
        vec![card],
    )
    .expect("package");

    assert_eq!(
        package.tiered_evidence_bindings[0].answer_use,
        "stable_basis"
    );
    assert_eq!(
        package.tiered_evidence_bindings[0].evidence_gate["status"],
        json!("downgraded")
    );
    assert_eq!(package.review.status, "needs_revision");
    assert!(
        package
            .review
            .issues
            .iter()
            .any(|issue| issue.contains("证据层级门槛未通过"))
    );
}

#[test]
fn upstream_draft_using_only_supplemental_evidence_is_rejected() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-source-snapshot-raw-only-draft".to_string();
    card.text = "黛玉方進榮國府，賈母等敘其年幼情狀。".to_string();
    card.verification_status = "source_snapshot_ready_not_scholarly_collated".to_string();
    let package = create_evidence_package(
        &conn,
        "trace-raw-only-draft",
        "林黛玉进贾府时多大了",
        vec![card],
    )
    .expect("package");
    let extraction = upstream_bundle::UpstreamBundleDraftExtraction {
        draft_answer: Some("林黛玉进贾府时尚幼。".to_string()),
        result_format: "json",
        package_id: Some(package.package_id.clone()),
        package_id_rebound: false,
        observed_bundle_package_id: Some(package.package_id.clone()),
        observed_candidate_package_id: Some(package.package_id.clone()),
        claim_statement_count: Some(1),
        claim_evidence_refs: vec![vec![package.cards[0].evidence_id.clone()]],
        rejected_reason: None,
        coverage_status: Some("passed".to_string()),
        evidence_hint_count: Some(0),
        retrieval_repair_recommended: Some(false),
        retrieval_repair_queries: Vec::new(),
        out_of_scope_hint_count: Some(0),
    };

    assert_eq!(
        agent_runtime_draft_evidence_tier_rejection(&extraction, &package),
        Some("draft_claim_uses_only_supplemental_evidence")
    );
}

fn yousanjie_test_cards() -> Vec<EvidenceCard> {
    let mut vow = sample_card("base_text");
    vow.evidence_id = "ev-yousanjie-vow".to_string();
    vow.source_title = "紅樓夢/第066回".to_string();
    vow.block_id = "block-yousanjie-vow".to_string();
    vow.text = "二人正說之間，只見尤三姐走來說道：“姐夫，你只放心。我們不是那心口兩樣的人，說什麼是什麼。若有了姓柳的來，我便嫁他。”說著，將一根玉簪，擊作兩段。".to_string();
    vow.evidence_level = "正文直接".to_string();
    let mut death = sample_card("base_text");
    death.evidence_id = "ev-yousanjie-death".to_string();
    death.source_title = "紅樓夢/第067回".to_string();
    death.block_id = "block-yousanjie-death".to_string();
    death.text = "話說尤三姐自盡之後，尤老娘和二姐兒，賈珍，賈璉等俱不胜悲慟。柳湘蓮見尤三姐身亡，痴情眷戀，卻被道人數句冷言打破迷關，竟自截發出家。薛姨媽說：珍大嫂子的妹妹三姑娘，已經許定給柳湘蓮了。".to_string();
    death.evidence_level = "正文直接".to_string();
    vec![vow, death]
}

fn lost_jade_test_cards() -> Vec<EvidenceCard> {
    let mut loss = sample_card("base_text");
    loss.evidence_id = "ev-lost-jade-94".to_string();
    loss.source_title = "紅樓夢/第094回".to_string();
    loss.block_id = "block-lost-jade-94".to_string();
    loss.text = "襲人見寶玉脖子上沒有挂著，便問：“那塊玉呢？”襲人回看桌上并沒有玉，便向各處找尋，蹤影全無。王夫人問：“那塊玉真丟了么？”".to_string();
    loss.evidence_level = "正文直接".to_string();
    let mut returned = sample_card("base_text");
    returned.evidence_id = "ev-lost-jade-return".to_string();
    returned.source_title = "紅樓夢/第116回".to_string();
    returned.block_id = "block-lost-jade-return".to_string();
    returned.text = "王夫人等放心，只叫人仍把那玉交給寶釵給他帶上。寶釵道：“頭里丟的時候，必是那和尚取去的。”襲人麝月道：“那年丟了玉。”".to_string();
    returned.evidence_level = "正文直接".to_string();
    vec![loss, returned]
}

fn in_scope_lost_jade_event_cards() -> Vec<EvidenceCard> {
    let mut lianger = sample_card("base_text");
    lianger.evidence_id = "ev-lianger-stole-jade".to_string();
    lianger.source_title = "紅樓夢/第五十二回".to_string();
    lianger.block_id = "block-lianger-stole-jade".to_string();
    lianger.text = "平兒道：“寶玉是偏在你們身上留心用意、爭勝要強的，那一年有一個良兒偷玉，剛冷了一二年間，還有人提起來趁願。”".to_string();
    let mut zhen = sample_card("commentary");
    zhen.evidence_id = "ev-zhen-baoyu-delivers-jade".to_string();
    zhen.source_title = "脂硯齋重評石頭記/第十八回".to_string();
    zhen.block_id = "block-zhen-baoyu-delivers-jade".to_string();
    zhen.text = "第三齣《仙緣》；{{~|【庚辰雙行夾批：《邯鄲夢》中伏甄寶玉送玉。】}}".to_string();
    let mut fengjie = sample_card("commentary");
    fengjie.evidence_id = "ev-fengjie-snow-pickup-jade".to_string();
    fengjie.source_title = "脂硯齋重評石頭記/第二十三回".to_string();
    fengjie.block_id = "block-fengjie-snow-pickup-jade".to_string();
    fengjie.text = "剛至穿堂門前，{{~|【庚辰雙行夾批：妙！這便是鳳姐掃雪拾玉之處，一絲不亂。】}}只見襲人倚門立在那裡。".to_string();
    vec![lianger, zhen, fengjie]
}

fn seed_lost_jade_runtime_blocks(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    for (block_id, source_title, chapter_no, block_index, text) in [
        (
            "quality-block-lost-jade-body",
            "紅樓夢/第九十四回",
            94_i64,
            2_i64,
            "襲人見寶玉脖子上沒有挂著，便問：“那塊玉呢？”襲人回看桌上并沒有玉，便向各處找尋，蹤影全無。王夫人問：“那塊玉真丟了么？”",
        ),
        (
            "quality-block-lost-jade-return",
            "紅樓夢/第一百十六回",
            116_i64,
            3_i64,
            "王夫人等放心，只叫人仍把那玉交給寶釵給他帶上。寶釵道：“頭里丟的時候，必是那和尚取去的。”襲人麝月道：“那年丟了玉。”",
        ),
    ] {
        conn.execute(
            r#"
                INSERT INTO blocks (
                    block_id, source_id, section_id, source_title, normalized_source_title,
                    source_url, revision_id, block_index, kind, tag, text, normalized_text,
                    evidence_type, chapter_no
                ) VALUES (?1, 'quality-source', 'quality-section-lost-jade',
                    ?2, ?3, 'https://example.test/source/lost-jade',
                    1, ?4, 'paragraph', NULL, ?5, ?6, 'base_text', ?7)
                "#,
            params![
                block_id,
                source_title,
                normalize_title(source_title),
                block_index,
                text,
                normalize_text(text),
                chapter_no,
            ],
        )
        .expect("insert lost jade runtime block");
    }
}

fn seed_basic_tonglingyu_runtime_block(conn: &Connection) {
    let source_title = "紅樓夢/第008回";
    let text = "寶玉項上所佩之玉，通稱通靈寶玉；此處材料只支持說明通靈玉與寶玉隨身之玉相關。";
    conn.execute(
        r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, 'quality-source', 'quality-section-basic-tonglingyu',
            ?2, ?3, 'https://example.test/source/basic-tonglingyu',
            1, 1, 'paragraph', NULL, ?4, ?5, 'base_text', 8)
        "#,
        params![
            "quality-block-basic-tonglingyu",
            source_title,
            normalize_title(source_title),
            text,
            normalize_text(text),
        ],
    )
    .expect("insert basic tonglingyu runtime block");
}

fn seed_relation_object_only_runtime_block(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    let source_title = "紅樓夢/第070回";
    let text = "時值暮春之際，史湘雲無聊，因見柳花飄舞，便偶成一小令，調寄《如夢令》。";
    conn.execute(
        r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, 'quality-source', 'quality-section-relation-object-only',
            ?2, ?3, 'https://example.test/source/relation-object-only',
            1, 1, 'paragraph', NULL, ?4, ?5, 'base_text', 70)
        "#,
        params![
            "quality-block-relation-object-only",
            source_title,
            normalize_title(source_title),
            text,
            normalize_text(text),
        ],
    )
    .expect("insert relation object-only runtime block");
}

fn seed_relation_direct_support_runtime_block(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    let source_title = "脂硯齋重評石頭記甲戌本/第三回";
    let text = "原來這襲人亦是賈母之婢，伏侍賈母時，心中眼中只有一個賈母。";
    conn.execute(
        r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, 'quality-source', 'quality-section-relation-direct',
            ?2, ?3, 'https://example.test/source/relation-direct',
            1, 1, 'paragraph', NULL, ?4, ?5, 'commentary', 3)
        "#,
        params![
            "quality-block-relation-direct",
            source_title,
            normalize_title(source_title),
            text,
            normalize_text(text),
        ],
    )
    .expect("insert relation direct runtime block");
}

fn seed_relation_open_object_runtime_blocks(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    for (block_id, source_title, block_index, text, evidence_type) in [
        (
            "quality-block-xiren-generic",
            "紅樓夢/第021回",
            1_i64,
            "賢襲人嬌嗔箴寶玉。",
            "base_text",
        ),
        (
            "quality-block-xiren-jiamu-open",
            "脂硯齋重評石頭記甲戌本/第三回",
            2_i64,
            "原來這襲人亦是賈母之婢，本名珍珠。伏侍賈母時，心中眼中只有一個賈母，今與寶玉，心中眼中又只有一個寶玉。",
            "commentary",
        ),
        (
            "quality-block-xiren-jiamu-short",
            "紅樓夢/第003回",
            3_i64,
            "襲人伏侍賈母。",
            "base_text",
        ),
        (
            "quality-block-xiren-baoyu-short",
            "紅樓夢/第003回",
            4_i64,
            "襲人陪侍寶玉。",
            "base_text",
        ),
        (
            "quality-block-xiren-xiangyun-open",
            "紅樓夢/第019回",
            5_i64,
            "襲人道：「自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。」",
            "base_text",
        ),
        (
            "quality-block-xiren-baoyu-open",
            "脂硯齋重評石頭記甲戌本/第三回",
            6_i64,
            "寶玉之乳母李嬤嬤，並大丫嬛名喚襲人者，陪侍在外大床上。",
            "commentary",
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, 'quality-source', ?2,
                ?3, ?4, 'https://example.test/source/relation-open-object',
                1, ?5, 'paragraph', NULL, ?6, ?7, ?8, 3)
            "#,
            params![
                block_id,
                format!("quality-section-{block_id}"),
                source_title,
                normalize_title(source_title),
                block_index,
                text,
                normalize_text(text),
                evidence_type,
            ],
        )
        .expect("insert relation open-object runtime block");
    }
}

fn seed_entity_focus_runtime_blocks(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    for (block_id, source_title, block_index, text) in [
        (
            "quality-block-generic-title",
            "紅樓夢/第001回",
            1_i64,
            "從此空空道人因空見色，由色生情，改《石頭記》為《情僧錄》，又題曰《紅樓夢》。",
        ),
        (
            "quality-block-zijuan-focus",
            "紅樓夢/第003回",
            2_i64,
            "賈母見雪雁甚小，便將自己身邊的一個二等丫頭，名喚鸚哥者與了黛玉。王嬤嬤與鸚哥陪侍黛玉在碧紗櫥內。",
        ),
    ] {
        conn.execute(
            r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, 'quality-source', 'quality-section-entity-focus',
            ?2, ?3, 'https://example.test/source/entity-focus',
            1, ?4, 'paragraph', NULL, ?5, ?6, 'base_text', 3)
        "#,
            params![
                block_id,
                source_title,
                normalize_title(source_title),
                block_index,
                text,
                normalize_text(text),
            ],
        )
        .expect("insert entity focus runtime block");
    }
}

fn seed_attribute_age_runtime_blocks(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    for (block_id, block_index, text) in [
        (
            "quality-block-lindaiyu-age-noise",
            1_i64,
            "寶玉見林姑娘來了，忙起身讓坐。",
        ),
        (
            "quality-block-lindaiyu-age-direct",
            2_i64,
            "今只有嫡妻賈氏，生得一女，乳名黛玉，年方五歲。夫妻無子，故愛如珍寶。",
        ),
    ] {
        conn.execute(
            r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, 'quality-source', 'quality-section-attribute-age',
            '脂硯齋重評石頭記甲戌本/第二回',
            '脂砚斋重评石头记甲戌本/第二回',
            'https://example.test/source/attribute-age',
            1, ?2, 'paragraph', NULL, ?3, ?4, 'base_text', 2)
        "#,
            params![block_id, block_index, text, normalize_text(text)],
        )
        .expect("insert attribute age runtime block");
    }
}

fn seed_lindaiyu_character_fate_runtime_blocks(conn: &Connection) {
    seed_retrieval_quality_source(
        conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://example.test/license",
            "attribution": "test attribution",
            "usage_boundary": "test usage boundary"
        }),
    );
    for (block_id, block_index, evidence_type, source_title, text) in [
        (
            "quality-block-lindaiyu-fate-noise",
            1_i64,
            "base_text",
            "紅樓夢/第三回",
            "林黛玉自在榮府，一來賈母萬般憐愛，寢食起居，一如寶玉。",
        ),
        (
            "quality-block-lindaiyu-fate-commentary",
            2_i64,
            "commentary",
            "脂硯齋重評石頭記/第五回",
            "可嘆停機德，{{~~|【甲夾：此句薛。】}}\n堪憐咏絮才。{{~~|【甲夾：此句林。】}}\n玉帶林中掛，\n金簪雪裡埋。",
        ),
    ] {
        conn.execute(
            r#"
        INSERT INTO blocks (
            block_id, source_id, section_id, source_title, normalized_source_title,
            source_url, revision_id, block_index, kind, tag, text, normalized_text,
            evidence_type, chapter_no
        ) VALUES (?1, 'quality-source', 'quality-section-character-fate',
            ?4, ?5, 'https://example.test/source/character-fate',
            1, ?2, 'paragraph', NULL, ?6, ?7, ?3, 5)
        "#,
            params![
                block_id,
                block_index,
                evidence_type,
                source_title,
                normalize_text(source_title),
                text,
                normalize_text(text)
            ],
        )
        .expect("insert character fate runtime block");
    }
}

fn yousanjie_test_package() -> EvidencePackage {
    let cards = yousanjie_test_cards();
    EvidencePackage {
        package_id: "pkg-yousanjie-test".to_string(),
        trace_id: "trace-yousanjie-test".to_string(),
        question: "介绍尤三姐".to_string(),
        claims: vec!["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()],
        claim_evidence_map: claim_evidence_map(
            &["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()],
            &cards,
        ),
        knowledge_state_summary: KnowledgeStateSummary::default(),
        cards,
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过：1 条结论声明均有证据包约束。".to_string(),
        },
    }
}

#[test]
fn local_answer_lists_character_intro_evidence_without_synthesizing_profile() {
    let package = yousanjie_test_package();

    let answer = local_answer("介绍尤三姐", &package);

    assert!(answer.contains("根据目前可检索到的文本"));
    assert!(answer.contains("目前能支持回答的主要材料如下"));
    assert!(answer.contains("尤三姐"));
    assert!(answer.contains("柳湘莲") || answer.contains("柳湘蓮"));
    assert!(answer.contains("自尽") || answer.contains("自盡"));
    assert!(answer.contains("尤三姐走來"));
    assert!(answer.contains("若有了姓柳的來"));
    assert!(!answer.contains("出身柳湘莲"));
    assert!(!answer.contains("礼教"));
    assert!(!answer.contains("封建"));
    assert!(!answer.contains("情感决绝"));
    assert!(!answer.contains("人物小传"));
    assert!(!answer.contains("Wikisource"));
    assert!(!answer.contains("source snapshot"));
    assert!(!answer.contains("证据包"));
    assert!(!answer.contains("reviewer"));
    assert!(!answer.contains(&package.package_id));
}

#[test]
fn hermes_draft_rejects_unsupported_interpretive_terms() {
    let cards = yousanjie_test_cards();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "介绍尤三姐",
        "尤三姐出身柳湘莲一支亲戚关系，并反抗封建礼教。",
        &cards,
    );
    let accepted = agent_runtime_draft_evidence_boundary_rejection(
        "介绍尤三姐",
        "尤三姐与柳湘莲婚约、自尽情节相连。",
        &cards,
    );

    assert_eq!(rejected, Some("draft_claim_exceeds_evidence_boundary"));
    assert_eq!(accepted, None);
}

fn seed_retrieval_quality_source(conn: &Connection, snapshot_contract: Value) {
    let license = snapshot_text_field(
        &snapshot_contract,
        &["license", "license_id", "license_note", "licence", "rights"],
    );
    let license_url = snapshot_text_field(
        &snapshot_contract,
        &["license_url", "license_uri", "rights_url"],
    );
    let license_source_url = snapshot_text_field(
        &snapshot_contract,
        &[
            "license_source_url",
            "rights_source_url",
            "copyright_policy_url",
        ],
    );
    let attribution = snapshot_text_field(
        &snapshot_contract,
        &["attribution", "attribution_note", "citation"],
    );
    let usage_boundary = snapshot_text_field(
        &snapshot_contract,
        &["usage_boundary", "usage_limit", "source_usage_boundary"],
    );
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
                "测试底本；仅用于 RQA 单元测试",
                "zh",
                "https://example.test/source",
                "https://example.test/api",
                "2026-05-15T00:00:00Z",
                license,
                license_url,
                license_source_url,
                attribution,
                usage_boundary,
                "测试 source snapshot",
                serde_json::to_string(&snapshot_contract).expect("snapshot serializes"),
                "hash-quality-source",
            ],
        )
        .expect("insert source");
    conn.execute(
            "INSERT INTO version_notes (version_note_id, source_id, note, source_status, usage_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "version-note:quality-source",
                "quality-source",
                "测试 source snapshot",
                "source_snapshot_ready",
                "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            ],
        )
        .expect("insert version note");
    conn.execute(
        r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, source_url, revision_id,
                block_index, kind, tag, text, normalized_text, evidence_type, chapter_no
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        params![
            "quality-block-001",
            "quality-source",
            "quality-section-001",
            "质量测试红楼梦/第一回",
            "https://example.test/source/1",
            1_i64,
            1_i64,
            "paragraph",
            Option::<String>::None,
            "通靈玉上写着莫失莫忘，仙壽恒昌。",
            normalize_text("通靈玉上写着莫失莫忘，仙壽恒昌。"),
            "base_text",
            1_i64,
        ],
    )
    .expect("insert block");
}

#[test]
fn kb_schema_adds_source_usage_metadata_columns_to_existing_sources_table() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(
        r#"
            CREATE TABLE sources (
                source_id TEXT PRIMARY KEY,
                source_category TEXT NOT NULL,
                format TEXT,
                title TEXT,
                work TEXT,
                edition TEXT,
                language TEXT,
                api_url TEXT,
                fetched_at TEXT,
                notes TEXT,
                snapshot_contract_json TEXT NOT NULL,
                source_hash TEXT NOT NULL
            );
            "#,
    )
    .expect("old sources table");

    init_knowledge_base_schema(&conn).expect("kb schema upgrades source metadata");

    let columns = conn
        .prepare("PRAGMA table_info(sources)")
        .expect("table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect columns");
    for column in [
        "source_url",
        "license",
        "license_url",
        "license_source_url",
        "attribution",
        "usage_boundary",
    ] {
        assert!(columns.contains(column), "missing column {column}");
    }
    let block_columns = conn
        .prepare("PRAGMA table_info(blocks)")
        .expect("blocks table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query block columns")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect block columns");
    assert!(block_columns.contains("normalized_source_title"));
    let alias_columns = conn
        .prepare("PRAGMA table_info(aliases)")
        .expect("aliases table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query alias columns")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect alias columns");
    assert!(alias_columns.contains("normalized_alias"));
}

#[test]
fn kb_schema_upgrades_legacy_search_normalization_columns_before_indexing() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(
        r#"
            CREATE TABLE blocks (
                block_id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                chapter_no INTEGER,
                evidence_type TEXT NOT NULL,
                source_title TEXT NOT NULL
            );
            CREATE TABLE aliases (
                alias TEXT PRIMARY KEY,
                person_id TEXT NOT NULL,
                scope TEXT NOT NULL
            );
            INSERT INTO blocks (
                block_id, source_id, chapter_no, evidence_type, source_title
            ) VALUES (
                'legacy-block', 'legacy-source', 1, 'base_text', '紅樓夢/第一回'
            );
            INSERT INTO aliases (alias, person_id, scope)
            VALUES ('寳玉', 'person-baoyu', 'global');
            "#,
    )
    .expect("legacy search tables");

    init_knowledge_base_schema(&conn).expect("kb schema upgrades legacy search columns");

    let block_columns = conn
        .prepare("PRAGMA table_info(blocks)")
        .expect("blocks table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query block columns")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect block columns");
    assert!(block_columns.contains("normalized_source_title"));
    let alias_columns = conn
        .prepare("PRAGMA table_info(aliases)")
        .expect("aliases table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query alias columns")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect alias columns");
    assert!(alias_columns.contains("normalized_alias"));

    let indexes = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'")
        .expect("index query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query indexes")
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .expect("collect indexes");
    assert!(indexes.contains("idx_blocks_normalized_source_title"));
    assert!(indexes.contains("idx_aliases_normalized_alias"));

    let normalized_title: String = conn
        .query_row(
            "SELECT normalized_source_title FROM blocks WHERE block_id = 'legacy-block'",
            [],
            |row| row.get(0),
        )
        .expect("normalized title");
    let normalized_alias: String = conn
        .query_row(
            "SELECT normalized_alias FROM aliases WHERE alias = '寳玉'",
            [],
            |row| row.get(0),
        )
        .expect("normalized alias");
    assert_eq!(normalized_title, "红楼梦/第一回");
    assert_eq!(normalized_alias, "宝玉");
}

#[test]
fn text_normalizer_uses_opencc_plus_project_overrides() {
    let normalized = normalize_text("介紹史湘雲與寶釵，通靈玉上寫有仙壽恒昌。");

    assert_eq!(normalized, "介绍史湘云与宝钗，通灵玉上写有仙寿恒昌。");
    assert_eq!(normalize_text("寳玉與顰兒"), "宝玉与颦儿");
}

#[test]
fn text_search_required_types_respect_explicit_version_boundary_without_default_base() {
    let required = vec!["version_note".to_string()];

    let text_required = text_search_required_evidence_types(&required);

    assert_eq!(text_required, vec!["version_note".to_string()]);
    assert!(!text_required.contains(&"base_text".to_string()));
}

#[test]
fn text_search_returns_production_ready_retrieval_quality_report() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
        params!["quality-person", "通灵玉", "RQA alias test"],
    )
    .expect("insert person");
    conn.execute(
        "INSERT INTO aliases (alias, person_id, scope) VALUES (?1, ?2, ?3)",
        params!["灵玉", "quality-person", "test"],
    )
    .expect("insert alias");
    let question = "灵玉 password=SECRET_RUNTIME_TOKEN_01234567890123456789";

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: question.to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert_eq!(cards.len(), 1);
    assert_eq!(
        quality_report.schema_version,
        RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION
    );
    assert_eq!(quality_report.tool_name, "tonglingyu.text.search");
    assert_eq!(quality_report.candidate_count, 1);
    assert_eq!(quality_report.selected_count, 1);
    assert_eq!(quality_report.quality_status, "passed");
    assert!(quality_report.production_ready);
    assert!(!quality_report.truncated);
    assert_eq!(
        quality_report.channel_distribution.get("base_text"),
        Some(&1_usize)
    );
    assert!(
        quality_report
            .expanded_aliases
            .iter()
            .any(|alias| alias == "灵玉")
    );
    assert_eq!(quality_report.expected_evidence_hit, None);
    assert_eq!(
        quality_report.expected_evidence_status,
        "not_applicable_runtime_search"
    );
    assert_eq!(
        quality_report.evidence_type_coverage.selected,
        vec!["base_text".to_string()]
    );
    assert!(quality_report.evidence_type_coverage.missing.is_empty());
    assert!(!quality_report.query_summary.raw_question_included);
    assert_eq!(
        quality_report.source_usage_refs[0].metadata_status,
        "complete"
    );
    assert_eq!(
        quality_report.source_usage_refs[0].license.as_deref(),
        Some("CC-BY-SA-4.0")
    );
    assert_eq!(
        quality_report.source_usage_refs[0].license_url.as_deref(),
        Some("https://creativecommons.org/licenses/by-sa/4.0/")
    );
    let report_json = serde_json::to_string(&quality_report).expect("report serializes");
    assert!(!report_json.contains(question));
    assert!(!report_json.contains("SECRET_RUNTIME_TOKEN"));
}

#[test]
fn text_search_strips_intro_shell_for_character_lookup() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, source_url,
                revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, 'quality-source', 'quality-section-002',
                '质量测试红楼梦/第六十六回', 'https://example.test/source/66',
                1, 2, 'paragraph', NULL, ?2, ?3, 'base_text', 66)
            "#,
        params![
            "quality-block-yousanjie",
            "尤三姐走来，说自己不是那心口两样的人。",
            normalize_text("尤三姐走来，说自己不是那心口两样的人。"),
        ],
    )
    .expect("insert character block");

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "介绍尤三姐".to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-yousanjie"),
        "intro-shell query should retrieve the character block"
    );
    assert!(
        quality_report
            .expanded_terms
            .iter()
            .any(|term| term == "尤三姐")
    );
    assert!(quality_report.production_ready);
}

#[test]
fn text_search_matches_simplified_query_to_traditional_alias_and_raw_evidence() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
        params!["person:xiangyun-test", "史湘云", "test"],
    )
    .expect("insert person");
    for alias in ["史湘云", "史湘雲", "湘云", "湘雲"] {
        conn.execute(
                "INSERT INTO aliases (alias, normalized_alias, person_id, scope) VALUES (?1, ?2, ?3, ?4)",
                params![alias, normalize_alias(alias), "person:xiangyun-test", "test"],
            )
            .expect("insert alias");
    }
    conn.execute(
        r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, 'quality-source', 'quality-section-xiangyun',
                '紅樓夢/第三十一回', ?2, 'https://example.test/source/31',
                1, 2, 'paragraph', NULL, ?3, ?4, 'base_text', 31)
            "#,
        params![
            "quality-block-xiangyun-trad",
            normalize_title("紅樓夢/第三十一回"),
            "賈母回頭囑咐湘雲：「別讓你寶哥哥多吃了。」湘雲答應著。",
            normalize_text("賈母回頭囑咐湘雲：「別讓你寶哥哥多吃了。」湘雲答應著。"),
        ],
    )
    .expect("insert xiangyun block");

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "介绍史湘云".to_string(),
            limit: 4,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    let card = cards
        .iter()
        .find(|card| card.block_id == "quality-block-xiangyun-trad")
        .expect("simplified query should retrieve traditional raw evidence");
    assert!(card.text.contains("湘雲"));
    assert!(!card.text.contains("湘云答应"));
    assert!(
        quality_report
            .normalized_match_channels
            .get("normalized_text")
            .is_some_and(|count| *count >= 1)
    );
    assert!(
        quality_report
            .expanded_aliases
            .iter()
            .any(|alias| alias == "湘雲")
    );
}

#[test]
fn commentary_search_for_character_fate_prefers_fate_markers_over_mentions() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        r#"
        INSERT INTO sources (
            source_id, source_category, format, title, work, edition, language,
            source_url, api_url, fetched_at, license, license_url,
            license_source_url, attribution, usage_boundary, notes,
            snapshot_contract_json, source_hash
        ) VALUES (
            'quality-source-zhiyanzhai-fate', 'commentary_material', 'mediawiki',
            '质量测试脂批 source', '红楼梦', '测试脂批', 'zh',
            'https://example.test/source/zhiyanzhai-fate',
            'https://example.test/api/zhiyanzhai-fate',
            '2026-05-15T00:00:00Z', 'CC-BY-SA-4.0',
            'https://creativecommons.org/licenses/by-sa/4.0/',
            'https://wikisource.org/wiki/Wikisource:Copyright_policy',
            'Wikisource contributors',
            '可作为默认回答证据；与前八十回正文同属 in-scope，证据来源层记录为脂批。',
            '测试 commentary source snapshot', '{}', 'hash-quality-source-zhiyanzhai-fate'
        )
        "#,
        [],
    )
    .expect("insert commentary source");
    for (block_id, text, block_index) in [
        (
            "quality-block-xiangyun-ordinary-commentary",
            "史湘雲問道：「寶玉哥哥不在家麽？」寶釵笑道：「他再不想著別人，只想寶兄弟。」",
            1_i64,
        ),
        (
            "quality-block-xiangyun-fate-commentary",
            "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。{{~~|【甲眉：悲壯之極，北曲中不能多得。】}}",
            2_i64,
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, 'quality-source-zhiyanzhai-fate', 'quality-section-fate',
                '脂硯齋重評石頭記/第五回', ?2, 'https://example.test/source/fate',
                1, ?3, 'paragraph', NULL, ?4, ?5, 'commentary', 5)
            "#,
            params![
                block_id,
                normalize_title("脂硯齋重評石頭記/第五回"),
                block_index,
                text,
                normalize_text(text),
            ],
        )
        .expect("insert commentary block");
    }

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::CommentarySearch {
            question: "关于史湘云的结局，脂批中的证据呢".to_string(),
            limit: 4,
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards { cards, .. } = output else {
        panic!("expected evidence cards");
    };
    assert_eq!(cards[0].block_id, "quality-block-xiangyun-fate-commentary");
    assert!(cards[0].text.contains("樂中悲"));
}

#[test]
fn text_search_for_later_forty_character_fate_uses_generic_fate_cues() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
        params!["person:xiangyun-later-fate", "史湘云", "test"],
    )
    .expect("insert person");
    for alias in ["史湘云", "史湘雲", "湘云", "湘雲", "史姑娘"] {
        conn.execute(
            "INSERT INTO aliases (alias, normalized_alias, person_id, scope) VALUES (?1, ?2, ?3, ?4)",
            params![alias, normalize_alias(alias), "person:xiangyun-later-fate", "test"],
        )
        .expect("insert alias");
    }
    for (block_id, text, block_index) in [
        (
            "quality-block-xiangyun-later-ordinary",
            "史湘雲仍在賈母房中，迎春便往惜春那裡去了。",
            1_i64,
        ),
        (
            "quality-block-xiangyun-later-fate",
            "老太太想史姑娘，叫我們去打聽。那裡知道史姑娘哭得了不得，說是姑爺得了暴病，大夫都瞧了，說這病只怕不能好，若變了個癆病，還可捱過四五年。",
            2_i64,
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, 'quality-source', 'quality-section-xiangyun-later',
                '紅樓夢/第110回', ?2, 'https://example.test/source/110',
                1, ?3, 'paragraph', NULL, ?4, ?5, 'base_text', 110)
            "#,
            params![
                block_id,
                normalize_title("紅樓夢/第110回"),
                block_index,
                text,
                normalize_text(text),
            ],
        )
        .expect("insert later-forty block");
    }

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "关于史湘云的结局，后四十回呢？".to_string(),
            limit: 4,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert_eq!(
        cards.first().map(|card| card.block_id.as_str()),
        Some("quality-block-xiangyun-later-fate"),
        "fate evidence should outrank mention-only later-forty cards"
    );
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-xiangyun-later-fate"),
        "later-forty fate query should retrieve spouse-illness fate evidence"
    );
    let catalog = query_expansion_catalog().expect("query expansion catalog");
    let normalized = normalize_query("关于史湘云的结局，后四十回呢？");
    let mut expanded = Vec::new();
    apply_query_expansion_terms(
        &catalog,
        "关于史湘云的结局，后四十回呢？",
        &normalized,
        &mut expanded,
    );
    assert!(expanded.iter().any(|term| term == "姑爷" || term == "姑爺"));
    assert!(
        quality_report
            .expanded_aliases
            .iter()
            .any(|alias| alias == "史姑娘")
    );
}

#[test]
fn runtime_required_evidence_types_adds_commentary_for_default_fate_questions() {
    assert_eq!(
        runtime_required_evidence_types("史湘云的结局", &[], None),
        vec!["commentary".to_string()]
    );
    assert_eq!(
        runtime_required_evidence_types("史湘云的结局，后四十回呢？", &[], None),
        Vec::<String>::new()
    );
}

#[test]
fn entity_fate_scope_filter_drops_unbound_fate_cards() {
    let frame: question_frame::RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "史湘云的结局",
        "subject": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "湘云", "湘雲", "史姑娘"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("question frame");
    let mut unbound = sample_card("base_text");
    unbound.evidence_id = "unbound-fate".to_string();
    unbound.source_title = "紅樓夢（程甲本）/五".to_string();
    unbound.text =
        "卻說寶玉聽了此曲，散漫無稽，未見得好處。又看下面冊子，寫著紅樓夢曲。".to_string();
    let mut bound_commentary = sample_card("commentary");
    bound_commentary.evidence_id = "bound-commentary".to_string();
    bound_commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    bound_commentary.text =
        "富貴又何為？襁褓之間父母違；展眼弔斜暉，湘江水逝楚雲飛。此為湘雲判詞。".to_string();
    let mut bound_later_forty = sample_card("base_text");
    bound_later_forty.evidence_id = "bound-later-forty".to_string();
    bound_later_forty.source_title = "紅樓夢/第110回".to_string();
    bound_later_forty.text =
        "且說史湘雲因他女婿病著，又見他女婿的病已成癆症，想到自己命苦。".to_string();

    let filtered = filter_cards_for_question_frame_answer_scope(
        "史湘云的结局",
        Some(&frame),
        vec![
            unbound.clone(),
            bound_commentary.clone(),
            bound_later_forty.clone(),
        ],
    );

    assert_eq!(filtered.len(), 2);
    assert!(
        filtered
            .iter()
            .any(|card| card.evidence_id == "bound-commentary")
    );
    assert!(
        filtered
            .iter()
            .any(|card| card.evidence_id == "bound-later-forty")
    );
    assert!(
        !filtered
            .iter()
            .any(|card| card.evidence_id == "unbound-fate")
    );
}

#[test]
fn entity_fate_draft_rejects_generic_appearance_answer_without_bound_fate_cue() {
    let frame: question_frame::RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "史湘云的结局，后四十回呢？",
        "subject": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "湘云", "湘雲", "史姑娘"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("question frame");
    let mut card = sample_card("base_text");
    card.source_title = "紅樓夢/第110回".to_string();
    card.text = "且說史湘雲因他女婿病著，賈母死後只來的一次。又見他女婿的病已成癆症。".to_string();

    assert_eq!(
        entity_fate_draft_rejection_reason(
            "史湘云的结局，后四十回呢？",
            Some(&frame),
            &[card.clone()],
            "就後四十回所見，史湘雲仍有出場，但不能推出完整結局。"
        ),
        Some("entity_fate_draft_missing_bound_fate_cue")
    );
    assert_eq!(
        entity_fate_draft_rejection_reason(
            "史湘云的结局，后四十回呢？",
            Some(&frame),
            &[card],
            "就後四十回所見，史湘雲女婿病著，且病已成癆症，這只能說明該版本中的後續遭際。"
        ),
        None
    );
}

#[test]
fn text_search_expands_tonglingyu_loss_question_to_lost_jade_event_terms() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        r#"
        INSERT INTO sources (
            source_id, source_category, format, title, work, edition, language,
            source_url, api_url, fetched_at, license, license_url,
            license_source_url, attribution, usage_boundary, notes,
            snapshot_contract_json, source_hash
        ) VALUES (
            'quality-source-zhiyanzhai', 'commentary_material', 'mediawiki',
            '质量测试脂批 source', '红楼梦', '测试脂批', 'zh',
            'https://example.test/source/zhiyanzhai',
            'https://example.test/api/zhiyanzhai',
            '2026-05-15T00:00:00Z', 'CC-BY-SA-4.0',
            'https://creativecommons.org/licenses/by-sa/4.0/',
            'https://wikisource.org/wiki/Wikisource:Copyright_policy',
            'Wikisource contributors',
            '可作为默认回答证据；与前八十回正文同属 in-scope，证据来源层记录为脂批。',
            '测试 commentary source snapshot', '{}', 'hash-quality-source-zhiyanzhai'
        )
        "#,
        [],
    )
    .expect("insert commentary quality source");
    let long_fengjie_commentary = format!(
        "{}剛至穿堂門前，{{{{~|【庚辰雙行夾批：妙！這便是鳳姐掃雪拾玉之處，一絲不亂。】}}}}只見襲人倚門立在那裡。",
        "前置脂批材料。".repeat(80)
    );
    for (block_id, source_id, source_title, chapter_no, block_index, kind, evidence_type, text) in [
        (
            "quality-block-lost-jade-heading",
            "quality-source",
            "紅樓夢/第九十四回",
            94_i64,
            1_i64,
            "heading",
            "base_text",
            "第九十四回 宴海棠賈母賞花妖 失寶玉通靈知奇禍".to_string(),
        ),
        (
            "quality-block-lost-jade-body",
            "quality-source",
            "紅樓夢/第九十四回",
            94_i64,
            2_i64,
            "paragraph",
            "base_text",
            "襲人見寶玉脖子上沒有挂著，便問：“那塊玉呢？”寶玉道：“才剛忙亂換衣，摘下來放在炕桌上，我沒有帶。”襲人回看桌上并沒有玉，便向各處找尋，蹤影全無。眾人又道：“你二哥哥的玉丟了，你瞧見了沒有？”".to_string(),
        ),
        (
            "quality-block-lianger-stole-jade",
            "quality-source",
            "紅樓夢/第五十二回",
            52_i64,
            3_i64,
            "paragraph",
            "base_text",
            "平兒道：“我赶忙接了鐲子，想了一想：寳玉是偏在你們身上留心用意、争勝要强的，那一年有一個良兒偷玉，剛冷了這二年，閒時還常有人提起來趂愿。”".to_string(),
        ),
        (
            "quality-block-monk-delivers-jade",
            "quality-source",
            "紅樓夢/第一百十五回",
            115_i64,
            4_i64,
            "paragraph",
            "base_text",
            "只見那和尚道：“施主們，我是送玉来的。”說着，把那塊玉擎着道：“快把銀子拿出來，我好救他。”和尚哈哈大笑，手拿着玉在寳玉耳邊呌道：“寶玉，寳玉，你的寳玉囬來了。”".to_string(),
        ),
        (
            "quality-block-zhen-baoyu-delivers-jade-commentary",
            "quality-source-zhiyanzhai",
            "脂硯齋重評石頭記/第十八回",
            18_i64,
            8_i64,
            "paragraph",
            "commentary",
            "第三齣《仙緣》；{{~|【庚辰雙行夾批：《邯鄲夢》中伏甄寶玉送玉。】}}".to_string(),
        ),
        (
            "quality-block-snow-pickup-cover-story",
            "quality-source",
            "紅樓夢/第五十二回",
            52_i64,
            5_i64,
            "paragraph",
            "base_text",
            "平兒道：“我徃大奶奶那裡去來着，誰知鐲子褪了口，丢在草根底下，雪深了，没看見。今兒雪化盡了，黃澄澄的映着日頭，還在那裡呢。我就揀了起來。”".to_string(),
        ),
        (
            "quality-block-fengjie-snow-pickup-commentary",
            "quality-source-zhiyanzhai",
            "脂硯齋重評石頭記/第二十三回",
            23_i64,
            6_i64,
            "paragraph",
            "commentary",
            long_fengjie_commentary,
        ),
        (
            "quality-block-jade-inscription-distractor",
            "quality-source",
            "紅樓夢/第八回",
            8_i64,
            7_i64,
            "paragraph",
            "base_text",
            "通靈寶玉正面鐫著“莫失莫忘，仙壽恒昌”，反面又有“一除邪祟，二療冤疾，三知禍福”。".to_string(),
        ),
    ] {
        let normalized_text = normalize_text(&text);
        conn.execute(
            r#"
                INSERT INTO blocks (
                    block_id, source_id, section_id, source_title, normalized_source_title,
                    source_url, revision_id, block_index, kind, tag, text, normalized_text,
                    evidence_type, chapter_no
                ) VALUES (?1, ?2, 'quality-section-lost-jade',
                    ?3, ?4, 'https://example.test/source/lost-jade',
                    1, ?5, ?6, NULL, ?8, ?9, ?7, ?10)
                "#,
            params![
                block_id,
                source_id,
                source_title,
                normalize_title(source_title),
                block_index,
                kind,
                evidence_type,
                &text,
                normalized_text,
                chapter_no,
            ],
        )
        .expect("insert lost jade block");
    }
    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "通灵宝玉丢了几次".to_string(),
            limit: 6,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-lost-jade-body"),
        "loss-count query should retrieve the 第九十四回 lost-jade body evidence"
    );
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-lianger-stole-jade"),
        "loss-count query should retrieve 良儿偷玉 recall evidence"
    );
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-monk-delivers-jade"),
        "loss-count query should retrieve 送玉来 recall evidence"
    );
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-zhen-baoyu-delivers-jade-commentary"),
        "loss-count query should retrieve 甄宝玉送玉 commentary evidence"
    );
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-snow-pickup-cover-story"),
        "loss-count query should retrieve snow-pickup recall evidence"
    );
    let fengjie_snow_pickup_card = cards
        .iter()
        .find(|card| card.block_id == "quality-block-fengjie-snow-pickup-commentary")
        .expect("loss-count query should retrieve Fengjie snow-pickup commentary evidence");
    assert_eq!(fengjie_snow_pickup_card.evidence_type, "commentary");
    assert!(fengjie_snow_pickup_card.text.contains("鳳姐掃雪拾玉"));
    let later_forty_card = cards
        .iter()
        .find(|card| card.block_id == "quality-block-lost-jade-body")
        .expect("later forty lost-jade evidence selected");
    assert!(later_forty_card.support_scope.contains("后四十回"));
    assert!(
        later_forty_card
            .unsupported_scope
            .contains("未标注时不能作为证据或参考")
    );
    assert!(
        quality_report
            .expanded_terms
            .iter()
            .any(|term| term == "失寶玉")
    );
    assert!(
        quality_report
            .expanded_terms
            .iter()
            .any(|term| term == "玉丢了")
    );
    assert!(
        quality_report
            .expanded_terms
            .iter()
            .any(|term| term == "良兒偷玉")
    );
    assert!(
        quality_report
            .expanded_terms
            .iter()
            .any(|term| term == "甄宝玉送玉")
    );
    assert!(
        quality_report
            .expanded_terms
            .iter()
            .any(|term| term == "掃雪拾玉")
    );
    assert!(
        quality_report
            .protected_terms
            .iter()
            .any(|term| term == "甄寶玉送玉")
    );
    assert!(
        quality_report
            .protected_terms
            .iter()
            .any(|term| term == "鳳姐掃雪拾玉")
    );
}

#[test]
fn text_search_prefers_full_normalized_character_match_over_short_alias_only() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
        params!["person:xiangyun-default-alias-test", "史湘云", "test"],
    )
    .expect("insert person");
    for alias in ["湘云", "湘雲"] {
        conn.execute(
                "INSERT INTO aliases (alias, normalized_alias, person_id, scope) VALUES (?1, ?2, ?3, ?4)",
                params![alias, normalize_alias(alias), "person:xiangyun-default-alias-test", "test"],
            )
            .expect("insert alias");
    }
    for (block_id, block_index, text) in [
        (
            "quality-block-xiangyun-full-name",
            1_i64,
            "且說史湘雲住了兩日，因要回去。賈母因說：“等過了你寶姐姐的生日，看了戲再回去。”",
        ),
        (
            "quality-block-xiangyun-short-alias-only",
            2_i64,
            "湘雲笑道：“你快下去，你不中用。”",
        ),
    ] {
        conn.execute(
            r#"
                INSERT INTO blocks (
                    block_id, source_id, section_id, source_title, normalized_source_title,
                    source_url, revision_id, block_index, kind, tag, text, normalized_text,
                    evidence_type, chapter_no
                ) VALUES (?1, 'quality-source', 'quality-section-xiangyun-rank',
                    '紅樓夢/第二十二回', ?2, 'https://example.test/source/22',
                    1, ?3, 'paragraph', NULL, ?4, ?5, 'base_text', 22)
                "#,
            params![
                block_id,
                normalize_title("紅樓夢/第二十二回"),
                block_index,
                text,
                normalize_text(text),
            ],
        )
        .expect("insert xiangyun ranking block");
    }

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "介绍史湘云".to_string(),
            limit: 1,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards { cards, .. } = output else {
        panic!("expected evidence cards");
    };
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].block_id, "quality-block-xiangyun-full-name");
    assert!(cards[0].text.contains("史湘雲"));
}

#[test]
fn text_search_prioritizes_body_match_over_source_title_only_match() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    for (block_id, source_title, block_index, text) in [
        (
            "quality-block-title-only",
            "红楼梦/第一回",
            1_i64,
            "短句不含题名。",
        ),
        (
            "quality-block-body-title",
            "测试来源/题名线索",
            2_i64,
            "此处正文直接写出题名红楼梦，可作为题名出现位置的证据。",
        ),
    ] {
        conn.execute(
            r#"
                INSERT INTO blocks (
                    block_id, source_id, section_id, source_title, normalized_source_title,
                    source_url, revision_id, block_index, kind, tag, text, normalized_text,
                    evidence_type, chapter_no
                ) VALUES (?1, 'quality-source', 'quality-section-title-rank',
                    ?2, ?3, 'https://example.test/source/title',
                    1, ?4, 'paragraph', NULL, ?5, ?6, 'base_text', 1)
                "#,
            params![
                block_id,
                source_title,
                normalize_title(source_title),
                block_index,
                text,
                normalize_text(text),
            ],
        )
        .expect("insert title ranking block");
    }

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "红楼梦题名在哪里出现？".to_string(),
            limit: 1,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards { cards, .. } = output else {
        panic!("expected evidence cards");
    };
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].block_id, "quality-block-body-title");
    assert!(cards[0].text.contains("红楼梦"));
}

#[test]
fn text_search_matches_traditional_query_to_simplified_alias_and_text() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    conn.execute(
        r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, 'quality-source', 'quality-section-baochai',
                '红楼梦/第八回', ?2, 'https://example.test/source/8',
                1, 3, 'paragraph', NULL, ?3, ?4, 'base_text', 8)
            "#,
        params![
            "quality-block-baochai-simplified",
            normalize_title("红楼梦/第八回"),
            "宝钗笑道：宝兄弟从那里来？",
            normalize_text("宝钗笑道：宝兄弟从那里来？"),
        ],
    )
    .expect("insert baochai block");

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "寶釵是谁".to_string(),
            limit: 4,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert!(
        cards
            .iter()
            .any(|card| card.block_id == "quality-block-baochai-simplified")
    );
    assert!(
        quality_report
            .normalized_match_channels
            .get("normalized_text")
            .is_some_and(|count| *count >= 1)
    );
}

#[test]
fn local_answer_keeps_raw_quotes_without_intro_synthesis() {
    let mut card = sample_card("base_text");
    card.source_title = "紅樓夢/第三十一回".to_string();
    card.block_id = "quality-block-xiangyun-answer".to_string();
    card.text = "賈母回頭囑咐湘雲：「別讓你寶哥哥多吃了。」湘雲答應著。".to_string();
    let package = EvidencePackage {
        package_id: "pkg-answer-normalization-test".to_string(),
        trace_id: "trace-answer-normalization-test".to_string(),
        question: "介紹史湘雲".to_string(),
        cards: vec![card],
        claims: vec!["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("介紹史湘雲", &package);

    assert!(answer.contains("根据目前可检索到的文本"));
    assert!(answer.contains("目前能支持回答的主要材料如下"));
    assert!(answer.contains("湘雲"));
    assert!(!answer.starts_with("史湘云是当前"));
    assert!(!answer.contains("source snapshot"));
    assert!(!answer.contains("證據"));
    assert!(!answer.contains(&package.package_id));
}

#[test]
fn local_answer_prefers_requested_commentary_and_cleans_markup() {
    let mut base = sample_card("base_text");
    base.source_title = "紅樓夢/第005回".to_string();
    base.text = "（樂中悲）襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "<center>'''第六支，樂中悲：'''</center> 襁褓中，父母嘆雙亡。{{~~|【甲側：意真辭切。】}}終久是雲散高唐，水涸湘江。".to_string();

    let mut weak_commentary = sample_card("commentary");
    weak_commentary.source_id = "shitouji-wikisource-jiaxu".to_string();
    weak_commentary.source_title = "脂硯齋重評石頭記甲戌本/第五回".to_string();
    weak_commentary.text =
        "寶玉看了，便知{{~|[「便知」二字是字法。]}}只見那邊厨上封條上大書七字云：「金陵十二釵正冊」。"
            .to_string();

    let package = EvidencePackage {
        package_id: "pkg-commentary-answer-test".to_string(),
        trace_id: "trace-commentary-answer-test".to_string(),
        question: "关于史湘云的结局，脂批中的证据呢".to_string(),
        cards: vec![base, weak_commentary, commentary],
        claims: vec!["命中的脂批材料可作为默认回答证据。".to_string()],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("关于史湘云的结局，脂批中的证据呢", &package);

    assert!(answer.starts_with("有。脂批里最直接可用的是"));
    assert!(answer.contains("脂硯齋重評石頭記/第五回"));
    assert!(!answer.contains("紅樓夢/第005回"));
    assert!(!answer.contains("金陵十二釵正冊"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
    assert!(answer.contains("第六支，樂中悲"));
    assert!(answer.contains("甲側：意真辭切"));
    assert!(!answer.contains("{{"));
    assert!(!answer.contains("<center>"));
}

#[test]
fn local_answer_does_not_count_tonglingyu_lost_jade_with_fixed_oracle() {
    let package = EvidencePackage {
        package_id: "pkg-lost-jade-answer-test".to_string(),
        trace_id: "trace-lost-jade-answer-test".to_string(),
        question: "通灵宝玉丢了几次".to_string(),
        cards: lost_jade_test_cards(),
        claims: vec![
            "涉及事件归纳或次数统计的问题必须按当前证据包命中的证据槽位说明范围。".to_string(),
        ],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("通灵宝玉丢了几次", &package);

    assert!(answer.contains("目前能支持回答的主要材料如下"));
    assert!(answer.contains("以下包含第八十一回及以后（后四十回）材料"));
    assert!(answer.contains("第094回（后四十回）"));
    assert!(answer.contains("第094回"));
    assert!(answer.contains("那塊玉真丟了么"));
    assert!(!answer.contains("当前证据能稳妥确认的是一次"));
    assert!(!answer.contains("送回这一次失玉"));
    assert!(!answer.contains("至少两次"));
}

#[test]
fn local_answer_uses_slot_semantics_for_lost_jade_count() {
    let package = EvidencePackage {
        package_id: "pkg-lost-jade-slot-answer-test".to_string(),
        trace_id: "trace-lost-jade-slot-answer-test".to_string(),
        question: "通灵宝玉丢了几次".to_string(),
        cards: in_scope_lost_jade_event_cards(),
        claims: vec![
            "涉及事件归纳或次数统计的问题必须按 evidence slot rules 的 role/counts_as 解释。"
                .to_string(),
        ],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("通灵宝玉丢了几次", &package);

    assert!(answer.contains("直接支持两处"));
    assert!(answer.contains("良儿偷玉"));
    assert!(answer.contains("甄宝玉送玉"));
    assert!(answer.contains("凤姐扫雪拾玉"));
    assert!(answer.contains("按“拾玉/失而复得”计入明确失玉"));
    assert!(answer.contains("不能直接计入次数"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
    assert!(!answer.contains("直接支持三处"));
}

#[test]
fn local_answer_skips_broken_shell_evidence_cards() {
    let mut broken = sample_card("base_text");
    broken.source_title = "紅樓夢/第050回".to_string();
    broken.block_id = "block-broken-speech-lead".to_string();
    broken.text = "寶玉道：".to_string();
    let mut usable = sample_card("base_text");
    usable.source_title = "紅樓夢/第052回".to_string();
    usable.block_id = "block-lianger-theft".to_string();
    usable.text = "只聽麝月說道：“那一年有一個良兒偷玉，剛冷了一二年，間或有人提起來，還有人無事生非。”平兒道：“二奶奶就不許吵嚷，只叫小心查訪。”"
        .to_string();
    let package = EvidencePackage {
        package_id: "pkg-broken-card-answer-test".to_string(),
        trace_id: "trace-broken-card-answer-test".to_string(),
        question: "通灵宝玉丢失".to_string(),
        cards: vec![broken, usable],
        claims: vec!["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("通灵宝玉丢失", &package);

    assert!(!answer.contains("紅樓夢/第050回：寶玉道："));
    assert!(answer.contains("1. 紅樓夢/第052回"));
    assert!(answer.contains("良兒偷玉"));
}

#[test]
fn local_answer_deduplicates_repeated_base_text_evidence() {
    let mut chengjia = sample_card("base_text");
    chengjia.source_title = "紅樓夢（程甲本）/五十二".to_string();
    chengjia.block_id = "block-lianger-theft-chengjia".to_string();
    chengjia.text = "只聽麝月說道：“那一年有一個良兒偷玉，剛冷了一二年，間或有人提起來，還有人無事生非。”平兒道：“二奶奶就不許吵嚷，只叫小心查訪。”"
        .to_string();
    let mut wikisource = sample_card("base_text");
    wikisource.source_title = "紅樓夢/第052回".to_string();
    wikisource.block_id = "block-lianger-theft-wikisource".to_string();
    wikisource.text = "只听麝月说道：“那一年有一个良儿偷玉，刚冷了一二年，间或有人提起来，还会有人无事生非。”平儿道：“二奶奶就不许吵嚷，只叫小心查访。”"
        .to_string();
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五十二回".to_string();
    commentary.block_id = "block-lianger-theft-commentary".to_string();
    commentary.text = "只聽麝月說道：“那一年有一個良兒偷玉，剛冷了一二年，間或有人提起來，還有人無事生非。”【庚辰雙行夾批：二次小竊皆出於寶玉房中。】"
        .to_string();
    let package = EvidencePackage {
        package_id: "pkg-duplicate-card-answer-test".to_string(),
        trace_id: "trace-duplicate-card-answer-test".to_string(),
        question: "通灵宝玉丢失".to_string(),
        cards: vec![chengjia, wikisource, commentary],
        claims: vec!["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("通灵宝玉丢失", &package);

    assert!(answer.contains("1. 紅樓夢（程甲本）/五十二"));
    assert!(!answer.contains("2. 紅樓夢/第052回"));
    assert!(answer.contains("2. 脂硯齋重評石頭記/第五十二回"));
    assert!(answer.contains("二次小竊皆出於寶玉房中"));
}

#[test]
fn local_answer_deduplicates_long_edition_variants_in_evidence_brief() {
    let mut chengjia = sample_card("base_text");
    chengjia.source_title = "紅樓夢（程甲本）/五十二".to_string();
    chengjia.block_id = "block-lianger-theft-chengjia-long".to_string();
    chengjia.text = "月悄問道：「你怎麽就得了的？」平兒道：「那日彼時洗手時不見了，二奶奶就不許吵嚷。出了園子，卽刻就傳給園裡各處的媽媽們，小心訪查。我們只疑惑邢姑娘的丫頭，本來又窮，只怕小孩子家没見過，拿了起来是有的，再不料定是你們這裡的。幸而二奶奶没有在屋裡，你們這裡的宋媽去了，拿着這支鐲子，說是小丫頭墜兒偷起來的，被他看見，來囬二奶奶的。我赶忙接了鐲子，想了一想：寳玉是偏在你們身上留心用意、争勝要强的，那一年有一個良兒偷玉，剛冷了這二年，閒時還常有人提起來趂愿。這㑹子又跑出一個偷金子的来了，而且更偷到街坊家去了。偏是他這様，偏是他的人打嘴。所以我倒忙叮嚀宋媽千萬别告訴寶玉，只當没有這事，總别和一個人提起。第二件，老太太、太太𦗟了生氣。三則襲人和你們也不好看。所以我囬二奶奶，只說：『我徃大奶奶那裡去來着，誰知鐲子褪了口，丢在草根底下，雪深了，没看見。今兒雪化盡了，黃澄澄的映着日頭，還在那裡呢。我就揀了起來。』二奶奶也就信了，所以我來告訴你們。你們以後防着他些，别使喚他到别處去。等襲人囘來，你們商議着，變個法子打發出去就完了。」"
        .to_string();
    let mut wikisource = sample_card("base_text");
    wikisource.source_title = "紅樓夢/第052回".to_string();
    wikisource.block_id = "block-lianger-theft-wikisource-long".to_string();
    wikisource.text = "只聞麝月悄問道：“你怎麼就得了的？”平兒道：“那日洗手時不見了，二奶奶就不許吵嚷，出了園子，即刻就傳給園里各處的媽媽們小心查訪。我們只疑惑邢姑娘的丫頭，本來又窮，只怕小孩子家沒見過，拿了起來也是有的。再不料定是你們這里的。幸而二奶奶沒有在屋里，你們這里的宋媽媽去了，拿著這支鐲子，說是小丫頭子墜兒偷起來的，被他看見，來回二奶奶的。我赶著忙接了鐲子，想了一想：寶玉是偏在你們身上留心用意，爭胜要強的，那一年有一個良兒偷玉，剛冷了一二年間，還有人提起來趁願，這會子又跑出一個偷金子的來了。而且更偷到街坊家去了。偏是他這樣，偏是他的人打嘴。所以我倒忙叮嚀宋媽，千萬別告訴寶玉，只當沒有這事，別和一個人提起。第二件，老太太，太太聽了也生氣。三則襲人和你們也不好看。所以我回二奶奶，只說：‘我往大奶奶那里去的，誰知鐲子褪了口，丟在草根底下，雪深了沒看見。今兒雪化盡了，黃澄澄的映著日頭，還在那里呢，我就揀了起來。’二奶奶也就信了，所以我來告訴你們。你們以後防著他些，別使喚他到別處去。等襲人回來，你們商議著，變個法子打發出去就完了。”"
        .to_string();
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五十二回".to_string();
    commentary.block_id = "block-lianger-theft-commentary-long".to_string();
    commentary.text = "幸而二奶奶沒有在屋裡，你們這裡的宋媽媽去了，拿著這支鐲子，說是小丫頭子墜兒偷起來的，被他看見，來回二奶奶的。{{~|【庚辰雙行夾批：二次小竊皆出於寶玉房中，亦大有深意在焉。】}}我趕著忙接了鐲子，想了一想：寶玉是偏在你們身上留心用意、爭勝要強的，那一年有一個良兒偷玉，剛冷了一二年間，還有人提起來趁願。"
        .to_string();
    let mut chengyi = sample_card("base_text");
    chengyi.source_title = "紅樓夢（程乙本）/第五十一回 至第六十回".to_string();
    chengyi.block_id = "block-lianger-theft-chengyi-long".to_string();
    chengyi.text = "說著，果然從後門出去至窗下潛聽。麝月悄悄問道：「你怎麼就得了的？」平兒道：「那日彼時洗手時不見了，二奶奶就不許吵嚷，出了園子，即刻就傳給園裡各處的媽媽們，小心訪查。我們只疑惑邢姑娘的丫頭，本來又窮，只怕小孩子家沒見過，拿起來是有的，再不料定是你們這裡的。幸而二奶奶沒有在屋裡，你們這裡的宋媽去了，拿著這支鐲子，說是小丫頭墜兒偷起來的，被他看見，來回二奶奶的。我趕忙接了鐲子，想了一想。寶玉是偏在你們身上留心用意，爭勝要強的。那一年有個良兒偷玉，剛冷了這二年，閒時還常有人提起來趁願；這會子又跑出一個偷金子的來了，而且更偷到街坊家去了。偏是他這麼著，偏是他的人打嘴。所以我倒忙叮嚀宋媽，千萬別告訴寶玉，只當沒有這事，總別和一個人提起。第二件，老太太、太太聽了生氣。三則襲人和你們也不好看。所以我回二奶奶，只說：『我往大奶奶那裡去來著。誰知鐲子褪了口，丟在草根底下，雪深了，沒看見。今兒雪化盡了，黃澄澄的映著日頭，還在那裡呢，我就撿了起來。』二奶奶也就信了，所以我來告訴你們。"
        .to_string();
    let mut index = sample_card("base_text");
    index.source_title = "紅樓夢（程甲本）".to_string();
    index.block_id = "block-chengjia-navigation-index".to_string();
    index.text = "[[/八十七|第八十七回]] 感秋聲撫琴悲往事 坐禪寂走火入邪魔\n[[/八十八|第八十八回]] 博庭歡寶玉讚孤兒 正家法賈珍鞭悍僕\n[[/八十九|第八十九回]] 人亡物在公子填詞 蛇影盃弓顰卿絶粧\n{{***}}\n[[/九十|第九十回]] 失綿衣貧女耐嗷嘈 送菓品小郎驚叵測\n[[/九十一|第九十一回]] 縱淫心寶蟾工設計 布疑陣寶玉妄談禪\n[[/九十四|第九十四回]] 晏海棠賈母賞花妖 失寶玉通靈知奇禍"
        .to_string();
    let package = EvidencePackage {
        package_id: "pkg-long-duplicate-card-answer-test".to_string(),
        trace_id: "trace-long-duplicate-card-answer-test".to_string(),
        question: "通灵宝玉丢失".to_string(),
        cards: vec![chengjia, wikisource, commentary, chengyi, index],
        claims: vec!["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()],
        claim_evidence_map: Vec::new(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "reviewer 通过。".to_string(),
        },
        knowledge_state_summary: KnowledgeStateSummary::default(),
    };

    let answer = local_answer("通灵宝玉丢失", &package);

    assert!(answer.contains("目前能支持回答的主要材料如下"));
    assert!(answer.contains("1. 紅樓夢（程甲本）/五十二"));
    assert!(!answer.contains("2. 紅樓夢/第052回"));
    assert!(answer.contains("2. 脂硯齋重評石頭記/第五十二回"));
    assert!(!answer.contains("紅樓夢（程乙本）/第五十一回 至第六十回"));
    assert!(!answer.contains("第九十四回"));
    assert!(!answer.contains("失寶玉通靈知奇禍"));
}

#[test]
fn upstream_evidence_brief_is_bounded_and_keeps_commentary_loss_marker() {
    let mut cards = in_scope_lost_jade_event_cards();
    cards[2].text = format!(
        "{}剛至穿堂門前，{{{{~|【庚辰雙行夾批：妙！這便是鳳姐掃雪拾玉之處，一絲不亂。】}}}}只見襲人倚門立在那裡。",
        "前置脂批材料。".repeat(100)
    );

    let brief = upstream_evidence_brief_for_frame("通灵宝玉丢了几次", &cards, None);
    let rendered = serde_json::to_string(&brief).expect("brief serializes");

    assert!(
        rendered.len() < 3200,
        "brief should stay comfortably below profile message safety budget: {}",
        rendered.len()
    );
    assert!(rendered.contains("鳳姐掃雪拾玉"));
    assert!(rendered.contains("凤姐扫雪拾玉"));
    assert!(rendered.contains("lianger_stole_jade"));
    assert!(rendered.contains("zhen_baoyu_delivers_jade"));
    assert!(rendered.contains("fengjie_snow_pickup_jade"));
    assert!(rendered.contains("suspected_transfer_related_to_loss"));
    assert!(rendered.contains("recovery_or_lost_and_found_clue"));
    for item in brief {
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .expect("brief item has text");
        assert!(
            text.chars().count() <= UPSTREAM_EVIDENCE_BRIEF_TEXT_CHARS + 6,
            "evidence brief text should be excerpted: {text}"
        );
    }
}

#[test]
fn upstream_evidence_brief_for_open_relation_keeps_object_cue() {
    let frame: question_frame::RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人服侍过谁？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头", "跟著"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("question frame");
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-xiren-shidaguniang".to_string();
    card.source_title = "紅樓夢/第019回".to_string();
    card.text = format!(
        "{}襲人道：“自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。”",
        "前置对话。".repeat(80)
    );

    let cards = vec![card];
    let brief = upstream_evidence_brief_for_frame("袭人先后服侍过哪些人？", &cards, Some(&frame));
    let requirements = question_frame_answer_requirements_value(Some(&frame), &cards)
        .expect("question frame answer requirements");
    let rendered = serde_json::to_string(&brief).expect("brief serializes");
    let rendered_requirements =
        serde_json::to_string(&requirements).expect("requirements serialize");

    assert!(rendered.contains("史大姑娘"));
    assert!(rendered.contains("ev-xiren-shidaguniang"));
    assert!(!rendered.contains(&"前置对话。".repeat(20)));
    assert!(rendered_requirements.contains("先伏侍了史大姑娘幾年"));
    assert!(!rendered_requirements.contains(&"前置对话。".repeat(20)));
}

#[test]
fn agent_runtime_step_message_compacts_context_projection_payload() {
    let mut projection = test_runtime_projection(
        "trace-compact-context",
        "context-pack://test/compact",
        "honglou-main",
        "他是谁？",
        Some("最近讨论对象：贾宝玉；最近用户问题：他是谁？".to_string()),
        vec!["tonglingyu.evidence.package.read".to_string()],
    );
    projection.projection_payload["llm_agent_context_path"] = json!({
        "raw_provider_audit": "x".repeat(20_000),
        "validator_trace": "y".repeat(20_000)
    });
    projection.projection_payload["resolver"] = json!({
        "strategy": "llm_agent_enforced",
        "needs_clarification": false,
        "resolved_question": "贾宝玉是谁？",
        "referent_bindings": ["贾宝玉"],
        "used_context_refs": ["prior_subject", "current_question"],
        "agent_decision": {"raw": "z".repeat(20_000)}
    });
    let mut cards = Vec::new();
    for index in 0..12 {
        let mut card = sample_card("base_text");
        card.evidence_id = format!("ev-compact-context-{index}");
        card.source_title = format!("紅樓夢/第{:03}回", index + 1);
        card.text = format!(
            "{}通靈玉正面鐫著“莫失莫忘，仙壽恒昌”，反面又有“一除邪祟，二療冤疾，三知禍福”。{}",
            "前置正文。".repeat(80),
            "後續正文。".repeat(80)
        );
        cards.push(card);
    }
    let evidence_brief = upstream_evidence_brief_for_frame("通灵玉是什么？", &cards, None);
    let evidence_ids = evidence_brief
        .iter()
        .filter_map(|item| item.get("evidence_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let step = RuntimeWorkflowStepReport {
        step_id: "step-03-draft-answer".to_string(),
        profile: "honglou-main".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "draft_answer".to_string(),
        status: "completed".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
        input_ref: Some("result://runtime-profiles/honglou-main".to_string()),
        output_ref: "runtime://test/step-03-draft-answer".to_string(),
        duration_ms: 1,
        trace_id: "trace-compact-context".to_string(),
        output: json!({
            "object": "tonglingyu.draft_answer",
            "package_id": "pkg-compact-context",
            "evidence_ids": evidence_ids,
            "evidence_brief": evidence_brief,
            "source_scope_policy": source_scope_policy_for_question("他是谁？"),
        }),
        agent_runtime: None,
    };

    let message = agent_runtime_profile_step_message(
        "trace-compact-context",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
    )
    .expect("runtime profile message");

    assert!(
        message.content.len() < 8192,
        "runtime profile message should stay inside safety budget: {}",
        message.content.len()
    );
    assert!(message.content.contains("贾宝玉是谁"));
    assert!(message.content.contains("projection_payload_sha256"));
    assert!(!message.content.contains(&"x".repeat(128)));
    assert!(!message.content.contains(&"z".repeat(128)));
}

#[test]
fn agent_runtime_draft_message_compacts_entity_intro_policy_under_budget() {
    let projection = test_runtime_projection(
        "trace-compact-entity-policy",
        "context-pack://test/compact-entity-policy",
        "honglou-main",
        "后四十回呢？",
        Some("最近讨论对象：史湘云；最近用户问题：史湘云的结局".to_string()),
        vec!["tonglingyu.evidence.package.read".to_string()],
    );
    let mut cards = Vec::new();
    for index in 0..8 {
        let mut card = sample_card("base_text");
        card.evidence_id = format!("ev-compact-entity-policy-{index}");
        card.source_title = format!(
            "紅樓夢（程乙本）/第{:03}回 至第{:03}回",
            81 + index,
            90 + index
        );
        card.text = format!(
            "{}史湘雲在後文相關段落中出場，仍須按本包證據邊界回答。{}",
            "前置正文。".repeat(40),
            "後續正文。".repeat(40)
        );
        cards.push(card);
    }
    let evidence_brief =
        upstream_evidence_brief_for_frame("关于史湘云的结局，后四十回呢？", &cards, None);
    let evidence_ids = evidence_brief
        .iter()
        .filter_map(|item| item.get("evidence_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let long_rule = "For entity introduction answers, use readable in-package evidence and avoid package-external biography. ".repeat(40);
    let step = RuntimeWorkflowStepReport {
        step_id: "step-03-draft-answer".to_string(),
        profile: "honglou-main".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "draft_answer".to_string(),
        status: "completed".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
        input_ref: Some("runtime://test/step-02-package-create".to_string()),
        output_ref: "runtime://test/step-03-draft-answer".to_string(),
        duration_ms: 1,
        trace_id: "trace-compact-entity-policy".to_string(),
        output: json!({
            "object": "tonglingyu.draft_answer",
            "package_id": "pkg-compact-entity-policy",
            "evidence_ids": evidence_ids,
            "evidence_brief": evidence_brief,
            "source_scope_policy": source_scope_policy_for_question("关于史湘云的结局，后四十回呢？"),
            "entity_intro_answer_policy": {
                "object": "tonglingyu.entity_intro_answer_policy",
                "schema_version": "tonglingyu.entity_intro_answer_policy.v1",
                "applies": true,
                "entity": {
                    "canonical": "史湘云",
                    "aliases": ["史湘云", "史湘雲", "湘云", "湘雲", "云妹妹", "雲妹妹"]
                },
                "min_supporting_cards": 2,
                "max_supporting_cards": 4,
                "max_quote_chars": 56,
                "min_substantive_chars": 18,
                "short_speech_shell_max_chars": 8,
                "excluded_public_quote_terms": [
                    "要知端的，且聽下回分解",
                    "要知端的，且听下回分解",
                    "且聽下回分解",
                    "且听下回分解"
                ],
                "rule": long_rule
            }
        }),
        agent_runtime: None,
    };

    let message = agent_runtime_profile_step_message(
        "trace-compact-entity-policy",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
    )
    .expect("runtime profile message");

    assert!(
        message.content.len() < 8192,
        "entity intro draft profile message should stay inside safety budget: {}",
        message.content.len()
    );
    assert!(message.content.contains("tonglingyu-upstream-bundle-v1"));
    assert!(
        !message
            .content
            .contains(&"For entity introduction answers".repeat(8))
    );
}

#[test]
fn agent_runtime_evidence_search_message_does_not_expose_runtime_evidence_ids() {
    let projection = test_runtime_projection(
        "trace-evidence-search-message-boundary",
        "context-pack://test/evidence-search-message-boundary",
        "honglou-text",
        "介绍贾宝玉",
        None,
        vec!["tonglingyu.text.search".to_string()],
    );
    let step = RuntimeWorkflowStepReport {
        step_id: "step-01-text-search".to_string(),
        profile: "honglou-text".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "text_evidence_search".to_string(),
        status: "completed".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.text.search".to_string()],
        tool_calls: vec!["tonglingyu.text.search".to_string()],
        input_ref: None,
        output_ref: "runtime://test/step-01-text-search".to_string(),
        duration_ms: 1,
        trace_id: "trace-evidence-search-message-boundary".to_string(),
        output: json!({
            "object": "tonglingyu.text.evidence_search",
            "card_count": 2,
            "evidence_ids": ["ev-transient-one", "ev-transient-two"],
            "evidence_types": ["base_text"],
        }),
        agent_runtime: None,
    };

    let message = agent_runtime_profile_step_message(
        "trace-evidence-search-message-boundary",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
    )
    .expect("runtime profile message");

    assert!(message.content.contains("do_not_echo_runtime_ids"));
    assert!(message.content.contains("evidence_set_ref"));
    assert!(message.content.contains("non-empty JSON object"));
    assert!(message.content.contains("\"evidence_refs\":[]"));
    assert!(!message.content.contains("ev-transient-one"));
    assert!(!message.content.contains("ev-transient-two"));
}

#[test]
fn agent_runtime_step_message_bounds_loss_event_evidence_slot_payload() {
    let projection = test_runtime_projection(
        "trace-loss-event-message-budget",
        "context-pack://test/loss-event-message-budget",
        "honglou-main",
        "通灵宝玉丢了几次",
        Some("最近用户在追问通灵宝玉失玉次数。".to_string()),
        vec!["tonglingyu.evidence.package.read".to_string()],
    );
    let mut cards = in_scope_lost_jade_event_cards();
    cards[0].text = format!(
        "{}{}{}",
        "前置正文。".repeat(120),
        cards[0].text,
        "后续正文。".repeat(120)
    );
    cards[2].text = format!(
        "{}{}{}",
        "前置脂批材料。".repeat(120),
        cards[2].text,
        "后续脂批材料。".repeat(120)
    );
    let evidence_brief = upstream_evidence_brief_for_frame("通灵宝玉丢了几次", &cards, None);
    let evidence_ids = evidence_brief
        .iter()
        .filter_map(|item| item.get("evidence_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let step = RuntimeWorkflowStepReport {
        step_id: "step-03-draft-answer".to_string(),
        profile: "honglou-main".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "draft_answer".to_string(),
        status: "completed".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
        input_ref: Some("runtime://test/package".to_string()),
        output_ref: "runtime://test/step-03-draft-answer".to_string(),
        duration_ms: 1,
        trace_id: "trace-loss-event-message-budget".to_string(),
        output: json!({
            "object": "tonglingyu.draft_answer",
            "package_id": "pkg-loss-event-message-budget",
            "evidence_ids": evidence_ids,
            "evidence_brief": evidence_brief,
            "evidence_slot_count_policy": evidence_slot_count_context_value(
                "通灵宝玉丢了几次",
                &cards,
                true,
            )
            .expect("slot count policy"),
            "source_scope_policy": source_scope_policy_for_question("通灵宝玉丢了几次"),
            "answer_requirements": answer_requirements_for_message(
                "通灵宝玉丢了几次",
                &cards,
            )
            .expect("answer requirements"),
        }),
        agent_runtime: None,
    };

    let message = agent_runtime_profile_step_message(
        "trace-loss-event-message-budget",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
    )
    .expect("runtime profile message");

    assert!(
        message.content.len() < AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES,
        "loss event profile message should stay inside safety budget: {}",
        message.content.len()
    );
    assert!(message.content.contains("evidence_slots"));
    assert!(message.content.contains("lianger_stole_jade"));
    assert!(message.content.contains("zhen_baoyu_delivers_jade"));
    assert!(message.content.contains("fengjie_snow_pickup_jade"));
    assert!(!message.content.contains(&"前置脂批材料。".repeat(20)));

    let compact_step_output = step_output_message_payload_with_compaction(&step, 3);
    assert_eq!(
        compact_step_output["answer_requirements"]["rule_id"],
        json!("answer_requirements.anchor_cues_required")
    );
    assert!(
        !compact_step_output
            .to_string()
            .contains("When requested_evidence_type_anchors is non-empty")
    );
}

#[test]
fn agent_runtime_draft_repair_message_uses_minimal_payload_under_budget() {
    let projection = test_runtime_projection(
        "trace-repair-message-budget",
        "context-pack://test/repair-message-budget",
        "honglou-main",
        "通灵宝玉丢了几次",
        Some("最近用户在追问通灵宝玉失玉次数。".to_string()),
        vec!["tonglingyu.evidence.package.read".to_string()],
    );
    let cards = in_scope_lost_jade_event_cards();
    let evidence_brief = upstream_evidence_brief_for_frame("通灵宝玉丢了几次", &cards, None);
    let evidence_ids = evidence_brief
        .iter()
        .filter_map(|item| item.get("evidence_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let step = RuntimeWorkflowStepReport {
        step_id: "step-03-draft-answer".to_string(),
        profile: "honglou-main".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "draft_answer".to_string(),
        status: "completed".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
        input_ref: Some("runtime://test/package".to_string()),
        output_ref: "runtime://test/step-03-draft-answer".to_string(),
        duration_ms: 1,
        trace_id: "trace-repair-message-budget".to_string(),
        output: json!({
            "object": "tonglingyu.draft_answer",
            "package_id": "pkg-repair-message-budget",
            "evidence_ids": evidence_ids,
            "evidence_brief": evidence_brief,
            "evidence_slot_count_policy": evidence_slot_count_context_value(
                "通灵宝玉丢了几次",
                &cards,
                true,
            )
            .expect("slot count context"),
            "source_scope_policy": source_scope_policy_for_question("通灵宝玉丢了几次"),
        }),
        agent_runtime: None,
    };

    let message = agent_runtime_profile_step_repair_message(
        "trace-repair-message-budget",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
        "draft_missing_embedded_evidence_source",
    )
    .expect("runtime profile repair message");

    assert!(
        message.content.len() < AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES,
        "repair message should stay inside safety budget: {}",
        message.content.len()
    );
    assert!(message.content.contains("draft_repair_context_json"));
    assert!(message.content.contains("source_cues"));
    assert!(message.content.contains("第18回"));
    assert!(!message.content.contains("context_projection_ref"));
}

#[test]
fn agent_runtime_open_object_draft_repair_message_stays_under_budget() {
    let question = "袭人先后服侍过哪些人？";
    let frame: question_frame::RuntimeQuestionFrame =
        serde_json::from_value(xiren_open_object_relation_question_frame_value())
            .expect("open-object frame");
    let projection = test_runtime_projection(
        "trace-open-object-repair-message-budget",
        "context-pack://test/open-object-repair-message-budget",
        "honglou-main",
        question,
        Some("最近用户连续追问人物服侍关系，需要保持开放对象关系边界。".repeat(8)),
        vec!["tonglingyu.evidence.package.read".to_string()],
    );
    let mut cards = Vec::new();
    for (index, (evidence_id, source_title, text)) in [
        (
            "ev-xiren-jiamu-baoyu-budget",
            "脂硯齋重評石頭記甲戌本/第三回",
            "原來這襲人亦是賈母之婢，本名珍珠；伏侍賈母時，心中眼中只有一個賈母，今與寶玉，心中眼中又只有一個寶玉。",
        ),
        (
            "ev-xiren-shidaguniang-budget",
            "紅樓夢/第019回",
            "襲人道：「自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。」",
        ),
        (
            "ev-xiren-baoyu-budget",
            "脂硯齋重評石頭記甲戌本/第三回",
            "寶玉之乳母李嬤嬤，並大丫嬛名喚襲人者，陪侍在外大床上。",
        ),
        (
            "ev-xiren-many-objects-budget",
            "紅樓夢/第021回",
            "襲人伏侍了贾母、宝玉、林黛玉、薛宝钗、王熙凤、王夫人、晴雯、史湘云。",
        ),
        (
            "ev-xiren-more-objects-budget",
            "紅樓夢/第022回",
            "襲人侍候过贾探春、贾迎春、贾惜春、李纨、妙玉、秦可卿。",
        ),
    ]
    .iter()
    .enumerate()
    {
        let mut card = sample_card(if index % 2 == 0 {
            "base_text"
        } else {
            "commentary"
        });
        card.evidence_id = (*evidence_id).to_string();
        card.source_title = (*source_title).to_string();
        card.text = format!("{}{}{}", "前置材料。".repeat(80), text, "后续材料。".repeat(80));
        card.support_scope = "开放对象关系证据需要同时保留主体、关系词和对象线索。".repeat(8);
        card.unsupported_scope = "不能把未命中的对象扩展为最终完整名单。".repeat(8);
        cards.push(card);
    }
    let evidence_brief = upstream_evidence_brief_for_frame(question, &cards, Some(&frame));
    let evidence_ids = evidence_brief
        .iter()
        .filter_map(|item| item.get("evidence_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let step = RuntimeWorkflowStepReport {
        step_id: "step-04-draft-answer".to_string(),
        profile: "honglou-main".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "draft_answer".to_string(),
        status: "completed".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
        input_ref: Some("runtime://test/package".to_string()),
        output_ref: "runtime://test/step-04-draft-answer".to_string(),
        duration_ms: 1,
        trace_id: "trace-open-object-repair-message-budget".to_string(),
        output: json!({
            "object": "tonglingyu.draft_answer",
            "package_id": "pkg-open-object-repair-message-budget",
            "evidence_ids": evidence_ids,
            "evidence_brief": evidence_brief,
            "answer_requirements": answer_requirements_for_message(question, &cards)
                .expect("answer requirements"),
            "question_frame_answer_requirements": question_frame_answer_requirements_value(
                Some(&frame),
                &cards,
            )
            .expect("question frame answer requirements"),
            "source_scope_policy": source_scope_policy_for_question(question),
        }),
        agent_runtime: None,
    };

    let message = agent_runtime_profile_step_repair_message(
        "trace-open-object-repair-message-budget",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
        "draft_missing_open_relation_object",
    )
    .expect("runtime profile repair message");

    assert!(
        message.content.len() < AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES,
        "open-object repair message should stay inside safety budget: {}",
        message.content.len()
    );
    assert!(
        message
            .content
            .contains("draft_missing_open_relation_object")
    );
    assert!(message.content.contains("must_mention"));
    assert!(message.content.contains("史湘云"));
    assert!(!message.content.contains(&"前置材料。".repeat(20)));
    let compact_content = agent_runtime_profile_step_repair_message_content(
        "trace-open-object-repair-message-budget",
        &step,
        &projection,
        &agent_runtime_result_summary_contract(&step).expect("runtime prompt contract"),
        "draft_missing_open_relation_object",
        5,
    )
    .expect("runtime profile repair message content");
    assert!(compact_content.len() < AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES);
    assert!(compact_content.contains(UPSTREAM_BUNDLE_SCHEMA_VERSION));
    assert!(!compact_content.contains("Return exactly one non-empty JSON object with this shape"));
}

#[test]
fn trim_text_around_locates_normalized_focus_without_mutating_raw_text() {
    let text = format!("{}史湘雲問道：“寶玉哥哥不在家么？”", "甲".repeat(300));

    let snippet = trim_text_around(&text, "史湘云", 40);

    assert!(snippet.starts_with("..."));
    assert!(snippet.contains("史湘雲問道"));
    assert!(!snippet.contains("史湘云问道"));
}

#[test]
fn redacted_query_terms_hash_sensitive_patterns() {
    for sensitive in [
        "password=SECRET_RUNTIME_TOKEN",
        "token=SECRET_RUNTIME_TOKEN",
        "api_key=SECRET_RUNTIME_TOKEN",
        "https://example.invalid/path?token=SECRET_RUNTIME_TOKEN",
        "reader@example.invalid",
        "+8613800138000",
        "ABCD1234EFGH5678IJKL9012",
    ] {
        let redacted = redacted_query_term(sensitive);
        assert!(redacted.starts_with("sha256:"), "{sensitive} -> {redacted}");
        assert!(!redacted.contains("SECRET_RUNTIME_TOKEN"));
        assert!(!redacted.contains("example.invalid"));
        assert!(!redacted.contains("13800138000"));
    }

    let question = concat!(
        "通灵玉 token=SECRET_RUNTIME_TOKEN ",
        "https://example.invalid/a?secret=SECRET_RUNTIME_TOKEN ",
        "reader@example.invalid +8613800138000"
    );
    let terms = redacted_terms_from_question(question);
    let rendered = terms.join(" ");
    assert!(terms.iter().any(|term| term.starts_with("sha256:")));
    assert!(rendered.contains("通灵玉"));
    for leaked in [
        "SECRET_RUNTIME_TOKEN",
        "example.invalid",
        "reader@example.invalid",
        "13800138000",
    ] {
        assert!(!rendered.contains(leaked));
    }
}

#[test]
fn required_exact_terms_protect_core_eval_targets() {
    assert_eq!(
        required_exact_terms("通灵玉上的字是什么？").expect("exact terms"),
        vec!["莫失莫忘".to_string(), "一除邪祟".to_string()]
    );
    assert_eq!(
        required_exact_terms("青埂峰和顽石在哪里出现？").expect("exact terms"),
        vec!["青埂".to_string()]
    );
    assert_eq!(
        required_exact_terms("一百二十回本第八回通灵玉在哪里？").expect("exact terms"),
        vec!["第八回".to_string()]
    );
    assert_eq!(
        required_exact_terms("后四十回从哪里开始？").expect("exact terms"),
        vec!["第八十一".to_string()]
    );
    assert!(
        !required_exact_terms("关于史湘云的结局，后四十回呢？")
            .expect("exact terms")
            .contains(&"第八十一".to_string())
    );
    assert!(
        !required_exact_terms("通灵宝玉丢了几次")
            .expect("exact terms")
            .contains(&"寳玉".to_string())
    );
    assert_eq!(
        required_exact_terms("寳玉和通灵玉是什么关系？").expect("exact terms"),
        vec!["寳玉".to_string()]
    );
}

#[test]
fn query_expansion_catalog_cache_hot_reloads_external_file() {
    let catalog_path = std::env::temp_dir().join(format!(
        "tonglingyu-query-expansions-{}.json",
        uuid::Uuid::now_v7().simple()
    ));
    let initial_catalog = r#"{
        "schema_version": "tonglingyu.query_expansions.v1",
        "catalog_version": "test.1",
        "entries": [
            {
                "id": "test:hot-reload",
                "trigger": { "any": ["热加载问题"] },
                "terms": ["初始热词"]
            }
        ]
    }"#;
    let updated_catalog = r#"{
        "schema_version": "tonglingyu.query_expansions.v1",
        "catalog_version": "test.2",
        "entries": [
            {
                "id": "test:hot-reload",
                "trigger": { "any": ["热加载问题"] },
                "terms": ["更新热词", "新增热词"]
            }
        ]
    }"#;

    std::fs::write(&catalog_path, initial_catalog).expect("write initial catalog");
    let mut cache = rule_catalog::RuleFileCache::new(default_query_expansion_catalog());
    let catalog = cache
        .catalog(
            QUERY_EXPANSIONS_PATH_ENV,
            Some(catalog_path.clone()),
            default_query_expansion_catalog(),
            parse_query_expansion_catalog,
        )
        .expect("load initial catalog");
    let normalized = normalize_query("热加载问题");
    let mut terms = Vec::new();
    apply_query_expansion_terms(&catalog, "热加载问题", &normalized, &mut terms);
    assert_eq!(terms, vec!["初始热词".to_string()]);

    std::fs::write(&catalog_path, updated_catalog).expect("write updated catalog");
    let catalog = cache
        .catalog(
            QUERY_EXPANSIONS_PATH_ENV,
            Some(catalog_path.clone()),
            default_query_expansion_catalog(),
            parse_query_expansion_catalog,
        )
        .expect("reload updated catalog");
    let mut terms = Vec::new();
    apply_query_expansion_terms(&catalog, "热加载问题", &normalized, &mut terms);
    assert_eq!(terms, vec!["更新热词".to_string(), "新增热词".to_string()]);

    std::fs::remove_file(catalog_path).expect("remove catalog");
}

#[test]
fn query_expansion_catalog_rejects_invalid_schema() {
    let err = parse_query_expansion_catalog(
        r#"{"schema_version":"wrong","catalog_version":"test","entries":[]}"#,
    )
    .expect_err("invalid schema rejected");
    assert!(
        err.to_string()
            .contains("query expansion catalog schema_version must be")
    );
}

#[test]
fn query_expansion_catalog_requires_catalog_version() {
    let err = parse_query_expansion_catalog(
        r#"{"schema_version":"tonglingyu.query_expansions.v1","catalog_version":"","entries":[]}"#,
    )
    .expect_err("empty catalog version rejected");
    assert!(
        err.to_string()
            .contains("query expansion catalog catalog_version is required")
    );
}

#[test]
fn runtime_rule_catalog_metadata_reports_all_runtime_catalogs() {
    let metadata = runtime_rule_catalog_metadata().expect("runtime rule metadata");

    assert_eq!(
        metadata["object"],
        json!("tonglingyu.runtime_rule_catalog_metadata")
    );
    for key in [
        "query_expansions",
        "evidence_slot_rules",
        "governance_rules",
        "retrieval_rules",
        "ontology_aliases",
        "answer_rules",
    ] {
        assert!(
            metadata[key]["catalog_version"]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty()),
            "missing catalog_version for {key}"
        );
        assert!(
            matches!(
                metadata[key]["source"].as_str(),
                Some("embedded_default" | "external_file")
            ),
            "missing source for {key}"
        );
        assert!(metadata[key]["schema_version"].as_str().is_some());
    }
}

#[test]
fn exact_text_lookup_prefers_primary_source_snapshot() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_knowledge_base_schema(&conn).expect("kb schema");
    for source_id in [
        "hongloumeng-wikisource-chengjia",
        "hongloumeng-wikisource-120",
        "hongloumeng-wikisource-chengyi",
    ] {
        conn.execute(
            r#"
                INSERT INTO sources (
                    source_id, source_category, format, title, work, edition,
                    language, source_url, api_url, fetched_at,
                    snapshot_contract_json, source_hash
                ) VALUES (?1, 'base_material', 'mediawiki', ?1, '红楼梦',
                    'test', 'zh', 'https://example.test/source',
                    'https://example.test/api', '2026-05-15T00:00:00Z',
                    '{}', ?1)
                "#,
            params![source_id],
        )
        .expect("insert source");
    }
    for (block_id, source_id, text) in [
        (
            "chengjia-short",
            "hongloumeng-wikisource-chengjia",
            "青埂短文",
        ),
        (
            "primary-long",
            "hongloumeng-wikisource-120",
            "青埂峰下主要 source snapshot 证据，文字更长。",
        ),
        (
            "chengyi-short",
            "hongloumeng-wikisource-chengyi",
            "青埂短文",
        ),
    ] {
        conn.execute(
            r#"
                INSERT INTO blocks (
                    block_id, source_id, section_id, source_title, source_url,
                    revision_id, block_index, kind, tag, text, normalized_text,
                    evidence_type, chapter_no
                ) VALUES (?1, ?2, 'section', 'source title', 'https://example.test',
                    1, 1, 'paragraph', NULL, ?3, ?4, 'base_text', 1)
                "#,
            params![block_id, source_id, text, normalize_text(text)],
        )
        .expect("insert block");
    }

    let rows = query_blocks_exact_text(&conn, "青埂", 3).expect("query blocks");
    assert_eq!(rows[0].block_id, "primary-long");
}

#[test]
fn retrieval_quality_report_blocks_production_without_source_usage_metadata() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "source_of_record": "raw MediaWiki wikitext plus revision metadata",
        }),
    );

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "通灵玉是什么？".to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards { quality_report, .. } = output else {
        panic!("expected evidence cards");
    };
    assert_eq!(quality_report.quality_status, "needs_attention");
    assert!(!quality_report.production_ready);
    assert!(quality_report.issues.iter().any(|issue| {
            issue
                == "source_usage_metadata_incomplete:quality-source:missing_license_and_license_url_and_attribution_and_usage_boundary_metadata"
        }));
    assert!(
        quality_report.recommended_follow_up.iter().any(|item| {
            item == "add_machine_readable_source_license_usage_attribution_metadata"
        })
    );
}

#[test]
fn retrieval_quality_report_fails_when_required_type_is_missing() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "通灵玉是什么？".to_string(),
            limit: 2,
            required_evidence_types: vec!["commentary".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards { quality_report, .. } = output else {
        panic!("expected evidence cards");
    };
    assert_eq!(quality_report.quality_status, "failed");
    assert!(!quality_report.production_ready);
    assert!(
        quality_report
            .issues
            .iter()
            .any(|issue| { issue == "missing_required_evidence_type:commentary" })
    );
}

#[test]
fn retrieval_quality_report_fails_when_no_evidence_is_selected() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "不存在的检索目标".to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert!(cards.is_empty());
    assert_eq!(quality_report.quality_status, "failed");
    assert!(!quality_report.production_ready);
    assert!(
        quality_report
            .issues
            .iter()
            .any(|issue| { issue == "no_evidence_selected" })
    );
    assert!(
        quality_report
            .issues
            .iter()
            .any(|issue| { issue == "missing_required_evidence_type:base_text" })
    );
}

#[test]
fn retrieval_quality_report_fails_when_required_exact_term_is_missing() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );

    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: "寳玉和通灵玉是什么关系？".to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");

    let TonglingyuToolOutput::EvidenceCards { quality_report, .. } = output else {
        panic!("expected evidence cards");
    };
    assert_eq!(quality_report.quality_status, "failed");
    assert!(!quality_report.production_ready);
    assert!(
        quality_report
            .exact_match_coverage
            .iter()
            .any(|coverage| { coverage.term == "寳玉" && !coverage.matched })
    );
    assert_eq!(quality_report.protected_terms, vec!["寳玉".to_string()]);
    assert!(
        quality_report
            .issues
            .iter()
            .any(|issue| { issue == "required_exact_term_not_selected:寳玉" })
    );
}

#[test]
fn retrieval_failure_schema_migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let before = runtime_schema_migration_preflight(&conn).expect("preflight before schema");
    assert!(
        before["pending_migrations"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some(RETRIEVAL_FAILURE_SCHEMA_VERSION)))
    );
    assert!(
        before["pending_migrations"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some(RETRIEVAL_FAILURE_PRIVACY_MIGRATION)))
    );
    assert_eq!(before["contains_secret_values"], json!(false));
    assert_eq!(before["will_delete_runtime_data"], json!(false));

    init_runtime_schema(&conn).expect("runtime schema");
    init_runtime_schema(&conn).expect("runtime schema idempotent");

    let after = runtime_schema_migration_preflight(&conn).expect("preflight after schema");
    assert_eq!(after["pending_migrations"], json!([]));
    let migration_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE migration_id = ?1",
            params![RETRIEVAL_FAILURE_SCHEMA_VERSION],
            |row| row.get(0),
        )
        .expect("migration count");
    assert_eq!(migration_count, 1);
    assert!(sqlite_table_exists(&conn, "retrieval_failures").expect("table check"));
    let retrieval_failure_columns =
        sqlite_table_columns(&conn, "retrieval_failures").expect("retrieval failure columns");
    assert!(retrieval_failure_columns.contains("question_sha256"));
    assert!(retrieval_failure_columns.contains("question_summary"));
    assert!(retrieval_failure_columns.contains("redacted_question_excerpt"));
    assert!(retrieval_failure_columns.contains("redacted_query_terms_json"));
    assert!(!retrieval_failure_columns.contains("question"));
    assert!(sqlite_table_exists(&conn, "knowledge_governance_tasks").expect("table check"));
    assert!(sqlite_table_exists(&conn, "knowledge_patch_proposals").expect("table check"));
}

#[test]
fn governance_task_schema_migrates_legacy_failure_only_tasks() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(
        r#"
            CREATE TABLE retrieval_failures (
                failure_id TEXT PRIMARY KEY,
                trace_id TEXT NOT NULL,
                package_id TEXT,
                question_sha256 TEXT NOT NULL,
                question_char_count INTEGER NOT NULL,
                question_summary TEXT NOT NULL,
                kb_schema_version TEXT NOT NULL,
                kb_version_id TEXT,
                failure_type TEXT NOT NULL,
                redacted_query_terms_json TEXT NOT NULL,
                required_evidence_types_json TEXT NOT NULL,
                actual_evidence_types_json TEXT NOT NULL,
                expected_evidence_ids_json TEXT NOT NULL,
                selected_evidence_ids_json TEXT NOT NULL,
                missing_evidence_types_json TEXT NOT NULL,
                quality_issues_json TEXT NOT NULL,
                agent_diagnosis TEXT,
                proposed_fix TEXT,
                human_review_status TEXT NOT NULL,
                reviewer TEXT,
                review_note TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                resolved_at TEXT
            );
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
                'rf-legacy', 'trace-legacy', 'pkg-legacy',
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                12, 'legacy question', 'tonglingyu-kb-v1', NULL,
                'expected_evidence_missing', '[]', '[]', '[]', '[]', '[]',
                '[]', '[]', NULL, 'review legacy failure', 'open', NULL,
                NULL, '2026-05-15T00:00:00Z',
                '2026-05-15T00:00:00Z', NULL
            );
            CREATE TABLE knowledge_governance_tasks (
                task_id TEXT PRIMARY KEY,
                source_failure_id TEXT NOT NULL,
                trace_id TEXT NOT NULL,
                package_id TEXT,
                task_type TEXT NOT NULL,
                status TEXT NOT NULL,
                priority TEXT NOT NULL,
                agent_cluster_key TEXT NOT NULL,
                proposed_fix TEXT NOT NULL,
                reviewer TEXT,
                review_note TEXT,
                evidence_ref TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                accepted_at TEXT,
                closed_at TEXT
            );
            INSERT INTO knowledge_governance_tasks (
                task_id, source_failure_id, trace_id, package_id, task_type,
                status, priority, agent_cluster_key, proposed_fix, reviewer,
                review_note, evidence_ref, created_at, updated_at, accepted_at,
                closed_at
            ) VALUES (
                'kgt-legacy', 'rf-legacy', 'trace-legacy', 'pkg-legacy',
                'expected_evidence_fix', 'open', 'p0', 'rf:legacy',
                'review legacy failure', NULL, NULL, NULL,
                '2026-05-15T00:00:00Z', '2026-05-15T00:00:00Z', NULL, NULL
            );
            "#,
    )
    .expect("legacy governance task schema");

    init_runtime_schema(&conn).expect("runtime schema migrates legacy governance tasks");
    let failure_columns =
        sqlite_table_columns(&conn, "retrieval_failures").expect("failure table columns");
    assert!(failure_columns.contains("redacted_question_excerpt"));
    assert!(!failure_columns.contains("question"));
    let migrated_excerpt: String = conn
            .query_row(
                "SELECT redacted_question_excerpt FROM retrieval_failures WHERE failure_id = 'rf-legacy'",
                [],
                |row| row.get(0),
            )
            .expect("migrated excerpt");
    assert_eq!(migrated_excerpt, "legacy question");
    let columns = sqlite_table_columns(&conn, "knowledge_governance_tasks").expect("table columns");
    assert!(columns.contains("source_entity_type"));
    assert!(columns.contains("source_entity_id"));
    let task = load_governance_task(&conn, "kgt-legacy")
        .expect("load migrated task")
        .expect("migrated task exists");
    assert_eq!(task.source_failure_id.as_deref(), Some("rf-legacy"));
    assert_eq!(task.source_entity_type, "retrieval_failure");
    assert_eq!(task.source_entity_id, "rf-legacy");

    let trace_task = create_governance_task(
        &conn,
        KnowledgeGovernanceTaskCreateInput {
            source_entity_type: "trace".to_string(),
            source_entity_id: "trace-after-legacy-migration".to_string(),
            trace_id: "trace-after-legacy-migration".to_string(),
            package_id: None,
            source_failure_id: None,
            task_type: "expert_review".to_string(),
            priority: Some("p0".to_string()),
            proposed_fix: Some("request expert review".to_string()),
            agent_cluster_key: None,
        },
    )
    .expect("create trace task after migration");
    assert_eq!(trace_task.source_entity_type, "trace");
}

#[test]
fn runtime_schema_rolls_back_failed_migration_batch() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute(
        "CREATE TABLE retrieval_failures (failure_id TEXT PRIMARY KEY)",
        [],
    )
    .expect("create incompatible table");

    let error = init_runtime_schema(&conn).expect_err("incompatible schema should fail");
    assert!(error.to_string().contains("retrieval_failures"));
    assert!(!sqlite_table_exists(&conn, "schema_migrations").expect("table check"));
    assert!(!sqlite_table_exists(&conn, "audit_events").expect("table check"));
}

fn sample_knowledge_item_input(kind: KnowledgeItemKind, marker: &str) -> KnowledgeItemCreateInput {
    KnowledgeItemCreateInput {
        kind,
        initial_state: KnowledgeState::Candidate,
        source_refs: vec![format!("source://wikisource/chapter/{marker}")],
        evidence_refs: vec![format!("block://wikisource/{marker}")],
        payload: json!({
            "marker": marker,
            "claim": format!("sample knowledge item {marker}"),
        }),
        schema_version: None,
        trace_id: format!("trace-knowledge-item-{marker}"),
        actor: "system-calibration-test".to_string(),
        reason: "create candidate knowledge item".to_string(),
    }
}

fn all_knowledge_item_kinds() -> Vec<KnowledgeItemKind> {
    vec![
        KnowledgeItemKind::Alias,
        KnowledgeItemKind::Term,
        KnowledgeItemKind::CommentaryLink,
        KnowledgeItemKind::VersionNote,
        KnowledgeItemKind::Person,
        KnowledgeItemKind::Relationship,
        KnowledgeItemKind::Event,
        KnowledgeItemKind::Poem,
        KnowledgeItemKind::EvaluationCase,
    ]
}

fn sample_rule_context(marker: &str) -> KnowledgeCalibrationRuleContext {
    KnowledgeCalibrationRuleContext {
        source_id: "wikisource".to_string(),
        block_id: format!("wikisource/{marker}"),
        required_evidence_type: "base_text".to_string(),
        exact_terms: vec![marker.to_string()],
        version_boundary: "Wikisource source snapshot only".to_string(),
        usage_boundary: "runtime candidate, not human marked".to_string(),
    }
}

fn calibration_run_input(
    item: &KnowledgeItemRecord,
    method: KnowledgeCalibrationMethod,
) -> KnowledgeCalibrationRunInput {
    let marker = item
        .payload
        .get("marker")
        .and_then(Value::as_str)
        .unwrap_or("calibration");
    KnowledgeCalibrationRunInput {
        item_id: item.item_id.clone(),
        input_kind: KnowledgeCalibrationInputKind::SourceSnapshot,
        input_ref: format!("source://wikisource/chapter/{marker}"),
        method,
        trace_id: format!("trace-calibration-{marker}"),
        actor: "runtime-calibrator-test".to_string(),
        llm_config: None,
        llm_judgement: None,
        rule_context: (method == KnowledgeCalibrationMethod::Rule)
            .then(|| sample_rule_context(marker)),
        eval_context: None,
        rqa_context: None,
    }
}

fn sample_knowledge_item_review_task(
    conn: &Connection,
    item: &KnowledgeItemRecord,
    marker: &str,
) -> KnowledgeGovernanceTaskRecord {
    create_governance_task(
        conn,
        KnowledgeGovernanceTaskCreateInput {
            source_entity_type: "knowledge_item".to_string(),
            source_entity_id: item.item_id.clone(),
            trace_id: format!("trace-human-review-{marker}"),
            package_id: None,
            source_failure_id: None,
            task_type: "expert_review".to_string(),
            priority: Some("p0".to_string()),
            proposed_fix: Some("review knowledge item state without fact mutation".to_string()),
            agent_cluster_key: Some(format!("knowledge_item:{marker}")),
        },
    )
    .expect("create knowledge item review task")
}

fn valid_calibration_env() -> BTreeMap<String, String> {
    let digest_a = "a".repeat(64);
    let digest_b = "b".repeat(64);
    BTreeMap::from([
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE".to_string(),
            KNOWLEDGE_CALIBRATION_PROFILE_ID.to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION".to_string(),
            KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION.to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL".to_string(),
            "hermes-calibration-frontier".to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_UPSTREAM_ID".to_string(),
            "runtime-hermes-internal".to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROMPT_DIGEST".to_string(),
            digest_a,
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_TOOL_POLICY_DIGEST".to_string(),
            digest_b,
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_DECODING".to_string(),
            r#"{"temperature":0.0,"top_p":0.1}"#.to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_TIMEOUT_SECS".to_string(),
            "60".to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_RETRY_LIMIT".to_string(),
            "3".to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL_CAPABILITY".to_string(),
            "frontier".to_string(),
        ),
        (
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_REASONING_EFFORT".to_string(),
            "high".to_string(),
        ),
    ])
}

fn valid_llm_config() -> KnowledgeCalibrationLlmConfig {
    KnowledgeCalibrationLlmConfig::from_env_map(&valid_calibration_env())
        .expect("valid calibration config")
}

#[test]
fn knowledge_calibration_schema_and_profile_are_internal() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let before = runtime_schema_migration_preflight(&conn).expect("preflight before schema");
    for migration in [
        KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION,
        KNOWLEDGE_CALIBRATION_JOB_SCHEMA_VERSION,
        KNOWLEDGE_ITEM_CALIBRATION_LINK_MIGRATION,
    ] {
        assert!(
            before["pending_migrations"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(migration))),
            "missing pending migration {migration}"
        );
    }

    init_runtime_schema(&conn).expect("runtime schema");
    assert!(
        sqlite_table_exists(&conn, "knowledge_calibration_reports")
            .expect("calibration reports table")
    );
    assert!(
        sqlite_table_exists(&conn, "knowledge_calibration_jobs").expect("calibration jobs table")
    );
    assert!(
        sqlite_table_exists(&conn, "knowledge_calibration_job_history")
            .expect("calibration job history table")
    );
    let columns = sqlite_table_columns(&conn, "knowledge_items").expect("knowledge item columns");
    for column in [
        "source_boundary_json",
        "calibration_report_ref",
        "confidence",
    ] {
        assert!(columns.contains(column), "missing column {column}");
    }

    assert!(
        profile_catalog()
            .iter()
            .all(|profile| profile.profile != KNOWLEDGE_CALIBRATION_PROFILE_ID)
    );
    let descriptor = knowledge_calibration_profile_descriptor();
    assert_eq!(descriptor.profile, KNOWLEDGE_CALIBRATION_PROFILE_ID);
    assert!(descriptor.allowed_tools.is_empty());
    assert_eq!(
        descriptor.safety_contract["hidden_from_openwebui_model_list"],
        json!(true)
    );
    let contract = knowledge_calibration_profile_contract();
    assert_eq!(contract.profile_id, KNOWLEDGE_CALIBRATION_PROFILE_ID);
    assert!(contract.tool_policy.allowed_tools.is_empty());
}

#[test]
fn knowledge_calibration_llm_config_is_bound_and_fail_closed() {
    let env = valid_calibration_env();
    let config =
        KnowledgeCalibrationLlmConfig::from_env_map(&env).expect("valid calibration config");
    assert_eq!(config.profile_id, KNOWLEDGE_CALIBRATION_PROFILE_ID);
    assert_eq!(config.reasoning_effort, "high");
    assert_eq!(config.model_capability, "frontier");
    assert_eq!(config.config_digest.len(), 64);
    let release_report = knowledge_calibration_release_report(&config);
    assert_eq!(release_report["contains_secret_values"], json!(false));
    assert_eq!(
        release_report["runtime_usable_auto_promotion"],
        json!(false)
    );

    let mut missing_prompt = env.clone();
    missing_prompt.remove("TONGLINGYU_KNOWLEDGE_CALIBRATION_PROMPT_DIGEST");
    assert!(
        KnowledgeCalibrationLlmConfig::from_env_map(&missing_prompt)
            .expect_err("missing prompt digest fails closed")
            .to_string()
            .contains("PROMPT_DIGEST")
    );
    let mut missing_model = env.clone();
    missing_model.remove("TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL");
    assert!(
        KnowledgeCalibrationLlmConfig::from_env_map(&missing_model)
            .expect_err("missing model fails closed")
            .to_string()
            .contains("MODEL")
    );
    let mut missing_upstream = env.clone();
    missing_upstream.remove("TONGLINGYU_KNOWLEDGE_CALIBRATION_UPSTREAM_ID");
    assert!(
        KnowledgeCalibrationLlmConfig::from_env_map(&missing_upstream)
            .expect_err("missing upstream fails closed")
            .to_string()
            .contains("UPSTREAM_ID")
    );
    let mut unknown_profile = env.clone();
    unknown_profile.insert(
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE".to_string(),
        "honglou-main".to_string(),
    );
    assert!(
        KnowledgeCalibrationLlmConfig::from_env_map(&unknown_profile)
            .expect_err("unknown profile fails closed")
            .to_string()
            .contains(KNOWLEDGE_CALIBRATION_PROFILE_ID)
    );
    let mut low_reasoning = env.clone();
    low_reasoning.insert(
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_REASONING_EFFORT".to_string(),
        "medium".to_string(),
    );
    assert!(
        KnowledgeCalibrationLlmConfig::from_env_map(&low_reasoning)
            .expect_err("low reasoning fails closed")
            .to_string()
            .contains("high")
    );
    let mut simple_model = env;
    simple_model.insert(
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL_CAPABILITY".to_string(),
        "small".to_string(),
    );
    assert!(
        KnowledgeCalibrationLlmConfig::from_env_map(&simple_model)
            .expect_err("simple model capability fails closed")
            .to_string()
            .contains("complex")
    );
}

#[test]
fn knowledge_item_schema_migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let before = runtime_schema_migration_preflight(&conn).expect("preflight before schema");
    assert!(
        before["pending_migrations"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some(KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION)))
    );

    init_runtime_schema(&conn).expect("runtime schema");
    init_runtime_schema(&conn).expect("runtime schema idempotent");

    let after = runtime_schema_migration_preflight(&conn).expect("preflight after schema");
    assert_eq!(after["pending_migrations"], json!([]));
    let migration_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE migration_id = ?1",
            params![KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION],
            |row| row.get(0),
        )
        .expect("knowledge item migration count");
    assert_eq!(migration_count, 1);
    assert!(sqlite_table_exists(&conn, "knowledge_items").expect("knowledge items table"));
    assert!(
        sqlite_table_exists(&conn, "knowledge_item_state_history")
            .expect("knowledge item history table")
    );
    let columns = sqlite_table_columns(&conn, "knowledge_items").expect("columns");
    for column in [
        "item_id",
        "kind",
        "state",
        "source_refs_json",
        "evidence_refs_json",
        "payload_sha256",
        "schema_version",
        "state_version",
    ] {
        assert!(columns.contains(column), "missing column {column}");
    }
}

#[test]
fn knowledge_item_store_state_flow_records_history_and_audit() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let created = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Alias, "alias-flow"),
    )
    .expect("create knowledge item");
    assert!(created.item_id.starts_with("ki-alias-"));
    assert_eq!(created.kind, KnowledgeItemKind::Alias);
    assert_eq!(created.state, KnowledgeState::Candidate);
    assert_eq!(created.state_version, 1);
    assert_eq!(created.schema_version, KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION);
    assert!(!created.source_refs.is_empty());
    assert!(!created.evidence_refs.is_empty());

    let duplicate = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Alias, "alias-flow"),
    )
    .expect("duplicate create is idempotent");
    assert_eq!(duplicate.item_id, created.item_id);
    let updated = update_knowledge_item_state(
        &conn,
        &created.item_id,
        KnowledgeItemStateUpdateInput {
            new_state: KnowledgeState::SystemCalibrated,
            trace_id: "trace-knowledge-item-alias-flow".to_string(),
            actor: "runtime-policy".to_string(),
            reason: "rule and evidence judge passed".to_string(),
            evidence_refs: vec!["block://wikisource/alias-flow".to_string()],
            expected_state_version: created.state_version,
        },
    )
    .expect("state update")
    .expect("knowledge item exists");
    assert_eq!(updated.state, KnowledgeState::SystemCalibrated);
    assert_eq!(updated.state_version, 2);

    let listed = list_knowledge_items(
        &conn,
        KnowledgeItemListInput {
            kind: Some(KnowledgeItemKind::Alias),
            state: Some(KnowledgeState::SystemCalibrated),
            limit: 10,
            offset: 0,
        },
    )
    .expect("list knowledge items");
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].item_id, created.item_id);
    let history_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_item_state_history WHERE item_id = ?1",
            params![created.item_id],
            |row| row.get(0),
        )
        .expect("history count");
    assert_eq!(history_count, 2);
    let events = runtime_audit_events_for_trace(&conn, "trace-knowledge-item-alias-flow")
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "knowledge_item_created"
            && event["payload"]["state"] == json!("candidate")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "knowledge_item_state_updated"
            && event["payload"]["previous_state"] == json!("candidate")
            && event["payload"]["new_state"] == json!("system_calibrated")
            && event["payload"]["reason_sha256"].as_str().is_some()
    }));
    let stats = runtime_store_stats(&conn).expect("stats");
    assert_eq!(stats.knowledge_items, 1);
    assert_eq!(stats.knowledge_item_state_history, 2);
    assert_eq!(
        stats.knowledge_item_state.get("system_calibrated"),
        Some(&1_i64)
    );
    assert_eq!(stats.knowledge_item_kind.get("alias"), Some(&1_i64));
}

#[test]
fn knowledge_item_state_update_conflict_preserves_existing_state() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let created = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, "term-conflict"),
    )
    .expect("create knowledge item");

    let error = update_knowledge_item_state(
        &conn,
        &created.item_id,
        KnowledgeItemStateUpdateInput {
            new_state: KnowledgeState::SystemCalibrated,
            trace_id: "trace-knowledge-item-term-conflict".to_string(),
            actor: "runtime-policy".to_string(),
            reason: "stale state version".to_string(),
            evidence_refs: vec!["block://wikisource/term-conflict".to_string()],
            expected_state_version: created.state_version + 1,
        },
    )
    .expect_err("stale update must fail");
    assert!(error.to_string().contains("conflict"));
    let current = read_knowledge_item(&conn, &created.item_id)
        .expect("read knowledge item")
        .expect("knowledge item exists");
    assert_eq!(current.state, KnowledgeState::Candidate);
    assert_eq!(current.state_version, 1);
    let history_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_item_state_history WHERE item_id = ?1",
            params![created.item_id],
            |row| row.get(0),
        )
        .expect("history count");
    assert_eq!(history_count, 1);
}

#[test]
fn knowledge_item_state_update_rejects_direct_human_marked() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let created = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Alias, "direct-human-forbidden"),
    )
    .expect("create knowledge item");

    let error = update_knowledge_item_state(
        &conn,
        &created.item_id,
        KnowledgeItemStateUpdateInput {
            new_state: KnowledgeState::HumanMarked,
            trace_id: "trace-direct-human-forbidden".to_string(),
            actor: "manual-state-patch".to_string(),
            reason: "attempt to bypass human review action".to_string(),
            evidence_refs: vec!["block://wikisource/direct-human-forbidden".to_string()],
            expected_state_version: created.state_version,
        },
    )
    .expect_err("direct human_marked transition is rejected");
    assert!(error.to_string().contains("requires human review action"));
    let current = read_knowledge_item(&conn, &created.item_id)
        .expect("read knowledge item")
        .expect("knowledge item exists");
    assert_eq!(current.state, KnowledgeState::Candidate);
    assert_eq!(current.state_version, created.state_version);
}

#[test]
fn knowledge_item_human_review_accepts_with_task_and_is_idempotent() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let created = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, "human-review-accept"),
    )
    .expect("create knowledge item");
    let task = sample_knowledge_item_review_task(&conn, &created, "human-review-accept");

    let input = KnowledgeItemHumanReviewInput {
        task_id: task.task_id.clone(),
        decision: KnowledgeItemHumanReviewDecision::Accept,
        trace_id: task.trace_id.clone(),
        actor: "openwebui-admin-action".to_string(),
        reviewer: "admin-1".to_string(),
        review_note: "证据边界清楚，人工复核通过。".to_string(),
        evidence_ref: "source://review-note/human-review-accept".to_string(),
        expected_state_version: created.state_version,
        expected_task_updated_at: Some(task.updated_at.clone()),
    };
    let accepted = review_knowledge_item_human(&conn, &created.item_id, input.clone())
        .expect("human review succeeds")
        .expect("item exists");
    assert_eq!(accepted.object, "tonglingyu.knowledge_item_human_review");
    assert_eq!(accepted.decision, KnowledgeItemHumanReviewDecision::Accept);
    assert_eq!(accepted.item.state, KnowledgeState::HumanMarked);
    assert_eq!(accepted.task.status, "accepted");
    assert_eq!(accepted.task.reviewer.as_deref(), Some("admin-1"));
    assert_eq!(
        accepted.item.payload["human_review"]["target_state"],
        json!("human_marked")
    );
    assert_eq!(
        accepted.item.payload["human_review"]["kb_rebuild_required"],
        json!(true)
    );
    assert_eq!(accepted.item.state_version, created.state_version + 1);

    let repeated = review_knowledge_item_human(&conn, &created.item_id, input)
        .expect("human review retry is idempotent")
        .expect("item exists");
    assert_eq!(repeated.item.state_version, accepted.item.state_version);
    let history_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_item_state_history WHERE item_id = ?1",
            params![created.item_id],
            |row| row.get(0),
        )
        .expect("history count");
    assert_eq!(history_count, 2);
    let events = runtime_audit_events_for_trace(&conn, &task.trace_id).expect("audit events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event_type"] == "knowledge_item_human_reviewed")
            .count(),
        1
    );
    assert!(events.iter().any(|event| {
        event["event_type"] == "knowledge_item_human_reviewed"
            && event["payload"]["review_note_sha256"].as_str().is_some()
            && event["payload"]["kb_rebuild_required"] == json!(true)
            && event["payload"].get("review_note").is_none()
    }));
}

#[test]
fn knowledge_item_human_review_rejects_and_conflicts_are_atomic() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let stale = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::VersionNote, "human-review-conflict"),
    )
    .expect("create stale item");
    let stale_task = sample_knowledge_item_review_task(&conn, &stale, "human-review-conflict");
    let conflict = review_knowledge_item_human(
        &conn,
        &stale.item_id,
        KnowledgeItemHumanReviewInput {
            task_id: stale_task.task_id.clone(),
            decision: KnowledgeItemHumanReviewDecision::Reject,
            trace_id: stale_task.trace_id.clone(),
            actor: "openwebui-admin-action".to_string(),
            reviewer: "admin-1".to_string(),
            review_note: "证据不足，暂不使用。".to_string(),
            evidence_ref: "source://review-note/human-review-conflict".to_string(),
            expected_state_version: stale.state_version + 1,
            expected_task_updated_at: Some(stale_task.updated_at.clone()),
        },
    )
    .expect_err("stale state version conflicts");
    assert!(conflict.to_string().contains("conflict"));
    let unchanged = read_knowledge_item(&conn, &stale.item_id)
        .expect("read stale item")
        .expect("item exists");
    assert_eq!(unchanged.state, KnowledgeState::Candidate);
    let unchanged_task = load_governance_task(&conn, &stale_task.task_id)
        .expect("read stale task")
        .expect("task exists");
    assert_eq!(unchanged_task.status, "open");

    let rejected = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, "human-review-reject"),
    )
    .expect("create rejected item");
    let rejected_task = sample_knowledge_item_review_task(&conn, &rejected, "human-review-reject");
    let result = review_knowledge_item_human(
        &conn,
        &rejected.item_id,
        KnowledgeItemHumanReviewInput {
            task_id: rejected_task.task_id.clone(),
            decision: KnowledgeItemHumanReviewDecision::Reject,
            trace_id: rejected_task.trace_id.clone(),
            actor: "openwebui-admin-action".to_string(),
            reviewer: "admin-1".to_string(),
            review_note: "证据不足，人工否决。".to_string(),
            evidence_ref: "source://review-note/human-review-reject".to_string(),
            expected_state_version: rejected.state_version,
            expected_task_updated_at: Some(rejected_task.updated_at.clone()),
        },
    )
    .expect("human reject succeeds")
    .expect("item exists");
    assert_eq!(result.item.state, KnowledgeState::Rejected);
    assert_eq!(result.task.status, "rejected");
    let package = create_evidence_package(
        &conn,
        "trace-human-review-reject-package",
        "请说明文本证据",
        vec![runtime_policy_test_card("human-review-reject")],
    )
    .expect("evidence package");
    assert_eq!(
        package.knowledge_state_summary.rejected_or_deprecated_count,
        1
    );
    assert_eq!(package.knowledge_state_summary.runtime_usable_count, 0);
    assert!(
        package
            .claim_evidence_map
            .iter()
            .all(|claim| claim.knowledge_item_refs.is_empty())
    );
}

#[test]
fn knowledge_item_list_paginates_all_kinds_and_keeps_rejected_history() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let kinds = [
        KnowledgeItemKind::Alias,
        KnowledgeItemKind::Term,
        KnowledgeItemKind::CommentaryLink,
        KnowledgeItemKind::VersionNote,
        KnowledgeItemKind::Person,
        KnowledgeItemKind::Relationship,
        KnowledgeItemKind::Event,
        KnowledgeItemKind::Poem,
        KnowledgeItemKind::EvaluationCase,
    ];
    let mut first_item_id = None;
    for index in 0..105 {
        let kind = kinds[index % kinds.len()];
        let item = create_knowledge_item(
            &conn,
            sample_knowledge_item_input(kind, &format!("item-{index:03}")),
        )
        .expect("create knowledge item");
        if index == 0 {
            first_item_id = Some(item.item_id);
        }
    }

    let first_page = list_knowledge_items(
        &conn,
        KnowledgeItemListInput {
            kind: None,
            state: Some(KnowledgeState::Candidate),
            limit: 0,
            offset: 0,
        },
    )
    .expect("list first page");
    assert_eq!(first_page.limit, KNOWLEDGE_ITEM_DEFAULT_PAGE_SIZE);
    assert_eq!(first_page.items.len(), KNOWLEDGE_ITEM_DEFAULT_PAGE_SIZE);
    assert_eq!(
        first_page.next_offset,
        Some(KNOWLEDGE_ITEM_DEFAULT_PAGE_SIZE)
    );
    let capped_page = list_knowledge_items(
        &conn,
        KnowledgeItemListInput {
            kind: None,
            state: Some(KnowledgeState::Candidate),
            limit: 500,
            offset: 0,
        },
    )
    .expect("list capped page");
    assert_eq!(capped_page.limit, KNOWLEDGE_ITEM_MAX_PAGE_SIZE);
    assert_eq!(capped_page.items.len(), KNOWLEDGE_ITEM_MAX_PAGE_SIZE);

    let first_item_id = first_item_id.expect("first item id");
    let rejected = update_knowledge_item_state(
        &conn,
        &first_item_id,
        KnowledgeItemStateUpdateInput {
            new_state: KnowledgeState::Rejected,
            trace_id: "trace-knowledge-item-item-000".to_string(),
            actor: "reviewer".to_string(),
            reason: "source boundary unclear".to_string(),
            evidence_refs: vec!["block://wikisource/item-000".to_string()],
            expected_state_version: 1,
        },
    )
    .expect("reject item")
    .expect("item exists");
    assert_eq!(rejected.state, KnowledgeState::Rejected);
    let rejected_list = list_knowledge_items(
        &conn,
        KnowledgeItemListInput {
            kind: None,
            state: Some(KnowledgeState::Rejected),
            limit: 10,
            offset: 0,
        },
    )
    .expect("list rejected");
    assert_eq!(rejected_list.items.len(), 1);
    assert_eq!(rejected_list.items[0].item_id, first_item_id);
    let rejected_history_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_item_state_history WHERE item_id = ?1",
            params![first_item_id],
            |row| row.get(0),
        )
        .expect("rejected history count");
    assert_eq!(rejected_history_count, 2);
}

#[test]
fn knowledge_calibration_rule_path_covers_all_kinds_without_runtime_promotion() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let alias_count_before = table_count(&conn, "aliases").expect("alias count before");

    for (index, kind) in all_knowledge_item_kinds().into_iter().enumerate() {
        let marker = format!("rule-kind-{index:02}");
        let item = create_knowledge_item(&conn, sample_knowledge_item_input(kind, &marker))
            .expect("create candidate item");
        let report = run_knowledge_calibration_offline(
            &conn,
            calibration_run_input(&item, KnowledgeCalibrationMethod::Rule),
        )
        .expect("rule calibration runs");
        assert_eq!(report.kind, kind);
        assert_eq!(report.method, KnowledgeCalibrationMethod::Rule);
        assert_eq!(
            report.decision,
            KnowledgeCalibrationDecision::SystemCalibrated
        );
        assert!(report.report_ref.contains(&report.report_id));
        assert!(report.source_boundary.is_object());
        assert!(!report.evidence_refs.is_empty());
        assert_eq!(
            report.coverage_matrix["runtime_usable_auto_promotion"],
            json!(false)
        );
        assert_eq!(
            report.coverage_matrix["runtime_policy_rejected"],
            json!(true)
        );
        let updated = read_knowledge_item(&conn, &item.item_id)
            .expect("read item")
            .expect("item exists");
        assert_eq!(updated.state, KnowledgeState::SystemCalibrated);
        assert_ne!(updated.state, KnowledgeState::RuntimeUsable);
        assert_eq!(updated.calibration_report_ref, Some(report.report_ref));
        assert!(updated.source_boundary.is_some());
        assert!(updated.confidence.is_some_and(|value| value >= 0.8));
    }

    let stats = runtime_store_stats(&conn).expect("stats");
    assert_eq!(stats.knowledge_calibration_reports, 9);
    assert_eq!(
        stats
            .knowledge_calibration_report_decision
            .get("system_calibrated"),
        Some(&9)
    );
    assert_eq!(
        stats.knowledge_calibration_report_method.get("rule"),
        Some(&9)
    );
    let alias_count_after = table_count(&conn, "aliases").expect("alias count after");
    assert_eq!(
        alias_count_after, alias_count_before,
        "calibration must not mutate fact-layer aliases"
    );
}

#[test]
fn system_calibrated_and_rejected_items_are_not_runtime_evidence() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let marker = "runtime-policy-blocked";
    let item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, marker),
    )
    .expect("create candidate item");
    let report = run_knowledge_calibration_offline(
        &conn,
        calibration_run_input(&item, KnowledgeCalibrationMethod::Rule),
    )
    .expect("rule calibration");
    assert_eq!(
        report.decision,
        KnowledgeCalibrationDecision::SystemCalibrated
    );

    let mut rejected_input =
        sample_knowledge_item_input(KnowledgeItemKind::VersionNote, "runtime-policy-blocked-2");
    rejected_input.evidence_refs = vec![format!("block://wikisource/{marker}")];
    let rejected_item =
        create_knowledge_item(&conn, rejected_input).expect("create rejected candidate item");
    update_knowledge_item_state(
        &conn,
        &rejected_item.item_id,
        KnowledgeItemStateUpdateInput {
            new_state: KnowledgeState::Rejected,
            trace_id: "trace-runtime-policy-blocked-item".to_string(),
            actor: "runtime-policy-test".to_string(),
            reason: "blocked item must not enter runtime evidence".to_string(),
            evidence_refs: vec![format!("block://wikisource/{marker}")],
            expected_state_version: rejected_item.state_version,
        },
    )
    .expect("reject item")
    .expect("rejected item exists");

    let package = create_evidence_package(
        &conn,
        "trace-runtime-policy-blocked",
        "请说明文本证据",
        vec![runtime_policy_test_card(marker)],
    )
    .expect("evidence package");

    assert_eq!(
        package
            .knowledge_state_summary
            .system_calibrated_rejected_count,
        1
    );
    assert_eq!(
        package.knowledge_state_summary.rejected_or_deprecated_count,
        1
    );
    assert_eq!(package.knowledge_state_summary.runtime_usable_count, 0);
    assert_eq!(package.knowledge_state_summary.human_marked_count, 0);
    assert!(
        package
            .claim_evidence_map
            .iter()
            .all(|claim| claim.knowledge_item_refs.is_empty())
    );
    assert_eq!(package.review.status, "needs_revision");
    assert!(
        package
            .review
            .issues
            .iter()
            .any(|issue| issue.contains("runtime_usable"))
    );
    let link_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM evidence_claim_knowledge_links WHERE package_id = ?1",
            params![&package.package_id],
            |row| row.get(0),
        )
        .expect("knowledge link count");
    assert_eq!(link_count, 0);

    let answer = local_answer("请说明文本证据", &package);
    assert!(!answer.contains("人工标记"));
    assert!(!answer.contains("基于当前已登记资料"));
    let rendered_public =
        serde_json::to_string(&package_json(&package)).expect("package json serializes");
    assert!(!rendered_public.contains(&item.item_id));
    assert!(!rendered_public.contains(&rejected_item.item_id));
    assert!(!rendered_public.contains(&report.report_ref));
    assert!(!rendered_public.contains("knowledge_item_refs"));
    assert!(!rendered_public.contains("system_calibrated"));
    assert!(!rendered_public.contains("runtime_usable"));
    assert!(!rendered_public.contains("rejected"));
}

#[test]
fn runtime_usable_requires_explicit_promotion_and_records_claim_links() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let marker = "runtime-policy-promoted";
    let item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, marker),
    )
    .expect("create candidate item");
    let report = run_knowledge_calibration_offline(
        &conn,
        calibration_run_input(&item, KnowledgeCalibrationMethod::Rule),
    )
    .expect("rule calibration");
    let calibrated = read_knowledge_item(&conn, &item.item_id)
        .expect("read calibrated item")
        .expect("calibrated item exists");
    assert_eq!(calibrated.state, KnowledgeState::SystemCalibrated);

    let promoted = promote_knowledge_item_runtime_usable(
        &conn,
        &item.item_id,
        KnowledgeRuntimePromotionInput {
            trace_id: "trace-runtime-policy-promoted".to_string(),
            actor: "release-manager".to_string(),
            reason: "release gate accepted calibrated evidence".to_string(),
            release_run_id: "release-runtime-policy-promoted".to_string(),
            expires_at: Some("2999-01-01T00:00:00Z".to_string()),
            expected_state_version: calibrated.state_version,
        },
    )
    .expect("promote runtime usable")
    .expect("promoted item exists");
    assert_eq!(promoted.state, KnowledgeState::RuntimeUsable);
    assert_eq!(
        promoted.payload["runtime_policy"]["policy_version"],
        json!(KNOWLEDGE_RUNTIME_POLICY_VERSION)
    );

    let card = runtime_policy_test_card(marker);
    insert_test_source_hash(
        &conn,
        &card.source_id,
        "source-hash-runtime-policy-promoted",
    );
    let package = create_evidence_package(
        &conn,
        "trace-runtime-policy-promoted",
        "请说明文本证据",
        vec![card],
    )
    .expect("evidence package");

    assert_eq!(package.review.status, "passed");
    assert_eq!(package.knowledge_state_summary.selected_count, 1);
    assert_eq!(package.knowledge_state_summary.runtime_usable_count, 1);
    assert_eq!(package.knowledge_state_summary.human_marked_count, 0);
    assert_eq!(
        package.knowledge_state_summary.safe_public_label.as_deref(),
        Some("基于当前已登记资料")
    );
    let knowledge_ref = package
        .claim_evidence_map
        .iter()
        .flat_map(|claim| claim.knowledge_item_refs.iter())
        .next()
        .expect("claim knowledge ref");
    assert_eq!(knowledge_ref.item_id, item.item_id);
    assert_eq!(knowledge_ref.state, KnowledgeState::RuntimeUsable);
    assert_eq!(
        knowledge_ref.evidence_ref,
        format!("ev-runtime-policy-{marker}")
    );
    assert_eq!(
        knowledge_ref.policy_version,
        KNOWLEDGE_RUNTIME_POLICY_VERSION
    );
    assert_eq!(
        knowledge_ref.calibration_report_ref.as_deref(),
        Some(report.report_ref.as_str())
    );
    let stored = load_evidence_package_from_conn(&conn, &package.package_id)
        .expect("load package")
        .expect("package exists");
    assert_eq!(stored.knowledge_state_summary.runtime_usable_count, 1);
    let link_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM evidence_claim_knowledge_links WHERE package_id = ?1",
            params![&package.package_id],
            |row| row.get(0),
        )
        .expect("knowledge link count");
    assert_eq!(link_count, 1);

    let answer = local_answer("请说明文本证据", &package);
    assert!(answer.contains("基于当前已登记资料"));
    assert!(!answer.contains("人工标记"));
    let rendered_public =
        serde_json::to_string(&package_json(&package)).expect("package json serializes");
    assert!(rendered_public.contains("基于当前已登记资料"));
    assert!(!rendered_public.contains(&item.item_id));
    assert!(!rendered_public.contains(&report.report_ref));
    assert!(!rendered_public.contains("knowledge_item_refs"));
    assert!(!rendered_public.contains("runtime_usable"));
    assert!(!rendered_public.contains("system_calibrated"));
}

#[test]
fn human_marked_is_the_only_state_that_gets_human_label() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let marker = "runtime-policy-human-marked";
    let item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, marker),
    )
    .expect("create candidate item");
    let report = run_knowledge_calibration_offline(
        &conn,
        calibration_run_input(&item, KnowledgeCalibrationMethod::Rule),
    )
    .expect("rule calibration");
    let calibrated = read_knowledge_item(&conn, &item.item_id)
        .expect("read calibrated item")
        .expect("calibrated item exists");
    let review_task = sample_knowledge_item_review_task(&conn, &calibrated, marker);
    let human_marked = review_knowledge_item_human(
        &conn,
        &item.item_id,
        KnowledgeItemHumanReviewInput {
            task_id: review_task.task_id.clone(),
            decision: KnowledgeItemHumanReviewDecision::Accept,
            trace_id: review_task.trace_id.clone(),
            actor: "human-reviewer".to_string(),
            reviewer: "reviewer-1".to_string(),
            review_note: "human reviewer accepted this knowledge item".to_string(),
            evidence_ref: format!("block://wikisource/{marker}"),
            expected_state_version: calibrated.state_version,
            expected_task_updated_at: Some(review_task.updated_at),
        },
    )
    .expect("mark item human")
    .expect("human item exists")
    .item;
    assert_eq!(human_marked.state, KnowledgeState::HumanMarked);

    let card = runtime_policy_test_card(marker);
    insert_test_source_hash(
        &conn,
        &card.source_id,
        "source-hash-runtime-policy-human-marked",
    );
    let package = create_evidence_package(
        &conn,
        "trace-runtime-policy-human-marked",
        "请说明文本证据",
        vec![card],
    )
    .expect("evidence package");

    assert_eq!(package.review.status, "passed");
    assert_eq!(package.knowledge_state_summary.selected_count, 1);
    assert_eq!(package.knowledge_state_summary.runtime_usable_count, 0);
    assert_eq!(package.knowledge_state_summary.human_marked_count, 1);
    assert_eq!(
        package.knowledge_state_summary.safe_public_label.as_deref(),
        Some("人工标记")
    );
    let knowledge_ref = package
        .claim_evidence_map
        .iter()
        .flat_map(|claim| claim.knowledge_item_refs.iter())
        .next()
        .expect("claim knowledge ref");
    assert_eq!(knowledge_ref.state, KnowledgeState::HumanMarked);
    assert_eq!(knowledge_ref.display_label.as_deref(), Some("人工标记"));
    assert_eq!(
        knowledge_ref.calibration_report_ref.as_deref(),
        Some(report.report_ref.as_str())
    );

    let answer = local_answer("请说明文本证据", &package);
    assert!(answer.contains("人工标记资料显示"));
    let rendered_public =
        serde_json::to_string(&package_json(&package)).expect("package json serializes");
    assert!(rendered_public.contains("人工标记"));
    assert!(!rendered_public.contains(&item.item_id));
    assert!(!rendered_public.contains(&report.report_ref));
    assert!(!rendered_public.contains("human_marked"));
    assert!(!rendered_public.contains("system_calibrated"));
}

#[test]
fn knowledge_calibration_rule_failure_keeps_candidate_and_records_issue() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Term, "rule-fail"),
    )
    .expect("create candidate");
    let mut input = calibration_run_input(&item, KnowledgeCalibrationMethod::Rule);
    input.rule_context = Some(KnowledgeCalibrationRuleContext {
        exact_terms: vec!["missing-term".to_string()],
        ..sample_rule_context("rule-fail")
    });
    let report = run_knowledge_calibration_offline(&conn, input).expect("rule calibration");
    assert_eq!(report.decision, KnowledgeCalibrationDecision::KeepCandidate);
    assert!(
        report
            .quality_issues
            .iter()
            .any(|issue| issue.starts_with("exact_term_not_in_payload:"))
    );
    let current = read_knowledge_item(&conn, &item.item_id)
        .expect("read item")
        .expect("item exists");
    assert_eq!(current.state, KnowledgeState::Candidate);
    assert!(current.calibration_report_ref.is_none());
    let events =
        runtime_audit_events_for_trace(&conn, "trace-calibration-rule-fail").expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "knowledge_calibration_candidate_kept"
            && event["payload"]["decision"] == json!("keep_candidate")
    }));
}

#[test]
fn knowledge_calibration_eval_and_rqa_failures_do_not_pollute_runtime_usable() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let eval_item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::EvaluationCase, "eval-forbidden"),
    )
    .expect("create eval item");
    let mut eval_input = calibration_run_input(&eval_item, KnowledgeCalibrationMethod::Eval);
    eval_input.input_kind = KnowledgeCalibrationInputKind::EvalMiss;
    eval_input.input_ref = "eval://forbidden-conclusion".to_string();
    eval_input.eval_context = Some(KnowledgeCalibrationEvalContext {
        expected_evidence_hit: true,
        forbidden_conclusion_hit: true,
        reviewer_status: "needs_revision".to_string(),
        source_boundary_confirmed: true,
    });
    let eval_report =
        run_knowledge_calibration_offline(&conn, eval_input).expect("eval calibration");
    assert_eq!(eval_report.decision, KnowledgeCalibrationDecision::Rejected);
    let eval_current = read_knowledge_item(&conn, &eval_item.item_id)
        .expect("read eval item")
        .expect("eval item exists");
    assert_eq!(eval_current.state, KnowledgeState::Rejected);

    let rqa_item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::CommentaryLink, "rqa-blocking"),
    )
    .expect("create rqa item");
    let mut rqa_input = calibration_run_input(&rqa_item, KnowledgeCalibrationMethod::Rqa);
    rqa_input.input_kind = KnowledgeCalibrationInputKind::RetrievalFailure;
    rqa_input.input_ref = "retrieval-failure://rf-1".to_string();
    rqa_input.rqa_context = Some(KnowledgeCalibrationRqaContext {
        retrieval_quality_issues: vec!["missing_required_evidence_type:commentary".to_string()],
        blocking_quality_issues: vec!["missing_required_evidence_type:commentary".to_string()],
        failure_cluster_refs: vec!["rqa-cluster://commentary-miss".to_string()],
        governance_task_refs: vec!["governance-task://kgt-1".to_string()],
        proposed_fix_refs: vec!["proposal://commentary-link-fix".to_string()],
        rqa_report_refs: vec!["rqa-report://trace-1".to_string()],
    });
    let rqa_report = run_knowledge_calibration_offline(&conn, rqa_input).expect("rqa calibration");
    assert_eq!(
        rqa_report.decision,
        KnowledgeCalibrationDecision::KeepCandidate
    );
    assert_eq!(rqa_report.report["input_kind"], json!("retrieval_failure"));
    assert_eq!(
        rqa_report.report["source_boundary"]["rqa_report_refs"],
        json!(["rqa-report://trace-1"])
    );
    let rqa_current = read_knowledge_item(&conn, &rqa_item.item_id)
        .expect("read rqa item")
        .expect("rqa item exists");
    assert_eq!(rqa_current.state, KnowledgeState::Candidate);
    assert_ne!(rqa_current.state, KnowledgeState::RuntimeUsable);
}

#[tokio::test]
async fn knowledge_calibration_llm_fake_output_is_report_only_and_privacy_checked() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let config = valid_llm_config();
    let item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Alias, "llm-judge"),
    )
    .expect("create candidate");
    let judgement = execute_knowledge_calibration_llm_evidence_judge(
        Arc::new(CalibrationJudgeRuntimeClient),
        &item,
        &config,
        "trace-calibration-llm-judge",
    )
    .await
    .expect("LLM judge runs through runtime client");
    assert_eq!(
        judgement.decision,
        KnowledgeCalibrationDecision::SystemCalibrated
    );
    let mut input = calibration_run_input(&item, KnowledgeCalibrationMethod::LlmEvidenceJudge);
    input.llm_config = Some(config.clone());
    input.llm_judgement = Some(judgement);
    let aliases_before = table_count(&conn, "aliases").expect("aliases before");
    let report = run_knowledge_calibration_offline(&conn, input).expect("LLM calibration");
    assert_eq!(
        report.decision,
        KnowledgeCalibrationDecision::SystemCalibrated
    );
    assert_eq!(
        report
            .config_summary
            .as_ref()
            .and_then(|value| value.get("config_digest"))
            .and_then(Value::as_str),
        Some(config.config_digest.as_str())
    );
    assert_eq!(report.report["secret_values_stored"], json!(false));
    assert_eq!(
        report.report["fact_layer_mutated"],
        json!(false),
        "LLM judge cannot write fact layer"
    );
    let aliases_after = table_count(&conn, "aliases").expect("aliases after");
    assert_eq!(aliases_after, aliases_before);

    let private_output = serde_json::to_string(&json!({
        "llm_evidence_judge": {
            "decision": "system_calibrated",
            "confidence": 0.9,
            "evidence_refs": ["block://private"],
            "source_boundary": {"source_id": "wikisource"},
            "quality_issues": [],
            "raw_question": "不要保存的问题原文"
        }
    }))
    .expect("private output serializes");
    assert!(
        parse_knowledge_calibration_llm_judge_output(&private_output)
            .expect_err("raw question must be rejected")
            .to_string()
            .contains("raw_question")
    );

    let missing_config_item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Alias, "llm-missing-config"),
    )
    .expect("create missing config candidate");
    let missing_config_input = calibration_run_input(
        &missing_config_item,
        KnowledgeCalibrationMethod::LlmEvidenceJudge,
    );
    assert!(
        run_knowledge_calibration_offline(&conn, missing_config_input)
            .expect_err("missing LLM config fails closed")
            .to_string()
            .contains("configured LLM")
    );
    let missing_config_current = read_knowledge_item(&conn, &missing_config_item.item_id)
        .expect("read missing config item")
        .expect("missing config item exists");
    assert_eq!(missing_config_current.state, KnowledgeState::Candidate);
}

#[test]
fn knowledge_calibration_job_model_is_idempotent_leased_and_audited() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::VersionNote, "job-flow"),
    )
    .expect("create candidate");
    let job_input = KnowledgeCalibrationJobCreateInput {
        input_kind: KnowledgeCalibrationInputKind::GovernanceTask,
        input_ref: "governance-task://job-flow".to_string(),
        item_id: item.item_id.clone(),
        method: KnowledgeCalibrationMethod::Rule,
        trace_id: "trace-calibration-job-flow".to_string(),
        idempotency_key: "job-flow-idempotency".to_string(),
        config_digest: Some("c".repeat(64)),
        retry_limit: 2,
        concurrency_key: "version-note:job-flow".to_string(),
    };
    let job = create_knowledge_calibration_job(&conn, job_input.clone()).expect("create job");
    let duplicate =
        create_knowledge_calibration_job(&conn, job_input).expect("duplicate job idempotent");
    assert_eq!(duplicate.job_id, job.job_id);
    assert_eq!(job.status, KnowledgeCalibrationJobStatus::Queued);
    assert_eq!(job.attempt_count, 0);

    let leased = lease_knowledge_calibration_job(&conn, &job.job_id, "worker-1", 60)
        .expect("lease job")
        .expect("job exists");
    assert_eq!(leased.status, KnowledgeCalibrationJobStatus::Running);
    assert_eq!(leased.attempt_count, 1);
    assert!(leased.lease_expires_at.is_some());

    let second_item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::VersionNote, "job-conflict"),
    )
    .expect("create second candidate");
    let second_job = create_knowledge_calibration_job(
        &conn,
        KnowledgeCalibrationJobCreateInput {
            input_kind: KnowledgeCalibrationInputKind::GovernanceTask,
            input_ref: "governance-task://job-conflict".to_string(),
            item_id: second_item.item_id,
            method: KnowledgeCalibrationMethod::Rule,
            trace_id: "trace-calibration-job-conflict".to_string(),
            idempotency_key: "job-conflict-idempotency".to_string(),
            config_digest: None,
            retry_limit: 2,
            concurrency_key: "version-note:job-flow".to_string(),
        },
    )
    .expect("create conflicting job");
    assert!(
        lease_knowledge_calibration_job(&conn, &second_job.job_id, "worker-2", 60)
            .expect_err("concurrency limit rejects second lease")
            .to_string()
            .contains("concurrency limit")
    );

    let heartbeat = heartbeat_knowledge_calibration_job(&conn, &job.job_id, "worker-1")
        .expect("heartbeat")
        .expect("job exists");
    assert!(heartbeat.heartbeat_at.is_some());

    let report = run_knowledge_calibration_offline(
        &conn,
        calibration_run_input(&item, KnowledgeCalibrationMethod::Rule),
    )
    .expect("calibration report");
    let completed =
        complete_knowledge_calibration_job(&conn, &job.job_id, "worker-1", &report.report_id)
            .expect("complete job")
            .expect("job exists");
    assert_eq!(completed.status, KnowledgeCalibrationJobStatus::Succeeded);
    assert_eq!(completed.report_id, Some(report.report_id));

    let retry_item = create_knowledge_item(
        &conn,
        sample_knowledge_item_input(KnowledgeItemKind::Poem, "job-retry"),
    )
    .expect("create retry candidate");
    let retry_job = create_knowledge_calibration_job(
        &conn,
        KnowledgeCalibrationJobCreateInput {
            input_kind: KnowledgeCalibrationInputKind::RetrievalFailure,
            input_ref: "retrieval-failure://job-retry".to_string(),
            item_id: retry_item.item_id,
            method: KnowledgeCalibrationMethod::Rqa,
            trace_id: "trace-calibration-job-retry".to_string(),
            idempotency_key: "job-retry-idempotency".to_string(),
            config_digest: None,
            retry_limit: 2,
            concurrency_key: "poem:job-retry".to_string(),
        },
    )
    .expect("retry job");
    let retry_leased = lease_knowledge_calibration_job(&conn, &retry_job.job_id, "worker-3", 60)
        .expect("lease retry job")
        .expect("retry job exists");
    let retry_waiting = fail_knowledge_calibration_job(
        &conn,
        &retry_leased.job_id,
        "worker-3",
        "temporary RQA dependency unavailable",
        true,
    )
    .expect("fail retry job")
    .expect("retry job exists");
    assert_eq!(
        retry_waiting.status,
        KnowledgeCalibrationJobStatus::RetryWaiting
    );

    let stats = runtime_store_stats(&conn).expect("stats");
    assert_eq!(stats.knowledge_calibration_jobs, 3);
    assert!(stats.knowledge_calibration_job_history >= 7);
    assert_eq!(
        stats.knowledge_calibration_job_status.get("succeeded"),
        Some(&1)
    );
    assert_eq!(
        stats.knowledge_calibration_job_status.get("retry_waiting"),
        Some(&1)
    );
    let events = runtime_audit_events_for_trace(&conn, "trace-calibration-job-flow")
        .expect("job audit events");
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "knowledge_calibration_job_created")
    );
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "knowledge_calibration_job_completed")
    );
}

#[test]
fn workflow_records_retrieval_failure_with_admin_and_safe_views() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "source_of_record": "raw MediaWiki wikitext plus revision metadata",
        }),
    );
    let question = "通灵玉 password=SECRET_RUNTIME_TOKEN_01234567890123456789";

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input(
            "trace-retrieval-failure-test",
            question,
            2,
            vec!["base_text".to_string()],
        ),
    )
    .expect("workflow executes");

    let list = list_retrieval_failures(
        &conn,
        RetrievalFailureListInput {
            human_review_status: Some("open".to_string()),
            failure_type: None,
            limit: 10,
            offset: 0,
            view: RetrievalFailureView::AdminDetail,
        },
    )
    .expect("list failures");
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(
        item["failure_type"],
        json!("source_usage_metadata_incomplete")
    );
    assert_eq!(item["trace_id"], json!("trace-retrieval-failure-test"));
    assert_eq!(item["package_id"], json!(workflow.package.package_id));
    assert_eq!(item["human_review_status"], json!("open"));
    assert!(item["question_sha256"].as_str().is_some());
    assert!(item["question_summary"].as_str().is_some());
    assert!(item["redacted_question_excerpt"].as_str().is_some());
    assert!(
        item["redacted_query_terms"]
            .as_array()
            .is_some_and(|terms| {
                terms.iter().any(|term| {
                    term.as_str()
                        .is_some_and(|term| term.starts_with("sha256:"))
                })
            })
    );
    assert!(item["selected_evidence_ids"].as_array().is_some_and(|ids| {
        ids.iter()
            .any(|id| id.as_str().is_some_and(|id| id.starts_with("ev-")))
    }));
    let admin_json = serde_json::to_string(item).expect("admin serializes");
    assert!(!admin_json.contains(question));
    assert!(!admin_json.contains("SECRET_RUNTIME_TOKEN"));
    assert!(!admin_json.contains("password="));

    let failure_id = item["failure_id"].as_str().expect("failure id");
    let updated = update_retrieval_failure_status(
        &conn,
        failure_id,
        "resolved",
        Some("rqa-reviewer"),
        Some("source metadata follow-up recorded"),
    )
    .expect("update failure")
    .expect("failure exists");
    assert_eq!(updated.human_review_status, "resolved");
    assert!(updated.resolved_at.is_some());

    let safe = read_retrieval_failure(&conn, failure_id, RetrievalFailureView::SafeSummary)
        .expect("read safe failure")
        .expect("failure exists");
    assert_eq!(safe["view"], json!("safe_summary"));
    assert!(safe.get("trace_id").is_none());
    assert!(safe.get("package_id").is_none());
    assert!(safe.get("selected_evidence_ids").is_none());
    assert!(safe["redacted_question_excerpt"].as_str().is_some());
    assert_eq!(safe["quality_issue_count"], json!(1));

    let stats = runtime_store_stats(&conn).expect("stats");
    assert_eq!(stats.retrieval_failures, 1);
    assert_eq!(stats.governance_tasks, 1);
    assert_eq!(stats.retrieval_failure_status.get("resolved"), Some(&1_i64));
    assert_eq!(stats.governance_task_status.get("open"), Some(&1_i64));
    let events = runtime_audit_events_for_trace(&conn, "trace-retrieval-failure-test")
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "retrieval_failure_recorded"
            && event["payload"]["failure_type"] == json!("source_usage_metadata_incomplete")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "retrieval_failure_status_updated"
            && event["payload"]["review_note_sha256"].as_str().is_some()
    }));
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "governance_task_created")
    );
    let governance_tasks = list_governance_tasks(
        &conn,
        KnowledgeGovernanceTaskListInput {
            status: Some("open".to_string()),
            task_type: Some("source_metadata_fix".to_string()),
            priority: Some("p0".to_string()),
            source_failure_id: Some(failure_id.to_string()),
            source_entity_type: None,
            source_entity_id: None,
            limit: 10,
            offset: 0,
        },
    )
    .expect("list governance tasks");
    assert_eq!(governance_tasks.items.len(), 1);
    assert_eq!(
        governance_tasks.items[0]["source_failure_id"],
        json!(failure_id)
    );
}

#[test]
fn retrieval_failure_records_expected_evidence_miss_and_dedupes() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    let question = "通灵玉是什么？";
    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: question.to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");
    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    assert!(quality_report.production_ready);
    let selected_evidence_ids = evidence_ids(&cards);

    let first = create_retrieval_failure(
        &conn,
        RetrievalFailureCreateInput {
            trace_id: "trace-expected-evidence-test".to_string(),
            package_id: Some("pkg-expected-evidence-test".to_string()),
            question: question.to_string(),
            quality_report: (*quality_report).clone(),
            selected_evidence_ids: selected_evidence_ids.clone(),
            expected_evidence_ids: vec!["ev-expected-missing".to_string()],
            agent_diagnosis: None,
            proposed_fix: None,
        },
    )
    .expect("expected evidence failure records");
    let second = create_retrieval_failure(
        &conn,
        RetrievalFailureCreateInput {
            trace_id: "trace-expected-evidence-test".to_string(),
            package_id: Some("pkg-expected-evidence-test".to_string()),
            question: question.to_string(),
            quality_report: (*quality_report).clone(),
            selected_evidence_ids,
            expected_evidence_ids: vec!["ev-expected-missing".to_string()],
            agent_diagnosis: None,
            proposed_fix: None,
        },
    )
    .expect("deduped expected evidence failure returns existing record");

    assert_eq!(first.failure_id, second.failure_id);
    assert_eq!(first.failure_type, "expected_evidence_missing");
    assert!(
        first
            .quality_issues
            .iter()
            .any(|issue| { issue == "expected_evidence_missing:ev-expected-missing" })
    );
    let list = list_retrieval_failures(
        &conn,
        RetrievalFailureListInput {
            human_review_status: None,
            failure_type: Some("expected_evidence_missing".to_string()),
            limit: 10,
            offset: 0,
            view: RetrievalFailureView::AdminDetail,
        },
    )
    .expect("list expected evidence failures");
    assert_eq!(list.items.len(), 1);
    let events = runtime_audit_events_for_trace(&conn, "trace-expected-evidence-test")
        .expect("audit events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event_type"] == "retrieval_failure_recorded")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event_type"] == "governance_task_created")
            .count(),
        1
    );
    let governance_tasks = list_governance_tasks(
        &conn,
        KnowledgeGovernanceTaskListInput {
            status: Some("open".to_string()),
            task_type: Some("expected_evidence_fix".to_string()),
            priority: Some("p0".to_string()),
            source_failure_id: Some(first.failure_id.clone()),
            source_entity_type: None,
            source_entity_id: None,
            limit: 10,
            offset: 0,
        },
    )
    .expect("list governance tasks");
    assert_eq!(governance_tasks.items.len(), 1);
    assert_eq!(
        governance_tasks.items[0]["task_type"],
        json!("expected_evidence_fix")
    );
}

#[test]
fn retrieval_failure_cluster_creates_proposed_fix_task_without_fact_mutation() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    let question = "通灵玉是什么？";
    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: question.to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");
    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    let selected_evidence_ids = evidence_ids(&cards);
    for index in 1..=2 {
        create_retrieval_failure(
            &conn,
            RetrievalFailureCreateInput {
                trace_id: format!("trace-cluster-{index}"),
                package_id: Some(format!("pkg-cluster-{index}")),
                question: question.to_string(),
                quality_report: (*quality_report).clone(),
                selected_evidence_ids: selected_evidence_ids.clone(),
                expected_evidence_ids: vec![format!("ev-expected-missing-{index}")],
                agent_diagnosis: None,
                proposed_fix: None,
            },
        )
        .expect("expected evidence failure records");
    }

    let result = cluster_retrieval_failures(
        &conn,
        RetrievalFailureClusterInput {
            human_review_status: Some("open".to_string()),
            failure_type: Some("expected_evidence_missing".to_string()),
            min_cluster_size: 2,
            limit: 20,
            create_tasks: true,
        },
    )
    .expect("failures cluster");

    assert_eq!(result.scanned_failure_count, 2);
    assert_eq!(result.cluster_count, 1);
    assert_eq!(result.task_count, 1);
    assert_eq!(result.clusters[0]["direct_fact_mutation"], json!(false));
    assert!(
        result.clusters[0]["proposed_fix"]
            .as_str()
            .is_some_and(|value| value.contains("no_direct_fact_mutation=true"))
    );
    assert_eq!(
        result.clusters[0]["task"]["source_entity_type"],
        json!("retrieval_failure_cluster")
    );
    assert_eq!(
        result.clusters[0]["task"]["task_type"],
        json!("expected_evidence_fix")
    );
    let cluster_key = result.clusters[0]["cluster_key"]
        .as_str()
        .expect("cluster key");
    let tasks = list_governance_tasks(
        &conn,
        KnowledgeGovernanceTaskListInput {
            status: Some("open".to_string()),
            task_type: Some("expected_evidence_fix".to_string()),
            priority: Some("p0".to_string()),
            source_failure_id: None,
            source_entity_type: Some("retrieval_failure_cluster".to_string()),
            source_entity_id: Some(cluster_key.to_string()),
            limit: 10,
            offset: 0,
        },
    )
    .expect("list cluster governance task");
    assert_eq!(tasks.items.len(), 1);
    assert!(
        tasks.items[0]["proposed_fix"]
            .as_str()
            .is_some_and(|value| value.contains("agent_cluster_proposed_fix"))
    );
    let open_failures = list_retrieval_failures(
        &conn,
        RetrievalFailureListInput {
            human_review_status: Some("open".to_string()),
            failure_type: Some("expected_evidence_missing".to_string()),
            limit: 10,
            offset: 0,
            view: RetrievalFailureView::AdminDetail,
        },
    )
    .expect("list open failures");
    assert_eq!(open_failures.items.len(), 2);
    let cluster_audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'retrieval_failures_clustered'",
            [],
            |row| row.get(0),
        )
        .expect("cluster audit count");
    assert_eq!(cluster_audit_count, 1);
}

#[test]
fn knowledge_patch_proposal_creates_human_review_task_without_fact_mutation() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let alias_count_before = table_count(&conn, "aliases").expect("alias count before proposal");
    let version_note_count_before =
        table_count(&conn, "version_notes").expect("version note count before proposal");

    let result = create_knowledge_patch_proposal(
        &conn,
        KnowledgePatchProposalCreateInput {
            proposal_type: "alias".to_string(),
            trace_id: "trace-knowledge-patch-proposal".to_string(),
            package_id: None,
            source_ref: Some("trace:trace-knowledge-patch-proposal".to_string()),
            payload: json!({
                "alias": "灵玉",
                "target_ref": "person:baoyu",
                "rationale": "专家建议进入人工复核，不直接写入别名表。",
            }),
            created_by: Some("agent-rqa".to_string()),
            priority: Some("p1".to_string()),
        },
    )
    .expect("proposal creates");

    assert_eq!(
        result["object"],
        json!("tonglingyu.knowledge_patch_proposal_create")
    );
    assert_eq!(
        result["schema_version"],
        json!(KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION)
    );
    assert_eq!(result["direct_fact_mutation"], json!(false));
    assert_eq!(result["proposal"]["proposal_type"], json!("alias"));
    assert_eq!(
        result["task"]["source_entity_type"],
        json!("knowledge_patch_proposal")
    );
    assert_eq!(result["task"]["task_type"], json!("alias_term_review"));
    assert_eq!(result["task"]["status"], json!("open"));
    assert!(
        result["task"]["proposed_fix"]
            .as_str()
            .is_some_and(|value| value.contains("no_direct_fact_mutation=true"))
    );

    let duplicate = create_knowledge_patch_proposal(
        &conn,
        KnowledgePatchProposalCreateInput {
            proposal_type: "alias".to_string(),
            trace_id: "trace-knowledge-patch-proposal".to_string(),
            package_id: None,
            source_ref: Some("trace:trace-knowledge-patch-proposal".to_string()),
            payload: json!({
                "target_ref": "person:baoyu",
                "rationale": "专家建议进入人工复核，不直接写入别名表。",
                "alias": "灵玉",
            }),
            created_by: Some("agent-rqa".to_string()),
            priority: Some("p1".to_string()),
        },
    )
    .expect("duplicate proposal returns existing");
    assert_eq!(
        duplicate["proposal"]["proposal_id"],
        result["proposal"]["proposal_id"]
    );
    assert_eq!(duplicate["task"]["task_id"], result["task"]["task_id"]);

    let task_id = result["task"]["task_id"].as_str().expect("task id");
    update_governance_task(
        &conn,
        task_id,
        KnowledgeGovernanceTaskUpdateInput {
            status: "accepted".to_string(),
            reviewer: Some("expert-reviewer".to_string()),
            review_note: Some("accept proposal for later KB rebuild input".to_string()),
            evidence_ref: Some("source://expert-review/alias/001".to_string()),
            expected_updated_at: Some(
                result["task"]["updated_at"]
                    .as_str()
                    .expect("task updated_at")
                    .to_string(),
            ),
        },
    )
    .expect("proposal task accepts")
    .expect("proposal task exists");

    assert_eq!(
        table_count(&conn, "aliases").expect("alias count after proposal"),
        alias_count_before
    );
    assert_eq!(
        table_count(&conn, "version_notes").expect("version note count after proposal"),
        version_note_count_before
    );
    let proposal_task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_governance_tasks WHERE source_entity_type = 'knowledge_patch_proposal'",
                [],
                |row| row.get(0),
            )
            .expect("proposal task count");
    assert_eq!(proposal_task_count, 1);
    let events = runtime_audit_events_for_trace(&conn, "trace-knowledge-patch-proposal")
        .expect("proposal audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "knowledge_patch_proposal_created"
            && event["payload"]["payload_sha256"].as_str().is_some()
            && event["payload"].get("payload").is_none()
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "governance_task_status_updated"
            && event["payload"]["evidence_ref_sha256"].as_str().is_some()
    }));
}

#[test]
fn prune_runtime_data_preserves_active_rqa_refs_and_writes_tombstones() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let old = "2020-01-01T00:00:00Z";
    let active_failure_package = create_evidence_package(
        &conn,
        "trace-active-failure-retention",
        "active failure question",
        vec![retention_test_card("active-failure")],
    )
    .expect("active failure package");
    let active_task_package = create_evidence_package(
        &conn,
        "trace-active-task-retention",
        "active task question",
        vec![retention_test_card("active-task")],
    )
    .expect("active task package");
    let expired_package = create_evidence_package(
        &conn,
        "trace-expired-retention",
        "expired question",
        vec![retention_test_card("expired")],
    )
    .expect("expired package");
    conn.execute(
        "UPDATE evidence_packages SET created_at = ?1 WHERE package_id IN (?2, ?3, ?4)",
        params![
            old,
            &active_failure_package.package_id,
            &active_task_package.package_id,
            &expired_package.package_id
        ],
    )
    .expect("packages old");
    conn.execute(
        "UPDATE evidence_cards SET created_at = ?1 WHERE package_id IN (?2, ?3, ?4)",
        params![
            old,
            &active_failure_package.package_id,
            &active_task_package.package_id,
            &expired_package.package_id
        ],
    )
    .expect("cards old");
    conn.execute(
        "UPDATE review_records SET created_at = ?1 WHERE package_id IN (?2, ?3, ?4)",
        params![
            old,
            &active_failure_package.package_id,
            &active_task_package.package_id,
            &expired_package.package_id
        ],
    )
    .expect("reviews old");
    conn.execute("UPDATE audit_events SET created_at = ?1", params![old])
        .expect("audit old");
    conn.execute(
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
                'rf-active-retention', ?1, ?2, ?3, 23, 'sha256:active',
                ?4, NULL, 'expected_evidence_missing', '[]', '["base_text"]',
                '[]', '["ev-missing"]', '[]', '["base_text"]',
                '["expected_evidence_missing"]', NULL,
                'review expected evidence', 'open', NULL, NULL, ?5, ?5, NULL
            )
            "#,
        params![
            &active_failure_package.trace_id,
            &active_failure_package.package_id,
            hash_text("active failure question"),
            KNOWLEDGE_BASE_SCHEMA_VERSION,
            old,
        ],
    )
    .expect("active failure inserts");
    conn.execute(
        r#"
            INSERT INTO knowledge_governance_tasks (
                task_id, source_failure_id, source_entity_type, source_entity_id,
                trace_id, package_id, task_type, status, priority,
                agent_cluster_key, proposed_fix, reviewer, review_note,
                evidence_ref, created_at, updated_at, accepted_at, closed_at
            ) VALUES (
                'kgt-active-retention', NULL, 'lifecycle_test',
                'active-retention-task', ?1, ?2, 'expected_evidence_fix',
                'accepted', 'p1', 'lifecycle:active-retention-task',
                'rebuild after accepted governance task', 'reviewer',
                'accepted for lifecycle protection', 'source://review/retention',
                ?3, ?3, ?3, NULL
            )
            "#,
        params![
            &active_task_package.trace_id,
            &active_task_package.package_id,
            old
        ],
    )
    .expect("active task inserts");
    conn.execute(
            "INSERT INTO audit_events (event_id, trace_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "audit-old-unrelated-retention",
                "trace-unrelated-retention",
                "old_unrelated",
                "{}",
                old,
            ],
        )
        .expect("old unrelated audit inserts");

    let dry_run = prune_runtime_data(&conn, 1, true).expect("dry run prune");
    assert_eq!(
        dry_run["lifecycle_policy_version"],
        json!(RQA_LIFECYCLE_POLICY_VERSION)
    );
    assert_eq!(dry_run["counts"]["package_candidates"], json!(3));
    assert_eq!(dry_run["counts"]["packages"], json!(1));
    assert_eq!(dry_run["counts"]["protected_packages"], json!(2));
    assert!(
        dry_run["counts"]["protected_audit_events"]
            .as_i64()
            .is_some_and(|count| count >= 4)
    );

    let report = prune_runtime_data(&conn, 1, false).expect("runtime prune");
    assert_eq!(report["status"], json!("pruned"));
    assert_eq!(report["counts"]["packages"], json!(1));
    assert_eq!(report["counts"]["protected_packages"], json!(2));
    assert!(
        report["counts"]["tombstones"]
            .as_i64()
            .is_some_and(|count| count >= 2)
    );
    assert_eq!(
        table_count_where_package(
            &conn,
            "evidence_packages",
            &active_failure_package.package_id
        ),
        1
    );
    assert_eq!(
        table_count_where_package(&conn, "evidence_packages", &active_task_package.package_id),
        1
    );
    assert_eq!(
        table_count_where_package(&conn, "evidence_packages", &expired_package.package_id),
        0
    );
    let active_audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE trace_id = ?1",
            params![&active_failure_package.trace_id],
            |row| row.get(0),
        )
        .expect("active audit count");
    assert!(active_audit_count > 0);
    let expired_audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE trace_id = ?1",
            params![&expired_package.trace_id],
            |row| row.get(0),
        )
        .expect("expired audit count");
    assert_eq!(expired_audit_count, 0);
    let prune_audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'rqa_retention_pruned'",
            [],
            |row| row.get(0),
        )
        .expect("retention audit count");
    assert_eq!(prune_audit_count, 1);
    let tombstone_payloads = tombstone_payloads(&conn);
    assert!(tombstone_payloads.iter().all(|payload| {
        !payload.contains("active failure question") && !payload.contains("expired question")
    }));
}

fn retention_test_card(suffix: &str) -> EvidenceCard {
    let mut card = sample_card("base_text");
    card.evidence_id = format!("ev-retention-{suffix}");
    card.block_id = format!("block-retention-{suffix}");
    card
}

fn table_count_where_package(conn: &Connection, table: &str, package_id: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE package_id = ?1"),
        params![package_id],
        |row| row.get(0),
    )
    .expect("package count")
}

fn tombstone_payloads(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT payload_json FROM rqa_lifecycle_tombstones ORDER BY created_at")
        .expect("prepare tombstones")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query tombstones")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("collect tombstones")
}

#[test]
fn governance_task_status_flow_requires_human_acceptance_metadata() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    let question = "通灵玉是什么？";
    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: question.to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");
    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    let failure = create_retrieval_failure(
        &conn,
        RetrievalFailureCreateInput {
            trace_id: "trace-governance-task-test".to_string(),
            package_id: Some("pkg-governance-task-test".to_string()),
            question: question.to_string(),
            quality_report: (*quality_report).clone(),
            selected_evidence_ids: evidence_ids(&cards),
            expected_evidence_ids: vec!["ev-expected-missing".to_string()],
            agent_diagnosis: Some("expected evidence absent".to_string()),
            proposed_fix: Some("review_expected_evidence_fixture".to_string()),
        },
    )
    .expect("failure creates governance task");
    let task = create_governance_task_from_failure(
        &conn,
        KnowledgeGovernanceTaskCreateFromFailureInput {
            source_failure_id: failure.failure_id.clone(),
            task_type: None,
            priority: None,
            proposed_fix: None,
            agent_cluster_key: None,
        },
    )
    .expect("governance task loads")
    .expect("governance task exists");

    let rejected = update_governance_task(
        &conn,
        &task.task_id,
        KnowledgeGovernanceTaskUpdateInput {
            status: "accepted".to_string(),
            reviewer: Some("rqa-reviewer".to_string()),
            review_note: Some("accepted".to_string()),
            evidence_ref: None,
            expected_updated_at: None,
        },
    )
    .expect_err("accepted task requires evidence ref");
    assert!(rejected.to_string().contains("requires reviewer"));

    let accepted = update_governance_task(
        &conn,
        &task.task_id,
        KnowledgeGovernanceTaskUpdateInput {
            status: "accepted".to_string(),
            reviewer: Some("rqa-reviewer".to_string()),
            review_note: Some("accepted with source patch".to_string()),
            evidence_ref: Some("source://review-note/001".to_string()),
            expected_updated_at: Some(task.updated_at.clone()),
        },
    )
    .expect("governance task updates")
    .expect("governance task exists");
    assert_eq!(accepted.status, "accepted");
    assert!(accepted.accepted_at.is_some());
    assert_eq!(
        accepted.evidence_ref.as_deref(),
        Some("source://review-note/001")
    );
    let events =
        runtime_audit_events_for_trace(&conn, "trace-governance-task-test").expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "governance_task_status_updated"
            && event["payload"]["evidence_ref_sha256"].as_str().is_some()
    }));
}

#[test]
fn governance_task_can_target_trace_without_source_failure() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");

    let task = create_governance_task(
        &conn,
        KnowledgeGovernanceTaskCreateInput {
            source_entity_type: "trace".to_string(),
            source_entity_id: "trace-expert-review-test".to_string(),
            trace_id: "trace-expert-review-test".to_string(),
            package_id: None,
            source_failure_id: None,
            task_type: "expert_review".to_string(),
            priority: Some("p0".to_string()),
            proposed_fix: Some("request_expert_review_without_fact_mutation".to_string()),
            agent_cluster_key: None,
        },
    )
    .expect("trace governance task creates");

    assert_eq!(task.source_failure_id, None);
    assert_eq!(task.source_entity_type, "trace");
    let listed = list_governance_tasks(
        &conn,
        KnowledgeGovernanceTaskListInput {
            status: Some("open".to_string()),
            task_type: Some("expert_review".to_string()),
            priority: Some("p0".to_string()),
            source_failure_id: None,
            source_entity_type: Some("trace".to_string()),
            source_entity_id: Some("trace-expert-review-test".to_string()),
            limit: 10,
            offset: 0,
        },
    )
    .expect("list trace governance task");
    assert_eq!(listed.items.len(), 1);
    assert_eq!(
        listed.items[0]["source_entity_id"],
        json!("trace-expert-review-test")
    );
}

#[test]
fn workflow_records_reviewer_failure_when_local_review_downgrades() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input(
            "trace-reviewer-failure-test",
            "不存在的检索目标",
            2,
            vec!["base_text".to_string()],
        ),
    )
    .expect("workflow executes");

    assert_eq!(workflow.package.review.status, "needs_revision");
    let list = list_retrieval_failures(
        &conn,
        RetrievalFailureListInput {
            human_review_status: Some("open".to_string()),
            failure_type: None,
            limit: 10,
            offset: 0,
            view: RetrievalFailureView::AdminDetail,
        },
    )
    .expect("list failures");
    let failure_types = list
        .items
        .iter()
        .filter_map(|item| item["failure_type"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(failure_types.contains("no_evidence_selected"));
    assert!(failure_types.contains("reviewer_evidence_insufficient"));
}

#[test]
fn retrieval_failure_rolls_back_when_audit_append_fails() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_retrieval_quality_source(
        &conn,
        json!({
            "license": "CC-BY-SA-4.0",
            "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
            "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
            "attribution": "Wikisource contributors",
            "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
        }),
    );
    let question = "通灵玉是什么？";
    let output = execute_tool(
        &conn,
        TonglingyuToolCall::TextSearch {
            question: question.to_string(),
            limit: 2,
            required_evidence_types: vec!["base_text".to_string()],
        },
    )
    .expect("search executes");
    let TonglingyuToolOutput::EvidenceCards {
        cards,
        quality_report,
        ..
    } = output
    else {
        panic!("expected evidence cards");
    };
    conn.execute_batch(
        r#"
            DROP TABLE audit_events;
            CREATE TABLE audit_events (event_id TEXT PRIMARY KEY);
            "#,
    )
    .expect("break audit table");

    let error = create_retrieval_failure(
        &conn,
        RetrievalFailureCreateInput {
            trace_id: "trace-audit-failure-test".to_string(),
            package_id: Some("pkg-audit-failure-test".to_string()),
            question: question.to_string(),
            quality_report: (*quality_report).clone(),
            selected_evidence_ids: evidence_ids(&cards),
            expected_evidence_ids: vec!["ev-expected-missing".to_string()],
            agent_diagnosis: None,
            proposed_fix: None,
        },
    )
    .expect_err("audit append failure should fail closed");
    assert!(error.to_string().contains("audit_events"));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM retrieval_failures", [], |row| {
            row.get(0)
        })
        .expect("failure count");
    assert_eq!(count, 0);
}

#[test]
fn detects_later_forty_source_titles() {
    assert!(!source_title_in_later_forty("紅樓夢/第八十回"));
    assert!(source_title_in_later_forty("紅樓夢/第八十一回"));
}

#[test]
fn source_scope_filter_excludes_later_forty_by_default_and_allows_explicit_scope() {
    let mut pre_80 = sample_card("base_text");
    pre_80.evidence_id = "ev-pre-80".to_string();
    pre_80.source_title = "紅樓夢/第080回".to_string();
    let mut later_40 = sample_card("base_text");
    later_40.evidence_id = "ev-later-40".to_string();
    later_40.source_title = "紅樓夢/第094回".to_string();

    let default_scope =
        filter_cards_for_source_scope("通灵宝玉丢了几次", vec![pre_80.clone(), later_40.clone()]);
    assert_eq!(
        evidence_ids(&default_scope.included_cards),
        vec!["ev-pre-80"]
    );
    assert!(!default_scope.report.policy.later_forty_allowed);
    assert_eq!(default_scope.report.out_of_scope_hints.len(), 1);
    assert_eq!(
        default_scope.report.out_of_scope_hints[0].evidence_id,
        "ev-later-40"
    );

    let explicit_scope =
        filter_cards_for_source_scope("按后四十回材料，通灵宝玉丢了几次", vec![later_40]);
    assert_eq!(
        evidence_ids(&explicit_scope.included_cards),
        vec!["ev-later-40"]
    );
    assert!(explicit_scope.report.policy.later_forty_allowed);
    assert!(explicit_scope.report.out_of_scope_hints.is_empty());
}

#[test]
fn source_scope_filter_explicit_later_forty_excludes_default_scope_layers() {
    let mut pre_80 = sample_card("base_text");
    pre_80.evidence_id = "ev-pre-80-explicit-later".to_string();
    pre_80.source_title = "紅樓夢/第076回".to_string();
    let mut commentary = sample_card("commentary");
    commentary.evidence_id = "ev-commentary-explicit-later".to_string();
    commentary.source_id = "shitouji-wikisource-zhiyanzhai".to_string();
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    let mut later_40 = sample_card("base_text");
    later_40.evidence_id = "ev-later-40-explicit".to_string();
    later_40.source_title = "紅樓夢/第116回".to_string();

    let explicit_scope = filter_cards_for_source_scope(
        "史湘云的结局，后四十回呢？",
        vec![pre_80, commentary, later_40],
    );

    assert_eq!(
        evidence_ids(&explicit_scope.included_cards),
        vec!["ev-later-40-explicit"]
    );
    assert_eq!(
        explicit_scope.report.policy.allowed_source_layers,
        vec!["base_text_later_40", "version_note"]
    );
    assert_eq!(explicit_scope.report.out_of_scope_hints.len(), 2);
}

#[test]
fn source_scope_filter_excludes_later_forty_navigation_but_keeps_commentary_foreshadowing() {
    let mut navigation = sample_card("base_text");
    navigation.evidence_id = "ev-navigation-later-forty".to_string();
    navigation.source_title = "紅樓夢（程甲本）".to_string();
    navigation.text = "[[/九十四|第九十四回]] 晏海棠賈母賞花妖 失寶玉通靈知奇禍\n[[/一百一十五|第一百一十五回]] 惑偏私惜春矢素志 證同類寶玉失相知".to_string();
    let mut commentary = sample_card("commentary");
    commentary.evidence_id = "ev-commentary-foreshadowing".to_string();
    commentary.source_title = "脂硯齋重評石頭記/第十八回".to_string();
    commentary.text =
        "第三齣《仙緣》；{{~|【庚辰雙行夾批：《邯鄲夢》中伏甄寶玉送玉。】}}".to_string();

    let default_scope =
        filter_cards_for_source_scope("通灵宝玉丢了几次", vec![navigation.clone(), commentary]);

    assert_eq!(
        evidence_ids(&default_scope.included_cards),
        vec!["ev-commentary-foreshadowing"]
    );
    assert_eq!(
        default_scope.report.out_of_scope_hints[0].evidence_id,
        "ev-navigation-later-forty"
    );
    assert_eq!(
        default_scope.report.out_of_scope_hints[0].source_layer,
        "base_text_later_40"
    );

    let explicit_scope =
        filter_cards_for_source_scope("按后四十回材料说明通灵宝玉丢失", vec![navigation]);
    assert_eq!(
        evidence_ids(&explicit_scope.included_cards),
        vec!["ev-navigation-later-forty"]
    );
}

#[test]
fn reviewer_blocks_no_evidence() {
    let review = review("黛玉结局是什么", &[], &[]);
    assert_eq!(review.status, "needs_revision");
    assert_eq!(review.severity, "high");
}

#[test]
fn reviewer_allows_commentary_only_body_claim() {
    let cards = vec![sample_card("commentary")];
    let question = "只根据脂批原文说明正文事实可以吗？";
    let claims = claims_from_cards(question, &cards);
    let review = review(question, &cards, &claims);
    assert_eq!(review.status, "passed", "{:?}", review.issues);
    assert!(review.issues.is_empty());
}

#[test]
fn reviewer_allows_commentary_original_text_question() {
    let cards = vec![sample_card("commentary")];
    let question = "脂批原文如何评价石头？";
    let claims = claims_from_cards(question, &cards);
    let review = review(question, &cards, &claims);

    assert_eq!(review.status, "passed", "{:?}", review.issues);
    assert!(review.issues.is_empty());
}

#[test]
fn reviewer_rejects_fate_question_when_cards_are_only_character_mentions() {
    let mut card = sample_card("base_text");
    card.text = "話說史湘雲回家後，寶玉等仍不過在園中嬉遊吟詠。".to_string();
    let question = "史湘云的结局";
    let claims = claims_from_cards(question, &[card.clone()]);
    let review = review(question, &[card], &claims);

    assert_eq!(review.status, "needs_revision");
    assert!(
        review
            .issues
            .iter()
            .any(|issue| { issue.contains("人物结局") && issue.contains("判词") })
    );
}

#[test]
fn reviewer_allows_fate_question_when_commentary_contains_fate_markers() {
    let mut card = sample_card("commentary");
    card.text =
        "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。{{~~|【甲眉：悲壯之極。】}}"
            .to_string();
    let question = "关于史湘云的结局，脂批中的证据呢";
    let claims = claims_from_cards(question, &[card.clone()]);
    let review = review(question, &[card], &claims);

    assert_eq!(review.status, "passed");
    assert!(review.issues.is_empty());
}

#[test]
fn reviewer_allows_later_forty_fate_question_when_cards_have_later_forty_markers() {
    let mut card = sample_card("base_text");
    card.source_id = "wikisource/chengyi/110".to_string();
    card.source_title = "紅樓夢/第110回".to_string();
    card.text =
        "且說史湘雲因他女婿病著，又見他女婿的病已成癆症，想到自己命苦，剛配了一個才貌雙全的男人。"
            .to_string();
    let question = "史湘云的结局，后四十回呢？";
    let claims = claims_from_cards(question, &[card.clone()]);
    let review = review(question, &[card], &claims);

    assert_eq!(review.status, "passed", "{:?}", review.issues);
    assert!(review.issues.is_empty());
}

#[test]
fn reviewer_downgrades_facsimile_authoritative_collation_claim() {
    let cards = vec![sample_card("base_text")];
    let question = "请确认通灵玉铭文在影印件、权威校注本和专家校勘中完全一致吗？";
    let claims = claims_from_cards(question, &cards);
    let review = review(question, &cards, &claims);

    assert_eq!(review.status, "needs_revision");
    assert_eq!(review.severity, "medium");
    assert!(
        review
            .issues
            .iter()
            .any(|issue| issue.contains("缺少影印件、权威校注本或专家校勘复核"))
    );
}

#[test]
fn reviewer_requires_later_forty_boundary_when_later_forty_cards_are_used() {
    let question = "通灵宝玉丢了几次";
    let cards = lost_jade_test_cards();
    let claims = claims_from_cards(question, &cards);

    assert!(claims.iter().any(|claim| claim.contains("后四十回")));
    assert_eq!(review(question, &cards, &claims).status, "passed");

    let unmarked_claims = vec!["命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string()];
    let review = review(question, &cards, &unmarked_claims);

    assert_eq!(review.status, "needs_revision");
    assert!(review.issues.iter().any(
            |issue| issue.contains("后四十回") && issue.contains("未标注时不能作为证据或参考")
        ));
}

#[test]
fn replay_keeps_package_id_and_review_downgrade() {
    let package = EvidencePackage {
        package_id: "pkg-test".to_string(),
        trace_id: "trace-test".to_string(),
        question: "量子计算机是什么？".to_string(),
        cards: vec![],
        claims: vec!["当前知识库未找到可追溯证据，不能给出确定结论。".to_string()],
        claim_evidence_map: vec![],
        knowledge_state_summary: KnowledgeStateSummary::default(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review: review("量子计算机是什么？", &[], &[]),
    };
    let answer = replay_answer(&package);
    assert!(!answer.contains("pkg-test"));
    assert!(!answer.contains("证据包"));
    assert!(!answer.contains("reviewer"));
    assert!(answer.contains("缺少足够证据") || answer.contains("没有检索到足够"));
}

#[test]
fn runtime_workflow_emits_profile_step_refs_and_review() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input(
            "trace-workflow-test",
            "量子红学理论如何解释通灵玉？",
            3,
            vec!["base_text".to_string()],
        ),
    )
    .expect("workflow executes");

    assert_eq!(workflow.steps.len(), 4);
    assert_eq!(workflow.package.review.status, "needs_revision");
    assert!(!workflow.final_answer.contains(&workflow.package.package_id));
    assert!(!workflow.final_answer.contains("证据包"));
    assert!(!workflow.final_answer.contains("reviewer"));
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "deterministic_workflow_only"
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_step_count"],
        json!(workflow.steps.len())
    );
    let plan = runtime_workflow_plan(RuntimeWorkflowPlanInput {
        question_type: "runtime_workflow".to_string(),
        required_evidence_types: vec!["base_text".to_string()],
        blocked_controls: Vec::new(),
        profiles: RuntimeWorkflowProfiles::default(),
    });
    let planned_steps = plan
        .steps
        .iter()
        .map(|step| {
            (
                step.step_id.clone(),
                step.operation.clone(),
                step.allowed_tools.clone(),
            )
        })
        .collect::<Vec<_>>();
    let actual_steps = workflow
        .steps
        .iter()
        .map(|step| {
            (
                step.step_id.clone(),
                step.operation.clone(),
                step.allowed_tools.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual_steps, planned_steps);
    assert!(
        workflow
            .steps
            .iter()
            .all(|step| step.output_ref.starts_with("runtime://tonglingyu/"))
    );
    assert!(
        workflow
            .steps
            .iter()
            .any(|step| step.operation == "review_answer" && step.output["draft_consumed"] == true)
    );
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "content_delta"
            && event
                .content_delta
                .as_deref()
                .is_some_and(|chunk| !chunk.is_empty())
    }));
    let profile_step_events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'runtime_profile_step_completed'",
            [],
            |row| row.get(0),
        )
        .expect("audit count");
    assert_eq!(profile_step_events, workflow.steps.len() as i64);
}

#[test]
fn runtime_workflow_consumes_pre_retrieved_evidence() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-pre-retrieved".to_string();
    card.block_id = "chunk-pre-retrieved".to_string();
    card.source_id = "knownledge-retriever".to_string();
    card.source_title = "knownledge retriever evidence".to_string();
    card.text = "黛玉葬花一节可作为人物心境与诗文结构的检索证据。".to_string();

    let mut input = test_workflow_input(
        "trace-pre-retrieved-workflow",
        "黛玉葬花为什么重要？",
        3,
        vec!["base_text".to_string()],
    );
    input.retrieved_evidence = Some(RuntimeWorkflowRetrievedEvidence {
        schema_version: RUNTIME_WORKFLOW_RETRIEVED_EVIDENCE_SCHEMA_VERSION.to_string(),
        source: "knownledge_http_retriever".to_string(),
        cards: vec![card],
        retrieval: json!({
            "schema_version": RUNTIME_WORKFLOW_RETRIEVED_EVIDENCE_SCHEMA_VERSION,
            "request": {
                "search_plan": {
                    "schema_version": "tonglingyu.agent_retriever.search_plan.v1",
                    "query": "黛玉葬花为什么重要？",
                    "routes": ["bm25", "vector", "entity", "event", "poem", "commentary"],
                },
                "retrieve_options": {
                    "schema_version": "tonglingyu.agent_retriever.retrieve_options.v1",
                    "trace_level": "route",
                },
            },
            "request_response_trace": {
                "transport": "http",
                "tool_name": "tonglingyu.agent_retriever.retrieve_http",
                "method": "POST",
                "path": "/retrieve",
                "request": {
                    "search_plan": {
                        "query": "黛玉葬花为什么重要？",
                    },
                },
                "response": {
                    "schema_version": "tonglingyu.agent_retriever.retrieve_response.v1",
                },
            },
            "raw_retrieve_response": {
                "schema_version": "tonglingyu.agent_retriever.retrieve_response.v1",
                "record_type": "agent_retriever_retrieve_response",
            },
            "evidence_pack": {
                "schema_version": "tonglingyu.agent_retriever.evidence_pack.v1",
                "doc_count": 1,
            },
            "diagnostics": {
                "sufficiency": {
                    "doc_count": 1,
                },
            },
        }),
    });

    let workflow = execute_runtime_workflow(&conn, input).expect("workflow executes");

    assert_eq!(workflow.steps[0].operation, "text_evidence_search");
    assert_eq!(
        workflow.steps[0].allowed_tools,
        vec!["tonglingyu.agent_retriever.retrieve_http".to_string()]
    );
    assert_eq!(
        workflow.steps[0].tool_calls,
        vec!["tonglingyu.agent_retriever.retrieve_http".to_string()]
    );
    assert_eq!(
        workflow.steps[0].output["object"],
        json!("tonglingyu.retriever_http.evidence_analysis")
    );
    assert_eq!(
        workflow.steps[0].output["request_response_trace"]["tool_name"],
        json!("tonglingyu.agent_retriever.retrieve_http")
    );
    assert_eq!(
        workflow.steps[0].output["raw_retrieve_response"]["schema_version"],
        json!("tonglingyu.agent_retriever.retrieve_response.v1")
    );
    assert!(workflow.package.cards.iter().any(|stored| {
        stored.block_id == "chunk-pre-retrieved" && stored.source_id == "knownledge-retriever"
    }));
    assert!(workflow.final_answer.contains("黛玉葬花"));
}

#[test]
fn runtime_workflow_rejects_commentary_required_without_external_retrieved_evidence() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");

    let error = execute_runtime_workflow(
        &conn,
        test_workflow_input(
            "trace-commentary-no-external-retrieval",
            "脂批如何评价通灵玉？",
            3,
            vec!["base_text".to_string(), "commentary".to_string()],
        ),
    )
    .expect_err("commentary evidence without external retrieval must fail closed");

    assert!(
        error
            .to_string()
            .contains("runtime commentary fallback is disabled")
    );
}

#[test]
fn runtime_workflow_binds_relation_frame_to_retrieval_and_review() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_relation_object_only_runtime_block(&conn);

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input_with_question_frame(
            "trace-relation-frame-workflow",
            "紫鹃服侍过史湘云吗？",
            4,
            vec!["base_text".to_string()],
            relation_question_frame_value(),
        ),
    )
    .expect("workflow executes");

    assert!(
        workflow
            .steps
            .iter()
            .all(|step| step.operation != "commentary_evidence_search"),
        "current workflow must not add commentary fallback search"
    );
    assert!(
        workflow
            .steps
            .iter()
            .filter(|step| step.operation == "text_evidence_search")
            .any(|step| step.output["query_frame_bound"] == json!(true))
    );
    assert!(workflow.package.question_frame.is_some());
    assert!(
        workflow
            .package
            .review
            .issues
            .iter()
            .any(|issue| issue == "relation_predicate_evidence_missing")
    );
    assert!(workflow.final_answer.contains("没有直接证据"));
    assert!(workflow.final_answer.contains("不能确认"));
    assert!(
        workflow
            .final_answer
            .contains("紫鹃与史湘云之间存在“服侍”关系")
    );
    assert!(!workflow.final_answer.contains("紫鹃服侍过史湘云"));
    let online_requests = list_online_evidence_card_update_requests_for_trace(
        &conn,
        "trace-relation-frame-workflow",
        10,
    )
    .expect("online evidence card requests list");
    assert_eq!(online_requests.len(), 1);
    assert_eq!(online_requests[0]["status"], json!("queued"));
    assert!(
        online_requests[0]["coverage_gap_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("review:reviewer_evidence_insufficient"))
    );
}

#[test]
fn runtime_workflow_promotes_relation_direct_support_cards() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_relation_direct_support_runtime_block(&conn);

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input_with_question_frame(
            "trace-relation-direct-workflow",
            "袭人服侍过贾母吗？",
            4,
            vec!["base_text".to_string()],
            xiren_jiamu_relation_question_frame_value(),
        ),
    )
    .expect("workflow executes");

    assert!(workflow.package.question_frame.is_some());
    assert!(
        workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-relation-direct")
    );
    assert!(
        !workflow
            .package
            .review
            .issues
            .iter()
            .any(|issue| issue == "relation_predicate_evidence_missing")
    );
    assert!(workflow.final_answer.contains("可以确认"));
    assert!(workflow.final_answer.contains("袭人服侍过贾母"));
    assert!(
        workflow
            .steps
            .iter()
            .filter(|step| step.operation == "text_evidence_search")
            .any(|step| {
                step.output["question_frame_direct_support_evidence_ids"]
                    .as_array()
                    .is_some_and(|ids| !ids.is_empty())
            })
    );
    let stored = load_evidence_package_from_conn(&conn, &workflow.package.package_id)
        .expect("stored package loads")
        .expect("stored package exists");
    assert!(stored.question_frame.is_some());
}

#[test]
fn runtime_workflow_promotes_open_object_relation_support_cards() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_relation_open_object_runtime_blocks(&conn);

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input_with_question_frame(
            "trace-relation-open-object-workflow",
            "袭人先后服侍过哪些人？",
            3,
            vec!["base_text".to_string()],
            xiren_open_object_relation_question_frame_value(),
        ),
    )
    .expect("workflow executes");

    assert!(workflow.package.question_frame.is_some());
    assert!(
        workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-xiren-jiamu-open")
    );
    assert!(
        workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-xiren-xiangyun-open")
    );
    assert!(workflow.final_answer.contains("袭人"));
    assert!(workflow.final_answer.contains("贾母"));
    assert!(workflow.final_answer.contains("史湘云"));
    assert!(workflow.final_answer.contains("贾宝玉"));
    assert!(!workflow.final_answer.contains("尚不能"));
    assert!(
        workflow
            .steps
            .iter()
            .filter(|step| step.operation == "text_evidence_search")
            .any(|step| {
                step.output["question_frame_open_object_support_evidence_ids"]
                    .as_array()
                    .is_some_and(|ids| !ids.is_empty())
            })
    );
}

#[test]
fn runtime_workflow_promotes_entity_question_frame_focus_cards() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_entity_focus_runtime_blocks(&conn);

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input_with_question_frame(
            "trace-entity-focus-workflow",
            "紫鹃在《红楼梦》里是什么样的人？",
            4,
            vec!["base_text".to_string()],
            zijuan_entity_question_frame_value(),
        ),
    )
    .expect("workflow executes");

    assert!(
        workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-zijuan-focus")
    );
    assert!(workflow.final_answer.contains("紫鹃"));
    assert!(workflow.final_answer.contains("紅樓夢/第003回"));
    assert!(
        !workflow
            .final_answer
            .contains("目前能支持回答的主要材料如下")
    );
    let draft_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "draft_answer")
        .expect("draft step");
    assert_eq!(
        draft_step.output["entity_intro_answer_policy"]["applies"],
        json!(true)
    );
    assert!(
        draft_step.output["entity_intro_answer_policy"]["rule"]
            .as_str()
            .is_some_and(|rule| rule.contains("multiple readable"))
    );
    assert!(
        workflow
            .steps
            .iter()
            .filter(|step| step.operation == "text_evidence_search")
            .any(|step| {
                step.output["question_frame_focus_evidence_ids"]
                    .as_array()
                    .is_some_and(|ids| !ids.is_empty())
            })
    );
}

#[test]
fn runtime_workflow_answers_character_fate_with_bound_slot_evidence() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_lindaiyu_character_fate_runtime_blocks(&conn);

    let mut commentary = sample_card("commentary");
    commentary.evidence_id = "ev-lindaiyu-fate-commentary".to_string();
    commentary.block_id = "quality-block-lindaiyu-fate-commentary".to_string();
    commentary.source_id = "quality-source".to_string();
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "可嘆停機德，{{~~|【甲夾：此句薛。】}}\n堪憐咏絮才。{{~~|【甲夾：此句林。】}}\n玉帶林中掛，\n金簪雪裡埋。".to_string();
    let mut input = test_workflow_input_with_question_frame(
        "trace-character-fate-workflow",
        "林黛玉结局如何",
        4,
        vec!["base_text".to_string()],
        lindaiyu_character_fate_question_frame_value(),
    );
    input.retrieved_evidence = Some(test_retrieved_evidence("林黛玉结局如何", vec![commentary]));

    let workflow = execute_runtime_workflow(&conn, input).expect("workflow executes");

    assert!(workflow.package.question_frame.is_some());
    assert!(
        workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-lindaiyu-fate-commentary")
    );
    assert!(
        !workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-lindaiyu-fate-noise")
    );
    assert!(
        !workflow
            .package
            .review
            .issues
            .iter()
            .any(|issue| issue == "character_fate_evidence_missing")
    );
    assert!(
        workflow
            .final_answer
            .contains("林黛玉的结局不能直接写成完整情节结局")
    );
    assert!(!workflow.final_answer.contains("当前可绑定的证据线索"));
    assert!(workflow.final_answer.contains("此句林"));
    assert!(workflow.final_answer.contains("玉帶林中掛"));
    assert!(!workflow.final_answer.contains("賈母萬般憐愛"));
}

#[test]
fn runtime_workflow_promotes_attribute_support_cards() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    init_knowledge_base_schema(&conn).expect("kb schema");
    seed_attribute_age_runtime_blocks(&conn);

    let workflow = execute_runtime_workflow(
        &conn,
        test_workflow_input_with_question_frame(
            "trace-attribute-age-workflow",
            "林黛玉进贾府时多大了",
            4,
            vec!["base_text".to_string()],
            lindaiyu_age_question_frame_value(),
        ),
    )
    .expect("workflow executes");

    assert!(
        workflow
            .package
            .cards
            .iter()
            .any(|card| card.block_id == "quality-block-lindaiyu-age-direct")
    );
    assert!(workflow.final_answer.contains("年方五歲"));
    assert!(!workflow.final_answer.contains("还没有直接命中"));
    assert!(
        workflow
            .steps
            .iter()
            .filter(|step| step.operation == "text_evidence_search")
            .any(|step| {
                step.output["question_frame_attribute_support_evidence_ids"]
                    .as_array()
                    .is_some_and(|ids| !ids.is_empty())
            })
    );
}

#[test]
fn hermes_mode_applies_runtime_draft_when_local_review_passes() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("runtime draft consumed");
    assert!(application.draft_consumed);
    assert!(application.content_used_for_final_answer);
    assert!(workflow.draft_answer.contains("Hermes profile 草稿"));
    assert_eq!(workflow.final_answer, workflow.draft_answer);
    assert_eq!(
        workflow.answer_source,
        "agent_runtime_hermes_profile_with_local_review"
    );
    assert_eq!(
        workflow.steps[0].agent_runtime.as_ref().unwrap()["content_used_for_final_answer"],
        json!(true)
    );
    assert_eq!(
        workflow.steps[1].output["draft_source"],
        "agent_runtime_hermes_profile"
    );
}

#[test]
fn agent_runtime_retrieval_repair_queries_enqueue_search_requests() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    let retrieval_query = json!({
        "query_text": "林黛玉 进贾府 年龄",
        "search_terms": ["林黛玉", "进贾府", "年纪"],
        "corpus_ids": ["cheng_120_base_text", "zhi_pre80_base_text_commentary"],
        "source_layers": ["base_text_pre_80", "commentary"],
        "chapter_range": {"start": 1, "end": 80},
        "required_evidence_types": ["base_text", "commentary"],
        "reason": "本地证据包未覆盖年龄推算所需原文与批语线索",
    });
    let upstream_bundle = serde_json::to_string(&json!({
        "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
        "package_id": package_id,
        "source_scope_policy": source_scope_policy_for_question(&workflow.question),
        "draft_candidate": {
            "draft_answer": "当前材料可以回答，但建议继续补强年龄相关证据。",
            "package_id": package_id,
            "claim_statements": [{
                "text": "当前材料可以回答，但证据建设仍可补强。",
                "evidence_refs": evidence_ids(&workflow.package.cards),
            }],
        },
        "coverage_assessment": {
            "status": "passed",
            "missing_in_scope_slots": [],
            "out_of_scope_slots": [],
        },
        "evidence_hints": [],
        "retrieval_repair": {
            "recommended": true,
            "queries": [retrieval_query],
        },
        "out_of_scope_hints": [],
    }))
    .expect("upstream bundle serializes");
    workflow.steps[0]
        .agent_runtime
        .as_mut()
        .and_then(Value::as_object_mut)
        .expect("draft step agent runtime object")
        .insert("result_summary".to_string(), json!(upstream_bundle));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("runtime draft consumed");
    assert!(application.draft_consumed);
    assert_eq!(
        workflow.steps[0].output["agent_runtime_retrieval_repair_query_count"],
        json!(1)
    );

    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_runtime_schema(&conn).expect("runtime schema");
    let inserted = record_agent_runtime_retrieval_repair_search_requests(&conn, &workflow)
        .expect("search requests recorded");
    assert_eq!(inserted, 1);
    let search_requests =
        list_online_evidence_card_search_requests_for_trace(&conn, &workflow.trace_id, 10)
            .expect("search requests queryable");
    assert!(search_requests.iter().any(|request| {
        let search_terms = request
            .request
            .get("search_terms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        request.request_source == "upstream_retrieval_repair"
            && search_terms.contains(&json!("林黛玉"))
            && search_terms.contains(&json!("进贾府"))
            && search_terms.contains(&json!("年纪"))
            && request.request["chapter_start"] == json!(1)
            && request.request["chapter_end"] == json!(80)
    }));
}

#[test]
fn openai_compatible_relation_question_keeps_local_relation_answer() {
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-relation-direct".to_string();
    card.source_title = "紅樓夢/第003回".to_string();
    card.text = "原來這襲人亦是賈母之婢，伏侍賈母時，心中眼中只有一個賈母。".to_string();
    let mut workflow = runtime_draft_workflow(
        vec![card],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "袭人服侍过贾母吗？".to_string();
    workflow.package.question = workflow.question.clone();
    workflow.package.question_frame = Some(xiren_jiamu_relation_question_frame_value());
    workflow.draft_answer = local_answer(&workflow.question, &workflow.package);
    workflow.final_answer = workflow.draft_answer.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &workflow.package.package_id,
            "未见有可靠证据表明袭人曾服侍过贾母。",
            "未见袭人服侍贾母的证据。",
            evidence_ids(&workflow.package.cards),
        ));

    let application = apply_agent_runtime_content_outputs(
        &mut workflow,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
    )
    .expect("runtime draft observed");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("question_frame_relation_answer_contradicts_evidence")
    );
    assert!(workflow.final_answer.contains("可以确认"));
    assert!(workflow.final_answer.contains("袭人服侍过贾母"));
    assert!(!workflow.final_answer.contains("未见有可靠证据"));
}

#[test]
fn openai_compatible_relation_question_accepts_predicate_preserving_draft() {
    let mut card = sample_card("base_text");
    card.evidence_id = "ev-relation-direct".to_string();
    card.source_title = "紅樓夢/第003回".to_string();
    card.text = "原來這襲人亦是賈母之婢，伏侍賈母時，心中眼中只有一個賈母。".to_string();
    let mut workflow = runtime_draft_workflow(
        vec![card],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "袭人服侍过贾母吗？".to_string();
    workflow.package.question = workflow.question.clone();
    workflow.package.question_frame = Some(xiren_jiamu_relation_question_frame_value());
    workflow.draft_answer = local_answer(&workflow.question, &workflow.package);
    workflow.final_answer = workflow.draft_answer.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &workflow.package.package_id,
            "可以确认，当前证据显示袭人服侍过贾母。",
            "袭人服侍贾母有直接证据。",
            evidence_ids(&workflow.package.cards),
        ));

    let application = apply_agent_runtime_content_outputs(
        &mut workflow,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
    )
    .expect("runtime draft observed");

    assert!(application.draft_consumed);
    assert!(application.content_used_for_final_answer);
    assert_eq!(application.rejected_reason, None);
    assert_eq!(
        workflow.final_answer,
        "可以确认，当前证据显示袭人服侍过贾母。"
    );
}

#[test]
fn hermes_mode_does_not_use_loss_count_oracle_to_reject_multiple_count_draft() {
    let mut workflow = runtime_draft_workflow(
        lost_jade_test_cards(),
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "按后四十回材料，通灵宝玉丢了几次".to_string();
    workflow.package.question = workflow.question.clone();
    let local = local_answer(&workflow.question, &workflow.package);
    workflow.draft_answer = local.clone();
    workflow.final_answer = local.clone();
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "若把后四十回材料单独标明，通灵宝玉并非只丢过一次，较稳妥的说法是至少两次。",
            "后四十回材料显示，通灵宝玉至少丢过两次。",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("runtime draft applied");

    assert!(application.draft_consumed);
    assert_eq!(application.rejected_reason, None);
    assert!(workflow.final_answer.contains("至少两次"));
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_consumed"],
        json!(true)
    );
}

#[test]
fn runtime_rejects_user_opt_in_continuation_draft() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "就目前这组可追溯事实来看，通灵宝玉不能稳妥地概括成“明确丢了几次”。如果你愿意，我可以继续按可核实情节帮你逐段梳理哪些算“丢失”。",
        &lost_jade_test_cards(),
    );

    assert_eq!(rejected, Some("draft_stops_for_user_opt_in"));
}

#[test]
fn runtime_rejects_lost_jade_nonanswer_without_later_forty_boundary() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "继续梳理的话，按目前这组可追溯事实，通灵宝玉还不能稳妥地概括成“明确丢了几次”。更稳妥的说法是：需要先把原著里符合“丢失”定义的具体情节逐条界定，再统计；在没有完成这一步之前，不宜先给出一个确定次数。",
        &lost_jade_test_cards(),
    );

    assert_eq!(rejected, Some("draft_missing_later_forty_boundary"));
}

#[test]
fn runtime_accepts_lost_jade_fuzzy_multiple_count_draft_with_later_forty_boundary() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "后四十回材料需要单独标明：通灵宝玉在《红楼梦》中并没有一个固定的“丢了几次”的标准答案；通常可概括为一次明显的丢失，以及若干次与遗失、失而复得相关的情节变动。",
        &lost_jade_test_cards(),
    );

    assert_eq!(rejected, None);
}

#[test]
fn runtime_accepts_loss_count_draft_that_matches_direct_loss_slot_semantics() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "通灵宝玉在前八十回正文与脂批范围内，能按明确失玉/被盗计入两处：第五十二回良儿偷玉、脂批第二十三回凤姐扫雪拾玉；脂批第十八回甄宝玉送玉只是疑似流转线索。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, None);
}

#[test]
fn runtime_count_policy_exposes_public_slot_context_for_upstream() {
    let value = evidence_slot_count_context_value(
        "通灵宝玉丢了几次",
        &in_scope_lost_jade_event_cards(),
        true,
    )
    .expect("count context");

    assert_eq!(value["direct_count"], json!(2));
    assert_eq!(
        value["direct_slots"]
            .as_array()
            .expect("direct slots")
            .iter()
            .filter_map(|item| item.get("label").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        vec!["凤姐扫雪拾玉", "良儿偷玉"]
    );
    assert_eq!(value["related_slots"][0]["label"], json!("甄宝玉送玉"));

    let compact = compact_evidence_slot_count_policy_for_message(value, 3);
    assert_eq!(compact["direct_count"], json!(2));
    assert_eq!(compact["direct_slots"][0]["label"], json!("凤姐扫雪拾玉"));
    assert_eq!(compact["related_slots"][0]["label"], json!("甄宝玉送玉"));
    assert!(
        compact["related_slots"][0]["source_cues"]
            .as_array()
            .expect("source cues")
            .iter()
            .any(|value| value == "第18回" || value == "第十八回")
    );
    assert!(compact["direct_slots"][0].get("counts_as").is_none());
}

#[test]
fn runtime_rejects_loss_count_draft_with_internal_slot_ids() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "按“明确失玉/被盗”口径，通灵宝玉明确算作丢失 2 次：证据槽位是「lianger_stole_jade（良儿偷玉）」和「fengjie_snow_pickup_jade（凤姐扫雪拾玉）」；另有相关线索「zhen_baoyu_delivers_jade（伏甄宝玉送玉）」可作旁证，但不计入直接次数。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, Some("draft_exposes_internal_evidence_slot_id"));
}

#[test]
fn runtime_rejects_public_draft_with_internal_answer_terms() {
    let mut commentary = sample_card("commentary");
    commentary.text = "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，脂批中的证据呢",
        "当前可直接用的证据槽主要是《第五回》“乐中悲”：终久是云散高唐，水涸湘江。",
        &[commentary],
    );

    assert_eq!(rejected, Some("draft_exposes_internal_public_term"));
}

#[test]
fn runtime_rejects_commentary_question_draft_without_commentary_source_anchor() {
    let mut base = sample_card("base_text");
    base.source_title = "紅樓夢/第050回".to_string();
    base.text = "湘雲忙聯道：".to_string();

    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，脂批中的证据呢",
        "脂批里能直接看到的证据主要是第50回中多次写到“湘雲道”“湘雲忙聯道”，说明史湘云仍活跃登场。",
        &[base, commentary],
    );

    assert_eq!(
        rejected,
        Some("draft_missing_requested_evidence_type_anchor")
    );
}

#[test]
fn runtime_allows_commentary_question_draft_with_commentary_source_and_text_anchor() {
    let mut base = sample_card("base_text");
    base.source_title = "紅樓夢/第050回".to_string();
    base.text = "湘雲忙聯道：".to_string();

    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，脂批中的证据呢",
        "脂硯齋重評石頭記/第五回《樂中悲》可作脂批证据：其中有“終久是雲散高唐，水涸湘江”的结局线索，但不能推出后四十回叙事。",
        &[base, commentary],
    );

    assert_eq!(rejected, None);
}

#[test]
fn runtime_rejects_evidence_question_draft_with_only_source_anchor() {
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，脂批中的证据呢",
        "脂批证据可以看《脂硯齋重評石頭記/第五回》，它与史湘云结局相关，但不能推出后四十回叙事。",
        &[commentary],
    );

    assert_eq!(
        rejected,
        Some("draft_missing_requested_evidence_type_anchor")
    );
}

#[test]
fn runtime_rejects_commentary_question_draft_with_only_generic_commentary_label() {
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，脂批中的证据呢",
        "脂批里确实有史湘云结局的证据，但这里只能说它提示悲剧意味，不能直接推成后四十回叙事。",
        &[commentary],
    );

    assert_eq!(
        rejected,
        Some("draft_missing_requested_evidence_type_anchor")
    );
}

#[test]
fn runtime_allows_commentary_question_draft_with_commentary_text_anchor() {
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第六支，樂中悲：襁褓中，父母嘆雙亡。終久是雲散高唐，水涸湘江。".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，脂批中的证据呢",
        "脂批中可用的是第五回《樂中悲》：“終久是雲散高唐，水涸湘江”；它能作为史湘云结局的悲剧线索，但不能替代后四十回叙事。",
        &[commentary],
    );

    assert_eq!(rejected, None);
}

#[test]
fn runtime_allows_version_boundary_question_draft_with_text_chapter_anchor() {
    let mut version_note = sample_card("version_note");
    version_note.source_id = "hongloumeng-wikisource-chengyi".to_string();
    version_note.source_title = "紅樓夢（程乙本）/第七十一回 至第八十回".to_string();
    version_note.text = "|next=[[../第八十一回 至第九十回|第八十一回 至第九十回]]".to_string();

    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "关于史湘云的结局，后四十回呢",
        "按版本说明能确认的是：第八十一回以后已进入后四十回范围；这只能给出版本边界，不能单凭该边界断定史湘云最终命运。",
        &[version_note],
    );

    assert_eq!(rejected, None);
}

#[test]
fn draft_message_preserves_answer_requirement_evidence_ids() {
    let mut base = sample_card("base_text");
    base.evidence_id = "ev-base".to_string();
    base.source_title = "紅樓夢/第050回".to_string();
    base.text = "湘雲道：".to_string();
    let mut commentary = sample_card("commentary");
    commentary.evidence_id = "ev-commentary".to_string();
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第六支，樂中悲：終久是雲散高唐，水涸湘江。".to_string();
    let evidence_brief = json!([{
        "evidence_id": "ev-base",
        "evidence_type": "base_text",
        "source_title": "紅樓夢/第050回",
        "text": "湘雲道："
    }]);
    let answer_requirements =
        answer_requirements_for_message("关于史湘云的结局，脂批中的证据呢", &[base, commentary])
            .expect("answer requirements");
    let ids = evidence_ids_from_evidence_brief_and_answer_requirements(
        &evidence_brief,
        &answer_requirements,
        &json!({}),
    )
    .expect("evidence ids")
    .as_array()
    .cloned()
    .expect("ids array");

    assert!(ids.iter().any(|value| value == "ev-base"));
    assert!(ids.iter().any(|value| value == "ev-commentary"));
    assert!(
        answer_requirements
            .to_string()
            .contains("requested_evidence_type_anchors")
    );
    assert_eq!(answer_requirements["evidence_request"], json!(true));
    assert_eq!(
        answer_requirements["requested_evidence_type_anchors"][0]["required_cue_kind"],
        json!("text")
    );
    assert!(
        answer_requirements["requested_evidence_type_anchors"][0]["cards"][0]["text_anchor_cues"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[test]
fn runtime_rejects_loss_count_draft_without_embedded_slot_evidence() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "按所给在范围内证据，通灵宝玉“明确失玉/被盗”可确认有 2 次；另有疑似流转线索，但不计入明确失玉次数。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, Some("draft_missing_embedded_evidence_anchor"));
}

#[test]
fn runtime_rejects_loss_count_draft_without_embedded_source_cues() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "通灵宝玉在前八十回正文与脂批范围内，能按明确失玉/被盗计入两处：良儿偷玉、凤姐扫雪拾玉；甄宝玉送玉只是疑似流转线索。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, Some("draft_missing_embedded_evidence_source"));
}

#[test]
fn runtime_rejects_loss_count_draft_that_counts_related_slots_as_direct_loss() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "按前八十回正文与脂批可见的证据，通灵宝玉明确涉及丢失相关情节共3次：良儿偷玉、凤姐扫雪拾玉和甄宝玉送玉。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, Some("draft_count_conflicts_with_evidence_events"));
}

#[test]
fn runtime_rejects_loss_count_draft_with_numeric_count_conflict() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "按现有证据，通灵宝玉明确“失玉/被盗”可计1次：第52回良儿偷玉；另有第23回脂批凤姐扫雪拾玉属于找回/拾回线索，不计入失玉次数；第18回脂批伏甄宝玉送玉也只是相关伏笔。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, Some("draft_count_conflicts_with_evidence_events"));
}

#[test]
fn runtime_rejects_loss_count_draft_that_negates_direct_slot() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "第52回良儿偷玉、第23回脂批凤姐扫雪拾玉、第18回脂批甄宝玉送玉均有材料；但第23回脂批凤姐扫雪拾玉不计入失玉次数，甄宝玉送玉只是疑似流转线索。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, Some("draft_negates_direct_evidence_slot_count"));
}

#[test]
fn runtime_allows_loss_count_draft_that_excludes_related_slot_after_direct_count_clause() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "按现有前八十回正文与评语证据，通灵宝玉可明确算到 2 处失玉/被盗线索：第23回“凤姐扫雪拾玉”，以及第52回“良儿偷玉”；另有第18回“甄宝玉送玉”只是“送玉/流转疑似线索”，不计入这 2 次明确失玉。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, None);
}

#[test]
fn runtime_allows_loss_count_draft_using_commentary_foreshadowing_without_later_forty_scope() {
    let rejected = agent_runtime_draft_evidence_boundary_rejection(
        "通灵宝玉丢了几次",
        "按当前证据包的默认范围，可以说有两处明确失玉证据：第五十二回良儿偷玉、脂批第二十三回称凤姐扫雪拾玉；另有脂批第十八回伏甄宝玉送玉，属于疑似流转线索。",
        &in_scope_lost_jade_event_cards(),
    );

    assert_eq!(rejected, None);
}

#[test]
fn hermes_mode_accepts_default_scope_draft_using_in_scope_commentary_foreshadowing() {
    let mut workflow = runtime_draft_workflow(
        in_scope_lost_jade_event_cards(),
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "通灵宝玉丢了几次".to_string();
    workflow.package.question = workflow.question.clone();
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "按当前证据包的默认范围，可以说有两处明确失玉证据：第五十二回良儿偷玉、脂批第二十三回称凤姐扫雪拾玉；另有脂批第十八回伏甄宝玉送玉，属于疑似流转线索。",
            "默认范围内的正文和脂批证据支持两处明确失玉证据，并保留一条疑似流转线索。",
            evidence_ids(&workflow.package.cards),
        )
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("commentary foreshadowing draft consumed");

    assert!(application.draft_consumed);
    assert_eq!(application.rejected_reason, None);
    assert!(workflow.final_answer.contains("两处"));
    assert!(workflow.final_answer.contains("凤姐扫雪拾玉"));
}

#[test]
fn hermes_mode_rejects_user_opt_in_continuation_draft() {
    let mut workflow = runtime_draft_workflow(
        lost_jade_test_cards(),
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "通灵宝玉丢了几次".to_string();
    workflow.package.question = workflow.question.clone();
    let local = local_answer(&workflow.question, &workflow.package);
    workflow.draft_answer = local.clone();
    workflow.final_answer = local.clone();
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "就目前这组可追溯事实来看，通灵宝玉不能稳妥地概括成“明确丢了几次”。如果你愿意，我可以继续按可核实情节帮你逐段梳理哪些算“丢失”。",
            "通灵宝玉失玉次数需要继续梳理后再确定。",
            evidence_ids(&workflow.package.cards),
        )
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("runtime draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("draft_stops_for_user_opt_in")
    );
    assert!(workflow.final_answer.contains("那塊玉真丟了么"));
    assert!(!workflow.final_answer.contains("如果你愿意"));
    assert!(!workflow.final_answer.contains("我可以继续"));
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "draft_stops_for_user_opt_in"
    );
}

#[test]
fn chapter_location_draft_rejections_are_governed_decisions() {
    assert!(agent_runtime_draft_rejection_is_governed_decision(
        "chapter_location_chapter_number_missing"
    ));
    assert!(agent_runtime_draft_rejection_is_governed_decision(
        "chapter_location_title_cue_missing"
    ));
}

#[tokio::test]
async fn runtime_repairs_rejected_profile_draft_with_same_package_boundary() {
    let mut workflow = runtime_draft_workflow(
        in_scope_lost_jade_event_cards(),
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "通灵宝玉丢了几次".to_string();
    workflow.package.question = workflow.question.clone();
    let package_id = workflow.package.package_id.clone();
    let count_question = question_asks_for_count(&workflow.question).expect("count intent");
    workflow.steps[0].output = json!({
        "object": "tonglingyu.draft_answer",
        "package_id": &package_id,
        "evidence_ids": evidence_ids(&workflow.package.cards),
        "evidence_brief": upstream_evidence_brief_for_frame(&workflow.question, &workflow.package.cards, None),
        "evidence_slot_count_policy": evidence_slot_count_context_value(
            &workflow.question,
            &workflow.package.cards,
            count_question,
        )
        .expect("count policy"),
        "claim_statements": &workflow.package.claims,
        "answer_source": "runtime_local_profile",
        "source_scope_policy": source_scope_policy_for_question(&workflow.question),
        "out_of_scope_hints": [],
    });
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "第52回良儿偷玉、第23回脂批凤姐扫雪拾玉、第18回脂批甄宝玉送玉均有材料；但第23回脂批凤姐扫雪拾玉不计入失玉次数，甄宝玉送玉只是疑似流转线索。",
            "通灵宝玉只有一处明确失玉证据。",
            evidence_ids(&workflow.package.cards),
        )
    );

    let rejected = apply_agent_runtime_content_outputs(
        &mut workflow,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
    )
    .expect("initial draft is rejected");
    assert!(!rejected.draft_consumed);
    assert_eq!(
        rejected.rejected_reason,
        Some("draft_negates_direct_evidence_slot_count")
    );

    let profiles = RuntimeWorkflowProfiles::default();
    let context = test_runtime_context(&workflow.trace_id, &workflow.question, &profiles);
    repair_agent_runtime_draft(
        &mut workflow,
        &profiles,
        &context,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
        Arc::new(DraftRepairRuntimeClient),
        &rejected,
    )
    .await
    .expect("draft repair executes");
    let repaired = apply_agent_runtime_content_outputs(
        &mut workflow,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
    )
    .expect("repaired draft is consumed");

    assert!(repaired.draft_consumed);
    assert_eq!(repaired.rejected_reason, None);
    assert!(workflow.final_answer.contains("两处"));
    assert!(workflow.final_answer.contains("凤姐扫雪拾玉"));
    assert_eq!(
        workflow.steps[0].agent_runtime.as_ref().unwrap()["draft_repair"]["initial_rejection"]["rejected_reason"],
        json!("draft_negates_direct_evidence_slot_count")
    );
}

#[tokio::test]
async fn runtime_repairs_open_object_relation_draft_missing_supported_object() {
    let question = "袭人先后服侍过哪些人？";
    let question_frame: question_frame::RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人服侍过谁？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头", "跟著"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("question frame");
    let mut grandparent = sample_card("base_text");
    grandparent.evidence_id = "ev-xiren-jiamu-baoyu".to_string();
    grandparent.source_title = "紅樓夢/第003回".to_string();
    grandparent.text =
        "原來這襲人亦是賈母之婢，本名珍珠，遂與了寶玉。這襲人有些癡處：伏侍賈母時，心中眼中只有一個賈母，如今服侍寶玉，心中眼中又只有一個寶玉。"
            .to_string();
    let mut shidaguniang = sample_card("base_text");
    shidaguniang.evidence_id = "ev-xiren-shidaguniang".to_string();
    shidaguniang.source_title = "紅樓夢/第019回".to_string();
    shidaguniang.text =
        "襲人道：“自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。”"
            .to_string();
    let mut workflow = runtime_draft_workflow(
        vec![grandparent, shidaguniang],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = question.to_string();
    workflow.package.question = question.to_string();
    workflow.package.question_frame = serde_json::to_value(&question_frame).ok();
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].output = json!({
        "object": "tonglingyu.draft_answer",
        "package_id": &package_id,
        "evidence_ids": evidence_ids(&workflow.package.cards),
        "evidence_brief": upstream_evidence_brief_for_frame(
            question,
            &workflow.package.cards,
            Some(&question_frame),
        ),
        "answer_requirements": answer_requirements_for_message(question, &workflow.package.cards)
            .expect("answer requirements"),
        "question_frame_answer_requirements": question_frame_answer_requirements_value(
            Some(&question_frame),
            &workflow.package.cards,
        )
        .expect("question frame answer requirements"),
        "evidence_slot_count_policy": evidence_slot_count_context_value(
            question,
            &workflow.package.cards,
            false,
        )
        .expect("count policy"),
        "claim_statements": &workflow.package.claims,
        "answer_source": "runtime_local_profile",
        "source_scope_policy": source_scope_policy_for_question(question),
        "out_of_scope_hints": [],
    });
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        upstream_bundle_summary(
            question,
            &package_id,
            "袭人先前本是贾母的婢女，后来又在宝玉房里贴身服侍宝玉；据第003回与第019回可见，她先后服侍过贾母和宝玉。",
            "袭人服侍过贾母和宝玉。",
            evidence_ids(&workflow.package.cards),
        )
    );

    let rejected = apply_agent_runtime_content_outputs(
        &mut workflow,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
    )
    .expect("initial open-object draft is rejected");
    assert!(!rejected.draft_consumed);
    assert_eq!(
        rejected.rejected_reason,
        Some("draft_missing_open_relation_object")
    );

    let profiles = RuntimeWorkflowProfiles::default();
    let context = test_runtime_context(&workflow.trace_id, question, &profiles);
    repair_agent_runtime_draft(
        &mut workflow,
        &profiles,
        &context,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
        Arc::new(OpenObjectDraftRepairRuntimeClient),
        &rejected,
    )
    .await
    .expect("open-object draft repair executes");
    let repaired = apply_agent_runtime_content_outputs(
        &mut workflow,
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
    )
    .expect("repaired open-object draft is consumed");

    assert!(repaired.draft_consumed);
    assert!(workflow.final_answer.contains("贾母"));
    assert!(workflow.final_answer.contains("史大姑娘"));
    assert!(workflow.final_answer.contains("宝玉"));
}

#[test]
fn hermes_mode_rejects_runtime_draft_when_local_review_downgrades() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "needs_revision".to_string(),
            severity: "high".to_string(),
            issues: vec!["当前没有可追溯证据。".to_string()],
            summary: "reviewer requires downgrade".to_string(),
        },
    );

    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "Hermes profile 草稿：必须引用证据包 pkg-runtime-draft-test。",
            "Hermes profile 草稿绑定本地证据包。",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("runtime draft consumed");
    assert!(application.draft_consumed);
    assert!(!application.content_used_for_final_answer);
    assert!(workflow.draft_answer.contains("Hermes profile 草稿"));
    assert!(!workflow.final_answer.contains("Hermes profile 草稿"));
    assert_eq!(
        workflow.answer_source,
        "agent_runtime_hermes_profile_rejected_by_local_review"
    );
    assert_eq!(
        workflow.steps[0].agent_runtime.as_ref().unwrap()["content_used_for_final_answer"],
        json!(false)
    );
    assert_eq!(
        workflow.steps[1].output["final_answer_source"],
        "agent_runtime_hermes_profile_rejected_by_local_review"
    );
}

#[test]
fn hermes_mode_accepts_structured_draft_with_matching_package() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "结构化 Hermes 草稿：必须引用证据包 pkg-runtime-draft-test。",
            "结构化 claim",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("structured runtime draft consumed");

    assert!(application.draft_consumed);
    assert_eq!(application.result_format, "json");
    assert!(application.rejected_reason.is_none());
    assert_eq!(
        workflow.draft_answer,
        "结构化 Hermes 草稿：必须引用证据包 pkg-runtime-draft-test。"
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_result_format"],
        "json"
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_claim_statement_count"],
        json!(1)
    );
}

#[test]
fn hermes_mode_rejects_bare_draft_candidate_without_upstream_bundle() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "draft_candidate": {
                "draft_answer": "裸 draft_candidate 不应绕过 upstream bundle。",
                "package_id": package_id,
                "claim_statements": [{
                    "text": "裸草稿 claim",
                    "evidence_refs": evidence_ids(&workflow.package.cards),
                }],
            }
        }))
        .expect("bare draft candidate serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("bare draft candidate rejected");

    assert!(!application.draft_consumed);
    assert_eq!(application.result_format, "json");
    assert_eq!(application.rejected_reason, Some("bundle_schema_missing"));
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "bundle_schema_missing"
    );
}

#[test]
fn hermes_mode_rejects_plain_text_draft_summary() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let original_draft = workflow.draft_answer.clone();
    let original_final = workflow.final_answer.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!("Hermes profile 草稿：必须引用证据包 pkg-runtime-draft-test。");

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("plain text runtime draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(application.result_format, "invalid");
    assert_eq!(application.rejected_reason, Some("invalid_json_draft"));
    assert_eq!(workflow.draft_answer, original_draft);
    assert_eq!(workflow.final_answer, original_final);
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "invalid_json_draft"
    );
}

#[test]
fn hermes_mode_rejects_upstream_bundle_with_scope_policy_mismatch() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary_with_policy(
            json!(source_scope_policy_for_question(
                "按后四十回材料，通灵宝玉丢了几次"
            )),
            &package_id,
            &package_id,
            "错误 scope policy 的草稿不应被消费。",
            "scope policy mismatch claim",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("scope policy mismatch rejected");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("source_scope_policy_mismatch")
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "source_scope_policy_mismatch"
    );
}

#[test]
fn hermes_mode_rejects_default_scope_draft_using_later_forty_material() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "第九十四回扫雪拾玉可以直接证明通灵宝玉又失而复得。",
            "默认范围内不应使用后四十回具体情节。",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("unscoped later-forty draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("draft_uses_unscoped_later_forty")
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "draft_uses_unscoped_later_forty"
    );
}

#[test]
fn hermes_mode_rejects_default_scope_draft_with_generic_later_forty_leak() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "通灵宝玉在前八十回中至少有两次；若把后四十回算进去，还会有更多相关情节。",
            "默认范围内不应泛化引用后四十回。",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("generic unscoped later-forty draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("draft_uses_unscoped_later_forty")
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "draft_uses_unscoped_later_forty"
    );
}

#[test]
fn hermes_mode_allows_default_scope_draft_that_excludes_later_forty_as_boundary() {
    let base = sample_card("base_text");
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text = "第五回脂批有「樂中悲」一支，又说「終久是雲散高唐，水涸湘江」。".to_string();
    let mut workflow = runtime_draft_workflow(
        vec![base, commentary],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "关于史湘云的结局，脂批中的证据呢".to_string();
    workflow.package.question = workflow.question.clone();
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "就现有前八十回正文与脂批看，脂硯齋重評石頭記/第五回的“樂中悲”“終久是雲散高唐，水涸湘江”只能支持史湘云有结局意象、无终局定论；这些证据只够支持她的命运基调与悲剧倾向，不能据此证明后四十回的具体终局。",
            "默认范围内只使用前八十回正文和脂批，并把后四十回排除在证据范围外。",
            evidence_ids(&workflow.package.cards),
        )
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("boundary-only later-forty mention is allowed");

    assert!(application.draft_consumed);
    assert_eq!(application.rejected_reason, None);
    assert!(workflow.final_answer.contains("不能据此证明后四十回"));
}

#[test]
fn hermes_mode_allows_later_forty_style_boundary_without_using_later_forty_evidence() {
    let base = sample_card("base_text");
    let mut commentary = sample_card("commentary");
    commentary.source_title = "脂硯齋重評石頭記/第五回".to_string();
    commentary.text =
        "第五回曲文写「襁褓中，父母嘆雙亡」，脂批以「樂中悲」点出这一支。".to_string();
    let mut workflow = runtime_draft_workflow(
        vec![base, commentary],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.question = "关于史湘云的结局，脂批中的证据呢".to_string();
    workflow.package.question = workflow.question.clone();
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "就现有前八十回正文与脂批来看，史湘云的结局只能作“有悲意的归结”来答，不能据此断成后四十回式的定论。第五回曲文已写她“襁褓中，父母叹双亡”，同回脂批又以“乐中悲”点出这一支的底色；至于更具体的终局细节，现有证据并不能证明。",
            "默认范围内只使用前八十回正文和脂批；若要谈史湘云更具体的终局，只能另行引入后四十回材料，本次包内不允许。",
            evidence_ids(&workflow.package.cards),
        )
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("later-forty boundary phrasing is not source use");

    assert!(application.draft_consumed);
    assert_eq!(application.rejected_reason, None);
    assert!(
        workflow
            .final_answer
            .contains("不能据此断成后四十回式的定论")
    );
}

#[test]
fn hermes_mode_rejects_partial_coverage_count_draft() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text"), sample_card("commentary")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": package_id,
            "source_scope_policy": source_scope_policy_for_question(&workflow.question),
            "draft_candidate": {
                "draft_answer": "通灵宝玉在前八十回里，通常可明确算作丢失/失而复得的情节主要有两次。",
                "package_id": package_id,
                "claim_statements": [{
                    "text": "前八十回里，通灵宝玉可概括为有两次主要的丢失/失而复得情节。",
                    "evidence_refs": evidence_ids(&workflow.package.cards),
                }],
            },
            "coverage_assessment": {
                "status": "partial",
                "missing_in_scope_slots": ["计数口径和事件边界仍缺少本地证据覆盖。"],
                "out_of_scope_slots": [],
            },
            "evidence_hints": [],
            "retrieval_repair": {
                "recommended": false,
                "queries": [],
            },
            "out_of_scope_hints": [],
        }))
        .expect("partial coverage bundle serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("partial coverage draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("coverage_assessment_not_passed")
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_coverage_status"],
        "partial"
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "coverage_assessment_not_passed"
    );
}

#[test]
fn hermes_mode_rejects_partial_coverage_before_claim_ref_validation() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text"), sample_card("commentary")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    let mut evidence_refs = evidence_ids(&workflow.package.cards);
    evidence_refs.push("ev-outside-package".to_string());
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": package_id,
            "source_scope_policy": source_scope_policy_for_question(&workflow.question),
            "draft_candidate": {
                "draft_answer": "通灵宝玉失落后，当前可用证据仍不足以回答完整经过。",
                "package_id": package_id,
                "claim_statements": [{
                    "text": "上游报告 coverage=partial 时，claim refs 不再决定是否完成治理。",
                    "evidence_refs": evidence_refs,
                }],
            },
            "coverage_assessment": {
                "status": "partial",
                "missing_in_scope_slots": ["完整经过仍缺证。"],
                "out_of_scope_slots": [],
            },
            "evidence_hints": [],
            "retrieval_repair": {
                "recommended": false,
                "queries": [],
            },
            "out_of_scope_hints": [],
        }))
        .expect("partial coverage bundle serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("partial coverage rejected before refs");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("coverage_assessment_not_passed")
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "coverage_assessment_not_passed"
    );
}

#[test]
fn hermes_mode_allows_default_scope_pre80_chengjia_source_label() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary(
            &workflow.question,
            &package_id,
            "程甲本五十二回的前八十回正文证据可作为默认范围内材料使用。",
            "程甲本五十二回属于前八十回正文证据。",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("pre-80 Chengjia source label accepted");

    assert!(application.draft_consumed);
    assert_eq!(application.rejected_reason, None);
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_consumed"],
        json!(true)
    );
}

#[test]
fn hermes_mode_rejects_direct_draft_object_without_candidate_wrapper() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "draft_answer": "直接对象草稿不应被消费。",
            "package_id": package_id,
            "claim_statements": ["direct claim"],
        }))
        .expect("direct draft serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("direct object runtime draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(application.result_format, "json");
    assert_eq!(application.rejected_reason, Some("bundle_schema_missing"));
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "bundle_schema_missing"
    );
}

#[test]
fn hermes_mode_rejects_draft_candidate_without_claim_statements() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": package_id,
            "source_scope_policy": source_scope_policy_for_question(&workflow.question),
            "draft_candidate": {
                "draft_answer": "缺少 claim_statements 的草稿不应被消费。",
                "package_id": package_id,
            },
            "coverage_assessment": {
                "status": "passed",
                "missing_in_scope_slots": [],
                "out_of_scope_slots": [],
            },
            "evidence_hints": [],
            "retrieval_repair": {
                "recommended": false,
                "queries": [],
            },
            "out_of_scope_hints": [],
        }))
        .expect("draft candidate serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("draft candidate without claims rejected");

    assert!(!application.draft_consumed);
    assert_eq!(
        application.rejected_reason,
        Some("claim_statements_missing")
    );
}

#[test]
fn hermes_mode_rejects_draft_candidate_answer_alias() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": package_id,
            "source_scope_policy": source_scope_policy_for_question(&workflow.question),
            "draft_candidate": {
                "answer": "answer 别名不应被当作 draft_answer。",
                "package_id": package_id,
                "claim_statements": [{
                    "text": "alias claim",
                    "evidence_refs": evidence_ids(&workflow.package.cards),
                }],
            },
            "coverage_assessment": {
                "status": "passed",
                "missing_in_scope_slots": [],
                "out_of_scope_slots": [],
            },
            "evidence_hints": [],
            "retrieval_repair": {
                "recommended": false,
                "queries": [],
            },
            "out_of_scope_hints": [],
        }))
        .expect("draft candidate serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("draft candidate answer alias rejected");

    assert!(!application.draft_consumed);
    assert_eq!(application.rejected_reason, Some("draft_answer_missing"));
}

#[test]
fn hermes_mode_rejects_nested_result_summary_draft() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "result_summary": serde_json::to_string(&json!({
                "draft_candidate": {
                    "draft_answer": "嵌套 Hermes 草稿：必须引用本地证据包。",
                    "package_id": package_id,
                    "claim_statements": ["nested claim"],
                }
            }))
            .expect("inner draft serializes")
        }))
        .expect("outer draft serializes")
    );

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("nested structured runtime draft rejected");

    assert!(!application.draft_consumed);
    assert_eq!(application.result_format, "json");
    assert_eq!(application.rejected_reason, Some("bundle_schema_missing"));
    assert_eq!(
        workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
        "bundle_schema_missing"
    );
}

#[test]
fn runtime_rebinds_structured_draft_package_id_to_local_package() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    let package_id = workflow.package.package_id.clone();
    workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] =
        json!(upstream_bundle_summary_with_candidate_package(
            &workflow.question,
            "pkg-other-bundle",
            "pkg-other",
            "上游草稿内容绑定本地证据引用，package id 由本地运行时强制重绑。",
            "package id 是运行时绑定字段，不由上游模型决定。",
            evidence_ids(&workflow.package.cards),
        ));

    let application =
        apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("structured runtime draft applied");

    assert!(application.draft_consumed);
    assert!(application.content_used_for_final_answer);
    assert_eq!(application.result_format, "json");
    assert_eq!(application.rejected_reason, None);
    assert!(workflow.final_answer.contains("本地证据引用"));
    assert_eq!(
        workflow.steps[0].output["agent_runtime_package_id"],
        json!(package_id)
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_package_id_rebound"],
        json!(true)
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_observed_bundle_package_id"],
        json!("pkg-other-bundle")
    );
    assert_eq!(
        workflow.steps[0].output["agent_runtime_observed_candidate_package_id"],
        json!("pkg-other")
    );
    assert_eq!(
        workflow.steps[0].agent_runtime.as_ref().unwrap()["content_application"]["package_id_rebound"],
        json!(true)
    );
    assert_eq!(
        workflow.steps[0].agent_runtime.as_ref().unwrap()["content_application"]["draft_consumed"],
        json!(true)
    );
}

#[test]
fn hermes_mode_observes_structured_reviewer_agreement() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.steps[1].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "review_status": "passed",
            "severity": "none",
            "issues": [],
            "required_revisions": [],
        }))
        .expect("structured review serializes")
    );

    let observation =
        apply_agent_runtime_reviewer_output(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("reviewer observation is recorded");

    assert_eq!(observation.result_format, "json");
    assert_eq!(observation.review_status.as_deref(), Some("passed"));
    assert!(observation.agrees_with_local_reviewer);
    assert!(!observation.local_reviewer_override);
    assert_eq!(
        workflow.steps[1].output["agent_runtime_review_agrees_with_local"],
        json!(true)
    );
    assert_eq!(
        workflow.steps[1].agent_runtime.as_ref().unwrap()["review_observation"]["local_reviewer_enforced"],
        json!(true)
    );
}

#[test]
fn hermes_mode_observes_nested_result_summary_reviewer_agreement() {
    let mut workflow = runtime_draft_workflow(
        vec![sample_card("base_text")],
        ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: vec![],
            summary: "reviewer passed".to_string(),
        },
    );
    workflow.steps[1].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "result_summary": serde_json::to_string(&json!({
                "review_observation": {
                    "review_status": "passed",
                    "severity": "none",
                    "issues": [],
                    "required_revisions": [],
                }
            }))
            .expect("inner review serializes")
        }))
        .expect("outer review serializes")
    );

    let observation =
        apply_agent_runtime_reviewer_output(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("nested reviewer observation is recorded");

    assert_eq!(observation.result_format, "json");
    assert_eq!(observation.review_status.as_deref(), Some("passed"));
    assert!(observation.agrees_with_local_reviewer);
    assert!(!observation.local_reviewer_override);
}

#[test]
fn hermes_mode_marks_reviewer_disagreement_as_local_override() {
    let mut workflow = runtime_draft_workflow(
        Vec::new(),
        ReviewRecord {
            status: "needs_revision".to_string(),
            severity: "high".to_string(),
            issues: vec!["当前没有可追溯证据。".to_string()],
            summary: "reviewer requires downgrade".to_string(),
        },
    );
    workflow.steps[1].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
        serde_json::to_string(&json!({
            "review_status": "passed",
            "severity": "none",
            "issues": [],
            "required_revisions": [],
        }))
        .expect("structured review serializes")
    );

    let observation =
        apply_agent_runtime_reviewer_output(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
            .expect("reviewer observation is recorded");

    assert_eq!(observation.review_status.as_deref(), Some("passed"));
    assert!(!observation.agrees_with_local_reviewer);
    assert!(observation.local_reviewer_override);
    assert_eq!(workflow.package.review.status, "needs_revision");
    assert_eq!(
        workflow.steps[1].output["agent_runtime_local_reviewer_override"],
        json!(true)
    );
    assert_eq!(
        workflow.steps[1].agent_runtime.as_ref().unwrap()["review_observation"]["local_review_status"],
        "needs_revision"
    );
}

#[test]
fn package_observation_rejects_wrong_runtime_package_id() {
    let observation = extract_agent_runtime_package_observation(
        r#"{"package_id":"pkg-other","summary":"wrong package"}"#,
        "pkg-runtime-draft-test",
    );

    assert_eq!(observation.result_format, "json");
    assert_eq!(observation.package_id.as_deref(), Some("pkg-other"));
    assert!(!observation.matches_runtime_package);
    assert_eq!(observation.rejected_reason, Some("package_id_mismatch"));
}

#[test]
fn package_observation_accepts_text_containing_expected_package_id() {
    let observation = extract_agent_runtime_package_observation(
        "result_summary: 已创建证据包 pkg-runtime-draft-test，包含 8 张卡片。",
        "pkg-runtime-draft-test",
    );

    assert_eq!(observation.result_format, "text");
    assert_eq!(
        observation.package_id.as_deref(),
        Some("pkg-runtime-draft-test")
    );
    assert!(observation.matches_runtime_package);
    assert!(observation.rejected_reason.is_none());
}

#[test]
fn package_observation_accepts_named_package_observation() {
    let observation = extract_agent_runtime_package_observation(
        r#"{"package_observation":{"package_id":"pkg-runtime-draft-test","summary":"package observed"}}"#,
        "pkg-runtime-draft-test",
    );

    assert_eq!(observation.result_format, "json");
    assert_eq!(
        observation.package_id.as_deref(),
        Some("pkg-runtime-draft-test")
    );
    assert!(observation.matches_runtime_package);
    assert!(observation.rejected_reason.is_none());
}

#[test]
fn evidence_observation_rejects_unknown_refs() {
    let observation = extract_agent_runtime_evidence_observation(
        r#"{"evidence_refs":["ev-known","ev-unknown"],"evidence_analysis":"test","unsupported_scope":"test"}"#,
        "text_evidence_search",
        "honglou-text",
        &["ev-known".to_string()],
    );

    assert_eq!(observation.result_format, "json");
    assert_eq!(observation.evidence_ref_count, 2);
    assert_eq!(
        observation.unknown_evidence_refs,
        vec!["ev-unknown".to_string()]
    );
    assert!(!observation.matches_runtime_evidence);
    assert_eq!(observation.rejected_reason, Some("unknown_evidence_ref"));
}

#[test]
fn evidence_observation_accepts_nested_result_summary_refs() {
    let summary = serde_json::to_string(&json!({
        "result_summary": serde_json::to_string(&json!({
            "evidence_observation": {
                "evidence_refs": ["ev-known"],
                "evidence_analysis": "test",
                "unsupported_scope": "test",
            }
        }))
        .expect("inner evidence serializes")
    }))
    .expect("outer evidence serializes");

    let observation = extract_agent_runtime_evidence_observation(
        &summary,
        "text_evidence_search",
        "honglou-text",
        &["ev-known".to_string()],
    );

    assert_eq!(observation.result_format, "json");
    assert_eq!(observation.evidence_ref_count, 1);
    assert!(observation.matches_runtime_evidence);
    assert!(observation.rejected_reason.is_none());
}

fn runtime_draft_workflow(cards: Vec<EvidenceCard>, review: ReviewRecord) -> RuntimeWorkflowOutput {
    let package = EvidencePackage {
        package_id: "pkg-runtime-draft-test".to_string(),
        trace_id: "trace-runtime-draft-test".to_string(),
        question: "通灵玉是什么？".to_string(),
        cards,
        claims: vec!["Hermes 草稿候选需要保留证据边界。".to_string()],
        claim_evidence_map: vec![],
        knowledge_state_summary: KnowledgeStateSummary::default(),
        question_frame: None,
        tiered_evidence_bindings: Vec::new(),
        online_learning_trace: None,
        review,
    };
    let default_draft_summary = upstream_bundle_summary(
        &package.question,
        &package.package_id,
        "Hermes profile 草稿：必须引用证据包 pkg-runtime-draft-test。",
        "Hermes profile 草稿绑定本地证据包。",
        evidence_ids(&package.cards),
    );
    RuntimeWorkflowOutput {
        trace_id: package.trace_id.clone(),
        question: package.question.clone(),
        package,
        draft_answer: "本地草稿".to_string(),
        final_answer: "本地最终回答".to_string(),
        answer_source: "runtime_local_profile".to_string(),
        agent_runtime_summary: default_agent_runtime_summary(),
        steps: vec![
            RuntimeWorkflowStepReport {
                step_id: "step-01-draft-answer".to_string(),
                profile: "honglou-main".to_string(),
                profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
                operation: "draft_answer".to_string(),
                status: "completed".to_string(),
                required: true,
                allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
                tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
                input_ref: None,
                output_ref: "runtime://tonglingyu/trace-runtime-draft-test/step-01-draft-answer"
                    .to_string(),
                duration_ms: 1,
                trace_id: "trace-runtime-draft-test".to_string(),
                output: json!({"object": "tonglingyu.draft_answer"}),
                agent_runtime: Some(json!({
                    "client": "hermes",
                    "status": "executed",
                    "content_used_for_final_answer": false,
                    "result_summary": default_draft_summary,
                })),
            },
            RuntimeWorkflowStepReport {
                step_id: "step-02-review-answer".to_string(),
                profile: "honglou-reviewer".to_string(),
                profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
                operation: "review_answer".to_string(),
                status: "completed".to_string(),
                required: true,
                allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
                tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
                input_ref: Some(
                    "runtime://tonglingyu/trace-runtime-draft-test/step-01-draft-answer"
                        .to_string(),
                ),
                output_ref: "runtime://tonglingyu/trace-runtime-draft-test/step-02-review-answer"
                    .to_string(),
                duration_ms: 1,
                trace_id: "trace-runtime-draft-test".to_string(),
                output: json!({"object": "tonglingyu.review_result"}),
                agent_runtime: Some(json!({
                    "client": "hermes",
                    "status": "executed",
                    "content_used_for_final_answer": false,
                    "result_summary": "Hermes reviewer envelope",
                })),
            },
        ],
        stream_events: Vec::new(),
    }
}

#[test]
fn tool_catalog_defines_expected_readonly_contracts() {
    let catalog = tool_catalog();
    let names = catalog
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<BTreeSet<_>>();
    for expected in [
        "tonglingyu.text.search",
        "tonglingyu.commentary.search",
        "tonglingyu.evidence.package.create",
        "tonglingyu.evidence.package.read",
        "tonglingyu.evidence.package.replay",
    ] {
        assert!(names.contains(expected), "missing tool contract {expected}");
    }
    assert!(
        catalog
            .iter()
            .all(|tool| tool.version == TOOL_CATALOG_VERSION)
    );
    assert!(
        catalog
            .iter()
            .filter(|tool| tool.name.ends_with(".search"))
            .all(|tool| tool.effect_scope == "read_only_kb")
    );
}

#[test]
fn profile_catalog_defines_four_runtime_profiles() {
    let catalog = profile_catalog();
    let profiles = catalog
        .iter()
        .map(|profile| profile.profile.as_str())
        .collect::<BTreeSet<_>>();
    for expected in [
        "honglou-text",
        "honglou-commentary",
        "honglou-main",
        "honglou-reviewer",
    ] {
        assert!(profiles.contains(expected), "missing profile {expected}");
    }
    assert!(
        catalog
            .iter()
            .all(|profile| profile.version == PROFILE_CONTRACT_VERSION)
    );
    let reviewer = catalog
        .iter()
        .find(|profile| profile.profile == "honglou-reviewer")
        .expect("reviewer profile exists");
    assert!(
        reviewer
            .allowed_tools
            .contains(&"tonglingyu.evidence.package.read".to_string())
    );
    assert!(reviewer.safety_contract["cannot_be_disabled_by_user"] == true);
}

#[tokio::test]
async fn runtime_store_executes_workflow_step_envelopes_through_agent_runtime() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-agent-step-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
    }
    let workflow = store
        .execute_workflow_with_agent_runtime_mode(
            test_workflow_input(
                "trace-agent-runtime-step-test",
                "量子红学理论如何解释通灵玉？",
                3,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::Minimal,
        )
        .await
        .expect("workflow executes");

    assert!(
        workflow
            .steps
            .iter()
            .all(|step| step.agent_runtime.is_some())
    );
    assert!(workflow.steps.iter().any(|step| {
        step.agent_runtime.as_ref().is_some_and(|value| {
            value["client"] == "minimal"
                && value["content_source"] == "tonglingyu-deterministic-workflow"
                && value["content_used_for_final_answer"] == json!(false)
        })
    }));
    assert!(workflow.steps.iter().any(|step| {
        step.operation == "draft_answer"
            && step.agent_runtime.as_ref().is_some_and(|value| {
                value["result_summary_contract"]
                    .as_str()
                    .is_some_and(|contract| {
                        contract.contains("draft_answer") && contract.contains("package_id")
                    })
                    && value["result_summary"].as_str().is_some_and(|summary| {
                        summary.contains("operation: draft_answer")
                            && summary.contains(&workflow.package.package_id)
                    })
            })
    }));
    assert!(workflow.steps.iter().any(|step| {
        step.operation == "review_answer"
            && step.agent_runtime.as_ref().is_some_and(|value| {
                value["result_summary_contract"]
                    .as_str()
                    .is_some_and(|contract| {
                        contract.contains("review_status") && contract.contains("local reviewer")
                    })
            })
    }));
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "step_completed"
            && event.metadata["agent_runtime"]["status"] == "executed"
    }));
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "minimal_envelope_only"
    );
    assert_eq!(
        workflow.agent_runtime_summary["executed_profile_step_count"],
        json!(workflow.steps.len())
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_content_execution_complete"],
        json!(false)
    );
    let conn = store.open_connection().expect("runtime conn");
    let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'agent_runtime_profile_step_executed'",
                [],
                |row| row.get(0),
            )
            .expect("audit count");
    assert_eq!(count, workflow.steps.len() as i64);
    let summary_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'agent_runtime_profile_execution_summarized'",
                [],
                |row| row.get(0),
            )
            .expect("summary audit count");
    assert_eq!(summary_count, 1);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn runtime_store_consumes_upstream_bundle_draft_through_full_workflow() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-draft-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                "trace-hermes-draft-workflow-test",
                "通灵玉是什么？",
                2,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(DraftRuntimeClient),
        )
        .await
        .expect("workflow executes");

    assert_eq!(workflow.package.review.status, "passed");
    assert!(workflow.draft_answer.contains("当前材料"));
    assert!(workflow.final_answer.contains("当前材料"));
    assert_eq!(
        workflow.answer_source,
        "agent_runtime_hermes_profile_with_local_review"
    );
    let draft_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "draft_answer")
        .expect("draft step");
    assert_eq!(
        draft_step.agent_runtime.as_ref().unwrap()["content_used_for_final_answer"],
        json!(true)
    );
    let draft_agent_runtime = draft_step.agent_runtime.as_ref().unwrap();
    assert_eq!(
        draft_agent_runtime["content_source"],
        json!("agent-runtime-hermes-profile")
    );
    assert_eq!(draft_agent_runtime["tool_rounds"], json!(1));
    assert_eq!(draft_agent_runtime["tool_result_count"], json!(1));
    assert_eq!(draft_agent_runtime["tool_audit_event_count"], json!(1));
    assert_eq!(
        draft_agent_runtime["tool_results"][0]["tool_name"],
        "tonglingyu.evidence.package.read"
    );
    let text_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "text_evidence_search")
        .expect("text evidence step");
    assert_eq!(
        text_step.agent_runtime.as_ref().unwrap()["content_source"],
        json!("agent-runtime-hermes-evidence-observation")
    );
    assert_eq!(
        text_step.agent_runtime.as_ref().unwrap()["evidence_observation"]["matches_runtime_evidence"],
        json!(true)
    );
    let package_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "evidence_package_create")
        .expect("package step");
    assert_eq!(
        package_step.agent_runtime.as_ref().unwrap()["content_source"],
        json!("agent-runtime-hermes-package-observation")
    );
    assert_eq!(
        package_step.agent_runtime.as_ref().unwrap()["package_observation"]["matches_runtime_package"],
        json!(true)
    );
    let review_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "review_answer")
        .expect("review step");
    assert_eq!(
        review_step.agent_runtime.as_ref().unwrap()["content_source"],
        json!("agent-runtime-hermes-review-observation")
    );
    assert_eq!(
        review_step.agent_runtime.as_ref().unwrap()["review_observation"]["local_reviewer_override"],
        json!(false)
    );
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "step_completed"
            && event.metadata["operation"] == json!("draft_answer")
            && event.metadata["agent_runtime"]["content_source"]
                == json!("agent-runtime-hermes-profile")
    }));
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "step_completed"
            && event.metadata["agent_runtime"]["tool_result_count"] == json!(1)
    }));
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "step_completed"
            && event.metadata["agent_runtime"]["evidence_observation"]["matches_runtime_evidence"]
                == json!(true)
    }));
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "step_completed"
            && event.metadata["agent_runtime"]["package_observation"]["matches_runtime_package"]
                == json!(true)
    }));
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "step_completed"
            && event.metadata["agent_runtime"]["review_observation"]["local_reviewer_override"]
                == json!(false)
    }));
    assert!(workflow.stream_events.iter().any(|event| {
        event.event_type == "content_delta"
            && event
                .content_delta
                .as_deref()
                .is_some_and(|chunk| chunk.contains("当前材料"))
    }));
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "hermes_profile_observed_with_local_governance"
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_content_execution_complete"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_consumed"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["content_used_for_final_answer"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["tool_result_count"],
        json!(4)
    );
    assert_eq!(
        workflow.agent_runtime_summary["tool_audit_event_count"],
        json!(4)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["sync_llm_call_count"],
        json!(workflow.steps.len())
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["call_budget_exceeded"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["latency_regression"],
        json!(true)
    );
    let events = store
        .audit_events_for_trace(&workflow.trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_draft_consumed"
            && event["payload"]["content_used_for_final_answer"] == json!(true)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_step_executed"
            && event["payload"]["agent_runtime"]["tool_result_count"] == json!(1)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_evidence_observed"
            && event["payload"]["matches_runtime_evidence"] == json!(true)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_package_observed"
            && event["payload"]["matches_runtime_package"] == json!(true)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_review_observed"
            && event["payload"]["local_reviewer_override"] == json!(false)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_execution_status"]
                == json!("hermes_profile_observed_with_local_governance")
            && event["payload"]["profile_content_execution_complete"] == json!(true)
            && event["payload"]["tool_result_count"] == json!(4)
            && event["payload"]["tool_audit_event_count"] == json!(4)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn runtime_store_consumes_openai_compatible_profile_without_runtime_tools() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-openai-compatible-profile-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                "trace-openai-compatible-profile-test",
                "通灵玉是什么？",
                2,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(NoToolRuntimeClient),
        )
        .await
        .expect("openai-compatible profile workflow executes");

    assert_eq!(workflow.package.review.status, "passed");
    assert!(workflow.draft_answer.contains("当前材料"));
    assert!(workflow.final_answer.contains("当前材料"));
    assert_eq!(
        workflow.answer_source,
        "agent_runtime_openai_compatible_profile_with_local_review"
    );
    let draft_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "draft_answer")
        .expect("draft step");
    let draft_agent_runtime = draft_step.agent_runtime.as_ref().unwrap();
    assert_eq!(
        draft_agent_runtime["content_source"],
        json!("agent-runtime-openai-compatible-profile")
    );
    assert_eq!(draft_agent_runtime["latency_ms"], json!(250));
    assert_eq!(draft_agent_runtime["tool_result_count"], json!(0));
    assert_eq!(draft_agent_runtime["tool_audit_event_count"], json!(0));
    assert_eq!(
        draft_step.output["answer_source"],
        "agent_runtime_openai_compatible_profile"
    );
    let text_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "text_evidence_search")
        .expect("text evidence step");
    assert!(text_step.agent_runtime.is_none());
    let package_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "evidence_package_create")
        .expect("package step");
    assert!(package_step.agent_runtime.is_none());
    let review_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "review_answer")
        .expect("review step");
    assert!(review_step.agent_runtime.is_none());
    assert_eq!(
        review_step.output["draft_source"],
        "agent_runtime_openai_compatible_profile"
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "openai_compatible_draft_with_local_governance"
    );
    assert_eq!(
        workflow.agent_runtime_summary["sync_execution_policy"],
        "draft_only"
    );
    assert_eq!(
        workflow.agent_runtime_summary["executed_profile_step_count"],
        json!(1)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["sync_llm_call_count"],
        json!(1)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["provider_latency_ms"],
        json!(250)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["latency_regression"],
        json!(false)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["skipped_profile_step_count"],
        json!(3)
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_observation_complete"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_content_execution_complete"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["tool_result_count"],
        json!(0)
    );
    assert_eq!(
        workflow.agent_runtime_summary["tool_audit_event_count"],
        json!(0)
    );
    let events = store
        .audit_events_for_trace(&workflow.trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_execution_status"]
                == json!("openai_compatible_draft_with_local_governance")
            && event["payload"]["profile_observation_complete"] == json!(true)
            && event["payload"]["profile_content_execution_complete"] == json!(true)
            && event["payload"]["llm_call_budget"]["sync_llm_call_count"] == json!(1)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openai_compatible_workflow_skips_upstream_draft_without_local_answer_basis() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-openai-compatible-no-answer-basis-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
    }
    let trace_id = "trace-openai-compatible-no-answer-basis-test";
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                trace_id,
                "量子红学理论如何解释通灵玉？",
                2,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(FailingProfileRuntimeClient),
        )
        .await
        .expect("OpenAI-compatible workflow should skip draft when no local answer basis exists");

    assert!(workflow.package.cards.is_empty());
    assert_eq!(workflow.answer_source, "runtime_local_profile");
    assert!(workflow.final_answer.contains("找不到足够的原文依据"));
    let draft_step = workflow
        .steps
        .iter()
        .find(|step| step.operation == "draft_answer")
        .expect("draft step");
    let draft_agent_runtime = draft_step.agent_runtime.as_ref().expect("draft runtime");
    assert_eq!(draft_agent_runtime["status"], json!("skipped"));
    assert_eq!(
        draft_agent_runtime["skip_reason"],
        json!("local_answer_basis_unavailable")
    );
    assert_eq!(
        draft_step.output["agent_runtime_provider_request_skipped"],
        json!(true)
    );
    assert_eq!(
        draft_step.output["agent_runtime_draft_rejected_reason"],
        json!("local_answer_basis_unavailable")
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        json!("openai_compatible_draft_skipped_by_local_governance")
    );
    assert_eq!(
        workflow.agent_runtime_summary["executed_profile_step_count"],
        json!(0)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["sync_llm_call_count"],
        json!(0)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["provider_latency_ms"],
        json!(0)
    );
    assert_eq!(
        workflow.agent_runtime_summary["llm_call_budget"]["skipped_profile_step_count"],
        json!(workflow.steps.len())
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_skipped_by_local_governance"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_governance_completed"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_observation_complete"],
        json!(true)
    );
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_step_executed"
            && event["payload"]["agent_runtime"]["status"] == json!("skipped")
            && event["payload"]["agent_runtime"]["skip_reason"]
                == json!("local_answer_basis_unavailable")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_draft_rejected"
            && event["payload"]["rejected_reason"] == json!("local_answer_basis_unavailable")
            && event["payload"]["draft_consumed"] == json!(false)
    }));
    assert!(
        !events
            .iter()
            .any(|event| event["event_type"] == "agent_runtime_profile_execution_rejected")
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openai_compatible_workflow_keeps_controlled_local_answer_when_draft_coverage_fails() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-openai-compatible-draft-coverage-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-openai-compatible-draft-coverage-test";
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(PartialCoverageDraftRuntimeClient),
        )
        .await
        .expect("draft coverage rejection should produce controlled local-governed response");

    assert_eq!(workflow.answer_source, "runtime_local_profile");
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "openai_compatible_draft_with_local_governance"
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_observation_complete"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_consumed"],
        json!(false)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_rejected_by_local_governance"],
        json!(true)
    );
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_draft_rejected"
            && event["payload"]["rejected_reason"] == json!("coverage_assessment_not_passed")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_observation_complete"] == json!(true)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openai_compatible_workflow_keeps_local_answer_when_draft_claim_refs_are_invalid() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-openai-compatible-draft-claim-ref-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-openai-compatible-draft-invalid-claim-ref-test";
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(InvalidEvidenceRefDraftRuntimeClient),
        )
        .await
        .expect("invalid upstream claim refs should be rejected without failing local governed response");

    assert_eq!(workflow.answer_source, "runtime_local_profile");
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "openai_compatible_draft_with_local_governance"
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_observation_complete"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_consumed"],
        json!(false)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_rejected_by_local_governance"],
        json!(true)
    );
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_draft_rejected"
            && event["payload"]["rejected_reason"] == json!("claim_evidence_ref_outside_package")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_observation_complete"] == json!(true)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openai_compatible_workflow_keeps_local_answer_when_draft_exceeds_evidence_boundary() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-openai-compatible-draft-boundary-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-openai-compatible-draft-boundary-test";
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(EvidenceBoundaryDraftRuntimeClient),
        )
        .await
        .expect("unsupported upstream claims should be rejected without failing local governed response");

    assert_eq!(workflow.answer_source, "runtime_local_profile");
    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        "openai_compatible_draft_with_local_governance"
    );
    assert_eq!(
        workflow.agent_runtime_summary["profile_observation_complete"],
        json!(true)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_consumed"],
        json!(false)
    );
    assert_eq!(
        workflow.agent_runtime_summary["draft_rejected_by_local_governance"],
        json!(true)
    );
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_draft_rejected"
            && event["payload"]["rejected_reason"] == json!("draft_claim_exceeds_evidence_boundary")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_observation_complete"] == json!(true)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openai_compatible_profile_step_does_not_duplicate_provider_retry() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-profile-retry-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-openai-compatible-profile-retry-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(FlakyProfileRuntimeClient::default()),
        )
        .await
        .expect_err("OpenAI-compatible workflow step must not add a second retry layer");

    assert!(error.to_string().contains("failed after 1 attempts"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(
        !events
            .iter()
            .any(|event| event["event_type"] == "agent_runtime_profile_local_answer_fallback")
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn runtime_store_audits_openai_compatible_provider_request_payload() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-provider-request-audit-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-provider-request-audit-test";
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(ProviderRequestRuntimeClient),
        )
        .await
        .expect("provider request audit workflow executes");

    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        json!("openai_compatible_draft_with_local_governance")
    );
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    let draft_event = events
        .iter()
        .find(|event| {
            event["event_type"] == "agent_runtime_profile_step_executed"
                && event["payload"]["operation"] == "draft_answer"
        })
        .expect("draft profile audit event");
    let provider_request = &draft_event["payload"]["agent_runtime"]["provider_request"];
    assert_eq!(
        provider_request["schema_version"],
        json!("openai-compatible-provider-request-v1")
    );
    assert_eq!(provider_request["message_count"], json!(1));
    assert_eq!(provider_request["messages"][0]["role"], json!("user"));
    assert!(
        provider_request["messages"][0]["content"]
            .as_str()
            .is_some_and(|content| content.contains("Tonglingyu profile step execution context"))
    );
    assert_eq!(
        draft_event["payload"]["agent_runtime"]["provider_request_embedded"],
        json!(true)
    );
    assert_eq!(
        provider_request["authorization_header_embedded"],
        json!(false)
    );
    assert_eq!(provider_request["api_key_embedded"], json!(false));
    assert!(!draft_event.to_string().contains("test-secret"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_host_enforces_missing_runtime_tool_results() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-required-tools-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let workflow = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                "trace-hermes-required-tools-test",
                "通灵玉是什么？",
                2,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(NoToolRuntimeClient),
        )
        .await
        .expect("Hermes profile steps should host-enforce required tool observations");

    assert_eq!(
        workflow.agent_runtime_summary["profile_execution_status"],
        json!("hermes_profile_observed_with_local_governance")
    );
    assert_eq!(
        workflow.agent_runtime_summary["tool_result_count"],
        json!(4)
    );
    assert_eq!(
        workflow.agent_runtime_summary["tool_audit_event_count"],
        json!(8)
    );
    assert!(workflow.steps.iter().all(|step| {
        step.agent_runtime
            .as_ref()
            .and_then(|value| value.get("tool_results"))
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("host_enforced") == Some(&json!(true))
                        && item.get("tool_name").and_then(Value::as_str).is_some()
                })
            })
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_rejects_unbound_runtime_tool_output_refs() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-output-ref-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                "trace-hermes-output-ref-test",
                "通灵玉是什么？",
                2,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(BadOutputRefRuntimeClient),
        )
        .await
        .expect_err("Hermes runtime tool output refs must bind to Tonglingyu runtime refs");

    let message = error.to_string();
    assert!(message.contains("invalid output_ref"));
    assert!(message.contains("text_evidence_search"));
    assert!(message.contains("tonglingyu.text.search"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_rejects_mismatched_evidence_tool_output_refs() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-evidence-output-ref-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                "trace-hermes-evidence-output-ref-test",
                "通灵玉是什么？",
                2,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(WrongEvidenceOutputRefRuntimeClient),
        )
        .await
        .expect_err("Hermes evidence tool output refs must bind to exact runtime evidence set");

    let message = error.to_string();
    assert!(message.contains("evidence tool"));
    assert!(message.contains("mismatched output_ref"));
    assert!(message.contains("text_evidence_search"));
    assert!(message.contains("tonglingyu.text.search"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_rejects_incomplete_profile_content_execution() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-incomplete-content-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
    }
    let trace_id = "trace-hermes-incomplete-content-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(IncompleteHermesContentRuntimeClient),
        )
        .await
        .expect_err("Hermes mode must fail closed when profile content execution is incomplete");

    let message = error.to_string();
    assert!(message.contains("Hermes runtime profile execution incomplete"));
    assert!(message.contains("hermes_profile_incomplete_local_governance"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_execution_status"]
                == json!("hermes_profile_incomplete_local_governance")
            && event["payload"]["profile_content_execution_complete"] == json!(false)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_rejected"
            && event["payload"]["summary"]["profile_execution_status"]
                == json!("hermes_profile_incomplete_local_governance")
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_draft_rejected"
            && event["payload"]["draft_consumed"] == json!(false)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_audits_profile_backend_failure() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-profile-failure-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
    }
    let trace_id = "trace-hermes-profile-failure-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(FailingProfileRuntimeClient),
        )
        .await
        .expect_err("Hermes mode must fail closed when a profile backend fails");

    assert!(error.to_string().contains("backend unavailable"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_rejected"
            && event["payload"]["failure_stage"] == json!("agent_runtime_step_execution")
            && event["payload"]["runtime_mode"] == json!("hermes")
            && event["payload"]["profile_step_count"].as_u64().unwrap_or(0) > 0
            && event["payload"]["executed_profile_step_count"] == json!(0)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn openai_compatible_workflow_audits_provider_diagnostic_on_profile_failure() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-openai-profile-diagnostic-failure-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-openai-profile-diagnostic-failure-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork,
            Arc::new(DiagnosticFailingProfileRuntimeClient),
        )
        .await
        .expect_err("OpenAI-compatible profile diagnostic failure must fail closed");

    assert!(error.to_string().contains("provider_empty_content"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_rejected"
            && event["payload"]["failure_stage"] == json!("agent_runtime_step_execution")
            && event["payload"]["runtime_mode"] == json!("openai-compatible-network")
            && event["payload"]["provider_diagnostic"]["schema_version"]
                == json!("openai-compatible-provider-diagnostic-v1")
            && event["payload"]["provider_diagnostic"]["error_type"]
                == json!("provider_empty_content")
            && event["payload"]["provider_diagnostic"]["raw_response_body_embedded"] == json!(false)
            && event["payload"]["provider_diagnostic"]["raw_content_embedded"] == json!(false)
            && event["payload"]["provider_diagnostic"]["secret_values_printed"] == json!(false)
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_fails_closed_on_timeout_even_with_local_loss_answer() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-timeout-local-loss-answer-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_lost_jade_runtime_blocks(&conn);
    }
    let trace_id = "trace-hermes-timeout-local-loss-answer-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(
                trace_id,
                "通灵宝玉丢了几次",
                4,
                vec!["base_text".to_string()],
            ),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(TimeoutProfileRuntimeClient),
        )
        .await
        .expect_err("Hermes timeout must fail closed even when local evidence is answerable");

    assert!(error.to_string().contains("Hermes Runtime timed out"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_rejected"
            && event["payload"]["failure_stage"] == json!("agent_runtime_step_execution")
            && event["payload"]["runtime_mode"] == json!("hermes")
    }));
    assert!(
        !events
            .iter()
            .any(|event| event["event_type"] == "agent_runtime_profile_local_answer_fallback")
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_still_fails_closed_on_timeout_without_local_loss_answer() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-timeout-no-local-answer-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
    }
    let trace_id = "trace-hermes-timeout-no-local-answer-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(TimeoutProfileRuntimeClient),
        )
        .await
        .expect_err("Hermes timeout should fail closed without a deterministic local answer");

    assert!(error.to_string().contains("Hermes Runtime timed out"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_rejected"
            && event["payload"]["failure_stage"] == json!("agent_runtime_step_execution")
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn hermes_workflow_rejects_missing_tool_audit_events() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-hermes-missing-tool-audit-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        seed_basic_tonglingyu_runtime_block(&conn);
    }
    let trace_id = "trace-hermes-missing-tool-audit-test";
    let error = store
        .execute_workflow_with_agent_runtime_client(
            test_workflow_input(trace_id, "通灵玉是什么？", 2, vec!["base_text".to_string()]),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(MissingToolAuditRuntimeClient),
        )
        .await
        .expect_err("Hermes mode must fail closed when tool results are not audited");

    let message = error.to_string();
    assert!(message.contains("missing tool audit events"));
    assert!(message.contains("0/4"));
    let events = store
        .audit_events_for_trace(trace_id)
        .expect("audit events");
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_summarized"
            && event["payload"]["profile_execution_status"]
                == json!("hermes_profile_observed_with_local_governance")
            && event["payload"]["profile_content_execution_complete"] == json!(true)
            && event["payload"]["tool_result_count"] == json!(4)
            && event["payload"]["tool_audit_event_count"] == json!(0)
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "agent_runtime_profile_execution_rejected"
            && event["payload"]["error"]
                == json!("Hermes runtime profile execution missing tool audit events: 0/4")
    }));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn agent_runtime_profile_steps_execute_concurrently_and_preserve_step_order() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-agent-step-concurrency-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
    }
    let mut workflow = store
        .execute_workflow(test_workflow_input(
            "trace-agent-runtime-step-concurrency-test",
            "脂批如何评价通灵玉？",
            3,
            vec!["base_text".to_string()],
        ))
        .expect("workflow executes");
    let profiles = RuntimeWorkflowProfiles::default();
    let context = test_runtime_context(
        "trace-agent-runtime-step-concurrency-test",
        "脂批如何评价通灵玉？",
        &profiles,
    );
    let expected_step_ids = workflow
        .steps
        .iter()
        .map(|step| step.step_id.clone())
        .collect::<Vec<_>>();
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    attach_agent_runtime_step_execution(
        &mut workflow,
        &profiles,
        &context,
        TonglingyuAgentRuntimeMode::Hermes,
        Arc::new(SlowDraftRuntimeClient::new(
            Arc::clone(&active),
            Arc::clone(&max_active),
        )),
    )
    .await
    .expect("profile steps execute");

    assert!(
        max_active.load(std::sync::atomic::Ordering::SeqCst) > 1,
        "profile steps should overlap instead of serializing"
    );
    assert_eq!(
        workflow
            .steps
            .iter()
            .map(|step| step.step_id.clone())
            .collect::<Vec<_>>(),
        expected_step_ids
    );
    assert!(
        workflow
            .steps
            .iter()
            .all(|step| step.agent_runtime.is_some())
    );
    for step in &workflow.steps {
        let agent_runtime = step.agent_runtime.as_ref().expect("agent runtime attached");
        let expected_runtime_step_id = format!("agent-runtime-{}", step.step_id);
        assert_eq!(
            agent_runtime["runtime_step"]["step_id"].as_str(),
            Some(expected_runtime_step_id.as_str())
        );
        assert_eq!(
            agent_runtime["runtime_step"]["metadata"]["workflow_step_id"].as_str(),
            Some(step.step_id.as_str())
        );
        assert_eq!(agent_runtime["client"].as_str(), Some("hermes"));
    }

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn runtime_tool_executor_runs_store_backed_tools() {
    let db_path = std::env::temp_dir().join(format!(
        "tonglingyu-runtime-tool-executor-{}.db",
        uuid::Uuid::now_v7().simple()
    ));
    let store = TonglingyuRuntimeStore::new(db_path.clone());
    {
        let conn = store.open_connection().expect("runtime conn");
        init_knowledge_base_schema(&conn).expect("kb schema");
    }
    let executor = TonglingyuRuntimeToolExecutor::new(store);
    let search = executor
        .execute_tool(
            RuntimeToolCall::new(
                "honglou-text",
                "tonglingyu.text.search",
                json!({
                    "question": "通灵玉是什么？",
                    "limit": 2,
                    "required_evidence_types": ["base_text"],
                }),
                "trace-runtime-tool-executor-test",
            ),
            RuntimeToolSpec::read_only("tonglingyu.text.search"),
        )
        .await
        .expect("text search tool executes");
    assert_eq!(search.tool_name, "tonglingyu.text.search");
    assert!(search.output_ref.as_deref().is_some_and(|value| {
        value.starts_with("runtime://tonglingyu/trace-runtime-tool-executor-test/evidence/")
    }));
    assert_eq!(search.output["object"], "evidence_cards");
    assert_eq!(
        search.metadata["runtime_tool_executor"],
        "tonglingyu-runtime-store"
    );

    let package = executor
        .execute_tool(
            RuntimeToolCall::new(
                "honglou-main",
                "tonglingyu.evidence.package.create",
                json!({
                    "trace_id": "trace-runtime-tool-executor-test",
                    "question": "脂批如何评价通灵玉？",
                    "cards": [sample_card("base_text")],
                }),
                "trace-runtime-tool-executor-test",
            ),
            RuntimeToolSpec::read_only("tonglingyu.evidence.package.create"),
        )
        .await
        .expect("package create tool executes");
    let package_id = package.output["package"]["package_id"]
        .as_str()
        .expect("package id")
        .to_string();
    assert!(package.output_ref.as_deref().is_some_and(|value| {
        value
            == format!(
                "runtime://tonglingyu/trace-runtime-tool-executor-test/packages/{package_id}"
            )
    }));

    let read = executor
        .execute_tool(
            RuntimeToolCall::new(
                "honglou-main",
                "tonglingyu.evidence.package.read",
                json!({"package_id": package_id}),
                "trace-runtime-tool-executor-test",
            ),
            RuntimeToolSpec::read_only("tonglingyu.evidence.package.read"),
        )
        .await
        .expect("package read tool executes");
    assert_eq!(
        read.output["package"]["package_id"],
        package.output["package"]["package_id"]
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn agent_runtime_plan_gate_executes_profile_contracts() {
    let profiles = RuntimeWorkflowProfiles::default();
    let context = test_runtime_context(
        "trace-agent-runtime-gate-test",
        "脂批如何评价通灵玉？",
        &profiles,
    );
    let report = execute_agent_runtime_plan_gate(AgentRuntimePlanGateInput {
        trace_id: "trace-agent-runtime-gate-test".to_string(),
        question: "脂批如何评价通灵玉？".to_string(),
        required_evidence_types: vec!["base_text".to_string(), "commentary".to_string()],
        profiles,
        context,
    })
    .await
    .expect("agent-runtime plan gate executes");

    assert_eq!(report.status, "passed");
    assert_eq!(report.agent_runtime_client, "minimal");
    assert_eq!(report.profile_contract_count, 3);
    assert_eq!(report.runtime_step_count, 4);
    assert_eq!(
        report.runtime_step_plan["owner"].as_str(),
        Some("domain_gateway")
    );
    assert!(
        report
            .runtime_step_outputs
            .as_array()
            .is_some_and(|outputs| {
                outputs
                    .iter()
                    .any(|output| output["profile_id"] == "honglou-reviewer")
            })
    );
    assert!(
        report.requested_tools_by_profile["honglou-main"]
            .contains(&"tonglingyu.evidence.package.create".to_string())
    );
    assert!(
        report.requested_tools_by_profile["honglou-reviewer"]
            .contains(&"tonglingyu.evidence.package.read".to_string())
    );
}

#[tokio::test]
async fn agent_runtime_plan_gate_rejects_projection_digest_mismatch() {
    let profiles = RuntimeWorkflowProfiles::default();
    let mut context = test_runtime_context(
        "trace-agent-runtime-gate-bad-digest-test",
        "脂批如何评价通灵玉？",
        &profiles,
    );
    context.projections[0].context_projection_digest = "bad-digest".to_string();

    let error = execute_agent_runtime_plan_gate(AgentRuntimePlanGateInput {
        trace_id: "trace-agent-runtime-gate-bad-digest-test".to_string(),
        question: "脂批如何评价通灵玉？".to_string(),
        required_evidence_types: vec!["base_text".to_string()],
        profiles,
        context,
    })
    .await
    .expect_err("projection digest mismatch must fail closed");

    assert!(
        error
            .to_string()
            .contains("context_projection_digest mismatch")
    );
}

#[test]
fn runtime_workflow_rejects_context_pack_digest_mismatch() {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute(
        "CREATE TABLE context_packs (context_pack_ref TEXT PRIMARY KEY, digest TEXT)",
        [],
    )
    .expect("context pack table");
    let input = test_workflow_input(
        "trace-runtime-pack-digest-test",
        "脂批如何评价通灵玉？",
        2,
        vec!["base_text".to_string()],
    );
    conn.execute(
        "INSERT INTO context_packs (context_pack_ref, digest) VALUES (?1, ?2)",
        params![&input.context.context_pack_ref, "wrong-digest"],
    )
    .expect("context pack row");

    let error = execute_runtime_workflow(&conn, input)
        .expect_err("context pack digest mismatch must fail closed");

    assert!(error.to_string().contains("context_pack_digest mismatch"));
}

#[tokio::test]
async fn agent_runtime_plan_gate_rejects_tools_outside_projection() {
    let profiles = RuntimeWorkflowProfiles::default();
    let mut context = test_runtime_context(
        "trace-agent-runtime-gate-tool-policy-test",
        "脂批如何评价通灵玉？",
        &profiles,
    );
    let text_projection = context
        .projections
        .iter_mut()
        .find(|projection| projection.consumer_name == profiles.text)
        .expect("text projection");
    text_projection.allowed_tools.clear();
    text_projection.tool_policy_digest = hash_json(&text_projection.tool_policy_value());
    text_projection.context_projection_digest = hash_json(&text_projection.digest_value());

    let error = execute_agent_runtime_plan_gate(AgentRuntimePlanGateInput {
        trace_id: "trace-agent-runtime-gate-tool-policy-test".to_string(),
        question: "脂批如何评价通灵玉？".to_string(),
        required_evidence_types: vec!["base_text".to_string()],
        profiles,
        context,
    })
    .await
    .expect_err("tool outside projection must fail closed");

    assert!(
        error
            .to_string()
            .contains("requested tool outside context projection")
    );
}
