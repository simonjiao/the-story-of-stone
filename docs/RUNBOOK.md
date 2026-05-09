# 运行手册

本手册只记录当前仓库可执行或明确尚未实现的命令。知识库、Gateway 和 reviewer 服务仍未实现；不要把计划命令当作当前可运行入口。

## 环境准备

当前脚本可直接使用现有虚拟环境：

```bash
.venv/bin/python --version
.venv/bin/pip install -r requirements.txt
```

后续服务化实现后再迁移到 `uv`、`pyproject.toml` 和 `uv.lock`。

## 视频处理

默认输出到 `resources/styles/buhongjushi/`。其中 `transcripts/` 和 `metadata/` 可复用并可提交，音频、视频和中间缓存默认忽略。

查看会处理哪些视频，不下载：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --dry-run --limit 10
```

下载并处理指定范围：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --offset 3 --limit 3 --asr-model base
```

带红楼术语词表重转录：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py \
  --limit 1 \
  --asr-model small \
  --asr-glossary resources/hongloumeng_asr_glossary.txt \
  --prefer-asr \
  --force-transcript
```

## EPUB Source Snapshot

`scripts/extract_epub.py` 将任意 EPUB 抽取为规范化 source snapshot。它只做资料标准化，不构造知识库。

```bash
.venv/bin/python scripts/extract_epub.py path/to/source.epub \
  --source-id tonglingyu-source-id \
  --source-category base_material \
  --edition "edition label" \
  --out resources/sources/epub
```

主要输出：

- `metadata/source.json`
- `metadata/manifest.json`
- `metadata/spine.json`
- `documents/documents.jsonl`
- `documents/blocks.jsonl`
- `combined/all_sections.txt`
- `combined/all_sections.md`
- `assets/`

若 EPUB 中包含 ruby 注音，脚本会在文本中渲染为 `字形（读音）`，并在对应 block 写入 `rare_char_annotations`。

## 维基文库 Source Snapshot

全本《红楼梦》可用根页面加子页前缀：

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

脂批本或其他版本使用独立来源快照：

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

如果本机 Python 证书链不完整，可临时加 `--insecure-skip-tls-verify` 做本地调试；正式流程不要默认使用该参数。

维基文库页面如包含 ruby 注音，脚本同样写入 `rare_char_annotations`。后续建库不得只保留规范化检索文本而丢弃该字段。

## 验证命令

检查 Python 脚本语法：

```bash
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub.py scripts/download_wikisource.py src/tonglingyu_agent/__init__.py
```

检查 Markdown 和空白：

```bash
git diff --check
```

检查第一条视频转录 JSON：

```bash
python3 -m json.tool \
  resources/styles/buhongjushi/transcripts/001_BV1qSjdz5ET2_司棋大闹大观园厨房，一碗炖鸡蛋，埋伏着曹雪芹精心设置的妙笔.transcript.json \
  >/dev/null
```

检查第一条视频三份文本内容一致：

```bash
python3 - <<'PY'
from pathlib import Path
import json, re, hashlib, sys

stem = '001_BV1qSjdz5ET2_司棋大闹大观园厨房，一碗炖鸡蛋，埋伏着曹雪芹精心设置的妙笔'
base = Path('resources/styles/buhongjushi/transcripts')
txt = [line.strip() for line in (base / f'{stem}.txt').read_text(encoding='utf-8').splitlines() if line.strip()]

def srt_lines(text):
    result = []
    for block in re.split(r'\n\s*\n', text.strip()):
        parts = block.splitlines()
        if len(parts) >= 3:
            result.append(' '.join(line.strip() for line in parts[2:] if line.strip()))
    return [line for line in result if line]

srt = srt_lines((base / f'{stem}.srt').read_text(encoding='utf-8'))
data = json.loads((base / f'{stem}.transcript.json').read_text(encoding='utf-8'))
segments = [str(seg.get('content', '')).strip() for seg in data.get('segments', []) if str(seg.get('content', '')).strip()]

def digest(lines):
    return hashlib.sha256('\n'.join(lines).encode('utf-8')).hexdigest()[:16]

print(f'txt: {len(txt)} {digest(txt)}')
print(f'srt: {len(srt)} {digest(srt)}')
print(f'json: {len(segments)} {digest(segments)}')
sys.exit(0 if txt == srt == segments else 1)
PY
```

## 尚未实现

以下能力还没有可执行命令：

- `tonglingyu-kb-build`
- `tonglingyu-kb-query`
- `tonglingyu-kb-serve`
- Gateway 服务
- reviewer 审校链路
- Open WebUI “通灵玉”模型注册

实现计划见 [知识库实现计划](KB_SERVICE_PLAN.md) 和 [通灵玉设计文档](tonglingyu-agent-design/00_阅读路径与文档地图.md)。
