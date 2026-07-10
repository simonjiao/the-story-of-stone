use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::auth::{gateway_auth_and_rate_limit, header_value};
use crate::{
    AppState, error_response, product_binding_for_response, response_id_for_run,
    response_owned_by_request, response_state_for_id,
};

pub(crate) const PRODUCT_HANDOFF_SCHEMA_VERSION: &str = "tonglingyu.product_handoff.v1";
pub(crate) const PRODUCT_HANDOFF_TTL_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProductHandoffRecord {
    pub(crate) schema_version: String,
    pub(crate) user_ref: String,
    pub(crate) run_id: String,
    pub(crate) product_id: String,
    pub(crate) artifact_id: String,
    pub(crate) artifact_path: String,
    pub(crate) issued_at: i64,
    pub(crate) expires_at: i64,
}

impl ProductHandoffRecord {
    pub(crate) fn new(
        user_ref: impl Into<String>,
        run_id: impl Into<String>,
        product_id: impl Into<String>,
        artifact_id: impl Into<String>,
        ttl_secs: u64,
    ) -> Self {
        let issued_at = OffsetDateTime::now_utc().unix_timestamp();
        let artifact_id = artifact_id.into();
        Self {
            schema_version: PRODUCT_HANDOFF_SCHEMA_VERSION.to_string(),
            user_ref: user_ref.into(),
            run_id: run_id.into(),
            product_id: product_id.into(),
            artifact_path: format!("/articles/{artifact_id}"),
            artifact_id,
            issued_at,
            expires_at: issued_at + ttl_secs.max(1) as i64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProductHandoffStoreError {
    BackendUnavailable(String),
    CorruptRecord(String),
    UnknownOrConsumed,
    Expired,
}

#[derive(Debug, Default)]
pub(crate) struct InMemoryProductHandoffStore {
    records: BTreeMap<String, ProductHandoffRecord>,
}

#[derive(Debug)]
pub(crate) struct RedisProductHandoffStore {
    client: redis::Client,
    prefix: String,
}

#[derive(Debug)]
pub(crate) enum ProductHandoffStoreBackend {
    InMemory(InMemoryProductHandoffStore),
    Redis(RedisProductHandoffStore),
}

#[derive(Debug, Serialize)]
pub(crate) struct ProductHandoffOpenResponse {
    pub(crate) url: String,
    pub(crate) expires_in: u64,
}

pub(crate) async fn open_product_artifact_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((run_id, artifact_id)): AxumPath<(String, String)>,
) -> Response {
    let trace_id = format!("tr_{}", uuid::Uuid::now_v7().simple());
    let auth_subject = match gateway_auth_and_rate_limit(&state, &headers, Some(&trace_id)) {
        Ok(subject) => subject,
        Err(response) => return *response,
    };
    let response_id = match response_id_for_run(&state, &run_id) {
        Ok(response_id) => response_id,
        Err(response) => return response,
    };
    let response = match response_state_for_id(&state, &response_id) {
        Ok(response) => response,
        Err(response) => return response,
    };
    if !response_owned_by_request(&response, &headers, &auth_subject) {
        return error_response(
            StatusCode::NOT_FOUND,
            "run_not_found",
            "run not found",
            Some(&trace_id),
        );
    }
    let binding = match product_binding_for_response(&state, &response_id) {
        Ok(Some(binding)) => binding,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "artifact_not_found",
                "product artifact not found",
                Some(&trace_id),
            );
        }
        Err(response) => return response,
    };
    if !binding
        .artifacts
        .iter()
        .any(|artifact| artifact.id == artifact_id)
    {
        return error_response(
            StatusCode::NOT_FOUND,
            "artifact_not_found",
            "product artifact not found",
            Some(&trace_id),
        );
    }
    let Some(public_base_url) = state.studio_public_base_url.as_deref() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "studio_public_url_unavailable",
            "Studio public URL is not configured",
            Some(&trace_id),
        );
    };
    let user_ref = response
        .user_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&response.subject)
        .to_string();
    let record = ProductHandoffRecord::new(
        user_ref,
        &run_id,
        &binding.product_id,
        &artifact_id,
        PRODUCT_HANDOFF_TTL_SECS,
    );
    let code = {
        let mut store = match state.product_handoffs.lock() {
            Ok(store) => store,
            Err(_) => {
                return error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "product_handoff_store_unavailable",
                    "product handoff store is unavailable",
                    Some(&trace_id),
                );
            }
        };
        match store.issue(record) {
            Ok(code) => code,
            Err(_) => {
                return error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "product_handoff_store_unavailable",
                    "product handoff could not be issued",
                    Some(&trace_id),
                );
            }
        }
    };
    Json(ProductHandoffOpenResponse {
        url: format!(
            "{}/api/auth/handoff?code={code}",
            public_base_url.trim_end_matches('/')
        ),
        expires_in: PRODUCT_HANDOFF_TTL_SECS,
    })
    .into_response()
}

pub(crate) async fn consume_product_handoff_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(code): AxumPath<String>,
) -> Response {
    if !studio_service_authorized(&state, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "studio_service_unauthorized",
            "missing or invalid Studio service credential",
            None,
        );
    }
    let result = state
        .product_handoffs
        .lock()
        .map_err(|_| ProductHandoffStoreError::BackendUnavailable("lock poisoned".to_string()))
        .and_then(|mut store| store.consume(&code));
    match result {
        Ok(record) => Json(record).into_response(),
        Err(ProductHandoffStoreError::UnknownOrConsumed | ProductHandoffStoreError::Expired) => {
            error_response(
                StatusCode::GONE,
                "product_handoff_invalid",
                "product handoff is expired or already consumed",
                None,
            )
        }
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "product_handoff_store_unavailable",
            "product handoff store is unavailable",
            None,
        ),
    }
}

fn studio_service_authorized(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.studio_service_key.as_deref() else {
        return false;
    };
    let bearer = header_value(headers, "authorization")
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_string));
    bearer.as_deref() == Some(expected)
        || header_value(headers, "x-api-key").as_deref() == Some(expected)
}

impl ProductHandoffStoreBackend {
    pub(crate) fn from_config(
        redis_url: Option<&str>,
        prefix: &str,
    ) -> Result<Self, ProductHandoffStoreError> {
        let redis_url = redis_url.unwrap_or_default().trim();
        if redis_url.is_empty() {
            return Ok(Self::InMemory(InMemoryProductHandoffStore::default()));
        }
        let client = redis::Client::open(redis_url).map_err(backend_error)?;
        let store = RedisProductHandoffStore {
            client,
            prefix: sanitize_prefix(prefix),
        };
        store.ping()?;
        Ok(Self::Redis(store))
    }

    pub(crate) fn issue(
        &mut self,
        record: ProductHandoffRecord,
    ) -> Result<String, ProductHandoffStoreError> {
        match self {
            Self::InMemory(store) => store.issue(record),
            Self::Redis(store) => store.issue(record),
        }
    }

    pub(crate) fn consume(
        &mut self,
        code: &str,
    ) -> Result<ProductHandoffRecord, ProductHandoffStoreError> {
        match self {
            Self::InMemory(store) => store.consume(code),
            Self::Redis(store) => store.consume(code),
        }
    }
}

impl InMemoryProductHandoffStore {
    fn issue(&mut self, record: ProductHandoffRecord) -> Result<String, ProductHandoffStoreError> {
        let code = new_code();
        self.records.insert(code_digest(&code), record);
        Ok(code)
    }

    fn consume(&mut self, code: &str) -> Result<ProductHandoffRecord, ProductHandoffStoreError> {
        let record = self
            .records
            .remove(&code_digest(code))
            .ok_or(ProductHandoffStoreError::UnknownOrConsumed)?;
        validate_expiry(record)
    }
}

impl RedisProductHandoffStore {
    fn issue(&self, record: ProductHandoffRecord) -> Result<String, ProductHandoffStoreError> {
        let code = new_code();
        let ttl = (record.expires_at - record.issued_at).max(1) as u64;
        let value = serde_json::to_string(&record).map_err(corrupt_error)?;
        let mut connection = self.connection()?;
        let inserted: Option<String> = redis::cmd("SET")
            .arg(self.key(&code))
            .arg(value)
            .arg("NX")
            .arg("EX")
            .arg(ttl)
            .query(&mut connection)
            .map_err(backend_error)?;
        if inserted.is_some() {
            Ok(code)
        } else {
            Err(ProductHandoffStoreError::BackendUnavailable(
                "handoff code collision".to_string(),
            ))
        }
    }

    fn consume(&self, code: &str) -> Result<ProductHandoffRecord, ProductHandoffStoreError> {
        let mut connection = self.connection()?;
        let value: Option<String> = redis::cmd("GETDEL")
            .arg(self.key(code))
            .query(&mut connection)
            .map_err(backend_error)?;
        let record = value
            .as_deref()
            .map(parse_record)
            .transpose()?
            .ok_or(ProductHandoffStoreError::UnknownOrConsumed)?;
        validate_expiry(record)
    }

    fn ping(&self) -> Result<(), ProductHandoffStoreError> {
        let mut connection = self.connection()?;
        let pong: String = redis::cmd("PING")
            .query(&mut connection)
            .map_err(backend_error)?;
        if pong.eq_ignore_ascii_case("PONG") {
            Ok(())
        } else {
            Err(ProductHandoffStoreError::BackendUnavailable(
                "Redis ping failed".to_string(),
            ))
        }
    }

    fn connection(&self) -> Result<redis::Connection, ProductHandoffStoreError> {
        self.client.get_connection().map_err(backend_error)
    }

    fn key(&self, code: &str) -> String {
        format!("{}:product-handoff:{}", self.prefix, code_digest(code))
    }
}

fn validate_expiry(
    record: ProductHandoffRecord,
) -> Result<ProductHandoffRecord, ProductHandoffStoreError> {
    if record.expires_at <= OffsetDateTime::now_utc().unix_timestamp() {
        Err(ProductHandoffStoreError::Expired)
    } else {
        Ok(record)
    }
}

fn new_code() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}
fn code_digest(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}
fn parse_record(value: &str) -> Result<ProductHandoffRecord, ProductHandoffStoreError> {
    serde_json::from_str(value).map_err(corrupt_error)
}
fn backend_error(error: redis::RedisError) -> ProductHandoffStoreError {
    ProductHandoffStoreError::BackendUnavailable(error.to_string())
}
fn corrupt_error(error: serde_json::Error) -> ProductHandoffStoreError {
    ProductHandoffStoreError::CorruptRecord(error.to_string())
}
fn sanitize_prefix(value: &str) -> String {
    if value.trim().is_empty() {
        "tonglingyu".to_string()
    } else {
        value.trim().to_string()
    }
}

#[cfg(test)]
mod tests;
