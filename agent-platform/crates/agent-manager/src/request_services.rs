use crate::{StoreRef, telemetry_support};
use agent_core::{
    AGENT_TYPE_BACKGROUND_WORKER, AgentBridgeBinding, AgentCoreError, AgentInstance, AgentRequest,
    AgentRequestInput, AgentRequestResponse, AgentRequestStatus, AgentRun, AgentSession,
    ApprovalRequest, ApprovalStatus, AuditDecision, AuditLog, AuthContext, CoreResult, ErrorCode,
    ExternalActionMode, PolicyContext, PolicyDecision, ProfileContract, RequestType, ResourceRef,
    RiskLevel, TriggerType, new_id, request_action,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

pub(crate) async fn submit_request(
    store: &StoreRef,
    auth: &AuthContext,
    input: AgentRequestInput,
) -> CoreResult<AgentRequestResponse> {
    if let Some(key) = &input.idempotency_key
        && let Some(existing) = store
            .find_agent_request_by_idempotency(&auth.user_id, &auth.service_id, key)
            .await?
    {
        return Ok(request_response(&existing));
    }
    let action = request_action(input.request_type);
    telemetry_support::record_request_metric(action, &auth.service_id);

    let mut request = AgentRequest::new(
        auth,
        input.request_type,
        input.agent_type.clone(),
        input.target_resource.clone(),
        input.intent_text.clone(),
        input.structured_payload.clone(),
        input.idempotency_key,
    );
    request = store.create_agent_request(request).await?;
    tracing::info!(
        trace_id = %auth.trace_id,
        request_id = %request.id,
        status = %request.status,
        "agent request created"
    );
    request.status = AgentRequestStatus::Parsed;
    request = store.update_agent_request(request).await?;

    let risk_level = input.risk_level.unwrap_or(RiskLevel::Low);
    let external_action_mode = input.external_action_mode.unwrap_or_else(|| {
        if input.request_type == RequestType::CreateAgent {
            ExternalActionMode::ApprovalRequired
        } else {
            ExternalActionMode::ReadOnly
        }
    });
    let policy = policy_ctx(
        action,
        Some(input.request_type),
        input.agent_type.clone(),
        input.target_resource.clone(),
        risk_level,
        external_action_mode,
    )?;
    let decision = agent_core::DefaultPolicy::authorize(auth, &policy);
    telemetry_support::record_policy_decision_metric(action, &decision);
    request.status = AgentRequestStatus::PolicyChecked;
    request = store.update_agent_request(request).await?;
    tracing::info!(
        trace_id = %auth.trace_id,
        request_id = %request.id,
        status = %request.status,
        action = %action,
        "agent request policy checked"
    );

    match decision {
        PolicyDecision::Denied { reason } => {
            request.status = AgentRequestStatus::Denied;
            request.denial_reason = Some(reason.clone());
            request = store.update_agent_request(request).await?;
            append_audit(
                store,
                Some(auth),
                action,
                AuditDecision::Denied,
                Some(reason),
                &auth.trace_id,
            )
            .await;
            Ok(request_response(&request))
        }
        PolicyDecision::ApprovalRequired { reason } => {
            let approval = ApprovalRequest {
                id: new_id("approval"),
                request_id: request.id.clone(),
                requested_by_user: auth.user_id.clone(),
                approver_user: None,
                status: ApprovalStatus::Pending,
                risk_level: Some(risk_level),
                reason: Some(reason.clone()),
                decision_reason: None,
                created_at: OffsetDateTime::now_utc(),
                decided_at: None,
            };
            let approval = store.create_approval(approval).await?;
            request.status = AgentRequestStatus::ApprovalRequired;
            request.approval_id = Some(approval.id.clone());
            request = store.update_agent_request(request).await?;
            append_audit(
                store,
                Some(auth),
                action,
                AuditDecision::ApprovalRequired,
                Some(reason),
                &auth.trace_id,
            )
            .await;
            Ok(request_response(&request))
        }
        PolicyDecision::Allowed => {
            append_audit(
                store,
                Some(auth),
                action,
                AuditDecision::Allowed,
                None,
                &auth.trace_id,
            )
            .await;
            fulfill_request(store, auth, request).await
        }
    }
}

pub(crate) fn request_response(request: &AgentRequest) -> AgentRequestResponse {
    let message = match request.status {
        AgentRequestStatus::ApprovalRequired => "该请求需要资源负责人审批。",
        AgentRequestStatus::Denied => "该请求已被策略拒绝。",
        AgentRequestStatus::Fulfilled => "请求已完成。",
        AgentRequestStatus::Cancelled => "请求已取消。",
        _ => "请求已记录。",
    };
    AgentRequestResponse {
        request_id: request.id.clone(),
        status: request.status,
        message: message.to_string(),
        approval_id: request.approval_id.clone(),
        agent_id: request.result_agent_id.clone(),
        run_id: request.result_run_id.clone(),
        trace_id: request.trace_id.clone(),
    }
}

pub(crate) async fn fulfill_request(
    store: &StoreRef,
    auth: &AuthContext,
    mut request: AgentRequest,
) -> CoreResult<AgentRequestResponse> {
    match request.request_type {
        RequestType::CreateAgent | RequestType::ChangeAgent | RequestType::ResumeAgent => {
            let agent_type = request
                .agent_type
                .clone()
                .unwrap_or_else(|| AGENT_TYPE_BACKGROUND_WORKER.to_string());
            let target_resource = request.target_resource.clone().ok_or_else(|| {
                AgentCoreError::coded(ErrorCode::Conflict, "target_resource required")
            })?;
            let mut config = request.structured_payload.clone();
            normalize_runtime_requested_tools(&mut config);
            let hash = core_hash(&config);
            if let Some(existing) = store
                .find_reusable_agent(
                    &request.requested_by_user,
                    &agent_type,
                    &target_resource,
                    &hash,
                )
                .await?
            {
                request.status = AgentRequestStatus::Fulfilled;
                request.result_agent_id = Some(existing.id.clone());
                let request = store.update_agent_request(request).await?;
                ensure_open_webui_bridge_binding(
                    store,
                    &request.requested_by_user,
                    &existing.id,
                    &target_resource,
                    &request,
                    &auth.trace_id,
                )
                .await?;
                return Ok(request_response(&request));
            }
            request.status = AgentRequestStatus::Provisioning;
            request = store.update_agent_request(request).await?;
            let mut agent = AgentInstance::new(
                request.requested_by_user.clone(),
                agent_type,
                target_resource,
                hash,
                config,
                auth.trace_id.clone(),
            );
            agent.display_name = request
                .intent_text
                .clone()
                .or_else(|| Some("Agent Platform background worker".to_string()));
            let agent = store.create_agent_instance(agent).await?;
            request.status = AgentRequestStatus::Fulfilled;
            request.result_agent_id = Some(agent.id.clone());
            let request = store.update_agent_request(request).await?;
            ensure_open_webui_bridge_binding(
                store,
                &request.requested_by_user,
                &agent.id,
                &agent.target_resource,
                &request,
                &auth.trace_id,
            )
            .await?;
            Ok(request_response(&request))
        }
        RequestType::CreateRun => {
            let payload_agent_id = request
                .structured_payload
                .get("agent_id")
                .and_then(Value::as_str)
                .ok_or_else(|| AgentCoreError::coded(ErrorCode::Conflict, "agent_id required"))?;
            let agent = store
                .get_agent(payload_agent_id)
                .await?
                .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "agent not found"))?;
            let run = AgentRun::new(
                agent.id,
                None,
                TriggerType::Manual,
                agent.target_resource,
                auth.trace_id.clone(),
            );
            let run = store.create_run(run).await?;
            request.status = AgentRequestStatus::Fulfilled;
            request.result_run_id = Some(run.id);
            let request = store.update_agent_request(request).await?;
            Ok(request_response(&request))
        }
        RequestType::CreateSession | RequestType::CreateChildSession => {
            request.status = AgentRequestStatus::Fulfilled;
            let request = store.update_agent_request(request).await?;
            Ok(request_response(&request))
        }
    }
}

fn normalize_runtime_requested_tools(config: &mut Value) {
    if config.pointer("/runtime/requested_tools").is_some()
        || config.get("requested_tools").is_some()
    {
        return;
    }
    let Some(contract_value) = config
        .pointer("/runtime/profile_contract")
        .or_else(|| config.get("profile_contract"))
        .cloned()
    else {
        return;
    };
    let Ok(contract) = serde_json::from_value::<ProfileContract>(contract_value) else {
        return;
    };
    let requested_tools = contract.tool_policy.effective_tools();
    if requested_tools.is_empty() {
        return;
    }
    let Some(object) = config.as_object_mut() else {
        return;
    };
    let runtime = object.entry("runtime").or_insert_with(|| json!({}));
    if !runtime.is_object() {
        *runtime = json!({});
    }
    if let Some(runtime_object) = runtime.as_object_mut() {
        runtime_object.insert("requested_tools".to_string(), json!(requested_tools));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::RuntimeToolPolicy;

    #[test]
    fn normalize_runtime_requested_tools_derives_scope_from_contract() {
        let mut contract = ProfileContract::new("profile-a", "v1");
        contract.tool_policy =
            RuntimeToolPolicy::read_only(vec!["tool.alpha".to_string(), "tool.beta".to_string()]);
        let mut config = json!({
            "runtime": {
                "profile_contract": contract
            }
        });

        normalize_runtime_requested_tools(&mut config);

        assert_eq!(
            config.pointer("/runtime/requested_tools").unwrap(),
            &json!(["tool.alpha", "tool.beta"])
        );
    }

    #[test]
    fn normalize_runtime_requested_tools_keeps_explicit_scope() {
        let mut contract = ProfileContract::new("profile-a", "v1");
        contract.tool_policy =
            RuntimeToolPolicy::read_only(vec!["tool.alpha".to_string(), "tool.beta".to_string()]);
        let mut config = json!({
            "runtime": {
                "profile_contract": contract,
                "requested_tools": ["tool.alpha"]
            }
        });

        normalize_runtime_requested_tools(&mut config);

        assert_eq!(
            config.pointer("/runtime/requested_tools").unwrap(),
            &json!(["tool.alpha"])
        );
    }
}

pub(crate) fn policy_ctx(
    action: &str,
    request_type: Option<RequestType>,
    agent_type: Option<String>,
    target_resource: Option<String>,
    risk_level: RiskLevel,
    external_action_mode: ExternalActionMode,
) -> CoreResult<PolicyContext> {
    Ok(PolicyContext {
        action: action.to_string(),
        request_type,
        agent_type,
        resource: target_resource.map(ResourceRef::parse).transpose()?,
        risk_level,
        external_action_mode,
        resource_attributes: Value::Null,
        observer_mode: false,
    })
}

fn core_hash(payload: &Value) -> String {
    let mut hasher = Sha256::new();
    let mut payload = payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("bridge_source");
    }
    let encoded = serde_json::to_vec(&payload).unwrap_or_default();
    hasher.update(encoded);
    format!("{:x}", hasher.finalize())
}

async fn ensure_open_webui_bridge_binding(
    store: &StoreRef,
    owner_user: &str,
    agent_id: &str,
    target_resource: &str,
    request: &AgentRequest,
    trace_id: &str,
) -> CoreResult<Option<AgentBridgeBinding>> {
    let Some(source) = BridgeSource::from_payload(&request.structured_payload)? else {
        return Ok(None);
    };
    let idempotency_key = format!("openwebui:{}:{}:{}", owner_user, source.chat_id, agent_id);
    let session = if let Some(existing) = store
        .find_session_by_idempotency(owner_user, agent_id, &idempotency_key)
        .await?
    {
        existing
    } else {
        let mut session = AgentSession::new(
            agent_id.to_string(),
            owner_user.to_string(),
            json!({
                "resource": target_resource,
                "bridge_source": {
                    "kind": "open_webui",
                    "chat_id": source.chat_id.clone(),
                    "session_id": source.session_id.clone(),
                    "model": source.model.clone(),
                }
            }),
            trace_id.to_string(),
        );
        session.idempotency_key = Some(idempotency_key);
        session.source_conversation_id = Some(source.chat_id.clone());
        store.create_session(session).await?
    };
    let mut binding = AgentBridgeBinding::new(
        owner_user.to_string(),
        source.chat_id,
        source.session_id,
        source.model,
        agent_id.to_string(),
        session.id,
        trace_id.to_string(),
    );
    binding.last_message_id = source.message_id;
    store
        .upsert_open_webui_bridge_binding(binding)
        .await
        .map(Some)
}

#[derive(Debug)]
struct BridgeSource {
    chat_id: String,
    session_id: Option<String>,
    message_id: Option<String>,
    model: String,
}

impl BridgeSource {
    fn from_payload(payload: &Value) -> CoreResult<Option<Self>> {
        let Some(source) = payload.get("bridge_source") else {
            return Ok(None);
        };
        if source.get("kind").and_then(Value::as_str) != Some("open_webui") {
            return Ok(None);
        }
        let chat_id = source
            .get("chat_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AgentCoreError::coded(ErrorCode::Conflict, "bridge chat_id required"))?
            .to_string();
        let model = source
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("hermes-agent")
            .to_string();
        Ok(Some(Self {
            chat_id,
            session_id: source
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            message_id: source
                .get("message_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            model,
        }))
    }
}

async fn append_audit(
    store: &StoreRef,
    auth: Option<&AuthContext>,
    action: &str,
    decision: AuditDecision,
    reason: Option<String>,
    trace_id: &str,
) {
    let _ = store
        .append_audit(AuditLog::new(
            auth,
            action,
            decision,
            reason,
            trace_id.to_string(),
        ))
        .await;
}
