# Agent Runtime Implementation Checklist

## 文档定位

本 checklist 跟踪 Agent Runtime 完善专项的实施状态。设计边界以
[09-agent-runtime-design.md](09-agent-runtime-design.md) 为准；本文件只记录
可执行任务、验收、测试和待确认项。

## 当前状态

- [x] R0 文档整合完成。
- [x] R1 Profile Contract 和 Schema Validation。
- [x] R2 Runtime Streaming。
- [x] R3 Per-profile Tool Permission Enforcement。
- [x] R4 Multi-profile Step Plan。
- [x] R5B Runtime Tool Execution Loop core。
- [ ] R5 通灵玉按薄 Gateway + Runtime Agent 目标重新接入。
- [ ] 目标环境 Open WebUI 单入口复测。

## 实施前确认

实现前确认已关闭。以下决策已经用于本轮实现，若后续要改变，再单独回到
设计审查：

- [x] R1 profile contract 先采用代码内 registry，后续如需运营动态配置再
  引入持久化表。
- [x] R1 schema 先使用 JSON Schema 语义，具体 Rust crate 由实现时按
  workspace 兼容性选择。
- [x] R2 streaming 不替换现有非 streaming `RuntimeOutput`，只新增
  streaming event contract。
- [x] R3 写入类工具仍强制走 Manager external-action apply/compensate。
- [x] R4 多 profile 编排只创建显式 step plan，不塞进 `HermesRuntimeClient`。
- [x] R5 通灵玉 Gateway 仍是唯一公开入口，Open WebUI 只看到 `tonglingyu`。

R5 决策：

- [x] 通灵玉 Gateway 只做协议适配、鉴权、限流、trace/session 透传和
  OpenAI-compatible 响应封装。
- [x] 通灵玉 Gateway 不做 SQLite/FTS 查询、不构建证据包、不执行 reviewer。
- [x] `honglou-text` 和 `honglou-commentary` 走 LLM profile。
- [x] `honglou-text` 和 `honglou-commentary` 通过 Runtime read-only tools
  查询正文、版本、脂批和评语证据。
- [x] 证据包、replay 和 admin trace 的领域数据归 Runtime Agent/tool 侧维护；
  Gateway 只透传或代理结果。
- [x] 通灵玉四 profile contract 先出工程版，R5 前做一次轻量领域复核后
  再冻结首版字段。
- [x] 通灵玉 Runtime 接入先做本地 dry run，再做远端或目标环境
  Open WebUI 单入口复测。

## R1 Profile Contract 和 Schema Validation

目标：Runtime 不再只是按 profile 名称调用模型，而是先校验 profile 输入，
再校验 profile 输出。

### R1 代码任务

- [x] 在 `agent-core` 新增 `ProfileContract`。
- [x] 在 `agent-core` 新增 `ProfileContractVersion`。
- [x] 在 `agent-core` 新增 `RuntimeToolPolicy`。
- [x] 在 `agent-core` 新增 profile schema 校验错误类型。
- [x] 在 `agent-runtime` 新增 profile contract registry。
- [x] 在 `agent-runtime` 增加 runtime input schema validation。
- [x] 在 `agent-runtime` 增加 runtime output schema validation。
- [x] 在 `agent-worker` 调用 Runtime 时带上 profile contract metadata。
- [x] 当前不需要持久化 contract version；后续如需运营动态配置，再在
  `agent-store` 新增非破坏字段或新表。

### R1 验收

- [x] schema valid 时正常返回 `RuntimeOutput`。
- [x] schema invalid 时返回安全错误。
- [x] schema invalid 不进入 successful runtime output。
- [x] 错误不泄露 prompt、secret、connector payload 或内部栈。
- [x] metadata 包含 `profile_id`、`schema_version` 和 `runtime_profile`。

### R1 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-worker`

### R1 提交

- [x] `runtime: add profile contract validation`

## R2 Runtime Streaming

目标：Runtime 支持原生流式输出，同时保留最终 `RuntimeOutput` 作为落盘结果。

### R2 代码任务

- [x] 在 `agent-core` 新增 `RuntimeStreamEvent`。
- [x] 在 `agent-core` 新增 streaming trait 或 feature-gated adapter 边界。
- [x] 在 `agent-runtime` 为 Hermes adapter 增加 streaming path。
- [x] streaming final event 携带最终 `RuntimeOutput`；Worker 非 streaming
  完成态语义保持不变。
- [x] `agent-orchestrator` 现有 OpenAI-compatible SSE wrapper 不回退。
- [x] 保留非 streaming path 的原有行为。

### R2 验收

- [x] 非 streaming path 不回退。
- [x] streaming final 和非 streaming 输出语义一致。
- [x] error event 不泄露 prompt、credential、connector payload 或内部栈。
- [x] trace/audit 能看到流式调用的 final 状态。

### R2 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-orchestrator`

### R2 提交

- [x] `runtime: add streaming event contract`

## R3 Per-profile Tool Permission Enforcement

目标：profile 工具权限成为真实执行约束，而不是 prompt 约定。

### R3 代码任务

- [x] 在 `agent-core` 定义 tool capability。
- [x] 在 `agent-core` 定义 allowed tools、denied tools 和 effective tool set。
- [x] Manager 持有 agent config / profile contract 来源；Worker 从该受控配置
  转发授权 tool scope。
- [x] 在 `agent-runtime` 执行前计算 effective tool set。
- [x] 在 `agent-runtime` 拒绝越权 tool request。
- [x] 保持写入类工具必须走 Manager external-action apply/compensate。
- [x] audit / metadata 记录 effective tool set。

### R3 验收

- [x] profile 不能调用未授权工具。
- [x] prompt injection 不能打开额外工具权限。
- [x] 写入类工具不能绕过 Manager external-action plan。
- [x] audit 能记录 effective tool set。

### R3 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-manager`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`

### R3 提交

- [x] `runtime: enforce profile tool policy`

## R4 Multi-profile Step Plan

目标：多 profile 编排显式化，不藏在 `HermesRuntimeClient` 或一次 run/message
调用里。

### R4 代码任务

- [x] 在 `agent-core` 新增 `RuntimeStepPlan`。
- [x] 在 `agent-core` 新增 `RuntimeStep`。
- [x] 在 `agent-core` 新增 `RuntimeStepStatus`。
- [x] 在 `agent-manager` 或 domain gateway helper 中创建 step plan。
- [x] 在 `agent-runtime` 只执行单个已授权 step。
- [x] step 输出只通过 schema 校验后的 `output_ref` 传递。
- [x] 当前复用 Runtime metadata、Gateway workflow state 和 audit；后续如需
  跨 gateway 查询，再新增 append-only step audit 表。
- [x] 增加 step 失败降级或终止策略。

### R4 验收

- [x] 每个 profile step 独立可追踪。
- [x] step output 未通过 schema 时不能进入下一 step。
- [x] Runtime 不能自行创建新 step。
- [x] step 失败不会导致权限扩大或未审计输出。

### R4 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-worker`

### R4 提交

- [x] `runtime: add multi-profile step plan`

## R5 通灵玉薄 Gateway + Runtime Agent 接入

目标：把通灵玉内部角色升级为真实 Runtime profile 执行边界，同时把 Gateway
收敛为协议入口，不在 Gateway 请求路径中执行领域检索、证据包或 reviewer。

### R5A 薄 Gateway 边界

- [ ] Gateway 只做 OpenAI-compatible 协议适配、鉴权、限流、路由、
  trace/session 透传、SSE 转发、模型隐藏和响应封装。
- [ ] Gateway 不直接执行 source snapshot、SQLite 或 FTS 查询。
- [ ] Gateway 不构建证据卡片或证据包。
- [ ] Gateway 不执行 reviewer 或本地审校规则。
- [ ] Gateway 不维护证据包 replay 的领域逻辑。
- [ ] Open WebUI 仍只看到 `tonglingyu`，用户不能选择 `honglou-*`
  内部 profile。

### R5B Runtime Tool Execution Loop

- [x] 在 `agent-core` / `agent-runtime` 补齐 `RuntimeToolCall`、
  `RuntimeToolResult` 和 `RuntimeToolExecutor` 等价 contract。
- [x] Runtime adapter 能处理 LLM profile 发起的 read-only tool call，或执行
  受控 step tool call。
- [x] per-profile allowed tools 在真实 tool execution 前强制校验。
- [x] tool output 做 schema 校验，并通过 `output_ref` 或 evidence/package ref
  传递给后续 step。
- [x] 越权 tool、写入类 tool 或未知 tool 返回安全错误。
- [ ] tool call / tool result 事件接入 append-only audit 或等价 runtime trace。
- [x] 写入类工具仍只能走 Manager external-action apply/compensate。

### R5C 通灵玉 Evidence Read-only Tools

- [ ] 从 `tonglingyu-gateway` 请求路径抽出 source snapshot loader。
- [ ] 从 `tonglingyu-gateway` 请求路径抽出 SQLite/FTS 查询。
- [ ] 从 `tonglingyu-gateway` 请求路径抽出证据卡片和证据包构建。
- [ ] 从 `tonglingyu-gateway` 请求路径抽出证据包 read/replay。
- [ ] 定义 `tonglingyu.text.search` read-only tool。
- [ ] 定义 `tonglingyu.commentary.search` read-only tool。
- [ ] 定义 `tonglingyu.evidence.package.create` read-only tool。
- [ ] 定义 `tonglingyu.evidence.package.read` read-only tool。
- [ ] 定义 `tonglingyu.evidence.package.replay` read-only tool。
- [ ] 工具输出保留原始字形、source snapshot 位置、版本和 evidence refs。
- [ ] 工具不暴露 secret、写权限 credential 或内部 prompt。

### R5D 四 Profile 编排

- [ ] 为 `honglou-text` 定义 LLM profile contract、允许工具和输出 schema。
- [ ] 为 `honglou-commentary` 定义 LLM profile contract、允许工具和输出 schema。
- [ ] 为 `honglou-main` 定义 LLM profile contract、输入依赖和输出 schema。
- [ ] 为 `honglou-reviewer` 定义 LLM profile contract、输入依赖和输出 schema。
- [ ] `honglou-text` 通过 `tonglingyu.text.search` 生成正文 evidence analysis。
- [ ] `honglou-commentary` 通过 `tonglingyu.commentary.search` 生成脂批 evidence
  analysis。
- [ ] 证据包由 Runtime Agent/tool 侧创建，`honglou-main` 只消费 package ref
  和前序 profile 输出。
- [ ] `honglou-reviewer` 强制消费草稿、claim statements 和 package ref。
- [ ] reviewer 不可关闭；未通过 reviewer 的结果不能作为最终回答返回。
- [ ] 四 profile step 的 schema、duration、tool set、output_ref 和 trace_id
  可追踪。

### R5E Gateway 集成和验证

- [ ] 将旧 `answer_with_optional_upstream` / 本地 query path 替换为 Runtime
  step plan 调用。
- [ ] Gateway streaming response 只转发 Runtime event，不自行生成领域内容。
- [ ] Gateway final response 只包含最终回答、trace_id、session/package ref 和
  安全元数据，不暴露内部日志或 prompt。
- [ ] 增加 fake runtime/tools 的本地 dry run。
- [ ] 增加 Gateway 不直接触碰 SQLite/FTS/reviewer 的回归断言。
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
- [ ] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`
- [ ] 目标环境 Open WebUI 单入口复测。

### R5 提交

- [ ] `tonglingyu: move domain flow behind runtime agent`

## 完成口径

Runtime 专项完成前，只能声明：

- [x] Agent Platform 已具备 P1 真实 Hermes Runtime 只读闭环。
- [x] Agent Platform 已具备 P2 external-action 执行链路。
- [x] Agent Runtime 已具备 streaming、schema、tool policy 和 step plan。
- [ ] 通灵玉已满足薄 Gateway + Runtime Agent 架构。

Runtime 专项全部完成后，必须满足：

- [x] R1 到 R4 repo/local checkbox 关闭。
- [ ] R5A 到 R5E checkbox 关闭。
- [ ] 对应测试命令全部通过。
- [ ] `PROGRESS.md` 更新完成状态和残余风险。
- [ ] 通灵玉远端或目标部署完成 Open WebUI 单入口复测。
