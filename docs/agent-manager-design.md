# Agent Platform 设计文档

本文档是 Agent Platform 设计入口。详细设计按职责拆分在 `docs/agent-platform-design/`。

## 文档结构

| 文档 | 内容定位 |
|---|---|
| [00-overview.md](agent-platform-design/00-overview.md) | 总览、组件边界、核心不变量、文档职责 |
| [01-design-principles.md](agent-platform-design/01-design-principles.md) | 设计原则、安全边界、禁止事项 |
| [02-architecture.md](agent-platform-design/02-architecture.md) | 总体架构、组件职责、网络拓扑、调用方向 |
| [03-product-external-definition.md](agent-platform-design/03-product-external-definition.md) | 面向用户、resource owner、管理员和 Open WebUI 的外部行为 |
| [04-internal-definition.md](agent-platform-design/04-internal-definition.md) | 内部对象、状态机、权限模型、隔离规则 |
| [05-technical-implementation.md](agent-platform-design/05-technical-implementation.md) | Rust workspace、crate、API、Postgres schema、并发和 Worker |
| [06-negative-list.md](agent-platform-design/06-negative-list.md) | 明确禁止、不建议、延期实现项 |
| [07-scenarios-decisions-acceptance.md](agent-platform-design/07-scenarios-decisions-acceptance.md) | 原则覆盖、场景验证、决策清单、验收标准 |
| [08-implementation-roadmap.md](agent-platform-design/08-implementation-roadmap.md) | P0/P1/P2 实施路线与 TODO |

## 核心不变量

```text
1. Open WebUI 只连接 Agent Orchestrator / Gateway。
2. Agent Manager 是唯一控制面。
3. Orchestrator 只路由，不执行任务、不持有目标 Agent credential。
4. Runtime 只执行已授权 session/run。
5. Worker 只 claim 和推进 run，不绕过 Manager。
6. Agent instance 复用能力配置，不复用 session、run、credential、memory 或 workdir。
7. 长交互用 agent_session，单次执行用 agent_run。
8. Child session v1 只允许一层。
9. Observer Agent 是 v1 必须内置的只读系统观察 Agent，只输出报告和建议。
10. 所有副作用必须经过 Manager 策略、审批和资源锁。
```

## 实施阶段

```text
P0：控制面 + Open WebUI Agent Identity Bridge + Minimal Runtime 闭环 + Observer Agent。
P1：在现有 Bridge/session/run 链路上接真实 Hermes Runtime，但只读。
P2：开放受控外部执行。
```

实现前应先检查 [07-scenarios-decisions-acceptance.md](agent-platform-design/07-scenarios-decisions-acceptance.md) 的场景覆盖和 [08-implementation-roadmap.md](agent-platform-design/08-implementation-roadmap.md) 的阶段 TODO。
