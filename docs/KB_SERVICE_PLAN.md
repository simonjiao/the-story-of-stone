# 通灵玉知识库实现计划

本文件描述通灵玉第一版知识库的实现方向。当前仓库尚未实现建库服务；本文中的数据库、API 和 CLI 是下一阶段要实现的目标，不代表当前已有命令。

主设计入口仍是 `docs/tonglingyu-agent-design/08_知识库与RAG设计.md`。如有冲突，以 `docs/tonglingyu-agent-design/` 中的产品和架构约束为准。

## 当前现实

已有：

- 通用 EPUB source snapshot 抽取脚本；
- 维基文库/MediaWiki source snapshot 下载脚本；
- source snapshot 中的 ruby 注音保留字段 `rare_char_annotations`；
- “不红居士”风格转录资料；
- `src/tonglingyu_agent/` 实现入口。

未有：

- 正式 `resources/sources/` 基础资料快照；
- SQLite/FTS 数据库；
- 建库 CLI；
- 检索 API；
- 证据卡片和证据包持久化；
- Gateway 与 reviewer 调用链路。

## 建库输入

知识库只接受已登记的 source snapshot。每个 source snapshot 必须包含：

- `source_id`
- `source_category`
- `format`
- `title`
- `work`
- `edition`
- `language`
- 来源 URL、文件 hash 或等价来源说明
- 抽取时间
- `documents/blocks.jsonl`

当前允许的 `source_category`：

- `base_material`
- `extended_base_material`
- `commentary_material`
- `research_material`
- `style_material`
- `evaluation_material`

## 数据模型

第一版 SQLite 至少包含：

- `sources`: 来源登记和来源边界。
- `editions`: 作品版本、校本、批本和来源说明。
- `chapters`: 回目、章节序号、标题和版本归属。
- `blocks`: 文本块、类型、章节、段落号、来源路径和原始文本。
- `blocks_fts`: FTS5 精确检索索引。
- `rare_char_annotations`: 生僻字、异体字、旧字形及其可追溯读音，关联具体 `block_id` 和来源位置。
- `terms`: 人名、器物、术语、异名、例句和规则说明。
- `evidence_cards`: 可返回给 Gateway 的证据卡片。
- `kb_version`: 数据源集合、生成时间、代码 commit、索引版本。

本地生成物写入 `data/tonglingyu/`，默认不提交。

## 证据规则

- 原始字形必须保留；规范化文本只能作为检索辅助字段。
- `rare_char_annotations` 必须从 source snapshot 进入独立表，证据卡片返回时原样带出。
- 向量或语义召回只负责候选发现，不直接成为事实依据。
- 脂批、正文、版本说明、研究观点必须保持不同证据类型。
- 模型生成内容不得反写为知识库事实。
- 风格资料只影响表达方式，不覆盖正文、脂批或版本证据。

## 检索流程

第一版检索采用保守 hybrid 流程：

1. FTS 按回目、人名、诗句、关键词和术语召回；
2. 语义检索召回解释性问题候选；
3. 以章节线索、精确词命中、来源等级和证据类型重排；
4. 返回证据卡片；
5. Gateway 基于证据卡片组织证据包；
6. reviewer 审校证据是否足够、是否过度推断。

## 计划 API

第一版服务化后再提供 HTTP API。候选接口：

- `GET /health`
- `GET /sources`
- `GET /sources?category=base_material`
- `GET /search?q=&kind=&chapter=&limit=`
- `GET /chapter/{chapter_no}`
- `GET /terms/{term}`
- `GET /evidence/{evidence_id}`
- `POST /evidence-packages`
- `POST /review`

这些接口名是通灵玉知识库/Gateway 边界，不沿用旧的 `hongloumeng_*` 工具命名。

## 计划 CLI

当前不可执行。实现时使用通灵玉命名：

```bash
uv run tonglingyu-kb-build --source resources/sources/wiki/hongloumeng-wikisource --out data/tonglingyu/kb.sqlite
uv run tonglingyu-kb-query --db data/tonglingyu/kb.sqlite search "官中的钱"
uv run tonglingyu-kb-serve --db data/tonglingyu/kb.sqlite --host 0.0.0.0 --port 8000
```

## 实施顺序

1. 下载并登记维基文库《红楼梦》全本、脂批本和版本说明资料。
2. 定义 `src/tonglingyu_agent/` 下的 source snapshot loader。
3. 实现 SQLite schema 和建库 CLI。
4. 将 `rare_char_annotations` 建入独立表。
5. 实现 FTS 查询和证据卡片 schema。
6. 增加最小评测集，覆盖正文定位、脂批定位、版本说明、字形读音和证据不足。
7. 再实现 Gateway 调用、证据包记录和 reviewer 审校。
