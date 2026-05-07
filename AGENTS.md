# AGENTS.md

## 测试与 Issue 规则

- 测试先按最小关键路径执行：登录、模型选择、基础聊天、会话保存；关键路径失败时停止后续测试。
- 失败后先记录证据，再判断是否能在当前权限内修复；能修复则修复并复测，不能修复则标记为 `BLOCKED`。
- 测试报告写入 `docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md`，只记录摘要、结果和证据，不写入密码或密钥。
- 独立问题写入 `docs/chat_huixiangdou_issues/`，每个 issue 记录状态、等级、关联测试、证据和后续动作。
- 遵循渐进披露：`AGENTS.md` 只保留规则，测试计划、报告和 issue 文件承载细节。

## 部署规则

- 部署以当前 `deploy/` 内容为准：`deploy/README.md`、`deploy/docker-compose.yml`、`deploy/.env`、`deploy/scripts/render-hermes-config.sh`。
- `deploy/.env` 是当前配置来源；读取时只暴露变量名，避免输出 token、API key、密码等值。
- 更新任何配置前先备份原文件，优先使用同目录时间戳备份，例如 `deploy/.env.bak.YYYYMMDD-HHMMSS`。
- Open WebUI 只作为登录和聊天窗口；Hermes 通过 `http://hermes:8642/v1` 作为 OpenAI-compatible 后端暴露 `hermes-agent`。
- 不把 Hermes 的 `8642` 或 `9119` 端口暴露到公网；公网入口走 Cloudflare Tunnel 到 `open-webui:8080`。
