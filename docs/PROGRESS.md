# 进展与决策记录

## 当前现实

- 仓库分支主线已切到“通灵玉”第一版。
- `resources/base/hongloumeng/` 旧基础资料产物已删除。
- 旧红楼梦专用资料抽取脚本已删除。
- EPUB 抽取已改为通用 source snapshot 脚本。
- 维基文库/MediaWiki source snapshot 下载脚本已新增。
- source snapshot 抽取已能保留 ruby 注音到 `rare_char_annotations`。
- `resources/styles/buhongjushi/` 风格转录和元数据保留。
- `src/tonglingyu_agent/` 仍是实现骨架。
- 正式基础资料 source snapshot 尚未生成。
- 知识库、Gateway、内部 Agent profile、reviewer 审校和 Open WebUI “通灵玉”模型入口尚未实现。

## 已完成

- 建立 B 站视频处理流水线。
- 处理“红楼梦文本探究”合集 60 个视频。
- 生成视频 `.txt`、`.srt`、`.transcript.json`。
- 完成第一条视频重点术语校订，并确认三份转录文件文本一致。
- 导入通灵玉第一版产品和架构设计文档。
- 明确视频转录是 `style_material`，不是主证据库。
- 明确新资料先进入 `resources/sources/` source snapshot，再进入知识库构造。
- 明确生僻字、异体字、旧字形和来源中已有读音必须保留。
- 清理旧平台化测试和旧基础库相关文档入口。

## 重要决策

- 通灵玉第一版先验证窄闭环：资料快照、知识库、证据卡片、证据包、reviewer 审校和分层回答。
- 不再使用旧 `resources/base/hongloumeng/` 作为任何知识库输入。
- 不再沿用旧专用抽取脚本或旧知识库命名。
- 维基文库《红楼梦》全本、脂批本和可追溯版本资料是第一批基础资料候选。
- source snapshot 是资料进入知识库前的标准形态。
- 规范化文本只能作为检索辅助字段；原始字形和读音标注必须保留。
- 风格资料只影响表达方式和讲解路径，不能覆盖正文、脂批、版本或校订证据。
- `不红居士` 是风格名，不替换转录文本中的 `不红君`。
- `官中`、`宫中`、`公中` 等同音高风险词必须回到已登记证据确认。

## 下一步

1. 下载维基文库《红楼梦》全本 source snapshot。
2. 下载脂批本、脂评相关页面或其他可追溯公开来源。
3. 为 source snapshot 建立来源登记检查。
4. 实现 `src/tonglingyu_agent/` 下的最小建库 loader。
5. 实现 SQLite schema、FTS 和 `rare_char_annotations` 表。
6. 定义证据卡片 schema 并实现最小查询。
7. 建立第一批评测问题，覆盖正文、脂批、版本、字形读音和证据不足。
8. 再实现 Gateway、内部 Agent profile 和 reviewer 审校链路。
