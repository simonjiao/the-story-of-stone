use crate::now_rfc3339;
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

pub const ONLINE_LEARNING_PROMOTION_MANIFEST_SCHEMA_VERSION: &str =
    "tonglingyu-online-learning-promotion-manifest-v1";
const PROMOTION_MANIFEST_SCHEMA_MIGRATION_ID: &str =
    "tonglingyu-online-learning-promotion-manifest-v1";

#[derive(Debug, Clone)]
pub struct PromotionManifestInput {
    pub artifact_kind: String,
    pub candidate_ids: Vec<String>,
    pub source_trace_refs: Vec<String>,
    pub source_span_refs: Value,
    pub rule_diff_refs: Value,
    pub merge_conflict_decision: Value,
    pub target_ref: String,
    pub expected_version_bump: String,
    pub regression_cases: Value,
    pub reviewer_policy: Value,
    pub dry_run_result: Value,
    pub rollback_ref: Value,
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS online_learning_promotion_manifests (
            promotion_batch_id TEXT PRIMARY KEY,
            artifact_kind TEXT NOT NULL,
            status TEXT NOT NULL,
            candidate_ids_json TEXT NOT NULL,
            source_trace_refs_json TEXT NOT NULL,
            source_span_refs_json TEXT NOT NULL,
            rule_diff_refs_json TEXT NOT NULL,
            merge_conflict_decision_json TEXT NOT NULL,
            target_ref TEXT NOT NULL,
            expected_version_bump TEXT NOT NULL,
            regression_cases_json TEXT NOT NULL,
            reviewer_policy_json TEXT NOT NULL,
            dry_run_result_json TEXT NOT NULL,
            rollback_ref_json TEXT NOT NULL,
            blocking_reasons_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            completed_at TEXT NOT NULL,
            schema_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS online_learning_promotion_manifest_trace_refs (
            promotion_batch_id TEXT NOT NULL REFERENCES online_learning_promotion_manifests(promotion_batch_id),
            trace_id TEXT NOT NULL,
            PRIMARY KEY(promotion_batch_id, trace_id)
        );

        CREATE INDEX IF NOT EXISTS idx_online_learning_promotion_manifest_kind
            ON online_learning_promotion_manifests(artifact_kind, status, created_at);
        CREATE INDEX IF NOT EXISTS idx_online_learning_promotion_manifest_trace
            ON online_learning_promotion_manifest_trace_refs(trace_id, promotion_batch_id);
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![PROMOTION_MANIFEST_SCHEMA_MIGRATION_ID, now_rfc3339()],
    )?;
    Ok(())
}

pub fn record_online_learning_promotion_manifest(
    conn: &Connection,
    input: PromotionManifestInput,
) -> Result<Value> {
    init_schema(conn)?;
    validate_manifest_input(&input)?;
    let blocking_reasons = promotion_blocking_reasons(&input);
    let status = if blocking_reasons.is_empty() {
        "passed"
    } else {
        "blocked"
    };
    let promotion_batch_id = format!("promotion-batch-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO online_learning_promotion_manifests (
            promotion_batch_id, artifact_kind, status, candidate_ids_json,
            source_trace_refs_json, source_span_refs_json, rule_diff_refs_json,
            merge_conflict_decision_json, target_ref, expected_version_bump,
            regression_cases_json, reviewer_policy_json, dry_run_result_json,
            rollback_ref_json, blocking_reasons_json, created_at, completed_at,
            schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18)
        "#,
        params![
            &promotion_batch_id,
            &input.artifact_kind,
            status,
            serde_json::to_string(&input.candidate_ids)?,
            serde_json::to_string(&input.source_trace_refs)?,
            serde_json::to_string(&input.source_span_refs)?,
            serde_json::to_string(&input.rule_diff_refs)?,
            serde_json::to_string(&input.merge_conflict_decision)?,
            &input.target_ref,
            &input.expected_version_bump,
            serde_json::to_string(&input.regression_cases)?,
            serde_json::to_string(&input.reviewer_policy)?,
            serde_json::to_string(&input.dry_run_result)?,
            serde_json::to_string(&input.rollback_ref)?,
            serde_json::to_string(&blocking_reasons)?,
            &now,
            &now,
            ONLINE_LEARNING_PROMOTION_MANIFEST_SCHEMA_VERSION,
        ],
    )?;
    for trace_id in unique_trace_refs(&input.source_trace_refs) {
        conn.execute(
            "INSERT OR IGNORE INTO online_learning_promotion_manifest_trace_refs (
                promotion_batch_id, trace_id
            ) VALUES (?1, ?2)",
            params![&promotion_batch_id, &trace_id],
        )?;
    }
    Ok(json!({
        "object": "tonglingyu.online_learning.promotion_manifest",
        "schema_version": ONLINE_LEARNING_PROMOTION_MANIFEST_SCHEMA_VERSION,
        "promotion_batch_id": promotion_batch_id,
        "artifact_kind": input.artifact_kind,
        "status": status,
        "candidate_ids": input.candidate_ids,
        "source_trace_refs": input.source_trace_refs,
        "source_span_refs": input.source_span_refs,
        "rule_diff_refs": input.rule_diff_refs,
        "merge_conflict_decision": input.merge_conflict_decision,
        "target_ref": input.target_ref,
        "expected_version_bump": input.expected_version_bump,
        "regression_cases": input.regression_cases,
        "reviewer_policy": input.reviewer_policy,
        "dry_run_result": input.dry_run_result,
        "rollback_ref": input.rollback_ref,
        "blocking_reasons": blocking_reasons,
        "created_at": now,
        "completed_at": now,
        "active_path_visible": false,
    }))
}

pub fn list_online_learning_promotion_manifests_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
    let mut stmt = conn.prepare(
        r#"
        SELECT m.promotion_batch_id, m.artifact_kind, m.status, m.candidate_ids_json,
               m.source_trace_refs_json, m.source_span_refs_json, m.rule_diff_refs_json,
               m.merge_conflict_decision_json, m.target_ref, m.expected_version_bump,
               m.regression_cases_json, m.reviewer_policy_json, m.dry_run_result_json,
               m.rollback_ref_json, m.blocking_reasons_json, m.created_at,
               m.completed_at, m.schema_version
        FROM online_learning_promotion_manifests m
        JOIN online_learning_promotion_manifest_trace_refs r
          ON r.promotion_batch_id = m.promotion_batch_id
        WHERE r.trace_id = ?1
        ORDER BY m.created_at, m.promotion_batch_id
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![trace_id, limit], promotion_manifest_row_json)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn read_online_learning_promotion_manifest(
    conn: &Connection,
    promotion_batch_id: &str,
) -> Result<Option<Value>> {
    init_schema(conn)?;
    conn.query_row(
        r#"
        SELECT promotion_batch_id, artifact_kind, status, candidate_ids_json,
               source_trace_refs_json, source_span_refs_json, rule_diff_refs_json,
               merge_conflict_decision_json, target_ref, expected_version_bump,
               regression_cases_json, reviewer_policy_json, dry_run_result_json,
               rollback_ref_json, blocking_reasons_json, created_at, completed_at,
               schema_version
        FROM online_learning_promotion_manifests
        WHERE promotion_batch_id = ?1
        "#,
        params![promotion_batch_id],
        promotion_manifest_row_json,
    )
    .optional()
    .map_err(Into::into)
}

fn validate_manifest_input(input: &PromotionManifestInput) -> Result<()> {
    if !matches!(input.artifact_kind.as_str(), "evidence" | "rule" | "prompt") {
        return Err(anyhow!(
            "unsupported promotion manifest artifact_kind: {}",
            input.artifact_kind
        ));
    }
    if input
        .candidate_ids
        .iter()
        .all(|candidate_id| candidate_id.trim().is_empty())
    {
        return Err(anyhow!(
            "promotion manifest candidate_ids must not be empty"
        ));
    }
    if input
        .source_trace_refs
        .iter()
        .all(|trace_id| trace_id.trim().is_empty())
    {
        return Err(anyhow!(
            "promotion manifest source_trace_refs must not be empty"
        ));
    }
    if input.target_ref.trim().is_empty() {
        return Err(anyhow!("promotion manifest target_ref is required"));
    }
    if input.expected_version_bump.trim().is_empty() {
        return Err(anyhow!(
            "promotion manifest expected_version_bump is required"
        ));
    }
    if input.reviewer_policy.is_null() {
        return Err(anyhow!("promotion manifest reviewer_policy is required"));
    }
    if input.dry_run_result.is_null() {
        return Err(anyhow!("promotion manifest dry_run_result is required"));
    }
    if !json_has_content(&input.regression_cases) {
        return Err(anyhow!("promotion manifest regression_cases are required"));
    }
    match input.artifact_kind.as_str() {
        "evidence" if !json_has_content(&input.source_span_refs) => {
            return Err(anyhow!(
                "evidence promotion manifest source_span_refs are required"
            ));
        }
        "rule" | "prompt" if !json_has_content(&input.rule_diff_refs) => {
            return Err(anyhow!(
                "rule and prompt promotion manifests require rule_diff_refs"
            ));
        }
        _ => {}
    }
    if input.rollback_ref.is_null() {
        return Err(anyhow!("promotion manifest rollback_ref is required"));
    }
    Ok(())
}

fn promotion_blocking_reasons(input: &PromotionManifestInput) -> Vec<String> {
    let mut reasons = Vec::new();
    if has_conflict(&input.merge_conflict_decision) {
        reasons.push("merge_conflict_blocks_batch".to_string());
    }
    if dry_run_failed(&input.dry_run_result) {
        reasons.push("dry_run_failure_blocks_batch".to_string());
    }
    if regression_failed(&input.regression_cases) || regression_failed(&input.dry_run_result) {
        reasons.push("regression_failure_blocks_batch".to_string());
    }
    reasons.sort();
    reasons.dedup();
    reasons
}

fn has_conflict(value: &Value) -> bool {
    value
        .get("has_conflict")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || status_is_blocking(value.get("status"))
        || value
            .get("decision")
            .and_then(Value::as_str)
            .is_some_and(|decision| matches!(decision, "conflict" | "blocked" | "failed"))
}

fn dry_run_failed(value: &Value) -> bool {
    status_is_blocking(value.get("status"))
        || value
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
}

fn regression_failed(value: &Value) -> bool {
    if status_is_blocking(value.get("status")) || status_is_blocking(value.get("regression_status"))
    {
        return true;
    }
    if value
        .get("failed_count")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        > 0
    {
        return true;
    }
    if value
        .get("skipped_count")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        > 0
    {
        return true;
    }
    value
        .as_array()
        .is_some_and(|items| items.iter().any(regression_failed))
}

fn status_is_blocking(status: Option<&Value>) -> bool {
    status
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "blocked" | "conflict" | "failed"))
}

fn json_has_content(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

fn unique_trace_refs(trace_refs: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    for trace_id in trace_refs {
        let trace_id = trace_id.trim();
        if trace_id.is_empty() || values.iter().any(|existing| existing == trace_id) {
            continue;
        }
        values.push(trace_id.to_string());
    }
    values
}

fn promotion_manifest_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "object": "tonglingyu.online_learning.promotion_manifest",
        "promotion_batch_id": row.get::<_, String>(0)?,
        "artifact_kind": row.get::<_, String>(1)?,
        "status": row.get::<_, String>(2)?,
        "candidate_ids": parse_json(row.get::<_, String>(3)?),
        "source_trace_refs": parse_json(row.get::<_, String>(4)?),
        "source_span_refs": parse_json(row.get::<_, String>(5)?),
        "rule_diff_refs": parse_json(row.get::<_, String>(6)?),
        "merge_conflict_decision": parse_json(row.get::<_, String>(7)?),
        "target_ref": row.get::<_, String>(8)?,
        "expected_version_bump": row.get::<_, String>(9)?,
        "regression_cases": parse_json(row.get::<_, String>(10)?),
        "reviewer_policy": parse_json(row.get::<_, String>(11)?),
        "dry_run_result": parse_json(row.get::<_, String>(12)?),
        "rollback_ref": parse_json(row.get::<_, String>(13)?),
        "blocking_reasons": parse_json(row.get::<_, String>(14)?),
        "created_at": row.get::<_, String>(15)?,
        "completed_at": row.get::<_, String>(16)?,
        "schema_version": row.get::<_, String>(17)?,
        "active_path_visible": false,
    }))
}

fn parse_json(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests;
