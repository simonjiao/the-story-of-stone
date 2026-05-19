use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration as TimeDuration, OffsetDateTime};

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
pub(crate) const MEMORY_PROMOTION_POLICY_VERSION: &str = "tonglingyu-memory-promotion-policy-v1";

const SESSION_SUMMARY_MAX_CHARS: usize = 600;
const JOURNAL_SUMMARY_MAX_CHARS: usize = 240;
const MEMORY_SUMMARY_MAX_CHARS: usize = 220;
const MEMORY_RAW_EXCERPT_MAX_CHARS: usize = 420;
const MEMORY_COLLECTOR_LEASE_NAME: &str = "memory-collector";
const MEMORY_COLLECTOR_LEASE_TTL_SECS: i64 = 300;
const MEMORY_PHASE3_READ_DISABLED_REASON: &str = "phase3_read_path_not_enabled";

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
        CREATE INDEX IF NOT EXISTS idx_memory_collector_runs_started
            ON memory_collector_runs(started_at);
        "#,
    )?;
    ensure_column(conn, "context_packs", "context_pack_ref", "TEXT")?;
    ensure_column(conn, "context_packs", "policy_versions_json", "TEXT")?;
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
        params![CONTEXT_SCHEMA_VERSION, now_rfc3339()],
    )?;
    Ok(())
}

pub(crate) fn create_context_for_request(
    conn: &Connection,
    input: ContextRequestInput<'_>,
) -> Result<ContextResolution> {
    let user_session_id = get_or_create_user_session(
        conn,
        input.external_user_ref,
        input.external_session_id,
        input.model_id,
    )?;
    let interaction_context_id = get_or_create_interaction_context(conn, &user_session_id)?;
    assert_memory_reads_disabled(conn)?;
    let prior_subject = latest_subject_from_journal(conn, &user_session_id)?;
    let session_summary = session_summary(input.messages, prior_subject.as_deref());
    let resolver = resolve_question(input.question, input.messages, prior_subject.as_deref());
    let context_pack_id = format!("context-pack-{}", uuid::Uuid::now_v7().simple());
    let context_pack_ref = context_pack_ref(input.trace_id, &context_pack_id);
    let active_scopes = vec![json!({
        "scope_type": "session",
        "scope_id": &input.external_session_id,
        "relation_type": "primary",
    })];
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
    let profile_views = profile_views(&resolver.resolved_question, &session_summary);
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
        "memory_read_refs": [],
        "forbidden_context": [
            "complete_user_history",
            "unauthorized_memory",
            "system_prompt",
            "unreviewed_memory_candidate"
        ],
        "output_contract": {
            "public_response_exposes_context_ids": false,
            "evidence_package_allows_memory": false,
            "schema_version": CONTEXT_SCHEMA_VERSION,
        },
        "profile_views": &profile_views,
        "schema_version": CONTEXT_SCHEMA_VERSION,
        "policy_version": CONTEXT_POLICY_VERSION,
        "policy_versions": {
            "context_policy": CONTEXT_POLICY_VERSION,
            "resolver": RESOLVER_SCHEMA_VERSION,
            "journal_retention": JOURNAL_RETENTION_POLICY_VERSION,
        },
        "resolver": resolver.audit_json(),
    });
    let context_pack_digest = digest_json(&context_pack);
    context_pack["digest"] = json!(&context_pack_digest);
    let context_projections = build_context_projections(
        input.trace_id,
        &interaction_context_id,
        &context_pack_id,
        &context_pack_ref,
        &resolver.resolved_question,
        &session_summary,
    );
    conn.execute(
        "INSERT INTO context_packs (
            context_pack_id, context_pack_ref, trace_id, interaction_context_id, profile_name, resolved_question,
            session_summary, active_scopes_json, candidate_scopes_json, allowed_tools_json,
            forbidden_tools_json, memory_read_refs_json, forbidden_context_json,
            output_contract_json, profile_views_json, policy_versions_json, schema_version, digest, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
            serde_json::to_string(&json!([]))?,
            serde_json::to_string(&json!([
                "complete_user_history",
                "unauthorized_memory",
                "system_prompt",
                "unreviewed_memory_candidate"
            ]))?,
            serde_json::to_string(&context_pack["output_contract"])?,
            serde_json::to_string(&profile_views)?,
            serde_json::to_string(&context_pack["policy_versions"])?,
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
                    candidate_summaries.push(memory_candidate_summary_json(draft.as_ref()));
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
        "error_count": error_count,
        "watermark_journal_id": watermark_journal_id,
        "started_at": started_at,
        "completed_at": completed_at,
        "llm_boundary": llm_boundary_contract_json(),
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
        "read_path_enabled": false,
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
        "read_path_enabled": false,
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
            update_candidate_status(
                conn,
                &current.candidate_id,
                "approved",
                None,
                input.expires_at,
                &now,
            )?;
            append_memory_transition_audit(
                conn,
                MemoryTransitionAuditInput {
                    entity_type: "memory_candidate",
                    entity_id: Some(&current.candidate_id),
                    action: "approve",
                    from_status: Some(&current.status),
                    to_status: Some("approved"),
                    actor,
                    reason: Some(reason),
                    metadata: json!({"candidate_ref": &current.candidate_ref}),
                },
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
        "read_path_enabled": false,
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
    let to_status = match input.action {
        "revoke" => "revoked",
        "expire" => "expired",
        _ => unreachable!("validated memory card action"),
    };
    conn.execute(
        "UPDATE memory_cards
         SET status = ?1, revoked_by = ?2, revoked_at = ?3,
             expires_at = COALESCE(expires_at, ?4), read_enabled = 0
         WHERE memory_card_id = ?5",
        params![to_status, actor, &now, &now, &current.memory_card_id],
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
            }),
        },
    )?;
    let refreshed = read_memory_card(conn, input.memory_card_id)?
        .ok_or_else(|| anyhow!("memory card not found after transition"))?;
    Ok(json!({
        "object": "tonglingyu.memory_card_transition",
        "schema_version": MEMORY_TRANSITION_AUDIT_SCHEMA_VERSION,
        "status": "ok",
        "action": input.action,
        "memory_card": refreshed,
        "read_path_enabled": false,
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

pub(crate) fn assert_memory_reads_disabled(conn: &Connection) -> Result<()> {
    let count = conn.query_row(
        "SELECT COUNT(*) FROM memory_cards WHERE read_enabled <> 0",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if count > 0 {
        return Err(anyhow!(
            "memory read path is disabled for Phase3 but read_enabled cards exist"
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
    scope_type: String,
    scope_ref: String,
    candidate_type: String,
    summary: String,
    summary_sha256: String,
    sensitivity: String,
    risk_flags: Value,
}

#[derive(Debug, Clone)]
struct MemoryCardCore {
    memory_card_id: String,
    memory_card_ref: String,
    source_candidate_id: String,
    status: String,
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

fn promote_memory_candidate(
    conn: &Connection,
    candidate: &MemoryCandidateCore,
    actor: &str,
    reason: &str,
) -> Result<()> {
    validate_memory_scope_type(&candidate.scope_type)?;
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
                    "read_enabled": false,
                    }),
                },
            )?;
            return Ok(());
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
        "schema_version": "tonglingyu-memory-acl-phase3-v1",
        "scope_type": &candidate.scope_type,
        "scope_ref_sha256": hash_text(&candidate.scope_ref),
        "read_enabled": false,
        "allowed_readers": [],
        "phase3_read_disable_reason": MEMORY_PHASE3_READ_DISABLED_REASON,
    });
    conn.execute(
        "INSERT INTO memory_cards (
            memory_card_id, memory_card_ref, source_candidate_id, status,
            scope_type, scope_ref, summary, summary_sha256, acl_json, sensitivity,
            promotion_policy_version, promoted_by, promoted_at, revoked_by, revoked_at,
            expires_at, read_enabled, audit_ref, schema_version
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                  NULL, NULL, NULL, 0, ?13, ?14)",
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
            MEMORY_PROMOTION_POLICY_VERSION,
            actor,
            &now,
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
            "read_enabled": false,
            "phase3_read_disable_reason": MEMORY_PHASE3_READ_DISABLED_REASON,
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
            "read_enabled": false,
            }),
        },
    )?;
    assert_memory_reads_disabled(conn)?;
    Ok(())
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
        "SELECT candidate_id, candidate_ref, status, scope_type, scope_ref,
                candidate_type, summary, summary_sha256, sensitivity, risk_flags_json
         FROM memory_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        |row| {
            Ok(MemoryCandidateCore {
                candidate_id: row.get(0)?,
                candidate_ref: row.get(1)?,
                status: row.get(2)?,
                scope_type: row.get(3)?,
                scope_ref: row.get(4)?,
                candidate_type: row.get(5)?,
                summary: row.get(6)?,
                summary_sha256: row.get(7)?,
                sensitivity: row.get(8)?,
                risk_flags: parse_json_column(row.get::<_, String>(9)?),
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
        "SELECT memory_card_id, memory_card_ref, source_candidate_id, status
         FROM memory_cards WHERE memory_card_id = ?1",
        params![memory_card_id],
        |row| {
            Ok(MemoryCardCore {
                memory_card_id: row.get(0)?,
                memory_card_ref: row.get(1)?,
                source_candidate_id: row.get(2)?,
                status: row.get(3)?,
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
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "candidate_type" | "summary" | "confidence" | "risk_flags" | "scope_hint"
        ) {
            return Err(anyhow!("unsupported LLM memory extraction field"));
        }
    }
    let candidate_type = object
        .get("candidate_type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("LLM memory extraction missing candidate_type"))?;
    validate_memory_candidate_type(candidate_type)?;
    let summary = object
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("LLM memory extraction missing summary"))?;
    let confidence = object
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("LLM memory extraction missing confidence"))?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err(anyhow!("LLM memory extraction confidence out of range"));
    }
    Ok(json!({
        "candidate_type": candidate_type,
        "summary": bounded_summary(summary, MEMORY_SUMMARY_MAX_CHARS),
        "confidence": confidence,
        "risk_flags": object.get("risk_flags").cloned().unwrap_or_else(|| json!([])),
        "status": if confidence >= 0.45 { "pending" } else { "suppressed" },
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
        "user_identity_preference".to_string()
    } else if [
        "简体",
        "繁体",
        "短句",
        "详细",
        "回答时",
        "以后回答",
        "请用",
        "引用原文",
    ]
    .iter()
    .any(|marker| text.contains(marker))
    {
        "user_response_preference".to_string()
    } else {
        "user_preference".to_string()
    }
}

fn user_private_scope_ref(external_user_ref: &str) -> String {
    format!("user_private:sha256:{}", hash_text(external_user_ref))
}

fn memory_candidate_summary(candidate_type: &str, text: &str) -> String {
    let prefix = match candidate_type {
        "user_identity_preference" => "用户称呼偏好",
        "user_response_preference" => "用户回答偏好",
        _ => "用户偏好",
    };
    bounded_summary(
        &format!("{prefix}: {}", text.trim()),
        MEMORY_SUMMARY_MAX_CHARS,
    )
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
        "position": "after_hard_deny_and_redaction_only",
        "input_contract": [
            "redacted_journal_summary",
            "scope_hint",
            "json_schema"
        ],
        "allowed_decisions": ["structured_candidate_extraction"],
        "forbidden_decisions": [
            "approve",
            "promote",
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
    if matches!(action, "revoke" | "expire") {
        Ok(())
    } else {
        Err(anyhow!("invalid memory card action"))
    }
}

fn validate_memory_candidate_type(candidate_type: &str) -> Result<()> {
    if matches!(
        candidate_type,
        "user_preference" | "user_response_preference" | "user_identity_preference"
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid memory candidate type"))
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

#[derive(Debug)]
struct ResolverOutput {
    resolved_question: String,
    referent_bindings: Vec<String>,
    used_context_refs: Vec<String>,
    confidence: f64,
    needs_clarification: bool,
    clarification_question: Option<String>,
    unsupported_reason: Option<String>,
}

impl ResolverOutput {
    fn audit_json(&self) -> Value {
        json!({
            "schema_version": RESOLVER_SCHEMA_VERSION,
            "strategy": "deterministic_rules",
            "resolved_question": self.resolved_question,
            "referent_bindings": self.referent_bindings,
            "used_context_refs": self.used_context_refs,
            "confidence": self.confidence,
            "needs_clarification": self.needs_clarification,
            "clarification_question": self.clarification_question,
            "unsupported_reason": self.unsupported_reason,
            "llm_used": false,
        })
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
            "memory-disabled-phase1",
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
) -> ResolverOutput {
    let current_subject = latest_subject_in_text(question);
    if let Some(subject) = current_subject {
        return ResolverOutput {
            resolved_question: question.to_string(),
            referent_bindings: vec![subject],
            used_context_refs: Vec::new(),
            confidence: 1.0,
            needs_clarification: false,
            clarification_question: None,
            unsupported_reason: None,
        };
    }
    if contains_referential_pronoun(question) {
        let referent =
            latest_subject_from_messages(messages).or_else(|| prior_subject.map(str::to_string));
        if let Some(referent) = referent {
            let resolved_question = bind_referent(question, &referent);
            return ResolverOutput {
                resolved_question,
                referent_bindings: vec![referent],
                used_context_refs: vec!["session_summary".to_string()],
                confidence: 0.86,
                needs_clarification: false,
                clarification_question: None,
                unsupported_reason: None,
            };
        }
        return ResolverOutput {
            resolved_question: question.to_string(),
            referent_bindings: Vec::new(),
            used_context_refs: Vec::new(),
            confidence: 0.2,
            needs_clarification: true,
            clarification_question: Some(
                "请明确你指的是哪位人物或对象，我再继续回答。".to_string(),
            ),
            unsupported_reason: Some("unresolved_referent".to_string()),
        };
    }
    ResolverOutput {
        resolved_question: question.to_string(),
        referent_bindings: Vec::new(),
        used_context_refs: Vec::new(),
        confidence: 1.0,
        needs_clarification: false,
        clarification_question: None,
        unsupported_reason: None,
    }
}

fn session_summary(messages: &[ContextMessage], prior_subject: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(subject) =
        latest_subject_from_messages(messages).or_else(|| prior_subject.map(str::to_string))
    {
        parts.push(format!("最近讨论对象：{subject}"));
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
        "无可用会话摘要。".to_string()
    } else {
        bounded_summary(&parts.join("；"), SESSION_SUMMARY_MAX_CHARS)
    }
}

fn profile_views(resolved_question: &str, session_summary: &str) -> Vec<ContextPackProfileView> {
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
            memory_read_refs: Vec::new(),
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
            memory_read_refs: Vec::new(),
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
            memory_read_refs: Vec::new(),
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
            memory_read_refs: Vec::new(),
        },
    ]
}

fn build_context_projections(
    trace_id: &str,
    interaction_context_id: &str,
    context_pack_id: &str,
    context_pack_ref: &str,
    resolved_question: &str,
    session_summary: &str,
) -> Vec<ContextProjection> {
    profile_views(resolved_question, session_summary)
        .into_iter()
        .map(|view| {
            build_context_projection(
                trace_id,
                interaction_context_id,
                context_pack_id,
                context_pack_ref,
                view,
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
) -> ContextProjection {
    let context_projection_id = format!("context-projection-{}", uuid::Uuid::now_v7().simple());
    let context_projection_ref =
        format!("context-projection://tonglingyu/{trace_id}/{context_projection_id}");
    let output_contract = projection_output_contract(&view.profile_name);
    let forbidden_tools = Vec::<String>::new();
    let projection_payload = json!({
        "object": "tonglingyu.context_projection_payload",
        "visible_question": &view.visible_question,
        "session_summary": &view.session_summary,
        "forbidden_context": &view.forbidden_context,
        "memory_read_refs": &view.memory_read_refs,
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
        if let Some(subject) = latest_subject_in_text(&row?) {
            return Ok(Some(subject));
        }
    }
    Ok(None)
}

fn latest_subject_from_messages(messages: &[ContextMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == "user" || message.role == "assistant")
        .find_map(|message| latest_subject_in_text(&message.content))
}

fn latest_subject_in_text(text: &str) -> Option<String> {
    known_subjects()
        .iter()
        .find(|subject| text.contains(*subject))
        .map(|subject| (*subject).to_string())
}

fn contains_referential_pronoun(text: &str) -> bool {
    ["她", "他", "那个人", "这个人", "刚才那个人", "刚才的人"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn bind_referent(question: &str, referent: &str) -> String {
    let mut output = question.to_string();
    for needle in ["刚才那个人", "刚才的人", "那个人", "这个人", "她", "他"] {
        if output.contains(needle) {
            output = output.replacen(needle, referent, 1);
            break;
        }
    }
    output
}

fn is_openwebui_metadata_prompt(text: &str) -> bool {
    text.contains("### Task:") && text.contains("### Chat History:")
}

fn known_subjects() -> &'static [&'static str] {
    &[
        "尤三姐",
        "林黛玉",
        "黛玉",
        "贾宝玉",
        "宝玉",
        "薛宝钗",
        "宝钗",
        "王熙凤",
        "凤姐",
        "贾母",
        "袭人",
        "晴雯",
        "妙玉",
        "探春",
        "迎春",
        "惜春",
        "元春",
        "巧姐",
        "李纨",
        "秦可卿",
        "刘姥姥",
        "甄士隐",
        "贾雨村",
    ]
}

fn load_context_packs(conn: &Connection, trace_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT context_pack_id, COALESCE(context_pack_ref, context_pack_id),
                interaction_context_id, profile_name, resolved_question,
                session_summary, active_scopes_json, candidate_scopes_json, allowed_tools_json,
                forbidden_tools_json, memory_read_refs_json, forbidden_context_json,
                output_contract_json, profile_views_json, COALESCE(policy_versions_json, '{}'),
                schema_version, COALESCE(digest, ''), created_at
         FROM context_packs WHERE trace_id = ?1 ORDER BY created_at, context_pack_id",
    )?;
    let rows = stmt.query_map(params![trace_id], |row| {
        Ok(json!({
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
            "memory_read_refs": parse_json_column(row.get::<_, String>(10)?),
            "forbidden_context": parse_json_column(row.get::<_, String>(11)?),
            "output_contract": parse_json_column(row.get::<_, String>(12)?),
            "profile_views": parse_json_column(row.get::<_, String>(13)?),
            "policy_versions": parse_json_column(row.get::<_, String>(14)?),
            "schema_version": row.get::<_, String>(15)?,
            "digest": row.get::<_, String>(16)?,
            "created_at": row.get::<_, String>(17)?,
        }))
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
            "metadata": redact_journal_metadata(parse_json_column(row.get::<_, String>(13)?)),
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

fn parse_json_column(value: String) -> Value {
    serde_json::from_str(&value).unwrap_or(Value::Null)
}

fn projection_payload_summary(value: &Value) -> Value {
    json!({
        "visible_question_sha256": value
            .get("visible_question")
            .and_then(Value::as_str)
            .map(hash_text),
        "has_session_summary": value.get("session_summary").is_some_and(|item| !item.is_null()),
        "forbidden_context_count": value
            .get("forbidden_context")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        "memory_read_ref_count": value
            .get("memory_read_refs")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
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
    use super::*;

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

        let resolved = resolve_question("她最后怎么样？", &messages, None);

        assert_eq!(resolved.resolved_question, "尤三姐最后怎么样？");
        assert!(!resolved.needs_clarification);
        assert!(resolved.confidence >= 0.75);
    }

    #[test]
    fn unresolved_referent_fails_closed() {
        let resolved = resolve_question("她最后怎么样？", &[], None);

        assert!(resolved.needs_clarification);
        assert!(resolved.confidence < 0.45);
        assert_eq!(
            resolved.unsupported_reason.as_deref(),
            Some("unresolved_referent")
        );
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
    fn memory_collector_creates_pending_candidate_and_promotes_read_disabled_card() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "以后回答时，请用简体中文短句总结。".to_string(),
        }];
        create_context_for_request(
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
        assert!(
            report["denied_count"].as_i64().unwrap_or_default() >= 1,
            "{report}"
        );
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
        let candidate = candidates["items"][0].clone();
        assert_eq!(
            candidate["candidate_type"],
            json!("user_response_preference")
        );
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
        let candidate_id = candidate["candidate_id"].as_str().expect("candidate id");

        transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id,
                action: "approve",
                actor: "test-admin",
                reason: Some("approved in test"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("approve candidate");
        transition_memory_candidate(
            &conn,
            MemoryCandidateTransitionInput {
                candidate_id,
                action: "promote",
                actor: "test-admin",
                reason: Some("promote in test"),
                candidate_type: None,
                sensitivity: None,
                merge_target_candidate_id: None,
                expires_at: None,
            },
        )
        .expect("promote candidate");

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
        assert_eq!(card["read_enabled"], json!(false));
        assert_eq!(
            card["acl"]["phase3_read_disable_reason"],
            json!(MEMORY_PHASE3_READ_DISABLED_REASON)
        );
        assert_memory_reads_disabled(&conn).expect("read path remains disabled");

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
        let audit_count = table_count(&conn, "memory_transition_audit").expect("audit count");
        assert!(audit_count >= 4, "audit_count={audit_count}");
    }

    #[test]
    fn memory_collector_hard_denies_secrets_without_candidate() {
        let conn = conn();
        let messages = vec![ContextMessage {
            role: "user".to_string(),
            content: "请记住 token=sk-test-secret-value".to_string(),
        }];
        create_context_for_request(
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
            create_context_for_request(
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
                candidate_type: Some("user_response_preference"),
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
        assert_eq!(
            first_after["candidate_type"],
            json!("user_response_preference")
        );

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
        create_context_for_request(
            &conn,
            ContextRequestInput {
                trace_id: "trace-memory-required-reason",
                model_id: "tonglingyu",
                external_user_ref: "memory-required-user",
                external_session_id: "memory-required-chat",
                external_message_id: "memory-required-message",
                question: "以后回答时，请用简体中文短句总结。",
                messages: &messages,
                history_over_limit: false,
                max_messages: 40,
            },
        )
        .expect("context created");
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
            "candidate_type": "user_response_preference",
            "summary": "用户回答偏好: 以后回答时用简体短句。",
            "confidence": 0.82,
            "risk_flags": [],
        }))
        .expect("valid llm output");
        assert_eq!(valid["status"], json!("pending"));
        assert_eq!(valid["promotion_allowed"], json!(false));

        let invalid = validate_llm_memory_extraction_output(&json!({
            "candidate_type": "user_response_preference",
            "summary": "用户回答偏好",
            "confidence": 0.91,
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
