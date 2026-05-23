use super::*;

fn count_basis() -> EvidenceSlotCountBasis {
    EvidenceSlotCountBasis {
        id: "direct_loss".to_string(),
        label: "直接丢失/被盗".to_string(),
        question_terms: vec!["丢".to_string()],
        answer_unit: "处".to_string(),
        answer_noun: "直接丢失证据".to_string(),
    }
}

fn slot_match(
    slot_id: &str,
    label: &str,
    role: &str,
    counts_as: &[&str],
    source_layer: &str,
    text: &str,
) -> EvidenceSlotMatch {
    EvidenceSlotMatch {
        slot_id: slot_id.to_string(),
        label: label.to_string(),
        role: role.to_string(),
        counts_as: counts_as.iter().map(|value| (*value).to_string()).collect(),
        display_group: "related_clue".to_string(),
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
                "direct_loss_or_theft",
                &["direct_loss"],
                "base_text_pre_80",
                "那一年有一个良儿偷玉。",
            ),
            slot_match(
                "zhen_baoyu_delivers_jade",
                "甄宝玉送玉",
                "suspected_transfer_related_to_loss",
                &["related_loss_clue"],
                "commentary",
                "伏甄宝玉送玉。",
            ),
        ],
    )
    .expect("composed answer");

    assert!(answer.contains("直接支持一处"));
    assert!(answer.contains("良儿偷玉"));
    assert!(answer.contains("不能直接计为"));
    assert!(answer.contains("甄宝玉送玉"));
}
