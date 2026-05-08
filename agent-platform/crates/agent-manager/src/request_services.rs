use crate::{StoreRef, telemetry_support};
use agent_core::{
    AGENT_TYPE_BACKGROUND_WORKER, AgentCoreError, AgentInstance, AgentRequest, AgentRequestInput,
    AgentRequestResponse, AgentRequestStatus, AgentRun, ApprovalRequest, ApprovalStatus,
    AuditDecision, AuditLog, AuthContext, CoreResult, ErrorCode, PolicyContext, PolicyDecision,
    RequestType, ResourceRef, RiskLevel, SideEffectMode, TriggerType, new_id, request_action,
};
use serde_json::Value;
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
    let side_effect_mode = input.side_effect_mode.unwrap_or_else(|| {
        if input.request_type == RequestType::CreateAgent {
            SideEffectMode::ApprovalRequired
        } else {
            SideEffectMode::ReadOnly
        }
    });
    let policy = policy_ctx(
        action,
        Some(input.request_type),
        input.agent_type.clone(),
        input.target_resource.clone(),
        risk_level,
        side_effect_mode,
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
            let hash = core_hash(&request.structured_payload);
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
                request.result_agent_id = Some(existing.id);
                let request = store.update_agent_request(request).await?;
                return Ok(request_response(&request));
            }
            request.status = AgentRequestStatus::Provisioning;
            request = store.update_agent_request(request).await?;
            let mut agent = AgentInstance::new(
                request.requested_by_user.clone(),
                agent_type,
                target_resource,
                hash,
                request.structured_payload.clone(),
                auth.trace_id.clone(),
            );
            agent.display_name = request
                .intent_text
                .clone()
                .or_else(|| Some("P0 background worker".to_string()));
            let agent = store.create_agent_instance(agent).await?;
            request.status = AgentRequestStatus::Fulfilled;
            request.result_agent_id = Some(agent.id);
            let request = store.update_agent_request(request).await?;
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

pub(crate) fn policy_ctx(
    action: &str,
    request_type: Option<RequestType>,
    agent_type: Option<String>,
    target_resource: Option<String>,
    risk_level: RiskLevel,
    side_effect_mode: SideEffectMode,
) -> CoreResult<PolicyContext> {
    Ok(PolicyContext {
        action: action.to_string(),
        request_type,
        agent_type,
        resource: target_resource.map(ResourceRef::parse).transpose()?,
        risk_level,
        side_effect_mode,
        resource_attributes: Value::Null,
        observer_mode: false,
    })
}

fn core_hash(payload: &Value) -> String {
    let mut hasher = Sha256::new();
    let encoded = serde_json::to_vec(payload).unwrap_or_default();
    hasher.update(encoded);
    format!("{:x}", hasher.finalize())
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
