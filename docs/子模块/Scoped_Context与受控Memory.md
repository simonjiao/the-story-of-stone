# Scoped Context 与受控 Memory

## 定位

Scoped Context 与受控 Memory 是 Gateway/Runtime 的运行期上下文治理模块。它负责把
Open WebUI 请求、当前窗口、session journal、授权 memory 摘要和 Runtime profile
可见上下文隔离开来。

该模块不是第 5 个 Agent，也不是长期事实源。它只提供请求上下文、受控投影、审计和
可回放边界；最终事实仍来自 runtime DB、证据包、reviewer、外部发布记录、状态流转和
action/audit log。

## 核心对象

| 对象 | 作用 |
| --- | --- |
| `user_session` | 外部用户会话隔离，不等于 Hermes session。 |
| `interaction_context` | 一次业务交互的上下文边界。 |
| `context_pack` | 请求级父对象，记录 resolved question、scope、policy 和 memory refs。 |
| `context_projection` | 面向单个 Runtime profile 裁剪后的可见上下文。 |
| `session_journal` | 结构化记录交互过程，用于审计、回放和延迟 memory 抽取。 |
| `memory_candidate` | 从 journal 抽取出的待审核长期 memory 候选。 |
| `memory_card` | 已审核并按 policy 启用读取的长期 memory 摘要。 |
| `memory_policy_decision` | 自动或人工策略决策记录。 |
| `memory_read_ref` | Runtime profile 可读取的脱敏、预算内 memory 摘要引用。 |

## 边界

1. 外部 Open WebUI conversation 不等于 Hermes session。
2. Runtime 调用前必须生成 `context_pack`。
3. Runtime profile 只能读取自己的 `context_projection`。
4. 长期 memory 只能由延迟 collector 从 journal 抽取、审核和沉淀。
5. Memory 不能进入 evidence package，不能改变 reviewer 裁决。
6. 普通响应、SSE、metrics 和普通日志不得暴露 raw journal、memory card id、
   candidate id、ACL 或 read refs。
7. replay 必须使用当时保存的 pack/projection snapshot 或 digest 绑定记录。

## 子能力

### Scoped Context Request Path

目标是让每次请求都有明确的 context pack、resolved question、scope 和审计记录。

验收边界：

1. 公共请求不能提交 memory/candidate/control 字段。
2. context pack 必须绑定 trace、session、resolved question 和 policy version。
3. 错误路径 fail-closed，并写入审计。

### Context Projection Runtime

目标是把 Runtime profile 可见上下文从完整 pack 收敛为按 consumer 裁剪的 projection。

验收边界：

1. `honglou-main`、`honglou-text`、`honglou-commentary`、`honglou-reviewer`
   只能读取各自 projection。
2. 未知 consumer、未知 runtime adapter 和 external agent 类型必须 fail-closed。
3. projection digest、tool policy digest 和 output contract digest 必须进入 audit。

### Memory Candidate Workflow

目标是把长期 memory 的候选、审核、状态流转和审计实现完整，但不默认打开读取面。

验收边界：

1. candidate 必须绑定 journal、trace、context 和 scope。
2. LLM 只能辅助抽取和分类，不能决定 ACL、retention、promotion 或 reviewer 裁决。
3. admin-only API/CLI 才能 list/read/update/review candidate。
4. rejected、expired、merged candidate 不能写出 active memory card。

### Scoped Memory Production

目标是在 Memory Candidate Workflow 之上打开受 policy、ACL、scope、TTL 和 read budget
约束的读取路径。

验收边界：

1. 自动策略只能处理低风险、低敏、稳定偏好和工作方法类 memory。
2. shared scope、profile_common、knowledge_space、research_topic 和 source_collection
   默认更保守，必要时进入人工审核。
3. read enablement 必须绑定 policy decision、scope、retention 和 revocation 状态。
4. context pack 只能携带 memory read ref，不携带 raw memory。

## 验证

最小验证集合：

```bash
cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-gateway
cargo test --manifest-path agent-platform/Cargo.toml -p tonglingyu-runtime
agent-platform/scripts/tonglingyu-gateway-smoke.sh
```

目标环境声明仍需要对应 release gate 或 saved report validator 绑定当前部署产物。
