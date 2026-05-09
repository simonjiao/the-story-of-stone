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

基础功能测试已执行；文件/RAG/Knowledge 按要求跳过。2026-05-08 已完成正式 Agent Platform 集成部署验证，Open WebUI 当前通过正式 `agent-orchestrator` 访问默认 `hermes-agent`。Open WebUI 管理员账号级 Function API、Admin Panel Function 更新和 admin 运行时权限边界补测已完成。2026-05-09 已完成 Agent Identity Bridge hardening 的正式环境部署复测。

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
| [ISSUE-003](chat_huixiangdou_issues/ISSUE-003-cross-chat-memory-visibility.md) | P2 | Open WebUI 新对话未显式提示会继承 Hermes Agent 外部记忆，但模型可回答上一会话测试代号。 | 用户可能误以为新对话完全隔离。 | RESOLVED: 正式部署后已确认 Hermes memory 关闭，API 级跨请求复测返回 `UNKNOWN_ONLY` |
| [ISSUE-004](chat_huixiangdou_issues/ISSUE-004-open-webui-origin-ip-conflict.md) | P1 | 正式 compose 首次启动 Open WebUI 时，固定 origin IP 被动态分配给 `agent-manager`。 | Open WebUI 无法启动，Cloudflare Tunnel 无法访问聊天窗口。 | RESOLVED: 已为内部服务分配稳定 IP 并复测通过 |
| [ISSUE-005](chat_huixiangdou_issues/ISSUE-005-followup-prompt-agent-control-false-positive.md) | P1 | Open WebUI 自动追问建议提示包含历史中的 Agent Platform 控制指令时，被 Orchestrator 误判为新的 Agent Platform 请求。 | Agent Platform 请求列表和审计被内部辅助提示污染，可能产生重复审批噪音。 | RESOLVED: 已收窄识别范围并部署复测 |
| [ISSUE-006](chat_huixiangdou_issues/ISSUE-006-stop-generation-control-unlabelled.md) | P3 | 长回答生成中的停止控制在可访问 DOM 中表现为无标签按钮。 | 自动化和辅助技术难以明确识别“停止生成”按钮。 | OPEN: 停止功能可用，但建议补充 `aria-label` |
| [ISSUE-007](chat_huixiangdou_issues/ISSUE-007-approved-agent-owner-mismatch.md) | P1 | 审批后 agent 曾归属审批人而不是原始请求人。 | 原始请求人无法在 `my-agents` 中看到审批后的 agent，影响 agent 复用、多 session 和用户侧 run。 | RESOLVED: Manager fulfill 改用 `requested_by_user` 并通过正式多 session/Worker run 复测 |
| [ISSUE-008](chat_huixiangdou_issues/ISSUE-008-open-webui-agent-identity-session-bridge.md) | P2 | Open WebUI 默认调用未透传动态 Agent Platform 用户身份和 agent session 元数据。 | 多用户审计和 UI 级 agent session 绑定不能依赖旧的默认聊天请求。 | RESOLVED: 已部署 Agent Identity Bridge，Open WebUI Filter 注入签名上下文，Manager 使用 `openwebui:<id>` 审计并支持 UI chat 到 agent session 绑定 |
| [ISSUE-009](chat_huixiangdou_issues/ISSUE-009-agent-identity-bridge-hardening.md) | P1 | Bridge baseline 后仍缺少 replay、message append 幂等、lifecycle audit、internal 权限收窄和部署校验闭环。 | 可能导致窗口内重放、重复消息、审计不可追踪或过宽内部权限。 | RESOLVED: 已部署 hardening，正式环境复测覆盖 Function、migration、dedup、replay、subject isolation、Orchestrator 重启复用和日志 |

## 修复记录

说明：早期记录中的阶段名、临时容器和临时测试前缀不代表当前正式方案命名。当前正式部署镜像 tag 使用 `formal`，Agent Identity Bridge 不使用阶段名作为协议、字段、函数、Docker tag 或部署命名。

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
| 2026-05-08 16:07 CST | 正式部署前备份远程 `deploy/.env`。 | 备份路径：`/home/simon/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260508-160755`。 |
| 2026-05-08 16:08 CST | 同步正式 Agent Platform 构建上下文、compose 和 README 到远程部署目录。 | 远程 `docker compose config` 通过；早期测试镜像构建成功。当前正式 tag 已改为 `formal`。 |
| 2026-05-08 16:15 CST | 首次正式 `docker compose up` 启动 Open WebUI 失败。 | Docker 返回 `failed to set up container networking: Address already in use`；`agent-manager` 动态占用 `172.20.0.3/16`，与 Open WebUI 固定 origin IP 冲突。 |
| 2026-05-08 16:18 CST | 修复内部网络地址分配。 | `hermes-agent` 固定 `172.20.0.2`，`open-webui` 固定 `172.20.0.3`，Agent Platform 服务固定在 `.4` 与 `.10`-`.13`。 |
| 2026-05-08 16:20 CST | 重新启动正式 compose。 | `agent-manager`、`agent-orchestrator`、`agent-worker`、`agent-observer`、`hermes-agent`、`hermes-open-webui` 均启动；Open WebUI 指向 `http://agent-orchestrator:8080/v1`。 |
| 2026-05-08 16:20 CST | 验证正式 Orchestrator 普通聊天穿透。 | `/v1/chat/completions` 返回 `HERMES_FORMAL_OK`。 |
| 2026-05-08 16:20 CST | 验证 Agent Platform 控制类请求。 | 控制指令返回 `approval_required`，请求 `req_019e06ac5efa7663a9a397b00408ea4d` 写入 Manager，审计记录包含 `request:create_agent`。 |
| 2026-05-08 16:20 CST | 验证公网入口。 | `https://chat.huixiangdou.top/api/config` 返回 HTTP 200。 |
| 2026-05-08 16:21 CST | 清理临时测试资源。 | 已删除早期临时测试容器、临时数据库和临时用户，`hermes-internal` 网络中不再存在临时测试容器。 |
| 2026-05-08 16:23 CST | 正式部署 Hermes memory 配置。 | 配置备份：`/home/simon/hermes-home-deploy/data/hermes/config.yaml.bak.20260508-082325`；已确认 `memory_enabled: false` 与 `user_profile_enabled: false`。 |
| 2026-05-08 16:24 CST | Hermes 重启后复测。 | 普通聊天返回 `HERMES_FORMAL_AFTER_RESTART_OK`；API 级跨请求记忆复测返回 `UNKNOWN_ONLY`；公网 `/api/config` 仍返回 HTTP 200。 |
| 2026-05-08 18:20 CST | 使用测试账号重新执行 UI 关键路径与长回答专项。 | 登录成功，模型选择器显示 `hermes-agent`；UI 普通聊天返回 `ROUTE_UI_OK`；UI Agent Platform 控制指令返回 `approval_required`。 |
| 2026-05-08 18:24 CST | 执行长回答 Markdown 渲染测试。 | 会话 `/c/cd0fa0fe-43b5-49e8-ac93-bb905e91e32a` 返回二级标题、列表、表格、代码块和结束标记 `LONG_RENDER_DONE_20260508_A`。 |
| 2026-05-08 18:26 CST | 执行长回答停止生成测试。 | 1000 行清单生成被中断在第 6 行附近，未出现第 1000 行，页面出现 `继续生成`。停止控制功能可用，但生成中控制按钮在可访问 DOM 中无明确标签。 |
| 2026-05-08 18:27 CST | 刷新长回答会话。 | 长回答、被中断回答和 `继续生成` 状态均恢复；误输入代码块的草稿未保存。 |
| 2026-05-08 18:28 CST | 执行 UI 新会话隔离复测。 | 新会话 `/c/19884914-a0ae-42ce-9c82-3fcb799d7dbf` 追问上一会话测试代号，模型返回 `UNKNOWN_ONLY`。 |
| 2026-05-08 18:31 CST | 执行历史正文搜索回归。 | 新长回答正文关键词可找到新会话；旧正文关键词 `codex-test-basic-20260507` 仍返回 `未找到结果`。 |
| 2026-05-08 18:33 CST | 发现并修复 Open WebUI 追问建议导致的 控制请求误判。 | 追问建议内部提示曾额外创建 `req_019e071a9a667a13a10be4f718ee3746`；修复 Orchestrator 只识别 `### Chat History` 之前的直接用户文本。 |
| 2026-05-08 18:38 CST | 部署并复测 控制请求误判修复。 | 内部追问建议提示复测前后最新请求 ID 不变，响应不含 `approval_required`；直接用户控制指令仍创建 `req_019e072a5bc87352b4ca99c26664157f`。 |
| 2026-05-08 18:47 CST | 发现审批后 agent owner 错误。 | 审批请求 `req_019e072a5bc87352b4ca99c26664157f` 后生成的 `agent_019e0732cc207a11b34713104a7f2e6d` 出现在审批人 `admin` 的 `my-agents`，不在原始请求人下。 |
| 2026-05-08 18:55 CST | 修复并部署 Manager owner 逻辑。 | `fulfill_request` 改用 `request.requested_by_user` 做 agent owner 和复用查询 owner；正式 Agent Platform 共享镜像重建，Manager/Orchestrator/Worker/Observer 全部重启。 |
| 2026-05-08 18:58 CST | 执行 Agent Platform 多 session 与 Worker run 复测。 | 新请求 `req_019e073c5df57450902773f4c872c1d0` 审批后 agent 归属原始请求人；同一 agent 下两个 session、两个 run 均完成。 |
| 2026-05-09 11:00 CST | 部署 Agent Identity Bridge hardening 到远端正式环境。 | 同步 `agent-platform/`、Function verify 脚本和 README；重建 `hermes-agent-platform:formal`，重启 Manager/Orchestrator/Worker/Observer。 |
| 2026-05-09 11:01 CST | 修复正式环境 Function 校验脚本差异。 | 远端没有 `OPEN_WEBUI_ADMIN_TOKEN`；`verify-openwebui-function.sh` 新增 compose DB fallback，输出 `source=compose-db`、Function 状态和 valve key names，不输出 secret 值。 |
| 2026-05-09 11:03 CST | 执行 hardening 远端复测。 | 合成 Open WebUI subject/chat 覆盖审批建链、follow-up run、同 message_id 去重、nonce replay 冲突、关闭 session、不同 subject 隔离、Orchestrator 重启后 binding 复用；关键服务日志无错误关键词。 |
| 2026-05-09 11:33 CST | 使用正式 Open WebUI 真实账号复测 Bridge。 | 用真实 admin 与普通 user 账号的 Open WebUI API auth 走 `/api/chat/completions`，确认 Function 注入真实账号上下文；两个测试 chat 已在复测后删除。 |

## 2026-05-08 正式部署验证

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| DEPLOY-01 | PASS | 远程 `.env` 已在变更前备份。 | `/home/simon/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260508-160755`。 |  | 未输出密钥或密码。 |
| DEPLOY-02 | PASS | 正式 compose 配置可解析，Agent Platform 镜像可构建。 | `remote_compose_config_ok`；早期测试镜像构建成功。当前正式 tag 已改为 `formal`。 |  | 首次构建受 crates.io 网络重试影响，但最终成功。 |
| DEPLOY-03 | PASS | 正式服务全部健康或运行中。 | `docker compose ps` 显示 `agent-manager`、`agent-orchestrator`、`agent-platform-postgres`、`hermes-agent`、`hermes-open-webui` healthy，worker/observer running。 |  |  |
| NET-01 | PASS | Open WebUI origin IP 冲突已修复。 | `hermes-open-webui 172.20.0.3/16`；`agent-manager 172.20.0.10/16`；无临时测试容器。 | P1 | 见 `ISSUE-004`。 |
| ROUTE-01 | PASS | Open WebUI 后端已切到正式 Orchestrator。 | 容器环境仅验证非敏感项：`OPENAI_API_BASE_URL=http://agent-orchestrator:8080/v1`。 |  |  |
| CHAT-AGENT-01 | PASS | 普通聊天经 Orchestrator 穿透到默认 Hermes Agent。 | 重启前返回 `HERMES_FORMAL_OK`；Hermes 重启后返回 `HERMES_FORMAL_AFTER_RESTART_OK`。 |  |  |
| AGENT-CTRL-01 | PASS | Agent Platform 控制类聊天请求进入 Manager 并要求审批。 | 响应包含 `request_id=req_019e06ac5efa7663a9a397b00408ea4d`、`status=approval_required`。 |  |  |
| AGENT-AUDIT-01 | PASS | Agent Platform 控制请求有审计记录，Observer 正常 tick。 | `agentctl audit --limit 10` 包含 `request:create_agent` 与多条 `observer:tick`。 |  |  |
| PUBLIC-01 | PASS | 公网 Cloudflare 入口可访问 Open WebUI API。 | `https://chat.huixiangdou.top/api/config` 返回 HTTP 200，响应体 464 bytes。 |  |  |
| MEMORY-01 | PASS | Hermes 跨请求持久记忆已关闭。 | 配置显示 `memory_enabled: false`、`user_profile_enabled: false`；独立追问返回 `UNKNOWN_ONLY`。 |  | 覆盖 `ISSUE-003` 的部署后验证。 |
| UI-SESSION-20260508 | NOT_RUN | 未在本轮重新执行浏览器登录、发送消息和会话保存。 | 未请求或记录登录密码；本轮用正式后端/API 路径验证部署。 |  | 2026-05-07 已完成 UI 登录、聊天和刷新恢复测试；正式路由切换后的浏览器会话保存仍需人工登录态复核。 |

## 2026-05-08 UI 长回答与 Agent Platform 回归

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| AUTH-20260508 | PASS | 使用测试账号登录成功。 | 登录后 URL `https://chat.huixiangdou.top/`；首屏显示 `hermes-agent`。 |  | 密码未写入报告。 |
| MODEL-20260508 | PASS | 模型选择器可打开，`hermes-agent` 可用且已选中。 | `listbox "可用模型"` 中有 `option "选择模型 “hermes-agent”"`，无 `未找到结果`。 |  |  |
| CHAT-UI-ROUTE-20260508 | PASS | UI 普通聊天经正式 Orchestrator 走默认 Hermes Agent。 | 会话 `/c/c56d362f-db7a-4826-8252-859d9750ddd8`；回复 `ROUTE_UI_OK`。 |  |  |
| AGENT-UI-CTRL-20260508 | PASS | UI 控制类请求进入 Agent Manager 并要求审批。 | UI 回复包含 `request_id=req_019e071a9a2572c08488577cc52d77d6`、`status=approval_required`。 |  |  |
| AGENT-UI-AUDIT-20260508 | PASS | 远程 Manager 可见 UI 触发的 Agent Platform 请求。 | `agentctl requests list` 包含 `req_019e071a9a2572c08488577cc52d77d6`，状态 `approval_required`。 |  |  |
| LONG-RENDER-20260508 | PASS | 800-1000 字中文长回答可完整显示并渲染 Markdown。 | 会话 `/c/cd0fa0fe-43b5-49e8-ac93-bb905e91e32a`；包含二级标题、列表、表格、代码块和 `LONG_RENDER_DONE_20260508_A`。 |  |  |
| LONG-STOP-20260508 | PASS | 长回答生成可中断，已生成内容保留。 | 1000 行清单请求被中断在第 6 行附近，未出现 `第 1000 行`，页面显示 `继续生成`。 |  | 停止控制可用，但见 `ISSUE-006`。 |
| LONG-SAVE-20260508 | PASS | 刷新后长回答和中断状态可恢复。 | 刷新 `/c/cd0fa0fe-43b5-49e8-ac93-bb905e91e32a` 后仍可见 `LONG_RENDER_DONE_20260508_A`、长回答测试标记和 `继续生成`。 |  |  |
| MEMORY-UI-20260508 | PASS | 新会话不会继承上一会话测试代号。 | 新会话 `/c/19884914-a0ae-42ce-9c82-3fcb799d7dbf` 返回 `UNKNOWN_ONLY`。 |  | 覆盖 `ISSUE-003` 的 UI 级回归。 |
| HIST-NEW-BODY-20260508 | PASS | 新长回答会话可通过正文关键词搜索到。 | 搜索新长回答测试标记返回 `🤖 Hermes Agent Chat`。 |  |  |
| HIST-OLD-BODY-20260508 | FAIL | 旧会话正文关键词仍无法搜索到。 | 搜索 `codex-test-basic-20260507` 返回 `未找到结果`。 | P2 | `ISSUE-002` 仍未完全解决，可能是旧历史未重建索引或历史正文搜索覆盖不一致。 |
| AGENT-FOLLOWUP-FP-20260508 | FAIL->PASS | Open WebUI 追问建议内部提示曾误创建 Agent Platform 请求；修复部署后不再复现。 | 修复前额外请求 `req_019e071a9a667a13a10be4f718ee3746`；修复后同类提示复测前后最新请求 ID 不变，且响应不含 `approval_required`。 | P1 | 见 `ISSUE-005`。 |

## 2026-05-08 Agent Owner、多 Session 与 Worker 复测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| AGENT-APPROVAL-OWNER-20260508 | FAIL->PASS | 审批后 agent 归属原始请求人，不再归属审批人。 | 请求 `req_019e073c5df57450902773f4c872c1d0` 审批后生成 `agent_019e073c5ea27640b34fc51a49be5f96`；`codex-owner-20260508` 的 `my-agents` 命中 1 条，`admin` 的 `my-agents` 命中 0 条。 | P1 | 见 `ISSUE-007`。 |
| AGENT-MULTI-SESSION-20260508 | PASS | 同一 agent 下可创建并保持两个活跃 session。 | `sess_019e073cf05b72f08e3d20c625c86443`、`sess_019e073cf1eb724188643a0e15effef2` 均为 `active`；`agentctl agents list` 显示 `active_session_count=2`。 |  | API 路径验证；UI 默认聊天桥接见 `ISSUE-008`。 |
| AGENT-WORKER-RUN-20260508 | PASS | Worker 能处理同一 agent 的两个 session run。 | `run_019e073cf1637c40adb597c8960649ed`、`run_019e073cf2f87580add2d1e6500811af` 均为 `completed`，摘要均包含 `with 1 recent messages`。 |  | 审计包含 `worker:run_claim`、`worker:run_status`、`worker:run_finish`。 |
| AGENT-OBSERVER-20260508 | PASS | Observer 在 Worker run 后继续报告健康。 | 最新报告 `obsr_019e073d65d67222ac9e55bd2822a7e5` 为 `healthy`，摘要 `dead_letter=0`。 |  |  |

## 2026-05-08 Agent Identity Bridge 正式部署复测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| BRIDGE-DEPLOY-20260508 | PASS | 正式远程 Docker 已部署 `hermes-agent-platform:formal`，Manager/Orchestrator/Worker/Observer 健康。 | `docker compose ps` 显示 `agent-manager`、`agent-orchestrator` healthy，worker/observer running；Open WebUI healthy。 |  | 变更前备份 `.env` 到 `/home/simon/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260508-201853`。 |
| BRIDGE-FUNCTION-20260508 | PASS | 正式 Open WebUI 已安装全局 `agent_identity_bridge` Filter。 | Open WebUI `function` 表存在 `agent_identity_bridge`，`type=filter`、`is_active=1`、`is_global=1`。 |  | 测试账号非 admin，Function 通过正式容器持久 DB 写入并重启 Open WebUI 生效；未使用临时 Open WebUI。 |
| BRIDGE-CHAT-SHORT-20260508 | PASS | 普通短聊天仍走默认 Hermes。 | Open WebUI `/api/chat/completions` 返回 `我在线。`。 |  |  |
| BRIDGE-CHAT-LONG-20260508 | PASS | 普通长回答仍可完成。 | `BRIDGE-SMOKE-LONG-20260508` 返回 634 字中文回答。 |  |  |
| BRIDGE-AUTH-20260508 | PASS | UI 控制请求按真实 Open WebUI 用户进入 Manager。 | 请求 `req_019e0796c6f77c52ae779adc787de54f` 的 `requested_by_user=openwebui:7fe86b5c-4248-46ac-a8bc-bb716e1ca102`，`requested_by_service=agent-orchestrator`。 | P2 | 覆盖 `ISSUE-008`。 |
| BRIDGE-JWT-20260508 | PASS | Manager 已进入 JWT 模式，dev headers 被拒绝。 | 直接用 `x-agent-user=dev-user` 调 Manager 创建请求返回 HTTP 401 `unauthorized`。 |  | `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false`。 |
| BRIDGE-TAMPER-20260508 | PASS | 缺失 `agent_bridge_context` 的控制请求被 Orchestrator 拒绝。 | 直连 Orchestrator 控制请求返回内容 `{"error":"unauthorized",...}`。 |  | 普通聊天不受影响。 |
| BRIDGE-BINDING-20260508 | PASS | 审批后自动创建 Open WebUI chat 到 agent session 的持久 binding。 | `open_webui_bridge_bindings` 记录 `chat_id=3c77dba8-6890-44d2-8f97-39e08cb723b9`、`agent_session_id=sess_019e07979ebc7200b06652ccd3716d73`、`status=active`。 |  |  |
| BRIDGE-RUN-20260508 | PASS | 同一 Open WebUI chat 后续消息自动 append session message 并创建 run，Worker 完成执行。 | 后续消息返回 `run_019e07981bc17b02ae64ff45e243f67a`、`session_id=sess_019e07979ebc7200b06652ccd3716d73`，摘要含 `with 1 recent messages`。 |  |  |
| BRIDGE-MULTI-SESSION-20260508 | PASS | 同一 Open WebUI 用户的新 chat 复用同一 agent 但创建不同 session。 | `agent_019e07979eb673d3ac7dab69b8088388` 下 active binding 聚合为 `2` 个 distinct chat、`2` 个 distinct session。 |  |  |
| BRIDGE-CLOSE-20260508 | PASS | 关闭当前 agent session 后 binding 标记 closed。 | `sess_019e07979ebc7200b06652ccd3716d73` 对应 binding 状态为 `closed`，`closed_at is not null`。 |  |  |
| BRIDGE-LOGS-20260508 | PASS | 复测窗口内关键服务无 error/panic/failed 日志。 | `docker compose logs --since=10m agent-manager agent-orchestrator agent-worker open-webui` 未检出错误关键词。 |  |  |

## 2026-05-08 Agent Identity Bridge 一致性 hardening 复测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| BRIDGE-HARDEN-DEPLOY-20260508 | PASS | 远程正式镜像重建并重启 Manager/Orchestrator，服务健康。 | `agent-manager`、`agent-orchestrator` 均运行 `hermes-agent-platform:formal` 且 healthy；Open WebUI healthy。 |  | 本次未修改远程 `.env`。 |
| BRIDGE-HARDEN-ROUTE-20260508 | PASS | Orchestrator 模型列表和普通 Hermes passthrough 正常。 | 内网 `/v1/models` 返回 `hermes-agent`；普通聊天包含 `HARDENING_OK_20260508`。 |  |  |
| BRIDGE-HARDEN-FAILCLOSED-20260508 | PASS | 缺少 `agent_bridge_context` 的控制请求继续 fail closed。 | 内网控制请求返回 OpenAI-compatible 响应，内容包含 `unauthorized`。 |  |  |
| BRIDGE-HARDEN-JWT-20260508 | PASS | Manager 仍拒绝 dev headers。 | 直连 Manager，带 `x-agent-user: dev-user` 的创建请求返回 HTTP 401。 |  |  |
| BRIDGE-HARDEN-PUBLIC-20260508 | PASS | 公网 Open WebUI API 仍可访问。 | `https://chat.huixiangdou.top/api/config` 返回 HTTP 200，响应 464 bytes。 |  |  |
| BRIDGE-HARDEN-LOGS-20260508 | PASS | hardening 复测窗口内关键服务无错误关键词。 | `docker compose logs --since=5m agent-manager agent-orchestrator open-webui` 未检出 `error/panic/failed/forbidden`。 |  |  |

## 2026-05-08 Agent Platform 管理面远程复测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| ADMIN-NONADMIN-REJECT-20260508 | PASS | 非 admin JWT 访问 admin request list 被拒绝。 | `GET /v1/admin/requests?limit=1` 返回 HTTP 403。 |  | 验证 Open WebUI 普通用户不等于 Agent Platform admin。 |
| ADMIN-REQUEST-AUDIT-LIST-20260508 | PASS | admin JWT 可读取 request list 和 audit list。 | `GET /v1/admin/requests?limit=3`、`GET /v1/admin/audit?limit=3` 均返回 HTTP 200。 |  | 测试 JWT 临时签发，未输出 secret。 |
| ADMIN-DENY-20260508 | PASS | admin 可 deny 待审批请求。 | 请求 `req_019e07dc3e917331aeadfbbabb55f250` 从 `approval_required` 变为 `denied`。 |  |  |
| ADMIN-APPROVE-20260508 | PASS | admin 可 approve 待审批请求并创建 agent。 | 请求 `req_019e07dc3fc173e1a1dd2e4e502d2f9f` fulfilled，生成 `agent_019e07dc405e7d50a29535da3dbf7992`。 |  |  |
| ADMIN-AGENT-LIFECYCLE-20260508 | PASS | admin agent list/pause/resume/delete 均可用。 | `agent_019e07dc405e7d50a29535da3dbf7992` 可被 list 命中，状态依次为 `paused`、`running`、`terminated`。 |  | delete 语义为标记 terminated。 |
| ADMIN-GRANT-20260508 | PASS | admin 可创建 grant。 | `POST /v1/admin/grants` 返回 `grant_019e07dc42eb72e28cf668cdf8d32e6e`。 |  |  |
| ADMIN-OBSERVER-20260508 | PASS | admin 可 list observer reports、手动触发 observer run 并读取 report。 | 手动触发生成 `obsr_019e07dc44107b329f4355143ed1df33`，随后 `GET /v1/admin/observer/reports/{id}` 返回 HTTP 200。 |  |  |
| ADMIN-RUN-RETRY-TERMINATE-20260508 | PASS | admin 可对 dead-letter run 执行 retry 和 terminate。 | `run_admin_run_smoke_1778248384_retry` dead-letter 后 retry 回 `queued`；`run_admin_run_smoke_1778248384_terminate` dead-letter 后 terminate 为 `cancelled`。 |  | 测试 run 由 internal service endpoint 构造，只用于管理面 smoke。 |
| ADMIN-LOGS-20260508 | PASS | 管理面复测窗口内关键服务无错误关键词。 | `docker compose logs --since=5m agent-manager agent-worker agent-observer` 未检出 `error/panic/failed`。 |  |  |

## 2026-05-08 Open WebUI 管理员实账号补测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| OPENWEBUI-FUNCTION-API-20260508 | PASS | 使用真实 Open WebUI admin 登录 token 通过正式 Function API 更新 `agent_identity_bridge`。 | `deploy/scripts/install-openwebui-function.sh` 返回 `{"function_id":"agent_identity_bridge","action":"updated","target_model":"hermes-agent"}`；随后 `GET /api/v1/functions/id/agent_identity_bridge` 返回 HTTP 200。 |  | 走远端 Docker 内网 `http://172.20.0.3:8080`，未使用临时 Open WebUI；未记录 token。 |
| OPENWEBUI-FUNCTION-VALVES-20260508 | PASS | Function API 更新后，Filter 仍为全局启用且 valves 完整。 | `export?include_valves=true` 中 `is_active=true`、`is_global=true`，valve keys 为 `AGENT_BRIDGE_ISSUER`、`AGENT_BRIDGE_SECRET`、`TARGET_MODEL`。 |  | 只记录 key，不记录 secret 值。 |
| OPENWEBUI-FUNCTION-UI-20260508 | PASS | Admin Panel 可进入 Function 编辑页并通过 UI 保存更新。 | 管理员菜单显示 `管理员面板`；`/admin/functions` 显示 `Agent Identity Bridge v1.0.0`；编辑页点击 `保存` 后 toast `函数更新成功`；DB `updated_at=2026-05-08 23:01:24 CST`，`is_active=1`、`is_global=1`。 |  | 验证 UI 更新路径；未创建临时 Function。 |
| OPENWEBUI-ADMIN-BRIDGE-ROLE-20260508 | PASS | Open WebUI admin 登录用户通过 Bridge 发起控制请求时，默认不会映射为 Agent Platform admin。 | UI/API 请求返回 `request_id=req_019e0814ba0e71d393fe0c915fce02de` 且包含 `approval_required`；Manager DB 行为 `requested_by_user=openwebui:e85ce153-bdd3-4ef1-a82a-e107c0a12a53`、`requested_by_service=agent-orchestrator`、`status=approval_required`。 |  | Orchestrator 环境为 `AGENT_BRIDGE_USER_ROLE=viewer`、`AGENT_BRIDGE_ADMIN_ROLE_MAPPING=disabled`。 |
| OPENWEBUI-ADMIN-BRIDGE-LOGS-20260508 | PASS | 管理员实账号补测窗口内关键服务无错误日志。 | `docker compose logs --since=15m open-webui agent-orchestrator agent-manager` 未检出 `error/panic/failed/forbidden/unauthorized`。 |  |  |

## 2026-05-09 Agent Identity Bridge hardening 本地验证

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| BRIDGE-HARDEN-CODE-20260509 | PASS | 本地代码已补 internal action 收窄、nonce replay 防护、message append 去重、binding lifecycle audit 和 closed binding guard。 | `cargo test --manifest-path agent-platform/Cargo.toml` 通过；覆盖 `bridge_nonce_claim_rejects_replay_through_manager`、`bridge_binding_lifecycle_is_audited`、`append_message_dedupes_external_message_id`、`dev_manager_headers_only_allow_bridge_internal_namespace`。 | P1 | 正式环境验证见下节。 |
| BRIDGE-HARDEN-VERIFY-SCRIPT-20260509 | PASS | 新增 Open WebUI Function 校验脚本，输出 valve key names，不输出 secret 值。 | `bash -n deploy/scripts/verify-openwebui-function.sh` 通过。 | P1 | 正式环境验证见下节。 |

## 2026-05-09 Agent Identity Bridge hardening 正式环境复测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| BRIDGE-HARDEN-DEPLOY-20260509 | PASS | 远端正式镜像已重建并重启，Manager/Orchestrator 健康，Worker/Observer 运行中。 | `docker compose ps` 显示 `agent-manager`、`agent-orchestrator` healthy，`agent-worker`、`agent-observer` running；Open WebUI healthy。 | P1 | 本次未修改远端 `.env`。 |
| BRIDGE-HARDEN-FUNCTION-20260509 | PASS | `agent_identity_bridge` Function 在正式 Open WebUI 中为全局启用 filter。 | `verify-openwebui-function.sh` 返回 `source=compose-db`、`type=filter`、`is_active=true`、`is_global=true`、valve keys 为 `AGENT_BRIDGE_ISSUER`、`AGENT_BRIDGE_SECRET`、`TARGET_MODEL`。 | P1 | 只记录 key names，不记录 secret 值。 |
| BRIDGE-HARDEN-MIGRATION-20260509 | PASS | 正式 Postgres 已应用 nonce 表和 message 去重字段/索引。 | `open_webui_bridge_nonces` 表存在；`agent_session_messages.external_message_id` 存在；唯一索引 `ux_session_messages_external_message` 存在。 | P1 |  |
| BRIDGE-HARDEN-SMOKE-20260509 | PASS | 合成 Open WebUI subject/chat 完成审批建链、follow-up run、message 去重、nonce replay 拦截和关闭。 | 临时 request 审批后 binding `active`；follow-up message count `0 -> 1`；同 message_id/new nonce 后仍为 `1`；同 nonce replay 返回 `{"error":"conflict"}`；关闭后 binding `closed`。 | P1 | 使用合成 subject，未污染已有用户 active binding。 |
| BRIDGE-HARDEN-ISOLATION-20260509 | PASS | 相同 chat/model 下不同 Open WebUI subject 不能复用对方 binding。 | subject B 发送普通消息后，subject A session 对应 message count 仍为 `0`，subject B 无 binding。 | P1 | 覆盖 subject 级隔离；未重复执行第二个真实浏览器账号登录。 |
| BRIDGE-HARDEN-RESTART-20260509 | PASS | Orchestrator 重启后仍从 Manager/Postgres 复用 active binding。 | 重启 `agent-orchestrator` 后健康恢复；subject A post-restart follow-up message count `0 -> 1`，Worker run 完成。 | P1 | 证明 binding 不依赖 Orchestrator 内存。 |
| BRIDGE-HARDEN-LOGS-20260509 | PASS | 复测窗口内关键服务无错误关键词。 | `docker compose logs --since=10m agent-manager agent-orchestrator agent-worker agent-observer` 未检出 `panic/error/failed/deadletter/unauthorized/forbidden`。 |  |  |

## 2026-05-09 Agent Identity Bridge 真实账号复测

| 用例 ID | 状态 | 实际结果 | 证据 | 问题等级 | 备注 |
| --- | --- | --- | --- | --- | --- |
| BRIDGE-REAL-AUTH-20260509 | PASS | 正式 Open WebUI 真实 admin 与普通 user 账号均可通过 Open WebUI auth 访问 API。 | `GET /api/v1/auths/` 对两个真实账号均返回 HTTP 200。 | P1 | 使用服务器端短期 JWT 代表真实账号，未输出 token、密码或 secret。 |
| BRIDGE-REAL-BOOTSTRAP-20260509 | PASS | 两个真实账号分别通过 `/api/chat/completions` 触发 Function、Orchestrator 和 Manager，并进入审批状态。 | 普通 user 请求 `req_019e0acbc79071d2b9b19446ad10c419`、admin 请求 `req_019e0acbc9d57d53966e2c13d3c4b5ad`，状态均为 `approval_required`。 | P1 | admin 账号未被自动映射为 Agent Platform admin，符合 `AGENT_BRIDGE_ADMIN_ROLE_MAPPING=disabled`。 |
| BRIDGE-REAL-BINDING-20260509 | PASS | 审批后两个真实账号各自创建独立 binding/session。 | user session `sess_019e0acbc8877a70a382dd3b163585ae`，admin session `sess_019e0acbcaaa74639eadff79c6beb161`，二者不同。 | P1 |  |
| BRIDGE-REAL-CROSS-CHAT-20260509 | PASS | admin 使用 user 的测试 chat id 发普通消息时，不会复用或污染 user 的 agent session binding。 | user session 对 admin cross message 的 count `0 -> 0`；`admin + user_chat` 没有生成 binding。 | P1 | 覆盖真实账号维度的 subject 隔离。 |
| BRIDGE-REAL-FOLLOWUP-20260509 | PASS | 两个真实账号的后续消息只 append 到各自 session。 | user follow-up own count `0 -> 1`、other count `0 -> 0`；admin follow-up own count `0 -> 1`、other count `0 -> 0`。 | P1 |  |
| BRIDGE-REAL-DEDUP-20260509 | PASS | 真实 Open WebUI user 重复发送同一 `user_message_id`，不重复 append session message。 | user duplicate count `1 -> 1`。 | P1 | 复盘后已修正 Filter：优先使用 `user_message_id`，缺失时才退回 assistant placeholder `message_id`。 |
| BRIDGE-REAL-USER-MESSAGE-ID-PRIORITY-20260509 | PASS | 复盘发现此前去重测试没有证明 `user_message_id` 优先于 assistant placeholder `message_id`；修复后已用真实普通 user 账号复测同一 `user_message_id` 搭配不同 assistant id。 | request `req_019e0b67510b7013a436b3c5fa9b4253`、session `sess_019e0b6754e77892afc0b4baf2534d55`；`external_message_id=openwebui:<user_message_id>` 计数 `0 -> 1 -> 1`；测试 binding closed，测试 chat 删除 HTTP 200。 | P1 | 这条是完成前反思补测，覆盖真实 Open WebUI 元数据形态。 |
| BRIDGE-FUNCTION-RECOVERY-20260509 | PASS | 更新正式 Function 时曾将 `function.updated_at` 写成字符串，导致 Open WebUI 启动校验失败；已修复为整数 epoch 并重新健康检查。 | Open WebUI `healthy`；`verify-openwebui-function.sh` 返回 `status=ok`；DB 校验 `FUNCTION_UPDATED_AT_IS_INT=True`、`USER_MESSAGE_ID_PRIORITY=True`。 | P1 | 记录部署操作缺口，避免只看最终 PASS。 |
| BRIDGE-REAL-CLOSE-20260509 | PASS | 两个真实账号的测试 binding 均可关闭，测试 chat 已清理。 | user/admin close 后 binding 状态均为 `closed`；`DELETE /api/v1/chats/{id}` 对两个测试 chat 均返回 HTTP 200。 | P1 |  |
| BRIDGE-REAL-LOGS-20260509 | PASS | 修复恢复后的真实账号复测窗口内关键服务无新错误关键词。 | `docker compose logs --since=5m open-webui agent-manager agent-orchestrator agent-worker agent-observer` 未检出 `panic/traceback/exception/error/failed/deadletter/unauthorized/forbidden`；`docker compose ps` 显示 Open WebUI、Manager、Orchestrator healthy，Worker/Observer running。 |  | 更早窗口包含 `updated_at` 类型错误，已由 `BRIDGE-FUNCTION-RECOVERY-20260509` 单独记录。 |

剩余未覆盖边界：

1. 本轮未通过浏览器手动输入密码登录；已通过正式 Open WebUI auth 以真实 admin/user 账号调用真实聊天 API。
2. Hermes 上游 payload 未做抓包级验证；代码单测覆盖 passthrough 前删除 `agent_bridge_context`，普通 passthrough 仍正常。

## 修复结论

当前状态：

1. `ISSUE-001` 已通过 admin 配置修复并复测通过。
2. `ISSUE-003` 已部署并通过 API 级跨请求复测。
3. `ISSUE-002` 对新会话正文搜索已部分恢复，但旧会话正文关键词仍失败；需要 Open WebUI 代码修复、索引重建或升级验证。
4. `ISSUE-004` 已在正式 compose 中修复，Open WebUI origin IP 不再被 Agent Platform 动态占用。
5. `ISSUE-005` 已在 Orchestrator 中修复并部署，Open WebUI 追问建议不会再污染 Agent Platform 请求列表。
6. `ISSUE-006` 不阻断核心路径；建议后续补充停止生成按钮可访问标签。
7. `ISSUE-007` 已在 Manager 中修复并部署，审批后的 agent 归属原始请求人，Worker 复用和多 session API 路径通过。
8. `ISSUE-008` 已通过 Agent Identity Bridge 修复并部署：Open WebUI Function 注入签名上下文，Orchestrator 验签并签发 Manager JWT，Manager 持久化 Open WebUI chat 到 agent session binding，后续消息可自动创建 Worker run。
9. `ISSUE-009` 已完成 hardening 代码、正式环境部署和复测：Function 校验、migration、message 去重、nonce replay、subject 隔离、Orchestrator 重启后复用和日志检查均通过。
