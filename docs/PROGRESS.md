# 进展与决策记录

## 当前状态

- 主线已切到“通灵玉”第一版。
- 旧基础库产物和旧专用抽取脚本已删除。
- `scripts/extract_epub.py` 和 `scripts/download_wikisource.py` 已输出 source snapshot，并保留 `rare_char_annotations`。
- `resources/styles/buhongjushi/` 风格转录保留，不作为主证据库。
- `src/tonglingyu_agent/` 仍是骨架。
- 正式 `resources/sources/` 基础资料、SQLite/FTS 证据型知识库、Gateway、profiles、reviewer 和 Open WebUI 入口尚未实现。

## 已确认

- 第一版只验证“资料快照 -> 知识库 -> 证据卡片 -> 证据包 -> reviewer -> 分层回答”闭环。
- 维基文库《红楼梦》全本、脂批本和可追溯版本资料是第一批基础资料候选。
- 知识库按原文、脂批、版本、人物、关系、事件、诗词判词和评测题库分层，不做大向量库。
- 原始字形和来源中已有读音必须保留；规范化文本只能作为检索辅助字段。
- 现代白话摘要只能辅助检索，不能作为可引用证据。
- 风格资料只影响表达方式和讲解路径，不能覆盖正文、脂批、版本或校订证据。
- `不红居士` 是风格名，不替换转录文本中的 `不红君`。
- `官中`、`宫中`、`公中` 等高风险同音词必须回到已登记证据确认。

## 下一步

1. 下载并登记维基文库《红楼梦》全本、脂批本和版本资料。
2. 实现 source snapshot loader、SQLite schema、FTS、别名索引、反证/限制索引和 `rare_char_annotations` 表。
3. 定义证据卡片 schema，强制支持范围、不支持范围和校验状态。
4. 建立正文、脂批、版本、人物别名、诗词判词、字形读音和证据不足评测。
5. 再实现 Gateway、内部 Agent profiles、reviewer 审校和 Open WebUI 单入口。
