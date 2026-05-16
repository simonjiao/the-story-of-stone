#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve-layout.sh
. "${SCRIPT_DIR}/lib/resolve-layout.sh"
resolve_tonglingyu_layout "${SCRIPT_DIR}"
# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

RUN_ID="${TONGLINGYU_KB_SOURCE_METADATA_RUN_ID:-kb-source-metadata-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
APPLY="${TONGLINGYU_KB_SOURCE_METADATA_APPLY:-false}"
DB_PATH_RAW="${TONGLINGYU_KB_SOURCE_METADATA_DB_PATH:-${TONGLINGYU_RQA_DB_PATH:-${TONGLINGYU_DB_PATH:-${REPO_DIR}/data/tonglingyu/tonglingyu.db}}}"
SOURCE_ROOT_RAW="${TONGLINGYU_KB_SOURCE_METADATA_SOURCE_ROOT:-${REPO_DIR}/resources/sources/wiki}"
BACKUP_PATH_RAW="${TONGLINGYU_KB_SOURCE_METADATA_BACKUP_PATH:-${REPO_DIR}/data/tonglingyu/backups/${RUN_ID}.db}"
REPORT_PATH="${TONGLINGYU_KB_SOURCE_METADATA_REPORT_PATH:-}"

resolve_path() {
  local raw_path="$1"
  python3 - "${raw_path}" "${REPO_DIR}" <<'PY'
import sys
from pathlib import Path

raw, repo_dir = sys.argv[1:3]
path = Path(raw)
if not path.is_absolute():
    path = Path(repo_dir) / path
print(path.resolve())
PY
}

is_true() {
  case "${1:-}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

DB_PATH="$(resolve_path "${DB_PATH_RAW}")"
SOURCE_ROOT="$(resolve_path "${SOURCE_ROOT_RAW}")"
BACKUP_PATH="$(resolve_path "${BACKUP_PATH_RAW}")"

python3 - "${DB_PATH}" "${SOURCE_ROOT}" "${BACKUP_PATH}" "${REPORT_PATH}" \
  "${RUN_ID}" "${APPLY}" <<'PY'
import hashlib
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

(
    db_raw,
    source_root_raw,
    backup_raw,
    report_raw,
    run_id,
    apply_raw,
) = sys.argv[1:7]
db_path = Path(db_raw)
source_root = Path(source_root_raw)
backup_path = Path(backup_raw)
report_path = Path(report_raw) if report_raw else None
apply = apply_raw.lower() in {"1", "true", "yes", "on"}
metadata_fields = [
    "source_url",
    "license",
    "license_url",
    "license_source_url",
    "attribution",
    "usage_boundary",
]
runtime_count_tables = [
    "evidence_packages",
    "audit_events",
    "retrieval_failures",
    "knowledge_governance_tasks",
]


def sha256_file(path):
    if not path.is_file():
        return ""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_source_metadata(root):
    sources = {}
    missing = []
    if not root.is_dir():
        return sources, ["source_root_missing"]
    for source_json in sorted(root.glob("*/metadata/source.json")):
        try:
            value = json.loads(source_json.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            missing.append(f"invalid_source_metadata:{source_json.parent.parent.name}")
            continue
        source_id = str(value.get("source_id") or "").strip()
        if not source_id:
            missing.append(f"source_id_missing:{source_json.parent.parent.name}")
            continue
        fields = {field: value.get(field) for field in metadata_fields}
        for field, field_value in fields.items():
            if not isinstance(field_value, str) or not field_value.strip():
                missing.append(f"{source_id}.{field}_missing")
        sources[source_id] = {
            field: str(fields[field]).strip()
            for field in metadata_fields
            if isinstance(fields[field], str)
        }
    if not sources:
        missing.append("source_metadata_empty")
    return sources, missing


def table_columns(conn, table):
    return {
        row[1]
        for row in conn.execute(f"PRAGMA table_info({table})")
    }


def runtime_counts(conn):
    counts = {}
    for table in runtime_count_tables:
        try:
            counts[table] = conn.execute(
                f"SELECT count(*) FROM {table}"
            ).fetchone()[0]
        except sqlite3.Error:
            counts[table] = None
    return counts


def missing_value_summary(conn, source_ids, existing_columns):
    summary = {}
    for field in metadata_fields:
        if field not in existing_columns:
            summary[field] = "column_missing"
            continue
        summary[field] = conn.execute(
            f"""
            SELECT count(*)
            FROM sources
            WHERE source_id IN ({",".join("?" for _ in source_ids)})
              AND ({field} IS NULL OR trim({field}) = '')
            """,
            list(source_ids),
        ).fetchone()[0]
    return summary


errors = []
sources, metadata_errors = load_source_metadata(source_root)
if metadata_errors:
    errors.extend(metadata_errors)
if not db_path.is_file():
    errors.append("db_missing")

backup_sha = ""
backup_created = False
db_sha_before = sha256_file(db_path)
db_sha_after = ""
runtime_counts_before = {}
runtime_counts_after = {}
missing_columns = []
missing_values_before = {}
missing_values_after = {}
updated_sources = []
db_source_ids = []

if db_path.is_file() and sources:
    conn = sqlite3.connect(str(db_path), timeout=30)
    conn.row_factory = sqlite3.Row
    try:
        existing_columns = table_columns(conn, "sources")
        if not existing_columns:
            errors.append("sources_table_missing")
        missing_columns = [
            field for field in metadata_fields if field not in existing_columns
        ]
        db_source_ids = [
            row["source_id"]
            for row in conn.execute("SELECT source_id FROM sources ORDER BY source_id")
        ]
        source_ids_for_summary = [
            source_id for source_id in db_source_ids if source_id in sources
        ]
        if source_ids_for_summary:
            missing_values_before = missing_value_summary(
                conn,
                source_ids_for_summary,
                existing_columns,
            )
        missing_source_metadata = [
            source_id for source_id in db_source_ids if source_id not in sources
        ]
        if missing_source_metadata:
            errors.extend(
                f"metadata_not_found_for_db_source:{source_id}"
                for source_id in missing_source_metadata
            )
        runtime_counts_before = runtime_counts(conn)
    finally:
        conn.close()

    if apply and not errors:
        backup_path.parent.mkdir(parents=True, exist_ok=True)
        if backup_path.exists():
            errors.append("backup_path_already_exists")
        else:
            source = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
            target = sqlite3.connect(str(backup_path))
            try:
                source.backup(target)
            finally:
                target.close()
                source.close()
            backup_created = True
            backup_sha = sha256_file(backup_path)

    if apply and not errors:
        conn = sqlite3.connect(str(db_path), timeout=30)
        conn.row_factory = sqlite3.Row
        try:
            conn.execute("BEGIN IMMEDIATE")
            try:
                for field in missing_columns:
                    conn.execute(f"ALTER TABLE sources ADD COLUMN {field} TEXT")
                for source_id in db_source_ids:
                    metadata = sources.get(source_id)
                    if not metadata:
                        continue
                    conn.execute(
                        """
                        UPDATE sources
                        SET source_url = ?,
                            license = ?,
                            license_url = ?,
                            license_source_url = ?,
                            attribution = ?,
                            usage_boundary = ?
                        WHERE source_id = ?
                        """,
                        [
                            metadata.get("source_url"),
                            metadata.get("license"),
                            metadata.get("license_url"),
                            metadata.get("license_source_url"),
                            metadata.get("attribution"),
                            metadata.get("usage_boundary"),
                            source_id,
                        ],
                    )
                    updated_sources.append(source_id)
                conn.commit()
            except Exception:
                conn.rollback()
                raise
            existing_columns_after = table_columns(conn, "sources")
            if source_ids_for_summary:
                missing_values_after = missing_value_summary(
                    conn,
                    source_ids_for_summary,
                    existing_columns_after,
                )
            runtime_counts_after = runtime_counts(conn)
        finally:
            conn.close()
        db_sha_after = sha256_file(db_path)
    elif db_path.is_file() and sources:
        conn = sqlite3.connect(str(db_path), timeout=30)
        conn.row_factory = sqlite3.Row
        try:
            runtime_counts_after = runtime_counts(conn)
            if source_ids_for_summary:
                missing_values_after = missing_value_summary(
                    conn,
                    source_ids_for_summary,
                    existing_columns,
                )
        finally:
            conn.close()
        db_sha_after = db_sha_before

runtime_counts_preserved = runtime_counts_before == runtime_counts_after
if apply and not runtime_counts_preserved:
    errors.append("runtime_counts_changed")
if apply and not backup_created:
    errors.append("backup_not_created")
if apply and any(value not in (0, "column_missing") for value in missing_values_after.values()):
    errors.append("source_metadata_missing_after_apply")
if apply and any(value == "column_missing" for value in missing_values_after.values()):
    errors.append("source_metadata_columns_missing_after_apply")

payload = {
    "object": "tonglingyu.kb_source_metadata_backfill",
    "schema_version": 1,
    "status": "ok" if not errors else "failed",
    "run_id": run_id,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "applied": apply,
    "db": {
        "path": str(db_path),
        "path_sha256": hashlib.sha256(str(db_path).encode("utf-8")).hexdigest(),
        "sha256_before": db_sha_before,
        "sha256_after": db_sha_after,
    },
    "backup": {
        "path": str(backup_path) if apply else "",
        "path_sha256": hashlib.sha256(str(backup_path).encode("utf-8")).hexdigest()
        if apply
        else "",
        "sha256": backup_sha,
        "created": backup_created,
    },
    "source_root": {
        "path": str(source_root),
        "path_sha256": hashlib.sha256(str(source_root).encode("utf-8")).hexdigest(),
        "metadata_source_count": len(sources),
        "db_source_count": len(db_source_ids),
    },
    "checks": {
        "additive_only": True,
        "runtime_counts_preserved": runtime_counts_preserved,
        "backup_created_before_apply": backup_created if apply else None,
        "source_metadata_complete": not metadata_errors,
        "db_sources_have_metadata": not [
            source_id for source_id in db_source_ids if source_id not in sources
        ],
        "secret_values_printed": False,
    },
    "migration": {
        "missing_columns_before": missing_columns,
        "metadata_fields": metadata_fields,
        "missing_values_before": missing_values_before,
        "missing_values_after": missing_values_after,
        "updated_source_count": len(updated_sources),
    },
    "runtime_counts": {
        "before": runtime_counts_before,
        "after": runtime_counts_after,
    },
    "errors": errors,
    "secret_values_printed": False,
}
encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True)
if report_path is not None:
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(encoded + "\n", encoding="utf-8")
print(encoded)
raise SystemExit(0 if not errors else 1)
PY
