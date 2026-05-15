#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

DB_PATH="${TONGLINGYU_RQA_EVAL_ARTIFACT_DB_PATH:-${TONGLINGYU_RQA_DB_PATH:-${REPO_DIR}/data/tonglingyu/tonglingyu.db}}"
REPORT_PATH="${TONGLINGYU_RQA_EVAL_ARTIFACT_REPORT_PATH:-}"
APPLY="${TONGLINGYU_RQA_EVAL_ARTIFACT_APPLY:-false}"
BACKUP_PATH="${TONGLINGYU_RQA_EVAL_ARTIFACT_BACKUP_PATH:-}"
ACTOR="${TONGLINGYU_RQA_EVAL_ARTIFACT_ACTOR:-rqa-eval-artifact-remediation}"
REASON="${TONGLINGYU_RQA_EVAL_ARTIFACT_REASON:-pre-snapshot eval artifact; eval now runs on SQLite snapshot}"

python3 - "${DB_PATH}" "${REPORT_PATH}" "${APPLY}" "${BACKUP_PATH}" "${ACTOR}" "${REASON}" <<'PY'
import hashlib
import json
import shutil
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    db_path_raw,
    report_path_raw,
    apply_raw,
    backup_path_raw,
    actor,
    reason,
) = sys.argv[1:7]

POLICY_VERSION = "tonglingyu-rqa-eval-artifact-remediation-v1"
db_path = Path(db_path_raw)
apply_changes = str(apply_raw).strip().lower() in {"1", "true", "yes", "on"}
errors = []


def now_iso():
    return datetime.now(timezone.utc).isoformat()


def sha256_text(value):
    return hashlib.sha256(str(value).encode("utf-8")).hexdigest()


def backup_database(source, target):
    target.parent.mkdir(parents=True, exist_ok=True)
    source_conn = sqlite3.connect(str(source))
    target_conn = sqlite3.connect(str(target))
    try:
        source_conn.backup(target_conn)
    finally:
        target_conn.close()
        source_conn.close()
    target.chmod(0o600)


def event_id(prefix, key, index):
    return f"{prefix}-{sha256_text(key)[:20]}-{index:06d}"


if not db_path.is_file():
    errors.append("db_not_found")

candidate_failures = []
candidate_tasks = []
backup_path = ""

if not errors:
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    try:
        candidate_failures = [
            dict(row)
            for row in conn.execute(
                """
                SELECT failure_id, trace_id, package_id, failure_type,
                       human_review_status, updated_at
                FROM retrieval_failures
                WHERE trace_id LIKE 'eval-tly-%'
                  AND human_review_status IN ('open', 'in_review')
                ORDER BY created_at, failure_id
                """
            )
        ]
        failure_ids = [row["failure_id"] for row in candidate_failures]
        if failure_ids:
            placeholders = ",".join("?" for _ in failure_ids)
            candidate_tasks = [
                dict(row)
                for row in conn.execute(
                    f"""
                    SELECT task_id, source_failure_id, trace_id, package_id,
                           task_type, status, updated_at
                    FROM knowledge_governance_tasks
                    WHERE source_failure_id IN ({placeholders})
                      AND status IN ('open', 'in_review', 'accepted')
                    ORDER BY created_at, task_id
                    """,
                    failure_ids,
                )
            ]
    except sqlite3.Error:
        errors.append("db_query_failed")
    finally:
        conn.close()

if apply_changes and not errors:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    if backup_path_raw.strip():
        backup = Path(backup_path_raw)
    else:
        backup = db_path.parent / "backups" / f"rqa-eval-artifact-remediation-{timestamp}.db"
    try:
        backup_database(db_path, backup)
        backup_path = str(backup)
    except OSError:
        errors.append("db_backup_failed")

if apply_changes and not errors:
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    try:
        now = now_iso()
        reason_hash = sha256_text(reason)
        conn.execute("BEGIN IMMEDIATE")
        for index, row in enumerate(candidate_failures, start=1):
            cursor = conn.execute(
                """
                UPDATE retrieval_failures
                SET human_review_status = 'resolved',
                    reviewer = ?,
                    review_note = ?,
                    updated_at = ?,
                    resolved_at = ?
                WHERE failure_id = ?
                  AND trace_id LIKE 'eval-tly-%'
                  AND human_review_status = ?
                """,
                (
                    actor,
                    reason,
                    now,
                    now,
                    row["failure_id"],
                    row["human_review_status"],
                ),
            )
            if cursor.rowcount != 1:
                raise sqlite3.DatabaseError("failure_update_lost_race")
            payload = {
                "actor": actor,
                "failure_id": row["failure_id"],
                "previous_status": row["human_review_status"],
                "new_status": "resolved",
                "human_review_status": "resolved",
                "reason_sha256": reason_hash,
                "review_note_sha256": reason_hash,
                "status_history": {
                    "previous_status": row["human_review_status"],
                    "new_status": "resolved",
                    "reason_sha256": reason_hash,
                    "timestamp": now,
                },
                "remediation_policy_version": POLICY_VERSION,
            }
            conn.execute(
                """
                INSERT INTO audit_events (event_id, trace_id, event_type, payload_json, created_at)
                VALUES (?1, ?2, 'retrieval_failure_status_updated', ?3, ?4)
                """,
                (
                    event_id("audit-rf-eval-remediation", row["failure_id"], index),
                    row["trace_id"],
                    json.dumps(payload, ensure_ascii=True, sort_keys=True),
                    now,
                ),
            )
        for index, row in enumerate(candidate_tasks, start=1):
            cursor = conn.execute(
                """
                UPDATE knowledge_governance_tasks
                SET status = 'closed',
                    reviewer = ?,
                    review_note = ?,
                    updated_at = ?,
                    closed_at = ?
                WHERE task_id = ?
                  AND status = ?
                  AND source_failure_id IN (
                    SELECT failure_id
                    FROM retrieval_failures
                    WHERE trace_id LIKE 'eval-tly-%'
                  )
                """,
                (
                    actor,
                    reason,
                    now,
                    now,
                    row["task_id"],
                    row["status"],
                ),
            )
            if cursor.rowcount != 1:
                raise sqlite3.DatabaseError("task_update_lost_race")
            payload = {
                "actor": actor,
                "task_id": row["task_id"],
                "source_failure_id": row["source_failure_id"],
                "previous_status": row["status"],
                "new_status": "closed",
                "reason_sha256": reason_hash,
                "status_history": {
                    "previous_status": row["status"],
                    "new_status": "closed",
                    "reason_sha256": reason_hash,
                    "timestamp": now,
                },
                "remediation_policy_version": POLICY_VERSION,
            }
            conn.execute(
                """
                INSERT INTO audit_events (event_id, trace_id, event_type, payload_json, created_at)
                VALUES (?1, ?2, 'governance_task_status_updated', ?3, ?4)
                """,
                (
                    event_id("audit-kgt-eval-remediation", row["task_id"], index),
                    row["trace_id"],
                    json.dumps(payload, ensure_ascii=True, sort_keys=True),
                    now,
                ),
            )
        summary_payload = {
            "actor": actor,
            "reason_sha256": reason_hash,
            "remediation_policy_version": POLICY_VERSION,
            "retrieval_failure_count": len(candidate_failures),
            "governance_task_count": len(candidate_tasks),
            "backup_path_sha256": sha256_text(backup_path),
        }
        conn.execute(
            """
            INSERT INTO audit_events (event_id, trace_id, event_type, payload_json, created_at)
            VALUES (?1, ?2, 'rqa_eval_artifact_remediated', ?3, ?4)
            """,
            (
                event_id("audit-rqa-eval-remediation", f"{now}:{len(candidate_failures)}", 1),
                "rqa-eval-artifact-remediation",
                json.dumps(summary_payload, ensure_ascii=True, sort_keys=True),
                now,
            ),
        )
        conn.commit()
    except sqlite3.Error:
        conn.rollback()
        errors.append("db_apply_failed")
    finally:
        conn.close()

by_failure_type = {}
for row in candidate_failures:
    by_failure_type[row["failure_type"]] = by_failure_type.get(row["failure_type"], 0) + 1
by_task_type = {}
for row in candidate_tasks:
    by_task_type[row["task_type"]] = by_task_type.get(row["task_type"], 0) + 1

payload = {
    "object": "tonglingyu.rqa_eval_artifact_remediation",
    "schema_version": 1,
    "status": "failed" if errors else "ok",
    "policy_version": POLICY_VERSION,
    "mode": "apply" if apply_changes else "dry_run",
    "applied": apply_changes and not errors,
    "db_path_sha256": sha256_text(str(db_path)),
    "backup_path": backup_path,
    "backup_path_sha256": sha256_text(backup_path) if backup_path else "",
    "eligible_retrieval_failure_count": len(candidate_failures),
    "eligible_governance_task_count": len(candidate_tasks),
    "by_failure_type": by_failure_type,
    "by_task_type": by_task_type,
    "sample_failure_sha256": [
        sha256_text(row["failure_id"]) for row in candidate_failures[:5]
    ],
    "sample_task_sha256": [
        sha256_text(row["task_id"]) for row in candidate_tasks[:5]
    ],
    "generated_at": now_iso(),
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
print(encoded)
if report_path_raw:
    report_path = Path(report_path_raw)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
if errors:
    raise SystemExit(1)
PY
