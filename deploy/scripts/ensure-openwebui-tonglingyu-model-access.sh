#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_deploy_env_file_or_local

COMPOSE_SERVICE="${OPEN_WEBUI_COMPOSE_SERVICE:-open-webui}"
MODEL_ID="${TONGLINGYU_MODEL_ID:-tonglingyu}"
MODEL_NAME="${TONGLINGYU_MODEL_NAME:-通灵玉}"

if [[ -z "${MODEL_ID// }" ]]; then
  echo "TONGLINGYU_MODEL_ID must not be empty" >&2
  exit 1
fi

docker compose exec -T \
  -e TLY_MODEL_ID="${MODEL_ID}" \
  -e TLY_MODEL_NAME="${MODEL_NAME}" \
  "${COMPOSE_SERVICE}" \
  python3 - <<'PY'
import json
import os
import sqlite3
import time
import uuid

model_id = os.environ["TLY_MODEL_ID"].strip()
model_name = os.environ.get("TLY_MODEL_NAME", model_id).strip() or model_id
now = int(time.time())

conn = sqlite3.connect("/app/backend/data/webui.db")
conn.row_factory = sqlite3.Row
try:
    model = conn.execute(
        "select id, name, is_active from model where id = ?",
        (model_id,),
    ).fetchone()
    created_model = False
    if model is None:
        conn.execute(
            """
            insert into model(id, user_id, base_model_id, name, meta, params, created_at, updated_at, is_active)
            values(?,?,?,?,?,?,?,?,?)
            """,
            (
                model_id,
                None,
                None,
                model_name,
                json.dumps({"profile_image_url": "/static/favicon.png"}, ensure_ascii=False),
                "{}",
                now,
                now,
                1,
            ),
        )
        created_model = True
    elif not bool(model["is_active"]):
        conn.execute(
            "update model set is_active = 1, updated_at = ? where id = ?",
            (now, model_id),
        )

    grant = conn.execute(
        """
        select id from access_grant
        where resource_type = 'model'
          and resource_id = ?
          and principal_type = 'user'
          and principal_id = '*'
          and permission = 'read'
        """,
        (model_id,),
    ).fetchone()
    created_grant = False
    if grant is None:
        conn.execute(
            """
            insert into access_grant(id, resource_type, resource_id, principal_type, principal_id, permission, created_at)
            values(?,?,?,?,?,?,?)
            """,
            (
                str(uuid.uuid4()),
                "model",
                model_id,
                "user",
                "*",
                "read",
                now,
            ),
        )
        created_grant = True

    public_read_grants = conn.execute(
        """
        select count(*) from access_grant
        where resource_type = 'model'
          and resource_id = ?
          and principal_type = 'user'
          and principal_id = '*'
          and permission = 'read'
        """,
        (model_id,),
    ).fetchone()[0]
    conn.commit()
finally:
    conn.close()

payload = {
    "object": "tonglingyu.openwebui_model_access_ensure",
    "schema_version": 1,
    "status": "ok" if public_read_grants == 1 else "failed",
    "model_id": model_id,
    "model_created": created_model,
    "public_read_grant_created": created_grant,
    "public_read_grant_count": public_read_grants,
    "secret_values_printed": False,
}
print(json.dumps(payload, ensure_ascii=False, sort_keys=True))
if public_read_grants != 1:
    raise SystemExit(1)
PY
