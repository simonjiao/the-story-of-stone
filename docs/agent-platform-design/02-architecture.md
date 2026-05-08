# 总体架构

## 主链路

```text
用户
  ↓
Open WebUI
  ↓
Agent Orchestrator / Gateway
  ├─ 普通聊天 → Default Hermes Agent Profile
  ├─ 创建 / 查询 / 管理意图 → Agent Manager
  └─ 已绑定 session 消息 → Agent Runtime

Agent Manager
  ├─ Policy / Approval / Lifecycle / Audit
  ├─ Resource Lock / Grant / Template
  └─ Read-only Snapshot API for Observer

Worker / Scheduler
  ↓ claim / heartbeat / finish
Agent Manager
  ↓ authorized run
Agent Runtime
  ↓
专用 Hermes Agent Profiles / Minimal Runtime

Memory / Session Store
  ↑ session / message / summary / result_ref
  └─ 被 Manager、Runtime、Worker 按授权访问

Observer Agent
  ↓ read-only snapshot API
Agent Manager
  ↓ aggregated status / audit / memory summaries
observer_report
```

## 管理员链路

```text
管理员
  ↓
agentctl CLI
  ↓
Agent Manager admin API
  ↓
审批 / 授权 / 暂停 / 恢复 / 审计 / Observer report 查询 / dead-letter run 管理
```

## 组件职责

| 组件 | 定位 | 负责内容 | 不负责内容 |
|---|---|---|---|
| Open WebUI | 前端聊天入口 | 用户交互、展示 Gateway 返回的流式响应和安全摘要 | 不注册 Manager Tool，不直接访问 Manager / Runtime / Worker / Observer |
| Agent Orchestrator / Gateway | 用户入口与路由层 | 身份绑定、意图路由、session binding、流式转发、限流、错误归一化 | 不执行任务，不审批，不持有目标 Agent credential，不保存长期上下文 |
| Agent Manager | 控制面 | 授权、策略、审批、生命周期、Agent 复用、资源锁、审计、Observer 只读快照 | 不替模型执行任务，不直接暴露给 Open WebUI |
| Agent Runtime | 执行面 | 承载 session/run，调用 Hermes profile、Minimal Runtime 和工具适配，返回结果 | 不决定授权边界，不绕过 Manager，不直接暴露给 Open WebUI |
| Memory / Session Store | 上下文存储 | 保存 session、message、summary、result_ref、上下文索引和 retention 状态 | 不保存明文密钥，不替代审计日志 |
| Worker / Scheduler | 后台执行层 | claim run、heartbeat、timeout、retry、dead-letter、定时触发 | 不处理前台路由，不绕过 Manager 策略 |
| Observer Agent | 系统观察执行体 | 读取只读摘要快照，生成健康、风险、异常和建议报告 | 不审批、不暂停、不恢复、不授权、不修改配置、不持有写权限 credential |
| Default Hermes Agent Profile | 普通聊天执行体 | 普通问答、意图澄清、非控制面任务 | 不作为固定中间层，不管理其他 Agent |
| 专用 Hermes Agent Profile | 专用执行体 | 专项分析、代码/数据/系统任务执行 | 不决定权限边界，不越权访问资源 |
| agentctl CLI | 管理员入口 | 审批、查询、暂停、恢复、审计、Observer report 查询、dead-letter run inspect / retry / terminate | 不面向普通用户 |
| 外部系统连接器 | 外部接口 | 提供最小权限 API 访问 | 不保存平台状态，不绕过 Manager |

## 网络拓扑

Agent Manager、Agent Runtime、Memory / Session Store、Worker / Scheduler、Observer Agent 只部署在内网。公网入口只到 Open WebUI；Open WebUI 只连接 Agent Orchestrator / Gateway。

```yaml
services:
  open-webui:
    image: ghcr.io/open-webui/open-webui:main
    networks:
      - frontend
      - gateway-net

  agent-orchestrator:
    build: ./agent-platform/orchestrator
    expose:
      - "8080"
    networks:
      - gateway-net
      - internal-agent-net
    environment:
      - AGENT_MANAGER_URL=http://agent-manager:8088
      - DEFAULT_AGENT_URL=http://hermes-default:8642

  hermes-default:
    image: hermes-agent-runtime:latest
    networks:
      - internal-agent-net

  agent-manager:
    build: ./agent-platform/agent-manager
    expose:
      - "8088"
    networks:
      - internal-agent-net

  agent-runtime:
    build: ./agent-platform/agent-runtime
    networks:
      - internal-agent-net

  agent-worker:
    build: ./agent-platform/agent-worker
    command: agent-worker
    networks:
      - internal-agent-net

  agent-observer:
    build: ./agent-platform/agent-worker
    command: agent-observer
    networks:
      - internal-agent-net

  postgres:
    image: postgres:16
    networks:
      - internal-agent-net

  redis:
    image: redis:7
    networks:
      - internal-agent-net

networks:
  frontend:
  gateway-net:
  internal-agent-net:
    internal: true
```

网络规则：

```text
1. Open WebUI 只访问 agent-orchestrator。
2. agent-manager、agent-runtime、agent-worker、agent-observer 不配置 ports。
3. agent-orchestrator 可以访问 agent-manager 和 default profile。
4. agent-runtime、agent-worker、agent-observer、agentctl 可以访问 agent-manager；P0 worker/observer 也可以通过受控 `RunQueue` / `ObserverSnapshotStore` 直接推进已授权 run 和只读 snapshot。
5. agent-observer 只能访问只读 snapshot / report API。
6. 外部 connector 的写入 API 只能由已授权 side-effect run 经 Manager 策略后使用。
```

## 调用方向

允许的调用：

```text
Open WebUI → Orchestrator
Orchestrator → Default Hermes Agent Profile
Orchestrator → Manager user API
Orchestrator → Runtime session API（仅限 Manager 已授权 session）
agentctl → Manager admin API
Worker → RunQueue / Manager internal run API（只处理已授权 run）
Worker → Runtime run API（仅限 claimed run）
Runtime → Memory / Session Store（仅限授权 session/run）
Observer → ObserverSnapshotStore / Manager read-only snapshot API
Manager → Storage / Audit
```

禁止的调用：

```text
Open WebUI → Manager / Runtime / Worker / Observer
Orchestrator → Manager admin API
Orchestrator → Observer admin report API
Runtime → Manager 授权绕过接口
Worker → 外部 connector 写入接口（未持有授权和锁时）
Observer → 任何控制或写入 API
```
