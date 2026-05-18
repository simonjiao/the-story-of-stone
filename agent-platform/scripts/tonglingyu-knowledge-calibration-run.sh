#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${ROOT}/.." && pwd)"
CARGO_BIN="${CARGO:-cargo}"
RUN_ID="${TONGLINGYU_KNOWLEDGE_CALIBRATION_RUN_ID:-local-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
ARTIFACT_ROOT="${TONGLINGYU_KNOWLEDGE_CALIBRATION_ARTIFACT_ROOT:-${REPO_ROOT}/data/tonglingyu/knowledge-calibration-runs}"
ARTIFACT_DIR="${TONGLINGYU_KNOWLEDGE_CALIBRATION_ARTIFACT_DIR:-${ARTIFACT_ROOT}/${RUN_ID}}"
DB_PATH="${TONGLINGYU_KNOWLEDGE_CALIBRATION_DB_PATH:-${ARTIFACT_DIR}/knowledge-calibration.db}"
INPUT_PATH="${TONGLINGYU_KNOWLEDGE_CALIBRATION_INPUT_PATH:-${ARTIFACT_DIR}/calibration-input.json}"
REPORT_PATH="${TONGLINGYU_KNOWLEDGE_CALIBRATION_REPORT_PATH:-${ARTIFACT_DIR}/calibration-report.json}"
SUMMARY_PATH="${TONGLINGYU_KNOWLEDGE_CALIBRATION_SUMMARY_PATH:-${ARTIFACT_DIR}/calibration-run-summary.json}"
STDERR_PATH="${ARTIFACT_DIR}/knowledge-calibrate.stderr"
GATEWAY_BIN="${TONGLINGYU_GATEWAY_BIN:-}"

mkdir -p "${ARTIFACT_DIR}"

python3 - "${DB_PATH}" "${INPUT_PATH}" <<'PY'
import datetime
import hashlib
import json
import sqlite3
import sys
import uuid
from pathlib import Path

db_path = Path(sys.argv[1])
input_path = Path(sys.argv[2])
db_path.parent.mkdir(parents=True, exist_ok=True)
input_path.parent.mkdir(parents=True, exist_ok=True)
conn = sqlite3.connect(str(db_path))
try:
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS audit_events (
            event_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS knowledge_items (
            item_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            state TEXT NOT NULL,
            source_refs_json TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            payload_sha256 TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            source_boundary_json TEXT,
            calibration_report_ref TEXT,
            confidence REAL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            state_version INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS knowledge_item_state_history (
            history_id TEXT PRIMARY KEY,
            item_id TEXT NOT NULL REFERENCES knowledge_items(item_id),
            previous_state TEXT,
            new_state TEXT NOT NULL,
            actor TEXT NOT NULL,
            reason_sha256 TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL,
            state_version INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS knowledge_calibration_reports (
            report_id TEXT PRIMARY KEY,
            report_ref TEXT NOT NULL UNIQUE,
            item_id TEXT NOT NULL REFERENCES knowledge_items(item_id),
            kind TEXT NOT NULL,
            method TEXT NOT NULL,
            decision TEXT NOT NULL,
            confidence REAL NOT NULL,
            quality_issues_json TEXT NOT NULL,
            source_refs_json TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL,
            source_boundary_json TEXT NOT NULL,
            coverage_matrix_json TEXT NOT NULL,
            config_summary_json TEXT,
            report_json TEXT NOT NULL,
            report_hash TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        """
    )
    marker = "calibration-run-smoke"
    payload = {
        "claim": f"sample knowledge item {marker}",
        "marker": marker,
    }
    source_refs = [f"source://wikisource/chapter/{marker}"]
    evidence_refs = [f"block://wikisource/{marker}"]
    payload_json = json.dumps(
        payload,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    )
    source_refs_json = json.dumps(source_refs, ensure_ascii=True, separators=(",", ":"))
    evidence_refs_json = json.dumps(evidence_refs, ensure_ascii=True, separators=(",", ":"))
    payload_sha256 = hashlib.sha256(payload_json.encode("utf-8")).hexdigest()
    item_id = f"knowledge-item-calibration-smoke-{payload_sha256[:16]}"
    now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat()
    now = now.replace("+00:00", "Z")
    conn.execute(
        """
        INSERT OR REPLACE INTO knowledge_items (
            item_id, kind, state, source_refs_json, evidence_refs_json,
            payload_json, payload_sha256, schema_version, created_at,
            updated_at, state_version
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            item_id,
            "term",
            "candidate",
            source_refs_json,
            evidence_refs_json,
            payload_json,
            payload_sha256,
            "tonglingyu-knowledge-item-states-v1",
            now,
            now,
            1,
        ),
    )
    conn.execute(
        """
        INSERT INTO knowledge_item_state_history (
            history_id, item_id, previous_state, new_state, actor,
            reason_sha256, evidence_refs_json, state_version, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            f"kish-{uuid.uuid4().hex}",
            item_id,
            None,
            "candidate",
            "knowledge-calibration-smoke",
            hashlib.sha256(b"create smoke candidate").hexdigest(),
            evidence_refs_json,
            1,
            now,
        ),
    )
    run_input = {
        "actor": "runtime-calibrator-smoke",
        "eval_context": None,
        "input_kind": "source_snapshot",
        "input_ref": f"source://wikisource/chapter/{marker}",
        "item_id": item_id,
        "llm_config": None,
        "llm_judgement": None,
        "method": "rule",
        "rqa_context": None,
        "rule_context": {
            "block_id": f"wikisource/{marker}",
            "exact_terms": [marker],
            "required_evidence_type": "base_text",
            "source_id": "wikisource",
            "usage_boundary": "runtime candidate, not human marked",
            "version_boundary": "Wikisource source snapshot only",
        },
        "trace_id": f"trace-calibration-{marker}",
    }
    with input_path.open("w", encoding="utf-8") as handle:
        json.dump(run_input, handle, ensure_ascii=True, indent=2, sort_keys=True)
        handle.write("\n")
    conn.commit()
finally:
    conn.close()
PY

if [[ -n "${GATEWAY_BIN}" && -x "${GATEWAY_BIN}" ]]; then
  "${GATEWAY_BIN}" knowledge-calibrate \
    --db "${DB_PATH}" \
    --input "${INPUT_PATH}" \
    >"${REPORT_PATH}" 2>"${STDERR_PATH}"
else
  (
    cd "${ROOT}"
    "${CARGO_BIN}" run -p tonglingyu-gateway -- knowledge-calibrate \
      --db "${DB_PATH}" \
      --input "${INPUT_PATH}"
  ) >"${REPORT_PATH}" 2>"${STDERR_PATH}"
fi

python3 - "${DB_PATH}" "${INPUT_PATH}" "${REPORT_PATH}" "${SUMMARY_PATH}" \
  "${RUN_ID}" <<'PY'
import json
import sqlite3
import sys
from pathlib import Path

db_path = Path(sys.argv[1])
input_path = Path(sys.argv[2])
report_path = Path(sys.argv[3])
summary_path = Path(sys.argv[4])
run_id = sys.argv[5]
errors = []

try:
    with report_path.open("r", encoding="utf-8") as handle:
        report = json.load(handle)
except (OSError, json.JSONDecodeError) as exc:
    raise SystemExit(f"calibration report unreadable: {exc}")

if report.get("method") != "rule":
    errors.append("method_not_rule")
if report.get("decision") != "system_calibrated":
    errors.append("decision_not_system_calibrated")
if not report.get("evidence_refs"):
    errors.append("evidence_refs_missing")
if not isinstance(report.get("source_boundary"), dict) or not report["source_boundary"]:
    errors.append("source_boundary_missing")
if report.get("coverage_matrix", {}).get("runtime_usable_auto_promotion") is not False:
    errors.append("runtime_usable_auto_promotion_not_false")
if report.get("coverage_matrix", {}).get("runtime_policy_rejected") is not True:
    errors.append("runtime_policy_rejected_not_true")
report_body = report.get("report") or {}
if report_body.get("fact_layer_mutated") is not False:
    errors.append("fact_layer_mutated_not_false")
if report_body.get("secret_values_stored") is not False:
    errors.append("secret_values_stored_not_false")

conn = sqlite3.connect(str(db_path))
try:
    item = conn.execute(
        """
        SELECT state, calibration_report_ref, confidence, state_version
        FROM knowledge_items
        WHERE item_id = ?
        """,
        (report.get("item_id"),),
    ).fetchone()
    if item is None:
        errors.append("item_missing_after_calibration")
    else:
        state, calibration_report_ref, confidence, state_version = item
        if state != "system_calibrated":
            errors.append(f"item_state_invalid={state}")
        if calibration_report_ref != report.get("report_ref"):
            errors.append("calibration_report_ref_mismatch")
        if confidence is None or confidence < 0.8:
            errors.append("confidence_below_runtime_threshold")
        if state_version != 2:
            errors.append("state_version_not_incremented")
    report_count = conn.execute(
        "SELECT COUNT(*) FROM knowledge_calibration_reports"
    ).fetchone()[0]
    if report_count != 1:
        errors.append(f"calibration_report_count_invalid={report_count}")
    history_count = conn.execute(
        """
        SELECT COUNT(*)
        FROM knowledge_item_state_history
        WHERE item_id = ?
        """,
        (report.get("item_id"),),
    ).fetchone()[0]
    if history_count < 2:
        errors.append("state_history_missing_calibration_transition")
    audit_count = conn.execute(
        """
        SELECT COUNT(*)
        FROM audit_events
        WHERE event_type = 'knowledge_calibration_report_created'
        """
    ).fetchone()[0]
    if audit_count != 1:
        errors.append("calibration_audit_event_missing")
finally:
    conn.close()

summary = {
    "object": "tonglingyu.knowledge_calibration_run_smoke",
    "schema_version": 1,
    "status": "passed" if not errors else "failed",
    "run_id": run_id,
    "db_path": str(db_path),
    "input_path": str(input_path),
    "report_path": str(report_path),
    "summary_path": str(summary_path),
    "item_id": report.get("item_id"),
    "report_ref": report.get("report_ref"),
    "decision": report.get("decision"),
    "runtime_usable_auto_promotion": report.get("coverage_matrix", {}).get(
        "runtime_usable_auto_promotion"
    ),
    "secret_values_printed": False,
    "errors": errors,
}
summary_path.parent.mkdir(parents=True, exist_ok=True)
with summary_path.open("w", encoding="utf-8") as handle:
    json.dump(summary, handle, ensure_ascii=True, sort_keys=True)
    handle.write("\n")
print(json.dumps(summary, ensure_ascii=True, sort_keys=True))
if errors:
    raise SystemExit(1)
PY
