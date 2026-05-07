use agent_core::{
    AgentRunStatus, AuditDecision, AuditLog, CoreResult, EmptyResponse, HealthStatus,
    ObserverReport, ResourceLock, RiskLevel, RuntimeClient, RuntimeRunInput, SideEffectMode,
    metric_names, new_id,
};
use agent_runtime::MinimalRuntimeClient;
use agent_store::{AgentStore, MemoryAgentStore, PgAgentStore};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use time::OffsetDateTime;

pub type StoreRef = Arc<dyn AgentStore>;
pub type RuntimeRef = Arc<dyn RuntimeClient>;

#[derive(Clone)]
pub struct Worker {
    pub store: StoreRef,
    pub runtime: RuntimeRef,
    pub worker_id: String,
    pub lease: Duration,
    pub max_retries: i32,
}

impl Worker {
    pub fn new(store: StoreRef, runtime: RuntimeRef, worker_id: impl Into<String>) -> Self {
        Self {
            store,
            runtime,
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
        metrics::counter!(metric_names::RUN_CLAIM_TOTAL, "trigger_type" => claim.run.trigger_type.to_string()).increment(1);

        let result = self.execute_claim(claim.run.clone()).await;
        match result {
            Ok(summary) => {
                self.audit_run(
                    &run_id,
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
                        AuditDecision::Failed,
                        Some(error.to_string()),
                        &trace_id,
                    )
                    .await;
                } else {
                    metrics::counter!(metric_names::RUN_RETRY_TOTAL, "status" => "queued")
                        .increment(1);
                }
                Ok(Some(run_id))
            }
        }
    }

    async fn execute_claim(&self, mut run: agent_core::AgentRun) -> CoreResult<String> {
        self.store
            .update_run_status(&run.id, AgentRunStatus::ContextBuilt, &run.trace_id)
            .await?;
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

        self.store
            .update_run_status(&run.id, AgentRunStatus::PolicyChecked, &run.trace_id)
            .await?;

        let mut lock_held = false;
        if matches!(run.side_effect_mode, SideEffectMode::Authorized) {
            let resource = agent_core::ResourceRef::parse(run.target_resource.clone())?;
            self.store
                .acquire_resource_lock(
                    ResourceLock {
                        id: new_id("lock"),
                        resource_type: resource.resource_type,
                        resource_id: resource.resource_id,
                        lock_scope: "side_effect".to_string(),
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
        self.store
            .heartbeat_run(&run.id, &self.worker_id, self.lease)
            .await?;
        run = self
            .store
            .get_run(&run.id)
            .await?
            .ok_or_else(|| agent_store::store_error("run disappeared"))?;
        let output = self
            .runtime
            .execute_run(RuntimeRunInput {
                run: run.clone(),
                context,
                snapshot: None,
                trace_id: run.trace_id.clone(),
            })
            .await?;
        self.store
            .update_run_status(&run.id, AgentRunStatus::Validating, &run.trace_id)
            .await?;
        let summary = output.result_summary.clone();
        self.store.finish_run(&run.id, output).await?;
        if lock_held {
            self.store.release_resource_lock(&run.id).await?;
        }
        Ok(summary)
    }

    async fn audit_run(
        &self,
        run_id: &str,
        decision: AuditDecision,
        reason: Option<String>,
        trace_id: &str,
    ) {
        let mut audit = AuditLog::new(None, "worker:run", decision, reason, trace_id.to_string());
        audit.run_id = Some(run_id.to_string());
        let _ = self.store.append_audit(audit).await;
    }
}

pub async fn observer_tick(store: StoreRef, trace_id: &str) -> CoreResult<ObserverReport> {
    let snapshot = store.collect_observer_snapshot(trace_id).await?;
    let dead_letters = snapshot
        .run_counts
        .get("dead_letter")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let health = if dead_letters > 0 {
        HealthStatus::Degraded
    } else {
        HealthStatus::Healthy
    };
    let report = ObserverReport::new(
        new_id("observer_run"),
        health,
        if dead_letters > 0 {
            Some(RiskLevel::Medium)
        } else {
            Some(RiskLevel::Low)
        },
        format!("Observer generated read-only P0 report; dead_letter={dead_letters}."),
        json!({
            "run_counts": snapshot.run_counts,
            "agent_counts": snapshot.agent_counts,
            "session_counts": snapshot.session_counts,
        }),
        json!([
            {
                "recommended_priority": if dead_letters > 0 { "medium" } else { "low" },
                "recommendation": "Inspect audit and worker heartbeat summaries before any control-plane action."
            }
        ]),
        json!({
            "lock_summary": snapshot.lock_summary,
            "audit_summary": snapshot.audit_summary,
            "worker_summary": snapshot.worker_summary,
        }),
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
    use agent_core::{AgentInstance, AgentRun, TriggerType, new_trace_id};
    use serde_json::json;

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
}
