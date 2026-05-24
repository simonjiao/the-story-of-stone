# Rust Coding Rules

These rules apply to Rust code under `agent-platform/`, which contains the
Tonglingyu gateway/runtime crates and their minimal runtime support crates. They
are implementation constraints, not general style preferences.

## Rule Priority

- Preserve the existing crate, module, schema, and contract boundaries before
  adding new abstraction.
- Prefer typed domain contracts over ad hoc JSON, stringly typed state, or
  broad "common" helpers.
- Keep production behavior explicit: trace ids, audit records, idempotency keys,
  lease ownership, policy decisions, and degraded states must remain visible.
- Do not let a fallback, mock, default empty value, old code path, or replay path
  silently replace the intended code path.
- When a local rule conflicts with existing code, migrate the touched area
  deliberately and keep verification proportional to the behavior being changed.

## Crate Boundaries

- `agent-core` owns shared models, typed IDs, policy, errors, and public API
  contracts.
- `agent-runtime` owns reusable Runtime client behavior and minimal/Hermes
  Runtime adapters used by Tonglingyu.
- `tonglingyu-runtime` owns source snapshot ingestion, SQLite/FTS, evidence,
  reviewer, RQA, and Tonglingyu Runtime workflow behavior.
- `tonglingyu-gateway` owns OpenAI-compatible HTTP, auth, model hiding, rate
  limits, admin surfaces, and request/stream response wrapping.
- Web frameworks, databases, queues, telemetry, and deployment concerns must not
  define core model semantics.

## Module Organization

- Large, relatively independent features must live in a dedicated module or
  module directory instead of expanding a crate root or unrelated module.
- Prefer a small public module facade plus private submodules for substantial
  behavior.
- Move coherent groups of types, helpers, tests, and private functions together
  when the feature can be reasoned about independently.
- Keep cross-module APIs typed and domain-shaped. Do not expose broad catch-all
  helpers just to share implementation details.

## External Rule Catalog Boundaries

- External rule catalogs may hold variable language and domain vocabulary:
  entity aliases, predicate aliases, source-scope terms, query expansion terms,
  evidence slot labels, and public answer boundary phrases.
- External rule catalogs must not own workflow invariants, fail-open behavior,
  role boundaries, source-scope gates, reviewer authority, package binding,
  current-window semantics, or question-frame propagation. Those decisions must
  live in typed Rust code with tests.
- Do not fix a single eval miss by adding a question-specific catalog entry that
  encodes the answer. Catalog changes must represent reusable language, source,
  ontology, or evidence semantics across more than one phrasing or case.
- Query expansion, ontology, evidence slot rules, answer rules, and review rules
  must remain separate responsibilities. A query expansion term can help retrieve
  material, but it cannot prove what the material supports or decide how the
  answer is phrased.
- When adding a catalog-driven capability, add code-level gates that preserve the
  stable system invariant even if the catalog is incomplete, stale, or overbroad.
  The degraded outcome must be explicit: clarification, insufficient coverage,
  review rejection, or auditable fail-closed status.

## Reuse and Templates

- Do not copy-paste the same handler, repository, DTO mapping, config loading,
  audit append, or test fixture shape more than twice. On the third occurrence,
  introduce a local helper, typed builder, trait, or macro-backed template.
- Prefer ordinary Rust reuse first: constructors, builder structs, generic
  helpers, trait methods, and shared test fixtures.
- Use `macro_rules!` only when repeated declarations have identical semantics and
  the macro expansion stays easy to inspect.
- Templates must preserve domain boundaries. Do not hide request lifecycle, run
  lifecycle, approval state, lease ownership, idempotency key, trace id, or audit
  decision behind a vague shared function.
- API responses and error bodies must come from shared response and error types.
  Avoid ad hoc `serde_json::json!` response shapes in production handlers unless
  the endpoint is intentionally dynamic.
- SQL row mapping, status transitions, and enum parsing should be centralized in
  repository or model helpers. New SQL paths must reuse existing mappers when the
  returned shape is the same.
- Test setup should use fixture builders or factory helpers for agents, sessions,
  runs, approvals, leases, locks, and external actions instead of repeating large
  JSON or model literals.

## Traits and Generics

- Use traits to express stable behavior boundaries such as stores, queues,
  policy evaluators, runtime adapters, and external service clients.
- Use generic traits or associated types only when at least two concrete
  implementations are expected, or when tests need a clean in-memory
  implementation behind the same contract.
- Keep trait methods domain-shaped. Prefer `create_run`, `claim_next_run`, or
  `append_audit` over generic verbs such as `handle`, `process`, or `execute`
  when the contract has specific state semantics.
- Avoid over-generic APIs that hide ownership, lifetimes, error semantics, async
  boundaries, retry behavior, or ordering requirements.
- Prefer associated types for implementation-owned types and generic parameters
  for caller-supplied data. Keep bounds narrow and local to the function or impl
  that actually needs them.
- Public traits must return typed `Result` values and document idempotency,
  ordering, ownership, and retry expectations when they cross crate or process
  boundaries.
- Library-style crates should avoid unnecessary runtime-specific trait
  requirements. Bind to Tokio only when the contract genuinely owns Tokio I/O,
  timers, processes, synchronization, or task spawning.

## Async Runtime and Concurrency

- Rust standard library does not provide an official async runtime. In this
  workspace, service and network-facing Rust code should default to Tokio unless
  there is an explicit target-environment constraint or a crate boundary requires
  a runtime-agnostic API.
- Do not introduce a second async runtime such as `async-std` or `smol` into an
  existing Tokio service.
- New async dependencies should run cleanly on the workspace Tokio runtime and
  use the workspace `tokio` dependency when Tokio APIs are needed.
- Application crates may use Tokio-specific entrypoints and primitives such as
  `#[tokio::main]`, `#[tokio::test]`, `tokio::spawn`, `tokio::time`, and
  `tokio::sync`.
- Do not create nested Tokio runtimes or call `Runtime::new().block_on(...)`
  inside async request, worker, or Runtime paths.
- If synchronous CPU-heavy work or blocking I/O is unavoidable, isolate it with
  `tokio::task::spawn_blocking` or a dedicated bounded worker path, and keep
  timeout/cancellation behavior explicit.
- Do not hold locks, transactions, or mutable shared state across external I/O
  unless the ownership model is explicit and tested.
- Queue claiming must remain lease-based and worker-owned. Changes touching run
  execution must cover claim, heartbeat, expiry sweep, retry backoff, dead-letter,
  and finish behavior.
- Concurrent workers must use database-level ownership primitives such as
  `FOR UPDATE SKIP LOCKED`, compare-and-update predicates, leases, or unique
  constraints instead of process-local assumptions.
- Idempotency checks belong at request, session, and run creation boundaries.
  Retries must be safe to repeat under network timeout or worker restart.

## Errors, Panics, and Fallbacks

- Use `CoreResult<T>` and `AgentCoreError` for domain and contract failures.
  Convert infrastructure errors at the boundary with safe, traceable messages.
- Prefer `Result` for all recoverable failures. Do not use `unwrap`, `expect`, or
  `panic!` in production paths except for impossible invariant violations that
  are documented at the call site.
- Convert `Option` to typed errors with context at the boundary where absence
  becomes invalid. Do not let `None` propagate until it causes a panic.
- User-visible errors must expose stable error codes and `trace_id`, not raw
  database, token, network, or secret-bearing strings.
- State-transition failures should use typed conflict errors and keep the
  rejected entity, source state, and target state debuggable.
- Background workers, queue consumers, and HTTP handlers must not panic on bad
  input, missing rows, expired leases, malformed config, or downstream failure.
- Fallbacks are allowed only as explicit degraded behavior. They must produce a
  typed degraded status, audit/event record, metric, or report field.
- Code paths that intentionally use fallback, replay, mock, local no-upstream, or
  default-empty behavior must make that source explicit in the returned type,
  report payload, audit event, or test fixture name.

## Unsafe

- Avoid `unsafe`.
- If a dependency or FFI boundary makes `unsafe` unavoidable, keep it in the
  smallest possible module, document the safety invariant, and cover it with
  targeted tests.
- Do not use `unsafe` to work around borrow-checker or lifetime design issues.
  Refactor ownership, clone small typed IDs, or move state into explicit structs
  instead.
- If `unsafe` is genuinely required for FFI or a low-level boundary, do not
  weaken the whole crate. Put that code in a small module, remove
  `forbid(unsafe_code)` only at the crate where necessary, add local safety
  documentation, and keep the rest of the crate linted.

## Config, Audit, and Secrets

- Parse environment and config into typed structs at process boundaries. Do not
  scatter `std::env::var` reads through business logic.
- Secrets must stay in `.env`, deployment config, or secret-provider paths.
- Do not log token values, API keys, passwords, private keys, credential leases,
  system prompts, raw user-private memory, or secret-bearing payloads.
- Control-plane decisions, approvals, external actions, credential leases,
  compensations, context projection decisions, memory policy decisions, and
  fallback/degraded states must append auditable records with stable action names
  and trace ids.

## Code-Level Lints for New Files

- New crate roots (`lib.rs` or `main.rs`) should declare lint policy in code
  unless the workspace already enforces an equivalent policy centrally:

```rust
#![forbid(unsafe_code)]
#![warn(
    clippy::dbg_macro,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used,
)]
```

- New non-root module files may use the same module-level lint block when the
  crate root does not cover them yet.
- Prefer moving repeated lint policy up to the crate root instead of duplicating
  it in many files.
- Do not add broad `#![allow(...)]` or `#[allow(...)]` attributes to make new
  code pass. Any lint exception must be narrow, placed on the smallest item, and
  include a short reason.
- Test modules may allow `unwrap` or `expect` for fixture setup, but production
  modules should convert failures into typed `Result` values.

## Tests and Verification

- Rust module tests must live in a separate test module file instead of growing
  inline `#[cfg(test)] mod tests { ... }` blocks inside production modules.
- Wire separate test modules from the production module with `#[cfg(test)] mod
  tests;`, using `tests.rs` or `tests/mod.rs` in the corresponding module
  directory.
- When changing an existing Rust module that already has inline tests, do not add
  more inline test bodies there. Move the touched tests into the separate test
  module file as part of the change.
- For Rust-only changes, start with the smallest crate-level `cargo fmt`,
  `cargo clippy`, and `cargo test` command that covers the touched crate.
- Broaden to the full `agent-platform` workspace when shared models, traits,
  runtime behavior, lifecycle behavior, concurrency behavior, public API
  contracts, or release gates change.
- State and concurrency changes must include tests or smoke evidence for lease
  ownership, heartbeat expiry, idempotency, lock behavior, retry timing, and error
  propagation.
- Fallback and degraded-path changes must include both sides of the assertion:
  the primary path still runs when dependencies are healthy, and the degraded
  path is observable through typed status, audit, metrics, or reports.
