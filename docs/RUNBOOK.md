# 运行手册

本手册只放当前可执行命令。通灵玉当前 Rust 入口是
`agent-platform/crates/tonglingyu-gateway/`。Gateway 运行时只消费
`TONGLINGYU_DB_PATH` 或 `--db` 指向的 runtime SQLite DB。

## 环境

```bash
uv sync
uv run python --version
```

## 验证

```bash
scripts/qa.sh --quick
git diff --check
```

## Runtime Schema

首次使用或升级 DB 后运行 schema migration：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  runtime-schema-migrate \
  --db data/tonglingyu/tonglingyu.db
```

迁移前检查：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  runtime-schema-preflight \
  --db data/tonglingyu/tonglingyu.db
```

## 外部 Runtime DB

runtime 数据由外部流水线发布为 runtime SQLite DB 或 release bundle。本仓库只做
schema 检查、迁移、Gateway 启动和 release/smoke 验证。

```bash
EXTERNAL_TONGLINGYU_DB=/path/to/tonglingyu.db

cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  runtime-schema-preflight \
  --db "${EXTERNAL_TONGLINGYU_DB}"

cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  runtime-schema-migrate \
  --db "${EXTERNAL_TONGLINGYU_DB}"
```

启动 Gateway 时绑定同一个外部 DB：

```bash
TONGLINGYU_DB_PATH="${EXTERNAL_TONGLINGYU_DB}" \
  cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
    serve \
    --bind 127.0.0.1:8090 \
    --model-id tonglingyu \
    --model-name 通灵玉
```

## 通灵玉 Gateway

本地启动 OpenAI-compatible Gateway：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  serve \
  --bind 127.0.0.1:8090 \
  --db data/tonglingyu/tonglingyu.db \
  --model-id tonglingyu \
  --model-name 通灵玉
```

健康检查：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  healthcheck \
  --url http://127.0.0.1:8090/healthz
```

Gateway smoke 默认验证 schema、retriever health/metadata contract、gateway health、
auth、models、metrics、Response/Run 创建、事件 replay、取消和保留控制字段拒绝。默认
关闭 response worker，且只启动 retriever health/metadata stub，因此该 smoke 只覆盖
gateway 协议和事件底座，不证明 RQA 生产可用：

```bash
agent-platform/scripts/tonglingyu-gateway-smoke.sh
```

如需额外验证 RQA 查询和聊天链路，提供外部发布的 runtime DB：

```bash
TONGLINGYU_SMOKE_DB_PATH=/path/to/tonglingyu.db \
  agent-platform/scripts/tonglingyu-gateway-smoke.sh
```

calibration smoke 使用临时 synthetic governance task 候选，只验证运行期状态机，
不加载外部内容：

```bash
agent-platform/scripts/tonglingyu-knowledge-calibration-smoke.sh
```

## 运行期操作

查询并生成证据包，要求 DB 已包含外部发布的 runtime 证据数据：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  query \
  --db data/tonglingyu/tonglingyu.db \
  "通灵玉上的字是什么？" \
  --limit 8
```

回放已生成的证据包：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  replay-package \
  --db data/tonglingyu/tonglingyu.db \
  pkg-example
```

运行内置评测样例，要求 DB 已包含评测所需 runtime 证据数据：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  eval \
  --db data/tonglingyu/tonglingyu.db \
  --report data/tonglingyu/reports/eval-smoke.json
```
