use crate::{
    EvidencePackage,
    evidence_slot_rules::{EvidenceSlotCountBasis, EvidenceSlotRule},
    upstream_bundle::{evidence_card_source_layer, source_scope_policy_for_question},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(crate) struct EvidenceSlotMatch {
    pub slot_id: String,
    pub label: String,
    pub role: String,
    pub counts_as: Vec<String>,
    pub display_group: String,
    pub matched_terms: Vec<String>,
    pub source_title: String,
    pub source_layer: String,
    pub text: String,
}

impl EvidenceSlotMatch {
    pub(crate) fn from_rule(
        _evidence_id: &str,
        source_title: &str,
        source_layer: &str,
        text: &str,
        matched_terms: Vec<String>,
        rule: EvidenceSlotRule,
    ) -> Self {
        Self {
            slot_id: rule.id,
            label: rule.label,
            role: rule.role,
            counts_as: rule.counts_as,
            display_group: rule.display_group,
            matched_terms,
            source_title: source_title.to_string(),
            source_layer: source_layer.to_string(),
            text: text.to_string(),
        }
    }
}

pub(crate) fn compose_slot_count_answer(
    package: &EvidencePackage,
    active_basis: &EvidenceSlotCountBasis,
    slot_matches: &[EvidenceSlotMatch],
) -> Option<String> {
    if slot_matches.is_empty() {
        return None;
    }
    let direct = representative_matches(slot_matches, |item| {
        item.counts_as.iter().any(|basis| basis == &active_basis.id)
    });
    let related = representative_matches(slot_matches, |item| {
        !item.counts_as.iter().any(|basis| basis == &active_basis.id)
            && item.display_group != "unclassified"
    });
    if direct.is_empty() && related.is_empty() {
        return None;
    }

    let mut answer = String::new();
    let policy = source_scope_policy_for_question(&package.question);
    if direct.is_empty() {
        answer.push_str(&format!(
            "严格按“{}”口径，当前证据不能直接支持具体次数。",
            active_basis.label
        ));
    } else {
        answer.push_str(&format!(
            "严格按“{}”口径，当前证据能直接支持{}{}：{}。",
            active_basis.label,
            chinese_number(direct.len()),
            active_basis.answer_unit,
            labels_join(&direct)
        ));
    }
    if policy.later_forty_allowed {
        answer.push_str("本次问题已显式打开后四十回范围；回答仍按证据来源层标注。");
    } else {
        answer.push_str("默认范围是前八十回正文 + 脂批；后四十回未纳入，除非用户明确要求。");
    }

    if !related.is_empty() {
        answer.push_str(&format!(
            "\n\n另有{}条相关线索，不能直接计为“{}”：{}。",
            chinese_number(related.len()),
            active_basis.answer_noun,
            related_labels_with_roles(&related)
        ));
    }

    answer.push_str("\n\n依据：");
    let mut index = 1;
    for item in &direct {
        answer.push_str(&format!(
            "\n{}. {}（{}，{}）：{}",
            index,
            item.label,
            source_layer_label(&item.source_layer),
            item.source_title,
            concise_slot_quote(item)
        ));
        index += 1;
    }
    for item in &related {
        answer.push_str(&format!(
            "\n{}. {}（{}，{}）：{}",
            index,
            item.label,
            source_layer_label(&item.source_layer),
            item.source_title,
            concise_slot_quote(item)
        ));
        index += 1;
    }
    Some(answer)
}

pub(crate) fn representative_matches<F>(
    slot_matches: &[EvidenceSlotMatch],
    include: F,
) -> Vec<EvidenceSlotMatch>
where
    F: Fn(&EvidenceSlotMatch) -> bool,
{
    let mut by_slot = BTreeMap::<String, EvidenceSlotMatch>::new();
    for item in slot_matches {
        if !include(item) {
            continue;
        }
        by_slot
            .entry(item.slot_id.clone())
            .and_modify(|existing| {
                if evidence_rank(item) < evidence_rank(existing) {
                    *existing = item.clone();
                }
            })
            .or_insert_with(|| item.clone());
    }
    by_slot.into_values().collect()
}

pub(crate) fn direct_count_for_basis(
    active_basis: &EvidenceSlotCountBasis,
    slot_matches: &[EvidenceSlotMatch],
) -> usize {
    representative_matches(slot_matches, |item| {
        item.counts_as.iter().any(|basis| basis == &active_basis.id)
    })
    .len()
}

fn evidence_rank(item: &EvidenceSlotMatch) -> usize {
    match item.source_layer.as_str() {
        "base_text_pre_80" => 0,
        "commentary" => 1,
        "version_note" => 2,
        "base_text_later_40" => 3,
        _ => 4,
    }
}

fn labels_join(items: &[EvidenceSlotMatch]) -> String {
    items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("、")
}

fn related_labels_with_roles(items: &[EvidenceSlotMatch]) -> String {
    items
        .iter()
        .map(|item| format!("{}（{}）", item.label, public_role_label(&item.role)))
        .collect::<Vec<_>>()
        .join("、")
}

fn public_role_label(role: &str) -> &'static str {
    match role {
        "suspected_transfer_related_to_loss" => "送玉/流转疑似线索",
        "recovery_or_lost_and_found_clue" => "拾玉/失而复见线索",
        "direct_loss_or_theft" => "直接丢失或被盗",
        "later_forty_direct_loss" => "后四十回直接失玉",
        _ => "相关线索",
    }
}

fn source_layer_label(source_layer: &str) -> &'static str {
    match source_layer {
        "base_text_pre_80" => "正文",
        "base_text_later_40" => "后四十回正文",
        "commentary" => "脂批",
        "version_note" => "版本说明",
        _ => "证据",
    }
}

fn concise_slot_quote(item: &EvidenceSlotMatch) -> String {
    for term in &item.matched_terms {
        if let Some(quote) = quote_around(&item.text, term) {
            return quote;
        }
    }
    trim_chars(&item.text, 56)
}

fn quote_around(text: &str, term: &str) -> Option<String> {
    let index = text.find(term)?;
    let start = char_floor(text, index, 18);
    let end = char_ceil(text, index + term.len(), 24);
    let mut excerpt = String::new();
    if start > 0 {
        excerpt.push('…');
    }
    excerpt.push_str(text[start..end].trim());
    if end < text.len() {
        excerpt.push('…');
    }
    Some(excerpt)
}

fn char_floor(text: &str, byte_index: usize, chars_before: usize) -> usize {
    text[..byte_index]
        .char_indices()
        .rev()
        .nth(chars_before)
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn char_ceil(text: &str, byte_index: usize, chars_after: usize) -> usize {
    text[byte_index..]
        .char_indices()
        .nth(chars_after)
        .map(|(index, _)| byte_index + index)
        .unwrap_or(text.len())
}

fn trim_chars(text: &str, limit: usize) -> String {
    let mut output = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        output.push('…');
    }
    output
}

fn chinese_number(value: usize) -> String {
    match value {
        0 => "零".to_string(),
        1 => "一".to_string(),
        2 => "两".to_string(),
        3 => "三".to_string(),
        4 => "四".to_string(),
        5 => "五".to_string(),
        6 => "六".to_string(),
        7 => "七".to_string(),
        8 => "八".to_string(),
        9 => "九".to_string(),
        _ => value.to_string(),
    }
}

pub(crate) fn source_layer_for_card(card: &crate::EvidenceCard) -> String {
    evidence_card_source_layer(card).to_string()
}

#[cfg(test)]
mod tests;
