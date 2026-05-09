# P2 Implementation Checklist

状态：部分实现。当前已完成 Agent Manager external-action 平台骨架、通用 HTTP CredentialProvider / WriteConnector adapter、仓库内 `agent-action-gateway` + `action-journal` 低风险 target、本地单测和端到端 smoke。尚未完成第三方生产 provider / connector 接入、正式 Manager compensation workflow 和目标部署环境 smoke，因此不能宣告 P2 全部完成。

## 0. 前提反思

- [x] P1 固定了 `ExternalActionPlan`、`CredentialLease`、`CredentialProvider`、`WriteConnector`、dry-run policy 和审计事件。
- [x] P0/P1 的 Manager 授权、Open WebUI Bridge、run/session 状态机、Worker claim、Memory schema 和 audit contract 未被重写。
- [x] 当前部署默认不启用真实 write connector 或 credential provider，必须显式配置 Action Gateway 或第三方 provider / connector 后才会执行外部写入。
- [x] 当前实现提供通用 HTTP contract，并提供仓库内 `action-journal` target 用于低风险本地 smoke。
- [x] 旧命名、旧字段、旧路由和旧 CLI 不保留兼容层；当前 contract 统一使用 `ExternalAction` / `external_action` / `external-action`。
- [ ] 第三方生产系统前提未满足：尚未接入具体生产 provider / connector，也未完成对应目标环境 smoke。

总结：平台前提部分满足；已具备受控外部动作的最小平台路径，但生产第三方系统接入仍是未完成项。

## 1. Credential Provider

- [x] `CredentialProvider` 增加 active lease 能力。
- [x] 新增 HTTP CredentialProvider，调用 `POST /credential-leases`。
- [x] active lease 只持久化 opaque `provider_ref`、scope、状态和过期时间；不保存 secret 值。
- [x] 新增 `agent-action-gateway` 的 `action-journal` CredentialProvider，签发 opaque `action-journal-credential://...` provider ref，并通过端到端 smoke 验证。
- [ ] 第三方生产 CredentialProvider 尚未实现和验证。

总结：credential 通过 provider reference 进入执行边界，不进入 prompt、memory、audit 明文或长期 agent state；当前只验证了通用 HTTP adapter 与仓库内 action-journal provider。

## 2. Write Connector / Action Target

- [x] `WriteConnector` 增加 execute 能力。
- [x] 新增 HTTP WriteConnector，调用 `POST /action-executions/execute`。
- [x] execute 输入包含 plan、`idempotency_key`、opaque provider ref、payload 和 trace_id；connector 成功时必须返回 `result_ref` 和 `compensation_ref`。
- [x] Manager 校验 connector accepted result，缺少 applied 状态、`result_ref` 或 `compensation_ref` 时标记 `connector_invalid_result`。
- [x] 新增 `agent-action-gateway` 的 `action-journal` WriteConnector / JSONL target，实际写入独立 target log，按 plan id 幂等返回稳定 `result_ref` / `compensation_ref`。
- [x] 新增 `POST /action-executions/compensate` action-journal compensation endpoint，并通过 smoke 验证补偿引用可执行。
- [ ] 第三方生产 WriteConnector / target adapter 尚未实现和验证。
- [ ] Manager 级 compensation API / 工作流尚未实现；当前只验证 adapter endpoint 能按 `compensation_ref` 执行补偿。

总结：当前满足通用 HTTP connector contract 和仓库内低风险 target smoke；不能宣告生产外部写入 adapter 已完成。

## 3. Apply Path

- [x] 新增 `POST /v1/admin/runs/{run_id}/external-action-plans/{plan_id}/apply`。
- [x] apply 只接受 `dry_run_ready` plan，并复用 approval、credential_scope、critical risk 和 resource lock 校验。
- [x] 执行前获取 `resource_locks`，执行后释放；锁冲突或 precheck 失败会推进为明确失败状态并写 audit。
- [x] apply audit 记录 `plan_id`、`run_id`、`approval_id`、`lock_id` 和结果状态，便于释放锁后仍可回溯本次外部动作持有过的锁。
- [x] connector 支持 timeout 和 bounded retry；耗尽后 plan 进入 `failed`，错误码为 `connector_dead_letter`，非终态 run 同步进入 dead-letter。

总结：apply path 已可被授权、审批、加锁、执行、审计和回溯；正式第三方外部写入仍依赖目标 provider / connector 配置。

## 4. CLI / Deploy

- [x] `agentctl runs dry-run-external-action` / `agentctl runs apply-external-action` 支持按 run/plan 执行 external action dry-run / apply。
- [x] `deploy/docker-compose.yml` 只从 `.env` 引用 external action provider / connector 配置。
- [x] `deploy/README.md` 记录 external action 环境变量，不输出或硬编码 secret。
- [x] `agent-action-gateway` binary 已加入镜像，`deploy/docker-compose.yml` 提供 `action-gateway-smoke` profile，默认不启动。
- [x] `agent-platform/scripts/action-gateway-smoke.sh` 可启动 Manager + Action Gateway，完成 approval、dry-run、apply、target write 和 compensation 验证。
- [ ] 目标部署环境尚未执行 `action-gateway-smoke` compose profile 或第三方 provider / connector smoke。

总结：P2 默认关闭；本地低风险 smoke 已覆盖仓库内 target，目标部署和第三方生产接入仍未验证。

## 5. Observer

- [x] Observer risk taxonomy 增加 applied / failed external-action 计数。
- [x] external-action failed count、approval bypass attempt、resource lock conflict、abnormal write result 和 audit failure decision 会进入健康评估。

总结：Observer 仍只读，只报告外部动作失败、锁压力和异常决策，不自动修复。

## 6. Validation

- [x] 本地 Rust 单测覆盖 HTTP provider、HTTP write connector、Action Gateway、external action apply active lease、resource lock release、lock conflict、invalid connector result、connector dead-letter、HTTP target 和 audit。
- [x] `action-journal` CredentialProvider smoke：签发 opaque provider ref，不返回 secret。
- [x] `action-journal` WriteConnector / 目标 target smoke：写入 JSONL target log，返回 `result_ref` 和 `compensation_ref`。
- [x] 端到端 external action smoke：配置 provider / connector 后执行一次低风险 approved external action，并核对 dry-run、plan 状态、credential lease、target write、connector result_ref 和 compensation。
- [ ] 第三方 provider / connector 切换规则只保留了配置入口；具体生产系统接入后必须重跑同一 smoke，当前不能视为完成。

总结：P2 平台骨架、仓库内 Action Gateway target 和本地端到端 smoke 已通过；P2 全量完成还需要生产 adapter、Manager compensation workflow 和目标环境验证。
