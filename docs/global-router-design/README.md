# Global Router 设计

进展记录见 `PROGRESS.md`。

## 定位

`global-router` 是独立的 OpenAI-compatible 路由层，不从属于“通灵玉”
Gateway、Agent Platform 控制面或其它具体业务项目。

它的职责是把 Open WebUI 可见的 model id 映射到后端 OpenAI-compatible
gateway，并在统一入口处执行模型 allowlist、基础请求转发和必要的路由级
边界控制。

## 当前生产化第一阶段基线

这是受控试运行基线，不是真正完整 production-grade router 标准。

已完成：

1. OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
2. 模型列表从 `GLOBAL_ROUTER_ROUTES_JSON` 或 `GLOBAL_ROUTER_ROUTES_FILE`
   显式 allowlist 暴露。
3. 按 Open WebUI 可见 model 路由到后端 `base_url`。
4. 支持 `other/default` 这类 namespaced model id，避免多个后端模型重名。
5. 支持把可见模型重写为后端 `upstream_model`，例如
   `model=other/default` 转发为 `model=default`。
6. 每条 route 支持 `timeout_seconds`。
7. Router 自己的入站 Bearer 鉴权。
8. Route-level 用户角色和 subject allowlist。
9. Router 内校验 `agent_bridge_context` 的 HMAC 签名、issuer、model、
   timestamp、nonce、subject 和 chat 边界；通过校验后剥离该上下文再转发。
10. Bridge nonce 在进程内防重放。
11. JSONL 持久化审计记录。
12. Router 自己错误和上游非 2xx 错误归一化。
13. 自动向多个 gateway 拉取 `/v1/models` 并按 namespace 聚合模型列表。
14. 管理面 route reload、route 配置查看和 route 健康状态汇总。
15. Route 熔断和 fallback 策略；fallback 不会绕过目标 route 的权限或打到
   已熔断 route。
16. 基础鉴权透传：配置 `api_key_env` 时给后端注入 Bearer token；否则透传
   入站 `Authorization`。
17. Streaming 透传结构：以 bytes stream 透传上游成功响应。

## 尚未完成

以下能力不属于当前第一阶段基线完成范围：

1. 远端 streaming smoke。
2. 多实例共享熔断状态和 Bridge nonce 防重放状态。
3. 审计落库或外部审计 sink。
4. Admin API 的独立 JWT/RBAC；当前是单一 admin Bearer token。

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
    "timeout_seconds": 120,
    "failure_threshold": 3,
    "circuit_breaker_seconds": 30
  },
  {
    "model": "other/default",
    "name": "Other Gateway",
    "base_url": "http://other-gateway:8090/v1",
    "upstream_model": "default",
    "requires_bridge": true,
    "api_key_env": "OTHER_GATEWAY_API_KEY",
    "timeout_seconds": 120,
    "allowed_user_roles": ["admin"],
    "failure_threshold": 3,
    "circuit_breaker_seconds": 30,
    "fallback_model": "default"
  },
  {
    "model": "other",
    "name": "Other Gateway",
    "base_url": "http://other-gateway:8090/v1",
    "requires_bridge": true,
    "api_key_env": "OTHER_GATEWAY_API_KEY",
    "discover_models": true,
    "allowed_user_roles": ["admin"]
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
| `discover_models` | 可选；是否从后端 `/models` 聚合模型并挂到 `model/` namespace 下 |
| `allowed_user_roles` | 可选；Bridge 验签后的用户角色 allowlist |
| `allowed_subjects` | 可选；Bridge 验签后的 Open WebUI subject allowlist |
| `failure_threshold` | 可选；连续失败多少次后打开 route circuit，缺省 3 |
| `circuit_breaker_seconds` | 可选；circuit 打开时长，缺省 30 |
| `fallback_model` | 可选；primary route 熔断或 5xx/转发失败后的可见 fallback model |

## 边界

`global-router` 不处理具体业务 RAG、证据包、reviewer、Agent 控制面状态机或
业务知识库。具体业务 gateway 负责业务工作流，Agent Platform 负责控制面和
执行面，部署层只负责把 Open WebUI 指向 `global-router`。

## 验收口径

生产化第一阶段验收证明：

1. Open WebUI 只能看到 allowlist 模型。
2. 已知模型能转发到对应后端。
3. 未 allowlist 模型会被拒绝。
4. 可见模型名能按需重写为后端模型名。
5. route timeout、鉴权、Bridge HMAC、RBAC、审计、健康、熔断和 fallback 生效。

完整生产级远端验收仍必须覆盖 streaming smoke、目标部署的 admin token /
audit path / route file reload 配置，以及多实例共享状态、外部审计 sink 和
独立管理面身份体系。
