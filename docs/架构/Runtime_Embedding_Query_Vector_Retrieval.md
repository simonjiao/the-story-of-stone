# Gateway 与 Knownledge Retriever 外部接口

## 边界

向量库查询不在 Gateway 内实现。Gateway 不加载向量索引、不调用 embedding provider、不维护 query embedding cache，也不选择或调用任何向量库后端。

Gateway 只作为外部调用方接入 `tonglingyu-knownledge` HTTP retriever：

- 启动时检查 `/health` 和 `/metadata`。
- 请求时调用 `/retrieve`。
- 校验返回 schema 和关键 contract。
- 把 EvidenceDoc 映射为 Runtime EvidenceCard。
- 把完整 request / raw response trace 交给 Runtime workflow。

向量检索、batch embedding、rerank、route fusion、EvidenceDoc 生成和 release DB 读取都属于 knownledge retriever 内部职责。

## Gateway 配置

| 配置 | 必需 | 说明 |
| --- | --- | --- |
| `TONGLINGYU_RETRIEVER_BASE_URL` | 是 | knownledge retriever HTTP base URL，Gateway `serve` 启动时必须配置。 |
| `TONGLINGYU_RETRIEVER_TIMEOUT_SECS` | 否 | Gateway 调用 retriever 的 HTTP 超时，默认 20 秒。 |
| `TONGLINGYU_RETRIEVER_RERANK` | 否 | 是否在 SearchPlan 中要求 retriever 执行 rerank，默认 false。 |

Gateway 健康检查会把 retriever health 纳入 `/healthz`。retriever 不 ready 或不可达时，Gateway 返回 degraded / 503。

## Startup Contract

Gateway 启动必须先调用：

```text
GET /health
GET /metadata
```

`/health` 必须满足：

- `ok = true`
- `schema_version = tonglingyu.agent_retriever.service_health.v1`
- `record_type = agent_retriever_service_health`
- `ready = true`
- `components` 是对象

`/metadata` 必须满足：

- `ok = true`
- `schema_version = tonglingyu.agent_retriever.service_metadata.v1`
- `record_type = agent_retriever_service_metadata`
- `contracts.search_plan_schema = tonglingyu.agent_retriever.search_plan.v1`
- `contracts.retrieve_options_schema = tonglingyu.agent_retriever.retrieve_options.v1`
- `contracts.retrieve_response_schema = tonglingyu.agent_retriever.retrieve_response.v1`
- `contracts.evidence_pack_schema = tonglingyu.agent_retriever.evidence_pack.v1`
- `contracts.error_response_schema = tonglingyu.agent_retriever.error_response.v1`
- `capabilities.routes` 包含 `bm25/vector/entity/event/poem/commentary`
- `capabilities.required_routes` 包含 `vector`
- `adapter_guidance.stable_input` 声明 `SearchPlan + RetrieveOptions`

任一检查失败时 Gateway 启动失败，不进入服务状态。

## Retrieve Request

Gateway 在线 chat workflow 只调用：

```text
POST /retrieve
```

请求 envelope：

```json
{
  "request_id": "tly-...",
  "session_id": "user-session-...",
  "caller": "tonglingyu-gateway",
  "graph_node": "chat_workflow",
  "search_plan": {
    "schema_version": "tonglingyu.agent_retriever.search_plan.v1",
    "query": "用户解析后的问题",
    "routes": ["bm25", "vector", "entity", "event", "poem", "commentary"],
    "keyword_queries": ["用户解析后的问题"],
    "semantic_queries": ["用户解析后的问题"],
    "top_k": 8,
    "candidate_limit": 160,
    "route_record_limit": 10,
    "route_doc_limit": 4,
    "vector_top_k": 80,
    "include_cards": true,
    "rerank": false,
    "rerank_top_k": 8,
    "fail_on_route_error": true,
    "fail_on_rerank_error": true,
    "raw_plan": {
      "planner": "tonglingyu-gateway",
      "common_recall_kind": "workflow",
      "route_policy": "all_knownledge_retriever_routes",
      "vector_required": true
    }
  },
  "retrieve_options": {
    "schema_version": "tonglingyu.agent_retriever.retrieve_options.v1",
    "trace_level": "route",
    "trace_doc_limit": 8,
    "include_ref_audit": false,
    "expected_refs": {},
    "forbidden_refs": {},
    "audit_k_values": [5, 10, 20]
  },
  "include_raw": false,
  "metadata": {
    "trace_id": "tly-...",
    "user_session_id": "user-session-...",
    "interaction_context_id": "interaction-context-...",
    "context_pack_id": "context-pack-...",
    "context_pack_ref": "context-pack://...",
    "required_evidence_types": ["base_text"],
    "question_type": "base_text"
  }
}
```

Gateway 只表达 route 和数量上限要求；具体向量库查询由 retriever 内部完成。

## Gateway Common Recall API

外部 workflow 如果只需要常用召回，不应手写 retriever SearchPlan。Gateway 提供认证后的封装接口：

```text
POST /v1/retrieval/recall/{kind}
Authorization: Bearer <gateway key>
```

`kind` 支持：

| kind | 说明 | Gateway SearchPlan 映射 |
| --- | --- | --- |
| `person` | 查询人/人物/别名/身份 | `entity` first，辅以 `event/bm25/vector` |
| `event` | 查询事/情节/事件 | `event` first，辅以 `entity/bm25/vector` |
| `poem` | 查询诗词/曲词/文本对象 | `poem` first，辅以 `entity/event/commentary/bm25/vector` |
| `judgement` | 查询判词/册页判语 | `poem` first，辅以 `commentary/event/entity/bm25/vector`，并写入 `entity_subtype:judgement` / `entity_facets:judgment` cues |
| `commentary` | 查询脂批/批语 | `commentary` first，辅以 `poem/event/bm25/vector` |
| `workflow` | 与 chat workflow 一致的全路由召回 | `bm25/vector/entity/event/poem/commentary` |

请求体：

```json
{
  "query": "宝钗判词",
  "session_id": "optional-session",
  "limit": 8,
  "rerank": false,
  "trace_level": "route",
  "trace_doc_limit": 8,
  "include_raw": false,
  "metadata": {}
}
```

Gateway 会构造 `tonglingyu.agent_retriever.search_plan.v1`，写入：

- canonical `routes`，必须包含 `vector`；
- `route_weights`、route-specific `queries`、`keyword_queries`、`semantic_queries`；
- 常用召回所需的 `structured_terms`、`expansion_terms`；
- schema 允许的 `filters`，例如 `chunk_kinds`、`entity_subtypes`、`entity_facets`；
- `include_cards=true`、`fail_on_route_error=true`、`fail_on_rerank_error=true`。

Gateway 不解释这些 filters，也不查询向量库；它只把封装后的 SearchPlan 通过 knownledge retriever HTTP `/retrieve` 发送出去。

成功 response：

```json
{
  "object": "tonglingyu.retrieval_common_recall",
  "schema_version": "tonglingyu.gateway.retrieval_common_recall.v1",
  "trace_id": "tly-...",
  "kind": "judgement_poem",
  "request": {
    "search_plan": {},
    "retrieve_options": {}
  },
  "response": {
    "schema_version": "tonglingyu.agent_retriever.retrieve_response.v1",
    "diagnostics": {}
  },
  "evidence_pack": {}
}
```

失败行为与 chat workflow 一致：retriever 不可达、超时、非 JSON、schema 不匹配或 `ok=false` 时返回 `503 retriever_failed`，记录 `retriever_common_recall_failed`，且 `fallback_used=false`。

## Retrieve Response

Gateway 期望 successful response：

```json
{
  "ok": true,
  "schema_version": "tonglingyu.agent_retriever.retrieve_response.v1",
  "record_type": "agent_retriever_retrieve_response",
  "request_id": "tly-...",
  "service": {
    "schema_version": "tonglingyu.agent_retriever.service.v1",
    "name": "tonglingyu_agent_retriever",
    "version": "agent_retriever_service_core_v0.1",
    "repo_root": "...",
    "index_path": "...",
    "env_file": "..."
  },
  "evidence_pack": {
    "schema_version": "tonglingyu.agent_retriever.evidence_pack.v1",
    "query": "...",
    "search_plan": {},
    "docs": [],
    "diagnostics": {},
    "sufficiency": {}
  },
  "diagnostics": {}
}
```

Gateway 会校验：

- response / pack / doc / service schema version。
- response `record_type`。
- pack 中 normalized SearchPlan 必须包含 `vector` route。
- EvidenceDoc 主 route 必须是 canonical route。
- `sufficiency.doc_count == docs.len()`。
- `sufficiency.direct_evidence_doc_count` 与 `segment_ids/commentary_ids` 实际数量一致。
- `diagnostics.fusion.fused_count >= docs.len()`。
- doc refs、source、display、source_scope、usage_policy 等 required 字段存在。

Gateway 不把 response 再解释为向量库结果，只把 EvidenceDoc 转成 Runtime EvidenceCard。

## Error Response

retriever 错误 envelope：

```json
{
  "ok": false,
  "schema_version": "tonglingyu.agent_retriever.error_response.v1",
  "record_type": "agent_retriever_error_response",
  "operation": "retrieve",
  "error": {
    "code": "vector_unavailable",
    "message": "...",
    "type": "RetrieverServiceRequestError",
    "retryable": false
  }
}
```

Gateway 行为：

- `/retrieve` 失败、超时、非 JSON、schema 不匹配或 `ok=false` 都返回 `503 retriever_failed`。
- 记录 `workflow_states.state = Failed with Controlled Response`。
- 记录 `audit_events.event_type = retriever_http_failed`。
- 不回退到 Runtime 本地 text/commentary search。

## Runtime Trace

Gateway 传给 Runtime 的 `retrieved_evidence.retrieval` 必须包含：

```json
{
  "schema_version": "tonglingyu.gateway.workflow_retrieval_input.v1",
  "retriever_base_url": "http://...",
  "request": {},
  "raw_request": {},
  "retrieve_response": {},
  "raw_retrieve_response": {},
  "request_response_trace": {
    "transport": "http",
    "tool_name": "tonglingyu.agent_retriever.retrieve_http",
    "method": "POST",
    "path": "/retrieve",
    "request": {},
    "response": {}
  },
  "evidence_pack": {},
  "cards": [],
  "diagnostics": {}
}
```

Runtime workflow step 会暴露：

- `allowed_tools = ["tonglingyu.agent_retriever.retrieve_http"]`
- `tool_calls = ["tonglingyu.agent_retriever.retrieve_http"]`
- `output.request_response_trace`
- `output.raw_retrieve_response`
- `output.evidence_pack`
- `output.diagnostics`

线上排查时，`workflow_states` 的 `Runtime Executed` 记录中可以从 `runtime_step_outputs[0].output.request_response_trace` 查到实际 HTTP request 和 raw response。

## 验收

- Gateway 代码中不得出现向量库查询实现或 embedding provider client。
- Gateway `serve` 缺少 `TONGLINGYU_RETRIEVER_BASE_URL` 时不能启动。
- retriever `/health` 或 `/metadata` contract 不满足时 Gateway 不能启动。
- `/retrieve` 成功后 audit 中有 `retriever_http_completed`，并记录 schema、doc_count、route_coverage、`fallback_used=false`。
- `/retrieve` 失败后 response 为 `503 retriever_failed`，并记录 `retriever_http_failed`。
- Runtime step trace 中保留 request / raw response / EvidencePack。
