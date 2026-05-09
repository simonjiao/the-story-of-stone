# P2 Implementation Checklist

状态：仓库侧实现完成。当前已完成 Agent Manager external-action 平台路径、通用 HTTP CredentialProvider / WriteConnector contract、Manager 级 apply / compensate workflow、仓库内 `agent-action-gateway` + `action-journal` 低风险 target、本地单测和端到端 smoke。默认部署仍关闭写入；真实第三方 provider / connector 不在仓库里伪造，必须在目标环境按本文明确的配置位接入并运行 contract smoke。

## 0. 前提反思

- [x] P1 固定了 `ExternalActionPlan`、`CredentialLease`、`CredentialProvider`、`WriteConnector`、dry-run policy 和审计事件。
- [x] P0/P1 的 Manager 授权、Open WebUI Bridge、run/session 状态机、Worker claim、Memory schema 和 audit contract 未被重写。
- [x] 当前部署默认不启用真实 write connector 或 credential provider，必须显式配置 Action Gateway 或第三方 provider / connector 后才会执行外部写入。
- [x] 当前实现提供通用 HTTP contract，并提供仓库内 `action-journal` target 用于低风险本地 smoke。
- [x] 旧命名、旧字段、旧路由和旧 CLI 不保留兼容层；当前 contract 统一使用 `ExternalAction` / `external_action` / `external-action`。
- [x] 第三方生产系统前提不再以仓库内硬编码 adapter 表示；接入边界固定为 HTTP CredentialProvider、HTTP WriteConnector 和 `external-action-contract-smoke.sh` 验收脚本。

总结：P2 的仓库侧前提已满足；真实第三方系统是否满足，以目标环境配置和 smoke 输出为准。

## 1. Credential Provider

- [x] `CredentialProvider` 增加 active lease 能力。
- [x] 新增 HTTP CredentialProvider，调用 `POST /credential-leases`。
- [x] active lease 只持久化 opaque `provider_ref`、scope、状态和过期时间；不保存 secret 值。
- [x] 新增 `agent-action-gateway` 的 `action-journal` CredentialProvider，签发 opaque `action-journal-credential://...` provider ref，并通过端到端 smoke 验证。
- [x] 第三方 CredentialProvider 的正式接入位置已明确：`AGENT_CREDENTIAL_PROVIDER_BASE_URL`、`AGENT_CREDENTIAL_PROVIDER_API_KEY`、`AGENT_CREDENTIAL_PROVIDER_TIMEOUT_SECONDS`、`AGENT_CREDENTIAL_LEASE_TTL_SECONDS`。
- [x] 第三方 CredentialProvider 的验收入口已明确：运行 `agent-platform/scripts/external-action-contract-smoke.sh`，并设置 `EXTERNAL_ACTION_CREDENTIAL_SCOPE`。

总结：credential 通过 provider reference 进入执行边界，不进入 prompt、memory、audit 明文或长期 agent state。

## 2. Write Connector / Action Target

- [x] `WriteConnector` 增加 execute 和 compensate 能力。
- [x] 新增 HTTP WriteConnector，调用 `POST /action-executions/execute` 和 `POST /action-executions/compensate`。
- [x] execute 输入包含 plan、`idempotency_key`、opaque provider ref、payload 和 trace_id；connector 成功时必须返回 `result_ref` 和 `compensation_ref`。
- [x] compensate 输入包含 plan、`compensation_ref`、reason、payload 和 trace_id；connector 成功时必须返回 `status=compensated` 和 `result_ref`。
- [x] Manager 校验 connector accepted result，缺少 applied / compensated 状态、`result_ref` 或 `compensation_ref` 时记录明确错误并写 audit。
- [x] 新增 `agent-action-gateway` 的 `action-journal` WriteConnector / JSONL target，实际写入独立 target log，按 plan id 幂等返回稳定 `result_ref` / `compensation_ref`。
- [x] 新增 `action-journal` compensation endpoint，并通过 Manager 级 compensation workflow 验证补偿引用可执行。
- [x] 第三方 WriteConnector 的正式接入位置已明确：`AGENT_WRITE_CONNECTOR_BASE_URL`、`AGENT_WRITE_CONNECTOR_API_KEY`、`AGENT_WRITE_CONNECTOR_TIMEOUT_SECONDS`、`AGENT_WRITE_CONNECTOR_MAX_ATTEMPTS`。
- [x] 第三方 action target 的验收入口已明确：运行 `agent-platform/scripts/external-action-contract-smoke.sh`，并设置 `EXTERNAL_ACTION_CONNECTOR`、`EXTERNAL_ACTION_NAME`、`EXTERNAL_ACTION_RESOURCE_REF`、`EXTERNAL_ACTION_PAYLOAD_JSON` 和 `EXTERNAL_ACTION_COMPENSATE_PAYLOAD_JSON`。

总结：当前满足通用 HTTP connector contract、仓库内低风险 target smoke 和第三方接入验收入口；生产目标是否可用由目标环境 smoke 证明。

## 3. Apply / Compensate Path

- [x] 新增 `POST /v1/admin/runs/{run_id}/external-action-plans/{plan_id}/apply`。
- [x] apply 只接受 `dry_run_ready` plan，并复用 approval、credential_scope、critical risk 和 resource lock 校验。
- [x] 执行前获取 `resource_locks`，执行后释放；锁冲突或 precheck 失败会推进为明确失败状态并写 audit。
- [x] apply audit 记录 `plan_id`、`run_id`、`approval_id`、`lock_id` 和结果状态，便于释放锁后仍可回溯本次外部动作持有过的锁。
- [x] connector 支持 timeout 和 bounded retry；耗尽后 plan 进入 `failed`，错误码为 `connector_dead_letter`，非终态 run 同步进入 dead-letter。
- [x] 新增 `POST /v1/admin/runs/{run_id}/external-action-plans/{plan_id}/compensate`。
- [x] compensate 只接受 `applied` plan，必须存在 `compensation_ref`，执行前同样获取 `external_action` resource lock，并通过 audit 记录 `lock_id`。
- [x] compensate 成功后 plan 进入 `compensated`，持久化 `compensation_result_ref`，Observer 能计入 external action compensated 统计。

总结：外部动作已可被授权、审批、加锁、执行、补偿、审计和回溯；Runtime / Worker 仍不能绕过 Manager 直接写外部目标。

## 4. CLI / Deploy

- [x] `agentctl runs dry-run-external-action` / `agentctl runs apply-external-action` / `agentctl runs compensate-external-action` 支持按 run/plan 执行 external action dry-run / apply / compensate。
- [x] `deploy/docker-compose.yml` 只从 `.env` 引用 external action provider / connector 配置。
- [x] `deploy/README.md` 记录 external action 环境变量，不输出或硬编码 secret。
- [x] `agent-action-gateway` binary 已加入镜像，`deploy/docker-compose.yml` 提供 `action-gateway-smoke` profile，默认不启动。
- [x] `agent-platform/scripts/action-gateway-smoke.sh` 可启动 Manager + Action Gateway，完成 approval、dry-run、apply、target write 和 Manager compensation 验证。
- [x] `agent-platform/scripts/external-action-contract-smoke.sh` 可对任意已配置的第三方 provider / connector 执行同一 Manager workflow，并断言 `dry_run_ready`、`applied` 和 `compensated`。

总结：P2 默认关闭；本地低风险 smoke 覆盖仓库内 target，第三方目标通过同一 contract smoke 在目标环境验收。

## 5. Observer

- [x] Observer risk taxonomy 增加 applied / failed / compensated external-action 计数。
- [x] external-action failed count、approval bypass attempt、resource lock conflict、abnormal write result 和 audit failure decision 会进入健康评估。

总结：Observer 仍只读，只报告外部动作失败、锁压力、补偿状态和异常决策，不自动修复。

## 6. Validation

- [x] 本地 Rust 单测覆盖 HTTP provider、HTTP write connector、Action Gateway、external action apply active lease、resource lock release、lock conflict、invalid connector result、connector dead-letter、HTTP target、compensation 和 audit。
- [x] `action-journal` CredentialProvider smoke：签发 opaque provider ref，不返回 secret。
- [x] `action-journal` WriteConnector / 目标 target smoke：写入 JSONL target log，返回 `result_ref` 和 `compensation_ref`。
- [x] 端到端 external action smoke：配置 provider / connector 后执行一次低风险 approved external action，并核对 dry-run、plan 状态、credential lease、target write、connector result_ref、compensation_ref 和 compensation_result_ref。
- [x] 第三方 provider / connector 切换规则已固定为配置入口和 contract smoke；目标环境完成接入后必须重跑同一 smoke，结果以脚本输出为准。

总结：P2 仓库侧实现、仓库内 Action Gateway target 和本地端到端 smoke 已完成；真实第三方目标不在本地提交中伪造完成证明。
