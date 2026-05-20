# 33 LLM Agent Question Context Path Checklist

## 状态口径

目标：把 `normalized question`、`resolved_question` 和 context 构建前置链路升级为
真实 LLM Agent 参与的生产路径。

当前状态：repo-local Runtime profile 接入与 validator 主路径已实现；gatekeeper
release report validator、release-readiness manifest validator 和 synthetic contract tests
已补齐并本地通过。目标环境真实 provider live gate、saved validator 和 full remote
automation 尚未执行，不能声明 target ready 或 production-ready。

已落地的 repo-local 事实：

- `tonglingyu-gateway/src/llm_agent_contracts.rs` 定义两个内部 Runtime profile、
  `LlmAgentRequestEnvelope`、bounded input、strict question output、profile contract
  和 read-only tool policy。
- `tonglingyu-gateway/src/llm_agent_validator.rs` 是统一业务 validator。Question
  normalizer accepted result 和 conversation state accepted result 都由私有字段 sealed
  type 表示，raw Runtime output 只能经 validator 转成 decision audit。
- `/v1/chat/completions` 主路径已切换到 agent-aware context path：deterministic
  pre-resolver -> Runtime profile -> validator -> deterministic ContextPackBuilder。
- Runtime 调用期间不持有 SQLite connection，避免真实 Axum 服务因 non-Send DB handle
  跨 `await` 无法挂载 handler。
- 定向测试覆盖 accepted rewrite、forbidden-field rejection fallback、conversation-state
  validated projection 和 validator 负例。
- forbidden-control 请求在进入 Runtime profile 前写入
  `llm_agent_provider_not_called` audit，记录两个 LLM Agent profile 均未调用和 raw output
  未嵌入。
- gatekeeper 新增 `verify-tonglingyu-llm-agent-live-gate.sh`，release readiness 和 remote
  live/release automation 已把 LLM Agent mode matrix live gate 纳入 required live gate。

仍不能声明完成：目标环境真实 provider live gate、saved report validator、full remote
automation、target artifact digest、image/rollback evidence 尚未全部闭合。

## 重构原则

以下原则是硬约束，不是建议：

- [ ] 不接受“先在 Gateway 里临时调用 LLM，后面再迁移”的方案。
- [ ] 不接受 fake provider 或 fixture 被描述为真实 Agent。
- [ ] 不接受让 LLM 直接生成 `context_pack`、`context_projection`、scope、ACL、
      tool policy 或 evidence package。
- [ ] 不接受通过新增一批 if/else 绕过 Runtime profile、AgentRequest、projection
      和 audit contract。
- [ ] 不接受 shadow/enforced 逻辑分散在多个模块里；必须有统一 mode gate。
- [ ] 不接受 public response 夹带 trace、context、memory、Agent、provider 或 raw
      LLM 输出字段。
- [ ] 不接受只跑本地测试就声明 production-ready；必须有目标环境真实 Agent live gate。
- [ ] 不接受把 schema、fixture、fake provider、shadow gate 或 repo-local gate 任一单项
      当作真实 Agent 接入完成。
- [ ] 不接受把实现拆成“这次只接 question normalizer，下次再接 conversation state”；
      两个内部 Agent 必须在同一实施链路里完成接入和验证。
- [ ] 不接受只靠文档约定或调用顺序保证 Agent 输出安全；必须用类型边界、私有构造器、
      validator API 和测试让 raw Agent output 无法绕过验收进入 ContextPackBuilder。

重构完成后的结构必须清晰到可以回答三个问题：

1. 这个 Agent 是谁；
2. 它能看什么；
3. 它的输出如何被验收、审计和回放。

## 一口气完成边界

这里的“一口气”指代码、契约、测试、artifact、文档和目标环境 gate 在同一实施链路里闭合。
部署启用可以按 `disabled -> shadow -> enforced` 做风险门控，但这只是同一 release run 内的
运行安全顺序，不是把交付拆成多个阶段。

- [ ] 本次实施必须同时完成 `tonglingyu-question-normalizer` 和
      `tonglingyu-conversation-state-writer` 两个 Runtime profile。
- [ ] 本次实施必须同时完成 Gateway 主路径接入、统一 mode gate、schema validator、
      denylist scanner、confidence gate、audit、replay anchor 和 admin digest view。
- [ ] 本次实施必须完成 Agent 输出控制闭环：raw Runtime output 只能进入 validator，
      ContextPackBuilder 的 API 只能接收 validator 产出的 sealed decision 类型。
- [ ] 本次实施必须同时完成 deterministic fallback、clarification、fail-closed、
      provider-not-called、public leakage scanner 和 saved replay validator。
- [ ] 本次实施必须让 repo-local tests、LLM eval、Gateway smoke、strict live gate、
      release readiness report 和 saved report validator 全部执行通过，并产出可复核
      artifact；只“接入脚本”不算完成。
- [ ] 如果 SSH、provider credential、target image 或目标环境权限不可用，状态只能写成
      `BLOCKED: target live gate unavailable`，不能写成 completed、done 或 production-ready。

完成动词必须按以下含义使用：

- `接入`：真实请求路径已执行对应 Runtime profile，并由测试证明 provider 被调用或按策略
  provider-not-called。
- `通过`：命令非 0 失败、0 成功，且 stdout/report artifact 被保存并记录 digest。
- `完成`：实现、测试、artifact、release report、saved validator 和 `PROGRESS.md` 状态全部闭合。
- `blocked`：缺目标权限、凭据、镜像或外部 provider 时的唯一允许状态；不得改名为 done。

## 待确认项

以下不是折中空间，而是执行前必须关闭的 blocker。任一项未确认时，可以继续做 repo-local
实现和 contract tests，但不得声明真实 Agent 接入完成、目标环境 ready 或 production-ready。

- [ ] 目标部署入口：确认真实目标环境使用的 deploy / gatekeeper 仓库、`<deployment>` 根目录、
      远端同步路径和最终入口命令；源码树当前只直接包含 `scripts/qa.sh`、
      `agent-platform/scripts/tonglingyu-gateway-smoke.sh` 等本地脚本，`<deployment>/scripts/...`
      必须在目标部署产物中复核。
- [ ] 真实 provider 能力：确认目标 provider / model 支持严格 JSON 输出、schema repair 后重试、
      1500ms 级别 timeout、并发两类内部 profile、错误分类和可观测 latency。
- [ ] profile model mapping：确认
      `AGENT_RUNTIME_HERMES_PROFILE_MODELS=tonglingyu-question-normalizer=...,tonglingyu-conversation-state-writer=...`
      在目标环境使用的真实模型名、base URL、API key、network route 和失败回滚值。
- [ ] AgentRequest 对齐方式：确认本次实现是直接复用 `agent_core::AgentRequest`，还是新增
      `LlmAgentRequestEnvelope` 并逐字段对齐；无论选择哪种，都必须有 serialization /
      migration / replay tests。
- [ ] sealed decision 测试方式：确认使用 Rust 可见性单元测试、compile-fail test
      或等价机制，证明 validator 模块外不能构造 accepted decision，且
      ContextPackBuilder 不能接收 raw `serde_json::Value`。
- [ ] authorized memory summary 策略：确认本次是否支持该字段。默认必须 absent；如果支持，
      必须同一轮完成 pre-resolver authorization、脱敏 digest、二次 policy tests 和泄露负例。
- [ ] fault injection 路径：确认 provider timeout、5xx、schema invalid、forbidden field、
      unknown context ref、schema repair failure 如何在 repo-local、Gateway smoke 和目标 live gate
      中稳定触发。
- [ ] release artifact schema：确认 LLM Agent release report、mode matrix、live gate report、
      saved validator report 的字段、digest 规则、case count 规则和 artifact registry 入口。
- [ ] rollback 命令：确认 `disabled` mode 回滚、profile model mapping 回滚、image 回滚、
      env 回滚和目标服务重启命令，并要求写入 live gate artifact。
- [ ] raw output 保存策略：确认是否允许 encrypted debug artifact；若没有加密存储、访问控制、
      retention 和清理策略，则 raw Agent output 只能保存 digest，不能保存原文。
- [ ] Open WebUI 真实用例：确认多轮追问、长 history、streaming、缓存命中、管理员 trace、
      普通用户 public response 的目标环境 case 列表和期望输出。

## 目标架构

```text
Open WebUI
  -> tonglingyu-gateway
      -> protocol / auth / rate limit / forbidden control guard
      -> deterministic pre-resolver
      -> AgentRequest: question_normalization
          -> Runtime profile: tonglingyu-question-normalizer
          -> schema validator / confidence gate / denylist scanner
      -> AgentRequest: conversation_state
          -> Runtime profile: tonglingyu-conversation-state-writer
          -> schema validator / boundary validator / leakage scanner
      -> deterministic ContextPackBuilder
          -> active_scopes
          -> candidate_scopes
          -> authorized memory summary, only after pre-resolver authorization
          -> context_pack
          -> context_projection
      -> existing honglou-* Runtime profiles
      -> OpenAI-compatible response
```

Open WebUI 仍只看到 `tonglingyu` 一个模型。`tonglingyu-question-normalizer` 和
`tonglingyu-conversation-state-writer` 是内部 Runtime profile，不是用户可选模型。

## Validator 实现落点

Validator 是 Gateway 侧业务输出验收层，不是 Runtime profile contract 本身，也不是
ContextPackBuilder 的内部逻辑。

实现必须按以下边界拆分：

- [x] `tonglingyu-gateway/src/llm_agent_contracts.rs` 定义
      `LlmAgentRequestEnvelope`、`QuestionNormalizationAgentInput`、
      `QuestionNormalizationAgentOutput`、`ConversationStateAgentInput`、
      `ConversationStateAgentOutput`、`AgentDecision`、digest / replay anchor 类型。
- [x] `tonglingyu-gateway/src/llm_agent_validator.rs` 实现确定性业务 validator：
      `validate_question_normalizer_output(...)`、
      `validate_conversation_state_output(...)`，输出只能是
      `accepted`、`rejected`、`clarify`、`fail_closed`、`shadow_only`。
- [x] `tonglingyu-gateway/src/context_governance.rs` 只负责编排：deterministic
      pre-resolver、projection builder、Runtime profile 调用、validator 调用、
      deterministic ContextPackBuilder 调用和 audit 写入。
- [x] `ContextPackBuilder` 只能接收 validator accepted 后的
      `AgentDecision` / deterministic fallback result；不得解析 raw Agent output。
- [x] `agent-runtime` / `tonglingyu-runtime` 只负责 Runtime profile contract、profile
      注册、adapter 执行、tool policy、timeout 和模型映射；不得替代 Gateway 业务
      validator 决定 `resolved_question` 是否进入 context pack。
- [ ] 现有 `llm_resolver.rs::evaluate_resolver_contract(...)` 和
      `conversation_state.rs::validate_conversation_state_summary(...)` 只能作为雏形；
      本次重构必须收敛到统一 Agent output validator 层，不能继续分散调用。

## Agent 输出控制闭环

目标：Agent 输出必须被真实控制。这里的“控制”不是日志里记录 rejected，而是 raw output
没有任何路径能绕过 validator 改写 `resolved_question`、conversation state、scope、tool、
memory、evidence 或 context projection。

- [x] Runtime 返回的 raw output 只能以 `RawLlmAgentOutput` / `RuntimeOutput` 形式进入
      `llm_agent_validator.rs`；除 validator 和 audit digest 代码外，禁止其他模块读取
      raw output body。
- [x] `QuestionNormalizationAgentOutput` 和 `ConversationStateAgentOutput` 必须使用严格
      schema 反序列化；未知字段、重复控制语义、类型不匹配、空字符串、越界数组必须 rejected。
- [x] Rust 输出结构必须启用等价于 `deny_unknown_fields` 的约束；若需要手写 parser，
      必须显式枚举 allowed keys 并拒绝 extras。
- [x] Validator 只能产出 sealed 类型，例如 `ValidatedAgentDecision`、
      `ValidatedQuestionResolution`、`ValidatedConversationState`；这些类型的构造器必须
      对 validator 模块外不可见。
- [x] `ContextPackBuilder` / `create_context_for_request` 的主路径函数签名不得接收
      `serde_json::Value`、raw provider response、raw `QuestionNormalizationAgentOutput`
      或 raw `ConversationStateAgentOutput`。
- [x] `resolved_question` 只能来自 deterministic result 或
      `ValidatedQuestionResolution::accepted`；`clarify`、`rejected`、`fail_closed`、
      `shadow_only` 不能生成 candidate scope。
- [x] `referent_bindings` 必须由 validator 绑定到 deterministic candidate set 或已授权
      projection refs；未知人物、未知 context ref、memory id、package id、trace id 必须 rejected。
- [x] `used_context_refs` 必须是枚举值，不得是任意字符串；conversation state summary
      不能作为 question resolver 的 `used_context_refs`。
- [x] `confidence` 只能参与 decision gate，不能参与 scope、tool、memory 或 evidence policy
      放权。
- [x] schema repair 只能发生在 validator 前的 parser repair 阶段；repair 后仍必须重新走
      完整 validator，repair attempt 必须写 audit，不能把 repair success 当 accepted。
- [x] raw Agent output 只能进入内部 audit digest / encrypted debug artifact；普通日志、
      metrics、public response、evidence package、context pack、context projection 均不得保存原文。

## 真实 Agent 完成定义

以下条件全部满足，才算真实 Agent 支持：

- [x] Agent 有稳定身份：`agent_type`、`agent_request_type`、`profile_id`、
      `consumer_name`、`runtime_adapter`、`trace_id`。
- [x] 必须定义并贯穿 `LlmAgentRequestEnvelope`。该 envelope 必须与
      `agent_core::AgentRequest` 字段语义对齐，并在 audit、replay、admin trace digest
      中作为一等对象出现；禁止匿名 JSON、临时函数参数或 provider payload 充当
      Agent request。
- [x] Agent 通过 Runtime profile 执行，不能由 Gateway 直接裸调 provider。
- [x] Agent 输入由确定性 projection builder 生成，不能直接传完整
      `ChatCompletionRequest`。
- [x] Agent 输出是 schema-bound JSON，不能返回自由文本后再解析猜测。
- [x] Agent 输出必须经过 contract validator、denylist scanner、confidence gate 和
      mode gate。
- [x] Agent 调用必须写 audit：request id、input digest、output digest、mode、decision、
      latency、provider/model 摘要、schema version、error type、replay anchor。
- [x] Agent raw input/output 不能进入 public response、evidence package、普通日志或 metrics。
- [x] Agent accepted result 必须是 validator 产出的 sealed decision，不得直接使用
      provider response、Runtime output 或 schema parser output。
- [ ] fake provider 只能用于 contract tests；目标环境必须证明真实 provider / runtime
      agent 被调用。

## 非目标和禁止边界

- [ ] 不把 LLM Agent 当事实源。
- [ ] 不让 LLM Agent 决定 reviewer 裁决。
- [ ] 不让 LLM Agent 打开 memory 读取面。
- [ ] 不让 LLM Agent 决定 ACL、scope grant、tool policy 或 Runtime Adapter。
- [ ] 不让 LLM Agent 写 evidence package。
- [ ] 不把 `session_summary`、conversation state、memory summary 或用户偏好当 evidence。
- [ ] 不支持任意外部 Agent。非登记 Runtime profile、未知 consumer、未知 adapter
      必须 fail-closed。

## P0 拆旧路径和 Contract 冻结

目标：先把现有 helper 化能力拆清楚，再接真实 Agent。不能在旧路径上继续加分支。

- [ ] 梳理当前 `resolve_question(...)` 的 deterministic 输出，拆成：
      `NormalizedQuestionSeed`、`ResolverTrigger`、`ResolverFallbackDecision`。
- [ ] `ResolverTrigger` 固定为 allowed / forbidden 两组枚举。
- [ ] allowed trigger 只包括：
      `unresolved_referent`、`elliptical_followup`、`multi_candidate_entity`、
      `prior_subject_needed`、`low_confidence_binding`。
- [ ] forbidden trigger 只包括：
      `prompt_injection_detected`、`forbidden_control_field_detected`、
      `unsupported_domain`、`context_budget_exceeded`、`memory_policy_denied`、
      `schema_or_model_not_allowed`。
- [ ] 删除或隔离任何会让 LLM 直接影响 scope/tool/context object 的隐式路径。
- [ ] 定义 `QuestionNormalizationAgentInput`，只允许：
      `current_question`、bounded recent user messages、bounded recent assistant messages、
      `prior_subject`、deterministic `session_summary`、trigger、schema version。
- [ ] `authorized_memory_summary` 不属于默认字段。若本次实现需要该字段，必须同一轮完成
      pre-resolver authorization、脱敏 digest、二次 policy tests 和泄露负例；否则该字段
      必须在 schema、fixture、runtime payload 中全部 absent。
- [ ] 定义 `QuestionNormalizationAgentOutput`，只允许：
      `resolved_question`、`referent_bindings`、`used_context_refs`、`confidence`、
      `needs_clarification`、`clarification_question`、`unsupported_reason`、schema version。
- [ ] 定义 `ConversationStateAgentInput`，只允许：
      current question、bounded recent messages、deterministic session summary、上一轮公开
      answer boundary、authorized package refs 摘要。
- [ ] 定义 `ConversationStateAgentOutput`，沿用
      `tonglingyu.conversation_state_summary`，禁止新增自由字段。
- [ ] 定义统一 `LlmAgentRequestEnvelope`，至少包含：
      `request_id`、`agent_type`、`agent_request_type`、`profile_id`、`mode`、
      `trace_id`、`user_session_id`、`interaction_context_id`、`input_digest`、
      `projection_ref`、`schema_version`、`timeout_ms`。

## P1 Agent Runtime Profile 重构

目标：把“LLM 辅助能力”变成 Runtime profile，而不是 Gateway helper。

- [x] 新增 Runtime profile：`tonglingyu-question-normalizer`。
- [x] 新增 Runtime profile：`tonglingyu-conversation-state-writer`。
- [x] 两个 profile 均必须注册 `ProfileContract`。
- [x] 两个 profile 均必须有 input schema、output schema、safety policy、
      max context budget、max runtime seconds。
- [x] 两个 profile 默认 `allowed_tools=[]`。
- [x] `authorized_memory_summary` 只能在 pre-resolver authorization 已落地且测试证明无扩权时
      作为脱敏 summary 字段进入 input；否则不得出现在 schema、fixture 或 runtime payload 中。
- [x] Runtime step message 只能包含该 Agent 的 projection payload，不能包含完整
      `context_pack`、完整 Open WebUI history 或 raw journal。
- [x] Runtime metadata 必须绑定 `context_projection_ref`、projection digest、
      tool policy digest、output contract digest。
- [ ] 未知 profile、未知 consumer、未知 Runtime Adapter、digest mismatch 必须
      fail-closed。

## P2 Gateway 主路径重构

目标：Gateway 只做编排、验收和审计，不直接充当 Agent。

- [x] 新增独立配置：
      `TONGLINGYU_LLM_RESOLVER_AGENT_MODE=disabled|shadow|enforced`。
- [x] 新增独立配置：
      `TONGLINGYU_CONVERSATION_STATE_AGENT_MODE=disabled|shadow|enforced`。
- [x] 新增统一 mode gate，默认 `disabled`，非法值启动失败或请求 fail-closed。
- [x] `/v1/chat/completions` 主路径必须先执行 deterministic pre-resolver。
- [x] 只有 allowed trigger 才创建 `question_normalization` Agent request。
- [x] forbidden trigger 必须记录 audit，并证明 provider-not-called。
- [x] `shadow` 模式只写 audit，不改变主路径 `resolved_question`。
- [x] `enforced` 模式只有 accepted result 才能替换 deterministic resolver 输出。
- [x] Agent result 未 accepted 时，只允许回退 deterministic safe result、要求澄清或
      fail-closed，不能编造补全。
- [x] `confidence >= 0.75` 且 `needs_clarification=false` 才能 accepted。
- [x] `0.45 <= confidence < 0.75` 必须走 clarification。
- [x] `confidence < 0.45`、schema invalid、provider timeout、provider 5xx、
      forbidden field、unknown context ref 必须 fail-closed 或回退安全澄清。
- [x] Conversation State Agent 只在 question normalization gate 之后运行。
- [x] `conversation_state_summary` 不能作为 resolver `used_context_refs`。
- [x] Gateway admin trace 展示 digest、decision 和 summary，不默认展示 raw Agent payload。

## P3 ContextPackBuilder 重构

目标：ContextPackBuilder 消费受控结果，但不把控制权交给 LLM。

- [x] `context_pack.resolver.strategy` 必须区分：
      `deterministic_rules`、`deterministic_with_llm_shadow`、
      `llm_agent_enforced`、`llm_agent_rejected_fallback`。
- [x] `context_pack.resolver` 必须记录 Agent request id、mode、trigger、decision、
      input digest、output digest 和 replay anchor。
- [x] `context_pack.resolver` 只能记录 validated decision summary 和 digest，不能保存 raw
      Agent output 或 repair transcript。
- [x] `active_scopes` 只能由固定 policy 生成。
- [x] `candidate_scopes` 只能由 deterministic builder 根据 accepted
      `referent_bindings` 派生。
- [x] `allowed_tools`、`forbidden_tools`、consumer、Runtime Adapter 只能由确定性
      policy 生成。
- [x] authorized memory reads 必须二次 policy 校验，不能因 Agent 建议扩权。
- [x] `context_projection` 必须按 consumer 分离；question normalizer 可见内容不能透传给
      `honglou-text`、`honglou-commentary` 或 `honglou-reviewer`。
- [ ] context pack / projection digest 必须覆盖 Agent decision digest，保证 replay
      不会从当前状态重新推导历史 Agent 输出。

## P4 Eval 和 Contract Tests

目标：测试必须证明真实边界，而不是只证明 happy path。

- [ ] `question_resolution.jsonl` 覆盖全部 allowed trigger。
- [ ] `question_resolution.jsonl` 覆盖全部 forbidden trigger。
- [ ] 每个 allowed trigger 至少覆盖 pass、clarify、fail-closed。
- [ ] 每个 forbidden trigger 至少覆盖 rejected 和 provider-not-called。
- [ ] 新增 Agent request envelope fixtures。
- [ ] 新增 Agent output schema invalid 负例。
- [ ] 新增 unknown field / forbidden field 负例。
- [ ] 新增 unknown context ref 负例。
- [ ] 新增 raw memory / memory card id / ACL / tool policy 夹带负例。
- [ ] 新增 raw output bypass 负例：直接把 raw provider response、raw runtime output、
      raw `serde_json::Value` 或 raw parser output 传入 ContextPackBuilder 必须无法编译或测试失败。
- [ ] 新增 sealed decision 负例：validator 模块外无法构造 accepted decision。
- [ ] 新增 schema repair 负例：repair 后仍含 forbidden field、unknown ref 或低 confidence
      时必须 rejected，不能 accepted。
- [ ] 新增 low confidence / missing clarification question 负例。
- [ ] 新增 Conversation State hallucination、boundary loss、memory-as-evidence、
      internal ref leakage 负例。
- [ ] 新增 public response scanner，覆盖非流式、SSE、缓存命中。
- [ ] 新增 replay validator，证明 Agent decision、context pack、projection digest
      能按 trace 重放。
- [ ] 所有 fixtures 必须被 runner 枚举并计入 report；孤立 fixture、未挂载 fixture、
      snapshot-only fixture 不算测试。
- [ ] runner 必须在 hard gate failure、case count 不足、fixture 未覆盖 required trigger、
      scanner 未运行或 replay validator 缺失时非 0 退出。
- [ ] 测试必须包含真实 provider smoke 和 fake provider contract 两类证据；二者互不替代。
- [ ] fake provider contract tests 必须通过，但不能作为 production-ready 证据。

## P5 目标环境接入

目标：真实环境必须证明这些 Agent 被真实 Runtime 调用。shadow/enforced 是同一 release
run 内的运行门控，不是分批实现理由。

- [x] 在 compose / env 中注册两个内部 Runtime profile 的 mode 开关。
- [ ] 配置 profile model mapping，例如：
      `AGENT_RUNTIME_HERMES_PROFILE_MODELS=tonglingyu-question-normalizer=hermes-agent,tonglingyu-conversation-state-writer=hermes-agent`。
- [x] gatekeeper live gate 脚本定义同一目标环境 release run 必须覆盖 baseline disabled、
      two-agent shadow、question normalizer enforced、two-agent enforced 四组 gate。
- [ ] 同一目标环境 release run 必须实际覆盖 baseline disabled、two-agent shadow、
      question normalizer enforced、two-agent enforced 四组 gate。
- [ ] shadow live gate 必须证明两个 Agent 都调用真实 provider / runtime agent，且主路径未被改变。
- [ ] question normalizer enforced live gate 必须证明 accepted result 能替换
      deterministic resolver 输出，rejected result 会安全回退。
- [ ] two-agent enforced live gate 必须证明 conversation state writer 在 accepted
      question normalization 之后运行，且不会反向影响 resolver used context refs。
- [ ] live gate 覆盖真实 Open WebUI 多轮追问。
- [ ] live gate 覆盖 provider timeout / 5xx / schema invalid。
- [ ] live gate 覆盖 forbidden trigger provider-not-called。
- [ ] live gate 覆盖 public response 无内部字段泄露。
- [ ] live gate 输出 artifact：case id、trace id、mode、Agent request id、decision、
      input/output digest、latency、error rate、rollback command、image id、commit。

## P6 Release Readiness

目标：release readiness 必须把真实 Agent 作为 required gate。

- [x] 新增 LLM Agent release report。
- [x] release report 必须包含 repo-local eval 结果。
- [ ] release report 必须包含真实 provider live gate 结果。
- [x] release report 必须包含 disabled、two-agent shadow、question normalizer enforced、
      two-agent enforced 的 required mode matrix contract。
- [ ] target artifact 必须包含 disabled、two-agent shadow、question normalizer enforced、
      two-agent enforced 的实际 mode matrix 证据。
- [ ] release report 必须包含 provider-not-called 负例证据。
- [x] release report 必须确认无 raw prompt、raw response、raw memory、tool payload、
      ACL 或 secret。
- [x] release readiness validator 必须消费 LLM Agent release report。
- [x] release readiness validator 必须把 LLM Agent live gate 作为 required live gate。
- [ ] saved validator 必须能按 trace 重放 Agent request、Agent decision、
      context pack 和 projection digest。
- [ ] `hhost` full remote release automation 必须通过。
- [ ] `PROGRESS.md` 必须写入版本、commit、image、artifact、case counts、validator
      status、失败边界和剩余非目标。

## P7 一口气实施工作包

目标：把真实 Agent 接入拆成可执行工作包，但这些工作包必须在同一轮实现、验证和提交。

- [ ] W1 Contract：新增并贯穿 `LlmAgentRequestEnvelope`、agent input/output schema、
      sealed output decision enum、audit event、replay anchor 和 migration/serialization tests。
- [ ] W2 Runtime：注册两个 Runtime profile，绑定 profile contract、adapter contract、
      timeout、tool policy、model mapping 和 provider error taxonomy。
- [ ] W3 Validator：新增统一 Gateway 业务 validator，覆盖 question normalization 和
      conversation state 输出，并复用/迁移现有 `llm_resolver` 与 `conversation_state`
      合同校验；validator 必须是唯一能构造 accepted decision 的模块。
- [ ] W4 Gateway：替换 Gateway helper 调用，接入 deterministic pre-resolver、统一 mode gate、
      accepted/rejected decision、clarification、fail-closed 和 provider-not-called audit。
- [ ] W5 Context：重构 ContextPackBuilder，只消费 accepted deterministic/Agent result，
      并把 Agent decision digest 纳入 context pack / projection replay digest；删除或封闭
      任何接收 raw Agent JSON / raw provider response 的构造入口。
- [ ] W6 Eval：补齐 allowed/forbidden trigger fixture、schema invalid 负例、leakage 负例、
      fake-provider contract tests、真实 provider smoke、public scanner 和 replay validator。
- [ ] W7 Target：更新 compose/env/release automation，执行并通过真实 provider live gate、
      release readiness validator、saved validator，并保存 rollback command。
- [ ] W8 Docs：更新 `PROGRESS.md`、release report schema、操作手册和剩余非目标；不得只写
      “已接入”而缺 artifact path、commit、image、case count 和失败边界。

## P8 验证命令和 Artifact

目标：验收必须能被别人复跑；如果命令或脚本尚不存在，本次实现必须补齐，不能删除 gate。

- [ ] 禁止用 placeholder script、手工 curl 截图、日志肉眼检查或 README 说明替代 gate。
- [ ] 每个 gate 必须写入 artifact path、sha256 digest、start/end time、commit、image
      或 binary digest；缺任一项只能算未完成。
- [ ] release readiness validator 必须消费这些 artifact；只把 artifact 放到目录里不算完成。
- [x] `git diff --check`
- [x] `scripts/qa.sh --full`
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-core`
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p agent-runtime`
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
- [x] `cargo clippy --manifest-path agent-platform/Cargo.toml --workspace --all-targets -- -D warnings`
- [x] `cargo run --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway -- llm-eval --fixture-dir agent-platform/crates/tonglingyu-gateway/evals/fixtures --report-out agent-platform/crates/tonglingyu-gateway/evals/reports/llm-eval.json --fail-on-hard-gate`
- [x] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`
- [x] gatekeeper `deploy/scripts/verify-tonglingyu-llm-release-report.sh <llm-release-report.json>`
- [x] gatekeeper `deploy/scripts/verify-tonglingyu-llm-agent-live-gate.sh` 已接入 release
      readiness / remote live gates / remote release automation；目标环境实际执行仍未完成。
- [x] gatekeeper `scripts/qa.sh --quick`
- [x] gatekeeper `deploy/scripts/test-tonglingyu-release-readiness-contract.sh`
- [ ] target `<deployment>/scripts/verify-tonglingyu-strict-gateway.sh`
- [ ] target full remote release automation，且 saved report validator 返回 `status=ok`。

## 反提前宣布胜利检查

以下任一情况存在时，只能声明 blocked 或 incomplete，不得声明真实 Agent 接入完成：

- [ ] 只完成 schema / envelope，主路径没有调用 Runtime profile。
- [ ] 只完成 question normalizer，conversation state writer 未接入或未验证。
- [ ] 只完成 shadow，enforced accepted/rejected/fail-closed gate 未通过。
- [ ] 只跑 fake provider，目标环境真实 provider / runtime agent 未被调用。
- [ ] 只跑 repo-local tests，缺 strict live gate、release readiness 或 saved validator artifact。
- [ ] 只创建验证脚本或 release report schema，但没有真实运行结果和 validator 消费记录。
- [ ] artifact 缺 digest、commit、image、case count、mode matrix 或 rollback command。
- [ ] ContextPackBuilder 仍能从 raw Agent output、raw parser output 或 `serde_json::Value`
      构建 context。
- [ ] validator 模块外仍能伪造 accepted Agent decision。
- [ ] schema repair 成功被直接当成 accepted，而没有重新通过 validator。
- [ ] public response scanner 没覆盖非流式、SSE、缓存命中。
- [ ] replay validator 不能按 trace 重放 Agent decision、context pack 和 projection digest。
- [ ] 目标环境不可访问但文档写成 completed。

## Production-ready 完成条件

全部满足前，不得声明 production-ready：

- [ ] P0-P8 全部完成。
- [ ] repo-local Rust tests 通过。
- [ ] clippy 通过。
- [ ] llm-eval 通过。
- [ ] strict Gateway gate 通过。
- [ ] fake provider contract tests 通过。
- [ ] 真实 provider / Runtime Agent live gate 通过。
- [ ] release readiness validator 通过。
- [ ] saved validator 通过。
- [ ] full remote release automation 通过。
- [ ] 普通用户响应无 Agent/context/memory/provider 内部字段。
- [ ] 管理员 trace 可审计、可 replay、可定位失败。

## 完成后仍不能声明

- 不能声明 LLM Agent 是事实来源。
- 不能声明 LLM Agent 可以决定 reviewer 裁决。
- 不能声明 LLM Agent 可以打开 memory 读取面。
- 不能声明支持任意外部 Agent。
- 不能声明用户可以指定 Agent、profile、context projection、scope 或 tool policy。
