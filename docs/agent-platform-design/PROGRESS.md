# Agent Platform 进展

## 当前状态

- P0 代码基线已实现并通过 `cargo test --manifest-path agent-platform/Cargo.toml`。
- P1 implementation gap 已补齐，本地和远端 Docker 测试均已通过，正式部署
  smoke 已完成。
- P2 仓库侧实现完成；默认部署仍关闭写入，真实第三方 provider / connector
  需要在目标环境配置并运行 contract smoke。
- Bridge hardening 已单独记录，完成口径以 hardening checklist 和部署复测为准。
- Agent Runtime 完善专项已完成 R1 到 R4.5 repo/local 实现：profile contract、
  schema validation、Runtime streaming、per-profile tool permission 和
  multi-profile step plan 已落地。
- R4.5 Runtime Tool Execution Loop 已落地：Runtime 已有 tool call /
  tool result / tool executor contract，Hermes profile step 可执行
  OpenAI-compatible tool loop，并在真实 tool execution 前校验 per-profile
  tool permission、tool schema、tool round、runtime budget 和 output ref/summary
  约束；Worker 会把 tool call / tool result 写入现有 append-only audit logs。
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
- Agent Runtime 完成状态不能替代任何领域 Gateway 接入完成状态；通灵玉
  目标架构、四 profile contract、read-only tools 和 Open WebUI 复测以
  通灵玉设计文档为准。
