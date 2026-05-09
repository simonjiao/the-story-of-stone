# 运行手册

本手册只放当前可执行命令。知识库、Gateway、reviewer 和 Open WebUI “通灵玉”入口尚未实现。

## 环境

```bash
.venv/bin/python --version
.venv/bin/pip install -r requirements.txt
```

## 视频转录

默认输出到 `resources/styles/buhongjushi/`。`transcripts/` 和 `metadata/` 可提交；音频、视频和缓存不提交。

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --dry-run --limit 10
.venv/bin/python scripts/bilibili_hlm_pipeline.py --offset 3 --limit 3 --asr-model base
```

带术语词表重转录：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py \
  --limit 1 \
  --asr-model small \
  --asr-glossary resources/hongloumeng_asr_glossary.txt \
  --prefer-asr \
  --force-transcript
```

## Source Snapshot

EPUB 抽取：

```bash
.venv/bin/python scripts/extract_epub.py path/to/source.epub \
  --source-id tonglingyu-source-id \
  --source-category base_material \
  --edition "edition label" \
  --out resources/sources/epub
```

维基文库《红楼梦》全本：

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

脂批本或其他版本使用独立 `source_id` 和 `source_category`：

```bash
.venv/bin/python scripts/download_wikisource.py \
  --source-id zhipiben-wikisource \
  --source-category commentary_material \
  --title "脂批本 维基文库资料" \
  --work "红楼梦" \
  --edition "脂批本" \
  --page "脂砚斋重评石頭記庚辰本" \
  --prefix "脂砚斋重评石頭記庚辰本/" \
  --out resources/sources/wiki
```

如果本机证书链异常，可临时加 `--insecure-skip-tls-verify` 调试；正式流程不要默认使用。

以上脚本会把 ruby 注音写入 `rare_char_annotations`。后续建库必须消费该字段，不能只保留规范化检索文本。

## 验证

```bash
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub.py scripts/download_wikisource.py
git diff --check
```

转录校订和三文件一致性规则见 [转录校订流程](VERIFICATION_WORKFLOW.md)。

## 尚未实现

`tonglingyu-kb-build`、`tonglingyu-kb-query`、`tonglingyu-kb-serve`、Gateway、reviewer 和 Open WebUI 模型注册还没有可执行入口。实施方向见 [当前差距与实施方向](tonglingyu-agent-design/16_现有架构差距与实施方向.md)。
