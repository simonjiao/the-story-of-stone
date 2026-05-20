# 23 hhost 通灵玉重建 Checklist

## 目标

在 `hhost` 上重建一个以通灵玉 Agent 为中心的新系统，避免继续继承多次开发和部署过程中形成的补丁层。

目标生产链路：

```text
Cloudflare Tunnel
  -> Open WebUI
      -> tonglingyu-gateway
          -> tonglingyu-runtime / SQLite KB / evidence packages / RQA
          -> Hermes Agent upstream
```

## 命名和拼写

- [x] 统一使用 `tonglingyu`；发现错写、多写或漏写字母的形式时必须修正。
- [x] 用户可见中文名统一为“通灵玉”。
- [x] 通灵玉本身按 Agent 处理，不只是一个 Gateway、模型 id 或知识库。
- [x] 用于通灵玉的 Hermes Agent 容器名必须以 `tonglingyu-` 开头；目标名为 `tonglingyu-hermes-agent`。
- [x] Open WebUI 和 Cloudflared 是位于 Tonglingyu Agent 前面的入口层，不按
  Tonglingyu Agent 容器命名，不强制加 `tonglingyu-` 前缀。
- [x] Docker Compose 服务名可以继续用 `hermes`，以保持内部 DNS 和 Gateway 配置稳定。
- [x] Open WebUI Function ID `agent_identity_bridge` 保持不变；不改名为 Tonglingyu 专用 ID。
- [x] `agent_identity_bridge` 的目标模型和语义可以调整，但历史 Function ID、安装脚本入口和验证脚本入口必须保持兼容。

## hhost 重建前现状快照

本节来自 2026-05-17 重建前对 `hhost` 的只读检查；未读取或输出
`.env` secret 值。

- [x] 主机名：`DESKTOP-1C5QUGQ`
- [x] 重建前部署目录仍是旧目录：`$HOME/hermes-home-deploy`
- [x] 重建前运行时目录仍是：`$HOME/huixiangdou-home-runtime`
- [x] 重建前存在备份目录：`$HOME/hermes-home-deploy-backups`
- [x] 重建前 compose 文件：
  - `$HOME/hermes-home-deploy/docker-compose.yml`
  - `$HOME/hermes-home-deploy-backups/20260509-215438/docker-compose.yml`
  - `$HOME/sub2api/docker-compose.yml`
- [x] 重建前仍在运行旧 Agent Platform 容器：`agent-manager`、
  `agent-orchestrator`、`agent-worker`、`agent-observer`、
  `agent-platform-postgres`
- [x] 重建前 Tonglingyu Gateway 容器为 `tonglingyu-gateway`
- [x] 重建前 Hermes Agent 容器仍为 `hermes-agent`，不满足新命名要求，
  重建时必须改为 `tonglingyu-hermes-agent`
- [x] 重建前 Open WebUI 容器为 `hermes-open-webui`
- [x] 重建前 Cloudflare 容器为 `hermes-cloudflared`
- [x] 重建前还存在独立 `sub2api` stack：`sub2api`、`sub2api-postgres`、
  `sub2api-redis`

## 新目录规划

- [x] 新部署目录使用 `$HOME/tonglingyu-home-deploy`
- [x] 前置层运行时目录使用 `$HOME/huixiangdou-home-runtime`
- [x] 通灵玉运行时目录使用 `$HOME/tonglingyu-home-runtime`
- [x] 旧目录 `$HOME/hermes-home-deploy` 只作为回滚参考，不作为新系统继续打补丁
- [x] `$HOME/huixiangdou-home-runtime` 继续承载 Open WebUI 前置层状态；其中旧
  Tonglingyu 数据只作为一次性迁移来源或回滚参考
- [x] 新部署目录只保存 compose、scripts、Open WebUI Functions、Rust build
  context 和 source snapshots
- [x] 通灵玉运行时目录保存 Hermes、Tonglingyu SQLite、证据、报告和备份

建议目录：

```text
$HOME/tonglingyu-home-deploy/
  docker-compose.yml
  .env
  open-webui/functions/
  scripts/
  agent-platform/
  resources/sources/wiki/

$HOME/huixiangdou-home-runtime/
  data/open-webui/

$HOME/tonglingyu-home-runtime/
  data/tonglingyu/
  data/hermes/
  evidence/
  reports/
  backups/
```

## 保留范围

- [x] 保留 `tonglingyu-gateway`
- [x] 保留 `tonglingyu-runtime`
- [x] 保留支撑 Tonglingyu Runtime 的 `agent-core` 和 `agent-runtime` 代码依赖
- [x] 保留 Hermes Agent 作为通灵玉的上游 Agent 容器
- [x] 保留 Open WebUI 作为用户入口
- [x] 保留 Cloudflare Tunnel 到 Open WebUI
- [x] 保留 `agent_identity_bridge` Function ID
- [x] 保留 `tonglingyu_gateway_admin` Action
- [x] 保留 `tonglingyu_gateway_feedback` Action
- [x] 保留 `resources/sources/wiki` source snapshots
- [x] 保留 `resources/styles/buhongjushi` 风格资料边界

## 删除或退出生产路径

- [x] `agent-manager` 不进入新生产路径
- [x] `agent-orchestrator` 不进入新生产路径
- [x] `agent-worker` 不进入新生产路径
- [x] `agent-observer` 不进入新生产路径
- [x] `agent-platform-postgres` 不进入新生产路径
- [x] `agent-action-gateway` 不进入新生产路径
- [x] Open WebUI 不再配置 `http://agent-orchestrator:8080/v1`

## 数据重建策略

- [ ] `resources/sources/wiki` 是 Tonglingyu KB 的可重建事实输入
- [ ] `data/tonglingyu/tonglingyu.db` 是可删除再建产物
- [ ] 新系统默认删除旧 Tonglingyu SQLite，重新从 source snapshots build KB
- [ ] SQLite schema 必须有版本检查和重建检查，不靠临时 SQL 补丁判断成功
- [ ] Open WebUI `webui.db` 只保存 UI 账号、配置和 Function 状态，不作为 Tonglingyu KB 来源
- [ ] 如果无法安全迁移 Open WebUI 账号和设置，允许备份后删除旧 Open WebUI 数据并重新初始化
- [ ] Hermes 配置从 `.env` 渲染，不复制旧容器内手工补丁

## 重构补丁层

- [x] 删除或重写仍引用 `agent-orchestrator` 的 deploy 脚本和 release gate
- [x] 删除或重写仍引用 `agent-worker`、`agent-manager`、`AGENT_PLATFORM_*` 的生产部署脚本
- [x] release readiness 只检查 Tonglingyu 链路：Open WebUI、Gateway、Hermes、
  Cloudflare、source snapshot、RQA
- [x] Open WebUI provider 只配置 `http://tonglingyu-gateway:8090/v1`
- [x] `agent_identity_bridge` 检查保留 Function ID，但验证目标改为 Tonglingyu
  所需的 user/chat/message/session 元数据
- [x] admin action 和 feedback action 的 gate 继续作为必过项
- [x] 所有脚本输出只打印变量名、路径、状态和摘要，不打印 token、API key、密码

## 新系统启动顺序

- [x] 备份旧 `$HOME/hermes-home-deploy` 和旧 `$HOME/huixiangdou-home-runtime`
  中的 Tonglingyu 相关数据
- [x] 停止旧 `hermes-home` compose stack
- [x] 创建 `$HOME/tonglingyu-home-deploy`
- [x] 确认 `$HOME/huixiangdou-home-runtime` 用作前置层运行时目录
- [x] 创建 `$HOME/tonglingyu-home-runtime`
- [x] 同步新的 compose、scripts、Functions、source snapshots 和 Rust build context
- [x] 生成新的 `.env`，只写 Tonglingyu 所需配置
- [x] `docker compose config` 通过
- [x] 构建 `tonglingyu-gateway`
- [x] 启动 `hermes`
- [x] 启动 `tonglingyu-gateway`
- [x] 启动 `open-webui`
- [x] 安装或更新 `agent_identity_bridge`
- [x] 安装或更新 `tonglingyu_gateway_admin`
- [x] 安装或更新 `tonglingyu_gateway_feedback`
- [x] 启动 `cloudflared`

## 验收闸门

- [x] 容器名检查：Tonglingyu Hermes Agent 容器名为 `tonglingyu-hermes-agent`
- [x] 前置层容器名检查：Open WebUI 为 `home-open-webui`，Cloudflared 为
  `home-cloudflared`，不使用 `tonglingyu-` 前缀
- [x] 拼写检查：仓库和部署产物中不得出现把 `tonglingyu` 写错的形式
- [x] `/healthz` 返回 KB 非空，source/block 数符合当前 source snapshot registry
- [x] `/v1/models` 只暴露 `tonglingyu`
- [x] Open WebUI 默认模型为 `tonglingyu`
- [x] Open WebUI provider 只指向 `http://tonglingyu-gateway:8090/v1`
- [x] 非 streaming `/v1/chat/completions` 可用
- [x] streaming `/v1/chat/completions` 可用，包含 `[DONE]`
- [x] admin action 可查 metrics、trace、package 和 session
- [x] feedback action 安装完成并通过源码/契约 gate；真实用户反馈写入需浏览器
  或 UI action 侧复核
- [x] 普通用户不能调用 admin action
- [x] `agent_identity_bridge` Function ID 未改变
- [x] release readiness summary report 已记录本次 hhost runtime config、
  Open WebUI Function、admin action、model upstream 和 strict Gateway 证据

## 本轮仓库完成项

- [x] `deploy/docker-compose.yml` 已收敛为 Tonglingyu-only stack：
  `hermes`、`tonglingyu-gateway`、`open-webui`、`cloudflared`
- [x] Tonglingyu 后端固定容器名使用明确的 Agent/网关命名：
  `tonglingyu-hermes-agent`、`tonglingyu-gateway`
- [x] 前置层固定容器名不使用 `tonglingyu-` 前缀：
  `home-open-webui`、`home-cloudflared`
- [x] Compose project name 已改为 `tonglingyu-home`
- [x] 内部 Docker 网络目标已改为 `tonglingyu-internal`
- [x] Open WebUI 默认只连接 `http://tonglingyu-gateway:8090/v1`
- [x] `agent_identity_bridge` Function ID 保持不变，默认目标模型改为
  `tonglingyu`
- [x] `ensure-tonglingyu-gateway-env.sh` 会把 Open WebUI provider 收敛为单
  Gateway 入口，并保持密钥只在 `.env` 中
- [x] `verify-tonglingyu-runtime-config.sh` 会拒绝旧控制面服务、
  非 `tonglingyu-` 容器名、旧多 provider 入口和
  `tonglignyu` 拼写错误

## hhost 执行结果

- [x] 旧 stack 已停止；当前只保留 `sub2api` 外部依赖 stack 和新的
  `tonglingyu-home` stack
- [x] 新运行容器：`tonglingyu-hermes-agent`、`tonglingyu-gateway`、
  `home-open-webui`、`home-cloudflared`
- [x] 新内部网络：`tonglingyu-internal`
- [x] 前置层 runtime dir：Open WebUI 使用
  `$HOME/huixiangdou-home-runtime/data/open-webui`；Cloudflared 无本地 runtime
  data dir
- [x] Tonglingyu runtime dir：Hermes 和 Tonglingyu Gateway/KB/RQA 使用
  `$HOME/tonglingyu-home-runtime`
- [x] 远端备份目录：
  `$HOME/tonglingyu-home-rebuild-backups/20260517T064057Z`
- [x] `verify-tonglingyu-runtime-config.sh` 通过，config mode 为
  `docker_compose_config`
- [x] `verify-openwebui-function.sh` 通过，`agent_identity_bridge` 来源为
  `compose-db`
- [x] `verify-openwebui-gateway-admin-action.sh` 通过，`tonglingyu_gateway_admin`
  来源为 `compose-db`
- [x] `verify-model-upstream-network.sh` 通过，probe container 为 `sub2api`
- [x] `verify-tonglingyu-strict-gateway.sh` 独立复核通过；最终 trace 为
  `tly-019e34b4aef3728090f56eec23f49376`，stream trace 为
  `tly-019e34b4d7fe79f092483acea4b25bbf`
- [x] Open WebUI `webui.db` 检查未发现 `agent-orchestrator` provider 残留
- [x] 公开入口 `https://chat.huixiangdou.top/` 返回 HTTP 200

## 尚未达到 production-ready 证据

- [ ] aggregate release readiness 当前是 summary-only，`production_release_ready=false`
- [ ] 需要补齐 browser-side review evidence
- [ ] 需要补齐 production security scan 和 digest-pinned `tonglingyu-gateway`
  image ref
- [ ] 需要补齐 RQA migration/restore/performance/API/user-lifecycle live evidence；
  当前远端 deploy node 没有 `cargo`，这些 gate 不能在该目录直接构建运行
- [ ] 需要补齐 release ops、incident/capacity、post-release monitor 证据
