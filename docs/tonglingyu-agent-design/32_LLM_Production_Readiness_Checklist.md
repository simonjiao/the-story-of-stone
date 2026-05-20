# LLM Production Readiness Checklist

本文由 `31_LLM支持点与全路径Eval方案.md` 生成，用于跟踪 LLM 接入与全路径 eval
从 repo-local gate 进入 production-ready 的实现证据、缺口和阻塞项。本文是 checklist/progress
载体，不改变 31 号设计文档的约束。

## 0. 当前判断

状态：in progress。

当前不能声明 production-ready。S1-S7 repo-local eval、LLM release report、gatekeeper release
readiness LLM required gate 和本地 contract tests 已在本轮闭合；剩余阻塞是目标环境 live gate、目标环境
release readiness 和目标环境 saved validator 还没有针对当前提交重新通过。

## 1. 设计来源

本 checklist 只采用 31 号设计中的以下硬条件：

- 第 2 节不可变原则：LLM 不能作为事实源，不能决定 scope、ACL、memory read enablement、reviewer
  裁决或 evidence package 写入。
- 第 3.1 节模块边界：`llm_contracts.rs`、`llm_modes.rs`、`llm_provider.rs`、
  `llm_resolver.rs`、`conversation_state.rs`、`retrieval_suggestion.rs`、
  `draft_revision.rs`、`llm_eval.rs`、`user_response_safety.rs` 必须职责分离。
- 第 7 节 eval 设计：10 个 JSONL 数据集、最小条数、hard gate、failure attribution 和 release
  report schema。
- 第 8.3 节 production-ready 声明条件。
- 第 9 节 S1-S7 阶段化实施边界。
- 第 10 节默认 disabled、shadow/enforced、fail-closed 和回滚要求。
- 第 13 节最终冻结 checklist：实现证据、目标环境 live gate 和 release readiness report 才能支撑
  production-ready。

## 2. 设计 Checklist 与实现对比

### 2.1 模块边界

| 设计项 | 当前实现证据 | 状态 | 缺口 |
|---|---|---|---|
| LLM contract 常量/schema 独立 | `llm_contracts.rs` | implemented | 本轮 `cargo test`/`clippy` 已复验 |
| mode enum 与默认回滚 | `llm_modes.rs` | implemented | 本轮 `cargo test`/`clippy` 已复验 |
| provider-neutral adapter、fake/schema repair | `llm_provider.rs` | implemented | 本轮 `cargo test`/`clippy` 已复验 |
| resolver contract/routing | `llm_resolver.rs` | implemented | 本轮 `cargo test`/`llm-eval` 已复验 |
| conversation state summary | `conversation_state.rs` | implemented | 本轮 `cargo test`/`llm-eval` 已复验 |
| retrieval suggestion schema/patch | `retrieval_suggestion.rs` | implemented | 本轮 `cargo test`/`llm-eval` 已复验 |
| draft/reviewer observation gate | `draft_revision.rs` | implemented | 本轮 `cargo test`/`llm-eval` 已复验 |
| eval runner/release report | `llm_eval.rs` | implemented | release readiness 已消费该 report |
| public response scanner | `user_response_safety.rs` | implemented | 本轮 `cargo test`/`llm-eval` 已复验 |

### 2.2 Eval 数据集

| 数据集 | 设计最小条数 | 当前条数 | 状态 | 备注 |
|---|---:|---:|---|---|
| `request_safety.jsonl` | 20 | 20 | implemented | 本轮 `llm-eval` 已复验 |
| `streaming_dedupe.jsonl` | 16 | 16 | implemented | 覆盖 SSE/cache/dedupe |
| `question_resolution.jsonl` | 33 | 39 | implemented | 超过设计下限 |
| `session_summary.jsonl` | 20 | 20 | implemented | 本轮 `llm-eval` 已复验 |
| `retrieval_policy.jsonl` | 18 | 18 | implemented | 本轮 `llm-eval` 已复验 |
| `rag_evidence.jsonl` | 20 | 20 | implemented | 本轮 `llm-eval` 已复验 |
| `context_projection.jsonl` | 18 | 18 | implemented | 本轮 `llm-eval` 已复验 |
| `package_claims.jsonl` | 20 | 20 | implemented | 本轮 `llm-eval` 已复验 |
| `reviewer_security.jsonl` | 24 | 24 | implemented | 本轮 `llm-eval` 已复验 |
| `memory_policy.jsonl` | 20 | 20 | implemented | 本轮 `llm-eval` 已复验 |

### 2.3 S1-S7 阶段

| 阶段 | 设计退出条件 | 当前实现对比 | 状态 |
|---|---|---|---|
| S1 | runner、安全基线、非流式/SSE/cache 泄露扫描 | fixture 与 scanner 已存在 | repo-local evidence passed |
| S2 | resolver schema、白名单、fail-closed、fixture | `llm_resolver.rs` 与 `question_resolution.jsonl` 已存在 | repo-local evidence passed |
| S3 | provider adapter、shadow/enforced、schema repair | `llm_provider.rs`、`llm_modes.rs` 已存在 | repo-local evidence passed |
| S4 | conversation summary writer/loader/projection/eval | `conversation_state.rs` 与 `session_summary.jsonl` 已存在 | repo-local evidence passed |
| S5 | retrieval suggestion schema、deterministic patch、eval | `retrieval_suggestion.rs` 与 S5 fixtures 已存在 | repo-local evidence passed |
| S6 | profile observation、claim-first draft、review override、eval | `draft_revision.rs` 与 S6 fixtures 已存在 | repo-local evidence passed |
| S7 | full-path runner、failure attribution、release report、P6 持续回归 | `llm-release-report` 与 gatekeeper full QA 已运行 | repo-local evidence passed; target live gate pending |

### 2.4 Production-ready 条件

| 条件 | 当前实现对比 | 状态 |
|---|---|---|
| S1-S7 均有可复现通过证据 | 本轮 `llm-eval` 215/215，通过 hard gate | repo-local passed |
| full-path eval suite 有 release report | 本轮 `llm-release-report` status passed | repo-local passed |
| 非流式、SSE、缓存复用脱敏回归 | S1 fixtures 已复跑 | repo-local passed |
| 本地测试、clippy、smoke、strict Gateway gate | Rust test/clippy 和 gatekeeper full 已通过；strict Gateway 仍需目标环境 live gate | local passed, target pending |
| 目标环境 live gate | 尚未针对当前 LLM S1-S7 版本运行 | blocked |
| release readiness gate | 本地 release readiness 已纳入 LLM release report；目标环境未复跑 | local passed, target pending |
| blocker 全部关闭或排除 | B4/B5 未关闭 | blocked |

## 3. 当前 Blocker

- [x] B1：`deploy/scripts/verify-tonglingyu-release-readiness.sh` 没有 LLM required gate。
- [x] B2：`deploy/scripts/verify-tonglingyu-release-readiness-report.sh` 没有校验
  `tonglingyu.llm_release_report`。
- [x] B3：release manifest 没有绑定 `llm_eval_run_id`、`llm_eval_report_sha256`、case counts 和
  LLM artifact policy。
- [ ] B4：目标环境 live gate 尚未针对当前 LLM S1-S7 版本运行。
- [ ] B5：gatekeeper release 工具已提交；进入 production-ready 前必须同步/部署到目标环境。

## 4. 实施 Checklist

- [x] P0：从 31 号设计生成本 checklist。
- [x] P0：完成当前实现对比，明确 production-ready 阻塞项。
- [x] P1：在 gatekeeper release readiness 中新增 LLM release report required gate。
- [x] P2：在 release readiness validator 中校验 LLM report schema、status、dataset minimums、case counts、
  hard gate、artifact policy、live gate required 字段。
- [x] P3：将 LLM gate 摘要写入 release manifest，并禁止 raw LLM payload、raw memory、tool payload 进入
  release report。
- [x] P4：补本地 contract test，证明缺失/失败/过期/夹带 raw payload 的 LLM report 会 hard fail。
- [x] P5：复跑 repo-local `llm-eval`、`llm-release-report`、Rust test、clippy、gatekeeper full QA。
- [x] P6：提交 source/gatekeeper 本地变更，保证目标环境绑定的 git commit 不处于 tracked dirty。
- [ ] P7：同步目标环境 release 工具并运行目标环境 live gate。
- [ ] P8：运行 release readiness gate 与 saved validator，生成 production readiness evidence。
- [ ] P9：把目标环境 evidence 路径、digest、commit、image 写回本 checklist 与 `PROGRESS.md`。

## 5. 实施级任务拆解

### 5.1 Gatekeeper LLM Release Gate

目标：release readiness 必须把 LLM release report 作为 required gate，而不是只在 `scripts/qa.sh --full`
中临时运行。

需要修改：

- [x] 新增 `deploy/scripts/verify-tonglingyu-llm-release-report.sh`。
- [x] 修改 `deploy/scripts/verify-tonglingyu-release-readiness.sh`，新增 `llm_release` required gate。
- [x] 修改 `deploy/scripts/verify-tonglingyu-release-readiness-report.sh`，新增 `llm_release` gate 校验。
- [x] 修改 `deploy/scripts/test-tonglingyu-release-readiness-contract.sh`，补正向和负向 contract tests。
- [x] 保持 `scripts/qa.sh --full` 继续生成 LLM eval 与 LLM release report。

### 5.2 Env Contract

新增或固定以下环境变量：

| 变量 | 必填 | 默认值 | 用途 |
|---|---|---|---|
| `TONGLINGYU_LLM_RELEASE_REPORT_PATH` | release readiness 必填 | 无 | 指向 `tonglingyu.llm_release_report` JSON。 |
| `TONGLINGYU_LLM_RELEASE_REPORT_MAX_AGE_HOURS` | 否 | `24` | LLM release report 最大年龄。 |
| `TONGLINGYU_RELEASE_LLM_REPORT_CMD` | 否 | `deploy/scripts/verify-tonglingyu-llm-release-report.sh` | gate command override，仅允许本地 contract test。 |
| `TONGLINGYU_STORY_OF_STONE_DIR` | QA 场景必填 | gatekeeper 旁路默认路径 | `scripts/qa.sh --full` 生成 LLM report 时使用。 |

Command override 规则：

- [x] `TONGLINGYU_RELEASE_LLM_REPORT_CMD` 非空时，必须受
  `TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true` 保护。
- [x] production readiness 不能依赖 overridden LLM gate command。

### 5.3 LLM Release Report Gate Acceptance

`verify-tonglingyu-llm-release-report.sh` 必须校验：

- [x] report path 非空、文件存在、JSON 可解析。
- [x] `object == "tonglingyu.llm_release_report"`。
- [x] `schema_version == "v1"`。
- [x] `suite_version == "tonglingyu-llm-eval-v1"`。
- [x] `status == "passed"`。
- [x] `llm_eval_run_id` 非空。
- [x] `llm_eval_report_sha256` 为 `sha256:<64 hex>`。
- [x] `case_counts.total > 0`，`passed == total`，`failed == 0`。
- [x] `readiness_checks.repo_local_llm_eval_passed == true`。
- [x] `readiness_checks.hard_gate_failure_count == 0`。
- [x] `readiness_checks.s1_to_s7_dataset_minimums_present == true`。
- [x] `readiness_checks.missing_or_short_datasets == []`。
- [x] `readiness_checks.user_response_safety_gate_present == true`。
- [x] `readiness_checks.target_environment_live_gate_required == true`。
- [x] `readiness_checks.production_ready_declaration_allowed == false`。
- [x] `artifact_policy.raw_llm_payload_embedded == false`。
- [x] `artifact_policy.raw_memory_embedded == false`。
- [x] `artifact_policy.tool_payload_embedded == false`。
- [x] gate stdout 只能输出 gate summary，不输出 raw prompt、raw response、raw memory、tool payload 或 secret-like
  value。

Gate stdout 必须输出：

- [x] `object == "tonglingyu.llm_release_gate"`。
- [x] `status == "ok"`。
- [x] `report_path`。
- [x] `report_sha256`。
- [x] `llm_eval_run_id`。
- [x] `llm_eval_report_sha256`。
- [x] `suite_version`。
- [x] `case_counts`。
- [x] `readiness_checks`。
- [x] `artifact_policy`。
- [x] `secret_values_printed == false`。

### 5.4 Release Readiness Manifest

`verify-tonglingyu-release-readiness.sh` 的 release manifest 必须新增：

- [x] `release_manifest.llm.object == "tonglingyu.llm_release_manifest"`.
- [x] `release_manifest.llm.llm_eval_run_id`。
- [x] `release_manifest.llm.llm_eval_report_sha256`。
- [x] `release_manifest.llm.llm_release_report_sha256`。
- [x] `release_manifest.llm.suite_version`。
- [x] `release_manifest.llm.case_counts`。
- [x] `release_manifest.llm.artifact_policy`。
- [x] `release_manifest.llm.readiness_checks_sha256`。

`release_artifact_registry.entries` 必须新增：

- [x] `llm_eval_report`，source gate 为 `llm_release`。
- [x] `llm_release_report`，source gate 为 `llm_release`。

### 5.5 Validator Negative Tests

`test-tonglingyu-release-readiness-contract.sh` 必须至少证明以下情况会失败：

- [x] 缺少 `llm_release` gate。
- [x] `llm_release` gate stdout 缺失 success JSON。
- [x] LLM report `object` 错误。
- [x] LLM report `status != passed`。
- [x] LLM report `case_counts.failed > 0` 或 `passed != total`。
- [x] LLM report `hard_gate_failure_count > 0`。
- [x] LLM report `missing_or_short_datasets` 非空。
- [x] LLM report artifact policy 任一 raw 字段为 `true`。
- [x] LLM report `production_ready_declaration_allowed == true`。
- [x] LLM report 超过 `TONGLINGYU_LLM_RELEASE_REPORT_MAX_AGE_HOURS`。
- [x] release manifest 中 LLM digest 与 gate stdout 不一致。
- [x] artifact registry 缺少 LLM eval/release report entry。

### 5.6 Local Acceptance Commands

本地必须通过：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  llm-eval \
  --fixture-dir agent-platform/crates/tonglingyu-gateway/evals/fixtures \
  --report-out agent-platform/crates/tonglingyu-gateway/evals/reports/llm-eval-production.json \
  --fail-on-hard-gate

cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  llm-release-report \
  --eval-report agent-platform/crates/tonglingyu-gateway/evals/reports/llm-eval-production.json \
  --report-out agent-platform/crates/tonglingyu-gateway/evals/reports/llm-release-production.json

cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway --all-targets -- -D warnings
```

gatekeeper 本地必须通过：

```bash
TONGLINGYU_STORY_OF_STONE_DIR=/Users/simon/huixiangdou/.worktrees/the-story-of-stone-llm-as-problem-resolver \
  ./scripts/qa.sh --full

./deploy/scripts/test-tonglingyu-release-readiness-contract.sh
```

### 5.7 Target Environment Acceptance

目标环境阶段必须能由命令直接执行，不能只写“运行 live gate”。

#### T0 Local Commit Boundary

- [ ] source repo 当前分支提交完成，目标 commit 可记录。
- [ ] gatekeeper release 工具提交完成，目标 commit 可记录。
- [ ] `git status --porcelain --untracked-files=no` 在 source repo 与 gatekeeper repo 均为空。

#### T1 Deploy, Sync Tools, Live Gate, Release Automation

目标环境入口命令固定为：

```bash
TONGLINGYU_SOURCE_REPO_DIR=/Users/simon/huixiangdou/.worktrees/the-story-of-stone-llm-as-problem-resolver \
  ./deploy/scripts/deploy-hhost-stack.sh --run-release-automation
```

该命令必须完成：

- [ ] 部署包含当前 LLM S1-S7 代码与 fixtures 的 story/gateway 版本。
- [ ] 同步 gatekeeper release tools 到目标环境。
- [ ] 远端工具同步校验必须包含 `verify-tonglingyu-llm-release-report.sh`。
- [ ] 目标机 release automation 必须生成当次 `llm-eval-production.json`。
- [ ] 目标机 release automation 必须生成当次 `llm-release-production.json`。
- [ ] release readiness 必须以目标机生成的
  `TONGLINGYU_LLM_RELEASE_REPORT_PATH=<remote-artifact>/llm-release-production.json`
  运行，不能使用本地 report 或 synthetic report。
- [ ] 运行目标环境 live gates。
- [ ] 运行 `TONGLINGYU_RELEASE_REQUIRE_LIVE=true` 的 release readiness。
- [ ] 运行 saved release readiness validator。

#### T2 Target Pass Criteria

production-ready 只能在以下条件全部成立时声明：

- [ ] deploy report `status == "ok"`。
- [ ] source commit 与 gatekeeper commit 均记录，且 tracked dirty 均为 `false`。
- [ ] release readiness manifest 中的 git commit 必须绑定 source commit，不能误绑定 gatekeeper commit。
- [ ] remote live gates report `status == "ok"`，gate result 全部 passed。
- [ ] remote release automation report `status == "ok"` 且 `production_ready_proven == true`。
- [ ] release automation 内部 `checks.llm_eval == "passed"`。
- [ ] release automation 内部 `checks.llm_release_report == "passed"`。
- [ ] release readiness report `status == "passed"` 且 `production_release_ready == true`。
- [ ] release readiness `llm_release` required gate 为 passed。
- [ ] saved validator `status == "ok"` 且 `errors == []`。
- [ ] `release_blockers == []` 且 `required_failures == []`。
- [ ] `release_manifest.llm` 中的 eval/report digest 与 release artifact registry 一致。
- [ ] post-release monitor 为 passed 或等价的当前 release automation policy 通过。

#### T3 Evidence To Record

目标环境通过后，必须写回：

- [ ] deploy report path 与 sha256。
- [ ] remote live gate report path 与 sha256。
- [ ] remote release automation report path 与 sha256。
- [ ] release readiness report path 与 sha256。
- [ ] saved validator report path 与 sha256。
- [ ] remote `llm-eval-production.json` path 与 sha256。
- [ ] remote `llm-release-production.json` path 与 sha256。
- [ ] source commit、gatekeeper commit、running image id、running image label version。
- [ ] `required_failures`、`release_blockers`、`post_release_monitor` 状态。

#### T4 Hard Fail Conditions

任一情况出现时，必须保持 in progress，不能声明 production-ready：

- [ ] 使用本地 LLM release report 替代目标机当次 report。
- [ ] 使用 gate command override 支撑 production-ready。
- [ ] source/gatekeeper tracked dirty 为 `true`。
- [ ] `llm_release` gate 缺失、失败、过期或 digest 不一致。
- [ ] saved validator 未运行、失败或未保存。
- [ ] 目标环境 live gate 未运行或 required gate 有失败。

## 6. 验证记录

2026-05-20 本地验证记录：

- `cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- llm-eval ... --fail-on-hard-gate`
  通过：`215/215`，hard gate failure 为空，`eval_run_id=llm-eval-019e453d39bd7393b41c1a6fdd4c1629`。
- `cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- llm-release-report ...`
  通过：`status=passed`，`llm_eval_report_sha256=sha256:14192ded4a5dd022542c57e2b79b176d5c6dd8179a0aefd82dc8a08424280f22`。
- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway` 通过：`95 passed`。
- `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway --all-targets -- -D warnings` 通过。
- `./deploy/scripts/test-tonglingyu-release-readiness-contract.sh` 通过。
- `TONGLINGYU_STORY_OF_STONE_DIR=... TONGLINGYU_SOURCE_REPO_DIR=... ./scripts/qa.sh --full` 通过：
  `qa_status=passed mode=full`，full QA 内部 LLM eval 为 `215/215`。
- release automation 本地 mock gate 检查通过关键 LLM 生成链路：`checks.llm_eval=passed`、
  `checks.llm_release_report=passed`，`llm_eval_report_sha256` 与 `llm_release_report_sha256`
  均存在；该检查因非 live 且使用 gate override 按预期不产生 production-ready 结论。
- `TONGLINGYU_STORY_OF_STONE_DIR=... TONGLINGYU_SOURCE_REPO_DIR=... ./scripts/qa.sh --full`
  在 release automation 补强后复跑通过：`qa_status=passed mode=full`，
  `eval_run_id=llm-eval-019e454a3a237e53b989eede0fc7d3e4`，
  `release_run_id=llm-release-019e454a3e157682b59258dd658721be`。
- `verify-tonglingyu-llm-release-report.sh` 校验实际
  `llm-release-production.json` 通过：`status=ok`，`report_sha256=14bf4478186fb423dfc36f15db948b4ee7975c4a2d561e145342a1ad2d7c1a79`。
- 本地 release readiness 集成验证通过：使用实际 LLM release report、其他 gate command override 的
  saved validator 结果为 `status=passed_with_skipped_gates`、`production_release_ready=false`、
  `llm_gate_status=passed`。

以上只能证明 repo-local 与 gatekeeper-local readiness wiring；不能替代目标环境 live gate、目标环境
release readiness 或 production-ready 结论。
