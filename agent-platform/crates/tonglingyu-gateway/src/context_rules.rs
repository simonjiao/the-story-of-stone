use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

pub(crate) const SUBJECT_ONTOLOGY_PATH_ENV: &str = "TONGLINGYU_SUBJECT_ONTOLOGY_PATH";
pub(crate) const REFERENT_CANDIDATE_RULES_PATH_ENV: &str =
    "TONGLINGYU_REFERENT_CANDIDATE_RULES_PATH";
pub(crate) const ELLIPSIS_RESOLUTION_RULES_PATH_ENV: &str =
    "TONGLINGYU_ELLIPSIS_RESOLUTION_RULES_PATH";
pub(crate) const CURRENT_WINDOW_COMPRESSION_RULES_PATH_ENV: &str =
    "TONGLINGYU_CURRENT_WINDOW_COMPRESSION_RULES_PATH";

const SUBJECT_ONTOLOGY_SCHEMA_VERSION: &str = "tonglingyu.subject_ontology.v1";
const REFERENT_CANDIDATE_RULES_SCHEMA_VERSION: &str = "tonglingyu.referent_candidate_rules.v1";
const ELLIPSIS_RESOLUTION_RULES_SCHEMA_VERSION: &str = "tonglingyu.ellipsis_resolution_rules.v1";
const CURRENT_WINDOW_COMPRESSION_RULES_SCHEMA_VERSION: &str =
    "tonglingyu.current_window_compression_rules.v1";

const DEFAULT_SUBJECT_ONTOLOGY_JSON: &str = include_str!("../resources/subject_ontology.json");
const DEFAULT_REFERENT_CANDIDATE_RULES_JSON: &str =
    include_str!("../resources/referent_candidate_rules.json");
const DEFAULT_ELLIPSIS_RESOLUTION_RULES_JSON: &str =
    include_str!("../resources/ellipsis_resolution_rules.json");
const DEFAULT_CURRENT_WINDOW_COMPRESSION_RULES_JSON: &str =
    include_str!("../resources/current_window_compression_rules.json");

static CONTEXT_RULES_CACHE: OnceLock<Mutex<ContextRulesCache>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectOntologyCatalog {
    schema_version: String,
    catalog_version: String,
    subjects: Vec<SubjectRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectRule {
    canonical: String,
    #[serde(rename = "type")]
    subject_type: String,
    work: String,
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferentCandidateRules {
    schema_version: String,
    catalog_version: String,
    pronoun_terms: Vec<String>,
    replacement_terms: Vec<String>,
    history_reference_terms: Vec<String>,
    source_priority: Vec<String>,
    max_candidates: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EllipsisResolutionRules {
    schema_version: String,
    catalog_version: String,
    continuation_questions: Vec<String>,
    followup_questions: Vec<String>,
    followup_suffix_terms: Vec<String>,
    trigger: String,
    clarification_template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentWindowCompressionRules {
    schema_version: String,
    catalog_version: String,
    policy_id: String,
    max_raw_messages: usize,
    max_raw_chars: usize,
    max_compressor_input_chars: usize,
    must_preserve_user_turns: usize,
    compressor_profile: String,
    digest_schema: String,
    timeout_ms: u64,
    coverage_statuses: Vec<String>,
    reject_on_new_entities: bool,
    reject_on_missing_source_refs: bool,
    reject_on_schema_invalid: bool,
    allow_rejected_digest_on_main_path: bool,
}

#[derive(Debug, Clone)]
struct ContextRuleCatalogs {
    subject_ontology: SubjectOntologyCatalog,
    referent_candidate_rules: ReferentCandidateRules,
    ellipsis_resolution_rules: EllipsisResolutionRules,
    current_window_compression_rules: CurrentWindowCompressionRules,
}

#[derive(Debug, Clone)]
struct RuleFileCache<T> {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    catalog: T,
}

impl<T: Clone> RuleFileCache<T> {
    fn new(catalog: T) -> Self {
        Self {
            path: None,
            modified: None,
            len: 0,
            catalog,
        }
    }

    fn catalog(
        &mut self,
        env_name: &str,
        path: Option<PathBuf>,
        default_catalog: T,
        parse: fn(&str) -> Result<T>,
    ) -> Result<T> {
        let Some(path) = path else {
            if self.path.is_some() {
                *self = Self::new(default_catalog);
            }
            return Ok(self.catalog.clone());
        };
        let metadata = fs::metadata(&path)
            .with_context(|| format!("{env_name}={} is not readable", path.display()))?;
        let modified = metadata.modified().ok();
        let len = metadata.len();
        if self.path.as_ref() == Some(&path) && self.modified == modified && self.len == len {
            return Ok(self.catalog.clone());
        }
        let source = fs::read_to_string(&path)
            .with_context(|| format!("{env_name}={} could not be read", path.display()))?;
        let catalog = parse(&source)
            .with_context(|| format!("{env_name}={} is not a valid catalog", path.display()))?;
        self.path = Some(path);
        self.modified = modified;
        self.len = len;
        self.catalog = catalog.clone();
        Ok(catalog)
    }
}

#[derive(Debug, Clone)]
struct ContextRulesCache {
    subject_ontology: RuleFileCache<SubjectOntologyCatalog>,
    referent_candidate_rules: RuleFileCache<ReferentCandidateRules>,
    ellipsis_resolution_rules: RuleFileCache<EllipsisResolutionRules>,
    current_window_compression_rules: RuleFileCache<CurrentWindowCompressionRules>,
}

impl Default for ContextRulesCache {
    fn default() -> Self {
        Self {
            subject_ontology: RuleFileCache::new(
                parse_subject_ontology(DEFAULT_SUBJECT_ONTOLOGY_JSON)
                    .expect("embedded subject ontology must parse"),
            ),
            referent_candidate_rules: RuleFileCache::new(
                parse_referent_candidate_rules(DEFAULT_REFERENT_CANDIDATE_RULES_JSON)
                    .expect("embedded referent candidate rules must parse"),
            ),
            ellipsis_resolution_rules: RuleFileCache::new(
                parse_ellipsis_resolution_rules(DEFAULT_ELLIPSIS_RESOLUTION_RULES_JSON)
                    .expect("embedded ellipsis resolution rules must parse"),
            ),
            current_window_compression_rules: RuleFileCache::new(
                parse_current_window_compression_rules(
                    DEFAULT_CURRENT_WINDOW_COMPRESSION_RULES_JSON,
                )
                .expect("embedded current-window compression rules must parse"),
            ),
        }
    }
}

impl ContextRulesCache {
    fn catalogs(&mut self) -> Result<ContextRuleCatalogs> {
        Ok(ContextRuleCatalogs {
            subject_ontology: self.subject_ontology.catalog(
                SUBJECT_ONTOLOGY_PATH_ENV,
                configured_path(SUBJECT_ONTOLOGY_PATH_ENV),
                parse_subject_ontology(DEFAULT_SUBJECT_ONTOLOGY_JSON)
                    .expect("embedded subject ontology must parse"),
                parse_subject_ontology,
            )?,
            referent_candidate_rules: self.referent_candidate_rules.catalog(
                REFERENT_CANDIDATE_RULES_PATH_ENV,
                configured_path(REFERENT_CANDIDATE_RULES_PATH_ENV),
                parse_referent_candidate_rules(DEFAULT_REFERENT_CANDIDATE_RULES_JSON)
                    .expect("embedded referent candidate rules must parse"),
                parse_referent_candidate_rules,
            )?,
            ellipsis_resolution_rules: self.ellipsis_resolution_rules.catalog(
                ELLIPSIS_RESOLUTION_RULES_PATH_ENV,
                configured_path(ELLIPSIS_RESOLUTION_RULES_PATH_ENV),
                parse_ellipsis_resolution_rules(DEFAULT_ELLIPSIS_RESOLUTION_RULES_JSON)
                    .expect("embedded ellipsis resolution rules must parse"),
                parse_ellipsis_resolution_rules,
            )?,
            current_window_compression_rules: self.current_window_compression_rules.catalog(
                CURRENT_WINDOW_COMPRESSION_RULES_PATH_ENV,
                configured_path(CURRENT_WINDOW_COMPRESSION_RULES_PATH_ENV),
                parse_current_window_compression_rules(
                    DEFAULT_CURRENT_WINDOW_COMPRESSION_RULES_JSON,
                )
                .expect("embedded current-window compression rules must parse"),
                parse_current_window_compression_rules,
            )?,
        })
    }
}

fn configured_path(env_name: &str) -> Option<PathBuf> {
    std::env::var(env_name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn context_rule_catalogs() -> Result<ContextRuleCatalogs> {
    let cache = CONTEXT_RULES_CACHE.get_or_init(|| Mutex::new(ContextRulesCache::default()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow!("context rules cache is poisoned"))?;
    cache.catalogs()
}

pub(crate) fn context_rule_versions() -> Result<Value> {
    let catalogs = context_rule_catalogs()?;
    Ok(json!({
        "subject_ontology": catalogs.subject_ontology.catalog_version,
        "referent_candidate_rules": catalogs.referent_candidate_rules.catalog_version,
        "ellipsis_resolution_rules": catalogs.ellipsis_resolution_rules.catalog_version,
        "current_window_compression_rules": catalogs.current_window_compression_rules.catalog_version,
    }))
}

pub(crate) fn current_window_compression_policy() -> Result<Value> {
    let rules = context_rule_catalogs()?.current_window_compression_rules;
    Ok(json!({
        "schema_version": rules.schema_version,
        "catalog_version": rules.catalog_version,
        "policy_id": rules.policy_id,
        "max_raw_messages": rules.max_raw_messages,
        "max_raw_chars": rules.max_raw_chars,
        "max_compressor_input_chars": rules.max_compressor_input_chars,
        "must_preserve_user_turns": rules.must_preserve_user_turns,
        "compressor_profile": rules.compressor_profile,
        "digest_schema": rules.digest_schema,
        "timeout_ms": rules.timeout_ms,
        "reject_on_new_entities": rules.reject_on_new_entities,
        "reject_on_missing_source_refs": rules.reject_on_missing_source_refs,
        "reject_on_schema_invalid": rules.reject_on_schema_invalid,
        "allow_rejected_digest_on_main_path": rules.allow_rejected_digest_on_main_path,
    }))
}

pub(crate) fn latest_subject_in_text(text: &str) -> Result<Option<String>> {
    let catalog = context_rule_catalogs()?.subject_ontology;
    let mut best: Option<(usize, usize, String)> = None;
    for subject in catalog.subjects {
        for term in subject_terms(&subject) {
            for (index, _) in text.match_indices(&term) {
                let len = term.chars().count();
                if best.as_ref().is_none_or(|(best_index, best_len, _)| {
                    index > *best_index || (index == *best_index && len > *best_len)
                }) {
                    best = Some((index, len, subject.canonical.clone()));
                }
            }
        }
    }
    Ok(best.map(|(_, _, canonical)| canonical))
}

pub(crate) fn contains_referential_pronoun(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.referent_candidate_rules;
    Ok(contains_any(text, &rules.pronoun_terms))
}

pub(crate) fn bind_referent(question: &str, referent: &str) -> Result<String> {
    let rules = context_rule_catalogs()?.referent_candidate_rules;
    let mut output = question.to_string();
    for needle in rules.replacement_terms {
        if output.contains(&needle) {
            output = output.replacen(&needle, referent, 1);
            break;
        }
    }
    Ok(output)
}

pub(crate) fn is_continue_only_question(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.ellipsis_resolution_rules;
    let text = question_key(text);
    Ok(rules
        .continuation_questions
        .iter()
        .any(|term| question_key(term) == text))
}

pub(crate) fn is_elliptical_followup_question(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.ellipsis_resolution_rules;
    let text_key = question_key(text);
    if rules
        .followup_questions
        .iter()
        .any(|term| question_key(term) == text_key)
    {
        return Ok(true);
    }
    Ok(rules
        .followup_suffix_terms
        .iter()
        .any(|term| text_key.ends_with(&question_key(term))))
}

pub(crate) fn ellipsis_trigger() -> Result<String> {
    Ok(context_rule_catalogs()?.ellipsis_resolution_rules.trigger)
}

pub(crate) fn max_referent_candidates() -> Result<usize> {
    Ok(context_rule_catalogs()?
        .referent_candidate_rules
        .max_candidates)
}

fn subject_terms(subject: &SubjectRule) -> Vec<String> {
    let mut terms = Vec::with_capacity(subject.aliases.len() + 1);
    terms.push(subject.canonical.clone());
    terms.extend(subject.aliases.iter().cloned());
    terms
        .into_iter()
        .map(|term| term.trim().to_string())
        .filter(|term| !term.is_empty())
        .collect()
}

fn contains_any(text: &str, terms: &[String]) -> bool {
    terms
        .iter()
        .map(|term| term.trim())
        .any(|term| !term.is_empty() && text.contains(term))
}

fn question_key(text: &str) -> String {
    text.trim()
        .trim_matches(|ch| matches!(ch, '?' | '？' | '!' | '！' | '。' | '.' | ' '))
        .split_whitespace()
        .collect::<String>()
}

fn parse_subject_ontology(source: &str) -> Result<SubjectOntologyCatalog> {
    let catalog: SubjectOntologyCatalog =
        serde_json::from_str(source).context("subject ontology must be JSON")?;
    if catalog.schema_version != SUBJECT_ONTOLOGY_SCHEMA_VERSION {
        return Err(anyhow!(
            "subject ontology schema_version must be {}",
            SUBJECT_ONTOLOGY_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!("subject ontology catalog_version is required"));
    }
    if catalog.subjects.is_empty() {
        return Err(anyhow!("subject ontology subjects is required"));
    }
    for subject in &catalog.subjects {
        if subject.canonical.trim().is_empty()
            || subject.subject_type.trim().is_empty()
            || subject.work.trim().is_empty()
            || subject.aliases.is_empty()
            || subject.aliases.iter().all(|alias| alias.trim().is_empty())
        {
            return Err(anyhow!(
                "subject ontology subjects require canonical, type, work, and aliases"
            ));
        }
    }
    Ok(catalog)
}

fn parse_referent_candidate_rules(source: &str) -> Result<ReferentCandidateRules> {
    let rules: ReferentCandidateRules =
        serde_json::from_str(source).context("referent candidate rules must be JSON")?;
    if rules.schema_version != REFERENT_CANDIDATE_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "referent candidate rules schema_version must be {}",
            REFERENT_CANDIDATE_RULES_SCHEMA_VERSION
        ));
    }
    if rules.catalog_version.trim().is_empty() || rules.max_candidates == 0 {
        return Err(anyhow!(
            "referent candidate rules require catalog_version and max_candidates"
        ));
    }
    require_non_empty_terms("referent_candidate.pronoun_terms", &rules.pronoun_terms)?;
    require_non_empty_terms(
        "referent_candidate.replacement_terms",
        &rules.replacement_terms,
    )?;
    require_non_empty_terms(
        "referent_candidate.history_reference_terms",
        &rules.history_reference_terms,
    )?;
    require_non_empty_terms("referent_candidate.source_priority", &rules.source_priority)?;
    Ok(rules)
}

fn parse_ellipsis_resolution_rules(source: &str) -> Result<EllipsisResolutionRules> {
    let rules: EllipsisResolutionRules =
        serde_json::from_str(source).context("ellipsis resolution rules must be JSON")?;
    if rules.schema_version != ELLIPSIS_RESOLUTION_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "ellipsis resolution rules schema_version must be {}",
            ELLIPSIS_RESOLUTION_RULES_SCHEMA_VERSION
        ));
    }
    if rules.catalog_version.trim().is_empty()
        || rules.trigger.trim().is_empty()
        || rules.clarification_template.trim().is_empty()
    {
        return Err(anyhow!(
            "ellipsis resolution rules require catalog_version, trigger, and clarification_template"
        ));
    }
    require_non_empty_terms(
        "ellipsis_resolution.continuation_questions",
        &rules.continuation_questions,
    )?;
    require_non_empty_terms(
        "ellipsis_resolution.followup_questions",
        &rules.followup_questions,
    )?;
    require_non_empty_terms(
        "ellipsis_resolution.followup_suffix_terms",
        &rules.followup_suffix_terms,
    )?;
    Ok(rules)
}

fn parse_current_window_compression_rules(source: &str) -> Result<CurrentWindowCompressionRules> {
    let rules: CurrentWindowCompressionRules =
        serde_json::from_str(source).context("current-window compression rules must be JSON")?;
    if rules.schema_version != CURRENT_WINDOW_COMPRESSION_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "current-window compression rules schema_version must be {}",
            CURRENT_WINDOW_COMPRESSION_RULES_SCHEMA_VERSION
        ));
    }
    if rules.catalog_version.trim().is_empty()
        || rules.policy_id.trim().is_empty()
        || rules.compressor_profile.trim().is_empty()
        || rules.digest_schema.trim().is_empty()
        || rules.timeout_ms == 0
        || rules.max_raw_messages == 0
        || rules.max_raw_chars == 0
        || rules.max_compressor_input_chars == 0
        || rules.must_preserve_user_turns == 0
    {
        return Err(anyhow!(
            "current-window compression rules require catalog_version, policy, budgets, profile, schema, and timeout"
        ));
    }
    require_non_empty_terms(
        "current_window_compression.coverage_statuses",
        &rules.coverage_statuses,
    )?;
    if rules.allow_rejected_digest_on_main_path {
        return Err(anyhow!(
            "current-window compression rules cannot allow rejected digest on main path"
        ));
    }
    Ok(rules)
}

fn require_non_empty_terms(name: &str, terms: &[String]) -> Result<()> {
    if terms.is_empty() || terms.iter().all(|term| term.trim().is_empty()) {
        return Err(anyhow!("{name} must define non-empty terms"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalogs_parse_and_expose_versions() {
        let versions = context_rule_versions().expect("versions load");
        assert_eq!(versions["subject_ontology"], json!("2026-05-24.1"));
        assert_eq!(
            current_window_compression_policy().expect("policy loads")["policy_id"],
            json!("current_window.llm_compression.v1")
        );
    }

    #[test]
    fn subject_matching_uses_external_ontology_aliases() {
        assert_eq!(
            latest_subject_in_text("继续说湘云的结局")
                .expect("subject lookup")
                .as_deref(),
            Some("史湘云")
        );
        assert_eq!(
            latest_subject_in_text("黛玉和宝玉分别如何？")
                .expect("subject lookup")
                .as_deref(),
            Some("贾宝玉")
        );
    }

    #[test]
    fn ellipsis_and_referent_terms_are_catalog_driven() {
        assert!(is_continue_only_question("继续？").expect("continue check"));
        assert!(is_elliptical_followup_question("脂批中的证据呢？").expect("ellipsis check"));
        assert!(contains_referential_pronoun("她的结局呢").expect("pronoun check"));
        assert_eq!(
            bind_referent("她的结局呢", "史湘云").expect("binds"),
            "史湘云的结局呢"
        );
    }
}
