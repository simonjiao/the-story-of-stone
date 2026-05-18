# 25 繁简检索与默认简体回答 Checklist

## 当前状态

状态：repo-local 实现已完成并通过验证。

本文件记录通灵玉问答的繁简检索、默认简体回答和 raw evidence 保真实现。
这里的完成口径是代码、schema、检索、回答、reviewer、eval 和本地 replay
闭合；不把本地验证等同于目标环境 production release。目标环境
Open WebUI/browser-side 发布验收仍按既有 release readiness gate 执行。

本次没有采用 `zhconv`，也没有更改项目 license。原因是当前需求可由
`ferrous-opencc` 的确定性 T2S 转换能力加项目补充规则覆盖，且
`ferrous-opencc` 为 Apache-2.0；只有在后续确认必须引入 `zhconv` 时，
才需要重新评估并更改 license。

## 目标链路

```text
source raw text
  -> normalized search text
  -> normalized source title
  -> normalized alias / canonical person id
  -> raw + normalized retrieval ranking
  -> raw evidence card
  -> simplified Chinese answer renderer
  -> reviewer raw evidence check
  -> deterministic replay / RQA report
```

## 设计边界

- [x] 原始 source snapshot 不被繁简转换覆盖。
- [x] `EvidenceCard.text` 保留来源原字形。
- [x] `block_id`、`source_title`、`source_url`、`revision_id` 不因转换改变。
- [x] 自动转换只进入检索归一化字段、别名归一化字段和 query expansion。
- [x] 回答正文默认简体；原文引文保留 raw 字形。
- [x] reviewer 审校 claim 时回到 raw evidence，而不是只看 normalized text。
- [x] 人物、别名、称谓绑定 canonical `person_id`，不只靠单点字符串命中。
- [x] 繁简转换集中在统一 normalizer，业务路径不散落手写 mapping。
- [x] 转换规则确定性、可测试、可 replay。
- [x] schema 采用 additive migration，不删除既有 package、audit、session、RQA
  数据。

## Milestone A：统一 Text Normalizer

状态：已完成。

- [x] 定义统一 `TextNormalizer` 抽象。
- [x] 提供 `normalize_for_search(text)`。
- [x] 提供 `normalize_query(question)`。
- [x] 提供 `normalize_alias(alias)`。
- [x] 提供 `normalize_title(source_title)`。
- [x] 引入确定性 OpenCC 类转换能力：`ferrous-opencc` T2S。
- [x] 保留项目补充规则，覆盖 `寳`、旧字形、异体字和 OpenCC 未覆盖字符。
- [x] Gateway plan 侧复用 runtime normalizer，不再维护独立 mapping。
- [x] 单测覆盖繁转简、异体字、旧字形、混合文本和稳定输出。

完成证据：

- `text_normalizer_uses_opencc_plus_project_overrides`
- `cargo test --manifest-path Cargo.toml -p tonglingyu-runtime`

## Milestone B：Schema 与索引

状态：已完成。

- [x] `blocks.normalized_text` 继续作为正文归一化字段。
- [x] `blocks.normalized_source_title` 已加入。
- [x] `aliases.normalized_alias` 已加入。
- [x] `people.person_id` 作为 canonical entity id。
- [x] `normalized_text` 查询路径保留。
- [x] `normalized_source_title` 查询索引已加入。
- [x] `normalized_alias` 查询索引已加入。
- [x] migration additive。
- [x] migration 幂等。
- [x] rebuild 可输出 KB diff。
- [x] 旧 package、audit、session、RQA 表不删除、不重建。

完成证据：

- `kb_schema_adds_source_usage_metadata_columns_to_existing_sources_table`
- `runtime_schema_rolls_back_failed_migration_batch`
- KB rebuild smoke：5 sources、10419 blocks。

## Milestone C：Alias 与 Canonical Entity

状态：已完成。

- [x] 标准人物名、别名、称谓统一绑定 `person_id`。
- [x] `史湘云`、`史湘雲`、`湘云`、`湘雲`、`云妹妹` 归到
  `person:xiangyun`。
- [x] `林黛玉`、`黛玉`、`林姑娘`、`颦儿`、`顰兒` 归到
  `person:daiyu`。
- [x] `薛宝钗`、`薛寶釵`、`宝钗`、`寶釵` 归到同一 canonical id。
- [x] 查询时先识别 canonical entity，再扩展该 entity 的全部别名。
- [x] 别名扩展同时包含 raw alias 和 normalized alias。
- [x] 别名冲突 fail closed，不静默覆盖。
- [x] 人物 id 只用于检索聚合，不生成无证据人物事实。

完成证据：

- `text_search_matches_simplified_query_to_traditional_alias_and_raw_evidence`
- `text_search_matches_traditional_query_to_simplified_alias_and_text`
- `text_search_prefers_full_normalized_character_match_over_short_alias_only`

## Milestone D：检索双轨匹配

状态：已完成。

- [x] 查询 term 保留 raw term。
- [x] 查询 term 生成 normalized term。
- [x] SQL 同时匹配 `text` 与 `normalized_text`。
- [x] SQL 同时匹配 `source_title` 与 `normalized_source_title`。
- [x] alias 查询同时匹配 `alias` 与 `normalized_alias`。
- [x] RQA report 记录 raw/normalized 命中通道。
- [x] ranking 优先正文命中，不让 source title-only 命中压过正文证据。
- [x] 标题、极短块、纯人名块有降权策略。
- [x] 人物介绍类问题优先完整人物名 normalized 命中，不被短别名-only
  证据压过。
- [x] 章节定位词进入 exact-term 保护，避免关键边界块被泛排序挤出。

完成证据：

- `text_search_prioritizes_body_match_over_source_title_only_match`
- `required_exact_terms_protect_core_eval_targets`
- 内置 eval：103/103 passed。

## Milestone E：回答默认简体

状态：已完成。

- [x] 回答 renderer 默认输出简体说明文字。
- [x] 人物标准名默认使用简体 canonical display name。
- [x] 原文引文保留 raw 字形。
- [x] 引文外解释不混用不可控繁体。
- [x] 人物介绍不再只输出 evidence list，而是先成文回答再列依据。
- [x] 证据不足说明为简体。
- [x] reviewer 降级说明沿用简体安全回答。
- [x] 不牺牲证据准确性，不把 normalized text 当原文引文。

完成证据：

- `intro_answer_defaults_to_simplified_body_and_keeps_raw_quotes`
- `trim_text_around_locates_normalized_focus_without_mutating_raw_text`
- dry-run：`介绍史湘云` 与 `介紹史湘雲` 返回同一组证据和同一简体主体。

## Milestone F：Reviewer 防污染

状态：已完成。

- [x] reviewer 输入包含 raw evidence。
- [x] normalized 命中信息只作为检索质量信号，不替代 raw evidence。
- [x] claim-to-evidence map 绑定 raw `evidence_id`。
- [x] 人名、诗词、判词、铭文、异体字 claim 回到 raw text。
- [x] normalized 命中但 raw evidence 不支持时仍会降级。
- [x] 回答不得把简化后的文本声称为原文引文。
- [x] 别名归并只扩大召回，不扩大结论。

完成证据：

- `reviewer_blocks_no_evidence`
- `reviewer_blocks_commentary_only_body_claim`
- `reviewer_downgrades_facsimile_authoritative_collation_claim`
- 内置 eval：forbidden conclusion avoided 103/103。

## Milestone G：评测矩阵

状态：已完成。

- [x] 简体 query 到繁体正文。
- [x] 繁体 query 到简体 alias / 正文。
- [x] 简繁混合 query。
- [x] 异体字 query。
- [x] 旧字形 query。
- [x] 人物名。
- [x] 称谓。
- [x] 地名。
- [x] 诗词判词。
- [x] 通灵玉铭文。
- [x] source title 命中。
- [x] 极短块降权。
- [x] raw 引文保真。
- [x] 默认简体回答。
- [x] reviewer raw evidence check。
- [x] 旧 package replay 不退化。

最低样例验收：

- [x] `介绍史湘云` 命中 `史湘雲` / `湘雲` raw 证据。
- [x] `介紹史湘雲` 命中同一 canonical 人物证据。
- [x] `介绍林黛玉` 不再只返回 `<poem>林黛玉</poem>`，而是成文回答。
- [x] `寶釵是谁` 可命中 `宝钗` 或 `寶釵`。
- [x] `顰兒是谁` 可归到 `林黛玉` 相关别名。
- [x] `通靈玉上的字` 可命中铭文证据。
- [x] 回答主体为简体。
- [x] 原文引文保持 raw 字形。

## Milestone H：发布验收

状态：repo-local gate 已完成；目标环境 live gate 未在本 worktree 执行。

- [x] `cargo fmt --all --check` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [x] `cargo test --workspace` 通过。
- [x] KB rebuild smoke 通过。
- [x] retrieval eval 通过：103/103。
- [x] evidence package replay 通过：
  `answer_source=local_replay_no_upstream`。
- [x] runtime dry-run 通过：`介绍林黛玉`、`介绍史湘云`、
  `介紹史湘雲`。
- [x] RQA report 能区分 raw hit 与 normalized hit。
- [x] 普通回答没有泄露 secret、trace、内部治理字段。
- [ ] 目标环境 Open WebUI/browser-side smoke：本次未在 worktree 内执行；
  生产发布前仍必须走既有 release readiness gate。
- [ ] release report 生产发布摘要：本次不声明 production release ready；
  生产发布前仍必须由 release report 验证。

完成口径：

- repo-local 代码、DB、检索、回答、reviewer、eval、replay 已闭合。
- 自动转换只用于检索和归一化字段，不改证据。
- 默认简体回答不影响原文准确性。
- 当前结论不能外推为目标环境 production release ready。
