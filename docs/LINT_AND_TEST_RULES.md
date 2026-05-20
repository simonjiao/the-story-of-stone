# Lint and Test Rules

Run the smallest check set that covers the touched files. Expand only when
shared contracts, generated data, deployment behavior, or source snapshot
formats change.

## General

- Do not format unrelated files.
- Do not print secrets from `.env`, compose files, logs, or local credentials.
- Treat `resources/sources/`, `resources/styles/`, and transcript outputs as
  data; rewrite them only when the task explicitly changes that corpus.
- If a tool, service, model, network, or credential is missing, report `BLOCKED`
  with the command and reason.
- After generated-output or formatter runs, inspect `git diff --stat`.

## Python

```bash
uv lock --check
uv run --no-sync python -m py_compile \
  scripts/bilibili_hlm_pipeline.py \
  scripts/extract_epub.py \
  scripts/download_wikisource.py \
  scripts/validate_source_snapshots.py \
  scripts/version.py
uv run --no-sync python -m compileall -q scripts tests
uv run --no-sync python -m unittest discover -s tests -p 'test_*.py'
```

Use `ruff` when the repo later adds config. The current lightweight Python
tests use `unittest`; add `pytest` only when a test suite needs it. Source
snapshot changes also need a temp-dir smoke test rather than a full corpus
rewrite.

## Versioning

Version rules are in `docs/VERSIONING_RULES.md`. The minimum local check is:

```bash
uv run --no-sync python scripts/version.py check
```

Patch version bumps are source-owned and should go through:

```bash
uv run --no-sync python scripts/version.py bump patch
```

The project QA wrapper combines version, Python, shell, and Rust format gates:

```bash
scripts/qa.sh --quick
```

## Markdown

```bash
git diff --check -- AGENTS.md README.md docs/
npx --yes markdownlint-cli2 "AGENTS.md" "README.md" "docs/**/*.md"
```

Validate only cheap, local, non-destructive examples unless the task is
specifically about network download behavior.

## Shell and Deploy

```bash
bash -n path/to/script.sh
shellcheck path/to/script.sh
```

For custom deployment config and production evidence, use the sibling
`../tonglingyu-gatekeeper/deploy/` repo. For this repo's local stack, use
dry-runs or render checks first and never output secret values.

## Rust

Rust coding rules are in `docs/RUST_CODING_RULES.md`. If Rust code is touched,
use the workspace commands scoped to the changed crates first:

```bash
cargo fmt --manifest-path agent-platform/Cargo.toml --all --check
cargo clippy --manifest-path agent-platform/Cargo.toml \
  --workspace --all-targets -- -D warnings
cargo test --manifest-path agent-platform/Cargo.toml --workspace
```

State or concurrency changes must cover lease ownership, heartbeat expiry,
idempotency, lock behavior, and error propagation.

## Minimum Gates

- Markdown only: `git diff --check`.
- Python only: `py_compile` or `compileall`, plus targeted smoke when source
  snapshot logic changes.
- Shell: `bash -n`, and `shellcheck` when available.
- Deployment: backup first, then syntax/render checks with secrets sanitized.
