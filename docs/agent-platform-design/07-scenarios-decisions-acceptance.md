# 场景、决策与验收

本文件用于检查设计覆盖面，不引入新的设计规则。阶段状态以 [08-implementation-roadmap.md](08-implementation-roadmap.md) 为准。

## 一致性结论

当前文档集的一致性基线：

```text
1. Open WebUI 只连 Orchestrator / Gateway，不直连 Manager、Runtime、Worker 或 Observer。
2. Agent Identity Bridge 已作为现有能力提供 Open WebUI user/chat 到 Agent Platform session 的可信绑定。
3. Manager 是唯一控制面；授权、审批、生命周期、资源锁和审计不下放给 Runtime / Worker / Observer。
4. P0 正确性边界是 Postgres lease / SKIP LOCKED；Redis 只作为后续 adapter。
5. Observer 只读，只生成 observer_report；report discussion 由普通 agent_session 承载。
6. P1 接真实 Hermes Runtime 但只读；P2 只启用 P1 已固定的真实 provider / connector adapter。
7. 后续新增 transport、memory、connector、queue、telemetry 实现时，不重写 domain model、API contract、状态机或 audit contract。
```

## 场景覆盖

| 场景 | 期望处理 | 依据 |
|---|---|---|
| S01 普通聊天 | Orchestrator 路由到 Default Hermes Agent Profile，流式返回 | 01, 02, 03 |
| S02 Open WebUI 试图直连内部服务 | 网络和 capability 均不可达 | 01, 02, 06 |
| S03 创建新 Agent | Orchestrator 提交 request，Manager 做身份、权限、策略、审批 | 03, 04, 05 |
| S04 创建请求需要审批 | 返回 `approval_required`，不创建未授权 Agent | 01, 03, 04 |
| S05 请求命中已有 Agent | 配置一致则复用，配置冲突则创建 change_request | 03, 04 |
| S06 查询 Agent / Run / Session | 只返回用户有权看的摘要 | 03, 05 |
| S07 无权限访问资源 | Manager 拒绝并写 audit，Orchestrator 返回安全摘要 | 01, 03, 04 |
| S08 长时间交互 | 创建或恢复 `agent_session`，消息进入 Runtime | 03, 04, 05 |
| S09 上下文增长 | 最近消息 + rolling summary，不保存 secrets | 01, 04 |
| S10 Child session | Manager 校验 scope、depth、budget 后创建 | 03, 04 |
| S11 Child session 继续创建 child | 拒绝，v1 只允许一层 | 01, 04, 06 |
| S12 触发已有 Agent run | 创建 `agent_run`，Worker claim 后执行 | 04, 05 |
| S13 Worker 崩溃或 lease 超时 | lease 过期后接管或 timed_out / dead_letter | 05 |
| S14 Scheduled / Webhook 触发 | webhook dedupe，Manager 策略校验后创建 run | 05 |
| S15 P0 Minimal Runtime | run 调用 mock/local/read-only runtime 并回写 audit/result | 05, 08 |
| S16 Open WebUI Agent Identity Bridge | 签名 bridge context 绑定真实 Open WebUI user/chat/session，后续消息复用 session 并创建 run | 02, 03, 04, 05 |
| S17 P1 Hermes Runtime | 复用现有 Bridge/session/run 链路，RuntimeClient 接真实 Hermes，但不持有写 credential | 01, 08 |
| S18 P2 外部执行 | 经过审批、最小权限 credential、resource lock 和审计 | 01, 04, 05, 08 |
| S19 Observer 定期评测 | 读取只读摘要，生成 `observer_report` | 02, 03, 04, 05 |
| S20 Observer 尝试控制动作 | 硬拒绝并写 audit | 01, 04, 06 |
| S21 管理员查询 Observer report | 通过 agentctl/admin API 查询，Open WebUI 不可见 | 02, 03, 05 |
| S22 高风险副作用 | 默认审批或硬拒绝，不能绕过 Manager | 01, 06, 08 |
| S23 Agent paused / failed / terminated | 按状态动作矩阵返回安全摘要或管理员操作 | 04 |
| S24 并发 run 争用同一资源 | Postgres lease + resource_locks 保证同一资源副作用串行 | 05 |
| S25 新增 adapter | 只新增 trait 实现、adapter 或 feature，不改核心 contract | 05, 08 |
| S26 Observer Report Discussion | Manager 创建普通 session 讨论 report；Observer 不参与对话、不执行动作 | 03, 04, 05, 08 |
| S27 P1 P2 readiness dry-run | P1 固定 side-effect / credential / write connector contract，只 dry-run；P2 启用真实 adapter | 04, 05, 08 |

场景数量不追求膨胀；新增场景只有在暴露新的权限、状态机、隔离或执行边界时才加入。

## 决策清单

| 决策点 | v1 决策 |
|---|---|
| 用户入口 | Open WebUI 只连 Orchestrator / Gateway |
| Manager | 唯一控制面，负责授权、策略、审批、生命周期、审计和资源锁 |
| Orchestrator | 只路由、绑定 session、转发流式响应和安全摘要 |
| Agent Identity Bridge | Open WebUI Filter 注入签名 user/chat context；Manager 持久保存 bridge binding |
| Runtime | 执行已授权 session/run，不决定权限 |
| Worker | claim 和推进 run，不扩大权限 |
| Observer | 内置只读评测 Agent，只生成 report 和建议 |
| Report discussion | P1 可围绕 report 创建普通 `agent_session`，由目标 Agent 讨论 |
| Agent 复用 | 默认复用同一 `owner_user + agent_type + target_resource + core_constraints_hash` |
| Agent 变更 | 配置冲突创建 change_request，不覆盖原 Agent |
| 动态 Agent 创建 | 只允许 allowlisted template |
| 长交互 / 单次执行 | 长交互用 `agent_session`，单次执行用 `agent_run` |
| Child Session | v1 只允许一层；child 只接收 summary + resource refs，不继承 credential |
| Memory | 最近 30 条原文消息 + rolling summary，summary 上限 8k tokens，不保存 secrets |
| Queue / lease | P0 使用 Postgres lease / SKIP LOCKED |
| P1/P2 扩展 | P1 固定 side-effect readiness；P2 只启用真实 adapter |
| 高风险动作 | 默认审批或硬拒绝 |
| v1 不做 | 多级 child session、自动 merge/release/deploy、任意 template 动态创建、复杂多租户计费、外部 memory provider 深度集成 |

## 验收标准

P0 验收：

```text
1. Open WebUI 中看不到 Manager、Runtime、Worker、Observer 的工具或页面。
2. Manager 可以根据用户、服务、资源、动作和 policy 做权限判断。
3. agentctl 可以审批、拒绝、暂停、恢复、查询和审计。
4. background_worker template 可以按策略创建并运行 Minimal Runtime。
5. observer_agent 可以生成 observer_report，且不能改变系统状态。
6. Open WebUI Agent Identity Bridge 可以验证签名 context、持久化 chat/session binding，并让后续消息创建 read-only run。
7. Worker 可以通过 Postgres lease claim、heartbeat、timeout 和 dead_letter 管理 run。
8. 每次 run 可追溯到 request、user、service、session、resource、result 和 audit。
9. P0 已包含 telemetry facade 和核心 trait 边界。
```

P1 验收：

```text
1. 真实 Hermes Runtime 可以承载长 session 和只读 run。
2. Hermes Runtime 不持有写权限 credential，不执行外部 side effect。
3. 真实 Hermes Runtime 复用现有 Open WebUI Bridge、agent_session、agent_run、Worker 和 audit 链路。
4. Observer 可以评测 Runtime 延迟、失败、重试和上下文膨胀。
5. 授权 operator 可以围绕 observer_report 创建受控 discussion session。
6. P1 已提供 side-effect plan、credential lease、write connector contract、dry-run policy、no-op provider 和审计事件。
7. P1 不改 request、run、session、audit、Manager 授权、Open WebUI Bridge 或 Worker claim 基线模型。
```

P2 验收：

```text
1. 外部执行必须经过审批、最小权限 credential、resource lock 和 audit。
2. 并发写同一 resource 时只有一个 active side-effect run 持有锁。
3. 审批拒绝、锁冲突、超时和 connector 失败有明确状态。
4. Observer 只能报告副作用风险，不能自动修复。
5. P2 不重写 P0/P1 的 Manager 授权、Worker claim、run/session 状态机、Memory schema 或 audit contract。
```
