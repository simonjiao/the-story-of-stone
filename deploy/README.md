# Hermes Home Deployment

This directory deploys Hermes Agent behind Open WebUI through Cloudflare Tunnel.

Public endpoint:

```text
https://chat.huixiangdou.top
```

Services:

- `hermes`: Hermes Agent API server, internal Docker network only.
- `global-router`: Rust OpenAI-compatible model allowlist and routing layer.
  The current implementation is a first productionization baseline for controlled
  trial deployments, not a complete production-grade router. Open WebUI connects
  only to this service; it owns inbound auth, route-level permissions, Bridge
  HMAC validation, audit JSONL, health summaries, circuit breaking, fallback,
  route reload, and optional upstream model discovery.
- `tonglingyu-gateway`: Rust OpenAI-compatible “通灵玉” gateway. It builds the
  SQLite/FTS evidence knowledge base from source snapshots, assembles evidence
  packages, runs reviewer checks, and calls Hermes as the upstream generation
  layer when configured.
- `agent-manager`: Agent Platform control plane, internal Docker network only.
- `agent-orchestrator`: internal Agent Platform gateway for control-plane
  workflows; ordinary 通灵玉 chat enters through `tonglingyu-gateway`.
- `agent-worker`: Agent run worker.
- `agent-observer`: read-only Observer Agent report loop.
- `open-webui`: email/password login and chat UI. It connects only to
  `global-router`; visible models come from the router allowlist.
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
- `GLOBAL_ROUTER_BUILD_CONTEXT`: build context for the standalone Global Router
  image. Set to `./agent-platform` on the remote deploy node. The local default
  is `../agent-platform`.
- `GLOBAL_ROUTER_IMAGE_TAG`: standalone Global Router image tag. Default is
  `formal`.
- `GLOBAL_ROUTER_ROUTES_JSON`: optional JSON array that declares the Open WebUI
  visible model allowlist and route targets. If empty, only `tonglingyu` is
  exposed and routed to `http://tonglingyu-gateway:8090/v1`.
- `GLOBAL_ROUTER_ROUTES_FILE`: optional JSON route config file path. When set,
  `POST /v1/router/reload` reloads routes from this file.
- `GLOBAL_ROUTER_INBOUND_API_KEYS`: optional comma-separated Bearer tokens
  accepted from Open WebUI. Leave empty only for trusted internal networks or
  local development.
- `GLOBAL_ROUTER_ADMIN_API_KEY`: Bearer token required for
  `/v1/router/health`, `/v1/router/routes`, and `/v1/router/reload`.
- `GLOBAL_ROUTER_DATA_DIR`: persistent directory for router audit records.
- `GLOBAL_ROUTER_AUDIT_LOG_PATH`: JSONL audit path inside the container. Default
  is `/data/audit.jsonl`.
- `GLOBAL_ROUTER_IP`: optional stable internal IP for `global-router`.
- `TONGLINGYU_SOURCE_ROOT`: host path for the checked-in Wikisource source
  snapshots. The local default is `../resources/sources/wiki` when running from
  this `deploy/` directory.
- `TONGLINGYU_DATA_DIR`: persistent data directory for the generated SQLite/FTS
  knowledge base. The local default is `./data/tonglingyu`.
- `TONGLINGYU_MODEL_ID`: Open WebUI-visible model id. Default is `tonglingyu`.
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

Create persistent directories:

```bash
mkdir -p data/hermes data/open-webui data/agent-platform-postgres
```

Agent Platform uses its own Postgres container. Do not reuse
`sub2api-postgres`; it belongs to the separate `sub2api` compose project and
has its own lifecycle, data directory, and schema ownership.

Run Hermes setup once if the Hermes data directory is fresh:

```bash
docker compose run --rm hermes setup
```

If Hermes should use a local OpenAI-compatible container as its model provider,
render `data/hermes/config.yaml` after editing `.env`:

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

If the available Open WebUI account is not an admin and the Function API returns
401, use the formal container/DB installer instead of creating a temporary Open
WebUI:

```bash
./scripts/install-openwebui-function-db.sh
./scripts/verify-openwebui-function.sh
```

This writes the Function into the mounted Open WebUI `webui.db`, stores valves
there, and restarts only the formal `open-webui` service.

The verify script checks Function type, active/global flags, bridge content, and
valve key presence. When `OPEN_WEBUI_ADMIN_TOKEN` is unavailable, run it from the
compose deploy directory and it verifies the mounted `webui.db` inside the formal
Open WebUI container. It reports valve key names only, never secret values.

Do not print or commit `OPEN_WEBUI_ADMIN_TOKEN`, `AGENT_BRIDGE_SECRET`, or
`AGENT_JWT_SECRET`. Before editing `deploy/.env`, run:

```bash
./scripts/env-backup.sh backup
```

Test the endpoint from the same Docker network:

```bash
docker run --rm --network "${LOCAL_OPENAI_DOCKER_NETWORK}" curlimages/curl:latest \
  -sS -m 8 \
  -H "Authorization: Bearer ${LOCAL_OPENAI_API_KEY}" \
  "${LOCAL_OPENAI_BASE_URL}/models"
```

Build the Tonglingyu knowledge base locally before deployment smoke tests:

```bash
cargo run --manifest-path ../agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  build-kb \
  --source-root ../resources/sources/wiki \
  --db data/tonglingyu/tonglingyu.db \
  --rebuild
```

Start the stack:

```bash
docker compose build global-router
docker compose build tonglingyu-gateway
docker compose build agent-manager agent-orchestrator agent-worker agent-observer
docker compose pull
docker compose up -d
docker compose ps
```

`global-router` is built from `agent-platform/crates/global-router/Dockerfile`
as a standalone image. It exposes configured allowlist models, rewrites visible
model ids to backend model ids before forwarding, applies per-route timeout,
validates inbound router auth when configured, validates signed
`agent_bridge_context` for bridge routes, writes audit JSONL when configured,
normalizes router/upstream errors, tracks per-route health, opens a circuit after
repeated 5xx/transport failures, and can use a configured fallback route. Example
route config:

```json
[
  {
    "model": "tonglingyu",
    "name": "通灵玉",
    "base_url": "http://tonglingyu-gateway:8090/v1",
    "upstream_model": "tonglingyu",
    "requires_bridge": false,
    "timeout_seconds": 120,
    "failure_threshold": 3,
    "circuit_breaker_seconds": 30
  },
  {
    "model": "other/default",
    "name": "Other Gateway",
    "base_url": "http://other-gateway:8090/v1",
    "upstream_model": "default",
    "requires_bridge": true,
    "api_key_env": "OTHER_GATEWAY_API_KEY",
    "timeout_seconds": 120,
    "allowed_user_roles": ["admin"],
    "failure_threshold": 3,
    "circuit_breaker_seconds": 30,
    "fallback_model": "tonglingyu"
  },
  {
    "model": "other",
    "name": "Other Gateway",
    "base_url": "http://other-gateway:8090/v1",
    "requires_bridge": true,
    "api_key_env": "OTHER_GATEWAY_API_KEY",
    "discover_models": true,
    "allowed_user_roles": ["admin"]
  }
]
```

Use namespaced visible model ids such as `other/default` to avoid collisions.
Set the Open WebUI `agent_identity_bridge` Function valve `TARGET_MODELS` to
the comma-separated subset that requires identity context, for example
`other/default` or a discovery namespace such as `other`. For
`requires_bridge=true`, the router verifies the Bridge HMAC using
`AGENT_BRIDGE_SECRET`, issuer, model, nonce, timestamp, subject, chat, session,
and message fields, rejects in-process nonce replay, then strips the bridge
context before forwarding.

If `discover_models=true`, `/v1/models` calls the upstream `/models` endpoint and
exposes returned ids under the configured namespace, for example
`other/default`. Requests to `other/default` then forward as upstream
`model=default`.

Fallback routes are only used when the fallback route itself is not circuit-open
and the trusted identity, when present, is also allowed by the fallback route.

Admin checks:

```bash
docker compose exec global-router curl -fsS \
  -H "Authorization: Bearer ${GLOBAL_ROUTER_ADMIN_API_KEY}" \
  http://127.0.0.1:8099/v1/router/health

docker compose exec global-router curl -fsS -X POST \
  -H "Authorization: Bearer ${GLOBAL_ROUTER_ADMIN_API_KEY}" \
  http://127.0.0.1:8099/v1/router/reload
```

The standalone design record lives in `../docs/global-router-design/`; progress
lives in `../docs/global-router-design/PROGRESS.md`.

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

Check the Tonglingyu Gateway from the internal Docker network:

```bash
docker compose exec global-router curl -fsS http://127.0.0.1:8099/healthz
docker compose exec global-router curl -fsS http://127.0.0.1:8099/v1/models
docker compose exec tonglingyu-gateway curl -fsS http://127.0.0.1:8090/healthz
docker compose exec tonglingyu-gateway curl -fsS http://127.0.0.1:8090/v1/models
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
