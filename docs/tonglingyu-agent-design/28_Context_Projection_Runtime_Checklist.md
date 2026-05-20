# 28 Context Projection Runtime 实现 Checklist

## 状态口径

目标：把 Context Projection Runtime 做到目标环境 production-ready，而不是只完成本地代码切片或文档
闭环。具体目标是把 Scoped Context Request Path 生成的 scoped context 真正接入 Runtime 执行链，并把
Runtime 可见上下文从请求级 `context_pack` 收敛为面向 consumer 的
`context_projection`，最终通过本地验证、strict Gateway gate、scoped context live
gate 和 `hhost` full remote release automation。

当前状态：Context Projection Runtime 已实现并通过目标环境 production gate。`hhost` 当前部署版本为
`0.1.7`，Gateway 运行 image id 为
`sha256:93df2e2555669a77097590eda1bb4b63e6cc709b58604f741fbf04ce1c6845ab`；
live gate artifact 为
`data/tonglingyu/remote-live-gates/remote-live-20260519T013236Z-78446/remote-live-gates.json`，
full remote release automation artifact 为
`data/tonglingyu/remote-release-automation/remote-release-20260519T013318Z-78823/remote-release-automation.json`。
该结论只覆盖 Context Projection Runtime，不覆盖长期 memory、Memory Collector、
审核页面或非 Hermes external agent 接入。

Context Projection Runtime 只覆盖 Context-aware Runtime。它不实现长期 memory、Memory Collector、
memory 审核页面或 Context Governance 独立服务拆分。

## 防过早声明规则

Context Projection Runtime 允许声明的状态只能按证据逐级推进：

1. 设计已对齐：只表示文档、contract、验收和边界一致；
2. 本地实现已完成：只表示代码和本地测试通过；
3. 目标环境已验证：只表示 `hhost` live gate 通过；
4. production-ready：必须同时满足本地验证、strict Gateway gate、scoped context
   live gate、full remote release automation、saved validator 和 release readiness。

以下情况一律不能声明 Context Projection Runtime production-ready：

1. 只完成文档或 checklist；
2. 只完成本地单元测试、smoke 或 clippy；
3. 只证明 Scoped Context Request Path production-ready；
4. 只新增 `context_projection` schema，但 Runtime 仍能读取完整 `context_pack`；
5. 只预留 `external_agent` schema，但没有独立实现和 gate；
6. hhost gate 未覆盖 consumer projection 隔离、fail-closed、replay 和 public
   surface 泄露检查；
7. saved validator、release readiness 或 open P0 retrieval/governance 检查缺失。

## 进入条件

- [x] Scoped Context Request Path 已 production-ready。
- [x] `hhost` 当次完整 remote release automation 已通过：
      `remote-release-20260518T181347Z-50849`。
- [x] `user_session`、`interaction_context`、`context_pack`、`session_journal`
      和 `resolved_question` 已进入生产路径。
- [x] public response 不暴露 context、journal 或 memory 内部字段。
- [x] active memory、Memory Collector、`memory_candidate` 和审核入口仍未实现。

## 非目标

以下不是待办项，而是 Context Projection Runtime 实现时必须持续满足的禁止边界：

1. 不实现 Memory Collector；
2. 不实现 `memory_candidate` 队列；
3. 不实现 active `memory_card`；
4. 不实现 memory 审核页面或公网审核入口；
5. 不把 Hermes transcript 当作业务 session；
6. 不让 context、session summary 或用户偏好进入 evidence package；
7. 不让 Runtime profile 读取完整 Open WebUI history、journal 原文或完整
   `context_pack`；
8. 不让用户通过请求字段指定 `context_pack_ref`、`context_projection_ref`、
   consumer、profile、tool policy、Runtime Adapter 或 reviewer 状态；
9. 不在 Context Projection Runtime 宣称支持非 Hermes external agent。`external_agent` 只作为保留
   consumer 类型，默认 unsupported / fail-closed。

## 核心确认

1. `context_pack` 是一次请求或 trace 级的受控上下文包。
   它用于审计、回放和生成 projection，不是 Runtime profile 的直接输入。

2. `context_projection` 是 Runtime 可见上下文的一等对象。
   每个 Runtime step 只能通过 `context_projection_ref` 读取自己的 projection；
   `context_pack_ref` 只作为父级绑定和回放锚点。

3. Runtime 上下文入口必须同时绑定 pack 与 projection。
   `context_pack_ref` 绑定 trace、interaction context、pack schema version 和
   pack digest；`context_projection_ref` 绑定 trace、pack、consumer、projection
   schema version、projection digest、tool policy digest 和 output contract
   digest。

4. Context Projection Runtime 只实现当前通灵玉 Runtime consumer。
   当前允许的 `consumer_type` 是 `runtime_profile`，当前允许的
   `runtime_adapter` 是现有 Hermes/Tonglingyu Runtime Adapter，当前允许的
   `consumer_name` 是 `honglou-main`、`honglou-text`、`honglou-commentary` 和
   `honglou-reviewer`。

5. 非 Hermes agent 只做 schema 预留，不做假支持。
   `consumer_type=external_agent`、未知 `runtime_adapter` 或未知 `consumer_name`
   必须 fail-closed，并写 audit；不得空壳映射到 `honglou-main` 或默认 profile。

6. 每个 consumer 只能读取自己的 projection。
   `honglou-text` 和 `honglou-commentary` 不能看到完整 session summary；
   `honglou-reviewer` 不能看到 user_private memory、未审核 candidate、Hermes
   私有 transcript 或可改变 reviewer 裁决的上下文。

7. Context-aware Runtime 不改变事实来源。
   正式事实仍只来自 source snapshot、证据包、reviewer、外部快照、状态流转和
   action/audit log。

8. Reviewer 仍是本地 review enforcement 的最终裁决点。
   context projection 只能影响问题解析、检索约束和回答策略，不能覆盖 reviewer
   裁决。

9. replay 必须使用当时保存的 pack/projection snapshot 或 digest 绑定记录。
   replay 不能从当前 journal 重新推导新的 pack/projection 后冒充历史执行。

10. hhost live gate 必须重新跑。
    Scoped Context Request Path 的 production run 证明进入条件，不证明 Context Projection Runtime 已完成。

## Runtime Context Contract

新增 Runtime step input 字段：

1. `trace_id`；
2. `interaction_context_id`；
3. `context_pack_ref`；
4. `context_pack_schema_version`；
5. `context_pack_digest`；
6. `context_projection_ref`；
7. `context_projection_schema_version`；
8. `context_projection_digest`；
9. `consumer_type`；
10. `consumer_name`；
11. `runtime_adapter`；
12. `tool_policy_digest`；
13. `output_contract_digest`。

字段规则：

1. `context_pack_ref` 和 `context_projection_ref` 必填；
2. `context_projection_ref` 必须属于当前 `context_pack_ref`；
3. pack 与 projection 的 trace、`interaction_context_id` 必须一致；
4. pack schema version 和 projection schema version 必须被当前 Gateway 与 Runtime
   Adapter 支持；
5. pack digest 和 projection digest 必须匹配持久化 snapshot；
6. `consumer_type`、`consumer_name` 和 `runtime_adapter` 必须等于 projection 绑定值；
7. 当前 Context Projection Runtime 只接受 `consumer_type=runtime_profile`；
8. `tool_policy_digest` 必须匹配 projection 和 Runtime step plan 的 allowed tools；
9. `output_contract_digest` 必须匹配 projection 输出 contract；
10. 任一校验失败时 workflow fail-closed，并写 audit。

## Context Pack 与 Projection Schema

### `context_pack`

请求级父对象，不直接进入 Runtime profile message。

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

面向单个 consumer 的 Runtime 可见上下文。

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

## Consumer Projection

### `honglou-main`

可见：

1. `resolved_question`；
2. 必要 session summary projection；
3. evidence package ref 和 reviewer 意见；
4. 输出偏好；
5. forbidden context。

不可见：

1. 完整 Open WebUI history；
2. journal 原文；
3. 未授权 memory；
4. 系统提示词；
5. 未审核 candidate。

### `honglou-text`

可见：

1. 检索问题；
2. 检索条件；
3. 必要正文字形和版本策略；
4. allowed read-only tools；
5. output contract。

不可见：

1. 完整 session summary；
2. user_private memory；
3. 脂批 consumer 私有推理；
4. journal 原文；
5. 未授权 scoped memory。

### `honglou-commentary`

可见：

1. 脂批或版本检索问题；
2. 版本边界；
3. 对应正文需求；
4. allowed read-only tools；
5. output contract。

不可见：

1. 完整用户历史；
2. 正文库原始全量数据；
3. user_private memory；
4. journal 原文；
5. 未授权 scoped memory。

### `honglou-reviewer`

可见：

1. 用户问题；
2. 草稿；
3. 证据包；
4. 回答策略；
5. 负面清单；
6. forbidden context。

不可见：

1. user_private memory；
2. 未审核 candidate；
3. Hermes 私有 transcript；
4. 完整 journal 原文；
5. 可改变 reviewer 裁决的任何上下文。

## 工作包

### 工作包 A：Contract 与 schema

- [x] 定义请求级 `context_pack` schema。
- [x] 定义 consumer 级 `context_projection` schema。
- [x] 定义 Runtime context input 结构。
- [x] 为 pack ref、projection ref、schema version、digest、consumer、runtime
      adapter、tool policy 和 output contract 增加校验函数。
- [x] 为不支持 schema version、pack digest mismatch、projection digest
      mismatch、consumer mismatch、tool policy mismatch 和 output contract
      mismatch 增加错误类别。

### 工作包 B：Context Governance 到 Runtime 的传递链

- [x] Context Governance 为每次 trace 生成请求级 `context_pack`。
- [x] Context Governance 为每个已登记 consumer 生成独立 `context_projection`。
- [x] Gateway Runtime step plan 携带 `context_projection_ref` 和父级
      `context_pack_ref`。
- [x] Gateway 不把完整 context pack 或 projection 塞进 public response、SSE 或
      普通日志。
- [x] 去重 replay 请求必须复用原 trace 的 pack/projection ref，不重新解析用户历史。

### 工作包 C：Runtime Adapter projection enforcement

- [x] Runtime workflow input 从 `question` 单字段升级为 question +
      `context_projection_ref` + `context_pack_ref`。
- [x] Runtime Adapter 只按 projection ref 读取 Runtime 可见上下文。
- [x] Runtime Adapter 不向 profile step message 注入完整 context pack。
- [x] 未知 `consumer_type`、`consumer_name` 或 `runtime_adapter` fail-closed。
- [x] `external_agent` consumer 类型保留但默认 unsupported / fail-closed。

### 工作包 D：Consumer projection isolation

- [x] Runtime 在进入每个 consumer 前读取并校验对应 projection。
- [x] profile step message 只包含该 consumer projection。
- [x] `honglou-text` / `honglou-commentary` 测试证明看不到完整 session summary。
- [x] `honglou-reviewer` 测试证明看不到未审核 memory/candidate 和 Hermes transcript。

### 工作包 E：Tool policy binding

- [x] Runtime allowed tools 必须来自 context projection 和 step plan 的交集。
- [x] 任一 consumer 请求未授权 tool 时 fail-closed。
- [x] `tool_policy_digest` 与 projection 或 step plan 不一致时 fail-closed。
- [x] 负面测试覆盖用户伪造 `allowed_tools`、`forbidden_tools` 和
      `runtime_step_plan`。

### 工作包 F：Audit、admin trace 与 replay

- [x] Runtime audit 记录 pack ref、projection ref、consumer、runtime adapter、tool
      policy digest、output ref 和 schema version。
- [x] admin trace 只展示 context 摘要、hash、ref、consumer、runtime adapter 和校验
      状态。
- [x] replay 可以重建 context pack、context projection、Runtime step、package 和
      review 链。
- [x] replay 不读取当前 journal 来替代历史 pack/projection。

### 工作包 G：Public surface 与隐私

- [x] public chat response 不返回 context/journal/memory 内部 ID。
- [x] SSE chunk 不泄露 context projection、context pack 或 journal 原文。
- [x] metrics 不包含用户原文、高基数 context id、projection id 或 journal id。
- [x] 普通用户不能提交 context、scope、memory、consumer、tool、runtime adapter 或
      reviewer 控制字段。

### 工作包 H：本地验证

- [x] `cargo fmt --all --check`。
- [x] `cargo clippy -p tonglingyu-gateway --all-targets -- -D warnings`。
- [x] `cargo test -p tonglingyu-gateway`。
- [x] `cargo test -p tonglingyu-runtime`。
- [x] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`。
- [x] `../tonglingyu-gatekeeper/deploy/scripts/verify-tonglingyu-scoped-context-live.sh` 增强或新增 Context Projection Runtime
      projection gate。
- [x] `scripts/qa.sh --quick`。

### 工作包 I：hhost production gate

- [x] 按当时版本规则完成版本更新：`0.1.6` -> `0.1.7`。
- [x] 同步 release tools 到 `hhost`。
- [x] 重建并部署 `tonglingyu-gateway` 镜像。
- [x] 运行 strict Gateway live gate。
- [x] 运行 scoped context live gate，覆盖多轮追问、consumer projection 隔离和
      fail-closed。
- [x] 运行 full remote release automation：
      `remote-release-20260519T013318Z-78823`。
- [x] release readiness 为 `production_release_ready=true`，
      `required_failures=[]`，`release_blockers=[]`。
- [x] saved validator 为 `status=ok`、`errors=[]`。
- [x] open P0 retrieval failures / governance tasks 均为 `0`。

## Fail-closed Matrix

| 场景 | 期望 |
| --- | --- |
| 缺失 `context_pack_ref` | workflow fail-closed |
| 缺失 `context_projection_ref` | workflow fail-closed |
| `context_pack_ref` 不属于当前 trace | workflow fail-closed |
| `context_projection_ref` 不属于当前 pack | workflow fail-closed |
| context pack schema version 不支持 | workflow fail-closed |
| context projection schema version 不支持 | workflow fail-closed |
| context pack digest 不匹配 | workflow fail-closed |
| context projection digest 不匹配 | workflow fail-closed |
| consumer 读取非本 consumer projection | workflow fail-closed |
| 未知 `consumer_type` / `consumer_name` / `runtime_adapter` | workflow fail-closed |
| `external_agent` consumer 在 Context Projection Runtime 被请求 | workflow fail-closed |
| tool policy digest 不匹配 | workflow fail-closed |
| output contract digest 不匹配 | workflow fail-closed |
| 用户伪造 context/projection/consumer/tool/reviewer 字段 | public request rejected |
| Hermes transcript 尝试替代业务 session | ignored and audited |
| replay 找不到历史 pack 或 projection | replay failed, 不重新推导 |

## 退出条件

- [x] consumer 间未授权上下文不可见。
- [x] Runtime profile 只能读取自己的 `context_projection`，不能读取完整
      `context_pack`。
- [x] Hermes transcript 不替代业务 session。
- [x] pack/projection schema 或 digest 不匹配时 workflow fail-closed。
- [x] 未知 consumer、未知 runtime adapter 和 `external_agent` 在 Context Projection Runtime
      fail-closed。
- [x] replay 能重建 context pack、context projection、Runtime step、package 和
      review 链。
- [x] 普通用户不能传入 context、scope、memory、consumer、tool、runtime adapter 或
      reviewer 控制字段。
- [x] public response、SSE、metrics 和普通日志不泄露 context/journal/memory 内部字段。
- [x] hhost full remote release automation 通过。

## 待确认项

无。

当前 Context Projection Runtime 已按既定决策收敛为 `context_pack` + `context_projection` + registered
consumer 模型。非 Hermes agent 只保留 schema 扩展点，不进入当前实现范围；如果未来
要接入，需要新增独立设计、实现、验收和 hhost gate，不能复用本 Context Projection Runtime 完成结论。

## 不可声明事项

Context Projection Runtime 完成后仍不能声明：

1. scoped memory production-ready；
2. Memory Collector 完成；
3. `memory_candidate` 队列可用；
4. active memory 可用；
5. memory 审核页面或远程审核工作流可用；
6. Context Governance 已拆成独立服务；
7. 非 Hermes external agent 已接入。

只有 Memory Candidate Workflow 和 Scoped Memory Production 退出条件全部通过，并重新完成 hhost live gate 后，才能进入
scoped memory production-ready 结论。
