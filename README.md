# 通灵玉 Agent

本仓库当前主线是“通灵玉”：面向《红楼梦》的研究型 Hermes Agent。第一版只验证一条证据型 RAG 链路：

`source snapshot -> 知识库 -> 证据卡片 -> 证据包 -> reviewer 审校 -> 分层回答`

## 当前状态

已有：

- 通用 EPUB 抽取：`scripts/extract_epub.py`
- 维基文库/MediaWiki 下载：`scripts/download_wikisource.py`
- B 站视频转录流水线：`scripts/bilibili_hlm_pipeline.py`
- 风格资料：`resources/styles/buhongjushi/`
- 设计文档：`docs/tonglingyu-agent-design/`
- 实现入口骨架：`src/tonglingyu_agent/`

未有：

- 正式基础资料快照：`resources/sources/`
- SQLite/FTS 建库、八类知识库、证据卡片和证据包
- Gateway、内部 Agent profiles、reviewer 审校链路
- Open WebUI “通灵玉”模型入口

已废弃：旧基础库产物和旧专用抽取脚本。第一版不从已删除内容继续叠加。

## 文档入口

- [设计文档地图](docs/tonglingyu-agent-design/00_阅读路径与文档地图.md)
- [当前差距与实施方向](docs/tonglingyu-agent-design/16_现有架构差距与实施方向.md)
- [运行手册](docs/RUNBOOK.md)
- [转录校订流程](docs/VERIFICATION_WORKFLOW.md)
- [进展与决策记录](docs/PROGRESS.md)
- [Lint and Test Rules](docs/LINT_AND_TEST_RULES.md)

## 资料边界

新资料先进入 `resources/sources/` source snapshot，再进入知识库。第一批基础资料目标是维基文库《红楼梦》全本、脂批本或同等可追溯公开来源。

`resources/styles/` 只保存讲解风格和待校订转录，不作为正文、脂批或版本校勘的最高证据。

资料处理必须保留原始字形；生僻字、异体字、旧字形和来源中已有读音不得被规范化文本覆盖。

知识库不是大向量库。正文、脂批、版本、人物关系、事件、诗词判词、现代白话摘要和研究观点必须分层；现代白话摘要只可辅助检索，不能作为回答证据。

## 常用命令

```bash
.venv/bin/python scripts/extract_epub.py path/to/source.epub \
  --source-id tonglingyu-source-id \
  --source-category base_material \
  --edition "edition label" \
  --out resources/sources/epub
```

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

```bash
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub.py scripts/download_wikisource.py src/tonglingyu_agent/__init__.py
git diff --check
```
