# 实施路线与 TODO

本文件只记录阶段边界、交付顺序和验收标准。设计规则以 [01-design-principles.md](01-design-principles.md) 为准；内部对象以 [04-internal-definition.md](04-internal-definition.md) 为准；API 和 schema 以 [05-technical-implementation.md](05-technical-implementation.md) 为准。

## 阶段边界

| 阶段 | 状态 | 目标 | 不变边界 |
|---|---|---|---|
| P0 | 已实现 | 控制面、Minimal Runtime、Worker、Observer、audit 形成最小闭环 | Postgres lease / SKIP LOCKED 是正确性边界；不接真实写 connector；不注入写权限 credential |
| P1 | TODO | 接入真实 Hermes Runtime，只做只读 session/run，并让 Observer report 可进入受控 discussion session | 不改 Manager 授权、run/session 状态机、Worker claim、Memory schema、audit contract |
| P2 | TODO | 启用受控外部写入 | 只启用 P1 已固定的真实 WriteConnector / CredentialProvider adapter，不把 P2 变成重构阶段 |

P1 会提前落地 P2 需要的 side-effect plan、credential lease、write connector contract、dry-run policy、no-op provider 和审计事件。P1 只 dry-run / validate / reject，不获取真实 credential，不调用真实写 connector，不把 run 推进到真实外部写入。

## P0 控制面闭环

P0 已具备：

```text
Open WebUI / Orchestrator
  → Manager
  → Session / Run
  → Worker
  → Minimal Runtime
  → Memory
  → Audit
  → Observer Report
```

实现范围：

```text
1. Rust workspace：agent-core、agent-store、agent-manager、agent-orchestrator、agent-runtime、agent-worker、agentctl。
2. Postgres migration、核心表、idempotency、resource lock、run lease、retry、dead_letter。
3. Manager user/admin/internal API，包含 request、agent、run、audit、observer report 管理。
4. Orchestrator 安全路由、session binding、streaming 和安全错误摘要。
5. Minimal Runtime、Memory / Session Store、Worker claim/heartbeat/finish。
6. observer_agent 只读 snapshot、observer_report 持久化和 audit 关联。
7. RuntimeClient、MemoryStore、ConnectorClient、RunQueue、Telemetry facade。
```

P0 验收：

```text
1. 可以创建受控 agent request，并返回 fulfilled / approval_required / denied。
2. 可以创建 agent_session 并追加消息。
3. 可以创建 agent_run，由 worker claim 后调用 Minimal Runtime。
4. run、session、audit、observer_report 可以用 trace_id 串起来追踪。
5. Observer 只能报告系统健康、失败、延迟、锁占用和建议，不能改变系统状态。
6. P1/P2 只需新增 adapter、feature 或非破坏字段，不需要重写核心 contract。
```

## P1 真实 Hermes Runtime，只读

P1 目标是让平台开始使用真实 Hermes Runtime，同时保持控制面和副作用边界不变。

交付顺序：

| 阶段 | 目标 | 主要交付 | 验证重点 |
|---|---|---|---|
| P1.0 Contract freeze | 固定不可改边界 | 为 RuntimeClient、ConnectorClient、Telemetry、安全错误增加 contract test；确认 request/run/session/audit 无破坏性 schema change | `cargo test --manifest-path agent-platform/Cargo.toml`；schema diff 只允许新增配置或非破坏字段 |
| P1.1 Hermes adapter | 新增真实 Runtime adapter | `HermesRuntimeClient`、Hermes HTTP client、profile config、timeout、safe error mapping、`AGENT_RUNTIME_MODE=minimal|hermes` | unit/wiremock 覆盖成功、timeout、5xx、malformed response 和 trace_id |
| P1.2 Session path | 真实 Hermes 承载长 session | bound conversation → `agent_session` → Runtime → assistant message/result_ref → Orchestrator streaming | 登录、模型选择、基础聊天、会话保存；响应不含 prompt/context/credential |
| P1.3 Run path | 真实 Hermes 执行只读 run | Worker 按 profile 选择 Hermes adapter，构建上下文和只读 snapshot，finish 写 summary/result_ref/audit | claim、heartbeat、timeout、retry、dead_letter、finish audit 状态机不变 |
| P1.4 Read-only connector | 只读工具与 snapshot | `ConnectorClient::read_only_snapshot` 返回摘要或 payload_ref；Runtime 只消费只读引用 | `side_effect_mode=authorized` 仍拒绝真实写入 |
| P1.5 Observer upgrade | 评测真实 Runtime 行为 | observer snapshot 增加 latency、timeout、retry、context size、异常建议和结果质量摘要 | report 只生成 findings/recommendations，不触发控制动作 |
| P1.6 Report discussion | 快速讨论报告并形成后续需求 | `POST /v1/admin/observer/reports/{report_id}/discussions` 和 agentctl 命令；Manager 创建普通 `agent_session` | 权限、脱敏、session/audit 关联；Observer 不参与对话 |
| P1.7 P2 readiness | 避免 P2 重构 | SideEffectPlanner / CredentialProvider / WriteConnector contract，`side_effect_plans` / `credential_leases` migration，dry-run policy，no-op provider/write connector，审计事件 | P1 只能 dry-run 或拒绝；no-op provider 不产生 secret |
| P1.8 Smoke | 本地和部署回归 | 示例 env / runbook 覆盖 Hermes URL、profile、mode、report discussion、P2 readiness dry-run | 最小关键路径 + Hermes smoke + report discussion + P2 readiness dry-run + Postgres smoke |

P1 验收：

```text
1. `agent_session` 可以和真实 Hermes Runtime 多轮对话。
2. `agent_run` 可以调用真实 Hermes Runtime 完成只读分析。
3. Runtime 不持有写权限 credential，不执行外部 side effect。
4. 所有 Runtime 调用都能追溯到 request / user / service / session / run / audit / trace_id。
5. Observer 能识别 Runtime 异常、性能退化、上下文膨胀和高风险建议。
6. 授权 operator 可以围绕 observer_report 创建受控 discussion session，并与目标 Agent 讨论后续需求。
7. P2 需要的 side-effect contract、credential lease、write connector contract、dry-run policy、no-op adapter 和 audit 事件已存在。
```

## P2 受控外部执行

P2 目标是启用真实外部写入，但只沿用 P1 已固定的 contract。

必须实现：

```text
1. 真实 WriteConnector adapter。
2. 真实 CredentialProvider adapter；credential 只通过 opaque provider_ref 进入执行边界。
3. 启用 P1 已 dry-run 的 side_effect_mode、risk_level、approval_required 策略。
4. 启用 resource_locks 写入侧强制校验。
5. side_effect_plan 状态推进、前后审计、结果校验和错误归一化。
6. connector retry / timeout / dead_letter，复用 P0/P1 run queue 和 audit 语义。
7. connector adapter 层的最小 rollback / compensation 策略。
8. Observer 评测副作用失败率、审批绕过尝试、锁冲突和异常写入模式。
```

硬性限制：

```text
1. 默认不允许自动 merge / release / deploy。
2. 高风险动作默认审批或硬拒绝。
3. 外部写入必须经过 Manager，不允许 Runtime 或 Worker 绕过。
4. credential 不进入长期 agent_instance，不进入 prompt、memory、observer_report 或 audit 明文。
5. Observer 只能建议，不能自动执行修复。
6. 如需改写 Manager 授权、Worker claim、run/session 状态机、Memory schema 或 audit contract，必须回到设计审查。
```

P2 验收：

```text
1. 外部执行可以被明确授权、审批、加锁、执行、审计和回溯。
2. 并发写同一 resource 时，只有一个 active side-effect run 可以持有锁。
3. 审批失败、锁冲突、超时和 connector 失败都有明确状态。
4. 每次 side effect 都能追溯到 request / approval / run / resource_lock / audit。
5. P2 只新增真实 provider/connector 实现和策略配置，不重写 P0/P1 核心 contract。
```
