#![allow(dead_code)]

use crate::product_protocol::{
    GatewayCapabilities, PRODUCT_RUN_EVENT_SCHEMA_VERSION, PRODUCT_RUN_SCHEMA_VERSION,
};
use crate::product_router::WRITING_ASSISTANT_PRODUCT_ID;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProductAvailability {
    Available,
    Unavailable { reason: String },
}

#[derive(Debug, Clone)]
pub(crate) struct ProductRegistration {
    pub(crate) product_id: String,
    pub(crate) executor: &'static str,
    pub(crate) availability: ProductAvailability,
}

#[derive(Debug, Clone)]
pub(crate) struct ProductRegistry {
    writing: ProductRegistration,
}

impl ProductRegistry {
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            writing: ProductRegistration {
                product_id: WRITING_ASSISTANT_PRODUCT_ID.to_string(),
                executor: "story-of-stone-studio",
                availability: ProductAvailability::Unavailable {
                    reason: reason.into(),
                },
            },
        }
    }

    pub(crate) fn from_studio_capabilities(
        capabilities: &GatewayCapabilities,
        durable_store: bool,
    ) -> Self {
        if !durable_store {
            return Self::unavailable("durable Redis product binding store is required");
        }
        let protocols_ready = capabilities
            .protocol_versions
            .iter()
            .any(|version| version == PRODUCT_RUN_SCHEMA_VERSION)
            && capabilities
                .protocol_versions
                .iter()
                .any(|version| version == PRODUCT_RUN_EVENT_SCHEMA_VERSION);
        let product_ready = capabilities.products.iter().any(|product| {
            product.id == WRITING_ASSISTANT_PRODUCT_ID
                && product.actions
                && !product.artifacts.is_empty()
        });
        if !protocols_ready || !product_ready {
            return Self::unavailable(
                "Studio capabilities do not satisfy writing-assistant requirements",
            );
        }
        Self {
            writing: ProductRegistration {
                product_id: WRITING_ASSISTANT_PRODUCT_ID.to_string(),
                executor: "story-of-stone-studio",
                availability: ProductAvailability::Available,
            },
        }
    }

    pub(crate) fn require_available(
        &self,
        product_id: &str,
    ) -> Result<&ProductRegistration, String> {
        if product_id != self.writing.product_id {
            return Err(format!("product is not registered: {product_id}"));
        }
        match &self.writing.availability {
            ProductAvailability::Available => Ok(&self.writing),
            ProductAvailability::Unavailable { reason } => Err(reason.clone()),
        }
    }

    pub(crate) fn writing(&self) -> &ProductRegistration {
        &self.writing
    }
}

#[cfg(test)]
mod tests;
