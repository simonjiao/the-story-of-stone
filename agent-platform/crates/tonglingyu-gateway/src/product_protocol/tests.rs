use serde_json::{Value, json};

use super::*;

fn fixture(name: &str) -> Value {
    let raw = match name {
        "capabilities" => include_str!("../../fixtures/story-of-stone/capabilities.v1.json"),
        "create" => include_str!("../../fixtures/story-of-stone/product-run-create.v1.json"),
        "event" => include_str!("../../fixtures/story-of-stone/product-run-event.v1.json"),
        _ => panic!("unknown fixture"),
    };
    serde_json::from_str(raw).expect("valid fixture json")
}

#[test]
fn parses_and_validates_shared_story_of_stone_fixtures() {
    let capabilities: GatewayCapabilities =
        serde_json::from_value(fixture("capabilities")).expect("capabilities");
    let create: ProductRunCreateRequest =
        serde_json::from_value(fixture("create")).expect("create");
    let event: ProductRunEvent = serde_json::from_value(fixture("event")).expect("event");

    assert_eq!(
        validate_capabilities(capabilities).expect("valid").products[0].id,
        "writing-assistant"
    );
    validate_create_request(&create).expect("valid create");
    validate_event(&event, 11).expect("valid event");
}

#[test]
fn rejects_unknown_fields_versions_empty_ids_and_non_increasing_sequences() {
    let mut unknown_field = fixture("create");
    unknown_field["executor_url"] = json!("https://attacker.example");
    assert!(serde_json::from_value::<ProductRunCreateRequest>(unknown_field).is_err());

    let mut wrong_version: ProductRunCreateRequest =
        serde_json::from_value(fixture("create")).expect("create");
    wrong_version.schema_version = "story-of-stone.product-run.v2".to_string();
    assert!(validate_create_request(&wrong_version).is_err());
    wrong_version.schema_version = PRODUCT_RUN_SCHEMA_VERSION.to_string();
    wrong_version.request_id.clear();
    assert!(validate_create_request(&wrong_version).is_err());

    let event: ProductRunEvent = serde_json::from_value(fixture("event")).expect("event");
    assert!(validate_event(&event, event.sequence).is_err());
}
