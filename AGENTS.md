# AGENTS.md

## 项目边界

- 本仓库当前收敛为通灵玉 Gateway/Runtime、Open WebUI 集成和本地运行入口。
- 通灵玉当前主线是 Gateway、Runtime、证据卡片、证据包、reviewer 审校、
  Open WebUI 入口、上游模型适配和实时文字协议。
- Rust workspace 保留 `agent-core`、`agent-runtime`、`tonglingyu-runtime`
  和 `tonglingyu-gateway`；前两者是通灵玉 Runtime 所需的支撑库。
- Gateway 只消费外部发布的 runtime SQLite DB；本仓库只维护读取、迁移、
  协议、安全、审计、smoke 和 release gate。

## 编码规则

- 先读相邻实现和接口契约，沿用现有模块边界、命名和错误处理风格。
- 改动保持最小可验证范围；格式化和重构只覆盖本次触及文件。
- Runtime 回答链路必须保留证据类型、source hash、source span、text cue、
  claim binding、review decision 和 trace 等可追溯字段。
- 配置和密钥只走 `.env` 或既有配置入口，不写入代码、compose 或日志输出。
- 提交前运行与改动匹配的检查或测试；无法运行时在交付说明中写清 blocker。
- Git 提交按 task 或节点及时拆分；大提交的提交信息正文按关键更改列出不超过
  5 条要点。
- Rust 编码规则遵循 `docs/RUST_CODING_RULES.md`，尤其注意模板化、并发和错误边界。
- Lint/test 细则遵循 `docs/LINT_AND_TEST_RULES.md`，`AGENTS.md` 不承载长命令清单。
- 版本管理和版本分级规则遵循 `docs/VERSIONING_RULES.md`，这里只保留入口。

## 文档规则

- 通灵玉产品和架构以 `docs/tonglingyu-agent-design/` 为准。
- 根 `docs/PROGRESS.md` 只是通灵玉文档入口索引；当前现实状态以
  `docs/tonglingyu-agent-design/PROGRESS.md` 为准。
- 运行命令只写当前可执行命令；计划中的命令必须明确标记为尚未实现。
- 设计文档必须描述完整目标、全量边界和最终验收条件；分阶段只用于安排实施顺序，
  不用于降低目标、隐藏困难或替换主流程。

## 部署规则

- 部署以当前 `deploy/` 内容为准。
