# Question Frame 与关系题治理 Checklist

本文用于约束关系题、追问题和外部规则治理的实现。目标不是修某个
small10 用例，而是建立稳定的 question frame 主路径，避免把答案逻辑
转移到外部规则或单题特例中。

## 目标

- 独立问题不因 conversation-state writer 输出不合规而直接 500。
- 关系题解析为结构化 question frame，保留 subject / predicate / object。
- 多轮追问继承上一轮关系框架，而不是只绑定新出现实体。
- 检索、draft 和 reviewer 都绑定 question frame，不允许证据真实但答非所问。
- 外部规则只承载可变语言和领域词表；系统不变量必须由 typed code 和测试保证。

## 非目标

- 不为 `紫鹃`、`史湘云` 或 small10 写问题专属分支。
- 不把“X 是否服侍 Y”的答案写进 query expansion、ontology 或 answer rules。
- 不用本地 fallback 把失败包装成成功。
- 不放宽 reviewer 或 source-scope gate。

## Work Packages

- [x] WP1: 定义 `QuestionFrame`
  - [x] 包含 `intent`、`subject`、`predicate`、`object`、`source_scope`、
        `required_evidence_types`、`confidence`、`needs_clarification`。
  - [x] frame 字段必须来自 current window、当前问题或外部 ontology 候选池。
  - [x] frame 不得读取 raw memory 或完整历史。

- [x] WP2: 调整 context governance 主路径
  - [x] current window + deterministic rules 先构造 frame。
  - [x] conversation-state writer 只写未来 state；schema invalid、reasoning-only、
        hallucinated entity 时拒绝写入 projection 并 audit。
  - [x] 独立问题 frame 已解析时，state writer 失败不阻塞当前 runtime。
  - [x] 指代依赖但 frame 无法解析时返回澄清，不返回内部错误。

- [ ] WP3: 关系 predicate ontology 与规则边界
  - [x] 外部规则只表达 predicate alias，例如 `serve` 的服侍、伏侍、侍候等。
  - [x] 规则不能表达某个具体人物关系的真假。
  - [ ] catalog 不完整时进入 clarification / insufficient coverage / review rejection。
        已覆盖已识别 relation 的 insufficient coverage / review rejection；
        未识别 predicate 的澄清边界仍需补齐。

- [x] WP4: frame 驱动检索
  - [x] 关系题检索必须同时使用 subject alias、predicate alias、object alias。
  - [x] follow-up `那 X 呢` 必须继承上一轮 predicate 和未变 subject/object 角色。
  - [x] package 如果只命中对象生平或诗词活动，不能视为关系题覆盖充分。

- [ ] WP5: draft 与 answer 边界
  - [x] 负例关系回答统一使用公共边界措辞：未见直接证据、不能确认或证据不足。
  - [ ] 正例关系回答必须明确回答 predicate。当前已要求证据直连 predicate，
        但还需要补齐 draft 文本层的明确回答检查。
  - [x] draft 不得暴露 frame、slot、package、trace 等内部字段。

- [ ] WP6: reviewer predicate-preservation gate
  - [ ] `predicate=serve` 时最终回答必须回答服侍关系。当前已在证据层拒绝
        非直连 package；draft 文本层 gate 待补齐。
  - [ ] 回答跑到人物介绍、诗词、结局等其他维度时必须拒绝。当前覆盖了
        package 只有对象生平/诗词活动的场景；最终 answer semantic gate 待补齐。
  - [ ] 拒绝后走 retrieval repair 或 clarification，不接受偏题答案。

- [ ] WP7: Eval 与回归
  - [x] 新增 state writer reasoning-only 不阻塞独立问题测试。
  - [ ] 新增 state writer hallucinated entity 不进入 candidate pool 测试。
  - [x] 新增 `X 服侍过谁？/那 Y 呢？` 继承关系框架测试。
  - [x] 新增 relation package 覆盖不足测试。
  - [ ] 新增 relation answer predicate 跑偏被 reviewer 拒绝测试。
  - [ ] small10 作为回归集重跑，但不作为唯一验收。

## Exit Criteria

- [ ] Rust unit tests 覆盖 question frame、context governance、relation retrieval、
      reviewer gate。
- [x] `scripts/qa.sh --quick` 通过。
- [ ] hhost 部署后 small10 有效运行，不再出现 relation follow-up 跑偏。
- [ ] trace 中可看到 frame / resolver / context governance 的可审计边界，
      普通用户响应不泄露内部字段。
