# chat.huixiangdou.top 测试问题索引

来源测试报告：`docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md`

| 问题 ID | 状态 | 等级 | 关联测试 | 摘要 |
| --- | --- | --- | --- | --- |
| [ISSUE-001](ISSUE-001-model-list-empty.md) | RESOLVED | P1 | `CHAT-01`, `CHAT-03`, `CHAT-01-RERUN`, `CHAT-03-RERUN` | 登录后模型列表为空，基础聊天被阻断；admin 修复后复测通过。 |
| [ISSUE-002](ISSUE-002-history-body-search.md) | BLOCKED_CODE_REQUIRED | P2 | `HIST-04A`, `HIST-04B` | 历史搜索按消息正文关键词无法找到会话；官方文档预期支持正文搜索，但当前部署无配置项可修。 |
| [ISSUE-003](ISSUE-003-cross-chat-memory-visibility.md) | READY_FOR_DEPLOY | P2 | `CTX-04` | 已通过 Hermes 配置生成脚本关闭持久记忆，需部署后复测。 |

## 状态定义

- `OPEN`：仍需修复或产品决策。
- `READY_FOR_DEPLOY`：仓库内修复已完成，等待部署应用并复测。
- `RESOLVED`：已修复并通过复测。
- `BLOCKED`：当前权限或环境无法继续定位/修复。
- `BLOCKED_CODE_REQUIRED`：无配置级修复，需要改上游/服务端代码或升级验证。
- `WONTFIX`：确认符合设计预期，不作为缺陷修复。
