use crate::{EvidenceCard, RuntimeContextContract, normalize_text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelationSupportTerms {
    pub(crate) subject: Vec<String>,
    pub(crate) predicate: Vec<String>,
    pub(crate) object: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeQuestionFrame {
    pub(crate) intent: String,
    pub(crate) canonical_question: String,
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
    fn identity_terms(&self) -> Vec<String> {
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
    extend_terms(&mut terms, &frame.relation_terms());
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
    if let Some(subject) = &frame.subject {
        extend_terms(&mut terms, &subject.identity_terms());
    }
    if let Some(object) = &frame.object {
        extend_terms(&mut terms, &object.identity_terms());
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
    let quote = short_quote(&card.text);
    Some(format!(
        "可以确认。{}有直接证据：{}。因此，在当前证据范围内，{}{}过{}。",
        card.source_title, quote, subject.canonical, predicate.label, object.canonical
    ))
}

pub(crate) fn question_frame_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    if let Some(answer) = relation_direct_answer(frame, cards) {
        return Some(answer);
    }
    if let Some(answer) = relation_boundary_answer(frame, cards) {
        return Some(answer);
    }
    entity_intro_answer(frame, cards)
}

pub(crate) fn relation_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    relation_direct_answer(frame, cards).or_else(|| relation_boundary_answer(frame, cards))
}

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
        "就当前证据包看，没有直接证据能确认{}{}过{}；因此不能确认这是一条已被文本支持的关系。",
        subject.canonical, predicate.label, object.canonical
    );
    if !cards.is_empty() {
        answer.push_str("当前命中的材料没有同时给出主体、关系谓词和对象三者的直接支撑；只能作为继续检索的线索，不能替代关系证据。");
    }
    Some(answer)
}

pub(crate) fn relation_direct_support_cards<'a>(
    frame: &RuntimeQuestionFrame,
    cards: &'a [EvidenceCard],
) -> Vec<&'a EvidenceCard> {
    let Some(groups) = relation_support_terms(frame) else {
        return Vec::new();
    };
    let subject_terms = normalized_terms(&groups.subject);
    let predicate_terms = normalized_terms(&groups.predicate);
    let object_terms = normalized_terms(&groups.object);
    cards
        .iter()
        .filter(|card| {
            let normalized = normalize_text(&card.text);
            contains_any_normalized(&normalized, &subject_terms)
                && contains_any_normalized(&normalized, &predicate_terms)
                && contains_any_normalized(&normalized, &object_terms)
        })
        .collect()
}

fn entity_intro_answer(
    frame: Option<&RuntimeQuestionFrame>,
    cards: &[EvidenceCard],
) -> Option<String> {
    let frame = frame.filter(|frame| frame.intent == "entity_query")?;
    let subject = frame.subject.as_ref().or(frame.object.as_ref())?;
    let terms = normalized_terms(&subject.identity_terms());
    let direct_card = cards.iter().find(|card| {
        let normalized = normalize_text(&card.text);
        contains_any_normalized(&normalized, &terms)
    });
    let Some(card) = direct_card else {
        return Some(format!(
            "就当前证据包看，没有命中关于{}的直接材料，不能可靠概括这个人物。",
            subject.canonical
        ));
    };
    Some(format!(
        "就当前证据包看，{}的直接材料是{}：{}。这能支持其在该处的文本定位和相关人物关系；更完整的性格或结局概括，需要继续命中对应情节。",
        subject.canonical,
        card.source_title,
        short_quote(&card.text)
    ))
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
    serde_json::from_value(value.clone()).ok()
}

fn predicate_terms(predicate: &RuntimeQuestionFramePredicate) -> Vec<String> {
    let mut terms = vec![predicate.label.clone(), predicate.id.clone()];
    terms.extend(predicate.aliases.clone());
    terms.extend(predicate.evidence_terms.clone());
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
