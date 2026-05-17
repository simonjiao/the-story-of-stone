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

- [ ] 统一使用 `tonglingyu`；发现错写、多写或漏写字母的形式时必须修正。
- [ ] 用户可见中文名统一为“通灵玉”。
- [ ] 通灵玉本身按 Agent 处理，不只是一个 Gateway、模型 id 或知识库。
- [ ] 用于通灵玉的 Hermes Agent 容器名必须以 `tonglingyu-` 开头；目标名为 `tonglingyu-hermes-agent`。
- [ ] Docker Compose 服务名可以继续用 `hermes`，以保持内部 DNS 和 Gateway 配置稳定。
- [ ] Open WebUI Function ID `agent_identity_bridge` 保持不变；不改名为 Tonglingyu 专用 ID。
- [ ] `agent_identity_bridge` 的目标模型和语义可以调整，但历史 Function ID、安装脚本入口和验证脚本入口必须保持兼容。

## hhost 当前现状快照

本节来自 2026-05-17 对 `hhost` 的只读检查；未读取或输出 `.env` secret 值。

- [x] 主机名：`DESKTOP-1C5QUGQ`
- [x] 当前部署目录仍是旧目录：`$HOME/hermes-home-deploy`
- [x] 当前运行时目录仍是：`$HOME/huixiangdou-home-runtime`
- [x] 当前存在备份目录：`$HOME/hermes-home-deploy-backups`
- [x] 当前 compose 文件：
  - `$HOME/hermes-home-deploy/docker-compose.yml`
  - `$HOME/hermes-home-deploy-backups/20260509-215438/docker-compose.yml`
  - `$HOME/sub2api/docker-compose.yml`
- [x] 当前仍在运行旧 Agent Platform 容器：`agent-manager`、`agent-orchestrator`、`agent-worker`、`agent-observer`、`agent-platform-postgres`
- [x] 当前 Tonglingyu Gateway 容器为 `tonglingyu-gateway`
- [ ] 当前 Hermes Agent 容器仍为 `hermes-agent`，不满足新命名要求，重建时必须改为 `tonglingyu-hermes-agent`
- [x] 当前 Open WebUI 容器为 `hermes-open-webui`
- [x] 当前 Cloudflare 容器为 `hermes-cloudflared`
- [x] 当前还存在独立 `sub2api` stack：`sub2api`、`sub2api-postgres`、`sub2api-redis`

## 新目录规划

- [ ] 新部署目录使用 `$HOME/tonglingyu-home-deploy`
- [ ] 新运行时目录使用 `$HOME/tonglingyu-home-runtime`
- [ ] 旧目录 `$HOME/hermes-home-deploy` 只作为回滚参考，不作为新系统继续打补丁
- [ ] 旧运行时目录 `$HOME/huixiangdou-home-runtime` 只作为回滚参考或一次性迁移来源
- [ ] 新部署目录只保存 compose、scripts、Open WebUI Functions、Rust build
  context 和 source snapshots
- [ ] 新运行时目录保存 Open WebUI、Hermes、Tonglingyu SQLite、证据、报告和备份

建议目录：

```text
$HOME/tonglingyu-home-deploy/
  docker-compose.yml
  .env
  open-webui/functions/
  scripts/
  agent-platform/
  resources/sources/wiki/

$HOME/tonglingyu-home-runtime/
  data/open-webui/
  data/tonglingyu/
  data/hermes/
  evidence/
  reports/
  backups/
```

## 保留范围

- [ ] 保留 `tonglingyu-gateway`
- [ ] 保留 `tonglingyu-runtime`
- [ ] 保留支撑 Tonglingyu Runtime 的 `agent-core` 和 `agent-runtime` 代码依赖
- [ ] 保留 Hermes Agent 作为通灵玉的上游 Agent 容器
- [ ] 保留 Open WebUI 作为用户入口
- [ ] 保留 Cloudflare Tunnel 到 Open WebUI
- [ ] 保留 `agent_identity_bridge` Function ID
- [ ] 保留 `tonglingyu_gateway_admin` Action
- [ ] 保留 `tonglingyu_gateway_feedback` Action
- [ ] 保留 `resources/sources/wiki` source snapshots
- [ ] 保留 `resources/styles/buhongjushi` 风格资料边界

## 删除或退出生产路径

- [ ] Global Router 不进入新生产路径
- [ ] `agent-manager` 不进入新生产路径
- [ ] `agent-orchestrator` 不进入新生产路径
- [ ] `agent-worker` 不进入新生产路径
- [ ] `agent-observer` 不进入新生产路径
- [ ] `agent-platform-postgres` 不进入新生产路径
- [ ] `agent-action-gateway` 不进入新生产路径
- [ ] Open WebUI 不再配置 `http://agent-orchestrator:8080/v1`

## 数据重建策略

- [ ] `resources/sources/wiki` 是 Tonglingyu KB 的可重建事实输入
- [ ] `data/tonglingyu/tonglingyu.db` 是可删除再建产物
- [ ] 新系统默认删除旧 Tonglingyu SQLite，重新从 source snapshots build KB
- [ ] SQLite schema 必须有版本检查和重建检查，不靠临时 SQL 补丁判断成功
- [ ] Open WebUI `webui.db` 只保存 UI 账号、配置和 Function 状态，不作为 Tonglingyu KB 来源
- [ ] 如果无法安全迁移 Open WebUI 账号和设置，允许备份后删除旧 Open WebUI 数据并重新初始化
- [ ] Hermes 配置从 `.env` 渲染，不复制旧容器内手工补丁

## 重构补丁层

- [ ] 删除或重写仍引用 `agent-orchestrator` 的 deploy 脚本和 release gate
- [ ] 删除或重写仍引用 `agent-worker`、`agent-manager`、`AGENT_PLATFORM_*` 的生产部署脚本
- [ ] release readiness 只检查 Tonglingyu 链路：Open WebUI、Gateway、Hermes、
  Cloudflare、source snapshot、RQA
- [ ] Open WebUI provider 只配置 `http://tonglingyu-gateway:8090/v1`
- [ ] `agent_identity_bridge` 检查保留 Function ID，但验证目标改为 Tonglingyu
  所需的 user/chat/message/session 元数据
- [ ] admin action 和 feedback action 的 gate 继续作为必过项
- [ ] 所有脚本输出只打印变量名、路径、状态和摘要，不打印 token、API key、密码

## 新系统启动顺序

- [ ] 备份旧 `$HOME/hermes-home-deploy` 和 `$HOME/huixiangdou-home-runtime`
- [ ] 停止旧 `hermes-home` compose stack
- [ ] 创建 `$HOME/tonglingyu-home-deploy`
- [ ] 创建 `$HOME/tonglingyu-home-runtime`
- [ ] 同步新的 compose、scripts、Functions、source snapshots 和 Rust build context
- [ ] 生成新的 `.env`，只写 Tonglingyu 所需配置
- [ ] `docker compose config` 通过
- [ ] 构建 `tonglingyu-gateway`
- [ ] 启动 `hermes`
- [ ] 启动 `tonglingyu-gateway`
- [ ] 启动 `open-webui`
- [ ] 安装或更新 `agent_identity_bridge`
- [ ] 安装或更新 `tonglingyu_gateway_admin`
- [ ] 安装或更新 `tonglingyu_gateway_feedback`
- [ ] 启动 `cloudflared`

## 验收闸门

- [ ] 容器名检查：Tonglingyu Hermes Agent 容器名为 `tonglingyu-hermes-agent`
- [ ] 拼写检查：仓库和部署产物中不得出现把 `tonglingyu` 写错的形式
- [ ] `/healthz` 返回 KB 非空，source/block 数符合当前 source snapshot registry
- [ ] `/v1/models` 只暴露 `tonglingyu`
- [ ] Open WebUI 默认模型为 `tonglingyu`
- [ ] Open WebUI provider 只指向 `http://tonglingyu-gateway:8090/v1`
- [ ] 非 streaming `/v1/chat/completions` 可用
- [ ] streaming `/v1/chat/completions` 可用，包含 `[DONE]`
- [ ] admin action 可查 metrics、trace、package 和 session
- [ ] feedback action 能写入 RQA 队列
- [ ] 普通用户不能调用 admin action
- [ ] `agent_identity_bridge` Function ID 未改变
- [ ] release readiness report 记录本次 hhost 目录、容器名、镜像、trace、package 和 session 证据
