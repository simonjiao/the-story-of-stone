use super::*;

#[test]
fn default_catalog_marks_zhen_baoyu_as_related_not_direct_loss() {
    let rules = evidence_slot_rules_for_ids(&[
        "lianger_stole_jade".to_string(),
        "zhen_baoyu_delivers_jade".to_string(),
        "fengjie_snow_pickup_jade".to_string(),
    ])
    .expect("slot rules");

    assert!(rules[0].counts_as.contains(&"direct_loss".to_string()));
    assert_eq!(rules[0].public_role_label, "直接丢失或被盗");
    assert_eq!(rules[1].role, "suspected_transfer_related_to_loss");
    assert_eq!(rules[1].public_role_label, "送玉/流转疑似线索");
    assert!(!rules[1].counts_as.contains(&"direct_loss".to_string()));
    assert_eq!(rules[2].role, "recovery_or_lost_and_found_clue");
    assert_eq!(rules[2].public_role_label, "拾玉/失而复见线索");
    assert!(rules[2].counts_as.contains(&"direct_loss".to_string()));
    assert!(rules[2].counts_as.contains(&"recovery_clue".to_string()));
    assert!(
        rules[2]
            .count_note
            .as_deref()
            .is_some_and(|note| note.contains("失而复得"))
    );
}

#[test]
fn count_basis_activates_for_loss_count_question() {
    let basis = active_count_basis_for_question("通灵宝玉丢了几次", true)
        .expect("count basis")
        .expect("active count basis");

    assert_eq!(basis.id, "direct_loss");
}

#[test]
fn catalog_cache_hot_reloads_external_file() {
    let catalog_path = std::env::temp_dir().join(format!(
        "tonglingyu-evidence-slot-rules-{}.json",
        uuid::Uuid::now_v7().simple()
    ));
    let initial_catalog = r#"{
        "schema_version": "tonglingyu.evidence_slot_rules.v1",
        "catalog_version": "test.1",
        "count_bases": [
            {
                "id": "direct_loss",
                "label": "直接丢失",
                "question_terms": ["丢"],
                "count_question_terms": ["几次"],
                "total_count_units": ["次"],
                "total_count_prefixes": ["共"],
                "direct_slot_negation_terms": ["不计入"],
                "answer_unit": "处",
                "answer_noun": "直接丢失证据"
            }
        ],
        "slots": [
            {
                "id": "slot:test",
                "label": "初始槽位",
                "role": "initial_role",
                "public_role_label": "初始角色",
                "counts_as": ["direct_loss"],
                "display_group": "direct_evidence"
            }
        ]
    }"#;
    let updated_catalog = r#"{
        "schema_version": "tonglingyu.evidence_slot_rules.v1",
        "catalog_version": "test.2",
        "count_bases": [
            {
                "id": "direct_loss",
                "label": "直接丢失",
                "question_terms": ["丢"],
                "count_question_terms": ["几次"],
                "total_count_units": ["次"],
                "total_count_prefixes": ["共"],
                "direct_slot_negation_terms": ["不计入"],
                "answer_unit": "处",
                "answer_noun": "直接丢失证据"
            }
        ],
        "slots": [
            {
                "id": "slot:test",
                "label": "更新槽位",
                "role": "updated_role",
                "public_role_label": "更新角色",
                "counts_as": [],
                "display_group": "related_clue",
                "count_note": "更新计数说明"
            }
        ]
    }"#;

    std::fs::write(&catalog_path, initial_catalog).expect("write initial catalog");
    let mut cache = EvidenceSlotRuleCatalogCache::default();
    let catalog = cache
        .catalog(Some(catalog_path.clone()))
        .expect("load initial catalog");
    assert_eq!(catalog.slots[0].label, "初始槽位");

    std::fs::write(&catalog_path, updated_catalog).expect("write updated catalog");
    cache.modified = Some(std::time::SystemTime::UNIX_EPOCH);
    let catalog = cache
        .catalog(Some(catalog_path.clone()))
        .expect("reload updated catalog");
    assert_eq!(catalog.slots[0].label, "更新槽位");
    assert_eq!(catalog.slots[0].public_role_label, "更新角色");
    assert_eq!(catalog.slots[0].display_group, "related_clue");
    assert_eq!(catalog.slots[0].count_note.as_deref(), Some("更新计数说明"));

    std::fs::remove_file(catalog_path).expect("remove catalog");
}

#[test]
fn count_parser_uses_catalog_terms_and_markers() {
    assert!(question_asks_for_count("通灵宝玉丢了几次").expect("count question terms"));
    let basis = active_count_basis_for_question("通灵宝玉丢了几次", true)
        .expect("count basis")
        .expect("active count basis");
    assert_eq!(
        explicit_total_count_for_basis("明确两处。", &basis),
        Some(2)
    );
    assert_eq!(
        explicit_total_count_for_basis("明确三处。", &basis),
        Some(3)
    );
    assert_eq!(explicit_total_count_for_basis("可计1次。", &basis), Some(1));
    assert!(
        basis
            .direct_slot_negation_terms
            .contains(&"不计入".to_string())
    );
}
