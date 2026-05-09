use crate::StoreRef;
use agent_core::{
    AgentCoreError, AgentGrant, AgentInstanceStatus, AgentRun, AgentSessionMessage, AgentSummary,
    AppendMessageInput, AuditDecision, AuditLog, AuthContext, CoreResult, CreateGrantInput,
    ErrorCode, ExternalActionMode, ResourceRef, RiskLevel, TriggerType, WebhookTriggerInput,
    actions, new_id,
};
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;

#[derive(Debug, Serialize)]
pub(crate) struct WebhookAcceptedResponse {
    pub(crate) status: &'static str,
    pub(crate) connector: String,
    pub(crate) event_type: String,
    pub(crate) dedupe_key: String,
    pub(crate) payload_ref: String,
    pub(crate) run_ids: Vec<String>,
    pub(crate) trace_id: String,
}

pub(crate) async fn create_grant(
    store: &StoreRef,
    auth: &AuthContext,
    input: CreateGrantInput,
) -> CoreResult<AgentGrant> {
    validate_grant_input(&input)?;
    let grant = AgentGrant {
        id: new_id("grant"),
        subject_type: input.subject_type,
        subject_id: input.subject_id,
        action: input.action,
        resource_type: input.resource_type,
        resource_id: input.resource_id,
        constraints: if input.constraints.is_null() {
            json!({})
        } else {
            input.constraints
        },
        granted_by: Some(auth.user_id.clone()),
        created_at: OffsetDateTime::now_utc(),
        expires_at: input.expires_at,
    };
    let grant = store.create_grant(grant).await?;
    append_audit(
        store,
        Some(auth),
        actions::ADMIN_GRANT_CREATE,
        AuditDecision::Allowed,
        Some(format!("grant_id={}", grant.id)),
        &auth.trace_id,
    )
    .await;
    Ok(grant)
}

pub(crate) async fn append_message_to_session(
    store: &StoreRef,
    auth: &AuthContext,
    session_id: &str,
    input: AppendMessageInput,
    require_owner: bool,
    action: &str,
) -> CoreResult<AgentSessionMessage> {
    let session = store
        .get_session(session_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    if require_owner && session.owner_user != auth.user_id {
        return Err(AgentCoreError::coded(ErrorCode::NotFound, "not found"));
    }
    let sequence = store.next_message_sequence(session_id).await?;
    let AppendMessageInput {
        role,
        content_summary,
        content_ref,
        external_message_id,
        run_id,
    } = input;
    let mut message = AgentSessionMessage::new(
        session_id,
        sequence,
        role,
        Some(content_summary),
        run_id,
        auth.trace_id.clone(),
    );
    message.content_ref = content_ref;
    message.external_message_id = external_message_id;
    let message = store.append_message(message).await?;
    append_audit(
        store,
        Some(auth),
        action,
        AuditDecision::Allowed,
        None,
        &auth.trace_id,
    )
    .await;
    Ok(message)
}

pub(crate) async fn accept_webhook(
    store: &StoreRef,
    auth: &AuthContext,
    connector: String,
    input: WebhookTriggerInput,
) -> CoreResult<WebhookAcceptedResponse> {
    let resource = validate_webhook_input(&connector, &input)?;
    let runs = create_webhook_runs(store, auth, &input).await?;
    append_audit(
        store,
        Some(auth),
        actions::INTERNAL_WEBHOOK,
        AuditDecision::Allowed,
        Some(format!(
            "connector={connector}; event_type={}; resource_type={}; resource_id={}; run_count={}",
            input.event_type,
            resource.resource_type,
            resource.resource_id,
            runs.len()
        )),
        &auth.trace_id,
    )
    .await;
    Ok(WebhookAcceptedResponse {
        status: "accepted",
        connector,
        event_type: input.event_type,
        dedupe_key: input.dedupe_key,
        payload_ref: input.payload_ref,
        run_ids: runs.iter().map(|run| run.id.clone()).collect(),
        trace_id: auth.trace_id.clone(),
    })
}

pub(crate) async fn create_internal_run(
    store: &StoreRef,
    auth: &AuthContext,
    mut run: AgentRun,
) -> CoreResult<AgentRun> {
    run.trace_id = auth.trace_id.clone();
    if let Some(key) = &run.idempotency_key
        && let Some(existing) = store.find_run_by_idempotency(&run.agent_id, key).await?
    {
        return Ok(existing);
    }
    store.create_run(run).await
}

fn validate_grant_input(input: &CreateGrantInput) -> CoreResult<()> {
    if input.subject_type.trim().is_empty()
        || input.subject_id.trim().is_empty()
        || input.action.trim().is_empty()
        || input.resource_type.trim().is_empty()
        || input.resource_id.trim().is_empty()
    {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "grant fields must not be empty",
        ))
    } else {
        Ok(())
    }
}

fn validate_webhook_input(
    input_connector: &str,
    input: &WebhookTriggerInput,
) -> CoreResult<ResourceRef> {
    if input.connector != input_connector {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "webhook connector path/body mismatch",
        ));
    }
    if input.trigger_type != TriggerType::Webhook.to_string() {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "webhook trigger_type must be webhook",
        ));
    }
    if input.dedupe_key.trim().is_empty() {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "webhook dedupe_key is required",
        ));
    }
    if input.payload_ref.trim().is_empty() {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "webhook payload_ref is required",
        ));
    }
    ResourceRef::parse(input.resource.clone())
}

async fn create_webhook_runs(
    store: &StoreRef,
    auth: &AuthContext,
    input: &WebhookTriggerInput,
) -> CoreResult<Vec<AgentRun>> {
    let agents = store.list_agents(None, 1000).await?;
    let mut runs = Vec::new();
    for agent in agents
        .into_iter()
        .filter(|agent| webhook_targets_agent(agent, input))
    {
        if let Some(existing) = store
            .find_run_by_idempotency(&agent.agent_id, &input.dedupe_key)
            .await?
        {
            runs.push(existing);
            continue;
        }

        let mut run = AgentRun::new(
            agent.agent_id,
            None,
            TriggerType::Webhook,
            input.resource.clone(),
            auth.trace_id.clone(),
        );
        run.idempotency_key = Some(input.dedupe_key.clone());
        run.risk_level = RiskLevel::Low;
        run.external_action_mode = ExternalActionMode::ReadOnly;
        runs.push(store.create_run(run).await?);
    }
    Ok(runs)
}

fn webhook_targets_agent(agent: &AgentSummary, input: &WebhookTriggerInput) -> bool {
    agent.status == AgentInstanceStatus::Running && agent.target_resource == input.resource
}

pub(crate) async fn append_audit(
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
