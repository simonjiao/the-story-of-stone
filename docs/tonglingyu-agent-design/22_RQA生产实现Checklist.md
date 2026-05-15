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

完成前不能声明 RQA production-ready，也不能把既有 R5D 生产入口基线当作本轮
production-ready 的替代验收。

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
9. 写入 quality report、failure、governance task 或 audit 失败后仍把请求标记为
   完整成功；
10. 没有覆盖并发更新、重复请求、保留清理、备份恢复和真实 live release。
11. RQA 为了诊断保存完整用户隐私文本、未脱敏 query、未截断 payload 或无界
    admin list；
12. Prometheus labels、JSON metrics 或 release report 使用 trace、user、question
    等高基数字段。
13. 没有 operator runbook、告警规则、回滚步骤和 post-release 监控窗口，却声明
    RQA production-ready。
14. 事故期间通过关闭 RQA、跳过持久化、使用无界队列或删除审计历史来维持
    production-ready。
15. RQA admin read/list 不写访问审计，或允许无界过滤、无界排序、枚举式探测。
16. release report 无法指出通过 gate 的 git commit、镜像 digest、schema version、
    eval suite version 和有效配置摘要。
17. public response、RQA report 或 release report 没有声明 source coverage
    boundary，却把当前 Wikisource snapshot 包装成影印件、权威校注或专家校勘
    已完成。
18. RQA release gate、saved report validator 或 contract smoke 只在人工本地命令中
    执行，没有进入 CI 或 release automation 的强制路径。
19. production-ready report 没有绑定当前 live KB 的 source snapshot digest、
    KB build hash、kb_version 和 eval run id，或使用了另一个 KB 构建的 RQA 指标。
20. production evidence chain 使用的 source 缺少机器可读 license、usage boundary
    或 attribution metadata。
21. backup/restore 没有 RTO/RPO、最近一次恢复演练证据，或恢复后没有重新运行
    RQA gate。
22. release automation 没有依赖、运行镜像和发布脚本的安全扫描或等价风险评估，
    或存在未分级的 critical/high 风险。
23. production-ready report 没有绑定 Runtime profile、prompt、tool policy、
    reviewer policy、model upstream 和 decoding 参数摘要，或 RQA eval 与 live gate
    使用的行为配置不一致。
24. RQA 保存了可关联用户、session、trace 或 package 的数据，但没有导出、
    删除/匿名化、retention、legal hold 和 audit tombstone 策略。

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
| 写入失败 | RQA 持久化和 audit 失败必须 fail-closed 或返回明确 degraded 状态 |
| 运维闭环 | backup、restore、retention、prune、live release 均纳入验收 |
| 隐私边界 | RQA 默认只保存脱敏摘要、hash、枚举和 bounded excerpt |
| API 契约 | admin API / Action 必须分页、限长、schema version 和兼容校验 |
| 发布值守 | runbook、alert、rollback、post-release monitor 都是 production blocker |
| 发布安全 | emergency disable / degraded mode 只能产生 non-production 状态 |
| Admin 访问 | RQA read/list/update 都必须鉴权、限流、审计和防枚举 |
| 发布溯源 | git commit、image digest、schema/eval/config digest 必须进入 release report |
| 资料覆盖 | source coverage boundary 必须进入 report、公共回答边界和 release gate |
| 强制执行 | CI 或 release automation 必须运行 RQA gate、validator 和 contract smoke |
| KB 绑定 | live KB、source snapshot、eval run 和 release report 必须同源可复现 |
| 来源许可 | production source 必须具备可核验 license / usage / attribution metadata |
| 恢复目标 | RTO/RPO、恢复演练和恢复后 gate 复核必须进入 production report |
| 供应链安全 | 依赖、运行镜像和发布脚本扫描必须进入 release automation |
| 行为配置 | profile、prompt、tool policy、reviewer policy、model upstream 必须绑定 |
| 数据生命周期 | RQA 用户相关数据必须有导出、删除/匿名化、保留和 tombstone 规则 |

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
| source_coverage_boundary_passed | 100% |
| admin_trace_quality_summary | 100% |

## Milestone A：Runtime 质量报告

状态：已完成（2026-05-15；runtime search 报告闭环已实现并通过单测，
但不代表整体 RQA production-ready）

目标：Runtime search tools 为每次检索生成结构化 `RetrievalQualityReport`。

- [x] 定义 `RetrievalQualityReport` Rust 类型和 JSON schema version。
- [x] 覆盖 `tonglingyu.text.search`。
- [x] 覆盖 `tonglingyu.commentary.search`。
- [x] 记录 query terms、protected terms、expanded aliases。
- [x] query terms 和 question 字段进入 report 前必须脱敏、截断并保留 hash。
- [x] report payload 有明确大小上限，超限时记录 truncated 标记。
- [x] 记录 required / actual / missing evidence types。
- [x] 记录 candidate / selected count 和 channel 分布。
- [x] 记录 source diversity、edition diversity、exact term coverage。
- [x] 记录 source coverage boundary，区分当前 source snapshot、影印件复核、
      权威校注本复核和专家校勘状态。
- [x] 记录 source license refs、usage boundary 和 attribution refs，不把缺失
      许可/署名 metadata 的 source 用作 production evidence。
- [x] 记录 expected evidence hit 结果。
- [x] 记录 quality status、issues、recommended follow-up。
- [x] 单测覆盖完整、缺证据、缺 required type、exact term missing。

节点总结：

- `agent-platform/crates/tonglingyu-runtime/src/lib.rs` 已新增
  `tonglingyu-rqa-report-v1`，`tonglingyu.text.search` 和
  `tonglingyu.commentary.search` 的 `EvidenceCards` 输出都会携带
  `quality_report`。
- report 已覆盖问题 hash、redacted terms、protected terms、expanded aliases、
  candidate/selected count、channel distribution、required/selected/missing
  evidence types、exact-match coverage、source/edition boundary、source usage refs、
  `expected_evidence_status` 和 `truncated`。
- production 语义已 fail-closed：无证据、缺 required type、protected exact term
  未命中时 `quality_status=failed`；source license/attribution metadata 缺失时
  `production_ready=false`。
- 验证命令：`cargo test -p tonglingyu-runtime`，32 个测试通过。
- 仍不能宣布整体 RQA production-ready：后续 `retrieval_failures`、eval gate、
  release gate、saved report validator、live KB 绑定、source metadata migration
  和 lifecycle gate 仍未完成。

## Milestone B：retrieval_failures schema

状态：未开始

目标：线上和 eval 的召回失败样本可追踪、可分派、可人工处理。

- [ ] 添加 additive runtime schema migration。
- [ ] 新增 `retrieval_failures` 表。
- [ ] 字段覆盖 failure id、trace id、package id、question summary/hash、kb version。
- [ ] 字段覆盖 failure type、redacted query terms、required / actual evidence types。
- [ ] 字段覆盖 expected / selected evidence ids、missing evidence types。
- [ ] 字段覆盖 quality issues、agent diagnosis、proposed fix。
- [ ] 字段覆盖 human review status、reviewer、note、created/resolved time。
- [ ] 添加必要索引：trace、package、status、failure type、created_at。
- [ ] 添加 Runtime store API：create/list/update/read。
- [ ] list API 默认分页，最大 page size 有硬上限。
- [ ] read/list 输出区分 admin detail 和 safe summary。
- [ ] schema migration 支持从现有生产 DB 升级，不重建 KB 或删除既有数据。
- [ ] migration preflight 输出 schema version 和待执行 migration，不输出 secret。
- [ ] migration 失败时不留下半初始化表或不一致 schema version。
- [ ] 单测覆盖迁移幂等、写入、读取、状态更新和失败回滚。

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
- [ ] failure 写入和 audit append 在同一可追溯操作中完成。
- [ ] RQA 持久化失败时请求不得被标记为完整成功。
- [ ] 单测覆盖所有触发条件、去重、写入失败和 audit 失败。

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
- [ ] eval case 覆盖需要影印件、权威校注或专家校勘才能确认的问题，并要求
      reviewer 降级为资料不足。
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
- [ ] Prometheus labels 只能使用 bounded enum，不允许 trace/user/question/package id。
- [ ] JSON metrics 只暴露聚合计数和 bounded histogram，不返回原始 query。
- [ ] admin-only API / Action 支持 list/read/update retrieval failure 人工状态。
- [ ] admin API / Action 输出带 RQA schema version，字段变更必须有兼容测试。
- [ ] admin list/read 响应大小有上限，超限必须分页或截断。
- [ ] admin list/read/update 都写访问审计，记录 actor、action、filter summary、
      page size、result count 和 trace id。
- [ ] admin list filter 和 sort 只能使用 allowlist 字段，未知字段 fail-closed。
- [ ] 直接枚举不存在或未授权 RQA id 时返回脱敏错误，不泄露内部存在性细节。
- [ ] RQA admin endpoints 继承或定义专用 rate limit / body limit。
- [ ] admin auth failure、role denial 和 rate-limit denial 都写脱敏 audit event。
- [ ] retrieval failure 人工状态更新写入 audit event。
- [ ] 普通用户不能读取或更新 retrieval failure。
- [ ] admin update 使用 version / updated_at compare-and-set，避免并发覆盖。
- [ ] 重复 admin update 使用 idempotency key 或等价机制去重。
- [ ] 普通 chat response 不暴露完整 quality report 或 failure 内部字段。
- [ ] streaming response 同样不泄露内部字段。
- [ ] strict Gateway gate 增加 admin trace quality summary 校验。
- [ ] 单测覆盖 admin 可见、admin update、并发冲突、重复更新、普通响应不可见、
      普通用户不可更新、admin read audit、filter allowlist、rate-limit denial。

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
- [ ] gate 校验 source coverage boundary 在 eval summary、quality report 和
      release report 中一致。
- [ ] gate 校验需要影印件、权威校注或专家校勘的问题没有被公共回答声明为已确认。
- [ ] gate 支持阈值配置，默认值采用本文 Production 默认阈值。
- [ ] 低于默认阈值的配置必须把报告标记为 non-production。
- [ ] gate report 记录实际阈值来源和有效阈值。
- [ ] gate report 记录 RQA schema version、eval suite version 和有效配置摘要。
- [ ] gate report 记录 source snapshot digest、KB build hash、kb_version 和
      eval run id。
- [ ] gate report 记录 source license summary 和 attribution summary。
- [ ] gate report 记录 Runtime profile digest、prompt digest、tool policy digest、
      reviewer policy digest、model upstream id 和 decoding 参数摘要。
- [ ] gate 校验 eval quality summary 与当前 live KB 的 kb_version / build hash 一致。
- [ ] gate 校验 production evidence chain 中的 source 都有 license / usage /
      attribution metadata；缺失时 fail-closed。
- [ ] gate 校验 RQA eval 使用的行为配置与 live gate 读取的行为配置一致。
- [ ] 缺少 quality summary、缺少 report 或阈值配置不可解析时 fail-closed。
- [ ] gate 输出不泄露 secret 和过长日志。
- [ ] gate stdout / stderr 不输出原始 question、query terms 或高基数 id 列表。
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
- [ ] validator 校验 RQA schema version、eval suite version 和有效配置摘要不缺失。
- [ ] validator 校验 source coverage boundary 不缺失，且不高于当前 source snapshot
      实际登记范围。
- [ ] validator 校验 source snapshot digest、KB build hash、kb_version 和 eval run id
      不缺失，且 RQA 指标绑定同一个 KB 构建。
- [ ] validator 校验 source license summary / attribution summary 不缺失，且引用的
      source id 都存在于当前 source snapshot metadata。
- [ ] validator 校验 Runtime profile / prompt / tool policy / reviewer policy /
      model upstream / decoding 参数摘要不缺失，且与 quality gate 输出一致。
- [ ] validator 校验低于默认阈值的报告不能 production-ready。
- [ ] validator 校验 report 不包含原始用户问题、未脱敏 query 或高基数字段列表。
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
- [ ] release automation 执行依赖、运行镜像和发布脚本安全扫描或等价风险评估。
- [ ] production-ready report 不允许依赖 mock gate command override。
- [ ] production-ready report 不允许使用低于默认阈值的 RQA 配置。
- [ ] production-ready report 必须包含 RQA quality gate 和治理任务 gate。
- [ ] production-ready report 必须包含 git commit、image digest、RQA schema version、
      eval suite version 和有效配置摘要。
- [ ] production-ready report 必须包含 source snapshot digest、KB build hash、
      kb_version 和 eval run id，且与 live gate 读取的当前 KB 一致。
- [ ] production-ready report 必须包含 source license summary 和 attribution
      summary，缺失 source 许可/署名 metadata 时不能生成 ready artifact。
- [ ] production-ready report 必须包含 Runtime profile digest、prompt digest、
      tool policy digest、reviewer policy digest、model upstream id 和 decoding
      参数摘要。
- [ ] production-ready report 中的行为配置摘要必须与 RQA eval、strict Gateway
      live gate 和 admin trace 摘要一致。
- [ ] CI 或 release automation 必须强制运行 RQA quality gate、saved report
      validator 和 contract smoke；失败时不能生成 production-ready artifact。
- [ ] 自动化产物必须记录 workflow/job id 或 release run id、触发 commit 和 gate
      结果摘要。
- [ ] 自动化产物必须记录安全扫描摘要、risk owner、accepted risk id 或空风险结论。
- [ ] `npx --yes markdownlint-cli2 docs/tonglingyu-agent-design/*.md`

节点总结：

- 待实现。

## Milestone J：运维、恢复和真实发布

状态：未开始

目标：RQA 不是只在测试库可用，必须能在生产环境升级、保留、恢复和复测。

- [ ] RQA 表纳入现有 backup / restore 演练。
- [ ] 定义 RQA 数据的 RTO 和 RPO 默认目标，并写入 release report。
- [ ] RQA 表纳入 retention / prune dry-run 和实际 prune 路径。
- [ ] prune 不删除仍被 open failure、open governance task 或 production report 引用的数据。
- [ ] retention / prune 与用户 delete/anonymize 策略一致，并保留 audit tombstone。
- [ ] restore 后 admin trace、failure、governance task 和 quality gate 可继续读取。
- [ ] restore 后必须重新运行 RQA quality gate 和 saved report validator。
- [ ] 最近一次恢复演练必须记录 started_at、finished_at、RTO/RPO 是否满足、
      operator、环境和恢复后 gate 结果。
- [ ] 生产 DB migration 前必须有备份路径和 schema preflight 输出。
- [ ] live release mode 必须生成真实 RQA quality gate，不接受 fixture-only report。
- [ ] production-ready report 必须绑定当前 live environment、generated_at 和有效期。
- [ ] production-ready report 必须绑定当前运行镜像 digest、代码版本和 migration 状态。
- [ ] production-ready report 必须绑定当前 live KB 的 source snapshot digest、
      KB build hash、kb_version 和 eval run id。
- [ ] production-ready report 必须绑定当前 live KB 的 source license summary 和
      attribution summary。
- [ ] production-ready report 必须包含 RTO/RPO、最近一次恢复演练证据和恢复后
      gate 结果。
- [ ] production-ready report 必须包含依赖/镜像/发布脚本安全扫描摘要或已审批
      risk exception。
- [ ] production-ready report 必须绑定当前 live Runtime profile、prompt、tool
      policy、reviewer policy、model upstream 和 decoding 参数摘要。
- [ ] production-ready report 必须包含 RQA 用户数据生命周期策略版本和最近一次
      lifecycle contract smoke 结果。
- [ ] live gate 必须验证 RQA admin Action/API 权限边界。
- [ ] live gate 必须验证 RQA metrics 和 Prometheus 不泄露 query 原文或 secret。
- [ ] RQA 写入、查询和 release gate 的耗时必须有 bounded timeout 或明确上限。
- [ ] admin list/read 必须覆盖分页、最大 page size、payload 截断和 schema version。
- [ ] performance smoke 记录 RQA 写入、admin 查询、release gate 的耗时摘要。
- [ ] release gate 在缺少性能摘要或超过默认预算时不能 production-ready。
- [ ] 单测或 smoke 覆盖 backup/restore、retention/prune、live report freshness。

节点总结：

- 待实现。

## Milestone K：隐私、契约和性能预算

状态：未开始

目标：RQA 诊断能力不能以泄露用户文本、无界 API 或不可观测性能开销为代价。

- [ ] RQA 数据模型区分 `question_summary`、`question_hash` 和 redacted excerpt。
- [ ] 默认不保存完整用户问题；如需诊断原文，必须另有显式受控配置和审计。
- [ ] redaction 覆盖疑似 key、token、URL secret、邮箱、手机号和长随机串。
- [ ] admin detail 只能返回 redacted 字段，不能返回完整隐私文本。
- [ ] 定义 RQA 用户数据生命周期：export、delete/anonymize、retention、legal hold
      和 audit tombstone。
- [ ] 删除或匿名化用户相关 RQA 数据时，不能破坏 production report、open failure、
      governance task 或审计历史的可追责性；必须用 tombstone 记录处理结果。
- [ ] 用户数据生命周期操作必须写 audit event，且输出不包含原始问题或 secret。
- [ ] 所有 RQA list API 必须分页、排序稳定、最大 page size 固定。
- [ ] RQA admin API / Action 响应包含 schema version 和 pagination metadata。
- [ ] API contract smoke 覆盖旧 report 兼容、新字段兼容和未知字段拒绝/忽略策略。
- [ ] Prometheus label set 固定且低基数。
- [ ] JSON metrics 不输出原始 query、完整 question、trace 列表或 package 列表。
- [ ] 定义 RQA 写入、admin 查询、release gate 默认性能预算。
- [ ] release report 记录性能预算、实际耗时和是否超限。
- [ ] 性能预算缺失或超限时，production-ready 必须失败。

节点总结：

- 待实现。

## Milestone L：发布值守、告警和回滚

状态：未开始

目标：RQA production-ready 必须能被 operator 接住，而不是只在发布瞬间通过。

- [ ] 在 deploy runbook 或专门文档中写明 RQA release 流程。
- [ ] runbook 覆盖 migration preflight、backup、deploy、live gate、saved report
      validation。
- [ ] runbook 覆盖回滚到上一镜像/配置的步骤。
- [ ] runbook 覆盖 DB restore 或 additive schema 保留后的降级处理。
- [ ] runbook 覆盖 RTO/RPO 目标、恢复步骤、恢复后 RQA gate 和 validator 复核。
- [ ] rollback 后必须重新运行 release readiness 或明确标记 non-production。
- [ ] 定义 RQA 写入失败率、admin API 5xx、admin API latency、open P0 failure、
      quality gate failure 的告警条件。
- [ ] 告警指标必须低基数且不包含 query、question、trace 或 package id。
- [ ] post-release 监控窗口至少覆盖一次真实 live gate 和一次 admin Action/API
      查询。
- [ ] post-release 监控记录 operator、时间、环境、报告路径和结论。
- [ ] production-ready report 必须引用 runbook / rollback / post-release 证据。
- [ ] runbook 必须说明如何按 release report 的 commit/image/config/KB/security
      摘要复现本次发布。
- [ ] release gate 或 saved report validator 缺少值守证据时不能
      production-ready。
- [ ] smoke 覆盖告警字段存在性、runbook ref、rollback evidence ref 和
      post-release monitor ref、RTO/RPO evidence ref、安全扫描 evidence ref。

节点总结：

- 待实现。

## Milestone M：事故响应、容量和审计完整性

状态：未开始

目标：事故或压力场景下仍保持可追责、可降级、可恢复，不能用关闭治理来伪装可用。

- [ ] 提供 RQA emergency disable 或 degraded mode 配置时，release report 必须标记
      non-production。
- [ ] RQA persistence degraded 时，公共响应必须暴露稳定错误/降级状态和 trace id，
      不能伪装成完整成功。
- [ ] RQA 写入不能使用无界内存队列；队列、batch 或 retry 必须有容量上限。
- [ ] RQA retry 必须可幂等，且不会重复创建 failure、governance task 或 audit event。
- [ ] 管理员状态更新必须保留历史记录：actor、reason、previous status、new status、
      timestamp。
- [ ] 不允许硬删除 open failure、open governance task 或相关 audit history。
- [ ] 事故 runbook 定义 severity、owner、first response、mitigation、rollback、
      recovery validation。
- [ ] 事故 runbook 定义 RTO/RPO breach 的升级路径和发布状态处理。
- [ ] capacity smoke 覆盖代表性 eval report 数量、failure 数量和 admin list 翻页。
- [ ] load / soak smoke 覆盖 RQA 写入、admin 查询、metrics 和 release gate 在默认预算内。
- [ ] release gate 缺少 capacity / incident / audit-history / RTO-RPO / security-scan
      证据时不能
      production-ready。
- [ ] saved report validator 校验 emergency disabled、capacity missing、audit history
      missing、RTO/RPO missing、security scan missing、data lifecycle missing 不能
      production-ready。

节点总结：

- 待实现。

## 提交节奏

1. Checklist 基线单独提交。
2. Milestone A-B 完成后提交 runtime schema/report 基础。
3. Milestone C-D 完成后提交 eval/failure 闭环。
4. Milestone E 完成后提交 admin trace/metrics 边界。
5. Milestone F-G 完成后提交 release gate 和 saved report validator。
6. Milestone H 完成后提交治理任务和反馈闭环。
7. Milestone I 完成后提交端到端验证。
8. Milestone J 完成后提交运维恢复和真实发布。
9. Milestone K 完成后提交隐私契约和性能预算。
10. Milestone L 完成后提交发布值守和回滚。
11. Milestone M 通过后提交最终 production-ready 验证更新。

每次提交前必须更新本 checklist 的状态和节点总结。
