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

## Gateway Run/Response 实施记录

### 2026-06-29

- 基线：`gateway-orchestrator-docs` 已 rebase 到 `origin/main` 的 `c28dd2b`，保留
  gateway orchestrator 设计提交。
- P0 开始：新增 `tonglingyu-gateway` 合同模块 `run_manager.rs` 和
  `response_events.rs`。
- P0 落点：
  - `run_manager.rs` 负责 canonical `run_id -> response_id -> chat_completion_id`
    归一化、owner scope、幂等键、metadata 信任边界和 forbidden control field gate。
  - `response_events.rs` 负责 `ResponseEvent`、`ResponseStatus`、事件白名单、
    public/admin 可见性和公开 payload 递归脱敏。
  - `response_events.rs` 已补齐 `RuntimeWorkflowStreamEvent -> ResponseEvent` 映射，
    当前将 `started`、`content_delta`、`final_output` 投影为公开事件，将
    `step_completed` 和未知 Runtime event 投影为 admin-only 事件。
- P1 开始：新增 `response_store.rs`，定义 `ResponseEventStore` trait 和内存实现，
  覆盖 `run_id -> response_id` 映射、原子 sequence/state 更新、状态机校验、
  replay 和 `requires_action` 计数。
- P4 最小 HTTP 投影开始：`main.rs` 已接入 `/v1/responses` 和 `/v1/runs`
  create/status/events/cancel，以及 `/v1/runs/{run_id}/actions/{action_id}`。
  当前所有入口共用 `RunIdentity` 和 `ResponseEventStore`，不会创建第二套执行对象。
- P4 已补强：
  - create 支持幂等键复用既有 response/run state。
  - status 与 events 使用 tenant + subject owner scope 校验。
  - SSE replay 输出 `id: <sequence>`，支持 query `after` 和 `Last-Event-ID` 恢复。
  - cancel 写入 `response.status(canceling)` 和 `response.canceled` 事件，并返回终态。
  - action submit 当前只允许在非终态继续检查；终态 run 返回 `run_terminal`。
- 当前限制：
  - P1 仍未连接 Redis；内存实现只作为合同测试和最小 HTTP 投影基底，后续 Redis
    实现必须遵守同一 trait 和状态机。
  - P2/P3 worker 与真正在线 stream 尚未接入；`/v1/responses` create 当前只创建
    queued state 和可 replay 事件，不伪装成已完成同步 workflow。
  - `/v1/chat/completions` 仍沿用现有 RQA path，尚未桥接到统一 Run/Response store。
  - `control stream`、action store、WS 和 background callback 尚未实现。
- 已验证：
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `run_manager` 通过，5 个测试。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_events` 通过，7 个测试。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_store` 通过，6 个测试。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过，224 个测试。
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - 上述命令编译过程中出现的 `tonglingyu-runtime` dead code warning 属于既有 warning，
    本轮未处理。
- 下一步：按顺序进入 P1 Redis store 与 P2 response job worker。Redis 未接入前，
  不能声明后台任务、断线长轮询、WS 或 production-ready replay 已完成。

### 2026-06-29 P1 Redis event store

- P1 已将 `response_store.rs` 从纯内存合同扩展为 `ResponseStoreBackend`：
  - 未配置 Redis 且 `TONGLINGYU_REDIS_REQUIRED=false` 时，仅作为本地开发/单测内存
    store 运行。
  - 配置 Redis URL 或生产 `TONGLINGYU_REDIS_REQUIRED=true` 时，启动阶段会 `PING`
    Redis；不可用直接 fail-closed。
  - Redis store 使用 Lua 脚本完成 `create_response` 的 response/run/idempotency
    原子登记，以及 `append_event` 的 state compare-and-set、sequence 递增、stream
    append 和 TTL 设置。
  - `response:{id}:control` 和 `response:{id}:actions` 已接入 cancel/action 写入；
    Run cancel 和 action submit 不再只改公开事件。
- Gateway 配置新增：
  - `TONGLINGYU_REDIS_URL`
  - `TONGLINGYU_REDIS_REQUIRED`
  - `TONGLINGYU_RESPONSE_STREAM_PREFIX`
  - `TONGLINGYU_RESPONSE_EVENT_MAXLEN`
  - `TONGLINGYU_RESPONSE_EVENT_TTL_SECS`
- `/healthz`、JSON metrics 和 Prometheus metrics 已报告 response store mode/status；
  Redis 异常会让 health 降级为 `response_store_unavailable`。
- `deploy/docker-compose.yml` 已新增 Redis 7 service、AOF volume、healthcheck，并让
  Gateway depends_on Redis 且默认 `TONGLINGYU_REDIS_REQUIRED=true`。
- 已验证：
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_store` 通过，10 个测试。
- 仍需在 P2-P7 完成：
  - job stream/consumer group、worker lease、retry、dead letter 和 reclaim。
  - chat stream 真正桥接 response event stream。
  - Responses/Run 的 background/sync wait 完整语义和 action 等待态。
  - `/v1/realtime/ws`、Runtime live event sink、smoke/runbook/release gate。

### 2026-06-29 P2 job queue and worker

- P2 新增 `response_jobs.rs`：
  - `ResponseJob` 使用 `tonglingyu.response_job.v1` schema，记录 run/response/session、
    owner scope、原始 request、attempt 和 max_attempts。
  - `ResponseJobQueueBackend` 支持本地内存测试后端和 Redis 后端。
  - Redis 后端使用 `tonglingyu:jobs` stream、`tonglingyu:jobs:dead` dead-letter stream、
    `XGROUP CREATE MKSTREAM`、`XREADGROUP`、`XACK`、retry requeue 和 `XAUTOCLAIM`
    stale reclaim。
- Gateway create 流程已改为：
  - 新建 response state 后写 `response.created` 并入队 `ResponseJob`。
  - 幂等命中只返回既有 response state，不重复入队。
  - 入队失败时将刚创建的 response 标记为 `response.failed`，避免永久 queued 半状态。
- Gateway worker 已接入：
  - `TONGLINGYU_RESPONSE_WORKER_ENABLED` 控制后台 worker。
  - worker claim job 后执行真实链路：context governance -> HTTP retriever -> Runtime
    workflow -> SQLite journal -> response events。
  - worker 在安全点检查 cancel；已取消/终态 job 正常 ack，不进入 retry。
  - worker failure 写 `worker.retry_scheduled`；达到最大重试后写
    `worker.dead_lettered` 和公开 `response.failed`。
  - worker 输出 `evidence.searching`、`evidence.found`、`review.started`、
    `review.completed`、Runtime stream event 投影和 `response.completed`。
- `/healthz`、JSON metrics 和 Prometheus metrics 已增加 `response_jobs` 依赖状态。
- `deploy/docker-compose.yml` 已补齐 worker、job group、retry、claim/reclaim 相关 env。
- 已验证：
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_jobs` 通过，5 个测试。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_create_enqueues_job_only_for_new_identity` 通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_worker_completes_empty_input_from_queued_job` 通过。
  - 既有 Run/Response 投影测试、owner scope、cancel/action 和 metadata override 测试
    均已重跑通过。
- 当前边界：
  - Runtime workflow 的 step-level live emit 仍等待 P6；P2 worker 目前在 Runtime
    workflow 返回后投影 `RuntimeWorkflowStreamEvent`。
  - chat stream 仍未桥接统一 response event stream，进入 P3 处理。
  - background webhook、sync wait、requires_action 等完整 Responses/Run 语义进入 P4。

## 验证边界

必须区分三类验证：

1. Repo-local gate：版本、Python function tests、shell syntax、Rust fmt/check/test。
2. Gateway smoke：空 runtime DB 下验证 schema、health、auth、models 和 metrics。
3. RQA/release gate：绑定外部 runtime DB、live Gateway、Open WebUI 和 release report。

不能用 repo-local gate 或空 DB smoke 宣称 RQA production-ready。
