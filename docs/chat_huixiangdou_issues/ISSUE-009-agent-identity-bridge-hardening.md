# ISSUE-009: Agent Identity Bridge hardening 需要部署复测

## 状态

READY_FOR_DEPLOY

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `BRIDGE-HARDEN-CODE-20260509` | PASS | 本地代码已补 nonce replay、message append 去重、binding lifecycle audit 和 internal action 收窄。 |
| `BRIDGE-HARDEN-DEPLOY-20260509` | TODO | 待正式环境部署后复测 Function、valves、JWT/dev headers、binding/run/close、多用户隔离和重启后复用。 |

## 背景

ISSUE-008 解决了 Open WebUI 用户身份和 chat/session binding 的 smoke baseline，但后续复盘发现“已完成”口径偏早：代码和报告仍存在重放防护、message append 幂等、binding lifecycle audit、internal 权限收窄和部署校验 gate 的缺口。

## 本地修复

```text
1. Orchestrator Manager service token 从 `internal:*` 收窄为 `internal:open_webui_bridge:*`。
2. Manager 新增 Bridge nonce claim endpoint；Store 新增 `open_webui_bridge_nonces`。
3. session message 新增 `external_message_id`，同一 session 内重复 Open WebUI message 不重复 append。
4. Bridge binding upsert、run update、close 写 audit；closed binding 不允许继续 update run。
5. 新增 `deploy/scripts/verify-openwebui-function.sh`，部署后校验 Function type、active/global、bridge content 和 valve key。
```

## 验证

```text
cargo test --manifest-path agent-platform/Cargo.toml
bash -n deploy/scripts/verify-openwebui-function.sh
```

## 待部署复测

```text
1. 安装或更新正式 Open WebUI `agent_identity_bridge` Function 后执行 verify script。
2. 确认 `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false`。
3. 用正式 Open WebUI 复测登录、模型选择、基础聊天、会话保存。
4. 复测 Bridge 控制请求、binding 持久化、后续消息 run、关闭 session。
5. 复测第二个 Open WebUI 登录用户隔离。
6. 复测 Orchestrator 重启后 binding 从 Manager/Postgres 复用。
```

## 完成口径

本 issue 不能仅凭代码合并关闭。必须完成正式环境部署复测，并把证据写回测试报告或本 issue 后，才能改为 `RESOLVED`。
