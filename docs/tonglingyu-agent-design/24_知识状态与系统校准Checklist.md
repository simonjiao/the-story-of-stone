# 24 知识状态与系统校准 Checklist

## 目标

本 checklist 用于把通灵玉下一阶段从“已有 RQA 治理能力”推进到“知识条目可分层标记、
可系统校准、可运行使用、可运行中人工升级”的闭环。

目标链路：

```text
source snapshot
  -> candidate knowledge item
  -> system_calibrated
  -> runtime_usable
  -> human_marked / rejected / deprecated
  -> KB rebuild diff
  -> eval and release gate
```

完成前不能声明“运行中知识治理闭环已完成”。只完成文档、只完成临时 JSON、只完成
人工表格或只在 fixture 中通过，都不能算完成。

## 设计一致性约束

本 checklist 必须同时满足以下设计边界：

1. 不改变 Open WebUI 单模型入口；普通用户仍只看到 `tonglingyu`。
2. 不把 Gateway 做成第 5 个 Agent；Gateway 只做协议、鉴权、trace、SSE、模型隐藏和
   Runtime step plan。
3. 不让 Gateway 重新持有 source snapshot、FTS、证据包、reviewer 或知识治理领域逻辑。
4. 所有可运行知识条目必须来自已登记 source snapshot 或可追溯治理任务，不能由模型
   无来源生成事实。
5. `system_calibrated` 不是自动上线许可；只有被运行策略显式允许并记录为
   `runtime_usable` 的条目，才能进入普通回答、证据包或 eval 样本。
6. `human_marked` 只能来自运行中的人工或专家复核动作，并且必须有 reviewer、note、
   evidence ref 和 audit event。
7. 人工复核不是当前阶段的前置批处理；它是运行过程中的升级、否决和纠错动作。
8. 影印件、权威校注本、学术整理本仍放在知识治理末尾；当前 checklist 不把这些来源
   设为前置项。
9. 普通回答可以展示安全的来源/标记摘要，但不能泄露 admin trace、RQA failure、
   governance task 内部字段或原始隐私文本。
10. reviewer 仍是硬流程；系统校准知识不得绕过证据包、claim-to-evidence 和 reviewer。
11. release gate 检查边界、质量、open P0 和行为配置一致性；不能要求所有条目都先人工
    确认后才能运行。
12. 所有新增 schema 必须 additive migration，不破坏既有 package、audit、session、
    RQA 或 KB 数据。
13. LLM 校准执行者必须是受 Runtime/Hermes 配置和 release report 绑定的执行面；
    不能临时调用未声明模型，也不能把测试 fake 当成生产校准。
14. 校准入口可以引入新的内部治理 profile contract，但该 profile 不改变四个回答
    profile 的用户链路，不对普通用户可见，也不能替代 reviewer。
15. 校准主流程必须离线或异步执行；普通用户实时请求路径只能读取已经完成状态流转的
    知识条目，不能在同一次回答中临时调用 LLM 校准并立即使用结果。

## 不折中红线

以下情况不能标记为完成：

1. 只有文档或命名，没有 Rust 类型、schema version、store API 和测试。
2. 只有 `human_marked` 一个状态，缺少 `system_calibrated` 和 `runtime_usable`。
3. `system_calibrated` 条目没有 evidence ref、source boundary、confidence 和
   calibration report。
4. 普通回答、admin trace、RQA report 或 release report 无法区分系统校准和人工标记。
5. LLM 直接写入 facts、aliases、terms、relationships、events、poems 或 eval cases，
   没有治理任务、校准报告、KB diff 和 eval gate。
6. 人工复核结果直接覆盖事实层，不经过状态流转、KB rebuild、eval 和 audit。
7. rejected/deprecated 条目仍被 Runtime 用于证据或回答。
8. 低置信、证据不足或 source boundary 不清的条目被提升为高置信或人工标记。
9. release report 缺知识状态摘要、KB diff、eval impact 或 open P0 governance 状态。
10. 新增字段泄露用户问题原文、trace/package 列表、secret 或高基数字段。
11. 只覆盖 alias、term、commentary_link 或 version_note 等局部类型，却把知识状态
    治理闭环标为完成。
12. LLM 校准缺少显式 profile contract、model/upstream binding、prompt digest、
    tool policy、timeout、decoding 参数和 release report 记录。
13. 普通 chat 请求路径同步触发 LLM 校准，并把同次返回结果直接当作
    `runtime_usable` 或 `human_marked` 知识。
14. 异步校准任务没有 durable job id、input digest、lease/heartbeat、幂等键、重试上限、
    状态历史和 audit event。
15. `system_calibrated` 未经过 runtime policy、release run 和 saved report validator
    绑定，就被当作普通回答可用知识。
16. 计划中的验收命令仍是“命令名待定”，却把对应 milestone 标记为完成。
17. 只给 happy path 样例，没有按 KnowledgeItemKind、source boundary、低置信、
    forbidden conclusion 和 reviewer downgrade 建立覆盖矩阵。

## LLM 校准执行者

本文中的 LLM 不是任意外部模型，也不是人工聊天式判断。它是通灵玉 Runtime 下的
受控校准执行者，必须满足：

1. 通过 Runtime/Hermes client 执行，或通过等价的本地 Runtime client contract 执行；
2. 有专门的 calibration profile contract，例如 `honglou-knowledge-calibrator`；
3. profile contract 记录输入 schema、输出 schema、允许工具、禁止行为和版本号；
4. 输入只能包含候选条目、证据片段、source boundary、负面清单和校准任务上下文；
5. 输出只能是结构化 `KnowledgeCalibrationReport` 候选，不得直接写事实表；
6. 生产配置必须绑定 model/upstream id、prompt digest、tool policy digest、reviewer
   policy digest、decoding 参数、timeout 和 retry 策略；
7. 生产配置必须绑定模型能力档位和 reasoning effort；默认使用高推理校准 profile，
   降级为低推理或低能力模型必须有 eval 非退化证据和 release report 记录；
8. 配置值从既有 `.env` 或配置入口读取，release report 只记录摘要、digest 和变量名，
   不输出 token、API key 或 secret 值；
9. 生产校准缺少 LLM 配置时必须 fail-closed，不能自动退回未声明模型；
10. fixture/fake LLM 只允许用于单测和 contract smoke，不能作为完成或 production-ready
   证据；
11. LLM 校准结果必须继续经过规则 gate、eval gate、reviewer gate 和 release gate。

## 执行模式

校准默认不是在线聊天链路的一步。执行模式分三层：

1. 离线批处理：source snapshot、治理任务、低置信清单、retrieval failure 和 eval miss
   生成候选条目后，由校准 runner 批量执行规则、LLM、eval 和 RQA 校准。
2. 运行中异步触发：普通用户 feedback、admin trace、package replay 或 reviewer downgrade
   只能生成 governance task / calibration job；任务完成、写入 report、通过 gate 后，结果
   才能进入后续请求可用的知识状态。
3. 实时回答路径：Runtime 只读取已发布或已允许运行的 `source_snapshot`、
   `runtime_usable` 和 `human_marked` 条目；`system_calibrated` 必须先经过
   runtime policy 绑定，才能作为 `runtime_usable` 被引用。遇到低置信或缺证据时
   生成异步治理任务并降级回答，不能临时调用校准 LLM 后立即引用。

离线和异步任务必须具备 durable job id、input digest、output artifact digest、
lease/heartbeat、幂等键、重试上限、并发上限、状态历史和 audit event。没有这些工程
边界，不能声明“系统校准入口”完成。

## Milestone A：知识状态模型

状态：已完成，2026-05-17；提交见本节点提交。

目标：把知识条目的状态层级固化为生产 schema 和 Rust contract。

- [x] 定义 `KnowledgeState` Rust enum：
  `source_snapshot`、`candidate`、`system_calibrated`、`runtime_usable`、
  `human_marked`、`rejected`、`deprecated`。
- [x] 定义 `KnowledgeItemKind`：alias、term、commentary_link、version_note、
  person、relationship、event、poem、evaluation_case。
- [x] 为每类 knowledge item 定义稳定 id、source refs、evidence refs、payload hash、
  schema version 和 created/updated metadata。
- [x] 新增 additive migration，不重建 KB，不删除既有 package、audit、session 或 RQA
  数据。
- [x] Store API 支持 create/read/list/update state，并有分页、排序稳定和 max page
  size。
- [x] 状态转换必须有 compare-and-set 或版本条件，避免并发覆盖。
- [x] 每次状态转换写 audit event，记录 actor、previous state、new state、reason hash
  和 evidence ref。
- [x] rejected/deprecated 条目必须保留 tombstone 或状态历史，不能硬删除后失去复盘能力。
- [x] 单测覆盖 migration 幂等、状态转换、并发冲突、分页和 audit。

完成口径：

- [x] 生产 DB 可以在不重建 KB 的情况下升级 schema。
- [x] Runtime store 能稳定读写知识状态。
- [x] admin/API 输出能区分系统状态，但普通响应不泄露内部治理字段。

节点总结：

- Runtime 新增 additive `knowledge_items` 和 `knowledge_item_state_history` schema，
  并纳入 schema migration preflight。
- Runtime store 新增 create/read/list/update state API；item id 基于 kind、source refs
  和 payload hash 稳定生成，状态更新使用 `state_version` CAS。
- Gateway 新增只读 admin API：`/v1/admin/knowledge/items` 和
  `/v1/admin/knowledge/items/{item_id}`，只暴露状态边界，不提供人工复核写入口。
- `rejected` / `deprecated` 通过状态历史保留复盘链路，不硬删除。

验证：

- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`

## Milestone B：系统校准入口

状态：已完成。

目标：配置化 LLM、规则、eval 和 RQA report 可以把全部知识类型的候选知识校准为
`system_calibrated`，并在证据边界内进入运行候选。

- [x] 定义 `KnowledgeCalibrationReport`，包含 source refs、evidence refs、
  calibration method、confidence、quality issues、source boundary 和 report hash。
- [x] 定义 `honglou-knowledge-calibrator` 或等价内部治理 profile contract；该 profile
  不能对普通用户可见，不能进入 Open WebUI model list。
- [x] 支持配置化 LLM 校准；LLM 输出只能生成 candidate/report，不能直接改事实层。
- [x] LLM 校准配置必须绑定 model/upstream id、prompt digest、tool policy digest、
  decoding 参数、timeout、retry、模型能力档位、reasoning effort 和 profile contract
  version。
- [x] release report、calibration report 和 admin trace 记录 LLM 校准配置摘要和 digest，
  不记录 secret 值。
- [x] 实现离线校准 runner：输入 source snapshot / governance task / eval miss /
  retrieval failure，输出 calibration report artifact，不直接写事实层。
- [x] 实现异步 calibration job 模型：durable job id、input digest、幂等键、状态历史、
  lease/heartbeat、retry limit、concurrency limit 和 audit event。
- [x] 运行中触发只创建治理任务或 calibration job；任务未完成、未通过 gate 或未发布前，
  不能影响普通用户同次回答。
- [x] 支持规则校准：source id、block id、required evidence type、exact term、version
  boundary、usage boundary。
- [x] 支持 eval 校准：expected evidence hit、forbidden conclusion、reviewer status、
  source boundary confirmation。
- [x] 支持 RQA 校准：retrieval quality issue、failure cluster、governance task 和
  proposed fix 关联。
- [x] 校准通过后，条目从 `candidate` 进入 `system_calibrated`；未通过进入
  `rejected` 或保持 candidate 并写 issue。
- [x] `system_calibrated` 不能自动进入普通回答；必须由 runtime policy 依据 kind、
  confidence、source boundary、evidence type、expiry 和 release run 显式提升为
  `runtime_usable`。
- [x] 每个 `system_calibrated` 条目必须有非空 evidence ref、source boundary 和
  calibration report ref。
- [x] 校准报告不得保存完整用户问题、未脱敏 query、secret 或无界 LLM prompt。
- [x] 单测覆盖 LLM fake output、规则校准失败、eval miss、RQA failure 触发和隐私边界。
- [x] 集成测试覆盖真实配置解析；缺 LLM 配置、未知 profile、prompt digest 缺失或
  model/upstream 未绑定时 fail-closed。
- [x] 覆盖矩阵按 KnowledgeItemKind、source boundary、低置信、forbidden conclusion、
  reviewer downgrade、LLM 配置缺失和 runtime policy 拒绝分组保存。
- [x] 全量覆盖所有 `KnowledgeItemKind`：alias、term、commentary_link、version_note、
  person、relationship、event、poem、evaluation_case。任何类型缺校准路径都不能声明
  Milestone B 完成。

完成口径：

- [x] 所有 `KnowledgeItemKind` 都能至少走通 candidate -> system_calibrated。
- [x] 校准失败不会污染 runtime_usable 或事实层。
- [x] 校准 report 能被 admin trace、RQA report 和 KB diff 引用。
- [x] 真实 LLM 配置已进入 release/config digest；fake LLM 只作为测试证据存在。
- [x] `runtime_usable` 提升策略和覆盖矩阵已经进入可复核 artifact。

节点总结：

- Runtime 新增 `KnowledgeCalibrationReport`、内部
  `honglou-knowledge-calibrator` profile contract、配置化
  `KnowledgeCalibrationLlmConfig`、离线校准 runner 和 calibration job 模型。
- `honglou-knowledge-calibrator` 不进入公开 `profile_catalog()`，Gateway `/v1/models`
  仍只暴露普通 `tonglingyu` 模型；LLM evidence judge 只能输出 judgement/report，
  不能调用写工具或直接修改事实层。
- LLM 配置解析 fail-closed：profile、model/upstream、prompt digest、tool policy
  digest、decoding、timeout、retry、复杂模型能力档位、高推理强度和 profile
  contract version 均必须绑定。
- 校准 report 写入 report hash、source/evidence refs、source boundary、coverage
  matrix、config digest 和隐私标记；admin trace 通过 audit event 引用 report，RQA
  校准 report 可保留 RQA report refs，KB summary/diff 可带出 calibration report refs。
- 校准通过只把 `candidate` 升为 `system_calibrated`；`runtime_usable` 自动提升明确为
  false，覆盖矩阵保留 runtime policy 拒绝原因，后续仍需 Milestone C/E 接管真正运行使用。
- Gateway 新增可执行离线入口：

  ```bash
  cargo run --manifest-path agent-platform/Cargo.toml \
    -p tonglingyu-gateway -- knowledge-calibrate \
    --db <db> --input <calibration-input.json>
  ```

验证：

- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
- `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime
  --all-targets -- -D warnings`
- `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
  --all-targets -- -D warnings`

边界：

- Milestone B 完成不表示普通回答已经使用 knowledge item；`system_calibrated` 仍不能进入
  selected evidence 或展示为“人工标记”。
- Milestone C-E 仍未完成，不能声明运行中知识状态治理闭环完成。

## Milestone C：Runtime / Gateway 使用规则

状态：已完成，2026-05-17；提交见本节点提交。

目标：`system_calibrated` 只能作为运行策略候选；被 runtime policy 显式提升为
`runtime_usable` 后才能运行使用，且不能冒充人工确认。

- [x] Runtime 工具读取 knowledge item 时默认排除 `rejected` 和 `deprecated`。
- [x] Runtime 只能使用 `runtime_usable` 和 `human_marked` 条目；`system_calibrated`
  必须先被 runtime policy 显式提升或物化为 `runtime_usable`。
- [x] reviewer 能看到知识状态摘要，并在缺证据、低置信或状态不匹配时降级。
- [x] claim-to-evidence link 必须记录使用的 knowledge item id、state 和 evidence ref。
- [x] 普通回答可显示安全摘要，例如“基于当前已登记资料”或“人工标记”，但不能泄露
  governance task / RQA failure 内部字段。
- [x] 只有 `human_marked` 条目能展示“人工标记”字样。
- [x] `system_calibrated` 条目不得展示为“人工确认”“专家确认”或等价文案。
- [x] Gateway 不直接查询或修改 knowledge item；所有领域读取和状态逻辑通过 Runtime
  store/API。
- [x] streaming 和 non-streaming 响应边界一致，均不得泄露内部治理字段。
- [x] strict Gateway gate 增加知识状态泄露和错误标记检查。
- [x] 普通 chat、streaming chat 和 package replay 路径不得同步调用校准 LLM；缺证据时
  只能降级回答并生成异步治理任务。
- [x] Runtime policy 拒绝、过期、缺 release run、缺 evidence ref 或缺 source boundary
  的 `system_calibrated` 条目不得进入 selected evidence。

完成口径：

- [x] 普通 chat、streaming chat、admin trace 和 package replay 都能稳定处理知识状态。
- [x] 由 `system_calibrated` 提升为 `runtime_usable` 的回答不会越界声称人工确认。
- [x] `human_marked` 的显示路径可复核且有 audit。
- [x] selected evidence 中每个知识条目都能追溯到 `runtime_usable` 策略决策。

节点总结：

- Runtime 新增 `KNOWLEDGE_RUNTIME_POLICY_VERSION` 和 additive
  `evidence_claim_knowledge_links` schema；证据包内的 claim-to-evidence link 会在
  内部记录 item id、state、evidence ref、policy decision 和 calibration report ref。
- `system_calibrated` 只计入候选和 reviewer 降级摘要；未经过显式
  `promote_knowledge_item_runtime_usable`、release run、source boundary、evidence ref、
  calibration report ref、confidence 和 expiry 校验的条目不会进入 selected evidence。
- `runtime_usable` 和 `human_marked` 是运行期唯二可引用状态；`rejected`、
  `deprecated`、`candidate` 和 `source_snapshot` 默认被 Runtime policy 排除。
- 公开 `package_json`、package replay、本地回答、普通 completion 和 streaming delta
  只输出安全标签或中性计数；公开路径不输出 `item_id`、状态名、
  `calibration_report_ref`、`runtime_policy`、`policy_version` 或 release run。
- 只有 `human_marked` 会公开出现“人工标记”；`runtime_usable` 只显示
  “基于当前已登记资料”，`system_calibrated` 不会显示为人工或专家确认。
- Gateway 保持薄边界：普通路径和 admin/package 读取仍经 `TonglingyuRuntimeStore`；
  strict Gateway live gate 已加入知识状态标签和错误字段泄露检查。

验证：

- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
- `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
- `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime
  --all-targets -- -D warnings`
- `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
  --all-targets -- -D warnings`
- `bash -n deploy/scripts/verify-tonglingyu-strict-gateway.sh`

边界：

- Milestone C 完成只表示 Runtime/Gateway 使用规则闭合；不表示运行中人工复核入口
  已完成，也不表示 KB diff、eval 和 release gate 已能复核所有知识状态变化。
- `human_marked` 的完整人工复核写入口、reviewer metadata 强约束和 Open WebUI admin
  Action 操作面仍属于 Milestone D。
- release report、saved report validator 和 per-kind 状态变化 release gate 仍属于
  Milestone E。

## Milestone D：运行中人工复核入口

状态：已完成。

目标：人工复核作为运行中的治理动作，只负责升级、否决和纠错，不作为系统启动前置。

- [x] admin 可以把 trace、package、knowledge item、retrieval failure、eval miss 或用户
  feedback 标记为复核对象。
- [x] 复核任务必须绑定 source entity、evidence ref、reason、priority、reviewer 和
  status history。
- [x] 人工通过后，条目只能升级为 `human_marked`，并写“人工标记”所需 metadata。
- [x] 人工否决后，条目进入 `rejected` 或 `deprecated`，Runtime 不再使用。
- [x] 人工复核不能直接改 source snapshot 原文、已登记 source metadata 或 audit history。
- [x] 人工复核不能绕过 KB rebuild、eval diff 和 release gate。
- [x] Open WebUI admin Action 覆盖 list/read/update/review 知识状态入口，并保留 role
  guard、valves 和 secret 输出边界。
- [x] 普通用户 feedback 只能生成候选复核任务，不能直接升级条目。
- [x] 单测覆盖 admin role、普通用户拒绝、CAS 冲突、幂等提交、audit 和状态历史。

完成口径：

- [x] 运行中发现的问题可以进入复核队列。
- [x] 人工通过能升级为 `human_marked`，人工否决能阻止 Runtime 使用。
- [x] 所有复核动作可审计、可回放、可作为 Milestone E 的 KB diff 输入。

节点总结：

- Runtime 新增 `KnowledgeItemHumanReviewDecision`、
  `KnowledgeItemHumanReviewInput` 和 `review_knowledge_item_human` Store API；
  `human_marked` 不能再通过通用 state update 直接写入，必须绑定 governance task。
- Governance task source entity 扩展到 `knowledge_item` 和 `eval_miss`；Gateway
  管理端创建任务时支持 trace/package/retrieval failure/knowledge item/eval miss/user
  feedback 复核对象。
- 人工通过写入 `human_marked` 和 `human_review` metadata；人工否决写入
  `rejected` 或 `deprecated`。复核 payload 只记录 reviewer、evidence ref 和 note
  hash，不写原文 note 到 audit。
- Gateway 新增 `/v1/admin/knowledge/items/{item_id}/review`，并保留 admin auth、
  rate limit、CAS conflict、idempotent retry 和 admin audit。
- Open WebUI admin Action 新增 `knowledge_items`、`knowledge_item` 和
  `knowledge_item_review`，contract gate 校验 required actions、API path、role
  guard、valves 和 secret 输出边界。

验证：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime
cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
cargo clippy \
  --manifest-path agent-platform/Cargo.toml \
  -p tonglingyu-runtime \
  --all-targets -- -D warnings
cargo clippy \
  --manifest-path agent-platform/Cargo.toml \
  -p tonglingyu-gateway \
  --all-targets -- -D warnings
bash deploy/scripts/test-openwebui-gateway-admin-action-contract.sh
```

未提前声明：

- Milestone D 完成只表示运行中人工复核入口闭合；KB diff、eval impact、saved report
  validator 和 release gate 仍属于 Milestone E，不能声明完整知识状态治理闭环完成。

## Milestone E：KB diff、eval 和 release gate

状态：未开始。

目标：每次知识状态变化都能被 KB diff、eval 和 release gate 复核，避免提前宣布胜利。

- [ ] KB rebuild diff 记录新增、系统校准、runtime_usable、human_marked、rejected 和
  deprecated 的数量与摘要。
- [ ] KB diff 记录每个状态变化的 source refs、calibration report refs、human review
  refs 和 audit refs。
- [ ] eval report 输出按 knowledge state 分组的命中率、失败率、forbidden conclusion
  avoidance 和 reviewer downgrade。
- [ ] RQA quality gate 校验 rejected/deprecated 条目没有进入 selected evidence。
- [ ] release report 记录 knowledge state summary、KB diff hash、eval impact 和
  open P0 governance state。
- [ ] release report 记录本次采用的离线/异步 calibration run id、job summary、
  input/output artifact digest、配置 digest 和失败任务摘要。
- [ ] release report 记录 runtime policy version、`runtime_usable` promotion summary、
  per-kind coverage matrix 和未解决的校准缺口。
- [ ] saved report validator 重算 knowledge state summary，拒绝 report 手改状态。
- [ ] saved report validator 校验被使用的 `system_calibrated` 条目来自已完成、
  已通过 gate、未过期的 calibration report，而不是同次请求临时结果。
- [ ] saved report validator 校验 selected evidence 中不存在未提升为 `runtime_usable`
  的 `system_calibrated` 条目。
- [ ] release gate 不要求所有 runtime_usable 条目都是 human_marked，但要求状态边界、
  source boundary、reviewer 和 eval 全部一致。
- [ ] 当 `system_calibrated` 条目导致 eval 退化、forbidden conclusion 或 reviewer
  downgrade 时，release gate fail-closed，并生成治理任务。
- [ ] 文档、contract smoke、Rust tests、Gateway smoke 和 release readiness 都必须覆盖
  本 checklist 的新增状态字段。

完成口径：

- [ ] 能从 release report 追溯“本次上线使用了哪些状态层级的知识”。
- [ ] 能从 KB diff 看出哪些条目从 system_calibrated 升级为 human_marked 或被 rejected。
- [ ] eval 和 release gate 可以阻止错误状态进入 production-ready 报告。

## 最终验收矩阵

完成全部前 5 个 milestone 后，也必须分层声明，不能合并口径。

只允许声明：

```text
repo-local 通灵玉已具备知识状态治理闭环实现和自动化验收证据。
```

只有在目标 live 环境重新生成 release readiness report、calibration report、KB diff、
saved report validator 和 Open WebUI/Gateway 证据后，才允许声明：

```text
目标 live 环境当前 release 具备运行中知识状态治理闭环。
```

仍不能声明：

```text
通灵玉已完成影印件、权威校注本或学术整理本增强。
通灵玉已完成完整学术校勘。
所有知识条目都已经人工确认。
历史 artifact 可以证明当前 live 环境仍然 production-ready。
```

最低验收命令或证据：

1. `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
2. `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
3. `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
   `--all-targets -- -D warnings`
4. `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
   `--all-targets -- -D warnings`
5. `agent-platform/scripts/tonglingyu-gateway-smoke.sh`
6. `deploy/scripts/test-openwebui-gateway-admin-action-contract.sh`
7. `deploy/scripts/test-tonglingyu-release-readiness-contract.sh`
8. `deploy/scripts/verify-tonglingyu-release-readiness.sh`
9. `deploy/scripts/verify-tonglingyu-release-readiness-report.sh`
10. `npx --yes markdownlint-cli2 docs/tonglingyu-agent-design/*.md`

计划新增并在实现后纳入验收的命令或证据：

1. `agent-platform/scripts/tonglingyu-knowledge-calibration-run.sh`：离线 calibration
   runner 执行记录和 report artifact，待 Milestone B 实现。
2. `agent-platform/scripts/tonglingyu-calibration-job-smoke.sh`：异步 calibration job
   的 fail-closed / retry / lease smoke，待 Milestone B 实现。
3. `deploy/scripts/verify-tonglingyu-knowledge-state-release.sh`：release readiness
   validator 对 calibration run id、artifact digest、runtime policy promotion 和同次请求
   临时结果的拒绝证据，待 Milestone E 实现。

以上 3 项在命令名、artifact 路径、退出码和 validator 规则落地前，只能作为缺口清单，
不能作为任何 milestone 完成证据。

如果目标 live 环境参与声明，还必须重新生成当次 live release readiness report 和 saved
report validator 输出；旧 artifact 只能作为历史证据，不能证明当前状态。

## 提交节奏

1. Checklist 和设计口径单独提交。
2. Milestone A 完成后提交 schema/type/store。
3. Milestone B 完成后提交 calibration report 和校准入口。
4. Milestone C 完成后提交 Runtime/Gateway 使用边界。
5. Milestone D 完成后提交运行中人工复核入口。
6. Milestone E 完成后提交 KB diff、eval 和 release gate。

每个 milestone 完成前必须更新本 checklist 的状态、节点总结、验证命令和仍未完成项。
