# Online Evidence Card Ingest Checklist

本文用于约束 evidence card 在线补建、staging、自动入库和后续 job 化实现。
目标不是为某个失败问题补卡，而是建立可审计、可恢复、可去重的证据卡片增长主路径。

## 设计结论

1. 第一阶段一开始就引入 background worker。在线请求只发现 coverage gap
   并创建 staging update request，不阻塞请求上下文等待 card 构建完成。
2. `staged_card` 必须可查询，用于 trace、admin、去重、冲突观察和后续请求识别
   pending work；但不能作为最终回答证据。
3. 最终 `package_v2` 只能绑定已 promoted 的 evidence card。请求当下如果没有
   promoted card，只能基于当前 card store 回答、澄清或受控报告 coverage partial。
4. `recall_advisor` 只能提供结构化线索。入库内容必须由本地 retrieval 命中原文或
   脂批 span，再经 builder、validator、promoter 生成。
5. 规则不足时不能让在线请求自动发明规则。系统应保存 `raw_evidence_candidate`，
   供规则补齐后 reprocess；不能把模型解释固化为 promoted card。
6. 合并、冲突、promote 由确定性规则和 typed code 执行；LLM 不能决定是否合并、
   是否冲突、是否入库。

## 非目标

1. 不把 staged card 放进最终 answer package。
2. 不让请求路径同步等待完整 card ingest 完成。
3. 不把 upstream draft 或 recall advice 直接写成 evidence card。
4. 不为 `通灵宝玉`、`史湘云`、small10 或任何单题写专属补卡逻辑。
5. 不把 query expansion 当作 slot semantics、merge rule 或 conflict rule。
6. 不把 rule gap 伪装成 evidence gap，也不把本地 card 不完善解释成全局无证据。
7. 不用 silent fallback 把 ingest、validator、promote 或 worker 失败包装成成功。

## 反硬编码约束

1. Rust 主流程不得出现面向具体问题、具体人物、具体章节或具体 eval case 的分支。
2. 文学领域可变语义只能通过 versioned external rules 表达；规则描述 slot、
   role、polarity、modality、scope、strength 和 corpus mapping，不能写入某个
   具体问答结论。
3. query expansion 只能增加召回候选词，不能表达“材料支持什么 claim”。
4. answer / review rules 只能表达输出边界和证据使用规则，不能写入特定问题答案。
5. 测试可以使用具体文本 fixture，但断言必须验证通用 contract：状态机、去重、
   冲突、scope、rule gap、package 读取边界，而不是只验证某个问题的最终话术。
6. 新增任何规则文件时必须带 `rules_version`、schema 校验和热加载 / reload
   边界；不得把临时规则散落在 Rust `if/else` 中。
7. 每次实现提交前必须检查新增代码中的中文问题片段、人物名、章节名和 eval id。
   若出现，必须证明它们只在 test fixture、外部规则样例或文档中，而不在主流程逻辑中。

## 数据对象

### `staged_card_update_request`

在线请求发现 coverage gap 时创建。它代表一次补建需求，不代表证据成立。

必需字段：

1. `request_id`；
2. `trace_id`；
3. `session_id`；
4. `resolved_question`；
5. `question_frame`；
6. `coverage_gap_reason`；
7. `source_scope_policy`；
8. `recall_advice_ref`，若未调用上游则为空；
9. `status`：`queued`、`processing`、`completed`、`failed`；
10. `created_at`、`updated_at`。

### `raw_evidence_candidate`

本地 retrieval 找到原文或脂批 span，但现有规则还不能稳定解释为 card assertion
时写入。它可查询、可聚类、可统计，但不能进入 package。

必需字段：

1. `candidate_id`；
2. `update_request_id`；
3. `trace_id`；
4. `source_id`；
5. `source_layer`；
6. `source_hash`；
7. `span_start`、`span_end`；
8. `matched_terms`；
9. `query_frame`；
10. `rule_gap_reason`；
11. `cluster_key`；
12. `created_at`。

### `canonical_staged_card`

规则能解释 claim，但还未 promoted 的 canonical staging 对象。多个请求级
candidate 可以合并到同一个 canonical staged card。

必需字段：

1. `staged_card_id`；
2. `exact_span_key`；
3. `claim_key`；
4. `cluster_key`；
5. `source_scope`；
6. `slot_id`；
7. `canonical_entities_with_roles`；
8. `polarity`；
9. `modality`；
10. `evidence_strength`；
11. `supporting_spans`；
12. `schema_version`；
13. `source_corpus_version`；
14. `source_hash`；
15. `rules_version`；
16. `builder_version`；
17. `validator_version`；
18. `status`：
    `staged`、`merged`、`validated`、`promoted`、`rejected`、
    `conflicted`、`needs_disambiguation`、`superseded_by_promoted`、
    `promote_failed`；
19. `created_from_trace_ids`；
20. `created_at`、`updated_at`。

### `evidence_card`

已 promoted、可进入 `package_v2` 的正式 card。`card_id` 必须由稳定字段生成，
保证重复发现时幂等 upsert。

推荐 key：

```text
hash(source_id + source_hash + span_start + span_end + card_schema_version)
```

当一个 claim 有多个 supporting span 时，主 card id 仍要稳定；新增 span 只能经过
validator 后作为 provenance 或 support span 追加，不能改变原 card 的 claim 语义。

### `staged_card_event`

所有状态推进、合并、冲突、validator 结果和 promote 结果都必须写 event。

必需字段：

1. `event_id`；
2. `trace_id`；
3. `update_request_id`；
4. `staged_card_id` 或 `candidate_id`；
5. `event_type`；
6. `from_status`、`to_status`；
7. `reason_code`；
8. `rule_id`；
9. `created_at`。

## 合并规则

合并只能减少重复，不能提高证据强度，不能扩大 source scope，不能把线索提升为
直接事实。

1. **`exact_span_key` 相同**：同一 `source_id + source_hash + span_start +
   span_end + card_schema_version` 直接合并，只追加 trace、hit count 和 query frame。
2. **`claim_key` 相同**：同一 `source_scope + slot_id +
   canonical_entities_with_roles + polarity + modality + evidence_strength +
   rules_version` 合并为一个 canonical staged card，保留多个 supporting spans。
3. **span 重叠且 claim 相同**：同一 source、slot、实体角色、polarity、modality
   和 evidence strength，span 明显重叠时合并；canonical span 选择边界更完整、
   更自然者，同时保留原 supporting spans。
4. **已有 promoted card 覆盖同一 claim**：不生成新 promoted card。candidate
   转为 `superseded_by_promoted`，必要时追加 provenance 或 supporting span，但必须
   先通过 validator。
5. **同一 span 支持多个 slot**：不合成一个事实 card。可以共用 source span，
   但必须生成不同 card assertion，避免“材料相同”误合并成“事实相同”。
6. **相似但不能确定同一 claim**：只进入同一 `cluster_key`，不 merge、不 promote。
7. **规则不足的材料**：只能保存为 `raw_evidence_candidate`，不能合并为
   `canonical_staged_card`。

## 冲突规则

冲突规则优先级高于合并规则。冲突影响 claim、scope、role、polarity、modality、
strength 或版本时，必须阻断 promote。

1. **polarity 冲突**：同一 `slot_id + canonical_entities_with_roles` 出现肯定与
   否定或互斥断言，状态转为 `conflicted`，禁止 promote。
2. **role 冲突**：主体、客体、谓词角色方向不一致，状态转为
   `needs_disambiguation`，禁止 promote。
3. **source scope 冲突**：默认 scope 是前八十回正文 + 脂批。后四十回、程高本
   补文、异本文本不能与默认 scope 混合 promote，只能形成不同 scope card。
4. **modality 冲突**：已发生叙事、伏笔提示、疑似线索、评语推断不能合并成同一
   确定事实。
5. **evidence strength 冲突**：直接叙事证据、旁证、线索、版本说明不能互相提升
   强度。可以同 cluster，但不能合成更强 claim。
6. **entity resolution 冲突**：实体别名无法稳定绑定，或一个 mention 对应多个候选
   实体，不能 promote 为确定 card。
7. **source hash 冲突**：同一 source id 但 hash 不同，旧 staged card 必须重新
   validate；不能直接合并或 promote。
8. **schema / rule / builder version 冲突**：跨版本 candidate 必须重跑 builder 和
   validator；不能直接沿用旧结果。
9. **promoted card 语义冲突**：candidate 与已 promoted card 在同一 claim key
   下产生冲突时，不能覆盖 promoted card；必须创建 conflict event 并阻断新 promote。
10. **LLM-only claim 冲突**：只有 upstream advice 支持、本地 span 不支持的 claim
    不进入 staged card；只能记录为 recall advice 或 rule gap。

## 状态机

`raw_evidence_candidate`：

```text
observed -> rule_gap
observed -> staged_card_created
rule_gap -> reprocessed_after_rule_update
```

`canonical_staged_card`：

```text
staged -> merged -> validated -> promoted
staged -> rejected
staged -> conflicted
staged -> needs_disambiguation
validated -> promote_failed
validated -> superseded_by_promoted
```

状态推进要求：

1. `promoted` 前必须有 validator event。
2. `conflicted`、`needs_disambiguation`、`rejected` 不得自动 promote。
3. `promote_failed` 不能被 answer path 当作成功。
4. 每次状态变化必须能从 trace 找回触发请求和规则版本。

## 分阶段实施

### Phase 0: Checklist 与规则边界冻结

- [x] 新增并冻结本文 checklist。
- [x] 明确 code invariant 与外部语义规则边界：
  - [x] code invariant：幂等 key、状态机、source hash、schema version、promote
        upsert、trace event。
  - [x] 外部规则：slot、entity role、polarity、modality、evidence strength、
        source scope、哪些材料类型能支持哪些 claim。
- [x] 明确 `raw_evidence_candidate`、`canonical_staged_card`、
      `evidence_card` 三层模型。
- [x] 明确 query expansion 只负责找材料，不表达 slot 语义、合并语义或冲突语义。
- [x] 新增反硬编码检查清单：主流程不得含具体问题、人物、章节或 eval case 分支。
- [x] 明确 promoted 之前必须先实现合并和冲突阻断；不能先做 happy path promote，
      再把冲突处理留到后续补丁。

### Phase 1: Async staging worker 主路径

第一阶段已经是异步主形态，不做请求内同步 promote。同时，Phase 1 必须包含
promote 前必需的合并与冲突规则；不能先让 worker 自动入库，再把冲突处理推迟。

- [x] coverage checker 在 package 覆盖不足时创建
      `staged_card_update_request`，不阻塞请求上下文。
- [x] background worker 消费 staging queue，执行 local retrieval、card builder、
      validator 和自动 promote。
- [x] staged card、raw candidate、events 都可通过 admin / trace 查询。
- [x] 后续请求能查询 pending staged card，用于去重和观察，但不能把 pending card
      放入最终 package。
- [x] 最终回答只读取已 promoted 的 evidence card。
- [x] worker 完成 promote 后，后续请求从 card store 自然受益。
- [x] trace 记录 coverage gap、update request、raw candidate、staged card、
      merge/conflict、validator、promote 结果。
- [x] LLM recall advisor 只写 recall advice，不写 card、不决定 promote。
- [x] rules 不足时生成 `raw_evidence_candidate` 和 `rule_gap` event，不 promote。
- [x] 实现 `exact_span_key` 幂等合并。
- [x] 实现 `claim_key` 合并。
- [x] 实现 span 重叠且 claim 相同的 canonical span 选择。
- [x] 实现 promoted card 覆盖同一 claim 时的 `superseded_by_promoted`。
- [x] 实现同一 span 多 slot 的分卡逻辑。
- [x] 实现相似但不确定同 claim 的 cluster-only 逻辑。
- [x] 实现 polarity、role、scope、modality、evidence strength、entity resolution、
      source hash、schema、rule、builder version 冲突阻断。
- [x] 实现冲突 event 查询和 trace 展示。
- [x] Rust module tests 放入单独 tests 文件，覆盖 staging request、worker 主路径、
      staged 可查询、package 禁用 staged card、promote 后可读。
- [x] 补 property-style 或 table-driven tests，证明合并不能提高证据强度。

### Phase 2: Job 管理与中断恢复

第二阶段把 worker 升级成完整 job runtime。Phase 1 已经必须保证 staging、
合并、冲突和 promote 语义正确；Phase 2 只解决中断恢复、重试和长期运行可靠性。

- [x] 新增 durable `card_ingest_job`，关联 update request、candidate、staged card。
- [x] 实现 `leased_by`、`lease_until`、heartbeat、`attempt_count`、`last_error`。
- [x] 实现启动 reconciler，恢复 `queued`、`processing` 超时、`promote_failed`、
      lease expired 的 job。
- [x] 实现 retry policy、backoff 和 dead letter。
- [x] job event stream 可关联原始 trace，后续异步事件不丢失。
- [x] worker crash / restart 后从最近安全阶段继续，不重跑整条用户请求。
- [x] promote/upsert 全程幂等，重复执行不制造重复 card。

### Phase 3: Package、answer、reviewer 与 eval gate

- [x] package builder 只读取 promoted evidence card。
- [x] answer composer 对 coverage partial 做业务化表达，不泄露 staged、job、trace
      等内部字段。
- [x] reviewer 拒绝使用 staged / raw candidate / recall advice 作为正式证据。
- [x] admin trace 显示 staged/promotion 链路，普通响应不泄露内部治理字段。
- [x] 既有 eval suite 和代表性多轮领域对话回归通过，但不得把回归 case 写成
      主流程特例。
- [x] 新增在线补卡 fixture：第一次请求创建 staging request，worker promote 后，
      第二次请求使用 promoted card。
- [x] 新增规则缺口 fixture：保存 raw candidate，但不进入 package。
- [x] 新增冲突 fixture：相反 polarity、不同 source scope、不同 modality 均阻断
      promote。
- [x] 新增反硬编码 fixture：更换实体名、slot alias、source scope 后，流程仍按
      contract 工作，不依赖固定问题文本。
- [x] hhost 部署后验证版本、运行镜像、OCI label、远端 env、worker health、
      admin trace 和 eval report。

## 阶段验收矩阵

阶段不能只用“代码已写完”关闭。每个阶段都必须有可重复运行的测试、trace、
report 或部署证据；人工观察只能作为补充，不能作为唯一验收。

### Phase 0 验收

- [x] checklist 文档通过 `markdownlint-cli2` 和 `git diff --check`。
- [x] schema / rules 边界有独立 review 记录：哪些是 code invariant，哪些是
      external rules，哪些禁止进入 query expansion。
- [x] 反硬编码检查有可运行命令或脚本，至少扫描主流程代码中的具体问题文本、
      人物名、章节名、eval id 和临时 `if/else`。
- [x] 文档地图已接入本 checklist，后续实现不得绕过本文阶段边界。
- [x] Phase 0 关闭时必须有提交记录；不能把未冻结 checklist 当作实现依据。

### Phase 1 验收

- [x] Rust unit tests 覆盖 `staged_card_update_request` 创建、raw candidate 写入、
      canonical staged card 生成、validator、promote 和 package reload。
- [x] Rust unit tests 覆盖 package builder 拒绝 staged / raw candidate / recall
      advice 进入最终 package。
- [x] table-driven tests 覆盖 exact span merge、claim merge、overlap merge、
      cluster-only、multi-slot split、superseded-by-promoted。
- [x] table-driven tests 覆盖 polarity、role、scope、modality、strength、
      entity resolution、source hash、schema / rule / builder version 冲突。
- [x] 集成测试覆盖一次请求创建 staging request，worker 自动处理，后续请求读取
      promoted card；当前请求不使用 pending card。
- [x] trace fixture 覆盖 coverage gap、update request、raw candidate、merge/conflict、
      validator、promote event。
- [x] 失败验收覆盖 worker / validator / promote 失败时不写入 package、不伪装成功。

### Phase 2 验收

- [x] Rust unit tests 覆盖 lease acquire、heartbeat、lease expired 接管、attempt
      count、last error 和 retry backoff。
- [x] Rust unit tests 覆盖 reconciler 从 `queued`、`processing` 超时、
      `promote_failed`、dead letter 前状态恢复。
- [x] fault-injection integration test 覆盖 worker 中断后重启，从最近安全阶段继续。
- [x] 幂等回放测试覆盖重复执行同一 job 不产生重复 card、不重复 promotion event。
- [x] job event stream 测试覆盖原始 trace 与后续异步 event 可关联。
- [x] 运维验收覆盖 worker health、queue depth、retry count、dead letter count 指标。

### Phase 3 验收

- [x] package / answer / reviewer 单元测试覆盖：只读取 promoted card，拒绝 staged、
      raw candidate、recall advice 作为正式证据。
- [x] public response fixture 覆盖 coverage partial 的业务化表达，不泄露 staged、
      job、trace、rule id 或内部状态机。
- [x] eval suite 覆盖在线补卡、规则缺口、冲突阻断、反硬编码和代表性多轮领域对话。
- [x] hhost deploy gate 覆盖运行镜像版本、OCI label、远端 env、worker health、
      admin trace、eval report。
- [x] 回归结果必须同时包含成功路径和受控失败路径；不能只用 happy path 证明上线。

## Exit Criteria

- [x] 所有新增 Rust module tests 独立放在 tests 文件中。
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
      通过。
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
      通过。
- [x] `scripts/qa.sh --quick` 通过。
- [x] hhost 部署后能证明：
  - [x] 运行镜像版本等于 source `VERSION`；
  - [x] OCI label 等于 source `VERSION`；
  - [x] worker 正常消费 staging queue；
  - [x] trace 能看到 online ingest 链路；
  - [x] final package 不包含未 promoted card；
  - [x] eval report 通过。

## 本轮收口证据

本轮以 source commit `05606c05915931a30cfb75144e7f62722f9a7c7d` 和
gatekeeper commit `51eba94c7f6d354eba510acbf1e7a0672a3b8d09` 为部署基线。
runtime 版本为 `0.7.5`。

本地验证：

1. `cargo check --release --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
   通过，无 release warning。
2. `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
   通过，`202 passed`。
3. `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
   通过，`135 passed`。
4. `scripts/qa.sh --quick` 通过。
5. `scripts/check-tonglingyu-no-question-hardcode.sh --base HEAD` 通过。

hhost 部署验证：

1. deploy run `hhost-deploy-20260524T173853Z-6420` 状态为 `ok`。
2. 远端 version gate 显示 `compose_image=tonglingyu-gateway:0.7.5`、
   `running_image=tonglingyu-gateway:0.7.5`、`compose_label_version=0.7.5`、
   `running_label_version=0.7.5`、`env_version=0.7.5`。
3. 远端 env state 显示 `tonglingyu_version_matches_source=true`，
   `online_evidence_card_worker_enabled=true`，batch size `20`，interval `30s`，
   retrieval limit `12`。

在线 ingest 验证：

1. small10 eval run `small10-0.7.5-20260524T174549Z` 通过，`10 passed / 0 failed`。
2. targeted eval run `targeted-0.7.5-20260524T175119Z` 通过，`2 passed / 0 failed`，
   覆盖 `通灵宝玉丢了几次` 与 `史湘云的结局` 后追问 `脂批中的证据呢`。
3. worker metrics 显示 `update_requests.completed=2`、`jobs.completed=2`、
   `raw_candidate_count=24`、`staged_cards.total=0`。
4. traces `tly-019e5b1aaa497c508ea35d8eb583b62e` 和
   `tly-019e5b1badc97f50b0d2d8d801945634` 显示 coverage gap 创建 update request，
   background worker lease job，记录 raw candidates，并完成 job。
5. 对 small10 与 targeted run 的 final package 检查未发现 `staged_card_id`、
   `raw_candidate`、`update_request_id` 或 `card_ingest_job` 进入正式 package。
