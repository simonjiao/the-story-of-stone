# 多机器多 Agent 校验服务方案

本服务尚未实现。本文件记录已确定的实现方向，供后续开发直接执行。

新的“通灵玉”第一版知识库与 RAG 设计入口为 `docs/tonglingyu-agent-design/08_知识库与RAG设计.md`。本文件保留为校验服务技术计划草案；若两者出现冲突，以 `docs/tonglingyu-agent-design/` 中已冻结的产品/架构设计为准。

## 目标

构建 Docker 化的《红楼梦》校验服务，支持多机器、多 agent 共同访问同一套已登记文本证据、研究资料、风格资料和校订记录。旧的 EPUB 抽取基础库路径已删除；服务端主证据库应从 `resources/sources/` 下的新资料 source snapshot 构建。

## 技术选型

- 语言：Python。
- 项目管理：`uv`，后续新增 `pyproject.toml` 和 `uv.lock`。
- 主证据库：SQLite + FTS5，输入来自已批准的 source snapshot。
- 向量粗召回：v1 使用 SQLite 内嵌 embedding 表，由 Python 计算相似度。
- HTTP API：FastAPI + uvicorn。
- Agent 接入：远程 HTTP MCP。
- 部署：Docker + docker-compose。
- 鉴权：API Token，环境变量 `HLDM_KB_TOKEN`。

## 数据模型

主库只读、可重建，至少包含：

- `sources`: 资料来源，必须标记 `source_category`，取值为 `base_material`、`extended_base_material`、`research_material`、`style_material`。
- `editions`: 《红楼梦》发行版本信息；v1 等待新的基础资料来源确定后记录。
- `chapters`: 回目、标题、章节序号。
- `blocks`: 文本块、类型、章节、段落号、来源路径。
- `blocks_fts`: FTS5 精确检索索引。
- `terms`: 术语、出现章节、例句、规则说明。
- `evidence_assets`: 已批准来源附带的非文本证据路径、上下文和确认状态。
- `block_embeddings`: `block_id`、embedding、模型名、维度、生成版本。
- `styles`: 风格档案，记录风格名、来源语料、适用场景和表达约束。
- `kb_version`: 数据源集合、生成时间、代码 commit、embedding 模型。

资料分类规则：

- `base_material`: 重新选定并登记来源的《红楼梦》文本证据，可直接引用和用于校订。
- `extended_base_material`: 后续新增的其他《红楼梦》发行版本，可直接引用，用于版本比较和异文校验。
- `research_material`: 研究资料，可引用观点和出处，但不覆盖基础资料。
- `style_material`: 风格资料，只影响表达方式、讲解路径和用户偏好的对话风格，不能作为原文校订的最高证据。

校订记录单独保存，服务端追加写入：

- `record_id`
- `created_at`
- `agent_id`
- `video_id`
- `segment_id`
- `timestamp`
- `original_text`
- `corrected_text`
- `evidence_refs`
- `confidence`
- `note`

## Hybrid Search

默认检索流程：

1. 向量检索召回 Top N 语义相关文本块。
2. SQLite FTS 按关键词、术语、章节、人名、器物名召回精确候选。
3. 合并候选，用章节线索、词项命中、字符串相似度、术语规则重排。
4. 返回 Top K 证据，包含回目、段落、原文、差异和建议。

约束：

- 向量结果只用于候选召回。
- 最终校订建议必须落到 SQLite 中可追溯的原文、注释、批语、术语或其他已批准证据。
- `verify_transcript_quote` 必须跨章节查找，不默认锁定单一章节。

## HTTP API

- `GET /health`
- `GET /search?q=&chapter=&kind=&mode=hybrid|fts|vector&limit=`
- `GET /chapter/{chapter_no}`
- `GET /term/{term}`
- `GET /evidence-assets/{asset_id}`
- `GET /styles`
- `GET /styles/{style_id}`
- `GET /sources`
- `GET /sources?category=base_material|extended_base_material|research_material|style_material`
- `POST /verify-candidates`
- `POST /records`

`POST /verify-candidates` 输入包括：

- `video_id`
- `segment_id`
- `timestamp`
- `line`
- `context_before`
- `context_after`

返回包括：

- 候选章节和文本块。
- 原文证据。
- 差异说明。
- 建议改法。
- 置信度。

## MCP Tools

- `search_hongloumeng_text`
- `get_hongloumeng_chapter`
- `lookup_hongloumeng_term`
- `get_text_evidence_context`
- `list_dialogue_styles`
- `get_dialogue_style`
- `verify_transcript_quote`
- `append_verification_record`

MCP tools 与 HTTP API 共用同一服务逻辑，避免两套校订规则分叉。

## Docker 部署

目标形态：

- 服务容器只读挂载主证据库。
- 服务容器读取已批准的数据源快照。
- 校订记录挂载为单独可写目录。
- `HLDM_KB_TOKEN` 通过环境变量注入。

计划命令：

```bash
uv run hlm-kb-build --source <approved-source> --out data/hongloumeng.sqlite
uv run hlm-kb-serve --db data/hongloumeng.sqlite --records data/verification_records.jsonl --host 0.0.0.0 --port 8000
docker compose up
```

## 实现 TODO

- 建立 `src/tonglingyu_agent/` 下的知识服务、证据 schema 和 Gateway 调用边界。
- 新增 `pyproject.toml`、`uv.lock` 和 CLI entry points。
- 实现建库脚本、FTS 查询、向量查询、hybrid 重排。
- 建立 `不红居士` 风格档案，来源为 B 站“红楼梦文本探究”视频转录。
- 下载维基文库《红楼梦》全本、脂批本等资料，并将其登记为 `base_material`、`commentary_material` 或 `extended_base_material`。
- 将 `不红居士` 视频转录登记为 `style_material`。
- 预留多发行版本和研究资料接入字段，但 v1 不强制实现版本比较。
- 实现 HTTP API、MCP tools、API Token 鉴权。
- 实现校订记录追加写入。
- 增加 Dockerfile 和 compose 配置。
