# ISSUE-005: Open WebUI 追问建议提示被误判为 Agent Platform 控制请求

## 状态

RESOLVED

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `AGENT-UI-CTRL-20260508` | PASS | 用户直接输入 Agent Platform 控制请求时，Manager 正确创建审批请求。 |
| `AGENT-FOLLOWUP-FP-20260508` | FAIL->PASS | Open WebUI 追问建议内部提示曾误触发 Agent Platform 请求；修复部署后不再复现。 |

## 影响

Open WebUI 会在回答后向模型发送内部提示，用于生成“追问”建议。该提示中包含完整 `### Chat History`，历史里如果出现 `创建 agent ...`，Orchestrator 旧的宽松关键词识别会把这条内部提示误判成新的用户控制请求。

这会污染 Agent Platform 请求列表和审计记录，产生重复审批噪音。虽然默认仍是 `approval_required`，不会直接执行副作用，但会降低 Agent Platform 控制面的可信度。

## 证据

- 用户真实 UI 指令创建了预期请求：`req_019e071a9a2572c08488577cc52d77d6`。
- 随后 Open WebUI 自动追问建议提示额外创建了非预期请求：`req_019e071a9a667a13a10be4f718ee3746`。
- 非预期请求的 `intent_text` 不是用户直接命令，而是以 `### Task: Suggest 3-5 relevant follow-up questions...` 开头，内部 `### Chat History` 中包含上一轮 `创建 agent ...`。

## 修复记录

| 时间 | 操作 | 结果 |
| --- | --- | --- |
| 2026-05-08 18:33 CST | 定位误判来源。 | `looks_like_agent_request` 对整段 Open WebUI 内部提示做关键词匹配，命中了 `### Chat History` 中的历史用户文本。 |
| 2026-05-08 18:34 CST | 修改 Orchestrator 分类逻辑。 | 仅使用 `### Chat History:` 或 `<chat_history>` 之前的直接用户文本做控制请求识别。 |
| 2026-05-08 18:34 CST | 增加回归单测。 | `ignores_open_webui_followup_prompt_chat_history` 覆盖内部追问提示；`detects_direct_agent_create_request` 覆盖真实用户控制请求。 |
| 2026-05-08 18:38 CST | 同步并部署远程正式 Orchestrator。 | `agent-orchestrator` 重新构建并健康启动。 |

## 验证

- 本地 `cargo test --workspace` 通过。
- 远程复测同类 Open WebUI 追问建议提示：
  - 复测前最新请求 ID：`req_019e071a9a667a13a10be4f718ee3746`
  - 复测后最新请求 ID：`req_019e071a9a667a13a10be4f718ee3746`
  - 响应不含 `approval_required`
- 远程复测直接用户控制指令仍有效：
  - 新请求：`req_019e072a5bc87352b4ca99c26664157f`
  - 状态：`approval_required`

## 后续动作

已解决。后续如果增加更多 Agent Platform 自然语言入口，应优先采用显式 metadata、工具调用或专用命令通道，避免只靠全文关键词扫描。
