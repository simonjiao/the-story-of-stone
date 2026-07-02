use crate::{
    EvidenceCard,
    answer_composer::public_quote_text,
    answer_requirements::public_source_cues_for_title,
    answer_rules::{ChapterLocationPolicy, chapter_location_policy},
    extract_chapter_no, normalize_text, query_expansion_search_terms,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::{RuntimeQuestionFrame, RuntimeQuestionFrameEntity};

#[derive(Debug, Clone)]
struct ChapterLocationCandidate<'a> {
    card: &'a EvidenceCard,
    chapter_no: i64,
    quote: String,
    chapter_title: Option<String>,
    score: i64,
    kind: ChapterLocationCandidateKind,
    death_role: Option<DeathChapterEvidenceRole>,
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChapterLocationCandidateKind {
    ChapterTitle,
    BaseText,
    Commentary,
    WeakMention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeathChapterEvidenceRole {
    Occurrence,
    Confirmation,
    Memorial,
}

#[derive(Debug, Clone)]
struct ChapterLocationSelection<'a> {
    chapter_no: i64,
    title: Option<String>,
    base: Option<ChapterLocationCandidate<'a>>,
    commentary: Option<ChapterLocationCandidate<'a>>,
    death_occurrence: Option<ChapterLocationCandidate<'a>>,
    death_confirmation: Option<ChapterLocationCandidate<'a>>,
    death_memorial: Option<ChapterLocationCandidate<'a>>,
    best: ChapterLocationCandidate<'a>,
    support_count: usize,
}

pub(crate) fn chapter_location_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.is_chapter_location())?;
    let policy = match chapter_location_policy() {
        Ok(policy) => policy,
        Err(_) => {
            return Some("回答规则目录不可用，不能可靠回答这个章回定位问题。".to_string());
        }
    };
    let terms = chapter_location_answer_terms(frame);
    if terms.is_empty() {
        return Some(
            "请说明要定位的是哪个具体情节、诗文或文本线索；当前问题只问回目，但缺少可定位对象。"
                .to_string(),
        );
    }
    let event_label = chapter_location_event_label(frame, &terms);
    if is_death_chapter_location(frame)
        && let Some(answer) = death_fate_boundary_answer(frame, cards)
    {
        return Some(answer);
    }
    let mut candidates = chapter_location_candidates(frame, cards, &terms, &policy);
    if candidates.is_empty() {
        return Some(render_chapter_location_template(
            &policy.no_evidence_template,
            &event_label,
            None,
            None,
            None,
            None,
        ));
    }
    candidates.sort_by_key(|candidate| (std::cmp::Reverse(candidate.score), candidate.index));
    let selections = ranked_chapter_location_selections(candidates, event_label.clone());
    if let Some(answer) =
        render_death_chapter_location_answer(frame, &selections, &event_label, &policy)
    {
        return Some(answer);
    }
    let Some(selection) = dominant_chapter_selection(&selections, &policy) else {
        return Some(ambiguous_chapter_location_answer(
            &selections,
            &event_label,
            &policy,
        ));
    };
    if selection.base.is_none() && selection.title.is_none() && selection.commentary.is_none() {
        return Some(ambiguous_chapter_location_answer(
            &selections,
            &event_label,
            &policy,
        ));
    }
    Some(render_chapter_location_answer(
        selection,
        &event_label,
        &policy,
    ))
}

pub(crate) fn chapter_location_answer_requirement_value(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<Value> {
    let frame = frame.filter(|frame| frame.is_chapter_location())?;
    let policy = chapter_location_policy().ok()?;
    let terms = chapter_location_answer_terms(frame);
    if terms.is_empty() {
        return Some(json!({
            "answer_shape": "clarify_missing_event",
            "rule": policy.rule,
        }));
    }
    let event_label = chapter_location_event_label(frame, &terms);
    if is_death_chapter_location(frame)
        && let Some(requirements) = death_fate_boundary_answer_requirement_value(frame, cards)
    {
        return Some(requirements);
    }
    let mut candidates = chapter_location_candidates(frame, cards, &terms, &policy);
    if candidates.is_empty() {
        return Some(json!({
            "event": event_label,
            "answer_shape": "clarify_no_chapter_location_evidence",
            "rule": policy.rule,
        }));
    }
    candidates.sort_by_key(|candidate| (std::cmp::Reverse(candidate.score), candidate.index));
    let selections = ranked_chapter_location_selections(candidates, event_label.clone());
    if let Some(requirements) =
        death_chapter_location_answer_requirement_value(frame, &selections, &event_label, &policy)
    {
        return Some(requirements);
    }
    let selection = dominant_chapter_selection(&selections, &policy)?;
    let primary = primary_chapter_location_candidate(selection);
    Some(json!({
        "event": event_label,
        "must_answer_chapter_no": selection.chapter_no,
        "chapter_title": selection.title,
        "primary_evidence_id": primary.map(|candidate| candidate.card.evidence_id.clone()),
        "primary_source_title": primary.map(|candidate| candidate.card.source_title.clone()),
        "primary_source_cues": primary
            .and_then(|candidate| public_source_cues_for_title(&candidate.card.source_title).ok())
            .unwrap_or_default(),
        "primary_text_cue": primary.map(|candidate| candidate.quote.clone()),
        "answer_shape": "direct_chapter_first_then_optional_title_then_short_evidence",
        "commentary_visibility_rule": "Do not cite commentary unless the quoted commentary itself explains the chapter/title/location relation.",
        "rule": policy.rule,
    }))
}

pub(crate) fn chapter_location_draft_rejection_reason(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
    draft: &str,
) -> Option<&'static str> {
    let frame = frame.filter(|frame| frame.is_chapter_location())?;
    let policy = chapter_location_policy().ok()?;
    let terms = chapter_location_answer_terms(frame);
    if terms.is_empty() {
        return None;
    }
    let event_label = chapter_location_event_label(frame, &terms);
    let mut candidates = chapter_location_candidates(frame, cards, &terms, &policy);
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|candidate| (std::cmp::Reverse(candidate.score), candidate.index));
    let selections = ranked_chapter_location_selections(candidates, event_label);
    if let Some(primary_chapter_no) =
        death_chapter_primary_selection(frame, &selections).map(|selection| selection.chapter_no)
    {
        let normalized_draft = normalize_text(draft);
        if !chapter_number_mentioned(&normalized_draft, primary_chapter_no) {
            return Some("chapter_location_chapter_number_missing");
        }
        return None;
    }
    let selection = dominant_chapter_selection(&selections, &policy)?;
    let normalized_draft = normalize_text(draft);
    if !chapter_number_mentioned(&normalized_draft, selection.chapter_no) {
        return Some("chapter_location_chapter_number_missing");
    }
    if let Some(title) = &selection.title {
        let title_cues = chapter_title_cues(title);
        if !title_cues.is_empty()
            && !title_cues
                .iter()
                .any(|cue| normalized_draft.contains(&normalize_text(cue)))
        {
            return Some("chapter_location_title_cue_missing");
        }
    }
    None
}

pub(crate) fn chapter_location_evidence_ids_for_requirements(value: &Value) -> Vec<String> {
    value
        .get("chapter_location")
        .and_then(|item| item.get("primary_evidence_id"))
        .and_then(Value::as_str)
        .map(|id| vec![id.to_string()])
        .unwrap_or_default()
}

pub(crate) fn chapter_location_focus_terms(frame: &RuntimeQuestionFrame) -> Vec<String> {
    let mut residual = normalize_text(&frame.canonical_question);
    for entity in [frame.subject.as_ref(), frame.object.as_ref()]
        .into_iter()
        .flatten()
    {
        remove_entity_terms(&mut residual, entity);
    }
    for term in CHAPTER_LOCATION_NOISE_TERMS {
        let normalized = normalize_text(term);
        if !normalized.is_empty() {
            residual = residual.replace(&normalized, "");
        }
    }
    let compact = compact_question_text(&residual);
    let mut terms = Vec::new();
    if compact.chars().count() >= 2 {
        terms.push(compact);
    }
    extend_unique(&mut terms, std::slice::from_ref(&frame.canonical_question));
    terms
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect()
}

fn chapter_location_answer_terms(frame: &RuntimeQuestionFrame) -> Vec<String> {
    let mut terms = chapter_location_event_cue_terms(frame);
    extend_unique(&mut terms, &chapter_location_focus_terms(frame));
    if let Ok(expanded) = query_expansion_search_terms(&frame.canonical_question) {
        extend_unique(&mut terms, &expanded);
    }
    terms
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect()
}

fn chapter_location_event_cue_terms(frame: &RuntimeQuestionFrame) -> Vec<String> {
    let normalized = normalize_text(&frame.canonical_question);
    if !["死", "去世", "亡故", "病逝", "夭逝", "长逝", "長逝"]
        .iter()
        .any(|term| normalized.contains(&normalize_text(term)))
    {
        return Vec::new();
    }
    [
        "蕭然長逝",
        "萧然长逝",
        "長逝",
        "长逝",
        "夭逝",
        "病逝",
        "亡故",
        "去世",
        "死",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

fn chapter_location_event_label(frame: &RuntimeQuestionFrame, terms: &[String]) -> String {
    let event = terms
        .iter()
        .find(|term| *term != &frame.canonical_question)
        .or_else(|| terms.first())
        .cloned()
        .unwrap_or_else(|| frame.canonical_question.clone());
    if let Some(subject) = &frame.subject
        && !normalize_text(&event).contains(&normalize_text(&subject.canonical))
    {
        return format!("{}{}", subject.canonical, event);
    }
    event
}

fn ambiguous_chapter_location_answer(
    selections: &[ChapterLocationSelection<'_>],
    event_label: &str,
    policy: &ChapterLocationPolicy,
) -> String {
    let locations = selections
        .iter()
        .take(3)
        .map(chapter_location_selection_label)
        .collect::<Vec<_>>()
        .join("；");
    render_chapter_location_template(
        &policy.ambiguous_template,
        event_label,
        None,
        None,
        None,
        Some(&locations),
    )
}

fn render_chapter_location_answer(
    selection: &ChapterLocationSelection<'_>,
    event_label: &str,
    policy: &ChapterLocationPolicy,
) -> String {
    let mut sentences = vec![render_chapter_location_direct_answer(
        event_label,
        selection.chapter_no,
        policy,
    )];
    if let Some(title) = &selection.title {
        sentences.push(render_chapter_location_template(
            &policy.chapter_title_template,
            event_label,
            Some(selection.chapter_no),
            Some(title),
            None,
            None,
        ));
    }
    if let Some(base) = &selection.base {
        sentences.push(render_chapter_location_template(
            &policy.base_evidence_template,
            event_label,
            Some(selection.chapter_no),
            selection.title.as_deref(),
            Some(&base.quote),
            None,
        ));
    } else if let Some(commentary) = &selection.commentary {
        sentences.push(render_chapter_location_template(
            &policy.commentary_evidence_template,
            event_label,
            Some(selection.chapter_no),
            selection.title.as_deref(),
            Some(&commentary.quote),
            None,
        ));
    }
    sentences.join("")
}

fn render_death_chapter_location_answer(
    frame: &RuntimeQuestionFrame,
    selections: &[ChapterLocationSelection<'_>],
    _event_label: &str,
    _policy: &ChapterLocationPolicy,
) -> Option<String> {
    if !is_death_chapter_location(frame) {
        return None;
    }
    let subject = frame.subject.as_ref()?.canonical.as_str();
    let occurrence = death_chapter_occurrence_selection(selections);
    let confirmation = death_chapter_confirmation_selection(selections);
    let memorial = death_chapter_memorial_selection(selections);
    if let Some(selection) = occurrence {
        let mut answer = format!(
            "{subject}的死亡情节应归在《红楼梦》第{}回。",
            selection.chapter_no
        );
        if let Some(card) = selection
            .death_occurrence
            .as_ref()
            .or(selection.base.as_ref())
            .or(Some(&selection.best))
        {
            answer.push_str(&format!("正文依据可见“{}”。", card.quote));
        }
        if let Some(confirm) = confirmation
            && confirm.chapter_no != selection.chapter_no
        {
            answer.push_str(&format!(
                "第{}回又集中确认死讯或处理身后事。",
                confirm.chapter_no
            ));
            if let Some(card) = confirm
                .death_confirmation
                .as_ref()
                .or(confirm.base.as_ref())
                .or(Some(&confirm.best))
            {
                answer.push_str(&format!("相关依据是“{}”。", card.quote));
            }
        }
        if let Some(memorial) = memorial
            && memorial.chapter_no != selection.chapter_no
            && confirmation
                .map(|confirm| confirm.chapter_no != memorial.chapter_no)
                .unwrap_or(true)
        {
            answer.push_str(&format!(
                "第{}回属于祭奠、诔文或悼亡展开，不宜改作死亡发生回。",
                memorial.chapter_no
            ));
        }
        return Some(answer);
    }
    if let Some(selection) = confirmation {
        let mut answer = format!(
            "当前证据能确认{}的死讯在《红楼梦》第{}回出现；但这只能说明死讯确认或身后事处理的回目，不能把它直接说成死亡发生回。",
            subject, selection.chapter_no
        );
        if let Some(card) = selection
            .death_confirmation
            .as_ref()
            .or(selection.base.as_ref())
            .or(Some(&selection.best))
        {
            answer.push_str(&format!("正文依据可见“{}”。", card.quote));
        }
        return Some(answer);
    }
    if let Some(selection) = memorial {
        let mut answer = format!(
            "当前证据只命中《红楼梦》第{}回的祭奠、诔文或悼亡材料，不能据此回答{}“第几回死”。",
            selection.chapter_no, subject
        );
        if let Some(card) = selection
            .death_memorial
            .as_ref()
            .or(selection.base.as_ref())
            .or(Some(&selection.best))
        {
            answer.push_str(&format!("可见材料是“{}”。", card.quote));
        }
        return Some(answer);
    }
    None
}

fn death_chapter_location_answer_requirement_value(
    frame: &RuntimeQuestionFrame,
    selections: &[ChapterLocationSelection<'_>],
    event_label: &str,
    policy: &ChapterLocationPolicy,
) -> Option<Value> {
    if !is_death_chapter_location(frame) {
        return None;
    }
    let occurrence = death_chapter_occurrence_selection(selections);
    let confirmation = death_chapter_confirmation_selection(selections);
    let memorial = death_chapter_memorial_selection(selections);
    if let Some(selection) = occurrence {
        let primary = selection
            .death_occurrence
            .as_ref()
            .or(selection.base.as_ref())
            .or(Some(&selection.best));
        return Some(json!({
            "event": event_label,
            "must_answer_chapter_no": selection.chapter_no,
            "primary_evidence_id": primary.map(|candidate| candidate.card.evidence_id.clone()),
            "primary_source_title": primary.map(|candidate| candidate.card.source_title.clone()),
            "primary_text_cue": primary.map(|candidate| candidate.quote.clone()),
            "answer_shape": "death_occurrence_chapter_first_then_confirmation_boundary",
            "confirmation_chapter_no": confirmation.map(|selection| selection.chapter_no),
            "memorial_chapter_no": memorial.map(|selection| selection.chapter_no),
            "rule": policy.rule,
        }));
    }
    if let Some(selection) = confirmation {
        let primary = selection
            .death_confirmation
            .as_ref()
            .or(selection.base.as_ref())
            .or(Some(&selection.best));
        return Some(json!({
            "event": event_label,
            "must_answer_chapter_no": selection.chapter_no,
            "primary_evidence_id": primary.map(|candidate| candidate.card.evidence_id.clone()),
            "primary_source_title": primary.map(|candidate| candidate.card.source_title.clone()),
            "primary_text_cue": primary.map(|candidate| candidate.quote.clone()),
            "answer_shape": "death_confirmation_chapter_with_occurrence_boundary",
            "rule": policy.rule,
        }));
    }
    if let Some(selection) = memorial {
        let primary = selection
            .death_memorial
            .as_ref()
            .or(selection.base.as_ref())
            .or(Some(&selection.best));
        return Some(json!({
            "event": event_label,
            "primary_evidence_id": primary.map(|candidate| candidate.card.evidence_id.clone()),
            "primary_source_title": primary.map(|candidate| candidate.card.source_title.clone()),
            "primary_text_cue": primary.map(|candidate| candidate.quote.clone()),
            "answer_shape": "death_memorial_only_no_death_chapter",
            "rule": policy.rule,
        }));
    }
    None
}

fn render_chapter_location_direct_answer(
    event_label: &str,
    chapter_no: i64,
    policy: &ChapterLocationPolicy,
) -> String {
    if let Some(subject) = chapter_location_death_subject(event_label) {
        return format!("{subject}死于《红楼梦》第{chapter_no}回。");
    }
    render_chapter_location_template(
        &policy.direct_answer_template,
        event_label,
        Some(chapter_no),
        None,
        None,
        None,
    )
}

fn chapter_location_death_subject(event_label: &str) -> Option<String> {
    let mut subject = event_label.trim().to_string();
    let mut matched = false;
    for term in [
        "蕭然長逝",
        "萧然长逝",
        "長逝",
        "长逝",
        "病逝",
        "亡故",
        "去世",
        "死",
    ] {
        if subject.contains(term) {
            matched = true;
        }
        subject = subject.replace(term, "");
    }
    if !matched {
        return None;
    }
    let subject = subject.trim();
    if subject.is_empty() {
        None
    } else {
        Some(subject.to_string())
    }
}

fn render_chapter_location_template(
    template: &str,
    event: &str,
    chapter_no: Option<i64>,
    chapter_title: Option<&str>,
    quote: Option<&str>,
    locations: Option<&str>,
) -> String {
    template
        .replace("{event}", event)
        .replace(
            "{chapter_no}",
            &chapter_no.map_or_else(String::new, |value| value.to_string()),
        )
        .replace("{chapter_title}", chapter_title.unwrap_or(""))
        .replace("{quote}", quote.unwrap_or(""))
        .replace("{locations}", locations.unwrap_or(""))
}

fn chapter_location_selection_label(selection: &ChapterLocationSelection<'_>) -> String {
    if let Some(title) = &selection.title {
        format!("第{}回（{}）", selection.chapter_no, title)
    } else {
        format!(
            "第{}回（{}：{}）",
            selection.chapter_no, selection.best.card.source_title, selection.best.quote
        )
    }
}

fn primary_chapter_location_candidate<'a>(
    selection: &'a ChapterLocationSelection<'a>,
) -> Option<&'a ChapterLocationCandidate<'a>> {
    selection
        .base
        .as_ref()
        .or(selection.commentary.as_ref())
        .or(Some(&selection.best))
}

fn death_chapter_primary_selection<'a>(
    frame: &RuntimeQuestionFrame,
    selections: &'a [ChapterLocationSelection<'a>],
) -> Option<&'a ChapterLocationSelection<'a>> {
    if !is_death_chapter_location(frame) {
        return None;
    }
    death_chapter_occurrence_selection(selections)
        .or_else(|| death_chapter_confirmation_selection(selections))
}

fn death_chapter_occurrence_selection<'a>(
    selections: &'a [ChapterLocationSelection<'a>],
) -> Option<&'a ChapterLocationSelection<'a>> {
    selections
        .iter()
        .filter(|selection| selection.death_occurrence.is_some())
        .min_by_key(|selection| selection.chapter_no)
}

fn death_chapter_confirmation_selection<'a>(
    selections: &'a [ChapterLocationSelection<'a>],
) -> Option<&'a ChapterLocationSelection<'a>> {
    selections
        .iter()
        .filter(|selection| selection.death_confirmation.is_some())
        .min_by_key(|selection| selection.chapter_no)
}

fn death_chapter_memorial_selection<'a>(
    selections: &'a [ChapterLocationSelection<'a>],
) -> Option<&'a ChapterLocationSelection<'a>> {
    selections
        .iter()
        .filter(|selection| selection.death_memorial.is_some())
        .min_by_key(|selection| selection.chapter_no)
}

fn dominant_chapter_selection<'a>(
    selections: &'a [ChapterLocationSelection<'a>],
    policy: &ChapterLocationPolicy,
) -> Option<&'a ChapterLocationSelection<'a>> {
    let top = selections.first()?;
    if top.base.is_none() && top.title.is_none() && top.commentary.is_none() {
        return None;
    }
    let Some(next) = selections.get(1) else {
        return Some(top);
    };
    if top.best.score - next.best.score >= policy.dominant_chapter_score_margin {
        return Some(top);
    }
    None
}

fn ranked_chapter_location_selections<'a>(
    candidates: Vec<ChapterLocationCandidate<'a>>,
    _event_label: String,
) -> Vec<ChapterLocationSelection<'a>> {
    let mut grouped: BTreeMap<i64, Vec<ChapterLocationCandidate<'a>>> = BTreeMap::new();
    for candidate in candidates {
        grouped
            .entry(candidate.chapter_no)
            .or_default()
            .push(candidate);
    }
    let mut selections = grouped
        .into_iter()
        .filter_map(|(chapter_no, mut items)| {
            items.sort_by_key(|candidate| (std::cmp::Reverse(candidate.score), candidate.index));
            let best = items.first()?.clone();
            let title = items.iter().find_map(|item| item.chapter_title.clone());
            let base = items
                .iter()
                .find(|item| item.kind == ChapterLocationCandidateKind::BaseText)
                .cloned();
            let commentary = items
                .iter()
                .find(|item| item.kind == ChapterLocationCandidateKind::Commentary)
                .cloned();
            let death_occurrence = items
                .iter()
                .find(|item| item.death_role == Some(DeathChapterEvidenceRole::Occurrence))
                .cloned();
            let death_confirmation = items
                .iter()
                .find(|item| item.death_role == Some(DeathChapterEvidenceRole::Confirmation))
                .cloned();
            let death_memorial = items
                .iter()
                .find(|item| item.death_role == Some(DeathChapterEvidenceRole::Memorial))
                .cloned();
            Some(ChapterLocationSelection {
                chapter_no,
                title,
                base,
                commentary,
                death_occurrence,
                death_confirmation,
                death_memorial,
                best,
                support_count: items.len(),
            })
        })
        .collect::<Vec<_>>();
    selections.sort_by_key(|selection| {
        (
            std::cmp::Reverse(selection.best.score),
            std::cmp::Reverse(selection.support_count),
            selection.chapter_no,
        )
    });
    selections
}

fn remove_entity_terms(text: &mut String, entity: &RuntimeQuestionFrameEntity) {
    let mut terms = entity.identity_terms();
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    for term in terms {
        let normalized = normalize_text(&term);
        if !normalized.is_empty() {
            *text = text.replace(&normalized, "");
        }
    }
}

fn chapter_location_candidates<'a>(
    frame: &RuntimeQuestionFrame,
    cards: &'a [EvidenceCard],
    terms: &[String],
    policy: &ChapterLocationPolicy,
) -> Vec<ChapterLocationCandidate<'a>> {
    let death_location = is_death_chapter_location(frame);
    cards
        .iter()
        .enumerate()
        .filter_map(|(index, card)| {
            let chapter_no = chapter_no_for_card(card)?;
            let chapter_title = chapter_location_title(card, chapter_no, terms, policy);
            let weak_mention = chapter_location_weak_mention(card, policy);
            let kind = if chapter_title.is_some() {
                ChapterLocationCandidateKind::ChapterTitle
            } else if weak_mention {
                ChapterLocationCandidateKind::WeakMention
            } else if card.evidence_type == "base_text" {
                ChapterLocationCandidateKind::BaseText
            } else if card.evidence_type == "commentary"
                && commentary_explains_location(card, terms)
            {
                ChapterLocationCandidateKind::Commentary
            } else {
                ChapterLocationCandidateKind::WeakMention
            };
            let death_role = death_location
                .then(|| death_chapter_evidence_role(card))
                .flatten();
            let score = chapter_location_card_score(card, terms, &chapter_title, weak_mention)
                + death_role_score(death_role);
            if score <= 0 {
                return None;
            }
            Some(ChapterLocationCandidate {
                card,
                chapter_no,
                quote: chapter_location_quote(card, terms, policy.max_quote_chars),
                chapter_title,
                score,
                kind,
                death_role,
                index,
            })
        })
        .collect()
}

fn chapter_no_for_card(card: &EvidenceCard) -> Option<i64> {
    extract_chapter_no(&card.source_title)
        .or_else(|| extract_chapter_no(&card.text))
        .or_else(|| chapter_no_from_record_code(&card.block_id))
        .or_else(|| chapter_no_from_record_code(&card.source_id))
        .or_else(|| chapter_no_from_record_code(&card.support_scope))
}

fn chapter_no_from_record_code(value: &str) -> Option<i64> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|part| {
            let digits = part.strip_prefix('c').or_else(|| part.strip_prefix('C'))?;
            if digits.is_empty()
                || digits.len() > 3
                || !digits.chars().all(|ch| ch.is_ascii_digit())
            {
                return None;
            }
            let chapter_no = digits.parse::<i64>().ok()?;
            (1..=120).contains(&chapter_no).then_some(chapter_no)
        })
        .next()
}

fn chapter_location_card_score(
    card: &EvidenceCard,
    terms: &[String],
    chapter_title: &Option<String>,
    weak_mention: bool,
) -> i64 {
    let normalized_title = normalize_text(&card.source_title);
    let normalized_text = normalize_text(&card.text);
    let mut score = 0;
    for term in terms {
        let normalized = normalize_text(term);
        if normalized.is_empty() {
            continue;
        }
        if normalized_title.contains(&normalized) {
            score += 40 + normalized.chars().count() as i64;
        }
        if normalized_text.contains(&normalized) {
            score += 25 + normalized.chars().count() as i64;
        }
    }
    if chapter_title.is_some() {
        score += 120;
    }
    if card.evidence_type == "base_text" {
        score += 35;
    } else if card.evidence_type == "commentary" {
        score += 20;
    }
    if weak_mention {
        score -= 55;
    }
    score
}

fn chapter_location_quote(card: &EvidenceCard, terms: &[String], max_chars: usize) -> String {
    let clean = public_quote_text(&card.text);
    if let Some(quote) = chapter_location_death_quote(&clean) {
        return quote;
    }
    for term in terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        if let Some(index) = clean.find(term) {
            return trim_chars_around_byte(&clean, index, term.len(), max_chars);
        }
        let normalized_term = normalize_text(term);
        let normalized_clean = normalize_text(&clean);
        if let Some(index) = normalized_clean.find(&normalized_term) {
            let char_index = normalized_clean[..index].chars().count();
            return trim_chars_around_char_index(&clean, char_index, max_chars);
        }
    }
    trim_chars(&clean, max_chars)
}

fn chapter_location_death_quote(text: &str) -> Option<String> {
    [
        "說畢，便長歎一聲，蕭然長逝了",
        "说毕，便长叹一声，萧然长逝了",
        "便長歎一聲，蕭然長逝了",
        "便长叹一声，萧然长逝了",
        "果然是未正二刻他咽了氣",
        "果然是未正二刻他咽了气",
        "晴雯死了",
        "一咽氣便回了進去",
        "一咽气便回了进去",
        "即刻送到外頭焚化了罷",
        "即刻送到外头焚化了罢",
        "名曰《芙蓉女兒誄》",
        "名曰《芙蓉女儿诔》",
    ]
    .iter()
    .find(|quote| text.contains(**quote))
    .map(|quote| quote.to_string())
}

fn is_death_chapter_location(frame: &RuntimeQuestionFrame) -> bool {
    if !frame.is_chapter_location() {
        return false;
    }
    let normalized = normalize_text(&frame.canonical_question);
    [
        "死", "去世", "亡故", "病逝", "夭亡", "夭逝", "长逝", "長逝", "咽气", "咽氣", "死讯",
        "死訊",
    ]
    .iter()
    .any(|term| normalized.contains(&normalize_text(term)))
}

fn death_chapter_evidence_role(card: &EvidenceCard) -> Option<DeathChapterEvidenceRole> {
    let text = normalize_text(&format!(
        "{}\n{}\n{}",
        card.source_title, card.text, card.support_scope
    ));
    if contains_any_normalized(
        &text,
        &[
            "芙蓉女儿诔",
            "芙蓉女兒誄",
            "祭晴雯",
            "诔文",
            "誄文",
            "泣涕念曰",
            "悼亡",
        ],
    ) {
        return Some(DeathChapterEvidenceRole::Memorial);
    }
    if contains_any_normalized(
        &text,
        &[
            "死讯",
            "死訊",
            "打听晴雯",
            "探问晴雯",
            "得知她已经死亡",
            "得知她已經死亡",
            "已死",
            "閉了眼",
            "闭了眼",
            "住了口",
            "倒气",
            "倒氣",
            "一咽气便回了进去",
            "一咽氣便回了進去",
            "停柩",
            "遗体",
            "遺體",
            "入殓",
            "入殮",
            "焚化",
            "灵前",
            "靈前",
        ],
    ) {
        return Some(DeathChapterEvidenceRole::Confirmation);
    }
    if contains_any_normalized(
        &text,
        &[
            "萧然长逝",
            "蕭然長逝",
            "长逝了",
            "長逝了",
            "咽了气",
            "咽了氣",
            "晴雯死了",
            "死了",
            "身亡",
            "病逝",
            "去世",
            "亡故",
            "夭亡",
            "夭逝",
            "自尽",
            "自盡",
            "自刎",
            "投井",
            "吞金",
            "上吊",
            "临终",
            "臨終",
            "临死",
            "臨死",
        ],
    ) {
        return Some(DeathChapterEvidenceRole::Occurrence);
    }
    None
}

fn death_role_score(role: Option<DeathChapterEvidenceRole>) -> i64 {
    match role {
        Some(DeathChapterEvidenceRole::Occurrence) => 180,
        Some(DeathChapterEvidenceRole::Confirmation) => 125,
        Some(DeathChapterEvidenceRole::Memorial) => 70,
        None => 0,
    }
}

fn death_fate_boundary_answer(
    frame: &RuntimeQuestionFrame,
    cards: &[EvidenceCard],
) -> Option<String> {
    if !is_death_chapter_location(frame) || cards.is_empty() {
        return None;
    }
    let subject = frame.subject.as_ref()?;
    if cards.iter().any(|card| {
        card_mentions_entity(card, subject) && death_chapter_evidence_role(card).is_some()
    }) {
        return None;
    }
    let card = cards
        .iter()
        .filter(|card| card_mentions_entity(card, subject))
        .find(|card| card_has_fate_boundary_cue(card))?;
    Some(format!(
        "当前证据不能定位{}“第几回死”：材料只支持人物命运线索，而没有命中正文死亡发生场景或死讯确认。可见依据是{}“{}”。因此在当前范围内，应回答为：不能确定死亡回目，只能说明这些材料预示或关联其命运。",
        subject.canonical,
        card.source_title,
        chapter_location_quote(card, &subject.identity_terms(), 72)
    ))
}

fn death_fate_boundary_answer_requirement_value(
    frame: &RuntimeQuestionFrame,
    cards: &[EvidenceCard],
) -> Option<Value> {
    if !is_death_chapter_location(frame) {
        return None;
    }
    let subject = frame.subject.as_ref()?;
    if cards.iter().any(|card| {
        card_mentions_entity(card, subject) && death_chapter_evidence_role(card).is_some()
    }) {
        return None;
    }
    let card = cards
        .iter()
        .filter(|card| card_mentions_entity(card, subject))
        .find(|card| card_has_fate_boundary_cue(card))?;
    Some(json!({
        "event": format!("{}死亡回目", subject.canonical),
        "primary_evidence_id": card.evidence_id.clone(),
        "primary_source_title": card.source_title.clone(),
        "primary_text_cue": chapter_location_quote(card, &subject.identity_terms(), 72),
        "answer_shape": "death_chapter_unlocated_use_fate_boundary",
        "rule": "Do not answer a death chapter number from fate, judgement, song, or commentary cues unless the evidence explicitly locates the death occurrence or death-news confirmation.",
    }))
}

fn card_mentions_entity(card: &EvidenceCard, entity: &RuntimeQuestionFrameEntity) -> bool {
    let text = normalize_text(&format!("{}\n{}", card.source_title, card.text));
    entity
        .identity_terms()
        .iter()
        .any(|term| text.contains(&normalize_text(term)))
}

fn card_has_fate_boundary_cue(card: &EvidenceCard) -> bool {
    let text = normalize_text(&format!(
        "{}\n{}\n{}",
        card.source_title, card.text, card.support_scope
    ));
    contains_any_normalized(
        &text,
        &[
            "判词",
            "判詞",
            "红楼梦曲",
            "紅樓夢曲",
            "曲文",
            "册页",
            "冊頁",
            "薄命",
            "命运",
            "命運",
            "结局",
            "結局",
            "预示",
            "預示",
            "玉带林中挂",
            "玉帶林中掛",
            "枉凝眉",
            "还泪",
            "還淚",
        ],
    )
}

fn contains_any_normalized(text: &str, terms: &[&str]) -> bool {
    terms
        .iter()
        .any(|term| text.contains(&normalize_text(term)))
}

fn chapter_location_title(
    card: &EvidenceCard,
    chapter_no: i64,
    terms: &[String],
    policy: &ChapterLocationPolicy,
) -> Option<String> {
    card.text
        .lines()
        .filter_map(|line| chapter_location_title_from_line(line, chapter_no, terms, policy))
        .next()
}

fn chapter_location_title_from_line(
    line: &str,
    chapter_no: i64,
    terms: &[String],
    policy: &ChapterLocationPolicy,
) -> Option<String> {
    let clean = clean_chapter_title_line(line);
    if extract_chapter_no(&clean) != Some(chapter_no) {
        return None;
    }
    if let Some(title) = quoted_chapter_title_from_line(&clean, terms, policy) {
        return Some(title);
    }
    let after_marker = clean
        .split_once('回')
        .map(|(_, right)| right)
        .or_else(|| clean.split_once('囬').map(|(_, right)| right))?;
    let title = trim_chars(&clean_title_text(after_marker), policy.max_title_chars);
    if title.chars().count() < 4 {
        return None;
    }
    let normalized_title = normalize_text(&title);
    if !terms
        .iter()
        .any(|term| normalized_title.contains(&normalize_text(term)))
    {
        return None;
    }
    Some(format!("《{}》", title))
}

fn quoted_chapter_title_from_line(
    line: &str,
    terms: &[String],
    policy: &ChapterLocationPolicy,
) -> Option<String> {
    if !line.contains("回目") && !line.contains("題目") && !line.contains("题目") {
        return None;
    }
    let marker_index = ["回目", "題目", "题目"]
        .iter()
        .filter_map(|marker| line.find(marker).map(|index| index + marker.len()))
        .min()?;
    let tail = &line[marker_index..];
    for (open, close) in [('“', '”'), ('「', '」'), ('『', '』'), ('《', '》')] {
        let Some(start) = tail.find(open) else {
            continue;
        };
        let body = &tail[start + open.len_utf8()..];
        let Some(end) = body.find(close) else {
            continue;
        };
        let title = trim_chars(&clean_title_text(&body[..end]), policy.max_title_chars);
        if chapter_title_matches_terms(&title, terms) {
            return Some(format!("《{}》", title));
        }
    }
    None
}

fn chapter_title_matches_terms(title: &str, terms: &[String]) -> bool {
    let normalized_title = normalize_text(title);
    terms
        .iter()
        .any(|term| normalized_title.contains(&normalize_text(term)))
}

fn clean_chapter_title_line(line: &str) -> String {
    line.replace("{{center|", "")
        .replace("{{Novel|", "")
        .replace("'''", "")
        .replace("==", "")
        .replace('|', " ")
        .replace(['}', '{', '[', ']'], "")
        .trim()
        .to_string()
}

fn clean_title_text(text: &str) -> String {
    let mut title = text
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '|' | '=' | ':' | '：' | '-' | '—' | '　' | '\'' | '"' | '“' | '”'
                )
        })
        .trim()
        .to_string();
    while title.contains("  ") {
        title = title.replace("  ", " ");
    }
    title
}

fn chapter_location_weak_mention(card: &EvidenceCard, policy: &ChapterLocationPolicy) -> bool {
    let normalized = normalize_text(&card.text);
    let marker_count = policy
        .weak_mention_markers
        .iter()
        .filter(|marker| normalized.contains(&normalize_text(marker)))
        .count();
    marker_count >= 2
}

fn commentary_explains_location(card: &EvidenceCard, terms: &[String]) -> bool {
    let normalized = normalize_text(&card.text);
    let has_commentary_marker = normalized.contains("批") || normalized.contains("評");
    has_commentary_marker
        && terms.iter().any(|term| {
            let normalized_term = normalize_text(term);
            normalized_term.chars().count() >= 3 && normalized.contains(&normalized_term)
        })
}

fn chapter_number_mentioned(normalized_draft: &str, chapter_no: i64) -> bool {
    normalized_draft.contains(&format!("第{}回", chapter_no))
        || normalized_draft.contains(&format!("第{:03}回", chapter_no))
        || normalized_draft.contains(&chapter_no_to_chinese(chapter_no))
}

fn chapter_no_to_chinese(chapter_no: i64) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if chapter_no <= 0 || chapter_no >= 100 {
        return format!("第{}回", chapter_no);
    }
    let text = if chapter_no < 10 {
        DIGITS[chapter_no as usize].to_string()
    } else {
        let tens = chapter_no / 10;
        let ones = chapter_no % 10;
        let mut value = String::new();
        if tens > 1 {
            value.push_str(DIGITS[tens as usize]);
        }
        value.push('十');
        if ones > 0 {
            value.push_str(DIGITS[ones as usize]);
        }
        value
    };
    format!("第{}回", text)
}

fn chapter_title_cues(title: &str) -> Vec<String> {
    title
        .split(['《', '》', ' ', '　', '/', '\\'])
        .map(normalize_text)
        .filter(|part| part.chars().count() >= 3)
        .collect()
}

fn trim_chars_around_byte(text: &str, start: usize, byte_len: usize, max_chars: usize) -> String {
    let end = start + byte_len;
    let prefix = text[..start]
        .chars()
        .rev()
        .take(max_chars / 3)
        .collect::<Vec<_>>();
    let suffix = text[end..].chars().take(max_chars / 2).collect::<String>();
    let mut output = String::new();
    if text[..start].chars().count() > prefix.len() {
        output.push('…');
    }
    output.extend(prefix.into_iter().rev());
    output.push_str(&text[start..end]);
    output.push_str(&suffix);
    if text[end..].chars().count() > max_chars / 2 {
        output.push('…');
    }
    output
}

fn trim_chars_around_char_index(text: &str, char_index: usize, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let start = char_index.saturating_sub(max_chars / 3);
    let end = (char_index + max_chars / 2).min(chars.len());
    let mut output = String::new();
    if start > 0 {
        output.push('…');
    }
    output.extend(chars[start..end].iter());
    if end < chars.len() {
        output.push('…');
    }
    output
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    let mut output = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        output.push('…');
    }
    output
}

fn compact_question_text(text: &str) -> String {
    text.chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '?' | '？'
                        | '!'
                        | '！'
                        | ','
                        | '，'
                        | '.'
                        | '。'
                        | ':'
                        | '：'
                        | ';'
                        | '；'
                        | '"'
                        | '“'
                        | '”'
                        | '\''
                        | '‘'
                        | '’'
                )
        })
        .collect()
}

fn extend_unique(target: &mut Vec<String>, source: &[String]) {
    let mut seen = target.iter().cloned().collect::<BTreeSet<_>>();
    for term in source {
        let term = term.trim();
        if !term.is_empty() && seen.insert(term.to_string()) {
            target.push(term.to_string());
        }
    }
}

const CHAPTER_LOCATION_NOISE_TERMS: &[&str] = &[
    "是在",
    "发生在",
    "發生在",
    "出现在",
    "出現於",
    "出现于",
    "写在",
    "寫在",
    "见于",
    "見於",
    "属于",
    "屬於",
    "哪一回",
    "那一回",
    "哪回",
    "那回",
    "第几回",
    "第幾回",
    "第几囬",
    "第幾囬",
    "第多少回",
    "第多少囬",
    "回目",
    "哪一章",
    "那一章",
    "第几章",
    "第幾章",
    "是",
    "在",
    "的",
    "了",
    "吗",
    "嗎",
    "呢",
];
