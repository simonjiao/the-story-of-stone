use super::*;

fn binding(response_id: &str, chat_id: &str) -> ProductRunBinding {
    ProductRunBinding::new(
        response_id,
        format!("run-{response_id}"),
        "writing-assistant",
        chat_id,
        format!("message-{response_id}"),
    )
}

#[test]
fn idempotently_binds_a_response_and_rejects_two_active_runs_per_chat() {
    let mut store = ProductBindingStoreBackend::InMemory(InMemoryProductBindingStore::default());
    let first = store
        .create(binding("response-1", "chat-1"))
        .expect("create");
    assert_eq!(
        store
            .create(binding("response-1", "chat-1"))
            .expect("idempotent"),
        first
    );
    assert!(matches!(
        store.create(binding("response-2", "chat-1")),
        Err(ProductBindingStoreError::ActiveChatConflict(_))
    ));
}

#[test]
fn persists_monotonic_remote_progress_and_releases_terminal_chats() {
    let mut store = ProductBindingStoreBackend::InMemory(InMemoryProductBindingStore::default());
    let mut first = store
        .create(binding("response-1", "chat-1"))
        .expect("create");
    first.remote_run_id = Some("studio-run-1".to_string());
    first.remote_last_sequence = 4;
    first.status = ProductBindingStatus::Running;
    let saved = store.save(first, 1).expect("save");
    let mut regressed = saved.clone();
    regressed.remote_last_sequence = 3;
    assert!(matches!(
        store.save(regressed, saved.version),
        Err(ProductBindingStoreError::SequenceRegression { .. })
    ));
    let mut completed = saved.clone();
    completed.status = ProductBindingStatus::Completed;
    store.save(completed, saved.version).expect("complete");
    assert!(store.create(binding("response-2", "chat-1")).is_ok());
}
