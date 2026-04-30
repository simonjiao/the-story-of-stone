# 运行手册

本仓库当前已有 `.venv` 和 `requirements.txt`。后续校验知识库服务实现时，Python 项目和依赖统一迁移到 `uv` 管理。

## 环境准备

当前脚本可直接使用现有虚拟环境：

```bash
.venv/bin/python --version
```

如需重新安装当前依赖：

```bash
.venv/bin/pip install -r requirements.txt
```

后续服务实现后的标准方式将改为：

```bash
uv sync --locked
```

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

用更高质量模型处理单条视频：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --offset 3 --limit 1 --asr-model small
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

## 基础版本文本抽取

```bash
.venv/bin/python scripts/extract_epub_hongloumeng.py
```

主要输出：

- `resources/base/hongloumeng/chapters_txt/`
- `resources/base/hongloumeng/sections_txt/`
- `resources/base/hongloumeng/combined/`
- `resources/base/hongloumeng/images/`
- `resources/base/hongloumeng/metadata/`

## 常用验证命令

检查 Python 脚本语法：

```bash
python3 -m py_compile scripts/bilibili_hlm_pipeline.py scripts/extract_epub_hongloumeng.py
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

## 计划中的知识库命令

这些命令尚未实现，作为后续服务开发的目标接口：

```bash
uv run hlm-kb-build --base-corpus resources/base/hongloumeng --out data/hongloumeng.sqlite
uv run hlm-kb-serve --db data/hongloumeng.sqlite --records data/verification_records.jsonl --host 0.0.0.0 --port 8000
uv run hlm-kb-query search "官中的钱"
```
