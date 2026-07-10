use serde_json::from_str;

use super::*;
use crate::product_protocol::{GatewayCapabilities, validate_capabilities};

fn capabilities() -> GatewayCapabilities {
    validate_capabilities(
        from_str(include_str!(
            "../../fixtures/story-of-stone/capabilities.v1.json"
        ))
        .expect("fixture"),
    )
    .expect("valid capabilities")
}

#[test]
fn requires_capabilities_and_a_durable_binding_store() {
    let registry = ProductRegistry::from_studio_capabilities(&capabilities(), true);
    assert!(
        registry
            .require_available(WRITING_ASSISTANT_PRODUCT_ID)
            .is_ok()
    );

    let registry = ProductRegistry::from_studio_capabilities(&capabilities(), false);
    assert!(
        registry
            .require_available(WRITING_ASSISTANT_PRODUCT_ID)
            .is_err()
    );
}

#[test]
fn keeps_unknown_products_fail_closed() {
    let registry = ProductRegistry::from_studio_capabilities(&capabilities(), true);
    assert!(registry.require_available("illustrated-book").is_err());
}
