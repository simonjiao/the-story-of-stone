# 内部定义

本文件定义平台内部对象、状态机、权限上下文、隔离规则和 v1 固定约束。

## 核心对象

| 对象 | 含义 |
|---|---|
| `agent_template` | 可被创建的 Agent 类型定义、默认约束和允许能力 |
| `agent_instance` | 已创建 Agent 的能力配置，可复用 |
| `agent_request` | 创建、变更、恢复、执行等受控请求 |
| `agent_session` | 长时间交互上下文，按用户、Agent 和资源隔离 |
| `agent_session_message` | session 内顺序追加的消息、摘要或 result_ref |
| `agent_run` | 一次执行，默认异步，带 lease、heartbeat 和状态 |
| `resource_lock` | 外部副作用并发锁 |
| `approval_request` | 需要人工审批的动作 |
| `audit_log` | 控制面和执行面的审计记录 |
| `observer_report` | Observer Agent 生成的只读评测报告和建议 |

## Agent Template

v1 内置两个 template：

```text
background_worker:
  用途：通用后台任务、监控、分析、准备变更、运行检查。
  默认副作用：approval_required。
  触发方式：manual / scheduled / webhook / session_message。

observer_agent:
  用途：系统监控、评测、建议。
  默认副作用：deny。
  触发方式：scheduled / admin_manual。
  credential：只读 snapshot，不持有写权限 credential。
```

动态 Agent 创建只允许来自 allowlisted template，不允许任意 prompt/template 直接变成常驻 Agent。

## 隔离模型

Agent 复用的是能力，不复用状态：

```text
agent_instance = 可复用的能力配置
agent_session  = 隔离的长交互上下文
agent_run      = 隔离的一次执行
credential     = 每次 session/run 按 scope 重新注入
memory         = 按 session_id 隔离
workdir        = 按 run_id/session_id 隔离
observer_report = 只读评测产物，不含 secrets
```

`agent_instance` 不得长期保存 credential。Credential 只能由 Manager 在 session/run 执行前根据最小权限 scope 临时注入。

## 权限模型

采用 RBAC + ABAC。

RBAC 角色：

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
resource_type
resource_id
resource_owner
scope
environment
resource_attributes
protected_scopes
risk_level
side_effect_mode
schedule
credential_scope
source_service
source_gateway
user_id
session_id
parent_session_id
observer_mode
```

授权判断：

```text
authorize(service, user, action, resource, context):
  1. service 必须可信。
  2. user 必须有效。
  3. user 必须具备 action 权限。
  4. resource 必须在 allowlist 或授权范围内。
  5. agent_template / policy 必须允许该 action。
  6. risk_level 和 side_effect_mode 必须满足策略。
  7. 需要审批时返回 approval_required。
  8. 需要资源锁时必须获取 resource_lock。
  9. 所有结果必须写 audit。
```

## 状态机

### Agent Request

```text
requested
  ↓
parsed
  ↓
policy_checked
  ├─ denied
  ├─ approval_required
  └─ approved
       ↓
   provisioning / enqueued
       ↓
   fulfilled

任意阶段可进入：
cancelled / expired / failed
```

### Agent Instance

```text
provisioning
  ↓
running
  ├─ paused
  └─ terminated

任意阶段可进入：
failed
```

### Agent Session

```text
created
  ↓
active
  ↓
closing
  ↓
closed

任意阶段可进入：
expired / failed
```

### Agent Run

```text
queued
  ↓
claimed
  ↓
context_built
  ↓
policy_checked
  ↓
executing
  ↓
validating
  ├─ awaiting_approval
  ├─ applying_side_effects
  └─ completed

任意阶段可进入：
failed / cancelled / timed_out / dead_letter
```

### Observer Report

```text
scheduled / admin_requested
  ↓
snapshot_collected
  ↓
evaluated
  ↓
reported

任意阶段可进入：
failed
```

Observer report 不改变系统状态。报告中的建议需要执行时，必须转换成新的 request 或管理员操作。

## 已存在 Agent 复用与变更

Agent 唯一性：

```text
owner_user
agent_type
target_resource
core_constraints_hash
```

`core_constraints_hash` 只包含影响权限边界、资源范围、触发模式和副作用能力的约束。展示名称、描述文案、非权限标签不进入 hash。

处理规则：

```text
1. 命中同一唯一键且 existing agent 为 running / paused / provisioning 时，默认复用。
2. 请求配置完全一致时，返回 existing agent 摘要，不创建新 agent。
3. 请求配置不一致但唯一键一致时，创建 change_request，不覆盖 existing agent。
4. change_request 必须走 Manager 策略；权限、资源范围或副作用扩大时进入审批。
5. existing agent 为 terminated 时不复用，允许创建新的 agent request。
```

## Memory / Session 策略

```text
1. 每个 session 保留最近 30 条原文消息。
2. 超出最近窗口的消息进入滚动 summary。
3. 单个 context_summary 目标上限为 8k tokens。
4. summary 不保存 secrets、credential、私钥、token 或完整敏感 payload。
5. Runtime 构建上下文时优先使用最近消息，再补充 summary 和必要 resource refs。
6. 关闭或过期 session 后，按 retention 策略保留 summary、result_ref 和 audit 关联。
```

## Child Session

v1 固定限制：

```text
root session 深度为 0。
child session 深度为 1。
child session 不得再创建 child。
每个 parent session 默认最多 3 个 child session。
active child session 默认最多 2 个。
child session 默认并行执行。
child session 完成后只回写 summary + result_ref + run_id。
child session 不继承完整 context。
child session 不继承 parent credential，由 Manager 重新注入最小权限 credential。
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

输入范围：

```text
agent_instances 状态摘要
agent_sessions 状态和统计摘要
agent_runs 状态、耗时、失败、重试、lease 摘要
resource_locks 当前占用和超时摘要
approval_requests 决策摘要
audit_logs 决策摘要
worker heartbeat / timeout / dead-letter 摘要
Runtime 延迟、失败、重试和上下文膨胀摘要
```

输出范围：

```text
health_status
risk_findings
anomaly_patterns
recommendations
recommended_priority
evidence_refs
```

硬性限制：

```text
1. Observer Agent 只读，不持有写权限 credential。
2. Observer Agent 不读取 secrets、完整 prompt、完整 context 或完整内部日志。
3. Observer Agent 不直接 approve / deny / pause / resume / delete / grant。
4. Observer Agent 不自动修改配置，不自动创建高权限 Agent。
5. Observer Agent 建议必须转成普通 request 或管理员操作后重新走策略和审批。
6. Observer Agent 每次运行必须写 audit，并绑定 observer_report。
```

## Agent 状态动作矩阵

| Agent 状态 | 普通用户 | resource owner | admin |
|---|---|---|---|
| provisioning | 查询进度 | 查询进度 / cancel request | inspect / cancel |
| running | 创建 session / 创建 run / 查询摘要 | 创建 session / 创建 run / request pause / request change | pause / inspect / delete |
| paused | 只读查询 | request resume | resume / inspect / delete |
| failed | 查看安全摘要 | request restart | inspect / restart / delete |
| terminated | 只读查询历史，不复用 | 重新创建 request | inspect / purge metadata |
