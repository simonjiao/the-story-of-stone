# Agent Platform 进展

## 当前状态

- P0 代码基线已实现并通过 `cargo test --manifest-path agent-platform/Cargo.toml`。
- P1 implementation gap 已补齐，本地和远端 Docker 测试均已通过，正式部署
  smoke 已完成。
- P2 仓库侧实现完成；默认部署仍关闭写入，真实第三方 provider / connector
  需要在目标环境配置并运行 contract smoke。
- Bridge hardening 已单独记录，完成口径以 hardening checklist 和部署复测为准。
- Agent Runtime 完善专项已完成 R1 到 R4 repo/local 实现：profile contract、
  schema validation、Runtime streaming、per-profile tool permission 和
  multi-profile step plan 已落地。
- R5 通灵玉接入已按“薄 Gateway + Runtime Agent”目标重新打开；旧的
  Gateway 内检索、证据包、reviewer 和确定性 text/commentary step 不再作为
  完成口径。
- 通灵玉目标环境 Open WebUI 单入口复测仍需在 R5 完成后执行。

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
- 通灵玉目标架构是薄 Gateway + Runtime Agent：Gateway 只做协议、鉴权、
  路由、trace/session、SSE、模型隐藏和响应封装；正文/脂批检索、证据包、
  reviewer 和 replay 归 Runtime profile 与 read-only tools 负责。
- `honglou-text` 和 `honglou-commentary` 走 LLM profile，不再按确定性
  Gateway 检索 step 作为目标方案。
- 完成口径仍保留 R5 和部署侧限制：未完成薄 Gateway 改造和目标环境
  Open WebUI 单入口复测前，不能宣称通灵玉 Runtime 接入完成。
