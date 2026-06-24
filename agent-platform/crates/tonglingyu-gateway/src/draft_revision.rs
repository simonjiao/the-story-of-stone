use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const PROFILE_OBSERVATION_SCHEMA_VERSION: &str = "tonglingyu-profile-observation-v1";
pub(crate) const DRAFT_CANDIDATE_SCHEMA_VERSION: &str = "tonglingyu-draft-candidate-v1";
pub(crate) const DRAFT_REVISION_SCHEMA_VERSION: &str = "tonglingyu-draft-revision-v1";
pub(crate) const LLM_REVIEW_OBSERVATION_SCHEMA_VERSION: &str = "tonglingyu-review-observation-v1";
pub(crate) const REVIEW_OVERRIDE_SCHEMA_VERSION: &str = "tonglingyu-review-override-v1";

const MIN_OBSERVATION_CONFIDENCE: f64 = 0.5;
const MIN_DRAFT_CLAIM_CONFIDENCE: f64 = 0.5;
const MAX_REVISION_COUNT: usize = 2;

const PROFILE_OBSERVATION_FORBIDDEN_FIELDS: &[&str] = &[
    "final_answer",
    "reviewer_decision",
    "reviewer_state",
    "tool_policy",
    "tool_choice",
    "allowed_tools",
    "forbidden_tools",
    "acl",
    "read_refs",
    "memory_card_id",
    "memory_read_refs",
    "raw_memory",
    "context_pack_id",
    "context_pack_ref",
    "trace_id",
    "system_prompt",
];

const DRAFT_FORBIDDEN_FIELDS: &[&str] = &[
    "final_answer",
    "reviewer_decision",
    "reviewer_state",
    "tool_policy",
    "tool_choice",
    "allowed_tools",
    "forbidden_tools",
    "acl",
    "read_refs",
    "memory_card_id",
    "memory_read_refs",
    "raw_memory",
    "context_pack_id",
    "context_pack_ref",
    "trace_id",
    "system_prompt",
    "profile",
    "evidence_package_write",
    "package_mutated",
];

const REVIEW_FORBIDDEN_FIELDS: &[&str] = &[
    "final_answer",
    "final_decision",
    "reviewer_decision",
    "local_reviewer_status",
    "tool_policy",
    "tool_choice",
    "allowed_tools",
    "forbidden_tools",
    "acl",
    "read_refs",
    "memory_card_id",
    "memory_read_refs",
    "raw_memory",
    "context_pack_id",
    "context_pack_ref",
    "trace_id",
    "system_prompt",
    "evidence_package_write",
    "package_mutated",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileObservation {
    schema_version: String,
    profile: String,
    observation: String,
    #[serde(default)]
    evidence_refs: Vec<String>,
    #[serde(default)]
    package_ref: Option<String>,
    confidence: f64,
    #[serde(default)]
    candidate_claims: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftCandidate {
    schema_version: String,
    #[serde(default)]
    draft_candidate_id: Option<String>,
    evidence_package_id: String,
    resolved_question: String,
    draft: String,
    claims: Vec<DraftClaim>,
    #[serde(default)]
    unsupported_claims: Vec<String>,
    #[serde(default)]
    style_notes_applied: Vec<String>,
    memory_used_as_evidence: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftClaim {
    claim_id: String,
    text: String,
    evidence_refs: Vec<String>,
    confidence: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LlmReviewObservation {
    schema_version: String,
    status: String,
    severity: String,
    #[serde(default)]
    issues: Vec<LlmReviewIssue>,
    #[serde(default)]
    required_revisions: Vec<String>,
    confidence: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LlmReviewIssue {
    issue_id: String,
    category: String,
    severity: String,
    #[serde(default)]
    claim_id: Option<String>,
    #[serde(default)]
    evidence_refs: Vec<String>,
    summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftRevisionRecord {
    schema_version: String,
    revision_id: String,
    revision_index: usize,
    evidence_package_id: String,
    previous_draft_candidate_id: String,
    previous_review_id: String,
    required_revision_reasons: Vec<String>,
    revised_draft_candidate_id: String,
    package_mutated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProfileObservationReport {
    pub(crate) accepted: bool,
    pub(crate) errors: Vec<String>,
    pub(crate) unknown_consumer: bool,
    pub(crate) digest_mismatch: bool,
    pub(crate) cross_profile_leakage: bool,
    pub(crate) external_ref_detected: bool,
    pub(crate) package_ref_allowed: bool,
    pub(crate) final_answer_attempt: bool,
    pub(crate) internal_field_detected: bool,
    pub(crate) evidence_ref_count: usize,
    pub(crate) candidate_claim_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DraftGateReport {
    pub(crate) accepted: bool,
    pub(crate) errors: Vec<String>,
    pub(crate) draft_candidate_id: Option<String>,
    pub(crate) external_ref_detected: bool,
    pub(crate) unsupported_claim_detected: bool,
    pub(crate) memory_as_evidence_detected: bool,
    pub(crate) internal_field_detected: bool,
    pub(crate) final_answer_attempt: bool,
    pub(crate) package_mutated: bool,
    pub(crate) claim_map_complete: bool,
    pub(crate) package_replay_ok: bool,
    pub(crate) review_required: bool,
    pub(crate) claim_count: usize,
    pub(crate) style_note_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReviewOverrideRecord {
    pub(crate) schema_version: &'static str,
    pub(crate) evidence_package_id: String,
    pub(crate) draft_candidate_id: String,
    pub(crate) local_reviewer_status: String,
    pub(crate) llm_reviewer_status: String,
    pub(crate) llm_reviewer_severity: String,
    pub(crate) final_decision: String,
    pub(crate) override_reason: String,
    pub(crate) required_revision_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RevisionLoopReport {
    pub(crate) revision_required: bool,
    pub(crate) revision_count: usize,
    pub(crate) revision_limit_respected: bool,
    pub(crate) package_mutated: bool,
    pub(crate) terminal_status: String,
    pub(crate) final_answer_allowed: bool,
    pub(crate) errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReviewerFlowReport {
    pub(crate) draft: DraftGateReport,
    pub(crate) override_record: ReviewOverrideRecord,
    pub(crate) revision: RevisionLoopReport,
    pub(crate) high_risk_false_pass: bool,
    pub(crate) override_violation: bool,
    pub(crate) llm_review_parse_error: bool,
    pub(crate) llm_high_risk_issue_count: usize,
}

#[derive(Debug, Clone)]
struct PackageSnapshot {
    package_id: String,
    evidence_ids: BTreeSet<String>,
    replay_ok: bool,
}

#[derive(Debug, Clone)]
struct LocalReview {
    status: String,
}

pub(crate) fn evaluate_profile_observation_contract(input: &Value) -> ProfileObservationReport {
    let projection = input.get("projection").unwrap_or(&Value::Null);
    let profile_output = input.get("profile_output").unwrap_or(&Value::Null);
    let allowed_consumers = string_set(input.get("allowed_consumers"));
    let allowed_evidence_ids = string_set(projection.get("allowed_evidence_ids"));
    let allowed_package_refs = string_set(projection.get("allowed_package_refs"));
    let consumer = projection
        .get("consumer")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let expected_digest = projection
        .get("expected_projection_digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let projection_digest = projection
        .get("projection_digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let visible_profiles = string_set(projection.get("visible_profile_refs"));
    let unknown_consumer = consumer.is_empty() || !allowed_consumers.contains(consumer);
    let digest_mismatch = expected_digest != projection_digest;
    let cross_profile_leakage = visible_profiles
        .iter()
        .any(|profile| profile.as_str() != consumer);
    let final_answer_attempt = first_forbidden_field(profile_output, &["final_answer"]).is_some();
    let internal_field_detected =
        first_forbidden_field(profile_output, PROFILE_OBSERVATION_FORBIDDEN_FIELDS).is_some();
    let mut errors = Vec::new();
    if unknown_consumer {
        errors.push("unknown_consumer".to_string());
    }
    if digest_mismatch {
        errors.push("projection_digest_mismatch".to_string());
    }
    if cross_profile_leakage {
        errors.push("cross_profile_leakage".to_string());
    }
    if internal_field_detected {
        errors.push("profile_observation_forbidden_field".to_string());
    }
    let observation = match serde_json::from_value::<ProfileObservation>(profile_output.clone()) {
        Ok(observation) => Some(observation),
        Err(error) => {
            errors.push(format!("profile_observation_schema_invalid: {error}"));
            None
        }
    };
    let mut external_ref_detected = false;
    let mut package_ref_allowed = true;
    let mut evidence_ref_count = 0;
    let mut candidate_claim_count = 0;
    if let Some(observation) = observation {
        evidence_ref_count = observation.evidence_refs.len();
        candidate_claim_count = observation.candidate_claims.len();
        if observation.schema_version != PROFILE_OBSERVATION_SCHEMA_VERSION {
            errors.push(format!(
                "schema_version_mismatch: {}",
                observation.schema_version
            ));
        }
        if observation.profile != consumer {
            errors.push("profile_consumer_mismatch".to_string());
        }
        if observation.observation.trim().is_empty() {
            errors.push("empty_observation".to_string());
        }
        if observation.confidence < MIN_OBSERVATION_CONFIDENCE || observation.confidence > 1.0 {
            errors.push("confidence_out_of_range".to_string());
        }
        external_ref_detected = observation
            .evidence_refs
            .iter()
            .any(|evidence_ref| !allowed_evidence_ids.contains(evidence_ref));
        if external_ref_detected {
            errors.push("external_evidence_ref".to_string());
        }
        if let Some(package_ref) = observation.package_ref.as_deref() {
            package_ref_allowed = allowed_package_refs.contains(package_ref);
            if !package_ref_allowed {
                errors.push("package_ref_not_allowed".to_string());
            }
        }
    }

    ProfileObservationReport {
        accepted: errors.is_empty(),
        errors,
        unknown_consumer,
        digest_mismatch,
        cross_profile_leakage,
        external_ref_detected,
        package_ref_allowed,
        final_answer_attempt,
        internal_field_detected,
        evidence_ref_count,
        candidate_claim_count,
    }
}

pub(crate) fn evaluate_draft_candidate_contract(
    package: &Value,
    draft_output: &Value,
) -> DraftGateReport {
    let snapshot = package_snapshot(package);
    let final_answer_attempt = first_forbidden_field(draft_output, &["final_answer"]).is_some();
    let package_mutated =
        first_forbidden_field(draft_output, &["package_mutated", "evidence_package_write"])
            .is_some();
    let internal_field_detected =
        first_forbidden_field(draft_output, DRAFT_FORBIDDEN_FIELDS).is_some();
    let mut errors = Vec::new();
    if snapshot.package_id.is_empty() {
        errors.push("package_id_missing".to_string());
    }
    if !snapshot.replay_ok {
        errors.push("package_replay_failed".to_string());
    }
    if internal_field_detected {
        errors.push("draft_forbidden_field".to_string());
    }
    let mut draft_candidate_id = draft_output
        .get("draft_candidate_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let draft = match serde_json::from_value::<DraftCandidate>(draft_output.clone()) {
        Ok(draft) => Some(draft),
        Err(error) => {
            errors.push(format!("draft_schema_invalid: {error}"));
            None
        }
    };
    let mut external_ref_detected = false;
    let mut unsupported_claim_detected = false;
    let mut memory_as_evidence_detected = false;
    let mut claim_map_complete = false;
    let mut claim_count = 0;
    let mut style_note_count = 0;
    if let Some(draft) = draft {
        draft_candidate_id = draft.draft_candidate_id.clone().or(draft_candidate_id);
        claim_count = draft.claims.len();
        style_note_count = draft.style_notes_applied.len();
        if draft.schema_version != DRAFT_CANDIDATE_SCHEMA_VERSION {
            errors.push(format!("schema_version_mismatch: {}", draft.schema_version));
        }
        if draft.evidence_package_id != snapshot.package_id {
            errors.push("package_id_mismatch".to_string());
        }
        if draft.resolved_question.trim().is_empty() {
            errors.push("resolved_question_missing".to_string());
        }
        if draft.draft.trim().is_empty() {
            errors.push("empty_draft".to_string());
        }
        unsupported_claim_detected = !draft.unsupported_claims.is_empty();
        if unsupported_claim_detected {
            errors.push("unsupported_claims_present".to_string());
        }
        memory_as_evidence_detected = draft.memory_used_as_evidence;
        if memory_as_evidence_detected {
            errors.push("memory_used_as_evidence".to_string());
        }
        claim_map_complete = !draft.claims.is_empty()
            && draft.claims.iter().all(|claim| {
                !claim.claim_id.trim().is_empty()
                    && !claim.text.trim().is_empty()
                    && !claim.evidence_refs.is_empty()
                    && claim.confidence >= MIN_DRAFT_CLAIM_CONFIDENCE
                    && claim.confidence <= 1.0
            });
        if !claim_map_complete {
            errors.push("claim_map_incomplete".to_string());
        }
        external_ref_detected = draft
            .claims
            .iter()
            .flat_map(|claim| &claim.evidence_refs)
            .any(|evidence_ref| !snapshot.evidence_ids.contains(evidence_ref));
        if external_ref_detected {
            errors.push("package_external_evidence_ref".to_string());
        }
    }
    let accepted = errors.is_empty();

    DraftGateReport {
        accepted,
        errors,
        draft_candidate_id,
        external_ref_detected,
        unsupported_claim_detected,
        memory_as_evidence_detected,
        internal_field_detected,
        final_answer_attempt,
        package_mutated,
        claim_map_complete,
        package_replay_ok: snapshot.replay_ok,
        review_required: accepted,
        claim_count,
        style_note_count,
    }
}

pub(crate) fn evaluate_reviewer_flow(input: &Value) -> ReviewerFlowReport {
    let package = input.get("package").unwrap_or(&Value::Null);
    let draft_output = input.get("draft_candidate").unwrap_or(&Value::Null);
    let draft = evaluate_draft_candidate_contract(package, draft_output);
    let snapshot = package_snapshot(package);
    let local = local_review(input.get("local_review"));
    let mut llm_review_parse_error = false;
    let llm_review_output = input.get("llm_review").unwrap_or(&Value::Null);
    let review_forbidden_field = first_forbidden_field(llm_review_output, REVIEW_FORBIDDEN_FIELDS);
    let llm_review = match serde_json::from_value::<LlmReviewObservation>(llm_review_output.clone())
    {
        Ok(review)
            if review_forbidden_field.is_none()
                && review.schema_version == LLM_REVIEW_OBSERVATION_SCHEMA_VERSION
                && review.confidence >= MIN_OBSERVATION_CONFIDENCE
                && review.confidence <= 1.0 =>
        {
            Some(review)
        }
        Ok(_) => {
            llm_review_parse_error = true;
            None
        }
        Err(_) => {
            llm_review_parse_error = true;
            None
        }
    };
    let high_risk_issue_present = input
        .get("high_risk_issue_present")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || draft.external_ref_detected
        || draft.unsupported_claim_detected
        || draft.memory_as_evidence_detected
        || draft.internal_field_detected;
    let conflict = review_conflict(
        &snapshot,
        &draft,
        &local,
        llm_review.as_ref(),
        high_risk_issue_present,
        llm_review_parse_error,
    );
    let revision = revision_loop_report(input, &snapshot, &conflict, &draft);
    let override_violation = (is_fail_status(&local.status)
        && conflict.llm_reviewer_status == "pass"
        && conflict.final_decision.starts_with("passed"))
        || conflict.high_risk_false_pass;

    ReviewerFlowReport {
        draft,
        override_record: conflict.override_record,
        revision,
        high_risk_false_pass: conflict.high_risk_false_pass,
        override_violation,
        llm_review_parse_error,
        llm_high_risk_issue_count: conflict.llm_high_risk_issue_count,
    }
}

struct ReviewConflict {
    override_record: ReviewOverrideRecord,
    high_risk_false_pass: bool,
    llm_reviewer_status: String,
    final_decision: String,
    llm_high_risk_issue_count: usize,
}

fn review_conflict(
    package: &PackageSnapshot,
    draft: &DraftGateReport,
    local: &LocalReview,
    llm_review: Option<&LlmReviewObservation>,
    high_risk_issue_present: bool,
    llm_review_parse_error: bool,
) -> ReviewConflict {
    let local_pass = is_pass_status(&local.status);
    let llm_status = llm_review
        .map(|review| normalize_review_status(&review.status))
        .unwrap_or_else(|| "fail".to_string());
    let llm_severity = llm_review
        .map(|review| normalize_severity(&review.severity))
        .unwrap_or_else(|| "high".to_string());
    let llm_high_risk_issue_count = llm_review
        .map(|review| {
            review
                .issues
                .iter()
                .filter(|issue| {
                    normalize_severity(&issue.severity) == "high"
                        && !issue.issue_id.trim().is_empty()
                        && !issue.category.trim().is_empty()
                        && !issue.summary.trim().is_empty()
                        && issue
                            .claim_id
                            .as_deref()
                            .is_none_or(|claim_id| !claim_id.is_empty())
                        && issue
                            .evidence_refs
                            .iter()
                            .all(|evidence_ref| !evidence_ref.is_empty())
                })
                .count()
        })
        .unwrap_or(usize::from(llm_review_parse_error));
    let llm_high_risk = llm_severity == "high" || llm_high_risk_issue_count > 0;
    let llm_low_risk = llm_severity == "low";
    let llm_pass = llm_status == "pass";
    let llm_fail = llm_status == "fail";
    let high_risk_false_pass = high_risk_issue_present && llm_pass;
    let (final_decision, override_reason) = if !draft.accepted && local_pass {
        ("revision_required", "llm_high_risk_blocks_final")
    } else if !local_pass && llm_pass {
        ("revision_required", "local_enforcement_blocks_llm_pass")
    } else if high_risk_false_pass || (local_pass && llm_fail && llm_high_risk) {
        ("revision_required", "llm_high_risk_blocks_final")
    } else if local_pass && llm_fail && llm_low_risk {
        ("passed_with_warning", "llm_low_risk_warning_recorded")
    } else if !local_pass && llm_fail {
        ("revision_required", "both_reviewers_block_final")
    } else if local_pass && llm_pass {
        ("passed", "review_pass")
    } else if local_pass && llm_review_parse_error {
        ("revision_required", "llm_high_risk_blocks_final")
    } else {
        ("revision_required", "both_reviewers_block_final")
    };
    let required_revision_ids = if final_decision == "revision_required" {
        llm_review
            .map(|review| review.required_revisions.clone())
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(|| vec!["revision:required".to_string()])
    } else {
        Vec::new()
    };
    let draft_candidate_id = draft
        .draft_candidate_id
        .clone()
        .unwrap_or_else(|| "draft:missing".to_string());

    ReviewConflict {
        override_record: ReviewOverrideRecord {
            schema_version: REVIEW_OVERRIDE_SCHEMA_VERSION,
            evidence_package_id: package.package_id.clone(),
            draft_candidate_id,
            local_reviewer_status: normalize_review_status(&local.status),
            llm_reviewer_status: llm_status.clone(),
            llm_reviewer_severity: llm_severity,
            final_decision: final_decision.to_string(),
            override_reason: override_reason.to_string(),
            required_revision_ids,
        },
        high_risk_false_pass,
        llm_reviewer_status: llm_status,
        final_decision: final_decision.to_string(),
        llm_high_risk_issue_count,
    }
}

fn revision_loop_report(
    input: &Value,
    package: &PackageSnapshot,
    conflict: &ReviewConflict,
    draft: &DraftGateReport,
) -> RevisionLoopReport {
    let revisions = input
        .get("revisions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let revision_required = conflict.override_record.final_decision == "revision_required";
    let mut errors = Vec::new();
    let mut package_mutated = false;
    let mut revision_count = 0;
    for (index, value) in revisions.iter().enumerate() {
        revision_count += 1;
        match serde_json::from_value::<DraftRevisionRecord>(value.clone()) {
            Ok(record) => {
                if record.schema_version != DRAFT_REVISION_SCHEMA_VERSION {
                    errors.push(format!(
                        "revision_schema_version_mismatch:{}",
                        record.schema_version
                    ));
                }
                if record.revision_index != index + 1 {
                    errors.push("revision_index_not_sequential".to_string());
                }
                if record.evidence_package_id != package.package_id {
                    errors.push("revision_package_id_mismatch".to_string());
                }
                if record.revision_id.trim().is_empty()
                    || record.previous_draft_candidate_id.trim().is_empty()
                    || record.previous_review_id.trim().is_empty()
                    || record.revised_draft_candidate_id.trim().is_empty()
                {
                    errors.push("revision_ref_missing".to_string());
                }
                if record.required_revision_reasons.is_empty() {
                    errors.push("revision_reason_missing".to_string());
                }
                if record.package_mutated {
                    package_mutated = true;
                    errors.push("revision_package_mutated".to_string());
                }
            }
            Err(error) => {
                errors.push(format!("revision_schema_invalid: {error}"));
            }
        }
    }
    let revision_limit_respected = revision_count <= MAX_REVISION_COUNT;
    if !revision_limit_respected {
        errors.push("revision_limit_exceeded".to_string());
    }
    let post_revision_passed = input
        .get("post_revision")
        .map(post_revision_review_passed)
        .unwrap_or(false);
    let terminal_status = if !errors.is_empty() {
        "failed_closed"
    } else if !revision_required {
        conflict.final_decision.as_str()
    } else if revision_count == 0 {
        "revision_required"
    } else if post_revision_passed {
        "passed_after_revision"
    } else if revision_count >= MAX_REVISION_COUNT {
        "failed_closed"
    } else {
        "revision_required"
    };
    let final_answer_allowed = draft.accepted
        && matches!(
            terminal_status,
            "passed" | "passed_with_warning" | "passed_after_revision"
        );

    RevisionLoopReport {
        revision_required,
        revision_count,
        revision_limit_respected,
        package_mutated,
        terminal_status: terminal_status.to_string(),
        final_answer_allowed,
        errors,
    }
}

fn post_revision_review_passed(value: &Value) -> bool {
    let local_status = value
        .get("local_reviewer_status")
        .and_then(Value::as_str)
        .map(normalize_review_status)
        .unwrap_or_else(|| "fail".to_string());
    let llm_status = value
        .get("llm_reviewer_status")
        .and_then(Value::as_str)
        .map(normalize_review_status)
        .unwrap_or_else(|| "fail".to_string());
    is_pass_status(&local_status) && llm_status == "pass"
}

fn package_snapshot(value: &Value) -> PackageSnapshot {
    let package_id = value
        .get("package_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut evidence_ids = string_set(value.get("evidence_ids"));
    if let Some(cards) = value.get("cards").and_then(Value::as_array) {
        for card in cards {
            if let Some(evidence_id) = card.get("evidence_id").and_then(Value::as_str) {
                evidence_ids.insert(evidence_id.to_string());
            }
        }
    }
    if let Some(claim_maps) = value.get("claim_evidence_map").and_then(Value::as_array) {
        for claim_map in claim_maps {
            evidence_ids.extend(string_set(claim_map.get("evidence_ids")));
        }
    }
    let replay_ok = value
        .get("replay_ok")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    PackageSnapshot {
        package_id,
        evidence_ids,
        replay_ok,
    }
}

fn local_review(value: Option<&Value>) -> LocalReview {
    let value = value.unwrap_or(&Value::Null);
    LocalReview {
        status: value
            .get("status")
            .and_then(Value::as_str)
            .map(normalize_review_status)
            .unwrap_or_else(|| "fail".to_string()),
    }
}

fn is_pass_status(status: &str) -> bool {
    normalize_review_status(status) == "pass"
}

fn is_fail_status(status: &str) -> bool {
    normalize_review_status(status) == "fail"
}

fn normalize_review_status(status: &str) -> String {
    match status {
        "pass" | "passed" => "pass".to_string(),
        "warning" | "passed_with_warning" => "warning".to_string(),
        _ => "fail".to_string(),
    }
}

fn normalize_severity(severity: &str) -> String {
    match severity {
        "none" | "low" | "high" => severity.to_string(),
        _ => "high".to_string(),
    }
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn first_forbidden_field<'a>(value: &Value, forbidden_fields: &'a [&'a str]) -> Option<&'a str> {
    match value {
        Value::Object(map) => {
            for key in map.keys() {
                if let Some(field) = forbidden_fields.iter().copied().find(|field| key == field) {
                    return Some(field);
                }
            }
            map.values()
                .find_map(|item| first_forbidden_field(item, forbidden_fields))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| first_forbidden_field(item, forbidden_fields)),
        _ => None,
    }
}

pub(crate) fn draft_gate_observation(report: &DraftGateReport) -> Value {
    json!({
        "accepted": report.accepted,
        "error_count": report.errors.len(),
        "errors": &report.errors,
        "draft_candidate_id": &report.draft_candidate_id,
        "external_ref_detected": report.external_ref_detected,
        "unsupported_claim_detected": report.unsupported_claim_detected,
        "memory_as_evidence_detected": report.memory_as_evidence_detected,
        "internal_field_detected": report.internal_field_detected,
        "final_answer_attempt": report.final_answer_attempt,
        "package_mutated": report.package_mutated,
        "claim_map_complete": report.claim_map_complete,
        "package_replay_ok": report.package_replay_ok,
        "review_required": report.review_required,
        "claim_count": report.claim_count,
        "style_note_count": report.style_note_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> Value {
        json!({
            "package_id": "package:test",
            "evidence_ids": ["evidence:1"],
            "replay_ok": true
        })
    }

    fn draft(evidence_refs: Vec<&str>) -> Value {
        json!({
            "schema_version": DRAFT_CANDIDATE_SCHEMA_VERSION,
            "draft_candidate_id": "draft:test",
            "evidence_package_id": "package:test",
            "resolved_question": "通灵玉是什么？",
            "draft": "通灵玉的文字有正文证据。",
            "claims": [{
                "claim_id": "claim:1",
                "text": "通灵玉的文字有正文证据。",
                "evidence_refs": evidence_refs,
                "confidence": 0.86
            }],
            "unsupported_claims": [],
            "style_notes_applied": ["简洁回答"],
            "memory_used_as_evidence": false
        })
    }

    #[test]
    fn draft_candidate_rejects_package_external_refs() {
        let report = evaluate_draft_candidate_contract(&package(), &draft(vec!["evidence:other"]));

        assert!(!report.accepted);
        assert!(report.external_ref_detected);
    }

    #[test]
    fn llm_reviewer_pass_cannot_override_local_fail() {
        let report = evaluate_reviewer_flow(&json!({
            "package": package(),
            "draft_candidate": draft(vec!["evidence:1"]),
            "local_review": {"status": "fail", "severity": "high"},
            "llm_review": {
                "schema_version": LLM_REVIEW_OBSERVATION_SCHEMA_VERSION,
                "status": "pass",
                "severity": "none",
                "issues": [],
                "required_revisions": [],
                "confidence": 0.9
            },
            "revisions": []
        }));

        assert_eq!(
            report.override_record.override_reason,
            "local_enforcement_blocks_llm_pass"
        );
        assert!(!report.revision.final_answer_allowed);
        assert!(!report.override_violation);
    }

    #[test]
    fn second_failed_revision_closes_without_final_answer() {
        let report = evaluate_reviewer_flow(&json!({
            "package": package(),
            "draft_candidate": draft(vec!["evidence:1"]),
            "local_review": {"status": "pass", "severity": "none"},
            "llm_review": {
                "schema_version": LLM_REVIEW_OBSERVATION_SCHEMA_VERSION,
                "status": "fail",
                "severity": "high",
                "issues": [{
                    "issue_id": "issue:1",
                    "category": "unsupported_claim",
                    "severity": "high",
                    "claim_id": "claim:1",
                    "evidence_refs": ["evidence:1"],
                    "summary": "claim needs revision"
                }],
                "required_revisions": ["revision:1"],
                "confidence": 0.9
            },
            "revisions": [
                {
                    "schema_version": DRAFT_REVISION_SCHEMA_VERSION,
                    "revision_id": "revision:1",
                    "revision_index": 1,
                    "evidence_package_id": "package:test",
                    "previous_draft_candidate_id": "draft:test",
                    "previous_review_id": "review:1",
                    "required_revision_reasons": ["llm_high_risk_blocks_final"],
                    "revised_draft_candidate_id": "draft:revision:1",
                    "package_mutated": false
                },
                {
                    "schema_version": DRAFT_REVISION_SCHEMA_VERSION,
                    "revision_id": "revision:2",
                    "revision_index": 2,
                    "evidence_package_id": "package:test",
                    "previous_draft_candidate_id": "draft:revision:1",
                    "previous_review_id": "review:2",
                    "required_revision_reasons": ["llm_high_risk_blocks_final"],
                    "revised_draft_candidate_id": "draft:revision:2",
                    "package_mutated": false
                }
            ]
        }));

        assert_eq!(report.revision.terminal_status, "failed_closed");
        assert!(!report.revision.final_answer_allowed);
    }
}
