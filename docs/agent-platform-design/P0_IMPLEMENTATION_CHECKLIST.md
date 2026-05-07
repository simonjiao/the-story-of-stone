# P0 Implementation Checklist

状态：已实现并通过 `cargo test`。

## 1. Rust Workspace / Crate 边界

- [x] 新增 `agent-platform/` Rust workspace。
- [x] 新增 `agent-core`、`agent-store`、`agent-manager`、`agent-orchestrator`、`agent-runtime`、`agent-worker`、`agentctl`。
- [x] `agent-core` 不依赖 `axum`、`sqlx`、`reqwest`。

总结：P0 crate 边界已按设计落地，后续 P1/P2 可以通过 adapter 扩展，不需要重写 domain model。

## 2. Core Domain / State Machine / Trait Boundary

- [x] 定义 request、agent、session、run、approval、audit、observer report 等 P0 domain model。
- [x] 定义状态枚举和关键状态机校验。
- [x] 定义 `RuntimeClient`、`MemoryStore`、`ConnectorClient`、`RunQueue`、`Telemetry` facade。
- [x] 定义 RBAC / ABAC 授权上下文和默认 P0 policy。
- [x] 固定 trace / metrics 名称和 label。

总结：控制面状态、策略和扩展边界集中在 `agent-core`，Runtime、Worker、Store 都只能通过显式 trait 接入。

## 3. Store / Migration / Queue / Lock

- [x] 新增 P0 Postgres migration 和核心表。
- [x] migration 内置 `background_worker` 和 `observer_agent` template seed。
- [x] 实现 `PgAgentStore` SQLx repository。
- [x] 实现 Postgres `SELECT ... FOR UPDATE SKIP LOCKED` run claim。
- [x] 实现 heartbeat、timeout sweep、retry、dead_letter。
- [x] 实现 resource lock、message append、summary、result_ref、observer snapshot。
- [x] 实现 `MemoryAgentStore` 作为测试和无 DB 本地闭环。

总结：P0 正确性以 Postgres 为边界；内存实现只用于测试和本地无数据库运行。

## 4. Agent Manager

- [x] 实现 user API：agent request、agent/session/run 查询、session message、child session、close。
- [x] 实现 admin API：request list/approve/deny、agent list/pause/resume/delete、audit、observer report。
- [x] 实现 internal API：webhook、run claim/heartbeat/finish/dead-letter、session context、memory summary、observer tick。
- [x] 实现 service token + user claims 双主体授权；无 JWT 时支持开发头。
- [x] 实现 request policy、approval_required、denied、fulfilled。
- [x] 所有关键决策写 audit 并携带 trace_id。

总结：Manager 是唯一控制面；审批、生命周期、审计、observer report 都不绕过 Manager。

## 5. Minimal Runtime

- [x] 实现 `MinimalRuntimeClient`。
- [x] 支持 run execution 和 session message 两条 Runtime 路径。
- [x] 默认 read-only echo/local profile。
- [x] P0 硬拒绝 authorized side effect。

总结：Runtime 只执行已授权输入，不做权限判断，也不持有 credential。

## 6. Worker / Observer

- [x] Worker sweep expired lease。
- [x] Worker claim run、heartbeat、推进状态、调用 Runtime、finish。
- [x] Worker 支持 retry 和 dead_letter。
- [x] Worker side-effect lock 路径存在，但 P0 Runtime 仍拒绝写副作用。
- [x] Observer tick 只读聚合 snapshot 并写 observer_report。

总结：Worker 只推进 run；Observer 只生成报告和建议，不执行控制动作。

## 7. Orchestrator / Gateway

- [x] 提供 OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
- [x] 支持 streaming SSE。
- [x] 支持 Open WebUI conversation 到 agent_session 的轻量 binding。
- [x] Agent 创建意图只转成 Manager request。
- [x] 只返回安全错误摘要，不暴露 Manager admin API。

总结：Open WebUI 的唯一入口是 Orchestrator；Orchestrator 只路由、绑定、转发和归一化错误。

## 8. agentctl / Verification

- [x] `agentctl requests list/approve/deny`。
- [x] `agentctl agents list/pause/resume`。
- [x] `agentctl audit`。
- [x] `agentctl observer reports/show/run`。
- [x] 单元测试覆盖状态机、policy、runtime side-effect 拒绝、queue claim、observer snapshot、worker completion。
- [x] `cargo test` 通过。

总结：P0 已具备 CLI 管理闭环和自动化验证入口。
