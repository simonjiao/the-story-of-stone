use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tonglingyu_runtime::{EvidencePackage, RuntimeWorkflowStreamEvent};

use crate::DEFAULT_MODEL_ID;

const PUBLIC_OUTPUT_FORBIDDEN_KNOWLEDGE_STATE_TERMS: &[&str] = &[
    "system_calibrated",
    "runtime_usable",
    "human_marked",
    "knowledge_item_ref",
    "knowledge_item_refs",
    "calibration_report_ref",
    "runtime_policy",
    "policy_version",
    "state_version",
    "release_run_id",
];

pub(crate) fn completion_value(
    model: &str,
    content: String,
    package: Option<&EvidencePackage>,
    session_id: Option<&str>,
) -> Value {
    let mut value = json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::now_v7().simple()),
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }]
    });
    if let Some(package) = package {
        value["trace_id"] = json!(&package.trace_id);
        value["evidence_package_id"] = json!(&package.package_id);
        value["review"] = json!(&package.review);
    }
    if let Some(session_id) = session_id {
        value["session_id"] = json!(session_id);
    }
    value
}

pub(crate) fn cache_completion_value(
    value: &Value,
    events: &[RuntimeWorkflowStreamEvent],
) -> Value {
    let mut cached = value.clone();
    if let Value::Object(map) = &mut cached {
        map.insert("_runtime_stream_events".to_string(), json!(events));
        map.insert("_stream_source".to_string(), json!("runtime_workflow"));
    }
    cached
}

pub(crate) fn public_completion_value(value: &Value) -> Value {
    let mut public = value.clone();
    if let Value::Object(map) = &mut public {
        map.remove("_runtime_stream_events");
        map.remove("_stream_source");
        map.remove("trace_id");
        map.remove("evidence_package_id");
        map.remove("review");
        map.remove("session_id");
        map.remove("user_session_id");
        map.remove("interaction_context_id");
        map.remove("context_pack_id");
        map.remove("context_pack_ref");
        map.remove("context_projection_id");
        map.remove("context_projection_ref");
        map.remove("context_projections");
        map.remove("session_journal");
        map.remove("context_pack");
        map.remove("memory_read_refs");
        map.remove("memory_read_ref_digest");
        map.remove("memory_read_policy_digest");
        map.remove("memory_summaries");
        map.remove("memory_policy");
        map.remove("memory_policy_digest");
        map.remove("memory_usage_summary");
        map.remove("memory_candidate");
        map.remove("memory_candidate_id");
        map.remove("memory_candidate_ref");
        map.remove("memory_candidates");
        map.remove("memory_card");
        map.remove("memory_card_id");
        map.remove("memory_card_ref");
        map.remove("memory_cards");
        map.remove("memory_policy_decision");
        map.remove("memory_policy_decision_id");
        map.remove("memory_policy_decision_ref");
        map.remove("memory_policy_decisions");
        map.remove("memory_transition_audit");
        map.remove("llm_extraction");
        map.remove("llm_filter");
        map.remove("rule_filter");
        map.remove("read_enabled");
    }
    if let Some(content) = public
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(public_answer_content)
    {
        public["choices"][0]["message"]["content"] = json!(content);
    }
    public
}

fn public_answer_content(content: &str) -> String {
    if contains_public_forbidden_knowledge_state_term(content) {
        "当前回答未通过公开输出检查，不能直接返回。请基于可追溯证据重新提问。".to_string()
    } else {
        content.to_string()
    }
}

fn contains_public_forbidden_knowledge_state_term(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    PUBLIC_OUTPUT_FORBIDDEN_KNOWLEDGE_STATE_TERMS
        .iter()
        .any(|term| lower.contains(term))
}

pub(crate) fn cached_runtime_stream_events(
    value: &Value,
) -> Option<Vec<RuntimeWorkflowStreamEvent>> {
    serde_json::from_value::<Vec<RuntimeWorkflowStreamEvent>>(
        value.get("_runtime_stream_events")?.clone(),
    )
    .ok()
    .filter(|events| {
        events
            .iter()
            .any(|event| event.event_type == "content_delta")
    })
}

pub(crate) fn streaming_response_from_completion_value(value: &Value) -> Response {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_MODEL_ID);
    let content = public_answer_content(
        value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let completion_id = format!("chatcmpl-{}", uuid::Uuid::now_v7().simple());
    let mut chunks = Vec::new();
    chunks.push(format!(
        "data: {}\n\n",
        json!({
            "id": &completion_id,
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant"},
                "finish_reason": null
            }]
        })
    ));
    for piece in text_stream_chunks(&content, 96) {
        chunks.push(format!(
            "data: {}\n\n",
            json!({
            "id": &completion_id,
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": piece},
                    "finish_reason": null
                }]
            })
        ));
    }
    chunks.push(format!(
        "data: {}\n\n",
        json!({
            "id": &completion_id,
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        })
    ));
    chunks.push("data: [DONE]\n\n".to_string());
    let body = chunks.join("");
    (
        [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
        body,
    )
        .into_response()
}

pub(crate) fn streaming_response_from_cached_completion_value(value: &Value) -> Response {
    let public = public_completion_value(value);
    let model = public
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_MODEL_ID);
    if let Some(events) = cached_runtime_stream_events(value) {
        streaming_response_from_runtime_events(model, &public, &events)
    } else {
        streaming_response_from_completion_value(&public)
    }
}

pub(crate) fn streaming_response_from_runtime_events(
    model: &str,
    value: &Value,
    events: &[RuntimeWorkflowStreamEvent],
) -> Response {
    let streamed_content = events
        .iter()
        .filter(|event| event.event_type == "content_delta")
        .filter_map(|event| event.content_delta.as_deref())
        .collect::<String>();
    if contains_public_forbidden_knowledge_state_term(&streamed_content) {
        return streaming_response_from_completion_value(&public_completion_value(value));
    }
    let completion_id = format!("chatcmpl-{}", uuid::Uuid::now_v7().simple());
    let mut chunks = Vec::new();
    chunks.push(format!(
        "data: {}\n\n",
        json!({
            "id": &completion_id,
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant"},
                "finish_reason": null
            }]
        })
    ));
    let mut forwarded_delta = false;
    for event in events
        .iter()
        .filter(|event| event.event_type == "content_delta")
    {
        let Some(piece) = event.content_delta.as_deref() else {
            continue;
        };
        forwarded_delta = true;
        chunks.push(format!(
            "data: {}\n\n",
            json!({
                "id": &completion_id,
                "object": "chat.completion.chunk",
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {"content": piece},
                    "finish_reason": null
                }]
            })
        ));
    }
    if !forwarded_delta {
        return streaming_response_from_completion_value(value);
    }
    chunks.push(format!(
        "data: {}\n\n",
        json!({
            "id": &completion_id,
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        })
    ));
    chunks.push("data: [DONE]\n\n".to_string());
    let body = chunks.join("");
    (
        [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
        body,
    )
        .into_response()
}

fn text_stream_chunks(content: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in content.chars() {
        current.push(ch);
        if current.chars().count() >= max_chars || ch == '\n' {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}
