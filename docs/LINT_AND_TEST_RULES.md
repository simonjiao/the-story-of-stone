# Lint and Test Rules

This document defines the default lint and test path for code changes. Run the
smallest check set that covers the files touched by the change, then expand only
when shared contracts, generated outputs, or deployment behavior are affected.

## General Rules

- Keep checks scoped to the touched files or package first; run workspace-wide
  checks when changing shared APIs, build config, or cross-module behavior.
- Do not run broad formatters over unrelated files. After any formatter, inspect
  `git diff --stat` and keep unrelated churn out of the change.
- Do not print secrets from `.env`, compose files, or logs. Report variable
  names, command status, paths, and sanitized evidence only.
- Treat `resources/sources/` and bulk transcript outputs as data. Lint or
  rewrite them only when the task explicitly changes that corpus.
- If a required tool, external service, model, network, or credential is missing,
  stop that check and report it as `BLOCKED` with the exact command and reason.

## Bash and Shell

Targets: `deploy/scripts/*.sh` and any other shell script changed in the task.

Lint:

```bash
bash -n path/to/script.sh
shellcheck path/to/script.sh
```

Use `bash -n` as the minimum required check. Use `shellcheck` when it is
installed; if unavailable, record that explicitly instead of installing tooling
as part of an unrelated change.

Test:

```bash
path/to/script.sh --help
```

For scripts that mutate files, run smoke tests against a temporary directory or
throwaway copied fixture. Never run a destructive restore, deploy, or overwrite
against live config unless the user explicitly requested it.

## Python

Targets: `scripts/*.py`, `src/**/*.py`, and Python helpers introduced for tests
or verification.

Lint:

```bash
python3 -m compileall scripts src
python3 -m ruff check scripts src
```

`compileall` is the minimum syntax gate. Run `ruff` when available or when the
repo later adds a `pyproject.toml`/lint config.

Test:

```bash
python3 -m pytest
python3 -m pytest path/to/test_file.py
```

If no pytest suite covers the change, use an import or CLI smoke test for the
changed module. Avoid network downloads, large ASR/model runs, or corpus rewrites
unless they are the purpose of the task.

## Rust

Targets: `agent-platform/` workspace and changed crates under
`agent-platform/crates/`.

Lint:

```bash
cd agent-platform
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

Use crate-scoped clippy while iterating when only one crate changed:

```bash
cargo clippy -p agent-manager --all-targets -- -D warnings
```

Test:

```bash
cd agent-platform
cargo test --workspace
cargo test -p agent-manager
```

For API, store, queue, runtime, or concurrency changes, include tests or manual
evidence for lease ownership, heartbeat expiry, idempotency, lock behavior, and
error propagation. If Postgres, Docker, or another service is required and not
available, mark the service-backed test as `BLOCKED` and still run the pure unit
or compile checks that do not require it.

## Markdown

Targets: `AGENTS.md`, `README.md`, and `docs/**/*.md` changed in the task.

Lint:

```bash
npx --yes markdownlint-cli2 "AGENTS.md" "README.md" "docs/**/*.md"
```

Use markdownlint when Node/npm access is available. For documentation-only
changes where markdownlint is unavailable, run `git diff --check` and manually
check heading order, fenced code block languages, relative links, and wrapped
secret-free examples.

Test:

```bash
git diff --check -- AGENTS.md README.md docs/
```

For docs that contain commands, validate commands that are local, cheap, and
non-destructive. For operational docs, prefer a dry-run, config render, syntax
check, or temporary-directory smoke test over touching live deployment state.

## Minimum Gate by Change Type

- Shell-only change: `bash -n`, `shellcheck` if available, and one safe smoke
  test.
- Python-only change: `compileall`, `ruff` if available, and targeted pytest or
  import/CLI smoke.
- Rust-only change: `cargo fmt --check`, targeted `cargo clippy`, and targeted
  `cargo test`; run workspace checks for shared contracts.
- Markdown-only change: `git diff --check` plus markdownlint if available.
- Deployment config change: back up `deploy/.env` when relevant, validate compose
  or render syntax, and do not expose secrets in output.
