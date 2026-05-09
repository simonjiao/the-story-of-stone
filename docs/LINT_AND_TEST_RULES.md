# Lint and Test Rules

Run the smallest check set that covers the files touched by the change. Expand only when shared contracts, generated data, deployment behavior, or source snapshot formats are affected.

## General

- Do not run broad formatters over unrelated files.
- Do not print secrets from `.env`, compose files, logs, or local credentials.
- Treat `resources/sources/`, `resources/styles/`, and bulk transcript outputs as data. Rewrite them only when the task explicitly changes that corpus.
- If a required tool, external service, model, network, or credential is missing, mark the check as `BLOCKED` with the exact command and reason.
- After generated-output or formatter runs, inspect `git diff --stat`.

## Python

Targets: `scripts/*.py`, `src/**/*.py`, and Python verification helpers.

Minimum syntax gate:

```bash
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub.py scripts/download_wikisource.py src/tonglingyu_agent/__init__.py
```

Broader check when touching many Python files:

```bash
python3 -m compileall scripts src
```

Use `ruff` and `pytest` when the repo later adds config or tests:

```bash
python3 -m ruff check scripts src
python3 -m pytest
```

For source snapshot changes, include a small temp-dir smoke test rather than rewriting the full corpus.

## Markdown

Targets: `AGENTS.md`, `README.md`, and `docs/**/*.md`.

Minimum gate:

```bash
git diff --check -- AGENTS.md README.md docs/
```

Use markdownlint when available:

```bash
npx --yes markdownlint-cli2 "AGENTS.md" "README.md" "docs/**/*.md"
```

For documentation commands, validate local, cheap, non-destructive examples. Do not run network downloads unless the task is about the download path.

## Shell and Deploy

Targets: `deploy/scripts/*.sh` and other shell scripts.

Minimum syntax gate:

```bash
bash -n path/to/script.sh
```

Use `shellcheck` when installed:

```bash
shellcheck path/to/script.sh
```

For deployment config, use dry-runs or render checks first. Back up `deploy/.env` before editing it and never output secret values.

## Rust

Current Tonglingyu work does not require Rust changes. If a future task explicitly touches Rust code, use the Rust workspace’s own local commands and keep the check scoped to the changed crates first:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For state, queue, runtime, or concurrency changes, tests or manual evidence must cover lease ownership, heartbeat expiry, idempotency, lock behavior, and error propagation.

## Minimum Gate by Change Type

- Markdown-only change: `git diff --check`.
- Python-only change: `py_compile` or `compileall`, plus a targeted smoke test.
- Source snapshot script change: Python syntax check plus temp-dir extraction/download parser smoke.
- Shell change: `bash -n`, and `shellcheck` if available.
- Deployment change: backup first, then syntax/render checks, with secrets sanitized.
