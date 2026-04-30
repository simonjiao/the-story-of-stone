# 红楼梦文本与视频校验项目

本项目的目标是构建一个可修订、可追溯、可自定义风格的《红楼梦》交互机器人。当前阶段围绕《红楼梦》相关视频和基础版本文本，完成下载、音频提取、机器转录、文本抽取、术语校订和后续多 agent 校验知识库建设。

当前重点不是做泛泛的问答库，而是建立可追溯的校订流程和可扩展的风格语料体系：每条转录中的原文引用、脂批、章回名、人物名、器物名和同音词，都要能回到基础版本文本、注释或图片字形资源确认；用户后续可以选择不同文本版本、研究视角和对话风格进行探讨。

## 当前状态

- 已处理 B 站“红楼梦文本探究”合集全部 60 个视频。
- 已生成每条视频的 `.txt`、`.srt`、`.transcript.json`。
- 已抽取当前《红楼梦》基础版本文本、注释/校记、图片字形和元数据。
- 第一条视频已做重点术语校订，并确认三份转录文件文本内容一致。
- B 站视频转录被定位为一种讲解风格语料，风格名为“不红居士”。
- 多机器多 agent 校验知识库方案已确定，尚未实现服务端。

## 目录入口

- [项目概览](docs/PROJECT_OVERVIEW.md)
- [交互机器人愿景](docs/INTERACTIVE_BOT_VISION.md)
- [目录结构](docs/DIRECTORY_STRUCTURE.md)
- [运行手册](docs/RUNBOOK.md)
- [转录校订流程](docs/VERIFICATION_WORKFLOW.md)
- [校验知识库服务方案](docs/KB_SERVICE_PLAN.md)
- [进展与决策记录](docs/PROGRESS.md)

## 快速命令

只查看会选中哪些视频：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py --dry-run --limit 10
```

按词表重转录视频：

```bash
.venv/bin/python scripts/bilibili_hlm_pipeline.py \
  --limit 1 \
  --asr-model small \
  --asr-glossary resources/hongloumeng_asr_glossary.txt \
  --prefer-asr \
  --force-transcript
```

抽取基础版本文本：

```bash
.venv/bin/python scripts/extract_epub_hongloumeng.py
```

检查第一条视频三份转录文本是否一致：

```bash
python3 - <<'PY'
from pathlib import Path
import json, re, hashlib, sys

stem = '001_BV1qSjdz5ET2_司棋大闹大观园厨房，一碗炖鸡蛋，埋伏着曹雪芹精心设置的妙笔'
base = Path('resources/styles/buhongjushi/transcripts')
txt = [line.strip() for line in (base / f'{stem}.txt').read_text(encoding='utf-8').splitlines() if line.strip()]

def srt_lines(text):
    out = []
    for block in re.split(r'\n\s*\n', text.strip()):
        parts = block.splitlines()
        if len(parts) >= 3:
            out.append(' '.join(line.strip() for line in parts[2:] if line.strip()))
    return [line for line in out if line]

srt = srt_lines((base / f'{stem}.srt').read_text(encoding='utf-8'))
data = json.loads((base / f'{stem}.transcript.json').read_text(encoding='utf-8'))
segments = [str(seg.get('content', '')).strip() for seg in data.get('segments', []) if str(seg.get('content', '')).strip()]

def digest(lines):
    return hashlib.sha256('\n'.join(lines).encode('utf-8')).hexdigest()[:16]

print(len(txt), digest(txt))
print(len(srt), digest(srt))
print(len(segments), digest(segments))
sys.exit(0 if txt == srt == segments else 1)
PY
```

## 关键原则

- `官中` 是前八十回表示荣国府公家钱物/供应体系的常用词，不要误校为 `公中` 或 `宫中`。
- `不红君` 是讲解者自称，应原样保留。
- 原文引用、脂批和章回名必须跨章节交叉验证，不能只查视频标题对应章节。
- 生僻字/异体字图片保留为字形证据，不强行 OCR 成不确定字符。
- 音频、视频、原始大文件和临时缓存不提交。
