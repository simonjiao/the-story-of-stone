# 通灵玉进展与决策记录

## 当前状态

- 主线已切到“通灵玉”第一版。
- 旧基础库产物和旧专用抽取脚本已删除。
- `scripts/extract_epub.py` 和 `scripts/download_wikisource.py` 已输出
  source snapshot，并保留 `rare_char_annotations`。
- `resources/styles/buhongjushi/` 风格转录保留，不作为主证据库。
- Rust 主线入口为 `agent-platform/crates/tonglingyu-gateway/`。

## 已确认

- 第一版只验证“资料快照 -> 知识库 -> 证据卡片 -> 证据包 -> reviewer -> 分层回答”闭环。
- 维基文库《红楼梦》全本、脂批本和可追溯版本资料是第一批基础资料候选。
- 知识库按原文、脂批、版本、人物、关系、事件、诗词判词和评测题库分层，不做大向量库。
- 原始字形和来源中已有读音必须保留；规范化文本只能作为检索辅助字段。
- 现代白话摘要只能辅助检索，不能作为可引用证据。
- 风格资料只影响表达方式和讲解路径，不能覆盖正文、脂批、版本或校订证据。
- `不红居士` 是风格名，不替换转录文本中的 `不红君`。
- `官中`、`宫中`、`公中` 等高风险同音词必须回到已登记证据确认。
- 第一批资料从维基文库获取；允许联网下载公开资料；第一版只使用 SQLite/FTS，不接外部向量库。
- 远程部署复用一个现有 Open WebUI，Gateway 单独部署。
- 已下载第一批 source snapshot：`hongloumeng-wikisource-120`、`hongloumeng-wikisource-chengjia`、`hongloumeng-wikisource-chengyi`、`shitouji-wikisource-zhiyanzhai`、`shitouji-wikisource-jiaxu`。
- `hongloumeng-wikisource-chengjia` 已通过 ProofreadPage Page namespace 展开补齐正文。
- 第一批 Wikisource snapshot 已补 source snapshot ready 口径和 19 个跨版本
  抽样点；当前可支持通俗分析场景下的跨版本对照和证据链检索，但仍不代表
  完成学术校勘、影印复核或权威校注本复核。后续 RQA production-ready
  必须把这个 source coverage boundary 写入公共回答边界、RQA report 和
  release report。
- M1 完成闸门已明确：进入 M2 前必须完成并通过 source snapshot registry
  校验；本计划不设置独立“M1.5”。若影印件、权威校注本或评测题库要阻塞
  M2，必须先提升为 M1 P0。
- 当前 `python3 scripts/validate_source_snapshots.py` 已通过：5 个来源和
  19 个抽样点满足 M1 source snapshot 闸门；validator 同时要求每个来源
  具备机器可读 `source_url`、`license`、`license_url`、
  `license_source_url`、`attribution` 和 `usage_boundary`。
- Rust `tonglingyu-gateway` + `tonglingyu-runtime` 已实现 M2-M6 最小工程闭环：
  source snapshot loader、SQLite/FTS、别名种子、证据卡片、证据包、
  reviewer、OpenAI-compatible `/v1/models` 和 `/v1/chat/completions`。
- 通灵玉目标架构已调整为“薄 Gateway + Runtime Agent”；当前 Gateway 内部
  闭环作为运行基线和回归基线保留，目标 Runtime 接入设计和 checklist 见
  `20_Runtime接入设计与实施计划.md`。
- 本地建库已验证：5 个来源、10419 个 blocks、10419 条 FTS 记录。
- 本地 HTTP 验证已通过：`/healthz`、`/v1/models`、`/v1/evidence/search`
  和 `/v1/chat/completions`。
- `deploy/docker-compose.yml` 已加入真实 `tonglingyu-gateway` 服务，Open WebUI
  默认连接该 Rust Gateway，Gateway 再按配置调用 Hermes 上游生成层。
- 2026-05-09 已在远程 `hhost:/home/simon/hermes-home-deploy` 真实部署：
  启动 `tonglingyu-gateway` 和现有 `hermes-open-webui`，远端 gateway
  healthcheck 为 healthy。
- `tonglingyu-gateway` 已拆为独立镜像，使用
  `agent-platform/crates/tonglingyu-gateway/Dockerfile` 构建，并通过
  BuildKit cache mount 缓存 Cargo registry、git 源和 `target/`；通用
  `hermes-agent-platform` 镜像不再包含 gateway 二进制。
- 远端已验证第二次 `docker compose build tonglingyu-gateway` 全部命中
  Docker/BuildKit 缓存；`tonglingyu-gateway:formal` 含 gateway 二进制，
  `hermes-agent-platform:formal` 不含 gateway 二进制。
- Open WebUI 当前通过独立 Rust `global-router` 作为单入口；它是 MVP
  路由层，不是完整生产级 router。设计和进展独立记录在
  `docs/global-router-design/`。
- 远端 KB 由 `tonglingyu-gateway` 容器启动时从 source snapshot 构建，
  当前 `/healthz` 返回 5 个来源、10419 个 blocks；Open WebUI 容器内
  `DEFAULT_MODELS=tonglingyu`。
- 远端容器内已验证 `/v1/models`、`/v1/evidence/search`、
  `/v1/chat/completions`、证据包 owner-only 读取、admin
  trace/session/package/metrics 和 Prometheus 指标；“通灵玉上的字是什么？”
  返回带证据包和 reviewer 约束的回答。
- 2026-05-10 已更新远端 `tonglingyu-gateway:formal`
  (`sha256:faff12b147dab57dfb3e041f551c4a9320c20158acb763cc7fe12b82e25c2127`)
  并重启 `tonglingyu-gateway` 与 `hermes-open-webui`；compose 内两个服务
  均为 healthy，公网 `https://chat.huixiangdou.top/api/config` 返回 200。
- 最终远端 smoke 记录：`package_id=pkg-019e0ffe32617c7291e58267bca26655`，
  `trace_id=tly-019e0ffe30bc7c918613681c2b5cd27a`，
  `session_id=session-019e0ffe30c57742904e728a3cd14aea`。
- 管理员账号 API 侧验收已通过：Open WebUI 登录角色为 `admin`，
  `/api/models` 可见 `tonglingyu`，`/api/config` 的 `default_models` 为
  `tonglingyu`，并通过 `/api/chat/completions` 成功转发到 Gateway。
  对应 Gateway 审计可查：
  `package_id=pkg-019e1006535a76139f0eb7e568b8d70d`，
  `trace_id=tly-019e10064fa079118d3dbf84d344f8ba`，
  `session_id=session-019e1005c3ff7dd1b5bc3d02c6270b82`；admin metrics
  显示 `admin_key_isolated=true`。
- `tonglingyu-gateway` 已补证据包确定性回放：CLI
  `replay-package` 和 HTTP
  `/v1/evidence/packages/{package_id}/replay` 都可在不调用上游模型的情况下
  重建受 reviewer 约束的本地回答。
- evidence package 创建和 reviewer 完成事件已写入 SQLite `audit_events`，
  便于后续按 trace ID 回放与审计。
- 已补内置评测入口 `eval`，当前覆盖正文、脂批、版本边界、人物别名、
  诗词判词、字形读音、证据不足、prompt injection、预期证据 ID 和禁止
  结论等 103 个发布回归 case；评测报告可落盘到
  `data/tonglingyu/reports/`。
- 已新增 `agent-platform/scripts/tonglingyu-gateway-smoke.sh`，可临时建库并
  验证 Gateway 鉴权、单可见模型、会话映射、消息去重、内部字段拒绝、
  模型越权拒绝、SSE streaming 元数据、证据包读取、证据包回放、
  admin trace/session/package/metrics、Prometheus 指标和内置评测。
- 完整通灵玉产品 Gateway 的本地代码级差距已收敛：接口层鉴权、会话映射、
  状态机、证据源强制策略、claim-to-evidence 映射、审校拦截、审计查询、
  脱敏错误、streaming 追踪、备份和保留清理均已有测试覆盖。
- R5 Runtime 接入已开始实现：新增
  `agent-platform/crates/tonglingyu-runtime/`，先承接证据包创建和读取、
  claim-to-evidence 映射、reviewer 规则、本地受控回答和 package replay；
  Gateway 请求路径改为调用该 runtime crate，不再在 Gateway 内维护这些领域逻辑。
- Runtime crate 已继续承接 SQLite/FTS 检索、alias 取词、exact term 保护和
  evidence card 构建；Gateway 保留 search policy/plan 决策，只把
  `required_evidence_types` 交给 runtime 执行。
- Gateway 已新增 `tonglingyu-gateway::plan` 模块，集中维护 search policy、
  Runtime step plan schema/policy 版本和受控 step 快照；Gateway 审计中的
  Planned 状态会记录该 runtime step plan，但当前仍是本地 runtime API 调用，
  不是完整 `agent-runtime` step 执行。
- Evidence package、review record、claim link 和 audit event 的运行时表
  初始化已迁入 `tonglingyu-runtime::init_runtime_schema`；Gateway 只初始化
  gateway session/message/workflow 表并调用 runtime schema 初始化。
- Source snapshot loader、KB schema、FTS 写入、别名种子和章节解析已迁入
  `tonglingyu-runtime::rebuild_knowledge_base_from_snapshots`；Gateway
  `build-kb` 只保留 DB 文件生命周期、gateway session/workflow 清理和
  Runtime rebuild 调用。
- Gateway 单元测试已加入源码级回归断言，防止 `extract_terms`、
  `query_blocks_like`、`evidence_card_from_block`、`review`、source snapshot
  loader 和 FTS 写入等 runtime 领域函数重新回流到 Gateway。
- Runtime 已定义 `tonglingyu.text.search`、`tonglingyu.commentary.search`、
  `tonglingyu.evidence.package.create/read/replay` 的 tool catalog 和结构化
  `TonglingyuToolCall` / `TonglingyuToolOutput`；Gateway 主路径已改为通过
  `execute_runtime_workflow` 调用 Runtime workflow，不再直接编排
  search/package/draft/review。
- Runtime workflow 已生成 `honglou-text`、`honglou-commentary`、`honglou-main`
  和 `honglou-reviewer` 的 profile step reports，包含 schema version、
  duration、tool set、tool calls、input_ref、output_ref 和 trace_id；Gateway
  smoke 也会校验 `runtime_profile_step_completed` audit event。
- 本轮验证已补跑 `agent-runtime` 单包测试、`tonglingyu-runtime` /
  `tonglingyu-gateway` 单包测试、clippy、文档 lint 和 gateway smoke。
- Runtime 已定义 `honglou-text`、`honglou-commentary`、`honglou-main`、
  `honglou-reviewer` 四个 profile descriptor；Gateway Runtime step plan
  已带 `PROFILE_CONTRACT_VERSION`，避免 plan 与 profile contract 脱节。
- `tonglingyu-runtime` 已把四个 profile descriptor 映射为 `agent-core`
  `ProfileContract`、read-only `RuntimeToolPolicy` 和 `RuntimeStepPlan`；
  Gateway 新请求和 `runtime-dry-run` 会先执行 `agent-runtime`
  `MinimalRuntimeClient` plan gate，校验 step dependency、requested tool scope、
  output_ref 和 Runtime step metadata。
- Runtime plan factory 已收敛到 `tonglingyu-runtime::runtime_workflow_plan`；
  Gateway plan、agent-runtime plan gate 和实际 workflow 共用 step_id、operation
  和 allowed tools，测试会比较实际 workflow step 与 runtime plan 是否一致。
- `tonglingyu-runtime` 已新增 `TonglingyuRuntimeStore`；Gateway 请求路径、
  `runtime-dry-run`、health、search、metrics、admin package/trace 读取以及
  build/prune 管理路径已改为通过 runtime store handle 按 DB path 访问 runtime，
  不再把 Gateway `Connection` 直接传入 Runtime workflow/tool/schema/prune/rebuild
  API。
- Gateway 新请求和 `runtime-dry-run` 已让每个确定性 profile step 经过
  `agent-runtime` `MinimalRuntimeClient::execute_profile_step` envelope，并把
  `agent_runtime` metadata 写入 step report、stream event metadata 和
  `agent_runtime_profile_step_executed` audit event；该 metadata 明确标记
  `content_source=tonglingyu-deterministic-workflow`、
  `content_used_for_final_answer=false`。
- `tonglingyu-runtime` 已新增 store-backed `TonglingyuRuntimeToolExecutor`，
  实现 `agent_core::RuntimeToolExecutor`，可把 agent-runtime/Hermes tool call
  转成 `TonglingyuToolCall` 并调用本地 SQLite evidence/package 工具；单测覆盖
  text search、package create/read。
- profile step execution envelope 已支持 `TONGLINGYU_AGENT_RUNTIME_MODE=minimal|hermes`；
  默认 `minimal`，`hermes` 模式使用 `HermesRuntimeClient::from_env()` 并挂载本地
  Tonglingyu tool executor，但最终回答仍未切到 Hermes content/tool execution。
- `hermes` 模式下 `draft_answer` profile output 已可成为 workflow 草稿，并由
  本地 reviewer enforcement 重新生成最终回答；Runtime 会记录
  `agent_runtime_profile_draft_consumed` audit event，并区分
  `content_used_for_final_answer`。该路径仍不是四 profile 全量 content/tool
  execution 完成。
- profile step message 已携带 trace_id、profile、operation、question、input/output
  ref、allowed tools 和 step output JSON，避免 Hermes profile 只收到空泛
  envelope。
- Runtime 单测已覆盖完整 store workflow：注入 fake Hermes runtime client，验证
  Hermes draft 候选在 reviewer 降级时不会进入 final answer，并写入
  `agent_runtime_profile_draft_consumed` audit event。
- Gateway health、JSON metrics、Prometheus info 和 `runtime-dry-run` 已暴露
  `TONGLINGYU_AGENT_RUNTIME_MODE` 的有效模式；smoke 断言默认 `minimal`。
- Runtime step report、SQLite audit 和 streaming step summary 已透出
  agent-runtime/Hermes 工具 loop 观测信息；完整 tool result/audit event
  保留在 step report/audit payload 中，stream 只暴露计数级摘要。
- Hermes mode 已对 required profile step 增加 runtime tool result 强制校验：
  step 声明了 allowed tools 但 agent-runtime/Hermes 没有返回匹配工具结果时，
  workflow fail-closed，避免把未实际调用工具的 profile 文本误判为
  content/tool execution。
- Hermes mode 同时要求 runtime tool result 的 `output_ref` 绑定
  `runtime://tonglingyu/{trace_id}/...`；package create/read/replay 类工具必须
  绑定当前 evidence package id，text/commentary search 工具必须绑定当前本地
  evidence set 指纹，防止无法追溯到本地 Runtime store 的伪工具结果。
- Hermes `draft_answer` 结构化 JSON 候选必须匹配当前 evidence package
  `package_id` 且提供非空 `draft_answer`；错误 package 或缺少草稿时只写
  rejected audit，不进入本地草稿或最终回答。
- strict Gateway live gate 已要求 admin trace 同时出现 evidence/package/reviewer
  local enforcement observation 和已消费的 Hermes draft observation，避免只因
  tool-result plumbing 存在就误判 Runtime 接入完成。
- Hermes mode 的 step `content_source` 已从固定
  `tonglingyu-deterministic-workflow` 改为按 observation/application 状态标记
  `agent-runtime-hermes-*`，使 audit 和 streaming metadata 能区分 Hermes 观察、
  Hermes 草稿消费和本地治理兜底。
- Runtime output 已新增 `agent_runtime_summary`，并写入
  `agent_runtime_profile_execution_summarized` audit event；Gateway dry-run、
  workflow state、admin trace 顶层字段和 strict live gate 都会读取该 summary，明确区分
  `minimal_envelope_only`、Hermes observation/local governance 和 incomplete
  fallback，避免只靠分散 step metadata 人工推断；strict gate 会同时校验 summary
  的 step/tool 计数与详细 runtime step audit event 一致。
- Gateway 公共 OpenAI-compatible 请求路径的 forbidden control fields 已覆盖
  后续新增的 Runtime/admin trace 字段，包括 `agent_runtime_summary`、
  `runtime_step_plan`、`allowed_tools` 和 `admin_trace`，防止普通 Open WebUI
  请求伪造内部执行、工具或审计状态；`metadata`、`extra_body`、`options`、
  `parameters` 和 `config` 下会递归扫描这些控制字段。
- strict Gateway live gate 已增加公共 chat 响应字段检查，拒绝
  `agent_runtime_summary`、`runtime_step_outputs`、`audit_events`、
  `_runtime_stream_events` 等内部 runtime/admin trace 字段以任意嵌套层级泄露到
  普通响应。
- Gateway smoke 已对新 streaming 和去重 replay streaming 响应增加同类内部字段
  结构化 SSE JSON 负向检查，避免 SSE 路径只验证可用性、不验证信息边界。
- strict Gateway live gate 已增加 streaming chat completion 验证，要求 `[DONE]`、
  package metadata、Runtime workflow source marker，并按 SSE JSON chunk 递归复用
  内部字段泄露检查。
- strict Gateway live gate 会从 streaming SSE chunk 解析 `trace_id` 并读取对应
  admin trace，确认 streaming 请求也有 Hermes Runtime summary/audit 闭环。
- Gateway smoke 和 strict Gateway live gate 已要求 streaming 响应包含 Runtime
  `content_delta` chunk，防止普通 cached completion stream 或空 marker 被误判为
  Runtime event replay。
- Gateway smoke 已补 streaming trace 级 admin 校验：从 streaming 与 replay SSE
  解析唯一 trace/package/session，读取 stream trace 并校验 Runtime summary、
  audit event 与消息归属。
- Runtime summary 已把 `tool_audit_event_count` 提升为一等生产校验字段；
  Hermes mode 若存在 tool result 但缺少对应 tool audit event 会 fail-closed，
  strict Gateway gate 也会交叉校验 summary 和 step audit，避免把“返回了工具结果
  字段”误判为“工具执行已被审计”。
- strict Gateway gate 进一步要求每个 tool result 都有匹配的
  `runtime_tool_result` audit event，且 `tool_name` / `output_ref` 相同，避免只靠
  audit 数量覆盖但不能绑定到具体工具输出。
- strict Gateway gate 已校验 admin trace 顶层 `agent_runtime_summary` 必须等于
  最新 `agent_runtime_profile_execution_summarized` audit event，避免管理员入口
  展示字段与审计链脱节。
- Hermes mode 已把 incomplete profile content/tool execution 前移为 Runtime
  fail-closed：如果 summary 未达到
  `hermes_profile_observed_with_local_governance`，请求会写
  `agent_runtime_profile_execution_rejected` audit event 并返回错误，不再用本地
  deterministic fallback 伪装成成功回答。
- Hermes `review_answer` 结构化 JSON 输出已进入 review observation；Runtime
  会记录 LLM reviewer status/severity/issues 与本地强制 reviewer 的一致性，
  不一致时标记 `local_reviewer_override=true`，最终裁决仍由本地 reviewer 决定。
- Profile step message 和 metadata 已携带 operation-specific
  `result_summary_contract`，明确 `draft_answer` 与 `review_answer` 的结构化
  JSON 输出要求，避免 Hermes 只收到泛化 step context。
- `text_evidence_search` 与 `commentary_evidence_search` 结构化输出已进入
  evidence observation；Runtime 校验 Hermes evidence refs 是否来自本地
  runtime step `evidence_ids`，未知 ref 只写 rejected reason。
- Hermes `evidence_package_create` 结构化输出已进入 package observation；
  Runtime 校验 observation `package_id` 是否匹配本地 evidence package，不匹配
  只写 rejected reason，不允许改写本地证据包。
- Gateway eval/replay 检查已改为通过 `TonglingyuRuntimeStore` 调 runtime replay；
  gateway crate 不再直接调用 runtime reviewer 或 `replay_answer` 领域函数。
- Gateway 公共 `/v1/*` 入口已增加 per-subject rate limit，
  `TONGLINGYU_RATE_LIMIT_PER_MINUTE` 默认 120，`0` 表示关闭；health、JSON
  metrics 和 Prometheus info 暴露有效配置，smoke 覆盖默认值。
- Gateway HTTP request body 上限已由 `TONGLINGYU_MAX_BODY_BYTES` 显式限制，
  默认 1 MiB；health、JSON metrics 和 Prometheus info 暴露有效配置，
  smoke 覆盖默认值。
- Gateway CLI 已新增 `runtime-dry-run`，可在本地 DB 上通过 runtime tools
  执行 search、package create、package replay 和 reviewer 约束检查；
  gateway smoke 已覆盖该 dry run。
- Gateway health、metrics、admin trace 的 KB/runtime 统计和 runtime audit 读取
  已改为调用 `tonglingyu-runtime` stats/audit API；runtime prune 和 audit
  append 也已迁入 Runtime，Gateway 只保留 gateway session/workflow 清理。
- Runtime workflow 已生成 `started`、`step_completed`、`content_delta` 和
  `final_output` stream events；新请求的 Gateway streaming response 已改为
  转发 Runtime `content_delta` event，并由 smoke 校验 `runtime_workflow`
  标记和 dry run 的 `runtime_stream_events`。去重缓存命中的 streaming replay
  已复用缓存中的 Runtime stream events；旧缓存如果缺少 events，会 fallback
  到 cached completion stream。
- Gateway final response 和去重缓存非 streaming replay 会剥离内部
  `_runtime_stream_events` / `_stream_source`，smoke 已断言公开 completion 不暴露
  runtime step plan、agent runtime plan gate、planned profiles 或内部 stream
  event 列表。
- Gateway 已强制 admin API key 与 Gateway service key 集合隔离：启动时拒绝
  重叠 key，拒绝在已配置 admin key 时继续开启 gateway-key admin fallback；
  metrics 的 `admin_key_isolated` 现在反映真实 key 集合隔离状态。
- `deploy/scripts/verify-tonglingyu-runtime-config.sh` 已补 compose 渲染配置 gate：
  检查 Tonglingyu Gateway/Hermes/Agent Runtime strict wiring、Open WebUI 默认
  模型、admin/gateway key 集合隔离，以及 Open WebUI provider key 不含 admin
  credential；输出只包含变量名和 gate 状态。
- `deploy/docker-compose.yml` 已把 `tonglingyu-gateway` 的
  `TONGLINGYU_AGENT_RUNTIME_MODE` 生产默认值设为 `hermes`，并显式注入
  `AGENT_RUNTIME_HERMES_*`；配置 gate 会拒绝 Gateway 自身仍落回 `minimal`
  runtime mode 的生产渲染结果。
- `deploy/scripts/verify-tonglingyu-strict-gateway.sh` 已补运行态 Gateway gate：
  从正式 Docker 网络检查 `/healthz`、`/v1/models`、admin metrics、Prometheus、
  live chat completion 和对应 admin trace，要求 Gateway 实际报告 `hermes`
  runtime、只暴露 `tonglingyu` 模型、隐藏 `honglou-*` profile、KB 非空、
  rate limit 开启、admin key 已隔离，并且 trace 中 Hermes profile step 有
  runtime tool result。
- `deploy/scripts/verify-tonglingyu-release-readiness.sh` 已补聚合发布 gate：
  默认运行 compose 渲染配置检查，`TONGLINGYU_RELEASE_REQUIRE_LIVE=true` 时把
  strict Gateway 和 Open WebUI Function 检查作为必过 gate；报告会显式记录
  `production_release_ready`、`browser_review_acknowledged`、optional failures、
  skipped live gates、release blockers 和仍需人工页面复核的项目；optional gate
  失败会把 `status` 标为 `passed_with_failed_optional_gates`；只有显式 live
  release mode、必过 gate 和页面 ACK 都满足时才会标记 production ready，避免
  把局部验证当作生产完成；脚本默认退出码也跟随 `production_release_ready`，
  只有显式 `TONGLINGYU_RELEASE_SUMMARY_ONLY=true` 才允许非发布 summary 返回成功。
- release readiness 报告已固定 `object=tonglingyu.release_readiness_report`
  和 `schema_version=1`；本地 contract smoke 会断言报告对象和 schema，避免
  后续自动化或人工复核靠字段猜测报告版本。
- 已新增 `verify-tonglingyu-release-readiness-report.sh` 校验保存后的 release
  readiness report。它会检查 schema、production-ready invariants、browser
  validation、live gate 状态、override、blocker 和人工检查项，避免报告文件被
  手动篡改成 `production_release_ready=true` 后仍被后续流程采信。
- Saved release report 校验继续收紧：summary-only 报告不能被标记为生产 ready；
  `browser_review_validation` 必须是同一 release review ref/evidence path 的
  成功 verifier 输出，并携带 checked items、空 errors 和 evidence SHA-256。
- Saved release report validator 现在会从 gate records 重算 `status`、
  failures、live gate 列表、blockers、manual checks、release conditions 和
  production-ready flag / exit policy；contract 覆盖派生字段、exit policy
  和 ready flag 篡改，避免 artifact 字段与实际 gate 证据分叉。
- Saved release report validator 进一步要求顶层 `browser_review_validation`
  与 `openwebui_browser_review` gate `stdout_tail` 中实际输出的成功 verifier
  JSON 一致；contract 覆盖删除 stdout validation 和顶层 validation 篡改。
- Saved release report validator 还会校验非 override / production-ready 报告中
  runtime config、model upstream、strict Gateway、Open WebUI Function 和
  Gateway Admin Action 的 passed gate stdout JSON；contract 覆盖 live gate
  stdout 被删除后仍试图保持 ready 的篡改路径。
- Saved release report validator 现在要求每份报告都包含 exact canonical release
  gate set：`runtime_config`、`model_upstream_network`、`strict_gateway`、
  `openwebui_function`、`openwebui_admin_action` 和
  `openwebui_browser_review`；缺 gate 即使不是 production-ready report 也会失败，
  未知 gate 也会失败，避免报告漏掉 live/browser gate 或塞入未定义 passed gate 后
  仍被当作完整发布证据。
- Saved release report validator 新增 `generated_at` 时区和新鲜度校验；
  production-ready 报告默认 24 小时后过期，contract 覆盖缺失 generated_at 和
  过期 ready artifact，避免复用旧报告宣称当前生产就绪。
- Production-ready saved report 现在要求 browser review verifier 显式绑定
  release review ref 和 Open WebUI 公网入口；contract 覆盖解除 ref/public URL
  binding 后仍试图保持 ready 的篡改路径。
- Release report 现在带 `secret_values_printed=false`，saved report validator
  会递归扫描 secret-like 值并只报告 JSON path；contract 覆盖 gate tail 泄露
  `authorization` / `sk-` 风格值时报告校验失败。
- Saved release report validator 对 gate `stdout_tail` / `stderr_tail` 增加
  有界 schema 校验；contract 覆盖 tail 超过 20 行和非字符串 tail item，避免
  发布 artifact 被篡改成难以审计或展示的日志载体。
- release readiness gate 在 `TONGLINGYU_RELEASE_REQUIRE_LIVE=true` 时还要求
  `TONGLINGYU_RELEASE_ACK_OPENWEBUI_BROWSER_REVIEW=true` 和非空
  `TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_REF`，并新增
  `verify-openwebui-browser-review-evidence.sh` 校验
  `TONGLINGYU_RELEASE_OPENWEBUI_BROWSER_REVIEW_EVIDENCE` 指向的 JSON 证据报告；
  Open WebUI 页面侧普通用户模型可见性、streaming UX、admin audit 和持久化
  provider 设置复核必须逐项 `passed` 且带 `evidence_ref`；证据报告内
  `review_ref` 必须匹配 release ref，`reviewed_at` 必须带时区，公网入口必须是
  HTTPS，不能只靠口头 ACK 或任意 JSON。
- 已新增 `record-openwebui-browser-review-evidence.sh`，人工页面复核完成后用
  env 填入 reviewer、公网 URL、四项 evidence ref 和 provider 设置匹配确认，
  由脚本生成 evidence JSON 并立即运行 verifier；脚本要求显式 ACK 且默认不
  覆盖已有证据文件，并支持 `--preflight` 在不写证据文件的情况下检查必填输入和
  覆盖安全，减少手写 JSON 或人工交接遗漏造成的发布误判。
- browser review evidence verifier 已继续收紧 evidence ref：截图/本地文件
  ref 必须能在证据目录或
  `TONGLINGYU_BROWSER_REVIEW_EVIDENCE_ROOT` 下找到，admin audit ref 需绑定
  `trace:tly-...`、文件或 HTTPS 链接，provider 设置复核需绑定 `runbook:...`、
  文件或 HTTPS 链接。
- browser review evidence verifier 现在要求 `checks` 是 exact browser review
  set；未知检查项会失败，避免手写 evidence JSON 塞入未定义检查后暗示额外发布
  复核已完成。
- browser review evidence verifier 也会校验发布入口和时间窗口：设置
  `TONGLINGYU_RELEASE_OPENWEBUI_PUBLIC_URL` 后，证据中的 `public_webui_url`
  必须匹配该入口；`reviewed_at` 默认必须在 24 小时内，避免用其他环境或过期
  复核证据关闭 release gate。
- browser review evidence verifier 已输出 evidence JSON SHA-256 和本地 artifact
  SHA-256；release readiness 聚合报告新增 `browser_review_validation`，保留
  `reviewer`、`reviewed_at`、`public_webui_url`、evidence digest 和本地 artifact
  digest，并把 evidence JSON 规范化为绝对路径后写回顶层
  `browser_review_evidence`，确保发布报告自身能说明谁在何时复核了哪个 Open
  WebUI 公网入口，而不只保存会随 cwd/report 位置变化的证据路径。
- Saved release report validator 现在会重新校验
  `browser_review_validation.checked_items` 和 `validated_evidence_refs`：
  browser review item 必须是 exact required set，存在 validation 时顶层
  `browser_review_ref` 和 `browser_review_evidence` 必须保留，四项都必须有 ref，
  evidence path 必须是绝对路径，ref kind 必须合法，本地文件 ref 必须带 64 位
  SHA-256，且
  `browser_review_evidence` 指向的 JSON 必须与
  `browser_review_validation.evidence_sha256` 匹配；本地文件 ref 也会按 evidence
  目录或 `TONGLINGYU_BROWSER_REVIEW_EVIDENCE_ROOT` 重新计算 digest，避免
  production-ready artifact 用额外检查名、空 refs、相对 evidence path、假 digest
  或不可复核的 evidence 摘要通过。
- release readiness 聚合逻辑现在要求 browser review gate 成功时必须解析出
  `browser_review_validation`；如果 gate 退出 0 但没有 validation 摘要，会记为
  `openwebui_browser_review_validation`；live release 模式下作为必过失败，
  非 live summary 模式下作为 optional failure，避免报告出现 gate passed 但
  browser review 仍未被承认，或 summary/report 状态分类错误的灰色状态。
- `deploy/scripts/test-tonglingyu-release-readiness-contract.sh` 已补 release
  readiness contract smoke，覆盖 browser review recorder 正负路径、
  browser evidence ref 文件存在性、public URL mismatch、过期 evidence、
  evidence/artifact digest 输出、aggregate report validation 摘要、缺 validation
  摘要在 optional/live 两种模式下的分类、override guard、默认非 live 不 ready、
  summary-only optional failure、mock live 条件满足但不 production ready、live
  必过 gate 失败等路径；聚合脚本只允许显式
  `TONGLINGYU_RELEASE_ALLOW_GATE_CMD_OVERRIDE=true` 使用 mock gate，且一旦使用
  override 报告会保持 `production_release_ready=false`，条件满足时的状态也会
  标为 `passed_with_gate_command_overrides`；报告 validator 也覆盖篡改 ready
  状态的负向路径。
- Open WebUI 已补 `tonglingyu_gateway_admin` Action Function：只读查询 Gateway
  metrics、trace、evidence package audit 和 session，Function 内强制
  `__user__.role == "admin"` 后才调用 Gateway admin read/update API；普通用户只会
  触发 `/v1/admin/access-denials` 写脱敏拒绝审计，不会读取或修改 admin 资源。
  已补 API/DB 两条安装路径、fixture/API/DB verify gate，并纳入 release
  readiness live gate。
- `deploy/scripts/test-openwebui-gateway-admin-action-contract.sh` 已补 Gateway
  Admin Action contract smoke，覆盖 Action 编译和单测、verify fixture 正向、
  admin key 为空、缺少 admin role guard、缺少 admin action endpoint，以及
  verify 输出不泄露 fixture-secret 值。
- release/readiness、runtime config、strict Gateway、Open WebUI Bridge Function
  和 Gateway Admin Action 的 install/verify 脚本已支持
  `TONGLINGYU_DEPLOY_ENV_FILE` / `DEPLOY_ENV_FILE`，实现分支可以复用目标部署
  `.env` 做只读 gate，不需要把密钥文件复制进工作树；已补
  `test-deploy-env-file-contract.sh` 验证显式 env-file、本地 `.env` fallback 和
  缺失文件错误不泄露 env 值。
- 已补 `deploy/scripts/ensure-tonglingyu-gateway-env.sh`，用于在备份后生成缺失的
  `TONGLINGYU_GATEWAY_API_KEY` / `TONGLINGYU_ADMIN_API_KEY`、关闭 Gateway key
  admin fallback，并把 Open WebUI provider key 第一项收敛为 Gateway service key；
  输出只包含变量名和状态。`test-tonglingyu-gateway-env-contract.sh` 覆盖
  check/apply/idempotent/重叠 key 拒绝、provider key 边界残留引号清理和输出不泄露
  生成值。
- 2026-05-11 已用目标 `.env` 先执行 `deploy/scripts/env-backup.sh backup`，
  再用 `ensure-tonglingyu-gateway-env.sh --apply` 补齐 Gateway service/admin
  credential、关闭 Gateway key admin fallback，并收敛 Open WebUI provider key；
  helper 输出未打印 secret value。
- 2026-05-11 已改用远程 `hhost` Docker 做 live gate：同步当前 compose、部署脚本
  和 `agent-platform` 源码到 `/home/simon/hermes-home-deploy`，不覆盖远端
  `.env`；远端 env guard `--check` 通过，`verify-tonglingyu-runtime-config.sh`
  通过，`tonglingyu-gateway` 重建后 `/healthz` 和 metrics 均显示
  `agent_runtime.mode=hermes`。
- 远端 `.env` 的 `OPEN_WEBUI_OPENAI_API_KEYS` 曾残留尾部双引号，导致 Open WebUI
  DB installer 无法 source env；已在运行 `env-backup.sh backup` 后用 env guard
  修复，并补 contract 防止再次残留边界引号。
- strict Gateway gate 曾因固定 `x-tonglingyu-message-id` 命中旧部署 dedupe 缓存；
  已改为每次 gate 生成唯一 chat/message id，避免把旧缓存误判为当前 Runtime
  live 结果。
- `tonglingyu-runtime` 已补 profile backend 失败审计：Hermes profile step 执行阶段
  如果后端失败或超时，会写 `agent_runtime_profile_execution_rejected`，记录
  `failure_stage=agent_runtime_step_execution`、profile step 计数和错误摘要，便于
  admin trace 追踪，而不是只暴露 HTTP 500。
- Runtime Agent live blocker 已收敛：Hermes tool step 现在会发送显式
  `tool_choice`，并且通灵玉 Runtime 会 host-enforce 必需只读工具观察；如果
  Hermes 未返回 tool result，则用已执行的确定性本地 step 输出补齐绑定
  trace/evidence/package 的 tool result 和审计事件，最终仍由本地 reviewer/治理层
  决定是否消费内容。
- Gateway Runtime Agent profile budget 已从硬编码 5 秒改为
  `TONGLINGYU_AGENT_RUNTIME_PROFILE_MAX_SECONDS`，compose 默认 `30` 秒；
  runtime config gate 会校验该值为正整数。
- 远端 Open WebUI Bridge Function 和 `tonglingyu_gateway_admin` Action 已通过 DB
  installer 更新并重启 `hermes-open-webui`；复测
  `verify-openwebui-function.sh` 与
  `verify-openwebui-gateway-admin-action.sh` 均通过。
- 最新远端 production readiness 显示 runtime config、model upstream network、
  strict Gateway、Open WebUI Bridge Function、Gateway Admin Action 和
  `openwebui_browser_review` 均已通过；`production_release_ready=true`。
- 已新增 `deploy/scripts/verify-model-upstream-network.sh`，release readiness
  live mode 会在 strict Gateway 之前运行该 gate；它从 `sub2api`/Hermes 容器内
  检查模型上游 DNS、198.18.0.0/15 fake-IP 和 TLS 握手状态，只输出 host、
  DNS class、HTTP/TLS 状态和错误摘要，不输出 credential；每个 URL 默认最多
  探测 3 次，降低瞬时 TLS reset 造成的假 release blocker。
- 远端 `hhost` 当前 model upstream gate 通过；`chatgpt.com` 仍可能解析到
  198.18.0.0/15 fake-IP，但 TLS/HTTP 可观测，因此该 gate 会作为网络层早期
  诊断，而不是替代 strict Gateway 的端到端契约。
- `agent-platform/Dockerfile` 和 `agent-platform/crates/tonglingyu-gateway/Dockerfile`
  的 BuildKit frontend 已从浮动 `docker/dockerfile:1.7` pin 到
  `docker/dockerfile:1.7.0`；远程 `hhost` build 已验证 `1.7.0` 可解析并完成
  `tonglingyu-gateway:formal` 构建。
- Open WebUI Function gate 已要求 Bridge secret、issuer 和 target model
  valves 非空，并补齐 `TARGET_MODELS` 安装/校验，避免 Function active/global
  但实际不注入 signed context 仍被 release gate 误判为通过。
- Open WebUI Function gate 已增加 `OPEN_WEBUI_FUNCTION_VERIFY_JSON` fixture
  模式，CI/本地可以不依赖真实 Open WebUI DB/API 直接覆盖 empty/missing valves
  等负向路径。
- Open WebUI Function API/DB 安装脚本已支持 `AGENT_BRIDGE_TARGET_MODELS`，
  避免 Filter 和 verify gate 已支持多 target model，但安装脚本仍覆盖成单值。
- 当前 R5D 生产入口基线已通过，但不能据此宣布本轮 RQA 或完整通灵玉
  production-ready：
  Hermes profile content/tool execution 已通过 `agent-runtime`/Hermes 接入并由
  summary/audit gate fail-closed；目标 `hhost` runtime config、model upstream
  network、strict Gateway、Open WebUI Bridge Function、Gateway Admin Action 和
  Open WebUI browser-side 单入口复测均已通过。最终 production report 为
  `/home/simon/hermes-home-deploy/tonglingyu-release-readiness-production.json`，
  browser review evidence 为
  `/home/simon/hermes-home-deploy/openwebui-browser-review/openwebui-browser-review.json`。
  事实源、证据包和最终 reviewer 裁决仍由 `tonglingyu-runtime` 本地治理强制约束。
- RQA Milestone A 已完成代码切片：
  `tonglingyu-runtime` 的 text/commentary search 输出已携带
  `RetrievalQualityReport`，覆盖 redacted query summary、candidate/selected
  count、channel distribution、protected terms、expanded aliases、
  required/missing evidence types、exact-match coverage、source coverage boundary、
  source license/usage/attribution refs、`expected_evidence_status` 和
  `truncated`；`cargo test -p tonglingyu-runtime` 已通过 32 个测试。该结果只证明
  runtime search 质量报告闭环，不证明后续
  RQA release gate、saved report validator 或 live KB 已 production-ready。
- RQA Milestone B 已完成代码切片：
  runtime schema 新增 `tonglingyu-retrieval-failures-v1` 和
  `retrieval_failures` 表；create/list/read/update API、默认分页与最大 page size、
  admin detail / safe summary、migration preflight、失败 rollback 测试和 workflow
  quality failure 自动登记均已接入。`cargo test -p tonglingyu-runtime` 已通过 35
  个测试。该结果仍不等于完整 production-ready，因为 Milestone C 的完整触发矩阵、
  去重、eval expected evidence、release gate 和 saved report validator 尚未完成。
- RQA Milestone C 的 runtime/API 层已完成代码切片：
  workflow 会为非 production-ready quality report、reviewer downgrade 和 package
  无法支持关键 claim 登记 failure；expected evidence miss 可由 eval/gate 调用方
  通过 expected/selected evidence ids 写入；相同 trace/package/failure type 会
  去重；failure insert 与 audit append 在同一事务中完成，audit append 失败会
  rollback。当前 runtime 回归套件已扩展到 42 个测试并通过。该结果仍不等于
  完整 production-ready，因为 eval quality metrics、release quality gate 和 saved
  report validator 尚未完成。
- RQA Milestone D 已完成代码切片并关闭当前 eval quality blocker：
  `tonglingyu-gateway eval` 已输出 `tonglingyu-eval-quality-v1` quality summary
  和 case-level quality details，覆盖 expected evidence classification、hit@1/@3/@8、
  required type、exact term、source/edition diversity、source coverage boundary、
  forbidden conclusion、reviewer status 和 eval failure 写入
  `retrieval_failures`。expected evidence 分母当前为 5，hit@8 为 5/5；新增
  影印件、权威校注本、专家校勘边界 case，并由 Runtime reviewer 降级为资料不足。
  `cargo test -p tonglingyu-runtime` 已通过 42 个测试，
  `cargo test -p tonglingyu-gateway` 已通过 21 个测试。
- 当前 RQA eval 按 production 口径通过：
  本地 live eval 生成 103/103 个 case report，103/103 case 通过，
  expected-passed / evidence-bearing report 的
  `quality_report_production_ready=86/86`，`quality_summary.status=passed`，
  blocker 为空，`eval_failure_records=0`。本轮修复包含 source license /
  usage / attribution metadata 入库与校验、version boundary 与程乙正文检索
  策略校准，以及脂批“原文”问题和“正文事实”问题的 reviewer 边界拆分。
- RQA Milestone E 已完成代码切片：
  Gateway admin trace 和 package audit 现在暴露 `retrieval_quality_summary`、
  `retrieval_failure_ids` 与 admin detail failure 列表；JSON metrics 新增
  `rqa.retrieval_failures.total/by_status/by_type`，Prometheus 只保留 bounded
  labels，覆盖 Gateway info、review status、retrieval failure status/type 和
  audit event type，不输出 trace/user/question/package id；新增 admin retrieval failure
  list/read/update API，并在 Open WebUI admin Action 中提供 list/read/update 入口。
  admin list/read/update 成功、not-found 和 conflict 路径都会写访问审计；
  admin auth failure、Open WebUI role denial 和 rate-limit denial 都写脱敏
  `rqa_admin_access_denied`。`admin update` 支持 `if_match_updated_at` 冲突检测，
  重复同 payload 且未带 CAS 时 runtime update no-op，不重复写状态更新 audit。
  普通 completion 和 streaming completion 均有 RQA 内部字段不可见测试。
  `cargo test -p tonglingyu-gateway` 已通过 36 个测试，Open WebUI admin Action
  单测已通过 10 个测试。
- RQA Milestone F/G 已完成 release artifact 主干实现：
  新增 `deploy/scripts/verify-tonglingyu-rqa-quality-gate.sh`，并把
  `retrieval_quality` 加入 `verify-tonglingyu-release-readiness.sh` 的 required
  gate；saved report validator 的 canonical gate set 也加入
  `retrieval_quality`。gate 会验证 eval quality summary、Production 默认阈值、
  open retrieval failures、source coverage boundary、source license/attribution
  metadata、source snapshot digest、KB build hash、kb_version、eval run id、
  runtime profile/prompt/tool/reviewer policy digest、model upstream id 和 decoding
  参数摘要，并输出可复核的 `eval_report_path`。当 release report path 已设置且
  未显式指定 eval report path 时，release readiness 会为真实 RQA gate 生成同目录
  `.rqa-eval.json` artifact，避免 production-ready report 绑定临时文件。
- RQA quality gate 已支持 `TONGLINGYU_RQA_THRESHOLD_*` 阈值配置；默认值仍是
  Production 默认阈值。更严格阈值会进入 `effective_thresholds` 和
  `threshold_config`，低于默认值或不可解析的覆盖会 fail-closed，不能生成
  production-ready artifact。新增 eval 指标
  `source_boundary_confirmation_avoided`，用于证明需要影印件、权威校注或专家
  校勘的问题没有被公共回答声明为已确认。
- RQA Milestone G 已完成 saved report validator 的 artifact 复核：
  validator 会读取 `eval_report_path`，校验 `eval_report_sha256`、`eval_run_id`
  和 `eval_suite_version`，并从原始 eval cases 重算 quality report coverage、
  production-ready quality report coverage、eval classification、expected
  evidence hit@1/@3/@8、required type coverage、exact term coverage、forbidden
  conclusion avoided、reviewer status matched、eval failure records 和 source
  boundary confirmation avoided、source diversity。为支持完整重算，
  `tonglingyu-gateway eval` 的 case result 已新增 `expected_review_status`、
  `required_evidence_type`、`quality.required_type_required`、
  `quality.source_boundary_confirmation_required` 和
  `quality.source_boundary_confirmation_avoided`。
- Saved report validator 还会扫描 release report 中的 raw question/query/prompt
  字段、stdout JSON 字符串泄露，以及 trace/package/evidence/block/case/user 等
  高基数 id 列表。contract smoke 已覆盖 RQA gate stdout 缺失、阈值被降低、
  open P0 tamper、eval artifact 缺失、summary tamper、privacy leak 和
  high-cardinality list leak。
- `deploy/scripts/verify-tonglingyu-strict-gateway.sh` 已输出与 RQA quality gate 同
  结构的 `behavior_config` 和 `behavior_config_digest`；saved report validator
  会逐字段比较 RQA eval gate 与 strict live gate 的 Runtime profile、prompt、
  tool policy、reviewer policy、model upstream 和 decoding 参数摘要。
- 当前本地 RQA quality gate 正确 fail-closed：`quality_summary.status=passed`，
  但 `data/tonglingyu/tonglingyu.db` 里仍有 157 个 open retrieval failures。对该
  DB 的临时副本执行真实 RQA gate 时，schema migration 会 backfill governance
  tasks，eval 运行后 gate 输出 `open_p0_retrieval_failures=182` 和
  `open_p0_governance_tasks=182`，因此 release readiness 报告会把
  `retrieval_quality` 记为 required failure，`production_release_ready=false`。这
  不是测试失败，而是当前现实状态还不能声明 RQA production-ready 的证据。
- RQA Milestone H 已完成第一批治理任务代码切片：
  runtime 新增 `knowledge_governance_tasks` schema、通用
  `source_entity_type/source_entity_id`、open/in_review retrieval failure backfill、
  新 failure 自动生成 open P0 governance task，以及 governance task
  create/list/read/update API；accepted task 必须带 reviewer、review note 和
  evidence ref，closed/rejected 必须带 reviewer 和 review note。Gateway 和 Open
  WebUI admin Action 新增 governance task list/read/create/update 与
  create-from-failure；管理员可把 trace、package 或 retrieval failure 标记为
  expert-review 任务。trace/package audit 会返回 governance task ids/tasks，admin
  成功、not-found、conflict 和 update 路径写访问审计。RQA quality gate、saved
  report validator 和 release contract smoke 新增 `open_p0_governance_tasks=0`
  blocker。
- 普通用户反馈入口已完成第一批代码切片：Gateway 新增 `/v1/feedback`，Open
  WebUI 新增 feedback Action；反馈必须绑定用户可访问的 trace/package，只生成
  `source_entity_type=user_feedback` 的 `expert_review` governance task，并写
  `user_feedback_received` audit，不接受事实层 mutation 字段。
- retrieval failure 聚类已完成第一批代码切片：Runtime 新增
  `tonglingyu-retrieval-failure-clusters-v1` 聚类结果，按 failure type、KB、
  missing/required evidence types 和 issue family 聚合 open/in_review failures；
  Gateway admin API 和 Open WebUI admin Action 可触发聚类，只生成
  `source_entity_type=retrieval_failure_cluster` 的 governance task proposed fix，
  并写 `retrieval_failures_clustered` / `retrieval_failure_admin_cluster` audit；
  不改 retrieval failure 状态或 source/alias/term/commentary/fact 表。
- 该 H 切片仍不等于 H 完成或 production-ready：proposed alias / term /
  commentary link / version note 的人工状态流转、KB diff report、eval 前后对比、
  retention/restore 和用户数据 lifecycle contract 仍未完成。
- 后续 RQA production-ready 还必须提供 RTO/RPO、最近一次恢复演练、恢复后 gate
  复核、依赖/镜像/发布脚本安全扫描摘要；缺失时不能生成 production-ready artifact。
- 后续 RQA production-ready 还必须把 RQA quality gate、saved report validator 和
  contract smoke 接入 CI 或 release automation 的强制路径；只靠人工本地命令不能
  作为最终发布证据。
- 后续 RQA production-ready 还必须定义 RQA 用户数据生命周期：export、
  delete/anonymize、retention、legal hold 和 audit tombstone；缺少策略版本或
  lifecycle contract smoke 时不能生成 production-ready artifact。

## 下一步

1. 清理或分派当前 open retrieval failures / open governance tasks，使 quality
   gate 的 blocker 能由真实治理状态关闭，而不是绕过阈值。
2. 实现 RQA Milestone H-J：治理任务/反馈闭环、端到端自动化、backup/restore、
   retention/prune、runbook/alert/rollback 和 production report 运维证据。
3. 补齐 RQA Milestone K-M：隐私生命周期、API 契约、性能预算、发布值守、
   回滚、事故响应、容量和审计完整性。
4. 补齐人物、关系、事件、诗词判词和评测题库的人工标注层。
5. 按证据校验与发布 QA 闸门后续再补充影印/权威校注本复核，不作为当前
   M2 loader 的默认前置项；当前版本继续保持“通俗分析优先”。
