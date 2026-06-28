use crate::{
    ANSWER_RULES_PATH_ENV, ONTOLOGY_ALIASES_PATH_ENV, QUERY_EXPANSIONS_PATH_ENV, answer_rules,
    normalize_for_search, ontology_aliases, rule_catalog::configured_path,
    validate_query_expansion_catalog_source,
};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

pub const RUNTIME_RULE_CANDIDATE_TYPES: &[&str] = &[
    QUERY_EXPANSION_TERM,
    QUERY_EXPANSION_EXACT_TERM,
    ANSWER_EVIDENCE_REQUEST_TERM,
    RUNTIME_PERSON_ALIAS,
];

pub const QUERY_EXPANSION_TERM: &str = "query_expansion_term";
pub const QUERY_EXPANSION_EXACT_TERM: &str = "query_expansion_exact_term";
pub const ANSWER_EVIDENCE_REQUEST_TERM: &str = "answer_evidence_request_term";
pub const RUNTIME_PERSON_ALIAS: &str = "runtime_person_alias";

const DEFAULT_QUERY_EXPANSIONS_JSON: &str = include_str!("../resources/query_expansions.json");
const DEFAULT_ANSWER_RULES_JSON: &str = include_str!("../resources/answer_rules.json");
const DEFAULT_ONTOLOGY_ALIASES_JSON: &str = include_str!("../resources/ontology_aliases.json");

#[derive(Debug, Clone, Default)]
pub struct RuntimeRuleCandidatePromotionPaths {
    pub query_expansions_path: Option<PathBuf>,
    pub answer_rules_path: Option<PathBuf>,
    pub ontology_aliases_path: Option<PathBuf>,
}

impl RuntimeRuleCandidatePromotionPaths {
    pub fn from_env() -> Self {
        Self {
            query_expansions_path: configured_path(QUERY_EXPANSIONS_PATH_ENV),
            answer_rules_path: configured_path(ANSWER_RULES_PATH_ENV),
            ontology_aliases_path: configured_path(ONTOLOGY_ALIASES_PATH_ENV),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRuleCandidateActiveMatch {
    pub candidate_type: String,
    pub rule_ref: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeRuleCandidatePromotionInput<'a> {
    pub candidate_type: &'a str,
    pub primary_term: &'a str,
    pub target_ref: Option<&'a str>,
    pub catalog_version: &'a str,
    pub paths: &'a RuntimeRuleCandidatePromotionPaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRuleCandidatePromotionPatch {
    pub catalog_name: String,
    pub catalog_path: String,
    pub target_ref: String,
    pub before_sha256: String,
    pub after_sha256: String,
    pub changed: bool,
}

pub fn is_runtime_rule_candidate_type(candidate_type: &str) -> bool {
    RUNTIME_RULE_CANDIDATE_TYPES.contains(&candidate_type)
}

pub fn runtime_rule_candidate_target_requirement(candidate_type: &str) -> Option<Value> {
    let requirement = match candidate_type {
        QUERY_EXPANSION_TERM => json!({
            "catalog_name": "query_expansions",
            "target_ref_pattern": "query_expansion:<entry_id>",
            "target_field": "entries[].terms",
        }),
        QUERY_EXPANSION_EXACT_TERM => json!({
            "catalog_name": "query_expansions",
            "target_ref_pattern": "query_expansion:<entry_id>",
            "target_field": "entries[].exact_terms",
        }),
        ANSWER_EVIDENCE_REQUEST_TERM => json!({
            "catalog_name": "answer_rules",
            "target_ref_pattern": "answer_rules:answer_requirements.evidence_request_terms",
            "target_field": "answer_requirements.evidence_request_terms",
        }),
        RUNTIME_PERSON_ALIAS => json!({
            "catalog_name": "ontology_aliases",
            "target_ref_pattern": "person_alias:<person_id>",
            "target_field": "people[].aliases",
        }),
        _ => return None,
    };
    Some(requirement)
}

pub fn active_runtime_rule_candidate_matches(
    paths: &RuntimeRuleCandidatePromotionPaths,
    candidate_type: &str,
    term: &str,
) -> Result<Vec<RuntimeRuleCandidateActiveMatch>> {
    let term_key = rule_candidate_term_key(term);
    if term_key.is_empty() || !is_runtime_rule_candidate_type(candidate_type) {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    match candidate_type {
        QUERY_EXPANSION_TERM => {
            let catalog = active_catalog_json(
                "query_expansions",
                paths.query_expansions_path.as_ref(),
                DEFAULT_QUERY_EXPANSIONS_JSON,
            )?;
            for entry in array_at(&catalog, &["entries"])? {
                let entry_id = required_str(entry, &["id"])?;
                push_array_matches(
                    &mut matches,
                    candidate_type,
                    &term_key,
                    entry,
                    &["terms"],
                    &format!("query_expansions.entry:{entry_id}.terms"),
                )?;
            }
        }
        QUERY_EXPANSION_EXACT_TERM => {
            let catalog = active_catalog_json(
                "query_expansions",
                paths.query_expansions_path.as_ref(),
                DEFAULT_QUERY_EXPANSIONS_JSON,
            )?;
            for entry in array_at(&catalog, &["entries"])? {
                let entry_id = required_str(entry, &["id"])?;
                push_array_matches(
                    &mut matches,
                    candidate_type,
                    &term_key,
                    entry,
                    &["exact_terms"],
                    &format!("query_expansions.entry:{entry_id}.exact_terms"),
                )?;
            }
        }
        ANSWER_EVIDENCE_REQUEST_TERM => {
            let catalog = active_catalog_json(
                "answer_rules",
                paths.answer_rules_path.as_ref(),
                DEFAULT_ANSWER_RULES_JSON,
            )?;
            push_array_matches(
                &mut matches,
                candidate_type,
                &term_key,
                &catalog,
                &["answer_requirements", "evidence_request_terms"],
                "answer_rules.answer_requirements.evidence_request_terms",
            )?;
        }
        RUNTIME_PERSON_ALIAS => {
            let catalog = active_catalog_json(
                "ontology_aliases",
                paths.ontology_aliases_path.as_ref(),
                DEFAULT_ONTOLOGY_ALIASES_JSON,
            )?;
            for person in array_at(&catalog, &["people"])? {
                let person_id = required_str(person, &["person_id"])?;
                push_one_match(
                    &mut matches,
                    candidate_type,
                    &term_key,
                    required_str(person, &["canonical_name"])?,
                    &format!("ontology_aliases.person:{person_id}.canonical_name"),
                );
                push_array_matches(
                    &mut matches,
                    candidate_type,
                    &term_key,
                    person,
                    &["aliases"],
                    &format!("ontology_aliases.person:{person_id}.aliases"),
                )?;
            }
        }
        _ => {}
    }
    Ok(matches)
}

pub fn promote_runtime_rule_candidate_to_catalog(
    input: RuntimeRuleCandidatePromotionInput<'_>,
) -> Result<RuntimeRuleCandidatePromotionPatch> {
    match input.candidate_type {
        QUERY_EXPANSION_TERM => patch_query_expansion_array(input, "terms"),
        QUERY_EXPANSION_EXACT_TERM => patch_query_expansion_array(input, "exact_terms"),
        ANSWER_EVIDENCE_REQUEST_TERM => patch_answer_evidence_request_term(input),
        RUNTIME_PERSON_ALIAS => patch_runtime_person_alias(input),
        other => Err(anyhow!(
            "unsupported runtime rule candidate type for promotion: {other}"
        )),
    }
}

pub fn validate_runtime_rule_catalog_source(catalog_name: &str, source: &str) -> Result<()> {
    match catalog_name {
        "query_expansions" => validate_query_expansion_catalog_source(source),
        "answer_rules" => answer_rules::validate_answer_rule_catalog_source(source),
        "ontology_aliases" => ontology_aliases::validate_ontology_alias_catalog_source(source),
        other => Err(anyhow!("unsupported runtime rule catalog: {other}")),
    }
}

fn patch_query_expansion_array(
    input: RuntimeRuleCandidatePromotionInput<'_>,
    field: &str,
) -> Result<RuntimeRuleCandidatePromotionPatch> {
    let path =
        input.paths.query_expansions_path.as_ref().ok_or_else(|| {
            anyhow!("{QUERY_EXPANSIONS_PATH_ENV} is required for runtime promotion")
        })?;
    let target_ref = required_target_ref(input.target_ref, "query_expansion:")?;
    let entry_id = target_ref.trim_start_matches("query_expansion:");
    patch_catalog_file(
        "query_expansions",
        path,
        &target_ref,
        input.catalog_version,
        |catalog| {
            let entry = query_expansion_entry_mut(catalog, entry_id)?;
            push_unique_string(ensure_array_field(entry, field)?, input.primary_term)
        },
    )
}

fn patch_answer_evidence_request_term(
    input: RuntimeRuleCandidatePromotionInput<'_>,
) -> Result<RuntimeRuleCandidatePromotionPatch> {
    let path = input
        .paths
        .answer_rules_path
        .as_ref()
        .ok_or_else(|| anyhow!("{ANSWER_RULES_PATH_ENV} is required for runtime promotion"))?;
    let target_ref = require_exact_target_ref(
        input.target_ref,
        "answer_rules:answer_requirements.evidence_request_terms",
    )?;
    patch_catalog_file(
        "answer_rules",
        path,
        &target_ref,
        input.catalog_version,
        |catalog| {
            let terms = array_mut_at(catalog, &["answer_requirements", "evidence_request_terms"])?;
            push_unique_string(terms, input.primary_term)
        },
    )
}

fn patch_runtime_person_alias(
    input: RuntimeRuleCandidatePromotionInput<'_>,
) -> Result<RuntimeRuleCandidatePromotionPatch> {
    let path =
        input.paths.ontology_aliases_path.as_ref().ok_or_else(|| {
            anyhow!("{ONTOLOGY_ALIASES_PATH_ENV} is required for runtime promotion")
        })?;
    let target_ref = required_target_ref(input.target_ref, "person_alias:")?;
    let person_id = target_ref.trim_start_matches("person_alias:");
    patch_catalog_file(
        "ontology_aliases",
        path,
        &target_ref,
        input.catalog_version,
        |catalog| {
            let people = array_mut_at(catalog, &["people"])?;
            let person = people
                .iter_mut()
                .find(|person| person.get("person_id").and_then(Value::as_str) == Some(person_id))
                .ok_or_else(|| anyhow!("ontology alias target not found: {target_ref}"))?;
            push_unique_string(ensure_array_field(person, "aliases")?, input.primary_term)
        },
    )
}

fn active_catalog_json(
    catalog_name: &str,
    path: Option<&PathBuf>,
    default_source: &str,
) -> Result<Value> {
    let source = match path {
        Some(path) => fs::read_to_string(path).with_context(|| {
            format!("{catalog_name} catalog is not readable: {}", path.display())
        })?,
        None => default_source.to_string(),
    };
    validate_runtime_rule_catalog_source(catalog_name, &source)?;
    serde_json::from_str(&source).with_context(|| format!("{catalog_name} catalog is not JSON"))
}

fn patch_catalog_file(
    catalog_name: &str,
    path: &PathBuf,
    target_ref: &str,
    catalog_version: &str,
    patch: impl FnOnce(&mut Value) -> Result<bool>,
) -> Result<RuntimeRuleCandidatePromotionPatch> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("runtime catalog file is not readable: {}", path.display()))?;
    let before_sha256 = hash_text(&source);
    let mut catalog: Value = serde_json::from_str(&source)
        .with_context(|| format!("runtime catalog file is not JSON: {}", path.display()))?;
    let changed = patch(&mut catalog)?;
    if changed {
        catalog["catalog_version"] = json!(catalog_version);
    }
    let updated = serde_json::to_string_pretty(&catalog)? + "\n";
    validate_runtime_rule_catalog_source(catalog_name, &updated)?;
    let after_sha256 = hash_text(&updated);
    if changed && before_sha256 != after_sha256 {
        write_catalog_atomically(path, &updated)?;
    }
    Ok(RuntimeRuleCandidatePromotionPatch {
        catalog_name: catalog_name.to_string(),
        catalog_path: path.display().to_string(),
        target_ref: target_ref.to_string(),
        before_sha256,
        after_sha256,
        changed,
    })
}

fn query_expansion_entry_mut<'a>(catalog: &'a mut Value, entry_id: &str) -> Result<&'a mut Value> {
    array_mut_at(catalog, &["entries"])?
        .iter_mut()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(entry_id))
        .ok_or_else(|| anyhow!("query expansion target not found: query_expansion:{entry_id}"))
}

fn required_target_ref(target_ref: Option<&str>, prefix: &str) -> Result<String> {
    let value = target_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("target_ref with prefix {prefix} is required"))?;
    if !value.starts_with(prefix) || value.len() == prefix.len() {
        return Err(anyhow!("target_ref must start with {prefix}"));
    }
    Ok(value.to_string())
}

fn require_exact_target_ref(target_ref: Option<&str>, expected: &str) -> Result<String> {
    let value = target_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("target_ref {expected} is required"))?;
    if value != expected {
        return Err(anyhow!("target_ref must be {expected}"));
    }
    Ok(value.to_string())
}

fn array_at<'a>(root: &'a Value, path: &[&str]) -> Result<&'a Vec<Value>> {
    let mut cursor = root;
    for key in path {
        cursor = cursor
            .get(*key)
            .ok_or_else(|| anyhow!("catalog path not found: {}", path.join(".")))?;
    }
    cursor
        .as_array()
        .ok_or_else(|| anyhow!("catalog path must be an array: {}", path.join(".")))
}

fn optional_array_at<'a>(root: &'a Value, path: &[&str]) -> Result<Option<&'a Vec<Value>>> {
    let mut cursor = root;
    for key in path {
        let Some(next) = cursor.get(*key) else {
            return Ok(None);
        };
        cursor = next;
    }
    cursor
        .as_array()
        .map(Some)
        .ok_or_else(|| anyhow!("catalog path must be an array: {}", path.join(".")))
}

fn array_mut_at<'a>(root: &'a mut Value, path: &[&str]) -> Result<&'a mut Vec<Value>> {
    let mut cursor = root;
    for key in &path[..path.len().saturating_sub(1)] {
        cursor = cursor
            .get_mut(*key)
            .ok_or_else(|| anyhow!("catalog path not found: {}", path.join(".")))?;
    }
    let leaf = path
        .last()
        .ok_or_else(|| anyhow!("catalog path must not be empty"))?;
    cursor
        .get_mut(*leaf)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("catalog path must be an array: {}", path.join(".")))
}

fn ensure_array_field<'a>(root: &'a mut Value, field: &str) -> Result<&'a mut Vec<Value>> {
    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("catalog target must be an object for field {field}"))?;
    if !object.contains_key(field) {
        object.insert(field.to_string(), json!([]));
    }
    object
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("catalog field must be an array: {field}"))
}

fn required_str<'a>(root: &'a Value, path: &[&str]) -> Result<&'a str> {
    let mut cursor = root;
    for key in path {
        cursor = cursor
            .get(*key)
            .ok_or_else(|| anyhow!("catalog path not found: {}", path.join(".")))?;
    }
    cursor
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "catalog path must be a non-empty string: {}",
                path.join(".")
            )
        })
}

fn push_array_matches(
    matches: &mut Vec<RuntimeRuleCandidateActiveMatch>,
    candidate_type: &str,
    term_key: &str,
    root: &Value,
    path: &[&str],
    rule_ref: &str,
) -> Result<()> {
    let Some(array) = optional_array_at(root, path)? else {
        return Ok(());
    };
    for value in array {
        if let Some(active_term) = value.as_str() {
            push_one_match(matches, candidate_type, term_key, active_term, rule_ref);
        }
    }
    Ok(())
}

fn push_one_match(
    matches: &mut Vec<RuntimeRuleCandidateActiveMatch>,
    candidate_type: &str,
    term_key: &str,
    active_term: &str,
    rule_ref: &str,
) {
    if rule_candidate_term_key(active_term) != term_key {
        return;
    }
    matches.push(RuntimeRuleCandidateActiveMatch {
        candidate_type: candidate_type.to_string(),
        rule_ref: rule_ref.to_string(),
    });
}

fn push_unique_string(array: &mut Vec<Value>, term: &str) -> Result<bool> {
    let term = term.trim();
    if term.is_empty() {
        return Err(anyhow!("promoted runtime rule term must not be empty"));
    }
    if array.iter().any(|value| value.as_str() == Some(term)) {
        return Ok(false);
    }
    array.push(json!(term));
    Ok(true)
}

fn write_catalog_atomically(path: &PathBuf, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("runtime catalog path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            anyhow!(
                "runtime catalog path has invalid file name: {}",
                path.display()
            )
        })?;
    let tmp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::now_v7().simple()
    ));
    fs::write(&tmp_path, content)
        .with_context(|| format!("write temp runtime catalog failed: {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("replace runtime catalog failed: {}", path.display()))?;
    Ok(())
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn rule_candidate_term_key(text: &str) -> String {
    normalize_for_search(text)
        .trim()
        .trim_matches(|ch| matches!(ch, '?' | '？' | '!' | '！' | '。' | '.' | ' '))
        .split_whitespace()
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests;
