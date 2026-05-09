# Agent Platform 进展

## 当前状态

- P0 代码基线已实现并通过 `cargo test --manifest-path agent-platform/Cargo.toml`。
- P1 implementation gap 已补齐，本地和远端 Docker 测试均已通过，正式部署
  smoke 已完成。
- P2 仓库侧实现完成；默认部署仍关闭写入，真实第三方 provider / connector
  需要在目标环境配置并运行 contract smoke。
- Bridge hardening 已单独记录，完成口径以 hardening checklist 和部署复测为准。

## 详细记录

- P0：`P0_IMPLEMENTATION_CHECKLIST.md`
- P1：`P1_IMPLEMENTATION_CHECKLIST.md`
- P2：`P2_IMPLEMENTATION_CHECKLIST.md`
- Bridge hardening：`BRIDGE_HARDENING_CHECKLIST.md`
- 阶段路线：`08-implementation-roadmap.md`

## 当前边界

- P0/P1/P2 不应重写 Manager 授权、Open WebUI Bridge、run/session 状态机、
  Worker claim、Memory schema 或 audit contract。
- P2 默认关闭真实写入；仓库内只提供通用 HTTP contract 和低风险
  `action-journal` target 用于 smoke。
- 真实第三方目标是否可用，以目标环境 contract smoke 输出为准。
