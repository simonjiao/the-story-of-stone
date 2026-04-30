# 红楼梦 B 站视频下载与转写

当前脚本默认使用这个空间页：

```bash
https://space.bilibili.com/558777092/lists
```

默认会选择标题/说明里包含“红楼梦”的合集，也就是“红楼梦文本探究”。

## 已完成

已处理“红楼梦文本探究”合集全部 60 个视频，输出在：

- `downloads/bilibili/videos/`: MP4 视频
- `downloads/bilibili/audio/`: m4a 音轨和 16k WAV
- `downloads/bilibili/text/`: ASR 文本、SRT 字幕、转写 JSON
- `downloads/bilibili/metadata/`: 合集清单和运行状态

## 继续处理

本项目已创建 `.venv` 并安装 `faster-whisper`。继续处理后续视频：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --offset 3 --limit 3 --asr-model base
```

只看会选中哪些视频，不下载：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --dry-run --limit 10
```

处理更高质量转写可以把模型换成 `small` 或 `medium`，但会明显更慢、占用更多磁盘：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --offset 3 --limit 1 --asr-model small
```

如需下载 720P/1080P，B 站要求登录，可导出 cookies 后传入：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --limit 1 --cookies cookies.txt
```

文本是机器 ASR 初稿，适合检索和粗读；涉及引用、整理出版或校勘时需要人工复核。

## EPUB 文本提取

从 `books/` 下的《红楼梦》EPUB 提取章节正文、注释/校记、目录、元数据和图片：

```bash
.venv/bin/python scripts/extract_epub_hongloumeng.py
```

默认输出到 `downloads/epub_hongloumeng/`。当前提交保留其中的可读资料和必要小型图片：

- `chapters_txt/`, `chapters_md/`: 按回整理的正文、注释/校记。
- `sections_txt/`, `sections_md/`: EPUB 全部分节，包括序言、目录、前后折页等。
- `combined/`: 合并版 TXT/Markdown。
- `metadata/`: 目录、spine、注释、manifest 和提取报告。
- `images/*.jpeg`, `cover.jpg`: EPUB 内嵌图片；正文中的异体字/生僻字多以小字形图片保存，文本里用 `[[image:images/00006.jpeg]]` 或 Markdown 图片语法引用。

未提交音频、视频、EPUB 源文件、原始解包目录和较大的 JSON 中间产物。
