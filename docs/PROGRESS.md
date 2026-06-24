# 通灵玉 Gateway/Runtime 进展与决策记录

## 进度边界

本文记录通灵玉 Gateway、Runtime、Open WebUI 集成、部署和运行侧进展。

当前 Gateway 只消费外部准备好的 runtime SQLite DB。DB 路径由 `TONGLINGYU_DB_PATH`
或 CLI `--db` 指定；schema 初始化和迁移由 `runtime-schema-migrate` 负责。
后续新增内容时，应由外部流水线发布 versioned runtime DB 或 release
bundle；本仓库只执行 preflight、migration、Gateway 读取和 release/smoke 验证。

## 当前状态

- `agent-platform/` 保留 `agent-core`、`agent-runtime`、`tonglingyu-runtime`
  和 `tonglingyu-gateway`。
- `tonglingyu-gateway` 是 OpenAI-compatible 入口，负责鉴权、限流、会话、
  trace、SSE、模型隐藏、admin API、metrics、响应封装和受控 Runtime step plan。
- `tonglingyu-runtime` 负责运行期 schema、FTS 查询、证据卡片、证据包、
  reviewer、replay、audit、治理、memory 和 rule catalog。
- Open WebUI 只通过 Gateway 暴露 `tonglingyu`；Function/Action 代码保留在
  `open-webui/functions/`。
- `deploy/` 保留 Tonglingyu-only compose，只挂载 Gateway/Runtime 运行所需配置和
  外部 runtime DB。
- Gateway realtime 目标采用仓库自有 `/v1/responses`、`/v1/realtime/ws` 和
  Redis Streams 事件基底，不集成 OpenAI Realtime；语音处理留在端侧或独立服务。

## 当前命令

```bash
scripts/qa.sh --quick
```

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  runtime-schema-migrate \
  --db data/tonglingyu/tonglingyu.db
```

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  serve \
  --bind 127.0.0.1:8090 \
  --db data/tonglingyu/tonglingyu.db \
  --model-id tonglingyu \
  --model-name 通灵玉
```

Gateway smoke 默认验证 schema、health、auth、models 和 metrics。RQA 查询/聊天 smoke
需要通过 `TONGLINGYU_SMOKE_DB_PATH` 提供外部发布的 runtime DB。

```bash
agent-platform/scripts/tonglingyu-gateway-smoke.sh
TONGLINGYU_SMOKE_DB_PATH=/path/to/tonglingyu.db agent-platform/scripts/tonglingyu-gateway-smoke.sh
```

calibration smoke 只验证运行期治理状态机，不加载外部内容：

```bash
agent-platform/scripts/tonglingyu-knowledge-calibration-smoke.sh
```

## 决策记录

- Gateway CLI 只保留运行、迁移、查询、health、admin 和 smoke 相关入口。
- Python 依赖只服务当前 Open WebUI Function、版本和轻量脚本。
- 默认 smoke 不依赖完整 runtime 数据；端到端 RQA smoke 显式依赖外部 runtime DB。
- 后续如需接入新的内容，应扩展外部 runtime DB 发布契约或 release bundle 校验。

## 验证边界

必须区分三类验证：

1. Repo-local gate：版本、Python function tests、shell syntax、Rust fmt/check/test。
2. Gateway smoke：空 runtime DB 下验证 schema、health、auth、models 和 metrics。
3. RQA/release gate：绑定外部 runtime DB、live Gateway、Open WebUI 和 release report。

不能用 repo-local gate 或空 DB smoke 宣称 RQA production-ready。
