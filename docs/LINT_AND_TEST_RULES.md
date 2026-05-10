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
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub.py scripts/download_wikisource.py
python3 -m compileall scripts
```

Use `ruff` and `pytest` when the repo later adds config or tests. Source
snapshot changes also need a temp-dir smoke test rather than a full corpus
rewrite.

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

For deployment config, use dry-runs or render checks first. Back up
`deploy/.env` before editing it and never output secret values.

## Rust

Rust coding rules are in `docs/RUST_CODING_RULES.md`. If Rust code is touched,
use the workspace commands scoped to the changed crates first:

```bash
cargo fmt --manifest-path agent-platform/Cargo.toml --check
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
