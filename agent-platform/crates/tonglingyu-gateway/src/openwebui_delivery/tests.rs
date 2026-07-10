use serde_json::from_str;

use super::*;

#[test]
fn completed_delivery_contains_only_summary_and_artifact_references() {
    let mut event: ProductRunEvent = from_str(include_str!(
        "../../fixtures/story-of-stone/product-run-event.v1.json"
    ))
    .expect("fixture");
    event.event_type = ProductRunEventType::RunCompleted;
    event.payload = json!({
        "summary": "写作任务已完成。",
        "artifacts": [{"id": "article-1", "kind": "article", "title": "晴雯"}],
        "full_text": "不应复制到 Open WebUI 的完整正文"
    });
    let delivery = delivery_for_product_event("run-1", &event);
    assert!(delivery.snapshot.contains("article-1"));
    assert!(!delivery.snapshot.contains("完整正文"));
    assert_eq!(delivery.body["type"], "replace");
}

#[test]
fn requires_action_delivery_keeps_gateway_and_action_ids_for_recovery() {
    let mut event: ProductRunEvent = from_str(include_str!(
        "../../fixtures/story-of-stone/product-run-event.v1.json"
    ))
    .expect("fixture");
    event.event_type = ProductRunEventType::RunRequiresAction;
    event.payload = json!({"action": {"id": "action-1", "title": "确认任务卡"}});
    let delivery = delivery_for_product_event("run-1", &event);
    assert!(delivery.snapshot.contains("Run ID: run-1"));
    assert!(delivery.snapshot.contains("Action ID: action-1"));
}
