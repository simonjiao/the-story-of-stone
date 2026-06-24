# 48 Gateway 总体架构与实时能力改造方案

## 0. 结论

本轮 Gateway 改造不接 OpenAI Realtime。Gateway 提供自有实时文字协议，
移动端、桌面端和 Web 端自行处理麦克风、ASR、TTS、VAD、播放打断和重新输入。
Gateway 只接收文字、控制事件和会话事件，并通过 Redis Streams 提供可靠的任务、
事件、状态和回放底座。

目标入口保留三层：

```text
/v1/chat/completions
  OpenAI-compatible 入口，继续服务 Open WebUI 和兼容客户端。

/v1/responses
  新主接口，支持 stream、background、status、cancel 和 replay。

/v1/realtime/ws
  Gateway 自有 WebSocket 实时文字协议，只传文字和控制事件，不传音频。
```

目标数据面统一为：

```text
Android / iOS / Desktop / Web
  端侧 ASR / TTS / VAD / 播放打断
  <-> tonglingyu-gateway /v1/realtime/ws
        |
        v
  Redis Streams jobs / response events / state
        |
        v
  RQA Runtime workflow
        |
        v
  evidence package / reviewer / final response / audit
```

`/v1/chat/completions` 的 SSE、`/v1/responses` 的 SSE 和
`/v1/realtime/ws` 的 server event 必须读取同一条 response event stream，不能各自
拼装一套返回格式。

## 1. 当前仓库基线

当前仓库已经具备以下基线：

1. `agent-platform/crates/tonglingyu-gateway/` 是 Rust Gateway 主实现。
2. `main.rs` 已有 `/healthz`、`/v1/models`、`/v1/chat/completions`、证据包、
   admin trace、metrics、memory、quality、governance 等路由。
3. `response.rs` 已能把 completion value 和 Runtime workflow stream events 包装为
   `text/event-stream`。
4. `tonglingyu-runtime` 已能执行 RQA workflow，生成 evidence package、reviewer
   结果、final answer 和 `RuntimeWorkflowStreamEvent`。
5. SQLite 仍是证据包、审计、context、memory、runtime governance 的正式持久化
   边界。

当前缺口是：

1. 没有 `/v1/responses` 主接口。
2. 没有 WebSocket 实时入口。
3. 没有 Redis Streams 任务和事件底座。
4. SSE 目前主要是把已完成 workflow 的 events 渲染成 SSE，不能作为断线可恢复、后台
   可继续、worker 可重试的统一事件源。
5. response state、event replay、job retry、cancel 和 resume 还不是一等抽象。

因此本改造首先抽象内部 `ResponseEvent`，再落 Redis Streams，最后让 SSE、WS 和
chat compatible 层全部复用该事件源。

## 2. 目标职责边界

### 2.1 端侧职责

端侧必须负责：

1. 麦克风采集。
2. 端侧或端侧调用服务完成 ASR。
3. VAD 和分段提交。
4. TTS 播放。
5. 播放中断、停止播放和重新输入。
6. 本地输入缓存和 UI 层的 token 展示。

端侧可以连续发送 `input_text.delta`，但 Gateway 只在 `input_text.commit` 或
`response.create` 后创建可执行 RQA response。

### 2.2 Gateway 职责

Gateway 必须负责：

1. 外部协议适配：OpenAI-compatible chat、Responses API、WebSocket realtime。
2. 鉴权、限流、模型隐藏和 forbidden control field gate。
3. 会话映射、trace、response id、幂等键和请求归一化。
4. 创建 response job，写入 Redis Streams。
5. 消费 job 或调用 worker 执行 RQA workflow。
6. 把 workflow 进度、证据状态、文本增量和完成状态写成 `ResponseEvent`。
7. SSE 与 WS 从 response event stream 读取并转发公开事件。
8. 取消、恢复、断线 replay 和后台任务状态查询。
9. evidence package、reviewer、context、memory、audit 与 admin trace 的索引。
10. 公开响应安全过滤，防止内部 trace、package、context、memory、LLM 原始输出泄露。

Gateway 不能负责：

1. 音频收发、音频转码、音频缓冲或音频持久化。
2. ASR、TTS、VAD。
3. 把用户 prompt 当成内部控制字段。
4. 绕过 evidence package 或 reviewer。
5. 让 WS 客户端直接指定内部 profile、tool policy、Runtime Adapter 或 reviewer 开关。

## 3. 总体模块结构

新增能力不要继续堆进现有 `main.rs`。建议按以下模块拆分：

<!-- markdownlint-disable MD013 -->
| 模块 | crate | 职责 | 禁止承担 |
| --- | --- | --- | --- |
| `response_events.rs` | `tonglingyu-gateway` | `ResponseEvent`、`ResponseState`、事件类型、公开/管理员可见性、schema 校验 | Redis I/O、HTTP handler、Runtime 领域逻辑 |
| `response_store.rs` | `tonglingyu-gateway` | `ResponseEventStore` trait、Redis Streams 实现、状态原子更新、事件 replay | RQA workflow 执行、WebSocket frame 处理 |
| `response_jobs.rs` | `tonglingyu-gateway` | job schema、consumer group、retry、lease、取消检查、worker loop | 用户协议 envelope、音频处理 |
| `responses_api.rs` | `tonglingyu-gateway` | `/v1/responses` create/status/cancel/events handler | 证据包构造、LLM/provider 调用 |
| `realtime_ws.rs` | `tonglingyu-gateway` | `/v1/realtime/ws` handshake、client event 校验、session buffer、server event fan-out | RQA 领域判断、音频处理 |
| `sse.rs` | `tonglingyu-gateway` | 从 `ResponseEventStore` 输出 SSE，供 chat/responses 复用 | 拼装内部 admin payload |
| `workflow_runner.rs` | `tonglingyu-gateway` | 复用当前 chat path 的 normalization/context/runtime workflow，并将阶段进度写成事件 | 外部协议解析 |
| `response_security.rs` | `tonglingyu-gateway` | public event/response 递归过滤，复用现有 public response 负向检查 | admin trace 展示 |
<!-- markdownlint-enable MD013 -->

`main.rs` 只允许保留 CLI、配置解析、AppState wiring 和 route registration。

## 4. ResponseEvent 内部合同

### 4.1 事件 envelope

所有对外流式输出、WS server event、status 查询和 replay 都以
`ResponseEvent` 为唯一事实源。

```json
{
  "schema_version": "tonglingyu.response_event.v1",
  "event_id": "evt_019...",
  "response_id": "resp_019...",
  "session_id": "session_019...",
  "trace_id": "tly-019...",
  "sequence": 12,
  "type": "output_text.delta",
  "visibility": "public",
  "created_at": "2026-06-24T00:00:00Z",
  "payload": {
    "text": "通灵宝玉相关情节需要区分..."
  }
}
```

字段要求：

1. `schema_version` 固定为 `tonglingyu.response_event.v1`，版本升级必须兼容旧 replay。
2. `event_id` 是全局唯一 ID，可用 UUIDv7。
3. `response_id` 是 response 主键，贯穿 HTTP、SSE、WS、Redis 和 admin trace。
4. `sequence` 在单个 response 内严格递增。
5. `type` 必须来自白名单。
6. `visibility=public` 的 payload 允许进入 SSE/WS/普通用户 status。
7. `visibility=admin` 只允许进入 admin trace，不允许进入公开 stream。
8. `payload` 不得包含 raw prompt、raw memory、tool policy 明文、provider 原始响应、
   context projection 原文或内部 profile 私有字段。

### 4.2 公共事件类型

MVP 必须支持以下公共事件：

<!-- markdownlint-disable MD013 -->
| type | 方向 | 语义 | payload |
| --- | --- | --- | --- |
| `response.created` | server | response 已创建并入队 | `response_id`、`session_id`、`status=queued` |
| `response.status` | server | 状态变化 | `status`、`reason?` |
| `evidence.searching` | server | 开始检索 | `required_evidence_types` 只允许公开枚举，不暴露内部 query payload |
| `evidence.found` | server | 找到证据候选或证据包完成 | `count`、`evidence_types`、`package_id?` |
| `review.started` | server | reviewer 开始 | `package_id?` |
| `review.completed` | server | reviewer 完成 | `status`，不暴露详细内部 issue payload |
| `output_text.delta` | server | 文本增量 | `text` |
| `output_text.done` | server | 文本输出结束 | 空对象或 `char_count` |
| `response.completed` | server | response 完成 | `response_id`、`status=completed` |
| `response.failed` | server | response 失败 | `error.code`、`error.message`、`trace_id?` |
| `response.canceled` | server | response 被取消 | `reason` |
<!-- markdownlint-enable MD013 -->

### 4.3 管理员事件类型

管理员事件只进入 admin trace 或 release report，不进入公开 SSE/WS：

1. `runtime.plan.created`。
2. `runtime.profile.started`。
3. `runtime.profile.completed`。
4. `runtime.tool.summary`。
5. `context.pack.created`。
6. `context.projection.created`。
7. `audit.linked`。
8. `dedupe.hit`。
9. `worker.retry_scheduled`。
10. `worker.dead_lettered`。

管理员事件可带 `context_pack_id`、`projection_digest`、`tool_policy_digest`、
`output_ref` 等内部 ref，但必须继续遵守既有 admin auth 与 admin key 隔离。

## 5. Redis Streams 设计

### 5.1 Redis key

生产必须启用 Redis。新增 key 命名如下：

```text
tonglingyu:jobs
  待执行 response job stream。

tonglingyu:jobs:dead
  多次失败后的 dead letter stream。

tonglingyu:response:{response_id}:events
  单个 response 的事件流，SSE 和 WS 共同读取。

tonglingyu:response:{response_id}:state
  单个 response 的状态 hash。

tonglingyu:session:{session_id}:responses
  session 下 response_id 索引，便于恢复最近会话。

tonglingyu:idempotency:{idempotency_key}
  幂等键到 response_id 的映射。

tonglingyu:lease:{response_id}
  worker lease，用于取消、重试和故障恢复。
```

可选 key：

```text
tonglingyu:response:{response_id}:cancel
  取消标志。也可以合并进 state hash。

tonglingyu:response:{response_id}:subscribers
  调试用，不作为可靠性依赖。
```

### 5.2 state hash 字段

`response:{id}:state` 至少包含：

<!-- markdownlint-disable MD013 -->
| 字段 | 说明 |
| --- | --- |
| `response_id` | response 主键 |
| `session_id` | 内部 session id |
| `trace_id` | Gateway trace id |
| `status` | `queued/in_progress/retrieving/composing/reviewing/completed/failed/canceled` |
| `created_at` | 创建时间 |
| `updated_at` | 最近状态更新时间 |
| `completed_at` | 完成时间，可空 |
| `sequence` | 最新事件 sequence |
| `last_event_id` | 最新 Redis stream id 或 event id |
| `package_id` | evidence package id，可空 |
| `final_response_ref` | SQLite final response journal ref，可空 |
| `cancel_requested` | `true/false` |
| `worker_id` | 当前 worker，可空 |
| `attempt` | job attempt 次数 |
| `error_code` | 失败码，可空 |
| `public_output_char_count` | 输出字数统计 |
<!-- markdownlint-enable MD013 -->

### 5.3 原子写入要求

事件追加和 state 更新必须是原子的。推荐用 Redis Lua 脚本完成：

1. 读取并递增 `state.sequence`。
2. 写入 `ResponseEvent` 到 `response:{id}:events`。
3. 更新 `state.status`、`state.updated_at`、`last_event_id`、`package_id` 等字段。
4. 对 events stream 执行 `XTRIM MAXLEN ~ N`。
5. 返回新 sequence 和 Redis stream id。

禁止先写 state 再写 event，或先写 event 再异步补 state。否则断线恢复时会出现
status 与 replay 事件不一致。

### 5.4 Job consumer group

`tonglingyu:jobs` 使用 consumer group：

```text
group: tonglingyu-gateway-workers
consumer: {hostname}:{pid}:{uuid}
```

worker 行为：

1. `XREADGROUP` 领取 job。
2. 设置 `lease:{response_id}`，写入 `worker_id` 和 heartbeat。
3. 将 response state 从 `queued` 迁移为 `in_progress`。
4. 执行 context governance、plan gate、RQA workflow。
5. 在每个阶段检查 `cancel_requested`。
6. 将 workflow 事件映射为 `ResponseEvent`。
7. 完成后写 `response.completed`，`XACK` job。
8. 失败时按错误类别决定 retry、failed 或 dead letter。

retry 规则：

1. 鉴权、模型越权、请求 schema 错误不 retry。
2. Redis 写入失败必须 fail-closed，不能伪造成功 response。
3. Runtime transient error 可按指数退避 retry。
4. reviewer 不通过不是 worker failure，应返回受控回答或 evidence 不足说明。
5. 达到最大 attempt 后写 `response.failed` 和 dead letter。

### 5.5 Redis 与 SQLite 分工

Redis 负责在线任务和事件：

1. job queue。
2. response state。
3. response event replay。
4. WS/SSE 共享事件源。
5. worker retry 和 cancel flag。

SQLite 继续负责：

1. runtime domain data。
2. evidence cards/packages/review records。
3. audit events。
4. context pack/projection/journal/memory。
5. final response journal 和 dedupe。
6. governance 和 quality 状态。

Redis event 不能替代 SQLite evidence package，也不能成为事实证据源。

## 6. `/v1/responses` 接口设计

### 6.1 创建 response

路径：

```text
POST /v1/responses
```

请求：

```json
{
  "model": "tonglingyu",
  "session_id": "session_019...",
  "input": [
    {
      "role": "user",
      "content": [
        {"type": "input_text", "text": "宝玉的玉丢过几次？"}
      ]
    }
  ],
  "stream": false,
  "background": false,
  "idempotency_key": "openwebui-message-id-or-client-id",
  "metadata": {
    "client": "desktop"
  }
}
```

响应，非 streaming 同步模式：

```json
{
  "id": "resp_019...",
  "object": "response",
  "status": "completed",
  "model": "tonglingyu",
  "output": [
    {
      "type": "output_text",
      "text": "通灵宝玉相关情节需要区分..."
    }
  ],
  "evidence_package_id": "pkg-019..."
}
```

响应，`background=true`：

```json
{
  "id": "resp_019...",
  "object": "response",
  "status": "queued",
  "events_url": "/v1/responses/resp_019.../events"
}
```

响应，`stream=true`：

```text
Content-Type: text/event-stream; charset=utf-8

data: {"type":"response.created","response_id":"resp_019..."}

data: {"type":"response.status","status":"retrieving"}

data: {"type":"output_text.delta","text":"通灵宝玉"}

data: {"type":"response.completed","response_id":"resp_019..."}

data: [DONE]
```

### 6.2 查询状态

路径：

```text
GET /v1/responses/{response_id}
```

语义：

1. 返回 response state 和公开 final output。
2. 未完成时返回当前 status、已公开 event count 和 `events_url`。
3. 普通用户只能查询自己 session 下的 response。
4. 管理员可通过 admin trace 查询内部事件。

### 6.3 订阅事件

路径：

```text
GET /v1/responses/{response_id}/events?after={event_id}
```

语义：

1. 返回 SSE。
2. `after` 缺省时从头 replay。
3. `after` 存在时从指定事件之后继续。
4. response 已完成时 replay 历史事件后发送 `[DONE]`。
5. response 未完成时 replay 后继续阻塞读取 Redis stream。

### 6.4 取消 response

路径：

```text
POST /v1/responses/{response_id}/cancel
```

语义：

1. 将 state 标记为 `cancel_requested=true`。
2. 写入 `response.status`，`status=canceling`。
3. worker 在安全点停止后写 `response.canceled`。
4. 若 response 已完成，返回当前 completed state，不回滚证据包和 final response。
5. 若 Runtime 当前 step 不支持硬取消，Gateway 必须在 step 返回后丢弃未公开 final output，
   并返回 canceled。

## 7. `/v1/chat/completions` 改造

### 7.1 保持兼容

`/v1/chat/completions` 对 Open WebUI 继续保持兼容：

1. `/v1/models` 仍只返回 `tonglingyu`。
2. `model` 仍必须等于 visible model。
3. 用户不能指定内部 profile、tool policy、reviewer 或 context 字段。
4. 非 streaming 返回 OpenAI-compatible `chat.completion`。
5. streaming 返回 OpenAI-compatible `chat.completion.chunk` SSE。

### 7.2 新执行路径

chat 请求进入后不要直接执行完整 workflow 再渲染 SSE，而应桥接到 response
执行路径：

```text
chat_completions handler
  -> normalize existing ChatCompletionRequest
  -> create ResponseCreateInput
  -> create response job/state/events in Redis
  -> if stream=true:
       return SSE adapter over response event stream
     else:
       wait until completed/failed/canceled or timeout
       return OpenAI-compatible completion
```

SSE adapter 负责把 `ResponseEvent` 转成 OpenAI chunk：

```text
response.created      -> role chunk 或 comment event
response.status       -> 可选 metadata，不进入 content delta
output_text.delta     -> choices[0].delta.content
response.completed    -> finish_reason=stop + [DONE]
response.failed       -> error chunk + [DONE]
response.canceled     -> finish_reason=stop + [DONE]
```

chat SSE 不允许输出 evidence package、trace、review、context 和 memory 内部字段。

### 7.3 幂等与去重

现有 chat path 已按 external message id 做 dedupe。改造后：

1. `idempotency_key` 优先来自请求 metadata/header 中的 message id。
2. 命中已有 final response 时可直接返回 SQLite final response journal。
3. 命中未完成 response 时返回已有 `response_id`，stream 模式接入已有 event stream。
4. 去重命中必须写 admin-only `dedupe.hit` 事件。

## 8. WebSocket 实时文字协议

### 8.1 连接

路径：

```text
GET /v1/realtime/ws
```

鉴权：

1. 复用 Gateway API key 或受控用户 token。
2. handshake 阶段执行 auth 和 rate limit。
3. 未授权时返回 HTTP 401，不升级协议。
4. 已升级后发现协议错误，发送 `error` event，然后按 WebSocket close code 关闭。

连接参数：

```text
/v1/realtime/ws?session_id=session_019...&last_event_id=evt_019...
```

`session_id` 可省略。省略时 Gateway 创建新的内部 session，并在
`session.started` 中返回。

### 8.2 Client event envelope

客户端发 JSON 文本帧：

```json
{
  "schema_version": "tonglingyu.realtime.client_event.v1",
  "event_id": "cli_evt_019...",
  "type": "input_text.commit",
  "session_id": "session_019...",
  "response_id": "resp_019...",
  "sequence": 3,
  "payload": {
    "text": "宝玉的玉丢过几次？"
  }
}
```

必须拒绝：

1. binary frame。
2. `input_audio.*`。
3. `output_audio.*`。
4. 任意尝试指定内部 profile、tool、reviewer、context、memory、trace 的字段。
5. 超过限长的 `input_text.delta` 或 `input_text.commit`。
6. sequence 回退或 event_id 重复。

### 8.3 Client event 类型

MVP 支持：

<!-- markdownlint-disable MD013 -->
| type | 语义 |
| --- | --- |
| `session.start` | 初始化或恢复 session。 |
| `input_text.delta` | 端侧 ASR 的中间文字。Gateway 只用于 session buffer，不创建 RQA。 |
| `input_text.commit` | 提交最终文字片段。可只更新 buffer，也可配合 `response.create` 创建回答。 |
| `response.create` | 基于已 commit 文本或 payload.text 创建 response。 |
| `response.cancel` | 取消指定 response。 |
| `response.resume` | 从 `last_event_id` 后 replay 指定 response。 |
| `ping` | 保活。 |
| `session.close` | 端侧主动关闭。 |
<!-- markdownlint-enable MD013 -->

推荐一次语音提交流程：

```json
{"type":"session.start","session_id":"session_019..."}
{"type":"input_text.delta","payload":{"text":"宝玉的玉"}}
{"type":"input_text.commit","payload":{"text":"宝玉的玉丢过几次？"}}
{"type":"response.create","response_id":"resp_019..."}
```

### 8.4 Server event envelope

服务端回 JSON 文本帧：

```json
{
  "schema_version": "tonglingyu.realtime.server_event.v1",
  "event_id": "evt_019...",
  "type": "output_text.delta",
  "session_id": "session_019...",
  "response_id": "resp_019...",
  "sequence": 12,
  "payload": {
    "text": "通灵宝玉相关情节需要区分..."
  }
}
```

server event 直接由 `ResponseEvent` 投影得到。WS handler 不允许重新生成或改写
RQA 内容，只负责协议投影。

### 8.5 Session buffer

WS session buffer 用于收集端侧实时 ASR 文本：

1. `input_text.delta` 更新临时 buffer，默认不写 Redis response events。
2. `input_text.commit` 写 session-level input journal，可写 Redis session stream 或 SQLite
   session journal 摘要。
3. `response.create` 读取最近 commit 文本并创建 response job。
4. buffer 限长由 `TONGLINGYU_REALTIME_MAX_BUFFER_CHARS` 控制。
5. 断线后，未 commit 的 delta 不保证恢复；已 commit 的 input 可恢复。

### 8.6 打断和取消

端侧播放 TTS 时，用户再次说话或点击停止：

```json
{"type":"response.cancel","response_id":"resp_019..."}
{"type":"input_text.commit","payload":{"text":"换个问法，宝玉的玉第一次怎么来的？"}}
{"type":"response.create","response_id":"resp_019..."}
```

Gateway 行为：

1. 对旧 response 写 `cancel_requested=true`。
2. WS 广播 `response.status status=canceling`。
3. 新 response 独立创建，不复用旧 response_id。
4. 旧 response 的后台 worker 到安全点后写 `response.canceled`。
5. 若旧 response 已 completed，cancel 返回 no-op completed，不删除事件。

## 9. SSE 与 WS 分工

SSE 适合：

1. Open WebUI。
2. 普通 HTTP 客户端。
3. 一问一答。
4. 只需要服务端到客户端流式输出的场景。

WS 适合：

1. 端侧连续会话。
2. ASR 增量文字提交。
3. 用户打断、取消、重新输入。
4. 多 response 状态同步。
5. 弱网断线恢复。

统一要求：

1. SSE 和 WS 都从 `response:{id}:events` 读取。
2. SSE 和 WS 都只能暴露 `visibility=public` 的事件。
3. SSE 和 WS 的 replay 语义一致。
4. SSE 和 WS 的错误码、取消语义、completed 语义一致。
5. SSE 和 WS 都必须复用公开输出安全 scanner。

## 10. Runtime workflow 改造点

当前 `tonglingyu-runtime` 返回 `RuntimeWorkflowOutput`，其中包含
`stream_events`。为了支持真正在线事件，需要新增 event sink 形式。

建议新增内部 trait：

```rust
#[async_trait::async_trait]
pub trait RuntimeWorkflowEventSink: Send + Sync {
    async fn emit(&self, event: RuntimeWorkflowStreamEvent) -> anyhow::Result<()>;
    async fn is_cancel_requested(&self) -> anyhow::Result<bool>;
}
```

新增执行函数：

```rust
execute_workflow_with_agent_runtime_client_and_event_sink(
    input: RuntimeWorkflowInput,
    mode: TonglingyuAgentRuntimeMode,
    runtime: Arc<dyn RuntimeClient>,
    sink: Arc<dyn RuntimeWorkflowEventSink>,
) -> Result<RuntimeWorkflowOutput>
```

最低落地方式：

1. 每完成一个 Runtime step，立即 `sink.emit(step_completed)`。
2. evidence package 创建后立即 `sink.emit(evidence_package_created)`。
3. reviewer 开始和完成时立即 emit。
4. final answer 生成后按 chunk emit `content_delta`。
5. 每个 step 前后调用 `sink.is_cancel_requested()`。

进一步增强：

1. 当上游 LLM/provider 支持 streaming 时，provider adapter 可将 token delta 转成
   `RuntimeWorkflowStreamEvent`。
2. 本地 deterministic composer 不必伪造 token streaming，但必须实时写阶段 status。
3. Runtime event sink 失败时，workflow 必须 fail-closed，不能继续生成不可回放回答。

## 11. 状态机

Response 状态机：

```text
queued
  -> in_progress
  -> retrieving
  -> composing
  -> reviewing
  -> completed

queued / in_progress / retrieving / composing / reviewing
  -> canceling
  -> canceled

queued / in_progress / retrieving / composing / reviewing
  -> failed
```

规则：

1. `completed`、`failed`、`canceled` 是终态。
2. 终态不能回退。
3. `canceling` 只允许进入 `canceled`、`completed` 或 `failed`；若 step 已不可取消并已
   完成，可进入 `completed`，但必须记录 `cancel_race_lost` admin event。
4. `failed` 必须有公开错误码和 admin-only 详细错误。
5. status transition 必须由 `response_store` 校验，不能由 handler 随意写 hash。

## 12. 安全与权限

### 12.1 公共字段白名单

公开 SSE/WS/status 只能包含：

1. `response_id`。
2. `session_id`。
3. `status`。
4. `text`。
5. `count`。
6. 公开 evidence type 枚举。
7. `package_id`，仅当现有 owner-only package 读取策略允许时。
8. 脱敏错误码和错误消息。

禁止公开：

1. `trace_id`，除非现有公开错误口径允许短 trace id；默认不进入 WS/SSE。
2. `review` 完整对象。
3. `context_pack_id`、`context_projection_id`、`memory_*`。
4. `runtime_step_plan`、`agent_runtime_plan_gate`。
5. `tool_policy_digest`、`output_contract_digest`。
6. raw provider response、raw prompt、raw memory。
7. Redis stream id 以外的内部实现细节。

### 12.2 Forbidden control fields

`/v1/responses` 和 `/v1/realtime/ws` 必须复用 chat path 的 forbidden control
field gate。新增拒绝字段包括：

```text
profile
profiles
agent
agents
tool_policy
reviewer
skip_review
context_pack
context_projection
memory_card
memory_read_refs
runtime_adapter
trace_id
evidence_package_override
```

### 12.3 音频拒绝

Gateway 不处理音频。必须明确拒绝：

1. WebSocket binary frame。
2. JSON event type 以 `input_audio.`、`output_audio.`、`audio.` 开头。
3. payload 含 `audio`、`pcm`、`wav`、`opus`、`sample_rate`、`media` 等媒体字段。

拒绝响应：

```json
{
  "type": "error",
  "payload": {
    "error": {
      "code": "audio_not_supported",
      "message": "gateway realtime protocol accepts text and control events only"
    }
  }
}
```

## 13. 配置与部署

### 13.1 Cargo dependency

需要新增能力：

1. `axum` 启用 `ws` feature。
2. 引入 Redis async client，版本按 workspace 统一依赖策略选择。
3. 如果 SSE 使用 stream body，补齐所需 async stream 依赖。
4. 所有新增依赖必须放在 workspace dependencies 中统一管理。

示例方向，不绑定精确版本：

```toml
axum = { version = "0.8", features = ["macros", "json", "ws"] }
redis = { version = "...", features = ["tokio-comp", "connection-manager"] }
```

### 13.2 Gateway 环境变量

新增环境变量：

```text
TONGLINGYU_REDIS_URL=redis://redis:6379/0
TONGLINGYU_REDIS_REQUIRED=true
TONGLINGYU_RESPONSE_STREAM_PREFIX=tonglingyu
TONGLINGYU_RESPONSE_EVENT_MAXLEN=2000
TONGLINGYU_RESPONSE_EVENT_TTL_SECS=86400
TONGLINGYU_RESPONSE_JOB_GROUP=tonglingyu-gateway-workers
TONGLINGYU_RESPONSE_WORKER_CONCURRENCY=4
TONGLINGYU_RESPONSE_JOB_MAX_ATTEMPTS=3
TONGLINGYU_RESPONSE_JOB_LEASE_SECS=60
TONGLINGYU_RESPONSE_SYNC_TIMEOUT_SECS=120
TONGLINGYU_REALTIME_MAX_SESSION_SECS=3600
TONGLINGYU_REALTIME_MAX_BUFFER_CHARS=4000
TONGLINGYU_REALTIME_MAX_IN_FLIGHT_RESPONSES=2
TONGLINGYU_REALTIME_PING_INTERVAL_SECS=20
```

### 13.3 docker-compose

`deploy/docker-compose.yml` 需要新增 Redis 服务，并让 Gateway depends_on Redis
healthcheck：

```yaml
redis:
  image: redis:7-alpine
  command: ["redis-server", "--appendonly", "yes"]
  volumes:
    - tonglingyu-redis:/data
  healthcheck:
    test: ["CMD", "redis-cli", "ping"]
    interval: 10s
    timeout: 3s
    retries: 10
```

Gateway 配置：

```yaml
environment:
  TONGLINGYU_REDIS_URL: redis://redis:6379/0
  TONGLINGYU_REDIS_REQUIRED: "true"
depends_on:
  redis:
    condition: service_healthy
```

### 13.4 healthz 与 metrics

`/healthz` 必须增加：

1. Redis connectivity。
2. Redis required flag。
3. job stream group existence。
4. worker heartbeat summary。
5. pending job count。
6. dead letter count。
7. response event store status。

admin metrics 增加：

1. `response_jobs_queued`。
2. `response_jobs_in_progress`。
3. `response_jobs_failed_total`。
4. `response_jobs_dead_total`。
5. `response_events_written_total`。
6. `response_event_write_failures_total`。
7. `realtime_ws_connections`。
8. `realtime_ws_reconnects_total`。
9. `response_cancellations_total`。
10. `response_replay_requests_total`。

Prometheus 指标名称沿用现有 metrics 风格，禁止暴露用户文本、问题全文、证据全文和
密钥。

## 14. 实施顺序

### P0：合同冻结

交付物：

1. `response_events.rs`。
2. `ResponseEvent` schema 和事件白名单。
3. `ResponseState` 状态机。
4. 公开事件安全 scanner。
5. 单元测试：事件 JSON、未知 type 拒绝、admin-only 不进入 public projection。

验收：

1. 不接 Redis 也能跑 contract tests。
2. `ResponseEvent` 能从现有 `RuntimeWorkflowStreamEvent` 投影为公开事件。
3. public projection 不包含 trace/context/memory/review 内部字段。

### P1：Redis Streams event store

交付物：

1. `response_store.rs` trait。
2. Redis 实现。
3. Lua 原子 append + state update。
4. state transition 校验。
5. replay API。
6. healthz Redis 检查。

验收：

1. 创建 response state。
2. 连续 append 事件 sequence 严格递增。
3. `read_after` 能从任意事件后恢复。
4. Redis 断开时返回 typed error，不产生半成功 response。
5. state 与 event stream 不一致的 fixture 会 fail。

### P2：Job queue 和 worker

交付物：

1. `response_jobs.rs`。
2. `tonglingyu:jobs` consumer group。
3. worker lease、heartbeat、retry、dead letter。
4. cancel flag。
5. worker 将当前 RQA workflow 结果写入 response events。

验收：

1. job 入队后 worker 可执行当前 RQA workflow。
2. worker crash 后 pending job 可被新 worker reclaim。
3. cancel_requested 会在安全点停止。
4. 最大 retry 后进入 dead letter。
5. admin trace 可按 response_id 找到 trace/package/session。

### P3：`/v1/chat/completions stream=true` 真正 SSE

交付物：

1. chat handler 桥接 response job。
2. `sse.rs` 从 Redis event stream 输出 OpenAI chunk。
3. streaming dedupe 命中未完成 response 时接入已有 stream。
4. 非 stream chat 仍返回 OpenAI-compatible completion。

验收：

1. stream 请求在 workflow 完成前返回 SSE headers。
2. 状态事件可在 workflow 运行中到达客户端。
3. content delta 从 response events 转换为 OpenAI chunk。
4. `[DONE]` 必定出现。
5. public SSE 负向扫描覆盖内部字段。
6. Open WebUI smoke 不回归。

### P4：`/v1/responses`

交付物：

1. create/status/events/cancel handler。
2. background mode。
3. sync wait mode。
4. response replay。
5. owner-only access control。

验收：

1. `background=true` 立即返回 queued response。
2. `stream=true` 读取同一 response event stream。
3. `GET /v1/responses/{id}` 能返回未完成和已完成状态。
4. cancel 可取消未完成 response。
5. 普通用户不能读取他人 response。

### P5：`/v1/realtime/ws`

交付物：

1. WebSocket route。
2. handshake auth/rate limit。
3. client event envelope parser。
4. session buffer。
5. `response.create/cancel/resume`。
6. response event stream fan-out。
7. ping/pong 和 close handling。

验收：

1. binary/audio frame 被拒绝。
2. `input_text.delta` 不触发 RQA。
3. `input_text.commit + response.create` 创建 response。
4. WS 可收到 `response.created/status/output_text.delta/completed`。
5. 断线后用 `response.resume` 可从 last event 后继续。
6. 播放打断场景可取消旧 response 并创建新 response。

### P6：Runtime event sink 增强

交付物：

1. `RuntimeWorkflowEventSink`。
2. workflow step-level live emit。
3. provider streaming delta adapter，若上游支持。
4. cancel check 嵌入 Runtime 安全点。

验收：

1. `evidence.searching/evidence.found/review.started/review.completed` 实时出现。
2. final answer delta 可通过 event sink 输出。
3. event sink 写失败导致 workflow 受控失败，不产生不可 replay 回答。
4. 取消在长 workflow 中可观测。

### P7：发布与回归

交付物：

1. gateway smoke 增加 Redis、Responses、WS、cancel、resume。
2. strict live gate 增加真正 SSE 与 WS text protocol。
3. docs/runbook 更新 Redis backup、监控和故障处理。
4. release report 记录 Redis event store enabled、worker group、dead letter 为 0。

验收：

1. 原有 chat、admin、evidence、memory、quality smoke 全部通过。
2. Redis down 在 production required 模式下 fail-closed。
3. WS/SSE public events 不泄露内部字段。
4. 断线恢复和 cancel 有自动化测试。
5. 不接 OpenAI Realtime，不接音频。

## 15. 测试矩阵

### 15.1 单元测试

必须覆盖：

1. `ResponseEvent` schema roundtrip。
2. unknown event type 拒绝。
3. state transition 非法回退拒绝。
4. public projection 移除 admin-only 字段。
5. audio event 和 binary payload 拒绝。
6. Redis append Lua 脚本 sequence 递增。
7. `RuntimeWorkflowStreamEvent -> ResponseEvent` 映射。
8. chat chunk adapter 输出 OpenAI-compatible JSON。

### 15.2 集成测试

必须覆盖：

1. create response -> worker -> completed。
2. create response -> SSE replay from beginning。
3. create response -> SSE after event id。
4. background response status 查询。
5. cancel before worker start。
6. cancel during workflow。
7. worker crash reclaim pending job。
8. dead letter。
9. WS session start。
10. WS input delta/commit/create。
11. WS reconnect resume。
12. WS cancel old response and create new response。

### 15.3 负向测试

必须覆盖：

1. 用户指定 `profile` 被拒。
2. 用户指定 `skip_review` 被拒。
3. WS binary frame 被拒。
4. WS `input_audio.delta` 被拒。
5. 未授权读取他人 response 被拒。
6. Redis 写入失败时不返回 completed。
7. public SSE/WS 不包含 `context_pack_id`、`memory_card_id`、`tool_policy_digest`。
8. duplicate idempotency key 不创建第二个 response。

## 16. 端侧接入建议

端侧状态机：

```text
idle
  -> listening
  -> transcribing
  -> committed
  -> response_running
  -> speaking
  -> interrupted
  -> idle
```

端侧建议：

1. ASR interim 发送 `input_text.delta`，final 发送 `input_text.commit`。
2. VAD end 后自动 `response.create`。
3. TTS 播放新回答时记录 `response_id`。
4. 用户打断时先本地停止 TTS，再发 `response.cancel`。
5. 网络断线时保存最近 `response_id` 和 `last_event_id`。
6. 重连后发 `response.resume`。
7. 端侧不要发送音频给 Gateway。
8. 端侧不要依赖 Redis stream id；只依赖 Gateway server event id。

## 17. 回滚方案

分阶段回滚：

1. P0/P1 只新增模块，未接生产入口，可直接关闭配置。
2. P2 worker 可通过 `TONGLINGYU_RESPONSE_WORKER_ENABLED=false` 停止。
3. P3 chat bridge 必须保留 legacy direct workflow 开关，仅用于紧急回滚。
4. P4 `/v1/responses` 可由路由层关闭。
5. P5 `/v1/realtime/ws` 可由 feature flag 关闭。
6. Production-ready 声明必须要求 Redis required 模式通过；legacy direct workflow 不能作为
   新架构 production-ready 证据。

建议开关：

```text
TONGLINGYU_RESPONSES_ENABLED=true
TONGLINGYU_REALTIME_WS_ENABLED=true
TONGLINGYU_CHAT_USE_RESPONSE_PIPELINE=true
TONGLINGYU_RESPONSE_WORKER_ENABLED=true
TONGLINGYU_LEGACY_CHAT_FALLBACK_ENABLED=false
```

## 18. 最终能力说明

改造完成后，Gateway 对外能力应描述为：

1. OpenAI-compatible chat gateway：兼容 Open WebUI 和普通 OpenAI-compatible 客户端。
2. Responses gateway：支持同步、流式、后台、状态查询、取消和事件 replay。
3. Realtime text gateway：支持 WebSocket 连续文字会话、ASR 增量文字提交、取消、恢复和
   状态同步。
4. Reliable response event substrate：Redis Streams 提供 response event、job、state、retry
   和 replay。
5. RQA governance gateway：强制 evidence package、reviewer、trace、context projection、
   public response safety 和 admin audit。
6. Audio-free boundary：Gateway 不处理音频，语音媒体能力留在端侧。

一句话口径：

> Gateway 不接 OpenAI Realtime，也不处理音频；它提供自有 WebSocket 实时文字协议和
> `/v1/responses` 主接口，以 Redis Streams 作为可靠 response 事件底座，并继续强制
> 通灵玉 RQA 的证据包、reviewer、审计和公开输出安全边界。
