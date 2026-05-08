# ISSUE-002: 历史搜索按消息正文关键词无法找到会话

## 状态

BLOCKED_CODE_REQUIRED

## 等级

P2

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `HIST-01` | PASS | 刷新当前会话后，消息记录可恢复。 |
| `HIST-04A` | FAIL | 搜索消息正文关键词 `codex-test-basic-20260507`，搜索弹窗显示 `未找到结果`。 |
| `HIST-04B` | PASS | 搜索自动生成标题 `AI 助手能力介绍`，可以找到目标会话。 |
| `HIST-NEW-BODY-20260508` | PASS | 搜索新长回答正文关键词 `codex-test-p0-long-20260508-A`，可以找到新会话。 |
| `HIST-OLD-BODY-20260508` | FAIL | 搜索旧正文关键词 `codex-test-basic-20260507`，仍显示 `未找到结果`。 |

## 影响

用户如果只记得消息正文内容，而不记得会话标题，可能无法通过搜索找回历史会话。

## 证据

- 目标会话刷新后可恢复，正文中包含 `codex-test-basic-20260507`、`RENDER_OK`、`收到两行`。
- 搜索 `codex-test-basic-20260507` 返回 `未找到结果`。
- 搜索 `AI 助手能力介绍` 可见链接 `/c/932c625d-daa0-468c-93fa-c60e97f1070e`。
- 2026-05-08 正式部署后，新长回答会话 `/c/cd0fa0fe-43b5-49e8-ac93-bb905e91e32a` 可以通过正文关键词 `codex-test-p0-long-20260508-A` 搜到。
- 同轮回归中，旧正文关键词 `codex-test-basic-20260507` 仍返回 `未找到结果`。

## 当前判断

历史数据本身已保存，问题集中在搜索范围、索引策略或旧数据索引重建。2026-05-08 回归显示新会话正文关键词可以命中，但旧会话正文关键词仍不能命中，因此当前行为不是完全不可用，而是不一致。

Open WebUI 官方文档说明全局历史搜索应对 `Chat Titles`、`Message Content` 和 tags 做 fuzzy search，并且 agentic `search_chats` 工具也应搜索标题和消息内容。因此这不是产品预期不清，而是当前部署行为与官方说明不一致。

当前 `deploy/` 只有 Open WebUI 镜像、环境变量和数据卷配置；未发现可用于开启“消息正文历史搜索”的部署配置项。这个问题不能通过现有配置修正。

## 修复结论

需要改 Open WebUI 代码、重建历史索引或升级验证：

1. 在 Open WebUI 后端搜索实现中确认全局搜索是否只查标题，或正文字段是否未进入搜索索引。
2. 确认旧会话消息正文是否进入搜索索引，以及是否需要迁移或重建索引。
3. 修复搜索接口，使新旧消息正文命中时都返回对应 chat。
4. 若当前镜像版本存在已知 bug，升级到包含修复的 Open WebUI 版本并复测。

## 官方依据

- Open WebUI History & Search: https://docs.openwebui.com/features/chat-conversations/chat-features/history-search/

## 后续动作

1. 获取或 fork Open WebUI 服务端代码，定位历史搜索 API。
2. 增加或修复消息正文搜索覆盖。
3. 修复后复测 `HIST-04A`：搜索 `codex-test-basic-20260507` 应能找到目标会话。
