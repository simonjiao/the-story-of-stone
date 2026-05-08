use agent_core::{PolicyDecision, metric_names};

pub(crate) fn record_request_metric(action: &str, service_id: &str) {
    metrics::counter!(
        metric_names::REQUEST_TOTAL,
        "action" => action.to_string(),
        "service" => service_id.to_string()
    )
    .increment(1);
}

pub(crate) fn record_policy_decision_metric(action: &str, decision: &PolicyDecision) {
    let decision_label = match decision {
        PolicyDecision::Allowed => "allowed",
        PolicyDecision::ApprovalRequired { .. } => "approval_required",
        PolicyDecision::Denied { .. } => "denied",
    };
    metrics::counter!(
        metric_names::POLICY_DECISION_TOTAL,
        "action" => action.to_string(),
        "decision" => decision_label
    )
    .increment(1);
}
