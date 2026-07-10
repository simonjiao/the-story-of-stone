use std::sync::Arc;

use serde_json::Value;

use crate::product_binding::{ProductBindingStatus, ProductBindingStoreError, ProductRunBinding};
use crate::product_delivery::{attempt_product_delivery, prepare_product_delivery};
use crate::product_protocol::{
    self, PRODUCT_RUN_SCHEMA_VERSION, ProductRunCreateRequest, ProductRunIdentity, ProductRunInput,
};
use crate::product_router::ProductRoute;
use crate::remote_product_run::project_product_event;
use crate::response_events::{ResponseEvent, ResponseEventType, ResponseStatus};
use crate::response_jobs::ResponseJob;
use crate::{
    AppState, ResponseJobExecutionError, append_response_event_for_job, response_events_for_id,
    response_state_record_for_job,
};

pub(crate) async fn execute_product_response_job(
    state: Arc<AppState>,
    job: ResponseJob,
    message: String,
    route: ProductRoute,
) -> Result<(), ResponseJobExecutionError> {
    state
        .product_registry
        .require_available(&route.product_id)
        .map_err(|_| {
            ResponseJobExecutionError::new(
                "product_unavailable",
                "requested product is unavailable",
            )
        })?;
    let studio = state.studio_client.clone().ok_or_else(|| {
        ResponseJobExecutionError::new("product_unavailable", "Studio client is unavailable")
    })?;
    let new_binding = ProductRunBinding::new(
        &job.response_id,
        &job.run_id,
        &route.product_id,
        &route.chat_ref,
        &route.external_message_id,
    );
    let mut binding = {
        let mut store = state.product_bindings.lock().map_err(|_| {
            ResponseJobExecutionError::new(
                "product_binding_store_unavailable",
                "product binding store is unavailable",
            )
        })?;
        store
            .create(new_binding)
            .map_err(product_binding_execution_error)?
    };
    let user_ref = job
        .user_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&job.subject)
        .to_string();
    let create_request = ProductRunCreateRequest {
        schema_version: PRODUCT_RUN_SCHEMA_VERSION.to_string(),
        request_id: job.run_id.clone(),
        external_message_id: route.external_message_id,
        trace_id: job.trace_id.clone(),
        product_id: route.product_id,
        identity: ProductRunIdentity {
            issuer: "tonglingyu-gateway".to_string(),
            user_ref,
            chat_ref: route.chat_ref,
        },
        input: ProductRunInput {
            message,
            article_id: None,
            workspace_id: None,
            section_id: None,
            target_stage: None,
            replace_existing: None,
        },
    };
    let remote = studio
        .create_run(&create_request)
        .await
        .map_err(studio_execution_error)?;
    if remote.request_id != job.run_id
        || remote.external_message_id != create_request.external_message_id
        || remote.product_id != create_request.product_id
    {
        return Err(ResponseJobExecutionError::new(
            "studio_contract_invalid",
            "Studio returned a Product Run bound to another request",
        ));
    }
    if binding
        .remote_run_id
        .as_deref()
        .is_some_and(|run_id| run_id != remote.id)
    {
        return Err(ResponseJobExecutionError::new(
            "product_binding_conflict",
            "Product Run is already bound to another Studio Run",
        ));
    }
    if binding.remote_run_id.is_none() {
        let expected_version = binding.version;
        binding.remote_run_id = Some(remote.id.clone());
        binding.status = product_binding_status(&remote.status);
        binding = save_product_binding(&state, binding, expected_version)?;
    }
    let remote_run_id = binding.remote_run_id.clone().ok_or_else(|| {
        ResponseJobExecutionError::new("product_binding_invalid", "Studio Run binding is missing")
    })?;
    let gateway_state = response_state_record_for_job(&state, &job)?;
    if gateway_state.cancel_requested || gateway_state.status == ResponseStatus::Canceling {
        studio
            .cancel(&remote_run_id)
            .await
            .map_err(studio_execution_error)?;
    }
    let start_sequence = binding.remote_last_sequence;
    let stream_state = state.clone();
    let stream_job = job.clone();
    let stream_remote_run_id = remote_run_id.clone();
    let stream_product_id = binding.product_id.clone();
    studio
        .stream_events(&remote_run_id, start_sequence, move |event| {
            let state = stream_state.clone();
            let job = stream_job.clone();
            let remote_run_id = stream_remote_run_id.clone();
            let product_id = stream_product_id.clone();
            async move { process_product_event(state, job, remote_run_id, product_id, event).await }
        })
        .await
        .map_err(studio_execution_error)
}

async fn process_product_event(
    state: Arc<AppState>,
    job: ResponseJob,
    remote_run_id: String,
    product_id: String,
    event: product_protocol::ProductRunEvent,
) -> Result<bool, crate::studio_http::StudioHttpError> {
    if event.run_id != remote_run_id || event.product_id != product_id {
        return Err(crate::studio_http::StudioHttpError {
            code: "studio_contract_invalid",
            message: "Studio event does not match the bound Product Run".to_string(),
            retryable: false,
        });
    }
    let mut binding = {
        let store =
            state
                .product_bindings
                .lock()
                .map_err(|_| crate::studio_http::StudioHttpError {
                    code: "product_binding_store_unavailable",
                    message: "product binding store is unavailable".to_string(),
                    retryable: true,
                })?;
        store.get(&job.response_id).map_err(|error| {
            let error = product_binding_execution_error(error);
            crate::studio_http::StudioHttpError {
                code: error.code,
                message: error.message,
                retryable: true,
            }
        })?
    };
    let projected =
        project_product_event(&event).map_err(|message| crate::studio_http::StudioHttpError {
            code: "studio_contract_invalid",
            message,
            retryable: false,
        })?;
    let duplicate_action = projected
        .iter()
        .find_map(|item| item.pending_action_id.as_deref())
        .is_some_and(|action_id| binding.pending_remote_action_id.as_deref() == Some(action_id));
    let existing_events = response_events_for_id(&state, &job.response_id).map_err(|response| {
        crate::studio_http::StudioHttpError {
            code: "response_event_read_failed",
            message: format!(
                "failed to inspect projected Studio event: {}",
                response.status()
            ),
            retryable: true,
        }
    })?;
    if !duplicate_action {
        for item in &projected {
            if product_projection_exists(&existing_events, &item.event_type, &event.event_id) {
                continue;
            }
            let final_response_ref = if item.terminal {
                event
                    .payload
                    .get("artifacts")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            };
            append_response_event_for_job(
                &state,
                &job,
                item.event_type.clone(),
                item.payload.clone(),
                item.status_update.clone(),
                None,
                None,
                final_response_ref,
            )
            .map_err(|error| crate::studio_http::StudioHttpError {
                code: error.code,
                message: error.message,
                retryable: true,
            })?;
        }
    }
    let expected_version = binding.version;
    binding.remote_last_sequence = event.sequence;
    if let Some(artifacts) = event.payload.get("artifacts") {
        binding.artifacts = serde_json::from_value(artifacts.clone()).map_err(|_| {
            crate::studio_http::StudioHttpError {
                code: "studio_contract_invalid",
                message: "Studio event contains invalid artifact references".to_string(),
                retryable: false,
            }
        })?;
    }
    if let Some(last) = projected.last() {
        binding.status = last.binding_status.clone();
    }
    binding.pending_remote_action_id = match event.event_type {
        product_protocol::ProductRunEventType::RunRequiresAction => projected
            .iter()
            .find_map(|item| item.pending_action_id.clone()),
        product_protocol::ProductRunEventType::RunResumed
        | product_protocol::ProductRunEventType::RunCompleted
        | product_protocol::ProductRunEventType::RunFailed
        | product_protocol::ProductRunEventType::RunCanceled => None,
        _ => binding.pending_remote_action_id.clone(),
    };
    let delivery = prepare_product_delivery(&job.run_id, &event, &mut binding);
    binding = save_product_binding(&state, binding, expected_version).map_err(|error| {
        crate::studio_http::StudioHttpError {
            code: error.code,
            message: error.message,
            retryable: true,
        }
    })?;
    let terminal = projected.iter().any(|item| item.terminal);
    attempt_product_delivery(&state, binding, delivery).await?;
    Ok(terminal)
}

pub(crate) fn product_projection_exists(
    events: &[ResponseEvent],
    event_type: &ResponseEventType,
    remote_event_id: &str,
) -> bool {
    events.iter().any(|existing| {
        &existing.event_type == event_type
            && existing
                .payload
                .get("remote_event_id")
                .and_then(Value::as_str)
                == Some(remote_event_id)
    })
}

pub(crate) fn product_binding_for_delivery(
    state: &AppState,
    response_id: &str,
) -> Result<ProductRunBinding, ResponseJobExecutionError> {
    state
        .product_bindings
        .lock()
        .map_err(|_| {
            ResponseJobExecutionError::new(
                "product_binding_store_unavailable",
                "product binding store is unavailable",
            )
        })?
        .get(response_id)
        .map_err(product_binding_execution_error)
}

pub(crate) fn binding_studio_error(
    error: ResponseJobExecutionError,
) -> crate::studio_http::StudioHttpError {
    crate::studio_http::StudioHttpError {
        code: error.code,
        message: error.message,
        retryable: true,
    }
}

pub(crate) fn save_product_binding(
    state: &AppState,
    binding: ProductRunBinding,
    expected_version: u64,
) -> Result<ProductRunBinding, ResponseJobExecutionError> {
    state
        .product_bindings
        .lock()
        .map_err(|_| {
            ResponseJobExecutionError::new(
                "product_binding_store_unavailable",
                "product binding store is unavailable",
            )
        })?
        .save(binding, expected_version)
        .map_err(product_binding_execution_error)
}

fn product_binding_status(status: &product_protocol::ProductRunStatus) -> ProductBindingStatus {
    match status {
        product_protocol::ProductRunStatus::Queued => ProductBindingStatus::Queued,
        product_protocol::ProductRunStatus::Running => ProductBindingStatus::Running,
        product_protocol::ProductRunStatus::RequiresAction => ProductBindingStatus::RequiresAction,
        product_protocol::ProductRunStatus::Completed => ProductBindingStatus::Completed,
        product_protocol::ProductRunStatus::Failed => ProductBindingStatus::Failed,
        product_protocol::ProductRunStatus::Canceling => ProductBindingStatus::Canceling,
        product_protocol::ProductRunStatus::Canceled => ProductBindingStatus::Canceled,
    }
}

pub(crate) fn product_binding_execution_error(
    error: ProductBindingStoreError,
) -> ResponseJobExecutionError {
    let code = match error {
        ProductBindingStoreError::BackendUnavailable(_) => "product_binding_store_unavailable",
        ProductBindingStoreError::CorruptBinding(_) => "product_binding_corrupt",
        ProductBindingStoreError::UnknownResponse(_) => "product_binding_missing",
        ProductBindingStoreError::BindingConflict(_) => "product_binding_conflict",
        ProductBindingStoreError::ActiveChatConflict(_) => "product_run_busy",
        ProductBindingStoreError::SequenceRegression { .. } => "product_sequence_regression",
    };
    ResponseJobExecutionError::new(code, "Product Run binding update failed")
}

fn studio_execution_error(error: crate::studio_http::StudioHttpError) -> ResponseJobExecutionError {
    ResponseJobExecutionError::new(error.code, error.message)
}
