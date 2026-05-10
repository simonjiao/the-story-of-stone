# Agent Runtime Implementation Checklist

## 文档定位

本 checklist 跟踪 Agent Runtime 完善专项的实施状态。设计边界以
[09-agent-runtime-design.md](09-agent-runtime-design.md) 为准；本文件只记录
可执行任务、验收、测试和待确认项。

## 当前状态

- [x] R0 文档整合完成。
- [x] R1 Profile Contract 和轻量 Schema Validation。
- [x] R2 Runtime Streaming 基础事件。
- [x] R2 tool progress / schema partial streaming 扩展。
- [x] R3 Per-profile Tool Permission Enforcement。
- [x] R4 Step Plan 数据模型和单 step metadata。
- [x] R4 Multi-profile Step Plan 执行器、依赖、fallback 和 output_ref 流转。
- [x] R4.5 Runtime Tool Execution Loop。
- [x] Agent Runtime 本体 repo/local 完成口径复核；领域 Gateway 接入不作为
  本 checklist 范围。

## 实施前确认

实现前确认已关闭。以下决策已经用于本轮实现，若后续要改变，再单独回到
设计审查：

- [x] R1 profile contract 先采用代码内 registry，后续如需运营动态配置再
  引入持久化表。
- [x] R1 schema 先使用轻量 JSON Schema 子集，完整 JSON Schema 不作为当前
  完成口径。
- [x] R2 streaming 不替换现有非 streaming `RuntimeOutput`，只新增
  streaming event contract。
- [x] R3 写入类工具仍强制走 Manager external-action apply/compensate。
- [x] R4 多 profile 编排只创建显式 step plan，不塞进 `HermesRuntimeClient`。
- [x] 领域 Gateway 接入计划迁出 Runtime checklist；通灵玉以
  `docs/tonglingyu-agent-design/` 为准。

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
- [ ] 如需要完整 JSON Schema，替换当前轻量校验器或接入标准 schema crate。

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
- [x] 增加 tool progress streaming event。
- [x] 增加 schema partial streaming event。

### R2 验收

- [x] 非 streaming path 不回退。
- [x] streaming final 和非 streaming 输出语义一致。
- [x] error event 不泄露 prompt、credential、connector payload 或内部栈。
- [x] trace/audit 能看到流式调用的 final 状态。
- [x] tool progress / schema partial 有端到端验证。

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
- [x] Worker 从受控 agent config 转发 `requested_tools` 作为本次授权 tool
  scope。
- [x] 在 `agent-runtime` 执行前按 `requested_tools ∩ allowed_tools -
  denied_tools - non_read_only` 计算 effective tool set。
- [x] 在 `agent-runtime` 拒绝越权 tool request。
- [x] 在 `agent-core` / `agent-runtime` 增加 `RuntimeToolCapability`，拒绝
  non-read-only tool scope 和 tool call。
- [x] 空 `allowed_tools` 解释为无工具权限，不解释为通配符。
- [x] 保持写入类工具必须走 Manager external-action apply/compensate。
- [x] metadata 记录 effective tool set。
- [x] Manager 创建 agent 时从 profile contract 生成默认授权 `requested_tools`；
  Worker 对已有 agent 按 contract 做防御性派生。

### R3 验收

- [x] profile 不能调用未授权工具。
- [x] prompt injection 不能打开额外工具权限。
- [x] 写入类工具不能绕过 Manager external-action plan。
- [x] metadata 能记录 effective tool set。
- [x] Runtime 只向 Hermes 暴露本次请求授权的工具交集。
- [x] non-read-only tool 即使被误配置为 allowed，也会被拒绝。
- [x] 未在 contract `allowed_tools` 中声明的 requested tool 会被拒绝。

### R3 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-manager`

### R3 提交

- [x] `runtime: enforce profile tool policy`

## R4 Multi-profile Step Plan

目标：多 profile 编排显式化，不藏在 `HermesRuntimeClient` 或一次 run/message
调用里。

### R4 代码任务

- [x] 在 `agent-core` 新增 `RuntimeStepPlan`。
- [x] 在 `agent-core` 新增 `RuntimeStep`。
- [x] 在 `agent-core` 新增 `RuntimeStepStatus`。
- [x] 在 Worker 路径创建单个 `RuntimeStep` metadata。
- [x] 在 `agent-runtime` 只执行单个已授权 step。
- [x] 单 step 输出通过 schema 校验后写入 `RuntimeOutput`。
- [x] 当前复用 Runtime metadata、Gateway workflow state 和 audit；后续如需
  跨 gateway 查询，再新增 append-only step audit 表。
- [x] 在 `agent-core` / `RuntimeClient` 中提供完整 `RuntimeStepPlan` 执行器。
- [x] step 输出只通过 schema 校验后的 `output_ref` 进入下一 step。
- [x] 增加多 step 失败降级或终止策略。

### R4 验收

- [x] 每个 profile step 独立可追踪。
- [x] 单 step output 未通过 schema 时不能作为 successful output 返回。
- [x] Runtime 不能自行创建新 step。
- [x] 多 step 失败不会导致权限扩大或未审计输出。

### R4 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-worker`

### R4 提交

- [x] `runtime: add multi-profile step plan`

## R4.5 Runtime Tool Execution Loop

目标：Agent Runtime 本体支持 LLM profile 发起 read-only tool call，并在
真实执行前完成权限、schema、预算和审计约束。

### R4.5 代码任务

- [x] 在 `agent-core` / `agent-runtime` 补齐 `RuntimeToolCall`、
  `RuntimeToolResult`、`RuntimeToolSpec` 和 `RuntimeToolExecutor` contract。
- [x] Runtime adapter 能处理 LLM profile 发起的 read-only tool call。
- [x] per-profile allowed tools 和本次 requested tool scope 在真实 tool
  execution 前强制校验。
- [x] tool input/output 都按 tool spec schema 校验。
- [x] tool output 通过 `output_ref` 和安全摘要传递；大 payload 不进入
  final `RuntimeOutput.metadata`。
- [x] profile step 受最大 tool round 和 `max_runtime_seconds` 预算约束。
- [x] Worker 路径 tool call / tool result 事件接入现有 append-only
  `audit_logs`。
- [x] 越权 tool、写入类 tool 或未知 tool 返回安全错误。
- [x] 写入类工具仍只能走 Manager external-action apply/compensate。
- [x] Runtime 只向 Hermes 暴露 read-only effective tool specs。
- [x] Runtime adapter 提供直连场景可配置的 append-only JSONL audit sink。

### R4.5 验收

- [x] 授权 tool call 会执行，并把安全 tool result 回灌给 profile。
- [x] 未授权或 denied tool call 在执行前被拒绝。
- [x] 不在本次 requested tool scope 内的 tool call 在执行前被拒绝。
- [x] non-read-only tool scope 在执行前被拒绝。
- [x] tool output schema invalid 时不会回灌给 profile 或形成 successful
  step output。
- [x] final metadata 只保留 tool result ref、schema、summary 和 trace 信息。
- [x] 超出 tool round 或 runtime budget 时返回安全错误。
- [x] Worker audit logs 能按 run trace 看到 runtime tool call / result 事件。
- [x] Runtime adapter 直连 JSONL audit sink 有回归验证。

### R4.5 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-worker`

### R4.5 提交

- [x] `runtime: implement profile tool execution`
- [x] `runtime: audit profile tool execution`

## 完成口径

Runtime 专项完成后可以声明：

- [x] Agent Platform 已具备 P1 真实 Hermes Runtime 只读闭环。
- [x] Agent Platform 已具备 P2 external-action 执行链路。
- [x] Agent Runtime 已具备 streaming events、轻量 schema、tool policy、
  multi-profile step plan 和 read-only tool execution loop repo/local 实现。

Runtime 专项完成条件：

- [x] R1 到 R4.5 repo/local checkbox 全部关闭。
- [x] 本次 Runtime/Core/Worker/Manager 对应测试命令通过。
- [x] `PROGRESS.md` 已更新当前状态和残余风险。

领域 Gateway 接入的完成口径、测试和 Open WebUI 复测由对应领域设计文档
维护；通灵玉接入不再记录在本 Runtime checklist 中。
