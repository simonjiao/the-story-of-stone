use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use time::OffsetDateTime;

pub(crate) const ANSWER_QUALITY_OBSERVATION_SCHEMA_VERSION: &str =
    "tonglingyu-answer-quality-observation-v1";
pub(crate) const ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION: &str =
    "tonglingyu-answer-quality-action-item-v1";

const TEXT_MAX_CHARS: usize = 720;
const TAG_MAX_CHARS: usize = 96;
const TAG_MAX_COUNT: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct AnswerQualityObservationInput<'a> {
    pub(crate) actor: &'a str,
    pub(crate) source_entity_type: &'a str,
    pub(crate) source_entity_id: &'a str,
    pub(crate) run_id: Option<&'a str>,
    pub(crate) case_id: Option<&'a str>,
    pub(crate) trace_id: Option<&'a str>,
    pub(crate) package_id: Option<&'a str>,
    pub(crate) severity: &'a str,
    pub(crate) failure_tags: &'a [String],
    pub(crate) user_visible_issue: Option<&'a str>,
    pub(crate) runtime_issue: Option<&'a str>,
    pub(crate) suggested_owner: Option<&'a str>,
    pub(crate) suggested_action: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct AnswerQualityObservationListInput<'a> {
    pub(crate) status: Option<&'a str>,
    pub(crate) action_route: Option<&'a str>,
    pub(crate) severity: Option<&'a str>,
    pub(crate) trace_id: Option<&'a str>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct AnswerQualityActionItemListInput<'a> {
    pub(crate) status: Option<&'a str>,
    pub(crate) action_type: Option<&'a str>,
    pub(crate) action_route: Option<&'a str>,
    pub(crate) trace_id: Option<&'a str>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct AnswerQualityActionTransitionInput<'a> {
    pub(crate) action_item_id: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) action: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) linked_entity_type: Option<&'a str>,
    pub(crate) linked_entity_id: Option<&'a str>,
}

pub(crate) fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS answer_quality_observations (
            observation_id TEXT PRIMARY KEY,
            source_entity_type TEXT NOT NULL,
            source_entity_id TEXT NOT NULL,
            run_id TEXT,
            case_id TEXT,
            trace_id TEXT,
            package_id TEXT,
            severity TEXT NOT NULL,
            failure_tags_json TEXT NOT NULL,
            user_visible_issue TEXT NOT NULL,
            runtime_issue TEXT NOT NULL,
            suggested_owner TEXT,
            suggested_action TEXT,
            action_route TEXT NOT NULL,
            action_reason TEXT NOT NULL,
            action_payload_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_by TEXT NOT NULL,
            first_observed_at TEXT NOT NULL,
            last_observed_at TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            UNIQUE(source_entity_type, source_entity_id)
        );

        CREATE TABLE IF NOT EXISTS answer_quality_observation_events (
            event_id TEXT PRIMARY KEY,
            observation_id TEXT NOT NULL REFERENCES answer_quality_observations(observation_id),
            event_type TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT,
            actor TEXT NOT NULL,
            reason_code TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            schema_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS answer_quality_action_items (
            action_item_id TEXT PRIMARY KEY,
            observation_id TEXT NOT NULL REFERENCES answer_quality_observations(observation_id),
            action_route TEXT NOT NULL,
            action_type TEXT NOT NULL,
            target_surface TEXT NOT NULL,
            status TEXT NOT NULL,
            priority TEXT NOT NULL,
            trace_id TEXT,
            package_id TEXT,
            linked_entity_type TEXT,
            linked_entity_id TEXT,
            action_payload_json TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            UNIQUE(observation_id, action_type)
        );

        CREATE TABLE IF NOT EXISTS answer_quality_action_events (
            event_id TEXT PRIMARY KEY,
            action_item_id TEXT NOT NULL REFERENCES answer_quality_action_items(action_item_id),
            event_type TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT,
            actor TEXT NOT NULL,
            reason_code TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            schema_version TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_answer_quality_observations_status
            ON answer_quality_observations(status, last_observed_at);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_observations_route
            ON answer_quality_observations(action_route, last_observed_at);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_observations_trace
            ON answer_quality_observations(trace_id);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_observation_events_observation
            ON answer_quality_observation_events(observation_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_action_items_status
            ON answer_quality_action_items(status, updated_at);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_action_items_route
            ON answer_quality_action_items(action_route, updated_at);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_action_items_trace
            ON answer_quality_action_items(trace_id);
        CREATE INDEX IF NOT EXISTS idx_answer_quality_action_events_item
            ON answer_quality_action_events(action_item_id, created_at);
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![ANSWER_QUALITY_OBSERVATION_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION, now_rfc3339()],
    )?;
    Ok(())
}

pub(crate) fn record_answer_quality_observation(
    conn: &Connection,
    input: AnswerQualityObservationInput<'_>,
) -> Result<Value> {
    let source_entity_type = bounded_required(input.source_entity_type, "source_entity_type", 96)?;
    let source_entity_id = bounded_required(input.source_entity_id, "source_entity_id", 220)?;
    let actor = bounded_required(input.actor, "actor", 96)?;
    let severity = normalize_severity(input.severity)?;
    let failure_tags = normalize_failure_tags(input.failure_tags);
    let route = classify_action_route(&failure_tags, input.suggested_owner);
    let now = now_rfc3339();
    let observation_id = format!(
        "answer-quality-observation-{}",
        &hash_text(&format!("{source_entity_type}\n{source_entity_id}"))[..16]
    );
    let user_visible_issue = bounded_optional(input.user_visible_issue, TEXT_MAX_CHARS)
        .unwrap_or_else(|| "quality issue observed".to_string());
    let runtime_issue = bounded_optional(input.runtime_issue, TEXT_MAX_CHARS)
        .unwrap_or_else(|| "runtime issue not provided".to_string());
    let suggested_owner = bounded_optional(input.suggested_owner, 96);
    let suggested_action = bounded_optional(input.suggested_action, TEXT_MAX_CHARS);
    let action_payload =
        action_payload_json(&route, &failure_tags, input.trace_id, input.package_id);
    let failure_tags_json = serde_json::to_string(&failure_tags)?;
    let action_payload_json = serde_json::to_string(&action_payload)?;

    conn.execute(
        r#"
        INSERT INTO answer_quality_observations (
            observation_id, source_entity_type, source_entity_id, run_id, case_id,
            trace_id, package_id, severity, failure_tags_json, user_visible_issue,
            runtime_issue, suggested_owner, suggested_action, action_route,
            action_reason, action_payload_json, status, created_by, first_observed_at,
            last_observed_at, schema_version
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16, 'observed', ?17, ?18, ?18, ?19
        )
        ON CONFLICT(source_entity_type, source_entity_id) DO UPDATE SET
            run_id = excluded.run_id,
            case_id = excluded.case_id,
            trace_id = excluded.trace_id,
            package_id = excluded.package_id,
            severity = excluded.severity,
            failure_tags_json = excluded.failure_tags_json,
            user_visible_issue = excluded.user_visible_issue,
            runtime_issue = excluded.runtime_issue,
            suggested_owner = excluded.suggested_owner,
            suggested_action = excluded.suggested_action,
            action_route = excluded.action_route,
            action_reason = excluded.action_reason,
            action_payload_json = excluded.action_payload_json,
            status = 'observed',
            last_observed_at = excluded.last_observed_at,
            schema_version = excluded.schema_version
        "#,
        params![
            &observation_id,
            source_entity_type,
            source_entity_id,
            bounded_optional(input.run_id, 160),
            bounded_optional(input.case_id, 160),
            bounded_optional(input.trace_id, 120),
            bounded_optional(input.package_id, 120),
            severity,
            failure_tags_json,
            user_visible_issue,
            runtime_issue,
            suggested_owner,
            suggested_action,
            route.route,
            route.reason,
            action_payload_json,
            actor,
            now,
            ANSWER_QUALITY_OBSERVATION_SCHEMA_VERSION,
        ],
    )?;

    let event_id = format!(
        "answer-quality-event-{}",
        &hash_text(&format!("{observation_id}\n{now}\n{}", route.route))[..16]
    );
    conn.execute(
        r#"
        INSERT OR IGNORE INTO answer_quality_observation_events (
            event_id, observation_id, event_type, from_status, to_status, actor,
            reason_code, metadata_json, created_at, schema_version
        ) VALUES (?1, ?2, 'observed', NULL, 'observed', ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            event_id,
            &observation_id,
            actor,
            route.reason,
            serde_json::to_string(&json!({
                "action_route": route.route,
                "failure_tag_count": failure_tags.len(),
                "active_path_visible": false,
            }))?,
            now,
            ANSWER_QUALITY_OBSERVATION_SCHEMA_VERSION,
        ],
    )?;

    upsert_answer_quality_action_item(
        conn,
        &observation_id,
        &route,
        severity,
        input.trace_id,
        input.package_id,
        &action_payload,
        &actor,
        &now,
    )?;

    read_answer_quality_observation(conn, &observation_id)?
        .ok_or_else(|| anyhow!("answer quality observation was not readable after write"))
}

pub(crate) fn list_answer_quality_observations(
    conn: &Connection,
    input: AnswerQualityObservationListInput<'_>,
) -> Result<Value> {
    validate_optional_filter(input.status, &allowed_statuses(), "status")?;
    validate_optional_filter(input.action_route, &allowed_action_routes(), "action_route")?;
    validate_optional_filter(input.severity, &allowed_severities(), "severity")?;
    let limit = input.limit.clamp(1, 200);
    let offset = input.offset;

    let mut sql = String::from(
        "SELECT observation_id, source_entity_type, source_entity_id, run_id, case_id,
                trace_id, package_id, severity, failure_tags_json, user_visible_issue,
                runtime_issue, suggested_owner, suggested_action, action_route,
                action_reason, action_payload_json, status, created_by, first_observed_at,
                last_observed_at, schema_version
         FROM answer_quality_observations WHERE 1 = 1",
    );
    let mut filters = Vec::<String>::new();
    let mut values = Vec::<String>::new();
    if let Some(status) = input.status {
        filters.push("status = ?".to_string());
        values.push(status.to_string());
    }
    if let Some(action_route) = input.action_route {
        filters.push("action_route = ?".to_string());
        values.push(action_route.to_string());
    }
    if let Some(severity) = input.severity {
        filters.push("severity = ?".to_string());
        values.push(severity.to_string());
    }
    if let Some(trace_id) = input
        .trace_id
        .and_then(|value| bounded_optional(Some(value), 120))
    {
        filters.push("trace_id = ?".to_string());
        values.push(trace_id);
    }
    for filter in filters {
        sql.push_str(" AND ");
        sql.push_str(&filter);
    }
    sql.push_str(" ORDER BY last_observed_at DESC, observation_id DESC LIMIT ? OFFSET ?");

    let limit_i64 = limit as i64;
    let offset_i64 = offset as i64;
    let mut dynamic_params: Vec<&dyn rusqlite::ToSql> = Vec::new();
    for value in &values {
        dynamic_params.push(value);
    }
    dynamic_params.push(&limit_i64);
    dynamic_params.push(&offset_i64);

    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(rusqlite::params_from_iter(dynamic_params), |row| {
            answer_quality_observation_row_json(row)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(json!({
        "object": "tonglingyu.answer_quality_observation_list",
        "schema_version": ANSWER_QUALITY_OBSERVATION_SCHEMA_VERSION,
        "items": items,
        "limit": limit,
        "offset": offset,
        "active_path_visible": false,
    }))
}

pub(crate) fn read_answer_quality_observation(
    conn: &Connection,
    observation_id: &str,
) -> Result<Option<Value>> {
    let observation = conn
        .query_row(
            "SELECT observation_id, source_entity_type, source_entity_id, run_id, case_id,
                    trace_id, package_id, severity, failure_tags_json, user_visible_issue,
                    runtime_issue, suggested_owner, suggested_action, action_route,
                    action_reason, action_payload_json, status, created_by, first_observed_at,
                    last_observed_at, schema_version
             FROM answer_quality_observations WHERE observation_id = ?1",
            params![observation_id],
            answer_quality_observation_row_json,
        )
        .optional()?;
    let Some(mut observation) = observation else {
        return Ok(None);
    };
    let events = read_answer_quality_observation_events(conn, observation_id)?;
    let action_items = read_answer_quality_action_items_for_observation(conn, observation_id)?;
    observation["events"] = json!(events);
    observation["action_items"] = json!(action_items);
    Ok(Some(json!({
        "object": "tonglingyu.answer_quality_observation",
        "schema_version": ANSWER_QUALITY_OBSERVATION_SCHEMA_VERSION,
        "observation": observation,
        "active_path_visible": false,
    })))
}

pub(crate) fn load_answer_quality_observations_for_trace(
    conn: &Connection,
    trace_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT observation_id, source_entity_type, source_entity_id, run_id, case_id,
                trace_id, package_id, severity, failure_tags_json, user_visible_issue,
                runtime_issue, suggested_owner, suggested_action, action_route,
                action_reason, action_payload_json, status, created_by, first_observed_at,
                last_observed_at, schema_version
         FROM answer_quality_observations
         WHERE trace_id = ?1
         ORDER BY last_observed_at, observation_id",
    )?;
    let items = stmt
        .query_map(params![trace_id], answer_quality_observation_row_json)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

pub(crate) fn list_answer_quality_action_items(
    conn: &Connection,
    input: AnswerQualityActionItemListInput<'_>,
) -> Result<Value> {
    validate_optional_filter(input.status, &allowed_action_statuses(), "action_status")?;
    validate_optional_filter(input.action_route, &allowed_action_routes(), "action_route")?;
    validate_optional_filter(input.action_type, &allowed_action_types(), "action_type")?;
    let limit = input.limit.clamp(1, 200) as i64;
    let offset = input.offset as i64;
    let trace_id = input
        .trace_id
        .and_then(|value| bounded_optional(Some(value), 120));
    let mut stmt = conn.prepare(
        "SELECT action_item_id, observation_id, action_route, action_type,
                target_surface, status, priority, trace_id, package_id,
                linked_entity_type, linked_entity_id, action_payload_json,
                created_by, created_at, updated_at, schema_version
         FROM answer_quality_action_items
         WHERE (?1 IS NULL OR status = ?1)
           AND (?2 IS NULL OR action_route = ?2)
           AND (?3 IS NULL OR action_type = ?3)
           AND (?4 IS NULL OR trace_id = ?4)
         ORDER BY updated_at DESC, action_item_id DESC
         LIMIT ?5 OFFSET ?6",
    )?;
    let items = stmt
        .query_map(
            params![
                input.status,
                input.action_route,
                input.action_type,
                trace_id,
                limit,
                offset,
            ],
            answer_quality_action_item_row_json,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({
        "object": "tonglingyu.answer_quality_action_item_list",
        "schema_version": ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION,
        "items": items,
        "limit": limit,
        "offset": offset,
        "active_path_visible": false,
    }))
}

pub(crate) fn read_answer_quality_action_item(
    conn: &Connection,
    action_item_id: &str,
) -> Result<Option<Value>> {
    let action_item = conn
        .query_row(
            "SELECT action_item_id, observation_id, action_route, action_type,
                    target_surface, status, priority, trace_id, package_id,
                    linked_entity_type, linked_entity_id, action_payload_json,
                    created_by, created_at, updated_at, schema_version
             FROM answer_quality_action_items WHERE action_item_id = ?1",
            params![action_item_id],
            answer_quality_action_item_row_json,
        )
        .optional()?;
    let Some(mut action_item) = action_item else {
        return Ok(None);
    };
    let events = read_answer_quality_action_events(conn, action_item_id)?;
    action_item["events"] = json!(events);
    Ok(Some(json!({
        "object": "tonglingyu.answer_quality_action_item",
        "schema_version": ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION,
        "action_item": action_item,
        "active_path_visible": false,
    })))
}

pub(crate) fn transition_answer_quality_action_item(
    conn: &Connection,
    input: AnswerQualityActionTransitionInput<'_>,
) -> Result<Value> {
    let actor = bounded_required(input.actor, "actor", 96)?;
    let reason = bounded_required(input.reason, "reason", TEXT_MAX_CHARS)?;
    let action = normalize_action_transition(input.action)?;
    let current = read_action_item_for_transition(conn, input.action_item_id)?;
    let from_status = current.status.as_str();
    let to_status = action_transition_target_status(action, from_status)?;
    let linked_entity_type = match bounded_optional(input.linked_entity_type, 96) {
        Some(value) => Some(normalize_linked_entity_type(&value)?),
        None => None,
    };
    let linked_entity_id = bounded_optional(input.linked_entity_id, 220);
    if action == "link" && (linked_entity_type.is_none() || linked_entity_id.is_none()) {
        return Err(anyhow!(
            "linked entity is required for answer quality link action"
        ));
    }
    if action != "link" && (linked_entity_type.is_some() || linked_entity_id.is_some()) {
        return Err(anyhow!(
            "linked entity is only supported by answer quality link action"
        ));
    }
    let now = now_rfc3339();
    conn.execute(
        "UPDATE answer_quality_action_items
         SET status = ?2,
             linked_entity_type = COALESCE(?3, linked_entity_type),
             linked_entity_id = COALESCE(?4, linked_entity_id),
             updated_at = ?5
         WHERE action_item_id = ?1",
        params![
            input.action_item_id,
            to_status,
            linked_entity_type,
            linked_entity_id,
            now,
        ],
    )?;
    let event_id = format!(
        "answer-quality-action-event-{}",
        &hash_text(&format!(
            "{}\n{}\n{}\n{}",
            input.action_item_id, action, to_status, now
        ))[..16]
    );
    conn.execute(
        "INSERT INTO answer_quality_action_events (
             event_id, action_item_id, event_type, from_status, to_status, actor,
             reason_code, metadata_json, created_at, schema_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event_id,
            input.action_item_id,
            action,
            from_status,
            to_status,
            actor,
            action,
            serde_json::to_string(&json!({
                "reason_sha256": hash_text(&reason),
                "linked_entity_type": linked_entity_type,
                "linked_entity_id_sha256": input.linked_entity_id.map(hash_text),
                "active_path_visible": false,
            }))?,
            now,
            ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION,
        ],
    )?;
    read_answer_quality_action_item(conn, input.action_item_id)?
        .ok_or_else(|| anyhow!("answer quality action item was not readable after transition"))
}

pub(crate) fn load_answer_quality_action_items_for_trace(
    conn: &Connection,
    trace_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT action_item_id, observation_id, action_route, action_type,
                target_surface, status, priority, trace_id, package_id,
                linked_entity_type, linked_entity_id, action_payload_json,
                created_by, created_at, updated_at, schema_version
         FROM answer_quality_action_items
         WHERE trace_id = ?1
         ORDER BY updated_at, action_item_id",
    )?;
    let items = stmt
        .query_map(params![trace_id], answer_quality_action_item_row_json)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn read_answer_quality_observation_events(
    conn: &Connection,
    observation_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT event_id, observation_id, event_type, from_status, to_status, actor,
                reason_code, metadata_json, created_at, schema_version
         FROM answer_quality_observation_events
         WHERE observation_id = ?1
         ORDER BY created_at, event_id",
    )?;
    let events = stmt
        .query_map(params![observation_id], |row| {
            let metadata: String = row.get(7)?;
            Ok(json!({
                "event_id": row.get::<_, String>(0)?,
                "observation_id": row.get::<_, String>(1)?,
                "event_type": row.get::<_, String>(2)?,
                "from_status": row.get::<_, Option<String>>(3)?,
                "to_status": row.get::<_, Option<String>>(4)?,
                "actor": row.get::<_, String>(5)?,
                "reason_code": row.get::<_, String>(6)?,
                "metadata": serde_json::from_str::<Value>(&metadata).unwrap_or(Value::Null),
                "created_at": row.get::<_, String>(8)?,
                "schema_version": row.get::<_, String>(9)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(events)
}

fn read_answer_quality_action_items_for_observation(
    conn: &Connection,
    observation_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT action_item_id, observation_id, action_route, action_type,
                target_surface, status, priority, trace_id, package_id,
                linked_entity_type, linked_entity_id, action_payload_json,
                created_by, created_at, updated_at, schema_version
         FROM answer_quality_action_items
         WHERE observation_id = ?1
         ORDER BY updated_at, action_item_id",
    )?;
    let items = stmt
        .query_map(params![observation_id], answer_quality_action_item_row_json)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn read_answer_quality_action_events(
    conn: &Connection,
    action_item_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT event_id, action_item_id, event_type, from_status, to_status, actor,
                reason_code, metadata_json, created_at, schema_version
         FROM answer_quality_action_events
         WHERE action_item_id = ?1
         ORDER BY created_at, event_id",
    )?;
    let events = stmt
        .query_map(params![action_item_id], |row| {
            let metadata: String = row.get(7)?;
            Ok(json!({
                "event_id": row.get::<_, String>(0)?,
                "action_item_id": row.get::<_, String>(1)?,
                "event_type": row.get::<_, String>(2)?,
                "from_status": row.get::<_, Option<String>>(3)?,
                "to_status": row.get::<_, Option<String>>(4)?,
                "actor": row.get::<_, String>(5)?,
                "reason_code": row.get::<_, String>(6)?,
                "metadata": serde_json::from_str::<Value>(&metadata).unwrap_or(Value::Null),
                "created_at": row.get::<_, String>(8)?,
                "schema_version": row.get::<_, String>(9)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(events)
}

fn answer_quality_observation_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let failure_tags_json: String = row.get(8)?;
    let action_payload_json: String = row.get(15)?;
    Ok(json!({
        "observation_id": row.get::<_, String>(0)?,
        "source_entity_type": row.get::<_, String>(1)?,
        "source_entity_id": row.get::<_, String>(2)?,
        "run_id": row.get::<_, Option<String>>(3)?,
        "case_id": row.get::<_, Option<String>>(4)?,
        "trace_id": row.get::<_, Option<String>>(5)?,
        "package_id": row.get::<_, Option<String>>(6)?,
        "severity": row.get::<_, String>(7)?,
        "failure_tags": serde_json::from_str::<Value>(&failure_tags_json).unwrap_or(json!([])),
        "user_visible_issue": row.get::<_, String>(9)?,
        "runtime_issue": row.get::<_, String>(10)?,
        "suggested_owner": row.get::<_, Option<String>>(11)?,
        "suggested_action": row.get::<_, Option<String>>(12)?,
        "action_route": row.get::<_, String>(13)?,
        "action_reason": row.get::<_, String>(14)?,
        "action_payload": serde_json::from_str::<Value>(&action_payload_json).unwrap_or(Value::Null),
        "status": row.get::<_, String>(16)?,
        "created_by": row.get::<_, String>(17)?,
        "first_observed_at": row.get::<_, String>(18)?,
        "last_observed_at": row.get::<_, String>(19)?,
        "schema_version": row.get::<_, String>(20)?,
        "active_path_visible": false,
    }))
}

fn answer_quality_action_item_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let action_payload_json: String = row.get(11)?;
    Ok(json!({
        "action_item_id": row.get::<_, String>(0)?,
        "observation_id": row.get::<_, String>(1)?,
        "action_route": row.get::<_, String>(2)?,
        "action_type": row.get::<_, String>(3)?,
        "target_surface": row.get::<_, String>(4)?,
        "status": row.get::<_, String>(5)?,
        "priority": row.get::<_, String>(6)?,
        "trace_id": row.get::<_, Option<String>>(7)?,
        "package_id": row.get::<_, Option<String>>(8)?,
        "linked_entity_type": row.get::<_, Option<String>>(9)?,
        "linked_entity_id": row.get::<_, Option<String>>(10)?,
        "action_payload": serde_json::from_str::<Value>(&action_payload_json).unwrap_or(Value::Null),
        "created_by": row.get::<_, String>(12)?,
        "created_at": row.get::<_, String>(13)?,
        "updated_at": row.get::<_, String>(14)?,
        "schema_version": row.get::<_, String>(15)?,
        "active_path_visible": false,
    }))
}

#[derive(Clone, Copy)]
struct ActionRoute {
    route: &'static str,
    reason: &'static str,
}

fn classify_action_route(tags: &[String], suggested_owner: Option<&str>) -> ActionRoute {
    let owner = suggested_owner.unwrap_or_default();
    if owner == "rule_governance"
        || owner == "question_resolution"
        || tags.iter().any(|tag| {
            tag.contains("rule_candidate")
                || tag.contains("question_frame")
                || tag.contains("followup_resolution")
                || tag.contains("multi_turn")
        })
    {
        return ActionRoute {
            route: "context_rule_candidate",
            reason: "structured_question_or_rule_candidate_gap",
        };
    }
    if owner == "retrieval_policy"
        || tags.iter().any(|tag| {
            tag.contains("evidence")
                || tag.contains("negative_case")
                || tag.contains("retrieval")
                || tag.contains("package")
        })
    {
        return ActionRoute {
            route: "evidence_card_or_retrieval",
            reason: "local_evidence_or_retrieval_gap",
        };
    }
    if owner == "answer_renderer"
        || tags.iter().any(|tag| {
            tag.contains("internal_metadata")
                || tag.contains("forbidden_term")
                || tag.contains("visible_internal_error")
        })
    {
        return ActionRoute {
            route: "answer_rule_or_renderer",
            reason: "public_answer_boundary_gap",
        };
    }
    if owner == "runtime_trace_contract"
        || tags.iter().any(|tag| {
            tag.contains("runtime_check")
                || tag.contains("reviewer")
                || tag.contains("http_error")
                || tag.contains("request_error")
        })
    {
        return ActionRoute {
            route: "runtime_contract",
            reason: "runtime_contract_or_observability_gap",
        };
    }
    ActionRoute {
        route: "manual_review",
        reason: "quality_observation_requires_triage",
    }
}

#[allow(clippy::too_many_arguments)]
fn upsert_answer_quality_action_item(
    conn: &Connection,
    observation_id: &str,
    route: &ActionRoute,
    severity: &str,
    trace_id: Option<&str>,
    package_id: Option<&str>,
    action_payload: &Value,
    actor: &str,
    now: &str,
) -> Result<()> {
    let action_type = action_type_for_route(route.route);
    let target_surface = target_surface_for_route(route.route);
    let action_item_id = format!(
        "answer-quality-action-{}",
        &hash_text(&format!("{observation_id}\n{action_type}"))[..16]
    );
    let previous_status = read_existing_action_status(conn, &action_item_id)?;
    let action_payload_json = serde_json::to_string(action_payload)?;
    conn.execute(
        "INSERT INTO answer_quality_action_items (
             action_item_id, observation_id, action_route, action_type, target_surface,
             status, priority, trace_id, package_id, action_payload_json, created_by,
             created_at, updated_at, schema_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)
         ON CONFLICT(observation_id, action_type) DO UPDATE SET
             action_route = excluded.action_route,
             target_surface = excluded.target_surface,
             status = CASE
                 WHEN answer_quality_action_items.status IN ('closed', 'rejected')
                 THEN 'queued'
                 ELSE answer_quality_action_items.status
             END,
             priority = excluded.priority,
             trace_id = excluded.trace_id,
             package_id = excluded.package_id,
             action_payload_json = excluded.action_payload_json,
             updated_at = excluded.updated_at,
             schema_version = excluded.schema_version",
        params![
            &action_item_id,
            observation_id,
            route.route,
            action_type,
            target_surface,
            priority_for_severity(severity),
            bounded_optional(trace_id, 120),
            bounded_optional(package_id, 120),
            action_payload_json,
            actor,
            now,
            ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION,
        ],
    )?;
    match previous_status.as_deref() {
        None => insert_answer_quality_action_event(
            conn,
            &action_item_id,
            "queued",
            None,
            Some("queued"),
            actor,
            route.reason,
            json!({
                "action_route": route.route,
                "action_type": action_type,
                "target_surface": target_surface,
                "active_path_visible": false,
            }),
            now,
        )?,
        Some("closed" | "rejected") => insert_answer_quality_action_event(
            conn,
            &action_item_id,
            "requeued_by_observation",
            previous_status.as_deref(),
            Some("queued"),
            actor,
            route.reason,
            json!({
                "action_route": route.route,
                "action_type": action_type,
                "target_surface": target_surface,
                "observation_id": observation_id,
                "active_path_visible": false,
            }),
            now,
        )?,
        _ => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_answer_quality_action_event(
    conn: &Connection,
    action_item_id: &str,
    event_type: &str,
    from_status: Option<&str>,
    to_status: Option<&str>,
    actor: &str,
    reason_code: &str,
    metadata: Value,
    now: &str,
) -> Result<()> {
    let event_id = format!(
        "answer-quality-action-event-{}",
        &hash_text(&format!(
            "{action_item_id}\n{event_type}\n{}\n{now}",
            from_status.unwrap_or_default()
        ))[..16]
    );
    conn.execute(
        "INSERT OR IGNORE INTO answer_quality_action_events (
             event_id, action_item_id, event_type, from_status, to_status, actor,
             reason_code, metadata_json, created_at, schema_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event_id,
            action_item_id,
            event_type,
            from_status,
            to_status,
            actor,
            reason_code,
            serde_json::to_string(&metadata)?,
            now,
            ANSWER_QUALITY_ACTION_ITEM_SCHEMA_VERSION,
        ],
    )?;
    Ok(())
}

fn action_payload_json(
    route: &ActionRoute,
    tags: &[String],
    trace_id: Option<&str>,
    package_id: Option<&str>,
) -> Value {
    let next_surfaces = match route.route {
        "context_rule_candidate" => {
            vec!["/v1/admin/rules/candidates", "/v1/admin/traces/{trace_id}"]
        }
        "evidence_card_or_retrieval" => vec![
            "/v1/admin/evidence-card-ingest/run",
            "/v1/admin/traces/{trace_id}",
        ],
        "answer_rule_or_renderer" => vec![
            "/v1/admin/traces/{trace_id}",
            "/v1/admin/packages/{package_id}",
        ],
        "runtime_contract" => vec!["/v1/admin/traces/{trace_id}", "/v1/admin/metrics"],
        _ => vec!["/v1/admin/quality/observations/{observation_id}"],
    };
    json!({
        "route": route.route,
        "reason": route.reason,
        "failure_tags": tags,
        "trace_id": trace_id,
        "package_id": package_id,
        "next_admin_surfaces": next_surfaces,
        "requires_human_review_before_promotion": true,
        "active_path_visible": false,
        "candidate_payloads_created": false,
    })
}

fn action_type_for_route(route: &str) -> &'static str {
    match route {
        "context_rule_candidate" => "review_context_rule_candidate_gap",
        "evidence_card_or_retrieval" => "review_evidence_card_or_retrieval_gap",
        "answer_rule_or_renderer" => "review_answer_rule_or_renderer_gap",
        "runtime_contract" => "review_runtime_contract_gap",
        _ => "manual_quality_triage",
    }
}

fn target_surface_for_route(route: &str) -> &'static str {
    match route {
        "context_rule_candidate" => "/v1/admin/rules/candidates",
        "evidence_card_or_retrieval" => "/v1/admin/evidence-card-ingest/run",
        "answer_rule_or_renderer" => "/v1/admin/traces/{trace_id}",
        "runtime_contract" => "/v1/admin/metrics",
        _ => "/v1/admin/quality/observations/{observation_id}",
    }
}

fn priority_for_severity(severity: &str) -> &'static str {
    match severity {
        "high" => "p1",
        "medium" => "p2",
        _ => "p3",
    }
}

struct ActionItemForTransition {
    status: String,
}

fn read_action_item_for_transition(
    conn: &Connection,
    action_item_id: &str,
) -> Result<ActionItemForTransition> {
    conn.query_row(
        "SELECT status FROM answer_quality_action_items WHERE action_item_id = ?1",
        params![action_item_id],
        |row| {
            Ok(ActionItemForTransition {
                status: row.get(0)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("answer quality action item not found"))
}

fn read_existing_action_status(conn: &Connection, action_item_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT status FROM answer_quality_action_items WHERE action_item_id = ?1",
        params![action_item_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn normalize_action_transition(action: &str) -> Result<&'static str> {
    match action.trim() {
        "start_review" => Ok("start_review"),
        "link" => Ok("link"),
        "close" => Ok("close"),
        "reject" => Ok("reject"),
        "requeue" => Ok("requeue"),
        other => Err(anyhow!(
            "unsupported answer quality action transition {other}"
        )),
    }
}

fn action_transition_target_status(action: &str, from_status: &str) -> Result<&'static str> {
    match (action, from_status) {
        ("start_review", "queued") => Ok("in_review"),
        ("link", "queued" | "in_review") => Ok("linked"),
        ("close", "queued" | "in_review" | "linked") => Ok("closed"),
        ("reject", "queued" | "in_review") => Ok("rejected"),
        ("requeue", "in_review" | "linked" | "closed" | "rejected") => Ok("queued"),
        _ => Err(anyhow!(
            "invalid answer quality action transition {action} from {from_status}"
        )),
    }
}

fn normalize_linked_entity_type(value: &str) -> Result<&'static str> {
    match value.trim() {
        "rule_candidate" => Ok("rule_candidate"),
        "online_evidence_card_update_request" => Ok("online_evidence_card_update_request"),
        "raw_evidence_candidate" => Ok("raw_evidence_candidate"),
        "governance_task" => Ok("governance_task"),
        "manual_review_note" => Ok("manual_review_note"),
        other => Err(anyhow!(
            "unsupported answer quality linked entity type {other}"
        )),
    }
}

fn normalize_failure_tags(tags: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let cleaned = tag.trim();
        if cleaned.is_empty() {
            continue;
        }
        let bounded: String = cleaned.chars().take(TAG_MAX_CHARS).collect();
        if seen.insert(bounded.clone()) {
            normalized.push(bounded);
        }
        if normalized.len() >= TAG_MAX_COUNT {
            break;
        }
    }
    normalized
}

fn normalize_severity(value: &str) -> Result<&'static str> {
    match value.trim() {
        "high" => Ok("high"),
        "medium" => Ok("medium"),
        "low" => Ok("low"),
        other => Err(anyhow!("unsupported answer quality severity {other}")),
    }
}

fn bounded_required(value: &str, field: &str, max_chars: usize) -> Result<String> {
    bounded_optional(Some(value), max_chars).ok_or_else(|| anyhow!("{field} is required"))
}

fn bounded_optional(value: Option<&str>, max_chars: usize) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(max_chars).collect())
    }
}

fn validate_optional_filter(value: Option<&str>, allowed: &[&str], field_name: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(anyhow!(
            "unsupported answer quality {field_name} filter {value}"
        ))
    }
}

fn allowed_statuses() -> [&'static str; 3] {
    ["observed", "triaged", "closed"]
}

fn allowed_action_statuses() -> [&'static str; 5] {
    ["queued", "in_review", "linked", "closed", "rejected"]
}

fn allowed_action_routes() -> [&'static str; 5] {
    [
        "context_rule_candidate",
        "evidence_card_or_retrieval",
        "answer_rule_or_renderer",
        "runtime_contract",
        "manual_review",
    ]
}

fn allowed_action_types() -> [&'static str; 5] {
    [
        "review_context_rule_candidate_gap",
        "review_evidence_card_or_retrieval_gap",
        "review_answer_rule_or_renderer_gap",
        "review_runtime_contract_gap",
        "manual_quality_triage",
    ]
}

fn allowed_severities() -> [&'static str; 3] {
    ["high", "medium", "low"]
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format rfc3339")
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests;
