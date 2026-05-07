# 技术实现细节

本文件定义 Rust 落地方案。实现必须满足 [01-design-principles.md](01-design-principles.md) 和 [04-internal-definition.md](04-internal-definition.md)，不得新增绕过 Manager 的隐式能力。

## Rust Workspace

```text
agent-platform/
  Cargo.toml
  crates/
    agent-core/           # domain model、状态枚举、错误、ID、时间、policy context
    agent-store/          # Postgres 访问、migration、transaction、query
    agent-manager/        # Manager HTTP/API、auth、policy、approval、audit、observer report
    agent-orchestrator/   # Gateway、intent routing、session binding、streaming、安全错误摘要
    agent-runtime/        # Runtime trait、Minimal Runtime、HermesRuntimeClient、context assembly
    agent-worker/         # run claim、heartbeat、timeout、retry、resource lock、observer tick
    agentctl/             # 管理员 CLI
```

模块职责：

```text
agent-core 不依赖 axum/sqlx/reqwest，只保存类型和纯逻辑。
agent-store 只处理数据库和事务，不做产品策略。
agent-manager 是唯一控制面服务。
agent-orchestrator 不访问 admin API，不持有目标 Agent credential。
agent-runtime 不决定授权，只执行已授权 session/run。
agent-worker 不绕过 Manager，所有 run 状态推进写 audit。
agentctl 只调用 admin API。
```

## Rust Crate 选型

选型原则：

```text
1. 优先选择维护活跃、Tokio / Tower 兼容的 crate。
2. 优先组合小而清晰的 crate，不引入大而全框架。
3. 控制面使用类型安全、显式错误和显式权限边界。
4. 性能敏感路径保持异步、池化、限流、可观测。
5. P0 先固定抽象边界和可观测埋点；重依赖按 feature 或 adapter 分层启用，避免 P1/P2 大范围重构。
```

依赖分层：

```text
P0 直接依赖：运行闭环必须使用，且后续不会导致架构反转的 crate。
P0 预埋边界：先定义 trait / port / span / metric，不一定启用重后端。
P1/P2 启用实现：按 feature 或 adapter 接入，不改变 domain model、API contract 和状态机。
```

不引入某些 crate 的含义不是“永远不用”，而是“不让它们决定 P0 架构”。P0 必须把下列扩展点先定好：

```text
RuntimeClient trait：P1 接 Hermes HTTP，后续可加 gRPC transport。
MemoryStore trait：P0 Postgres，后续可加外部 memory provider adapter。
ConnectorClient trait：P1 read-only connector，P2 side-effect connector。
Telemetry facade：P0 trace_id + tracing span + metrics name，后续可接 OpenTelemetry / Prometheus exporter。
RunQueue trait：P0 Postgres lease，后续可加 Redis queue/cache adapter。
```

### P0 直接依赖

| 领域 | crate | 用途 |
|---|---|---|
| Async runtime | `tokio` | 服务、worker、timeout、signal、process |
| HTTP server | `axum` | Manager / Orchestrator API |
| Middleware | `tower`, `tower-http` | timeout、limit、trace、request id、body limit、sensitive headers |
| Serialization | `serde`, `serde_json` | API、DB JSONB、event payload |
| Database | `sqlx` | Postgres pool、migration、query 校验 |
| HTTP client | `reqwest` | Hermes Runtime、只读 connector、后续外部 connector |
| Logging / tracing | `tracing`, `tracing-subscriber`, `tracing-appender` | trace_id、span、结构化日志；P0 必须全链路埋点 |
| Metrics facade | `metrics` | P0 定义稳定指标名和 label，exporter 可按部署启用 |
| Error | `thiserror`, `anyhow` | domain error 和 binary/CLI 边界错误 |
| CLI | `clap` | `agentctl` |
| ID / time | `uuid`, `time` | id、timestamp |
| Config | `config`, `dotenvy` | 配置文件、环境变量、本地开发 |
| Secrets | `secrecy`, `zeroize` | token / credential 包装，避免 Debug 泄露 |
| JWT / claims | `jsonwebtoken` | service token、user claims |
| OpenAPI | `utoipa`, `utoipa-swagger-ui` | 内部 API 文档，仅内网 |

### P0 预埋、按需启用

| 领域 | crate | 使用条件 |
|---|---|---|
| Prometheus exporter | `metrics-exporter-prometheus` | P0 可 feature-gated 启用；不影响 metrics facade |
| OpenTelemetry exporter | `tracing-opentelemetry`, `opentelemetry-otlp` | P0 保留 feature gate；有 collector 和跨服务 trace 需求时启用 |
| Redis adapter | `redis` | P1/P2 需要更高吞吐 queue/cache 时启用；P0 正确性不依赖 Redis |
| Rate limit | `governor` | Tower limit 不能满足多 key 限流时启用 |
| Integration test | `testcontainers`, `wiremock` | Postgres / Hermes / connector 集成测试 |
| Snapshot / property | `insta`, `proptest` | API 响应、策略和状态机测试 |

### 暂不作为 P0 默认依赖

| crate / 类型 | 原因 |
|---|---|
| `actix-web` | 本设计更依赖 Tower 生态和中间件组合 |
| `diesel` / `sea-orm` | 控制面依赖显式 SQL、`SKIP LOCKED`、条件更新、审计查询和锁语义；ORM 不提升这部分性能，反而可能隐藏关键 SQL |
| `tonic` | P0/P1 先通过 `RuntimeClient` 固定 transport 边界；如后续 Runtime 确认需要 gRPC，只新增 transport 实现，不改 domain / API / 状态机 |
| `async-graphql` | 当前产品不是图查询入口；REST + OpenAPI 更适合权限审计。若后续需要管理查询聚合，应作为只读 query facade，不替代 Manager API |
| 外部 memory provider SDK | P0 先实现 `MemoryStore` trait + Postgres；外部 provider 后续作为 adapter 接入，不改变 session/message/summary 模型 |
| `tokio` unstable features | 避免运行时行为和编译配置复杂化 |

## 为什么不在 P0 默认启用重依赖

| 类型 | 不默认启用的原因 | P0 必须预留的边界 | 后续启用方式 |
|---|---|---|---|
| ORM | 控制面性能关键在事务、索引、锁和条件更新，不在对象映射；ORM 会削弱 SQL 可审计性 | `agent-store` repository 接口、migration、显式 SQL | 通常不建议替换；如确需引入，只限非关键查询 |
| gRPC / `tonic` | transport 不应决定 Runtime 抽象；P0 需要先稳定 session/run contract | `RuntimeClient` trait、streaming response abstraction、timeout/error model | 新增 `GrpcRuntimeClient`，不改 Manager/Worker/domain |
| GraphQL | 不是用户入口，也不是控制面授权模型；容易扩大查询面 | REST admin/user API、只读 query service 边界 | 只作为只读管理查询 facade，不能替代 mutation API |
| OpenTelemetry | 可观测性必须从 P0 开始，但 collector/exporter 可以按部署启用 | `trace_id`、`tracing` span、metrics name/label、audit trace 关联 | feature-gated exporter，启用后不改业务代码 |
| Redis | P0 正确性依赖 Postgres 事务和锁；过早双写会增加一致性复杂度 | `RunQueue` / cache adapter trait | 高吞吐时新增 Redis adapter，Postgres 仍保留状态源 |
| 外部 memory provider | 会引入隐私、召回质量和一致性问题 | `MemoryStore` trait、summary/result_ref schema | 新增 provider adapter，不能绕过 retention 和 secret scrub |

Cargo 起始草案：

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "signal", "process"] }
axum = { version = "0.8", features = ["macros"] }
tower = { version = "0.5", features = ["timeout", "limit", "util"] }
tower-http = { version = "0.6", features = ["trace", "request-id", "sensitive-headers", "limit", "cors"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "uuid", "time", "json", "migrate", "macros"] }
reqwest = { version = "0.13", default-features = false, features = ["rustls", "json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive", "env"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
metrics = "0.24"
thiserror = "2"
anyhow = "1"
uuid = { version = "1", features = ["v7", "serde"] }
time = { version = "0.3", features = ["serde", "formatting", "parsing"] }
config = { version = "0.15", default-features = false, features = ["toml"] }
dotenvy = "0.15"
secrecy = { version = "0.10", features = ["serde"] }
zeroize = "1"
jsonwebtoken = { version = "10", default-features = false, features = ["rust_crypto", "use_pem"] }
utoipa = { version = "5", features = ["axum_extras", "uuid", "time"] }
utoipa-swagger-ui = { version = "9", features = ["axum"] }
```

## API 分组

### Orchestrator 可用 API

```http
GET  /v1/my-agents
GET  /v1/my-runs
GET  /v1/my-sessions
POST /v1/agent-requests
GET  /v1/agent-requests/{request_id}
POST /v1/agent-requests/{request_id}/cancel
POST /v1/my-agents/{agent_id}/runs
POST /v1/my-agents/{agent_id}/sessions
GET  /v1/agent-sessions/{session_id}
POST /v1/agent-sessions/{session_id}/messages
POST /v1/agent-sessions/{session_id}/child-sessions
GET  /v1/agent-sessions/{session_id}/children
POST /v1/agent-sessions/{session_id}/close
```

Orchestrator 禁止调用：

```http
POST   /v1/admin/agents
PATCH  /v1/admin/agents/{agent_id}
DELETE /v1/admin/agents/{agent_id}
POST   /v1/admin/grants
GET    /v1/admin/audit
GET    /v1/admin/observer/reports
POST   /v1/admin/observer/runs
```

### 管理员 CLI API

```http
GET    /v1/admin/requests
POST   /v1/admin/requests/{request_id}/approve
POST   /v1/admin/requests/{request_id}/deny
GET    /v1/admin/agents
POST   /v1/admin/agents/{agent_id}/pause
POST   /v1/admin/agents/{agent_id}/resume
DELETE /v1/admin/agents/{agent_id}
GET    /v1/admin/audit
POST   /v1/admin/grants
GET    /v1/admin/observer/reports
GET    /v1/admin/observer/reports/{report_id}
POST   /v1/admin/observer/runs
```

### 系统内部 API

```http
POST /v1/internal/webhooks/{connector}
POST /v1/internal/runs
POST /v1/internal/runs/{run_id}/claim
POST /v1/internal/runs/{run_id}/heartbeat
POST /v1/internal/runs/{run_id}/finish
POST /v1/internal/runs/{run_id}/dead-letter
POST /v1/internal/sessions/{session_id}/messages
GET  /v1/internal/sessions/{session_id}/context
POST /v1/internal/memory/summaries
POST /v1/internal/observer/tick
```

## P0 并发与队列选择

P0 使用 Postgres 作为 run queue、lease 和 resource lock 的一致性边界。Redis 只作为后续性能扩展，不参与 P0 正确性。

```text
1. 创建类 API 必须支持 idempotency_key。
2. Worker 使用 SELECT ... FOR UPDATE SKIP LOCKED claim queued run。
3. run claim 必须写 lease_owner、lease_until 和 claimed_at。
4. Worker 必须定期 heartbeat 延长 lease_until。
5. lease_until 过期后，run 可被其他 worker 接管或标记 timed_out。
6. 同一 agent 默认只允许一个带副作用的 active run。
7. 同一 resource 默认只允许一个带副作用的 active run。
8. resource lock 以 resource_type + resource_id + lock_scope 建模，必须有 lease_until。
9. 审批、取消、暂停、finish 必须使用条件更新和 version 字段。
10. session message 必须按 session_id + sequence 顺序追加。
11. Observer 同一时间只允许一个 active observer run，且只能获取只读摘要快照。
12. 服务关闭时停止 claim 新 run，释放或续租已有 lease，并写 audit。
```

## 数据表

P0 必须包含以下表。字段是实现基线，后续可以加字段，但不得删除关键审计、隔离和唯一性字段。

```sql
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    display_name TEXT,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE roles (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE service_accounts (
    id TEXT PRIMARY KEY,
    service_name TEXT NOT NULL,
    status TEXT NOT NULL,
    allowed_actions JSONB NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE resource_bindings (
    id TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    owner_user TEXT NOT NULL,
    attributes JSONB NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE agent_templates (
    agent_type TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    allowed_triggers JSONB NOT NULL,
    allowed_actions JSONB NOT NULL,
    default_constraints JSONB NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE agent_instances (
    id TEXT PRIMARY KEY,
    agent_type TEXT NOT NULL,
    hermes_profile TEXT NOT NULL,
    owner_user TEXT NOT NULL,
    target_resource TEXT NOT NULL,
    core_constraints_hash TEXT NOT NULL,
    status TEXT NOT NULL,
    display_name TEXT,
    config JSONB NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE UNIQUE INDEX ux_agent_instances_reuse_key
ON agent_instances(owner_user, agent_type, target_resource, core_constraints_hash)
WHERE status IN ('provisioning', 'running', 'paused', 'failed');

CREATE TABLE agent_policies (
    id TEXT PRIMARY KEY,
    agent_id TEXT,
    agent_type TEXT,
    policy JSONB NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE agent_requests (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT,
    requested_by_user TEXT NOT NULL,
    requested_by_service TEXT NOT NULL,
    request_type TEXT NOT NULL,
    agent_type TEXT,
    target_resource TEXT,
    intent_text TEXT,
    structured_payload JSONB NOT NULL,
    status TEXT NOT NULL,
    denial_reason TEXT,
    approval_id TEXT,
    result_agent_id TEXT,
    result_run_id TEXT,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE UNIQUE INDEX ux_agent_requests_idempotency
ON agent_requests(requested_by_user, requested_by_service, idempotency_key)
WHERE idempotency_key IS NOT NULL;

CREATE TABLE approval_requests (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    requested_by_user TEXT NOT NULL,
    approver_user TEXT,
    status TEXT NOT NULL,
    risk_level TEXT,
    reason TEXT,
    decision_reason TEXT,
    created_at TIMESTAMP NOT NULL,
    decided_at TIMESTAMP
);

CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    owner_user TEXT NOT NULL,
    source_conversation_id TEXT,
    parent_session_id TEXT,
    created_by_session_id TEXT,
    status TEXT NOT NULL,
    depth INT NOT NULL DEFAULT 0,
    resource_scope JSONB NOT NULL,
    context_summary TEXT,
    version BIGINT NOT NULL DEFAULT 0,
    expires_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE agent_session_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    role TEXT NOT NULL,
    content_ref TEXT,
    content_summary TEXT,
    run_id TEXT,
    created_at TIMESTAMP NOT NULL
);

CREATE UNIQUE INDEX ux_session_messages_sequence
ON agent_session_messages(session_id, sequence);

CREATE TABLE agent_runs (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT,
    agent_id TEXT NOT NULL,
    session_id TEXT,
    trigger_type TEXT NOT NULL,
    target_resource TEXT NOT NULL,
    run_status TEXT NOT NULL,
    risk_level TEXT,
    side_effect_mode TEXT NOT NULL,
    lease_owner TEXT,
    lease_until TIMESTAMP,
    retry_count INT NOT NULL DEFAULT 0,
    result_summary TEXT,
    result_ref TEXT,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    claimed_at TIMESTAMP,
    finished_at TIMESTAMP
);

CREATE UNIQUE INDEX ux_agent_runs_idempotency
ON agent_runs(agent_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;

CREATE TABLE agent_run_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_name TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    started_at TIMESTAMP NOT NULL,
    finished_at TIMESTAMP
);

CREATE TABLE resource_locks (
    id TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    lock_scope TEXT NOT NULL,
    holder_run_id TEXT NOT NULL,
    lease_until TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE UNIQUE INDEX ux_resource_locks_active
ON resource_locks(resource_type, resource_id, lock_scope);

CREATE TABLE agent_grants (
    id TEXT PRIMARY KEY,
    subject_type TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    constraints JSONB NOT NULL,
    granted_by TEXT,
    created_at TIMESTAMP NOT NULL,
    expires_at TIMESTAMP
);

CREATE TABLE observer_reports (
    id TEXT PRIMARY KEY,
    observer_run_id TEXT NOT NULL,
    health_status TEXT NOT NULL,
    risk_level TEXT,
    summary TEXT NOT NULL,
    findings JSONB NOT NULL,
    recommendations JSONB NOT NULL,
    evidence_refs JSONB NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE audit_logs (
    id TEXT PRIMARY KEY,
    actor_user TEXT,
    actor_service TEXT,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    decision TEXT,
    reason TEXT,
    request_id TEXT,
    session_id TEXT,
    run_id TEXT,
    approval_id TEXT,
    observer_report_id TEXT,
    trace_id TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);
```

## Webhook Trigger

所有 webhook 必须先规范化：

```json
{
  "trigger_type": "webhook",
  "connector": "generic",
  "event_type": "item.updated",
  "resource": "resource:team/project-alpha",
  "dedupe_key": "connector:event:id",
  "payload_ref": "payload://connector/event/id",
  "received_at": "2026-05-07T00:00:00Z"
}
```

规则：

```text
1. dedupe_key 必填，重复事件幂等返回。
2. payload_ref 指向原始事件，不在 audit、observer_report 或用户响应中展开敏感 payload。
3. Manager 根据 connector、event_type、resource 和 policy 判断是否创建 run。
4. 无权限或不匹配 allowlist 的 webhook 只写 audit，不创建 run。
```

## Retry 和 Dead-letter

```text
1. 每个 run 最多重试 3 次。
2. 退避时间为 30s、120s、300s。
3. retry 只适用于 worker crash、临时网络错误、connector 5xx。
4. 权限拒绝、策略拒绝、审批拒绝、用户取消不重试。
5. 超过重试次数后进入 dead_letter。
6. dead_letter 只允许 admin 通过 agentctl inspect / retry / terminate。
7. 每次 retry 和 dead_letter 状态迁移都写 audit。
```

## 内置 Agent Template

```yaml
agent_type: background_worker
display_name: 通用后台执行 Agent
supported_triggers:
  - manual
  - scheduled
  - webhook
  - session_message
allowed_resource_types:
  - workspace
  - repository
  - issue_tracker
  - database
constraints:
  default_side_effect_mode: approval_required
  max_items_per_run: 8
  max_runtime_seconds: 1800
  max_concurrent_runs_per_agent: 1
  max_concurrent_runs_per_resource: 1
  max_active_agents_per_user: 10
  max_active_agents_per_resource: 3
  max_active_agents_per_user_resource: 1
  max_session_depth: 1
  max_child_sessions_per_parent: 3
  active_child_sessions_per_parent: 2
  protected_scopes:
    - secrets
    - credentials
    - production
    - protected_branch
```

```yaml
agent_type: observer_agent
display_name: 系统观察 Agent
supported_triggers:
  - scheduled
  - admin_manual
allowed_resource_types:
  - agent_platform
constraints:
  default_side_effect_mode: deny
  max_concurrent_observer_runs: 1
  readable_scopes:
    - status_summary
    - audit_summary
    - worker_heartbeat_summary
    - lock_summary
    - error_metrics
  forbidden_scopes:
    - secrets
    - credentials
    - full_prompt
    - full_context
    - raw_internal_logs
```
