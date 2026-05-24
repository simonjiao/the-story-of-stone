use crate::{
    EvidenceCard, append_runtime_audit_event, bounded_optional_text, canonical_json_value,
    evidence_card_source_layer, hash_text, now_rfc3339, question_frame, relation_support_terms,
    relation_text_matches_support_terms, search_evidence_result, sqlite_table_exists,
};
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

const ONLINE_INGEST_SCHEMA_VERSION: &str = "tonglingyu-online-evidence-card-ingest-v1";
const ONLINE_CARD_SCHEMA_VERSION: &str = "tonglingyu.online_evidence_card.v1";
const ONLINE_CARD_BUILDER_VERSION: &str = "tonglingyu-online-card-builder-v1";
const ONLINE_CARD_VALIDATOR_VERSION: &str = "tonglingyu-online-card-validator-v1";
const DEFAULT_WORKER_LIMIT: usize = 8;
const DEFAULT_RETRIEVAL_LIMIT: usize = 12;
const CARD_INGEST_JOB_LEASE_SECS: i64 = 60;
const CARD_INGEST_JOB_MAX_ATTEMPTS: i64 = 3;
const CARD_INGEST_JOB_BACKOFF_BASE_SECS: i64 = 5;
const CARD_INGEST_JOB_BACKOFF_MAX_SECS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineEvidenceCardUpdateRequestInput {
    pub trace_id: String,
    pub session_id: Option<String>,
    pub resolved_question: String,
    pub question_frame: Option<Value>,
    pub coverage_gap_reason: String,
    pub source_scope_policy: Value,
    pub recall_advice_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineEvidenceCardUpdateRequestRecord {
    pub update_request_id: String,
    pub trace_id: String,
    pub session_id: Option<String>,
    pub resolved_question: String,
    pub question_frame: Option<Value>,
    pub coverage_gap_reason: String,
    pub source_scope_policy: Value,
    pub recall_advice_ref: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineEvidenceCardWorkerRunInput {
    pub actor: String,
    pub limit: usize,
    pub retrieval_limit: usize,
}

impl Default for OnlineEvidenceCardWorkerRunInput {
    fn default() -> Self {
        Self {
            actor: "online-evidence-card-worker".to_string(),
            limit: DEFAULT_WORKER_LIMIT,
            retrieval_limit: DEFAULT_RETRIEVAL_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineEvidenceCardWorkerRunReport {
    pub object: String,
    pub schema_version: String,
    pub actor: String,
    pub processed_count: usize,
    pub raw_candidate_count: usize,
    pub staged_count: usize,
    pub promoted_count: usize,
    pub conflicted_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvidenceCandidateRecord {
    pub candidate_id: String,
    pub update_request_id: String,
    pub trace_id: String,
    pub source_id: String,
    pub source_layer: String,
    pub source_hash: String,
    pub span_start: i64,
    pub span_end: i64,
    pub matched_terms: Vec<String>,
    pub query_frame: Value,
    pub rule_gap_reason: String,
    pub cluster_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardIngestJobRecord {
    pub job_id: String,
    pub update_request_id: String,
    pub trace_id: String,
    pub status: String,
    pub stage: String,
    pub leased_by: Option<String>,
    pub lease_until: Option<String>,
    pub heartbeat_at: Option<String>,
    pub attempt_count: i64,
    pub max_attempts: i64,
    pub next_run_at: String,
    pub last_error: Option<String>,
    pub candidate_id: Option<String>,
    pub staged_card_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalStagedCardRecord {
    pub staged_card_id: String,
    pub exact_span_key: String,
    pub claim_key: String,
    pub cluster_key: String,
    pub source_scope: String,
    pub slot_id: String,
    pub entities_key: String,
    pub entities: Value,
    pub polarity: String,
    pub modality: String,
    pub evidence_strength: String,
    pub supporting_spans: Vec<Value>,
    pub evidence: EvidenceCard,
    pub schema_version: String,
    pub source_corpus_version: String,
    pub source_hash: String,
    pub rules_version: String,
    pub builder_version: String,
    pub validator_version: String,
    pub status: String,
    pub promoted_evidence_id: Option<String>,
    pub created_from_trace_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

struct StageCandidateInput {
    update_request_id: String,
    trace_id: String,
    source_hash: String,
    source_scope: String,
    slot_id: String,
    entities: Value,
    entities_key: String,
    polarity: String,
    modality: String,
    evidence_strength: String,
    rules_version: String,
    card: EvidenceCard,
    matched_terms: Vec<String>,
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS online_evidence_card_update_requests (
            update_request_id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            trace_id TEXT NOT NULL,
            session_id TEXT,
            resolved_question TEXT NOT NULL,
            question_frame_json TEXT,
            coverage_gap_reason TEXT NOT NULL,
            source_scope_policy_json TEXT NOT NULL,
            recall_advice_ref TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS raw_evidence_candidates (
            candidate_id TEXT PRIMARY KEY,
            update_request_id TEXT NOT NULL REFERENCES online_evidence_card_update_requests(update_request_id),
            trace_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_layer TEXT NOT NULL,
            source_hash TEXT NOT NULL,
            span_start INTEGER NOT NULL,
            span_end INTEGER NOT NULL,
            matched_terms_json TEXT NOT NULL,
            query_frame_json TEXT NOT NULL,
            rule_gap_reason TEXT NOT NULL,
            cluster_key TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS card_ingest_jobs (
            job_id TEXT PRIMARY KEY,
            update_request_id TEXT NOT NULL UNIQUE REFERENCES online_evidence_card_update_requests(update_request_id),
            trace_id TEXT NOT NULL,
            status TEXT NOT NULL,
            stage TEXT NOT NULL,
            leased_by TEXT,
            lease_until TEXT,
            heartbeat_at TEXT,
            attempt_count INTEGER NOT NULL,
            max_attempts INTEGER NOT NULL,
            next_run_at TEXT NOT NULL,
            last_error TEXT,
            candidate_id TEXT,
            staged_card_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS canonical_staged_cards (
            staged_card_id TEXT PRIMARY KEY,
            exact_span_key TEXT NOT NULL,
            claim_key TEXT NOT NULL,
            cluster_key TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            slot_id TEXT NOT NULL,
            entities_key TEXT NOT NULL,
            entities_json TEXT NOT NULL,
            polarity TEXT NOT NULL,
            modality TEXT NOT NULL,
            evidence_strength TEXT NOT NULL,
            supporting_spans_json TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            source_corpus_version TEXT NOT NULL,
            source_hash TEXT NOT NULL,
            rules_version TEXT NOT NULL,
            builder_version TEXT NOT NULL,
            validator_version TEXT NOT NULL,
            status TEXT NOT NULL,
            promoted_evidence_id TEXT,
            created_from_trace_ids_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS staged_card_events (
            event_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            update_request_id TEXT,
            staged_card_id TEXT,
            candidate_id TEXT,
            event_type TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT,
            reason_code TEXT NOT NULL,
            rule_id TEXT,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_online_evidence_update_idempotency
            ON online_evidence_card_update_requests(idempotency_key);
        CREATE INDEX IF NOT EXISTS idx_online_evidence_update_status
            ON online_evidence_card_update_requests(status, created_at);
        CREATE INDEX IF NOT EXISTS idx_online_evidence_update_trace
            ON online_evidence_card_update_requests(trace_id);
        CREATE INDEX IF NOT EXISTS idx_raw_evidence_candidates_trace
            ON raw_evidence_candidates(trace_id);
        CREATE INDEX IF NOT EXISTS idx_raw_evidence_candidates_cluster
            ON raw_evidence_candidates(cluster_key);
        CREATE INDEX IF NOT EXISTS idx_card_ingest_jobs_status
            ON card_ingest_jobs(status, next_run_at, lease_until);
        CREATE INDEX IF NOT EXISTS idx_card_ingest_jobs_trace
            ON card_ingest_jobs(trace_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_canonical_staged_exact_claim
            ON canonical_staged_cards(exact_span_key, claim_key);
        CREATE INDEX IF NOT EXISTS idx_canonical_staged_claim
            ON canonical_staged_cards(claim_key);
        CREATE INDEX IF NOT EXISTS idx_canonical_staged_cluster
            ON canonical_staged_cards(cluster_key);
        CREATE INDEX IF NOT EXISTS idx_canonical_staged_entities
            ON canonical_staged_cards(slot_id, entities_key, source_scope);
        CREATE INDEX IF NOT EXISTS idx_canonical_staged_status
            ON canonical_staged_cards(status, updated_at);
        CREATE INDEX IF NOT EXISTS idx_staged_card_events_trace
            ON staged_card_events(trace_id, created_at);
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![ONLINE_INGEST_SCHEMA_VERSION, now_rfc3339()],
    )?;
    Ok(())
}

pub fn create_online_evidence_card_update_request(
    conn: &Connection,
    input: OnlineEvidenceCardUpdateRequestInput,
) -> Result<OnlineEvidenceCardUpdateRequestRecord> {
    init_schema(conn)?;
    let trace_id = bounded_optional_text(&input.trace_id, 160)
        .ok_or_else(|| anyhow!("online evidence card trace_id is required"))?;
    let resolved_question = bounded_optional_text(&input.resolved_question, 2000)
        .ok_or_else(|| anyhow!("online evidence card resolved_question is required"))?;
    let coverage_gap_reason = bounded_optional_text(&input.coverage_gap_reason, 240)
        .ok_or_else(|| anyhow!("online evidence card coverage_gap_reason is required"))?;
    let question_frame = input
        .question_frame
        .map(|value| canonical_json_value(&value));
    let source_scope_policy = canonical_json_value(&input.source_scope_policy);
    let recall_advice_ref = input
        .recall_advice_ref
        .and_then(|value| bounded_optional_text(&value, 240));
    let session_id = input
        .session_id
        .and_then(|value| bounded_optional_text(&value, 180));
    let idempotency_key = stable_hash(&json!({
        "trace_id": &trace_id,
        "session_id": &session_id,
        "resolved_question": &resolved_question,
        "question_frame": &question_frame,
        "coverage_gap_reason": &coverage_gap_reason,
        "source_scope_policy": &source_scope_policy,
        "recall_advice_ref": &recall_advice_ref,
    }))?;
    if let Some(existing) = load_update_request_by_idempotency(conn, &idempotency_key)? {
        ensure_card_ingest_job_for_request(conn, &existing)?;
        return Ok(existing);
    }
    let update_request_id = format!("ecur-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO online_evidence_card_update_requests (
            update_request_id, idempotency_key, trace_id, session_id,
            resolved_question, question_frame_json, coverage_gap_reason,
            source_scope_policy_json, recall_advice_ref, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'queued', ?10, ?10)
        "#,
        params![
            &update_request_id,
            &idempotency_key,
            &trace_id,
            &session_id,
            &resolved_question,
            optional_json_text(question_frame.as_ref())?,
            &coverage_gap_reason,
            serde_json::to_string(&source_scope_policy)?,
            &recall_advice_ref,
            &now,
        ],
    )?;
    append_runtime_audit_event(
        conn,
        &trace_id,
        "online_evidence_card_update_requested",
        &json!({
            "update_request_id": &update_request_id,
            "status": "queued",
            "coverage_gap_reason": &coverage_gap_reason,
            "idempotency_key_sha256": hash_text(&idempotency_key),
            "recall_advice_ref": &recall_advice_ref,
        }),
    )?;
    let request = load_update_request(conn, &update_request_id)?
        .ok_or_else(|| anyhow!("online evidence card update request unreadable after insert"))?;
    ensure_card_ingest_job_for_request(conn, &request)?;
    Ok(request)
}

fn ensure_card_ingest_job_for_request(
    conn: &Connection,
    request: &OnlineEvidenceCardUpdateRequestRecord,
) -> Result<CardIngestJobRecord> {
    if let Some(existing) = load_card_ingest_job_by_request(conn, &request.update_request_id)? {
        return Ok(existing);
    }
    let now = now_rfc3339();
    let job_id = format!(
        "ecj-{}",
        &stable_hash(&json!({
            "update_request_id": &request.update_request_id,
            "trace_id": &request.trace_id,
        }))?[..32]
    );
    conn.execute(
        r#"
        INSERT OR IGNORE INTO card_ingest_jobs (
            job_id, update_request_id, trace_id, status, stage,
            leased_by, lease_until, heartbeat_at, attempt_count, max_attempts,
            next_run_at, last_error, candidate_id, staged_card_id, created_at, updated_at
        ) VALUES (?1, ?2, ?3, 'queued', 'request_queued',
                  NULL, NULL, NULL, 0, ?4, ?5, NULL, NULL, NULL, ?5, ?5)
        "#,
        params![
            &job_id,
            &request.update_request_id,
            &request.trace_id,
            CARD_INGEST_JOB_MAX_ATTEMPTS,
            &now,
        ],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: &request.trace_id,
            update_request_id: Some(&request.update_request_id),
            staged_card_id: None,
            candidate_id: None,
            event_type: "card_ingest_job_created",
            from_status: None,
            to_status: Some("queued"),
            reason_code: "update_request_queued",
            rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
            payload: &json!({"job_id": &job_id}),
        },
    )?;
    load_card_ingest_job(conn, &job_id)?
        .ok_or_else(|| anyhow!("card ingest job unreadable after insert"))
}

fn reconcile_card_ingest_jobs(conn: &Connection) -> Result<usize> {
    let mut repaired = 0;
    let mut stmt = conn.prepare(
        r#"
        SELECT update_request_id, trace_id, session_id, resolved_question,
               question_frame_json, coverage_gap_reason, source_scope_policy_json,
               recall_advice_ref, status, created_at, updated_at
        FROM online_evidence_card_update_requests
        WHERE status IN ('queued', 'processing')
          AND update_request_id NOT IN (SELECT update_request_id FROM card_ingest_jobs)
        ORDER BY created_at, update_request_id
        "#,
    )?;
    let missing = query_update_request_rows(&mut stmt, [])?;
    for request in missing {
        ensure_card_ingest_job_for_request(conn, &request)?;
        repaired += 1;
    }

    let expired_jobs = load_expired_processing_jobs(conn)?;
    for job in expired_jobs {
        let previous = job.status.clone();
        let now = now_rfc3339();
        conn.execute(
            r#"
            UPDATE card_ingest_jobs
            SET status = 'queued',
                stage = 'lease_expired',
                leased_by = NULL,
                lease_until = NULL,
                heartbeat_at = NULL,
                updated_at = ?2
            WHERE job_id = ?1 AND status = 'processing'
            "#,
            params![&job.job_id, &now],
        )?;
        conn.execute(
            "UPDATE online_evidence_card_update_requests SET status = 'queued', updated_at = ?2 WHERE update_request_id = ?1 AND status = 'processing'",
            params![&job.update_request_id, &now],
        )?;
        insert_staged_card_event(
            conn,
            StagedCardEventInput {
                trace_id: &job.trace_id,
                update_request_id: Some(&job.update_request_id),
                staged_card_id: job.staged_card_id.as_deref(),
                candidate_id: job.candidate_id.as_deref(),
                event_type: "card_ingest_job_recovered",
                from_status: Some(&previous),
                to_status: Some("queued"),
                reason_code: "lease_expired",
                rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
                payload: &json!({"job_id": &job.job_id}),
            },
        )?;
        repaired += 1;
    }

    let retry_ready_jobs = load_retry_ready_jobs(conn)?;
    for job in retry_ready_jobs {
        let previous = job.status.clone();
        let now = now_rfc3339();
        conn.execute(
            r#"
            UPDATE card_ingest_jobs
            SET status = 'queued',
                stage = 'retry_ready',
                updated_at = ?2
            WHERE job_id = ?1 AND status = 'retry_wait'
            "#,
            params![&job.job_id, &now],
        )?;
        insert_staged_card_event(
            conn,
            StagedCardEventInput {
                trace_id: &job.trace_id,
                update_request_id: Some(&job.update_request_id),
                staged_card_id: job.staged_card_id.as_deref(),
                candidate_id: job.candidate_id.as_deref(),
                event_type: "card_ingest_job_recovered",
                from_status: Some(&previous),
                to_status: Some("queued"),
                reason_code: "retry_backoff_elapsed",
                rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
                payload: &json!({"job_id": &job.job_id}),
            },
        )?;
        repaired += 1;
    }
    Ok(repaired)
}

fn lease_card_ingest_jobs(
    conn: &Connection,
    actor: &str,
    limit: usize,
) -> Result<Vec<CardIngestJobRecord>> {
    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
    let now = now_rfc3339();
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id
        FROM card_ingest_jobs
        WHERE status = 'queued'
          AND next_run_at <= ?1
        ORDER BY next_run_at, created_at, job_id
        LIMIT ?2
        "#,
    )?;
    let ids = stmt
        .query_map(params![&now, limit_i64], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let lease_until = rfc3339_after_seconds(CARD_INGEST_JOB_LEASE_SECS);
    let mut jobs = Vec::new();
    for id in ids {
        let updated = conn.execute(
            r#"
            UPDATE card_ingest_jobs
            SET status = 'processing',
                stage = 'worker_processing',
                leased_by = ?2,
                lease_until = ?3,
                heartbeat_at = ?4,
                attempt_count = attempt_count + 1,
                updated_at = ?4
            WHERE job_id = ?1 AND status = 'queued'
            "#,
            params![&id, actor, &lease_until, &now],
        )?;
        if updated == 0 {
            continue;
        }
        let Some(job) = load_card_ingest_job(conn, &id)? else {
            continue;
        };
        conn.execute(
            "UPDATE online_evidence_card_update_requests SET status = 'processing', updated_at = ?2 WHERE update_request_id = ?1",
            params![&job.update_request_id, &now],
        )?;
        insert_staged_card_event(
            conn,
            StagedCardEventInput {
                trace_id: &job.trace_id,
                update_request_id: Some(&job.update_request_id),
                staged_card_id: job.staged_card_id.as_deref(),
                candidate_id: job.candidate_id.as_deref(),
                event_type: "card_ingest_job_leased",
                from_status: Some("queued"),
                to_status: Some("processing"),
                reason_code: "worker_lease_acquired",
                rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
                payload: &json!({
                    "job_id": &job.job_id,
                    "actor_sha256": hash_text(actor),
                    "lease_until": &job.lease_until,
                    "attempt_count": job.attempt_count,
                }),
            },
        )?;
        jobs.push(job);
    }
    Ok(jobs)
}

pub fn heartbeat_card_ingest_job(conn: &Connection, job_id: &str, actor: &str) -> Result<bool> {
    let now = now_rfc3339();
    let lease_until = rfc3339_after_seconds(CARD_INGEST_JOB_LEASE_SECS);
    let updated = conn.execute(
        r#"
        UPDATE card_ingest_jobs
        SET heartbeat_at = ?3,
            lease_until = ?4,
            updated_at = ?3
        WHERE job_id = ?1
          AND status = 'processing'
          AND leased_by = ?2
        "#,
        params![job_id, actor, &now, &lease_until],
    )?;
    Ok(updated == 1)
}

pub fn run_online_evidence_card_worker_once(
    conn: &Connection,
    input: OnlineEvidenceCardWorkerRunInput,
) -> Result<OnlineEvidenceCardWorkerRunReport> {
    init_schema(conn)?;
    let actor = bounded_optional_text(&input.actor, 120)
        .ok_or_else(|| anyhow!("online evidence card worker actor is required"))?;
    let limit = input.limit.clamp(1, 100);
    let retrieval_limit = input.retrieval_limit.clamp(1, 64);
    reconcile_card_ingest_jobs(conn)?;
    let jobs = lease_card_ingest_jobs(conn, &actor, limit)?;
    let mut report = OnlineEvidenceCardWorkerRunReport {
        object: "tonglingyu.online_evidence_card_worker_run".to_string(),
        schema_version: ONLINE_INGEST_SCHEMA_VERSION.to_string(),
        actor: actor.clone(),
        processed_count: 0,
        raw_candidate_count: 0,
        staged_count: 0,
        promoted_count: 0,
        conflicted_count: 0,
        failed_count: 0,
    };
    for job in jobs {
        report.processed_count += 1;
        let request = match load_update_request(conn, &job.update_request_id)? {
            Some(request) => request,
            None => {
                report.failed_count += 1;
                fail_card_ingest_job(conn, &job, "update_request_missing")?;
                continue;
            }
        };
        heartbeat_card_ingest_job(conn, &job.job_id, &actor)?;
        match process_update_request(conn, &job, &request, retrieval_limit) {
            Ok(processed) => {
                report.raw_candidate_count += processed.raw_candidate_count;
                report.staged_count += processed.staged_count;
                report.promoted_count += processed.promoted_count;
                report.conflicted_count += processed.conflicted_count;
                complete_card_ingest_job(conn, &job, &request)?;
            }
            Err(error) => {
                report.failed_count += 1;
                fail_card_ingest_job(conn, &job, &error.to_string())?;
            }
        }
    }
    Ok(report)
}

pub fn list_online_evidence_card_raw_candidates_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<RawEvidenceCandidateRecord>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
    let mut stmt = conn.prepare(
        r#"
        SELECT candidate_id, update_request_id, trace_id, source_id, source_layer,
               source_hash, span_start, span_end, matched_terms_json, query_frame_json,
               rule_gap_reason, cluster_key, created_at
        FROM raw_evidence_candidates
        WHERE trace_id = ?1
        ORDER BY created_at, candidate_id
        LIMIT ?2
        "#,
    )?;
    query_raw_candidate_rows(&mut stmt, params![trace_id, limit])
}

pub fn list_online_evidence_card_update_requests_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
    let mut stmt = conn.prepare(
        r#"
        SELECT update_request_id, trace_id, session_id, resolved_question,
               question_frame_json, coverage_gap_reason, source_scope_policy_json,
               recall_advice_ref, status, created_at, updated_at
        FROM online_evidence_card_update_requests
        WHERE trace_id = ?1
        ORDER BY created_at, update_request_id
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![trace_id, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
        ))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|row| {
            let question_frame = row
                .4
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose()?;
            Ok(json!({
                "update_request_id": row.0,
                "trace_id": row.1,
                "session_id": row.2,
                "resolved_question": row.3,
                "question_frame": question_frame,
                "coverage_gap_reason": row.5,
                "source_scope_policy": serde_json::from_str::<Value>(&row.6)?,
                "recall_advice_ref": row.7,
                "status": row.8,
                "created_at": row.9,
                "updated_at": row.10,
            }))
        })
        .collect()
}

pub fn list_online_evidence_card_jobs_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<CardIngestJobRecord>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, update_request_id, trace_id, status, stage,
               leased_by, lease_until, heartbeat_at, attempt_count, max_attempts,
               next_run_at, last_error, candidate_id, staged_card_id, created_at, updated_at
        FROM card_ingest_jobs
        WHERE trace_id = ?1
        ORDER BY created_at, job_id
        LIMIT ?2
        "#,
    )?;
    query_card_ingest_job_rows(&mut stmt, params![trace_id, limit])
}

pub fn list_online_evidence_card_staged_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<CanonicalStagedCardRecord>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
    let mut stmt = conn.prepare(
        r#"
        SELECT staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
               slot_id, entities_key, entities_json, polarity, modality,
               evidence_strength, supporting_spans_json, evidence_json, schema_version,
               source_corpus_version, source_hash, rules_version, builder_version,
               validator_version, status, promoted_evidence_id, created_from_trace_ids_json,
               created_at, updated_at
        FROM canonical_staged_cards
        WHERE created_from_trace_ids_json LIKE ?1
        ORDER BY updated_at, staged_card_id
        LIMIT ?2
        "#,
    )?;
    let pattern = format!("%{}%", escape_like(trace_id));
    query_staged_card_rows(&mut stmt, params![pattern, limit])
}

pub fn list_online_evidence_card_events_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 1000)).unwrap_or(1000);
    let mut stmt = conn.prepare(
        r#"
        SELECT event_id, trace_id, update_request_id, staged_card_id, candidate_id,
               event_type, from_status, to_status, reason_code, rule_id, payload_json, created_at
        FROM staged_card_events
        WHERE trace_id = ?1
        ORDER BY created_at, event_id
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![trace_id, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11)?,
        ))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|row| {
            Ok(json!({
                "event_id": row.0,
                "trace_id": row.1,
                "update_request_id": row.2,
                "staged_card_id": row.3,
                "candidate_id": row.4,
                "event_type": row.5,
                "from_status": row.6,
                "to_status": row.7,
                "reason_code": row.8,
                "rule_id": row.9,
                "payload": serde_json::from_str::<Value>(&row.10)?,
                "created_at": row.11,
            }))
        })
        .collect()
}

struct ProcessedUpdateRequest {
    raw_candidate_count: usize,
    staged_count: usize,
    promoted_count: usize,
    conflicted_count: usize,
}

fn process_update_request(
    conn: &Connection,
    job: &CardIngestJobRecord,
    request: &OnlineEvidenceCardUpdateRequestRecord,
    retrieval_limit: usize,
) -> Result<ProcessedUpdateRequest> {
    let frame = request
        .question_frame
        .as_ref()
        .and_then(question_frame::parse_runtime_question_frame);
    let required_evidence_types = frame
        .as_ref()
        .map(|frame| frame.required_evidence_types.clone())
        .unwrap_or_default();
    let search = search_evidence_result(
        conn,
        &request.resolved_question,
        retrieval_limit,
        &required_evidence_types,
    )?;
    let mut processed = ProcessedUpdateRequest {
        raw_candidate_count: 0,
        staged_count: 0,
        promoted_count: 0,
        conflicted_count: 0,
    };
    for card in search.cards {
        let source_hash = source_hash_for_card(conn, &card)?;
        if let Some(candidate) =
            stage_candidate_from_frame(request, frame.as_ref(), card.clone(), source_hash.clone())?
        {
            let staged = stage_evidence_card_candidate(conn, candidate)?;
            record_card_ingest_job_stage_ref(conn, job, None, Some(&staged.staged_card_id))?;
            if staged.status == "conflicted" {
                processed.conflicted_count += 1;
                continue;
            }
            processed.staged_count += 1;
            let promoted = validate_and_promote_staged_card(conn, &staged.staged_card_id)?;
            if promoted
                .as_ref()
                .is_some_and(|record| record.status == "promoted")
            {
                processed.promoted_count += 1;
            }
        } else {
            let raw_candidate = insert_raw_candidate_for_card(
                conn,
                request,
                &card,
                &source_hash,
                "rule_gap_no_supported_card_assertion",
            )?;
            record_card_ingest_job_stage_ref(conn, job, Some(&raw_candidate.candidate_id), None)?;
            processed.raw_candidate_count += 1;
        }
    }
    Ok(processed)
}

fn stage_candidate_from_frame(
    request: &OnlineEvidenceCardUpdateRequestRecord,
    frame: Option<&question_frame::RuntimeQuestionFrame>,
    card: EvidenceCard,
    source_hash: String,
) -> Result<Option<StageCandidateInput>> {
    let Some(frame) = frame.filter(|frame| frame.has_relation_object()) else {
        return Ok(None);
    };
    let Some(groups) = relation_support_terms(frame) else {
        return Ok(None);
    };
    if !relation_text_matches_support_terms(&card.text, &groups) {
        return Ok(None);
    }
    let Some(subject) = &frame.subject else {
        return Ok(None);
    };
    let Some(predicate) = &frame.predicate else {
        return Ok(None);
    };
    let Some(object) = &frame.object else {
        return Ok(None);
    };
    let entities = canonical_json_value(&json!([
        {"role": "subject", "canonical": subject.canonical, "aliases": subject.aliases},
        {"role": "object", "canonical": object.canonical, "aliases": object.aliases}
    ]));
    let entities_key = stable_hash(&entities)?;
    let mut matched_terms = Vec::new();
    for term in groups
        .subject
        .iter()
        .chain(groups.predicate.iter())
        .chain(groups.object.iter())
    {
        if card.text.contains(term)
            || crate::normalize_text(&card.text).contains(&crate::normalize_text(term))
        {
            push_unique(&mut matched_terms, term);
        }
    }
    Ok(Some(StageCandidateInput {
        update_request_id: request.update_request_id.clone(),
        trace_id: request.trace_id.clone(),
        source_hash,
        source_scope: "question_frame_relation_scope".to_string(),
        slot_id: predicate.id.clone(),
        entities,
        entities_key,
        polarity: "supports".to_string(),
        modality: "direct_textual_relation".to_string(),
        evidence_strength: "direct".to_string(),
        rules_version: ONLINE_INGEST_SCHEMA_VERSION.to_string(),
        card,
        matched_terms,
    }))
}

fn stage_evidence_card_candidate(
    conn: &Connection,
    input: StageCandidateInput,
) -> Result<CanonicalStagedCardRecord> {
    let span = support_span_for_card(&input.card, &input.source_hash, &input.matched_terms);
    let exact_span_key = stable_hash(&json!({
        "source_id": &input.card.source_id,
        "source_hash": &input.source_hash,
        "block_id": &input.card.block_id,
        "span_start": span["span_start"],
        "span_end": span["span_end"],
        "card_schema_version": ONLINE_CARD_SCHEMA_VERSION,
    }))?;
    let claim_key = stable_hash(&json!({
        "source_scope": &input.source_scope,
        "slot_id": &input.slot_id,
        "entities": &input.entities,
        "polarity": &input.polarity,
        "modality": &input.modality,
        "evidence_strength": &input.evidence_strength,
        "rules_version": &input.rules_version,
    }))?;
    let cluster_key = stable_hash(&json!({
        "source_scope": &input.source_scope,
        "source_id": &input.card.source_id,
        "slot_id": &input.slot_id,
        "entities": &input.entities,
    }))?;
    if let Some(existing) = load_staged_card_by_exact_claim(conn, &exact_span_key, &claim_key)? {
        return merge_staged_card(
            conn,
            existing,
            &input,
            span,
            StagedCardMergeReason::SameExactSpanAndClaim,
        );
    }
    if canonical_entity_participants_key(&input.entities).is_none() {
        return insert_needs_disambiguation_staged_card(
            conn,
            input,
            exact_span_key,
            claim_key,
            cluster_key,
            span,
            "entity_resolution_conflict",
        );
    }
    if let Some(existing) = load_staged_card_by_claim(conn, &claim_key)? {
        if staged_source_hash_conflicts(&existing, &input) {
            return insert_conflicted_staged_card(
                conn,
                input,
                exact_span_key,
                claim_key,
                cluster_key,
                span,
                &existing,
                "source_hash_conflict",
            );
        }
        if existing.status == "promoted" {
            return insert_superseded_staged_card(
                conn,
                input,
                exact_span_key,
                claim_key,
                cluster_key,
                span,
                &existing,
            );
        }
        return merge_staged_card(
            conn,
            existing,
            &input,
            span,
            StagedCardMergeReason::SameClaim,
        );
    }
    if let Some(conflict) = first_role_conflict(conn, &input)? {
        return insert_conflicted_staged_card(
            conn,
            input,
            exact_span_key,
            claim_key,
            cluster_key,
            span,
            &conflict,
            "role_conflict",
        );
    }
    if let Some(conflict) = first_claim_conflict(
        conn,
        &input.slot_id,
        &input.entities_key,
        &input.source_scope,
        &claim_key,
    )? {
        return insert_conflicted_staged_card(
            conn,
            input,
            exact_span_key,
            claim_key,
            cluster_key,
            span,
            &conflict,
            "claim_dimension_conflict",
        );
    }
    insert_staged_card(conn, input, exact_span_key, claim_key, cluster_key, span)
}

fn insert_staged_card(
    conn: &Connection,
    mut input: StageCandidateInput,
    exact_span_key: String,
    claim_key: String,
    cluster_key: String,
    span: Value,
) -> Result<CanonicalStagedCardRecord> {
    let staged_card_id = format!("esc-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    let trace_ids = vec![input.trace_id.clone()];
    input.card.evidence_id = promoted_evidence_id(&exact_span_key, &claim_key);
    conn.execute(
        r#"
        INSERT INTO canonical_staged_cards (
            staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
            slot_id, entities_key, entities_json, polarity, modality, evidence_strength,
            supporting_spans_json, evidence_json, schema_version, source_corpus_version,
            source_hash, rules_version, builder_version, validator_version, status,
            promoted_evidence_id, created_from_trace_ids_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18, ?19, 'staged', NULL, ?20, ?21, ?21)
        "#,
        params![
            &staged_card_id,
            &exact_span_key,
            &claim_key,
            &cluster_key,
            &input.source_scope,
            &input.slot_id,
            &input.entities_key,
            serde_json::to_string(&input.entities)?,
            &input.polarity,
            &input.modality,
            &input.evidence_strength,
            serde_json::to_string(&vec![span.clone()])?,
            serde_json::to_string(&input.card)?,
            ONLINE_CARD_SCHEMA_VERSION,
            crate::KNOWLEDGE_BASE_SCHEMA_VERSION,
            &input.source_hash,
            &input.rules_version,
            ONLINE_CARD_BUILDER_VERSION,
            ONLINE_CARD_VALIDATOR_VERSION,
            serde_json::to_string(&trace_ids)?,
            &now,
        ],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: &input.trace_id,
            update_request_id: Some(&input.update_request_id),
            staged_card_id: Some(&staged_card_id),
            candidate_id: None,
            event_type: "staged_card_created",
            from_status: None,
            to_status: Some("staged"),
            reason_code: "claim_supported_by_local_span",
            rule_id: Some(&input.rules_version),
            payload: &json!({
                "exact_span_key": &exact_span_key,
                "claim_key": &claim_key,
                "cluster_key": &cluster_key,
            }),
        },
    )?;
    load_staged_card(conn, &staged_card_id)?
        .ok_or_else(|| anyhow!("staged card unreadable after insert"))
}

fn insert_conflicted_staged_card(
    conn: &Connection,
    input: StageCandidateInput,
    exact_span_key: String,
    claim_key: String,
    cluster_key: String,
    span: Value,
    conflict: &CanonicalStagedCardRecord,
    reason_code: &'static str,
) -> Result<CanonicalStagedCardRecord> {
    let update_request_id = input.update_request_id.clone();
    let mut record = insert_staged_card(conn, input, exact_span_key, claim_key, cluster_key, span)?;
    let previous = record.status.clone();
    let now = now_rfc3339();
    conn.execute(
        "UPDATE canonical_staged_cards SET status = 'conflicted', updated_at = ?2 WHERE staged_card_id = ?1",
        params![&record.staged_card_id, &now],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: record
                .created_from_trace_ids
                .first()
                .map(String::as_str)
                .unwrap_or("online-evidence-card"),
            update_request_id: Some(&update_request_id),
            staged_card_id: Some(&record.staged_card_id),
            candidate_id: None,
            event_type: "staged_card_conflicted",
            from_status: Some(&previous),
            to_status: Some("conflicted"),
            reason_code,
            rule_id: Some(&record.rules_version),
            payload: &json!({
                "conflicting_staged_card_id": &conflict.staged_card_id,
                "conflicting_claim_key": &conflict.claim_key,
                "slot_id": &record.slot_id,
                "entities_key": &record.entities_key,
            }),
        },
    )?;
    record.status = "conflicted".to_string();
    record.updated_at = now;
    Ok(record)
}

fn insert_needs_disambiguation_staged_card(
    conn: &Connection,
    input: StageCandidateInput,
    exact_span_key: String,
    claim_key: String,
    cluster_key: String,
    span: Value,
    reason_code: &'static str,
) -> Result<CanonicalStagedCardRecord> {
    let update_request_id = input.update_request_id.clone();
    let mut record = insert_staged_card(conn, input, exact_span_key, claim_key, cluster_key, span)?;
    let previous = record.status.clone();
    let now = now_rfc3339();
    conn.execute(
        "UPDATE canonical_staged_cards SET status = 'needs_disambiguation', updated_at = ?2 WHERE staged_card_id = ?1",
        params![&record.staged_card_id, &now],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: record
                .created_from_trace_ids
                .first()
                .map(String::as_str)
                .unwrap_or("online-evidence-card"),
            update_request_id: Some(&update_request_id),
            staged_card_id: Some(&record.staged_card_id),
            candidate_id: None,
            event_type: "staged_card_needs_disambiguation",
            from_status: Some(&previous),
            to_status: Some("needs_disambiguation"),
            reason_code,
            rule_id: Some(&record.rules_version),
            payload: &json!({
                "claim_key": &record.claim_key,
                "entities_key": &record.entities_key,
            }),
        },
    )?;
    record.status = "needs_disambiguation".to_string();
    record.updated_at = now;
    Ok(record)
}

fn insert_superseded_staged_card(
    conn: &Connection,
    input: StageCandidateInput,
    exact_span_key: String,
    claim_key: String,
    cluster_key: String,
    span: Value,
    promoted: &CanonicalStagedCardRecord,
) -> Result<CanonicalStagedCardRecord> {
    let update_request_id = input.update_request_id.clone();
    let mut record = insert_staged_card(conn, input, exact_span_key, claim_key, cluster_key, span)?;
    let previous = record.status.clone();
    let now = now_rfc3339();
    conn.execute(
        r#"
        UPDATE canonical_staged_cards
        SET status = 'superseded_by_promoted',
            promoted_evidence_id = ?2,
            updated_at = ?3
        WHERE staged_card_id = ?1
        "#,
        params![&record.staged_card_id, &promoted.promoted_evidence_id, &now,],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: record
                .created_from_trace_ids
                .first()
                .map(String::as_str)
                .unwrap_or("online-evidence-card"),
            update_request_id: Some(&update_request_id),
            staged_card_id: Some(&record.staged_card_id),
            candidate_id: None,
            event_type: "staged_card_superseded_by_promoted",
            from_status: Some(&previous),
            to_status: Some("superseded_by_promoted"),
            reason_code: "same_claim_already_promoted",
            rule_id: Some(&record.rules_version),
            payload: &json!({
                "promoted_staged_card_id": &promoted.staged_card_id,
                "promoted_evidence_id": &promoted.promoted_evidence_id,
                "claim_key": &record.claim_key,
            }),
        },
    )?;
    record.status = "superseded_by_promoted".to_string();
    record.promoted_evidence_id = promoted.promoted_evidence_id.clone();
    record.updated_at = now;
    Ok(record)
}

enum StagedCardMergeReason {
    SameExactSpanAndClaim,
    SameClaim,
}

fn merge_staged_card(
    conn: &Connection,
    mut existing: CanonicalStagedCardRecord,
    input: &StageCandidateInput,
    span: Value,
    reason: StagedCardMergeReason,
) -> Result<CanonicalStagedCardRecord> {
    let previous = existing.status.clone();
    append_unique_json(&mut existing.supporting_spans, span);
    push_unique(&mut existing.created_from_trace_ids, &input.trace_id);
    if should_replace_canonical_evidence(&existing.evidence, &input.card) {
        existing.evidence = input.card.clone();
    }
    let next_status = match previous.as_str() {
        "promoted" => "promoted",
        "conflicted" | "needs_disambiguation" | "superseded_by_promoted" | "rejected" => {
            previous.as_str()
        }
        _ => "merged",
    };
    let reason_code = match reason {
        StagedCardMergeReason::SameExactSpanAndClaim => "same_exact_span_and_claim",
        StagedCardMergeReason::SameClaim => "same_claim",
    };
    let now = now_rfc3339();
    conn.execute(
        r#"
        UPDATE canonical_staged_cards
        SET supporting_spans_json = ?2,
            created_from_trace_ids_json = ?3,
            status = ?4,
            evidence_json = ?5,
            updated_at = ?6
        WHERE staged_card_id = ?1
        "#,
        params![
            &existing.staged_card_id,
            serde_json::to_string(&existing.supporting_spans)?,
            serde_json::to_string(&existing.created_from_trace_ids)?,
            next_status,
            serde_json::to_string(&existing.evidence)?,
            &now,
        ],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: &input.trace_id,
            update_request_id: Some(&input.update_request_id),
            staged_card_id: Some(&existing.staged_card_id),
            candidate_id: None,
            event_type: "staged_card_merged",
            from_status: Some(&previous),
            to_status: Some(next_status),
            reason_code,
            rule_id: Some(&existing.rules_version),
            payload: &json!({
                "supporting_span_count": existing.supporting_spans.len(),
                "trace_count": existing.created_from_trace_ids.len(),
                "source_hash": &input.source_hash,
            }),
        },
    )?;
    existing.status = next_status.to_string();
    existing.updated_at = now;
    Ok(existing)
}

fn should_replace_canonical_evidence(existing: &EvidenceCard, candidate: &EvidenceCard) -> bool {
    existing.source_id == candidate.source_id
        && existing.block_id == candidate.block_id
        && candidate.text.chars().count() > existing.text.chars().count()
}

fn validate_and_promote_staged_card(
    conn: &Connection,
    staged_card_id: &str,
) -> Result<Option<CanonicalStagedCardRecord>> {
    let Some(mut record) = load_staged_card(conn, staged_card_id)? else {
        return Ok(None);
    };
    if !matches!(record.status.as_str(), "staged" | "merged" | "validated") {
        return Ok(Some(record));
    }
    let trace_id = record
        .created_from_trace_ids
        .first()
        .map(String::as_str)
        .unwrap_or("online-evidence-card");
    let previous = record.status.clone();
    let now = now_rfc3339();
    conn.execute(
        "UPDATE canonical_staged_cards SET status = 'validated', updated_at = ?2 WHERE staged_card_id = ?1",
        params![&record.staged_card_id, &now],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id,
            update_request_id: None,
            staged_card_id: Some(&record.staged_card_id),
            candidate_id: None,
            event_type: "staged_card_validated",
            from_status: Some(&previous),
            to_status: Some("validated"),
            reason_code: "local_validator_passed",
            rule_id: Some(&record.validator_version),
            payload: &json!({
                "claim_key": &record.claim_key,
                "exact_span_key": &record.exact_span_key,
            }),
        },
    )?;
    record.status = "validated".to_string();
    let promoted_id = record
        .promoted_evidence_id
        .clone()
        .unwrap_or_else(|| promoted_evidence_id(&record.exact_span_key, &record.claim_key));
    let mut evidence = record.evidence.clone();
    evidence.evidence_id = promoted_id.clone();
    evidence.verification_status = "online_promoted_source_backed".to_string();
    conn.execute(
        r#"
        INSERT OR IGNORE INTO evidence_cards (
            evidence_id, package_id, evidence_type, source_id, block_id, support_scope,
            unsupported_scope, evidence_level, confidence, verification_status,
            evidence_json, created_at
        ) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
        params![
            &promoted_id,
            &evidence.evidence_type,
            &evidence.source_id,
            &evidence.block_id,
            &evidence.support_scope,
            &evidence.unsupported_scope,
            &evidence.evidence_level,
            &evidence.confidence,
            &evidence.verification_status,
            serde_json::to_string(&evidence)?,
            &now,
        ],
    )?;
    conn.execute(
        r#"
        UPDATE canonical_staged_cards
        SET status = 'promoted',
            promoted_evidence_id = ?2,
            evidence_json = ?3,
            updated_at = ?4
        WHERE staged_card_id = ?1
        "#,
        params![
            &record.staged_card_id,
            &promoted_id,
            serde_json::to_string(&evidence)?,
            &now,
        ],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id,
            update_request_id: None,
            staged_card_id: Some(&record.staged_card_id),
            candidate_id: None,
            event_type: "staged_card_promoted",
            from_status: Some("validated"),
            to_status: Some("promoted"),
            reason_code: "promoted_to_evidence_card_store",
            rule_id: Some(&record.validator_version),
            payload: &json!({
                "promoted_evidence_id": &promoted_id,
                "package_id": Value::Null,
                "claim_key": &record.claim_key,
            }),
        },
    )?;
    append_runtime_audit_event(
        conn,
        trace_id,
        "online_evidence_card_promoted",
        &json!({
            "staged_card_id": &record.staged_card_id,
            "promoted_evidence_id": &promoted_id,
            "claim_key": &record.claim_key,
            "slot_id": &record.slot_id,
            "source_id": &record.evidence.source_id,
            "block_id": &record.evidence.block_id,
        }),
    )?;
    record.status = "promoted".to_string();
    record.promoted_evidence_id = Some(promoted_id);
    record.evidence = evidence;
    record.updated_at = now;
    Ok(Some(record))
}

fn insert_raw_candidate_for_card(
    conn: &Connection,
    request: &OnlineEvidenceCardUpdateRequestRecord,
    card: &EvidenceCard,
    source_hash: &str,
    rule_gap_reason: &str,
) -> Result<RawEvidenceCandidateRecord> {
    let source_layer = evidence_card_source_layer(card).to_string();
    let span_start = 0_i64;
    let span_end = i64::try_from(card.text.chars().count()).unwrap_or(i64::MAX);
    let query_frame = request.question_frame.clone().unwrap_or_else(
        || json!({"resolved_question_sha256": hash_text(&request.resolved_question)}),
    );
    let cluster_key = stable_hash(&json!({
        "source_layer": &source_layer,
        "source_id": &card.source_id,
        "source_hash": source_hash,
        "block_id": &card.block_id,
        "query_frame": &query_frame,
        "rule_gap_reason": rule_gap_reason,
    }))?;
    let candidate_id = format!(
        "raw-{}",
        &stable_hash(&json!({
            "update_request_id": &request.update_request_id,
            "source_id": &card.source_id,
            "source_hash": source_hash,
            "block_id": &card.block_id,
            "rule_gap_reason": rule_gap_reason,
        }))?[..32]
    );
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT OR IGNORE INTO raw_evidence_candidates (
            candidate_id, update_request_id, trace_id, source_id, source_layer,
            source_hash, span_start, span_end, matched_terms_json, query_frame_json,
            rule_gap_reason, cluster_key, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
        params![
            &candidate_id,
            &request.update_request_id,
            &request.trace_id,
            &card.source_id,
            &source_layer,
            source_hash,
            span_start,
            span_end,
            serde_json::to_string(&Vec::<String>::new())?,
            serde_json::to_string(&query_frame)?,
            rule_gap_reason,
            &cluster_key,
            &now,
        ],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: &request.trace_id,
            update_request_id: Some(&request.update_request_id),
            staged_card_id: None,
            candidate_id: Some(&candidate_id),
            event_type: "raw_evidence_candidate_recorded",
            from_status: None,
            to_status: Some("rule_gap"),
            reason_code: rule_gap_reason,
            rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
            payload: &json!({
                "source_id": &card.source_id,
                "block_id": &card.block_id,
                "source_layer": &source_layer,
                "cluster_key": &cluster_key,
            }),
        },
    )?;
    load_raw_candidate(conn, &candidate_id)?
        .ok_or_else(|| anyhow!("raw evidence candidate unreadable after insert"))
}

fn complete_card_ingest_job(
    conn: &Connection,
    job: &CardIngestJobRecord,
    request: &OnlineEvidenceCardUpdateRequestRecord,
) -> Result<()> {
    let now = now_rfc3339();
    conn.execute(
        r#"
        UPDATE card_ingest_jobs
        SET status = 'completed',
            stage = 'completed',
            leased_by = NULL,
            lease_until = NULL,
            heartbeat_at = NULL,
            last_error = NULL,
            updated_at = ?2
        WHERE job_id = ?1
        "#,
        params![&job.job_id, &now],
    )?;
    conn.execute(
        "UPDATE online_evidence_card_update_requests SET status = 'completed', updated_at = ?2 WHERE update_request_id = ?1",
        params![&request.update_request_id, &now],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: &request.trace_id,
            update_request_id: Some(&request.update_request_id),
            staged_card_id: None,
            candidate_id: None,
            event_type: "card_ingest_job_completed",
            from_status: Some("processing"),
            to_status: Some("completed"),
            reason_code: "job_completed",
            rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
            payload: &json!({"job_id": &job.job_id}),
        },
    )
}

fn fail_card_ingest_job(conn: &Connection, job: &CardIngestJobRecord, error: &str) -> Result<()> {
    let now = now_rfc3339();
    let terminal = job.attempt_count >= job.max_attempts;
    let next_status = if terminal {
        "dead_letter"
    } else {
        "retry_wait"
    };
    let next_run_at = if terminal {
        now.clone()
    } else {
        rfc3339_after_seconds(job_retry_backoff_secs(job.attempt_count))
    };
    let reason_code = if terminal {
        "max_attempts_exhausted"
    } else {
        "retry_scheduled"
    };
    conn.execute(
        r#"
        UPDATE card_ingest_jobs
        SET status = ?2,
            stage = ?3,
            leased_by = NULL,
            lease_until = NULL,
            heartbeat_at = NULL,
            next_run_at = ?4,
            last_error = ?5,
            updated_at = ?6
        WHERE job_id = ?1
        "#,
        params![
            &job.job_id,
            next_status,
            reason_code,
            &next_run_at,
            bounded_optional_text(error, 2000),
            &now,
        ],
    )?;
    conn.execute(
        "UPDATE online_evidence_card_update_requests SET status = ?2, updated_at = ?3 WHERE update_request_id = ?1",
        params![
            &job.update_request_id,
            if terminal { "failed" } else { "queued" },
            &now,
        ],
    )?;
    insert_staged_card_event(
        conn,
        StagedCardEventInput {
            trace_id: &job.trace_id,
            update_request_id: Some(&job.update_request_id),
            staged_card_id: job.staged_card_id.as_deref(),
            candidate_id: job.candidate_id.as_deref(),
            event_type: "card_ingest_job_failed",
            from_status: Some("processing"),
            to_status: Some(next_status),
            reason_code,
            rule_id: Some(ONLINE_INGEST_SCHEMA_VERSION),
            payload: &json!({
                "job_id": &job.job_id,
                "attempt_count": job.attempt_count,
                "max_attempts": job.max_attempts,
                "next_run_at": &next_run_at,
                "error_sha256": hash_text(error),
            }),
        },
    )
}

fn record_card_ingest_job_stage_ref(
    conn: &Connection,
    job: &CardIngestJobRecord,
    candidate_id: Option<&str>,
    staged_card_id: Option<&str>,
) -> Result<()> {
    let now = now_rfc3339();
    conn.execute(
        r#"
        UPDATE card_ingest_jobs
        SET candidate_id = COALESCE(?2, candidate_id),
            staged_card_id = COALESCE(?3, staged_card_id),
            stage = ?4,
            updated_at = ?5
        WHERE job_id = ?1
        "#,
        params![
            &job.job_id,
            candidate_id,
            staged_card_id,
            if staged_card_id.is_some() {
                "staged_card_observed"
            } else {
                "raw_candidate_observed"
            },
            &now,
        ],
    )?;
    Ok(())
}

fn first_claim_conflict(
    conn: &Connection,
    slot_id: &str,
    entities_key: &str,
    source_scope: &str,
    claim_key: &str,
) -> Result<Option<CanonicalStagedCardRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
               slot_id, entities_key, entities_json, polarity, modality,
               evidence_strength, supporting_spans_json, evidence_json, schema_version,
               source_corpus_version, source_hash, rules_version, builder_version,
               validator_version, status, promoted_evidence_id, created_from_trace_ids_json,
               created_at, updated_at
        FROM canonical_staged_cards
        WHERE slot_id = ?1
          AND entities_key = ?2
          AND source_scope = ?3
          AND claim_key <> ?4
          AND status IN ('staged', 'merged', 'validated', 'promoted')
        ORDER BY updated_at, staged_card_id
        LIMIT 1
        "#,
    )?;
    query_optional_staged_card(
        &mut stmt,
        params![slot_id, entities_key, source_scope, claim_key],
    )
}

fn first_role_conflict(
    conn: &Connection,
    input: &StageCandidateInput,
) -> Result<Option<CanonicalStagedCardRecord>> {
    let Some(input_participants_key) = canonical_entity_participants_key(&input.entities) else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        r#"
        SELECT staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
               slot_id, entities_key, entities_json, polarity, modality,
               evidence_strength, supporting_spans_json, evidence_json, schema_version,
               source_corpus_version, source_hash, rules_version, builder_version,
               validator_version, status, promoted_evidence_id, created_from_trace_ids_json,
               created_at, updated_at
        FROM canonical_staged_cards
        WHERE slot_id = ?1
          AND source_scope = ?2
          AND entities_key <> ?3
          AND status IN ('staged', 'merged', 'validated', 'promoted')
        ORDER BY updated_at, staged_card_id
        "#,
    )?;
    let candidates = query_staged_card_rows(
        &mut stmt,
        params![&input.slot_id, &input.source_scope, &input.entities_key],
    )?;
    Ok(candidates.into_iter().find(|record| {
        canonical_entity_participants_key(&record.entities).as_deref()
            == Some(input_participants_key.as_str())
    }))
}

fn canonical_entity_participants_key(entities: &Value) -> Option<String> {
    let mut participants = entities
        .as_array()?
        .iter()
        .filter_map(|entity| {
            entity
                .get("canonical")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    participants.sort();
    participants.dedup();
    if participants.len() < 2 {
        return None;
    }
    Some(participants.join("\u{1f}"))
}

fn load_staged_card_by_exact_claim(
    conn: &Connection,
    exact_span_key: &str,
    claim_key: &str,
) -> Result<Option<CanonicalStagedCardRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
               slot_id, entities_key, entities_json, polarity, modality,
               evidence_strength, supporting_spans_json, evidence_json, schema_version,
               source_corpus_version, source_hash, rules_version, builder_version,
               validator_version, status, promoted_evidence_id, created_from_trace_ids_json,
               created_at, updated_at
        FROM canonical_staged_cards
        WHERE exact_span_key = ?1 AND claim_key = ?2
        LIMIT 1
        "#,
    )?;
    query_optional_staged_card(&mut stmt, params![exact_span_key, claim_key])
}

fn load_staged_card_by_claim(
    conn: &Connection,
    claim_key: &str,
) -> Result<Option<CanonicalStagedCardRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
               slot_id, entities_key, entities_json, polarity, modality,
               evidence_strength, supporting_spans_json, evidence_json, schema_version,
               source_corpus_version, source_hash, rules_version, builder_version,
               validator_version, status, promoted_evidence_id, created_from_trace_ids_json,
               created_at, updated_at
        FROM canonical_staged_cards
        WHERE claim_key = ?1
          AND status IN ('staged', 'merged', 'validated', 'promoted')
        ORDER BY CASE status
            WHEN 'promoted' THEN 0
            WHEN 'validated' THEN 1
            WHEN 'merged' THEN 2
            ELSE 3
          END,
          updated_at,
          staged_card_id
        LIMIT 1
        "#,
    )?;
    query_optional_staged_card(&mut stmt, params![claim_key])
}

fn staged_source_hash_conflicts(
    existing: &CanonicalStagedCardRecord,
    input: &StageCandidateInput,
) -> bool {
    existing.evidence.source_id == input.card.source_id
        && existing.evidence.block_id == input.card.block_id
        && existing.source_hash != input.source_hash
}

fn load_staged_card(
    conn: &Connection,
    staged_card_id: &str,
) -> Result<Option<CanonicalStagedCardRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT staged_card_id, exact_span_key, claim_key, cluster_key, source_scope,
               slot_id, entities_key, entities_json, polarity, modality,
               evidence_strength, supporting_spans_json, evidence_json, schema_version,
               source_corpus_version, source_hash, rules_version, builder_version,
               validator_version, status, promoted_evidence_id, created_from_trace_ids_json,
               created_at, updated_at
        FROM canonical_staged_cards
        WHERE staged_card_id = ?1
        LIMIT 1
        "#,
    )?;
    query_optional_staged_card(&mut stmt, params![staged_card_id])
}

fn query_optional_staged_card<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Option<CanonicalStagedCardRecord>>
where
    P: rusqlite::Params,
{
    stmt.query_row(params, staged_card_sql_row)
        .optional()?
        .map(staged_card_from_sql_row)
        .transpose()
}

fn query_staged_card_rows<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<CanonicalStagedCardRecord>>
where
    P: rusqlite::Params,
{
    let rows = stmt.query_map(params, staged_card_sql_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(staged_card_from_sql_row)
        .collect()
}

#[allow(clippy::type_complexity)]
fn staged_card_sql_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
        row.get(22)?,
        row.get(23)?,
    ))
}

#[allow(clippy::type_complexity)]
fn staged_card_from_sql_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
    ),
) -> Result<CanonicalStagedCardRecord> {
    Ok(CanonicalStagedCardRecord {
        staged_card_id: row.0,
        exact_span_key: row.1,
        claim_key: row.2,
        cluster_key: row.3,
        source_scope: row.4,
        slot_id: row.5,
        entities_key: row.6,
        entities: serde_json::from_str(&row.7)?,
        polarity: row.8,
        modality: row.9,
        evidence_strength: row.10,
        supporting_spans: serde_json::from_str(&row.11)?,
        evidence: serde_json::from_str(&row.12)?,
        schema_version: row.13,
        source_corpus_version: row.14,
        source_hash: row.15,
        rules_version: row.16,
        builder_version: row.17,
        validator_version: row.18,
        status: row.19,
        promoted_evidence_id: row.20,
        created_from_trace_ids: serde_json::from_str(&row.21)?,
        created_at: row.22,
        updated_at: row.23,
    })
}

pub fn online_evidence_card_ingest_stats(conn: &Connection) -> Result<Value> {
    init_schema(conn)?;
    Ok(json!({
        "object": "tonglingyu.online_evidence_card_ingest_stats",
        "schema_version": ONLINE_INGEST_SCHEMA_VERSION,
        "update_requests": grouped_count_json(conn, "online_evidence_card_update_requests", "status")?,
        "jobs": grouped_count_json(conn, "card_ingest_jobs", "status")?,
        "staged_cards": grouped_count_json(conn, "canonical_staged_cards", "status")?,
        "raw_candidate_count": table_count(conn, "raw_evidence_candidates")?,
        "event_count": table_count(conn, "staged_card_events")?,
    }))
}

fn load_raw_candidate(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<RawEvidenceCandidateRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT candidate_id, update_request_id, trace_id, source_id, source_layer,
               source_hash, span_start, span_end, matched_terms_json, query_frame_json,
               rule_gap_reason, cluster_key, created_at
        FROM raw_evidence_candidates
        WHERE candidate_id = ?1
        "#,
    )?;
    let mut rows = query_raw_candidate_rows(&mut stmt, params![candidate_id])?;
    Ok(rows.pop())
}

fn query_raw_candidate_rows<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<RawEvidenceCandidateRecord>>
where
    P: rusqlite::Params,
{
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, String>(12)?,
        ))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|row| {
            Ok(RawEvidenceCandidateRecord {
                candidate_id: row.0,
                update_request_id: row.1,
                trace_id: row.2,
                source_id: row.3,
                source_layer: row.4,
                source_hash: row.5,
                span_start: row.6,
                span_end: row.7,
                matched_terms: serde_json::from_str(&row.8)?,
                query_frame: serde_json::from_str(&row.9)?,
                rule_gap_reason: row.10,
                cluster_key: row.11,
                created_at: row.12,
            })
        })
        .collect()
}

fn load_update_request_by_idempotency(
    conn: &Connection,
    idempotency_key: &str,
) -> Result<Option<OnlineEvidenceCardUpdateRequestRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT update_request_id, trace_id, session_id, resolved_question,
               question_frame_json, coverage_gap_reason, source_scope_policy_json,
               recall_advice_ref, status, created_at, updated_at
        FROM online_evidence_card_update_requests
        WHERE idempotency_key = ?1
        "#,
    )?;
    query_optional_update_request(&mut stmt, params![idempotency_key])
}

fn load_update_request(
    conn: &Connection,
    update_request_id: &str,
) -> Result<Option<OnlineEvidenceCardUpdateRequestRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT update_request_id, trace_id, session_id, resolved_question,
               question_frame_json, coverage_gap_reason, source_scope_policy_json,
               recall_advice_ref, status, created_at, updated_at
        FROM online_evidence_card_update_requests
        WHERE update_request_id = ?1
        "#,
    )?;
    query_optional_update_request(&mut stmt, params![update_request_id])
}

fn query_optional_update_request<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Option<OnlineEvidenceCardUpdateRequestRecord>>
where
    P: rusqlite::Params,
{
    Ok(query_update_request_rows(stmt, params)?.pop())
}

fn query_update_request_rows<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<OnlineEvidenceCardUpdateRequestRecord>>
where
    P: rusqlite::Params,
{
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
        ))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|row| {
            Ok(OnlineEvidenceCardUpdateRequestRecord {
                update_request_id: row.0,
                trace_id: row.1,
                session_id: row.2,
                resolved_question: row.3,
                question_frame: row.4.as_deref().map(serde_json::from_str).transpose()?,
                coverage_gap_reason: row.5,
                source_scope_policy: serde_json::from_str(&row.6)?,
                recall_advice_ref: row.7,
                status: row.8,
                created_at: row.9,
                updated_at: row.10,
            })
        })
        .collect()
}

fn load_card_ingest_job_by_request(
    conn: &Connection,
    update_request_id: &str,
) -> Result<Option<CardIngestJobRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, update_request_id, trace_id, status, stage,
               leased_by, lease_until, heartbeat_at, attempt_count, max_attempts,
               next_run_at, last_error, candidate_id, staged_card_id, created_at, updated_at
        FROM card_ingest_jobs
        WHERE update_request_id = ?1
        LIMIT 1
        "#,
    )?;
    query_optional_card_ingest_job(&mut stmt, params![update_request_id])
}

fn load_card_ingest_job(conn: &Connection, job_id: &str) -> Result<Option<CardIngestJobRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, update_request_id, trace_id, status, stage,
               leased_by, lease_until, heartbeat_at, attempt_count, max_attempts,
               next_run_at, last_error, candidate_id, staged_card_id, created_at, updated_at
        FROM card_ingest_jobs
        WHERE job_id = ?1
        LIMIT 1
        "#,
    )?;
    query_optional_card_ingest_job(&mut stmt, params![job_id])
}

fn load_expired_processing_jobs(conn: &Connection) -> Result<Vec<CardIngestJobRecord>> {
    let now = now_rfc3339();
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, update_request_id, trace_id, status, stage,
               leased_by, lease_until, heartbeat_at, attempt_count, max_attempts,
               next_run_at, last_error, candidate_id, staged_card_id, created_at, updated_at
        FROM card_ingest_jobs
        WHERE status = 'processing'
          AND lease_until IS NOT NULL
          AND lease_until <= ?1
        ORDER BY lease_until, job_id
        "#,
    )?;
    query_card_ingest_job_rows(&mut stmt, params![&now])
}

fn load_retry_ready_jobs(conn: &Connection) -> Result<Vec<CardIngestJobRecord>> {
    let now = now_rfc3339();
    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, update_request_id, trace_id, status, stage,
               leased_by, lease_until, heartbeat_at, attempt_count, max_attempts,
               next_run_at, last_error, candidate_id, staged_card_id, created_at, updated_at
        FROM card_ingest_jobs
        WHERE status = 'retry_wait'
          AND next_run_at <= ?1
        ORDER BY next_run_at, job_id
        "#,
    )?;
    query_card_ingest_job_rows(&mut stmt, params![&now])
}

fn query_optional_card_ingest_job<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Option<CardIngestJobRecord>>
where
    P: rusqlite::Params,
{
    Ok(query_card_ingest_job_rows(stmt, params)?.pop())
}

fn query_card_ingest_job_rows<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<CardIngestJobRecord>>
where
    P: rusqlite::Params,
{
    let rows = stmt.query_map(params, |row| {
        Ok(CardIngestJobRecord {
            job_id: row.get(0)?,
            update_request_id: row.get(1)?,
            trace_id: row.get(2)?,
            status: row.get(3)?,
            stage: row.get(4)?,
            leased_by: row.get(5)?,
            lease_until: row.get(6)?,
            heartbeat_at: row.get(7)?,
            attempt_count: row.get(8)?,
            max_attempts: row.get(9)?,
            next_run_at: row.get(10)?,
            last_error: row.get(11)?,
            candidate_id: row.get(12)?,
            staged_card_id: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

struct StagedCardEventInput<'a> {
    trace_id: &'a str,
    update_request_id: Option<&'a str>,
    staged_card_id: Option<&'a str>,
    candidate_id: Option<&'a str>,
    event_type: &'a str,
    from_status: Option<&'a str>,
    to_status: Option<&'a str>,
    reason_code: &'a str,
    rule_id: Option<&'a str>,
    payload: &'a Value,
}

fn insert_staged_card_event(conn: &Connection, input: StagedCardEventInput<'_>) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO staged_card_events (
            event_id, trace_id, update_request_id, staged_card_id, candidate_id,
            event_type, from_status, to_status, reason_code, rule_id, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        "#,
        params![
            format!("sce-{}", uuid::Uuid::now_v7().simple()),
            input.trace_id,
            input.update_request_id,
            input.staged_card_id,
            input.candidate_id,
            input.event_type,
            input.from_status,
            input.to_status,
            input.reason_code,
            input.rule_id,
            serde_json::to_string(&canonical_json_value(input.payload))?,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

fn source_hash_for_card(conn: &Connection, card: &EvidenceCard) -> Result<String> {
    let source_hash = if sqlite_table_exists(conn, "sources")? {
        conn.query_row(
            "SELECT source_hash FROM sources WHERE source_id = ?1 LIMIT 1",
            params![&card.source_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    } else {
        None
    };
    Ok(source_hash.unwrap_or_else(|| {
        hash_text(&format!(
            "{}:{}:{}",
            card.source_id, card.block_id, card.source_title
        ))
    }))
}

fn support_span_for_card(
    card: &EvidenceCard,
    source_hash: &str,
    matched_terms: &[String],
) -> Value {
    json!({
        "source_id": &card.source_id,
        "source_hash": source_hash,
        "block_id": &card.block_id,
        "source_title": &card.source_title,
        "span_start": 0,
        "span_end": card.text.chars().count(),
        "text_sha256": hash_text(&card.text),
        "matched_terms": matched_terms,
    })
}

fn promoted_evidence_id(exact_span_key: &str, claim_key: &str) -> String {
    format!(
        "evc-{}",
        &hash_text(&format!("{exact_span_key}:{claim_key}"))[..32]
    )
}

fn rfc3339_after_seconds(seconds: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::seconds(seconds))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_rfc3339())
}

fn job_retry_backoff_secs(attempt_count: i64) -> i64 {
    let exponent = attempt_count.saturating_sub(1).clamp(0, 8) as u32;
    CARD_INGEST_JOB_BACKOFF_BASE_SECS
        .saturating_mul(2_i64.saturating_pow(exponent))
        .clamp(1, CARD_INGEST_JOB_BACKOFF_MAX_SECS)
}

fn stable_hash(value: &Value) -> Result<String> {
    Ok(hash_text(&serde_json::to_string(&canonical_json_value(
        value,
    ))?))
}

fn optional_json_text(value: Option<&Value>) -> Result<Option<String>> {
    Ok(value.map(serde_json::to_string).transpose()?)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !value.trim().is_empty() && !values.iter().any(|item| item == value) {
        values.push(value.to_string());
    }
}

fn append_unique_json(values: &mut Vec<Value>, value: Value) {
    if !values.iter().any(|item| item == &value) {
        values.push(value);
    }
}

fn grouped_count_json(conn: &Connection, table: &str, column: &str) -> Result<Value> {
    let sql = format!("SELECT {column}, COUNT(*) FROM {table} GROUP BY {column}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut object = serde_json::Map::new();
    let mut total = 0_i64;
    for row in rows {
        let (status, count) = row?;
        total += count;
        object.insert(status, json!(count));
    }
    Ok(json!({
        "total": total,
        "by_status": Value::Object(object),
    }))
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(Into::into)
}

fn escape_like(value: &str) -> String {
    value.replace('%', "\\%").replace('_', "\\_")
}

#[cfg(test)]
mod tests;
