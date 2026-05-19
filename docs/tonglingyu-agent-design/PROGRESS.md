# 通灵玉进展与决策记录

## 当前状态

- 主线已切到“通灵玉”第一版。
- 旧基础库产物和旧专用抽取脚本已删除。
- `scripts/extract_epub.py` 和 `scripts/download_wikisource.py` 已输出
  source snapshot，并保留 `rare_char_annotations`。
- `resources/styles/buhongjushi/` 风格转录保留，不作为主证据库。
- Rust 主线入口为 `agent-platform/crates/tonglingyu-gateway/`。
- 2026-05-17 仓库边界已收敛为通灵玉 Agent 系统：Rust workspace 只保留
  `agent-core`、`agent-runtime`、`tonglingyu-runtime` 和
  `tonglingyu-gateway`；旧 Agent Platform 控制面、Global Router、Postgres
  store、worker、agentctl、旧 Dockerfile 和旧设计文档已退出仓库主线。
- 2026-05-17 `deploy/docker-compose.yml` 已收敛为 Tonglingyu-only stack：
  `hermes`、`tonglingyu-gateway`、`open-webui`、`cloudflared`；Tonglingyu
  后端容器使用 `tonglingyu-hermes-agent` 和 `tonglingyu-gateway`，Open
  WebUI/Cloudflared 作为前置入口层使用 `home-open-webui` 和
  `home-cloudflared`，Open WebUI 只连接
  `http://tonglingyu-gateway:8090/v1`。
- 2026-05-17 已在 `hhost` 完成 Tonglingyu-only 重建：当前部署目录为
  `$HOME/tonglingyu-home-deploy`；Open WebUI 前置层运行时目录为
  `$HOME/huixiangdou-home-runtime`，Cloudflared 无本地 runtime data dir；
  Tonglingyu/Hermes 运行时目录为 `$HOME/tonglingyu-home-runtime`。运行容器为
  `tonglingyu-hermes-agent`、`tonglingyu-gateway`、`home-open-webui` 和
  `home-cloudflared`。
- `hhost` 重建后 runtime config、`agent_identity_bridge`、
  `tonglingyu_gateway_admin`、model-upstream probe、strict Gateway
  chat/streaming/admin trace 和公网 Open WebUI HTTP 200 均已通过复核。
- 2026-05-18 当前 `hhost` 版本已完成 production-ready release automation：
  `remote-release-20260518T181347Z-50849` 为 `status=ok`、
  `production_ready_proven=true`，release readiness 为 `status=passed`、
  `production_release_ready=true`，saved validator 为 `status=ok`、`errors=[]`。
  该结论覆盖当前 RQA release gate 和 Phase 1 scoped context，不覆盖 scoped memory
  或 Memory Collector。
- 2026-05-18 Phase 1 Scoped Context 已实现并部署：Gateway 请求路径写入
  `user_session`、`interaction_context`、`context_pack` 和 `session_journal`，
  支持 `resolved_question`、summary-only admin trace 回放和 fail-closed 追问解析。
  旧 `gateway_sessions` / `gateway_messages` 未迁移为新 memory 来源。
- 2026-05-18 `26_Scoped_Context与受控Memory设计.md` 已重构为稳定实现规格稿：
  固定解释顺序、数据对象、运行流程、Phase 1 工作包、禁止项、Phase 2-4 边界和
  production gate。当前 Phase 1 scoped context 已闭合；Phase 2 仍必须继续遵守
  active memory、Memory Collector、审核、ACL 和跨 scope memory 的边界，不能把
  Phase 1 结论扩展成 scoped memory production-ready。
- 2026-05-19 Phase 2 已细化为独立实现 checklist：
  `28_Phase2_Context_Aware_Runtime_Implementation_Checklist.md`。Phase 2 只做
  Context-aware Runtime，包括请求级 `context_pack`、consumer 级
  `context_projection`、projection ref/digest contract、consumer projection 隔离、
  tool policy digest、Runtime audit、replay 和 hhost production gate；不做 Memory
  Collector、`memory_candidate`、active `memory_card`、审核页面或非 Hermes
  external agent 接入。目标是 Phase 2 production-ready，不是本地代码切片完成；
  当前已完成实现和 hhost production gate。
- 2026-05-19 Phase 2 Context-aware Runtime 已部署为 `0.1.7` 并通过 `hhost`
  production-ready gate：`tonglingyu-gateway` 运行 image id 为
  `sha256:93df2e2555669a77097590eda1bb4b63e6cc709b58604f741fbf04ce1c6845ab`。
  live gate artifact 为
  `data/tonglingyu/remote-live-gates/remote-live-20260519T013236Z-78446/remote-live-gates.json`，
  其中 model upstream、Open WebUI Function、Open WebUI Admin Action、strict Gateway
  和 scoped context gate 均通过。完整远端 release automation artifact 为
  `data/tonglingyu/remote-release-automation/remote-release-20260519T013318Z-78823/remote-release-automation.json`，
  `status=ok`、`production_ready_proven=true`；release readiness 为
  `status=passed`、`production_release_ready=true`、`required_failures=[]`、
  `release_blockers=[]`，saved validator 为 `status=ok`、`errors=[]`，open P0
  retrieval failures / governance tasks 均为 0。该结论只覆盖 Phase 2
  Context-aware Runtime，仍不覆盖长期 memory、Memory Collector、审核页面、
  Context Governance 独立服务或非 Hermes external agent 接入。
- 2026-05-19 Phase 3 Memory Candidate workflow 已实现并部署为 `0.1.11`：
  `29_Phase3_Memory_Candidate_Implementation_Checklist.md` 已记录实现证据。
  Phase 3 覆盖 Memory Collector、`memory_candidate`、`memory_card`、完整状态机、
  admin-only CLI/API、background worker / scheduled / admin manual 三种触发方式和
  LLM participation fail-closed contract；状态机包含
  `approve/promote/reject/reclassify/expire/revoke/merge`。`hhost` 运行的
  `tonglingyu-gateway` image id 为
  `sha256:8fddab2d2d4213641cba382721844374af4ea09265a1b389f36ff6f788bc0109`。
  live gate artifact 为
  `data/tonglingyu/remote-live-gates/remote-live-20260519T082735Z-42867/remote-live-gates.json`。
  完整远端 release automation artifact 为
  `data/tonglingyu/remote-release-automation/remote-release-20260519T084157Z-43947/remote-release-automation.stdout`，
  `status=ok`、`production_ready=true`；wrapper 为
  `production_ready_proven=true`、`release_blockers=[]`、`required_failures=[]`；
  release readiness 为 `status=passed`、`production_release_ready=true`。容量 gate
  为 `rqa_write_p95_ms=4553`、`admin_read_p95_ms=382`、
  `metrics_read_p95_ms=162`，post-release monitor 60 分钟窗口
  `sample_count=13`、`failed_sample_count=0`。background worker 已在 hhost
  自动运行，最终日志为 `processed_count=60`、`candidate_count=0`、
  `denied_count=0`、`suppressed_count=60`。该结论只覆盖 Phase 3
  candidate/card 工作流；自动 promotion 和 active memory 读取路径仍放在 Phase 4，
  不声明 scoped memory production-ready。
- 2026-05-19 Phase 3 设计反思后已把 memory lifecycle 重构为三层：candidate
  lifecycle、card lifecycle 和 read enablement lifecycle。`reclassify` 是
  `pending -> pending` 的 action，不再作为独立状态；人工 `promote` 是
  `approved candidate -> active memory_card` 的跨对象 transition，但 Phase 3 固定
  `read_enabled=false`。Phase 4 不再重新定义候选/卡片状态机，只负责打开 ACL 约束下
  的读取面、自动 promotion 和完整 scoped memory production gate。
- 2026-05-19 Phase 4 已按最新讨论重构为 Scoped Memory Production 设计，不再把目标
  降为最小 user_private 闭环或 collector smoke。`30_Phase4_Scoped_Memory_Production_Checklist.md`
  已冻结 Phase 4 口径：自动策略是一等生产主路径，人工审核流程保留但可被策略跳过；
  LLM 只做 semantic filter，最终 auto approve / promote / read enablement 由
  versioned policy engine 决定；主链路必须覆盖
  `session_journal -> Memory Collector -> memory_candidate -> policy decision ->
  memory_card -> read enablement -> context_pack.memory_read_refs -> context_projection ->
  Runtime answer`。该更新只是设计冻结，不声明 scoped memory production-ready。
- 2026-05-19 Phase 4 policy contract 已冻结为 `scoped-memory-policy-v1`：默认
  `policy_mode=auto_policy`，保留 `shadow_only` 和 `manual_required` 降级；
  scope automation matrix、confidence threshold、TTL、candidate type allowlist、
  LLM schema `scoped-memory-llm-filter-v1`、LLM overreach fail-closed 字段和
  context read budget 均已写入 checklist。实现不得临场硬编码阈值或以补丁绕过
  policy contract；任何生产策略调整都必须形成新 policy version 并重新通过 release
  gate。

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
- 早期远程部署曾复用现有 Open WebUI，Gateway 单独部署；2026-05-17
  重建后改为 Tonglingyu-only stack，Open WebUI、Gateway、Hermes 和
  Cloudflare Tunnel 均在新的 `tonglingyu-home` compose 项目内运行。
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
- 2026-05-09 已在远程 `hhost:~/hermes-home-deploy` 真实部署：
  启动 `tonglingyu-gateway` 和现有 `hermes-open-webui`，远端 gateway
  healthcheck 为 healthy。
- `tonglingyu-gateway` 已拆为独立镜像，使用
  `agent-platform/crates/tonglingyu-gateway/Dockerfile` 构建，并通过
  BuildKit cache mount 缓存 Cargo registry、git 源和 `target/`。
- 远端已验证第二次 `docker compose build tonglingyu-gateway` 全部命中
  Docker/BuildKit 缓存；`tonglingyu-gateway:formal` 含 gateway 二进制。
- 旧 Global Router 不再进入当前生产路径；Open WebUI 的目标生产入口是
  `tonglingyu-gateway`。
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
- `deploy/scripts/test-openwebui-gateway-admin-action-contract.sh` 已升级为结构化
  release gate `openwebui_admin_action_contract`：编译 Open WebUI Admin/Feedback
  Action，运行 21 个 Action 单测，验证 fixture 正向、admin key 为空、缺少
  admin role guard、缺少 admin action endpoint、required Action 列表和 verify
  输出不泄露 fixture-secret 值；saved report validator 会拒绝缺 gate stdout、
  guard check 失败或 required Action 覆盖被篡改的 production-ready report。
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
  和 `agent-platform` 源码到 `$HOME/hermes-home-deploy`，不覆盖远端
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
- 此前 R5D 生产入口 readiness 显示 runtime config、model upstream network、
  strict Gateway、Open WebUI Bridge Function、Gateway Admin Action 和
  `openwebui_browser_review` 通过；该记录只证明 R5D 入口基线，不能替代本轮
  RQA release automation / security / ops / capacity 生产门禁。
- 已新增 `deploy/scripts/verify-model-upstream-network.sh`，release readiness
  live mode 会在 strict Gateway 之前运行该 gate；它从 `sub2api`/Hermes 容器内
  检查模型上游 DNS、198.18.0.0/15 fake-IP 和 TLS 握手状态，只输出 host、
  DNS class、HTTP/TLS 状态和错误摘要，不输出 credential；每个 URL 默认最多
  探测 3 次，降低瞬时 TLS reset 造成的假 release blocker。
- 远端 `hhost` 当前 model upstream gate 通过；`chatgpt.com` 仍可能解析到
  198.18.0.0/15 fake-IP，但 TLS/HTTP 可观测，因此该 gate 会作为网络层早期
  诊断，而不是替代 strict Gateway 的端到端契约。
- `agent-platform/crates/tonglingyu-gateway/Dockerfile` 的 BuildKit frontend 已从
  浮动 `docker/dockerfile:1.7` pin 到 `docker/dockerfile:1.7.0`；远程
  `hhost` build 已验证 `1.7.0` 可解析并完成 `tonglingyu-gateway:formal`
  构建。
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
  `$HOME/hermes-home-deploy/tonglingyu-release-readiness-production.json`，
  browser review evidence 为
  `$HOME/hermes-home-deploy/openwebui-browser-review/openwebui-browser-review.json`。
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
  策略校准、脂批“原文”问题和“正文事实”问题的 reviewer 边界拆分。
  2026-05-16 进一步修正通灵玉铭文与青埂峰核心 eval target：通灵玉“字”
  问题会强制 exact term 保护 `莫失莫忘` 和 `一除邪祟`，青埂问题会强制
  exact term 保护 `青埂`；exact-text fallback 优先返回
  `hongloumeng-wikisource-120` source snapshot，避免被更短的程甲/程乙文本抢占。
  `tonglingyu-gateway eval` 也已改为默认在 SQLite 临时副本上运行；只有显式设置
  `--allow-db-mutation` / `TONGLINGYU_EVAL_ALLOW_DB_MUTATION=true` 才允许把 eval
  workflow 写回目标 DB。
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
- 2026-05-15 本地 RQA quality gate 曾因旧 eval artifact 正确 fail-closed：
  `quality_summary.status=passed`，但默认 DB 存在 open P0 retrieval failures；
  schema migration backfill 后同时出现 open P0 governance tasks。2026-05-16
  执行 eval artifact remediation 并复跑 preflight 后，`retrieval_quality`
  输出 open P0 failure/task 为 0 并通过。该节点的当前 blocker 已关闭，但目标 live
  DB 仍必须单独证明没有 open P0 RQA blocker。
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
- proposed alias / term / commentary link / version note 的人工状态流转已完成第一批
  代码切片：Runtime 新增 `tonglingyu-knowledge-patch-proposals-v1` 和
  `knowledge_patch_proposals`，proposal 必须有类型化 payload、payload hash、
  source ref、trace/package 绑定，并创建
  `source_entity_type=knowledge_patch_proposal` 的 governance task；Gateway admin
  API 和 Open WebUI admin Action 可创建 proposal，并写
  `knowledge_patch_proposal_created` / `knowledge_patch_proposal_admin_create` audit。
  accepted/rejected 仍只更新人工状态，不直接写 source、alias、term、commentary
  link、version note 或事实层。
- KB rebuild diff report 与 eval 前后对比已完成第一批代码切片：Runtime 新增
  `tonglingyu-kb-version-diff-v1` 和 `kb_version_diff_reports`，rebuild 会记录
  before/after KB summary、source hash 变化、count delta、source snapshot digest
  和 KB build hash；Gateway `build-kb` 默认在临时 SQLite 副本上运行 rebuild 前后
  eval，只把 `quality_summary` 写回 diff report，post-rebuild eval 不通过时
  fail-closed，且不把 eval package/failure 污染 live DB。
- accepted knowledge patch proposal 已接入 KB rebuild 输入：rebuild 只应用
  `accepted` 且带 evidence ref 的 proposal，按类型写入 aliases、terms、
  commentary_links 或 version_notes，并记录 `knowledge_patch_applications` 与
  `knowledge_patch_proposals_applied` audit；不满足目标表约束或引用不存在时
  rebuild fail-closed。
- RQA Milestone H 已完成，但不等于整体 production-ready：Milestone I-J-K 的端到端
  自动化、live release 证据、真实安全扫描 artifact、live/load 容量和值守仍未完成；
  用户数据 lifecycle 已有本地 gate，但还不能替代 live/Open WebUI admin Action
  证据。accepted 状态本身仍不能直接等同为事实层已更新，必须经过 rebuild
  application、diff report 和 eval gate。
- RQA retention/prune 已完成第一批 production 保护切片：Runtime 新增
  `tonglingyu-rqa-lifecycle-v1` 和 `rqa_lifecycle_tombstones`，prune 会保护仍被
  open retrieval failure、open/in_review/accepted governance task 引用的
  trace/package 数据；Gateway prune 同步保护对应 `gateway_messages`、
  `workflow_states` 和 session。实际删除前写 tombstone，完成后写
  `rqa_retention_pruned` audit；actual prune 会在写事务内重新判定候选集合再删除，
  避免判定后新增活动 RQA 引用造成误删。单测覆盖 dry-run、实际 prune、active
  RQA 引用保护和 tombstone payload 不含原始 question/secret。
- Gateway Prometheus `tonglingyu_gateway_info` 已恢复低基数运行边界标签：
  `agent_runtime_mode`、`rate_limit_per_minute` 和 `max_body_bytes`，使 gateway
  smoke 能复核限流与请求体边界没有从指标中丢失。
- RQA backup/restore drill 已接入 release readiness 必跑 gate：
  `deploy/scripts/verify-tonglingyu-rqa-backup-restore-drill.sh` 会执行 DB backup、
  restored DB integrity check、restored Gateway admin trace/failure/governance
  task/package 读取、package replay、恢复后 RQA quality gate 和 saved report
  validator 复跑。默认 RTO/RPO 为 900s / 3600s，gate stdout 写入
  started_at、finished_at、operator、environment、RTO/RPO、artifact hash 和
  post-restore checks。live mode 现在默认把 restore-drill 备份证据持久化到
  `data/tonglingyu/restore-drills/<run_id>/`，也可通过
  `TONGLINGYU_RQA_RESTORE_DRILL_ARTIFACT_DIR` 显式绑定 artifact 目录。
- saved report validator 已要求 `rqa_backup_restore_drill` gate 存在并校验
  RTO/RPO、backup/restore hash、恢复后 checks；production-ready report 若使用
  fixture-only restore drill、缺失持久备份 artifact，或备份 artifact 内容 hash
  与 gate stdout 不一致会失败，live release 必须提供真实 trace/package/failure/
  governance task restore refs。
- Release security gate 已接入 release readiness 必跑路径：
  `deploy/scripts/verify-tonglingyu-release-security.sh` 会记录 dependency scan、
  image scan、release script static scan 和 risk acceptance。缺依赖扫描、镜像扫描、
  镜像 digest、存在 `latest/main` 等可变 tag 或未审批风险时 fail-closed；已审批
  risk exception 必须包含 risk owner、accepted risk id、approved/expires 时间和
  accepted findings。saved report validator 会拒绝缺 `security_scan` gate stdout、
  缺 scan 且无 risk acceptance、release script finding 等篡改。
- RQA performance budget gate 已接入 release readiness 必跑路径：
  `deploy/scripts/verify-tonglingyu-rqa-performance-budget.sh` 会启动本地 Gateway，
  真实执行 chat 写入 RQA failure/governance task、admin trace/list、admin 状态
  关闭和 RQA quality gate 复跑；默认预算覆盖 RQA 写入、admin 查询、状态更新和
  quality gate，curl、KB build、eval 和 quality gate 都有可配置 timeout。saved
  report validator 会拒绝缺 `rqa_performance_budget` gate stdout、缺 timeout
  边界、预算超限、budget/measurement 不一致和关键 checks 未通过的
  production-ready report。
- RQA API contract gate 已接入 release readiness 必跑路径：
  `deploy/scripts/verify-tonglingyu-rqa-api-contract.sh` 会启动本地 Gateway，验证
  retrieval failure 与 governance task 的 admin list/read schema version、pagination
  metadata、max page size clamp、稳定排序、未知 filter 和非法 enum filter 的 400
  边界，以及旧客户端解析、响应新增字段容忍、RQA admin mutation 未知 request body
  字段 422 拒绝、admin metrics/Prometheus 不输出原始 prompt、trace/package id 或
  secret、Prometheus label set 只使用低基数字段、admin payload 不返回完整原始
  prompt。
  兼容策略版本为
  `tonglingyu-rqa-api-compatibility-v1`；saved report validator 会拒绝缺 gate
  stdout、contract check 失败、兼容策略漂移、metrics 边界漂移、未知 request body
  未拒绝和负向状态码不是 400 的 production-ready report。
- RQA failure 隐私 schema 已接入 runtime migration：
  `tonglingyu-retrieval-failure-privacy-v1` 新增 `redacted_question_excerpt`，
  `retrieval_failures` 默认只存 `question_sha256`、`question_summary`、
  `redacted_question_excerpt` 和 `redacted_query_terms_json`，不接受 raw `question`
  列。redaction 覆盖 password/key、token、URL secret、邮箱、手机号和长随机串；
  RQA API contract gate 会验证 admin detail 不回显原始 prompt 或敏感片段。
- RQA 用户数据生命周期 gate 已接入 release readiness 必跑路径：
  `deploy/scripts/verify-tonglingyu-rqa-user-lifecycle.sh` 会启动本地 Gateway，验证
  export 脱敏 manifest、legal hold 阻断 anonymize、release legal hold、
  delete/anonymize、audit event、tombstone、原始用户值移除和
  trace/package/failure/task 可追责性；gate stdout 只输出计数和 hash ref，不输出
  原始 question、response、user_ref、chat_ref 或 secret。saved report validator
  会拒绝缺 `rqa_user_lifecycle` gate stdout、关键 check 失败和 action status drift
  的 production-ready report。
- Open WebUI admin Action source/fixture contract 现在要求 Action 单测覆盖
  retrieval failure / governance task list 响应保留 `schema_version`、`limit`、
  `offset` 和 `next_offset`；saved report validator 会拒绝缺少该 Action 响应契约
  check 的 production-ready report。
- RQA Milestone K 的本地 contract / release gate 切片已闭合：隐私 schema、
  API 兼容策略、稳定分页、metrics 低基数、Open WebUI admin Action source/fixture
  contract 和性能预算 fail-closed gate 均已纳入本地验证。但这仍不是整体
  production-ready，因为 live Action、目标环境 live/load 性能和 operator handoff
  证据尚未闭合。
- RQA Milestone L 已新增发布值守 gate：
  `deploy/runbooks/tonglingyu-rqa-release-runbook.md` 记录 release flow、
  migration preflight、backup、deploy、live gate、saved report validation、
  rollback、DB restore/additive downgrade、RTO/RPO、alert policy、incident
  response、post-release monitor 和 release report reproduction；
  `deploy/scripts/verify-tonglingyu-release-ops-readiness.sh` 接入
  `verify-tonglingyu-release-readiness.sh` 的 required gate
  `release_ops_readiness`。preflight 模式可验证 runbook/alert/rollback 结构；
  live 模式缺 rollback/RTO-RPO/alert/post-release/operator/environment/report
  evidence 会 fail-closed。saved report validator 会拒绝缺
  `release_ops_readiness` stdout、缺 post-release live gate ref 或高基数告警标签
  的 production-ready report。
- RQA Milestone M 的本地 incident/capacity gate 已开始落地：
  Runtime/Gateway 管理员状态更新现在把 previous status、new status、reason
  hash 和 timestamp 写入 status-history audit；新增
  `deploy/scripts/verify-tonglingyu-rqa-incident-capacity.sh` 并接入 release
  readiness required gate `rqa_incident_capacity`。该 gate 在 preflight 模式只
  证明 emergency/degraded fail-closed 规则、无无界队列静态检查、幂等标记、
  status-history audit 标记和 incident runbook 结构；live 模式缺 capacity、
  load、audit-history 或 incident evidence 会 fail-closed。saved report
  validator 会拒绝缺 gate stdout、emergency/degraded/persistence-degraded
  状态、非 live 模式、缺代表性数量、缺 load measurement 或缺 incident/audit
  evidence 的 production-ready report。
- RQA backup/restore drill 的嵌套 release report 已同步新增 gate 边界：
  恢复后 report 会显式处理 performance、API、lifecycle、security、ops、
  incident/capacity 和 Open WebUI admin Action contract gates，避免新增 gate
  反向打断恢复演练；contract 已覆盖该边界。2026-05-16 持久 artifact 加固后
  曾因恢复后 RQA eval 失败 fail-closed；本轮修正 exact-term / source-priority
  检索后，本地 eval 重新达到 103/103，expected_evidence_hit@8 为 5/5，
  exact_term_coverage 为 3/3。随后用真实 `existing_refs` 重新运行恢复演练，
  恢复后 admin trace、retrieval failure、governance task、package replay、
  RQA quality gate 和 saved report validator 均通过，RTO/RPO 约为
  45s / 45s，低于 900s / 3600s 目标。
- 2026-05-16 带 digest image refs 和 fixture image scan 的 preflight release
  readiness 可消除 required gate failure：runtime config 静态 compose/env 解析通过，
  默认 RQA DB 的旧 eval artifact 已审计关闭，`retrieval_quality` open P0
  failure/task 为 0，`security_scan` 在该 fixture 配置下通过。仍不能声明
  production-ready：live mode 未开启，model upstream、strict Gateway、
  Open WebUI Function、Open WebUI admin Action 仍 skipped，browser review 尚未确认。
- 2026-05-16 默认本地配置复跑 release readiness：新增
  `rqa_migration_preflight` 通过并写入 release manifest / artifact registry；
  saved report validator 对失败报告校验通过；但 `security_scan` 因本地 image refs
  未 digest-pinned 且缺 image scan 证据失败，live gates 仍 skipped。因此默认本地
  状态仍是 fail-closed，不是 production-ready。
- 2026-05-16 全量 `docs/tonglingyu-agent-design/*.md` markdownlint 已通过：
  历史表格分隔行已规范化，重复标题规则改为同一父级内不重复，未改变通灵玉
  设计文档的正文语义。
- RQA quality gate 生成 eval report 时已改为先创建 SQLite snapshot，并在 snapshot
  上运行 eval；gate 仍从 live DB 检查发布前真实 open P0 failure/task。这样 release
  eval 的负向/降级用例不会写入生产 RQA 队列。2026-05-16 已用干净 KB 验证：
  quality gate 通过后原 DB 的 retrieval_failures 和 governance tasks 仍为 0。
- 已新增 `deploy/scripts/verify-tonglingyu-rqa-release-automation.sh` 作为 RQA
  release automation wrapper：强制串联 release readiness contract smoke、release
  readiness report 和 saved report validator，并记录 run id、git commit、gate
  summary 和 artifact hash。当前执行结果按预期 fail-closed，因为 release readiness
  阻塞仍未关闭。
- Release automation wrapper 现在默认把 automation report、release readiness
  report 和 saved report validator JSON 写入
  `data/tonglingyu/release-artifacts/<run_id>/`；production-ready 结论会要求这些
  证据不在脚本临时工作目录内，避免真实发布通过后核心 artifact 被 cleanup 删除。
- Release readiness report 已新增 `tonglingyu.release_manifest` 和
  `release_manifest_digest`：manifest 绑定 git commit / tracked dirty 状态、
  runtime config digest、RQA schema、eval suite、eval run id、source snapshot
  digest、KB build hash、kb_version、source license summary digest、behavior
  config digest、model upstream、decoding 参数摘要、dependency scan hash、
  digest-pinned image refs、image inventory hash 和 per-image scan report hash。
  saved report validator 会重算 manifest digest，并反查 manifest 与
  `runtime_config`、`retrieval_quality` 和 `security_scan` gate stdout 一致；
  contract smoke 已覆盖 manifest source snapshot 篡改和 manifest digest 篡改。
- Release readiness report 已新增 `tonglingyu.release_artifact_registry` 和
  `release_artifact_registry_digest`：registry 按
  `tonglingyu-release-artifact-registry-v1` 记录 release manifest、runtime
  config、RQA eval report、source license summary、behavior config、dependency
  scan、image inventory、image scan reports 和 browser review evidence 的
  digest/source gate/ref/path、365 天保留策略与 legal hold 支持。saved report
  validator 会重算 registry digest，并拒绝 production-ready report 缺关键 registry
  entry；release automation artifact 已记录 registry digest 和 entry count。
- Release readiness 已新增 `rqa_migration_preflight` 必跑 gate：
  `tonglingyu-gateway runtime-schema-preflight` 暴露 runtime schema preflight；
  `verify-tonglingyu-rqa-migration-preflight.sh` 先执行 SQLite 只读 backup，再输出
  backup path/hash、source DB hash、schema preflight digest、migration count 和
  no-secret/no-rebuild/no-delete 检查。release manifest / artifact registry 已绑定
  migration backup 与 preflight digest；saved report validator 会拒绝缺 gate
  stdout、缺备份路径、preflight digest 不匹配、production-ready 使用非 live
  preflight 或 pending migration 未清零的报告。2026-05-16 contract smoke、
  fixture preflight gate、非 live readiness saved report validation 和
  `cargo check -p tonglingyu-gateway` 已通过。
- Release readiness report 已新增 `tonglingyu.release_context` 和
  `release_context_digest`：报告会绑定 environment、target、generated_at、
  valid_until、validity_hours、require_live 和 context source；artifact registry
  记录 release context digest。saved report validator 会拒绝缺 context、context
  digest 漂移、generated_at 不一致、无效有效期、production-ready 过期、live
  模式未显式绑定目标环境或使用 local/preflight/test/fixture 环境名的报告；
  contract smoke 已覆盖缺 context 和无效 validity window；默认非 live readiness
  失败报告已通过 saved report validator 结构校验。该机制已闭合，但目标
  production-ready 仍必须由真实 live release run 生成并保存仍有效的报告 artifact。
- Release readiness report 已新增 `tonglingyu.release_runtime_identity` 和
  `release_runtime_identity_digest`：strict Gateway live gate 会采集当前 Docker
  Compose 运行镜像 inventory；release report 将该 inventory 与 git commit、
  tracked dirty 状态、security image inventory、migration preflight mode/count/hash
  共同写入 runtime identity，并在 artifact registry 记录 digest。saved report
  validator 会拒绝缺 runtime identity、identity digest 漂移、live migration 不是
  live、pending migration 未清零、tracked tree 不干净或缺
  `tonglingyu-gateway` / `open-webui` 运行镜像的 production-ready report。2026-05-16
  contract smoke 已覆盖缺 runtime identity 和缺运行镜像 inventory；机制已闭合，
  但目标 production-ready 仍必须由真实 live release run 生成并保存 runtime
  identity artifact。
- Strict Gateway live gate 已新增 `behavior_config_binding`：把
  `behavior_config_digest` 与普通 chat admin trace、streaming admin trace 的
  `agent_runtime_summary` digest 绑定，并要求 summary 证明 Hermes content
  execution complete、local governance enforced、tool result / tool audit 计数
  一致。saved report validator 会拒绝缺 binding 或 binding 与 strict Gateway
  行为配置不一致的 production-ready report；contract smoke 已覆盖 binding digest
  篡改。目标 production-ready 仍必须由真实 live strict Gateway gate 产出该证据。
- Live Open WebUI admin Action gate 已升级为结构化
  `tonglingyu.openwebui_admin_action_live_gate`：输出 active/global、valve keys、
  admin role guard、role denied、RQA admin Action 覆盖、Gateway admin API path
  覆盖、target model 绑定和 secret 输出边界。saved report validator 会拒绝
  role guard 缺失、RQA Action/API 覆盖不完整或 valves 未绑定的 production-ready
  report；contract smoke 已覆盖 live Action 权限边界篡改。目标 production-ready
  仍必须由真实 live Open WebUI admin Action gate 产出该证据。
- Strict Gateway live gate 已新增 `metrics_privacy`：递归检查 JSON metrics 是否
  出现 query/question/trace/package/session/user 等高基数字段，检查 Prometheus
  是否出现 query、trace、package、session、user 或鉴权 label，并确认已知 secret
  值没有进入 metrics 输出。saved report validator 会拒绝 metrics privacy 摘要
  缺失或含敏感 token 的 production-ready report；contract smoke 已覆盖
  Prometheus sensitive token 篡改。
- 已新增 `deploy/scripts/remediate-tonglingyu-rqa-eval-artifacts.sh` 处理旧版
  live DB eval 污染：脚本只选择 `eval-tly-*` trace 的 open/in_review RQA
  failure 和关联 governance task，apply 前备份 DB，事务内关闭状态并写
  status-history audit。2026-05-16 已对本地默认 RQA DB 执行一次 remediation，
  审计关闭 182 个旧 eval failure 和 182 个 governance tasks；备份路径为
  `data/tonglingyu/backups/rqa-eval-artifact-remediation-20260515T205220Z.db`。
  本轮 eval 修正后又审计关闭 25 个 `eval-tly-*` failure 和 25 个 governance
  tasks，备份路径为
  `data/tonglingyu/backups/rqa-eval-artifact-remediation-20260516T005451Z.db`，
  备份 SHA-256 为
  `00426cf0fe5c17e8946e0704de4f82bfd9ab41c4d16fcaff5172957affdd8d22`。在修正 eval
  CLI 默认 snapshot-copy 前，手动复测又产生一批 25/25 eval artifact，已同样按
  remediation 策略审计关闭；备份为
  `data/tonglingyu/backups/rqa-eval-artifact-remediation-20260516T010554Z.db`，
  备份 SHA-256 为
  `3d668a2549d1125f36d6b979a6dcf99fc2bd0600082cb2d0e99370dee33ca45a`。
- 2026-05-16 以 digest image refs 和 fixture image scan 复跑 preflight release
  readiness 后，`runtime_config`、`retrieval_quality`、
  `rqa_backup_restore_drill` 和 `security_scan` 均已通过；`required_failures=[]`。
  browser review 仍未执行，因此仍不能声明 production-ready。
- 2026-05-16 已新增 `deploy/scripts/sync-tonglingyu-remote-release-tools.sh`：
  它通过 SSH/rsync 同步当前 `scripts/`、runbook、Open WebUI Function、
  `agent-platform` 源码和 `resources` 到 `hhost`，不覆盖远端 `.env`；同时从正在
  运行的 `tonglingyu-gateway` 容器复制 Gateway 二进制到
  `agent-platform/target/debug/tonglingyu-gateway`，并写入不含 secret 的
  `.tonglingyu-release-tool-env`，让远端无 Rust toolchain 时也能运行当前 RQA
  gates。最新同步 artifact 为
  `data/tonglingyu/remote-release-tools/remote-tools-20260516T020637Z-81768/remote-release-tools-sync.json`。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-remote-live-gates.sh`
  作为本机缺 Docker CLI 时的 SSH 远端 live gate evidence collector。同步当前
  release 工具、升级远端 `tonglingyu_gateway_admin` Action、重建并重启
  `tonglingyu-gateway:formal` 后，远端运行镜像 digest 为
  `sha256:a117351dd4436659b7f3d9abd3be43a95b6f06dbaa5b9b6325693b92c8f774b8`，
  最新 live gate artifact 为
  `data/tonglingyu/remote-live-gates/remote-live-20260516T020657Z-81956/remote-live-gates.json`；
  `model_upstream_network`、`openwebui_function`、`openwebui_admin_action` 和
  `strict_gateway` 均通过。该 artifact 仍只证明基础 live gates 通过，不能替代
  完整 live release automation / release report 绑定。
- 2026-05-16 已新增 `deploy/scripts/verify-tonglingyu-remote-release-automation.sh`：
  本机缺 Docker CLI 时可通过 SSH 在 `hhost` 执行完整 live release automation，
  注入本地源 commit/dirty 状态，绑定目标 live DB、pre-migration backup、远端
  artifact 目录，并把 release automation / release readiness / saved validator
  artifact 回收到本地。同步当前工具后，最新 artifact：
  `data/tonglingyu/remote-release-automation/remote-release-20260516T050324Z-37074/remote-release-automation.json`。
  该 run 证明目标 DB 的 open retrieval failures / governance tasks 均为 0，
  `restore_ref_available=true`，contract smoke 和 saved report validator 通过，且
  `runtime_config`、`rqa_migration_preflight`、
  `retrieval_quality`、`rqa_backup_restore_drill`、`rqa_performance_budget`、
  `rqa_api_contract`、`rqa_user_lifecycle`、`openwebui_admin_action_contract`、
  `model_upstream_network`、`strict_gateway`、`openwebui_function` 和
  `openwebui_admin_action` 已在 live automation 中通过。restore drill stdout
  记录 `source_mode=existing_refs`、`backup.execution_mode=docker`，并重新跑过
  RQA quality gate 与 saved report validator。当前仍 fail-closed：
  `security_scan` 已有真实依赖/镜像扫描，但 6 镜像 aggregate Trivy report 存在
  未审批 high/critical findings；`release_ops_readiness` 缺 post-release monitor
  evidence，`rqa_incident_capacity` 缺 capacity/load 与 incident/audit live evidence，
  `openwebui_browser_review` 未确认。提交后复跑已确认
  `tracked worktree must be clean for live release` blocker 消失。
- 2026-05-16 为 live restore drill 新增闭环 canary 路径：
  `tonglingyu-gateway rqa-restore-canary` 使用 Runtime API 写入
  `restore_drill_canary` retrieval failure 和关联 governance task，并在同一事务中
  置为 `resolved` / `closed`、priority=`p1`，不留下 open P0；
  `deploy/scripts/prepare-tonglingyu-rqa-restore-canary.sh` 会先备份 live DB，host
  权限不足时通过 `docker compose exec tonglingyu-gateway` 在容器内执行。`hhost`
  canary artifact 为
  `$HOME/huixiangdou-home-runtime/data/tonglingyu/restore-canaries/20260516T030746Z-1265117/restore-canary-prepare.json`。
  restore drill 本身也已支持容器内备份并 `docker compose cp` 回 gate artifact，
  避免 root-owned live DB 使 host tool 误判失败。
- 2026-05-16 远端 live DB 暴露出旧 KB schema：`sources` 缺
  `source_url`、`license`、`license_url`、`license_source_url`、
  `attribution` 和 `usage_boundary`，导致 live RQA eval fail-closed。已新增
  `tonglingyu-gateway kb-source-metadata-backfill` 和
  `deploy/scripts/remediate-tonglingyu-kb-source-metadata.sh`，并在 `hhost` 容器内
  执行 additive backfill；备份保存在
  `hhost:~/hermes-home-deploy/data/tonglingyu/kb-source-metadata/kb-source-metadata-20260516T023622Z-1242044/live-db-before-kb-source-metadata.db`，
  backfill 报告显示 6 个 metadata 列补齐、5 个 source 更新、缺失值为 0。随后
  目标 live RQA quality gate 通过，`expected_evidence_hit@8=5/5`、
  `quality_report_coverage=103/103`、open P0 failure/task 为 0。远端 gateway
  已重建并重启到包含该 CLI 的 `tonglingyu-gateway:formal`
  (`sha256:f1e27233696cd2282f269d3d1a68085fefa5e972588a042960ffd89139c70b55`)。
- Security gate 已支持生产 digest-pinned image refs：compose 可通过
  `TONGLINGYU_GATEWAY_IMAGE_REF`、`HERMES_IMAGE_REF`、`OPEN_WEBUI_IMAGE_REF` 和
  `CLOUDFLARED_IMAGE_REF` 绑定 immutable digest；security gate 会读取 deploy env
  后解析 image refs 并检查 mutable tag / digest missing。2026-05-17 仓库已删除
  旧 Agent Platform Postgres/store/JWT 链路；真实 `cargo-audit` 扫描应只覆盖当前
  Tonglingyu workspace。当前 security gate 已用真实 dependency scan、fixture image
  scan 和 digest refs 通过；真实 Trivy 路径会把 per-image raw
  JSON 持久化到 `data/tonglingyu/security-image-scans/<run_id>/` 或显式
  `TONGLINGYU_RELEASE_SECURITY_IMAGE_SCAN_ARTIFACT_DIR`，并解析每个 image
  report 的 HIGH/CRITICAL vulnerability。image scan artifact 已绑定当前
  compose image inventory hash、per-image report content hash、raw report
  path hash 和 raw report artifact dir；saved report validator 会重算 raw
  report path/content digest，并拒绝 scan refs 与 release refs 不一致、raw
  report 缺失或 raw report 不可读取的 production-ready 报告。
- 2026-05-16 已把 `tonglingyu-gateway` runtime image 切到 Chainguard
  `glibc-dynamic` 基线，并在 first-party 容器内改用内置 `healthcheck` 子命令，
  移除对 runtime `curl`/apt 包的依赖。远端当时 first-party image refs 已更新为
  `TONGLINGYU_GATEWAY_IMAGE_REF=sha256:084aa51d528359e6f86b3b574ebb59f4f7ddd72e4dda1adae0323190e6546bcb`；
  该 first-party image 的 Trivy raw report 为 0 critical / 0 high。
- 2026-05-16 已新增
  `deploy/scripts/prepare-tonglingyu-remote-security-evidence.sh`，完整远端 release
  automation 会先生成并同步真实 `cargo audit` dependency scan、当前 compose
  image inventory 和 per-image Trivy raw reports。最新 security evidence
  artifact 为
  `data/tonglingyu/remote-release-automation/remote-release-20260516T050324Z-37074/remote-security-evidence.json`：
  dependency scan 0 critical / 0 high，image refs 均 immutable 且 raw reports
  可读取，但 aggregate image scan 仍 fail-closed：`critical_count=63`、
  `high_count=714`，来源为第三方 `hermes-agent`、`open-webui`、`cloudflared`
  镜像。未审批这些 high/critical 风险或替换镜像前，不能生成
  production-ready artifact。
- `runtime_config` gate 已支持非 live preflight 的静态 compose/env 解析；live
  release 仍要求 Docker Compose config，不允许用静态解析替代。2026-05-16 以
  digest image refs 和 fixture image scan 复跑 preflight release readiness 后，
  所有 required preflight gates 已通过；随后 `hhost` 已同步当前 release 工具并由
  独立 remote live-gate collector 证明 model upstream、strict Gateway、Open
  WebUI Function 和 Open WebUI admin Action 通过，但这些结果尚未进入当前
  release report。
- 目标环境 live `existing_refs` 恢复演练已在 release automation 中执行并保留
  持久备份 artifact；后续 RQA production-ready 仍必须保证自有镜像
  (`TONGLINGYU_GATEWAY_IMAGE_REF`) 无 high/critical findings，并在干净 release
  commit 上复跑 dependency/image scan 且绑定报告 hash。
  第三方镜像 high/critical findings 不作为 production-ready blocker，但必须保留
  Trivy raw reports、ownership 分类和 nonblocking 风险摘要。
- 后续 RQA production-ready 还必须提供 live/load 性能证据；本地 performance
  budget gate 证明 release 门禁可执行并 fail-closed，但不能替代目标生产环境容量
  与值守验证。
- RQA quality gate、saved report validator 和 contract smoke 已进入
  `verify-tonglingyu-rqa-release-automation.sh` 强制路径；后续 production-ready
  仍必须在目标 release run 中真实执行该 wrapper，并保存 automation artifact /
  release report / validator 输出，不能只引用本地人工命令。
- RQA 用户数据生命周期和 API 兼容性已具备本地 contract smoke 和策略版本；
  `hhost` 已有独立 live Open WebUI admin Action gate artifact，但后续
  production-ready 仍必须把当前版本的 live Action、目标环境 live/load 性能和
  值守证据绑定进 live release report；Open WebUI admin Action source/fixture
  contract 与单独 live gate 不能替代完整 release automation 证据。
- Post-release monitor 已有可复核 evidence 机制：
  `deploy/scripts/verify-tonglingyu-post-release-monitor.sh` 会生成
  `tonglingyu.post_release_monitor` JSON，校验 60 分钟窗口、operator/environment、
  live release report、live gates passed、admin Action/API evidence ref 和 `passed`
  结论；live `release_ops_readiness` 必须绑定该 evidence path/hash，saved report
  validator 会拒绝缺失、未校验或 hash 不匹配的 production-ready 报告。2026-05-16
  目标环境 run `remote-release-20260516T074522Z-71051` 已执行 60 分钟
  post-release monitor，生成 13 条样本，artifact 位于
  `$HOME/hermes-home-deploy/data/tonglingyu/release-artifacts/remote-release-20260516T074522Z-71051/post-release-ops/`；
  `post-release-monitor-evidence.json` 为 `status=ok`，live gates、admin
  Action/API evidence、operator/environment 和 report path 校验均通过。该 evidence
  已绑定进同一 artifact 目录的 `release_ops_readiness` 复核；Milestone L 的值守证据
  blocker 已关闭。
- Capacity/load smoke 已有可复核 evidence 机制：
  `deploy/scripts/verify-tonglingyu-rqa-capacity-load-evidence.sh` 会生成
  `tonglingyu.rqa_capacity_load_evidence` JSON，校验代表性 eval report、failure、
  admin list 翻页、RQA 写入、admin 查询、metrics 查询和 release gate 预算；
  live `rqa_incident_capacity` 必须绑定该 evidence path/hash，saved report
  validator 会拒绝缺失、未校验或 hash 不匹配的 production-ready 报告。早期本地
  smoke 不能替代目标环境真实 capacity/load、incident drill 和 audit-history evidence；
  当前目标环境执行状态见后续 live runner/result 记录。
- Capacity/load smoke 现在已有真实执行 runner：
  `deploy/scripts/verify-tonglingyu-rqa-capacity-load-smoke.sh` 会实际运行本地
  performance budget gate，提取代表性 counts、admin pagination、metrics read、
  status-history audit 和 p95 耗时，生成 capacity/load evidence 与
  incident/audit evidence，再以 live 模式运行 `rqa_incident_capacity` gate 绑定
  evidence path/hash。该输出 scope 是 `local_gateway_smoke`，明确不是目标环境
  live/load 证据。
- 2026-05-16 已新增目标环境 live runner
  `deploy/scripts/verify-tonglingyu-rqa-live-capacity-load-smoke.sh`，并接入
  `verify-tonglingyu-rqa-release-automation.sh` 的 live release 路径和远端工具同步
  校验。该 runner 通过 Open WebUI 容器访问正在运行的 `tonglingyu-gateway`，
  创建 RQA failure / governance task，再通过 live admin API 查询、翻页、metrics、
  状态关闭和 live DB quality gate 生成 capacity/load、incident/audit 和
  `rqa_incident_capacity` evidence。短窗口远端 smoke 已证明脚本可生成完整失败报告：
  `$HOME/hermes-home-deploy/data/tonglingyu/live-capacity-load/live-capacity-smoke-20260516T052621Z-1397271/`
  的 `tonglingyu.rqa_live_capacity_load_smoke` 显示 live gateway 请求、admin 查询、
  incident audit 和 quality gate 均可执行，但 `rqa_write_p95_ms=11567`，超过当前
  10s production 预算，因此 capacity/load evidence 和 `rqa_incident_capacity`
  仍 fail-closed。不能通过调松预算或缩短窗口来宣布 production-ready。
- 已定位 live `rqa_write_p95_ms` 超预算的主要代码风险：RQA 写请求会先完成本地
  deterministic workflow，再逐个串行执行四个 Agent Runtime profile step；这些
  profile step 只绑定和审计对应 step output，不参与本地证据/包/reviewer 的因果
  生成链。`tonglingyu-runtime` 已把 profile step execution 改为并发执行，并按
  原 workflow step index 写回 `agent_runtime` metadata，同时保留 required tool
  enforcement、output_ref 校验、Hermes draft/reviewer 本地治理和后续按序 audit
  append。本地验证已通过：`cargo test -p tonglingyu-runtime`（55 tests）、
  `cargo test -p tonglingyu-gateway`（45 tests）、两包 `cargo clippy -D warnings`、
  `cargo fmt --check` 和 `deploy/scripts/test-tonglingyu-release-readiness-contract.sh`。
  2026-05-16 已将提交 `4f514d0` 同步到 `hhost`，重建并重启
  `tonglingyu-gateway`；远端 `.env` 先备份到
  `$HOME/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260516-134919`
  后临时使用 `tonglingyu-gateway:formal` 完成 build/up，再备份到
  `$HOME/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260516-140333`
  并 pin 回新 image id
  `sha256:f7a3752b4981eeddd17c314dba2503261f76d24a7aab72509a62c2941306925b`。
  完整远端 release automation
  `remote-release-20260516T055004Z-50395` 已在默认 10 分钟 live capacity
  窗口下通过 `rqa_incident_capacity`：`rqa_write_p95_ms=7816`、
  `admin_read_p95_ms=381`、`metrics_read_p95_ms=171`、`release_gate_ms=22558`，
  artifact 位于
  `$HOME/hermes-home-deploy/data/tonglingyu/release-artifacts/remote-release-20260516T055004Z-50395/live-capacity-load/`。
  因此 RQA incident/capacity 性能 blocker 已关闭；该 run 当时仍失败，剩余
  required failures 为 `security_scan`、`release_ops_readiness` 和
  `openwebui_browser_review`，后续记录已继续收敛这些 blocker。
- 2026-05-16 已修复 Open WebUI 普通用户模型可见性：新增
  `deploy/scripts/ensure-openwebui-tonglingyu-model-access.sh`，在 live Open WebUI
  DB 中确保 `model:tonglingyu` active 且存在 `access_grant user:* read`。远端执行
  结果为 `public_read_grant_count=1`，普通用户内部 `/api/models` 验证
  `has_tonglingyu=true`。
- 同日已完成 Open WebUI browser-side review evidence，并绑定进远端 `.env`
  （更新前已备份到
  `$HOME/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260516-143756`）：
  evidence ref 为 `browser-review-20260516T063114Z`，远端 evidence JSON 为
  `$HOME/hermes-home-deploy/data/tonglingyu/browser-review/browser-review-20260516T063114Z/openwebui-browser-review.json`。
  `verify-openwebui-browser-review-evidence.sh` 已验证 ordinary-user model
  visibility、streaming chat UX、admin audit visibility 和 persisted provider
  settings 四项 evidence ref，`status=ok`，`evidence_sha256=e9564f9c586...`。
- browser review 暴露出一个真实产品路径问题：Open WebUI 自动标题/标签/追问后台任务也会
  走 `tonglingyu`，旧 gateway 会把这些非 RQA metadata prompt 当成文学问答处理，
  从而写入 open P0 retrieval failure / governance task。`tonglingyu-gateway`
  已新增 Open WebUI metadata prompt 隔离：识别 title/tags/follow-up 任务后返回确定性 JSON，
  记录 `openwebui_metadata_request_handled` audit event，但不创建 evidence package、
  retrieval failure 或治理任务。本地验证：
  `cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway`
  56 tests 通过，`cargo clippy --manifest-path agent-platform/Cargo.toml -p
  tonglingyu-gateway -- -D warnings` 通过。
- 已将 metadata 隔离修复部署到 `hhost`。远端 `.env` 先备份到
  `$HOME/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260516-145106`，
  `tonglingyu-gateway` 已重建并 pin 到 image id
  `sha256:e63ea6deda84bc6f93a023f1736af6b502908cd12b419a5fde4f2703bcafb947`。
  远端 metadata smoke 证明 title prompt 返回 JSON、没有
  `evidence_package_id`，trace 为 `tly-019e2f90fc947651abccbdb2b91f6f00`；随后 live
  DB 复核 `open_failures=0`、`open_p0_tasks=0`、metadata audit events `>=1`。
- 2026-05-17 最新 release evidence baseline 为
  `remote-release-20260517T185847Z-39274`。该完整远端 automation 在当前
  `/home/simon/tonglingyu-home-deploy` 目标环境运行，已生成并回收 live
  capacity/load evidence、60 分钟 post-release ops evidence、release readiness
  report、release automation report 和 saved report validator artifact。最终
  `release-readiness.json` 显示 `status=passed`、
  `production_release_ready=true`、`required_failures=[]`、
  `release_blockers=[]`，open P0 retrieval failures / governance tasks 均为 0。
- 同一 run 绑定当前 live context：`environment=hhost`、`target=tonglingyu-rqa`、
  `valid_until=2026-05-18T20:12:40.878578+00:00`、source commit
  `d9d17bc27a1f93d51d59ec2500dacd3ed18229cf` 且
  `tracked_dirty=false`。运行镜像 inventory 记录 4 个服务，其中
  `tonglingyu-gateway` pin 到
  `sha256:8743346ed34fe58b9e503564d4322c16c7fff4d2da347a4389b9616b2b8dfb23`。
- 该 run 的 RQA/知识状态证据已绑定 live KB：
  `source_snapshot_digest=f80cd6f7c3f314396bce39cdeb89a7237083537ff0196f55abd94712bf776119`、
  `kb_build_hash=39a48e74c2e76491d473c419f2ba9cae417c12519d9c295398d8239680a31a28`、
  `kb_version=kb-019e34ad728a70728646513367bcc15a`、
  `eval_run_id=rqa-eval-16e7dfebf2e8a8e2`、
  `knowledge_state.unresolved_calibration_gaps` 全部为 0。
- 2026-05-17 已调整 release security policy：镜像扫描按所有权分类，
  `TONGLINGYU_GATEWAY_IMAGE_REF` 的 high/critical findings 仍 fail-closed，第三方
  镜像 findings 进入 `nonblocking_errors` 和 ownership summary，不再阻塞
  production-ready。contract smoke 已覆盖“自有镜像 high 仍失败”和“仅第三方镜像
  high 通过但记录 nonblocking risk”；使用最新远端 Trivy raw reports 本地复核时，
  自有镜像为 0 critical / 0 high，第三方镜像为 63 critical / 714 high，security
  gate 在新策略下通过。当前最终 automation 中 `security_scan`、
  `release_ops_readiness`、`rqa_incident_capacity`、
  `openwebui_browser_review`、`model_upstream_network`、`strict_gateway`、
  `openwebui_function` 和 `openwebui_admin_action` 均通过。
- 为避免把模型上游瞬时 500 当作系统失败或靠手工重跑碰运气，release readiness
  对 live `strict_gateway` 采用 bounded retry policy：默认最多 3 次、失败尝试写入
  gate 结果摘要，最终仍必须拿到原 strict gate 的成功 JSON。本次最终报告中
  `strict_gateway.attempt_count=1`、`failed_attempt_count=0`、
  `retry_policy=bounded_retry`。
- 最终 saved report validator 输出
  `/home/simon/tonglingyu-home-deploy/data/tonglingyu/release-artifacts/remote-release-20260517T185847Z-39274/release-readiness-validation.json`
  为 `status=ok`、`production_release_ready=true`、`errors=[]`。release
  automation report
  `/home/simon/tonglingyu-home-deploy/data/tonglingyu/release-artifacts/remote-release-20260517T185847Z-39274/release-automation.json`
  为 `status=ok`、`production_ready=true`，本地回收 artifact 位于
  `data/tonglingyu/remote-release-automation/remote-release-20260517T185847Z-39274/`。
  因此通灵玉 RQA production-ready release gate 已在当前 run 中闭合。
- Incident drill / audit-history 已有可复核 evidence 机制：
  `deploy/scripts/verify-tonglingyu-rqa-incident-audit-evidence.sh` 会生成
  `tonglingyu.rqa_incident_audit_evidence` JSON，校验 status-history event/actor、
  audit tombstone、incident severity/owner、first response、mitigation、rollback、
  recovery validation 和 RTO/RPO breach escalation evidence ref；live
  `rqa_incident_capacity` 必须绑定该 evidence path/hash，saved report validator
  会拒绝缺失、未校验或 hash 不匹配的 production-ready 报告。目标环境真实
  incident drill、capacity/load 和 audit-history evidence 已在
  `remote-release-20260516T055004Z-50395`、历史最终
  `remote-release-20260516T074522Z-71051` 和当前最终
  `remote-release-20260517T185847Z-39274` 复核中执行并通过该 gate。当前 run 的
  60 分钟 post-release monitor 从 `2026-05-17T19:11:08Z` 到
  `2026-05-17T20:11:09Z`，13 条样本全部 `status=ok`。

## 下一步

1. 保持 RQA release gate 闭环：后续正式 release 仍必须在当前代码策略下绑定当次
   dependency/image scan、release ops、incident/capacity、browser review 和 live gate
   evidence，并由 saved report validator 复核 `production_release_ready=true`。
2. 保持 RQA Milestone L 值守证据闭环：后续正式 release 仍必须绑定当次
   post-release monitor JSON artifact、60 分钟窗口、operator/environment、live gate
   evidence 和 admin Action/API evidence；当前
   `remote-release-20260517T185847Z-39274` 已证明该路径可通过。
3. 在目标 live 环境持续复核 open retrieval failures / open governance tasks 为 0；
   最终 production-ready report 已证明当前为 0，后续 release 仍必须绑定当次证据。
4. 建立分层知识标记：人物、关系、事件、诗词判词和评测题先允许经过 LLM、规则、
   eval 或其他系统校准后进入 `system_calibrated`；这不是自动上线许可，必须再由
   runtime policy 明确提升为 `runtime_usable` 后，才能进入普通回答、证据包或 eval
   样本。人工复核不是前置批处理，而是在运行中把稳定条目升级为带“人工标记”字样的
   `human_marked`。
5. 当前知识治理先基于已登记 Wikisource source snapshot 推进，不把尚未入库的
   影印件、权威校注本或学术整理本设为前置项；待分层标记、低置信清单、
   retrieval failure 修正和 KB rebuild eval diff 稳定后，再在知识治理末尾评估并
   引入程甲/程乙影印件、庚辰/甲戌脂本影印、权威校注本和可标注页码卷册的
   学术整理本。
6. 2026-05-17 已新增 `24_知识状态与系统校准Checklist.md` 作为下一阶段执行口径：
   前 5 个节点必须依次覆盖知识状态模型、系统校准入口、Runtime/Gateway 使用规则、
   运行中人工复核入口、KB diff/eval/release gate；完成前不得声明运行中知识状态
   治理闭环已完成。
7. 系统校准中的 LLM 必须是配置化 Runtime/Hermes 校准执行者，并绑定 profile
   contract、model/upstream、prompt digest、tool policy、timeout 和 release report；
   fake LLM 只能用于测试。校准必须覆盖全部 KnowledgeItemKind，不能用最小实现切片
   替代完成口径。
8. 知识校准按离线批处理或运行中异步任务执行；普通 chat/streaming/package replay
   不得同步调用校准 LLM 并使用同次结果。Milestone B/E 必须补齐 calibration job id、
   input/output digest、lease/heartbeat、幂等、retry、audit、run summary 和 saved
   report validator 证据，否则不能声明系统校准入口或知识状态治理闭环完成。
9. `system_calibrated` 与 `runtime_usable` 必须分离：没有 runtime policy version、
   promotion summary、per-kind coverage matrix、release run 和 saved report validator
   证据时，不能把系统校准条目放入 selected evidence，也不能声明知识状态治理闭环完成。
10. 2026-05-17 已完成 Knowledge State Milestone A：`tonglingyu-runtime` 新增
    `KnowledgeState`、`KnowledgeItemKind`、`knowledge_items`、
    `knowledge_item_state_history`、状态历史、CAS 更新、Runtime store API 和 Gateway
    只读 admin API。已通过 `cargo test -p tonglingyu-runtime` 和
    `cargo test -p tonglingyu-gateway`。这只证明知识状态模型完成，不能声明运行中
    人工复核或完整知识状态治理闭环完成。
11. 2026-05-17 已完成 Knowledge State Milestone B：`tonglingyu-runtime` 新增
    `KnowledgeCalibrationReport`、内部 `honglou-knowledge-calibrator` profile
    contract、配置化 LLM 校准配置、离线 calibration runner、异步 calibration job
    模型、规则/eval/RQA/LLM evidence judge 校准路径、coverage matrix、report hash、
    admin audit 引用和 KB summary/diff report refs；`tonglingyu-gateway` 新增
    `knowledge-calibrate --input <json>` 离线命令。已通过 runtime/gateway 单包测试和
    clippy。Milestone B 完成只表示 candidate 可以被系统校准为
    `system_calibrated`；Milestone C-E 仍未完成，`system_calibrated` 仍不能进入普通
    selected evidence、不能自动提升为 `runtime_usable`，也不能显示“人工标记”。
12. 2026-05-17 已完成 Knowledge State Milestone C：`tonglingyu-runtime` 新增
    runtime knowledge policy、`evidence_claim_knowledge_links`、显式
    `runtime_usable` promotion API、知识状态摘要、claim-to-evidence 的 knowledge item
    内部追踪，以及公开 package/replay/local answer 的安全摘要；`system_calibrated`、
    `candidate`、`source_snapshot`、`rejected` 和 `deprecated` 不进入 selected
    evidence，只有 `runtime_usable` / `human_marked` 可被运行使用。`tonglingyu-gateway`
    的非流式/流式公开输出和 strict Gateway gate 已增加知识状态标签泄露检查。已通过
    runtime/gateway 单包测试和 clippy。Milestone C 完成不表示人工复核入口或
    release gate 闭合；Milestone D 已在后续节点处理，Milestone E 仍未完成，不能
    声明运行中知识状态治理闭环完成。
13. 2026-05-17 已完成 Knowledge State Milestone D：`tonglingyu-runtime` 新增
    `KnowledgeItemHumanReviewDecision`、`KnowledgeItemHumanReviewInput` 和
    `review_knowledge_item_human` Store API，强制 `human_marked` 只能通过绑定
    governance task、reviewer、review note、evidence ref 和 CAS state version 的人工
    复核动作写入；人工否决可进入 `rejected` 或 `deprecated`，并继续保留状态历史和
    audit。`tonglingyu-gateway` 新增 knowledge item review 管理端入口，并支持
    knowledge item / eval miss 作为 governance task source entity；Open WebUI admin
    Action 新增 knowledge item list/read/review 操作，并通过 action contract gate
    校验 role guard、valves、required API path 和 secret 输出边界。已通过
    runtime/gateway 单包测试、clippy 和 Open WebUI admin action contract。Milestone D
    完成不表示 KB diff、eval impact、saved report validator 或 release gate 闭合；
    Milestone E 仍未完成，不能声明完整知识状态治理闭环完成。
14. 2026-05-17 已完成 Knowledge State Milestone E：`tonglingyu-runtime` 的
    KB summary/diff 记录 knowledge state counts、state change refs、calibration
    report refs、human review refs、audit refs、runtime policy promotion summary、
    calibration job summary 和 unresolved gaps；`tonglingyu-gateway` eval report
    新增 `knowledge_state_quality`，对未提升 `system_calibrated`、
    rejected/deprecated selected evidence、reviewer downgrade 和 forbidden failure
    fail-closed；RQA quality gate、release manifest、artifact registry 和 saved
    report validator 绑定 knowledge state summary、KB diff hash、eval impact、
    calibration run/job digest、promotion summary、per-kind coverage matrix 和
    open P0 governance state。已通过 runtime/gateway 单包测试、clippy 和
    `test-tonglingyu-release-readiness-contract.sh`。前 5 个 milestone repo-local
    已闭合，但这仍不是目标 live 环境当次 production-ready release；正式上线仍必须
    重新生成并验证当次 release readiness、KB diff、calibration report、saved report
    validator 和 Open WebUI/Gateway 证据。
