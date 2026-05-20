# 07 Gateway 设计

## 定义

Gateway 是 OpenAI-compatible 的入口服务，也是通灵玉工作流的确定性边界层。
目标架构采用“薄 Gateway + Runtime Agent”：Gateway 不做文学解释，不直接执行
领域检索、证据包构建或 reviewer。它不是第 5 个 Agent。

通用 `global-router` 的设计不从属于“通灵玉”项目。本文只描述“通灵玉”业务
Gateway 的职责。

## 核心定位

Gateway 等于：

1. 协议适配层；
2. 鉴权层；
3. 会话映射层；
4. Runtime step plan 编排层；
5. 安全边界；
6. 审计层；
7. 响应封装层。

Gateway 不等于：

1. 文学解释者；
2. 研究 Agent；
3. 检索 Agent；
4. 审校 Agent；
5. 长期记忆系统；
6. 自主推理系统。

Scoped Context 与受控 Memory 的详细设计见
`26_Scoped_Context与受控Memory设计.md`。该设计不改变 Gateway 的薄边界：
Gateway 只负责 context pack 调用、trace、step plan、journal 索引和响应封装，
不直接沉淀长期 memory，也不把 memory 当作正式事实源。

## Gateway 与 `honglou-main` 的区别

| 项目 | Gateway | `honglou-main` |
| --- | --- | --- |
| 本质 | 后端入口和编排服务 | Runtime 内部 Agent |
| 是否生成文学回答 | 否 | 是 |
| 是否理解红楼梦内容 | 不要求 | 要求 |
| 是否管理鉴权 | 是 | 否 |
| 是否管理流程 | 创建受控 Runtime step plan | 参与但不控制硬规则 |
| 是否能跳过 reviewer | 不能 | 不能 |
| 是否对 Open WebUI 暴露 | 是 | 否 |
| 是否可被 prompt injection 影响 | 不应被影响 | 需通过审校控制 |

## Gateway 职责

### 协议适配

将 Open WebUI 的外部聊天请求转换为内部工作流请求，并将内部结果封装回 Open WebUI 可识别的响应。

### 鉴权

验证 Open WebUI 到 Gateway 的服务端凭证，并验证 Gateway 到内部服务的服务间凭证。

### 请求归一化

提取用户问题、会话上下文、可见模型、用户标识和策略配置，去除不允许由用户控制的内部字段。

### 会话映射

将 Open WebUI 会话映射到内部会话，并控制不同 Agent 可见的上下文范围。

### 工作流编排

创建或提交固定 Runtime step plan，强制包含必要证据 step、证据包 step、
`honglou-main` 和 `honglou-reviewer`。Gateway 不直接执行这些 profile。

### 证据包引用管理

透传或代理 Runtime 返回的 evidence package ref，并记录必要索引，保证最终
回答可追溯。证据包创建、读取和 replay 的领域逻辑归 Runtime Agent/tool 侧。

### 审校强制

确保所有复杂回答的 Runtime step plan 经过 reviewer。用户不能通过提示词或
请求字段关闭审校。

### 错误降级

当某个 Agent 或知识服务失败时，Gateway 负责返回谨慎的降级响应，而不是让系统编造答案。

### 审计日志

记录请求、Runtime step、profile、tool policy、证据包 ref、审校结果和返回
状态。

## Gateway 状态机

第一版建议使用以下状态：

1. Received；
2. Authenticated；
3. Normalized；
4. Runtime Plan Created；
5. Runtime Evidence Steps Completed；
6. Evidence Package Ref Received；
7. Draft Step Completed；
8. Review Step Completed；
9. Revision Step Completed if Needed；
10. Finalized；
11. Failed with Controlled Response。

## Gateway 不保存什么

Gateway 不应保存：

1. 红楼梦领域知识；
2. 用户长期偏好；
3. Agent 私有推理；
4. 原始数据库内容；
5. 内部提示词；
6. 不必要的完整用户隐私文本。

## Gateway 保存什么

Gateway 可以保存：

1. 会话映射；
2. 请求追踪编号；
3. Runtime step 调用日志；
4. 证据包索引；
5. 审校状态；
6. 错误类别；
7. 必要的审计摘要；
8. scoped context 所需的 context/journal 索引和限长摘要。

这些 context/journal 数据必须遵循 `11_权限审计与安全治理.md` 和
`26_Scoped_Context与受控Memory设计.md`：默认不保存不必要的完整隐私文本，
长期 memory 只能由延迟 collector 和审核策略生成。

## 验收标准

Gateway 合格标准：

1. Open WebUI 只能看到一个可见模型；
2. 普通用户不能指定内部 Agent；
3. 普通用户不能跳过 reviewer；
4. 每次请求都有追踪编号；
5. 每次复杂回答都有 evidence package ref；
6. 所有最终回答经过 Runtime reviewer step；
7. 错误时不泄露内部栈和配置；
8. 失败时降级为谨慎回答或证据不足说明。

## Runtime 接入目标

通灵玉 Gateway 的目标接入形态由
`20_Runtime接入设计与实施计划.md` 统一维护。简化边界是：

```text
Open WebUI
  -> tonglingyu-gateway
      -> protocol / auth / routing / trace / SSE / model hiding
      -> Runtime step plan
          -> honglou-text / honglou-commentary
          -> tonglingyu evidence read-only tools
          -> honglou-main
          -> honglou-reviewer
      -> OpenAI-compatible final response
```

Gateway 只负责入口、安全、状态和响应；正文、脂批、证据包、reviewer 和 replay
进入 Runtime profile 与受控 read-only tools。

## 完整产品 Gateway 覆盖状态

当前 Rust `tonglingyu-gateway` 已补齐完整产品 Gateway 的服务端、容器内和
公网入口验证边界。该实现是运行基线和回归基线；目标 Runtime 化仍按
`20_Runtime接入设计与实施计划.md` 迁移。真实登录态页面点击仍需使用实际
Open WebUI 账号做发布复核，不改变 Gateway 合同。

<!-- markdownlint-disable MD013 -->
| 领域 | 当前实现状态 | 发布验收要求 |
| --- | --- | --- |
| Open WebUI 页面验收 | OpenAI-compatible HTTP、本地 smoke、远端容器内入口和公网 `/api/config` 已覆盖 | 用真实 Open WebUI 账号复核登录态、普通用户模型可见性、管理员追踪入口和 Cloudflare 公网入口 |
| 鉴权和权限 | Gateway/admin API key、key rotation、admin 隔离已实现；拒绝内部 Agent、reviewer 开关、私有 trace/package 字段和非 `tonglingyu` 可见模型 | 部署侧必须把凭证放入 `.env`，并确认普通用户无法直接获得 admin key |
| 会话映射 | Open WebUI user/chat/message 可映射到内部 session/trace/package；同一 message 支持幂等去重；scoped context / session journal 仍按文档 `26` 进入后续实现 | 页面侧复核多轮会话、刷新重试体验和 context pack 生成 |
| 状态机 | 已持久化 Received、Authenticated、Normalized、Planned、Evidence Retrieved、Bundle Created、Drafted、Reviewed、Revised if Needed、Finalized 和受控失败原因 | 发布记录必须包含 trace ID 和状态链 |
| 内部 Agent 编排 | 已按 `honglou-main`、`honglou-text`、`honglou-commentary`、`honglou-reviewer` profile 口径记录计划和受限调用摘要；上游调用有超时与本地降级 | 目标 Runtime 化后必须由 Runtime step plan 执行四 profile，并沿用同一审计与降级合同 |
| 证据源强制策略 | 已按问题类型强制正文、脂批、版本、人物别名、诗词判词和字形读音检索；缺必要证据时由 reviewer 阻断或降级 | 扩展人工标注层后继续增加关系、事件和更细版本索引 |
| 证据包合同 | evidence package 已包含证据卡、结论声明、claim-to-evidence 映射、禁止结论、reviewer 记录和 deterministic replay | 合同变更必须保持 replay 兼容测试 |
| reviewer | 已覆盖无证据断言、脂批误当正文、版本边界缺失、人物命运缺正文、prompt injection、现代概念污染和过度绕过请求；失败时最多一次受控修订 | 扩展人工标注后继续补新题型 |
| 审计 | 已覆盖请求归一化、检索计划、受限 profile 调用、上游失败、修订、最终返回和受控失败；admin 可按 trace/session/package 查询 | 发布记录必须保存 trace、package 和 smoke report 路径 |
| 错误和安全 | 统一错误码、公开错误脱敏、内部栈隐藏、上游超时/5xx 降级已实现 | 生产日志只给管理员访问 |
| streaming | SSE 按内容分块返回，包含 trace、evidence package、session 和 reviewer 元数据，且复用审校后结果 | 页面侧复核真实流式体验 |
| 评测闸门 | 当前内置 102 条发布回归 case，覆盖正文、脂批、版本、人物别名、诗词判词、字形读音、证据不足、prompt injection、预期证据 ID 和禁止结论 | 扩展人工标注后继续补回归 case |
| 可观测性 | healthz、admin metrics、Prometheus、状态链、审计事件和请求耗时已实现 | 后续接入集中指标系统 |
| 发布流程 | 本地完整 smoke、远端容器内 smoke 和公网配置入口均已验证；远端记录包含 package/trace/session ID | 使用真实账号做最终页面点击复核 |
<!-- markdownlint-enable MD013 -->

完整产品 Gateway 的完成口径不是“能回答一个问题”，而是：

1. 普通用户只能通过 Open WebUI 的“通灵玉”入口访问；
2. 用户不能通过 prompt 或请求字段绕过证据链、内部 Agent 或 reviewer；
3. 每个复杂回答都有可回放证据包、结论映射和审校记录；
4. 管理员能按 trace ID 或 package ID 查到完整审计链；
5. 失败、证据不足、版本冲突和上游异常都会返回受控回答；
6. 核心评测集和公开入口 smoke 均通过后才允许声明发布就绪。

## RAG 编排边界

Gateway 在 RAG 链路中的职责不是检索和解释，而是强制流程和边界：

1. 根据问题类型和最低策略要求，确保必要证据源被调用；
2. 确保 Runtime Agent/tools 产出证据卡片、证据包和 package ref；
3. 确保复杂回答在返回前经过 Runtime reviewer step；
4. 当证据不足或下游失败时，要求返回谨慎降级回答；
5. 不允许用户要求“只凭模型记忆回答”来绕过证据流程。

涉及人物命运的问题，即使主控计划遗漏脂批或版本证据，Gateway 也应根据策略
在 Runtime step plan 中强制补充相关证据源，或要求回答标注证据不足。
