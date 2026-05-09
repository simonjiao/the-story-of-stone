# Agent Platform 设计入口

本文是兼容入口。权威设计已拆分到 [docs/agent-platform-design/](agent-platform-design/)，阅读顺序和文档职责以 [00-overview.md](agent-platform-design/00-overview.md) 为准。

常用入口：

| 目标 | 文档 |
|---|---|
| 总览、术语和文档职责 | [00-overview.md](agent-platform-design/00-overview.md) |
| 不可破坏边界 | [01-design-principles.md](agent-platform-design/01-design-principles.md) |
| 组件关系和调用方向 | [02-architecture.md](agent-platform-design/02-architecture.md) |
| 用户、管理员和 Open WebUI 可见行为 | [03-product-external-definition.md](agent-platform-design/03-product-external-definition.md) |
| 内部对象、状态和权限 | [04-internal-definition.md](agent-platform-design/04-internal-definition.md) |
| Rust、API、schema 和 Worker 落地 | [05-technical-implementation.md](agent-platform-design/05-technical-implementation.md) |
| 不做什么 | [06-negative-list.md](agent-platform-design/06-negative-list.md) |
| 覆盖检查和验收 | [07-scenarios-decisions-acceptance.md](agent-platform-design/07-scenarios-decisions-acceptance.md) |
| P0/P1/P2 路线图 | [08-implementation-roadmap.md](agent-platform-design/08-implementation-roadmap.md) |
| Bridge hardening 执行记录 | [BRIDGE_HARDENING_CHECKLIST.md](agent-platform-design/BRIDGE_HARDENING_CHECKLIST.md) |

阶段摘要：

```text
P0 代码基线已实现：控制面、Open WebUI Agent Identity Bridge、Minimal Runtime、Worker、Observer 和 audit 最小闭环；Bridge 环境完成口径以 hardening checklist 和部署复测为准。
P1 目标：在现有 Bridge/session/run 链路上接真实 Hermes Runtime，保持只读，并支持 observer_report discussion。
P2 目标：沿用 P1 固定 contract，启用受控外部写入。
```
