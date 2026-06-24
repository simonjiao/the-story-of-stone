use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const SUBJECT_ONTOLOGY_PATH_ENV: &str = "TONGLINGYU_SUBJECT_ONTOLOGY_PATH";
pub(crate) const REFERENT_CANDIDATE_RULES_PATH_ENV: &str =
    "TONGLINGYU_REFERENT_CANDIDATE_RULES_PATH";
pub(crate) const ELLIPSIS_RESOLUTION_RULES_PATH_ENV: &str =
    "TONGLINGYU_ELLIPSIS_RESOLUTION_RULES_PATH";
pub(crate) const CURRENT_WINDOW_COMPRESSION_RULES_PATH_ENV: &str =
    "TONGLINGYU_CURRENT_WINDOW_COMPRESSION_RULES_PATH";
pub(crate) const QUESTION_FRAME_RULES_PATH_ENV: &str = "TONGLINGYU_QUESTION_FRAME_RULES_PATH";

const SUBJECT_ONTOLOGY_SCHEMA_VERSION: &str = "tonglingyu.subject_ontology.v1";
const REFERENT_CANDIDATE_RULES_SCHEMA_VERSION: &str = "tonglingyu.referent_candidate_rules.v1";
const ELLIPSIS_RESOLUTION_RULES_SCHEMA_VERSION: &str = "tonglingyu.ellipsis_resolution_rules.v1";
const CURRENT_WINDOW_COMPRESSION_RULES_SCHEMA_VERSION: &str =
    "tonglingyu.current_window_compression_rules.v1";
const QUESTION_FRAME_RULES_SCHEMA_VERSION: &str = "tonglingyu.question_frame_rules.v1";

const DEFAULT_SUBJECT_ONTOLOGY_JSON: &str = include_str!("../resources/subject_ontology.json");
const DEFAULT_REFERENT_CANDIDATE_RULES_JSON: &str =
    include_str!("../resources/referent_candidate_rules.json");
const DEFAULT_ELLIPSIS_RESOLUTION_RULES_JSON: &str =
    include_str!("../resources/ellipsis_resolution_rules.json");
const DEFAULT_CURRENT_WINDOW_COMPRESSION_RULES_JSON: &str =
    include_str!("../resources/current_window_compression_rules.json");
const DEFAULT_QUESTION_FRAME_RULES_JSON: &str =
    include_str!("../resources/question_frame_rules.json");

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
    contextual_pronoun_terms: Vec<String>,
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
    contextual_continuation: ContextualContinuationRules,
    followup_questions: Vec<String>,
    followup_suffix_terms: Vec<String>,
    trigger: String,
    contextual_followup_template: String,
    clarification_template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextualContinuationRules {
    context_terms: Vec<String>,
    action_terms: Vec<String>,
    rewrite_template: String,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionFrameRules {
    schema_version: String,
    catalog_version: String,
    default_source_scope: String,
    source_scope_phrases: Vec<SourceScopePhraseRule>,
    evidence_followup: EvidenceFollowupRules,
    count_question: CountQuestionRules,
    character_fate_question: CharacterFateQuestionRules,
    chapter_location_question: ChapterLocationQuestionRules,
    attribute_question: AttributeQuestionRules,
    relation_question: RelationQuestionRules,
    predicates: Vec<PredicateRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceScopePhraseRule {
    scope: String,
    phrases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceFollowupRules {
    terms: Vec<String>,
    required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CountQuestionRules {
    terms: Vec<String>,
    required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CharacterFateQuestionRules {
    terms: Vec<String>,
    required_evidence_types: Vec<String>,
    later_forty_required_evidence_types: Vec<String>,
    clarification_template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChapterLocationQuestionRules {
    question_terms: Vec<String>,
    location_verbs: Vec<String>,
    removable_terms: Vec<String>,
    required_evidence_types: Vec<String>,
    clarification_template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributeQuestionRules {
    event_markers: Vec<String>,
    comparison_markers: Vec<String>,
    comparison_target_prefix_terms: Vec<String>,
    comparison_question_terms: Vec<String>,
    attributes: Vec<AttributeRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributeRule {
    id: String,
    label: String,
    aliases: Vec<String>,
    comparison_terms: Vec<String>,
    evidence_terms: Vec<String>,
    required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationQuestionRules {
    object_placeholder_terms: Vec<String>,
    yes_no_terms: Vec<String>,
    followup_prefix_terms: Vec<String>,
    open_object_followup_marker_terms: Vec<String>,
    open_object_followup_suffix_terms: Vec<String>,
    open_object_followup_connector_terms: Vec<String>,
    standalone_entity_query_terms: Vec<String>,
    unknown_predicate_markers: Vec<String>,
    unknown_predicate_candidate_block_terms: Vec<String>,
    unknown_predicate_clarification_template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PredicateRule {
    id: String,
    label: String,
    aliases: Vec<String>,
    evidence_terms: Vec<String>,
    required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PredicateRuleView {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) evidence_terms: Vec<String>,
    pub(crate) required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeRuleView {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) comparison_terms: Vec<String>,
    pub(crate) evidence_terms: Vec<String>,
    pub(crate) required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubjectMentionView {
    pub(crate) canonical: String,
    pub(crate) start: usize,
    pub(crate) len: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RelationQuestionParse {
    pub(crate) predicate: PredicateRuleView,
    pub(crate) subject: Option<String>,
    pub(crate) object: Option<String>,
    pub(crate) open_slot: Option<String>,
    pub(crate) explicit_open_slot: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct UnknownRelationQuestionParse {
    pub(crate) subject: Option<String>,
    pub(crate) object: Option<String>,
    pub(crate) open_slot: Option<String>,
    pub(crate) predicate_candidate_term: Option<String>,
    pub(crate) clarification_question: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AttributeQuestionParse {
    pub(crate) intent: String,
    pub(crate) attribute: AttributeRuleView,
    pub(crate) subject: Option<String>,
    pub(crate) object: Option<String>,
    pub(crate) open_slot: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChapterLocationQuestionParse {
    pub(crate) subject: Option<String>,
    pub(crate) event_phrase: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuleCandidateActiveMatch {
    pub(crate) candidate_type: String,
    pub(crate) rule_ref: String,
}

#[derive(Debug, Clone)]
struct ContextRuleCatalogs {
    subject_ontology: SubjectOntologyCatalog,
    referent_candidate_rules: ReferentCandidateRules,
    ellipsis_resolution_rules: EllipsisResolutionRules,
    current_window_compression_rules: CurrentWindowCompressionRules,
    question_frame_rules: QuestionFrameRules,
}

#[derive(Debug, Clone)]
struct PredicateRuleMatch {
    rule: PredicateRule,
    start: usize,
    len: usize,
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

    fn metadata(&self, schema_version: &str, catalog_version: &str) -> Value {
        json!({
            "schema_version": schema_version,
            "catalog_version": catalog_version,
            "source": if self.path.is_some() { "external_file" } else { "embedded_default" },
            "path": self.path.as_ref().map(|path| path.display().to_string()),
            "mtime_unix_ms": self.modified.and_then(system_time_unix_ms),
            "len": self.len,
        })
    }
}

#[derive(Debug, Clone)]
struct ContextRulesCache {
    subject_ontology: RuleFileCache<SubjectOntologyCatalog>,
    referent_candidate_rules: RuleFileCache<ReferentCandidateRules>,
    ellipsis_resolution_rules: RuleFileCache<EllipsisResolutionRules>,
    current_window_compression_rules: RuleFileCache<CurrentWindowCompressionRules>,
    question_frame_rules: RuleFileCache<QuestionFrameRules>,
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
            question_frame_rules: RuleFileCache::new(
                parse_question_frame_rules(DEFAULT_QUESTION_FRAME_RULES_JSON)
                    .expect("embedded question-frame rules must parse"),
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
            question_frame_rules: self.question_frame_rules.catalog(
                QUESTION_FRAME_RULES_PATH_ENV,
                configured_path(QUESTION_FRAME_RULES_PATH_ENV),
                parse_question_frame_rules(DEFAULT_QUESTION_FRAME_RULES_JSON)
                    .expect("embedded question-frame rules must parse"),
                parse_question_frame_rules,
            )?,
        })
    }

    fn metadata(&mut self) -> Result<Value> {
        let catalogs = self.catalogs()?;
        Ok(json!({
            "subject_ontology": self.subject_ontology.metadata(
                SUBJECT_ONTOLOGY_SCHEMA_VERSION,
                &catalogs.subject_ontology.catalog_version,
            ),
            "referent_candidate_rules": self.referent_candidate_rules.metadata(
                REFERENT_CANDIDATE_RULES_SCHEMA_VERSION,
                &catalogs.referent_candidate_rules.catalog_version,
            ),
            "ellipsis_resolution_rules": self.ellipsis_resolution_rules.metadata(
                ELLIPSIS_RESOLUTION_RULES_SCHEMA_VERSION,
                &catalogs.ellipsis_resolution_rules.catalog_version,
            ),
            "current_window_compression_rules": self.current_window_compression_rules.metadata(
                CURRENT_WINDOW_COMPRESSION_RULES_SCHEMA_VERSION,
                &catalogs.current_window_compression_rules.catalog_version,
            ),
            "question_frame_rules": self.question_frame_rules.metadata(
                QUESTION_FRAME_RULES_SCHEMA_VERSION,
                &catalogs.question_frame_rules.catalog_version,
            ),
        }))
    }
}

fn system_time_unix_ms(value: SystemTime) -> Option<u128> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
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
        "question_frame_rules": catalogs.question_frame_rules.catalog_version,
    }))
}

pub(crate) fn context_rule_catalog_metadata() -> Result<Value> {
    let cache = CONTEXT_RULES_CACHE.get_or_init(|| Mutex::new(ContextRulesCache::default()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow!("context rules cache is poisoned"))?;
    cache.metadata()
}

pub(crate) fn validate_subject_ontology_source(source: &str) -> Result<()> {
    parse_subject_ontology(source).map(|_| ())
}

pub(crate) fn validate_ellipsis_resolution_rules_source(source: &str) -> Result<()> {
    parse_ellipsis_resolution_rules(source).map(|_| ())
}

pub(crate) fn validate_question_frame_rules_source(source: &str) -> Result<()> {
    parse_question_frame_rules(source).map(|_| ())
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
    Ok(subject_mentions_in_text(text)?.into_iter().next_back())
}

pub(crate) fn subject_mentions_in_text(text: &str) -> Result<Vec<String>> {
    Ok(subject_mentions_with_positions(text)?
        .into_iter()
        .map(|mention| mention.canonical)
        .collect())
}

pub(crate) fn subject_mentions_with_positions(text: &str) -> Result<Vec<SubjectMentionView>> {
    let catalog = context_rule_catalogs()?.subject_ontology;
    let mut matches = Vec::<SubjectMentionView>::new();
    let mut seen = Vec::<String>::new();
    for subject in catalog.subjects {
        for term in subject_terms(&subject) {
            for (index, _) in text.match_indices(&term) {
                let len = term.chars().count();
                matches.push(SubjectMentionView {
                    canonical: subject.canonical.clone(),
                    start: index,
                    len,
                });
            }
        }
    }
    matches.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| right.len.cmp(&left.len))
    });
    let mut subjects = Vec::new();
    for mention in matches {
        if !seen.iter().any(|item| item == &mention.canonical) {
            seen.push(mention.canonical.clone());
            subjects.push(mention);
        }
    }
    Ok(subjects)
}

pub(crate) fn subject_aliases(canonical: &str) -> Result<Vec<String>> {
    let catalog = context_rule_catalogs()?.subject_ontology;
    let Some(subject) = catalog
        .subjects
        .into_iter()
        .find(|subject| subject.canonical == canonical)
    else {
        return Err(anyhow!(
            "subject ontology does not contain canonical subject"
        ));
    };
    Ok(subject_terms(&subject))
}

pub(crate) fn predicate_in_text(text: &str) -> Result<Option<PredicateRuleView>> {
    Ok(predicate_match_in_text(text)?.map(|matched| predicate_rule_view(matched.rule)))
}

fn predicate_match_in_text(text: &str) -> Result<Option<PredicateRuleMatch>> {
    let catalog = context_rule_catalogs()?.question_frame_rules;
    let mut best: Option<(usize, usize, String)> = None;
    let mut best_rule: Option<PredicateRule> = None;
    for rule in catalog.predicates {
        for term in &rule.aliases {
            for (index, _) in text.match_indices(term.as_str()) {
                let len = term.chars().count();
                if best.as_ref().is_none_or(|(best_index, best_len, _)| {
                    index > *best_index || (index == *best_index && len > *best_len)
                }) {
                    best = Some((index, len, rule.id.clone()));
                    best_rule = Some(rule.clone());
                }
            }
        }
    }
    Ok(best_rule.map(|rule| {
        let (start, len, _) = best.expect("best match exists when best_rule exists");
        PredicateRuleMatch { rule, start, len }
    }))
}

pub(crate) fn predicate_by_id(id: &str) -> Result<Option<PredicateRuleView>> {
    let catalog = context_rule_catalogs()?.question_frame_rules;
    Ok(catalog
        .predicates
        .into_iter()
        .find(|rule| rule.id == id)
        .map(predicate_rule_view))
}

pub(crate) fn attribute_by_id(id: &str) -> Result<Option<AttributeRuleView>> {
    let catalog = context_rule_catalogs()?.question_frame_rules;
    Ok(catalog
        .attribute_question
        .attributes
        .into_iter()
        .find(|rule| rule.id == id)
        .map(attribute_rule_view))
}

pub(crate) fn parse_attribute_question(text: &str) -> Result<Option<AttributeQuestionParse>> {
    let catalog = context_rule_catalogs()?.question_frame_rules;
    let Some(attribute) = attribute_match_in_text(text, &catalog.attribute_question) else {
        return Ok(None);
    };
    let mentions = subject_mentions_with_positions(text)?;
    let comparison_marked = contains_any(text, &catalog.attribute_question.comparison_markers);
    let comparison_question = comparison_marked
        && (mentions.len() >= 2
            || contains_any(text, &catalog.attribute_question.comparison_question_terms)
            || contains_any(text, &attribute.comparison_terms));
    if comparison_question {
        let (subject, object, open_slot) = attribute_compare_entities(
            text,
            &mentions,
            &catalog.attribute_question.comparison_target_prefix_terms,
            &catalog.attribute_question.comparison_markers,
        )?;
        return Ok(Some(AttributeQuestionParse {
            intent: "attribute_compare".to_string(),
            attribute: attribute_rule_view(attribute),
            subject,
            object,
            open_slot,
        }));
    }
    let Some(subject) = mentions.first().map(|mention| mention.canonical.clone()) else {
        return Ok(None);
    };
    let intent = if contains_any(text, &catalog.attribute_question.event_markers) {
        "attribute_at_event"
    } else {
        "attribute_query"
    };
    Ok(Some(AttributeQuestionParse {
        intent: intent.to_string(),
        attribute: attribute_rule_view(attribute),
        subject: Some(subject),
        object: None,
        open_slot: None,
    }))
}

pub(crate) fn parse_relation_question(text: &str) -> Result<Option<RelationQuestionParse>> {
    let Some(predicate_match) = predicate_match_in_text(text)? else {
        return Ok(None);
    };
    let placeholder_open_slot = relation_question_open_slot_from_placeholder(text)?;
    let mentions = subject_mentions_with_positions(text)?;
    let subject_before_predicate = mentions
        .iter()
        .rev()
        .find(|mention| mention.start < predicate_match.start)
        .map(|mention| mention.canonical.clone());
    let object_after_predicate = mentions
        .iter()
        .find(|mention| mention.start >= predicate_match.start + predicate_match.len)
        .map(|mention| mention.canonical.clone());

    let (subject, object, implicit_open_slot) = match placeholder_open_slot.as_deref() {
        Some("subject") => (
            None,
            object_after_predicate
                .or_else(|| mentions.first().map(|mention| mention.canonical.clone())),
            None,
        ),
        Some("object") => (
            subject_before_predicate
                .or_else(|| mentions.first().map(|mention| mention.canonical.clone())),
            None,
            None,
        ),
        _ => {
            let subject = subject_before_predicate.or_else(|| {
                mentions
                    .iter()
                    .find(|mention| mention.start < predicate_match.start + predicate_match.len)
                    .map(|mention| mention.canonical.clone())
            });
            let object = object_after_predicate.or_else(|| {
                mentions
                    .iter()
                    .find(|mention| {
                        subject
                            .as_ref()
                            .is_none_or(|subject| &mention.canonical != subject)
                    })
                    .map(|mention| mention.canonical.clone())
            });
            let implicit_open_slot = if subject.is_none() {
                Some("subject".to_string())
            } else if object.is_none() {
                Some("object".to_string())
            } else {
                None
            };
            (subject, object, implicit_open_slot)
        }
    };

    Ok(Some(RelationQuestionParse {
        predicate: predicate_rule_view(predicate_match.rule),
        subject,
        object,
        open_slot: placeholder_open_slot.clone().or(implicit_open_slot),
        explicit_open_slot: placeholder_open_slot.is_some(),
    }))
}

pub(crate) fn parse_unknown_relation_predicate_question(
    text: &str,
) -> Result<Option<UnknownRelationQuestionParse>> {
    let catalogs = context_rule_catalogs()?;
    let rules = catalogs.question_frame_rules.relation_question;
    if predicate_match_in_text(text)?.is_some()
        || contains_any(text, &rules.standalone_entity_query_terms)
    {
        return Ok(None);
    }

    let mentions = subject_mentions_with_positions(text)?;
    let placeholder = first_relation_placeholder(text, &rules.object_placeholder_terms);
    let relation_marked = placeholder.is_some()
        || contains_any(text, &rules.yes_no_terms)
        || contains_any(text, &rules.unknown_predicate_markers);
    if !relation_marked || (mentions.len() < 2 && !(mentions.len() == 1 && placeholder.is_some())) {
        return Ok(None);
    }

    let (subject, object, open_slot) = match placeholder {
        Some((placeholder_start, _)) if mentions.len() == 1 => {
            let mention = mentions[0].canonical.clone();
            if placeholder_start < mentions[0].start {
                (None, Some(mention), Some("subject".to_string()))
            } else {
                (Some(mention), None, Some("object".to_string()))
            }
        }
        Some((placeholder_start, _)) => {
            if placeholder_start < mentions[0].start {
                (
                    None,
                    Some(mentions[0].canonical.clone()),
                    Some("subject".to_string()),
                )
            } else {
                (
                    Some(mentions[0].canonical.clone()),
                    None,
                    Some("object".to_string()),
                )
            }
        }
        None => (
            Some(mentions[0].canonical.clone()),
            Some(mentions[1].canonical.clone()),
            None,
        ),
    };
    let predicate_candidate_term = unknown_relation_predicate_candidate_term_from_parts(
        text,
        &rules,
        subject.as_deref(),
        object.as_deref(),
    )?;

    Ok(Some(UnknownRelationQuestionParse {
        clarification_question: relation_clarification_from_template(
            &rules.unknown_predicate_clarification_template,
            subject.as_deref(),
            object.as_deref(),
        ),
        subject,
        object,
        open_slot,
        predicate_candidate_term,
    }))
}

pub(crate) fn parse_chapter_location_question(
    text: &str,
) -> Result<Option<ChapterLocationQuestionParse>> {
    let catalogs = context_rule_catalogs()?;
    let rules = catalogs.question_frame_rules.chapter_location_question;
    if !contains_any(text, &rules.question_terms) {
        return Ok(None);
    }
    let mentions = subject_mentions_with_positions(text)?;
    let subject = mentions.first().map(|mention| mention.canonical.clone());
    let event_phrase = chapter_location_event_phrase(text, &rules, &mentions)?;
    Ok(Some(ChapterLocationQuestionParse {
        subject,
        event_phrase,
    }))
}

pub(crate) fn chapter_location_required_evidence_types() -> Result<Vec<String>> {
    Ok(context_rule_catalogs()?
        .question_frame_rules
        .chapter_location_question
        .required_evidence_types)
}

pub(crate) fn chapter_location_clarification_question() -> Result<String> {
    Ok(context_rule_catalogs()?
        .question_frame_rules
        .chapter_location_question
        .clarification_template)
}

pub(crate) fn unknown_relation_predicate_candidate_term(text: &str) -> Result<Option<String>> {
    Ok(parse_unknown_relation_predicate_question(text)?
        .and_then(|parsed| parsed.predicate_candidate_term))
}

pub(crate) fn active_rule_candidate_matches(
    _candidate_type: &str,
    term: &str,
) -> Result<Vec<RuleCandidateActiveMatch>> {
    let catalogs = context_rule_catalogs()?;
    let term_key = rule_candidate_term_key(term);
    if term_key.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for subject in &catalogs.subject_ontology.subjects {
        for active_term in subject_terms(subject) {
            push_active_candidate_match(
                &mut matches,
                &term_key,
                "entity_alias",
                &active_term,
                &format!("subject_ontology.subject:{}", subject.canonical),
            );
        }
    }

    for predicate in &catalogs.question_frame_rules.predicates {
        for active_term in std::iter::once(predicate.label.clone()).chain(predicate.aliases.clone())
        {
            push_active_candidate_match(
                &mut matches,
                &term_key,
                "predicate_alias",
                &active_term,
                &format!("question_frame_rules.predicate:{}", predicate.id),
            );
        }
    }

    let relation = &catalogs.question_frame_rules.relation_question;
    for scope_rule in &catalogs.question_frame_rules.source_scope_phrases {
        for active_term in &scope_rule.phrases {
            push_active_candidate_match(
                &mut matches,
                &term_key,
                "source_scope_phrase",
                active_term,
                &format!(
                    "question_frame_rules.source_scope_phrases:{}",
                    scope_rule.scope
                ),
            );
        }
    }
    for active_term in &catalogs.question_frame_rules.evidence_followup.terms {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "evidence_followup_term",
            active_term,
            "question_frame_rules.evidence_followup.terms",
        );
    }
    for active_term in &catalogs.question_frame_rules.count_question.terms {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "count_question_term",
            active_term,
            "question_frame_rules.count_question.terms",
        );
    }
    for active_term in &catalogs.question_frame_rules.character_fate_question.terms {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "character_fate_question_term",
            active_term,
            "question_frame_rules.character_fate_question.terms",
        );
    }
    for active_term in &catalogs
        .question_frame_rules
        .chapter_location_question
        .question_terms
    {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "chapter_location_question_term",
            active_term,
            "question_frame_rules.chapter_location_question.question_terms",
        );
    }
    for active_term in &catalogs
        .question_frame_rules
        .chapter_location_question
        .location_verbs
    {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "chapter_location_location_verb",
            active_term,
            "question_frame_rules.chapter_location_question.location_verbs",
        );
    }
    for attribute in &catalogs.question_frame_rules.attribute_question.attributes {
        for active_term in std::iter::once(attribute.label.clone())
            .chain(attribute.aliases.clone())
            .chain(attribute.comparison_terms.clone())
            .chain(attribute.evidence_terms.clone())
        {
            push_active_candidate_match(
                &mut matches,
                &term_key,
                "attribute_term",
                &active_term,
                &format!("question_frame_rules.attribute:{}", attribute.id),
            );
        }
    }
    for active_term in &relation.open_object_followup_marker_terms {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "open_object_followup_marker",
            active_term,
            "question_frame_rules.relation_question.open_object_followup_marker_terms",
        );
    }
    for active_term in &relation.open_object_followup_suffix_terms {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "open_object_followup_suffix",
            active_term,
            "question_frame_rules.relation_question.open_object_followup_suffix_terms",
        );
    }
    for active_term in &relation.open_object_followup_connector_terms {
        push_active_candidate_match(
            &mut matches,
            &term_key,
            "open_object_followup_connector",
            active_term,
            "question_frame_rules.relation_question.open_object_followup_connector_terms",
        );
    }
    push_active_candidate_match(
        &mut matches,
        &term_key,
        "source_scope_phrase",
        &catalogs.question_frame_rules.default_source_scope,
        "question_frame_rules.default_source_scope",
    );
    push_active_candidate_match(
        &mut matches,
        &term_key,
        "clarification_pattern",
        &catalogs.ellipsis_resolution_rules.clarification_template,
        "ellipsis_resolution_rules.clarification_template",
    );
    Ok(matches)
}

fn push_active_candidate_match(
    matches: &mut Vec<RuleCandidateActiveMatch>,
    term_key: &str,
    active_candidate_type: &str,
    active_term: &str,
    rule_ref: &str,
) {
    if rule_candidate_term_key(active_term) != term_key {
        return;
    }
    matches.push(RuleCandidateActiveMatch {
        candidate_type: active_candidate_type.to_string(),
        rule_ref: rule_ref.to_string(),
    });
}

pub(crate) fn question_source_scope(text: &str) -> Result<String> {
    let rules = context_rule_catalogs()?.question_frame_rules;
    let mut best: Option<(usize, usize, String)> = None;
    for scope_rule in rules.source_scope_phrases {
        for phrase in scope_rule.phrases {
            let phrase = phrase.trim();
            if phrase.is_empty() {
                continue;
            }
            for (index, _) in text.match_indices(phrase) {
                let len = phrase.chars().count();
                if best.as_ref().is_none_or(|(best_index, best_len, _)| {
                    index > *best_index || (index == *best_index && len > *best_len)
                }) {
                    best = Some((index, len, scope_rule.scope.clone()));
                }
            }
        }
    }
    Ok(best
        .map(|(_, _, scope)| scope)
        .unwrap_or(rules.default_source_scope))
}

pub(crate) fn question_mentions_source_scope(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.question_frame_rules;
    Ok(rules
        .source_scope_phrases
        .iter()
        .any(|scope_rule| contains_any(text, &scope_rule.phrases)))
}

pub(crate) fn question_mentions_evidence_followup(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.question_frame_rules;
    Ok(contains_any(text, &rules.evidence_followup.terms))
}

pub(crate) fn evidence_followup_required_evidence_types() -> Result<Vec<String>> {
    Ok(context_rule_catalogs()?
        .question_frame_rules
        .evidence_followup
        .required_evidence_types)
}

pub(crate) fn question_mentions_count(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.question_frame_rules;
    Ok(contains_any(text, &rules.count_question.terms))
}

pub(crate) fn count_question_required_evidence_types() -> Result<Vec<String>> {
    Ok(context_rule_catalogs()?
        .question_frame_rules
        .count_question
        .required_evidence_types)
}

pub(crate) fn question_mentions_character_fate(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.question_frame_rules;
    Ok(contains_any(text, &rules.character_fate_question.terms))
}

pub(crate) fn character_fate_required_evidence_types(source_scope: &str) -> Result<Vec<String>> {
    let rules = context_rule_catalogs()?
        .question_frame_rules
        .character_fate_question;
    if source_scope == "later_40_base_text" {
        Ok(rules.later_forty_required_evidence_types)
    } else {
        Ok(rules.required_evidence_types)
    }
}

pub(crate) fn character_fate_clarification_question() -> Result<String> {
    Ok(context_rule_catalogs()?
        .question_frame_rules
        .character_fate_question
        .clarification_template)
}

pub(crate) fn relation_question_open_slot_from_placeholder(text: &str) -> Result<Option<String>> {
    let rules = context_rule_catalogs()?
        .question_frame_rules
        .relation_question;
    let Some(predicate) = predicate_match_in_text(text)? else {
        return Ok(None);
    };
    let best_placeholder = first_relation_placeholder(text, &rules.object_placeholder_terms);
    Ok(best_placeholder.map(|(index, _)| {
        if index < predicate.start {
            "subject".to_string()
        } else {
            "object".to_string()
        }
    }))
}

pub(crate) fn relation_followup_can_fill_open_object(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?
        .question_frame_rules
        .relation_question;
    if contains_any(text, &rules.standalone_entity_query_terms) {
        return Ok(false);
    }
    let key = question_key(text);
    let has_prefix = rules
        .followup_prefix_terms
        .iter()
        .map(|term| question_key(term))
        .any(|term| !term.is_empty() && key.starts_with(&term));
    let has_suffix = rules
        .open_object_followup_suffix_terms
        .iter()
        .map(|term| question_key(term))
        .any(|term| !term.is_empty() && key.ends_with(&term));
    Ok(has_prefix
        || has_suffix
        || contains_any(text, &rules.yes_no_terms)
        || contains_any(text, &rules.open_object_followup_marker_terms))
}

pub(crate) fn relation_followup_has_only_open_object_terms(
    text: &str,
    canonical_subject: &str,
) -> Result<bool> {
    let catalogs = context_rule_catalogs()?;
    let rules = catalogs.question_frame_rules.relation_question;
    let mut allowed_terms = Vec::new();
    if let Some(subject) = catalogs
        .subject_ontology
        .subjects
        .into_iter()
        .find(|subject| subject.canonical == canonical_subject)
    {
        allowed_terms.extend(subject_terms(&subject));
    } else {
        allowed_terms.push(canonical_subject.to_string());
    }
    allowed_terms.extend(rules.followup_prefix_terms);
    allowed_terms.extend(rules.open_object_followup_marker_terms);
    allowed_terms.extend(rules.open_object_followup_suffix_terms);
    allowed_terms.extend(rules.open_object_followup_connector_terms);
    allowed_terms.extend(rules.yes_no_terms);
    allowed_terms.sort_by_key(|term| std::cmp::Reverse(question_key(term).chars().count()));

    let mut residual = question_key(text);
    for term in allowed_terms {
        let key = question_key(&term);
        if !key.is_empty() {
            residual = residual.replace(&key, "");
        }
    }
    Ok(residual.is_empty())
}

fn attribute_match_in_text(text: &str, rules: &AttributeQuestionRules) -> Option<AttributeRule> {
    let comparison_marked = contains_any(text, &rules.comparison_markers);
    let mut best: Option<(usize, AttributeRule)> = None;
    for rule in &rules.attributes {
        for term in &rule.aliases {
            let term = term.trim();
            if !term.is_empty() && text.contains(term) {
                let len = term.chars().count();
                if best.as_ref().is_none_or(|(best_len, _)| len > *best_len) {
                    best = Some((len, rule.clone()));
                }
            }
        }
        if comparison_marked {
            for term in &rule.comparison_terms {
                let term = term.trim();
                if !term.is_empty() && text.contains(term) {
                    let len = term.chars().count();
                    if best.as_ref().is_none_or(|(best_len, _)| len > *best_len) {
                        best = Some((len, rule.clone()));
                    }
                }
            }
        }
    }
    best.map(|(_, rule)| rule)
}

fn attribute_compare_entities(
    text: &str,
    mentions: &[SubjectMentionView],
    target_prefix_terms: &[String],
    comparison_markers: &[String],
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    if mentions.len() >= 2 {
        return Ok((
            Some(mentions[0].canonical.clone()),
            Some(mentions[1].canonical.clone()),
            None,
        ));
    }
    let Some(mention) = mentions.first() else {
        return Ok((None, None, Some("subject".to_string())));
    };
    if comparison_target_is_context_object(
        text,
        &mention.canonical,
        target_prefix_terms,
        comparison_markers,
    )? {
        Ok((
            None,
            Some(mention.canonical.clone()),
            Some("subject".to_string()),
        ))
    } else {
        Ok((
            Some(mention.canonical.clone()),
            None,
            Some("object".to_string()),
        ))
    }
}

fn comparison_target_is_context_object(
    text: &str,
    canonical: &str,
    target_prefix_terms: &[String],
    comparison_markers: &[String],
) -> Result<bool> {
    let aliases = subject_aliases(canonical)?;
    for alias in aliases {
        for prefix in target_prefix_terms {
            for marker in comparison_markers {
                if text.contains(&format!("{prefix}{alias}{marker}")) {
                    return Ok(true);
                }
                if marker != "比" && text.contains(&format!("{marker}{alias}")) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn attribute_rule_view(rule: AttributeRule) -> AttributeRuleView {
    AttributeRuleView {
        id: rule.id,
        label: rule.label,
        aliases: rule.aliases,
        comparison_terms: rule.comparison_terms,
        evidence_terms: rule.evidence_terms,
        required_evidence_types: rule.required_evidence_types,
    }
}

fn predicate_rule_view(rule: PredicateRule) -> PredicateRuleView {
    PredicateRuleView {
        id: rule.id,
        label: rule.label,
        aliases: rule.aliases,
        evidence_terms: rule.evidence_terms,
        required_evidence_types: rule.required_evidence_types,
    }
}

fn first_relation_placeholder(text: &str, placeholder_terms: &[String]) -> Option<(usize, usize)> {
    let mut best_placeholder: Option<(usize, usize)> = None;
    for term in placeholder_terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        for (index, _) in text.match_indices(term) {
            let len = term.chars().count();
            if best_placeholder
                .as_ref()
                .is_none_or(|(best_index, best_len)| {
                    index < *best_index || (index == *best_index && len > *best_len)
                })
            {
                best_placeholder = Some((index, len));
            }
        }
    }
    best_placeholder
}

fn chapter_location_event_phrase(
    text: &str,
    rules: &ChapterLocationQuestionRules,
    mentions: &[SubjectMentionView],
) -> Result<Option<String>> {
    let mut residual = question_key(text);
    let mut removable_terms = rules.removable_terms.clone();
    removable_terms.extend(rules.question_terms.clone());
    removable_terms.extend(rules.location_verbs.clone());
    removable_terms.sort_by_key(|term| std::cmp::Reverse(question_key(term).chars().count()));
    for term in removable_terms {
        let key = question_key(&term);
        if !key.is_empty() {
            residual = residual.replace(&key, "");
        }
    }
    for mention in mentions {
        for alias in subject_aliases(&mention.canonical)? {
            let key = question_key(&alias);
            if !key.is_empty() {
                residual = residual.replace(&key, "");
            }
        }
    }
    let phrase = question_key(&residual);
    if phrase.chars().count() >= 2 {
        Ok(Some(phrase))
    } else {
        Ok(None)
    }
}

fn relation_clarification_from_template(
    template: &str,
    subject: Option<&str>,
    object: Option<&str>,
) -> String {
    template
        .replace("{subject}", subject.unwrap_or("主体人物"))
        .replace("{object}", object.unwrap_or("对象人物"))
}

fn unknown_relation_predicate_candidate_term_from_parts(
    text: &str,
    rules: &RelationQuestionRules,
    subject: Option<&str>,
    object: Option<&str>,
) -> Result<Option<String>> {
    let mut residual = question_key(text);
    for canonical in [subject, object].into_iter().flatten() {
        for alias in subject_aliases(canonical)? {
            let alias_key = question_key(&alias);
            if !alias_key.is_empty() {
                residual = residual.replace(&alias_key, "");
            }
        }
    }

    let mut removable_terms = Vec::new();
    removable_terms.extend(rules.object_placeholder_terms.iter().cloned());
    removable_terms.extend(rules.yes_no_terms.iter().cloned());
    removable_terms.extend(rules.followup_prefix_terms.iter().cloned());
    removable_terms.extend(rules.open_object_followup_marker_terms.iter().cloned());
    removable_terms.extend(rules.open_object_followup_suffix_terms.iter().cloned());
    removable_terms.extend(rules.open_object_followup_connector_terms.iter().cloned());
    removable_terms.extend(rules.unknown_predicate_markers.iter().cloned());
    removable_terms.sort_by_key(|term| std::cmp::Reverse(question_key(term).chars().count()));
    for term in removable_terms {
        let key = question_key(&term);
        if !key.is_empty() {
            residual = residual.replace(&key, "");
        }
    }

    let term = question_key(&residual);
    if term.is_empty()
        || term.chars().count() > 80
        || contains_any(&term, &rules.unknown_predicate_candidate_block_terms)
    {
        return Ok(None);
    }
    Ok(Some(term))
}

pub(crate) fn contains_referential_pronoun(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.referent_candidate_rules;
    Ok(contains_any(text, &rules.pronoun_terms)
        || contains_any(text, &rules.contextual_pronoun_terms))
}

pub(crate) fn contains_strong_referential_pronoun(text: &str) -> Result<bool> {
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

pub(crate) fn is_contextual_continuation_question(text: &str) -> Result<bool> {
    let rules = context_rule_catalogs()?.ellipsis_resolution_rules;
    let text_key = question_key(text);
    Ok(
        contains_any(&text_key, &rules.contextual_continuation.context_terms)
            && contains_any(&text_key, &rules.contextual_continuation.action_terms),
    )
}

pub(crate) fn resolve_contextual_continuation(
    question: &str,
    anchor: &str,
) -> Result<Option<String>> {
    if !is_contextual_continuation_question(question)? {
        return Ok(None);
    }
    let rules = context_rule_catalogs()?.ellipsis_resolution_rules;
    let question = question.trim();
    let anchor = anchor.trim();
    if question.is_empty() || anchor.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        rules
            .contextual_continuation
            .rewrite_template
            .replace("{anchor}", anchor)
            .replace("{question}", question),
    ))
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

pub(crate) fn resolve_elliptical_followup(question: &str, anchor: &str) -> Result<Option<String>> {
    if !is_elliptical_followup_question(question)? {
        return Ok(None);
    }
    let rules = context_rule_catalogs()?.ellipsis_resolution_rules;
    let question = question.trim();
    let anchor = anchor.trim();
    if question.is_empty() || anchor.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        rules
            .contextual_followup_template
            .replace("{anchor}", anchor)
            .replace("{question}", question),
    ))
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

pub(crate) fn rule_candidate_term_key(text: &str) -> String {
    question_key(text).to_lowercase()
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
        "referent_candidate.contextual_pronoun_terms",
        &rules.contextual_pronoun_terms,
    )?;
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
        || rules.contextual_followup_template.trim().is_empty()
        || rules.clarification_template.trim().is_empty()
    {
        return Err(anyhow!(
            "ellipsis resolution rules require catalog_version, trigger, contextual_followup_template, and clarification_template"
        ));
    }
    if !rules.contextual_followup_template.contains("{anchor}")
        || !rules.contextual_followup_template.contains("{question}")
    {
        return Err(anyhow!(
            "ellipsis resolution contextual_followup_template must include {{anchor}} and {{question}}"
        ));
    }
    require_non_empty_terms(
        "ellipsis_resolution.continuation_questions",
        &rules.continuation_questions,
    )?;
    require_non_empty_terms(
        "ellipsis_resolution.contextual_continuation.context_terms",
        &rules.contextual_continuation.context_terms,
    )?;
    require_non_empty_terms(
        "ellipsis_resolution.contextual_continuation.action_terms",
        &rules.contextual_continuation.action_terms,
    )?;
    if rules
        .contextual_continuation
        .rewrite_template
        .trim()
        .is_empty()
        || !rules
            .contextual_continuation
            .rewrite_template
            .contains("{anchor}")
        || !rules
            .contextual_continuation
            .rewrite_template
            .contains("{question}")
    {
        return Err(anyhow!(
            "ellipsis resolution contextual_continuation.rewrite_template must include {{anchor}} and {{question}}"
        ));
    }
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

fn parse_question_frame_rules(source: &str) -> Result<QuestionFrameRules> {
    let rules: QuestionFrameRules =
        serde_json::from_str(source).context("question-frame rules must be JSON")?;
    if rules.schema_version != QUESTION_FRAME_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "question-frame rules schema_version must be {}",
            QUESTION_FRAME_RULES_SCHEMA_VERSION
        ));
    }
    if rules.catalog_version.trim().is_empty() || rules.default_source_scope.trim().is_empty() {
        return Err(anyhow!(
            "question-frame rules require catalog_version and default_source_scope"
        ));
    }
    if rules.source_scope_phrases.is_empty() {
        return Err(anyhow!("question-frame rules require source_scope_phrases"));
    }
    for scope_rule in &rules.source_scope_phrases {
        if scope_rule.scope.trim().is_empty() {
            return Err(anyhow!("question-frame source scope phrase requires scope"));
        }
        require_non_empty_terms(
            "question_frame.source_scope_phrases.phrases",
            &scope_rule.phrases,
        )?;
    }
    require_non_empty_terms(
        "question_frame.evidence_followup.terms",
        &rules.evidence_followup.terms,
    )?;
    require_non_empty_terms(
        "question_frame.evidence_followup.required_evidence_types",
        &rules.evidence_followup.required_evidence_types,
    )?;
    require_non_empty_terms(
        "question_frame.count_question.terms",
        &rules.count_question.terms,
    )?;
    require_non_empty_terms(
        "question_frame.count_question.required_evidence_types",
        &rules.count_question.required_evidence_types,
    )?;
    require_non_empty_terms(
        "question_frame.character_fate_question.terms",
        &rules.character_fate_question.terms,
    )?;
    require_non_empty_terms(
        "question_frame.character_fate_question.required_evidence_types",
        &rules.character_fate_question.required_evidence_types,
    )?;
    require_non_empty_terms(
        "question_frame.character_fate_question.later_forty_required_evidence_types",
        &rules
            .character_fate_question
            .later_forty_required_evidence_types,
    )?;
    if rules
        .character_fate_question
        .clarification_template
        .trim()
        .is_empty()
    {
        return Err(anyhow!(
            "question-frame character fate question requires clarification template"
        ));
    }
    require_non_empty_terms(
        "question_frame.chapter_location_question.question_terms",
        &rules.chapter_location_question.question_terms,
    )?;
    require_non_empty_terms(
        "question_frame.chapter_location_question.location_verbs",
        &rules.chapter_location_question.location_verbs,
    )?;
    require_non_empty_terms(
        "question_frame.chapter_location_question.removable_terms",
        &rules.chapter_location_question.removable_terms,
    )?;
    require_non_empty_terms(
        "question_frame.chapter_location_question.required_evidence_types",
        &rules.chapter_location_question.required_evidence_types,
    )?;
    if rules
        .chapter_location_question
        .clarification_template
        .trim()
        .is_empty()
    {
        return Err(anyhow!(
            "question-frame chapter location question requires clarification template"
        ));
    }
    require_non_empty_terms(
        "question_frame.attribute_question.event_markers",
        &rules.attribute_question.event_markers,
    )?;
    require_non_empty_terms(
        "question_frame.attribute_question.comparison_markers",
        &rules.attribute_question.comparison_markers,
    )?;
    require_non_empty_terms(
        "question_frame.attribute_question.comparison_target_prefix_terms",
        &rules.attribute_question.comparison_target_prefix_terms,
    )?;
    require_non_empty_terms(
        "question_frame.attribute_question.comparison_question_terms",
        &rules.attribute_question.comparison_question_terms,
    )?;
    if rules.attribute_question.attributes.is_empty() {
        return Err(anyhow!(
            "question-frame rules require attribute_question.attributes"
        ));
    }
    for attribute in &rules.attribute_question.attributes {
        if attribute.id.trim().is_empty() || attribute.label.trim().is_empty() {
            return Err(anyhow!(
                "question-frame attribute rules require id and label"
            ));
        }
        require_non_empty_terms("question_frame.attribute.aliases", &attribute.aliases)?;
        require_non_empty_terms(
            "question_frame.attribute.comparison_terms",
            &attribute.comparison_terms,
        )?;
        require_non_empty_terms(
            "question_frame.attribute.evidence_terms",
            &attribute.evidence_terms,
        )?;
        require_non_empty_terms(
            "question_frame.attribute.required_evidence_types",
            &attribute.required_evidence_types,
        )?;
    }
    require_non_empty_terms(
        "question_frame.relation_question.object_placeholder_terms",
        &rules.relation_question.object_placeholder_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.yes_no_terms",
        &rules.relation_question.yes_no_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.followup_prefix_terms",
        &rules.relation_question.followup_prefix_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.open_object_followup_marker_terms",
        &rules.relation_question.open_object_followup_marker_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.open_object_followup_suffix_terms",
        &rules.relation_question.open_object_followup_suffix_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.open_object_followup_connector_terms",
        &rules.relation_question.open_object_followup_connector_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.standalone_entity_query_terms",
        &rules.relation_question.standalone_entity_query_terms,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.unknown_predicate_markers",
        &rules.relation_question.unknown_predicate_markers,
    )?;
    require_non_empty_terms(
        "question_frame.relation_question.unknown_predicate_candidate_block_terms",
        &rules
            .relation_question
            .unknown_predicate_candidate_block_terms,
    )?;
    if rules
        .relation_question
        .unknown_predicate_clarification_template
        .trim()
        .is_empty()
    {
        return Err(anyhow!(
            "question-frame relation question requires unknown predicate clarification template"
        ));
    }
    if rules.predicates.is_empty() {
        return Err(anyhow!("question-frame rules require predicates"));
    }
    for predicate in &rules.predicates {
        if predicate.id.trim().is_empty() || predicate.label.trim().is_empty() {
            return Err(anyhow!("question-frame predicate requires id and label"));
        }
        require_non_empty_terms("question_frame.predicates.aliases", &predicate.aliases)?;
        require_non_empty_terms(
            "question_frame.predicates.required_evidence_types",
            &predicate.required_evidence_types,
        )?;
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
        assert_eq!(versions["subject_ontology"], json!("2026-06-01.1"));
        assert_eq!(versions["referent_candidate_rules"], json!("2026-05-24.2"));
        assert_eq!(versions["ellipsis_resolution_rules"], json!("2026-06-01.2"));
        assert_eq!(versions["question_frame_rules"], json!("2026-06-02.2"));
        let metadata = context_rule_catalog_metadata().expect("metadata loads");
        assert_eq!(
            metadata["question_frame_rules"]["catalog_version"],
            json!("2026-06-02.2")
        );
        assert_eq!(
            metadata["question_frame_rules"]["source"],
            json!("embedded_default")
        );
        assert!(metadata["question_frame_rules"]["path"].is_null());
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
        assert!(
            is_contextual_continuation_question("你的推理逻辑是什么？")
                .expect("contextual continuation check")
        );
        assert_eq!(
            resolve_contextual_continuation("你的推理逻辑是什么？", "林黛玉进贾府多大了")
                .expect("contextual continuation resolves"),
            Some("关于林黛玉进贾府多大了，你的推理逻辑是什么？".to_string())
        );
        assert!(is_elliptical_followup_question("脂批中的证据呢？").expect("ellipsis check"));
        assert_eq!(
            resolve_elliptical_followup("脂批中的证据呢？", "史湘云的结局")
                .expect("followup resolves"),
            Some("关于史湘云的结局，脂批中的证据呢？".to_string())
        );
        assert!(contains_referential_pronoun("她的结局呢").expect("pronoun check"));
        assert!(contains_strong_referential_pronoun("她的结局呢").expect("strong pronoun check"));
        assert!(contains_referential_pronoun("这个人呢").expect("contextual pronoun check"));
        assert!(
            !contains_strong_referential_pronoun("这个人呢")
                .expect("contextual pronoun is not strong")
        );
        assert_eq!(
            bind_referent("她的结局呢", "史湘云").expect("binds"),
            "史湘云的结局呢"
        );
    }
}

#[cfg(test)]
#[path = "context_rules/tests.rs"]
mod relation_rule_tests;
