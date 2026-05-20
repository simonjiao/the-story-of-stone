# the-story-of-stone LLM 支持点与全路径 Eval 设计候选稿

<!-- markdownlint-disable MD013 MD060 -->

版本：v1.2
日期：2026-05-20
结论：本文只定义设计约束、决策边界、目标 contract、eval gate 和阶段实施边界；不记录实现进度，不替代实现证据或上线证据。

## 0. 文档边界与禁止口径

本文的第一原则是：没有证据就不宣布完成；没有 gate 通过就不宣布 ready；有不一致就明确指出，不猜测、不折中。

本文不使用状态表记录实现进度。实现进度应放在 checklist、progress、release report 或提交记录中；本文件只回答“设计上允许什么、禁止什么、如何验证”。

禁止口径：

1. “基本完成”。
2. “大体完成”。
3. “差不多完成”。
4. “可以认为完成”。
5. “先算完成”。
6. “设计写了，所以已经实现”。
7. “设计允许，所以当前链路已经支持”。

## 1. 证据口径

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

外部实现只能作为工程模式参考。OpenAI、Anthropic、LangGraph、Hermes/AiBot 等不能作为本仓库当前实现事实。

可借鉴的只是模式：

| 外部模式 | 可借鉴 | 必须重新落到本仓库的约束 |
|---|---|---|
| OpenAI / Agents | structured output、tool call、trace、eval、single entry orchestration | schema、policy、audit、eval、release gate |
| Anthropic / MCP / Skills | 最小工具权限、上下文隔离、能力包版本化 | context projection、tool policy、能力版本、权限审计 |
| LangGraph / LangSmith | state graph、节点 I/O、replay、失败归因 | request -> response 全路径节点级 eval |
| Hermes / AiBot | profile 隔离、runtime client、tool registry、session/runtime audit | Runtime/Gateway contract、output_ref、hhost gate |

## 2. 不可变设计原则

以下原则不参与折中：

1. LLM 不能作为事实源。
2. LLM 不能决定 Gateway 鉴权、限流、请求字段、profile、tool policy 或 Runtime Adapter。
3. LLM 不能决定 scope、ACL、memory read enablement、reviewer 裁决或 evidence package 写入。
4. LLM 不能把 session summary、memory、用户偏好或模型推断写入 evidence package 当证据。
5. LLM 不能绕过用户响应 wrapper。
6. Runtime profile 的 LLM 输出只能是 observation 或 candidate，不能是最终事实或最终裁决。
7. evidence refs 必须来自本地 Runtime/tool 返回的 evidence ids。
8. memory 只能进入受控 context/projection，不能进入 evidence package。
9. resolver 若读取 memory，只能读取 policy engine 授权、脱敏、预算内的摘要，不能读取 raw memory、memory card id、ACL 或 read refs。
10. 用户响应与 SSE 必须由确定性 wrapper 过滤内部字段。
11. 每个 LLM 输出都必须 schema-bound、audit-bound、replayable、fail-closed。

## 3. 基线流程与 LLM 可插入点

基线链路按节点治理。LLM 只能插在允许的节点，且必须受 contract 约束。

| 阶段 | 节点 | 职责 | LLM 口径 |
|---|---|---|---|
| 0 | Open WebUI request | 用户消息、模型、stream、metadata/header | 不允许。用户不能指定内部 profile/tool/reviewer/context。 |
| 1 | Gateway auth / rate limit / schema init | 鉴权、限流、DB、runtime schema | 不允许。必须确定性。 |
| 2 | Forbidden control field gate | 拒绝内部控制字段注入 | 不允许。必须确定性。 |
| 3 | Request normalization | model allowlist、问题长度、最后 user message、user/chat/message 映射 | 不允许。必须确定性。 |
| 4 | Dedupe | 按 external message id 复用已保存 final response | 不允许。必须确定性。 |
| 5 | Session summary | 基线为确定性 summary | 设计可选项：受控 `conversation_state_summary`。 |
| 6 | Pre-resolver memory authorization | 目标新增节点：在 resolver 前由 policy engine 决定是否给 resolver 可见 memory summary | LLM 不参与。只生成授权、脱敏、预算内摘要。 |
| 7 | Question Resolver | 规则优先，歧义 fail-closed 澄清 | 设计可选项：只在规则不足时调用 LLM resolver；可读取受控 memory summary。 |
| 8 | Context pack | active scopes、candidate scopes、memory read refs、policy versions、resolver audit | 不允许自由 LLM。只能记录已校验决策。 |
| 9 | Context projection | 为不同 Runtime profile 生成最小可见上下文和工具权限 | LLM profile 只能读自己的 projection。 |
| 10 | Retrieval policy | 当前由 `search_policy(resolved_question)` 确定 | 目标增强：LLM suggested policy，deterministic patch 最终约束。 |
| 11 | Runtime step plan gate | 校验 profile contract、tool policy、dependency、output_ref、projection digest | 不允许自由 LLM。 |
| 12 | Text/commentary retrieval | 只读工具返回 evidence cards、quality report、evidence ids | LLM profile 可做 evidence observation，不能写 evidence。 |
| 13 | Evidence package | 本地工具创建 package、claims、review metadata、replay anchor | LLM 可提出 claim scaffold，不能写 package。 |
| 14 | Draft | 最终回答受 Runtime workflow 和本地治理约束 | `honglou-main` 可提供 draft candidate，必须绑定 package。 |
| 15 | Reviewer | 本地 reviewer enforcement 是最终裁决点 | `honglou-reviewer` 可输出 observation，不可替代本地裁决。 |
| 16 | Final response | `completion_value` 构造内部值，journal 写入 final response | LLM 不直接决定公开字段。 |
| 17 | 用户响应 wrapper / SSE | 删除 trace、package、review、context、memory、LLM 内部字段 | 不允许 LLM。必须确定性。 |

## 4. 事实纠偏与待确认项

### 4.1 必须纠偏的不一致

| 编号 | 原稿或讨论中容易误写的说法 | 当前证据口径 | 处理 |
|---|---|---|---|
| I1 | Question Resolver schema 是 `tonglingyu-question-resolver-v2` | 当前代码常量是 `tonglingyu-question-resolver-v1` | 必须统一为 v1；v2 只能作为未来迁移。 |
| I2 | LLM resolver 可使用 `session_hint`、`conversation_state_summary` 等任意 context refs | 未来 contract 必须使用白名单；D3 已允许 resolver 读取受控 memory summary | 不能默认允许 raw memory、完整 history 或任意 session hint。 |
| I3 | `conversation_state_summary` 已在链路中存在 | 当前只有确定性 `session_summary` | 只能写成目标增强。 |
| I4 | Question Resolver 当前可见受控 memory summary | 基线 resolver 在 memory read 之前执行；D3 已决策允许目标设计读取受控 memory summary | 不能把 D3 写成无需新增节点；必须新增 pre-resolver memory authorization 并补 ACL/budget/fail-closed。 |
| I5 | 显式 Question Resolution 完全未实现 | 当前已有 `resolved_question`、context pack、admin trace、fail-closed 澄清 | 正确口径：显式节点已存在，LLM resolver 未生产接入。 |
| I6 | 四个 Runtime profile 是完整 LLM 生成链 | 当前仍受本地工具、package、reviewer enforcement 约束 | profile 是 observation/candidate，不是最终事实链。 |
| I7 | 27/28 checklist 的 active memory 未实现代表全局事实 | 30 checklist 和 PROGRESS 属于后续阶段 | 必须按阶段解释，不能用旧阶段覆盖新事实。 |
| I8 | Retrieval policy 当前由 LLM suggested policy 决定 | 当前主路径是确定性 `search_policy(resolved_question)` | LLM suggested policy 只能作为目标增强。 |
| I9 | Evidence package 示例可写 `review: null` | 当前主链路需要 review/journal/wrapper 过滤 | 示例必须表达 review record 存在或可回放。 |
| I10 | Eval 百分比是已通过事实 | 多数据集 eval suite 尚未整体落地 | 百分比只能作为目标门槛。 |

### 4.2 待确认决策登记

这些决策未关闭前，不能宣布设计完成。

| 编号 | 决策问题 | 设计结论 | 必须落实的边界 | 若边界不清的风险 |
|---|---|---|---|---|
| D1 | 是否实现 Question Resolver LLM contract | 待定 | 若做，必须确认 schema、调用点、fail-closed、audit、eval | LLM resolver 可能越权进入事实、scope、tool、memory 判断 |
| D2 | LLM resolver 能读取哪些 context refs | 待定 | 必须使用白名单；候选白名单包含 `authorized_memory_summary`；未知 ref fail-closed | context 膨胀或越权读取 raw memory/full history |
| D3 | resolver 是否允许读取 memory summary | 允许 | 只能读取 policy engine 授权、脱敏、预算内的 `authorized_memory_summary` | 把“设计允许”误写成当前支持，或让 resolver 读取 raw memory |
| D4 | 是否新增 `conversation_state_summary` | 待定 | 若新增，必须确认 writer/loader/schema/projection | summary 幻觉污染 resolver 或 draft |
| D5 | 是否引入 LLM suggested retrieval policy | 待定 | 必须确认 suggested policy schema 与 deterministic patch | LLM 降级版本/脂批/人物命运必需证据 |
| D6 | Runtime profile 输出权力边界 | 必须限制 | 每个 profile 只能输出 observation/candidate | profile 输出被误用为事实源 |
| D7 | Evidence package review record 形态 | 必须存在或可回放 | review record 形态必须明确 | reviewer 可被绕过 |
| D8 | Claim-first draft 是否作为 final answer 主路径 | 待定 | 必须明确 draft/package/reviewer 绑定关系 | draft 引入无证据 claim |
| D9 | LLM reviewer observation 与本地 reviewer 冲突时如何记录 | 必须记录 override | override audit schema 和裁决规则必须明确 | 语义 reviewer 被误当最终裁决 |
| D10 | Full-path eval suite 的数据集、阈值和 runner | 必须定义 | fixture、指标、runner、release report 必须明确 | 只看最终回答，无法定位节点失败 |
| D11 | 用户响应脱敏回归是否作为长期门禁 | 必须作为长期门禁 | denylist、recursive scan、SSE replay、cache replay 必须明确 | 内部 trace/context/memory/tool payload 泄露 |
| D12 | 目标环境 release gate 的边界 | 必须分层 | 本地 gate、smoke、strict gateway、live gate、release readiness 必须分层明确 | repo-local 通过被误写成目标环境 production ready |

## 5. LLM 支持面设计

| 支持面 | 基线事实 | 目标用法 | 强制边界 | 所属阶段 |
|---|---|---|---|---|
| Session Summary | 确定性 summary 已存在 | 可选新增 `conversation_state_summary` | 不引入事实，不进 evidence package，不给 text/commentary/reviewer 完整可见 | S4 |
| Question Resolver | 规则 resolver 是基线；LLM contract 是目标设计 | 复杂指代、省略补全、澄清问题生成；可读取受控 memory summary | 规则优先；只接受 schema JSON；低置信澄清或 fail-closed；不能读取 raw memory | S2-S3 |
| Retrieval Policy | 确定性策略为基线 | 题型、版本敏感度、脂批需求建议 | LLM 只建议；deterministic patch 强制必需证据 | S5 |
| Text Profile | Runtime profile 方向存在 | 正文证据 observation | 不能输出最终回答；refs 必须来自本地 evidence ids | S6 |
| Commentary Profile | Runtime profile 方向存在 | 脂批/版本 observation | 脂批不能写成正文事实 | S6 |
| Main Profile | Runtime workflow 治理下的 candidate | claim-first draft candidate | 必须绑定 package id；不能绕过 reviewer | S6 |
| Reviewer Profile | 本地 reviewer enforcement authoritative | review observation、severity、required revisions | 不能替代本地裁决 | S6 |
| Memory Collector / Filter | Scoped memory 主线存在 | semantic filter、分类、TTL 建议、风险标记 | 不能 approve/promote/enable_read | 现有能力延续，S7 回归 |
| Knowledge Calibration | 已有 LLM evidence judge 接口方向 | 候选知识证据支持判断 | 不能写事实表，仍需 rule/eval/reviewer gate | S7 回归 |
| 用户响应安全检查 | 确定性过滤为基线 | 泄露检测 observation | 最终 wrapper 必须确定性执行 | S1-S7 |

## 6. 目标 Contract

### 6.1 Question Resolver LLM Contract

设计目标：若使用 LLM resolver，schema version 必须使用：

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

允许的 `used_context_refs`：

1. `current_question`
2. `recent_user_messages`
3. `recent_assistant_messages`
4. `prior_subject`
5. `session_summary`
6. `authorized_memory_summary`

禁止字段：

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

处理规则：

1. 先走 deterministic resolver。
2. 只有 deterministic resolver 需要澄清时，才允许调用 LLM resolver。
3. 若读取 memory，只能读取 pre-resolver memory authorization 产出的 `authorized_memory_summary`。
4. `authorized_memory_summary` 必须是授权、脱敏、预算内摘要；不能包含 raw memory、memory card id、ACL 或 read refs。
5. schema invalid、未知 context ref、越权字段、低置信都不得进入 RAG。
6. `confidence >= 0.75` 才接受。
7. `0.45 <= confidence < 0.75` 必须返回澄清问题。
8. `< 0.45` fail-closed。

### 6.2 Conversation State Summary Contract

设计可选项：是否新增由 D4 决定。若新增，目标 schema 为：

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

硬规则：

1. 不能作为证据源。
2. 不能进入 evidence package。
3. 不能让 text/commentary/reviewer 看到完整会话历史。
4. 只能进入授权 projection。
5. 必须有 anti-hallucination eval。

### 6.3 Retrieval Policy Suggestion Contract

设计可选项：若新增 retrieval policy suggestion，LLM 只能输出 suggestion，不能输出最终 policy。

```json
{
  "schema_version": "tonglingyu-retrieval-policy-suggestion-v1",
  "question_type": "character_fate",
  "alias_expansions": ["晴雯"],
  "version_sensitive": true,
  "commentary_recommended": true,
  "confidence": 0.86,
  "unsupported_reason": null
}
```

禁止字段：

1. `required_evidence_final`
2. `tool_choice`
3. `profile`
4. `reviewer_state`
5. `skip_review`
6. `final_answer`

deterministic patch 必须强制补齐：

| 问题类型 | 必需证据 | 不可降级规则 |
|---|---|---|
| 原文定位 | 原文库、回目结构 | 不可凭模型记忆回答 |
| 诗词判词 | 诗词判词曲文库、原文库 | 必须区分原文与解读 |
| 人物关系 | 人物库、关系库、正文证据 | 关系必须可追溯 |
| 人物命运 | 原文、脂批、版本、事件 | 必须区分前八十回、后四十回、脂批、推断 |
| 脂批问题 | 脂批、对应正文、版本 | 脂批不可写成正文事实 |
| 版本差异 | 版本库、对齐正文、脂批 | 禁止无版本标签结论 |

### 6.4 Evidence Package / Draft / Reviewer Contract

Evidence package 必须由本地 Runtime/tool 创建和回放。LLM 可以生成 claim scaffold 或 package observation，但 evidence refs 必须来自本地工具返回的 evidence ids。

Evidence package 必须表达：

1. `package_id`
2. `trace_id`
3. `question` / `resolved_question`
4. `cards`
5. `claims`
6. `claim_evidence_map`
7. `review`
8. replay metadata

Draft candidate 必须满足：

1. 绑定当前 evidence package id。
2. 提供非空 draft。
3. 不引入 package 外 evidence ref。
4. 不绕过 reviewer。
5. 本地治理不接受时，不能进入 final answer。

Reviewer 分两层：

| 层级 | 职责 | 裁决权 |
|---|---|---|
| deterministic / local reviewer enforcement | 拦截硬规则错误、版本边界、无证据 claim、内部泄露 | 最终裁决 |
| LLM reviewer observation | 判断语义支持、措辞过度、需要修订项 | 观察和建议 |

### 6.5 Memory 与 LLM Contract

Scoped Memory 的设计口径：

1. Memory 可以进入 `context_pack.memory_read_refs` 和 `context_projection`。
2. Memory 不能进入 evidence package。
3. Memory 不能成为事实源。
4. Memory 不能改变 reviewer 裁决。
5. LLM 只能做 semantic filter、分类、TTL 建议和风险标记。
6. `auto_approve`、`auto_promote`、`enable_read` 只能由 versioned policy engine 决定。

LLM semantic filter 输出包含 `approve`、`promote`、`read_enabled`、`acl`、`reviewer_decision`、`evidence_package_id` 或任务状态字段时，必须 fail-closed。

### 6.6 用户响应 Contract

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

用户响应脱敏与泄露回归是横切安全门禁，不属于 LLM 能力，但必须从 S1 到 S7 持续运行。

## 7. Eval 设计

Eval 必须按节点归因，不只评最终回答。

| 数据集 | 覆盖内容 | 核心指标 | 首次阶段 |
|---|---|---|---|
| `request_safety.jsonl` | forbidden fields、model allowlist、body/message/question limit | reject accuracy、no false accept | S1 |
| `streaming_dedupe.jsonl` | SSE、缓存复用、用户可见字段过滤 | response consistency、no internal leakage | S1 |
| `question_resolution.jsonl` | 指代、省略、歧义、prompt injection、低置信 | canonical accuracy、false resolution、clarification recall | S2 |
| `session_summary.jsonl` | 长对话、多人物、上一轮边界、metadata prompt | active entity recall、boundary preservation、hallucination rate | S4 |
| `retrieval_policy.jsonl` | 题型、版本、脂批、人物命运 | required evidence recall、policy patch correctness | S5 |
| `rag_evidence.jsonl` | gold evidence、别名、版本、脂批对应正文 | hit@k、source/version accuracy | S5 |
| `context_projection.jsonl` | consumer isolation、digest mismatch、unknown consumer | projection isolation、fail-closed | S6 |
| `package_claims.jsonl` | evidence package、claim map、replay | package replay、unsupported claim rate | S6 |
| `reviewer_security.jsonl` | 无证据断言、脂批正文混淆、内部泄露 | high-risk false pass | S6 |
| `memory_policy.jsonl` | candidate extraction、semantic filter、ACL、read budget | policy decision correctness、no evidence misuse | S7 |

失败归因规则：

1. resolved question 错导致 RAG 错，记为 Resolution failure，不记为 RAG failure。
2. summary 引入新事实导致后续错误，记为 Summary hallucination。
3. LLM suggested policy 漏掉版本/脂批必需证据，但 deterministic patch 未补齐，记为 Policy patch failure。
4. evidence refs 不来自本地 evidence ids，记为 Package/ref validation failure。
5. reviewer 放过无证据 claim，记为 Reviewer false pass。
6. 用户响应或 SSE 泄露 context/memory/internal fields，记为 User response wrapper failure。
7. memory 被写进 evidence package 或改变 reviewer 裁决，记为 Memory boundary failure。

## 8. Gate 设计

### 8.1 节点级 gate

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

### 8.2 设计可实施 gate

设计进入实现前必须满足：

1. D1-D12 每项都有明确设计结论、禁止边界和验证入口。
2. S0 的事实、目标、待确认项和禁止口径均完成审阅，并留下审阅记录。
3. 每个目标 contract 有 schema、禁止字段、fail-closed 行为、eval fixture 入口。
4. S1 的最小 runner 和用户响应安全基线有具体文件路径计划。
5. 文档不包含“基本完成”类口径，也不记录实现进度。

### 8.3 Production-ready 声明条件

本文只定义 production-ready 声明条件，不评价实现进度。要声明 production-ready，至少需要：

1. S1-S7 均有可复现通过证据。
2. full-path eval suite 有 release report。
3. 用户响应脱敏与泄露回归覆盖非流式、SSE、缓存复用。
4. 本地测试、clippy、smoke、strict Gateway gate 通过。
5. 目标环境 live gate 和 release readiness gate 通过。
6. 所有 blocker 均关闭，或被明确排除在发布范围之外。

## 9. 阶段化实施路线

实施不能按 P0-P6 一次性落地。P0-P5 是 LLM 支持点和 eval 能力，P6 是横切安全门禁。正确做法是先建立尺子和用户响应安全边界，再逐个打开 LLM 支持点，每个阶段都必须能单独验证、提交和回滚。

阶段定义：

| 阶段 | 目标 | 包含工作 | 退出条件 | 不可跨越边界 |
|---|---|---|---|---|
| S0 口径冻结 | 固定事实口径、目标增强和待确认项 | 只整理文档、确认 P0-P6 边界、确认 P6 为横切门禁 | I1-I10、D1-D12、S0-S7 均完成审阅；文档检查通过 | 不能写“基本完成”；不能把目标写成实现 |
| S1 评测与用户响应安全基线 | 先建立最小 gate，再接 LLM | P6 最小回归、P5 最小 runner、`request_safety.jsonl`、`streaming_dedupe.jsonl` | 内部字段扫描、stream replay、缓存复用、request safety 能自动跑 | 不能引入新的 LLM 调用 |
| S2 Question Resolver contract | 只做 resolver 输出约束，不接生产 LLM | P0 schema、字段白名单、context refs 白名单、confidence gate、audit、fixture；包含 `authorized_memory_summary` 输入 contract | 规则 resolver 行为不变；contract tests 通过 | LLM 不能决定事实、scope、tool、memory ACL、reviewer、package |
| S3 Resolver LLM shadow/受控接入 | 在规则 resolver 不足时受控试用 LLM | runtime/Hermes 调用、schema repair、shadow audit、fail-closed | 只在 deterministic 需要澄清时调用；RAG 不被未校验输出驱动 | LLM resolver 不能读取 raw memory 或完整 history |
| S4 Conversation State Summary 决策 | 决定是否新增强状态摘要节点 | P1 writer/loader/schema、可见范围、anti-hallucination eval | summary 不引入事实；只进入授权 projection；不进 evidence package | 不能把 summary 当证据源 |
| S5 Retrieval Policy schema 化 | 让 LLM 只建议检索策略 | P2 suggested policy schema、deterministic patch、required evidence eval | 高风险问题必需证据不被降级 | LLM 不能决定最终证据类型和工具权限 |
| S6 Profile observation 与 draft/reviewer eval | 评估 LLM profile 输出质量和边界 | P3 datasets、P4 claim-first draft、reviewer false-pass eval | refs 全部来自本地 evidence ids；override 可审计 | observation/candidate 不能成为最终事实或最终裁决 |
| S7 全路径发布门禁 | 汇总节点级 eval 和 release report | P5 full-path suite、失败归因、release report、P6 持续回归 | request -> response 可回放；失败能归因到节点 | 不能只用最终回答效果替代节点级验证 |

阶段与工作包映射：

| 工作包 | 所属阶段 | 实施口径 |
|---|---|---|
| P0 Question Resolver LLM contract | S2-S3 | 先 contract，后 shadow/受控接入。 |
| P1 Conversation State Summary | S4 | 先决策是否新增，再实现。 |
| P2 Retrieval Policy schema 化 | S5 | LLM 只建议，deterministic patch 最终约束。 |
| P3 Profile observation eval | S6 | 只评 observation，不放权给最终裁决。 |
| P4 Claim-first draft + reviewer eval | S6 | draft/reviewer 都必须绑定 package 和 refs。 |
| P5 Full-path Eval Suite | S1、S7 | S1 做最小 runner，S7 做完整 suite 和 release report。 |
| P6 用户响应脱敏与泄露回归门禁 | S1-S7 | 横切安全门禁，不属于 LLM 能力，贯穿所有阶段。 |

阶段交付细化：

| 阶段 | 具体任务 | 主要产物 | 最小验证 | 停止条件 |
|---|---|---|---|---|
| S0 | 审阅并冻结事实、目标、待确认项、禁止口径 | 31 号文档、文档地图、决策登记 | markdownlint、diff check、无 Rust 净变更、逐条审阅记录 | 仍有未解释冲突或模糊完成口径 |
| S1 | 建最小 eval runner 和用户响应泄露扫描 | request safety fixture、streaming fixture、denylist、gate report | 用户响应和 SSE 无内部字段泄露 | runner 不稳定或 admin-only 与普通用户面混淆 |
| S2 | 定义 resolver schema 和校验 | contract、unit tests、fixture、audit schema | 规则 resolver 行为不变；非法输出 fail-closed | contract 改变现有 resolver 语义 |
| S3 | 接 LLM resolver shadow/受控路径 | feature flag、shadow audit、schema repair、failure record | shadow 可回放；受控输出才可进入 RAG | LLM 输出绕过 contract |
| S4 | 决策并实现 conversation state summary | schema、writer/loader、projection rule、summary eval | summary 不引入事实，不进 package | summary 被用作证据 |
| S5 | schema 化 retrieval suggestion | suggested policy schema、patch rule、retrieval eval | 必需证据不被降级 | LLM 可决定最终工具或证据类型 |
| S6 | 建 profile observation 和 draft/reviewer eval | datasets、package claim eval、reviewer security eval | refs 来自本地 evidence ids；override 可审计 | profile 输出被用作最终事实 |
| S7 | 全路径 gate 和 release report | full-path runner、failure attribution、release report | 所有阶段 gate 有可复现通过证据 | 只看最终回答，不看节点证据 |

每个阶段提交前必须满足：

1. 只包含该阶段声明范围内的变更。
2. 对应 fixture、单测或脚本能独立运行。
3. 用户响应安全 gate 没有回退。
4. 文档同步更新设计事实和剩余风险。
5. 未通过的 gate 必须写成 blocker，不能写成完成。
6. 本设计文档不记录阶段实现进度；阶段进度必须写入专门 checklist、progress 或 release report。

## 10. 回滚与开关要求

任何引入 LLM 的阶段都必须有回滚路径：

| 能力 | 必须有的开关 | 回滚后应保持 |
|---|---|---|
| LLM resolver | resolver LLM enable/disable、shadow-only | deterministic resolver 主路径可用 |
| conversation state summary | summary writer enable/disable、projection visibility | 原 `session_summary` 可用 |
| suggested retrieval policy | LLM suggestion enable/disable | deterministic `search_policy` 可用 |
| profile observation | per-profile enable/disable | 本地 tools/package/reviewer 可用 |
| draft candidate | draft candidate enable/disable | 本地治理不接受时不进 final answer |
| LLM reviewer observation | reviewer observation enable/disable | local reviewer enforcement 仍为最终裁决 |
| 用户响应安全 gate | 不允许关闭 release gate，只允许增加 denylist | 普通用户响应不泄露内部字段 |

## 11. 第一轮实施建议

第一轮不能接生产 LLM。建议只进入 S1，原因是没有 eval runner 和用户响应安全基线时，后续任何 LLM 接入都无法证明没有回退。

S1 最小范围：

1. 建 `request_safety.jsonl`。
2. 建 `streaming_dedupe.jsonl`。
3. 建用户响应内部字段 denylist。
4. 增加非流式 response replay。
5. 增加 SSE stream replay。
6. 增加 cache/dedupe replay。
7. 输出最小 gate report。

S1 不做：

1. 不接 LLM resolver。
2. 不新增 conversation_state_summary。
3. 不改 retrieval policy。
4. 不改 profile contract。
5. 不改变用户响应内容，只检查泄露。

## 12. 最终判断标准

第一版成功标准不是回答是否流畅，而是：

1. 多轮场景下 resolved question 能正确承接意图，歧义时主动澄清。
2. summary 保留必要上下文和边界，不引入事实。
3. 检索和证据包能支撑关键 claim。
4. LLM draft/reviewer/memory/filter 输出全部有 schema、policy、audit 和 fail-closed。
5. 最终用户响应经过 reviewer 和用户响应 wrapper，不泄露内部状态。
6. 每次失败都能在 trace 中归因到具体节点。

本文只定义判断标准，不替代审阅记录、实现证据或上线证据。

<!-- markdownlint-enable MD013 MD060 -->
