# 目录结构

## 根目录

- `README.md`: 当前项目入口、状态和快速命令。
- `AGENTS.md`: 仓库内协作规则，保持简短。
- `requirements.txt`: 现有 Python 脚本依赖。后续实现服务化入口时再迁移到 `pyproject.toml` / `uv.lock`。
- `.gitignore`: 大文件、缓存和生成物规则。
- `deploy/`: 现有 Hermes/Open WebUI 部署资料。通灵玉第一版可复用部署经验，但当前文档主线不以部署为起点。

## 代码

- `scripts/bilibili_hlm_pipeline.py`: B 站视频下载、音频提取、字幕获取和 ASR 转录。
- `scripts/extract_epub.py`: 通用 EPUB source snapshot 抽取脚本。
- `scripts/download_wikisource.py`: 维基文库/MediaWiki source snapshot 下载脚本。
- `src/tonglingyu_agent/`: 通灵玉 Gateway、知识库、证据 schema、Agent profile 和 reviewer 流程的实现入口。目前只有骨架。

## 资源

- `resources/hongloumeng_asr_glossary.txt`: ASR 热词和红楼术语表。
- `resources/styles/`: 风格资料。当前已提交 `buhongjushi/metadata/` 和 `buhongjushi/transcripts/`。
- `resources/sources/`: 新资料的 source snapshot 输出目录，运行资料脚本后生成。
- `resources/cache/`: 本地下载、音频、视频、ASR 中间产物和可重建缓存，默认不提交。
- `data/tonglingyu/`: 本地生成的 SQLite、索引、审计和评测产物，默认不提交。

## 文档

- `docs/PROJECT_OVERVIEW.md`: 项目目标、现实状态和资料边界。
- `docs/RUNBOOK.md`: 当前可执行命令。
- `docs/KB_SERVICE_PLAN.md`: 通灵玉知识库实现计划。
- `docs/VERIFICATION_WORKFLOW.md`: 视频转录校订流程。
- `docs/PROGRESS.md`: 当前进展、决策和下一步。
- `docs/LINT_AND_TEST_RULES.md`: Lint/test 规则。
- `docs/tonglingyu-agent-design/`: 通灵玉第一版产品和架构设计文档。

## 已移除的旧文档

以下文档线不再作为通灵玉第一版依据：

- 旧通用平台设计文档；
- 旧 Open WebUI bridge 设计和实施清单；
- 旧 Open WebUI 测试报告和 issue 记录；
- 旧红楼梦专用基础库抽取说明。

相关历史仍可从 Git 历史追溯，但不保留在当前文档入口中。

## 提交规则

可以提交：

- 文档、脚本、词表和轻量 schema；
- 已确认可复用的风格资料；
- 小规模、可追溯、适合进入仓库的 source snapshot 元数据。

不要提交：

- `.venv/`、`__pycache__/`、缓存目录；
- `resources/cache/` 下的音频、视频和中间产物；
- `data/tonglingyu/` 下的本地数据库、索引、审计和评测运行产物；
- 原始大文件、临时解包目录和未登记来源的大宗资料。
