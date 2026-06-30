#[cfg(test)]
use crate::entity_intro_answer::compose_entity_intro_answer;
use crate::{
    EvidenceCard, RuntimeContextContract, normalize_text, ontology_aliases,
    query_expansion_search_terms,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

mod attribute_answer;
mod chapter_location;

pub(crate) use chapter_location::{
    chapter_location_answer_requirement_value, chapter_location_draft_rejection_reason,
    chapter_location_evidence_ids_for_requirements,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelationSupportTerms {
    pub(crate) subject: Vec<String>,
    pub(crate) predicate: Vec<String>,
    pub(crate) object: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelationOpenObjectSupport {
    pub(crate) canonical: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) evidence_id: String,
    pub(crate) source_title: String,
    pub(crate) text: String,
    pub(crate) text_cue: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeCardSupport {
    pub(crate) claim_value: String,
    pub(crate) matched_terms: Vec<String>,
    pub(crate) modality: String,
    pub(crate) evidence_strength: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeQuestionFrame {
    pub(crate) intent: String,
    pub(crate) canonical_question: String,
    #[serde(default)]
    pub(crate) task: Option<String>,
    #[serde(default)]
    pub(crate) answer_target: Option<String>,
    pub(crate) subject: Option<RuntimeQuestionFrameEntity>,
    pub(crate) predicate: Option<RuntimeQuestionFramePredicate>,
    pub(crate) object: Option<RuntimeQuestionFrameEntity>,
    #[serde(default)]
    pub(crate) required_evidence_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeQuestionFrameEntity {
    pub(crate) canonical: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeQuestionFramePredicate {
    pub(crate) id: String,
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) evidence_terms: Vec<String>,
}

impl RuntimeQuestionFrame {
    pub(crate) fn is_relation(&self) -> bool {
        self.intent == "relation_query" && self.subject.is_some() && self.predicate.is_some()
    }

    pub(crate) fn has_relation_object(&self) -> bool {
        self.is_relation() && self.object.is_some()
    }

    pub(crate) fn has_open_relation_object(&self) -> bool {
        self.is_relation() && self.object.is_none()
    }

    pub(crate) fn is_attribute(&self) -> bool {
        matches!(
            self.intent.as_str(),
            "attribute_query" | "attribute_at_event" | "attribute_compare"
        ) && self.subject.is_some()
            && self.predicate.is_some()
    }

    pub(crate) fn is_character_fate(&self) -> bool {
        self.intent == "character_fate_query" && self.character_fate_entity().is_some()
    }

    pub(crate) fn is_chapter_location(&self) -> bool {
        self.intent == "chapter_location_query"
            || (self.task.as_deref() == Some("locate_event")
                && matches!(
                    self.answer_target.as_deref(),
                    Some("chapter_no" | "chapter_or_time")
                ))
    }

    pub(crate) fn character_fate_entity(&self) -> Option<&RuntimeQuestionFrameEntity> {
        self.subject.as_ref().or(self.object.as_ref())
    }

    fn relation_terms(&self) -> Vec<String> {
        let mut terms = Vec::new();
        if let Some(subject) = &self.subject {
            extend_terms(&mut terms, &subject.identity_terms());
        }
        if let Some(predicate) = &self.predicate {
            extend_terms(&mut terms, &predicate.aliases);
            extend_terms(&mut terms, &predicate.evidence_terms);
        }
        if let Some(object) = &self.object {
            extend_terms(&mut terms, &object.identity_terms());
        }
        terms
    }
}

impl RuntimeQuestionFrameEntity {
    pub(crate) fn identity_terms(&self) -> Vec<String> {
        let mut terms = vec![self.canonical.clone()];
        terms.extend(self.aliases.clone());
        terms
    }
}

pub(crate) fn frame_focus_terms(frame: Option<&RuntimeQuestionFrame>) -> Vec<String> {
    let Some(frame) = frame else {
        return Vec::new();
    };
    let mut terms = Vec::new();
    if frame.is_chapter_location() {
        extend_terms(
            &mut terms,
            &chapter_location::chapter_location_focus_terms(frame),
        );
        if let Ok(expanded) = query_expansion_search_terms(&frame.canonical_question) {
            extend_terms(&mut terms, &expanded);
        }
    }
    if let Some(subject) = &frame.subject {
        extend_terms(&mut terms, &subject.identity_terms());
    }
    if let Some(object) = &frame.object {
        extend_terms(&mut terms, &object.identity_terms());
    }
    terms
}

pub(crate) fn question_frame_from_context(
    context: &RuntimeContextContract,
) -> Option<RuntimeQuestionFrame> {
    context
        .projections
        .iter()
        .find(|projection| projection.consumer_name == "honglou-main")
        .or_else(|| context.projections.first())
        .and_then(|projection| projection.projection_payload.get("question_frame"))
        .and_then(parse_runtime_question_frame)
}

pub(crate) fn relation_search_query(
    question: &str,
    frame: Option<&RuntimeQuestionFrame>,
) -> String {
    let Some(frame) = frame.filter(|frame| frame.is_relation()) else {
        return question.to_string();
    };
    let mut terms = Vec::new();
    if frame.has_open_relation_object() {
        if let Some(subject) = &frame.subject {
            extend_terms(&mut terms, &subject.identity_terms());
        }
    } else {
        extend_terms(&mut terms, &frame.relation_terms());
    }
    extend_terms(
        &mut terms,
        &[question.to_string(), frame.canonical_question.clone()],
    );
    terms.into_iter().take(24).collect::<Vec<_>>().join(" ")
}

pub(crate) fn frame_search_query(question: &str, frame: Option<&RuntimeQuestionFrame>) -> String {
    let Some(frame) = frame else {
        return question.to_string();
    };
    if frame.is_relation() {
        return relation_search_query(question, Some(frame));
    }
    let mut terms = Vec::new();
    if frame.is_chapter_location() {
        extend_terms(
            &mut terms,
            &chapter_location::chapter_location_focus_terms(frame),
        );
        if let Ok(expanded) = query_expansion_search_terms(&frame.canonical_question) {
            extend_terms(&mut terms, &expanded);
        }
    }
    if let Some(subject) = &frame.subject {
        extend_terms(&mut terms, &subject.identity_terms());
    }
    if let Some(object) = &frame.object {
        extend_terms(&mut terms, &object.identity_terms());
    }
    if let Some(predicate) = &frame.predicate {
        extend_terms(&mut terms, &predicate_terms(predicate));
        extend_terms(&mut terms, &predicate.evidence_terms);
    }
    if terms.is_empty() {
        return question.to_string();
    }
    extend_terms(
        &mut terms,
        &[question.to_string(), frame.canonical_question.clone()],
    );
    terms.into_iter().take(24).collect::<Vec<_>>().join(" ")
}

pub(crate) fn relation_required_evidence_types(
    fallback: &[String],
    frame: Option<&RuntimeQuestionFrame>,
) -> Vec<String> {
    let Some(frame) = frame.filter(|frame| frame.is_relation()) else {
        return fallback.to_vec();
    };
    if frame.required_evidence_types.is_empty() {
        return fallback.to_vec();
    }
    let mut merged = fallback.to_vec();
    extend_terms(&mut merged, &frame.required_evidence_types);
    merged
}

pub(crate) fn relation_review_issues(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Vec<String> {
    let Some(frame) = frame.filter(|frame| frame.has_relation_object()) else {
        return Vec::new();
    };
    if relation_direct_support_cards(frame, cards).is_empty() {
        return vec!["relation_predicate_evidence_missing".to_string()];
    }
    Vec::new()
}

#[cfg(test)]
pub(crate) fn relation_direct_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.has_relation_object())?;
    let direct_cards = relation_direct_support_cards(frame, cards);
    let card = direct_cards.first()?;
    let subject = frame.subject.as_ref()?;
    let predicate = frame.predicate.as_ref()?;
    let object = frame.object.as_ref()?;
    let groups = relation_support_terms(frame)?;
    let quote = relation_support_quote(
        &card.text,
        &groups.subject,
        &groups.predicate,
        &groups.object,
    );
    Some(format!(
        "可以确认。{}有直接证据：{}。因此，在当前证据范围内，{}{}过{}。",
        card.source_title, quote, subject.canonical, predicate.label, object.canonical
    ))
}

pub(crate) fn attribute_card_support(
    frame: &RuntimeQuestionFrame,
    card: &EvidenceCard,
) -> Option<AttributeCardSupport> {
    attribute_answer::attribute_card_support(frame, card)
}

#[cfg(test)]
pub(crate) fn question_frame_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    if let Some(answer) = chapter_location::chapter_location_answer(frame, cards) {
        return Some(answer);
    }
    if let Some(answer) = relation_direct_answer(frame, cards) {
        return Some(answer);
    }
    if let Some(answer) = relation_open_object_answer(frame, cards) {
        return Some(answer);
    }
    if let Some(answer) = relation_boundary_answer(frame, cards) {
        return Some(answer);
    }
    if let Some(answer) = attribute_answer::attribute_answer(frame, cards) {
        return Some(answer);
    }
    compose_entity_intro_answer(frame, cards)
}

pub(crate) fn relation_draft_rejection_reason(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
    draft: &str,
) -> Option<&'static str> {
    let frame = frame.filter(|frame| frame.has_relation_object())?;
    let groups = relation_support_terms(frame)?;
    if !relation_text_matches_support_terms(draft, &groups) {
        return Some("question_frame_relation_answer_missing");
    }
    let direct_support = !relation_direct_support_cards(frame, cards).is_empty();
    let has_boundary = relation_answer_has_boundary(draft);
    if direct_support && has_boundary {
        return Some("question_frame_relation_answer_contradicts_evidence");
    }
    if !direct_support && !has_boundary {
        return Some("question_frame_relation_boundary_missing");
    }
    None
}

pub(crate) fn relation_open_object_draft_rejection_reason(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
    draft: &str,
) -> Option<&'static str> {
    let frame = frame.filter(|frame| frame.has_open_relation_object())?;
    let supported_objects = relation_open_object_supported_objects(Some(frame), cards);
    if supported_objects.is_empty() {
        if cards.is_empty() || relation_answer_has_boundary(draft) {
            return None;
        }
        return Some("question_frame_relation_boundary_missing");
    }
    let draft_text = normalize_text(draft);
    if supported_objects
        .iter()
        .any(|object| !contains_any_normalized(&draft_text, &normalized_terms(&object.aliases)))
    {
        return Some("draft_missing_open_relation_object");
    }
    if supported_objects
        .iter()
        .any(|object| !relation_open_object_draft_has_evidence_cue(object, &draft_text))
    {
        return Some("draft_missing_open_relation_evidence_cue");
    }
    None
}

#[cfg(test)]
pub(crate) fn relation_boundary_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.has_relation_object())?;
    if !relation_direct_support_cards(frame, cards).is_empty() {
        return None;
    }
    let subject = frame.subject.as_ref()?;
    let predicate = frame.predicate.as_ref()?;
    let object = frame.object.as_ref()?;
    let mut answer = format!(
        "就当前证据包看，没有直接证据能确认{}与{}之间存在“{}”关系；因此不能确认这是一条已被文本支持的关系。",
        subject.canonical, object.canonical, predicate.label
    );
    if !cards.is_empty() {
        answer.push_str("当前命中的材料没有同时给出主体、关系谓词和对象三者的直接支撑；只能作为继续检索的线索，不能替代关系证据。");
    }
    Some(answer)
}

#[cfg(test)]
pub(crate) fn relation_open_object_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.has_open_relation_object())?;
    let subject = frame.subject.as_ref()?;
    let predicate = frame.predicate.as_ref()?;
    let object_candidates = relation_open_object_supported_objects(Some(frame), cards);
    if object_candidates.is_empty() {
        return Some(format!(
            "就当前证据包看，尚不能从命中材料中抽出明确的{}“{}”对象；需要继续命中同时出现主体、关系和对象的直接材料。",
            subject.canonical, predicate.label
        ));
    }

    let summaries = object_candidates
        .iter()
        .map(|candidate| {
            format!(
                "{}（{}：{}）",
                candidate.canonical, candidate.source_title, candidate.text_cue
            )
        })
        .collect::<Vec<_>>();
    Some(format!(
        "就当前证据包看，可以直接支持的{}“{}”对象包括：{}。其他对象仍需继续检索能同时出现主体、关系和对象的直接材料。",
        subject.canonical,
        predicate.label,
        summaries.join("；")
    ))
}

pub(crate) fn relation_open_object_supported_objects(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Vec<RelationOpenObjectSupport> {
    let Some(frame) = frame.filter(|frame| frame.has_open_relation_object()) else {
        return Vec::new();
    };
    let Some(subject) = &frame.subject else {
        return Vec::new();
    };
    let Some(predicate) = &frame.predicate else {
        return Vec::new();
    };
    let predicate_terms = predicate_terms(predicate);
    relation_open_object_candidates(
        subject,
        cards,
        &predicate_terms,
        &normalized_terms(&predicate_terms),
    )
}

pub(crate) fn relation_open_object_focus_terms(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Vec<String> {
    let mut terms = Vec::new();
    for object in relation_open_object_supported_objects(frame, cards) {
        extend_terms(&mut terms, &object.aliases);
    }
    terms
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect()
}

pub(crate) fn relation_open_object_search_terms(
    frame: Option<&RuntimeQuestionFrame>,
) -> Vec<String> {
    let Some(frame) = frame.filter(|frame| frame.has_open_relation_object()) else {
        return Vec::new();
    };
    let mut terms = Vec::new();
    if let Some(subject) = &frame.subject {
        extend_terms(&mut terms, &subject.identity_terms());
    }
    terms
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect()
}

pub(crate) fn relation_open_object_text_candidate_names(
    frame: Option<&RuntimeQuestionFrame>,
    text: &str,
) -> Vec<String> {
    let Some(frame) = frame.filter(|frame| frame.has_open_relation_object()) else {
        return Vec::new();
    };
    let Some(subject) = &frame.subject else {
        return Vec::new();
    };
    let Some(predicate) = &frame.predicate else {
        return Vec::new();
    };
    relation_open_object_text_candidates(
        subject,
        text,
        &normalized_terms(&predicate_terms(predicate)),
    )
    .map(|person| person.canonical_name)
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect()
}

fn relation_open_object_candidates(
    subject: &RuntimeQuestionFrameEntity,
    cards: &[EvidenceCard],
    predicate_terms: &[String],
    normalized_predicate_terms: &[String],
) -> Vec<RelationOpenObjectSupport> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for card in cards {
        for person in
            relation_open_object_text_candidates(subject, &card.text, normalized_predicate_terms)
        {
            if seen.insert(person.canonical_name.clone()) {
                candidates.push(RelationOpenObjectSupport {
                    canonical: person.canonical_name.clone(),
                    aliases: person.aliases.clone(),
                    evidence_id: card.evidence_id.clone(),
                    source_title: card.source_title.clone(),
                    text: card.text.clone(),
                    text_cue: relation_support_quote(
                        &card.text,
                        &subject.identity_terms(),
                        predicate_terms,
                        &person.aliases,
                    ),
                });
            }
        }
    }
    candidates
}

fn relation_open_object_draft_has_evidence_cue(
    object: &RelationOpenObjectSupport,
    normalized_draft: &str,
) -> bool {
    relation_open_object_evidence_cue_terms(object)
        .iter()
        .any(|term| normalized_draft.contains(term))
}

fn relation_open_object_evidence_cue_terms(object: &RelationOpenObjectSupport) -> Vec<String> {
    let mut terms = Vec::new();
    extend_terms(&mut terms, &source_title_cue_terms(&object.source_title));
    let clean_text_cue = object
        .text_cue
        .trim()
        .trim_matches('“')
        .trim_matches('”')
        .trim_matches('"')
        .trim_matches('…')
        .trim_matches('.');
    if !clean_text_cue.is_empty() {
        extend_terms(&mut terms, &[clean_text_cue.to_string()]);
    }
    normalized_terms(&terms)
        .into_iter()
        .filter(|term| term.chars().count() >= 3)
        .collect()
}

fn source_title_cue_terms(source_title: &str) -> Vec<String> {
    let mut terms = vec![source_title.to_string()];
    if let Some(last) = source_title.rsplit('/').next() {
        terms.push(last.to_string());
        if let Some(cue) = chapter_cue_without_leading_zero(last) {
            terms.push(cue);
        }
    }
    if let Some(cue) = chapter_cue_without_leading_zero(source_title) {
        terms.push(cue);
    }
    terms
}

fn chapter_cue_without_leading_zero(text: &str) -> Option<String> {
    let start = text.find('第')?;
    let suffix = &text[start + '第'.len_utf8()..];
    let end = suffix.find('回')?;
    let digits = &suffix[..end];
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() || trimmed == digits {
        return None;
    }
    Some(format!("第{trimmed}回"))
}

fn relation_open_object_text_candidates<'a>(
    subject: &RuntimeQuestionFrameEntity,
    text: &str,
    predicate_terms: &'a [String],
) -> impl Iterator<Item = ontology_aliases::PersonAliasView> + 'a {
    let subject_terms = normalized_terms(&subject.identity_terms());
    let people = ontology_aliases::people_aliases().unwrap_or_default();
    let normalized = normalize_text(text);
    let subject_canonical = subject.canonical.clone();
    people.into_iter().filter(move |person| {
        if person.canonical_name == subject_canonical {
            return false;
        }
        if !contains_any_normalized(&normalized, &subject_terms)
            || !contains_any_normalized(&normalized, predicate_terms)
        {
            return false;
        }
        let object_terms = normalized_terms(&person.aliases);
        relation_text_links_subject_predicate_object(
            &normalized,
            &subject_terms,
            predicate_terms,
            &object_terms,
        )
    })
}

pub(crate) fn relation_direct_support_cards<'a>(
    frame: &RuntimeQuestionFrame,
    cards: &'a [EvidenceCard],
) -> Vec<&'a EvidenceCard> {
    let Some(groups) = relation_support_terms(frame) else {
        return Vec::new();
    };
    cards
        .iter()
        .filter(|card| relation_text_matches_support_terms(&card.text, &groups))
        .collect()
}

pub(crate) fn relation_text_matches_support_terms(
    text: &str,
    groups: &RelationSupportTerms,
) -> bool {
    let subject_terms = normalized_terms(&groups.subject);
    let predicate_terms = normalized_terms(&groups.predicate);
    let object_terms = normalized_terms(&groups.object);
    let normalized = normalize_text(text);
    relation_text_links_subject_predicate_object(
        &normalized,
        &subject_terms,
        &predicate_terms,
        &object_terms,
    )
}

pub(crate) fn relation_support_terms(frame: &RuntimeQuestionFrame) -> Option<RelationSupportTerms> {
    if !frame.has_relation_object() {
        return None;
    }
    let subject = frame.subject.as_ref()?;
    let predicate = frame.predicate.as_ref()?;
    let object = frame.object.as_ref()?;
    Some(RelationSupportTerms {
        subject: subject.identity_terms(),
        predicate: predicate_terms(predicate),
        object: object.identity_terms(),
    })
}

pub(crate) fn parse_runtime_question_frame(value: &Value) -> Option<RuntimeQuestionFrame> {
    if value.get("schema_version").and_then(Value::as_str)? != "tonglingyu.question_frame.v2" {
        return None;
    }
    let task = value
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let answer_target = value
        .get("answer_target")
        .and_then(|target| target.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(ToString::to_string);
    let slots = value.get("slots")?;
    let event_type = slots
        .get("event")
        .and_then(|event| event.get("type"))
        .and_then(Value::as_str);
    let intent = match task {
        "verify_relation" => "relation_query",
        "compare" => "attribute_compare",
        "locate_event" => {
            if event_type == Some("death") {
                "character_fate_query"
            } else {
                "chapter_location_query"
            }
        }
        "count_occurrences" => "count_query",
        "extract_evidence" => "evidence_query",
        "summarize_entity" => "entity_query",
        _ if slots.get("relation").is_some() => "relation_query",
        _ if slots.get("attribute").is_some() => "attribute_query",
        _ if event_type == Some("death") => "character_fate_query",
        _ => "general_query",
    };
    let predicate = slots
        .get("relation")
        .or_else(|| slots.get("attribute"))
        .and_then(runtime_predicate_from_v2_slot);
    let mut required_evidence_types = runtime_string_array(
        value
            .get("evidence_contract")
            .and_then(|contract| contract.get("required_types")),
    );
    for item in runtime_string_array(
        value
            .get("evidence_contract")
            .and_then(|contract| contract.get("supporting_types")),
    ) {
        if !required_evidence_types
            .iter()
            .any(|existing| existing == &item)
        {
            required_evidence_types.push(item);
        }
    }
    Some(RuntimeQuestionFrame {
        intent: intent.to_string(),
        canonical_question: value
            .get("normalized_question")
            .and_then(Value::as_str)
            .or_else(|| value.get("original_question").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string(),
        task: (!task.is_empty()).then(|| task.to_string()),
        answer_target,
        subject: slots.get("subject").and_then(runtime_entity_from_v2_slot),
        predicate,
        object: slots.get("object").and_then(runtime_entity_from_v2_slot),
        required_evidence_types,
    })
}

fn runtime_entity_from_v2_slot(value: &Value) -> Option<RuntimeQuestionFrameEntity> {
    if value.is_null() {
        return None;
    }
    Some(RuntimeQuestionFrameEntity {
        canonical: value.get("name").and_then(Value::as_str)?.to_string(),
        aliases: runtime_string_array(value.get("aliases")),
    })
}

fn runtime_predicate_from_v2_slot(value: &Value) -> Option<RuntimeQuestionFramePredicate> {
    if value.is_null() {
        return None;
    }
    Some(RuntimeQuestionFramePredicate {
        id: value.get("id").and_then(Value::as_str)?.to_string(),
        label: value.get("label").and_then(Value::as_str)?.to_string(),
        aliases: runtime_string_array(value.get("aliases")),
        evidence_terms: runtime_string_array(value.get("evidence_terms")),
    })
}

fn runtime_string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn predicate_terms(predicate: &RuntimeQuestionFramePredicate) -> Vec<String> {
    let mut terms = vec![predicate.label.clone()];
    terms.extend(predicate.aliases.clone());
    terms
}

fn extend_terms(target: &mut Vec<String>, source: &[String]) {
    let mut seen = target
        .iter()
        .map(|item| item.trim().to_string())
        .collect::<BTreeSet<_>>();
    for term in source {
        let term = term.trim();
        if !term.is_empty() && seen.insert(term.to_string()) {
            target.push(term.to_string());
        }
    }
}

fn normalized_terms(terms: &[String]) -> Vec<String> {
    terms
        .iter()
        .map(|term| normalize_text(term))
        .filter(|term| !term.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn contains_any_normalized(text: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn relation_text_links_subject_predicate_object(
    text: &str,
    subject_terms: &[String],
    predicate_terms: &[String],
    object_terms: &[String],
) -> bool {
    contains_any_normalized(text, subject_terms)
        && contains_any_normalized(text, predicate_terms)
        && contains_any_normalized(text, object_terms)
        && (relation_text_has_three_term_window(text, subject_terms, predicate_terms, object_terms)
            || relation_text_has_speaker_self_relation(
                text,
                subject_terms,
                predicate_terms,
                object_terms,
            ))
}

fn relation_text_has_three_term_window(
    text: &str,
    subject_terms: &[String],
    predicate_terms: &[String],
    object_terms: &[String],
) -> bool {
    const MAX_RELATION_WINDOW_CHARS: usize = 48;
    for subject in subject_terms {
        for subject_span in person_term_spans(text, subject) {
            for predicate in predicate_terms {
                for predicate_span in term_spans(text, predicate) {
                    for object in object_terms {
                        for object_span in person_term_spans(text, object) {
                            let start = subject_span
                                .start
                                .min(predicate_span.start)
                                .min(object_span.start);
                            let end = subject_span
                                .end
                                .max(predicate_span.end)
                                .max(object_span.end);
                            if text[start..end].chars().count() <= MAX_RELATION_WINDOW_CHARS
                                && !text[start..end].chars().any(is_relation_hard_boundary)
                                && relation_term_order_supports_subject_control(
                                    text,
                                    subject_span,
                                    predicate_span,
                                    object_span,
                                    subject_terms,
                                    object_terms,
                                )
                                && spans_are_near_without_clause_boundary(
                                    text,
                                    predicate_span,
                                    object_span,
                                    18,
                                )
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

fn relation_text_has_speaker_self_relation(
    text: &str,
    subject_terms: &[String],
    predicate_terms: &[String],
    object_terms: &[String],
) -> bool {
    const MAX_SUBJECT_ANCHOR_CHARS: usize = 180;
    const MAX_SELF_MARKER_CHARS: usize = 36;
    for predicate in predicate_terms {
        for predicate_span in term_spans(text, predicate) {
            for object in object_terms {
                for object_span in person_term_spans(text, object) {
                    if !spans_are_near_without_clause_boundary(
                        text,
                        predicate_span,
                        object_span,
                        18,
                    ) {
                        continue;
                    }
                    let relation_start = predicate_span.start.min(object_span.start);
                    let Some(subject_span) =
                        last_person_term_span_before(text, subject_terms, relation_start)
                    else {
                        continue;
                    };
                    if text[subject_span.end..relation_start].chars().count()
                        > MAX_SUBJECT_ANCHOR_CHARS
                    {
                        continue;
                    }
                    if !contains_speaker_anchor(&text[subject_span.end..relation_start]) {
                        continue;
                    }
                    let local_prefix = text_after_last_hard_boundary_before(text, relation_start);
                    if contains_self_relation_marker(&tail_chars(
                        local_prefix,
                        MAX_SELF_MARKER_CHARS,
                    )) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn relation_answer_has_boundary(text: &str) -> bool {
    let normalized = normalize_text(text);
    [
        "没有直接证据",
        "沒有直接證據",
        "没有明确证据",
        "沒有明確證據",
        "无明确证据",
        "無明確證據",
        "没有",
        "沒有",
        "并未",
        "並未",
        "并没有",
        "並沒有",
        "不曾",
        "未曾",
        "未见直接证据",
        "未見直接證據",
        "未见",
        "未見",
        "不能确认",
        "不能確認",
        "证据不足",
        "證據不足",
        "缺少足够证据",
        "缺少足夠證據",
        "暂不能确认",
        "暫不能確認",
        "暂时不能确认",
        "暫時不能確認",
    ]
    .iter()
    .map(|term| normalize_text(term))
    .any(|term| normalized.contains(&term))
}

fn relation_support_quote(
    text: &str,
    subject_terms: &[String],
    predicate_terms: &[String],
    object_terms: &[String],
) -> String {
    relation_support_quote_span(text, subject_terms, predicate_terms, object_terms)
        .map(|span| quote_span(text, span))
        .unwrap_or_else(|| short_quote(text))
}

fn relation_support_quote_span(
    text: &str,
    subject_terms: &[String],
    predicate_terms: &[String],
    object_terms: &[String],
) -> Option<TermSpan> {
    let subject_terms = quote_terms(subject_terms);
    let predicate_terms = quote_terms(predicate_terms);
    let object_terms = quote_terms(object_terms);
    let mut candidates = Vec::new();

    for predicate in &predicate_terms {
        for predicate_span in term_spans(text, predicate) {
            for object in &object_terms {
                for object_span in term_spans(text, object) {
                    if !spans_are_near_without_clause_boundary(
                        text,
                        predicate_span,
                        object_span,
                        18,
                    ) {
                        continue;
                    }
                    let relation_start = predicate_span.start.min(object_span.start);
                    let relation_end = predicate_span.end.max(object_span.end);
                    if quote_has_subject_anchor(text, &subject_terms, relation_start) {
                        candidates.push(relation_phrase_span(text, relation_start, relation_end));
                    }
                }
            }
        }
    }

    candidates.into_iter().min_by_key(|span| {
        (
            text[span.start..span.end].chars().count(),
            span.start,
            span.end,
        )
    })
}

fn quote_has_subject_anchor(text: &str, subject_terms: &[String], relation_start: usize) -> bool {
    if subject_terms.is_empty() {
        return true;
    }
    let Some(subject_span) = last_term_span_before(text, subject_terms, relation_start) else {
        return false;
    };
    let between = &text[subject_span.end..relation_start];
    if between.chars().count() > 180 || !contains_speaker_anchor(between) {
        return !between.chars().any(is_relation_hard_boundary) && between.chars().count() <= 48;
    }
    if contains_self_relation_marker(&tail_chars(between, 36)) {
        return true;
    }
    !between.chars().any(is_relation_hard_boundary) && between.chars().count() <= 48
}

fn relation_phrase_span(text: &str, relation_start: usize, relation_end: usize) -> TermSpan {
    let start = clause_start_before(text, relation_start);
    let end = clause_end_after(text, relation_end);
    if text[start..end].chars().count() <= 72 {
        return TermSpan { start, end };
    }
    TermSpan {
        start: relation_start,
        end: relation_end,
    }
}

fn clause_start_before(text: &str, before: usize) -> usize {
    text[..before]
        .char_indices()
        .rev()
        .find(|(_, ch)| is_relation_clause_boundary(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0)
}

fn clause_end_after(text: &str, after: usize) -> usize {
    text[after..]
        .char_indices()
        .find(|(_, ch)| is_relation_clause_boundary(*ch))
        .map(|(index, _)| after + index)
        .unwrap_or(text.len())
}

fn quote_span(text: &str, span: TermSpan) -> String {
    let cleaned = text[span.start..span.end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("");
    if cleaned.trim().is_empty() {
        return short_quote(text);
    }
    format!("“{}”", cleaned)
}

fn quote_terms(terms: &[String]) -> Vec<String> {
    let mut terms = terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    terms
}

#[derive(Debug, Clone, Copy)]
struct TermSpan {
    start: usize,
    end: usize,
}

fn term_spans(text: &str, term: &str) -> Vec<TermSpan> {
    if term.is_empty() {
        return Vec::new();
    }
    text.match_indices(term)
        .map(|(start, matched)| TermSpan {
            start,
            end: start + matched.len(),
        })
        .collect()
}

fn last_person_term_span_before(text: &str, terms: &[String], before: usize) -> Option<TermSpan> {
    terms
        .iter()
        .flat_map(|term| person_term_spans(text, term))
        .filter(|span| span.end <= before)
        .max_by_key(|span| span.start)
}

fn last_term_span_before(text: &str, terms: &[String], before: usize) -> Option<TermSpan> {
    terms
        .iter()
        .flat_map(|term| term_spans(text, term))
        .filter(|span| span.end <= before)
        .max_by_key(|span| span.start)
}

fn person_term_spans(text: &str, term: &str) -> Vec<TermSpan> {
    term_spans(text, term)
        .into_iter()
        .filter(|span| !person_term_span_is_shadowed(text, term, *span))
        .collect()
}

fn person_term_span_is_shadowed(text: &str, term: &str, span: TermSpan) -> bool {
    if term.is_empty() {
        return false;
    }
    ontology_aliases::people_aliases()
        .unwrap_or_default()
        .into_iter()
        .flat_map(|person| normalized_terms(&person.aliases))
        .any(|alias| {
            alias != term
                && alias.chars().count() > term.chars().count()
                && alias.contains(term)
                && text.match_indices(&alias).any(|(start, matched)| {
                    let alias_span = TermSpan {
                        start,
                        end: start + matched.len(),
                    };
                    alias_span.start <= span.start && span.end <= alias_span.end
                })
        })
}

fn relation_term_order_supports_subject_control(
    text: &str,
    subject: TermSpan,
    predicate: TermSpan,
    object: TermSpan,
    subject_terms: &[String],
    object_terms: &[String],
) -> bool {
    if subject.end <= predicate.start && predicate.end <= object.start {
        return !has_intervening_non_relation_person(
            text,
            subject.end,
            predicate.start,
            subject_terms,
            object_terms,
        );
    }
    if object.end <= subject.start && subject.end <= predicate.start {
        return !has_intervening_non_relation_person(
            text,
            subject.end,
            predicate.start,
            subject_terms,
            object_terms,
        );
    }
    false
}

fn has_intervening_non_relation_person(
    text: &str,
    start: usize,
    end: usize,
    subject_terms: &[String],
    object_terms: &[String],
) -> bool {
    if start >= end {
        return false;
    }
    let segment = &text[start..end];
    ontology_aliases::people_aliases()
        .unwrap_or_default()
        .into_iter()
        .any(|person| {
            normalized_terms(&person.aliases).into_iter().any(|alias| {
                !subject_terms.contains(&alias)
                    && !object_terms.contains(&alias)
                    && !person_term_spans(segment, &alias).is_empty()
            })
        })
}

fn spans_are_near_without_clause_boundary(
    text: &str,
    left: TermSpan,
    right: TermSpan,
    max_chars_between: usize,
) -> bool {
    let (first, second) = if left.start <= right.start {
        (left, right)
    } else {
        (right, left)
    };
    let between = &text[first.end..second.start];
    between.chars().count() <= max_chars_between
        && !between.chars().any(is_relation_clause_boundary)
}

fn text_after_last_hard_boundary_before(text: &str, before: usize) -> &str {
    let prefix = &text[..before];
    match prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| is_relation_hard_boundary(*ch))
    {
        Some((index, ch)) => &prefix[index + ch.len_utf8()..],
        None => prefix,
    }
}

fn tail_chars(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

fn contains_speaker_anchor(text: &str) -> bool {
    ["道", "說", "说", "曰", "云", "雲"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn contains_self_relation_marker(text: &str) -> bool {
    ["我", "自我", "自己", "咱們", "咱们", "俺"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn is_relation_hard_boundary(ch: char) -> bool {
    matches!(
        ch,
        '。' | '；' | ';' | '？' | '?' | '！' | '!' | '\n' | '\r'
    )
}

fn is_relation_clause_boundary(ch: char) -> bool {
    matches!(
        ch,
        '。' | '，' | ',' | '、' | '；' | ';' | '：' | ':' | '？' | '?' | '！' | '!' | '\n' | '\r'
    )
}

fn short_quote(text: &str) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join("");
    let mut output = String::new();
    for (index, ch) in cleaned.chars().enumerate() {
        if index >= 72 {
            output.push_str("...");
            break;
        }
        output.push(ch);
    }
    format!("“{}”", output)
}

#[cfg(test)]
mod tests;
