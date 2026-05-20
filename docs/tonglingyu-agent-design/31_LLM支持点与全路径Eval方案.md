# the-story-of-stone LLM 支持点与全路径 Eval 方案

<!-- markdownlint-disable MD013 MD060 -->

版本：v1.1
日期：2026-05-20
口径：融合当前仓库实现、通灵玉设计文档，以及本轮关于“从 user request 到 user response 的完整流程中所有可用 LLM 支持点”的讨论。

## 0. 证据口径

本文只采用当前可核验的 repo-local 证据：

1. `agent-platform/crates/tonglingyu-gateway/src/main.rs`
2. `agent-platform/crates/tonglingyu-gateway/src/context_governance.rs`
3. `agent-platform/crates/tonglingyu-runtime/src/lib.rs`
4. `docs/tonglingyu-agent-design/20_Runtime接入设计与实施计划.md`
5. `docs/tonglingyu-agent-design/26_Scoped_Context与受控Memory设计.md`
6. `docs/tonglingyu-agent-design/27_Scoped_Context_Request_Path_Checklist.md`
7. `docs/tonglingyu-agent-design/28_Context_Projection_Runtime_Checklist.md`
8. `docs/tonglingyu-agent-design/30_Scoped_Memory_Production_Checklist.md`
9. `docs/tonglingyu-agent-design/PROGRESS.md`

本文不使用未经当前核验的外部产品能力作为设计依据。OpenAI、Anthropic、LangGraph、
Hermes/AiBot 等只能作为工程模式类比，不能作为通灵玉当前实现事实。

## 0.1 外部实现模式的借鉴边界

可以借鉴外部实现模式，但不能借鉴完成状态。理由如下：

1. OpenAI / Agents 类模式可借鉴 structured output、tool call、trace、eval 和
   single entry orchestration。这些模式适合映射到 resolver、profile step、
   reviewer observation 和节点级 eval，但不能证明本仓库已完成对应能力。
2. Anthropic / MCP / Skills 类模式可借鉴最小工具权限、上下文隔离和能力包版本化。
   这些模式适合映射到 `context_projection`、profile-specific tools 和可版本化
   能力模块，但不能替代本仓库的权限、证据治理和审计账本。
3. LangGraph / LangSmith 类模式可借鉴 state graph、节点输入输出、replay 和失败归因。
   这些模式适合映射到 request -> context -> retrieval -> package -> review ->
   response 的全路径 eval，但每个节点必须落到通灵玉自己的 schema 和 release gate。
4. Hermes / AiBot 类模式可借鉴 profile 隔离、runtime client、tool registry、
   session/runtime audit。当前系统已有这类接入方向，但仍必须由本仓库的
   Runtime/Gateway contract、tool policy、output_ref、audit 和 hhost gate 验证。

因此，外部系统只作为工程模式参考；凡是借鉴的模式，都必须重新落到通灵玉自己的
schema、policy、audit、replay、eval 和 release gate 中。

## 1. 必须明确指出的不一致

以下不是折中项，而是必须在文档中改正或标注的事实差异。

| 编号 | 原稿说法 | 当前实现或设计证据 | 结论 |
|---|---|---|---|
| I1 | Question Resolver schema 写为 `tonglingyu-question-resolver-v2` | 当前代码常量是 `RESOLVER_SCHEMA_VERSION = "tonglingyu-question-resolver-v1"` | 原稿 schema version 不一致，本文统一为 v1。 |
| I2 | LLM resolver 的 `used_context_refs` 示例为 `session_hint`、`conversation_state_summary` | 若后续落地 LLM resolver contract，受控 context refs 应限制为 `current_question`、`recent_user_messages`、`recent_assistant_messages`、`prior_subject`、`session_summary` 一类白名单 | 原稿示例不可直接落地，必须改为当前允许集合，或先新增 schema 版本和迁移。 |
| I3 | `conversation_state_summary` 被写成目标结构，像是已在链路中存在 | 当前请求路径只有确定性 `session_summary(...)`；没有独立持久化的 `conversation_state_summary` 节点 | 只能标为目标增强，不能写成当前实现。 |
| I4 | Question Resolver 可见“受控 memory summary” | 当前 `create_context_for_request` 先执行 `resolve_question(...)`，之后才 `load_authorized_memory_reads(...)` | 当前 resolver 不能依赖 memory summary；若要引入，必须重新设计顺序和 fail-closed gate。 |
| I5 | “P0 显式 Question Resolution”像是完全未实现 | 当前已有 `resolved_question`、context pack、admin trace 和 fail-closed 澄清；LLM resolver 输出 contract 仍是后续目标增强 | 正确口径是：显式节点已存在，但真实外部 LLM resolver 尚未生产接入。 |
| I6 | 四个 Runtime profile 被描述为完整 LLM 生成链 | 当前 Runtime workflow 仍保留本地工具、证据包、本地 reviewer enforcement；Hermes/Agent Runtime 输出必须受 contract 和本地治理约束 | LLM profile 是受控执行/观察/候选能力，不是最终事实或最终裁决来源。 |
| I7 | 27/28 checklist 中“active memory 未实现”容易被读成当前全局状态 | 30 checklist 和 PROGRESS 记录 Scoped Memory Production 已进入 production-ready gate，27/28 的禁止项只属于当时阶段边界 | 文档必须按阶段解释，不能把旧阶段的“非目标”套到当前总状态。 |
| I8 | Retrieval policy 使用“LLM suggested policy + deterministic patch”像是当前实现 | 当前主路径 `search_policy(resolved_question)` 是确定性入口；LLM suggested policy 尚未接入 | 只能作为后续目标，不是当前事实。 |
| I9 | Evidence package 示例 `review: null` | 当前主链路 package 带 review，Gateway 后续还记录 review journal 和用户响应 wrapper 过滤 | 示例应表达“review record 必须存在或可回放”，不能暗示 review 可空。 |
| I10 | Eval 门槛全写成绝对百分比，像是当前已有数据集 | 当前已有多类 gate 和测试，但该文定义的多数据集 eval suite 尚未整体落地 | 百分比可作为发布目标，不能写成已通过事实。 |

### 1.1 不一致项的待确认口径

| 编号 | 是否待确认 | 需要确认的问题 | 确认后动作 | 不能误写成 |
|---|---|---|---|---|
| I1 | 否，已决纠偏 | 无。当前只能按 v1 书写。 | 文档、schema 示例和 eval fixture 统一使用 `tonglingyu-question-resolver-v1`。 | 不能写成 v2 已存在，除非先做 schema v2 迁移。 |
| I2 | 是 | LLM resolver 未来能读取哪些 context refs。 | 明确白名单、未知 ref fail-closed、补 schema 和 eval。 | 不能默认允许 memory、完整 history、任意 session hint。 |
| I3 | 是 | 是否新增 `conversation_state_summary` 节点。 | 若确认新增，需要 writer/loader/schema/audit/eval；若不新增，保留确定性 `session_summary`。 | 不能写成当前链路已有该节点。 |
| I4 | 是，架构级 | resolver 是否允许读取 memory summary。 | 若允许，需要调整 resolver 与 memory read 的执行顺序，并补 ACL、budget、fail-closed gate。 | 不能在当前顺序下声称 resolver 已可见 memory。 |
| I5 | 是 | 是否继续实现 LLM resolver contract。 | 若确认实现，补 resolver runtime/Hermes 调用、schema repair、audit 和 eval；若不实现，保持规则 resolver。 | 不能把“显式 Question Resolution 已存在”说成“LLM resolver 已生产接入”。 |
| I6 | 是 | 每个 Runtime profile 的输出权力边界。 | 确认 profile 只输出 observation/candidate，最终事实、证据包和裁决仍由本地治理约束。 | 不能写成四个 profile 共同生成最终事实链。 |
| I7 | 否，阶段口径纠偏 | 无。27/28 和 30 属于不同阶段。 | 文档引用时标明阶段边界，避免旧 checklist 覆盖新 production gate。 | 不能把 27/28 的“未实现/非目标”当成当前全局状态。 |
| I8 | 是 | 是否引入 LLM suggested retrieval policy。 | 若确认引入，需要建议 schema、deterministic patch、版本/脂批强制证据 eval。 | 不能写成当前 retrieval policy 已由 LLM 决定。 |
| I9 | 是，schema 表达 | evidence package 中 review record 的必需形态。 | 明确 review record 必须存在或可回放，示例避免 `review: null`。 | 不能暗示 reviewer 可缺席或可由 LLM 直接替代。 |
| I10 | 是，评测口径 | 多数据集 eval suite 的数据集、阈值和基线。 | 建 fixture、指标、release report；百分比只作为目标门槛。 | 不能把目标百分比写成已通过事实。 |

## 2. 设计结论

LLM 在通灵玉系统中只能做结构化辅助：理解、分类、摘要、检索意图建议、证据观察、草稿候选、review observation、memory semantic filter 和知识校准 judge。

LLM 不能做以下事情：

1. 不能作为事实源。
2. 不能决定 Gateway 鉴权、限流、请求字段、profile、tool policy 或 Runtime Adapter。
3. 不能决定 scope、ACL、memory read enablement、reviewer 裁决或 evidence package 写入。
4. 不能把 session summary、memory、用户偏好或模型推断写入 evidence package 当证据。
5. 不能绕过用户响应 wrapper。

因此，本方案的目标不是“让模型回答得更像专家”，而是让每个节点的 LLM 输出都能被 schema、policy、audit、replay 和 eval 约束。

## 3. 当前 user request 到 user response 的主流程

| 阶段 | 当前节点 | 当前职责 | LLM 支持口径 |
|---|---|---|---|
| 0 | Open WebUI request | 用户消息、模型、stream、metadata/header | 无。用户不能指定内部 profile/tool/reviewer/context。 |
| 1 | Gateway auth / rate limit / schema init | 鉴权、限流、DB、runtime schema | 无。必须确定性。 |
| 2 | Forbidden control field gate | 拒绝内部控制字段注入 | 无。必须确定性。 |
| 3 | Request normalization | model allowlist、问题长度、最后 user message、user/chat/message 映射 | 无。必须确定性。 |
| 4 | Dedupe | 按 external message id 复用已保存 final response | 无。必须确定性。 |
| 5 | Session summary | 当前为确定性 summary：最近对象、最近用户问题、长度限制 | 可增强为受控 LLM `conversation_state_summary`，但当前未实现。 |
| 6 | Question Resolver | 当前规则优先；明确实体直接通过，简单指代绑定，不清楚则澄清 | 可在规则不足时调用 LLM resolver，只接受 schema-bound JSON。 |
| 7 | Context pack | 生成 active scopes、candidate scopes、memory read refs、policy versions、resolver audit | 无自由 LLM。只能记录被校验后的 resolver/summary/memory 决策。 |
| 8 | Context projection | 为 `honglou-main/text/commentary/reviewer` 生成最小可见上下文和工具权限 | LLM profile 只能读取自己的 projection，不能读完整 context pack。 |
| 9 | Retrieval policy | 当前由 `search_policy(resolved_question)` 确定 | 可引入 LLM suggested policy，但 deterministic policy patch 必须最终约束高风险证据源。 |
| 10 | Runtime step plan gate | 校验 profile contract、tool policy、dependency、output_ref、context projection digest | 无自由 LLM。plan gate 是执行前约束。 |
| 11 | Text/commentary evidence retrieval | 只读工具返回 evidence cards、quality report、evidence ids | LLM profile 可做 evidence observation，但 evidence refs 必须来自工具输出。 |
| 12 | Evidence package | 本地工具创建 package、claims、review metadata 和 replay anchor | LLM 可提出 package observation 或 claim scaffold；不能写 package。 |
| 13 | Draft | 当前最终回答仍受 Runtime workflow 和本地治理约束 | `honglou-main` 可提供 draft candidate；必须绑定当前 package。 |
| 14 | Reviewer | 本地 reviewer enforcement 是最终裁决点 | `honglou-reviewer` 可输出 review observation；不一致时本地 reviewer 覆盖。 |
| 15 | Final response | `completion_value` 构造内部值，journal 写入 final response | LLM 不直接决定公开字段。 |
| 16 | 用户响应 wrapper / SSE | 删除 trace、package、review、context、memory、LLM 内部字段；stream 只发用户可见 delta | 无。必须确定性。 |

## 4. LLM 支持点总表

| 模块 | 当前状态 | 可使用 LLM 的方式 | 强制边界 |
|---|---|---|---|
| Session Summary | 规则 summary 已存在 | 生成 `conversation_state_summary`：主题、活跃实体、未决问题、上一轮边界、reviewer 警告 | 不能引入事实；不能进入 evidence package；不能给 text/commentary/reviewer 完整可见。 |
| Question Resolver | 规则优先；LLM output contract 尚未落地 | 复杂指代、省略补全、澄清问题生成 | `confidence >= 0.75` 才接受；低置信澄清或 fail-closed；不能决定事实/权限/scope/tool/memory/reviewer/package。 |
| Retrieval Policy | 当前确定性 | 题型、版本敏感度、是否需要 commentary 的建议 | Gateway/Runtime 必须强制 patch 高风险 evidence requirements。 |
| Text Profile | 设计上是 LLM profile；工具为 `tonglingyu.text.search` | 正文证据观察、支持范围、不支持范围 | 不能输出最终回答；不能解释脂批；refs 必须来自本地 evidence ids。 |
| Commentary Profile | 设计上是 LLM profile；工具为 `tonglingyu.commentary.search` | 脂批/版本证据观察 | 不能把脂批当正文事实；不能输出最终回答。 |
| Main Profile | 设计上是 LLM profile；当前受本地 workflow 治理 | claim-first draft candidate、分层回答草稿 | 必须绑定 package id；不能绕过 reviewer；不能伪造证据。 |
| Reviewer Profile | 设计上是 LLM profile；本地 reviewer enforcement authoritative | review status/severity/issues/required revisions observation | 不能重写最终答案；不能改变 evidence package；不能替代本地裁决。 |
| Memory Collector | 已有 collector/policy 主线 | LLM 辅助抽取 candidate 或 semantic filter | 输入必须 redacted；输出只能 schema JSON；不能 approve/promote/enable_read。 |
| Scoped Memory Read | 已有 read-enabled memory path | 可做摘要去重、排序建议、风险标记 | ACL、scope、budget、read enablement 由 policy engine 决定。 |
| Knowledge Calibration | 已有 LLM evidence judge 接口 | 判断候选知识是否被证据支持 | 不能写事实表；结果还要过 rule/eval/reviewer/release gate。 |
| 用户响应安全检查 | 当前确定性过滤 | 可作为额外 observation 检查泄露/无证据断言 | 用户响应 wrapper 必须最终确定性执行。 |

## 5. Question Resolver 目标 schema

若后续落地 Question Resolver LLM contract，schema version 必须使用：

```json
{
  "schema_version": "tonglingyu-question-resolver-v1",
  "resolved_question": "晴雯的判词和晴雯结局有什么关系？",
  "referent_bindings": ["晴雯"],
  "used_context_refs": ["session_summary"],
  "confidence": 0.93,
  "needs_clarification": false,
  "clarification_question": null,
  "unsupported_reason": null
}
```

目标 contract 允许的 `used_context_refs` 集合：

1. `current_question`
2. `recent_user_messages`
3. `recent_assistant_messages`
4. `prior_subject`
5. `session_summary`

目标 contract 不允许的字段：

1. `answer`
2. `final_answer`
3. `facts`
4. `scope`
5. `tool_policy`
6. `allowed_tools`
7. `forbidden_tools`
8. `acl`
9. `memory_acl`
10. `reviewer_decision`
11. `evidence_package_id`
12. `promotion`
13. `read_enabled`
14. `system_prompt`

目标处理规则：

1. 先走 deterministic resolver。
2. 只有 deterministic resolver 需要澄清时，才允许调用 LLM resolver。
3. LLM 输出必须是 JSON。
4. schema invalid、未知 context ref、越权字段、低置信都不得进入 RAG。
5. `confidence >= 0.75` 才接受。
6. `0.45 <= confidence < 0.75` 必须返回澄清问题。
7. `< 0.45` fail-closed。

## 6. Session Summary 设计

当前实现只有确定性 `session_summary`，可作为 `session_hint` 使用。目标增强是增加 `conversation_state_summary`，但它必须是新增节点，不是当前已存在能力。

| 层级 | 当前状态 | 用途 | 可见范围 |
|---|---|---|---|
| `session_summary` / `session_hint` | 已存在，确定性 | 最近讨论对象、最近用户问题、规则 fallback | Question Resolver、`honglou-main` |
| `conversation_state_summary` | 未实现，目标增强 | 当前主题、活跃实体、未决问题、上一轮边界、reviewer 警告 | 只能给 Question Resolver 和 `honglou-main` 的受控 projection |

`conversation_state_summary` 不能作为证据源，不能进入 evidence package，不能让 text/commentary/reviewer 看到完整会话历史。

目标 schema：

```json
{
  "object": "tonglingyu.conversation_state_summary",
  "schema_version": "v1",
  "current_topic": "晴雯判词与人物命运",
  "active_entities": ["晴雯"],
  "open_questions": ["判词是否指向晴雯结局"],
  "last_answer_boundaries": ["上一轮只确认判词位置"],
  "evidence_package_refs": ["package:..."],
  "reviewer_warnings": [],
  "memory_allowed_as_evidence": false,
  "summary_confidence": 0.92
}
```

## 7. Context Pack 与 Context Projection

当前已实现的边界：

1. `context_pack` 是请求/trace 级受控上下文包，用于审计、回放和生成 projection。
2. Runtime profile 不直接读取完整 `context_pack`。
3. `context_projection` 是 Runtime 可见上下文。
4. 每个 consumer 只能读取自己的 projection。
5. `honglou-text` 和 `honglou-commentary` 不应看到完整 session summary。
6. `honglou-reviewer` 不应看到 user_private memory、未审核 candidate 或 Hermes 私有 transcript。
7. projection 绑定 `tool_policy_digest`、`output_contract_digest`、pack/projection ref 和 digest。

Consumer 可见性：

| Consumer | 当前可见内容 | 禁止内容 |
|---|---|---|
| `honglou-main` | resolved question、有限 session summary、授权 memory summaries、package tool | 完整用户历史、未授权 memory、系统提示词、未审核 candidate |
| `honglou-text` | resolved question、text search、部分非 user_private 工具偏好 memory | 完整 session summary、user_private memory、commentary tool |
| `honglou-commentary` | resolved question、commentary search、部分非 user_private 工具偏好 memory | 完整正文库、user_private memory、最终回答 |
| `honglou-reviewer` | visible question、evidence package read、review usage summary | user_private memory、未审核 candidate、Hermes private transcript |

## 8. Retrieval Policy 与证据型 RAG

当前主路径是确定性 `search_policy(resolved_question)`。后续可以加入 LLM suggested policy，但必须满足：

1. LLM 只能建议题型、检索意图、别名扩展、版本敏感度。
2. 高风险问题由 deterministic policy patch 强制补齐证据类型。
3. 必需证据类型不能被 LLM 降级。
4. 用户不能通过请求字段指定内部 profile、tool choice 或 reviewer 状态。

| 问题类型 | 必需证据 | LLM 可支持 | 硬规则 |
|---|---|---|---|
| 原文定位 | 原文库、回目结构 | 别名识别、查询扩展 | 不强制脂批；不可凭记忆回答。 |
| 诗词判词 | 诗词判词曲文库、原文库 | 专题识别、文本定位 | 解释必须区分原文与解读。 |
| 人物关系 | 人物库、关系库、正文证据 | 关系意图拆解 | 关系必须可追溯。 |
| 人物命运 | 原文、脂批、版本、事件 | 版本敏感计划 | 必须区分前八十回、后四十回、脂批、推断。 |
| 脂批问题 | 脂批、对应正文、版本 | 批语意图识别 | 脂批不可写成正文事实。 |
| 版本差异 | 版本库、对齐正文、脂批 | 差异维度拆解 | 禁止无版本标签结论。 |

## 9. Evidence Package、Draft、Reviewer 与 Response

### 9.1 Evidence Package

证据包必须是本地 Runtime/tool 创建和回放的对象。LLM 可以生成 claim scaffold 或 package observation，但 evidence refs 必须来自本地工具返回的 evidence ids。

Evidence package 必须表达：

1. `package_id`
2. `trace_id`
3. `question` / `resolved_question`
4. `cards`
5. `claims`
6. `claim_evidence_map`
7. `review`
8. replay metadata

### 9.2 Draft

`honglou-main` 可做 claim-first draft candidate。若使用 Hermes/LLM draft：

1. 必须绑定当前 evidence package id。
2. 必须提供非空 draft。
3. 不得引入 package 外 evidence ref。
4. 不得绕过 reviewer。
5. 本地治理不接受时，不能进入 final answer。

### 9.3 Reviewer

Reviewer 分两层：

| 层级 | 职责 | 裁决权 |
|---|---|---|
| deterministic / local reviewer enforcement | 拦截硬规则错误、版本边界、无证据 claim、内部泄露 | 最终裁决 |
| LLM reviewer observation | 判断语义支持、措辞过度、需要修订项 | 观察和建议 |

LLM reviewer 与本地 reviewer 不一致时，必须记录 override，最终以本地 reviewer 为准。

### 9.4 用户响应

用户响应只能保留 OpenAI-compatible 用户可读内容。必须移除：

1. `trace_id`
2. `evidence_package_id`
3. `review`
4. `session_id`
5. `user_session_id`
6. `interaction_context_id`
7. `context_pack_id`
8. `context_pack_ref`
9. `context_projection_id`
10. `context_projection_ref`
11. `memory_read_refs`
12. `memory_read_ref_digest`
13. `memory_policy`
14. `memory_candidate`
15. `memory_card`
16. `llm_extraction`
17. `llm_filter`
18. `rule_filter`
19. `_runtime_stream_events`

## 10. Memory 与 LLM

当前 Scoped Memory Production 的正确口径：

1. Memory 可以进入 `context_pack.memory_read_refs` 和 `context_projection`。
2. Memory 不能进入 evidence package。
3. Memory 不能成为事实源。
4. Memory 不能改变 reviewer 裁决。
5. LLM 只能做 semantic filter、分类、TTL 建议和风险标记。
6. `auto_approve`、`auto_promote`、`enable_read` 只能由 versioned policy engine 决定。

LLM semantic filter schema 使用 `scoped-memory-llm-filter-v1`。输出包含：

```json
{
  "schema_version": "scoped-memory-llm-filter-v1",
  "is_long_term_memory": true,
  "is_temporary_instruction": false,
  "is_quoted_or_third_party": false,
  "has_contradiction": false,
  "scope_type": "user_private",
  "candidate_type": "answer_style_preference",
  "confidence": 0.91,
  "sensitivity": "low",
  "risk_flags": [],
  "ttl_hint": "90d",
  "exclusion_flags": []
}
```

输出包含 `approve`、`promote`、`read_enabled`、`acl`、`reviewer_decision`、`evidence_package_id` 或任务状态字段时，必须 fail-closed。

## 11. 全路径 Eval 方案

Eval 必须按节点归因，不只评最终回答。

| 数据集 | 覆盖内容 | 核心指标 |
|---|---|---|
| `request_safety.jsonl` | forbidden fields、model allowlist、body/message/question limit | reject accuracy、no false accept |
| `session_summary.jsonl` | 长对话、多人物、上一轮边界、metadata prompt | active entity recall、boundary preservation、hallucination rate |
| `question_resolution.jsonl` | 指代、省略、歧义、prompt injection、低置信 | canonical accuracy、false resolution、clarification recall |
| `context_projection.jsonl` | consumer isolation、digest mismatch、unknown consumer | projection isolation、fail-closed |
| `retrieval_policy.jsonl` | 题型、版本、脂批、人物命运 | required evidence recall、policy patch correctness |
| `rag_evidence.jsonl` | gold evidence、别名、版本、脂批对应正文 | hit@k、source/version accuracy |
| `package_claims.jsonl` | evidence package、claim map、replay | package replay、unsupported claim rate |
| `reviewer_security.jsonl` | 无证据断言、脂批正文混淆、内部泄露 | high-risk false pass |
| `memory_policy.jsonl` | candidate extraction、semantic filter、ACL、read budget | policy decision correctness、no evidence misuse |
| `streaming_dedupe.jsonl` | SSE、缓存复用、用户可见字段过滤 | response consistency、no internal leakage |

## 12. 发布门槛

| Gate | 必须通过 |
|---|---|
| G1 Request Gate | 鉴权、限流、model、字段、body size、message count 全部正确处理。 |
| G2 Summary Gate | summary 无事实幻觉、无内部泄露、边界保留达标。 |
| G3 Resolution Gate | resolved question 正确；歧义必须澄清；无新事实、无答案性断言。 |
| G4 Projection Gate | profile 只能看到自己的 projection；digest mismatch fail-closed。 |
| G5 Retrieval Gate | 高风险问题强制必需证据源；gold evidence hit 达标。 |
| G6 Package Gate | package 可回放；claim-evidence map 完整；refs 来自本地 evidence ids。 |
| G7 Review Gate | LLM reviewer observation 与本地 reviewer enforcement 均可审计；本地裁决最终有效。 |
| G8 Memory Gate | memory 不进 evidence package；policy engine 才能 enable read。 |
| G9 用户响应安全 Gate | 用户响应和 SSE 无内部字段、prompt、tool payload、memory/ref 泄露。 |
| G10 Release Gate | 本地测试、clippy、smoke、strict Gateway gate、live gate、saved validator 和 release readiness 按对应阶段全部通过。 |

## 13. 实施路线

| 优先级 | 工作包 | 当前判断 | 交付物 |
|---|---|---|---|
| P0 | Question Resolver LLM contract | 目标增强，当前尚未落地；外部 LLM 调用未接 | resolver runtime/Hermes 调用、schema repair、audit、eval |
| P1 | Conversation State Summary | 目标增强，当前未实现 | summary writer/loader、schema、anti-hallucination eval |
| P2 | Retrieval Policy schema 化 | 目标增强，当前主路径确定性 | LLM suggested policy、deterministic patch、policy eval |
| P3 | Profile observation eval | Runtime/Hermes 已有受控接口和 observation 边界 | text/commentary/main/reviewer observation datasets |
| P4 | Claim-first draft + reviewer eval | 当前有本地 package/reviewer 约束 | claim map eval、reviewer false-pass eval、revision gate |
| P5 | Full-path Eval Suite | 目标增强 | 多数据集、节点级失败归因、release report |
| P6 | 用户响应脱敏与泄露回归门禁 | 横切安全门禁，不属于 LLM 能力；当前已有过滤，需要持续守护 | recursive internal-field scan、stream replay checks |

### 13.1 实施路线的待确认口径

| 编号 | 待确认问题 | 确认依据 | 确认后动作 | 主要风险 |
|---|---|---|---|---|
| P0 | 是否实现 Question Resolver LLM contract。 | 需要产品上确认复杂指代/省略补全是否值得引入 LLM；工程上确认 resolver 调用点、schema repair 和 fail-closed 策略。 | 增加 contract、runtime/Hermes 调用、audit 字段、`question_resolution.jsonl`。 | LLM resolver 若越权，会把 memory、scope、tool 或事实判断提前带入 RAG。 |
| P1 | 是否新增 Conversation State Summary。 | 需要确认多轮体验是否需要比当前 `session_summary` 更强的状态表达。 | 增加 summary writer/loader、schema、可见范围、anti-hallucination eval。 | summary 幻觉会污染 resolver、draft 或 memory 判断。 |
| P2 | 是否将 Retrieval Policy schema 化并引入 LLM suggested policy。 | 需要确认问题类型、版本敏感度、脂批需求是否难以靠确定性规则覆盖。 | 定义 suggested policy schema、deterministic patch、required evidence eval。 | LLM 降级必需证据，会让版本/脂批/人物命运问题失去证据约束。 |
| P3 | 是否为 profile observation 建 eval。 | 需要确认 `text/commentary/main/reviewer` 的输出 contract 是否稳定。 | 建 profile observation datasets，校验 refs、边界、不可最终裁决。 | profile observation 被误用为事实源或最终回答。 |
| P4 | 是否推进 claim-first draft 和 reviewer eval。 | 需要确认 final answer 是否必须由 claim map 驱动。 | 建 claim map eval、reviewer false-pass eval、revision gate。 | draft 绕过 evidence package 或 reviewer，形成无证据回答。 |
| P5 | 是否建设 full-path eval suite。 | 需要确认 release 是否要求节点级失败归因，而不是只看最终答案。 | 建多数据集、统一 runner、失败归因和 release report。 | 只评最终回答会掩盖 resolver、summary、policy、package、reviewer 的局部失败。 |
| P6 | 是否把用户响应脱敏与泄露回归设为长期门禁。 | 需要确认用户响应安全是否纳入每次发布检查。 | 增加递归内部字段扫描、stream replay、缓存复用检查。 | 内部 trace、context、memory、review、tool payload 泄露到普通用户响应。 |

## 14. 节点级失败归因

失败必须归因到具体节点：

1. resolved question 错导致 RAG 错，记为 Resolution failure，不记为 RAG failure。
2. summary 引入新事实导致后续错误，记为 Summary hallucination。
3. LLM suggested policy 漏掉版本/脂批必需证据，但 deterministic patch 未补齐，记为 Policy patch failure。
4. evidence refs 不来自本地 evidence ids，记为 Package/ref validation failure。
5. reviewer 放过无证据 claim，记为 Reviewer false pass。
6. 用户响应或 SSE 泄露 context/memory/internal fields，记为 User response wrapper failure。
7. memory 被写进 evidence package 或改变 reviewer 裁决，记为 Memory boundary failure。

## 15. 最终判断标准

第一版成功标准不是回答是否流畅，而是：

1. 多轮场景下 resolved question 能正确承接意图，歧义时主动澄清。
2. summary 保留必要上下文和边界，不引入事实。
3. 检索和证据包能支撑关键 claim。
4. LLM draft/reviewer/memory/filter 输出全部有 schema、policy、audit 和 fail-closed。
5. 最终用户响应经过 reviewer 和用户响应 wrapper，不泄露内部状态。
6. 每次失败都能在 trace 中归因到具体节点。

<!-- markdownlint-enable MD013 MD060 -->
