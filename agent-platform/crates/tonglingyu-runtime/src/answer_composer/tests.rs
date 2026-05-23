use super::*;

fn count_basis() -> EvidenceSlotCountBasis {
    EvidenceSlotCountBasis {
        id: "direct_loss".to_string(),
        label: "明确失玉/被盗".to_string(),
        question_terms: vec!["丢".to_string()],
        answer_unit: "处".to_string(),
        answer_noun: "明确失玉证据".to_string(),
    }
}

fn slot_match(
    slot_id: &str,
    label: &str,
    public_role_label: &str,
    counts_as: &[&str],
    count_note: Option<&str>,
    source_layer: &str,
    text: &str,
) -> EvidenceSlotMatch {
    EvidenceSlotMatch {
        slot_id: slot_id.to_string(),
        label: label.to_string(),
        public_role_label: public_role_label.to_string(),
        counts_as: counts_as.iter().map(|value| (*value).to_string()).collect(),
        display_group: "related_clue".to_string(),
        count_note: count_note.map(str::to_string),
        matched_terms: vec![label.to_string()],
        source_title: "测试来源".to_string(),
        source_layer: source_layer.to_string(),
        text: text.to_string(),
    }
}

#[test]
fn composer_counts_direct_slots_and_separates_related_clues() {
    let package = EvidencePackage {
        package_id: "pkg-test".to_string(),
        trace_id: "trace-test".to_string(),
        question: "通灵宝玉丢了几次".to_string(),
        cards: Vec::new(),
        claims: Vec::new(),
        claim_evidence_map: Vec::new(),
        knowledge_state_summary: Default::default(),
        review: crate::ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "passed".to_string(),
        },
    };
    let answer = compose_slot_count_answer(
        &package,
        &count_basis(),
        &[
            slot_match(
                "lianger_stole_jade",
                "良儿偷玉",
                "直接丢失或被盗",
                &["direct_loss"],
                None,
                "base_text_pre_80",
                "那一年有一个良儿偷玉。",
            ),
            slot_match(
                "fengjie_snow_pickup_jade",
                "凤姐扫雪拾玉",
                "拾玉/失而复见线索",
                &["direct_loss", "recovery_clue"],
                Some("按“拾玉/失而复得”计入明确失玉；它能证明曾经失玉，但不补出丢失经过"),
                "commentary",
                "凤姐扫雪拾玉。",
            ),
            slot_match(
                "zhen_baoyu_delivers_jade",
                "甄宝玉送玉",
                "送玉/流转疑似线索",
                &["related_loss_clue"],
                None,
                "commentary",
                "伏甄宝玉送玉。",
            ),
        ],
    )
    .expect("composed answer");

    assert!(answer.contains("严格按“明确失玉/被盗”口径"));
    assert!(answer.contains("直接支持两处"));
    assert!(answer.contains("良儿偷玉"));
    assert!(answer.contains("计数说明"));
    assert!(answer.contains("按“拾玉/失而复得”计入明确失玉"));
    assert!(answer.contains("凤姐扫雪拾玉"));
    assert!(!answer.contains("广义失玉线索"));
    assert!(answer.contains("不能直接计入次数"));
    assert!(answer.contains("甄宝玉送玉"));
}

#[test]
fn composer_strips_internal_markup_from_public_quotes() {
    let package = EvidencePackage {
        package_id: "pkg-test".to_string(),
        trace_id: "trace-test".to_string(),
        question: "通灵宝玉丢了几次".to_string(),
        cards: Vec::new(),
        claims: Vec::new(),
        claim_evidence_map: Vec::new(),
        knowledge_state_summary: Default::default(),
        review: crate::ReviewRecord {
            status: "passed".to_string(),
            severity: "none".to_string(),
            issues: Vec::new(),
            summary: "passed".to_string(),
        },
    };
    let answer = compose_slot_count_answer(
        &package,
        &count_basis(),
        &[slot_match(
            "fengjie_snow_pickup_jade",
            "凤姐扫雪拾玉",
            "拾玉/失而复见线索",
            &["direct_loss", "recovery_clue"],
            Some("按“拾玉/失而复得”计入明确失玉；它能证明曾经失玉，但不补出丢失经过"),
            "commentary",
            "剛至穿堂門前，{{~|【庚辰雙行夾批：妙！這便是凤姐扫雪拾玉之處，一絲不亂。】}}<br>只見襲人倚門立在那裡。",
        )],
    )
    .expect("composed answer");

    assert!(answer.contains("凤姐扫雪拾玉"));
    assert!(!answer.contains("{{~|"));
    assert!(!answer.contains("}}"));
    assert!(!answer.contains("<br>"));
}
