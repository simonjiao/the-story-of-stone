# AGENTS.md

## 项目边界

- 本仓库包含多个相关但边界独立的项目：通灵玉、Global Router、Agent Platform 和 `deploy/`。
- 通灵玉第一版主线是资料 source snapshot、知识库、证据卡片、证据包、reviewer 审校和 Open WebUI 入口。
- Global Router 是独立 OpenAI-compatible 路由层，不从属于通灵玉或 Agent Platform。
- Agent Platform 的控制面、运行面和审计链路以 `docs/agent-platform-design/` 为准。
- `resources/styles/` 是风格资料边界；除非任务明确要求，不改写风格转录和元数据。

## 编码规则

- 先读相邻实现和接口契约，沿用现有模块边界、命名和错误处理风格。
- 改动保持最小可验证范围；格式化和重构只覆盖本次触及文件。
- 资料处理必须保留原始字形；生僻字、异体字、旧字形和已登记读音不得被静默规范化丢弃。
- 配置和密钥只走 `.env` 或既有配置入口，不写入代码、compose 或日志输出。
- 提交前运行与改动匹配的检查或测试；无法运行时在交付说明中写清 blocker。
- Rust 编码规则遵循 `docs/RUST_CODING_RULES.md`，尤其注意模板化、并发和错误边界。
- Lint/test 细则遵循 `docs/LINT_AND_TEST_RULES.md`，`AGENTS.md` 不承载长命令清单。

## 文档规则

- 通灵玉产品和架构以 `docs/tonglingyu-agent-design/` 为准。
- Global Router 设计和进展以 `docs/global-router-design/` 为准。
- Agent Platform 设计和进展以 `docs/agent-platform-design/` 为准。
- 根 `docs/PROGRESS.md` 只是跨项目进展索引；当前现实状态以对应项目目录下的 `PROGRESS.md` 为准。
- 运行命令只写当前可执行命令；计划中的命令必须明确标记为尚未实现。

## 部署规则

- 部署以当前 `deploy/` 内容为准。
- `deploy/.env` 是当前配置来源；读取时只暴露变量名，避免输出 token、API key、密码等值。
- 更新 `deploy/.env` 前先执行 `deploy/scripts/env-backup.sh backup`。
- 不把 Hermes 的 `8642` 或 `9119` 端口暴露到公网；公网入口走 Cloudflare Tunnel 到 Open WebUI。
