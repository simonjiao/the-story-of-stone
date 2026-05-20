use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{
    DEFAULT_MODEL_ID,
    llm_contracts::{
        DEFAULT_MAX_BODY_CHARS, DEFAULT_MAX_MESSAGES, DEFAULT_MAX_QUESTION_CHARS,
        LLM_EVAL_REPORT_SCHEMA_VERSION, LLM_EVAL_SUITE_VERSION, LlmEvalFixture,
        PUBLIC_OUTPUT_FORBIDDEN_KEYS, REQUEST_SAFETY_DATASET, REQUEST_SAFETY_MIN_CASES, S1_STAGE,
        STREAMING_DEDUPE_DATASET, STREAMING_DEDUPE_MIN_CASES,
    },
    user_response_safety::{fixture_has_internal_leakage, scan_fixture_surfaces},
};

const REQUEST_GATE_EXPECTED: &str = "request_gate_expected";
const NO_INTERNAL_LEAKAGE: &str = "no_internal_leakage";
const INTERNAL_LEAKAGE_DETECTED: &str = "internal_leakage_detected";
const RESPONSE_CONSISTENCY: &str = "response_consistency";

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

    let suite_failures = validate_s1_suite(&dataset_counts);
    for failure in &suite_failures {
        hard_gate_failures.push(json!({
            "case_id": "suite",
            "dataset": "suite",
            "hard_gates": ["s1_fixture_contract"],
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
    if fixture.stage != S1_STAGE {
        failures.push(format!("expected stage={S1_STAGE} got {}", fixture.stage));
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

fn validate_s1_suite(dataset_counts: &BTreeMap<String, usize>) -> Vec<String> {
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
}
