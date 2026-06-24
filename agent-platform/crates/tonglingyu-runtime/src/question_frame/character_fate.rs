use crate::{
    EvidenceCard,
    answer_composer::{EvidenceSlotMatch, public_quote_text, representative_matches},
    answer_rules::{CharacterFatePolicy, character_fate_policy},
    evidence_slot_matches_for_cards, normalize_text,
    retrieval_rules::source_layer_label,
};
use std::collections::BTreeSet;

use super::{RuntimeQuestionFrame, RuntimeQuestionFrameEntity};

const MAX_CHARACTER_FATE_SUPPORTS: usize = 4;
const CHARACTER_FATE_DISPLAY_GROUP_PREFIX: &str = "character_fate";

pub(crate) fn character_fate_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.is_character_fate())?;
    let entity = frame.character_fate_entity()?;
    let policy = match character_fate_policy() {
        Ok(policy) => policy,
        Err(_) => {
            return Some("治理规则目录不可用，不能可靠回答这个人物结局问题。".to_string());
        }
    };
    let matches =
        representative_matches(&character_fate_slot_matches(Some(frame), cards), |_| true)
            .into_iter()
            .take(MAX_CHARACTER_FATE_SUPPORTS)
            .collect::<Vec<_>>();
    if matches.is_empty() {
        return Some(render_character_fate_template(
            &policy.missing_evidence_template,
            "",
            &entity.canonical,
            "",
        ));
    }

    let mut answer = character_fate_opening(entity, &matches, &policy);
    answer.push_str("\n\n依据：\n");
    for (index, item) in matches.iter().enumerate() {
        answer.push_str(&format!(
            "{}. {}（{}，{}）：{}\n",
            index + 1,
            item.label,
            source_layer_label(&item.source_layer).unwrap_or_else(|_| item.source_layer.clone()),
            item.source_title,
            character_fate_quote(item)
        ));
    }
    answer.push_str(&character_fate_scope_boundary(&matches, &policy));
    Some(answer)
}

fn character_fate_opening(
    entity: &RuntimeQuestionFrameEntity,
    matches: &[EvidenceSlotMatch],
    policy: &CharacterFatePolicy,
) -> String {
    let scope_label = if matches
        .iter()
        .any(|item| item.source_layer == "base_text_later_40")
    {
        policy.later_forty_scope_label.as_str()
    } else {
        policy.default_scope_label.as_str()
    };
    let cue_text = character_fate_cue_text(matches, policy);
    let has_prophecy = matches.iter().any(character_fate_is_prophecy);
    let has_attribution = matches.iter().any(character_fate_is_attribution);

    let mut opening = if has_prophecy {
        render_character_fate_template(
            &policy.prophecy_opening_template,
            scope_label,
            &entity.canonical,
            &cue_text,
        )
    } else {
        render_character_fate_template(
            &policy.limited_opening_template,
            scope_label,
            &entity.canonical,
            &cue_text,
        )
    };
    if has_attribution {
        opening.push_str(&policy.attribution_sentence);
    }
    opening
}

fn render_character_fate_template(template: &str, scope: &str, entity: &str, cues: &str) -> String {
    template
        .replace("{scope}", scope)
        .replace("{entity}", entity)
        .replace("{cues}", cues)
}

fn character_fate_scope_boundary(
    matches: &[EvidenceSlotMatch],
    policy: &CharacterFatePolicy,
) -> String {
    if matches
        .iter()
        .any(|item| item.source_layer == "base_text_later_40")
    {
        policy.later_forty_scope_boundary.clone()
    } else {
        policy.default_scope_boundary.clone()
    }
}

fn character_fate_cue_text(matches: &[EvidenceSlotMatch], policy: &CharacterFatePolicy) -> String {
    let mut cues = Vec::new();
    let mut seen = BTreeSet::new();
    collect_character_fate_cues(matches, policy, &mut cues, &mut seen, |item| {
        character_fate_is_prophecy(item)
    });
    if cues.is_empty() {
        collect_character_fate_cues(matches, policy, &mut cues, &mut seen, |_| true);
    }
    cues.into_iter().take(3).collect::<Vec<_>>().join("、")
}

fn collect_character_fate_cues<F>(
    matches: &[EvidenceSlotMatch],
    policy: &CharacterFatePolicy,
    cues: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    predicate: F,
) where
    F: Fn(&EvidenceSlotMatch) -> bool,
{
    for item in matches.iter().filter(|item| predicate(item)) {
        for term in &item.matched_terms {
            if let Some(cue) = character_fate_public_cue(term, policy) {
                let key = normalize_text(&cue);
                if seen.insert(key) {
                    cues.push(cue);
                }
            }
            if cues.len() >= 3 {
                return;
            }
        }
    }
    if cues.is_empty() {
        for item in matches.iter().filter(|item| predicate(item)) {
            let key = normalize_text(&item.label);
            if seen.insert(key) {
                cues.push(item.label.clone());
            }
            if cues.len() >= 3 {
                return;
            }
        }
    }
}

fn character_fate_public_cue(term: &str, policy: &CharacterFatePolicy) -> Option<String> {
    let term = term
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '。' | '，'
                    | ','
                    | '.'
                    | '；'
                    | ';'
                    | '：'
                    | ':'
                    | '、'
                    | ' '
                    | '['
                    | ']'
                    | '【'
                    | '】'
            )
        })
        .trim();
    let normalized = normalize_text(term);
    if term.chars().count() < 3
        || policy
            .generic_opening_cue_terms
            .iter()
            .any(|rule_term| normalized == normalize_text(rule_term))
        || policy
            .excluded_opening_cue_terms
            .iter()
            .any(|rule_term| normalized.contains(&normalize_text(rule_term)))
    {
        return None;
    }
    Some(term.to_string())
}

fn character_fate_is_prophecy(item: &EvidenceSlotMatch) -> bool {
    item.evidence_strength.as_deref() == Some("prophetic")
        || item
            .support_modality
            .as_deref()
            .is_some_and(|value| value.contains("prophecy"))
}

fn character_fate_is_attribution(item: &EvidenceSlotMatch) -> bool {
    item.support_modality
        .as_deref()
        .is_some_and(|value| value.contains("attribution"))
        || item.role.contains("attribution")
}

pub(crate) fn character_fate_review_issues(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Vec<String> {
    let Some(frame) = frame.filter(|frame| frame.is_character_fate()) else {
        return Vec::new();
    };
    if character_fate_slot_matches(Some(frame), cards).is_empty() {
        return vec!["character_fate_evidence_missing".to_string()];
    }
    Vec::new()
}

pub(crate) fn character_fate_draft_rejection_reason(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
    draft: &str,
) -> Option<&'static str> {
    let frame = frame.filter(|frame| frame.is_character_fate())?;
    let matches = character_fate_slot_matches(Some(frame), cards);
    if matches.is_empty() {
        return None;
    }
    let draft = normalize_text(draft);
    if matches
        .iter()
        .any(|item| character_fate_draft_mentions_slot(item, &draft))
    {
        return None;
    }
    Some("character_fate_draft_missing_bound_evidence_cue")
}

pub(crate) fn character_fate_supported_evidence_ids(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> BTreeSet<String> {
    character_fate_slot_matches(frame, cards)
        .into_iter()
        .map(|item| item.evidence_id)
        .collect()
}

pub(crate) fn character_fate_slot_matches(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Vec<EvidenceSlotMatch> {
    let Some(frame) = frame.filter(|frame| frame.is_character_fate()) else {
        return Vec::new();
    };
    let Some(entity) = frame.character_fate_entity() else {
        return Vec::new();
    };
    evidence_slot_matches_for_cards(&frame.canonical_question, cards)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| {
            item_is_character_fate_slot(item) && item_binds_character_fate_entity(item, entity)
        })
        .collect()
}

fn item_is_character_fate_slot(item: &EvidenceSlotMatch) -> bool {
    item.display_group
        .starts_with(CHARACTER_FATE_DISPLAY_GROUP_PREFIX)
        || item.role.contains("character_fate")
        || item
            .support_modality
            .as_deref()
            .is_some_and(|value| value.contains("character_fate"))
}

fn item_binds_character_fate_entity(
    item: &EvidenceSlotMatch,
    entity: &RuntimeQuestionFrameEntity,
) -> bool {
    if item
        .supports_subjects
        .iter()
        .any(|subject| subject == &entity.canonical)
    {
        return true;
    }
    let combined = normalize_text(&format!("{} {}", item.source_title, item.text));
    entity
        .identity_terms()
        .into_iter()
        .map(|term| normalize_text(&term))
        .filter(|term| !term.trim().is_empty())
        .any(|term| combined.contains(&term))
}

fn character_fate_draft_mentions_slot(item: &EvidenceSlotMatch, normalized_draft: &str) -> bool {
    std::iter::once(item.label.as_str())
        .chain(std::iter::once(item.public_role_label.as_str()))
        .chain(item.matched_terms.iter().map(String::as_str))
        .map(normalize_text)
        .filter(|term| term.chars().count() >= 2)
        .any(|term| normalized_draft.contains(&term))
}

fn character_fate_quote(item: &EvidenceSlotMatch) -> String {
    let text = public_quote_text(&item.text);
    let mut terms = item.matched_terms.clone();
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    for term in terms {
        if let Some(quote) = quote_around(&text, &term, 72) {
            return quote;
        }
    }
    trim_chars(&text, 72)
}

fn quote_around(text: &str, focus: &str, limit: usize) -> Option<String> {
    let start = text.find(focus)?;
    let before = tail_chars(&text[..start], limit / 2);
    let focus_end = start + focus.len();
    let after = head_chars(&text[focus_end..], limit / 2);
    Some(trim_chars(&format!("{before}{focus}{after}"), limit))
}

fn trim_chars(text: &str, limit: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= limit {
        return text.trim().to_string();
    }
    chars.into_iter().take(limit).collect::<String>() + "..."
}

fn head_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn tail_chars(text: &str, limit: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(limit);
    chars[start..].iter().copied().collect()
}
