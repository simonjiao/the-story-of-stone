# 30 Scoped Memory Production Checklist

## 状态口径

Scoped Memory Production 的目标不是最小闭环、demo、shadow smoke
或只证明 Collector 通畅。Scoped Memory Production 必须把以下链路做到生产可验证：

`session_journal -> Memory Collector -> memory_candidate -> policy decision ->
memory_card -> read enablement -> context_pack.memory_read_refs -> context_projection ->
Runtime answer`

Scoped Memory Production 不重新定义 Memory Candidate Workflow 的 candidate/card 状态机。它在 Memory Candidate Workflow 已验证的
`memory_candidate`、`memory_card` 和 transition audit 基础上，打开受 ACL、scope、
retention、revoke/expire、policy 和 lifecycle 约束的读取面。

自动策略是允许的生产主路径。人工审核流程必须保留，但不再是所有 memory 进入可用
状态的默认必经关卡。人工审核用于高风险、低置信、共享 scope、抽样复核、纠错、
用户撤销、策略回滚和运营排障。

## 已冻结决策

1. **Scoped Memory Production 保持 Scoped Memory Production 目标**：不能把目标降级为只跑通
   `user_private` 或只验证 collector/context build。
2. **自动策略是一等生产路径**：规则、LLM semantic filter 和 versioned policy engine
   可以自动把 memory 变成可用状态。
3. **人工审核保留但可被策略跳过**：manual review 不是默认 blocker，但 admin-only
   CLI/API、状态机、reason、operator identity 和 audit 必须完整保留。
4. **状态流转不可省略**：自动路径也必须显式写出 `pending -> approved`、
   `approved -> active memory_card`、`read_enabled=false -> true`。
5. **LLM 不是授权者**：LLM 只能做语义过滤、分类、TTL 建议和风险标记。最终是否
   auto approve、promote 或 enable read 只能由 policy engine 决定。
6. **Scoped Memory 不是 user_private-only**：Scoped Memory Production 必须保留 `user_private`、
   `profile_common`、`knowledge_space`、`research_topic` 和 `source_collection` 的
   ACL、读取策略、状态流转和 gate。不同 scope 的自动化门槛可以不同，但不能只有
   `user_private` 有生产路径。
7. **memory 不能成为证据**：memory 只能作为偏好、背景、工作方法或长期上下文摘要。
   它不能进入 evidence package，不能替代原文/脂批/版本证据，不能改变 reviewer 裁决。
8. **Gateway 不能变成长期 memory 系统**：Gateway 可以在当前阶段承载同进程
   Context Governance 实现，但 memory 的生产语义必须由 policy、ACL、audit 和
   lifecycle 边界表达。

## 进入条件

- [x] Scoped Context Request Path 已 production-ready。
- [x] Context Projection Runtime 已 production-ready。
- [x] Memory Candidate Workflow 已 production-ready。
- [x] Memory Candidate Workflow 已证明 collector、candidate/card 状态机、admin-only CLI/API 和 audit
      可用。
- [x] 当前生产结论明确不覆盖 active memory 读取、自动 read enablement 或 scoped
      memory production-ready。
- [x] public response、SSE 和 metrics 不暴露 context/journal/memory 内部字段。

## 非目标

以下不是 Scoped Memory Production 的实现目标：

1. 不把 memory 当作正式事实源；
2. 不让 memory 进入 evidence package；
3. 不让 memory 改变 reviewer 裁决；
4. 不让普通用户提交 context、scope、memory read/write scope、policy 或 reviewer
   控制字段；
5. 不暴露公网 memory 审核入口；
6. 不用 Open WebUI conversation、Hermes transcript 或旧 `gateway_messages` 作为
   memory 来源；
7. 不通过手工 SQL 直接跳过状态机；
8. 不把非 Hermes external agent memory 接入纳入本工作流。

## Policy Contract

Scoped Memory Production 必须新增或复用结构化 policy decision 记录。最低字段：

1. `policy_decision_id`；
2. `policy_version`；
3. `policy_mode`：`shadow_only`、`auto_policy`、`manual_required`；
4. `candidate_id`；
5. `memory_card_id`，若尚未 promote 则为空；
6. `scope_type`；
7. `scope_ref`；
8. `candidate_type`；
9. `rule_filter_json`；
10. `llm_filter_json`；
11. `confidence`；
12. `sensitivity`；
13. `risk_flags_json`；
14. `decision`：`suppress`、`pending_manual_review`、`auto_approve`、
    `auto_promote`、`enable_read`、`disable_read`；
15. `decision_reason`；
16. `ttl_policy_ref`；
17. `expires_at`；
18. `actor`，自动策略使用 `memory_policy:auto`；
19. `created_at`；
20. `audit_ref`。

Policy 配置必须版本化。阈值、TTL、scope 允许列表、risk flag 规则和 LLM schema
版本不得作为无文档硬编码存在。具体数值由 policy version 定义，production gate
验证每条自动可读 memory 都能回放到对应 policy decision。

## `scoped-memory-policy-v1`

Scoped Memory Production 默认 policy version 固定为 `scoped-memory-policy-v1`。任何阈值、TTL、scope
自动化规则或 LLM schema 的生产变更，都必须形成新的 policy version，并重新通过
hhost release gate。实现不得在代码里临时改阈值来绕过 policy contract。

默认配置：

1. `TONGLINGYU_MEMORY_POLICY_MODE=auto_policy`；
2. `shadow_only` 可用于发布前观测和策略回滚；
3. `manual_required` 可用于事故降级、租户级降级或高风险 scope；
4. policy actor 使用 `memory_policy:auto:scoped-memory-policy-v1`；
5. LLM schema version 使用 `scoped-memory-llm-filter-v1`；
6. 所有自动可读 memory 必须带 `policy_decision_id`、`policy_version`、
   `policy_mode`、`confidence`、`risk_flags`、`expires_at` 和 audit ref。

### Scope Automation Matrix

| Scope | 默认自动化 | 自动可读条件 | 默认降级 |
| --- | --- | --- | --- |
| `user_private` | `auto_enable` | 低风险偏好、背景或工作方法，`confidence >= 0.85` | `manual_required` |
| `profile_common` | `auto_enable_limited` | 明确绑定 profile 的低风险运行偏好，`confidence >= 0.92` | `manual_required` |
| `knowledge_space` | `auto_enable_limited` | 非事实类检索偏好或工作方法，`confidence >= 0.94` | `manual_required` |
| `research_topic` | `auto_enable_limited` | 主题上下文摘要、问题偏好或方法偏好，`confidence >= 0.94` | `manual_required` |
| `source_collection` | `manual_first_with_shadow` | 默认不自动读；只能 shadow 观测来源使用边界和检索偏好 | `manual_required` |

`auto_enable_limited` 表示 policy 可以自动 approve、promote 和 enable read，但只允许
allowlist 中的 candidate type，且 scope ref 必须精确绑定。`manual_first_with_shadow`
表示生产读取必须人工 enable，自动策略只能生成 policy decision 和候选证据。

### 自动可用最低条件

自动 `enable_read` 必须同时满足：

1. 来源是已完成 trace/context 的 `user_message`；
2. journal 已绑定 `context_pack_id`；
3. hard deny 未命中；
4. LLM semantic filter 返回 `is_long_term_memory=true`；
5. LLM semantic filter 返回 `is_temporary_instruction=false`；
6. LLM semantic filter 返回 `is_quoted_or_third_party=false`；
7. LLM semantic filter 返回 `has_contradiction=false`；
8. scope 明确且 ACL 可验证；
9. candidate type 在当前 scope allowlist 内；
10. sensitivity 不高于当前 scope policy 允许值；
11. 不涉及 source fact、reviewer 裁决、action result、任务状态、签署状态或权限变更；
12. confidence 达到当前 scope 阈值；
13. TTL 可计算；
14. revoke、expire、disable 和 anonymize 路径可用。

不满足任一条件时，policy 只能输出 `suppress`、`shadow_only` 或
`pending_manual_review`，不得输出 `enable_read`。

### TTL Policy

| Candidate type | 默认 TTL |
| --- | --- |
| `answer_style_preference` | 90 天 |
| `verbosity_preference` | 90 天 |
| `language_preference` | 180 天 |
| `workflow_preference` | 180 天 |
| `retrieval_preference` | 180 天 |
| `stable_user_background` | 365 天，仅限 `user_private` |
| `research_interest` | 180 天 |
| `research_topic_context` | 90 天 |
| `source_collection_usage_preference` | 90 天，默认 manual enable |
| `pending_manual_review` | 30 天后过期 |

TTL 到期不得删除审计链。到期行为是 `read_enabled=false` 或 card/candidate
`expired`，并写 transition audit。legal hold 可以阻止删除和匿名化，但不得延长读取权限。

### Candidate Type Allowlist

允许自动化的 candidate type：

1. `answer_style_preference`；
2. `verbosity_preference`；
3. `language_preference`；
4. `workflow_preference`；
5. `retrieval_preference`；
6. `stable_user_background`，仅限 `user_private`；
7. `research_interest`；
8. `research_topic_context`；
9. `source_collection_usage_preference`，默认只允许 shadow 或 manual enable。

禁止自动化的 candidate type：

1. `source_fact`；
2. `literary_claim`；
3. `reviewer_decision`；
4. `task_status`；
5. `action_result`；
6. `credential`；
7. `legal_or_identity_assertion`；
8. `permission_or_acl_request`；
9. `temporary_instruction`；
10. `system_or_prompt_instruction`。

### LLM Schema `scoped-memory-llm-filter-v1`

LLM semantic filter 的输出必须匹配以下语义字段：

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

LLM 输出包含以下任一字段时 fail-closed：

1. `approve`；
2. `promote`；
3. `read_enabled`；
4. `acl`；
5. `reviewer_decision`；
6. `evidence_package`；
7. `task_status`；
8. `source_fact`；
9. `tool_permission`；
10. `system_prompt`。

### Read Budget

ContextPackBuilder 必须限制 memory 读取量，防止 context bloat 和 scope 泄露：

1. 每个 `context_pack` 最多读取 8 条 memory ref；
2. `user_private` 最多 4 条；
3. shared scope 总计最多 4 条；
4. `honglou-main` 最多接收 8 条摘要；
5. `honglou-text` 和 `honglou-commentary` 最多接收 2 条非隐私检索偏好；
6. `honglou-reviewer` 不接收 memory 内容，只接收 memory policy digest 和使用摘要；
7. 超出预算时按 confidence、recency、scope specificity 和 policy priority 排序截断；
8. 截断必须写 read decision audit，不得静默丢弃。

## 自动策略分层

自动策略不是单一开关。Scoped Memory Production 必须按 scope 和风险分层：

1. `user_private`：稳定偏好、表达方式、工作方法和长期背景可以在低风险条件下自动
   approve、promote 和 enable read；
2. `profile_common`：只允许对明确绑定 profile、不会改变证据或 reviewer 的低风险
   运行偏好自动化；否则进入 manual review；
3. `knowledge_space`：只能保存知识域使用偏好、检索偏好或工作方法，不能保存
   source fact 或校勘结论；不满足 policy allowlist 时进入 manual review；
4. `research_topic`：只能保存当前研究主题的长期上下文摘要、问题偏好和方法偏好；
   不能把未证实研究结论变成 memory fact；
5. `source_collection`：只能保存来源集合的使用边界、检索偏好或审校提示；默认必须
   manual enable，不能替代 source snapshot、license metadata 或证据登记。

所有 scope 都必须支持 `manual_required` 降级。所有自动策略都必须支持
`shadow_only`，用于发布前观测和策略回滚。

## 规则过滤

规则过滤先于 LLM。命中 hard deny 时不得自动可用：

1. secret、token、API key、密码、cookie；
2. 系统提示词、内部配置、admin key 或工具凭证；
3. prompt injection 或要求绕过 reviewer / evidence / policy 的内容；
4. source fact、原文事实、脂批事实、版本结论、校勘结论；
5. reviewer 裁决、签署状态、任务关闭状态、action result；
6. 明确临时指令，例如“这次先...”、“临时...”、“只在本轮...”；
7. 明确测试输入、调试输入、重复噪声；
8. 无法绑定 user/session/scope 的 journal；
9. 未完成 trace/context 或缺 `context_pack_id` 的 journal；
10. 已 revoked、expired、legal hold 冲突或 retention 不合法的来源。

规则过滤输出必须写入 policy decision 或 collector run report，包含命中规则、
输入 digest、输出 digest、是否调用 LLM 和 suppressed reason。

## LLM Semantic Filter

LLM semantic filter 用于规则难以判断的语义分类。它必须满足：

1. 输入只包含 redacted journal 摘要、scope hint、candidate summary 和 JSON schema；
2. 不输入完整 conversation、Hermes transcript、系统提示词、密钥、未授权 context 或
   journal 原文；
3. 输出只能是 schema-bound JSON；
4. 输出至少包含 `is_long_term_memory`、`is_temporary_instruction`、
   `is_quoted_or_third_party`、`has_contradiction`、`scope_type`、`candidate_type`、
   `confidence`、`sensitivity`、`risk_flags`、`ttl_hint` 和 `exclusion_flags`；
5. LLM 输出包含 promotion、ACL、read_enabled、reviewer 裁决、evidence package 或
   任务状态字段时 fail-closed；
6. LLM 低置信、非法 JSON、未知 scope 或 exclusion flag 命中时不得自动可用；
7. LLM prompt/schema/model/input digest/output digest/failure reason 必须进入 audit。

## 状态机与 Read Enablement

Scoped Memory Production 保留 Memory Candidate Workflow 三层 lifecycle，并正式打开 read enablement lifecycle：

1. candidate lifecycle：沿用 `pending`、`approved`、`rejected`、`expired`、`merged`；
2. card lifecycle：沿用 `active`、`revoked`、`expired`；
3. read enablement lifecycle：新增受控 action `enable_read`、`disable_read`。

允许的自动 transition：

1. `pending -> approved`，仅限 policy decision 为 `auto_approve`；
2. `approved -> active memory_card`，仅限 policy decision 为 `auto_promote`；
3. `read_enabled=false -> true`，仅限 policy decision 为 `enable_read`；
4. `read_enabled=true -> false`，用于 revoke、expire、policy disable、legal hold、
   user revoke 或 incident response。

禁止的 transition：

1. `rejected`、`expired`、`merged` candidate 自动 promote；
2. 未写 policy decision 的 read enablement；
3. LLM 直接设置 `read_enabled=true`；
4. 普通用户输入控制 `read_enabled`、scope、ACL 或 policy；
5. 通过 SQL 绕过 service 和 audit；
6. revoked / expired card 被新的 context pack 读取。

## Context Build 读取规则

ContextPackBuilder 是 Scoped Memory Production 的核心生产边界。它只能读取同时满足以下条件的 memory：

1. `memory_card.status=active`；
2. `memory_card.read_enabled=true`；
3. 未过期、未 revoked、未被 policy disabled；
4. scope 与当前 user/session/profile/knowledge_space/research_topic/source_collection
   匹配；
5. ACL 允许当前 consumer/profile 读取；
6. sensitivity 允许进入该 projection；
7. memory summary 已脱敏；
8. policy decision 和 transition audit 可追溯。

`context_pack.memory_read_refs` 只能包含已授权、已审核或已 policy-enabled 的摘要 ref。
普通 public response、SSE、metrics 和日志不得输出 memory 内部 id、candidate id、
policy payload、journal 原文或 LLM payload。

## Runtime Projection 规则

1. `honglou-main` 可以读取授权 memory 摘要，用于表达偏好、背景和工作方法；
2. `honglou-text` 默认不得读取 `user_private` memory；只有检索策略允许的非隐私摘要可
   进入 projection；
3. `honglou-commentary` 默认不得读取 `user_private` memory；只能读取授权的检索偏好；
4. `honglou-reviewer` 不能把 memory 当证据，不能因为 memory 改变 reviewer 裁决；
5. 所有 projection 必须记录 memory read policy digest 和 memory read ref digest；
6. projection replay 必须能证明相同 memory 读取集合。

## Manual Review 保留规则

人工审核不是默认 blocker，但必须完整存在：

1. admin-only CLI/API 可查询 `pending_manual_review`、auto-enabled、revoked、expired；
2. reviewer/operator 可对 auto-enabled memory 做抽样复核；
3. 用户投诉、撤销或策略回滚时可按 source journal、candidate、policy decision 和
   card 追溯；
4. manual review 使用同一套 service、transition audit 和 lifecycle；
5. 远程 CLI 审核只能走 hhost 本机、SSH 或内网受控通道；
6. 凭证不得进入命令行参数、日志或 public response。

## Lifecycle 与隐私

Scoped Memory Production 必须让 export、anonymize、legal hold 和 retention 覆盖：

1. `session_journal`；
2. `memory_candidate`；
3. `policy_decision`；
4. `memory_card`；
5. `memory_transition_audit`；
6. `context_pack.memory_read_refs`；
7. `context_projection`；
8. 相关 trace/audit ref。

要求：

1. export 只能导出允许披露的摘要、ACL、policy、transition 和来源 ref；
2. anonymize 必须处理 user_private memory、外部 user/chat ref、hash 和 tombstone；
3. legal hold 阻止删除、匿名化和 retention pruning，但不扩大读取权限；
4. revoke/expire/disable 后，新的 context pack 不得读取对应 memory；
5. backup/restore 后 memory 与 context/journal/package/reviewer 链可恢复。

## Work Packages

### 工作包 A：Policy Schema 与配置

- [x] 新增或复用 policy decision 记录。
- [x] 定义 policy mode：`shadow_only`、`auto_policy`、`manual_required`。
- [x] 实现 `scoped-memory-policy-v1`。
- [x] 实现 scope automation matrix。
- [x] 实现 policy threshold、TTL、risk flag 和 LLM schema version。
- [x] 实现 read budget 和截断 audit。
- [x] 配置和 metrics 暴露有效 policy mode，但不暴露敏感 payload。

### 工作包 B：Rule + LLM Filter

- [x] 规则 hard deny 先于 LLM。
- [x] LLM 输入 redaction 和 digest 完整记录。
- [x] LLM 输出 schema-bound JSON。
- [x] LLM 越权字段 fail-closed。
- [x] 低置信、未知 scope、临时指令、引用他人或矛盾内容不得自动可用。

### 工作包 C：Auto Policy Transition

- [x] 自动路径写 `pending -> approved` audit。
- [x] 自动路径写 `approved -> active memory_card` audit。
- [x] 自动路径写 `read_enabled=false -> true` audit。
- [x] 自动 actor 固定为 `memory_policy:auto` 或带 policy version 的等价身份。
- [x] manual review 与 auto policy 使用同一 service 和状态机。

### 工作包 D：Context Build Read Path

- [x] ContextPackBuilder 读取 active/read-enabled memory。
- [x] `memory_read_refs` 只包含授权摘要 ref。
- [x] ACL 不匹配 fail-closed。
- [x] revoked/expired/disabled memory 不进入新 context pack。
- [x] context replay 可复现 memory read refs。

### 工作包 E：Runtime Projection 与回答边界

- [x] memory 只进入授权 consumer projection。
- [x] `honglou-main` 仅把 memory 用作偏好、背景和工作方法。
- [x] `honglou-text`、`honglou-commentary`、`honglou-reviewer` 的 memory 可见性按规则
      fail-closed。
- [x] evidence package 不包含 memory。
- [x] reviewer 裁决不受 memory 改写。
- [x] public response/SSE 不泄露 memory 内部字段。

### 工作包 F：Lifecycle 与运维

- [x] export 覆盖 candidate、policy decision、card、read enablement 和 audit。
- [x] anonymize 覆盖 user_private memory 和关联 ref。
- [x] legal hold 阻止删除/匿名化但不扩大读取权限。
- [x] retention pruning 不破坏 audit 链。
- [x] backup/restore 后 memory/context/journal/package/reviewer 链可恢复。

### 工作包 G：Gate 与发布

- [x] 本地 cargo check/test/clippy 通过。
- [x] collector -> policy -> card -> context build contract smoke 通过。
- [x] auto policy 与 manual review contract smoke 通过。
- [x] `scoped-memory-policy-v1` replay gate 通过。
- [x] LLM schema overreach fail-closed gate 通过。
- [x] read budget / truncation audit gate 通过。
- [x] ACL/scope fail-closed matrix 通过。
- [x] revoke/expire/disable read path smoke 通过。
- [x] export/anonymize/legal hold/restore gate 通过。
- [x] hhost live gate 通过。
- [x] full remote release automation 通过。
- [x] release readiness 记录 scoped memory production 证据，且 p95、错误率和
      post-release monitor 不恶化。

### 本地实现证据

- 2026-05-19 本地实现已覆盖 `memory_policy_decisions`、`scoped-memory-policy-v1`、
  `scoped-memory-llm-filter-v1`、自动 `approve/promote/enable_read`、manual
  review 保留、read budget、projection 隔离、public sanitizer、metrics、lifecycle
  export/anonymize/legal hold 和 backup/restore memory read path。
- 2026-05-19 LLM 参与边界为 schema-bound semantic filter contract：规则过滤和
  redaction/digest 已实现，LLM probe 输出按 `scoped-memory-llm-filter-v1` 校验；
  当前未接外部 LLM 调用，`llm_participation.used=false`，不得把它描述成外部 LLM
  已在线参与授权。
- 2026-05-19 本地验证通过：`cargo test --workspace`、`cargo clippy --workspace
  --all-targets -- -D warnings`、`agent-platform/scripts/tonglingyu-gateway-smoke.sh`、
  `<deployment>/scripts/verify-tonglingyu-rqa-user-lifecycle.sh`、
  `<deployment>/scripts/verify-tonglingyu-rqa-backup-restore-drill.sh`、
  `<deployment>/scripts/test-tonglingyu-release-readiness-contract.sh`。
- 2026-05-19 已部署到 hhost 为 `tonglingyu-gateway:0.1.12`，运行 image id 为
  `sha256:1e1e53ef3d079166a8c3eb1fd2df088a9535d76b7c3efd495aa69d9ef4e6a17f`。
  live gate artifact 为
  `data/tonglingyu/remote-live-gates/remote-live-20260519T143702Z-79221/remote-live-gates.json`，
  model upstream、Open WebUI Function、Open WebUI Admin Action、strict Gateway 和
  scoped context gate 均通过。
- 2026-05-19 完整远端 release automation artifact 为
  `data/tonglingyu/remote-release-automation/remote-release-20260519T184551Z-93162/remote-release-automation.json`，
  `status=ok`、`production_ready_proven=true`、`release_blockers=[]`、
  `required_failures=[]`、`secret_values_printed=false`。release readiness 为
  `status=passed`、`production_release_ready=true`；saved validator 为
  `status=ok`、`production_release_ready=true`、`errors=[]`。
- 2026-05-19 同一 release 绑定 `environment=hhost`、`target=tonglingyu-rqa`、
  git commit `cbba91bb73dd6e3004975eecc0326c32e5c661dd`、`tracked_dirty=false`。
  所有 16 个 required gate 均为 `passed`，包括 `rqa_user_lifecycle`、
  `rqa_backup_restore_drill`、`security_scan`、`release_ops_readiness`、
  `rqa_incident_capacity`、`strict_gateway`、`openwebui_function`、
  `openwebui_admin_action` 和 `openwebui_browser_review`。
- 2026-05-19 容量与值守证据通过：`rqa_write_p95_ms=4618`、
  `admin_read_p95_ms=387`、`metrics_read_p95_ms=173`、
  `release_gate_ms=26759`；post-release monitor 为 60 分钟窗口、
  `sample_count=13`、`failed_sample_count=0`。
- 2026-05-20 设计-实现一致性复查发现并修复两个 contract 漏项，并消除一个
  patch-style 双路径构造点：
  1. policy engine 原先记录了 `source_entry_type_allowed`，但自动策略的 `suppress`
     条件未显式纳入该字段；现已改为非 `user_message` candidate 必须
     `suppress -> rejected`，并写 `source_entry_type_not_allowed` policy/audit reason；
     manual promote 和 read path 也同样要求 `user_message` 与 `context_pack_id`。
  2. projection 原先只有 `memory_policy_digest`，没有显式
     `memory_read_ref_digest`；现已在 `context_pack`、`context_projection` payload、
     admin trace summary 和 smoke/live gate 中固定该 digest。
  3. `context_pack.profile_views` 和 `context_projection` 原先分别构造 profile view；
     现已改为 projection 消费同一份 profile view，避免两条路径后续漂移。
  这些修复不改变 Scoped Memory Production 目标口径，也不放宽 release gate；它们是对已冻结 contract
  的收敛，而不是新的折中路径。
- 2026-05-20 上述一致性收紧已随 `0.1.13` 部署到 hhost，并通过完整生产验证：
  `tonglingyu-gateway` 运行 image 为 `tonglingyu-gateway:0.1.13`，image id 为
  `sha256:214a8977e8454549d2f7f787929fb6bedb62373280ff3459521b7e2d258fb464`。
  live gate artifact 为
  `data/tonglingyu/remote-live-gates/remote-live-20260520T005806Z-8320/remote-live-gates.json`，
  model upstream、Open WebUI Function、Open WebUI Admin Action、strict Gateway 和
  scoped context gate 均通过。
- 2026-05-20 完整远端 release automation artifact 为
  `data/tonglingyu/remote-release-automation/remote-release-20260520T005901Z-8691/remote-release-automation.json`，
  `status=ok`、`production_ready_proven=true`、`release_blockers=[]`、
  `required_failures=[]`、`secret_values_printed=false`。release readiness 为
  `status=passed`、`production_release_ready=true`；saved validator 为
  `status=ok`、`errors=[]`。同一 release 绑定 git commit
  `1ae3a2d633b6eaf55606072645cc257c89bfcffd`、`tracked_dirty=false`，open P0
  retrieval failures / governance tasks 均为 0。
- 2026-05-20 容量与值守证据通过：`rqa_write_p95_ms=4307`、
  `admin_read_p95_ms=409`、`metrics_read_p95_ms=152`、
  `release_gate_ms=26896`；post-release monitor 为 60 分钟窗口、
  `sample_count=13`、`failed_sample_count=0`。

## Fail-closed Matrix

| 场景 | 期望 |
| --- | --- |
| journal 未完成 trace/context | 不生成可用 memory |
| hard deny 命中 secret/token/system prompt | suppress，不调用 LLM |
| prompt injection | suppress 或 manual review，不自动可用 |
| 临时指令 | 不自动可用 |
| source fact / reviewer 裁决 / action result | 不自动可用 |
| LLM 非法 JSON | fail-closed |
| LLM 输出 ACL/read_enabled/promotion 字段 | fail-closed |
| policy decision 缺失 | 不允许 read_enabled=true |
| scope 未知或 ACL 不匹配 | fail-closed |
| user_private 跨用户读取 | gate failed |
| profile_common 跨 profile 读取 | gate failed |
| shared scope 未授权读取 | gate failed |
| revoked/expired memory 进入新 context pack | gate failed |
| memory 进入 evidence package | gate failed |
| memory 改变 reviewer 裁决 | gate failed |
| public response/SSE 泄露 memory id 或 policy payload | gate failed |
| export/anonymize/legal hold 未覆盖 memory | gate failed |
| backup/restore 后 memory 链断裂 | gate failed |

## 退出条件

- [x] `session_journal -> collector -> candidate -> policy -> memory_card -> context_pack`
      主链路在本地和 hhost 均通过。
- [x] 自动策略可让符合 policy 的 scoped memory 进入可用状态。
- [x] 人工审核流程完整保留，且可处理自动 memory 的复核、撤销和回滚。
- [x] 所有 read-enabled memory 都可追溯到 policy decision、source journal、candidate、
      card 和 transition audit。
- [x] `user_private`、`profile_common`、`knowledge_space`、`research_topic` 和
      `source_collection` 都有明确 ACL、读取策略和 fail-closed gate。
- [x] memory 只作为偏好、背景、工作方法或长期上下文摘要进入授权 projection。
- [x] memory 不进入 evidence package，不替代证据，不改变 reviewer 裁决。
- [x] revoke、expire、disable、anonymize 和 legal hold 对新 context build 立即生效。
- [x] public response、SSE、metrics 和普通日志不泄露 memory 内部字段。
- [x] hhost full remote release automation 通过，release readiness 记录 scoped memory
      production 证据。

## 待确认项

无产品/架构方向待确认项。Scoped Memory Production 保持 Scoped Memory Production 目标，自动策略
作为主生产路径，人工审核流程保留但可被策略跳过，LLM 只做语义过滤，最终授权由
versioned policy engine 决定。

实现参数已在 `scoped-memory-policy-v1` 中冻结。后续如需调整阈值、TTL、scope
automation matrix、LLM schema 或 read budget，必须形成新的 policy version，并重新
通过 release gate；不得在实现中临场硬编码或以补丁方式绕过 policy contract。
