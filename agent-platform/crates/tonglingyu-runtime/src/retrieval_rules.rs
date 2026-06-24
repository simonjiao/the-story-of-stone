use crate::{
    RETRIEVAL_RULES_PATH_ENV, normalize_text,
    rule_catalog::{RuleFileCache, configured_path, lock_rule_cache},
};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

const RETRIEVAL_RULES_SCHEMA_VERSION: &str = "tonglingyu.retrieval_rules.v1";
const DEFAULT_RETRIEVAL_RULES_JSON: &str = include_str!("../resources/retrieval_rules.json");

static RETRIEVAL_RULES_CATALOG_CACHE: OnceLock<Mutex<RuleFileCache<RetrievalRuleCatalog>>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetrievalRuleCatalog {
    schema_version: String,
    catalog_version: String,
    source_layer_labels: Vec<LabelRule>,
    evidence_text_hygiene: EvidenceTextHygieneRules,
    generic_question_terms: Vec<String>,
    ranking: RankingRules,
    source_classification: SourceClassificationRules,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelRule {
    id: String,
    label: String,
    answer_rank: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceTextHygieneRules {
    broken_shell_suffixes: Vec<String>,
    broken_shell_max_substantive_chars: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RankingRules {
    pub intro_question_terms: Vec<String>,
    pub commentary_question_terms: Vec<String>,
    pub commentary_source_id_terms: Vec<String>,
    pub version_source_boosts: Vec<VersionSourceBoostRule>,
    pub inscription_question_terms: Vec<String>,
    pub inscription_text_terms: Vec<String>,
    pub fate_question_terms: Vec<String>,
    pub fate_text_terms: Vec<String>,
    pub tonglingyu_terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionSourceBoostRule {
    pub question_terms: Vec<String>,
    pub source_id_terms: Vec<String>,
    pub score: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceClassificationRules {
    commentary_source_categories: Vec<String>,
    commentary_source_id_terms: Vec<String>,
    version_note_text_terms: Vec<String>,
    evidence_type_scopes: Vec<EvidenceTypeScopeRule>,
    source_systems: Vec<SourceSystemRule>,
    exact_source_priority: Vec<SourcePriorityRule>,
    default_source_system: String,
    usage_limits: Vec<UsageLimitRule>,
    default_usage_limit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceTypeScopeRule {
    pub id: String,
    pub support_scope: String,
    pub unsupported_scope: String,
    pub evidence_level: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSystemRule {
    source_id_terms: Vec<String>,
    label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePriorityRule {
    source_id_terms: Vec<String>,
    rank: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageLimitRule {
    source_categories: Vec<String>,
    usage_limit: String,
}

pub(crate) fn retrieval_rule_catalog() -> Result<RetrievalRuleCatalog> {
    let path = configured_path(RETRIEVAL_RULES_PATH_ENV);
    let cache =
        RETRIEVAL_RULES_CATALOG_CACHE.get_or_init(|| Mutex::new(default_retrieval_rule_cache()));
    let mut cache = lock_rule_cache(cache, "retrieval")?;
    cache.catalog(
        RETRIEVAL_RULES_PATH_ENV,
        path,
        default_retrieval_rule_catalog(),
        parse_retrieval_rule_catalog,
    )
}

pub(crate) fn retrieval_rule_catalog_metadata() -> Result<Value> {
    let path = configured_path(RETRIEVAL_RULES_PATH_ENV);
    let cache =
        RETRIEVAL_RULES_CATALOG_CACHE.get_or_init(|| Mutex::new(default_retrieval_rule_cache()));
    let mut cache = lock_rule_cache(cache, "retrieval")?;
    let catalog = cache.catalog(
        RETRIEVAL_RULES_PATH_ENV,
        path,
        default_retrieval_rule_catalog(),
        parse_retrieval_rule_catalog,
    )?;
    Ok(cache.metadata(RETRIEVAL_RULES_SCHEMA_VERSION, &catalog.catalog_version))
}

fn default_retrieval_rule_cache() -> RuleFileCache<RetrievalRuleCatalog> {
    RuleFileCache::new(default_retrieval_rule_catalog())
}

fn default_retrieval_rule_catalog() -> RetrievalRuleCatalog {
    parse_retrieval_rule_catalog(DEFAULT_RETRIEVAL_RULES_JSON)
        .expect("embedded retrieval rule catalog must parse")
}

fn parse_retrieval_rule_catalog(source: &str) -> Result<RetrievalRuleCatalog> {
    let catalog: RetrievalRuleCatalog =
        serde_json::from_str(source).context("retrieval rule catalog must be JSON")?;
    if catalog.schema_version != RETRIEVAL_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "retrieval rule catalog schema_version must be {}",
            RETRIEVAL_RULES_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "retrieval rule catalog catalog_version is required"
        ));
    }
    if catalog
        .source_layer_labels
        .iter()
        .any(|rule| rule.id.trim().is_empty() || rule.label.trim().is_empty())
    {
        return Err(anyhow!(
            "retrieval rule catalog source_layer_labels require id and label"
        ));
    }
    require_non_empty_terms(
        "evidence_text_hygiene.broken_shell_suffixes",
        &catalog.evidence_text_hygiene.broken_shell_suffixes,
    )?;
    require_non_empty_terms("generic_question_terms", &catalog.generic_question_terms)?;
    require_non_empty_terms(
        "ranking.intro_question_terms",
        &catalog.ranking.intro_question_terms,
    )?;
    require_non_empty_terms(
        "ranking.inscription_text_terms",
        &catalog.ranking.inscription_text_terms,
    )?;
    require_non_empty_terms(
        "ranking.fate_question_terms",
        &catalog.ranking.fate_question_terms,
    )?;
    require_non_empty_terms("ranking.fate_text_terms", &catalog.ranking.fate_text_terms)?;
    for rule in &catalog.ranking.version_source_boosts {
        require_non_empty_terms(
            "ranking.version_source_boosts.question_terms",
            &rule.question_terms,
        )?;
        require_non_empty_terms(
            "ranking.version_source_boosts.source_id_terms",
            &rule.source_id_terms,
        )?;
    }
    for scope in &catalog.source_classification.evidence_type_scopes {
        if scope.id.trim().is_empty()
            || scope.support_scope.trim().is_empty()
            || scope.unsupported_scope.trim().is_empty()
            || scope.evidence_level.trim().is_empty()
            || scope.confidence.trim().is_empty()
        {
            return Err(anyhow!(
                "retrieval rule catalog evidence_type_scopes require complete fields"
            ));
        }
    }
    if catalog
        .source_classification
        .default_source_system
        .trim()
        .is_empty()
        || catalog
            .source_classification
            .default_usage_limit
            .trim()
            .is_empty()
    {
        return Err(anyhow!(
            "retrieval rule catalog default source system and usage limit are required"
        ));
    }
    Ok(catalog)
}

fn require_non_empty_terms(name: &str, terms: &[String]) -> Result<()> {
    if terms.is_empty() || terms.iter().all(|term| term.trim().is_empty()) {
        return Err(anyhow!(
            "retrieval rule catalog {name} must define non-empty terms"
        ));
    }
    Ok(())
}

pub(crate) fn source_layer_label(source_layer: &str) -> Result<String> {
    let catalog = retrieval_rule_catalog()?;
    Ok(catalog
        .source_layer_labels
        .iter()
        .find(|rule| rule.id == source_layer)
        .or_else(|| {
            catalog
                .source_layer_labels
                .iter()
                .find(|rule| rule.id == "default")
        })
        .map(|rule| rule.label.clone())
        .unwrap_or_else(|| source_layer.to_string()))
}

pub(crate) fn source_layer_answer_rank(source_layer: &str) -> Result<usize> {
    let catalog = retrieval_rule_catalog()?;
    Ok(catalog
        .source_layer_labels
        .iter()
        .find(|rule| rule.id == source_layer)
        .or_else(|| {
            catalog
                .source_layer_labels
                .iter()
                .find(|rule| rule.id == "default")
        })
        .map(|rule| rule.answer_rank)
        .unwrap_or(usize::MAX))
}

pub(crate) fn evidence_text_is_broken_shell(text: &str, substantive_count: usize) -> Result<bool> {
    let catalog = retrieval_rule_catalog()?;
    let trimmed = text.trim();
    Ok(catalog
        .evidence_text_hygiene
        .broken_shell_suffixes
        .iter()
        .any(|suffix| trimmed.ends_with(suffix))
        && substantive_count
            <= catalog
                .evidence_text_hygiene
                .broken_shell_max_substantive_chars)
}

pub(crate) fn generic_question_term(term: &str) -> Result<bool> {
    let catalog = retrieval_rule_catalog()?;
    let normalized = normalize_text(term);
    Ok(catalog
        .generic_question_terms
        .iter()
        .any(|item| term_matches(term, &normalized, item)))
}

pub(crate) fn ranking_rules() -> Result<RankingRules> {
    Ok(retrieval_rule_catalog()?.ranking)
}

pub(crate) fn classify_evidence_type(
    source_category: &str,
    source_id: &str,
    text: &str,
) -> Result<String> {
    let catalog = retrieval_rule_catalog()?;
    let rules = catalog.source_classification;
    if rules
        .commentary_source_categories
        .iter()
        .any(|category| source_category == category.trim())
        || contains_any_raw(source_id, &rules.commentary_source_id_terms)
    {
        Ok("commentary".to_string())
    } else if contains_any_raw(text, &rules.version_note_text_terms) {
        Ok("version_note".to_string())
    } else {
        Ok("base_text".to_string())
    }
}

pub(crate) fn evidence_type_scope(evidence_type: &str) -> Result<EvidenceTypeScopeRule> {
    let catalog = retrieval_rule_catalog()?;
    catalog
        .source_classification
        .evidence_type_scopes
        .into_iter()
        .find(|scope| scope.id == evidence_type)
        .ok_or_else(|| anyhow!("retrieval rules missing evidence_type_scope for {evidence_type}"))
}

pub(crate) fn version_system(source_id: &str) -> Result<String> {
    let catalog = retrieval_rule_catalog()?;
    Ok(catalog
        .source_classification
        .source_systems
        .iter()
        .find(|rule| contains_any_raw(source_id, &rule.source_id_terms))
        .map(|rule| rule.label.clone())
        .unwrap_or(catalog.source_classification.default_source_system))
}

pub(crate) fn usage_limit_for_source_id(source_id: &str) -> Result<String> {
    let catalog = retrieval_rule_catalog()?;
    let source_classification = catalog.source_classification;
    if contains_any_raw(source_id, &source_classification.commentary_source_id_terms)
        && let Some(rule) = source_classification.usage_limits.iter().find(|rule| {
            rule.source_categories
                .iter()
                .any(|category| category == "commentary_material")
        })
    {
        return Ok(rule.usage_limit.clone());
    }
    Ok(source_classification.default_usage_limit)
}

pub(crate) fn exact_source_priority_rank(source_id: &str) -> Result<usize> {
    let catalog = retrieval_rule_catalog()?;
    Ok(catalog
        .source_classification
        .exact_source_priority
        .iter()
        .find(|rule| contains_any_raw(source_id, &rule.source_id_terms))
        .map(|rule| rule.rank)
        .unwrap_or(usize::MAX))
}

pub(crate) fn contains_any_term(text: &str, terms: &[String]) -> bool {
    let normalized = normalize_text(text);
    terms
        .iter()
        .any(|term| term_matches(text, &normalized, term))
}

pub(crate) fn contains_any_raw(text: &str, terms: &[String]) -> bool {
    terms
        .iter()
        .any(|term| !term.trim().is_empty() && text.contains(term.trim()))
}

fn term_matches(text: &str, normalized: &str, term: &str) -> bool {
    let term = term.trim();
    !term.is_empty() && (text.contains(term) || normalized.contains(&normalize_text(term)))
}

#[cfg(test)]
mod tests;
