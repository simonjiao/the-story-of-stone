# the-story-of-stone LLM 接入与全路径 Eval 实施前设计冻结稿

<!-- markdownlint-disable MD013 MD060 -->

版本：v1.7
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
| 5 | Session summary | 基线为确定性 summary | 目标新增：受控 `conversation_state_summary`。 |
| 6 | Pre-resolver memory authorization | 目标新增节点：在 resolver 前由 policy engine 决定是否给 resolver 可见 memory summary | LLM 不参与。只生成授权、脱敏、预算内摘要。 |
| 7 | Question Resolver | 规则优先，歧义 fail-closed 澄清 | 目标新增：只在规则不足时调用 LLM resolver；可读取受控 memory summary。 |
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

### 3.1 进入实施前的重构结论

当前设计已经关闭产品决策和策略决策，但不能直接把新增能力继续写进现有大文件。进入实现前必须按模块拆分落地；否则会把 LLM 接入、eval、response safety、context governance 和 CLI wiring 混在一起，形成补丁叠补丁。

实施层强制拆分：

| 目标模块 | 归属 crate | 职责 | 禁止承担 |
|---|---|---|---|
| `llm_contracts.rs` | `tonglingyu-gateway` | schema struct、enum、denylist、threshold 常量 | provider 调用、DB 写入、HTTP handler |
| `llm_modes.rs` | `tonglingyu-gateway` | `disabled/shadow/enforced` 解析、stage guard、rollback defaults | 业务决策、eval 执行 |
| `llm_provider.rs` | `tonglingyu-gateway` | provider-neutral request/response envelope、timeout/error 分类、schema repair 入口、fake provider 测试接口 | prompt 构造策略、事实判断、evidence/package/context mutation |
| `llm_resolver.rs` | `tonglingyu-gateway` | resolver trigger、schema validation、shadow/enforced routing | 事实判断、scope/ACL/tool policy 决策 |
| `conversation_state.rs` | `tonglingyu-gateway` | `conversation_state_summary` writer/loader/projection guard | evidence package 写入、用户响应拼接 |
| `retrieval_suggestion.rs` | `tonglingyu-gateway` | LLM retrieval suggestion schema、fallback、deterministic patch adapter | 最终工具权限、最终 evidence policy |
| `draft_revision.rs` | `tonglingyu-gateway` | draft candidate、revision loop、override audit envelope | 本地 reviewer 裁决、package 写入 |
| `llm_eval.rs` | `tonglingyu-gateway` | `llm-eval` CLI、fixture runner、report schema、threshold matrix | production release 声明 |
| `user_response_safety.rs` | `tonglingyu-gateway` | public response/SSE/cache replay recursive scanner | admin trace 展示、证据包生成 |

现有文件边界：

1. `main.rs` 只允许增加 CLI/route/config wiring，不允许承载 LLM contract 和 eval 业务逻辑。
2. `context_governance.rs` 只保留 context、projection、memory policy 相关原语；不得加入 provider 调用和 eval runner。
3. `tonglingyu-runtime/src/lib.rs` 继续拥有本地证据、检索、evidence package、local reviewer enforcement；LLM 输出不能写入 Runtime 作为事实。
4. `llm_provider.rs` 必须可被 fake provider 替换；contract、runner 和 shadow tests 不能依赖真实外部模型。
5. 持久化与 migration 必须按第 3.5 节执行；任何新增表都必须是独立 migration PR，不能夹带在能力接入 PR 中。

### 3.2 实施序列总线

实现必须沿单一总线推进，不能在多个阶段并行打开 LLM 能力：

```text
request
  -> deterministic request gates
  -> deterministic session_summary
  -> pre-resolver memory authorization
  -> deterministic resolver
  -> optional LLM resolver by trigger and mode
  -> context_pack
  -> context_projection
  -> optional conversation_state_summary writer/loader
  -> deterministic search_policy
  -> optional LLM retrieval suggestion
  -> deterministic policy patch
  -> local retrieval and evidence package
  -> optional profile observations
  -> optional claim-first draft candidate
  -> local reviewer enforcement
  -> optional LLM reviewer observation
  -> revision loop
  -> final assembly
  -> deterministic user response safety wrapper
```

任一 optional LLM 节点失败时，必须按第 10.2 节降级矩阵处理。optional 节点不能把失败包装成正常事实回答。

### 3.3 Audit 与可见性总则

所有 LLM 参与都必须留下 admin-only audit，不得进入普通用户响应。

| event type | 触发点 | 必填字段 |
|---|---|---|
| `llm_resolver_evaluated` | resolver shadow/enforced 调用后 | `mode`、`trigger`、`accepted`、`confidence`、`error_type`、`replay_anchor` |
| `authorized_memory_summary_projected` | pre-resolver memory summary 生成后 | `item_count`、`budget`、`scope_label`、`redaction_applied`、`replay_anchor` |
| `conversation_state_summary_written` | summary writer 成功后 | `summary_confidence`、`projection_visible`、`replay_anchor` |
| `retrieval_suggestion_evaluated` | suggestion patch 前后 | `mode`、`accepted`、`fallback_used`、`policy_patch_status`、`replay_anchor` |
| `llm_profile_observation_recorded` | text/commentary/profile observation 后 | `profile`、`mode`、`output_ref`、`package_ref_allowed`、`replay_anchor` |
| `draft_revision_recorded` | draft 或 revision 生成后 | `draft_candidate_id`、`revision_index`、`package_mutated`、`review_required` |
| `llm_review_override_recorded` | LLM reviewer 与 local reviewer 不一致时 | `local_status`、`llm_status`、`severity`、`override_reason`、`final_decision` |
| `user_response_safety_gate_failed` | wrapper/SSE/cache replay 泄露检测失败 | `surface`、`denylist_category`、`case_id`、`replay_anchor` |

Audit payload 可以包含内部 ref，但必须只出现在 admin-only surface、local report 或 release artifact 中。

### 3.4 Provider Adapter 边界

Provider adapter 是技术边界，不是业务决策点。所有 LLM 能力必须通过同一个 provider-neutral adapter 调用，业务模块只能提交结构化 request 并接收结构化 response。

Provider adapter 必须做到：

1. 隐藏具体 OpenAI/Anthropic/Hermes/AiBot runtime 差异。
2. 统一 timeout、错误枚举、usage、latency、model id 和 finish reason。
3. 只接受已经 projection 过的 `input_json`，不得自己拼接 raw memory、完整历史或未授权 tool payload。
4. 只执行一次 schema repair；repair 输入只能包含原 projection、目标 schema 和 schema error summary，不能扩大上下文。
5. 输出 raw response digest 和 parsed JSON；普通用户响应、fixture report 和 release report 默认不得保存 raw provider text。
6. 支持 fake provider 和 replay provider；S1-S3 的 contract/eval 测试不得依赖真实 provider 可用性。

Provider adapter 不能做：

1. 不能决定 resolver 是否接受。
2. 不能决定 retrieval policy 是否降级或升级。
3. 不能写 evidence package、context pack、projection 或 memory。
4. 不能绕过 `llm_modes.rs` 的 `disabled/shadow/enforced`。
5. 不能把 provider refusal、timeout 或 schema invalid 包装成正常回答。

### 3.5 持久化与 Migration 策略

进入实施前的默认策略是复用现有 journal、projection 和 audit surface；不为 S1 新增 DB migration。

| 阶段 | 默认持久化落点 | 是否允许新增 migration | 约束 |
|---|---|---|---|
| S1 | eval report 文件、测试 fixture、现有 response replay 路径 | 不允许 | 只建立安全 gate，不改变生产数据模型。 |
| S2 | contract tests、fixture、admin-only audit envelope | 不允许 | resolver contract 不能改变现有 resolver 语义。 |
| S3 | 现有 journal/audit surface 记录 shadow/enforced 调用摘要 | 默认不允许 | 若确需新表，必须拆成 migration PR，先落 rollback 和 fixture。 |
| S4 | `context_projections` / journal metadata 记录 summary 可见性和 digest | 默认不允许 | `conversation_state_summary` 先作为 projection/journal payload；只有查询保留需求被证明后才允许独立表。 |
| S5 | audit/report 记录 suggestion 与 deterministic patch 结果 | 不允许 | suggestion 不成为事实表，不写最终 evidence policy。 |
| S6 | audit/report/package-adjacent metadata 记录 draft、revision、review observation | 默认不允许 | 不允许 mutation evidence package；如需长期检索 revision，单独设计 `llm_draft_revisions` migration。 |
| S7 | release report 文件，引用 eval report sha256 | 不允许夹带 | release artifact 只引用 report path/hash/run id，不保存 raw LLM payload。 |

任何 migration PR 必须单独包含：

1. table 名、字段、索引、唯一约束。
2. 写入者、读取者、删除或过期策略。
3. rollback SQL 或等价回滚路径。
4. fixture 和 replay case。
5. admin-only 可见性说明。
6. 与用户响应 safety scanner 的泄露回归。

## 4. 事实纠偏与决策收敛

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

### 4.2 决策登记

以下登记记录已关闭的设计决策。后续实现必须按对应 contract 落地，不能把设计结论重新降级为开放问题。

| 编号 | 决策问题 | 设计结论 | 必须落实的边界 | 若边界不清的风险 |
|---|---|---|---|---|
| D1 | 是否实现 Question Resolver LLM contract | 必须实现，作为 LLM 嵌入流程的入口 contract | schema、trigger enum、fail-closed、audit、eval fixture 按第 6.1 节落地 | LLM resolver 可能越权进入事实、scope、tool、memory 判断 |
| D2 | LLM resolver 能读取哪些 context refs | 采用固定白名单 | 仅允许 `current_question`、`recent_user_messages`、`recent_assistant_messages`、`prior_subject`、`session_summary`、`authorized_memory_summary`；未知 ref fail-closed | context 膨胀或越权读取 raw memory/full history |
| D3 | resolver 是否允许读取 memory summary | 允许 | 只能读取 policy engine 授权、脱敏、预算内的 `authorized_memory_summary`；schema、预算和 audit 可见性按第 6.1.2 节落地 | 把“设计允许”误写成当前支持，或让 resolver 读取 raw memory |
| D4 | 是否新增 `conversation_state_summary` | 新增 | writer、loader、schema、projection 和 anti-hallucination eval 按第 6.2/7.2 节落地；不得进入 evidence package | summary 幻觉污染 resolver 或 draft |
| D5 | 是否引入 LLM suggested retrieval policy | 引入 | suggested policy schema、deterministic patch、fallback 和 required evidence eval 按第 6.3/7.2 节落地 | LLM 降级版本/脂批/人物命运必需证据 |
| D6 | Runtime profile 输出权力边界 | 必须限制 | 每个 profile 只能输出 observation/candidate | profile 输出被误用为事实源 |
| D7 | Evidence package review record 形态 | 必须采用最小 review record | 至少包含 package/draft refs、local/LLM status、severity/issues、required revisions、final decision、override reason、replay anchor | reviewer 可被绕过 |
| D8 | Claim-first draft 是否作为 final answer 主路径 | 采用，作为 LLM 嵌入回答生成流程的主路径 | draft 必须绑定 package、claim map、reviewer；revision loop 按第 6.4.3 节落地 | draft 引入无证据 claim，或绕过 reviewer 直接进入用户响应 |
| D9 | LLM reviewer observation 与本地 reviewer 冲突时如何记录 | 必须记录 override；本地 reviewer 最终裁决 | severity、override reason、冲突矩阵、revision gate 和最终裁决规则按第 6.4.2/6.4.3 节落地 | 语义 reviewer 被误当最终裁决，或 LLM pass 放行本地失败 |
| D10 | Full-path eval suite 的数据集、阈值和 runner | 必须定义 | fixture、指标、runner、release report 和 threshold matrix 按第 7 节落地 | 只看最终回答，无法定位节点失败 |
| D11 | 用户响应脱敏回归是否作为长期门禁 | 必须作为长期门禁 | denylist、recursive scan、SSE replay、cache replay 按第 6.6/7.2 节落地 | 内部 trace/context/memory/tool payload 泄露 |
| D12 | 目标环境 release gate 的边界 | 必须分层 | 本地 gate、smoke、strict gateway、live gate、release readiness 按第 8.3 节落地 | repo-local 通过被误写成目标环境 production ready |

### 4.3 U 项收敛状态

U1-U16 在设计层面全部关闭。后续实现不能重新打开这些问题，只能按本节和第 6/7/10 节的 contract、阈值、开关与失败策略落地。

| 编号 | 关联决策 | 状态 | 设计结论 | 实现落地要求 |
|---|---|---|---|---|
| U1 | D1 | 可关闭 | LLM resolver 只能在允许的 deterministic failure 上触发；prompt injection、forbidden control field、unsupported domain、context over-budget 不能触发 LLM。 | 使用 `llm_resolver_trigger` 枚举和 `question_resolution.jsonl` fixture。 |
| U2 | D2/D3 | 可关闭 | `used_context_refs` 固定为 `current_question`、`recent_user_messages`、`recent_assistant_messages`、`prior_subject`、`session_summary`、`authorized_memory_summary`。 | 无设计缺口；实现必须对未知 ref fail-closed。 |
| U3 | D3 | 可关闭 | `authorized_memory_summary` 固定为最多 5 条、总长 800 字符、单条 160 字符的脱敏摘要；只暴露 coarse `scope_label`，不向 LLM 暴露 audit ref。 | 使用 `tonglingyu.authorized_memory_summary.v1` schema；audit ref 只写 admin audit。 |
| U4 | D4 | 可关闭 | 当前设计新增 `conversation_state_summary`；它不能作为事实源，不能进 evidence package，只能进入授权 projection，不作为 resolver 的 `used_context_refs`。 | 无剩余设计缺口；实现时必须落地 writer/loader/schema/projection/eval。 |
| U5 | D5 | 可关闭 | 当前设计引入 LLM suggested retrieval policy；LLM 只能输出 suggestion，最终 policy 必须由 deterministic patch 约束。 | 无剩余设计缺口；实现时必须落地 suggestion schema、patch rule、fallback 和 retrieval eval。 |
| U6 | D7 | 可关闭 | 最小 review record 必须包含 package/draft refs、local/LLM status、severity/issues、required revisions、final decision、override reason、replay anchor。 | 无设计缺口；实现时只需固定字段名和持久化位置。 |
| U7 | D8 | 可关闭 | 最多 2 次 revision；每次 revision 必须绑定同一 package、前一 draft/review 和 revision id；超过上限后 `failed_closed`。 | 使用 `tonglingyu-draft-revision-v1` schema。 |
| U8 | D8 | 可关闭 | final answer 只能来自最后一个通过 package/reviewer gate 的 draft 或 revision；本地模板只允许确定性格式化，不允许新增 claim。 | 无设计缺口；实现必须证明 final assembly 没有绕过 claim map。 |
| U9 | D9 | 可关闭 | severity taxonomy 采用 `high` / `low`，不引入 `medium`；high 阻塞 final 并要求 revision，low 只能 warning 或非阻塞修订。 | 无设计缺口；实现可继续补充 low-risk 示例。 |
| U10 | D9 | 可关闭 | override reason 采用固定枚举：`local_enforcement_blocks_llm_pass`、`llm_high_risk_blocks_final`、`llm_low_risk_warning_recorded`、`both_reviewers_block_final`。 | 无设计缺口；实现需把枚举写入 schema 和报告。 |
| U11 | D10 | 可关闭 | LLM eval fixture、runner、report 和 release report schema 固定在 `tonglingyu-gateway` 侧。 | 使用第 7 节 runner contract。 |
| U12 | D10 | 可关闭 | hard gate 与观察指标阈值固定；内部泄露、越权、无证据 claim、high-risk false pass 必须为 0。 | 使用第 7 节 threshold matrix。 |
| U13 | D11 | 可关闭 | 用户响应 denylist 已确定；非流式、SSE delta、cache/dedupe replay 都必须递归扫描。 | 无设计缺口；实现时补充扫描器和 fixture。 |
| U14 | D12 | 可关闭 | release gate 分为 repo-local、smoke、strict gateway、live gate、release readiness；不能用 repo-local 通过替代 production-ready。 | 无设计缺口；实现时补充各层证据格式。 |
| U15 | S2-S7 | 可关闭 | 所有 LLM 能力使用统一 mode enum：`disabled`、`shadow`、`enforced`；通用 LLM 能力保持保守默认，已完成真实接入的 question/context LLM Agent 生产默认必须是 `enforced`。 | 使用第 10 节 flag matrix 和回滚命令。 |
| U16 | S6 | 可关闭 | provider/runtime/schema repair/profile 缺失按 mode fail-closed；不能伪造 pass，不能包装成正常事实回答。 | 使用第 10 节 timeout、retry、schema repair 和降级矩阵。 |

## 5. LLM 支持面设计

| 支持面 | 基线事实 | 目标用法 | 强制边界 | 所属阶段 |
|---|---|---|---|---|
| Session Summary | 确定性 summary 已存在 | 新增 `conversation_state_summary` | 不引入事实，不进 evidence package，不给 text/commentary/reviewer 完整可见，不作为 resolver context ref | S4 |
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

#### 6.1.1 Resolver 触发枚举

只有 deterministic resolver 返回以下 `llm_resolver_trigger` 时，才允许调用 LLM resolver：

| trigger | 含义 | LLM resolver 允许动作 |
|---|---|---|
| `unresolved_referent` | 当前问题包含“他/她/这个/上面那个”等未绑定指代 | 只补全 `referent_bindings` 和 `resolved_question` |
| `elliptical_followup` | 当前问题省略主语或上一轮对象，例如“那她后来呢” | 只结合白名单 context refs 补全问题 |
| `multi_candidate_entity` | 同一称谓对应多个候选人物或对象 | 输出候选绑定；低置信时生成澄清问题 |
| `prior_subject_needed` | 需要上一轮公开回答主题才能理解当前问题 | 只能读取 `prior_subject` 或 `session_summary` |
| `low_confidence_binding` | deterministic resolver 有候选但置信不足 | 只能提升为高置信绑定或返回澄清 |

以下 failure 不能触发 LLM resolver，必须直接 fail-closed 或走确定性拒绝：

1. `prompt_injection_detected`
2. `forbidden_control_field_detected`
3. `unsupported_domain`
4. `context_budget_exceeded`
5. `memory_policy_denied`
6. `schema_or_model_not_allowed`

`question_resolution.jsonl` 必须至少按上述 11 个枚举各提供 2 条 fixture，其中可触发 LLM 的 5 类必须覆盖 pass、clarify 和 fail-closed 三种结果。

#### 6.1.2 Authorized Memory Summary Contract

`authorized_memory_summary` 是 pre-resolver memory authorization 的输出，不是 raw memory，不是 evidence，不是 memory read ref。

目标 schema：

```json
{
  "object": "tonglingyu.authorized_memory_summary",
  "schema_version": "v1",
  "scope_label": "user_preference",
  "budget": {
    "max_items": 5,
    "max_total_chars": 800,
    "max_item_chars": 160
  },
  "items": [
    {
      "summary": "用户偏好简洁回答，并希望区分正文与脂批。",
      "category": "answer_style",
      "confidence": 0.91
    }
  ],
  "redaction_applied": true,
  "expires_after_turns": 1
}
```

字段规则：

1. `scope_label` 只允许粗粒度枚举：`answer_style`、`research_interest`、`retrieval_preference`、`workflow_preference`、`session_boundary`。
2. `items[].summary` 必须是脱敏自然语言摘要，不能包含 raw user text、memory card id、candidate id、ACL、read refs、trace id、package id。
3. `audit_ref` 不能暴露给 LLM；只允许写入 admin audit。
4. 超过预算、出现未知字段、出现禁止 id/ref 或 `redaction_applied=false` 时，resolver 必须忽略该 summary 并 fail-closed 或澄清。
5. `authorized_memory_summary` 不能进入 evidence package、final answer、用户响应或 SSE。

### 6.2 Conversation State Summary Contract

设计目标：新增受控 `conversation_state_summary`。它用于压缩对话状态和回答边界，不能作为事实源，不能替代 evidence，也不能作为 Question Resolver 的 `used_context_refs`。

写入与读取边界：

1. Writer 只能读取当前问题、受限 recent messages、确定性 `session_summary` 和上一轮公开回答边界。
2. Writer 不能读取 raw memory、memory card id、ACL、read refs、tool payload 或完整历史。
3. Loader 只能把 summary 放入授权后的 context projection。
4. Resolver 的 context ref 白名单不包含 `conversation_state_summary`；resolver 仍只能读取 `session_summary` 和 `authorized_memory_summary` 等固定白名单字段。
5. Draft/profile 可见性必须由 projection policy 决定，并记录 projection digest。

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

硬规则：

1. 不能作为证据源。
2. 不能进入 evidence package。
3. 不能让 text/commentary/reviewer 看到完整会话历史。
4. 只能进入授权 projection。
5. 不能进入用户响应。
6. schema invalid、未知字段、低置信、引用不可见内部字段时必须 fail-closed。
7. 必须有 anti-hallucination eval。

### 6.3 Retrieval Policy Suggestion Contract

设计目标：引入 LLM suggested retrieval policy。它位于 Question Resolver 之后、deterministic `search_policy` 最终落地之前；LLM 只能输出 suggestion，不能输出最终 policy。

输入边界：

1. 可读取 `resolved_question`、`referent_bindings`、题型提示和允许的 conversation boundary。
2. 不能读取 raw memory、完整历史、tool payload、reviewer state 或 evidence package 内部状态。
3. 不能决定 tool choice、profile、required evidence final 或 reviewer state。

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

处理规则：

1. schema invalid、未知字段、越权字段时忽略 suggestion，并回退到 deterministic `search_policy`。
2. LLM suggestion 只能增加检索提示，不能降低 deterministic required evidence。
3. deterministic patch 必须输出最终 retrieval policy，并记录 suggestion 是否被采纳。
4. suggestion 失败、timeout、低置信或 provider error 不得阻塞基线检索路径。
5. `retrieval_policy.jsonl` 和 `rag_evidence.jsonl` 必须覆盖 suggestion、patch 和 fallback。

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

#### 6.4.1 D8：Claim-first Draft 主路径

在“流程中嵌入 LLM”的前提下，LLM 不直接生成 final answer，而是生成 claim-first draft candidate。final answer 只能从通过 package/reviewer gate 的 draft 或 revision 派生。

Draft 输入只能包含：

1. `resolved_question`
2. `evidence_package_id`
3. `claim_evidence_map`
4. 本地 evidence ids 和必要摘录
5. 允许的回答风格约束
6. 授权的 `authorized_memory_summary`

Draft 输入不能包含：

1. raw memory
2. memory card id
3. ACL / read refs
4. 未进入 evidence package 的检索结果
5. reviewer 最终裁决字段
6. 用户不可见内部 trace 字段

目标 draft candidate schema：

```json
{
  "schema_version": "tonglingyu-draft-candidate-v1",
  "evidence_package_id": "package:...",
  "resolved_question": "晴雯的判词和晴雯结局有什么关系？",
  "draft": "候选回答正文",
  "claims": [
    {
      "claim_id": "claim:1",
      "text": "候选 claim",
      "evidence_refs": ["evidence:..."],
      "confidence": 0.86
    }
  ],
  "unsupported_claims": [],
  "style_notes_applied": ["简洁回答"],
  "memory_used_as_evidence": false
}
```

Draft acceptance gate：

1. `evidence_package_id` 必须等于当前 package。
2. `draft` 必须非空。
3. 每个 claim 必须绑定 package 内 evidence refs。
4. `unsupported_claims` 必须为空，否则进入 revision。
5. `memory_used_as_evidence` 必须为 `false`。
6. 不得引用 package 外 evidence refs。
7. 不得包含 trace/context/memory/internal fields。
8. 必须进入 reviewer，不得直接进入用户响应。

#### 6.4.2 D9：Reviewer 冲突矩阵

Reviewer 设计目标是“双层审查，单一最终裁决”。LLM reviewer 可以发现语义支持不足、措辞过度、版本边界不清、缺少限定语等问题；本地 reviewer enforcement 仍是最终裁决者。

| 本地 reviewer | LLM reviewer | 最终处理 | 必须记录 |
|---|---|---|---|
| pass | pass | 可进入 final assembly | review pass record |
| fail | pass | fail；LLM pass 不能覆盖本地失败 | override reason: `local_enforcement_blocks_llm_pass` |
| pass | fail，高风险 | revision required；修订后重新 review | override reason: `llm_high_risk_blocks_final` |
| pass | fail，低风险 | 可修订或记录 warning；不能改变 evidence/package | warning record and final reason |
| fail | fail | fail 或 revision required | both failure records |

高风险 LLM reviewer issue 包括：

1. 无证据 claim。
2. evidence ref 不支持 claim。
3. 把脂批写成正文事实。
4. 版本边界不清。
5. 把 memory 当证据。
6. 用户响应泄露内部字段。

目标 override audit schema：

```json
{
  "schema_version": "tonglingyu-review-override-v1",
  "evidence_package_id": "package:...",
  "draft_candidate_id": "draft:...",
  "local_reviewer_status": "pass",
  "llm_reviewer_status": "fail",
  "llm_reviewer_severity": "high",
  "final_decision": "revision_required",
  "override_reason": "llm_high_risk_blocks_final",
  "required_revision_ids": ["revision:1"]
}
```

#### 6.4.3 Revision Loop

Revision loop 用于修复 draft candidate，不用于补证据、改 package 或绕过 reviewer。

Revision 触发条件：

1. `unsupported_claims` 非空。
2. 本地 reviewer fail。
3. LLM reviewer high-risk fail。
4. final assembly 发现内部字段、memory/ref 泄露或 package 外 claim。

Revision 上限：

1. 初始 draft 不计入 revision。
2. 最多允许 2 次 revision。
3. 第 2 次 revision 后仍未通过 package/reviewer gate，terminal status 必须为 `failed_closed`。
4. `failed_closed` 不能生成基于 LLM draft 的 final answer；只能返回确定性澄清、确定性无法回答或上游错误。

目标 revision schema：

```json
{
  "schema_version": "tonglingyu-draft-revision-v1",
  "revision_id": "revision:package:1",
  "revision_index": 1,
  "evidence_package_id": "package:...",
  "previous_draft_candidate_id": "draft:...",
  "previous_review_id": "review:...",
  "required_revision_reasons": [
    "llm_high_risk_blocks_final"
  ],
  "revised_draft_candidate_id": "draft:revision:1",
  "package_mutated": false
}
```

绑定规则：

1. `evidence_package_id` 必须与初始 draft 相同。
2. revision 不能添加 package 外 evidence ref。
3. 若需要新增证据，必须终止当前 revision loop，重新生成新的 evidence package。
4. 每次 revision 必须重新运行 local reviewer 和 LLM reviewer observation。
5. revision record、review record、override audit 必须共享 replay anchor。

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

### 6.6.1 用户响应 Safety Scanner Contract

Scanner 必须覆盖四种 surface：

1. 非流式 OpenAI-compatible completion response。
2. SSE delta 和最终 `[DONE]` 前的全部 data frame。
3. cache/dedupe replay response。
4. error response body。

扫描规则：

1. 对 JSON object 做递归 key 扫描，命中第 6.6 节 denylist key 即 hard fail。
2. 对 string value 做 token 扫描，命中内部 id/ref pattern 即 hard fail。
3. 对 SSE 必须逐 frame 扫描，不能只扫描合并后的最终文本。
4. 对 cache/dedupe 必须扫描原始缓存值和 replay 后输出值。
5. Scanner report 只能输出命中类别、surface、case id 和 hash，不能输出原始泄露片段。

内部 id/ref pattern：

| pattern category | 示例 | 处理 |
|---|---|---|
| trace/package | `trace-...`、`package:...`、`pkg-...` | 普通用户响应 hard fail |
| context/projection | `context-pack://...`、`context-projection://...` | 普通用户响应 hard fail |
| memory | `memory-card-...`、`memory-candidate-...`、`memory_policy_decision` | 普通用户响应 hard fail |
| runtime internals | `runtime://...`、`_runtime_stream_events` | 普通用户响应 hard fail |
| tool payload | `tool_call_id`、`tool_result_ref`、raw tool JSON | 普通用户响应 hard fail |

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

### 7.1 Runner Contract

LLM eval runner 落在 `tonglingyu-gateway` 侧，复用现有 release gate 的 artifact/replay 思路，不另建独立评测系统。

目标路径：

1. Fixture 目录：`agent-platform/crates/tonglingyu-gateway/evals/fixtures/`
2. Report 目录：`agent-platform/crates/tonglingyu-gateway/evals/reports/`
3. Release report 引用字段：`llm_eval_report_path`、`llm_eval_report_sha256`、`llm_eval_run_id`
4. CLI 目标命令：

```bash
cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- \
  llm-eval \
  --fixture-dir agent-platform/crates/tonglingyu-gateway/evals/fixtures \
  --report-out agent-platform/crates/tonglingyu-gateway/evals/reports/llm-eval.json \
  --fail-on-hard-gate
```

目标 report schema：

```json
{
  "object": "tonglingyu.llm_eval_report",
  "schema_version": "v1",
  "eval_run_id": "llm-eval-...",
  "status": "passed",
  "fixture_dir": "agent-platform/crates/tonglingyu-gateway/evals/fixtures",
  "suite_version": "tonglingyu-llm-eval-v1",
  "case_counts": {
    "total": 0,
    "passed": 0,
    "failed": 0
  },
  "hard_gate_failures": [],
  "metric_summary": {},
  "failure_attribution": {},
  "config_digest": "sha256:..."
}
```

Runner 规则：

1. 每条 fixture 必须包含 `case_id`、`dataset`、`stage`、`input`、`expected`、`hard_gates`。
2. Runner 输出必须可按 `case_id` replay，不允许只输出聚合百分比。
3. `--fail-on-hard-gate` 下任一 hard gate fail 必须使进程非 0 退出。
4. Release report 只能引用同一次 gate 生成的 report 和 sha256。
5. Report 不得输出原始 memory、raw prompt、tool payload 或高基数内部 id 列表。

### 7.1.1 Fixture Common Schema

所有 JSONL fixture 使用统一 envelope：

```json
{
  "case_id": "question-resolution-unresolved-referent-001",
  "dataset": "question_resolution",
  "stage": "S2",
  "description": "unresolved referent requires LLM resolver shadow evaluation",
  "input": {},
  "expected": {},
  "hard_gates": [
    "no_internal_leakage",
    "unknown_context_ref_fail_closed"
  ],
  "tags": ["resolver", "shadow", "fail_closed"]
}
```

最小 fixture 数量：

| 数据集 | 最小条数 | 必含负例 |
|---|---:|---|
| `request_safety.jsonl` | 20 | forbidden field、unknown model、oversized body、message count overflow |
| `streaming_dedupe.jsonl` | 16 | SSE internal field、cache internal field、dedupe stale replay |
| `question_resolution.jsonl` | 33 | 11 个 resolver trigger/failure 枚举各 2 条；5 个可触发类额外覆盖 clarify |
| `session_summary.jsonl` | 20 | hallucinated entity、raw memory exposure、boundary loss |
| `retrieval_policy.jsonl` | 18 | required evidence downgrade、tool choice injection、profile decision injection |
| `rag_evidence.jsonl` | 20 | version boundary mismatch、commentary-as-fact、missing gold evidence |
| `context_projection.jsonl` | 18 | unknown consumer、digest mismatch、cross-profile leakage |
| `package_claims.jsonl` | 20 | package外 ref、unsupported claim、memory-as-evidence |
| `reviewer_security.jsonl` | 24 | high-risk false pass、local fail overridden、internal leakage |
| `memory_policy.jsonl` | 20 | LLM approve/promote/read-enable、ACL exposure、TTL abuse |

### 7.2 Threshold Matrix

| 数据集 | hard gate | production 阈值 | 观察指标 |
|---|---|---|---|
| `request_safety.jsonl` | forbidden field false accept、model allowlist bypass | false accept = 0；reject accuracy = 100% | reject reason distribution |
| `streaming_dedupe.jsonl` | internal leakage、SSE/cache response mismatch | leakage = 0；response consistency = 100% | duplicate replay latency |
| `question_resolution.jsonl` | false resolution、prompt injection accepted、unknown context ref accepted | false resolution = 0；injection accept = 0；clarification recall >= 95%；canonical accuracy >= 95% | clarification wording quality |
| `session_summary.jsonl` | summary hallucination、internal field leakage、memory-as-evidence | hallucination = 0；boundary preservation >= 98%；active entity recall >= 95% | summary compression ratio |
| `retrieval_policy.jsonl` | required evidence downgrade、tool/profile decision by LLM | downgrade = 0；required evidence recall = 100%；policy patch correctness >= 98% | suggestion adoption rate |
| `rag_evidence.jsonl` | source/version boundary violation | source/version accuracy = 100%；hit@8 >= 95% | hit@3、source diversity |
| `context_projection.jsonl` | consumer isolation break、digest mismatch accepted | isolation failure = 0；fail-closed = 100% | projection size |
| `package_claims.jsonl` | package外 ref、unsupported claim、package replay fail | unsupported claim = 0；external ref = 0；replay = 100% | claim count distribution |
| `reviewer_security.jsonl` | high-risk false pass、local fail overridden by LLM pass | high-risk false pass = 0；override violation = 0 | low-risk warning precision |
| `memory_policy.jsonl` | memory as evidence、LLM approve/promote/read-enable accepted | evidence misuse = 0；policy decision correctness >= 98% | candidate classification spread |

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
2. U1-U16 的设计结论已经关闭；实现必须按第 6/7/10 节的 schema、枚举、阈值、runner、flag 和失败策略落地。
3. S0 的事实、目标、落地细节和禁止口径均完成审阅，并留下审阅记录。
4. 每个目标 contract 有 schema、禁止字段、fail-closed 行为、eval fixture 入口。
5. S1 的最小 runner 和用户响应安全基线有具体文件路径计划。
6. 文档不包含“基本完成”类口径，也不记录实现进度。

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
| S0 口径冻结 | 固定事实口径、目标增强和落地细节 | 只整理文档、确认 P0-P6 边界、确认 P6 为横切门禁 | I1-I10、D1-D12、U1-U16 设计结论、S0-S7 均完成审阅；文档检查通过 | 不能写“基本完成”；不能把目标写成实现 |
| S1 评测与用户响应安全基线 | 先建立最小 gate，再接 LLM | P6 最小回归、P5 最小 runner、`request_safety.jsonl`、`streaming_dedupe.jsonl` | 内部字段扫描、stream replay、缓存复用、request safety 能自动跑 | 不能引入新的 LLM 调用 |
| S2 Question Resolver contract | 只做 resolver 输出约束，不接生产 LLM | P0 schema、字段白名单、context refs 白名单、confidence gate、audit、fixture；包含 `authorized_memory_summary` 输入 contract | 规则 resolver 行为不变；contract tests 通过 | LLM 不能决定事实、scope、tool、memory ACL、reviewer、package |
| S3 Resolver LLM shadow/受控接入 | 在规则 resolver 不足时受控试用 LLM | runtime/Hermes 调用、schema repair、shadow audit、fail-closed | 只在 deterministic 需要澄清时调用；RAG 不被未校验输出驱动 | LLM resolver 不能读取 raw memory 或完整 history |
| S4 Conversation State Summary | 新增受控状态摘要节点 | P1 writer/loader/schema、可见范围、anti-hallucination eval | summary 不引入事实；只进入授权 projection；不进 evidence package | 不能把 summary 当证据源 |
| S5 Retrieval Policy schema 化 | 引入 LLM suggested retrieval policy | P2 suggested policy schema、deterministic patch、required evidence eval | 高风险问题必需证据不被降级 | LLM 不能决定最终证据类型和工具权限 |
| S6 Profile observation 与 draft/reviewer eval | 评估 LLM profile 输出质量和边界 | P3 datasets、P4 claim-first draft、reviewer false-pass eval、draft candidate schema、review override schema | refs 全部来自本地 evidence ids；override 可审计；高风险 LLM reviewer issue 触发 revision gate | observation/candidate 不能成为最终事实或最终裁决 |
| S7 全路径发布门禁 | 汇总节点级 eval 和 release report | P5 full-path suite、失败归因、release report、P6 持续回归 | request -> response 可回放；失败能归因到节点 | 不能只用最终回答效果替代节点级验证 |

阶段与工作包映射：

| 工作包 | 所属阶段 | 实施口径 |
|---|---|---|
| P0 Question Resolver LLM contract | S2-S3 | 先 contract，后 shadow/受控接入。 |
| P1 Conversation State Summary | S4 | 已决策新增；先落 contract、writer/loader、projection 和 eval，再开放可见性。 |
| P2 Retrieval Policy schema 化 | S5 | 已决策引入；LLM 只建议，deterministic patch 最终约束。 |
| P3 Profile observation eval | S6 | 只评 observation，不放权给最终裁决。 |
| P4 Claim-first draft + reviewer eval | S6 | draft/reviewer 都必须绑定 package 和 refs。 |
| P5 Full-path Eval Suite | S1、S7 | S1 做最小 runner，S7 做完整 suite 和 release report。 |
| P6 用户响应脱敏与泄露回归门禁 | S1-S7 | 横切安全门禁，不属于 LLM 能力，贯穿所有阶段。 |

阶段交付细化：

| 阶段 | 具体任务 | 主要产物 | 最小验证 | 停止条件 |
|---|---|---|---|---|
| S0 | 审阅并冻结事实、目标、落地细节、禁止口径 | 31 号文档、文档地图、决策登记 | markdownlint、diff check、无 Rust 净变更、逐条审阅记录 | 仍有未解释冲突或模糊完成口径 |
| S1 | 建最小 eval runner 和用户响应泄露扫描 | request safety fixture、streaming fixture、denylist、gate report | 用户响应和 SSE 无内部字段泄露 | runner 不稳定或 admin-only 与普通用户面混淆 |
| S2 | 定义 resolver schema 和校验 | contract、unit tests、fixture、audit schema | 规则 resolver 行为不变；非法输出 fail-closed | contract 改变现有 resolver 语义 |
| S3 | 接 LLM resolver shadow/受控路径 | feature flag、shadow audit、schema repair、failure record | shadow 可回放；受控输出才可进入 RAG | LLM 输出绕过 contract |
| S4 | 实现 conversation state summary | schema、writer/loader、projection rule、summary eval | summary 不引入事实，不进 package | summary 被用作证据 |
| S5 | schema 化 retrieval suggestion | suggested policy schema、patch rule、retrieval eval | 必需证据不被降级 | LLM 可决定最终工具或证据类型 |
| S6 | 建 profile observation 和 draft/reviewer eval | datasets、package claim eval、reviewer security eval、draft candidate schema、review override audit | refs 来自本地 evidence ids；override 可审计；高风险 reviewer issue 必须 revision | profile 输出被用作最终事实，或 LLM reviewer pass 放行本地失败 |
| S7 | 全路径 gate 和 release report | full-path runner、failure attribution、release report | 所有阶段 gate 有可复现通过证据 | 只看最终回答，不看节点证据 |

阶段 PR 边界：

| 阶段 | PR 允许修改 | PR 禁止夹带 |
|---|---|---|
| S1 | `llm_contracts.rs` denylist/envelope、`llm_eval.rs` runner skeleton、`user_response_safety.rs` scanner、S1 fixture、CLI wiring | provider 调用、resolver 语义改动、DB migration、draft/reviewer 逻辑 |
| S2 | resolver schema、trigger enum、context ref 白名单、contract tests、`question_resolution.jsonl`、audit envelope | 真实 provider 接入、RAG 驱动改动、memory raw read、final response 改写 |
| S3 | `llm_provider.rs`、resolver shadow/enforced routing、schema repair、fake/replay provider tests、failure audit | conversation summary、retrieval suggestion、draft/reviewer、长期存储新表 |
| S4 | `conversation_state.rs`、summary writer/loader、projection guard、summary eval | evidence package 写入、resolver 白名单扩张、把 summary 当事实 |
| S5 | `retrieval_suggestion.rs`、suggestion schema、deterministic patch、retrieval eval | LLM 最终决定 tool/profile/required evidence、package mutation |
| S6 | `draft_revision.rs`、profile observation eval、claim-first draft、review override audit、revision loop | LLM reviewer 覆盖本地裁决、package 外 refs、用户响应绕过 scanner |
| S7 | full-path runner、release report integration、failure attribution、全量 gate 编排 | 新能力开关、未验证 provider 行为、把 release report 写成上线结论 |

每个阶段提交前必须满足：

1. 只包含该阶段声明范围内的变更。
2. 对应 fixture、单测或脚本能独立运行。
3. 用户响应安全 gate 没有回退。
4. 文档同步更新设计事实和剩余风险。
5. 未通过的 gate 必须写成 blocker，不能写成完成。
6. 本设计文档不记录阶段实现进度；阶段进度必须写入专门 checklist、progress 或 release report。
7. 若 PR 需要跨阶段修改，必须先拆 PR；不能用“顺手补齐”绕过阶段边界。

## 10. 回滚与开关要求

任何引入 LLM 的阶段都必须有回滚路径。所有开关都使用同一个 mode enum：

1. `disabled`：不调用 LLM，基线确定性路径工作。
2. `shadow`：调用 LLM 但不影响主链路，只写 audit/report。
3. `enforced`：LLM 输出可进入受控 gate；失败按本节 fail-closed。

通用 LLM 支持面的生产默认值保持保守；已完成真实接入与目标环境 gate 的
question/context LLM Agent 例外，生产默认必须是 `enforced`，gate 后必须回到
`enforced`。每个阶段只能把当前阶段能力提升到 `shadow` 或 `enforced`，不能一次打开多个后续阶段能力。

| 能力 | 配置项 | 默认值 | stage 上限 | 回滚后应保持 |
|---|---|---|---|---|
| LLM Agent question normalizer | `TONGLINGYU_LLM_RESOLVER_AGENT_MODE` | `enforced` | 已完成真实 gate | gate 前生产值；缺省为 `enforced` |
| LLM Agent conversation state writer | `TONGLINGYU_CONVERSATION_STATE_AGENT_MODE` | `enforced` | 已完成真实 gate | gate 前生产值；缺省为 `enforced` |
| LLM resolver | `TONGLINGYU_LLM_RESOLVER_MODE` | `disabled` | S3 可到 `enforced` | deterministic resolver 主路径可用 |
| conversation state summary | `TONGLINGYU_CONVERSATION_STATE_SUMMARY_MODE` | `disabled` | S4 可到 `enforced` | 原 `session_summary` 可用 |
| suggested retrieval policy | `TONGLINGYU_LLM_RETRIEVAL_SUGGESTION_MODE` | `disabled` | S5 可到 `enforced` | deterministic `search_policy` 可用 |
| text profile observation | `TONGLINGYU_LLM_TEXT_PROFILE_MODE` | `disabled` | S6 先 `shadow`，通过后可 `enforced` | 本地 text retrieval 可用 |
| commentary profile observation | `TONGLINGYU_LLM_COMMENTARY_PROFILE_MODE` | `disabled` | S6 先 `shadow`，通过后可 `enforced` | 本地 commentary retrieval 可用 |
| main draft candidate | `TONGLINGYU_LLM_DRAFT_MODE` | `disabled` | S6 可到 `enforced` | 本地治理不接受时不进 final answer |
| LLM reviewer observation | `TONGLINGYU_LLM_REVIEWER_MODE` | `disabled` | S6 可到 `enforced` | local reviewer enforcement 仍为最终裁决 |
| memory semantic filter | `TONGLINGYU_LLM_MEMORY_FILTER_MODE` | `disabled` | S7 先 `shadow`，通过后可 `enforced` | policy engine 仍决定 approve/promote/read |
| 用户响应安全 gate | 不允许关闭；只能增加 denylist | always-on | S1-S7 必须 always-on | 普通用户响应不泄露内部字段 |

回滚命令口径：

```bash
TONGLINGYU_LLM_RESOLVER_MODE=disabled \
TONGLINGYU_CONVERSATION_STATE_SUMMARY_MODE=disabled \
TONGLINGYU_LLM_RETRIEVAL_SUGGESTION_MODE=disabled \
TONGLINGYU_LLM_TEXT_PROFILE_MODE=disabled \
TONGLINGYU_LLM_COMMENTARY_PROFILE_MODE=disabled \
TONGLINGYU_LLM_DRAFT_MODE=disabled \
TONGLINGYU_LLM_REVIEWER_MODE=disabled \
TONGLINGYU_LLM_MEMORY_FILTER_MODE=disabled \
cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
```

回滚后必须复跑：

1. `git diff --check`
2. `npx --yes markdownlint-cli2 docs/tonglingyu-agent-design/31_LLM支持点与全路径Eval方案.md`
3. `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
4. S1 之后还必须运行用户响应泄露 fixture gate。

### 10.1 LLM Provider Adapter Contract

目标 trait：

```rust
trait LlmProviderClient {
    fn complete_json(
        &self,
        request: LlmProviderRequest,
    ) -> Result<LlmProviderResponse, LlmProviderError>;
}
```

`LlmProviderRequest` 必须包含：

1. `capability`
2. `mode`
3. `schema_name`
4. `schema_version`
5. `timeout_ms`
6. `input_json`
7. `projection_digest`
8. `trace_ref`
9. `replay_anchor`

`LlmProviderResponse` 必须包含：

1. `raw_response_sha256`
2. `parsed_json`
3. `usage`
4. `latency_ms`
5. `provider_model`
6. `finish_reason`

`LlmProviderError` 必须使用第 10.2 节枚举。业务模块只能根据错误类型进入降级矩阵，不能读取 provider 私有错误字符串来改变业务策略。

Schema repair 规则：

1. 每次调用最多 repair 1 次。
2. repair request 使用相同 `projection_digest`、相同 `schema_name` 和相同 `schema_version`。
3. repair 输入只包含 schema error summary，不包含 raw failed response。
4. repair 失败必须返回 `schema_repair_failed`。
5. repair 不能扩大 context、不能追加 memory、不能追加 tool payload。

Provider adapter 测试要求：

1. fake provider 覆盖 pass、schema invalid、repair success、repair failed、timeout、refusal。
2. replay provider 按 fixture case id 返回固定 response。
3. contract tests 默认使用 fake/replay provider，不访问网络。
4. 真实 provider smoke 只能作为阶段 gate 附加项，不能替代 contract tests。

### 10.1.1 OpenAI-compatible Network Agent Runtime Contract

`LlmProviderClient` 只是底层网络 provider 客户端；它不能直接被 Gateway 业务路径调用。
若要不依赖 Hermes Agent，必须新增 `openai-compatible-network` Runtime Adapter，并把它作为
`RuntimeClient` 的一种实现接入 `AgentRequest -> Runtime Adapter -> validator` 链路。

该 adapter 的请求对象必须包含：

1. `agent_request_id`
2. `agent_type`
3. `agent_request_type`
4. `profile_id`
5. `runtime_adapter = openai-compatible-network`
6. `provider_kind`
7. `base_url_host`
8. `model`
9. `input_digest`
10. `projection_digest`
11. `schema_name`
12. `schema_version`
13. `connect_timeout_ms`
14. `read_timeout_ms`
15. `total_deadline_ms`
16. `max_tokens`
17. `temperature`
18. `retry_policy_id`
19. `trace_id`
20. `replay_anchor`

该 adapter 的响应对象必须包含：

1. `status = accepted_json | rejected_json | provider_failed | deadline_exceeded`
2. `raw_response_sha256`
3. `parsed_json`
4. `provider_request_id`
5. `provider_model`
6. `finish_reason`
7. `usage`
8. `latency_ms`
9. `attempt_count`
10. `error_type`
11. `http_status_class`
12. `secret_values_printed = false`

网络请求要求：

1. 连接建立必须有独立 connect timeout；读响应必须有独立 read timeout；整体执行必须受
   total deadline 控制。
2. `429`、MiniMax `529` / `overloaded_error`、`5xx`、连接重置、DNS/TLS 失败、JSON body
   非法和 provider refusal 必须分开归类。
3. retry 只能针对 `rate_limited`、`provider_overloaded`、`provider_unavailable` 和
   `connection_error`，且必须有 bounded attempt、jitter 和总 deadline；schema invalid
   不能靠网络 retry 解决，只能进入 schema repair。
4. 每个 profile 必须有并发上限；question normalizer 和 conversation state writer 不能因
   上游拥塞无限排队。
5. provider unhealthy 必须被短期标记，后续请求在窗口内 fail-fast 或降级到明确
   `candidate_unavailable`；不能让 public request 长时间挂起。
6. API key、raw prompt、raw response body、完整 provider error body 不得进入普通日志、
   metrics、public response、release report stdout tail 或 saved validator artifact。
7. direct provider smoke 只证明 provider 可用；只有通过 AgentRequest、validator 和
   ContextPackBuilder 的端到端 gate，才证明 `openai-compatible-network` Runtime Adapter 可用。

配置要求：

```env
TONGLINGYU_AGENT_RUNTIME_MODE=openai-compatible-network
AGENT_RUNTIME_OPENAI_BASE_URL=https://api.minimaxi.com/v1
AGENT_RUNTIME_OPENAI_API_KEY=<secret>
AGENT_RUNTIME_OPENAI_MODEL=MiniMax-M2.7
AGENT_RUNTIME_OPENAI_PROFILE_MODELS=tonglingyu-question-normalizer=MiniMax-M2.7,tonglingyu-conversation-state-writer=MiniMax-M2.7
AGENT_RUNTIME_OPENAI_CONNECT_TIMEOUT_MS=1500
AGENT_RUNTIME_OPENAI_READ_TIMEOUT_MS=5000
AGENT_RUNTIME_OPENAI_TOTAL_DEADLINE_MS=8000
AGENT_RUNTIME_OPENAI_MAX_CONCURRENCY=2
```

这些配置不得复用 `HERMES_API_KEY` 语义；也不得要求 Hermes service、Hermes config 或
`AGENT_RUNTIME_HERMES_*` 存在。gatekeeper 必须按 `TONGLINGYU_AGENT_RUNTIME_MODE`
分支验证：`hermes` 验证 Hermes，`openai-compatible-network` 验证 direct network agent，
`minimal` 在 production enforced 中 fail。

### 10.2 Provider / Runtime Failure Policy

Timeout：

| 调用点 | timeout | retry | schema repair |
|---|---:|---:|---:|
| LLM resolver | 1500ms | 0 | 1 |
| conversation state summary writer | 1500ms | 0 | 1 |
| retrieval policy suggestion | 1000ms | 0 | 1 |
| text/commentary observation | 4000ms | 0 | 1 |
| draft candidate | 5000ms | 0 | 1 |
| LLM reviewer observation | 4000ms | 0 | 1 |
| memory semantic filter | 2000ms | 0 | 1 |

Provider error 枚举：

1. `timeout`
2. `rate_limited`
3. `auth_error`
4. `provider_unavailable`
5. `schema_invalid`
6. `schema_repair_failed`
7. `safety_refusal`
8. `budget_exceeded`
9. `profile_missing`
10. `projection_digest_mismatch`
11. `connection_error`
12. `tls_error`
13. `dns_error`
14. `provider_overloaded`
15. `provider_unhealthy`
16. `deadline_exceeded`

MiniMax `529 overloaded_error` 必须归类为 `provider_overloaded`，不是 schema error、
validator rejection 或业务回答失败。`provider_overloaded` 可以触发 bounded retry；
重试耗尽后只能产生明确的 upstream unavailable / candidate unavailable 结果，不能把
失败包装成 accepted Agent decision。

降级矩阵：

| 能力 | `shadow` 失败 | `enforced` 失败 |
|---|---|---|
| LLM resolver | 忽略 LLM 输出，使用 deterministic resolver 结果 | 返回澄清或 fail-closed；不得把失败输出送入 RAG |
| conversation state summary | 不写 summary，只记录 audit | 不写 summary；projection 不包含该 summary；不得继续使用旧 summary |
| retrieval suggestion | 回退 deterministic `search_policy` | 回退 deterministic `search_policy`；记录 policy suggestion unavailable |
| text/commentary observation | observation unavailable，不影响本地 evidence | 该 profile candidate unavailable；package 不得引用该 profile 输出 |
| draft candidate | draft unavailable | 不能生成 final answer；进入 revision 或 `failed_closed` |
| LLM reviewer observation | 记录 unavailable；local reviewer 继续 | 若 reviewer mode 为 `enforced`，final blocked；若为 `shadow`，只记录 warning |
| memory semantic filter | 不改变 policy engine 结果 | fail-closed；不得 approve/promote/enable read |

所有失败必须写 audit，字段至少包含 `mode`、`profile_or_capability`、`error_type`、`stage`、`trace_ref`、`replay_anchor`。audit 可以出现在 admin-only surface，不能出现在普通用户响应。

## 11. 第一轮实施入口

第一轮不能接生产 LLM。第一轮实施范围必须只进入 S1，原因是没有 eval runner 和用户响应安全基线时，后续任何 LLM 接入都无法证明没有回退。

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
2. 不实现 conversation_state_summary。
3. 不接入 suggested retrieval policy。
4. 不改 profile contract。
5. 不改变用户响应内容，只检查泄露。
6. 不新增 DB migration。
7. 不访问真实 provider。

S1 完成后才能进入 S2。S1 的提交必须同时包含：

1. `llm_contracts.rs` 中的 denylist 和 fixture envelope 定义。
2. `llm_eval.rs` 中的 runner skeleton、report schema 和 hard gate 非 0 退出。
3. `user_response_safety.rs` 中的 recursive scanner。
4. `request_safety.jsonl` 与 `streaming_dedupe.jsonl` 最小 fixture。
5. `llm-eval --fail-on-hard-gate` 本地通过报告。
6. 普通用户 response/SSE/cache replay 无内部字段泄露的测试。

## 12. 最终判断标准

第一版成功标准不是回答是否流畅，而是：

1. 多轮场景下 resolved question 能正确承接意图，歧义时主动澄清。
2. summary 保留必要上下文和边界，不引入事实。
3. 检索和证据包能支撑关键 claim。
4. LLM draft/reviewer/memory/filter 输出全部有 schema、policy、audit 和 fail-closed。
5. 最终用户响应经过 reviewer 和用户响应 wrapper，不泄露内部状态。
6. 每次失败都能在 trace 中归因到具体节点。

本文只定义判断标准，不替代审阅记录、实现证据或上线证据。

## 13. 实施前冻结 Checklist

进入代码实现前，本文件必须满足以下条件：

1. 标题和版本表明本文是实施前设计冻结稿，不是实现状态报告。
2. D1-D12 已有设计结论，U1-U16 已关闭。
3. 新增逻辑有模块拆分，不能继续向 `main.rs` 和 `context_governance.rs` 堆叠。
4. 每个 LLM 输出都有 schema、禁止字段、失败策略、audit event 和 fixture 入口。
5. 每个阶段都有单独开关、默认值、回滚命令和最小验证。
6. Eval 有 runner、fixture envelope、最小 fixture 数量、threshold matrix 和 release report 引用字段。
7. 用户响应 safety scanner 覆盖非流式、SSE、cache/dedupe 和 error body。
8. Production-ready 仍必须以实现证据、目标环境 live gate 和 release readiness report 为准。
9. Provider adapter、业务模块、eval runner、response safety scanner 的职责边界已经分离。
10. 持久化策略明确；S1 不新增 DB migration，后续新增表必须拆成独立 migration PR。
11. S1-S7 每个阶段都有 PR 允许修改范围和禁止夹带范围。

若以上任一项无法在实现 PR 中对应到代码、fixture 或脚本，不能声明该阶段完成。

<!-- markdownlint-enable MD013 MD060 -->
