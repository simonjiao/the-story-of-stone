use super::*;
use crate::product_binding::ProductRunBinding;
use serde_json::json;

#[test]
fn rebuilds_persisted_delivery_with_stable_id_and_body() {
    let mut binding = ProductRunBinding::new(
        "response-1",
        "run-1",
        "writing-assistant",
        "chat-1",
        "message-1",
    );
    binding.delivery_id = Some("delivery-1".to_string());
    binding.delivery_body = Some(json!({"type": "replace"}));
    binding.delivery_snapshot = Some("done".to_string());

    let delivery = persisted_delivery(&binding).expect("persisted delivery");

    assert_eq!(delivery.id, "delivery-1");
    assert_eq!(delivery.body["type"], "replace");
}

#[test]
fn a_new_studio_event_gets_an_independent_delivery_attempt_budget() {
    let mut binding = ProductRunBinding::new(
        "response-1",
        "run-1",
        "writing-assistant",
        "chat-1",
        "message-1",
    );
    binding.delivery_id = Some("product-delivery:run-1:old-event".to_string());
    binding.delivery_attempts = 24;
    let event = crate::product_protocol::ProductRunEvent {
        schema_version: crate::product_protocol::PRODUCT_RUN_EVENT_SCHEMA_VERSION.to_string(),
        event_id: "completed-event".to_string(),
        run_id: "studio-run-1".to_string(),
        product_id: "writing-assistant".to_string(),
        sequence: 9,
        event_type: crate::product_protocol::ProductRunEventType::RunCompleted,
        created_at: "2026-07-10T00:00:00Z".to_string(),
        payload: json!({"summary": "done", "artifacts": []}),
    };

    let delivery = prepare_product_delivery("run-1", &event, &mut binding);

    assert_eq!(delivery.id, "product-delivery:run-1:completed-event");
    assert_eq!(binding.delivery_attempts, 0);
    assert_eq!(binding.delivery_status, ProductDeliveryStatus::Pending);
}

#[test]
fn recovery_exhaustion_moves_delivery_to_dead_letter() {
    let mut binding = ProductRunBinding::new(
        "response-1",
        "run-1",
        "writing-assistant",
        "chat-1",
        "message-1",
    );
    binding.delivery_id = Some("delivery-1".to_string());
    binding.delivery_body = Some(json!({"type": "replace"}));
    binding.delivery_snapshot = Some("done".to_string());
    binding.delivery_attempts = 3;

    assert!(matches!(
        recovery_decision(&binding, 3),
        RecoveryDecision::DeadLetter
    ));
}

#[test]
fn delivery_result_transitions_are_persistable_and_retry_aware() {
    let mut delivered = ProductRunBinding::new(
        "response-1",
        "run-1",
        "writing-assistant",
        "chat-1",
        "message-1",
    );
    apply_delivery_result(&mut delivered, 2, &Ok(1));
    assert_eq!(delivered.delivery_status, ProductDeliveryStatus::Delivered);
    assert_eq!(delivered.delivery_attempts, 3);

    let mut retryable = delivered.clone();
    apply_delivery_result(
        &mut retryable,
        3,
        &Err(OpenWebuiDeliveryError {
            code: "temporary",
            retryable: true,
        }),
    );
    assert_eq!(retryable.delivery_status, ProductDeliveryStatus::Failed);
    assert!(retryable.delivery_retryable);

    let mut rejected = delivered;
    apply_delivery_result(
        &mut rejected,
        3,
        &Err(OpenWebuiDeliveryError {
            code: "unauthorized",
            retryable: false,
        }),
    );
    assert_eq!(rejected.delivery_status, ProductDeliveryStatus::DeadLetter);
    assert!(!rejected.delivery_retryable);
}
