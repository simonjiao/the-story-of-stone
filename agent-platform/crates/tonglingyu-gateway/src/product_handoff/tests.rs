use super::*;

#[test]
fn handoff_code_is_single_use_and_does_not_embed_identity() {
    let mut store = ProductHandoffStoreBackend::InMemory(InMemoryProductHandoffStore::default());
    let record = ProductHandoffRecord::new(
        "openwebui:user-1",
        "run-1",
        "writing-assistant",
        "article-1",
        60,
    );
    let code = store.issue(record.clone()).expect("issue");
    assert!(!code.contains("user-1"));
    assert_eq!(store.consume(&code).expect("consume"), record);
    assert_eq!(
        store.consume(&code),
        Err(ProductHandoffStoreError::UnknownOrConsumed)
    );
}

#[test]
fn expired_handoff_is_rejected_after_consumption() {
    let mut store = ProductHandoffStoreBackend::InMemory(InMemoryProductHandoffStore::default());
    let mut record =
        ProductHandoffRecord::new("user-1", "run-1", "writing-assistant", "article-1", 60);
    record.expires_at = OffsetDateTime::now_utc().unix_timestamp() - 1;
    let code = store.issue(record).expect("issue");
    assert_eq!(store.consume(&code), Err(ProductHandoffStoreError::Expired));
}
