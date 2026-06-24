use super::*;

fn setup_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("open test db");
    conn.execute(
        "CREATE TABLE schema_migrations (migration_id TEXT PRIMARY KEY, applied_at TEXT NOT NULL)",
        [],
    )
    .expect("schema migration table");
    init_schema(&conn).expect("answer quality schema");
    conn
}

fn tags(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn records_question_frame_quality_observation_without_active_path_visibility() {
    let conn = setup_conn();
    let failure_tags = tags(&[
        "question_frame_mismatch:object",
        "followup_resolution_mismatch:subject",
    ]);

    let observation = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "conversation-eval:run-1:case-1",
            run_id: Some("run-1"),
            case_id: Some("case-1"),
            trace_id: Some("trace-1"),
            package_id: Some("pkg-1"),
            severity: "medium",
            failure_tags: &failure_tags,
            user_visible_issue: Some("追问绑定错了对象。"),
            runtime_issue: Some("question_frame.object expected 史湘云 actual 贾宝玉"),
            suggested_owner: Some("question_resolution"),
            suggested_action: Some("检查结构化问句理解规则。"),
        },
    )
    .expect("record quality observation");

    assert_eq!(
        observation["observation"]["action_route"],
        json!("context_rule_candidate")
    );
    assert_eq!(
        observation["observation"]["action_reason"],
        json!("structured_question_or_rule_candidate_gap")
    );
    assert_eq!(
        observation["observation"]["action_payload"]["active_path_visible"],
        json!(false)
    );
    assert_eq!(
        observation["observation"]["action_payload"]["candidate_payloads_created"],
        json!(false)
    );
    assert_eq!(observation["active_path_visible"], json!(false));

    let trace_items =
        load_answer_quality_observations_for_trace(&conn, "trace-1").expect("trace observations");
    assert_eq!(trace_items.len(), 1);
    assert_eq!(trace_items[0]["case_id"], json!("case-1"));
}

#[test]
fn routes_evidence_and_answer_renderer_failures_to_separate_channels() {
    let conn = setup_conn();
    let evidence_tags = tags(&["negative_case_may_have_unsupported_yes"]);
    let answer_tags = tags(&["internal_metadata_leak:trace_id"]);

    let evidence = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "conversation-eval:run-2:evidence-case",
            run_id: Some("run-2"),
            case_id: Some("evidence-case"),
            trace_id: Some("trace-evidence"),
            package_id: Some("pkg-evidence"),
            severity: "high",
            failure_tags: &evidence_tags,
            user_visible_issue: None,
            runtime_issue: None,
            suggested_owner: Some("retrieval_policy"),
            suggested_action: None,
        },
    )
    .expect("record evidence observation");
    let answer = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "conversation-eval:run-2:answer-case",
            run_id: Some("run-2"),
            case_id: Some("answer-case"),
            trace_id: Some("trace-answer"),
            package_id: Some("pkg-answer"),
            severity: "medium",
            failure_tags: &answer_tags,
            user_visible_issue: None,
            runtime_issue: None,
            suggested_owner: Some("answer_renderer"),
            suggested_action: None,
        },
    )
    .expect("record answer observation");

    assert_eq!(
        evidence["observation"]["action_route"],
        json!("evidence_card_or_retrieval")
    );
    assert_eq!(
        answer["observation"]["action_route"],
        json!("answer_rule_or_renderer")
    );

    let listed = list_answer_quality_observations(
        &conn,
        AnswerQualityObservationListInput {
            status: Some("observed"),
            action_route: Some("evidence_card_or_retrieval"),
            severity: Some("high"),
            trace_id: None,
            limit: 50,
            offset: 0,
        },
    )
    .expect("list observations");
    assert_eq!(listed["items"].as_array().expect("items").len(), 1);
    assert_eq!(listed["items"][0]["case_id"], json!("evidence-case"));
}

#[test]
fn duplicate_source_entity_updates_existing_observation() {
    let conn = setup_conn();
    let first_tags = tags(&["runtime_check_mismatch:admin_trace_readable"]);
    let second_tags = tags(&["runtime_check_mismatch:reviewer_executed"]);

    let first = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "conversation-eval:run-3:case-1",
            run_id: Some("run-3"),
            case_id: Some("case-1"),
            trace_id: Some("trace-first"),
            package_id: None,
            severity: "medium",
            failure_tags: &first_tags,
            user_visible_issue: Some("first"),
            runtime_issue: Some("first"),
            suggested_owner: Some("runtime_trace_contract"),
            suggested_action: None,
        },
    )
    .expect("record first");
    let second = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "conversation-eval:run-3:case-1",
            run_id: Some("run-3"),
            case_id: Some("case-1"),
            trace_id: Some("trace-second"),
            package_id: Some("pkg-second"),
            severity: "high",
            failure_tags: &second_tags,
            user_visible_issue: Some("second"),
            runtime_issue: Some("second"),
            suggested_owner: Some("runtime_trace_contract"),
            suggested_action: None,
        },
    )
    .expect("record second");

    assert_eq!(
        first["observation"]["observation_id"],
        second["observation"]["observation_id"]
    );
    assert_eq!(second["observation"]["severity"], json!("high"));
    assert_eq!(second["observation"]["trace_id"], json!("trace-second"));

    let listed = list_answer_quality_observations(
        &conn,
        AnswerQualityObservationListInput {
            status: None,
            action_route: None,
            severity: None,
            trace_id: None,
            limit: 50,
            offset: 0,
        },
    )
    .expect("list observations");
    assert_eq!(listed["items"].as_array().expect("items").len(), 1);
}

#[test]
fn observation_creates_idempotent_action_item_and_trace_surface() {
    let conn = setup_conn();
    let failure_tags = tags(&["retrieval_gap:evidence_card_missing"]);

    let observation = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "manual_observation",
            source_entity_id: "quality:run-4:case-1",
            run_id: Some("run-4"),
            case_id: Some("case-1"),
            trace_id: Some("trace-action"),
            package_id: Some("pkg-action"),
            severity: "high",
            failure_tags: &failure_tags,
            user_visible_issue: Some("本地证据覆盖不足。"),
            runtime_issue: Some("retrieval did not return required evidence card"),
            suggested_owner: Some("retrieval_policy"),
            suggested_action: Some("检查 staged evidence card 或 retrieval rule。"),
        },
    )
    .expect("record observation");

    let action_items = observation["observation"]["action_items"]
        .as_array()
        .expect("action items");
    assert_eq!(action_items.len(), 1);
    assert_eq!(
        action_items[0]["action_type"],
        json!("review_evidence_card_or_retrieval_gap")
    );
    assert_eq!(action_items[0]["status"], json!("queued"));
    assert_eq!(action_items[0]["priority"], json!("p1"));
    assert_eq!(action_items[0]["active_path_visible"], json!(false));

    let listed = list_answer_quality_action_items(
        &conn,
        AnswerQualityActionItemListInput {
            status: Some("queued"),
            action_type: Some("review_evidence_card_or_retrieval_gap"),
            action_route: Some("evidence_card_or_retrieval"),
            trace_id: Some("trace-action"),
            limit: 50,
            offset: 0,
        },
    )
    .expect("list action items");
    assert_eq!(listed["items"].as_array().expect("items").len(), 1);

    let trace_actions = load_answer_quality_action_items_for_trace(&conn, "trace-action")
        .expect("trace action items");
    assert_eq!(trace_actions.len(), 1);
    assert_eq!(trace_actions[0]["package_id"], json!("pkg-action"));
}

#[test]
fn action_item_state_machine_requires_valid_link_and_reason() {
    let conn = setup_conn();
    let failure_tags = tags(&["question_frame_mismatch:object"]);
    let observation = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "quality:run-5:case-1",
            run_id: Some("run-5"),
            case_id: Some("case-1"),
            trace_id: Some("trace-transition"),
            package_id: None,
            severity: "medium",
            failure_tags: &failure_tags,
            user_visible_issue: None,
            runtime_issue: None,
            suggested_owner: Some("question_resolution"),
            suggested_action: None,
        },
    )
    .expect("record observation");
    let action_item_id = observation["observation"]["action_items"][0]["action_item_id"]
        .as_str()
        .expect("action item id")
        .to_string();

    let reviewed = transition_answer_quality_action_item(
        &conn,
        AnswerQualityActionTransitionInput {
            action_item_id: &action_item_id,
            actor: "reviewer",
            action: "start_review",
            reason: "triage started",
            linked_entity_type: None,
            linked_entity_id: None,
        },
    )
    .expect("start review");
    assert_eq!(reviewed["action_item"]["status"], json!("in_review"));

    let invalid_link = transition_answer_quality_action_item(
        &conn,
        AnswerQualityActionTransitionInput {
            action_item_id: &action_item_id,
            actor: "reviewer",
            action: "link",
            reason: "try unsupported link",
            linked_entity_type: Some("freeform_entity"),
            linked_entity_id: Some("x-1"),
        },
    );
    assert!(invalid_link.is_err());

    let linked = transition_answer_quality_action_item(
        &conn,
        AnswerQualityActionTransitionInput {
            action_item_id: &action_item_id,
            actor: "reviewer",
            action: "link",
            reason: "linked to staged rule candidate",
            linked_entity_type: Some("rule_candidate"),
            linked_entity_id: Some("rule-candidate-1"),
        },
    )
    .expect("link action");
    assert_eq!(linked["action_item"]["status"], json!("linked"));
    assert_eq!(
        linked["action_item"]["linked_entity_type"],
        json!("rule_candidate")
    );
    assert_eq!(
        linked["action_item"]["events"]
            .as_array()
            .expect("events")
            .last()
            .expect("last event")["event_type"],
        json!("link")
    );
}

#[test]
fn closed_or_rejected_action_requeues_when_observed_again() {
    let conn = setup_conn();
    let failure_tags = tags(&["visible_internal_error:answer_renderer"]);
    let observation = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "quality:run-6:case-1",
            run_id: Some("run-6"),
            case_id: Some("case-1"),
            trace_id: Some("trace-requeue"),
            package_id: None,
            severity: "low",
            failure_tags: &failure_tags,
            user_visible_issue: None,
            runtime_issue: None,
            suggested_owner: Some("answer_renderer"),
            suggested_action: None,
        },
    )
    .expect("record observation");
    let action_item_id = observation["observation"]["action_items"][0]["action_item_id"]
        .as_str()
        .expect("action item id")
        .to_string();

    transition_answer_quality_action_item(
        &conn,
        AnswerQualityActionTransitionInput {
            action_item_id: &action_item_id,
            actor: "reviewer",
            action: "close",
            reason: "fixed by renderer rule",
            linked_entity_type: None,
            linked_entity_id: None,
        },
    )
    .expect("close action");

    let observed_again = record_answer_quality_observation(
        &conn,
        AnswerQualityObservationInput {
            actor: "eval-loop",
            source_entity_type: "eval_miss",
            source_entity_id: "quality:run-6:case-1",
            run_id: Some("run-6"),
            case_id: Some("case-1"),
            trace_id: Some("trace-requeue-again"),
            package_id: Some("pkg-requeue"),
            severity: "medium",
            failure_tags: &failure_tags,
            user_visible_issue: Some("same issue observed again"),
            runtime_issue: None,
            suggested_owner: Some("answer_renderer"),
            suggested_action: None,
        },
    )
    .expect("record again");

    let action = &observed_again["observation"]["action_items"][0];
    assert_eq!(action["action_item_id"], json!(action_item_id));
    assert_eq!(action["status"], json!("queued"));
    assert_eq!(action["priority"], json!("p2"));
    assert!(action["events"].is_null());

    let read = read_answer_quality_action_item(&conn, &action_item_id)
        .expect("read action")
        .expect("action exists");
    let events = read["action_item"]["events"].as_array().expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == json!("requeued_by_observation")
            && event["from_status"] == json!("closed")
            && event["to_status"] == json!("queued")
    }));
}
