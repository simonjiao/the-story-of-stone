# ISSUE-010: Open WebUI session secret 未由 `.env` 管理

## 状态

RESOLVED

## 等级

P1

## 关联测试

| 用例 ID | 状态 | 说明 |
| --- | --- | --- |
| `P1-OPENWEBUI-AUTH-20260509` | PASS | 正式 Open WebUI auth API 返回 HTTP 200，模型列表包含 `hermes-agent`。 |
| `P1-OPENWEBUI-SECRET-20260509` | FAIL->PASS | 初次 smoke 发现测试 JWT 只能读取容器内生成的 `.webui_secret_key`，修复后 `WEBUI_SECRET_KEY` 来自 `.env` 且长度满足要求。 |
| `P1-OPENWEBUI-SESSION-20260509` | PASS | 修复后通过 `/api/chat/completions` 完成基础聊天，并通过 `/api/v1/chats/new` 创建、读取、清理测试会话。 |

## 背景

P1 正式 smoke 补齐登录、模型选择、基础聊天和会话保存关键路径时，发现 Open WebUI 容器没有从 `deploy/.env` 注入 `WEBUI_SECRET_KEY`。这会让 Open WebUI 退回容器内生成文件，不符合“配置和密钥只走 `.env` 或既有配置入口”的部署规则，并可能在容器重建后导致会话签名不稳定。

## 修复

```text
1. `deploy/docker-compose.yml` 为 `open-webui` 显式注入 `WEBUI_SECRET_KEY`，来源为 `OPEN_WEBUI_SECRET_KEY`。
2. `deploy/README.md` 将 `OPEN_WEBUI_SECRET_KEY` 加入必填配置。
3. 远端变更前执行 `deploy/scripts/env-backup.sh backup`。
4. 远端 `.env` 生成强随机 `OPEN_WEBUI_SECRET_KEY`，未输出 secret 值。
5. 重建启动 `open-webui`，并确认 `WEBUI_SECRET_KEY` 来自环境变量。
```

## 验证

```text
1. 远端备份路径：`$HOME/OneDrive/backup/the-story-of-stone/deploy-env/deploy.env.bak.20260509-154242`。
2. `docker compose config --quiet` 通过。
3. `docker compose up -d open-webui` 后 Open WebUI health 为 healthy。
4. 容器内校验：`webui_secret_source=env`、`webui_secret_length_ok=True`。
5. 关键路径复测：auth 200、models 200 且选中 `hermes-agent`、chat 200、临时 chat 创建后可读取并已删除。
6. 公网 `/api/config` 返回 HTTP 200；重启恢复后 30 秒日志窗口未检出错误关键词。
```
