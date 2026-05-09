use crate::StoreRef;
use agent_core::{
    AgentCoreError, AgentRun, ApprovalStatus, AuditDecision, AuditLog, AuthContext, CoreResult,
    CredentialLeaseRequest, ErrorCode, ResourceRef, RiskLevel, SideEffectMode, SideEffectPlan,
    SideEffectPlanDryRunInput, SideEffectPlanDryRunResponse, SideEffectPlanStatus,
    WriteConnectorDryRunInput, actions, metric_names, side_effect_requires_credential,
};
use agent_runtime::{NoopCredentialProvider, NoopWriteConnector};
use serde_json::json;

pub(crate) async fn dry_run_side_effect_plan(
    store: &StoreRef,
    auth: &AuthContext,
    run_id: String,
    input: SideEffectPlanDryRunInput,
) -> CoreResult<SideEffectPlanDryRunResponse> {
    validate_input(&input)?;
    let run = store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    let risk_level = input.risk_level.unwrap_or(run.risk_level);
    let side_effect_mode = input.side_effect_mode.unwrap_or(run.side_effect_mode);
    let mut plan = SideEffectPlan::new(
        run.id.clone(),
        input.connector,
        input.action,
        input.resource_ref,
        risk_level,
        side_effect_mode,
        auth.trace_id.clone(),
    );
    plan.approval_id = input.approval_id;
    plan.credential_scope = input.credential_scope;
    plan.input_summary = input.input_summary;
    plan.input_ref = input.input_ref;

    let decision = dry_run_decision(store, &run, &plan).await?;
    match decision {
        DryRunDecision::Ready => {
            let dry_run = agent_core::WriteConnector::dry_run(
                &NoopWriteConnector,
                WriteConnectorDryRunInput {
                    plan: plan.clone(),
                    payload: json!({}),
                    trace_id: auth.trace_id.clone(),
                },
            )
            .await?;
            plan.status = SideEffectPlanStatus::DryRunReady;
            plan.result_ref = dry_run.result_ref;
        }
        DryRunDecision::Rejected(error_code) => {
            plan.status = SideEffectPlanStatus::DryRunRejected;
            plan.error_code = Some(error_code);
        }
    }

    let plan = store.create_side_effect_plan(plan).await?;
    let credential_lease = if plan.status == SideEffectPlanStatus::DryRunReady {
        let scope = plan.credential_scope.clone().ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::Conflict,
                "credential_scope required for dry-run ready plan",
            )
        })?;
        let lease = agent_core::CredentialProvider::dry_run_lease(
            &NoopCredentialProvider,
            CredentialLeaseRequest {
                side_effect_plan_id: plan.id.clone(),
                credential_scope: scope,
                trace_id: auth.trace_id.clone(),
            },
        )
        .await?;
        Some(store.create_credential_lease(lease).await?)
    } else {
        None
    };

    let mut audit = AuditLog::new(
        Some(auth),
        actions::ADMIN_SIDE_EFFECT_DRY_RUN,
        if plan.status == SideEffectPlanStatus::DryRunReady {
            AuditDecision::Allowed
        } else {
            AuditDecision::Denied
        },
        Some(format!(
            "plan_id={} run_id={} status={}",
            plan.id, plan.run_id, plan.status
        )),
        auth.trace_id.clone(),
    );
    audit.run_id = Some(plan.run_id.clone());
    audit.approval_id = plan.approval_id.clone();
    audit.resource_type = Some("side_effect_plan".to_string());
    audit.resource_id = Some(plan.id.clone());
    let _ = store.append_audit(audit).await;
    metrics::counter!(
        metric_names::SIDE_EFFECT_DRY_RUN_TOTAL,
        "status" => plan.status.to_string()
    )
    .increment(1);

    Ok(SideEffectPlanDryRunResponse {
        dry_run_status: plan.status,
        trace_id: auth.trace_id.clone(),
        plan,
        credential_lease,
    })
}

enum DryRunDecision {
    Ready,
    Rejected(String),
}

fn validate_input(input: &SideEffectPlanDryRunInput) -> CoreResult<()> {
    if input.connector.trim().is_empty()
        || input.action.trim().is_empty()
        || input.resource_ref.trim().is_empty()
    {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "connector, action and resource_ref are required",
        ));
    }
    ResourceRef::parse(input.resource_ref.clone())?;
    Ok(())
}

async fn dry_run_decision(
    store: &StoreRef,
    run: &AgentRun,
    plan: &SideEffectPlan,
) -> CoreResult<DryRunDecision> {
    if matches!(
        plan.side_effect_mode,
        SideEffectMode::Deny | SideEffectMode::ReadOnly
    ) {
        return Ok(DryRunDecision::Rejected(
            "side_effect_mode_not_write".to_string(),
        ));
    }
    if matches!(plan.risk_level, RiskLevel::Critical) {
        return Ok(DryRunDecision::Rejected("critical_risk_denied".to_string()));
    }
    if side_effect_requires_credential(plan.side_effect_mode, plan.risk_level)
        && plan.credential_scope.as_deref().is_none_or(str::is_empty)
    {
        return Ok(DryRunDecision::Rejected(
            "credential_scope_required".to_string(),
        ));
    }
    if approval_required(plan) && plan.approval_id.as_deref().is_none_or(str::is_empty) {
        return Ok(DryRunDecision::Rejected("approval_required".to_string()));
    }
    if let Some(approval_id) = plan
        .approval_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let Some(approval) = store.get_approval(approval_id).await? else {
            return Ok(DryRunDecision::Rejected("approval_not_found".to_string()));
        };
        if approval.status != ApprovalStatus::Approved {
            return Ok(DryRunDecision::Rejected(format!(
                "approval_not_approved:{}",
                approval.status
            )));
        }
    }
    let resource = ResourceRef::parse(plan.resource_ref.clone())?;
    if let Some(lock) = store
        .active_resource_lock(
            &resource.resource_type,
            &resource.resource_id,
            "side_effect",
        )
        .await?
        && lock.holder_run_id != run.id
    {
        return Ok(DryRunDecision::Rejected("resource_locked".to_string()));
    }
    Ok(DryRunDecision::Ready)
}

fn approval_required(plan: &SideEffectPlan) -> bool {
    matches!(
        plan.side_effect_mode,
        SideEffectMode::ApprovalRequired | SideEffectMode::Authorized
    ) || matches!(plan.risk_level, RiskLevel::High | RiskLevel::Critical)
}
