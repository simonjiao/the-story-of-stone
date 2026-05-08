# P0 Implementation Checklist

状态：已实现并通过 `cargo test --manifest-path agent-platform/Cargo.toml`。

2026-05-08 复盘结论：原 checklist 对部分 Manager internal/admin 能力写得过满。复盘发现的占位实现已补齐，补丁式路径已重构；当前没有剩余 P0 功能缺口。

复盘补齐项：

- [x] `POST /v1/admin/grants` 从占位拒绝改为真实创建 `AgentGrant`，并写入 audit。
- [x] `POST /v1/internal/webhooks/{connector}` 从只返回 accepted 改为校验 connector / trigger / dedupe / payload / resource，并为匹配 running agent 幂等创建 read-only webhook run。
- [x] internal API 统一 service action gate；`internal_append_message` 不再复用用户 owner 路径。
- [x] run claim API 改为 `POST /v1/internal/runs/claim`，与 claim-next 语义一致。
- [x] Orchestrator session binding 去除 `.expect("bindings lock")`，锁异常返回安全错误摘要。
- [x] Manager 中 auth/JWT/error gate、request 生命周期、grant/webhook/session append、metrics 分别抽到 `http_support.rs`、`request_services.rs`、`control_services.rs`、`telemetry_support.rs`；handler 只保留路由编排和错误映射。
- [x] grant/webhook HTTP 输入的 `OffsetDateTime` 字段固定为 RFC3339 JSON contract，并已在远程 Postgres smoke 中验证。
- [x] session/run 创建 API 不再只携带 idempotency_key；已实现 session/run/internal-run 的幂等查询与复用，新增 `0002_session_idempotency.sql` 兼容既有 DB。
- [x] Worker 状态推进补齐 audit：claim、context_built、policy_checked、executing、validating、finish、retry/dead_letter 都会写审计。
- [x] retry 不再立即重新 claim；已新增 `next_retry_at` 和 30s / 120s / 300s 退避，claim 只消费到期 queued run。
- [x] dead_letter 管理闭环补齐：Manager admin runs list/show/retry/terminate、store 状态约束、agentctl runs list/show(inspect)/retry/terminate。
- [x] 新增 Manager 回归测试覆盖 grant、webhook 幂等 run 创建、internal append message service gate、dead_letter inspect/retry/terminate。

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
- [x] 补齐 admin grant 和 internal service action 常量，避免 handler 使用临时字符串。

总结：控制面状态、策略和扩展边界集中在 `agent-core`，Runtime、Worker、Store 都只能通过显式 trait 接入。

## 3. Store / Migration / Queue / Lock

- [x] 新增 P0 Postgres migration 和核心表。
- [x] migration 内置 `background_worker` 和 `observer_agent` template seed。
- [x] 实现 `PgAgentStore` SQLx repository。
- [x] 实现 Postgres `SELECT ... FOR UPDATE SKIP LOCKED` run claim。
- [x] 实现 heartbeat、timeout sweep、retry、dead_letter。
- [x] retry 使用 `next_retry_at` 执行 30s / 120s / 300s 退避；admin retry 会清除退避并重新进入 queued。
- [x] 实现 resource lock、message append、summary、result_ref、observer snapshot。
- [x] session/run 创建幂等语义覆盖 Postgres 与 Memory store。
- [x] 实现 `MemoryAgentStore` 作为测试和无 DB 本地闭环。

总结：P0 正确性以 Postgres 为边界；内存实现只用于测试和本地无数据库运行。

## 4. Agent Manager

- [x] 实现 user API：agent request、agent/session/run 查询、session message、child session、close。
- [x] 实现 admin API：request list/approve/deny、agent list/pause/resume/delete、audit、observer report、run list/show/retry/terminate。
- [x] 实现 admin grant 创建，写入 `agent_grants` 并审计。
- [x] 实现 internal API：webhook、run claim/heartbeat/finish/dead-letter、session context、memory summary、observer tick。
- [x] webhook 会按 connector 事件为匹配 agent 创建 read-only run，并通过 dedupe_key 保持幂等。
- [x] 所有 internal API 都检查 service action；内部 session message append 不复用用户 owner 检查路径。
- [x] 实现 service token + user claims 双主体授权；无 JWT 时支持开发头。
- [x] 实现 request policy、approval_required、denied、fulfilled。
- [x] 所有关键决策写 audit 并携带 trace_id。

总结：Manager 是唯一控制面；审批、生命周期、webhook run 创建、internal service gate、审计、observer report 都不绕过 Manager。

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
- [x] Worker 对 run claim/status/final/retry/dead_letter 写 audit。
- [x] Worker retry 进入 delayed queued，避免失败后被立即重新 claim。
- [x] Observer tick 只读聚合 snapshot 并写 observer_report。

总结：Worker 只推进 run；Observer 只生成报告和建议，不执行控制动作。

## 7. Orchestrator / Gateway

- [x] 提供 OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
- [x] 支持 streaming SSE。
- [x] 支持 Open WebUI conversation 到 agent_session 的轻量 binding。
- [x] session binding 锁异常不 panic，统一返回安全错误摘要。
- [x] Agent 创建意图只转成 Manager request。
- [x] 只返回安全错误摘要，不暴露 Manager admin API。

总结：Open WebUI 的唯一入口是 Orchestrator；Orchestrator 只路由、绑定、转发和归一化错误。

## 8. agentctl / Verification

- [x] `agentctl requests list/approve/deny`。
- [x] `agentctl agents list/pause/resume`。
- [x] `agentctl audit`。
- [x] `agentctl observer reports/show/run`。
- [x] `agentctl runs list/show(inspect)/retry/terminate`。
- [x] 单元测试覆盖状态机、policy、runtime side-effect 拒绝、queue claim、observer snapshot、worker completion。
- [x] 单元测试覆盖 admin grant、webhook 幂等 run 创建、session/run 创建幂等、internal append message service gate、retry 退避、dead_letter admin 管理闭环。
- [x] `cargo test` 通过。
- [x] hhost 远程环境使用 `rust:1.95` 系列镜像完成 `cargo test --workspace` 和隔离 Postgres smoke；未修改现有 deploy 服务。

总结：P0 已具备 CLI 管理闭环、自动化验证入口和远程 Postgres 最小关键路径验证。
