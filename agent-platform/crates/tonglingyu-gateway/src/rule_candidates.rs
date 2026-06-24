use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use time::OffsetDateTime;
use tonglingyu_runtime::{
    PromotionManifestInput, RuntimeRuleCandidatePromotionInput, RuntimeRuleCandidatePromotionPaths,
    active_runtime_rule_candidate_matches, is_runtime_rule_candidate_type,
    promote_runtime_rule_candidate_to_catalog, record_online_learning_promotion_manifest,
    runtime_rule_candidate_target_requirement,
};

use crate::{
    context_rules::{
        self, ELLIPSIS_RESOLUTION_RULES_PATH_ENV, QUESTION_FRAME_RULES_PATH_ENV,
        SUBJECT_ONTOLOGY_PATH_ENV,
    },
    llm_agent_contracts::{RULE_CANDIDATE_TYPES, RuleCandidateSuggestion},
    question_frame::QuestionFrame,
};

pub(crate) const RULE_CANDIDATE_SCHEMA_VERSION: &str = "tonglingyu-rule-candidate-v1";
pub(crate) const RULE_CANDIDATE_PREFLIGHT_SCHEMA_VERSION: &str =
    "tonglingyu-rule-candidate-preflight-v1";
pub(crate) const RULE_CANDIDATE_PROMOTION_SCHEMA_VERSION: &str =
    "tonglingyu-rule-candidate-promotion-v1";
pub(crate) const RULE_CANDIDATE_TRANSITION_SCHEMA_VERSION: &str =
    "tonglingyu-rule-candidate-transition-v1";
pub(crate) const RULE_CANDIDATE_REGRESSION_EVIDENCE_SCHEMA_VERSION: &str =
    "tonglingyu-rule-candidate-regression-evidence-v1";
pub(crate) const RULE_CANDIDATE_REVIEW_RUN_SCHEMA_VERSION: &str =
    "tonglingyu-rule-candidate-review-run-v1";

#[derive(Debug, Clone, Default)]
pub(crate) struct RuleCandidatePromotionPaths {
    pub(crate) subject_ontology_path: Option<PathBuf>,
    pub(crate) question_frame_rules_path: Option<PathBuf>,
    pub(crate) ellipsis_resolution_rules_path: Option<PathBuf>,
    pub(crate) runtime_paths: RuntimeRuleCandidatePromotionPaths,
}

impl RuleCandidatePromotionPaths {
    pub(crate) fn from_env() -> Self {
        Self {
            subject_ontology_path: configured_path(SUBJECT_ONTOLOGY_PATH_ENV),
            question_frame_rules_path: configured_path(QUESTION_FRAME_RULES_PATH_ENV),
            ellipsis_resolution_rules_path: configured_path(ELLIPSIS_RESOLUTION_RULES_PATH_ENV),
            runtime_paths: RuntimeRuleCandidatePromotionPaths::from_env(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuleCandidatePromotionInput {
    pub(crate) actor: String,
    pub(crate) reason: String,
    pub(crate) target_ref: Option<String>,
    pub(crate) catalog_version: Option<String>,
    pub(crate) paths: RuleCandidatePromotionPaths,
}

#[derive(Debug, Clone)]
pub(crate) struct RuleCandidateTransitionInput<'a> {
    pub(crate) candidate_id: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) action: &'a str,
    pub(crate) reason: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct RuleCandidateRegressionEvidenceInput<'a> {
    pub(crate) candidate_id: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) suite_ref: &'a str,
    pub(crate) report_ref: &'a str,
    pub(crate) report_sha256: &'a str,
    pub(crate) case_count: i64,
    pub(crate) passed_count: i64,
    pub(crate) failed_count: i64,
    pub(crate) skipped_count: i64,
    pub(crate) notes: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuleCandidateReviewRunInput<'a> {
    pub(crate) candidate_id: &'a str,
    pub(crate) actor: &'a str,
    pub(crate) reopen_reason: &'a str,
    pub(crate) suite_ref: &'a str,
    pub(crate) report_ref: &'a str,
    pub(crate) report_sha256: &'a str,
    pub(crate) case_count: i64,
    pub(crate) passed_count: i64,
    pub(crate) failed_count: i64,
    pub(crate) skipped_count: i64,
    pub(crate) notes: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuleCandidateStagingReport {
    staged_count: usize,
    blocked_count: usize,
    observation_count: usize,
    candidate_refs: Vec<String>,
}

impl RuleCandidateStagingReport {
    pub(crate) fn merge(&mut self, other: RuleCandidateStagingReport) {
        self.staged_count += other.staged_count;
        self.blocked_count += other.blocked_count;
        self.observation_count += other.observation_count;
        self.candidate_refs.extend(other.candidate_refs);
    }

    pub(crate) fn audit_json(&self) -> Value {
        json!({
            "schema_version": RULE_CANDIDATE_SCHEMA_VERSION,
            "staged_count": self.staged_count,
            "blocked_count": self.blocked_count,
            "observation_count": self.observation_count,
            "candidate_refs": self.candidate_refs,
            "active_path_visible": false,
        })
    }
}

pub(crate) struct RuleCandidateObservationInput<'a> {
    pub(crate) trace_id: &'a str,
    pub(crate) user_session_id: &'a str,
    pub(crate) interaction_context_id: &'a str,
    pub(crate) context_pack_id: &'a str,
    pub(crate) external_message_id: &'a str,
    pub(crate) source_question: &'a str,
    pub(crate) resolved_question: &'a str,
    pub(crate) question_frame: &'a QuestionFrame,
    pub(crate) agent_audit: Option<&'a Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuleCandidateListInput<'a> {
    pub(crate) status: Option<&'a str>,
    pub(crate) candidate_type: Option<&'a str>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

pub(crate) fn stage_rule_candidates_from_agent_audit(
    conn: &Connection,
    input: RuleCandidateObservationInput<'_>,
) -> Result<RuleCandidateStagingReport> {
    let Some(agent_audit) = input.agent_audit else {
        return Ok(RuleCandidateStagingReport::default());
    };
    if !agent_audit
        .get("contract_accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(RuleCandidateStagingReport::default());
    }
    let candidates = agent_audit
        .get("rule_candidates")
        .cloned()
        .map(serde_json::from_value::<Vec<RuleCandidateSuggestion>>)
        .transpose()?
        .unwrap_or_default();
    if candidates.is_empty() {
        return Ok(RuleCandidateStagingReport::default());
    }

    stage_rule_candidates_from_suggestions(
        conn,
        input,
        &candidates,
        agent_audit,
        "question_normalizer_agent",
    )
}

pub(crate) fn stage_rule_candidates_from_suggestions(
    conn: &Connection,
    input: RuleCandidateObservationInput<'_>,
    candidates: &[RuleCandidateSuggestion],
    validator_audit: &Value,
    created_by: &str,
) -> Result<RuleCandidateStagingReport> {
    if candidates.is_empty() {
        return Ok(RuleCandidateStagingReport::default());
    }

    let mut report = RuleCandidateStagingReport::default();
    for candidate in candidates {
        let staged =
            stage_single_rule_candidate(conn, &input, candidate, validator_audit, created_by)?;
        report.observation_count += staged.observation_inserted as usize;
        if staged.status == "staged" {
            report.staged_count += 1;
        } else {
            report.blocked_count += 1;
        }
        report.candidate_refs.push(staged.candidate_ref);
    }
    Ok(report)
}

pub(crate) fn run_rule_candidate_preflight(
    conn: &Connection,
    candidate_id: &str,
    actor: &str,
) -> Result<Value> {
    let candidate = read_rule_candidate_for_preflight(conn, candidate_id)?;
    let mut errors = Vec::<String>::new();
    let mut checks = Vec::<Value>::new();

    let schema_passed = candidate.schema_version == RULE_CANDIDATE_SCHEMA_VERSION
        && RULE_CANDIDATE_TYPES.contains(&candidate.candidate_type.as_str())
        && !candidate.term_key.trim().is_empty()
        && !candidate.primary_term.trim().is_empty()
        && candidate.status == "staged";
    if !schema_passed {
        errors.push("schema_or_status_check_failed".to_string());
    }
    checks.push(json!({
        "check": "schema",
        "passed": schema_passed,
        "required_status": "staged",
        "actual_status": candidate.status,
    }));

    let active_matches =
        active_rule_candidate_matches(&candidate.candidate_type, &candidate.primary_term, None)?;
    let conflict_passed = candidate.conflict_status == "none" && active_matches.is_empty();
    if !conflict_passed {
        errors.push("active_conflict_check_failed".to_string());
    }
    checks.push(json!({
        "check": "active_conflict",
        "passed": conflict_passed,
        "stored_conflict_status": candidate.conflict_status,
        "active_rule_refs": active_matches
            .into_iter()
            .map(|active| json!({
                "candidate_type": active.candidate_type,
                "rule_ref": active.rule_ref,
            }))
            .collect::<Vec<_>>(),
    }));

    let observation_count = rule_candidate_observation_count(conn, &candidate.candidate_id)?;
    let regression_gate_min_created_at = regression_gate_min_created_at(conn, &candidate)?;
    let latest_regression_evidence =
        latest_rule_candidate_regression_evidence(conn, &candidate.candidate_id)?;
    let latest_regression_status = latest_regression_evidence
        .as_ref()
        .and_then(|evidence| evidence.get("status"))
        .and_then(Value::as_str);
    let latest_regression_created_at = latest_regression_evidence
        .as_ref()
        .and_then(|evidence| evidence.get("created_at"))
        .and_then(Value::as_str);
    let regression_evidence_current = latest_regression_created_at
        .is_some_and(|created_at| created_at >= regression_gate_min_created_at.as_str());
    let regression_passed = observation_count >= 1
        && candidate.observation_count >= observation_count
        && latest_regression_status == Some("passed")
        && regression_evidence_current;
    if !regression_passed {
        errors.push("regression_evidence_check_failed".to_string());
    }
    checks.push(json!({
        "check": "regression",
        "passed": regression_passed,
        "observation_count": observation_count,
        "candidate_observation_count": candidate.observation_count,
        "minimum_evidence_created_at": regression_gate_min_created_at,
        "latest_regression_evidence": latest_regression_evidence,
        "latest_regression_evidence_current": regression_evidence_current,
        "requires_eval_before_active_promotion": true,
    }));

    let hardcode_passed = hardcode_check_passed(&candidate);
    if !hardcode_passed {
        errors.push("hardcode_boundary_check_failed".to_string());
    }
    checks.push(json!({
        "check": "hardcode",
        "passed": hardcode_passed,
        "active_rule_may_encode_answer": false,
    }));

    let status = if errors.is_empty() {
        "passed"
    } else {
        "failed"
    };
    let next_candidate_status = if errors.is_empty() {
        "ready_for_review"
    } else {
        "preflight_failed"
    };
    let run_id = format!("rule-candidate-preflight-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO rule_candidate_preflight_runs (
            run_id, candidate_id, actor, status, checks_json, errors_json,
            started_at, completed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            &run_id,
            &candidate.candidate_id,
            non_empty_or(actor, "system"),
            status,
            serde_json::to_string(&checks)?,
            serde_json::to_string(&errors)?,
            &now,
            &now,
            RULE_CANDIDATE_PREFLIGHT_SCHEMA_VERSION,
        ],
    )?;
    conn.execute(
        "UPDATE rule_candidates
         SET status = ?1,
             last_preflight_run_id = ?2,
             preflight_status = ?3,
             updated_at = ?4
         WHERE candidate_id = ?5",
        params![
            next_candidate_status,
            &run_id,
            status,
            &now,
            &candidate.candidate_id
        ],
    )?;
    Ok(json!({
        "object": "tonglingyu.rule_candidate_preflight",
        "schema_version": RULE_CANDIDATE_PREFLIGHT_SCHEMA_VERSION,
        "run_id": run_id,
        "candidate_id": candidate.candidate_id,
        "candidate_ref": candidate.candidate_ref,
        "status": status,
        "candidate_status": next_candidate_status,
        "checks": checks,
        "errors": errors,
        "active_path_visible": false,
    }))
}

pub(crate) fn promote_rule_candidate(
    conn: &Connection,
    candidate_id: &str,
    input: RuleCandidatePromotionInput,
) -> Result<Value> {
    let candidate = read_rule_candidate_for_promotion(conn, candidate_id)?;
    let actor = non_empty_or(&input.actor, "system").to_string();
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(anyhow!("rule candidate promotion reason is required"));
    }

    let run_id = format!("rule-candidate-promotion-{}", uuid::Uuid::now_v7().simple());
    let expected_version_bump = input
        .catalog_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("online.{run_id}"));
    let source_trace_refs = rule_candidate_source_trace_refs(conn, &candidate.candidate_id)?;
    let latest_regression =
        latest_rule_candidate_regression_evidence(conn, &candidate.candidate_id)?;
    let mut errors = Vec::<String>::new();
    if candidate.status != "ready_for_review"
        || candidate.preflight_status.as_deref() != Some("passed")
    {
        errors.push("candidate_not_ready_for_promotion".to_string());
    }
    if candidate.schema_version != RULE_CANDIDATE_SCHEMA_VERSION {
        errors.push("candidate_schema_mismatch".to_string());
    }
    let active_matches = active_rule_candidate_matches(
        &candidate.candidate_type,
        &candidate.primary_term,
        Some(&input.paths),
    )?;
    let active_rule_refs = active_matches
        .iter()
        .map(|active| {
            json!({
                "candidate_type": active.candidate_type,
                "rule_ref": active.rule_ref,
            })
        })
        .collect::<Vec<_>>();
    if candidate.conflict_status != "none" || !active_matches.is_empty() {
        errors.push("active_conflict_check_failed".to_string());
    }
    if !hardcode_term_check_passed(&candidate.primary_term) {
        errors.push("hardcode_boundary_check_failed".to_string());
    }

    let now = now_rfc3339();
    if !errors.is_empty() {
        let promotion_manifest = record_rule_promotion_manifest(
            conn,
            &candidate,
            &input,
            &actor,
            reason,
            &run_id,
            &expected_version_bump,
            source_trace_refs,
            latest_regression,
            active_rule_refs,
            &errors,
            None,
        )?;
        let promotion_batch_id = promotion_manifest
            .get("promotion_batch_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("promotion manifest missing promotion_batch_id"))?
            .to_string();
        insert_promotion_run(
            conn,
            &run_id,
            &candidate,
            &actor,
            "failed",
            input.target_ref.as_deref(),
            None,
            None,
            None,
            None,
            Some(&promotion_batch_id),
            &errors,
            &now,
        )?;
        update_promotion_status(
            conn,
            &candidate.candidate_id,
            "promotion_failed",
            &run_id,
            &now,
        )?;
        return Ok(json!({
            "object": "tonglingyu.rule_candidate_promotion",
            "schema_version": RULE_CANDIDATE_PROMOTION_SCHEMA_VERSION,
            "run_id": run_id,
            "candidate_id": candidate.candidate_id,
            "candidate_ref": candidate.candidate_ref,
            "status": "failed",
            "candidate_status": "promotion_failed",
            "promotion_batch_id": promotion_batch_id,
            "promotion_manifest": promotion_manifest,
            "errors": errors,
            "active_path_visible": false,
        }));
    }

    let patch = match apply_candidate_to_external_catalog(&candidate, &input, &run_id) {
        Ok(patch) => patch,
        Err(error) => {
            let errors = vec![format!(
                "rule candidate promotion apply failed: {}: {error}",
                candidate.candidate_id
            )];
            let promotion_manifest = record_rule_promotion_manifest(
                conn,
                &candidate,
                &input,
                &actor,
                reason,
                &run_id,
                &expected_version_bump,
                rule_candidate_source_trace_refs(conn, &candidate.candidate_id)?,
                latest_rule_candidate_regression_evidence(conn, &candidate.candidate_id)?,
                Vec::new(),
                &errors,
                None,
            )?;
            let promotion_batch_id = promotion_manifest
                .get("promotion_batch_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("promotion manifest missing promotion_batch_id"))?
                .to_string();
            insert_promotion_run(
                conn,
                &run_id,
                &candidate,
                &actor,
                "failed",
                input.target_ref.as_deref(),
                None,
                None,
                None,
                None,
                Some(&promotion_batch_id),
                &errors,
                &now,
            )?;
            update_promotion_status(
                conn,
                &candidate.candidate_id,
                "promotion_failed",
                &run_id,
                &now,
            )?;
            return Ok(json!({
                "object": "tonglingyu.rule_candidate_promotion",
                "schema_version": RULE_CANDIDATE_PROMOTION_SCHEMA_VERSION,
                "run_id": run_id,
                "candidate_id": candidate.candidate_id,
                "candidate_ref": candidate.candidate_ref,
                "status": "failed",
                "candidate_status": "promotion_failed",
                "candidate_type": candidate.candidate_type,
                "primary_term": candidate.primary_term,
                "target_ref": input.target_ref,
                "promotion_batch_id": promotion_batch_id,
                "promotion_manifest": promotion_manifest,
                "errors": errors,
                "active_path_visible": false,
            }));
        }
    };
    let promotion_manifest = record_rule_promotion_manifest(
        conn,
        &candidate,
        &input,
        &actor,
        reason,
        &run_id,
        &expected_version_bump,
        rule_candidate_source_trace_refs(conn, &candidate.candidate_id)?,
        latest_rule_candidate_regression_evidence(conn, &candidate.candidate_id)?,
        Vec::new(),
        &[],
        Some(&patch),
    )?;
    let promotion_batch_id = promotion_manifest
        .get("promotion_batch_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("promotion manifest missing promotion_batch_id"))?
        .to_string();
    insert_promotion_run(
        conn,
        &run_id,
        &candidate,
        &actor,
        "passed",
        Some(&patch.target_ref),
        Some(&patch.catalog_name),
        Some(&patch.catalog_path),
        Some(&patch.before_sha256),
        Some(&patch.after_sha256),
        Some(&promotion_batch_id),
        &[],
        &now,
    )?;
    update_promotion_status(conn, &candidate.candidate_id, "promoted", &run_id, &now)?;
    Ok(json!({
        "object": "tonglingyu.rule_candidate_promotion",
        "schema_version": RULE_CANDIDATE_PROMOTION_SCHEMA_VERSION,
        "run_id": run_id,
        "candidate_id": candidate.candidate_id,
        "candidate_ref": candidate.candidate_ref,
        "status": "passed",
        "candidate_status": "promoted",
        "candidate_type": candidate.candidate_type,
        "primary_term": candidate.primary_term,
        "target_ref": patch.target_ref,
        "catalog_name": patch.catalog_name,
        "catalog_path": patch.catalog_path,
        "catalog_before_sha256": patch.before_sha256,
        "catalog_after_sha256": patch.after_sha256,
        "catalog_changed": patch.changed,
        "promotion_batch_id": promotion_batch_id,
        "promotion_manifest": promotion_manifest,
        "active_path_visible": true,
        "activation": "external_catalog_mtime_hot_reload",
    }))
}

pub(crate) fn transition_rule_candidate(
    conn: &Connection,
    input: RuleCandidateTransitionInput<'_>,
) -> Result<Value> {
    let candidate = read_rule_candidate_for_transition(conn, input.candidate_id)?;
    if candidate.schema_version != RULE_CANDIDATE_SCHEMA_VERSION {
        return Err(anyhow!("rule candidate schema mismatch"));
    }
    let actor = non_empty_or(input.actor, "system").to_string();
    let action = input.action.trim();
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(anyhow!("rule candidate transition reason is required"));
    }

    let transition = match action {
        "reject" => reject_transition(&candidate)?,
        "reopen" => reopen_transition(&candidate)?,
        "refresh_conflict" => refresh_conflict_transition(&candidate)?,
        _ => {
            return Err(anyhow!(
                "unsupported rule candidate transition action: {action}"
            ));
        }
    };
    let run_id = format!(
        "rule-candidate-transition-{}",
        uuid::Uuid::now_v7().simple()
    );
    let now = now_rfc3339();
    update_transitioned_candidate(conn, &candidate, &transition, &now)?;
    conn.execute(
        "INSERT INTO rule_candidate_transition_runs (
            run_id, candidate_id, actor, action, from_status, to_status,
            reason_sha256, metadata_json, created_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            &run_id,
            &candidate.candidate_id,
            &actor,
            action,
            &candidate.status,
            &transition.status,
            hash_text(reason),
            serde_json::to_string(&transition.metadata)?,
            &now,
            RULE_CANDIDATE_TRANSITION_SCHEMA_VERSION,
        ],
    )?;
    Ok(json!({
        "object": "tonglingyu.rule_candidate_transition",
        "schema_version": RULE_CANDIDATE_TRANSITION_SCHEMA_VERSION,
        "run_id": run_id,
        "candidate_id": candidate.candidate_id,
        "candidate_ref": candidate.candidate_ref,
        "action": action,
        "from_status": candidate.status,
        "candidate_status": transition.status,
        "conflict_status": transition.conflict_status,
        "active_rule_refs": transition.active_rule_refs,
        "review_state_reset": transition.review_state_reset,
        "active_path_visible": false,
    }))
}

pub(crate) fn record_rule_candidate_regression_evidence(
    conn: &Connection,
    input: RuleCandidateRegressionEvidenceInput<'_>,
) -> Result<Value> {
    let candidate = read_rule_candidate_for_transition(conn, input.candidate_id)?;
    if candidate.schema_version != RULE_CANDIDATE_SCHEMA_VERSION {
        return Err(anyhow!("rule candidate schema mismatch"));
    }
    let actor = non_empty_or(input.actor, "system").to_string();
    let suite_ref = non_empty_or(input.suite_ref, "");
    let report_ref = non_empty_or(input.report_ref, "");
    let report_sha256 = non_empty_or(input.report_sha256, "");
    validate_regression_evidence_fields(
        suite_ref,
        report_ref,
        report_sha256,
        input.case_count,
        input.passed_count,
        input.failed_count,
        input.skipped_count,
    )?;

    let status = if input.failed_count == 0
        && input.skipped_count == 0
        && input.passed_count == input.case_count
    {
        "passed"
    } else {
        "failed"
    };
    let evidence_id = format!(
        "rule-candidate-regression-{}",
        uuid::Uuid::now_v7().simple()
    );
    let now = now_rfc3339();
    let notes_sha256 = input
        .notes
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
        .map(hash_text);
    conn.execute(
        "INSERT INTO rule_candidate_regression_evidence (
            evidence_id, candidate_id, actor, status, suite_ref, report_ref,
            report_sha256, case_count, passed_count, failed_count, skipped_count,
            notes_sha256, created_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            &evidence_id,
            &candidate.candidate_id,
            &actor,
            status,
            suite_ref,
            report_ref,
            report_sha256,
            input.case_count,
            input.passed_count,
            input.failed_count,
            input.skipped_count,
            notes_sha256,
            &now,
            RULE_CANDIDATE_REGRESSION_EVIDENCE_SCHEMA_VERSION,
        ],
    )?;
    Ok(json!({
        "object": "tonglingyu.rule_candidate_regression_evidence",
        "schema_version": RULE_CANDIDATE_REGRESSION_EVIDENCE_SCHEMA_VERSION,
        "evidence_id": evidence_id,
        "candidate_id": candidate.candidate_id,
        "candidate_ref": candidate.candidate_ref,
        "status": status,
        "suite_ref": suite_ref,
        "report_ref": report_ref,
        "report_sha256": report_sha256,
        "case_count": input.case_count,
        "passed_count": input.passed_count,
        "failed_count": input.failed_count,
        "skipped_count": input.skipped_count,
        "created_at": now,
        "active_path_visible": false,
    }))
}

pub(crate) fn run_rule_candidate_review_run(
    conn: &Connection,
    input: RuleCandidateReviewRunInput<'_>,
) -> Result<Value> {
    let candidate = read_rule_candidate_for_transition(conn, input.candidate_id)?;
    validate_regression_evidence_fields(
        non_empty_or(input.suite_ref, ""),
        non_empty_or(input.report_ref, ""),
        non_empty_or(input.report_sha256, ""),
        input.case_count,
        input.passed_count,
        input.failed_count,
        input.skipped_count,
    )?;
    let run_id = format!("rule-candidate-review-{}", uuid::Uuid::now_v7().simple());
    let actor = non_empty_or(input.actor, "system").to_string();
    let started_at = now_rfc3339();
    let mut steps = Vec::<Value>::new();
    let mut errors = Vec::<String>::new();
    let mut transition_run_id = None::<String>;
    let mut regression_evidence_id = None::<String>;
    let mut preflight_run_id = None::<String>;

    let boundary_action = review_boundary_action(&candidate.status)?;
    let boundary_reason = non_empty_or(
        input.reopen_reason,
        "rule candidate regression evidence review run",
    );
    match transition_rule_candidate(
        conn,
        RuleCandidateTransitionInput {
            candidate_id: &candidate.candidate_id,
            actor: &actor,
            action: boundary_action,
            reason: boundary_reason,
        },
    ) {
        Ok(result) => {
            transition_run_id = result
                .get("run_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            steps.push(json!({
                "name": "review_boundary",
                "action": boundary_action,
                "status": "passed",
                "result": result,
            }));
        }
        Err(error) => {
            errors.push(format!(
                "review_boundary_failed: {}",
                safe_error_message(&error)
            ));
            steps.push(json!({
                "name": "review_boundary",
                "action": boundary_action,
                "status": "failed",
                "error": safe_error_message(&error),
            }));
        }
    }

    if errors.is_empty() {
        match record_rule_candidate_regression_evidence(
            conn,
            RuleCandidateRegressionEvidenceInput {
                candidate_id: &candidate.candidate_id,
                actor: &actor,
                suite_ref: input.suite_ref,
                report_ref: input.report_ref,
                report_sha256: input.report_sha256,
                case_count: input.case_count,
                passed_count: input.passed_count,
                failed_count: input.failed_count,
                skipped_count: input.skipped_count,
                notes: input.notes,
            },
        ) {
            Ok(result) => {
                regression_evidence_id = result
                    .get("evidence_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                steps.push(json!({
                    "name": "regression_evidence",
                    "status": "passed",
                    "result": result,
                }));
            }
            Err(error) => {
                errors.push(format!(
                    "regression_evidence_failed: {}",
                    safe_error_message(&error)
                ));
                steps.push(json!({
                    "name": "regression_evidence",
                    "status": "failed",
                    "error": safe_error_message(&error),
                }));
            }
        }
    }

    if errors.is_empty() {
        match run_rule_candidate_preflight(conn, &candidate.candidate_id, &actor) {
            Ok(result) => {
                preflight_run_id = result
                    .get("run_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if result.get("status").and_then(Value::as_str) != Some("passed") {
                    errors.push("preflight_failed".to_string());
                }
                steps.push(json!({
                    "name": "preflight",
                    "status": result.get("status").cloned().unwrap_or(Value::Null),
                    "result": result,
                }));
            }
            Err(error) => {
                errors.push(format!("preflight_failed: {}", safe_error_message(&error)));
                steps.push(json!({
                    "name": "preflight",
                    "status": "failed",
                    "error": safe_error_message(&error),
                }));
            }
        }
    }

    let status = if errors.is_empty() {
        "passed"
    } else {
        "failed"
    };
    let completed_at = now_rfc3339();
    insert_review_run(
        conn,
        ReviewRunInsertInput {
            run_id: &run_id,
            candidate_id: &candidate.candidate_id,
            actor: &actor,
            status,
            transition_run_id: transition_run_id.as_deref(),
            regression_evidence_id: regression_evidence_id.as_deref(),
            preflight_run_id: preflight_run_id.as_deref(),
            steps: &steps,
            errors: &errors,
            started_at: &started_at,
            completed_at: &completed_at,
        },
    )?;
    Ok(json!({
        "object": "tonglingyu.rule_candidate_review_run",
        "schema_version": RULE_CANDIDATE_REVIEW_RUN_SCHEMA_VERSION,
        "run_id": run_id,
        "candidate_id": candidate.candidate_id,
        "candidate_ref": candidate.candidate_ref,
        "status": status,
        "transition_run_id": transition_run_id,
        "regression_evidence_id": regression_evidence_id,
        "preflight_run_id": preflight_run_id,
        "steps": steps,
        "errors": errors,
        "active_path_visible": false,
    }))
}

pub(crate) fn load_rule_candidate_observations_for_trace(
    conn: &Connection,
    trace_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT
            rule_candidate_observations.observation_id,
            rule_candidates.candidate_id,
            rule_candidates.candidate_ref,
            rule_candidates.candidate_hash,
            rule_candidates.candidate_type,
            rule_candidates.term_key,
            rule_candidates.primary_term,
            rule_candidates.status,
            rule_candidates.conflict_status,
            rule_candidates.active_rule_refs_json,
            rule_candidates.observation_count,
            rule_candidate_observations.interaction_context_id,
            rule_candidate_observations.context_pack_id,
            rule_candidate_observations.external_message_id,
            rule_candidate_observations.source_question,
            rule_candidate_observations.resolved_question,
            rule_candidate_observations.reason,
            rule_candidate_observations.question_frame_json,
            rule_candidate_observations.validator_audit_json,
            rule_candidate_observations.observed_at,
            rule_candidate_observations.schema_version
         FROM rule_candidate_observations
         JOIN rule_candidates
           ON rule_candidates.candidate_id = rule_candidate_observations.candidate_id
         WHERE rule_candidate_observations.trace_id = ?1
         ORDER BY rule_candidate_observations.observed_at,
                  rule_candidate_observations.observation_id",
    )?;
    let rows = stmt.query_map(params![trace_id], |row| {
        Ok(json!({
            "observation_id": row.get::<_, String>(0)?,
            "candidate_id": row.get::<_, String>(1)?,
            "candidate_ref": row.get::<_, String>(2)?,
            "candidate_hash": row.get::<_, String>(3)?,
            "candidate_type": row.get::<_, String>(4)?,
            "term_key": row.get::<_, String>(5)?,
            "primary_term": row.get::<_, String>(6)?,
            "status": row.get::<_, String>(7)?,
            "conflict_status": row.get::<_, String>(8)?,
            "active_rule_refs": parse_json_column(row.get::<_, String>(9)?),
            "observation_count": row.get::<_, i64>(10)?,
            "interaction_context_id": row.get::<_, String>(11)?,
            "context_pack_id": row.get::<_, String>(12)?,
            "external_message_id": row.get::<_, String>(13)?,
            "source_question": row.get::<_, String>(14)?,
            "resolved_question": row.get::<_, String>(15)?,
            "reason": row.get::<_, String>(16)?,
            "question_frame": parse_json_column(row.get::<_, String>(17)?),
            "validator_audit": validator_audit_summary(parse_json_column(row.get::<_, String>(18)?)),
            "observed_at": row.get::<_, String>(19)?,
            "schema_version": row.get::<_, String>(20)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(crate) fn list_rule_candidates(
    conn: &Connection,
    input: RuleCandidateListInput<'_>,
) -> Result<Value> {
    validate_optional_filter(
        input.status,
        "rule candidate status",
        allowed_rule_candidate_statuses(),
    )?;
    validate_optional_filter(
        input.candidate_type,
        "rule candidate type",
        RULE_CANDIDATE_TYPES,
    )?;
    let limit = clamp_list_limit(input.limit, 100) as i64;
    let offset = input.offset.min(10_000) as i64;
    let mut stmt = conn.prepare(
        "SELECT candidate_id, candidate_ref, candidate_hash, candidate_type, term_key,
                primary_term, status, conflict_status, active_rule_refs_json,
                observation_count, created_by, first_observed_at, last_observed_at,
                preflight_status, last_preflight_run_id, promotion_status,
                last_promotion_run_id, created_at, updated_at, schema_version
         FROM rule_candidates
         WHERE (?1 IS NULL OR status = ?1)
           AND (?2 IS NULL OR candidate_type = ?2)
         ORDER BY updated_at DESC, candidate_id DESC
         LIMIT ?3 OFFSET ?4",
    )?;
    let rows = stmt.query_map(
        params![input.status, input.candidate_type, limit, offset],
        rule_candidate_row_json,
    )?;
    let candidates = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(json!({
        "object": "tonglingyu.rule_candidate_list",
        "schema_version": RULE_CANDIDATE_SCHEMA_VERSION,
        "items": candidates,
        "limit": limit,
        "offset": offset,
        "review_path_enabled": true,
        "active_path_visible": false,
    }))
}

pub(crate) fn read_rule_candidate_review_packet(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<Value>> {
    let candidate = conn
        .query_row(
            "SELECT candidate_id, candidate_ref, candidate_hash, candidate_type, term_key,
                    primary_term, status, conflict_status, active_rule_refs_json,
                    observation_count, created_by, first_observed_at, last_observed_at,
                    preflight_status, last_preflight_run_id, promotion_status,
                    last_promotion_run_id, created_at, updated_at, schema_version
             FROM rule_candidates WHERE candidate_id = ?1",
            params![candidate_id],
            rule_candidate_row_json,
        )
        .optional()?;
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let candidate_type = candidate
        .get("candidate_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(Some(json!({
        "object": "tonglingyu.rule_candidate_review_packet",
        "schema_version": RULE_CANDIDATE_SCHEMA_VERSION,
        "candidate": candidate,
        "promotion_target": rule_candidate_promotion_target_requirement(&candidate_type),
        "observations": read_rule_candidate_observations(conn, candidate_id)?,
        "regression_evidence": read_rule_candidate_regression_evidence(conn, candidate_id)?,
        "preflight_runs": read_rule_candidate_preflight_runs(conn, candidate_id)?,
        "promotion_runs": read_rule_candidate_promotion_runs(conn, candidate_id)?,
        "transition_runs": read_rule_candidate_transition_runs(conn, candidate_id)?,
        "review_runs": read_rule_candidate_review_runs(conn, candidate_id)?,
        "review_path_enabled": true,
        "active_path_visible": candidate_status_is_promoted(conn, candidate_id)?,
    })))
}

#[derive(Debug)]
struct RuleCandidateStageResult {
    candidate_ref: String,
    status: String,
    observation_inserted: bool,
}

fn stage_single_rule_candidate(
    conn: &Connection,
    input: &RuleCandidateObservationInput<'_>,
    candidate: &RuleCandidateSuggestion,
    validator_audit: &Value,
    created_by: &str,
) -> Result<RuleCandidateStageResult> {
    let term_key = context_rules::rule_candidate_term_key(&candidate.term);
    let candidate_hash = hash_text(&format!("{}:{term_key}", candidate.candidate_type));
    let candidate_id = format!("rule-candidate-{}", &candidate_hash[..16]);
    let candidate_ref = format!("rule-candidate://tonglingyu/{candidate_id}");
    let active_matches =
        active_rule_candidate_matches(&candidate.candidate_type, &candidate.term, None)?;
    let conflict_state =
        conflict_state_from_active_matches(&candidate.candidate_type, active_matches);
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO rule_candidates (
            candidate_id, candidate_ref, candidate_hash, candidate_type, term_key,
            primary_term, status, conflict_status, active_rule_refs_json, observation_count,
            created_by, first_observed_at, last_observed_at, created_at, updated_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(candidate_hash) DO UPDATE SET
            primary_term = excluded.primary_term,
            status = excluded.status,
            conflict_status = excluded.conflict_status,
            active_rule_refs_json = excluded.active_rule_refs_json,
            last_observed_at = excluded.last_observed_at,
            updated_at = excluded.updated_at",
        params![
            &candidate_id,
            &candidate_ref,
            &candidate_hash,
            &candidate.candidate_type,
            &term_key,
            candidate.term.trim(),
            &conflict_state.status,
            &conflict_state.conflict_status,
            serde_json::to_string(&conflict_state.active_rule_refs)?,
            created_by,
            &now,
            &now,
            &now,
            &now,
            RULE_CANDIDATE_SCHEMA_VERSION,
        ],
    )?;
    let observation_hash = hash_text(&format!(
        "{candidate_hash}:{}:{}",
        input.trace_id, input.external_message_id
    ));
    let observation_id = format!("rule-candidate-observation-{}", &observation_hash[..16]);
    let observation_inserted = conn.execute(
        "INSERT OR IGNORE INTO rule_candidate_observations (
            observation_id, candidate_id, trace_id, user_session_id, interaction_context_id,
            context_pack_id, external_message_id, source_question, resolved_question, reason,
            question_frame_json, validator_audit_json, observed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            &observation_id,
            &candidate_id,
            input.trace_id,
            input.user_session_id,
            input.interaction_context_id,
            input.context_pack_id,
            input.external_message_id,
            input.source_question,
            input.resolved_question,
            candidate.reason.trim(),
            serde_json::to_string(&input.question_frame.audit_json())?,
            serde_json::to_string(validator_audit)?,
            &now,
            RULE_CANDIDATE_SCHEMA_VERSION,
        ],
    )? == 1;
    if observation_inserted {
        conn.execute(
            "UPDATE rule_candidates
             SET observation_count = observation_count + 1,
                 last_observed_at = ?1,
                 updated_at = ?1
             WHERE candidate_id = ?2",
            params![&now, &candidate_id],
        )?;
    }
    Ok(RuleCandidateStageResult {
        candidate_ref,
        status: conflict_state.status,
        observation_inserted,
    })
}

#[derive(Debug)]
struct RuleCandidateForPreflight {
    candidate_id: String,
    candidate_ref: String,
    candidate_type: String,
    term_key: String,
    primary_term: String,
    status: String,
    conflict_status: String,
    observation_count: i64,
    last_observed_at: String,
    schema_version: String,
}

#[derive(Debug)]
struct RuleCandidateForPromotion {
    candidate_id: String,
    candidate_ref: String,
    candidate_type: String,
    primary_term: String,
    status: String,
    conflict_status: String,
    preflight_status: Option<String>,
    schema_version: String,
}

#[derive(Debug)]
struct RuleCandidateForTransition {
    candidate_id: String,
    candidate_ref: String,
    candidate_type: String,
    primary_term: String,
    status: String,
    conflict_status: String,
    active_rule_refs_json: String,
    schema_version: String,
}

#[derive(Debug)]
struct RuleCandidateTransitionPlan {
    status: String,
    conflict_status: String,
    active_rule_refs: Vec<Value>,
    review_state_reset: bool,
    metadata: Value,
}

#[derive(Debug)]
struct RuleCandidateConflictState {
    status: String,
    conflict_status: String,
    active_rule_refs: Vec<Value>,
}

#[derive(Debug)]
struct ReviewRunInsertInput<'a> {
    run_id: &'a str,
    candidate_id: &'a str,
    actor: &'a str,
    status: &'a str,
    transition_run_id: Option<&'a str>,
    regression_evidence_id: Option<&'a str>,
    preflight_run_id: Option<&'a str>,
    steps: &'a [Value],
    errors: &'a [String],
    started_at: &'a str,
    completed_at: &'a str,
}

#[derive(Debug)]
struct PromotionPatchResult {
    catalog_name: String,
    catalog_path: String,
    target_ref: String,
    before_sha256: String,
    after_sha256: String,
    changed: bool,
}

fn read_rule_candidate_for_preflight(
    conn: &Connection,
    candidate_id: &str,
) -> Result<RuleCandidateForPreflight> {
    conn.query_row(
        "SELECT candidate_id, candidate_ref, candidate_type, term_key, primary_term,
                status, conflict_status, observation_count, last_observed_at, schema_version
         FROM rule_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        |row| {
            Ok(RuleCandidateForPreflight {
                candidate_id: row.get(0)?,
                candidate_ref: row.get(1)?,
                candidate_type: row.get(2)?,
                term_key: row.get(3)?,
                primary_term: row.get(4)?,
                status: row.get(5)?,
                conflict_status: row.get(6)?,
                observation_count: row.get(7)?,
                last_observed_at: row.get(8)?,
                schema_version: row.get(9)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow::anyhow!("rule candidate not found: {candidate_id}"))
}

fn read_rule_candidate_for_promotion(
    conn: &Connection,
    candidate_id: &str,
) -> Result<RuleCandidateForPromotion> {
    conn.query_row(
        "SELECT candidate_id, candidate_ref, candidate_type, primary_term,
                status, conflict_status, preflight_status, schema_version
         FROM rule_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        |row| {
            Ok(RuleCandidateForPromotion {
                candidate_id: row.get(0)?,
                candidate_ref: row.get(1)?,
                candidate_type: row.get(2)?,
                primary_term: row.get(3)?,
                status: row.get(4)?,
                conflict_status: row.get(5)?,
                preflight_status: row.get(6)?,
                schema_version: row.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("rule candidate not found: {candidate_id}"))
}

fn read_rule_candidate_for_transition(
    conn: &Connection,
    candidate_id: &str,
) -> Result<RuleCandidateForTransition> {
    conn.query_row(
        "SELECT candidate_id, candidate_ref, candidate_type, primary_term,
                status, conflict_status, active_rule_refs_json, schema_version
         FROM rule_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        |row| {
            Ok(RuleCandidateForTransition {
                candidate_id: row.get(0)?,
                candidate_ref: row.get(1)?,
                candidate_type: row.get(2)?,
                primary_term: row.get(3)?,
                status: row.get(4)?,
                conflict_status: row.get(5)?,
                active_rule_refs_json: row.get(6)?,
                schema_version: row.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("rule candidate not found: {candidate_id}"))
}

fn reject_transition(
    candidate: &RuleCandidateForTransition,
) -> Result<RuleCandidateTransitionPlan> {
    if !matches!(
        candidate.status.as_str(),
        "staged"
            | "blocked_active_conflict"
            | "blocked_active_duplicate"
            | "ready_for_review"
            | "preflight_failed"
            | "promotion_failed"
    ) {
        return Err(anyhow!(
            "rule candidate status cannot be rejected: {}",
            candidate.status
        ));
    }
    let active_rule_refs = json_array_column(&candidate.active_rule_refs_json);
    Ok(RuleCandidateTransitionPlan {
        status: "rejected".to_string(),
        conflict_status: candidate.conflict_status.clone(),
        active_rule_refs: active_rule_refs.clone(),
        review_state_reset: false,
        metadata: json!({
            "transition_kind": "manual_reject",
            "previous_conflict_status": candidate.conflict_status,
            "active_rule_refs": active_rule_refs,
            "catalog_mutated": false,
        }),
    })
}

fn reopen_transition(
    candidate: &RuleCandidateForTransition,
) -> Result<RuleCandidateTransitionPlan> {
    if !matches!(
        candidate.status.as_str(),
        "ready_for_review" | "rejected" | "preflight_failed" | "promotion_failed"
    ) {
        return Err(anyhow!(
            "rule candidate status cannot be reopened: {}",
            candidate.status
        ));
    }
    recomputed_conflict_transition(candidate, "manual_reopen", true)
}

fn refresh_conflict_transition(
    candidate: &RuleCandidateForTransition,
) -> Result<RuleCandidateTransitionPlan> {
    if !matches!(
        candidate.status.as_str(),
        "staged" | "blocked_active_conflict" | "blocked_active_duplicate"
    ) {
        return Err(anyhow!(
            "rule candidate status cannot refresh conflict: {}",
            candidate.status
        ));
    }
    recomputed_conflict_transition(candidate, "manual_refresh_conflict", false)
}

fn review_boundary_action(status: &str) -> Result<&'static str> {
    match status {
        "staged" | "blocked_active_conflict" | "blocked_active_duplicate" => Ok("refresh_conflict"),
        "ready_for_review" | "rejected" | "preflight_failed" | "promotion_failed" => Ok("reopen"),
        "promoted" => Err(anyhow!(
            "promoted rule candidate cannot run review workflow without revocation"
        )),
        _ => Err(anyhow!(
            "unsupported rule candidate review status: {status}"
        )),
    }
}

fn recomputed_conflict_transition(
    candidate: &RuleCandidateForTransition,
    transition_kind: &str,
    review_state_reset: bool,
) -> Result<RuleCandidateTransitionPlan> {
    let previous_active_rule_refs = json_array_column(&candidate.active_rule_refs_json);
    let active_matches =
        active_rule_candidate_matches(&candidate.candidate_type, &candidate.primary_term, None)?;
    let conflict_state =
        conflict_state_from_active_matches(&candidate.candidate_type, active_matches);
    Ok(RuleCandidateTransitionPlan {
        status: conflict_state.status,
        conflict_status: conflict_state.conflict_status,
        active_rule_refs: conflict_state.active_rule_refs.clone(),
        review_state_reset,
        metadata: json!({
            "transition_kind": transition_kind,
            "previous_conflict_status": candidate.conflict_status,
            "previous_active_rule_refs": previous_active_rule_refs,
            "active_rule_refs": conflict_state.active_rule_refs,
            "review_state_reset": review_state_reset,
            "catalog_mutated": false,
        }),
    })
}

fn update_transitioned_candidate(
    conn: &Connection,
    candidate: &RuleCandidateForTransition,
    transition: &RuleCandidateTransitionPlan,
    now: &str,
) -> Result<()> {
    let active_rule_refs_json = serde_json::to_string(&transition.active_rule_refs)?;
    if transition.review_state_reset {
        conn.execute(
            "UPDATE rule_candidates
             SET status = ?1,
                 conflict_status = ?2,
                 active_rule_refs_json = ?3,
                 preflight_status = NULL,
                 last_preflight_run_id = NULL,
                 promotion_status = NULL,
                 last_promotion_run_id = NULL,
                 updated_at = ?4
             WHERE candidate_id = ?5",
            params![
                &transition.status,
                &transition.conflict_status,
                active_rule_refs_json,
                now,
                &candidate.candidate_id
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE rule_candidates
             SET status = ?1,
                 conflict_status = ?2,
                 active_rule_refs_json = ?3,
                 updated_at = ?4
             WHERE candidate_id = ?5",
            params![
                &transition.status,
                &transition.conflict_status,
                active_rule_refs_json,
                now,
                &candidate.candidate_id
            ],
        )?;
    }
    Ok(())
}

fn apply_candidate_to_external_catalog(
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
    run_id: &str,
) -> Result<PromotionPatchResult> {
    let catalog_version = input
        .catalog_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("online.{run_id}"));
    if is_runtime_rule_candidate_type(&candidate.candidate_type) {
        return patch_runtime_rule_catalog(candidate, input, &catalog_version);
    }
    match candidate.candidate_type.as_str() {
        "entity_alias" => patch_subject_ontology(candidate, input, &catalog_version),
        "predicate_alias"
        | "open_object_followup_marker"
        | "open_object_followup_suffix"
        | "source_scope_phrase"
        | "evidence_followup_term"
        | "count_question_term" => patch_question_frame_rules(candidate, input, &catalog_version),
        "clarification_pattern" => {
            patch_ellipsis_resolution_rules(candidate, input, &catalog_version)
        }
        other => Err(anyhow!(
            "unsupported rule candidate type for promotion: {other}"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn record_rule_promotion_manifest(
    conn: &Connection,
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
    actor: &str,
    reason: &str,
    run_id: &str,
    expected_version_bump: &str,
    source_trace_refs: Vec<String>,
    latest_regression: Option<Value>,
    active_rule_refs: Vec<Value>,
    errors: &[String],
    patch: Option<&PromotionPatchResult>,
) -> Result<Value> {
    let target_ref = patch
        .map(|patch| patch.target_ref.clone())
        .or_else(|| input.target_ref.clone())
        .unwrap_or_else(|| format!("rule_candidate:{}", candidate.candidate_type));
    let conflict_status = if active_rule_refs.is_empty() && candidate.conflict_status == "none" {
        "passed"
    } else {
        "conflict"
    };
    record_online_learning_promotion_manifest(
        conn,
        PromotionManifestInput {
            artifact_kind: "rule".to_string(),
            candidate_ids: vec![candidate.candidate_id.clone()],
            source_trace_refs,
            source_span_refs: json!([]),
            rule_diff_refs: json!([{
                "candidate_type": &candidate.candidate_type,
                "primary_term": &candidate.primary_term,
                "target_ref": &target_ref,
                "catalog_name": patch.map(|patch| patch.catalog_name.as_str()),
                "catalog_path": patch.map(|patch| patch.catalog_path.as_str()),
                "before_sha256": patch.map(|patch| patch.before_sha256.as_str()),
                "after_sha256": patch.map(|patch| patch.after_sha256.as_str()),
                "changed": patch.map(|patch| patch.changed),
            }]),
            merge_conflict_decision: json!({
                "status": conflict_status,
                "decision": if conflict_status == "passed" { "promote" } else { "blocked" },
                "stored_conflict_status": &candidate.conflict_status,
                "active_rule_refs": active_rule_refs,
            }),
            target_ref,
            expected_version_bump: expected_version_bump.to_string(),
            regression_cases: latest_regression.unwrap_or_else(|| {
                json!({
                    "status": "failed",
                    "reason": "missing_regression_evidence",
                })
            }),
            reviewer_policy: json!({
                "actor": actor,
                "reason_sha256": hash_text(reason),
                "policy": "manual_admin_review_plus_regression_preflight",
                "preflight_status": &candidate.preflight_status,
            }),
            dry_run_result: json!({
                "status": if errors.is_empty() { "passed" } else { "failed" },
                "errors": errors,
                "run_id": run_id,
                "candidate_status": &candidate.status,
                "preflight_status": &candidate.preflight_status,
                "schema_version": &candidate.schema_version,
            }),
            rollback_ref: patch
                .map(|patch| {
                    json!({
                        "kind": "catalog_sha",
                        "catalog_name": &patch.catalog_name,
                        "catalog_path": &patch.catalog_path,
                        "before_sha256": &patch.before_sha256,
                    })
                })
                .unwrap_or_else(|| {
                    json!({
                        "kind": "not_applied",
                        "reason": "promotion_blocked_before_catalog_write",
                    })
                }),
        },
    )
}

fn patch_runtime_rule_catalog(
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
    catalog_version: &str,
) -> Result<PromotionPatchResult> {
    let patch = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: &candidate.candidate_type,
        primary_term: &candidate.primary_term,
        target_ref: input.target_ref.as_deref(),
        catalog_version,
        paths: &input.paths.runtime_paths,
    })?;
    Ok(PromotionPatchResult {
        catalog_name: patch.catalog_name,
        catalog_path: patch.catalog_path,
        target_ref: patch.target_ref,
        before_sha256: patch.before_sha256,
        after_sha256: patch.after_sha256,
        changed: patch.changed,
    })
}

fn patch_subject_ontology(
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
    catalog_version: &str,
) -> Result<PromotionPatchResult> {
    let path = input.paths.subject_ontology_path.as_ref().ok_or_else(|| {
        anyhow!("{SUBJECT_ONTOLOGY_PATH_ENV} is required for entity_alias promotion")
    })?;
    let target_ref = required_target_ref(input, "subject:")?;
    let canonical = target_ref.trim_start_matches("subject:");
    patch_catalog_file(
        "subject_ontology",
        path,
        &target_ref,
        catalog_version,
        |catalog| {
            let subjects = catalog
                .get_mut("subjects")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| anyhow!("subject ontology subjects must be an array"))?;
            let subject = subjects
                .iter_mut()
                .find(|subject| subject.get("canonical").and_then(Value::as_str) == Some(canonical))
                .ok_or_else(|| anyhow!("subject ontology target not found: {target_ref}"))?;
            push_unique_string(subject, &["aliases"], &candidate.primary_term)
        },
        context_rules::validate_subject_ontology_source,
    )
}

fn patch_question_frame_rules(
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
    catalog_version: &str,
) -> Result<PromotionPatchResult> {
    let path = input
        .paths
        .question_frame_rules_path
        .as_ref()
        .ok_or_else(|| {
            anyhow!("{QUESTION_FRAME_RULES_PATH_ENV} is required for question-frame rule promotion")
        })?;
    let target_ref = promotion_target_ref(candidate, input)?;
    patch_catalog_file(
        "question_frame_rules",
        path,
        &target_ref,
        catalog_version,
        |catalog| match candidate.candidate_type.as_str() {
            "predicate_alias" => {
                let predicate_id = target_ref.trim_start_matches("predicate:");
                let predicates = catalog
                    .get_mut("predicates")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| anyhow!("question frame predicates must be an array"))?;
                let predicate = predicates
                    .iter_mut()
                    .find(|predicate| {
                        predicate.get("id").and_then(Value::as_str) == Some(predicate_id)
                    })
                    .ok_or_else(|| anyhow!("predicate target not found: {target_ref}"))?;
                push_unique_string(predicate, &["aliases"], &candidate.primary_term)
            }
            "open_object_followup_marker" => push_unique_string(
                catalog,
                &["relation_question", "open_object_followup_marker_terms"],
                &candidate.primary_term,
            ),
            "open_object_followup_suffix" => push_unique_string(
                catalog,
                &["relation_question", "open_object_followup_suffix_terms"],
                &candidate.primary_term,
            ),
            "source_scope_phrase" => {
                let scope = target_ref.trim_start_matches("source_scope:");
                let scopes = catalog
                    .get_mut("source_scope_phrases")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| anyhow!("source_scope_phrases must be an array"))?;
                let scope_rule = scopes
                    .iter_mut()
                    .find(|rule| rule.get("scope").and_then(Value::as_str) == Some(scope))
                    .ok_or_else(|| anyhow!("source scope target not found: {target_ref}"))?;
                push_unique_string(scope_rule, &["phrases"], &candidate.primary_term)
            }
            "evidence_followup_term" => push_unique_string(
                catalog,
                &["evidence_followup", "terms"],
                &candidate.primary_term,
            ),
            "count_question_term" => push_unique_string(
                catalog,
                &["count_question", "terms"],
                &candidate.primary_term,
            ),
            other => Err(anyhow!(
                "unsupported question-frame promotion type: {other}"
            )),
        },
        context_rules::validate_question_frame_rules_source,
    )
}

fn patch_ellipsis_resolution_rules(
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
    catalog_version: &str,
) -> Result<PromotionPatchResult> {
    let path = input
        .paths
        .ellipsis_resolution_rules_path
        .as_ref()
        .ok_or_else(|| anyhow!("{ELLIPSIS_RESOLUTION_RULES_PATH_ENV} is required for clarification_pattern promotion"))?;
    let target_ref = input
        .target_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ellipsis:followup_questions")
        .to_string();
    patch_catalog_file(
        "ellipsis_resolution_rules",
        path,
        &target_ref,
        catalog_version,
        |catalog| match target_ref.as_str() {
            "ellipsis:followup_questions" => {
                push_unique_string(catalog, &["followup_questions"], &candidate.primary_term)
            }
            "ellipsis:followup_suffix_terms" => {
                push_unique_string(catalog, &["followup_suffix_terms"], &candidate.primary_term)
            }
            _ => Err(anyhow!("unsupported ellipsis target_ref: {target_ref}")),
        },
        context_rules::validate_ellipsis_resolution_rules_source,
    )
}

fn patch_catalog_file(
    catalog_name: &str,
    path: &PathBuf,
    target_ref: &str,
    catalog_version: &str,
    patch: impl FnOnce(&mut Value) -> Result<bool>,
    validate: fn(&str) -> Result<()>,
) -> Result<PromotionPatchResult> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("catalog file is not readable: {}", path.display()))?;
    let before_sha256 = hash_text(&source);
    let mut catalog: Value = serde_json::from_str(&source)
        .with_context(|| format!("catalog file is not JSON: {}", path.display()))?;
    let changed = patch(&mut catalog)?;
    if changed {
        catalog["catalog_version"] = json!(catalog_version);
    }
    let updated = serde_json::to_string_pretty(&catalog)? + "\n";
    validate(&updated)?;
    let after_sha256 = hash_text(&updated);
    if changed && before_sha256 != after_sha256 {
        write_catalog_atomically(path, &updated)?;
    }
    Ok(PromotionPatchResult {
        catalog_name: catalog_name.to_string(),
        catalog_path: path.display().to_string(),
        target_ref: target_ref.to_string(),
        before_sha256,
        after_sha256,
        changed,
    })
}

fn promotion_target_ref(
    candidate: &RuleCandidateForPromotion,
    input: &RuleCandidatePromotionInput,
) -> Result<String> {
    match candidate.candidate_type.as_str() {
        "predicate_alias" => required_target_ref(input, "predicate:"),
        "source_scope_phrase" => required_target_ref(input, "source_scope:"),
        "open_object_followup_marker" => {
            Ok("relation_question.open_object_followup_marker_terms".to_string())
        }
        "open_object_followup_suffix" => {
            Ok("relation_question.open_object_followup_suffix_terms".to_string())
        }
        "evidence_followup_term" => Ok("evidence_followup.terms".to_string()),
        "count_question_term" => Ok("count_question.terms".to_string()),
        other => Err(anyhow!(
            "unsupported promotion target for candidate type: {other}"
        )),
    }
}

fn required_target_ref(input: &RuleCandidatePromotionInput, prefix: &str) -> Result<String> {
    let value = input
        .target_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("target_ref with prefix {prefix} is required"))?;
    if !value.starts_with(prefix) || value.len() == prefix.len() {
        return Err(anyhow!("target_ref must start with {prefix}"));
    }
    Ok(value.to_string())
}

fn push_unique_string(root: &mut Value, path: &[&str], term: &str) -> Result<bool> {
    let term = term.trim();
    if term.is_empty() {
        return Err(anyhow!("promoted term must not be empty"));
    }
    let mut cursor = root;
    for key in &path[..path.len().saturating_sub(1)] {
        cursor = cursor
            .get_mut(*key)
            .ok_or_else(|| anyhow!("catalog path not found: {}", path.join(".")))?;
    }
    let leaf = path
        .last()
        .ok_or_else(|| anyhow!("catalog path must not be empty"))?;
    let array = cursor
        .get_mut(*leaf)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("catalog path must be an array: {}", path.join(".")))?;
    if array.iter().any(|value| value.as_str() == Some(term)) {
        return Ok(false);
    }
    array.push(json!(term));
    Ok(true)
}

fn write_catalog_atomically(path: &PathBuf, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("catalog path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("catalog path has invalid file name: {}", path.display()))?;
    let tmp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::now_v7().simple()
    ));
    fs::write(&tmp_path, content)
        .with_context(|| format!("write temp catalog failed: {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("replace catalog failed: {}", path.display()))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_promotion_run(
    conn: &Connection,
    run_id: &str,
    candidate: &RuleCandidateForPromotion,
    actor: &str,
    status: &str,
    target_ref: Option<&str>,
    catalog_name: Option<&str>,
    catalog_path: Option<&str>,
    before_sha256: Option<&str>,
    after_sha256: Option<&str>,
    promotion_batch_id: Option<&str>,
    errors: &[String],
    now: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO rule_candidate_promotion_runs (
            run_id, candidate_id, actor, status, target_ref, catalog_name,
            catalog_path, before_sha256, after_sha256, promotion_batch_id, errors_json,
            started_at, completed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            run_id,
            &candidate.candidate_id,
            actor,
            status,
            target_ref,
            catalog_name,
            catalog_path,
            before_sha256,
            after_sha256,
            promotion_batch_id,
            serde_json::to_string(errors)?,
            now,
            now,
            RULE_CANDIDATE_PROMOTION_SCHEMA_VERSION,
        ],
    )?;
    Ok(())
}

fn update_promotion_status(
    conn: &Connection,
    candidate_id: &str,
    status: &str,
    run_id: &str,
    now: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE rule_candidates
         SET status = ?1,
             promotion_status = ?1,
             last_promotion_run_id = ?2,
             updated_at = ?3
         WHERE candidate_id = ?4",
        params![status, run_id, now, candidate_id],
    )?;
    Ok(())
}

fn configured_path(env_name: &str) -> Option<PathBuf> {
    std::env::var(env_name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn rule_candidate_observation_count(conn: &Connection, candidate_id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM rule_candidate_observations WHERE candidate_id = ?1",
        params![candidate_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn rule_candidate_source_trace_refs(conn: &Connection, candidate_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT trace_id
         FROM rule_candidate_observations
         WHERE candidate_id = ?1
         ORDER BY trace_id",
    )?;
    let rows = stmt.query_map(params![candidate_id], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn insert_review_run(conn: &Connection, input: ReviewRunInsertInput<'_>) -> Result<()> {
    conn.execute(
        "INSERT INTO rule_candidate_review_runs (
            run_id, candidate_id, actor, status, transition_run_id,
            regression_evidence_id, preflight_run_id, steps_json, errors_json,
            started_at, completed_at, schema_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            input.run_id,
            input.candidate_id,
            input.actor,
            input.status,
            input.transition_run_id,
            input.regression_evidence_id,
            input.preflight_run_id,
            serde_json::to_string(input.steps)?,
            serde_json::to_string(input.errors)?,
            input.started_at,
            input.completed_at,
            RULE_CANDIDATE_REVIEW_RUN_SCHEMA_VERSION,
        ],
    )?;
    Ok(())
}

fn regression_gate_min_created_at(
    conn: &Connection,
    candidate: &RuleCandidateForPreflight,
) -> Result<String> {
    let latest_transition_created_at = conn
        .query_row(
            "SELECT created_at
             FROM rule_candidate_transition_runs
             WHERE candidate_id = ?1
             ORDER BY created_at DESC, run_id DESC
             LIMIT 1",
            params![&candidate.candidate_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(latest_transition_created_at
        .filter(|created_at| created_at > &candidate.last_observed_at)
        .unwrap_or_else(|| candidate.last_observed_at.clone()))
}

fn latest_rule_candidate_regression_evidence(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<Value>> {
    conn.query_row(
        "SELECT evidence_id, candidate_id, actor, status, suite_ref, report_ref,
                report_sha256, case_count, passed_count, failed_count, skipped_count,
                notes_sha256, created_at, schema_version
         FROM rule_candidate_regression_evidence
         WHERE candidate_id = ?1
         ORDER BY created_at DESC, evidence_id DESC
         LIMIT 1",
        params![candidate_id],
        rule_candidate_regression_evidence_row_json,
    )
    .optional()
    .map_err(Into::into)
}

fn read_rule_candidate_observations(conn: &Connection, candidate_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT observation_id, candidate_id, trace_id, user_session_id,
                interaction_context_id, context_pack_id, external_message_id,
                source_question, resolved_question, reason, question_frame_json,
                validator_audit_json, observed_at, schema_version
         FROM rule_candidate_observations
         WHERE candidate_id = ?1
         ORDER BY observed_at DESC, observation_id DESC
         LIMIT 100",
    )?;
    let rows = stmt.query_map(params![candidate_id], |row| {
        Ok(json!({
            "observation_id": row.get::<_, String>(0)?,
            "candidate_id": row.get::<_, String>(1)?,
            "trace_id": row.get::<_, String>(2)?,
            "user_session_id": row.get::<_, String>(3)?,
            "interaction_context_id": row.get::<_, String>(4)?,
            "context_pack_id": row.get::<_, String>(5)?,
            "external_message_id": row.get::<_, String>(6)?,
            "source_question": row.get::<_, String>(7)?,
            "resolved_question": row.get::<_, String>(8)?,
            "reason": row.get::<_, String>(9)?,
            "question_frame": parse_json_column(row.get::<_, String>(10)?),
            "validator_audit": validator_audit_summary(parse_json_column(row.get::<_, String>(11)?)),
            "observed_at": row.get::<_, String>(12)?,
            "schema_version": row.get::<_, String>(13)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn read_rule_candidate_regression_evidence(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT evidence_id, candidate_id, actor, status, suite_ref, report_ref,
                report_sha256, case_count, passed_count, failed_count, skipped_count,
                notes_sha256, created_at, schema_version
         FROM rule_candidate_regression_evidence
         WHERE candidate_id = ?1
         ORDER BY created_at DESC, evidence_id DESC
         LIMIT 50",
    )?;
    let rows = stmt.query_map(
        params![candidate_id],
        rule_candidate_regression_evidence_row_json,
    )?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn read_rule_candidate_preflight_runs(conn: &Connection, candidate_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, candidate_id, actor, status, checks_json, errors_json,
                started_at, completed_at, schema_version
         FROM rule_candidate_preflight_runs
         WHERE candidate_id = ?1
         ORDER BY started_at DESC, run_id DESC
         LIMIT 50",
    )?;
    let rows = stmt.query_map(params![candidate_id], |row| {
        Ok(json!({
            "run_id": row.get::<_, String>(0)?,
            "candidate_id": row.get::<_, String>(1)?,
            "actor": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "checks": parse_json_column(row.get::<_, String>(4)?),
            "errors": parse_json_column(row.get::<_, String>(5)?),
            "started_at": row.get::<_, String>(6)?,
            "completed_at": row.get::<_, String>(7)?,
            "schema_version": row.get::<_, String>(8)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn read_rule_candidate_promotion_runs(conn: &Connection, candidate_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, candidate_id, actor, status, target_ref, catalog_name,
                catalog_path, before_sha256, after_sha256, promotion_batch_id, errors_json,
                started_at, completed_at, schema_version
         FROM rule_candidate_promotion_runs
         WHERE candidate_id = ?1
         ORDER BY started_at DESC, run_id DESC
         LIMIT 50",
    )?;
    let rows = stmt.query_map(params![candidate_id], |row| {
        Ok(json!({
            "run_id": row.get::<_, String>(0)?,
            "candidate_id": row.get::<_, String>(1)?,
            "actor": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "target_ref": row.get::<_, Option<String>>(4)?,
            "catalog_name": row.get::<_, Option<String>>(5)?,
            "catalog_path": row.get::<_, Option<String>>(6)?,
            "before_sha256": row.get::<_, Option<String>>(7)?,
            "after_sha256": row.get::<_, Option<String>>(8)?,
            "promotion_batch_id": row.get::<_, Option<String>>(9)?,
            "errors": parse_json_column(row.get::<_, String>(10)?),
            "started_at": row.get::<_, String>(11)?,
            "completed_at": row.get::<_, String>(12)?,
            "schema_version": row.get::<_, String>(13)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn read_rule_candidate_transition_runs(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, candidate_id, actor, action, from_status, to_status,
                reason_sha256, metadata_json, created_at, schema_version
         FROM rule_candidate_transition_runs
         WHERE candidate_id = ?1
         ORDER BY created_at DESC, run_id DESC
         LIMIT 50",
    )?;
    let rows = stmt.query_map(params![candidate_id], |row| {
        Ok(json!({
            "run_id": row.get::<_, String>(0)?,
            "candidate_id": row.get::<_, String>(1)?,
            "actor": row.get::<_, String>(2)?,
            "action": row.get::<_, String>(3)?,
            "from_status": row.get::<_, String>(4)?,
            "to_status": row.get::<_, String>(5)?,
            "reason_sha256": row.get::<_, String>(6)?,
            "metadata": parse_json_column(row.get::<_, String>(7)?),
            "created_at": row.get::<_, String>(8)?,
            "schema_version": row.get::<_, String>(9)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn read_rule_candidate_review_runs(conn: &Connection, candidate_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, candidate_id, actor, status, transition_run_id,
                regression_evidence_id, preflight_run_id, steps_json, errors_json,
                started_at, completed_at, schema_version
         FROM rule_candidate_review_runs
         WHERE candidate_id = ?1
         ORDER BY started_at DESC, run_id DESC
         LIMIT 50",
    )?;
    let rows = stmt.query_map(params![candidate_id], |row| {
        Ok(json!({
            "run_id": row.get::<_, String>(0)?,
            "candidate_id": row.get::<_, String>(1)?,
            "actor": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "transition_run_id": row.get::<_, Option<String>>(4)?,
            "regression_evidence_id": row.get::<_, Option<String>>(5)?,
            "preflight_run_id": row.get::<_, Option<String>>(6)?,
            "steps": parse_json_column(row.get::<_, String>(7)?),
            "errors": parse_json_column(row.get::<_, String>(8)?),
            "started_at": row.get::<_, String>(9)?,
            "completed_at": row.get::<_, String>(10)?,
            "schema_version": row.get::<_, String>(11)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn candidate_status_is_promoted(conn: &Connection, candidate_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT status = 'promoted' FROM rule_candidates WHERE candidate_id = ?1",
        params![candidate_id],
        |row| Ok(row.get::<_, i64>(0)? != 0),
    )
    .map_err(Into::into)
}

fn rule_candidate_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "candidate_id": row.get::<_, String>(0)?,
        "candidate_ref": row.get::<_, String>(1)?,
        "candidate_hash": row.get::<_, String>(2)?,
        "candidate_type": row.get::<_, String>(3)?,
        "term_key": row.get::<_, String>(4)?,
        "primary_term": row.get::<_, String>(5)?,
        "status": row.get::<_, String>(6)?,
        "conflict_status": row.get::<_, String>(7)?,
        "active_rule_refs": parse_json_column(row.get::<_, String>(8)?),
        "observation_count": row.get::<_, i64>(9)?,
        "created_by": row.get::<_, String>(10)?,
        "first_observed_at": row.get::<_, String>(11)?,
        "last_observed_at": row.get::<_, String>(12)?,
        "preflight_status": row.get::<_, Option<String>>(13)?,
        "last_preflight_run_id": row.get::<_, Option<String>>(14)?,
        "promotion_status": row.get::<_, Option<String>>(15)?,
        "last_promotion_run_id": row.get::<_, Option<String>>(16)?,
        "created_at": row.get::<_, String>(17)?,
        "updated_at": row.get::<_, String>(18)?,
        "schema_version": row.get::<_, String>(19)?,
    }))
}

fn rule_candidate_regression_evidence_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "evidence_id": row.get::<_, String>(0)?,
        "candidate_id": row.get::<_, String>(1)?,
        "actor": row.get::<_, String>(2)?,
        "status": row.get::<_, String>(3)?,
        "suite_ref": row.get::<_, String>(4)?,
        "report_ref": row.get::<_, String>(5)?,
        "report_sha256": row.get::<_, String>(6)?,
        "case_count": row.get::<_, i64>(7)?,
        "passed_count": row.get::<_, i64>(8)?,
        "failed_count": row.get::<_, i64>(9)?,
        "skipped_count": row.get::<_, i64>(10)?,
        "notes_sha256": row.get::<_, Option<String>>(11)?,
        "created_at": row.get::<_, String>(12)?,
        "schema_version": row.get::<_, String>(13)?,
    }))
}

fn validator_audit_summary(validator_audit: Value) -> Value {
    json!({
        "schema_version": validator_audit.get("schema_version").cloned().unwrap_or(Value::Null),
        "profile_id": validator_audit.get("profile_id").cloned().unwrap_or(Value::Null),
        "agent_request_id": validator_audit.get("agent_request_id").cloned().unwrap_or(Value::Null),
        "decision": validator_audit.get("decision").cloned().unwrap_or(Value::Null),
        "contract_accepted": validator_audit.get("contract_accepted").cloned().unwrap_or(Value::Null),
        "accepted_for_main": validator_audit.get("accepted_for_main").cloned().unwrap_or(Value::Null),
        "rule_candidate_count": validator_audit.get("rule_candidate_count").cloned().unwrap_or(Value::Null),
        "input_digest": validator_audit.get("input_digest").cloned().unwrap_or(Value::Null),
        "projection_ref": validator_audit.get("projection_ref").cloned().unwrap_or(Value::Null),
        "raw_output_sha256": validator_audit.get("raw_output_sha256").cloned().unwrap_or(Value::Null),
    })
}

fn active_rule_candidate_matches(
    candidate_type: &str,
    term: &str,
    paths: Option<&RuleCandidatePromotionPaths>,
) -> Result<Vec<context_rules::RuleCandidateActiveMatch>> {
    if is_runtime_rule_candidate_type(candidate_type) {
        let env_paths;
        let runtime_paths = match paths {
            Some(paths) => &paths.runtime_paths,
            None => {
                env_paths = RuntimeRuleCandidatePromotionPaths::from_env();
                &env_paths
            }
        };
        return Ok(
            active_runtime_rule_candidate_matches(runtime_paths, candidate_type, term)?
                .into_iter()
                .map(|active| context_rules::RuleCandidateActiveMatch {
                    candidate_type: active.candidate_type,
                    rule_ref: active.rule_ref,
                })
                .collect(),
        );
    }
    context_rules::active_rule_candidate_matches(candidate_type, term)
}

fn rule_candidate_promotion_target_requirement(candidate_type: &str) -> Value {
    if let Some(requirement) = runtime_rule_candidate_target_requirement(candidate_type) {
        return requirement;
    }
    match candidate_type {
        "entity_alias" => json!({
            "catalog_name": "subject_ontology",
            "target_ref_pattern": "subject:<canonical>",
            "target_field": "subjects[].aliases",
        }),
        "predicate_alias" => json!({
            "catalog_name": "question_frame_rules",
            "target_ref_pattern": "predicate:<predicate_id>",
            "target_field": "predicates[].aliases",
        }),
        "source_scope_phrase" => json!({
            "catalog_name": "question_frame_rules",
            "target_ref_pattern": "source_scope:<scope>",
            "target_field": "source_scope_phrases[].phrases",
        }),
        "open_object_followup_marker" => json!({
            "catalog_name": "question_frame_rules",
            "target_ref_pattern": "relation_question.open_object_followup_marker_terms",
            "target_field": "relation_question.open_object_followup_marker_terms",
        }),
        "open_object_followup_suffix" => json!({
            "catalog_name": "question_frame_rules",
            "target_ref_pattern": "relation_question.open_object_followup_suffix_terms",
            "target_field": "relation_question.open_object_followup_suffix_terms",
        }),
        "evidence_followup_term" => json!({
            "catalog_name": "question_frame_rules",
            "target_ref_pattern": "evidence_followup.terms",
            "target_field": "evidence_followup.terms",
        }),
        "count_question_term" => json!({
            "catalog_name": "question_frame_rules",
            "target_ref_pattern": "count_question.terms",
            "target_field": "count_question.terms",
        }),
        "clarification_pattern" => json!({
            "catalog_name": "ellipsis_resolution_rules",
            "target_ref_pattern": "ellipsis:followup_questions|ellipsis:followup_suffix_terms",
            "target_field": "followup_questions or followup_suffix_terms",
        }),
        _ => json!({
            "catalog_name": null,
            "target_ref_pattern": null,
            "unsupported": true,
        }),
    }
}

fn conflict_state_from_active_matches(
    candidate_type: &str,
    active_matches: Vec<context_rules::RuleCandidateActiveMatch>,
) -> RuleCandidateConflictState {
    let has_cross_type_conflict = active_matches
        .iter()
        .any(|active| active.candidate_type != candidate_type);
    let has_same_type_duplicate = active_matches
        .iter()
        .any(|active| active.candidate_type == candidate_type);
    let (status, conflict_status) = if has_cross_type_conflict {
        ("blocked_active_conflict", "active_conflict")
    } else if has_same_type_duplicate {
        ("blocked_active_duplicate", "active_duplicate")
    } else {
        ("staged", "none")
    };
    let active_rule_refs = active_matches
        .iter()
        .map(|active| {
            json!({
                "candidate_type": active.candidate_type,
                "rule_ref": active.rule_ref,
            })
        })
        .collect::<Vec<_>>();
    RuleCandidateConflictState {
        status: status.to_string(),
        conflict_status: conflict_status.to_string(),
        active_rule_refs,
    }
}

fn hardcode_check_passed(candidate: &RuleCandidateForPreflight) -> bool {
    hardcode_term_check_passed(&candidate.primary_term)
}

fn hardcode_term_check_passed(term: &str) -> bool {
    if term.contains('\n') || term.contains('\r') {
        return false;
    }
    let forbidden = [
        "答案",
        "结论",
        "回答",
        "应当",
        "应该",
        "必须",
        "evidence_id",
        "trace_id",
        "package_id",
    ];
    !forbidden.iter().any(|forbidden| term.contains(forbidden))
}

fn validate_regression_counts(
    case_count: i64,
    passed_count: i64,
    failed_count: i64,
    skipped_count: i64,
) -> Result<()> {
    if case_count <= 0 {
        return Err(anyhow!(
            "rule candidate regression case_count must be positive"
        ));
    }
    if passed_count < 0 || failed_count < 0 || skipped_count < 0 {
        return Err(anyhow!(
            "rule candidate regression counts must be non-negative"
        ));
    }
    if passed_count + failed_count + skipped_count != case_count {
        return Err(anyhow!(
            "rule candidate regression counts must add up to case_count"
        ));
    }
    Ok(())
}

fn validate_regression_evidence_fields(
    suite_ref: &str,
    report_ref: &str,
    report_sha256: &str,
    case_count: i64,
    passed_count: i64,
    failed_count: i64,
    skipped_count: i64,
) -> Result<()> {
    if suite_ref.is_empty() {
        return Err(anyhow!("rule candidate regression suite_ref is required"));
    }
    if report_ref.is_empty() {
        return Err(anyhow!("rule candidate regression report_ref is required"));
    }
    if report_sha256.len() < 16 || report_sha256.contains(|ch: char| !ch.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "rule candidate regression report_sha256 is invalid"
        ));
    }
    validate_regression_counts(case_count, passed_count, failed_count, skipped_count)
}

fn safe_error_message(error: &anyhow::Error) -> String {
    const MAX_ERROR_DETAIL_CHARS: usize = 240;
    let mut detail = error.to_string().replace(['\n', '\r'], " ");
    if detail.chars().count() > MAX_ERROR_DETAIL_CHARS {
        detail = detail
            .chars()
            .take(MAX_ERROR_DETAIL_CHARS)
            .collect::<String>();
    }
    detail
}

fn parse_json_column(value: String) -> Value {
    serde_json::from_str(&value).unwrap_or(Value::Null)
}

fn json_array_column(value: &str) -> Vec<Value> {
    serde_json::from_str::<Vec<Value>>(value).unwrap_or_default()
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() { fallback } else { value }
}

fn validate_optional_filter(value: Option<&str>, name: &str, allowed: &[&str]) -> Result<()> {
    if let Some(value) = value
        && !allowed.contains(&value)
    {
        return Err(anyhow!("{name} filter is not supported: {value}"));
    }
    Ok(())
}

fn allowed_rule_candidate_statuses() -> &'static [&'static str] {
    &[
        "staged",
        "blocked_active_conflict",
        "blocked_active_duplicate",
        "ready_for_review",
        "preflight_failed",
        "promoted",
        "promotion_failed",
        "rejected",
    ]
}

fn clamp_list_limit(limit: usize, max: usize) -> usize {
    limit.clamp(1, max)
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
