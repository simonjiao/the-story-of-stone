# Hermes Home Deployment

This directory deploys Hermes Agent behind Open WebUI through Cloudflare Tunnel.

Public endpoint:

```text
https://chat.huixiangdou.top
```

Services:

- `hermes`: Hermes Agent API server, internal Docker network only.
- `global-router`: Rust OpenAI-compatible model allowlist and routing gateway.
  Open WebUI connects only to this service.
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
```

This writes the Function into the mounted Open WebUI `webui.db`, stores valves
there, and restarts only the formal `open-webui` service.

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
as a standalone image. It only exposes configured allowlist models and rewrites
visible model ids to backend model ids before forwarding. Example route config:

```json
[
  {
    "model": "tonglingyu",
    "name": "通灵玉",
    "base_url": "http://tonglingyu-gateway:8090/v1",
    "upstream_model": "tonglingyu",
    "requires_bridge": false
  },
  {
    "model": "other/default",
    "name": "Other Gateway",
    "base_url": "http://other-gateway:8090/v1",
    "upstream_model": "default",
    "requires_bridge": true,
    "api_key_env": "OTHER_GATEWAY_API_KEY"
  }
]
```

Use namespaced visible model ids such as `other/default` to avoid collisions.
Set the Open WebUI `agent_identity_bridge` Function valve `TARGET_MODELS` to
the comma-separated subset that requires identity context, for example
`other/default`.

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
