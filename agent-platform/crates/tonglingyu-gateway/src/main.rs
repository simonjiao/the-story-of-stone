use agent_core::{
    AgentCoreError, CoreResult, ErrorCode, RuntimeClient, RuntimeOutput, RuntimeProfileInput,
    RuntimeRunInput, RuntimeSessionInput,
};
#[cfg(test)]
use agent_runtime::MinimalRuntimeClient;
use agent_runtime::{
    OpenAiCompatibleNetworkRuntimeClient, OpenAiCompatibleNetworkRuntimeConfig,
    RuntimeProfileRegistry,
};
use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::{Parser, Subcommand, ValueEnum};
use reqwest::header;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use time::OffsetDateTime;
use tonglingyu_runtime::{
    AgentRuntimePlanGateInput, EvidenceCard, EvidencePackage, KNOWLEDGE_BASE_SCHEMA_VERSION,
    KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION, KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION,
    KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION, KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION,
    KnowledgeCalibrationRunInput, KnowledgeGovernanceTaskCreateFromFailureInput,
    KnowledgeGovernanceTaskCreateInput, KnowledgeGovernanceTaskListInput,
    KnowledgeGovernanceTaskRecord, KnowledgeGovernanceTaskUpdateInput,
    KnowledgeItemHumanReviewDecision, KnowledgeItemHumanReviewInput, KnowledgeItemKind,
    KnowledgeItemListInput, KnowledgePatchProposalCreateInput, KnowledgeState,
    OnlineEvidenceCardWorkerRunInput, RETRIEVAL_FAILURE_CLUSTER_SCHEMA_VERSION,
    RETRIEVAL_FAILURE_SCHEMA_VERSION, RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION,
    RQA_LIFECYCLE_POLICY_VERSION, RUNTIME_CONTEXT_CONSUMER_TYPE,
    RUNTIME_CONTEXT_PACK_SCHEMA_VERSION, RUNTIME_CONTEXT_PROJECTION_SCHEMA_VERSION,
    RetrievalEvidenceTypeCoverage, RetrievalFailureClusterInput, RetrievalFailureCreateInput,
    RetrievalFailureListInput, RetrievalFailureView, RetrievalQualityReport, RetrievalQuerySummary,
    RetrievalSourceCoverageBoundary, RuntimeContextContract, RuntimeContextProjection,
    RuntimeWorkflowInput, RuntimeWorkflowProfiles, TONGLINGYU_RUNTIME_ADAPTER,
    TonglingyuAgentRuntimeMode, TonglingyuRuntimeStore, agent_runtime_profile_contracts,
    append_rqa_lifecycle_tombstone, append_runtime_audit_event, execute_agent_runtime_plan_gate,
    package_json,
};
#[cfg(test)]
use tonglingyu_runtime::{OnlineEvidenceCardUpdateRequestInput, RuntimeWorkflowStreamEvent};
use tower_http::trace::TraceLayer;

mod auth;
mod context_governance;
mod context_rules;
mod conversation_state;
mod draft_revision;
mod eval_command;
mod llm_agent_contracts;
mod llm_agent_prompt;
mod llm_agent_validator;
mod llm_contracts;
mod llm_eval;
mod llm_modes;
mod llm_provider;
mod llm_resolver;
mod plan;
mod question_frame;
mod response;
mod retrieval_suggestion;
mod rqa_lifecycle;
mod user_response_safety;

use crate::auth::{
    GatewayRateLimiter, PackageAccessContext, admin_auth_and_rate_limit, audit_subject_ref,
    configured_keys, gateway_auth_and_rate_limit, header_value, is_admin_key_isolated,
    package_access_context, validate_admin_key_isolation,
};
#[cfg(test)]
use crate::context_governance::create_context_for_request;
use crate::context_governance::{
    ContextMessage, ContextProjection, ContextRequestInput, ContextResolution,
    FinalResponseJournalInput, MemoryCandidateListInput, MemoryCandidateTransitionInput,
    MemoryCardListInput, MemoryCardTransitionInput, MemoryCollectorRunInput, append_final_response,
    append_review_journal, append_runtime_step_journal,
    create_context_for_request_with_agent_runtime, list_memory_candidates, list_memory_cards,
    load_deduped_final_response, read_memory_candidate, read_memory_card, run_memory_collector,
    table_counts as context_table_counts, transition_memory_candidate, transition_memory_card,
    validate_llm_memory_extraction_output,
};
use crate::eval_command::{eval_report_on_db_copy, run_eval_command};
use crate::llm_agent_contracts::{
    CONVERSATION_STATE_WRITER_PROFILE_ID, QUESTION_NORMALIZER_PROFILE_ID,
    tonglingyu_llm_agent_profile_contracts,
};
use crate::plan::{
    RuntimeStepPlan, SearchPolicy, planned_profiles_for_policy, public_search_policy, search_policy,
};
#[cfg(test)]
use crate::response::cached_runtime_stream_events;
use crate::response::{
    cache_completion_value, completion_value, public_completion_value,
    streaming_response_from_cached_completion_value, streaming_response_from_completion_value,
    streaming_response_from_runtime_events,
};
use crate::rqa_lifecycle::rqa_user_lifecycle_command;

const DEFAULT_MODEL_ID: &str = "tonglingyu";
const DEFAULT_MODEL_NAME: &str = "通灵玉";
const USER_FEEDBACK_SCHEMA_VERSION: &str = "tonglingyu-user-feedback-v1";
const USER_FEEDBACK_MAX_CHARS: usize = 2_000;
const USER_FEEDBACK_TASK_TEXT_MAX_CHARS: usize = 360;
const RQA_RESTORE_CANARY_SCHEMA_VERSION: &str = "tonglingyu-rqa-restore-canary-v1";

#[derive(Debug, Parser)]
#[command(name = "tonglingyu-gateway")]
#[command(about = "Tonglingyu Runtime-backed OpenAI-compatible gateway")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    BuildKb(BuildKbArgs),
    KbSourceMetadataBackfill(KbSourceMetadataBackfillArgs),
    Query(QueryArgs),
    ReplayPackage(ReplayPackageArgs),
    RuntimeDryRun(RuntimeDryRunArgs),
    Eval(EvalArgs),
    LlmEval(LlmEvalArgs),
    LlmReleaseReport(LlmReleaseReportArgs),
    KnowledgeCalibrate(KnowledgeCalibrateArgs),
    RuntimeSchemaPreflight(RuntimeSchemaPreflightArgs),
    RuntimeSchemaMigrate(RuntimeSchemaMigrateArgs),
    BackupDb(BackupDbArgs),
    PruneRuntime(PruneRuntimeArgs),
    RqaRestoreCanary(RqaRestoreCanaryArgs),
    RqaUserLifecycle(RqaUserLifecycleArgs),
    MemoryCollectorRun(MemoryCollectorRunArgs),
    MemoryCandidateList(MemoryCandidateListArgs),
    MemoryCandidateTransition(MemoryCandidateTransitionArgs),
    MemoryCardList(MemoryCardListArgs),
    MemoryCardTransition(MemoryCardTransitionArgs),
    Healthcheck(HealthcheckArgs),
    Serve(ServeArgs),
}

#[derive(Debug, Parser, Clone)]
struct BuildKbArgs {
    #[arg(
        long,
        env = "TONGLINGYU_SOURCE_ROOT",
        default_value = "resources/sources/wiki"
    )]
    source_root: PathBuf,
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long, default_value_t = false)]
    rebuild: bool,
    #[arg(long, default_value_t = 8)]
    eval_limit: usize,
    #[arg(long, default_value_t = false)]
    skip_diff_eval: bool,
}

#[derive(Debug, Parser, Clone)]
struct KbSourceMetadataBackfillArgs {
    #[arg(
        long,
        env = "TONGLINGYU_SOURCE_ROOT",
        default_value = "resources/sources/wiki"
    )]
    source_root: PathBuf,
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Parser, Clone)]
struct QueryArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    question: String,
    #[arg(long, default_value_t = 8)]
    limit: usize,
}

#[derive(Debug, Parser, Clone)]
struct ReplayPackageArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    package_id: String,
}

#[derive(Debug, Parser, Clone)]
struct RuntimeDryRunArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    question: String,
    #[arg(long, default_value_t = 8)]
    limit: usize,
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct EvalArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long, default_value_t = 8)]
    limit: usize,
    #[arg(long)]
    report: Option<PathBuf>,
    #[arg(
        long,
        env = "TONGLINGYU_EVAL_ALLOW_DB_MUTATION",
        default_value_t = false
    )]
    allow_db_mutation: bool,
}

#[derive(Debug, Parser, Clone)]
struct LlmEvalArgs {
    #[arg(long)]
    fixture_dir: PathBuf,
    #[arg(long)]
    report_out: PathBuf,
    #[arg(long, default_value_t = false)]
    fail_on_hard_gate: bool,
}

#[derive(Debug, Parser, Clone)]
struct LlmReleaseReportArgs {
    #[arg(long)]
    eval_report: PathBuf,
    #[arg(long)]
    report_out: PathBuf,
}

#[derive(Debug, Parser, Clone)]
struct KnowledgeCalibrateArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    input: PathBuf,
}

#[derive(Debug, Parser, Clone)]
struct RuntimeSchemaPreflightArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
}

#[derive(Debug, Parser, Clone)]
struct RuntimeSchemaMigrateArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
}

#[derive(Debug, Parser, Clone)]
struct BackupDbArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Parser, Clone)]
struct PruneRuntimeArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long, default_value_t = 90)]
    retention_days: u32,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Parser, Clone)]
struct RqaRestoreCanaryArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    package_id: Option<String>,
    #[arg(long, default_value = "restore-drill")]
    reviewer: String,
    #[arg(long, default_value = "closed restore drill canary")]
    review_note: String,
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct RqaUserLifecycleArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    user_ref: String,
    #[arg(long, value_enum)]
    action: RqaUserLifecycleAction,
    #[arg(long, default_value = "operator_requested")]
    reason: String,
}

#[derive(Debug, Parser, Clone)]
struct MemoryCollectorRunArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long, default_value = "admin_manual")]
    trigger: String,
    #[arg(long, default_value = "cli")]
    actor: String,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    #[arg(long)]
    trace_id: Option<String>,
}

#[derive(Debug, Parser, Clone)]
struct MemoryCandidateListArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    scope_type: Option<String>,
    #[arg(long)]
    scope_ref: Option<String>,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Debug, Parser, Clone)]
struct MemoryCandidateTransitionArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    candidate_id: String,
    #[arg(long)]
    action: String,
    #[arg(long, default_value = "cli")]
    actor: String,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    candidate_type: Option<String>,
    #[arg(long)]
    sensitivity: Option<String>,
    #[arg(long)]
    merge_target_candidate_id: Option<String>,
    #[arg(long)]
    expires_at: Option<String>,
}

#[derive(Debug, Parser, Clone)]
struct MemoryCardListArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    scope_type: Option<String>,
    #[arg(long)]
    scope_ref: Option<String>,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Debug, Parser, Clone)]
struct MemoryCardTransitionArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long)]
    memory_card_id: String,
    #[arg(long)]
    action: String,
    #[arg(long, default_value = "cli")]
    actor: String,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser, Clone)]
struct HealthcheckArgs {
    #[arg(long, default_value = "http://127.0.0.1:8090/healthz")]
    url: String,
    #[arg(long, default_value_t = 5)]
    timeout_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum RqaUserLifecycleAction {
    Export,
    Anonymize,
    LegalHold,
    ReleaseLegalHold,
}

#[derive(Debug, Parser, Clone)]
struct ServeArgs {
    #[arg(long, env = "TONGLINGYU_BIND", default_value = "127.0.0.1:8090")]
    bind: SocketAddr,
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(
        long,
        env = "TONGLINGYU_SOURCE_ROOT",
        default_value = "resources/sources/wiki"
    )]
    source_root: PathBuf,
    #[arg(long, env = "TONGLINGYU_AUTO_BUILD_KB", default_value_t = false)]
    auto_build_kb: bool,
    #[arg(long, env = "TONGLINGYU_MODEL_ID", default_value = DEFAULT_MODEL_ID)]
    model_id: String,
    #[arg(long, env = "TONGLINGYU_MODEL_NAME", default_value = DEFAULT_MODEL_NAME)]
    model_name: String,
    #[arg(long, env = "TONGLINGYU_UPSTREAM_BASE_URL")]
    upstream_base_url: Option<String>,
    #[arg(long, env = "TONGLINGYU_UPSTREAM_API_KEY")]
    upstream_api_key: Option<String>,
    #[arg(
        long,
        env = "TONGLINGYU_UPSTREAM_MODEL",
        default_value = "gpt-5.4-mini"
    )]
    upstream_model: String,
    #[arg(long, env = "TONGLINGYU_MAX_EVIDENCE", default_value_t = 8)]
    max_evidence: usize,
    #[arg(long, env = "TONGLINGYU_GATEWAY_API_KEY")]
    gateway_api_key: Option<String>,
    #[arg(long, env = "TONGLINGYU_GATEWAY_API_KEYS")]
    gateway_api_keys: Option<String>,
    #[arg(long, env = "TONGLINGYU_ADMIN_API_KEY")]
    admin_api_key: Option<String>,
    #[arg(long, env = "TONGLINGYU_ADMIN_API_KEYS")]
    admin_api_keys: Option<String>,
    #[arg(
        long,
        env = "TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY",
        default_value_t = false
    )]
    allow_admin_with_gateway_key: bool,
    #[arg(long, env = "TONGLINGYU_UPSTREAM_TIMEOUT_SECS", default_value_t = 30)]
    upstream_timeout_secs: u64,
    #[arg(long, env = "TONGLINGYU_MAX_MESSAGES", default_value_t = 40)]
    max_messages: usize,
    #[arg(long, env = "TONGLINGYU_MAX_QUESTION_CHARS", default_value_t = 4000)]
    max_question_chars: usize,
    #[arg(long, env = "TONGLINGYU_MAX_BODY_BYTES", default_value_t = 1_048_576)]
    max_body_bytes: usize,
    #[arg(long, env = "TONGLINGYU_RATE_LIMIT_PER_MINUTE", default_value_t = 120)]
    rate_limit_per_minute: usize,
    #[arg(long, env = "TONGLINGYU_RETENTION_DAYS", default_value_t = 0)]
    retention_days: u32,
    #[arg(
        long,
        env = "TONGLINGYU_MEMORY_COLLECTOR_BACKGROUND_ENABLED",
        default_value_t = true
    )]
    memory_collector_background_enabled: bool,
    #[arg(
        long,
        env = "TONGLINGYU_MEMORY_COLLECTOR_INTERVAL_SECS",
        default_value_t = 300
    )]
    memory_collector_interval_secs: u64,
    #[arg(
        long,
        env = "TONGLINGYU_MEMORY_COLLECTOR_BATCH_SIZE",
        default_value_t = 100
    )]
    memory_collector_batch_size: usize,
    #[arg(
        long,
        env = "TONGLINGYU_ONLINE_EVIDENCE_CARD_WORKER_ENABLED",
        default_value_t = true
    )]
    online_evidence_card_worker_enabled: bool,
    #[arg(
        long,
        env = "TONGLINGYU_ONLINE_EVIDENCE_CARD_WORKER_INTERVAL_SECS",
        default_value_t = 30
    )]
    online_evidence_card_worker_interval_secs: u64,
    #[arg(
        long,
        env = "TONGLINGYU_ONLINE_EVIDENCE_CARD_WORKER_BATCH_SIZE",
        default_value_t = 20
    )]
    online_evidence_card_worker_batch_size: usize,
    #[arg(
        long,
        env = "TONGLINGYU_ONLINE_EVIDENCE_CARD_WORKER_RETRIEVAL_LIMIT",
        default_value_t = 12
    )]
    online_evidence_card_worker_retrieval_limit: usize,
    #[arg(long, env = "TONGLINGYU_PROFILE_MAIN", default_value = "honglou-main")]
    profile_main: String,
    #[arg(long, env = "TONGLINGYU_PROFILE_TEXT", default_value = "honglou-text")]
    profile_text: String,
    #[arg(
        long,
        env = "TONGLINGYU_PROFILE_COMMENTARY",
        default_value = "honglou-commentary"
    )]
    profile_commentary: String,
    #[arg(
        long,
        env = "TONGLINGYU_PROFILE_REVIEWER",
        default_value = "honglou-reviewer"
    )]
    profile_reviewer: String,
}

#[derive(Clone)]
struct AppState {
    db: PathBuf,
    runtime_store: TonglingyuRuntimeStore,
    model_id: String,
    model_name: String,
    upstream_base_url: Option<String>,
    upstream_api_key: Option<String>,
    upstream_model: String,
    upstream_timeout_secs: u64,
    max_evidence: usize,
    gateway_api_keys: Vec<String>,
    admin_api_keys: Vec<String>,
    allow_admin_with_gateway_key: bool,
    max_messages: usize,
    max_question_chars: usize,
    max_body_bytes: usize,
    rate_limit_per_minute: usize,
    rate_limiter: Arc<GatewayRateLimiter>,
    admin_rate_limiter: Arc<GatewayRateLimiter>,
    retention_days: u32,
    online_evidence_card_worker_enabled: bool,
    online_evidence_card_worker_interval_secs: u64,
    online_evidence_card_worker_batch_size: usize,
    online_evidence_card_worker_retrieval_limit: usize,
    profiles: InternalProfiles,
    agent_runtime: Arc<dyn RuntimeClient>,
    agent_runtime_mode: TonglingyuAgentRuntimeMode,
    llm_agent_runtime: Arc<dyn RuntimeClient>,
    llm_agent_runtime_mode: String,
    llm_agent_provider_profiles: Value,
    workflow_agent_provider_profiles: Value,
    started_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct InternalProfiles {
    main: String,
    text: String,
    commentary: String,
    reviewer: String,
}

const TEXT_PROVIDER_ENV: &str = "TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER";
const PACKAGE_PROVIDER_ENV: &str = "TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER";
const DRAFT_PROVIDER_ENV: &str = "TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER";
const REVIEW_PROVIDER_ENV: &str = "TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER";
const QUESTION_NORMALIZER_PROVIDER_ENV: &str = "TONGLINGYU_AGENT_ROLE_QUESTION_NORMALIZER_PROVIDER";
const CONVERSATION_STATE_PROVIDER_ENV: &str = "TONGLINGYU_AGENT_ROLE_CONVERSATION_STATE_PROVIDER";

#[derive(Debug, Clone)]
struct AgentRoleProviderAssignment {
    runtime_profile: String,
    role_env: &'static str,
    provider_profile: String,
}

#[derive(Debug, Clone)]
struct AgentProviderProfile {
    name: String,
    backend: String,
    base_url: String,
    model: String,
    api_key_env: String,
    api_key: String,
}

struct ProfileRoutingRuntimeClient {
    clients_by_profile: BTreeMap<String, Arc<dyn RuntimeClient>>,
}

#[async_trait::async_trait]
impl RuntimeClient for ProfileRoutingRuntimeClient {
    async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "profile routing runtime only supports profile steps",
        ))
    }

    async fn send_session_message(&self, _input: RuntimeSessionInput) -> CoreResult<RuntimeOutput> {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "profile routing runtime only supports profile steps",
        ))
    }

    async fn execute_profile_step(&self, input: RuntimeProfileInput) -> CoreResult<RuntimeOutput> {
        let client = self
            .clients_by_profile
            .get(&input.profile_id)
            .cloned()
            .ok_or_else(|| {
                AgentCoreError::coded(
                    ErrorCode::Conflict,
                    format!(
                        "LLM Agent provider profile not configured for {}",
                        input.profile_id
                    ),
                )
            })?;
        client.execute_profile_step(input).await
    }
}

fn build_llm_agent_runtime() -> Result<(Arc<dyn RuntimeClient>, String, Value)> {
    build_llm_agent_runtime_from_source(&env_nonempty)
}

fn build_llm_agent_runtime_from_source(
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Arc<dyn RuntimeClient>, String, Value)> {
    let registry = RuntimeProfileRegistry::new(tonglingyu_llm_agent_profile_contracts());
    let assignments = llm_agent_provider_assignments_from_source(get_env)?;
    let mut profiles_by_provider = BTreeMap::<String, Vec<String>>::new();
    for assignment in &assignments {
        profiles_by_provider
            .entry(assignment.provider_profile.clone())
            .or_default()
            .push(assignment.runtime_profile.clone());
    }
    let mut clients_by_provider = BTreeMap::<String, Arc<dyn RuntimeClient>>::new();
    let mut provider_summaries = Vec::new();
    for (provider_profile, runtime_profiles) in &profiles_by_provider {
        let profile = AgentProviderProfile::from_source(provider_profile, get_env)?;
        let client =
            build_runtime_client_for_agent_provider(&profile, runtime_profiles, registry.clone())?;
        provider_summaries.push(profile.safe_summary(runtime_profiles));
        clients_by_provider.insert(provider_profile.clone(), client);
    }
    let mut clients_by_profile = BTreeMap::new();
    for assignment in &assignments {
        let client = clients_by_provider
            .get(&assignment.provider_profile)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "provider profile not built: {}",
                    assignment.provider_profile
                )
            })?;
        clients_by_profile.insert(assignment.runtime_profile.clone(), client);
    }
    let provider_config = json!({
        "object": "tonglingyu.llm_agent_provider_profile_config",
        "schema_version": 1,
        "routing_source": "role_provider_profile_env",
        "role_bindings": assignments.iter().map(|assignment| json!({
            "runtime_profile": assignment.runtime_profile,
            "role_env": assignment.role_env,
            "provider_profile": assignment.provider_profile,
        })).collect::<Vec<_>>(),
        "provider_profiles": provider_summaries,
        "secret_values_printed": false,
    });
    Ok((
        Arc::new(ProfileRoutingRuntimeClient { clients_by_profile }),
        "provider-profile".to_string(),
        provider_config,
    ))
}

fn build_workflow_agent_runtime_config(profiles: &InternalProfiles) -> Result<Value> {
    build_workflow_agent_runtime_config_from_source(profiles, &env_nonempty)
}

fn build_workflow_agent_runtime(
    profiles: &InternalProfiles,
) -> Result<(Arc<dyn RuntimeClient>, TonglingyuAgentRuntimeMode, Value)> {
    let config = build_workflow_agent_runtime_config(profiles)?;
    let mode = workflow_agent_runtime_mode_from_config(&config)?;
    let assignments = workflow_agent_provider_assignments_from_source(profiles, &env_nonempty)?;
    let provider_profile = assignments
        .first()
        .ok_or_else(|| anyhow!("workflow agent provider config must not be empty"))?
        .provider_profile
        .clone();
    let profile = AgentProviderProfile::from_source(&provider_profile, &env_nonempty)?;
    let workflow_runtime_profiles = vec![
        profiles.text.clone(),
        profiles.commentary.clone(),
        profiles.main.clone(),
        profiles.reviewer.clone(),
    ];
    let registry = RuntimeProfileRegistry::new(agent_runtime_profile_contracts(
        &runtime_workflow_profiles(profiles),
    ));
    let client = build_workflow_runtime_client_for_agent_provider(
        &profile,
        &workflow_runtime_profiles,
        registry,
    )?;
    Ok((client, mode, config))
}

fn workflow_agent_runtime_mode_from_config(config: &Value) -> Result<TonglingyuAgentRuntimeMode> {
    match config.get("mode").and_then(Value::as_str) {
        Some("openai-compatible-network") => {
            Ok(TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork)
        }
        Some(other) => Err(anyhow!(
            "workflow agent runtime config mode must be openai-compatible-network; got {other}"
        )),
        None => Err(anyhow!("workflow agent runtime config mode is missing")),
    }
}

fn build_workflow_runtime_client_for_agent_provider(
    profile: &AgentProviderProfile,
    runtime_profiles: &[String],
    registry: RuntimeProfileRegistry,
) -> Result<Arc<dyn RuntimeClient>> {
    workflow_agent_runtime_mode_for_provider(profile)?;
    let mut config = OpenAiCompatibleNetworkRuntimeConfig::new(&profile.base_url, &profile.model);
    config.api_key = Some(profile.api_key.clone());
    config.profile_models = runtime_profiles
        .iter()
        .map(|runtime_profile| (runtime_profile.clone(), profile.model.clone()))
        .collect();
    config.reasoning_split = Some(true);
    Ok(Arc::new(
        OpenAiCompatibleNetworkRuntimeClient::new(config)?.with_profile_registry(registry),
    ))
}

fn build_workflow_agent_runtime_config_from_source(
    profiles: &InternalProfiles,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Value> {
    let assignments = workflow_agent_provider_assignments_from_source(profiles, get_env)?;
    let mut profiles_by_provider = BTreeMap::<String, Vec<String>>::new();
    for assignment in &assignments {
        profiles_by_provider
            .entry(assignment.provider_profile.clone())
            .or_default()
            .push(assignment.runtime_profile.clone());
    }
    if profiles_by_provider.len() > 1 {
        let provider_names = profiles_by_provider.keys().cloned().collect::<Vec<_>>();
        return Err(anyhow!(
            "workflow agent roles must use one provider profile until per-step runtime routing is enabled: {}",
            provider_names.join(",")
        ));
    }
    let mut provider_summaries = Vec::new();
    let mut mode = None;
    let workflow_runtime_profiles = vec![
        profiles.text.clone(),
        profiles.commentary.clone(),
        profiles.main.clone(),
        profiles.reviewer.clone(),
    ];
    for provider_profile in profiles_by_provider.keys() {
        let profile = AgentProviderProfile::from_source(provider_profile, get_env)?;
        mode = Some(workflow_agent_runtime_mode_for_provider(&profile)?);
        provider_summaries.push(profile.safe_summary(&workflow_runtime_profiles));
    }
    Ok(json!({
        "object": "tonglingyu.workflow_agent_provider_profile_config",
        "schema_version": 1,
        "mode": mode
            .ok_or_else(|| anyhow!("workflow agent provider config must not be empty"))?
            .as_str(),
        "routing_source": "role_provider_profile_env",
        "role_bindings": assignments.iter().map(|assignment| json!({
            "runtime_profile": assignment.runtime_profile,
            "role_env": assignment.role_env,
            "provider_profile": assignment.provider_profile,
        })).collect::<Vec<_>>(),
        "provider_profiles": provider_summaries,
        "secret_values_printed": false,
    }))
}

impl AgentProviderProfile {
    fn from_source(name: &str, get_env: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        let backend = required_agent_provider_env_from(name, "BACKEND", get_env)?;
        let base_url = required_agent_provider_env_from(name, "BASE_URL", get_env)?;
        let model = required_agent_provider_env_from(name, "MODEL", get_env)?;
        let api_key_env = required_agent_provider_env_from(name, "API_KEY_ENV", get_env)?;
        let api_key = get_env(&api_key_env)
            .ok_or_else(|| anyhow!("{api_key_env} must be configured for {name}"))?;
        Ok(Self {
            name: name.to_string(),
            backend,
            base_url,
            model,
            api_key_env,
            api_key,
        })
    }

    fn safe_summary(&self, runtime_profiles: &[String]) -> Value {
        json!({
            "name": self.name,
            "backend": normalized_agent_provider_backend(&self.backend),
            "base_url_host": host_label_from_url(&self.base_url),
            "model": self.model,
            "api_key_env": self.api_key_env,
            "runtime_profiles": runtime_profiles,
            "secret_values_printed": false,
        })
    }
}

fn build_runtime_client_for_agent_provider(
    profile: &AgentProviderProfile,
    runtime_profiles: &[String],
    registry: RuntimeProfileRegistry,
) -> Result<Arc<dyn RuntimeClient>> {
    let backend = normalized_agent_provider_backend(&profile.backend);
    if backend != "openai-compatible-network" && backend != "minimax" {
        return Err(anyhow!(
            "agent provider {} must use backend openai-compatible-network or minimax; got {backend}",
            profile.name
        ));
    }

    let mut config = OpenAiCompatibleNetworkRuntimeConfig::new(&profile.base_url, &profile.model);
    config.api_key = Some(profile.api_key.clone());
    config.profile_models = runtime_profiles
        .iter()
        .map(|runtime_profile| (runtime_profile.clone(), profile.model.clone()))
        .collect();
    config.reasoning_split = Some(true);
    Ok(Arc::new(
        OpenAiCompatibleNetworkRuntimeClient::new(config)?.with_profile_registry(registry),
    ))
}

fn workflow_agent_runtime_mode_for_provider(
    profile: &AgentProviderProfile,
) -> Result<TonglingyuAgentRuntimeMode> {
    let backend = profile.backend.trim();
    if backend == "openai-compatible-network" {
        Ok(TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork)
    } else {
        Err(anyhow!(
            "workflow agent provider {} must use backend openai-compatible-network; got {backend}",
            profile.name
        ))
    }
}

fn llm_agent_provider_assignments_from_source(
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<AgentRoleProviderAssignment>> {
    Ok(vec![
        AgentRoleProviderAssignment {
            runtime_profile: QUESTION_NORMALIZER_PROFILE_ID.to_string(),
            role_env: QUESTION_NORMALIZER_PROVIDER_ENV,
            provider_profile: required_env_value_from(QUESTION_NORMALIZER_PROVIDER_ENV, get_env)?,
        },
        AgentRoleProviderAssignment {
            runtime_profile: CONVERSATION_STATE_WRITER_PROFILE_ID.to_string(),
            role_env: CONVERSATION_STATE_PROVIDER_ENV,
            provider_profile: required_env_value_from(CONVERSATION_STATE_PROVIDER_ENV, get_env)?,
        },
    ])
}

fn workflow_agent_provider_assignments_from_source(
    profiles: &InternalProfiles,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<AgentRoleProviderAssignment>> {
    Ok(vec![
        AgentRoleProviderAssignment {
            runtime_profile: profiles.text.clone(),
            role_env: TEXT_PROVIDER_ENV,
            provider_profile: required_env_value_from(TEXT_PROVIDER_ENV, get_env)?,
        },
        AgentRoleProviderAssignment {
            runtime_profile: profiles.main.clone(),
            role_env: PACKAGE_PROVIDER_ENV,
            provider_profile: required_env_value_from(PACKAGE_PROVIDER_ENV, get_env)?,
        },
        AgentRoleProviderAssignment {
            runtime_profile: profiles.main.clone(),
            role_env: DRAFT_PROVIDER_ENV,
            provider_profile: required_env_value_from(DRAFT_PROVIDER_ENV, get_env)?,
        },
        AgentRoleProviderAssignment {
            runtime_profile: profiles.reviewer.clone(),
            role_env: REVIEW_PROVIDER_ENV,
            provider_profile: required_env_value_from(REVIEW_PROVIDER_ENV, get_env)?,
        },
    ])
}

fn required_env_value_from(name: &str, get_env: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    get_env(name).ok_or_else(|| anyhow!("{name} must be configured"))
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn provider_profile_env_suffix(profile: &str) -> Result<String> {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("agent provider profile name must not be empty"));
    }
    let mut output = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_uppercase());
        } else if ch == '_' || ch == '-' {
            output.push('_');
        } else {
            return Err(anyhow!(
                "agent provider profile name contains unsupported character: {profile}"
            ));
        }
    }
    Ok(output)
}

fn agent_provider_env_name(profile: &str, field: &str) -> Result<String> {
    Ok(format!(
        "TONGLINGYU_AGENT_PROVIDER_{}_{}",
        provider_profile_env_suffix(profile)?,
        field
    ))
}

fn required_agent_provider_env_from(
    profile: &str,
    field: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String> {
    let env_name = agent_provider_env_name(profile, field)?;
    required_env_value_from(&env_name, get_env)
}

fn normalized_agent_provider_backend(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "hermes_agent" | "hermes-agent" => "hermes-agent".to_string(),
        "openai_compatible_network" | "openai-compatible-network" => {
            "openai-compatible-network".to_string()
        }
        "openai_compatible" | "openai-compatible" => "openai-compatible-network".to_string(),
        "minimax" => "minimax".to_string(),
        other => other.to_string(),
    }
}

fn host_label_from_url(value: &str) -> String {
    reqwest::Url::parse(value)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "invalid-url".to_string())
}

fn runtime_workflow_profiles(profiles: &InternalProfiles) -> RuntimeWorkflowProfiles {
    RuntimeWorkflowProfiles {
        main: profiles.main.clone(),
        text: profiles.text.clone(),
        commentary: profiles.commentary.clone(),
        reviewer: profiles.reviewer.clone(),
    }
}

fn runtime_context_contract(scoped_context: &ContextResolution) -> RuntimeContextContract {
    RuntimeContextContract {
        trace_id: scoped_context
            .context_pack
            .get("trace_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        interaction_context_id: scoped_context.interaction_context_id.clone(),
        context_pack_ref: scoped_context.context_pack_ref.clone(),
        context_pack_schema_version: context_governance::CONTEXT_SCHEMA_VERSION.to_string(),
        context_pack_digest: scoped_context.context_pack_digest.clone(),
        projections: scoped_context
            .context_projections
            .iter()
            .map(runtime_context_projection)
            .collect(),
    }
}

fn apply_question_frame_required_evidence_types(policy: &mut SearchPolicy, context_pack: &Value) {
    let Some(items) = context_pack
        .get("resolver")
        .and_then(|resolver| resolver.get("question_frame"))
        .and_then(|frame| frame.get("required_evidence_types"))
        .and_then(Value::as_array)
    else {
        return;
    };
    let mut required = policy
        .required_evidence_types
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for item in items.iter().filter_map(Value::as_str) {
        let item = item.trim();
        if !item.is_empty() {
            required.insert(item.to_string());
        }
    }
    policy.required_evidence_types = required.into_iter().collect();
}

fn runtime_context_projection(projection: &ContextProjection) -> RuntimeContextProjection {
    RuntimeContextProjection {
        context_projection_id: projection.context_projection_id.clone(),
        context_projection_ref: projection.context_projection_ref.clone(),
        context_pack_ref: projection.context_pack_ref.clone(),
        context_projection_schema_version: projection.schema_version.clone(),
        context_projection_digest: projection.digest.clone(),
        consumer_type: projection.consumer_type.clone(),
        consumer_name: projection.consumer_name.clone(),
        runtime_adapter: projection.runtime_adapter.clone(),
        projection_payload: projection.projection_payload.clone(),
        allowed_tools: projection.allowed_tools.clone(),
        forbidden_tools: projection.forbidden_tools.clone(),
        output_contract: projection.output_contract.clone(),
        tool_policy_digest: projection.tool_policy_digest.clone(),
        output_contract_digest: projection.output_contract_digest.clone(),
    }
}

fn local_runtime_context_contract(
    trace_id: &str,
    question: &str,
    profiles: &RuntimeWorkflowProfiles,
) -> Result<RuntimeContextContract> {
    let interaction_context_id = format!("local-interaction-context-{trace_id}");
    let context_pack_ref = format!("context-pack://tonglingyu/{trace_id}/local");
    let pack_payload = json!({
        "trace_id": trace_id,
        "interaction_context_id": &interaction_context_id,
        "resolved_question": question,
        "schema_version": RUNTIME_CONTEXT_PACK_SCHEMA_VERSION,
        "source": "local_runtime_gate",
    });
    let context_pack_digest = hash_value(&pack_payload)?;
    let projection_specs = [
        (
            profiles.main.as_str(),
            vec![
                "tonglingyu.evidence.package.create".to_string(),
                "tonglingyu.evidence.package.read".to_string(),
            ],
            Some("local runtime context".to_string()),
        ),
        (
            profiles.text.as_str(),
            vec!["tonglingyu.text.search".to_string()],
            None,
        ),
        (
            profiles.commentary.as_str(),
            vec!["tonglingyu.commentary.search".to_string()],
            None,
        ),
        (
            profiles.reviewer.as_str(),
            vec!["tonglingyu.evidence.package.read".to_string()],
            None,
        ),
    ];
    let mut projections = Vec::new();
    for (consumer_name, allowed_tools, session_summary) in projection_specs {
        let context_projection_id = format!("local-context-projection-{consumer_name}");
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
            "object": "tonglingyu.local_runtime_projection",
            "must_return_output_ref": true,
        });
        let tool_policy_digest = hash_value(&json!({
            "allowed_tools": &allowed_tools,
            "forbidden_tools": &forbidden_tools,
        }))?;
        let output_contract_digest = hash_value(&output_contract)?;
        let projection_unsigned = json!({
            "context_projection_id": &context_projection_id,
            "context_projection_ref": &context_projection_ref,
            "context_pack_ref": &context_pack_ref,
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
        projections.push(RuntimeContextProjection {
            context_projection_id,
            context_projection_ref,
            context_pack_ref: context_pack_ref.clone(),
            context_projection_schema_version: RUNTIME_CONTEXT_PROJECTION_SCHEMA_VERSION
                .to_string(),
            context_projection_digest: hash_value(&projection_unsigned)?,
            consumer_type: RUNTIME_CONTEXT_CONSUMER_TYPE.to_string(),
            consumer_name: consumer_name.to_string(),
            runtime_adapter: TONGLINGYU_RUNTIME_ADAPTER.to_string(),
            projection_payload,
            allowed_tools,
            forbidden_tools,
            output_contract,
            tool_policy_digest,
            output_contract_digest,
        });
    }
    Ok(RuntimeContextContract {
        trace_id: trace_id.to_string(),
        interaction_context_id,
        context_pack_ref,
        context_pack_schema_version: RUNTIME_CONTEXT_PACK_SCHEMA_VERSION.to_string(),
        context_pack_digest,
        projections,
    })
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GatewayRequestContext {
    user_ref: String,
    chat_ref: String,
    external_message_id: String,
    external_message_id_provided: bool,
    auth_subject: String,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<MessagePart>),
    Other(Value),
}

#[derive(Debug, Deserialize)]
struct MessagePart {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    q: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserFeedbackRequest {
    trace_id: Option<String>,
    package_id: Option<String>,
    feedback_type: Option<String>,
    feedback_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetrievalFailureUpdateRequest {
    human_review_status: String,
    reviewer: Option<String>,
    review_note: Option<String>,
    if_match_updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetrievalFailureClusterRequest {
    human_review_status: Option<String>,
    failure_type: Option<String>,
    min_cluster_size: Option<usize>,
    limit: Option<usize>,
    create_tasks: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernanceTaskCreateRequest {
    task_type: Option<String>,
    priority: Option<String>,
    proposed_fix: Option<String>,
    agent_cluster_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernanceTaskManualCreateRequest {
    source_entity_type: String,
    source_entity_id: String,
    trace_id: Option<String>,
    package_id: Option<String>,
    task_type: Option<String>,
    priority: Option<String>,
    proposed_fix: Option<String>,
    agent_cluster_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KnowledgePatchProposalCreateRequest {
    proposal_type: String,
    trace_id: Option<String>,
    package_id: Option<String>,
    source_ref: Option<String>,
    payload: Value,
    priority: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernanceTaskUpdateRequest {
    status: String,
    reviewer: Option<String>,
    review_note: Option<String>,
    evidence_ref: Option<String>,
    if_match_updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KnowledgeItemHumanReviewRequest {
    task_id: String,
    decision: String,
    trace_id: String,
    reviewer: String,
    review_note: String,
    evidence_ref: String,
    if_match_state_version: i64,
    if_match_task_updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdminAccessDenialRequest {
    action: Option<String>,
    denial: String,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryCollectorRunRequest {
    trigger: Option<String>,
    limit: Option<usize>,
    dry_run: Option<bool>,
    trace_id: Option<String>,
    llm_extraction_probe: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnlineEvidenceCardWorkerRunRequest {
    actor: Option<String>,
    limit: Option<usize>,
    retrieval_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryCandidateTransitionRequest {
    action: String,
    reason: Option<String>,
    candidate_type: Option<String>,
    sensitivity: Option<String>,
    merge_target_candidate_id: Option<String>,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryCardTransitionRequest {
    action: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
struct UserFeedbackSource {
    trace_id: String,
    package_id: Option<String>,
}

#[derive(Debug, Clone)]
struct KnowledgePatchProposalSource {
    trace_id: String,
    package_id: Option<String>,
}

struct UserFeedbackTaskInput<'a> {
    feedback_id: &'a str,
    trace_id: &'a str,
    package_id: Option<&'a str>,
    feedback_type: &'a str,
    feedback_text: &'a str,
    feedback_char_count: usize,
    access: &'a PackageAccessContext,
    proposed_fix: String,
}

fn metadata_string(payload: &Value, keys: &[&str]) -> Option<String> {
    let metadata = payload.get("metadata").or_else(|| payload.get("user"))?;
    keys.iter().find_map(|key| {
        metadata
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn request_context(
    headers: &HeaderMap,
    payload: &Value,
    auth_subject: String,
) -> GatewayRequestContext {
    let user_ref = header_value(headers, "x-tonglingyu-user-id")
        .or_else(|| header_value(headers, "x-open-webui-user-id"))
        .or_else(|| header_value(headers, "x-user-id"))
        .or_else(|| metadata_string(payload, &["user_id", "user", "username"]))
        .unwrap_or_else(|| auth_subject.clone());
    let chat_ref = header_value(headers, "x-tonglingyu-chat-id")
        .or_else(|| header_value(headers, "x-open-webui-chat-id"))
        .or_else(|| header_value(headers, "x-chat-id"))
        .or_else(|| metadata_string(payload, &["chat_id", "conversation_id", "session_id"]))
        .unwrap_or_else(|| "default-chat".to_string());
    let external_message_id = header_value(headers, "x-tonglingyu-message-id")
        .or_else(|| header_value(headers, "x-open-webui-message-id"))
        .or_else(|| header_value(headers, "x-open-webui-user-message-id"))
        .or_else(|| header_value(headers, "x-message-id"))
        .or_else(|| metadata_string(payload, &["user_message_id", "message_id", "request_id"]));
    let external_message_id_provided = external_message_id.is_some();
    GatewayRequestContext {
        user_ref,
        chat_ref,
        external_message_id: external_message_id
            .unwrap_or_else(|| format!("generated-{}", uuid::Uuid::now_v7().simple())),
        external_message_id_provided,
        auth_subject,
    }
}

fn forbidden_control_fields(payload: &Value) -> Vec<String> {
    const FORBIDDEN: &[&str] = &[
        "agent",
        "agent_id",
        "agent_profile",
        "agent_runtime",
        "agent_runtime_plan_gate",
        "agent_runtime_summary",
        "profile",
        "internal_agent",
        "honglou_agent",
        "runtime_profile",
        "runtime_step_outputs",
        "runtime_step_plan",
        "reviewer",
        "skip_reviewer",
        "disable_reviewer",
        "allowed_tools",
        "required_evidence_types",
        "trace_id",
        "package_id",
        "evidence_package_id",
        "admin_trace",
        "audit_events",
        "internal_trace",
        "runtime_tools_used",
        "workflow_states",
        "workflow_state",
        "tools",
        "tool_choice",
        "functions",
        "function_call",
        "parallel_tool_calls",
        "system_prompt",
        "instructions",
        "profile_config",
        "internal_config",
        "interaction_context_id",
        "context_pack_id",
        "context_pack_ref",
        "context_projection",
        "context_projection_id",
        "context_projection_ref",
        "context_projection_digest",
        "consumer_type",
        "consumer_name",
        "runtime_adapter",
        "context_scope_binding",
        "scope_id",
        "scope_graph",
        "memory_read_scopes",
        "memory_read_refs",
        "memory_read_ref_digest",
        "memory_read_policy_digest",
        "memory_write_scopes",
        "memory_scope",
        "memory_summaries",
        "memory_policy",
        "memory_policy_digest",
        "memory_usage_summary",
        "memory_candidate",
        "memory_candidate_id",
        "memory_candidate_ref",
        "memory_candidates",
        "memory_card",
        "memory_card_id",
        "memory_card_ref",
        "memory_cards",
        "memory_policy_decision",
        "memory_policy_decision_id",
        "memory_policy_decision_ref",
        "memory_policy_decisions",
        "memory_transition_audit",
        "memory_collector",
        "llm_extraction",
        "llm_filter",
        "rule_filter",
        "read_enabled",
        "forbidden_tools",
        "tool_policy_digest",
        "output_contract_digest",
        "session_journal",
    ];
    const NESTED_OBJECTS: &[&str] = &["metadata", "extra_body", "options", "parameters", "config"];
    const SHALLOW_NESTED_OBJECTS: &[&str] = &["user"];
    let mut found = Vec::new();
    if let Some(object) = payload.as_object() {
        for key in FORBIDDEN {
            if object.contains_key(*key) {
                found.push((*key).to_string());
            }
        }
        for nested_name in NESTED_OBJECTS {
            collect_forbidden_control_fields(
                nested_name,
                object.get(*nested_name),
                FORBIDDEN,
                &mut found,
            );
        }
        for nested_name in SHALLOW_NESTED_OBJECTS {
            if let Some(nested) = object.get(*nested_name).and_then(Value::as_object) {
                for key in FORBIDDEN {
                    if nested.contains_key(*key) {
                        found.push(format!("{nested_name}.{key}"));
                    }
                }
            }
        }
    }
    found
}

fn collect_forbidden_control_fields(
    prefix: &str,
    value: Option<&Value>,
    forbidden: &[&str],
    found: &mut Vec<String>,
) {
    let Some(value) = value else {
        return;
    };
    match value {
        Value::Object(object) => {
            for forbidden_key in forbidden {
                if object.contains_key(*forbidden_key) {
                    let field = format!("{prefix}.{forbidden_key}");
                    found.push(field.clone());
                }
            }
            for (key, nested) in object {
                if forbidden.iter().any(|forbidden_key| forbidden_key == key) {
                    continue;
                }
                collect_forbidden_control_fields(
                    &format!("{prefix}.{key}"),
                    Some(nested),
                    forbidden,
                    found,
                );
            }
        }
        Value::Array(items) => {
            for (index, nested) in items.iter().enumerate() {
                collect_forbidden_control_fields(
                    &format!("{prefix}[{index}]"),
                    Some(nested),
                    forbidden,
                    found,
                );
            }
        }
        _ => {}
    }
}

fn error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    trace_id: Option<&str>,
) -> Response {
    let _ = trace_id;
    let value = json!({
        "error": {
            "code": code,
            "message": message,
        }
    });
    (status, Json(value)).into_response()
}

fn safe_error_detail(_error: &anyhow::Error) -> &'static str {
    "internal details are hidden"
}

fn bounded_audit_text(value: Option<&str>, max_chars: usize) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(max_chars).collect())
    }
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    match Args::parse().command {
        Command::BuildKb(args) => {
            build_kb(&args)?;
            Ok(())
        }
        Command::Query(args) => {
            let runtime_store = TonglingyuRuntimeStore::new(args.db.clone());
            let (cards, _policy) =
                search_evidence_with_policy(&runtime_store, &args.question, args.limit)?;
            let trace_id = new_trace_id();
            let package = runtime_store.create_package(&trace_id, &args.question, cards)?;
            println!("{}", serde_json::to_string_pretty(&package)?);
            Ok(())
        }
        Command::ReplayPackage(args) => {
            let replay = TonglingyuRuntimeStore::new(args.db.clone())
                .replay_package(&args.package_id)?
                .ok_or_else(|| anyhow!("evidence package not found: {}", args.package_id))?;
            println!("{}", serde_json::to_string_pretty(&replay)?);
            Ok(())
        }
        Command::RuntimeDryRun(args) => {
            let report = runtime_dry_run(&args).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Eval(args) => {
            let report = run_eval_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "passed" {
                Ok(())
            } else {
                Err(anyhow!("tonglingyu eval failed"))
            }
        }
        Command::LlmEval(args) => {
            let report = llm_eval::run_llm_eval(
                &args.fixture_dir,
                &args.report_out,
                args.fail_on_hard_gate,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::LlmReleaseReport(args) => {
            let report = llm_eval::write_llm_release_report(&args.eval_report, &args.report_out)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::KnowledgeCalibrate(args) => {
            let data = fs::read_to_string(&args.input)
                .with_context(|| format!("read {}", args.input.display()))?;
            let input: KnowledgeCalibrationRunInput = serde_json::from_str(&data)
                .with_context(|| format!("parse {}", args.input.display()))?;
            let report = TonglingyuRuntimeStore::new(args.db.clone())
                .run_knowledge_calibration_offline(input)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::RuntimeSchemaPreflight(args) => {
            let report = TonglingyuRuntimeStore::new(args.db.clone())
                .runtime_schema_migration_preflight()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::RuntimeSchemaMigrate(args) => {
            let report = runtime_schema_migrate_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "ok" {
                Ok(())
            } else {
                Err(anyhow!("runtime schema migration did not complete"))
            }
        }
        Command::KbSourceMetadataBackfill(args) => {
            let conn = open_db(&args.db)?;
            let report = tonglingyu_runtime::backfill_source_metadata_from_snapshots(
                &conn,
                &args.source_root,
                !args.dry_run,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.get("status").and_then(Value::as_str) == Some("ok") {
                Ok(())
            } else {
                Err(anyhow!("kb source metadata backfill failed"))
            }
        }
        Command::BackupDb(args) => {
            backup_db(&args)?;
            Ok(())
        }
        Command::PruneRuntime(args) => {
            let report = prune_runtime_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::RqaRestoreCanary(args) => {
            let report = rqa_restore_canary_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "ok" {
                Ok(())
            } else {
                Err(anyhow!("rqa restore canary did not complete"))
            }
        }
        Command::RqaUserLifecycle(args) => {
            let report = rqa_user_lifecycle_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "ok" {
                Ok(())
            } else {
                Err(anyhow!("rqa user lifecycle action did not complete"))
            }
        }
        Command::MemoryCollectorRun(args) => {
            let report = memory_collector_run_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "ok" {
                Ok(())
            } else {
                Err(anyhow!("memory collector run did not complete"))
            }
        }
        Command::MemoryCandidateList(args) => {
            let report = memory_candidate_list_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::MemoryCandidateTransition(args) => {
            let report = memory_candidate_transition_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "ok" {
                Ok(())
            } else {
                Err(anyhow!("memory candidate transition did not complete"))
            }
        }
        Command::MemoryCardList(args) => {
            let report = memory_card_list_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::MemoryCardTransition(args) => {
            let report = memory_card_transition_command(&args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report["status"] == "ok" {
                Ok(())
            } else {
                Err(anyhow!("memory card transition did not complete"))
            }
        }
        Command::Healthcheck(args) => {
            let report = healthcheck_command(&args).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Serve(args) => serve(args).await,
    }
}

async fn healthcheck_command(args: &HealthcheckArgs) -> Result<Value> {
    let response = reqwest::Client::new()
        .get(&args.url)
        .timeout(Duration::from_secs(args.timeout_seconds))
        .send()
        .await
        .context("healthcheck request failed")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("healthcheck returned {status}: {}", body.trim()));
    }
    Ok(json!({
        "object": "tonglingyu.healthcheck",
        "status": "ok",
        "url": args.url,
        "http_status": status.as_u16(),
    }))
}

async fn serve(args: ServeArgs) -> Result<()> {
    let gateway_api_keys = configured_keys(args.gateway_api_key, args.gateway_api_keys);
    let admin_api_keys = configured_keys(args.admin_api_key, args.admin_api_keys);
    validate_admin_key_isolation(
        &gateway_api_keys,
        &admin_api_keys,
        args.allow_admin_with_gateway_key,
    )?;
    if args.auto_build_kb && !TonglingyuRuntimeStore::new(args.db.clone()).has_knowledge_base()? {
        let build = BuildKbArgs {
            source_root: args.source_root.clone(),
            db: args.db.clone(),
            rebuild: false,
            eval_limit: args.max_evidence,
            skip_diff_eval: true,
        };
        build_kb(&build)?;
    }
    if args.retention_days > 0 {
        let report = prune_gateway_and_runtime_data(&args.db, args.retention_days, false)?;
        tracing::info!(retention_days = args.retention_days, %report, "pruned tonglingyu runtime data");
    }
    let profiles = InternalProfiles {
        main: args.profile_main,
        text: args.profile_text,
        commentary: args.profile_commentary,
        reviewer: args.profile_reviewer,
    };
    let (agent_runtime, agent_runtime_mode, workflow_agent_provider_profiles) =
        build_workflow_agent_runtime(&profiles)?;
    let (llm_agent_runtime, llm_agent_runtime_mode, llm_agent_provider_profiles) =
        build_llm_agent_runtime()?;
    let state = Arc::new(AppState {
        db: args.db.clone(),
        runtime_store: TonglingyuRuntimeStore::new(args.db.clone()),
        model_id: args.model_id,
        model_name: args.model_name,
        upstream_base_url: args
            .upstream_base_url
            .map(|value| value.trim_end_matches('/').to_string()),
        upstream_api_key: args.upstream_api_key.filter(|value| !value.is_empty()),
        upstream_model: args.upstream_model,
        upstream_timeout_secs: args.upstream_timeout_secs,
        max_evidence: args.max_evidence,
        gateway_api_keys,
        admin_api_keys,
        allow_admin_with_gateway_key: args.allow_admin_with_gateway_key,
        max_messages: args.max_messages,
        max_question_chars: args.max_question_chars,
        max_body_bytes: args.max_body_bytes,
        rate_limit_per_minute: args.rate_limit_per_minute,
        rate_limiter: Arc::new(GatewayRateLimiter::per_minute(args.rate_limit_per_minute)),
        admin_rate_limiter: Arc::new(GatewayRateLimiter::per_minute(args.rate_limit_per_minute)),
        retention_days: args.retention_days,
        online_evidence_card_worker_enabled: args.online_evidence_card_worker_enabled,
        online_evidence_card_worker_interval_secs: args.online_evidence_card_worker_interval_secs,
        online_evidence_card_worker_batch_size: args.online_evidence_card_worker_batch_size,
        online_evidence_card_worker_retrieval_limit: args
            .online_evidence_card_worker_retrieval_limit,
        profiles,
        agent_runtime,
        agent_runtime_mode,
        llm_agent_runtime,
        llm_agent_runtime_mode,
        llm_agent_provider_profiles,
        workflow_agent_provider_profiles,
        started_at: now_rfc3339(),
    });
    if args.memory_collector_background_enabled {
        spawn_memory_collector_background_worker(
            args.db.clone(),
            args.memory_collector_interval_secs,
            args.memory_collector_batch_size,
        );
    }
    if args.online_evidence_card_worker_enabled {
        spawn_online_evidence_card_background_worker(
            args.db.clone(),
            args.online_evidence_card_worker_interval_secs,
            args.online_evidence_card_worker_batch_size,
            args.online_evidence_card_worker_retrieval_limit,
        );
    }
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/feedback", post(user_feedback_endpoint))
        .route("/v1/evidence/search", get(search_endpoint))
        .route("/v1/evidence/packages/{package_id}", get(package_endpoint))
        .route(
            "/v1/evidence/packages/{package_id}/replay",
            get(replay_package_endpoint),
        )
        .route("/v1/admin/traces/{trace_id}", get(trace_endpoint))
        .route(
            "/v1/admin/packages/{package_id}",
            get(admin_package_endpoint),
        )
        .route("/v1/admin/sessions/{session_id}", get(session_endpoint))
        .route("/v1/admin/metrics", get(metrics_endpoint))
        .route(
            "/v1/admin/metrics/prometheus",
            get(prometheus_metrics_endpoint),
        )
        .route(
            "/v1/admin/access-denials",
            post(admin_access_denial_endpoint),
        )
        .route(
            "/v1/admin/memory/collector/run",
            post(memory_collector_run_endpoint),
        )
        .route(
            "/v1/admin/evidence-card-ingest/run",
            post(online_evidence_card_worker_run_endpoint),
        )
        .route(
            "/v1/admin/memory/candidates",
            get(memory_candidates_endpoint),
        )
        .route(
            "/v1/admin/memory/candidates/{candidate_id}",
            get(memory_candidate_endpoint),
        )
        .route(
            "/v1/admin/memory/candidates/{candidate_id}/transition",
            post(memory_candidate_transition_endpoint),
        )
        .route("/v1/admin/memory/cards", get(memory_cards_endpoint))
        .route(
            "/v1/admin/memory/cards/{memory_card_id}",
            get(memory_card_endpoint),
        )
        .route(
            "/v1/admin/memory/cards/{memory_card_id}/transition",
            post(memory_card_transition_endpoint),
        )
        .route(
            "/v1/admin/retrieval-failures",
            get(retrieval_failures_endpoint),
        )
        .route(
            "/v1/admin/retrieval-failures/cluster",
            post(cluster_retrieval_failures_endpoint),
        )
        .route(
            "/v1/admin/retrieval-failures/{failure_id}",
            get(retrieval_failure_endpoint).patch(update_retrieval_failure_endpoint),
        )
        .route(
            "/v1/admin/retrieval-failures/{failure_id}/governance-task",
            post(create_governance_task_from_failure_endpoint),
        )
        .route(
            "/v1/admin/governance/tasks",
            get(governance_tasks_endpoint).post(create_governance_task_endpoint),
        )
        .route(
            "/v1/admin/governance/proposals",
            post(create_knowledge_patch_proposal_endpoint),
        )
        .route("/v1/admin/knowledge/items", get(knowledge_items_endpoint))
        .route(
            "/v1/admin/knowledge/items/{item_id}",
            get(knowledge_item_endpoint),
        )
        .route(
            "/v1/admin/knowledge/items/{item_id}/review",
            post(review_knowledge_item_endpoint),
        )
        .route(
            "/v1/admin/governance/tasks/{task_id}",
            get(governance_task_endpoint).patch(update_governance_task_endpoint),
        )
        .with_state(state)
        .layer(DefaultBodyLimit::max(args.max_body_bytes))
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(bind = %args.bind, "tonglingyu gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn spawn_memory_collector_background_worker(db: PathBuf, interval_secs: u64, batch_size: usize) {
    let interval_secs = interval_secs.max(30);
    let limit = batch_size.clamp(1, 100);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let db = db.clone();
            let result = tokio::task::spawn_blocking(move || {
                let conn = open_db(&db)?;
                run_memory_collector(
                    &conn,
                    MemoryCollectorRunInput {
                        trigger_type: "background_worker",
                        actor: "gateway-memory-collector-worker",
                        limit,
                        dry_run: false,
                        trace_id: None,
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => {
                    let run_id = report
                        .get("run_id")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let processed_count = report
                        .get("processed_count")
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    let candidate_count = report
                        .get("candidate_count")
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    let denied_count = report
                        .get("denied_count")
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    let suppressed_count = report
                        .get("suppressed_count")
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    let duplicate_count = report
                        .get("duplicate_count")
                        .and_then(Value::as_i64)
                        .unwrap_or_default();
                    tracing::info!(
                        %run_id,
                        processed_count,
                        candidate_count,
                        denied_count,
                        suppressed_count,
                        duplicate_count,
                        "memory collector background worker completed"
                    );
                }
                Ok(Err(error)) => {
                    tracing::warn!(error = %error, "memory collector background worker failed");
                }
                Err(error) => {
                    tracing::error!(error = %error, "memory collector background worker panicked");
                }
            }
        }
    });
}

fn spawn_online_evidence_card_background_worker(
    db: PathBuf,
    interval_secs: u64,
    batch_size: usize,
    retrieval_limit: usize,
) {
    let interval_secs = interval_secs.max(5);
    let limit = batch_size.clamp(1, 100);
    let retrieval_limit = retrieval_limit.clamp(1, 64);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let db = db.clone();
            let result = tokio::task::spawn_blocking(move || {
                TonglingyuRuntimeStore::new(db).run_online_evidence_card_worker_once(
                    OnlineEvidenceCardWorkerRunInput {
                        actor: "gateway-online-evidence-card-worker".to_string(),
                        limit,
                        retrieval_limit,
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => {
                    if report.processed_count > 0 {
                        tracing::info!(
                            processed_count = report.processed_count,
                            raw_candidate_count = report.raw_candidate_count,
                            staged_count = report.staged_count,
                            promoted_count = report.promoted_count,
                            conflicted_count = report.conflicted_count,
                            failed_count = report.failed_count,
                            "online evidence card background worker completed"
                        );
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(
                        error = %error,
                        "online evidence card background worker failed"
                    );
                }
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        "online evidence card background worker panicked"
                    );
                }
            }
        }
    });
}

fn build_kb(args: &BuildKbArgs) -> Result<()> {
    let before_eval_report = if args.skip_diff_eval {
        None
    } else {
        eval_report_on_db_copy(&args.db, "before-kb-rebuild", args.eval_limit)?
    };
    if args.rebuild && args.db.exists() {
        fs::remove_file(&args.db)
            .with_context(|| format!("remove existing db {}", args.db.display()))?;
    }
    if let Some(parent) = args.db.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let conn = open_db(&args.db)?;
    clear_gateway_generated_rows(&conn)?;
    let runtime_store = TonglingyuRuntimeStore::new(args.db.clone());
    let mut report = runtime_store.rebuild_knowledge_base_from_snapshots(&args.source_root)?;
    let after_eval_report = if args.skip_diff_eval {
        None
    } else {
        eval_report_on_db_copy(&args.db, "after-kb-rebuild", args.eval_limit)?
    };
    if let Some(after_eval_report) = after_eval_report.as_ref() {
        let before_eval_summary = before_eval_report
            .as_ref()
            .and_then(|report| report.get("quality_summary"))
            .cloned();
        let after_eval_summary = after_eval_report
            .get("quality_summary")
            .cloned()
            .ok_or_else(|| anyhow!("after rebuild eval report missing quality_summary"))?;
        let report_id = report
            .diff_report
            .get("report_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("kb diff report missing report_id"))?;
        report.diff_report = runtime_store
            .record_kb_version_diff_eval_summaries(
                report_id,
                before_eval_summary,
                after_eval_summary,
            )?
            .ok_or_else(|| anyhow!("kb diff report not found after eval summary update"))?;
        if after_eval_report.get("status").and_then(Value::as_str) != Some("passed") {
            return Err(anyhow!("post-rebuild eval quality failed"));
        }
    }
    println!(
        "OK build_kb db={} source_root={} kb_version={} sources={} blocks={} schema={} kb_build_hash={} diff_report={} eval_diff={}",
        args.db.display(),
        report.source_root,
        report.version_id,
        report.source_count,
        report.block_count,
        report.schema_version,
        report.kb_build_hash,
        report
            .diff_report
            .get("report_id")
            .and_then(Value::as_str)
            .unwrap_or("missing"),
        if args.skip_diff_eval {
            "skipped"
        } else {
            "recorded"
        }
    );
    Ok(())
}

fn remove_sqlite_file_set(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("db-wal"));
    let _ = fs::remove_file(path.with_extension("db-shm"));
}

fn clear_gateway_generated_rows(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        DELETE FROM memory_collector_leases;
        DELETE FROM memory_collector_journal_status;
        DELETE FROM memory_collector_runs;
        DELETE FROM memory_policy_decisions;
        DELETE FROM memory_transition_audit;
        DELETE FROM memory_cards;
        DELETE FROM memory_candidates;
        DELETE FROM session_journal;
        DELETE FROM context_packs;
        DELETE FROM context_scope_bindings;
        DELETE FROM interaction_contexts;
        DELETE FROM user_sessions;
        DELETE FROM gateway_messages;
        DELETE FROM gateway_sessions;
        DELETE FROM workflow_states;
        "#,
    )?;
    Ok(())
}

fn backup_db(args: &BackupDbArgs) -> Result<()> {
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let conn = open_db(&args.db)?;
    conn.execute("VACUUM INTO ?1", params![args.output.display().to_string()])?;
    println!(
        "OK backup_db db={} output={}",
        args.db.display(),
        args.output.display()
    );
    Ok(())
}

fn runtime_schema_migrate_command(args: &RuntimeSchemaMigrateArgs) -> Result<Value> {
    let store = TonglingyuRuntimeStore::new(args.db.clone());
    let before = store.runtime_schema_migration_preflight()?;
    if let Some(parent) = args.db.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let conn = open_db(&args.db)?;
    tonglingyu_runtime::init_runtime_schema(&conn)?;
    let after = store.runtime_schema_migration_preflight()?;
    let before_pending = before
        .get("pending_migrations")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let after_pending = after
        .get("pending_migrations")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    Ok(json!({
        "object": "tonglingyu.runtime_schema_migration_apply",
        "schema_version": 1,
        "status": if after_pending == 0 { "ok" } else { "failed" },
        "db_path_sha256": hash_text(&args.db.display().to_string()),
        "before": before,
        "after": after,
        "pending_before": before_pending,
        "pending_after": after_pending,
        "applied_count": before_pending.saturating_sub(after_pending),
        "will_rebuild_knowledge_base": false,
        "will_delete_runtime_data": false,
        "secret_values_printed": false,
    }))
}

fn prune_runtime_command(args: &PruneRuntimeArgs) -> Result<Value> {
    prune_gateway_and_runtime_data(&args.db, args.retention_days, args.dry_run)
}

fn rqa_restore_canary_command(args: &RqaRestoreCanaryArgs) -> Result<Value> {
    if !args.db.is_file() {
        return Err(anyhow!(
            "restore canary db not found: {}",
            args.db.display()
        ));
    }
    let reviewer = args.reviewer.trim();
    if reviewer.is_empty() {
        return Err(anyhow!("--reviewer must not be empty"));
    }
    let review_note = args.review_note.trim();
    if review_note.is_empty() {
        return Err(anyhow!("--review-note must not be empty"));
    }

    let conn = open_db(&args.db)?;
    tonglingyu_runtime::init_runtime_schema(&conn)?;
    let package = resolve_restore_canary_package(&args.db, args.package_id.as_deref())?;
    let package_id = package.package_id.clone();
    let trace_id = package.trace_id.clone();
    let started_at = now_rfc3339();
    let (failure, task) = run_immediate_transaction(&conn, |tx| {
        let report = restore_canary_quality_report(&package);
        let selected_evidence_ids = package
            .cards
            .iter()
            .map(|card| card.evidence_id.clone())
            .collect::<Vec<_>>();
        let failure = tonglingyu_runtime::create_retrieval_failure(
            tx,
            RetrievalFailureCreateInput {
                trace_id: trace_id.clone(),
                package_id: Some(package_id.clone()),
                question: restore_canary_question(),
                quality_report: report,
                selected_evidence_ids: selected_evidence_ids.clone(),
                expected_evidence_ids: selected_evidence_ids,
                agent_diagnosis: Some(
                    "restore_drill_canary_reference_only; no_direct_fact_mutation=true".to_string(),
                ),
                proposed_fix: Some(
                    "close restore drill canary after backup/restore reference verification"
                        .to_string(),
                ),
            },
        )?;
        let task = tonglingyu_runtime::create_governance_task_from_failure(
            tx,
            KnowledgeGovernanceTaskCreateFromFailureInput {
                source_failure_id: failure.failure_id.clone(),
                task_type: Some("expert_review".to_string()),
                priority: Some("p1".to_string()),
                proposed_fix: Some(
                    "restore drill canary closed after verification; no knowledge mutation required"
                        .to_string(),
                ),
                agent_cluster_key: Some(format!(
                    "restore-drill-canary:{}",
                    &hash_text(&package_id)[..16]
                )),
            },
        )?
        .ok_or_else(|| anyhow!("restore canary governance task was not created"))?;
        let failure = tonglingyu_runtime::update_retrieval_failure_status(
            tx,
            &failure.failure_id,
            "resolved",
            Some(reviewer),
            Some(review_note),
        )?
        .ok_or_else(|| anyhow!("restore canary retrieval failure was not readable"))?;
        let task = tonglingyu_runtime::update_governance_task(
            tx,
            &task.task_id,
            KnowledgeGovernanceTaskUpdateInput {
                status: "closed".to_string(),
                reviewer: Some(reviewer.to_string()),
                review_note: Some(review_note.to_string()),
                evidence_ref: Some(format!("package:{package_id}")),
                expected_updated_at: Some(task.updated_at.clone()),
            },
        )?
        .ok_or_else(|| anyhow!("restore canary governance task was not readable"))?;
        append_runtime_audit_event(
            tx,
            &trace_id,
            "rqa_restore_canary_recorded",
            &json!({
                "failure_id": &failure.failure_id,
                "task_id": &task.task_id,
                "package_id": &package_id,
                "failure_type": &failure.failure_type,
                "failure_status": &failure.human_review_status,
                "task_status": &task.status,
                "task_priority": &task.priority,
                "reviewer": reviewer,
                "review_note_sha256": hash_text(review_note),
                "direct_fact_mutation": false,
                "raw_question_included": false,
                "secret_values_printed": false,
            }),
        )?;
        Ok((failure, task))
    })?;
    let open_p0 = restore_canary_open_p0_counts(&conn)?;

    Ok(json!({
        "object": "tonglingyu.rqa_restore_canary",
        "schema_version": RQA_RESTORE_CANARY_SCHEMA_VERSION,
        "status": "ok",
        "started_at": started_at,
        "finished_at": now_rfc3339(),
        "db_path_sha256": hash_text(&args.db.display().to_string()),
        "refs": {
            "trace_id": trace_id,
            "package_id": package_id,
            "failure_id": failure.failure_id,
            "task_id": task.task_id,
        },
        "checks": {
            "failure_type": failure.failure_type,
            "failure_status": failure.human_review_status,
            "task_status": task.status,
            "task_priority": task.priority,
            "open_p0_retrieval_failures": open_p0.0,
            "open_p0_governance_tasks": open_p0.1,
            "direct_fact_mutation": false,
        },
        "raw_question_included": false,
        "secret_values_printed": false,
    }))
}

fn resolve_restore_canary_package(
    db: &Path,
    requested_package_id: Option<&str>,
) -> Result<EvidencePackage> {
    let runtime_store = TonglingyuRuntimeStore::new(db.to_path_buf());
    if let Some(package_id) = requested_package_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return runtime_store
            .read_package(package_id)?
            .ok_or_else(|| anyhow!("restore canary package not found: {package_id}"));
    }
    runtime_store
        .latest_package()?
        .ok_or_else(|| anyhow!("restore canary requires at least one evidence package"))
}

fn restore_canary_question() -> String {
    "restore drill canary reference".to_string()
}

fn restore_canary_quality_report(package: &EvidencePackage) -> RetrievalQualityReport {
    let question = restore_canary_question();
    let selected_types = package
        .cards
        .iter()
        .map(|card| card.evidence_type.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let channel_distribution =
        package
            .cards
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut counts, card| {
                *counts.entry(card.evidence_type.clone()).or_insert(0) += 1;
                counts
            });
    RetrievalQualityReport {
        object: "tonglingyu.retrieval_quality_report".to_string(),
        schema_version: RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION.to_string(),
        tool_name: "tonglingyu.rqa_restore_canary".to_string(),
        quality_status: "failed".to_string(),
        production_ready: false,
        truncated: false,
        query_summary: RetrievalQuerySummary {
            question_sha256: hash_text(&question),
            question_char_count: question.chars().count(),
            raw_question_included: false,
            redacted_terms: vec!["restore-drill-canary".to_string()],
        },
        expanded_terms: Vec::new(),
        protected_terms: Vec::new(),
        expanded_aliases: Vec::new(),
        normalized_match_channels: BTreeMap::new(),
        candidate_count: package.cards.len(),
        selected_count: package.cards.len(),
        channel_distribution,
        evidence_type_coverage: RetrievalEvidenceTypeCoverage {
            required: selected_types.clone(),
            selected: selected_types,
            missing: Vec::new(),
        },
        exact_match_coverage: Vec::new(),
        expected_evidence_hit: Some(true),
        expected_evidence_status: "restore_drill_canary".to_string(),
        source_coverage_boundary: RetrievalSourceCoverageBoundary {
            source_ids: package
                .cards
                .iter()
                .map(|card| card.source_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            source_categories: package
                .cards
                .iter()
                .map(|card| card.evidence_type.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            edition_boundaries: package
                .cards
                .iter()
                .map(|card| card.source_title.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            kb_schema_version: KNOWLEDGE_BASE_SCHEMA_VERSION.to_string(),
            source_snapshot_status: "source_snapshot_ready".to_string(),
            facsimile_review_status: "not_reviewed".to_string(),
            authoritative_edition_review_status: "not_reviewed".to_string(),
            scholarly_collation_status: "not_scholarly_collated".to_string(),
            expert_collation_status: "restore_drill_canary_reviewed".to_string(),
        },
        source_usage_refs: Vec::new(),
        issues: vec!["restore_drill_canary".to_string()],
        recommended_follow_up: vec![
            "restore_drill_canary_closed_no_knowledge_mutation".to_string(),
        ],
    }
}

fn restore_canary_open_p0_counts(conn: &Connection) -> Result<(i64, i64)> {
    let open_failures = conn.query_row(
        "SELECT COUNT(*) FROM retrieval_failures WHERE human_review_status IN ('open', 'in_review')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let open_tasks = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_governance_tasks WHERE priority = 'p0' AND status IN ('open', 'in_review', 'accepted')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    Ok((open_failures, open_tasks))
}

fn prune_gateway_and_runtime_data(db: &Path, retention_days: u32, dry_run: bool) -> Result<Value> {
    let runtime_store = TonglingyuRuntimeStore::new(db.to_path_buf());
    if retention_days == 0 {
        return runtime_store.prune_data(retention_days, dry_run);
    }
    let mut report = runtime_store.prune_data(retention_days, dry_run)?;
    let conn = open_db(db)?;
    let cutoff = report
        .get("cutoff")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("runtime prune report missing cutoff"))?
        .to_string();
    if dry_run {
        let plan = build_gateway_prune_plan(&conn, &cutoff)?;
        merge_gateway_prune_counts(&mut report, &plan, 0);
        return Ok(report);
    }
    let (plan, gateway_tombstones) = run_immediate_transaction(&conn, |tx| {
        let plan = build_gateway_prune_plan(tx, &cutoff)?;
        let mut gateway_tombstones = 0_i64;
        if plan.prunable_messages > 0 {
            append_rqa_lifecycle_tombstone(
                tx,
                "gateway_message_batch",
                &format!("gateway_messages:{}:{}", cutoff, plan.prunable_messages),
                "retention_prune",
                "retention_expired",
                &json!({
                    "object_type": "gateway_message_batch",
                    "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                    "row_count": plan.prunable_messages,
                    "protected_row_count": plan.message_candidates - plan.prunable_messages,
                    "retention_days": retention_days,
                    "cutoff": cutoff,
                    "raw_question_included": false,
                    "raw_response_included": false,
                    "secret_values_printed": false,
                }),
            )?;
            gateway_tombstones += 1;
        }
        tx.execute(
            &format!(
                "DELETE FROM gateway_messages WHERE {}",
                plan.message_prune_predicate
            ),
            params![&cutoff],
        )?;
        if plan.prunable_workflow_states > 0 {
            append_rqa_lifecycle_tombstone(
                tx,
                "workflow_state_batch",
                &format!(
                    "workflow_states:{}:{}",
                    cutoff, plan.prunable_workflow_states
                ),
                "retention_prune",
                "retention_expired",
                &json!({
                    "object_type": "workflow_state_batch",
                    "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                    "row_count": plan.prunable_workflow_states,
                    "protected_row_count": plan.workflow_candidates - plan.prunable_workflow_states,
                    "retention_days": retention_days,
                    "cutoff": cutoff,
                    "raw_detail_included": false,
                    "secret_values_printed": false,
                }),
            )?;
            gateway_tombstones += 1;
        }
        tx.execute(
            &format!(
                "DELETE FROM workflow_states WHERE {}",
                plan.workflow_prune_predicate
            ),
            params![&cutoff],
        )?;
        if plan.prunable_sessions > 0 {
            append_rqa_lifecycle_tombstone(
                tx,
                "gateway_session_batch",
                &format!("gateway_sessions:{}:{}", cutoff, plan.prunable_sessions),
                "retention_prune",
                "retention_expired",
                &json!({
                    "object_type": "gateway_session_batch",
                    "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                    "row_count": plan.prunable_sessions,
                    "protected_row_count": plan.session_candidates - plan.prunable_sessions,
                    "retention_days": retention_days,
                    "cutoff": cutoff,
                    "raw_user_ref_included": false,
                    "raw_chat_ref_included": false,
                    "secret_values_printed": false,
                }),
            )?;
            gateway_tombstones += 1;
        }
        tx.execute(
            &format!(
                "DELETE FROM gateway_sessions WHERE {}",
                plan.session_prune_predicate
            ),
            params![&cutoff],
        )?;
        Ok((plan, gateway_tombstones))
    })?;
    merge_gateway_prune_counts(&mut report, &plan, gateway_tombstones);
    Ok(report)
}

fn memory_collector_run_command(args: &MemoryCollectorRunArgs) -> Result<Value> {
    let conn = open_db(&args.db)?;
    run_memory_collector(
        &conn,
        MemoryCollectorRunInput {
            trigger_type: args.trigger.trim(),
            actor: args.actor.trim(),
            limit: args.limit,
            dry_run: args.dry_run,
            trace_id: args
                .trace_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
        },
    )
}

fn memory_candidate_list_command(args: &MemoryCandidateListArgs) -> Result<Value> {
    let conn = open_db(&args.db)?;
    list_memory_candidates(
        &conn,
        MemoryCandidateListInput {
            status: args
                .status
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            scope_type: args
                .scope_type
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            scope_ref: args
                .scope_ref
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            limit: args.limit,
            offset: args.offset,
        },
    )
}

fn memory_candidate_transition_command(args: &MemoryCandidateTransitionArgs) -> Result<Value> {
    let conn = open_db(&args.db)?;
    transition_memory_candidate(
        &conn,
        MemoryCandidateTransitionInput {
            candidate_id: args.candidate_id.trim(),
            action: args.action.trim(),
            actor: args.actor.trim(),
            reason: args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            candidate_type: args
                .candidate_type
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            sensitivity: args
                .sensitivity
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            merge_target_candidate_id: args
                .merge_target_candidate_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            expires_at: args
                .expires_at
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
        },
    )
}

fn memory_card_list_command(args: &MemoryCardListArgs) -> Result<Value> {
    let conn = open_db(&args.db)?;
    list_memory_cards(
        &conn,
        MemoryCardListInput {
            status: args
                .status
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            scope_type: args
                .scope_type
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            scope_ref: args
                .scope_ref
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            limit: args.limit,
            offset: args.offset,
        },
    )
}

fn memory_card_transition_command(args: &MemoryCardTransitionArgs) -> Result<Value> {
    let conn = open_db(&args.db)?;
    transition_memory_card(
        &conn,
        MemoryCardTransitionInput {
            memory_card_id: args.memory_card_id.trim(),
            action: args.action.trim(),
            actor: args.actor.trim(),
            reason: args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
        },
    )
}

#[derive(Debug)]
struct GatewayPrunePlan {
    message_prune_predicate: String,
    workflow_prune_predicate: String,
    session_prune_predicate: String,
    message_candidates: i64,
    prunable_messages: i64,
    workflow_candidates: i64,
    prunable_workflow_states: i64,
    session_candidates: i64,
    prunable_sessions: i64,
}

fn build_gateway_prune_plan(conn: &Connection, cutoff: &str) -> Result<GatewayPrunePlan> {
    let message_prune_predicate = format!(
        "created_at < ?1 AND NOT ({})",
        gateway_rqa_protection_predicate("gateway_messages")
    );
    let workflow_prune_predicate = format!(
        "created_at < ?1 AND NOT ({})",
        gateway_rqa_protection_predicate("workflow_states")
    );
    let session_prune_predicate = gateway_session_prune_predicate();
    let message_candidates = count_where(conn, "gateway_messages", "created_at < ?1", cutoff)?;
    let prunable_messages =
        count_where(conn, "gateway_messages", &message_prune_predicate, cutoff)?;
    let workflow_candidates = count_where(conn, "workflow_states", "created_at < ?1", cutoff)?;
    let prunable_workflow_states =
        count_where(conn, "workflow_states", &workflow_prune_predicate, cutoff)?;
    let session_candidates = count_where(conn, "gateway_sessions", "updated_at < ?1", cutoff)?;
    let prunable_sessions =
        count_where(conn, "gateway_sessions", &session_prune_predicate, cutoff)?;
    Ok(GatewayPrunePlan {
        message_prune_predicate,
        workflow_prune_predicate,
        session_prune_predicate,
        message_candidates,
        prunable_messages,
        workflow_candidates,
        prunable_workflow_states,
        session_candidates,
        prunable_sessions,
    })
}

fn merge_gateway_prune_counts(
    report: &mut Value,
    plan: &GatewayPrunePlan,
    gateway_tombstones: i64,
) {
    let gateway_counts = json!({
        "gateway_message_candidates": plan.message_candidates,
        "gateway_messages": plan.prunable_messages,
        "protected_gateway_messages": plan.message_candidates - plan.prunable_messages,
        "workflow_state_candidates": plan.workflow_candidates,
        "workflow_states": plan.prunable_workflow_states,
        "protected_workflow_states": plan.workflow_candidates - plan.prunable_workflow_states,
        "gateway_session_candidates": plan.session_candidates,
        "gateway_sessions": plan.prunable_sessions,
        "protected_gateway_sessions": plan.session_candidates - plan.prunable_sessions,
        "gateway_tombstone_candidates": i64::from(plan.prunable_messages > 0)
            + i64::from(plan.prunable_workflow_states > 0)
            + i64::from(plan.prunable_sessions > 0),
        "gateway_tombstones": gateway_tombstones,
    });
    if let Some(counts) = report.get_mut("counts").and_then(Value::as_object_mut) {
        for (key, value) in gateway_counts.as_object().expect("gateway counts object") {
            counts.insert(key.to_string(), value.clone());
        }
        if let Some(existing) = counts.get("tombstones").and_then(Value::as_i64) {
            counts.insert(
                "tombstones".to_string(),
                json!(existing + gateway_tombstones),
            );
        }
    }
}

fn run_immediate_transaction<T>(
    conn: &Connection,
    work: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match work(conn) {
        Ok(value) => {
            if let Err(error) = conn.execute_batch("COMMIT") {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error.into())
            } else {
                Ok(value)
            }
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn gateway_rqa_protection_predicate(table_alias: &str) -> String {
    format!(
        r#"
        EXISTS (
            SELECT 1 FROM retrieval_failures AS rf
            WHERE rf.human_review_status IN ('open', 'in_review')
              AND (rf.trace_id = {table_alias}.trace_id OR rf.package_id = {table_alias}.package_id)
        )
        OR EXISTS (
            SELECT 1 FROM knowledge_governance_tasks AS kgt
            WHERE kgt.status IN ('open', 'in_review', 'accepted')
              AND (kgt.trace_id = {table_alias}.trace_id OR kgt.package_id = {table_alias}.package_id)
        )
        "#
    )
}

fn gateway_session_prune_predicate() -> String {
    let message_protection = gateway_rqa_protection_predicate("gm");
    let workflow_protection = gateway_rqa_protection_predicate("ws");
    format!(
        r#"
        updated_at < ?1
        AND NOT EXISTS (
            SELECT 1 FROM gateway_messages AS gm
            WHERE gm.session_id = gateway_sessions.session_id
              AND NOT (gm.created_at < ?1 AND NOT ({message_protection}))
        )
        AND NOT EXISTS (
            SELECT 1 FROM workflow_states AS ws
            WHERE ws.session_id = gateway_sessions.session_id
              AND NOT (ws.created_at < ?1 AND NOT ({workflow_protection}))
        )
        "#
    )
}

fn count_where(conn: &Connection, table: &str, predicate: &str, value: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
    conn.query_row(&sql, params![value], |row| row.get(0))
        .map_err(Into::into)
}

fn open_db(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("open sqlite db {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    init_gateway_schema(&conn)?;
    Ok(conn)
}

fn init_gateway_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            migration_id TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS gateway_sessions (
            session_id TEXT PRIMARY KEY,
            user_ref TEXT NOT NULL,
            chat_ref TEXT NOT NULL,
            model_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(user_ref, chat_ref, model_id)
        );

        CREATE TABLE IF NOT EXISTS gateway_messages (
            message_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES gateway_sessions(session_id),
            external_message_id TEXT NOT NULL,
            trace_id TEXT NOT NULL,
            package_id TEXT,
            request_hash TEXT NOT NULL,
            question TEXT NOT NULL,
            response_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(session_id, external_message_id)
        );

        CREATE TABLE IF NOT EXISTS workflow_states (
            state_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            session_id TEXT,
            package_id TEXT,
            state TEXT NOT NULL,
            status TEXT NOT NULL,
            detail_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS rqa_user_legal_holds (
            hold_id TEXT PRIMARY KEY,
            user_ref_sha256 TEXT NOT NULL,
            reason TEXT NOT NULL,
            active INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            released_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_gateway_messages_session ON gateway_messages(session_id);
        CREATE INDEX IF NOT EXISTS idx_gateway_messages_trace ON gateway_messages(trace_id);
        CREATE INDEX IF NOT EXISTS idx_gateway_messages_package ON gateway_messages(package_id);
        CREATE INDEX IF NOT EXISTS idx_workflow_states_trace ON workflow_states(trace_id);
        CREATE INDEX IF NOT EXISTS idx_workflow_states_package ON workflow_states(package_id);
        CREATE INDEX IF NOT EXISTS idx_rqa_user_legal_holds_subject
            ON rqa_user_legal_holds(user_ref_sha256, active);
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params!["tonglingyu-gateway-schema-v1", now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params!["tonglingyu-rqa-user-lifecycle-v1", now_rfc3339()],
    )?;
    context_governance::init_schema(conn)?;
    Ok(())
}

fn search_evidence_with_policy(
    runtime_store: &TonglingyuRuntimeStore,
    question: &str,
    limit: usize,
) -> Result<(Vec<EvidenceCard>, SearchPolicy)> {
    let policy = search_policy(question);
    let cards = runtime_store.search_cards(question, limit, &policy.required_evidence_types)?;
    Ok((cards, policy))
}

fn insert_audit_event(
    conn: &Connection,
    trace_id: &str,
    event_type: &str,
    payload: &Value,
) -> Result<()> {
    tonglingyu_runtime::append_runtime_audit_event(conn, trace_id, event_type, payload)
}

fn record_workflow_state(
    conn: &Connection,
    trace_id: &str,
    session_id: Option<&str>,
    package_id: Option<&str>,
    state: &str,
    status: &str,
    detail: &Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO workflow_states (state_id, trace_id, session_id, package_id, state, status, detail_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            format!("state-{}", uuid::Uuid::now_v7().simple()),
            trace_id,
            session_id,
            package_id,
            state,
            status,
            serde_json::to_string(detail)?,
            now_rfc3339(),
        ],
    )?;
    insert_audit_event(
        conn,
        trace_id,
        "workflow_state",
        &json!({
            "session_id": session_id,
            "package_id": package_id,
            "state": state,
            "status": status,
            "detail": detail,
        }),
    )?;
    Ok(())
}

fn hash_value(value: &Value) -> Result<String> {
    let data = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(data);
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn runtime_dry_run(args: &RuntimeDryRunArgs) -> Result<Value> {
    if args.limit == 0 {
        return Err(anyhow!("--limit must be greater than 0"));
    }
    let started = Instant::now();
    let runtime_store = TonglingyuRuntimeStore::new(args.db.clone());
    let agent_runtime_mode = TonglingyuAgentRuntimeMode::from_env()?;
    let trace_id = format!("dryrun-{}", new_trace_id());
    let profiles = InternalProfiles {
        main: "honglou-main".to_string(),
        text: "honglou-text".to_string(),
        commentary: "honglou-commentary".to_string(),
        reviewer: "honglou-reviewer".to_string(),
    };
    let mut policy = search_policy(&args.question);
    policy.planned_profiles = planned_profiles_for_policy(&profiles, &policy);
    let runtime_step_plan = RuntimeStepPlan::from_policy(&profiles, &policy);
    let runtime_profiles = runtime_workflow_profiles(&profiles);
    let runtime_context =
        local_runtime_context_contract(&trace_id, &args.question, &runtime_profiles)?;
    let agent_runtime_plan_gate = execute_agent_runtime_plan_gate(AgentRuntimePlanGateInput {
        trace_id: trace_id.clone(),
        question: args.question.clone(),
        required_evidence_types: policy.required_evidence_types.clone(),
        profiles: runtime_profiles.clone(),
        context: runtime_context.clone(),
    })
    .await?;
    let workflow = runtime_store
        .execute_workflow_with_agent_runtime_steps(RuntimeWorkflowInput {
            trace_id: trace_id.clone(),
            question: args.question.clone(),
            limit: args.limit,
            required_evidence_types: policy.required_evidence_types.clone(),
            profiles: runtime_profiles,
            context: runtime_context,
        })
        .await?;
    let package = workflow.package;
    let replay = runtime_store
        .replay_package(&package.package_id)?
        .ok_or_else(|| anyhow!("runtime dry run package replay missing"))?;
    Ok(json!({
        "object": "tonglingyu.runtime_dry_run",
        "status": "passed",
        "trace_id": trace_id,
        "question": &args.question,
        "policy": policy,
        "runtime_step_plan": runtime_step_plan,
        "agent_runtime_plan_gate": agent_runtime_plan_gate,
        "agent_runtime": {
            "mode": agent_runtime_mode.as_str(),
            "config_source": "TONGLINGYU_AGENT_ROLE_TEXT/PACKAGE/DRAFT/REVIEW_PROVIDER",
            "summary": &workflow.agent_runtime_summary,
        },
        "runtime_step_outputs": workflow.steps,
        "runtime_stream_events": workflow.stream_events,
        "package_id": &package.package_id,
        "review": &package.review,
        "final_answer": workflow.final_answer,
        "replay": replay,
        "elapsed_ms": elapsed_ms(started),
        "checks": {
            "card_count": package.cards.len(),
            "claim_count": package.claims.len(),
            "reviewer_enforced": true,
            "agent_runtime_plan_gate": "passed",
            "agent_runtime_profile_execution_status": workflow.agent_runtime_summary
                .get("profile_execution_status")
                .cloned()
                .unwrap_or(Value::Null),
            "profile_step_count": workflow.steps.len(),
            "runtime_stream_event_count": workflow.stream_events.len(),
            "runtime_tools_used": [
                "tonglingyu.text.search",
                "tonglingyu.evidence.package.create",
                "tonglingyu.evidence.package.replay"
            ],
        },
    }))
}

async fn healthz(State(state): State<Arc<AppState>>) -> Response {
    match (
        state.runtime_store.store_stats(),
        state.runtime_store.online_evidence_card_ingest_stats(),
    ) {
        (Ok(stats), Ok(online_evidence_card_ingest)) => Json(json!({
            "status": "ok",
            "model": state.model_id,
            "agent_runtime": {
                "mode": state.agent_runtime_mode.as_str(),
                "config_source": "TONGLINGYU_AGENT_ROLE_TEXT/PACKAGE/DRAFT/REVIEW_PROVIDER",
                "provider_profiles": &state.workflow_agent_provider_profiles,
            },
            "llm_agent_runtime": {
                "mode": &state.llm_agent_runtime_mode,
                "config_source": "TONGLINGYU_AGENT_ROLE_*_PROVIDER",
                "provider_profiles": &state.llm_agent_provider_profiles,
            },
            "rate_limit": {
                "public_per_minute": state.rate_limit_per_minute,
                "env": "TONGLINGYU_RATE_LIMIT_PER_MINUTE",
                "disabled": state.rate_limit_per_minute == 0,
            },
            "request_limits": {
                "max_messages": state.max_messages,
                "max_question_chars": state.max_question_chars,
                "max_body_bytes": state.max_body_bytes,
            },
            "online_evidence_card_ingest": {
                "worker_enabled": state.online_evidence_card_worker_enabled,
                "worker_interval_secs": state.online_evidence_card_worker_interval_secs,
                "worker_batch_size": state.online_evidence_card_worker_batch_size,
                "worker_retrieval_limit": state.online_evidence_card_worker_retrieval_limit,
                "stats": online_evidence_card_ingest,
            },
            "sources": stats.sources,
            "blocks": stats.blocks
        }))
        .into_response(),
        (Err(error), _) | (_, Err(error)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "degraded",
                "error": "health_check_failed",
                "detail": safe_error_detail(&error),
            })),
        )
            .into_response(),
    }
}

async fn models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(response) = gateway_auth_and_rate_limit(&state, &headers, None) {
        return *response;
    }
    Json(json!({
        "object": "list",
        "data": [{
            "id": state.model_id,
            "object": "model",
            "owned_by": "tonglingyu",
            "name": state.model_name,
            "description": "红楼文本证据与脂批问答系统"
        }]
    }))
    .into_response()
}

async fn search_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Response {
    if let Err(response) = gateway_auth_and_rate_limit(&state, &headers, None) {
        return *response;
    }
    match search_evidence_with_policy(&state.runtime_store, &params.q, params.limit.unwrap_or(8)) {
        Ok((cards, policy)) => Json(json!({
            "object": "list",
            "data": cards,
            "policy": public_search_policy(&policy),
        }))
        .into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "evidence search failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "search_failed",
                "evidence search failed",
                None,
            )
        }
    }
}

async fn package_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(package_id): AxumPath<String>,
) -> Response {
    let subject = match gateway_auth_and_rate_limit(&state, &headers, None) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let access = package_access_context(&headers, subject);
    match load_package_for_subject(&state.db, &package_id, &access) {
        Ok(Some(package)) => Json(package).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => {
            tracing::warn!(package_id = %package_id, error = %error, "package load failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "package_load_failed",
                "evidence package load failed",
                None,
            )
        }
    }
}

async fn replay_package_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(package_id): AxumPath<String>,
) -> Response {
    let subject = match gateway_auth_and_rate_limit(&state, &headers, None) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let access = package_access_context(&headers, subject);
    match load_package_replay_for_subject(&state.db, &package_id, &access) {
        Ok(Some(replay)) => Json(replay).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => {
            tracing::warn!(package_id = %package_id, error = %error, "package replay failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "package_replay_failed",
                "evidence package replay failed",
                None,
            )
        }
    }
}

async fn user_feedback_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserFeedbackRequest>,
) -> Response {
    let subject = match gateway_auth_and_rate_limit(&state, &headers, None) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let access = package_access_context(&headers, subject);
    let feedback_text = payload.feedback_text.trim();
    if feedback_text.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "feedback_text_required",
            "feedback_text is required",
            None,
        );
    }
    let feedback_char_count = feedback_text.chars().count();
    if feedback_char_count > USER_FEEDBACK_MAX_CHARS {
        return error_response(
            StatusCode::BAD_REQUEST,
            "feedback_text_too_long",
            "feedback_text is too long",
            None,
        );
    }
    let feedback_type = match normalize_user_feedback_type(payload.feedback_type.as_deref()) {
        Ok(feedback_type) => feedback_type,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_feedback_type",
                &error.to_string(),
                None,
            );
        }
    };
    let requested_trace_id = payload
        .trace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_package_id = payload
        .package_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if requested_trace_id.is_none() && requested_package_id.is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "feedback_source_required",
            "trace_id or package_id is required",
            None,
        );
    }
    let source = match resolve_user_feedback_source(
        &state.db,
        &state.runtime_store,
        &access,
        requested_trace_id,
        requested_package_id,
    ) {
        Ok(Some(source)) => source,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "feedback_source_not_found",
                "feedback source was not found for this user",
                None,
            );
        }
        Err(error) => {
            if error.to_string().contains("mismatch") {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "feedback_source_mismatch",
                    "feedback trace and package do not match",
                    None,
                );
            }
            tracing::warn!(error = %error, "user feedback source resolution failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "feedback_source_failed",
                "feedback source lookup failed",
                None,
            );
        }
    };

    let feedback_id = format!("uf-{}", uuid::Uuid::now_v7().simple());
    let proposed_fix = user_feedback_proposed_fix(&feedback_type, feedback_text);
    match create_user_feedback_governance_task(
        &state.db,
        UserFeedbackTaskInput {
            feedback_id: &feedback_id,
            trace_id: &source.trace_id,
            package_id: source.package_id.as_deref(),
            feedback_type: &feedback_type,
            feedback_text,
            feedback_char_count,
            access: &access,
            proposed_fix,
        },
    ) {
        Ok(record) => Json(json!({
            "object": "tonglingyu.user_feedback",
            "schema_version": USER_FEEDBACK_SCHEMA_VERSION,
            "feedback_id": feedback_id,
            "status": "queued_for_human_review",
            "direct_fact_mutation": false,
            "task": {
                "task_id": record.task_id,
                "status": record.status,
                "priority": record.priority,
                "source_entity_type": record.source_entity_type,
                "source_entity_id": record.source_entity_id,
                "task_type": record.task_type,
            },
            "trace_id": source.trace_id,
            "package_id": source.package_id,
        }))
        .into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "user feedback task create failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "feedback_create_failed",
                "feedback could not be queued",
                None,
            )
        }
    }
}

async fn trace_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(trace_id): AxumPath<String>,
) -> Response {
    if let Err(response) = admin_auth_and_rate_limit(&state, &headers, "trace_read") {
        return *response;
    }
    match load_trace(&state.db, &trace_id) {
        Ok(Some(trace)) => Json(trace).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => {
            tracing::warn!(trace_id = %trace_id, error = %error, "trace load failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "trace_load_failed",
                "trace load failed",
                None,
            )
        }
    }
}

async fn admin_package_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(package_id): AxumPath<String>,
) -> Response {
    if let Err(response) = admin_auth_and_rate_limit(&state, &headers, "package_audit_read") {
        return *response;
    }
    match load_package_audit(&state.db, &package_id) {
        Ok(Some(package)) => Json(package).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => {
            tracing::warn!(package_id = %package_id, error = %error, "admin package load failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "package_audit_load_failed",
                "package audit load failed",
                None,
            )
        }
    }
}

async fn session_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    if let Err(response) = admin_auth_and_rate_limit(&state, &headers, "session_read") {
        return *response;
    }
    match load_session(&state.db, &session_id) {
        Ok(Some(session)) => Json(session).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => {
            tracing::warn!(session_id = %session_id, error = %error, "session load failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_load_failed",
                "session load failed",
                None,
            )
        }
    }
}

async fn metrics_endpoint(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(response) = admin_auth_and_rate_limit(&state, &headers, "metrics_read") {
        return *response;
    }
    match load_metrics(&state) {
        Ok(metrics) => Json(metrics).into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "metrics load failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "metrics_load_failed",
                "metrics load failed",
                None,
            )
        }
    }
}

async fn memory_collector_run_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<MemoryCollectorRunRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_collector_run") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory collector db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_collector_db_failed",
                "memory collector db failed",
                None,
            );
        }
    };
    let trigger = payload.trigger.as_deref().unwrap_or("admin_manual");
    let llm_probe_validation = if let Some(probe) = payload.llm_extraction_probe.as_ref() {
        match validate_llm_memory_extraction_output(probe) {
            Ok(value) => Some(value),
            Err(error) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "llm_memory_extraction_invalid",
                    safe_error_detail(&error),
                    None,
                );
            }
        }
    } else {
        None
    };
    match run_memory_collector(
        &conn,
        MemoryCollectorRunInput {
            trigger_type: trigger,
            actor: &actor,
            limit: payload.limit.unwrap_or(50),
            dry_run: payload.dry_run.unwrap_or(false),
            trace_id: payload
                .trace_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
        },
    ) {
        Ok(mut report) => {
            if let Some(validation) = llm_probe_validation {
                report["llm_extraction_probe_validation"] = validation;
            }
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_collector_admin_run",
                &actor,
                json!({
                    "run_id": report.get("run_id"),
                    "trigger_type": trigger,
                    "dry_run": payload.dry_run.unwrap_or(false),
                    "processed_count": report.get("processed_count"),
                    "candidate_count": report.get("candidate_count"),
                    "suppressed_count": report.get("suppressed_count"),
                    "denied_count": report.get("denied_count"),
                    "duplicate_count": report.get("duplicate_count"),
                    "secret_values_printed": false,
                }),
            ) {
                tracing::warn!(error = %error, "memory collector admin audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(report).into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "memory collector run failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "memory_collector_run_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn online_evidence_card_worker_run_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<OnlineEvidenceCardWorkerRunRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "online_evidence_card_worker_run")
    {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let requested_actor = payload
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&actor);
    let input = OnlineEvidenceCardWorkerRunInput {
        actor: requested_actor.to_string(),
        limit: payload
            .limit
            .unwrap_or(state.online_evidence_card_worker_batch_size),
        retrieval_limit: payload
            .retrieval_limit
            .unwrap_or(state.online_evidence_card_worker_retrieval_limit),
    };
    match state
        .runtime_store
        .run_online_evidence_card_worker_once(input)
    {
        Ok(report) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "online_evidence_card_worker_admin_run",
                &actor,
                json!({
                    "processed_count": report.processed_count,
                    "raw_candidate_count": report.raw_candidate_count,
                    "staged_count": report.staged_count,
                    "promoted_count": report.promoted_count,
                    "conflicted_count": report.conflicted_count,
                    "failed_count": report.failed_count,
                    "actor_overridden": requested_actor != actor,
                }),
            ) {
                tracing::warn!(error = %error, "online evidence card worker admin audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!(report)).into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "online evidence card worker run failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "online_evidence_card_worker_run_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn memory_candidates_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_candidate_list") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = match memory_candidate_list_input_from_params(&params) {
        Ok(input) => input,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_memory_candidate_filter",
                safe_error_detail(&error),
                None,
            );
        }
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory candidate db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_candidate_db_failed",
                "memory candidate db failed",
                None,
            );
        }
    };
    match list_memory_candidates(&conn, input) {
        Ok(result) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_candidate_admin_list",
                &actor,
                json!({
                    "filter": memory_admin_filter_summary(&params),
                    "result_count": result.get("items").and_then(Value::as_array).map(Vec::len),
                }),
            ) {
                tracing::warn!(error = %error, "memory candidate list audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(result).into_response()
        }
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "memory_candidate_list_failed",
            safe_error_detail(&error),
            None,
        ),
    }
}

async fn memory_candidate_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(candidate_id): AxumPath<String>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_candidate_read") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory candidate db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_candidate_db_failed",
                "memory candidate db failed",
                None,
            );
        }
    };
    match read_memory_candidate(&conn, &candidate_id) {
        Ok(Some(candidate)) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_candidate_admin_read",
                &actor,
                json!({
                    "candidate_id": &candidate_id,
                    "status": candidate.get("status"),
                }),
            ) {
                tracing::warn!(error = %error, "memory candidate read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.memory_candidate_admin_read",
                "candidate": candidate,
                "read_path_enabled": true,
            }))
            .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "memory_candidate_read_failed",
            safe_error_detail(&error),
            None,
        ),
    }
}

async fn memory_candidate_transition_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(candidate_id): AxumPath<String>,
    Json(payload): Json<MemoryCandidateTransitionRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_candidate_transition") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory candidate db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_candidate_db_failed",
                "memory candidate db failed",
                None,
            );
        }
    };
    match transition_memory_candidate(
        &conn,
        MemoryCandidateTransitionInput {
            candidate_id: &candidate_id,
            action: payload.action.trim(),
            actor: &actor,
            reason: payload
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            candidate_type: payload
                .candidate_type
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            sensitivity: payload
                .sensitivity
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            merge_target_candidate_id: payload
                .merge_target_candidate_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            expires_at: payload
                .expires_at
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
        },
    ) {
        Ok(result) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_candidate_admin_transition",
                &actor,
                json!({
                    "candidate_id": &candidate_id,
                    "action": payload.action,
                    "reason_sha256": payload.reason.as_deref().map(hash_text),
                    "status": result.get("candidate").and_then(|v| v.get("status")),
                    "read_path_enabled": true,
                }),
            ) {
                tracing::warn!(error = %error, "memory candidate transition audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(result).into_response()
        }
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "memory_candidate_transition_failed",
            safe_error_detail(&error),
            None,
        ),
    }
}

async fn memory_cards_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_card_list") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = match memory_card_list_input_from_params(&params) {
        Ok(input) => input,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_memory_card_filter",
                safe_error_detail(&error),
                None,
            );
        }
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory card db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_card_db_failed",
                "memory card db failed",
                None,
            );
        }
    };
    match list_memory_cards(&conn, input) {
        Ok(result) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_card_admin_list",
                &actor,
                json!({
                    "filter": memory_admin_filter_summary(&params),
                    "result_count": result.get("items").and_then(Value::as_array).map(Vec::len),
                }),
            ) {
                tracing::warn!(error = %error, "memory card list audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(result).into_response()
        }
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "memory_card_list_failed",
            safe_error_detail(&error),
            None,
        ),
    }
}

async fn memory_card_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(memory_card_id): AxumPath<String>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_card_read") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory card db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_card_db_failed",
                "memory card db failed",
                None,
            );
        }
    };
    match read_memory_card(&conn, &memory_card_id) {
        Ok(Some(memory_card)) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_card_admin_read",
                &actor,
                json!({
                    "memory_card_id": &memory_card_id,
                    "status": memory_card.get("status"),
                    "read_enabled": memory_card.get("read_enabled"),
                }),
            ) {
                tracing::warn!(error = %error, "memory card read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.memory_card_admin_read",
                "memory_card": memory_card,
                "read_path_enabled": true,
            }))
            .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "memory_card_read_failed",
            safe_error_detail(&error),
            None,
        ),
    }
}

async fn memory_card_transition_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(memory_card_id): AxumPath<String>,
    Json(payload): Json<MemoryCardTransitionRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "memory_card_transition") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(error = %error, "memory card db open failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "memory_card_db_failed",
                "memory card db failed",
                None,
            );
        }
    };
    match transition_memory_card(
        &conn,
        MemoryCardTransitionInput {
            memory_card_id: &memory_card_id,
            action: payload.action.trim(),
            actor: &actor,
            reason: payload
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
        },
    ) {
        Ok(result) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "memory_card_admin_transition",
                &actor,
                json!({
                    "memory_card_id": &memory_card_id,
                    "action": payload.action,
                    "reason_sha256": payload.reason.as_deref().map(hash_text),
                    "status": result.get("memory_card").and_then(|v| v.get("status")),
                    "read_enabled": result
                        .get("memory_card")
                        .and_then(|v| v.get("read_enabled")),
                }),
            ) {
                tracing::warn!(error = %error, "memory card transition audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(result).into_response()
        }
        Err(error) => error_response(
            StatusCode::BAD_REQUEST,
            "memory_card_transition_failed",
            safe_error_detail(&error),
            None,
        ),
    }
}

async fn retrieval_failures_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "retrieval_failure_list") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = match retrieval_failure_list_input(&params) {
        Ok(input) => input,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_retrieval_failure_filter",
                safe_error_detail(&error),
                None,
            );
        }
    };
    match state.runtime_store.list_retrieval_failures(input.clone()) {
        Ok(list) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_list",
                &actor,
                json!({
                    "action": "list",
                    "filter_summary": retrieval_failure_filter_summary(&params),
                    "page_size": list.limit,
                    "offset": list.offset,
                    "result_count": list.items.len(),
                    "trace_id": Value::Null,
                    "result": "listed",
                }),
            ) {
                tracing::warn!(error = %error, "retrieval failure admin list audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.retrieval_failure_admin_list",
                "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
                "list": list,
            }))
            .into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "retrieval failure list failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "retrieval_failure_list_failed",
                "retrieval failure list failed",
                None,
            )
        }
    }
}

async fn retrieval_failure_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(failure_id): AxumPath<String>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "retrieval_failure_read") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    match state
        .runtime_store
        .read_retrieval_failure(&failure_id, RetrievalFailureView::AdminDetail)
    {
        Ok(Some(failure)) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_read",
                &actor,
                json!({
                    "action": "read",
                    "failure_id": failure_id,
                    "trace_id": failure.get("trace_id").and_then(Value::as_str),
                    "result_count": 1,
                    "result": "found",
                }),
            ) {
                tracing::warn!(error = %error, "retrieval failure admin read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.retrieval_failure_admin_read",
                "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
                "failure": failure,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_read",
                &actor,
                json!({
                    "action": "read",
                    "failure_id_sha256": hash_text(&failure_id),
                    "trace_id": Value::Null,
                    "result_count": 0,
                    "result": "not_found",
                }),
            ) {
                tracing::warn!(error = %error, "retrieval failure admin read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) => {
            tracing::warn!(failure_id = %failure_id, error = %error, "retrieval failure read failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "retrieval_failure_read_failed",
                "retrieval failure read failed",
                None,
            )
        }
    }
}

async fn update_retrieval_failure_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(failure_id): AxumPath<String>,
    Json(payload): Json<RetrievalFailureUpdateRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "retrieval_failure_update") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let previous_status = state
        .runtime_store
        .read_retrieval_failure(&failure_id, RetrievalFailureView::AdminDetail)
        .ok()
        .flatten()
        .and_then(|failure| {
            failure
                .get("failure")
                .and_then(Value::as_object)
                .and_then(|failure| failure.get("human_review_status"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        });
    match state.runtime_store.update_retrieval_failure_status_checked(
        &failure_id,
        &payload.human_review_status,
        payload.reviewer.as_deref(),
        payload.review_note.as_deref(),
        payload.if_match_updated_at.as_deref(),
    ) {
        Ok(Some(record)) => {
            let failure = match state
                .runtime_store
                .read_retrieval_failure(&record.failure_id, RetrievalFailureView::AdminDetail)
            {
                Ok(Some(failure)) => failure,
                Ok(None) => Value::Null,
                Err(error) => {
                    tracing::warn!(failure_id = %record.failure_id, error = %error, "updated retrieval failure reload failed");
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "retrieval_failure_reload_failed",
                        "retrieval failure reload failed",
                        None,
                    );
                }
            };
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_update",
                &actor,
                json!({
                    "action": "update",
                    "failure_id": &record.failure_id,
                    "trace_id": failure.get("trace_id").and_then(Value::as_str),
                    "previous_status": previous_status,
                    "new_status": &record.human_review_status,
                    "reason_sha256": payload.review_note.as_deref().map(hash_text),
                    "status_history": {
                        "previous_status": previous_status,
                        "new_status": &record.human_review_status,
                        "reason_sha256": payload.review_note.as_deref().map(hash_text),
                        "timestamp": &record.updated_at,
                    },
                    "human_review_status": &record.human_review_status,
                    "if_match_updated_at": payload.if_match_updated_at.as_deref().is_some(),
                    "result_count": 1,
                    "result": "updated",
                }),
            ) {
                tracing::warn!(error = %error, "retrieval failure admin update audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.retrieval_failure_admin_update",
                "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
                "failure": failure,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_update",
                &actor,
                json!({
                    "action": "update",
                    "failure_id_sha256": hash_text(&failure_id),
                    "trace_id": Value::Null,
                    "human_review_status": &payload.human_review_status,
                    "if_match_updated_at": payload.if_match_updated_at.as_deref().is_some(),
                    "result_count": 0,
                    "result": "not_found",
                }),
            ) {
                tracing::warn!(error = %error, "retrieval failure admin update audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) if error.to_string().contains("update conflict") => {
            let trace_id = state
                .runtime_store
                .read_retrieval_failure(&failure_id, RetrievalFailureView::AdminDetail)
                .ok()
                .flatten()
                .and_then(|failure| {
                    failure
                        .get("trace_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            if let Err(audit_error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_update",
                &actor,
                json!({
                    "action": "update",
                    "failure_id": &failure_id,
                    "trace_id": trace_id,
                    "human_review_status": &payload.human_review_status,
                    "if_match_updated_at": payload.if_match_updated_at.as_deref().is_some(),
                    "result_count": 0,
                    "result": "conflict",
                }),
            ) {
                tracing::warn!(error = %audit_error, "retrieval failure admin update audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            error_response(
                StatusCode::CONFLICT,
                "retrieval_failure_update_conflict",
                "retrieval failure update conflict",
                None,
            )
        }
        Err(error) => {
            tracing::warn!(failure_id = %failure_id, error = %error, "retrieval failure update failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "retrieval_failure_update_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn cluster_retrieval_failures_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RetrievalFailureClusterRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "retrieval_failure_cluster") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = RetrievalFailureClusterInput {
        human_review_status: payload
            .human_review_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        failure_type: payload
            .failure_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        min_cluster_size: payload.min_cluster_size.unwrap_or(2),
        limit: payload.limit.unwrap_or(0),
        create_tasks: payload.create_tasks.unwrap_or(true),
    };
    match state.runtime_store.cluster_retrieval_failures(input) {
        Ok(result) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "retrieval_failure_admin_cluster",
                &actor,
                json!({
                    "action": "cluster",
                    "human_review_status": payload.human_review_status.as_deref(),
                    "failure_type": payload.failure_type.as_deref(),
                    "min_cluster_size": payload.min_cluster_size,
                    "limit": payload.limit,
                    "create_tasks": payload.create_tasks.unwrap_or(true),
                    "scanned_failure_count": result.scanned_failure_count,
                    "cluster_count": result.cluster_count,
                    "task_count": result.task_count,
                    "direct_fact_mutation": false,
                    "trace_id": Value::Null,
                    "result": "clustered",
                }),
            ) {
                tracing::warn!(error = %error, "retrieval failure cluster admin audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.retrieval_failure_cluster_admin_result",
                "schema_version": RETRIEVAL_FAILURE_CLUSTER_SCHEMA_VERSION,
                "result": result,
            }))
            .into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "retrieval failure clustering failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "retrieval_failure_cluster_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn governance_tasks_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "governance_task_list") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = match governance_task_list_input(&params) {
        Ok(input) => input,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_governance_task_filter",
                safe_error_detail(&error),
                None,
            );
        }
    };
    match state.runtime_store.list_governance_tasks(input) {
        Ok(list) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_list",
                &actor,
                json!({
                    "action": "list",
                    "filter_summary": governance_task_filter_summary(&params),
                    "page_size": list.limit,
                    "offset": list.offset,
                    "result_count": list.items.len(),
                    "trace_id": Value::Null,
                    "result": "listed",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin list audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.governance_task_admin_list",
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
                "list": list,
            }))
            .into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "governance task list failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "governance_task_list_failed",
                "governance task list failed",
                None,
            )
        }
    }
}

async fn governance_task_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(task_id): AxumPath<String>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "governance_task_read") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    match state.runtime_store.read_governance_task(&task_id) {
        Ok(Some(task)) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_read",
                &actor,
                json!({
                    "action": "read",
                    "task_id": task_id,
                    "trace_id": task.get("trace_id").and_then(Value::as_str),
                    "result_count": 1,
                    "result": "found",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.governance_task_admin_read",
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
                "task": task,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_read",
                &actor,
                json!({
                    "action": "read",
                    "task_id_sha256": hash_text(&task_id),
                    "trace_id": Value::Null,
                    "result_count": 0,
                    "result": "not_found",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) => {
            tracing::warn!(task_id = %task_id, error = %error, "governance task read failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "governance_task_read_failed",
                "governance task read failed",
                None,
            )
        }
    }
}

async fn create_governance_task_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GovernanceTaskManualCreateRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "governance_task_create") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let source_entity_type = payload.source_entity_type.trim().to_string();
    let source_entity_id = payload.source_entity_id.trim().to_string();
    let request_trace_id = payload
        .trace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let request_package_id = payload
        .package_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let task_type = payload
        .task_type
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "expert_review".to_string());
    let create_result = match source_entity_type.as_str() {
        "retrieval_failure" => state
            .runtime_store
            .create_governance_task_from_failure(KnowledgeGovernanceTaskCreateFromFailureInput {
                source_failure_id: source_entity_id.clone(),
                task_type: Some(task_type),
                priority: payload.priority,
                proposed_fix: payload.proposed_fix,
                agent_cluster_key: payload.agent_cluster_key,
            })
            .and_then(|record| record.ok_or_else(|| anyhow!("source retrieval failure not found"))),
        "trace" => match load_trace(&state.db, &source_entity_id) {
            Ok(Some(_)) => {
                state
                    .runtime_store
                    .create_governance_task(KnowledgeGovernanceTaskCreateInput {
                        source_entity_type: "trace".to_string(),
                        source_entity_id: source_entity_id.clone(),
                        trace_id: source_entity_id.clone(),
                        package_id: None,
                        source_failure_id: None,
                        task_type,
                        priority: payload.priority,
                        proposed_fix: payload.proposed_fix,
                        agent_cluster_key: payload.agent_cluster_key,
                    })
            }
            Ok(None) => Err(anyhow!("source trace not found")),
            Err(error) => Err(error),
        },
        "package" => match state.runtime_store.read_package(&source_entity_id) {
            Ok(Some(package)) => {
                state
                    .runtime_store
                    .create_governance_task(KnowledgeGovernanceTaskCreateInput {
                        source_entity_type: "package".to_string(),
                        source_entity_id: source_entity_id.clone(),
                        trace_id: package.trace_id,
                        package_id: Some(package.package_id),
                        source_failure_id: None,
                        task_type,
                        priority: payload.priority,
                        proposed_fix: payload.proposed_fix,
                        agent_cluster_key: payload.agent_cluster_key,
                    })
            }
            Ok(None) => Err(anyhow!("source package not found")),
            Err(error) => Err(error),
        },
        "knowledge_item" => match state.runtime_store.read_knowledge_item(&source_entity_id) {
            Ok(Some(_)) => {
                let trace_id = match request_trace_id {
                    Some(trace_id) => trace_id,
                    None => {
                        return error_response(
                            StatusCode::BAD_REQUEST,
                            "governance_task_trace_required",
                            "trace_id is required for knowledge item review tasks",
                            None,
                        );
                    }
                };
                state
                    .runtime_store
                    .create_governance_task(KnowledgeGovernanceTaskCreateInput {
                        source_entity_type: "knowledge_item".to_string(),
                        source_entity_id: source_entity_id.clone(),
                        trace_id,
                        package_id: request_package_id,
                        source_failure_id: None,
                        task_type,
                        priority: payload.priority,
                        proposed_fix: payload.proposed_fix,
                        agent_cluster_key: payload.agent_cluster_key,
                    })
            }
            Ok(None) => Err(anyhow!("source knowledge item not found")),
            Err(error) => Err(error),
        },
        "eval_miss" | "user_feedback" => {
            let trace_id = match request_trace_id {
                Some(trace_id) => trace_id,
                None => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "governance_task_trace_required",
                        "trace_id is required for this governance task source",
                        None,
                    );
                }
            };
            state
                .runtime_store
                .create_governance_task(KnowledgeGovernanceTaskCreateInput {
                    source_entity_type: source_entity_type.clone(),
                    source_entity_id: source_entity_id.clone(),
                    trace_id,
                    package_id: request_package_id,
                    source_failure_id: None,
                    task_type,
                    priority: payload.priority,
                    proposed_fix: payload.proposed_fix,
                    agent_cluster_key: payload.agent_cluster_key,
                })
        }
        _ => Err(anyhow!("unsupported governance task source entity")),
    };
    match create_result {
        Ok(record) => {
            let task = match state.runtime_store.read_governance_task(&record.task_id) {
                Ok(Some(task)) => task,
                Ok(None) => Value::Null,
                Err(error) => {
                    tracing::warn!(task_id = %record.task_id, error = %error, "created governance task reload failed");
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "governance_task_reload_failed",
                        "governance task reload failed",
                        None,
                    );
                }
            };
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_create",
                &actor,
                json!({
                    "action": "create",
                    "task_id": &record.task_id,
                    "source_failure_id": &record.source_failure_id,
                    "source_entity_type": &record.source_entity_type,
                    "source_entity_id_sha256": hash_text(&record.source_entity_id),
                    "trace_id": &record.trace_id,
                    "task_type": &record.task_type,
                    "priority": &record.priority,
                    "result_count": 1,
                    "result": "created_or_existing",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin create audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.governance_task_admin_create",
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
                "task": task,
            }))
            .into_response()
        }
        Err(error) if error.to_string().contains("not found") => {
            if let Err(audit_error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_create",
                &actor,
                json!({
                    "action": "create",
                    "source_entity_type": source_entity_type,
                    "source_entity_id_sha256": hash_text(&source_entity_id),
                    "trace_id": Value::Null,
                    "result_count": 0,
                    "result": "source_not_found",
                }),
            ) {
                tracing::warn!(error = %audit_error, "governance task admin create audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) => {
            tracing::warn!(source_entity_type = %source_entity_type, error = %error, "governance task create failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "governance_task_create_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn create_knowledge_patch_proposal_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<KnowledgePatchProposalCreateRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "knowledge_patch_proposal_create")
    {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let proposal_type = payload.proposal_type.trim().to_string();
    let source = match resolve_knowledge_patch_proposal_source(
        &state,
        payload.trace_id.as_deref(),
        payload.package_id.as_deref(),
    ) {
        Ok(source) => source,
        Err(error) if error.to_string().contains("not found") => {
            if let Err(audit_error) = append_admin_audit_event(
                &state.db,
                "knowledge_patch_proposal_admin_create",
                &actor,
                json!({
                    "action": "create",
                    "proposal_type": proposal_type,
                    "trace_id_sha256": payload.trace_id.as_deref().map(hash_text),
                    "package_id_sha256": payload.package_id.as_deref().map(hash_text),
                    "result_count": 0,
                    "direct_fact_mutation": false,
                    "result": "source_not_found",
                }),
            ) {
                tracing::warn!(error = %audit_error, "knowledge patch proposal admin audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response();
        }
        Err(error) => {
            tracing::warn!(error = %error, "knowledge patch proposal source resolution failed");
            return error_response(
                StatusCode::BAD_REQUEST,
                "knowledge_patch_proposal_source_failed",
                safe_error_detail(&error),
                None,
            );
        }
    };
    let create_result =
        state
            .runtime_store
            .create_knowledge_patch_proposal(KnowledgePatchProposalCreateInput {
                proposal_type,
                trace_id: source.trace_id.clone(),
                package_id: source.package_id.clone(),
                source_ref: payload
                    .source_ref
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                payload: payload.payload,
                created_by: Some(actor.clone()),
                priority: payload.priority,
            });
    match create_result {
        Ok(result) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "knowledge_patch_proposal_admin_create",
                &actor,
                json!({
                    "action": "create",
                    "proposal_type": result["proposal"]["proposal_type"].as_str(),
                    "proposal_id": result["proposal"]["proposal_id"].as_str(),
                    "payload_sha256": result["proposal"]["payload_sha256"].as_str(),
                    "source_ref_sha256": result["proposal"]["source_ref"].as_str().map(hash_text),
                    "task_id": result["task"]["task_id"].as_str(),
                    "task_type": result["task"]["task_type"].as_str(),
                    "trace_id": result["proposal"]["trace_id"].as_str(),
                    "package_id": result["proposal"]["package_id"].as_str(),
                    "direct_fact_mutation": false,
                    "result_count": 1,
                    "result": "created_or_existing",
                }),
            ) {
                tracing::warn!(error = %error, "knowledge patch proposal admin audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.knowledge_patch_proposal_admin_create",
                "schema_version": KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION,
                "result": result,
            }))
            .into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "knowledge patch proposal create failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "knowledge_patch_proposal_create_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn knowledge_items_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "knowledge_item_list") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = match knowledge_item_list_input(&params) {
        Ok(input) => input,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_knowledge_item_filter",
                safe_error_detail(&error),
                None,
            );
        }
    };
    match state.runtime_store.list_knowledge_items(input) {
        Ok(list) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_list",
                &actor,
                json!({
                    "action": "list",
                    "filter_summary": knowledge_item_filter_summary(&params),
                    "page_size": list.limit,
                    "offset": list.offset,
                    "result_count": list.items.len(),
                    "trace_id": Value::Null,
                    "result": "listed",
                }),
            ) {
                tracing::warn!(error = %error, "knowledge item admin list audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.knowledge_item_admin_list",
                "schema_version": KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION,
                "list": list,
            }))
            .into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "knowledge item list failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "knowledge_item_list_failed",
                "knowledge item list failed",
                None,
            )
        }
    }
}

async fn knowledge_item_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(item_id): AxumPath<String>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "knowledge_item_read") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    match state.runtime_store.read_knowledge_item(&item_id) {
        Ok(Some(item)) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_read",
                &actor,
                json!({
                    "action": "read",
                    "item_id": item_id,
                    "kind": item.kind.as_str(),
                    "state": item.state.as_str(),
                    "result_count": 1,
                    "result": "found",
                }),
            ) {
                tracing::warn!(error = %error, "knowledge item admin read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.knowledge_item_admin_read",
                "schema_version": KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION,
                "item": item,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_read",
                &actor,
                json!({
                    "action": "read",
                    "item_id_sha256": hash_text(&item_id),
                    "result_count": 0,
                    "result": "not_found",
                }),
            ) {
                tracing::warn!(error = %error, "knowledge item admin read audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            error_response(
                StatusCode::NOT_FOUND,
                "knowledge_item_not_found",
                "knowledge item was not found",
                None,
            )
        }
        Err(error) => {
            tracing::warn!(item_id = %item_id, error = %error, "knowledge item read failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "knowledge_item_read_failed",
                "knowledge item read failed",
                None,
            )
        }
    }
}

async fn review_knowledge_item_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(item_id): AxumPath<String>,
    Json(payload): Json<KnowledgeItemHumanReviewRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "knowledge_item_review") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let decision = match KnowledgeItemHumanReviewDecision::parse(&payload.decision) {
        Ok(decision) => decision,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_knowledge_item_review_decision",
                safe_error_detail(&error),
                None,
            );
        }
    };
    let input = KnowledgeItemHumanReviewInput {
        task_id: payload.task_id,
        decision,
        trace_id: payload.trace_id,
        actor: actor.clone(),
        reviewer: payload.reviewer,
        review_note: payload.review_note,
        evidence_ref: payload.evidence_ref,
        expected_state_version: payload.if_match_state_version,
        expected_task_updated_at: payload.if_match_task_updated_at,
    };
    match state
        .runtime_store
        .review_knowledge_item_human(&item_id, input)
    {
        Ok(Some(result)) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_review",
                &actor,
                json!({
                    "action": "review",
                    "item_id": &result.item.item_id,
                    "task_id": &result.task.task_id,
                    "decision": result.decision.as_str(),
                    "state": result.item.state.as_str(),
                    "state_version": result.item.state_version,
                    "reviewer": &result.task.reviewer,
                    "review_note_sha256": result.task.review_note.as_deref().map(hash_text),
                    "evidence_ref_sha256": result.task.evidence_ref.as_deref().map(hash_text),
                    "kb_rebuild_required": result.kb_rebuild_required,
                    "eval_diff_required": result.eval_diff_required,
                    "release_gate_required": result.release_gate_required,
                    "result_count": 1,
                    "result": "reviewed_or_idempotent",
                }),
            ) {
                tracing::warn!(error = %error, "knowledge item admin review audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.knowledge_item_admin_review",
                "schema_version": KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION,
                "result": result,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_review",
                &actor,
                json!({
                    "action": "review",
                    "item_id_sha256": hash_text(&item_id),
                    "result_count": 0,
                    "result": "not_found",
                }),
            ) {
                tracing::warn!(error = %error, "knowledge item admin review audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            error_response(
                StatusCode::NOT_FOUND,
                "knowledge_item_not_found",
                "knowledge item was not found",
                None,
            )
        }
        Err(error) if error.to_string().contains("conflict") => {
            if let Err(audit_error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_review",
                &actor,
                json!({
                    "action": "review",
                    "item_id": &item_id,
                    "result_count": 0,
                    "result": "conflict",
                }),
            ) {
                tracing::warn!(error = %audit_error, "knowledge item admin review audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            error_response(
                StatusCode::CONFLICT,
                "knowledge_item_review_conflict",
                "knowledge item review conflict",
                None,
            )
        }
        Err(error) if error.to_string().contains("not found") => {
            if let Err(audit_error) = append_admin_audit_event(
                &state.db,
                "knowledge_item_admin_review",
                &actor,
                json!({
                    "action": "review",
                    "item_id": &item_id,
                    "result_count": 0,
                    "result": "source_not_found",
                }),
            ) {
                tracing::warn!(error = %audit_error, "knowledge item admin review audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) => {
            tracing::warn!(item_id = %item_id, error = %error, "knowledge item review failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "knowledge_item_review_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn create_governance_task_from_failure_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(failure_id): AxumPath<String>,
    Json(payload): Json<GovernanceTaskCreateRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "governance_task_create") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let input = KnowledgeGovernanceTaskCreateFromFailureInput {
        source_failure_id: failure_id.clone(),
        task_type: payload.task_type,
        priority: payload.priority,
        proposed_fix: payload.proposed_fix,
        agent_cluster_key: payload.agent_cluster_key,
    };
    match state
        .runtime_store
        .create_governance_task_from_failure(input)
    {
        Ok(Some(record)) => {
            let task = match state.runtime_store.read_governance_task(&record.task_id) {
                Ok(Some(task)) => task,
                Ok(None) => Value::Null,
                Err(error) => {
                    tracing::warn!(task_id = %record.task_id, error = %error, "created governance task reload failed");
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "governance_task_reload_failed",
                        "governance task reload failed",
                        None,
                    );
                }
            };
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_create",
                &actor,
                json!({
                    "action": "create_from_failure",
                    "task_id": &record.task_id,
                    "source_failure_id": &record.source_failure_id,
                    "trace_id": &record.trace_id,
                    "task_type": &record.task_type,
                    "priority": &record.priority,
                    "result_count": 1,
                    "result": "created_or_existing",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin create audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.governance_task_admin_create",
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
                "task": task,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_create",
                &actor,
                json!({
                    "action": "create_from_failure",
                    "source_failure_id_sha256": hash_text(&failure_id),
                    "trace_id": Value::Null,
                    "result_count": 0,
                    "result": "source_failure_not_found",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin create audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) => {
            tracing::warn!(failure_id = %failure_id, error = %error, "governance task create failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "governance_task_create_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn update_governance_task_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(task_id): AxumPath<String>,
    Json(payload): Json<GovernanceTaskUpdateRequest>,
) -> Response {
    let actor = match admin_auth_and_rate_limit(&state, &headers, "governance_task_update") {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let previous_status = state
        .runtime_store
        .read_governance_task(&task_id)
        .ok()
        .flatten()
        .and_then(|task| {
            task.get("status")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        });
    let input = KnowledgeGovernanceTaskUpdateInput {
        status: payload.status,
        reviewer: payload.reviewer,
        review_note: payload.review_note,
        evidence_ref: payload.evidence_ref,
        expected_updated_at: payload.if_match_updated_at,
    };
    match state.runtime_store.update_governance_task(&task_id, input) {
        Ok(Some(record)) => {
            let task = match state.runtime_store.read_governance_task(&record.task_id) {
                Ok(Some(task)) => task,
                Ok(None) => Value::Null,
                Err(error) => {
                    tracing::warn!(task_id = %record.task_id, error = %error, "updated governance task reload failed");
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "governance_task_reload_failed",
                        "governance task reload failed",
                        None,
                    );
                }
            };
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_update",
                &actor,
                json!({
                    "action": "update",
                    "task_id": &record.task_id,
                    "source_failure_id": &record.source_failure_id,
                    "trace_id": &record.trace_id,
                    "previous_status": previous_status,
                    "new_status": &record.status,
                    "reason_sha256": record.review_note.as_deref().map(hash_text),
                    "status_history": {
                        "previous_status": previous_status,
                        "new_status": &record.status,
                        "reason_sha256": record.review_note.as_deref().map(hash_text),
                        "timestamp": &record.updated_at,
                    },
                    "status": &record.status,
                    "result_count": 1,
                    "result": "updated",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin update audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            Json(json!({
                "object": "tonglingyu.governance_task_admin_update",
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
                "task": task,
            }))
            .into_response()
        }
        Ok(None) => {
            if let Err(error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_update",
                &actor,
                json!({
                    "action": "update",
                    "task_id_sha256": hash_text(&task_id),
                    "trace_id": Value::Null,
                    "result_count": 0,
                    "result": "not_found",
                }),
            ) {
                tracing::warn!(error = %error, "governance task admin update audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(error) if error.to_string().contains("update conflict") => {
            let trace_id = state
                .runtime_store
                .read_governance_task(&task_id)
                .ok()
                .flatten()
                .and_then(|task| {
                    task.get("trace_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            if let Err(audit_error) = append_admin_audit_event(
                &state.db,
                "governance_task_admin_update",
                &actor,
                json!({
                    "action": "update",
                    "task_id": &task_id,
                    "trace_id": trace_id,
                    "result_count": 0,
                    "result": "conflict",
                }),
            ) {
                tracing::warn!(error = %audit_error, "governance task admin update audit failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "admin_audit_failed",
                    "admin audit failed",
                    None,
                );
            }
            error_response(
                StatusCode::CONFLICT,
                "governance_task_update_conflict",
                "governance task update conflict",
                None,
            )
        }
        Err(error) => {
            tracing::warn!(task_id = %task_id, error = %error, "governance task update failed");
            error_response(
                StatusCode::BAD_REQUEST,
                "governance_task_update_failed",
                safe_error_detail(&error),
                None,
            )
        }
    }
}

async fn prometheus_metrics_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = admin_auth_and_rate_limit(&state, &headers, "prometheus_metrics_read") {
        return *response;
    }
    match load_prometheus_metrics(&state) {
        Ok(metrics) => (
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            metrics,
        )
            .into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "prometheus metrics load failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "metrics_load_failed",
                "metrics load failed",
                None,
            )
        }
    }
}

async fn admin_access_denial_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminAccessDenialRequest>,
) -> Response {
    let subject = match admin_auth_and_rate_limit(&state, &headers, "admin_access_denial_report") {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let denial = payload.denial.trim();
    if !matches!(denial, "role_denied") {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_admin_access_denial",
            "invalid admin access denial",
            None,
        );
    }
    let subject_ref = audit_subject_ref(&subject);
    let admin_trace_id = match append_admin_audit_event(
        &state.db,
        "rqa_admin_access_denied",
        &subject_ref,
        json!({
            "action": bounded_audit_text(payload.action.as_deref(), 64)
                .unwrap_or_else(|| "unknown".to_string()),
            "denial": denial,
            "subject_ref": subject_ref,
            "model": bounded_audit_text(payload.model.as_deref(), 80),
            "reported_by": "openwebui_admin_action",
        }),
    ) {
        Ok(admin_trace_id) => admin_trace_id,
        Err(error) => {
            tracing::warn!(error = %error, "admin access denial audit failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "admin_audit_failed",
                "admin audit failed",
                None,
            );
        }
    };
    Json(json!({
        "object": "tonglingyu.admin_access_denial_audit",
        "schema_version": "tonglingyu-admin-access-denial-v1",
        "admin_trace_id": admin_trace_id,
        "recorded": true,
    }))
    .into_response()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenWebUiMetadataTask {
    Title,
    Tags,
    FollowUps,
}

impl OpenWebUiMetadataTask {
    fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Tags => "tags",
            Self::FollowUps => "follow_ups",
        }
    }
}

fn detect_openwebui_metadata_task(question: &str) -> Option<OpenWebUiMetadataTask> {
    let normalized = question.trim();
    if !normalized.contains("### Task:") || !normalized.contains("### Chat History:") {
        return None;
    }
    if normalized.contains("Generate a concise, 3-5 word title")
        && normalized.contains(r#"JSON format: { "title""#)
    {
        return Some(OpenWebUiMetadataTask::Title);
    }
    if normalized.contains("Generate 1-3 broad tags")
        && normalized.contains(r#"JSON format: { "tags""#)
    {
        return Some(OpenWebUiMetadataTask::Tags);
    }
    if normalized.contains("Suggest 3-5 relevant follow-up questions or prompts")
        && normalized.contains(r#"JSON format: { "follow_ups""#)
    {
        return Some(OpenWebUiMetadataTask::FollowUps);
    }
    None
}

fn openwebui_metadata_completion_content(task: OpenWebUiMetadataTask, question: &str) -> String {
    let use_chinese = contains_cjk(question);
    match task {
        OpenWebUiMetadataTask::Title => {
            let title = if use_chinese {
                "通灵玉证据复核"
            } else {
                "Evidence Review"
            };
            json!({ "title": title }).to_string()
        }
        OpenWebUiMetadataTask::Tags => {
            let tags = if use_chinese {
                vec!["文学", "证据审校", "通灵玉"]
            } else {
                vec!["Arts", "Literature", "Evidence Review"]
            };
            json!({ "tags": tags }).to_string()
        }
        OpenWebUiMetadataTask::FollowUps => {
            let follow_ups = if use_chinese {
                vec![
                    "还需要哪些证据才能确认版本边界？",
                    "当前证据包覆盖了哪些正文位置？",
                    "哪些结论必须等待人工复核？",
                ]
            } else {
                vec![
                    "Which evidence is still needed for edition boundaries?",
                    "Which source passages are covered by the current package?",
                    "Which claims still require human review?",
                ]
            };
            json!({ "follow_ups": follow_ups }).to_string()
        }
    }
}

fn contains_cjk(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let trace_id = new_trace_id();
    let started = Instant::now();
    let auth_subject = match gateway_auth_and_rate_limit(&state, &headers, Some(&trace_id)) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(%trace_id, error = %error, "database unavailable");
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "db_unavailable",
                "database unavailable",
                Some(&trace_id),
            );
        }
    };
    if let Err(error) = tonglingyu_runtime::init_runtime_schema(&conn) {
        tracing::warn!(%trace_id, error = %error, "runtime schema unavailable");
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_schema_unavailable",
            "runtime schema unavailable",
            Some(&trace_id),
        );
    }
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        None,
        None,
        "Received",
        "ok",
        &json!({"payload_hash": hash_value(&payload).unwrap_or_default()}),
    );
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        None,
        None,
        "Authenticated",
        "ok",
        &json!({"auth_subject": &auth_subject}),
    );

    let forbidden = forbidden_control_fields(&payload);
    if !forbidden.is_empty() {
        let forbidden_digest = hash_value(&json!(&forbidden)).unwrap_or_default();
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            None,
            None,
            "Failed with Controlled Response",
            "rejected",
            &json!({"reason": "forbidden_control_fields", "fields": forbidden}),
        );
        let _ = insert_audit_event(
            &conn,
            &trace_id,
            "llm_agent_provider_not_called",
            &json!({
                "reason": "forbidden_control_fields",
                "trigger": "forbidden_control_field_detected",
                "provider_called": false,
                "profiles_not_called": [
                    QUESTION_NORMALIZER_PROFILE_ID,
                    CONVERSATION_STATE_WRITER_PROFILE_ID,
                ],
                "forbidden_field_count": forbidden.len(),
                "forbidden_fields_sha256": forbidden_digest,
                "raw_agent_output_embedded": false,
            }),
        );
        return error_response(
            StatusCode::BAD_REQUEST,
            "forbidden_control_fields",
            "request contains fields reserved for gateway control",
            Some(&trace_id),
        );
    }

    let request = match serde_json::from_value::<ChatCompletionRequest>(payload.clone()) {
        Ok(request) => request,
        Err(error) => {
            let _ = record_workflow_state(
                &conn,
                &trace_id,
                None,
                None,
                "Failed with Controlled Response",
                "invalid_request",
                &json!({"error": error.to_string()}),
            );
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "invalid chat completion request",
                Some(&trace_id),
            );
        }
    };
    let message_count = request.messages.len();
    let history_over_limit = message_count > state.max_messages;
    let requested_model = request.model.as_deref().unwrap_or(&state.model_id);
    if requested_model != state.model_id {
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            None,
            None,
            "Failed with Controlled Response",
            "model_rejected",
            &json!({"requested_model": requested_model}),
        );
        return error_response(
            StatusCode::BAD_REQUEST,
            "model_not_allowed",
            "only the tonglingyu visible model is allowed",
            Some(&trace_id),
        );
    }
    let question = last_user_message(&request.messages);
    let question_chars = question.chars().count();
    if question_chars > state.max_question_chars {
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            None,
            None,
            "Failed with Controlled Response",
            "request_too_large",
            &json!({
                "question_chars": question_chars,
                "max_question_chars": state.max_question_chars,
            }),
        );
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_too_large",
            "request question is too large",
            Some(&trace_id),
        );
    }
    let context = request_context(&headers, &payload, auth_subject);
    let user_session_id = match context_governance::get_or_create_user_session(
        &conn,
        &context.user_ref,
        &context.chat_ref,
        &state.model_id,
    ) {
        Ok(user_session_id) => user_session_id,
        Err(error) => {
            tracing::warn!(%trace_id, error = %error, "session mapping failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_mapping_failed",
                "session mapping failed",
                Some(&trace_id),
            );
        }
    };
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&user_session_id),
        None,
        "Normalized",
        "ok",
        &json!({
            "user_session_id": &user_session_id,
            "user_ref": &context.user_ref,
            "chat_ref": &context.chat_ref,
            "external_message_id": &context.external_message_id,
            "external_message_id_provided": context.external_message_id_provided,
            "message_count": message_count,
            "max_messages": state.max_messages,
            "history_over_limit": history_over_limit,
            "question_chars": question_chars,
        }),
    );
    if history_over_limit {
        let detail = json!({
            "message_count": message_count,
            "max_messages": state.max_messages,
            "behavior": "session_summary_created",
        });
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            Some(&user_session_id),
            None,
            "Message History Truncated",
            "ok",
            &detail,
        );
        let _ = insert_audit_event(&conn, &trace_id, "message_history_truncated", &detail);
    }
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "request_normalized",
        &json!({
            "user_session_id": &user_session_id,
            "user_ref": &context.user_ref,
            "chat_ref": &context.chat_ref,
            "external_message_id": &context.external_message_id,
            "message_count": message_count,
            "history_over_limit": history_over_limit,
            "question_chars": question_chars,
        }),
    );

    if context.external_message_id_provided {
        match load_deduped_final_response(&conn, &user_session_id, &context.external_message_id) {
            Ok(Some(value)) => {
                let _ = record_workflow_state(
                    &conn,
                    &trace_id,
                    Some(&user_session_id),
                    value.get("evidence_package_id").and_then(Value::as_str),
                    "Finalized",
                    "deduped",
                    &json!({"deduped_external_message_id": &context.external_message_id}),
                );
                return if request.stream.unwrap_or(false) {
                    streaming_response_from_cached_completion_value(&value)
                } else {
                    Json(public_completion_value(&value)).into_response()
                };
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%trace_id, error = %error, "dedupe lookup failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "dedupe_lookup_failed",
                    "dedupe lookup failed",
                    Some(&trace_id),
                );
            }
        }
    }

    let context_messages = context_messages_from_chat(&request.messages);
    drop(conn);
    let scoped_context = match create_context_for_request_with_agent_runtime(
        &state.db,
        ContextRequestInput {
            trace_id: &trace_id,
            model_id: &state.model_id,
            external_user_ref: &context.user_ref,
            external_session_id: &context.chat_ref,
            external_message_id: &context.external_message_id,
            question: &question,
            messages: &context_messages,
            history_over_limit,
            max_messages: state.max_messages,
        },
        state.llm_agent_runtime.as_ref(),
    )
    .await
    {
        Ok(scoped_context) => scoped_context,
        Err(error) => {
            tracing::warn!(%trace_id, error = %error, "context governance failed");
            if let Ok(conn) = open_db(&state.db) {
                let _ = insert_audit_event(
                    &conn,
                    &trace_id,
                    "context_governance_failed",
                    &json!({
                        "error": error.to_string(),
                        "local_governance_enforced": true,
                    }),
                );
            }
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "context_governance_failed",
                "context governance failed",
                Some(&trace_id),
            );
        }
    };
    let conn = match open_db(&state.db) {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(%trace_id, error = %error, "database unavailable after context governance");
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "db_unavailable",
                "database unavailable",
                Some(&trace_id),
            );
        }
    };
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        None,
        "Context Pack Created",
        if scoped_context.needs_clarification {
            "needs_clarification"
        } else {
            "ok"
        },
        &json!({
            "context_pack_id": &scoped_context.context_pack_id,
            "interaction_context_id": &scoped_context.interaction_context_id,
            "resolved_question": &scoped_context.resolved_question,
            "session_summary_sha256": hash_text(&scoped_context.session_summary),
            "confidence": scoped_context.confidence,
            "needs_clarification": scoped_context.needs_clarification,
            "used_context_refs": &scoped_context.used_context_refs,
            "context_pack": &scoped_context.context_pack,
        }),
    );
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "context_pack_created",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "interaction_context_id": &scoped_context.interaction_context_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "resolved_question": &scoped_context.resolved_question,
            "confidence": scoped_context.confidence,
            "needs_clarification": scoped_context.needs_clarification,
            "used_context_refs": &scoped_context.used_context_refs,
        }),
    );
    let runtime_context = runtime_context_contract(&scoped_context);
    let projection_audit = runtime_context
        .projections
        .iter()
        .map(|projection| {
            json!({
                "context_projection_ref": &projection.context_projection_ref,
                "context_projection_digest": &projection.context_projection_digest,
                "consumer_type": &projection.consumer_type,
                "consumer_name": &projection.consumer_name,
                "runtime_adapter": &projection.runtime_adapter,
                "tool_policy_digest": &projection.tool_policy_digest,
                "output_contract_digest": &projection.output_contract_digest,
            })
        })
        .collect::<Vec<_>>();
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "context_projections_created",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "interaction_context_id": &scoped_context.interaction_context_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "context_pack_ref": &scoped_context.context_pack_ref,
            "context_pack_digest": &scoped_context.context_pack_digest,
            "context_projections": projection_audit,
        }),
    );

    if question.trim().is_empty() {
        let value = completion_value(
            &state.model_id,
            "请提出一个《红楼梦》相关问题。".to_string(),
            None,
            Some(&scoped_context.user_session_id),
        );
        let _ = append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: &trace_id,
                user_session_id: &scoped_context.user_session_id,
                interaction_context_id: &scoped_context.interaction_context_id,
                context_pack_id: &scoped_context.context_pack_id,
                external_message_id: &context.external_message_id,
                package_id: None,
                response: &value,
            },
        );
        return Json(public_completion_value(&value)).into_response();
    }

    if scoped_context.needs_clarification {
        let content = scoped_context
            .clarification_question
            .clone()
            .unwrap_or_else(|| "请补充明确的指代对象后再继续。".to_string());
        let mut value = completion_value(
            &state.model_id,
            content,
            None,
            Some(&scoped_context.user_session_id),
        );
        value["trace_id"] = json!(&trace_id);
        let _ = append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: &trace_id,
                user_session_id: &scoped_context.user_session_id,
                interaction_context_id: &scoped_context.interaction_context_id,
                context_pack_id: &scoped_context.context_pack_id,
                external_message_id: &context.external_message_id,
                package_id: None,
                response: &value,
            },
        );
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            Some(&scoped_context.user_session_id),
            None,
            "Finalized",
            "clarification_required",
            &json!({
                "context_pack_id": &scoped_context.context_pack_id,
                "unsupported_reason": &scoped_context.unsupported_reason,
                "elapsed_ms": elapsed_ms(started),
            }),
        );
        return if request.stream.unwrap_or(false) {
            streaming_response_from_completion_value(&value)
        } else {
            Json(public_completion_value(&value)).into_response()
        };
    }

    if let Some(metadata_task) = detect_openwebui_metadata_task(&question) {
        let content = openwebui_metadata_completion_content(metadata_task, &question);
        let mut value = completion_value(
            &state.model_id,
            content,
            None,
            Some(&scoped_context.user_session_id),
        );
        value["trace_id"] = json!(&trace_id);
        let _ = append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: &trace_id,
                user_session_id: &scoped_context.user_session_id,
                interaction_context_id: &scoped_context.interaction_context_id,
                context_pack_id: &scoped_context.context_pack_id,
                external_message_id: &context.external_message_id,
                package_id: None,
                response: &value,
            },
        );
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            Some(&scoped_context.user_session_id),
            None,
            "Open WebUI Metadata Request Handled",
            "ok",
            &json!({
                "metadata_task": metadata_task.as_str(),
                "question_sha256": hash_text(&question),
                "evidence_package_created": false,
                "rqa_governance_mutated": false,
            }),
        );
        let _ = insert_audit_event(
            &conn,
            &trace_id,
            "openwebui_metadata_request_handled",
            &json!({
                "user_session_id": &scoped_context.user_session_id,
                "context_pack_id": &scoped_context.context_pack_id,
                "user_ref": &context.user_ref,
                "chat_ref": &context.chat_ref,
                "external_message_id": &context.external_message_id,
                "metadata_task": metadata_task.as_str(),
                "question_sha256": hash_text(&question),
                "evidence_package_created": false,
                "rqa_governance_mutated": false,
            }),
        );
        let _ = record_workflow_state(
            &conn,
            &trace_id,
            Some(&scoped_context.user_session_id),
            None,
            "Finalized",
            "metadata_response",
            &json!({
                "stream": request.stream.unwrap_or(false),
                "elapsed_ms": elapsed_ms(started),
            }),
        );
        return if request.stream.unwrap_or(false) {
            streaming_response_from_completion_value(&value)
        } else {
            Json(public_completion_value(&value)).into_response()
        };
    }

    let mut policy = search_policy(&scoped_context.resolved_question);
    apply_question_frame_required_evidence_types(&mut policy, &scoped_context.context_pack);
    policy.planned_profiles = planned_profiles_for_policy(&state.profiles, &policy);
    let runtime_step_plan = RuntimeStepPlan::from_policy(&state.profiles, &policy);
    let agent_runtime_plan_gate = match execute_agent_runtime_plan_gate(AgentRuntimePlanGateInput {
        trace_id: trace_id.clone(),
        question: scoped_context.resolved_question.clone(),
        required_evidence_types: policy.required_evidence_types.clone(),
        profiles: runtime_workflow_profiles(&state.profiles),
        context: runtime_context.clone(),
    })
    .await
    {
        Ok(report) => report,
        Err(error) => {
            let _ = record_workflow_state(
                &conn,
                &trace_id,
                Some(&scoped_context.user_session_id),
                None,
                "Failed with Controlled Response",
                "agent_runtime_plan_gate_failed",
                &json!({"error": error.to_string()}),
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "agent_runtime_plan_gate_failed",
                "agent runtime plan gate failed",
                Some(&trace_id),
            );
        }
    };
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        None,
        "Planned",
        "ok",
        &json!({
            "policy": &policy,
            "runtime_step_plan": &runtime_step_plan,
            "agent_runtime_plan_gate": &agent_runtime_plan_gate,
        }),
    );
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "retrieval_plan_created",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "resolved_question": &scoped_context.resolved_question,
            "question_type": &policy.question_type,
            "required_evidence_types": &policy.required_evidence_types,
            "planned_profiles": &policy.planned_profiles,
            "blocked_controls": &policy.blocked_controls,
            "runtime_step_plan": &runtime_step_plan,
            "agent_runtime_plan_gate": &agent_runtime_plan_gate,
        }),
    );
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "agent_runtime_plan_gate_completed",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "agent_runtime_client": &agent_runtime_plan_gate.agent_runtime_client,
            "profile_contract_version": &agent_runtime_plan_gate.profile_contract_version,
            "profile_contract_count": agent_runtime_plan_gate.profile_contract_count,
            "runtime_step_count": agent_runtime_plan_gate.runtime_step_count,
            "runtime_step_outputs": &agent_runtime_plan_gate.runtime_step_outputs,
        }),
    );
    let workflow = match state
        .runtime_store
        .execute_workflow_with_agent_runtime_client(
            RuntimeWorkflowInput {
                trace_id: trace_id.clone(),
                question: scoped_context.resolved_question.clone(),
                limit: state.max_evidence,
                required_evidence_types: policy.required_evidence_types.clone(),
                profiles: runtime_workflow_profiles(&state.profiles),
                context: runtime_context.clone(),
            },
            state.agent_runtime_mode,
            state.agent_runtime.clone(),
        )
        .await
    {
        Ok(workflow) => workflow,
        Err(error) => {
            let _ = record_workflow_state(
                &conn,
                &trace_id,
                Some(&scoped_context.user_session_id),
                None,
                "Failed with Controlled Response",
                "runtime_workflow_failed",
                &json!({"error": error.to_string()}),
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "runtime_workflow_failed",
                "runtime workflow failed",
                Some(&trace_id),
            );
        }
    };
    let agent_runtime_summary = workflow.agent_runtime_summary.clone();
    let package = workflow.package;
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Runtime Executed",
        "ok",
        &json!({
            "runtime_step_outputs": &workflow.steps,
            "step_count": workflow.steps.len(),
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Evidence Retrieved",
        "ok",
        &json!({
            "card_count": package.cards.len(),
            "evidence_types": package.cards.iter().map(|card| card.evidence_type.clone()).collect::<BTreeSet<_>>(),
        }),
    );
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "agent_invocation_completed",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "profiles": &policy.planned_profiles,
            "operation": "runtime_profile_workflow",
            "package_id": &package.package_id,
            "card_count": package.cards.len(),
            "evidence_types": package.cards.iter().map(|card| card.evidence_type.clone()).collect::<BTreeSet<_>>(),
            "runtime_step_outputs": &workflow.steps,
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Bundle Created",
        "ok",
        &json!({
            "package_id": &package.package_id,
            "claim_count": package.claims.len(),
            "card_count": package.cards.len(),
        }),
    );

    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Drafted",
        "ok",
        &json!({
            "answer_source": &workflow.answer_source,
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "agent_invocation_completed",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "package_id": &package.package_id,
            "profile": "honglou-main",
            "operation": "draft_answer",
            "answer_source": &workflow.answer_source,
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    let final_answer = workflow.final_answer;
    let revision_status = if package.review.status == "passed" {
        "not_needed"
    } else {
        "applied"
    };
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Reviewed",
        &package.review.status,
        &json!({"review": &package.review}),
    );
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Revised if Needed",
        revision_status,
        &json!({"revision_applied": package.review.status != "passed"}),
    );
    if package.review.status != "passed" {
        let _ = insert_audit_event(
            &conn,
            &trace_id,
            "revision_applied",
            &json!({
                "user_session_id": &scoped_context.user_session_id,
                "context_pack_id": &scoped_context.context_pack_id,
                "package_id": &package.package_id,
                "review_status": &package.review.status,
                "issues": &package.review.issues,
            }),
        );
    }
    let value = completion_value(
        &state.model_id,
        final_answer,
        Some(&package),
        Some(&scoped_context.user_session_id),
    );
    let cached_value = cache_completion_value(&value, &workflow.stream_events);
    let _ = append_runtime_step_journal(
        &conn,
        &trace_id,
        &scoped_context.user_session_id,
        &scoped_context.interaction_context_id,
        &scoped_context.context_pack_id,
        Some(&package.package_id),
        json!({
            "step_count": workflow.steps.len(),
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    let _ = append_review_journal(
        &conn,
        &trace_id,
        &scoped_context.user_session_id,
        &scoped_context.interaction_context_id,
        &scoped_context.context_pack_id,
        Some(&package.package_id),
        json!(&package.review),
    );
    let _ = append_final_response(
        &conn,
        FinalResponseJournalInput {
            trace_id: &trace_id,
            user_session_id: &scoped_context.user_session_id,
            interaction_context_id: &scoped_context.interaction_context_id,
            context_pack_id: &scoped_context.context_pack_id,
            external_message_id: &context.external_message_id,
            package_id: Some(&package.package_id),
            response: &cached_value,
        },
    );
    let _ = record_workflow_state(
        &conn,
        &trace_id,
        Some(&scoped_context.user_session_id),
        Some(&package.package_id),
        "Finalized",
        "ok",
        &json!({
            "stream": request.stream.unwrap_or(false),
            "elapsed_ms": elapsed_ms(started),
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    let _ = insert_audit_event(
        &conn,
        &trace_id,
        "response_finalized",
        &json!({
            "user_session_id": &scoped_context.user_session_id,
            "context_pack_id": &scoped_context.context_pack_id,
            "package_id": &package.package_id,
            "stream": request.stream.unwrap_or(false),
            "elapsed_ms": elapsed_ms(started),
            "agent_runtime_summary": &agent_runtime_summary,
        }),
    );
    if request.stream.unwrap_or(false) {
        streaming_response_from_runtime_events(&state.model_id, &value, &workflow.stream_events)
    } else {
        Json(public_completion_value(&value)).into_response()
    }
}

fn load_package_for_subject(
    db: &Path,
    package_id: &str,
    access: &PackageAccessContext,
) -> Result<Option<Value>> {
    Ok(
        load_evidence_package_for_subject(db, package_id, access)?.map(|package| {
            let mut value = package_json(&package);
            value["access"] = json!({
                "scope": "owner",
                "subject": &access.subject,
                "user_ref": &access.user_ref,
            });
            value
        }),
    )
}

fn load_evidence_package_for_subject(
    db: &Path,
    package_id: &str,
    access: &PackageAccessContext,
) -> Result<Option<EvidencePackage>> {
    let conn = open_db(db)?;
    if !package_owned_by_subject(&conn, package_id, access)? {
        return Ok(None);
    }
    TonglingyuRuntimeStore::new(db.to_path_buf()).read_package(package_id)
}

fn load_package_replay_for_subject(
    db: &Path,
    package_id: &str,
    access: &PackageAccessContext,
) -> Result<Option<Value>> {
    let conn = open_db(db)?;
    if !package_owned_by_subject(&conn, package_id, access)? {
        return Ok(None);
    }
    TonglingyuRuntimeStore::new(db.to_path_buf()).replay_package(package_id)
}

fn package_owned_by_subject(
    conn: &Connection,
    package_id: &str,
    access: &PackageAccessContext,
) -> Result<bool> {
    let owned: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM session_journal AS sj
        JOIN user_sessions AS us ON us.user_session_id = sj.user_session_id
        WHERE sj.package_id = ?1
          AND (us.external_user_ref = ?2 OR us.external_user_ref = ?3)
        "#,
        params![package_id, &access.user_ref, &access.subject],
        |row| row.get(0),
    )?;
    Ok(owned > 0)
}

fn trace_owned_by_subject(
    conn: &Connection,
    trace_id: &str,
    access: &PackageAccessContext,
) -> Result<bool> {
    let owned: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM session_journal AS sj
        JOIN user_sessions AS us ON us.user_session_id = sj.user_session_id
        WHERE sj.trace_id = ?1
          AND (us.external_user_ref = ?2 OR us.external_user_ref = ?3)
        "#,
        params![trace_id, &access.user_ref, &access.subject],
        |row| row.get(0),
    )?;
    Ok(owned > 0)
}

fn resolve_user_feedback_source(
    db: &Path,
    runtime_store: &TonglingyuRuntimeStore,
    access: &PackageAccessContext,
    trace_id: Option<&str>,
    package_id: Option<&str>,
) -> Result<Option<UserFeedbackSource>> {
    let requested_trace_id = trace_id.map(str::trim).filter(|value| !value.is_empty());
    let requested_package_id = package_id.map(str::trim).filter(|value| !value.is_empty());
    if requested_trace_id.is_none() && requested_package_id.is_none() {
        return Err(anyhow!(
            "feedback source trace_id or package_id is required"
        ));
    }
    let conn = open_db(db)?;
    if let Some(package_id) = requested_package_id {
        if !package_owned_by_subject(&conn, package_id, access)? {
            return Ok(None);
        }
        let Some(package) = runtime_store.read_package(package_id)? else {
            return Ok(None);
        };
        if requested_trace_id.is_some_and(|trace_id| trace_id != package.trace_id.as_str()) {
            return Err(anyhow!("feedback source trace/package mismatch"));
        }
        return Ok(Some(UserFeedbackSource {
            trace_id: package.trace_id,
            package_id: Some(package.package_id),
        }));
    }
    let trace_id = requested_trace_id.expect("checked trace_id presence");
    if !trace_owned_by_subject(&conn, trace_id, access)? {
        return Ok(None);
    }
    Ok(Some(UserFeedbackSource {
        trace_id: trace_id.to_string(),
        package_id: None,
    }))
}

fn resolve_knowledge_patch_proposal_source(
    state: &AppState,
    trace_id: Option<&str>,
    package_id: Option<&str>,
) -> Result<KnowledgePatchProposalSource> {
    let requested_trace_id = trace_id.map(str::trim).filter(|value| !value.is_empty());
    let requested_package_id = package_id.map(str::trim).filter(|value| !value.is_empty());
    if requested_trace_id.is_none() && requested_package_id.is_none() {
        return Err(anyhow!(
            "knowledge patch proposal trace_id or package_id is required"
        ));
    }
    if let Some(package_id) = requested_package_id {
        let Some(package) = state.runtime_store.read_package(package_id)? else {
            return Err(anyhow!("knowledge patch proposal source package not found"));
        };
        if requested_trace_id.is_some_and(|trace_id| trace_id != package.trace_id.as_str()) {
            return Err(anyhow!(
                "knowledge patch proposal source trace/package mismatch"
            ));
        }
        return Ok(KnowledgePatchProposalSource {
            trace_id: package.trace_id,
            package_id: Some(package.package_id),
        });
    }
    let trace_id = requested_trace_id.expect("checked trace_id presence");
    if load_trace(&state.db, trace_id)?.is_none() {
        return Err(anyhow!("knowledge patch proposal source trace not found"));
    }
    Ok(KnowledgePatchProposalSource {
        trace_id: trace_id.to_string(),
        package_id: None,
    })
}

fn normalize_user_feedback_type(value: Option<&str>) -> Result<String> {
    let feedback_type = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("other");
    if matches!(
        feedback_type,
        "missing_evidence" | "wrong_evidence" | "wrong_answer" | "source_request" | "other"
    ) {
        Ok(feedback_type.to_string())
    } else {
        Err(anyhow!("unsupported feedback_type {feedback_type}"))
    }
}

fn user_feedback_proposed_fix(feedback_type: &str, feedback_text: &str) -> String {
    let summary = feedback_text
        .chars()
        .take(USER_FEEDBACK_TASK_TEXT_MAX_CHARS)
        .collect::<String>();
    format!("user_feedback_type={feedback_type}; requires_human_review=true; feedback={summary}")
}

fn create_user_feedback_governance_task(
    db: &Path,
    input: UserFeedbackTaskInput<'_>,
) -> Result<KnowledgeGovernanceTaskRecord> {
    let UserFeedbackTaskInput {
        feedback_id,
        trace_id,
        package_id,
        feedback_type,
        feedback_text,
        feedback_char_count,
        access,
        proposed_fix,
    } = input;
    let conn = open_db(db)?;
    tonglingyu_runtime::init_runtime_schema(&conn)?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<KnowledgeGovernanceTaskRecord> {
        let record = tonglingyu_runtime::create_governance_task(
            &conn,
            KnowledgeGovernanceTaskCreateInput {
                source_entity_type: "user_feedback".to_string(),
                source_entity_id: feedback_id.to_string(),
                trace_id: trace_id.to_string(),
                package_id: package_id.map(ToOwned::to_owned),
                source_failure_id: None,
                task_type: "expert_review".to_string(),
                priority: Some("p1".to_string()),
                proposed_fix: Some(proposed_fix),
                agent_cluster_key: Some(format!("user_feedback:{feedback_id}")),
            },
        )?;
        append_runtime_audit_event(
            &conn,
            trace_id,
            "user_feedback_received",
            &json!({
                "feedback_id": feedback_id,
                "feedback_type": feedback_type,
                "feedback_text_sha256": hash_text(feedback_text),
                "feedback_char_count": feedback_char_count,
                "subject_ref": audit_subject_ref(&access.subject),
                "user_ref": audit_subject_ref(&access.user_ref),
                "package_id_sha256": package_id.map(hash_text),
                "task_id": &record.task_id,
                "direct_fact_mutation": false,
            }),
        )?;
        Ok(record)
    })();
    match result {
        Ok(record) => {
            conn.execute_batch("COMMIT")?;
            Ok(record)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn retrieval_failure_list_input(
    params: &BTreeMap<String, String>,
) -> Result<RetrievalFailureListInput> {
    for key in params.keys() {
        if !matches!(
            key.as_str(),
            "human_review_status" | "status" | "failure_type" | "limit" | "offset"
        ) {
            return Err(anyhow!("unsupported retrieval failure filter {key}"));
        }
    }
    let human_review_status = params
        .get("human_review_status")
        .or_else(|| params.get("status"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    validate_optional_filter_value(
        human_review_status.as_deref(),
        "human_review_status",
        &["open", "in_review", "resolved", "wontfix"],
    )?;
    let failure_type = params
        .get("failure_type")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = parse_optional_usize(params.get("limit"), "limit")?.unwrap_or(50);
    let offset = parse_optional_usize(params.get("offset"), "offset")?.unwrap_or(0);
    Ok(RetrievalFailureListInput {
        human_review_status,
        failure_type,
        limit,
        offset,
        view: RetrievalFailureView::AdminDetail,
    })
}

fn parse_optional_usize(value: Option<&String>, name: &str) -> Result<Option<usize>> {
    value
        .map(|raw| {
            raw.parse::<usize>()
                .with_context(|| format!("{name} must be a positive integer"))
        })
        .transpose()
}

fn retrieval_failure_filter_summary(params: &BTreeMap<String, String>) -> Value {
    json!({
        "human_review_status": params.get("human_review_status").or_else(|| params.get("status")),
        "failure_type": params.get("failure_type"),
        "has_limit": params.contains_key("limit"),
        "has_offset": params.contains_key("offset"),
    })
}

fn governance_task_list_input(
    params: &BTreeMap<String, String>,
) -> Result<KnowledgeGovernanceTaskListInput> {
    for key in params.keys() {
        if !matches!(
            key.as_str(),
            "status"
                | "task_type"
                | "priority"
                | "source_failure_id"
                | "source_entity_type"
                | "source_entity_id"
                | "limit"
                | "offset"
        ) {
            return Err(anyhow!("unsupported governance task filter {key}"));
        }
    }
    let status = params
        .get("status")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    validate_optional_filter_value(
        status.as_deref(),
        "status",
        &["open", "in_review", "accepted", "rejected", "closed"],
    )?;
    let task_type = params
        .get("task_type")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    validate_optional_filter_value(
        task_type.as_deref(),
        "task_type",
        &[
            "source_metadata_fix",
            "expected_evidence_fix",
            "retrieval_policy_fix",
            "alias_term_review",
            "commentary_link_review",
            "version_note_review",
            "expert_review",
        ],
    )?;
    let priority = params
        .get("priority")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    validate_optional_filter_value(priority.as_deref(), "priority", &["p0", "p1", "p2"])?;
    let source_failure_id = params
        .get("source_failure_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let source_entity_type = params
        .get("source_entity_type")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    validate_optional_filter_value(
        source_entity_type.as_deref(),
        "source_entity_type",
        &[
            "retrieval_failure",
            "retrieval_failure_cluster",
            "trace",
            "package",
            "knowledge_item",
            "eval_miss",
            "user_feedback",
            "knowledge_patch_proposal",
        ],
    )?;
    let source_entity_id = params
        .get("source_entity_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = parse_optional_usize(params.get("limit"), "limit")?.unwrap_or(50);
    let offset = parse_optional_usize(params.get("offset"), "offset")?.unwrap_or(0);
    Ok(KnowledgeGovernanceTaskListInput {
        status,
        task_type,
        priority,
        source_failure_id,
        source_entity_type,
        source_entity_id,
        limit,
        offset,
    })
}

fn validate_optional_filter_value(
    value: Option<&str>,
    field_name: &str,
    allowed_values: &[&str],
) -> Result<()> {
    if let Some(value) = value
        && !allowed_values.contains(&value)
    {
        return Err(anyhow!("invalid {field_name} filter {value}"));
    }
    Ok(())
}

fn governance_task_filter_summary(params: &BTreeMap<String, String>) -> Value {
    json!({
        "status": params.get("status"),
        "task_type": params.get("task_type"),
        "priority": params.get("priority"),
        "has_source_failure_id": params.contains_key("source_failure_id"),
        "source_entity_type": params.get("source_entity_type"),
        "has_source_entity_id": params.contains_key("source_entity_id"),
        "has_limit": params.contains_key("limit"),
        "has_offset": params.contains_key("offset"),
    })
}

fn knowledge_item_list_input(params: &BTreeMap<String, String>) -> Result<KnowledgeItemListInput> {
    for key in params.keys() {
        if !matches!(key.as_str(), "kind" | "state" | "limit" | "offset") {
            return Err(anyhow!("unsupported knowledge item filter {key}"));
        }
    }
    let kind = params
        .get("kind")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(KnowledgeItemKind::parse)
        .transpose()?;
    let state = params
        .get("state")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(KnowledgeState::parse)
        .transpose()?;
    let limit = parse_optional_usize(params.get("limit"), "limit")?.unwrap_or(50);
    let offset = parse_optional_usize(params.get("offset"), "offset")?.unwrap_or(0);
    Ok(KnowledgeItemListInput {
        kind,
        state,
        limit,
        offset,
    })
}

fn knowledge_item_filter_summary(params: &BTreeMap<String, String>) -> Value {
    json!({
        "kind": params.get("kind"),
        "state": params.get("state"),
        "has_limit": params.contains_key("limit"),
        "has_offset": params.contains_key("offset"),
    })
}

fn memory_candidate_list_input_from_params(
    params: &BTreeMap<String, String>,
) -> Result<MemoryCandidateListInput<'_>> {
    validate_memory_filter_keys(params)?;
    Ok(MemoryCandidateListInput {
        status: params
            .get("status")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        scope_type: params
            .get("scope_type")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        scope_ref: params
            .get("scope_ref")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        limit: parse_optional_usize(params.get("limit"), "limit")?.unwrap_or(50),
        offset: parse_optional_usize(params.get("offset"), "offset")?.unwrap_or(0),
    })
}

fn memory_card_list_input_from_params(
    params: &BTreeMap<String, String>,
) -> Result<MemoryCardListInput<'_>> {
    validate_memory_filter_keys(params)?;
    Ok(MemoryCardListInput {
        status: params
            .get("status")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        scope_type: params
            .get("scope_type")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        scope_ref: params
            .get("scope_ref")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        limit: parse_optional_usize(params.get("limit"), "limit")?.unwrap_or(50),
        offset: parse_optional_usize(params.get("offset"), "offset")?.unwrap_or(0),
    })
}

fn validate_memory_filter_keys(params: &BTreeMap<String, String>) -> Result<()> {
    for key in params.keys() {
        if !matches!(
            key.as_str(),
            "status" | "scope_type" | "scope_ref" | "limit" | "offset"
        ) {
            return Err(anyhow!("unsupported memory filter {key}"));
        }
    }
    Ok(())
}

fn memory_admin_filter_summary(params: &BTreeMap<String, String>) -> Value {
    json!({
        "status": params.get("status"),
        "scope_type": params.get("scope_type"),
        "has_scope_ref": params.contains_key("scope_ref"),
        "has_limit": params.contains_key("limit"),
        "has_offset": params.contains_key("offset"),
    })
}

fn append_admin_audit_event(
    db: &Path,
    event_type: &str,
    actor: &str,
    payload: Value,
) -> Result<String> {
    let admin_trace_id = format!("admin-{}", uuid::Uuid::now_v7().simple());
    let conn = open_db(db)?;
    tonglingyu_runtime::init_runtime_schema(&conn)?;
    append_runtime_audit_event(
        &conn,
        &admin_trace_id,
        event_type,
        &json!({
            "actor": actor,
            "admin_trace_id": &admin_trace_id,
            "payload": payload,
        }),
    )?;
    Ok(admin_trace_id)
}

fn load_trace(db: &Path, trace_id: &str) -> Result<Option<Value>> {
    let conn = open_db(db)?;
    let runtime_store = TonglingyuRuntimeStore::new(db.to_path_buf());
    let package_ids = runtime_store.package_ids_for_trace(trace_id)?;
    let workflow_states = load_rows_json(
        &conn,
        "SELECT state_id, session_id, package_id, state, status, detail_json, created_at FROM workflow_states WHERE trace_id = ?1 ORDER BY created_at, state_id",
        trace_id,
    )?;
    let audit_events = runtime_store.audit_events_for_trace(trace_id)?;
    let agent_runtime_summary = latest_agent_runtime_summary(&audit_events);
    let retrieval_failures = runtime_store.list_retrieval_failures_for_trace(
        trace_id,
        RetrievalFailureView::AdminDetail,
        100,
    )?;
    let governance_tasks = runtime_store.list_governance_tasks_for_trace(trace_id, 100)?;
    let online_evidence_card_update_requests =
        runtime_store.online_evidence_card_update_requests_for_trace(trace_id, 100)?;
    let online_evidence_card_jobs =
        runtime_store.online_evidence_card_jobs_for_trace(trace_id, 100)?;
    let online_evidence_card_raw_candidates =
        runtime_store.online_evidence_card_raw_candidates_for_trace(trace_id, 100)?;
    let online_evidence_card_staged =
        runtime_store.online_evidence_card_staged_for_trace(trace_id, 100)?;
    let online_evidence_card_events =
        runtime_store.online_evidence_card_events_for_trace(trace_id, 200)?;
    let retrieval_quality_summary = retrieval_quality_summary(&retrieval_failures);
    let scoped_context = context_governance::load_trace_context(&conn, trace_id)?;
    let scoped_context_has_rows = scoped_context
        .get("context_packs")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
        || scoped_context
            .get("session_journal")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty());
    if package_ids.is_empty()
        && workflow_states.is_empty()
        && audit_events.is_empty()
        && online_evidence_card_update_requests.is_empty()
        && online_evidence_card_jobs.is_empty()
        && online_evidence_card_raw_candidates.is_empty()
        && online_evidence_card_staged.is_empty()
        && online_evidence_card_events.is_empty()
        && !scoped_context_has_rows
    {
        return Ok(None);
    }
    let mut packages = Vec::new();
    for package_id in package_ids {
        if let Some(package) = runtime_store.read_package(&package_id)? {
            packages.push(package_json(&package));
        }
    }
    let mut trace = json!({
        "object": "tonglingyu.trace",
        "trace_id": trace_id,
        "workflow_states": workflow_states,
        "audit_events": audit_events,
        "agent_runtime_summary": agent_runtime_summary,
        "retrieval_quality_summary": retrieval_quality_summary,
        "retrieval_failure_ids": retrieval_failure_ids(&retrieval_failures),
        "retrieval_failures": retrieval_failures,
        "governance_task_ids": governance_task_ids(&governance_tasks),
        "governance_tasks": governance_tasks,
        "online_evidence_card_ingest": {
            "update_requests": online_evidence_card_update_requests,
            "jobs": online_evidence_card_jobs,
            "raw_candidates": online_evidence_card_raw_candidates,
            "staged_cards": online_evidence_card_staged,
            "events": online_evidence_card_events,
        },
        "scoped_context": scoped_context,
        "packages": packages,
    });
    context_governance::redact_admin_trace_content_fields(&mut trace);
    Ok(Some(trace))
}

fn latest_agent_runtime_summary(audit_events: &[Value]) -> Value {
    audit_events
        .iter()
        .rev()
        .find(|event| {
            event.get("event_type") == Some(&json!("agent_runtime_profile_execution_summarized"))
        })
        .and_then(|event| event.get("payload"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn load_package_audit(db: &Path, package_id: &str) -> Result<Option<Value>> {
    let runtime_store = TonglingyuRuntimeStore::new(db.to_path_buf());
    let Some(package) = runtime_store.read_package(package_id)? else {
        return Ok(None);
    };
    let retrieval_failures = runtime_store.list_retrieval_failures_for_package(
        package_id,
        RetrievalFailureView::AdminDetail,
        100,
    )?;
    let governance_tasks = runtime_store.list_governance_tasks_for_package(package_id, 100)?;
    let retrieval_quality_summary = retrieval_quality_summary(&retrieval_failures);
    let trace = load_trace(db, &package.trace_id)?;
    Ok(Some(json!({
        "object": "tonglingyu.package_audit",
        "package_id": &package.package_id,
        "trace_id": &package.trace_id,
        "package": package_json(&package),
        "retrieval_quality_summary": retrieval_quality_summary,
        "retrieval_failure_ids": retrieval_failure_ids(&retrieval_failures),
        "retrieval_failures": retrieval_failures,
        "governance_task_ids": governance_task_ids(&governance_tasks),
        "governance_tasks": governance_tasks,
        "trace": trace,
    })))
}

fn retrieval_quality_summary(failures: &[Value]) -> Value {
    let mut status_counts = BTreeMap::<String, usize>::new();
    let mut type_counts = BTreeMap::<String, usize>::new();
    let mut quality_issue_count = 0_usize;
    for failure in failures {
        if let Some(status) = failure
            .get("human_review_status")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            *status_counts.entry(status).or_default() += 1;
        }
        if let Some(failure_type) = failure
            .get("failure_type")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            *type_counts.entry(failure_type).or_default() += 1;
        }
        quality_issue_count += failure
            .get("quality_issues")
            .and_then(Value::as_array)
            .map(Vec::len)
            .or_else(|| {
                failure
                    .get("quality_issue_count")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
            })
            .unwrap_or_default();
    }
    let open_failure_count = status_counts.get("open").copied().unwrap_or_default()
        + status_counts.get("in_review").copied().unwrap_or_default();
    json!({
        "object": "tonglingyu.retrieval_quality_admin_summary",
        "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
        "status": if open_failure_count == 0 { "passed" } else { "needs_attention" },
        "failure_count": failures.len(),
        "open_failure_count": open_failure_count,
        "quality_issue_count": quality_issue_count,
        "failure_ids": retrieval_failure_ids(failures),
        "by_status": status_counts,
        "by_type": type_counts,
    })
}

fn retrieval_failure_ids(failures: &[Value]) -> Vec<String> {
    failures
        .iter()
        .filter_map(|failure| failure.get("failure_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn governance_task_ids(tasks: &[Value]) -> Vec<String> {
    tasks
        .iter()
        .filter_map(|task| task.get("task_id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn load_session(db: &Path, session_id: &str) -> Result<Option<Value>> {
    let conn = open_db(db)?;
    context_governance::load_session(&conn, session_id)
}

fn load_metrics(state: &AppState) -> Result<Value> {
    let conn = open_db(&state.db)?;
    let runtime_stats = state.runtime_store.store_stats()?;
    let online_evidence_card_ingest = state.runtime_store.online_evidence_card_ingest_stats()?;
    let scoped_context_counts = context_table_counts(&conn)?;
    let workflow_status_counts = grouped_counts(
        &conn,
        "SELECT status, COUNT(*) FROM workflow_states GROUP BY status",
    )?;
    Ok(json!({
        "object": "tonglingyu.gateway_metrics",
        "generated_at": now_rfc3339(),
        "started_at": &state.started_at,
        "profiles": &state.profiles,
        "dependencies": {
            "sqlite": "ok",
            "upstream": if state.upstream_base_url.is_some() { "configured" } else { "local" },
            "upstream_model": &state.upstream_model,
            "upstream_api_key_configured": state.upstream_api_key.is_some(),
            "upstream_timeout_secs": state.upstream_timeout_secs,
            "agent_runtime": {
                "mode": state.agent_runtime_mode.as_str(),
                "config_source": "TONGLINGYU_AGENT_ROLE_TEXT/PACKAGE/DRAFT/REVIEW_PROVIDER",
                "provider_profiles": &state.workflow_agent_provider_profiles,
            },
            "llm_agent_runtime": {
                "mode": &state.llm_agent_runtime_mode,
                "config_source": "TONGLINGYU_AGENT_ROLE_*_PROVIDER",
                "provider_profiles": &state.llm_agent_provider_profiles,
            },
        },
        "security": {
            "gateway_key_count": state.gateway_api_keys.len(),
            "admin_key_count": state.admin_api_keys.len(),
            "admin_key_isolated": is_admin_key_isolated(state),
            "rate_limit_per_minute": state.rate_limit_per_minute,
            "rate_limit_disabled": state.rate_limit_per_minute == 0,
        },
        "limits": {
            "max_messages": state.max_messages,
            "max_question_chars": state.max_question_chars,
            "max_body_bytes": state.max_body_bytes,
        },
        "retention": {
            "retention_days": state.retention_days,
            "auto_prune_enabled": state.retention_days > 0,
        },
        "online_evidence_card_ingest": {
            "worker_enabled": state.online_evidence_card_worker_enabled,
            "worker_interval_secs": state.online_evidence_card_worker_interval_secs,
            "worker_batch_size": state.online_evidence_card_worker_batch_size,
            "worker_retrieval_limit": state.online_evidence_card_worker_retrieval_limit,
            "stats": online_evidence_card_ingest,
        },
        "counts": {
            "sources": runtime_stats.sources,
            "blocks": runtime_stats.blocks,
            "sessions": scoped_context_counts["user_sessions"].clone(),
            "messages": scoped_context_counts["session_journal"].clone(),
            "user_sessions": scoped_context_counts["user_sessions"].clone(),
            "interaction_contexts": scoped_context_counts["interaction_contexts"].clone(),
            "context_packs": scoped_context_counts["context_packs"].clone(),
            "context_projections": scoped_context_counts["context_projections"].clone(),
            "session_journal": scoped_context_counts["session_journal"].clone(),
            "memory_candidates": scoped_context_counts["memory_candidates"].clone(),
            "memory_cards": scoped_context_counts["memory_cards"].clone(),
            "memory_policy_decisions": scoped_context_counts["memory_policy_decisions"].clone(),
            "memory_transition_audit": scoped_context_counts["memory_transition_audit"].clone(),
            "memory_collector_runs": scoped_context_counts["memory_collector_runs"].clone(),
            "evidence_packages": runtime_stats.evidence_packages,
            "evidence_cards": runtime_stats.evidence_cards,
            "retrieval_failures": runtime_stats.retrieval_failures,
            "governance_tasks": runtime_stats.governance_tasks,
            "knowledge_patch_proposals": runtime_stats.knowledge_patch_proposals,
            "workflow_states": table_count(&conn, "workflow_states")?,
            "audit_events": runtime_stats.audit_events,
        },
        "scoped_context": scoped_context_counts,
        "review_status": runtime_stats.review_status,
        "evidence_types": runtime_stats.evidence_types,
        "rqa": {
            "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
            "retrieval_failures": {
                "total": runtime_stats.retrieval_failures,
                "by_status": runtime_stats.retrieval_failure_status,
                "by_type": runtime_stats.retrieval_failure_type,
            },
            "governance_tasks": {
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
                "total": runtime_stats.governance_tasks,
                "by_status": runtime_stats.governance_task_status,
                "by_type": runtime_stats.governance_task_type,
                "by_priority": runtime_stats.governance_task_priority,
            },
            "knowledge_patch_proposals": {
                "schema_version": KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION,
                "total": runtime_stats.knowledge_patch_proposals,
            },
        },
        "workflow_status": workflow_status_counts,
    }))
}

fn load_prometheus_metrics(state: &AppState) -> Result<String> {
    let conn = open_db(&state.db)?;
    let runtime_stats = state.runtime_store.store_stats()?;
    let scoped_context_counts = context_table_counts(&conn)?;
    let mut lines = Vec::new();
    lines.push("# HELP tonglingyu_gateway_info Gateway static configuration info.".to_string());
    lines.push("# TYPE tonglingyu_gateway_info gauge".to_string());
    lines.push(format!(
        "tonglingyu_gateway_info{{agent_runtime_mode=\"{}\",llm_agent_runtime_mode=\"{}\",rate_limit_per_minute=\"{}\",max_body_bytes=\"{}\"}} 1",
        bounded_metric_enum_label(
            state.agent_runtime_mode.as_str(),
            &["openai-compatible-network"]
        ),
        bounded_metric_enum_label(
            &state.llm_agent_runtime_mode,
            &["provider-profile", "minimal-test"]
        ),
        state.rate_limit_per_minute,
        state.max_body_bytes,
    ));
    for (metric, count) in [
        ("tonglingyu_sources_total", runtime_stats.sources),
        ("tonglingyu_blocks_total", runtime_stats.blocks),
        (
            "tonglingyu_sessions_total",
            metric_i64(&scoped_context_counts, "user_sessions"),
        ),
        (
            "tonglingyu_messages_total",
            metric_i64(&scoped_context_counts, "session_journal"),
        ),
        (
            "tonglingyu_interaction_contexts_total",
            metric_i64(&scoped_context_counts, "interaction_contexts"),
        ),
        (
            "tonglingyu_context_packs_total",
            metric_i64(&scoped_context_counts, "context_packs"),
        ),
        (
            "tonglingyu_context_projections_total",
            metric_i64(&scoped_context_counts, "context_projections"),
        ),
        (
            "tonglingyu_session_journal_entries_total",
            metric_i64(&scoped_context_counts, "session_journal"),
        ),
        (
            "tonglingyu_memory_candidates_total",
            metric_i64(&scoped_context_counts, "memory_candidates"),
        ),
        (
            "tonglingyu_memory_cards_total",
            metric_i64(&scoped_context_counts, "memory_cards"),
        ),
        (
            "tonglingyu_memory_policy_decisions_total",
            metric_i64(&scoped_context_counts, "memory_policy_decisions"),
        ),
        (
            "tonglingyu_memory_transition_audit_total",
            metric_i64(&scoped_context_counts, "memory_transition_audit"),
        ),
        (
            "tonglingyu_memory_collector_runs_total",
            metric_i64(&scoped_context_counts, "memory_collector_runs"),
        ),
        (
            "tonglingyu_evidence_packages_total",
            runtime_stats.evidence_packages,
        ),
        (
            "tonglingyu_retrieval_failures_total",
            runtime_stats.retrieval_failures,
        ),
        (
            "tonglingyu_governance_tasks_total",
            runtime_stats.governance_tasks,
        ),
        (
            "tonglingyu_knowledge_patch_proposals_total",
            runtime_stats.knowledge_patch_proposals,
        ),
        ("tonglingyu_audit_events_total", runtime_stats.audit_events),
    ] {
        lines.push(format!("# TYPE {metric} gauge"));
        lines.push(format!("{metric} {count}"));
    }
    for (status, count) in
        bounded_metric_count_map(runtime_stats.review_status, &["passed", "needs_revision"])
    {
        lines.push(format!(
            "tonglingyu_review_status_total{{status=\"{}\"}} {}",
            status, count
        ));
    }
    for (status, count) in bounded_metric_count_map(
        runtime_stats.retrieval_failure_status,
        &["open", "in_review", "resolved", "wontfix"],
    ) {
        lines.push(format!(
            "tonglingyu_retrieval_failures_by_status_total{{status=\"{}\"}} {}",
            status, count
        ));
    }
    for (failure_type, count) in bounded_metric_count_map(
        runtime_stats.retrieval_failure_type,
        &[
            "no_evidence_selected",
            "expected_evidence_missing",
            "missing_required_evidence_type",
            "exact_term_missing",
            "source_usage_metadata_incomplete",
            "reviewer_evidence_insufficient",
            "restore_drill_canary",
            "quality_report_not_passed",
        ],
    ) {
        lines.push(format!(
            "tonglingyu_retrieval_failures_by_type_total{{failure_type=\"{}\"}} {}",
            failure_type, count
        ));
    }
    for (status, count) in bounded_metric_count_map(
        runtime_stats.governance_task_status,
        &["open", "in_review", "accepted", "rejected", "closed"],
    ) {
        lines.push(format!(
            "tonglingyu_governance_tasks_by_status_total{{status=\"{}\"}} {}",
            status, count
        ));
    }
    for (task_type, count) in bounded_metric_count_map(
        runtime_stats.governance_task_type,
        &[
            "source_metadata_fix",
            "expected_evidence_fix",
            "retrieval_policy_fix",
            "alias_term_review",
            "commentary_link_review",
            "version_note_review",
            "expert_review",
        ],
    ) {
        lines.push(format!(
            "tonglingyu_governance_tasks_by_type_total{{task_type=\"{}\"}} {}",
            task_type, count
        ));
    }
    for (priority, count) in
        bounded_metric_count_map(runtime_stats.governance_task_priority, &["p0", "p1", "p2"])
    {
        lines.push(format!(
            "tonglingyu_governance_tasks_by_priority_total{{priority=\"{}\"}} {}",
            priority, count
        ));
    }
    for (event_type, count) in bounded_metric_count_map(
        runtime_stats.audit_event_types,
        &[
            "agent_runtime_profile_draft_consumed",
            "agent_runtime_profile_step_executed",
            "agent_runtime_profile_execution_summarized",
            "evidence_package_created",
            "evidence_package_replayed",
            "retrieval_failure_recorded",
            "retrieval_failure_status_updated",
            "retrieval_failure_admin_list",
            "retrieval_failure_admin_read",
            "retrieval_failure_admin_update",
            "retrieval_failure_admin_cluster",
            "retrieval_failures_clustered",
            "knowledge_patch_proposal_created",
            "knowledge_patch_proposal_admin_create",
            "rqa_retention_pruned",
            "governance_task_created",
            "governance_task_status_updated",
            "governance_task_admin_list",
            "governance_task_admin_read",
            "governance_task_admin_create",
            "governance_task_admin_update",
            "user_feedback_received",
            "rqa_admin_access_denied",
            "reviewer_completed",
            "runtime_profile_step_completed",
        ],
    ) {
        lines.push(format!(
            "tonglingyu_audit_events_by_type_total{{event_type=\"{}\"}} {}",
            event_type, count
        ));
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn metric_i64(counts: &Value, key: &str) -> i64 {
    counts.get(key).and_then(Value::as_i64).unwrap_or_default()
}

fn escape_metric_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn bounded_metric_count_map(
    values: BTreeMap<String, i64>,
    allowed: &[&str],
) -> BTreeMap<String, i64> {
    let mut bounded = BTreeMap::new();
    for (value, count) in values {
        let label = bounded_metric_enum_label(&value, allowed);
        *bounded.entry(label).or_insert(0) += count;
    }
    bounded
}

fn bounded_metric_enum_label(value: &str, allowed: &[&str]) -> String {
    if allowed.contains(&value) {
        escape_metric_label(value)
    } else {
        "other".to_string()
    }
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(Into::into)
}

fn grouped_counts(conn: &Connection, sql: &str) -> Result<Value> {
    let mut object = serde_json::Map::new();
    for (key, count) in grouped_count_pairs(conn, sql)? {
        object.insert(key, json!(count));
    }
    Ok(Value::Object(object))
}

fn grouped_count_pairs(conn: &Connection, sql: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(Into::into)
}

fn load_rows_json(conn: &Connection, sql: &str, trace_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let column_names = stmt
        .column_names()
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let rows = stmt.query_map(params![trace_id], |row| {
        let mut object = serde_json::Map::new();
        for (index, name) in column_names.iter().enumerate() {
            let value: Option<String> = row.get(index)?;
            if name.ends_with("_json") {
                object.insert(
                    name.trim_end_matches("_json").to_string(),
                    value
                        .as_deref()
                        .and_then(|item| serde_json::from_str::<Value>(item).ok())
                        .unwrap_or(Value::Null),
                );
            } else {
                object.insert(
                    name.clone(),
                    value.map(Value::String).unwrap_or(Value::Null),
                );
            }
        }
        Ok(Value::Object(object))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn last_user_message(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(chat_message_text)
        .unwrap_or_default()
}

fn context_messages_from_chat(messages: &[ChatMessage]) -> Vec<ContextMessage> {
    messages
        .iter()
        .map(|message| ContextMessage {
            role: message.role.clone(),
            content: chat_message_text(message),
        })
        .collect()
}

fn chat_message_text(message: &ChatMessage) -> String {
    match &message.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Parts(parts) => parts
            .iter()
            .filter(|part| part.kind.as_deref().unwrap_or("text") == "text")
            .filter_map(|part| part.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n"),
        MessageContent::Other(value) => value.to_string(),
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn new_trace_id() -> String {
    format!("tly-{}", uuid::Uuid::now_v7().simple())
}

#[cfg(test)]
mod tests;
