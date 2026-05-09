# 内部定义

本文件定义平台内部对象、状态机、权限上下文和隔离规则。产品可见行为见 [03-product-external-definition.md](03-product-external-definition.md)，实现细节见 [05-technical-implementation.md](05-technical-implementation.md)。

## 核心对象

| 对象 | 含义 |
|---|---|
| `agent_template` | 可创建的 Agent 类型、默认约束和允许能力 |
| `agent_instance` | 已创建 Agent 的能力配置，可复用 |
| `agent_request` | 创建、变更、恢复、执行等受控请求 |
| `agent_session` | 长时间交互上下文，按用户、Agent 和资源隔离 |
| `agent_session_message` | session 内顺序追加的消息、摘要或 result_ref |
| `agent_run` | 一次执行，默认异步，带 lease、heartbeat 和状态 |
| `agent_bridge_binding` | Open WebUI user/chat/model 到 `agent_session` 的持久绑定 |
| `agent_bridge_nonce` | 已消费的 Open WebUI Bridge nonce，用于阻止签名 context 重放 |
| `resource_lock` | 外部动作并发锁 |
| `external_action_plan` | P1 预置的外部动作计划；P1 dry-run，P2 才执行 |
| `credential_lease` | 最小权限 credential 的短期租约引用；不保存明文 secret |
| `approval_request` | 需要人工审批的动作 |
| `audit_log` | 控制面和执行面的审计记录 |
| `observer_report` | Observer 生成的只读评测报告 |
| `system_observer_status_session` | 由普通 `agent_session` 承载的只读系统状态诊断会话，绑定脱敏 `observer_report` packet |

## Agent Template

v1 内置两个 template：

```text
background_worker:
  用途：后台任务、监控、分析、准备变更、运行检查
  默认外部动作模式：approval_required
  触发方式：manual / scheduled / webhook / session_message

observer_agent:
  用途：系统监控、评测、建议和只读状态诊断
  默认外部动作模式：deny
  触发方式：scheduled / admin_manual / system_status_session
  credential：只读 snapshot，不持有写权限 credential
```

动态 Agent 创建只允许来自 allowlisted template，不允许任意 prompt/template 直接变成常驻 Agent。

## 隔离模型

Agent 复用能力，不复用状态：

```text
agent_instance  = 可复用能力配置
agent_session   = 隔离的长交互上下文
agent_run       = 隔离的一次执行
agent_bridge_binding = 按 Open WebUI subject + chat_id + model 隔离
memory          = 按 session_id 隔离
workdir         = 按 run_id/session_id 隔离
observer_report = 只读评测产物，不含 secrets
system_observer_status_session = 普通 agent_session，绑定 dedicated observer_agent 和脱敏 report packet
credential      = P0/P1 不注入写权限；P2 由 Manager 按 scope 临时注入最小权限引用
```

`agent_instance` 不得长期保存 credential。Credential 不得进入 prompt、memory、observer_report 或 audit 明文。

## Open WebUI Bridge Binding

Bridge binding 是 Open WebUI 到 Agent Platform 的身份和 session 桥接对象，不是权限授予对象。

```text
binding key:
  open_webui_subject
  open_webui_chat_id
  model

binding value:
  agent_id
  agent_session_id
  status
  last_message_id
  last_run_id
  trace_id
```

规则：

```text
1. Orchestrator 只有在验证 `agent_bridge_context` 后才能读写 binding。
2. Mutating Bridge 请求必须先 claim `agent_bridge_nonce`；同一 subject/chat/model/nonce 只能消费一次。
3. Manager 按 `auth.user_id` 约束 subject；调用者不能传入任意 subject 读取他人 binding。
4. 同一 Open WebUI user/chat/model 只能有一个 active binding。
5. 同一 reusable agent 可以被多个 Open WebUI chat 复用，但每个 chat 使用独立 agent_session。
6. 后续消息通过 binding 追加 session message，并创建 read-only session_message run。
7. Open WebUI `user_message_id` 优先映射为 session message `external_message_id`，缺失时退回 `message_id`；同一 session 内重复用户消息不重复 append。
8. 关闭 session 时必须把 binding 标记为 closed。
9. binding upsert、close、run update 必须写 audit；closed binding 不允许继续 update run。
10. Bridge source 不参与 agent 复用 hash，避免同一 agent 因 chat id 不同被重复创建。
```

## 权限模型

采用 RBAC + ABAC。RBAC 角色：

```text
system_admin
agent_admin
resource_owner
resource_maintainer
operator
viewer
```

ABAC 上下文：

```text
action
agent_type
resource_type / resource_id / resource_owner / resource_attributes
scope / protected_scopes / environment
risk_level / external_action_mode / external_action_intent
credential_scope
source_service / source_gateway / user_id
session_id / parent_session_id
observer_mode
```

授权判断：

```text
authorize(service, user, action, resource, context):
  1. 校验 service 和 user。
  2. 校验 action、resource allowlist、template、policy、risk_level、external_action_mode。
  3. 需要人工审批时返回 approval_required。
  4. 需要资源锁时必须先获取 resource_lock。
  5. 所有允许、拒绝、冲突和审批结果都写 audit。
```

## Side-effect Contract

P1 先固定 P2 所需 contract，但不执行真实外部写入。

```text
external_action_plan:
  run_id / connector / action / resource_ref
  risk_level / external_action_mode / credential_scope / approval_id
  input_summary / input_ref / result_ref
  status / trace_id

credential_lease:
  external_action_plan_id / credential_scope / provider_ref
  status / expires_at / trace_id
```

阶段语义：

```text
P1:
  - 可创建 external_action_plan 草案、执行 policy dry-run、校验 approval / lock / credential_scope。
  - 可实现 no-op CredentialProvider 和 no-op WriteConnector。
  - 不进入真实 applying_external_actions，不获取真实 credential，不调用真实写 connector。
  - dry-run、拒绝、缺少审批、缺少 lock、缺少 credential_scope 都写 audit。

P2:
  - 只新增真实 CredentialProvider 和 WriteConnector adapter。
  - 只推进 P1 已存在的 external_action_plan 状态。
  - 不改 request、run、session、audit、resource_lock、observer_report 的基线模型。
```

## 状态机

```text
agent_request:
  requested → parsed → policy_checked
    ├─ denied
    ├─ approval_required
    └─ approved → provisioning/enqueued → fulfilled
  any → cancelled / expired / failed

agent_instance:
  provisioning → running → paused / terminated
  any → failed

agent_session:
  created → active → closing → closed
  any → expired / failed

agent_run:
  queued → claimed → context_built → policy_checked → executing → validating
    ├─ awaiting_approval
    ├─ applying_external_actions
    └─ completed
  any → failed / cancelled / timed_out / dead_letter

observer_report:
  scheduled/admin_requested → snapshot_collected → evaluated → reported
  any → failed
```

Observer report 不改变系统状态。报告建议需要落地时，必须转换成新的 request 或管理员操作。

## Observer Report Discussion

P1 的报告讨论复用普通 `agent_session`，不是新增控制能力。

```text
observer_report
  → authorized discussion request
  → Agent Manager creates target agent_session
  → Runtime discussion
  → summary / requirement draft / request suggestion
```

约束：

```text
1. discussion session 绑定 report_id、target agent_id、requesting user、trace_id。
2. context 只包含 report summary、findings、recommendations、evidence_refs、snapshot_ref 和脱敏 resource refs。
3. 不复制完整 snapshot、完整 prompt、完整 context、内部日志或 credential。
4. discussion session 不得直接 approve / deny / pause / resume / delete / grant / retry / terminate。
5. 需要改变系统状态时，必须创建普通 agent_request 或调用已有 admin API，并单独写 audit。
```

## System Observer Status Session

System Observer status session 也是普通 `agent_session`，但目标 Agent 是 dedicated `observer_agent`。它用于让授权 admin/operator 在一个可追问的会话中查看当前系统状态和最新 report 解释，不是 Open WebUI 直连 Manager，也不是 Observer 控制面。

```text
system status request
  → Orchestrator validates bridge context and system-status intent
  → Manager authorizes operator/admin
  → Manager selects explicit or latest observer_report
  → Manager creates/reuses dedicated observer_agent
  → Manager creates ordinary agent_session with redacted report packet
  → Orchestrator returns safe session/report summary
```

固定约束：

```text
1. dedicated observer_agent 的 target_resource 固定为 `resource:system/agent-platform`。
2. 会话幂等键使用 `system-observer:status-session` 前缀，避免重复创建同一状态入口。
3. 初始 system message 只包含脱敏 report packet、health_status、risk_level、summary、findings、recommendations、evidence_refs 和 trace_id。
4. 初始 user message 是用户的系统状态问题；缺省问题由 Manager 生成，不包含 secret。
5. service JWT 只允许 `admin:observer_discuss` 和必要的 `session:*` 动作；user JWT 必须是授权 operator/admin。
6. Open WebUI admin 到 Agent Platform role 的映射只在 observer-admin role mapping 中生效，不影响普通 Bridge admin mapping。
7. 会话不得 approve / deny / pause / resume / delete / grant / retry / terminate。
8. 会话不得注入完整 snapshot、完整 prompt、完整 context、内部日志、credential、token、私钥或 `.env`。
```

## Agent 复用与变更

Agent 唯一性：

```text
owner_user + agent_type + target_resource + core_constraints_hash
```

`core_constraints_hash` 只包含影响权限边界、资源范围、触发模式和外部动作能力的约束。展示名称、描述文案、非权限标签不进入 hash。

处理规则：

```text
1. 同一唯一键且 existing agent 为 provisioning / running / paused / failed 时，默认复用。
2. 请求配置一致时返回 existing agent 摘要。
3. 请求配置不一致但唯一键一致时创建 change_request，不覆盖 existing agent。
4. 权限、资源范围或外部动作权限扩大时进入审批。
5. existing agent 为 terminated 时不复用，允许重新创建 request。
```

## Memory / Session 策略

```text
1. 每个 session 保留最近 30 条原文消息。
2. 超出窗口的消息进入 rolling summary。
3. 单个 context_summary 目标上限为 8k tokens。
4. summary 不保存 secrets、credential、私钥、token 或完整敏感 payload。
5. Runtime 构建上下文时优先使用最近消息，再补充 summary 和必要 resource refs。
6. 关闭或过期 session 后，按 retention 策略保留 summary、result_ref 和 audit 关联。
```

## Child Session

v1 固定限制：

```text
root session 深度为 0；child session 深度为 1；child 不得再创建 child。
每个 parent session 默认最多 3 个 child，active child 默认最多 2 个。
child session 默认并行执行。
child 完成后只回写 summary + result_ref + run_id。
child 不继承完整 context 或 parent credential。
P0/P1 child 不注入写权限 credential；P2 如需 credential，必须由 Manager 按 child scope 临时注入。
```

回写格式：

```json
{
  "child_session_id": "sess_child_001",
  "parent_session_id": "sess_parent_001",
  "status": "completed",
  "summary": "完成专项分析，发现两个需要父会话继续处理的问题。",
  "result_ref": "result://agent-runs/run_001",
  "run_id": "run_001"
}
```

## Observer Agent

Observer 只读取聚合摘要：

```text
agent/session/run 状态、耗时、失败、重试、lease 摘要
resource_locks 占用和超时摘要
approval / audit 决策摘要
worker heartbeat / timeout / dead_letter 摘要
Runtime 延迟、失败、重试、上下文膨胀和异常建议摘要
```

Observer 输出：

```text
health_status
risk_findings
anomaly_patterns
recommendations
recommended_priority
evidence_refs
redacted_status_session_context
```

Observer 硬性限制：

```text
1. 不持有写权限 credential。
2. 不读取 secrets、完整 prompt、完整 context 或完整内部日志。
3. 不 approve / deny / pause / resume / delete / grant。
4. 不自动修改配置，不自动创建高权限 Agent。
5. 建议必须转成普通 request 或管理员操作后重新走策略和审批。
6. 每次运行必须写 audit，并绑定 observer_report。
```

## Agent 状态动作矩阵

| Agent 状态 | 普通用户 | resource owner | admin |
|---|---|---|---|
| provisioning | 查询进度 | 查询进度 / cancel request | inspect / cancel |
| running | 创建 session / 创建 run / 查询摘要 | 创建 session / 创建 run / request pause / request change | pause / inspect / delete |
| paused | 只读查询 | request resume | resume / inspect / delete |
| failed | 查看安全摘要 | request restart | inspect / restart / delete |
| terminated | 只读查询历史，不复用 | 重新创建 request | inspect / purge metadata |
