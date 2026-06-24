# Tonglingyu Gateway

本仓库当前只保留通灵玉 Gateway 运行相关代码和文档。

- `agent-platform/`：通灵玉 Gateway、Runtime 和共享 Agent runtime 组件。
- `open-webui/functions/`：Open WebUI Function/Filter 正式代码。
- `deploy/`：本地 Tonglingyu stack 入口。
- `docs/`：按需求、架构、外部、内部、子模块组织的 Gateway/Runtime 文档。

Gateway 只消费外部发布的 runtime SQLite DB。DB 路径由 `TONGLINGYU_DB_PATH`
或 CLI `--db` 指定；本仓库负责 schema preflight/migration、协议入口、
鉴权、会话、SSE/WebSocket、admin、metrics、smoke 和 release gate。

具体进展不要写在根 README；以各项目目录下的 `PROGRESS.md` 为准。

## 项目入口

<!-- markdownlint-disable MD013 -->
| 项目 | 代码入口 | 文档入口 | 进展 |
| --- | --- | --- | --- |
| 通灵玉 Gateway/Runtime | `agent-platform/crates/tonglingyu-gateway/`、`agent-platform/crates/tonglingyu-runtime/` | `docs/` | `docs/PROGRESS.md` |
| Open WebUI Functions | `open-webui/functions/` | `docs/` | `docs/PROGRESS.md` |
| Local Stack | `deploy/` | `deploy/README.md` | `deploy/README.md` |
<!-- markdownlint-enable MD013 -->

## 文档入口

- [通灵玉文档地图](docs/README.md)
- [通灵玉进展](docs/PROGRESS.md)
- [Gateway 设计](docs/架构/Gateway设计.md)
- [Gateway 总体架构与实时能力改造方案](docs/架构/Gateway_Realtime_Redis_Architecture.md)
- [运行手册](docs/RUNBOOK.md)
- [进展索引](docs/PROGRESS.md)
- [Lint and Test Rules](docs/LINT_AND_TEST_RULES.md)
- [Versioning Rules](docs/VERSIONING_RULES.md)

## Runtime DB

外部发布 runtime DB 后，本仓库的接入步骤是：

1. 对 DB 运行 `runtime-schema-preflight`。
2. 对 DB 运行 `runtime-schema-migrate`。
3. 通过 `TONGLINGYU_DB_PATH` 或 CLI `--db` 启动 Gateway。
4. 用 `TONGLINGYU_SMOKE_DB_PATH` 显式绑定 DB 跑 RQA 查询/聊天 smoke。

`agent-platform/scripts/tonglingyu-knowledge-calibration-smoke.sh` 只是运行期治理
smoke：它在临时 DB 中创建 synthetic governance task 候选，验证 calibration
状态机，不加载外部内容。

## 常用命令

```bash
scripts/qa.sh --quick
git diff --check
```

运行 schema migration：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  runtime-schema-migrate \
  --db data/tonglingyu/tonglingyu.db
```

本地启动 OpenAI-compatible Gateway：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  serve \
  --bind 127.0.0.1:8090 \
  --db data/tonglingyu/tonglingyu.db \
  --model-id tonglingyu \
  --model-name 通灵玉
```

查询并生成证据包，要求 `--db` 指向已包含 runtime 数据的 SQLite DB：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  query \
  --db data/tonglingyu/tonglingyu.db \
  "通灵玉上的字是什么？" \
  --limit 8
```

Gateway smoke 默认只验证 schema、health、auth、models 和 metrics；RQA 查询/聊天
smoke 必须显式提供 runtime DB。

```bash
agent-platform/scripts/tonglingyu-gateway-smoke.sh
TONGLINGYU_SMOKE_DB_PATH=/path/to/tonglingyu.db agent-platform/scripts/tonglingyu-gateway-smoke.sh
```
