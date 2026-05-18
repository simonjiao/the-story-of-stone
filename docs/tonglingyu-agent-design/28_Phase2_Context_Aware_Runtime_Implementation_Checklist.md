# 28 Phase 2 Context-aware Runtime 实现 Checklist

## 状态口径

目标：把 Phase 1 生成的 `context_pack` 真正接入 Runtime profile 执行链，
使 Runtime 每个 step 只读取自己被授权的上下文投影。

当前状态：Phase 2 设计已细化，可以进入实现准备；尚未开始编码，尚不能声明
Phase 2 完成或 scoped context 全量 production-ready。

Phase 2 只覆盖 Context-aware Runtime。它不实现长期 memory。

## 进入条件

- [x] Phase 1 Scoped Context 已 production-ready。
- [x] `hhost` 当次完整 remote release automation 已通过：
      `remote-release-20260518T181347Z-50849`。
- [x] `user_session`、`interaction_context`、`context_pack`、`session_journal`
      和 `resolved_question` 已进入生产路径。
- [x] public response 不暴露 context、journal 或 memory 内部字段。
- [x] active memory、Memory Collector、`memory_candidate` 和审核入口仍未实现。

## 非目标

以下不是待办项，而是 Phase 2 实现时必须持续满足的禁止边界：

1. 不实现 Memory Collector；
2. 不实现 `memory_candidate` 队列；
3. 不实现 active `memory_card`；
4. 不实现 memory 审核页面或公网审核入口；
5. 不把 Hermes transcript 当作业务 session；
6. 不让 context、session summary 或用户偏好进入 evidence package；
7. 不让 Runtime profile 读取完整 Open WebUI history 或 journal 原文；
8. 不让用户通过请求字段指定 `context_pack_ref`、profile、tool policy 或 reviewer
   状态。

## 核心确认

1. Runtime 上下文入口只允许 `context_pack_ref`。
   完整 context pack 由 Gateway / Context Governance 管理，Runtime step 只按 ref
   读取当前 profile 的受控投影。

2. `context_pack_ref` 必须绑定 trace、profile、schema version 和 digest。
   ref 与当前 trace 或 profile 不匹配时 fail-closed。

3. 每个 profile 只能读取自己的 context projection。
   `honglou-text` 和 `honglou-commentary` 不能看到完整 session summary；
   `honglou-reviewer` 不能看到 user_private memory、未审核 candidate 或 Hermes
   私有 transcript。

4. Context-aware Runtime 不改变事实来源。
   正式事实仍只来自 source snapshot、证据包、reviewer、外部快照、状态流转和
   action/audit log。

5. Reviewer 仍是本地 review enforcement 的最终裁决点。
   context pack 只能影响问题解析、检索约束和回答策略，不能覆盖 reviewer 裁决。

6. replay 必须使用当时保存的 context pack snapshot 或 digest 绑定记录。
   replay 不能从当前 journal 重新推导一个新的 context pack 后冒充历史执行。

7. hhost live gate 必须重新跑。
   Phase 1 的 production run 证明进入条件，不证明 Phase 2 已完成。

## Runtime Context Contract

新增 Runtime step input 字段：

1. `context_pack_ref`；
2. `context_pack_schema_version`；
3. `context_pack_digest`；
4. `interaction_context_id`；
5. `profile_name`；
6. `tool_policy_digest`；
7. `output_contract_digest`。

字段规则：

1. `context_pack_ref` 必填；
2. `profile_name` 必须等于当前 Runtime step profile；
3. `context_pack_schema_version` 必须被当前 runtime 支持；
4. `context_pack_digest` 必须匹配持久化 context pack；
5. `tool_policy_digest` 必须匹配 Runtime step plan 的 allowed tools；
6. `output_contract_digest` 必须匹配该 profile 的输出 contract；
7. 任一校验失败时 workflow fail-closed，并写 audit。

## Profile Projection

### `honglou-main`

可见：

1. `resolved_question`；
2. 必要 session summary；
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
3. 脂批 profile 私有推理；
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

### P2A Contract 与 schema

- [ ] 定义 Runtime context input 结构。
- [ ] 为 `context_pack_ref`、schema version、digest、profile 和 tool policy
      增加校验函数。
- [ ] 为 context projection 定义稳定 schema。
- [ ] 为不支持 schema version、digest mismatch、profile mismatch 和 tool policy
      mismatch 增加错误类别。

### P2B Gateway 到 Runtime 的传递链

- [ ] Gateway Runtime step plan 携带 `context_pack_ref`。
- [ ] Gateway 不把完整 context pack 塞进 public response、SSE 或普通日志。
- [ ] Runtime workflow input 从 `question` 单字段升级为 question +
      `context_pack_ref`。
- [ ] 去重 replay 请求必须复用原 trace 的 context pack ref，不重新解析用户历史。

### P2C Runtime profile projection enforcement

- [ ] Runtime 在进入每个 profile 前读取并校验对应 projection。
- [ ] profile step message 只包含该 profile projection。
- [ ] `honglou-text` / `honglou-commentary` 测试证明看不到完整 session summary。
- [ ] `honglou-reviewer` 测试证明看不到未审核 memory/candidate 和 Hermes transcript。

### P2D Tool policy binding

- [ ] Runtime allowed tools 必须来自 context pack projection 和 step plan 的交集。
- [ ] 任一 profile 请求未授权 tool 时 fail-closed。
- [ ] `tool_policy_digest` 与 step plan 不一致时 fail-closed。
- [ ] 负面测试覆盖用户伪造 `allowed_tools`、`forbidden_tools` 和
      `runtime_step_plan`。

### P2E Audit、admin trace 与 replay

- [ ] Runtime audit 记录 `context_pack_ref`、profile、tool policy digest、
      output ref 和 context schema version。
- [ ] admin trace 只展示 context 摘要、hash、ref 和校验状态。
- [ ] replay 可以重建 context、Runtime step、package 和 review 链。
- [ ] replay 不读取当前 journal 来替代历史 context pack。

### P2F Public surface 与隐私

- [ ] public chat response 不返回 context/journal/memory 内部 ID。
- [ ] SSE chunk 不泄露 context projection 或 journal 原文。
- [ ] metrics 不包含用户原文、高基数 context id 或 journal id。
- [ ] 普通用户不能提交 context、scope、memory、tool 或 reviewer 控制字段。

### P2G 本地验证

- [ ] `cargo fmt --all --check`。
- [ ] `cargo clippy -p tonglingyu-gateway --all-targets -- -D warnings`。
- [ ] `cargo test -p tonglingyu-gateway`。
- [ ] `cargo test -p tonglingyu-runtime`。
- [ ] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`。
- [ ] `deploy/scripts/verify-tonglingyu-scoped-context-live.sh` 增强或新增 Phase2
      gate。
- [ ] `scripts/qa.sh --quick`。

### P2H hhost production gate

- [ ] 按版本规则 bump deploy patch version。
- [ ] 同步 release tools 到 `hhost`。
- [ ] 重建并部署 `tonglingyu-gateway` 镜像。
- [ ] 运行 strict Gateway live gate。
- [ ] 运行 scoped context live gate，覆盖多轮追问和 profile 隔离。
- [ ] 运行 full remote release automation。
- [ ] release readiness 为 `production_release_ready=true`。
- [ ] saved validator 为 `status=ok`、`errors=[]`。
- [ ] open P0 retrieval failures / governance tasks 均为 `0`。

## Fail-closed Matrix

| 场景 | 期望 |
| --- | --- |
| 缺失 `context_pack_ref` | workflow fail-closed |
| `context_pack_ref` 不属于当前 trace | workflow fail-closed |
| context pack schema version 不支持 | workflow fail-closed |
| context pack digest 不匹配 | workflow fail-closed |
| profile 读取非本 profile projection | workflow fail-closed |
| tool policy digest 不匹配 | workflow fail-closed |
| 用户伪造 context/tool/reviewer 字段 | public request rejected |
| Hermes transcript 尝试替代业务 session | ignored and audited |
| replay 找不到历史 context pack | replay failed, 不重新推导 |

## 退出条件

- [ ] profile 间未授权上下文不可见。
- [ ] Hermes transcript 不替代业务 session。
- [ ] context pack schema 不匹配时 workflow fail-closed。
- [ ] replay 能重建 context、Runtime step、package 和 review 链。
- [ ] 普通用户不能传入 context、scope、memory、tool 或 reviewer 控制字段。
- [ ] public response、SSE、metrics 和普通日志不泄露 context/journal/memory 内部字段。
- [ ] hhost full remote release automation 通过。

## 不可声明事项

Phase 2 完成后仍不能声明：

1. scoped memory production-ready；
2. Memory Collector 完成；
3. `memory_candidate` 队列可用；
4. active memory 可用；
5. memory 审核页面或远程审核工作流可用；
6. Context Governance 已拆成独立服务。

只有 Phase 3 和 Phase 4 退出条件全部通过，并重新完成 hhost live gate 后，才能进入
scoped memory production-ready 结论。
