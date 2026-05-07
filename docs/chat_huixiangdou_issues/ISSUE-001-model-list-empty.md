# ISSUE-001: 登录后模型列表为空

## 状态

RESOLVED

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `CHAT-01` | FAIL | 模型选择器可打开，但模型列表为空，显示 `未找到结果`。 |
| `CHAT-03` | BLOCKED | 输入短消息后点击发送，页面显示 `未选择模型`，消息没有提交为聊天请求。 |
| `CHAT-01-RERUN` | PASS | admin 修复后，登录首页默认选中 `hermes-agent`。 |
| `CHAT-03-RERUN` | PASS | admin 修复后，简短中文问答可提交并返回回答。 |

## 影响

用户无法选择模型，基础聊天无法继续验证。该问题阻断所有依赖聊天能力的测试。

## 证据

- 修复前：模型按钮 `选择模型` 展开后，搜索框 `搜索模型` 下方显示 `未找到结果`。
- 修复前：发送 `用一句话说明你能做什么。` 后 toast 显示 `未选择模型`。
- 修复后：首页导航显示 `已选择：hermes-agent`。
- 修复后：会话 `/c/932c625d-daa0-468c-93fa-c60e97f1070e` 返回中文回答。

## 修复记录

| 时间 | 操作 | 结果 |
| --- | --- | --- |
| 2026-05-07 13:00 CST | 暂停后续测试，定位模型列表为空。 | 当前测试账号无 admin 权限，无法在站点内修复。 |
| 2026-05-07 13:04 CST | 给出 Open WebUI admin 修复路径。 | 需要在 Admin Settings → Connections 添加/启用 provider 或配置模型 allowlist。 |
| 2026-05-07 14:00 CST | 用户完成 admin 侧修复后复测。 | `hermes-agent` 已可见并默认选中，基础聊天通过。 |

## 后续动作

已解决。后续若模型再次不可见，优先检查 Open WebUI Admin Settings → Connections 的 provider 开关、API Key、`/models` 能力和 Model IDs allowlist。
