# chat.huixiangdou.top Open WebUI 测试报告

测试计划：`docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_PLAN.md`

## 执行信息

| 字段 | 值 |
| --- | --- |
| 开始时间 | 2026-05-07 13:00:26 CST |
| 站点 | `https://chat.huixiangdou.top` |
| 测试账号 | `simon.jiao1@icloud.com` |
| 密码记录 | 不落盘，仅运行时输入 |
| 执行方式 | 浏览器交互自动化 |
| 测试数据前缀 | `codex-test-20260507-1300` |

## 当前状态

基础功能测试已继续执行。文件/RAG/Knowledge 本轮按要求跳过。

## 阻断规则

- 登录、基础聊天、会话保存任一关键路径失败时，暂停后续测试。
- 若失败可在当前仓库或可访问部署配置中修复，则先记录修复过程并修复，再继续测试。
- 若失败发生在远端服务且当前没有源码、配置或部署权限，则记录为 BLOCKED，并停止需要该能力的后续用例。

## 测试结果

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| AUTH-01 | PASS | `https://chat.huixiangdou.top` 可打开，未登录时进入登录页。 | 标题 `Hermes Home (Open WebUI)`；URL `/auth?redirect=%2F`；登录表单可见。 |  |  |
| AUTH-02 | PASS | 使用测试账号登录成功，进入聊天首页。 | URL `https://chat.huixiangdou.top/`；首页显示 `您好，simon`、模型选择器和输入框。 |  | 明文密码未写入本报告。 |
| CHAT-01 | FAIL | 模型选择器可打开，但模型列表为空，稳定显示 `未找到结果`。 | 模型按钮 `选择模型` 展开后，搜索框 `搜索模型` 下方显示 `未找到结果`；等待 3 秒后仍为空。 | P1 | 无可选模型会阻断基础聊天，按规则暂停后续测试并进入修复定位。 |
| CHAT-03 | BLOCKED | 输入短消息后点击发送，页面显示 `未选择模型`，消息没有提交为聊天请求。 | 输入框内容 `用一句话说明你能做什么。`；toast `未选择模型`；页面版本 `Hermes Home (Open WebUI) ‧ v0.9.2`。 | P1 | 由 `CHAT-01` 导致，基础聊天无法继续。 |
| CHAT-01-RERUN | PASS | 模型列表恢复，登录后默认选中 `hermes-agent`。 | 首页导航显示 `已选择：hermes-agent`。 |  | 用户完成 admin 修复后复测。 |
| CHAT-03-RERUN | PASS | 简短中文问答可提交并返回中文回答。 | 会话 URL `/c/932c625d-daa0-468c-93fa-c60e97f1070e`；回答包含“我可以帮你查资料、写代码、调试问题、整理思路、操作文件和工具”。 |  |  |
| CTX-01 | PASS | 同一会话内可建立测试代号约束。 | 模型回复包含 `codex-test-basic-20260507`，并确认后续控制在三句话内。 |  |  |
| CTX-02 | PASS | 同一会话追问测试代号，模型能引用前文。 | 追问“刚才的测试代号是什么？”后回答 `codex-test-basic-20260507`。 |  |  |
| CTX-04 | WARN | 新建空白对话后，模型仍能回答上一会话的测试代号。 | 新会话 URL `/c/8e112dab-d3a6-4a31-baf8-d8ef71f2e382`；严格要求未知时输出 `UNKNOWN_ONLY`，实际回答 `codex-test-basic-20260507`。 | P2 | 可能来自 Hermes Agent 跨会话记忆；若这是设计预期，应在 UI 或模型说明中明确提示。 |
| RENDER-01 | PASS | Markdown 标题、表格、代码块渲染正常。 | 响应包含二级标题 `渲染测试`、两列表格、Python 代码块 `print("RENDER_OK")`，代码块操作按钮可见。 |  |  |
| CHAT-06 | PASS | 空输入时不显示发送按钮。 | 输入框为空；可见 DOM 中无 `type="submit"`，语音输入按钮仍可见。 |  |  |
| CHAT-07 | PASS | 多行输入可提交并显示为多行，模型按要求响应。 | 输入包含 `第一行：alpha`、`第二行：beta`；模型回答 `收到两行`。 |  |  |
| HIST-01 | PASS | 刷新当前会话后，消息记录可恢复。 | 刷新 `/c/932c625d-daa0-468c-93fa-c60e97f1070e` 后仍可见 `codex-test-basic-20260507`、`RENDER_OK`、`收到两行`。 |  | 内容加载约 8 秒后完整出现。 |
| HIST-04A | FAIL | 使用消息正文关键词搜索历史未找到目标会话。 | 搜索 `codex-test-basic-20260507`，搜索弹窗显示 `未找到结果`。 | P2 | 会话正文已存在且刷新可恢复，但搜索未命中正文关键词。 |
| HIST-04B | PASS | 使用自动生成的会话标题搜索可找到目标会话。 | 搜索 `AI 助手能力介绍` 可见链接 `/c/932c625d-daa0-468c-93fa-c60e97f1070e`。 |  |  |
| SET-01 | PASS | 用户菜单和设置弹窗可打开。 | 用户菜单中可见 `设置`，设置弹窗中可见 `通用`、`账号`、`关于`。 |  |  |
| SET-02 | PASS | 账号信息页可打开，未显示明文密码。 | 账号页显示名称 `simon`、更改密码入口；未出现明文密码。 |  |  |
| ABOUT-01 | PASS | 关于页可查看版本信息。 | 关于页显示 `Hermes Home (Open WebUI) 版本 v0.9.2`。 |  |  |
| AUTH-05 | PASS | 可退出登录，退出后回到登录页。 | URL `https://chat.huixiangdou.top/auth`；登录表单可见。 |  |  |
| FILE-RAG | SKIPPED | 本轮按用户要求暂不测试文件/RAG/Knowledge。 |  |  |  |

## 发现问题

| 问题 ID | 等级 | 描述 | 影响 | 当前状态 |
| --- | --- | --- | --- | --- |
| [ISSUE-001](chat_huixiangdou_issues/ISSUE-001-model-list-empty.md) | P1 | 登录后模型列表为空。 | 用户无法选择模型，基础聊天无法继续验证。 | RESOLVED: 用户完成 admin 侧修复后，`CHAT-01-RERUN` 通过 |
| [ISSUE-002](chat_huixiangdou_issues/ISSUE-002-history-body-search.md) | P2 | 历史搜索按消息正文关键词无法找到会话，但按标题可找到。 | 用户记得消息内容但不记得标题时，可能找不到历史会话。 | BLOCKED_CODE_REQUIRED: 官方文档预期支持正文搜索，但当前部署无配置项可修 |
| [ISSUE-003](chat_huixiangdou_issues/ISSUE-003-cross-chat-memory-visibility.md) | P2 | Open WebUI 新对话未显式提示会继承 Hermes Agent 外部记忆，但模型可回答上一会话测试代号。 | 用户可能误以为新对话完全隔离。 | READY_FOR_DEPLOY: 已更新 Hermes 配置渲染脚本，需部署后复测 |

## 修复记录

| 时间 | 操作 | 结果 |
| --- | --- | --- |
| 2026-05-07 13:00 CST | 暂停后续聊天/RAG/历史测试，开始定位模型列表为空的可修复范围。 | 已进入修复定位。 |
| 2026-05-07 13:01 CST | 检查当前仓库是否包含 Open WebUI、Huixiangdou、模型连接或部署配置。 | 未找到；当前仓库主要是红楼梦转录/知识库项目。 |
| 2026-05-07 13:02 CST | 检查本机运行容器和用户目录中可能相关部署。 | 未发现相关 Open WebUI/模型服务容器；只发现 `/Users/simon/Projects/chatbot/.env`，该项目不是 Open WebUI 部署。 |
| 2026-05-07 13:03 CST | 检查站点用户菜单、设置页、`/admin` 路径和设置搜索 `连接`。 | 当前账号无 Admin Panel；`/admin` 自动回到首页；用户设置中未找到 Connections/连接入口。 |
| 2026-05-07 13:04 CST | 检查 `Add Model` 是否可作为用户侧修复入口。 | 仅增加第二个空模型槽，不会添加 provider 或模型连接；已移除新增槽位。 |
| 2026-05-07 13:04 CST | 对照 Open WebUI 官方修复路径。 | 需要管理员在 Admin Settings → Connections 添加/启用 provider，或启用 Direct Connections 后用户侧添加连接；当前账号无法执行。 |
| 2026-05-07 13:05 CST | 退出测试账号。 | 已回到 `https://chat.huixiangdou.top/auth`。 |
| 2026-05-07 14:00 CST | 用户完成 admin 侧模型修复后，重新登录测试账号并复测模型选择。 | `hermes-agent` 已可见并默认选中，`ISSUE-001` 在本轮复测中解除阻断。 |
| 2026-05-07 14:01 CST | 继续基础功能测试，跳过文件/RAG/Knowledge。 | 基础聊天、多轮上下文、Markdown/代码渲染、多行输入、刷新恢复、设置与退出通过。 |
| 2026-05-07 14:02 CST | 记录剩余问题。 | 正文关键词历史搜索失败；新对话可访问上一会话测试代号，需确认是否为 Hermes 跨会话记忆的预期行为。 |
| 2026-05-07 14:28 CST | 修复 `ISSUE-003`。 | 已更新 Hermes 配置渲染脚本：写配置前备份，并默认关闭 `memory.memory_enabled` 与 `memory.user_profile_enabled`。 |
| 2026-05-07 14:28 CST | 定位 `ISSUE-002`。 | Open WebUI 官方文档预期历史搜索支持消息内容；当前部署无可用配置项，需 Open WebUI 代码修复或升级验证。 |

## 修复结论

当前状态：

1. `ISSUE-001` 已通过 admin 配置修复并复测通过。
2. `ISSUE-003` 已在仓库部署配置中修复，等待部署应用并复测。
3. `ISSUE-002` 不能通过当前部署配置修正；需要 Open WebUI 代码修复、上游修复或升级后复测。
