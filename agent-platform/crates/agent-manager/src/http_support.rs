use agent_core::{
    AgentCoreError, AuthContext, ErrorCode, ExternalActionMode, PolicyContext, PolicyDecision,
    RiskLevel, RoleAssignment, RoleName, SafeError, new_trace_id,
};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ManagerConfig {
    pub jwt_secret: Option<SecretString>,
    pub allow_dev_headers: bool,
    pub default_service_actions: Vec<String>,
}

impl ManagerConfig {
    pub fn from_env() -> Self {
        Self {
            jwt_secret: std::env::var("AGENT_JWT_SECRET")
                .ok()
                .filter(|value| !value.is_empty())
                .map(SecretString::from),
            allow_dev_headers: std::env::var("AGENT_ALLOW_DEV_HEADERS")
                .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
                .unwrap_or(true),
            default_service_actions: vec![
                "request:*".to_string(),
                "session:*".to_string(),
                "run:*".to_string(),
                "admin:*".to_string(),
                "internal:*".to_string(),
            ],
        }
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: SafeError,
}

impl ApiError {
    pub(crate) fn from_core(error: AgentCoreError, trace_id: impl Into<String>) -> Self {
        let code = error.code();
        let status = match code {
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::ApprovalRequired => StatusCode::ACCEPTED,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            body: SafeError::new(code, trace_id),
        }
    }

    fn unauthorized(trace_id: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: SafeError::new(ErrorCode::Unauthorized, trace_id),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceJwtClaims {
    sub: String,
    service_name: Option<String>,
    allowed_actions: Vec<String>,
    exp: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserJwtClaims {
    sub: String,
    roles: Vec<String>,
    resource_allowlist: Vec<String>,
    exp: usize,
}

pub fn extract_auth(headers: &HeaderMap, config: &ManagerConfig) -> Result<AuthContext, ApiError> {
    let trace_id = header_value(headers, "x-agent-trace-id").unwrap_or_else(new_trace_id);

    if let Some(secret) = &config.jwt_secret {
        let service_token = header_value(headers, "authorization")
            .and_then(|value| bearer(&value).map(ToString::to_string))
            .ok_or_else(|| ApiError::unauthorized(trace_id.clone()))?;
        let user_token = header_value(headers, "x-agent-user-token")
            .and_then(|value| bearer(&value).map(ToString::to_string).or(Some(value)))
            .ok_or_else(|| ApiError::unauthorized(trace_id.clone()))?;
        let service = decode::<ServiceJwtClaims>(
            &service_token,
            &DecodingKey::from_secret(secret.expose_secret().as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| ApiError::unauthorized(trace_id.clone()))?
        .claims;
        let user = decode::<UserJwtClaims>(
            &user_token,
            &DecodingKey::from_secret(secret.expose_secret().as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| ApiError::unauthorized(trace_id.clone()))?
        .claims;
        return Ok(AuthContext {
            user_id: user.sub,
            service_id: service.sub,
            service_allowed_actions: service.allowed_actions,
            roles: user
                .roles
                .into_iter()
                .filter_map(|role| role.parse::<RoleName>().ok())
                .map(RoleAssignment::global)
                .collect(),
            resource_allowlist: user.resource_allowlist,
            trace_id,
        });
    }

    if config.allow_dev_headers {
        let service_id = header_value(headers, "x-agent-service")
            .unwrap_or_else(|| "dev-orchestrator".to_string());
        let user_id =
            header_value(headers, "x-agent-user").unwrap_or_else(|| "dev-user".to_string());
        let roles = parse_roles(
            &header_value(headers, "x-agent-roles").unwrap_or_else(|| "system_admin".to_string()),
        );
        let allowed = parse_csv(header_value(headers, "x-agent-allowed-actions"));
        return Ok(AuthContext {
            user_id,
            service_id,
            service_allowed_actions: if allowed.is_empty() {
                config.default_service_actions.clone()
            } else {
                allowed
            },
            roles,
            resource_allowlist: parse_csv(header_value(headers, "x-agent-resource-allowlist"))
                .into_iter()
                .chain(std::iter::once("*".to_string()))
                .collect(),
            trace_id,
        });
    }

    Err(ApiError::unauthorized(trace_id))
}

pub(crate) fn ensure_admin(auth: &AuthContext, action: &str) -> Result<(), ApiError> {
    let ctx = PolicyContext {
        action: action.to_string(),
        request_type: None,
        agent_type: None,
        resource: None,
        risk_level: RiskLevel::Low,
        external_action_mode: ExternalActionMode::Deny,
        resource_attributes: Value::Null,
        observer_mode: false,
    };
    match agent_core::DefaultPolicy::authorize(auth, &ctx) {
        PolicyDecision::Allowed => Ok(()),
        PolicyDecision::Denied { reason } | PolicyDecision::ApprovalRequired { reason } => {
            Err(ApiError::from_core(
                AgentCoreError::coded(ErrorCode::Forbidden, reason),
                auth.trace_id.clone(),
            ))
        }
    }
}

pub(crate) fn ensure_operator_or_admin(auth: &AuthContext, action: &str) -> Result<(), ApiError> {
    let ctx = PolicyContext {
        action: action.to_string(),
        request_type: None,
        agent_type: None,
        resource: None,
        risk_level: RiskLevel::Low,
        external_action_mode: ExternalActionMode::Deny,
        resource_attributes: Value::Null,
        observer_mode: false,
    };
    match agent_core::DefaultPolicy::authorize(auth, &ctx) {
        PolicyDecision::Allowed => Ok(()),
        PolicyDecision::Denied { reason } | PolicyDecision::ApprovalRequired { reason } => {
            Err(ApiError::from_core(
                AgentCoreError::coded(ErrorCode::Forbidden, reason),
                auth.trace_id.clone(),
            ))
        }
    }
}

pub(crate) fn ensure_service_allows(auth: &AuthContext, action: &str) -> Result<(), ApiError> {
    if auth.service_allows(action) {
        Ok(())
    } else {
        Err(ApiError::from_core(
            AgentCoreError::coded(
                ErrorCode::Forbidden,
                "internal action requires service claim",
            ),
            auth.trace_id.clone(),
        ))
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn parse_roles(value: &str) -> Vec<RoleAssignment> {
    value
        .split(',')
        .filter_map(|role| role.trim().parse::<RoleName>().ok())
        .map(RoleAssignment::global)
        .collect()
}

fn parse_csv(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn bearer(value: &str) -> Option<&str> {
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}
