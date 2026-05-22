# Tonglingyu Query Expansion Management

## 目标

查询扩展词用于提高召回率，只能影响候选证据检索。它不能直接生成答案，不能作为 draft/review 拒收或放行 oracle，也不能把未命中的情节当作事实。

## 目录位置

- 内置默认目录：`agent-platform/crates/tonglingyu-runtime/resources/query_expansions.json`
- 可选外部目录：通过环境变量 `TONGLINGYU_QUERY_EXPANSIONS_PATH` 指向 JSON 文件

未配置外部目录时，runtime 使用内置目录。配置外部目录后，runtime 每次检索前读取文件元信息；当路径或修改时间变化时，下一次请求会重新解析目录，不需要重启进程。

外部目录解析失败、schema 不匹配或词条非法时，本次检索直接失败。不要静默回退到旧目录或内置目录，否则会掩盖配置错误并污染 trace。

## Schema

```json
{
  "schema_version": "tonglingyu.query_expansions.v1",
  "catalog_version": "2026-05-22.1",
  "entries": [
    {
      "id": "tonglingyu:loss-event",
      "trigger": {
        "all_any": [
          ["通灵宝玉", "通靈寶玉", "通灵玉", "通靈玉"],
          ["丢", "丟", "失", "不见", "不見", "几次", "幾次", "多少次"]
        ]
      },
      "terms": ["失玉", "良儿偷玉", "甄宝玉送玉", "扫雪拾玉"],
      "exact_terms": []
    }
  ]
}
```

每个 entry 必须包含：

- `id`：稳定标识，用于审计和排查。
- `trigger`：触发条件，至少包含 `any`、`all` 或 `all_any` 之一。
- `terms` 或 `exact_terms`：至少一类非空。

`trigger.any` 表示命中任一词即可触发；`trigger.all` 表示列出的词必须全部命中。匹配会同时看原始问题和项目规范化后的查询文本。

`trigger.all_any` 表示一组组“每组选一”的组合条件。例如通灵宝玉失玉类问题可以要求同时命中“通灵玉/通灵宝玉”组和“丢/失/几次”组。这样具体触发词也归 catalog 管理，而不是散落在代码里。

`schema_version` 表示 JSON 结构兼容性；`catalog_version` 表示内容版本。部署侧 catalog 必须与 story 内置 catalog 保持相同 `catalog_version` 和 entries digest，否则部署 gate 应失败。

## 管理规则

1. 召回词可以覆盖别名、繁简、异体字、章节标题、事件线索和常见问法。
2. 后四十回线索可以用于召回，但第八十一回及以后内容仍必须在证据和回答中显式标注为后四十回。
3. 新增事件线索时，需要补一条最小检索测试，证明它只改善候选证据召回，不改变本地答案或 reviewer 结论。
4. 线上调整外部目录后，用 trace 检查 `expanded_terms` 是否包含预期词，并检查最终证据包是否仍按证据边界表达。
