# 进展与决策记录

## 已完成

- 建立 B 站视频处理流水线。
- 已处理“红楼梦文本探究”合集 60 个视频。
- 已生成视频转录文本、SRT 字幕和转录 JSON。
- 已删除旧 `resources/base/hongloumeng/` EPUB 抽取产物。
- 已将 EPUB 抽取改为通用 source snapshot 脚本，不再绑定红楼梦旧基础资料路径。
- 已新增维基文库 source snapshot 下载脚本。
- 已将临时大文件归入 `resources/cache/`。
- 已提交视频转录文本、词表和处理脚本。
- 第一条视频已完成重点术语校订。
- 第一条视频 `.txt/.srt/.transcript.json` 已确认文本内容一致。
- 已明确长期产品目标：构建可修订、可追溯、可自定义风格的《红楼梦》交互机器人。
- 已导入“通灵玉”红楼研究型 Hermes Agent 第一版渐进式设计文档。

## 关键提交

- `e90b92e Add Hongloumeng extraction pipelines and text outputs`
- `df72c08 Improve Hongloumeng ASR terminology handling`
- `745488f Correct Hongloumeng ASR glossary terms`
- `262a4c8 Align first Hongloumeng transcript files`

## 重要决策

- 基于旧 EPUB 抽取并构造基础知识库的路径已废弃，旧产物已删除。
- 新的基础资料来源需要重新选定、登记并记录来源边界。
- 新资料先输出到 `resources/sources/` source snapshot，再由知识库构造流程消费。
- 《红楼梦》全本、脂批本等基础资料优先从维基文库等可追溯来源下载并登记。
- 研究资料用于观点和研究脉络，需标明来源，不覆盖已登记基础资料。
- 风格资料附加在基础资料之上，只影响表达方式和讲解路径。
- `resources/styles/` 存放附加风格资料；`resources/cache/` 只作本地缓存。
- 前八十回表示荣国府公家钱物/供应体系的常用词是 `官中`。
- `公中` 不进入当前 ASR 热词表，避免干扰前八十回相关讲解。
- `宫中` 与 `官中` 分立，不能按同音替换。
- `不红君` 是讲解者自称，应原样保留。
- B 站“红楼梦文本探究”视频转录作为一种讲解风格语料，风格名为 `不红居士`。
- `不红居士` 是风格档案名，不替换转录文本中的 `不红君`。
- 后续校验服务采用 Python + uv + SQLite FTS + SQLite 内嵌 embedding + FastAPI + 远程 HTTP MCP + Docker；建库输入等待新的基础资料来源确定。
- “通灵玉”第一版采用 Open WebUI 单入口、Gateway 确定性编排、四个内部 Agent、证据包和 reviewer 审校硬流程。
- Gateway 不是第 5 个 Agent；它负责协议适配、鉴权、会话映射、编排、安全边界和审计，不生成文学回答。
- 通灵玉实现不以 `agent-platform/` 为前提；第一版新建独立 Agent/Gateway/知识库闭环。

## 待做

- 将 Python 项目迁移到 `uv` 管理。
- 重新确定基础资料来源和登记规则。
- 下载并登记维基文库《红楼梦》全本、脂批本和版本说明资料。
- 基于新来源实现 SQLite 建库脚本。
- 建立术语索引和 embedding 表。
- 新建 `src/tonglingyu_agent/` 下的 Gateway、profile 调用、证据 schema 和 reviewer 审校流程。
- 实现 HTTP API 和远程 HTTP MCP tools。
- 实现 `verify_transcript_quote` 跨章节校验接口。
- 实现服务端追加校订记录。
- 建立 `不红居士` 风格档案，并预留更多发行版本和研究资料接入。
- 基于 `docs/tonglingyu-agent-design/` 拆分第一版实施 checklist。
- 为剩余视频逐步执行跨章节校订。
