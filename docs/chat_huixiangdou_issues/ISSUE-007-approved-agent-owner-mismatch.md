# ISSUE-007: 审批后 agent 归属审批人而不是原始请求人

## 状态

RESOLVED

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `AGENT-APPROVAL-OWNER-20260508` | FAIL->PASS | 审批创建 agent 后，agent 应归属原始请求人，而不是审批人。 |
| `AGENT-MULTI-SESSION-20260508` | PASS | 同一请求人可在同一 agent 下创建多个 session。 |
| `AGENT-WORKER-RUN-20260508` | PASS | 两个 session run 均被 Worker claim 并完成。 |

## 影响

审批流旧实现使用审批人的 `AuthContext` 创建 agent，并用审批人作为复用查询 owner。结果是 UI/用户发起的请求被审批后，agent 进入审批人的 `my-agents`，原始请求人看不到该 agent，也不能基于它创建自己的 session/run。

这会破坏 agent 复用、多 session 和用户侧审计归属，是 Agent Platform 控制面功能性问题。

## 证据

- 修复前审批请求 `req_019e072a5bc87352b4ca99c26664157f` 后得到 `agent_019e0732cc207a11b34713104a7f2e6d`。
- 该 agent 出现在 `admin` 的 `my-agents` 下，而不在原始请求人的 `my-agents` 下。
- 根因是 `request_services::fulfill_request` 使用 `auth.user_id` 进行 `find_reusable_agent` 和 `AgentInstance::new` owner 赋值；审批时 `auth.user_id` 是审批人。

## 修复记录

| 时间 | 操作 | 结果 |
| --- | --- | --- |
| 2026-05-08 18:47 CST | 复核审批后 agent 归属。 | 确认审批创建的测试 agent 归属 `admin`，不是原始请求人。 |
| 2026-05-08 18:49 CST | 修改 Manager fulfill 逻辑。 | agent owner 和复用查询 owner 改为 `request.requested_by_user`。 |
| 2026-05-08 18:50 CST | 增加回归单测。 | `approved_create_agent_keeps_requester_as_owner` 覆盖请求人与审批人不同的审批路径。 |
| 2026-05-08 18:55 CST | 重建并部署正式 Agent Platform 镜像。 | `agent-manager`、`agent-orchestrator`、`agent-worker`、`agent-observer` 均使用新 Agent Platform 镜像重启；当前正式 tag 已改为 `formal`。 |

## 验证

- 本地 `cargo test -p agent-manager approved_create_agent_keeps_requester_as_owner` 通过。
- 远端复测请求 `req_019e073c5df57450902773f4c872c1d0` 审批后得到 `agent_019e073c5ea27640b34fc51a49be5f96`。
- `codex-owner-20260508` 的 `my-agents` 可见该 agent，`admin` 的 `my-agents` 不包含该 agent。
- 在该 agent 下创建两个 session：
  - `sess_019e073cf05b72f08e3d20c625c86443`
  - `sess_019e073cf1eb724188643a0e15effef2`
- 两个 run 均完成：
  - `run_019e073cf1637c40adb597c8960649ed`
  - `run_019e073cf2f87580add2d1e6500811af`
- `agentctl agents list` 显示该 agent `active_session_count=2`，`last_run_status=completed`。
- `agentctl audit --limit 40` 包含两个 run 的 `worker:run_claim`、`worker:run_status` 和 `worker:run_finish`。

## 后续动作

已解决。保留现有回归测试，避免后续审批流改动再次把资源归属切到审批人。
