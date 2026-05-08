# ISSUE-008: Open WebUI 未透传 P0 用户身份和 agent session 元数据

## 状态

OPEN

## 等级

P2

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `P0-UI-CTRL-20260508` | PASS | UI 控制类请求能进入 P0 Manager。 |
| `P0-UI-AUDIT-20260508` | PASS_WITH_GAP | UI 请求可审计，但请求人落为默认 `dev-user`。 |
| `P0-MULTI-SESSION-20260508` | PASS_API_ONLY | Manager/Worker API 路径支持多 session，但 Open WebUI 默认聊天未自动绑定 P0 session。 |

## 影响

Open WebUI 当前只作为 OpenAI-compatible 聊天窗口调用 Orchestrator，没有把登录用户身份、Open WebUI conversation id 或 P0 `agent_session_id` 作为受信元数据传给 Manager。

因此 UI 触发的 P0 控制请求目前落到默认 dev 身份，审计中的 `requested_by_user` 是 `dev-user`，不是 Open WebUI 登录账号。Manager/Worker 后端已经支持同一 agent 多 session 和 run 执行，但从 Open WebUI 默认聊天界面还不能自动建立真实 P0 agent session 绑定。

## 证据

- UI 触发请求 `req_019e071a9a2572c08488577cc52d77d6` 的 `requested_by_user` 为 `dev-user`，`requested_by_service` 为 `dev-orchestrator`。
- UI 触发请求 `req_019e072a5bc87352b4ca99c26664157f` 同样落为 `dev-user`。
- Orchestrator 只会转发已有的 `x-agent-user`、`x-agent-user-token`、`x-agent-service` 等 header；Open WebUI 默认 OpenAI provider 调用没有提供动态用户 header。
- Orchestrator 的 conversation-to-session bind 是轻量绑定入口，默认 UI 聊天不会自动调用该绑定，也不会自动携带 `agent_session_id` metadata。

## 当前判断

这不是 Worker 执行问题，也不是 Open WebUI 普通聊天问题，而是 Open WebUI 到 P0 控制面的身份/session 桥接缺口。

静态配置最多只能给 provider 设置固定 header，不能把每个 Open WebUI 登录用户动态映射为 P0 用户，也不能自动把每个 Open WebUI conversation 映射为 P0 `agent_session_id`。正式多用户方案需要显式桥接层，例如 Open WebUI 插件/中间代理/JWT 用户 token 注入。

## 后续动作

建议作为 P1/P2 边界决策：如果部署只服务单用户，可暂时接受默认 P0 用户；如果要支持正式多用户或 UI 级 agent session，需要实现动态身份和 session 元数据桥接后再复测。
