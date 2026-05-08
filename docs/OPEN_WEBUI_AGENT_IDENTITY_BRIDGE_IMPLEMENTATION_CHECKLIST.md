# Open WebUI Agent Identity Bridge Implementation Checklist

## 状态

- Owner: Codex
- Scope: 一次性完成 Agent Identity Bridge 正式实现、远程 Docker 部署和 smoke 测试
- Public entry: `chat.huixiangdou.top` -> Open WebUI
- Constraint: 不使用临时 Open WebUI，不写入 secret，不暴露 Manager/Worker/Observer

## Checklist

- [x] Core/API
  - [x] 增加 bridge binding domain model、status、API 输入输出。
  - [x] 增加 run 单条读取输出，供 Orchestrator 等待 run 结果。
- [x] Store
  - [x] 增加 Postgres migration `open_webui_bridge_bindings`。
  - [x] AgentStore 增加 get/upsert/close/update-run binding 方法。
  - [x] Memory store 和 Postgres store 都实现 binding 方法。
  - [x] 覆盖 active unique binding、close 后重建、多用户隔离测试。
- [x] Manager
  - [x] 增加 internal bridge binding endpoints。
  - [x] 增加 `GET /v1/my-runs/{run_id}`。
  - [x] approval fulfill 后根据 `bridge_source` 创建/复用 session 并写 binding。
  - [x] JWT 模式由现有 Manager 鉴权路径承载；远程 smoke 后关闭 dev headers。
- [x] Orchestrator
  - [x] 解析并验证 `agent_bridge_context`。
  - [x] 生成 Manager service/user JWT。
  - [x] 控制请求缺 context fail closed。
  - [x] 普通 Hermes passthrough 前删除 `agent_bridge_context`。
  - [x] 移除 legacy Orchestrator 内存 binding，正式路径只使用 Manager/Postgres binding。
  - [x] 控制请求 fulfilled 后读取 Manager 写入的 binding。
  - [x] 已绑定 chat 后续消息 append session message、create run、等待结果。
  - [x] 关闭当前 agent session 后清除 binding 并恢复默认 Hermes。
  - [x] 保留 Open WebUI follow-up prompt 误触发回归测试。
- [x] Open WebUI Function
  - [x] 新增 `agent_identity_bridge_filter.py`。
  - [x] Python/Rust 签名规范完全一致。
  - [x] 新增正式 Open WebUI Function 安装/更新脚本。
  - [x] 明确 Open WebUI admin 只用于 Function 管理，不自动获得 Agent Platform admin。
  - [x] 增加正式容器/DB installer 作为非 admin API token 场景的正式 fallback。
- [x] Deploy
  - [x] compose 增加 Bridge/JWT env 变量名。
  - [x] README 说明正式部署步骤和 `.env` 备份要求。
  - [x] 更新 `.env` 前执行 `deploy/scripts/env-backup.sh backup`。
  - [x] smoke 通过后设置 `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false`。
- [x] Local verification
  - [x] `cargo fmt --check`
  - [x] `cargo test --workspace`
  - [x] Python Filter unit test
  - [x] `git diff --check`
  - [x] `docker compose config`
- [x] Remote verification
  - [x] 构建并部署正式 Agent Platform 镜像。
  - [x] 安装/更新正式 Open WebUI Function。
  - [x] 登录正式 Open WebUI，验证模型、短聊、长回答。
  - [x] 会话保存沿用本日 UI 回归；Bridge 本轮通过正式 Open WebUI API 和 DB 证据验证。
  - [x] 验证真实 Open WebUI user 审计。
  - [x] 验证 session binding、multi-session、worker run。
  - [x] 验证篡改 context 拒绝、dev header 拒绝、普通聊天不回退。
  - [x] 记录未做专项远程 smoke 的边界：第二 Open WebUI 用户隔离、Orchestrator 重启后 binding 复用、Hermes 上游 payload 捕获。
  - [x] 失败时记录证据、修复、复测。
- [x] Docs/commit
  - [x] 更新测试报告。
  - [x] 更新 ISSUE-008 状态和证据。
  - [x] 生成提交。
