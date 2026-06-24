use crate::{
    EvidenceCard,
    answer_composer::public_quote_text,
    answer_rules::{EntityIntroPolicy, entity_intro_policy},
    normalize_text,
    question_frame::{RuntimeQuestionFrame, RuntimeQuestionFrameEntity},
    retrieval_rules,
    upstream_bundle::{evidence_card_is_later_forty, evidence_card_source_layer},
};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
struct EntityIntroCandidate<'a> {
    card: &'a EvidenceCard,
    quote: String,
    substantive_chars: usize,
    source_rank: usize,
    canonical_match: bool,
    alias_match_count: usize,
    short_shell: bool,
    index: usize,
}

#[derive(Debug, Clone)]
struct EntityFateCandidate<'a> {
    card: &'a EvidenceCard,
    quote: String,
    score: i64,
    signature: BTreeSet<String>,
    index: usize,
}

pub(crate) fn compose_entity_intro_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.intent == "entity_query")?;
    let entity = frame.subject.as_ref().or(frame.object.as_ref())?;
    let policy = match entity_intro_policy() {
        Ok(policy) => policy,
        Err(_) => {
            return Some("治理规则目录不可用，不能可靠回答这个人物介绍问题。".to_string());
        }
    };
    if entity_intro_blocked_by_question(&frame.canonical_question, &policy) {
        return Some(
            "请明确要定位的具体情节、诗文或文本线索；当前问法不是人物介绍，不能改答成人物概括。"
                .to_string(),
        );
    }
    if let Some(answer) = compose_entity_fate_answer(Some(frame), entity, cards, &policy) {
        return Some(answer);
    }
    let candidates = entity_intro_candidates(entity, cards, &policy);
    if candidates.is_empty() {
        return Some(format!(
            "就当前证据包看，没有命中关于{}的直接材料，不能可靠概括这个人物。",
            entity.canonical
        ));
    }

    let readable = candidates
        .iter()
        .filter(|candidate| !candidate.short_shell)
        .cloned()
        .collect::<Vec<_>>();
    if readable.is_empty() {
        return Some(format!(
            "就当前证据包看，只命中关于{}的过短片段，如{}：{}；这些材料不足以可靠概括这个人物。",
            entity.canonical, candidates[0].card.source_title, candidates[0].quote
        ));
    }

    let selected = readable
        .into_iter()
        .take(policy.max_supporting_cards)
        .collect::<Vec<_>>();
    let evidence_text = selected
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "{}. {}：{}",
                index + 1,
                candidate.card.source_title,
                candidate.quote
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let coverage = if selected.len() >= policy.min_supporting_cards {
        "这些材料能支持其出场、称呼或相关人物关系；更完整的性格、命运或结局概括，还需要继续命中对应情节。"
    } else {
        "当前可读材料仍偏少，只能支持有限定位；更完整的性格、命运或结局概括，还需要继续命中对应情节。"
    };

    Some(format!(
        "就当前证据包看，{}可以先按命中材料作有边界的介绍：\n{}\n{}",
        entity.canonical, evidence_text, coverage
    ))
}

fn compose_entity_fate_answer(
    frame: Option<&RuntimeQuestionFrame>,
    entity: &RuntimeQuestionFrameEntity,
    cards: &[EvidenceCard],
    policy: &EntityIntroPolicy,
) -> Option<String> {
    let frame = frame?;
    if !question_asks_for_fate(&frame.canonical_question) {
        return None;
    }
    let mut candidates = entity_fate_candidates(entity, cards, policy);
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|candidate| (std::cmp::Reverse(candidate.score), candidate.index));
    let mut selected = Vec::new();
    let mut selected_signatures = Vec::new();
    let limit = policy.max_supporting_cards.min(3);
    for candidate in candidates {
        if selected_signatures
            .iter()
            .any(|signature| fate_candidate_duplicate(signature, &candidate.signature))
        {
            continue;
        }
        selected_signatures.push(candidate.signature.clone());
        selected.push(candidate);
        if selected.len() >= limit {
            break;
        }
    }
    if selected.is_empty() {
        return None;
    }
    let has_later_forty = selected
        .iter()
        .any(|candidate| evidence_card_is_later_forty(candidate.card));
    let mut answer = String::new();
    if has_later_forty {
        answer.push_str(&policy.fate_later_forty_opening);
    } else {
        answer.push_str(&policy.fate_default_opening);
    }
    for (index, candidate) in selected.iter().enumerate() {
        let index = (index + 1).to_string();
        answer.push_str(&render_entity_intro_template(
            &policy.fate_evidence_item_template,
            &[
                ("{index}", &index),
                ("{source_title}", &candidate.card.source_title),
                ("{quote}", &candidate.quote),
            ],
        ));
    }
    if has_later_forty {
        answer.push_str(&policy.fate_later_forty_boundary);
    } else {
        answer.push_str(&policy.fate_default_boundary);
    }
    Some(answer)
}

fn render_entity_intro_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, value);
    }
    rendered
}

fn entity_intro_blocked_by_question(question: &str, policy: &EntityIntroPolicy) -> bool {
    let normalized_question = normalize_text(question);
    policy.blocked_question_terms.iter().any(|term| {
        let term = term.trim();
        !term.is_empty()
            && (question.contains(term) || normalized_question.contains(&normalize_text(term)))
    })
}

fn entity_fate_candidates<'a>(
    entity: &RuntimeQuestionFrameEntity,
    cards: &'a [EvidenceCard],
    policy: &EntityIntroPolicy,
) -> Vec<EntityFateCandidate<'a>> {
    let terms = normalized_identity_terms(entity);
    let raw_identity_terms = entity.identity_terms();
    let Ok(ranking) = retrieval_rules::ranking_rules() else {
        return Vec::new();
    };
    cards
        .iter()
        .enumerate()
        .filter_map(|(index, card)| {
            let normalized_text = normalize_text(&card.text);
            let normalized_title = normalize_text(&card.source_title);
            let alias_match_count = terms
                .iter()
                .filter(|term| normalized_text.contains(*term) || normalized_title.contains(*term))
                .count();
            if alias_match_count == 0 {
                return None;
            }
            let fate_match_count = matching_rule_term_count(&card.text, &ranking.fate_text_terms);
            if fate_match_count == 0 {
                return None;
            }
            let quote = entity_fate_excerpt(
                &card.text,
                &raw_identity_terms,
                &terms,
                &ranking.fate_text_terms,
                policy.max_quote_chars.saturating_mul(3).clamp(120, 220),
            );
            if quote_has_excluded_terms(&quote, policy) || quote.trim().is_empty() {
                return None;
            }
            if !quote_contains_identity_and_fate(&quote, &terms, &ranking.fate_text_terms) {
                return None;
            }
            let source_rank =
                retrieval_rules::source_layer_answer_rank(evidence_card_source_layer(card))
                    .unwrap_or(usize::MAX) as i64;
            let score =
                (fate_match_count as i64 * 40) + (alias_match_count as i64 * 12) - source_rank;
            let signature = text_shingles(&compact_text_signature(&quote), 4);
            Some(EntityFateCandidate {
                card,
                quote,
                score,
                signature,
                index,
            })
        })
        .collect()
}

fn entity_fate_excerpt(
    text: &str,
    raw_identity_terms: &[String],
    identity_terms: &[String],
    fate_terms: &[String],
    limit: usize,
) -> String {
    let clean = public_quote_text(text);
    for group in [raw_identity_terms, fate_terms] {
        let mut focus_terms = group
            .iter()
            .map(|term| term.trim())
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        focus_terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
        for term in focus_terms {
            if clean.contains(term) {
                let quote = trim_chars_around(&clean, term, limit);
                if quote_contains_identity_and_fate(&quote, identity_terms, fate_terms) {
                    return quote;
                }
            }
        }
    }
    trim_chars(&clean, limit)
}

fn quote_contains_identity_and_fate(
    quote: &str,
    identity_terms: &[String],
    fate_terms: &[String],
) -> bool {
    let normalized = normalize_text(quote);
    identity_terms
        .iter()
        .any(|term| normalized.contains(&normalize_text(term)))
        && matching_rule_term_count(quote, fate_terms) > 0
}

fn fate_candidate_duplicate(left: &BTreeSet<String>, right: &BTreeSet<String>) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let common = left.intersection(right).count();
    let min_len = left.len().min(right.len());
    min_len >= 4 && common * 100 >= min_len * 60
}

fn question_asks_for_fate(question: &str) -> bool {
    retrieval_rules::ranking_rules()
        .map(|ranking| retrieval_rules::contains_any_term(question, &ranking.fate_question_terms))
        .unwrap_or(false)
}

pub(crate) fn entity_intro_answer_policy_value(
    frame: Option<&RuntimeQuestionFrame>,
) -> anyhow::Result<serde_json::Value> {
    let Some(frame) = frame.filter(|frame| frame.intent == "entity_query") else {
        return Ok(serde_json::json!({}));
    };
    let Some(entity) = frame.subject.as_ref().or(frame.object.as_ref()) else {
        return Ok(serde_json::json!({}));
    };
    let policy = entity_intro_policy()?;
    Ok(serde_json::json!({
        "object": "tonglingyu.entity_intro_answer_policy",
        "schema_version": "tonglingyu.entity_intro_answer_policy.v1",
        "applies": true,
        "entity": {
            "canonical": &entity.canonical,
            "aliases": entity.identity_terms(),
        },
        "min_supporting_cards": policy.min_supporting_cards,
        "max_supporting_cards": policy.max_supporting_cards,
        "max_quote_chars": policy.max_quote_chars,
        "min_substantive_chars": policy.min_substantive_chars,
        "short_speech_shell_max_chars": policy.short_speech_shell_max_chars,
        "excluded_public_quote_terms": policy.excluded_public_quote_terms,
        "rule": policy.rule,
    }))
}

fn entity_intro_candidates<'a>(
    entity: &RuntimeQuestionFrameEntity,
    cards: &'a [EvidenceCard],
    policy: &EntityIntroPolicy,
) -> Vec<EntityIntroCandidate<'a>> {
    let terms = normalized_identity_terms(entity);
    let canonical = normalize_text(&entity.canonical);
    let mut seen = BTreeSet::new();
    let mut candidates = cards
        .iter()
        .enumerate()
        .filter_map(|(index, card)| {
            let normalized_text = normalize_text(&card.text);
            let normalized_title = normalize_text(&card.source_title);
            let matched_terms = terms
                .iter()
                .filter(|term| normalized_text.contains(*term) || normalized_title.contains(*term))
                .collect::<Vec<_>>();
            if matched_terms.is_empty() {
                return None;
            }
            let quote = trim_chars(&public_quote_text(&card.text), policy.max_quote_chars);
            if quote_has_excluded_terms(&quote, policy) {
                return None;
            }
            let substantive_chars = substantive_char_count(&quote);
            let short_shell = substantive_chars <= policy.short_speech_shell_max_chars
                && retrieval_rules::evidence_text_is_broken_shell(&quote, substantive_chars)
                    .unwrap_or(true);
            let signature = compact_text_signature(&quote);
            if signature.is_empty() || !seen.insert(signature.clone()) {
                return None;
            }
            Some(EntityIntroCandidate {
                card,
                quote,
                substantive_chars,
                source_rank: retrieval_rules::source_layer_answer_rank(evidence_card_source_layer(
                    card,
                ))
                .unwrap_or(usize::MAX),
                canonical_match: !canonical.is_empty()
                    && (normalized_text.contains(&canonical)
                        || normalized_title.contains(&canonical)),
                alias_match_count: matched_terms.len(),
                short_shell,
                index,
            })
        })
        .collect::<Vec<_>>();

    candidates.sort_by_key(|candidate| {
        (
            candidate.short_shell,
            candidate.source_rank,
            std::cmp::Reverse(candidate.canonical_match),
            std::cmp::Reverse(candidate.alias_match_count),
            std::cmp::Reverse(candidate.substantive_chars >= policy.min_substantive_chars),
            candidate.index,
        )
    });
    candidates
}

fn normalized_identity_terms(entity: &RuntimeQuestionFrameEntity) -> Vec<String> {
    entity
        .identity_terms()
        .into_iter()
        .map(|term| normalize_text(&term))
        .filter(|term| !term.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn trim_chars(text: &str, limit: usize) -> String {
    let mut output = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        output.push('…');
    }
    output
}

fn trim_chars_around(text: &str, term: &str, limit: usize) -> String {
    if limit == 0 {
        return text.to_string();
    }
    let Some(byte_index) = text.find(term) else {
        return trim_chars(text, limit);
    };
    let term_start = text[..byte_index].chars().count();
    let term_len = term.chars().count();
    let available = limit.saturating_sub(term_len).max(8);
    let before_context = (available / 16).max(6);
    if text.chars().count() <= limit && term_start <= before_context {
        return text.to_string();
    }
    let after_context = available.saturating_sub(before_context);
    let start = term_start.saturating_sub(before_context);
    let end = (term_start + term_len + after_context).min(text.chars().count());
    let mut output = String::new();
    if start > 0 {
        output.push('…');
    }
    output.extend(text.chars().skip(start).take(end - start));
    if end < text.chars().count() {
        output.push('…');
    }
    output
}

fn quote_has_excluded_terms(text: &str, policy: &EntityIntroPolicy) -> bool {
    let normalized = normalize_text(text);
    policy.excluded_public_quote_terms.iter().any(|term| {
        let term = term.trim();
        !term.is_empty() && (text.contains(term) || normalized.contains(&normalize_text(term)))
    })
}

fn substantive_char_count(text: &str) -> usize {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && !text_punctuation(*ch))
        .count()
}

fn matching_rule_term_count(text: &str, terms: &[String]) -> usize {
    let normalized = normalize_text(text);
    terms
        .iter()
        .filter(|term| {
            let term = term.trim();
            !term.is_empty() && (text.contains(term) || normalized.contains(&normalize_text(term)))
        })
        .count()
}

fn compact_text_signature(text: &str) -> String {
    normalize_text(text)
        .chars()
        .filter(|ch| !ch.is_whitespace() && !text_punctuation(*ch))
        .collect()
}

fn text_shingles(text: &str, width: usize) -> BTreeSet<String> {
    let chars = text.chars().collect::<Vec<_>>();
    if width == 0 || chars.len() < width {
        return BTreeSet::new();
    }
    chars
        .windows(width)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn text_punctuation(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '，' | '。'
                | '：'
                | '；'
                | '、'
                | '？'
                | '！'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '《'
                | '》'
                | '【'
                | '】'
                | '…'
        )
}

#[cfg(test)]
mod tests;
