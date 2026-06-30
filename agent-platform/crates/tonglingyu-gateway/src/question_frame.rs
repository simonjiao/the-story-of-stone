use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::context_rules;

pub(crate) const QUESTION_FRAME_SCHEMA_VERSION: &str = "tonglingyu.question_frame.v2";

#[derive(Debug, Clone, PartialEq)]
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
    pub(crate) open_slot: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2Wire {
    schema_version: String,
    frame_id: String,
    original_question: String,
    normalized_question: String,
    task: String,
    slots: QuestionFrameV2Slots,
    answer_target: QuestionFrameV2AnswerTarget,
    evidence_contract: QuestionFrameV2EvidenceContract,
    #[serde(default)]
    subquestions: Vec<QuestionFrameV2Wire>,
    clarification: QuestionFrameV2Clarification,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    context_binding: Option<QuestionFrameContextBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2Slots {
    #[serde(default)]
    subject: Option<QuestionFrameV2EntitySlot>,
    #[serde(default)]
    object: Option<QuestionFrameV2EntitySlot>,
    #[serde(default)]
    relation: Option<QuestionFrameV2RelationSlot>,
    #[serde(default)]
    attribute: Option<QuestionFrameV2RelationSlot>,
    #[serde(default)]
    event: Option<QuestionFrameV2EventSlot>,
    #[serde(default)]
    entity_group: Option<QuestionFrameV2EntityGroupSlot>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    evidence_focus: Option<String>,
    source_scope: QuestionFrameV2SourceScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2EntitySlot {
    #[serde(rename = "type")]
    entity_type: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2EntityGroupSlot {
    #[serde(rename = "type")]
    group_type: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2RelationSlot {
    id: String,
    label: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    evidence_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2EventSlot {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    trigger: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2SourceScope {
    work: String,
    range: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2AnswerTarget {
    #[serde(rename = "type")]
    target_type: String,
    #[serde(default)]
    approx_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2EvidenceContract {
    required_types: Vec<String>,
    #[serde(default)]
    supporting_types: Vec<String>,
    min_answer_basis: usize,
    require_claim_evidence_map: bool,
    allow_navigation_hint_as_answer_basis: bool,
    citation_granularity: String,
    unsupported_behavior: String,
    #[serde(default)]
    require_per_case_evidence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QuestionFrameV2Clarification {
    needed: bool,
    #[serde(default)]
    open_slots: Vec<String>,
    #[serde(default)]
    question: Option<String>,
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

    fn to_v2_wire(&self) -> QuestionFrameV2Wire {
        QuestionFrameV2Wire {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            frame_id: "qf_primary".to_string(),
            original_question: self.canonical_question.clone(),
            normalized_question: self.canonical_question.clone(),
            task: v2_task_for_frame(self),
            slots: v2_slots_for_frame(self),
            answer_target: v2_answer_target_for_frame(self),
            evidence_contract: v2_evidence_contract_for_frame(self),
            subquestions: v2_subquestions_for_frame(self),
            clarification: QuestionFrameV2Clarification {
                needed: self.needs_clarification,
                open_slots: self.open_slot.clone().into_iter().collect(),
                question: self.clarification_question.clone(),
            },
            confidence: Some(self.confidence),
            context_binding: self.context_binding.clone(),
        }
    }

    fn from_v2_wire(wire: QuestionFrameV2Wire) -> Result<Self> {
        if wire.schema_version != QUESTION_FRAME_SCHEMA_VERSION {
            return Err(anyhow!(
                "question_frame_candidate_schema_version_mismatch: {}",
                wire.schema_version
            ));
        }
        let predicate = wire
            .slots
            .relation
            .as_ref()
            .or(wire.slots.attribute.as_ref())
            .map(|slot| QuestionFramePredicate {
                id: slot.id.clone(),
                label: slot.label.clone(),
                aliases: slot.aliases.clone(),
                evidence_terms: slot.evidence_terms.clone(),
            });
        let subject = wire.slots.subject.as_ref().map(v2_entity_to_frame_entity);
        let object = wire.slots.object.as_ref().map(v2_entity_to_frame_entity);
        let mut required_evidence_types = wire.evidence_contract.required_types.clone();
        for item in &wire.evidence_contract.supporting_types {
            if !required_evidence_types
                .iter()
                .any(|existing| existing == item)
            {
                required_evidence_types.push(item.clone());
            }
        }
        let intent = v2_intent_for_wire(&wire);
        Ok(Self {
            schema_version: wire.schema_version,
            intent,
            canonical_question: wire.normalized_question,
            subject,
            predicate,
            object,
            source_scope: wire.slots.source_scope.range,
            required_evidence_types,
            confidence: wire.confidence.unwrap_or(1.0),
            needs_clarification: wire.clarification.needed,
            clarification_question: wire.clarification.question,
            open_slot: wire.clarification.open_slots.first().cloned(),
            context_binding: wire.context_binding,
        })
    }
}

impl Serialize for QuestionFrame {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_v2_wire().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QuestionFrame {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QuestionFrameV2Wire::deserialize(deserializer)?;
        Self::from_v2_wire(wire).map_err(serde::de::Error::custom)
    }
}

fn v2_entity_to_frame_entity(slot: &QuestionFrameV2EntitySlot) -> QuestionFrameEntity {
    QuestionFrameEntity {
        canonical: slot.name.clone(),
        aliases: slot.aliases.clone(),
        source: slot.source.clone(),
    }
}

fn v2_intent_for_wire(wire: &QuestionFrameV2Wire) -> String {
    match wire.task.as_str() {
        "verify_relation" => "relation_query".to_string(),
        "compare" => "attribute_compare".to_string(),
        "locate_event" => {
            if wire
                .slots
                .event
                .as_ref()
                .is_some_and(|event| event.event_type == "death")
            {
                "character_fate_query".to_string()
            } else {
                "chapter_location_query".to_string()
            }
        }
        "count_occurrences" => "count_query".to_string(),
        "extract_evidence" => "evidence_query".to_string(),
        "summarize_entity" => "entity_query".to_string(),
        "clarify" => "unknown_relation_predicate".to_string(),
        _ if wire.slots.relation.is_some() => "relation_query".to_string(),
        _ if wire.slots.attribute.is_some() => "attribute_query".to_string(),
        _ if wire
            .slots
            .event
            .as_ref()
            .is_some_and(|event| event.event_type == "death") =>
        {
            "character_fate_query".to_string()
        }
        _ => "general_query".to_string(),
    }
}

fn v2_task_for_frame(frame: &QuestionFrame) -> String {
    if is_analysis_composition_question(&frame.canonical_question) {
        return "compose_analysis".to_string();
    }
    if frame.intent == "character_fate_query" && asks_event_location(&frame.canonical_question) {
        return "locate_event".to_string();
    }
    match frame.intent.as_str() {
        "relation_query" => "verify_relation".to_string(),
        "unknown_relation_predicate" => "clarify".to_string(),
        "attribute_compare" => "compare".to_string(),
        "attribute_query" | "attribute_at_event" => "answer_fact".to_string(),
        "chapter_location_query" => "locate_event".to_string(),
        "character_fate_query" => "answer_fact".to_string(),
        "evidence_query" => "extract_evidence".to_string(),
        "count_query" => "count_occurrences".to_string(),
        "entity_query" => "summarize_entity".to_string(),
        _ => "answer_fact".to_string(),
    }
}

fn v2_slots_for_frame(frame: &QuestionFrame) -> QuestionFrameV2Slots {
    let relation = (frame.intent == "relation_query")
        .then(|| frame.predicate.as_ref())
        .flatten()
        .map(v2_relation_slot);
    let attribute = matches!(
        frame.intent.as_str(),
        "attribute_query" | "attribute_at_event" | "attribute_compare"
    )
    .then(|| frame.predicate.as_ref())
    .flatten()
    .map(v2_relation_slot);
    QuestionFrameV2Slots {
        subject: frame.subject.as_ref().map(v2_entity_slot),
        object: frame.object.as_ref().map(v2_entity_slot),
        relation,
        attribute,
        event: v2_event_slot_for_frame(frame),
        entity_group: v2_entity_group_for_frame(frame),
        topic: v2_topic_for_frame(frame),
        evidence_focus: v2_evidence_focus_for_frame(frame),
        source_scope: QuestionFrameV2SourceScope {
            work: "hongloumeng".to_string(),
            range: frame.source_scope.clone(),
        },
    }
}

fn v2_entity_slot(entity: &QuestionFrameEntity) -> QuestionFrameV2EntitySlot {
    QuestionFrameV2EntitySlot {
        entity_type: "character".to_string(),
        name: entity.canonical.clone(),
        aliases: entity.aliases.clone(),
        source: entity.source.clone(),
    }
}

fn v2_relation_slot(predicate: &QuestionFramePredicate) -> QuestionFrameV2RelationSlot {
    QuestionFrameV2RelationSlot {
        id: predicate.id.clone(),
        label: predicate.label.clone(),
        aliases: predicate.aliases.clone(),
        evidence_terms: predicate.evidence_terms.clone(),
    }
}

fn v2_event_slot_for_frame(frame: &QuestionFrame) -> Option<QuestionFrameV2EventSlot> {
    let question = frame.canonical_question.as_str();
    if contains_any_local(question, &["死", "去世", "亡故", "病逝", "夭亡"]) {
        return Some(QuestionFrameV2EventSlot {
            event_type: "death".to_string(),
            trigger: first_matching_term(question, &["死的", "死", "去世", "亡故", "病逝", "夭亡"]),
        });
    }
    if contains_any_local(question, &["进贾府", "進賈府", "进府", "進府"]) {
        return Some(QuestionFrameV2EventSlot {
            event_type: "enter_jia_household".to_string(),
            trigger: first_matching_term(question, &["进贾府", "進賈府", "进府", "進府"]),
        });
    }
    if contains_any_local(question, &["葬花"]) {
        return Some(QuestionFrameV2EventSlot {
            event_type: "bury_flowers".to_string(),
            trigger: Some("葬花".to_string()),
        });
    }
    if is_loss_or_theft_question(question) {
        return Some(QuestionFrameV2EventSlot {
            event_type: "loss_or_theft".to_string(),
            trigger: first_matching_term(
                question,
                &[
                    "良儿偷玉",
                    "良兒偷玉",
                    "偷玉",
                    "失玉",
                    "丢了",
                    "丟了",
                    "丢",
                    "丟",
                    "不见",
                    "不見",
                    "遗失",
                    "遺失",
                ],
            ),
        });
    }
    (frame.intent == "chapter_location_query").then(|| QuestionFrameV2EventSlot {
        event_type: "located_event".to_string(),
        trigger: None,
    })
}

fn v2_entity_group_for_frame(frame: &QuestionFrame) -> Option<QuestionFrameV2EntityGroupSlot> {
    if frame.canonical_question.contains("大观园") && frame.canonical_question.contains("丫鬟")
    {
        Some(QuestionFrameV2EntityGroupSlot {
            group_type: "character_group".to_string(),
            name: "大观园丫鬟".to_string(),
        })
    } else {
        None
    }
}

fn v2_topic_for_frame(frame: &QuestionFrame) -> Option<String> {
    if frame.canonical_question.contains("下层女性") && frame.canonical_question.contains("命运")
    {
        Some("古代下层女性的命运".to_string())
    } else {
        None
    }
}

fn v2_evidence_focus_for_frame(frame: &QuestionFrame) -> Option<String> {
    if frame.intent == "character_fate_query" || frame.canonical_question.contains("结局") {
        Some("character_fate".to_string())
    } else if frame.intent == "count_query" && is_loss_or_theft_question(&frame.canonical_question)
    {
        Some("loss_event".to_string())
    } else {
        None
    }
}

fn v2_answer_target_for_frame(frame: &QuestionFrame) -> QuestionFrameV2AnswerTarget {
    if is_analysis_composition_question(&frame.canonical_question) {
        return QuestionFrameV2AnswerTarget {
            target_type: "essay".to_string(),
            approx_chars: approx_chars_from_question(&frame.canonical_question),
        };
    }
    let target_type = if contains_any_local(
        &frame.canonical_question,
        &["第几回", "第幾回", "哪一回", "那一回"],
    ) {
        "chapter_no"
    } else if contains_any_local(
        &frame.canonical_question,
        &["什么时候", "什麼時候", "何时", "何時"],
    ) {
        "chapter_or_time"
    } else {
        match frame.intent.as_str() {
            "relation_query" => "yes_no",
            "attribute_compare" => "comparison",
            "count_query" => "count",
            "evidence_query" => "evidence_list",
            "chapter_location_query" => "chapter_no",
            "entity_query" => "entity_summary",
            _ => "explanation",
        }
    };
    QuestionFrameV2AnswerTarget {
        target_type: target_type.to_string(),
        approx_chars: None,
    }
}

fn v2_evidence_contract_for_frame(frame: &QuestionFrame) -> QuestionFrameV2EvidenceContract {
    let asks_commentary = contains_any_local(
        &frame.canonical_question,
        &["脂批", "批语", "批語", "评语", "評語"],
    );
    let asks_base_text = contains_any_local(
        &frame.canonical_question,
        &["原文", "正文", "本文", "文本", "原著"],
    );
    let (required_types, supporting_types, min_answer_basis, require_per_case_evidence) =
        if is_analysis_composition_question(&frame.canonical_question) {
            (
                vec!["base_text".to_string()],
                vec!["commentary".to_string()],
                4,
                true,
            )
        } else if frame.intent == "evidence_query" && asks_commentary && asks_base_text {
            (
                vec!["base_text".to_string(), "commentary".to_string()],
                Vec::new(),
                2,
                false,
            )
        } else if frame.intent == "evidence_query" && asks_commentary {
            (
                vec!["commentary".to_string()],
                vec!["base_text".to_string()],
                1,
                false,
            )
        } else if matches!(
            frame.intent.as_str(),
            "character_fate_query" | "chapter_location_query" | "count_query"
        ) {
            (
                vec!["base_text".to_string()],
                vec!["commentary".to_string()],
                1,
                false,
            )
        } else {
            (frame.required_evidence_types.clone(), Vec::new(), 1, false)
        };
    QuestionFrameV2EvidenceContract {
        required_types,
        supporting_types,
        min_answer_basis,
        require_claim_evidence_map: true,
        allow_navigation_hint_as_answer_basis: false,
        citation_granularity: "chapter_or_span".to_string(),
        unsupported_behavior: "fail_closed".to_string(),
        require_per_case_evidence,
    }
}

fn v2_subquestions_for_frame(frame: &QuestionFrame) -> Vec<QuestionFrameV2Wire> {
    if !is_analysis_composition_question(&frame.canonical_question) {
        return Vec::new();
    }
    let scope = QuestionFrameV2SourceScope {
        work: "hongloumeng".to_string(),
        range: frame.source_scope.clone(),
    };
    let base_contract = QuestionFrameV2EvidenceContract {
        required_types: vec!["base_text".to_string()],
        supporting_types: vec!["commentary".to_string()],
        min_answer_basis: 1,
        require_claim_evidence_map: true,
        allow_navigation_hint_as_answer_basis: false,
        citation_granularity: "chapter_or_span".to_string(),
        unsupported_behavior: "fail_closed".to_string(),
        require_per_case_evidence: true,
    };
    vec![
        QuestionFrameV2Wire {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            frame_id: "qf_identify_group".to_string(),
            original_question: frame.canonical_question.clone(),
            normalized_question: "识别大观园丫鬟人物集合".to_string(),
            task: "find_entities".to_string(),
            slots: QuestionFrameV2Slots {
                subject: None,
                object: None,
                relation: None,
                attribute: None,
                event: None,
                entity_group: v2_entity_group_for_frame(frame),
                topic: None,
                evidence_focus: None,
                source_scope: scope.clone(),
            },
            answer_target: QuestionFrameV2AnswerTarget {
                target_type: "entity_set".to_string(),
                approx_chars: None,
            },
            evidence_contract: base_contract.clone(),
            subquestions: Vec::new(),
            clarification: QuestionFrameV2Clarification {
                needed: false,
                open_slots: Vec::new(),
                question: None,
            },
            confidence: Some(frame.confidence),
            context_binding: None,
        },
        QuestionFrameV2Wire {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            frame_id: "qf_collect_fates".to_string(),
            original_question: frame.canonical_question.clone(),
            normalized_question: "收集大观园丫鬟结局证据".to_string(),
            task: "collect_entity_fates".to_string(),
            slots: QuestionFrameV2Slots {
                subject: None,
                object: None,
                relation: None,
                attribute: None,
                event: Some(QuestionFrameV2EventSlot {
                    event_type: "fate".to_string(),
                    trigger: Some("结局".to_string()),
                }),
                entity_group: v2_entity_group_for_frame(frame),
                topic: None,
                evidence_focus: Some("character_fate".to_string()),
                source_scope: scope.clone(),
            },
            answer_target: QuestionFrameV2AnswerTarget {
                target_type: "evidence_table".to_string(),
                approx_chars: None,
            },
            evidence_contract: base_contract.clone(),
            subquestions: Vec::new(),
            clarification: QuestionFrameV2Clarification {
                needed: false,
                open_slots: Vec::new(),
                question: None,
            },
            confidence: Some(frame.confidence),
            context_binding: None,
        },
        QuestionFrameV2Wire {
            schema_version: QUESTION_FRAME_SCHEMA_VERSION.to_string(),
            frame_id: "qf_compose".to_string(),
            original_question: frame.canonical_question.clone(),
            normalized_question: "写古代下层女性命运分析文章".to_string(),
            task: "compose_analysis".to_string(),
            slots: QuestionFrameV2Slots {
                subject: None,
                object: None,
                relation: None,
                attribute: None,
                event: None,
                entity_group: v2_entity_group_for_frame(frame),
                topic: v2_topic_for_frame(frame),
                evidence_focus: Some("character_fate".to_string()),
                source_scope: scope,
            },
            answer_target: QuestionFrameV2AnswerTarget {
                target_type: "essay".to_string(),
                approx_chars: approx_chars_from_question(&frame.canonical_question),
            },
            evidence_contract: QuestionFrameV2EvidenceContract {
                min_answer_basis: 4,
                ..base_contract
            },
            subquestions: Vec::new(),
            clarification: QuestionFrameV2Clarification {
                needed: false,
                open_slots: Vec::new(),
                question: None,
            },
            confidence: Some(frame.confidence),
            context_binding: None,
        },
    ]
}

fn is_analysis_composition_question(question: &str) -> bool {
    question.contains("写")
        && question.contains("分析")
        && (question.contains("文章") || question.contains("字"))
}

fn asks_event_location(question: &str) -> bool {
    contains_any_local(
        question,
        &[
            "第几回",
            "第幾回",
            "哪一回",
            "那一回",
            "什么时候",
            "什麼時候",
            "何时",
            "何時",
        ],
    )
}

fn contains_any_local(text: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn first_matching_term(text: &str, terms: &[&str]) -> Option<String> {
    terms
        .iter()
        .find(|term| text.contains(**term))
        .map(|term| (*term).to_string())
}

fn is_loss_or_theft_question(question: &str) -> bool {
    contains_any_local(
        question,
        &["通灵宝玉", "通靈寶玉", "通灵玉", "通靈玉", "那块玉", "那塊玉"],
    ) && contains_any_local(
        question,
        &[
            "良儿偷玉",
            "良兒偷玉",
            "偷玉",
            "失玉",
            "丢",
            "丟",
            "不见",
            "不見",
            "遗失",
            "遺失",
        ],
    )
}

fn approx_chars_from_question(question: &str) -> Option<usize> {
    if question.contains("1000") || question.contains("一千") {
        Some(1000)
    } else {
        None
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
    let intent = if is_evidence_query {
        "evidence_query".to_string()
    } else if is_character_fate_query {
        "character_fate_query".to_string()
    } else if is_count_query {
        "count_query".to_string()
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
        required_evidence_types: if is_evidence_query {
            context_rules::evidence_followup_required_evidence_types()?
        } else if is_character_fate_query {
            context_rules::character_fate_required_evidence_types(&source_scope)?
        } else if is_count_query {
            context_rules::count_question_required_evidence_types()?
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
