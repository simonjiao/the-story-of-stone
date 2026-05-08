# ISSUE-004: Open WebUI 固定 origin IP 被 Agent Platform 服务占用

## 状态

RESOLVED

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `DEPLOY-03` | PASS | 正式服务最终全部启动并保持健康。 |
| `NET-01` | PASS | Open WebUI 固定在 `172.20.0.3/16`，Agent Platform 服务使用独立固定地址。 |
| `PUBLIC-01` | PASS | 公网 `/api/config` 返回 HTTP 200。 |

## 影响

Open WebUI 需要固定内网 origin IP 供 `cloudflared` 的 host-network 模式通过 `extra_hosts` 访问。正式 compose 首次启动时，Open WebUI 重建期间 `172.20.0.3` 被 `agent-manager` 动态分配，导致 Open WebUI 容器无法启动。

## 证据

- 首次正式 `docker compose up` 失败：`failed to set up container networking: Address already in use`。
- 故障时 `docker network inspect hermes-internal` 显示 `agent-manager 172.20.0.3/16`。
- `hermes-open-webui` 处于 `Created` 状态，无法启动。

## 修复记录

| 时间 | 操作 | 结果 |
| --- | --- | --- |
| 2026-05-08 16:15 CST | 保留现场并检查 compose、容器状态和 `hermes-internal` 网络。 | 确认是固定 origin IP 与动态 IP 分配冲突。 |
| 2026-05-08 16:18 CST | 更新 `deploy/docker-compose.yml`，为内部服务分配稳定 IP。 | `hermes-agent` 使用 `.2`，`open-webui` 使用 `.3`，Agent Platform Postgres 使用 `.4`，Manager/Orchestrator/Worker/Observer 使用 `.10`-`.13`。 |
| 2026-05-08 16:20 CST | 同步正式 compose 到远程并重建相关服务。 | Open WebUI 成功启动并变为 healthy。 |

## 验证

- `docker compose ps` 显示 `hermes-open-webui` 为 healthy。
- `docker network inspect hermes-internal` 显示：
  - `hermes-open-webui 172.20.0.3/16`
  - `agent-manager 172.20.0.10/16`
  - `agent-orchestrator 172.20.0.11/16`
  - `agent-worker 172.20.0.12/16`
  - `agent-observer 172.20.0.13/16`
- `https://chat.huixiangdou.top/api/config` 返回 HTTP 200。
- 临时 `codex-p0-*` 测试容器已删除，网络中不再存在临时测试节点。

## 后续动作

已解决。后续新增内部服务时，应继续使用显式固定 IP 或避开 `OPEN_WEBUI_ORIGIN_IP`，避免 Cloudflare Tunnel origin 地址被动态分配占用。
