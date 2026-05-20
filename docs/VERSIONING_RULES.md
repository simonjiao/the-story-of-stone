# Versioning Rules

The project version source of truth is the repository-root `VERSION` file.
The initial version is `0.1.0`.

## Format

- Use numeric `MAJOR.MINOR.PATCH`, for example `0.1.0`.
- Do not add `v` prefixes, release suffixes, build metadata, or leading zeroes.
- Keep `MAJOR` manual while the project is in `0.x`. Use `set` for a reviewed
  first-position change only when the public product contract is being reset.

## Bump Policy

Use one explicit source-owned version bump for changes that affect build,
runtime, local compose behavior, deployable assets, or release evidence.

- `PATCH`: bugfixes, small features, narrow script fixes, local stack usability
  updates, test-only fixes that guard released behavior, and small documentation
  updates that are part of a release handoff.
- `MINOR`: large features, cross-module behavior changes, runtime or gateway
  contract changes, architecture refactors, data-schema changes, and deployment
  boundary changes. A minor bump resets patch to zero.
- No bump: typo-only documentation, commentary-only design notes, formatting,
  or local notes that do not change a build, runtime, deployable asset, or
  release handoff.

When a change is both a bugfix and a broad refactor, use `MINOR`. Do not split a
single coherent feature into several patch bumps just to avoid a minor version.

## Managed Surfaces

Keep these surfaces synchronized with `VERSION`:

- Rust workspace: `agent-platform/Cargo.toml` owns
  `[workspace.package].version`; crates use `version.workspace = true`.
- Python tooling: `pyproject.toml` and `uv.lock`; do not reintroduce
  `requirements.txt` as the dependency source of truth.
- Containers: `tonglingyu-gateway` Dockerfile `APP_VERSION`, OCI labels, and
  Compose image/build defaults.
- Scripts and tests: versioned QA/local-start scripts expose `--version`, and
  the version-management tests carry the same expected project version.

## Commands

Check version synchronization:

```bash
uv run --no-sync python scripts/version.py check
```

For small features or bugfixes:

```bash
uv run --no-sync python scripts/version.py bump patch
uv lock
cargo metadata --manifest-path agent-platform/Cargo.toml --format-version 1 >/dev/null
```

For large features or refactors:

```bash
uv run --no-sync python scripts/version.py bump minor
uv lock
cargo metadata --manifest-path agent-platform/Cargo.toml --format-version 1 >/dev/null
```

Set a specific version only when initializing, repairing drift, or performing a
reviewed manual `MAJOR` change:

```bash
uv run --no-sync python scripts/version.py set 0.1.0
uv lock
cargo metadata --manifest-path agent-platform/Cargo.toml --format-version 1 >/dev/null
```

For local compose startup, use:

```bash
deploy/scripts/start-local-stack.sh
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
  with a digest-pinned image. Custom environment release evidence is maintained
  outside this source repository.
