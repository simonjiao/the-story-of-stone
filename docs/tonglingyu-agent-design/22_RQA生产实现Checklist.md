# 22 RQA 生产实现 Checklist

## 目标

本 checklist 用于跟踪通灵玉二阶段召回质量治理的生产级实现。目标不是只补可观测
字段，而是交付一个能进入 release gate 的闭环：

```text
RetrievalQualityReport
  -> retrieval_failures
  -> eval quality metrics
  -> admin trace
  -> release readiness quality gate
  -> saved report validator
```

完成前不能声明 RQA production-ready。

## 实施原则

1. 不降低现有 Gateway + Runtime Agent production-ready 入口标准；
2. 不允许本轮验收停留在“只报告不阻塞”；
3. 不把 Agent 诊断或用户反馈直接写入事实层；
4. 不让普通用户响应暴露 RetrievalQualityReport 或 admin trace 内部字段；
5. 所有新增 schema 采用 additive migration，不重写既有 package/audit 数据；
6. 每个大节点完成后更新本 checklist，并及时提交。

## 不折中红线

以下情况不能被标记为完成，也不能进入 production-ready 报告：

1. 只写日志、临时 JSON 或脚本输出，没有 Rust 类型、schema version 和 store API；
2. 只在 fixture/mock gate 下通过，真实 release gate 未接入或可被 override 绕过；
3. 通过降低阈值让 production-ready 通过；
4. expected evidence 分母为空或关键 eval case 未分类，却声明 hit@k 达标；
5. 只有人工操作说明，没有 admin API / Action、权限校验和 audit event；
6. schema 迁移破坏既有 package、audit、session 或 KB 数据；
7. 普通用户能看到或修改 RQA 内部字段、failure 状态或治理任务；
8. Agent 生成的诊断、聚类或修正建议绕过人工状态流转直接写入事实层。

## 决策基线

| 决策 | 口径 |
| --- | --- |
| 第一轮范围 | RQA-1 到 RQA-12 均纳入生产闭环 |
| release gate | 本轮必须支持质量 gate 作为 blocker |
| 阈值配置 | 本轮必须内置 production 默认值；配置只能进入报告并接受校验 |
| expected evidence | 有标注的 case 进入 hit@k 分母，无标注 case 仍计普通 eval |
| failure 表 | 独立 `retrieval_failures` 表 |
| report 落点 | 完整 report 进入 audit/admin，普通响应只出安全摘要 |
| 人工状态 | retrieval failure 必须有人工处理状态字段 |
| 治理任务 | RQA-6 到 RQA-12 必须有可持久化任务和状态流转 |

## Production 默认阈值

本轮默认阈值必须先按严格 production gate 实现。后续可以配置覆盖，但覆盖值必须进入
release report 和 saved report validator，不能只存在于运行环境中。低于默认阈值的
配置只能产生 non-production report，不能关闭 production blocker。

| 指标 | 默认要求 |
| --- | --- |
| quality report coverage | 100% eval case 生成 report |
| eval case classification | 100% release case 标记 expected / not-applicable |
| expected evidence denominator | release suite 中必须大于 0 |
| expected_evidence_hit@8 | 有 expected evidence 标注的 case 必须 100% 命中 |
| required_type_coverage | 100% |
| exact_term_coverage | 有 protected term 的 case 必须 100% |
| forbidden_conclusion_avoided | 100% |
| reviewer_status_matched | 100% |
| open P0 retrieval failures | 0 |
| public_response_boundary_passed | 100% |
| admin_trace_quality_summary | 100% |

## Milestone A：Runtime 质量报告

状态：未开始

目标：Runtime search tools 为每次检索生成结构化 `RetrievalQualityReport`。

- [ ] 定义 `RetrievalQualityReport` Rust 类型和 JSON schema version。
- [ ] 覆盖 `tonglingyu.text.search`。
- [ ] 覆盖 `tonglingyu.commentary.search`。
- [ ] 记录 query terms、protected terms、expanded aliases。
- [ ] 记录 required / actual / missing evidence types。
- [ ] 记录 candidate / selected count 和 channel 分布。
- [ ] 记录 source diversity、edition diversity、exact term coverage。
- [ ] 记录 expected evidence hit 结果。
- [ ] 记录 quality status、issues、recommended follow-up。
- [ ] 单测覆盖完整、缺证据、缺 required type、exact term missing。

节点总结：

- 待实现。

## Milestone B：retrieval_failures schema

状态：未开始

目标：线上和 eval 的召回失败样本可追踪、可分派、可人工处理。

- [ ] 添加 additive runtime schema migration。
- [ ] 新增 `retrieval_failures` 表。
- [ ] 字段覆盖 failure id、trace id、package id、question、kb version。
- [ ] 字段覆盖 failure type、query terms、required / actual evidence types。
- [ ] 字段覆盖 expected / selected evidence ids、missing evidence types。
- [ ] 字段覆盖 quality issues、agent diagnosis、proposed fix。
- [ ] 字段覆盖 human review status、reviewer、note、created/resolved time。
- [ ] 添加必要索引：trace、package、status、failure type、created_at。
- [ ] 添加 Runtime store API：create/list/update/read。
- [ ] 单测覆盖迁移幂等、写入、读取、状态更新。

节点总结：

- 待实现。

## Milestone C：自动 failure 记录

状态：未开始

目标：Runtime 和 eval 在生产规则下自动记录 P0/P1 召回失败。

- [ ] 候选数为 0 时记录 failure。
- [ ] required evidence type 缺失时记录 failure。
- [ ] expected evidence 未命中时记录 failure。
- [ ] exact protected term 未命中时记录 failure。
- [ ] reviewer 因证据不足降级时记录 failure。
- [ ] package 无法支持关键 claim 时记录 failure。
- [ ] 去重相同 trace/package/failure type，避免重复刷表。
- [ ] failure 记录写入 audit event。
- [ ] 单测覆盖所有触发条件和去重。

节点总结：

- 待实现。

## Milestone D：eval 质量指标

状态：未开始

目标：eval 从 pass/fail 扩展为生产质量指标。

- [ ] 扩展 eval case，支持 expected evidence ids / block ids。
- [ ] 所有 release eval case 必须标记 expected evidence 或 not-applicable reason。
- [ ] expected evidence 分母为 0 时 release quality gate fail-closed。
- [ ] 增加 expected_evidence_hit@1。
- [ ] 增加 expected_evidence_hit@3。
- [ ] 增加 expected_evidence_hit@8。
- [ ] 增加 required_type_coverage。
- [ ] 增加 exact_term_coverage。
- [ ] 增加 source_diversity。
- [ ] 增加 edition_diversity。
- [ ] source / edition diversity 标明当前只覆盖 Wikisource source snapshot，
      不等同于影印或权威校注本复核。
- [ ] 增加 forbidden_conclusion_avoided。
- [ ] 增加 reviewer_status_matched。
- [ ] 无 expected evidence 标注的 case 不计入 hit@k 分母。
- [ ] eval report 输出 quality summary 和 case-level quality details。
- [ ] eval failure 自动写入 `retrieval_failures`。
- [ ] 单测覆盖有标注、无标注、阈值达标和阈值失败。

节点总结：

- 待实现。

## Milestone E：admin trace 和 metrics

状态：未开始

目标：管理员能从 trace/package/session 看到召回质量摘要和 failure 状态。

- [ ] admin trace 顶层暴露 retrieval quality summary。
- [ ] admin trace 关联 retrieval failure ids。
- [ ] admin package audit 可看到 quality issue 摘要。
- [ ] JSON metrics 暴露 retrieval failure count by status/type。
- [ ] Prometheus 暴露有界指标，不暴露 query 原文和 secret。
- [ ] admin-only API / Action 支持 list/read/update retrieval failure 人工状态。
- [ ] retrieval failure 人工状态更新写入 audit event。
- [ ] 普通用户不能读取或更新 retrieval failure。
- [ ] 普通 chat response 不暴露完整 quality report 或 failure 内部字段。
- [ ] streaming response 同样不泄露内部字段。
- [ ] strict Gateway gate 增加 admin trace quality summary 校验。
- [ ] 单测覆盖 admin 可见、admin update、普通响应不可见、普通用户不可更新。

节点总结：

- 待实现。

## Milestone F：release quality gate

状态：未开始

目标：召回质量指标进入 release readiness，并可作为 production blocker。

- [ ] 新增或扩展 release readiness quality gate。
- [ ] gate 读取 eval quality summary。
- [ ] gate 校验 expected_evidence_hit@8 阈值。
- [ ] gate 校验 eval case classification 为 100%。
- [ ] gate 校验 expected evidence denominator 大于 0。
- [ ] gate 校验 required_type_coverage 阈值。
- [ ] gate 校验 forbidden_conclusion_avoided。
- [ ] gate 校验 reviewer_status_matched。
- [ ] gate 校验 open P0 retrieval failures 为 0。
- [ ] gate 校验 quality report coverage 为 100%。
- [ ] gate 支持阈值配置，默认值采用本文 Production 默认阈值。
- [ ] 低于默认阈值的配置必须把报告标记为 non-production。
- [ ] gate report 记录实际阈值来源和有效阈值。
- [ ] 缺少 quality summary、缺少 report 或阈值配置不可解析时 fail-closed。
- [ ] gate 输出不泄露 secret 和过长日志。
- [ ] release readiness report 包含 quality gate record。
- [ ] production-ready 时 quality gate 必须 passed。
- [ ] contract smoke 覆盖 passed、threshold failed、open P0 failure、missing report。

节点总结：

- 待实现。

## Milestone G：saved report validator

状态：未开始

目标：保存后的 release report 不能通过手改绕过质量 gate。

- [ ] validator 要求 canonical gate set 包含 retrieval quality gate。
- [ ] validator 重算 quality gate 派生字段。
- [ ] validator 校验 production-ready report 必须 quality gate passed。
- [ ] validator 校验 quality gate stdout JSON schema。
- [ ] validator 校验 threshold / blocker / ready flag 不漂移。
- [ ] validator 校验 effective thresholds 与 report 中 gate 输出一致。
- [ ] validator 校验低于默认阈值的报告不能 production-ready。
- [ ] validator 继续扫描 secret-like values。
- [ ] contract smoke 覆盖删除 gate、篡改 passed、篡改 ready flag。

节点总结：

- 待实现。

## Milestone H：治理任务和反馈闭环

状态：未开始

目标：RQA-6 到 RQA-12 不停留在报告层，必须进入可审计的治理任务流。

- [ ] 新增 `knowledge_governance_tasks` 或等价持久化任务 schema。
- [ ] 支持从 retrieval failure 生成治理任务。
- [ ] 支持管理员把 trace / package / failure 标记为待专家复核。
- [ ] 支持普通用户反馈生成 retrieval failure 候选或治理任务。
- [ ] 普通用户反馈不能直接修改 source、alias、term、commentary link 或事实层。
- [ ] Agent 聚类 retrieval failures，并只生成 proposed fix。
- [ ] proposed alias / term / commentary link / version note 必须进入人工状态流转。
- [ ] accepted fix 必须绑定 reviewer、note、source 或 evidence ref。
- [ ] KB rebuild 后生成 kb_version diff report。
- [ ] kb_version diff report 对比前后 eval quality summary。
- [ ] release gate 可读取 open P0 governance tasks 并作为 blocker。
- [ ] 单测覆盖任务创建、状态流转、权限边界、Agent 建议不可直接采纳。

节点总结：

- 待实现。

## Milestone I：端到端验证

状态：未开始

目标：本地、容器、release contract 全部验证通过。

- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
- [ ] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
<!-- markdownlint-disable MD013 -->
- [ ] `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime --all-targets -- -D warnings`
- [ ] `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway --all-targets -- -D warnings`
<!-- markdownlint-enable MD013 -->
- [ ] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`
- [ ] `deploy/scripts/test-tonglingyu-release-readiness-contract.sh`
- [ ] `deploy/scripts/verify-tonglingyu-release-readiness.sh`
- [ ] `deploy/scripts/verify-tonglingyu-release-readiness-report.sh` fixture path
- [ ] production-ready report 不允许依赖 mock gate command override。
- [ ] production-ready report 不允许使用低于默认阈值的 RQA 配置。
- [ ] production-ready report 必须包含 RQA quality gate 和治理任务 gate。
- [ ] `npx --yes markdownlint-cli2 docs/tonglingyu-agent-design/*.md`

节点总结：

- 待实现。

## 提交节奏

1. Checklist 基线单独提交。
2. Milestone A-B 完成后提交 runtime schema/report 基础。
3. Milestone C-D 完成后提交 eval/failure 闭环。
4. Milestone E 完成后提交 admin trace/metrics 边界。
5. Milestone F-G 完成后提交 release gate 和 saved report validator。
6. Milestone H 完成后提交治理任务和反馈闭环。
7. Milestone I 通过后提交最终 production-ready 验证更新。

每次提交前必须更新本 checklist 的状态和节点总结。
