use serde_json::from_str;

use super::*;

#[test]
fn projects_studio_status_and_requires_action() {
    let mut event: ProductRunEvent = from_str(include_str!(
        "../../fixtures/story-of-stone/product-run-event.v1.json"
    ))
    .expect("fixture");
    let status = project_product_event(&event).expect("status");
    assert_eq!(status[0].event_type, ResponseEventType::ResponseStatus);
    assert_eq!(status[0].status_update, Some(ResponseStatus::InProgress));

    event.event_type = ProductRunEventType::RunRequiresAction;
    event.payload = serde_json::json!({"action": {"id": "action-1", "title": "确认任务卡"}});
    let action = project_product_event(&event).expect("action");
    assert_eq!(
        action[0].event_type,
        ResponseEventType::ResponseRequiresAction
    );
    assert_eq!(action[0].pending_action_id.as_deref(), Some("action-1"));
}

#[test]
fn completed_event_emits_summary_before_terminal_state() {
    let mut event: ProductRunEvent = from_str(include_str!(
        "../../fixtures/story-of-stone/product-run-event.v1.json"
    ))
    .expect("fixture");
    event.event_type = ProductRunEventType::RunCompleted;
    event.payload = serde_json::json!({
        "summary": "写作任务已完成。",
        "artifacts": [{"id": "article-1", "kind": "article", "title": "晴雯"}]
    });
    let projected = project_product_event(&event).expect("complete");
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].event_type, ResponseEventType::OutputTextDone);
    assert_eq!(
        projected[1].event_type,
        ResponseEventType::ResponseCompleted
    );
    assert!(projected[1].terminal);
}
