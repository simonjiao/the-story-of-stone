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
agent-orchestrator 不访问通用 admin API，不持有目标 Agent credential；唯一例外是已验证 Open WebUI admin/operator 的 System Observer status session 窄口。
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

Orchestrator 默认禁止调用 admin、observer report 查询和普通 observer discussion API。它只允许两类 Manager 非 user API：

```text
1. `internal:open_webui_bridge:*` namespace 下的 Open WebUI Bridge endpoint，且必须先验证 `agent_bridge_context`。
2. `POST /v1/admin/observer/system-session`，仅限已验证 bridge context、系统状态/Observer 报告意图、Open WebUI admin 经 observer-admin role mapping 映射为授权 operator/admin 的请求。
```

System Observer status session 窄口不得扩展为通用 admin proxy。Orchestrator 给该请求签发的 service JWT 只能包含 `admin:observer_discuss` 和必要的 `session:*`，user JWT 只能使用 observer-admin role mapping 生成的 operator/admin role。

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

P1 新增 admin API：

```http
POST /v1/admin/observer/reports/{report_id}/discussions
POST /v1/admin/observer/system-session
POST /v1/admin/runs/{run_id}/external-action-plans/dry-run
```

P2 新增 admin API：

```http
POST /v1/admin/runs/{run_id}/external-action-plans/{plan_id}/apply
POST /v1/admin/runs/{run_id}/external-action-plans/{plan_id}/compensate
```

Observer discussion API 只允许管理员或授权 operator 使用。它创建普通 `agent_session`，写入脱敏 report context 和 initial user message，并在 audit 中关联 `report_id / session_id / agent_id / trace_id`。它不得调用 Observer 控制动作，不得注入完整 snapshot、完整 prompt、完整 context、内部日志或 credential。

System Observer status session API 只允许管理员或授权 operator 使用。它接受可选 `report_id`、`message` 和 `idempotency_key`，选择指定或最新 `observer_report`，确保 dedicated `observer_agent` 存在，创建普通 `agent_session`，并写入脱敏 report packet 与用户问题。返回只包含 `report_id / agent_id / session_id / health_status / risk_level / summary / trace_id` 等安全摘要。它不得查询或返回原始 snapshot，不得执行控制动作。

`external-action-plans/dry-run` 只允许管理员使用。它固定 P2 external-action contract，创建 `external_action_plan`，校验 approval 状态、active resource lock、credential_scope 和 critical risk，必要时通过 no-op `CredentialProvider` 创建 `credential_lease` dry-run 记录，并写 audit。P1 不获取真实 credential，不调用真实写 connector，不推进到真实外部写入。

`external-action-plans/{plan_id}/apply` 只允许管理员使用。它只接受 `dry_run_ready` plan，重新校验 approval、credential_scope、critical risk 和 resource lock，通过 CredentialProvider 获取 active opaque `provider_ref`，再调用 WriteConnector execute。execute 请求必须携带以 `external_action_plan.id` 派生的 `idempotency_key`。connector 成功响应必须包含 `status=applied`、`result_ref` 和 `compensation_ref`；缺失时 Manager 将 plan 标记为 `connector_invalid_result`。credential secret 不进入 Manager、Runtime、Memory 或 audit 明文；失败会推进 `external_action_plan.status=failed` 并写入安全 `error_code`。当前实现提供通用 HTTP adapter，并提供可部署的 `action-journal` provider/connector/JSONL target adapter 用于低风险 external action smoke；第三方生产 provider / connector 必须按同一 contract 单独配置并复测。

`external-action-plans/{plan_id}/compensate` 只允许管理员使用。它只接受 `applied` plan，要求 plan 已持久化 `compensation_ref`，通过 WriteConnector 调用 `POST /action-executions/compensate`，成功响应必须包含 `status=compensated` 和 `result_ref`。Manager 会把 plan 推进到 `compensated`，保存 `compensation_result_ref`，并写入 `admin:external_action_compensate` audit。

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
5. 同一 agent 默认只允许一个带外部动作 active run。
6. 同一 resource 默认只允许一个带外部动作 active run。
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
| `agent_runs` | `idempotency_key`、`agent_id`、`session_id`、`trigger_type`、`target_resource`、`run_status`、`risk_level`、`external_action_mode`、lease fields、retry fields、result fields、`version`；run 幂等和 retry-ready 索引 |
| `open_webui_bridge_bindings` | `open_webui_subject`、`open_webui_chat_id`、`open_webui_session_id`、`model`、`agent_id`、`agent_session_id`、`status`、`last_message_id`、`last_run_id`、`trace_id`、`version`；active binding 唯一键为 subject + chat + model |
| `open_webui_bridge_nonces` | `open_webui_subject`、`open_webui_chat_id`、`model`、`nonce`、`issued_at`、`trace_id`；唯一键为 subject + chat + model + nonce |
| `agent_run_steps` | `run_id`、`step_name`、`status`、`summary`、timestamps |
| `resource_locks` | `resource_type`、`resource_id`、`lock_scope`、`holder_run_id`、`lease_until`；active lock 唯一 |
| `agent_grants` | `subject_type`、`subject_id`、`action`、resource、`constraints`、`granted_by`、`expires_at` |
| `observer_reports` | `observer_run_id`、`health_status`、`risk_level`、`summary`、`findings`、`recommendations`、`evidence_refs` |
| `audit_logs` | actor、`action`、resource、`decision`、`reason`、request/session/run/approval/report ids、`trace_id` |

System Observer status session 不新增专用表；它复用 `agent_instances` 中 dedicated `observer_agent`、`agent_sessions`、`agent_session_messages`、`observer_reports` 和 `audit_logs`，用 `idempotency_key`、`report_id`、`session_id`、`agent_id` 与 `trace_id` 建立关联。

P1 新增 data model：

| 表 | 关键字段 / 约束 |
|---|---|
| `external_action_plans` | `run_id`、connector/action/resource、risk/mode、`approval_id`、`credential_scope`、input/result refs、`compensation_ref`、`compensation_result_ref`、`status`、`error_code`、`version`、`trace_id` |
| `credential_leases` | `external_action_plan_id`、`credential_scope`、opaque `provider_ref`、`status`、`expires_at`、`trace_id`、`revoked_at` |

P1 新增 schema 先用于 dry-run 和 contract test；P2 使用同一 schema 推进 `dry_run_ready -> applied|failed`，并通过 active `credential_leases.provider_ref` 调用真实 provider / connector。

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
9. Open WebUI admin 只管理 Function 和 Valves，不默认映射为通用 Agent Platform admin。
10. System Observer status session 使用独立 `AGENT_BRIDGE_OBSERVER_ADMIN_ROLE_MAPPING`；生产默认可把 Open WebUI admin 映射为 Agent Platform operator，仅用于 `POST /v1/admin/observer/system-session`。
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
  default_external_action_mode: approval_required
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
supported_triggers: [scheduled, admin_manual, system_status_session]
allowed_resource_types: [agent_platform]
constraints:
  default_external_action_mode: deny
  max_concurrent_observer_runs: 1
  readable_scopes: [status_summary, audit_summary, worker_heartbeat_summary, lock_summary, error_metrics]
  forbidden_scopes: [secrets, credentials, full_prompt, full_context, raw_internal_logs]
  status_session_context: redacted_report_packet_only
```
