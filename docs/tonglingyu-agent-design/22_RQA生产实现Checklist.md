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
| quality_report_production_ready | 100% expected-passed |
| eval case classification | 100% release case 标记 expected / not-applicable |
| expected evidence denominator | release suite 中必须大于 0 |
| expected_evidence_hit@8 | 有 expected evidence 标注的 case 必须 100% 命中 |
| required_type_coverage | 100% |
| exact_term_coverage | 有 protected term 的 case 必须 100% |
| source_boundary_confirmation_avoided | 100% |
| forbidden_conclusion_avoided | 100% |
| reviewer_status_matched | 100% |
| open P0 retrieval failures | 0 |
| public_response_boundary_passed | 100% |
| source_coverage_boundary_passed | 100% |
| admin_trace_quality_summary | 100% |

`quality_report_production_ready` 的分母是 expected-passed / evidence-bearing
eval report。expected downgrade case 只允许预期内 no-evidence 或
missing-required-type issue；source metadata、license、attribution 或其他非预期
quality issue 仍然 fail-closed。

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

状态：已完成（2026-05-15；schema、API、preflight、rollback 和基础自动登记已验证）

目标：线上和 eval 的召回失败样本可追踪、可分派、可人工处理。

- [x] 添加 additive runtime schema migration。
- [x] 新增 `retrieval_failures` 表。
- [x] 字段覆盖 failure id、trace id、package id、question summary/hash、kb version。
- [x] 字段覆盖 failure type、redacted query terms、required / actual evidence types。
- [x] 字段覆盖 expected / selected evidence ids、missing evidence types。
- [x] 字段覆盖 quality issues、agent diagnosis、proposed fix。
- [x] 字段覆盖 human review status、reviewer、note、created/resolved time。
- [x] 添加必要索引：trace、package、status、failure type、created_at。
- [x] 添加 Runtime store API：create/list/update/read。
- [x] list API 默认分页，最大 page size 有硬上限。
- [x] read/list 输出区分 admin detail 和 safe summary。
- [x] schema migration 支持从现有生产 DB 升级，不重建 KB 或删除既有数据。
- [x] migration preflight 输出 schema version 和待执行 migration，不输出 secret。
- [x] migration 失败时不留下半初始化表或不一致 schema version。
- [x] 单测覆盖迁移幂等、写入、读取、状态更新和失败回滚。

节点总结：

- `tonglingyu-runtime` 已新增 `tonglingyu-retrieval-failures-v1` runtime
  migration、`retrieval_failures` 表和 trace/package/status/type/created_at 索引。
- 已新增 create/list/read/update API；list 有默认 page size 和最大 page size，
  read/list 支持 `admin_detail` 与 `safe_summary`，safe summary 不输出 trace、
  package 或 selected evidence ids。
- schema preflight 会列出 required/applied/pending migrations，声明不重建 KB、
  不删除 runtime data、不包含 secret；autocommit 初始化会在 schema apply 失败时
  rollback，避免留下半初始化 migration 状态。
- workflow 会在 `RetrievalQualityReport.production_ready=false` 时写入
  retrieval failure，并追加 `retrieval_failure_recorded` audit event；状态更新会
  写 `retrieval_failure_status_updated`，audit 中只记录 review note hash。
- 验证命令：`cargo test -p tonglingyu-runtime`；当前 runtime 回归套件已扩展到
  42 个测试并通过。
- 仍不能宣布整体 RQA production-ready：Milestone C 的完整触发矩阵、去重、
  eval expected evidence、release gate 和 saved report validator 仍未完成。

## Milestone C：自动 failure 记录

状态：已完成（2026-05-15；runtime/API 触发矩阵已验证，eval 指标化仍属
Milestone D）

目标：Runtime 和 eval 在生产规则下自动记录 P0/P1 召回失败。

- [x] 候选数为 0 时记录 failure。
- [x] required evidence type 缺失时记录 failure。
- [x] expected evidence 未命中时记录 failure。
- [x] exact protected term 未命中时记录 failure。
- [x] reviewer 因证据不足降级时记录 failure。
- [x] package 无法支持关键 claim 时记录 failure。
- [x] 去重相同 trace/package/failure type，避免重复刷表。
- [x] failure 记录写入 audit event。
- [x] failure 写入和 audit append 在同一可追溯操作中完成。
- [x] RQA 持久化失败时请求不得被标记为完整成功。
- [x] 单测覆盖所有触发条件、去重、写入失败和 audit 失败。

节点总结：

- `RetrievalQualityReport.production_ready=false` 会在 workflow 中自动登记
  failure；候选数为 0、required type 缺失、exact protected term 未命中和
  source usage metadata 缺失都会进入 `retrieval_failures`。
- expected evidence miss 可通过 `RetrievalFailureCreateInput.expected_evidence_ids`
  与 selected evidence ids 计算并登记，允许 eval/gate 调用方复用同一失败表。
- reviewer downgrade 会登记 `reviewer_evidence_insufficient`，覆盖证据不足或
  package 无法支持关键 claim 的本地 reviewer 结论。
- 相同 trace/package/failure type 通过 dedupe 查询和唯一索引去重；failure insert
  与 audit append 在同一事务中完成，audit append 失败会 rollback failure。
- 验证命令：`cargo test -p tonglingyu-runtime`；当前 runtime 回归套件已扩展到
  42 个测试并通过。
- 仍不能宣布整体 RQA production-ready：Milestone D 的 eval quality metrics、
  Milestone F 的 release quality gate 和 Milestone G 的 saved report validator
  仍未完成。

## Milestone D：eval 质量指标

状态：已完成代码切片（2026-05-15；当前 live KB 因 source usage metadata
缺失仍 fail-closed，不能声明 RQA production-ready）

目标：eval 从 pass/fail 扩展为生产质量指标。

- [x] 扩展 eval case，支持 expected evidence ids / block ids。
- [x] 所有 release eval case 必须标记 expected evidence 或 not-applicable reason。
- [x] expected evidence 分母为 0 时 release quality gate fail-closed。
- [x] 增加 expected_evidence_hit@1。
- [x] 增加 expected_evidence_hit@3。
- [x] 增加 expected_evidence_hit@8。
- [x] 增加 required_type_coverage。
- [x] 增加 exact_term_coverage。
- [x] 增加 source_diversity。
- [x] 增加 edition_diversity。
- [x] source / edition diversity 标明当前只覆盖 Wikisource source snapshot，
      不等同于影印或权威校注本复核。
- [x] eval case 覆盖需要影印件、权威校注或专家校勘才能确认的问题，并要求
      reviewer 降级为资料不足。
- [x] 增加 forbidden_conclusion_avoided。
- [x] 增加 reviewer_status_matched。
- [x] 无 expected evidence 标注的 case 不计入 hit@k 分母。
- [x] eval report 输出 quality summary 和 case-level quality details。
- [x] eval failure 自动写入 `retrieval_failures`。
- [x] 单测覆盖有标注、无标注、阈值达标和阈值失败。

节点总结：

- `tonglingyu-gateway eval` 已输出 `tonglingyu-eval-quality-v1`
  `quality_summary` 和 case-level `quality`：包含 expected evidence
  classification、hit@1/@3/@8、required type、exact term、source/edition
  diversity、source coverage boundary、forbidden conclusion 和 reviewer status。
- expected evidence hit@k 采用严格口径：一个 case 若标注多个 expected block，
  必须全部在 top-k 中命中才计为通过；无 expected 标注的 case 只进入普通 eval
  和 classification，不进入 hit@k 分母。
- eval case 显式携带 expected ids/block ids 或 not-applicable reason；新增未分类
  case 不再被默认兜底为 not-applicable。
- 新增影印件、权威校注本、专家校勘边界 case；Runtime reviewer 对这类确认性
  问题降级为资料不足，防止把当前 Wikisource snapshot 包装成校勘完成。
- eval case 失败会通过 `retrieval_failures` API 写入失败样本；当前本地
  live eval 的本轮 `eval_failure_records=0`。
- 验证命令：
  `cargo test -p tonglingyu-runtime`（42 个测试通过）；
  `cargo test -p tonglingyu-gateway`（21 个测试通过）；
  `cargo run -p tonglingyu-gateway -- eval`，参数为
  `--db ../data/tonglingyu/tonglingyu.db`、`--limit 8` 和
  `--report /tmp/tonglingyu-eval-quality-check.json`，当前按 production 口径
  返回通过。
- 当前 eval 结果：103/103 case 通过，103/103 case 生成 quality report，
  expected evidence 分母为 5，expected_evidence_hit@8 为 5/5；
  expected-passed / evidence-bearing report 的
  `quality_report_production_ready=86/86`，required type、exact term、
  forbidden conclusion 和 reviewer status 均为 100%，blocker 为空。
- 已补齐 source snapshot 机器可读 `license`、`license_url`、
  `license_source_url`、`attribution` 和 `usage_boundary`，并修正版本边界、
  程乙正文、脂批原文问题的检索 / reviewer 策略差异；这只关闭 Milestone D
  的 eval quality blocker，不代表 Milestone E-G 和 release artifact 已完成。

## Milestone E：admin trace 和 metrics

状态：已完成代码切片（2026-05-15；admin surface、metrics、安全边界和
endpoint-level 测试已闭合；整体 RQA production-ready 仍取决于 Milestone F-G
release gate / saved report validator，以及 H-J 的治理、自动化和运维闭环）

目标：管理员能从 trace/package/session 看到召回质量摘要和 failure 状态。

- [x] admin trace 顶层暴露 retrieval quality summary。
- [x] admin trace 关联 retrieval failure ids。
- [x] admin package audit 可看到 quality issue 摘要。
- [x] JSON metrics 暴露 retrieval failure count by status/type。
- [x] Prometheus 暴露 RQA failure 有界指标，不暴露 query 原文和 secret。
- [x] Prometheus labels 只能使用 bounded enum，不允许 trace/user/question/package id。
- [x] JSON metrics 只暴露聚合计数和 bounded histogram，不返回原始 query。
- [x] admin-only API / Action 支持 list/read/update retrieval failure 人工状态。
- [x] admin API / Action 输出带 RQA schema version，字段变更必须有兼容测试。
- [x] admin list/read 响应大小有上限，超限必须分页或截断。
- [x] admin list/read/update 都写访问审计，记录 actor、action、filter summary、
      page size、result count 和 trace id。
- [x] admin list filter 和 sort 只能使用 allowlist 字段，未知字段 fail-closed。
- [x] 直接枚举不存在或未授权 RQA id 时返回脱敏错误，不泄露内部存在性细节。
- [x] RQA admin endpoints 继承或定义专用 rate limit / body limit。
- [x] admin auth failure、role denial 和 rate-limit denial 都写脱敏 audit event。
- [x] retrieval failure 人工状态更新写入 audit event。
- [x] 普通用户不能读取或更新 retrieval failure。
- [x] admin update 使用 version / updated_at compare-and-set，避免并发覆盖。
- [x] 重复 admin update 使用 idempotency key 或等价机制去重。
- [x] 普通 chat response 不暴露完整 quality report 或 failure 内部字段。
- [x] streaming response 同样不泄露内部字段。
- [x] strict Gateway gate 增加 admin trace quality summary 校验。
- [x] 单测覆盖 admin 可见、admin update、并发冲突、重复更新、普通响应不可见、
      普通用户不可更新、admin read audit、filter allowlist、rate-limit denial。

节点总结：

- Gateway admin trace 现在返回 `retrieval_quality_summary`、
  `retrieval_failure_ids` 和 admin detail failures；admin package audit 也返回同
  一组 RQA 摘要，便于从 package 追到 failure 状态。
- `/v1/admin/metrics` 新增 `rqa.retrieval_failures.total/by_status/by_type`；
  Prometheus 新增 `tonglingyu_retrieval_failures_total`、
  `tonglingyu_retrieval_failures_by_status_total` 和
  `tonglingyu_retrieval_failures_by_type_total`，status/type label 只输出 allowlist
  枚举或 `other`；Gateway info、review status、audit event type 也只保留 bounded
  label，避免 trace/user/question/package 等高基数字段进入 Prometheus。
- 新增 `/v1/admin/retrieval-failures` list、
  `/v1/admin/retrieval-failures/{failure_id}` read/update；Open WebUI admin Action
  新增 retrieval failure list/read/update 入口。
- admin list/read/update 全路径写 admin access audit；not-found 和 conflict 返回
  脱敏错误并写审计，not-found 使用 id hash，成功 read/update 记录 trace id。
- admin endpoints 使用独立 admin rate limiter 并继承 Gateway body limit；auth
  failure、rate-limit denial 和 Open WebUI Action role denial 都写
  `rqa_admin_access_denied`，subject 只落 hashed ref。
- admin update 支持 `if_match_updated_at` 冲突检测；重复同 payload 且未带 CAS
  时 runtime update no-op，不重复写 `retrieval_failure_status_updated`，但保留
  admin update attempt audit。
- 普通 completion 和 streaming completion 已覆盖 RQA 内部字段负向测试，不输出
  `retrieval_failures`、`retrieval_quality_summary` 或 `quality_report`。
- 验证命令：
  `cargo test -p tonglingyu-runtime`（42 个测试通过）；
  `cargo test -p tonglingyu-gateway`（36 个测试通过）；
  `python3 -m unittest deploy/open-webui/functions/test_tonglingyu_gateway_admin_action.py`
  （10 个测试通过）。
- 仍不能宣布整体 RQA production-ready：Milestone F 的 release quality gate、
  Milestone G 的 saved report validator、Milestone H 的治理任务、Milestone I 的
  自动化强制执行和 Milestone J 的运维/恢复闭环尚未完成。

## Milestone F：release quality gate

状态：完成（2026-05-16；release readiness 已接入 canonical RQA quality gate，
gate 输出已绑定可复核 eval artifact、production 阈值配置和 strict live gate
行为配置 fingerprint；旧 eval artifact 已审计关闭后，本地 preflight
`retrieval_quality` 可在 open P0 为 0 时通过）

目标：召回质量指标进入 release readiness，并可作为 production blocker。

- [x] 新增或扩展 release readiness quality gate。
- [x] gate 读取 eval quality summary。
- [x] gate 校验 expected_evidence_hit@8 阈值。
- [x] gate 校验 eval case classification 为 100%。
- [x] gate 校验 expected evidence denominator 大于 0。
- [x] gate 校验 required_type_coverage 阈值。
- [x] gate 校验 forbidden_conclusion_avoided。
- [x] gate 校验 reviewer_status_matched。
- [x] gate 校验 open P0 retrieval failures 为 0。
- [x] gate 校验 quality report coverage 为 100%。
- [x] gate 校验 source coverage boundary 在 eval summary、quality report 和
      release report 中一致。
- [x] gate 校验需要影印件、权威校注或专家校勘的问题没有被公共回答声明为已确认。
- [x] gate 支持阈值配置，默认值采用本文 Production 默认阈值。
- [x] 低于默认阈值的配置必须把报告标记为 non-production。
- [x] gate report 记录实际阈值来源和有效阈值。
- [x] gate report 记录 RQA schema version、eval suite version 和有效配置摘要。
- [x] gate report 记录 source snapshot digest、KB build hash、kb_version 和
      eval run id。
- [x] gate report 记录 source license summary 和 attribution summary。
- [x] gate report 记录 Runtime profile digest、prompt digest、tool policy digest、
      reviewer policy digest、model upstream id 和 decoding 参数摘要。
- [x] gate 校验 eval quality summary 与当前 live KB 的 kb_version / build hash 一致。
- [x] gate 校验 production evidence chain 中的 source 都有 license / usage /
      attribution metadata；缺失时 fail-closed。
- [x] gate 校验 RQA eval 使用的行为配置与 live gate 读取的行为配置一致。
- [x] 缺少 quality summary、缺少 report 或阈值配置不可解析时 fail-closed。
- [x] gate 输出不泄露 secret 和过长日志。
- [x] gate stdout / stderr 不输出原始 question、query terms 或高基数 id 列表。
- [x] release readiness report 包含 quality gate record。
- [x] production-ready 时 quality gate 必须 passed。
- [x] contract smoke 覆盖 passed、threshold failed、open P0 failure、missing report。

节点总结：

- 新增 `deploy/scripts/verify-tonglingyu-rqa-quality-gate.sh`。默认会运行
  `tonglingyu-gateway eval` 生成 eval report；也可用
  `TONGLINGYU_RQA_EVAL_REPORT_PATH` 验证既有 report，但 production-ready
  saved report validator 要求 ready artifact 绑定 gate 生成的 eval report。
- `deploy/scripts/verify-tonglingyu-release-readiness.sh` 已把
  `retrieval_quality` 加入 required gate，release report 会保存 gate stdout
  中的 RQA schema version、eval suite version、eval run id、source snapshot
  digest、KB build hash、kb_version、source license/attribution summary、行为配置
  digest、有效阈值和 `eval_report_path`。当 release report path 已设置且未显式
  指定 eval report path 时，release readiness 会为真实 RQA gate 生成同目录
  `.rqa-eval.json` artifact，避免 production-ready report 只能引用临时文件。
- gate 使用 Production 默认阈值：quality report coverage、expected evidence
  hit@8、required type、exact term、source-boundary confirmation avoided、
  forbidden conclusion、reviewer status 均需 100%，expected evidence denominator
  必须大于 0，open retrieval failures 必须为 0。`TONGLINGYU_RQA_THRESHOLD_*`
  可设置更严格阈值；低于 production 默认值或不可解析时 gate fail-closed，并在
  `threshold_config` 中记录来源、覆盖项和 non-production 原因。
- 2026-05-15 本地 DB 曾因旧 eval artifact 存在 open P0 retrieval failures /
  governance tasks 而使 RQA quality gate 正确 fail-closed；2026-05-16 执行
  eval artifact remediation 后复跑 preflight，`open_p0_retrieval_failures=0`、
  `open_p0_governance_tasks=0`，`retrieval_quality` 已通过。这证明 gate 会按真实
  治理状态阻断或放行；目标 live DB 仍必须单独复核。
- contract smoke 已覆盖 synthetic passed report、降阈值篡改、open P0 failure
  篡改、RQA gate stdout 缺失和 eval artifact 缺失。真实 eval 也已在 `/tmp`
  复制 DB 上验证会输出 `expected_review_status`、`required_evidence_type`、
  `required_type_required`、`source_boundary_confirmation_required` 和
  `source_boundary_confirmation_avoided`，供 saved validator 重算。
- `deploy/scripts/verify-tonglingyu-strict-gateway.sh` 和 RQA quality gate 输出同一
  `behavior_config_digest`；saved report validator 会逐字段比较 RQA eval gate 与
  strict live gate 的 Runtime profile、prompt、tool policy、reviewer policy、model
  upstream 和 decoding 参数摘要。
- Milestone F/G 的 release gate 和 saved report validator 实现已完成；仍不能
  宣布整体 RQA production-ready，因为目标 live 环境、Open WebUI browser review、
  生产镜像 scan、live/load、post-release monitor 和事故容量证据尚未闭环。

## Milestone G：saved report validator

状态：完成（2026-05-15；validator 已接入 retrieval quality canonical gate，
并能从 eval artifact 重算 RQA gate 派生字段、校验 artifact hash/run id 和
扫描 release report 隐私/高基数字段）

目标：保存后的 release report 不能通过手改绕过质量 gate。

- [x] validator 要求 canonical gate set 包含 retrieval quality gate。
- [x] validator 重算 quality gate 派生字段。
- [x] validator 校验 production-ready report 必须 quality gate passed。
- [x] validator 校验 quality gate stdout JSON schema。
- [x] validator 校验 threshold / blocker / ready flag 不漂移。
- [x] validator 校验 effective thresholds 与 report 中 gate 输出一致。
- [x] validator 校验 RQA schema version、eval suite version 和有效配置摘要不缺失。
- [x] validator 校验 source coverage boundary 不缺失，且不高于当前 source snapshot
      实际登记范围。
- [x] validator 校验 source snapshot digest、KB build hash、kb_version 和 eval run id
      不缺失，且 RQA 指标绑定同一个 KB 构建。
- [x] validator 校验 source license summary / attribution summary 不缺失，且引用的
      source id 都存在于当前 source snapshot metadata。
- [x] validator 校验 Runtime profile / prompt / tool policy / reviewer policy /
      model upstream / decoding 参数摘要不缺失，且与 quality gate 输出一致。
- [x] validator 校验低于默认阈值的报告不能 production-ready。
- [x] validator 校验 report 不包含原始用户问题、未脱敏 query 或高基数字段列表。
- [x] validator 继续扫描 secret-like values。
- [x] contract smoke 覆盖删除 gate、篡改 passed、篡改 ready flag。

节点总结：

- `verify-tonglingyu-release-readiness-report.sh` 的 canonical gate set 已新增
  `retrieval_quality`，production-ready report 若删除该 gate、移除 stdout
  success JSON、降低默认阈值、缺少 source/KB digest、缺少行为配置摘要、
  RQA gate 未通过、eval artifact 不可读或 artifact hash/run id 不匹配，都会
  验证失败。
- validator 会读取 `eval_report_path`，校验 `eval_report_sha256`，并从原始
  eval cases 重算 quality report coverage、production-ready quality report
  coverage、eval classification、expected_evidence_hit@1/@3/@8、required
  type coverage、exact term coverage、forbidden conclusion avoided、reviewer
  status matched、eval failure records 和 source diversity，再与 gate stdout
  逐字段比较。
- release report 专门扫描 raw question/query/prompt 字段、stdout JSON 字符串
  泄露以及 trace/package/evidence/block/case/user 等高基数 id 列表；contract
  smoke 已覆盖 RQA gate stdout 缺失、threshold tamper、open P0 tamper、eval
  artifact 缺失、summary tamper、privacy leak 和 high-cardinality list leak。

## Milestone H：治理任务和反馈闭环

状态：已完成（2026-05-16；治理任务 schema、通用 source entity、failure-to-task、
普通用户反馈、retrieval failure 聚类、knowledge patch proposal、accepted patch
application、KB diff/eval diff、admin API/Action 和 release gate blocker 已完成并
验证；不代表整体 RQA production-ready）

目标：RQA-6 到 RQA-12 不停留在报告层，必须进入可审计的治理任务流。

- [x] 新增 `knowledge_governance_tasks` 或等价持久化任务 schema。
- [x] 支持从 retrieval failure 生成治理任务。
- [x] 支持管理员把 trace / package / failure 标记为待专家复核。
- [x] 支持普通用户反馈生成 retrieval failure 候选或治理任务。
- [x] 普通用户反馈不能直接修改 source、alias、term、commentary link 或事实层。
- [x] Agent 聚类 retrieval failures，并只生成 proposed fix。
- [x] proposed alias / term / commentary link / version note 必须进入人工状态流转。
- [x] accepted fix 必须绑定 reviewer、note、source 或 evidence ref。
- [x] KB rebuild 后生成 kb_version diff report。
- [x] kb_version diff report 对比前后 eval quality summary。
- [x] accepted knowledge patch proposal 必须能进入 KB rebuild 输入，并由 diff/eval
      gate 证明事实层变更。
- [x] release gate 可读取 open P0 governance tasks 并作为 blocker。
- [x] 单测覆盖任务创建、状态流转、权限边界、Agent 建议不可直接采纳。

节点总结：

- `tonglingyu-runtime` 新增
  `tonglingyu-knowledge-governance-tasks-v2`，包含
  `knowledge_governance_tasks` 表、通用 `source_entity_type/source_entity_id`、
  failure/type 与 entity/type 唯一索引、trace/package/status/type/priority 索引、
  existing open/in_review failure backfill、failure 创建时自动生成 open P0
  governance task，以及 create/list/read/update API。
- Governance task 只保存 proposed fix 和人工状态，不改写 source、alias、term、
  commentary link 或事实层；`accepted` 必须带 reviewer、review note 和 evidence
  ref，`closed/rejected` 必须带 reviewer 和 review note。
- Gateway 新增 admin-only governance task list/read/create/update 和
  create-from-failure API；管理员可把 trace、package 或 retrieval failure 标记为
  expert-review 任务。Open WebUI admin Action 暴露同等入口；trace/package audit 会
  返回 `governance_task_ids` 和 `governance_tasks`。访问、not-found、update 和冲突
  路径均写 admin audit。
- Gateway 新增普通用户 `/v1/feedback` 入口和 Open WebUI feedback Action；反馈必须
  绑定用户可访问的 trace 或 package，只生成 `source_entity_type=user_feedback` 的
  `expert_review` governance task，并写 `user_feedback_received` audit，不接受
  source/alias/term/commentary/fact mutation 字段。
- Runtime 新增 `tonglingyu-retrieval-failure-clusters-v1` 聚类结果：按 failure
  type、KB、missing/required evidence types 和 issue family 聚合 open/in_review
  retrieval failures，只生成 `source_entity_type=retrieval_failure_cluster` 的
  governance task proposed fix，不改 retrieval failure 状态或 source/alias/term/
  commentary/fact 表。Gateway admin API 和 Open WebUI admin Action 新增
  retrieval failure cluster 触发入口，并写 `retrieval_failures_clustered` 与
  `retrieval_failure_admin_cluster` audit。
- Runtime 新增 `tonglingyu-knowledge-patch-proposals-v1`：alias、term、
  commentary link 和 version note 建议先写入 `knowledge_patch_proposals`，
  再生成 `source_entity_type=knowledge_patch_proposal` 的 governance task。
  proposal payload 有类型化必填字段、大小上限、payload hash、source ref 和
  trace/package 绑定；Gateway admin API 与 Open WebUI admin Action 新增创建
  入口，并写 `knowledge_patch_proposal_created` 与
  `knowledge_patch_proposal_admin_create` audit。accepted/rejected 仍只更新人工
  状态，不直接写 source、alias、term、commentary link、version note 或事实层。
- Runtime 新增 `tonglingyu-kb-version-diff-v1` 和 `kb_version_diff_reports`：
  rebuild 会记录 before/after KB summary、source hash 变化、count delta、
  source snapshot digest 和 KB build hash。Gateway `build-kb` 默认在临时 SQLite
  副本上运行 rebuild 前后 eval，只把 `quality_summary` 写回 diff report，不把 eval
  package/failure 污染 live DB；post-rebuild eval 不通过时 build fail-closed。
- Accepted knowledge patch proposal 已接入 KB rebuild 输入：rebuild 只应用
  `accepted` 且带 evidence ref 的 proposal，按类型写入 aliases、terms、
  commentary_links 或 version_notes，并记录 `knowledge_patch_applications` 与
  `knowledge_patch_proposals_applied` audit；不满足目标表约束或引用不存在时
  rebuild fail-closed。
- RQA quality gate 新增 `open_p0_governance_tasks` Production 默认阈值 0；saved
  report validator 和 release contract smoke 会拒绝 open P0 governance task tamper。
- H 已完成，但仍不能宣布整体 RQA production-ready：Milestone I-J-K 的端到端
  自动化、用户数据 lifecycle、live release 证据、安全扫描 artifact 和容量/值守
  仍未完成；accepted 状态本身仍不能直接等同为事实层已更新，必须经过 rebuild
  application、diff report 和 eval gate。

## Milestone I：端到端验证

状态：进行中

目标：本地、容器、release contract 全部验证通过。

- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime`
- [x] `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
<!-- markdownlint-disable MD013 -->
- [x] `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime --all-targets -- -D warnings`
- [x] `cargo clippy --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway --all-targets -- -D warnings`
<!-- markdownlint-enable MD013 -->
- [x] `agent-platform/scripts/tonglingyu-gateway-smoke.sh`
- [x] `deploy/scripts/test-tonglingyu-release-readiness-contract.sh`
- [x] `deploy/scripts/verify-tonglingyu-rqa-backup-restore-drill.sh`
- [x] `deploy/scripts/verify-tonglingyu-rqa-api-contract.sh`
- [x] `deploy/scripts/verify-tonglingyu-rqa-performance-budget.sh`
- [x] `deploy/scripts/verify-tonglingyu-rqa-user-lifecycle.sh`
- [x] `deploy/scripts/verify-tonglingyu-release-security.sh`
- [x] `deploy/scripts/remediate-tonglingyu-rqa-eval-artifacts.sh` dry-run/apply
      contract
- [ ] `deploy/scripts/verify-tonglingyu-release-readiness.sh`
- [x] `deploy/scripts/verify-tonglingyu-rqa-release-automation.sh` fail-closed path
- [x] `deploy/scripts/verify-tonglingyu-release-readiness-report.sh` fixture path
- [x] `deploy/scripts/test-openwebui-gateway-admin-action-contract.sh`
- [x] release automation 执行依赖、运行镜像和发布脚本安全扫描或等价风险评估。
- [x] production-ready report 不允许依赖 mock gate command override。
- [x] production-ready report 不允许使用低于默认阈值的 RQA 配置。
- [x] production-ready report 必须包含 RQA quality gate 和治理任务 gate。
- [x] production-ready report 必须包含 git commit、image digest、RQA schema version、
      eval suite version 和有效配置摘要。
- [x] production-ready report 必须包含 source snapshot digest、KB build hash、
      kb_version 和 eval run id，且与 live gate 读取的当前 KB 一致。
- [x] production-ready report 必须包含 source license summary 和 attribution
      summary，缺失 source 许可/署名 metadata 时不能生成 ready artifact。
- [x] production-ready report 必须包含 Runtime profile digest、prompt digest、
      tool policy digest、reviewer policy digest、model upstream id 和 decoding
      参数摘要。
- [x] production-ready report 中的行为配置摘要必须与 RQA eval、strict Gateway
      live gate 和 admin trace 摘要一致。
- [x] CI 或 release automation 必须强制运行 RQA quality gate、saved report
      validator 和 contract smoke；失败时不能生成 production-ready artifact。
- [x] 自动化产物必须记录 workflow/job id 或 release run id、触发 commit 和 gate
      结果摘要。
- [x] 自动化产物必须默认持久保存 automation report、release readiness report
      和 saved report validator JSON；production-ready 不能依赖临时工作目录证据。
- [x] 自动化产物必须记录安全扫描摘要、risk owner、accepted risk id 或空风险结论。
- [x] `npx --yes markdownlint-cli2 docs/tonglingyu-agent-design/*.md`

节点总结：

- 2026-05-16 release readiness contract 已通过；此前本地 RQA backup/restore
  drill 通过过一次。持久 artifact 加固后重新跑真实脚本路径时，恢复演练已能产出
  持久 `backup.db`，但当前本地 fixture/existing_refs 路径在恢复后 RQA eval 阶段因
  5 个 eval case 失败而 fail-closed，不能再把本地 restore drill 当作当前通过证据。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-release-security.sh` 并接入
  release readiness 必跑 gate。该 gate 会记录 dependency scan、image scan、
  release script static scan、risk acceptance、risk owner / accepted risk id
  或空风险结论；缺依赖/镜像扫描、镜像 digest、可变 tag 或未审批风险时
  fail-closed。saved report validator 已覆盖缺 gate stdout、缺 scan 且无 risk
  acceptance、release script finding 等篡改。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-rqa-performance-budget.sh`
  并接入 release readiness 必跑 gate。该 gate 会真实启动本地 Gateway，执行
  chat 写入 RQA failure/task、admin trace/list、admin 状态关闭和 RQA quality gate
  复跑；默认预算覆盖 RQA 写入、admin 查询、状态更新和 quality gate，总输出只保留
  hash ref，不输出原始 query/trace/package id。contract smoke 已覆盖缺 gate stdout、
  预算超限和关键检查失败的 saved report 篡改。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-rqa-api-contract.sh`
  并接入 release readiness 必跑 gate。该 gate 会真实启动本地 Gateway，验证
  retrieval failure 与 governance task 的 admin list/read schema version、
  pagination metadata、max page size clamp、未知 filter 和非法 enum filter 的 400
  边界，以及 admin payload 不返回完整原始 prompt。saved report validator 已覆盖
  缺 gate stdout、contract check 失败和负向状态码不是 400 的篡改。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-rqa-user-lifecycle.sh`
  并接入 release readiness 必跑 gate。该 gate 会真实启动本地 Gateway，验证
  export 脱敏 manifest、legal hold、legal hold 阻断 anonymize、release legal hold、
  anonymize、audit event、tombstone、原始用户值移除和 trace/package/failure/task
  可追责性；gate stdout 只保留计数和 hash ref，不输出原始 question、response、
  user_ref、chat_ref 或 secret。saved report validator 已覆盖缺 gate stdout、
  关键 check 失败和 action status drift 的篡改。
- 2026-05-16 已把 `deploy/scripts/test-openwebui-gateway-admin-action-contract.sh`
  接入 release readiness 必跑 gate `openwebui_admin_action_contract`。该 gate
  会编译 Open WebUI Admin/Feedback Action，运行 21 个 Action 单测，验证 admin
  role guard、必需 valves、空 admin key fail-closed、admin action endpoint
  覆盖和输出不泄露 fixture secret；saved report validator 已覆盖缺 gate stdout、
  guard check 失败和 required Action 覆盖被篡改。
- 这只覆盖 release contract、恢复演练和安全门禁切片；真实 live release
  readiness、生产镜像 scan artifact 和 live/load 性能证据仍未完成。
- 2026-05-16 已补跑 runtime/gateway cargo test、clippy 和 gateway smoke；这些
  本地工程回归通过。
- 2026-05-16 已修复 backup/restore drill 的嵌套 release report 边界，使其显式
  处理后续新增的 performance、API、lifecycle、security、ops、incident/capacity
  和 Open WebUI admin Action contract gates；contract 已覆盖该边界。当前真实脚本
  路径仍受恢复后 RQA eval 质量 blocker 影响，不能声明 restore drill 当前通过。
- 2026-05-16 已补齐 tonglingyu 设计文档 markdownlint：表格分隔行已统一为
  markdownlint 可识别格式，同名小节规则改为同一父级内不重复，保留四 profile
  设计文档的结构化“职责/输入/输出/禁止”写法。
- 2026-05-16 已修正 RQA quality gate 的 eval 运行边界：gate 生成 eval report
  时先创建 SQLite snapshot，并在 snapshot 上跑 eval；live DB 只用于检查发布前
  真实 open P0 failure/task，避免 release eval 的负向用例污染生产 RQA 队列。
- 2026-05-16 已新增 release automation wrapper
  `deploy/scripts/verify-tonglingyu-rqa-release-automation.sh`，强制串联
  release readiness contract smoke、release readiness report 和 saved report
  validator；输出 run id、git commit、gate summary 和 artifact hash，当前因
  release readiness 阻塞按预期 fail-closed，不能生成 production-ready 结论。
- 2026-05-16 release automation wrapper 已改为默认写入
  `data/tonglingyu/release-artifacts/<run_id>/`，持久保存 release readiness
  report、automation report 和 saved report validator JSON；production-ready
  结论会要求 release/validator/automation 证据不在脚本临时工作目录内。
- 2026-05-16 release readiness report 已新增 `tonglingyu.release_manifest`：
  记录 git commit / tracked dirty 状态、runtime config digest、RQA schema /
  eval suite / eval run、source snapshot digest、KB build hash、kb_version、
  source license summary digest、behavior config digest、model upstream、
  decoding 参数摘要、dependency scan hash、digest-pinned image refs 和 per-image
  scan report digest。saved report validator 会重算 `release_manifest_digest`，
  并逐项反查 manifest 与 gate stdout 是否一致；manifest source snapshot 或
  digest 被篡改时 contract smoke 会 fail-closed。
- 2026-05-16 strict Gateway live gate 已新增 `behavior_config_binding`：将
  `behavior_config_digest` 与普通 chat admin trace、streaming admin trace 的
  `agent_runtime_summary` digest 绑定，并要求 summary 显示 Hermes content
  execution complete、local governance enforced、工具结果和工具审计事件计数匹配。
  saved report validator 会拒绝缺 binding 或 binding 与 strict Gateway 行为配置
  不一致的 production-ready report。
- 2026-05-16 release readiness report 已新增
  `tonglingyu.release_artifact_registry`：按
  `tonglingyu-release-artifact-registry-v1` 记录 release manifest、runtime
  config、RQA eval report、source license summary、behavior config、dependency
  scan、image inventory、image scan reports 和 browser review evidence 的
  digest/source gate/ref/path、365 天保留策略和 legal hold 支持。saved report
  validator 会重算 registry digest 并拒绝 production-ready report 缺关键 registry
  entry；release automation artifact 已记录 registry digest 和 entry count。
- 2026-05-16 已新增 `remediate-tonglingyu-rqa-eval-artifacts.sh`，默认 dry-run，
  只选择 `eval-tly-*` trace 的旧 eval artifact；apply 前备份 DB，事务内关闭
  retrieval failure / governance task，并写 status-history audit。已在临时 DB
  验证 apply 逻辑，并对本地默认 RQA DB 执行一次 remediation：
  182 个旧 eval failure 和 182 个 governance tasks 已审计关闭，备份保存在
  `data/tonglingyu/backups/rqa-eval-artifact-remediation-20260515T205220Z.db`。
- 2026-05-16 重新运行 `verify-tonglingyu-release-readiness.sh` 后，
  `retrieval_quality` 和 `rqa_backup_restore_drill` 已通过。
- 2026-05-16 security gate 已支持生产 digest-pinned image refs：
  `deploy/docker-compose.yml` 可通过 `*_IMAGE_REF` 绑定 immutable digest，
  `verify-tonglingyu-release-security.sh` 会读取 deploy env、解析 compose image
  refs 后再检查 mutable tag / digest missing。已将 Agent Platform JWT provider
  从 RustCrypto RSA 链路切到 `aws_lc_rs`，并把 `agent-store` 从 `sqlx`
  umbrella crate 收窄到 `sqlx-core` / `sqlx-postgres`，使 lockfile 不再包含
  `rsa`、`sqlx-mysql` 或 `sqlx-macros`。使用真实 `cargo-audit`、fixture image
  scan 和 digest image refs 的 security gate 已通过；真实 Trivy 路径会把
  per-image raw JSON 持久化到 `data/tonglingyu/security-image-scans/<run_id>/`
  或显式 `TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_DIR`，并逐个解析
  HIGH/CRITICAL vulnerability。image scan artifact 现在必须绑定
  `scanned_image_refs_sha256`、`scanned_reports_sha256`、
  `raw_report_paths_sha256` 和 `raw_report_artifact_dir`，且与当前 compose
  解析出的 `image_refs_sha256` 一致。saved report validator 会重算 raw
  report path digest 和 raw report content digest，并拒绝不匹配或 raw report
  不可读取的 production-ready report。生产镜像的真实 Trivy artifact 仍未生成。
- 2026-05-16 `runtime_config` 已支持非 live preflight 的静态 compose/env 解析；
  live release 仍通过 `TONGLINGYU_RUNTIME_CONFIG_REQUIRE_DOCKER=true` 要求真实
  Docker Compose config。
- 2026-05-16 以 digest image refs 和 fixture image scan 复跑
  `verify-tonglingyu-release-readiness.sh`：所有 required preflight gates 已通过，
  `required_failures=[]`，状态为 `passed_with_skipped_gates`；但
  `production_release_ready=false`，因为 live mode 未开启，model upstream、
  strict Gateway、Open WebUI Function、Open WebUI admin Action 仍 skipped，且
  browser review 未确认。
- 仍不能宣布 I 完成：生产镜像真实 scan artifact、live gates 和 browser review
  仍未执行；当前通过的是 repo-local / preflight 证据，不是 production-ready 证据。

## Milestone J：运维、恢复和真实发布

状态：进行中

目标：RQA 不是只在测试库可用，必须能在生产环境升级、保留、恢复和复测。

- [x] RQA 表纳入现有 backup / restore 演练。
- [x] 定义 RQA 数据的 RTO 和 RPO 默认目标，并写入 release report。
- [x] RQA 表纳入 retention / prune dry-run 和实际 prune 路径。
- [x] prune 不删除仍被 open failure、open governance task 或 accepted governance task
      引用的 runtime/gateway 数据。
- [x] production report 引用的数据保留必须接入 release artifact lifecycle 或 report
      reference registry。
- [x] retention / prune 保留 audit tombstone，且 tombstone payload 不包含原始
      question、response 或 secret。
- [x] retention / prune 与用户 delete/anonymize 和 legal hold 策略一致。
- [x] restore 后 admin trace、failure、governance task 和 quality gate 可继续读取。
- [x] restore 后必须重新运行 RQA quality gate 和 saved report validator。
- [x] 最近一次恢复演练必须记录 started_at、finished_at、RTO/RPO 是否满足、
      operator、环境和恢复后 gate 结果。
- [x] release gate / saved report validator 已强制生产 DB migration 前必须有
      备份路径和 schema preflight 输出。
- [ ] 目标 production DB migration 前已保存真实备份路径和 schema preflight 输出。
- [ ] live release mode 必须生成真实 RQA quality gate，不接受 fixture-only report。
- [x] release report schema / saved report validator 已强制绑定 environment、
      generated_at 和有效期。
- [ ] 目标 production-ready report 已绑定真实 live environment、generated_at
      和仍有效的有效期窗口。
- [x] release report schema / saved report validator 已强制绑定 runtime
      identity：当前运行镜像 inventory、代码版本和 migration 状态。
- [ ] 目标 production-ready report 已绑定真实 live runtime identity artifact：
      当前运行镜像 digest、代码版本和 migration 状态。
- [ ] production-ready report 必须绑定当前 live KB 的 source snapshot digest、
      KB build hash、kb_version 和 eval run id。
- [ ] production-ready report 必须绑定当前 live KB 的 source license summary 和
      attribution summary。
- [x] production-ready report 必须包含 RTO/RPO、最近一次恢复演练证据和恢复后
      gate 结果。
- [x] production-ready report 必须包含依赖/镜像/发布脚本安全扫描摘要或已审批
      risk exception。
- [ ] production-ready report 必须绑定当前 live Runtime profile、prompt、tool
      policy、reviewer policy、model upstream 和 decoding 参数摘要。
- [x] production-ready report 必须包含 RQA 用户数据生命周期策略版本和最近一次
      lifecycle contract smoke 结果。
- [x] live gate schema / saved report validator 已验证 RQA admin Action/API
      权限边界。
- [ ] 目标 live Open WebUI admin Action/API 权限边界证据已生成并绑定到
      release report。
- [x] live gate schema / saved report validator 已验证 RQA metrics 和 Prometheus
      不泄露 query 原文或 secret。
- [ ] 目标 live RQA metrics / Prometheus 隐私证据已生成并绑定到 release report。
- [x] RQA 写入、查询和 release gate 的耗时必须有 bounded timeout 或明确上限。
- [x] admin list/read 必须覆盖分页、最大 page size、payload 截断和 schema version。
- [x] performance smoke 记录 RQA 写入、admin 查询、release gate 的耗时摘要。
- [x] release gate 在缺少性能摘要或超过默认预算时不能 production-ready。
- [x] 单测覆盖 retention/prune 的 active RQA 引用保护和 tombstone。
- [x] contract smoke 覆盖 saved report freshness / release context 篡改。
- [ ] live smoke 覆盖 backup/restore 和 live report freshness。

节点总结：

- 2026-05-16 已新增 `tonglingyu-rqa-lifecycle-v1` 和
  `rqa_lifecycle_tombstones`。Runtime prune 先分区 expired package / audit event，
  只删除没有 open retrieval failure、open/in_review/accepted governance task
  trace/package 引用的数据；删除前写 tombstone，完成后写 `rqa_retention_pruned`
  audit。Actual prune 在写事务内重新判定候选集合再删除，避免判定后新增活动
  RQA 引用造成误删。Gateway prune 使用同一 RQA 保护谓词，避免删除仍被活动 RQA
  trace/package 引用的 `gateway_messages`、`workflow_states` 和 session，并写
  batch tombstone。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-rqa-backup-restore-drill.sh`。
  演练会构建或读取 RQA DB、执行 `backup-db`、恢复到隔离 DB、跑
  `PRAGMA integrity_check`、启动 restored Gateway、读取 admin trace /
  retrieval failure / governance task / package audit、执行 package replay，并在
  restored DB 上复跑 RQA quality gate 与 saved report validator。默认 RTO/RPO
  目标为 900s / 3600s，并写入 `tonglingyu.rqa_backup_restore_drill` gate stdout。
- 2026-05-16 restore drill 已新增持久 artifact 目录：
  live mode 默认写入 `data/tonglingyu/restore-drills/<run_id>/`，也可用
  `TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_DIR` 显式指定。gate stdout 会绑定
  `artifact_dir`、`artifact_dir_sha256`、`backup.artifact_path`、
  `backup.artifact_path_sha256` 和 `backup.artifact_sha256`。
- `verify-tonglingyu-release-readiness.sh` 已把 `rqa_backup_restore_drill` 接为必跑
  gate；saved report validator 要求该 gate 包含 backup、restore、RTO/RPO、
  post-restore checks 和可复核 artifact hash。production-ready report 若使用
  fixture source_mode、缺失持久备份 artifact，或备份 artifact 内容 hash 与
  stdout 不一致会被拒绝；live release 需要提供
  `TONGLINGYU_RQA_RESTORE_DRILL_TRACE_ID`、`PACKAGE_ID`、`FAILURE_ID` 和
  `TASK_ID` 对应的真实恢复引用。
- 2026-05-16 已新增 `runtime-schema-preflight` gateway CLI 和
  `verify-tonglingyu-rqa-migration-preflight.sh`。该 gate 在 preflight 模式会构建
  隔离 DB，生产/live 模式必须提供真实 DB 和显式备份路径；脚本先用 SQLite
  只读 backup 生成备份 artifact，再运行 runtime schema preflight，并输出
  source DB hash、backup path/hash、preflight digest、pending migration count 和
  secret/rebuild/delete 检查。release readiness 将
  `rqa_migration_preflight` 设为必跑 gate，saved report validator 会拒绝缺
  stdout、缺备份路径、preflight digest 不匹配或 production-ready 时仍有 pending
  migration 的报告。
- 2026-05-16 release readiness report 已新增
  `tonglingyu.release_context`：记录 environment、target、generated_at、
  valid_until、validity_hours、require_live 和 context_source；artifact registry
  记录 `release_context` digest。saved report validator 会重算 context digest，
  拒绝缺 context、generated_at 不一致、valid_until 不在 generated_at 之后、
  production-ready 过期、live 模式未显式绑定目标环境或使用 local/preflight/test
  环境名的报告；contract smoke 已覆盖缺 context 和无效 validity window。
- 2026-05-16 release readiness report 已新增
  `tonglingyu.release_runtime_identity`：strict Gateway live gate 会输出当前
  Docker Compose 运行镜像 inventory，release report 将该 inventory 与 git
  commit、tracked dirty 状态、security image inventory、migration preflight
  mode/count/hash 一起写入 runtime identity，并在 artifact registry 记录 digest。
  saved report validator 会拒绝缺 runtime identity、identity digest 漂移、
  live migration 不是 live、pending migration 未清零、tracked tree 不干净或缺
  `tonglingyu-gateway` / `open-webui` 运行镜像的 production-ready report。
- 2026-05-16 RQA 用户数据生命周期 gate 已闭合本地 contract：export 脱敏
  manifest、legal hold、delete/anonymize、release hold、audit tombstone、脱敏和
  可追责性均有 smoke 证据；release report validator 会拒绝缺 lifecycle gate
  stdout、关键 check 失败或 action 状态漂移的 production-ready 报告。
- 2026-05-16 live Open WebUI admin Action gate 已升级为结构化
  `tonglingyu.openwebui_admin_action_live_gate`：输出 active/global、valve keys、
  admin role guard、role denied、RQA admin Action 覆盖、Gateway admin API path
  覆盖、target model 绑定和 secret 输出边界。saved report validator 会拒绝
  role guard 缺失、RQA Action/API 覆盖不完整或 valves 未绑定的 production-ready
  report。
- 2026-05-16 strict Gateway live gate 已新增 `metrics_privacy`：递归检查 JSON
  metrics 是否出现 query/question/trace/package/session/user 等高基数或隐私字段，
  检查 Prometheus 是否出现 query、trace、package、session、user 或鉴权 label，
  并确认已知 secret 值没有出现在 metrics 输出；saved report validator 会拒绝
  metrics privacy 摘要缺失或含敏感 token 的 production-ready report。
- 仍不能宣布 J 完成或整体 RQA production-ready：本地
  migration preflight/runtime identity 机制、性能预算 gate 和 lifecycle gate 已闭合；
  本地 restore drill contract 已闭合但真实脚本路径当前被恢复后 RQA eval 质量
  blocker 阻断。目标 production DB 的真实 pre-migration backup/preflight artifact、真实 live
  release_context/report artifact、live existing_refs 恢复证据、真实 live runtime
  identity artifact、live admin Action/API 与 metrics privacy gate 证据、
  live KB/Runtime 绑定、真实安全扫描 artifact 和 live/load 性能证据仍未闭合。

## Milestone K：隐私、契约和性能预算

状态：已完成本地 contract / release gate 切片（2026-05-16；不等于整体
RQA production-ready，live Open WebUI admin Action、目标环境 live/load 性能和
发布值守证据仍需在 Milestone L/M 与 live release 阶段闭合）

目标：RQA 诊断能力不能以泄露用户文本、无界 API 或不可观测性能开销为代价。

- [x] RQA 数据模型区分 `question_summary`、`question_hash` 和 redacted excerpt。
- [x] 默认不保存完整用户问题；如需诊断原文，必须另有显式受控配置和审计。
- [x] redaction 覆盖疑似 key、token、URL secret、邮箱、手机号和长随机串。
- [x] admin detail 只能返回 redacted 字段，不能返回完整隐私文本。
- [x] 定义 RQA 用户数据生命周期：export、delete/anonymize、retention、legal hold
      和 audit tombstone。
- [x] 删除或匿名化用户相关 RQA 数据时，不能破坏 production report、open failure、
      governance task 或审计历史的可追责性；必须用 tombstone 记录处理结果。
- [x] 用户数据生命周期操作必须写 audit event，且输出不包含原始问题或 secret。
- [x] 所有 RQA list API 必须分页、排序稳定、最大 page size 固定。
- [x] RQA admin API / Action 响应包含 schema version 和 pagination metadata。
- [x] API contract smoke 覆盖旧 report 兼容、新字段兼容和未知字段拒绝/忽略策略。
- [x] Prometheus label set 固定且低基数。
- [x] JSON metrics 不输出原始 query、完整 question、trace 列表或 package 列表。
- [x] 定义 RQA 写入、admin 查询、release gate 默认性能预算。
- [x] release report 记录性能预算、实际耗时和是否超限。
- [x] 性能预算缺失或超限时，production-ready 必须失败。

节点总结：

- retention/prune 已具备 policy version 和 tombstone 记录，且单测覆盖 tombstone
  不包含原始问题或 secret；这只是 lifecycle 的 retention 子集。
- RQA failure 隐私 schema 已新增
  `tonglingyu-retrieval-failure-privacy-v1` migration：
  `retrieval_failures` 只保留 `question_sha256`、`question_summary`、
  `redacted_question_excerpt` 和 `redacted_query_terms_json`，并在 schema migration
  中拒绝含 raw `question` 列的 RQA failure 表。redaction 单测覆盖 password/key、
  token、URL secret、邮箱、手机号和长随机串；API contract gate 也会验证 admin
  detail 不返回原始 prompt 或敏感片段。
- Prometheus `tonglingyu_gateway_info` 已保留 `agent_runtime_mode`、
  `rate_limit_per_minute` 和 `max_body_bytes` 低基数配置标签，避免 gateway smoke
  对运行边界的复核退化。
- RQA performance budget gate 已定义 `tonglingyu-rqa-performance-budget-v1` 默认
  预算：RQA 写入 10s、admin trace/list 2s、状态更新 3s、RQA quality gate 90s；
  curl、KB build、eval 和 quality gate 都有可配置 timeout。saved report validator
  会拒绝缺 `rqa_performance_budget` stdout、缺 timeout 边界、预算超限、
  budget/measurement 不一致或关键 checks 未通过的 production-ready report。
- RQA API contract gate 已定义 `tonglingyu-rqa-api-contract-v1`，覆盖 admin list/read
  schema/pagination/max page、稳定排序、未知 filter、非法 enum filter、旧客户端解析、
  响应新增字段容忍、RQA admin mutation 未知字段拒绝和完整 prompt 泄露检查。兼容
  策略版本为 `tonglingyu-rqa-api-compatibility-v1`：响应新增字段允许旧客户端忽略，
  query 未知字段拒绝为 400，request body 未知字段拒绝为 422。
- Open WebUI admin Action contract 会运行 Action 单测并要求
  retrieval failure / governance task list 响应中的 `schema_version`、`limit`、
  `offset` 和 `next_offset` 被保留在 Action 返回内容中，避免 Action 层吞掉 Gateway
  的分页契约。
- RQA API contract gate 同时复核 `/v1/admin/metrics` 和
  `/v1/admin/metrics/prometheus`：JSON metrics 只保留聚合计数和有界状态桶，不含
  raw question、trace/package id 或 gateway/admin key；Prometheus label 名称只允许
  `status`、`failure_type`、`task_type`、`priority`、`event_type` 和低基数运行配置。
- RQA 用户数据生命周期 gate 已定义 `tonglingyu-rqa-user-lifecycle-contract-v1` 和
  `tonglingyu-rqa-lifecycle-v1` 策略版本，覆盖 export 脱敏 manifest、legal hold
  阻断、release legal hold、delete/anonymize、audit event、tombstone、原始用户值
  移除和 trace/package/failure/task 可追责性；报告字段使用 count/hash ref，避免
  在 release report stdout 中出现原始 question、response、user_ref、chat_ref 或
  secret。
- Open WebUI admin Action source/fixture contract 已接入 release readiness 必跑
  gate，并覆盖 role guard、必需 valves、RQA admin Action 列表、负向 fixture 和
  secret 输出边界；live Open WebUI admin Action gate 也已强制输出权限边界摘要，
  但目标环境真实 live Action 证据、live/load 性能证据和值守证据仍未完成。
- K 的本地隐私、API 兼容、分页契约、metrics 低基数和性能预算 fail-closed gate
  已闭合；它只能证明 release gate 具备可执行的 contract 边界，不能替代目标环境
  的 live admin Action、真实容量/负载和 operator handoff 证据。

## Milestone L：发布值守、告警和回滚

状态：进行中（2026-05-16；runbook、static ops gate、saved report validator
门禁已落地；真实 post-release monitor/live evidence 尚未完成）

目标：RQA production-ready 必须能被 operator 接住，而不是只在发布瞬间通过。

- [x] 在 deploy runbook 或专门文档中写明 RQA release 流程。
- [x] runbook 覆盖 migration preflight、backup、deploy、live gate、saved report
      validation。
- [x] runbook 覆盖回滚到上一镜像/配置的步骤。
- [x] runbook 覆盖 DB restore 或 additive schema 保留后的降级处理。
- [x] runbook 覆盖 RTO/RPO 目标、恢复步骤、恢复后 RQA gate 和 validator 复核。
- [x] rollback 后必须重新运行 release readiness 或明确标记 non-production。
- [x] 定义 RQA 写入失败率、admin API 5xx、admin API latency、open P0 failure、
      quality gate failure 的告警条件。
- [x] 告警指标必须低基数且不包含 query、question、trace 或 package id。
- [ ] post-release 监控窗口至少覆盖一次真实 live gate 和一次 admin Action/API
      查询。
- [ ] post-release 监控记录 operator、时间、环境、报告路径和结论。
- [x] post-release monitor 必须生成可复核 JSON evidence，并由 live ops gate
      与 saved report validator 绑定哈希。
- [x] production-ready report 必须引用 runbook / rollback / post-release 证据。
- [x] runbook 必须说明如何按 release report 的 commit/image/config/KB/security
      摘要复现本次发布。
- [x] release gate 或 saved report validator 缺少值守证据时不能
      production-ready。
- [x] smoke 覆盖告警字段存在性、runbook ref、rollback evidence ref 和
      post-release monitor ref、RTO/RPO evidence ref、安全扫描 evidence ref。

节点总结：

- 新增 `deploy/runbooks/tonglingyu-rqa-release-runbook.md`，覆盖 RQA release
  flow、migration preflight、backup、deploy、live gate、saved report validation、
  rollback、DB restore/additive downgrade、RTO/RPO、alert policy、incident
  response、post-release monitor 和 release report reproduction。
- 新增 `deploy/scripts/verify-tonglingyu-release-ops-readiness.sh`，并接入
  `verify-tonglingyu-release-readiness.sh` 的 required gate
  `release_ops_readiness`。preflight 模式只证明 runbook/alert/rollback 结构
  就绪；live 模式缺 rollback evidence、RTO/RPO evidence、alert evidence、
  post-release monitor、operator、environment、report path 或 `passed` 结论会
  fail-closed。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-post-release-monitor.sh`：
  生成并校验 `tonglingyu.post_release_monitor` JSON，要求 monitor 窗口至少 60
  分钟、operator/environment/report path/结论完整、release report 为 live 且
  live gates passed、admin Action/API 证据 ref 有效。live ops gate 现在必须绑定
  该 evidence path/hash；saved report validator 会拒绝未校验、缺失或哈希不匹配的
  production-ready 报告。
- Saved report validator 已要求 production-ready report 绑定
  `release_ops_readiness` stdout，并验证 runbook sha、低基数告警标签、rollback
  evidence、RTO/RPO evidence、post-release live gate/admin Action 证据和
  reproduction inputs。contract smoke 覆盖缺 gate stdout、缺 post-release live
  gate ref 和高基数告警标签篡改。
- 仍不能宣布 L 完成或整体 RQA production-ready：真实 post-release monitor 尚未
  执行，live gate evidence 和 live admin Action/API 查询证据尚未绑定到目标环境
  release report。

## Milestone M：事故响应、容量和审计完整性

状态：进行中（2026-05-16；status-history audit 与 incident/capacity gate
已落地；目标环境容量、负载、事故演练和审计历史 live evidence 尚未闭合）

目标：事故或压力场景下仍保持可追责、可降级、可恢复，不能用关闭治理来伪装可用。

- [x] 提供 RQA emergency disable 或 degraded mode 配置时，release report 必须标记
      non-production。
- [x] RQA persistence degraded 时，公共响应必须暴露稳定错误/降级状态和 trace id，
      不能伪装成完整成功。
- [x] RQA 写入不能使用无界内存队列；队列、batch 或 retry 必须有容量上限。
- [x] RQA retry 必须可幂等，且不会重复创建 failure、governance task 或 audit event。
- [x] 管理员状态更新必须保留历史记录：actor、reason、previous status、new status、
      timestamp。
- [x] 不允许硬删除 open failure、open governance task 或相关 audit history。
- [x] 事故 runbook 定义 severity、owner、first response、mitigation、rollback、
      recovery validation。
- [x] 事故 runbook 定义 RTO/RPO breach 的升级路径和发布状态处理。
- [x] incident drill / audit-history 必须生成可复核 JSON evidence，并由
      incident/capacity gate 与 saved report validator 绑定哈希。
- [ ] capacity smoke 覆盖代表性 eval report 数量、failure 数量和 admin list 翻页。
- [ ] load / soak smoke 覆盖 RQA 写入、admin 查询、metrics 和 release gate 在默认预算内。
- [x] capacity/load smoke 必须生成可复核 JSON evidence，并由 incident/capacity
      gate 与 saved report validator 绑定哈希。
- [x] release gate 缺少 capacity / incident / audit-history / RTO-RPO / security-scan
      证据时不能
      production-ready。
- [x] saved report validator 校验 emergency disabled、capacity missing、audit history
      missing、RTO/RPO missing、security scan missing、data lifecycle missing 不能
      production-ready。

节点总结：

- Runtime 和 Gateway 管理员状态更新已补 status-history audit：runtime audit 和
  admin audit 均记录 previous status、new status、reason hash 和 timestamp，
  幂等重复提交不会重复创建状态变更记录。
- 新增 `deploy/scripts/verify-tonglingyu-rqa-incident-capacity.sh`，并接入
  `verify-tonglingyu-release-readiness.sh` 的 required gate
  `rqa_incident_capacity`。preflight 模式只证明 emergency/degraded fail-closed
  规则、无无界队列静态检查、幂等标记、status-history audit 标记和 incident
  runbook 结构就绪；live 模式缺容量、负载、审计历史或事故响应证据会
  fail-closed。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-rqa-capacity-load-evidence.sh`：
  生成并校验 `tonglingyu.rqa_capacity_load_evidence` JSON，要求 capacity smoke
  覆盖代表性 eval report、failure 和 admin list 翻页，load/soak smoke 覆盖 RQA
  写入、admin 查询、metrics 查询和 release gate，并按默认预算校验。live
  `rqa_incident_capacity` gate 现在必须绑定该 evidence path/hash；saved report
  validator 会拒绝未校验、缺失或哈希不匹配的 production-ready 报告。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-rqa-incident-audit-evidence.sh`：
  生成并校验 `tonglingyu.rqa_incident_audit_evidence` JSON，要求 status-history
  event/actor 覆盖、audit tombstone、incident severity/owner、first response、
  mitigation、rollback、recovery validation 和 RTO/RPO breach escalation evidence
  ref 完整。live `rqa_incident_capacity` gate 现在必须绑定该 evidence path/hash；
  saved report validator 会拒绝未校验、缺失或哈希不匹配的 production-ready 报告。
- Saved report validator 已要求 production-ready report 绑定
  `rqa_incident_capacity` stdout，并拒绝 emergency disabled、degraded mode、
  persistence degraded、非 live 模式、缺 evidence ref、代表性数量不足、负载
  measurement 缺失、status-history 字段缺失或 incident runbook 证据缺失的报告。
- 仍不能宣布 M 完成或整体 RQA production-ready：目标环境 capacity smoke、
  load/soak smoke、incident response drill 和 audit-history live evidence 尚未生成；
  当前只完成本地闸门与 fail-closed contract。

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
