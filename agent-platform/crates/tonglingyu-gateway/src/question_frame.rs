use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::context_rules;

pub(crate) const QUESTION_FRAME_SCHEMA_VERSION: &str = "tonglingyu.question_frame.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct QuestionFrame {
    pub(crate) schema_version: String,
    pub(crate) intent: String,
    pub(crate) canonical_question: String,
    pub(crate) subject: Option<QuestionFrameEntity>,
    pub(crate) predicate: Option<QuestionFramePredicate>,
    pub(crate) object: Option<QuestionFrameEntity>,
    pub(crate) source_scope: String,
    pub(crate) required_evidence_types: Vec<String>,
    pub(crate) confidence: f64,
    pub(crate) needs_clarification: bool,
    pub(crate) clarification_question: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct QuestionFrameEntity {
    pub(crate) canonical: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct QuestionFramePredicate {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) evidence_terms: Vec<String>,
}

impl QuestionFrame {
    pub(crate) fn audit_json(&self) -> Value {
        json!(self)
    }

    pub(crate) fn entities(&self) -> Vec<String> {
        [self.subject.as_ref(), self.object.as_ref()]
            .into_iter()
            .flatten()
            .map(|entity| entity.canonical.clone())
            .collect()
    }

    pub(crate) fn with_canonical_question(mut self, canonical_question: String) -> Self {
        self.canonical_question = canonical_question;
        self
    }
}

pub(crate) fn build_question_frame(question: &str) -> Result<QuestionFrame> {
    let source_scope = context_rules::default_question_frame_source_scope()?;
    let subjects = context_rules::subject_mentions_in_text(question)?;
    let predicate = context_rules::predicate_in_text(question)?;
    if let Some(predicate) = predicate {
        let subject = subjects
            .first()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        let object = if context_rules::relation_question_has_object_placeholder(question)? {
            None
        } else {
            subjects
                .get(1)
                .map(|canonical| frame_entity(canonical, "current_question"))
                .transpose()?
        };
        let confidence = if subject.is_some() { 0.9 } else { 0.35 };
        let needs_clarification = subject.is_none();
        let clarification_question =
            needs_clarification.then(|| "请说明这条关系题中的主体人物。".to_string());
        return Ok(QuestionFrame {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            intent: "relation_query".to_string(),
            canonical_question: canonical_relation_question(
                question,
                &subject,
                &predicate.label,
                &object,
            ),
            subject,
            predicate: Some(QuestionFramePredicate {
                id: predicate.id,
                label: predicate.label,
                aliases: predicate.aliases,
                evidence_terms: predicate.evidence_terms,
            }),
            object,
            source_scope,
            required_evidence_types: predicate.required_evidence_types,
            confidence,
            needs_clarification,
            clarification_question,
        });
    }

    let subject = subjects
        .first()
        .map(|canonical| frame_entity(canonical, "current_question"))
        .transpose()?;
    Ok(QuestionFrame {
        schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
        intent: if subject.is_some() {
            "entity_query".to_string()
        } else {
            "general_query".to_string()
        },
        canonical_question: question.to_string(),
        subject,
        predicate: None,
        object: None,
        source_scope,
        required_evidence_types: Vec::new(),
        confidence: 1.0,
        needs_clarification: false,
        clarification_question: None,
    })
}

pub(crate) fn resolve_relation_entity_followup(
    question: &str,
    anchor_question: &str,
    used_context_ref: &str,
) -> Result<Option<(String, QuestionFrame, String)>> {
    let current_subjects = context_rules::subject_mentions_in_text(question)?;
    if current_subjects.len() != 1
        || !question_key(question).ends_with('呢')
        || !context_rules::relation_followup_has_prefix(question)?
    {
        return Ok(None);
    }
    let anchor_frame = build_question_frame(anchor_question)?;
    if anchor_frame.intent != "relation_query"
        || anchor_frame.subject.is_none()
        || anchor_frame.predicate.is_none()
        || anchor_frame.object.is_some()
    {
        return Ok(None);
    }
    let mut frame = anchor_frame;
    frame.object = Some(frame_entity(&current_subjects[0], "current_window")?);
    frame.needs_clarification = false;
    frame.clarification_question = None;
    frame.confidence = 0.91;
    let predicate_label = frame
        .predicate
        .as_ref()
        .expect("relation frame predicate checked above")
        .label
        .clone();
    frame.canonical_question =
        canonical_relation_question(question, &frame.subject, &predicate_label, &frame.object);
    Ok(Some((
        frame.canonical_question.clone(),
        frame,
        used_context_ref.to_string(),
    )))
}

pub(crate) fn unresolved_frame(
    question: &str,
    reason: &str,
    clarification: &str,
) -> Result<QuestionFrame> {
    let mut frame = build_question_frame(question)?;
    frame.confidence = 0.2;
    frame.needs_clarification = true;
    frame.clarification_question = Some(clarification.to_string());
    frame.intent = reason.to_string();
    Ok(frame)
}

fn frame_entity(canonical: &str, source: &str) -> Result<QuestionFrameEntity> {
    Ok(QuestionFrameEntity {
        canonical: canonical.to_string(),
        aliases: context_rules::subject_aliases(canonical)?,
        source: source.to_string(),
    })
}

fn canonical_relation_question(
    fallback: &str,
    subject: &Option<QuestionFrameEntity>,
    predicate_label: &str,
    object: &Option<QuestionFrameEntity>,
) -> String {
    match (subject, object) {
        (Some(subject), Some(object)) => {
            format!(
                "{}{}过{}吗？",
                subject.canonical, predicate_label, object.canonical
            )
        }
        (Some(subject), None) => format!("{}{}过谁？", subject.canonical, predicate_label),
        _ => fallback.to_string(),
    }
}

fn question_key(text: &str) -> String {
    text.trim()
        .trim_matches(|ch| matches!(ch, '?' | '？' | '!' | '！' | '。' | '.' | ' '))
        .split_whitespace()
        .collect::<String>()
}

#[cfg(test)]
mod tests;
