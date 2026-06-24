use super::*;
use rusqlite::Connection;
use serde_json::json;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    crate::init_runtime_schema(&conn).expect("runtime schema");
    conn
}

fn valid_manifest_input(kind: &str) -> PromotionManifestInput {
    PromotionManifestInput {
        artifact_kind: kind.to_string(),
        candidate_ids: vec![format!("{kind}-candidate-a")],
        source_trace_refs: vec!["trace-online-learning-a".to_string()],
        source_span_refs: json!([{
            "source_id": "source-a",
            "block_id": "block-a",
            "source_hash": "hash-a",
            "span_start": 0,
            "span_end": 12
        }]),
        rule_diff_refs: json!([{
            "target_ref": "catalog:entry-a",
            "before_sha256": "before",
            "after_sha256": "after"
        }]),
        merge_conflict_decision: json!({
            "status": "passed",
            "decision": "merge"
        }),
        target_ref: format!("{kind}:target-store"),
        expected_version_bump: "0.35.0".to_string(),
        regression_cases: json!([{
            "suite_ref": "conversation_cases.small20",
            "status": "passed",
            "case_count": 20,
            "passed_count": 20,
            "failed_count": 0,
            "skipped_count": 0
        }]),
        reviewer_policy: json!({
            "actor": "test-admin",
            "policy": "manual_review_plus_regression"
        }),
        dry_run_result: json!({
            "status": "passed",
            "errors": []
        }),
        rollback_ref: json!({
            "kind": "catalog_sha",
            "before_sha256": "before"
        }),
    }
}

#[test]
fn records_passed_manifest_and_trace_index() {
    let conn = test_conn();
    let manifest = record_online_learning_promotion_manifest(&conn, valid_manifest_input("prompt"))
        .expect("manifest");

    assert_eq!(manifest["status"], json!("passed"));
    let trace_manifests =
        list_online_learning_promotion_manifests_for_trace(&conn, "trace-online-learning-a", 10)
            .expect("trace manifests");
    assert_eq!(trace_manifests.len(), 1);
    assert_eq!(
        trace_manifests[0]["promotion_batch_id"],
        manifest["promotion_batch_id"]
    );
    assert_eq!(trace_manifests[0]["artifact_kind"], json!("prompt"));
}

#[test]
fn conflict_or_regression_blocks_whole_batch() {
    let conn = test_conn();
    let mut input = valid_manifest_input("rule");
    input.candidate_ids.push("rule-candidate-b".to_string());
    input.merge_conflict_decision = json!({
        "status": "conflict",
        "has_conflict": true
    });
    input.regression_cases = json!([{
        "suite_ref": "conversation_cases.small20",
        "status": "failed",
        "case_count": 20,
        "passed_count": 18,
        "failed_count": 2,
        "skipped_count": 0
    }]);

    let manifest =
        record_online_learning_promotion_manifest(&conn, input).expect("blocked manifest");

    assert_eq!(manifest["status"], json!("blocked"));
    let blockers = manifest["blocking_reasons"]
        .as_array()
        .expect("blocking reasons");
    assert!(blockers.contains(&json!("merge_conflict_blocks_batch")));
    assert!(blockers.contains(&json!("regression_failure_blocks_batch")));
}
