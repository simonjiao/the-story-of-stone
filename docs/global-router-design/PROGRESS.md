# Global Router 进展

## 当前状态

- `global-router` 是独立 OpenAI-compatible 路由层，不从属于“通灵玉”
  Gateway、Agent Platform 控制面或其它具体业务项目。
- 当前已完成生产化第一阶段基线，可用于受控试运行；它不等同于完整生产级
  router。远端 streaming smoke 尚待目标环境复测。
- 实现入口：`agent-platform/crates/global-router/`。
- 部署入口：`deploy/docker-compose.yml` 中的 `global-router` 服务。

## 已完成

- OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
- 模型列表从 `GLOBAL_ROUTER_ROUTES_JSON` 或 `GLOBAL_ROUTER_ROUTES_FILE`
  显式 allowlist 暴露。
- 按 Open WebUI 可见 model 路由到后端 `base_url`。
- 支持 `other/default` 这类 namespaced model id，避免多个后端模型重名。
- 支持把可见模型重写为后端 `upstream_model`，例如
  `model=other/default` 转发为 `model=default`。
- 每条 route 支持 `timeout_seconds`。
- Router 入站 Bearer 鉴权。
- Route-level 用户角色和 subject allowlist。
- Router 内校验 `agent_bridge_context` 的 HMAC 签名、issuer、model、
  timestamp、nonce、subject 和 chat 边界；验证后剥离上下文再转发。
- Bridge nonce 在进程内防重放。
- JSONL 持久化审计记录。
- Router 自己错误和上游非 2xx 错误归一化。
- 自动向多个 gateway 拉取 `/v1/models` 并按 namespace 聚合模型列表。
- 管理面 route reload、route 配置查看和 route 健康状态汇总。
- Route 熔断和 fallback 策略；fallback 不绕过目标 route 权限，也不打到
  已熔断 route。
- 基础鉴权透传：route 配置 `api_key_env` 时给后端注入 Bearer token；
  否则透传入站 `Authorization`。
- 基础 streaming 透传结构：以 bytes stream 透传上游响应。

## 已验证

- 本地 Rust 检查：`cargo clippy --manifest-path agent-platform/Cargo.toml -p
  global-router -- -D warnings`。
- 本地 Rust 测试：`cargo test --manifest-path agent-platform/Cargo.toml`。
- Open WebUI Function 测试：`python3 -m unittest
  deploy/open-webui/functions/test_agent_identity_bridge_filter.py`。
- Compose 配置渲染：使用 dummy secret/env 执行 `docker compose -f
  deploy/docker-compose.yml config --quiet`。
- CLI 默认配置输出：`cargo run --quiet --manifest-path agent-platform/Cargo.toml
  -p global-router -- print-config`。

生产化第一阶段改造前的既有远端连接验证：

- 远端 `hhost` 已部署 `global-router:formal`。
- Open WebUI 通过 `OPENAI_API_BASE_URL=http://global-router:8099/v1` 连接
  `global-router`。
- 容器内验证 `/v1/models` 只返回 allowlist 中的 `tonglingyu`。
- 未 allowlist 的 `other/default` 返回 `model_not_allowed`。
- `tonglingyu` 模型请求可转发到后端 `tonglingyu-gateway`。

## 未完成 / 待远端验证

- 远端 streaming smoke。
- 多实例共享熔断状态和 Bridge nonce 防重放状态。
- 审计落库或外部审计 sink。
- Admin API 的独立 JWT/RBAC；当前是单一 admin Bearer token。

## 下一步

1. 补远端 streaming smoke，确认 Open WebUI 到上游 gateway 的流式透传。
2. 在目标部署设置 `GLOBAL_ROUTER_ADMIN_API_KEY`、审计路径和可选 route file。
3. 如需多副本部署，设计共享熔断状态或外部健康控制面。
