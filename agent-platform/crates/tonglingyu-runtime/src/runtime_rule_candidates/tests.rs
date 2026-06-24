use super::*;
use serde_json::Value;

fn temp_catalog_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tonglingyu-runtime-rule-candidate-{name}-{}.json",
        uuid::Uuid::now_v7().simple()
    ))
}

fn write_temp_catalog(name: &str, source: &str) -> PathBuf {
    let path = temp_catalog_path(name);
    std::fs::write(&path, source).expect("write temp catalog");
    path
}

fn read_json(path: &PathBuf) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read catalog"))
        .expect("catalog json")
}

#[test]
fn active_matches_read_embedded_runtime_catalogs() {
    let paths = RuntimeRuleCandidatePromotionPaths::default();

    let matches =
        active_runtime_rule_candidate_matches(&paths, ANSWER_EVIDENCE_REQUEST_TERM, "脂批")
            .expect("active answer matches");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].candidate_type, ANSWER_EVIDENCE_REQUEST_TERM);
    assert_eq!(
        matches[0].rule_ref,
        "answer_rules.answer_requirements.evidence_request_terms"
    );
}

#[test]
fn promotes_query_expansion_term_and_rejects_duplicate_without_rewriting() {
    let path = write_temp_catalog("query", DEFAULT_QUERY_EXPANSIONS_JSON);
    let paths = RuntimeRuleCandidatePromotionPaths {
        query_expansions_path: Some(path.clone()),
        ..RuntimeRuleCandidatePromotionPaths::default()
    };

    let first = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: QUERY_EXPANSION_TERM,
        primary_term: "新召回词",
        target_ref: Some("query_expansion:core:tonglingyu"),
        catalog_version: "test.runtime.1",
        paths: &paths,
    })
    .expect("promote query term");
    assert!(first.changed);

    let catalog = read_json(&path);
    let entry = catalog["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["id"] == "core:tonglingyu")
        .expect("target entry");
    assert!(
        entry["terms"]
            .as_array()
            .expect("terms")
            .iter()
            .any(|term| term.as_str() == Some("新召回词"))
    );
    assert_eq!(catalog["catalog_version"], "test.runtime.1");

    let second = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: QUERY_EXPANSION_TERM,
        primary_term: "新召回词",
        target_ref: Some("query_expansion:core:tonglingyu"),
        catalog_version: "test.runtime.2",
        paths: &paths,
    })
    .expect("idempotent duplicate query term");
    assert!(!second.changed);

    std::fs::remove_file(path).ok();
}

#[test]
fn promotes_query_expansion_evidence_slot_term() {
    let path = write_temp_catalog("query-slot", DEFAULT_QUERY_EXPANSIONS_JSON);
    let paths = RuntimeRuleCandidatePromotionPaths {
        query_expansions_path: Some(path.clone()),
        ..RuntimeRuleCandidatePromotionPaths::default()
    };

    let patch = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: QUERY_EXPANSION_EVIDENCE_SLOT_TERM,
        primary_term: "新证据槽词",
        target_ref: Some("query_expansion:core:tonglingyu:evidence_slot:test_slot"),
        catalog_version: "test.runtime.slot",
        paths: &paths,
    })
    .expect("promote query evidence slot term");
    assert!(patch.changed);

    let catalog = read_json(&path);
    let entry = catalog["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["id"] == "core:tonglingyu")
        .expect("target entry");
    let slot = entry["evidence_slots"]
        .as_array()
        .expect("evidence slots")
        .iter()
        .find(|slot| slot["id"] == "test_slot")
        .expect("created slot");
    assert!(
        slot["terms"]
            .as_array()
            .expect("slot terms")
            .iter()
            .any(|term| term.as_str() == Some("新证据槽词"))
    );

    std::fs::remove_file(path).ok();
}

#[test]
fn promotes_answer_rule_evidence_request_term() {
    let path = write_temp_catalog("answer", DEFAULT_ANSWER_RULES_JSON);
    let paths = RuntimeRuleCandidatePromotionPaths {
        answer_rules_path: Some(path.clone()),
        ..RuntimeRuleCandidatePromotionPaths::default()
    };

    let patch = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: ANSWER_EVIDENCE_REQUEST_TERM,
        primary_term: "凭据",
        target_ref: Some("answer_rules:answer_requirements.evidence_request_terms"),
        catalog_version: "test.runtime.answer",
        paths: &paths,
    })
    .expect("promote answer evidence term");

    assert!(patch.changed);
    let catalog = read_json(&path);
    assert!(
        catalog["answer_requirements"]["evidence_request_terms"]
            .as_array()
            .expect("evidence request terms")
            .iter()
            .any(|term| term.as_str() == Some("凭据"))
    );

    std::fs::remove_file(path).ok();
}

#[test]
fn promotes_runtime_person_alias() {
    let path = write_temp_catalog("ontology", DEFAULT_ONTOLOGY_ALIASES_JSON);
    let paths = RuntimeRuleCandidatePromotionPaths {
        ontology_aliases_path: Some(path.clone()),
        ..RuntimeRuleCandidatePromotionPaths::default()
    };

    let patch = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: RUNTIME_PERSON_ALIAS,
        primary_term: "枕霞旧友",
        target_ref: Some("person_alias:person:xiangyun"),
        catalog_version: "test.runtime.ontology",
        paths: &paths,
    })
    .expect("promote person alias");

    assert!(patch.changed);
    let catalog = read_json(&path);
    let xiangyun = catalog["people"]
        .as_array()
        .expect("people")
        .iter()
        .find(|person| person["person_id"] == "person:xiangyun")
        .expect("xiangyun");
    assert!(
        xiangyun["aliases"]
            .as_array()
            .expect("aliases")
            .iter()
            .any(|alias| alias.as_str() == Some("枕霞旧友"))
    );

    std::fs::remove_file(path).ok();
}

#[test]
fn promotes_evidence_count_basis_terms() {
    let path = write_temp_catalog("evidence-slot", DEFAULT_EVIDENCE_SLOT_RULES_JSON);
    let paths = RuntimeRuleCandidatePromotionPaths {
        evidence_slot_rules_path: Some(path.clone()),
        ..RuntimeRuleCandidatePromotionPaths::default()
    };

    promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: EVIDENCE_COUNT_BASIS_QUESTION_TERM,
        primary_term: "遗落",
        target_ref: Some("evidence_count_basis:direct_loss:question_terms"),
        catalog_version: "test.runtime.evidence.1",
        paths: &paths,
    })
    .expect("promote question term");
    promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: EVIDENCE_COUNT_BASIS_COUNT_TERM,
        primary_term: "几桩",
        target_ref: Some("evidence_count_basis:direct_loss:count_question_terms"),
        catalog_version: "test.runtime.evidence.2",
        paths: &paths,
    })
    .expect("promote count term");

    let catalog = read_json(&path);
    let basis = catalog["count_bases"]
        .as_array()
        .expect("count bases")
        .iter()
        .find(|basis| basis["id"] == "direct_loss")
        .expect("direct loss basis");
    assert!(
        basis["question_terms"]
            .as_array()
            .expect("question terms")
            .iter()
            .any(|term| term.as_str() == Some("遗落"))
    );
    assert!(
        basis["count_question_terms"]
            .as_array()
            .expect("count terms")
            .iter()
            .any(|term| term.as_str() == Some("几桩"))
    );

    std::fs::remove_file(path).ok();
}

#[test]
fn promotion_requires_explicit_target_ref() {
    let path = write_temp_catalog("answer-target", DEFAULT_ANSWER_RULES_JSON);
    let paths = RuntimeRuleCandidatePromotionPaths {
        answer_rules_path: Some(path.clone()),
        ..RuntimeRuleCandidatePromotionPaths::default()
    };

    let err = promote_runtime_rule_candidate_to_catalog(RuntimeRuleCandidatePromotionInput {
        candidate_type: ANSWER_EVIDENCE_REQUEST_TERM,
        primary_term: "凭据",
        target_ref: None,
        catalog_version: "test.runtime.answer",
        paths: &paths,
    })
    .expect_err("missing target_ref must fail");

    assert!(err.to_string().contains("target_ref"));
    std::fs::remove_file(path).ok();
}
