# 09 Agent Runtime 专项设计

## 文档定位

本文专门收敛 Agent Runtime 的已实现边界、缺口和后续实施顺序。
它不替代现有设计文档，而是在以下文档之上补齐 Runtime 专项视角：

1. [01-design-principles.md](01-design-principles.md)：Runtime 只执行已授权
   session/run，不决定权限，不绕过 Manager。
2. [04-internal-definition.md](04-internal-definition.md)：`agent_instance`、
   `agent_session`、`agent_run`、credential 和 workdir 的隔离规则。
3. [05-technical-implementation.md](05-technical-implementation.md)：Rust crate、
   `RuntimeClient`、`ConnectorClient`、`MemoryStore`、`RunQueue` 和 adapter 边界。
4. [08-implementation-roadmap.md](08-implementation-roadmap.md)：P0/P1/P2 阶段边界。
5. [P1_IMPLEMENTATION_CHECKLIST.md](P1_IMPLEMENTATION_CHECKLIST.md)：真实
   Hermes Runtime adapter、session/run path 和只读 connector 的完成记录。
6. [P2_IMPLEMENTATION_CHECKLIST.md](P2_IMPLEMENTATION_CHECKLIST.md)：external-action
   provider/connector、apply/compensate 和默认关闭真实写入的完成记录。
7. [AGENT_RUNTIME_IMPLEMENTATION_CHECKLIST.md](AGENT_RUNTIME_IMPLEMENTATION_CHECKLIST.md)：
   Runtime 专项的实施 checklist、验收、测试和待确认项。

本文只处理 Runtime 执行面完善，Runtime 本体范围限定为 `agent-core` 中的
通用 contract / trait / model，以及 `agent-runtime` 中的 adapter、streaming、
schema、tool loop 和 step plan 执行器。不改变 Manager 授权、Open WebUI
Bridge、run/session 状态机、Worker claim、Memory schema 或 audit contract。
领域 Gateway 的接入设计不放在本文。

## 当前实现基线

当前仓库已具备以下 Runtime 能力：

1. `agent-core` 定义 `RuntimeClient`、`RuntimeRunInput`、
   `RuntimeSessionInput` 和 `RuntimeOutput`。
2. `agent-runtime` 实现 `MinimalRuntimeClient` 和 `HermesRuntimeClient`。
3. `HermesRuntimeClient` 通过 OpenAI-compatible `/chat/completions` 调用 Hermes。
4. Runtime 支持 `AGENT_RUNTIME_MODE=minimal|hermes`。
5. Runtime 支持 `AGENT_RUNTIME_HERMES_PROFILE_MODELS`，可按
   `agent.hermes_profile` 路由实际 Hermes model。
6. Runtime 调用携带 `x-agent-trace-id`，metadata 记录 runtime、profile、model、
   trace 和 duration。
7. Worker claim run 后调用 `RuntimeClient::execute_run` 或
   `RuntimeClient::send_session_message`。
8. P1 Runtime 只读，不持有写 credential，不执行 external action。
9. P2 external-action 通过 Manager apply/compensate、CredentialProvider 和
   WriteConnector 完成；Runtime / Worker 仍不能绕过 Manager 写外部目标。

因此当前状态是：

```text
Manager 创建和授权 session/run
  -> Worker claim / heartbeat / retry / dead-letter
  -> RuntimeClient 执行一次 run 或 session message
  -> Worker 写回 result / assistant message / audit
```

这已经满足 P1 的真实 Hermes Runtime 只读闭环。Runtime 完善专项在该
基线之上补齐 profile contract、streaming event、工具权限、step plan 和
read-only tool loop；完整 JSON Schema 和领域 Gateway 接入不纳入 Runtime
本体完成口径。

## 缺口和当前处置

以下能力不能用 P2 external-action 完成状态替代。当前 repo/local 已补齐
Runtime 执行面要求，完整 JSON Schema 和领域 Gateway 接入仍不纳入本体
完成口径：

1. Runtime streaming 输出：R2 已补齐 `started` / `delta` / `final` /
   `error` 基础事件、`tool_progress`、`schema_partial` 和 final output
   语义；streaming 路径失败时返回安全 `error` event，不把 upstream
   response、prompt 或内部错误细节写入 event。
2. 结构化 schema 输出校验：R1 已补齐 profile input/output contract 和轻量
   JSON Schema 子集校验；完整 JSON Schema 不是当前完成口径。
3. per-profile tool permission：R3 已补齐 profile 工具策略、`requested_tools`
   输入契约、Runtime 内交集计算、read-only capability 校验和执行前约束；
   Manager/Worker 如何生成本次授权 scope 是调用方集成边界。
4. 多 profile 编排：R4 已补齐 `RuntimeStepPlan` / `RuntimeStep` 数据模型、
   step 级 `output_contract` / `tool_policy`、完整 step plan helper、多 step
   executor、依赖校验、fallback policy 和跨 step output_ref 流转。
5. 领域 profile 强类型契约：Runtime 已提供 profile contract 机制；
   具体领域 contract 在领域设计文档中定义和复核。
6. Runtime tool execution loop：R4.5 已补齐 read-only tool call、
   tool input/output schema、预算、metadata 摘要、成功/失败 tool event
   metadata 和 Runtime adapter 直连 JSONL audit sink；Worker 是否把这些
   events 追加到 `audit_logs` 是调用方集成边界。具体领域 Gateway 仍需
   显式配置并复测。

## 完善目标

Runtime 完善专项的目标不是让 Runtime 变成控制面，而是把执行面从
“一次文本调用”提升为“可审计、可校验、可组合的受控 profile 执行层”。

完成后应满足：

1. Runtime 可以返回非流式和流式两类输出，并保留最终 `RuntimeOutput`
   语义；Worker/Orchestrator 如何消费这些输出不属于 Runtime 本体实现。
2. 每个 profile 可以声明输入 schema、输出 schema、允许工具、禁止工具和
   安全策略；当前 Runtime 本体执行确定性安全策略子集，不做 LLM 安全判断。
3. Runtime 对 profile 输出做 schema 校验，失败时返回安全错误，不泄露 prompt、
   credential、connector payload 或内部栈。
4. 多 profile 编排必须由 Manager、Orchestrator 或领域 Gateway 明确授权和记录；
   Runtime 只能执行已授权 step，不能自行扩权或创建新 step。
5. 领域 profile 可以在不破坏通用 Agent Platform contract 的前提下注册 typed
   contract；具体字段、领域工具和验收条件由领域设计文档维护。

## 非目标

Runtime 完善专项不做以下事情：

1. 不让 Open WebUI 直连 Agent Runtime。
2. 不让 Runtime 决定用户权限、审批、资源锁或 credential scope。
3. 不让 Runtime 持有长期 credential 或明文 secret。
4. 不把 external-action apply/compensate 从 Manager 下放给 Runtime。
5. 不把所有领域知识库迁入 Agent Platform core；领域知识库可以作为
   Runtime profile 的 read-only tool/service 存在。
6. 不要求所有领域 agent 都必须使用多 profile；简单 agent 仍可只用一个 profile。

## 目标架构

Runtime 专项完成后的调用边界如下：

```text
Open WebUI / External Client
  -> Orchestrator / Domain Gateway
      -> protocol / auth / routing / trace / SSE / model hiding
  -> Manager 授权 session/run 或 domain gateway 内部授权 step plan
  -> Worker 或 Gateway runtime client
  -> Runtime Adapter
      -> profile contract registry
      -> schema validation
      -> allowed tool snapshot
      -> read-only tool executor
      -> Hermes / other runtime transport
  -> Domain read-only tools
      -> domain data / search index / evidence refs / replay data
  -> RuntimeOutput / RuntimeStreamEvent
  -> audit / memory / evidence refs / final response
```

如果由 Worker 执行，必须沿用现有 run/session 状态机。
如果由领域 Gateway 直接调用 Runtime adapter，必须满足通用边界：

1. Gateway 仍是唯一公开入口。
2. Gateway 只做协议适配、鉴权、限流、路由、trace/session 透传、
   OpenAI-compatible 响应封装和 SSE 转发。
3. Gateway 不把内部 profile 暴露为外部可见模型。
4. Gateway 记录或透传 trace、profile step、schema version、tool policy、
   evidence/package ref 和 result。
5. Gateway 不能通过提示词或请求字段给 Runtime 扩权。
6. 领域数据、领域工具和领域 replay 规则在领域设计文档中定义，不进入
   Agent Platform core。

## 统一实施计划

Runtime 专项按 R0 到 R4.5 推进。每个节点必须在同一处说明设计目标、
代码范围、验收和提交口径，避免在 roadmap、checklist 和专项文档之间
形成重复但不一致的计划。

### R0：现状锁定和口径统一

目标：关闭设计口径分歧，明确 P1/P2 已完成内容和 Runtime 未完成内容。

当前结论：

1. P1 已完成真实 Hermes Runtime 只读闭环。
2. P2 已完成 external-action provider/connector、apply/compensate 和
   contract smoke 路径。
3. P2 不包含 Runtime streaming、profile schema、per-profile tool permission、
   multi-profile step plan 或 read-only tool execution loop。
4. P0/P1 已有的是 Orchestrator / OpenAI-compatible SSE，不是 Runtime 原生
   streaming contract。

文档范围：

1. 本文作为 Runtime 专项权威入口。
2. `08-implementation-roadmap.md` 只保留阶段索引和状态口径。
3. `PROGRESS.md` 只记录当前现实状态和下一步。
4. `AGENT_RUNTIME_IMPLEMENTATION_CHECKLIST.md` 只记录可执行任务、验收、
   测试和待确认项。

验收：

1. `09-agent-runtime-design.md` 通过 markdown lint。
2. `git diff --check` 通过。
3. 文档明确 P2 external-action 不等于 Runtime 完整化。

提交建议：

```text
docs: consolidate agent runtime implementation plan
```

### R1：Profile Contract 和 Schema Validation

目标：Runtime 不再只是按 profile 名称调用模型，而是先校验 profile 输入，
再校验 profile 输出。

设计对象：

```text
ProfileContract
  - profile_id
  - version
  - input_schema
  - output_schema
  - allowed_tools
  - denied_tools
  - max_context_messages
  - max_runtime_seconds
  - safety_policy
```

代码范围：

1. `agent-core`：新增 `ProfileContract`、`ProfileContractVersion`、
   `RuntimeToolPolicy` 和 schema 校验错误类型。
2. `agent-runtime`：新增 contract registry、input validation、
   `max_context_messages` 预算校验、确定性 `safety_policy` 校验和
   output validation。
3. 调用方可以把 profile contract metadata 传给 Runtime；Worker、Gateway
   或其他调用方如何生成和保存 contract version 属于各自集成边界。
4. 如需运营态动态 contract registry，再另行在存储层设计非破坏字段或新表；
   当前 Runtime 本体不修改 `agent-store`。

验收：

1. schema valid 时正常返回 `RuntimeOutput`。
2. schema invalid 时返回安全错误，不进入 successful runtime output。
3. 错误不泄露 prompt、secret、connector payload 或内部栈。
4. metadata 包含 `profile_id`、`schema_version` 和 `runtime_profile`。
5. 超过 `max_context_messages` 时返回安全错误，不进入 successful runtime output。
6. `safety_policy.deny_message_roles` 和 `safety_policy.max_message_bytes`
   会在进入模型前执行，未知 safety policy 字段 fail closed，失败时返回
   安全错误。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
```

提交建议：

```text
runtime: add profile contract validation
```

### R2：Runtime Streaming Events

目标：Runtime 支持 Hermes 上游 SSE 解析和有序 `RuntimeStreamEvent` 输出，
同时保留最终 `RuntimeOutput` 作为落盘结果。当前完成口径是
`RuntimeClient::stream_*()` 返回完整 event 序列；调用方可边读边转发的
object-safe async stream / callback API 是后续扩展，不在本轮 repo/local
完成口径内。

设计对象：

```text
RuntimeStreamEvent
  - trace_id
  - run_id or session_id
  - profile_id
  - schema_version
  - sequence
  - event_type: started | delta | tool_progress | schema_partial | final | error
  - content_delta
  - output
  - error_code
  - metadata
```

代码范围：

1. `agent-core`：新增 `RuntimeStreamEvent` 和 event-returning streaming trait
   边界。
2. `agent-runtime`：为 Hermes adapter 增加上游 SSE streaming path。
3. `agent-core` / `agent-runtime`：streaming path 失败时返回安全 `error`
   event；非 streaming path 仍按原 `CoreResult` 错误语义返回。

验收：

1. 非 streaming path 不回退。
2. streaming final 和非 streaming 输出语义一致。
3. error event 不泄露 prompt、credential、connector payload 或内部栈。
4. Runtime stream event 能携带 trace、run/session、profile 和 schema version；
   是否写入审计由调用方决定。
5. tool loop streaming path 会发出 `tool_progress`；schema 校验通过后会发出
   `schema_partial`。
6. safe error event 有回归测试，确认不包含 prompt 或 upstream error body。
7. 本轮不声明 Runtime 已提供下游 async stream/backpressure API。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
```

提交建议：

```text
runtime: add streaming event contract
```

### R3：Per-profile Tool Permission Enforcement

目标：profile 工具权限成为真实执行约束，而不是 prompt 约定。

权限合成规则：

```text
effective_tool_set =
  requested authorized tools
  ∩ ProfileContract.allowed_tools
  - ProfileContract.denied_tools
  - non-read-only RuntimeToolSpec
```

代码范围：

1. `agent-core`：定义 tool capability、allowed/denied/effective tool set。
2. `agent-core`：`RuntimeRunInput` / `RuntimeSessionInput` /
   `RuntimeProfileInput` 携带本次调用的 `requested_tools`。
3. `agent-runtime`：执行前计算 effective tool set，拒绝越权 tool request。
4. external-action 写入能力仍在 Manager apply/compensate 边界内；Runtime
   只负责拒绝 write-capability tool scope 和 tool call。

`allowed_tools` 为空时表示 profile 没有工具权限，不表示通配所有工具。

验收：

1. profile 不能调用未授权工具。
2. prompt injection 不能打开额外工具权限。
3. 写入类工具不能绕过 Manager external-action plan。
4. metadata 记录 effective tool set；调用方可按需把 runtime tool events
   追加到自己的 audit。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-core
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
```

提交建议：

```text
runtime: enforce profile tool policy
```

### R4：Multi-profile Step Plan

目标：多 profile 编排显式化，不藏在 `HermesRuntimeClient` 或一次 run/message
调用里。

目标设计对象：

```text
RuntimeStepPlan
  - plan_id
  - trace_id
  - owner: manager | orchestrator | domain_gateway
  - steps:
      - step_id
      - profile_id
      - input_ref
      - output_contract
      - depends_on
      - tool_policy
      - fallback_policy
```

已实现 `RuntimeStepPlan` / `RuntimeStep` 数据模型、plan owner、step
dependency、fallback policy、step 级 `output_contract` / `tool_policy` 和
通用多 step 执行器。执行器遵循以下边界规则：

1. step plan 必须由 Manager、Orchestrator 或领域 Gateway 明确创建。
2. Runtime 只执行已授权 step，不自行追加新 profile step。
3. step 之间只能传递 step output contract 校验后的 `output_ref`。
4. 任一 step 执行失败、依赖缺失、output_ref 缺失或输出契约失败，都必须有
   明确降级或终止策略。

代码范围：

1. `agent-core`：新增 `RuntimeStepPlan`、`RuntimeStep`、
   `RuntimeStepStatus`、owner、dependency、fallback policy、step
   `output_contract` 和 step `tool_policy`。
2. `agent-core` / `RuntimeClient`：提供通用 `execute_profile_step_plan()`
   默认执行器。
3. `agent-runtime`：执行单个已授权 profile step，并把 step metadata 写入
   `RuntimeOutput.metadata`。
4. `agent-core`：提供 `RuntimeStepPlan::for_profile_contracts()` 作为完整
   step plan 创建 helper；Manager、Orchestrator 或领域 Gateway 可以使用该
   helper 或生成等价完整 plan。
5. 持久 step audit / 跨进程 step 查询是调用方或存储层集成边界，不属于
   Runtime 本体完成条件。

验收：

1. 每个 profile step 独立可追踪。
2. step output 未通过 schema 时不能作为 successful output 进入下一 step。
3. Runtime 执行既有 step plan，不自行追加新 step。
4. 多 step 依赖、fallback、输出契约失败降级和跨 step output_ref 流转有
   回归验证。
5. step 级 tool policy 会收窄本 step 的 effective tool set。
6. 持久 step audit 表不是当前 Runtime 本体完成条件。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-core
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
```

提交建议：

```text
runtime: add multi-profile step plan
```

### R4.5：Runtime Tool Execution Loop

目标：Agent Runtime 本体支持 LLM profile 发起 read-only tool call，同时确保
工具执行仍受 profile contract、schema、预算和审计约束。

边界规则：

1. Runtime 只执行本次授权 scope 与 profile contract 交集内的 read-only tools。
2. 写入类工具仍只能走 Manager external-action apply/compensate。
3. 每个 tool call 在执行前校验 tool name、input schema 和 profile tool policy。
4. 每个 tool result 在回灌给 profile 前校验 output schema。
5. 大 tool output 不进入 final `RuntimeOutput.metadata`；metadata 只保留
   output_ref、类型/长度/数量摘要、已校验的 output_schema contract、
   tool name、call id 和 trace 信息。
   摘要不得包含 raw string、object key 名或 executor metadata payload。
6. `RuntimeToolSpec.output_ref_required=true` 时，tool executor 缺失
   `output_ref` 必须失败，不能由 Runtime 自动补全后当作成功结果。
7. profile step 必须受最大 tool round 和 `max_runtime_seconds` 预算约束。
8. RuntimeOutput metadata 中必须返回安全 tool event 摘要；Runtime adapter
   直连场景必须配置等价 append-only audit sink。
9. tool call 失败时必须写安全 `runtime_tool_error` audit event，且不记录
   tool arguments 或明文 payload。

代码范围：

1. `agent-core`：定义 `RuntimeToolCall`、`RuntimeToolResult`、
   `RuntimeToolSpec` 和 `RuntimeToolExecutor`。
2. `agent-runtime`：Hermes adapter 支持 OpenAI-compatible tool loop。
3. `agent-runtime`：执行前校验 tool policy、tool input schema 和预算。
4. `agent-runtime`：执行后校验 tool output schema，并生成安全 metadata 摘要。
5. `agent-runtime`：提供 `RuntimeAuditSink` 和 JSONL append-only sink，供
   Gateway 直连 Runtime 时显式配置。
6. `agent-runtime`：tool call 执行失败时追加 `runtime_tool_error` audit
   event。

验收：

1. 授权 tool call 会执行，并把安全 tool result 回灌给 profile。
2. 未授权或 denied tool call 在执行前被拒绝。
3. tool output schema invalid 时不会回灌给 profile，也不会形成 successful
   step output。
4. final metadata 不包含大 payload，只包含 ref 和摘要。
5. tool executor 返回的 metadata payload 不进入 final metadata 或 adapter audit。
6. tool output summary 不泄漏 raw string、object key 名或 executor metadata
   payload。
7. required `output_ref` 缺失时返回安全错误，并写 `runtime_tool_error`
   audit event。
8. tool executor 返回的 call/profile/tool 身份不能覆盖 Runtime 已授权 tool call。
9. 超出 tool round 或 runtime budget 时返回安全错误。
10. streaming profile step 超出 runtime budget 时返回安全 `error` event。
11. RuntimeOutput metadata 或 Runtime adapter audit sink 可以看到 runtime
   tool call / result / error event。
12. Runtime adapter 直连 JSONL audit sink 有单独验证。
13. 未授权 tool call 的失败 audit 有回归验证，且不包含 tool arguments。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-core
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
```

提交建议：

```text
runtime: audit profile tool execution
```

## 状态口径

当前可以说：

```text
Agent Platform 已具备 P1 真实 Hermes Runtime 只读闭环和 P2 external-action
执行链路；Runtime 已具备 profile contract、轻量 schema validation、
streaming event contract、per-profile tool permission、multi-profile step plan
和 read-only tool execution loop 的 repo/local 实现。
```

不能说：

```text
P2 已完成 Runtime 全量完善。
Agent Runtime 本体完成等于完整 JSON Schema 或领域 Gateway 接入完成。
Runtime 本体完成即可代表任何领域 Gateway 已完成接入。
Runtime 已提供调用方可边读边转发的 async stream/backpressure API。
```
