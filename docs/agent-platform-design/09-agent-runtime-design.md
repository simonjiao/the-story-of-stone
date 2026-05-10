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

本文只处理 Runtime 执行面完善，不改变 Manager 授权、Open WebUI Bridge、
run/session 状态机、Worker claim、Memory schema 或 audit contract。

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

这已经满足 P1 的真实 Hermes Runtime 只读闭环，但还不是完整的多 profile、
强 schema、强工具权限 Runtime。

## 缺口和当前处置

以下能力不能用 P2 external-action 完成状态替代。R1 到 R4 已按本专项
补齐 repo/local 实现；R5 因通灵玉目标架构调整为“薄 Gateway + Runtime Agent”
而重新打开：

1. Runtime streaming 输出：R2 已补齐 Runtime 原生流式事件和 final output
   语义。
2. 结构化 schema 输出校验：R1 已补齐 profile input/output contract 和
   schema validation。
3. per-profile tool permission：R3 已补齐 profile 工具策略快照和执行前约束。
4. 多 profile 编排：R4 已补齐显式 step plan，Runtime 只执行已授权 step。
5. 领域 profile 强类型契约：R5 待按通灵玉四 profile 工程版 contract
   落地，并在轻量领域复核后冻结首版字段。
6. 通灵玉薄 Gateway + Runtime Agent 接入：待实现。旧的 Gateway 内
   SQLite/FTS 检索、证据包构建、reviewer 执行和确定性 text/commentary
   step 不再作为目标架构。

## 完善目标

Runtime 完善专项的目标不是让 Runtime 变成控制面，而是把执行面从
“一次文本调用”提升为“可审计、可校验、可组合的受控 profile 执行层”。

完成后应满足：

1. Runtime 可以返回非流式和流式两类输出，且 Worker/Orchestrator 能保留现有
   audit 和 final result 语义。
2. 每个 profile 可以声明输入 schema、输出 schema、允许工具、禁止工具和
   安全策略。
3. Runtime 对 profile 输出做 schema 校验，失败时返回安全错误，不泄露 prompt、
   credential、connector payload 或内部栈。
4. 多 profile 编排必须由 Manager/Orchestrator/Gateway 明确授权和记录；
   Runtime 只能执行已授权 step，不能自行扩权或创建新 step。
5. 领域 profile 可以在不破坏通用 Agent Platform contract 的前提下注册 typed
   contract，例如通灵玉的 `honglou-main`、`honglou-text`、
   `honglou-commentary`、`honglou-reviewer`。
6. 通灵玉固定采用“薄 Gateway + Runtime Agent”：Gateway 只做协议、鉴权、
   路由、trace、SSE 和模型隐藏；正文、脂批、证据包、reviewer 和 replay
   归 Runtime profile 及其受控工具负责。Open WebUI 仍只看到一个
   `tonglingyu` 入口。

## 非目标

Runtime 完善专项不做以下事情：

1. 不让 Open WebUI 直连 Agent Runtime。
2. 不让 Runtime 决定用户权限、审批、资源锁或 credential scope。
3. 不让 Runtime 持有长期 credential 或明文 secret。
4. 不把 external-action apply/compensate 从 Manager 下放给 Runtime。
5. 不把所有领域知识库迁入 Agent Platform core；通灵玉知识库可以作为
   Runtime Agent 的领域 read-only tool/service 存在。
6. 不要求所有领域 agent 都必须使用多 profile；简单 agent 仍可只用一个 profile。

## 目标架构

Runtime 专项完成后的调用边界如下：

```text
Open WebUI
  -> Orchestrator / Thin Domain Gateway
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
      -> source snapshot / SQLite / FTS / evidence package / replay data
  -> RuntimeOutput / RuntimeStreamEvent
  -> audit / memory / evidence refs / final response
```

如果由 Worker 执行，必须沿用现有 run/session 状态机。
如果由领域 Gateway 直接调用 Runtime adapter，必须满足：

1. Gateway 仍是唯一公开入口。
2. Gateway 只做协议适配、鉴权、限流、路由、trace/session 透传、
   OpenAI-compatible 响应封装和 SSE 转发。
3. Gateway 不直接执行 SQLite/FTS 查询，不构建证据包，不执行 reviewer，
   不维护 replay 领域逻辑。
4. Runtime profile 和 read-only tools 负责正文、脂批、证据包、reviewer、
   replay 和 admin trace 的领域数据。
5. Gateway 记录或透传 trace、profile step、schema version、tool policy、
   evidence/package ref 和 result。
6. Gateway 不把内部 profile 暴露为 Open WebUI 可见模型。

## 统一实施计划

Runtime 专项按 R0 到 R5 推进。每个节点必须在同一处说明设计目标、
代码范围、验收和提交口径，避免在 roadmap、checklist 和专项文档之间
形成重复但不一致的计划。

### R0：现状锁定和口径统一

目标：关闭设计口径分歧，明确 P1/P2 已完成内容和 Runtime 未完成内容。

当前结论：

1. P1 已完成真实 Hermes Runtime 只读闭环。
2. P2 已完成 external-action provider/connector、apply/compensate 和
   contract smoke 路径。
3. P2 不包含 Runtime streaming、profile schema、per-profile tool permission、
   multi-profile step plan 或通灵玉四 profile 接入。
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
2. `agent-runtime`：新增 contract registry、input validation 和
   output validation。
3. `agent-worker`：调用 Runtime 时传入 profile contract metadata。
4. `agent-store`：如需持久化 contract version，只新增非破坏字段或新表。

验收：

1. schema valid 时正常返回 `RuntimeOutput`。
2. schema invalid 时返回安全错误，不进入 successful runtime output。
3. 错误不泄露 prompt、secret、connector payload 或内部栈。
4. metadata 包含 `profile_id`、`schema_version` 和 `runtime_profile`。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
cargo test --manifest-path agent-platform/Cargo.toml -p agent-worker
```

提交建议：

```text
runtime: add profile contract validation
```

### R2：Runtime Streaming

目标：Runtime 支持原生流式输出，同时保留最终 `RuntimeOutput` 作为落盘结果。

设计对象：

```text
RuntimeStreamEvent
  - trace_id
  - run_id or session_id
  - profile_id
  - schema_version
  - sequence
  - event_type: token | tool_progress | schema_partial | final | error
  - content_delta
  - metadata
```

代码范围：

1. `agent-core`：新增 `RuntimeStreamEvent` 和 streaming trait 边界。
2. `agent-runtime`：为 Hermes adapter 增加 streaming path。
3. `agent-worker`：只在 final event 后推进 run 完成态。
4. `agent-orchestrator`：复用现有 OpenAI-compatible SSE wrapper。

验收：

1. 非 streaming path 不回退。
2. streaming final 和非 streaming 输出语义一致。
3. error event 不泄露 prompt、credential、connector payload 或内部栈。
4. trace/audit 能看到流式调用的 final 状态。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
cargo test --manifest-path agent-platform/Cargo.toml -p agent-orchestrator
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
  Manager authorized tools
  ∩ ProfileContract.allowed_tools
  - ProfileContract.denied_tools
```

代码范围：

1. `agent-core`：定义 tool capability、allowed/denied/effective tool set。
2. `agent-manager`：为 run/session 生成授权 tool scope。
3. `agent-runtime`：执行前计算 effective tool set，拒绝越权 tool request。
4. `agent-manager` external-action：保持写入工具必须走 apply/compensate。

验收：

1. profile 不能调用未授权工具。
2. prompt injection 不能打开额外工具权限。
3. 写入类工具不能绕过 Manager external-action plan。
4. audit 记录 effective tool set。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-core
cargo test --manifest-path agent-platform/Cargo.toml -p agent-manager
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
```

提交建议：

```text
runtime: enforce profile tool policy
```

### R4：Multi-profile Step Plan

目标：多 profile 编排显式化，不藏在 `HermesRuntimeClient` 或一次 run/message
调用里。

设计对象：

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

边界规则：

1. step plan 必须由 Manager、Orchestrator 或领域 Gateway 明确创建。
2. Runtime 只执行已授权 step，不自行追加新 profile step。
3. step 之间只能传递 schema 校验后的 `output_ref`。
4. 任一 step 失败必须有明确降级或终止策略。

代码范围：

1. `agent-core`：新增 `RuntimeStepPlan`、`RuntimeStep` 和
   `RuntimeStepStatus`。
2. `agent-manager` 或 domain gateway helper：创建 step plan。
3. `agent-runtime`：执行单个已授权 step。
4. `agent-store`：如需持久化 step，只新增 append-only step audit 表。

验收：

1. 每个 profile step 独立可追踪。
2. step output 未通过 schema 时不能进入下一 step。
3. Runtime 不能自行创建新 step。
4. step 失败不会导致权限扩大或未审计输出。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-core
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
cargo test --manifest-path agent-platform/Cargo.toml -p agent-worker
```

提交建议：

```text
runtime: add multi-profile step plan
```

### R5：通灵玉薄 Gateway + Runtime Agent 接入

目标：把通灵玉从“Gateway 内部执行领域流程”调整为“Gateway 只做入口，
Runtime Agent 执行四个 profile 和领域工具”的架构。

目标接入形态：

```text
Open WebUI
  -> tonglingyu-gateway
      -> protocol / auth / routing / trace / SSE / model hiding
      -> Runtime step plan
          -> honglou-text LLM profile
              -> read-only tool: tonglingyu.text.search
          -> honglou-commentary LLM profile
              -> read-only tool: tonglingyu.commentary.search
          -> read-only tool: tonglingyu.evidence.package.create/read/replay
          -> honglou-main LLM profile
          -> honglou-reviewer LLM profile
      -> OpenAI-compatible final response
```

Gateway 只负责：

1. OpenAI-compatible 单入口。
2. 鉴权、限流、路由、trace/session 透传、SSE 转发和响应封装。
3. 只暴露 `tonglingyu`，不暴露 `honglou-*` 内部 profile。
4. 防止用户指定内部 profile、工具权限或关闭 reviewer。
5. 透传或代理 Runtime 返回的 trace id、package ref、session id 和安全错误。

Gateway 不负责：

1. 不直接执行 source snapshot、SQLite 或 FTS 查询。
2. 不构建证据卡片或证据包。
3. 不执行 reviewer 或本地审校规则。
4. 不维护证据包 replay 的领域逻辑。
5. 不把通灵玉领域数据写入 Agent Platform core contract。

Runtime Agent 负责：

1. 执行 `honglou-main`、`honglou-text`、`honglou-commentary`、
   `honglou-reviewer` 四个 profile。
2. 校验每个 profile 的 typed input/output。
3. 执行受 per-profile tool policy 约束的 read-only tool call。
4. 记录 profile、step、model、schema version、tool set、duration、
   trace_id 和 evidence/package ref。
5. 返回受约束的结构化结果、流式事件或安全错误。

领域 read-only tools 负责：

1. `tonglingyu.text.search`：查询正文、版本、回目、人物和 source snapshot
   位置，返回 evidence refs。
2. `tonglingyu.commentary.search`：查询脂批、评语、版本对应正文和来源位置，
   返回 commentary evidence refs。
3. `tonglingyu.evidence.package.create`：根据 profile 输出和 evidence refs
   生成证据包。
4. `tonglingyu.evidence.package.read`：读取证据包摘要和引用明细。
5. `tonglingyu.evidence.package.replay`：按 evidence/package ref 回放证据，
   不依赖上游模型。

领域 profile contract 草案：

1. `honglou-text`：LLM profile。输入用户问题、检索意图、版本/回目/人物条件
   和 top_k；通过 `tonglingyu.text.search` 获取正文证据；输出正文证据分析、
   支持范围、不支持范围和 evidence refs；不得解释脂批或输出最终回答。
2. `honglou-commentary`：LLM profile。输入用户问题、脂批/版本问题、版本条件
   和对应正文需求；通过 `tonglingyu.commentary.search` 获取批语证据；输出
   脂批证据分析、对应正文、支持范围、不支持范围和 evidence refs；不得把脂批
   当正文事实。
3. `honglou-main`：输入用户问题、`honglou-text` 输出、`honglou-commentary`
   输出、证据包 ref 和回答策略；输出草稿回答、claim statements 和证据引用
   关系；不得直接访问数据库或绕过 reviewer。
4. `honglou-reviewer`：输入用户问题、草稿、证据包 ref、claim statements 和
   负面清单；输出 review status、issues、severity 和 required revisions；
   不得重写最终答案或泄露内部规则全文。

代码范围：

1. `agent-core` / `agent-runtime`：补齐 tool call / tool result / tool executor
   contract，确保 tool output 可 schema 校验并以 `output_ref` 传递。
2. `agent-runtime`：为通灵玉 read-only tools 接入 per-profile tool policy。
3. `tonglingyu` 领域 tool/service：从 Gateway request path 中抽出 source
   snapshot、SQLite/FTS、证据卡片、证据包和 replay 逻辑。
4. `agent-runtime`：注册通灵玉四 profile contract 和允许工具矩阵。
5. `tonglingyu-gateway`：删除请求路径中的本地检索、证据包和 reviewer 执行，
   改为创建 Runtime step plan 并转发 streaming/final result。
6. `tonglingyu-gateway`：保留 OpenAI-compatible 响应封装、模型隐藏和 trace
   透传。

验收：

1. Open WebUI 仍只看到 `tonglingyu`。
2. 用户不能指定内部 profile、工具列表或关闭 reviewer。
3. Gateway 请求路径不直接调用 SQLite/FTS、证据包构建或 reviewer。
4. `honglou-text` 和 `honglou-commentary` 均走 LLM profile，并通过 Runtime
   read-only tools 获取证据。
5. 四个 profile 的调用、schema、duration、tool set 和结果都可按 trace 查询。
6. 证据包创建、读取和 replay 在 Runtime Agent/tool 侧完成，replay 不依赖
   上游模型。
7. 写入类工具仍只能走 Manager external-action apply/compensate，不因通灵玉
   read-only tools 下放到 Runtime。

测试：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime
cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
agent-platform/scripts/tonglingyu-gateway-smoke.sh
```

先做本地 dry run 和 fake runtime/tools 验证，再做目标环境 Open WebUI 单入口
复测。

提交建议：

```text
tonglingyu: move domain flow behind runtime agent
```

## 状态口径

在 R5 完成前，只能说：

```text
Agent Platform 已具备 P1 真实 Hermes Runtime 只读闭环和 P2 external-action
执行链路；Runtime 完善专项已补齐 R1 到 R4 的 streaming、schema、
per-profile tool permission 和 multi-profile step plan；通灵玉薄 Gateway +
Runtime Agent 接入仍未完成。
```

不能说：

```text
P2 已完成 Runtime 全量完善。
通灵玉四个内部 Agent 已真实 runtime 化。
通灵玉 Gateway 已满足薄 Gateway + Runtime Agent 架构。
```
