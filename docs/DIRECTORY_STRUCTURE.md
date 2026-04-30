# 目录结构

## 根目录

- `README.md`: 项目入口和快速命令。
- `requirements.txt`: 当前脚本依赖清单。后续知识库服务实现时迁移到 `uv` 管理的 `pyproject.toml` 和 `uv.lock`。
- `.gitignore`: 大文件、缓存和生成物提交规则。

## 代码与资源

- `scripts/`: 现有可执行脚本。
  - `bilibili_hlm_pipeline.py`: B 站视频下载、音频提取、字幕获取和 ASR 转录。
  - `extract_epub_hongloumeng.py`: 当前基础版本文本、注释、图片和元数据抽取脚本。脚本名保留历史命名。
  - `download_ctext_hongloumeng.py`: 旧的 ctext 下载脚本；相关下载内容已移除，不作为当前主数据源。
- `resources/`: 校订和转录资源。
  - `hongloumeng_asr_glossary.txt`: ASR 热词和红楼术语表。
  - `base/`: 可直接引用的基础资料和扩展基础资料。
  - `styles/`: 风格资料及其转录、元数据和风格档案。
  - `cache/`: 本地下载、音频、视频、ASR 中间产物和可重建缓存，默认不提交。
- `src/hongloumeng_kb/`: 预留给后续校验知识库服务和查询逻辑。

## 数据目录

- `books/`: 原始书籍文件目录，默认不提交大文件。
- `resources/styles/buhongjushi/transcripts/`: 已提交的视频转录文本、字幕和 JSON。
- `resources/styles/buhongjushi/metadata/`: 已提交的视频合集元数据。
- `resources/base/hongloumeng/chapters_txt/`: 当前基础版本按回文本。
- `resources/base/hongloumeng/chapters_md/`: 当前基础版本按回 Markdown。
- `resources/base/hongloumeng/sections_txt/`: 当前基础版本全部分节文本。
- `resources/base/hongloumeng/sections_md/`: 当前基础版本全部分节 Markdown。
- `resources/base/hongloumeng/combined/`: 合并版 TXT/Markdown。
- `resources/base/hongloumeng/images/`: 生僻字/异体字等图片字形。
- `resources/base/hongloumeng/metadata/`: 目录、spine、注释、manifest 和抽取报告。
- `resources/cache/`: 临时下载、音频、视频、ASR 中间产物和可重建缓存。

## 资料分类

- 基础资料：当前《红楼梦》原文、注释/校记、图片字形和元数据，可直接引用。
- 扩展基础资料：后续新增的其他《红楼梦》发行版本，可直接引用，并用于版本比较。
- 研究资料：论文、专著摘录、讲义、札记等，用于观点和研究脉络。
- 风格资料：视频转录、讲稿、解读文章等，附加在基础资料之上。
- B 站“红楼梦文本探究”视频转录作为 `不红居士` 风格的来源资料。
- 风格资料用于后续交互机器人的表达习惯和分析路径，不作为原文校订的最高证据。
- 转录中的讲解者自称按原文保留，例如 `不红君` 不因风格名而替换。

## 提交规则

可以提交：

- 文档、脚本、词表。
- `resources/base/` 下可直接引用的基础资料。
- `resources/styles/` 下可复用的风格资料。

不要提交：

- `.venv/`、`__pycache__/`、缓存目录。
- `resources/cache/` 下的音频、视频、中间产物和临时缓存。
- `*.mp4`、`*.m4a`、`*.wav`、`*.mp3` 等媒体文件。
- 原始书籍大文件和临时解包目录。
