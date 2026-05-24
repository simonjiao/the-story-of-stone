use std::collections::BTreeSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{context_rules, llm_contracts::CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION};

pub(crate) const CONVERSATION_STATE_SUMMARY_OBJECT: &str = "tonglingyu.conversation_state_summary";

const MAX_RECENT_MESSAGES: usize = 8;
const MAX_TOPIC_CHARS: usize = 80;
const MAX_OPEN_QUESTIONS: usize = 4;
const MAX_OPEN_QUESTION_CHARS: usize = 120;
const MAX_BOUNDARIES: usize = 4;
const MAX_BOUNDARY_CHARS: usize = 160;
const MAX_EVIDENCE_REFS: usize = 4;
const MAX_WARNINGS: usize = 4;
const MAX_WARNING_CHARS: usize = 120;
const MIN_SUMMARY_CONFIDENCE: f64 = 0.5;

#[derive(Debug, Clone)]
pub(crate) struct ConversationStateMessage<'a> {
    pub(crate) role: &'a str,
    pub(crate) content: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct ConversationStateInput<'a> {
    pub(crate) current_question: &'a str,
    pub(crate) recent_messages: &'a [ConversationStateMessage<'a>],
    pub(crate) session_summary: &'a str,
    pub(crate) last_public_answer_boundary: Option<&'a str>,
    pub(crate) evidence_package_refs: &'a [&'a str],
    pub(crate) reviewer_warnings: &'a [&'a str],
}

#[derive(Debug, Clone)]
pub(crate) struct ConversationStateValidationContext<'a> {
    pub(crate) source_text: String,
    pub(crate) allowed_evidence_package_refs: &'a [&'a str],
    pub(crate) required_active_entities: &'a [&'a str],
    pub(crate) required_last_answer_boundaries: &'a [&'a str],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ConversationStateSummary {
    pub(crate) object: String,
    pub(crate) schema_version: String,
    pub(crate) current_topic: String,
    pub(crate) active_entities: Vec<String>,
    pub(crate) open_questions: Vec<String>,
    pub(crate) last_answer_boundaries: Vec<String>,
    pub(crate) evidence_package_refs: Vec<String>,
    pub(crate) reviewer_warnings: Vec<String>,
    pub(crate) memory_allowed_as_evidence: bool,
    pub(crate) summary_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConversationStateValidation {
    pub(crate) accepted: bool,
    pub(crate) errors: Vec<String>,
    pub(crate) hallucination_detected: bool,
    pub(crate) internal_leakage_detected: bool,
    pub(crate) boundary_preserved: bool,
    pub(crate) active_entity_recall: f64,
}

pub(crate) fn write_conversation_state_summary(
    input: &ConversationStateInput<'_>,
) -> Result<ConversationStateSummary> {
    let source_text = source_text_for_input(input);
    let active_entities = extract_active_entities(&source_text)?;
    let current_topic = if let Some(entity) = active_entities.first() {
        format!("{entity}相关问题")
    } else if is_metadata_prompt(input.current_question) {
        "metadata prompt ignored by conversation state".to_string()
    } else {
        bounded_text(input.current_question, MAX_TOPIC_CHARS)
    };
    let mut open_questions = Vec::new();
    if !is_metadata_prompt(input.current_question) && !input.current_question.trim().is_empty() {
        open_questions.push(bounded_text(
            input.current_question,
            MAX_OPEN_QUESTION_CHARS,
        ));
    }
    let last_answer_boundaries = input
        .last_public_answer_boundary
        .into_iter()
        .filter(|boundary| !boundary.trim().is_empty())
        .map(|boundary| bounded_text(boundary, MAX_BOUNDARY_CHARS))
        .take(MAX_BOUNDARIES)
        .collect::<Vec<_>>();
    let evidence_package_refs = input
        .evidence_package_refs
        .iter()
        .copied()
        .filter(|item| item.starts_with("package:"))
        .map(|item| bounded_text(item, MAX_BOUNDARY_CHARS))
        .take(MAX_EVIDENCE_REFS)
        .collect::<Vec<_>>();
    let reviewer_warnings = input
        .reviewer_warnings
        .iter()
        .copied()
        .filter(|item| !item.trim().is_empty())
        .map(|item| bounded_text(item, MAX_WARNING_CHARS))
        .take(MAX_WARNINGS)
        .collect::<Vec<_>>();
    let summary_confidence = if active_entities.is_empty() {
        0.74
    } else {
        0.92
    };

    Ok(ConversationStateSummary {
        object: CONVERSATION_STATE_SUMMARY_OBJECT.to_string(),
        schema_version: CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION.to_string(),
        current_topic,
        active_entities,
        open_questions: open_questions
            .into_iter()
            .take(MAX_OPEN_QUESTIONS)
            .collect(),
        last_answer_boundaries,
        evidence_package_refs,
        reviewer_warnings,
        memory_allowed_as_evidence: false,
        summary_confidence,
    })
}

pub(crate) fn validate_conversation_state_summary(
    summary: &ConversationStateSummary,
    context: &ConversationStateValidationContext<'_>,
) -> ConversationStateValidation {
    let mut errors = Vec::new();
    if summary.object != CONVERSATION_STATE_SUMMARY_OBJECT {
        errors.push(format!("object_mismatch: {}", summary.object));
    }
    if summary.schema_version != CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version_mismatch: {}",
            summary.schema_version
        ));
    }
    if summary.memory_allowed_as_evidence {
        errors.push("memory_allowed_as_evidence_true".to_string());
    }
    if !(MIN_SUMMARY_CONFIDENCE..=1.0).contains(&summary.summary_confidence) {
        errors.push(format!(
            "summary_confidence_out_of_range: {}",
            summary.summary_confidence
        ));
    }
    validate_bounded_array(
        &summary.active_entities,
        MAX_OPEN_QUESTIONS,
        MAX_OPEN_QUESTION_CHARS,
        "active_entities",
        &mut errors,
    );
    validate_bounded_array(
        &summary.open_questions,
        MAX_OPEN_QUESTIONS,
        MAX_OPEN_QUESTION_CHARS,
        "open_questions",
        &mut errors,
    );
    validate_bounded_array(
        &summary.last_answer_boundaries,
        MAX_BOUNDARIES,
        MAX_BOUNDARY_CHARS,
        "last_answer_boundaries",
        &mut errors,
    );
    validate_bounded_array(
        &summary.evidence_package_refs,
        MAX_EVIDENCE_REFS,
        MAX_BOUNDARY_CHARS,
        "evidence_package_refs",
        &mut errors,
    );
    validate_bounded_array(
        &summary.reviewer_warnings,
        MAX_WARNINGS,
        MAX_WARNING_CHARS,
        "reviewer_warnings",
        &mut errors,
    );

    let allowed_refs = context
        .allowed_evidence_package_refs
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for package_ref in &summary.evidence_package_refs {
        if !package_ref.starts_with("package:") || !allowed_refs.contains(package_ref.as_str()) {
            errors.push(format!(
                "unauthorized_evidence_package_ref: {}",
                hash_text(package_ref)
            ));
        }
    }

    let mut hallucination_detected = false;
    for entity in &summary.active_entities {
        if !context.source_text.contains(entity) {
            hallucination_detected = true;
            errors.push(format!("hallucinated_entity: {}", hash_text(entity)));
        }
    }
    let active_entity_recall =
        required_recall(&summary.active_entities, context.required_active_entities);
    if active_entity_recall < 1.0 {
        errors.push(format!(
            "active_entity_recall_below_required: {active_entity_recall:.3}"
        ));
    }

    let boundary_preserved = context
        .required_last_answer_boundaries
        .iter()
        .all(|required| {
            summary
                .last_answer_boundaries
                .iter()
                .any(|boundary| boundary.contains(required))
        });
    if !boundary_preserved {
        errors.push("last_answer_boundary_lost".to_string());
    }

    let rendered = serde_json::to_string(summary).unwrap_or_default();
    let internal_leakage_detected = contains_internal_leakage(&rendered);
    if internal_leakage_detected {
        errors.push("internal_leakage_detected".to_string());
    }
    if rendered.contains("memory as evidence") || rendered.contains("memory-as-evidence") {
        errors.push("memory_as_evidence_text_detected".to_string());
    }

    ConversationStateValidation {
        accepted: errors.is_empty(),
        errors,
        hallucination_detected,
        internal_leakage_detected,
        boundary_preserved,
        active_entity_recall,
    }
}

pub(crate) fn conversation_state_validation_context<'a>(
    input: &ConversationStateInput<'a>,
    required_active_entities: &'a [&'a str],
    required_last_answer_boundaries: &'a [&'a str],
) -> ConversationStateValidationContext<'a> {
    ConversationStateValidationContext {
        source_text: source_text_for_input(input),
        allowed_evidence_package_refs: input.evidence_package_refs,
        required_active_entities,
        required_last_answer_boundaries,
    }
}

pub(crate) fn project_conversation_state_summary(
    summary: &ConversationStateSummary,
    consumer_name: &str,
) -> Option<Value> {
    if consumer_name == "honglou-main" {
        Some(json!(summary))
    } else {
        None
    }
}

pub(crate) fn conversation_state_summary_digest(summary: &ConversationStateSummary) -> String {
    serde_json::to_string(summary)
        .map(|value| hash_text(&value))
        .unwrap_or_else(|_| hash_text("invalid-conversation-state-summary"))
}

pub(crate) fn source_text_for_input(input: &ConversationStateInput<'_>) -> String {
    let mut parts = vec![
        input.current_question.to_string(),
        input.session_summary.to_string(),
    ];
    let start = input
        .recent_messages
        .len()
        .saturating_sub(MAX_RECENT_MESSAGES);
    for message in &input.recent_messages[start..] {
        if message.role == "user" || message.role == "assistant" {
            parts.push(bounded_text(message.content, MAX_BOUNDARY_CHARS));
        }
    }
    if let Some(boundary) = input.last_public_answer_boundary {
        parts.push(boundary.to_string());
    }
    parts.join("\n")
}

fn extract_active_entities(text: &str) -> Result<Vec<String>> {
    Ok(context_rules::latest_subject_in_text(text)?
        .into_iter()
        .take(MAX_OPEN_QUESTIONS)
        .collect())
}

fn validate_bounded_array(
    values: &[String],
    max_items: usize,
    max_chars: usize,
    field: &str,
    errors: &mut Vec<String>,
) {
    if values.len() > max_items {
        errors.push(format!("{field}_too_many_items"));
    }
    for value in values {
        if value.trim().is_empty() {
            errors.push(format!("{field}_empty_item"));
        }
        if value.chars().count() > max_chars {
            errors.push(format!("{field}_item_too_long"));
        }
    }
}

fn required_recall(observed: &[String], required: &[&str]) -> f64 {
    if required.is_empty() {
        return 1.0;
    }
    let hit_count = required
        .iter()
        .filter(|required| observed.iter().any(|item| item == **required))
        .count();
    hit_count as f64 / required.len() as f64
}

fn contains_internal_leakage(text: &str) -> bool {
    [
        "trace-",
        "context-pack://",
        "context-projection://",
        "memory-card-",
        "memory-candidate-",
        "memory_read_refs",
        "memory_read_ref_digest",
        "memory_policy_digest",
        "raw_memory",
        "system_prompt",
        "tool_call_id",
        "tool_result_ref",
        "acl_json",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn is_metadata_prompt(text: &str) -> bool {
    text.contains("### Task:") && text.contains("### Chat History:")
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect::<String>()
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_preserves_entity_and_public_answer_boundary() {
        let messages = [ConversationStateMessage {
            role: "assistant",
            content: "上一轮只确认晴雯判词位置，未断言结局。",
        }];
        let evidence_refs = ["package:pkg-1"];
        let input = ConversationStateInput {
            current_question: "她的判词是否指向晴雯结局？",
            recent_messages: &messages,
            session_summary: "历史消息提到晴雯。",
            last_public_answer_boundary: Some("上一轮只确认晴雯判词位置"),
            evidence_package_refs: &evidence_refs,
            reviewer_warnings: &[],
        };
        let summary = write_conversation_state_summary(&input).expect("summary writes");
        let context =
            conversation_state_validation_context(&input, &["晴雯"], &["上一轮只确认晴雯判词位置"]);
        let validation = validate_conversation_state_summary(&summary, &context);

        assert!(validation.accepted, "{:?}", validation.errors);
        assert!(!summary.memory_allowed_as_evidence);
        assert!(summary.active_entities.contains(&"晴雯".to_string()));
    }

    #[test]
    fn validator_rejects_hallucinated_entity_and_memory_evidence() {
        let input = ConversationStateInput {
            current_question: "林黛玉为何葬花？",
            recent_messages: &[],
            session_summary: "历史消息提到林黛玉。",
            last_public_answer_boundary: None,
            evidence_package_refs: &[],
            reviewer_warnings: &[],
        };
        let summary = ConversationStateSummary {
            object: CONVERSATION_STATE_SUMMARY_OBJECT.to_string(),
            schema_version: CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION.to_string(),
            current_topic: "晴雯相关问题".to_string(),
            active_entities: vec!["晴雯".to_string()],
            open_questions: vec!["林黛玉为何葬花？".to_string()],
            last_answer_boundaries: Vec::new(),
            evidence_package_refs: vec!["memory-card-1".to_string()],
            reviewer_warnings: Vec::new(),
            memory_allowed_as_evidence: true,
            summary_confidence: 0.91,
        };
        let context = conversation_state_validation_context(&input, &[], &[]);
        let validation = validate_conversation_state_summary(&summary, &context);

        assert!(!validation.accepted);
        assert!(validation.hallucination_detected);
        assert!(validation.internal_leakage_detected);
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error == "memory_allowed_as_evidence_true")
        );
    }

    #[test]
    fn projection_guard_allows_only_main_profile() {
        let summary = ConversationStateSummary {
            object: CONVERSATION_STATE_SUMMARY_OBJECT.to_string(),
            schema_version: CONVERSATION_STATE_SUMMARY_SCHEMA_VERSION.to_string(),
            current_topic: "林黛玉相关问题".to_string(),
            active_entities: vec!["林黛玉".to_string()],
            open_questions: vec!["林黛玉为何葬花？".to_string()],
            last_answer_boundaries: Vec::new(),
            evidence_package_refs: Vec::new(),
            reviewer_warnings: Vec::new(),
            memory_allowed_as_evidence: false,
            summary_confidence: 0.91,
        };

        assert!(project_conversation_state_summary(&summary, "honglou-main").is_some());
        assert!(project_conversation_state_summary(&summary, "honglou-text").is_none());
        assert!(project_conversation_state_summary(&summary, "honglou-reviewer").is_none());
    }
}
