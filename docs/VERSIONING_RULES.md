# Versioning Rules

The project version source of truth is the repository-root `VERSION` file.
The initial version is `0.1.0`.

## Format

- Use numeric `MAJOR.MINOR.PATCH`, for example `0.1.0`.
- Do not add `v` prefixes, release suffixes, build metadata, or leading zeroes.
- Real deployments bump only `PATCH`. Every deploy must increase `PATCH` by
  exactly `1`.

## Managed Surfaces

Keep these surfaces synchronized with `VERSION`:

- Rust workspace: `agent-platform/Cargo.toml` owns
  `[workspace.package].version`; crates use `version.workspace = true`.
- Python tooling: `pyproject.toml` and `uv.lock`; do not reintroduce
  `requirements.txt` as the dependency source of truth.
- Containers: `tonglingyu-gateway` Dockerfile `APP_VERSION`, OCI labels, and
  Compose image/build defaults.
- Scripts and tests: versioned release/QA scripts expose `--version`, and the
  version-management tests carry the same expected project version.

## Commands

Check version synchronization:

```bash
uv run --no-sync python scripts/version.py check
```

Set a specific version only when initializing or repairing drift:

```bash
uv run --no-sync python scripts/version.py set 0.1.0
uv lock
cargo metadata --manifest-path agent-platform/Cargo.toml --format-version 1 >/dev/null
```

Before every real deploy, use the deploy bump wrapper so `PATCH` increments and
the Rust/Python lockfiles are refreshed:

```bash
deploy/scripts/bump-deploy-version.sh
```

For a local one-command deploy, use:

```bash
deploy/scripts/deploy-versioned-stack.sh
```

Run the project QA entrypoint before committing release or version changes:

```bash
scripts/qa.sh --quick
```

Use `scripts/qa.sh --full` for release-grade local gates when Docker and the
Rust toolchain are available.

## Deploy Notes

- Do not manually edit `deploy/.env` for version bumps.
- Keep secrets in `.env`; version scripts must not print token, key, or password
  values.
- `TONGLINGYU_GATEWAY_IMAGE_REF` may still override the local versioned default
  with a digest-pinned production image. The release record must still show the
  deploy version that produced that image.
