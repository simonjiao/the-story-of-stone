# ISSUE-003: 新对话未提示会继承 Hermes Agent 外部记忆

## 状态

READY_FOR_DEPLOY

## 等级

P2

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `CTX-01` | PASS | 在原会话建立测试代号 `codex-test-basic-20260507`。 |
| `CTX-02` | PASS | 原会话内追问测试代号，模型正确回答。 |
| `CTX-04` | WARN | 新建空白对话后，模型仍能回答上一会话的测试代号。 |

## 影响

Open WebUI 的“新对话”通常会让用户预期上下文隔离；如果 Hermes Agent 会注入跨会话记忆，而 UI 没有提示，用户可能误判隐私边界或上下文来源。

## 证据

- 新建空白对话后，页面不包含旧会话历史。
- 在新会话中提问：`请直接给出上一轮测试代号。如果你不知道，必须只输出 UNKNOWN_ONLY。不要输出其他文字。`
- 实际回答：`codex-test-basic-20260507`。
- 新会话 URL：`/c/8e112dab-d3a6-4a31-baf8-d8ef71f2e382`。

## 当前判断

从 Open WebUI 交互窗口视角，本部署更适合让“新对话”表现为会话隔离。Hermes 官方配置支持关闭持久 memory 与 user profile，因此本问题可以通过 Hermes 配置修复，不需要改 Open WebUI 代码。

## 已做修复

已更新 `deploy/scripts/render-hermes-config.sh`：

- 写入 `config.yaml` 前自动备份旧配置为 `config.yaml.bak.YYYYMMDD-HHMMSS`。
- 默认生成：

```yaml
memory:
  memory_enabled: false
  user_profile_enabled: false
```

已更新 `deploy/README.md`，说明渲染配置后需要重启 Hermes：

```bash
./scripts/render-hermes-config.sh
docker compose restart hermes
```

如果未来确认要启用跨会话记忆，可在 `.env` 设置：

```bash
HERMES_MEMORY_ENABLED=true
HERMES_USER_PROFILE_ENABLED=true
```

## 官方依据

- Hermes Agent configuration options: https://nousresearch-hermes-agent.mintlify.app/reference/configuration-options

## 后续动作

1. 在部署节点运行 `./scripts/render-hermes-config.sh`。
2. 运行 `docker compose restart hermes`。
3. 复测 `CTX-04`：新建空白对话后询问上一轮测试代号，应回答不知道或不包含旧代号。
4. 复测通过后将状态改为 `RESOLVED`。
