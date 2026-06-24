use crate::{
    ClaimEvidenceMap, EvidenceCard, KnowledgeState, OnlineEvidenceCardUpdateRequestRecord,
    ReviewRecord, RuntimeWorkflowOutput, RuntimeWorkflowStepReport, append_runtime_audit_event,
    hash_text, now_rfc3339,
    online_evidence_card_ingest::{
        list_online_evidence_card_jobs_for_trace,
        list_online_evidence_card_search_requests_for_trace,
    },
    sqlite_table_exists, trim_text,
    upstream_bundle::UPSTREAM_BUNDLE_SCHEMA_VERSION,
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const ONLINE_LEARNING_TRACE_SCHEMA_VERSION: &str = "tonglingyu-online-learning-trace-v1";
pub const ONLINE_LEARNING_PROMPT_CANDIDATE_SCHEMA_VERSION: &str =
    "tonglingyu-online-learning-prompt-candidate-v1";
const ONLINE_LEARNING_SCHEMA_MIGRATION_ID: &str = "tonglingyu-online-learning-v1";

const TIER_PROMOTED_EVIDENCE_CARD: &str = "promoted_evidence_card";
const TIER_REQUEST_SCOPED_EVIDENCE: &str = "request_scoped_evidence";
const TIER_REQUEST_RAW_FULL_TEXT_HIT: &str = "request_raw_full_text_hit";
const ANSWER_USE_STABLE_BASIS: &str = "stable_basis";
const ANSWER_USE_REQUEST_BOUND_BASIS: &str = "request_bound_basis";
const ANSWER_USE_SUPPLEMENTAL_ONLY: &str = "supplemental_only";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TieredEvidenceBinding {
    pub claim_index: usize,
    pub claim: String,
    pub evidence_id: String,
    pub evidence_tier: String,
    pub answer_use: String,
    pub source_trace_id: String,
    pub source_id: String,
    pub source_hash: String,
    pub source_scope_policy_sha256: String,
    pub block_id: String,
    pub source_span_ref: Value,
    pub source_title: String,
    pub text_cue: String,
    pub claim_binding: Value,
    pub evidence_gate: Value,
    pub review_status: String,
    pub admin_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OnlineLearningTraceSummary {
    pub object: String,
    pub schema_version: String,
    pub online_learning_trace_id: String,
    pub source_trace_id: String,
    pub package_id: String,
    pub source_scope_policy: Value,
    pub tiered_evidence_bindings: Vec<TieredEvidenceBinding>,
    pub candidate_ids: Value,
    pub review_decision: Value,
    pub catalog_versions: Value,
    pub prompt_versions: Value,
}

pub(crate) fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS online_learning_prompt_candidates (
            candidate_id TEXT PRIMARY KEY,
            target_profile TEXT NOT NULL,
            operation TEXT NOT NULL,
            failure_pattern TEXT NOT NULL,
            proposed_change_summary TEXT NOT NULL,
            expected_regression_cases_json TEXT NOT NULL,
            risk_level TEXT NOT NULL,
            status TEXT NOT NULL,
            observation_count INTEGER NOT NULL,
            first_observed_at TEXT NOT NULL,
            last_observed_at TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            UNIQUE(target_profile, operation, failure_pattern)
        );

        CREATE TABLE IF NOT EXISTS online_learning_prompt_candidate_observations (
            observation_id TEXT PRIMARY KEY,
            candidate_id TEXT NOT NULL REFERENCES online_learning_prompt_candidates(candidate_id),
            trace_id TEXT NOT NULL,
            package_id TEXT NOT NULL,
            step_id TEXT NOT NULL,
            source_ref_json TEXT NOT NULL,
            review_decision_json TEXT NOT NULL,
            observed_at TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            UNIQUE(candidate_id, trace_id, step_id)
        );

        CREATE INDEX IF NOT EXISTS idx_online_learning_prompt_candidates_status
            ON online_learning_prompt_candidates(status, updated_at);
        CREATE INDEX IF NOT EXISTS idx_online_learning_prompt_candidates_failure
            ON online_learning_prompt_candidates(target_profile, operation, failure_pattern);
        CREATE INDEX IF NOT EXISTS idx_online_learning_prompt_observations_trace
            ON online_learning_prompt_candidate_observations(trace_id, observed_at);
        CREATE INDEX IF NOT EXISTS idx_online_learning_prompt_observations_candidate
            ON online_learning_prompt_candidate_observations(candidate_id, observed_at);
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![ONLINE_LEARNING_SCHEMA_MIGRATION_ID, now_rfc3339()],
    )?;
    Ok(())
}

pub(crate) fn build_online_learning_trace_summary(
    conn: &Connection,
    package_id: &str,
    trace_id: &str,
    cards: &[EvidenceCard],
    claim_evidence_map: &[ClaimEvidenceMap],
    source_scope_policy: &Value,
    review: &ReviewRecord,
) -> Result<OnlineLearningTraceSummary> {
    let source_scope_policy_sha256 = hash_json(source_scope_policy);
    let cards_by_id = cards
        .iter()
        .map(|card| (card.evidence_id.as_str(), card))
        .collect::<BTreeMap<_, _>>();
    let mut bindings = Vec::new();
    let mut seen = BTreeSet::new();
    for claim_map in claim_evidence_map {
        for evidence_id in &claim_map.evidence_ids {
            if !seen.insert((claim_map.claim_index, evidence_id.clone())) {
                continue;
            }
            let Some(card) = cards_by_id.get(evidence_id.as_str()) else {
                continue;
            };
            bindings.push(tiered_binding_for_card(
                conn,
                trace_id,
                claim_map,
                card,
                &source_scope_policy_sha256,
                review,
            )?);
        }
    }
    Ok(OnlineLearningTraceSummary {
        object: "tonglingyu.online_learning.trace_summary".to_string(),
        schema_version: ONLINE_LEARNING_TRACE_SCHEMA_VERSION.to_string(),
        online_learning_trace_id: format!(
            "olt-{}",
            &hash_text(&format!(
                "{trace_id}:{package_id}:{source_scope_policy_sha256}"
            ))[..32]
        ),
        source_trace_id: trace_id.to_string(),
        package_id: package_id.to_string(),
        source_scope_policy: source_scope_policy.clone(),
        tiered_evidence_bindings: bindings,
        candidate_ids: json!({
            "evidence": [],
            "rule": [],
            "prompt": [],
        }),
        review_decision: json!({
            "status": &review.status,
            "severity": &review.severity,
            "issues": &review.issues,
        }),
        catalog_versions: json!({
            "online_learning_trace_schema_version": ONLINE_LEARNING_TRACE_SCHEMA_VERSION,
            "source_scope_policy_sha256": source_scope_policy_sha256,
        }),
        prompt_versions: json!({
            "upstream_bundle_schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
        }),
    })
}

pub(crate) fn record_agent_runtime_online_learning_assets(
    conn: &Connection,
    workflow: &RuntimeWorkflowOutput,
    runtime_mode: &str,
    retrieval_repair_search_request_count: usize,
) -> Result<Option<Value>> {
    init_schema(conn)?;
    let profile_steps = workflow
        .steps
        .iter()
        .filter(|step| step.agent_runtime.is_some())
        .collect::<Vec<_>>();
    let draft_steps = profile_steps
        .iter()
        .copied()
        .filter(|step| step.operation == "draft_answer")
        .collect::<Vec<_>>();
    if draft_steps.is_empty() {
        return Ok(None);
    }

    let search_requests =
        list_online_evidence_card_search_requests_for_trace(conn, &workflow.trace_id, 100)?;
    let mut prompt_candidate_ids = Vec::new();
    let mut failure_patterns = Vec::new();
    for step in &profile_steps {
        for failure_pattern in prompt_failure_patterns(step) {
            push_unique_string(&mut failure_patterns, &failure_pattern);
            let candidate_id = stage_prompt_candidate_from_step(
                conn,
                workflow,
                step,
                &failure_pattern,
                Some("profile_output"),
                None,
            )?;
            push_unique_string(&mut prompt_candidate_ids, &candidate_id);
        }
    }

    let full_text_search_requests = draft_steps
        .iter()
        .flat_map(|step| {
            step.output
                .get("agent_runtime_retrieval_repair_queries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "object": "tonglingyu.online_learning.llm_assets",
        "schema_version": ONLINE_LEARNING_TRACE_SCHEMA_VERSION,
        "source_trace_id": &workflow.trace_id,
        "package_id": &workflow.package.package_id,
        "runtime_mode": runtime_mode,
        "llm_semantic_parse_ref": draft_steps
            .iter()
            .filter_map(|step| agent_runtime_string(step, "result_ref"))
            .next(),
        "source_profile_refs": draft_steps
            .iter()
            .map(|step| llm_asset_step_ref(step))
            .collect::<Vec<_>>(),
        "full_text_search_requests": full_text_search_requests,
        "persisted_full_text_search_request_ids": search_requests
            .iter()
            .map(|request| request.search_request_id.clone())
            .collect::<Vec<_>>(),
        "retrieval_repair_search_request_count": retrieval_repair_search_request_count,
        "coverage_statuses": draft_steps
            .iter()
            .filter_map(|step| step.output.get("agent_runtime_coverage_status").cloned())
            .collect::<Vec<_>>(),
        "failure_patterns": failure_patterns,
        "candidate_ids": {
            "evidence": {
                "search_request_ids": search_requests
                    .iter()
                    .map(|request| request.search_request_id.clone())
                    .collect::<Vec<_>>(),
            },
            "rule": [],
            "prompt": prompt_candidate_ids,
        },
    });
    append_runtime_audit_event(
        conn,
        &workflow.trace_id,
        "online_learning_llm_assets_recorded",
        &payload,
    )?;
    Ok(Some(payload))
}

pub(crate) fn record_agent_runtime_prompt_failure_candidate(
    conn: &Connection,
    workflow: &RuntimeWorkflowOutput,
    failure_stage: &str,
    error: &anyhow::Error,
) -> Result<Option<String>> {
    init_schema(conn)?;
    let error_text = error.to_string();
    let Some(failure_pattern) = prompt_failure_pattern_from_error(failure_stage, &error_text)
    else {
        return Ok(None);
    };
    let Some(step) = prompt_candidate_step_for_failure(workflow, failure_stage, &error_text) else {
        return Ok(None);
    };
    let candidate_id = stage_prompt_candidate_from_step(
        conn,
        workflow,
        step,
        &failure_pattern,
        Some(failure_stage),
        Some(&error_text),
    )?;
    Ok(Some(candidate_id))
}

pub(crate) fn record_online_learning_candidate_refs(
    conn: &Connection,
    trace_id: &str,
    online_learning_trace: Option<&OnlineLearningTraceSummary>,
    update_request: Option<&OnlineEvidenceCardUpdateRequestRecord>,
) -> Result<Option<Value>> {
    let Some(online_learning_trace) = online_learning_trace else {
        return Ok(None);
    };
    let Some(update_request) = update_request else {
        return Ok(None);
    };
    let search_requests = list_online_evidence_card_search_requests_for_trace(conn, trace_id, 100)?;
    let jobs = list_online_evidence_card_jobs_for_trace(conn, trace_id, 100)?;
    let payload = json!({
        "object": "tonglingyu.online_learning.candidate_refs",
        "schema_version": ONLINE_LEARNING_TRACE_SCHEMA_VERSION,
        "online_learning_trace_id": &online_learning_trace.online_learning_trace_id,
        "source_trace_id": trace_id,
        "package_id": &online_learning_trace.package_id,
        "candidate_ids": {
            "evidence": {
                "update_request_id": &update_request.update_request_id,
                "search_request_ids": search_requests.iter().map(|request| request.search_request_id.clone()).collect::<Vec<_>>(),
                "worker_job_ids": jobs.iter().map(|job| job.job_id.clone()).collect::<Vec<_>>(),
            },
            "rule": [],
            "prompt": [],
        },
        "evidence_update_request": {
            "update_request_id": &update_request.update_request_id,
            "status": &update_request.status,
            "coverage_gap_reason": &update_request.coverage_gap_reason,
        },
    });
    append_runtime_audit_event(
        conn,
        trace_id,
        "online_learning_candidate_refs_recorded",
        &payload,
    )?;
    Ok(Some(payload))
}

pub fn list_online_learning_prompt_candidates_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    init_schema(conn)?;
    let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
    let mut stmt = conn.prepare(
        r#"
        SELECT c.candidate_id, c.target_profile, c.operation, c.failure_pattern,
               c.proposed_change_summary, c.expected_regression_cases_json,
               c.risk_level, c.status, c.observation_count,
               c.first_observed_at, c.last_observed_at, c.created_at,
               c.updated_at, c.schema_version,
               o.observation_id, o.trace_id, o.package_id, o.step_id,
               o.source_ref_json, o.review_decision_json, o.observed_at,
               o.schema_version
        FROM online_learning_prompt_candidate_observations o
        JOIN online_learning_prompt_candidates c
          ON c.candidate_id = o.candidate_id
        WHERE o.trace_id = ?1
        ORDER BY o.observed_at, c.candidate_id
        LIMIT ?2
        "#,
    )?;
    stmt.query_map(params![trace_id, limit], |row| {
        let expected_regression_cases_json: String = row.get(5)?;
        let source_ref_json: String = row.get(18)?;
        let review_decision_json: String = row.get(19)?;
        Ok(json!({
            "candidate_id": row.get::<_, String>(0)?,
            "target_profile": row.get::<_, String>(1)?,
            "operation": row.get::<_, String>(2)?,
            "failure_pattern": row.get::<_, String>(3)?,
            "proposed_change_summary": row.get::<_, String>(4)?,
            "expected_regression_cases": serde_json::from_str::<Value>(&expected_regression_cases_json).unwrap_or(Value::Null),
            "risk_level": row.get::<_, String>(6)?,
            "status": row.get::<_, String>(7)?,
            "observation_count": row.get::<_, i64>(8)?,
            "first_observed_at": row.get::<_, String>(9)?,
            "last_observed_at": row.get::<_, String>(10)?,
            "created_at": row.get::<_, String>(11)?,
            "updated_at": row.get::<_, String>(12)?,
            "schema_version": row.get::<_, String>(13)?,
            "observation": {
                "observation_id": row.get::<_, String>(14)?,
                "trace_id": row.get::<_, String>(15)?,
                "package_id": row.get::<_, String>(16)?,
                "step_id": row.get::<_, String>(17)?,
                "source_ref": serde_json::from_str::<Value>(&source_ref_json).unwrap_or(Value::Null),
                "review_decision": serde_json::from_str::<Value>(&review_decision_json).unwrap_or(Value::Null),
                "observed_at": row.get::<_, String>(20)?,
                "schema_version": row.get::<_, String>(21)?,
            },
        }))
    })?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(Into::into)
}

fn tiered_binding_for_card(
    conn: &Connection,
    trace_id: &str,
    claim_map: &ClaimEvidenceMap,
    card: &EvidenceCard,
    source_scope_policy_sha256: &str,
    review: &ReviewRecord,
) -> Result<TieredEvidenceBinding> {
    let source_hash = source_hash_for_card(conn, card)?;
    let source_span_ref = source_span_ref_for_card(card, &source_hash);
    let text_cue = trim_text(&card.text, 180);
    let claim_binding = json!({
        "claim_index": claim_map.claim_index,
        "support_relation": "supports_scope_limited_claim",
        "forbidden_conclusions": &claim_map.forbidden_conclusions,
    });
    let gate = evidence_gate_for_card(
        trace_id,
        card,
        &source_hash,
        source_scope_policy_sha256,
        &source_span_ref,
        &text_cue,
        claim_map,
        review,
    );
    let evidence_tier = evidence_tier_for_card(card, claim_map, &gate).to_string();
    let answer_use = answer_use_for_tier(&evidence_tier).to_string();
    let evidence_gate = finalize_evidence_gate(gate, &evidence_tier, &answer_use);
    Ok(TieredEvidenceBinding {
        claim_index: claim_map.claim_index,
        claim: claim_map.claim.clone(),
        evidence_id: card.evidence_id.clone(),
        evidence_tier,
        answer_use,
        source_trace_id: trace_id.to_string(),
        source_id: card.source_id.clone(),
        source_hash: source_hash.value,
        source_scope_policy_sha256: source_scope_policy_sha256.to_string(),
        block_id: card.block_id.clone(),
        source_span_ref,
        source_title: trim_text(&card.source_title, 120),
        text_cue,
        claim_binding,
        evidence_gate,
        review_status: review.status.clone(),
        admin_only: true,
    })
}

fn evidence_tier_for_card(
    card: &EvidenceCard,
    claim_map: &ClaimEvidenceMap,
    gate: &Value,
) -> &'static str {
    if claim_has_stable_knowledge_ref(claim_map) {
        return TIER_PROMOTED_EVIDENCE_CARD;
    }
    if card.verification_status == "online_promoted_source_backed"
        || card.evidence_id.starts_with("evc-")
    {
        return TIER_PROMOTED_EVIDENCE_CARD;
    }
    if gate
        .get("request_scoped_evidence_ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return TIER_REQUEST_SCOPED_EVIDENCE;
    }
    TIER_REQUEST_RAW_FULL_TEXT_HIT
}

fn claim_has_stable_knowledge_ref(claim_map: &ClaimEvidenceMap) -> bool {
    claim_map.knowledge_item_refs.iter().any(|item| {
        matches!(
            item.state,
            KnowledgeState::RuntimeUsable | KnowledgeState::HumanMarked
        )
    })
}

fn answer_use_for_tier(evidence_tier: &str) -> &'static str {
    match evidence_tier {
        TIER_PROMOTED_EVIDENCE_CARD => ANSWER_USE_STABLE_BASIS,
        TIER_REQUEST_SCOPED_EVIDENCE => ANSWER_USE_REQUEST_BOUND_BASIS,
        _ => ANSWER_USE_SUPPLEMENTAL_ONLY,
    }
}

fn prompt_failure_patterns(step: &RuntimeWorkflowStepReport) -> Vec<String> {
    let mut patterns = Vec::new();
    if step.operation == "draft_answer" {
        if let Some(reason) = output_string(step, "agent_runtime_draft_rejected_reason") {
            push_unique_string(&mut patterns, &format!("draft_rejected:{reason}"));
        }
        if step
            .output
            .get("agent_runtime_draft_repair_attempted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !step
                .output
                .get("agent_runtime_draft_consumed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            let reason = output_string(step, "agent_runtime_draft_rejected_reason")
                .or_else(|| output_string(step, "agent_runtime_initial_draft_rejected_reason"))
                .unwrap_or_else(|| "repair_output_rejected".to_string());
            push_unique_string(&mut patterns, &format!("draft_repair_rejected:{reason}"));
        }
    }
    if step.operation == "review_answer"
        && let Some(reason) = output_string(step, "agent_runtime_review_rejected_reason")
    {
        push_unique_string(
            &mut patterns,
            &format!("review_observation_rejected:{reason}"),
        );
    }
    patterns
}

fn output_string(step: &RuntimeWorkflowStepReport, key: &str) -> Option<String> {
    step.output
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| *value != "null")
        .map(ToOwned::to_owned)
}

fn prompt_failure_pattern_from_error(failure_stage: &str, error_text: &str) -> Option<String> {
    if error_text.contains("message exceeded safety budget") {
        return Some(format!("oversized_prompt:{failure_stage}"));
    }
    if failure_stage == "agent_runtime_draft_repair" {
        return Some("draft_repair_failed:runtime_error".to_string());
    }
    None
}

fn prompt_candidate_step_for_failure<'a>(
    workflow: &'a RuntimeWorkflowOutput,
    failure_stage: &str,
    error_text: &str,
) -> Option<&'a RuntimeWorkflowStepReport> {
    if failure_stage == "agent_runtime_draft_repair"
        || error_text.contains("operation=draft_answer")
        || error_text.contains("draft repair message exceeded safety budget")
    {
        return workflow
            .steps
            .iter()
            .find(|step| step.operation == "draft_answer");
    }
    if error_text.contains("operation=review_answer") {
        return workflow
            .steps
            .iter()
            .find(|step| step.operation == "review_answer");
    }
    None
}

fn stage_prompt_candidate_from_step(
    conn: &Connection,
    workflow: &RuntimeWorkflowOutput,
    step: &RuntimeWorkflowStepReport,
    failure_pattern: &str,
    failure_stage: Option<&str>,
    error_text: Option<&str>,
) -> Result<String> {
    let candidate_hash = hash_text(&format!(
        "{}:{}:{}",
        step.profile, step.operation, failure_pattern
    ));
    let candidate_id = format!("prompt-candidate-{}", &candidate_hash[..16]);
    let now = now_rfc3339();
    let expected_regression_cases = json!([{
        "trace_id": &workflow.trace_id,
        "package_id": &workflow.package.package_id,
        "question_sha256": hash_text(&workflow.question),
        "failure_pattern": failure_pattern,
    }]);
    let proposed_change_summary = format!(
        "Review {} {} prompt for failure pattern {}; preserve evidence package, source scope, and public wording boundaries.",
        step.profile, step.operation, failure_pattern
    );
    conn.execute(
        r#"
        INSERT INTO online_learning_prompt_candidates (
            candidate_id, target_profile, operation, failure_pattern,
            proposed_change_summary, expected_regression_cases_json, risk_level,
            status, observation_count, first_observed_at, last_observed_at,
            created_at, updated_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'medium',
                  'staged', 0, ?7, ?7, ?7, ?7, ?8)
        ON CONFLICT(target_profile, operation, failure_pattern) DO UPDATE SET
            proposed_change_summary = excluded.proposed_change_summary,
            expected_regression_cases_json = excluded.expected_regression_cases_json,
            last_observed_at = excluded.last_observed_at,
            updated_at = excluded.updated_at
        "#,
        params![
            &candidate_id,
            &step.profile,
            &step.operation,
            failure_pattern,
            &proposed_change_summary,
            serde_json::to_string(&expected_regression_cases)?,
            &now,
            ONLINE_LEARNING_PROMPT_CANDIDATE_SCHEMA_VERSION,
        ],
    )?;
    let observation_hash = hash_text(&format!(
        "{}:{}:{}",
        candidate_id, workflow.trace_id, step.step_id
    ));
    let observation_id = format!("prompt-candidate-observation-{}", &observation_hash[..16]);
    let source_ref = json!({
        "step_id": &step.step_id,
        "failure_stage": failure_stage,
        "profile": &step.profile,
        "operation": &step.operation,
        "output_ref": &step.output_ref,
        "error_sha256": error_text.map(hash_text),
        "result_ref": agent_runtime_string(step, "result_ref"),
        "provider_request_sha256": agent_runtime_string(step, "provider_request_sha256"),
        "content_source": agent_runtime_string(step, "content_source"),
        "result_format": step.output.get("agent_runtime_result_format").cloned().unwrap_or(Value::Null),
        "coverage_status": step.output.get("agent_runtime_coverage_status").cloned().unwrap_or(Value::Null),
        "retrieval_repair_recommended": step.output.get("agent_runtime_retrieval_repair_recommended").cloned().unwrap_or(Value::Null),
        "retrieval_repair_query_count": step.output.get("agent_runtime_retrieval_repair_query_count").cloned().unwrap_or(Value::Null),
    });
    let review_decision = json!({
        "package_review_status": &workflow.package.review.status,
        "package_review_severity": &workflow.package.review.severity,
        "package_review_issues": &workflow.package.review.issues,
        "failure_pattern": failure_pattern,
    });
    let inserted = conn.execute(
        r#"
        INSERT OR IGNORE INTO online_learning_prompt_candidate_observations (
            observation_id, candidate_id, trace_id, package_id, step_id,
            source_ref_json, review_decision_json, observed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            &observation_id,
            &candidate_id,
            &workflow.trace_id,
            &workflow.package.package_id,
            &step.step_id,
            serde_json::to_string(&source_ref)?,
            serde_json::to_string(&review_decision)?,
            &now,
            ONLINE_LEARNING_PROMPT_CANDIDATE_SCHEMA_VERSION,
        ],
    )? == 1;
    if inserted {
        conn.execute(
            r#"
            UPDATE online_learning_prompt_candidates
            SET observation_count = observation_count + 1,
                last_observed_at = ?1,
                updated_at = ?1
            WHERE candidate_id = ?2
            "#,
            params![&now, &candidate_id],
        )?;
    }
    Ok(candidate_id)
}

fn llm_asset_step_ref(step: &RuntimeWorkflowStepReport) -> Value {
    json!({
        "step_id": &step.step_id,
        "profile": &step.profile,
        "operation": &step.operation,
        "output_ref": &step.output_ref,
        "result_ref": agent_runtime_string(step, "result_ref"),
        "provider_request_sha256": agent_runtime_string(step, "provider_request_sha256"),
        "content_source": agent_runtime_string(step, "content_source"),
        "result_format": step.output.get("agent_runtime_result_format").cloned().unwrap_or(Value::Null),
        "draft_consumed": step.output.get("agent_runtime_draft_consumed").cloned().unwrap_or(Value::Null),
        "draft_rejected_reason": step.output.get("agent_runtime_draft_rejected_reason").cloned().unwrap_or(Value::Null),
        "coverage_status": step.output.get("agent_runtime_coverage_status").cloned().unwrap_or(Value::Null),
    })
}

fn agent_runtime_string(step: &RuntimeWorkflowStepReport, key: &str) -> Option<String> {
    step.agent_runtime
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

struct SourceHashStatus {
    value: String,
    status: &'static str,
}

fn source_hash_for_card(conn: &Connection, card: &EvidenceCard) -> Result<SourceHashStatus> {
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
    if let Some(value) = source_hash.filter(|value| !value.trim().is_empty()) {
        return Ok(SourceHashStatus {
            value,
            status: "source_snapshot_hash",
        });
    }
    Ok(SourceHashStatus {
        value: hash_text(&format!(
            "{}:{}:{}",
            card.source_id, card.block_id, card.source_title
        )),
        status: "derived_fallback_hash",
    })
}

fn source_span_ref_for_card(card: &EvidenceCard, source_hash: &SourceHashStatus) -> Value {
    json!({
        "source_id": &card.source_id,
        "source_hash": &source_hash.value,
        "source_hash_status": source_hash.status,
        "block_id": &card.block_id,
        "source_title": &card.source_title,
        "span_start": 0,
        "span_end": card.text.chars().count(),
        "text_sha256": hash_text(&card.text),
    })
}

#[allow(clippy::too_many_arguments)]
fn evidence_gate_for_card(
    trace_id: &str,
    card: &EvidenceCard,
    source_hash: &SourceHashStatus,
    source_scope_policy_sha256: &str,
    source_span_ref: &Value,
    text_cue: &str,
    claim_map: &ClaimEvidenceMap,
    review: &ReviewRecord,
) -> Value {
    let mut missing = Vec::new();
    if card.source_id.trim().is_empty() {
        missing.push("source_id");
    }
    if source_hash.status != "source_snapshot_hash" {
        missing.push("source_hash");
    }
    if source_scope_policy_sha256.trim().is_empty() {
        missing.push("source_scope_policy");
    }
    if source_span_ref
        .get("block_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing.push("source_span_ref");
    }
    if text_cue.trim().is_empty() {
        missing.push("text_cue");
    }
    if claim_map.claim.trim().is_empty() {
        missing.push("claim_binding");
    }
    if trace_id.trim().is_empty() {
        missing.push("source_trace_id");
    }
    if review.status.trim().is_empty() {
        missing.push("review_decision");
    }
    json!({
        "schema_version": ONLINE_LEARNING_TRACE_SCHEMA_VERSION,
        "request_scoped_evidence_ready": missing.is_empty(),
        "status": if missing.is_empty() { "passed" } else { "downgraded" },
        "minimum_required_fields": [
            "source_id",
            "source_hash",
            "source_scope_policy",
            "source_span_ref",
            "text_cue",
            "claim_binding",
            "evidence_tier",
            "source_trace_id",
            "review_decision"
        ],
        "missing_required_fields": missing,
        "source_hash_status": source_hash.status,
    })
}

fn finalize_evidence_gate(mut gate: Value, evidence_tier: &str, answer_use: &str) -> Value {
    if let Some(object) = gate.as_object_mut() {
        object.insert("decision_evidence_tier".to_string(), json!(evidence_tier));
        object.insert("decision_answer_use".to_string(), json!(answer_use));
    }
    gate
}

fn hash_json(value: &Value) -> String {
    let encoded = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    hash_text(&encoded)
}

#[cfg(test)]
mod tests;
