use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::llm_contracts::{
    LlmEvalFixture, PUBLIC_OUTPUT_FORBIDDEN_KEYS, PUBLIC_OUTPUT_FORBIDDEN_PATTERNS,
    USER_RESPONSE_SAFETY_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafetyViolation {
    pub surface: String,
    pub case_id: String,
    pub denylist_category: String,
    pub location_sha256: String,
    pub value_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyScanReport {
    pub object: String,
    pub schema_version: String,
    pub status: String,
    pub case_id: String,
    pub surface: String,
    pub scanned_units: usize,
    pub violations: Vec<SafetyViolation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyFixtureScan {
    pub case_id: String,
    pub reports: Vec<SafetyScanReport>,
}

impl SafetyScanReport {
    fn new(case_id: &str, surface: &str) -> Self {
        Self {
            object: "tonglingyu.user_response_safety_scan".to_string(),
            schema_version: USER_RESPONSE_SAFETY_SCHEMA_VERSION.to_string(),
            status: "passed".to_string(),
            case_id: case_id.to_string(),
            surface: surface.to_string(),
            scanned_units: 0,
            violations: Vec::new(),
        }
    }

    fn push_violation(&mut self, category: &str, location: &str, value: &str) {
        self.status = "failed".to_string();
        self.violations.push(SafetyViolation {
            surface: self.surface.clone(),
            case_id: self.case_id.clone(),
            denylist_category: category.to_string(),
            location_sha256: sha256_hex(location),
            value_sha256: sha256_hex(value),
        });
    }
}

pub fn scan_json_surface(case_id: &str, surface: &str, value: &Value) -> SafetyScanReport {
    let mut report = SafetyScanReport::new(case_id, surface);
    scan_json_value(&mut report, "$", value);
    report
}

pub fn scan_text_surface(case_id: &str, surface: &str, text: &str) -> SafetyScanReport {
    let mut report = SafetyScanReport::new(case_id, surface);
    report.scanned_units = 1;
    scan_text_value(&mut report, "$", text);
    report
}

pub fn scan_sse_surface(case_id: &str, surface: &str, stream: &str) -> SafetyScanReport {
    let mut report = SafetyScanReport::new(case_id, surface);
    for (index, line) in stream.lines().enumerate() {
        let Some(frame) = line.trim_start().strip_prefix("data:") else {
            continue;
        };
        let frame = frame.trim();
        if frame.is_empty() || frame == "[DONE]" {
            continue;
        }
        report.scanned_units += 1;
        let location = format!("$.sse[{index}]");
        match serde_json::from_str::<Value>(frame) {
            Ok(value) => scan_json_value(&mut report, &location, &value),
            Err(_) => scan_text_value(&mut report, &location, frame),
        }
    }
    report
}

pub fn scan_fixture_surfaces(fixture: &LlmEvalFixture) -> SafetyFixtureScan {
    let mut reports = Vec::new();
    let input = &fixture.input;

    if let Some(value) = input.get("response_json") {
        reports.push(scan_json_surface(
            &fixture.case_id,
            "completion_response",
            value,
        ));
    }
    if let Some(value) = input.get("error_json") {
        reports.push(scan_json_surface(&fixture.case_id, "error_response", value));
    }
    if let Some(value) = input.get("cache_json") {
        reports.push(scan_json_surface(&fixture.case_id, "cache_raw", value));
    }
    if let Some(value) = input.get("replayed_json") {
        reports.push(scan_json_surface(&fixture.case_id, "cache_replay", value));
    }
    if let Some(stream) = input.get("sse_stream").and_then(Value::as_str) {
        reports.push(scan_sse_surface(&fixture.case_id, "sse_stream", stream));
    }
    if let Some(text) = input.get("completion_text").and_then(Value::as_str) {
        reports.push(scan_text_surface(&fixture.case_id, "completion_text", text));
    }

    SafetyFixtureScan {
        case_id: fixture.case_id.clone(),
        reports,
    }
}

pub fn fixture_has_internal_leakage(fixture: &LlmEvalFixture) -> bool {
    scan_fixture_surfaces(fixture)
        .reports
        .iter()
        .any(|report| !report.violations.is_empty())
}

fn scan_json_value(report: &mut SafetyScanReport, path: &str, value: &Value) {
    report.scanned_units += 1;
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let child_path = format!("{path}.{key}");
                if PUBLIC_OUTPUT_FORBIDDEN_KEYS
                    .iter()
                    .any(|forbidden| key == forbidden)
                {
                    report.push_violation("denylist_key", &child_path, key);
                }
                scan_text_value(report, &child_path, key);
                scan_json_value(report, &child_path, item);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                scan_json_value(report, &format!("{path}[{index}]"), item);
            }
        }
        Value::String(text) => scan_text_value(report, path, text),
        Value::Number(number) => scan_text_value(report, path, &number.to_string()),
        Value::Bool(flag) => scan_text_value(report, path, &flag.to_string()),
        Value::Null => {}
    }
}

fn scan_text_value(report: &mut SafetyScanReport, path: &str, text: &str) {
    for pattern in PUBLIC_OUTPUT_FORBIDDEN_PATTERNS {
        if pattern.tokens.iter().any(|token| text.contains(token)) {
            report.push_violation(pattern.category, path, text);
        }
    }
}

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn scanner_detects_recursive_internal_key_without_raw_value() {
        let report = scan_json_surface(
            "case-1",
            "completion_response",
            &json!({
                "choices": [{
                    "message": {
                        "content": "public",
                        "context_pack_id": "context-pack://secret"
                    }
                }]
            }),
        );

        assert_eq!(report.status, "failed");
        assert!(report.violations.iter().any(|item| {
            item.denylist_category == "denylist_key"
                || item.denylist_category == "context_projection"
        }));
        assert!(
            report
                .violations
                .iter()
                .all(|item| item.value_sha256.starts_with("sha256:"))
        );
    }

    #[test]
    fn scanner_checks_each_sse_frame() {
        let report = scan_sse_surface(
            "case-2",
            "sse_stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"package:abc\"}}]}\n\
             data: [DONE]\n",
        );

        assert!(report.scanned_units >= 2);
        assert!(
            report
                .violations
                .iter()
                .any(|item| item.denylist_category == "trace_package")
        );
    }
}
