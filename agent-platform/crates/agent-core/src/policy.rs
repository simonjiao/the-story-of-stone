use crate::{
    AGENT_TYPE_BACKGROUND_WORKER, AGENT_TYPE_OBSERVER, AgentCoreError, AuditDecision, AuthContext,
    CoreResult, ErrorCode, ExternalActionMode, RequestType, ResourceRef, RiskLevel, RoleName,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod actions {
    pub const REQUEST_CREATE_AGENT: &str = "request:create_agent";
    pub const REQUEST_CHANGE_AGENT: &str = "request:change_agent";
    pub const SESSION_CREATE: &str = "session:create";
    pub const SESSION_CREATE_CHILD: &str = "session:create_child";
    pub const SESSION_APPEND_MESSAGE: &str = "session:append_message";
    pub const RUN_CREATE: &str = "run:create";
    pub const ADMIN_APPROVE: &str = "admin:approve";
    pub const ADMIN_DENY: &str = "admin:deny";
    pub const ADMIN_AGENT_PAUSE: &str = "admin:agent_pause";
    pub const ADMIN_AGENT_RESUME: &str = "admin:agent_resume";
    pub const ADMIN_AUDIT_READ: &str = "admin:audit_read";
    pub const ADMIN_OBSERVER_READ: &str = "admin:observer_read";
    pub const ADMIN_OBSERVER_DISCUSS: &str = "admin:observer_discuss";
    pub const ADMIN_GRANT_CREATE: &str = "admin:grant_create";
    pub const ADMIN_RUN_READ: &str = "admin:run_read";
    pub const ADMIN_RUN_RETRY: &str = "admin:run_retry";
    pub const ADMIN_RUN_TERMINATE: &str = "admin:run_terminate";
    pub const ADMIN_EXTERNAL_ACTION_DRY_RUN: &str = "admin:external_action_dry_run";
    pub const ADMIN_EXTERNAL_ACTION_APPLY: &str = "admin:external_action_apply";
    pub const INTERNAL_RUN_CREATE: &str = "internal:run_create";
    pub const INTERNAL_RUN_CLAIM: &str = "internal:run_claim";
    pub const INTERNAL_RUN_HEARTBEAT: &str = "internal:run_heartbeat";
    pub const INTERNAL_RUN_FINISH: &str = "internal:run_finish";
    pub const INTERNAL_RUN_DEAD_LETTER: &str = "internal:run_dead_letter";
    pub const INTERNAL_SESSION_APPEND_MESSAGE: &str = "internal:session_append_message";
    pub const INTERNAL_SESSION_CONTEXT: &str = "internal:session_context";
    pub const INTERNAL_MEMORY_SUMMARY: &str = "internal:memory_summary";
    pub const INTERNAL_OBSERVER_TICK: &str = "internal:observer_tick";
    pub const INTERNAL_WEBHOOK: &str = "internal:webhook";
    pub const INTERNAL_OPEN_WEBUI_BRIDGE: &str = "internal:open_webui_bridge";
    pub const INTERNAL_OPEN_WEBUI_BRIDGE_NONCE: &str = "internal:open_webui_bridge:nonce";
    pub const INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_READ: &str =
        "internal:open_webui_bridge:binding_read";
    pub const INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_UPSERT: &str =
        "internal:open_webui_bridge:binding_upsert";
    pub const INTERNAL_OPEN_WEBUI_BRIDGE_BINDING_CLOSE: &str =
        "internal:open_webui_bridge:binding_close";
    pub const INTERNAL_OPEN_WEBUI_BRIDGE_RUN_UPDATE: &str = "internal:open_webui_bridge:run_update";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyContext {
    pub action: String,
    pub request_type: Option<RequestType>,
    pub agent_type: Option<String>,
    pub resource: Option<ResourceRef>,
    pub risk_level: RiskLevel,
    pub external_action_mode: ExternalActionMode,
    #[serde(default)]
    pub resource_attributes: Value,
    pub observer_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyDecision {
    Allowed,
    Denied { reason: String },
    ApprovalRequired { reason: String },
}

impl PolicyDecision {
    pub fn audit_decision(&self) -> AuditDecision {
        match self {
            Self::Allowed => AuditDecision::Allowed,
            Self::Denied { .. } => AuditDecision::Denied,
            Self::ApprovalRequired { .. } => AuditDecision::ApprovalRequired,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DefaultPolicy;

impl DefaultPolicy {
    pub fn authorize(auth: &AuthContext, ctx: &PolicyContext) -> PolicyDecision {
        if !auth.service_allows(&ctx.action) {
            return PolicyDecision::Denied {
                reason: format!("service {} cannot {}", auth.service_id, ctx.action),
            };
        }

        if auth.user_id.is_empty() || auth.service_id.is_empty() {
            return PolicyDecision::Denied {
                reason: "missing dual subject identity".to_string(),
            };
        }

        if ctx.observer_mode && !Self::observer_action_is_read_only(&ctx.action) {
            return PolicyDecision::Denied {
                reason: "observer_agent is read-only and cannot perform control actions"
                    .to_string(),
            };
        }

        if let Some(resource) = &ctx.resource
            && !auth.user_can_access_resource(resource)
        {
            return PolicyDecision::Denied {
                reason: "resource is outside user allowlist".to_string(),
            };
        }

        if matches!(ctx.risk_level, RiskLevel::Critical) {
            return PolicyDecision::Denied {
                reason: "critical risk action is denied by Agent Platform policy".to_string(),
            };
        }

        if matches!(ctx.risk_level, RiskLevel::High) {
            return PolicyDecision::ApprovalRequired {
                reason: "high risk action requires approval".to_string(),
            };
        }

        if matches!(ctx.external_action_mode, ExternalActionMode::Authorized) {
            return PolicyDecision::Denied {
                reason:
                    "Agent Platform read-only policy does not allow authorized external actions"
                        .to_string(),
            };
        }

        if matches!(
            ctx.external_action_mode,
            ExternalActionMode::ApprovalRequired
        ) {
            return PolicyDecision::ApprovalRequired {
                reason: "external actions require resource owner approval".to_string(),
            };
        }

        if let Some(agent_type) = &ctx.agent_type
            && agent_type != AGENT_TYPE_BACKGROUND_WORKER
            && agent_type != AGENT_TYPE_OBSERVER
        {
            return PolicyDecision::Denied {
                reason: format!("agent_type {agent_type} is not allowlisted"),
            };
        }

        match ctx.action.as_str() {
            actions::ADMIN_APPROVE
            | actions::ADMIN_DENY
            | actions::ADMIN_AGENT_PAUSE
            | actions::ADMIN_AGENT_RESUME
            | actions::ADMIN_AUDIT_READ
            | actions::ADMIN_OBSERVER_READ
            | actions::ADMIN_GRANT_CREATE
            | actions::ADMIN_RUN_READ
            | actions::ADMIN_RUN_RETRY
            | actions::ADMIN_RUN_TERMINATE
            | actions::ADMIN_EXTERNAL_ACTION_DRY_RUN
            | actions::ADMIN_EXTERNAL_ACTION_APPLY => {
                if auth.has_any_role(&[RoleName::SystemAdmin, RoleName::AgentAdmin]) {
                    PolicyDecision::Allowed
                } else {
                    PolicyDecision::Denied {
                        reason: "admin role required".to_string(),
                    }
                }
            }
            actions::ADMIN_OBSERVER_DISCUSS => {
                if auth.has_any_role(&[
                    RoleName::SystemAdmin,
                    RoleName::AgentAdmin,
                    RoleName::Operator,
                ]) {
                    PolicyDecision::Allowed
                } else {
                    PolicyDecision::Denied {
                        reason: "admin or operator role required".to_string(),
                    }
                }
            }
            actions::REQUEST_CREATE_AGENT | actions::REQUEST_CHANGE_AGENT => {
                if ctx.agent_type.as_deref() == Some(AGENT_TYPE_OBSERVER)
                    && !auth.has_role(RoleName::SystemAdmin)
                {
                    return PolicyDecision::Denied {
                        reason: "observer_agent can only be requested by system_admin".to_string(),
                    };
                }
                if auth.has_any_role(&[
                    RoleName::SystemAdmin,
                    RoleName::AgentAdmin,
                    RoleName::ResourceOwner,
                    RoleName::ResourceMaintainer,
                ]) {
                    PolicyDecision::Allowed
                } else {
                    PolicyDecision::ApprovalRequired {
                        reason: "create or change agent requires resource owner approval"
                            .to_string(),
                    }
                }
            }
            actions::RUN_CREATE | actions::SESSION_CREATE | actions::SESSION_APPEND_MESSAGE => {
                PolicyDecision::Allowed
            }
            action if action.starts_with("internal:") => {
                if auth.service_allows(action) {
                    PolicyDecision::Allowed
                } else {
                    PolicyDecision::Denied {
                        reason: "internal action requires service claim".to_string(),
                    }
                }
            }
            _ => PolicyDecision::Denied {
                reason: format!("unknown action {}", ctx.action),
            },
        }
    }

    fn observer_action_is_read_only(action: &str) -> bool {
        matches!(
            action,
            actions::ADMIN_OBSERVER_READ | actions::INTERNAL_OBSERVER_TICK
        )
    }
}

pub fn request_action(request_type: RequestType) -> &'static str {
    match request_type {
        RequestType::CreateAgent => actions::REQUEST_CREATE_AGENT,
        RequestType::ChangeAgent => actions::REQUEST_CHANGE_AGENT,
        RequestType::ResumeAgent => actions::REQUEST_CHANGE_AGENT,
        RequestType::CreateRun => actions::RUN_CREATE,
        RequestType::CreateSession | RequestType::CreateChildSession => actions::SESSION_CREATE,
    }
}

pub fn ensure_allowed(decision: PolicyDecision) -> CoreResult<()> {
    match decision {
        PolicyDecision::Allowed => Ok(()),
        PolicyDecision::Denied { reason } => {
            Err(AgentCoreError::coded(ErrorCode::Forbidden, reason))
        }
        PolicyDecision::ApprovalRequired { reason } => {
            Err(AgentCoreError::coded(ErrorCode::ApprovalRequired, reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_trace_id;

    fn auth(role: RoleName) -> AuthContext {
        AuthContext {
            user_id: "user-1".to_string(),
            service_id: "orchestrator".to_string(),
            service_allowed_actions: vec!["*".to_string()],
            roles: vec![crate::RoleAssignment::global(role)],
            resource_allowlist: vec!["resource:team/project-alpha".to_string()],
            trace_id: new_trace_id(),
        }
    }

    #[test]
    fn observer_cannot_do_control_actions() {
        let ctx = PolicyContext {
            action: actions::ADMIN_AGENT_PAUSE.to_string(),
            request_type: None,
            agent_type: Some(AGENT_TYPE_OBSERVER.to_string()),
            resource: Some(ResourceRef::parse("resource:team/project-alpha").unwrap()),
            risk_level: RiskLevel::Low,
            external_action_mode: ExternalActionMode::Deny,
            resource_attributes: Value::Null,
            observer_mode: true,
        };
        assert!(matches!(
            DefaultPolicy::authorize(&auth(RoleName::SystemAdmin), &ctx),
            PolicyDecision::Denied { .. }
        ));
    }

    #[test]
    fn resource_maintainer_can_request_low_risk_background_worker() {
        let ctx = PolicyContext {
            action: actions::REQUEST_CREATE_AGENT.to_string(),
            request_type: Some(RequestType::CreateAgent),
            agent_type: Some(AGENT_TYPE_BACKGROUND_WORKER.to_string()),
            resource: Some(ResourceRef::parse("resource:team/project-alpha").unwrap()),
            risk_level: RiskLevel::Low,
            external_action_mode: ExternalActionMode::ReadOnly,
            resource_attributes: Value::Null,
            observer_mode: false,
        };
        assert_eq!(
            DefaultPolicy::authorize(&auth(RoleName::ResourceMaintainer), &ctx),
            PolicyDecision::Allowed
        );
    }
}
