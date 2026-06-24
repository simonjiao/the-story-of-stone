use crate::{
    RUNTIME_PROMPT_CATALOG_PATH_ENV,
    rule_catalog::{RuleFileCache, configured_path, lock_rule_cache},
    upstream_bundle::UPSTREAM_BUNDLE_SCHEMA_VERSION,
};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

const RUNTIME_PROMPT_CATALOG_SCHEMA_VERSION: &str = "tonglingyu.runtime_prompt_catalog.v1";
const DEFAULT_RUNTIME_PROMPT_CATALOG_JSON: &str =
    include_str!("../resources/runtime_prompt_catalog.json");

static RUNTIME_PROMPT_CATALOG_CACHE: OnceLock<Mutex<RuleFileCache<RuntimePromptCatalog>>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimePromptCatalog {
    schema_version: String,
    catalog_version: String,
    draft_answer: DraftAnswerPromptCatalog,
    review_answer: ReviewAnswerPromptCatalog,
    step_contracts: BTreeMap<String, StepPromptCatalog>,
    default_step_contract: StepPromptCatalog,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftAnswerPromptCatalog {
    result_summary_contract_parts: Vec<String>,
    compact_result_summary_contract_parts: Vec<String>,
    repair_context: DraftRepairContextCatalog,
    repair_instruction: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftRepairContextCatalog {
    object: String,
    required_action: String,
    full_rules: Vec<String>,
    compact_rules: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewAnswerPromptCatalog {
    result_summary_contract_parts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepPromptCatalog {
    result_summary_contract_parts: Vec<String>,
}

pub(crate) fn runtime_prompt_catalog_metadata() -> Result<Value> {
    let path = configured_path(RUNTIME_PROMPT_CATALOG_PATH_ENV);
    let cache =
        RUNTIME_PROMPT_CATALOG_CACHE.get_or_init(|| Mutex::new(default_runtime_prompt_cache()));
    let mut cache = lock_rule_cache(cache, "runtime_prompt")?;
    let catalog = cache.catalog(
        RUNTIME_PROMPT_CATALOG_PATH_ENV,
        path,
        default_runtime_prompt_catalog(),
        parse_runtime_prompt_catalog,
    )?;
    Ok(cache.metadata(
        RUNTIME_PROMPT_CATALOG_SCHEMA_VERSION,
        &catalog.catalog_version,
    ))
}

pub(crate) fn result_summary_contract_for_operation(operation: &str) -> Result<String> {
    let catalog = runtime_prompt_catalog()?;
    match operation {
        "draft_answer" => Ok(join_prompt_parts(
            &catalog.draft_answer.result_summary_contract_parts,
        )),
        "review_answer" => Ok(join_prompt_parts(
            &catalog.review_answer.result_summary_contract_parts,
        )),
        _ => Ok(catalog
            .step_contracts
            .get(operation)
            .map(|contract| join_prompt_parts(&contract.result_summary_contract_parts))
            .unwrap_or_else(|| {
                join_prompt_parts(&catalog.default_step_contract.result_summary_contract_parts)
            })),
    }
}

pub(crate) fn compact_draft_result_summary_contract() -> Result<String> {
    let catalog = runtime_prompt_catalog()?;
    Ok(join_prompt_parts(
        &catalog.draft_answer.compact_result_summary_contract_parts,
    ))
}

pub(crate) fn draft_repair_context(
    rejected_reason: &str,
    compaction_level: usize,
) -> Result<Value> {
    let catalog = runtime_prompt_catalog()?;
    let context = catalog.draft_answer.repair_context;
    let rules = if compaction_level >= 4 {
        context.compact_rules
    } else {
        context.full_rules
    };
    Ok(json!({
        "object": context.object,
        "rejected_reason": rejected_reason,
        "required_action": context.required_action,
        "rules": rules,
    }))
}

pub(crate) fn draft_repair_instruction() -> Result<String> {
    let catalog = runtime_prompt_catalog()?;
    Ok(catalog.draft_answer.repair_instruction)
}

fn runtime_prompt_catalog() -> Result<RuntimePromptCatalog> {
    let path = configured_path(RUNTIME_PROMPT_CATALOG_PATH_ENV);
    let cache =
        RUNTIME_PROMPT_CATALOG_CACHE.get_or_init(|| Mutex::new(default_runtime_prompt_cache()));
    let mut cache = lock_rule_cache(cache, "runtime_prompt")?;
    cache.catalog(
        RUNTIME_PROMPT_CATALOG_PATH_ENV,
        path,
        default_runtime_prompt_catalog(),
        parse_runtime_prompt_catalog,
    )
}

fn default_runtime_prompt_cache() -> RuleFileCache<RuntimePromptCatalog> {
    RuleFileCache::new(default_runtime_prompt_catalog())
}

fn default_runtime_prompt_catalog() -> RuntimePromptCatalog {
    parse_runtime_prompt_catalog(DEFAULT_RUNTIME_PROMPT_CATALOG_JSON)
        .expect("embedded runtime prompt catalog must parse")
}

fn parse_runtime_prompt_catalog(source: &str) -> Result<RuntimePromptCatalog> {
    let catalog: RuntimePromptCatalog =
        serde_json::from_str(source).context("runtime prompt catalog must be JSON")?;
    if catalog.schema_version != RUNTIME_PROMPT_CATALOG_SCHEMA_VERSION {
        return Err(anyhow!(
            "runtime prompt catalog schema_version must be {}",
            RUNTIME_PROMPT_CATALOG_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "runtime prompt catalog catalog_version is required"
        ));
    }
    validate_prompt_parts(
        "draft_answer.result_summary_contract_parts",
        &catalog.draft_answer.result_summary_contract_parts,
    )?;
    validate_prompt_parts(
        "draft_answer.compact_result_summary_contract_parts",
        &catalog.draft_answer.compact_result_summary_contract_parts,
    )?;
    validate_prompt_parts(
        "review_answer.result_summary_contract_parts",
        &catalog.review_answer.result_summary_contract_parts,
    )?;
    for (operation, contract) in &catalog.step_contracts {
        validate_prompt_parts(
            &format!("step_contracts.{operation}.result_summary_contract_parts"),
            &contract.result_summary_contract_parts,
        )?;
    }
    validate_prompt_parts(
        "default_step_contract.result_summary_contract_parts",
        &catalog.default_step_contract.result_summary_contract_parts,
    )?;
    validate_prompt_parts(
        "draft_answer.repair_context.full_rules",
        &catalog.draft_answer.repair_context.full_rules,
    )?;
    validate_prompt_parts(
        "draft_answer.repair_context.compact_rules",
        &catalog.draft_answer.repair_context.compact_rules,
    )?;
    validate_required_text(
        "draft_answer.repair_context.object",
        &catalog.draft_answer.repair_context.object,
    )?;
    validate_required_text(
        "draft_answer.repair_context.required_action",
        &catalog.draft_answer.repair_context.required_action,
    )?;
    validate_required_text(
        "draft_answer.repair_instruction",
        &catalog.draft_answer.repair_instruction,
    )?;
    let draft_contract = join_prompt_parts(&catalog.draft_answer.result_summary_contract_parts);
    let compact_contract =
        join_prompt_parts(&catalog.draft_answer.compact_result_summary_contract_parts);
    for (name, value) in [
        ("draft_answer.result_summary_contract_parts", draft_contract),
        (
            "draft_answer.compact_result_summary_contract_parts",
            compact_contract,
        ),
    ] {
        if !value.contains(UPSTREAM_BUNDLE_SCHEMA_VERSION) {
            return Err(anyhow!(
                "runtime prompt catalog {name} must mention {}",
                UPSTREAM_BUNDLE_SCHEMA_VERSION
            ));
        }
        if !value.contains("evidence_refs") {
            return Err(anyhow!(
                "runtime prompt catalog {name} must mention evidence_refs"
            ));
        }
    }
    Ok(catalog)
}

fn validate_prompt_parts(name: &str, parts: &[String]) -> Result<()> {
    if parts.is_empty() {
        return Err(anyhow!("runtime prompt catalog {name} must not be empty"));
    }
    if parts.iter().any(|part| part.trim().is_empty()) {
        return Err(anyhow!(
            "runtime prompt catalog {name} must not contain blank parts"
        ));
    }
    Ok(())
}

fn validate_required_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("runtime prompt catalog {name} is required"));
    }
    Ok(())
}

fn join_prompt_parts(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static PROMPT_CATALOG_TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn embedded_catalog_exposes_draft_and_review_contracts() {
        let _guard = PROMPT_CATALOG_TEST_ENV_LOCK.lock().expect("env test lock");
        unsafe {
            std::env::remove_var(RUNTIME_PROMPT_CATALOG_PATH_ENV);
        }
        let draft = result_summary_contract_for_operation("draft_answer").expect("draft contract");
        let review =
            result_summary_contract_for_operation("review_answer").expect("review contract");
        let repair = draft_repair_context("draft_missing_anchor", 0).expect("repair context");

        assert!(draft.contains(UPSTREAM_BUNDLE_SCHEMA_VERSION));
        assert!(draft.contains("answer_use=supplemental_only"));
        assert!(review.contains("review_observation"));
        assert_eq!(repair["object"], json!("tonglingyu.draft_repair_context"));
    }

    #[test]
    fn catalog_cache_hot_reloads_external_file() {
        let _guard = PROMPT_CATALOG_TEST_ENV_LOCK.lock().expect("env test lock");
        let path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-prompt-catalog-{}.json",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&path, DEFAULT_RUNTIME_PROMPT_CATALOG_JSON).expect("write first catalog");
        unsafe {
            std::env::set_var(RUNTIME_PROMPT_CATALOG_PATH_ENV, &path);
        }
        let first = runtime_prompt_catalog_metadata().expect("first metadata");
        let mut value: Value = serde_json::from_str(DEFAULT_RUNTIME_PROMPT_CATALOG_JSON)
            .expect("default catalog json");
        value["catalog_version"] = json!("runtime-prompt-test-hot-reload");
        fs::write(
            &path,
            serde_json::to_string_pretty(&value).expect("serialize catalog"),
        )
        .expect("write second catalog");
        let second = runtime_prompt_catalog_metadata().expect("second metadata");
        unsafe {
            std::env::remove_var(RUNTIME_PROMPT_CATALOG_PATH_ENV);
        }
        let _ = fs::remove_file(path);

        assert_eq!(first["source"], json!("external_file"));
        assert_eq!(
            second["catalog_version"],
            json!("runtime-prompt-test-hot-reload")
        );
    }

    #[test]
    fn invalid_external_catalog_fails_without_fallback() {
        let _guard = PROMPT_CATALOG_TEST_ENV_LOCK.lock().expect("env test lock");
        let path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-prompt-catalog-invalid-{}.json",
            uuid::Uuid::now_v7().simple()
        ));
        fs::write(&path, r#"{"schema_version":"wrong"}"#).expect("write invalid catalog");
        unsafe {
            std::env::set_var(RUNTIME_PROMPT_CATALOG_PATH_ENV, &path);
        }
        let error = runtime_prompt_catalog_metadata().expect_err("invalid catalog must fail");
        unsafe {
            std::env::remove_var(RUNTIME_PROMPT_CATALOG_PATH_ENV);
        }
        let _ = fs::remove_file(path);

        assert!(error.to_string().contains("is not a valid catalog"));
    }
}
