# 场景、决策与验收

本文件用于验证拆分后的文档是否覆盖总体原则和架构。它不引入新的设计规则，只检查其它文档是否足以解释场景。

## 一致性审查结论

当前拆分文档已覆盖总体原则和架构，可以进入 P0 实现准备。

覆盖判断：

```text
1. 00-overview 定义统一术语、组件边界、核心不变量和文档职责。
2. 01-design-principles 定义 P01-P12 原则，包含 Open WebUI、Manager、Runtime、Worker、Observer、副作用和可观测性边界。
3. 02-architecture 将原则落成网络拓扑、调用方向和组件职责。
4. 03-product-external-definition 只描述外部可见行为，不新增内部控制能力。
5. 04-internal-definition 解释产品行为背后的对象、状态机、权限、隔离和复用规则。
6. 05-technical-implementation 将内部定义落成 Rust workspace、API、Postgres schema、lease、resource lock 和扩展 trait。
7. 06-negative-list 固化禁止项和延期项，防止功能诉求绕过原则。
8. 08-implementation-roadmap 将 P0/P1/P2 拆成可执行 TODO，且要求 P1/P2 只新增实现，不重写核心 contract。
```

冲突判断：

```text
1. 未发现 Open WebUI 直连 Manager / Runtime / Worker / Observer 的残留设计。
2. 未发现 Orchestrator 执行任务、审批、保存长期上下文或持有目标 credential 的残留设计。
3. 未发现 Observer Agent 执行控制动作或持有写权限 credential 的残留设计。
4. 未发现 P0 queue 在 Redis 与 Postgres 之间摇摆；已统一为 Postgres lease / SKIP LOCKED。
5. 未发现 P1/P2 需要重写 domain model、API contract、状态机或审计模型的设计。
```

需要持续注意：

```text
1. P0 编码时必须实际建立 RuntimeClient、MemoryStore、ConnectorClient、RunQueue、Telemetry facade。
2. OpenTelemetry / Prometheus exporter 可以后启用，但 trace_id、span、metrics name/label 必须从 P0 开始。
3. 外部 memory provider、Redis、gRPC、GraphQL 只能通过 adapter / feature 接入，不能反向改变控制面模型。
```

## 原则覆盖矩阵

| 原则 | 覆盖文档 | 验证场景 |
|---|---|---|
| P01 Open WebUI 只连 Gateway | 01, 02, 03, 06 | S02, S21 |
| P02 Manager 是唯一控制面 | 01, 02, 04, 05 | S03, S14, S18, S22 |
| P03 Orchestrator 只路由 | 01, 02, 03, 05 | S01, S04, S07 |
| P04 Runtime 只执行 | 01, 02, 04, 08 | S16, S17, S18 |
| P05 Worker 只推进 run | 01, 02, 05 | S13, S14, S15 |
| P06 Memory 不保存秘密 | 01, 03, 04, 06 | S08, S20 |
| P07 Agent 复用能力不复用状态 | 01, 04, 05 | S05, S06 |
| P08 长交互和单次执行分离 | 01, 03, 04, 05 | S07, S12 |
| P09 Child session 受限 | 01, 03, 04, 06 | S09, S10 |
| P10 Observer 只读 | 01, 02, 03, 04, 06 | S19, S20 |
| P11 副作用默认拒绝 | 01, 04, 05, 06, 08 | S17, S18, S22 |
| P12 可观测性从 P0 开始 | 01, 05, 08 | S14, S19, S25 |

## 场景覆盖检查

| 场景 | 期望处理 | 覆盖状态 | 设计依据 |
|---|---|---|---|
| S01 普通聊天 | Orchestrator 路由到 Default Hermes Agent Profile，流式返回 | 已覆盖 | 01, 02, 03 |
| S02 Open WebUI 试图直连 Manager | 网络和 capability 均不可达 | 已覆盖 | 01, 02, 06 |
| S03 创建新 Agent | Orchestrator 提交 request，Manager 做身份、权限、策略、审批 | 已覆盖 | 03, 04, 05 |
| S04 创建请求需要审批 | 返回 `approval_required`，不创建未授权 Agent | 已覆盖 | 01, 03, 04 |
| S05 请求创建的 Agent 已存在且配置一致 | 复用 existing agent，返回摘要 | 已覆盖 | 03, 04, 05 |
| S06 请求配置与已有 Agent 冲突 | 创建 change_request，不覆盖原 Agent | 已覆盖 | 03, 04 |
| S07 查询已有 Agent / Run / Session | 只返回用户有权看的摘要 | 已覆盖 | 03, 05 |
| S08 无权限访问资源 | Manager 拒绝并写 audit，Orchestrator 返回安全摘要 | 已覆盖 | 01, 03, 04 |
| S09 和已启动 Agent 长时间交互 | 创建或恢复 `agent_session`，消息进入 Runtime | 已覆盖 | 03, 04, 05 |
| S10 长 session 上下文增长 | 最近消息 + rolling summary，不保存 secrets | 已覆盖 | 01, 04 |
| S11 Session 内启动 child session | Manager 校验 scope、depth、budget 后创建 | 已覆盖 | 03, 04 |
| S12 Child session 继续创建 child | 拒绝，v1 只允许一层 | 已覆盖 | 01, 04, 06 |
| S13 触发已有 Agent 运行一次 | 创建 `agent_run`，Worker claim 后执行 | 已覆盖 | 04, 05 |
| S14 Worker 崩溃或 lease 超时 | lease 过期后接管或 timed_out / dead_letter | 已覆盖 | 05 |
| S15 Scheduled / Webhook 触发后台 run | webhook dedupe，Manager 策略校验后创建 run | 已覆盖 | 05 |
| S16 P0 Minimal Runtime | run 可调用 mock/local/read-only runtime 并回写 audit/result | 已覆盖 | 05, 08 |
| S17 P1 真实 Hermes Runtime 只读 | RuntimeClient 接真实 Hermes，但不持有写 credential | 已覆盖 | 01, 08 |
| S18 P2 受控外部执行 | 经过审批、最小权限 credential、resource lock 和审计 | 已覆盖 | 01, 05, 08 |
| S19 Observer 定期评测 | 读取只读摘要，生成 `observer_report` | 已覆盖 | 02, 03, 04, 05 |
| S20 Observer 尝试控制动作 | 硬拒绝并写 audit | 已覆盖 | 01, 04, 06 |
| S21 管理员查询 Observer report | 通过 agentctl/admin API 查询，Open WebUI 不可见 | 已覆盖 | 02, 03, 05 |
| S22 高风险副作用 | 默认审批或硬拒绝，不能绕过 Manager | 已覆盖 | 01, 06, 08 |
| S23 Agent paused / failed / terminated | 按状态动作矩阵返回安全摘要或管理员操作 | 已覆盖 | 04 |
| S24 并发 run 争用同一资源 | Postgres lease + resource_locks 保证同一资源副作用串行 | 已覆盖 | 05 |
| S25 P1/P2 新增 transport / connector / memory / telemetry 实现 | 只新增 trait 实现、adapter 或 feature，不改 domain model、API contract、状态机 | 已覆盖 | 05, 08 |

场景数量判断：当前 25 个场景覆盖普通聊天、控制面、复用、长会话、child session、Worker、Webhook、P0/P1/P2、Observer、安全拒绝、并发锁和后续扩展边界，已足够支撑 P0 编码启动。

## 决策清单

| 决策点 | v1 决策 |
|---|---|
| 用户入口 | Agent Orchestrator / Gateway 是 Open WebUI 唯一后台入口 |
| Default Agent 定位 | 只负责普通聊天和意图澄清，不作为固定中间层 |
| Manager 定位 | 唯一控制面，负责授权、策略、审批、生命周期、审计和资源锁 |
| Runtime 定位 | 执行已授权 session/run，不决定权限 |
| Worker 定位 | claim 和推进 run，不绕过 Manager |
| Observer Agent | v1 必须内置，只读监控、评测和建议 |
| 已存在 Agent | 默认复用，不重复创建 |
| Agent 唯一性 | `owner_user + agent_type + target_resource + core_constraints_hash` |
| 配置不一致 | 创建 `change_request`，不覆盖原 Agent |
| Agent 创建数量限制 | 每用户最多 10 个 active agent；每 resource 最多 3 个 active agent；同一用户+resource+agent_type 默认最多 1 个 |
| 动态 Agent 创建 | 只允许 allowlisted template |
| 长交互承载 | 用 `agent_session` |
| 单次执行 | 用 `agent_run` |
| Child Session 深度 | v1 只允许一层 |
| Child Session 数量 | 每 parent 最多 3 个 child，active child 最多 2 个 |
| Context 继承 | child 只接收 `context_summary + resource refs` |
| Credential 继承 | child 不继承 parent credential |
| Summary 策略 | 最近 30 条原文消息 + rolling summary，summary 上限 8k tokens |
| Webhook 去重 | webhook trigger 必须提供 `dedupe_key` |
| Queue / lease | P0 使用 Postgres lease / SKIP LOCKED |
| 可观测性 | P0 必须埋 trace_id、tracing span、metrics name/label；OpenTelemetry / Prometheus exporter 可 feature-gated |
| 后续扩展 | P0 必须先定义 RuntimeClient、MemoryStore、ConnectorClient、RunQueue、Telemetry facade，P1/P2 只新增实现 |
| Run 并发 | 同一 agent 同一时间只允许一个带副作用 active run |
| Resource 并发 | 同一 resource 同一时间只允许一个带副作用 active run |
| Worker 失败 | lease 过期后允许接管，超过重试次数进入 dead_letter |
| 高风险动作 | 默认审批或硬拒绝 |
| Observer 建议落地 | 只生成 `observer_report`；执行建议必须转 request 或管理员操作 |
| v1 不做 | 多级 child session、自动 merge/release/deploy、任意 template 动态创建、复杂多租户计费、外部 memory provider 深度集成 |

## 验收标准

P0 验收：

```text
1. Open WebUI 中看不到 Manager、Runtime、Worker、Observer 的工具或页面。
2. Open WebUI 只连接 Orchestrator。
3. Manager 可以根据用户、服务、资源、动作和 policy 做权限判断。
4. 无权限请求被拒绝并写 audit。
5. 高风险请求进入审批流或硬拒绝。
6. agentctl 可以审批、拒绝、暂停、恢复、查询和审计。
7. background_worker template 可以按策略创建并运行 Minimal Runtime。
8. observer_agent 可以生成 observer_report，且不能改变系统状态。
9. Worker 可以通过 Postgres lease claim、heartbeat、timeout 和 dead_letter 管理 run。
10. 每次 run 可追溯到 request、user、service、session、resource、result 和 audit。
11. P0 已包含 telemetry facade 和核心 trait 边界，P1/P2 不需要重写核心 contract。
```

P1 验收：

```text
1. 真实 Hermes Runtime 可以承载长 session。
2. 真实 Hermes Runtime 可以执行只读 run。
3. Hermes Runtime 不持有写权限 credential。
4. Observer 可以评测 Runtime 延迟、失败、重试和上下文膨胀。
```

P2 验收：

```text
1. 外部执行必须经过审批、最小权限 credential、resource lock 和 audit。
2. 并发写同一 resource 时只有一个 active side-effect run 持有锁。
3. 审批拒绝、锁冲突、超时和 connector 失败有明确状态。
4. Observer 只能报告副作用风险，不能自动修复。
```
