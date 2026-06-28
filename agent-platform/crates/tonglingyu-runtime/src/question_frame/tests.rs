use super::*;
use serde_json::json;

fn relation_frame() -> RuntimeQuestionFrame {
    serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "紫鹃服侍过史湘云吗？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "湘云"]},
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("frame")
}

fn card_with_text(text: &str) -> EvidenceCard {
    card_with_source_text("紅樓夢/第三回", text)
}

fn card_with_source_text(source_title: &str, text: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: source_title.to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: text.to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }
}

#[test]
fn relation_search_query_binds_subject_predicate_and_object_terms() {
    let frame = relation_frame();
    let query = relation_search_query("紫鹃服侍过史湘云吗？", Some(&frame));

    assert!(query.contains("紫鹃"));
    assert!(query.contains("服侍"));
    assert!(query.contains("丫鬟"));
    assert!(query.contains("史湘云"));
}

#[test]
fn frame_search_query_expands_entity_aliases() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");

    let query = frame_search_query("紫鹃在《红楼梦》里是什么样的人？", Some(&frame));

    assert!(query.contains("紫鹃"));
    assert!(query.contains("紫鵑"));
    assert!(query.contains("鹦哥"));
    assert!(query.contains("鸚哥"));
}

#[test]
fn frame_focus_terms_collects_entity_aliases_without_question_noise() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");

    let terms = frame_focus_terms(Some(&frame));

    assert!(terms.contains(&"紫鹃".to_string()));
    assert!(terms.contains(&"紫鵑".to_string()));
    assert!(terms.contains(&"鸚哥".to_string()));
    assert!(!terms.contains(&"红楼梦".to_string()));
}

#[test]
fn frame_search_query_includes_attribute_terms() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "attribute_at_event",
        "canonical_question": "林黛玉进贾府时几岁了",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大"],
            "evidence_terms": ["岁", "年纪"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("attribute frame");

    let query = frame_search_query("林黛玉进贾府时几岁了", Some(&frame));

    assert!(query.contains("林黛玉"));
    assert!(query.contains("黛玉"));
    assert!(query.contains("年龄"));
    assert!(query.contains("几岁"));
    assert!(query.contains("岁"));
}

#[test]
fn frame_search_query_includes_chapter_location_event_terms() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "chapter_location_query",
        "canonical_question": "林黛玉葬花是在那一回？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("chapter location frame");

    let query = frame_search_query("林黛玉葬花是在那一回？", Some(&frame));

    assert!(query.contains("葬花"));
    assert!(query.contains("埋香塚"));
    assert!(query.contains("林黛玉"));
}

#[test]
fn frame_focus_terms_include_chapter_location_expansion_terms() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "chapter_location_query",
        "canonical_question": "林黛玉葬花是在那一回？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("chapter location frame");

    let terms = frame_focus_terms(Some(&frame));

    assert!(terms.contains(&"葬花".to_string()));
    assert!(terms.contains(&"埋香塚".to_string()));
    assert!(terms.contains(&"林黛玉".to_string()));
}

#[test]
fn chapter_location_answer_asks_when_only_later_mention_is_available() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "chapter_location_query",
        "canonical_question": "林黛玉葬花是在那一回？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘", "顰兒"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("chapter location frame");
    let cards = vec![card_with_source_text(
        "紅樓夢/第030回",
        "丫頭，又像顰兒來葬花不成！因又自笑道。",
    )];

    let answer = question_frame_answer(Some(&frame), &cards).expect("chapter location answer");

    assert!(answer.contains("请明确"));
    assert!(answer.contains("第30回"));
    assert!(answer.contains("葬花"));
    assert!(!answer.contains("林黛玉葬花在《红楼梦》第30回"));
    assert!(!answer.contains("林黛玉可以先按命中材料作有边界的介绍"));
    assert!(!answer.contains("人物介绍"));
}

#[test]
fn chapter_location_answer_prefers_title_and_direct_base_text() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "chapter_location_query",
        "canonical_question": "林黛玉葬花是在那一回？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘", "顰兒"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("chapter location frame");
    let cards = vec![
        card_with_source_text(
            "紅樓夢/第027回",
            "==第二十七回 滴翠亭楊妃戲彩蝶 埋香塚飛燕泣殘紅==\n話說黛玉正自悲泣。",
        ),
        card_with_source_text(
            "紅樓夢/第027回",
            "寶玉道：「我就來。」說畢，等他二人去遠了，便把那花兜了起來，一直奔了那日同林黛玉葬桃花的去處來。",
        ),
        card_with_source_text("紅樓夢/第030回", "丫頭，又像顰兒來葬花不成！因又自笑道。"),
    ];

    let answer = question_frame_answer(Some(&frame), &cards).expect("chapter location answer");

    assert!(answer.contains("林黛玉葬花在《红楼梦》第27回"));
    assert!(answer.contains("回目是：《滴翠亭楊妃戲彩蝶 埋香塚飛燕泣殘紅》"));
    assert!(answer.contains("正文依据可见"));
    assert!(answer.contains("葬桃花"));
    assert!(!answer.contains("怕人笑说"));
    assert!(!answer.contains("人物介绍"));
}

#[test]
fn chapter_location_draft_requires_known_title_cue() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "chapter_location_query",
        "canonical_question": "林黛玉葬花是在那一回？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘", "顰兒"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("chapter location frame");
    let cards = vec![card_with_source_text(
        "紅樓夢/第027回",
        "==第二十七回 滴翠亭楊妃戲彩蝶 埋香塚飛燕泣殘紅==\n話說黛玉正自悲泣。",
    )];

    assert_eq!(
        chapter_location_draft_rejection_reason(
            Some(&frame),
            &cards,
            "林黛玉葬花在《红楼梦》第二十七回。"
        ),
        Some("chapter_location_title_cue_missing")
    );
    assert_eq!(
        chapter_location_draft_rejection_reason(
            Some(&frame),
            &cards,
            "林黛玉葬花在《红楼梦》第二十七回，回目有“埋香塚飛燕泣殘紅”。"
        ),
        None
    );
}

#[test]
fn chapter_location_accepts_quoted_title_cue_from_commentary_anchor() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "chapter_location_query",
        "canonical_question": "林黛玉进贾府在第几回",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘", "顰兒"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("chapter location frame");
    let cards = vec![card_with_source_text(
        "脂硯齋重評石頭記/第三回",
        "第003回｜批语锚点\n甲侧批“二字”位于脂批 source 的第三回 header 中，评回目“榮國府收養林黛玉”里的“收養”二字。\nion = 第三回 金陵城起復賈雨村 榮國府收養",
    )];

    assert_eq!(
        chapter_location_draft_rejection_reason(
            Some(&frame),
            &cards,
            "林黛玉进贾府在《红楼梦》第三回，回目为“荣国府收养林黛玉”。"
        ),
        None
    );
    assert_eq!(
        chapter_location_draft_rejection_reason(
            Some(&frame),
            &cards,
            "林黛玉进贾府在《红楼梦》第三回。"
        ),
        Some("chapter_location_title_cue_missing")
    );
}

#[test]
fn entity_query_misclassification_for_chapter_location_only_clarifies() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "林黛玉葬花是在那一回？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘", "顰兒"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("misclassified entity frame");
    let cards = vec![card_with_source_text(
        "紅樓夢/第030回",
        "丫頭，又像顰兒來葬花不成！因又自笑道。",
    )];

    let answer = question_frame_answer(Some(&frame), &cards).expect("clarification");

    assert!(answer.contains("请明确要定位的具体情节"));
    assert!(answer.contains("不能改答成人物概括"));
    assert!(!answer.contains("林黛玉可以先按命中材料作有边界的介绍"));
}

#[test]
fn attribute_question_answer_does_not_fall_back_to_entity_intro() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "attribute_at_event",
        "canonical_question": "林黛玉进贾府时几岁了",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大"],
            "evidence_terms": ["岁", "年纪"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("attribute frame");
    let cards = vec![card_with_text("顰兒來葬花不成，寶玉自笑。")];

    let answer = question_frame_answer(Some(&frame), &cards).expect("attribute answer");

    assert!(answer.contains("年龄"));
    assert!(answer.contains("还没有直接命中"));
    assert!(!answer.contains("林黛玉可以先按命中材料作有边界的介绍"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
}

#[test]
fn attribute_age_answer_uses_bounded_inference_from_age_cue() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "attribute_at_event",
        "canonical_question": "林黛玉进贾府多大了",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大"],
            "evidence_terms": ["岁", "年纪", "年方"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("attribute frame");
    let cards = vec![card_with_source_text(
        "脂硯齋重評石頭記甲戌本/第二回",
        "今只有嫡妻賈氏，生得一女，乳名黛玉，年方五歲。夫妻無子，故愛女如珍。",
    )];

    let answer = question_frame_answer(Some(&frame), &cards).expect("attribute answer");

    assert!(answer.contains("年方五歲"));
    assert!(answer.contains("大约5岁上下") || answer.contains("大约5歲上下"));
    assert!(answer.contains("不能说成更精确的定数"));
    assert!(!answer.contains("还没有直接命中"));
}

#[test]
fn attribute_age_rationale_followup_explains_evidence_chain() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "attribute_at_event",
        "canonical_question": "关于林黛玉进贾府时多大了，你的推理逻辑是什么",
        "context_binding": {
            "binding_reason": "filled_prior_contextual_continuation",
            "used_context_refs": ["current_window"]
        },
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大"],
            "evidence_terms": ["岁", "年纪", "年方"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("attribute frame");
    let cards = vec![card_with_source_text(
        "紅樓夢/第002回",
        "今只有嫡妻賈氏，生得一女，乳名黛玉，年方五歲。夫妻無子，故愛女如珍。",
    )];

    let answer = question_frame_answer(Some(&frame), &cards).expect("rationale answer");

    assert!(answer.contains("推理链条是"));
    assert!(answer.contains("年方五歲"));
    assert!(answer.contains("不能进一步说成精确年龄"));
    assert!(!answer.contains("你的推理逻辑是什么不能只凭"));
}

#[test]
fn attribute_card_support_extracts_age_claim_value() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "attribute_at_event",
        "canonical_question": "林黛玉进贾府多大了",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大"],
            "evidence_terms": ["岁", "年纪", "年方"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("attribute frame");
    let card = card_with_source_text(
        "脂硯齋重評石頭記甲戌本/第二回",
        "今只有嫡妻賈氏，生得一女，乳名黛玉，年方五歲。夫妻無子，故愛女如珍。",
    );

    let support = attribute_card_support(&frame, &card).expect("attribute support");

    assert_eq!(support.claim_value, "5岁");
    assert_eq!(support.modality, "bounded_event_attribute_inference");
    assert_eq!(support.evidence_strength, "inferred");
    assert!(support.matched_terms.iter().any(|term| term == "黛玉"));
    assert!(
        support
            .matched_terms
            .iter()
            .any(|term| term.contains("年方五歲"))
    );
}

#[test]
fn attribute_age_compare_answer_can_compare_ranges_from_cards() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "attribute_compare",
        "canonical_question": "林黛玉和贾宝玉相比，谁的年龄更大？",
        "subject": {"canonical": "林黛玉", "aliases": ["林黛玉", "黛玉", "林姑娘"]},
        "predicate": {
            "id": "age",
            "label": "年龄",
            "aliases": ["年龄", "几岁", "多大", "年龄大"],
            "evidence_terms": ["岁", "年纪", "年方", "长了"]
        },
        "object": {"canonical": "贾宝玉", "aliases": ["贾宝玉", "賈寶玉", "宝玉", "寶玉", "寳玉"]},
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("attribute frame");
    let cards = vec![
        card_with_source_text(
            "脂硯齋重評石頭記甲戌本/第二回",
            "今只有嫡妻賈氏，生得一女，乳名黛玉，年方五歲。",
        ),
        card_with_source_text(
            "脂硯齋重評石頭記甲戌本/第二回",
            "就取名呌作寳玉。那年週歲時，政老爹便要試他將來的志向。如今長了七八歲，雖然淘氣異常。",
        ),
    ];

    let answer = question_frame_answer(Some(&frame), &cards).expect("attribute answer");

    assert!(answer.contains("贾宝玉更大"));
    assert!(answer.contains("年方五歲"));
    assert!(answer.contains("七八歲"));
    assert!(answer.contains("只限于这些年龄线索"));
}

#[test]
fn entity_intro_answer_rejects_unfocused_evidence_list_template() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第001回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "從此空空道人因空見色，由色生情，改《石頭記》為《情僧錄》。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    let answer = question_frame_answer(Some(&frame), &cards).expect("entity answer");

    assert!(answer.contains("紫鹃"));
    assert!(answer.contains("没有命中"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
}

#[test]
fn entity_intro_answer_uses_focused_evidence_card() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "紫鹃在《红楼梦》里是什么样的人？",
        "subject": {"canonical": "紫鹃", "aliases": ["紫鹃", "紫鵑", "鹦哥", "鸚哥"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame");
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第003回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "賈母見雪雁甚小，便將自己身邊的一個二等丫頭，名喚鸚哥者與了黛玉。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    let answer = question_frame_answer(Some(&frame), &cards).expect("entity answer");

    assert!(answer.contains("紫鹃"));
    assert!(answer.contains("紅樓夢/第003回"));
    assert!(answer.contains("鸚哥"));
}

#[test]
fn relation_review_requires_direct_relation_support_for_yes_no_relation() {
    let frame = relation_frame();
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "source title".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "史湘云偶填柳絮词。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    assert_eq!(
        relation_review_issues(Some(&frame), &cards),
        vec!["relation_predicate_evidence_missing"]
    );
    let answer = relation_boundary_answer(Some(&frame), &cards).expect("boundary answer");
    assert!(answer.contains("没有直接证据"));
    assert!(answer.contains("不能确认"));
    assert!(answer.contains("紫鹃与史湘云之间存在“服侍”关系"));
    assert!(!answer.contains("紫鹃服侍过史湘云"));
}

#[test]
fn relation_direct_answer_uses_same_block_relation_support() {
    let frame = relation_frame();
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第三回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "紫鵑伏侍史湘雲，日夜不離。".to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    assert!(relation_review_issues(Some(&frame), &cards).is_empty());
    let answer = relation_direct_answer(Some(&frame), &cards).expect("direct answer");
    assert!(answer.contains("可以确认"));
    assert!(answer.contains("紫鹃服侍过史湘云"));
}

#[test]
fn relation_direct_answer_rejects_unlinked_block_cooccurrence() {
    let frame = relation_frame();
    let cards = vec![EvidenceCard {
        evidence_id: "ev-1".to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: "紅樓夢/第二十一回".to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: "block-1".to_string(),
        text: "黛玉起來叫醒湘雲。只見紫鵑、雪雁進來伏侍梳洗。湘雲洗了面，翠縷便拿殘水要潑。"
            .to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }];

    assert!(relation_direct_answer(Some(&frame), &cards).is_none());
    assert_eq!(
        relation_review_issues(Some(&frame), &cards),
        vec!["relation_predicate_evidence_missing"]
    );
}

#[test]
fn relation_open_object_answer_extracts_supported_objects_from_cards() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人先后服侍过哪些人？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");
    let cards = vec![card_with_text(
        "寶玉乳母李嬤嬤並大丫頭名喚襲人的陪侍在外面大床上。",
    )];

    let answer = relation_open_object_answer(Some(&frame), &cards).expect("open object answer");

    assert!(answer.contains("袭人"));
    assert!(answer.contains("服侍"));
    assert!(answer.contains("贾宝玉"));
    assert!(answer.contains("寶玉乳母"));
    assert!(answer.contains("襲人的陪侍"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
}

#[test]
fn relation_open_object_answer_quotes_each_relation_phrase() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人先后服侍过哪些人？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");
    let cards = vec![
        card_with_text("原來這襲人亦是賈母之婢，伏侍賈母時，心中眼中只有一個賈母。"),
        card_with_text("寶玉聽了，留下襲人伏侍寶玉不必來。"),
    ];

    let answer = relation_open_object_answer(Some(&frame), &cards).expect("open object answer");

    assert!(answer.contains("贾母"));
    assert!(answer.contains("贾宝玉"));
    assert!(answer.contains("伏侍賈母時"));
    assert!(answer.contains("伏侍寶玉不必來"));
    assert!(!answer.contains("原來這襲人亦是賈母之婢，本名珍珠"));
}

#[test]
fn relation_open_object_draft_rejects_missing_supported_object() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人先后服侍过哪些人？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");
    let cards = vec![
        card_with_text("原來這襲人亦是賈母之婢，遂與了寶玉。"),
        card_with_text(
            "襲人道：自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。",
        ),
    ];

    assert_eq!(
        relation_open_object_draft_rejection_reason(
            Some(&frame),
            &cards,
            "袭人先前本是贾母的婢女，后来又在宝玉房里贴身服侍宝玉。",
        ),
        Some("draft_missing_open_relation_object")
    );
    assert_eq!(
        relation_open_object_draft_rejection_reason(
            Some(&frame),
            &cards,
            "袭人先后服侍过贾母、史大姑娘和宝玉。",
        ),
        Some("draft_missing_open_relation_evidence_cue")
    );
    assert_eq!(
        relation_open_object_draft_rejection_reason(
            Some(&frame),
            &cards,
            "袭人先后服侍过史大姑娘，证据是“先伏侍了史大姑娘几年”。",
        ),
        None
    );
}

#[test]
fn relation_open_object_search_terms_use_subject_anchor_without_predicate_wide_scan() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人服侍过谁？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");

    let terms = relation_open_object_search_terms(Some(&frame));

    assert!(terms.contains(&"袭人".to_string()));
    assert!(terms.contains(&"襲人".to_string()));
    assert!(terms.contains(&"珍珠".to_string()));
    assert!(!terms.contains(&"服侍".to_string()));
    assert!(!terms.contains(&"伏侍".to_string()));
    assert!(!terms.contains(&"陪侍".to_string()));
    assert!(!terms.contains(&"丫鬟".to_string()));
    assert!(!terms.contains(&"丫头".to_string()));
    assert!(!terms.contains(&"贾母".to_string()));
    assert!(!terms.contains(&"史湘云".to_string()));
}

#[test]
fn relation_search_query_uses_subject_anchor_for_open_object_relation() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人服侍过谁？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");

    let query = relation_search_query("袭人主要服侍谁？", Some(&frame));

    assert!(query.contains("袭人"));
    assert!(query.contains("珍珠"));
    assert!(query.contains("袭人主要服侍谁？"));
    assert!(query.contains("袭人服侍过谁？"));
    assert!(!query.split_whitespace().any(|term| term == "服侍"));
    assert!(!query.split_whitespace().any(|term| term == "伏侍"));
    assert!(!query.split_whitespace().any(|term| term == "丫鬟"));
}

#[test]
fn relation_open_object_text_match_uses_external_ontology_aliases() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人服侍过谁？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");

    assert_eq!(
        relation_open_object_text_candidate_names(
            Some(&frame),
            "襲人道：「自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。」"
        ),
        vec!["史湘云".to_string()]
    );
    assert_eq!(
        relation_open_object_text_candidate_names(
            Some(&frame),
            "原來這襲人亦是賈母之婢，伏侍賈母時，心中眼中只有一個賈母。"
        ),
        vec!["贾母".to_string()]
    );
    assert_eq!(
        relation_open_object_text_candidate_names(
            Some(&frame),
            "當下王嬤嬤與鸚哥陪侍黛玉在碧紗櫥內；寶玉乳母李嬤嬤並大丫頭名喚襲人的陪侍在外面大床上。"
        ),
        vec!["贾宝玉".to_string()]
    );
    assert!(
        relation_open_object_text_candidate_names(
            Some(&frame),
            "襲人聽了，便忙到瀟湘館來，見紫鵑正伏侍黛玉吃藥，也顧不得什麼。"
        )
        .is_empty()
    );
    assert_eq!(
        relation_open_object_text_candidate_names(
            Some(&frame),
            "襲人冷笑道：“橫豎有人伏侍你，再別來支使我。我仍舊還伏侍老太太去。”"
        ),
        vec!["贾母".to_string()]
    );
    assert!(
        relation_open_object_text_candidate_names(
            Some(&frame),
            "襲人自幼在賈府當差，史湘雲也常來園中作客。"
        )
        .is_empty()
    );
}

#[test]
fn relation_open_object_answer_returns_controlled_boundary_when_objects_are_missing() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人先后服侍过哪些人？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": null,
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("open object frame");
    let cards = vec![card_with_text("襲人自幼在賈府當差。")];

    let answer = question_frame_answer(Some(&frame), &cards).expect("open object boundary");

    assert!(answer.contains("尚不能"));
    assert!(answer.contains("同时出现主体、关系和对象"));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
}

#[test]
fn relation_direct_support_accepts_speaker_self_relation() {
    let frame: RuntimeQuestionFrame = serde_json::from_value(json!({
        "intent": "relation_query",
        "canonical_question": "袭人服侍过史湘云吗？",
        "subject": {"canonical": "袭人", "aliases": ["袭人", "襲人", "珍珠"]},
        "predicate": {
            "id": "serve",
            "label": "服侍",
            "aliases": ["服侍", "伏侍", "侍候", "陪侍"],
            "evidence_terms": ["丫鬟", "丫头"]
        },
        "object": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "史大姑娘"]},
        "required_evidence_types": ["base_text", "commentary"]
    }))
    .expect("relation frame");
    let cards = vec![card_with_text(
        "襲人道：「自我從小兒來了，跟著老太太，先伏侍了史大姑娘幾年，如今又伏侍了你幾年。」",
    )];

    let answer = relation_direct_answer(Some(&frame), &cards).expect("direct answer");
    assert!(answer.contains("先伏侍了史大姑娘幾年"));
    assert!(!answer.contains("襲人道"));
    assert_eq!(
        relation_draft_rejection_reason(Some(&frame), &cards, "没有明确证据表明袭人服侍过史湘云。"),
        Some("question_frame_relation_answer_contradicts_evidence")
    );
    assert_eq!(
        relation_draft_rejection_reason(Some(&frame), &cards, "袭人没有服侍过史湘云。"),
        Some("question_frame_relation_answer_contradicts_evidence")
    );
}

#[test]
fn relation_draft_gate_accepts_explicit_predicate_preserving_answer() {
    let frame = relation_frame();
    let cards = vec![card_with_text("紫鵑伏侍史湘雲，日夜不離。")];

    assert_eq!(
        relation_draft_rejection_reason(
            Some(&frame),
            &cards,
            "可以确认，当前证据显示紫鹃服侍过史湘云。"
        ),
        None
    );
}

#[test]
fn relation_draft_gate_rejects_answer_that_drops_relation_predicate() {
    let frame = relation_frame();
    let cards = vec![card_with_text("紫鵑伏侍史湘雲，日夜不離。")];

    assert_eq!(
        relation_draft_rejection_reason(
            Some(&frame),
            &cards,
            "史湘云是《红楼梦》中常参与诗社活动的人物。"
        ),
        Some("question_frame_relation_answer_missing")
    );
}

#[test]
fn relation_draft_gate_rejects_boundary_answer_against_direct_support() {
    let frame = relation_frame();
    let cards = vec![card_with_text("紫鵑伏侍史湘雲，日夜不離。")];

    assert_eq!(
        relation_draft_rejection_reason(
            Some(&frame),
            &cards,
            "没有直接证据能确认紫鹃服侍过史湘云。"
        ),
        Some("question_frame_relation_answer_contradicts_evidence")
    );
}

#[test]
fn relation_draft_gate_requires_boundary_when_direct_support_is_missing() {
    let frame = relation_frame();
    let cards = vec![card_with_text(
        "黛玉起來叫醒湘雲。只見紫鵑、雪雁進來伏侍梳洗。",
    )];

    assert_eq!(
        relation_draft_rejection_reason(
            Some(&frame),
            &cards,
            "可以确认，当前证据显示紫鹃服侍过史湘云。"
        ),
        Some("question_frame_relation_boundary_missing")
    );
    assert_eq!(
        relation_draft_rejection_reason(
            Some(&frame),
            &cards,
            "就当前证据包看，没有直接证据能确认紫鹃服侍过史湘云。"
        ),
        None
    );
}
