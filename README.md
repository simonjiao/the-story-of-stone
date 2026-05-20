# The Story of Stone

本仓库承载几个相关但边界独立的 Hermes / Open WebUI 工作区：

- “通灵玉”：面向《红楼梦》的研究型 Hermes Agent。
- `global-router`：独立 OpenAI-compatible 路由层。
- Agent Platform：Hermes 多 Agent 控制面、运行面和审计链路。
- `open-webui/functions/`：Open WebUI Function/Filter 正式代码。
- `deploy/`：通用 compose、构建和启动入口。
- `../tonglingyu-gatekeeper/deploy/`：定制环境文件、维护脚本和验证流程。

`具体进展不要写在根` README；以各项目目录下的 `PROGRESS.md` 为准。

## 项目入口

<!-- markdownlint-disable MD013 -->
| 项目 | 代码入口 | 文档入口 | 进展 |
| --- | --- | --- | --- |
| 通灵玉 | `agent-platform/crates/tonglingyu-gateway/` | `docs/tonglingyu-agent-design/` | `docs/tonglingyu-agent-design/PROGRESS.md` |
| Global Router | `agent-platform/crates/global-router/` | `docs/global-router-design/` | `docs/global-router-design/PROGRESS.md` |
| Agent Platform | `agent-platform/` | `docs/agent-platform-design/` | `docs/agent-platform-design/PROGRESS.md` |
| Open WebUI Functions | `open-webui/functions/` | `docs/tonglingyu-agent-design/` | `docs/tonglingyu-agent-design/PROGRESS.md` |
| Deployment | `deploy/` | `deploy/README.md` | `docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md` |
| Gatekeeper | `../tonglingyu-gatekeeper/deploy/` | `../tonglingyu-gatekeeper/deploy/README.md` | `docs/tonglingyu-agent-design/PROGRESS.md` |
<!-- markdownlint-enable MD013 -->

## 当前边界

- 通灵玉第一版只验证证据型 RAG 链路：
  `source snapshot -> 知识库 -> 证据卡片 -> 证据包 -> reviewer 审校 -> 分层回答`。
- `global-router` 当前是 MVP 路由层，不是完整生产级 router。
- Agent Platform 的 P0/P1/P2 状态以 `docs/agent-platform-design/PROGRESS.md`
  和各 implementation checklist 为准。
- compose、构建和启动入口保留在本仓库 `deploy/`；定制环境维护和验证以
  `../tonglingyu-gatekeeper/deploy/` 当前内容为准；公网入口走 Cloudflare
  Tunnel 到 Open WebUI。

## 文档入口

- [通灵玉设计文档地图](docs/tonglingyu-agent-design/00_阅读路径与文档地图.md)
- [通灵玉进展](docs/tonglingyu-agent-design/PROGRESS.md)
- [通灵玉当前差距与实施方向](docs/tonglingyu-agent-design/16_现有架构差距与实施方向.md)
- [通灵玉完整知识库与风格扩展规划](docs/tonglingyu-agent-design/17_完整知识库与风格扩展规划.md)
- [通灵玉第一版实施细化计划](docs/tonglingyu-agent-design/18_第一版实施细化计划.md)
- [通灵玉第一批资料来源登记](docs/tonglingyu-agent-design/19_第一批资料来源登记.md)
- [Global Router 设计](docs/global-router-design/README.md)
- [Global Router 进展](docs/global-router-design/PROGRESS.md)
- [Agent Platform 进展](docs/agent-platform-design/PROGRESS.md)
- [Agent Platform 总览](docs/agent-platform-design/00-overview.md)
- [运行手册](docs/RUNBOOK.md)
- [转录校订流程](docs/VERIFICATION_WORKFLOW.md)
- [跨项目进展索引](docs/PROGRESS.md)
- [Lint and Test Rules](docs/LINT_AND_TEST_RULES.md)
- [Versioning Rules](docs/VERSIONING_RULES.md)

## 资料边界

新资料先进入 `resources/sources/` source snapshot，再进入知识库。第一批基础资料目标是维基文库《红楼梦》全本、脂批本或同等可追溯公开来源。

`resources/styles/` 只保存讲解风格和待校订转录，不作为正文、脂批或版本校勘的最高证据。

资料处理必须保留原始字形；生僻字、异体字、旧字形和来源中已有读音不得被规范化文本覆盖。

知识库不是大向量库。正文、脂批、版本、人物关系、事件、诗词判词、现代白话摘要和研究观点必须分层；现代白话摘要只可辅助检索，不能作为回答证据。

## 通灵玉常用命令

```bash
uv run python scripts/extract_epub.py path/to/source.epub \
  --source-id tonglingyu-source-id \
  --source-category base_material \
  --edition "edition label" \
  --out resources/sources/epub
```

```bash
uv run python scripts/download_wikisource.py \
  --source-id hongloumeng-wikisource \
  --title "红楼梦 维基文库全本" \
  --work "红楼梦" \
  --edition "维基文库" \
  --page "紅樓夢" \
  --prefix "紅樓夢/" \
  --out resources/sources/wiki
```

```bash
scripts/qa.sh --quick
git diff --check
```

Rust 建库和本地 Gateway：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  build-kb \
  --source-root resources/sources/wiki \
  --db data/tonglingyu/tonglingyu.db \
  --rebuild
```

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  serve \
  --bind 127.0.0.1:8090 \
  --db data/tonglingyu/tonglingyu.db \
  --model-id tonglingyu \
  --model-name 通灵玉
```

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  eval \
  --db data/tonglingyu/tonglingyu.db \
  --report data/tonglingyu/reports/eval-smoke.json
```

```bash
agent-platform/scripts/tonglingyu-gateway-smoke.sh
```

## Global Router 常用命令

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p global-router -- \
  print-config
```

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p global-router -- \
  serve \
  --bind 127.0.0.1:8099
```
