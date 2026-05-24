use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tonglingyu_runtime::{
    RQA_LIFECYCLE_POLICY_VERSION, append_rqa_lifecycle_tombstone, append_runtime_audit_event,
};

use crate::{
    RqaUserLifecycleAction, RqaUserLifecycleArgs, hash_text, now_rfc3339, open_db,
    run_immediate_transaction,
};

#[derive(Debug, Clone)]
struct LifecycleSessionRef {
    session_id: String,
    user_ref: String,
    chat_ref: String,
}

#[derive(Debug, Clone)]
struct LifecycleMessageRef {
    message_id: String,
    external_message_id: String,
    trace_id: String,
    package_id: Option<String>,
    context_pack_id: Option<String>,
    question: String,
    response_json: String,
}

#[derive(Debug, Clone)]
struct RqaUserLifecyclePlan {
    subject_sha256: String,
    sessions: Vec<LifecycleSessionRef>,
    messages: Vec<LifecycleMessageRef>,
    trace_ids: BTreeSet<String>,
    package_ids: BTreeSet<String>,
    context_pack_ids: BTreeSet<String>,
    memory_candidate_ids: BTreeSet<String>,
    memory_card_ids: BTreeSet<String>,
    memory_policy_decision_ids: BTreeSet<String>,
    memory_transition_audit_ids: BTreeSet<String>,
    workflow_state_ids: BTreeSet<String>,
    audit_event_ids: BTreeSet<String>,
    retrieval_failure_ids: BTreeSet<String>,
    governance_task_ids: BTreeSet<String>,
    active_legal_holds: i64,
}

pub(crate) fn rqa_user_lifecycle_command(args: &RqaUserLifecycleArgs) -> Result<Value> {
    let conn = open_db(&args.db)?;
    let user_ref = args.user_ref.trim();
    if user_ref.is_empty() {
        return Err(anyhow!("--user-ref must not be empty"));
    }
    let reason = args.reason.trim();
    if reason.is_empty() {
        return Err(anyhow!("--reason must not be empty"));
    }
    let plan = build_rqa_user_lifecycle_plan(&conn, user_ref)?;
    match args.action {
        RqaUserLifecycleAction::Export => rqa_user_lifecycle_export(&conn, &plan, reason),
        RqaUserLifecycleAction::LegalHold => rqa_user_lifecycle_legal_hold(&conn, &plan, reason),
        RqaUserLifecycleAction::ReleaseLegalHold => {
            rqa_user_lifecycle_release_legal_hold(&conn, &plan, reason)
        }
        RqaUserLifecycleAction::Anonymize => {
            rqa_user_lifecycle_anonymize(&conn, user_ref, &plan, reason)
        }
    }
}

fn build_rqa_user_lifecycle_plan(
    conn: &Connection,
    user_ref: &str,
) -> Result<RqaUserLifecyclePlan> {
    let subject_sha256 = hash_text(user_ref);
    let sessions = query_lifecycle_sessions(conn, user_ref)?;
    let messages = query_lifecycle_messages(conn, user_ref)?;
    let trace_ids = messages
        .iter()
        .map(|message| message.trace_id.clone())
        .collect::<BTreeSet<_>>();
    let package_ids = messages
        .iter()
        .filter_map(|message| message.package_id.clone())
        .collect::<BTreeSet<_>>();
    let context_pack_ids = messages
        .iter()
        .filter_map(|message| message.context_pack_id.clone())
        .collect::<BTreeSet<_>>();
    let session_ids = sessions
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<BTreeSet<_>>();
    let workflow_state_ids = query_lifecycle_workflow_state_ids(conn, &trace_ids, &session_ids)?;
    let audit_event_ids = query_lifecycle_audit_event_ids(conn, &trace_ids)?;
    let memory_candidate_ids =
        query_lifecycle_memory_candidate_ids(conn, &session_ids, &trace_ids)?;
    let memory_card_ids = query_lifecycle_memory_card_ids(conn, &memory_candidate_ids)?;
    let memory_policy_decision_ids =
        query_lifecycle_memory_policy_decision_ids(conn, &memory_candidate_ids, &memory_card_ids)?;
    let memory_transition_audit_ids = query_lifecycle_memory_transition_audit_ids(
        conn,
        &memory_candidate_ids,
        &memory_card_ids,
        &memory_policy_decision_ids,
    )?;
    let retrieval_failure_ids =
        query_lifecycle_retrieval_failure_ids(conn, &trace_ids, &package_ids)?;
    let governance_task_ids = query_lifecycle_governance_task_ids(conn, &trace_ids, &package_ids)?;
    let active_legal_holds = conn.query_row(
        "SELECT COUNT(*) FROM rqa_user_legal_holds WHERE user_ref_sha256 = ?1 AND active = 1",
        params![&subject_sha256],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(RqaUserLifecyclePlan {
        subject_sha256,
        sessions,
        messages,
        trace_ids,
        package_ids,
        context_pack_ids,
        memory_candidate_ids,
        memory_card_ids,
        memory_policy_decision_ids,
        memory_transition_audit_ids,
        workflow_state_ids,
        audit_event_ids,
        retrieval_failure_ids,
        governance_task_ids,
        active_legal_holds,
    })
}

fn query_lifecycle_sessions(conn: &Connection, user_ref: &str) -> Result<Vec<LifecycleSessionRef>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT user_session_id, external_user_ref, external_session_id
        FROM user_sessions
        WHERE external_user_ref = ?1
        ORDER BY user_session_id
        "#,
    )?;
    stmt.query_map(params![user_ref], |row| {
        Ok(LifecycleSessionRef {
            session_id: row.get(0)?,
            user_ref: row.get(1)?,
            chat_ref: row.get(2)?,
        })
    })?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(Into::into)
}

fn query_lifecycle_messages(conn: &Connection, user_ref: &str) -> Result<Vec<LifecycleMessageRef>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT sj.journal_id, COALESCE(sj.external_message_id, ''), sj.trace_id,
               sj.package_id, sj.context_pack_id, COALESCE(sj.content, sj.summary),
               sj.metadata_json
        FROM session_journal AS sj
        JOIN user_sessions AS us ON us.user_session_id = sj.user_session_id
        WHERE us.external_user_ref = ?1
        ORDER BY sj.created_at, sj.journal_id
        "#,
    )?;
    stmt.query_map(params![user_ref], |row| {
        Ok(LifecycleMessageRef {
            message_id: row.get(0)?,
            external_message_id: row.get(1)?,
            trace_id: row.get(2)?,
            package_id: row.get(3)?,
            context_pack_id: row.get(4)?,
            question: row.get(5)?,
            response_json: row.get(6)?,
        })
    })?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(Into::into)
}

fn query_lifecycle_workflow_state_ids(
    conn: &Connection,
    trace_ids: &BTreeSet<String>,
    session_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut by_trace = conn.prepare("SELECT state_id FROM workflow_states WHERE trace_id = ?1")?;
    for trace_id in trace_ids {
        for id in by_trace.query_map(params![trace_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    let mut by_session =
        conn.prepare("SELECT state_id FROM workflow_states WHERE session_id = ?1")?;
    for session_id in session_ids {
        for id in by_session.query_map(params![session_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn query_lifecycle_audit_event_ids(
    conn: &Connection,
    trace_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut stmt = conn.prepare("SELECT event_id FROM audit_events WHERE trace_id = ?1")?;
    for trace_id in trace_ids {
        for id in stmt.query_map(params![trace_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn query_lifecycle_memory_candidate_ids(
    conn: &Connection,
    session_ids: &BTreeSet<String>,
    trace_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut by_session =
        conn.prepare("SELECT candidate_id FROM memory_candidates WHERE user_session_id = ?1")?;
    for session_id in session_ids {
        for id in by_session.query_map(params![session_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    let mut by_trace =
        conn.prepare("SELECT candidate_id FROM memory_candidates WHERE trace_id = ?1")?;
    for trace_id in trace_ids {
        for id in by_trace.query_map(params![trace_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn query_lifecycle_memory_card_ids(
    conn: &Connection,
    candidate_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut stmt =
        conn.prepare("SELECT memory_card_id FROM memory_cards WHERE source_candidate_id = ?1")?;
    for candidate_id in candidate_ids {
        for id in stmt.query_map(params![candidate_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn query_lifecycle_memory_policy_decision_ids(
    conn: &Connection,
    candidate_ids: &BTreeSet<String>,
    memory_card_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut by_candidate = conn.prepare(
        "SELECT policy_decision_id FROM memory_policy_decisions WHERE candidate_id = ?1",
    )?;
    for candidate_id in candidate_ids {
        for id in by_candidate.query_map(params![candidate_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    let mut by_card = conn.prepare(
        "SELECT policy_decision_id FROM memory_policy_decisions WHERE memory_card_id = ?1",
    )?;
    for memory_card_id in memory_card_ids {
        for id in by_card.query_map(params![memory_card_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn query_lifecycle_memory_transition_audit_ids(
    conn: &Connection,
    candidate_ids: &BTreeSet<String>,
    memory_card_ids: &BTreeSet<String>,
    policy_decision_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut stmt =
        conn.prepare("SELECT audit_id FROM memory_transition_audit WHERE entity_id = ?1")?;
    for id_set in [candidate_ids, memory_card_ids, policy_decision_ids] {
        for entity_id in id_set {
            for audit_id in stmt.query_map(params![entity_id], |row| row.get::<_, String>(0))? {
                ids.insert(audit_id?);
            }
        }
    }
    Ok(ids)
}

fn query_lifecycle_retrieval_failure_ids(
    conn: &Connection,
    trace_ids: &BTreeSet<String>,
    package_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut by_trace =
        conn.prepare("SELECT failure_id FROM retrieval_failures WHERE trace_id = ?1")?;
    for trace_id in trace_ids {
        for id in by_trace.query_map(params![trace_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    let mut by_package =
        conn.prepare("SELECT failure_id FROM retrieval_failures WHERE package_id = ?1")?;
    for package_id in package_ids {
        for id in by_package.query_map(params![package_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn query_lifecycle_governance_task_ids(
    conn: &Connection,
    trace_ids: &BTreeSet<String>,
    package_ids: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut by_trace =
        conn.prepare("SELECT task_id FROM knowledge_governance_tasks WHERE trace_id = ?1")?;
    for trace_id in trace_ids {
        for id in by_trace.query_map(params![trace_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    let mut by_package =
        conn.prepare("SELECT task_id FROM knowledge_governance_tasks WHERE package_id = ?1")?;
    for package_id in package_ids {
        for id in by_package.query_map(params![package_id], |row| row.get::<_, String>(0))? {
            ids.insert(id?);
        }
    }
    Ok(ids)
}

fn rqa_user_lifecycle_export(
    conn: &Connection,
    plan: &RqaUserLifecyclePlan,
    reason: &str,
) -> Result<Value> {
    append_runtime_audit_event(
        conn,
        "rqa-user-lifecycle",
        "rqa_user_data_exported",
        &json!({
            "subject_sha256": &plan.subject_sha256,
            "reason": reason,
            "counts": lifecycle_counts(plan),
            "source_text_included": false,
            "secret_values_printed": false,
        }),
    )?;
    Ok(lifecycle_report(
        "export",
        "ok",
        plan,
        json!({
            "export_manifest": lifecycle_export_manifest(plan),
        }),
    ))
}

fn lifecycle_export_manifest(plan: &RqaUserLifecyclePlan) -> Value {
    let sessions = plan
        .sessions
        .iter()
        .map(|session| {
            json!({
                "session_sha256": hash_text(&session.session_id),
                "user_ref_sha256": hash_text(&session.user_ref),
                "chat_ref_sha256": hash_text(&session.chat_ref),
            })
        })
        .collect::<Vec<_>>();
    let messages = plan
        .messages
        .iter()
        .map(|message| {
            json!({
                "message_sha256": hash_text(&message.message_id),
                "external_message_sha256": hash_text(&message.external_message_id),
                "trace_sha256": hash_text(&message.trace_id),
                "package_sha256": message.package_id.as_ref().map(|package_id| hash_text(package_id)),
                "input_sha256": hash_text(&message.question),
                "response_sha256": hash_text(&message.response_json),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "export_format_version": "tonglingyu-rqa-user-export-v1",
        "content_mode": "redacted_hash_manifest_only",
        "counts": lifecycle_counts(plan),
        "subject_sha256": &plan.subject_sha256,
        "sessions": sessions,
        "messages": messages,
        "trace_sha256": hashed_values(&plan.trace_ids),
        "package_sha256": hashed_values(&plan.package_ids),
        "context_pack_sha256": hashed_values(&plan.context_pack_ids),
        "memory_candidate_sha256": hashed_values(&plan.memory_candidate_ids),
        "memory_card_sha256": hashed_values(&plan.memory_card_ids),
        "memory_policy_decision_sha256": hashed_values(&plan.memory_policy_decision_ids),
        "memory_transition_audit_sha256": hashed_values(&plan.memory_transition_audit_ids),
        "retrieval_failure_sha256": hashed_values(&plan.retrieval_failure_ids),
        "governance_task_sha256": hashed_values(&plan.governance_task_ids),
        "source_text_included": false,
        "response_body_included": false,
        "secret_values_printed": false,
    })
}

fn hashed_values(values: &BTreeSet<String>) -> Vec<String> {
    values.iter().map(|value| hash_text(value)).collect()
}

fn rqa_user_lifecycle_legal_hold(
    conn: &Connection,
    plan: &RqaUserLifecyclePlan,
    reason: &str,
) -> Result<Value> {
    run_immediate_transaction(conn, |tx| {
        tx.execute(
            r#"
            INSERT INTO rqa_user_legal_holds (
                hold_id, user_ref_sha256, reason, active, created_at, released_at
            ) VALUES (?1, ?2, ?3, 1, ?4, NULL)
            "#,
            params![
                format!("rqa-hold-{}", uuid::Uuid::now_v7().simple()),
                &plan.subject_sha256,
                reason,
                now_rfc3339(),
            ],
        )?;
        append_rqa_lifecycle_tombstone(
            tx,
            "rqa_user_data_subject",
            &plan.subject_sha256,
            "legal_hold",
            reason,
            &json!({
                "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                "subject_sha256": &plan.subject_sha256,
                "counts": lifecycle_counts(plan),
                "source_text_included": false,
                "secret_values_printed": false,
            }),
        )?;
        append_runtime_audit_event(
            tx,
            "rqa-user-lifecycle",
            "rqa_user_data_legal_hold_added",
            &json!({
                "subject_sha256": &plan.subject_sha256,
                "reason": reason,
                "counts": lifecycle_counts(plan),
                "secret_values_printed": false,
            }),
        )?;
        Ok(())
    })?;
    Ok(lifecycle_report_with_active_legal_hold_count(
        "legal_hold",
        "ok",
        plan,
        json!({"legal_hold_active": true}),
        plan.active_legal_holds.saturating_add(1),
    ))
}

fn rqa_user_lifecycle_release_legal_hold(
    conn: &Connection,
    plan: &RqaUserLifecyclePlan,
    reason: &str,
) -> Result<Value> {
    let released = run_immediate_transaction(conn, |tx| {
        let released = tx.execute(
            r#"
            UPDATE rqa_user_legal_holds
            SET active = 0, released_at = ?1
            WHERE user_ref_sha256 = ?2 AND active = 1
            "#,
            params![now_rfc3339(), &plan.subject_sha256],
        )?;
        append_rqa_lifecycle_tombstone(
            tx,
            "rqa_user_data_subject",
            &plan.subject_sha256,
            "release_legal_hold",
            reason,
            &json!({
                "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                "subject_sha256": &plan.subject_sha256,
                "released_hold_count": released,
                "source_text_included": false,
                "secret_values_printed": false,
            }),
        )?;
        append_runtime_audit_event(
            tx,
            "rqa-user-lifecycle",
            "rqa_user_data_legal_hold_released",
            &json!({
                "subject_sha256": &plan.subject_sha256,
                "reason": reason,
                "released_hold_count": released,
                "secret_values_printed": false,
            }),
        )?;
        Ok(released)
    })?;
    let released_count = i64::try_from(released).unwrap_or(i64::MAX);
    Ok(lifecycle_report_with_active_legal_hold_count(
        "release_legal_hold",
        "ok",
        plan,
        json!({"released_hold_count": released}),
        plan.active_legal_holds.saturating_sub(released_count),
    ))
}

fn rqa_user_lifecycle_anonymize(
    conn: &Connection,
    user_ref: &str,
    plan: &RqaUserLifecyclePlan,
    reason: &str,
) -> Result<Value> {
    if plan.active_legal_holds > 0 {
        append_runtime_audit_event(
            conn,
            "rqa-user-lifecycle",
            "rqa_user_data_anonymize_blocked",
            &json!({
                "subject_sha256": &plan.subject_sha256,
                "reason": reason,
                "active_legal_hold_count": plan.active_legal_holds,
                "secret_values_printed": false,
            }),
        )?;
        return Ok(lifecycle_report(
            "anonymize",
            "blocked",
            plan,
            json!({"blocked_by_legal_hold": true}),
        ));
    }

    let sensitive_values = lifecycle_sensitive_values(user_ref, plan);
    run_immediate_transaction(conn, |tx| {
        append_rqa_lifecycle_tombstone(
            tx,
            "rqa_user_data_subject",
            &plan.subject_sha256,
            "user_anonymize",
            reason,
            &json!({
                "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                "subject_sha256": &plan.subject_sha256,
                "counts": lifecycle_counts(plan),
                "delete_anonymize_strategy": "anonymize_in_place_to_preserve_rqa_traceability",
                "source_text_included": false,
                "response_body_included": false,
                "secret_values_printed": false,
            }),
        )?;
        for session in &plan.sessions {
            let anonymized_user = format!("anonymized-user:{}", &plan.subject_sha256[..16]);
            let anonymized_chat =
                format!("anonymized-chat:{}", &hash_text(&session.session_id)[..16]);
            tx.execute(
                "UPDATE user_sessions SET external_user_ref = ?1, external_session_id = ?2 WHERE user_session_id = ?3",
                params![anonymized_user, anonymized_chat, &session.session_id],
            )?;
        }
        for message in &plan.messages {
            let response_json = redact_json_string(&message.response_json, &sensitive_values)?;
            let anonymized_external_message = format!(
                "anonymized-message:{}",
                &hash_text(&message.message_id)[..16]
            );
            let redacted_question = format!(
                "[redacted:rqa-user-lifecycle:{}]",
                &hash_text(&message.question)[..12]
            );
            tx.execute(
                "UPDATE session_journal
                 SET external_message_id = CASE WHEN external_message_id IS NULL THEN NULL ELSE ?1 END,
                     content = CASE WHEN content IS NULL THEN NULL ELSE ?2 END,
                     summary = ?2,
                     metadata_json = ?3
                 WHERE journal_id = ?4",
                params![
                    anonymized_external_message,
                    redacted_question,
                    response_json,
                    &message.message_id,
                ],
            )?;
        }
        for package_id in &plan.package_ids {
            tx.execute(
                "UPDATE evidence_packages SET question = ?1 WHERE package_id = ?2",
                params![
                    format!(
                        "[redacted:rqa-user-lifecycle:{}]",
                        &hash_text(package_id)[..12]
                    ),
                    package_id,
                ],
            )?;
        }
        for context_pack_id in &plan.context_pack_ids {
            redact_text_column_by_ids(
                tx,
                "context_packs",
                "context_pack_id",
                "resolved_question",
                &BTreeSet::from([context_pack_id.clone()]),
                &sensitive_values,
            )?;
            redact_text_column_by_ids(
                tx,
                "context_packs",
                "context_pack_id",
                "session_summary",
                &BTreeSet::from([context_pack_id.clone()]),
                &sensitive_values,
            )?;
            for column in [
                "active_scopes_json",
                "candidate_scopes_json",
                "allowed_tools_json",
                "forbidden_tools_json",
                "memory_read_refs_json",
                "forbidden_context_json",
                "output_contract_json",
                "profile_views_json",
            ] {
                redact_json_column_by_ids(
                    tx,
                    "context_packs",
                    "context_pack_id",
                    column,
                    &BTreeSet::from([context_pack_id.clone()]),
                    &sensitive_values,
                )?;
            }
        }
        let anonymized_memory_scope = format!(
            "user_private:sha256:{}",
            hash_text(&format!("anonymized-memory:{}", plan.subject_sha256))
        );
        redact_text_column_by_ids(
            tx,
            "memory_candidates",
            "candidate_id",
            "summary",
            &plan.memory_candidate_ids,
            &sensitive_values,
        )?;
        redact_text_column_by_ids(
            tx,
            "memory_candidates",
            "candidate_id",
            "raw_excerpt_redacted",
            &plan.memory_candidate_ids,
            &sensitive_values,
        )?;
        for column in ["risk_flags_json", "llm_extraction_json"] {
            redact_json_column_by_ids(
                tx,
                "memory_candidates",
                "candidate_id",
                column,
                &plan.memory_candidate_ids,
                &sensitive_values,
            )?;
        }
        update_user_private_scope_refs(
            tx,
            "memory_candidates",
            "candidate_id",
            &plan.memory_candidate_ids,
            &anonymized_memory_scope,
        )?;
        redact_text_column_by_ids(
            tx,
            "memory_cards",
            "memory_card_id",
            "summary",
            &plan.memory_card_ids,
            &sensitive_values,
        )?;
        redact_json_column_by_ids(
            tx,
            "memory_cards",
            "memory_card_id",
            "acl_json",
            &plan.memory_card_ids,
            &sensitive_values,
        )?;
        update_user_private_scope_refs(
            tx,
            "memory_cards",
            "memory_card_id",
            &plan.memory_card_ids,
            &anonymized_memory_scope,
        )?;
        disable_memory_card_reads_for_ids(tx, &plan.memory_card_ids)?;
        redact_text_column_by_ids(
            tx,
            "memory_policy_decisions",
            "policy_decision_id",
            "decision_reason",
            &plan.memory_policy_decision_ids,
            &sensitive_values,
        )?;
        for column in ["rule_filter_json", "llm_filter_json", "risk_flags_json"] {
            redact_json_column_by_ids(
                tx,
                "memory_policy_decisions",
                "policy_decision_id",
                column,
                &plan.memory_policy_decision_ids,
                &sensitive_values,
            )?;
        }
        update_user_private_scope_refs(
            tx,
            "memory_policy_decisions",
            "policy_decision_id",
            &plan.memory_policy_decision_ids,
            &anonymized_memory_scope,
        )?;
        redact_json_column_by_ids(
            tx,
            "memory_transition_audit",
            "audit_id",
            "metadata_json",
            &plan.memory_transition_audit_ids,
            &sensitive_values,
        )?;
        redact_json_column_by_ids(
            tx,
            "workflow_states",
            "state_id",
            "detail_json",
            &plan.workflow_state_ids,
            &sensitive_values,
        )?;
        redact_json_column_by_ids(
            tx,
            "audit_events",
            "event_id",
            "payload_json",
            &plan.audit_event_ids,
            &sensitive_values,
        )?;
        append_runtime_audit_event(
            tx,
            "rqa-user-lifecycle",
            "rqa_user_data_anonymized",
            &json!({
                "subject_sha256": &plan.subject_sha256,
                "reason": reason,
                "counts": lifecycle_counts(plan),
                "delete_anonymize_strategy": "anonymize_in_place_to_preserve_rqa_traceability",
                "secret_values_printed": false,
            }),
        )?;
        Ok(())
    })?;
    Ok(lifecycle_report(
        "anonymize",
        "ok",
        plan,
        json!({"delete_anonymize_strategy": "anonymize_in_place_to_preserve_rqa_traceability"}),
    ))
}

fn lifecycle_sensitive_values(user_ref: &str, plan: &RqaUserLifecyclePlan) -> Vec<String> {
    let mut values = BTreeSet::new();
    if !user_ref.is_empty() {
        values.insert(user_ref.to_string());
    }
    for session in &plan.sessions {
        values.insert(session.user_ref.clone());
        values.insert(session.chat_ref.clone());
    }
    for message in &plan.messages {
        values.insert(message.external_message_id.clone());
        values.insert(message.question.clone());
    }
    values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn redact_text_column_by_ids(
    conn: &Connection,
    table: &str,
    id_column: &str,
    text_column: &str,
    ids: &BTreeSet<String>,
    sensitive_values: &[String],
) -> Result<()> {
    let select_sql = format!("SELECT {text_column} FROM {table} WHERE {id_column} = ?1");
    let update_sql = format!("UPDATE {table} SET {text_column} = ?1 WHERE {id_column} = ?2");
    let mut select = conn.prepare(&select_sql)?;
    for id in ids {
        let value = select
            .query_row(params![id], |row| row.get::<_, String>(0))
            .optional()?;
        if let Some(value) = value {
            conn.execute(
                &update_sql,
                params![redact_plain_text(&value, sensitive_values), id],
            )?;
        }
    }
    Ok(())
}

fn redact_json_column_by_ids(
    conn: &Connection,
    table: &str,
    id_column: &str,
    json_column: &str,
    ids: &BTreeSet<String>,
    sensitive_values: &[String],
) -> Result<()> {
    let select_sql = format!("SELECT {json_column} FROM {table} WHERE {id_column} = ?1");
    let update_sql = format!("UPDATE {table} SET {json_column} = ?1 WHERE {id_column} = ?2");
    let mut select = conn.prepare(&select_sql)?;
    for id in ids {
        let value = select
            .query_row(params![id], |row| row.get::<_, String>(0))
            .optional()?;
        if let Some(value) = value {
            let redacted = redact_json_string(&value, sensitive_values)?;
            conn.execute(&update_sql, params![redacted, id])?;
        }
    }
    Ok(())
}

fn update_user_private_scope_refs(
    conn: &Connection,
    table: &str,
    id_column: &str,
    ids: &BTreeSet<String>,
    anonymized_scope_ref: &str,
) -> Result<()> {
    let update_sql = format!(
        "UPDATE {table}
         SET scope_ref = CASE WHEN scope_type = 'user_private' THEN ?1 ELSE scope_ref END
         WHERE {id_column} = ?2"
    );
    for id in ids {
        conn.execute(&update_sql, params![anonymized_scope_ref, id])?;
    }
    Ok(())
}

fn disable_memory_card_reads_for_ids(
    conn: &Connection,
    memory_card_ids: &BTreeSet<String>,
) -> Result<()> {
    let mut select = conn.prepare("SELECT acl_json FROM memory_cards WHERE memory_card_id = ?1")?;
    for memory_card_id in memory_card_ids {
        let acl_json = select
            .query_row(params![memory_card_id], |row| row.get::<_, String>(0))
            .optional()?;
        let mut acl = acl_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap_or_else(|| json!({}));
        if let Some(object) = acl.as_object_mut() {
            object.insert("read_enabled".to_string(), json!(false));
        }
        conn.execute(
            "UPDATE memory_cards SET read_enabled = 0, acl_json = ?1 WHERE memory_card_id = ?2",
            params![serde_json::to_string(&acl)?, memory_card_id],
        )?;
    }
    Ok(())
}

fn redact_json_string(value: &str, sensitive_values: &[String]) -> Result<String> {
    match serde_json::from_str::<Value>(value) {
        Ok(parsed) => Ok(serde_json::to_string(&redact_json_value(
            parsed,
            sensitive_values,
        ))?),
        Err(_) => Ok(redact_plain_text(value, sensitive_values)),
    }
}

fn redact_json_value(value: Value, sensitive_values: &[String]) -> Value {
    match value {
        Value::String(text) => Value::String(redact_plain_text(&text, sensitive_values)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_json_value(item, sensitive_values))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, redact_json_value(value, sensitive_values)))
                .collect(),
        ),
        other => other,
    }
}

fn redact_plain_text(value: &str, sensitive_values: &[String]) -> String {
    sensitive_values
        .iter()
        .fold(value.to_string(), |redacted, sensitive| {
            if sensitive.is_empty() {
                redacted
            } else {
                redacted.replace(
                    sensitive,
                    &format!("[redacted:{}]", &hash_text(sensitive)[..12]),
                )
            }
        })
}

fn lifecycle_counts(plan: &RqaUserLifecyclePlan) -> Value {
    json!({
        "session_count": plan.sessions.len(),
        "message_count": plan.messages.len(),
        "trace_count": plan.trace_ids.len(),
        "package_count": plan.package_ids.len(),
        "context_pack_count": plan.context_pack_ids.len(),
        "memory_candidate_count": plan.memory_candidate_ids.len(),
        "memory_card_count": plan.memory_card_ids.len(),
        "memory_policy_decision_count": plan.memory_policy_decision_ids.len(),
        "memory_transition_audit_count": plan.memory_transition_audit_ids.len(),
        "workflow_state_count": plan.workflow_state_ids.len(),
        "audit_event_count": plan.audit_event_ids.len(),
        "retrieval_failure_count": plan.retrieval_failure_ids.len(),
        "governance_task_count": plan.governance_task_ids.len(),
        "active_legal_hold_count": plan.active_legal_holds,
    })
}

fn lifecycle_report(
    action: &str,
    status: &str,
    plan: &RqaUserLifecyclePlan,
    extra: Value,
) -> Value {
    json!({
        "object": "tonglingyu.rqa_user_lifecycle_report",
        "schema_version": 1,
        "status": status,
        "action": action,
        "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
        "subject_sha256": &plan.subject_sha256,
        "counts": lifecycle_counts(plan),
        "extra": extra,
        "refs": {
            "trace_count": plan.trace_ids.len(),
            "package_count": plan.package_ids.len(),
            "memory_candidate_count": plan.memory_candidate_ids.len(),
            "memory_card_count": plan.memory_card_ids.len(),
            "memory_policy_decision_count": plan.memory_policy_decision_ids.len(),
            "retrieval_failure_count": plan.retrieval_failure_ids.len(),
            "governance_task_count": plan.governance_task_ids.len(),
        },
        "source_text_included": false,
        "response_body_included": false,
        "secret_values_printed": false,
    })
}

fn lifecycle_report_with_active_legal_hold_count(
    action: &str,
    status: &str,
    plan: &RqaUserLifecyclePlan,
    extra: Value,
    active_legal_hold_count: i64,
) -> Value {
    let mut report = lifecycle_report(action, status, plan, extra);
    if let Some(counts) = report.get_mut("counts").and_then(Value::as_object_mut) {
        counts.insert(
            "active_legal_hold_count".to_string(),
            json!(active_legal_hold_count),
        );
    }
    report
}
