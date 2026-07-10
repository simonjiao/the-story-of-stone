use serde_json::json;

use super::*;

#[test]
fn routes_only_normalized_supported_products() {
    assert_eq!(
        product_route(&json!({"input": "question"})).expect("route"),
        None
    );
    let route = product_route(&json!({
        "metadata": {
            PRODUCT_METADATA_KEY: {
                "product_id": "writing-assistant",
                "chat_ref": "chat-1",
                "external_message_id": "message-1"
            }
        }
    }))
    .expect("route")
    .expect("product route");
    assert_eq!(route.product_id, WRITING_ASSISTANT_PRODUCT_ID);
    assert_eq!(route.chat_ref, "chat-1");
    assert_eq!(route.external_message_id, "message-1");
}

#[test]
fn rejects_unknown_products_and_incomplete_normalized_metadata() {
    assert!(
        product_route(&json!({
            "metadata": { PRODUCT_METADATA_KEY: {
                "product_id": "unknown", "chat_ref": "chat-1", "external_message_id": "message-1"
            }}
        }))
        .is_err()
    );
    assert!(
        product_route(&json!({
            "metadata": { PRODUCT_METADATA_KEY: {
                "product_id": "writing-assistant", "chat_ref": "chat-1"
            }}
        }))
        .is_err()
    );
}
