# 07 Gateway 设计

## 定义

Gateway 是 OpenAI-compatible 的入口服务，也是内部 Agent 工作流的确定性编排层。它不是第 5 个 Agent。

通用 `global-router` 的设计不从属于“通灵玉”项目，独立放在
`docs/global-router-design/`。本文只描述“通灵玉”业务 Gateway 的职责。

## 核心定位

Gateway 等于：

1. 协议适配层；
2. 鉴权层；
3. 会话映射层；
4. 编排层；
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

## Gateway 与 `honglou-main` 的区别

| 项目 | Gateway | `honglou-main` |
|---|---|---|
| 本质 | 后端编排服务 | Hermes 内部 Agent |
| 是否生成文学回答 | 否 | 是 |
| 是否理解红楼梦内容 | 不要求 | 要求 |
| 是否管理鉴权 | 是 | 否 |
| 是否管理流程 | 是 | 参与但不控制硬规则 |
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

按照固定状态机调用 `honglou-main`、`honglou-text`、`honglou-commentary` 和 `honglou-reviewer`。

### 证据包管理

创建和记录证据包，保证最终回答可追溯。

### 审校强制

确保所有复杂回答经过 reviewer。用户不能通过提示词关闭审校。

### 错误降级

当某个 Agent 或知识服务失败时，Gateway 负责返回谨慎的降级响应，而不是让系统编造答案。

### 审计日志

记录请求、Agent 调用、证据包、审校结果和返回状态。

## Gateway 状态机

第一版建议使用以下状态：

1. Received；
2. Authenticated；
3. Normalized；
4. Planned；
5. Evidence Retrieved；
6. Bundle Created；
7. Drafted；
8. Reviewed；
9. Revised if Needed；
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
3. Agent 调用日志；
4. 证据包索引；
5. 审校状态；
6. 错误类别；
7. 必要的审计摘要。

## 验收标准

Gateway 合格标准：

1. Open WebUI 只能看到一个可见模型；
2. 普通用户不能指定内部 Agent；
3. 普通用户不能跳过 reviewer；
4. 每次请求都有追踪编号；
5. 每次复杂回答都有证据包；
6. 所有最终回答经过审校；
7. 错误时不泄露内部栈和配置；
8. 失败时降级为谨慎回答或证据不足说明。

## RAG 编排边界

Gateway 在 RAG 链路中的职责不是检索和解释，而是强制流程和边界：

1. 根据问题类型和最低策略要求，确保必要证据源被调用；
2. 确保 RAG 结果被整理为证据卡片和证据包；
3. 确保复杂回答在返回前经过 reviewer；
4. 当证据不足或下游失败时，要求返回谨慎降级回答；
5. 不允许用户要求“只凭模型记忆回答”来绕过证据流程。

涉及人物命运的问题，即使主控计划遗漏脂批或版本证据，Gateway 也应根据策略强制补充相关证据源，或要求回答标注证据不足。
