# Global Router 设计

进展记录见 `PROGRESS.md`。

## 定位

`global-router` 是独立的 OpenAI-compatible 路由层，不从属于“通灵玉”
Gateway、Agent Platform 控制面或其它具体业务项目。

它的职责是把 Open WebUI 可见的 model id 映射到后端 OpenAI-compatible
gateway，并在统一入口处执行模型 allowlist、基础请求转发和必要的路由级
边界控制。

## 当前 MVP 范围

当前完成的是 MVP 路由层，不是完整生产级 router。

已完成：

1. OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
2. 模型列表从 `GLOBAL_ROUTER_ROUTES_JSON` 显式 allowlist 暴露。
3. 按 Open WebUI 可见 model 路由到后端 `base_url`。
4. 支持 `other/default` 这类 namespaced model id，避免多个后端模型重名。
5. 支持把可见模型重写为后端 `upstream_model`，例如
   `model=other/default` 转发为 `model=default`。
6. 每条 route 支持 `timeout_seconds`。
7. 基础错误归一化：缺 model、未 allowlist、缺 `agent_bridge_context`、
   转发失败。
8. 基础鉴权透传：配置 `api_key_env` 时给后端注入 Bearer token；否则透传
   入站 `Authorization`。
9. 基础 streaming 透传结构：以 bytes stream 透传上游响应。

## 尚未完成

以下能力不属于当前 MVP 完成范围：

1. Router 自己的入站鉴权。
2. Route-level 用户权限和 RBAC。
3. Router 内校验 `agent_bridge_context` 的 HMAC 签名。
4. 持久化审计记录。
5. 对上游错误做完整统一归一化。
6. 自动向多个 gateway 拉取 `/v1/models` 并聚合模型列表。
7. 远端 streaming smoke。
8. 管理面热更新 route。
9. Route 健康状态汇总。
10. 熔断、降级和 fallback 策略。

`requires_bridge=true` 当前只检查 `agent_bridge_context` 是否存在，不代表
router 已完成身份签名校验。

## 配置契约

`GLOBAL_ROUTER_ROUTES_JSON` 是 JSON 数组。每个 route 描述 Open WebUI
可见模型、后端地址和转发策略：

```json
[
  {
    "model": "default",
    "name": "Default",
    "base_url": "http://default-gateway:8090/v1",
    "upstream_model": "default",
    "requires_bridge": false,
    "timeout_seconds": 120
  },
  {
    "model": "other/default",
    "name": "Other Gateway",
    "base_url": "http://other-gateway:8090/v1",
    "upstream_model": "default",
    "requires_bridge": true,
    "api_key_env": "OTHER_GATEWAY_API_KEY",
    "timeout_seconds": 120
  }
]
```

字段含义：

| 字段 | 含义 |
|---|---|
| `model` | Open WebUI 可见 model id，必须全局唯一 |
| `name` | `/v1/models` 返回的展示名 |
| `base_url` | 后端 OpenAI-compatible `/v1` 基地址 |
| `upstream_model` | 转发给后端的真实 model id；缺省时等于 `model` |
| `requires_bridge` | 是否要求请求体包含 `agent_bridge_context` |
| `api_key_env` | 可选；从环境变量读取后端 Bearer token |
| `timeout_seconds` | 可选；单次转发超时，缺省 120 秒 |

## 边界

`global-router` 不处理具体业务 RAG、证据包、reviewer、Agent 控制面状态机或
业务知识库。具体业务 gateway 负责业务工作流，Agent Platform 负责控制面和
执行面。当前生产部署暂不使用 `global-router`；Open WebUI 直接连接
`tonglingyu-gateway` 和 `agent-orchestrator`。

## 验收口径

MVP 验收只证明：

1. Open WebUI 只能看到 allowlist 模型。
2. 已知模型能转发到对应后端。
3. 未 allowlist 模型会被拒绝。
4. 可见模型名能按需重写为后端模型名。
5. route timeout、基础鉴权透传和基础错误归一化生效。

生产级验收必须另行覆盖入站鉴权、RBAC、HMAC、审计、聚合、健康、熔断和
streaming 远端 smoke。
