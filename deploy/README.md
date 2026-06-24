# Tonglingyu Local Stack

This directory keeps only the source repo's local compose boundary for one
Tonglingyu environment. Custom home deployment configuration, release evidence,
remote operations, and production validation are intentionally outside this
source repository.

## What Stays Here

- `docker-compose.yml`: local stack definition for Tonglingyu Gateway,
  Open WebUI, and Cloudflare Tunnel.
- `scripts/start-local-stack.sh`: local start/rebuild wrapper.
- `scripts/lib/`: shared layout and env-file loading helpers used by the local
  wrapper.

Open WebUI Functions are formal source code under `../open-webui/functions/`.

## Start Locally

Use a local `deploy/.env`, or point at an external deploy environment file:

```bash
TONGLINGYU_DEPLOY_ENV_FILE=/path/to/deploy/.env \
  ./scripts/start-local-stack.sh
```

Runtime data directories must be provided by env (`OPEN_WEBUI_DATA_DIR`,
`TONGLINGYU_DATA_DIR`). Do not keep runtime artifacts under this source repo's
`deploy/`.

Useful options:

```bash
./scripts/start-local-stack.sh --no-build
./scripts/start-local-stack.sh --pull
./scripts/start-local-stack.sh --foreground
./scripts/start-local-stack.sh --version
```

The local wrapper reads the current project version from `../VERSION`, exports
it as `TONGLINGYU_VERSION`, builds `tonglingyu-gateway`, and starts the compose
stack. It does not bump versions, create release evidence, or run production
gates.
The compose default image tag is `latest` by design, so a missing
`TONGLINGYU_VERSION` cannot look like a numeric release version.

## Versioning

Version management remains source-owned and is run from the repo root. First
check synchronization:

```bash
cd ..
uv run --no-sync python scripts/version.py check
```

For a small feature or bugfix:

```bash
uv run --no-sync python scripts/version.py bump patch
```

For a large feature or refactor:

```bash
uv run --no-sync python scripts/version.py bump minor
```

Run source QA before committing code or version changes:

```bash
scripts/qa.sh --quick
```

Custom environment validation and release runbooks are maintained outside this
source repository.
