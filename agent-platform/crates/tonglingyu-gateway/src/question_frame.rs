use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::context_rules;

pub(crate) const QUESTION_FRAME_SCHEMA_VERSION: &str = "tonglingyu.question_frame.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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
    #[serde(default)]
    pub(crate) open_slot: Option<String>,
    #[serde(default)]
    pub(crate) context_binding: Option<QuestionFrameContextBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionFrameEntity {
    pub(crate) canonical: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionFramePredicate {
    pub(crate) id: String,
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    pub(crate) evidence_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionFrameContextBinding {
    pub(crate) used_context_refs: Vec<String>,
    pub(crate) binding_reason: String,
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

    pub(crate) fn with_context_binding(
        mut self,
        used_context_refs: Vec<String>,
        binding_reason: impl Into<String>,
    ) -> Self {
        self.context_binding = Some(QuestionFrameContextBinding {
            used_context_refs,
            binding_reason: binding_reason.into(),
        });
        self
    }
}

pub(crate) fn build_question_frame(question: &str) -> Result<QuestionFrame> {
    let source_scope = context_rules::question_source_scope(question)?;
    let subjects = context_rules::subject_mentions_in_text(question)?;
    if let Some(attribute) = context_rules::parse_attribute_question(question)? {
        let subject = attribute
            .subject
            .as_ref()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        let object = attribute
            .object
            .as_ref()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        let needs_clarification = attribute.open_slot.is_some();
        let confidence = if needs_clarification { 0.42 } else { 0.9 };
        let mut aliases = attribute.attribute.aliases;
        aliases.extend(attribute.attribute.comparison_terms);
        return Ok(QuestionFrame {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            intent: attribute.intent,
            canonical_question: question.to_string(),
            subject,
            predicate: Some(QuestionFramePredicate {
                id: attribute.attribute.id,
                label: attribute.attribute.label,
                aliases,
                evidence_terms: attribute.attribute.evidence_terms,
            }),
            object,
            source_scope,
            required_evidence_types: attribute.attribute.required_evidence_types,
            confidence,
            needs_clarification,
            clarification_question: needs_clarification
                .then(|| clarification_for_open_slot(attribute.open_slot.as_deref())),
            open_slot: attribute.open_slot,
            context_binding: None,
        });
    }
    if let Some(relation) = context_rules::parse_relation_question(question)? {
        let subject = relation
            .subject
            .as_ref()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        let object = relation
            .object
            .as_ref()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        let confidence = if subject.is_some() && object.is_some() {
            0.9
        } else if relation.explicit_open_slot {
            0.86
        } else {
            0.35
        };
        let needs_clarification = relation.open_slot.is_some() && !relation.explicit_open_slot;
        let clarification_question =
            needs_clarification.then(|| clarification_for_open_slot(relation.open_slot.as_deref()));
        return Ok(QuestionFrame {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            intent: "relation_query".to_string(),
            canonical_question: canonical_relation_question(
                question,
                &subject,
                &relation.predicate.label,
                &object,
            ),
            subject,
            predicate: Some(QuestionFramePredicate {
                id: relation.predicate.id,
                label: relation.predicate.label,
                aliases: relation.predicate.aliases,
                evidence_terms: relation.predicate.evidence_terms,
            }),
            object,
            source_scope,
            required_evidence_types: relation.predicate.required_evidence_types,
            confidence,
            needs_clarification,
            clarification_question,
            open_slot: relation.open_slot,
            context_binding: None,
        });
    }
    if let Some(relation) = context_rules::parse_unknown_relation_predicate_question(question)? {
        let subject = relation
            .subject
            .as_ref()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        let object = relation
            .object
            .as_ref()
            .map(|canonical| frame_entity(canonical, "current_question"))
            .transpose()?;
        return Ok(QuestionFrame {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            intent: "unknown_relation_predicate".to_string(),
            canonical_question: question.to_string(),
            subject,
            predicate: None,
            object,
            source_scope,
            required_evidence_types: Vec::new(),
            confidence: 0.35,
            needs_clarification: true,
            clarification_question: Some(relation.clarification_question),
            open_slot: relation.open_slot,
            context_binding: None,
        });
    }
    let subject = subjects
        .first()
        .map(|canonical| frame_entity(canonical, "current_question"))
        .transpose()?;
    let is_count_query = context_rules::question_mentions_count(question)?;
    let is_evidence_query = context_rules::question_mentions_evidence_followup(question)?;
    let is_character_fate_query = context_rules::question_mentions_character_fate(question)?;
    if !is_character_fate_query {
        if let Some(location) = context_rules::parse_chapter_location_question(question)? {
            let location_subject = location
                .subject
                .as_ref()
                .map(|canonical| frame_entity(canonical, "current_question"))
                .transpose()?;
            let needs_clarification = location.event_phrase.is_none();
            return Ok(QuestionFrame {
                schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
                intent: "chapter_location_query".to_string(),
                canonical_question: question.to_string(),
                subject: location_subject,
                predicate: None,
                object: None,
                source_scope,
                required_evidence_types: context_rules::chapter_location_required_evidence_types()?,
                confidence: if needs_clarification { 0.38 } else { 0.88 },
                needs_clarification,
                clarification_question: needs_clarification
                    .then(context_rules::chapter_location_clarification_question)
                    .transpose()?,
                open_slot: needs_clarification.then(|| "event".to_string()),
                context_binding: None,
            });
        }
    }

    let needs_character_fate_clarification = is_character_fate_query && subject.is_none();
    let intent = if is_count_query {
        "count_query".to_string()
    } else if is_evidence_query {
        "evidence_query".to_string()
    } else if is_character_fate_query {
        "character_fate_query".to_string()
    } else if subject.is_some() {
        "entity_query".to_string()
    } else {
        "general_query".to_string()
    };
    Ok(QuestionFrame {
        schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
        intent,
        canonical_question: question.to_string(),
        subject,
        predicate: None,
        object: None,
        source_scope: source_scope.clone(),
        required_evidence_types: if is_count_query {
            context_rules::count_question_required_evidence_types()?
        } else if is_evidence_query {
            context_rules::evidence_followup_required_evidence_types()?
        } else if is_character_fate_query {
            context_rules::character_fate_required_evidence_types(&source_scope)?
        } else {
            Vec::new()
        },
        confidence: if needs_character_fate_clarification {
            0.42
        } else {
            1.0
        },
        needs_clarification: needs_character_fate_clarification,
        clarification_question: needs_character_fate_clarification
            .then(context_rules::character_fate_clarification_question)
            .transpose()?,
        open_slot: needs_character_fate_clarification.then(|| "subject".to_string()),
        context_binding: None,
    })
}

pub(crate) fn general_query_frame(question: &str) -> Result<QuestionFrame> {
    Ok(QuestionFrame {
        schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
        intent: "general_query".to_string(),
        canonical_question: question.to_string(),
        subject: None,
        predicate: None,
        object: None,
        source_scope: context_rules::question_source_scope(question)?,
        required_evidence_types: Vec::new(),
        confidence: 1.0,
        needs_clarification: false,
        clarification_question: None,
        open_slot: None,
        context_binding: None,
    })
}

pub(crate) fn resolve_relation_entity_followup(
    question: &str,
    anchor_question: &str,
    used_context_ref: &str,
) -> Result<Option<(String, QuestionFrame, String)>> {
    let current_subjects = context_rules::subject_mentions_in_text(question)?;
    if current_subjects.len() != 1 || context_rules::predicate_in_text(question)?.is_some() {
        return Ok(None);
    }
    if !context_rules::relation_followup_can_fill_open_object(question)? {
        return Ok(None);
    }
    if !context_rules::relation_followup_has_only_open_object_terms(question, &current_subjects[0])?
    {
        return Ok(None);
    }
    let anchor_frame = build_question_frame(anchor_question)?;
    if anchor_frame.intent != "relation_query"
        || anchor_frame.predicate.is_none()
        || !matches!(
            anchor_frame.open_slot.as_deref(),
            Some("subject" | "object")
        )
    {
        return Ok(None);
    }
    let mut frame = anchor_frame;
    match frame.open_slot.as_deref() {
        Some("subject") => {
            frame.subject = Some(frame_entity(&current_subjects[0], "current_window")?);
        }
        Some("object") => {
            frame.object = Some(frame_entity(&current_subjects[0], "current_window")?);
        }
        _ => return Ok(None),
    }
    frame.needs_clarification = false;
    frame.clarification_question = None;
    let binding_reason = format!(
        "filled_prior_open_{}_relation_slot",
        frame.open_slot.as_deref().unwrap_or("unknown")
    );
    frame.open_slot = None;
    frame.confidence = 0.91;
    let predicate_label = frame
        .predicate
        .as_ref()
        .expect("relation frame predicate checked above")
        .label
        .clone();
    frame.canonical_question =
        canonical_relation_question(question, &frame.subject, &predicate_label, &frame.object);
    frame.context_binding = Some(QuestionFrameContextBinding {
        used_context_refs: vec![used_context_ref.to_string()],
        binding_reason,
    });
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
    frame.open_slot = None;
    Ok(frame)
}

pub(crate) fn resolve_attribute_compare_followup(
    question: &str,
    anchor_question: &str,
    used_context_ref: &str,
) -> Result<Option<(String, QuestionFrame, String)>> {
    let mut frame = build_question_frame(question)?;
    if frame.intent != "attribute_compare"
        || frame.predicate.is_none()
        || !matches!(frame.open_slot.as_deref(), Some("subject" | "object"))
    {
        return Ok(None);
    }
    let anchor_frame = build_question_frame(anchor_question)?;
    let Some(anchor_entity) = anchor_frame.subject.or(anchor_frame.object) else {
        return Ok(None);
    };
    if frame
        .entities()
        .iter()
        .any(|entity| entity == &anchor_entity.canonical)
    {
        return Ok(None);
    }
    match frame.open_slot.as_deref() {
        Some("subject") => {
            frame.subject = Some(frame_entity(&anchor_entity.canonical, "current_window")?);
        }
        Some("object") => {
            frame.object = Some(frame_entity(&anchor_entity.canonical, "current_window")?);
        }
        _ => return Ok(None),
    }
    frame.needs_clarification = false;
    frame.clarification_question = None;
    let filled_slot = frame
        .open_slot
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    frame.open_slot = None;
    frame.confidence = 0.91;
    frame.canonical_question = canonical_attribute_compare_question(question, &frame);
    frame.context_binding = Some(QuestionFrameContextBinding {
        used_context_refs: vec![used_context_ref.to_string()],
        binding_reason: format!("filled_prior_attribute_compare_{filled_slot}"),
    });
    Ok(Some((
        frame.canonical_question.clone(),
        frame,
        used_context_ref.to_string(),
    )))
}

pub(crate) fn validate_agent_question_frame_candidate(
    value: &Value,
    resolved_question: &str,
) -> Result<QuestionFrame> {
    let frame: QuestionFrame = serde_json::from_value(value.clone())
        .map_err(|error| anyhow!("question_frame_candidate_deserialize_failed: {error}"))?;
    validate_question_frame_contract(&frame, resolved_question)?;
    Ok(frame)
}

fn validate_question_frame_contract(frame: &QuestionFrame, resolved_question: &str) -> Result<()> {
    if frame.schema_version != QUESTION_FRAME_SCHEMA_VERSION {
        return Err(anyhow!(
            "question_frame_candidate_schema_version_mismatch: {}",
            frame.schema_version
        ));
    }
    if frame.canonical_question.trim() != resolved_question.trim() {
        return Err(anyhow!("question_frame_candidate_question_mismatch"));
    }
    if frame.source_scope != context_rules::question_source_scope(resolved_question)? {
        return Err(anyhow!("question_frame_candidate_source_scope_mismatch"));
    }
    validate_frame_entity(frame.subject.as_ref())?;
    validate_frame_entity(frame.object.as_ref())?;
    let expected_open_slot = expected_open_slot(frame);
    if frame.open_slot != expected_open_slot {
        return Err(anyhow!("question_frame_candidate_open_slot_mismatch"));
    }
    match frame.intent.as_str() {
        "relation_query" => {
            let Some(predicate) = &frame.predicate else {
                return Err(anyhow!(
                    "question_frame_candidate_relation_missing_predicate"
                ));
            };
            let Some(rule) = context_rules::predicate_by_id(&predicate.id)? else {
                return Err(anyhow!("question_frame_candidate_unknown_predicate"));
            };
            if predicate.label != rule.label {
                return Err(anyhow!("question_frame_candidate_predicate_label_mismatch"));
            }
            if frame.required_evidence_types != rule.required_evidence_types {
                return Err(anyhow!(
                    "question_frame_candidate_required_evidence_types_mismatch"
                ));
            }
        }
        "attribute_query" | "attribute_at_event" | "attribute_compare" => {
            let Some(predicate) = &frame.predicate else {
                return Err(anyhow!(
                    "question_frame_candidate_attribute_missing_predicate"
                ));
            };
            let Some(rule) = context_rules::attribute_by_id(&predicate.id)? else {
                return Err(anyhow!("question_frame_candidate_unknown_attribute"));
            };
            if predicate.label != rule.label {
                return Err(anyhow!("question_frame_candidate_attribute_label_mismatch"));
            }
            if frame.required_evidence_types != rule.required_evidence_types {
                return Err(anyhow!(
                    "question_frame_candidate_required_evidence_types_mismatch"
                ));
            }
            if frame.intent == "attribute_compare"
                && (frame.subject.is_none() || frame.object.is_none())
                && !frame.needs_clarification
            {
                return Err(anyhow!(
                    "question_frame_candidate_attribute_compare_missing_side"
                ));
            }
        }
        "evidence_query" => {
            if frame.predicate.is_some() {
                return Err(anyhow!(
                    "question_frame_candidate_non_relation_has_predicate"
                ));
            }
            if frame.required_evidence_types
                != context_rules::evidence_followup_required_evidence_types()?
            {
                return Err(anyhow!(
                    "question_frame_candidate_required_evidence_types_mismatch"
                ));
            }
        }
        "count_query" => {
            if frame.predicate.is_some() {
                return Err(anyhow!(
                    "question_frame_candidate_non_relation_has_predicate"
                ));
            }
            if frame.required_evidence_types
                != context_rules::count_question_required_evidence_types()?
            {
                return Err(anyhow!(
                    "question_frame_candidate_required_evidence_types_mismatch"
                ));
            }
        }
        "character_fate_query" => {
            if frame.predicate.is_some() {
                return Err(anyhow!(
                    "question_frame_candidate_non_relation_has_predicate"
                ));
            }
            if frame.required_evidence_types
                != context_rules::character_fate_required_evidence_types(&frame.source_scope)?
            {
                return Err(anyhow!(
                    "question_frame_candidate_required_evidence_types_mismatch"
                ));
            }
            let has_entity = frame.subject.is_some() || frame.object.is_some();
            if has_entity && frame.needs_clarification {
                return Err(anyhow!(
                    "question_frame_candidate_unneeded_character_fate_clarification"
                ));
            }
            if !has_entity && (!frame.needs_clarification || frame.clarification_question.is_none())
            {
                return Err(anyhow!(
                    "question_frame_candidate_character_fate_requires_subject"
                ));
            }
        }
        "chapter_location_query" => {
            if frame.predicate.is_some() {
                return Err(anyhow!(
                    "question_frame_candidate_non_relation_has_predicate"
                ));
            }
            if frame.required_evidence_types
                != context_rules::chapter_location_required_evidence_types()?
            {
                return Err(anyhow!(
                    "question_frame_candidate_required_evidence_types_mismatch"
                ));
            }
            if frame.needs_clarification
                && (frame.open_slot.as_deref() != Some("event")
                    || frame.clarification_question.is_none())
            {
                return Err(anyhow!(
                    "question_frame_candidate_chapter_location_requires_event"
                ));
            }
        }
        "unknown_relation_predicate" => {
            if frame.predicate.is_some() || !frame.required_evidence_types.is_empty() {
                return Err(anyhow!(
                    "question_frame_candidate_non_relation_has_predicate"
                ));
            }
            if !frame.needs_clarification || frame.clarification_question.is_none() {
                return Err(anyhow!(
                    "question_frame_candidate_unknown_relation_requires_clarification"
                ));
            }
        }
        "entity_query" | "general_query" => {
            if frame.predicate.is_some() || !frame.required_evidence_types.is_empty() {
                return Err(anyhow!(
                    "question_frame_candidate_non_relation_has_predicate"
                ));
            }
        }
        other if other.starts_with("unresolved_") => {}
        _ => return Err(anyhow!("question_frame_candidate_unknown_intent")),
    }
    Ok(())
}

fn expected_open_slot(frame: &QuestionFrame) -> Option<String> {
    if frame.intent == "character_fate_query" && frame.subject.is_none() && frame.object.is_none() {
        return Some("subject".to_string());
    }
    if frame.intent == "relation_query" || frame.intent == "unknown_relation_predicate" {
        if frame.subject.is_none() {
            return Some("subject".to_string());
        }
        if frame.object.is_none() {
            return Some("object".to_string());
        }
    }
    if frame.intent == "attribute_compare" {
        if frame.subject.is_none() {
            return Some("subject".to_string());
        }
        if frame.object.is_none() {
            return Some("object".to_string());
        }
    }
    if frame.intent == "chapter_location_query" && frame.needs_clarification {
        return Some("event".to_string());
    }
    None
}

fn validate_frame_entity(entity: Option<&QuestionFrameEntity>) -> Result<()> {
    if let Some(entity) = entity {
        context_rules::subject_aliases(&entity.canonical)
            .map_err(|_| anyhow!("question_frame_candidate_unknown_entity"))?;
    }
    Ok(())
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
        (None, Some(object)) => format!("谁{}过{}？", predicate_label, object.canonical),
        _ => fallback.to_string(),
    }
}

fn canonical_attribute_compare_question(fallback: &str, frame: &QuestionFrame) -> String {
    let Some(subject) = &frame.subject else {
        return fallback.to_string();
    };
    let Some(object) = &frame.object else {
        return fallback.to_string();
    };
    let Some(attribute) = &frame.predicate else {
        return fallback.to_string();
    };
    format!(
        "{}和{}相比，谁的{}更大？",
        subject.canonical, object.canonical, attribute.label
    )
}

pub(crate) fn relation_subject_from_open_object_anchor(
    anchor_question: &str,
) -> Result<Option<QuestionFrameEntity>> {
    let anchor_frame = build_question_frame(anchor_question)?;
    if anchor_frame.intent != "relation_query"
        || anchor_frame.subject.is_none()
        || anchor_frame.predicate.is_none()
        || anchor_frame.open_slot.as_deref() != Some("object")
    {
        return Ok(None);
    }
    Ok(anchor_frame.subject)
}

fn clarification_for_open_slot(open_slot: Option<&str>) -> String {
    match open_slot {
        Some("subject") => "请说明这条关系题中的主体人物。".to_string(),
        Some("object") => "请说明这条关系题中的对象人物。".to_string(),
        _ => "请补充这条关系题缺少的人物。".to_string(),
    }
}

#[cfg(test)]
mod tests;
