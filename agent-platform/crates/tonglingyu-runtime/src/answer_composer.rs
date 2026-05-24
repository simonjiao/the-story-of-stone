use crate::{
    EvidencePackage,
    evidence_slot_rules::{EvidenceSlotCountBasis, EvidenceSlotRule},
    retrieval_rules::{source_layer_answer_rank, source_layer_label},
    upstream_bundle::{evidence_card_source_layer, source_scope_policy_for_question},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(crate) struct EvidenceSlotMatch {
    pub slot_id: String,
    pub label: String,
    pub public_role_label: String,
    pub counts_as: Vec<String>,
    pub display_group: String,
    pub count_note: Option<String>,
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
            public_role_label: rule.public_role_label,
            counts_as: rule.counts_as,
            display_group: rule.display_group,
            count_note: rule.count_note,
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
    let direct_notes = direct
        .iter()
        .filter_map(|item| {
            item.count_note
                .as_deref()
                .map(|note| format!("{}：{}", item.label, note.trim_end_matches('。')))
        })
        .collect::<Vec<_>>();
    let related = representative_matches(slot_matches, |item| {
        !item.counts_as.iter().any(|basis| basis == &active_basis.id)
            && item.display_group != "unclassified"
    });
    if direct.is_empty() && related.is_empty() {
        return None;
    }

    let mut answer = String::new();
    let policy = source_scope_policy_for_question(&package.question);
    let scope_label = if policy.later_forty_allowed {
        "本次明确纳入后四十回范围"
    } else {
        "默认范围（前八十回正文 + 脂批）"
    };
    if direct.is_empty() {
        answer.push_str(&format!(
            "按{}看，严格按“{}”口径，当前证据不能直接支持具体次数。",
            scope_label, active_basis.label
        ));
    } else {
        answer.push_str(&format!(
            "按{}看，严格按“{}”口径，当前证据能直接支持{}{}：{}。",
            scope_label,
            active_basis.label,
            chinese_number(direct.len()),
            active_basis.answer_unit,
            labels_join(&direct)
        ));
    }
    if policy.later_forty_allowed {
        answer.push_str("回答仍按证据来源层标注。");
    } else {
        answer.push_str("后四十回未纳入，除非用户明确要求。");
    }

    if !direct_notes.is_empty() {
        answer.push_str(&format!("\n\n计数说明：{}。", direct_notes.join("；")));
    }

    if !related.is_empty() {
        answer.push_str(&format!(
            "\n\n另有{}条流转/疑似线索，不能直接计入次数：{}。",
            chinese_number(related.len()),
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
            source_layer_label(&item.source_layer).unwrap_or_else(|_| item.source_layer.clone()),
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
            source_layer_label(&item.source_layer).unwrap_or_else(|_| item.source_layer.clone()),
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
    source_layer_answer_rank(&item.source_layer).unwrap_or(usize::MAX)
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
        .map(|item| format!("{}（{}）", item.label, item.public_role_label))
        .collect::<Vec<_>>()
        .join("、")
}

fn concise_slot_quote(item: &EvidenceSlotMatch) -> String {
    let text = public_quote_text(&item.text);
    for term in &item.matched_terms {
        if let Some(quote) = quote_around(&text, term) {
            return quote;
        }
    }
    trim_chars(&text, 56)
}

pub(crate) fn public_quote_text(text: &str) -> String {
    let stripped = strip_angle_tags(&strip_wiki_templates(text))
        .replace("'''", "")
        .replace("<br />", " ")
        .replace("<br/>", " ")
        .replace("<br>", " ");
    collapse_whitespace(&stripped)
}

fn strip_wiki_templates(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let template = &rest[start + 2..];
        let Some(end) = template.find("}}") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let inner = template[..end].trim();
        if let Some((head, body)) = inner.split_once('|') {
            if !structural_template_name(head.trim()) {
                output.push_str(body);
            }
        } else if !structural_template_name(inner) {
            output.push_str(inner);
        }
        rest = &template[end + 2..];
    }
    output.push_str(rest);
    output
}

fn structural_template_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized.starts_with("block ")
        || normalized.starts_with("center")
        || normalized.starts_with("/center")
}

fn strip_angle_tags(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

pub(crate) fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
