# 场景、决策与验收

本文件只做覆盖检查，不引入新的设计规则。规则以 [01-design-principles.md](01-design-principles.md) 为准；阶段边界以 [08-implementation-roadmap.md](08-implementation-roadmap.md) 为准。

## 一致性结论

当前文档集必须同时满足：

```text
1. Open WebUI 只连 Orchestrator / Gateway，不直连 Manager、Runtime、Worker 或 Observer。
2. Agent Identity Bridge 只提供签名 user/chat context 和 session binding 入口，不授予 Manager 权限。
3. Manager 是唯一控制面；授权、审批、生命周期、资源锁和审计不下放给 Runtime / Worker / Observer。
4. Orchestrator 通过 Manager 的 user/bridge API 进入 session/run，不直接调用 Runtime session API。
5. P0 正确性边界是 Postgres lease / SKIP LOCKED；Redis 只作为后续 adapter。
6. Observer 只读，生成 observer_report，并通过普通 agent_session 承载 report discussion 和 System Observer status session。
7. P1 接真实 Hermes Runtime 但只读；P2 只启用 P1 已固定的真实 provider / connector adapter。
```

## 覆盖矩阵

| 领域 | 覆盖场景 | 必须满足 | 依据 |
|---|---|---|---|
| 用户入口 | 普通聊天、Agent 控制请求、Open WebUI 试图直连内部服务 | 普通聊天走 Default Hermes；控制请求必须有有效 bridge context；内部服务不可达 | 01, 02, 03 |
| Bridge/session | Open WebUI user/chat 绑定 agent_session，后续消息继续同一 session | Manager 持久保存 binding；后续消息 append session message 并创建 read-only run | 02, 03, 04, 05 |
| Agent lifecycle | 创建、审批、拒绝、复用、配置冲突、无权限访问 | Orchestrator 只提交 request；Manager 返回安全摘要并写 audit | 03, 04, 05 |
| 查询边界 | 查询 Agent / Session / Run / Observer report / System Observer status session | 普通用户只看摘要；report 仅 admin/operator 通过 admin API、agentctl 或受控 status session 查看脱敏上下文 | 03, 05, 06 |
| Session/run | 长交互、child session、manual/scheduled/webhook/session_message run | child v1 只允许一层；run 由 Manager 创建、Worker claim、Runtime 执行 | 03, 04, 05 |
| 并发与失败 | Worker crash、lease 过期、retry、dead-letter、资源争用 | lease 可接管；同一资源副作用串行；状态迁移写 audit | 05 |
| Memory/security | 上下文增长、summary、result_ref、错误摘要 | 不保存 secrets；不返回完整 prompt/context/log；错误只暴露安全摘要和 trace_id | 01, 03, 04, 05 |
| Observer | 定期评测、手动触发、report 生成、status session context、越权控制动作 | 只读 snapshot；只写 report、脱敏 session context 和 audit；控制动作硬拒绝 | 01, 03, 04, 06 |
| Report discussion | 围绕 report 快速讨论后续需求 | Manager 创建普通 agent_session；只注入脱敏 report 摘要；Observer 不作为目标 Agent 参与该 discussion | 03, 04, 05, 08 |
| System Observer status session | Open WebUI admin/operator 询问系统状态、最新 Observer 报告或平台健康 | Orchestrator 只能调用 Manager 窄口；Manager 创建 dedicated observer_agent session；普通用户拒绝；只注入脱敏 report packet | 01, 02, 03, 04, 05, 06 |
| P1 真实 Runtime | Hermes adapter、只读 run、只读 connector、Runtime 质量评测 | 复用 P0 Bridge/session/run/Worker/audit；不持有写 credential，不执行外部写入 | 01, 05, 08 |
| P2 readiness / execution | side-effect plan、credential lease、write connector、resource lock | P1 只 dry-run / validate / reject；P2 才启用真实 provider/connector | 04, 05, 08 |
| 后续 adapter | transport、memory、queue、telemetry 或 connector 替换 | 只新增 adapter/feature，不重写 domain model、API contract、状态机或 audit contract | 05, 08 |

新增覆盖项只有在暴露新的权限、状态机、隔离、执行或审计边界时才加入。

## 决策索引

| 边界 | v1 决策 |
|---|---|
| 用户入口 | Open WebUI 只连 Orchestrator / Gateway；Open WebUI admin 不等于通用 Agent Platform admin；System Observer status session 使用独立 observer-admin role mapping |
| 控制面 | Manager 是唯一授权、策略、审批、生命周期、资源锁和审计入口 |
| Bridge | Open WebUI Filter 注入签名 context；Manager 持久保存 bridge binding |
| 执行面 | Runtime 只执行已授权 session/run；Worker 只 claim 和推进 run |
| Agent 复用 | 默认复用同一 `owner_user + agent_type + target_resource + core_constraints_hash`；配置冲突创建 change_request |
| Session | 长交互用 `agent_session`；单次执行用 `agent_run`；child session v1 只允许一层 |
| Memory | 最近消息 + rolling summary + result_ref；不保存 secrets、credential、完整 prompt 或完整内部日志 |
| Observer | 只读评测，生成 report、建议和脱敏状态会话；report discussion 由目标 Agent 的普通 session 承载，System Observer status session 由 dedicated observer_agent 的普通 session 承载 |
| P1/P2 | P1 固定只读 Runtime 和 P2 readiness contract；P2 只启用真实 provider/connector |
| v1 不做 | 多级 child session、自动 merge/release/deploy、任意 template 动态创建、复杂多租户计费、外部 memory provider 深度集成 |

## 验收标准

P0 代码基线验收：

```text
1. Open WebUI 中看不到 Manager、Runtime、Worker、Observer 的工具或页面。
2. Agent Identity Bridge 能验证签名 context，Manager 能持久化 chat/session binding。
3. Bridge nonce replay、message append 幂等和 binding lifecycle audit 有代码与测试覆盖。
4. Manager 能按用户、服务、资源、动作和 policy 做权限判断，并写 audit。
5. agentctl 能审批、拒绝、暂停、恢复、查询 audit 和 observer_report。
6. background_worker 能按策略创建 run，Worker 能通过 Postgres lease claim、heartbeat、timeout 和 dead-letter 推进 run。
7. observer_agent 能生成 observer_report，且不能改变系统状态。
8. run 可追溯到 request、user/service、session、resource、result、audit 和 trace_id。
```

P1 验收：

```text
1. 真实 Hermes Runtime 可以承载长 session 和只读 run。
2. Hermes Runtime 复用现有 Bridge、agent_session、agent_run、Worker 和 audit 链路。
3. Runtime 不持有写权限 credential，不执行外部 side effect。
4. Observer 可以评测 Runtime 延迟、失败、重试、上下文膨胀和质量风险。
5. 授权 operator 可以围绕 observer_report 创建受控 discussion session。
6. 授权 admin/operator 可以通过 Open WebUI 系统状态意图创建 System Observer status session；普通用户被安全拒绝。
7. P1 已提供 side-effect plan、credential lease、write connector contract、dry-run policy、no-op provider 和审计事件。
```

P2 验收：

```text
1. 外部执行必须经过审批、最小权限 credential、resource lock 和 audit。
2. 并发写同一 resource 时只有一个 active side-effect run 持有锁。
3. 审批拒绝、锁冲突、超时和 connector 失败有明确状态。
4. Observer 只能报告副作用风险，不能自动修复。
5. P2 不重写 P0/P1 的 Manager 授权、Open WebUI Bridge、Worker claim、run/session 状态机、Memory schema 或 audit contract。
```
