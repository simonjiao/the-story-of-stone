use crate::{
    EvidenceCard,
    answer_composer::{EvidenceSlotMatch, concise_slot_quote, representative_matches},
    answer_rules::{EvidenceQueryPolicy, evidence_query_policy},
    evidence_slot_matches_for_cards,
    governance_rules::preferred_answer_evidence_types,
    normalize_text,
    retrieval_rules::source_layer_label,
};

use super::{RuntimeQuestionFrame, RuntimeQuestionFrameEntity};

const MAX_EVIDENCE_QUERY_SUPPORTS: usize = 4;

pub(crate) fn evidence_query_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.intent == "evidence_query")?;
    let entity = frame.subject.as_ref().or(frame.object.as_ref())?;
    let policy = match evidence_query_policy() {
        Ok(policy) => policy,
        Err(_) => {
            return Some("治理规则目录不可用，不能可靠回答这个证据追问。".to_string());
        }
    };
    let preferred_types = preferred_answer_evidence_types(&frame.canonical_question)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>();
    let slot_matches = evidence_slot_matches_for_cards(&frame.canonical_question, cards)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| slot_match_binds_entity(item, entity))
        .collect::<Vec<_>>();
    if slot_matches.is_empty() {
        return None;
    }
    let mut selected = slot_matches
        .iter()
        .filter(|item| {
            preferred_types.is_empty()
                || preferred_types
                    .iter()
                    .any(|evidence_type| slot_match_matches_evidence_type(item, evidence_type))
        })
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() && !preferred_types.is_empty() {
        return Some(render_evidence_query_template(
            &policy.no_requested_type_template,
            &[
                (
                    "{evidence_label}",
                    &preferred_evidence_label(&preferred_types),
                ),
                ("{subject}", &entity.canonical),
            ],
        ));
    }
    selected = representative_matches(&selected, |_| true)
        .into_iter()
        .take(MAX_EVIDENCE_QUERY_SUPPORTS)
        .collect();
    if selected.is_empty() {
        return None;
    }

    let mut answer = render_evidence_query_template(
        &policy.matched_intro_template,
        &[
            (
                "{evidence_label}",
                &preferred_evidence_label(&preferred_types),
            ),
            ("{subject}", &entity.canonical),
        ],
    );
    for (index, item) in selected.iter().enumerate() {
        answer.push_str(&evidence_query_item(&policy, index + 1, item));
    }
    answer.push_str(&policy.boundary_sentence);
    Some(answer)
}

fn slot_match_binds_entity(item: &EvidenceSlotMatch, entity: &RuntimeQuestionFrameEntity) -> bool {
    if item
        .supports_subjects
        .iter()
        .any(|subject| subject == &entity.canonical)
    {
        return true;
    }
    if !item.supports_subjects.is_empty() {
        return false;
    }
    let combined = normalize_text(&format!("{} {}", item.source_title, item.text));
    entity
        .identity_terms()
        .into_iter()
        .map(|term| normalize_text(&term))
        .filter(|term| !term.trim().is_empty())
        .any(|term| combined.contains(&term))
}

fn slot_match_matches_evidence_type(item: &EvidenceSlotMatch, evidence_type: &str) -> bool {
    let evidence_type = evidence_type.trim();
    match evidence_type {
        "commentary" => item.source_layer == "commentary",
        "base_text" => item.source_layer.starts_with("base_text"),
        _ => item.source_layer == evidence_type,
    }
}

fn preferred_evidence_label(preferred_types: &[String]) -> String {
    if preferred_types.is_empty() {
        return "证据".to_string();
    }
    preferred_types
        .iter()
        .map(|evidence_type| {
            source_layer_label(evidence_type).unwrap_or_else(|_| evidence_type.to_string())
        })
        .collect::<Vec<_>>()
        .join("、")
}

fn evidence_query_item(
    policy: &EvidenceQueryPolicy,
    index: usize,
    item: &EvidenceSlotMatch,
) -> String {
    render_evidence_query_template(
        &policy.evidence_item_template,
        &[
            ("{index}", &index.to_string()),
            ("{label}", &item.label),
            (
                "{source_layer}",
                &source_layer_label(&item.source_layer)
                    .unwrap_or_else(|_| item.source_layer.clone()),
            ),
            ("{source_title}", &item.source_title),
            ("{quote}", &concise_slot_quote(item)),
        ],
    )
}

fn render_evidence_query_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, value);
    }
    rendered
}
