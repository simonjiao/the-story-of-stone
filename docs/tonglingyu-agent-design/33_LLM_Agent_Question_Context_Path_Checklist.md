# 33 LLM Agent Question Context Path Checklist

## 状态口径

目标：把 `normalized question`、`resolved_question` 和 context 构建前置链路升级为
真实 LLM Agent 参与的生产路径。

当前状态：design checklist generated, implementation not started。

当前不能声明完成。现有链路已经具备 `llm_resolver.rs`、`llm_provider.rs`、
`conversation_state.rs`、`llm_modes.rs`、`llm_eval.rs` 等基础模块，但主路径仍以
Gateway 内部确定性 resolver 和 ContextPackBuilder 为核心。下一阶段不能继续叠加 helper
补丁，必须把“LLM 辅助函数”重构为“受 Runtime / Agent Platform 管理的真实 Agent”。

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

重构完成后的结构必须清晰到可以回答三个问题：

1. 这个 Agent 是谁；
2. 它能看什么；
3. 它的输出如何被验收、审计和回放。

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
          -> authorized memory summary
          -> context_pack
          -> context_projection
      -> existing honglou-* Runtime profiles
      -> OpenAI-compatible response
```

Open WebUI 仍只看到 `tonglingyu` 一个模型。`tonglingyu-question-normalizer` 和
`tonglingyu-conversation-state-writer` 是内部 Runtime profile，不是用户可选模型。

## 真实 Agent 完成定义

以下条件全部满足，才算真实 Agent 支持：

- [ ] Agent 有稳定身份：`agent_type`、`agent_request_type`、`profile_id`、
      `consumer_name`、`runtime_adapter`、`trace_id`。
- [ ] Agent 请求使用 `AgentRequest` 或等价内部 request envelope 表达，不能只是函数参数。
- [ ] Agent 通过 Runtime profile 执行，不能由 Gateway 直接裸调 provider。
- [ ] Agent 输入由确定性 projection builder 生成，不能直接传完整
      `ChatCompletionRequest`。
- [ ] Agent 输出是 schema-bound JSON，不能返回自由文本后再解析猜测。
- [ ] Agent 输出必须经过 contract validator、denylist scanner、confidence gate 和
      mode gate。
- [ ] Agent 调用必须写 audit：request id、input digest、output digest、mode、decision、
      latency、provider/model 摘要、schema version、error type、replay anchor。
- [ ] Agent raw input/output 不能进入 public response、evidence package、普通日志或 metrics。
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
      `prior_subject`、deterministic `session_summary`、可选
      `authorized_memory_summary`、trigger、schema version。
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

- [ ] 新增 Runtime profile：`tonglingyu-question-normalizer`。
- [ ] 新增 Runtime profile：`tonglingyu-conversation-state-writer`。
- [ ] 两个 profile 均必须注册 `ProfileContract`。
- [ ] 两个 profile 均必须有 input schema、output schema、safety policy、
      max context budget、max runtime seconds。
- [ ] 两个 profile 默认 `allowed_tools=[]`。
- [ ] 如后续需要 authorized memory summary，只能作为脱敏 summary 字段进入 input，
      不能授予 memory tool。
- [ ] Runtime step message 只能包含该 Agent 的 projection payload，不能包含完整
      `context_pack`、完整 Open WebUI history 或 raw journal。
- [ ] Runtime metadata 必须绑定 `context_projection_ref`、projection digest、
      tool policy digest、output contract digest。
- [ ] 未知 profile、未知 consumer、未知 Runtime Adapter、digest mismatch 必须
      fail-closed。

## P2 Gateway 主路径重构

目标：Gateway 只做编排、验收和审计，不直接充当 Agent。

- [ ] 新增独立配置：
      `TONGLINGYU_LLM_RESOLVER_AGENT_MODE=disabled|shadow|enforced`。
- [ ] 新增独立配置：
      `TONGLINGYU_CONVERSATION_STATE_AGENT_MODE=disabled|shadow|enforced`。
- [ ] 新增统一 mode gate，默认 `disabled`，非法值启动失败或请求 fail-closed。
- [ ] `/v1/chat/completions` 主路径必须先执行 deterministic pre-resolver。
- [ ] 只有 allowed trigger 才创建 `question_normalization` Agent request。
- [ ] forbidden trigger 必须记录 audit，并证明 provider-not-called。
- [ ] `shadow` 模式只写 audit，不改变主路径 `resolved_question`。
- [ ] `enforced` 模式只有 accepted result 才能替换 deterministic resolver 输出。
- [ ] Agent result 未 accepted 时，只允许回退 deterministic safe result、要求澄清或
      fail-closed，不能编造补全。
- [ ] `confidence >= 0.75` 且 `needs_clarification=false` 才能 accepted。
- [ ] `0.45 <= confidence < 0.75` 必须走 clarification。
- [ ] `confidence < 0.45`、schema invalid、provider timeout、provider 5xx、
      forbidden field、unknown context ref 必须 fail-closed 或回退安全澄清。
- [ ] Conversation State Agent 只在 question normalization gate 之后运行。
- [ ] `conversation_state_summary` 不能作为 resolver `used_context_refs`。
- [ ] Gateway admin trace 展示 digest、decision 和 summary，不默认展示 raw Agent payload。

## P3 ContextPackBuilder 重构

目标：ContextPackBuilder 消费受控结果，但不把控制权交给 LLM。

- [ ] `context_pack.resolver.strategy` 必须区分：
      `deterministic_rules`、`deterministic_with_llm_shadow`、
      `llm_agent_enforced`、`llm_agent_rejected_fallback`。
- [ ] `context_pack.resolver` 必须记录 Agent request id、mode、trigger、decision、
      input digest、output digest 和 replay anchor。
- [ ] `active_scopes` 只能由固定 policy 生成。
- [ ] `candidate_scopes` 只能由 deterministic builder 根据 accepted
      `referent_bindings` 派生。
- [ ] `allowed_tools`、`forbidden_tools`、consumer、Runtime Adapter 只能由确定性
      policy 生成。
- [ ] authorized memory reads 必须二次 policy 校验，不能因 Agent 建议扩权。
- [ ] `context_projection` 必须按 consumer 分离；question normalizer 可见内容不能透传给
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
- [ ] 新增 low confidence / missing clarification question 负例。
- [ ] 新增 Conversation State hallucination、boundary loss、memory-as-evidence、
      internal ref leakage 负例。
- [ ] 新增 public response scanner，覆盖非流式、SSE、缓存命中。
- [ ] 新增 replay validator，证明 Agent decision、context pack、projection digest
      能按 trace 重放。
- [ ] fake provider contract tests 必须通过，但不能作为 production-ready 证据。

## P5 目标环境接入

目标：真实环境必须证明这些 Agent 被真实 Runtime 调用。

- [ ] 在 compose / env 中注册两个内部 Runtime profile。
- [ ] 配置 profile model mapping，例如：
      `AGENT_RUNTIME_HERMES_PROFILE_MODELS=tonglingyu-question-normalizer=hermes-agent,tonglingyu-conversation-state-writer=hermes-agent`。
- [ ] `TONGLINGYU_LLM_RESOLVER_AGENT_MODE` 先部署为 `shadow`。
- [ ] `TONGLINGYU_CONVERSATION_STATE_AGENT_MODE` 先部署为 `shadow`。
- [ ] shadow live gate 证明真实 provider 被调用，但主路径未被改变。
- [ ] enforced live gate 只先打开 question normalizer。
- [ ] conversation state writer enforced 必须晚于 question normalizer enforced。
- [ ] live gate 覆盖真实 Open WebUI 多轮追问。
- [ ] live gate 覆盖 provider timeout / 5xx / schema invalid。
- [ ] live gate 覆盖 forbidden trigger provider-not-called。
- [ ] live gate 覆盖 public response 无内部字段泄露。
- [ ] live gate 输出 artifact：case id、trace id、mode、Agent request id、decision、
      input/output digest、latency、error rate、rollback command、image id、commit。

## P6 Release Readiness

目标：release readiness 必须把真实 Agent 作为 required gate。

- [ ] 新增 LLM Agent release report。
- [ ] release report 必须包含 repo-local eval 结果。
- [ ] release report 必须包含真实 provider live gate 结果。
- [ ] release report 必须包含 shadow/enforced 分阶段证据。
- [ ] release report 必须包含 provider-not-called 负例证据。
- [ ] release report 必须确认无 raw prompt、raw response、raw memory、tool payload、
      ACL 或 secret。
- [ ] release readiness validator 必须消费 LLM Agent release report。
- [ ] saved validator 必须能按 trace 重放 Agent request、Agent decision、
      context pack 和 projection digest。
- [ ] `hhost` full remote release automation 必须通过。
- [ ] `PROGRESS.md` 必须写入版本、commit、image、artifact、case counts、剩余非目标。

## Production-ready 完成条件

全部满足前，不得声明 production-ready：

- [ ] P0-P6 全部完成。
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
