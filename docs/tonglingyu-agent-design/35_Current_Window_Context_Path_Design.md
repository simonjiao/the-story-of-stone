# 35 Current Window Context Path Design

## 文档状态

本文冻结 `current_window` 作为 Gateway context path 请求级事实源的设计口径。
它不是 question normalizer 私有输入，也不是代码实现记录，而是后续重构
`resolved_question`、subject candidate、ellipsis resolution、conversation state、
session journal、retrieval projection、answer composer 和 reviewer 边界时的 contract 文档。

当前目标：让多轮省略问句先在受控上下文窗口中被解析为可检索的 `resolved_question`。
当窗口超出预算时，必须先经过 LLM current-window compression 生成并验收
`current_window_digest`，再按 agent 职责生成受控的 `current_window_projection`，进入
retrieval、evidence package、answer composition 和 review。不能再让未补全的
`脂批中的证据呢` 直接进入检索，也不能让旧 journal subject 覆盖当前请求窗口，
或让任一 profile 自行从 raw history 补救意图。

## 核心术语

`current_question`：当前用户这一句原文。例如 `脂批中的证据呢`。

`current_window`：当前请求 payload 携带的最近对话窗口。它来自本次 Open WebUI 到 Gateway
的 messages，不等同于数据库里的完整 session history，也不等同于长期 memory。

`session_journal_candidate`：同一 `user_session_id` 下已写入 `session_journal` 的历史消息中抽取出的候选。
它不是“上一个 conversation”，也不是当前窗口事实。

`memory_candidate`：经 scoped memory policy 授权读取的长期候选。它只能作为低权重上下文候选，
不能作为 evidence，也不能默认覆盖当前窗口。

`conversation_state_summary`：conversation-state writer 产出的结构化状态。它可辅助发现
normalizer 漏判，但不能替代 `resolved_question`，也不能反向直接驱动检索。

`current_window_projection`：从 `current_window` 派生、按 consumer 裁剪后的上下文。
它是请求级事实源的受控投影，不是长期 memory，也不是证据来源。

`current_window_digest`：当 `current_window` 超过预算时，由
`tonglingyu-current-window-compressor` 产出并通过 digest validator 验收的结构化压缩结果。
它只能保留指代解析、意图承接和回答边界所需的信息，不能新增人物、证据、出处或结论。

## 问题回放

历史 trace `tly-019e5520a5667c129df175b69c04ebf0` 的关键事实：

```text
current_question = 脂批中的证据呢
resolved_question = 脂批中的证据呢
session_summary = 最近讨论对象：林黛玉；最近用户问题：史湘云的结局 / 脂批中的证据呢
conversation_state_summary.open_questions = 脂批中关于史湘云结局的直接文本
```

失败点不是 conversation-state writer 滞后。它在当前请求内已经看到 `史湘云的结局`
和 `脂批中的证据呢`，并推断出更准确的 open question。真正的问题是：

1. deterministic resolver 把省略问句误判为完整问题；
2. question normalizer 没被触发；
3. `known_subjects` 缺 `史湘云`，导致当前窗口主体识别失败；
4. `session_summary` 过早混入 journal subject，把 `林黛玉` 写成“最近讨论对象”；
5. text/commentary 检索 profile 看不到 conversation state，只拿到未补全问题；
6. package/reviewer 只验证证据包内部一致性，没有验证证据是否回答省略后的真实意图。

## 设计原则

`resolved_question` 是检索入口的唯一权威问题。Retrieval、package、draft 和 reviewer
必须消费已验收的 `resolved_question`，不能消费未补全的省略问句。

`current_window` 优先于 `session_journal_candidate` 和 `memory_candidate`。当前窗口能确定
候选时，历史候选不能覆盖当前窗口。

`current_window` 可以被多个内部 agent 使用，但必须按职责投影。不能把完整 raw chat
window 无差别塞给所有 profile，也不能让 downstream agent 在自己的 prompt 里重新定义
指代解析规则。

上下文压缩必须是 context path 的显式步骤。目标路径启用 LLM compression，但它必须是
独立 profile、独立 schema、独立 validator 和独立 audit 事件。不能依赖某个 downstream
agent 在自己的 prompt 里临时“总结一下上文”，否则压缩结果不可审计，也无法判断它是否
引入了新的主体、证据或结论。

`session_summary` 可以拼接，但不能把历史候选伪装成当前事实。来源必须分层记录：

```text
当前窗口用户问题
当前窗口候选主体
session_journal_candidate
memory_candidate
```

`open_questions` 不是权威改写输出。它只能作为辅助信号、审计信号或 normalizer 漏判检测信号。
如果需要进入检索，必须先由 question normalizer 产出并通过 validator。

normalizer 的语义不确定不是系统错误。候选冲突或指代不清时，应返回普通 assistant 澄清，
不能返回 500 或 governance failed。

`referent_bindings` 可以为空。问题本身完整、无指代、或需要澄清时，空 bindings 是合法状态。
如果 bindings 非空，必须可追溯到候选池或当前问题文本直接出现的 subject。

规则必须外部化。Subject、别名、候选来源优先级、置信度衰减、省略问句类型和澄清模板
不得继续写成 scattered hardcode。

## 候选来源模型

后续 `allowed_referents` 应降级为结构化候选池，例如：

```json
{
  "referent_candidates": [
    {
      "text": "史湘云",
      "canonical": "史湘云",
      "source": "current_window.previous_user_message",
      "confidence": 0.95,
      "rule_id": "subject.current_window.previous_user_message"
    },
    {
      "text": "林黛玉",
      "canonical": "林黛玉",
      "source": "session_journal_candidate",
      "confidence": 0.35,
      "rule_id": "subject.session_journal.low_weight"
    }
  ]
}
```

候选来源优先级：

1. `current_question`：当前问题直接出现的人物、物件、版本、概念；
2. `current_window`：本次请求窗口中最近的用户/助手消息；
3. `conversation_state_summary`：validated state 中的 open question / active entities；
4. `session_journal_candidate`：同一 user session 的历史 journal 候选；
5. `memory_candidate`：已授权读取的长期 memory 候选。

默认权重方向：

```text
current_question > current_window > conversation_state_summary > session_journal_candidate > memory_candidate
```

`session_journal_candidate` 只有在用户显式引用历史时升权，例如：

```text
之前
上次
第几条
刚才那段
前面那个证据包
```

如果当前窗口高置信候选存在，而 normalizer 选择低权重历史候选，validator 必须拒绝，
除非输出明确记录了用户的历史引用触发词和规则 id。

## Subject Ontology

新的 `subject ontology` 解决“用户在说谁/什么”。它服务 resolver、normalizer 和
conversation state，不直接决定证据成立。

它不同于 query expansion、evidence slot、source scope、answer/review rules：

```text
subject ontology: 用户说的是谁/什么，有哪些别名，能否作为 referent candidate
query expansion: 用哪些词把材料找出来
evidence slot rules: 材料支持什么语义
answer/review rules: 答案怎么说，什么不能说
```

示例：

```json
{
  "schema_version": "tonglingyu.subject_ontology.v1",
  "subjects": [
    {
      "canonical": "史湘云",
      "aliases": ["湘云", "云妹妹", "史大姑娘"],
      "type": "character",
      "work": "hongloumeng"
    }
  ]
}
```

Subject ontology 必须允许热加载和版本审计。命中结果需要记录 `catalog_version`、
`rule_id`、`canonical` 和 `source`。

## Ellipsis Resolution

省略问句必须作为一类独立规则处理，不能只靠 pronoun 规则。候选例子：

```text
证据呢
脂批中的证据呢
出处呢
原文呢
这个呢
继续这个
```

处理流程：

1. 检测 current_question 是否为省略问句；
2. 从 current_window 提取最近可承接的问题；
3. 从 subject ontology 提取 referent candidates；
4. 生成 candidate resolved question；
5. 交给 normalizer 输出 strict JSON；
6. validator 验证绑定来源、置信度、澄清要求和规则 id；
7. 通过后写入 `context_pack.resolved_question`；
8. retrieval/package/draft/review 只使用通过后的 `resolved_question`。

对历史 trace 的期望输出：

```json
{
  "resolved_question": "关于史湘云的结局，脂批中有哪些证据？",
  "referent_bindings": ["史湘云"],
  "used_context_refs": ["current_window.previous_user_message"],
  "confidence": 0.9,
  "needs_clarification": false
}
```

如果候选冲突或没有足够上下文，期望输出是澄清：

```json
{
  "resolved_question": "脂批中的证据呢",
  "referent_bindings": [],
  "used_context_refs": [],
  "confidence": 0.4,
  "needs_clarification": true,
  "clarification_question": "你问“脂批中的证据呢”，是继续问上一条“史湘云的结局”的脂批证据吗？",
  "unsupported_reason": "ambiguous_ellipsis"
}
```

澄清应进入普通 assistant response，不得作为 runtime workflow failed。

## Session Summary 边界

`session_summary` 仍可保留拼接形态，但必须避免确定性误导。建议输出结构化来源或至少改写文本：

```text
当前窗口用户问题：史湘云的结局 / 脂批中的证据呢
当前窗口候选主体：史湘云
session_journal_candidate：林黛玉
```

禁止把低权重历史候选写成：

```text
最近讨论对象：林黛玉
```

当 current_window 无法确定 subject 时，才允许读取 `session_journal_candidate`；
当用户明确引用历史时，才允许给 journal candidate 升权。

## Current Window 构造与 LLM 压缩

`current_window` 每次请求现场构造，但必须有明确的预算、压缩 profile 和验收策略。
目标路径启用 LLM compression；确定性步骤只负责清洗、预算切片、source ref 和 hard cap，
不负责语义摘要。

推荐顺序：

1. 从 Open WebUI request payload 抽取 `current_question` 和最近 user/assistant 消息；
2. 删除 system prompt、raw provider output、internal draft、review、audit 和 memory 原文；
3. 按 `max_raw_messages`、`max_raw_chars`、`max_compressor_input_chars` 做 hard cap；
4. 保留当前 user message、最近可承接 user message 和显式历史引用词所在消息；
5. 对超预算窗口调用 `tonglingyu-current-window-compressor`；
6. compressor 输出 strict JSON `current_window_digest`；
7. digest validator 校验 source refs、覆盖范围、禁止新增事实、预算和 schema；
8. 从 raw/digest 中抽取 `referent_candidates` 和 `query_context_terms`；
9. 记录 `window_policy`、`compression_policy`、source refs、digest version、provider 和 rule ids；
10. 把 raw window、digest 和候选池按 consumer 生成不同 projection。

建议 input contract：

```json
{
  "current_window": {
    "messages": [
      {
        "id": "turn-12",
        "role": "user",
        "content": "史湘云的结局",
        "source_ref": "request.messages[-2]",
        "must_preserve": true
      },
      {
        "id": "turn-13",
        "role": "assistant",
        "content": "...",
        "source_ref": "request.messages[-1]",
        "compressible": true
      },
      {
        "id": "turn-14",
        "role": "user",
        "content": "脂批中的证据呢",
        "source_ref": "request.messages[-0]",
        "must_preserve": true
      }
    ],
    "window_policy": {
      "source": "request_payload",
      "max_raw_messages": 12,
      "max_raw_chars": 24000,
      "max_compressor_input_chars": 16000,
      "must_preserve_user_turns": 3
    },
    "compression_policy": {
      "mode": "llm_required_when_over_budget",
      "compressor_profile": "tonglingyu-current-window-compressor",
      "digest_schema": "tonglingyu.current_window_digest.v1",
      "preserve_roles": true,
      "preserve_user_questions": true,
      "preserve_source_refs": true
    }
  }
}
```

建议 output contract：

```json
{
  "schema_version": "tonglingyu.current_window_digest.v1",
  "source_window_hash": "sha256:...",
  "coverage": {
    "included_refs": ["turn-12", "turn-13", "turn-14"],
    "omitted_refs": [],
    "truncated_refs": ["turn-13"],
    "status": "complete_for_intent_resolution"
  },
  "recent_user_questions": [
    {
      "source_ref": "turn-12",
      "text": "史湘云的结局"
    },
    {
      "source_ref": "turn-14",
      "text": "脂批中的证据呢"
    }
  ],
  "candidate_subject_mentions": [
    {
      "text": "史湘云",
      "source_ref": "turn-12",
      "role": "topic_candidate"
    }
  ],
  "ellipsis_anchors": [
    {
      "source_ref": "turn-14",
      "anchor_ref": "turn-12",
      "reason": "follow_up_evidence_request"
    }
  ],
  "answer_boundary_notes": [
    {
      "source_ref": "turn-13",
      "note": "上一轮回答主题是史湘云结局，用户继续追问脂批证据。"
    }
  ],
  "unsupported_or_uncertain": []
}
```

LLM compression 不是自由摘要。Compressor prompt 必须要求：

1. 只从输入窗口抽取和压缩；
2. 每个 digest item 必须带 `source_ref`；
3. 不得新增输入中不存在的人物、版本、证据、出处、结论；
4. 不得把 assistant 旧回答当作红楼文本证据；
5. 不得输出最终答案、证据包、review decision 或 retrieval query；
6. 不确定时写入 `unsupported_or_uncertain`，不得猜测。

压缩结果的硬约束：

1. 不得把 `current_window_digest` 当 evidence；
2. 不得从 digest 中新增原窗口没有的人物、地点、版本、出处或结论；
3. user message 原文优先于 assistant digest；
4. digest 只能服务 normalizer、state writer、composer/reviewer 的意图边界；
5. retrieval 只能消费 validated `resolved_question` 和带 rule id 的 `query_context_terms`；
6. digest 冲突或缺失时必须降级为澄清，而不是让下游 profile 猜。

Digest validator 必须检查：

1. `source_window_hash` 匹配本次 compressor input；
2. 所有 `source_ref` 都来自本次 `current_window`；
3. `recent_user_questions` 不得改写用户原话，只能摘录；
4. `candidate_subject_mentions` 必须能在对应 source ref 中找到原文或别名；
5. `ellipsis_anchors.anchor_ref` 必须指向同一窗口内的可承接 user turn；
6. assistant turn 只能进入 `answer_boundary_notes`，不能进入 evidence 或 source scope；
7. `coverage.status` 为 `partial` 时，normalizer 必须降低 confidence 或转澄清；
8. digest 中出现输入窗口没有的实体、出处、证据或结论时直接拒绝；
9. compressor timeout、schema invalid 或 validator reject 时，不得用未验收 digest 继续主路径。

上下文大小超限时，不允许静默丢弃关键信息。必须在 audit 中记录：

```json
{
  "current_window_budget": {
    "raw_message_count": 18,
    "compressor_input_message_count": 12,
    "hard_cap_dropped_message_count": 6,
    "llm_compressed_message_count": 12,
    "max_raw_chars": 24000,
    "max_compressor_input_chars": 16000,
    "policy_id": "current_window.llm_compression.v1",
    "digest_status": "accepted"
  }
}
```

这样 trace 才能解释“为什么 normalizer 没绑定到某个历史对象”，也能区分“窗口中没有”、
“窗口中有但被压缩丢了”和“窗口中有但规则拒绝绑定”。

## Normalizer Contract 调整

后续 normalizer input 应包含：

```json
{
  "current_question": "...",
  "current_window": {
    "messages": [
      {"role": "user", "content": "..."},
      {"role": "assistant", "content": "..."},
      {"role": "user", "content": "..."}
    ],
    "window_policy": {
      "max_raw_messages": 12,
      "max_raw_chars": 24000,
      "max_compressor_input_chars": 16000,
      "source": "request_payload"
    }
  },
  "current_window_digest": {
    "schema_version": "tonglingyu.current_window_digest.v1",
    "source_window_hash": "sha256:...",
    "coverage": {
      "status": "complete_for_intent_resolution"
    },
    "recent_user_questions": [],
    "candidate_subject_mentions": [],
    "ellipsis_anchors": [],
    "answer_boundary_notes": []
  },
  "referent_candidates": [],
  "candidate_source_policy": {},
  "compression_rules_version": "...",
  "ellipsis_rules_version": "...",
  "subject_ontology_version": "..."
}
```

后续 normalizer output 应继续保持 strict JSON，但增加可审计字段：

```json
{
  "schema_version": "v1",
  "resolved_question": "...",
  "referent_bindings": [],
  "used_context_refs": [],
  "candidate_bindings": [
    {
      "canonical": "史湘云",
      "source": "current_window.previous_user_message",
      "rule_id": "ellipsis.evidence_followup"
    }
  ],
  "confidence": 0.9,
  "needs_clarification": false,
  "clarification_question": null,
  "unsupported_reason": null
}
```

如果暂不扩 schema，`candidate_bindings` 可以先进入 audit，不进入 public-facing contract；
但 validator 必须能记录 candidate source 和 rule id。

## 外部规则目录

设计上至少需要四类外部规则：

```text
subject_ontology.json
referent_candidate_rules.json
ellipsis_resolution_rules.json
current_window_compression_rules.json
```

`subject_ontology.json`：人物、物件、概念、版本术语和别名。

`referent_candidate_rules.json`：候选来源、优先级、置信度、升降权、是否允许跨 session。

`ellipsis_resolution_rules.json`：省略问句模式、承接窗口、澄清模板、冲突处理。

`current_window_compression_rules.json`：压缩预算、must-preserve turn、compressible turn、
digest schema、coverage 状态、validator 拒绝条件和超时处理。

这些规则必须：

1. 有 schema version 和 catalog version；
2. 支持热加载；
3. 在 trace/audit 中记录命中 rule id；
4. 不包含答案 oracle；
5. 不替代 evidence package 或 reviewer；
6. 与 query expansion、evidence slot rules 分离管理。

## Validator 规则

Validator 必须检查：

1. `resolved_question` 非空；
2. `referent_bindings` 可为空；
3. 非空 binding 必须来自候选池或当前问题直接出现；
4. 低权重 `session_journal_candidate` / `memory_candidate` 不能覆盖高权重 `current_window`；
5. 绑定历史候选必须有显式历史引用触发词；
6. `needs_clarification=true` 时必须有 `clarification_question`；
7. 澄清是正常业务响应，不是系统错误；
8. output 不得包含 forbidden fields；
9. raw provider output 不得进入 public response；
10. audit 必须记录 candidate source、confidence、rule id 和 rule catalog version。

## Runtime 与 Projection 边界

`current_window` 应进入统一 context path，先生成 `resolved_question` 和
`current_window_digest` / `current_window_projection`，再由不同 agent 消费各自投影。
它不应只投影给 main profile，也不应作为 raw session history 下发给所有 profile。

正确顺序：

```text
Open WebUI request
  -> current_question + current_window extraction
  -> current_window budget check + hard cap
  -> current-window compressor
  -> digest validator
  -> subject candidate generation
  -> question normalizer
  -> validator
  -> resolved_question
  -> context_pack
  -> profile projections
  -> retrieval/package/draft/review
```

错误顺序：

```text
Open WebUI request
  -> unresolved current_question
  -> retrieval
  -> package
  -> conversation_state_summary.open_questions
  -> main draft tries to recover intent
```

Text/commentary retrieval profiles should receive only the validated `resolved_question` and their
own projection. They should not receive full session history, raw journal, memory, or provider prompt。

## Agent 可见性矩阵

`current_window` 的使用范围应由 Gateway 统一裁剪，不能由各 agent 自行读取完整消息。

| Consumer | 可见内容 | 不可见内容 | 允许输出 |
| --- | --- | --- | --- |
| `tonglingyu-current-window-compressor` | hard-capped `current_window`、compression rules、source refs | evidence package、retrieval internals、journal、memory、draft/review | validated candidate digest JSON |
| `tonglingyu-question-normalizer` | `current_question`、bounded `current_window` 或 accepted `current_window_digest`、referent candidates、规则版本 | evidence package、retrieval internals、长期 memory 原文、rejected digest | `resolved_question`、bindings、clarification decision |
| `tonglingyu-conversation-state-writer` | bounded `current_window` 或 accepted `current_window_digest`、已验收的 `resolved_question`、上一轮 public answer 摘要 | raw provider output、未验收 normalizer draft、rejected digest | state summary、active entities、open questions |
| `honglou-text` | 已验收 `resolved_question`、source scope、检索参数 | raw `current_window`、journal、memory、composer draft | text evidence candidates |
| `honglou-commentary` | 已验收 `resolved_question`、source scope、检索参数 | raw `current_window`、journal、memory、composer draft | commentary evidence candidates |
| evidence package builder | evidence candidates、source scope、slot rules、必要的 `query_context_terms` | raw `current_window`、raw upstream draft | `package_v2` |
| answer composer | `resolved_question`、`package_v2`、compact current-window intent summary、conversation state | raw `current_window`、raw journal、memory 原文 | user-facing answer or clarification |
| reviewer | `resolved_question`、answer、`package_v2`、compact intent summary、answer boundary | raw `current_window`、raw provider output、未命中证据 | accept/reject/revise reason |

`query_context_terms` 只允许由 normalizer/rules 从 `current_window` 中提取，并带上
`rule_id`、`source_ref` 和 catalog version。Retrieval profile 不能直接读取 raw
`current_window` 来扩展查询。

## 非 Normalizer 用法

`current_window` 不只服务 normalizer，但其他 agent 的使用必须更窄：

1. current-window compressor 只把超预算窗口压成 accepted digest，不能回答问题、
   不能产出证据、不能产出检索 query。
2. conversation-state writer 用它更新会话状态，但不能覆盖本轮已验收的
   `resolved_question`。
3. answer composer 用 compact intent summary 组织自然回复、判断是否需要澄清，
   但不能把对话上下文当作红楼文本证据。
4. reviewer 用 compact intent summary 检查答案是否答错对象、答偏问题或泄露内部字段，
   但不能用它补充证据。
5. retrieval 只使用 validated `resolved_question` 和带规则来源的
   `query_context_terms`，不能消费完整对话窗口。
6. evidence package builder 只接收本地命中的 evidence cards 和 slot/rule 结果，
   不能把 current-window 里的用户说法写进 `package_v2` 当证据。

因此，`current_window` 的正确抽象不是“给 normalizer 的额外 prompt”，而是 Gateway
context path 的一等输入。它先被解析、裁剪、审计，再按 consumer 分发。

## Eval 与验收

新增 eval fixture 应覆盖：

1. `史湘云的结局` -> `脂批中的证据呢` 应改写为史湘云脂批证据问题；
2. subject ontology alias：`湘云的结局` -> `脂批证据呢`；
3. `她的证据呢` pronoun + current_window；
4. 当前窗口高权重候选与 `session_journal_candidate` 冲突时选择当前窗口；
5. 用户显式说“上次林黛玉那个”时 journal candidate 升权；
6. “之前第4条里说的”需要 journal cursor / turn index，定位不到时澄清；
7. `referent_bindings=[]` 的完整问题仍可通过；
8. `needs_clarification=true` 走普通 assistant clarification；
9. low-confidence memory candidate 不得直接绑定；
10. 超预算窗口触发 `tonglingyu-current-window-compressor`，accepted digest 进入 normalizer；
11. compressor 在 digest 中新增输入不存在的主体或证据时被 validator 拒绝；
12. compressor timeout / invalid JSON / coverage partial 时走澄清或受控失败，不继续使用 rejected digest；
13. public response 不泄露 current_window、candidate pool、rule id 或 internal audit。

验收必须至少包含：

```text
cargo test -p tonglingyu-gateway question_resolution
cargo test -p tonglingyu-gateway llm_agent_validator
cargo test -p tonglingyu-gateway context_governance
gateway smoke: Open WebUI multi-turn follow-up
trace audit: compression policy, digest status, resolved_question, candidate source, rule id present
```

目标环境证明必须使用新请求生成新 trace。旧 trace 只能作为问题回放材料，不能证明新设计已生效。

## 实施边界

后续实施可以分为若干提交，但不能保留双路径语义：

1. 外部化 subject ontology、referent/ellipsis rules 和 compression rules；
2. 引入 `current_window` extraction contract；
3. 引入 `tonglingyu-current-window-compressor` profile 和 digest validator；
4. 引入 `current_window_digest` / `current_window_projection` contract 和 agent 可见性矩阵；
5. 生成结构化 referent candidates；
6. 改 normalizer trigger，覆盖 ellipsis；
7. 改 validator，支持候选来源和澄清业务响应；
8. 确保 `resolved_question` 先于 retrieval 生效；
9. 让 composer/reviewer 消费 compact intent summary，而不是 raw window；
10. 补 eval、trace 脚本和 release gate。

不允许的折中：

1. 只把 `史湘云` 写进 hardcode subject list 后宣布修复；
2. 只让 draft 用 `open_questions` 自行补救；
3. 让 text/commentary profile 直接读取完整 session history；
4. 把 session journal subject 写成“最近讨论对象”；
5. normalizer 不确定时返回系统错误；
6. 把 query expansion 当作 subject ontology；
7. 把 candidate rules 写死在 Rust if/else 中；
8. 把 raw `current_window` 广播给所有 agent；
9. 允许 retrieval、package builder 或 reviewer 自行从 chat history 补证据；
10. compressor 输出未通过 validator 时继续使用 rejected digest；
11. compressor 把 assistant 旧回答、用户说法或 digest 当作红楼文本证据；
12. LLM compression 超时后静默退回不可审计摘要。
