use crate::{ONTOLOGY_ALIASES_PATH_ENV, normalize_alias};
use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const ONTOLOGY_ALIASES_SCHEMA_VERSION: &str = "tonglingyu.ontology_aliases.v1";
const DEFAULT_ONTOLOGY_ALIASES_JSON: &str = include_str!("../resources/ontology_aliases.json");

static ONTOLOGY_ALIAS_CATALOG_CACHE: OnceLock<Mutex<OntologyAliasCatalogCache>> = OnceLock::new();

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
struct OntologyAliasCatalogCache {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    catalog: OntologyAliasCatalog,
}

impl Default for OntologyAliasCatalogCache {
    fn default() -> Self {
        Self {
            path: None,
            modified: None,
            len: 0,
            catalog: parse_ontology_alias_catalog(DEFAULT_ONTOLOGY_ALIASES_JSON)
                .expect("embedded ontology alias catalog must parse"),
        }
    }
}

impl OntologyAliasCatalogCache {
    fn catalog(&mut self, path: Option<PathBuf>) -> Result<OntologyAliasCatalog> {
        let Some(path) = path else {
            if self.path.is_some() {
                *self = Self::default();
            }
            return Ok(self.catalog.clone());
        };
        let metadata = fs::metadata(&path).with_context(|| {
            format!(
                "{}={} is not readable",
                ONTOLOGY_ALIASES_PATH_ENV,
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
                ONTOLOGY_ALIASES_PATH_ENV,
                path.display()
            )
        })?;
        let catalog = parse_ontology_alias_catalog(&source).with_context(|| {
            format!(
                "{}={} is not a valid ontology alias catalog",
                ONTOLOGY_ALIASES_PATH_ENV,
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

fn configured_ontology_aliases_path() -> Option<PathBuf> {
    std::env::var(ONTOLOGY_ALIASES_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn ontology_alias_catalog() -> Result<OntologyAliasCatalog> {
    let path = configured_ontology_aliases_path();
    let cache = ONTOLOGY_ALIAS_CATALOG_CACHE
        .get_or_init(|| Mutex::new(OntologyAliasCatalogCache::default()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow!("ontology alias catalog cache is poisoned"))?;
    cache.catalog(path)
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

pub(crate) fn seed_aliases(conn: &Connection) -> Result<()> {
    let catalog = ontology_alias_catalog()?;
    for person in catalog.people {
        conn.execute(
            "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
            params![person.person_id, person.canonical_name, person.description],
        )?;
        for alias in person.aliases {
            conn.execute(
                "INSERT INTO aliases (alias, normalized_alias, person_id, scope) VALUES (?1, ?2, ?3, ?4)",
                params![alias, normalize_alias(&alias), person.person_id, "v1_seed_alias"],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
