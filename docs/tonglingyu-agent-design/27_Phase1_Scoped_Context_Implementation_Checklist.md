# 27 Phase 1 Scoped Context 实现 Checklist

## 状态口径

目标：实现 `26_Scoped_Context与受控Memory设计.md` 定义的 Phase 1 Scoped Context
最小闭环。

当前状态：repo-local 实现检查点已通过；hhost live gate 未执行。

禁止提前声明：

1. scoped context production-ready；
2. scoped memory production-ready；
3. Memory Collector 完成；
4. active memory 可用；
5. hhost live gate 已通过。

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
- [ ] hhost live gate 覆盖真实 Open WebUI 多轮会话、容器内 Gateway 验证、必要时的
      Cloudflare 公网路径、p95、错误率、超时、降级次数和回滚证据

## 反思记录

- 实现前反思：当前代码仍以 `gateway_sessions` / `gateway_messages` 表达会话与消息。
  Phase 1 不能在旧表上加字段伪装 scoped context，必须新增 Context Governance 模块和
  schema，并让新请求路径写入 `user_session`、`interaction_context`、`context_pack`
  和 `session_journal`。
- 检查点反思：本次实现已把新请求路径切到 scoped context schema，旧
  `gateway_sessions` / `gateway_messages` 只保留为历史 pruning 兼容测试对象，不再作为
  context、journal 或 memory 来源。默认 admin trace 只返回 summary/hash/ref，未增加
  journal 原文查看入口。
- 未完成边界：本地 cargo test、clippy、markdownlint 和 diff check 已通过，但尚未执行
  hhost live gate；因此不能声明 scoped context production-ready，也不能进入 scoped memory
  production-ready 结论。
