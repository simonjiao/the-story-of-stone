use crate::{StoreRef, control_services};
use agent_core::{
    AGENT_TYPE_OBSERVER, AgentCoreError, AgentInstance, AgentSession, AgentSessionMessage,
    AuditDecision, AuditLog, AuthContext, CoreResult, ErrorCode, MessageRole, ObserverReport,
    ObserverReportDiscussionInput, ObserverReportDiscussionResponse, SystemStatusSessionInput,
    SystemStatusSessionResponse, actions,
};
use serde_json::{Value, json};

const SYSTEM_OBSERVER_TARGET_RESOURCE: &str = "resource:system/agent-platform";
const SYSTEM_OBSERVER_CONSTRAINTS_HASH: &str = "system-observer-status-v1";
const SYSTEM_OBSERVER_SESSION_KEY: &str = "system-observer:status-session";

pub(crate) async fn create_report_discussion(
    store: &StoreRef,
    auth: &AuthContext,
    report_id: String,
    input: ObserverReportDiscussionInput,
) -> CoreResult<ObserverReportDiscussionResponse> {
    if input.initial_message.trim().is_empty() {
        return Err(AgentCoreError::coded(
            ErrorCode::Conflict,
            "initial_message is required",
        ));
    }
    let report = store
        .get_observer_report(&report_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;
    let agent = store
        .get_agent(&input.agent_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))?;

    let session = if let Some(key) = &input.idempotency_key {
        store
            .find_session_by_idempotency(&auth.user_id, &agent.id, key)
            .await?
    } else {
        None
    };
    let session = if let Some(existing) = session {
        existing
    } else {
        let mut session = AgentSession::new(
            agent.id.clone(),
            auth.user_id.clone(),
            json!({
                "observer_report_id": report.id.clone(),
                "target_agent_id": agent.id.clone(),
                "target_resource": agent.target_resource.clone(),
                "evidence_refs": redacted_evidence_refs(&report.evidence_refs),
            }),
            auth.trace_id.clone(),
        );
        session.idempotency_key = input.idempotency_key.clone();
        session.source_conversation_id = Some(format!("observer_report:{}", report.id));
        session.context_summary = Some(redacted_report_context(&report.summary));
        store.create_session(session).await?
    };

    let sequence = store.next_message_sequence(&session.id).await?;
    let mut message = AgentSessionMessage::new(
        session.id.clone(),
        sequence,
        MessageRole::User,
        Some(redacted_initial_message(
            &report_id,
            &report.summary,
            &input.initial_message,
        )),
        None,
        auth.trace_id.clone(),
    );
    message.external_message_id = Some(format!(
        "observer-report:{}:{}",
        report_id,
        input
            .idempotency_key
            .as_deref()
            .unwrap_or("initial-message")
    ));
    let first_message = store.append_message(message).await?;

    let mut audit = AuditLog::new(
        Some(auth),
        actions::ADMIN_OBSERVER_DISCUSS,
        AuditDecision::Allowed,
        Some(format!(
            "report_id={} session_id={} agent_id={}",
            report_id, session.id, agent.id
        )),
        auth.trace_id.clone(),
    );
    audit.observer_report_id = Some(report_id.clone());
    audit.session_id = Some(session.id.clone());
    audit.resource_type = Some("agent_session".to_string());
    audit.resource_id = Some(session.id.clone());
    let _ = store.append_audit(audit).await;
    control_services::append_audit(
        store,
        Some(auth),
        actions::SESSION_CREATE,
        AuditDecision::Allowed,
        Some(format!("observer_report_id={report_id}")),
        &auth.trace_id,
    )
    .await;

    Ok(ObserverReportDiscussionResponse {
        report_id,
        session,
        first_message,
        trace_id: auth.trace_id.clone(),
    })
}

pub(crate) async fn create_system_status_session(
    store: &StoreRef,
    auth: &AuthContext,
    input: SystemStatusSessionInput,
) -> CoreResult<SystemStatusSessionResponse> {
    let report = load_requested_or_latest_report(store, input.report_id.as_deref()).await?;
    let agent = ensure_system_observer_agent(store, auth).await?;
    let idempotency_key = input
        .idempotency_key
        .clone()
        .unwrap_or_else(|| SYSTEM_OBSERVER_SESSION_KEY.to_string());
    let session = if let Some(existing) = store
        .find_session_by_idempotency(&auth.user_id, &agent.id, &idempotency_key)
        .await?
    {
        existing
    } else {
        let mut session = AgentSession::new(
            agent.id.clone(),
            auth.user_id.clone(),
            json!({
                "system_status": true,
                "observer_report_id": report.id.clone(),
                "target_resource": SYSTEM_OBSERVER_TARGET_RESOURCE,
                "health_status": report.health_status,
                "risk_level": report.risk_level,
                "findings": report.findings,
                "recommendations": report.recommendations,
                "evidence_refs": redacted_evidence_refs(&report.evidence_refs),
            }),
            auth.trace_id.clone(),
        );
        session.idempotency_key = Some(idempotency_key.clone());
        session.source_conversation_id = Some("system_observer:status".to_string());
        session.context_summary = Some(system_status_context_summary(&report));
        store.create_session(session).await?
    };

    let report_message = append_system_status_message(store, auth, &session, &report).await?;
    let first_message =
        append_operator_question(store, auth, &session, &report, input.initial_message).await?;

    let mut audit = AuditLog::new(
        Some(auth),
        actions::ADMIN_OBSERVER_DISCUSS,
        AuditDecision::Allowed,
        Some(format!(
            "system_status report_id={} session_id={} agent_id={}",
            report.id, session.id, agent.id
        )),
        auth.trace_id.clone(),
    );
    audit.observer_report_id = Some(report.id.clone());
    audit.session_id = Some(session.id.clone());
    audit.resource_type = Some("agent_session".to_string());
    audit.resource_id = Some(session.id.clone());
    let _ = store.append_audit(audit).await;
    control_services::append_audit(
        store,
        Some(auth),
        actions::SESSION_CREATE,
        AuditDecision::Allowed,
        Some(format!("system_status observer_report_id={}", report.id)),
        &auth.trace_id,
    )
    .await;

    Ok(SystemStatusSessionResponse {
        report_id: report.id,
        agent,
        session,
        report_message,
        first_message,
        trace_id: auth.trace_id.clone(),
    })
}

fn redacted_report_context(summary: &str) -> String {
    format!(
        "Observer report discussion context: {}",
        truncate(summary, 600)
    )
}

fn redacted_initial_message(report_id: &str, report_summary: &str, user_message: &str) -> String {
    format!(
        "Discuss observer_report={report_id}. Report summary: {}. User message: {}",
        truncate(report_summary, 600),
        truncate(user_message, 1000)
    )
}

fn redacted_evidence_refs(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, value) in map {
                if key.contains("credential") || key.contains("secret") || key.contains("prompt") {
                    redacted.insert(key.clone(), json!("redacted"));
                } else {
                    redacted.insert(key.clone(), redacted_evidence_refs(value));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(items) => Value::Array(items.iter().map(redacted_evidence_refs).collect()),
        _ => value.clone(),
    }
}

async fn load_requested_or_latest_report(
    store: &StoreRef,
    report_id: Option<&str>,
) -> CoreResult<ObserverReport> {
    if let Some(report_id) = report_id.filter(|value| !value.trim().is_empty()) {
        return store
            .get_observer_report(report_id)
            .await?
            .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"));
    }
    let latest = store
        .list_observer_reports(1)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "no observer reports"))?;
    store
        .get_observer_report(&latest.report_id)
        .await?
        .ok_or_else(|| AgentCoreError::coded(ErrorCode::NotFound, "not found"))
}

async fn ensure_system_observer_agent(
    store: &StoreRef,
    auth: &AuthContext,
) -> CoreResult<AgentInstance> {
    if let Some(existing) = store
        .find_reusable_agent(
            &auth.user_id,
            AGENT_TYPE_OBSERVER,
            SYSTEM_OBSERVER_TARGET_RESOURCE,
            SYSTEM_OBSERVER_CONSTRAINTS_HASH,
        )
        .await?
    {
        return Ok(existing);
    }
    let mut agent = AgentInstance::new(
        auth.user_id.clone(),
        AGENT_TYPE_OBSERVER,
        SYSTEM_OBSERVER_TARGET_RESOURCE,
        SYSTEM_OBSERVER_CONSTRAINTS_HASH,
        json!({
            "mode": "system_status",
            "hermes_profile": "observer_agent:system-status",
            "external_action_mode": "deny",
            "readable_scopes": [
                "observer_reports",
                "audit_summary",
                "worker_heartbeat_summary",
                "run_quality_signals",
                "resource_lock_summary",
                "external_action_plan_summary"
            ],
            "forbidden_scopes": [
                "secrets",
                "credentials",
                "full_prompt",
                "raw_internal_logs"
            ]
        }),
        auth.trace_id.clone(),
    );
    agent.display_name = Some("System Observer".to_string());
    store.create_agent_instance(agent).await
}

async fn append_system_status_message(
    store: &StoreRef,
    auth: &AuthContext,
    session: &AgentSession,
    report: &ObserverReport,
) -> CoreResult<AgentSessionMessage> {
    let sequence = store.next_message_sequence(&session.id).await?;
    let mut message = AgentSessionMessage::new(
        session.id.clone(),
        sequence,
        MessageRole::System,
        Some(system_status_context(report)),
        None,
        auth.trace_id.clone(),
    );
    message.external_message_id = Some(format!("system-observer:report:{}", report.id));
    store.append_message(message).await
}

async fn append_operator_question(
    store: &StoreRef,
    auth: &AuthContext,
    session: &AgentSession,
    report: &ObserverReport,
    initial_message: Option<String>,
) -> CoreResult<AgentSessionMessage> {
    let prompt = initial_message
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "请基于最新 Observer report 总结系统状态，列出风险、证据、关键指标和下一步排查路径。"
                .to_string()
        });
    let sequence = store.next_message_sequence(&session.id).await?;
    let mut message = AgentSessionMessage::new(
        session.id.clone(),
        sequence,
        MessageRole::User,
        Some(format!(
            "Discuss system status from observer_report={}. User message: {}",
            report.id,
            truncate(&prompt, 1000)
        )),
        None,
        auth.trace_id.clone(),
    );
    message.external_message_id = Some(format!("system-observer:question:{}", report.id));
    store.append_message(message).await
}

fn system_status_context_summary(report: &ObserverReport) -> String {
    format!(
        "System Observer context report_id={} health={} risk={} summary={}",
        report.id,
        report.health_status,
        report
            .risk_level
            .map(|risk| risk.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        truncate(&report.summary, 400)
    )
}

fn system_status_context(report: &ObserverReport) -> String {
    format!(
        "System Observer report packet\nreport_id: {}\nhealth: {}\nrisk: {}\nsummary: {}\nfindings: {}\nrecommendations: {}\nevidence_refs: {}",
        report.id,
        report.health_status,
        report
            .risk_level
            .map(|risk| risk.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        truncate(&report.summary, 1200),
        truncate_json(&report.findings, 3000),
        truncate_json(&report.recommendations, 2000),
        truncate_json(&redacted_evidence_refs(&report.evidence_refs), 3000)
    )
}

fn truncate_json(value: &Value, max_chars: usize) -> String {
    serde_json::to_string(value)
        .map(|value| truncate(&value, max_chars))
        .unwrap_or_else(|_| "null".to_string())
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
