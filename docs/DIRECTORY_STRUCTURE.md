# 目录结构

## 根目录

- `README.md`: 项目入口和快速命令。
- `requirements.txt`: 当前脚本依赖清单。后续校验服务实现时迁移到 `uv` 管理的 `pyproject.toml` 和 `uv.lock`。
- `.gitignore`: 大文件、缓存和生成物提交规则。
- `docs/`: 项目说明、校验流程、部署/测试规则和设计文档。

## 代码与资源

- `scripts/`: 现有可执行脚本。
  - `bilibili_hlm_pipeline.py`: B 站视频下载、音频提取、字幕获取和 ASR 转录。
  - `extract_epub.py`: 通用 EPUB source snapshot 抽取脚本，不绑定《红楼梦》具体版本。
  - `download_wikisource.py`: MediaWiki/Wikisource source snapshot 下载脚本，用于《红楼梦》全本、脂批本等基础资料。
- `resources/`: 校订和转录资源。
  - `hongloumeng_asr_glossary.txt`: ASR 热词和红楼术语表。
  - `sources/`: 新资料的规范化 source snapshot，供后续知识库构造使用。
  - `styles/`: 风格资料及其转录、元数据和风格档案。
  - `cache/`: 本地下载、音频、视频、ASR 中间产物和可重建缓存，默认不提交。
- `src/tonglingyu_agent/`: 通灵玉 Agent、Gateway、证据 schema 和审校流程的实现入口。

## 设计文档

- `docs/tonglingyu-agent-design/`: “通灵玉”红楼研究型 Hermes Agent 第一版渐进式设计文档。
  - `00_阅读路径与文档地图.md`: 文档地图和推荐阅读顺序。
  - `01_产品定位与边界.md` 到 `04_概念模型与证据分层.md`: 产品边界、用户流程、总体原则和证据模型。
  - `05_总体架构.md` 到 `08_知识库与RAG设计.md`: Open WebUI、Gateway、四个内部 Agent、知识服务和 RAG 设计。
  - `09_外部接口契约.md` 到 `12_验证方案与验收标准.md`: 接口语义、权限审计、安全治理和验收方案。
  - `13_负面清单与反模式.md` 到 `15_命名与品牌语义.md`: 反模式、迭代路线和“通灵玉”命名语义。

## 数据目录

- `books/`: 原始书籍文件目录，默认不提交大文件。
- `resources/styles/buhongjushi/transcripts/`: 已提交的视频转录文本、字幕和 JSON。
- `resources/styles/buhongjushi/metadata/`: 已提交的视频合集元数据。
- `resources/sources/`: 新资料的规范化 source snapshot 输出目录。
- `resources/cache/`: 临时下载、音频、视频、ASR 中间产物和可重建缓存。

## 资料分类

- 基础资料：后续重新选定并登记的《红楼梦》文本证据，可直接引用。
- 扩展基础资料：后续新增的其他《红楼梦》发行版本，可直接引用，并用于版本比较。
- 研究资料：论文、专著摘录、讲义、札记等，用于观点和研究脉络。
- 风格资料：视频转录、讲稿、解读文章等，附加在基础资料之上。
- B 站“红楼梦文本探究”视频转录作为 `不红居士` 风格的来源资料。
- 风格资料用于后续交互机器人的表达习惯和分析路径，不作为原文校订的最高证据。
- 转录中的讲解者自称按原文保留，例如 `不红君` 不因风格名而替换。

## 提交规则

可以提交：

- 文档、脚本、词表。
- 重新选定并登记来源的基础资料。
- `resources/styles/` 下可复用的风格资料。

不要提交：

- `.venv/`、`__pycache__/`、缓存目录。
- `resources/cache/` 下的音频、视频、中间产物和临时缓存。
- `*.mp4`、`*.m4a`、`*.wav`、`*.mp3` 等媒体文件。
- 原始书籍大文件和临时解包目录。
