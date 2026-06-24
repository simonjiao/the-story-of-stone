use super::*;
use crate::question_frame::RuntimeQuestionFrame;
use serde_json::json;

fn entity_frame() -> RuntimeQuestionFrame {
    serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "史湘云是谁？",
        "subject": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "湘云", "湘雲"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity frame")
}

fn entity_fate_frame() -> RuntimeQuestionFrame {
    serde_json::from_value(json!({
        "intent": "entity_query",
        "canonical_question": "史湘云的结局，后四十回呢？",
        "subject": {"canonical": "史湘云", "aliases": ["史湘云", "史湘雲", "湘云", "湘雲", "史姑娘"]},
        "predicate": null,
        "object": null,
        "required_evidence_types": []
    }))
    .expect("entity fate frame")
}

fn card(evidence_id: &str, source_title: &str, text: &str) -> EvidenceCard {
    EvidenceCard {
        evidence_id: evidence_id.to_string(),
        evidence_type: "base_text".to_string(),
        source_id: "source".to_string(),
        source_title: source_title.to_string(),
        source_url: String::new(),
        revision_id: None,
        block_id: format!("block-{evidence_id}"),
        text: text.to_string(),
        support_scope: String::new(),
        unsupported_scope: String::new(),
        evidence_level: String::new(),
        confidence: String::new(),
        verification_status: String::new(),
    }
}

#[test]
fn entity_intro_composes_from_readable_cards_and_skips_short_speech_shell() {
    let frame = entity_frame();
    let cards = vec![
        card("ev-1", "紅樓夢/第050回", "湘雲道："),
        card(
            "ev-2",
            "紅樓夢/第020回",
            "只见湘雲大笑大说的进来，众人都说史大姑娘来了。",
        ),
        card(
            "ev-3",
            "紅樓夢/第031回",
            "寶玉笑道：云妹妹来了。史湘雲便和众姊妹说笑。",
        ),
    ];

    let answer = compose_entity_intro_answer(Some(&frame), &cards).expect("entity intro");

    assert!(answer.contains("史湘云"));
    assert!(answer.contains("紅樓夢/第020回"));
    assert!(answer.contains("史大姑娘来了"));
    assert!(answer.contains("紅樓夢/第031回"));
    assert!(answer.contains("云妹妹"));
    assert!(!answer.contains("湘雲道："));
    assert!(!answer.contains("目前能支持回答的主要材料如下"));
}

#[test]
fn entity_intro_skips_public_quote_terms_excluded_by_answer_rules() {
    let frame = entity_frame();
    let cards = vec![
        card(
            "ev-1",
            "紅樓夢/第026回",
            "史湘雲正自啼哭，忽聽“吱嘍”一聲，院門開處，不知是那一個出來。要知端的，且聽下回分解。",
        ),
        card(
            "ev-2",
            "紅樓夢/第031回",
            "寶玉笑道：云妹妹来了。史湘雲便和众姊妹说笑。",
        ),
    ];

    let answer = compose_entity_intro_answer(Some(&frame), &cards).expect("entity intro");

    assert!(!answer.contains("要知端的"));
    assert!(!answer.contains("下回分解"));
    assert!(answer.contains("紅樓夢/第031回"));
    assert!(answer.contains("云妹妹"));
}

#[test]
fn entity_intro_marks_only_short_shell_coverage_as_insufficient() {
    let frame = entity_frame();
    let cards = vec![card("ev-1", "紅樓夢/第050回", "湘雲道：")];

    let answer = compose_entity_intro_answer(Some(&frame), &cards).expect("entity intro");

    assert!(answer.contains("过短片段"));
    assert!(answer.contains("不足以可靠概括"));
    assert!(answer.contains("紅樓夢/第050回"));
}

#[test]
fn entity_intro_returns_boundary_when_no_entity_card_matches() {
    let frame = entity_frame();
    let cards = vec![card(
        "ev-1",
        "紅樓夢/第001回",
        "從此空空道人因空見色，由色生情，改《石頭記》為《情僧錄》。",
    )];

    let answer = compose_entity_intro_answer(Some(&frame), &cards).expect("entity intro");

    assert!(answer.contains("没有命中关于史湘云的直接材料"));
}

#[test]
fn entity_fate_answer_prefers_fate_evidence_over_mention_only_cards() {
    let frame = entity_fate_frame();
    let cards = vec![
        card(
            "ev-1",
            "紅樓夢（程甲本）/九十四",
            "史姑娘撿著金麒麟，外頭造出謠言來。",
        ),
        card(
            "ev-2",
            "紅樓夢（程甲本）/一百一十八",
            "就是史姑娘是他叔叔的主意，頭裡原好，如今姑爺癆病死了，你史妹妹立志守寡，也就苦了。",
        ),
        card(
            "ev-3",
            "紅樓夢（程乙本）/第一百零一回 至第一百一十回",
            "也不彀，赶車的也少，要到親戚家去借去呢。且說史湘雲因他女婿病著，賈母死後，只來了一次，又見他女婿的病已成癆症，想到自己命苦，剛配了一個才貌雙全的女婿。",
        ),
    ];

    let answer = compose_entity_intro_answer(Some(&frame), &cards).expect("entity fate");

    assert!(answer.contains("按后四十回范围看"));
    assert!(answer.contains("紅樓夢（程甲本）/一百一十八"));
    assert!(answer.contains("姑爺癆病死了"));
    assert!(answer.contains("立志守寡"));
    assert!(!answer.contains("赶車的也少"));
    assert!(!answer.contains("没有命中关于史湘云"));
}
