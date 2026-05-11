# 20 Runtime 接入设计与实施计划

## 文档定位

本文定义“通灵玉”接入 Agent Runtime 的目标架构、职责边界和实施 checklist。
它属于通灵玉领域设计，不属于 Agent Runtime 本体设计。

本文引用并整合以下现有文档：

1. `05_总体架构.md`：Open WebUI 单入口、Gateway、四 Agent 和 RAG 分层。
2. `06_四个Agent设计.md`：`honglou-main`、`honglou-text`、
   `honglou-commentary`、`honglou-reviewer` 的职责边界。
3. `07_Gateway设计.md`：通灵玉 Gateway 的入口、安全和编排边界。
4. `08_知识库与RAG设计.md`：正文、脂批、版本、人物和证据包语义。
5. `10_内部接口契约.md`：Gateway、Runtime profile、知识服务和审计协作。
6. `11_权限审计与安全治理.md`：内部 profile 不暴露、reviewer 不可关闭。
7. `12_验证方案与验收标准.md`：证据、审校、Gateway 和上线验证。
8. `18_第一版实施细化计划.md`：第一版工程阶段和本地/目标环境验证顺序。
9. `../agent-platform-design/09-agent-runtime-design.md`：通用 Runtime 能力基线。

## 目标

把通灵玉从“Gateway 内部执行领域流程”调整为“薄 Gateway + Runtime Agent”：

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

Open WebUI 仍只看到一个 `tonglingyu` 模型；`honglou-*` profile、工具列表、
reviewer 开关、证据包内部字段和 admin trace 字段都不能由普通用户控制。

## 当前基线与目标差距

当前 Rust `tonglingyu-gateway` 已具备可运行的产品闭环：source snapshot、
SQLite/FTS、证据卡片、证据包、reviewer、replay、admin trace、SSE 和
OpenAI-compatible 入口都已在 Gateway 内实现。

这可以作为运行基线和回归基线，但不是目标架构的最终形态。目标架构下：

1. Gateway 请求路径不直接查询 source snapshot、SQLite 或 FTS。
2. Gateway 不直接构建证据卡片、证据包或 replay 结果。
3. Gateway 不直接执行 reviewer 或本地审校规则。
4. `honglou-text` 和 `honglou-commentary` 都是 LLM profile，不是确定性
   Gateway 检索 step。
5. 证据检索、证据包、reviewer 和 replay 进入 Runtime profile 与
   read-only tools。

## Gateway 边界

Gateway 只负责：

1. OpenAI-compatible 单入口。
2. 鉴权、限流、路由、trace/session 透传、SSE 转发和响应封装。
3. 只暴露 `tonglingyu`，不暴露 `honglou-*` 内部 profile。
4. 防止用户指定内部 profile、工具权限或关闭 reviewer。
5. 创建或提交受控 Runtime step plan。
6. 透传或代理 Runtime 返回的 trace id、package ref、session id 和安全错误。
7. 保存必要的会话映射、请求状态和审计索引。

Gateway 不负责：

1. 不直接执行 source snapshot、SQLite 或 FTS 查询。
2. 不构建证据卡片或证据包。
3. 不执行 reviewer 或本地审校规则。
4. 不维护证据包 replay 的领域逻辑。
5. 不把通灵玉领域数据写入 Agent Platform core contract。
6. 不把内部 profile、prompt、tool payload 或 admin 字段暴露给普通用户。

## Runtime Agent 边界

Runtime Agent 负责：

1. 执行 `honglou-main`、`honglou-text`、`honglou-commentary`、
   `honglou-reviewer` 四个 profile。
2. 校验每个 profile 的 typed input/output。
3. 执行受 per-profile tool policy 约束的 read-only tool call。
4. 记录 profile、step、model、schema version、tool set、duration、
   trace_id 和 evidence/package ref。
5. 返回受约束的结构化结果、流式事件或安全错误。

Runtime Agent 不负责：

1. 不决定普通用户是否有权访问通灵玉入口。
2. 不暴露内部 profile 为 Open WebUI 可见模型。
3. 不执行写入类外部动作；写工具仍只能走 Agent Platform Manager 的
   external-action apply/compensate。
4. 不绕过 Gateway 的单入口、模型隐藏、reviewer 强制和审计要求。

## 领域 Read-only Tools

目标工具矩阵如下：

<!-- markdownlint-disable MD013 -->
| Tool | 责任 | 允许调用方 |
| --- | --- | --- |
| `tonglingyu.text.search` | 查询正文、版本、回目、人物和 source snapshot 位置，返回 evidence refs | `honglou-text` |
| `tonglingyu.commentary.search` | 查询脂批、评语、版本对应正文和来源位置，返回 commentary evidence refs | `honglou-commentary` |
| `tonglingyu.evidence.package.create` | 根据 profile 输出和 evidence refs 生成证据包 | Runtime step plan |
| `tonglingyu.evidence.package.read` | 读取证据包摘要和引用明细 | `honglou-main`、`honglou-reviewer`、Gateway admin proxy |
| `tonglingyu.evidence.package.replay` | 按 evidence/package ref 回放证据，不依赖上游模型 | Gateway admin proxy |
<!-- markdownlint-enable MD013 -->

工具输出必须保留原始字形、source snapshot 位置、版本边界、evidence refs、
支持范围和不支持范围。工具不得返回 secret、写权限 credential、内部 prompt
或无来源自然语言概括。

## 四 Profile Contract 草案

### `honglou-text`

LLM profile。输入用户问题、检索意图、版本/回目/人物条件和 top_k；通过
`tonglingyu.text.search` 获取正文证据；输出正文证据分析、支持范围、
不支持范围和 evidence refs。

禁止：解释脂批、输出最终回答、把影视或网络设定当正文证据。

### `honglou-commentary`

LLM profile。输入用户问题、脂批/版本问题、版本条件和对应正文需求；通过
`tonglingyu.commentary.search` 获取批语证据；输出脂批证据分析、对应正文、
支持范围、不支持范围和 evidence refs。

禁止：把脂批当正文事实、忽略批语版本、输出最终回答。

### `honglou-main`

LLM profile。输入用户问题、`honglou-text` 输出、`honglou-commentary` 输出、
证据包 ref 和回答策略；输出草稿回答、claim statements 和证据引用关系。

禁止：直接访问数据库、绕过 reviewer、把无证据推断写成事实。

### `honglou-reviewer`

LLM profile。输入用户问题、草稿、证据包 ref、claim statements 和负面清单；
输出 review status、issues、severity 和 required revisions。

禁止：重写最终答案、替代 text/commentary 大规模检索、泄露内部规则全文。

## 实施 Checklist

### R5A 薄 Gateway 边界

- [ ] Gateway 只做 OpenAI-compatible 协议适配、鉴权、限流、路由、
  trace/session 透传、SSE 转发、模型隐藏和响应封装。
- [x] Gateway 不直接执行 source snapshot loader、KB SQLite/FTS 检索或 FTS 写入。
- [x] Gateway 不直接读取 KB/domain SQLite 表；health、metrics、admin trace 和
  prune 通过 Runtime stats/audit/prune API 访问 runtime store。
- [x] Gateway 请求路径、dry-run、eval、health、search、metrics、admin trace/package
  读取和 build/prune 管理路径通过 `TonglingyuRuntimeStore` 访问 runtime store，
  不复用 Gateway `Connection`。
- [x] Gateway 公共 `/v1/*` 入口已增加 per-subject rate limit，
  `TONGLINGYU_RATE_LIMIT_PER_MINUTE` 可配置，`0` 表示关闭；health、JSON
  metrics 和 Prometheus info 暴露有效配置，smoke 覆盖默认值。
- [x] Gateway 请求体上限已显式配置为 `TONGLINGYU_MAX_BODY_BYTES`
  默认 1 MiB，避免依赖框架隐式默认；health、JSON metrics 和 Prometheus
  info 暴露有效配置，smoke 覆盖默认值。
- [x] Gateway 启动时强制 admin API key 与 Gateway service key 集合隔离；
  已配置 admin key 时不允许继续开启 gateway-key admin fallback，metrics
  的 `admin_key_isolated` 反映真实 key 集合状态。
- [x] Gateway 不构建证据卡片或证据包。
- [x] Gateway 不执行 reviewer 或本地审校规则。
- [x] Gateway 不维护证据包 replay 的领域逻辑。
- [x] Open WebUI 仍只看到 `tonglingyu`，用户不能选择 `honglou-*`
  内部 profile。

### R5B Evidence Read-only Tools

- [x] 从 `tonglingyu-gateway` 抽出 source snapshot loader。
- [x] 从 `tonglingyu-gateway` 请求路径抽出 SQLite/FTS 查询。
- [x] 从 `tonglingyu-gateway` 请求路径抽出证据卡片和证据包构建。
- [x] 从 `tonglingyu-gateway` 请求路径抽出证据包 read/replay。
- [x] 定义 `tonglingyu.text.search` read-only tool。
- [x] 定义 `tonglingyu.commentary.search` read-only tool。
- [x] 定义 `tonglingyu.evidence.package.create` runtime-scoped tool。
- [x] 定义 `tonglingyu.evidence.package.read` read-only tool。
- [x] 定义 `tonglingyu.evidence.package.replay` read-only tool。
- [x] 工具输出保留原始字形、source snapshot 位置、版本和 evidence refs。
- [x] 工具不暴露 secret、写权限 credential 或内部 prompt。

### R5B 当前实现校准

当前代码已新增 `tonglingyu-runtime` crate，并把 source snapshot loader、
KB schema、FTS 写入、别名种子、章节解析、证据包 create/read/replay、
claim-to-evidence 映射、reviewer 规则、本地受控回答、SQLite/FTS 检索和
evidence card 构建从 Gateway 函数体中迁出。Gateway `build-kb` 现在只处理
DB 文件生命周期、gateway session/workflow 清理和 Runtime rebuild 调用。
Gateway 现在通过
`tonglingyu-gateway::plan` 生成 search policy 和 Runtime step plan 快照，
并调用 runtime API 执行本地领域流程。Evidence package、review record、
claim link 和 audit event 的运行时表初始化已由
`tonglingyu-runtime::init_runtime_schema` 承接。Gateway 单元测试已加入
源码级回归断言，防止 runtime 领域函数重新回流到 Gateway。Runtime 已定义
`TonglingyuToolCall` / `TonglingyuToolOutput` / `tool_catalog`，Gateway 主路径
通过 `execute_runtime_workflow` 进入 Runtime profile workflow；Runtime
workflow 再调用 text/commentary search、package create/read，并返回
profile step output_ref、duration、tool set、trace_id、draft 和 final answer。
`tonglingyu-runtime` 也定义了四个 profile descriptor，Gateway Runtime step
plan 会记录 `PROFILE_CONTRACT_VERSION`，防止 plan 与 profile contract 脱节。
`tonglingyu-runtime` 现在会把这些 descriptor 映射为 `agent-core`
`ProfileContract`、read-only `RuntimeToolPolicy` 和 `RuntimeStepPlan`，并在
Gateway 新请求和 `runtime-dry-run` 中先执行 `agent-runtime`
`MinimalRuntimeClient` plan gate；该 gate 会校验 profile contract、step
dependency、requested tool scope、output_ref 和 Runtime step metadata。
Runtime plan factory 已收敛到 `tonglingyu-runtime::runtime_workflow_plan`；
Gateway 只把 search policy 转成 runtime plan input，agent-runtime plan gate
和实际 Runtime workflow 也从同一 plan 派生 step_id、operation 和 allowed tools。
Gateway 请求路径、`runtime-dry-run`、health、search、metrics、admin
package/trace 读取以及 build/prune 管理路径已通过 `TonglingyuRuntimeStore`
按 DB path 访问 runtime store，不再把 Gateway `Connection` 直接传入 Runtime
workflow/tool/schema/prune/rebuild API。
Gateway 新请求和 `runtime-dry-run` 现在也会让 `tonglingyu-runtime` 为每个
确定性 profile step 调用 `MinimalRuntimeClient::execute_profile_step`，并把
`agent_runtime` envelope 写入 step report、stream event metadata 和
`agent_runtime_profile_step_executed` audit event。该 envelope 明确标记
`content_source=tonglingyu-deterministic-workflow` 且
`content_used_for_final_answer=false`，避免把执行壳误判为 Hermes 内容执行。
`tonglingyu-runtime` 已提供 `TonglingyuRuntimeToolExecutor`，实现
`agent_core::RuntimeToolExecutor`，可把 agent-runtime/Hermes 的 tool call
转成 store-backed `TonglingyuToolCall`，覆盖 text search、package create/read
等本地证据工具。
profile step execution envelope 的 runtime client 已可通过
`TONGLINGYU_AGENT_RUNTIME_MODE=minimal|hermes` 选择；默认 `minimal` 保持本地
smoke 稳定，`hermes` 模式会使用 `HermesRuntimeClient::from_env()` 并挂载本地
`TonglingyuRuntimeToolExecutor`。
在 `hermes` 模式下，`draft_answer` profile 的 runtime output 可以成为
workflow 草稿，并由本地 reviewer enforcement 重新生成最终回答；Runtime 会记录
`agent_runtime_profile_draft_consumed` audit event，并区分
`content_used_for_final_answer`。如果本地 reviewer 降级，Hermes 草稿不会被标记为
最终回答内容。该路径仍只消费 draft profile 内容，不代表
text/commentary/package/reviewer 四 profile 全量内容和工具执行已经完成。
profile step message 已携带 trace_id、profile、operation、question、input/output
ref、allowed tools 和 step output JSON，避免 Hermes profile 只收到空泛
envelope 而无法理解当前通灵玉步骤。
Runtime 单测已覆盖完整 store workflow：注入 fake Hermes runtime client，验证
Hermes draft 候选会写入草稿、reviewer 降级时不会进入 final answer，并写入
`agent_runtime_profile_draft_consumed` audit event。
Gateway health、JSON metrics、Prometheus info 和 `runtime-dry-run` 已暴露
`TONGLINGYU_AGENT_RUNTIME_MODE` 的有效模式，smoke 会断言默认 `minimal`，避免
生产排障时误判当前使用的 Runtime client。
Runtime step report、SQLite audit 和 streaming step summary 已透出
agent-runtime/Hermes 工具 loop 观测信息，包括 `tool_rounds`、tool result
count 和 tool audit event count；完整 tool result/audit event 保留在 step
report/audit payload 中，stream 只暴露计数级摘要。
Hermes mode 下，required profile step 如果声明了 allowed tools，就必须产生
匹配的 `tool_results`；缺少必需工具结果时 Runtime workflow fail-closed，避免
把未实际调用工具的 profile 文本误判为 content/tool execution。
这些 `tool_results` 还必须携带 `runtime://tonglingyu/{trace_id}/...` output_ref；
package create/read/replay 类工具必须绑定当前 evidence package id，避免
Hermes 返回无法追溯到本地 Runtime store 的伪工具结果。text/commentary search
类工具必须绑定当前本地 evidence set 指纹，避免只带 trace 前缀但证据集合不匹配的
伪搜索结果。
Hermes `draft_answer` profile output 已支持结构化 JSON 候选；JSON 候选必须带
当前 evidence package 的 `package_id` 和非空 `draft_answer`，package 不匹配或
缺少草稿时只写 rejected audit，不进入本地草稿或最终回答。纯文本
`result_summary` 仅作为兼容路径保留。
Hermes `review_answer` profile output 已支持结构化 review observation，Runtime
会记录 review status、severity、issue count、required revision count，并标记
是否与本地强制 reviewer 一致；不一致时写 `local_reviewer_override=true`，
最终裁决仍以本地 reviewer enforcement 为准。
Runtime 发给 agent-runtime/Hermes 的 profile step message 和 metadata 已携带
operation-specific `result_summary_contract`：`draft_answer` 明确要求返回
`draft_answer`、`package_id`、`claim_statements` JSON object string，
`review_answer` 明确要求返回 `review_status`、`severity`、`issues` 和
`required_revisions` JSON object string。
`text_evidence_search` 与 `commentary_evidence_search` 也已要求结构化
evidence observation，Runtime 会校验 Hermes 返回的 evidence refs 是否来自该
step 的本地 `evidence_ids`，未知 ref 只写 rejected reason，不允许改写本地证据。
Hermes `evidence_package_create` profile output 已进入 package observation；
Runtime 会校验 observation 中的 `package_id` 是否匹配本地 evidence package，
不匹配时写 rejected reason。该 observation 不允许改写本地证据包。

Runtime workflow 现在会生成 `RuntimeWorkflowStreamEvent`，新请求的
Gateway streaming response 只把 Runtime `content_delta` event 包装为
OpenAI-compatible SSE chunk，不再由 Gateway 自行切分领域回答。去重缓存命中
的 streaming replay 会复用缓存中的 Runtime stream events；旧缓存如果缺少
events，会 fallback 到 cached completion stream。

这些改动仍不能宣布整体完成：`agent-runtime` 当前已覆盖 contract/step plan
gate 和 profile step execution envelope，但四 profile 的领域内容、工具调用和
reviewer 结果仍由 `tonglingyu-runtime` 确定性 workflow 执行。R5D 必须等
profile content/tool 执行面接入 `agent-runtime`/Hermes 和目标环境 Open WebUI
复测完成后再勾选。

### R5C 四 Profile 编排

- [x] 为 `honglou-text` 定义 LLM profile contract、允许工具和输出 schema。
- [x] 为 `honglou-commentary` 定义 LLM profile contract、允许工具和输出 schema。
- [x] 为 `honglou-main` 定义 LLM profile contract、输入依赖和输出 schema。
- [x] 为 `honglou-reviewer` 定义 LLM profile contract、输入依赖和输出 schema。
- [x] `honglou-text` 通过 `tonglingyu.text.search` 生成正文 evidence analysis。
- [x] `honglou-commentary` 通过 `tonglingyu.commentary.search` 生成脂批
  evidence analysis。
- [x] 证据包由 Runtime Agent/tool 侧创建，`honglou-main` 只消费 package ref
  和前序 profile 输出。
- [x] `honglou-reviewer` 强制消费草稿、claim statements 和 package ref。
- [x] reviewer 不可关闭；未通过 reviewer 的结果不能作为最终回答返回。
- [x] 四 profile step 的 schema、duration、tool set、output_ref 和 trace_id
  可追踪。

### R5D Gateway 集成和验证

- [x] 将旧 `answer_with_optional_upstream` / 本地 query path 替换为 Runtime
  workflow 调用。
- [x] 新请求先执行 `agent-runtime` plan gate，校验 profile contract、
  step dependency 和 read-only tool scope。
- [x] Runtime plan factory 收敛到 `tonglingyu-runtime`，Gateway plan、
  agent-runtime plan gate 和实际 workflow 共用 step source。
- [x] 每个确定性 profile step 先接入 `agent-runtime`
  `execute_profile_step` envelope，并记录 `agent_runtime_profile_step_executed`
  audit event。
- [x] 提供 store-backed `TonglingyuRuntimeToolExecutor`，为后续 Hermes profile
  content/tool execution 调用本地证据工具准备执行边界。
- [x] profile step execution envelope 支持 `minimal|hermes` runtime client
  选择；Hermes 模式挂载本地 Tonglingyu tool executor。
- [x] Hermes 模式可消费 `draft_answer` profile output 作为草稿，并强制经过
  本地 reviewer enforcement 后才生成最终回答。
- [x] profile step 输入携带 operation、output_ref、allowed tools 和 step output
  context，Hermes 不再只收到空泛 envelope。
- [x] health、metrics、Prometheus info 和 dry-run 暴露 agent runtime mode，
  smoke 覆盖默认 `minimal` 模式。
- [x] step report、audit 和 streaming step summary 暴露 Hermes tool loop
  观测信息，避免生产排障时看不到 profile 是否实际调用工具。
- [x] Hermes mode 下 required profile step 必须产生匹配 allowed tools 的
  runtime tool result；缺少必需工具结果时 workflow fail-closed。
- [x] Hermes mode 下 runtime tool result 必须携带 Tonglingyu runtime output_ref；
  package tool 的 output_ref 必须绑定当前 evidence package，text/commentary
  search tool 的 output_ref 必须绑定当前本地 evidence set。
- [x] Hermes `draft_answer` 结构化 JSON 候选必须校验 `package_id`，
  错误 package 或缺少草稿时拒绝消费并写入 rejected audit。
- [x] Hermes `review_answer` 结构化 JSON 输出进入 review observation，
  记录与本地强制 reviewer 的一致性，但不替代最终裁决。
- [x] profile step message/metadata 携带 operation-specific
  `result_summary_contract`，避免 Hermes 不知道 draft/reviewer 的结构化输出要求。
- [x] `text_evidence_search` 与 `commentary_evidence_search` 输出进入
  evidence observation，校验 refs 是否来自本地 runtime step evidence ids。
- [x] Hermes `evidence_package_create` 输出进入 package observation，
  校验 `package_id` 是否匹配本地 runtime package，但不允许改写证据包。
- [ ] profile content/tool execution 仍需从确定性 workflow 接入
  `agent-runtime`/Hermes 执行面。
- [x] 新请求 Gateway streaming response 只转发 Runtime `content_delta`
  event，不自行生成领域内容。
- [x] 去重缓存命中的 streaming replay 改为 Runtime event replay。
- [x] Gateway final response 只包含最终回答、trace_id、session/package ref 和
  安全元数据，不暴露内部日志或 prompt。
- [x] 增加 fake runtime/tools 的本地 dry run。
- [x] 增加 `deploy/scripts/verify-tonglingyu-runtime-config.sh`，基于 compose
  渲染结果检查 Tonglingyu/Hermes strict runtime wiring、Open WebUI 默认模型、
  admin/gateway key 隔离和 provider key 不含 admin credential。
- [x] 生产 compose 显式设置 Gateway `TONGLINGYU_AGENT_RUNTIME_MODE=hermes`
  并注入 `AGENT_RUNTIME_HERMES_*`；release gate 拒绝 Gateway runtime mode
  仍为 `minimal` 的渲染结果。
- [x] 增加 `deploy/scripts/verify-tonglingyu-strict-gateway.sh`，运行态检查
  Gateway health/models/admin metrics/Prometheus，确认 `hermes` runtime、单可见
  模型、隐藏内部 profile、KB 非空、rate limit 和 admin key 隔离。
- [x] strict Gateway gate 已增加 live chat completion 和 admin trace 校验，
  要求 Hermes profile step audit 中出现非空 runtime tool result。
- [x] strict Gateway gate 已要求 live admin trace 中 evidence/package/reviewer
  observation 均显示本地 Runtime enforcement，且 `draft_answer` Hermes draft
  observation 被消费，避免把工具链路存在误判为内容执行闭环。
- [x] Hermes mode 的 step audit/streaming metadata 按
  evidence/package/reviewer observation 和 draft application 写入真实
  `agent-runtime-hermes-*` content source，避免继续显示成纯确定性 workflow。
- [x] Runtime output、dry-run、workflow state 和 runtime audit 已增加
  `agent_runtime_summary` / `agent_runtime_profile_execution_summarized`，strict
  live gate 会要求 summary 显示 Hermes observation + local governance 闭环。
- [x] admin trace 顶层透出最新 `agent_runtime_summary`；strict live gate 会校验
  summary 的 step/tool 计数与详细 runtime step audit event 一致。
- [x] Runtime summary 和 strict Gateway gate 已增加 `tool_audit_event_count`
  交叉校验；Hermes mode 下 tool result 没有被 tool audit event 覆盖时会
  fail-closed，避免只验证工具结果字段、不验证工具执行审计。
- [x] strict Gateway gate 会要求 tool result 与 `runtime_tool_result` audit
  event 按 `tool_name` / `output_ref` 绑定，避免 audit 数量正确但具体工具输出
  无法审计归因。
- [x] strict Gateway gate 会校验 admin trace 顶层 `agent_runtime_summary`
  与最新 runtime summary audit event 完全一致，避免 Gateway 管理入口展示陈旧或
  缺失 summary 仍被误判为通过。
- [x] Hermes mode 下 profile content/tool execution 不完整时 Runtime fail-closed；
  会写 `agent_runtime_profile_execution_rejected`，不再返回本地 deterministic
  fallback 作为成功回答。
- [x] 增加 `deploy/scripts/verify-tonglingyu-release-readiness.sh` 聚合 gate，
  生成 JSON 报告并显式区分必过、失败、skipped live gate 和人工页面复核项。
- [x] release gate 在 `TONGLINGYU_RELEASE_REQUIRE_LIVE=true` 时要求显式
  `TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true`，避免自动 gate 通过
  但 Open WebUI 页面侧未复测时仍显示生产发布通过。
- [x] 增加 Gateway 不重新持有 source snapshot、FTS 和 reviewer 领域函数的
  回归断言。
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
- [x] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`
- [ ] 目标环境 Open WebUI 单入口复测。

## 验收口径

完成后才能声明：

```text
通灵玉四个内部 Agent 已真实 Runtime 化。
通灵玉 Gateway 已满足薄 Gateway + Runtime Agent 架构。
```

完成前只能声明：

```text
通灵玉已有可运行的 Gateway 内部闭环；目标 Runtime 接入仍在 R5 checklist 中。
```

Agent Runtime 本体完成不等于通灵玉接入完成；通灵玉接入必须以本文 checklist、
Gateway 回归测试和目标环境 Open WebUI 单入口复测为准。
