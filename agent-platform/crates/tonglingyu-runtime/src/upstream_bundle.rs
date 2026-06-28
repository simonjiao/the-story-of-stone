use crate::{
    EvidenceCard, extract_chapter_no,
    governance_rules::{
        draft_mentions_unscoped_later_forty_material, source_scope_question_allows_later_forty,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const UPSTREAM_BUNDLE_SCHEMA_VERSION: &str = "tonglingyu-upstream-bundle-v1";
const SOURCE_SCOPE_POLICY_SCHEMA_VERSION: &str = "tonglingyu-source-scope-policy-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SourceScopePolicy {
    pub schema_version: String,
    pub default_answer_scope: String,
    pub allowed_source_layers: Vec<String>,
    pub excluded_unless_user_explicit: Vec<String>,
    pub commentary_evidence_rank: String,
    pub later_forty_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OutOfScopeEvidenceHint {
    pub evidence_id: String,
    pub source_id: String,
    pub source_title: String,
    pub source_layer: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceScopeFilterReport {
    pub object: String,
    pub policy: SourceScopePolicy,
    pub included_evidence_ids: Vec<String>,
    pub out_of_scope_hints: Vec<OutOfScopeEvidenceHint>,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceScopeFilterResult {
    pub included_cards: Vec<EvidenceCard>,
    pub report: SourceScopeFilterReport,
}

#[derive(Debug, Clone)]
pub(crate) struct UpstreamBundleDraftExtraction {
    pub draft_answer: Option<String>,
    pub result_format: &'static str,
    pub package_id: Option<String>,
    pub package_id_rebound: bool,
    pub observed_bundle_package_id: Option<String>,
    pub observed_candidate_package_id: Option<String>,
    pub claim_statement_count: Option<usize>,
    pub claim_statements: Vec<String>,
    pub claim_evidence_refs: Vec<Vec<String>>,
    pub rejected_reason: Option<&'static str>,
    pub coverage_status: Option<String>,
    pub evidence_hint_count: Option<usize>,
    pub retrieval_repair_recommended: Option<bool>,
    pub retrieval_repair_queries: Vec<Value>,
    pub out_of_scope_hint_count: Option<usize>,
}

pub(crate) fn source_scope_policy_for_question(question: &str) -> SourceScopePolicy {
    let later_forty_allowed = question_explicitly_allows_later_forty(question);
    let allowed_source_layers = if later_forty_allowed {
        vec!["base_text_later_40".to_string(), "version_note".to_string()]
    } else {
        vec![
            "base_text_pre_80".to_string(),
            "commentary".to_string(),
            "version_note".to_string(),
        ]
    };
    let excluded_unless_user_explicit = if later_forty_allowed {
        Vec::new()
    } else {
        vec!["base_text_later_40".to_string()]
    };
    SourceScopePolicy {
        schema_version: SOURCE_SCOPE_POLICY_SCHEMA_VERSION.to_string(),
        default_answer_scope: if later_forty_allowed {
            "explicit_later_forty_scope".to_string()
        } else {
            "pre_80_text_and_commentary".to_string()
        },
        allowed_source_layers,
        excluded_unless_user_explicit,
        commentary_evidence_rank: "first_class".to_string(),
        later_forty_allowed,
    }
}

pub(crate) fn filter_cards_for_source_scope(
    question: &str,
    cards: Vec<EvidenceCard>,
) -> SourceScopeFilterResult {
    let policy = source_scope_policy_for_question(question);
    let allowed = policy
        .allowed_source_layers
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut included_cards = Vec::new();
    let mut out_of_scope_hints = Vec::new();
    for card in cards {
        let source_layer = evidence_card_source_layer(&card);
        if allowed.contains(source_layer) {
            included_cards.push(card);
            continue;
        }
        out_of_scope_hints.push(OutOfScopeEvidenceHint {
            evidence_id: card.evidence_id,
            source_id: card.source_id,
            source_title: card.source_title,
            source_layer: source_layer.to_string(),
            reason: if source_layer == "base_text_later_40" {
                "user did not explicitly allow later-forty evidence".to_string()
            } else {
                "source layer is outside the active answer scope".to_string()
            },
        });
    }
    let included_evidence_ids = included_cards
        .iter()
        .map(|card| card.evidence_id.clone())
        .collect();
    SourceScopeFilterResult {
        included_cards,
        report: SourceScopeFilterReport {
            object: "tonglingyu.source_scope_filter".to_string(),
            policy,
            included_evidence_ids,
            out_of_scope_hints,
        },
    }
}

pub(crate) fn evidence_card_source_layer(card: &EvidenceCard) -> &'static str {
    if card.evidence_type == "commentary" {
        "commentary"
    } else if card.evidence_type == "version_note" {
        "version_note"
    } else if evidence_card_is_later_forty(card) || base_text_card_contains_later_forty_marker(card)
    {
        "base_text_later_40"
    } else {
        "base_text_pre_80"
    }
}

pub(crate) fn evidence_card_is_later_forty(card: &EvidenceCard) -> bool {
    source_title_in_later_forty(&card.source_title)
}

pub(crate) fn source_title_in_later_forty(source_title: &str) -> bool {
    extract_chapter_no(source_title).is_some_and(|chapter_no| chapter_no >= 81)
}

fn base_text_card_contains_later_forty_marker(card: &EvidenceCard) -> bool {
    card.evidence_type == "base_text" && text_contains_later_forty_chapter_marker(&card.text)
}

fn text_contains_later_forty_chapter_marker(text: &str) -> bool {
    text.char_indices()
        .filter(|(_, ch)| *ch == '第')
        .any(|(index, _)| extract_chapter_no(&text[index..]).is_some_and(|number| number >= 81))
}

pub(crate) fn text_mentions_later_forty_boundary(text: &str) -> bool {
    text.contains("后四十") || text.contains("後四十")
}

pub(crate) fn extract_upstream_bundle_draft(
    result_summary: &str,
    expected_package_id: &str,
    expected_policy: &SourceScopePolicy,
    allowed_evidence_ids: &BTreeSet<String>,
) -> UpstreamBundleDraftExtraction {
    let trimmed = result_summary.trim();
    let Some(value) = parse_result_summary_json(trimmed) else {
        return rejected_bundle("invalid", Some("invalid_json_draft"));
    };
    let Some(object) = value.as_object() else {
        return rejected_bundle("json", Some("unsupported_json_bundle"));
    };
    let Some(schema_version) = object.get("schema_version").and_then(Value::as_str) else {
        return rejected_bundle("json", Some("bundle_schema_missing"));
    };
    if !upstream_bundle_schema_version_matches(schema_version) {
        return rejected_bundle("json", Some("bundle_schema_mismatch"));
    }
    let package_id = object
        .get("package_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if package_id.is_none() {
        return UpstreamBundleDraftExtraction {
            package_id,
            observed_bundle_package_id: None,
            ..rejected_bundle("json", Some("bundle_package_id_missing"))
        };
    }
    let bundle_package_id_rebound = package_id
        .as_deref()
        .is_some_and(|value| value != expected_package_id);
    let observed_bundle_package_id = package_id.clone();
    let Some(policy_value) = object.get("source_scope_policy") else {
        return UpstreamBundleDraftExtraction {
            package_id,
            package_id_rebound: bundle_package_id_rebound,
            observed_bundle_package_id,
            ..rejected_bundle("json", Some("source_scope_policy_missing"))
        };
    };
    if !source_scope_policy_matches(policy_value, expected_policy) {
        return UpstreamBundleDraftExtraction {
            package_id,
            package_id_rebound: bundle_package_id_rebound,
            observed_bundle_package_id,
            ..rejected_bundle("json", Some("source_scope_policy_mismatch"))
        };
    }
    let coverage_status = object_coverage_assessment_status(object);
    let evidence_hint_count = object
        .get("evidence_hints")
        .and_then(Value::as_array)
        .map(Vec::len);
    let retrieval_repair_recommended = object
        .get("retrieval_repair")
        .and_then(|value| value.get("recommended"))
        .and_then(Value::as_bool);
    let retrieval_repair_queries = object
        .get("retrieval_repair")
        .and_then(|value| value.get("queries"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let out_of_scope_hint_count = object
        .get("out_of_scope_hints")
        .and_then(Value::as_array)
        .map(Vec::len);
    let Some(draft_candidate) = object.get("draft_candidate").and_then(Value::as_object) else {
        return UpstreamBundleDraftExtraction {
            package_id,
            ..rejected_bundle("json", Some("draft_candidate_missing"))
        };
    };
    let candidate_package_id = draft_candidate
        .get("package_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if candidate_package_id.is_none() {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id.or(package_id),
            package_id_rebound: bundle_package_id_rebound,
            observed_bundle_package_id,
            observed_candidate_package_id: None,
            ..rejected_bundle("json", Some("package_id_missing"))
        };
    }
    let candidate_package_id_rebound = candidate_package_id
        .as_deref()
        .is_some_and(|value| value != expected_package_id);
    let observed_candidate_package_id = candidate_package_id.clone();
    let claim_statement_count = draft_candidate
        .get("claim_statements")
        .and_then(Value::as_array)
        .map(Vec::len);
    if coverage_status.as_deref() != Some("passed") {
        let rejected_reason = if coverage_status.is_some() {
            "coverage_assessment_not_passed"
        } else {
            "coverage_assessment_status_missing"
        };
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            package_id_rebound: bundle_package_id_rebound || candidate_package_id_rebound,
            observed_bundle_package_id,
            observed_candidate_package_id,
            claim_statement_count,
            coverage_status,
            evidence_hint_count,
            retrieval_repair_recommended,
            retrieval_repair_queries,
            out_of_scope_hint_count,
            ..rejected_bundle("json", Some(rejected_reason))
        };
    }
    if claim_statement_count.is_none() {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            package_id_rebound: bundle_package_id_rebound || candidate_package_id_rebound,
            observed_bundle_package_id,
            observed_candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some("claim_statements_missing"))
        };
    }
    if let Some(reason) = invalid_claim_statements(draft_candidate, allowed_evidence_ids) {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            package_id_rebound: bundle_package_id_rebound || candidate_package_id_rebound,
            observed_bundle_package_id,
            observed_candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some(reason))
        };
    }
    let claim_statements = claim_statement_texts(draft_candidate);
    let claim_evidence_refs = claim_statement_evidence_refs(draft_candidate);
    let draft_answer = draft_candidate
        .get("draft_answer")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if draft_answer.is_none() {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            package_id_rebound: bundle_package_id_rebound || candidate_package_id_rebound,
            observed_bundle_package_id,
            observed_candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some("draft_answer_missing"))
        };
    }
    if !expected_policy.later_forty_allowed
        && draft_mentions_unscoped_later_forty_material(draft_answer.as_deref().unwrap_or(""))
            .unwrap_or(true)
    {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            package_id_rebound: bundle_package_id_rebound || candidate_package_id_rebound,
            observed_bundle_package_id,
            observed_candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some("draft_uses_unscoped_later_forty"))
        };
    }
    UpstreamBundleDraftExtraction {
        draft_answer,
        result_format: "json",
        package_id: Some(expected_package_id.to_string()),
        package_id_rebound: bundle_package_id_rebound || candidate_package_id_rebound,
        observed_bundle_package_id,
        observed_candidate_package_id,
        claim_statement_count,
        claim_statements,
        claim_evidence_refs,
        rejected_reason: None,
        coverage_status,
        evidence_hint_count,
        retrieval_repair_recommended,
        retrieval_repair_queries,
        out_of_scope_hint_count,
    }
}

fn rejected_bundle(
    result_format: &'static str,
    rejected_reason: Option<&'static str>,
) -> UpstreamBundleDraftExtraction {
    UpstreamBundleDraftExtraction {
        draft_answer: None,
        result_format,
        package_id: None,
        package_id_rebound: false,
        observed_bundle_package_id: None,
        observed_candidate_package_id: None,
        claim_statement_count: None,
        claim_statements: Vec::new(),
        claim_evidence_refs: Vec::new(),
        rejected_reason,
        coverage_status: None,
        evidence_hint_count: None,
        retrieval_repair_recommended: None,
        retrieval_repair_queries: Vec::new(),
        out_of_scope_hint_count: None,
    }
}

fn parse_result_summary_json(trimmed: &str) -> Option<Value> {
    serde_json::from_str::<Value>(trimmed).ok().or_else(|| {
        extract_first_json_object(trimmed).and_then(|raw| serde_json::from_str(raw).ok())
    })
}

fn extract_first_json_object(text: &str) -> Option<&str> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if start.is_none() {
            if ch == '{' {
                start = Some(index);
                depth = 1;
            }
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return start.map(|start| &text[start..index + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

fn upstream_bundle_schema_version_matches(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed == UPSTREAM_BUNDLE_SCHEMA_VERSION {
        return true;
    }
    let normalized = trimmed.to_ascii_lowercase().replace(['_', '.'], "-");
    normalized == UPSTREAM_BUNDLE_SCHEMA_VERSION
        || normalized == format!("{}.0", UPSTREAM_BUNDLE_SCHEMA_VERSION).replace('.', "-")
}

fn question_explicitly_allows_later_forty(question: &str) -> bool {
    source_scope_question_allows_later_forty(question).unwrap_or(false)
}

fn source_scope_policy_matches(value: &Value, expected: &SourceScopePolicy) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let schema_version = object
        .get("schema_version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let default_answer_scope = object
        .get("default_answer_scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let commentary_evidence_rank = object
        .get("commentary_evidence_rank")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let later_forty_allowed = object
        .get("later_forty_allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let allowed_source_layers = value_string_set(object.get("allowed_source_layers"));
    let excluded_unless_user_explicit =
        value_string_set(object.get("excluded_unless_user_explicit"));
    let expected_allowed_source_layers = expected
        .allowed_source_layers
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected_excluded_unless_user_explicit = expected
        .excluded_unless_user_explicit
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    schema_version == expected.schema_version
        && default_answer_scope == expected.default_answer_scope
        && commentary_evidence_rank == "first_class"
        && later_forty_allowed == expected.later_forty_allowed
        && allowed_source_layers == expected_allowed_source_layers
        && excluded_unless_user_explicit == expected_excluded_unless_user_explicit
}

fn value_string_set(value: Option<&Value>) -> BTreeSet<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn object_coverage_assessment_status(object: &serde_json::Map<String, Value>) -> Option<String> {
    object
        .get("coverage_assessment")
        .and_then(coverage_assessment_status)
        .or_else(|| {
            object
                .get("coverage_status")
                .and_then(coverage_assessment_status)
        })
        .or_else(|| object.get("coverage").and_then(coverage_assessment_status))
}

fn coverage_assessment_status(value: &Value) -> Option<String> {
    let raw = value.as_str().or_else(|| {
        value.as_object().and_then(|object| {
            object
                .get("status")
                .or_else(|| object.get("coverage_status"))
                .or_else(|| object.get("coverage"))
                .or_else(|| object.get("result"))
                .and_then(Value::as_str)
        })
    })?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(match raw.to_ascii_lowercase().as_str() {
        "ok" | "complete" | "completed" | "covered" | "sufficient" => "passed".to_string(),
        value => value.to_string(),
    })
}

fn invalid_claim_statements(
    draft_candidate: &serde_json::Map<String, Value>,
    allowed_evidence_ids: &BTreeSet<String>,
) -> Option<&'static str> {
    let Some(claims) = draft_candidate
        .get("claim_statements")
        .and_then(Value::as_array)
    else {
        return Some("claim_statements_missing");
    };
    if claims.is_empty() {
        return Some("claim_statements_empty");
    }
    for claim in claims {
        let Some(object) = claim.as_object() else {
            return Some("claim_statement_invalid");
        };
        if object
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            return Some("claim_statement_text_missing");
        }
        let Some(refs) = object.get("evidence_refs").and_then(Value::as_array) else {
            return Some("claim_evidence_refs_missing");
        };
        if refs.is_empty() {
            if allowed_evidence_ids.is_empty() {
                return Some("claim_evidence_refs_unavailable");
            }
            return Some("claim_evidence_refs_empty");
        }
        for evidence_ref in refs {
            let Some(evidence_ref) = evidence_ref.as_str().map(str::trim) else {
                return Some("claim_evidence_ref_invalid");
            };
            if !allowed_evidence_ids.contains(evidence_ref) {
                return Some("claim_evidence_ref_outside_package");
            }
        }
    }
    None
}

fn claim_statement_texts(draft_candidate: &serde_json::Map<String, Value>) -> Vec<String> {
    draft_candidate
        .get("claim_statements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|object| object.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn claim_statement_evidence_refs(
    draft_candidate: &serde_json::Map<String, Value>,
) -> Vec<Vec<String>> {
    draft_candidate
        .get("claim_statements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|object| {
            object
                .get("evidence_refs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_upstream_bundle_accepts_coverage_alias_for_passed_status() {
        let expected_policy = source_scope_policy_for_question("通灵玉是什么？");
        let allowed_evidence_ids = BTreeSet::from(["ev-1".to_string()]);
        let result_summary = json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": "pkg-1",
            "source_scope_policy": expected_policy,
            "draft_candidate": {
                "package_id": "pkg-1",
                "draft_answer": "通灵玉即通灵宝玉。",
                "claim_statements": [
                    {
                        "text": "通灵玉即通灵宝玉。",
                        "evidence_refs": ["ev-1"]
                    }
                ]
            },
            "coverage_assessment": {
                "coverage": "sufficient",
                "missing_in_scope_slots": [],
                "out_of_scope_slots": []
            },
            "evidence_hints": [],
            "retrieval_repair": {"recommended": false, "queries": []},
            "out_of_scope_hints": []
        })
        .to_string();

        let extraction = extract_upstream_bundle_draft(
            &result_summary,
            "pkg-1",
            &source_scope_policy_for_question("通灵玉是什么？"),
            &allowed_evidence_ids,
        );

        assert_eq!(extraction.rejected_reason, None);
        assert_eq!(extraction.coverage_status.as_deref(), Some("passed"));
    }

    #[test]
    fn extract_upstream_bundle_still_rejects_partial_coverage_alias() {
        let expected_policy = source_scope_policy_for_question("通灵玉是什么？");
        let allowed_evidence_ids = BTreeSet::from(["ev-1".to_string()]);
        let result_summary = json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": "pkg-1",
            "source_scope_policy": expected_policy,
            "draft_candidate": {
                "package_id": "pkg-1",
                "draft_answer": "通灵玉即通灵宝玉。",
                "claim_statements": [
                    {
                        "text": "通灵玉即通灵宝玉。",
                        "evidence_refs": ["ev-1"]
                    }
                ]
            },
            "coverage_assessment": {
                "coverage": "partial",
                "missing_in_scope_slots": ["仍缺少一条证据"],
                "out_of_scope_slots": []
            }
        })
        .to_string();

        let extraction = extract_upstream_bundle_draft(
            &result_summary,
            "pkg-1",
            &source_scope_policy_for_question("通灵玉是什么？"),
            &allowed_evidence_ids,
        );

        assert_eq!(
            extraction.rejected_reason,
            Some("coverage_assessment_not_passed")
        );
        assert_eq!(extraction.coverage_status.as_deref(), Some("partial"));
    }

    #[test]
    fn extract_upstream_bundle_accepts_string_coverage_assessment() {
        let expected_policy = source_scope_policy_for_question("通灵玉是什么？");
        let allowed_evidence_ids = BTreeSet::from(["ev-1".to_string()]);
        let result_summary = json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": "pkg-1",
            "source_scope_policy": expected_policy,
            "draft_candidate": {
                "package_id": "pkg-1",
                "draft_answer": "通灵玉即通灵宝玉。",
                "claim_statements": [
                    {
                        "text": "通灵玉即通灵宝玉。",
                        "evidence_refs": ["ev-1"]
                    }
                ]
            },
            "coverage_assessment": "passed"
        })
        .to_string();

        let extraction = extract_upstream_bundle_draft(
            &result_summary,
            "pkg-1",
            &source_scope_policy_for_question("通灵玉是什么？"),
            &allowed_evidence_ids,
        );

        assert_eq!(extraction.rejected_reason, None);
        assert_eq!(extraction.coverage_status.as_deref(), Some("passed"));
    }

    #[test]
    fn extract_upstream_bundle_accepts_top_level_coverage_status() {
        let expected_policy = source_scope_policy_for_question("通灵玉是什么？");
        let allowed_evidence_ids = BTreeSet::from(["ev-1".to_string()]);
        let result_summary = json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": "pkg-1",
            "source_scope_policy": expected_policy,
            "draft_candidate": {
                "package_id": "pkg-1",
                "draft_answer": "通灵玉即通灵宝玉。",
                "claim_statements": [
                    {
                        "text": "通灵玉即通灵宝玉。",
                        "evidence_refs": ["ev-1"]
                    }
                ]
            },
            "coverage_status": "ok"
        })
        .to_string();

        let extraction = extract_upstream_bundle_draft(
            &result_summary,
            "pkg-1",
            &source_scope_policy_for_question("通灵玉是什么？"),
            &allowed_evidence_ids,
        );

        assert_eq!(extraction.rejected_reason, None);
        assert_eq!(extraction.coverage_status.as_deref(), Some("passed"));
    }

    #[test]
    fn extract_upstream_bundle_accepts_wrapped_json_object() {
        let expected_policy = source_scope_policy_for_question("通灵玉是什么？");
        let allowed_evidence_ids = BTreeSet::from(["ev-1".to_string()]);
        let bundle = json!({
            "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
            "package_id": "pkg-1",
            "source_scope_policy": expected_policy,
            "draft_candidate": {
                "package_id": "pkg-1",
                "draft_answer": "通灵玉即通灵宝玉。",
                "claim_statements": [
                    {
                        "text": "通灵玉即通灵宝玉。",
                        "evidence_refs": ["ev-1"]
                    }
                ]
            },
            "coverage_assessment": {"status": "passed"}
        });
        let result_summary = format!("```json\n{}\n```", bundle);

        let extraction = extract_upstream_bundle_draft(
            &result_summary,
            "pkg-1",
            &source_scope_policy_for_question("通灵玉是什么？"),
            &allowed_evidence_ids,
        );

        assert_eq!(extraction.rejected_reason, None);
        assert_eq!(extraction.coverage_status.as_deref(), Some("passed"));
    }

    #[test]
    fn extract_upstream_bundle_accepts_dotted_schema_version() {
        let expected_policy = source_scope_policy_for_question("通灵玉是什么？");
        let allowed_evidence_ids = BTreeSet::from(["ev-1".to_string()]);
        let result_summary = json!({
            "schema_version": "tonglingyu.upstream.bundle.v1",
            "package_id": "pkg-1",
            "source_scope_policy": expected_policy,
            "draft_candidate": {
                "package_id": "pkg-1",
                "draft_answer": "通灵玉即通灵宝玉。",
                "claim_statements": [
                    {
                        "text": "通灵玉即通灵宝玉。",
                        "evidence_refs": ["ev-1"]
                    }
                ]
            },
            "coverage_assessment": {"status": "passed"}
        })
        .to_string();

        let extraction = extract_upstream_bundle_draft(
            &result_summary,
            "pkg-1",
            &source_scope_policy_for_question("通灵玉是什么？"),
            &allowed_evidence_ids,
        );

        assert_eq!(extraction.rejected_reason, None);
        assert_eq!(extraction.coverage_status.as_deref(), Some("passed"));
    }
}
