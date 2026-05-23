use crate::{EVIDENCE_SLOT_RULES_PATH_ENV, normalize_text};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const EVIDENCE_SLOT_RULES_SCHEMA_VERSION: &str = "tonglingyu.evidence_slot_rules.v1";
const DEFAULT_EVIDENCE_SLOT_RULES_JSON: &str =
    include_str!("../resources/evidence_slot_rules.json");

static EVIDENCE_SLOT_RULES_CATALOG_CACHE: OnceLock<Mutex<EvidenceSlotRuleCatalogCache>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceSlotRuleCatalog {
    pub schema_version: String,
    pub catalog_version: String,
    #[serde(default)]
    pub count_bases: Vec<EvidenceSlotCountBasis>,
    #[serde(default)]
    pub slots: Vec<EvidenceSlotRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceSlotCountBasis {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub question_terms: Vec<String>,
    #[serde(default)]
    pub count_question_terms: Vec<String>,
    #[serde(default)]
    pub total_count_units: Vec<String>,
    #[serde(default)]
    pub total_count_prefixes: Vec<String>,
    pub answer_unit: String,
    pub answer_noun: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceSlotRule {
    pub id: String,
    pub label: String,
    pub role: String,
    pub public_role_label: String,
    #[serde(default)]
    pub counts_as: Vec<String>,
    pub display_group: String,
    #[serde(default)]
    pub count_note: Option<String>,
}

#[derive(Debug, Clone)]
struct EvidenceSlotRuleCatalogCache {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    catalog: EvidenceSlotRuleCatalog,
}

impl Default for EvidenceSlotRuleCatalogCache {
    fn default() -> Self {
        Self {
            path: None,
            modified: None,
            len: 0,
            catalog: parse_evidence_slot_rule_catalog(DEFAULT_EVIDENCE_SLOT_RULES_JSON)
                .expect("embedded evidence slot rule catalog must parse"),
        }
    }
}

impl EvidenceSlotRuleCatalogCache {
    fn catalog(&mut self, path: Option<PathBuf>) -> Result<EvidenceSlotRuleCatalog> {
        let Some(path) = path else {
            if self.path.is_some() {
                *self = Self::default();
            }
            return Ok(self.catalog.clone());
        };
        let metadata = fs::metadata(&path).with_context(|| {
            format!(
                "{}={} is not readable",
                EVIDENCE_SLOT_RULES_PATH_ENV,
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
                EVIDENCE_SLOT_RULES_PATH_ENV,
                path.display()
            )
        })?;
        let catalog = parse_evidence_slot_rule_catalog(&source).with_context(|| {
            format!(
                "{}={} is not a valid evidence slot rule catalog",
                EVIDENCE_SLOT_RULES_PATH_ENV,
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

fn configured_evidence_slot_rules_path() -> Option<PathBuf> {
    std::env::var(EVIDENCE_SLOT_RULES_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn evidence_slot_rule_catalog() -> Result<EvidenceSlotRuleCatalog> {
    let path = configured_evidence_slot_rules_path();
    let cache = EVIDENCE_SLOT_RULES_CATALOG_CACHE
        .get_or_init(|| Mutex::new(EvidenceSlotRuleCatalogCache::default()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow!("evidence slot rule catalog cache is poisoned"))?;
    cache.catalog(path)
}

fn parse_evidence_slot_rule_catalog(source: &str) -> Result<EvidenceSlotRuleCatalog> {
    let catalog: EvidenceSlotRuleCatalog =
        serde_json::from_str(source).context("evidence slot rule catalog must be JSON")?;
    if catalog.schema_version != EVIDENCE_SLOT_RULES_SCHEMA_VERSION {
        return Err(anyhow!(
            "evidence slot rule catalog schema_version must be {}",
            EVIDENCE_SLOT_RULES_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "evidence slot rule catalog catalog_version is required"
        ));
    }
    let mut count_basis_ids = BTreeSet::new();
    for basis in &catalog.count_bases {
        if basis.id.trim().is_empty() {
            return Err(anyhow!("evidence slot count basis id is required"));
        }
        if !count_basis_ids.insert(basis.id.clone()) {
            return Err(anyhow!("duplicate evidence slot count basis {}", basis.id));
        }
        if basis.label.trim().is_empty()
            || basis.answer_unit.trim().is_empty()
            || basis.answer_noun.trim().is_empty()
        {
            return Err(anyhow!(
                "evidence slot count basis {} must define label, answer_unit, and answer_noun",
                basis.id
            ));
        }
        if basis
            .question_terms
            .iter()
            .all(|term| term.trim().is_empty())
        {
            return Err(anyhow!(
                "evidence slot count basis {} must define non-empty question_terms",
                basis.id
            ));
        }
        for (field, values) in [
            ("count_question_terms", &basis.count_question_terms),
            ("total_count_units", &basis.total_count_units),
            ("total_count_prefixes", &basis.total_count_prefixes),
        ] {
            if values.is_empty() || values.iter().all(|term| term.trim().is_empty()) {
                return Err(anyhow!(
                    "evidence slot count basis {} must define non-empty {}",
                    basis.id,
                    field
                ));
            }
        }
    }
    let mut slot_ids = BTreeSet::new();
    for slot in &catalog.slots {
        if slot.id.trim().is_empty() {
            return Err(anyhow!("evidence slot rule id is required"));
        }
        if !slot_ids.insert(slot.id.clone()) {
            return Err(anyhow!("duplicate evidence slot rule {}", slot.id));
        }
        if slot.label.trim().is_empty()
            || slot.role.trim().is_empty()
            || slot.public_role_label.trim().is_empty()
            || slot.display_group.trim().is_empty()
        {
            return Err(anyhow!(
                "evidence slot rule {} must define label, role, public_role_label, and display_group",
                slot.id
            ));
        }
        if slot
            .count_note
            .as_deref()
            .is_some_and(|note| note.trim().is_empty())
        {
            return Err(anyhow!(
                "evidence slot rule {} count_note must be non-empty when provided",
                slot.id
            ));
        }
        for basis in &slot.counts_as {
            if basis.trim().is_empty() {
                return Err(anyhow!(
                    "evidence slot rule {} has an empty counts_as value",
                    slot.id
                ));
            }
        }
    }
    Ok(catalog)
}

pub(crate) fn active_count_basis_for_question(
    question: &str,
    count_question: bool,
) -> Result<Option<EvidenceSlotCountBasis>> {
    if !count_question {
        return Ok(None);
    }
    let catalog = evidence_slot_rule_catalog()?;
    let normalized = normalize_text(question);
    Ok(catalog.count_bases.into_iter().find(|basis| {
        basis
            .question_terms
            .iter()
            .any(|term| question_matches_term(question, &normalized, term))
    }))
}

pub(crate) fn question_asks_for_count(question: &str) -> Result<bool> {
    let catalog = evidence_slot_rule_catalog()?;
    let normalized = normalize_text(question);
    Ok(catalog.count_bases.iter().any(|basis| {
        basis
            .count_question_terms
            .iter()
            .any(|term| question_matches_term(question, &normalized, term))
    }))
}

pub(crate) fn explicit_total_count_for_basis(
    text: &str,
    basis: &EvidenceSlotCountBasis,
) -> Option<usize> {
    let compact = text.split_whitespace().collect::<String>();
    (1..=9)
        .filter(|count| total_count_marker_present(&compact, *count, basis))
        .max()
}

fn total_count_marker_present(text: &str, count: usize, basis: &EvidenceSlotCountBasis) -> bool {
    let mut count_words = vec![count.to_string()];
    count_words.extend(
        chinese_count_words(count)
            .into_iter()
            .map(ToOwned::to_owned),
    );
    for prefix in &basis.total_count_prefixes {
        for count_word in &count_words {
            for unit in &basis.total_count_units {
                if text.contains(&format!("{}{}{}", prefix.trim(), count_word, unit.trim())) {
                    return true;
                }
            }
        }
    }
    false
}

fn chinese_count_words(count: usize) -> Vec<&'static str> {
    match count {
        1 => vec!["一"],
        2 => vec!["二", "两", "兩"],
        3 => vec!["三"],
        4 => vec!["四"],
        5 => vec!["五"],
        6 => vec!["六"],
        7 => vec!["七"],
        8 => vec!["八"],
        9 => vec!["九"],
        _ => Vec::new(),
    }
}

pub(crate) fn evidence_slot_count_policy_value(
    question: &str,
    count_question: bool,
) -> Result<Value> {
    let active_basis = active_count_basis_for_question(question, count_question)?;
    Ok(json!({
        "schema_version": EVIDENCE_SLOT_RULES_SCHEMA_VERSION,
        "active_count_basis": active_basis,
        "count_question": count_question,
        "rule": "Count only evidence slots whose semantic rule counts_as contains active_count_basis.id; other slots may be displayed as related clues but must not change the direct count."
    }))
}

pub(crate) fn evidence_slot_rules_for_ids(slot_ids: &[String]) -> Result<Vec<EvidenceSlotRule>> {
    let catalog = evidence_slot_rule_catalog()?;
    let rules_by_id = catalog
        .slots
        .into_iter()
        .map(|rule| (rule.id.clone(), rule))
        .collect::<BTreeMap<_, _>>();
    Ok(slot_ids
        .iter()
        .map(|slot_id| {
            rules_by_id
                .get(slot_id)
                .cloned()
                .unwrap_or_else(|| unknown_slot_rule(slot_id))
        })
        .collect())
}

pub(crate) fn evidence_slot_rule_values_for_ids(slot_ids: &[String]) -> Result<Vec<Value>> {
    Ok(evidence_slot_rules_for_ids(slot_ids)?
        .into_iter()
        .map(|rule| {
            json!({
                "id": rule.id,
                "label": rule.label,
                "role": rule.role,
                "public_role_label": rule.public_role_label,
                "counts_as": rule.counts_as,
                "display_group": rule.display_group,
                "count_note": rule.count_note,
            })
        })
        .collect())
}

fn unknown_slot_rule(slot_id: &str) -> EvidenceSlotRule {
    EvidenceSlotRule {
        id: slot_id.to_string(),
        label: slot_id.to_string(),
        role: "unclassified".to_string(),
        public_role_label: "相关线索".to_string(),
        counts_as: Vec::new(),
        display_group: "unclassified".to_string(),
        count_note: None,
    }
}

fn question_matches_term(question: &str, normalized: &str, term: &str) -> bool {
    let term = term.trim();
    !term.is_empty() && (question.contains(term) || normalized.contains(&normalize_text(term)))
}

#[cfg(test)]
mod tests;
