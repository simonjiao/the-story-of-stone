use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub(crate) struct RuleFileCache<T> {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    catalog: T,
}

impl<T: Clone> RuleFileCache<T> {
    pub(crate) fn new(catalog: T) -> Self {
        Self {
            path: None,
            modified: None,
            len: 0,
            catalog,
        }
    }

    pub(crate) fn catalog(
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

    pub(crate) fn metadata(&self, schema_version: &str, catalog_version: &str) -> Value {
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

pub(crate) fn configured_path(env_name: &str) -> Option<PathBuf> {
    std::env::var(env_name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn lock_rule_cache<'a, T>(
    cache: &'a Mutex<RuleFileCache<T>>,
    catalog_name: &str,
) -> Result<MutexGuard<'a, RuleFileCache<T>>> {
    cache
        .lock()
        .map_err(|_| anyhow!("{catalog_name} rule catalog cache is poisoned"))
}

fn system_time_unix_ms(value: SystemTime) -> Option<u128> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
    struct TestCatalog {
        version: String,
    }

    fn parse_test_catalog(source: &str) -> Result<TestCatalog> {
        Ok(serde_json::from_str(source)?)
    }

    #[test]
    fn uses_embedded_default_without_external_path() {
        let default = TestCatalog {
            version: "embedded".to_string(),
        };
        let mut cache = RuleFileCache::new(default.clone());

        let catalog = cache
            .catalog("TEST_RULE_PATH", None, default, parse_test_catalog)
            .expect("embedded catalog");
        let metadata = cache.metadata("test.schema", &catalog.version);

        assert_eq!(catalog.version, "embedded");
        assert_eq!(metadata["source"], json!("embedded_default"));
        assert_eq!(metadata["path"], Value::Null);
    }

    #[test]
    fn reloads_external_file_on_len_change() {
        let path = std::env::temp_dir().join(format!(
            "tonglingyu-rule-cache-{}.json",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&path, r#"{"version":"external-one"}"#).expect("write first catalog");
        let default = TestCatalog {
            version: "embedded".to_string(),
        };
        let mut cache = RuleFileCache::new(default.clone());

        let first = cache
            .catalog(
                "TEST_RULE_PATH",
                Some(path.clone()),
                default.clone(),
                parse_test_catalog,
            )
            .expect("first external catalog");
        fs::write(&path, r#"{"version":"external-two-longer"}"#).expect("write second catalog");
        let second = cache
            .catalog(
                "TEST_RULE_PATH",
                Some(path.clone()),
                default,
                parse_test_catalog,
            )
            .expect("second external catalog");
        let metadata = cache.metadata("test.schema", &second.version);

        assert_eq!(first.version, "external-one");
        assert_eq!(second.version, "external-two-longer");
        assert_eq!(metadata["source"], json!("external_file"));
        assert_eq!(metadata["path"], json!(path.display().to_string()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_external_file_fails_without_fallback() {
        let path = std::env::temp_dir().join(format!(
            "tonglingyu-rule-cache-invalid-{}.json",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&path, r#"{"version":]"#).expect("write invalid catalog");
        let default = TestCatalog {
            version: "embedded".to_string(),
        };
        let mut cache = RuleFileCache::new(default.clone());

        let error = cache
            .catalog(
                "TEST_RULE_PATH",
                Some(path.clone()),
                default,
                parse_test_catalog,
            )
            .expect_err("invalid external catalog should fail");

        assert!(error.to_string().contains("is not a valid catalog"));
        assert_eq!(cache.catalog.version, "embedded");
        let _ = fs::remove_file(path);
    }
}
