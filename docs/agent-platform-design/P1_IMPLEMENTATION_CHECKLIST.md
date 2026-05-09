# P1 Implementation Checklist

状态：P1 implementation gap 已补齐，本地和远端 Docker 测试均已通过；正式部署 smoke 已完成并写入 `docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md`。

## 1. Contract Freeze

- [x] 保持 Manager 授权、Open WebUI Bridge、run/session 状态机、Worker claim、Memory schema 和 audit contract 不变。
- [x] 只新增 P1/P2 readiness schema：`external_action_plans`、`credential_leases`。
- [x] 为 Runtime safe error、Bridge regression、P2 readiness dry-run 和 report discussion 增加回归测试。

总结：P1 通过新增 adapter、schema 和 API 扩展 P0，没有改写 P0 控制面或 Worker 状态机。

## 2. Hermes Runtime Adapter

- [x] 新增 `HermesRuntimeClient`，通过 OpenAI-compatible `/chat/completions` 调用 Hermes。
- [x] 支持 `AGENT_RUNTIME_MODE=minimal|hermes`、Hermes base URL/model/API key/timeout 配置。
- [x] 支持 `AGENT_RUNTIME_HERMES_PROFILE_MODELS`，按 `agent.hermes_profile` 实际路由 Hermes model。
- [x] Runtime 调用携带 `x-agent-trace-id`，metadata 保留 runtime、profile、model、duration 和 read-only 标记。
- [x] 单测覆盖 success、profile routing、timeout、5xx、malformed response 和 trace_id。

总结：P1 Worker 可以从 Minimal Runtime 切到真实 Hermes Runtime，错误返回仍是安全摘要，不泄露 prompt、context 或 credential。

## 3. Session / Run Path

- [x] Worker claim 后加载 agent profile、session context 和只读 connector snapshot。
- [x] `session_message` run 通过 `RuntimeClient::send_session_message` 执行，仍由 Worker 完成 claim/finish。
- [x] Runtime 输出会追加为同一 `agent_session` 的 assistant message，并绑定 `run_id` 与 `result_ref`。
- [x] 保持 claim、heartbeat、retry、dead_letter、finish audit 状态机不变。
- [x] 单测覆盖 session run 读取 snapshot 并写回 assistant message。

总结：Bridge 后续消息仍走既有 session/run/Worker 链路，只替换 Runtime adapter。

## 4. Read-only Connector

- [x] 保留 `LocalReadOnlyConnector` 作为默认 snapshot adapter。
- [x] 新增可配置 `HttpReadOnlyConnector`，使用 `GET /snapshots?connector=...&resource=...` 获取只读 snapshot。
- [x] 单测覆盖 HTTP connector trace、snapshot 解析，以及错误路径不泄露 secret/resource。
- [x] Runtime 只消费 snapshot summary，不获取写 credential。

总结：P1 可以接入真实只读 snapshot provider；未配置 provider 时仍保持本地可测闭环。

## 5. Observer Upgrade

- [x] Observer snapshot 增加 retry、timeout、completed runtime latency、context size 和 external-action plan 状态摘要。
- [x] Observer report findings/recommendations 纳入 runtime quality signal 和 risk taxonomy。
- [x] Observer 仍只生成报告和建议，不触发控制动作。
- [x] 新增 System Observer status session，授权 admin/operator 可快速把最新报告、findings、recommendations 和脱敏 evidence 带入专用会话。

总结：Observer 可以评测 P1 Runtime 行为，并通过只读 System Observer session 支持深入诊断；它不会越界成为控制面。

## 6. Report Discussion

- [x] 新增 `POST /v1/admin/observer/reports/{report_id}/discussions`。
- [x] `agentctl observer discuss` 支持围绕 report 创建普通 `agent_session`。
- [x] Discussion context 只包含 report summary、redacted evidence refs、target agent/resource 和用户初始问题。
- [x] 审计关联 `report_id / session_id / agent_id / trace_id`。

总结：授权 operator/admin 可以围绕 Observer report 进入受控讨论；Observer 本身不参与对话。

## 7. P2 Readiness

- [x] 新增 `ExternalActionPlan`、`CredentialLease`、`CredentialProvider`、`WriteConnector` contract。
- [x] 新增 no-op credential provider 和 no-op write connector。
- [x] 新增 `POST /v1/admin/runs/{run_id}/external-action-plans/dry-run` 和 `agentctl runs dry-run-external-action`。
- [x] dry-run 校验 approval 状态、credential_scope、active resource lock 和 critical-risk deny。
- [x] P1 只产生 dry-run ready/rejected 状态；不获取真实 credential，不调用真实写 connector，不进入真实 external write。

总结：P2 所需写入 contract 已固定；P2 只应启用真实 provider/connector，不重写 P0/P1 核心链路。

## 8. Smoke Readiness

- [x] `deploy/docker-compose.yml` 为 worker 增加 P1 Runtime 环境变量，所有 secret 均从 `.env` 引用。
- [x] `deploy/README.md` 记录 P1 Runtime 和 read-only connector 配置。
- [x] 正式部署 smoke：登录、模型选择、基础聊天、会话保存。
- [x] Bridge regression：binding 复用、后续消息 create run、关闭 session。
- [x] Hermes smoke：`agent_session` 多轮与 `agent_run` 只读分析。
- [x] Report discussion smoke。
- [x] System Observer status session smoke。
- [x] P2 readiness dry-run smoke。
- [x] Postgres migration smoke。

总结：P1 实现、正式镜像、真实 Open WebUI 关键路径、Bridge 后续 run、Observer discussion、System Observer status session、P2 dry-run readiness 和 Postgres migration 均已完成验证；Open WebUI session secret 已改为 `.env` 管理并复测通过。
