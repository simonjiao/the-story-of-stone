# 产品外部定义

本文件定义 Open WebUI、普通用户、resource owner 和管理员能看到的行为。内部控制规则见 [04-internal-definition.md](04-internal-definition.md)，禁止项见 [06-negative-list.md](06-negative-list.md)。

## 普通聊天

普通聊天不涉及 Agent 控制面、已有 Agent session 或 run 查询时，Orchestrator 将请求路由到 Default Hermes Agent Profile，并把响应返回给 Open WebUI。

```text
用户 → Open WebUI → Orchestrator → Default Hermes Agent Profile → Orchestrator → Open WebUI
```

该链路不创建后台 run，不访问 Manager admin API，不暴露内部工具。

## Open WebUI Agent Identity Bridge

Open WebUI 通过全局 `agent_identity_bridge` Filter 为 `hermes-agent` 请求注入签名上下文。该上下文只用于让 Orchestrator 识别当前 Open WebUI user、chat、model 和 message，并不让 Open WebUI 直接访问 Manager。

外部可见行为：

```text
1. 普通聊天继续透传到默认 Hermes；Orchestrator 会移除内部 bridge 字段。
2. 明确 Agent 控制请求必须带有效 bridge context，否则 fail closed。
3. 控制请求 fulfilled 后，同一 Open WebUI chat 会绑定到一个 Agent Platform agent_session。
4. 同一用户同一 chat 后续消息复用该 session，并自动创建 read-only run。
5. 不同 Open WebUI chat 即使复用同一 agent，也使用不同 session。
6. 关闭当前 agent session 后，该 chat 恢复默认 Hermes passthrough。
```

权限边界：

```text
1. Open WebUI admin 只管理 Open WebUI Function 和 Valves，不默认映射为 Agent Platform admin。
2. Agent Platform 权限仍由 Manager 的 JWT、role、resource allowlist 和 policy 决定。
3. `agent_bridge_context` 不包含 secret、signature 以外的敏感字段、email 原文或完整业务 payload。
4. Bridge secret 只进入 `.env` 或 Open WebUI Function Valves，不进入文档示例值、prompt、日志或用户响应。
```

## 创建 Agent

用户可以用自然语言请求创建 Agent。Orchestrator 只解析意图并提交 `agent_request`，不直接创建 Agent。

示例：

```text
帮我启动一个常驻 Agent，监控 resource:team/project-alpha 的待处理事项。
```

Orchestrator 提交的请求必须包含：

```text
request_type
agent_type
target_resource
intent_text
constraints
idempotency_key
```

Manager 只返回安全摘要，例如 `fulfilled`、`approval_required`、`denied`、`conflict` 或 `not_found_or_forbidden`。

## 已存在 Agent

```text
1. 配置一致：返回 existing agent 摘要。
2. 配置不一致但唯一性键一致：创建 change_request，不覆盖原 Agent。
3. existing agent 为 terminated：不复用，允许重新创建 request。
4. 用户无权访问：返回安全错误摘要，不泄露资源是否存在。
```

## 查询 Agent / Session / Run

Orchestrator 面向用户只返回摘要字段，禁止返回内部队列、credential、完整日志、完整 prompt 或完整 context。

```text
agent summary:
  agent_id / agent_type / display_name / target_resource / status
  allowed_actions / active_session_count / last_run_status / last_run_at

session summary:
  session_id / agent_id / status / parent_session_id / depth
  created_at / updated_at / context_summary

run summary:
  run_id / agent_id / session_id / trigger_type / target_resource
  run_status / risk_level / result_summary / created_at / finished_at
```

## 长时间交互

当用户希望和已启动 Agent 持续交互时，Orchestrator 通过 Manager 创建或恢复 `agent_session`，后续消息追加到 session，并创建只读 run。

```text
Open WebUI conversation_id
  → Manager bridge/session binding
  → agent_session message + read-only run
  → Worker / Agent Runtime
  → Memory / Session Store
```

正式 Open WebUI 部署中，Open WebUI chat 到 `agent_session` 的持久 binding 由 Manager 保存。Orchestrator 只路由和转发安全摘要，不保存完整上下文或 credential。

## Child Session

用户或 parent session 可以请求启动 child session，用于并行分析、专项审查或分工执行。

Manager 必须校验 parent session、child agent_type、resource_scope、depth、child 数量预算和审批要求。

Child session 不继承完整上下文或 credential。P0/P1 只接收必要的 `context_summary` 和资源引用；P2 如需 credential，必须由 Manager 按 scope 临时注入最小权限引用，且不得进入 prompt、summary 或 `observer_report`。

## Observer Report

系统内置 `observer_agent`。它持续评测平台运行，只输出 `observer_report`，不直接改变系统状态。

用户可见边界：

```text
1. 普通 Open WebUI 用户看不到 observer_report 入口。
2. 管理员或明确授权的 operator 可以通过 agentctl / admin API 查看 report 列表和详情。
3. report 只包含 health_status、risk_level、summary、findings、recommendations、evidence_refs、created_at。
4. report 不包含完整 snapshot、完整 prompt、完整 context、内部日志、credential、token、私钥或 `.env`。
5. report 建议不会自动执行；需要改变状态时必须转成 agent_request 或管理员操作。
```

## Observer Report Discussion

P1 支持围绕 `observer_report` 发起受控讨论，但对话由目标 Agent 承载，Observer 不变成可对话控制面。

```text
1. 管理员或授权 operator 先获取 observer_report。
2. 用户选择 report_id 和目标 agent_id。
3. Manager 创建普通 agent_session，并只注入脱敏 report 摘要、evidence_refs、snapshot_ref 和用户有权查看的摘要。
4. 后续对话由目标 Agent Runtime 承载。
5. 对话只能解释报告、归因、形成需求草案或建议创建 request。
6. 任何状态变更仍必须重新经过 Manager 策略、审批和审计。
```

禁止：

```text
Open WebUI 直接查询 observer_report 或 Manager admin API。
Observer Agent 直接参与对话、执行修复或创建高权限动作。
将完整 snapshot、完整 prompt、完整 context、内部日志、credential 或 token 注入 discussion session。
```

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
agentctl observer show obsr_001
agentctl observer discuss obsr_001 --agent-id agent_001
agentctl audit tail
```

管理员 API 仍需经过服务身份、管理员身份、动作权限和审计记录。

## 错误码

```text
unauthorized       未登录或用户身份无效。
forbidden          用户存在，但缺少动作或资源权限。
approval_required  请求需要审批。
not_found          资源不存在，或用户无权知道该资源存在。
conflict           状态冲突、唯一性冲突、资源锁冲突或幂等冲突。
rate_limited       用户、session、resource 或 connector 达到限流。
internal_error     内部错误，只返回 trace_id。
```
