# Agent Platform 总览

## 目标

本文档集定义一套基于 Hermes Agent 的多 Agent 平台。平台目标是让用户可以在 Open WebUI 中发起普通聊天、创建或复用专用 Agent、与已启动 Agent 长时间交互、启动受控 child session，并由后台 Worker 执行 run，同时保持控制面、credential、审计和外部副作用不暴露给 Open WebUI。

v1 必须包含一个只读 `observer_agent`。它持续监控和评测系统运行，生成建议和 `observer_report`，但不能审批、授权、暂停、恢复、删除、修改配置或执行外部写入。

## 组件边界

以下组件名是本文档集的统一术语。后续文档不得改变这些职责边界。

| 组件 | 一句话职责 |
|---|---|
| Open WebUI | 用户聊天界面，只连接 Orchestrator |
| Agent Orchestrator / Gateway | 用户入口、路由、session binding、流式转发和安全错误摘要 |
| Agent Manager | 控制面，负责授权、策略、审批、生命周期、审计和资源锁决策 |
| Agent Runtime | 执行面，承载 session/run，调用 Hermes profile 和工具适配 |
| Memory / Session Store | 保存 session、message、summary、result_ref 和上下文索引 |
| Worker / Scheduler | claim run、heartbeat、timeout、retry、dead-letter 和调度 |
| Observer Agent | 只读观察、评测和建议，不执行控制动作 |
| Default Hermes Agent Profile | 普通聊天和意图澄清 |
| 专用 Hermes Agent Profile | 专用任务执行，由 Runtime 在授权上下文中调用 |
| agentctl CLI | 管理员审批、查询、暂停、恢复和审计入口 |

## 核心不变量

```text
1. Open WebUI 只连接 Agent Orchestrator / Gateway。
2. Agent Manager、Runtime、Worker、Observer 和存储服务只在内网。
3. Orchestrator 不执行任务、不持有目标 Agent credential、不保存长期上下文。
4. Manager 是唯一授权与策略决策点。
5. Runtime 只执行已授权 session/run，不决定权限边界。
6. Worker 只 claim 和推进 run，不绕过 Manager 策略。
7. Agent instance 复用能力配置，不复用 session、run、credential、memory 或 workdir。
8. 长交互用 `agent_session`，单次执行用 `agent_run`。
9. Child session v1 只允许一层，不支持嵌套。
10. Observer Agent 只读，只输出报告和建议。
11. 所有副作用必须经过 Manager 策略、审批和资源锁。
```

## v1 最小落地范围

```text
1. Agent Orchestrator / Gateway 作为 Open WebUI 唯一后台入口。
2. Agent Manager 内网部署。
3. Agent Runtime 的 Minimal Runtime 闭环。
4. Memory / Session Store 支持 session、message、summary 和 result_ref。
5. Worker / Scheduler 支持 run claim、heartbeat、timeout、resource lock 和 dead-letter。
6. agentctl CLI 支持 requests / agents / audit / observer reports。
7. service token + user claims 双主体授权。
8. agent_requests / agent_instances / agent_sessions / agent_runs / audit_logs / observer_reports 核心状态。
9. RuntimeClient / MemoryStore / ConnectorClient / RunQueue / Telemetry facade 扩展边界。
10. P0 可观测性：trace_id、tracing span、metrics name/label、audit trace 关联。
11. `background_worker` 通用 Agent template。
12. `observer_agent` 只读系统观察 Agent template。
13. resource allowlist。
14. manual / scheduled / webhook / session message 四类触发入口。
15. 默认禁止未审批外部写入。
16. 管理员审批 create_persistent_agent。
```

## 文档职责

| 文档 | 权威范围 |
|---|---|
| [01-design-principles.md](01-design-principles.md) | 不可破坏的原则、安全边界和硬性禁止 |
| [02-architecture.md](02-architecture.md) | 组件关系、网络拓扑和调用方向 |
| [03-product-external-definition.md](03-product-external-definition.md) | 用户、resource owner、管理员和 Open WebUI 可见行为 |
| [04-internal-definition.md](04-internal-definition.md) | 对象、状态机、权限上下文、隔离和复用规则 |
| [05-technical-implementation.md](05-technical-implementation.md) | Rust 模块、依赖、API、数据表、并发和 Worker 机制 |
| [06-negative-list.md](06-negative-list.md) | 明确禁止、不建议、延期实现和延期原因 |
| [07-scenarios-decisions-acceptance.md](07-scenarios-decisions-acceptance.md) | 场景覆盖、决策清单和验收标准 |
| [08-implementation-roadmap.md](08-implementation-roadmap.md) | P0/P1/P2 实施 TODO 和阶段验收 |

一致性规则：

```text
1. 原则文档优先于产品体验和技术便利。
2. 架构文档优先于单个 API 或模块命名。
3. 内部定义必须能解释所有外部行为。
4. 技术实现必须能落地内部定义，不得新增绕过原则的隐式能力。
5. 负面清单优先于功能愿望。
6. 场景文档用于验证覆盖，不引入新的设计规则。
```

## 阅读顺序

1. 先读 [01-design-principles.md](01-design-principles.md)。
2. 再读 [02-architecture.md](02-architecture.md)。
3. 产品和交互读 [03-product-external-definition.md](03-product-external-definition.md)。
4. 对象、状态和权限读 [04-internal-definition.md](04-internal-definition.md)。
5. 工程落地读 [05-technical-implementation.md](05-technical-implementation.md)。
6. 不做什么读 [06-negative-list.md](06-negative-list.md)。
7. 覆盖检查读 [07-scenarios-decisions-acceptance.md](07-scenarios-decisions-acceptance.md)。
8. 实施拆分读 [08-implementation-roadmap.md](08-implementation-roadmap.md)。
