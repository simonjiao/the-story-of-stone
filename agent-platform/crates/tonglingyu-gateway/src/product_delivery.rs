use std::{sync::Arc, time::Duration};

use crate::openwebui_delivery::{
    OpenWebuiDelivery, OpenWebuiDeliveryError, delivery_for_product_event,
};
use crate::product_binding::{
    ProductBindingStatus, ProductBindingStoreError, ProductDeliveryStatus, ProductRunBinding,
};
use crate::product_protocol::ProductRunEvent;
use crate::product_run_worker::{
    binding_studio_error, product_binding_execution_error, product_binding_for_delivery,
    save_product_binding,
};
use crate::response_events::ResponseStatus;
use crate::response_store::ResponseEventStore;
use crate::{AppState, ResponseJobExecutionError};

pub(crate) fn prepare_product_delivery(
    gateway_run_id: &str,
    event: &ProductRunEvent,
    binding: &mut ProductRunBinding,
) -> OpenWebuiDelivery {
    let delivery = delivery_for_product_event(gateway_run_id, event);
    if binding.delivery_id.as_deref() != Some(&delivery.id) {
        binding.delivery_attempts = 0;
    }
    binding.delivery_status = ProductDeliveryStatus::Pending;
    binding.delivery_last_error_code = None;
    binding.delivery_retryable = true;
    binding.delivery_id = Some(delivery.id.clone());
    binding.delivery_body = Some(delivery.body.clone());
    binding.delivery_snapshot = Some(delivery.snapshot.clone());
    delivery
}

pub(crate) async fn attempt_product_delivery(
    state: &AppState,
    binding: ProductRunBinding,
    delivery: OpenWebuiDelivery,
) -> Result<(), crate::studio_http::StudioHttpError> {
    attempt_delivery(state, binding, delivery).await
}

pub(crate) fn spawn_recovery_worker(state: Arc<AppState>, interval_secs: u64, max_attempts: u32) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = recover_pending(&state, max_attempts).await {
                tracing::warn!(
                    error_code = error.code,
                    "Open WebUI delivery recovery pass failed"
                );
            }
            tokio::time::sleep(Duration::from_secs(interval_secs.max(1))).await;
        }
    });
}

async fn recover_pending(
    state: &AppState,
    max_attempts: u32,
) -> Result<(), ResponseJobExecutionError> {
    reconcile_terminal_response_bindings(state)?;
    let stores = state.product_bindings.clone();
    let bindings = tokio::task::spawn_blocking(move || {
        stores
            .lock()
            .map_err(|_| {
                ResponseJobExecutionError::new(
                    "product_binding_store_unavailable",
                    "product binding store is unavailable",
                )
            })?
            .pending_deliveries(100)
            .map_err(product_binding_execution_error)
    })
    .await
    .map_err(|_| {
        ResponseJobExecutionError::new(
            "product_delivery_worker_failed",
            "product delivery recovery worker failed",
        )
    })??;
    for binding in bindings {
        match recovery_decision(&binding, max_attempts) {
            RecoveryDecision::Skip => {}
            RecoveryDecision::DeadLetter => {
                mark_delivery_dead_letter(&state, binding, "openwebui_delivery_attempts_exhausted")
                    .map_err(|error| ResponseJobExecutionError::new(error.code, error.message))?;
            }
            RecoveryDecision::Deliver(delivery) => {
                if let Err(error) = attempt_delivery(state, binding, delivery).await {
                    tracing::warn!(
                        error_code = error.code,
                        "Open WebUI delivery recovery attempt failed"
                    );
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn reconcile_terminal_response_bindings(
    state: &AppState,
) -> Result<(), ResponseJobExecutionError> {
    let bindings = state
        .product_bindings
        .lock()
        .map_err(|_| {
            ResponseJobExecutionError::new(
                "product_binding_store_unavailable",
                "product binding store is unavailable",
            )
        })?
        .active_bindings()
        .map_err(product_binding_execution_error)?;
    for mut binding in bindings {
        let response_status = state
            .response_store
            .lock()
            .map_err(|_| {
                ResponseJobExecutionError::new(
                    "response_store_unavailable",
                    "response store is unavailable",
                )
            })?
            .state(&binding.response_id)
            .map_err(|_| {
                ResponseJobExecutionError::new(
                    "response_state_unavailable",
                    "response state is unavailable",
                )
            })?
            .status;
        if !response_status.is_terminal() {
            continue;
        }
        let expected_version = binding.version;
        binding.status = terminal_binding_status(&response_status);
        let result = state
            .product_bindings
            .lock()
            .map_err(|_| {
                ResponseJobExecutionError::new(
                    "product_binding_store_unavailable",
                    "product binding store is unavailable",
                )
            })?
            .save(binding, expected_version);
        match result {
            Ok(_) | Err(ProductBindingStoreError::BindingConflict(_)) => {}
            Err(error) => return Err(product_binding_execution_error(error)),
        }
    }
    Ok(())
}

fn terminal_binding_status(status: &ResponseStatus) -> ProductBindingStatus {
    match status {
        ResponseStatus::Completed => ProductBindingStatus::Completed,
        ResponseStatus::Canceled => ProductBindingStatus::Canceled,
        ResponseStatus::Failed | ResponseStatus::Timeout | ResponseStatus::Expired => {
            ProductBindingStatus::Failed
        }
        _ => ProductBindingStatus::Running,
    }
}

async fn attempt_delivery(
    state: &AppState,
    binding: ProductRunBinding,
    delivery: OpenWebuiDelivery,
) -> Result<(), crate::studio_http::StudioHttpError> {
    let base_attempts = binding.delivery_attempts;
    let Some(client) = state.openwebui_delivery.as_ref() else {
        return mark_delivery_failed(
            state,
            binding,
            base_attempts + 1,
            "openwebui_delivery_not_configured",
            false,
        );
    };
    let response_id = binding.response_id.clone();
    let result = client
        .deliver(
            &binding.openwebui_chat_id,
            &binding.openwebui_assistant_message_id,
            &delivery,
            |attempt, error| {
                update_delivery_retrying(state, &response_id, base_attempts + attempt, error.code)
            },
        )
        .await;
    let mut latest =
        product_binding_for_delivery(state, &binding.response_id).map_err(binding_studio_error)?;
    let expected_version = latest.version;
    apply_delivery_result(&mut latest, base_attempts, &result);
    if let Err(error) = &result {
        tracing::warn!(
            response_id = %binding.response_id,
            delivery_id = %delivery.id,
            error_code = error.code,
            "Open WebUI product notification entered failed delivery state"
        );
    }
    save_product_binding(state, latest, expected_version).map_err(binding_studio_error)?;
    Ok(())
}

fn persisted_delivery(binding: &ProductRunBinding) -> Option<OpenWebuiDelivery> {
    Some(OpenWebuiDelivery {
        id: binding.delivery_id.clone()?,
        body: binding.delivery_body.clone()?,
        snapshot: binding.delivery_snapshot.clone()?,
    })
}

enum RecoveryDecision {
    Skip,
    Deliver(OpenWebuiDelivery),
    DeadLetter,
}

fn recovery_decision(binding: &ProductRunBinding, max_attempts: u32) -> RecoveryDecision {
    if binding.delivery_attempts >= max_attempts.max(1) {
        return RecoveryDecision::DeadLetter;
    }
    persisted_delivery(binding)
        .map(RecoveryDecision::Deliver)
        .unwrap_or(RecoveryDecision::Skip)
}

fn apply_delivery_result(
    binding: &mut ProductRunBinding,
    base_attempts: u32,
    result: &Result<u32, OpenWebuiDeliveryError>,
) {
    match result {
        Ok(attempts) => {
            binding.delivery_status = ProductDeliveryStatus::Delivered;
            binding.delivery_attempts = base_attempts + attempts;
            binding.delivery_last_error_code = None;
            binding.delivery_retryable = false;
        }
        Err(error) => {
            binding.delivery_status = if error.retryable {
                ProductDeliveryStatus::Failed
            } else {
                ProductDeliveryStatus::DeadLetter
            };
            binding.delivery_attempts = binding.delivery_attempts.max(base_attempts) + 1;
            binding.delivery_last_error_code = Some(error.code.to_string());
            binding.delivery_retryable = error.retryable;
        }
    }
}

fn update_delivery_retrying(
    state: &AppState,
    response_id: &str,
    attempts: u32,
    error_code: &str,
) -> Result<(), OpenWebuiDeliveryError> {
    let mut binding = product_binding_for_delivery(state, response_id).map_err(|error| {
        OpenWebuiDeliveryError {
            code: error.code,
            retryable: true,
        }
    })?;
    let expected_version = binding.version;
    binding.delivery_status = ProductDeliveryStatus::Retrying;
    binding.delivery_attempts = attempts;
    binding.delivery_last_error_code = Some(error_code.to_string());
    binding.delivery_retryable = true;
    save_product_binding(state, binding, expected_version)
        .map(|_| ())
        .map_err(|error| OpenWebuiDeliveryError {
            code: error.code,
            retryable: true,
        })
}

fn mark_delivery_failed(
    state: &AppState,
    mut binding: ProductRunBinding,
    attempts: u32,
    error_code: &str,
    retryable: bool,
) -> Result<(), crate::studio_http::StudioHttpError> {
    let expected_version = binding.version;
    binding.delivery_status = ProductDeliveryStatus::Failed;
    binding.delivery_attempts = attempts;
    binding.delivery_last_error_code = Some(error_code.to_string());
    binding.delivery_retryable = retryable;
    save_product_binding(state, binding, expected_version)
        .map(|_| ())
        .map_err(binding_studio_error)
}

fn mark_delivery_dead_letter(
    state: &AppState,
    mut binding: ProductRunBinding,
    error_code: &str,
) -> Result<(), crate::studio_http::StudioHttpError> {
    let expected_version = binding.version;
    binding.delivery_status = ProductDeliveryStatus::DeadLetter;
    binding.delivery_last_error_code = Some(error_code.to_string());
    binding.delivery_retryable = false;
    save_product_binding(state, binding, expected_version)
        .map(|_| ())
        .map_err(binding_studio_error)
}

#[cfg(test)]
mod tests;
