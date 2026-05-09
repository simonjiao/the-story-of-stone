# 设计原则

## 原则总表

| 编号 | 原则 | 含义 |
|---|---|---|
| P01 | Open WebUI 只连 Gateway | 前台不能看到 Manager、Runtime、Worker、Observer 或内部工具 |
| P02 | Manager 是唯一控制面 | 授权、策略、审批、生命周期、资源锁和审计只能由 Manager 决定 |
| P03 | Orchestrator 只路由 | Orchestrator 不执行任务、不重新规划、不复制目标上下文、不持有目标 credential |
| P04 | Runtime 只执行 | Runtime 执行已授权 session/run，不决定权限、不绕过 Manager |
| P05 | Worker 只推进 run | Worker claim、heartbeat、retry、timeout，不直接扩大权限或写外部资源 |
| P06 | Memory 不保存秘密 | Memory / Session Store 保存上下文摘要和引用，不保存明文 credential、token、私钥 |
| P07 | Agent 复用能力不复用状态 | `agent_instance` 可复用，session/run/credential/memory/workdir 必须隔离 |
| P08 | 长交互和单次执行分离 | 长对话用 `agent_session`，一次执行用 `agent_run` |
| P09 | Child session 受限 | v1 只允许一层 child session，且不继承完整 context 或 credential |
| P10 | Observer 只读 | Observer Agent 只生成评测和建议，不执行控制动作 |
| P11 | 副作用默认拒绝 | 外部写入、高风险动作必须审批或硬拒绝，并受资源锁保护 |
| P12 | 可观测性从 P0 开始 | trace_id、span、metrics 和 audit 关联必须先有，exporter 可以后启用 |

## Open WebUI 边界

Open WebUI 只配置 Agent Orchestrator / Gateway。以下信息不得出现在 Open WebUI 配置、工具列表、prompt 或可见响应中：

```text
Agent Manager API 地址
Agent Runtime 内部地址
Worker / Scheduler 内部入口
Observer 内部入口
内部工具 schema
service token
connector credential
队列、锁、审计原始记录
完整 prompt / 完整 context / 内部日志
```

Open WebUI 不配置以下 capability：

```text
create_agent
start_agent
pause_agent
resume_agent
delete_agent
grant_permission
view_audit
view_observer_report
raw_exec
direct_external_write
observer_control_action
```

这里的 `view_observer_report` 指 Open WebUI 直接配置 tool/capability 或直连 Manager 查询原始 report。授权 admin/operator 在 Open WebUI 中用自然语言请求系统状态时，只能进入 Orchestrator 的 System Observer status session 窄口；该窄口返回脱敏报告摘要和会话引用，不暴露 Manager API、Observer 内部入口或原始 snapshot。

用户可以通过自然语言表达创建、查询、继续会话或系统状态诊断等意图，但这些意图只能由 Orchestrator 转换成受控请求，再交给 Manager 判断。

## Orchestrator 边界

Orchestrator 是确定性 Gateway，不是执行 Agent。它只允许做：

```text
普通聊天路由
Agent intent 解析和 submit_agent_request
Open WebUI bridge binding 的验证、查询和关闭请求
agent_session message 转发
agent/run/session 摘要查询
System Observer status intent 路由
安全错误摘要返回
流式响应转发
用户级和 session 级限流
```

Orchestrator 明确不得做：

```text
执行任务
审批请求
修改 Agent 配置
持有目标 Agent credential
保存长期上下文
读取完整内部日志
调用未列明的 admin API
调用 Observer 控制动作
```

## Manager 边界

Agent Manager 默认拒绝所有请求。允许请求必须同时通过：

```text
服务身份校验
用户身份校验
动作权限校验
资源 allowlist 校验
Agent template / policy 校验
风险等级校验
审批校验
资源锁校验
审计记录
```

Manager 可以返回：

```text
allowed
denied
approval_required
conflict
not_found_or_forbidden
```

其中 `not_found_or_forbidden` 用于避免泄露用户无权知道的资源是否存在。

## Runtime 和 Worker 边界

Runtime 执行 Manager 已批准的 `agent_session` 或 `agent_run`，不自行决定权限。Worker 负责推进 run 状态，不直接调用外部写入接口，除非该 run 已获得 Manager 的 side effect 授权并持有有效 resource lock。

P0 代码基线已包含 Minimal Runtime 闭环和 Open WebUI Agent Identity Bridge。Bridge 是否可在某个环境宣告完成，必须同时看 hardening checklist、部署校验和回归证据。P1 在现有 Bridge/session/run 链路上接真实 Hermes Runtime，但只读。P2 才开放受控外部执行。

P1 可以提前落地 P2 所需的副作用 plan、credential scope、no-op provider、write connector contract 和 dry-run 审计，但这些 readiness 能力不得获取真实 credential、不得调用真实写 connector、不得把 run 推进到真实外部写入。

P0 必须先定义 RuntimeClient、MemoryStore、ConnectorClient、RunQueue 和 Telemetry facade。后续引入 gRPC、Redis、外部 memory provider 或 exporter 时，只能新增 adapter / feature，不能改写 Manager 授权模型、状态机和审计 contract。

## 可观测性边界

可观测性不是 P1/P2 才考虑的能力。P0 必须具备：

```text
trace_id 贯穿 request / session / run / audit / observer_report
关键状态迁移写 tracing span
稳定 metrics name 和 label
慢请求、锁等待、retry、dead_letter、Runtime timeout 的计数和耗时
```

OpenTelemetry / Prometheus exporter 可以按部署环境 feature-gated 启用；业务代码不得依赖具体 exporter。

## Observer 边界

Observer Agent 是 v1 必须内置的只读系统 Agent。它可以读取：

```text
agent/session/run 状态摘要
worker heartbeat 和 lease 摘要
resource lock 摘要
approval / audit 决策摘要
错误码、延迟、重试、超时、dead-letter 统计
```

Observer Agent 不得读取：

```text
明文 credential、token、私钥、.env
完整 prompt
完整 context
完整内部日志
用户无权访问的原始业务内容
```

Observer Agent 不得执行：

```text
approve / deny
pause / resume / delete
grant_permission
修改配置
扩大权限
外部写入
自动修复
```

Observer 的建议必须写入 `observer_report`。如果建议需要改变系统状态，必须转成普通 `agent_request` 或管理员操作，重新经过 Manager 策略、审批和审计。

## 高风险动作

以下动作默认硬拒绝：

```text
读取 secrets / .env / 私钥
直接写入受保护分支或生产资源
自动 merge / release / deploy
绕过保护规则
把 token 返回给模型
执行未授权 shell
让 Observer Agent 执行控制动作
```

以下动作默认需要审批：

```text
创建常驻 Agent
开启自动外部写入
修改 Agent 策略
扩大资源范围
提高扫描频率
允许修改受保护配置或自动化工作流
```

`merge / release / deploy` 是通用高风险生产副作用分类，不是 PR maintainer 专属规则。

## 隔离原则

```text
agent_instance = 可复用的能力配置
agent_session  = 隔离的长交互上下文
agent_run      = 隔离的一次执行
credential     = P0/P1 不向 Runtime 注入写权限 credential；P2 才允许 Manager 按 scope 临时注入最小权限 credential
memory         = 按 session_id 隔离
workdir        = 按 run_id/session_id 隔离
observer_report = 只读评测产物，不携带秘密
```
