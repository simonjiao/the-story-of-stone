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
| B5 部署和完成口径 | DONE | 不只停留在 code-ready；已在正式远端环境重建镜像、重启服务并执行 hardening 复测。追加真实账号复测后，admin/user 两个 Open WebUI 真实账号均通过真实聊天 API 覆盖 Bridge 隔离。残余风险：未通过浏览器手动输入密码登录，本轮使用正式 Open WebUI auth 代表真实账号。 | `verify-openwebui-function.sh` 支持 Admin API 和 compose DB fallback；正式环境复测通过 Function、migration、dedup、replay、subject isolation、Orchestrator restart reuse、真实账号 Bridge 回归和日志检查。 |

## 验证记录

已执行：

```text
cargo test --manifest-path agent-platform/Cargo.toml
bash -n deploy/scripts/verify-openwebui-function.sh
python3 -m unittest deploy/open-webui/functions/test_agent_identity_bridge_filter.py
```

正式环境复测已执行：

```text
1. 远端正式镜像 `hermes-agent-platform:formal` 重建并重启 Manager/Orchestrator/Worker/Observer。
2. `verify-openwebui-function.sh` 在正式环境走 `source=compose-db`，Function 为 active/global filter，valve keys 完整。
3. 正式 Postgres 存在 `open_webui_bridge_nonces`、`agent_session_messages.external_message_id` 和去重唯一索引。
4. 合成 Open WebUI subject/chat 覆盖审批建链、follow-up run、同 message_id 去重、nonce replay 冲突和关闭 session。
5. 相同 chat/model 下不同 signed subject 不复用对方 binding。
6. Orchestrator 重启后，从 Manager/Postgres 找回 active binding 并继续创建 Worker run。
7. 正式 Open WebUI 真实 admin/user 账号经 `/api/chat/completions` 触发 Function 和 Bridge，验证独立 binding/session、cross-chat 隔离、follow-up、dedupe 和 close。
8. 复测窗口内 Open WebUI/Manager/Orchestrator/Worker/Observer 无错误关键词。
```
