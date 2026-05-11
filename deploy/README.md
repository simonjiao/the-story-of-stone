# Hermes Home Deployment

This directory deploys Hermes Agent behind Open WebUI through Cloudflare Tunnel.

Public endpoint:

```text
https://chat.huixiangdou.top
```

Services:

- `hermes`: Hermes Agent API server, internal Docker network only.
- `tonglingyu-gateway`: Rust OpenAI-compatible “通灵玉” gateway. It owns the
  HTTP/auth/rate-limit/model surface and calls `tonglingyu-runtime` for
  source-snapshot, evidence package, reviewer, replay, audit, and stats work.
  Hermes remains the upstream generation layer when configured.
- `agent-manager`: Agent Platform control plane, internal Docker network only.
- `agent-orchestrator`: internal Agent Platform gateway for control-plane
  workflows; ordinary 通灵玉 chat enters through `tonglingyu-gateway`.
- `agent-worker`: Agent run worker.
- `agent-observer`: read-only Observer Agent report loop.
- `open-webui`: email/password login and chat UI. It connects directly to
  `tonglingyu-gateway` and `agent-orchestrator` as separate OpenAI-compatible
  connections.
- `cloudflared`: Cloudflare Tunnel connector.
- `agent-platform-postgres`: dedicated Agent Platform database, internal
  Docker network only.

Open WebUI starts in offline mode to avoid blocking first boot on Hugging Face
embedding-model downloads. Chat works normally through Hermes. Enable and
preload RAG/Whisper models later if document upload, RAG, or local speech
features are needed.

Before starting, edit `.env`:

```bash
nano .env
```

Required changes:

- `CLOUDFLARED_TOKEN`: paste the token from Cloudflare Zero Trust.
- `HERMES_API_KEY`: replace with a long random value.
- `OPEN_WEBUI_SECRET_KEY`: long random value used by Open WebUI to sign
  browser/API sessions. Keep it stable across container rebuilds.
- `AGENT_PLATFORM_POSTGRES_PASSWORD`: generated locally for the dedicated Agent
  Platform database; do not reuse `sub2api-postgres`.
- `AGENT_PLATFORM_DATABASE_URL`: Postgres URL for Agent Platform services,
  using the dedicated Agent Platform database credentials.
- `AGENT_PLATFORM_BUILD_CONTEXT`: set to `./agent-platform` on the remote
  deploy node when `agent-platform/` is copied next to `docker-compose.yml`.
  The local default is `../agent-platform` when running from this `deploy/`
  directory.
- `TONGLINGYU_GATEWAY_BUILD_CONTEXT`: build context for the standalone
  Tonglingyu Gateway image. Set to `./agent-platform` on the remote deploy node.
  The local default is `../agent-platform`.
- `TONGLINGYU_GATEWAY_IMAGE_TAG`: standalone gateway image tag. Default is
  `formal`.
- `OPEN_WEBUI_OPENAI_API_BASE_URLS`: optional semicolon-separated
  OpenAI-compatible endpoints for Open WebUI. The compose default is
  `http://tonglingyu-gateway:8090/v1;http://agent-orchestrator:8080/v1`.
- `OPEN_WEBUI_OPENAI_API_KEYS`: optional semicolon-separated provider keys for
  those Open WebUI connections. Leave empty for the default internal
  `tonglingyu-gateway` and `agent-orchestrator` connections. It may contain the
  Gateway service key, but must never contain `TONGLINGYU_ADMIN_API_KEY`.
- `TONGLINGYU_SOURCE_ROOT`: host path for the checked-in Wikisource source
  snapshots. The local default is `../resources/sources/wiki` when running from
  this `deploy/` directory.
- `TONGLINGYU_DATA_DIR`: persistent data directory for the generated SQLite/FTS
  knowledge base. On the remote node it should live under
  `$HOME/huixiangdou-home-runtime/data/tonglingyu`.
- `TONGLINGYU_MODEL_ID`: Open WebUI-visible model id. Default is `tonglingyu`.
- `TONGLINGYU_GATEWAY_API_KEY`: service credential used by Open WebUI to call
  `tonglingyu-gateway`. Keep it in `.env`; do not write it into compose or logs.
- `TONGLINGYU_GATEWAY_API_KEYS`: optional comma-separated old/new gateway keys
  during rotation.
- `TONGLINGYU_ADMIN_API_KEY`: separate administrator credential for
  `/v1/admin/*`; it must not overlap with any `TONGLINGYU_GATEWAY_API_KEY(S)`
  value.
- `TONGLINGYU_ADMIN_API_KEYS`: optional comma-separated old/new admin keys during
  rotation.
- `TONGLINGYU_ALLOW_ADMIN_WITH_GATEWAY_KEY`: defaults to `false`; keep it false
  outside local development so admin endpoints cannot be opened with the normal
  Open WebUI provider key. Production verification rejects enabling this flag
  when admin keys are configured.
- `TONGLINGYU_GATEWAY_ADMIN_BASE_URL`: internal Gateway origin used by the
  Open WebUI `tonglingyu_gateway_admin` Action. Default is
  `http://tonglingyu-gateway:8090`.
- `TONGLINGYU_GATEWAY_ADMIN_ACTION_TARGET_MODEL` and
  `TONGLINGYU_GATEWAY_ADMIN_ACTION_TARGET_MODELS`: Open WebUI model ids where
  the admin Action is allowed to run. Defaults to `TONGLINGYU_MODEL_ID`.
- `TONGLINGYU_AGENT_RUNTIME_MODE`: Tonglingyu Gateway profile execution client.
  The production compose default is `hermes`; use `minimal` only for local
  smoke or deterministic development runs.
- `AGENT_RUNTIME_HERMES_BASE_URL`, `AGENT_RUNTIME_HERMES_MODEL`,
  `AGENT_RUNTIME_HERMES_PROFILE_MODELS`, `AGENT_RUNTIME_HERMES_API_KEY`, and
  `AGENT_RUNTIME_TIMEOUT_SECONDS`: Hermes runtime client settings used by
  Tonglingyu Gateway and Agent Worker. The Gateway container receives the same
  Hermes key as its upstream generation key, but it still keeps separate
  Gateway/admin inbound credentials.
- `TONGLINGYU_RETENTION_DAYS`: runtime audit/session/package retention window.
  Default is `90`; set `0` only when automatic pruning must be disabled.
- `AGENT_BRIDGE_SECRET`: shared secret used by the Open WebUI
  `agent_identity_bridge` Filter and Agent Platform services.
- `AGENT_JWT_SECRET`: Manager JWT signing secret shared by `agent-manager` and
  `agent-orchestrator`.
- `AGENT_PLATFORM_ALLOW_DEV_HEADERS`: set to `false` after Bridge smoke passes
  so Open WebUI control requests cannot fall back to default dev identity.
- `AGENT_BRIDGE_OBSERVER_ADMIN_ROLE_MAPPING`: maps a signed Open WebUI admin
  subject to a read-only Agent Platform role for System Observer status
  sessions. Default `operator` allows report discussion without granting agent
  admin control.
- `AGENT_RUNTIME_MODE`: `hermes` for the P1 worker path, or `minimal` for the
  local P0 runtime. The formal compose default is `hermes`.
- `AGENT_RUNTIME_HERMES_BASE_URL`: internal OpenAI-compatible Hermes URL used
  by `agent-worker`, default `http://hermes:8642/v1`.
- `AGENT_RUNTIME_HERMES_MODEL`: model name sent to Hermes, default
  `hermes-agent`.
- `AGENT_RUNTIME_HERMES_PROFILE_MODELS`: optional profile-to-model override map
  for `agent.hermes_profile`, either JSON such as
  `{"background_worker:analysis":"hermes-agent"}` or comma syntax such as
  `background_worker:analysis=hermes-agent`.
- `AGENT_RUNTIME_TIMEOUT_SECONDS`: timeout for P1 Runtime calls.
- Optional read-only connector values for P1 snapshot collection:
  - `AGENT_READ_ONLY_CONNECTOR_BASE_URL`, serving `GET /snapshots`
  - `AGENT_READ_ONLY_CONNECTOR_API_KEY`, if that read-only connector requires
    auth. Leave both empty to use the built-in local read-only snapshot adapter.
- Optional controlled external action values. Leave these empty unless the
  target environment has a credential provider and write connector ready. For
  the repository-provided low-risk action journal target, enable the
  `action-gateway-smoke` compose profile and set both base URLs to
  `http://agent-action-gateway:8091`:
  - `AGENT_CREDENTIAL_PROVIDER_BASE_URL`, serving `POST /credential-leases`
    and returning only an opaque `provider_ref`.
  - `AGENT_CREDENTIAL_PROVIDER_API_KEY`, if the credential provider requires
    auth.
  - `AGENT_CREDENTIAL_PROVIDER_TIMEOUT_SECONDS` and
    `AGENT_CREDENTIAL_LEASE_TTL_SECONDS`.
  - `AGENT_WRITE_CONNECTOR_BASE_URL`, serving `POST /action-executions/execute`
    and `POST /action-executions/compensate`. Successful execute responses
    must include `status=applied`, `result_ref`, and `compensation_ref`.
    Successful compensate responses must include `status=compensated` and
    `result_ref`.
  - `AGENT_WRITE_CONNECTOR_API_KEY`, if the write connector requires auth.
  - `AGENT_WRITE_CONNECTOR_TIMEOUT_SECONDS`,
    `AGENT_WRITE_CONNECTOR_MAX_ATTEMPTS`, and
    `AGENT_EXTERNAL_ACTION_LOCK_LEASE_SECONDS`.
  - `AGENT_ACTION_GATEWAY_TARGET_LOG`,
    `AGENT_ACTION_GATEWAY_API_KEY`,
    `AGENT_ACTION_GATEWAY_ALLOWED_SCOPES`,
    `AGENT_ACTION_GATEWAY_CONNECTOR`, and
    `AGENT_ACTION_GATEWAY_LEASE_TTL_SECONDS` configure the optional
    `agent-action-gateway` service. Keep API keys in `.env`; do not put
    them in compose or logs.
  - `agent-platform/scripts/action-gateway-smoke.sh` runs a local Manager plus
    the action journal target and verifies approval, dry-run, apply, target
    write, result_ref, compensation_ref, and compensation.
  - `agent-platform/scripts/external-action-contract-smoke.sh` runs the same
    Manager workflow against any configured third-party provider/connector.
    Set `EXTERNAL_ACTION_CONNECTOR`, `EXTERNAL_ACTION_NAME`,
    `EXTERNAL_ACTION_RESOURCE_REF`, and `EXTERNAL_ACTION_CREDENTIAL_SCOPE`;
    keep provider/connector URLs and API keys in `.env`.
- Optional internal IP overrides: `HERMES_AGENT_IP`, `OPEN_WEBUI_ORIGIN_IP`,
  `AGENT_PLATFORM_POSTGRES_IP`, `AGENT_MANAGER_IP`,
  `AGENT_ORCHESTRATOR_IP`, `AGENT_WORKER_IP`, and `AGENT_OBSERVER_IP`.
  Defaults reserve stable addresses on `HERMES_INTERNAL_SUBNET` so Docker
  restart order cannot steal the Open WebUI origin IP used by Cloudflare
  Tunnel.
- local model endpoint values if Hermes should call a local OpenAI-compatible container:
  - `LOCAL_OPENAI_BASE_URL`, for example `http://vllm:8000/v1`
  - `LOCAL_OPENAI_MODEL`, matching the model name served by that container
  - `LOCAL_OPENAI_API_KEY`, usually `none` for local inference servers
  - `LOCAL_OPENAI_DOCKER_NETWORK`, for example `sub2api_sub2api-network`

Runtime data is separate from deploy files. On the remote node, keep all
runtime state under `$HOME/huixiangdou-home-runtime/data`; the deploy
directory `$HOME/hermes-home-deploy` should contain only compose, scripts,
source/build context, and Open WebUI Function files.

Create persistent directories:

```bash
mkdir -p \
  "$HOME/huixiangdou-home-runtime/data/hermes" \
  "$HOME/huixiangdou-home-runtime/data/open-webui" \
  "$HOME/huixiangdou-home-runtime/data/tonglingyu" \
  "$HOME/huixiangdou-home-runtime/data/agent-platform-postgres" \
  "$HOME/huixiangdou-home-runtime/data/agent-action-gateway"
```

If an older remote deploy still has `$HOME/hermes-home-deploy/data`, move
it once before restarting:

```bash
./scripts/migrate-runtime-data.sh
```

Verify the rendered formal runtime configuration before starting or restarting
the production stack:

```bash
./scripts/verify-tonglingyu-runtime-config.sh
```

This gate checks the compose-rendered service environment for strict
Tonglingyu/Hermes runtime wiring, `DEFAULT_MODELS=tonglingyu`, Gateway/admin key
set isolation, and Open WebUI provider keys that do not contain admin
credentials. It prints variable names and gate status only; it must not print
secret values.

After the stack is running, verify the live Gateway surface from inside the
formal Docker network:

```bash
./scripts/verify-tonglingyu-strict-gateway.sh
```

This runtime gate checks `/healthz`, `/v1/models`, `/v1/admin/metrics`,
Prometheus metrics, minimal non-streaming and streaming live chat completions,
and the resulting admin trace. It requires `agent_runtime_mode=hermes`, a single
visible `tonglingyu` model, hidden `honglou-*` profiles, positive KB counts,
active rate limiting, isolated admin credentials, public chat responses that do
not expose internal runtime/admin trace fields at any nesting level, streaming
responses with `[DONE]`, package metadata, and Runtime workflow source markers,
the streaming response's own admin trace with Hermes Runtime summary/audit
coverage, and Hermes runtime profile steps with non-empty tool results bound to
`runtime://tonglingyu/{trace_id}/...` output refs in the trace.
Evidence search tool refs must use the
`runtime://tonglingyu/{trace_id}/evidence/{digest}` namespace, while package
tool refs must match the current evidence package id. The same trace must also
show local evidence/package/reviewer enforcement and a consumed Hermes draft
observation, plus an `agent_runtime_profile_execution_summarized` event whose
summary reports `hermes_profile_observed_with_local_governance`, so the gate
does not pass on tool-result plumbing alone. The summary step/tool counts must
also match the detailed runtime step audit events, and every reported tool
result must be covered by a matching `runtime_tool_result` audit event with the
same tool name and output ref. The trace-level `agent_runtime_summary` must
match the latest runtime summary audit event, so the admin trace surface cannot
drift from the audit chain.

For a release-readiness summary, run the aggregate gate:

```bash
TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
  TONGLINGYU_RELEASE_REPORT_PATH=./tonglingyu-release-readiness.json \
  ./scripts/verify-tonglingyu-release-readiness.sh
```

Without `TONGLINGYU_RELEASE_REQUIRE_LIVE=true`, the aggregate gate runs the
compose-rendered config check and records live Gateway/Open WebUI Function and
Gateway Admin Action checks as skipped. The JSON report includes
`production_release_ready=false`, `browser_review_acknowledged`,
`optional_failures`, `skipped_live_gates`, and `release_blockers` whenever live
mode, live gates, or browser review are missing, so partial local verification
cannot be mistaken for a production release pass. Optional failed gates are
reflected in `status` as
`passed_with_failed_optional_gates`. By default the script exits non-zero unless
`production_release_ready=true`; set
`TONGLINGYU_RELEASE_SUMMARY_ONLY=true` only when intentionally generating a
non-release summary report.
The aggregate gate can be contract-tested without a live deployment:

```bash
./scripts/test-tonglingyu-release-readiness-contract.sh
```

That test uses explicit mock gate command overrides. The production aggregate
script rejects overrides unless `TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true`
is set, and any report generated with overrides keeps
`production_release_ready=false` with `status=passed_with_gate_command_overrides`
when the mocked release conditions otherwise pass; use the reported
`release_conditions_met` field only to verify local aggregation semantics.
When `TONGLINGYU_RELEASE_REQUIRE_LIVE=true`, the aggregate gate also requires
`TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true` and a non-empty
`TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF` after a human has checked
ordinary-user model visibility, streaming chat UX, admin audit visibility, and
that persisted Open WebUI provider settings match the rendered environment. Use
the ref for the review note, ticket, runbook entry, or captured report path.

Agent Platform uses its own Postgres container. Do not reuse
`sub2api-postgres`; it belongs to the separate `sub2api` compose project and
has its own lifecycle, data directory, and schema ownership.

Run Hermes setup once if the Hermes data directory is fresh:

```bash
docker compose run --rm hermes setup
```

If Hermes should use a local OpenAI-compatible container as its model provider,
render the Hermes `config.yaml` after editing `.env`:

```bash
./scripts/render-hermes-config.sh
```

The render script backs up an existing Hermes config before overwriting it. By
default it disables Hermes persistent memory for this Open WebUI deployment so
new Open WebUI chats do not inherit previous chat-specific facts:

```yaml
memory:
  memory_enabled: false
  user_profile_enabled: false
```

If cross-session Hermes memory is intentionally desired, set these in `.env`
before rendering:

```bash
HERMES_MEMORY_ENABLED=true
HERMES_USER_PROFILE_ENABLED=true
```

The model-serving container must be on the same Docker network as Hermes. This
compose file attaches Hermes to `LOCAL_OPENAI_DOCKER_NETWORK`.

Hermes config should then use the container name, not `localhost`:

```yaml
model:
  provider: custom
  model: your-served-model-name
  base_url: http://sub2api:8080/v1
api_key: none
```

## Open WebUI Model Connections

Global Router is intentionally not part of this production deploy path. It is
still tracked as a standalone design, but the current implementation is not
production-grade enough to sit between Open WebUI and the model endpoints.

Open WebUI is configured with two direct OpenAI-compatible connections:

```text
http://tonglingyu-gateway:8090/v1
http://agent-orchestrator:8080/v1
```

The expected visible models are:

- `tonglingyu`: 通灵玉 evidence and reviewer gateway.
- `hermes-agent`: Agent Platform Orchestrator. Ordinary chat is passed through
  to Hermes; Agent control/session requests require the signed Bridge context.

This direct setup is acceptable for now because Open WebUI can merge models from
multiple OpenAI-compatible connections and dispatch chat requests by the selected
model's source connection. It also removes the current Global Router MVP risks:
no router-owned inbound auth, no router RBAC, no partial Bridge validation, no
router audit gap, and no untested fallback or circuit-breaker behavior.

Operational constraints:

- Keep model ids unique across directly configured endpoints. If two endpoints
  expose the same id, Open WebUI keeps the first merged entry and routing becomes
  ambiguous.
- If a future endpoint requires inbound API auth from Open WebUI, set
  `OPEN_WEBUI_OPENAI_API_KEYS` with the same semicolon-separated entry count as
  `OPEN_WEBUI_OPENAI_API_BASE_URLS`.
- Keep `DEFAULT_MODELS=tonglingyu` unless the desired first-open experience is
  Agent Platform chat through `hermes-agent`.
- Keep the `agent_identity_bridge` Function target on `hermes-agent`; do not
  inject Agent Platform identity context into `tonglingyu` evidence chat.
- Keep the `tonglingyu_gateway_admin` Action target on `tonglingyu`; it is a
  read-only admin surface for Gateway audit and metrics, not an Agent Platform
  identity bridge.
- For a live Open WebUI with an existing `webui.db`, admin Settings →
  Connections may already persist provider settings. Verify the UI or admin API
  after changing env values.

## Open WebUI Agent Identity Bridge

The formal Open WebUI deployment uses `agent_identity_bridge` as a Filter
Function. It injects a signed `agent_bridge_context` before requests reach
`agent-orchestrator`; Orchestrator verifies the signature, signs Manager JWTs,
and maps each Open WebUI chat to a persistent Agent Platform session.

Open WebUI admin is only required to install or update the Function and its
valves. Runtime approval, audit, and management permissions stay inside Agent
Platform JWT roles. Open WebUI admin users are not mapped to Agent Platform
admin unless `AGENT_BRIDGE_ADMIN_ROLE_MAPPING=agent_admin` is explicitly set.

Install or update the Function against the formal Open WebUI only:

```bash
./scripts/install-openwebui-function.sh
./scripts/verify-openwebui-function.sh
```

The script requires these environment variables to be present in the shell or
`.env`-sourced environment:

```text
OPEN_WEBUI_BASE_URL or PUBLIC_WEBUI_URL
OPEN_WEBUI_ADMIN_TOKEN
AGENT_BRIDGE_SECRET
AGENT_BRIDGE_ISSUER
```

`AGENT_BRIDGE_TARGET_MODEL` and `AGENT_BRIDGE_TARGET_MODELS` default to
`hermes-agent`. Keep that default unless the Orchestrator model id changes or a
reviewed deployment intentionally maps multiple Open WebUI model ids into the
Bridge.

If the available Open WebUI account is not an admin and the Function API returns
401, use the formal container/DB installer instead of creating a temporary Open
WebUI:

```bash
./scripts/install-openwebui-function-db.sh
./scripts/verify-openwebui-function.sh
```

This writes the Function into the mounted Open WebUI `webui.db`, stores valves
there, and restarts only the formal `open-webui` service.

The verify script checks Function type, active/global flags, bridge content,
non-empty required valves, and target model valves. When
`OPEN_WEBUI_ADMIN_TOKEN` is unavailable, run it from the compose deploy
directory and it verifies the mounted `webui.db` inside the formal Open WebUI
container. It reports valve key names only, never secret values. For local/CI
contract checks, set `OPEN_WEBUI_FUNCTION_VERIFY_JSON` to a fixture file and the
script validates the fixture without connecting to Open WebUI.

## Open WebUI Gateway Admin Action

Install or update the read-only Gateway admin Action against the formal Open
WebUI only:

```bash
./scripts/install-openwebui-gateway-admin-action.sh
./scripts/verify-openwebui-gateway-admin-action.sh
```

The Action exposes Gateway metrics, trace, package audit, and session lookup
from Open WebUI while enforcing `__user__.role == "admin"` inside the Function
before it calls `/v1/admin/*`. The admin key is stored only in Function valves;
ordinary users receive a denial before any Gateway request is made.

The API installer requires these environment variables to be present in the
shell or `.env`-sourced environment:

```text
OPEN_WEBUI_BASE_URL or PUBLIC_WEBUI_URL
OPEN_WEBUI_ADMIN_TOKEN
TONGLINGYU_ADMIN_API_KEY
```

If the available Open WebUI account is not an admin and the Function API returns
401, use the formal container/DB installer:

```bash
./scripts/install-openwebui-gateway-admin-action-db.sh
./scripts/verify-openwebui-gateway-admin-action.sh
```

The verify script checks Function type, active/global flags, admin role guard,
Gateway admin endpoint coverage, and non-empty required valves. It reports valve
key names only, never secret values. For local/CI contract checks, set
`OPEN_WEBUI_GATEWAY_ADMIN_ACTION_VERIFY_JSON` to a fixture file and the script
validates the fixture without connecting to Open WebUI.

Run the local contract smoke before changing the Action or its verify gate:

```bash
./scripts/test-openwebui-gateway-admin-action-contract.sh
```

The smoke compiles the Action, runs its unit tests, verifies positive and
negative fixture reports, and checks that fixture-secret values are not emitted
by the verify script.

Do not print or commit `OPEN_WEBUI_ADMIN_TOKEN`, `AGENT_BRIDGE_SECRET`,
`AGENT_JWT_SECRET`, or `TONGLINGYU_ADMIN_API_KEY`. Before editing `deploy/.env`,
run:

```bash
./scripts/env-backup.sh backup
```

Test the endpoint from the same Docker network:

```bash
docker run --rm \
  --network "${LOCAL_OPENAI_DOCKER_NETWORK}" \
  curlimages/curl:latest \
  -sS -m 8 \
  -H "Authorization: Bearer ${LOCAL_OPENAI_API_KEY}" \
  "${LOCAL_OPENAI_BASE_URL}/models"
```

Build the Tonglingyu knowledge base locally before deployment smoke tests:

```bash
cargo run \
  --manifest-path ../agent-platform/Cargo.toml \
  -p tonglingyu-gateway -- \
  build-kb \
  --source-root ../resources/sources/wiki \
  --db data/tonglingyu/tonglingyu.db \
  --rebuild
```

Start the stack:

```bash
docker compose build tonglingyu-gateway
docker compose build agent-manager agent-orchestrator agent-worker agent-observer
docker compose pull
docker compose up -d
docker compose ps
```

`tonglingyu-gateway` is built from
`agent-platform/crates/tonglingyu-gateway/Dockerfile` as a standalone image. It
uses BuildKit cache mounts for Cargo registry, git sources, and `target/`, so
gateway-only code changes do not force the shared Agent Platform runtime image
to rebuild.

After re-rendering Hermes config, restart Hermes:

```bash
docker compose restart hermes
```

Check logs:

```bash
docker compose logs -f hermes
docker compose logs -f tonglingyu-gateway
docker compose logs -f agent-manager
docker compose logs -f agent-orchestrator
docker compose logs -f agent-worker
docker compose logs -f agent-observer
docker compose logs -f open-webui
docker compose logs -f cloudflared
docker compose logs -f agent-platform-postgres
```

Check the direct Open WebUI model endpoints from the internal Docker network:

```bash
docker compose exec tonglingyu-gateway curl -fsS http://127.0.0.1:8090/healthz
docker compose exec open-webui curl -fsS \
  -H "Authorization: Bearer ${TONGLINGYU_GATEWAY_API_KEY}" \
  http://tonglingyu-gateway:8090/v1/models
docker compose exec open-webui curl -fsS \
  -H "Authorization: Bearer ${TONGLINGYU_ADMIN_API_KEY}" \
  http://tonglingyu-gateway:8090/v1/admin/metrics
docker compose exec open-webui curl -fsS \
  -H "Authorization: Bearer ${TONGLINGYU_ADMIN_API_KEY}" \
  http://tonglingyu-gateway:8090/v1/admin/metrics/prometheus
docker compose exec agent-orchestrator curl -fsS http://127.0.0.1:8080/healthz
docker compose exec agent-orchestrator curl -fsS http://127.0.0.1:8080/v1/models
```

Cloudflare Tunnel public hostname should point to:

```text
chat.huixiangdou.top -> http://open-webui:8080
```

Do not expose Hermes ports `8642` or `9119` to the public internet.
Do not expose Agent Platform Postgres port `5432` to the public internet.
Do not expose Agent Platform Manager, Worker, Observer, or Runtime ports to the
public internet. The only public HTTP entrypoint remains Cloudflare Tunnel to
`open-webui:8080`.
