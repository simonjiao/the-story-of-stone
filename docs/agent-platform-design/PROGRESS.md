# Agent Platform 进展

## 当前状态

- P0 代码基线已实现并通过 `cargo test --manifest-path agent-platform/Cargo.toml`。
- P1 implementation gap 已补齐，本地和远端 Docker 测试均已通过，正式部署
  smoke 已完成。
- P2 仓库侧实现完成；默认部署仍关闭写入，真实第三方 provider / connector
  需要在目标环境配置并运行 contract smoke。
- Bridge hardening 已单独记录，完成口径以 hardening checklist 和部署复测为准。
- Agent Runtime 完善专项已完成 R1 到 R4.5 的 repo/local 实现：
  profile contract、轻量 schema validation、Runtime streaming events、
  per-profile tool permission、multi-profile step plan 和 read-only tool loop
  已落地；`max_context_messages` 会作为 Runtime 输入预算约束执行，
  确定性 `safety_policy` 子集会在进入模型前执行，streaming 失败会返回
  安全 `error` event；schema validation 错误不回显输入侧未知字段名/值、
  schema required 字段名、schema property path 或 raw value，未知
  `safety_policy` 字段也不会回显字段名或值。
  当前 streaming 完成口径是有序 event 序列；无工具路径解析 Hermes 上游
  SSE，tool loop 路径执行完整受控 tool loop 后合成 Runtime events，不声明
  调用方可边读边转发的 async stream/backpressure API；explicit contract
  或 registry contract 解析到版本时，成功和错误 stream event 都保留
  schema version。
- R4 Multi-profile Step Plan 已补齐完整 step contract：`RuntimeStep` 携带
  `output_contract` 和 `tool_policy`，`RuntimeStepPlan::for_profile_contracts()`
  可从 profile contract 创建完整 plan，执行器会用 step 级 schema/tool policy
  校验输出和收窄工具 scope；`requested_tools_by_profile` 缺省时按空工具 scope
  处理，不会默认授权 profile contract 的全部 allowed tools；依赖缺失、缺失
  `output_ref` 或 executor 侧 output contract 失败也会按 step `fallback_policy`
  降级或终止。
- R4.5 Runtime Tool Execution Loop 已落地：Runtime 已有 tool call /
  tool result / tool executor contract，Hermes run、session message 和
  profile step 路径可执行 OpenAI-compatible tool loop，并在真实 tool
  execution 前校验 requested tool scope、per-profile tool permission、
  read-only capability、tool schema、tool round、runtime budget 和
  output ref/summary 约束；普通和 streaming 的 run、session message、
  profile step 超出 runtime budget 都会返回安全错误；profile 回灌、
  RuntimeOutput metadata 和 adapter audit 只保留安全 tool event 摘要、
  已校验的 output_schema contract 与 output_ref，required output_ref 缺失会
  失败并写安全错误事件，tool round violation 也会写安全错误事件；tool output
  summary 只暴露类型和长度/数量，不透传 tool executor metadata payload、
  raw string output 或 object key 名，且不会让 executor 覆盖已授权 tool call
  身份；未授权/未知 tool name 和 call id 会在失败 audit 中脱敏；
  malformed arguments 和 tool input/output schema invalid 有直接回归验证，
  失败时不会执行越界 executor、回灌 raw arguments/output 或形成
  successful tool result；tool call 失败会写安全 `runtime_tool_error`
  adapter audit event；Runtime adapter 也提供直连场景可配置的 append-only
  JSONL audit sink，并有保留已有记录的回归验证。
- Runtime repo/local checklist 当前已关闭；完成口径限定为 Agent Runtime 本体，
  完整 JSON Schema 和领域 Gateway 接入复测不属于 Agent Runtime 本体完成条件。
- 领域 Gateway 接入不再作为 Agent Runtime 专项完成条件；通灵玉 Runtime
  接入设计和复测口径迁入 `docs/tonglingyu-agent-design/`。

## 详细记录

- P0：`P0_IMPLEMENTATION_CHECKLIST.md`
- P1：`P1_IMPLEMENTATION_CHECKLIST.md`
- P2：`P2_IMPLEMENTATION_CHECKLIST.md`
- Bridge hardening：`BRIDGE_HARDENING_CHECKLIST.md`
- 阶段路线：`08-implementation-roadmap.md`
- Runtime 专项：`09-agent-runtime-design.md`
- Runtime checklist：`AGENT_RUNTIME_IMPLEMENTATION_CHECKLIST.md`

## 当前边界

- P0/P1/P2 不应重写 Manager 授权、Open WebUI Bridge、run/session 状态机、
  Worker claim、Memory schema 或 audit contract。
- P2 默认关闭真实写入；仓库内只提供通用 HTTP contract 和低风险
  `action-journal` target 用于 smoke。
- 真实第三方目标是否可用，以目标环境 contract smoke 输出为准。
- Runtime 专项只新增 adapter、contract 或 feature；未改写 P0/P1/P2 已固定的
  Manager 授权、Open WebUI Bridge、run/session 状态机、Worker claim、
  Memory schema 或 audit contract。
- Runtime 本体完成口径限定在 `agent-core` / `agent-runtime`；Manager、
  Worker、Orchestrator 和领域 Gateway 的消费、落审计、SSE 包装和部署复测
  属于各自集成边界。
- Agent Runtime repo/local 完成状态不能替代任何领域 Gateway 接入完成状态；通灵玉
  目标架构、四 profile contract、read-only tools 和 Open WebUI 复测以
  通灵玉设计文档为准。
