use super::*;

#[test]
fn default_catalog_classifies_source_and_labels_layers() {
    assert_eq!(
        classify_evidence_type("commentary_material", "zhiyanzhai", "批语").expect("classify"),
        "commentary"
    );
    assert_eq!(
        classify_evidence_type("base_text", "chengjia", "程甲本说明").expect("classify"),
        "version_note"
    );
    assert_eq!(
        source_layer_label("base_text_later_40").expect("label"),
        "后四十回正文"
    );
}

#[test]
fn default_catalog_drives_hygiene_and_ranking_terms() {
    assert!(evidence_text_is_broken_shell("宝玉道：", 3).expect("hygiene"));
    assert!(generic_question_term("介绍").expect("generic term"));
    let ranking = ranking_rules().expect("ranking rules");
    assert!(
        ranking
            .inscription_text_terms
            .contains(&"莫失莫忘".to_string())
    );
}
