# 产品外部定义

本文件定义 Open WebUI、普通用户、resource owner 和管理员能看到的行为。所有行为必须遵守 [01-design-principles.md](01-design-principles.md) 的边界。

## 普通聊天

当用户消息不涉及 Agent 控制面、已有 Agent session 或 run 查询时，Orchestrator 将请求路由到 Default Hermes Agent Profile，并把流式响应返回给 Open WebUI。

```text
用户 → Open WebUI → Orchestrator → Default Hermes Agent Profile → Orchestrator → Open WebUI
```

该链路不创建后台 run，不访问 Manager admin API，不暴露内部工具。

## 创建 Agent

用户可以自然语言表达创建 Agent 的意图。Orchestrator 只负责解析和提交受控请求，不直接创建 Agent。

示例：

```text
帮我启动一个常驻 Agent，监控 resource:team/project-alpha 的待处理事项。
```

Orchestrator 提交：

```json
{
  "request_type": "create_agent",
  "agent_type": "background_worker",
  "target_resource": "resource:team/project-alpha",
  "intent_text": "监控该资源的待处理事项，并在符合策略时执行后台处理",
  "constraints": {
    "trigger_mode": "scheduled_or_event",
    "allowed_actions": ["analyze", "prepare_change", "run_checks"],
    "require_approval_for_side_effects": true,
    "max_items_per_run": 8
  }
}
```

Manager 返回安全摘要：

```json
{
  "request_id": "req_001",
  "status": "approval_required",
  "message": "该请求需要资源负责人审批。"
}
```

## 已存在 Agent

如果请求命中已有 Agent：

```text
1. 配置一致：返回 existing agent 摘要。
2. 配置不一致但唯一性键一致：创建 change_request，不覆盖原 Agent。
3. existing agent 为 terminated：不复用，允许重新创建 request。
4. 用户无权访问：返回 not_found 或 forbidden 的安全摘要。
```

## 查询 Agent / Session / Run

Orchestrator 面向用户只返回摘要字段，禁止返回内部队列、credential、完整日志、完整 prompt 或完整 context。

```text
agent summary:
  agent_id
  agent_type
  display_name
  target_resource
  status
  allowed_actions
  active_session_count
  last_run_status
  last_run_at

session summary:
  session_id
  agent_id
  status
  parent_session_id
  depth
  created_at
  updated_at
  context_summary

run summary:
  run_id
  agent_id
  session_id
  trigger_type
  target_resource
  run_status
  risk_level
  result_summary
  created_at
  finished_at
```

## 长时间交互

当用户希望和已启动 Agent 持续交互时，Orchestrator 创建或恢复 `agent_session`，后续消息进入 Agent Runtime。

```text
Open WebUI conversation_id
  ↓ binding
agent_session_id
  ↓
Agent Runtime
  ↓
Memory / Session Store
```

Orchestrator 只保存轻量绑定，例如 `conversation_id → agent_session_id`，不保存完整上下文和 credential。

## Child Session

用户或 parent session 可以请求启动 child session，用于并行分析、专项审查或分工执行。

```text
Parent Agent Session
  ↓ child_session_request
Agent Manager
  ↓ policy / scope / depth / budget check
Child Agent Session
```

Manager 必须校验：

```text
parent session 存在且属于当前用户
child agent_type 被允许
child resource_scope 不超过 parent scope
parent agent 允许创建 child session
未超过 max_session_depth / max_child_sessions_per_parent / active child budget
是否需要审批
```

Child session 不继承完整上下文和 credential，只接收必要的 `context_summary`、资源引用和最小权限 credential。

## Observer Agent

系统必须内置 `observer_agent`。它持续评测平台运行，但只输出报告和建议。

Observer 可读取：

```text
agent / session / run 状态摘要
worker heartbeat 和 lease 摘要
resource lock 摘要
approval / audit 决策摘要
错误码、延迟、重试、超时和 dead-letter 统计
```

Observer 不可读取：

```text
明文 credential、token、私钥、.env
完整 prompt、完整 context、完整内部日志
用户无权访问的原始业务内容
```

Observer 输出：

```text
observer_report:
  report_id
  health_status
  risk_level
  summary
  findings
  recommendations
  evidence_refs
  created_at
```

`observer_report` 只对管理员或明确授权的 operator 可见。报告中的建议不会自动执行；需要改变系统状态时，必须转为 `agent_request` 或管理员操作，并重新经过 Manager 策略、审批和审计。

## 管理员链路

管理员通过 `agentctl` 调用 Manager admin API：

```bash
agentctl requests list
agentctl requests approve req_001
agentctl requests deny req_001
agentctl agents list
agentctl agents pause agent_001
agentctl agents resume agent_001
agentctl observer reports
agentctl audit tail
```

管理员 API 仍需经过服务身份、管理员身份、动作权限和审计记录。

## 错误码

第一版固定以下错误码：

```text
unauthorized       未登录或用户身份无效。
forbidden          用户存在，但缺少动作或资源权限。
approval_required  请求需要审批。
not_found          资源不存在，或用户无权知道该资源存在。
conflict           状态冲突、唯一性冲突、资源锁冲突或幂等冲突。
rate_limited       用户、session、resource 或 connector 达到限流。
internal_error     内部错误，只返回 trace_id。
```

安全错误摘要示例：

```json
{
  "error": "forbidden",
  "message": "你没有执行该操作的权限。",
  "trace_id": "trace_001"
}
```
