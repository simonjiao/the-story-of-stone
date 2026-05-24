use crate::{
    EvidenceCard, GOVERNANCE_RULES_PATH_ENV, cards_include_later_forty, normalize_text,
    text_mentions_later_forty_boundary,
};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const GOVERNANCE_RULES_SCHEMA_VERSION: &str = "tonglingyu.governance_rules.v1";
const DEFAULT_GOVERNANCE_RULES_JSON: &str = include_str!("../resources/governance_rules.json");

static GOVERNANCE_RULES_CATALOG_CACHE: OnceLock<Mutex<GovernanceRuleCatalogCache>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernanceRuleCatalog {
    schema_version: String,
    catalog_version: String,
    draft_boundary: DraftBoundaryRules,
    source_scope: SourceScopeGovernanceRules,
    claims: ClaimGovernanceRules,
    #[serde(default)]
    claim_evidence_links: Vec<ClaimEvidenceLinkRule>,
    review: ReviewGovernanceRules,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DraftBoundaryRules {
    pub user_opt_in_stop_terms: Vec<String>,
    pub unsupported_terms_without_evidence: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceScopeGovernanceRules {
    later_forty_question_terms: Vec<String>,
    later_forty_draft_terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClaimGovernanceRules {
    pub empty_evidence: String,
    pub slot_count_rule: String,
    pub inactive_count_basis: String,
    pub later_forty_boundary: String,
    pub commentary_scope: String,
    pub base_text_scope: String,
    pub default_scope: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewGovernanceRules {
    empty_evidence_issue: String,
    later_forty_boundary_issue: String,
    blocked_prompt_control_issue_template: String,
    #[serde(default)]
    prompt_controls: Vec<PromptControlRule>,
    #[serde(default)]
    rules: Vec<ReviewRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimEvidenceLinkRule {
    claim_terms: Vec<String>,
    evidence_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptControlRule {
    term: String,
    code: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewRule {
    id: String,
    trigger: ReviewTrigger,
    require: ReviewRequirement,
    issue: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewTrigger {
    #[serde(default)]
    question_any: Vec<String>,
    #[serde(default)]
    question_any_ci: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewRequirement {
    #[serde(default)]
    always_issue: bool,
    #[serde(default)]
    evidence_type_any: Vec<String>,
    #[serde(default)]
    card_text_any: Vec<String>,
    #[serde(default)]
    source_id_any: Vec<String>,
    #[serde(default)]
    claim_any: Vec<String>,
}

#[derive(Debug, Clone)]
struct GovernanceRuleCatalogCache {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    catalog: GovernanceRuleCatalog,
}

impl Default for GovernanceRuleCatalogCache {
    fn default() -> Self {
        Self {
            path: None,
            modified: None,
            len: 0,
            catalog: parse_governance_rule_catalog(DEFAULT_GOVERNANCE_RULES_JSON)
                .expect("embedded governance rule catalog must parse"),
        }
    }
}

impl GovernanceRuleCatalogCache {
    fn catalog(&mut self, path: Option<PathBuf>) -> Result<GovernanceRuleCatalog> {
        let Some(path) = path else {
            if self.path.is_some() {
                *self = Self::default();
            }
            return Ok(self.catalog.clone());
        };
        let metadata = fs::metadata(&path).with_context(|| {
            format!(
                "{}={} is not readable",
                GOVERNANCE_RULES_PATH_ENV,
                path.display()
            )
        })?;
        let modified = metadata.modified().ok();
        let len = metadata.len();
        if self.path.as_ref() == Some(&path) && self.modified == modified && self.len == len {
            return Ok(self.catalog.clone());
        }
        let source = fs::read_to_string(&path).with_context(|| {
            format!(
                "{}={} could not be read",
                GOVERNANCE_RULES_PATH_ENV,
                path.display()
            )
        })?;
        let catalog = parse_governance_rule_catalog(&source).with_context(|| {
            format!(
                "{}={} is not a valid governance rule catalog",
                GOVERNANCE_RULES_PATH_ENV,
                path.display()
            )
        })?;
        self.path = Some(path);
        self.modified = modified;
        self.len = len;
        self.catalog = catalog.clone();
        Ok(catalog)
    }
}

fn configured_governance_rules_path() -> Option<PathBuf> {
    std::env::var(GOVERNANCE_RULES_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn governance_rule_catalog() -> Result<GovernanceRuleCatalog> {
    let path = configured_governance_rules_path();
    let cache = GOVERNANCE_RULES_CATALOG_CACHE
        .get_or_init(|| Mutex::new(GovernanceRuleCatalogCache::default()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow!("governance rule catalog cache is poisoned"))?;
    cache.catalog(path)
}

fn parse_governance_rule_catalog(source: &str) -> Result<GovernanceRuleCatalog> {
    let catalog: GovernanceRuleCatalog =
        serde_json::from_str(source).context("governance rule catalog must be JSON")?;
    if catalog.schema_version != GOVERNANCE_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "governance rule catalog schema_version must be {}",
            GOVERNANCE_RULES_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "governance rule catalog catalog_version is required"
        ));
    }
    require_non_empty_terms(
        "draft_boundary.user_opt_in_stop_terms",
        &catalog.draft_boundary.user_opt_in_stop_terms,
    )?;
    require_non_empty_terms(
        "draft_boundary.unsupported_terms_without_evidence",
        &catalog.draft_boundary.unsupported_terms_without_evidence,
    )?;
    require_non_empty_terms(
        "source_scope.later_forty_question_terms",
        &catalog.source_scope.later_forty_question_terms,
    )?;
    require_non_empty_terms(
        "source_scope.later_forty_draft_terms",
        &catalog.source_scope.later_forty_draft_terms,
    )?;
    for (name, value) in [
        ("claims.empty_evidence", &catalog.claims.empty_evidence),
        ("claims.slot_count_rule", &catalog.claims.slot_count_rule),
        (
            "claims.inactive_count_basis",
            &catalog.claims.inactive_count_basis,
        ),
        (
            "claims.later_forty_boundary",
            &catalog.claims.later_forty_boundary,
        ),
        ("claims.commentary_scope", &catalog.claims.commentary_scope),
        ("claims.base_text_scope", &catalog.claims.base_text_scope),
        ("claims.default_scope", &catalog.claims.default_scope),
        (
            "review.empty_evidence_issue",
            &catalog.review.empty_evidence_issue,
        ),
        (
            "review.later_forty_boundary_issue",
            &catalog.review.later_forty_boundary_issue,
        ),
        (
            "review.blocked_prompt_control_issue_template",
            &catalog.review.blocked_prompt_control_issue_template,
        ),
    ] {
        if value.trim().is_empty() {
            return Err(anyhow!("governance rule catalog {name} is required"));
        }
    }
    for control in &catalog.review.prompt_controls {
        if control.term.trim().is_empty() || control.code.trim().is_empty() {
            return Err(anyhow!(
                "governance rule catalog prompt_controls require term and code"
            ));
        }
    }
    for rule in &catalog.claim_evidence_links {
        require_non_empty_terms("claim_evidence_links.claim_terms", &rule.claim_terms)?;
        require_non_empty_terms("claim_evidence_links.evidence_types", &rule.evidence_types)?;
    }
    for rule in &catalog.review.rules {
        if rule.id.trim().is_empty() || rule.issue.trim().is_empty() {
            return Err(anyhow!("governance review rules require id and issue"));
        }
        if rule.trigger.question_any.is_empty() && rule.trigger.question_any_ci.is_empty() {
            return Err(anyhow!(
                "governance review rule {} must define a trigger",
                rule.id
            ));
        }
        if !rule.require.always_issue
            && rule.require.evidence_type_any.is_empty()
            && rule.require.card_text_any.is_empty()
            && rule.require.source_id_any.is_empty()
            && rule.require.claim_any.is_empty()
        {
            return Err(anyhow!(
                "governance review rule {} must define a requirement or always_issue",
                rule.id
            ));
        }
    }
    Ok(catalog)
}

fn require_non_empty_terms(name: &str, terms: &[String]) -> Result<()> {
    if terms.is_empty() || terms.iter().all(|term| term.trim().is_empty()) {
        return Err(anyhow!(
            "governance rule catalog {name} must define non-empty terms"
        ));
    }
    Ok(())
}

pub(crate) fn draft_stops_for_user_opt_in(draft_text: &str) -> Result<bool> {
    let catalog = governance_rule_catalog()?;
    Ok(contains_any_term(
        draft_text,
        &normalize_text(draft_text),
        &catalog.draft_boundary.user_opt_in_stop_terms,
    ))
}

pub(crate) fn draft_has_unsupported_term_without_evidence(
    draft_text: &str,
    evidence_text: &str,
) -> Result<bool> {
    let catalog = governance_rule_catalog()?;
    Ok(catalog
        .draft_boundary
        .unsupported_terms_without_evidence
        .iter()
        .any(|term| {
            term_matches(draft_text, &normalize_text(draft_text), term)
                && !term_matches(evidence_text, &normalize_text(evidence_text), term)
        }))
}

pub(crate) fn source_scope_question_allows_later_forty(question: &str) -> Result<bool> {
    let catalog = governance_rule_catalog()?;
    Ok(contains_any_term(
        question,
        &normalize_text(question),
        &catalog.source_scope.later_forty_question_terms,
    ))
}

pub(crate) fn draft_mentions_unscoped_later_forty_material(draft: &str) -> Result<bool> {
    let catalog = governance_rule_catalog()?;
    let normalized = normalize_text(draft);
    let compact = normalized.split_whitespace().collect::<String>();
    Ok(catalog
        .source_scope
        .later_forty_draft_terms
        .iter()
        .any(|term| term_matches(&normalized, &compact, term)))
}

pub(crate) fn claim_rules() -> Result<ClaimGovernanceRules> {
    Ok(governance_rule_catalog()?.claims)
}

pub(crate) fn claim_evidence_types_for_claim(claim: &str) -> Result<Option<Vec<String>>> {
    let catalog = governance_rule_catalog()?;
    let normalized = normalize_text(claim);
    Ok(catalog
        .claim_evidence_links
        .into_iter()
        .find(|rule| contains_any_term(claim, &normalized, &rule.claim_terms))
        .map(|rule| rule.evidence_types))
}

pub(crate) fn empty_evidence_review_issue() -> Result<String> {
    Ok(governance_rule_catalog()?.review.empty_evidence_issue)
}

pub(crate) fn later_forty_boundary_review_issue() -> Result<String> {
    Ok(governance_rule_catalog()?.review.later_forty_boundary_issue)
}

pub(crate) fn blocked_prompt_control_issues(question: &str) -> Result<Vec<String>> {
    let catalog = governance_rule_catalog()?;
    let normalized = normalize_text(question);
    Ok(catalog
        .review
        .prompt_controls
        .iter()
        .filter(|control| term_matches(question, &normalized, &control.term))
        .map(|control| {
            catalog
                .review
                .blocked_prompt_control_issue_template
                .replace("{control}", &control.code)
        })
        .collect())
}

pub(crate) fn triggered_review_rule_issues(
    question: &str,
    cards: &[EvidenceCard],
    claims: &[String],
) -> Result<Vec<String>> {
    let catalog = governance_rule_catalog()?;
    let normalized_question = normalize_text(question);
    Ok(catalog
        .review
        .rules
        .iter()
        .filter(|rule| review_trigger_matches(&rule.trigger, question, &normalized_question))
        .filter(|rule| !review_requirement_satisfied(&rule.require, cards, claims))
        .map(|rule| rule.issue.clone())
        .collect())
}

pub(crate) fn preferred_answer_evidence_types(question: &str) -> Result<Vec<String>> {
    let catalog = governance_rule_catalog()?;
    let normalized_question = normalize_text(question);
    let mut evidence_types = Vec::new();
    for rule in &catalog.review.rules {
        if !review_trigger_matches(&rule.trigger, question, &normalized_question) {
            continue;
        }
        for evidence_type in &rule.require.evidence_type_any {
            let evidence_type = evidence_type.trim();
            if !evidence_type.is_empty() && !evidence_types.iter().any(|item| item == evidence_type)
            {
                evidence_types.push(evidence_type.to_string());
            }
        }
    }
    Ok(evidence_types)
}

pub(crate) fn later_forty_boundary_missing_from_claims(
    cards: &[EvidenceCard],
    claims: &[String],
) -> bool {
    cards_include_later_forty(cards)
        && !claims
            .iter()
            .any(|claim| text_mentions_later_forty_boundary(claim))
}

fn review_trigger_matches(trigger: &ReviewTrigger, question: &str, normalized: &str) -> bool {
    contains_any_term(question, normalized, &trigger.question_any)
        || contains_any_case_insensitive(question, &trigger.question_any_ci)
}

fn review_requirement_satisfied(
    require: &ReviewRequirement,
    cards: &[EvidenceCard],
    claims: &[String],
) -> bool {
    if require.always_issue {
        return false;
    }
    evidence_type_requirement_satisfied(&require.evidence_type_any, cards)
        || card_text_requirement_satisfied(&require.card_text_any, cards)
        || source_id_requirement_satisfied(&require.source_id_any, cards)
        || claim_requirement_satisfied(&require.claim_any, claims)
}

fn evidence_type_requirement_satisfied(terms: &[String], cards: &[EvidenceCard]) -> bool {
    !terms.is_empty()
        && cards
            .iter()
            .any(|card| terms.iter().any(|term| card.evidence_type == term.trim()))
}

fn card_text_requirement_satisfied(terms: &[String], cards: &[EvidenceCard]) -> bool {
    !terms.is_empty()
        && cards.iter().any(|card| {
            let normalized = normalize_text(&card.text);
            contains_any_term(&card.text, &normalized, terms)
        })
}

fn source_id_requirement_satisfied(terms: &[String], cards: &[EvidenceCard]) -> bool {
    !terms.is_empty()
        && cards.iter().any(|card| {
            terms
                .iter()
                .any(|term| !term.trim().is_empty() && card.source_id.contains(term.trim()))
        })
}

fn claim_requirement_satisfied(terms: &[String], claims: &[String]) -> bool {
    !terms.is_empty()
        && claims.iter().any(|claim| {
            let normalized = normalize_text(claim);
            contains_any_term(claim, &normalized, terms)
        })
}

fn contains_any_term(text: &str, normalized: &str, terms: &[String]) -> bool {
    terms
        .iter()
        .any(|term| term_matches(text, normalized, term))
}

fn term_matches(text: &str, normalized: &str, term: &str) -> bool {
    let term = term.trim();
    !term.is_empty() && (text.contains(term) || normalized.contains(&normalize_text(term)))
}

fn contains_any_case_insensitive(text: &str, terms: &[String]) -> bool {
    let lower = text.to_lowercase();
    terms
        .iter()
        .map(|term| term.trim().to_lowercase())
        .any(|term| !term.is_empty() && lower.contains(&term))
}

#[cfg(test)]
mod tests;
