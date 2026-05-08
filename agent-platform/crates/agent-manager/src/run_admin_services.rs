use crate::StoreRef;
use agent_core::{
    AgentCoreError, AgentRun, AgentRunStatus, AuditDecision, AuthContext, CoreResult, ErrorCode,
    actions,
};

pub(crate) async fn retry_dead_letter_run(
    store: &StoreRef,
    auth: &AuthContext,
    run_id: &str,
    reason: Option<String>,
) -> CoreResult<AgentRun> {
    ensure_dead_letter(store, run_id).await?;
    let reason = reason.unwrap_or_else(|| "admin requested dead-letter retry".to_string());
    let run = store.retry_run(run_id, &reason, &auth.trace_id).await?;
    audit_run_admin(
        store,
        auth,
        actions::ADMIN_RUN_RETRY,
        AuditDecision::Allowed,
        Some(reason),
        &run.id,
    )
    .await;
    Ok(run)
}

pub(crate) async fn terminate_dead_letter_run(
    store: &StoreRef,
    auth: &AuthContext,
    run_id: &str,
    reason: Option<String>,
) -> CoreResult<AgentRun> {
    ensure_dead_letter(store, run_id).await?;
    let reason = reason.unwrap_or_else(|| "admin terminated dead-letter run".to_string());
    let run = store.terminate_run(run_id, &reason, &auth.trace_id).await?;
    audit_run_admin(
        store,
        auth,
        actions::ADMIN_RUN_TERMINATE,
        AuditDecision::Allowed,
        Some(reason),
        &run.id,
    )
    .await;
    Ok(run)
}

async fn ensure_dead_letter(store: &StoreRef, run_id: &str) -> CoreResult<()> {
    let run = store
        .get_run(run_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    if run.run_status == AgentRunStatus::DeadLetter {
        Ok(())
    } else {
        Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "admin run retry/terminate is only allowed for dead_letter runs",
        ))
    }
}

async fn audit_run_admin(
    store: &StoreRef,
    auth: &AuthContext,
    action: &str,
    decision: AuditDecision,
    reason: Option<String>,
    run_id: &str,
) {
    let mut audit =
        agent_core::AuditLog::new(Some(auth), action, decision, reason, auth.trace_id.clone());
    audit.run_id = Some(run_id.to_string());
    let _ = store.append_audit(audit).await;
}
