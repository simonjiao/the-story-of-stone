# 进展与决策记录

## 已完成

- 建立 B 站视频处理流水线。
- 已处理“红楼梦文本探究”合集 60 个视频。
- 已生成视频转录文本、SRT 字幕和转录 JSON。
- 已抽取当前《红楼梦》基础版本文本、注释/校记、目录、图片字形和元数据。
- 已将可复用资料归入 `resources/base/` 和 `resources/styles/`，临时大文件归入 `resources/cache/`。
- 已提交基础版本可读文本、视频转录文本、词表和处理脚本。
- 第一条视频已完成重点术语校订。
- 第一条视频 `.txt/.srt/.transcript.json` 已确认文本内容一致。
- 已明确长期产品目标：构建可修订、可追溯、可自定义风格的《红楼梦》交互机器人。

## 关键提交

- `e90b92e Add Hongloumeng extraction pipelines and text outputs`
- `df72c08 Improve Hongloumeng ASR terminology handling`
- `745488f Correct Hongloumeng ASR glossary terms`
- `262a4c8 Align first Hongloumeng transcript files`

## 重要决策

- 当前已抽取的《红楼梦》版本作为后续校验的 `base corpus`。
- 当前《红楼梦》原文、注释/校记、图片字形和元数据属于基础资料，可直接引用。
- 后续新增的其他《红楼梦》发行版本属于扩展基础资料，也可直接引用。
- 研究资料用于观点和研究脉络，需标明来源，不覆盖基础资料。
- 风格资料附加在基础资料之上，只影响表达方式和讲解路径。
- `resources/base/` 存放可直接引用的基础资料；`resources/styles/` 存放附加风格资料；`resources/cache/` 只作本地缓存。
- 文档和知识库概念使用 `base corpus` 或“基础版本文本”，不把知识库命名为具体文件格式。
- 前八十回表示荣国府公家钱物/供应体系的常用词是 `官中`。
- `公中` 不进入当前 ASR 热词表，避免干扰前八十回相关讲解。
- `宫中` 与 `官中` 分立，不能按同音替换。
- `不红君` 是讲解者自称，应原样保留。
- B 站“红楼梦文本探究”视频转录作为一种讲解风格语料，风格名为 `不红居士`。
- `不红居士` 是风格档案名，不替换转录文本中的 `不红君`。
- 生僻字/异体字图片作为字形证据保留，不强制 OCR。
- 后续知识库服务采用 Python + uv + SQLite FTS + SQLite 内嵌 embedding + FastAPI + 远程 HTTP MCP + Docker。

## 待做

- 将 Python 项目迁移到 `uv` 管理。
- 实现 base corpus SQLite 建库脚本。
- 建立术语索引、图片字形索引和 embedding 表。
- 实现 HTTP API 和远程 HTTP MCP tools。
- 实现 `verify_transcript_quote` 跨章节校验接口。
- 实现服务端追加校订记录。
- 建立 `不红居士` 风格档案，并预留更多发行版本和研究资料接入。
- 为剩余视频逐步执行跨章节校订。
