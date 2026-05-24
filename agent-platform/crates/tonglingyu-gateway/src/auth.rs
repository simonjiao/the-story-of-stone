use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{AppState, append_admin_audit_event, error_response, hash_text};

pub(crate) type AuthResult<T> = std::result::Result<T, Box<Response>>;

#[derive(Debug)]
pub(crate) struct GatewayRateLimiter {
    max_per_window: usize,
    window: Duration,
    buckets: Mutex<BTreeMap<String, RateLimitBucket>>,
}

#[derive(Debug, Clone, Copy)]
struct RateLimitBucket {
    window_start: Instant,
    count: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RateLimitDecision {
    pub(crate) allowed: bool,
    pub(crate) limit: usize,
    pub(crate) remaining: usize,
    pub(crate) retry_after_secs: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct PackageAccessContext {
    pub(crate) subject: String,
    pub(crate) user_ref: String,
}

impl GatewayRateLimiter {
    pub(crate) fn per_minute(max_per_minute: usize) -> Self {
        Self::new(max_per_minute, Duration::from_secs(60))
    }

    pub(crate) fn new(max_per_window: usize, window: Duration) -> Self {
        Self {
            max_per_window,
            window,
            buckets: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn check(&self, subject: &str) -> RateLimitDecision {
        if self.max_per_window == 0 {
            return RateLimitDecision {
                allowed: true,
                limit: 0,
                remaining: usize::MAX,
                retry_after_secs: 0,
            };
        }
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .expect("gateway rate limiter mutex poisoned");
        buckets.retain(|_, bucket| now.duration_since(bucket.window_start) < self.window);
        let bucket = buckets
            .entry(subject.to_string())
            .or_insert(RateLimitBucket {
                window_start: now,
                count: 0,
            });
        if now.duration_since(bucket.window_start) >= self.window {
            bucket.window_start = now;
            bucket.count = 0;
        }
        if bucket.count >= self.max_per_window {
            let elapsed = now.duration_since(bucket.window_start);
            let retry_after = self.window.saturating_sub(elapsed).as_secs().max(1);
            return RateLimitDecision {
                allowed: false,
                limit: self.max_per_window,
                remaining: 0,
                retry_after_secs: retry_after,
            };
        }
        bucket.count += 1;
        RateLimitDecision {
            allowed: true,
            limit: self.max_per_window,
            remaining: self.max_per_window.saturating_sub(bucket.count),
            retry_after_secs: 0,
        }
    }
}

fn gateway_auth_subject(state: &AppState, headers: &HeaderMap) -> AuthResult<String> {
    authorize_with_keys(
        headers,
        &state.gateway_api_keys,
        "gateway_unauthorized",
        false,
    )
}

pub(crate) fn gateway_auth_and_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    trace_id: Option<&str>,
) -> AuthResult<String> {
    let subject = gateway_auth_subject(state, headers)?;
    let decision = state.rate_limiter.check(&subject);
    if decision.allowed {
        Ok(subject)
    } else {
        Err(Box::new(rate_limit_response(&decision, trace_id)))
    }
}

pub(crate) fn admin_auth_and_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    action: &str,
) -> AuthResult<String> {
    let request_subject = request_subject(headers);
    let subject = match admin_auth_subject(state, headers) {
        Ok(subject) => subject,
        Err(response) => {
            let subject_ref = audit_subject_ref(&request_subject);
            let _ = append_admin_audit_event(
                &state.db,
                "rqa_admin_access_denied",
                &subject_ref,
                json!({
                    "action": action,
                    "denial": "auth_failed",
                    "subject_ref": subject_ref,
                }),
            );
            return Err(response);
        }
    };
    let decision = state.admin_rate_limiter.check(&subject);
    if decision.allowed {
        Ok(subject)
    } else {
        let subject_ref = audit_subject_ref(&subject);
        let _ = append_admin_audit_event(
            &state.db,
            "rqa_admin_access_denied",
            &subject_ref,
            json!({
                "action": action,
                "denial": "rate_limited",
                "subject_ref": subject_ref,
                "limit_per_minute": decision.limit,
                "retry_after_secs": decision.retry_after_secs,
            }),
        );
        Err(Box::new(rate_limit_response(&decision, None)))
    }
}

fn admin_auth_subject(state: &AppState, headers: &HeaderMap) -> AuthResult<String> {
    let keys = if state.admin_api_keys.is_empty() && state.allow_admin_with_gateway_key {
        &state.gateway_api_keys
    } else {
        &state.admin_api_keys
    };
    authorize_with_keys(headers, keys, "admin_unauthorized", true)
}

fn authorize_with_keys(
    headers: &HeaderMap,
    expected_keys: &[String],
    code: &str,
    require_configured_key: bool,
) -> AuthResult<String> {
    let subject = request_subject(headers);
    if expected_keys.is_empty() && !require_configured_key {
        return Ok(subject);
    }
    if expected_keys.is_empty() {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            code,
            "admin credential is not configured",
            None,
        )));
    }
    let bearer = bearer_token(headers);
    let api_key = header_value(headers, "x-api-key");
    if bearer
        .as_deref()
        .is_some_and(|token| expected_keys.iter().any(|key| key == token))
        || api_key
            .as_deref()
            .is_some_and(|token| expected_keys.iter().any(|key| key == token))
    {
        Ok(subject)
    } else {
        Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            code,
            "missing or invalid gateway credential",
            None,
        )))
    }
}

fn request_subject(headers: &HeaderMap) -> String {
    header_value(headers, "x-tonglingyu-subject")
        .or_else(|| header_value(headers, "x-open-webui-user-id"))
        .unwrap_or_else(|| "open-webui".to_string())
}

pub(crate) fn audit_subject_ref(subject: &str) -> String {
    let digest = hash_text(subject);
    format!("sha256:{}", &digest[..16])
}

fn rate_limit_response(decision: &RateLimitDecision, trace_id: Option<&str>) -> Response {
    let _ = trace_id;
    let value = json!({
        "error": {
            "code": "gateway_rate_limited",
            "message": "gateway rate limit exceeded",
            "limit_per_minute": decision.limit,
            "remaining": decision.remaining,
            "retry_after_secs": decision.retry_after_secs,
        }
    });
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, decision.retry_after_secs.to_string())],
        Json(value),
    )
        .into_response()
}

pub(crate) fn configured_keys(primary: Option<String>, additional: Option<String>) -> Vec<String> {
    primary
        .into_iter()
        .chain(additional.into_iter().flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        }))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn validate_admin_key_isolation(
    gateway_api_keys: &[String],
    admin_api_keys: &[String],
    allow_admin_with_gateway_key: bool,
) -> Result<()> {
    let gateway_keys = gateway_api_keys
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if admin_api_keys
        .iter()
        .any(|key| gateway_keys.contains(key.as_str()))
    {
        return Err(anyhow!(
            "TONGLINGYU admin API keys must not overlap gateway API keys"
        ));
    }
    if allow_admin_with_gateway_key && !admin_api_keys.is_empty() {
        return Err(anyhow!(
            "TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY requires empty admin API key configuration"
        ));
    }
    Ok(())
}

pub(crate) fn is_admin_key_isolated(state: &AppState) -> bool {
    if state.admin_api_keys.is_empty() || state.allow_admin_with_gateway_key {
        return false;
    }
    let gateway_keys = state
        .gateway_api_keys
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    state
        .admin_api_keys
        .iter()
        .all(|key| !gateway_keys.contains(key.as_str()))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = header_value(headers, "authorization")?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(|token| token.trim().to_string())
}

pub(crate) fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn package_access_context(headers: &HeaderMap, subject: String) -> PackageAccessContext {
    let user_ref = header_value(headers, "x-tonglingyu-user-id")
        .or_else(|| header_value(headers, "x-open-webui-user-id"))
        .or_else(|| header_value(headers, "x-user-id"))
        .unwrap_or_else(|| subject.clone());
    PackageAccessContext { subject, user_ref }
}
