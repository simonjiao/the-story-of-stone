#![allow(dead_code)]

use std::time::Duration;

use reqwest::{Client, StatusCode, Url, header};
use serde_json::{Value, json};

use crate::product_protocol::{ProductRunEvent, ProductRunEventType};

#[derive(Debug, Clone)]
pub(crate) struct OpenWebuiDeliveryClient {
    base_url: Url,
    service_key: String,
    client: Client,
    max_attempts: u32,
    retry_base_delay_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenWebuiDeliveryError {
    pub(crate) code: &'static str,
    pub(crate) retryable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpenWebuiDelivery {
    pub(crate) body: Value,
    pub(crate) snapshot: String,
}

impl OpenWebuiDeliveryClient {
    pub(crate) fn new(
        base_url: &str,
        service_key: &str,
        timeout_secs: u64,
        max_attempts: u32,
        retry_base_delay_ms: u64,
    ) -> Result<Self, OpenWebuiDeliveryError> {
        if service_key.trim().is_empty() {
            return Err(OpenWebuiDeliveryError {
                code: "openwebui_delivery_not_configured",
                retryable: false,
            });
        }
        let base_url = Url::parse(base_url.trim()).map_err(|_| OpenWebuiDeliveryError {
            code: "openwebui_delivery_url_invalid",
            retryable: false,
        })?;
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs.max(1)))
            .build()
            .map_err(|_| OpenWebuiDeliveryError {
                code: "openwebui_delivery_client_invalid",
                retryable: false,
            })?;
        Ok(Self {
            base_url,
            service_key: service_key.to_string(),
            client,
            max_attempts: max_attempts.max(1),
            retry_base_delay_ms,
        })
    }

    pub(crate) async fn deliver<F>(
        &self,
        chat_id: &str,
        message_id: &str,
        delivery: &OpenWebuiDelivery,
        mut on_retry: F,
    ) -> Result<u32, OpenWebuiDeliveryError>
    where
        F: FnMut(u32, &OpenWebuiDeliveryError) -> Result<(), OpenWebuiDeliveryError>,
    {
        let url = self.event_url(chat_id, message_id)?;
        let mut last_error = OpenWebuiDeliveryError {
            code: "openwebui_delivery_failed",
            retryable: true,
        };
        for attempt in 1..=self.max_attempts {
            match self
                .client
                .post(url.clone())
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", self.service_key),
                )
                .json(&delivery.body)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(attempt),
                Ok(response) => {
                    last_error = status_error(response.status());
                    if !last_error.retryable {
                        return Err(last_error);
                    }
                }
                Err(error) => {
                    last_error = OpenWebuiDeliveryError {
                        code: "openwebui_delivery_unavailable",
                        retryable: error.is_timeout() || error.is_connect() || error.is_request(),
                    };
                    if !last_error.retryable {
                        return Err(last_error);
                    }
                }
            }
            if attempt < self.max_attempts {
                on_retry(attempt, &last_error)?;
                let multiplier = 1_u64 << (attempt - 1).min(8);
                tokio::time::sleep(Duration::from_millis(
                    self.retry_base_delay_ms.saturating_mul(multiplier),
                ))
                .await;
            }
        }
        Err(last_error)
    }

    fn event_url(&self, chat_id: &str, message_id: &str) -> Result<Url, OpenWebuiDeliveryError> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| OpenWebuiDeliveryError {
                    code: "openwebui_delivery_url_invalid",
                    retryable: false,
                })?;
            segments.pop_if_empty();
            segments.extend([
                "api", "v1", "chats", chat_id, "messages", message_id, "event",
            ]);
        }
        Ok(url)
    }
}

pub(crate) fn delivery_for_product_event(
    gateway_run_id: &str,
    event: &ProductRunEvent,
) -> OpenWebuiDelivery {
    let message = event
        .payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("写作任务正在处理。");
    match event.event_type {
        ProductRunEventType::RunStarted
        | ProductRunEventType::RunStatus
        | ProductRunEventType::ArtifactUpdated
        | ProductRunEventType::RunResumed => OpenWebuiDelivery {
            body: json!({"type": "status", "data": {"description": message, "done": false}}),
            snapshot: message.to_string(),
        },
        ProductRunEventType::RunRequiresAction => {
            let action = event
                .payload
                .get("action")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let action_id = action
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let title = action
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("需要确认");
            let content = format!(
                "写作任务等待确认\n\n{title}\n\nRun ID: {gateway_run_id}\nAction ID: {action_id}"
            );
            OpenWebuiDelivery {
                body: json!({"type": "replace", "data": {"content": &content}}),
                snapshot: content,
            }
        }
        ProductRunEventType::RunCompleted => {
            let summary = event
                .payload
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("写作任务已完成。");
            let artifact_lines = event
                .payload
                .get("artifacts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|artifact| {
                    let id = artifact.get("id")?.as_str()?;
                    let title = artifact
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("产物");
                    Some(format!("- {title} (`{id}`)"))
                })
                .collect::<Vec<_>>();
            let content = if artifact_lines.is_empty() {
                summary.to_string()
            } else {
                format!(
                    "{summary}\n\n{}\n\nRun ID: {gateway_run_id}",
                    artifact_lines.join("\n")
                )
            };
            OpenWebuiDelivery {
                body: json!({"type": "replace", "data": {"content": &content}}),
                snapshot: content,
            }
        }
        ProductRunEventType::RunFailed => {
            let content = format!("写作任务失败。\n\nRun ID: {gateway_run_id}");
            OpenWebuiDelivery {
                body: json!({"type": "replace", "data": {"content": &content}}),
                snapshot: content,
            }
        }
        ProductRunEventType::RunCanceled => {
            let content = format!("写作任务已取消。\n\nRun ID: {gateway_run_id}");
            OpenWebuiDelivery {
                body: json!({"type": "replace", "data": {"content": &content}}),
                snapshot: content,
            }
        }
    }
}

fn status_error(status: StatusCode) -> OpenWebuiDeliveryError {
    OpenWebuiDeliveryError {
        code: if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            "openwebui_delivery_unauthorized"
        } else if status == StatusCode::NOT_FOUND {
            "openwebui_delivery_target_not_found"
        } else {
            "openwebui_delivery_http_error"
        },
        retryable: status.is_server_error()
            || status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::NOT_FOUND,
    }
}

#[cfg(test)]
mod tests;
