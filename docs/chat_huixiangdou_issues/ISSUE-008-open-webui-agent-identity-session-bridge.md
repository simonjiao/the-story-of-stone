# ISSUE-008: Open WebUI 未透传 Agent Platform 用户身份和 agent session 元数据

## 状态

RESOLVED

## 等级

P2

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `AGENT-UI-CTRL-20260508` | PASS | UI 控制类请求能进入 Agent Manager。 |
| `AGENT-UI-AUDIT-20260508` | PASS_WITH_GAP | UI 请求可审计，但请求人落为默认 `dev-user`。 |
| `AGENT-MULTI-SESSION-20260508` | PASS_API_ONLY | Manager/Worker API 路径支持多 session，但 Open WebUI 默认聊天未自动绑定 agent session。 |
| `BRIDGE-AUTH-20260508` | PASS | Agent Identity Bridge 部署后，UI 控制请求按 `openwebui:<id>` 进入 Manager。 |
| `BRIDGE-RUN-20260508` | PASS | 同一 Open WebUI chat 后续消息自动复用 agent session 并创建 Worker run。 |
| `BRIDGE-MULTI-SESSION-20260508` | PASS | 同一 agent 可被同一用户不同 Open WebUI chat 复用为不同 session。 |

## 影响

Open WebUI 旧路径只作为 OpenAI-compatible 聊天窗口调用 Orchestrator，没有把登录用户身份、Open WebUI conversation id 或 Agent Platform `agent_session_id` 作为受信元数据传给 Manager。

因此 UI 触发的 Agent Platform 控制请求曾落到默认 dev 身份，审计中的 `requested_by_user` 是 `dev-user`，不是 Open WebUI 登录账号。Manager/Worker 后端已支持同一 agent 多 session 和 run 执行；本次修复补齐了 Open WebUI 默认聊天到 Agent Platform session 的正式桥接。

## 证据

- UI 触发请求 `req_019e071a9a2572c08488577cc52d77d6` 的 `requested_by_user` 为 `dev-user`，`requested_by_service` 为 `dev-orchestrator`。
- UI 触发请求 `req_019e072a5bc87352b4ca99c26664157f` 同样落为 `dev-user`。
- Orchestrator 只会转发已有的 `x-agent-user`、`x-agent-user-token`、`x-agent-service` 等 header；Open WebUI 默认 OpenAI provider 调用没有提供动态用户 header。
- Orchestrator 的 conversation-to-session bind 是轻量绑定入口，默认 UI 聊天不会自动调用该绑定，也不会自动携带 `agent_session_id` metadata。
- 修复后请求 `req_019e0796c6f77c52ae779adc787de54f` 的 `requested_by_user` 为 `openwebui:7fe86b5c-4248-46ac-a8bc-bb716e1ca102`，`requested_by_service` 为 `agent-orchestrator`。
- 修复后 binding `3c77dba8-6890-44d2-8f97-39e08cb723b9 -> sess_019e07979ebc7200b06652ccd3716d73` 持久化在 `open_webui_bridge_bindings`。
- 修复后后续消息创建并完成 `run_019e07981bc17b02ae64ff45e243f67a`，结果摘要包含 `with 1 recent messages`。

## 当前判断

这不是 Worker 执行问题，也不是 Open WebUI 普通聊天问题，而是 Open WebUI 到 Agent Platform 控制面的身份/session 桥接缺口。

已通过独立 Agent Identity Bridge 修复：Open WebUI Filter 注入签名 `agent_bridge_context`，Orchestrator 验签后签发 Manager JWT，Manager 持久化 Open WebUI chat 到 Agent Platform `agent_session` 的 binding。正式部署中 `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false`，直接 dev header 请求返回 HTTP 401。

管理账号边界：Open WebUI admin 只用于安装/更新 Function 和 Valves；运行时审批、审计和管理能力仍由 Agent Platform JWT/role 决定。Open WebUI admin 默认不会映射为 Agent Platform admin。

实现边界：Bridge 的 active binding 存储在 Manager/Postgres，Orchestrator 不再保留正式 session binding 内存入口。Open WebUI `message_id`/`user_message_id` 当前用于 run 幂等；session message append 去重需要后续 schema 扩展。

## 后续动作

已完成。稳定设计已合并到 `docs/agent-platform-design/`，后续以 Agent Platform 设计文档为准，不再维护独立 Bridge 计划/清单文档。常规回归仍包括 Open WebUI Function 管理接口可用性、Function secret 轮换、历史搜索和停止按钮可访问性；相关问题分别由其他 issue 跟踪。
