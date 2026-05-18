# AGENTS.md

## 项目边界

- 本仓库当前收敛为通灵玉 Agent 系统和 `deploy/`。
- 通灵玉第一版主线是资料 source snapshot、知识库、证据卡片、证据包、
  reviewer 审校、Open WebUI 入口、Gateway 和 Hermes runtime 上游。
- Rust workspace 只保留 `agent-core`、`agent-runtime`、
  `tonglingyu-runtime` 和 `tonglingyu-gateway`；前两者是通灵玉 Runtime 所需
  的支撑库。
- Global Router 和旧 Agent Platform 控制面不属于当前生产路径，不再新增相关代码、脚本或文档。
- `resources/styles/` 是风格资料边界；除非任务明确要求，不改写风格转录和元数据。

## 编码规则

- 先读相邻实现和接口契约，沿用现有模块边界、命名和错误处理风格。
- 改动保持最小可验证范围；格式化和重构只覆盖本次触及文件。
- 资料处理必须保留原始字形；生僻字、异体字、旧字形和已登记读音不得被静默规范化丢弃。
- 配置和密钥只走 `.env` 或既有配置入口，不写入代码、compose 或日志输出。
- 提交前运行与改动匹配的检查或测试；无法运行时在交付说明中写清 blocker。
- Git 提交按 task 或节点及时拆分；大提交的提交信息正文按关键更改列出不超过 5 条要点。
- Rust 编码规则遵循 `docs/RUST_CODING_RULES.md`，尤其注意模板化、并发和错误边界。
- Lint/test 细则遵循 `docs/LINT_AND_TEST_RULES.md`，`AGENTS.md` 不承载长命令清单。
- 版本管理和 deploy 自增规则遵循 `docs/VERSIONING_RULES.md`，这里只保留入口。

## 文档规则

- 通灵玉产品和架构以 `docs/tonglingyu-agent-design/` 为准。
- 根 `docs/PROGRESS.md` 只是通灵玉文档入口索引；当前现实状态以
  `docs/tonglingyu-agent-design/PROGRESS.md` 和 hhost 重建 checklist 为准。
- 运行命令只写当前可执行命令；计划中的命令必须明确标记为尚未实现。

## 部署规则

- 部署以当前 `deploy/` 内容为准。
- `deploy/.env` 是当前配置来源；读取时只暴露变量名，避免输出 token、API key、密码等值。
- 更新 `deploy/.env` 前先执行 `deploy/scripts/env-backup.sh backup`。
- 不把 Hermes 的 `8642` 或 `9119` 端口暴露到公网；公网入口走 Cloudflare Tunnel 到 Open WebUI。
