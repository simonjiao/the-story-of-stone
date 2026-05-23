use crate::{EvidenceCard, extract_chapter_no, normalize_text};
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
    pub claim_statement_count: Option<usize>,
    pub rejected_reason: Option<&'static str>,
    pub coverage_status: Option<String>,
    pub evidence_hint_count: Option<usize>,
    pub retrieval_repair_recommended: Option<bool>,
    pub out_of_scope_hint_count: Option<usize>,
}

pub(crate) fn source_scope_policy_for_question(question: &str) -> SourceScopePolicy {
    let later_forty_allowed = question_explicitly_allows_later_forty(question);
    let mut allowed_source_layers = vec![
        "base_text_pre_80".to_string(),
        "commentary".to_string(),
        "version_note".to_string(),
    ];
    if later_forty_allowed {
        allowed_source_layers.push("base_text_later_40".to_string());
    }
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
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return rejected_bundle("invalid", Some("invalid_json_draft"));
    };
    let Some(object) = value.as_object() else {
        return rejected_bundle("json", Some("unsupported_json_bundle"));
    };
    let Some(schema_version) = object.get("schema_version").and_then(Value::as_str) else {
        return rejected_bundle("json", Some("bundle_schema_missing"));
    };
    if schema_version != UPSTREAM_BUNDLE_SCHEMA_VERSION {
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
            ..rejected_bundle("json", Some("bundle_package_id_missing"))
        };
    }
    if package_id
        .as_deref()
        .is_some_and(|value| value != expected_package_id)
    {
        return UpstreamBundleDraftExtraction {
            package_id,
            ..rejected_bundle("json", Some("bundle_package_id_mismatch"))
        };
    }
    let Some(policy_value) = object.get("source_scope_policy") else {
        return UpstreamBundleDraftExtraction {
            package_id,
            ..rejected_bundle("json", Some("source_scope_policy_missing"))
        };
    };
    if !source_scope_policy_matches(policy_value, expected_policy) {
        return UpstreamBundleDraftExtraction {
            package_id,
            ..rejected_bundle("json", Some("source_scope_policy_mismatch"))
        };
    }
    let coverage_status = object
        .get("coverage_assessment")
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let evidence_hint_count = object
        .get("evidence_hints")
        .and_then(Value::as_array)
        .map(Vec::len);
    let retrieval_repair_recommended = object
        .get("retrieval_repair")
        .and_then(|value| value.get("recommended"))
        .and_then(Value::as_bool);
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
            ..rejected_bundle("json", Some("package_id_missing"))
        };
    }
    if candidate_package_id
        .as_deref()
        .is_some_and(|value| value != expected_package_id)
    {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            ..rejected_bundle("json", Some("package_id_mismatch"))
        };
    }
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
            claim_statement_count,
            coverage_status,
            evidence_hint_count,
            retrieval_repair_recommended,
            out_of_scope_hint_count,
            ..rejected_bundle("json", Some(rejected_reason))
        };
    }
    if claim_statement_count.is_none() {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some("claim_statements_missing"))
        };
    }
    if let Some(reason) = invalid_claim_statements(draft_candidate, allowed_evidence_ids) {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some(reason))
        };
    }
    let draft_answer = draft_candidate
        .get("draft_answer")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if draft_answer.is_none() {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some("draft_answer_missing"))
        };
    }
    if !expected_policy.later_forty_allowed
        && draft_mentions_unscoped_later_forty_material(draft_answer.as_deref().unwrap_or(""))
    {
        return UpstreamBundleDraftExtraction {
            package_id: candidate_package_id,
            claim_statement_count,
            ..rejected_bundle("json", Some("draft_uses_unscoped_later_forty"))
        };
    }
    UpstreamBundleDraftExtraction {
        draft_answer,
        result_format: "json",
        package_id: candidate_package_id,
        claim_statement_count,
        rejected_reason: None,
        coverage_status,
        evidence_hint_count,
        retrieval_repair_recommended,
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
        claim_statement_count: None,
        rejected_reason,
        coverage_status: None,
        evidence_hint_count: None,
        retrieval_repair_recommended: None,
        out_of_scope_hint_count: None,
    }
}

fn question_explicitly_allows_later_forty(question: &str) -> bool {
    [
        "后四十",
        "後四十",
        "第八十一",
        "八十一回",
        "第九十",
        "九十回",
        "第094",
        "第94",
        "第九十四",
        "九十四回",
        "一百二十回",
        "百二十回",
        "120回",
        "120 回",
        "程高",
        "程甲",
        "程乙",
        "高鹗",
        "高鶚",
    ]
    .iter()
    .any(|term| question.contains(term))
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

fn draft_mentions_unscoped_later_forty_material(draft: &str) -> bool {
    let draft = normalize_text(draft);
    let compact_draft = draft.split_whitespace().collect::<String>();
    [
        "后四十",
        "後四十",
        "后四十回",
        "後四十回",
        "后40",
        "後40",
        "后40回",
        "後40回",
        "一百二十回",
        "第八十一回",
        "第九十回",
        "第九十四回",
        "第94回",
        "第094回",
        "一百二十回本",
        "120回",
        "120回本",
        "程高本",
        "高鹗",
        "高鶚",
    ]
    .iter()
    .any(|term| draft.contains(term) || compact_draft.contains(term))
}
