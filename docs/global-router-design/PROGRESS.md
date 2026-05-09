# Global Router 进展

## 当前状态

- `global-router` 是独立 OpenAI-compatible 路由层，不从属于“通灵玉”
  Gateway、Agent Platform 控制面或其它具体业务项目。
- 当前完成的是 MVP 路由层，不是完整生产级 router。
- 实现入口：`agent-platform/crates/global-router/`。
- 生产部署已暂停使用 `global-router`；`deploy/docker-compose.yml` 当前让
  Open WebUI 直接连接 `tonglingyu-gateway` 和 `agent-orchestrator`。

## 已完成

- OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
- 模型列表从 `GLOBAL_ROUTER_ROUTES_JSON` 显式 allowlist 暴露。
- 按 Open WebUI 可见 model 路由到后端 `base_url`。
- 支持 `other/default` 这类 namespaced model id，避免多个后端模型重名。
- 支持把可见模型重写为后端 `upstream_model`，例如
  `model=other/default` 转发为 `model=default`。
- 每条 route 支持 `timeout_seconds`。
- 基础错误归一化：缺 model、未 allowlist、缺 `agent_bridge_context` 和
  转发失败。
- 基础鉴权透传：route 配置 `api_key_env` 时给后端注入 Bearer token；
  否则透传入站 `Authorization`。
- 基础 streaming 透传结构：以 bytes stream 透传上游响应。

## 已验证过

- 早期远端 `hhost` 曾部署 `global-router:formal`。
- 早期 Open WebUI 曾通过 `OPENAI_API_BASE_URL=http://global-router:8099/v1`
  连接 `global-router`。
- 容器内验证 `/v1/models` 只返回 allowlist 中的 `tonglingyu`。
- 未 allowlist 的 `other/default` 返回 `model_not_allowed`。
- `tonglingyu` 模型请求可转发到后端 `tonglingyu-gateway`。

## 当前部署决策

- `global-router` 暂不进入生产部署。
- Open WebUI 直接配置多个 OpenAI-compatible connection：
  `http://tonglingyu-gateway:8090/v1` 和
  `http://agent-orchestrator:8080/v1`。
- `tonglingyu-gateway` 暴露 `tonglingyu`；`agent-orchestrator` 暴露
  `hermes-agent`。
- `agent_identity_bridge` Filter 只绑定 `hermes-agent`，不注入到
  `tonglingyu` 证据问答。

## 未完成

- Router 自己的入站鉴权。
- Route-level 用户权限和 RBAC。
- Router 内校验 `agent_bridge_context` 的 HMAC 签名。
- 持久化审计记录。
- 对上游错误做完整统一归一化。
- 自动向多个 gateway 拉取 `/v1/models` 并聚合模型列表。
- 远端 streaming smoke。
- 管理面热更新 route。
- Route 健康状态汇总。
- 熔断、降级和 fallback 策略。

`requires_bridge=true` 当前只检查 `agent_bridge_context` 是否存在，不代表
router 已完成身份签名校验。

## 下一步

1. 补远端 streaming smoke，确认 Open WebUI 到上游 gateway 的流式透传。
2. 定义 router 入站鉴权和 route-level RBAC 的最小生产口径。
3. 定义 bridge HMAC 校验边界，避免只依赖字段存在性。
4. 设计持久化审计记录和 trace id 关联。
5. 设计 route 健康检查、熔断、降级和热更新管理面。
