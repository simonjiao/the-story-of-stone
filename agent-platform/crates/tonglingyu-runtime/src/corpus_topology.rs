use crate::{
    CORPUS_TOPOLOGY_PATH_ENV, EvidenceCard, extract_chapter_no, hash_text, normalize_text,
    rule_catalog::{RuleFileCache, configured_path, lock_rule_cache},
    upstream_bundle::evidence_card_source_layer,
};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
};

pub(crate) const CORPUS_TOPOLOGY_SCHEMA_VERSION: &str = "tonglingyu.corpus_topology.v1";
pub(crate) const FULL_TEXT_SEARCH_REQUEST_SCHEMA_VERSION: &str =
    "tonglingyu.full_text_search_request.v1";
const DEFAULT_CORPUS_TOPOLOGY_JSON: &str = include_str!("../resources/corpus_topology.json");

static CORPUS_TOPOLOGY_CATALOG_CACHE: OnceLock<Mutex<RuleFileCache<CorpusTopologyCatalog>>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusTopologyCatalog {
    schema_version: String,
    catalog_version: String,
    corpora: Vec<CorpusRule>,
    source_mappings: Vec<SourceMappingRule>,
    search_defaults: SearchDefaultRules,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusRule {
    id: String,
    label: String,
    default_answer_scope: String,
    default_allowed: bool,
    layers: Vec<CorpusLayerRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusLayerRule {
    id: String,
    evidence_types: Vec<String>,
    default_allowed: bool,
    answer_rank: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMappingRule {
    id: String,
    corpus_id: String,
    source_id_terms: Vec<String>,
    #[serde(default)]
    source_title_terms: Vec<String>,
    default_layer: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchDefaultRules {
    default_corpus_ids: Vec<String>,
    default_layers: Vec<String>,
    explicit_later_forty_corpus_ids: Vec<String>,
    explicit_later_forty_layers: Vec<String>,
    default_chapter_start: Option<i64>,
    default_chapter_end: Option<i64>,
    explicit_later_forty_chapter_start: Option<i64>,
    explicit_later_forty_chapter_end: Option<i64>,
    max_terms_per_request: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct FullTextSearchRequest {
    pub schema_version: String,
    pub request_source: String,
    pub query_text: String,
    pub search_terms: Vec<String>,
    pub required_evidence_types: Vec<String>,
    pub corpus_ids: Vec<String>,
    pub source_layers: Vec<String>,
    pub chapter_start: Option<i64>,
    pub chapter_end: Option<i64>,
    pub source_scope_policy: Value,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CorpusEvidenceProfile {
    pub corpus_id: String,
    pub corpus_label: String,
    pub source_mapping_id: String,
    pub source_layer: String,
    pub default_answer_scope: String,
    pub default_allowed: bool,
    pub answer_rank: usize,
    pub chapter_no: Option<i64>,
}

pub(crate) fn corpus_topology_catalog_metadata() -> Result<Value> {
    let path = configured_path(CORPUS_TOPOLOGY_PATH_ENV);
    let cache =
        CORPUS_TOPOLOGY_CATALOG_CACHE.get_or_init(|| Mutex::new(default_corpus_topology_cache()));
    let mut cache = lock_rule_cache(cache, "corpus topology")?;
    let catalog = cache.catalog(
        CORPUS_TOPOLOGY_PATH_ENV,
        path,
        default_corpus_topology_catalog(),
        parse_corpus_topology_catalog,
    )?;
    Ok(cache.metadata(CORPUS_TOPOLOGY_SCHEMA_VERSION, &catalog.catalog_version))
}

pub(crate) fn default_full_text_search_requests(
    question: &str,
    question_frame: Option<&Value>,
    source_scope_policy: &Value,
) -> Result<Vec<FullTextSearchRequest>> {
    let catalog = corpus_topology_catalog()?;
    let later_forty_allowed = source_scope_policy
        .get("later_forty_allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut terms = Vec::new();
    push_unique_term(&mut terms, question);
    if let Some(frame) = question_frame {
        collect_question_frame_terms(frame, &mut terms);
    }
    for term in crate::query_expansion_search_terms(question).unwrap_or_default() {
        push_unique_term(&mut terms, &term);
    }
    let max_terms = catalog.search_defaults.max_terms_per_request.max(1);
    terms.truncate(max_terms);

    let required_evidence_types = question_frame
        .and_then(|frame| frame.get("required_evidence_types"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(bounded_rule_text)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let (corpus_ids, source_layers, chapter_start, chapter_end) = if later_forty_allowed {
        (
            catalog
                .search_defaults
                .explicit_later_forty_corpus_ids
                .clone(),
            catalog.search_defaults.explicit_later_forty_layers.clone(),
            catalog.search_defaults.explicit_later_forty_chapter_start,
            catalog.search_defaults.explicit_later_forty_chapter_end,
        )
    } else {
        (
            catalog.search_defaults.default_corpus_ids.clone(),
            catalog.search_defaults.default_layers.clone(),
            catalog.search_defaults.default_chapter_start,
            catalog.search_defaults.default_chapter_end,
        )
    };

    Ok(vec![FullTextSearchRequest {
        schema_version: FULL_TEXT_SEARCH_REQUEST_SCHEMA_VERSION.to_string(),
        request_source: if question_frame.is_some() {
            "local_question_frame".to_string()
        } else {
            "local_resolved_question".to_string()
        },
        query_text: bounded_rule_text(question).unwrap_or_else(|| hash_text(question)),
        search_terms: terms,
        required_evidence_types,
        corpus_ids,
        source_layers,
        chapter_start,
        chapter_end,
        source_scope_policy: canonical_json_value(source_scope_policy),
        reason: "coverage_gap_local_full_text_search".to_string(),
    }])
}

pub(crate) fn full_text_search_requests_from_retrieval_repair_queries(
    question: &str,
    question_frame: Option<&Value>,
    source_scope_policy: &Value,
    queries: &[Value],
) -> Result<Vec<FullTextSearchRequest>> {
    let catalog = corpus_topology_catalog()?;
    let defaults =
        default_full_text_search_requests(question, question_frame, source_scope_policy)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("default full-text search request missing"))?;
    let mut requests = Vec::new();
    for query in queries {
        let query_text = query
            .get("query_text")
            .or_else(|| query.get("query"))
            .and_then(Value::as_str)
            .and_then(bounded_rule_text)
            .unwrap_or_else(|| defaults.query_text.clone());
        let mut search_terms = string_array_field(query, "search_terms")
            .or_else(|| string_array_field(query, "terms"))
            .unwrap_or_default();
        push_unique_term(&mut search_terms, &query_text);
        if search_terms.is_empty() {
            search_terms = defaults.search_terms.clone();
        }
        search_terms.truncate(catalog.search_defaults.max_terms_per_request.max(1));
        let (chapter_start, chapter_end) = normalize_chapter_range(
            chapter_range_from_query(query),
            defaults.chapter_start,
            defaults.chapter_end,
        );
        let request = FullTextSearchRequest {
            schema_version: FULL_TEXT_SEARCH_REQUEST_SCHEMA_VERSION.to_string(),
            request_source: "upstream_retrieval_repair".to_string(),
            query_text,
            search_terms,
            required_evidence_types: string_array_field(query, "required_evidence_types")
                .unwrap_or_else(|| defaults.required_evidence_types.clone()),
            corpus_ids: normalize_requested_terms(
                string_array_field(query, "corpus_ids"),
                &defaults.corpus_ids,
            ),
            source_layers: normalize_requested_terms(
                string_array_field(query, "source_layers")
                    .or_else(|| string_array_field(query, "layers")),
                &defaults.source_layers,
            ),
            chapter_start,
            chapter_end,
            source_scope_policy: defaults.source_scope_policy.clone(),
            reason: query
                .get("reason")
                .and_then(Value::as_str)
                .and_then(bounded_rule_text)
                .unwrap_or_else(|| "upstream_retrieval_repair".to_string()),
        };
        validate_full_text_search_request(&request)?;
        requests.push(request);
    }
    Ok(requests)
}

pub(crate) fn parse_full_text_search_request(value: &Value) -> Result<FullTextSearchRequest> {
    let request: FullTextSearchRequest = serde_json::from_value(value.clone())
        .context("full-text search request must be a JSON object")?;
    validate_full_text_search_request(&request)?;
    Ok(request)
}

pub(crate) fn full_text_search_request_id(
    update_request_id: &str,
    request: &FullTextSearchRequest,
) -> Result<String> {
    Ok(format!(
        "ftsr-{}",
        &hash_text(&serde_json::to_string(&canonical_json_value(&json!({
            "update_request_id": update_request_id,
            "request": request,
        })))?)[..32]
    ))
}

pub(crate) fn card_matches_full_text_search_request(
    card: &EvidenceCard,
    request: &FullTextSearchRequest,
) -> Result<bool> {
    validate_full_text_search_request(request)?;
    if !request.required_evidence_types.is_empty()
        && !request
            .required_evidence_types
            .iter()
            .any(|item| item == &card.evidence_type)
    {
        return Ok(false);
    }
    let profile = classify_evidence_card(card)?;
    if !request.corpus_ids.is_empty()
        && !request
            .corpus_ids
            .iter()
            .any(|corpus_id| corpus_id == &profile.corpus_id)
    {
        return Ok(false);
    }
    if !request.source_layers.is_empty()
        && !request
            .source_layers
            .iter()
            .any(|layer| layer == &profile.source_layer)
    {
        return Ok(false);
    }
    if let Some(start) = request.chapter_start
        && profile.chapter_no.is_some_and(|chapter| chapter < start)
    {
        return Ok(false);
    }
    if let Some(end) = request.chapter_end
        && profile.chapter_no.is_some_and(|chapter| chapter > end)
    {
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn classify_evidence_card(card: &EvidenceCard) -> Result<CorpusEvidenceProfile> {
    let catalog = corpus_topology_catalog()?;
    let source_layer = evidence_card_source_layer(card).to_string();
    let mapping = catalog
        .source_mappings
        .iter()
        .find(|mapping| source_mapping_matches(mapping, card))
        .or_else(|| {
            catalog
                .source_mappings
                .iter()
                .find(|mapping| mapping.id == "default")
        })
        .ok_or_else(|| anyhow!("corpus topology requires a default source mapping"))?;
    let corpus = catalog
        .corpora
        .iter()
        .find(|corpus| corpus.id == mapping.corpus_id)
        .ok_or_else(|| anyhow!("corpus topology source mapping references unknown corpus"))?;
    let layer = corpus
        .layers
        .iter()
        .find(|layer| layer.id == source_layer)
        .or_else(|| {
            corpus
                .layers
                .iter()
                .find(|layer| layer.id == mapping.default_layer)
        })
        .ok_or_else(|| anyhow!("corpus topology corpus lacks matching layer"))?;
    Ok(CorpusEvidenceProfile {
        corpus_id: corpus.id.clone(),
        corpus_label: corpus.label.clone(),
        source_mapping_id: mapping.id.clone(),
        source_layer,
        default_answer_scope: corpus.default_answer_scope.clone(),
        default_allowed: corpus.default_allowed && layer.default_allowed,
        answer_rank: layer.answer_rank,
        chapter_no: extract_chapter_no(&card.source_title),
    })
}

fn corpus_topology_catalog() -> Result<CorpusTopologyCatalog> {
    let path = configured_path(CORPUS_TOPOLOGY_PATH_ENV);
    let cache =
        CORPUS_TOPOLOGY_CATALOG_CACHE.get_or_init(|| Mutex::new(default_corpus_topology_cache()));
    let mut cache = lock_rule_cache(cache, "corpus topology")?;
    cache.catalog(
        CORPUS_TOPOLOGY_PATH_ENV,
        path,
        default_corpus_topology_catalog(),
        parse_corpus_topology_catalog,
    )
}

fn default_corpus_topology_cache() -> RuleFileCache<CorpusTopologyCatalog> {
    RuleFileCache::new(default_corpus_topology_catalog())
}

fn default_corpus_topology_catalog() -> CorpusTopologyCatalog {
    parse_corpus_topology_catalog(DEFAULT_CORPUS_TOPOLOGY_JSON)
        .expect("embedded corpus topology catalog must parse")
}

fn parse_corpus_topology_catalog(source: &str) -> Result<CorpusTopologyCatalog> {
    let catalog: CorpusTopologyCatalog =
        serde_json::from_str(source).context("corpus topology catalog must be JSON")?;
    if catalog.schema_version != CORPUS_TOPOLOGY_SCHEMA_VERSION {
        return Err(anyhow!(
            "corpus topology catalog schema_version must be {}",
            CORPUS_TOPOLOGY_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "corpus topology catalog catalog_version is required"
        ));
    }
    let corpus_ids = catalog
        .corpora
        .iter()
        .map(|corpus| corpus.id.as_str())
        .collect::<BTreeSet<_>>();
    if corpus_ids.is_empty() {
        return Err(anyhow!("corpus topology catalog requires corpora"));
    }
    for corpus in &catalog.corpora {
        require_id("corpora.id", &corpus.id)?;
        require_text("corpora.label", &corpus.label)?;
        require_text("corpora.default_answer_scope", &corpus.default_answer_scope)?;
        if corpus.layers.is_empty() {
            return Err(anyhow!("corpus {} requires layers", corpus.id));
        }
        let mut layer_ids = BTreeSet::new();
        for layer in &corpus.layers {
            require_id("corpora.layers.id", &layer.id)?;
            if !layer_ids.insert(layer.id.as_str()) {
                return Err(anyhow!(
                    "corpus {} has duplicate layer {}",
                    corpus.id,
                    layer.id
                ));
            }
            require_terms("corpora.layers.evidence_types", &layer.evidence_types)?;
        }
    }
    for mapping in &catalog.source_mappings {
        require_id("source_mappings.id", &mapping.id)?;
        if !corpus_ids.contains(mapping.corpus_id.as_str()) {
            return Err(anyhow!(
                "source mapping {} references unknown corpus {}",
                mapping.id,
                mapping.corpus_id
            ));
        }
        require_terms("source_mappings.source_id_terms", &mapping.source_id_terms)?;
        require_id("source_mappings.default_layer", &mapping.default_layer)?;
    }
    require_terms(
        "search_defaults.default_corpus_ids",
        &catalog.search_defaults.default_corpus_ids,
    )?;
    require_terms(
        "search_defaults.default_layers",
        &catalog.search_defaults.default_layers,
    )?;
    require_terms(
        "search_defaults.explicit_later_forty_corpus_ids",
        &catalog.search_defaults.explicit_later_forty_corpus_ids,
    )?;
    require_terms(
        "search_defaults.explicit_later_forty_layers",
        &catalog.search_defaults.explicit_later_forty_layers,
    )?;
    if catalog.search_defaults.max_terms_per_request == 0 {
        return Err(anyhow!(
            "corpus topology search_defaults.max_terms_per_request must be positive"
        ));
    }
    Ok(catalog)
}

fn validate_full_text_search_request(request: &FullTextSearchRequest) -> Result<()> {
    if request.schema_version != FULL_TEXT_SEARCH_REQUEST_SCHEMA_VERSION {
        return Err(anyhow!(
            "full-text search request schema_version must be {}",
            FULL_TEXT_SEARCH_REQUEST_SCHEMA_VERSION
        ));
    }
    require_text(
        "full_text_search_request.request_source",
        &request.request_source,
    )?;
    require_text("full_text_search_request.query_text", &request.query_text)?;
    require_terms(
        "full_text_search_request.search_terms",
        &request.search_terms,
    )?;
    require_terms("full_text_search_request.corpus_ids", &request.corpus_ids)?;
    require_terms(
        "full_text_search_request.source_layers",
        &request.source_layers,
    )?;
    if request
        .chapter_start
        .zip(request.chapter_end)
        .is_some_and(|(start, end)| start > end)
    {
        return Err(anyhow!(
            "full-text search request chapter_start must not exceed chapter_end"
        ));
    }
    Ok(())
}

fn collect_question_frame_terms(value: &Value, terms: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if matches!(
                    key.as_str(),
                    "canonical_question"
                        | "canonical"
                        | "label"
                        | "aliases"
                        | "evidence_terms"
                        | "comparison_terms"
                ) || child.is_object()
                    || child.is_array()
                {
                    collect_question_frame_terms(child, terms);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_question_frame_terms(item, terms);
            }
        }
        Value::String(text) => push_unique_term(terms, text),
        _ => {}
    }
}

fn string_array_field(value: &Value, field: &str) -> Option<Vec<String>> {
    let items = value.get(field)?.as_array()?;
    let collected = items
        .iter()
        .filter_map(Value::as_str)
        .filter_map(bounded_rule_text)
        .collect::<Vec<_>>();
    (!collected.is_empty()).then_some(collected)
}

fn chapter_range_from_query(value: &Value) -> Option<(Option<i64>, Option<i64>)> {
    if let Some(range) = value.get("chapter_range").and_then(Value::as_object) {
        return Some((
            range.get("start").and_then(Value::as_i64),
            range.get("end").and_then(Value::as_i64),
        ));
    }
    if value.get("chapter_start").is_some() || value.get("chapter_end").is_some() {
        return Some((
            value.get("chapter_start").and_then(Value::as_i64),
            value.get("chapter_end").and_then(Value::as_i64),
        ));
    }
    None
}

fn normalize_requested_terms(requested: Option<Vec<String>>, defaults: &[String]) -> Vec<String> {
    let allowed = defaults.iter().cloned().collect::<BTreeSet<_>>();
    let mut normalized = Vec::new();
    for term in requested.unwrap_or_default() {
        if allowed.contains(&term) && !normalized.iter().any(|existing| existing == &term) {
            normalized.push(term);
        }
    }
    if normalized.is_empty() {
        defaults.to_vec()
    } else {
        normalized
    }
}

fn normalize_chapter_range(
    requested: Option<(Option<i64>, Option<i64>)>,
    default_start: Option<i64>,
    default_end: Option<i64>,
) -> (Option<i64>, Option<i64>) {
    let Some((requested_start, requested_end)) = requested else {
        return (default_start, default_end);
    };
    let mut start = requested_start.or(requested_end).or(default_start);
    let mut end = requested_end.or(requested_start).or(default_end);
    if let (Some(value), Some(boundary)) = (start, default_start)
        && value < boundary
    {
        start = Some(boundary);
    }
    if let (Some(value), Some(boundary)) = (end, default_end)
        && value > boundary
    {
        end = Some(boundary);
    }
    if start.zip(end).is_some_and(|(start, end)| start > end) {
        return (default_start, default_end);
    }
    (start, end)
}

fn source_mapping_matches(mapping: &SourceMappingRule, card: &EvidenceCard) -> bool {
    let normalized_source_id = normalize_text(&card.source_id);
    let normalized_source_title = normalize_text(&card.source_title);
    mapping.source_id_terms.iter().any(|term| {
        let normalized_term = normalize_text(term);
        normalized_source_id.contains(&normalized_term)
            || normalized_source_title.contains(&normalized_term)
    }) || mapping.source_title_terms.iter().any(|term| {
        let normalized_term = normalize_text(term);
        normalized_source_title.contains(&normalized_term)
    })
}

fn canonical_json_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json_value(value)))
                .collect::<BTreeMap<_, _>>();
            json!(ordered)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json_value).collect()),
        _ => value.clone(),
    }
}

fn push_unique_term(terms: &mut Vec<String>, value: &str) {
    if let Some(term) = bounded_rule_text(value)
        && !terms.iter().any(|existing| existing == &term)
    {
        terms.push(term);
    }
}

fn bounded_rule_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(160).collect())
}

fn require_id(field: &str, value: &str) -> Result<()> {
    let valid = !value.trim().is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.');
    if valid {
        Ok(())
    } else {
        Err(anyhow!("{field} must be a non-empty stable id"))
    }
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(anyhow!("{field} is required"))
    } else {
        Ok(())
    }
}

fn require_terms(field: &str, values: &[String]) -> Result<()> {
    if values.iter().any(|value| !value.trim().is_empty()) {
        Ok(())
    } else {
        Err(anyhow!("{field} requires at least one non-empty item"))
    }
}

#[cfg(test)]
mod tests;
