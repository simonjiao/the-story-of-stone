# 进展与决策记录

## 当前状态

- 主线已切到“通灵玉”第一版。
- 旧基础库产物和旧专用抽取脚本已删除。
- `scripts/extract_epub.py` 和 `scripts/download_wikisource.py` 已输出
  source snapshot，并保留 `rare_char_annotations`。
- `resources/styles/buhongjushi/` 风格转录保留，不作为主证据库。
- `src/tonglingyu_agent/` 仍是旧 Python 骨架，不作为当前 Rust 主线入口。
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
- 本地建库已验证：5 个来源、10419 个 blocks、10419 条 FTS 记录。
- 本地 HTTP 验证已通过：`/healthz`、`/v1/models`、`/v1/evidence/search`
  和 `/v1/chat/completions`。
- `deploy/docker-compose.yml` 已加入真实 `tonglingyu-gateway` 服务，Open WebUI
  默认连接该 Rust Gateway，Gateway 再按配置调用 Hermes 上游生成层。
- 2026-05-09 已在远程 `hhost:/home/simon/hermes-home-deploy` 真实部署：
  构建 `hermes-agent-platform:formal` 镜像，启动 `tonglingyu-gateway`
  和现有 `hermes-open-webui`，远端 gateway healthcheck 为 healthy。
- 远端 KB 由容器启动时从 source snapshot 构建，当前 `/healthz` 返回
  5 个来源、10419 个 blocks；Open WebUI 容器内 `OPENAI_API_BASE_URL`
  指向 `http://tonglingyu-gateway:8090/v1`，`DEFAULT_MODELS=tonglingyu`。
- 远端容器内已验证 `/v1/models`、`/v1/evidence/search` 和
  `/v1/chat/completions`；“通灵玉上的字是什么？”返回带证据包和 reviewer
  约束的回答。

## 下一步

1. 用 Open WebUI 页面侧做人工点击验证，确认登录态和 UI 中的模型选择
   与容器内配置一致。
2. 补齐人物、关系、事件、诗词判词和评测题库的人工标注层。
3. 增加证据包回放、reviewer 失败样例和公开入口的 smoke 测试脚本。
4. 后续按证据校验或发布 QA 闸门补充影印/权威校注本复核，不作为当前
   M2 loader 的默认前置项。
