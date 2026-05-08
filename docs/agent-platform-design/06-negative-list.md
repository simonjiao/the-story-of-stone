# 负面清单与延期项

本文件定义不能做、不建议做和暂不实现的内容。若功能诉求与本文件冲突，以本文件为准，除非先更新设计原则并重新通过场景验证。

## 明确禁止

以下 capability 不得暴露给 Open WebUI：

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

以下调用方向禁止：

```text
Open WebUI → Agent Manager
Open WebUI → Agent Runtime
Open WebUI → Worker / Scheduler
Open WebUI → Observer Agent
Orchestrator → Manager admin API
Orchestrator → Observer admin report API
Runtime → Manager 授权绕过接口
Worker → 外部写入接口（未持有授权和锁时）
Observer Agent → 任何控制或写入 API
```

以下动作默认硬拒绝：

```text
读取 secrets / .env / 私钥
直接写入受保护分支或生产资源
自动 merge / release / deploy
绕过保护规则
把 token 返回给模型
执行未授权 shell
让 Agent instance 持有长期 credential
让 child session 继承完整 context 或 credential
让 Observer Agent 审批、暂停、恢复、删除、授权或修改配置
```

## 不建议

```text
1. 不建议让 Default Agent 作为固定中间层。
2. 不建议让 Orchestrator 执行任务或保存长期上下文。
3. 不建议把完整 prompt、完整 context、内部日志返回给用户。
4. 不建议让 child session 自动合并完整上下文回 parent session。
5. 不建议把 Observer Agent 设计成超级管理员或自动修复执行体。
6. 不建议在 P0 引入 Redis 作为正确性依赖；P0 用 Postgres lease 先闭环。
7. 不建议让 ORM、GraphQL、gRPC 或外部 memory provider 决定 P0 架构；P0 应先固定 trait / adapter 边界。
8. 不建议跳过 P0 可观测性埋点；trace_id、tracing span、metrics name/label 必须从 P0 开始。
9. 不建议把 P2 当作重构阶段；P1 应先固定 side-effect plan、credential lease、write connector contract 和 no-op adapter，P2 只启用真实 provider / connector。
```

## v1 暂不实现

| 延期项 | 延期原因 | 后续触发条件 |
|---|---|---|
| Agent Manager UI | P0 已由 `agentctl CLI` 覆盖审批、暂停、查询和审计；UI 会增加权限和展示复杂度 | CLI 流程稳定且管理员操作频率证明需要 UI |
| Open WebUI Manager 页面 | Open WebUI 必须保持不知道控制面；前台管理页会破坏 Gateway 边界 | 明确需要普通用户自助管理，且能通过 Gateway 安全代理 |
| 多租户复杂计费 | 当前目标是单平台控制面、安全边界和运行闭环 | 出现多个组织或商业计费需求 |
| 跨组织 / 跨系统复杂授权 | 第一版只做 resource allowlist 和基础 RBAC / ABAC | 单组织 / 单系统授权模型稳定后 |
| 自动 merge / release / deploy | 通用高风险生产副作用，第一版默认审批或硬拒绝 | 具备完整审批、回滚、环境隔离和发布审计 |
| 动态无限 Agent 创建 | 任意 template、任意权限或无限数量会造成资源耗尽和权限扩散 | 配额、唯一性、预算、回收机制稳定后 |
| 外部 memory provider 深度集成 | 会引入一致性、隐私和召回质量问题 | 内部 Memory / Session Store 模型稳定后 |
| 多级 child session | 多级嵌套会造成权限链、预算、上下文和审计链失控 | 一层 child session 的回传、预算和审计模型稳定后 |
| Observer 自动修复 | 会把只读观察体变成控制面绕过路径 | 另行设计受控 auto-remediation，并仍经过 Manager 审批 |
| Runtime 写权限 credential | P1 只读，P2 才能受控注入最小权限 credential | P2 策略、审批、资源锁和审计稳定后 |
| 默认启用 OpenTelemetry exporter | exporter 依赖部署环境、collector、采样策略和成本控制；但 P0 必须保留 tracing/metrics 埋点和 feature gate | 部署环境具备 collector，并需要跨服务 trace 时启用 |

## 延期原则

```text
1. 会破坏安全边界的能力延后。
2. 会放大权限、资源或审计复杂度的能力延后。
3. 需要真实运行数据才能合理设计的策略延后。
4. 可以用 CLI、report 或人工审批覆盖的能力先不做 UI 自动化。
5. v1 保留必要字段和状态，但不承诺复杂行为。
```
