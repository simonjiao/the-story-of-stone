use crate::{StoreRef, control_services, request_services, telemetry_support};
use agent_core::{
    AgentCoreError, AgentInstance, AgentRun, AgentSession, AuditDecision, AuthContext, CoreResult,
    CreateChildSessionInput, CreateRunInput, CreateSessionInput, ErrorCode, PolicyDecision,
    RequestType, RiskLevel, RoleName, SideEffectMode, actions,
};
use serde_json::json;

pub(crate) async fn create_session(
    store: &StoreRef,
    auth: &AuthContext,
    agent_id: String,
    input: CreateSessionInput,
) -> CoreResult<AgentSession> {
    let agent = load_agent_for_user(store, auth, &agent_id).await?;
    if let Some(key) = &input.idempotency_key
        && let Some(existing) = store
            .find_session_by_idempotency(&auth.user_id, &agent.id, key)
            .await?
    {
        return Ok(existing);
    }

    let resource_scope = if input.resource_scope.is_null() {
        json!({"resource": agent.target_resource.clone()})
    } else {
        input.resource_scope
    };
    let mut session = AgentSession::new(
        agent.id.clone(),
        auth.user_id.clone(),
        resource_scope,
        auth.trace_id.clone(),
    );
    session.idempotency_key = input.idempotency_key;
    session.source_conversation_id = input.source_conversation_id;
    let session = store.create_session(session).await?;
    control_services::append_audit(
        store,
        Some(auth),
        actions::SESSION_CREATE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(session)
}

pub(crate) async fn create_child_session(
    store: &StoreRef,
    auth: &AuthContext,
    parent_session_id: String,
    input: CreateChildSessionInput,
) -> CoreResult<AgentSession> {
    let parent = store
        .get_session(&parent_session_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    if parent.owner_user != auth.user_id || parent.depth >= 1 {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "child session depth or owner constraint failed",
        ));
    }

    let children = store.list_child_sessions(&parent_session_id).await?;
    let active_children = children
        .iter()
        .filter(|child| child.status == agent_core::AgentSessionStatus::Active)
        .count();
    if children.len() >= 3 || active_children >= 2 {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "child session budget exceeded",
        ));
    }

    let mut child = AgentSession::new(
        input.agent_id.unwrap_or(parent.agent_id.clone()),
        auth.user_id.clone(),
        if input.resource_scope.is_null() {
            parent.resource_scope.clone()
        } else {
            input.resource_scope
        },
        auth.trace_id.clone(),
    );
    child.parent_session_id = Some(parent_session_id.clone());
    child.created_by_session_id = Some(parent_session_id);
    child.depth = parent.depth + 1;
    child.context_summary = input.context_summary.or(parent.context_summary);
    let child = store.create_session(child).await?;
    control_services::append_audit(
        store,
        Some(auth),
        actions::SESSION_CREATE_CHILD,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(child)
}

pub(crate) async fn create_run(
    store: &StoreRef,
    auth: &AuthContext,
    agent_id: String,
    input: CreateRunInput,
) -> CoreResult<AgentRun> {
    tracing::info!(
        trace_id = %auth.trace_id,
        user_id = %auth.user_id,
        service_id = %auth.service_id,
        agent_id = %agent_id,
        trigger_type = %input.trigger_type,
        "manager create run"
    );
    telemetry_support::record_request_metric(actions::RUN_CREATE, &auth.service_id);
    let agent = load_agent_for_user(store, auth, &agent_id).await?;
    if let Some(key) = &input.idempotency_key
        && let Some(existing) = store.find_run_by_idempotency(&agent.id, key).await?
    {
        return Ok(existing);
    }

    let mut run = AgentRun::new(
        agent.id.clone(),
        input.session_id,
        input.trigger_type,
        input
            .target_resource
            .unwrap_or(agent.target_resource.clone()),
        auth.trace_id.clone(),
    );
    run.idempotency_key = input.idempotency_key;
    run.risk_level = input.risk_level.unwrap_or(RiskLevel::Low);
    run.side_effect_mode = input.side_effect_mode.unwrap_or(SideEffectMode::ReadOnly);
    let policy = request_services::policy_ctx(
        actions::RUN_CREATE,
        Some(RequestType::CreateRun),
        Some(agent.agent_type.clone()),
        Some(run.target_resource.clone()),
        run.risk_level,
        run.side_effect_mode,
    )?;
    let decision = agent_core::DefaultPolicy::authorize(auth, &policy);
    telemetry_support::record_policy_decision_metric(actions::RUN_CREATE, &decision);
    match decision {
        PolicyDecision::Allowed => {}
        PolicyDecision::Denied { reason } | PolicyDecision::ApprovalRequired { reason } => {
            control_services::append_audit(
                store,
                Some(auth),
                actions::RUN_CREATE,
                AuditDecision::Denied,
                Some(reason.clone()),
                &auth.trace_id,
            )
            .await;
            return Err(AgentCoreError::coded(ErrorCode::Forbidden, reason));
        }
    }
    let run = store.create_run(run).await?;
    control_services::append_audit(
        store,
        Some(auth),
        actions::RUN_CREATE,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(run)
}

async fn load_agent_for_user(
    store: &StoreRef,
    auth: &AuthContext,
    agent_id: &str,
) -> CoreResult<AgentInstance> {
    let agent = store
        .get_agent(agent_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    if agent.owner_user != auth.user_id
        && !auth.has_any_role(&[RoleName::SystemAdmin, RoleName::AgentAdmin])
    {
        return Err(AgentCoreError::coded(ErrorCode::NotFound, "not found"));
    }
    Ok(agent)
}
