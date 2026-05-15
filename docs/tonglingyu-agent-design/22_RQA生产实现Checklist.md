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

状态：完成（2026-05-15；release readiness 已接入 canonical RQA quality gate，
gate 输出已绑定可复核 eval artifact、production 阈值配置和 strict live gate
行为配置 fingerprint；当前本地 DB 因仍有 157 个 open retrieval failures，真实
RQA quality gate 正确 fail-closed）

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
- 当前本地 DB 验证结果：`quality_summary.status=passed`，但
  `open_p0_retrieval_failures=157`，因此 RQA quality gate 返回
  `status=failed`，release readiness 报告保持 `production_release_ready=false`。
  这证明 gate 已作为 production blocker 工作，但也证明当前本地状态仍未达到
  RQA production-ready。
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
  宣布整体 RQA production-ready，因为当前 live/local 数据仍有 open retrieval
  failures，且 Milestone H-M 的治理、自动化、恢复、隐私生命周期、值守和事故容量
  闭环尚未完成。

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

状态：进行中（2026-05-16；治理任务 schema、通用 source entity、failure-to-task、
普通用户反馈、retrieval failure 聚类、knowledge patch proposal、KB diff/eval
diff、admin API/Action 和 release gate blocker 已完成第一批代码切片）

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
- 仍不能宣布 H 或整体 RQA production-ready：retention/restore 和 lifecycle
  contract 仍未完成；accepted 状态本身仍不能直接等同为事实层已更新，必须经过
  rebuild application、diff report 和 eval gate。

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
