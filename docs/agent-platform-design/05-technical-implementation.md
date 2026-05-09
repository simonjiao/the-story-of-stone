# 技术实现细节

本文件定义 Rust 落地契约。原则见 [01-design-principles.md](01-design-principles.md)，内部模型见 [04-internal-definition.md](04-internal-definition.md)。实现不得新增绕过 Manager 的隐式能力。

## Rust Workspace

```text
agent-platform/
  crates/
    agent-core/           # domain model、状态枚举、错误、ID、policy context、trait 边界
    agent-store/          # Postgres、migration、transaction、repository
    agent-manager/        # 唯一控制面：auth、policy、approval、audit、observer report
    agent-orchestrator/   # Gateway、intent routing、bridge/session routing、streaming、安全错误摘要
    agent-runtime/        # Runtime trait、Minimal Runtime、P1 HermesRuntimeClient、context assembly
    agent-worker/         # run claim、heartbeat、timeout、retry、resource lock、observer tick
    agentctl/             # 管理员 CLI
```

模块边界：

```text
agent-core 不依赖 axum/sqlx/reqwest，只保存类型和纯逻辑。
agent-store 只处理数据库和事务，不做产品策略。
agent-manager 是唯一控制面服务。
agent-orchestrator 不访问 admin API，不持有目标 Agent credential。
agent-runtime 不决定授权，只执行已授权 session/run。
agent-worker 只推进 Manager 已创建和授权的 run；状态推进必须写 audit。
agentctl 只调用 admin API。
```

## 依赖策略

P0 直接依赖只服务最小闭环：`tokio`、`axum`、`tower`、`serde`、`sqlx`、`reqwest`、`tracing`、`metrics`、`thiserror`、`anyhow`、`clap`、`uuid`、`time`、`config`、`dotenvy`、`secrecy`、`zeroize`、`jsonwebtoken`、`utoipa`。

以下能力必须先固定边界，再按 feature 或 adapter 启用实现：

| 能力 | P0/P1 边界 | 后续启用方式 |
|---|---|---|
| Runtime transport | `RuntimeClient` trait、streaming abstraction、timeout/error model | 新增 Hermes HTTP 或 gRPC client，不改 Manager/Worker/domain |
| Memory provider | `MemoryStore` trait、summary/result_ref schema | 新增 provider adapter，不能绕过 retention 和 secret scrub |
| Connector | `ConnectorClient` 只读 snapshot；`WriteConnector` contract 在 P1 dry-run | P2 启用真实 write connector |
| Credential | `CredentialProvider` contract 和 `credential_lease` schema；P1 no-op | P2 启用真实 provider，secret 不落库、不进 prompt/audit 明文 |
| Queue/cache | `RunQueue` trait；P0 Postgres lease 是正确性源 | 高吞吐时新增 Redis adapter，Postgres 仍保留状态源 |
| Telemetry | trace_id、tracing span、metrics name/label、audit trace 关联 | 按部署启用 OpenTelemetry / Prometheus exporter |

ORM、GraphQL、外部 memory SDK、gRPC 或 exporter 不是永久禁止；禁止的是让它们反向改变 P0/P1 的 domain model、API contract、状态机和审计模型。

## API 分组

### Orchestrator 可用 API

```http
GET  /v1/my-agents
GET  /v1/my-runs
GET  /v1/my-runs/{run_id}
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

Orchestrator 禁止调用 admin、observer report 查询和 observer discussion API。internal API 中只允许调用 `internal:open_webui_bridge:*` namespace 下的 Open WebUI Bridge endpoint，且必须先验证 `agent_bridge_context`。

### 管理员 API

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
GET    /v1/admin/runs
GET    /v1/admin/runs/{run_id}
POST   /v1/admin/runs/{run_id}/retry
POST   /v1/admin/runs/{run_id}/terminate
GET    /v1/admin/observer/reports
GET    /v1/admin/observer/reports/{report_id}
POST   /v1/admin/observer/runs
```

P1 计划新增 admin API：

```http
POST /v1/admin/observer/reports/{report_id}/discussions
```

该 API 只允许管理员或授权 operator 使用。它创建普通 `agent_session`，写入脱敏 report context 和 initial user message，并在 audit 中关联 `report_id / session_id / agent_id / trace_id`。它不得调用 Observer 控制动作，不得注入完整 snapshot、完整 prompt、完整 context、内部日志或 credential。

### 系统内部 API

```http
POST /v1/internal/webhooks/{connector}
POST /v1/internal/runs
POST /v1/internal/runs/claim
POST /v1/internal/runs/{run_id}/heartbeat
POST /v1/internal/runs/{run_id}/finish
POST /v1/internal/runs/{run_id}/dead-letter
POST /v1/internal/sessions/{session_id}/messages
GET  /v1/internal/sessions/{session_id}/context
POST /v1/internal/memory/summaries
POST /v1/internal/observer/tick
POST /v1/internal/open-webui-bridge/nonces
GET  /v1/internal/open-webui-bridge/bindings/{chat_id}?model=hermes-agent
PUT  /v1/internal/open-webui-bridge/bindings
POST /v1/internal/open-webui-bridge/bindings/{chat_id}/close
POST /v1/internal/open-webui-bridge/bindings/{binding_id}/run
```

`/v1/internal/runs/claim` 语义是 claim next queued run；后续 heartbeat / finish / dead-letter 才绑定具体 `{run_id}`。

Open WebUI bridge API 只供 Orchestrator 在验证 `agent_bridge_context` 后调用。Manager 以 `auth.user_id` 作为 subject 边界，不能接受外部传入 subject 越权读写 binding。

## 并发、Lease 和锁

P0 使用 Postgres 作为 run queue、lease 和 resource lock 的一致性边界。Redis 只作为后续性能扩展。

```text
1. 创建类 API 必须支持 idempotency_key。
2. Worker 使用 SELECT ... FOR UPDATE SKIP LOCKED claim queued run。
3. claim 必须写 lease_owner、lease_until、claimed_at。
4. heartbeat 延长 lease_until；lease 过期后允许接管或标记 timed_out。
5. 同一 agent 默认只允许一个带副作用 active run。
6. 同一 resource 默认只允许一个带副作用 active run。
7. resource lock 以 resource_type + resource_id + lock_scope 建模，必须有 lease_until。
8. 审批、取消、暂停、finish 必须使用条件更新和 version 字段。
9. session message 必须按 session_id + sequence 顺序追加。
10. Observer 同一时间只允许一个 active observer run，且只能获取只读摘要快照。
11. 服务关闭时停止 claim 新 run，释放或续租已有 lease，并写 audit。
```

## 数据模型

实现基线由 `agent-store` migrations 承载；本文只列不可删除的关键字段和约束。后续可以加字段，但不得删除审计、隔离、幂等、lease、锁和 trace 字段。

| 表 | 关键字段 / 约束 |
|---|---|
| `users` | `id`、`display_name`、`status`、`created_at` |
| `roles` | `user_id`、`role`、`resource_type`、`resource_id` |
| `service_accounts` | `service_name`、`status`、`allowed_actions` |
| `resource_bindings` | `resource_type`、`resource_id`、`owner_user`、`attributes`、`status` |
| `agent_templates` | `agent_type`、`allowed_triggers`、`allowed_actions`、`default_constraints`、`status` |
| `agent_instances` | `agent_type`、`hermes_profile`、`owner_user`、`target_resource`、`core_constraints_hash`、`status`、`config`、`version`；唯一复用键排除 terminated |
| `agent_policies` | `agent_id` 或 `agent_type`、`policy`、`version` |
| `agent_requests` | `idempotency_key`、`requested_by_user`、`requested_by_service`、`request_type`、`structured_payload`、`status`、`approval_id`、result ids、`version`；创建幂等索引 |
| `approval_requests` | `request_id`、`requested_by_user`、`approver_user`、`status`、`risk_level`、decision fields |
| `agent_sessions` | `agent_id`、`owner_user`、`source_conversation_id`、parent/creator session ids、`status`、`depth`、`resource_scope`、`context_summary`、`version` |
| `agent_session_messages` | `session_id`、`sequence`、`role`、`content_ref`、`content_summary`、`external_message_id`、`run_id`；`session_id + sequence` 唯一，`session_id + external_message_id` 在非空时唯一 |
| `agent_runs` | `idempotency_key`、`agent_id`、`session_id`、`trigger_type`、`target_resource`、`run_status`、`risk_level`、`side_effect_mode`、lease fields、retry fields、result fields、`version`；run 幂等和 retry-ready 索引 |
| `open_webui_bridge_bindings` | `open_webui_subject`、`open_webui_chat_id`、`open_webui_session_id`、`model`、`agent_id`、`agent_session_id`、`status`、`last_message_id`、`last_run_id`、`trace_id`、`version`；active binding 唯一键为 subject + chat + model |
| `open_webui_bridge_nonces` | `open_webui_subject`、`open_webui_chat_id`、`model`、`nonce`、`issued_at`、`trace_id`；唯一键为 subject + chat + model + nonce |
| `agent_run_steps` | `run_id`、`step_name`、`status`、`summary`、timestamps |
| `resource_locks` | `resource_type`、`resource_id`、`lock_scope`、`holder_run_id`、`lease_until`；active lock 唯一 |
| `agent_grants` | `subject_type`、`subject_id`、`action`、resource、`constraints`、`granted_by`、`expires_at` |
| `observer_reports` | `observer_run_id`、`health_status`、`risk_level`、`summary`、`findings`、`recommendations`、`evidence_refs` |
| `audit_logs` | actor、`action`、resource、`decision`、`reason`、request/session/run/approval/report ids、`trace_id` |

P1 计划新增 data model：

| 表 | 关键字段 / 约束 |
|---|---|
| `side_effect_plans` | `run_id`、connector/action/resource、risk/mode、`approval_id`、`credential_scope`、input/result refs、`status`、`error_code`、`version`、`trace_id` |
| `credential_leases` | `side_effect_plan_id`、`credential_scope`、opaque `provider_ref`、`status`、`expires_at`、`trace_id`、`revoked_at` |

P1 计划新增 schema 只能用于 dry-run 和 contract test；P2 才能接真实 provider / connector。

## Open WebUI Bridge Contract

Open WebUI Bridge 已是当前实现基线：

```text
1. Open WebUI Function / Filter 注入签名 `agent_bridge_context`。
2. Orchestrator 校验 issuer、subject、chat_id、model、timestamp、nonce 和 HMAC。
3. 控制请求和已绑定 session 消息必须先由 Manager claim nonce；重复 nonce 在同一 subject/chat/model 下拒绝。
4. 控制请求和已绑定 session 消息必须有有效 context；普通聊天可 passthrough，但必须先删除内部 bridge 字段。
5. Orchestrator 向 Manager 签发 service/user JWT；service token 只允许 user/session/run API 和 `internal:open_webui_bridge:*`。
6. Manager 在 request fulfilled 或 approval fulfilled 后根据 `bridge_source` 创建/复用 `agent_session` 并写 `open_webui_bridge_bindings`。
7. 后续消息通过 binding append session message、create read-only run、轮询 `GET /v1/my-runs/{run_id}`；`user_message_id` 优先映射为 `external_message_id` 做 append 幂等，缺失时退回 `message_id`。
8. Bridge binding upsert、close 和 run update 写 audit；closed binding 不允许继续 update run。
9. Open WebUI admin 只管理 Function 和 Valves，不默认映射为 Agent Platform admin。
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
supported_triggers: [manual, scheduled, webhook, session_message]
allowed_resource_types: [workspace, repository, issue_tracker, database]
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
  protected_scopes: [secrets, credentials, production, protected_branch]
```

```yaml
agent_type: observer_agent
display_name: 系统观察 Agent
supported_triggers: [scheduled, admin_manual]
allowed_resource_types: [agent_platform]
constraints:
  default_side_effect_mode: deny
  max_concurrent_observer_runs: 1
  readable_scopes: [status_summary, audit_summary, worker_heartbeat_summary, lock_summary, error_metrics]
  forbidden_scopes: [secrets, credentials, full_prompt, full_context, raw_internal_logs]
```
