# Agent Runtime Implementation Checklist

## 文档定位

本 checklist 跟踪 Agent Runtime 完善专项的实施状态。设计边界以
[09-agent-runtime-design.md](09-agent-runtime-design.md) 为准；本文件只记录
`agent-core` / `agent-runtime` 范围内的可执行任务、验收、测试和待确认项。
Manager、Worker、Orchestrator 和领域 Gateway 只作为调用方或集成边界引用，
不计入本 checklist 的完成条件。

## 当前状态

- [x] R0 文档整合完成。
- [x] R1 Profile Contract 和轻量 Schema Validation。
- [x] R2 Runtime Streaming 基础事件和安全 error event。
- [x] R2 tool progress / schema partial streaming 扩展。
- [x] R3 Per-profile Tool Permission Enforcement。
- [x] R4 Step Plan 数据模型和单 step metadata。
- [x] R4 Multi-profile Step Plan 执行器、step contract、依赖、fallback 和
  output_ref 流转。
- [x] R4.5 Runtime Tool Execution Loop 和成功/失败 tool audit。
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
- [x] 在 `agent-runtime` 增加 `max_context_messages` 上下文预算校验。
- [x] 在 `agent-runtime` 增加确定性 `safety_policy` 子集校验：
  `deny_message_roles` 和 `max_message_bytes`。
- [x] 在 `agent-runtime` 增加 runtime output schema validation。
- [x] Runtime input contract 支持调用方传入 profile contract metadata。
- [x] 当前不需要持久化 contract version；后续如需运营动态配置，再在
  `agent-store` 新增非破坏字段或新表。

### R1 验收

- [x] schema valid 时正常返回 `RuntimeOutput`。
- [x] schema invalid 时返回安全错误。
- [x] schema invalid 不进入 successful runtime output。
- [x] 错误不泄露 prompt、secret、connector payload 或内部栈。
- [x] schema validation 错误不回显输入侧未知字段名/值、schema required
  字段名、schema property path 或 raw value。
- [x] metadata 包含 `profile_id`、`schema_version` 和 `runtime_profile`。
- [x] 超过 `max_context_messages` 时返回安全错误，不进入 successful output。
- [x] `safety_policy` 拒绝指定 message role 或超大消息时返回安全错误。
- [x] 未支持的 `safety_policy` 字段 fail closed，不被静默忽略，也不回显
  未知字段名或字段值。

### R1 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `minimal_runtime_rejects_profile_context_over_budget`
- [x] `minimal_runtime_rejects_profile_safety_denied_role`
- [x] `minimal_runtime_rejects_profile_safety_oversized_message`
- [x] `minimal_runtime_rejects_profile_safety_unknown_field`
  覆盖未知字段名和字段值脱敏。
- [x] `schema_validation_error_omits_controlled_field_names_and_values`

### R1 提交

- [x] `runtime: add profile contract validation`

## R2 Runtime Streaming Events

目标：Runtime 支持无工具 Hermes 上游 SSE 解析和有序 `RuntimeStreamEvent`
输出，同时保留最终 `RuntimeOutput` 作为落盘结果。tool loop streaming
路径的完成口径是执行完整 tool loop 后返回 `tool_progress` /
`schema_partial` / `final` 事件序列，不声明 token 级 SSE 透传。当前完成口径是
`RuntimeClient::stream_*()` 返回完整 event 序列；下游 async stream /
backpressure API 是后续项。

### R2 代码任务

- [x] 在 `agent-core` 新增 `RuntimeStreamEvent`。
- [x] 在 `agent-core` 新增 event-returning streaming trait 边界。
- [x] 在 `agent-runtime` 为 Hermes adapter 增加无工具上游 SSE streaming
  path；tool loop streaming path 复用受控 tool loop 并合成有序 Runtime
  events。
- [x] streaming final event 携带最终 `RuntimeOutput`；Worker 非 streaming
  完成态语义保持不变。
- [x] 保留非 streaming path 的原有行为。
- [x] 增加 tool progress streaming event。
- [x] 增加 schema partial streaming event。
- [x] streaming path 失败时返回安全 `error` event，不泄露 prompt、
  upstream response body、credential、connector payload 或内部栈。

### R2 验收

- [x] 非 streaming path 不回退。
- [x] streaming final 和非 streaming 输出语义一致。
- [x] error event 不泄露 prompt、credential、connector payload 或内部栈。
- [x] stream event 能携带 trace、run/session、profile 和 schema version。
- [x] explicit contract 或 registry contract 解析到版本时，成功和错误 stream
  event 都保留 schema version。
- [x] run、session message 和 profile step 的 tool progress /
  schema partial 有端到端验证。
- [x] safe error event 有回归验证。
- [x] 当前完成口径不声明下游 async stream/backpressure API。

### R2 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `minimal_runtime_stream_registry_contract_error_preserves_schema_version`
- [x] `hermes_runtime_stream_registry_contract_errors_preserve_schema_version`

### R2 提交

- [x] `runtime: add streaming event contract`

## R3 Per-profile Tool Permission Enforcement

目标：profile 工具权限成为真实执行约束，而不是 prompt 约定。

### R3 代码任务

- [x] 在 `agent-core` 定义 tool capability。
- [x] 在 `agent-core` 定义 allowed tools、denied tools 和 effective tool set。
- [x] Runtime input contract 支持调用方传入本次授权 `requested_tools` scope。
- [x] 在 `agent-runtime` 执行前按 `requested_tools ∩ allowed_tools -
  denied_tools - non_read_only` 计算 effective tool set。
- [x] 在 `agent-runtime` 拒绝越权 tool request。
- [x] 在 `agent-core` / `agent-runtime` 增加 `RuntimeToolCapability`，拒绝
  non-read-only tool scope 和 tool call。
- [x] 空 `allowed_tools` 解释为无工具权限，不解释为通配符。
- [x] 保持写入类工具必须走 Manager external-action apply/compensate。
- [x] metadata 记录 effective tool set。
- [x] Runtime 不从 prompt 派生 tool scope，只接受显式 `requested_tools`。

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
- [x] `runtime_tool_policy_rejects_non_read_only_tool_call` 覆盖
  non-read-only tool scope 和 tool call 两层拒绝。

### R3 提交

- [x] `runtime: enforce profile tool policy`

## R4 Multi-profile Step Plan

目标：多 profile 编排显式化，不藏在 `HermesRuntimeClient` 或一次 run/message
调用里。

### R4 代码任务

- [x] 在 `agent-core` 新增 `RuntimeStepPlan`。
- [x] 在 `agent-core` 新增 `RuntimeStep`。
- [x] 在 `agent-core` 新增 `RuntimeStepStatus`。
- [x] Runtime input contract 支持调用方传入单个 `RuntimeStep` metadata。
- [x] 在 `agent-runtime` 只执行单个已授权 step。
- [x] 单 step 输出通过 schema 校验后写入 `RuntimeOutput`。
- [x] 当前复用 Runtime metadata、Gateway workflow state 和 audit；后续如需
  跨 gateway 查询，再新增 append-only step audit 表。
- [x] 在 `agent-core` / `RuntimeClient` 中提供完整 `RuntimeStepPlan` 执行器。
- [x] `RuntimeStep` 携带 step 级 `output_contract` 和 `tool_policy`。
- [x] `RuntimeStepPlan::for_profile_contracts()` 可从 profile contract 创建完整
  step plan。
- [x] step plan 执行器会实际使用 step 级 `output_contract` 和 `tool_policy`。
- [x] `requested_tools_by_profile` 缺省时按空工具 scope 处理，不默认授权
  profile contract 的全部 allowed tools。
- [x] step 输出只通过 schema 校验后的 `output_ref` 进入下一 step。
- [x] 增加多 step 失败降级或终止策略。
- [x] executor 侧 output contract 校验失败、缺失 `output_ref` 或依赖缺失时，
  会按当前 step `fallback_policy` 降级或终止。

### R4 验收

- [x] 每个 profile step 独立可追踪。
- [x] 单 step output 未通过 schema 时不能作为 successful output 返回。
- [x] Runtime 不能自行创建新 step。
- [x] 多 step 失败不会导致权限扩大或未审计输出。
- [x] step 级 tool policy 会收窄本 step 的 effective tool set。
- [x] step plan 未显式传入本次 requested tool scope 时不会默认授权任何工具。
- [x] step 级 output contract 失败时不会产生 successful output。
- [x] step 级 output contract 在 Runtime 客户端成功返回后失败时，也会走
  `fallback_policy`，不会绕过显式降级/终止策略。

### R4 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `runtime_step_plan_helper_materializes_step_contracts`
- [x] `runtime_step_plan_requires_explicit_requested_tool_scope`
- [x] `runtime_step_plan_validates_step_output_contract`
- [x] `runtime_step_plan_applies_fallback_to_executor_output_contract_failure`
- [x] `runtime_step_plan_applies_fallback_to_missing_output_ref`
- [x] `runtime_step_plan_applies_fallback_to_missing_dependency`

### R4 提交

- [x] `runtime: add multi-profile step plan`

## R4.5 Runtime Tool Execution Loop

目标：Agent Runtime 本体支持 LLM profile 发起 read-only tool call，并在
真实执行前完成权限、schema、预算和审计约束。

### R4.5 代码任务

- [x] 在 `agent-core` / `agent-runtime` 补齐 `RuntimeToolCall`、
  `RuntimeToolResult`、`RuntimeToolSpec` 和 `RuntimeToolExecutor` contract。
- [x] Runtime adapter 能处理 run、session message 和 profile step 中
  LLM profile 发起的 read-only tool call。
- [x] per-profile allowed tools 和本次 requested tool scope 在真实 tool
  execution 前强制校验。
- [x] tool input/output 都按 tool spec schema 校验。
- [x] tool output 通过 `output_ref` 和安全摘要传递；raw output 不回灌给
  profile，也不进入 final `RuntimeOutput.metadata`，摘要只暴露类型和长度/数量。
- [x] `RuntimeToolSpec.output_ref_required=true` 时，缺失 `output_ref` 会失败，
  不会被 Runtime 自动补全成 successful tool result。
- [x] run、session message 和 profile step 都受 `max_runtime_seconds`
  预算约束；tool loop 还受最大 tool round 约束。
- [x] RuntimeOutput metadata 返回安全 tool call / tool result event 摘要。
- [x] 越权 tool、写入类 tool 或未知 tool 返回安全错误。
- [x] 写入类工具仍只能走 Manager external-action apply/compensate。
- [x] Runtime 只向 Hermes 的 run、session message 和 profile step 路径暴露
  read-only effective tool specs。
- [x] Runtime adapter 提供直连场景可配置的 append-only JSONL audit sink。
- [x] tool executor 自身失败时，Runtime 包装为安全错误 message，只保留
  error code，不透传 executor error payload。
- [x] tool call 失败时追加安全 `runtime_tool_error` audit event。

### R4.5 验收

- [x] 授权 tool call 会执行，并把安全 tool result 回灌给 profile。
- [x] 未授权、denied、无 profile contract、no-tool run/session
  non-streaming path，或 no-tool run/session/profile streaming path 中的
  hallucinated tool call 在执行前被拒绝。
- [x] 不在本次 requested tool scope 内的 tool call 在执行前被拒绝。
- [x] non-read-only tool scope 在执行前被拒绝。
- [x] malformed tool arguments 时不会执行 tool executor；streaming path
  返回安全 `error` event，并写安全 `runtime_tool_error` audit event。
- [x] tool input schema invalid 时不会执行 tool executor 或形成 successful
  tool result，并写安全 `runtime_tool_error` audit event。
- [x] tool output schema invalid 时不会回灌给 profile 或形成 successful
  step output，并写安全 `runtime_tool_error` audit event。
- [x] tool executor 自身失败时，Runtime 返回安全错误 message；streaming path
  返回安全 `error` event；两者都会写安全 `runtime_tool_error` audit event。
- [x] profile 回灌和 final metadata 只保留 tool result ref、已校验的
  output_schema contract、summary 和 trace 信息，不包含 raw tool output。
- [x] tool executor 返回的 metadata payload 不进入 final metadata 或 adapter audit。
- [x] tool output summary 不包含 raw string、object key 名或 executor metadata
  payload。
- [x] required `output_ref` 缺失时返回安全错误，并写 `runtime_tool_error`
  audit event。
- [x] tool executor 返回的 call/profile/tool 身份不会覆盖 Runtime 已授权 tool call。
- [x] 超出 tool round 时返回安全错误并写 `runtime_tool_error` audit event；
  超出 runtime budget 时返回安全错误。
- [x] streaming run、session message 和 profile step 超出 runtime budget 时返回
  安全 `error` event。
- [x] RuntimeOutput metadata 或 Runtime adapter audit sink 能按 trace 看到
  runtime tool call / result 事件。
- [x] Runtime adapter 直连 JSONL audit sink 有回归验证，且确认已有 JSONL
  记录不会被覆盖。
- [x] 未授权 tool call 的失败 audit 有回归验证，且不包含 tool arguments 或
  raw tool name / raw call id。

### R4.5 测试

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `hermes_runtime_streams_safe_error_event`
- [x] `hermes_runtime_rejects_unauthorized_profile_tool_call`
  覆盖未授权 raw tool name / raw call id 在 call/error audit 中脱敏。
- [x] `hermes_runtime_rejects_tool_call_without_profile_contract`
  覆盖未暴露 tools 且无 profile contract 时的 hallucinated tool call 拒绝和
  audit 脱敏。
- [x] `hermes_runtime_rejects_unrequested_run_and_session_tool_calls_with_safe_audit`
  覆盖 no-tool run/session path 中 hallucinated tool call 的拒绝和 audit 脱敏。
- [x] `hermes_runtime_stream_rejects_unrequested_run_and_session_tool_calls`
  覆盖 no-tool streaming run/session path 中 hallucinated tool call 的安全
  error event 和 audit 脱敏。
- [x] `hermes_runtime_stream_profile_rejects_unrequested_tool_call`
  覆盖 no-tool streaming profile step path 中 hallucinated tool call 的安全
  error event 和 audit 脱敏。
- [x] `hermes_runtime_execute_run_exposes_requested_profile_tools`
- [x] `hermes_runtime_stream_session_with_tools_emits_tool_progress_events`
- [x] `hermes_runtime_stream_run_with_tools_emits_tool_progress_events`
- [x] `hermes_runtime_streams_safe_error_for_expired_profile_budget`
- [x] `hermes_runtime_run_and_session_reject_expired_contract_budget`
- [x] `hermes_runtime_stream_run_and_session_safe_error_for_expired_contract_budget`
- [x] `hermes_runtime_rejects_excessive_tool_rounds` 覆盖 tool round
  violation 的 `runtime_tool_error` audit event。
- [x] `hermes_runtime_rejects_invalid_tool_input_schema_with_safe_audit`
  覆盖 tool input schema invalid 的安全错误和 audit 不泄漏 arguments。
- [x] `hermes_runtime_streams_safe_error_for_malformed_tool_arguments`
  覆盖 malformed arguments 不执行 executor、streaming safe error event 和
  audit 不泄漏 arguments。
- [x] `hermes_runtime_sanitizes_tool_executor_failure_with_safe_audit`
  覆盖 tool executor error payload 不进入 Runtime 错误 message 或 audit。
- [x] `hermes_runtime_stream_run_safe_error_for_tool_executor_failure`
  覆盖 streaming tool loop 的 executor failure 不泄漏 error payload，并保留
  run/schema/audit 边界。
- [x] `hermes_runtime_rejects_invalid_tool_output_schema_with_safe_audit`
  覆盖 tool output schema invalid 不回灌、不形成 result event、audit 不泄漏
  raw output。
- [x] `hermes_runtime_omits_tool_metadata_payload_from_metadata_and_audit`
  覆盖 profile 回灌、executor metadata、raw string output summary 和 adapter
  audit 不泄漏。
- [x] tool result metadata / adapter audit 覆盖已校验 output_schema contract。
- [x] `hermes_runtime_rejects_required_tool_output_ref_missing`
- [x] `hermes_runtime_writes_tool_events_to_jsonl_audit_sink` 覆盖 append-only
  保留已有记录。

### R4.5 提交

- [x] `runtime: implement profile tool execution`
- [x] `runtime: audit profile tool execution`

## 后续项

以下条目不计入当前 Agent Runtime repo/local 完成口径，只有在后续设计把它们
升为本体要求时才进入必做 checklist：

- [ ] 如需要完整 JSON Schema，替换当前轻量校验器或接入标准 schema crate。
- [ ] 如调用方需要边读边转发 Runtime event，新增 object-safe async stream
  或 callback API，并补充 backpressure / cancellation 验证。

## 完成口径

Runtime 专项完成后可以声明：

- [x] Agent Platform 已具备 P1 真实 Hermes Runtime 只读闭环。
- [x] Agent Platform 已具备 P2 external-action 执行链路。
- [x] Agent Runtime 已具备 streaming events、轻量 schema、tool policy、
  multi-profile step plan 和 read-only tool execution loop repo/local 实现。

Runtime 专项完成条件：

- [x] R1 到 R4.5 repo/local checkbox 全部关闭。
- [x] 本次 `agent-core` / `agent-runtime` 对应测试命令通过。
- [x] `PROGRESS.md` 已更新当前状态和残余风险。

领域 Gateway 接入的完成口径、测试和 Open WebUI 复测由对应领域设计文档
维护；通灵玉接入不再记录在本 Runtime checklist 中。
