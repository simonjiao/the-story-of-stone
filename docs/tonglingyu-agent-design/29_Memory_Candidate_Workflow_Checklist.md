# 29 Memory Candidate Workflow 实现 Checklist

## 状态口径

目标：把 Memory Candidate Workflow 做到目标环境可验证，而不是只新增一个候选表。Memory Candidate Workflow 要实现
Memory Collector、`memory_candidate` 队列、完整审核状态机、admin-only CLI/API、
LLM 辅助抽取边界和 hhost gate。

Memory Candidate Workflow 不能声明 scoped memory production-ready。即使状态机已经完整实现，长期
memory 也不得进入正式回答、`context_pack`、Runtime projection、evidence package
或 reviewer 裁决。Scoped Memory Production 才允许打开 active memory 读取路径和自动 promotion。

## 已冻结决策

1. **状态机完整实现**：Memory Candidate Workflow 实现 `approve`、`promote`、`reject`、
   `reclassify`、`expire`、`revoke`、`merge` 及其 audit。`promote` 可以写出
   active `memory_card` 状态，但 Memory Candidate Workflow 必须保持读取面关闭，不能让 active
   memory 被回答链路消费。
2. **自动 promotion 放到 Scoped Memory Production**：自动 promotion 会让系统在无人确认时产生
   active memory，并且必须同时证明 ACL、retention、revocation、backup/restore、
   capacity 和 live gate 没有缺口。Memory Candidate Workflow 的核心风险是候选抽取和状态机正确性；
   把自动 active 推迟到 Scoped Memory Production，可以避免用候选队列的通过结论提前声明 scoped
   memory 可用。
3. **触发方式三者组合**：后台 worker 是主路径；定时任务和 admin 手动触发是辅助
   路径。三者必须共享同一 lease、水位、幂等键、重试和 audit，不允许各自实现一套
   扫描逻辑。
4. **允许 LLM 参与**：LLM 只能做结构化候选抽取辅助，不能决定事实、权限、scope
   ACL、promotion、reviewer 裁决或 evidence package 内容。LLM 输入必须先经过
   redaction，输出必须是受 schema 校验的 JSON。
5. **production gate 采用严格建议口径**：Memory Candidate Workflow gate 必须证明候选可追溯、禁生成项
   被过滤并审计、scope 不串线、状态机不可绕过、LLM 输出不可越权、public surface
   不泄露、active memory 不被读取，并且 hhost release gate 不恶化。

## 为什么自动 promotion 放到 Scoped Memory Production

自动 promotion 的问题不在于“能不能写一行 active memory”，而在于一旦系统自动把候选
变成 active memory，后续就必须承担完整生产语义：

1. active memory 可能影响未来回答，必须证明不替代证据包和 reviewer；
2. user_private、profile_common、knowledge_space、research_topic 和
   source_collection 的 ACL 必须 fail-closed；
3. revoke、expire、legal hold、export、anonymize 和 backup/restore 必须同时闭合；
4. 容量、错误率和长窗口 post-release monitor 必须重新证明；
5. 低风险分类一旦错判，就会从“待审核噪声”升级为“生产记忆污染”。

因此 Memory Candidate Workflow 只证明“能安全产生、审核、流转和审计候选/卡片状态”，不证明“memory 可被
回答读取”。Scoped Memory Production 再打开 ContextPackBuilder 的 active memory read path，并把自动
promotion 纳入 scoped memory production gate。

## 进入条件

- [x] Scoped Context Request Path 已 production-ready。
- [x] Context Projection Runtime 已 production-ready。
- [x] `session_journal`、`context_pack`、`context_projection` 和 admin trace 已进入
      生产路径。
- [x] public response、SSE 和 metrics 不暴露 context/journal/memory 内部字段。
- [x] 当前生产结论明确不覆盖 Memory Collector、`memory_candidate` 或 active memory。

## 非目标

以下不是 Memory Candidate Workflow 目标：

1. 不声明 scoped memory production-ready；
2. 不让 active memory 进入 `context_pack`；
3. 不让 Runtime profile 读取 active memory；
4. 不让 memory 进入 evidence package；
5. 不让 memory 改变 reviewer 裁决；
6. 不实现公网审核页面；
7. 不把 Open WebUI conversation 或 Hermes transcript 当作 memory 来源；
8. 不把 source fact、reviewer 裁决、签署状态、任务关闭状态或 action result 生成为
   普通 memory；
9. 不实现非 Hermes external agent memory 接入。

## 数据模型

### `memory_candidate`

最低字段：

1. `candidate_id`；
2. `candidate_ref`；
3. `status`：`pending`、`approved`、`rejected`、`expired`、`merged`；
4. `journal_id`；
5. `trace_id`；
6. `user_session_id`；
7. `interaction_context_id`；
8. `context_pack_id`；
9. `source_entry_type`；
10. `scope_type`：`user_private`、`profile_common`、`knowledge_space`、
    `research_topic`、`source_collection`；
11. `scope_ref`；
12. `candidate_type`；
13. `summary`；
14. `summary_sha256`；
15. `raw_excerpt_redacted`；
16. `raw_excerpt_sha256`；
17. `sensitivity`；
18. `risk_flags_json`；
19. `llm_extraction_json`；
20. `confidence`；
21. `created_by`；
22. `created_at`；
23. `updated_at`；
24. `expires_at`；
25. `merged_into_candidate_id`；
26. `audit_ref`.

### `memory_card`

Memory Candidate Workflow 可以实现 `memory_card` 状态机承载表，但读取面必须关闭。

最低字段：

1. `memory_card_id`；
2. `memory_card_ref`；
3. `source_candidate_id`；
4. `status`：`active`、`revoked`、`expired`；
5. `scope_type`；
6. `scope_ref`；
7. `summary`；
8. `summary_sha256`；
9. `acl_json`；
10. `sensitivity`；
11. `promotion_policy_version`；
12. `promoted_by`；
13. `promoted_at`；
14. `revoked_by`；
15. `revoked_at`；
16. `expires_at`；
17. `read_enabled`，Memory Candidate Workflow 固定为 `false`；
18. `audit_ref`.

### `memory_transition_audit`

所有状态变化必须写 transition audit。

最低字段：

1. `transition_id`；
2. `object_type`：`memory_candidate` 或 `memory_card`；
3. `object_id`；
4. `from_status`；
5. `to_status`；
6. `action`；
7. `operator_identity`；
8. `reason`；
9. `before_json`；
10. `after_json`；
11. `trace_id`；
12. `journal_id`；
13. `created_at`.

## 状态机

Memory Candidate Workflow 状态机分为三层：candidate lifecycle、card lifecycle 和 read enablement
lifecycle。实现时不得把这三层混成一个 `status` 字段。

允许 candidate transition：

1. `pending -> approved`；
2. `pending -> rejected`；
3. `pending -> expired`；
4. `pending -> merged`；
5. `pending -> pending`，仅限 `reclassify`，必须写 before/after audit。

允许 card transition：

1. `approved candidate -> active memory_card`，通过人工 `promote` 写入
   `memory_card`，且 `read_enabled=false`；
2. `active -> revoked`；
3. `active -> expired`。

Memory Candidate Workflow read enablement：

1. 所有 `memory_card.read_enabled` 必须为 `false`；
2. ContextPackBuilder、Runtime projection、evidence package 和 final answer 都不得读取
   `memory_card`；
3. 任何把 `read_enabled` 打开的操作都属于 Scoped Memory Production，Memory Candidate Workflow 必须 fail-closed。

禁止 transition：

1. `rejected candidate -> active memory_card`；
2. `expired candidate -> active memory_card`；
3. `merged candidate -> active memory_card`；
4. 未写 reason 的人工状态变化；
5. 缺 operator identity 的远程操作；
6. 通过 SQL 直接改状态；
7. Memory Candidate Workflow 中任何把 `read_enabled` 改为 `true` 的操作。

## Collector 触发

三种触发共享同一个 collector core：

1. background worker：主路径，按 lease 轮询完成 trace/context；
2. scheduled job：辅助路径，用于补偿 worker 停顿和低峰批处理；
3. admin manual trigger：辅助路径，用于指定 trace/context 回放、排障和受控补采。

必须具备：

1. scan watermark；
2. lease owner；
3. lease expires；
4. idempotency key；
5. retry count；
6. backoff；
7. max batch size；
8. dry-run；
9. run summary；
10. audit event。

## LLM 抽取边界

LLM participation 是允许项，但必须受以下 contract 约束：

1. 规则过滤先执行，命中 hard deny 的 journal 不进入 LLM；
2. LLM 输入只包含 redacted journal 摘要、必要 scope hint 和 schema，不包含密钥、
   token、系统提示、完整 conversation、Hermes transcript 或未授权 context；
3. LLM 输出只能是 JSON；
4. JSON 至少包含 `candidate_type`、`scope_type`、`scope_ref`、`summary`、
   `sensitivity`、`risk_flags`、`confidence`、`source_journal_refs` 和
   `exclusion_flags`；
5. `confidence >= 0.75` 可以写入 `pending`；
6. `0.45 <= confidence < 0.75` 可以写入 `pending`，但必须带
   `low_confidence` 和 `requires_manual_review` risk flags；
7. `confidence < 0.45` 不生成 candidate，只写 suppressed audit；
8. LLM 不能决定 `approve`、`promote`、ACL、retention、scope 权限或 reviewer 裁决；
9. LLM 不能把 source fact、reviewer 裁决、签署状态、任务关闭状态或 action result
   变成普通 memory；
10. LLM prompt、schema version、model id、input digest、output digest 和 failure
    reason 必须进入 collector run report。

## Work Packages

### 工作包 A：Schema 与迁移

- [x] 新增 `memory_candidate`。
- [x] 新增 `memory_card`，Memory Candidate Workflow `read_enabled=false`。
- [x] 新增 `memory_transition_audit`。
- [x] 新增 collector run / lease / watermark 表。
- [x] 迁移为 additive，不迁移旧 `gateway_sessions` / `gateway_messages`。
- [x] schema preflight 和 backup/restore gate 覆盖新增表。

### 工作包 B：Collector Core

- [x] 只扫描 `session_journal` 中已写入 trace/context/pack 的条目；admin manual
      trigger 支持指定 trace 回放，background worker 走同一 collector core。
- [x] 读取 `session_journal`，不读取 Open WebUI 原始 conversation 或 Hermes transcript。
- [x] hard deny 过滤密钥、token、系统提示、source fact、reviewer 裁决、签署状态、
      任务关闭状态和 action result。
- [x] 生成 candidate 时绑定 journal、trace、context、pack 和 source entry type。
- [x] 支持 dry-run、idempotency、lease、trigger type、run summary 和 journal status。

### 工作包 C：LLM Extractor

- [x] 规则过滤先于任何 LLM participation；命中 hard deny 时 `llm_called=false` 并写
      audit。
- [x] redaction 与 input digest 已进入 extractor payload；当前 production collector
      使用 `deterministic_rules`，LLM provider 调用未作为自动 promotion 或读取前置条件。
- [x] LLM 输出 probe 走 JSON schema 校验。
- [x] confidence 和 risk flags 按 contract 写入 candidate 或 suppressed audit。
- [x] LLM 越权字段、非法 scope 或 exclusion flag 命中时 fail-closed。
- [x] 单测覆盖 LLM 注入、低置信度、非法 JSON 和越权 promotion。

### 工作包 D：状态机与 CLI/API

- [x] admin-only list/read candidate。
- [x] admin-only `approve`。
- [x] admin-only `promote`，写 `memory_card` 但 `read_enabled=false`。
- [x] admin-only `reject`。
- [x] admin-only `reclassify`。
- [x] admin-only `expire`。
- [x] admin-only `revoke`。
- [x] admin-only `merge`。
- [x] 全部操作强制 reason、operator identity 和 audit。
- [x] CLI 与 API 使用同一 service，不允许两套状态机。

### 工作包 E：安全与 Public Surface

- [x] 普通 chat request 不能指定 memory/candidate/control 字段。
- [x] public response 不返回 candidate/card id。
- [x] SSE 不泄露 candidate/card id、journal 原文或 LLM extractor payload。
- [x] metrics 只输出低基数计数，不输出 trace/journal/candidate id。
- [x] admin API 只允许通过 admin key 访问；公网 Open WebUI 普通 path 不暴露审核入口。
- [x] Cloudflare/Open WebUI public path 不暴露 memory 审核入口。

### 工作包 F：Scope 隔离

- [x] `user_private` 不跨 user，scope ref 使用 `user_private:sha256:*`。
- [x] `profile_common` 不跨 profile；Memory Candidate Workflow 仅允许候选状态流转，不打开读取面。
- [x] `knowledge_space` 不跨知识域；Memory Candidate Workflow 仅允许候选状态流转，不打开读取面。
- [x] `research_topic` 不跨 topic；Memory Candidate Workflow 仅允许候选状态流转，不打开读取面。
- [x] `source_collection` 不跨 source collection；Memory Candidate Workflow 仅允许候选状态流转，不打开读取面。
- [x] 未知 scope fail-closed。
- [x] `project/system/work_item/group` 继续 unsupported / fail-closed。

### 工作包 G：Gate 与发布

- [x] 本地 `cargo fmt --all --check`。
- [x] 本地 `cargo clippy -p tonglingyu-gateway --all-targets -- -D warnings`。
- [x] 本地 `cargo test -p tonglingyu-gateway`。
- [x] 本地 `cargo test -p tonglingyu-runtime`。
- [x] collector contract smoke。
- [x] admin CLI/API contract smoke。
- [x] scoped context live gate 证明 active memory 不参与回答。
- [x] hhost full remote release automation 通过。
- [x] release readiness 记录 Memory Candidate Workflow gate，并且 p95、错误率、post-release monitor 不恶化。

## Memory Candidate Workflow 实现证据（2026-05-19）

Memory Candidate Workflow 已实现并部署为 `0.1.11`，覆盖 Memory Collector、`memory_candidate`、
`memory_card`、三层状态机、admin-only CLI/API、collector 后台 worker / scheduled /
manual 三种触发路径，以及 LLM participation 的 fail-closed contract。该结论只覆盖
memory candidate/card 工作流；active memory 读取路径、自动 promotion 和完整 scoped
memory production gate 仍属于 Scoped Memory Production。

目标环境证据：

1. `hhost` 运行的 `tonglingyu-gateway` image id 为
   `sha256:8fddab2d2d4213641cba382721844374af4ea09265a1b389f36ff6f788bc0109`；
2. live gate artifact：
   `data/tonglingyu/remote-live-gates/remote-live-20260519T082735Z-42867/remote-live-gates.json`；
3. full release automation artifact：
   `data/tonglingyu/remote-release-automation/remote-release-20260519T084157Z-43947/remote-release-automation.stdout`；
4. copied remote release report：
   `data/tonglingyu/remote-release-automation/remote-release-20260519T084157Z-43947/remote-artifacts/release-readiness.json`；
5. copied release automation report：
   `data/tonglingyu/remote-release-automation/remote-release-20260519T084157Z-43947/remote-artifacts/release-automation.json`。

Release 结果：

1. full release automation `status=ok`、`production_ready=true`；
2. wrapper `production_ready_proven=true`、`release_blockers=[]`、
   `required_failures=[]`；
3. release readiness `status=passed`、`production_release_ready=true`；
4. saved validator `status=ok`、`errors=[]`；
5. open P0 retrieval failures / governance tasks 均为 0。

容量与长窗口监控：

1. live capacity load smoke `status=ok`、`errors=[]`；
2. `rqa_write_p95_ms=4553`、`admin_read_p95_ms=382`、
   `metrics_read_p95_ms=162`、`release_gate_ms=26672`；
3. post-release monitor 60 分钟窗口 `sample_count=13`、
   `failed_sample_count=0`、`status=ok`。

Collector 运行边界：

1. background worker 已在 hhost 完成自动运行，最终日志显示
   `processed_count=60`、`candidate_count=0`、`denied_count=0`、
   `suppressed_count=60`；
2. collector SQL gate 只扫描 `user_message`、已绑定 `context_pack_id` 且同一
   trace/context 已存在 `final_response` 的 journal；
3. 当前 production collector 使用 `deterministic_rules`，LLM provider 调用不作为
   自动 promotion、ACL 或读取路径前置条件；LLM contract/probe 只证明 schema、
   越权字段和 fail-closed 边界。

## Fail-closed Matrix

| 场景 | 期望 |
| --- | --- |
| journal 不属于已完成 trace/context | 不生成 candidate |
| journal 缺 trace/context/pack ref | 不生成 candidate，写 audit |
| hard deny 命中 secret/token/system prompt | 不调用 LLM，不生成 candidate |
| source fact 被抽为普通 memory | reject candidate，写 filter audit |
| reviewer 裁决被抽为普通 memory | reject candidate，写 filter audit |
| action result 被抽为普通 memory | reject candidate，写 filter audit |
| LLM 输出非法 JSON | 不生成 candidate，写 failure audit |
| LLM 输出 promotion/ACL/reviewer 字段 | fail-closed |
| scope 未知或未授权 | fail-closed |
| duplicate idempotency key | 不重复生成 candidate |
| 状态跳转非法 | fail-closed |
| 人工操作缺 reason/operator | fail-closed |
| Memory Candidate Workflow `read_enabled=true` | fail-closed |
| public response/SSE 泄露 candidate/card id | gate failed |
| metrics 输出高基数 candidate/journal/trace id | gate failed |

## 退出条件

- [x] candidate 与 journal、trace、context、pack 可追溯。
- [x] 禁止项能被过滤并审计。
- [x] `user_private`、`profile_common`、`knowledge_space`、`research_topic` 和
      `source_collection` 的候选 scope 不串线。
- [x] 完整状态机已实现，所有 transition 写 audit。
- [x] CLI/API 审核路径已通过，且不暴露公网审核入口。
- [x] LLM extractor 只能生成 pending candidate，不能越权决定 promotion、ACL 或 reviewer。
- [x] active `memory_card` 即使存在，也不会进入 `context_pack`、Runtime projection、
      evidence package 或最终回答。
- [x] hhost full remote release automation 通过，且 release gate 记录 Memory Candidate Workflow 证据。

## 待确认项

无。

上述 5 项决策已冻结：完整状态机、自动 promotion 放 Scoped Memory Production、三种触发组合、允许 LLM
辅助抽取、采用严格 production gate。
