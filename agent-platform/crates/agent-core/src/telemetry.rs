use crate::new_trace_id;
use serde::{Deserialize, Serialize};

pub mod metric_names {
    pub const REQUEST_TOTAL: &str = "agent_platform_request_total";
    pub const REQUEST_DURATION_SECONDS: &str = "agent_platform_request_duration_seconds";
    pub const POLICY_DECISION_TOTAL: &str = "agent_platform_policy_decision_total";
    pub const RUN_CLAIM_TOTAL: &str = "agent_platform_run_claim_total";
    pub const RUN_HEARTBEAT_TOTAL: &str = "agent_platform_run_heartbeat_total";
    pub const RUN_RETRY_TOTAL: &str = "agent_platform_run_retry_total";
    pub const RUN_DEAD_LETTER_TOTAL: &str = "agent_platform_run_dead_letter_total";
    pub const RUNTIME_CALL_TOTAL: &str = "agent_platform_runtime_call_total";
    pub const RUNTIME_DURATION_SECONDS: &str = "agent_platform_runtime_duration_seconds";
    pub const RUNTIME_TIMEOUT_TOTAL: &str = "agent_platform_runtime_timeout_total";
    pub const CONNECTOR_SNAPSHOT_TOTAL: &str = "agent_platform_connector_snapshot_total";
    pub const EXTERNAL_ACTION_DRY_RUN_TOTAL: &str = "agent_platform_external_action_dry_run_total";
    pub const EXTERNAL_ACTION_APPLY_TOTAL: &str = "agent_platform_external_action_apply_total";
    pub const EXTERNAL_ACTION_COMPENSATE_TOTAL: &str =
        "agent_platform_external_action_compensate_total";
    pub const LOCK_WAIT_SECONDS: &str = "agent_platform_lock_wait_seconds";
    pub const OBSERVER_REPORT_TOTAL: &str = "agent_platform_observer_report_total";
}

pub mod labels {
    pub const SERVICE: &str = "service";
    pub const ACTION: &str = "action";
    pub const DECISION: &str = "decision";
    pub const STATUS: &str = "status";
    pub const AGENT_TYPE: &str = "agent_type";
    pub const RESOURCE_TYPE: &str = "resource_type";
    pub const TRIGGER_TYPE: &str = "trigger_type";
    pub const ERROR_CODE: &str = "error_code";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
}

impl TraceContext {
    pub fn new() -> Self {
        Self {
            trace_id: new_trace_id(),
        }
    }

    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
        }
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NoopTelemetry;

impl NoopTelemetry {
    pub fn counter(&self, name: &'static str, labels: &[(&'static str, &str)]) {
        let label_values: Vec<(&'static str, String)> = labels
            .iter()
            .map(|(key, value)| (*key, (*value).to_string()))
            .collect();
        metrics::counter!(name, &label_values).increment(1);
    }

    pub fn histogram(&self, name: &'static str, value: f64, labels: &[(&'static str, &str)]) {
        let label_values: Vec<(&'static str, String)> = labels
            .iter()
            .map(|(key, value)| (*key, (*value).to_string()))
            .collect();
        metrics::histogram!(name, &label_values).record(value);
    }
}
