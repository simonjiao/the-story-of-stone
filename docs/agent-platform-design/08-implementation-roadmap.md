# 实施路线与 TODO

本文件只记录阶段边界、交付顺序和验收标准。设计规则以 [01-design-principles.md](01-design-principles.md) 为准；内部对象以 [04-internal-definition.md](04-internal-definition.md) 为准；API 和 schema 以 [05-technical-implementation.md](05-technical-implementation.md) 为准。

## 阶段边界

| 阶段 | 状态 | 目标 | 不变边界 |
|---|---|---|---|
| P0 | 代码基线已实现 | 控制面、Open WebUI Agent Identity Bridge、Minimal Runtime、Worker、Observer、audit 形成最小闭环 | Postgres lease / SKIP LOCKED 是正确性边界；不接真实写 connector；不注入写权限 credential；Bridge 完成口径以 hardening checklist 和部署复测为准 |
| P1 | 已完成实现和部署 smoke | 在现有 Bridge/session/run 链路上接入真实 Hermes Runtime，只做只读 session/run，让 Observer report 可进入受控 discussion session，并提供 System Observer status session | 不改 Manager 授权、Open WebUI Bridge、run/session 状态机、Worker claim、Memory schema、audit contract；System Observer status session 只作为窄口例外，不扩展为通用 admin proxy |
| P2 | 仓库侧实现完成 | 启用受控外部写入 | 已完成 apply / compensate API、HTTP provider/connector、`action-journal` provider/connector/target adapter、锁、审计、补偿状态、本地回归和端到端 smoke；默认部署仍关闭写入，真实第三方目标以目标环境 contract smoke 证明 |

P1 会提前落地 P2 需要的 external-action plan、credential lease、write connector contract、dry-run policy、no-op provider 和审计事件。P1 只 dry-run / validate / reject，不获取真实 credential，不调用真实写 connector，不把 run 推进到真实外部写入。

## P0 当前基线

P0 代码基线已实现。路线图只保留 P1/P2 依赖的稳定基线；详细实施记录见 [P0_IMPLEMENTATION_CHECKLIST.md](P0_IMPLEMENTATION_CHECKLIST.md)，P1 实现与 smoke 记录见 [P1_IMPLEMENTATION_CHECKLIST.md](P1_IMPLEMENTATION_CHECKLIST.md)，P2 实现与前提复盘见 [P2_IMPLEMENTATION_CHECKLIST.md](P2_IMPLEMENTATION_CHECKLIST.md)，Bridge hardening 记录见 [BRIDGE_HARDENING_CHECKLIST.md](BRIDGE_HARDENING_CHECKLIST.md)。

```text
Open WebUI / Agent Identity Bridge
  → Orchestrator
  → Manager bridge/session/run API
  → Worker
  → Minimal Runtime
  → Memory / Audit / Observer Report
```

P1/P2 不应重写以下基线：

```text
1. Manager 授权、审批、生命周期、audit 和 `open_webui_bridge_bindings`。
2. agent_session / agent_run 状态机、Worker claim / heartbeat / finish / dead-letter。
3. Postgres lease / SKIP LOCKED、resource_locks、idempotency 和 trace_id 关联。
4. Observer 只读 snapshot、observer_report 持久化、System Observer status session 脱敏上下文和“只建议不控制”边界。
5. RuntimeClient、MemoryStore、ConnectorClient、RunQueue、Telemetry facade。
```

Bridge 只能在以下条件同时满足后宣告对应环境完成：代码测试通过、Function 安装和 valves 校验通过、dev headers 关闭、nonce/message/audit hardening 生效，并完成目标环境的登录、模型选择、基础聊天、会话保存、Bridge binding、后续 run、关闭 session 回归。

## P1 真实 Hermes Runtime，只读

P1 目标是让平台开始使用真实 Hermes Runtime，同时复用现有 Open WebUI Agent Identity Bridge、agent_session、agent_run、Worker lease 和 audit 链路。P1 不再重新设计 Open WebUI 身份或 session binding。

交付顺序：

| 阶段 | 目标 | 主要交付 | 验证重点 |
|---|---|---|---|
| P1.0 Contract freeze | 固定不可改边界 | 为 RuntimeClient、ConnectorClient、Telemetry、安全错误、Open WebUI Bridge 增加 contract/regression test；确认 request/run/session/audit/bridge 无破坏性 schema change | `cargo test --manifest-path agent-platform/Cargo.toml`；schema diff 只允许新增配置或非破坏字段 |
| P1.1 Hermes adapter | 新增真实 Runtime adapter | `HermesRuntimeClient`、Hermes HTTP client、profile-to-model routing、timeout、safe error mapping、`AGENT_RUNTIME_MODE=minimal|hermes` | unit/wiremock 覆盖成功、profile routing、timeout、5xx、malformed response 和 trace_id |
| P1.2 Bridge-backed session path | 真实 Hermes 承载已绑定长 session | 复用 `open_webui_bridge_bindings`、`agent_session`、session message append、Worker claim 和 OpenAI-compatible response/SSE wrapper；`session_message` run 调用 `RuntimeClient::send_session_message` | 登录、模型选择、基础聊天、会话保存；响应不含 prompt/context/credential；Bridge 绑定仍可复用 |
| P1.3 Run path | 真实 Hermes 执行只读 run | Worker 按 profile 选择 Hermes adapter，构建上下文和只读 snapshot，finish 写 summary/result_ref/audit | claim、heartbeat、timeout、retry、dead_letter、finish audit 状态机不变 |
| P1.4 Read-only connector adapter | 真实只读工具与外部 snapshot | 在既有 `ConnectorClient::read_only_snapshot` contract 上新增真实 read-only adapter；Runtime 只消费只读摘要或 payload_ref | `external_action_mode=authorized` 仍拒绝真实写入；connector payload 不泄露 secrets |
| P1.5 Observer upgrade | 评测真实 Runtime 行为 | observer snapshot 增加 latency、timeout、retry、context size、runtime quality signal、risk taxonomy 和结果质量摘要 | report 只生成 findings/recommendations，不触发控制动作 |
| P1.6 Report discussion / System Observer status session | 快速讨论报告并形成后续需求；在 Open WebUI 中快速进入系统状态诊断会话 | `POST /v1/admin/observer/reports/{report_id}/discussions`、`POST /v1/admin/observer/system-session` 和 agentctl 命令；Manager 创建普通 `agent_session` | 权限、脱敏、session/audit 关联；Observer 不执行控制动作；普通用户无法创建 System Observer session |
| P1.7 P2 readiness | 避免 P2 重构 | ExternalActionPlanner / CredentialProvider / WriteConnector contract，`external_action_plans` / `credential_leases` migration，dry-run approval/lock/credential policy，no-op provider/write connector，审计事件 | P1 只能 dry-run 或拒绝；no-op provider 不产生 secret；缺审批、锁冲突、缺 credential_scope 和 critical risk 都有明确拒绝状态 |
| P1.8 Smoke | 本地和部署回归 | 示例 env / runbook 覆盖 Hermes URL、profile、mode、Bridge regression、report discussion、System Observer status session、P2 readiness dry-run | 最小关键路径 + Bridge regression + Hermes smoke + report discussion + System Observer status session + P2 readiness dry-run + Postgres smoke |

P1 验收：

```text
1. `agent_session` 可以和真实 Hermes Runtime 多轮对话。
2. `agent_run` 可以调用真实 Hermes Runtime 完成只读分析。
3. Runtime 不持有写权限 credential，不执行外部动作。
4. 所有 Runtime 调用都能追溯到 request / Open WebUI subject / service / session / run / audit / trace_id。
5. 现有 Agent Identity Bridge 仍能完成签名校验、chat binding、session 复用、后续消息 create run 和关闭 session。
6. Observer 能识别 Runtime 异常、性能退化、上下文膨胀和高风险建议。
7. 授权 operator 可以围绕 observer_report 创建受控 discussion session，并与目标 Agent 讨论后续需求。
8. 授权 admin/operator 可以通过 Open WebUI 系统状态意图创建 System Observer status session，快速看到最新脱敏 report、health、risk、findings、recommendations 和 session_id；普通用户被安全拒绝。
9. P2 需要的 external-action contract、credential lease、write connector contract、dry-run policy、no-op adapter 和 audit 事件已存在。
```

## P2 受控外部执行

P2 目标是启用真实外部写入，但只沿用 P1 已固定的 contract。仓库侧提供通用 HTTP CredentialProvider / WriteConnector 和仓库内低风险 `action-journal` target；真实第三方系统不在仓库内硬编码，必须在目标环境配置 provider / connector 后运行 `agent-platform/scripts/external-action-contract-smoke.sh` 验收。

必须实现：

```text
1. 真实 WriteConnector adapter。
2. 真实 CredentialProvider adapter；credential 只通过 opaque provider_ref 进入执行边界。
3. 启用 P1 已 dry-run 的 external_action_mode、risk_level、approval_required 策略。
4. 启用 resource_locks 写入侧强制校验。
5. external_action_plan 状态推进、前后审计、结果校验和错误归一化。
6. connector retry / timeout / dead_letter，复用 P0/P1 run queue 和 audit 语义。
7. connector adapter 层的最小 rollback / compensation 策略，并由 Manager compensation API 统一审计和推进状态。
8. Observer 评测外部动作失败率、审批绕过尝试、锁冲突和异常写入模式。
```

硬性限制：

```text
1. 默认不允许自动 merge / release / deploy。
2. 高风险动作默认审批或硬拒绝。
3. 外部写入必须经过 Manager，不允许 Runtime 或 Worker 绕过。
4. credential 不进入长期 agent_instance，不进入 prompt、memory、observer_report 或 audit 明文。
5. Observer 只能建议，不能自动执行修复。
6. 如需改写 Manager 授权、Open WebUI Bridge、Worker claim、run/session 状态机、Memory schema 或 audit contract，必须回到设计审查。
```

P2 验收：

```text
1. 外部执行可以被明确授权、审批、加锁、执行、审计和回溯。
2. 并发写同一 resource 时，只有一个 active external-action run 可以持有锁。
3. 审批失败、锁冲突、超时和 connector 失败都有明确状态。
4. 每次 external action 都能追溯到 request / approval / run / resource_lock / audit。
5. P2 只新增真实 provider/connector 实现和策略配置，不重写 P0/P1 核心 contract 或 Open WebUI Bridge。
```
