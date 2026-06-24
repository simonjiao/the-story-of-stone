use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::plan::SearchPolicy;

pub(crate) const RETRIEVAL_POLICY_SUGGESTION_SCHEMA_VERSION: &str =
    "tonglingyu-retrieval-policy-suggestion-v1";

const MIN_SUGGESTION_CONFIDENCE: f64 = 0.5;

const FORBIDDEN_SUGGESTION_FIELDS: &[&str] = &[
    "required_evidence_final",
    "tool_choice",
    "profile",
    "reviewer_state",
    "skip_review",
    "final_answer",
    "allowed_tools",
    "forbidden_tools",
    "evidence_package_id",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetrievalPolicySuggestion {
    pub(crate) schema_version: String,
    pub(crate) question_type: String,
    #[serde(default)]
    pub(crate) alias_expansions: Vec<String>,
    pub(crate) version_sensitive: bool,
    pub(crate) commentary_recommended: bool,
    pub(crate) confidence: f64,
    pub(crate) unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RetrievalPolicyPatchReport {
    pub(crate) accepted: bool,
    pub(crate) fallback_used: bool,
    pub(crate) rejected_reason: Option<String>,
    pub(crate) adopted: bool,
    pub(crate) added_required_evidence_types: Vec<String>,
    pub(crate) required_evidence_downgraded: bool,
    pub(crate) tool_or_profile_mutated: bool,
    pub(crate) final_policy: SearchPolicy,
}

pub(crate) fn evaluate_retrieval_policy_suggestion(
    base_policy: &SearchPolicy,
    suggestion_output: &Value,
) -> RetrievalPolicyPatchReport {
    if let Some(field) = first_forbidden_field(suggestion_output) {
        return fallback_report(base_policy, Some(format!("forbidden_field: {field}")));
    }
    let suggestion =
        match serde_json::from_value::<RetrievalPolicySuggestion>(suggestion_output.clone()) {
            Ok(suggestion) => suggestion,
            Err(error) => {
                return fallback_report(
                    base_policy,
                    Some(format!("schema_invalid_or_unknown_field: {error}")),
                );
            }
        };
    if suggestion.schema_version != RETRIEVAL_POLICY_SUGGESTION_SCHEMA_VERSION {
        return fallback_report(
            base_policy,
            Some(format!(
                "schema_version_mismatch: {}",
                suggestion.schema_version
            )),
        );
    }
    if suggestion.confidence < MIN_SUGGESTION_CONFIDENCE || suggestion.confidence > 1.0 {
        return fallback_report(base_policy, Some("confidence_out_of_range".to_string()));
    }
    if suggestion.unsupported_reason.is_some() {
        return fallback_report(base_policy, Some("unsupported_reason_present".to_string()));
    }

    let mut final_policy = base_policy.clone();
    let before_required = required_set(&base_policy.required_evidence_types);
    let mut required = before_required.clone();
    apply_required_evidence_patch(&suggestion, &mut required);
    final_policy.required_evidence_types = required.iter().cloned().collect();
    let after_required = required_set(&final_policy.required_evidence_types);
    let added_required_evidence_types = after_required
        .difference(&before_required)
        .cloned()
        .collect::<Vec<_>>();
    let required_evidence_downgraded = !before_required.is_subset(&after_required);
    let tool_or_profile_mutated = final_policy.planned_profiles != base_policy.planned_profiles
        || final_policy.blocked_controls != base_policy.blocked_controls;

    RetrievalPolicyPatchReport {
        accepted: !required_evidence_downgraded && !tool_or_profile_mutated,
        fallback_used: false,
        rejected_reason: None,
        adopted: !added_required_evidence_types.is_empty(),
        added_required_evidence_types,
        required_evidence_downgraded,
        tool_or_profile_mutated,
        final_policy,
    }
}

pub(crate) fn retrieval_policy_patch_observation(report: &RetrievalPolicyPatchReport) -> Value {
    json!({
        "accepted": report.accepted,
        "fallback_used": report.fallback_used,
        "rejected_reason": report.rejected_reason,
        "adopted": report.adopted,
        "added_required_evidence_types": &report.added_required_evidence_types,
        "required_evidence_downgraded": report.required_evidence_downgraded,
        "tool_or_profile_mutated": report.tool_or_profile_mutated,
        "final_policy": {
            "question_type": &report.final_policy.question_type,
            "required_evidence_types": &report.final_policy.required_evidence_types,
            "planned_profiles": &report.final_policy.planned_profiles,
            "blocked_controls": &report.final_policy.blocked_controls,
        },
    })
}

fn fallback_report(
    base_policy: &SearchPolicy,
    rejected_reason: Option<String>,
) -> RetrievalPolicyPatchReport {
    RetrievalPolicyPatchReport {
        accepted: false,
        fallback_used: true,
        rejected_reason,
        adopted: false,
        added_required_evidence_types: Vec::new(),
        required_evidence_downgraded: false,
        tool_or_profile_mutated: false,
        final_policy: base_policy.clone(),
    }
}

fn apply_required_evidence_patch(
    suggestion: &RetrievalPolicySuggestion,
    required: &mut BTreeSet<String>,
) {
    match suggestion.question_type.as_str() {
        "character_fate" => {
            required.insert("base_text".to_string());
            required.insert("commentary".to_string());
            required.insert("version_note".to_string());
        }
        "commentary" => {
            required.insert("commentary".to_string());
        }
        "version" => {
            required.insert("version_note".to_string());
        }
        "poem_or_judgement" => {
            required.insert("base_text".to_string());
        }
        "location" | "base_text" | "relationship" => {
            required.insert("base_text".to_string());
        }
        _ => {}
    }
    if suggestion.commentary_recommended {
        required.insert("commentary".to_string());
    }
    if suggestion.version_sensitive {
        required.insert("version_note".to_string());
    }
}

fn required_set(values: &[String]) -> BTreeSet<String> {
    values.iter().cloned().collect()
}

fn first_forbidden_field(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(map) => {
            for key in map.keys() {
                if let Some(field) = FORBIDDEN_SUGGESTION_FIELDS
                    .iter()
                    .copied()
                    .find(|field| key == field)
                {
                    return Some(field);
                }
            }
            map.values().find_map(first_forbidden_field)
        }
        Value::Array(items) => items.iter().find_map(first_forbidden_field),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_policy(required: &[&str]) -> SearchPolicy {
        SearchPolicy {
            question_type: "base_text".to_string(),
            required_evidence_types: required.iter().map(|item| (*item).to_string()).collect(),
            planned_profiles: vec![
                "honglou-text".to_string(),
                "honglou-main".to_string(),
                "honglou-reviewer".to_string(),
            ],
            blocked_controls: Vec::new(),
        }
    }

    #[test]
    fn patch_never_downgrades_base_required_evidence() {
        let policy = base_policy(&["base_text"]);
        let report = evaluate_retrieval_policy_suggestion(
            &policy,
            &json!({
                "schema_version": RETRIEVAL_POLICY_SUGGESTION_SCHEMA_VERSION,
                "question_type": "character_fate",
                "alias_expansions": ["晴雯"],
                "version_sensitive": true,
                "commentary_recommended": true,
                "confidence": 0.86,
                "unsupported_reason": null
            }),
        );

        assert!(report.accepted);
        assert!(
            report
                .final_policy
                .required_evidence_types
                .contains(&"base_text".to_string())
        );
        assert!(
            report
                .final_policy
                .required_evidence_types
                .contains(&"commentary".to_string())
        );
        assert!(
            report
                .final_policy
                .required_evidence_types
                .contains(&"version_note".to_string())
        );
        assert!(!report.required_evidence_downgraded);
    }

    #[test]
    fn forbidden_tool_choice_falls_back_to_base_policy() {
        let policy = base_policy(&["base_text"]);
        let report = evaluate_retrieval_policy_suggestion(
            &policy,
            &json!({
                "schema_version": RETRIEVAL_POLICY_SUGGESTION_SCHEMA_VERSION,
                "question_type": "commentary",
                "alias_expansions": [],
                "version_sensitive": false,
                "commentary_recommended": true,
                "confidence": 0.86,
                "unsupported_reason": null,
                "tool_choice": "tonglingyu.commentary.search"
            }),
        );

        assert!(report.fallback_used);
        assert_eq!(
            report.final_policy.required_evidence_types,
            vec!["base_text"]
        );
        assert!(
            report
                .rejected_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("forbidden_field"))
        );
    }
}
