# Open WebUI Agent Identity Bridge Complete Plan

## 目标

为 `chat.huixiangdou.top` 的正式 Open WebUI 部署一次性补齐 Agent Identity Bridge 的完整能力：

1. Open WebUI 登录用户身份可信进入 Agent Platform。
2. Open WebUI conversation 持久绑定到 Agent Platform `agent_session`。
3. 同一用户、同一 Open WebUI chat 后续消息自动复用对应 session。
4. 同一 reusable agent 支持不同 Open WebUI chat 的多 session 隔离。
5. session 消息自动创建 run，由现有 Worker 队列执行。
6. 普通默认聊天仍走默认 Hermes Agent，不被误切到 Agent Platform 控制面。
7. 正式部署不依赖临时 Open WebUI，不 fork 或 patch Open WebUI core。
8. Manager 审计不再出现 Open WebUI 控制请求落到默认 `dev-user` 的情况。

本文档是独立修复方案，不属于 Agent Platform 阶段规划，也不使用阶段名作为协议、字段、函数、Docker tag 或部署命名。

## 当前问题

Open WebUI 通过 OpenAI-compatible provider 调用 Orchestrator：

```text
Open WebUI -> agent-orchestrator -> Hermes / Agent Manager
```

Open WebUI provider 的静态配置只能设置统一 API base URL 和 API key，不能按当前登录用户动态设置 Agent Platform 用户身份、roles、resource allowlist 或当前 chat id。因此当前 UI 触发 Agent Platform 控制请求时存在三个缺口：

1. 身份缺口：Manager 只能使用默认 dev 身份或静态 header。
2. session 缺口：Open WebUI conversation 不能稳定映射到 Agent Platform session。
3. 运行缺口：已绑定 session 的后续用户消息没有自动形成 run/result 闭环。

这不是 Worker 问题，也不是 Open WebUI 普通聊天问题；这是 Open WebUI 到 Agent Platform 控制面的正式身份和 session 桥接缺口。

## 正式方案原则

1. 不修改 Open WebUI core，不 fork Open WebUI。
2. 使用 Open WebUI 官方 Function / Filter 在请求进入模型前注入签名上下文。
3. Open WebUI 只连接 Orchestrator；Manager、Worker、Observer 不对 Open WebUI 或公网暴露。
4. Orchestrator 对 Agent Platform 控制请求 fail closed：没有有效 Bridge context 就拒绝。
5. Orchestrator 转发普通聊天到 Hermes 前必须删除内部 Bridge 字段。
6. Manager 正式鉴权使用 JWT；部署后关闭 dev headers。
7. Open WebUI conversation 到 Agent session 的绑定必须持久化到 Postgres，不能依赖 Orchestrator 内存。
8. Agent instance 可复用，但 session、run、message、context 必须按 Open WebUI user/chat 隔离。
9. 所有 secret 只进入 `.env` 或 Open WebUI Function Valves，不写入代码、compose、文档示例值或日志。

管理账号边界：

1. Open WebUI admin 只用于安装/更新 Function、设置 Valves，属于 Open WebUI 配置管理权限。
2. Agent Platform 的审批、审计和内部管理使用 Agent Platform 自己的 JWT、role 和 resource allowlist。
3. Open WebUI admin 默认不映射为 Agent Platform admin；如需映射必须显式配置 `AGENT_BRIDGE_ADMIN_ROLE_MAPPING=agent_admin`。

## 命名

使用中性命名：

```text
agent_identity_bridge
agent_bridge_context
open_webui_bridge_bindings
AGENT_BRIDGE_SECRET
AGENT_BRIDGE_ISSUER
```

禁止使用阶段名作为：

1. Function id。
2. JSON 字段。
3. header。
4. env var。
5. Docker tag。
6. 用户可见说明。

## 完整数据流

```text
Open WebUI Filter
  -> inject signed agent_bridge_context
  -> OpenAI-compatible request to agent-orchestrator
  -> Orchestrator verifies HMAC and timestamp
  -> Orchestrator signs Manager service/user JWT
  -> Manager receives real Open WebUI user subject
  -> control request creates/reuses agent
  -> Manager creates/reuses agent_session for Open WebUI chat
  -> Manager stores open_webui_bridge_bindings in Postgres
  -> later messages in same chat resolve binding
  -> Orchestrator appends session message and creates run
  -> existing Worker claims run by lease and writes result
  -> Orchestrator returns queued/completed summary to Open WebUI
```

普通默认聊天流：

```text
Open WebUI Filter
  -> inject signed agent_bridge_context
  -> Orchestrator verifies or ignores for non-control request
  -> Orchestrator strips agent_bridge_context
  -> Hermes default agent
```

默认聊天不会因为启用 Bridge 而自动进入 Agent Platform。只有两类请求会被 Orchestrator 拦截：

1. 明确的 Agent Platform 控制请求。
2. 已存在 active Open WebUI chat -> agent_session 绑定的后续消息。

## Open WebUI Filter

新增版本化 Function 文件：

```text
deploy/open-webui/functions/agent_identity_bridge_filter.py
```

Filter 在 `inlet()` 中读取：

| 来源 | 字段 | 用途 |
| --- | --- | --- |
| `__user__` | `id` | 必填，生成 `openwebui:<id>` |
| `__user__` | `email` | 可选，只用于 hash 或诊断，不发给 Hermes |
| `__user__` | `role` | 可选，映射 Agent Platform role |
| `__metadata__` | `chat_id` | 必填，作为绑定 key |
| `__metadata__` | `session_id` | 可选，用于浏览器会话诊断 |
| `__metadata__` | `message_id` 或 `user_message_id` | 可选，用于 run 幂等；message append 去重需要后续 schema 扩展 |
| `body` | `model` | 仅对 `hermes-agent` 生效 |

注入字段：

```json
{
  "agent_bridge_context": {
    "version": 1,
    "issuer": "open-webui",
    "subject": "openwebui:<user-id>",
    "user_role": "user",
    "chat_id": "<open-webui-chat-id>",
    "session_id": "<open-webui-session-id>",
    "message_id": "<open-webui-message-id>",
    "model": "hermes-agent",
    "issued_at": 1778220000,
    "nonce": "<random>",
    "signature": "<hmac-sha256>"
  }
}
```

签名输入使用规范化 JSON，只包含可验证字段，不包含 `signature`。

Filter 失败策略：

1. 缺少 secret：不注入 context，并让 Orchestrator 对控制请求 fail closed。
2. 缺少 user 或 chat id：不注入 context，并让 Orchestrator 对控制请求 fail closed。
3. 普通聊天即使未注入 context，也允许继续 passthrough。

## Orchestrator 变更

### 配置

新增环境变量：

```text
AGENT_BRIDGE_SECRET
AGENT_BRIDGE_ISSUER=open-webui
AGENT_BRIDGE_MAX_CLOCK_SKEW_SECONDS=300
AGENT_BRIDGE_RESOURCE_ALLOWLIST=resource:team/default
AGENT_BRIDGE_USER_ROLE=viewer
AGENT_BRIDGE_ADMIN_ROLE_MAPPING=disabled
AGENT_JWT_SECRET
AGENT_MANAGER_SERVICE_ID=agent-orchestrator
AGENT_MANAGER_JWT_TTL_SECONDS=300
AGENT_BRIDGE_RUN_WAIT_TIMEOUT_SECONDS=20
AGENT_BRIDGE_RUN_POLL_INTERVAL_MS=500
```

`AGENT_BRIDGE_SECRET` 用于验证 Open WebUI Filter 注入的 context。`AGENT_JWT_SECRET` 用于 Orchestrator 向 Manager 签发 service JWT 和 user JWT。二者必须分离。

### 验证规则

Orchestrator 必须验证：

1. `issuer` 匹配配置。
2. `subject` 以 `openwebui:` 开头。
3. `chat_id` 非空。
4. `model` 为 `hermes-agent`。
5. `issued_at` 在允许时间窗口内。
6. `nonce` 非空。
7. HMAC 签名正确。

控制请求和已绑定 session 消息必须有有效 context；普通聊天可以 passthrough，但必须先删除 `agent_bridge_context`。

### Manager 鉴权

完整交付不再依赖 Manager dev headers。

Orchestrator 对 Manager 请求生成：

```text
Authorization: Bearer <service-jwt>
x-agent-user-token: <user-jwt>
x-agent-trace-id: <trace-id>
```

service JWT claims：

```json
{
  "sub": "agent-orchestrator",
  "allowed_actions": ["request:*", "session:*", "run:*", "internal:*"],
  "exp": 1778220300
}
```

user JWT claims：

```json
{
  "sub": "openwebui:<user-id>",
  "roles": ["viewer"],
  "resource_allowlist": ["resource:team/default"],
  "exp": 1778220300
}
```

角色映射默认保守：

1. Open WebUI 普通用户映射到可配置的 `AGENT_BRIDGE_USER_ROLE`，默认 `viewer`。
2. Open WebUI admin 不默认映射为 Agent Platform admin。
3. 如需 admin 映射，必须显式设置 `AGENT_BRIDGE_ADMIN_ROLE_MAPPING`。

正式 deploy 中设置：

```text
AGENT_PLATFORM_ALLOW_DEV_HEADERS=false
```

### 路由决策

Orchestrator 对 `/v1/chat/completions` 做四类处理：

| 场景 | 处理 |
| --- | --- |
| 普通未绑定聊天 | 删除 `agent_bridge_context` 后转发 Hermes |
| 明确 Agent 控制请求 | 验证 context，提交 Agent request |
| 已绑定 chat 后续消息 | 验证 context，append session message，create run |
| 解绑/关闭 session 指令 | 验证 context，关闭 session 并清除 binding |

控制请求识别仍只扫描当前直接用户文本，不能扫描 Open WebUI 内部 follow-up prompt 的 `### Chat History` 或 `<chat_history>`。

## Manager / Store 变更

### Bridge binding 数据模型

新增 domain/API struct：

```text
AgentBridgeBinding
```

新增 Postgres migration：

```text
agent-platform/crates/agent-store/migrations/0004_open_webui_bridge_bindings.sql
```

建议表：

```sql
CREATE TABLE IF NOT EXISTS open_webui_bridge_bindings (
    id TEXT PRIMARY KEY,
    open_webui_subject TEXT NOT NULL,
    open_webui_chat_id TEXT NOT NULL,
    open_webui_session_id TEXT,
    model TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    agent_session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    last_message_id TEXT,
    last_run_id TEXT,
    trace_id TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    closed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_open_webui_bridge_active_chat
ON open_webui_bridge_bindings(open_webui_subject, open_webui_chat_id, model)
WHERE status = 'active';
```

绑定粒度是 `Open WebUI user + chat_id + model`。这样同一个用户的新聊天会创建新 session，同一个聊天会复用 session，不同用户不会互相看到 binding。

### Store trait

在 `AgentStore` 增加：

```text
get_open_webui_bridge_binding(subject, chat_id, model)
upsert_open_webui_bridge_binding(binding)
close_open_webui_bridge_binding(subject, chat_id, model, trace_id)
update_open_webui_bridge_run(binding_id, message_id, run_id, trace_id)
```

Memory store 和 Postgres store 都必须实现，保证单元测试和本地无数据库运行一致。

### Manager endpoints

新增内部路由，只供 Orchestrator 调用：

```text
GET  /v1/internal/open-webui-bridge/bindings/{chat_id}?model=hermes-agent
PUT  /v1/internal/open-webui-bridge/bindings
POST /v1/internal/open-webui-bridge/bindings/{chat_id}/close
POST /v1/internal/open-webui-bridge/bindings/{binding_id}/run
```

这些路由使用 Manager JWT 鉴权，并要求 service claim 包含 `internal:*` 或更窄的 bridge action。路由内部仍按 `auth.user_id` 约束 subject，不能接受外部传入 subject 越权操作。

新增用户级 run 读取路由：

```text
GET /v1/my-runs/{run_id}
```

Orchestrator 用它轮询当前用户 run 状态，避免使用 admin endpoint。

## Agent 创建、复用和绑定流程

### 明确控制请求

1. Orchestrator 验证 `agent_bridge_context`。
2. Orchestrator 提交 `CreateAgent` request 到 Manager。
3. `structured_payload` 包含 Bridge source 元数据：

```json
{
  "constraints": {
    "trigger_mode": "manual",
    "allowed_actions": ["analyze", "prepare_change", "run_checks"],
    "require_approval_for_side_effects": true
  },
  "bridge_source": {
    "kind": "open_webui",
    "chat_id": "<chat-id>",
    "session_id": "<open-webui-session-id>",
    "message_id": "<message-id>",
    "model": "hermes-agent"
  }
}
```

`bridge_source` 不包含 secret、signature 或 email。

4. 如果 request 立即 fulfilled 并返回 `agent_id`，Orchestrator 创建或复用 session，并写入 binding。
5. 如果 request 需要审批，Manager 在 approval fulfill 时根据 `bridge_source` 创建或复用 session，并写入 binding。
6. Orchestrator 返回 request_id、approval_id、agent_id/session_id/run_id 状态摘要。

### Session 创建幂等

session idempotency key：

```text
openwebui:<subject>:<chat_id>:<agent_id>
```

同一个 Open WebUI chat 重试不会创建重复 session。不同 Open WebUI chat 即使复用同一个 agent，也会得到不同 session。

### 后续消息

1. Orchestrator 验证 context。
2. 根据 `subject + chat_id + model` 解析 active binding。
3. 将用户消息 append 到 `agent_session_messages`。
4. 创建 `agent_run`，`session_id` 指向该 session。
5. run idempotency 优先使用 Open WebUI `message_id`，没有则使用 Bridge nonce。
6. Orchestrator 轮询 `GET /v1/my-runs/{run_id}`，直到完成或超时。
7. 完成时返回 `result_summary`；超时时返回 run_id、状态和 trace_id。

Worker 仍只通过现有 queue/lease/heartbeat 推进 run，不需要为 Open WebUI 单独启动 Worker。多 session 并发依赖现有 Postgres lease / `SKIP LOCKED` 正确性边界。

### 解绑和关闭

Orchestrator 识别明确关闭指令，例如：

```text
结束 agent session
关闭当前 agent session
退出 agent 模式
```

处理：

1. 验证 context。
2. 解析 binding。
3. 调用 Manager 关闭 `agent_session`。
4. 将 binding 标记为 `closed`。
5. 后续同一 Open WebUI chat 的普通消息恢复默认 Hermes passthrough，除非用户再次发起控制请求。

## 部署变更

### Repo 文件

新增：

```text
deploy/open-webui/functions/agent_identity_bridge_filter.py
deploy/scripts/install-openwebui-function.sh
deploy/scripts/install-openwebui-function-db.sh
deploy/open-webui/functions/test_agent_identity_bridge_filter.py
agent-platform/crates/agent-store/migrations/0004_open_webui_bridge_bindings.sql
```

更新：

```text
deploy/docker-compose.yml
deploy/README.md
agent-platform/crates/agent-core
agent-platform/crates/agent-store
agent-platform/crates/agent-manager
agent-platform/crates/agent-orchestrator
docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md
docs/chat_huixiangdou_issues/ISSUE-008-open-webui-agent-identity-session-bridge.md
```

### `.env`

更新 `deploy/.env` 前必须先执行：

```text
deploy/scripts/env-backup.sh backup
```

新增变量名：

```text
AGENT_BRIDGE_SECRET
AGENT_BRIDGE_ISSUER
AGENT_BRIDGE_MAX_CLOCK_SKEW_SECONDS
AGENT_BRIDGE_RESOURCE_ALLOWLIST
AGENT_BRIDGE_USER_ROLE
AGENT_BRIDGE_ADMIN_ROLE_MAPPING
AGENT_BRIDGE_RUN_WAIT_TIMEOUT_SECONDS
AGENT_BRIDGE_RUN_POLL_INTERVAL_MS
AGENT_JWT_SECRET
AGENT_MANAGER_SERVICE_ID
AGENT_MANAGER_JWT_TTL_SECONDS
AGENT_PLATFORM_ALLOW_DEV_HEADERS
```

只报告备份路径和变量名，不输出任何 secret 值。

### Open WebUI Function 安装

使用正式 Open WebUI，不创建临时 Open WebUI：

1. 优先通过正式 Open WebUI API 安装/更新 `agent_identity_bridge` Filter。
2. 绑定到 `hermes-agent`。
3. 设置 active。
4. Valves 写入 `AGENT_BRIDGE_SECRET`、issuer、目标 model。
5. 验证 Function 生效后再关闭 Manager dev headers。

如当前可用账号不是 Open WebUI admin，或 Function API 返回 401，使用正式容器/DB installer：

```text
deploy/scripts/install-openwebui-function-db.sh
```

该 fallback 只写入正式 Open WebUI 挂载的 `webui.db` 并重启正式 `open-webui` 服务，不创建临时 Open WebUI。它属于正式部署路径，不是测试 workaround。

## 测试计划

### 本地自动化

必须通过：

```text
cargo fmt --check
cargo test --workspace
git diff --check
docker compose config
```

新增测试覆盖：

1. Filter 规范化 JSON 和 HMAC 签名稳定。
2. Orchestrator 接受有效 context。
3. Orchestrator 拒绝错误 issuer。
4. Orchestrator 拒绝过期 timestamp。
5. Orchestrator 拒绝缺失 chat_id 的控制请求。
6. Orchestrator 对控制请求缺 context fail closed。
7. Orchestrator passthrough 前删除 `agent_bridge_context`。
8. Orchestrator 不扫描 Open WebUI follow-up prompt 历史区。
9. Manager JWT 模式下拒绝 dev headers。
10. Manager bridge binding upsert/get/close。
11. session idempotency 防止重复创建。
12. run idempotency 防止 Open WebUI 重试产生重复 run；message append 去重尚未做 schema 扩展。
13. 同一 agent 多 chat 创建不同 session。
14. 不同 Open WebUI 用户不能读取对方 binding/session/run。
15. Worker 多 session run claim 不串上下文。

### 正式远程 Docker smoke

按最小关键路径先执行，失败先记录证据再判断是否修复：

| 用例 ID | 预期 |
| --- | --- |
| BRIDGE-AUTH-01 | 登录正式 Open WebUI，模型列表仍显示 `hermes-agent` |
| BRIDGE-AUTH-02 | 普通短聊天仍走默认 Hermes Agent |
| BRIDGE-AUTH-03 | 普通长回答请求可完成，会话可保存 |
| BRIDGE-AUTH-04 | 控制请求在 Manager 审计中显示 `requested_by_user=openwebui:<id>` |
| BRIDGE-AUTH-05 | 直接篡改或删除 context 的控制请求被 Orchestrator 拒绝 |
| BRIDGE-AUTH-06 | Orchestrator 转发 Hermes 的 payload 不包含 `agent_bridge_context` |
| BRIDGE-SESSION-01 | 控制请求 fulfilled 后创建 active binding 和 agent_session |
| BRIDGE-SESSION-02 | 同一 Open WebUI chat 后续消息复用同一 agent_session |
| BRIDGE-SESSION-03 | 新 Open WebUI chat 复用同一 agent 但创建不同 agent_session |
| BRIDGE-SESSION-04 | 关闭当前 agent session 后，同一 chat 恢复默认 Hermes passthrough |
| BRIDGE-RUN-01 | 已绑定 session 的用户消息自动创建 run |
| BRIDGE-RUN-02 | Worker 处理 run 并写回 result_summary |
| BRIDGE-RUN-03 | 两个 Open WebUI chat 并发 run 不串 session context |
| BRIDGE-SEC-01 | `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false` 后 Open WebUI 控制面仍可用 |
| BRIDGE-SEC-02 | Manager、Worker、Observer 端口仍不暴露公网 |
| BRIDGE-REG-01 | Open WebUI follow-up suggestion 不误触发控制请求 |
| BRIDGE-REG-02 | 停止生成、刷新、历史会话保存行为不回退 |

测试报告写入：

```text
docs/CHAT_HUIXIANGDOU_OPENWEBUI_TEST_REPORT.md
```

Issue 状态更新：

```text
docs/chat_huixiangdou_issues/ISSUE-008-open-webui-agent-identity-session-bridge.md
```

测试和 issue 文档不得写入密码、token、API key 或 secret。

## 验收标准

完成后必须同时满足：

1. Open WebUI 普通聊天默认仍走 Hermes。
2. Open WebUI 控制请求不再落到 `dev-user`。
3. Manager 审计中可看到真实 `openwebui:<id>` subject。
4. Manager 正式运行在 JWT 模式，dev headers 关闭。
5. 同一 Open WebUI chat 重启 Orchestrator 后仍能找回 binding。
6. 同一用户多个 Open WebUI chat 对同一 agent 有不同 session。
7. 不同用户之间 agent/session/run/binding 不互相可见。
8. Worker 复用同一队列处理多 session run，不出现上下文串线。
9. `agent_bridge_context` 不会发给 Hermes。
10. Function 源码、安装脚本、migration、compose/env 说明都进入 repo。
11. 正式远程 Docker 部署完成后通过 smoke。
12. 不存在临时 Open WebUI 容器或临时测试入口。

## 风险和对策

| 风险 | 对策 |
| --- | --- |
| Function 未运行 | 控制请求 fail closed，普通聊天 passthrough |
| Function secret 泄露 | secret 只在 `.env` 和 Valves；支持轮换 |
| Open WebUI admin 自动映射为 Agent Platform admin | 默认禁用 admin 映射，必须显式配置 |
| Orchestrator 重启丢 session | binding 存 Postgres，不再依赖内存 |
| 审批后 Orchestrator 无法自动绑定 | Manager 在 fulfill approval 时根据 `bridge_source` 写 binding |
| Open WebUI 重试导致重复 run | 使用 message_id/nonce run idempotency；message append 去重另行扩展 |
| 内部字段进模型上下文 | passthrough 前删除并加测试 |
| 多 session 上下文串线 | binding key 包含 subject/chat/model，run 绑定 session_id |
| dev header 残留 | JWT smoke 通过后设置 `AGENT_PLATFORM_ALLOW_DEV_HEADERS=false` |
| Open WebUI Function API 漂移 | repo 保留 Function 源码和安装脚本；必要时正式 Admin Panel 导入 |

## 官方依据

- Open WebUI Filter Functions: https://docs.openwebui.com/features/extensibility/plugin/functions/filter/
- Open WebUI Functions: https://docs.openwebui.com/features/extensibility/plugin/functions/
- Open WebUI API / Filter behavior: https://docs.openwebui.com/reference/api-endpoints/
