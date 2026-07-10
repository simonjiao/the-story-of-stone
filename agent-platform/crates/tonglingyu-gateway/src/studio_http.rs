use std::future::Future;
use std::time::Duration;

use crate::product_protocol::{
    GatewayCapabilities, ProductRunActionSubmission, ProductRunCreateRequest, ProductRunEvent,
    ProductRunRecord, validate_capabilities, validate_create_request, validate_event,
};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header};

#[derive(Debug, Clone)]
pub(crate) struct StudioHttpClient {
    base_url: String,
    service_key: String,
    client: Client,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioHttpError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

impl StudioHttpClient {
    pub(crate) fn new(
        base_url: &str,
        service_key: &str,
        timeout_secs: u64,
    ) -> Result<Self, StudioHttpError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() || service_key.trim().is_empty() {
            return Err(StudioHttpError::configuration(
                "Studio base URL and service key are required",
            ));
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(timeout_secs.max(1)))
            .build()
            .map_err(|error| StudioHttpError::configuration(error.to_string()))?;
        Ok(Self {
            base_url,
            service_key: service_key.to_string(),
            client,
        })
    }

    pub(crate) async fn capabilities(&self) -> Result<GatewayCapabilities, StudioHttpError> {
        let response = self
            .authorized(
                self.client
                    .get(format!("{}/internal/gateway/capabilities", self.base_url)),
            )
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(StudioHttpError::transport)?;
        let capabilities: GatewayCapabilities = self.decode(response).await?;
        validate_capabilities(capabilities).map_err(StudioHttpError::contract)
    }

    pub(crate) async fn create_run(
        &self,
        request: &ProductRunCreateRequest,
    ) -> Result<ProductRunRecord, StudioHttpError> {
        validate_create_request(request).map_err(StudioHttpError::contract)?;
        let response = self
            .authorized(self.client.post(format!(
                "{}/internal/products/{}/runs",
                self.base_url, request.product_id
            )))
            .json(request)
            .send()
            .await
            .map_err(StudioHttpError::transport)?;
        self.decode(response).await
    }

    pub(crate) async fn stream_events<F, Fut>(
        &self,
        run_id: &str,
        after_sequence: u64,
        mut on_event: F,
    ) -> Result<(), StudioHttpError>
    where
        F: FnMut(ProductRunEvent) -> Fut,
        Fut: Future<Output = Result<bool, StudioHttpError>>,
    {
        let response = self
            .authorized(
                self.client
                    .get(format!("{}/internal/runs/{run_id}/events", self.base_url)),
            )
            .query(&[("after_sequence", after_sequence)])
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(StudioHttpError::transport)?;
        if !response.status().is_success() {
            return Err(Self::status_error(response.status()));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut previous = after_sequence;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(StudioHttpError::transport)?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(boundary) = buffer.find("\n\n") {
                let frame = buffer[..boundary].to_string();
                buffer.drain(..boundary + 2);
                if matches!(sse_event_name(&frame), Some("connected" | "ping")) {
                    continue;
                }
                let Some(data) = sse_data(&frame) else {
                    continue;
                };
                if data == "[DONE]" {
                    return Ok(());
                }
                let event: ProductRunEvent = serde_json::from_str(&data)
                    .map_err(|error| StudioHttpError::contract(error.to_string()))?;
                validate_event(&event, previous).map_err(StudioHttpError::contract)?;
                previous = event.sequence;
                if on_event(event).await? {
                    return Ok(());
                }
            }
        }
        Err(StudioHttpError {
            code: "studio_stream_disconnected",
            message: "Studio event stream disconnected before a terminal event".to_string(),
            retryable: true,
        })
    }

    pub(crate) async fn submit_action(
        &self,
        run_id: &str,
        action_id: &str,
        submission: &ProductRunActionSubmission,
    ) -> Result<ProductRunRecord, StudioHttpError> {
        let response = self
            .authorized(self.client.post(format!(
                "{}/internal/runs/{run_id}/actions/{action_id}",
                self.base_url
            )))
            .json(submission)
            .send()
            .await
            .map_err(StudioHttpError::transport)?;
        self.decode(response).await
    }

    pub(crate) async fn cancel(&self, run_id: &str) -> Result<ProductRunRecord, StudioHttpError> {
        let response = self
            .authorized(
                self.client
                    .post(format!("{}/internal/runs/{run_id}/cancel", self.base_url)),
            )
            .send()
            .await
            .map_err(StudioHttpError::transport)?;
        self.decode(response).await
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.header(
            header::AUTHORIZATION,
            format!("Bearer {}", self.service_key),
        )
    }

    async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, StudioHttpError> {
        let status = response.status();
        if !status.is_success() {
            return Err(Self::status_error(status));
        }
        response
            .json()
            .await
            .map_err(|error| StudioHttpError::contract(error.to_string()))
    }

    fn status_error(status: StatusCode) -> StudioHttpError {
        StudioHttpError {
            code: if status == StatusCode::CONFLICT {
                "studio_conflict"
            } else {
                "studio_http_error"
            },
            message: format!("Studio returned HTTP {}", status.as_u16()),
            retryable: status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS,
        }
    }
}

impl StudioHttpError {
    fn configuration(message: impl Into<String>) -> Self {
        Self {
            code: "studio_configuration_invalid",
            message: message.into(),
            retryable: false,
        }
    }

    fn contract(message: impl Into<String>) -> Self {
        Self {
            code: "studio_contract_invalid",
            message: message.into(),
            retryable: false,
        }
    }

    fn transport(error: reqwest::Error) -> Self {
        Self {
            code: "studio_unavailable",
            message: "Studio request failed".to_string(),
            retryable: error.is_timeout() || error.is_connect() || error.is_request(),
        }
    }
}

fn sse_data(frame: &str) -> Option<String> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>();
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

fn sse_event_name(frame: &str) -> Option<&str> {
    frame
        .lines()
        .find_map(|line| line.strip_prefix("event:").map(str::trim))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
