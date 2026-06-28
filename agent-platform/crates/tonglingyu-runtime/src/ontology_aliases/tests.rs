use super::*;

#[test]
fn default_catalog_contains_core_aliases() {
    let catalog = ontology_alias_catalog().expect("alias catalog");
    let baoyu = catalog
        .people
        .iter()
        .find(|person| person.person_id == "person:baoyu")
        .expect("baoyu alias entry");
    assert!(baoyu.aliases.contains(&"寳玉".to_string()));
    let qinzhong = catalog
        .people
        .iter()
        .find(|person| person.person_id == "person:qinzhong")
        .expect("qinzhong alias entry");
    assert!(qinzhong.aliases.contains(&"秦鐘".to_string()));
}
