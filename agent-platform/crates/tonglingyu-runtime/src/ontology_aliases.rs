use crate::{
    ONTOLOGY_ALIASES_PATH_ENV,
    rule_catalog::{RuleFileCache, configured_path, lock_rule_cache},
};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

const ONTOLOGY_ALIASES_SCHEMA_VERSION: &str = "tonglingyu.ontology_aliases.v1";
const DEFAULT_ONTOLOGY_ALIASES_JSON: &str = include_str!("../resources/ontology_aliases.json");

static ONTOLOGY_ALIAS_CATALOG_CACHE: OnceLock<Mutex<RuleFileCache<OntologyAliasCatalog>>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OntologyAliasCatalog {
    schema_version: String,
    catalog_version: String,
    people: Vec<PersonAliasEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonAliasEntry {
    person_id: String,
    canonical_name: String,
    description: String,
    aliases: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PersonAliasView {
    pub(crate) canonical_name: String,
    pub(crate) aliases: Vec<String>,
}

fn ontology_alias_catalog() -> Result<OntologyAliasCatalog> {
    let path = configured_path(ONTOLOGY_ALIASES_PATH_ENV);
    let cache =
        ONTOLOGY_ALIAS_CATALOG_CACHE.get_or_init(|| Mutex::new(default_ontology_alias_cache()));
    let mut cache = lock_rule_cache(cache, "ontology alias")?;
    cache.catalog(
        ONTOLOGY_ALIASES_PATH_ENV,
        path,
        default_ontology_alias_catalog(),
        parse_ontology_alias_catalog,
    )
}

pub(crate) fn ontology_alias_catalog_metadata() -> Result<Value> {
    let path = configured_path(ONTOLOGY_ALIASES_PATH_ENV);
    let cache =
        ONTOLOGY_ALIAS_CATALOG_CACHE.get_or_init(|| Mutex::new(default_ontology_alias_cache()));
    let mut cache = lock_rule_cache(cache, "ontology alias")?;
    let catalog = cache.catalog(
        ONTOLOGY_ALIASES_PATH_ENV,
        path,
        default_ontology_alias_catalog(),
        parse_ontology_alias_catalog,
    )?;
    Ok(cache.metadata(ONTOLOGY_ALIASES_SCHEMA_VERSION, &catalog.catalog_version))
}

fn default_ontology_alias_cache() -> RuleFileCache<OntologyAliasCatalog> {
    RuleFileCache::new(default_ontology_alias_catalog())
}

fn default_ontology_alias_catalog() -> OntologyAliasCatalog {
    parse_ontology_alias_catalog(DEFAULT_ONTOLOGY_ALIASES_JSON)
        .expect("embedded ontology alias catalog must parse")
}

fn parse_ontology_alias_catalog(source: &str) -> Result<OntologyAliasCatalog> {
    let catalog: OntologyAliasCatalog =
        serde_json::from_str(source).context("ontology alias catalog must be JSON")?;
    if catalog.schema_version != ONTOLOGY_ALIASES_SCHEMA_VERSION {
        return Err(anyhow!(
            "ontology alias catalog schema_version must be {}",
            ONTOLOGY_ALIASES_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "ontology alias catalog catalog_version is required"
        ));
    }
    if catalog.people.is_empty() {
        return Err(anyhow!("ontology alias catalog people is required"));
    }
    for person in &catalog.people {
        if person.person_id.trim().is_empty()
            || person.canonical_name.trim().is_empty()
            || person.description.trim().is_empty()
            || person.aliases.is_empty()
            || person.aliases.iter().all(|alias| alias.trim().is_empty())
        {
            return Err(anyhow!(
                "ontology alias catalog people entries require person_id, canonical_name, description, and aliases"
            ));
        }
    }
    Ok(catalog)
}

pub(crate) fn validate_ontology_alias_catalog_source(source: &str) -> Result<()> {
    parse_ontology_alias_catalog(source).map(|_| ())
}

pub(crate) fn people_aliases() -> Result<Vec<PersonAliasView>> {
    let catalog = ontology_alias_catalog()?;
    Ok(catalog
        .people
        .into_iter()
        .map(|person| {
            let mut aliases = vec![person.canonical_name.clone()];
            aliases.extend(person.aliases);
            PersonAliasView {
                canonical_name: person.canonical_name,
                aliases,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests;
