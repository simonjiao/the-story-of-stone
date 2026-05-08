# AGENTS.md

## 测试与 Issue 规则

- 测试先按最小关键路径执行：登录、模型选择、基础聊天、会话保存；关键路径失败时停止后续测试。
- 失败后先记录证据，再判断是否能在当前权限内修复；能修复则修复并复测，不能修复则标记为 `BLOCKED`。
- 测试报告写入 `docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md`，只记录摘要、结果和证据，不写入密码或密钥。
- 独立问题写入 `docs/chat_huixiangdou_issues/`，每个 issue 记录状态、等级、关联测试、证据和后续动作。
- 遵循渐进披露：`AGENTS.md` 只保留规则，测试计划、报告和 issue 文件承载细节。

## 编码规则

- 先读相邻实现和接口契约，沿用现有模块边界、命名和错误处理风格。
- 改动保持最小可验证范围；格式化和重构只覆盖本次触及文件，避免污染无关 diff。
- Rust 状态/并发代码要显式处理 lease、heartbeat、idempotency、锁归属和错误传播。
- 配置和密钥只走 `.env` 或既有配置入口，不写入代码、compose 或日志输出。
- 提交前运行与改动匹配的检查或测试；无法运行时在交付说明中写清 blocker。
- Lint/test 细则遵循 `docs/LINT_AND_TEST_RULES.md`，`AGENTS.md` 不承载长命令清单。

## 部署规则

- 部署以当前 `deploy/` 内容为准：`deploy/README.md`、`deploy/docker-compose.yml`、`deploy/.env`、`deploy/scripts/render-hermes-config.sh`、`deploy/scripts/env-backup.sh`。
- `deploy/.env` 是当前配置来源；读取时只暴露变量名，避免输出 token、API key、密码等值。
- 更新 `deploy/.env` 前先执行 `deploy/scripts/env-backup.sh backup`；备份保存到 `$HOME/OneDrive/backup/the-story-of-stone/deploy-env/`，只报告路径不输出内容。
- 恢复 `deploy/.env` 时先用 `deploy/scripts/env-backup.sh list` 确认备份，再执行 `deploy/scripts/env-backup.sh restore latest` 或指定备份文件；脚本会先备份当前 `.env`。
- Open WebUI 只作为登录和聊天窗口；Hermes 通过 `http://hermes:8642/v1` 作为 OpenAI-compatible 后端暴露 `hermes-agent`。
- 不把 Hermes 的 `8642` 或 `9119` 端口暴露到公网；公网入口走 Cloudflare Tunnel 到 `open-webui:8080`。
