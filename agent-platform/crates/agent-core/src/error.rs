use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Forbidden,
    ApprovalRequired,
    NotFound,
    Conflict,
    RateLimited,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::ApprovalRequired => "approval_required",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::InternalError => "internal_error",
        }
    }

    pub fn safe_message(self) -> &'static str {
        match self {
            Self::Unauthorized => "未登录或用户身份无效。",
            Self::Forbidden => "你没有执行该操作的权限。",
            Self::ApprovalRequired => "该请求需要审批。",
            Self::NotFound => "资源不存在，或你无权访问。",
            Self::Conflict => "请求与当前状态冲突。",
            Self::RateLimited => "请求已达到限流。",
            Self::InternalError => "内部错误，请使用 trace_id 排查。",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeError {
    pub error: String,
    pub message: String,
    pub trace_id: String,
}

impl SafeError {
    pub fn new(code: ErrorCode, trace_id: impl Into<String>) -> Self {
        Self {
            error: code.as_str().to_string(),
            message: code.safe_message().to_string(),
            trace_id: trace_id.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum AgentCoreError {
    #[error("{code:?}: {message}")]
    Coded { code: ErrorCode, message: String },

    #[error("invalid transition for {entity}: {from} -> {to}")]
    InvalidTransition {
        entity: &'static str,
        from: String,
        to: String,
    },

    #[error("invalid enum value for {kind}: {value}")]
    InvalidEnum { kind: &'static str, value: String },

    #[error("invalid resource ref: {0}")]
    InvalidResourceRef(String),
}

impl AgentCoreError {
    pub fn coded(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Coded {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Coded { code, .. } => *code,
            Self::InvalidTransition { .. } => ErrorCode::Conflict,
            Self::InvalidEnum { .. } | Self::InvalidResourceRef(_) => ErrorCode::Conflict,
        }
    }
}

pub type CoreResult<T> = Result<T, AgentCoreError>;
