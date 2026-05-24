use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{
    DEFAULT_MODEL_ID,
    context_governance::validate_llm_memory_extraction_output,
    conversation_state::{
        ConversationStateInput, ConversationStateMessage, ConversationStateSummary,
        conversation_state_validation_context, project_conversation_state_summary,
        validate_conversation_state_summary, write_conversation_state_summary,
    },
    draft_revision::{
        draft_gate_observation, evaluate_draft_candidate_contract,
        evaluate_profile_observation_contract, evaluate_reviewer_flow,
    },
    llm_contracts::{
        CONTEXT_PROJECTION_DATASET, CONTEXT_PROJECTION_MIN_CASES, DEFAULT_MAX_BODY_CHARS,
        DEFAULT_MAX_MESSAGES, DEFAULT_MAX_QUESTION_CHARS, LLM_EVAL_REPORT_SCHEMA_VERSION,
        LLM_EVAL_SUITE_VERSION, LlmEvalFixture, MEMORY_POLICY_DATASET, MEMORY_POLICY_MIN_CASES,
        PACKAGE_CLAIMS_DATASET, PACKAGE_CLAIMS_MIN_CASES, PUBLIC_OUTPUT_FORBIDDEN_KEYS,
        QUESTION_RESOLUTION_DATASET, QUESTION_RESOLUTION_MIN_CASES, RAG_EVIDENCE_DATASET,
        RAG_EVIDENCE_MIN_CASES, REQUEST_SAFETY_DATASET, REQUEST_SAFETY_MIN_CASES,
        RETRIEVAL_POLICY_DATASET, RETRIEVAL_POLICY_MIN_CASES, REVIEWER_SECURITY_DATASET,
        REVIEWER_SECURITY_MIN_CASES, S1_STAGE, S2_STAGE, S3_STAGE, S4_STAGE, S5_STAGE, S6_STAGE,
        S7_STAGE, SESSION_SUMMARY_DATASET, SESSION_SUMMARY_MIN_CASES, STREAMING_DEDUPE_DATASET,
        STREAMING_DEDUPE_MIN_CASES,
    },
    llm_modes::LlmMode,
    llm_provider::{FakeLlmProvider, LlmProviderError},
    llm_resolver::{
        ResolverContractDecision, evaluate_resolver_contract, evaluate_resolver_with_provider,
    },
    plan::SearchPolicy,
    retrieval_suggestion::{
        evaluate_retrieval_policy_suggestion, retrieval_policy_patch_observation,
    },
    user_response_safety::{fixture_has_internal_leakage, scan_fixture_surfaces},
};

const REQUEST_GATE_EXPECTED: &str = "request_gate_expected";
const NO_INTERNAL_LEAKAGE: &str = "no_internal_leakage";
const INTERNAL_LEAKAGE_DETECTED: &str = "internal_leakage_detected";
const RESPONSE_CONSISTENCY: &str = "response_consistency";
const RESOLVER_CONTRACT_EXPECTED: &str = "resolver_contract_expected";
const RESOLVER_PROVIDER_ROUTING_EXPECTED: &str = "resolver_provider_routing_expected";
const UNKNOWN_CONTEXT_REF_FAIL_CLOSED: &str = "unknown_context_ref_fail_closed";
const SUMMARY_CONTRACT_EXPECTED: &str = "summary_contract_expected";
const NO_SUMMARY_HALLUCINATION: &str = "no_summary_hallucination";
const BOUNDARY_PRESERVATION: &str = "boundary_preservation";
const NO_MEMORY_AS_EVIDENCE: &str = "no_memory_as_evidence";
const PROJECTION_GUARD_EXPECTED: &str = "projection_guard_expected";
const RETRIEVAL_POLICY_PATCH_EXPECTED: &str = "retrieval_policy_patch_expected";
const REQUIRED_EVIDENCE_NOT_DOWNGRADED: &str = "required_evidence_not_downgraded";
const TOOL_PROFILE_NOT_MUTATED: &str = "tool_profile_not_mutated";
const RAG_EVIDENCE_EXPECTED: &str = "rag_evidence_expected";
const SOURCE_VERSION_BOUNDARY: &str = "source_version_boundary";
const PROJECTION_ISOLATION_EXPECTED: &str = "projection_isolation_expected";
const PROFILE_OBSERVATION_EXPECTED: &str = "profile_observation_expected";
const DRAFT_CANDIDATE_EXPECTED: &str = "draft_candidate_expected";
const PACKAGE_CLAIMS_EXPECTED: &str = "package_claims_expected";
const REVIEWER_SECURITY_EXPECTED: &str = "reviewer_security_expected";
const REVIEW_OVERRIDE_EXPECTED: &str = "review_override_expected";
const REVISION_LOOP_EXPECTED: &str = "revision_loop_expected";
const MEMORY_POLICY_EXPECTED: &str = "memory_policy_expected";
const MEMORY_BOUNDARY_EXPECTED: &str = "memory_boundary_expected";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmEvalCaseResult {
    case_id: String,
    dataset: String,
    stage: String,
    passed: bool,
    failures: Vec<String>,
    observed: Value,
}

pub fn run_llm_eval(
    fixture_dir: &Path,
    report_out: &Path,
    fail_on_hard_gate: bool,
) -> Result<Value> {
    let fixtures = load_fixtures(fixture_dir)?;
    let config_digest = fixture_config_digest(&fixtures)?;
    let mut results = Vec::new();
    let mut hard_gate_failures = Vec::new();
    let mut dataset_counts = BTreeMap::<String, usize>::new();

    for fixture in &fixtures {
        *dataset_counts.entry(fixture.dataset.clone()).or_default() += 1;
        let result = evaluate_fixture(fixture);
        if !result.passed && !fixture.hard_gates.is_empty() {
            hard_gate_failures.push(json!({
                "case_id": fixture.case_id,
                "dataset": fixture.dataset,
                "hard_gates": fixture.hard_gates,
                "failure_count": result.failures.len(),
            }));
        }
        results.push(result);
    }

    let suite_failures = validate_fixture_suite(&dataset_counts);
    for failure in &suite_failures {
        hard_gate_failures.push(json!({
            "case_id": "suite",
            "dataset": "suite",
            "hard_gates": ["fixture_contract"],
            "failure": failure,
        }));
    }

    let case_failed = results.iter().filter(|result| !result.passed).count();
    let failed = case_failed + suite_failures.len();
    let status = if failed == 0 { "passed" } else { "failed" };
    let failure_attribution = failure_attribution(&results, &suite_failures);
    let report = json!({
        "object": "tonglingyu.llm_eval_report",
        "schema_version": LLM_EVAL_REPORT_SCHEMA_VERSION,
        "eval_run_id": format!("llm-eval-{}", uuid::Uuid::now_v7().simple()),
        "status": status,
        "fixture_dir": fixture_dir.display().to_string(),
        "suite_version": LLM_EVAL_SUITE_VERSION,
        "case_counts": {
            "total": fixtures.len(),
            "passed": results.len().saturating_sub(case_failed),
            "failed": failed,
        },
        "hard_gate_failures": hard_gate_failures,
        "metric_summary": {
            "datasets": dataset_counts,
            "s1_minimums": {
                REQUEST_SAFETY_DATASET: REQUEST_SAFETY_MIN_CASES,
                STREAMING_DEDUPE_DATASET: STREAMING_DEDUPE_MIN_CASES,
            },
            "s2_minimums": {
                QUESTION_RESOLUTION_DATASET: QUESTION_RESOLUTION_MIN_CASES,
            },
            "s4_minimums": {
                SESSION_SUMMARY_DATASET: SESSION_SUMMARY_MIN_CASES,
            },
            "s5_minimums": {
                RETRIEVAL_POLICY_DATASET: RETRIEVAL_POLICY_MIN_CASES,
                RAG_EVIDENCE_DATASET: RAG_EVIDENCE_MIN_CASES,
            },
            "s6_minimums": {
                CONTEXT_PROJECTION_DATASET: CONTEXT_PROJECTION_MIN_CASES,
                PACKAGE_CLAIMS_DATASET: PACKAGE_CLAIMS_MIN_CASES,
                REVIEWER_SECURITY_DATASET: REVIEWER_SECURITY_MIN_CASES,
            },
            "s7_minimums": {
                MEMORY_POLICY_DATASET: MEMORY_POLICY_MIN_CASES,
            },
            "fail_on_hard_gate": fail_on_hard_gate,
        },
        "failure_attribution": failure_attribution,
        "config_digest": config_digest,
        "cases": results,
    });

    if let Some(parent) = report_out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(
        report_out,
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )
    .with_context(|| format!("write {}", report_out.display()))?;

    if fail_on_hard_gate && status != "passed" {
        return Err(anyhow!("llm eval hard gate failed"));
    }
    Ok(report)
}

pub fn write_llm_release_report(eval_report_path: &Path, report_out: &Path) -> Result<Value> {
    let data = fs::read(eval_report_path)
        .with_context(|| format!("read {}", eval_report_path.display()))?;
    let eval_report: Value = serde_json::from_slice(&data)
        .with_context(|| format!("parse {}", eval_report_path.display()))?;
    let sha256 = format!("sha256:{:x}", Sha256::digest(&data));
    let dataset_counts = eval_report
        .pointer("/metric_summary/datasets")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let missing_or_short = required_dataset_minimums()
        .iter()
        .filter_map(|(dataset, minimum)| {
            let count = dataset_counts
                .get(*dataset)
                .and_then(Value::as_u64)
                .unwrap_or_default();
            if count < *minimum as u64 {
                Some(json!({
                    "dataset": dataset,
                    "minimum": minimum,
                    "actual": count,
                }))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let hard_gate_failures = eval_report
        .get("hard_gate_failures")
        .and_then(Value::as_array)
        .map_or(usize::MAX, Vec::len);
    let eval_report_object_valid =
        eval_report.get("object").and_then(Value::as_str) == Some("tonglingyu.llm_eval_report");
    let eval_report_schema_valid =
        eval_report.get("schema_version").and_then(Value::as_str) == Some("v1");
    let eval_report_suite_valid =
        eval_report.get("suite_version").and_then(Value::as_str) == Some(LLM_EVAL_SUITE_VERSION);
    let llm_eval_run_id_present = eval_report
        .get("eval_run_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let total_cases = eval_report
        .pointer("/case_counts/total")
        .and_then(Value::as_u64);
    let passed_cases = eval_report
        .pointer("/case_counts/passed")
        .and_then(Value::as_u64);
    let failed_cases = eval_report
        .pointer("/case_counts/failed")
        .and_then(Value::as_u64);
    let case_counts_complete = match (total_cases, passed_cases, failed_cases) {
        (Some(total), Some(passed), Some(failed)) => {
            total > 0 && failed == 0 && passed == total && passed.checked_add(failed) == Some(total)
        }
        _ => false,
    };
    let eval_passed = eval_report.get("status").and_then(Value::as_str) == Some("passed");
    let eval_report_valid = eval_report_object_valid
        && eval_report_schema_valid
        && eval_report_suite_valid
        && llm_eval_run_id_present
        && case_counts_complete;
    let local_release_gate_passed =
        eval_report_valid && eval_passed && hard_gate_failures == 0 && missing_or_short.is_empty();
    let report = json!({
        "object": "tonglingyu.llm_release_report",
        "schema_version": "v1",
        "release_run_id": format!("llm-release-{}", uuid::Uuid::now_v7().simple()),
        "status": if local_release_gate_passed { "passed" } else { "failed" },
        "llm_eval_report_path": eval_report_path.display().to_string(),
        "llm_eval_report_sha256": sha256,
        "llm_eval_run_id": eval_report.get("eval_run_id").cloned().unwrap_or(Value::Null),
        "suite_version": eval_report.get("suite_version").cloned().unwrap_or(Value::Null),
        "case_counts": eval_report.get("case_counts").cloned().unwrap_or(Value::Null),
        "readiness_checks": {
            "eval_report_object_valid": eval_report_object_valid,
            "eval_report_schema_valid": eval_report_schema_valid,
            "eval_report_suite_valid": eval_report_suite_valid,
            "llm_eval_run_id_present": llm_eval_run_id_present,
            "case_counts_complete": case_counts_complete,
            "repo_local_llm_eval_passed": eval_passed,
            "hard_gate_failure_count": hard_gate_failures,
            "s1_to_s7_dataset_minimums_present": missing_or_short.is_empty(),
            "missing_or_short_datasets": missing_or_short,
            "user_response_safety_gate_present": dataset_counts.contains_key(REQUEST_SAFETY_DATASET)
                && dataset_counts.contains_key(STREAMING_DEDUPE_DATASET),
            "target_environment_live_gate_required": true,
            "target_environment_live_gate_verified": false,
            "production_ready_declaration_allowed": false,
        },
        "llm_agent": {
            "schema_version": "tonglingyu-llm-agent-release-v1",
            "required_profiles": [
                "tonglingyu-question-normalizer",
                "tonglingyu-conversation-state-writer",
            ],
            "profile_contracts": {
                "question_normalizer_registered": true,
                "conversation_state_writer_registered": true,
                "allowed_tools_empty": true,
                "runtime_profile_execution_required": true,
            },
            "output_control": {
                "business_validator_required": true,
                "sealed_decision_required": true,
                "denylist_scanner_required": true,
                "confidence_gate_required": true,
                "raw_agent_output_embedded": false,
                "context_pack_raw_agent_output_embedded": false,
            },
            "mode_matrix": {
                "default_modes": {
                    "question_normalizer": "enforced",
                    "conversation_state_writer": "enforced",
                    "rollback_target_must_be_enforced": true,
                },
                "required_modes": [
                    "disabled",
                    "two_agent_shadow",
                    "question_normalizer_enforced",
                    "two_agent_enforced",
                ],
                "repo_local_contract_tests_required": true,
                "target_environment_live_gate_required": true,
                "target_environment_live_gate_verified": false,
            },
            "agent_request_envelope": {
                "schema_version": "tonglingyu-llm-agent-request-v1",
                "agent_request_aligned": true,
                "input_digest_required": true,
                "projection_ref_required": true,
            },
        },
        "artifact_policy": {
            "raw_llm_payload_embedded": false,
            "raw_agent_output_embedded": false,
            "context_pack_raw_agent_output_embedded": false,
            "raw_memory_embedded": false,
            "tool_payload_embedded": false,
        },
    });
    if let Some(parent) = report_out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(
        report_out,
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )
    .with_context(|| format!("write {}", report_out.display()))?;
    if !local_release_gate_passed {
        return Err(anyhow!("llm release gate failed"));
    }
    Ok(report)
}

fn required_dataset_minimums() -> &'static [(&'static str, usize)] {
    &[
        (REQUEST_SAFETY_DATASET, REQUEST_SAFETY_MIN_CASES),
        (STREAMING_DEDUPE_DATASET, STREAMING_DEDUPE_MIN_CASES),
        (QUESTION_RESOLUTION_DATASET, QUESTION_RESOLUTION_MIN_CASES),
        (SESSION_SUMMARY_DATASET, SESSION_SUMMARY_MIN_CASES),
        (RETRIEVAL_POLICY_DATASET, RETRIEVAL_POLICY_MIN_CASES),
        (RAG_EVIDENCE_DATASET, RAG_EVIDENCE_MIN_CASES),
        (CONTEXT_PROJECTION_DATASET, CONTEXT_PROJECTION_MIN_CASES),
        (PACKAGE_CLAIMS_DATASET, PACKAGE_CLAIMS_MIN_CASES),
        (REVIEWER_SECURITY_DATASET, REVIEWER_SECURITY_MIN_CASES),
        (MEMORY_POLICY_DATASET, MEMORY_POLICY_MIN_CASES),
    ]
}

fn load_fixtures(fixture_dir: &Path) -> Result<Vec<LlmEvalFixture>> {
    let mut paths = fs::read_dir(fixture_dir)
        .with_context(|| format!("read {}", fixture_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<PathBuf>>>()
        .with_context(|| format!("list {}", fixture_dir.display()))?;
    paths.sort();

    let mut fixtures = Vec::new();
    for path in paths {
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        for (line_index, line) in data.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let fixture = serde_json::from_str::<LlmEvalFixture>(line)
                .with_context(|| format!("parse {}:{}", path.display(), line_index + 1))?;
            fixtures.push(fixture);
        }
    }
    if fixtures.is_empty() {
        return Err(anyhow!(
            "no llm eval fixtures found in {}",
            fixture_dir.display()
        ));
    }
    Ok(fixtures)
}

fn evaluate_fixture(fixture: &LlmEvalFixture) -> LlmEvalCaseResult {
    let mut failures = validate_common_fixture(fixture);
    let observed = match fixture.dataset.as_str() {
        REQUEST_SAFETY_DATASET => evaluate_request_safety(fixture, &mut failures),
        STREAMING_DEDUPE_DATASET => evaluate_streaming_dedupe(fixture, &mut failures),
        QUESTION_RESOLUTION_DATASET => evaluate_question_resolution(fixture, &mut failures),
        SESSION_SUMMARY_DATASET => evaluate_session_summary(fixture, &mut failures),
        RETRIEVAL_POLICY_DATASET => evaluate_retrieval_policy(fixture, &mut failures),
        RAG_EVIDENCE_DATASET => evaluate_rag_evidence(fixture, &mut failures),
        CONTEXT_PROJECTION_DATASET => evaluate_context_projection(fixture, &mut failures),
        PACKAGE_CLAIMS_DATASET => evaluate_package_claims(fixture, &mut failures),
        REVIEWER_SECURITY_DATASET => evaluate_reviewer_security(fixture, &mut failures),
        MEMORY_POLICY_DATASET => evaluate_memory_policy(fixture, &mut failures),
        dataset => {
            failures.push(format!("unsupported dataset: {dataset}"));
            json!({})
        }
    };
    LlmEvalCaseResult {
        case_id: fixture.case_id.clone(),
        dataset: fixture.dataset.clone(),
        stage: fixture.stage.clone(),
        passed: failures.is_empty(),
        failures,
        observed,
    }
}

fn validate_common_fixture(fixture: &LlmEvalFixture) -> Vec<String> {
    let mut failures = Vec::new();
    if fixture.case_id.trim().is_empty() {
        failures.push("missing case_id".to_string());
    }
    let expected_stage = match fixture.dataset.as_str() {
        REQUEST_SAFETY_DATASET | STREAMING_DEDUPE_DATASET => S1_STAGE,
        QUESTION_RESOLUTION_DATASET if fixture.input.get("provider_mode").is_some() => S3_STAGE,
        QUESTION_RESOLUTION_DATASET => S2_STAGE,
        SESSION_SUMMARY_DATASET => S4_STAGE,
        RETRIEVAL_POLICY_DATASET | RAG_EVIDENCE_DATASET => S5_STAGE,
        CONTEXT_PROJECTION_DATASET | PACKAGE_CLAIMS_DATASET | REVIEWER_SECURITY_DATASET => S6_STAGE,
        MEMORY_POLICY_DATASET => S7_STAGE,
        _ => S1_STAGE,
    };
    if fixture.stage != expected_stage {
        failures.push(format!(
            "expected stage={expected_stage} got {}",
            fixture.stage
        ));
    }
    if fixture.hard_gates.is_empty() {
        failures.push("missing hard_gates".to_string());
    }
    if fixture.input == Value::Null || fixture.input == json!({}) {
        failures.push("missing input".to_string());
    }
    if fixture.expected == Value::Null || fixture.expected == json!({}) {
        failures.push("missing expected".to_string());
    }
    failures
}

fn evaluate_question_resolution(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    if fixture.input.get("provider_mode").is_some() {
        return evaluate_question_resolution_provider(fixture, failures);
    }
    let trigger = fixture
        .input
        .get("trigger")
        .and_then(Value::as_str)
        .unwrap_or("missing_trigger");
    let output = fixture.input.get("llm_output").unwrap_or(&Value::Null);
    let expected_decision = fixture
        .expected
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or("missing_expected_decision");
    let expected_accepted = fixture
        .expected
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let expected_can_call = fixture
        .expected
        .get("can_call_llm")
        .and_then(Value::as_bool);
    let evaluation = evaluate_resolver_contract(trigger, output);
    let decision = match evaluation.decision {
        ResolverContractDecision::Accept => "accept",
        ResolverContractDecision::Clarify => "clarify",
        ResolverContractDecision::FailClosed => "fail_closed",
    };

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == RESOLVER_CONTRACT_EXPECTED)
    {
        failures.push(format!("missing hard gate {RESOLVER_CONTRACT_EXPECTED}"));
    }
    if output
        .get("used_context_refs")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("raw_memory")))
        && !fixture
            .hard_gates
            .iter()
            .any(|gate| gate == UNKNOWN_CONTEXT_REF_FAIL_CLOSED)
    {
        failures.push(format!(
            "missing hard gate {UNKNOWN_CONTEXT_REF_FAIL_CLOSED}"
        ));
    }
    if evaluation.accepted != expected_accepted || decision != expected_decision {
        failures.push(format!(
            "resolver contract mismatch expected accepted={expected_accepted} decision={expected_decision} got accepted={} decision={decision}",
            evaluation.accepted
        ));
    }
    if let Some(expected) = expected_can_call
        && evaluation.can_call_llm != expected
    {
        failures.push(format!(
            "resolver trigger callability mismatch expected={expected} got={}",
            evaluation.can_call_llm
        ));
    }

    json!({
        "accepted": evaluation.accepted,
        "decision": decision,
        "can_call_llm": evaluation.can_call_llm,
        "error_count": evaluation.errors.len(),
        "errors": evaluation.errors,
    })
}

fn evaluate_question_resolution_provider(
    fixture: &LlmEvalFixture,
    failures: &mut Vec<String>,
) -> Value {
    let trigger = fixture
        .input
        .get("trigger")
        .and_then(Value::as_str)
        .unwrap_or("missing_trigger");
    let mode = fixture
        .input
        .get("provider_mode")
        .and_then(Value::as_str)
        .and_then(|value| LlmMode::parse(value).ok())
        .unwrap_or(LlmMode::Disabled);
    let provider_input = fixture
        .input
        .get("provider_input")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let responses = fixture
        .input
        .get("provider_responses")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    if let Some(error) = item.get("error").and_then(Value::as_str) {
                        Err(provider_error_from_str(error))
                    } else {
                        Ok(item.get("ok").cloned().unwrap_or_else(|| item.clone()))
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut provider = FakeLlmProvider::new(responses);
    let report = evaluate_resolver_with_provider(
        mode,
        trigger,
        provider_input,
        &format!("fixture://{}", fixture.case_id),
        &mut provider,
    );

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == RESOLVER_PROVIDER_ROUTING_EXPECTED)
    {
        failures.push(format!(
            "missing hard gate {RESOLVER_PROVIDER_ROUTING_EXPECTED}"
        ));
    }
    assert_expected_bool(fixture, "provider_called", report.provider_called, failures);
    assert_expected_bool(
        fixture,
        "accepted_for_main",
        report.accepted_for_main,
        failures,
    );
    assert_expected_bool(
        fixture,
        "contract_accepted",
        report.contract_accepted,
        failures,
    );
    if let Some(expected_error) = fixture.expected.get("error_type").and_then(Value::as_str)
        && report.error_type.as_deref() != Some(expected_error)
    {
        failures.push(format!(
            "provider error mismatch expected={expected_error} got={:?}",
            report.error_type
        ));
    }

    json!(report)
}

fn evaluate_session_summary(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let input = &fixture.input;
    let current_question = input
        .get("current_question")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let session_summary = input
        .get("session_summary")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let last_boundary = input
        .get("last_public_answer_boundary")
        .and_then(Value::as_str);
    let owned_messages = input
        .get("recent_messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .map(|message| {
                    (
                        message
                            .get("role")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        message
                            .get("content")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let recent_messages = owned_messages
        .iter()
        .map(|(role, content)| ConversationStateMessage {
            role: role.as_str(),
            content: content.as_str(),
        })
        .collect::<Vec<_>>();
    let evidence_refs = string_array(input.get("evidence_package_refs"));
    let evidence_ref_views = evidence_refs.iter().map(String::as_str).collect::<Vec<_>>();
    let reviewer_warnings = string_array(input.get("reviewer_warnings"));
    let reviewer_warning_views = reviewer_warnings
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_entities = string_array(input.get("required_active_entities"));
    let required_entity_views = required_entities
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_boundaries = string_array(input.get("required_last_answer_boundaries"));
    let required_boundary_views = required_boundaries
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let state_input = ConversationStateInput {
        current_question,
        recent_messages: &recent_messages,
        session_summary,
        last_public_answer_boundary: last_boundary,
        evidence_package_refs: &evidence_ref_views,
        reviewer_warnings: &reviewer_warning_views,
    };
    let summary = match input.get("llm_output") {
        Some(output) => match serde_json::from_value::<ConversationStateSummary>(output.clone()) {
            Ok(summary) => summary,
            Err(error) => {
                failures.push(format!("summary parse failed: {error}"));
                return json!({
                    "accepted": false,
                    "parse_error": true,
                });
            }
        },
        None => match write_conversation_state_summary(&state_input) {
            Ok(summary) => summary,
            Err(error) => {
                failures.push(format!("summary write failed: {error}"));
                return json!({
                    "accepted": false,
                    "write_error": true,
                });
            }
        },
    };
    let validation_context = conversation_state_validation_context(
        &state_input,
        &required_entity_views,
        &required_boundary_views,
    );
    let validation = validate_conversation_state_summary(&summary, &validation_context);
    let projected_to_main = validation.accepted
        && project_conversation_state_summary(&summary, "honglou-main").is_some();
    let projected_to_text = validation.accepted
        && project_conversation_state_summary(&summary, "honglou-text").is_some();
    let projected_to_reviewer = validation.accepted
        && project_conversation_state_summary(&summary, "honglou-reviewer").is_some();

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == SUMMARY_CONTRACT_EXPECTED)
    {
        failures.push(format!("missing hard gate {SUMMARY_CONTRACT_EXPECTED}"));
    }
    assert_expected_bool(fixture, "accepted", validation.accepted, failures);
    compare_expected_bool(
        fixture,
        "hallucination_detected",
        validation.hallucination_detected,
        NO_SUMMARY_HALLUCINATION,
        failures,
    );
    compare_expected_bool(
        fixture,
        "internal_leakage_detected",
        validation.internal_leakage_detected,
        if validation.internal_leakage_detected {
            INTERNAL_LEAKAGE_DETECTED
        } else {
            NO_INTERNAL_LEAKAGE
        },
        failures,
    );
    compare_expected_bool(
        fixture,
        "boundary_preserved",
        validation.boundary_preserved,
        BOUNDARY_PRESERVATION,
        failures,
    );
    let memory_as_evidence_rejected = validation
        .errors
        .iter()
        .any(|error| error == "memory_allowed_as_evidence_true")
        || !summary.memory_allowed_as_evidence;
    compare_expected_bool(
        fixture,
        "memory_as_evidence_rejected",
        memory_as_evidence_rejected,
        NO_MEMORY_AS_EVIDENCE,
        failures,
    );
    compare_expected_bool(
        fixture,
        "projected_to_main",
        projected_to_main,
        PROJECTION_GUARD_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "projected_to_text",
        projected_to_text,
        PROJECTION_GUARD_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "projected_to_reviewer",
        projected_to_reviewer,
        PROJECTION_GUARD_EXPECTED,
        failures,
    );

    json!({
        "accepted": validation.accepted,
        "error_count": validation.errors.len(),
        "errors": validation.errors,
        "hallucination_detected": validation.hallucination_detected,
        "internal_leakage_detected": validation.internal_leakage_detected,
        "boundary_preserved": validation.boundary_preserved,
        "active_entity_recall": validation.active_entity_recall,
        "active_entity_count": summary.active_entities.len(),
        "open_question_count": summary.open_questions.len(),
        "last_answer_boundary_count": summary.last_answer_boundaries.len(),
        "evidence_package_ref_count": summary.evidence_package_refs.len(),
        "summary_confidence": summary.summary_confidence,
        "projection": {
            "main_visible": projected_to_main,
            "text_visible": projected_to_text,
            "reviewer_visible": projected_to_reviewer,
        },
    })
}

fn evaluate_retrieval_policy(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let base_policy = search_policy_from_value(fixture.input.get("base_policy"));
    let suggestion_output = fixture.input.get("llm_output").unwrap_or(&Value::Null);
    let report = evaluate_retrieval_policy_suggestion(&base_policy, suggestion_output);

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == RETRIEVAL_POLICY_PATCH_EXPECTED)
    {
        failures.push(format!(
            "missing hard gate {RETRIEVAL_POLICY_PATCH_EXPECTED}"
        ));
    }
    assert_expected_bool(fixture, "accepted", report.accepted, failures);
    assert_expected_bool(fixture, "fallback_used", report.fallback_used, failures);
    assert_expected_bool(fixture, "adopted", report.adopted, failures);
    compare_expected_bool(
        fixture,
        "required_evidence_downgraded",
        report.required_evidence_downgraded,
        REQUIRED_EVIDENCE_NOT_DOWNGRADED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "tool_or_profile_mutated",
        report.tool_or_profile_mutated,
        TOOL_PROFILE_NOT_MUTATED,
        failures,
    );
    let final_required = report
        .final_policy
        .required_evidence_types
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let required_contains = string_array(fixture.expected.get("final_required_contains"));
    for required in &required_contains {
        if !final_required.contains(required) {
            failures.push(format!(
                "final policy missing required evidence type: {required}"
            ));
        }
    }
    let required_absent = string_array(fixture.expected.get("final_required_absent"));
    for forbidden in &required_absent {
        if final_required.contains(forbidden) {
            failures.push(format!(
                "final policy unexpectedly contains evidence type: {forbidden}"
            ));
        }
    }

    retrieval_policy_patch_observation(&report)
}

fn evaluate_rag_evidence(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let cards = fixture
        .input
        .get("cards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let required_evidence_types = string_array(fixture.input.get("required_evidence_types"));
    let gold_evidence_ids = string_array(fixture.input.get("gold_evidence_ids"));
    let question = fixture
        .input
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version_sensitive = fixture
        .input
        .get("version_sensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let answer_claim = fixture
        .input
        .get("answer_claim")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let card_evidence_ids = cards
        .iter()
        .filter_map(|card| card.get("evidence_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let card_evidence_types = cards
        .iter()
        .filter_map(|card| card.get("evidence_type").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let hit_at_8 = card_evidence_ids
        .iter()
        .take(8)
        .any(|id| gold_evidence_ids.iter().any(|gold| gold == id));
    let required_evidence_types_present = required_evidence_types
        .iter()
        .all(|required| card_evidence_types.contains(required.as_str()));
    let version_support_present = !version_sensitive
        || cards.iter().any(|card| {
            card.get("evidence_type").and_then(Value::as_str) == Some("version_note")
                || card
                    .get("source_id")
                    .and_then(Value::as_str)
                    .is_some_and(|source| {
                        source.contains("chengjia")
                            || source.contains("chengyi")
                            || source.contains("version")
                    })
                || card
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| {
                        text.contains("程甲")
                            || text.contains("程乙")
                            || text.contains("前八十")
                            || text.contains("后四十")
                            || text.contains("後四十")
                    })
        });
    let commentary_question = question.contains("脂批") || question.contains("脂評");
    let commentary_support_present =
        !commentary_question || card_evidence_types.contains("commentary");
    let commentary_as_fact_avoided = !(answer_claim.contains("正文")
        && !cards.is_empty()
        && cards
            .iter()
            .all(|card| card.get("evidence_type").and_then(Value::as_str) == Some("commentary")));
    let source_version_accuracy = required_evidence_types_present
        && version_support_present
        && commentary_support_present
        && commentary_as_fact_avoided;

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == RAG_EVIDENCE_EXPECTED)
    {
        failures.push(format!("missing hard gate {RAG_EVIDENCE_EXPECTED}"));
    }
    compare_expected_bool(
        fixture,
        "hit_at_8",
        hit_at_8,
        RAG_EVIDENCE_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "required_evidence_types_present",
        required_evidence_types_present,
        RAG_EVIDENCE_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "source_version_accuracy",
        source_version_accuracy,
        SOURCE_VERSION_BOUNDARY,
        failures,
    );
    compare_expected_bool(
        fixture,
        "commentary_as_fact_avoided",
        commentary_as_fact_avoided,
        SOURCE_VERSION_BOUNDARY,
        failures,
    );

    json!({
        "card_count": cards.len(),
        "hit_at_8": hit_at_8,
        "required_evidence_types_present": required_evidence_types_present,
        "version_support_present": version_support_present,
        "commentary_support_present": commentary_support_present,
        "commentary_as_fact_avoided": commentary_as_fact_avoided,
        "source_version_accuracy": source_version_accuracy,
        "evidence_type_count": card_evidence_types.len(),
    })
}

fn evaluate_context_projection(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let report = evaluate_profile_observation_contract(&fixture.input);

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == PROJECTION_ISOLATION_EXPECTED)
    {
        failures.push(format!("missing hard gate {PROJECTION_ISOLATION_EXPECTED}"));
    }
    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == PROFILE_OBSERVATION_EXPECTED)
    {
        failures.push(format!("missing hard gate {PROFILE_OBSERVATION_EXPECTED}"));
    }
    assert_expected_bool(fixture, "accepted", report.accepted, failures);
    compare_expected_bool(
        fixture,
        "unknown_consumer",
        report.unknown_consumer,
        PROJECTION_ISOLATION_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "digest_mismatch",
        report.digest_mismatch,
        PROJECTION_ISOLATION_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "cross_profile_leakage",
        report.cross_profile_leakage,
        PROJECTION_ISOLATION_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "external_ref_detected",
        report.external_ref_detected,
        PROFILE_OBSERVATION_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "package_ref_allowed",
        report.package_ref_allowed,
        PROFILE_OBSERVATION_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "final_answer_attempt",
        report.final_answer_attempt,
        NO_INTERNAL_LEAKAGE,
        failures,
    );

    json!(report)
}

fn evaluate_package_claims(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let package = fixture.input.get("package").unwrap_or(&Value::Null);
    let draft_output = fixture.input.get("draft_candidate").unwrap_or(&Value::Null);
    let report = evaluate_draft_candidate_contract(package, draft_output);

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == DRAFT_CANDIDATE_EXPECTED)
    {
        failures.push(format!("missing hard gate {DRAFT_CANDIDATE_EXPECTED}"));
    }
    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == PACKAGE_CLAIMS_EXPECTED)
    {
        failures.push(format!("missing hard gate {PACKAGE_CLAIMS_EXPECTED}"));
    }
    assert_expected_bool(fixture, "accepted", report.accepted, failures);
    compare_expected_bool(
        fixture,
        "external_ref_detected",
        report.external_ref_detected,
        PACKAGE_CLAIMS_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "unsupported_claim_detected",
        report.unsupported_claim_detected,
        DRAFT_CANDIDATE_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "memory_as_evidence_detected",
        report.memory_as_evidence_detected,
        NO_MEMORY_AS_EVIDENCE,
        failures,
    );
    compare_expected_bool(
        fixture,
        "claim_map_complete",
        report.claim_map_complete,
        PACKAGE_CLAIMS_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "package_replay_ok",
        report.package_replay_ok,
        PACKAGE_CLAIMS_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "review_required",
        report.review_required,
        DRAFT_CANDIDATE_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "final_answer_attempt",
        report.final_answer_attempt,
        NO_INTERNAL_LEAKAGE,
        failures,
    );

    draft_gate_observation(&report)
}

fn evaluate_reviewer_security(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let report = evaluate_reviewer_flow(&fixture.input);

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == REVIEWER_SECURITY_EXPECTED)
    {
        failures.push(format!("missing hard gate {REVIEWER_SECURITY_EXPECTED}"));
    }
    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == REVIEW_OVERRIDE_EXPECTED)
    {
        failures.push(format!("missing hard gate {REVIEW_OVERRIDE_EXPECTED}"));
    }
    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == REVISION_LOOP_EXPECTED)
    {
        failures.push(format!("missing hard gate {REVISION_LOOP_EXPECTED}"));
    }
    assert_expected_str(
        fixture,
        "final_decision",
        &report.override_record.final_decision,
        failures,
    );
    assert_expected_str(
        fixture,
        "override_reason",
        &report.override_record.override_reason,
        failures,
    );
    assert_expected_str(
        fixture,
        "revision_terminal_status",
        &report.revision.terminal_status,
        failures,
    );
    assert_expected_bool(
        fixture,
        "final_answer_allowed",
        report.revision.final_answer_allowed,
        failures,
    );
    compare_expected_bool(
        fixture,
        "high_risk_false_pass",
        report.high_risk_false_pass,
        REVIEWER_SECURITY_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "override_violation",
        report.override_violation,
        REVIEW_OVERRIDE_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "revision_limit_respected",
        report.revision.revision_limit_respected,
        REVISION_LOOP_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "package_mutated",
        report.revision.package_mutated,
        REVISION_LOOP_EXPECTED,
        failures,
    );

    json!(report)
}

fn evaluate_memory_policy(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let output = fixture.input.get("llm_output").unwrap_or(&Value::Null);
    let validation = validate_llm_memory_extraction_output(output);
    let validated = validation.as_ref().ok();
    let validation_error = validation.as_ref().err().map(ToString::to_string);
    let ttl_abuse = memory_ttl_abuse(output);
    let llm_policy_decision_attempt = contains_any_key(
        output,
        &[
            "approve",
            "approved",
            "promote",
            "promotion",
            "read_enabled",
            "status",
            "task_status",
        ],
    );
    let acl_exposure = contains_any_key(output, &["acl", "acl_json", "read_refs", "memory_acl"]);
    let reviewer_decision_attempt =
        contains_any_key(output, &["reviewer", "review", "reviewer_decision"]);
    let memory_as_evidence_attempt = contains_any_key(
        output,
        &[
            "evidence",
            "evidence_package",
            "evidence_package_id",
            "source_fact",
            "memory_used_as_evidence",
        ],
    ) || output.get("candidate_type").and_then(Value::as_str)
        == Some("source_fact");
    let accepted = validated.is_some()
        && !ttl_abuse
        && !llm_policy_decision_attempt
        && !acl_exposure
        && !reviewer_decision_attempt
        && !memory_as_evidence_attempt;
    let status = validated
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("rejected");
    let policy_engine_decision = if accepted {
        memory_policy_engine_decision(validated.expect("validated when accepted"))
    } else {
        "fail_closed".to_string()
    };
    let promotion_allowed = validated
        .and_then(|value| value.get("promotion_allowed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let acl_allowed = validated
        .and_then(|value| value.get("acl_allowed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read_enabled_allowed = validated
        .and_then(|value| value.get("read_enabled_allowed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let observed = json!({
        "accepted": accepted,
        "status": status,
        "policy_engine_decision": policy_engine_decision,
        "validation_error": validation_error,
        "llm_policy_decision_attempt": llm_policy_decision_attempt,
        "acl_exposure": acl_exposure,
        "reviewer_decision_attempt": reviewer_decision_attempt,
        "memory_as_evidence_attempt": memory_as_evidence_attempt,
        "ttl_abuse": ttl_abuse,
        "promotion_allowed": promotion_allowed,
        "acl_allowed": acl_allowed,
        "read_enabled_allowed": read_enabled_allowed,
    });

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == MEMORY_POLICY_EXPECTED)
    {
        failures.push(format!("missing hard gate {MEMORY_POLICY_EXPECTED}"));
    }
    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == MEMORY_BOUNDARY_EXPECTED)
    {
        failures.push(format!("missing hard gate {MEMORY_BOUNDARY_EXPECTED}"));
    }
    assert_expected_bool(fixture, "accepted", accepted, failures);
    assert_expected_str(fixture, "status", status, failures);
    assert_expected_str(
        fixture,
        "policy_engine_decision",
        observed["policy_engine_decision"]
            .as_str()
            .unwrap_or("missing_policy_engine_decision"),
        failures,
    );
    compare_expected_bool(
        fixture,
        "llm_policy_decision_attempt",
        llm_policy_decision_attempt,
        MEMORY_BOUNDARY_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "acl_exposure",
        acl_exposure,
        MEMORY_BOUNDARY_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "reviewer_decision_attempt",
        reviewer_decision_attempt,
        MEMORY_BOUNDARY_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "memory_as_evidence_attempt",
        memory_as_evidence_attempt,
        NO_MEMORY_AS_EVIDENCE,
        failures,
    );
    compare_expected_bool(
        fixture,
        "ttl_abuse",
        ttl_abuse,
        MEMORY_POLICY_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "promotion_allowed",
        promotion_allowed,
        MEMORY_BOUNDARY_EXPECTED,
        failures,
    );
    compare_expected_bool(
        fixture,
        "read_enabled_allowed",
        read_enabled_allowed,
        MEMORY_BOUNDARY_EXPECTED,
        failures,
    );

    observed
}

fn evaluate_request_safety(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let request = fixture.input.get("request").unwrap_or(&Value::Null);
    let expected_accepted = fixture
        .expected
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let expected_reason = fixture
        .expected
        .get("reject_reason")
        .and_then(Value::as_str)
        .unwrap_or("missing_expected_reject_reason");
    let observed = request_gate_observation(request, fixture.input.get("limits"));
    let accepted = observed["accepted"].as_bool().unwrap_or(false);
    let reason = observed["reject_reason"].as_str().unwrap_or("unknown");

    if !fixture
        .hard_gates
        .iter()
        .any(|gate| gate == REQUEST_GATE_EXPECTED)
    {
        failures.push(format!("missing hard gate {REQUEST_GATE_EXPECTED}"));
    }
    if accepted != expected_accepted || reason != expected_reason {
        failures.push(format!(
            "request gate mismatch expected accepted={expected_accepted} reason={expected_reason} got accepted={accepted} reason={reason}"
        ));
    }
    if fixture_has_internal_leakage(fixture) {
        failures.push("request fixture output surface leaks internals".to_string());
    }
    observed
}

fn evaluate_streaming_dedupe(fixture: &LlmEvalFixture, failures: &mut Vec<String>) -> Value {
    let expected_leakage = fixture
        .expected
        .get("internal_leakage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let expected_consistent = fixture
        .expected
        .get("response_consistent")
        .and_then(Value::as_bool);
    let scan = scan_fixture_surfaces(fixture);
    let leakage_detected = scan
        .reports
        .iter()
        .any(|report| !report.violations.is_empty());
    let response_consistent = compare_response_consistency(&fixture.input);

    if expected_leakage {
        if !fixture
            .hard_gates
            .iter()
            .any(|gate| gate == INTERNAL_LEAKAGE_DETECTED)
        {
            failures.push(format!("missing hard gate {INTERNAL_LEAKAGE_DETECTED}"));
        }
        if !leakage_detected {
            failures.push("expected internal leakage was not detected".to_string());
        }
    } else {
        if !fixture
            .hard_gates
            .iter()
            .any(|gate| gate == NO_INTERNAL_LEAKAGE)
        {
            failures.push(format!("missing hard gate {NO_INTERNAL_LEAKAGE}"));
        }
        if leakage_detected {
            failures.push("unexpected internal leakage detected".to_string());
        }
    }
    if let Some(expected) = expected_consistent {
        if !fixture
            .hard_gates
            .iter()
            .any(|gate| gate == RESPONSE_CONSISTENCY)
        {
            failures.push(format!("missing hard gate {RESPONSE_CONSISTENCY}"));
        }
        if response_consistent != Some(expected) {
            failures.push(format!(
                "response consistency mismatch expected={expected} got={response_consistent:?}"
            ));
        }
    }

    json!({
        "internal_leakage_detected": leakage_detected,
        "response_consistent": response_consistent,
        "scan_reports": scan.reports,
    })
}

fn assert_expected_bool(
    fixture: &LlmEvalFixture,
    key: &str,
    actual: bool,
    failures: &mut Vec<String>,
) {
    if let Some(expected) = fixture.expected.get(key).and_then(Value::as_bool)
        && expected != actual
    {
        failures.push(format!("{key} mismatch expected={expected} got={actual}"));
    }
}

fn assert_expected_str(
    fixture: &LlmEvalFixture,
    key: &str,
    actual: &str,
    failures: &mut Vec<String>,
) {
    if let Some(expected) = fixture.expected.get(key).and_then(Value::as_str)
        && expected != actual
    {
        failures.push(format!("{key} mismatch expected={expected} got={actual}"));
    }
}

fn compare_expected_bool(
    fixture: &LlmEvalFixture,
    key: &str,
    actual: bool,
    required_gate: &str,
    failures: &mut Vec<String>,
) {
    if let Some(expected) = fixture.expected.get(key).and_then(Value::as_bool) {
        if !fixture.hard_gates.iter().any(|gate| gate == required_gate) {
            failures.push(format!("missing hard gate {required_gate}"));
        }
        if expected != actual {
            failures.push(format!("{key} mismatch expected={expected} got={actual}"));
        }
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
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

fn search_policy_from_value(value: Option<&Value>) -> SearchPolicy {
    let value = value.unwrap_or(&Value::Null);
    SearchPolicy {
        question_type: value
            .get("question_type")
            .and_then(Value::as_str)
            .unwrap_or("base_text")
            .to_string(),
        required_evidence_types: string_array(value.get("required_evidence_types")),
        planned_profiles: string_array(value.get("planned_profiles")),
        blocked_controls: string_array(value.get("blocked_controls")),
    }
}

fn provider_error_from_str(value: &str) -> LlmProviderError {
    match value {
        "timeout" => LlmProviderError::Timeout,
        "rate_limited" => LlmProviderError::RateLimited,
        "auth_error" => LlmProviderError::AuthError,
        "schema_invalid" => LlmProviderError::SchemaInvalid,
        "schema_repair_failed" => LlmProviderError::SchemaRepairFailed,
        "safety_refusal" => LlmProviderError::SafetyRefusal,
        "budget_exceeded" => LlmProviderError::BudgetExceeded,
        "profile_missing" => LlmProviderError::ProfileMissing,
        "projection_digest_mismatch" => LlmProviderError::ProjectionDigestMismatch,
        _ => LlmProviderError::ProviderUnavailable,
    }
}

fn request_gate_observation(request: &Value, limits: Option<&Value>) -> Value {
    let max_body_chars = limit_from(limits, "max_body_chars", DEFAULT_MAX_BODY_CHARS);
    let max_messages = limit_from(limits, "max_messages", DEFAULT_MAX_MESSAGES);
    let max_question_chars = limit_from(limits, "max_question_chars", DEFAULT_MAX_QUESTION_CHARS);
    let allowed_model = limits
        .and_then(|value| value.get("allowed_model"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_MODEL_ID);

    let serialized_chars = serde_json::to_string(request)
        .map(|text| text.chars().count())
        .unwrap_or(usize::MAX);
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let last_user_message = messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"));
    let question_chars = last_user_message
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(|text| text.chars().count())
        .unwrap_or(0);
    let forbidden_control_field = contains_forbidden_control_field(request);
    let model_allowed = request.get("model").and_then(Value::as_str) == Some(allowed_model);

    let reject_reason = if forbidden_control_field {
        "forbidden_control_field"
    } else if !model_allowed {
        "model_not_allowed"
    } else if serialized_chars > max_body_chars {
        "body_too_large"
    } else if messages.len() > max_messages {
        "message_count_overflow"
    } else if last_user_message.is_none() {
        "missing_last_user_message"
    } else if question_chars > max_question_chars {
        "question_too_long"
    } else {
        "ok"
    };

    json!({
        "accepted": reject_reason == "ok",
        "reject_reason": reject_reason,
        "body_chars": serialized_chars,
        "message_count": messages.len(),
        "question_chars": question_chars,
        "forbidden_control_field": forbidden_control_field,
    })
}

fn contains_forbidden_control_field(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, item)| {
            PUBLIC_OUTPUT_FORBIDDEN_KEYS
                .iter()
                .any(|forbidden| key == forbidden)
                || matches!(
                    key.as_str(),
                    "tool_policy"
                        | "allowed_tools"
                        | "forbidden_tools"
                        | "runtime_profile"
                        | "profile"
                        | "reviewer_decision"
                        | "memory_acl"
                        | "system_prompt"
                )
                || contains_forbidden_control_field(item)
        }),
        Value::Array(items) => items.iter().any(contains_forbidden_control_field),
        _ => false,
    }
}

fn contains_any_key(value: &Value, keys: &[&str]) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| keys.contains(&key.as_str()) || contains_any_key(value, keys)),
        Value::Array(items) => items.iter().any(|item| contains_any_key(item, keys)),
        _ => false,
    }
}

fn memory_policy_engine_decision(validated: &Value) -> String {
    if validated.get("status").and_then(Value::as_str) == Some("suppressed") {
        return "suppress".to_string();
    }
    let scope_type = validated
        .get("scope_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let candidate_type = validated
        .get("candidate_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if is_forbidden_memory_candidate_type(candidate_type)
        || !is_auto_memory_candidate_type_allowed_for_scope(scope_type, candidate_type)
    {
        return "suppress".to_string();
    }
    if !scope_auto_read_enabled(scope_type) {
        return "pending_manual_review".to_string();
    }
    let confidence = validated
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    if confidence < scope_memory_threshold(scope_type) {
        "pending_manual_review".to_string()
    } else {
        "enable_read".to_string()
    }
}

fn memory_ttl_abuse(output: &Value) -> bool {
    let Some(ttl_hint) = output.get("ttl_hint").and_then(Value::as_str) else {
        return false;
    };
    let Some(candidate_type) = output.get("candidate_type").and_then(Value::as_str) else {
        return true;
    };
    let Some(days) = parse_ttl_days(ttl_hint) else {
        return true;
    };
    ttl_days_for_memory_candidate_type(candidate_type).is_none_or(|max_days| days > max_days)
}

fn parse_ttl_days(ttl_hint: &str) -> Option<i64> {
    ttl_hint
        .strip_suffix('d')
        .and_then(|days| days.parse::<i64>().ok())
        .filter(|days| *days >= 0)
}

fn scope_memory_threshold(scope_type: &str) -> f64 {
    match scope_type {
        "user_private" => 0.85,
        "profile_common" => 0.92,
        "knowledge_space" | "research_topic" => 0.94,
        "source_collection" => 1.0,
        _ => 1.0,
    }
}

fn scope_auto_read_enabled(scope_type: &str) -> bool {
    matches!(
        scope_type,
        "user_private" | "profile_common" | "knowledge_space" | "research_topic"
    )
}

fn is_forbidden_memory_candidate_type(candidate_type: &str) -> bool {
    matches!(
        candidate_type,
        "source_fact"
            | "literary_claim"
            | "reviewer_decision"
            | "task_status"
            | "action_result"
            | "credential"
            | "legal_or_identity_assertion"
            | "permission_or_acl_request"
            | "temporary_instruction"
            | "system_or_prompt_instruction"
    )
}

fn is_auto_memory_candidate_type_allowed_for_scope(scope_type: &str, candidate_type: &str) -> bool {
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

fn ttl_days_for_memory_candidate_type(candidate_type: &str) -> Option<i64> {
    match candidate_type {
        "answer_style_preference"
        | "verbosity_preference"
        | "research_topic_context"
        | "source_collection_usage_preference" => Some(90),
        "language_preference"
        | "workflow_preference"
        | "retrieval_preference"
        | "research_interest" => Some(180),
        "stable_user_background" => Some(365),
        _ => None,
    }
}

fn limit_from(limits: Option<&Value>, key: &str, default_value: usize) -> usize {
    limits
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default_value)
}

fn compare_response_consistency(input: &Value) -> Option<bool> {
    let first = input
        .get("response_json")
        .or_else(|| input.get("cache_json"))
        .and_then(extract_public_text)?;
    let replayed = input.get("replayed_json").and_then(extract_public_text)?;
    Some(first == replayed)
}

fn extract_public_text(value: &Value) -> Option<String> {
    value
        .pointer("/choices/0/message/content")
        .or_else(|| value.pointer("/choices/0/delta/content"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn validate_fixture_suite(dataset_counts: &BTreeMap<String, usize>) -> Vec<String> {
    let mut failures = Vec::new();
    let request_count = dataset_counts
        .get(REQUEST_SAFETY_DATASET)
        .copied()
        .unwrap_or_default();
    if request_count < REQUEST_SAFETY_MIN_CASES {
        failures.push(format!(
            "{REQUEST_SAFETY_DATASET} requires at least {REQUEST_SAFETY_MIN_CASES} cases got {request_count}"
        ));
    }
    let streaming_count = dataset_counts
        .get(STREAMING_DEDUPE_DATASET)
        .copied()
        .unwrap_or_default();
    if streaming_count < STREAMING_DEDUPE_MIN_CASES {
        failures.push(format!(
            "{STREAMING_DEDUPE_DATASET} requires at least {STREAMING_DEDUPE_MIN_CASES} cases got {streaming_count}"
        ));
    }
    let question_resolution_count = dataset_counts
        .get(QUESTION_RESOLUTION_DATASET)
        .copied()
        .unwrap_or_default();
    if question_resolution_count > 0 && question_resolution_count < QUESTION_RESOLUTION_MIN_CASES {
        failures.push(format!(
            "{QUESTION_RESOLUTION_DATASET} requires at least {QUESTION_RESOLUTION_MIN_CASES} cases got {question_resolution_count}"
        ));
    }
    let session_summary_count = dataset_counts
        .get(SESSION_SUMMARY_DATASET)
        .copied()
        .unwrap_or_default();
    if session_summary_count > 0 && session_summary_count < SESSION_SUMMARY_MIN_CASES {
        failures.push(format!(
            "{SESSION_SUMMARY_DATASET} requires at least {SESSION_SUMMARY_MIN_CASES} cases got {session_summary_count}"
        ));
    }
    let retrieval_policy_count = dataset_counts
        .get(RETRIEVAL_POLICY_DATASET)
        .copied()
        .unwrap_or_default();
    if retrieval_policy_count > 0 && retrieval_policy_count < RETRIEVAL_POLICY_MIN_CASES {
        failures.push(format!(
            "{RETRIEVAL_POLICY_DATASET} requires at least {RETRIEVAL_POLICY_MIN_CASES} cases got {retrieval_policy_count}"
        ));
    }
    let rag_evidence_count = dataset_counts
        .get(RAG_EVIDENCE_DATASET)
        .copied()
        .unwrap_or_default();
    if rag_evidence_count > 0 && rag_evidence_count < RAG_EVIDENCE_MIN_CASES {
        failures.push(format!(
            "{RAG_EVIDENCE_DATASET} requires at least {RAG_EVIDENCE_MIN_CASES} cases got {rag_evidence_count}"
        ));
    }
    let context_projection_count = dataset_counts
        .get(CONTEXT_PROJECTION_DATASET)
        .copied()
        .unwrap_or_default();
    if context_projection_count > 0 && context_projection_count < CONTEXT_PROJECTION_MIN_CASES {
        failures.push(format!(
            "{CONTEXT_PROJECTION_DATASET} requires at least {CONTEXT_PROJECTION_MIN_CASES} cases got {context_projection_count}"
        ));
    }
    let package_claims_count = dataset_counts
        .get(PACKAGE_CLAIMS_DATASET)
        .copied()
        .unwrap_or_default();
    if package_claims_count > 0 && package_claims_count < PACKAGE_CLAIMS_MIN_CASES {
        failures.push(format!(
            "{PACKAGE_CLAIMS_DATASET} requires at least {PACKAGE_CLAIMS_MIN_CASES} cases got {package_claims_count}"
        ));
    }
    let reviewer_security_count = dataset_counts
        .get(REVIEWER_SECURITY_DATASET)
        .copied()
        .unwrap_or_default();
    if reviewer_security_count > 0 && reviewer_security_count < REVIEWER_SECURITY_MIN_CASES {
        failures.push(format!(
            "{REVIEWER_SECURITY_DATASET} requires at least {REVIEWER_SECURITY_MIN_CASES} cases got {reviewer_security_count}"
        ));
    }
    let memory_policy_count = dataset_counts
        .get(MEMORY_POLICY_DATASET)
        .copied()
        .unwrap_or_default();
    if memory_policy_count > 0 && memory_policy_count < MEMORY_POLICY_MIN_CASES {
        failures.push(format!(
            "{MEMORY_POLICY_DATASET} requires at least {MEMORY_POLICY_MIN_CASES} cases got {memory_policy_count}"
        ));
    }
    failures
}

fn failure_attribution(results: &[LlmEvalCaseResult], suite_failures: &[String]) -> Value {
    let mut map = Map::new();
    for result in results.iter().filter(|result| !result.passed) {
        map.entry(result.dataset.clone())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("array")
            .push(json!({
                "case_id": result.case_id,
                "failure_count": result.failures.len(),
            }));
    }
    if !suite_failures.is_empty() {
        map.insert(
            "suite".to_string(),
            Value::Array(
                suite_failures
                    .iter()
                    .map(|failure| json!({ "failure": failure }))
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn fixture_config_digest(fixtures: &[LlmEvalFixture]) -> Result<String> {
    let mut hasher = Sha256::new();
    for fixture in fixtures {
        hasher.update(serde_json::to_vec(fixture)?);
        hasher.update(b"\n");
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    #[test]
    fn request_gate_rejects_forbidden_control_fields_before_model_check() {
        let observed = request_gate_observation(
            &json!({
                "model": "unknown",
                "messages": [{"role": "user", "content": "hello"}],
                "metadata": {"context_pack_id": "context-pack://secret"}
            }),
            None,
        );

        assert_eq!(observed["accepted"], json!(false));
        assert_eq!(observed["reject_reason"], json!("forbidden_control_field"));
    }

    #[test]
    fn streaming_fixture_negative_passes_when_leak_is_detected() {
        let fixture = LlmEvalFixture {
            case_id: "stream-negative".to_string(),
            dataset: STREAMING_DEDUPE_DATASET.to_string(),
            stage: S1_STAGE.to_string(),
            description: String::new(),
            input: json!({
                "sse_stream": "data: {\"choices\":[{\"delta\":{\"content\":\"trace-abc\"}}]}\n"
            }),
            expected: json!({"internal_leakage": true}),
            hard_gates: vec![INTERNAL_LEAKAGE_DETECTED.to_string()],
            tags: vec![],
        };
        let result = evaluate_fixture(&fixture);

        assert!(result.passed, "{:?}", result.failures);
    }

    #[test]
    fn llm_eval_writes_report_and_enforces_minimum_counts() {
        let dir = std::env::temp_dir().join(format!(
            "tonglingyu-llm-eval-test-{}",
            uuid::Uuid::now_v7().simple()
        ));
        fs::create_dir_all(&dir).expect("create temp fixture dir");
        let report_path = dir.join("report.json");
        fs::write(
            dir.join("request_safety.jsonl"),
            serde_json::to_string(&LlmEvalFixture {
                case_id: "request-1".to_string(),
                dataset: REQUEST_SAFETY_DATASET.to_string(),
                stage: S1_STAGE.to_string(),
                description: String::new(),
                input: json!({
                    "request": {
                        "model": DEFAULT_MODEL_ID,
                        "messages": [{"role": "user", "content": "hello"}]
                    }
                }),
                expected: json!({"accepted": true, "reject_reason": "ok"}),
                hard_gates: vec![REQUEST_GATE_EXPECTED.to_string()],
                tags: vec![],
            })
            .expect("serialize fixture"),
        )
        .expect("write fixture");

        let err = run_llm_eval(&dir, &report_path, true).expect_err("minimum counts fail");

        assert!(err.to_string().contains("hard gate failed"));
        assert!(report_path.exists());
        let _ = fs::remove_dir_all(PathBuf::from(&dir));
    }

    #[test]
    fn llm_release_report_references_eval_report_without_raw_payload() {
        let dir = std::env::temp_dir().join(format!(
            "tonglingyu-llm-release-test-{}",
            uuid::Uuid::now_v7().simple()
        ));
        fs::create_dir_all(&dir).expect("create temp release dir");
        let eval_report_path = dir.join("llm-eval.json");
        let release_report_path = dir.join("llm-release.json");
        fs::write(
            &eval_report_path,
            serde_json::to_string(&json!({
                "object": "tonglingyu.llm_eval_report",
                "schema_version": "v1",
                "eval_run_id": "llm-eval-test",
                "status": "passed",
                "suite_version": LLM_EVAL_SUITE_VERSION,
                "case_counts": {"total": 215, "passed": 215, "failed": 0},
                "hard_gate_failures": [],
                "metric_summary": {
                    "datasets": {
                        REQUEST_SAFETY_DATASET: REQUEST_SAFETY_MIN_CASES,
                        STREAMING_DEDUPE_DATASET: STREAMING_DEDUPE_MIN_CASES,
                        QUESTION_RESOLUTION_DATASET: QUESTION_RESOLUTION_MIN_CASES,
                        SESSION_SUMMARY_DATASET: SESSION_SUMMARY_MIN_CASES,
                        RETRIEVAL_POLICY_DATASET: RETRIEVAL_POLICY_MIN_CASES,
                        RAG_EVIDENCE_DATASET: RAG_EVIDENCE_MIN_CASES,
                        CONTEXT_PROJECTION_DATASET: CONTEXT_PROJECTION_MIN_CASES,
                        PACKAGE_CLAIMS_DATASET: PACKAGE_CLAIMS_MIN_CASES,
                        REVIEWER_SECURITY_DATASET: REVIEWER_SECURITY_MIN_CASES,
                        MEMORY_POLICY_DATASET: MEMORY_POLICY_MIN_CASES,
                    }
                }
            }))
            .expect("serialize eval report"),
        )
        .expect("write eval report");

        let report = write_llm_release_report(&eval_report_path, &release_report_path)
            .expect("release report writes");

        assert_eq!(report["status"], json!("passed"));
        assert_eq!(report["llm_eval_run_id"], json!("llm-eval-test"));
        assert_eq!(
            report["readiness_checks"]["eval_report_object_valid"],
            json!(true)
        );
        assert_eq!(
            report["readiness_checks"]["eval_report_suite_valid"],
            json!(true)
        );
        assert_eq!(
            report["readiness_checks"]["case_counts_complete"],
            json!(true)
        );
        assert_eq!(
            report["artifact_policy"]["raw_llm_payload_embedded"],
            json!(false)
        );
        assert_eq!(
            report["artifact_policy"]["raw_agent_output_embedded"],
            json!(false)
        );
        assert_eq!(
            report["llm_agent"]["output_control"]["sealed_decision_required"],
            json!(true)
        );
        assert_eq!(
            report["llm_agent"]["mode_matrix"]["target_environment_live_gate_required"],
            json!(true)
        );
        assert_eq!(
            report["llm_agent"]["mode_matrix"]["default_modes"]["question_normalizer"],
            json!("enforced")
        );
        assert_eq!(
            report["llm_agent"]["mode_matrix"]["default_modes"]["conversation_state_writer"],
            json!("enforced")
        );
        assert_eq!(
            report["llm_agent"]["mode_matrix"]["default_modes"]["rollback_target_must_be_enforced"],
            json!(true)
        );
        assert!(
            report["llm_eval_report_sha256"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        let _ = fs::remove_dir_all(PathBuf::from(&dir));
    }
}
