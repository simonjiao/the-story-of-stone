# Tonglingyu Local Stack

This directory keeps only the source repo's local compose boundary for one
Tonglingyu environment. Custom home deployment configuration, release evidence,
remote operations, and production validation live in the sibling
`../tonglingyu-gatekeeper/deploy/` repository.

## What Stays Here

- `docker-compose.yml`: local stack definition for Hermes, Tonglingyu Gateway,
  Open WebUI, and Cloudflare Tunnel.
- `scripts/start-local-stack.sh`: local start/rebuild wrapper.
- `scripts/lib/`: shared layout and env-file loading helpers used by the local
  wrapper.

Open WebUI Functions are formal source code under `../open-webui/functions/`.

## Start Locally

Use a local `deploy/.env`, or point at the gatekeeper environment file:

```bash
TONGLINGYU_DEPLOY_ENV_FILE=/Users/simon/huixiangdou/tonglingyu-gatekeeper/deploy/.env \
  ./scripts/start-local-stack.sh
```

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

## Versioning

Version management remains source-owned and is run from the repo root:

```bash
cd ..
uv run --no-sync python scripts/version.py check
uv run --no-sync python scripts/version.py bump patch
```

Run source QA before committing code or version changes:

```bash
scripts/qa.sh --quick
```

For custom environment validation and release runbooks, use:

```text
../tonglingyu-gatekeeper/deploy/
```
