use std::{collections::BTreeSet, env, path::Path};

use agent_core::{
    RuntimeClient, RuntimeOutput, RuntimeProfileInput, RuntimeProfileMessage, RuntimeStep, new_id,
};
use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration as TimeDuration, OffsetDateTime};

use crate::{
    context_rules,
    conversation_state::{
        ConversationStateInput, ConversationStateMessage, ConversationStateSummary,
        conversation_state_summary_digest, conversation_state_validation_context,
        project_conversation_state_summary, validate_conversation_state_summary,
        write_conversation_state_summary,
    },
    llm_agent_contracts::{
        AgentContextMessage, CONVERSATION_STATE_WRITER_AGENT_TYPE,
        CONVERSATION_STATE_WRITER_PROFILE_ID, CONVERSATION_STATE_WRITER_TIMEOUT_MS,
        LlmAgentRequestEnvelope, QUESTION_NORMALIZER_AGENT_TYPE, QUESTION_NORMALIZER_PROFILE_ID,
        QUESTION_NORMALIZER_TIMEOUT_MS, QuestionNormalizerAgentInput,
        conversation_state_writer_profile_contract, question_normalizer_profile_contract,
    },
    llm_agent_prompt::build_llm_agent_provider_prompt,
    llm_agent_validator::{
        ConversationStateValidationDecision, QuestionNormalizerValidationDecision,
        conversation_state_runtime_error_decision, error_digest,
        question_normalizer_runtime_error_decision, validate_conversation_state_runtime_output,
        validate_question_normalizer_runtime_output,
    },
    llm_contracts::{CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION, LLM_RESOLVER_ALLOWED_TRIGGERS},
    llm_modes::LlmMode,
};

pub(crate) const CONTEXT_SCHEMA_VERSION: &str = "tonglingyu-scoped-context-v1";
pub(crate) const CONTEXT_PROJECTION_SCHEMA_VERSION: &str = "tonglingyu-context-projection-v1";
pub(crate) const CONTEXT_POLICY_VERSION: &str = "tonglingyu-context-policy-v1";
pub(crate) const JOURNAL_RETENTION_POLICY_VERSION: &str = "tonglingyu-session-journal-retention-v1";
pub(crate) const RESOLVER_SCHEMA_VERSION: &str = "tonglingyu-question-resolver-v1";
pub(crate) const RUNTIME_CONSUMER_TYPE: &str = "runtime_profile";
pub(crate) const RUNTIME_ADAPTER: &str = "tonglingyu-runtime-adapter-v1";
pub(crate) const MEMORY_CANDIDATE_SCHEMA_VERSION: &str = "tonglingyu-memory-candidate-v1";
pub(crate) const MEMORY_CARD_SCHEMA_VERSION: &str = "tonglingyu-memory-card-v1";
pub(crate) const MEMORY_TRANSITION_AUDIT_SCHEMA_VERSION: &str =
    "tonglingyu-memory-transition-audit-v1";
pub(crate) const MEMORY_COLLECTOR_POLICY_VERSION: &str = "tonglingyu-memory-collector-policy-v1";
pub(crate) const MEMORY_POLICY_DECISION_SCHEMA_VERSION: &str =
    "tonglingyu-memory-policy-decision-v1";
pub(crate) const SCOPED_MEMORY_POLICY_VERSION: &str = "scoped-memory-policy-v1";
pub(crate) const SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION: &str = "scoped-memory-llm-filter-v1";

const SESSION_SUMMARY_MAX_CHARS: usize = 600;
const JOURNAL_SUMMARY_MAX_CHARS: usize = 240;
const CONVERSATION_STATE_BOUNDARY_MAX_CHARS: usize = 160;
const MEMORY_SUMMARY_MAX_CHARS: usize = 220;
const MEMORY_RAW_EXCERPT_MAX_CHARS: usize = 420;
const MEMORY_COLLECTOR_LEASE_NAME: &str = "memory-collector";
const MEMORY_COLLECTOR_LEASE_TTL_SECS: i64 = 300;
const MEMORY_POLICY_ACTOR: &str = "memory_policy:auto:scoped-memory-policy-v1";
const MEMORY_POLICY_MODE_ENV: &str = "TONGLINGYU_MEMORY_POLICY_MODE";
const MEMORY_POLICY_MODE_AUTO: &str = "auto_policy";
const MEMORY_POLICY_MODE_MANUAL: &str = "manual_required";
const MEMORY_POLICY_MODE_SHADOW: &str = "shadow_only";
#[cfg(test)]
const CONVERSATION_STATE_SUMMARY_MODE_ENV: &str = "TONGLINGYU_CONVERSATION_STATE_SUMMARY_MODE";
const MEMORY_READ_BUDGET_TOTAL: usize = 8;
const MEMORY_READ_BUDGET_USER_PRIVATE: usize = 4;
const MEMORY_READ_BUDGET_SHARED: usize = 4;
const MEMORY_READ_BUDGET_TOOL_PROFILE: usize = 2;
const PROFILE_COMMON_SCOPE_REF: &str = "profile:honglou-main";
const KNOWLEDGE_SPACE_SCOPE_REF: &str = "knowledge_space:tonglingyu-honglou";
const SOURCE_COLLECTION_SCOPE_REF: &str = "source_collection:wikisource-honglou";

#[derive(Debug, Clone)]
pub(crate) struct ContextMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextRequestInput<'a> {
    pub(crate) trace_id: &'a str,
    pub(crate) model_id: &'a str,
    pub(crate) external_user_ref: &'a str,
    pub(crate) external_session_id: &'a str,
    pub(crate) external_message_id: &'a str,
    pub(crate) question: &'a str,
    pub(crate) messages: &'a [ContextMessage],
    pub(crate) history_over_limit: bool,
    pub(crate) max_messages: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct FinalResponseJournalInput<'a> {
    pub(crate) trace_id: &'a str,
    pub(crate) user_session_id: &'a str,
    pub(crate) interaction_context_id: &'a str,
    pub(crate) context_pack_id: &'a str,
    pub(crate) external_message_id: &'a str,
    pub(crate) package_id: Option<&'a str>,
    pub(crate) response: &'a Value,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextResolution {
    pub(crate) user_session_id: String,
    pub(crate) interaction_context_id: String,
    pub(crate) context_pack_id: String,
    pub(crate) context_pack_ref: String,
    pub(crate) context_pack_digest: String,
    pub(crate) resolved_question: String,
    pub(crate) session_summary: String,
    pub(crate) needs_clarification: bool,
    pub(crate) clarification_question: Option<String>,
    pub(crate) unsupported_reason: Option<String>,
    pub(crate) confidence: f64,
    pub(crate) used_context_refs: Vec<String>,
    pub(crate) context_pack: Value,
    pub(crate) context_projections: Vec<ContextProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextPackProfileView {
    pub(crate) profile_name: String,
    pub(crate) visible_question: String,
    pub(crate) session_summary: Option<String>,
    pub(crate) allowed_tools: Vec<String>,
    pub(crate) forbidden_context: Vec<String>,
    pub(crate) memory_read_refs: Vec<String>,
    pub(crate) memory_summaries: Vec<Value>,
    pub(crate) memory_read_ref_digest: String,
    pub(crate) memory_policy_digest: String,
    pub(crate) memory_usage_summary: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextProjection {
    pub(crate) context_projection_id: String,
    pub(crate) context_projection_ref: String,
    pub(crate) context_pack_id: String,
    pub(crate) context_pack_ref: String,
    pub(crate) trace_id: String,
    pub(crate) interaction_context_id: String,
    pub(crate) consumer_type: String,
    pub(crate) consumer_name: String,
    pub(crate) runtime_adapter: String,
    pub(crate) projection_payload: Value,
    pub(crate) allowed_tools: Vec<String>,
    pub(crate) forbidden_tools: Vec<String>,
    pub(crate) output_contract: Value,
    pub(crate) tool_policy_digest: String,
    pub(crate) output_contract_digest: String,
    pub(crate) schema_version: String,
    pub(crate) digest: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryCollectorRunInput<'a> {
    pub(crate) trigger_type: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) limit: usize,
    pub(crate) dry_run: bool,
    pub(crate) trace_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryCandidateListInput<'a> {
    pub(crate) status: Option<&'a str>,
    pub(crate) scope_type: Option<&'a str>,
    pub(crate) scope_ref: Option<&'a str>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryCardListInput<'a> {
    pub(crate) status: Option<&'a str>,
    pub(crate) scope_type: Option<&'a str>,
    pub(crate) scope_ref: Option<&'a str>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryCandidateTransitionInput<'a> {
    pub(crate) candidate_id: &'a str,
    pub(crate) action: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) reason: Option<&'a str>,
    pub(crate) candidate_type: Option<&'a str>,
    pub(crate) sensitivity: Option<&'a str>,
    pub(crate) merge_target_candidate_id: Option<&'a str>,
    pub(crate) expires_at: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryCardTransitionInput<'a> {
    pub(crate) memory_card_id: &'a str,
    pub(crate) action: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) reason: Option<&'a str>,
}

pub(crate) fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS user_sessions (
            user_session_id TEXT PRIMARY KEY,
            external_user_ref TEXT NOT NULL,
            external_session_id TEXT NOT NULL,
            model_id TEXT NOT NULL,
            lifecycle_status TEXT NOT NULL,
            retention_policy TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(external_user_ref, external_session_id, model_id)
        );

        CREATE TABLE IF NOT EXISTS interaction_contexts (
            interaction_context_id TEXT PRIMARY KEY,
            user_session_id TEXT NOT NULL REFERENCES user_sessions(user_session_id),
            context_status TEXT NOT NULL,
            context_mode TEXT NOT NULL,
            resolution_version TEXT NOT NULL,
            permission_version TEXT NOT NULL,
            memory_policy_version TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(user_session_id, context_mode)
        );

        CREATE TABLE IF NOT EXISTS context_scope_bindings (
            binding_id TEXT PRIMARY KEY,
            interaction_context_id TEXT NOT NULL REFERENCES interaction_contexts(interaction_context_id),
            scope_id TEXT NOT NULL,
            scope_type TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            confidence REAL NOT NULL,
            resolved_by TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS context_packs (
            context_pack_id TEXT PRIMARY KEY,
            context_pack_ref TEXT,
            trace_id TEXT NOT NULL,
            interaction_context_id TEXT NOT NULL REFERENCES interaction_contexts(interaction_context_id),
            profile_name TEXT NOT NULL,
            resolved_question TEXT NOT NULL,
            session_summary TEXT NOT NULL,
            active_scopes_json TEXT NOT NULL,
            candidate_scopes_json TEXT NOT NULL,
            allowed_tools_json TEXT NOT NULL,
            forbidden_tools_json TEXT NOT NULL,
            memory_read_refs_json TEXT NOT NULL,
            forbidden_context_json TEXT NOT NULL,
            output_contract_json TEXT NOT NULL,
            profile_views_json TEXT NOT NULL,
            policy_versions_json TEXT,
            llm_agent_context_path_json TEXT,
            schema_version TEXT NOT NULL,
            digest TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS context_projections (
            context_projection_id TEXT PRIMARY KEY,
            context_projection_ref TEXT NOT NULL UNIQUE,
            context_pack_id TEXT NOT NULL REFERENCES context_packs(context_pack_id),
            context_pack_ref TEXT NOT NULL,
            trace_id TEXT NOT NULL,
            interaction_context_id TEXT NOT NULL REFERENCES interaction_contexts(interaction_context_id),
            consumer_type TEXT NOT NULL,
            consumer_name TEXT NOT NULL,
            runtime_adapter TEXT NOT NULL,
            projection_payload_json TEXT NOT NULL,
            allowed_tools_json TEXT NOT NULL,
            forbidden_tools_json TEXT NOT NULL,
            output_contract_json TEXT NOT NULL,
            tool_policy_digest TEXT NOT NULL,
            output_contract_digest TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            digest TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(context_pack_id, consumer_type, consumer_name, runtime_adapter)
        );

        CREATE TABLE IF NOT EXISTS session_journal (
            journal_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            user_session_id TEXT NOT NULL REFERENCES user_sessions(user_session_id),
            interaction_context_id TEXT NOT NULL REFERENCES interaction_contexts(interaction_context_id),
            context_pack_id TEXT,
            package_id TEXT,
            external_message_id TEXT,
            entry_type TEXT NOT NULL,
            content TEXT,
            summary TEXT NOT NULL,
            content_sha256 TEXT,
            content_ref TEXT,
            retention_policy TEXT NOT NULL,
            sensitivity TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memory_candidates (
            candidate_id TEXT PRIMARY KEY,
            candidate_ref TEXT NOT NULL UNIQUE,
            status TEXT NOT NULL,
            journal_id TEXT NOT NULL REFERENCES session_journal(journal_id),
            trace_id TEXT NOT NULL,
            user_session_id TEXT NOT NULL REFERENCES user_sessions(user_session_id),
            interaction_context_id TEXT NOT NULL REFERENCES interaction_contexts(interaction_context_id),
            context_pack_id TEXT,
            source_entry_type TEXT NOT NULL,
            scope_type TEXT NOT NULL,
            scope_ref TEXT NOT NULL,
            candidate_type TEXT NOT NULL,
            summary TEXT NOT NULL,
            summary_sha256 TEXT NOT NULL,
            raw_excerpt_redacted TEXT NOT NULL,
            raw_excerpt_sha256 TEXT NOT NULL,
            sensitivity TEXT NOT NULL,
            risk_flags_json TEXT NOT NULL,
            llm_extraction_json TEXT NOT NULL,
            confidence REAL NOT NULL,
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            expires_at TEXT,
            merged_into_candidate_id TEXT REFERENCES memory_candidates(candidate_id),
            audit_ref TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            UNIQUE(journal_id, scope_type, scope_ref, candidate_type, summary_sha256)
        );

        CREATE TABLE IF NOT EXISTS memory_cards (
            memory_card_id TEXT PRIMARY KEY,
            memory_card_ref TEXT NOT NULL UNIQUE,
            source_candidate_id TEXT NOT NULL REFERENCES memory_candidates(candidate_id),
            status TEXT NOT NULL,
            scope_type TEXT NOT NULL,
            scope_ref TEXT NOT NULL,
            summary TEXT NOT NULL,
            summary_sha256 TEXT NOT NULL,
            acl_json TEXT NOT NULL,
            sensitivity TEXT NOT NULL,
            promotion_policy_version TEXT NOT NULL,
            promoted_by TEXT NOT NULL,
            promoted_at TEXT NOT NULL,
            revoked_by TEXT,
            revoked_at TEXT,
            expires_at TEXT,
            read_enabled INTEGER NOT NULL DEFAULT 0,
            audit_ref TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            UNIQUE(source_candidate_id)
        );

        CREATE TABLE IF NOT EXISTS memory_transition_audit (
            audit_id TEXT PRIMARY KEY,
            audit_ref TEXT NOT NULL UNIQUE,
            entity_type TEXT NOT NULL,
            entity_id TEXT,
            action TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT,
            actor TEXT NOT NULL,
            reason_sha256 TEXT,
            metadata_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            schema_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memory_policy_decisions (
            policy_decision_id TEXT PRIMARY KEY,
            policy_decision_ref TEXT NOT NULL UNIQUE,
            policy_version TEXT NOT NULL,
            policy_mode TEXT NOT NULL,
            candidate_id TEXT NOT NULL REFERENCES memory_candidates(candidate_id),
            memory_card_id TEXT REFERENCES memory_cards(memory_card_id),
            scope_type TEXT NOT NULL,
            scope_ref TEXT NOT NULL,
            candidate_type TEXT NOT NULL,
            rule_filter_json TEXT NOT NULL,
            llm_filter_json TEXT NOT NULL,
            confidence REAL NOT NULL,
            sensitivity TEXT NOT NULL,
            risk_flags_json TEXT NOT NULL,
            decision TEXT NOT NULL,
            decision_reason TEXT NOT NULL,
            ttl_policy_ref TEXT NOT NULL,
            expires_at TEXT,
            actor TEXT NOT NULL,
            created_at TEXT NOT NULL,
            audit_ref TEXT NOT NULL,
            schema_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memory_collector_runs (
            run_id TEXT PRIMARY KEY,
            trigger_type TEXT NOT NULL,
            actor TEXT NOT NULL,
            status TEXT NOT NULL,
            dry_run INTEGER NOT NULL,
            processed_count INTEGER NOT NULL,
            candidate_count INTEGER NOT NULL,
            suppressed_count INTEGER NOT NULL,
            denied_count INTEGER NOT NULL,
            duplicate_count INTEGER NOT NULL,
            error_count INTEGER NOT NULL,
            lease_owner TEXT NOT NULL,
            watermark_journal_id TEXT,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            audit_ref TEXT NOT NULL,
            error TEXT,
            schema_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memory_collector_journal_status (
            journal_id TEXT PRIMARY KEY REFERENCES session_journal(journal_id),
            run_id TEXT NOT NULL REFERENCES memory_collector_runs(run_id),
            status TEXT NOT NULL,
            reason TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memory_collector_leases (
            lease_name TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
            leased_until TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_sessions_external
            ON user_sessions(external_user_ref, external_session_id, model_id);
        CREATE INDEX IF NOT EXISTS idx_interaction_contexts_user
            ON interaction_contexts(user_session_id, context_status);
        CREATE INDEX IF NOT EXISTS idx_context_packs_trace
            ON context_packs(trace_id);
        CREATE INDEX IF NOT EXISTS idx_context_packs_context
            ON context_packs(interaction_context_id);
        CREATE INDEX IF NOT EXISTS idx_context_projections_trace
            ON context_projections(trace_id);
        CREATE INDEX IF NOT EXISTS idx_context_projections_pack
            ON context_projections(context_pack_id);
        CREATE INDEX IF NOT EXISTS idx_context_projections_consumer
            ON context_projections(consumer_type, consumer_name, runtime_adapter);
        CREATE INDEX IF NOT EXISTS idx_session_journal_trace
            ON session_journal(trace_id);
        CREATE INDEX IF NOT EXISTS idx_session_journal_session
            ON session_journal(user_session_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_session_journal_external_message
            ON session_journal(user_session_id, external_message_id, entry_type);
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_status
            ON memory_candidates(status, created_at);
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_scope
            ON memory_candidates(scope_type, scope_ref, status);
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_trace
            ON memory_candidates(trace_id);
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_journal
            ON memory_candidates(journal_id);
        CREATE INDEX IF NOT EXISTS idx_memory_cards_status
            ON memory_cards(status, promoted_at);
        CREATE INDEX IF NOT EXISTS idx_memory_cards_scope
            ON memory_cards(scope_type, scope_ref, status);
        CREATE INDEX IF NOT EXISTS idx_memory_cards_source_candidate
            ON memory_cards(source_candidate_id);
        CREATE INDEX IF NOT EXISTS idx_memory_transition_audit_entity
            ON memory_transition_audit(entity_type, entity_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_memory_policy_decisions_candidate
            ON memory_policy_decisions(candidate_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_memory_policy_decisions_card
            ON memory_policy_decisions(memory_card_id, decision, created_at);
        CREATE INDEX IF NOT EXISTS idx_memory_policy_decisions_scope
            ON memory_policy_decisions(scope_type, scope_ref, decision);
        CREATE INDEX IF NOT EXISTS idx_memory_collector_runs_started
            ON memory_collector_runs(started_at);
        "#,
    )?;
    ensure_column(conn, "context_packs", "context_pack_ref", "TEXT")?;
    ensure_column(conn, "context_packs", "policy_versions_json", "TEXT")?;
    ensure_column(conn, "context_packs", "llm_agent_context_path_json", "TEXT")?;
    ensure_column(conn, "context_packs", "digest", "TEXT")?;
    ensure_column(conn, "session_journal", "package_id", "TEXT")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_journal_package ON session_journal(package_id)",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![MEMORY_CANDIDATE_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![MEMORY_CARD_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![MEMORY_TRANSITION_AUDIT_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![MEMORY_POLICY_DECISION_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![CONTEXT_SCHEMA_VERSION, now_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn create_context_for_request(
    conn: &Connection,
    input: ContextRequestInput<'_>,
) -> Result<ContextResolution> {
    create_context_for_request_with_mode(conn, input, conversation_state_summary_mode())
}

#[cfg(test)]
fn create_context_for_request_with_mode(
    conn: &Connection,
    input: ContextRequestInput<'_>,
    conversation_state_mode: LlmMode,
) -> Result<ContextResolution> {
    let prepared = prepare_context_request(conn, &input)?;
    let conversation_state_summary = deterministic_conversation_state_summary(
        &input,
        &prepared.session_summary,
        &prepared.last_public_answer,
        conversation_state_mode,
    );
    create_context_pack_from_validated_parts(
        conn,
        &input,
        prepared.user_session_id,
        prepared.interaction_context_id,
        prepared.session_summary,
        prepared.deterministic_resolver,
        prepared.last_public_answer,
        conversation_state_mode,
        conversation_state_summary,
        None,
        "deterministic_builder",
    )
}

pub(crate) async fn create_context_for_request_with_agent_runtime(
    db_path: &Path,
    input: ContextRequestInput<'_>,
    runtime_client: &dyn RuntimeClient,
) -> Result<ContextResolution> {
    create_context_for_request_with_agent_runtime_and_modes(
        db_path,
        input,
        runtime_client,
        question_normalizer_agent_mode(),
        conversation_state_agent_mode(),
    )
    .await
}

async fn create_context_for_request_with_agent_runtime_and_modes(
    db_path: &Path,
    input: ContextRequestInput<'_>,
    runtime_client: &dyn RuntimeClient,
    question_agent_mode: LlmMode,
    conversation_state_mode: LlmMode,
) -> Result<ContextResolution> {
    let conn = Connection::open(db_path).context("open context governance db")?;
    let prepared = prepare_context_request(&conn, &input)?;
    drop(conn);
    let mut resolver = prepared.deterministic_resolver.clone();
    if let Some(trigger) = question_normalizer_trigger(
        input.question,
        &prepared.deterministic_resolver,
        prepared.prior_subject.as_deref(),
    )? && question_agent_mode != LlmMode::Disabled
    {
        let decision = run_question_normalizer_agent(
            runtime_client,
            question_agent_mode,
            &trigger,
            &input,
            &prepared,
        )
        .await;
        resolver = if let Some(sealed) = decision.accepted_resolution() {
            ResolverOutput::from_agent_decision(sealed, Some(decision.audit_json()))
        } else {
            let audit = decision.audit_json();
            if question_agent_mode == LlmMode::Enforced {
                return Err(enforced_llm_agent_rejection(
                    QUESTION_NORMALIZER_PROFILE_ID,
                    &audit,
                    decision.errors(),
                ));
            }
            let mut resolver = resolver;
            resolver.strategy = "deterministic_with_llm_shadow".to_string();
            resolver.agent_audit = Some(audit);
            resolver
        };
    }

    let deterministic_summary = deterministic_conversation_state_summary(
        &input,
        &prepared.session_summary,
        &prepared.last_public_answer,
        conversation_state_mode,
    );
    let mut conversation_state_summary = deterministic_summary;
    let mut conversation_state_source = "deterministic_builder";
    let mut conversation_state_agent_audit = None;
    if conversation_state_mode != LlmMode::Disabled {
        let decision = run_conversation_state_agent(
            runtime_client,
            conversation_state_mode,
            &input,
            &prepared,
            &resolver,
        )
        .await;
        conversation_state_agent_audit = Some(decision.audit_json());
        if let Some(sealed) = decision.accepted_summary() {
            conversation_state_summary = Some(sealed.summary().clone());
            conversation_state_source = "llm_agent_validated";
        } else if conversation_state_mode == LlmMode::Enforced {
            return Err(enforced_llm_agent_rejection(
                CONVERSATION_STATE_WRITER_PROFILE_ID,
                conversation_state_agent_audit
                    .as_ref()
                    .expect("conversation state agent audit was just recorded"),
                decision.errors(),
            ));
        }
    }

    let conn = Connection::open(db_path).context("open context governance db")?;
    create_context_pack_from_validated_parts(
        &conn,
        &input,
        prepared.user_session_id,
        prepared.interaction_context_id,
        prepared.session_summary,
        resolver,
        prepared.last_public_answer,
        conversation_state_mode,
        conversation_state_summary,
        conversation_state_agent_audit,
        conversation_state_source,
    )
}

#[derive(Debug, Clone)]
struct PreparedContext {
    user_session_id: String,
    interaction_context_id: String,
    prior_subject: Option<String>,
    session_summary: String,
    deterministic_resolver: ResolverOutput,
    last_public_answer: PublicAnswerBoundary,
}

fn prepare_context_request(
    conn: &Connection,
    input: &ContextRequestInput<'_>,
) -> Result<PreparedContext> {
    let user_session_id = get_or_create_user_session(
        conn,
        input.external_user_ref,
        input.external_session_id,
        input.model_id,
    )?;
    let interaction_context_id = get_or_create_interaction_context(conn, &user_session_id)?;
    assert_read_enabled_memory_has_policy_decisions(conn)?;
    let prior_subject = latest_subject_from_journal(conn, &user_session_id)?;
    let prior_user_question = latest_prior_user_question_from_journal(conn, &user_session_id)?;
    let session_summary = session_summary(
        input.messages,
        prior_subject.as_deref(),
        prior_user_question.as_deref(),
    )?;
    let deterministic_resolver = resolve_question(
        input.question,
        input.messages,
        prior_subject.as_deref(),
        prior_user_question.as_deref(),
    )?;
    let last_public_answer = latest_public_answer_boundary(conn, &user_session_id)?;
    Ok(PreparedContext {
        user_session_id,
        interaction_context_id,
        prior_subject,
        session_summary,
        deterministic_resolver,
        last_public_answer,
    })
}

fn deterministic_conversation_state_summary(
    input: &ContextRequestInput<'_>,
    session_summary: &str,
    last_public_answer: &PublicAnswerBoundary,
    conversation_state_mode: LlmMode,
) -> Option<ConversationStateSummary> {
    if conversation_state_mode == LlmMode::Disabled {
        return None;
    }
    let recent_state_messages = input
        .messages
        .iter()
        .map(|message| ConversationStateMessage {
            role: message.role.as_str(),
            content: message.content.as_str(),
        })
        .collect::<Vec<_>>();
    let evidence_package_ref_views = last_public_answer
        .evidence_package_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_boundaries = last_public_answer
        .boundary
        .as_deref()
        .into_iter()
        .map(|boundary| bounded_summary(boundary, CONVERSATION_STATE_BOUNDARY_MAX_CHARS))
        .collect::<Vec<_>>();
    let required_boundary_refs = required_boundaries
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let conversation_state_input = ConversationStateInput {
        current_question: input.question,
        recent_messages: &recent_state_messages,
        session_summary,
        last_public_answer_boundary: last_public_answer.boundary.as_deref(),
        evidence_package_refs: &evidence_package_ref_views,
        reviewer_warnings: &[],
    };
    let summary = match write_conversation_state_summary(&conversation_state_input) {
        Ok(summary) => summary,
        Err(_) => return None,
    };
    let validation_context = conversation_state_validation_context(
        &conversation_state_input,
        &[],
        &required_boundary_refs,
    );
    let validation = validate_conversation_state_summary(&summary, &validation_context);
    validation.accepted.then_some(summary)
}

#[allow(clippy::too_many_arguments)]
fn create_context_pack_from_validated_parts(
    conn: &Connection,
    input: &ContextRequestInput<'_>,
    user_session_id: String,
    interaction_context_id: String,
    session_summary: String,
    resolver: ResolverOutput,
    last_public_answer: PublicAnswerBoundary,
    conversation_state_mode: LlmMode,
    conversation_state_summary: Option<ConversationStateSummary>,
    conversation_state_agent_audit: Option<Value>,
    conversation_state_source: &str,
) -> Result<ContextResolution> {
    let evidence_package_ref_views = last_public_answer
        .evidence_package_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let conversation_state_projected =
        conversation_state_mode == LlmMode::Enforced && conversation_state_summary.is_some();
    let conversation_state_digest = conversation_state_summary
        .as_ref()
        .map(conversation_state_summary_digest);
    let context_rule_versions = context_rules::context_rule_versions()?;
    let current_window_compression_policy = context_rules::current_window_compression_policy()?;
    let context_pack_id = format!("context-pack-{}", uuid::Uuid::now_v7().simple());
    let context_pack_ref = context_pack_ref(input.trace_id, &context_pack_id);
    let active_scopes = vec![
        json!({
            "scope_type": "session",
            "scope_id": &input.external_session_id,
            "relation_type": "primary",
        }),
        json!({
            "scope_type": "profile_common",
            "scope_id": PROFILE_COMMON_SCOPE_REF,
            "relation_type": "default_runtime_profile",
        }),
        json!({
            "scope_type": "knowledge_space",
            "scope_id": KNOWLEDGE_SPACE_SCOPE_REF,
            "relation_type": "default_knowledge_space",
        }),
        json!({
            "scope_type": "source_collection",
            "scope_id": SOURCE_COLLECTION_SCOPE_REF,
            "relation_type": "default_source_collection",
        }),
    ];
    let candidate_scopes = resolver
        .referent_bindings
        .iter()
        .map(|binding| {
            json!({
                "scope_type": "research_topic",
                "scope_id": format!("topic:{}", hash_text(binding)),
                "relation_type": "candidate",
                "label": binding,
            })
        })
        .collect::<Vec<_>>();
    let memory_read_set = load_authorized_memory_reads(
        conn,
        input.external_user_ref,
        &active_scopes,
        &candidate_scopes,
    )?;
    let profile_views = profile_views(
        &resolver.resolved_question,
        &session_summary,
        &memory_read_set.reads,
    );
    let memory_read_refs = memory_read_set
        .reads
        .iter()
        .map(|read| read.memory_read_ref.clone())
        .collect::<Vec<_>>();
    let memory_read_ref_digest = memory_read_ref_digest(&memory_read_refs);
    let memory_read_policy_digest = memory_read_policy_digest(&memory_read_set.reads);
    let memory_usage_summary =
        memory_usage_summary(&memory_read_set.reads, memory_read_set.truncated_count);
    let llm_agent_context_path = json!({
        "question_normalizer_mode": question_normalizer_agent_mode().as_str(),
        "conversation_state_mode": conversation_state_mode.as_str(),
        "question_normalizer_agent": resolver.agent_audit.clone(),
        "conversation_state_agent": &conversation_state_agent_audit,
        "conversation_state_summary_source": conversation_state_source,
        "raw_agent_output_embedded": false,
        "context_rule_versions": &context_rule_versions,
        "current_window_compression_policy": &current_window_compression_policy,
    });
    let mut context_pack = json!({
        "context_pack_id": &context_pack_id,
        "context_pack_ref": &context_pack_ref,
        "trace_id": input.trace_id,
        "interaction_context_id": &interaction_context_id,
        "profile_name": "all",
        "resolved_question": &resolver.resolved_question,
        "session_summary": &session_summary,
        "active_scopes": &active_scopes,
        "candidate_scopes": &candidate_scopes,
        "allowed_tools": ["tonglingyu.text.search", "tonglingyu.commentary.search"],
        "forbidden_tools": [],
        "memory_read_refs": &memory_read_refs,
        "memory_read_ref_digest": &memory_read_ref_digest,
        "memory_read_policy_digest": &memory_read_policy_digest,
        "memory_usage_summary": &memory_usage_summary,
        "conversation_state_summary_digest": &conversation_state_digest,
        "conversation_state_summary_projection_visible": conversation_state_projected,
        "forbidden_context": [
            "complete_user_history",
            "unauthorized_memory",
            "system_prompt",
            "unreviewed_memory_candidate",
            "conversation_state_summary_as_evidence"
        ],
        "output_contract": {
            "public_response_exposes_context_ids": false,
            "evidence_package_allows_memory": false,
            "conversation_state_summary_allows_evidence": false,
            "schema_version": CONTEXT_SCHEMA_VERSION,
        },
        "profile_views": &profile_views,
        "schema_version": CONTEXT_SCHEMA_VERSION,
        "policy_version": CONTEXT_POLICY_VERSION,
        "policy_versions": {
            "context_policy": CONTEXT_POLICY_VERSION,
            "resolver": RESOLVER_SCHEMA_VERSION,
            "journal_retention": JOURNAL_RETENTION_POLICY_VERSION,
            "scoped_memory_policy": SCOPED_MEMORY_POLICY_VERSION,
            "scoped_memory_llm_filter": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
            "conversation_state_summary": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
            "context_rules": &context_rule_versions,
            "llm_agent_validator": crate::llm_agent_contracts::LLM_AGENT_VALIDATOR_SCHEMA_VERSION,
        },
        "resolver": resolver.audit_json(),
        "llm_agent_context_path": &llm_agent_context_path,
    });
    let context_pack_digest = digest_json(&context_pack);
    context_pack["digest"] = json!(&context_pack_digest);
    let context_projections = build_context_projections(
        input.trace_id,
        &interaction_context_id,
        &context_pack_id,
        &context_pack_ref,
        &profile_views,
        if conversation_state_projected {
            conversation_state_summary.as_ref()
        } else {
            None
        },
    );
    conn.execute(
        "INSERT INTO context_packs (
            context_pack_id, context_pack_ref, trace_id, interaction_context_id, profile_name, resolved_question,
            session_summary, active_scopes_json, candidate_scopes_json, allowed_tools_json,
            forbidden_tools_json, memory_read_refs_json, forbidden_context_json,
            output_contract_json, profile_views_json, policy_versions_json, llm_agent_context_path_json,
            schema_version, digest, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            &context_pack_id,
            &context_pack_ref,
            input.trace_id,
            &interaction_context_id,
            "all",
            &resolver.resolved_question,
            &session_summary,
            serde_json::to_string(&active_scopes)?,
            serde_json::to_string(&candidate_scopes)?,
            serde_json::to_string(&json!([
                "tonglingyu.text.search",
                "tonglingyu.commentary.search"
            ]))?,
            serde_json::to_string(&json!([]))?,
            serde_json::to_string(&memory_read_refs)?,
            serde_json::to_string(&json!([
                "complete_user_history",
                "unauthorized_memory",
                "system_prompt",
                "unreviewed_memory_candidate",
                "conversation_state_summary_as_evidence"
            ]))?,
            serde_json::to_string(&context_pack["output_contract"])?,
            serde_json::to_string(&profile_views)?,
            serde_json::to_string(&context_pack["policy_versions"])?,
            serde_json::to_string(&llm_agent_context_path)?,
            CONTEXT_SCHEMA_VERSION,
            &context_pack_digest,
            now_rfc3339(),
        ],
    )?;
    for projection in &context_projections {
        insert_context_projection(conn, projection)?;
    }
    append_journal_entry(
        conn,
        JournalEntryInput {
            trace_id: input.trace_id,
            user_session_id: &user_session_id,
            interaction_context_id: &interaction_context_id,
            context_pack_id: Some(&context_pack_id),
            package_id: None,
            external_message_id: Some(input.external_message_id),
            entry_type: if is_openwebui_metadata_prompt(input.question) {
                "metadata_prompt"
            } else {
                "user_message"
            },
            content: Some(input.question),
            summary: &bounded_summary(input.question, JOURNAL_SUMMARY_MAX_CHARS),
            retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
            sensitivity: "user_raw_text_high",
            metadata: json!({
                "message_count": input.messages.len(),
                "history_over_limit": input.history_over_limit,
                "max_messages": input.max_messages,
                "content_char_count": input.question.chars().count(),
                "raw_content_in_default_admin_trace": false,
            }),
        },
    )?;
    append_journal_entry(
        conn,
        JournalEntryInput {
            trace_id: input.trace_id,
            user_session_id: &user_session_id,
            interaction_context_id: &interaction_context_id,
            context_pack_id: Some(&context_pack_id),
            package_id: None,
            external_message_id: Some(input.external_message_id),
            entry_type: "context_pack",
            content: None,
            summary: &format!("context pack created for {}", resolver.resolved_question),
            retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
            sensitivity: "internal_context",
            metadata: json!({
                "context_pack_id": &context_pack_id,
                "context_pack_ref": &context_pack_ref,
                "context_pack_digest": &context_pack_digest,
                "resolved_question": &resolver.resolved_question,
                "resolver": resolver.audit_json(),
                "session_summary_sha256": hash_text(&session_summary),
                "context_projection_count": context_projections.len(),
                "memory_read_ref_count": memory_read_refs.len(),
                "memory_read_ref_digest": &memory_read_ref_digest,
                "memory_read_policy_digest": &memory_read_policy_digest,
                "conversation_state_summary_digest": &conversation_state_digest,
                "conversation_state_summary_projection_visible": conversation_state_projected,
                "llm_agent_context_path": &llm_agent_context_path,
            }),
        },
    )?;
    if conversation_state_mode != LlmMode::Disabled {
        append_journal_entry(
            conn,
            JournalEntryInput {
                trace_id: input.trace_id,
                user_session_id: &user_session_id,
                interaction_context_id: &interaction_context_id,
                context_pack_id: Some(&context_pack_id),
                package_id: None,
                external_message_id: Some(input.external_message_id),
                entry_type: "conversation_state_summary_written",
                content: None,
                summary: if conversation_state_summary.is_some() {
                    "conversation state summary written"
                } else {
                    "conversation state summary rejected"
                },
                retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
                sensitivity: "internal_conversation_state",
                metadata: json!({
                    "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
                    "mode": conversation_state_mode.as_str(),
                    "status": if conversation_state_summary.is_some() { "accepted" } else { "rejected" },
                    "conversation_state_summary_digest": &conversation_state_digest,
                    "summary_confidence": conversation_state_summary
                        .as_ref()
                        .map(|summary| summary.summary_confidence),
                    "projection_visible": conversation_state_projected,
                    "summary_source": conversation_state_source,
                    "agent_decision": &conversation_state_agent_audit,
                    "memory_allowed_as_evidence": false,
                    "raw_history_included": false,
                    "evidence_package_refs_count": evidence_package_ref_views.len(),
                }),
            },
        )?;
    }
    append_journal_entry(
        conn,
        JournalEntryInput {
            trace_id: input.trace_id,
            user_session_id: &user_session_id,
            interaction_context_id: &interaction_context_id,
            context_pack_id: Some(&context_pack_id),
            package_id: None,
            external_message_id: Some(input.external_message_id),
            entry_type: "memory_read_decision",
            content: None,
            summary: "scoped memory read decision recorded",
            retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
            sensitivity: "internal_memory_read_policy",
            metadata: json!({
                "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                "policy_mode": memory_policy_mode(),
                "read_budget": memory_read_budget_json(),
                "memory_read_refs": &memory_read_refs,
                "memory_read_ref_digest": &memory_read_ref_digest,
                "memory_read_policy_digest": &memory_read_policy_digest,
                "memory_usage_summary": &memory_usage_summary,
                "truncated_count": memory_read_set.truncated_count,
                "candidate_count_before_budget": memory_read_set.candidate_count_before_budget,
                "raw_memory_content_included": false,
            }),
        },
    )?;
    for projection in &context_projections {
        append_journal_entry(
            conn,
            JournalEntryInput {
                trace_id: input.trace_id,
                user_session_id: &user_session_id,
                interaction_context_id: &interaction_context_id,
                context_pack_id: Some(&context_pack_id),
                package_id: None,
                external_message_id: Some(input.external_message_id),
                entry_type: "context_projection",
                content: None,
                summary: &format!(
                    "context projection created for {}",
                    projection.consumer_name
                ),
                retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
                sensitivity: "internal_context_projection",
                metadata: json!({
                    "context_pack_id": &context_pack_id,
                    "context_pack_ref": &context_pack_ref,
                    "context_projection_id": &projection.context_projection_id,
                    "context_projection_ref": &projection.context_projection_ref,
                    "context_projection_digest": &projection.digest,
                    "consumer_type": &projection.consumer_type,
                    "consumer_name": &projection.consumer_name,
                    "runtime_adapter": &projection.runtime_adapter,
                    "tool_policy_digest": &projection.tool_policy_digest,
                    "output_contract_digest": &projection.output_contract_digest,
                    "projection_payload_sha256": digest_json(&projection.projection_payload),
                }),
            },
        )?;
    }
    Ok(ContextResolution {
        user_session_id,
        interaction_context_id,
        context_pack_id,
        context_pack_ref,
        context_pack_digest,
        resolved_question: resolver.resolved_question,
        session_summary,
        needs_clarification: resolver.needs_clarification,
        clarification_question: resolver.clarification_question,
        unsupported_reason: resolver.unsupported_reason,
        confidence: resolver.confidence,
        used_context_refs: resolver.used_context_refs,
        context_pack,
        context_projections,
    })
}

fn question_normalizer_trigger(
    question: &str,
    deterministic: &ResolverOutput,
    prior_subject: Option<&str>,
) -> Result<Option<String>> {
    let trigger = if context_rules::is_elliptical_followup_question(question)?
        && (deterministic.needs_clarification || deterministic.confidence < 0.75)
    {
        context_rules::ellipsis_trigger()?
    } else if deterministic.needs_clarification
        && deterministic.unsupported_reason.as_deref() == Some("unresolved_referent")
    {
        "unresolved_referent".to_string()
    } else if context_rules::contains_referential_pronoun(question)?
        && (prior_subject.is_some() || !deterministic.referent_bindings.is_empty())
    {
        "prior_subject_needed".to_string()
    } else if deterministic.confidence < 0.75 {
        "low_confidence_binding".to_string()
    } else {
        return Ok(None);
    };
    Ok(LLM_RESOLVER_ALLOWED_TRIGGERS
        .contains(&trigger.as_str())
        .then_some(trigger))
}

async fn run_question_normalizer_agent(
    runtime_client: &dyn RuntimeClient,
    mode: LlmMode,
    trigger: &str,
    input: &ContextRequestInput<'_>,
    prepared: &PreparedContext,
) -> QuestionNormalizerValidationDecision {
    let allowed_referents = match allowed_referents_for_agent(
        input.question,
        input.messages,
        prepared.prior_subject.as_deref(),
        &prepared.deterministic_resolver,
    ) {
        Ok(referents) => referents,
        Err(error) => {
            return question_normalizer_runtime_error_decision(
                mode,
                trigger,
                &LlmAgentRequestEnvelope::new(
                    new_id("req"),
                    QUESTION_NORMALIZER_AGENT_TYPE,
                    QUESTION_NORMALIZER_PROFILE_ID,
                    mode.as_str(),
                    input.trace_id,
                    prepared.user_session_id.clone(),
                    prepared.interaction_context_id.clone(),
                    "llm-agent-input://tonglingyu/context-rules-error".to_string(),
                    "sha256:context-rules-error".to_string(),
                    QUESTION_NORMALIZER_TIMEOUT_MS,
                    json!({}),
                ),
                error.to_string(),
            );
        }
    };
    let agent_input = QuestionNormalizerAgentInput::new(
        trigger,
        input.question,
        recent_messages_for_agent(input.messages, "user"),
        recent_messages_for_agent(input.messages, "assistant"),
        prepared.prior_subject.clone(),
        prepared.session_summary.clone(),
        allowed_referents.clone(),
    );
    let structured_payload = json!(agent_input);
    let input_digest = format!("sha256:{}", digest_json(&structured_payload));
    let envelope = LlmAgentRequestEnvelope::new(
        new_id("req"),
        QUESTION_NORMALIZER_AGENT_TYPE,
        QUESTION_NORMALIZER_PROFILE_ID,
        mode.as_str(),
        input.trace_id,
        prepared.user_session_id.clone(),
        prepared.interaction_context_id.clone(),
        llm_agent_projection_ref(
            input.trace_id,
            QUESTION_NORMALIZER_PROFILE_ID,
            &input_digest,
        ),
        input_digest,
        QUESTION_NORMALIZER_TIMEOUT_MS,
        structured_payload,
    );
    let output = execute_llm_agent_profile(
        runtime_client,
        QUESTION_NORMALIZER_PROFILE_ID,
        &envelope,
        None,
    )
    .await;
    let decision = match output {
        Ok(output) => validate_question_normalizer_runtime_output(
            mode,
            trigger,
            &envelope,
            &output.result_summary,
            output.result_ref.as_deref(),
            Some(&output.metadata),
            &allowed_referents,
        ),
        Err(error) => {
            question_normalizer_runtime_error_decision(mode, trigger, &envelope, error.to_string())
        }
    };
    if decision.contract_accepted() {
        return decision;
    }
    let first_error_digest = error_digest(decision.errors());
    let repaired = execute_llm_agent_profile(
        runtime_client,
        QUESTION_NORMALIZER_PROFILE_ID,
        &envelope,
        Some(decision.errors()),
    )
    .await;
    match repaired {
        Ok(output) => validate_question_normalizer_runtime_output(
            mode,
            trigger,
            &envelope,
            &output.result_summary,
            output.result_ref.as_deref(),
            Some(&output.metadata),
            &allowed_referents,
        )
        .with_repair_metadata(true, Some(first_error_digest)),
        Err(_) => decision.with_repair_metadata(true, Some(first_error_digest)),
    }
}

async fn run_conversation_state_agent(
    runtime_client: &dyn RuntimeClient,
    mode: LlmMode,
    input: &ContextRequestInput<'_>,
    prepared: &PreparedContext,
    resolver: &ResolverOutput,
) -> ConversationStateValidationDecision {
    let recent_state_messages = input
        .messages
        .iter()
        .map(|message| ConversationStateMessage {
            role: message.role.as_str(),
            content: message.content.as_str(),
        })
        .collect::<Vec<_>>();
    let evidence_package_ref_views = prepared
        .last_public_answer
        .evidence_package_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_boundaries = prepared
        .last_public_answer
        .boundary
        .as_deref()
        .into_iter()
        .map(|boundary| bounded_summary(boundary, CONVERSATION_STATE_BOUNDARY_MAX_CHARS))
        .collect::<Vec<_>>();
    let required_boundary_refs = required_boundaries
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_entities = resolver
        .referent_bindings
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let conversation_state_input = ConversationStateInput {
        current_question: input.question,
        recent_messages: &recent_state_messages,
        session_summary: &prepared.session_summary,
        last_public_answer_boundary: prepared.last_public_answer.boundary.as_deref(),
        evidence_package_refs: &evidence_package_ref_views,
        reviewer_warnings: &[],
    };
    let validation_context = conversation_state_validation_context(
        &conversation_state_input,
        &required_entities,
        &required_boundary_refs,
    );
    let agent_messages = input
        .messages
        .iter()
        .rev()
        .take(8)
        .map(|message| AgentContextMessage {
            role: message.role.clone(),
            content: bounded_summary(&message.content, 360),
        })
        .collect::<Vec<_>>();
    let agent_input = crate::llm_agent_contracts::ConversationStateWriterAgentInput::new(
        input.question,
        agent_messages,
        prepared.session_summary.clone(),
        prepared.last_public_answer.boundary.clone(),
        prepared.last_public_answer.evidence_package_refs.clone(),
        Vec::new(),
        resolver.referent_bindings.clone(),
        required_boundaries.clone(),
    );
    let structured_payload = json!(agent_input);
    let input_digest = format!("sha256:{}", digest_json(&structured_payload));
    let envelope = LlmAgentRequestEnvelope::new(
        new_id("req"),
        CONVERSATION_STATE_WRITER_AGENT_TYPE,
        CONVERSATION_STATE_WRITER_PROFILE_ID,
        mode.as_str(),
        input.trace_id,
        prepared.user_session_id.clone(),
        prepared.interaction_context_id.clone(),
        llm_agent_projection_ref(
            input.trace_id,
            CONVERSATION_STATE_WRITER_PROFILE_ID,
            &input_digest,
        ),
        input_digest,
        CONVERSATION_STATE_WRITER_TIMEOUT_MS,
        structured_payload,
    );
    let output = execute_llm_agent_profile(
        runtime_client,
        CONVERSATION_STATE_WRITER_PROFILE_ID,
        &envelope,
        None,
    )
    .await;
    let decision = match output {
        Ok(output) => validate_conversation_state_runtime_output(
            mode,
            &envelope,
            &output.result_summary,
            output.result_ref.as_deref(),
            Some(&output.metadata),
            &validation_context,
        ),
        Err(error) => conversation_state_runtime_error_decision(mode, &envelope, error.to_string()),
    };
    if decision.contract_accepted() {
        return decision;
    }
    let first_error_digest = error_digest(decision.errors());
    let repaired = execute_llm_agent_profile(
        runtime_client,
        CONVERSATION_STATE_WRITER_PROFILE_ID,
        &envelope,
        Some(decision.errors()),
    )
    .await;
    match repaired {
        Ok(output) => validate_conversation_state_runtime_output(
            mode,
            &envelope,
            &output.result_summary,
            output.result_ref.as_deref(),
            Some(&output.metadata),
            &validation_context,
        )
        .with_repair_metadata(true, Some(first_error_digest)),
        Err(_) => decision.with_repair_metadata(true, Some(first_error_digest)),
    }
}

async fn execute_llm_agent_profile(
    runtime_client: &dyn RuntimeClient,
    profile_id: &str,
    envelope: &LlmAgentRequestEnvelope,
    repair_errors: Option<&[String]>,
) -> Result<RuntimeOutput> {
    let contract = if profile_id == QUESTION_NORMALIZER_PROFILE_ID {
        question_normalizer_profile_contract()
    } else {
        conversation_state_writer_profile_contract()
    };
    let runtime_step = RuntimeStep::from_profile_contract(
        &contract,
        json!({
            "agent_request_id": &envelope.agent_request_id,
            "projection_ref": &envelope.projection_ref,
            "input_digest": &envelope.input_digest,
            "raw_output_must_not_be_persisted": true,
        }),
    );
    let provider_prompt = build_llm_agent_provider_prompt(profile_id, envelope, repair_errors)?;
    runtime_client
        .execute_profile_step(RuntimeProfileInput {
            profile_id: profile_id.to_string(),
            messages: vec![
                RuntimeProfileMessage::new("system", provider_prompt.system_prompt),
                RuntimeProfileMessage::new("user", provider_prompt.user_payload),
            ],
            metadata: json!({
                "agent_request_id": &envelope.agent_request_id,
                "projection_ref": &envelope.projection_ref,
                "input_digest": &envelope.input_digest,
                "raw_output_persisted": false,
                "repair_attempt": repair_errors.is_some(),
            }),
            profile_contract: Some(contract),
            runtime_step: Some(runtime_step),
            requested_tools: Vec::new(),
            trace_id: envelope.trace_id.clone(),
        })
        .await
        .map_err(|error| anyhow!("{error}"))
}

fn enforced_llm_agent_rejection(
    profile_id: &str,
    audit: &Value,
    errors: &[String],
) -> anyhow::Error {
    let decision = audit
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let contract_accepted = audit
        .get("contract_accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let error_summary = bounded_summary(&errors.join("; "), 360);
    anyhow!(
        "llm_agent_enforced_rejected: profile_id={profile_id}; decision={decision}; contract_accepted={contract_accepted}; error_digest={}; error_summary={error_summary}",
        error_digest(errors)
    )
}

fn allowed_referents_for_agent(
    question: &str,
    messages: &[ContextMessage],
    prior_subject: Option<&str>,
    deterministic: &ResolverOutput,
) -> Result<Vec<String>> {
    let mut referents = deterministic
        .referent_bindings
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if let Some(subject) = latest_subject_in_text(question)? {
        referents.insert(subject);
    }
    if let Some(subject) = latest_subject_from_messages(messages)? {
        referents.insert(subject);
    }
    if let Some(subject) = prior_subject {
        referents.insert(subject.to_string());
    }
    let max_candidates = context_rules::max_referent_candidates()?;
    Ok(referents.into_iter().take(max_candidates).collect())
}

fn recent_messages_for_agent(messages: &[ContextMessage], role: &str) -> Vec<String> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == role)
        .take(4)
        .map(|message| bounded_summary(&message.content, 360))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn llm_agent_projection_ref(trace_id: &str, profile_id: &str, input_digest: &str) -> String {
    let digest = input_digest
        .strip_prefix("sha256:")
        .unwrap_or(input_digest)
        .chars()
        .take(16)
        .collect::<String>();
    format!("llm-agent-input://tonglingyu/{trace_id}/{profile_id}/{digest}")
}

pub(crate) fn load_deduped_final_response(
    conn: &Connection,
    user_session_id: &str,
    external_message_id: &str,
) -> Result<Option<Value>> {
    conn.query_row(
        "SELECT metadata_json FROM session_journal
         WHERE user_session_id = ?1 AND external_message_id = ?2 AND entry_type = 'final_response'
         ORDER BY created_at DESC LIMIT 1",
        params![user_session_id, external_message_id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|metadata| {
        let value: Value = serde_json::from_str(&metadata)?;
        value
            .get("response")
            .cloned()
            .context("final response journal missing response")
    })
    .transpose()
}

#[derive(Debug, Clone, Default)]
struct PublicAnswerBoundary {
    boundary: Option<String>,
    evidence_package_refs: Vec<String>,
}

fn latest_public_answer_boundary(
    conn: &Connection,
    user_session_id: &str,
) -> Result<PublicAnswerBoundary> {
    let row = conn
        .query_row(
            "SELECT package_id, metadata_json FROM session_journal
             WHERE user_session_id = ?1 AND entry_type = 'final_response'
             ORDER BY created_at DESC, journal_id DESC LIMIT 1",
            params![user_session_id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((package_id, metadata_json)) = row else {
        return Ok(PublicAnswerBoundary::default());
    };
    let metadata = serde_json::from_str::<Value>(&metadata_json)?;
    let boundary = metadata
        .get("response")
        .and_then(public_response_boundary)
        .map(|value| bounded_summary(&value, CONVERSATION_STATE_BOUNDARY_MAX_CHARS));
    let evidence_package_refs = package_id
        .map(|id| format!("package:{id}"))
        .into_iter()
        .collect::<Vec<_>>();
    Ok(PublicAnswerBoundary {
        boundary,
        evidence_package_refs,
    })
}

fn public_response_boundary(response: &Value) -> Option<String> {
    let content = public_response_content(response)?;
    Some(deterministic_public_answer_boundary(content))
}

fn public_response_content(response: &Value) -> Option<&str> {
    response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| {
            response
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
        })
        .or_else(|| response.get("content").and_then(Value::as_str))
        .filter(|content| !content.trim().is_empty())
}

fn deterministic_public_answer_boundary(content: &str) -> String {
    let content = compact_boundary_text(content);
    if content.is_empty() {
        return "(empty)".to_string();
    }
    if is_self_contained_public_boundary(&content)
        && content.chars().count() <= CONVERSATION_STATE_BOUNDARY_MAX_CHARS
    {
        return content;
    }

    let subject = latest_subject_in_text(&content).unwrap_or(None);
    let clause = extract_public_answer_boundary_clause(&content);
    let boundary = match (subject, clause) {
        (Some(subject), Some(clause)) => format!("上一轮回答围绕{subject}；{clause}"),
        (Some(subject), None) => {
            format!("上一轮回答仅限于{subject}相关可追溯文本事实，不作为额外人物定论。")
        }
        (None, Some(clause)) => format!("上一轮回答边界：{clause}"),
        (None, None) => "上一轮回答仅限于已返回的可追溯文本事实，不作为额外定论。".to_string(),
    };
    bounded_summary(&boundary, CONVERSATION_STATE_BOUNDARY_MAX_CHARS)
}

fn compact_boundary_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_self_contained_public_boundary(value: &str) -> bool {
    value.starts_with("上一轮") && contains_boundary_signal(value)
}

fn extract_public_answer_boundary_clause(content: &str) -> Option<String> {
    split_public_answer_sentences(content)
        .into_iter()
        .map(|sentence| normalize_public_answer_boundary_clause(&sentence))
        .find(|sentence| contains_boundary_signal(sentence))
}

fn split_public_answer_sentences(content: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for ch in content.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '!' | '？' | '?' | '；' | ';' | '\n') {
            let sentence = current.trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_string());
            }
            current.clear();
        }
    }
    let tail = current.trim();
    if !tail.is_empty() {
        sentences.push(tail.to_string());
    }
    sentences
}

fn normalize_public_answer_boundary_clause(sentence: &str) -> String {
    let mut value = sentence
        .trim()
        .trim_matches(|ch| matches!(ch, '。' | '！' | '!' | '？' | '?' | '；' | ';'))
        .trim();
    for prefix in [
        "但需要注意：",
        "需要注意：",
        "但需注意：",
        "需注意：",
        "注意：",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.trim();
            break;
        }
    }
    bounded_summary(value, CONVERSATION_STATE_BOUNDARY_MAX_CHARS)
}

fn contains_boundary_signal(value: &str) -> bool {
    [
        "只依据",
        "仅依据",
        "只基于",
        "仅基于",
        "只确认",
        "仅确认",
        "不能",
        "不可",
        "不得",
        "不把",
        "不扩展",
        "不作为",
        "未说明",
        "未断言",
        "版本差异",
        "证据不足",
        "需要降级",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

pub(crate) fn append_final_response(
    conn: &Connection,
    input: FinalResponseJournalInput<'_>,
) -> Result<()> {
    append_journal_entry(
        conn,
        JournalEntryInput {
            trace_id: input.trace_id,
            user_session_id: input.user_session_id,
            interaction_context_id: input.interaction_context_id,
            context_pack_id: Some(input.context_pack_id),
            package_id: input.package_id,
            external_message_id: Some(input.external_message_id),
            entry_type: "final_response",
            content: None,
            summary: "final response recorded",
            retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
            sensitivity: "internal_response_cache",
            metadata: json!({
                "package_id": input.package_id,
                "response": input.response,
                "raw_content_in_default_admin_trace": false,
            }),
        },
    )
}

pub(crate) fn append_runtime_step_journal(
    conn: &Connection,
    trace_id: &str,
    user_session_id: &str,
    interaction_context_id: &str,
    context_pack_id: &str,
    package_id: Option<&str>,
    detail: Value,
) -> Result<()> {
    append_journal_entry(
        conn,
        JournalEntryInput {
            trace_id,
            user_session_id,
            interaction_context_id,
            context_pack_id: Some(context_pack_id),
            package_id,
            external_message_id: None,
            entry_type: "runtime_step",
            content: None,
            summary: "runtime workflow executed",
            retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
            sensitivity: "internal_runtime_trace",
            metadata: json!({
                "package_id": package_id,
                "detail": detail,
            }),
        },
    )
}

pub(crate) fn append_review_journal(
    conn: &Connection,
    trace_id: &str,
    user_session_id: &str,
    interaction_context_id: &str,
    context_pack_id: &str,
    package_id: Option<&str>,
    review: Value,
) -> Result<()> {
    append_journal_entry(
        conn,
        JournalEntryInput {
            trace_id,
            user_session_id,
            interaction_context_id,
            context_pack_id: Some(context_pack_id),
            package_id,
            external_message_id: None,
            entry_type: "review_result",
            content: None,
            summary: "review result recorded",
            retention_policy: JOURNAL_RETENTION_POLICY_VERSION,
            sensitivity: "internal_review_trace",
            metadata: json!({
                "package_id": package_id,
                "review": review,
            }),
        },
    )
}

pub(crate) fn load_trace_context(conn: &Connection, trace_id: &str) -> Result<Value> {
    let context_packs = load_context_packs(conn, trace_id)?;
    let context_projections = load_context_projections(conn, trace_id)?;
    let journal = load_journal_summaries_for_trace(conn, trace_id)?;
    Ok(json!({
        "object": "tonglingyu.scoped_context_trace",
        "schema_version": CONTEXT_SCHEMA_VERSION,
        "trace_id": trace_id,
        "context_packs": context_packs,
        "context_projections": context_projections,
        "session_journal": journal,
        "raw_content_included": false,
    }))
}

pub(crate) fn load_session(conn: &Connection, user_session_id: &str) -> Result<Option<Value>> {
    let session = conn
        .query_row(
            "SELECT user_session_id, external_user_ref, external_session_id, model_id,
                    lifecycle_status, retention_policy, created_at, updated_at
             FROM user_sessions WHERE user_session_id = ?1",
            params![user_session_id],
            |row| {
                Ok(json!({
                    "user_session_id": row.get::<_, String>(0)?,
                    "external_user_ref": row.get::<_, String>(1)?,
                    "external_session_id": row.get::<_, String>(2)?,
                    "model_id": row.get::<_, String>(3)?,
                    "lifecycle_status": row.get::<_, String>(4)?,
                    "retention_policy": row.get::<_, String>(5)?,
                    "created_at": row.get::<_, String>(6)?,
                    "updated_at": row.get::<_, String>(7)?,
                }))
            },
        )
        .optional()?;
    let Some(session) = session else {
        return Ok(None);
    };
    let contexts = load_contexts_for_session(conn, user_session_id)?;
    let journal = load_journal_summaries_for_session(conn, user_session_id)?;
    Ok(Some(json!({
        "object": "tonglingyu.user_session",
        "schema_version": CONTEXT_SCHEMA_VERSION,
        "session": session,
        "interaction_contexts": contexts,
        "session_journal": journal,
        "raw_content_included": false,
    })))
}

pub(crate) fn table_counts(conn: &Connection) -> Result<Value> {
    Ok(json!({
        "user_sessions": table_count(conn, "user_sessions")?,
        "interaction_contexts": table_count(conn, "interaction_contexts")?,
        "context_packs": table_count(conn, "context_packs")?,
        "context_projections": table_count(conn, "context_projections")?,
        "session_journal": table_count(conn, "session_journal")?,
        "memory_candidates": table_count(conn, "memory_candidates")?,
        "memory_cards": table_count(conn, "memory_cards")?,
        "memory_policy_decisions": table_count(conn, "memory_policy_decisions")?,
        "memory_transition_audit": table_count(conn, "memory_transition_audit")?,
        "memory_collector_runs": table_count(conn, "memory_collector_runs")?,
    }))
}

pub(crate) fn run_memory_collector(
    conn: &Connection,
    input: MemoryCollectorRunInput<'_>,
) -> Result<Value> {
    validate_memory_collector_trigger(input.trigger_type)?;
    let actor = non_empty_or(input.actor, "system");
    let limit = clamp_list_limit(input.limit, 100);
    let run_id = format!("memory-collector-run-{}", uuid::Uuid::now_v7().simple());
    let lease_owner = run_id.clone();
    if !input.dry_run && !acquire_memory_collector_lease(conn, &lease_owner)? {
        return Err(anyhow!("memory collector lease is held by another owner"));
    }
    let started_at = now_rfc3339();
    let audit_ref = memory_audit_ref("collector-run", &run_id);
    if !input.dry_run {
        conn.execute(
            "INSERT INTO memory_collector_runs (
                run_id, trigger_type, actor, status, dry_run, processed_count,
                candidate_count, suppressed_count, denied_count, duplicate_count, error_count,
                lease_owner, watermark_journal_id, started_at, completed_at, audit_ref, error, schema_version
            ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, 0, 0, 0, ?4, NULL, ?5, NULL, ?6, NULL, ?7)",
            params![
                &run_id,
                input.trigger_type,
                actor,
                &lease_owner,
                &started_at,
                &audit_ref,
                MEMORY_COLLECTOR_POLICY_VERSION,
            ],
        )?;
    }

    let mut processed_count = 0_i64;
    let mut candidate_count = 0_i64;
    let mut suppressed_count = 0_i64;
    let mut denied_count = 0_i64;
    let mut duplicate_count = 0_i64;
    let error_count = 0_i64;
    let mut watermark_journal_id = None::<String>;
    let mut candidate_summaries = Vec::new();
    let mut policy_decision_summaries = Vec::new();
    let mut auto_enabled_count = 0_i64;
    let mut suppressed = Vec::new();
    let sources = load_collectable_journal_rows(conn, limit, input.trace_id)?;
    for source in sources {
        processed_count += 1;
        watermark_journal_id = Some(source.journal_id.clone());
        if let Some(reason) = memory_source_deny_reason(&source) {
            denied_count += 1;
            if !input.dry_run {
                record_memory_collector_journal_status(
                    conn,
                    &source.journal_id,
                    &run_id,
                    "denied",
                    Some(reason),
                )?;
                append_memory_transition_audit(
                    conn,
                    MemoryTransitionAuditInput {
                        entity_type: "memory_candidate",
                        entity_id: None,
                        action: "collector_hard_deny",
                        from_status: None,
                        to_status: None,
                        actor,
                        reason: Some(reason),
                        metadata: json!({
                        "journal_id": &source.journal_id,
                        "trace_id": &source.trace_id,
                        "source_entry_type": &source.entry_type,
                        "llm_called": false,
                        "reason": reason,
                        }),
                    },
                )?;
            }
            suppressed.push(json!({
                "journal_id": &source.journal_id,
                "status": "denied",
                "reason": reason,
                "llm_called": false,
            }));
            continue;
        }
        match extract_memory_candidate(&source) {
            MemoryExtractionOutcome::Candidate(draft) => {
                let inserted = if input.dry_run {
                    true
                } else {
                    insert_memory_candidate(conn, draft.as_ref(), actor)?
                };
                if inserted {
                    candidate_count += 1;
                    let mut summary = memory_candidate_summary_json(draft.as_ref());
                    if !input.dry_run {
                        let policy_result =
                            apply_scoped_memory_policy_for_candidate(conn, draft.as_ref(), actor)?;
                        if policy_result.auto_read_enabled {
                            auto_enabled_count += 1;
                        }
                        summary["policy_result"] = policy_result.public_summary.clone();
                        policy_decision_summaries.extend(policy_result.policy_decision_summaries);
                    } else {
                        summary["policy_result"] = json!({
                            "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                            "policy_mode": memory_policy_mode(),
                            "dry_run": true,
                        });
                    }
                    candidate_summaries.push(summary);
                    if !input.dry_run {
                        record_memory_collector_journal_status(
                            conn,
                            &source.journal_id,
                            &run_id,
                            "extracted",
                            None,
                        )?;
                    }
                } else {
                    duplicate_count += 1;
                    if !input.dry_run {
                        record_memory_collector_journal_status(
                            conn,
                            &source.journal_id,
                            &run_id,
                            "duplicate",
                            Some("candidate already exists"),
                        )?;
                    }
                    suppressed.push(json!({
                        "journal_id": &source.journal_id,
                        "status": "duplicate",
                        "reason": "candidate already exists",
                    }));
                }
            }
            MemoryExtractionOutcome::Suppressed { reason, confidence } => {
                suppressed_count += 1;
                if !input.dry_run {
                    record_memory_collector_journal_status(
                        conn,
                        &source.journal_id,
                        &run_id,
                        "suppressed",
                        Some(reason),
                    )?;
                    append_memory_transition_audit(
                        conn,
                        MemoryTransitionAuditInput {
                            entity_type: "memory_candidate",
                            entity_id: None,
                            action: "collector_suppressed",
                            from_status: None,
                            to_status: None,
                            actor,
                            reason: Some(reason),
                            metadata: json!({
                            "journal_id": &source.journal_id,
                            "trace_id": &source.trace_id,
                            "confidence": confidence,
                            "llm_called": false,
                            "reason": reason,
                            }),
                        },
                    )?;
                }
                suppressed.push(json!({
                    "journal_id": &source.journal_id,
                    "status": "suppressed",
                    "reason": reason,
                    "confidence": confidence,
                    "llm_called": false,
                }));
            }
        }
    }
    let completed_at = now_rfc3339();
    if !input.dry_run {
        conn.execute(
            "UPDATE memory_collector_runs
             SET status = 'ok', processed_count = ?1, candidate_count = ?2,
                 suppressed_count = ?3, denied_count = ?4, duplicate_count = ?5,
                 error_count = ?6, watermark_journal_id = ?7, completed_at = ?8
             WHERE run_id = ?9",
            params![
                processed_count,
                candidate_count,
                suppressed_count,
                denied_count,
                duplicate_count,
                error_count,
                watermark_journal_id.as_deref(),
                &completed_at,
                &run_id,
            ],
        )?;
        release_memory_collector_lease(conn, &lease_owner)?;
    }
    Ok(json!({
        "object": "tonglingyu.memory_collector_run",
        "schema_version": MEMORY_COLLECTOR_POLICY_VERSION,
        "status": "ok",
        "run_id": run_id,
        "trigger_type": input.trigger_type,
        "dry_run": input.dry_run,
        "trace_id": input.trace_id,
        "processed_count": processed_count,
        "candidate_count": candidate_count,
        "suppressed_count": suppressed_count,
        "denied_count": denied_count,
        "duplicate_count": duplicate_count,
        "auto_enabled_count": auto_enabled_count,
        "error_count": error_count,
        "watermark_journal_id": watermark_journal_id,
        "started_at": started_at,
        "completed_at": completed_at,
        "llm_boundary": llm_boundary_contract_json(),
        "memory_policy": scoped_memory_policy_public_contract(),
        "policy_decisions": policy_decision_summaries,
        "candidates": candidate_summaries,
        "suppressed": suppressed,
        "secret_values_printed": false,
    }))
}

pub(crate) fn list_memory_candidates(
    conn: &Connection,
    input: MemoryCandidateListInput<'_>,
) -> Result<Value> {
    validate_optional_filter(
        input.status,
        "candidate status",
        allowed_memory_candidate_statuses(),
    )?;
    validate_optional_filter(
        input.scope_type,
        "candidate scope",
        allowed_memory_scope_types(),
    )?;
    let limit = clamp_list_limit(input.limit, 100) as i64;
    let offset = input.offset.min(10_000) as i64;
    let mut stmt = conn.prepare(
        "SELECT candidate_id, candidate_ref, status, journal_id, trace_id, user_session_id,
                interaction_context_id, context_pack_id, source_entry_type, scope_type,
                scope_ref, candidate_type, summary, summary_sha256, raw_excerpt_redacted,
                raw_excerpt_sha256, sensitivity, risk_flags_json, llm_extraction_json,
                confidence, created_by, created_at, updated_at, expires_at,
                merged_into_candidate_id, audit_ref, schema_version
         FROM memory_candidates
         WHERE (?1 IS NULL OR status = ?1)
           AND (?2 IS NULL OR scope_type = ?2)
           AND (?3 IS NULL OR scope_ref = ?3)
         ORDER BY created_at DESC, candidate_id DESC
         LIMIT ?4 OFFSET ?5",
    )?;
    let rows = stmt.query_map(
        params![
            input.status,
            input.scope_type,
            input.scope_ref,
            limit,
            offset
        ],
        memory_candidate_row_json,
    )?;
    let candidates = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(json!({
        "object": "tonglingyu.memory_candidate_list",
        "schema_version": MEMORY_CANDIDATE_SCHEMA_VERSION,
        "items": candidates,
        "limit": limit,
        "offset": offset,
        "read_path_enabled": true,
    }))
}

pub(crate) fn list_memory_cards(
    conn: &Connection,
    input: MemoryCardListInput<'_>,
) -> Result<Value> {
    validate_optional_filter(
        input.status,
        "memory card status",
        allowed_memory_card_statuses(),
    )?;
    validate_optional_filter(
        input.scope_type,
        "memory card scope",
        allowed_memory_scope_types(),
    )?;
    let limit = clamp_list_limit(input.limit, 100) as i64;
    let offset = input.offset.min(10_000) as i64;
    let mut stmt = conn.prepare(
        "SELECT memory_card_id, memory_card_ref, source_candidate_id, status,
                scope_type, scope_ref, summary, summary_sha256, acl_json, sensitivity,
                promotion_policy_version, promoted_by, promoted_at, revoked_by,
                revoked_at, expires_at, read_enabled, audit_ref, schema_version
         FROM memory_cards
         WHERE (?1 IS NULL OR status = ?1)
           AND (?2 IS NULL OR scope_type = ?2)
           AND (?3 IS NULL OR scope_ref = ?3)
         ORDER BY promoted_at DESC, memory_card_id DESC
         LIMIT ?4 OFFSET ?5",
    )?;
    let rows = stmt.query_map(
        params![
            input.status,
            input.scope_type,
            input.scope_ref,
            limit,
            offset
        ],
        memory_card_row_json,
    )?;
    let cards = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(json!({
        "object": "tonglingyu.memory_card_list",
        "schema_version": MEMORY_CARD_SCHEMA_VERSION,
        "items": cards,
        "limit": limit,
        "offset": offset,
        "read_path_enabled": true,
    }))
}

pub(crate) fn transition_memory_candidate(
    conn: &Connection,
    input: MemoryCandidateTransitionInput<'_>,
) -> Result<Value> {
    validate_memory_candidate_action(input.action)?;
    let current = load_memory_candidate_core(conn, input.candidate_id)?
        .ok_or_else(|| anyhow!("memory candidate not found"))?;
    let actor = require_non_empty(input.actor, "operator identity is required")?;
    let reason = require_required_reason(input.reason)?;
    let now = now_rfc3339();
    match input.action {
        "approve" => {
            require_status(&current.status, &["pending"])?;
            approve_memory_candidate(
                conn,
                &current.candidate_id,
                &current.status,
                actor,
                reason,
                Some(json!({"candidate_ref": &current.candidate_ref})),
                input.expires_at,
            )?;
        }
        "reject" => {
            require_status(&current.status, &["pending"])?;
            update_candidate_status(
                conn,
                &current.candidate_id,
                "rejected",
                None,
                input.expires_at,
                &now,
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&current.candidate_id),
                    action: "reject",
                    from_status: Some(&current.status),
                    to_status: Some("rejected"),
                    actor,
                    reason: Some(reason),
                    metadata: json!({"candidate_ref": &current.candidate_ref}),
                },
            )?;
        }
        "expire" => {
            require_status(&current.status, &["pending"])?;
            update_candidate_status(
                conn,
                &current.candidate_id,
                "expired",
                None,
                Some(input.expires_at.unwrap_or(&now)),
                &now,
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&current.candidate_id),
                    action: "expire",
                    from_status: Some(&current.status),
                    to_status: Some("expired"),
                    actor,
                    reason: Some(reason),
                    metadata: json!({"candidate_ref": &current.candidate_ref}),
                },
            )?;
        }
        "merge" => {
            require_status(&current.status, &["pending"])?;
            let target_id = input
                .merge_target_candidate_id
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("merge_target_candidate_id is required"))?;
            if target_id == current.candidate_id {
                return Err(anyhow!("memory candidate cannot merge into itself"));
            }
            let target = load_memory_candidate_core(conn, target_id)?
                .ok_or_else(|| anyhow!("merge target candidate not found"))?;
            if current.scope_type != target.scope_type || current.scope_ref != target.scope_ref {
                return Err(anyhow!("merge target must be in the same scope"));
            }
            update_candidate_status(
                conn,
                &current.candidate_id,
                "merged",
                Some(target_id),
                input.expires_at,
                &now,
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&current.candidate_id),
                    action: "merge",
                    from_status: Some(&current.status),
                    to_status: Some("merged"),
                    actor,
                    reason: Some(reason),
                    metadata: json!({
                    "candidate_ref": &current.candidate_ref,
                    "merged_into_candidate_id": target_id,
                    "merged_into_candidate_ref": &target.candidate_ref,
                    }),
                },
            )?;
        }
        "reclassify" => {
            require_status(&current.status, &["pending"])?;
            let new_type = input
                .candidate_type
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(&current.candidate_type);
            validate_memory_candidate_type(new_type)?;
            let new_sensitivity = input
                .sensitivity
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(&current.sensitivity);
            conn.execute(
                "UPDATE memory_candidates
                 SET candidate_type = ?1, sensitivity = ?2, risk_flags_json = ?3, updated_at = ?4
                 WHERE candidate_id = ?5",
                params![
                    new_type,
                    new_sensitivity,
                    serde_json::to_string(&append_risk_flag(
                        current.risk_flags.clone(),
                        "admin_reclassified",
                    ))?,
                    &now,
                    &current.candidate_id,
                ],
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&current.candidate_id),
                    action: "reclassify",
                    from_status: Some("pending"),
                    to_status: Some("pending"),
                    actor,
                    reason: Some(reason),
                    metadata: json!({
                    "candidate_ref": &current.candidate_ref,
                    "previous_candidate_type": &current.candidate_type,
                    "new_candidate_type": new_type,
                    "previous_sensitivity": &current.sensitivity,
                    "new_sensitivity": new_sensitivity,
                    }),
                },
            )?;
        }
        "promote" => {
            require_status(&current.status, &["approved"])?;
            promote_memory_candidate(conn, &current, actor, reason)?;
        }
        _ => unreachable!("validated memory candidate action"),
    }
    let refreshed = read_memory_candidate(conn, input.candidate_id)?
        .ok_or_else(|| anyhow!("memory candidate not found after transition"))?;
    Ok(json!({
        "object": "tonglingyu.memory_candidate_transition",
        "schema_version": MEMORY_TRANSITION_AUDIT_SCHEMA_VERSION,
        "status": "ok",
        "action": input.action,
        "candidate": refreshed,
        "read_path_enabled": true,
    }))
}

pub(crate) fn transition_memory_card(
    conn: &Connection,
    input: MemoryCardTransitionInput<'_>,
) -> Result<Value> {
    validate_memory_card_action(input.action)?;
    let current = load_memory_card_core(conn, input.memory_card_id)?
        .ok_or_else(|| anyhow!("memory card not found"))?;
    require_status(&current.status, &["active"])?;
    let actor = require_non_empty(input.actor, "operator identity is required")?;
    let reason = require_required_reason(input.reason)?;
    let now = now_rfc3339();
    match input.action {
        "enable_read" => {
            let policy =
                manual_read_enable_policy_decision(conn, &current, actor, "enable_read", reason)?;
            set_memory_card_read_enabled(conn, &current, true, actor, reason, Some(&policy))?;
        }
        "disable_read" => {
            let policy =
                manual_read_enable_policy_decision(conn, &current, actor, "disable_read", reason)?;
            set_memory_card_read_enabled(conn, &current, false, actor, reason, Some(&policy))?;
        }
        "revoke" | "expire" => {
            let to_status = if input.action == "revoke" {
                "revoked"
            } else {
                "expired"
            };
            let mut acl = current.acl.clone();
            if let Some(object) = acl.as_object_mut() {
                object.insert("read_enabled".to_string(), json!(false));
            }
            conn.execute(
                "UPDATE memory_cards
                 SET status = ?1, revoked_by = ?2, revoked_at = ?3,
                     expires_at = COALESCE(expires_at, ?4), read_enabled = 0,
                     acl_json = ?5
                 WHERE memory_card_id = ?6",
                params![
                    to_status,
                    actor,
                    &now,
                    &now,
                    serde_json::to_string(&acl)?,
                    &current.memory_card_id
                ],
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_card",
                    entity_id: Some(&current.memory_card_id),
                    action: input.action,
                    from_status: Some(&current.status),
                    to_status: Some(to_status),
                    actor,
                    reason: Some(reason),
                    metadata: json!({
                    "memory_card_ref": &current.memory_card_ref,
                    "source_candidate_id": &current.source_candidate_id,
                    "read_enabled": false,
                    "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                    }),
                },
            )?;
        }
        _ => unreachable!("validated memory card action"),
    }
    let refreshed = read_memory_card(conn, input.memory_card_id)?
        .ok_or_else(|| anyhow!("memory card not found after transition"))?;
    let read_path_enabled = refreshed
        .get("read_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(json!({
        "object": "tonglingyu.memory_card_transition",
        "schema_version": MEMORY_TRANSITION_AUDIT_SCHEMA_VERSION,
        "status": "ok",
        "action": input.action,
        "memory_card": refreshed,
        "read_path_enabled": read_path_enabled,
    }))
}

pub(crate) fn read_memory_candidate(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<Value>> {
    conn.query_row(
        "SELECT candidate_id, candidate_ref, status, journal_id, trace_id, user_session_id,
                interaction_context_id, context_pack_id, source_entry_type, scope_type,
                scope_ref, candidate_type, summary, summary_sha256, raw_excerpt_redacted,
                raw_excerpt_sha256, sensitivity, risk_flags_json, llm_extraction_json,
                confidence, created_by, created_at, updated_at, expires_at,
                merged_into_candidate_id, audit_ref, schema_version
         FROM memory_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        memory_candidate_row_json,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn read_memory_card(conn: &Connection, memory_card_id: &str) -> Result<Option<Value>> {
    conn.query_row(
        "SELECT memory_card_id, memory_card_ref, source_candidate_id, status,
                scope_type, scope_ref, summary, summary_sha256, acl_json, sensitivity,
                promotion_policy_version, promoted_by, promoted_at, revoked_by,
                revoked_at, expires_at, read_enabled, audit_ref, schema_version
         FROM memory_cards WHERE memory_card_id = ?1",
        params![memory_card_id],
        memory_card_row_json,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn assert_read_enabled_memory_has_policy_decisions(conn: &Connection) -> Result<()> {
    let count = conn.query_row(
        "SELECT COUNT(*)
         FROM memory_cards AS card
         WHERE card.status = 'active'
           AND card.read_enabled <> 0
           AND NOT EXISTS (
             SELECT 1 FROM memory_policy_decisions AS decision
             WHERE decision.memory_card_id = card.memory_card_id
               AND decision.decision = 'enable_read'
               AND decision.policy_version = ?1
           )",
        params![SCOPED_MEMORY_POLICY_VERSION],
        |row| row.get::<_, i64>(0),
    )?;
    if count > 0 {
        return Err(anyhow!(
            "read-enabled memory cards without policy decision exist"
        ));
    }
    Ok(())
}

struct JournalEntryInput<'a> {
    trace_id: &'a str,
    user_session_id: &'a str,
    interaction_context_id: &'a str,
    context_pack_id: Option<&'a str>,
    package_id: Option<&'a str>,
    external_message_id: Option<&'a str>,
    entry_type: &'a str,
    content: Option<&'a str>,
    summary: &'a str,
    retention_policy: &'a str,
    sensitivity: &'a str,
    metadata: Value,
}

#[derive(Debug, Clone)]
struct MemorySourceRow {
    journal_id: String,
    trace_id: String,
    user_session_id: String,
    external_user_ref: String,
    interaction_context_id: String,
    context_pack_id: Option<String>,
    entry_type: String,
    content: Option<String>,
    summary: String,
    content_sha256: Option<String>,
    sensitivity: String,
    metadata: Value,
    created_at: String,
}

#[derive(Debug, Clone)]
struct MemoryCandidateDraft {
    candidate_id: String,
    candidate_ref: String,
    journal_id: String,
    trace_id: String,
    user_session_id: String,
    interaction_context_id: String,
    context_pack_id: Option<String>,
    source_entry_type: String,
    scope_type: String,
    scope_ref: String,
    candidate_type: String,
    summary: String,
    summary_sha256: String,
    raw_excerpt_redacted: String,
    raw_excerpt_sha256: String,
    sensitivity: String,
    risk_flags: Value,
    llm_extraction: Value,
    confidence: f64,
    audit_ref: String,
}

#[derive(Debug, Clone)]
enum MemoryExtractionOutcome {
    Candidate(Box<MemoryCandidateDraft>),
    Suppressed {
        reason: &'static str,
        confidence: f64,
    },
}

#[derive(Debug, Clone)]
struct MemoryCandidateCore {
    candidate_id: String,
    candidate_ref: String,
    status: String,
    source_entry_type: String,
    context_pack_id: Option<String>,
    scope_type: String,
    scope_ref: String,
    candidate_type: String,
    summary: String,
    summary_sha256: String,
    sensitivity: String,
    risk_flags: Value,
    confidence: f64,
}

#[derive(Debug, Clone)]
struct MemoryCardCore {
    memory_card_id: String,
    memory_card_ref: String,
    source_candidate_id: String,
    status: String,
    scope_type: String,
    scope_ref: String,
    sensitivity: String,
    acl: Value,
    read_enabled: bool,
    expires_at: Option<String>,
}

#[derive(Debug, Clone)]
struct MemoryPolicyDecisionDraft {
    candidate_id: String,
    memory_card_id: Option<String>,
    scope_type: String,
    scope_ref: String,
    candidate_type: String,
    rule_filter: Value,
    llm_filter: Value,
    confidence: f64,
    sensitivity: String,
    risk_flags: Value,
    decision: String,
    decision_reason: String,
    ttl_policy_ref: String,
    expires_at: Option<String>,
    actor: String,
}

#[derive(Debug, Clone)]
struct MemoryPolicyDecisionRecord {
    policy_decision_id: String,
    policy_decision_ref: String,
    summary: Value,
}

#[derive(Debug, Clone)]
struct MemoryPolicyApplication {
    auto_read_enabled: bool,
    public_summary: Value,
    policy_decision_summaries: Vec<Value>,
}

#[derive(Debug, Clone)]
struct ScopedMemoryRead {
    memory_card_ref: String,
    memory_read_ref: String,
    policy_decision_ref: String,
    policy_version: String,
    policy_mode: String,
    scope_type: String,
    candidate_type: String,
    summary: String,
    sensitivity: String,
    confidence: f64,
    expires_at: Option<String>,
    allowed_consumers: Vec<String>,
}

#[derive(Debug, Clone)]
struct ScopedMemoryReadSet {
    reads: Vec<ScopedMemoryRead>,
    candidate_count_before_budget: usize,
    truncated_count: usize,
}

struct MemoryTransitionAuditInput<'a> {
    entity_type: &'a str,
    entity_id: Option<&'a str>,
    action: &'a str,
    from_status: Option<&'a str>,
    to_status: Option<&'a str>,
    actor: &'a str,
    reason: Option<&'a str>,
    metadata: Value,
}

fn load_collectable_journal_rows(
    conn: &Connection,
    limit: usize,
    trace_id: Option<&str>,
) -> Result<Vec<MemorySourceRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_journal.journal_id, session_journal.trace_id, session_journal.user_session_id,
                user_sessions.external_user_ref, session_journal.interaction_context_id,
                session_journal.context_pack_id, session_journal.entry_type, session_journal.content,
                session_journal.summary, session_journal.content_sha256, session_journal.sensitivity,
                session_journal.metadata_json, session_journal.created_at
         FROM session_journal
         JOIN user_sessions ON user_sessions.user_session_id = session_journal.user_session_id
         WHERE (?1 IS NULL OR trace_id = ?1)
           AND session_journal.entry_type = 'user_message'
           AND session_journal.context_pack_id IS NOT NULL
           AND EXISTS (
             SELECT 1 FROM session_journal AS completed
             WHERE completed.trace_id = session_journal.trace_id
               AND completed.context_pack_id = session_journal.context_pack_id
               AND completed.entry_type = 'final_response'
           )
           AND NOT EXISTS (
             SELECT 1 FROM memory_collector_journal_status AS status
             WHERE status.journal_id = session_journal.journal_id
         )
         ORDER BY session_journal.created_at, session_journal.journal_id
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![trace_id, clamp_list_limit(limit, 100) as i64],
        |row| {
            Ok(MemorySourceRow {
                journal_id: row.get(0)?,
                trace_id: row.get(1)?,
                user_session_id: row.get(2)?,
                external_user_ref: row.get(3)?,
                interaction_context_id: row.get(4)?,
                context_pack_id: row.get(5)?,
                entry_type: row.get(6)?,
                content: row.get(7)?,
                summary: row.get(8)?,
                content_sha256: row.get(9)?,
                sensitivity: row.get(10)?,
                metadata: parse_json_column(row.get::<_, String>(11)?),
                created_at: row.get(12)?,
            })
        },
    )?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn memory_source_deny_reason(source: &MemorySourceRow) -> Option<&'static str> {
    if source.entry_type != "user_message" {
        return Some("source_entry_type_denied");
    }
    let text = source.content.as_deref().unwrap_or_default();
    if text.trim().is_empty() {
        return Some("empty_source_denied");
    }
    if is_openwebui_metadata_prompt(text) {
        return Some("openwebui_metadata_prompt_denied");
    }
    let lowered = text.to_ascii_lowercase();
    if lowered.contains("sk-")
        || lowered.contains("api_key")
        || lowered.contains("api-key")
        || lowered.contains("token=")
        || lowered.contains("password=")
        || text.contains("密钥")
        || text.contains("令牌")
        || text.contains("密码")
    {
        return Some("secret_or_token_denied");
    }
    if lowered.contains("system prompt")
        || text.contains("系统提示词")
        || text.contains("系统 prompt")
    {
        return Some("system_prompt_denied");
    }
    None
}

fn extract_memory_candidate(source: &MemorySourceRow) -> MemoryExtractionOutcome {
    let raw_text = source.content.as_deref().unwrap_or(&source.summary);
    let (redacted, redaction_applied) = redact_sensitive_text(raw_text);
    let durable_signal = durable_memory_signal(&redacted);
    if durable_signal.is_none() {
        return MemoryExtractionOutcome::Suppressed {
            reason: "no_durable_memory_signal",
            confidence: 0.2,
        };
    }
    let confidence = durable_signal.unwrap_or(0.2);
    if confidence < 0.45 {
        return MemoryExtractionOutcome::Suppressed {
            reason: "confidence_below_candidate_threshold",
            confidence,
        };
    }
    let candidate_id = format!("memory-candidate-{}", uuid::Uuid::now_v7().simple());
    let candidate_ref = format!(
        "memory-candidate://tonglingyu/{}/{}",
        source.trace_id, candidate_id
    );
    let candidate_type = classify_memory_candidate_type(&redacted);
    let summary = memory_candidate_summary(&candidate_type, &redacted);
    let mut risk_flags = Vec::<Value>::new();
    if confidence < 0.75 {
        risk_flags.push(json!("low_confidence"));
        risk_flags.push(json!("requires_manual_review"));
    }
    if redaction_applied {
        risk_flags.push(json!("redaction_applied"));
    }
    let llm_extraction = json!({
        "schema_version": "tonglingyu-memory-extraction-v1",
        "policy_version": MEMORY_COLLECTOR_POLICY_VERSION,
        "extractor": "deterministic_rules",
        "hard_deny_filter_passed": true,
        "redaction_applied": redaction_applied,
        "confidence": confidence,
        "confidence_rule": confidence_rule(confidence),
        "llm_participation": llm_boundary_contract_json(),
        "input_digest": {
            "journal_summary_sha256": hash_text(&source.summary),
            "redacted_excerpt_sha256": hash_text(&redacted),
            "content_sha256": &source.content_sha256,
        },
        "source": {
            "entry_type": &source.entry_type,
            "sensitivity": &source.sensitivity,
            "created_at": &source.created_at,
            "metadata_keys": json_object_keys(&source.metadata),
        },
    });
    MemoryExtractionOutcome::Candidate(Box::new(MemoryCandidateDraft {
        candidate_id: candidate_id.clone(),
        candidate_ref,
        journal_id: source.journal_id.clone(),
        trace_id: source.trace_id.clone(),
        user_session_id: source.user_session_id.clone(),
        interaction_context_id: source.interaction_context_id.clone(),
        context_pack_id: source.context_pack_id.clone(),
        source_entry_type: source.entry_type.clone(),
        scope_type: "user_private".to_string(),
        scope_ref: user_private_scope_ref(&source.external_user_ref),
        candidate_type,
        summary_sha256: hash_text(&summary),
        raw_excerpt_sha256: hash_text(&redacted),
        raw_excerpt_redacted: bounded_summary(&redacted, MEMORY_RAW_EXCERPT_MAX_CHARS),
        summary,
        sensitivity: "user_private_memory_candidate".to_string(),
        risk_flags: Value::Array(risk_flags),
        llm_extraction,
        confidence,
        audit_ref: memory_audit_ref("candidate-create", &candidate_id),
    }))
}

fn insert_memory_candidate(
    conn: &Connection,
    draft: &MemoryCandidateDraft,
    actor: &str,
) -> Result<bool> {
    validate_memory_scope_type(&draft.scope_type)?;
    let now = now_rfc3339();
    let rows = conn.execute(
        "INSERT OR IGNORE INTO memory_candidates (
            candidate_id, candidate_ref, status, journal_id, trace_id, user_session_id,
            interaction_context_id, context_pack_id, source_entry_type, scope_type,
            scope_ref, candidate_type, summary, summary_sha256, raw_excerpt_redacted,
            raw_excerpt_sha256, sensitivity, risk_flags_json, llm_extraction_json,
            confidence, created_by, created_at, updated_at, expires_at,
            merged_into_candidate_id, audit_ref, schema_version
        ) VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                  ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, NULL, NULL,
                  ?23, ?24)",
        params![
            &draft.candidate_id,
            &draft.candidate_ref,
            &draft.journal_id,
            &draft.trace_id,
            &draft.user_session_id,
            &draft.interaction_context_id,
            draft.context_pack_id.as_deref(),
            &draft.source_entry_type,
            &draft.scope_type,
            &draft.scope_ref,
            &draft.candidate_type,
            &draft.summary,
            &draft.summary_sha256,
            &draft.raw_excerpt_redacted,
            &draft.raw_excerpt_sha256,
            &draft.sensitivity,
            serde_json::to_string(&draft.risk_flags)?,
            serde_json::to_string(&draft.llm_extraction)?,
            draft.confidence,
            actor,
            &now,
            &now,
            &draft.audit_ref,
            MEMORY_CANDIDATE_SCHEMA_VERSION,
        ],
    )?;
    if rows == 0 {
        return Ok(false);
    }
    append_memory_transition_audit(
        conn,
        MemoryTransitionAuditInput {
            entity_type: "memory_candidate",
            entity_id: Some(&draft.candidate_id),
            action: "collector_create",
            from_status: None,
            to_status: Some("pending"),
            actor,
            reason: Some("collector extracted candidate"),
            metadata: json!({
            "candidate_ref": &draft.candidate_ref,
            "journal_id": &draft.journal_id,
            "trace_id": &draft.trace_id,
            "scope_type": &draft.scope_type,
            "scope_ref_sha256": hash_text(&draft.scope_ref),
            "candidate_type": &draft.candidate_type,
            "confidence": draft.confidence,
            "llm_called": false,
            }),
        },
    )?;
    Ok(true)
}

fn apply_scoped_memory_policy_for_candidate(
    conn: &Connection,
    draft: &MemoryCandidateDraft,
    collector_actor: &str,
) -> Result<MemoryPolicyApplication> {
    let policy_mode = memory_policy_mode();
    let policy_actor = if policy_mode == MEMORY_POLICY_MODE_AUTO {
        MEMORY_POLICY_ACTOR
    } else {
        collector_actor
    };
    let rule_filter = scoped_memory_rule_filter(draft);
    let llm_filter = scoped_memory_semantic_filter(draft, &rule_filter);
    let ttl = ttl_days_for_candidate_type(&draft.candidate_type)
        .unwrap_or_else(|| ttl_days_for_decision("pending_manual_review"));
    let expires_at = Some(rfc3339_after_days(ttl));
    let mut decisions = Vec::new();
    let mut auto_read_enabled = false;
    let reason = scoped_memory_policy_reason(draft, &rule_filter, &llm_filter, &policy_mode);

    if reason.auto_enable {
        let approve = record_memory_policy_decision(
            conn,
            MemoryPolicyDecisionDraft {
                candidate_id: draft.candidate_id.clone(),
                memory_card_id: None,
                scope_type: draft.scope_type.clone(),
                scope_ref: draft.scope_ref.clone(),
                candidate_type: draft.candidate_type.clone(),
                rule_filter: rule_filter.clone(),
                llm_filter: llm_filter.clone(),
                confidence: draft.confidence,
                sensitivity: draft.sensitivity.clone(),
                risk_flags: draft.risk_flags.clone(),
                decision: "auto_approve".to_string(),
                decision_reason: reason.reason.clone(),
                ttl_policy_ref: ttl_policy_ref(&draft.candidate_type, ttl),
                expires_at: expires_at.clone(),
                actor: policy_actor.to_string(),
            },
        )?;
        decisions.push(approve.summary.clone());
        approve_memory_candidate(
            conn,
            &draft.candidate_id,
            "pending",
            policy_actor,
            "scoped memory policy auto approve",
            Some(json!({
                "policy_decision_id": &approve.policy_decision_id,
                "policy_decision_ref": &approve.policy_decision_ref,
                "policy_version": SCOPED_MEMORY_POLICY_VERSION,
            })),
            expires_at.as_deref(),
        )?;
        let current = load_memory_candidate_core(conn, &draft.candidate_id)?
            .ok_or_else(|| anyhow!("auto-approved candidate not found"))?;
        let promote = record_memory_policy_decision(
            conn,
            MemoryPolicyDecisionDraft {
                candidate_id: draft.candidate_id.clone(),
                memory_card_id: None,
                scope_type: draft.scope_type.clone(),
                scope_ref: draft.scope_ref.clone(),
                candidate_type: draft.candidate_type.clone(),
                rule_filter: rule_filter.clone(),
                llm_filter: llm_filter.clone(),
                confidence: draft.confidence,
                sensitivity: draft.sensitivity.clone(),
                risk_flags: draft.risk_flags.clone(),
                decision: "auto_promote".to_string(),
                decision_reason: reason.reason.clone(),
                ttl_policy_ref: ttl_policy_ref(&draft.candidate_type, ttl),
                expires_at: expires_at.clone(),
                actor: policy_actor.to_string(),
            },
        )?;
        decisions.push(promote.summary.clone());
        let card = promote_memory_candidate_with_options(
            conn,
            &current,
            policy_actor,
            "scoped memory policy auto promote",
            expires_at.as_deref(),
            false,
            Some(&promote),
        )?;
        let enable = record_memory_policy_decision(
            conn,
            MemoryPolicyDecisionDraft {
                candidate_id: draft.candidate_id.clone(),
                memory_card_id: Some(card.memory_card_id.clone()),
                scope_type: draft.scope_type.clone(),
                scope_ref: draft.scope_ref.clone(),
                candidate_type: draft.candidate_type.clone(),
                rule_filter: rule_filter.clone(),
                llm_filter: llm_filter.clone(),
                confidence: draft.confidence,
                sensitivity: draft.sensitivity.clone(),
                risk_flags: draft.risk_flags.clone(),
                decision: "enable_read".to_string(),
                decision_reason: reason.reason.clone(),
                ttl_policy_ref: ttl_policy_ref(&draft.candidate_type, ttl),
                expires_at: expires_at.clone(),
                actor: policy_actor.to_string(),
            },
        )?;
        decisions.push(enable.summary.clone());
        set_memory_card_read_enabled(
            conn,
            &card,
            true,
            policy_actor,
            "scoped memory policy enable read",
            Some(&enable),
        )?;
        auto_read_enabled = true;
    } else {
        let decision = if reason.suppress {
            "suppress"
        } else {
            "pending_manual_review"
        };
        let ttl_days = if decision == "pending_manual_review" {
            ttl_days_for_decision("pending_manual_review")
        } else {
            ttl
        };
        let policy = record_memory_policy_decision(
            conn,
            MemoryPolicyDecisionDraft {
                candidate_id: draft.candidate_id.clone(),
                memory_card_id: None,
                scope_type: draft.scope_type.clone(),
                scope_ref: draft.scope_ref.clone(),
                candidate_type: draft.candidate_type.clone(),
                rule_filter: rule_filter.clone(),
                llm_filter: llm_filter.clone(),
                confidence: draft.confidence,
                sensitivity: draft.sensitivity.clone(),
                risk_flags: draft.risk_flags.clone(),
                decision: decision.to_string(),
                decision_reason: reason.reason.clone(),
                ttl_policy_ref: ttl_policy_ref(decision, ttl_days),
                expires_at: Some(rfc3339_after_days(ttl_days)),
                actor: policy_actor.to_string(),
            },
        )?;
        decisions.push(policy.summary.clone());
        if reason.suppress {
            update_candidate_status(
                conn,
                &draft.candidate_id,
                "rejected",
                None,
                Some(&rfc3339_after_days(ttl_days)),
                &now_rfc3339(),
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&draft.candidate_id),
                    action: "policy_suppress",
                    from_status: Some("pending"),
                    to_status: Some("rejected"),
                    actor: policy_actor,
                    reason: Some(&reason.reason),
                    metadata: json!({
                        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                        "policy_decision_id": &policy.policy_decision_id,
                        "policy_decision_ref": &policy.policy_decision_ref,
                    }),
                },
            )?;
        } else {
            update_candidate_status(
                conn,
                &draft.candidate_id,
                "pending",
                None,
                Some(&rfc3339_after_days(ttl_days)),
                &now_rfc3339(),
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&draft.candidate_id),
                    action: "policy_pending_manual_review",
                    from_status: Some("pending"),
                    to_status: Some("pending"),
                    actor: policy_actor,
                    reason: Some(&reason.reason),
                    metadata: json!({
                        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                        "policy_decision_id": &policy.policy_decision_id,
                        "policy_decision_ref": &policy.policy_decision_ref,
                    }),
                },
            )?;
        }
    }

    Ok(MemoryPolicyApplication {
        auto_read_enabled,
        public_summary: json!({
            "policy_version": SCOPED_MEMORY_POLICY_VERSION,
            "policy_mode": policy_mode,
            "decision": if auto_read_enabled { "enable_read" } else { reason.public_decision.as_str() },
            "decision_reason": reason.reason,
            "auto_read_enabled": auto_read_enabled,
            "policy_decision_count": decisions.len(),
        }),
        policy_decision_summaries: decisions,
    })
}

#[derive(Debug, Clone)]
struct ScopedMemoryPolicyReason {
    auto_enable: bool,
    suppress: bool,
    public_decision: String,
    reason: String,
}

fn scoped_memory_policy_reason(
    draft: &MemoryCandidateDraft,
    rule_filter: &Value,
    llm_filter: &Value,
    policy_mode: &str,
) -> ScopedMemoryPolicyReason {
    let scope = scope_policy_config(&draft.scope_type);
    let threshold = scope.map_or(1.0, |scope| scope.threshold);
    let disallowed_type =
        !is_auto_candidate_type_allowed_for_scope(&draft.scope_type, &draft.candidate_type);
    let exclusion_flags = llm_filter
        .get("exclusion_flags")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let llm_allows = llm_filter
        .get("is_long_term_memory")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !llm_filter
            .get("is_temporary_instruction")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && !llm_filter
            .get("is_quoted_or_third_party")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && !llm_filter
            .get("has_contradiction")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        && exclusion_flags == 0;
    let rule_suppressed = rule_filter
        .get("suppress")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source_entry_type_allowed = rule_filter
        .get("source_entry_type_allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let context_pack_bound = rule_filter
        .get("context_pack_bound")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if rule_suppressed || !llm_allows || disallowed_type || scope.is_none() {
        return ScopedMemoryPolicyReason {
            auto_enable: false,
            suppress: rule_suppressed || disallowed_type,
            public_decision: if rule_suppressed || disallowed_type {
                "suppress".to_string()
            } else {
                "pending_manual_review".to_string()
            },
            reason: if !source_entry_type_allowed {
                "source_entry_type_not_allowed".to_string()
            } else if !context_pack_bound {
                "context_pack_missing".to_string()
            } else if rule_suppressed {
                "rule_filter_suppressed".to_string()
            } else if disallowed_type {
                "candidate_type_not_allowed_for_scope".to_string()
            } else if scope.is_none() {
                "scope_not_supported".to_string()
            } else {
                "semantic_filter_requires_manual_review".to_string()
            },
        };
    }
    if policy_mode == MEMORY_POLICY_MODE_SHADOW {
        return ScopedMemoryPolicyReason {
            auto_enable: false,
            suppress: false,
            public_decision: "pending_manual_review".to_string(),
            reason: "policy_mode_shadow_only".to_string(),
        };
    }
    if policy_mode == MEMORY_POLICY_MODE_MANUAL {
        return ScopedMemoryPolicyReason {
            auto_enable: false,
            suppress: false,
            public_decision: "pending_manual_review".to_string(),
            reason: "policy_mode_manual_required".to_string(),
        };
    }
    let Some(scope) = scope else {
        unreachable!("scope none handled above");
    };
    if !scope.auto_read {
        return ScopedMemoryPolicyReason {
            auto_enable: false,
            suppress: false,
            public_decision: "pending_manual_review".to_string(),
            reason: "scope_requires_manual_enable".to_string(),
        };
    }
    if draft.confidence < threshold {
        return ScopedMemoryPolicyReason {
            auto_enable: false,
            suppress: false,
            public_decision: "pending_manual_review".to_string(),
            reason: "confidence_below_scope_threshold".to_string(),
        };
    }
    ScopedMemoryPolicyReason {
        auto_enable: true,
        suppress: false,
        public_decision: "enable_read".to_string(),
        reason: "scope_policy_auto_enable_conditions_met".to_string(),
    }
}

fn scoped_memory_rule_filter(draft: &MemoryCandidateDraft) -> Value {
    let temporary_instruction = is_temporary_memory_instruction(&draft.raw_excerpt_redacted);
    let forbidden_candidate_type = is_forbidden_memory_candidate_type(&draft.candidate_type);
    let scope_supported = validate_memory_scope_type(&draft.scope_type).is_ok();
    let source_entry_type_allowed = draft.source_entry_type == "user_message";
    let context_pack_bound = draft.context_pack_id.is_some();
    let mut suppression_reasons = Vec::new();
    if !source_entry_type_allowed {
        suppression_reasons.push(json!("source_entry_type_not_allowed"));
    }
    if !context_pack_bound {
        suppression_reasons.push(json!("context_pack_missing"));
    }
    if temporary_instruction {
        suppression_reasons.push(json!("temporary_instruction"));
    }
    if forbidden_candidate_type {
        suppression_reasons.push(json!("forbidden_candidate_type"));
    }
    if !scope_supported {
        suppression_reasons.push(json!("scope_not_supported"));
    }
    json!({
        "schema_version": "scoped-memory-rule-filter-v1",
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "hard_deny_filter_passed": true,
        "source_entry_type_allowed": source_entry_type_allowed,
        "context_pack_bound": context_pack_bound,
        "scope_supported": scope_supported,
        "temporary_instruction": temporary_instruction,
        "forbidden_candidate_type": forbidden_candidate_type,
        "scope_automation": scope_policy_config(&draft.scope_type).map(|scope| scope.automation),
        "threshold": scope_policy_config(&draft.scope_type).map(|scope| scope.threshold),
        "suppress": !source_entry_type_allowed || !context_pack_bound || temporary_instruction || forbidden_candidate_type || !scope_supported,
        "suppression_reasons": suppression_reasons,
        "input_digest": {
            "summary_sha256": &draft.summary_sha256,
            "raw_excerpt_sha256": &draft.raw_excerpt_sha256,
        },
        "llm_called": false,
    })
}

fn scoped_memory_semantic_filter(draft: &MemoryCandidateDraft, rule_filter: &Value) -> Value {
    let temporary = rule_filter
        .get("temporary_instruction")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let forbidden = rule_filter
        .get("forbidden_candidate_type")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ttl = ttl_days_for_candidate_type(&draft.candidate_type)
        .unwrap_or_else(|| ttl_days_for_decision("pending_manual_review"));
    let mut exclusion_flags = Vec::new();
    if temporary {
        exclusion_flags.push(json!("temporary_instruction"));
    }
    if !rule_filter
        .get("source_entry_type_allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        exclusion_flags.push(json!("source_entry_type_not_allowed"));
    }
    if !rule_filter
        .get("context_pack_bound")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        exclusion_flags.push(json!("context_pack_missing"));
    }
    if !rule_filter
        .get("scope_supported")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        exclusion_flags.push(json!("scope_not_supported"));
    }
    if forbidden {
        exclusion_flags.push(json!("forbidden_candidate_type"));
    }
    json!({
        "schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "semantic_filter": "deterministic_schema_bound_filter",
        "llm_called": false,
        "is_long_term_memory": !temporary && !forbidden,
        "is_temporary_instruction": temporary,
        "is_quoted_or_third_party": looks_like_quoted_or_third_party(&draft.raw_excerpt_redacted),
        "has_contradiction": false,
        "scope_type": &draft.scope_type,
        "candidate_type": &draft.candidate_type,
        "confidence": draft.confidence,
        "sensitivity": "low",
        "risk_flags": &draft.risk_flags,
        "ttl_hint": format!("{ttl}d"),
        "exclusion_flags": exclusion_flags,
        "input_digest": {
            "candidate_summary_sha256": &draft.summary_sha256,
            "redacted_excerpt_sha256": &draft.raw_excerpt_sha256,
        },
    })
}

fn record_memory_policy_decision(
    conn: &Connection,
    draft: MemoryPolicyDecisionDraft,
) -> Result<MemoryPolicyDecisionRecord> {
    validate_memory_scope_type(&draft.scope_type)?;
    validate_memory_candidate_type(&draft.candidate_type)?;
    validate_memory_policy_mode(&memory_policy_mode())?;
    validate_memory_policy_decision(&draft.decision)?;
    let policy_decision_id = format!("memory-policy-decision-{}", uuid::Uuid::now_v7().simple());
    let policy_decision_ref = format!(
        "memory-policy-decision://tonglingyu/{}/{}",
        &hash_text(&draft.scope_ref)[..16],
        &policy_decision_id
    );
    let audit_ref = memory_audit_ref("policy-decision", &policy_decision_id);
    let created_at = now_rfc3339();
    conn.execute(
        "INSERT INTO memory_policy_decisions (
            policy_decision_id, policy_decision_ref, policy_version, policy_mode,
            candidate_id, memory_card_id, scope_type, scope_ref, candidate_type,
            rule_filter_json, llm_filter_json, confidence, sensitivity, risk_flags_json,
            decision, decision_reason, ttl_policy_ref, expires_at, actor, created_at,
            audit_ref, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            &policy_decision_id,
            &policy_decision_ref,
            SCOPED_MEMORY_POLICY_VERSION,
            memory_policy_mode(),
            &draft.candidate_id,
            draft.memory_card_id.as_deref(),
            &draft.scope_type,
            &draft.scope_ref,
            &draft.candidate_type,
            serde_json::to_string(&draft.rule_filter)?,
            serde_json::to_string(&draft.llm_filter)?,
            draft.confidence,
            &draft.sensitivity,
            serde_json::to_string(&draft.risk_flags)?,
            &draft.decision,
            &draft.decision_reason,
            &draft.ttl_policy_ref,
            draft.expires_at.as_deref(),
            &draft.actor,
            &created_at,
            &audit_ref,
            MEMORY_POLICY_DECISION_SCHEMA_VERSION,
        ],
    )?;
    append_memory_transition_audit(
        conn,
        MemoryTransitionAuditInput {
            entity_type: "memory_policy_decision",
            entity_id: Some(&policy_decision_id),
            action: &draft.decision,
            from_status: None,
            to_status: Some(&draft.decision),
            actor: &draft.actor,
            reason: Some(&draft.decision_reason),
            metadata: json!({
                "policy_decision_ref": &policy_decision_ref,
                "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                "policy_mode": memory_policy_mode(),
                "candidate_id": &draft.candidate_id,
                "memory_card_id": draft.memory_card_id,
                "scope_type": &draft.scope_type,
                "scope_ref_sha256": hash_text(&draft.scope_ref),
                "candidate_type": &draft.candidate_type,
                "decision": &draft.decision,
                "ttl_policy_ref": &draft.ttl_policy_ref,
                "expires_at": &draft.expires_at,
                "llm_schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
            }),
        },
    )?;
    Ok(MemoryPolicyDecisionRecord {
        policy_decision_id: policy_decision_id.clone(),
        policy_decision_ref: policy_decision_ref.clone(),
        summary: json!({
            "policy_decision_id": policy_decision_id,
            "policy_decision_ref": policy_decision_ref,
            "policy_version": SCOPED_MEMORY_POLICY_VERSION,
            "policy_mode": memory_policy_mode(),
            "decision": draft.decision,
            "candidate_id": draft.candidate_id,
            "memory_card_id": draft.memory_card_id,
            "confidence": draft.confidence,
            "risk_flags": draft.risk_flags,
            "expires_at": draft.expires_at,
            "audit_ref": audit_ref,
        }),
    })
}

fn approve_memory_candidate(
    conn: &Connection,
    candidate_id: &str,
    from_status: &str,
    actor: &str,
    reason: &str,
    metadata_extra: Option<Value>,
    expires_at: Option<&str>,
) -> Result<()> {
    let now = now_rfc3339();
    update_candidate_status(conn, candidate_id, "approved", None, expires_at, &now)?;
    let mut metadata = json!({
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
    });
    merge_json_object(&mut metadata, metadata_extra);
    append_memory_transition_audit(
        conn,
        MemoryTransitionAuditInput {
            entity_type: "memory_candidate",
            entity_id: Some(candidate_id),
            action: "approve",
            from_status: Some(from_status),
            to_status: Some("approved"),
            actor,
            reason: Some(reason),
            metadata,
        },
    )?;
    Ok(())
}

fn promote_memory_candidate(
    conn: &Connection,
    candidate: &MemoryCandidateCore,
    actor: &str,
    reason: &str,
) -> Result<()> {
    promote_memory_candidate_with_options(conn, candidate, actor, reason, None, false, None)?;
    Ok(())
}

fn ensure_candidate_source_read_eligible(candidate: &MemoryCandidateCore) -> Result<()> {
    if candidate.source_entry_type != "user_message" {
        return Err(anyhow!(
            "memory candidate source_entry_type is not eligible for read path"
        ));
    }
    if candidate.context_pack_id.is_none() {
        return Err(anyhow!(
            "memory candidate context_pack_id is required for read path"
        ));
    }
    Ok(())
}

fn promote_memory_candidate_with_options(
    conn: &Connection,
    candidate: &MemoryCandidateCore,
    actor: &str,
    reason: &str,
    expires_at: Option<&str>,
    read_enabled: bool,
    policy_decision: Option<&MemoryPolicyDecisionRecord>,
) -> Result<MemoryCardCore> {
    validate_memory_scope_type(&candidate.scope_type)?;
    validate_memory_promotable_candidate_type(&candidate.scope_type, &candidate.candidate_type)?;
    ensure_candidate_source_read_eligible(candidate)?;
    if let Some(existing) = conn
        .query_row(
            "SELECT memory_card_id FROM memory_cards WHERE source_candidate_id = ?1",
            params![&candidate.candidate_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        let existing_card = load_memory_card_core(conn, &existing)?
            .ok_or_else(|| anyhow!("existing memory card not found"))?;
        if existing_card.status == "active" {
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&candidate.candidate_id),
                    action: "promote_idempotent",
                    from_status: Some("approved"),
                    to_status: Some("approved"),
                    actor,
                    reason: Some(reason),
                    metadata: json!({
                    "candidate_ref": &candidate.candidate_ref,
                    "existing_memory_card_id": existing,
                    "read_enabled": existing_card.read_enabled,
                    }),
                },
            )?;
            return Ok(existing_card);
        }
        return Err(anyhow!(
            "source candidate already has non-active memory card"
        ));
    }
    let memory_card_id = format!("memory-card-{}", uuid::Uuid::now_v7().simple());
    let scope_digest = hash_text(&candidate.scope_ref);
    let memory_card_ref = format!(
        "memory-card://tonglingyu/{}/{}",
        &scope_digest[..16],
        &memory_card_id
    );
    let now = now_rfc3339();
    let audit_ref = memory_audit_ref("card-promote", &memory_card_id);
    let acl = json!({
        "schema_version": "tonglingyu-memory-acl-v1",
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "scope_type": &candidate.scope_type,
        "scope_ref_sha256": hash_text(&candidate.scope_ref),
        "read_enabled": read_enabled,
        "allowed_consumers": allowed_memory_consumers(&candidate.scope_type, &candidate.candidate_type),
        "allowed_readers": allowed_memory_consumers(&candidate.scope_type, &candidate.candidate_type),
        "evidence_package_allowed": false,
        "reviewer_content_allowed": false,
    });
    conn.execute(
        "INSERT INTO memory_cards (
            memory_card_id, memory_card_ref, source_candidate_id, status,
            scope_type, scope_ref, summary, summary_sha256, acl_json, sensitivity,
            promotion_policy_version, promoted_by, promoted_at, revoked_by, revoked_at,
            expires_at, read_enabled, audit_ref, schema_version
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                  NULL, NULL, ?13, ?14, ?15, ?16)",
        params![
            &memory_card_id,
            &memory_card_ref,
            &candidate.candidate_id,
            &candidate.scope_type,
            &candidate.scope_ref,
            &candidate.summary,
            &candidate.summary_sha256,
            serde_json::to_string(&acl)?,
            &candidate.sensitivity,
            SCOPED_MEMORY_POLICY_VERSION,
            actor,
            &now,
            expires_at,
            if read_enabled { 1_i64 } else { 0_i64 },
            &audit_ref,
            MEMORY_CARD_SCHEMA_VERSION,
        ],
    )?;
    append_memory_transition_audit(
        conn,
        MemoryTransitionAuditInput {
            entity_type: "memory_card",
            entity_id: Some(&memory_card_id),
            action: "promote",
            from_status: None,
            to_status: Some("active"),
            actor,
            reason: Some(reason),
            metadata: json!({
            "memory_card_ref": &memory_card_ref,
            "source_candidate_id": &candidate.candidate_id,
            "candidate_ref": &candidate.candidate_ref,
            "read_enabled": read_enabled,
            "policy_version": SCOPED_MEMORY_POLICY_VERSION,
            "policy_decision_id": policy_decision.map(|decision| decision.policy_decision_id.as_str()),
            "policy_decision_ref": policy_decision.map(|decision| decision.policy_decision_ref.as_str()),
            }),
        },
    )?;
    append_memory_transition_audit(
        conn,
        MemoryTransitionAuditInput {
            entity_type: "memory_candidate",
            entity_id: Some(&candidate.candidate_id),
            action: "promote",
            from_status: Some("approved"),
            to_status: Some("approved"),
            actor,
            reason: Some(reason),
            metadata: json!({
            "candidate_ref": &candidate.candidate_ref,
            "memory_card_id": &memory_card_id,
            "memory_card_ref": &memory_card_ref,
            "read_enabled": read_enabled,
            "policy_version": SCOPED_MEMORY_POLICY_VERSION,
            "policy_decision_id": policy_decision.map(|decision| decision.policy_decision_id.as_str()),
            "policy_decision_ref": policy_decision.map(|decision| decision.policy_decision_ref.as_str()),
            }),
        },
    )?;
    load_memory_card_core(conn, &memory_card_id)?
        .ok_or_else(|| anyhow!("promoted memory card not found"))
}

fn set_memory_card_read_enabled(
    conn: &Connection,
    card: &MemoryCardCore,
    read_enabled: bool,
    actor: &str,
    reason: &str,
    policy_decision: Option<&MemoryPolicyDecisionRecord>,
) -> Result<()> {
    require_status(&card.status, &["active"])?;
    if read_enabled {
        ensure_memory_card_enable_read_policy(conn, card, policy_decision)?;
    }
    let mut acl = card.acl.clone();
    if let Some(object) = acl.as_object_mut() {
        object.insert("read_enabled".to_string(), json!(read_enabled));
        object.insert(
            "policy_version".to_string(),
            json!(SCOPED_MEMORY_POLICY_VERSION),
        );
    }
    conn.execute(
        "UPDATE memory_cards
         SET read_enabled = ?1, acl_json = ?2
         WHERE memory_card_id = ?3",
        params![
            if read_enabled { 1_i64 } else { 0_i64 },
            serde_json::to_string(&acl)?,
            &card.memory_card_id,
        ],
    )?;
    append_memory_transition_audit(
        conn,
        MemoryTransitionAuditInput {
            entity_type: "memory_card",
            entity_id: Some(&card.memory_card_id),
            action: if read_enabled {
                "enable_read"
            } else {
                "disable_read"
            },
            from_status: Some(&card.status),
            to_status: Some(&card.status),
            actor,
            reason: Some(reason),
            metadata: json!({
                "memory_card_ref": &card.memory_card_ref,
                "source_candidate_id": &card.source_candidate_id,
                "read_enabled": read_enabled,
                "policy_version": SCOPED_MEMORY_POLICY_VERSION,
                "policy_decision_id": policy_decision.map(|decision| decision.policy_decision_id.as_str()),
                "policy_decision_ref": policy_decision.map(|decision| decision.policy_decision_ref.as_str()),
            }),
        },
    )?;
    Ok(())
}

fn ensure_memory_card_enable_read_policy(
    conn: &Connection,
    card: &MemoryCardCore,
    policy_decision: Option<&MemoryPolicyDecisionRecord>,
) -> Result<()> {
    if let Some(policy_decision) = policy_decision {
        if policy_decision.policy_decision_id.trim().is_empty() {
            return Err(anyhow!("policy decision missing for read enablement"));
        }
        return Ok(());
    }
    let existing = conn.query_row(
        "SELECT COUNT(*) FROM memory_policy_decisions
             WHERE memory_card_id = ?1
               AND decision = 'enable_read'
               AND policy_version = ?2",
        params![&card.memory_card_id, SCOPED_MEMORY_POLICY_VERSION],
        |row| row.get::<_, i64>(0),
    )?;
    if existing <= 0 {
        return Err(anyhow!("policy decision missing for read enablement"));
    }
    Ok(())
}

fn manual_read_enable_policy_decision(
    conn: &Connection,
    card: &MemoryCardCore,
    actor: &str,
    decision: &str,
    reason: &str,
) -> Result<MemoryPolicyDecisionRecord> {
    let candidate = load_memory_candidate_core(conn, &card.source_candidate_id)?
        .ok_or_else(|| anyhow!("source memory candidate not found"))?;
    ensure_candidate_source_read_eligible(&candidate)?;
    let rule_filter = json!({
        "schema_version": "scoped-memory-rule-filter-v1",
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "manual_review": true,
        "source_entry_type_allowed": candidate.source_entry_type == "user_message",
        "context_pack_bound": candidate.context_pack_id.is_some(),
        "scope_supported": validate_memory_scope_type(&card.scope_type).is_ok(),
        "suppress": false,
    });
    let ttl = ttl_days_for_candidate_type(&candidate.candidate_type)
        .unwrap_or_else(|| ttl_days_for_decision("pending_manual_review"));
    let llm_filter = json!({
        "schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "semantic_filter": "manual_review_record",
        "llm_called": false,
        "is_long_term_memory": true,
        "is_temporary_instruction": false,
        "is_quoted_or_third_party": false,
        "has_contradiction": false,
        "scope_type": &card.scope_type,
        "candidate_type": &candidate.candidate_type,
        "confidence": candidate.confidence,
        "sensitivity": "low",
        "risk_flags": &candidate.risk_flags,
        "ttl_hint": format!("{ttl}d"),
        "exclusion_flags": [],
    });
    record_memory_policy_decision(
        conn,
        MemoryPolicyDecisionDraft {
            candidate_id: candidate.candidate_id,
            memory_card_id: Some(card.memory_card_id.clone()),
            scope_type: card.scope_type.clone(),
            scope_ref: card.scope_ref.clone(),
            candidate_type: candidate.candidate_type,
            rule_filter,
            llm_filter,
            confidence: candidate.confidence,
            sensitivity: card.sensitivity.clone(),
            risk_flags: candidate.risk_flags,
            decision: decision.to_string(),
            decision_reason: reason.to_string(),
            ttl_policy_ref: ttl_policy_ref(decision, ttl),
            expires_at: card
                .expires_at
                .clone()
                .or_else(|| Some(rfc3339_after_days(ttl))),
            actor: actor.to_string(),
        },
    )
}

fn update_candidate_status(
    conn: &Connection,
    candidate_id: &str,
    status: &str,
    merged_into_candidate_id: Option<&str>,
    expires_at: Option<&str>,
    updated_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE memory_candidates
         SET status = ?1, merged_into_candidate_id = COALESCE(?2, merged_into_candidate_id),
             expires_at = COALESCE(?3, expires_at), updated_at = ?4
         WHERE candidate_id = ?5",
        params![
            status,
            merged_into_candidate_id,
            expires_at,
            updated_at,
            candidate_id,
        ],
    )?;
    Ok(())
}

fn load_memory_candidate_core(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<MemoryCandidateCore>> {
    conn.query_row(
        "SELECT candidate_id, candidate_ref, status, source_entry_type, context_pack_id,
                scope_type, scope_ref,
                candidate_type, summary, summary_sha256, sensitivity, risk_flags_json,
                confidence
         FROM memory_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        |row| {
            Ok(MemoryCandidateCore {
                candidate_id: row.get(0)?,
                candidate_ref: row.get(1)?,
                status: row.get(2)?,
                source_entry_type: row.get(3)?,
                context_pack_id: row.get(4)?,
                scope_type: row.get(5)?,
                scope_ref: row.get(6)?,
                candidate_type: row.get(7)?,
                summary: row.get(8)?,
                summary_sha256: row.get(9)?,
                sensitivity: row.get(10)?,
                risk_flags: parse_json_column(row.get::<_, String>(11)?),
                confidence: row.get(12)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn load_memory_card_core(
    conn: &Connection,
    memory_card_id: &str,
) -> Result<Option<MemoryCardCore>> {
    conn.query_row(
        "SELECT memory_card_id, memory_card_ref, source_candidate_id, status,
                scope_type, scope_ref, sensitivity, acl_json,
                read_enabled, expires_at
         FROM memory_cards WHERE memory_card_id = ?1",
        params![memory_card_id],
        |row| {
            Ok(MemoryCardCore {
                memory_card_id: row.get(0)?,
                memory_card_ref: row.get(1)?,
                source_candidate_id: row.get(2)?,
                status: row.get(3)?,
                scope_type: row.get(4)?,
                scope_ref: row.get(5)?,
                sensitivity: row.get(6)?,
                acl: parse_json_column(row.get::<_, String>(7)?),
                read_enabled: row.get::<_, i64>(8)? != 0,
                expires_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn memory_candidate_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "candidate_id": row.get::<_, String>(0)?,
        "candidate_ref": row.get::<_, String>(1)?,
        "status": row.get::<_, String>(2)?,
        "journal_id": row.get::<_, String>(3)?,
        "trace_id": row.get::<_, String>(4)?,
        "user_session_id": row.get::<_, String>(5)?,
        "interaction_context_id": row.get::<_, String>(6)?,
        "context_pack_id": row.get::<_, Option<String>>(7)?,
        "source_entry_type": row.get::<_, String>(8)?,
        "scope_type": row.get::<_, String>(9)?,
        "scope_ref": row.get::<_, String>(10)?,
        "candidate_type": row.get::<_, String>(11)?,
        "summary": row.get::<_, String>(12)?,
        "summary_sha256": row.get::<_, String>(13)?,
        "raw_excerpt_redacted": row.get::<_, String>(14)?,
        "raw_excerpt_sha256": row.get::<_, String>(15)?,
        "sensitivity": row.get::<_, String>(16)?,
        "risk_flags": parse_json_column(row.get::<_, String>(17)?),
        "llm_extraction": parse_json_column(row.get::<_, String>(18)?),
        "confidence": row.get::<_, f64>(19)?,
        "created_by": row.get::<_, String>(20)?,
        "created_at": row.get::<_, String>(21)?,
        "updated_at": row.get::<_, String>(22)?,
        "expires_at": row.get::<_, Option<String>>(23)?,
        "merged_into_candidate_id": row.get::<_, Option<String>>(24)?,
        "audit_ref": row.get::<_, String>(25)?,
        "schema_version": row.get::<_, String>(26)?,
    }))
}

fn memory_card_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "memory_card_id": row.get::<_, String>(0)?,
        "memory_card_ref": row.get::<_, String>(1)?,
        "source_candidate_id": row.get::<_, String>(2)?,
        "status": row.get::<_, String>(3)?,
        "scope_type": row.get::<_, String>(4)?,
        "scope_ref": row.get::<_, String>(5)?,
        "summary": row.get::<_, String>(6)?,
        "summary_sha256": row.get::<_, String>(7)?,
        "acl": parse_json_column(row.get::<_, String>(8)?),
        "sensitivity": row.get::<_, String>(9)?,
        "promotion_policy_version": row.get::<_, String>(10)?,
        "promoted_by": row.get::<_, String>(11)?,
        "promoted_at": row.get::<_, String>(12)?,
        "revoked_by": row.get::<_, Option<String>>(13)?,
        "revoked_at": row.get::<_, Option<String>>(14)?,
        "expires_at": row.get::<_, Option<String>>(15)?,
        "read_enabled": row.get::<_, i64>(16)? != 0,
        "audit_ref": row.get::<_, String>(17)?,
        "schema_version": row.get::<_, String>(18)?,
    }))
}

fn memory_candidate_summary_json(draft: &MemoryCandidateDraft) -> Value {
    json!({
        "candidate_id": &draft.candidate_id,
        "candidate_ref": &draft.candidate_ref,
        "status": "pending",
        "journal_id": &draft.journal_id,
        "trace_id": &draft.trace_id,
        "scope_type": &draft.scope_type,
        "scope_ref": &draft.scope_ref,
        "candidate_type": &draft.candidate_type,
        "summary": &draft.summary,
        "summary_sha256": &draft.summary_sha256,
        "risk_flags": &draft.risk_flags,
        "confidence": draft.confidence,
        "llm_extraction": &draft.llm_extraction,
    })
}

fn record_memory_collector_journal_status(
    conn: &Connection,
    journal_id: &str,
    run_id: &str,
    status: &str,
    reason: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO memory_collector_journal_status (
            journal_id, run_id, status, reason, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(journal_id) DO UPDATE SET
            run_id = excluded.run_id,
            status = excluded.status,
            reason = excluded.reason,
            updated_at = excluded.updated_at",
        params![journal_id, run_id, status, reason, now_rfc3339()],
    )?;
    Ok(())
}

fn append_memory_transition_audit(
    conn: &Connection,
    input: MemoryTransitionAuditInput<'_>,
) -> Result<String> {
    let audit_id = format!("memory-audit-{}", uuid::Uuid::now_v7().simple());
    let audit_ref = memory_audit_ref("transition", &audit_id);
    conn.execute(
        "INSERT INTO memory_transition_audit (
            audit_id, audit_ref, entity_type, entity_id, action, from_status, to_status,
            actor, reason_sha256, metadata_json, created_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            &audit_id,
            &audit_ref,
            input.entity_type,
            input.entity_id,
            input.action,
            input.from_status,
            input.to_status,
            non_empty_or(input.actor, "system"),
            input.reason.map(hash_text),
            serde_json::to_string(&input.metadata)?,
            now_rfc3339(),
            MEMORY_TRANSITION_AUDIT_SCHEMA_VERSION,
        ],
    )?;
    Ok(audit_ref)
}

fn acquire_memory_collector_lease(conn: &Connection, owner: &str) -> Result<bool> {
    let now = now_rfc3339();
    let existing = conn
        .query_row(
            "SELECT owner, leased_until FROM memory_collector_leases WHERE lease_name = ?1",
            params![MEMORY_COLLECTOR_LEASE_NAME],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((existing_owner, leased_until)) = existing
        && leased_until > now
        && existing_owner != owner
    {
        return Ok(false);
    }
    let leased_until = OffsetDateTime::now_utc()
        .checked_add(TimeDuration::seconds(MEMORY_COLLECTOR_LEASE_TTL_SECS))
        .unwrap_or_else(OffsetDateTime::now_utc)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now.clone());
    conn.execute(
        "INSERT INTO memory_collector_leases (lease_name, owner, leased_until, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(lease_name) DO UPDATE SET
            owner = excluded.owner,
            leased_until = excluded.leased_until,
            updated_at = excluded.updated_at",
        params![MEMORY_COLLECTOR_LEASE_NAME, owner, leased_until, now],
    )?;
    Ok(true)
}

fn release_memory_collector_lease(conn: &Connection, owner: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM memory_collector_leases WHERE lease_name = ?1 AND owner = ?2",
        params![MEMORY_COLLECTOR_LEASE_NAME, owner],
    )?;
    Ok(())
}

pub(crate) fn validate_llm_memory_extraction_output(output: &Value) -> Result<Value> {
    let object = output
        .as_object()
        .ok_or_else(|| anyhow!("LLM memory extraction output must be a JSON object"))?;
    let mut forbidden_paths = Vec::new();
    collect_forbidden_llm_memory_fields("$", output, &mut forbidden_paths);
    if !forbidden_paths.is_empty() {
        return Err(anyhow!(
            "LLM memory extraction output contains forbidden fields: {}",
            forbidden_paths.join(",")
        ));
    }
    let schema_version = object
        .get("schema_version")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("LLM memory extraction missing schema_version"))?;
    if schema_version != SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION {
        return Err(anyhow!("unsupported LLM memory filter schema_version"));
    }
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "schema_version"
                | "is_long_term_memory"
                | "is_temporary_instruction"
                | "is_quoted_or_third_party"
                | "has_contradiction"
                | "scope_type"
                | "candidate_type"
                | "confidence"
                | "sensitivity"
                | "risk_flags"
                | "ttl_hint"
                | "exclusion_flags"
        ) {
            return Err(anyhow!("unsupported LLM memory extraction field"));
        }
    }
    for key in [
        "is_long_term_memory",
        "is_temporary_instruction",
        "is_quoted_or_third_party",
        "has_contradiction",
    ] {
        if object.get(key).and_then(Value::as_bool).is_none() {
            return Err(anyhow!("LLM memory extraction boolean field invalid"));
        }
    }
    let scope_type = object
        .get("scope_type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("LLM memory extraction missing scope_type"))?;
    validate_memory_scope_type(scope_type)?;
    let candidate_type = object
        .get("candidate_type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("LLM memory extraction missing candidate_type"))?;
    validate_memory_candidate_type(candidate_type)?;
    let confidence = object
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("LLM memory extraction missing confidence"))?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err(anyhow!("LLM memory extraction confidence out of range"));
    }
    let sensitivity = object
        .get("sensitivity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("LLM memory extraction missing sensitivity"))?;
    let ttl_hint = object
        .get("ttl_hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("LLM memory extraction missing ttl_hint"))?;
    let risk_flags = object
        .get("risk_flags")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("LLM memory extraction risk_flags must be an array"))?;
    let exclusion_flags = object
        .get("exclusion_flags")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("LLM memory extraction exclusion_flags must be an array"))?;
    let is_long_term = object
        .get("is_long_term_memory")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let temporary = object
        .get("is_temporary_instruction")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let quoted = object
        .get("is_quoted_or_third_party")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let contradiction = object
        .get("has_contradiction")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let status = if is_long_term
        && !temporary
        && !quoted
        && !contradiction
        && exclusion_flags.is_empty()
        && confidence >= 0.45
    {
        "pending"
    } else {
        "suppressed"
    };
    Ok(json!({
        "schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "is_long_term_memory": is_long_term,
        "is_temporary_instruction": temporary,
        "is_quoted_or_third_party": quoted,
        "has_contradiction": contradiction,
        "scope_type": scope_type,
        "candidate_type": candidate_type,
        "confidence": confidence,
        "sensitivity": sensitivity,
        "risk_flags": Value::Array(risk_flags.clone()),
        "ttl_hint": ttl_hint,
        "exclusion_flags": Value::Array(exclusion_flags.clone()),
        "status": status,
        "confidence_rule": confidence_rule(confidence),
        "promotion_allowed": false,
        "acl_allowed": false,
        "read_enabled_allowed": false,
    }))
}

fn collect_forbidden_llm_memory_fields(prefix: &str, value: &Value, found: &mut Vec<String>) {
    const FORBIDDEN: &[&str] = &[
        "approve",
        "approved",
        "promote",
        "promotion",
        "acl",
        "acl_json",
        "read_enabled",
        "reviewer",
        "review",
        "evidence",
        "evidence_package",
        "evidence_package_id",
        "status",
        "memory_card",
        "memory_card_id",
        "retention",
        "tool_policy",
        "reviewer_decision",
        "task_status",
        "source_fact",
        "tool_permission",
        "system_prompt",
    ];
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let path = format!("{prefix}.{key}");
                if FORBIDDEN.contains(&key.as_str()) {
                    found.push(path);
                } else {
                    collect_forbidden_llm_memory_fields(&path, child, found);
                }
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_forbidden_llm_memory_fields(&format!("{prefix}[{index}]"), child, found);
            }
        }
        _ => {}
    }
}

fn durable_memory_signal(text: &str) -> Option<f64> {
    let strong_markers = [
        "请记住",
        "记住",
        "以后",
        "下次",
        "回答时",
        "以后回答",
        "请用",
        "请叫我",
        "称呼我",
        "我希望",
        "我偏好",
        "我的偏好",
        "我习惯",
    ];
    if strong_markers.iter().any(|marker| text.contains(marker)) {
        return Some(0.86);
    }
    let weak_markers = ["我喜欢", "我不喜欢", "我更喜欢", "优先用", "不要用"];
    if weak_markers.iter().any(|marker| text.contains(marker)) {
        return Some(0.62);
    }
    None
}

fn classify_memory_candidate_type(text: &str) -> String {
    if ["请叫我", "称呼我", "叫我"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "stable_user_background".to_string()
    } else if ["简体", "繁体", "中文", "英文"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "language_preference".to_string()
    } else if ["短句", "简洁", "详细", "太长", "更短", "更长"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "verbosity_preference".to_string()
    } else if ["引用原文", "检索", "证据", "优先查", "搜索"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "retrieval_preference".to_string()
    } else if ["流程", "工作流", "先", "再", "每次"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "workflow_preference".to_string()
    } else if ["研究", "课题", "专题"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "research_interest".to_string()
    } else {
        "answer_style_preference".to_string()
    }
}

fn user_private_scope_ref(external_user_ref: &str) -> String {
    format!("user_private:sha256:{}", hash_text(external_user_ref))
}

fn memory_candidate_summary(candidate_type: &str, text: &str) -> String {
    let prefix = match candidate_type {
        "stable_user_background" => "用户长期背景",
        "language_preference" => "用户语言偏好",
        "verbosity_preference" => "用户详略偏好",
        "retrieval_preference" => "用户检索偏好",
        "workflow_preference" => "用户工作流偏好",
        "research_interest" => "用户研究兴趣",
        "research_topic_context" => "用户研究主题上下文",
        "source_collection_usage_preference" => "用户来源集合使用偏好",
        _ => "用户回答风格偏好",
    };
    bounded_summary(
        &format!("{prefix}: {}", text.trim()),
        MEMORY_SUMMARY_MAX_CHARS,
    )
}

fn is_temporary_memory_instruction(text: &str) -> bool {
    ["这次", "本轮", "临时", "暂时", "先不用记", "不要长期记"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn looks_like_quoted_or_third_party(text: &str) -> bool {
    ["他说", "她说", "别人说", "引号里", "原文说", "书里说"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn merge_json_object(target: &mut Value, extra: Option<Value>) {
    let Some(Value::Object(extra)) = extra else {
        return;
    };
    if let Value::Object(target) = target {
        target.extend(extra);
    }
}

fn redact_sensitive_text(value: &str) -> (String, bool) {
    let mut changed = false;
    let mut parts = Vec::new();
    for token in value.split_whitespace() {
        let lowered = token.to_ascii_lowercase();
        let digit_count = token.chars().filter(|ch| ch.is_ascii_digit()).count();
        let redacted = if token.contains('@') && token.contains('.') {
            changed = true;
            "[redacted_email]"
        } else if lowered.contains("sk-")
            || lowered.contains("token=")
            || lowered.contains("api_key")
            || lowered.contains("api-key")
            || lowered.contains("password=")
        {
            changed = true;
            "[redacted_secret]"
        } else if digit_count >= 8 {
            changed = true;
            "[redacted_number]"
        } else {
            token
        };
        parts.push(redacted.to_string());
    }
    let output = if parts.is_empty() {
        value.trim().to_string()
    } else {
        parts.join(" ")
    };
    (output, changed)
}

fn llm_boundary_contract_json() -> Value {
    json!({
        "allowed": true,
        "used": false,
        "schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
        "position": "after_hard_deny_and_redaction_only",
        "input_contract": [
            "redacted_journal_summary",
            "scope_hint",
            "candidate_summary",
            "json_schema"
        ],
        "allowed_decisions": ["semantic_filter", "classification", "ttl_hint", "risk_flags"],
        "forbidden_decisions": [
            "approve",
            "promote",
            "read_enabled",
            "acl",
            "retention",
            "reviewer_verdict",
            "evidence_package_content",
            "context_pack_read"
        ],
    })
}

fn confidence_rule(confidence: f64) -> &'static str {
    if confidence >= 0.75 {
        "pending"
    } else if confidence >= 0.45 {
        "pending_requires_manual_review"
    } else {
        "suppressed"
    }
}

fn append_risk_flag(mut risk_flags: Value, flag: &str) -> Value {
    match risk_flags.as_array_mut() {
        Some(items) => {
            if !items.iter().any(|item| item == flag) {
                items.push(json!(flag));
            }
            risk_flags
        }
        None => json!([flag]),
    }
}

fn json_object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn validate_memory_collector_trigger(trigger: &str) -> Result<()> {
    if matches!(
        trigger,
        "background_worker" | "scheduled_job" | "admin_manual"
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory collector trigger"))
    }
}

fn validate_memory_candidate_action(action: &str) -> Result<()> {
    if matches!(
        action,
        "approve" | "promote" | "reject" | "reclassify" | "expire" | "merge"
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory candidate action"))
    }
}

fn validate_memory_card_action(action: &str) -> Result<()> {
    if matches!(action, "enable_read" | "disable_read" | "revoke" | "expire") {
        Ok(())
    } else {
        Err(anyhow!("invalid memory card action"))
    }
}

fn validate_memory_candidate_type(candidate_type: &str) -> Result<()> {
    if allowed_memory_candidate_types().contains(&candidate_type)
        || forbidden_memory_candidate_types().contains(&candidate_type)
    {
        Ok(())
    } else {
        Err(anyhow!("invalid memory candidate type"))
    }
}

fn validate_memory_promotable_candidate_type(scope_type: &str, candidate_type: &str) -> Result<()> {
    validate_memory_candidate_type(candidate_type)?;
    if is_auto_candidate_type_allowed_for_scope(scope_type, candidate_type) {
        Ok(())
    } else {
        Err(anyhow!("memory candidate type is not promotable for scope"))
    }
}

fn validate_memory_scope_type(scope_type: &str) -> Result<()> {
    if allowed_memory_scope_types().contains(&scope_type) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory scope type"))
    }
}

fn validate_optional_filter(
    value: Option<&str>,
    name: &str,
    allowed_values: &[&str],
) -> Result<()> {
    if let Some(value) = value
        && !allowed_values.contains(&value)
    {
        return Err(anyhow!("invalid {name} filter"));
    }
    Ok(())
}

fn allowed_memory_candidate_statuses() -> &'static [&'static str] {
    &["pending", "approved", "rejected", "expired", "merged"]
}

fn allowed_memory_card_statuses() -> &'static [&'static str] {
    &["active", "revoked", "expired"]
}

fn allowed_memory_scope_types() -> &'static [&'static str] {
    &[
        "user_private",
        "profile_common",
        "knowledge_space",
        "research_topic",
        "source_collection",
    ]
}

fn allowed_memory_candidate_types() -> &'static [&'static str] {
    &[
        "answer_style_preference",
        "verbosity_preference",
        "language_preference",
        "workflow_preference",
        "retrieval_preference",
        "stable_user_background",
        "research_interest",
        "research_topic_context",
        "source_collection_usage_preference",
    ]
}

fn forbidden_memory_candidate_types() -> &'static [&'static str] {
    &[
        "source_fact",
        "literary_claim",
        "reviewer_decision",
        "task_status",
        "action_result",
        "credential",
        "legal_or_identity_assertion",
        "permission_or_acl_request",
        "temporary_instruction",
        "system_or_prompt_instruction",
    ]
}

fn is_forbidden_memory_candidate_type(candidate_type: &str) -> bool {
    forbidden_memory_candidate_types().contains(&candidate_type)
}

fn validate_memory_policy_decision(decision: &str) -> Result<()> {
    if matches!(
        decision,
        "suppress"
            | "pending_manual_review"
            | "auto_approve"
            | "auto_promote"
            | "enable_read"
            | "disable_read"
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory policy decision"))
    }
}

fn validate_memory_policy_mode(mode: &str) -> Result<()> {
    if matches!(
        mode,
        MEMORY_POLICY_MODE_AUTO | MEMORY_POLICY_MODE_MANUAL | MEMORY_POLICY_MODE_SHADOW
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory policy mode"))
    }
}

fn memory_policy_mode() -> String {
    let mode =
        env::var(MEMORY_POLICY_MODE_ENV).unwrap_or_else(|_| MEMORY_POLICY_MODE_AUTO.to_string());
    let mode = mode.trim();
    if validate_memory_policy_mode(mode).is_ok() {
        mode.to_string()
    } else {
        MEMORY_POLICY_MODE_MANUAL.to_string()
    }
}

#[cfg(test)]
fn conversation_state_summary_mode() -> LlmMode {
    env::var(CONVERSATION_STATE_SUMMARY_MODE_ENV)
        .ok()
        .and_then(|mode| LlmMode::parse(&mode).ok())
        .unwrap_or(LlmMode::Disabled)
}

fn question_normalizer_agent_mode() -> LlmMode {
    LlmMode::Enforced
}

fn conversation_state_agent_mode() -> LlmMode {
    LlmMode::Enforced
}

#[derive(Debug, Clone, Copy)]
struct ScopePolicyConfig {
    automation: &'static str,
    threshold: f64,
    auto_read: bool,
}

fn scope_policy_config(scope_type: &str) -> Option<ScopePolicyConfig> {
    match scope_type {
        "user_private" => Some(ScopePolicyConfig {
            automation: "auto_enable",
            threshold: 0.85,
            auto_read: true,
        }),
        "profile_common" => Some(ScopePolicyConfig {
            automation: "auto_enable_limited",
            threshold: 0.92,
            auto_read: true,
        }),
        "knowledge_space" => Some(ScopePolicyConfig {
            automation: "auto_enable_limited",
            threshold: 0.94,
            auto_read: true,
        }),
        "research_topic" => Some(ScopePolicyConfig {
            automation: "auto_enable_limited",
            threshold: 0.94,
            auto_read: true,
        }),
        "source_collection" => Some(ScopePolicyConfig {
            automation: "manual_first_with_shadow",
            threshold: 1.0,
            auto_read: false,
        }),
        _ => None,
    }
}

fn is_auto_candidate_type_allowed_for_scope(scope_type: &str, candidate_type: &str) -> bool {
    match scope_type {
        "user_private" => matches!(
            candidate_type,
            "answer_style_preference"
                | "verbosity_preference"
                | "language_preference"
                | "workflow_preference"
                | "retrieval_preference"
                | "stable_user_background"
                | "research_interest"
        ),
        "profile_common" => matches!(
            candidate_type,
            "answer_style_preference"
                | "verbosity_preference"
                | "language_preference"
                | "workflow_preference"
                | "retrieval_preference"
        ),
        "knowledge_space" => matches!(
            candidate_type,
            "workflow_preference" | "retrieval_preference" | "research_interest"
        ),
        "research_topic" => matches!(
            candidate_type,
            "research_interest"
                | "research_topic_context"
                | "workflow_preference"
                | "retrieval_preference"
        ),
        "source_collection" => candidate_type == "source_collection_usage_preference",
        _ => false,
    }
}

fn ttl_days_for_candidate_type(candidate_type: &str) -> Option<i64> {
    match candidate_type {
        "answer_style_preference" => Some(90),
        "verbosity_preference" => Some(90),
        "language_preference" => Some(180),
        "workflow_preference" => Some(180),
        "retrieval_preference" => Some(180),
        "stable_user_background" => Some(365),
        "research_interest" => Some(180),
        "research_topic_context" => Some(90),
        "source_collection_usage_preference" => Some(90),
        _ => None,
    }
}

fn ttl_days_for_decision(decision: &str) -> i64 {
    if decision == "pending_manual_review" {
        30
    } else {
        90
    }
}

fn ttl_policy_ref(kind: &str, ttl_days: i64) -> String {
    format!("{SCOPED_MEMORY_POLICY_VERSION}:ttl:{kind}:{ttl_days}d")
}

fn rfc3339_after_days(days: i64) -> String {
    OffsetDateTime::now_utc()
        .checked_add(TimeDuration::days(days))
        .unwrap_or_else(OffsetDateTime::now_utc)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_rfc3339())
}

fn allowed_memory_consumers(scope_type: &str, candidate_type: &str) -> Vec<String> {
    match (scope_type, candidate_type) {
        ("user_private", _) => vec!["honglou-main".to_string()],
        (_, "retrieval_preference") | (_, "source_collection_usage_preference") => vec![
            "honglou-main".to_string(),
            "honglou-text".to_string(),
            "honglou-commentary".to_string(),
        ],
        (_, _) => vec!["honglou-main".to_string()],
    }
}

fn memory_read_budget_json() -> Value {
    json!({
        "schema_version": "scoped-memory-read-budget-v1",
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "context_pack_max": MEMORY_READ_BUDGET_TOTAL,
        "user_private_max": MEMORY_READ_BUDGET_USER_PRIVATE,
        "shared_scope_total_max": MEMORY_READ_BUDGET_SHARED,
        "tool_profile_non_private_max": MEMORY_READ_BUDGET_TOOL_PROFILE,
    })
}

fn scoped_memory_policy_public_contract() -> Value {
    json!({
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "policy_mode": memory_policy_mode(),
        "policy_mode_env": MEMORY_POLICY_MODE_ENV,
        "llm_schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
        "policy_actor": MEMORY_POLICY_ACTOR,
        "read_budget": memory_read_budget_json(),
        "scope_automation": {
            "user_private": {"automation": "auto_enable", "threshold": 0.85},
            "profile_common": {"automation": "auto_enable_limited", "threshold": 0.92},
            "knowledge_space": {"automation": "auto_enable_limited", "threshold": 0.94},
            "research_topic": {"automation": "auto_enable_limited", "threshold": 0.94},
            "source_collection": {"automation": "manual_first_with_shadow", "threshold": 1.0},
        },
    })
}

fn require_status(status: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&status) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory state transition"))
    }
}

fn clamp_list_limit(limit: usize, max: usize) -> usize {
    limit.clamp(1, max)
}

fn require_non_empty<'a>(value: &'a str, message: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{message}"))
    } else {
        Ok(value)
    }
}

fn require_required_reason(reason: Option<&str>) -> Result<&str> {
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("transition reason is required"))
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() { fallback } else { value }
}

fn memory_audit_ref(kind: &str, id: &str) -> String {
    format!("memory-audit://tonglingyu/{kind}/{id}")
}

#[derive(Debug, Clone)]
struct ResolverOutput {
    resolved_question: String,
    referent_bindings: Vec<String>,
    used_context_refs: Vec<String>,
    confidence: f64,
    needs_clarification: bool,
    clarification_question: Option<String>,
    unsupported_reason: Option<String>,
    strategy: String,
    agent_audit: Option<Value>,
}

impl ResolverOutput {
    fn audit_json(&self) -> Value {
        json!({
            "schema_version": RESOLVER_SCHEMA_VERSION,
            "strategy": self.strategy,
            "resolved_question": self.resolved_question,
            "referent_bindings": self.referent_bindings,
            "used_context_refs": self.used_context_refs,
            "confidence": self.confidence,
            "needs_clarification": self.needs_clarification,
            "clarification_question": self.clarification_question,
            "unsupported_reason": self.unsupported_reason,
            "llm_used": self.agent_audit
                .as_ref()
                .and_then(|audit| audit.get("accepted_for_main"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "agent_invoked": self.agent_audit.is_some(),
            "agent_decision": self.agent_audit,
        })
    }

    fn from_agent_decision(
        sealed: &crate::llm_agent_validator::SealedQuestionResolution,
        agent_audit: Option<Value>,
    ) -> Self {
        Self {
            resolved_question: sealed.resolved_question().to_string(),
            referent_bindings: sealed.referent_bindings().to_vec(),
            used_context_refs: sealed.used_context_refs().to_vec(),
            confidence: sealed.confidence(),
            needs_clarification: sealed.needs_clarification(),
            clarification_question: sealed.clarification_question().map(str::to_string),
            unsupported_reason: sealed.unsupported_reason().map(str::to_string),
            strategy: "llm_agent_enforced".to_string(),
            agent_audit,
        }
    }
}

pub(crate) fn get_or_create_user_session(
    conn: &Connection,
    external_user_ref: &str,
    external_session_id: &str,
    model_id: &str,
) -> Result<String> {
    let existing = conn
        .query_row(
            "SELECT user_session_id FROM user_sessions
             WHERE external_user_ref = ?1 AND external_session_id = ?2 AND model_id = ?3",
            params![external_user_ref, external_session_id, model_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(user_session_id) = existing {
        conn.execute(
            "UPDATE user_sessions SET updated_at = ?1 WHERE user_session_id = ?2",
            params![now_rfc3339(), user_session_id],
        )?;
        return Ok(user_session_id);
    }
    let user_session_id = format!("user-session-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO user_sessions (
            user_session_id, external_user_ref, external_session_id, model_id,
            lifecycle_status, retention_policy, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            &user_session_id,
            external_user_ref,
            external_session_id,
            model_id,
            "active",
            JOURNAL_RETENTION_POLICY_VERSION,
            &now,
            &now,
        ],
    )?;
    Ok(user_session_id)
}

fn get_or_create_interaction_context(conn: &Connection, user_session_id: &str) -> Result<String> {
    let existing = conn
        .query_row(
            "SELECT interaction_context_id FROM interaction_contexts
             WHERE user_session_id = ?1 AND context_mode = 'session'",
            params![user_session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(interaction_context_id) = existing {
        conn.execute(
            "UPDATE interaction_contexts SET updated_at = ?1 WHERE interaction_context_id = ?2",
            params![now_rfc3339(), interaction_context_id],
        )?;
        return Ok(interaction_context_id);
    }
    let interaction_context_id = format!("interaction-context-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO interaction_contexts (
            interaction_context_id, user_session_id, context_status, context_mode,
            resolution_version, permission_version, memory_policy_version, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            &interaction_context_id,
            user_session_id,
            "active",
            "session",
            RESOLVER_SCHEMA_VERSION,
            CONTEXT_POLICY_VERSION,
            SCOPED_MEMORY_POLICY_VERSION,
            &now,
            &now,
        ],
    )?;
    Ok(interaction_context_id)
}

fn append_journal_entry(conn: &Connection, input: JournalEntryInput<'_>) -> Result<()> {
    let content_sha256 = input.content.map(hash_text);
    conn.execute(
        "INSERT INTO session_journal (
            journal_id, trace_id, user_session_id, interaction_context_id, context_pack_id,
            package_id, external_message_id, entry_type, content, summary, content_sha256, content_ref,
            retention_policy, sensitivity, metadata_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            format!("journal-{}", uuid::Uuid::now_v7().simple()),
            input.trace_id,
            input.user_session_id,
            input.interaction_context_id,
            input.context_pack_id,
            input.package_id,
            input.external_message_id,
            input.entry_type,
            input.content,
            input.summary,
            content_sha256,
            input.content.map(|content| format!("sha256:{}", &hash_text(content)[..16])),
            input.retention_policy,
            input.sensitivity,
            serde_json::to_string(&input.metadata)?,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

fn resolve_question(
    question: &str,
    messages: &[ContextMessage],
    prior_subject: Option<&str>,
    prior_user_question: Option<&str>,
) -> Result<ResolverOutput> {
    if is_continue_only_question(question)? {
        let (resolved_question, used_context_ref) =
            if let Some(anchor) = latest_prior_user_question(messages, question) {
                (anchor, "session_history")
            } else if let Some(anchor) = prior_user_question {
                (anchor.to_string(), "session_journal_prior_question")
            } else {
                ("".to_string(), "")
            };
        if !resolved_question.is_empty() {
            return Ok(ResolverOutput {
                resolved_question,
                referent_bindings: Vec::new(),
                used_context_refs: vec![used_context_ref.to_string()],
                confidence: 0.88,
                needs_clarification: false,
                clarification_question: None,
                unsupported_reason: None,
                strategy: "deterministic_rules".to_string(),
                agent_audit: None,
            });
        }
        return Ok(ResolverOutput {
            resolved_question: question.to_string(),
            referent_bindings: Vec::new(),
            used_context_refs: Vec::new(),
            confidence: 0.2,
            needs_clarification: true,
            clarification_question: Some("请说明要继续回答哪个问题。".to_string()),
            unsupported_reason: Some("unresolved_continuation".to_string()),
            strategy: "deterministic_rules".to_string(),
            agent_audit: None,
        });
    }
    if context_rules::is_elliptical_followup_question(question)? {
        let anchor = latest_prior_user_question(messages, question)
            .map(|value| (value, "session_history"))
            .or_else(|| {
                prior_user_question
                    .map(|value| (value.to_string(), "session_journal_prior_question"))
            });
        if let Some((anchor, used_context_ref)) = anchor {
            if let Some(resolved_question) =
                context_rules::resolve_elliptical_followup(question, &anchor)?
            {
                let referent_bindings = latest_subject_in_text(&anchor)?.into_iter().collect();
                return Ok(ResolverOutput {
                    resolved_question,
                    referent_bindings,
                    used_context_refs: vec![used_context_ref.to_string()],
                    confidence: 0.9,
                    needs_clarification: false,
                    clarification_question: None,
                    unsupported_reason: None,
                    strategy: "deterministic_elliptical_followup".to_string(),
                    agent_audit: None,
                });
            }
        }
        return Ok(ResolverOutput {
            resolved_question: question.to_string(),
            referent_bindings: Vec::new(),
            used_context_refs: Vec::new(),
            confidence: 0.2,
            needs_clarification: true,
            clarification_question: Some("请说明这条追问承接上一条中的哪个问题。".to_string()),
            unsupported_reason: Some("unresolved_elliptical_followup".to_string()),
            strategy: "deterministic_rules".to_string(),
            agent_audit: None,
        });
    }
    let current_subject = latest_subject_in_text(question)?;
    if let Some(subject) = current_subject {
        return Ok(ResolverOutput {
            resolved_question: question.to_string(),
            referent_bindings: vec![subject],
            used_context_refs: Vec::new(),
            confidence: 1.0,
            needs_clarification: false,
            clarification_question: None,
            unsupported_reason: None,
            strategy: "deterministic_rules".to_string(),
            agent_audit: None,
        });
    }
    if contains_referential_pronoun(question)? {
        let referent =
            latest_subject_from_messages(messages)?.or_else(|| prior_subject.map(str::to_string));
        if let Some(referent) = referent {
            let resolved_question = bind_referent(question, &referent)?;
            return Ok(ResolverOutput {
                resolved_question,
                referent_bindings: vec![referent],
                used_context_refs: vec!["session_summary".to_string()],
                confidence: 0.86,
                needs_clarification: false,
                clarification_question: None,
                unsupported_reason: None,
                strategy: "deterministic_rules".to_string(),
                agent_audit: None,
            });
        }
        return Ok(ResolverOutput {
            resolved_question: question.to_string(),
            referent_bindings: Vec::new(),
            used_context_refs: Vec::new(),
            confidence: 0.2,
            needs_clarification: true,
            clarification_question: Some(
                "请明确你指的是哪位人物或对象，我再继续回答。".to_string(),
            ),
            unsupported_reason: Some("unresolved_referent".to_string()),
            strategy: "deterministic_rules".to_string(),
            agent_audit: None,
        });
    }
    Ok(ResolverOutput {
        resolved_question: question.to_string(),
        referent_bindings: Vec::new(),
        used_context_refs: Vec::new(),
        confidence: 1.0,
        needs_clarification: false,
        clarification_question: None,
        unsupported_reason: None,
        strategy: "deterministic_rules".to_string(),
        agent_audit: None,
    })
}

fn is_continue_only_question(text: &str) -> Result<bool> {
    context_rules::is_continue_only_question(text)
}

fn latest_prior_user_question(
    messages: &[ContextMessage],
    current_question: &str,
) -> Option<String> {
    let mut skipped_current = false;
    messages.iter().rev().find_map(|message| {
        if message.role != "user" {
            return None;
        }
        let content = message.content.trim();
        if !skipped_current && content == current_question.trim() {
            skipped_current = true;
            return None;
        }
        if content.is_empty()
            || is_continue_only_question(content).unwrap_or(false)
            || is_openwebui_metadata_prompt(content)
        {
            return None;
        }
        Some(content.to_string())
    })
}

fn session_summary(
    messages: &[ContextMessage],
    prior_subject: Option<&str>,
    prior_user_question: Option<&str>,
) -> Result<String> {
    let mut parts = Vec::new();
    if let Some(subject) = latest_subject_from_messages(messages)? {
        parts.push(format!("当前窗口候选主体：{subject}"));
    } else if let Some(subject) = prior_subject {
        parts.push(format!("session_journal_candidate：{subject}"));
    }
    if let Some(prior_question) = prior_user_question
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!(
            "session_journal_prior_question：{}",
            bounded_summary(prior_question, 80)
        ));
    }
    let recent_users = messages
        .iter()
        .filter(|message| message.role == "user")
        .rev()
        .take(3)
        .map(|message| bounded_summary(&message.content, 80))
        .collect::<Vec<_>>();
    if !recent_users.is_empty() {
        parts.push(format!(
            "最近用户问题：{}",
            recent_users
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" / ")
        ));
    }
    if parts.is_empty() {
        Ok("无可用会话摘要。".to_string())
    } else {
        Ok(bounded_summary(
            &parts.join("；"),
            SESSION_SUMMARY_MAX_CHARS,
        ))
    }
}

fn load_authorized_memory_reads(
    conn: &Connection,
    external_user_ref: &str,
    active_scopes: &[Value],
    candidate_scopes: &[Value],
) -> Result<ScopedMemoryReadSet> {
    let user_scope_ref = user_private_scope_ref(external_user_ref);
    let now = now_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT card.memory_card_ref, card.source_candidate_id, card.scope_type,
                card.scope_ref, card.summary, card.summary_sha256, card.acl_json,
                card.sensitivity, card.expires_at, candidate.candidate_type,
                decision.policy_decision_ref, decision.policy_version,
                decision.policy_mode, decision.confidence
         FROM memory_cards AS card
         JOIN memory_candidates AS candidate
           ON candidate.candidate_id = card.source_candidate_id
         JOIN memory_policy_decisions AS decision
           ON decision.memory_card_id = card.memory_card_id
          AND decision.decision = 'enable_read'
          AND decision.policy_version = ?1
         WHERE card.status = 'active'
           AND card.read_enabled <> 0
           AND (card.expires_at IS NULL OR card.expires_at > ?2)
           AND candidate.source_entry_type = 'user_message'
           AND candidate.context_pack_id IS NOT NULL
         ORDER BY decision.confidence DESC, card.promoted_at DESC, card.memory_card_id DESC",
    )?;
    let rows = stmt.query_map(params![SCOPED_MEMORY_POLICY_VERSION, &now], |row| {
        let memory_card_ref: String = row.get(0)?;
        let summary_sha256: String = row.get(5)?;
        let acl = parse_json_column(row.get::<_, String>(6)?);
        Ok((
            memory_card_ref.clone(),
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            summary_sha256.clone(),
            acl,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, String>(12)?,
            row.get::<_, f64>(13)?,
        ))
    })?;
    let mut eligible = Vec::<ScopedMemoryRead>::new();
    for row in rows {
        let (
            memory_card_ref,
            _source_candidate_id,
            scope_type,
            scope_ref,
            summary,
            summary_sha256,
            acl,
            sensitivity,
            expires_at,
            candidate_type,
            policy_decision_ref,
            policy_version,
            policy_mode,
            confidence,
        ) = row?;
        if !memory_scope_matches_context(
            &scope_type,
            &scope_ref,
            &user_scope_ref,
            active_scopes,
            candidate_scopes,
        ) {
            continue;
        }
        let allowed_consumers = acl_allowed_consumers(&acl);
        if allowed_consumers.is_empty() {
            return Err(anyhow!("memory ACL has no allowed consumers"));
        }
        eligible.push(ScopedMemoryRead {
            memory_read_ref: format!(
                "memory-summary://tonglingyu/{}/{}",
                &hash_text(&memory_card_ref)[..16],
                &summary_sha256[..16]
            ),
            memory_card_ref,
            policy_decision_ref,
            policy_version,
            policy_mode,
            scope_type,
            candidate_type,
            summary,
            sensitivity,
            confidence,
            expires_at,
            allowed_consumers,
        });
    }
    let candidate_count_before_budget = eligible.len();
    let mut user_private = Vec::new();
    let mut shared = Vec::new();
    for read in eligible {
        if read.scope_type == "user_private" {
            user_private.push(read);
        } else {
            shared.push(read);
        }
    }
    let mut reads = user_private
        .into_iter()
        .take(MEMORY_READ_BUDGET_USER_PRIVATE)
        .collect::<Vec<_>>();
    reads.extend(shared.into_iter().take(MEMORY_READ_BUDGET_SHARED));
    reads.truncate(MEMORY_READ_BUDGET_TOTAL);
    let truncated_count = candidate_count_before_budget.saturating_sub(reads.len());
    Ok(ScopedMemoryReadSet {
        reads,
        candidate_count_before_budget,
        truncated_count,
    })
}

fn memory_scope_matches_context(
    scope_type: &str,
    scope_ref: &str,
    user_scope_ref: &str,
    active_scopes: &[Value],
    candidate_scopes: &[Value],
) -> bool {
    match scope_type {
        "user_private" => scope_ref == user_scope_ref,
        "research_topic" => candidate_scopes.iter().any(|scope| {
            scope
                .get("scope_id")
                .and_then(Value::as_str)
                .is_some_and(|scope_id| scope_id == scope_ref)
        }),
        "profile_common" | "knowledge_space" | "source_collection" => {
            active_scopes.iter().any(|scope| {
                scope
                    .get("scope_type")
                    .and_then(Value::as_str)
                    .is_some_and(|active_type| active_type == scope_type)
                    && scope
                        .get("scope_id")
                        .and_then(Value::as_str)
                        .is_some_and(|active_ref| active_ref == scope_ref)
            })
        }
        _ => false,
    }
}

fn acl_allowed_consumers(acl: &Value) -> Vec<String> {
    acl.get("allowed_consumers")
        .or_else(|| acl.get("allowed_readers"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn memory_reads_for_consumer(
    memory_reads: &[ScopedMemoryRead],
    consumer: &str,
    limit: usize,
) -> Vec<ScopedMemoryRead> {
    memory_reads
        .iter()
        .filter(|read| read.allowed_consumers.iter().any(|item| item == consumer))
        .take(limit)
        .cloned()
        .collect()
}

fn tool_profile_memory_reads(
    memory_reads: &[ScopedMemoryRead],
    consumer: &str,
) -> Vec<ScopedMemoryRead> {
    memory_reads
        .iter()
        .filter(|read| {
            read.scope_type != "user_private"
                && matches!(
                    read.candidate_type.as_str(),
                    "retrieval_preference" | "source_collection_usage_preference"
                )
                && read.allowed_consumers.iter().any(|item| item == consumer)
        })
        .take(MEMORY_READ_BUDGET_TOOL_PROFILE)
        .cloned()
        .collect()
}

fn memory_read_refs_for_reads(memory_reads: &[ScopedMemoryRead]) -> Vec<String> {
    memory_reads
        .iter()
        .map(|read| read.memory_read_ref.clone())
        .collect()
}

fn memory_read_summary_payload(read: &ScopedMemoryRead) -> Value {
    json!({
        "memory_read_ref": &read.memory_read_ref,
        "summary": &read.summary,
        "scope_type": &read.scope_type,
        "candidate_type": &read.candidate_type,
        "sensitivity": &read.sensitivity,
        "policy_version": &read.policy_version,
        "policy_mode": &read.policy_mode,
        "confidence": read.confidence,
        "expires_at": &read.expires_at,
    })
}

fn memory_read_ref_digest(memory_read_refs: &[String]) -> String {
    digest_json(&json!({
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "memory_read_refs": memory_read_refs,
    }))
}

fn memory_read_policy_digest(memory_reads: &[ScopedMemoryRead]) -> String {
    digest_json(&json!({
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "read_refs": memory_reads
            .iter()
            .map(|read| json!({
                "memory_read_ref": &read.memory_read_ref,
                "policy_decision_ref": &read.policy_decision_ref,
                "memory_card_ref_sha256": hash_text(&read.memory_card_ref),
                "policy_version": &read.policy_version,
                "policy_mode": &read.policy_mode,
                "scope_type": &read.scope_type,
                "candidate_type": &read.candidate_type,
                "confidence": read.confidence,
            }))
            .collect::<Vec<_>>(),
    }))
}

fn memory_usage_summary(memory_reads: &[ScopedMemoryRead], truncated_count: usize) -> Value {
    json!({
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "read_ref_count": memory_reads.len(),
        "truncated_count": truncated_count,
        "user_private_count": memory_reads.iter().filter(|read| read.scope_type == "user_private").count(),
        "shared_scope_count": memory_reads.iter().filter(|read| read.scope_type != "user_private").count(),
        "memory_content_visible": true,
    })
}

fn reviewer_memory_usage_summary(memory_reads: &[ScopedMemoryRead]) -> Value {
    json!({
        "policy_version": SCOPED_MEMORY_POLICY_VERSION,
        "read_ref_count": memory_reads.len(),
        "memory_content_visible": false,
        "reviewer_can_use_memory_as_evidence": false,
        "memory_policy_digest": memory_read_policy_digest(memory_reads),
    })
}

fn profile_views(
    resolved_question: &str,
    session_summary: &str,
    memory_reads: &[ScopedMemoryRead],
) -> Vec<ContextPackProfileView> {
    let main_reads =
        memory_reads_for_consumer(memory_reads, "honglou-main", MEMORY_READ_BUDGET_TOTAL);
    let text_reads = tool_profile_memory_reads(memory_reads, "honglou-text");
    let commentary_reads = tool_profile_memory_reads(memory_reads, "honglou-commentary");
    let reviewer_usage = reviewer_memory_usage_summary(memory_reads);
    let main_read_refs = memory_read_refs_for_reads(&main_reads);
    let text_read_refs = memory_read_refs_for_reads(&text_reads);
    let commentary_read_refs = memory_read_refs_for_reads(&commentary_reads);
    let reviewer_read_refs = Vec::<String>::new();
    vec![
        ContextPackProfileView {
            profile_name: "honglou-main".to_string(),
            visible_question: resolved_question.to_string(),
            session_summary: Some(session_summary.to_string()),
            allowed_tools: vec![
                "tonglingyu.evidence.package.create".to_string(),
                "tonglingyu.evidence.package.read".to_string(),
            ],
            forbidden_context: vec![
                "complete_user_history".to_string(),
                "unauthorized_memory".to_string(),
                "system_prompt".to_string(),
                "unreviewed_memory_candidate".to_string(),
            ],
            memory_read_ref_digest: memory_read_ref_digest(&main_read_refs),
            memory_read_refs: main_read_refs,
            memory_summaries: main_reads.iter().map(memory_read_summary_payload).collect(),
            memory_policy_digest: memory_read_policy_digest(&main_reads),
            memory_usage_summary: memory_usage_summary(&main_reads, 0),
        },
        ContextPackProfileView {
            profile_name: "honglou-text".to_string(),
            visible_question: resolved_question.to_string(),
            session_summary: None,
            allowed_tools: vec!["tonglingyu.text.search".to_string()],
            forbidden_context: vec![
                "complete_user_history".to_string(),
                "user_private_memory".to_string(),
                "unauthorized_scoped_memory".to_string(),
                "unreviewed_memory_candidate".to_string(),
            ],
            memory_read_ref_digest: memory_read_ref_digest(&text_read_refs),
            memory_read_refs: text_read_refs,
            memory_summaries: text_reads.iter().map(memory_read_summary_payload).collect(),
            memory_policy_digest: memory_read_policy_digest(&text_reads),
            memory_usage_summary: memory_usage_summary(&text_reads, 0),
        },
        ContextPackProfileView {
            profile_name: "honglou-commentary".to_string(),
            visible_question: resolved_question.to_string(),
            session_summary: None,
            allowed_tools: vec!["tonglingyu.commentary.search".to_string()],
            forbidden_context: vec![
                "complete_user_history".to_string(),
                "full_base_text_corpus".to_string(),
                "unreviewed_memory_candidate".to_string(),
            ],
            memory_read_ref_digest: memory_read_ref_digest(&commentary_read_refs),
            memory_read_refs: commentary_read_refs,
            memory_summaries: commentary_reads
                .iter()
                .map(memory_read_summary_payload)
                .collect(),
            memory_policy_digest: memory_read_policy_digest(&commentary_reads),
            memory_usage_summary: memory_usage_summary(&commentary_reads, 0),
        },
        ContextPackProfileView {
            profile_name: "honglou-reviewer".to_string(),
            visible_question: resolved_question.to_string(),
            session_summary: None,
            allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
            forbidden_context: vec![
                "user_private_memory".to_string(),
                "unreviewed_memory_candidate".to_string(),
                "hermes_private_transcript".to_string(),
            ],
            memory_read_ref_digest: memory_read_ref_digest(&reviewer_read_refs),
            memory_read_refs: reviewer_read_refs,
            memory_summaries: Vec::new(),
            memory_policy_digest: memory_read_policy_digest(memory_reads),
            memory_usage_summary: reviewer_usage,
        },
    ]
}

fn build_context_projections(
    trace_id: &str,
    interaction_context_id: &str,
    context_pack_id: &str,
    context_pack_ref: &str,
    profile_views: &[ContextPackProfileView],
    conversation_state_summary: Option<&ConversationStateSummary>,
) -> Vec<ContextProjection> {
    profile_views
        .iter()
        .cloned()
        .map(|view| {
            build_context_projection(
                trace_id,
                interaction_context_id,
                context_pack_id,
                context_pack_ref,
                view,
                conversation_state_summary,
            )
        })
        .collect()
}

fn build_context_projection(
    trace_id: &str,
    interaction_context_id: &str,
    context_pack_id: &str,
    context_pack_ref: &str,
    view: ContextPackProfileView,
    conversation_state_summary: Option<&ConversationStateSummary>,
) -> ContextProjection {
    let context_projection_id = format!("context-projection-{}", uuid::Uuid::now_v7().simple());
    let context_projection_ref =
        format!("context-projection://tonglingyu/{trace_id}/{context_projection_id}");
    let output_contract = projection_output_contract(&view.profile_name);
    let forbidden_tools = Vec::<String>::new();
    let projected_conversation_state = conversation_state_summary
        .and_then(|summary| project_conversation_state_summary(summary, &view.profile_name));
    let projected_conversation_state_digest = if projected_conversation_state.is_some() {
        conversation_state_summary.map(conversation_state_summary_digest)
    } else {
        None
    };
    let projection_payload = json!({
        "object": "tonglingyu.context_projection_payload",
        "visible_question": &view.visible_question,
        "session_summary": &view.session_summary,
        "conversation_state_summary": &projected_conversation_state,
        "conversation_state_summary_digest": &projected_conversation_state_digest,
        "forbidden_context": &view.forbidden_context,
        "memory_read_refs": &view.memory_read_refs,
        "memory_read_ref_digest": &view.memory_read_ref_digest,
        "memory_summaries": &view.memory_summaries,
        "memory_policy_digest": &view.memory_policy_digest,
        "memory_usage_summary": &view.memory_usage_summary,
        "consumer_name": &view.profile_name,
    });
    let tool_policy = json!({
        "allowed_tools": &view.allowed_tools,
        "forbidden_tools": &forbidden_tools,
    });
    let tool_policy_digest = digest_json(&tool_policy);
    let output_contract_digest = digest_json(&output_contract);
    let unsigned_projection = json!({
        "context_projection_id": &context_projection_id,
        "context_projection_ref": &context_projection_ref,
        "context_pack_ref": context_pack_ref,
        "consumer_type": RUNTIME_CONSUMER_TYPE,
        "consumer_name": &view.profile_name,
        "runtime_adapter": RUNTIME_ADAPTER,
        "projection_payload": &projection_payload,
        "allowed_tools": &view.allowed_tools,
        "forbidden_tools": &forbidden_tools,
        "output_contract": &output_contract,
        "tool_policy_digest": &tool_policy_digest,
        "output_contract_digest": &output_contract_digest,
        "schema_version": CONTEXT_PROJECTION_SCHEMA_VERSION,
    });
    let digest = digest_json(&unsigned_projection);
    ContextProjection {
        context_projection_id,
        context_projection_ref,
        context_pack_id: context_pack_id.to_string(),
        context_pack_ref: context_pack_ref.to_string(),
        trace_id: trace_id.to_string(),
        interaction_context_id: interaction_context_id.to_string(),
        consumer_type: RUNTIME_CONSUMER_TYPE.to_string(),
        consumer_name: view.profile_name,
        runtime_adapter: RUNTIME_ADAPTER.to_string(),
        projection_payload,
        allowed_tools: view.allowed_tools,
        forbidden_tools,
        output_contract,
        tool_policy_digest,
        output_contract_digest,
        schema_version: CONTEXT_PROJECTION_SCHEMA_VERSION.to_string(),
        digest,
        status: "active".to_string(),
    }
}

fn projection_output_contract(consumer_name: &str) -> Value {
    match consumer_name {
        "honglou-text" | "honglou-commentary" => json!({
            "object": "tonglingyu.evidence_analysis",
            "must_not_write_final_answer": true,
            "must_return_output_ref": true,
        }),
        "honglou-reviewer" => json!({
            "object": "tonglingyu.review_observation",
            "reviewer_is_observational": true,
            "local_review_enforcement_remains_authoritative": true,
            "must_return_output_ref": true,
        }),
        _ => json!({
            "object": "tonglingyu.main_runtime_projection",
            "must_return_output_ref": true,
            "evidence_package_allows_memory": false,
            "conversation_state_summary_allows_evidence": false,
            "conversation_state_summary_scope": "projection_only",
        }),
    }
}

fn insert_context_projection(conn: &Connection, projection: &ContextProjection) -> Result<()> {
    conn.execute(
        "INSERT INTO context_projections (
            context_projection_id, context_projection_ref, context_pack_id, context_pack_ref,
            trace_id, interaction_context_id, consumer_type, consumer_name, runtime_adapter,
            projection_payload_json, allowed_tools_json, forbidden_tools_json,
            output_contract_json, tool_policy_digest, output_contract_digest, schema_version,
            digest, status, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            &projection.context_projection_id,
            &projection.context_projection_ref,
            &projection.context_pack_id,
            &projection.context_pack_ref,
            &projection.trace_id,
            &projection.interaction_context_id,
            &projection.consumer_type,
            &projection.consumer_name,
            &projection.runtime_adapter,
            serde_json::to_string(&projection.projection_payload)?,
            serde_json::to_string(&projection.allowed_tools)?,
            serde_json::to_string(&projection.forbidden_tools)?,
            serde_json::to_string(&projection.output_contract)?,
            &projection.tool_policy_digest,
            &projection.output_contract_digest,
            &projection.schema_version,
            &projection.digest,
            &projection.status,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

fn latest_subject_from_journal(conn: &Connection, user_session_id: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT summary FROM session_journal
         WHERE user_session_id = ?1 AND entry_type = 'user_message'
         ORDER BY created_at DESC LIMIT 20",
    )?;
    let rows = stmt.query_map(params![user_session_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        if let Some(subject) = latest_subject_in_text(&row?)? {
            return Ok(Some(subject));
        }
    }
    Ok(None)
}

fn latest_prior_user_question_from_journal(
    conn: &Connection,
    user_session_id: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT content, summary FROM session_journal
         WHERE user_session_id = ?1 AND entry_type = 'user_message'
         ORDER BY created_at DESC, journal_id DESC LIMIT 1",
        params![user_session_id],
        |row| {
            let content = row.get::<_, Option<String>>(0)?;
            let summary = row.get::<_, String>(1)?;
            Ok(content
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    let value = summary.trim().to_string();
                    (!value.is_empty()).then_some(value)
                }))
        },
    )
    .optional()
    .map(|value| value.flatten())
    .map_err(Into::into)
}

fn latest_subject_from_messages(messages: &[ContextMessage]) -> Result<Option<String>> {
    for message in messages
        .iter()
        .rev()
        .filter(|message| message.role == "user" || message.role == "assistant")
    {
        if let Some(subject) = latest_subject_in_text(&message.content)? {
            return Ok(Some(subject));
        }
    }
    Ok(None)
}

fn latest_subject_in_text(text: &str) -> Result<Option<String>> {
    context_rules::latest_subject_in_text(text)
}

fn contains_referential_pronoun(text: &str) -> Result<bool> {
    context_rules::contains_referential_pronoun(text)
}

fn bind_referent(question: &str, referent: &str) -> Result<String> {
    context_rules::bind_referent(question, referent)
}

fn is_openwebui_metadata_prompt(text: &str) -> bool {
    text.contains("### Task:") && text.contains("### Chat History:")
}

fn load_context_packs(conn: &Connection, trace_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT context_pack_id, COALESCE(context_pack_ref, context_pack_id),
                interaction_context_id, profile_name, resolved_question,
                session_summary, active_scopes_json, candidate_scopes_json, allowed_tools_json,
                forbidden_tools_json, memory_read_refs_json, forbidden_context_json,
                output_contract_json, profile_views_json, COALESCE(policy_versions_json, '{}'),
                COALESCE(llm_agent_context_path_json, '{}'), schema_version, COALESCE(digest, ''),
                created_at
         FROM context_packs WHERE trace_id = ?1 ORDER BY created_at, context_pack_id",
    )?;
    let rows = stmt.query_map(params![trace_id], |row| {
        let memory_read_refs = parse_json_column(row.get::<_, String>(10)?);
        let mut pack = json!({
            "context_pack_id": row.get::<_, String>(0)?,
            "context_pack_ref": row.get::<_, String>(1)?,
            "interaction_context_id": row.get::<_, String>(2)?,
            "profile_name": row.get::<_, String>(3)?,
            "resolved_question": row.get::<_, String>(4)?,
            "session_summary": row.get::<_, String>(5)?,
            "active_scopes": parse_json_column(row.get::<_, String>(6)?),
            "candidate_scopes": parse_json_column(row.get::<_, String>(7)?),
            "allowed_tools": parse_json_column(row.get::<_, String>(8)?),
            "forbidden_tools": parse_json_column(row.get::<_, String>(9)?),
            "memory_read_ref_digest": memory_read_ref_digest_from_value(&memory_read_refs),
            "memory_read_refs": memory_read_refs,
            "forbidden_context": parse_json_column(row.get::<_, String>(11)?),
            "output_contract": parse_json_column(row.get::<_, String>(12)?),
            "profile_views": parse_json_column(row.get::<_, String>(13)?),
            "policy_versions": parse_json_column(row.get::<_, String>(14)?),
            "llm_agent_context_path": parse_json_column(row.get::<_, String>(15)?),
            "schema_version": row.get::<_, String>(16)?,
            "digest": row.get::<_, String>(17)?,
            "created_at": row.get::<_, String>(18)?,
        });
        redact_admin_trace_content_fields(&mut pack);
        Ok(pack)
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn load_context_projections(conn: &Connection, trace_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT context_projection_id, context_projection_ref, context_pack_id,
                context_pack_ref, interaction_context_id, consumer_type, consumer_name,
                runtime_adapter, projection_payload_json, allowed_tools_json,
                forbidden_tools_json, output_contract_json, tool_policy_digest,
                output_contract_digest, schema_version, digest, status, created_at
         FROM context_projections WHERE trace_id = ?1
         ORDER BY created_at, context_projection_id",
    )?;
    let rows = stmt.query_map(params![trace_id], |row| {
        let projection_payload = parse_json_column(row.get::<_, String>(8)?);
        Ok(json!({
            "context_projection_id": row.get::<_, String>(0)?,
            "context_projection_ref": row.get::<_, String>(1)?,
            "context_pack_id": row.get::<_, String>(2)?,
            "context_pack_ref": row.get::<_, String>(3)?,
            "interaction_context_id": row.get::<_, String>(4)?,
            "consumer_type": row.get::<_, String>(5)?,
            "consumer_name": row.get::<_, String>(6)?,
            "runtime_adapter": row.get::<_, String>(7)?,
            "projection_payload_summary": projection_payload_summary(&projection_payload),
            "projection_payload_sha256": digest_json(&projection_payload),
            "allowed_tools": parse_json_column(row.get::<_, String>(9)?),
            "forbidden_tools": parse_json_column(row.get::<_, String>(10)?),
            "output_contract": parse_json_column(row.get::<_, String>(11)?),
            "tool_policy_digest": row.get::<_, String>(12)?,
            "output_contract_digest": row.get::<_, String>(13)?,
            "schema_version": row.get::<_, String>(14)?,
            "digest": row.get::<_, String>(15)?,
            "status": row.get::<_, String>(16)?,
            "created_at": row.get::<_, String>(17)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn load_contexts_for_session(conn: &Connection, user_session_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT interaction_context_id, context_status, context_mode, resolution_version,
                permission_version, memory_policy_version, created_at, updated_at
         FROM interaction_contexts WHERE user_session_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt.query_map(params![user_session_id], |row| {
        Ok(json!({
            "interaction_context_id": row.get::<_, String>(0)?,
            "context_status": row.get::<_, String>(1)?,
            "context_mode": row.get::<_, String>(2)?,
            "resolution_version": row.get::<_, String>(3)?,
            "permission_version": row.get::<_, String>(4)?,
            "memory_policy_version": row.get::<_, String>(5)?,
            "created_at": row.get::<_, String>(6)?,
            "updated_at": row.get::<_, String>(7)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn load_journal_summaries_for_trace(conn: &Connection, trace_id: &str) -> Result<Vec<Value>> {
    load_journal_summaries(
        conn,
        "WHERE trace_id = ?1 ORDER BY created_at, journal_id",
        trace_id,
    )
}

fn load_journal_summaries_for_session(
    conn: &Connection,
    user_session_id: &str,
) -> Result<Vec<Value>> {
    load_journal_summaries(
        conn,
        "WHERE user_session_id = ?1 ORDER BY created_at, journal_id",
        user_session_id,
    )
}

fn load_journal_summaries(conn: &Connection, clause: &str, value: &str) -> Result<Vec<Value>> {
    let sql = format!(
        "SELECT journal_id, trace_id, user_session_id, interaction_context_id, context_pack_id,
                package_id,
                external_message_id, entry_type, summary, content_sha256, content_ref,
                retention_policy, sensitivity, metadata_json, created_at
         FROM session_journal {clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![value], |row| {
        let mut metadata = redact_journal_metadata(parse_json_column(row.get::<_, String>(13)?));
        redact_admin_trace_content_fields(&mut metadata);
        Ok(json!({
            "journal_id": row.get::<_, String>(0)?,
            "trace_id": row.get::<_, String>(1)?,
            "user_session_id": row.get::<_, String>(2)?,
            "interaction_context_id": row.get::<_, String>(3)?,
            "context_pack_id": row.get::<_, Option<String>>(4)?,
            "package_id": row.get::<_, Option<String>>(5)?,
            "external_message_id": row.get::<_, Option<String>>(6)?,
            "entry_type": row.get::<_, String>(7)?,
            "summary": row.get::<_, String>(8)?,
            "content_sha256": row.get::<_, Option<String>>(9)?,
            "content_ref": row.get::<_, Option<String>>(10)?,
            "retention_policy": row.get::<_, String>(11)?,
            "sensitivity": row.get::<_, String>(12)?,
            "metadata": metadata,
            "created_at": row.get::<_, String>(14)?,
            "raw_content_included": false,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn redact_journal_metadata(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("response");
        object.insert(
            "raw_content_in_default_admin_trace".to_string(),
            json!(false),
        );
    }
    value
}

pub(crate) fn redact_admin_trace_content_fields(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(raw_content) = object.remove("content") {
                object.insert("raw_content_redacted".to_string(), json!(true));
                object.insert(
                    "raw_content_value_type".to_string(),
                    json!(json_type_name(&raw_content)),
                );
                object.insert(
                    "raw_content_sha256".to_string(),
                    json!(format!("sha256:{}", digest_redacted_content(&raw_content))),
                );
                if let Some(text) = raw_content.as_str() {
                    object.insert(
                        "raw_content_char_count".to_string(),
                        json!(text.chars().count()),
                    );
                }
            }
            for child in object.values_mut() {
                redact_admin_trace_content_fields(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_admin_trace_content_fields(item);
            }
        }
        _ => {}
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn digest_redacted_content(value: &Value) -> String {
    value
        .as_str()
        .map(hash_text)
        .unwrap_or_else(|| digest_json(value))
}

fn parse_json_column(value: String) -> Value {
    serde_json::from_str(&value).unwrap_or(Value::Null)
}

fn memory_read_ref_digest_from_value(value: &Value) -> String {
    let refs = value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    memory_read_ref_digest(&refs)
}

fn projection_payload_summary(value: &Value) -> Value {
    json!({
        "visible_question_sha256": value
            .get("visible_question")
            .and_then(Value::as_str)
            .map(hash_text),
        "has_session_summary": value.get("session_summary").is_some_and(|item| !item.is_null()),
        "has_conversation_state_summary": value
            .get("conversation_state_summary")
            .is_some_and(|item| !item.is_null()),
        "conversation_state_summary_digest": value
            .get("conversation_state_summary_digest")
            .and_then(Value::as_str)
            .map(str::to_string),
        "forbidden_context_count": value
            .get("forbidden_context")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        "memory_read_ref_count": value
            .get("memory_read_refs")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        "memory_summary_count": value
            .get("memory_summaries")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        "memory_read_ref_digest": value
            .get("memory_read_ref_digest")
            .and_then(Value::as_str)
            .map(str::to_string),
        "memory_policy_digest": value
            .get("memory_policy_digest")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(Into::into)
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !columns.iter().any(|existing| existing == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn bounded_summary(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in value.trim().chars().enumerate() {
        if index >= max_chars {
            output.push_str("...");
            break;
        }
        output.push(ch);
    }
    if output.is_empty() {
        "(empty)".to_string()
    } else {
        output
    }
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn digest_json(value: &Value) -> String {
    hash_text(&serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()))
}

fn context_pack_ref(trace_id: &str, context_pack_id: &str) -> String {
    format!("context-pack://tonglingyu/{trace_id}/{context_pack_id}")
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use agent_core::{
        AgentCoreError, CoreResult, ErrorCode, RuntimeOutput, RuntimeRunInput, RuntimeSessionInput,
    };
    use async_trait::async_trait;

    use super::*;

    #[test]
    fn production_agent_modes_are_enforced_without_env_switches() {
        assert_eq!(question_normalizer_agent_mode(), LlmMode::Enforced);
        assert_eq!(conversation_state_agent_mode(), LlmMode::Enforced);
    }

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute(
            "CREATE TABLE schema_migrations (migration_id TEXT PRIMARY KEY, applied_at TEXT NOT NULL)",
            [],
        )
        .expect("schema migration table");
        init_schema(&conn).expect("context schema");
        conn
    }

    fn temp_context_db_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tonglingyu-context-{label}-{}.db",
            uuid::Uuid::now_v7().simple()
        ))
    }

    fn file_conn(path: &PathBuf) -> Connection {
        let conn = Connection::open(path).expect("file db");
        conn.execute(
            "CREATE TABLE schema_migrations (migration_id TEXT PRIMARY KEY, applied_at TEXT NOT NULL)",
            [],
        )
        .expect("schema migration table");
        init_schema(&conn).expect("context schema");
        conn
    }

    fn remove_file_db(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[derive(Debug, Clone)]
    struct FakeRuntimeClient {
        outputs: Arc<Mutex<VecDeque<FakeRuntimeOutput>>>,
        inputs: Arc<Mutex<Vec<RuntimeProfileInput>>>,
    }

    #[derive(Debug, Clone)]
    struct FakeRuntimeOutput {
        result_summary: String,
        result_ref: Option<String>,
        metadata: Value,
    }

    impl FakeRuntimeClient {
        fn new(outputs: Vec<Value>) -> Self {
            let outputs = outputs
                .into_iter()
                .map(|value| FakeRuntimeOutput {
                    result_summary: value.to_string(),
                    result_ref: None,
                    metadata: json!({"fake_runtime": true}),
                })
                .collect::<Vec<_>>();
            Self::with_outputs(outputs)
        }

        fn with_outputs(outputs: Vec<FakeRuntimeOutput>) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(outputs.into_iter().collect::<VecDeque<_>>())),
                inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn profile_inputs(&self) -> Vec<RuntimeProfileInput> {
            self.inputs.lock().expect("fake runtime inputs").clone()
        }
    }

    fn provider_output_metadata(provider_output: Value) -> Value {
        json!({ "provider_output": provider_output })
    }

    #[async_trait]
    impl RuntimeClient for FakeRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "fake runtime does not support execute_run",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "fake runtime does not support sessions",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            self.inputs
                .lock()
                .expect("fake runtime inputs")
                .push(input.clone());
            let output = self
                .outputs
                .lock()
                .expect("fake runtime lock")
                .pop_front()
                .ok_or_else(|| {
                    AgentCoreError::coded(ErrorCode::NotFound, "fake runtime output missing")
                })?;
            Ok(RuntimeOutput {
                result_summary: output.result_summary,
                result_ref: output
                    .result_ref
                    .or_else(|| Some(format!("fake://{}", input.profile_id))),
                messages: Vec::new(),
                metadata: output.metadata,
            })
        }
    }

    struct TestMemoryDraftInput<'a> {
        trace_id: &'a str,
        context: &'a ContextResolution,
        scope_type: &'a str,
        scope_ref: &'a str,
        candidate_type: &'a str,
        summary: &'a str,
        confidence: f64,
    }

    fn test_memory_draft(
        conn: &Connection,
        input: TestMemoryDraftInput<'_>,
    ) -> MemoryCandidateDraft {
        let journal_id = conn
            .query_row(
                "SELECT journal_id FROM session_journal
                 WHERE trace_id = ?1 AND entry_type = 'user_message'
                 ORDER BY created_at DESC, journal_id DESC LIMIT 1",
                params![input.trace_id],
                |row| row.get::<_, String>(0),
            )
            .expect("user journal id");
        let candidate_id = format!(
            "memory-candidate-test-{}",
            &hash_text(&format!(
                "{}:{}:{}",
                input.scope_type, input.scope_ref, input.candidate_type
            ))[..16]
        );
        let summary = input.summary.to_string();
        MemoryCandidateDraft {
            candidate_id: candidate_id.clone(),
            candidate_ref: format!(
                "memory-candidate://tonglingyu/{}/{candidate_id}",
                input.trace_id
            ),
            journal_id,
            trace_id: input.trace_id.to_string(),
            user_session_id: input.context.user_session_id.clone(),
            interaction_context_id: input.context.interaction_context_id.clone(),
            context_pack_id: Some(input.context.context_pack_id.clone()),
            source_entry_type: "user_message".to_string(),
            scope_type: input.scope_type.to_string(),
            scope_ref: input.scope_ref.to_string(),
            candidate_type: input.candidate_type.to_string(),
            summary_sha256: hash_text(&summary),
            raw_excerpt_sha256: hash_text(&summary),
            raw_excerpt_redacted: summary.clone(),
            summary,
            sensitivity: "low".to_string(),
            risk_flags: json!([]),
            llm_extraction: json!({
                "schema_version": "tonglingyu-memory-extraction-v1",
                "policy_version": MEMORY_COLLECTOR_POLICY_VERSION,
                "extractor": "test_fixture",
                "hard_deny_filter_passed": true,
                "redaction_applied": false,
                "confidence": input.confidence,
                "llm_participation": llm_boundary_contract_json(),
                "input_digest": {
                    "summary_sha256": hash_text(input.summary),
                },
            }),
            confidence: input.confidence,
            audit_ref: memory_audit_ref("candidate-create", &candidate_id),
        }
    }

    #[test]
    fn conversation_state_summary_projects_only_to_main_profile() {
        let conn = conn();
        let first_messages = [ContextMessage {
            role: "user".to_string(),
            content: "晴雯判词在哪里？".to_string(),
        }];
        let first_context = create_context_for_request_with_mode(
            &conn,
            ContextRequestInput {
                trace_id: "trace-conversation-state-first",
                model_id: "tonglingyu",
                external_user_ref: "user-conversation-state",
                external_session_id: "session-conversation-state",
                external_message_id: "message-first",
                question: "晴雯判词在哪里？",
                messages: &first_messages,
                history_over_limit: false,
                max_messages: 20,
            },
            LlmMode::Disabled,
        )
        .expect("first context");
        let response = json!({
            "choices": [{
                "message": {
                    "content": "上一轮只确认晴雯判词位置，未断言结局。"
                }
            }]
        });
        append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: "trace-conversation-state-first",
                user_session_id: &first_context.user_session_id,
                interaction_context_id: &first_context.interaction_context_id,
                context_pack_id: &first_context.context_pack_id,
                external_message_id: "message-first",
                package_id: Some("pkg-conversation-state-boundary"),
                response: &response,
            },
        )
        .expect("final response");

        let follow_messages = [
            ContextMessage {
                role: "user".to_string(),
                content: "晴雯判词在哪里？".to_string(),
            },
            ContextMessage {
                role: "assistant".to_string(),
                content: "上一轮只确认晴雯判词位置，未断言结局。".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "她的结局呢？".to_string(),
            },
        ];
        let follow_context = create_context_for_request_with_mode(
            &conn,
            ContextRequestInput {
                trace_id: "trace-conversation-state-follow",
                model_id: "tonglingyu",
                external_user_ref: "user-conversation-state",
                external_session_id: "session-conversation-state",
                external_message_id: "message-follow",
                question: "她的结局呢？",
                messages: &follow_messages,
                history_over_limit: false,
                max_messages: 20,
            },
            LlmMode::Enforced,
        )
        .expect("follow context");

        assert!(
            !follow_context
                .used_context_refs
                .iter()
                .any(|item| item == "conversation_state_summary")
        );
        assert!(
            follow_context
                .context_pack
                .get("conversation_state_summary")
                .is_none()
        );
        assert_eq!(
            follow_context.context_pack["conversation_state_summary_projection_visible"],
            json!(true)
        );

        let main_projection = follow_context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-main")
            .expect("main projection");
        let state_summary = &main_projection.projection_payload["conversation_state_summary"];
        assert_eq!(
            state_summary["object"],
            json!("tonglingyu.conversation_state_summary")
        );
        assert_eq!(state_summary["memory_allowed_as_evidence"], json!(false));
        assert!(
            state_summary["active_entities"]
                .as_array()
                .expect("active entities")
                .contains(&json!("晴雯"))
        );
        assert!(
            state_summary["last_answer_boundaries"]
                .as_array()
                .expect("boundaries")
                .iter()
                .any(|item| item
                    .as_str()
                    .is_some_and(|value| value.contains("上一轮只确认晴雯判词位置")))
        );
        for projection in &follow_context.context_projections {
            if projection.consumer_name != "honglou-main" {
                assert!(
                    projection.projection_payload["conversation_state_summary"].is_null(),
                    "{} should not receive conversation summary",
                    projection.consumer_name
                );
            }
        }

        let journal_metadata: String = conn
            .query_row(
                "SELECT metadata_json FROM session_journal
                 WHERE trace_id = ?1 AND entry_type = 'conversation_state_summary_written'",
                params!["trace-conversation-state-follow"],
                |row| row.get(0),
            )
            .expect("conversation state journal");
        let metadata: Value = serde_json::from_str(&journal_metadata).expect("metadata json");
        assert_eq!(metadata["status"], json!("accepted"));
        assert_eq!(metadata["projection_visible"], json!(true));
    }

    #[test]
    fn public_response_boundary_extracts_short_controlled_boundary() {
        let full_answer = "尤三姐在可追溯文本中主要以刚烈、有主见的人物形象出现，她与柳湘莲相关情节最能体现其性格。她不是泛泛的陪衬角色，而是带有强烈自尊和决断的人物。但需要注意：这只是基于当前可追溯文本的回答，不能自动扩展到所有版本或续书设定。";
        let response = json!({
            "choices": [{
                "message": {
                    "content": full_answer
                }
            }]
        });

        let boundary = public_response_boundary(&response).expect("boundary");

        assert!(boundary.chars().count() <= CONVERSATION_STATE_BOUNDARY_MAX_CHARS);
        assert_ne!(boundary, full_answer);
        assert!(boundary.contains("尤三姐"));
        assert!(boundary.contains("可追溯文本"));
        assert!(boundary.contains("不能自动扩展"));
    }

    #[test]
    fn public_response_boundary_preserves_existing_boundary_statement() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": "上一轮只确认晴雯判词位置，未断言结局。"
                }
            }]
        });

        let boundary = public_response_boundary(&response).expect("boundary");

        assert_eq!(boundary, "上一轮只确认晴雯判词位置，未断言结局。");
        assert!(boundary.chars().count() <= CONVERSATION_STATE_BOUNDARY_MAX_CHARS);
    }

    fn insert_and_apply_test_memory(conn: &Connection, draft: &MemoryCandidateDraft) {
        assert!(
            insert_memory_candidate(conn, draft, "test-admin").expect("insert memory candidate"),
            "test candidate should be inserted"
        );
        apply_scoped_memory_policy_for_candidate(conn, draft, "test-admin")
            .expect("apply scoped memory policy");
    }

    fn manually_enable_test_memory(conn: &Connection, candidate_id: &str) {
        transition_memory_candidate(
            conn,
            MemoryCandidateTransitionInput {
                candidate_id,
                action: "approve",
                actor: "test-admin",
                reason: Some("manual approve shared scoped memory"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("manual approve");
        transition_memory_candidate(
            conn,
            MemoryCandidateTransitionInput {
                candidate_id,
                action: "promote",
                actor: "test-admin",
                reason: Some("manual promote shared scoped memory"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("manual promote");
        let card_id = conn
            .query_row(
                "SELECT memory_card_id FROM memory_cards WHERE source_candidate_id = ?1",
                params![candidate_id],
                |row| row.get::<_, String>(0),
            )
            .expect("memory card id");
        transition_memory_card(
            conn,
            MemoryCardTransitionInput {
                memory_card_id: &card_id,
                action: "enable_read",
                actor: "test-admin",
                reason: Some("manual enable source collection scoped memory"),
            },
        )
        .expect("manual enable read");
    }

    #[test]
    fn resolver_binds_recent_referent() {
        let messages = vec![
            ContextMessage {
                role: "user".to_string(),
                content: "介绍尤三姐".to_string(),
            },
            ContextMessage {
                role: "assistant".to_string(),
                content: "尤三姐是重要人物。".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "她最后怎么样？".to_string(),
            },
        ];

        let resolved = resolve_question("她最后怎么样？", &messages, None, None).expect("resolves");

        assert_eq!(resolved.resolved_question, "尤三姐最后怎么样？");
        assert!(!resolved.needs_clarification);
        assert!(resolved.confidence >= 0.75);
    }

    #[test]
    fn resolver_binds_continue_only_turn_to_prior_user_question() {
        let messages = vec![
            ContextMessage {
                role: "user".to_string(),
                content: "通灵宝玉丢了几次".to_string(),
            },
            ContextMessage {
                role: "assistant".to_string(),
                content: "如果你愿意，我可以继续梳理。".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            },
        ];

        let resolved = resolve_question("继续", &messages, None, None).expect("resolves");

        assert_eq!(resolved.resolved_question, "通灵宝玉丢了几次");
        assert!(!resolved.needs_clarification);
        assert_eq!(resolved.used_context_refs, vec!["session_history"]);
    }

    #[test]
    fn unresolved_continue_only_turn_fails_closed() {
        let resolved = resolve_question("继续", &[], None, None).expect("resolves");

        assert!(resolved.needs_clarification);
        assert_eq!(
            resolved.unsupported_reason.as_deref(),
            Some("unresolved_continuation")
        );
    }

    #[test]
    fn unresolved_referent_fails_closed() {
        let resolved = resolve_question("她最后怎么样？", &[], None, None).expect("resolves");

        assert!(resolved.needs_clarification);
        assert!(resolved.confidence < 0.45);
        assert_eq!(
            resolved.unsupported_reason.as_deref(),
            Some("unresolved_referent")
        );
    }

    #[tokio::test]
    async fn enforced_question_agent_rewrites_only_after_validator_accepts() {
        let db_path = temp_context_db_path("question-agent-accept");
        let conn = file_conn(&db_path);
        drop(conn);
        let runtime = FakeRuntimeClient::new(vec![json!({
            "schema_version": RESOLVER_SCHEMA_VERSION,
            "resolved_question": "晴雯判词如何暗示结局？",
            "referent_bindings": ["晴雯"],
            "used_context_refs": ["current_question", "session_summary"],
            "confidence": 0.91,
            "needs_clarification": false,
            "clarification_question": null,
            "unsupported_reason": null
        })]);
        let messages = vec![
            ContextMessage {
                role: "user".to_string(),
                content: "介绍晴雯".to_string(),
            },
            ContextMessage {
                role: "assistant".to_string(),
                content: "晴雯是重要人物。".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "她最后怎么样？".to_string(),
            },
        ];

        let context = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-question-agent-accept",
                model_id: "tonglingyu",
                external_user_ref: "user-question-agent-accept",
                external_session_id: "session-question-agent-accept",
                external_message_id: "message-question-agent-accept",
                question: "她最后怎么样？",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Enforced,
            LlmMode::Disabled,
        )
        .await
        .expect("context created");

        assert_eq!(context.resolved_question, "晴雯判词如何暗示结局？");
        assert_eq!(
            context.context_pack["resolver"]["strategy"],
            json!("llm_agent_enforced")
        );
        assert_eq!(context.context_pack["resolver"]["llm_used"], json!(true));
        assert_eq!(
            context.context_pack["resolver"]["agent_decision"]["raw_output_embedded"],
            json!(false)
        );
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn elliptical_followup_preserves_prior_question_task_boundary() {
        let db_path = temp_context_db_path("question-agent-ellipsis");
        let conn = file_conn(&db_path);
        drop(conn);
        let runtime = FakeRuntimeClient::new(Vec::new());
        let messages = vec![
            ContextMessage {
                role: "user".to_string(),
                content: "史湘云的结局".to_string(),
            },
            ContextMessage {
                role: "assistant".to_string(),
                content: "目前先按前八十回和脂批边界回答。".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "脂批中的证据呢".to_string(),
            },
        ];

        let context = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-question-agent-ellipsis",
                model_id: "tonglingyu",
                external_user_ref: "user-question-agent-ellipsis",
                external_session_id: "session-question-agent-ellipsis",
                external_message_id: "message-question-agent-ellipsis",
                question: "脂批中的证据呢",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Enforced,
            LlmMode::Disabled,
        )
        .await
        .expect("context created");

        assert_eq!(
            context.resolved_question,
            "关于史湘云的结局，脂批中的证据呢"
        );
        assert_eq!(
            context.context_pack["resolver"]["strategy"],
            json!("deterministic_elliptical_followup")
        );
        assert_eq!(context.context_pack["resolver"]["llm_used"], json!(false));
        assert_eq!(
            context.context_pack["policy_versions"]["context_rules"]["subject_ontology"],
            json!("2026-05-24.1")
        );
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn elliptical_followup_uses_session_journal_prior_question_when_history_is_absent() {
        let db_path = temp_context_db_path("question-agent-ellipsis-journal");
        let conn = file_conn(&db_path);
        drop(conn);
        let runtime = FakeRuntimeClient::new(Vec::new());
        let first_messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "史湘云的结局".to_string(),
        }];
        let second_messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "脂批中的证据呢".to_string(),
        }];

        create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-question-agent-ellipsis-journal-first",
                model_id: "tonglingyu",
                external_user_ref: "user-question-agent-ellipsis-journal",
                external_session_id: "session-question-agent-ellipsis-journal",
                external_message_id: "message-question-agent-ellipsis-journal-first",
                question: "史湘云的结局",
                messages: &first_messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Enforced,
            LlmMode::Disabled,
        )
        .await
        .expect("first context created");

        let context = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-question-agent-ellipsis-journal-second",
                model_id: "tonglingyu",
                external_user_ref: "user-question-agent-ellipsis-journal",
                external_session_id: "session-question-agent-ellipsis-journal",
                external_message_id: "message-question-agent-ellipsis-journal-second",
                question: "脂批中的证据呢",
                messages: &second_messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Enforced,
            LlmMode::Disabled,
        )
        .await
        .expect("second context created");

        assert_eq!(
            context.resolved_question,
            "关于史湘云的结局，脂批中的证据呢"
        );
        assert_eq!(
            context.context_pack["resolver"]["used_context_refs"],
            json!(["session_journal_prior_question"])
        );
        assert_eq!(
            context.context_pack["resolver"]["strategy"],
            json!("deterministic_elliptical_followup")
        );
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn forbidden_question_agent_output_fails_closed_when_enforced() {
        let db_path = temp_context_db_path("question-agent-reject");
        let conn = file_conn(&db_path);
        drop(conn);
        let invalid = json!({
            "schema_version": RESOLVER_SCHEMA_VERSION,
            "resolved_question": "晴雯判词如何暗示结局？",
            "referent_bindings": ["晴雯"],
            "used_context_refs": ["current_question", "session_summary"],
            "confidence": 0.91,
            "needs_clarification": false,
            "clarification_question": null,
            "unsupported_reason": null,
            "allowed_tools": ["tonglingyu.memory.write"]
        });
        let runtime = FakeRuntimeClient::new(vec![invalid.clone(), invalid]);
        let messages = vec![
            ContextMessage {
                role: "user".to_string(),
                content: "介绍晴雯".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "她最后怎么样？".to_string(),
            },
        ];

        let result = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-question-agent-reject",
                model_id: "tonglingyu",
                external_user_ref: "user-question-agent-reject",
                external_session_id: "session-question-agent-reject",
                external_message_id: "message-question-agent-reject",
                question: "她最后怎么样？",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Enforced,
            LlmMode::Disabled,
        )
        .await;

        let error = result.expect_err("enforced invalid question agent should fail closed");
        let error_text = format!("{error:#}");
        assert!(error_text.contains("llm_agent_enforced_rejected"));
        assert!(error_text.contains(QUESTION_NORMALIZER_PROFILE_ID));
        assert!(error_text.contains("contract_accepted=false"));
        assert_eq!(runtime.profile_inputs().len(), 2);
        let persisted_conn = Connection::open(&db_path).expect("open persisted context db");
        let context_pack_count: i64 = persisted_conn
            .query_row("SELECT COUNT(*) FROM context_packs", [], |row| row.get(0))
            .expect("context pack count");
        assert_eq!(context_pack_count, 0);
        drop(persisted_conn);
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn enforced_conversation_state_agent_projects_only_validated_summary() {
        let db_path = temp_context_db_path("conversation-state-agent");
        let conn = file_conn(&db_path);
        drop(conn);
        let runtime = FakeRuntimeClient::new(vec![json!({
            "object": crate::conversation_state::CONVERSATION_STATE_SUMMARY_OBJECT,
            "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
            "current_topic": "晴雯相关问题",
            "active_entities": ["晴雯"],
            "open_questions": ["晴雯后来怎么样？"],
            "last_answer_boundaries": [],
            "evidence_package_refs": [],
            "reviewer_warnings": [],
            "memory_allowed_as_evidence": false,
            "summary_confidence": 0.9
        })]);
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "晴雯后来怎么样？".to_string(),
        }];

        let context = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-conversation-state-agent",
                model_id: "tonglingyu",
                external_user_ref: "user-conversation-state-agent",
                external_session_id: "session-conversation-state-agent",
                external_message_id: "message-conversation-state-agent",
                question: "晴雯后来怎么样？",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Disabled,
            LlmMode::Enforced,
        )
        .await
        .expect("context created");

        assert_eq!(
            context.context_pack["llm_agent_context_path"]["conversation_state_summary_source"],
            json!("llm_agent_validated")
        );
        assert_eq!(
            context.context_pack["llm_agent_context_path"]["conversation_state_agent"]["accepted_for_projection"],
            json!(true)
        );
        let persisted_conn = Connection::open(&db_path).expect("open persisted context db");
        let persisted_trace = load_trace_context(&persisted_conn, "trace-conversation-state-agent")
            .expect("persisted trace context");
        assert_eq!(
            persisted_trace["context_packs"][0]["llm_agent_context_path"]["conversation_state_summary_source"],
            json!("llm_agent_validated")
        );
        assert_eq!(
            persisted_trace["context_packs"][0]["llm_agent_context_path"]["conversation_state_agent"]
                ["accepted_for_projection"],
            json!(true)
        );
        drop(persisted_conn);
        let main_projection = context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-main")
            .expect("main projection");
        assert_eq!(
            main_projection.projection_payload["conversation_state_summary"]["active_entities"],
            json!(["晴雯"])
        );
        for projection in &context.context_projections {
            if projection.consumer_name != "honglou-main" {
                assert!(
                    projection.projection_payload["conversation_state_summary"].is_null(),
                    "{} should not receive conversation state",
                    projection.consumer_name
                );
            }
        }
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn context_pack_records_provider_features_without_raw_agent_output() {
        let db_path = temp_context_db_path("provider-features-context");
        let conn = file_conn(&db_path);
        drop(conn);
        let summary = json!({
            "object": crate::conversation_state::CONVERSATION_STATE_SUMMARY_OBJECT,
            "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
            "current_topic": "晴雯相关问题",
            "active_entities": ["晴雯"],
            "open_questions": ["晴雯后来怎么样？"],
            "last_answer_boundaries": [],
            "evidence_package_refs": [],
            "reviewer_warnings": [],
            "memory_allowed_as_evidence": false,
            "summary_confidence": 0.9
        });
        let runtime = FakeRuntimeClient::with_outputs(vec![FakeRuntimeOutput {
            result_summary: summary.to_string(),
            result_ref: Some(format!(
                "openai-compatible-network://profiles/{CONVERSATION_STATE_WRITER_PROFILE_ID}/trace-provider-features-context"
            )),
            metadata: {
                let provider_request = json!({
                    "schema_version": "openai-compatible-provider-request-v1",
                    "runtime_adapter": "openai-compatible-network",
                    "profile_id": CONVERSATION_STATE_WRITER_PROFILE_ID,
                    "model": "provider-request-test-model",
                    "messages": [
                        {
                            "role": "system",
                            "content": "Tonglingyu conversation-state writer system prompt"
                        },
                        {
                            "role": "user",
                            "content": "晴雯后来怎么样？"
                        }
                    ],
                    "message_count": 2,
                    "stream": false,
                    "authorization_header_embedded": false,
                    "api_key_embedded": false,
                    "secret_values_printed": false
                });
                let mut metadata = provider_output_metadata(json!({
                "schema_version": "openai-compatible-provider-output-v1",
                "response_format_json_requested": true,
                "content_present": true,
                "content_sha256": "sha256:raw",
                "content_contains_think_blocks": true,
                "content_without_think_sha256": "sha256:clean",
                "reasoning_details_present": true,
                "reasoning_details_sha256": "sha256:reasoning",
                "business_json_candidate_present": true,
                "business_json_candidate_sha256": "sha256:candidate",
                "business_json_candidate_source": "embedded_json_object",
                "business_json_candidate": summary.to_string(),
                "validator_content_sha256": "sha256:candidate",
                "preserved_raw_fields": {
                    "content": "<think>{\"not\":\"context\"}</think>",
                    "reasoning_details": {"items": [{"text": "internal reasoning"}]}
                }
                }));
                metadata["provider_request_sha256"] =
                    json!(format!("sha256:{}", digest_json(&provider_request)));
                metadata["provider_request_embedded"] = json!(true);
                metadata["provider_request"] = provider_request;
                metadata
            },
        }]);
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "晴雯后来怎么样？".to_string(),
        }];

        let context = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-provider-features-context",
                model_id: "tonglingyu",
                external_user_ref: "user-provider-features-context",
                external_session_id: "session-provider-features-context",
                external_message_id: "message-provider-features-context",
                question: "晴雯后来怎么样？",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Disabled,
            LlmMode::Enforced,
        )
        .await
        .expect("context created");

        let agent_audit =
            &context.context_pack["llm_agent_context_path"]["conversation_state_agent"];
        assert_eq!(agent_audit["accepted_for_projection"], json!(true));
        assert_eq!(
            agent_audit["provider_output_features"]["content_contains_think_blocks"],
            json!(true)
        );
        assert_eq!(
            agent_audit["provider_output_features"]["reasoning_details_present"],
            json!(true)
        );
        assert_eq!(
            agent_audit["provider_output_features"]["raw_provider_fields_embedded_in_validator_audit"],
            json!(false)
        );
        assert_eq!(
            agent_audit["provider_request"]["provider_request_embedded"],
            json!(true)
        );
        assert_eq!(
            agent_audit["provider_request"]["provider_request"]["messages"][0]["role"],
            json!("system")
        );
        assert_eq!(
            agent_audit["provider_request"]["provider_request"]["messages"][0]["content"],
            json!("Tonglingyu conversation-state writer system prompt")
        );
        assert_eq!(
            agent_audit["provider_request"]["provider_request"]["authorization_header_embedded"],
            json!(false)
        );
        assert_eq!(
            agent_audit["provider_request"]["provider_request"]["api_key_embedded"],
            json!(false)
        );
        let context_text = context.context_pack["llm_agent_context_path"].to_string();
        assert!(!context_text.contains("<think>"));
        assert!(!context_text.contains("internal reasoning"));
        assert_eq!(
            context.context_pack["llm_agent_context_path"]["raw_agent_output_embedded"],
            json!(false)
        );
        let persisted_conn = Connection::open(&db_path).expect("open persisted context db");
        let persisted_raw_context_path_json: String = persisted_conn
            .query_row(
                "SELECT llm_agent_context_path_json FROM context_packs WHERE trace_id = ?1",
                params!["trace-provider-features-context"],
                |row| row.get(0),
            )
            .expect("raw context path persisted");
        let persisted_raw_context_path: Value =
            serde_json::from_str(&persisted_raw_context_path_json).expect("raw context path json");
        assert_eq!(
            persisted_raw_context_path["conversation_state_agent"]["provider_request"]["provider_request"]
                ["messages"][0]["content"],
            json!("Tonglingyu conversation-state writer system prompt")
        );
        let persisted_trace =
            load_trace_context(&persisted_conn, "trace-provider-features-context")
                .expect("persisted trace context");
        let persisted_message = &persisted_trace["context_packs"][0]["llm_agent_context_path"]["conversation_state_agent"]
            ["provider_request"]["provider_request"]["messages"][0];
        assert_eq!(persisted_message["role"], json!("system"));
        assert!(persisted_message.get("content").is_none());
        assert_eq!(persisted_message["raw_content_redacted"], json!(true));
        assert_eq!(persisted_message["raw_content_value_type"], json!("string"));
        assert!(
            persisted_message["raw_content_sha256"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        drop(persisted_conn);
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn rejected_conversation_state_agent_fails_closed_when_enforced() {
        let db_path = temp_context_db_path("conversation-state-agent-reject");
        let conn = file_conn(&db_path);
        drop(conn);
        let invalid = json!({
            "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
            "current_question": "晴雯后来怎么样？",
            "session_summary": "最近讨论对象：晴雯",
            "required_active_entities": ["晴雯"]
        });
        let runtime = FakeRuntimeClient::new(vec![invalid.clone(), invalid]);
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "晴雯后来怎么样？".to_string(),
        }];

        let result = create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-conversation-state-agent-reject",
                model_id: "tonglingyu",
                external_user_ref: "user-conversation-state-agent-reject",
                external_session_id: "session-conversation-state-agent-reject",
                external_message_id: "message-conversation-state-agent-reject",
                question: "晴雯后来怎么样？",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Disabled,
            LlmMode::Enforced,
        )
        .await;

        let error = result.expect_err("enforced invalid conversation state should fail closed");
        let error_text = format!("{error:#}");
        assert!(error_text.contains("llm_agent_enforced_rejected"));
        assert!(error_text.contains(CONVERSATION_STATE_WRITER_PROFILE_ID));
        assert!(error_text.contains("contract_accepted=false"));
        assert_eq!(runtime.profile_inputs().len(), 2);
        let persisted_conn = Connection::open(&db_path).expect("open persisted context db");
        let context_pack_count: i64 = persisted_conn
            .query_row("SELECT COUNT(*) FROM context_packs", [], |row| row.get(0))
            .expect("context pack count");
        assert_eq!(context_pack_count, 0);
        drop(persisted_conn);
        remove_file_db(&db_path);
    }

    #[tokio::test]
    async fn llm_agent_runtime_receives_role_specific_provider_prompts() {
        let db_path = temp_context_db_path("llm-agent-provider-prompts");
        let conn = file_conn(&db_path);
        drop(conn);
        let runtime = FakeRuntimeClient::new(vec![
            json!({
                "schema_version": RESOLVER_SCHEMA_VERSION,
                "resolved_question": "晴雯后来怎么样？",
                "referent_bindings": ["晴雯"],
                "used_context_refs": ["current_question", "session_summary"],
                "confidence": 0.91,
                "needs_clarification": false,
                "clarification_question": null,
                "unsupported_reason": null
            }),
            json!({
                "object": crate::conversation_state::CONVERSATION_STATE_SUMMARY_OBJECT,
                "schema_version": CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION,
                "current_topic": "晴雯相关问题",
                "active_entities": ["晴雯"],
                "open_questions": ["晴雯后来怎么样？"],
                "last_answer_boundaries": [],
                "evidence_package_refs": [],
                "reviewer_warnings": [],
                "memory_allowed_as_evidence": false,
                "summary_confidence": 0.9
            }),
        ]);
        let messages = vec![
            ContextMessage {
                role: "user".to_string(),
                content: "介绍晴雯".to_string(),
            },
            ContextMessage {
                role: "user".to_string(),
                content: "她后来怎么样？".to_string(),
            },
        ];

        create_context_for_request_with_agent_runtime_and_modes(
            &db_path,
            ContextRequestInput {
                trace_id: "trace-provider-prompt",
                model_id: "tonglingyu",
                external_user_ref: "user-provider-prompt",
                external_session_id: "session-provider-prompt",
                external_message_id: "message-provider-prompt",
                question: "她后来怎么样？",
                messages: &messages,
                history_over_limit: false,
                max_messages: 20,
            },
            &runtime,
            LlmMode::Enforced,
            LlmMode::Enforced,
        )
        .await
        .expect("context created");

        let inputs = runtime.profile_inputs();
        assert_eq!(inputs.len(), 2);
        let question_input = &inputs[0];
        assert_eq!(question_input.profile_id, QUESTION_NORMALIZER_PROFILE_ID);
        assert!(
            question_input.messages[0]
                .content
                .contains("Role: question_normalizer")
        );
        assert!(
            question_input.messages[0]
                .content
                .contains("Do not answer it")
        );
        let question_payload: Value =
            serde_json::from_str(&question_input.messages[1].content).expect("question payload");
        assert_eq!(
            question_payload["task"]["role"],
            json!("question_normalizer")
        );
        assert!(question_payload.get("agent_request").is_none());
        assert!(question_payload.get("structured_payload").is_none());
        assert!(
            !question_input.messages[1]
                .content
                .contains("trace-provider-prompt")
        );

        let state_input = &inputs[1];
        assert_eq!(state_input.profile_id, CONVERSATION_STATE_WRITER_PROFILE_ID);
        assert!(
            state_input.messages[0]
                .content
                .contains("Role: conversation_state_writer")
        );
        assert!(
            state_input.messages[0]
                .content
                .contains("Context-only fields")
        );
        let state_payload: Value =
            serde_json::from_str(&state_input.messages[1].content).expect("state payload");
        assert_eq!(
            state_payload["task"]["role"],
            json!("conversation_state_writer")
        );
        assert!(state_payload.get("agent_request").is_none());
        assert!(state_payload.get("structured_payload").is_none());
        assert!(
            state_payload["input_context"]
                .get("session_summary")
                .is_none()
        );
        assert!(
            state_payload["output_contract"]["forbidden_output_fields"]
                .as_array()
                .expect("forbidden fields")
                .iter()
                .any(|field| field.as_str() == Some("session_summary"))
        );
        assert!(
            !state_input.messages[1]
                .content
                .contains("trace-provider-prompt")
        );

        remove_file_db(&db_path);
    }

    #[test]
    fn trace_context_does_not_expose_raw_journal_content() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "介绍尤三姐".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-test",
                model_id: "tonglingyu",
                external_user_ref: "user-1",
                external_session_id: "chat-1",
                external_message_id: "message-1",
                question: "介绍尤三姐",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");

        assert!(context.user_session_id.starts_with("user-session-"));
        assert_eq!(context.context_projections.len(), 4);
        let main_projection = context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-main")
            .expect("main projection");
        assert!(
            main_projection
                .allowed_tools
                .contains(&"tonglingyu.evidence.package.create".to_string())
        );
        assert_eq!(main_projection.consumer_type, "runtime_profile");
        assert_eq!(
            main_projection.runtime_adapter,
            "tonglingyu-runtime-adapter-v1"
        );
        assert!(
            main_projection
                .context_projection_ref
                .starts_with("context-projection://tonglingyu/trace-test/")
        );
        let trace = load_trace_context(&conn, "trace-test").expect("trace context");
        let rendered = serde_json::to_string(&trace).expect("trace json");

        assert!(!rendered.contains("\"content\":\"介绍尤三姐\""));
        assert!(!rendered.contains("\"projection_payload\":"));
        assert!(rendered.contains("raw_content_included"));
        assert_eq!(
            trace["context_projections"]
                .as_array()
                .expect("trace projections")
                .len(),
            4
        );
        assert!(
            trace["context_projections"]
                .as_array()
                .expect("trace projections")
                .iter()
                .all(|projection| projection["projection_payload_summary"].is_object())
        );
    }

    #[test]
    fn memory_collector_auto_enables_policy_approved_memory_and_context_reads_it() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "我喜欢回答里多引用原文。".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-candidate",
                model_id: "tonglingyu",
                external_user_ref: "memory-user",
                external_session_id: "memory-chat",
                external_message_id: "memory-message-1",
                question: "以后回答时，请用简体中文短句总结。",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");
        append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: "trace-memory-candidate",
                user_session_id: &context.user_session_id,
                interaction_context_id: &context.interaction_context_id,
                context_pack_id: &context.context_pack_id,
                external_message_id: "memory-message-1",
                package_id: Some("pkg-memory-candidate"),
                response: &json!({"status": "ok"}),
            },
        )
        .expect("final response journal");

        let report = run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "admin_manual",
                actor: "test-admin",
                limit: 20,
                dry_run: false,
                trace_id: None,
            },
        )
        .expect("memory collector run");

        assert_eq!(report["status"], json!("ok"));
        assert_eq!(report["candidate_count"], json!(1));
        assert_eq!(report["auto_enabled_count"], json!(1));
        let candidates = list_memory_candidates(
            &conn,
            MemoryCandidateListInput {
                status: Some("approved"),
                scope_type: None,
                scope_ref: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("candidate list");
        let candidate = candidates["items"][0].clone();
        assert_eq!(candidate["candidate_type"], json!("language_preference"));
        assert_eq!(candidate["status"], json!("approved"));
        assert_eq!(candidate["source_entry_type"], json!("user_message"));
        assert_eq!(candidate["scope_type"], json!("user_private"));
        assert!(
            candidate["scope_ref"]
                .as_str()
                .expect("scope ref")
                .starts_with("user_private:sha256:")
        );
        assert_eq!(
            candidate["llm_extraction"]["llm_participation"]["allowed"],
            json!(true)
        );
        assert_eq!(
            candidate["llm_extraction"]["llm_participation"]["used"],
            json!(false)
        );

        let cards = list_memory_cards(
            &conn,
            MemoryCardListInput {
                status: Some("active"),
                scope_type: None,
                scope_ref: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("memory card list");
        let card = cards["items"][0].clone();
        assert_eq!(card["status"], json!("active"));
        assert_eq!(card["read_enabled"], json!(true));
        assert_eq!(
            card["acl"]["policy_version"],
            json!(SCOPED_MEMORY_POLICY_VERSION)
        );
        let next_context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-candidate-followup",
                model_id: "tonglingyu",
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
        .expect("context reads enabled memory");
        let refs = next_context.context_pack["memory_read_refs"]
            .as_array()
            .expect("memory read refs");
        assert_eq!(refs.len(), 1);
        assert!(
            next_context.context_pack["memory_read_ref_digest"]
                .as_str()
                .is_some_and(|digest| digest.len() == 64)
        );
        let main_projection = next_context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-main")
            .expect("main projection");
        assert_eq!(
            main_projection.projection_payload["memory_read_ref_digest"],
            next_context.context_pack["memory_read_ref_digest"]
        );
        let main_profile_view = next_context.context_pack["profile_views"]
            .as_array()
            .expect("profile views")
            .iter()
            .find(|view| view["profile_name"] == json!("honglou-main"))
            .expect("main profile view");
        assert_eq!(
            main_profile_view["memory_read_ref_digest"],
            main_projection.projection_payload["memory_read_ref_digest"]
        );
        assert_eq!(
            main_projection.projection_payload["memory_summaries"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        let reviewer_projection = next_context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-reviewer")
            .expect("reviewer projection");
        assert!(
            reviewer_projection.projection_payload["memory_read_ref_digest"]
                .as_str()
                .is_some_and(|digest| digest.len() == 64)
        );
        assert_eq!(
            reviewer_projection.projection_payload["memory_summaries"]
                .as_array()
                .map(Vec::len),
            Some(0)
        );
        let persisted_trace =
            load_trace_context(&conn, "trace-memory-candidate-followup").expect("trace context");
        assert_eq!(
            persisted_trace["context_packs"][0]["memory_read_refs"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            persisted_trace["context_packs"][0]["memory_read_ref_digest"],
            next_context.context_pack["memory_read_ref_digest"]
        );
        assert_eq!(
            persisted_trace["context_packs"][0]["forbidden_tools"],
            json!([])
        );

        transition_memory_card(
            &conn,
            MemoryCardTransitionInput {
                memory_card_id: card["memory_card_id"].as_str().expect("memory card id"),
                action: "revoke",
                actor: "test-admin",
                reason: Some("revoke in test"),
            },
        )
        .expect("revoke memory card");
        let after_revoke = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-candidate-after-revoke",
                model_id: "tonglingyu",
                external_user_ref: "memory-user",
                external_session_id: "memory-chat",
                external_message_id: "memory-message-3",
                question: "介绍林黛玉",
                messages: &[ContextMessage {
                    role: "user".to_string(),
                    content: "介绍林黛玉".to_string(),
                }],
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created after revoke");
        assert_eq!(after_revoke.context_pack["memory_read_refs"], json!([]));
        let audit_count = table_count(&conn, "memory_transition_audit").expect("audit count");
        assert!(audit_count >= 4, "audit_count={audit_count}");
    }

    #[test]
    fn scoped_memory_policy_suppresses_non_user_message_candidates() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "以后回答时，请用简体中文短句总结。".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-non-user-source",
                model_id: "tonglingyu",
                external_user_ref: "memory-non-user-source",
                external_session_id: "memory-non-user-source-chat",
                external_message_id: "memory-non-user-source-message",
                question: "以后回答时，请用简体中文短句总结。",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");
        let mut draft = test_memory_draft(
            &conn,
            TestMemoryDraftInput {
                trace_id: "trace-memory-non-user-source",
                context: &context,
                scope_type: "user_private",
                scope_ref: &user_private_scope_ref("memory-non-user-source"),
                candidate_type: "language_preference",
                summary: "non user source should not become memory",
                confidence: 0.99,
            },
        );
        draft.candidate_id = "memory-candidate-non-user-source".to_string();
        draft.candidate_ref =
            "memory-candidate://tonglingyu/trace-memory-non-user-source/non-user-source"
                .to_string();
        draft.source_entry_type = "final_response".to_string();

        assert!(
            insert_memory_candidate(&conn, &draft, "test-admin")
                .expect("insert non-user-source candidate")
        );
        let result = apply_scoped_memory_policy_for_candidate(&conn, &draft, "test-admin")
            .expect("apply scoped memory policy");

        assert!(!result.auto_read_enabled);
        assert_eq!(result.public_summary["decision"], json!("suppress"));
        assert_eq!(
            result.public_summary["decision_reason"],
            json!("source_entry_type_not_allowed")
        );
        let candidate = read_memory_candidate(&conn, &draft.candidate_id)
            .expect("read candidate")
            .expect("candidate exists");
        assert_eq!(candidate["status"], json!("rejected"));
        assert_eq!(table_count(&conn, "memory_cards").expect("card count"), 0);

        let (decision, decision_reason, rule_filter_json, llm_filter_json): (
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT decision, decision_reason, rule_filter_json, llm_filter_json
                 FROM memory_policy_decisions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("policy decision row");
        let rule_filter: Value = serde_json::from_str(&rule_filter_json).expect("rule filter json");
        let llm_filter: Value = serde_json::from_str(&llm_filter_json).expect("llm filter json");
        assert_eq!(decision, "suppress");
        assert_eq!(decision_reason, "source_entry_type_not_allowed");
        assert_eq!(rule_filter["source_entry_type_allowed"], json!(false));
        assert_eq!(rule_filter["suppress"], json!(true));
        assert!(
            llm_filter["exclusion_flags"]
                .as_array()
                .expect("exclusion flags")
                .contains(&json!("source_entry_type_not_allowed"))
        );

        let mut manual_bypass = test_memory_draft(
            &conn,
            TestMemoryDraftInput {
                trace_id: "trace-memory-non-user-source",
                context: &context,
                scope_type: "user_private",
                scope_ref: &user_private_scope_ref("memory-non-user-source"),
                candidate_type: "language_preference",
                summary: "non user source manual promote should fail",
                confidence: 0.99,
            },
        );
        manual_bypass.candidate_id = "memory-candidate-non-user-manual-bypass".to_string();
        manual_bypass.candidate_ref =
            "memory-candidate://tonglingyu/trace-memory-non-user-source/manual-bypass".to_string();
        manual_bypass.source_entry_type = "final_response".to_string();
        assert!(
            insert_memory_candidate(&conn, &manual_bypass, "test-admin")
                .expect("insert manual bypass candidate")
        );
        transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &manual_bypass.candidate_id,
                action: "approve",
                actor: "test-admin",
                reason: Some("manual approve should not bypass source boundary"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("manual approve non-user source");
        let err = transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &manual_bypass.candidate_id,
                action: "promote",
                actor: "test-admin",
                reason: Some("manual promote should fail"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect_err("manual promote cannot bypass source boundary");
        assert!(
            err.to_string()
                .contains("source_entry_type is not eligible"),
            "{err}"
        );
        assert_eq!(table_count(&conn, "memory_cards").expect("card count"), 0);
    }

    #[test]
    fn memory_collector_hard_denies_secrets_without_candidate() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "请记住 token=sk-test-secret-value".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-secret",
                model_id: "tonglingyu",
                external_user_ref: "memory-secret-user",
                external_session_id: "memory-secret-chat",
                external_message_id: "memory-secret-message",
                question: "请记住 token=sk-test-secret-value",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");
        append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: "trace-memory-secret",
                user_session_id: &context.user_session_id,
                interaction_context_id: &context.interaction_context_id,
                context_pack_id: &context.context_pack_id,
                external_message_id: "memory-secret-message",
                package_id: Some("pkg-memory-secret"),
                response: &json!({"status": "ok"}),
            },
        )
        .expect("final response journal");

        let report = run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "admin_manual",
                actor: "test-admin",
                limit: 20,
                dry_run: false,
                trace_id: None,
            },
        )
        .expect("memory collector run");

        assert_eq!(report["candidate_count"], json!(0));
        assert!(
            report["denied_count"].as_i64().unwrap_or_default() >= 1,
            "{report}"
        );
        let candidates = list_memory_candidates(
            &conn,
            MemoryCandidateListInput {
                status: None,
                scope_type: None,
                scope_ref: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("candidate list");
        assert_eq!(candidates["items"].as_array().unwrap().len(), 0);
        let audit = load_rows_json_for_test(
            &conn,
            "SELECT action, metadata_json FROM memory_transition_audit ORDER BY created_at",
        );
        let rendered = serde_json::to_string(&audit).expect("audit json");
        assert!(rendered.contains("collector_hard_deny"));
        assert!(rendered.contains("secret_or_token_denied"));
        assert!(!rendered.contains("sk-test-secret-value"));
    }

    #[test]
    fn memory_collector_skips_unfinished_trace_until_final_response() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "以后回答时，请用简体中文短句总结。".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-unfinished",
                model_id: "tonglingyu",
                external_user_ref: "memory-unfinished-user",
                external_session_id: "memory-unfinished-chat",
                external_message_id: "memory-unfinished-message",
                question: "以后回答时，请用简体中文短句总结。",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");
        let unfinished_report = run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "background_worker",
                actor: "test-worker",
                limit: 20,
                dry_run: false,
                trace_id: Some("trace-memory-unfinished"),
            },
        )
        .expect("collector run");
        assert_eq!(unfinished_report["processed_count"], json!(0));
        assert_eq!(unfinished_report["candidate_count"], json!(0));

        append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: "trace-memory-unfinished",
                user_session_id: &context.user_session_id,
                interaction_context_id: &context.interaction_context_id,
                context_pack_id: &context.context_pack_id,
                external_message_id: "memory-unfinished-message",
                package_id: Some("pkg-memory-unfinished"),
                response: &json!({"status": "ok"}),
            },
        )
        .expect("final response journal");
        let completed_report = run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "background_worker",
                actor: "test-worker",
                limit: 20,
                dry_run: false,
                trace_id: Some("trace-memory-unfinished"),
            },
        )
        .expect("collector run");
        assert_eq!(completed_report["processed_count"], json!(1));
        assert_eq!(completed_report["candidate_count"], json!(1));
    }

    #[test]
    fn memory_context_read_budget_truncates_and_audits() {
        let conn = conn();
        for index in 0..6 {
            let question = format!("以后回答时，请用简体中文短句总结。偏好编号{index}。");
            let messages = vec![ContextMessage {
                role: "user".to_string(),
                content: question.clone(),
            }];
            let context = create_context_for_request(
                &conn,
                ContextRequestInput {
                    trace_id: &format!("trace-memory-budget-{index}"),
                    model_id: "tonglingyu",
                    external_user_ref: "memory-budget-user",
                    external_session_id: "memory-budget-chat",
                    external_message_id: &format!("memory-budget-message-{index}"),
                    question: &question,
                    messages: &messages,
                    history_over_limit: false,
                    max_messages: 40,
                },
            )
            .expect("context created");
            append_final_response(
                &conn,
                FinalResponseJournalInput {
                    trace_id: &format!("trace-memory-budget-{index}"),
                    user_session_id: &context.user_session_id,
                    interaction_context_id: &context.interaction_context_id,
                    context_pack_id: &context.context_pack_id,
                    external_message_id: &format!("memory-budget-message-{index}"),
                    package_id: Some("pkg-memory-budget"),
                    response: &json!({"status": "ok"}),
                },
            )
            .expect("final response journal");
        }
        let report = run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "admin_manual",
                actor: "test-admin",
                limit: 20,
                dry_run: false,
                trace_id: None,
            },
        )
        .expect("collector run");
        assert_eq!(report["candidate_count"], json!(6));
        assert_eq!(report["auto_enabled_count"], json!(6));

        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-budget-read",
                model_id: "tonglingyu",
                external_user_ref: "memory-budget-user",
                external_session_id: "memory-budget-chat",
                external_message_id: "memory-budget-read-message",
                question: "介绍贾宝玉",
                messages: &[ContextMessage {
                    role: "user".to_string(),
                    content: "介绍贾宝玉".to_string(),
                }],
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context reads budgeted memory");
        assert_eq!(
            context.context_pack["memory_read_refs"]
                .as_array()
                .map(Vec::len),
            Some(MEMORY_READ_BUDGET_USER_PRIVATE)
        );
        assert_eq!(
            context.context_pack["memory_usage_summary"]["truncated_count"],
            json!(2)
        );
        let audit = load_rows_json_for_test(
            &conn,
            "SELECT entry_type, metadata_json FROM session_journal WHERE entry_type = 'memory_read_decision'",
        );
        let rendered = serde_json::to_string(&audit).expect("audit json");
        assert!(rendered.contains("truncated_count"));
        assert!(rendered.contains("scoped-memory-policy-v1"));
    }

    #[test]
    fn shared_scope_memory_reads_follow_policy_acl_and_context_scope() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "介绍林黛玉".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-shared-scope-seed",
                model_id: "tonglingyu",
                external_user_ref: "memory-shared-seed-user",
                external_session_id: "memory-shared-seed-chat",
                external_message_id: "memory-shared-seed-message",
                question: "介绍林黛玉",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");
        append_final_response(
            &conn,
            FinalResponseJournalInput {
                trace_id: "trace-memory-shared-scope-seed",
                user_session_id: &context.user_session_id,
                interaction_context_id: &context.interaction_context_id,
                context_pack_id: &context.context_pack_id,
                external_message_id: "memory-shared-seed-message",
                package_id: Some("pkg-memory-shared"),
                response: &json!({"status": "ok"}),
            },
        )
        .expect("final response journal");

        for draft in [
            test_memory_draft(
                &conn,
                TestMemoryDraftInput {
                    trace_id: "trace-memory-shared-scope-seed",
                    context: &context,
                    scope_type: "profile_common",
                    scope_ref: PROFILE_COMMON_SCOPE_REF,
                    candidate_type: "retrieval_preference",
                    summary: "profile common retrieval preference",
                    confidence: 0.96,
                },
            ),
            test_memory_draft(
                &conn,
                TestMemoryDraftInput {
                    trace_id: "trace-memory-shared-scope-seed",
                    context: &context,
                    scope_type: "knowledge_space",
                    scope_ref: KNOWLEDGE_SPACE_SCOPE_REF,
                    candidate_type: "retrieval_preference",
                    summary: "knowledge space retrieval preference",
                    confidence: 0.96,
                },
            ),
            test_memory_draft(
                &conn,
                TestMemoryDraftInput {
                    trace_id: "trace-memory-shared-scope-seed",
                    context: &context,
                    scope_type: "research_topic",
                    scope_ref: &format!("topic:{}", hash_text("林黛玉")),
                    candidate_type: "research_topic_context",
                    summary: "research topic context",
                    confidence: 0.96,
                },
            ),
            test_memory_draft(
                &conn,
                TestMemoryDraftInput {
                    trace_id: "trace-memory-shared-scope-seed",
                    context: &context,
                    scope_type: "knowledge_space",
                    scope_ref: "knowledge_space:other",
                    candidate_type: "retrieval_preference",
                    summary: "unmatched knowledge scope preference",
                    confidence: 0.96,
                },
            ),
        ] {
            insert_and_apply_test_memory(&conn, &draft);
        }

        let source_collection = test_memory_draft(
            &conn,
            TestMemoryDraftInput {
                trace_id: "trace-memory-shared-scope-seed",
                context: &context,
                scope_type: "source_collection",
                scope_ref: SOURCE_COLLECTION_SCOPE_REF,
                candidate_type: "source_collection_usage_preference",
                summary: "source collection usage preference",
                confidence: 1.0,
            },
        );
        let source_collection_id = source_collection.candidate_id.clone();
        insert_and_apply_test_memory(&conn, &source_collection);
        manually_enable_test_memory(&conn, &source_collection_id);

        let read_context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-shared-scope-read",
                model_id: "tonglingyu",
                external_user_ref: "memory-shared-reader",
                external_session_id: "memory-shared-reader-chat",
                external_message_id: "memory-shared-read-message",
                question: "介绍林黛玉",
                messages: &[ContextMessage {
                    role: "user".to_string(),
                    content: "介绍林黛玉".to_string(),
                }],
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("shared memory context read");
        assert_eq!(
            read_context.context_pack["memory_read_refs"]
                .as_array()
                .map(Vec::len),
            Some(4)
        );
        let main_projection = read_context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-main")
            .expect("main projection");
        assert_eq!(
            main_projection.projection_payload["memory_summaries"]
                .as_array()
                .map(Vec::len),
            Some(4)
        );
        let text_projection = read_context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-text")
            .expect("text projection");
        assert_eq!(
            text_projection.projection_payload["memory_summaries"]
                .as_array()
                .map(Vec::len),
            Some(MEMORY_READ_BUDGET_TOOL_PROFILE)
        );
        let reviewer_projection = read_context
            .context_projections
            .iter()
            .find(|projection| projection.consumer_name == "honglou-reviewer")
            .expect("reviewer projection");
        assert_eq!(
            reviewer_projection.projection_payload["memory_summaries"]
                .as_array()
                .map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn memory_candidate_state_machine_reclassifies_merges_and_rejects() {
        let conn = conn();
        for (trace_id, message_id, question) in [
            (
                "trace-memory-reclassify-a",
                "message-a",
                "我喜欢回答里多引用原文。",
            ),
            (
                "trace-memory-reclassify-b",
                "message-b",
                "我喜欢回答里多引用原文。",
            ),
            (
                "trace-memory-reclassify-c",
                "message-c",
                "我不喜欢太长的答案。",
            ),
        ] {
            let messages = vec![ContextMessage {
                role: "user".to_string(),
                content: question.to_string(),
            }];
            let context = create_context_for_request(
                &conn,
                ContextRequestInput {
                    trace_id,
                    model_id: "tonglingyu",
                    external_user_ref: "memory-state-user",
                    external_session_id: "memory-state-chat",
                    external_message_id: message_id,
                    question,
                    messages: &messages,
                    history_over_limit: false,
                    max_messages: 40,
                },
            )
            .expect("context created");
            append_final_response(
                &conn,
                FinalResponseJournalInput {
                    trace_id,
                    user_session_id: &context.user_session_id,
                    interaction_context_id: &context.interaction_context_id,
                    context_pack_id: &context.context_pack_id,
                    external_message_id: message_id,
                    package_id: Some("pkg-memory-state"),
                    response: &json!({"status": "ok"}),
                },
            )
            .expect("final response journal");
        }
        run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "admin_manual",
                actor: "test-admin",
                limit: 50,
                dry_run: false,
                trace_id: None,
            },
        )
        .expect("collector run");
        let candidates = list_memory_candidates(
            &conn,
            MemoryCandidateListInput {
                status: Some("pending"),
                scope_type: None,
                scope_ref: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("candidate list");
        let items = candidates["items"].as_array().expect("items");
        assert_eq!(items.len(), 3);
        let first = items[0]["candidate_id"]
            .as_str()
            .expect("candidate")
            .to_string();
        let second = items[1]["candidate_id"]
            .as_str()
            .expect("candidate")
            .to_string();
        let third = items[2]["candidate_id"]
            .as_str()
            .expect("candidate")
            .to_string();

        transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &first,
                action: "reclassify",
                actor: "test-admin",
                reason: Some("classify as response preference"),
                candidate_type: Some("retrieval_preference"),
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("reclassify");
        let first_after = read_memory_candidate(&conn, &first)
            .expect("read candidate")
            .expect("candidate exists");
        assert_eq!(first_after["status"], json!("pending"));
        assert_eq!(first_after["candidate_type"], json!("retrieval_preference"));

        transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &second,
                action: "merge",
                actor: "test-admin",
                reason: Some("duplicate"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: Some(&first),
                expires_at: None,
            },
        )
        .expect("merge");
        let second_after = read_memory_candidate(&conn, &second)
            .expect("read candidate")
            .expect("candidate exists");
        assert_eq!(second_after["status"], json!("merged"));
        assert_eq!(second_after["merged_into_candidate_id"], json!(first));

        transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &third,
                action: "reject",
                actor: "test-admin",
                reason: Some("not durable enough"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("reject");
        let err = transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &third,
                action: "approve",
                actor: "test-admin",
                reason: Some("invalid retry"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect_err("rejected candidate cannot be approved");
        assert!(err.to_string().contains("invalid memory state transition"));
    }

    #[test]
    fn memory_transitions_require_operator_reason_and_supported_scope() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "以后回答时，请用简体中文短句总结。".to_string(),
        }];
        let context = create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-required-reason",
                model_id: "tonglingyu",
                external_user_ref: "memory-required-user",
                external_session_id: "memory-required-chat",
                external_message_id: "memory-required-message",
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
                trace_id: "trace-memory-required-reason",
                user_session_id: &context.user_session_id,
                interaction_context_id: &context.interaction_context_id,
                context_pack_id: &context.context_pack_id,
                external_message_id: "memory-required-message",
                package_id: Some("pkg-memory-required"),
                response: &json!({"status": "ok"}),
            },
        )
        .expect("final response journal");
        run_memory_collector(
            &conn,
            MemoryCollectorRunInput {
                trigger_type: "admin_manual",
                actor: "test-admin",
                limit: 20,
                dry_run: false,
                trace_id: Some("trace-memory-required-reason"),
            },
        )
        .expect("collector run");
        let candidates = list_memory_candidates(
            &conn,
            MemoryCandidateListInput {
                status: Some("pending"),
                scope_type: None,
                scope_ref: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("candidate list");
        let candidate_id = candidates["items"][0]["candidate_id"]
            .as_str()
            .expect("candidate id")
            .to_string();
        let missing_reason = transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &candidate_id,
                action: "approve",
                actor: "test-admin",
                reason: None,
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect_err("reason is required");
        assert!(missing_reason.to_string().contains("reason is required"));
        let missing_actor = transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id: &candidate_id,
                action: "approve",
                actor: " ",
                reason: Some("approved in test"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect_err("operator identity is required");
        assert!(missing_actor.to_string().contains("operator identity"));
        let invalid_scope = list_memory_candidates(
            &conn,
            MemoryCandidateListInput {
                status: None,
                scope_type: Some("project"),
                scope_ref: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect_err("unsupported memory scope must fail closed");
        assert!(
            invalid_scope
                .to_string()
                .contains("invalid candidate scope")
        );
    }

    #[test]
    fn llm_memory_extraction_contract_rejects_overreach() {
        let valid = validate_llm_memory_extraction_output(&json!({
            "schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
            "is_long_term_memory": true,
            "is_temporary_instruction": false,
            "is_quoted_or_third_party": false,
            "has_contradiction": false,
            "scope_type": "user_private",
            "candidate_type": "language_preference",
            "confidence": 0.82,
            "sensitivity": "low",
            "risk_flags": [],
            "ttl_hint": "180d",
            "exclusion_flags": [],
        }))
        .expect("valid llm output");
        assert_eq!(valid["status"], json!("pending"));
        assert_eq!(valid["promotion_allowed"], json!(false));

        let invalid = validate_llm_memory_extraction_output(&json!({
            "schema_version": SCOPED_MEMORY_LLM_FILTER_SCHEMA_VERSION,
            "is_long_term_memory": true,
            "is_temporary_instruction": false,
            "is_quoted_or_third_party": false,
            "has_contradiction": false,
            "scope_type": "user_private",
            "candidate_type": "language_preference",
            "confidence": 0.91,
            "sensitivity": "low",
            "risk_flags": [],
            "ttl_hint": "180d",
            "exclusion_flags": [],
            "promotion": "approve",
            "acl": {"read_enabled": true},
        }))
        .expect_err("llm must not decide promotion or acl");
        assert!(invalid.to_string().contains("forbidden fields"));
    }

    fn load_rows_json_for_test(conn: &Connection, sql: &str) -> Vec<Value> {
        let mut stmt = conn.prepare(sql).expect("prepare");
        let column_count = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                let mut values = Vec::new();
                for index in 0..column_count {
                    values.push(row.get::<_, String>(index)?);
                }
                Ok(json!(values))
            })
            .expect("query rows");
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect rows")
    }
}
