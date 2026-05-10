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
  抽样点；这只代表工程上可进入 loader，不代表完成学术校勘。
- M1 完成闸门已明确：进入 M2 前必须完成并通过 source snapshot registry
  校验；本计划不设置独立“M1.5”。若影印件、权威校注本或评测题库要阻塞
  M2，必须先提升为 M1 P0。
- 当前 `python3 scripts/validate_source_snapshots.py` 已通过：5 个来源和
  19 个抽样点满足 M1 source snapshot 闸门。
- Rust `tonglingyu-gateway` 已实现 M2-M6 最小工程闭环：
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
  结论等 102 个发布回归 case；评测报告可落盘到
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
- 当前不能宣布“薄 Gateway + Runtime Agent 已完成”：Gateway 仍直接负责
  source snapshot loader、SQLite 连接和 schema 初始化，且 Runtime
  profile/read-only tools 尚未接入 `agent-runtime` 执行面。

## 下一步

1. 用真实 Open WebUI 账号做页面侧人工点击复核，确认登录态、普通用户模型
   可见性、streaming 体验和管理员审计入口与容器内 smoke 口径一致。
2. 继续按 `20_Runtime接入设计与实施计划.md` 将 Gateway 内 source snapshot
   loader、SQLite schema/open/init、运行时 profile 调用和 read-only tools
   迁入 Runtime/tool 边界。
3. 在 Open WebUI 中嵌入通灵玉 Gateway 管理入口，仅 admin 可用。
4. 补齐人物、关系、事件、诗词判词和评测题库的人工标注层。
5. 后续按证据校验或发布 QA 闸门补充影印/权威校注本复核，不作为当前
   M2 loader 的默认前置项。
