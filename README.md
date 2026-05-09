# 通灵玉 Agent 项目

本仓库当前主线是“通灵玉”：一个面向《红楼梦》的研究型 Hermes Agent。目标不是泛泛聊天，也不是继续维护旧的 EPUB 基础库，而是建立一条可追溯的资料链路：

`source snapshot -> 知识库 -> 证据卡片 -> 证据包 -> reviewer 审校 -> 分层回答`

## 当前现实

已具备：

- `scripts/extract_epub.py`: 通用 EPUB source snapshot 抽取脚本。
- `scripts/download_wikisource.py`: 维基文库/MediaWiki source snapshot 下载脚本。
- `scripts/bilibili_hlm_pipeline.py`: B 站视频下载、音频处理和转录脚本。
- `resources/styles/buhongjushi/`: 已提交的“不红居士”风格转录和元数据。
- `docs/tonglingyu-agent-design/`: 通灵玉第一版产品和架构设计文档。
- `src/tonglingyu_agent/`: 通灵玉实现入口，目前仍是骨架。

尚未具备：

- `resources/sources/` 下的正式基础资料快照。
- SQLite/FTS 知识库建库脚本。
- Gateway、四个内部 Agent profile、证据 schema、reviewer 审校链路。
- Open WebUI 中的“通灵玉”模型注册和路由。

已废弃：

- 旧 `resources/base/hongloumeng/` 基础资料产物。
- 旧红楼梦专用资料抽取脚本。
- 旧平台化测试文档不作为通灵玉第一版依据。

## 文档入口

- [项目概览](docs/PROJECT_OVERVIEW.md)
- [目录结构](docs/DIRECTORY_STRUCTURE.md)
- [运行手册](docs/RUNBOOK.md)
- [知识库实现计划](docs/KB_SERVICE_PLAN.md)
- [转录校订流程](docs/VERIFICATION_WORKFLOW.md)
- [进展与决策记录](docs/PROGRESS.md)
- [通灵玉设计文档地图](docs/tonglingyu-agent-design/00_阅读路径与文档地图.md)

## 资料处理

抽取 EPUB：

```bash
.venv/bin/python scripts/extract_epub.py path/to/source.epub \
  --source-id tonglingyu-source-id \
  --source-category base_material \
  --edition "edition label" \
  --out resources/sources/epub
```

下载维基文库《红楼梦》全本：

```bash
.venv/bin/python scripts/download_wikisource.py \
  --source-id hongloumeng-wikisource \
  --title "红楼梦 维基文库全本" \
  --work "红楼梦" \
  --edition "维基文库" \
  --page "紅樓夢" \
  --prefix "紅樓夢/" \
  --out resources/sources/wiki
```

下载脂批本或其他版本资料时，使用独立 `source_id` 和合适的 `source_category`，不要混入同一个来源快照。

## 风格资料

“不红居士”是当前第一批讲解风格资料的项目内名称，来源于 B 站“红楼梦文本探究”视频转录。它只影响表达方式和讲解路径，不作为正文、脂批或版本校勘的最高证据。

转录文本里的讲解者自称按原文保留，例如 `不红君` 不因风格名而替换。

## 验证

Python 语法检查：

```bash
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub.py scripts/download_wikisource.py src/tonglingyu_agent/__init__.py
```

Markdown 和空白检查：

```bash
git diff --check
```

第一条视频三份转录一致性检查见 [运行手册](docs/RUNBOOK.md)。

## 关键原则

- 新资料先进入 `resources/sources/` source snapshot，再进入知识库构造。
- 基础资料必须可追溯；视频转录只是风格语料或待校订初稿。
- 生僻字、异体字、旧字形和来源中已有读音必须保留，不得只留下规范化检索文本。
- `官中`、`宫中`、`公中` 等高风险同音词必须回到已登记文本证据确认。
- 音频、视频、原始大文件和临时缓存不提交。
