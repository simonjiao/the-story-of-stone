use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

pub(crate) const CONTEXT_SCHEMA_VERSION: &str = "tonglingyu-scoped-context-v1";
pub(crate) const CONTEXT_PROJECTION_SCHEMA_VERSION: &str = "tonglingyu-context-projection-v1";
pub(crate) const CONTEXT_POLICY_VERSION: &str = "tonglingyu-context-policy-v1";
pub(crate) const JOURNAL_RETENTION_POLICY_VERSION: &str = "tonglingyu-session-journal-retention-v1";
pub(crate) const RESOLVER_SCHEMA_VERSION: &str = "tonglingyu-question-resolver-v1";
pub(crate) const RUNTIME_CONSUMER_TYPE: &str = "runtime_profile";
pub(crate) const RUNTIME_ADAPTER: &str = "tonglingyu-runtime-adapter-v1";

const SESSION_SUMMARY_MAX_CHARS: usize = 600;
const JOURNAL_SUMMARY_MAX_CHARS: usize = 240;

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
    }))
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
}
