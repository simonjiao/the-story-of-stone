# 26 Scoped Context 与受控 Memory 实现规格

## 文档状态

本文是通灵玉 scoped context、session journal 和受控 memory 的实现规格。
它采用稳定规格结构，直接约束后续实现、验收和上线声明。

当前结论：

1. Phase 1 Scoped Context 最小闭环已实现，并已通过当次 `hhost` production gate；
2. 可以进入 **Phase 2 Context-aware Runtime production-ready 实现阶段**；
3. 不能声明 scoped memory production-ready；
4. 不能声明长期 memory、Memory Collector、审核流或 memory lifecycle 已闭合。

当前代码已具备 Phase 1 scoped context：`user_session`、`interaction_context`、
`context_pack`、`session_journal` 和 `resolved_question` 已进入生产路径。下一阶段
重点是让 Runtime profile 只通过受控 `context_projection_ref` 读取自己的上下文投影。
`context_pack` 保持请求级父对象，用于审计、回放和 projection 生成，不作为 Runtime
profile 的直接输入。

## 解释顺序

如果本文和其他文档出现理解差异，按以下顺序解释：

1. 当前状态以 `PROGRESS.md` 为准；
2. Gateway 边界以 `07_Gateway设计.md` 和本文为准；
3. 内部接口以 `10_内部接口契约.md` 和本文为准；
4. 权限、隐私和审计以 `11_权限审计与安全治理.md` 和本文为准；
5. 验收以 `12_验证方案与验收标准.md` 和本文 production gate 为准；
6. Runtime 接入边界以 `20_Runtime接入设计与实施计划.md` 为准；
7. Phase 2 实现细化以
   `28_Phase2_Context_Aware_Runtime_Implementation_Checklist.md` 为准；
8. 生产目标和禁止过早声明以 `21_实现目标与落地计划.md` 为准。

## 架构结论

Scoped Context 与受控 Memory 不是 Hermes memory 插件，也不是第 5 个 Agent。
它是 Gateway 外围的业务上下文治理能力。

固定边界：

1. 外部 Open WebUI conversation 不等于 Hermes session；
2. 业务交互边界由 `interaction_context_id` 表达；
3. Runtime 调用前必须生成受控 `context_pack`；Phase 2 起还必须生成对应的
   `context_projection`；
4. 运行过程必须写 `session_journal`；
5. 长期 memory 只能由延迟 collector 从 journal 抽取、审核和沉淀；
6. 正式事实仍只来自 source snapshot、证据包、reviewer、外部快照、状态流转和
   action/audit log。

Gateway 可以保存 context/journal 索引、摘要、ref 和审计信息，但不能变成长期
memory 系统。Context Governance 的目标态是独立服务；Phase 1 可以同进程实现，
但必须保持独立模块、独立 schema、独立 contract、独立 config、独立 audit 和独立
版本治理。

## 当前实现差距

Phase 1 之后，当前运行路径已经可以被称为 scoped context 最小闭环。剩余差距集中在
Context-aware Runtime：

1. Runtime step input 尚未把 `context_pack_ref` 与 `context_projection_ref`
   作为强制 contract；
2. profile step message 尚未按 consumer projection 做强隔离；
3. Runtime audit 尚未把 context pack ref、projection ref、consumer、runtime
   adapter、schema version、tool policy digest 和 output ref 作为 Phase 2 必填链路；
4. replay 还需要证明可重建 context、Runtime step、package 和 review 链；
5. hhost live gate 还需要覆盖 Phase 2 consumer projection 隔离和 fail-closed 场景。

因此当前可以声明 Phase 1 scoped context production-ready，但不能声明 Phase 2
Context-aware Runtime 完成，也不能声明长期 memory 生产可用。

## 第一版 Scope Taxonomy

第一版只允许以下 scope：

1. `user_private`：单个用户私有偏好、读法和长期使用习惯；
2. `session`：当前 Open WebUI 会话和多轮追问上下文；
3. `knowledge_space`：通灵玉知识域或知识库边界；
4. `research_topic`：一次研究主题或专题上下文；
5. `source_collection`：资料集合、版本和来源边界；
6. `profile_common`：同一 profile 内可复用的受控背景；
7. `audit_scope`：审计、回放和管理视图边界。

第一版暂缓 `project`、`system`、`work_item` 和 `group`。这些 scope 必须
unsupported 或 fail-closed；不得实现伪兼容、空壳读取或默认映射。

## 数据对象

### `user_session`

表示外部用户在 Open WebUI 或其他入口中的用户隔离会话。它不等于 Hermes session，
也不携带 profile 执行 transcript。

最低字段：

1. `user_session_id`；
2. `external_user_ref`；
3. `external_session_id`；
4. `model_id`；
5. `created_at`；
6. `updated_at`；
7. lifecycle / retention 状态。

### `interaction_context`

表示业务交互边界。上下文可以先处于 `unbound` 或 `resolving`，再绑定到 session、
知识空间、研究主题、资料集合、profile 共享背景或审计范围。

最低字段：

1. `interaction_context_id`；
2. `user_session_id`；
3. `context_status`：`resolving`、`active`、`closed`；
4. `context_mode`：`unbound`、`session`、`knowledge_space`、`research_topic`、
   `source_collection`、`profile_common`、`audit_scope`；
5. `resolution_version`；
6. `permission_version`；
7. `memory_policy_version`。

### `context_scope_binding`

表示 context 与 scope 的关系。

最低字段：

1. `binding_id`；
2. `interaction_context_id`；
3. `scope_id`；
4. `scope_type`；
5. `relation_type`：`primary`、`candidate`、`related`、`permission_scope`；
6. `confidence`；
7. `resolved_by`；
8. `status`：`candidate`、`active`、`rejected`、`expired`。

### `context_pack`

每次业务 trace 动态生成的请求级父对象。它不是长期事实源，也不是 Runtime profile
的直接输入。Runtime profile 只能读取从它派生的 `context_projection`。

最低字段：

1. `context_pack_id`；
2. `context_pack_ref`；
3. `trace_id`；
4. `interaction_context_id`；
5. `resolved_question`；
6. `session_summary_ref` 或受控摘要；
7. `active_scopes`；
8. `candidate_scopes`；
9. `policy_versions`；
10. `memory_read_refs`，仅包含已授权、已审核、可读摘要 ref；
11. `forbidden_context`；
12. `schema_version`；
13. `digest`；
14. `created_at`。

### `context_projection`

从 `context_pack` 派生、面向单个 consumer 的 Runtime 可见上下文。它是 Phase 2
新增的一等对象，不是长期 memory，也不参与长期召回。

最低字段：

1. `context_projection_id`；
2. `context_projection_ref`；
3. `context_pack_id`；
4. `context_pack_ref`；
5. `trace_id`；
6. `interaction_context_id`；
7. `consumer_type`；
8. `consumer_name`；
9. `runtime_adapter`；
10. `projection_payload`；
11. `allowed_tools`；
12. `forbidden_tools`；
13. `output_contract`；
14. `tool_policy_digest`；
15. `output_contract_digest`；
16. `schema_version`；
17. `digest`；
18. `status`；
19. `created_at`。

Phase 2 只允许 `consumer_type=runtime_profile`，且只支持 `honglou-main`、
`honglou-text`、`honglou-commentary` 和 `honglou-reviewer`。`external_agent`
是保留 consumer 类型，Phase 2 请求该类型必须 fail-closed。

### `session_journal`

结构化记录业务过程，不等于 SQLite WAL，也不等于 Hermes transcript。

`session_journal.content` 保存用户原文，但按高敏数据治理。journal 同时必须保存
摘要、hash、ref、retention policy、sensitivity 和 trace ref，便于默认 admin 视图
不展示原文。

最低 journal 类型：

1. `user_message`；
2. `metadata_prompt`；
3. `context_pack`；
4. `context_projection`；
5. `runtime_step`；
6. `tool_call`；
7. `tool_result`；
8. `review_result`；
9. `final_response`；
10. `memory_candidate_created`。

### `memory_candidate`

Memory Collector 从已完成 trace/context 的 journal 异步生成候选项。
candidate 默认不参与正式回答。

最低字段：

1. `candidate_id`；
2. `source_journal_id`；
3. `trace_id`；
4. `interaction_context_id`；
5. `scope_type`；
6. `scope_id`；
7. `candidate_type`；
8. `summary`；
9. `hash`；
10. `sensitivity`；
11. `risk_flags`；
12. `suggested_action`；
13. `status`。

### `memory_card`

长期 memory 的统一对象。Phase 1 不实现 active `memory_card`。

最低原则：

1. 必须有 `scope_type + scope_id + profile_name + sharing_policy + ACL`；
2. 默认不能直接 active；
3. `source_type=session_candidate` 必须追溯到 journal；
4. `memory_type` 不能表示正式事实；
5. `memory_card` 不进入 evidence package。

## 运行流程

正式答题路径调整为：

1. Open WebUI 调用 `/v1/chat/completions`；
2. Gateway 鉴权、限流并拒绝内部控制字段；
3. Gateway 创建或恢复 `user_session`；
4. Context Governance 创建或恢复 `interaction_context`；
5. Context Governance 根据 Open WebUI history、journal 和候选 scope 生成
   `session_summary`；
6. Question Resolver 生成 `resolved_question`；
7. Policy Engine 计算 profile、tool、memory read scope 和 forbidden context；
8. ContextPackBuilder 生成请求级受控 `context_pack`；
9. ProjectionBuilder 为每个已登记 Runtime consumer 生成独立
   `context_projection`；
10. Gateway 创建 Runtime step plan，并传入 `context_projection_ref` 与父级
    `context_pack_ref`；
11. Runtime 只按 projection 执行 profile 和 read-only tools；
12. Runtime 生成 evidence package、draft、review result 和 final answer；
13. Gateway 写 `session_journal`；
14. Memory Collector 异步读取 journal 并生成 `memory_candidate`；
15. 审核或明确自动策略通过后才写 active `memory_card`；
16. Gateway 返回 OpenAI-compatible response。

Phase 1 只实现 1 到 13 中的最小 scoped context 路径，不包含
`context_projection` 强制 Runtime 接入。Phase 2 实现 9 到 11 的 projection 强隔离。
Phase 3 才实现 14。Phase 4 才实现 15。

## Consumer Projection 可见性

Phase 2 只支持当前通灵玉 Runtime profile consumer。以下 `honglou-*` 是
`consumer_type=runtime_profile` 下的 `consumer_name`，不是外部用户可指定的模型名。
未知 consumer、未知 Runtime Adapter 和 `external_agent` 在 Phase 2 必须 fail-closed。

<!-- markdownlint-disable MD013 -->
| Consumer | 可见 projection | 不可见上下文 |
| --- | --- | --- |
| `honglou-main` | `resolved_question`、必要 session summary、证据包、reviewer 意见、输出偏好 | 完整用户历史、未授权 memory、系统提示词 |
| `honglou-text` | 检索问题、检索条件、必要正文字形/版本策略 | 完整会话历史、user_private memory、未授权 scoped memory |
| `honglou-commentary` | 脂批/版本检索问题、版本边界、工具约束 | 完整会话历史、正文库原始全量数据 |
| `honglou-reviewer` | 用户问题、草稿、证据包、回答策略、负面清单 | user_private memory、未审核 candidate、Hermes 私有 transcript |
<!-- markdownlint-enable MD013 -->

## 输入与输出安全

外部请求不得控制：

1. `interaction_context_id`；
2. `context_pack_id`；
3. `context_projection_id` 或 `context_projection_ref`；
4. `scope_id`、`scope_graph`、`context_scope_binding`；
5. `memory_read_scopes`、`memory_write_scopes`；
6. `consumer_type`、`consumer_name`、`runtime_adapter`；
7. `allowed_tools`、`forbidden_tools`；
8. `runtime_step_plan`；
9. `reviewer_off`、`disable_reviewer`；
10. 内部 profile 名称；
11. `session_journal` 内容；
12. `memory_card` 状态。

外部可以提供的只有自然语言、附件、Open WebUI identity metadata、conversation id 和
message id。所有业务 context、role、scope、consumer、runtime adapter、tool policy
和 memory policy 必须由 Gateway / Context Governance 重新解析。

普通 `/v1/chat/completions` 只返回 OpenAI-compatible 内容。context、journal、memory
ref 只进入 admin trace 或受控 audit，不进入 public response、metrics 或普通日志。

## 冻结决策

以下决策已经冻结。实现时不得以默认值、配置项或临时分支改写。

1. scope taxonomy 使用本文第一版最小集合。
2. `session_journal.content` 保存用户原文，并按高敏数据治理。
3. `resolved_question` 允许 LLM 参与，但 LLM 只能做结构化解析辅助。
4. Memory Collector 允许极小范围自动 promotion，其余默认 pending 或人工审核。
5. Context Governance 目标态是独立服务。
6. Admin trace 默认不展示原文；特权管理员可按受控流程查看原文。
7. lifecycle、export、anonymize、legal hold 覆盖 context、journal、candidate、
   memory 和相关 trace/audit ref。
8. hhost live gate 必须实测容量、错误率、超时和降级路径后才能声明
   scoped context 或 scoped memory production-ready。
9. Phase 2 使用请求级 `context_pack` + consumer 级 `context_projection`。Runtime
   profile 只能读取 projection；非 Hermes external agent 只保留 schema 扩展点，
   当前 unsupported / fail-closed。

## Question Resolver Contract

Question Resolver 采用规则优先、LLM 结构化辅助、失败 fail-closed。

处理顺序：

1. 先用确定性规则解析当前问题、上一轮对象、session summary 和明确 scope；
2. 规则不足时才调用 LLM resolver；
3. LLM 只能输出 JSON；
4. JSON 不合法时只允许一次 schema repair；
5. 低置信或 schema 不合法时返回澄清问题或 fail-closed。

LLM 输出 schema 至少包含：

1. `resolved_question`；
2. `referent_bindings`；
3. `used_context_refs`；
4. `confidence`；
5. `needs_clarification`；
6. `clarification_question`；
7. `unsupported_reason`。

置信度规则：

1. `confidence >= 0.75` 才接受；
2. `0.45 <= confidence < 0.75` 返回澄清问题；
3. `< 0.45` fail-closed。

LLM 不能决定事实、权限、scope、tool policy、memory ACL、reviewer 裁决或
evidence package 内容。resolver 输入、输出、schema version、confidence 和失败原因
必须写 audit。

## 旧数据策略

既有 `gateway_sessions` 和 `gateway_messages` 直接舍弃。

固定规则：

1. 不迁移；
2. 不备份为新 scoped context 的一部分；
3. 不只读保留；
4. 不进入回放；
5. 不作为 context、journal 或 memory 来源；
6. 不反向生成 `user_session`、`interaction_context` 或 `memory_candidate`。

启用 scoped context 时按新 schema 初始化。若旧表由 Gateway 独占，初始化脚本可以删除
或重建旧表；若与其他在线路径共享，必须先移除读取路径，再由部署 runbook 明确清理
边界。

## Open WebUI Metadata Prompt

Open WebUI metadata prompt 记录为 `metadata_prompt` journal。

记录内容：

1. 来源；
2. 外部 user/chat/message ref；
3. trace ref；
4. hash；
5. 长度；
6. 接收时间；
7. sensitivity；
8. retention policy；
9. 受控摘要。

metadata prompt 不得被当作用户原文、证据、memory candidate 或 profile 输入。

## Admin Trace 与原文查看

默认 admin trace 只展示摘要、hash、ref、权限判定和治理结果。

查看 journal 原文必须满足：

1. 特权角色；
2. 查看理由；
3. audit event；
4. legal hold / anonymize 约束；
5. 受控展示路径。

journal 原文永不进入 public response、metrics、普通日志或 release report。

## Memory Collector 工作流

Memory Collector 冻结为异步 collector + `memory_candidate` 队列 +
admin-only CLI/API 审核。

Phase 1/2/3 不做审核页面。Phase 4 之后若候选量稳定存在，可以在同一套 admin-only
API 之上增加页面，但页面不得绕过状态机。

工作流：

1. Runtime 完成回答后只写 `session_journal`，不直接写 active memory；
2. Collector 异步扫描已完成 trace/context 的 journal；
3. Collector 过滤密钥、token、系统提示、source fact、reviewer 裁决、签署状态、
   任务关闭状态和绕过证据链的请求；
4. Collector 只生成 `memory_candidate`；
5. 低风险 `user_private` 偏好可以由 Promotion Policy 自动 active；
6. 其他 candidate 必须进入审核队列；
7. 未审核 candidate 不参与正式回答；
8. ContextPackBuilder 只读取当前 scope 和 ACL 授权的 active memory 摘要。

待审核项通过 admin-only API 或 CLI 获取。默认查询 `status=pending`，并支持按 scope、
profile、candidate type、sensitivity、risk flags、创建时间和来源 trace 过滤。
列表默认只显示摘要、hash、ref 和风险标记。查看 journal 原文必须具备特权角色、
填写查看理由并写 audit。

支持的操作：

1. `approve`：确认候选可进入下一步，但不等于立即 active；
2. `promote`：在 ACL、scope、retention 和 sensitivity 合法时转为 active memory；
3. `reject`：拒绝候选并记录原因；
4. `reclassify`：调整 scope、sensitivity 或 candidate type，并记录前后差异；
5. `expire`：让候选过期，不再进入审核队列；
6. `revoke`：撤销已 active memory，后续 context pack 不得再读取；
7. `merge`：合并重复候选，保留所有来源 ref。

允许的状态流转：

1. `pending -> approved -> active`；
2. `pending -> rejected`；
3. `pending -> expired`；
4. `pending -> merged`；
5. `pending -> reclassified -> pending`；
6. `active -> revoked`；
7. `active -> expired`。

批量操作只允许：

1. `reject`；
2. `expire`；
3. 低风险 `user_private` 偏好 `approve`。

共享 scope、profile_common、source_collection 或会影响回答策略的 candidate 必须逐条
审核。普通用户只能查看或撤销自己的 `user_private` memory；admin/operator 可处理
队列；reviewer 对影响回答策略的 memory 拥有最终确认权。所有操作都必须写 audit
event，禁止通过手工 SQL 绕过状态机。

远程 CLI 审核是允许路径，但只能通过受控运维通道。优先在 hhost 本机执行，或 SSH 到
hhost 后执行。admin API 只能监听 `127.0.0.1`、容器内网络或内网，不走 Cloudflare
public Open WebUI 入口，不暴露公网审核入口。凭证不得写入命令行参数，必须走 `.env`、
本机 socket、短期凭证或受控配置。远程操作必须记录 operator identity、action、
candidate id、reason、before/after、trace ref 和时间。

## Lifecycle 与运维接口

lifecycle、export、anonymize 和 legal hold 覆盖：

1. `user_session`；
2. `interaction_context`；
3. `context_scope_binding`；
4. `context_pack`；
5. `context_projection`；
6. `session_journal`；
7. `memory_candidate`；
8. `memory_card`；
9. 相关 trace/audit ref。

运维接口采用 admin-only API 或 CLI。

必须具备：

1. dry-run；
2. 执行报告；
3. audit event；
4. 关联记录计数；
5. 失败回滚说明。

export 必须包含关联记录、ACL、promotion、revocation、retention 和可披露原文。
anonymize 必须处理外部 user/chat ref、原文、user_private memory、hash 和
tombstone。legal hold 阻止删除、匿名化和 retention pruning，但不扩大读取权限。
revoke 或 expire 后，相关 memory 不得再进入新的 context pack。

## Phase 1 实现规格

Phase 1 只实现 Scoped Context 最小闭环。

允许实现：

1. Context Governance 同进程独立模块；
2. `user_session`；
3. `interaction_context`；
4. `context_pack`；
5. `session_journal`；
6. `resolved_question`；
7. admin trace 摘要回放；
8. contract、负面清单、strict Gateway gate 和 hhost live gate。

禁止实现：

1. active `memory_card`；
2. Memory Collector；
3. `memory_candidate` 队列；
4. 审核页面；
5. Context Governance 独立服务拆分；
6. 旧 `gateway_sessions` / `gateway_messages` 迁移；
7. `project/system/work_item/group` scoped memory 读取；
8. 不受结构化 contract 和 fail-closed 约束的 LLM 指代解析；
9. 任何让 memory、session summary 或用户偏好进入 evidence package 的路径；
10. 公网 admin API、memory 审核入口或 journal 原文查看入口。

Phase 1 退出条件：

1. 多轮追问可以产生 `resolved_question` 或明确 fail-closed；
2. 超过 `max_messages` 时生成 session summary，不只保留最后一问；
3. context pack 可按 trace 回放；
4. 普通用户不能提交 context、scope、memory 或 tool 控制字段；
5. text/commentary/reviewer 不获得完整用户历史；
6. journal 原文只在受控 admin 路径查看；
7. 单元测试、contract smoke、strict Gateway gate 覆盖上述行为；
8. hhost live gate 覆盖真实 Open WebUI 多轮会话、容器内 Gateway 验证、必要时的
   Cloudflare 公网路径、p95、错误率、超时、降级次数和回滚证据。

## Phase 2 实现规格

Phase 2 目标是 production-ready Context-aware Runtime，不是本地代码切片完成。
完成口径必须同时覆盖实现、测试、projection 隔离、fail-closed、replay、public
surface 和目标 `hhost` live release 证据。

详细实现 checklist、work package、fail-closed matrix 和 hhost gate 见
`28_Phase2_Context_Aware_Runtime_Implementation_Checklist.md`。本文只保留核心规格。

允许实现：

1. Runtime step input 增加 `context_pack_ref` 和 `context_projection_ref`；
2. `context_pack` 保持请求级父对象，不直接进入 profile step message；
3. `context_projection` 成为 Runtime 可见上下文的一等对象；
4. 当前只支持 `consumer_type=runtime_profile` 和四个 `honglou-*` consumer；
5. 未知 consumer、未知 Runtime Adapter 和 `external_agent` fail-closed；
6. `honglou-main` 可见 `resolved_question`、必要 session summary projection 和
   证据包；
7. `honglou-text` / `honglou-commentary` 只见检索问题和工具约束；
8. `honglou-reviewer` 只见用户问题、草稿、证据包和回答策略；
9. Runtime audit 记录 context pack ref、projection ref、consumer、runtime adapter、
   tool policy 和 output ref；
10. reviewer 裁决仍以 evidence package 和本地 review enforcement 为准。

退出条件：

1. consumer 间未授权上下文不可见；
2. Hermes transcript 不替代业务 session；
3. pack/projection schema 或 digest 不匹配时 workflow fail-closed；
4. Runtime profile 不能读取完整 `context_pack`；
5. 未知 consumer、未知 Runtime Adapter 和 `external_agent` 在 Phase 2 fail-closed；
6. replay 能重建 context pack、context projection、Runtime step、package 和
   review 链；
7. strict Gateway gate、scoped context live gate、saved validator、release readiness
   和 `hhost` full remote release automation 全部通过。

## Phase 3 实现规格

Phase 3 实现 Memory Candidate。它不让长期 memory 参与正式回答。

允许实现：

1. Collector 只读取已完成 trace/context 的 journal；
2. candidate 带 `journal_id`、`trace_id`、`interaction_context_id`、scope、摘要和
   sensitivity；
3. candidate 默认 `pending`；
4. candidate 不进入 context pack；
5. 禁止把 source fact、reviewer 裁决、签署状态或 action result 生成为普通 memory。

退出条件：

1. candidate 与 journal 可追溯；
2. 禁止项能被过滤并审计；
3. user_private、profile_common、knowledge_space、research_topic 和 source_collection
   的候选 scope 不串线；
4. 没有 active memory 读取路径。

## Phase 4 实现规格

Phase 4 实现 Scoped Memory Production。

进入条件：

1. Phase 1 到 Phase 3 已通过；
2. ACL、promotion、revocation、lifecycle、backup/restore、live gate 和容量验证已闭合；
3. admin-only CLI/API 审核路径已通过；
4. hhost live gate 未恶化既有 release p95、错误率或降级路径。

允许实现：

1. `memory_card` schema、status、version 和 ACL；
2. promotion、revoke、expire、audit；
3. 低风险 user preference 自动 promotion；
4. profile_common、knowledge_space、research_topic、source_collection 或共享 scope
   memory 审核路径；
5. ContextPackBuilder 读取当前 scope 和 ACL 授权的 active memory 摘要；
6. export、anonymize、legal hold 覆盖 memory candidate 和 memory card；
7. backup/restore 验证覆盖 context、journal、memory、package、review 链；
8. 可选审核页面，但只能调用同一套 admin-only API。

退出条件：

1. memory ACL 不匹配时 fail-closed；
2. active memory 不跨用户、不跨 profile、不跨未授权 scope；
3. memory 不进入 evidence package；
4. memory 不能改变 reviewer 裁决；
5. live release report 证明 capacity、error rate、privacy 和 restore gate 通过。

## Production Gate

该特性不能只靠本地单元测试宣布生产可用。

Scoped context production-ready 至少需要：

1. Phase 1 和 Phase 2 退出条件全部通过；
2. 普通用户不能传入 context、scope、memory、consumer、tool、runtime adapter 或
   reviewer 控制字段；
3. 每个请求都有 `user_session_id`、`interaction_context_id`、`context_pack_id` 和
   journal ref；
4. 每个 Runtime step 都有 `context_projection_id`、projection digest 和父级
   `context_pack_id`；
5. Runtime profile 只读取当前 consumer projection，不读取完整 `context_pack`；
6. 多轮追问能解析或 fail-closed；
7. 多用户同名 conversation 不串上下文；
8. admin trace 可按 trace/context/projection/journal/package 回放；
9. public response 不暴露 context/journal/memory 内部 ID；
10. metrics 不包含高基数字段或用户原文；
11. hhost live gate 通过。

Scoped memory production-ready 还需要：

1. Phase 3 和 Phase 4 退出条件全部通过；
2. memory candidate 与 journal、trace、context 可追溯；
3. active memory 只能由审核或明确自动策略产生；
4. user_private memory 不跨用户；
5. profile_common memory 不跨 profile；
6. shared scope memory 不跨未授权 knowledge_space、research_topic 或 source_collection；
7. memory 不能关闭任务、确认签署、替代证据或改变 reviewer 裁决；
8. export、anonymize、legal hold 覆盖 context、journal、candidate 和 memory；
9. backup/restore 后 context/journal/package/reviewer/memory 链可恢复。

## 当前状态口径

截至 Phase 2 细化完成时，可以声明：

1. 设计决策已冻结；
2. Phase 1 Scoped Context 已实现并通过当次 `hhost` production gate；
3. Phase 2 已达到进入 production-ready 实现阶段的设计条件；
4. Phase 2 Context-aware Runtime 尚未实现；
5. scoped memory 仍未实现；
6. 长期 memory、Memory Collector、审核页面、独立 Context Governance 服务和 memory
   lifecycle 仍未闭合；
7. Phase 2 不能复用 Phase 1 的 production-ready 结论，必须等 Phase 2 实现、测试和
   hhost live gate 通过后再声明；
8. scoped memory production-ready 必须等 Phase 3 和 Phase 4 退出条件全部通过后再
   声明；
9. 非 Hermes external agent 只是 Phase 2 schema 预留点，当前不能声明已接入。
