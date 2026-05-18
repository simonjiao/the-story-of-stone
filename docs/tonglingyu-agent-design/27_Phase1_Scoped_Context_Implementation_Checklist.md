# 27 Phase 1 Scoped Context 实现 Checklist

## 状态口径

目标：实现 `26_Scoped_Context与受控Memory设计.md` 定义的 Phase 1 Scoped Context
最小闭环。

当前状态：Phase 1 Scoped Context 最小闭环已实现、部署到 `hhost`，并通过当次完整
remote release automation。该结论只覆盖 Phase 1 scoped context，不覆盖 scoped
memory。

已可声明：

1. 当前 `hhost` 版本的 Phase 1 scoped context production-ready；
2. 当前 `hhost` 版本的 live release gate 已通过。

仍禁止提前声明：

1. scoped memory production-ready；
2. Memory Collector 完成；
3. active memory 可用；
4. Memory 审核页面或远程审核工作流可用。

## 工作包

- [x] Context Governance 同进程独立模块
- [x] 新 schema 初始化
- [x] `user_session`
- [x] `interaction_context`
- [x] `context_pack`
- [x] `session_journal`
- [x] `resolved_question`
- [x] admin trace 摘要回放
- [x] contract / 负面测试
- [x] strict Gateway gate 或等价本地验证

## 禁止项检查

- [x] 不生成或读取 active `memory_card`
- [x] 不实现 Memory Collector
- [x] 不实现 `memory_candidate` 队列
- [x] 不实现审核页面
- [x] 不拆独立 Context Governance 服务
- [x] 不迁移旧 `gateway_sessions` / `gateway_messages`
- [x] 不实现 `project/system/work_item/group` scoped memory 读取
- [x] 不让 LLM resolver 决定事实、权限、scope、tool policy、memory ACL 或 reviewer
      裁决
- [x] 不让 memory、session summary 或用户偏好进入 evidence package
- [x] 不暴露公网 admin API、memory 审核入口或 journal 原文查看入口

## Phase 1 退出条件

- [x] 多轮追问可以产生 `resolved_question` 或明确 fail-closed
- [x] 超过 `max_messages` 时生成 session summary，不只保留最后一问
- [x] context pack 可按 trace 回放
- [x] 普通用户不能提交 context、scope、memory 或 tool 控制字段
- [x] text/commentary/reviewer 不获得完整用户历史
- [x] journal 原文只在受控 admin 路径查看
- [x] 单元测试、contract smoke、strict Gateway gate 覆盖上述行为
- [x] hhost live gate 覆盖真实 Open WebUI 多轮会话、容器内 Gateway 验证、必要时的
      Cloudflare 公网路径、p95、错误率、超时、降级次数和回滚证据

## 生产验证证据

- 部署版本：`tonglingyu-gateway` `0.1.6`。
- 运行镜像：`sha256:9399f124cc3409a26ae15b373df78ee57fdd430a7b4c11b1b79e9bb08c657cc0`。
- 最终 remote release automation：
  `data/tonglingyu/remote-release-automation/remote-release-20260518T181347Z-50849/`。
- 远端 artifact：
  `/home/simon/tonglingyu-home-deploy/data/tonglingyu/release-artifacts/remote-release-20260518T181347Z-50849/`。
- automation report：
  `status=ok`、`production_ready_proven=true`、`required_failures=[]`、
  `release_blockers=[]`。
- release readiness：
  `status=passed`、`production_release_ready=true`、`release_conditions_met=true`。
- saved report validator：`status=ok`、`errors=[]`。
- Open WebUI browser review ref：`browser-review-20260518T175953Z`。
- post-release monitor：60 分钟窗口，13 条样本，`failed_sample_count=0`。
- live capacity gate：`status=ok`。
- open P0：retrieval failures `0`，governance tasks `0`。
- 先前 `remote-release-20260518T164116Z-48034` 只因过期 browser review evidence
  fail-closed；已重新执行完整 wrapper，不把单独刷新 readiness 当作最终生产证据。

## 反思记录

- 实现前反思：当前代码仍以 `gateway_sessions` / `gateway_messages` 表达会话与消息。
  Phase 1 不能在旧表上加字段伪装 scoped context，必须新增 Context Governance 模块和
  schema，并让新请求路径写入 `user_session`、`interaction_context`、`context_pack`
  和 `session_journal`。
- 检查点反思：本次实现已把新请求路径切到 scoped context schema，旧
  `gateway_sessions` / `gateway_messages` 只保留为历史 pruning 兼容测试对象，不再作为
  context、journal 或 memory 来源。默认 admin trace 只返回 summary/hash/ref，未增加
  journal 原文查看入口。
- hhost 反思：未用“单独补 browser evidence 后重算 readiness”替代 production
  验证，而是重新跑完整 remote release automation，保留 capacity、security、
  60 分钟 post-release、readiness 和 saved validator 的同一 run 证据。当前可以声明
  Phase 1 scoped context production-ready，但 scoped memory、Memory Collector、
  memory candidate 和审核入口仍未实现，不能进入 scoped memory production-ready 结论。
