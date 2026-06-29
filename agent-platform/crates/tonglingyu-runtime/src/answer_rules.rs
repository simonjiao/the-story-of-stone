use crate::{
    ANSWER_RULES_PATH_ENV, normalize_text,
    rule_catalog::{RuleFileCache, configured_path, lock_rule_cache},
};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

const ANSWER_RULES_SCHEMA_VERSION: &str = "tonglingyu.answer_rules.v1";
const DEFAULT_ANSWER_RULES_JSON: &str = include_str!("../resources/answer_rules.json");

static ANSWER_RULES_CATALOG_CACHE: OnceLock<Mutex<RuleFileCache<AnswerRuleCatalog>>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerRuleCatalog {
    schema_version: String,
    catalog_version: String,
    answer_requirements: AnswerRequirementRules,
    entity_intro: EntityIntroRules,
    chapter_location: ChapterLocationRules,
    attribute_age: AttributeAgeRules,
    rationale_followup: RationaleFollowupRules,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerRequirementRules {
    max_required_evidence_cards: usize,
    max_anchor_cues_per_card: usize,
    max_source_title_cue_chars: usize,
    max_text_anchor_cue_chars: usize,
    text_cue_min_chars: usize,
    evidence_request_requires_text_cue: bool,
    evidence_request_terms: Vec<String>,
    rule: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityIntroRules {
    min_supporting_cards: usize,
    max_supporting_cards: usize,
    max_quote_chars: usize,
    min_substantive_chars: usize,
    short_speech_shell_max_chars: usize,
    fate_default_opening: String,
    fate_later_forty_opening: String,
    fate_evidence_item_template: String,
    fate_default_boundary: String,
    fate_later_forty_boundary: String,
    blocked_question_terms: Vec<String>,
    excluded_public_quote_terms: Vec<String>,
    rule: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChapterLocationRules {
    max_quote_chars: usize,
    max_title_chars: usize,
    dominant_chapter_score_margin: i64,
    weak_mention_markers: Vec<String>,
    direct_answer_template: String,
    chapter_title_template: String,
    base_evidence_template: String,
    commentary_evidence_template: String,
    no_evidence_template: String,
    ambiguous_template: String,
    rule: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributeAgeRules {
    max_entity_age_distance: usize,
    cue_prefix_terms: Vec<String>,
    rule: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RationaleFollowupRules {
    terms: Vec<String>,
    rule: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AnswerCuePolicy {
    pub(crate) max_required_evidence_cards: usize,
    pub(crate) max_anchor_cues_per_card: usize,
    pub(crate) max_source_title_cue_chars: usize,
    pub(crate) max_text_anchor_cue_chars: usize,
    pub(crate) text_cue_min_chars: usize,
    pub(crate) evidence_request: bool,
    pub(crate) require_text_cue: bool,
    pub(crate) rule: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EntityIntroPolicy {
    pub(crate) min_supporting_cards: usize,
    pub(crate) max_supporting_cards: usize,
    pub(crate) max_quote_chars: usize,
    pub(crate) min_substantive_chars: usize,
    pub(crate) short_speech_shell_max_chars: usize,
    #[cfg(test)]
    pub(crate) fate_default_opening: String,
    #[cfg(test)]
    pub(crate) fate_later_forty_opening: String,
    #[cfg(test)]
    pub(crate) fate_evidence_item_template: String,
    #[cfg(test)]
    pub(crate) fate_default_boundary: String,
    #[cfg(test)]
    pub(crate) fate_later_forty_boundary: String,
    #[cfg(test)]
    pub(crate) blocked_question_terms: Vec<String>,
    pub(crate) excluded_public_quote_terms: Vec<String>,
    pub(crate) rule: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ChapterLocationPolicy {
    pub(crate) max_quote_chars: usize,
    pub(crate) max_title_chars: usize,
    pub(crate) dominant_chapter_score_margin: i64,
    pub(crate) weak_mention_markers: Vec<String>,
    #[cfg(test)]
    pub(crate) direct_answer_template: String,
    #[cfg(test)]
    pub(crate) chapter_title_template: String,
    #[cfg(test)]
    pub(crate) base_evidence_template: String,
    #[cfg(test)]
    pub(crate) commentary_evidence_template: String,
    #[cfg(test)]
    pub(crate) no_evidence_template: String,
    #[cfg(test)]
    pub(crate) ambiguous_template: String,
    pub(crate) rule: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AttributeAgePolicy {
    pub(crate) max_entity_age_distance: usize,
    pub(crate) cue_prefix_terms: Vec<String>,
}

#[derive(Debug, Clone)]
#[cfg(test)]
pub(crate) struct RationaleFollowupPolicy {
    pub(crate) applies: bool,
    pub(crate) rule: String,
}

pub(crate) fn answer_cue_policy_for_question(question: &str) -> Result<AnswerCuePolicy> {
    let catalog = answer_rule_catalog()?;
    let rules = catalog.answer_requirements;
    let normalized_question = normalize_text(question);
    let evidence_request = contains_any_rule_term(
        question,
        &normalized_question,
        &rules.evidence_request_terms,
    );
    Ok(AnswerCuePolicy {
        max_required_evidence_cards: rules.max_required_evidence_cards,
        max_anchor_cues_per_card: rules.max_anchor_cues_per_card,
        max_source_title_cue_chars: rules.max_source_title_cue_chars,
        max_text_anchor_cue_chars: rules.max_text_anchor_cue_chars,
        text_cue_min_chars: rules.text_cue_min_chars,
        evidence_request,
        require_text_cue: evidence_request && rules.evidence_request_requires_text_cue,
        rule: rules.rule,
    })
}

pub(crate) fn entity_intro_policy() -> Result<EntityIntroPolicy> {
    let catalog = answer_rule_catalog()?;
    let rules = catalog.entity_intro;
    Ok(EntityIntroPolicy {
        min_supporting_cards: rules.min_supporting_cards,
        max_supporting_cards: rules.max_supporting_cards,
        max_quote_chars: rules.max_quote_chars,
        min_substantive_chars: rules.min_substantive_chars,
        short_speech_shell_max_chars: rules.short_speech_shell_max_chars,
        #[cfg(test)]
        fate_default_opening: rules.fate_default_opening,
        #[cfg(test)]
        fate_later_forty_opening: rules.fate_later_forty_opening,
        #[cfg(test)]
        fate_evidence_item_template: rules.fate_evidence_item_template,
        #[cfg(test)]
        fate_default_boundary: rules.fate_default_boundary,
        #[cfg(test)]
        fate_later_forty_boundary: rules.fate_later_forty_boundary,
        #[cfg(test)]
        blocked_question_terms: rules.blocked_question_terms,
        excluded_public_quote_terms: rules.excluded_public_quote_terms,
        rule: rules.rule,
    })
}

pub(crate) fn chapter_location_policy() -> Result<ChapterLocationPolicy> {
    let catalog = answer_rule_catalog()?;
    let rules = catalog.chapter_location;
    Ok(ChapterLocationPolicy {
        max_quote_chars: rules.max_quote_chars,
        max_title_chars: rules.max_title_chars,
        dominant_chapter_score_margin: rules.dominant_chapter_score_margin,
        weak_mention_markers: rules.weak_mention_markers,
        #[cfg(test)]
        direct_answer_template: rules.direct_answer_template,
        #[cfg(test)]
        chapter_title_template: rules.chapter_title_template,
        #[cfg(test)]
        base_evidence_template: rules.base_evidence_template,
        #[cfg(test)]
        commentary_evidence_template: rules.commentary_evidence_template,
        #[cfg(test)]
        no_evidence_template: rules.no_evidence_template,
        #[cfg(test)]
        ambiguous_template: rules.ambiguous_template,
        rule: rules.rule,
    })
}

pub(crate) fn attribute_age_policy() -> Result<AttributeAgePolicy> {
    let catalog = answer_rule_catalog()?;
    let rules = catalog.attribute_age;
    Ok(AttributeAgePolicy {
        max_entity_age_distance: rules.max_entity_age_distance,
        cue_prefix_terms: rules.cue_prefix_terms,
    })
}

#[cfg(test)]
pub(crate) fn rationale_followup_policy_for_question(
    question: &str,
) -> Result<RationaleFollowupPolicy> {
    let catalog = answer_rule_catalog()?;
    let rules = catalog.rationale_followup;
    let normalized_question = normalize_text(question);
    Ok(RationaleFollowupPolicy {
        applies: contains_any_rule_term(question, &normalized_question, &rules.terms),
        rule: rules.rule,
    })
}

pub(crate) fn answer_rule_catalog_metadata() -> Result<Value> {
    let path = configured_path(ANSWER_RULES_PATH_ENV);
    let cache = ANSWER_RULES_CATALOG_CACHE.get_or_init(|| Mutex::new(default_answer_rule_cache()));
    let mut cache = lock_rule_cache(cache, "answer")?;
    let catalog = cache.catalog(
        ANSWER_RULES_PATH_ENV,
        path,
        default_answer_rule_catalog(),
        parse_answer_rule_catalog,
    )?;
    Ok(cache.metadata(ANSWER_RULES_SCHEMA_VERSION, &catalog.catalog_version))
}

fn answer_rule_catalog() -> Result<AnswerRuleCatalog> {
    let path = configured_path(ANSWER_RULES_PATH_ENV);
    let cache = ANSWER_RULES_CATALOG_CACHE.get_or_init(|| Mutex::new(default_answer_rule_cache()));
    let mut cache = lock_rule_cache(cache, "answer")?;
    cache.catalog(
        ANSWER_RULES_PATH_ENV,
        path,
        default_answer_rule_catalog(),
        parse_answer_rule_catalog,
    )
}

fn default_answer_rule_cache() -> RuleFileCache<AnswerRuleCatalog> {
    RuleFileCache::new(default_answer_rule_catalog())
}

fn default_answer_rule_catalog() -> AnswerRuleCatalog {
    parse_answer_rule_catalog(DEFAULT_ANSWER_RULES_JSON)
        .expect("embedded answer rule catalog must parse")
}

fn parse_answer_rule_catalog(source: &str) -> Result<AnswerRuleCatalog> {
    let catalog: AnswerRuleCatalog =
        serde_json::from_str(source).context("answer rule catalog must be JSON")?;
    if catalog.schema_version != ANSWER_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "answer rule catalog schema_version must be {}",
            ANSWER_RULES_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!("answer rule catalog catalog_version is required"));
    }
    let rules = &catalog.answer_requirements;
    for (name, value) in [
        (
            "answer_requirements.max_required_evidence_cards",
            rules.max_required_evidence_cards,
        ),
        (
            "answer_requirements.max_anchor_cues_per_card",
            rules.max_anchor_cues_per_card,
        ),
        (
            "answer_requirements.max_source_title_cue_chars",
            rules.max_source_title_cue_chars,
        ),
        (
            "answer_requirements.max_text_anchor_cue_chars",
            rules.max_text_anchor_cue_chars,
        ),
        (
            "answer_requirements.text_cue_min_chars",
            rules.text_cue_min_chars,
        ),
    ] {
        if value == 0 {
            return Err(anyhow!("answer rule catalog {name} must be positive"));
        }
    }
    if rules.evidence_request_requires_text_cue
        && rules
            .evidence_request_terms
            .iter()
            .all(|term| term.trim().is_empty())
    {
        return Err(anyhow!(
            "answer rule catalog evidence_request_terms must be non-empty when text cues are required"
        ));
    }
    if rules.rule.trim().is_empty() {
        return Err(anyhow!("answer rule catalog rule is required"));
    }
    let entity_intro = &catalog.entity_intro;
    for (name, value) in [
        (
            "entity_intro.min_supporting_cards",
            entity_intro.min_supporting_cards,
        ),
        (
            "entity_intro.max_supporting_cards",
            entity_intro.max_supporting_cards,
        ),
        ("entity_intro.max_quote_chars", entity_intro.max_quote_chars),
        (
            "entity_intro.min_substantive_chars",
            entity_intro.min_substantive_chars,
        ),
        (
            "entity_intro.short_speech_shell_max_chars",
            entity_intro.short_speech_shell_max_chars,
        ),
    ] {
        if value == 0 {
            return Err(anyhow!("answer rule catalog {name} must be positive"));
        }
    }
    if entity_intro.max_supporting_cards < entity_intro.min_supporting_cards {
        return Err(anyhow!(
            "answer rule catalog entity_intro.max_supporting_cards must be >= min_supporting_cards"
        ));
    }
    if entity_intro.rule.trim().is_empty() {
        return Err(anyhow!("answer rule catalog entity_intro.rule is required"));
    }
    for (name, value) in [
        (
            "entity_intro.fate_default_opening",
            &entity_intro.fate_default_opening,
        ),
        (
            "entity_intro.fate_later_forty_opening",
            &entity_intro.fate_later_forty_opening,
        ),
        (
            "entity_intro.fate_evidence_item_template",
            &entity_intro.fate_evidence_item_template,
        ),
        (
            "entity_intro.fate_default_boundary",
            &entity_intro.fate_default_boundary,
        ),
        (
            "entity_intro.fate_later_forty_boundary",
            &entity_intro.fate_later_forty_boundary,
        ),
    ] {
        if value.trim().is_empty() {
            return Err(anyhow!("answer rule catalog {name} is required"));
        }
    }
    if !entity_intro.fate_evidence_item_template.contains("{index}")
        || !entity_intro
            .fate_evidence_item_template
            .contains("{source_title}")
        || !entity_intro.fate_evidence_item_template.contains("{quote}")
    {
        return Err(anyhow!(
            "answer rule catalog entity_intro.fate_evidence_item_template must contain index, source_title, and quote placeholders"
        ));
    }
    if entity_intro
        .blocked_question_terms
        .iter()
        .any(|term| term.trim().is_empty())
    {
        return Err(anyhow!(
            "answer rule catalog entity_intro.blocked_question_terms must not contain empty terms"
        ));
    }
    if entity_intro
        .excluded_public_quote_terms
        .iter()
        .any(|term| term.trim().is_empty())
    {
        return Err(anyhow!(
            "answer rule catalog entity_intro.excluded_public_quote_terms must not contain empty terms"
        ));
    }
    let chapter_location = &catalog.chapter_location;
    for (name, value) in [
        (
            "chapter_location.max_quote_chars",
            chapter_location.max_quote_chars,
        ),
        (
            "chapter_location.max_title_chars",
            chapter_location.max_title_chars,
        ),
    ] {
        if value == 0 {
            return Err(anyhow!("answer rule catalog {name} must be positive"));
        }
    }
    if chapter_location.dominant_chapter_score_margin < 0 {
        return Err(anyhow!(
            "answer rule catalog chapter_location.dominant_chapter_score_margin must be non-negative"
        ));
    }
    if chapter_location
        .weak_mention_markers
        .iter()
        .any(|term| term.trim().is_empty())
    {
        return Err(anyhow!(
            "answer rule catalog chapter_location.weak_mention_markers must not contain empty terms"
        ));
    }
    for (name, value) in [
        (
            "chapter_location.direct_answer_template",
            &chapter_location.direct_answer_template,
        ),
        (
            "chapter_location.chapter_title_template",
            &chapter_location.chapter_title_template,
        ),
        (
            "chapter_location.base_evidence_template",
            &chapter_location.base_evidence_template,
        ),
        (
            "chapter_location.commentary_evidence_template",
            &chapter_location.commentary_evidence_template,
        ),
        (
            "chapter_location.no_evidence_template",
            &chapter_location.no_evidence_template,
        ),
        (
            "chapter_location.ambiguous_template",
            &chapter_location.ambiguous_template,
        ),
        ("chapter_location.rule", &chapter_location.rule),
    ] {
        if value.trim().is_empty() {
            return Err(anyhow!("answer rule catalog {name} is required"));
        }
    }
    for (name, value, placeholders) in [
        (
            "chapter_location.direct_answer_template",
            &chapter_location.direct_answer_template,
            &["{event}", "{chapter_no}"][..],
        ),
        (
            "chapter_location.chapter_title_template",
            &chapter_location.chapter_title_template,
            &["{chapter_title}"][..],
        ),
        (
            "chapter_location.base_evidence_template",
            &chapter_location.base_evidence_template,
            &["{quote}"][..],
        ),
        (
            "chapter_location.commentary_evidence_template",
            &chapter_location.commentary_evidence_template,
            &["{quote}"][..],
        ),
        (
            "chapter_location.no_evidence_template",
            &chapter_location.no_evidence_template,
            &["{event}"][..],
        ),
        (
            "chapter_location.ambiguous_template",
            &chapter_location.ambiguous_template,
            &["{event}", "{locations}"][..],
        ),
    ] {
        if placeholders
            .iter()
            .any(|placeholder| !value.contains(placeholder))
        {
            return Err(anyhow!(
                "answer rule catalog {name} is missing required placeholders"
            ));
        }
    }
    let attribute_age = &catalog.attribute_age;
    if attribute_age.max_entity_age_distance == 0 {
        return Err(anyhow!(
            "answer rule catalog attribute_age.max_entity_age_distance must be positive"
        ));
    }
    if attribute_age
        .cue_prefix_terms
        .iter()
        .any(|term| term.trim().is_empty())
    {
        return Err(anyhow!(
            "answer rule catalog attribute_age.cue_prefix_terms must not contain empty terms"
        ));
    }
    if attribute_age.rule.trim().is_empty() {
        return Err(anyhow!(
            "answer rule catalog attribute_age.rule is required"
        ));
    }
    let rationale_followup = &catalog.rationale_followup;
    if rationale_followup
        .terms
        .iter()
        .all(|term| term.trim().is_empty())
    {
        return Err(anyhow!(
            "answer rule catalog rationale_followup.terms must be non-empty"
        ));
    }
    if rationale_followup.rule.trim().is_empty() {
        return Err(anyhow!(
            "answer rule catalog rationale_followup.rule is required"
        ));
    }
    Ok(catalog)
}

pub(crate) fn validate_answer_rule_catalog_source(source: &str) -> Result<()> {
    parse_answer_rule_catalog(source).map(|_| ())
}

fn contains_any_rule_term(text: &str, normalized: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| {
        let term = term.trim();
        !term.is_empty() && (text.contains(term) || normalized.contains(&normalize_text(term)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_catalog_requires_text_cue_for_evidence_question() {
        let policy = answer_cue_policy_for_question("脂批中的证据呢").expect("answer rules");

        assert!(policy.evidence_request);
        assert!(policy.require_text_cue);
        assert!(policy.rule.contains("text cue"));
    }

    #[test]
    fn default_catalog_does_not_force_text_cue_for_plain_question() {
        let policy = answer_cue_policy_for_question("史湘云的结局").expect("answer rules");

        assert!(!policy.evidence_request);
        assert!(!policy.require_text_cue);
    }

    #[test]
    fn default_catalog_exposes_entity_intro_policy() {
        let policy = entity_intro_policy().expect("entity intro policy");

        assert!(policy.min_supporting_cards >= 1);
        assert!(policy.max_supporting_cards >= policy.min_supporting_cards);
        assert!(policy.fate_evidence_item_template.contains("{quote}"));
        assert!(
            policy
                .blocked_question_terms
                .iter()
                .any(|term| term == "哪一回")
        );
        assert!(policy.rule.contains("entity"));
    }

    #[test]
    fn default_catalog_exposes_chapter_location_policy() {
        let policy = chapter_location_policy().expect("chapter location policy");

        assert!(policy.max_quote_chars > 0);
        assert!(policy.direct_answer_template.contains("{chapter_no}"));
        assert!(policy.chapter_title_template.contains("{chapter_title}"));
        assert!(policy.rule.contains("chapter"));
    }

    #[test]
    fn default_catalog_exposes_attribute_age_policy() {
        let policy = attribute_age_policy().expect("attribute age policy");

        assert!(policy.max_entity_age_distance > 0);
        assert!(policy.cue_prefix_terms.iter().any(|term| term == "年方"));
    }

    #[test]
    fn default_catalog_detects_rationale_followup() {
        let policy =
            rationale_followup_policy_for_question("你的推理逻辑是什么").expect("rationale policy");

        assert!(policy.applies);
        assert!(policy.rule.contains("reasoning"));
    }

    #[test]
    fn catalog_cache_hot_reloads_external_file() {
        let catalog_path = std::env::temp_dir().join(format!(
            "tonglingyu-answer-rules-{}.json",
            uuid::Uuid::now_v7().simple()
        ));
        let initial_catalog = r#"{
            "schema_version": "tonglingyu.answer_rules.v1",
            "catalog_version": "test.1",
            "answer_requirements": {
                "max_required_evidence_cards": 2,
                "max_anchor_cues_per_card": 3,
                "max_source_title_cue_chars": 40,
                "max_text_anchor_cue_chars": 20,
                "text_cue_min_chars": 2,
                "evidence_request_requires_text_cue": true,
                "evidence_request_terms": ["证据"],
                "rule": "initial rule"
            },
            "entity_intro": {
                "min_supporting_cards": 1,
                "max_supporting_cards": 2,
                "max_quote_chars": 32,
                "min_substantive_chars": 8,
                "short_speech_shell_max_chars": 4,
                "fate_default_opening": "initial fate default opening",
                "fate_later_forty_opening": "initial fate later opening",
                "fate_evidence_item_template": "{index}. {source_title}: {quote}",
                "fate_default_boundary": "initial fate default boundary",
                "fate_later_forty_boundary": "initial fate later boundary",
                "blocked_question_terms": ["initial blocked"],
                "excluded_public_quote_terms": ["initial excluded"],
                "rule": "initial entity rule"
            },
            "chapter_location": {
                "max_quote_chars": 40,
                "max_title_chars": 30,
                "dominant_chapter_score_margin": 8,
                "weak_mention_markers": ["initial weak"],
                "direct_answer_template": "initial direct {event} {chapter_no}",
                "chapter_title_template": "initial title {chapter_title}",
                "base_evidence_template": "initial base {quote}",
                "commentary_evidence_template": "initial commentary {quote}",
                "no_evidence_template": "initial no {event}",
                "ambiguous_template": "initial ambiguous {event} {locations}",
                "rule": "initial chapter location rule"
            },
            "attribute_age": {
                "max_entity_age_distance": 80,
                "cue_prefix_terms": ["initial age prefix"],
                "rule": "initial age rule"
            },
            "rationale_followup": {
                "terms": ["initial why"],
                "rule": "initial rationale rule"
            }
        }"#;
        let updated_catalog = r#"{
            "schema_version": "tonglingyu.answer_rules.v1",
            "catalog_version": "test.2",
            "answer_requirements": {
                "max_required_evidence_cards": 4,
                "max_anchor_cues_per_card": 5,
                "max_source_title_cue_chars": 60,
                "max_text_anchor_cue_chars": 30,
                "text_cue_min_chars": 3,
                "evidence_request_requires_text_cue": true,
                "evidence_request_terms": ["证据"],
                "rule": "updated rule"
            },
            "entity_intro": {
                "min_supporting_cards": 2,
                "max_supporting_cards": 4,
                "max_quote_chars": 48,
                "min_substantive_chars": 12,
                "short_speech_shell_max_chars": 5,
                "fate_default_opening": "updated fate default opening",
                "fate_later_forty_opening": "updated fate later opening",
                "fate_evidence_item_template": "{index}. {source_title}: {quote}",
                "fate_default_boundary": "updated fate default boundary",
                "fate_later_forty_boundary": "updated fate later boundary",
                "blocked_question_terms": ["updated blocked"],
                "excluded_public_quote_terms": ["updated excluded"],
                "rule": "updated entity rule"
            },
            "chapter_location": {
                "max_quote_chars": 64,
                "max_title_chars": 50,
                "dominant_chapter_score_margin": 12,
                "weak_mention_markers": ["updated weak"],
                "direct_answer_template": "updated direct {event} {chapter_no}",
                "chapter_title_template": "updated title {chapter_title}",
                "base_evidence_template": "updated base {quote}",
                "commentary_evidence_template": "updated commentary {quote}",
                "no_evidence_template": "updated no {event}",
                "ambiguous_template": "updated ambiguous {event} {locations}",
                "rule": "updated chapter location rule"
            },
            "attribute_age": {
                "max_entity_age_distance": 120,
                "cue_prefix_terms": ["updated age prefix"],
                "rule": "updated age rule"
            },
            "rationale_followup": {
                "terms": ["updated why"],
                "rule": "updated rationale rule"
            }
        }"#;
        std::fs::write(&catalog_path, initial_catalog).expect("write initial catalog");
        let mut cache = RuleFileCache::new(default_answer_rule_catalog());
        let catalog = cache
            .catalog(
                ANSWER_RULES_PATH_ENV,
                Some(catalog_path.clone()),
                default_answer_rule_catalog(),
                parse_answer_rule_catalog,
            )
            .expect("load initial catalog");
        assert_eq!(catalog.catalog_version, "test.1");
        assert_eq!(catalog.answer_requirements.rule, "initial rule");
        assert_eq!(catalog.entity_intro.rule, "initial entity rule");
        assert_eq!(
            catalog.chapter_location.rule,
            "initial chapter location rule"
        );
        assert_eq!(catalog.attribute_age.rule, "initial age rule");
        assert_eq!(catalog.rationale_followup.rule, "initial rationale rule");

        std::fs::write(&catalog_path, updated_catalog).expect("write updated catalog");
        let catalog = cache
            .catalog(
                ANSWER_RULES_PATH_ENV,
                Some(catalog_path.clone()),
                default_answer_rule_catalog(),
                parse_answer_rule_catalog,
            )
            .expect("reload updated catalog");
        assert_eq!(catalog.catalog_version, "test.2");
        assert_eq!(catalog.answer_requirements.rule, "updated rule");
        assert_eq!(catalog.entity_intro.rule, "updated entity rule");
        assert_eq!(
            catalog.chapter_location.rule,
            "updated chapter location rule"
        );
        assert_eq!(catalog.chapter_location.max_quote_chars, 64);
        assert_eq!(catalog.attribute_age.rule, "updated age rule");
        assert_eq!(catalog.rationale_followup.rule, "updated rationale rule");
        assert_eq!(catalog.attribute_age.max_entity_age_distance, 120);
        assert_eq!(
            catalog.entity_intro.excluded_public_quote_terms,
            vec!["updated excluded".to_string()]
        );
        assert_eq!(
            catalog.attribute_age.cue_prefix_terms,
            vec!["updated age prefix".to_string()]
        );

        std::fs::remove_file(catalog_path).expect("remove catalog");
    }

    #[test]
    fn invalid_external_catalog_fails_without_fallback() {
        let path = std::env::temp_dir().join(format!(
            "tonglingyu-answer-rules-invalid-{}.json",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::write(&path, r#"{"schema_version":"wrong"}"#).expect("write invalid catalog");
        let mut cache = RuleFileCache::new(default_answer_rule_catalog());

        let error = cache
            .catalog(
                ANSWER_RULES_PATH_ENV,
                Some(path.clone()),
                default_answer_rule_catalog(),
                parse_answer_rule_catalog,
            )
            .expect_err("invalid external catalog should fail");

        assert!(error.to_string().contains("is not a valid catalog"));
        std::fs::remove_file(path).expect("remove invalid catalog");
    }
}
