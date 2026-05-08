# chat.huixiangdou.top 测试问题索引

来源测试报告：`docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md`

| 问题 ID | 状态 | 等级 | 关联测试 | 摘要 |
| --- | --- | --- | --- | --- |
| [ISSUE-001](ISSUE-001-model-list-empty.md) | RESOLVED | P1 | `CHAT-01`, `CHAT-03`, `CHAT-01-RERUN`, `CHAT-03-RERUN` | 登录后模型列表为空，基础聊天被阻断；admin 修复后复测通过。 |
| [ISSUE-002](ISSUE-002-history-body-search.md) | BLOCKED_CODE_REQUIRED | P2 | `HIST-04A`, `HIST-04B` | 历史搜索按消息正文关键词无法找到会话；官方文档预期支持正文搜索，但当前部署无配置项可修。 |
| [ISSUE-003](ISSUE-003-cross-chat-memory-visibility.md) | RESOLVED | P2 | `CTX-04`, `MEMORY-01` | 已正式部署 Hermes memory 关闭配置，API 级跨请求复测返回 `UNKNOWN_ONLY`。 |
| [ISSUE-004](ISSUE-004-open-webui-origin-ip-conflict.md) | RESOLVED | P1 | `DEPLOY-03`, `NET-01`, `PUBLIC-01` | Open WebUI 固定 origin IP 被 Agent Platform 动态占用；已改为稳定内网地址分配。 |

## 状态定义

- `OPEN`：仍需修复或产品决策。
- `READY_FOR_DEPLOY`：仓库内修复已完成，等待部署应用并复测。
- `RESOLVED`：已修复并通过复测。
- `BLOCKED`：当前权限或环境无法继续定位/修复。
- `BLOCKED_CODE_REQUIRED`：无配置级修复，需要改上游/服务端代码或升级验证。
- `WONTFIX`：确认符合设计预期，不作为缺陷修复。
