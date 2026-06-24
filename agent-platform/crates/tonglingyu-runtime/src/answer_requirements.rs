use crate::{
    EvidenceCard,
    answer_composer::public_quote_text,
    answer_rules::{AnswerCuePolicy, answer_cue_policy_for_question},
    extract_chapter_no,
    governance_rules::preferred_answer_evidence_types,
    normalize_text, retrieval_rules, trim_text,
    upstream_bundle::evidence_card_source_layer,
};
use anyhow::Result;
use serde_json::{Value, json};

const ANSWER_REQUIREMENTS_SCHEMA_VERSION: &str = "tonglingyu.answer_requirements.v1";

#[derive(Debug, Clone)]
struct RequestedEvidenceAnchor {
    evidence_type: String,
    public_label: String,
    require_text_cue: bool,
    cards: Vec<EvidenceAnchorCard>,
}

#[derive(Debug, Clone)]
struct EvidenceAnchorCard {
    evidence_id: String,
    source_title: String,
    source_layer: String,
    source_anchor_cues: Vec<String>,
    text_anchor_cues: Vec<String>,
}

pub(crate) fn answer_requirements_for_message(
    question: &str,
    cards: &[EvidenceCard],
) -> Result<Value> {
    let policy = answer_cue_policy_for_question(question)?;
    let anchors = requested_evidence_type_anchors(question, cards, &policy)?;
    Ok(json!({
        "object": "tonglingyu.answer_requirements",
        "schema_version": ANSWER_REQUIREMENTS_SCHEMA_VERSION,
        "evidence_request": policy.evidence_request,
        "required_cue_policy": {
            "require_text_cue": policy.require_text_cue,
            "rule": &policy.rule
        },
        "requested_evidence_type_anchors": anchors.iter().map(requested_anchor_value).collect::<Vec<_>>(),
        "rule": &policy.rule
    }))
}

pub(crate) fn draft_lacks_requested_evidence_type_anchor(
    question: &str,
    draft_text: &str,
    cards: &[EvidenceCard],
) -> bool {
    let Ok(policy) = answer_cue_policy_for_question(question) else {
        return true;
    };
    let Ok(anchors) = requested_evidence_type_anchors(question, cards, &policy) else {
        return true;
    };
    anchors.into_iter().any(|anchor| {
        !anchor.cards.is_empty()
            && !anchor.cards.iter().any(|card| {
                if anchor.require_text_cue {
                    card.text_anchor_cues
                        .iter()
                        .any(|cue| source_cue_matches(draft_text, cue))
                } else {
                    card.source_anchor_cues
                        .iter()
                        .chain(card.text_anchor_cues.iter())
                        .any(|cue| source_cue_matches(draft_text, cue))
                }
            })
    })
}

pub(crate) fn public_source_cues_for_title(source_title: &str) -> Result<Vec<String>> {
    let policy = answer_cue_policy_for_question("")?;
    Ok(public_source_cues_for_title_with_limit(
        source_title,
        policy.max_source_title_cue_chars,
    ))
}

fn requested_evidence_type_anchors(
    question: &str,
    cards: &[EvidenceCard],
    policy: &AnswerCuePolicy,
) -> Result<Vec<RequestedEvidenceAnchor>> {
    let preferred_types = preferred_answer_evidence_types(question)?;
    let mut anchors = Vec::new();
    for evidence_type in preferred_types {
        let matching_cards = cards
            .iter()
            .filter(|card| card.evidence_type == evidence_type)
            .take(policy.max_required_evidence_cards)
            .filter_map(|card| evidence_anchor_card(card, policy))
            .collect::<Vec<_>>();
        if matching_cards.is_empty() {
            continue;
        }
        anchors.push(RequestedEvidenceAnchor {
            public_label: source_layer_public_label(&matching_cards[0].source_layer),
            evidence_type,
            require_text_cue: policy.require_text_cue,
            cards: matching_cards,
        });
    }
    Ok(anchors)
}

fn evidence_anchor_card(
    card: &EvidenceCard,
    policy: &AnswerCuePolicy,
) -> Option<EvidenceAnchorCard> {
    let source_anchor_cues = source_anchor_cues_for_card(card, policy);
    let text_anchor_cues = text_anchor_cues_for_card(card, policy);
    if source_anchor_cues.is_empty() && text_anchor_cues.is_empty() {
        return None;
    }
    Some(EvidenceAnchorCard {
        evidence_id: card.evidence_id.clone(),
        source_title: trim_text(&card.source_title, policy.max_source_title_cue_chars),
        source_layer: evidence_card_source_layer(card).to_string(),
        source_anchor_cues,
        text_anchor_cues,
    })
}

fn requested_anchor_value(anchor: &RequestedEvidenceAnchor) -> Value {
    json!({
        "evidence_type": &anchor.evidence_type,
        "public_label": &anchor.public_label,
        "required_cue_kind": if anchor.require_text_cue { "text" } else { "source_or_text" },
        "cards": anchor.cards.iter().map(evidence_anchor_card_value).collect::<Vec<_>>(),
    })
}

fn evidence_anchor_card_value(card: &EvidenceAnchorCard) -> Value {
    json!({
        "evidence_id": &card.evidence_id,
        "source_layer": &card.source_layer,
        "source_title": &card.source_title,
        "source_anchor_cues": &card.source_anchor_cues,
        "text_anchor_cues": &card.text_anchor_cues,
        "acceptable_anchor_cues": card.source_anchor_cues.iter().chain(card.text_anchor_cues.iter()).collect::<Vec<_>>(),
    })
}

fn source_anchor_cues_for_card(card: &EvidenceCard, policy: &AnswerCuePolicy) -> Vec<String> {
    let mut cues = public_source_cues_for_title_with_limit(
        &card.source_title,
        policy.max_source_title_cue_chars,
    );
    if let Ok(source_system) = retrieval_rules::version_system(&card.source_id) {
        push_public_cue(
            &mut cues,
            &source_system,
            policy.max_text_anchor_cue_chars,
            1,
        );
    }
    cues.truncate(policy.max_anchor_cues_per_card);
    cues
}

fn text_anchor_cues_for_card(card: &EvidenceCard, policy: &AnswerCuePolicy) -> Vec<String> {
    let mut cues = Vec::new();
    push_external_ranking_text_cues(&mut cues, &card.text, policy);
    push_chapter_marker_text_cues(&mut cues, &card.text, policy);
    push_public_text_excerpt_cues(&mut cues, &card.text, policy);
    cues.truncate(policy.max_anchor_cues_per_card);
    cues
}

fn public_source_cues_for_title_with_limit(source_title: &str, max_chars: usize) -> Vec<String> {
    let mut cues = Vec::new();
    push_public_cue(&mut cues, source_title, max_chars, 1);
    if let Some(tail) = source_title
        .rsplit('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        push_public_cue(&mut cues, tail, max_chars, 1);
    }
    if let Some(chapter_no) = extract_chapter_no(source_title) {
        push_public_cue(&mut cues, &format!("第{chapter_no}回"), max_chars, 1);
        push_public_cue(&mut cues, &format!("第{chapter_no:03}回"), max_chars, 1);
    }
    cues
}

fn source_layer_public_label(source_layer: &str) -> String {
    retrieval_rules::source_layer_label(source_layer).unwrap_or_else(|_| source_layer.to_string())
}

fn push_external_ranking_text_cues(cues: &mut Vec<String>, text: &str, policy: &AnswerCuePolicy) {
    let Ok(ranking) = retrieval_rules::ranking_rules() else {
        return;
    };
    for term in ranking
        .fate_text_terms
        .iter()
        .chain(ranking.inscription_text_terms.iter())
        .chain(ranking.tonglingyu_terms.iter())
    {
        if text_contains_public_cue(text, term) {
            push_public_cue(
                cues,
                term,
                policy.max_text_anchor_cue_chars,
                policy.text_cue_min_chars,
            );
        }
    }
}

fn push_public_text_excerpt_cues(cues: &mut Vec<String>, text: &str, policy: &AnswerCuePolicy) {
    let cleaned = public_quote_text(text);
    if cleaned.trim().is_empty() {
        return;
    }
    for segment in cleaned
        .split(['。', '；', ';', '！', '!', '？', '?', '\n', '\r'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        push_public_cue(
            cues,
            segment,
            policy.max_text_anchor_cue_chars,
            policy.text_cue_min_chars,
        );
        if !cues.is_empty() {
            return;
        }
    }
}

fn push_chapter_marker_text_cues(cues: &mut Vec<String>, text: &str, policy: &AnswerCuePolicy) {
    for (index, _) in text.char_indices().filter(|(_, ch)| *ch == '第') {
        let Some(relative_end) = text[index..].find('回') else {
            continue;
        };
        let end = index + relative_end + '回'.len_utf8();
        let marker = &text[index..end];
        if marker.chars().count() > 12 || extract_chapter_no(marker).is_none() {
            continue;
        }
        push_public_cue(
            cues,
            marker,
            policy.max_text_anchor_cue_chars,
            policy.text_cue_min_chars,
        );
        if let Some(chapter_no) = extract_chapter_no(marker) {
            push_public_cue(
                cues,
                &format!("第{chapter_no}回"),
                policy.max_text_anchor_cue_chars,
                policy.text_cue_min_chars,
            );
            push_public_cue(
                cues,
                &format!("第{chapter_no:03}回"),
                policy.max_text_anchor_cue_chars,
                policy.text_cue_min_chars,
            );
        }
    }
}

fn text_contains_public_cue(text: &str, cue: &str) -> bool {
    let cue = cue.trim();
    !cue.is_empty() && (text.contains(cue) || normalize_text(text).contains(&normalize_text(cue)))
}

fn source_cue_matches(draft_text: &str, cue: &str) -> bool {
    text_contains_public_cue(draft_text, cue)
}

fn push_public_cue(cues: &mut Vec<String>, cue: &str, max_chars: usize, min_chars: usize) {
    let cue = trim_text(cue.trim(), max_chars);
    if cue.trim().is_empty() || cue.chars().filter(|ch| !ch.is_whitespace()).count() < min_chars {
        return;
    }
    let normalized = normalize_text(&cue);
    if normalized.is_empty()
        || cues
            .iter()
            .any(|existing| normalize_text(existing) == normalized)
    {
        return;
    }
    cues.push(cue);
}
