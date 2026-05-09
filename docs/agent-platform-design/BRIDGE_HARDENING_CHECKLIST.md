# Agent Identity Bridge Hardening Checklist

本文只跟踪 Bridge 实现完善，不新增设计规则。权威规则仍以 [00-overview.md](00-overview.md)、[01-design-principles.md](01-design-principles.md)、[05-technical-implementation.md](05-technical-implementation.md) 为准。

## 完成判定

关键节点宣告完成前必须同时满足：

```text
1. 实现覆盖真实代码路径，而不只是文档声明。
2. 有针对性测试或可复现验证命令。
3. 自查是否仍存在绕过、重放、幂等、审计或部署缺口。
4. 本文件记录 summary，说明完成证据和残余风险。
```

## 节点清单

| 节点 | 状态 | 完成前反思 | Summary |
|---|---|---|---|
| B1 Orchestrator internal 权限收窄 | DONE | 不是只改文档；Orchestrator service token 和 dev header 都已从 `internal:*` 收窄到 `internal:open_webui_bridge:*`。仍需依赖 Manager 侧 action 常量保持 namespace 隔离。 | 已补代码和测试：`dev_manager_headers_only_allow_bridge_internal_namespace`。 |
| B2 Bridge nonce replay 防护 | DONE | HMAC + clock skew 不足以防窗口内重放；mutating Bridge 请求现在必须先在 Manager/Store claim nonce。残余风险：claim 成功后如果下游网络失败，同一请求重试会被当成 replay，需要用户重新发起。 | 已新增 `open_webui_bridge_nonces` migration、Manager nonce endpoint、Memory/Pg store 实现和 replay 测试。 |
| B3 Open WebUI message append 去重 | DONE | run idempotency 不等于 message append 幂等；现在 `message_id` 映射为 `external_message_id`，同一 session 内重复 append 返回既有 message。残余风险：Open WebUI 不提供稳定 `message_id` 时只能退回 nonce 级保护。 | 已新增 `agent_session_messages.external_message_id`、唯一索引、append 去重和测试。 |
| B4 Bridge lifecycle 审计 | DONE | run/message audit 不能替代 binding 生命周期审计；现在 binding upsert、run update、close 都写 audit，且 closed binding 不能继续 update run。 | 已补 Manager audit 记录、active binding guard 和 lifecycle audit 测试。 |
| B5 部署和完成口径 | CODE_READY | 已补 Function verify script 和文档口径，但当前分支尚未对正式环境执行部署复测，因此只能宣告 code-ready，不能宣告目标环境完整完成。 | 已新增 `deploy/scripts/verify-openwebui-function.sh`，并在 README/roadmap 中要求部署后校验 Function、valves、JWT/dev headers 和 Bridge 回归。 |

## 验证记录

已执行：

```text
cargo test --manifest-path agent-platform/Cargo.toml
bash -n deploy/scripts/verify-openwebui-function.sh
```

待部署复测：

```text
1. deploy/scripts/install-openwebui-function.sh 或 DB installer 后执行 verify-openwebui-function.sh。
2. 正式环境确认 AGENT_PLATFORM_ALLOW_DEV_HEADERS=false。
3. Open WebUI 登录、模型选择、基础聊天、会话保存。
4. Bridge 控制请求、binding 持久化、后续消息 run、关闭 session。
5. 第二个 Open WebUI 登录用户隔离。
6. Orchestrator 重启后 binding 从 Manager/Postgres 复用。
```
