# 项目概览

本仓库当前服务于“通灵玉”第一版：一个面向《红楼梦》的研究型 Hermes Agent。用户在 Open WebUI 中只看到“通灵玉”一个入口；内部再由 Gateway、受控 Agent、知识库、证据包和 reviewer 审校流程协同完成回答。

## 当前目标

第一版先验证一条窄而确定的链路：

1. 从维基文库、EPUB 或其他可追溯来源生成 source snapshot；
2. 从 source snapshot 构造原文、脂批、人物事件、版本说明和字形读音索引；
3. 检索只返回证据卡片，不直接生成最终回答；
4. Gateway 组织证据包和内部 Agent 调用；
5. reviewer 对复杂回答执行硬审校；
6. 最终回答区分正文事实、脂批提示、版本差异和谨慎推断。

## 当前仓库现实

已有：

- 视频处理脚本和“不红居士”风格转录资料；
- 通用 EPUB source snapshot 抽取脚本；
- 维基文库/MediaWiki source snapshot 下载脚本；
- 通灵玉第一版产品、架构、接口和验收设计文档；
- `src/tonglingyu_agent/` 实现入口。

未有：

- 正式基础资料 source snapshot；
- SQLite/FTS 知识库；
- Gateway 服务；
- 内部 Agent profile、工具白名单和 reviewer 调用；
- Open WebUI 的“通灵玉”模型配置。

## 资料分类

- `base_material`: 可直接引用和用于校订的正文基础资料。
- `extended_base_material`: 其他版本、校本、可追溯异文和版本说明。
- `commentary_material`: 脂批、脂评、批语系统和对应正文位置。
- `research_material`: 论文、专著摘录、讲义、札记等研究观点。
- `style_material`: 视频转录、讲稿、解读文章等风格资料。
- `evaluation_material`: 标准问题、负面用例和验收集。

风格资料不能覆盖基础资料、脂批、版本证据或校订记录。

## 主要输入

- `https://zh.wikisource.org/`: 《红楼梦》全本、脂批本和相关公开页面。
- 新选定的 EPUB、校本、版本说明和人物事件资料。
- `resources/styles/buhongjushi/`: 已提交的“不红居士”风格转录。
- `resources/hongloumeng_asr_glossary.txt`: ASR 热词和红楼术语表。

## 主要输出

- `resources/sources/`: source snapshot 输出目录，运行资料脚本后生成。
- `data/tonglingyu/`: 本地生成的数据库、索引、审计和评测产物，默认不提交。
- `resources/styles/buhongjushi/transcripts/`: 已提交的视频转录文本、字幕和 JSON。
- `resources/styles/buhongjushi/metadata/`: 已提交的视频合集元数据。

## 风格资料边界

“不红居士”是项目内的风格名，不替换转录文本中的讲解者自称。转录文本里的 `不红君` 应按原文保留。

视频转录当前不是主证据库。它可以作为风格资料，也可以作为待校订文本，但不能直接证明《红楼梦》正文、脂批或版本判断。

## 设计文档

`docs/tonglingyu-agent-design/` 是通灵玉第一版的主设计集。阅读入口是 `00_阅读路径与文档地图.md`，覆盖产品定位、用户流程、证据分层、Gateway、四 Agent 分工、知识库与 RAG、接口契约、权限审计、验证方案、负面清单、迭代路线和命名语义。

## 核心原则

- source snapshot 是资料入库前的唯一标准中间形态。
- 旧 `resources/base/hongloumeng/` 和旧专用抽取路径不再作为输入。
- 原文引用、脂批和章回名必须回到已登记来源。
- 生僻字、异体字、旧字形和可追溯读音必须保真保存。
- 不把模型生成内容反写为知识库事实。
