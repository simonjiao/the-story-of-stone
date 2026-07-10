#![allow(dead_code)]

use serde_json::Value;

pub(crate) const PRODUCT_METADATA_KEY: &str = "_tonglingyu_product";
pub(crate) const WRITING_ASSISTANT_PRODUCT_ID: &str = "writing-assistant";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProductRoute {
    pub(crate) product_id: String,
    pub(crate) chat_ref: String,
    pub(crate) external_message_id: String,
}

pub(crate) fn product_route(request: &Value) -> Result<Option<ProductRoute>, String> {
    let Some(metadata) = request.get("metadata").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(product) = metadata.get(PRODUCT_METADATA_KEY) else {
        return Ok(None);
    };
    let product = product
        .as_object()
        .ok_or_else(|| "normalized product metadata must be an object".to_string())?;
    let product_id = required_string(product.get("product_id"), "product_id")?;
    if product_id != WRITING_ASSISTANT_PRODUCT_ID {
        return Err(format!("unsupported product: {product_id}"));
    }
    Ok(Some(ProductRoute {
        product_id,
        chat_ref: required_string(product.get("chat_ref"), "chat_ref")?,
        external_message_id: required_string(
            product.get("external_message_id"),
            "external_message_id",
        )?,
    }))
}

fn required_string(value: Option<&Value>, label: &str) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("normalized product metadata is missing {label}"))
}

#[cfg(test)]
mod tests;
