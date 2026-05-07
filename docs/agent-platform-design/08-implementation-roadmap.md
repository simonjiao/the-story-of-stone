# 实施路线与 TODO

本文件记录实现阶段拆分。目标是尽早形成端到端闭环，同时避免 Runtime、外部系统和副作用能力绕过 Agent Manager。

## P0 控制面 + Minimal Runtime 闭环

状态：TODO

目标：

```text
Open WebUI / Orchestrator → Manager → Session / Run → Worker → Minimal Runtime → Memory → Audit → Observer Report
```

必须实现：

```text
1. Rust workspace：agent-core、agent-store、agent-manager、agent-orchestrator、agent-runtime、agent-worker、agentctl。
2. Postgres migration 和 P0 核心表。
3. Agent Manager user/admin/internal API。
4. service token + user claims 双主体授权。
5. RBAC + ABAC 授权骨架、resource allowlist、policy check。
6. agent_request / agent_instance / agent_session / agent_run 状态机。
7. Orchestrator 路由、安全错误摘要、session binding、streaming。
8. Memory / Session Store 的 message append、summary 占位、result_ref。
9. Worker 的 Postgres claim、heartbeat、timeout、retry、dead_letter、resource lock。
10. Minimal Runtime：AgentRuntime trait、RuntimeClient、mock/local echo/read-only profile。
11. agentctl：request list / approve / deny / agents list / pause / resume / audit / observer reports。
12. Observer Agent：只读 snapshot、observer run、observer_report 持久化、audit 关联。
13. 扩展边界：RuntimeClient、MemoryStore、ConnectorClient、RunQueue、Telemetry facade。
14. P0 可观测性：trace_id、tracing span、metrics name/label、audit trace 关联；OpenTelemetry/Prometheus exporter 可 feature-gated。
```

明确不做：

```text
1. 不接真实外部系统写入。
2. 不注入写权限 credential。
3. 不做自动 merge / release / deploy。
4. 不做多级 child session。
5. 不让 Observer Agent 执行控制动作。
6. 不把 Redis 作为 P0 正确性依赖。
7. 不让 ORM、gRPC、GraphQL 或外部 memory SDK 决定 P0 架构。
```

P0 验收：

```text
1. 可以创建受控 agent request，并返回 fulfilled / approval_required / denied。
2. 可以创建 agent_session 并追加消息。
3. 可以创建 agent_run，由 worker claim 后调用 Minimal Runtime。
4. run、session、audit、observer_report 可以串起来追踪。
5. Observer Agent 可以报告系统健康、失败、延迟、锁占用和建议，但不能改变系统状态。
6. P1/P2 所需的 transport、memory、connector、queue、telemetry 扩展点已经存在，后续新增实现不需要重写 domain model、API contract 或状态机。
```

### P0 编码前置约束

以下约束需要先记入 P0，并在 Rust workspace scaffold 时细化成 trait、模块、指标名和测试。设计阶段不继续展开到完整代码签名，避免在没有代码结构前过早冻结实现细节。

| 约束 | P0 必须落地 | 编码时细化 |
|---|---|---|
| 扩展边界真实存在 | `RuntimeClient`、`MemoryStore`、`ConnectorClient`、`RunQueue`、`Telemetry` facade 必须进入 `agent-core` 或对应 crate 边界 | trait 方法、错误类型、mock 实现、contract test |
| 可观测性从第一版开始 | `trace_id` 贯穿 request / session / run / audit / observer_report；关键状态迁移有 span；核心指标名和 label 固定 | metrics 命名、span 字段、audit trace 关联、慢请求/锁等待/retry/dead_letter 指标 |
| 后续依赖只能 adapter 化 | Redis、gRPC、GraphQL、外部 memory provider、OpenTelemetry exporter 不得反向改变 Manager 授权、状态机、API contract 或审计模型 | feature gate、adapter crate、兼容测试、禁止绕过 Manager 的集成测试 |

P0 开始编码前，如果上述任一项不能在 scaffold 中落地，应先调整 workspace 结构，而不是继续写业务 API。

## P1 接真实 Hermes Runtime，但只读

状态：TODO

目标：

```text
把 Minimal Runtime 替换或扩展为真实 Hermes Runtime 调用，但只允许只读能力。
```

必须实现：

```text
1. HermesRuntimeClient 实现 RuntimeClient trait。
2. 专用 Hermes Agent Profile 的 session 对话能力。
3. read-only tool / connector 调用。
4. Runtime response streaming 到 Orchestrator。
5. Runtime 错误归一化、timeout、retry、trace_id 贯穿。
6. Memory / Session Store 保存真实 Runtime 返回摘要和 result_ref。
7. Observer Agent 评测真实 Runtime 的延迟、失败、重试、上下文膨胀和结果质量。
```

硬性限制：

```text
1. Hermes Runtime 不持有写权限 credential。
2. 不执行外部 side effect。
3. 不允许 Runtime 绕过 Manager 获取权限或资源。
4. 不向用户返回完整 prompt、完整 context、内部日志或 credential。
5. 不因接入真实 Hermes Runtime 而修改 Manager 授权模型、run/session 状态机或 Memory schema。
```

P1 验收：

```text
1. agent_session 可以和真实 Hermes Runtime 长时间对话。
2. agent_run 可以调用真实 Hermes Runtime 完成只读分析。
3. 所有 Runtime 调用都能追溯到 request / user / service / session / run / audit。
4. Observer Agent 能识别 Runtime 异常、性能退化和高风险建议。
5. 接入 Hermes Runtime 只新增 RuntimeClient 实现和配置，不重写 Orchestrator / Manager / Worker 的核心 contract。
```

## P2 开放受控外部执行

状态：TODO

目标：

```text
在 Manager 策略、审批、资源锁、最小权限 credential 和审计都稳定后，开放受控外部执行。
```

必须实现：

```text
1. 外部 connector 写入接口。
2. 最小权限 credential 注入和过期。
3. side_effect_mode、risk_level、approval_required 策略。
4. resource_locks 的写入侧强制校验。
5. side effect 前后审计和结果校验。
6. retry / timeout / dead-letter。
7. side effect rollback / compensation 的最小策略。
8. Observer Agent 对副作用失败率、审批绕过尝试、锁冲突和异常写入模式做评测。
```

硬性限制：

```text
1. 默认不允许自动 merge / release / deploy。
2. 高风险动作默认审批或硬拒绝。
3. 外部写入必须经过 Manager，不允许 Runtime 或 Worker 直接绕过。
4. credential 不进入长期 agent_instance，不进入 prompt，不进入 observer_report。
5. Observer Agent 只能建议，不能自动执行修复。
6. 不因开放外部执行而改变 P0/P1 的 request、run、session、audit 基线模型。
```

P2 验收：

```text
1. 外部执行可以被明确授权、审批、加锁、执行、审计和回溯。
2. 并发写同一 resource 时，只有一个 active side-effect run 可以持有锁。
3. 审批失败、锁冲突、超时和 connector 失败都有明确状态。
4. 每次 side effect 都能追溯到 request / approval / run / resource_lock / audit。
5. 外部 connector 只新增 ConnectorClient 实现、credential scope 和 side-effect policy，不重写 Worker claim、Manager authorization 或 audit contract。
```
