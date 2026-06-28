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

Gateway smoke 默认验证 schema、retriever health/metadata contract、gateway health、
auth、models、metrics、Response/Run 创建、事件 replay、取消和保留控制字段拒绝。RQA
查询/聊天 smoke 需要通过 `TONGLINGYU_SMOKE_DB_PATH` 提供外部发布的 runtime DB。

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

### 2026-06-29 P3 chat completions stream bridge

- P3 已将 `/v1/chat/completions` 的 `stream=true` 分支桥接到统一 Run/Response：
  - chat stream 请求通过 `run_manager` 归一化为 `RunApiType::ChatCompletions`，
    分配同一个 `run_id/response_id/chat_completion_id`。
  - 新建 response 后入队 `ResponseJob`；幂等命中会接入既有 response stream，不重复
    入队。
  - SSE body 使用 `Body::from_stream` 持续读取同一 `ResponseEventStore`，直到 response
    terminal 后发送 OpenAI-compatible stop chunk 和 `[DONE]`。
  - `output_text.delta` 转换为 OpenAI `chat.completion.chunk` 的 `delta.content`。
  - `response.status`、`evidence.searching`、`evidence.found`、review 和 terminal public
    events 作为 chunk 的 `tonglingyu_event` 额外字段转发；兼容客户端仍可按 `data:`
    JSON chunk 消费。
  - streaming content 复用公开输出安全检查，避免把内部知识状态标签直接写入
    `delta.content`。
- 非 stream chat 暂未重构，继续走原同步 RQA path，保证 Open WebUI 非流式兼容行为不在
  P3 混入额外风险。
- 已验证：
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `chat_stream_bridges_to_response_job_events` 通过。

### 2026-06-29 P4 Responses/Run HTTP projection

- P4 已补齐 `/v1/responses` 和 `/v1/runs` 的 create/status/events/cancel/action 语义：
  - `stream=true` create 不返回 queued JSON，而是直接读取同一个 ResponseEvent SSE
    stream；Responses/Run 原生 SSE 输出 `response.*` 事件，Chat Completions 仍由 P3
    转换为 OpenAI chunk。
  - `background=false` 且非 stream 的 create 会等待同一 response state 进入终态，等待
    上限由 `TONGLINGYU_RESPONSE_SYNC_WAIT_SECS` 控制，默认 30 秒；超时返回当前 state，
    不伪造 completed。
  - `background=true` 继续立即返回 queued state，并只入队一次 response job。
  - status/events/cancel/action 均复用 `run_id -> response_id` 映射和 owner scope 校验，
    普通用户跨 tenant 或 subject 读取仍返回 not found。
  - action submit 现在只有在 run 处于 `requires_action` 且事件流存在匹配 `action_id`
    时接受；过期、未知 action 和终态 run 均 fail-closed，并写入 action audit stream。
  - action submit 使用 idempotency key digest 做幂等复用；重复提交同一 action 不追加
    第二个公开 `action_status=submitted` 事件。
  - `requires_action_count` 在恢复到 `in_progress` 时递减，避免状态投影长期显示待处理。
- 配置与部署：
  - CLI/env 新增 `TONGLINGYU_RESPONSE_SYNC_WAIT_SECS`。
  - `deploy/docker-compose.yml` 已传入该变量。
- 已验证：
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过，239 个测试。
- 当前边界：
  - P4 action submit 只恢复等待态并写审计/公开状态事件；真正由 action result 重新唤醒
    paused worker 的细粒度安全点在 P6 与 Runtime live sink 中继续补齐。
  - background webhook 尚未实现，仍按设计留到 P7 smoke/runbook 与后续 webhook worker。

### 2026-06-29 P5 realtime WebSocket text protocol

- P5 新增 `/v1/realtime/ws`：
  - WebSocket handshake 复用 Gateway auth/rate limit。
  - 只接受 JSON text frame；binary frame 返回 error 后关闭。
  - `input_audio.*` 和 `output_audio.*` 明确拒绝，Gateway 不承担音频处理。
  - client event 校验 `schema_version`、`event_id` 去重、递增 sequence、buffer 长度和
    forbidden control fields。
  - 支持 `session.start`、`input_text.delta`、`input_text.commit`、`response.create`、
    `response.cancel`、`response.resume`、`response.action.submit`、`ping` 和
    `session.close`。
  - `input_text.delta` 只更新 session buffer 并返回 ack，不创建 RQA job。
  - `input_text.commit + response.create` 通过 `RunApiType::RealtimeWs` 创建同一
    Run/Response state 和 response job。
  - WS fan-out 从同一个 `ResponseEventStore` 读取 public projection；不会重新生成
    RQA 内容，也不会暴露 `trace_id`。
  - `response.cancel` 写同一 control/event stream；`response.resume` 按 sequence replay。
  - `response.action.submit` 复用 P4 的 waiting action、idempotency、expiry 和 audit 规则。
- 依赖与配置：
  - `axum` 固定为 `=0.8.8` 并启用 `ws` feature；原因是 `axum 0.8.9` 依赖当前 registry
    不可用的 `tokio-tungstenite 0.29`。
  - 新增 `TONGLINGYU_REALTIME_MAX_BUFFER_CHARS`，compose 默认 4000。
- 已验证：
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `realtime_` 通过，3 个测试。
- 当前边界：
  - P5 fan-out 依赖 P2 worker 产出事件；Runtime workflow 内部 step-level live emit
    仍在 P6 补齐。
  - WS session buffer 当前为连接内状态，断线后只通过 `response.resume` 恢复 response
    event，不恢复未 commit 的 delta。

### 2026-06-29 P6 Runtime event sink and cancel safe points

- P6 新增 `tonglingyu-runtime` 的 `RuntimeWorkflowEventSink` trait：
  - `emit(RuntimeWorkflowStreamEvent)` 负责把 Runtime workflow event 写到外部事件底座。
  - `cancel_requested()` 在 Runtime safe point 后检查是否应停止 workflow。
  - 原有 `execute_workflow_with_agent_runtime_client` 保持兼容；新增带 event sink
    的执行入口供 Gateway worker 使用。
- Runtime workflow 现在在以下边界 emit：
  - workflow started。
  - agent runtime step 执行完成后的 `step_completed` 管理员事件。
  - final answer `content_delta` 和 `final_output`。
  - 每次 emit 后检查 cancel；sink 写失败会让 Runtime workflow 返回错误，不产生不可
    replay 的 final answer。
- Gateway worker 新增 `ResponseWorkflowEventSink`：
  - 复用 `response_event_from_runtime_stream_event` 做唯一映射。
  - 将 Runtime public/admin 可见性写入同一个 `ResponseEventStore`。
  - sink 写失败会让 response job 走失败/retry；如果失败同时检测到 cancel，则写
    `response.canceled` 并正常 ack。
  - worker 不再在 Runtime workflow 完成后批量重写 `workflow.stream_events`，避免重复
    stream 事件。
- review 事件顺序调整：
  - `review.started` 在进入 Runtime workflow 前写入。
  - `review.completed` 在 review journal 落库后写入。
- 已验证：
  - `cargo check --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    通过。
  - `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
    filter `response_workflow_event_sink_maps_runtime_events_and_cancel_signal`
    通过。
- 当前边界：
  - 当前 `RuntimeClient` 接口仍是非 streaming provider API；P6 通过 event sink 输出
    workflow step 和 final answer chunks。若后续 provider 暴露 token streaming，需要在
    `RuntimeClient` 增加 streaming 方法后接入同一 sink。
  - cancel safe point 覆盖 workflow event 边界和 Gateway worker 既有阶段边界；长时间
    provider call 内部的硬中断仍依赖后续 provider client 支持。

### 2026-06-29 P7 smoke/runbook/release regression gates

- Gateway smoke 从基础健康检查扩展为 Run/Response regression gate：
  - 空 DB 默认验证 schema migrate、retriever health/metadata contract、gateway health、
    auth、models、JSON metrics 和 Prometheus。
  - smoke 内启动最小 retriever health/metadata stub，只满足 Gateway 启动依赖。
  - 默认 stub 记录所有请求；若收到 `/retrieve`，smoke 直接失败，避免 mock retrieval。
  - 验证 response store/job queue health 与 metrics 依赖状态。
  - 验证 `/v1/responses` 后台创建、状态查询和 `/events` replay。
  - 验证 `/v1/runs` 后台创建、取消、events replay 和 terminal `[DONE]`。
  - 验证保留控制字段 `profile` 被拒绝为 `forbidden_control_fields`。
- smoke 环境显式设置 `TONGLINGYU_RESPONSE_WORKER_ENABLED=false`：
  - 避免空 DB 或无外部 provider 时误触发 Runtime/RQA。
  - 默认 gate 只证明 Gateway 协议、事件底座、鉴权/作用域和控制面。
  - 传入 `TONGLINGYU_SMOKE_DB_PATH` 后额外验证 DB search。
  - 同时传入 `TONGLINGYU_SMOKE_RETRIEVER_BASE_URL` 后才验证真实 RQA chat。
- Runtime warning 加固：
  - test-only local answer renderer 不再进入生产 lib 编译面。
  - answer/retrieval rule catalog 解析会读取并校验模板、排序和 hygiene 字段。
  - gateway regression gate 增加 `RUSTFLAGS="-D warnings"`。
- 已验证：
  - `bash -n agent-platform/scripts/tonglingyu-gateway-smoke.sh` 通过。
  - `agent-platform/scripts/tonglingyu-gateway-smoke.sh` 通过。
  - `RUSTFLAGS="-D warnings"` gateway cargo test 通过，243 个测试。
  - markdownlint 覆盖 `docs/PROGRESS.md`、`docs/RUNBOOK.md` 和 Gateway 架构文档，通过。
  - `git diff --check` 通过。
- 当前边界：
  - 默认 smoke 不替代 live Gateway、Open WebUI、Redis 和外部 runtime DB 的发布前 gate。
  - smoke retriever stub 不提供 `/retrieve`，且会拒绝默认 smoke 中的检索调用。
  - Redis Streams 模式仍需要单独带 `TONGLINGYU_REDIS_URL` 的集成环境覆盖。

## 验证边界

必须区分三类验证：

1. Repo-local gate：版本、Python function tests、shell syntax、Rust fmt/check/test。
2. Gateway smoke：空 runtime DB 下验证 schema、retriever contract、gateway health、
   auth、models、metrics、Response/Run 投影、事件 replay、取消和请求边界。
3. RQA/release gate：绑定外部 runtime DB、真实 retriever、live Gateway、Open WebUI 和
   release report。

不能用 repo-local gate 或空 DB smoke 宣称 RQA production-ready。
