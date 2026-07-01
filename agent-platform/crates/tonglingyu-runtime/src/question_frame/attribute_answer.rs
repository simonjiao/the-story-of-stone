#[cfg(test)]
use crate::answer_rules::rationale_followup_policy_for_question;
use crate::{
    EvidenceCard,
    answer_rules::{AttributeAgePolicy, attribute_age_policy},
    normalize_text,
};

use super::{
    AttributeCardSupport, RuntimeQuestionFrame, RuntimeQuestionFrameEntity,
    RuntimeQuestionFramePredicate, contains_any_normalized, normalized_terms, predicate_terms,
};

#[cfg(test)]
const ATTRIBUTE_INTENTS: &[&str] = &["attribute_query", "attribute_at_event", "attribute_compare"];
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgeMention {
    value: AgeValue,
    cue: String,
    source_title: String,
    evidence_id: String,
    score: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgeValue {
    Exact(u8),
    Range(u8, u8),
}

impl AgeValue {
    #[cfg(test)]
    fn lower(self) -> u8 {
        match self {
            Self::Exact(value) => value,
            Self::Range(left, right) => left.min(right),
        }
    }

    #[cfg(test)]
    fn upper(self) -> u8 {
        match self {
            Self::Exact(value) => value,
            Self::Range(left, right) => left.max(right),
        }
    }

    fn display(self) -> String {
        match self {
            Self::Exact(value) => format!("{value}岁"),
            Self::Range(left, right) => format!("{}岁", chinese_adjacent_range(left, right)),
        }
    }
}

#[derive(Debug, Clone)]
struct AgeCue {
    start: usize,
    end: usize,
    value: AgeValue,
    cue: String,
}

#[cfg(test)]
pub(crate) fn attribute_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| ATTRIBUTE_INTENTS.contains(&frame.intent.as_str()))?;
    let predicate = frame.predicate.as_ref()?;
    let subject = frame.subject.as_ref()?;
    if predicate.id == "age" {
        return age_answer(frame, subject, predicate, cards);
    }
    generic_attribute_answer(frame, subject, predicate, cards)
}

#[cfg(test)]
fn age_answer(
    frame: &RuntimeQuestionFrame,
    subject: &RuntimeQuestionFrameEntity,
    predicate: &RuntimeQuestionFramePredicate,
    cards: &[EvidenceCard],
) -> Option<String> {
    let policy = match attribute_age_policy() {
        Ok(policy) => policy,
        Err(_) => return Some("回答规则目录不可用，不能可靠回答年龄问题。".to_string()),
    };
    if frame.intent == "attribute_compare" {
        return age_compare_answer(frame, subject, predicate, cards, &policy);
    }
    let mentions = age_mentions_for_entity(subject, cards, &policy);
    let Some(primary) = mentions.first() else {
        return Some(format!(
            "就这些材料看，还没有直接命中能说明{}“{}”的材料；因此不能把当前材料改写成{}结论。",
            subject.canonical, predicate.label, predicate.label
        ));
    };
    if frame.intent == "attribute_at_event" {
        if let Ok(policy) = rationale_followup_policy_for_question(&frame.canonical_question)
            && policy.applies
            && !policy.rule.trim().is_empty()
        {
            return Some(format!(
                "推理链条是：先把追问绑定回{}的{}问题；再只看这些材料中可追溯的年龄线索；当前最强线索是{}的{}。因此只能把{}说成所问情节附近大约{}上下。因为材料没有给出生日或精确时点，不能进一步说成精确年龄。",
                subject.canonical,
                predicate.label,
                primary.source_title,
                primary.cue,
                subject.canonical,
                primary.value.display()
            ));
        }
        return Some(format!(
            "{}不能只凭所问时间点读成精确生日；当前证据支持的有限推算是：{}有{}，所以只能说{}在所问情节附近大约{}上下，不能说成更精确的定数。",
            frame.canonical_question,
            primary.source_title,
            primary.cue,
            subject.canonical,
            primary.value.display()
        ));
    }
    Some(format!(
        "当前证据可直接支持{}的{}线索：{}有{}。在这个证据范围内，可说{}约为{}；若要精确到生日或另一情节时间点，还需要更多材料。",
        subject.canonical,
        predicate.label,
        primary.source_title,
        primary.cue,
        subject.canonical,
        primary.value.display()
    ))
}

pub(super) fn attribute_card_support(
    frame: &RuntimeQuestionFrame,
    card: &EvidenceCard,
) -> Option<AttributeCardSupport> {
    let frame = frame.is_attribute().then_some(frame)?;
    let subject = frame.subject.as_ref()?;
    let predicate = frame.predicate.as_ref()?;
    if predicate.id == "age" {
        let policy = attribute_age_policy().ok()?;
        let mention = age_mention_for_card(subject, card, &policy)?;
        let mut matched_terms = matched_attribute_terms(subject, predicate, card);
        push_unique_support_term(&mut matched_terms, &mention.cue);
        return Some(AttributeCardSupport {
            claim_value: mention.value.display(),
            matched_terms,
            modality: if frame.intent == "attribute_at_event" {
                "bounded_event_attribute_inference".to_string()
            } else {
                "direct_textual_attribute".to_string()
            },
            evidence_strength: if frame.intent == "attribute_at_event" {
                "inferred".to_string()
            } else {
                "direct".to_string()
            },
        });
    }
    if attribute_card_mentions_entity_and_attribute(subject, predicate, card) {
        return None;
    }
    None
}

#[cfg(test)]
fn age_compare_answer(
    frame: &RuntimeQuestionFrame,
    subject: &RuntimeQuestionFrameEntity,
    predicate: &RuntimeQuestionFramePredicate,
    cards: &[EvidenceCard],
    policy: &AttributeAgePolicy,
) -> Option<String> {
    let Some(object) = frame.object.as_ref() else {
        return Some(format!(
            "这个问题需要补全比较对象，才能判断{}的{}比较。",
            subject.canonical, predicate.label
        ));
    };
    let subject_ages = age_mentions_for_entity(subject, cards, policy);
    let object_ages = age_mentions_for_entity(object, cards, policy);
    match (subject_ages.first(), object_ages.first()) {
        (Some(subject_age), Some(object_age)) => {
            let comparison = if subject_age.value.upper() < object_age.value.lower() {
                format!("{}更大", object.canonical)
            } else if object_age.value.upper() < subject_age.value.lower() {
                format!("{}更大", subject.canonical)
            } else {
                "两者年龄区间有重叠，不能稳定判断谁更大".to_string()
            };
            Some(format!(
                "按这些材料中的年龄线索推算，{}。依据是：{}有{}，{}有{}。这个结论只限于这些年龄线索；若要精确到生日或不同情节时点，还需要更多材料。",
                comparison,
                subject_age.source_title,
                subject_age.cue,
                object_age.source_title,
                object_age.cue
            ))
        }
        (Some(subject_age), None) => Some(format!(
            "当前证据只支持{}的年龄线索：{}有{}；还没有命中能说明{}年龄的对应材料，所以不能稳定比较谁的{}更大。",
            subject.canonical,
            subject_age.source_title,
            subject_age.cue,
            object.canonical,
            predicate.label
        )),
        (None, Some(object_age)) => Some(format!(
            "当前证据只支持{}的年龄线索：{}有{}；还没有命中能说明{}年龄的对应材料，所以不能稳定比较谁的{}更大。",
            object.canonical,
            object_age.source_title,
            object_age.cue,
            subject.canonical,
            predicate.label
        )),
        (None, None) => Some(format!(
            "就这些材料看，还没有同时支持{}和{}“{}”比较的直接材料；因此不能只凭当前命中内容判断谁的{}更大。",
            subject.canonical, object.canonical, predicate.label, predicate.label
        )),
    }
}

fn matched_attribute_terms(
    entity: &RuntimeQuestionFrameEntity,
    predicate: &RuntimeQuestionFramePredicate,
    card: &EvidenceCard,
) -> Vec<String> {
    let combined = format!("{} {}", card.source_title, card.text);
    let normalized = normalize_text(&combined);
    let mut terms = Vec::new();
    for term in entity
        .identity_terms()
        .into_iter()
        .chain(predicate_terms(predicate))
        .chain(predicate.evidence_terms.clone())
    {
        let trimmed = term.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized_term = normalize_text(trimmed);
        if combined.contains(trimmed) || normalized.contains(&normalized_term) {
            push_unique_support_term(&mut terms, trimmed);
        }
    }
    terms
}

fn push_unique_support_term(terms: &mut Vec<String>, term: &str) {
    let term = term.trim();
    if !term.is_empty() && !terms.iter().any(|existing| existing == term) {
        terms.push(term.to_string());
    }
}

#[cfg(test)]
fn generic_attribute_answer(
    frame: &RuntimeQuestionFrame,
    subject: &RuntimeQuestionFrameEntity,
    predicate: &RuntimeQuestionFramePredicate,
    cards: &[EvidenceCard],
) -> Option<String> {
    if frame.intent == "attribute_compare" {
        let Some(object) = frame.object.as_ref() else {
            return Some(format!(
                "这个问题需要补全比较对象，才能判断{}的{}比较。",
                subject.canonical, predicate.label
            ));
        };
        let direct_support = cards.iter().any(|card| {
            attribute_card_mentions_entity_and_attribute(subject, predicate, card)
                && attribute_card_mentions_entity_and_attribute(object, predicate, card)
        });
        if direct_support {
            return Some(format!(
                "就这些材料看，已经命中{}和{}的“{}”相关材料，但还缺少能直接完成比较的并列依据；因此不能只凭当前命中内容判断谁的{}更大。",
                subject.canonical, object.canonical, predicate.label, predicate.label
            ));
        }
        return Some(format!(
            "就这些材料看，还没有同时支持{}和{}“{}”比较的直接材料；因此不能只凭当前命中内容判断谁的{}更大。",
            subject.canonical, object.canonical, predicate.label, predicate.label
        ));
    }
    let direct_support = cards
        .iter()
        .any(|card| attribute_card_mentions_entity_and_attribute(subject, predicate, card));
    if direct_support {
        return Some(format!(
            "就这些材料看，已经命中{}“{}”相关材料，但还不足以直接抽取稳定结论；需要继续补充能直接说明{}的材料。",
            subject.canonical, predicate.label, predicate.label
        ));
    }
    Some(format!(
        "就这些材料看，还没有直接命中能说明{}“{}”的材料；因此不能把当前材料改写成{}结论。",
        subject.canonical, predicate.label, predicate.label
    ))
}

#[cfg(test)]
fn age_mentions_for_entity(
    entity: &RuntimeQuestionFrameEntity,
    cards: &[EvidenceCard],
    policy: &AttributeAgePolicy,
) -> Vec<AgeMention> {
    let mut mentions = cards
        .iter()
        .filter_map(|card| age_mention_for_card(entity, card, policy))
        .collect::<Vec<_>>();
    mentions.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.source_title.cmp(&right.source_title))
            .then_with(|| left.evidence_id.cmp(&right.evidence_id))
    });
    mentions
}

fn age_mention_for_card(
    entity: &RuntimeQuestionFrameEntity,
    card: &EvidenceCard,
    policy: &AttributeAgePolicy,
) -> Option<AgeMention> {
    let text = format!("{} {}", card.source_title, card.text);
    let alias_positions = entity_alias_positions(entity, &text);
    if alias_positions.is_empty() {
        return None;
    }
    age_cues(&text, policy)
        .into_iter()
        .filter_map(|cue| {
            let score = alias_positions
                .iter()
                .filter_map(|position| age_cue_score(&text, *position, &cue, policy))
                .min()?;
            Some(AgeMention {
                value: cue.value,
                cue: format!("“{}”", cue.cue),
                source_title: card.source_title.clone(),
                evidence_id: card.evidence_id.clone(),
                score,
            })
        })
        .min_by(|left, right| left.score.cmp(&right.score))
}

fn entity_alias_positions(entity: &RuntimeQuestionFrameEntity, text: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    for alias in entity.identity_terms() {
        let alias = alias.trim();
        if alias.is_empty() {
            continue;
        }
        positions.extend(text.match_indices(alias).map(|(index, _)| index));
    }
    positions.sort_unstable();
    positions.dedup();
    positions
}

fn age_cue_score(
    text: &str,
    entity_start: usize,
    cue: &AgeCue,
    policy: &AttributeAgePolicy,
) -> Option<usize> {
    let distance = if entity_start <= cue.start {
        cue.start - entity_start
    } else {
        entity_start - cue.end
    };
    if distance > policy.max_entity_age_distance {
        return None;
    }
    let window = text_between(text, entity_start.min(cue.start), entity_start.max(cue.end));
    let boundary_penalty = if window.chars().any(is_strong_boundary) {
        120
    } else {
        0
    };
    let direction_penalty = if cue.start >= entity_start { 0 } else { 40 };
    Some(distance + boundary_penalty + direction_penalty)
}

fn text_between(text: &str, start: usize, end: usize) -> &str {
    text.get(start..end).unwrap_or("")
}

fn is_strong_boundary(ch: char) -> bool {
    matches!(ch, '。' | '？' | '?' | '！' | '!' | '\n' | '\r')
}

fn age_cues(text: &str, policy: &AttributeAgePolicy) -> Vec<AgeCue> {
    let mut cues = Vec::new();
    for (token, left, right) in adjacent_age_tokens() {
        push_age_cues(text, token, AgeValue::Range(left, right), policy, &mut cues);
    }
    push_age_cues(text, "十来", AgeValue::Range(10, 13), policy, &mut cues);
    push_age_cues(text, "十來", AgeValue::Range(10, 13), policy, &mut cues);
    for (value, tokens) in age_number_tokens() {
        for token in tokens {
            push_exact_age_cues(text, token, value, policy, &mut cues);
        }
    }
    cues.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| right.end.cmp(&left.end))
    });
    cues
}

fn push_exact_age_cues(
    text: &str,
    token: &str,
    value: u8,
    policy: &AttributeAgePolicy,
    cues: &mut Vec<AgeCue>,
) {
    for suffix in ["岁", "歲"] {
        let needle = format!("{token}{suffix}");
        for (start, _) in text.match_indices(&needle) {
            if preceding_char(text, start).is_some_and(is_chinese_number_char) {
                continue;
            }
            cues.push(AgeCue {
                start,
                end: start + needle.len(),
                value: AgeValue::Exact(value),
                cue: expanded_age_cue(text, start, &needle, &policy.cue_prefix_terms),
            });
        }
    }
}

fn push_age_cues(
    text: &str,
    token: &str,
    value: AgeValue,
    policy: &AttributeAgePolicy,
    cues: &mut Vec<AgeCue>,
) {
    for suffix in ["岁", "歲"] {
        let needle = format!("{token}{suffix}");
        for (start, _) in text.match_indices(&needle) {
            cues.push(AgeCue {
                start,
                end: start + needle.len(),
                value,
                cue: expanded_age_cue(text, start, &needle, &policy.cue_prefix_terms),
            });
        }
    }
}

fn expanded_age_cue(text: &str, start: usize, needle: &str, prefix_terms: &[String]) -> String {
    for prefix in prefix_terms {
        let Some(prefix_start) = start.checked_sub(prefix.len()) else {
            continue;
        };
        if text.get(prefix_start..start) == Some(prefix.as_str()) {
            return format!("{prefix}{needle}");
        }
    }
    needle.to_string()
}

fn preceding_char(text: &str, byte_index: usize) -> Option<char> {
    text.get(..byte_index)?.chars().next_back()
}

fn is_chinese_number_char(ch: char) -> bool {
    matches!(
        ch,
        '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '兩' | '两'
    )
}

fn adjacent_age_tokens() -> Vec<(&'static str, u8, u8)> {
    vec![
        ("一二", 1, 2),
        ("二三", 2, 3),
        ("兩三", 2, 3),
        ("两三", 2, 3),
        ("三四", 3, 4),
        ("四五", 4, 5),
        ("五六", 5, 6),
        ("六七", 6, 7),
        ("七八", 7, 8),
        ("八九", 8, 9),
        ("九十", 9, 10),
    ]
}

fn age_number_tokens() -> Vec<(u8, Vec<&'static str>)> {
    vec![
        (1, vec!["一", "1"]),
        (2, vec!["二", "两", "兩", "2"]),
        (3, vec!["三", "3"]),
        (4, vec!["四", "4"]),
        (5, vec!["五", "5"]),
        (6, vec!["六", "6"]),
        (7, vec!["七", "7"]),
        (8, vec!["八", "8"]),
        (9, vec!["九", "9"]),
        (10, vec!["十", "10"]),
        (11, vec!["十一", "11"]),
        (12, vec!["十二", "12"]),
        (13, vec!["十三", "13"]),
        (14, vec!["十四", "14"]),
        (15, vec!["十五", "15"]),
        (16, vec!["十六", "16"]),
        (17, vec!["十七", "17"]),
        (18, vec!["十八", "18"]),
        (19, vec!["十九", "19"]),
        (20, vec!["二十", "20"]),
    ]
}

fn chinese_adjacent_range(left: u8, right: u8) -> String {
    format!(
        "{}{}",
        chinese_digit(left).unwrap_or_else(|| left.to_string()),
        chinese_digit(right).unwrap_or_else(|| right.to_string())
    )
}

fn chinese_digit(value: u8) -> Option<String> {
    Some(
        match value {
            1 => "一",
            2 => "二",
            3 => "三",
            4 => "四",
            5 => "五",
            6 => "六",
            7 => "七",
            8 => "八",
            9 => "九",
            10 => "十",
            _ => return None,
        }
        .to_string(),
    )
}

fn attribute_card_mentions_entity_and_attribute(
    entity: &RuntimeQuestionFrameEntity,
    predicate: &RuntimeQuestionFramePredicate,
    card: &EvidenceCard,
) -> bool {
    let normalized = normalize_text(&format!("{} {}", card.source_title, card.text));
    contains_any_normalized(&normalized, &normalized_terms(&entity.identity_terms()))
        && contains_any_normalized(
            &normalized,
            &normalized_terms(
                &predicate_terms(predicate)
                    .into_iter()
                    .chain(predicate.evidence_terms.clone())
                    .collect::<Vec<_>>(),
            ),
        )
}
