use serde_json::{Value, json};

use crate::product_binding::ProductBindingStatus;
use crate::product_protocol::{ProductRunEvent, ProductRunEventType};
use crate::response_events::{ResponseEventType, ResponseStatus};

#[derive(Debug, Clone)]
pub(crate) struct ProjectedProductEvent {
    pub(crate) event_type: ResponseEventType,
    pub(crate) payload: Value,
    pub(crate) status_update: Option<ResponseStatus>,
    pub(crate) binding_status: ProductBindingStatus,
    pub(crate) pending_action_id: Option<String>,
    pub(crate) terminal: bool,
}

pub(crate) fn project_product_event(
    event: &ProductRunEvent,
) -> Result<Vec<ProjectedProductEvent>, String> {
    let common = json!({
        "remote_event_id": event.event_id,
        "remote_sequence": event.sequence,
        "product_id": event.product_id,
        "remote_run_id": event.run_id,
    });
    let projected = match event.event_type {
        ProductRunEventType::RunStarted | ProductRunEventType::RunStatus => vec![projection(
            ResponseEventType::ResponseStatus,
            merge_payload(&event.payload, &common),
            Some(ResponseStatus::InProgress),
            ProductBindingStatus::Running,
            None,
            false,
        )],
        ProductRunEventType::ArtifactUpdated => vec![projection(
            ResponseEventType::ArtifactUpdated,
            merge_payload(&event.payload, &common),
            None,
            ProductBindingStatus::Running,
            None,
            false,
        )],
        ProductRunEventType::RunRequiresAction => {
            let action_id = event
                .payload
                .get("action")
                .and_then(|action| action.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "Studio requires_action event is missing action.id".to_string())?;
            let mut payload = merge_payload(&event.payload, &common);
            if let Some(payload) = payload.as_object_mut() {
                payload.insert("action_id".to_string(), json!(&action_id));
            }
            vec![projection(
                ResponseEventType::ResponseRequiresAction,
                payload,
                Some(ResponseStatus::RequiresAction),
                ProductBindingStatus::RequiresAction,
                Some(action_id),
                false,
            )]
        }
        ProductRunEventType::RunResumed => vec![projection(
            ResponseEventType::ResponseStatus,
            merge_payload(&event.payload, &common),
            Some(ResponseStatus::InProgress),
            ProductBindingStatus::Running,
            None,
            false,
        )],
        ProductRunEventType::RunCompleted => {
            let summary = event
                .payload
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("写作任务已完成。");
            vec![
                projection(
                    ResponseEventType::OutputTextDone,
                    json!({
                        "text": summary,
                        "artifacts": event.payload.get("artifacts"),
                        "remote_event_id": event.event_id,
                        "remote_sequence": event.sequence,
                    }),
                    None,
                    ProductBindingStatus::Running,
                    None,
                    false,
                ),
                projection(
                    ResponseEventType::ResponseCompleted,
                    merge_payload(&event.payload, &common),
                    Some(ResponseStatus::Completed),
                    ProductBindingStatus::Completed,
                    None,
                    true,
                ),
            ]
        }
        ProductRunEventType::RunFailed => vec![projection(
            ResponseEventType::ResponseFailed,
            merge_payload(&event.payload, &common),
            Some(ResponseStatus::Failed),
            ProductBindingStatus::Failed,
            None,
            true,
        )],
        ProductRunEventType::RunCanceled => vec![projection(
            ResponseEventType::ResponseCanceled,
            merge_payload(&event.payload, &common),
            Some(ResponseStatus::Canceled),
            ProductBindingStatus::Canceled,
            None,
            true,
        )],
    };
    Ok(projected)
}

fn projection(
    event_type: ResponseEventType,
    payload: Value,
    status_update: Option<ResponseStatus>,
    binding_status: ProductBindingStatus,
    pending_action_id: Option<String>,
    terminal: bool,
) -> ProjectedProductEvent {
    ProjectedProductEvent {
        event_type,
        payload,
        status_update,
        binding_status,
        pending_action_id,
        terminal,
    }
}

fn merge_payload(payload: &Value, common: &Value) -> Value {
    let mut merged = payload.as_object().cloned().unwrap_or_default();
    if let Some(common) = common.as_object() {
        for (key, value) in common {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

#[cfg(test)]
mod tests;
