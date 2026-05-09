use crate::StoreRef;
use agent_core::{
    AgentCoreError, AgentRun, AgentRunStatus, ApprovalStatus, AuditDecision, AuditLog, AuthContext,
    CoreResult, CredentialLease, CredentialLeaseRequest, CredentialLeaseStatus, CredentialProvider,
    ErrorCode, ExternalActionMode, ExternalActionPlan, ExternalActionPlanApplyInput,
    ExternalActionPlanApplyResponse, ExternalActionPlanDryRunInput,
    ExternalActionPlanDryRunResponse, ExternalActionPlanStatus, ResourceLock, ResourceRef,
    RiskLevel, WriteConnector, WriteConnectorDryRunInput, WriteConnectorExecuteInput, actions,
    external_action_requires_credential, metric_names, new_id,
};
use agent_runtime::{
    HttpCredentialProvider, HttpCredentialProviderConfig, HttpWriteConnector,
    HttpWriteConnectorConfig, NoopCredentialProvider, NoopWriteConnector,
};
use serde_json::json;
use std::time::Duration;
use time::OffsetDateTime;

pub(crate) async fn dry_run_external_action_plan(
    store: &StoreRef,
    auth: &AuthContext,
    run_id: String,
    input: ExternalActionPlanDryRunInput,
) -> CoreResult<ExternalActionPlanDryRunResponse> {
    validate_input(&input)?;
    let run = store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    let risk_level = input.risk_level.unwrap_or(run.risk_level);
    let external_action_mode = input
        .external_action_mode
        .unwrap_or(run.external_action_mode);
    let mut plan = ExternalActionPlan::new(
        run.id.clone(),
        input.connector,
        input.action,
        input.resource_ref,
        risk_level,
        external_action_mode,
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
            plan.status = ExternalActionPlanStatus::DryRunReady;
            plan.result_ref = dry_run.result_ref;
        }
        DryRunDecision::Rejected(error_code) => {
            plan.status = ExternalActionPlanStatus::DryRunRejected;
            plan.error_code = Some(error_code);
        }
    }

    let plan = store.create_external_action_plan(plan).await?;
    let credential_lease = if plan.status == ExternalActionPlanStatus::DryRunReady {
        let scope = plan.credential_scope.clone().ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::Conflict,
                "credential_scope required for dry-run ready plan",
            )
        })?;
        let lease = agent_core::CredentialProvider::dry_run_lease(
            &NoopCredentialProvider,
            CredentialLeaseRequest {
                external_action_plan_id: plan.id.clone(),
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
        actions::ADMIN_EXTERNAL_ACTION_DRY_RUN,
        if plan.status == ExternalActionPlanStatus::DryRunReady {
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
    audit.resource_type = Some("external_action_plan".to_string());
    audit.resource_id = Some(plan.id.clone());
    let _ = store.append_audit(audit).await;
    metrics::counter!(
        metric_names::EXTERNAL_ACTION_DRY_RUN_TOTAL,
        "status" => plan.status.to_string()
    )
    .increment(1);

    Ok(ExternalActionPlanDryRunResponse {
        dry_run_status: plan.status,
        trace_id: auth.trace_id.clone(),
        plan,
        credential_lease,
    })
}

pub(crate) async fn apply_external_action_plan(
    store: &StoreRef,
    auth: &AuthContext,
    run_id: String,
    plan_id: String,
    input: ExternalActionPlanApplyInput,
) -> CoreResult<ExternalActionPlanApplyResponse> {
    let credential_provider = HttpCredentialProvider::new(
        HttpCredentialProviderConfig::from_env().ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::Conflict,
                "external action credential provider is not configured",
            )
        })?,
    )?;
    let write_connector =
        HttpWriteConnector::new(HttpWriteConnectorConfig::from_env().ok_or_else(|| {
            AgentCoreError::coded(
                ErrorCode::Conflict,
                "external action write connector is not configured",
            )
        })?)?;
    apply_external_action_plan_with_adapters(
        store,
        auth,
        run_id,
        plan_id,
        input,
        &credential_provider,
        &write_connector,
        ExternalActionApplyConfig::from_env(),
    )
    .await
}

pub(crate) async fn apply_external_action_plan_with_adapters(
    store: &StoreRef,
    auth: &AuthContext,
    run_id: String,
    plan_id: String,
    input: ExternalActionPlanApplyInput,
    credential_provider: &dyn CredentialProvider,
    write_connector: &dyn WriteConnector,
    config: ExternalActionApplyConfig,
) -> CoreResult<ExternalActionPlanApplyResponse> {
    let run = store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    let plan = store
        .get_external_action_plan(&plan_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    if plan.run_id != run.id {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "external-action plan does not belong to run",
        ));
    }
    validate_apply_preconditions(store, auth, &run, &plan).await?;
    let resource = ResourceRef::parse(plan.resource_ref.clone())?;
    let lock = match store
        .acquire_resource_lock(
            ResourceLock {
                id: new_id("lock"),
                resource_type: resource.resource_type,
                resource_id: resource.resource_id,
                lock_scope: "external_action".to_string(),
                holder_run_id: run.id.clone(),
                lease_until: OffsetDateTime::now_utc(),
                created_at: OffsetDateTime::now_utc(),
            },
            config.lock_lease,
        )
        .await
    {
        Ok(lock) => lock,
        Err(error) if error.code() == ErrorCode::Conflict => {
            let failed = store
                .update_external_action_plan_status(
                    &plan.id,
                    ExternalActionPlanStatus::Failed,
                    plan.result_ref.as_deref(),
                    plan.compensation_ref.as_deref(),
                    Some("resource_locked"),
                    &auth.trace_id,
                )
                .await?;
            append_apply_audit(
                store,
                auth,
                &failed,
                AuditDecision::Conflict,
                Some("resource_locked".to_string()),
            )
            .await;
            return Err(error);
        }
        Err(error) => return Err(error),
    };

    append_apply_audit(
        store,
        auth,
        &plan,
        AuditDecision::Allowed,
        Some(format!(
            "plan_id={} run_id={} lock_id={} lock_scope={} status=started",
            plan.id, run.id, lock.id, lock.lock_scope
        )),
    )
    .await;

    let credential_lease =
        match active_credential_lease(store, credential_provider, &plan, &auth.trace_id).await {
            Ok(lease) => lease,
            Err(error) => {
                let _ = store.release_resource_lock(&run.id).await;
                let failed = fail_plan(store, auth, &plan, "credential_provider_failed").await?;
                append_apply_audit(
                    store,
                    auth,
                    &failed,
                    AuditDecision::Failed,
                    Some(format!(
                        "plan_id={} lock_id={} status=credential_provider_failed",
                        failed.id, lock.id
                    )),
                )
                .await;
                return Err(error);
            }
        };

    let execute_result = execute_with_retries(
        write_connector,
        WriteConnectorExecuteInput {
            plan: plan.clone(),
            idempotency_key: plan.id.clone(),
            credential_provider_ref: credential_lease.provider_ref.clone(),
            payload: input.payload,
            trace_id: auth.trace_id.clone(),
        },
        config.max_attempts,
    )
    .await;
    let _ = store.release_resource_lock(&run.id).await;

    match execute_result {
        Ok(output) if output.accepted => {
            let output = match validate_connector_success(output) {
                Ok(output) => output,
                Err(error) => {
                    let failed = fail_plan(store, auth, &plan, "connector_invalid_result").await?;
                    append_apply_audit(
                        store,
                        auth,
                        &failed,
                        AuditDecision::Failed,
                        Some(format!(
                            "plan_id={} lock_id={} status=connector_invalid_result",
                            failed.id, lock.id
                        )),
                    )
                    .await;
                    metrics::counter!(
                        metric_names::EXTERNAL_ACTION_APPLY_TOTAL,
                        "status" => failed.status.to_string()
                    )
                    .increment(1);
                    return Err(error);
                }
            };
            let applied = store
                .update_external_action_plan_status(
                    &plan.id,
                    ExternalActionPlanStatus::Applied,
                    output.result_ref.as_deref(),
                    output.compensation_ref.as_deref(),
                    None,
                    &auth.trace_id,
                )
                .await?;
            append_apply_audit(
                store,
                auth,
                &applied,
                AuditDecision::Completed,
                Some(format!(
                    "plan_id={} run_id={} lock_id={} status=applied",
                    applied.id, applied.run_id, lock.id
                )),
            )
            .await;
            metrics::counter!(
                metric_names::EXTERNAL_ACTION_APPLY_TOTAL,
                "status" => applied.status.to_string()
            )
            .increment(1);
            Ok(ExternalActionPlanApplyResponse {
                apply_status: applied.status,
                plan: applied,
                credential_lease,
                resource_lock: lock,
                connector_metadata: output.metadata,
                trace_id: auth.trace_id.clone(),
            })
        }
        Ok(output) => {
            let error_code = output.error_code.as_deref().unwrap_or("connector_rejected");
            let failed = store
                .update_external_action_plan_status(
                    &plan.id,
                    ExternalActionPlanStatus::Failed,
                    output.result_ref.as_deref(),
                    output.compensation_ref.as_deref(),
                    Some(error_code),
                    &auth.trace_id,
                )
                .await?;
            append_apply_audit(
                store,
                auth,
                &failed,
                AuditDecision::Denied,
                Some(format!(
                    "plan_id={} lock_id={} status={error_code}",
                    failed.id, lock.id
                )),
            )
            .await;
            metrics::counter!(
                metric_names::EXTERNAL_ACTION_APPLY_TOTAL,
                "status" => failed.status.to_string()
            )
            .increment(1);
            Ok(ExternalActionPlanApplyResponse {
                apply_status: failed.status,
                plan: failed,
                credential_lease,
                resource_lock: lock,
                connector_metadata: output.metadata,
                trace_id: auth.trace_id.clone(),
            })
        }
        Err(error) => {
            let _ = dead_letter_external_action_run(store, &run, &auth.trace_id).await;
            let failed = fail_plan(store, auth, &plan, "connector_dead_letter").await?;
            append_apply_audit(
                store,
                auth,
                &failed,
                AuditDecision::Failed,
                Some(format!(
                    "plan_id={} lock_id={} status=connector_dead_letter",
                    failed.id, lock.id
                )),
            )
            .await;
            metrics::counter!(
                metric_names::EXTERNAL_ACTION_APPLY_TOTAL,
                "status" => failed.status.to_string()
            )
            .increment(1);
            Err(error)
        }
    }
}

enum DryRunDecision {
    Ready,
    Rejected(String),
}

fn validate_input(input: &ExternalActionPlanDryRunInput) -> CoreResult<()> {
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
    plan: &ExternalActionPlan,
) -> CoreResult<DryRunDecision> {
    if matches!(
        plan.external_action_mode,
        ExternalActionMode::Deny | ExternalActionMode::ReadOnly
    ) {
        return Ok(DryRunDecision::Rejected(
            "external_action_mode_not_write".to_string(),
        ));
    }
    if matches!(plan.risk_level, RiskLevel::Critical) {
        return Ok(DryRunDecision::Rejected("critical_risk_denied".to_string()));
    }
    if external_action_requires_credential(plan.external_action_mode, plan.risk_level)
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
            "external_action",
        )
        .await?
        && lock.holder_run_id != run.id
    {
        return Ok(DryRunDecision::Rejected("resource_locked".to_string()));
    }
    Ok(DryRunDecision::Ready)
}

fn approval_required(plan: &ExternalActionPlan) -> bool {
    matches!(
        plan.external_action_mode,
        ExternalActionMode::ApprovalRequired | ExternalActionMode::Authorized
    ) || matches!(plan.risk_level, RiskLevel::High | RiskLevel::Critical)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExternalActionApplyConfig {
    pub lock_lease: Duration,
    pub max_attempts: u32,
}

impl ExternalActionApplyConfig {
    pub fn from_env() -> Self {
        Self {
            lock_lease: Duration::from_secs(env_u64(
                "AGENT_EXTERNAL_ACTION_LOCK_LEASE_SECONDS",
                300,
            )),
            max_attempts: env_u32("AGENT_WRITE_CONNECTOR_MAX_ATTEMPTS", 3).max(1),
        }
    }
}

async fn validate_apply_preconditions(
    store: &StoreRef,
    auth: &AuthContext,
    run: &AgentRun,
    plan: &ExternalActionPlan,
) -> CoreResult<()> {
    if plan.status != ExternalActionPlanStatus::DryRunReady {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "only dry_run_ready external-action plans can be applied",
        ));
    }
    match dry_run_decision(store, run, plan).await? {
        DryRunDecision::Ready => Ok(()),
        DryRunDecision::Rejected(error_code) => {
            let failed = store
                .update_external_action_plan_status(
                    &plan.id,
                    ExternalActionPlanStatus::Failed,
                    plan.result_ref.as_deref(),
                    plan.compensation_ref.as_deref(),
                    Some(&error_code),
                    &auth.trace_id,
                )
                .await?;
            append_apply_audit(
                store,
                auth,
                &failed,
                if error_code == "resource_locked" {
                    AuditDecision::Conflict
                } else {
                    AuditDecision::Denied
                },
                Some(error_code.clone()),
            )
            .await;
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                format!("external-action plan is not applicable: {error_code}"),
            ))
        }
    }
}

fn validate_connector_success(
    output: agent_core::WriteConnectorExecuteOutput,
) -> CoreResult<agent_core::WriteConnectorExecuteOutput> {
    if output.status != "applied" {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "write connector accepted external action without applied status",
        ));
    }
    if output.result_ref.as_deref().is_none_or(str::is_empty) {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "write connector accepted external action without result_ref",
        ));
    }
    if output.compensation_ref.as_deref().is_none_or(str::is_empty) {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "write connector accepted external action without compensation_ref",
        ));
    }
    Ok(output)
}

async fn active_credential_lease(
    store: &StoreRef,
    credential_provider: &dyn CredentialProvider,
    plan: &ExternalActionPlan,
    trace_id: &str,
) -> CoreResult<CredentialLease> {
    let scope = plan.credential_scope.clone().ok_or_else(|| {
        AgentCoreError::coded(
            ErrorCode::Conflict,
            "credential_scope required for external-action apply",
        )
    })?;
    let lease = credential_provider
        .active_lease(CredentialLeaseRequest {
            external_action_plan_id: plan.id.clone(),
            credential_scope: scope.clone(),
            trace_id: trace_id.to_string(),
        })
        .await?;
    if lease.provider_ref.as_deref().is_none_or(str::is_empty) {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "active credential lease must include opaque provider_ref",
        ));
    }
    if lease.status != CredentialLeaseStatus::Active {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "credential provider did not issue an active lease",
        ));
    }
    if lease.external_action_plan_id != plan.id || lease.credential_scope != scope {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "active credential lease does not match external-action plan",
        ));
    }
    store.create_credential_lease(lease).await
}

async fn execute_with_retries(
    write_connector: &dyn WriteConnector,
    input: WriteConnectorExecuteInput,
    max_attempts: u32,
) -> CoreResult<agent_core::WriteConnectorExecuteOutput> {
    let mut last_error = None;
    for _ in 0..max_attempts.max(1) {
        match write_connector.execute(input.clone()).await {
            Ok(output) => return Ok(output),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AgentCoreError::coded(ErrorCode::InternalError, "write connector execution failed")
    }))
}

async fn dead_letter_external_action_run(
    store: &StoreRef,
    run: &AgentRun,
    trace_id: &str,
) -> CoreResult<Option<AgentRun>> {
    if matches!(
        run.run_status,
        AgentRunStatus::Completed | AgentRunStatus::DeadLetter | AgentRunStatus::Cancelled
    ) {
        return Ok(None);
    }
    let reason = format!("external-action connector dead-letter trace_id={trace_id}");
    store.dead_letter_run(&run.id, &reason).await.map(Some)
}

async fn fail_plan(
    store: &StoreRef,
    auth: &AuthContext,
    plan: &ExternalActionPlan,
    error_code: &str,
) -> CoreResult<ExternalActionPlan> {
    store
        .update_external_action_plan_status(
            &plan.id,
            ExternalActionPlanStatus::Failed,
            plan.result_ref.as_deref(),
            plan.compensation_ref.as_deref(),
            Some(error_code),
            &auth.trace_id,
        )
        .await
}

async fn append_apply_audit(
    store: &StoreRef,
    auth: &AuthContext,
    plan: &ExternalActionPlan,
    decision: AuditDecision,
    reason: Option<String>,
) {
    let mut audit = AuditLog::new(
        Some(auth),
        actions::ADMIN_EXTERNAL_ACTION_APPLY,
        decision,
        reason,
        auth.trace_id.clone(),
    );
    audit.run_id = Some(plan.run_id.clone());
    audit.approval_id = plan.approval_id.clone();
    audit.resource_type = Some("external_action_plan".to_string());
    audit.resource_id = Some(plan.id.clone());
    let _ = store.append_audit(audit).await;
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}
