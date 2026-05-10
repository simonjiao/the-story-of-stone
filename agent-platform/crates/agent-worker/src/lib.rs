use agent_core::{
    AgentInstance, AgentRunStatus, AgentSessionMessage, AuditDecision, AuditLog, ConnectorClient,
    CoreResult, EmptyResponse, ExternalActionMode, MessageRole, ObserverReport, ProfileContract,
    ResourceLock, RuntimeClient, RuntimeOutput, RuntimeRunInput, RuntimeSessionInput, RuntimeStep,
    TriggerType, assess_observer_snapshot, metric_names, new_id,
};
use agent_runtime::{
    HermesRuntimeClient, HttpReadOnlyConnector, HttpReadOnlyConnectorConfig,
    LocalReadOnlyConnector, MinimalRuntimeClient,
};
use agent_store::{AgentStore, MemoryAgentStore, PgAgentStore};
use std::{sync::Arc, time::Duration};
use time::OffsetDateTime;

pub type StoreRef = Arc<dyn AgentStore>;
pub type RuntimeRef = Arc<dyn RuntimeClient>;
pub type ConnectorRef = Arc<dyn ConnectorClient>;

#[derive(Clone)]
pub struct Worker {
    pub store: StoreRef,
    pub runtime: RuntimeRef,
    pub connector: ConnectorRef,
    pub worker_id: String,
    pub lease: Duration,
    pub max_retries: i32,
}

impl Worker {
    pub fn new(store: StoreRef, runtime: RuntimeRef, worker_id: impl Into<String>) -> Self {
        Self::with_connector(store, runtime, Arc::new(LocalReadOnlyConnector), worker_id)
    }

    pub fn with_connector(
        store: StoreRef,
        runtime: RuntimeRef,
        connector: ConnectorRef,
        worker_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            runtime,
            connector,
            worker_id: worker_id.into(),
            lease: Duration::from_secs(30),
            max_retries: 3,
        }
    }

    pub async fn tick(&self) -> CoreResult<Option<String>> {
        let swept = self.store.sweep_expired_leases(self.max_retries).await?;
        for run in swept {
            metrics::counter!(metric_names::RUN_RETRY_TOTAL, "status" => run.run_status.to_string()).increment(1);
        }

        let Some(claim) = self
            .store
            .claim_next_run(&self.worker_id, self.lease)
            .await?
        else {
            self.store
                .record_worker_heartbeat(&self.worker_id, None, "idle", &agent_core::new_trace_id())
                .await?;
            return Ok(None);
        };

        let run_id = claim.run.id.clone();
        let trace_id = claim.run.trace_id.clone();
        self.store
            .record_worker_heartbeat(&self.worker_id, Some(&run_id), "claimed", &trace_id)
            .await?;
        self.audit_run(
            &run_id,
            "worker:run_claim",
            AuditDecision::Allowed,
            Some("status=claimed".to_string()),
            &trace_id,
        )
        .await;
        metrics::counter!(metric_names::RUN_CLAIM_TOTAL, "trigger_type" => claim.run.trigger_type.to_string()).increment(1);

        let result = self.execute_claim(claim.run.clone()).await;
        match result {
            Ok(summary) => {
                self.audit_run(
                    &run_id,
                    "worker:run_finish",
                    AuditDecision::Completed,
                    Some(summary.clone()),
                    &trace_id,
                )
                .await;
                Ok(Some(run_id))
            }
            Err(error) => {
                let updated = self
                    .store
                    .fail_or_retry_run(&run_id, &error.to_string(), self.max_retries)
                    .await?;
                if updated.run_status == AgentRunStatus::DeadLetter {
                    metrics::counter!(metric_names::RUN_DEAD_LETTER_TOTAL, "status" => "dead_letter").increment(1);
                    self.audit_run(
                        &run_id,
                        "worker:run_dead_letter",
                        AuditDecision::Failed,
                        Some(error.to_string()),
                        &trace_id,
                    )
                    .await;
                } else {
                    metrics::counter!(metric_names::RUN_RETRY_TOTAL, "status" => "queued")
                        .increment(1);
                    self.audit_run(
                        &run_id,
                        "worker:run_retry",
                        AuditDecision::Allowed,
                        Some(format!("status={}", updated.run_status)),
                        &trace_id,
                    )
                    .await;
                }
                Ok(Some(run_id))
            }
        }
    }

    async fn execute_claim(&self, mut run: agent_core::AgentRun) -> CoreResult<String> {
        self.store
            .update_run_status(&run.id, AgentRunStatus::ContextBuilt, &run.trace_id)
            .await?;
        self.audit_run_status(&run.id, AgentRunStatus::ContextBuilt, &run.trace_id)
            .await;
        self.store
            .heartbeat_run(&run.id, &self.worker_id, self.lease)
            .await?;
        let context = if let Some(session_id) = &run.session_id {
            Some(
                self.store
                    .session_context(session_id, &run.trace_id)
                    .await?,
            )
        } else {
            None
        };
        let agent = self
            .store
            .get_agent(&run.agent_id)
            .await?
            .ok_or_else(|| agent_store::store_error("agent not found"))?;

        self.store
            .update_run_status(&run.id, AgentRunStatus::PolicyChecked, &run.trace_id)
            .await?;
        self.audit_run_status(&run.id, AgentRunStatus::PolicyChecked, &run.trace_id)
            .await;

        let mut lock_held = false;
        if matches!(run.external_action_mode, ExternalActionMode::Authorized) {
            let resource = agent_core::ResourceRef::parse(run.target_resource.clone())?;
            self.store
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
                    self.lease,
                )
                .await?;
            lock_held = true;
        }

        self.store
            .update_run_status(&run.id, AgentRunStatus::Executing, &run.trace_id)
            .await?;
        self.audit_run_status(&run.id, AgentRunStatus::Executing, &run.trace_id)
            .await;
        self.store
            .heartbeat_run(&run.id, &self.worker_id, self.lease)
            .await?;
        run = self
            .store
            .get_run(&run.id)
            .await?
            .ok_or_else(|| agent_store::store_error("run disappeared"))?;
        let snapshot = self
            .connector
            .read_only_snapshot(connector_name(&agent), &run.target_resource, &run.trace_id)
            .await?
            .summary;
        let profile_contract = profile_contract_from_agent(&agent)?;
        let requested_tools = requested_tools_from_agent(&agent);
        let runtime_step = Some(RuntimeStep::new(
            profile_contract
                .as_ref()
                .map(|contract| contract.profile_id.as_str())
                .unwrap_or(agent.hermes_profile.as_str()),
            profile_contract
                .as_ref()
                .map(|contract| contract.version.version.as_str())
                .unwrap_or("unversioned"),
            serde_json::json!({
                "run_id": &run.id,
                "worker_id": &self.worker_id,
                "trigger_type": run.trigger_type,
            }),
        ));
        let output = if matches!(run.trigger_type, TriggerType::SessionMessage) {
            let context =
                context.ok_or_else(|| agent_store::store_error("session context missing"))?;
            let message = latest_user_message(&context)?;
            self.runtime
                .send_session_message(RuntimeSessionInput {
                    session_id: context.session_id.clone(),
                    agent_id: agent.id.clone(),
                    agent: Some(agent.clone()),
                    message,
                    context,
                    snapshot: Some(snapshot),
                    profile_contract,
                    runtime_step,
                    requested_tools,
                    trace_id: run.trace_id.clone(),
                })
                .await?
        } else {
            self.runtime
                .execute_run(RuntimeRunInput {
                    run: run.clone(),
                    agent: Some(agent.clone()),
                    context,
                    snapshot: Some(snapshot),
                    profile_contract,
                    runtime_step,
                    requested_tools,
                    trace_id: run.trace_id.clone(),
                })
                .await?
        };
        self.store
            .update_run_status(&run.id, AgentRunStatus::Validating, &run.trace_id)
            .await?;
        self.audit_run_status(&run.id, AgentRunStatus::Validating, &run.trace_id)
            .await;
        let summary = output.result_summary.clone();
        self.append_runtime_messages(&run, &output).await?;
        self.store.finish_run(&run.id, output).await?;
        if lock_held {
            self.store.release_resource_lock(&run.id).await?;
        }
        Ok(summary)
    }

    async fn append_runtime_messages(
        &self,
        run: &agent_core::AgentRun,
        output: &RuntimeOutput,
    ) -> CoreResult<()> {
        let Some(session_id) = run.session_id.as_deref() else {
            return Ok(());
        };
        let mut messages = if output.messages.is_empty() {
            let mut message = AgentSessionMessage::new(
                session_id,
                0,
                MessageRole::Assistant,
                Some(output.result_summary.clone()),
                Some(run.id.clone()),
                run.trace_id.clone(),
            );
            message.content_ref = output.result_ref.clone();
            vec![message]
        } else {
            output.messages.clone()
        };
        for mut message in messages.drain(..) {
            message.session_id = session_id.to_string();
            message.sequence = self.store.next_message_sequence(session_id).await?;
            message.run_id = Some(run.id.clone());
            message.trace_id = run.trace_id.clone();
            self.store.append_message(message).await?;
        }
        Ok(())
    }

    async fn audit_run_status(&self, run_id: &str, status: AgentRunStatus, trace_id: &str) {
        self.audit_run(
            run_id,
            "worker:run_status",
            AuditDecision::Completed,
            Some(format!("status={status}")),
            trace_id,
        )
        .await;
    }

    async fn audit_run(
        &self,
        run_id: &str,
        action: &str,
        decision: AuditDecision,
        reason: Option<String>,
        trace_id: &str,
    ) {
        let mut audit = AuditLog::new(None, action, decision, reason, trace_id.to_string());
        audit.run_id = Some(run_id.to_string());
        let _ = self.store.append_audit(audit).await;
    }
}

pub async fn observer_tick(store: StoreRef, trace_id: &str) -> CoreResult<ObserverReport> {
    let snapshot = store.collect_observer_snapshot(trace_id).await?;
    let assessment = assess_observer_snapshot(&snapshot);
    let report = ObserverReport::new(
        new_id("observer_run"),
        assessment.health_status,
        Some(assessment.risk_level),
        assessment.summary,
        assessment.findings,
        assessment.recommendations,
        assessment.evidence_refs,
        trace_id.to_string(),
    );
    let report = store.create_observer_report(report).await?;
    let mut audit = AuditLog::new(
        None,
        "observer:tick",
        AuditDecision::Completed,
        None,
        trace_id.to_string(),
    );
    audit.observer_report_id = Some(report.id.clone());
    let _ = store.append_audit(audit).await;
    metrics::counter!(metric_names::OBSERVER_REPORT_TOTAL, "health_status" => report.health_status.to_string()).increment(1);
    Ok(report)
}

fn latest_user_message(context: &agent_core::SessionContext) -> CoreResult<AgentSessionMessage> {
    let recent = context
        .recent_messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
        .cloned()
        .ok_or_else(|| agent_store::store_error("session message missing"))?;
    let mut message = AgentSessionMessage::new(
        context.session_id.clone(),
        0,
        recent.role,
        Some(recent.content_summary),
        recent.run_id,
        context.trace_id.clone(),
    );
    message.content_ref = recent.content_ref;
    message.external_message_id = recent.external_message_id;
    Ok(message)
}

pub async fn store_from_env() -> CoreResult<StoreRef> {
    if let Ok(database_url) = std::env::var("DATABASE_URL") {
        let pg = PgAgentStore::connect(&database_url, 10).await?;
        pg.bootstrap().await?;
        Ok(Arc::new(pg))
    } else {
        let memory = MemoryAgentStore::new();
        memory.bootstrap().await?;
        Ok(Arc::new(memory))
    }
}

pub fn minimal_runtime() -> RuntimeRef {
    Arc::new(MinimalRuntimeClient::default())
}

pub fn runtime_from_env() -> CoreResult<RuntimeRef> {
    match std::env::var("AGENT_RUNTIME_MODE")
        .unwrap_or_else(|_| "minimal".to_string())
        .as_str()
    {
        "minimal" => Ok(minimal_runtime()),
        "hermes" => Ok(Arc::new(HermesRuntimeClient::from_env()?)),
        other => Err(agent_core::AgentCoreError::coded(
            agent_core::ErrorCode::Conflict,
            format!("unsupported AGENT_RUNTIME_MODE={other}"),
        )),
    }
}

pub fn connector_from_env() -> CoreResult<ConnectorRef> {
    if let Some(config) = HttpReadOnlyConnectorConfig::from_env() {
        Ok(Arc::new(HttpReadOnlyConnector::new(config)?))
    } else {
        Ok(Arc::new(LocalReadOnlyConnector))
    }
}

fn connector_name(agent: &agent_core::AgentInstance) -> &str {
    agent
        .config
        .get("read_only_connector")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("local")
}

fn profile_contract_from_agent(agent: &AgentInstance) -> CoreResult<Option<ProfileContract>> {
    let Some(value) = agent
        .config
        .pointer("/runtime/profile_contract")
        .or_else(|| agent.config.get("profile_contract"))
    else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| agent_store::store_error("agent profile contract is malformed"))
}

fn requested_tools_from_agent(agent: &AgentInstance) -> Vec<String> {
    agent
        .config
        .pointer("/runtime/requested_tools")
        .or_else(|| agent.config.get("requested_tools"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|tool| !tool.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub async fn idle_heartbeat(
    store: StoreRef,
    worker_id: &str,
    trace_id: &str,
) -> CoreResult<EmptyResponse> {
    store
        .record_worker_heartbeat(worker_id, None, "idle", trace_id)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        AgentInstance, AgentRun, AgentSession, AgentSessionMessage, ConnectorSnapshot, MemoryStore,
        MessageRole, RuntimeSessionInput, TriggerType, new_trace_id,
    };
    use async_trait::async_trait;
    use serde_json::json;

    #[derive(Debug)]
    struct RecordingRuntime;

    #[async_trait]
    impl RuntimeClient for RecordingRuntime {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            unreachable!("session message runs use send_session_message")
        }

        async fn send_session_message(
            &self,
            input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            assert!(input.agent.is_some());
            assert_eq!(input.snapshot.as_ref().unwrap()["mode"], "read_only");
            assert_eq!(
                input.message.content_summary.as_deref(),
                Some("please analyze this")
            );
            Ok(RuntimeOutput {
                result_summary: "runtime assistant response".to_string(),
                result_ref: Some("result://recording/session".to_string()),
                messages: Vec::new(),
                metadata: json!({"trace_id": input.trace_id}),
            })
        }
    }

    #[derive(Debug)]
    struct RecordingConnector;

    #[async_trait]
    impl ConnectorClient for RecordingConnector {
        async fn read_only_snapshot(
            &self,
            connector: &str,
            resource: &str,
            trace_id: &str,
        ) -> CoreResult<ConnectorSnapshot> {
            assert_eq!(connector, "local");
            assert_eq!(resource, "resource:team/project-alpha");
            Ok(ConnectorSnapshot {
                connector: connector.to_string(),
                resource: resource.to_string(),
                payload_ref: "snapshot://recording".to_string(),
                summary: json!({
                    "mode": "read_only",
                    "trace_id": trace_id,
                }),
            })
        }
    }

    #[tokio::test]
    async fn worker_claims_and_completes_run() {
        let store = MemoryAgentStore::new();
        store.bootstrap().await.unwrap();
        let agent = AgentInstance::new(
            "user-1",
            "background_worker",
            "resource:team/project-alpha",
            "hash",
            json!({}),
            new_trace_id(),
        );
        let agent = store.create_agent_instance(agent).await.unwrap();
        let run = AgentRun::new(
            agent.id,
            None,
            TriggerType::Manual,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        let run_id = run.id.clone();
        store.create_run(run).await.unwrap();
        let worker = Worker::new(Arc::new(store.clone()), minimal_runtime(), "worker-test");
        worker.tick().await.unwrap();
        let completed = store.get_run(&run_id).await.unwrap().unwrap();
        assert_eq!(completed.run_status, AgentRunStatus::Completed);
    }

    #[tokio::test]
    async fn worker_appends_runtime_output_to_session_with_snapshot() {
        let store = MemoryAgentStore::new();
        store.bootstrap().await.unwrap();
        let agent = store
            .create_agent_instance(AgentInstance::new(
                "user-1",
                "background_worker",
                "resource:team/project-alpha",
                "hash",
                json!({}),
                new_trace_id(),
            ))
            .await
            .unwrap();
        let session = store
            .create_session(AgentSession::new(
                agent.id.clone(),
                "user-1",
                json!({"resource": "resource:team/project-alpha"}),
                new_trace_id(),
            ))
            .await
            .unwrap();
        let user_message = AgentSessionMessage::new(
            session.id.clone(),
            1,
            MessageRole::User,
            Some("please analyze this".to_string()),
            None,
            new_trace_id(),
        );
        store.append_message(user_message).await.unwrap();
        let run = AgentRun::new(
            agent.id,
            Some(session.id.clone()),
            TriggerType::SessionMessage,
            "resource:team/project-alpha",
            new_trace_id(),
        );
        let run_id = run.id.clone();
        store.create_run(run).await.unwrap();
        let worker = Worker::with_connector(
            Arc::new(store.clone()),
            Arc::new(RecordingRuntime),
            Arc::new(RecordingConnector),
            "worker-test",
        );

        worker.tick().await.unwrap();

        let completed = store.get_run(&run_id).await.unwrap().unwrap();
        assert_eq!(completed.run_status, AgentRunStatus::Completed);
        let context = store
            .session_context(&session.id, &new_trace_id())
            .await
            .unwrap();
        assert!(context.recent_messages.iter().any(|message| {
            message.role == MessageRole::Assistant
                && message.content_summary == "runtime assistant response"
                && message.run_id.as_deref() == Some(run_id.as_str())
                && message.content_ref.as_deref() == Some("result://recording/session")
        }));
    }
}
