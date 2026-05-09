# ISSUE-009: Agent Identity Bridge hardening 需要部署复测

## 状态

RESOLVED

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `BRIDGE-HARDEN-CODE-20260509` | PASS | 本地代码已补 nonce replay、message append 去重、binding lifecycle audit 和 internal action 收窄。 |
| `BRIDGE-HARDEN-DEPLOY-20260509` | PASS | 远端正式镜像已重建并重启，Manager/Orchestrator healthy，Worker/Observer running。 |
| `BRIDGE-HARDEN-FUNCTION-20260509` | PASS | `verify-openwebui-function.sh` 在无 Admin API token 的正式环境走 compose DB fallback，确认 Function 为 active/global filter 且 valves key 完整。 |
| `BRIDGE-HARDEN-SMOKE-20260509` | PASS | 合成 Open WebUI subject/chat 覆盖审批建链、后续消息 run、同 message_id 去重、nonce replay 冲突和关闭 session。 |
| `BRIDGE-HARDEN-ISOLATION-20260509` | PASS | 相同 chat/model 下不同签名 subject 不复用 binding，未向原 subject session append message。 |
| `BRIDGE-HARDEN-RESTART-20260509` | PASS | Orchestrator 重启后仍从 Manager/Postgres 复用 active binding，并能继续创建 Worker run。 |
| `BRIDGE-REAL-ACCOUNT-20260509` | PASS | 使用正式 Open WebUI 真实 admin/user 账号，经 `/api/chat/completions` 触发 Function 和 Bridge，验证独立 binding、cross-chat 隔离、follow-up、dedupe 和 close。 |

## 背景

ISSUE-008 解决了 Open WebUI 用户身份和 chat/session binding 的 smoke baseline，但后续复盘发现“已完成”口径偏早：代码和报告仍存在重放防护、message append 幂等、binding lifecycle audit、internal 权限收窄和部署校验 gate 的缺口。

## 修复

```text
1. Orchestrator Manager service token 从 `internal:*` 收窄为 `internal:open_webui_bridge:*`。
2. Manager 新增 Bridge nonce claim endpoint；Store 新增 `open_webui_bridge_nonces`。
3. session message 新增 `external_message_id`，同一 session 内重复 Open WebUI message 不重复 append。
4. Bridge binding upsert、run update、close 写 audit；closed binding 不允许继续 update run。
5. 新增 `deploy/scripts/verify-openwebui-function.sh`，部署后校验 Function type、active/global、bridge content 和 valve key。
6. `verify-openwebui-function.sh` 增加 compose DB fallback；正式部署没有 `OPEN_WEBUI_ADMIN_TOKEN` 时仍可校验已安装 Function，且只输出 valve key names。
```

## 验证

```text
cargo test --manifest-path agent-platform/Cargo.toml
bash -n deploy/scripts/verify-openwebui-function.sh
python3 -m unittest deploy/open-webui/functions/test_agent_identity_bridge_filter.py
```

正式环境复测：

```text
1. `agent_identity_bridge` Function：source=compose-db、type=filter、active/global=true、valve keys 完整。
2. `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false`。
3. Postgres migration：nonce table、external_message_id column、dedupe unique index 存在。
4. Bridge 控制请求：合成 subject 创建 request，审批后 binding active。
5. 后续消息：Worker run completed，同 message_id/new nonce 不重复 append。
6. Replay：同 nonce 第二次请求返回 `conflict`。
7. 隔离：相同 chat/model 的第二个 signed subject 不复用第一个 subject 的 binding。
8. 重启：Orchestrator 重启后从 Manager/Postgres 复用 active binding。
9. 清理：合成 binding 关闭后状态为 `closed`。
10. 日志：复测窗口内 Manager/Orchestrator/Worker/Observer 无错误关键词。
11. 真实账号：Open WebUI admin/user 真实账号均通过 Open WebUI auth；admin 不自动获得 Agent Platform admin；两个真实账号拥有独立 binding/session；admin 使用 user 的测试 chat id 不复用 user binding；重复 message id 不重复 append；测试 chat 已删除。
```

## 完成口径

已满足：不是仅凭代码合并关闭，正式环境部署复测证据已写回 `docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md`。

残余边界：本轮没有通过浏览器手动输入密码登录；已使用正式 Open WebUI auth 代表真实 admin/user 账号调用真实聊天 API。
