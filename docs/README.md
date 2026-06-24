# 通灵玉文档地图

## 当前边界

本目录只保留通灵玉 Gateway、Runtime、Open WebUI 入口、运行期上下文治理、LLM 接入
边界和实时文字协议相关文档。外部内容由 runtime SQLite DB 或 release bundle 发布；
本仓库只负责读取、迁移、Gateway 协议、安全、审计、smoke 和 release gate。

## 目录结构

| 目录 | 用途 |
| --- | --- |
| `需求/` | 产品定位、用户场景、证据原则、概念模型、验收和负面清单。 |
| `架构/` | 总体架构、Agent/Gateway/Runtime 边界、实时协议和 Redis Streams 方案。 |
| `外部/` | Open WebUI、客户端、管理后台和运维入口看到的外部接口契约。 |
| `内部/` | Gateway、Runtime、profile、权限审计、LLM 和 eval 的内部协作契约。 |
| `子模块/` | Scoped Context、Memory、Query Expansion、Current Window、Question Frame 等模块文档。 |

根目录下保留运行和工程规则：

| 文档 | 用途 |
| --- | --- |
| `PROGRESS.md` | 当前 Gateway/Runtime 进展与决策记录。 |
| `RUNBOOK.md` | 当前可执行运行命令。 |
| `LINT_AND_TEST_RULES.md` | 验证规则。 |
| `RUST_CODING_RULES.md` | Rust 编码规则。 |
| `VERSIONING_RULES.md` | 版本规则。 |

## 推荐阅读顺序

第一轮只读当前状态和 Gateway 边界：

1. `PROGRESS.md`
2. `架构/Gateway设计.md`
3. `架构/Gateway_Realtime_Redis_Architecture.md`
4. `RUNBOOK.md`

第二轮读需求和接口契约：

1. `需求/产品定位与边界.md`
2. `需求/用户场景与产品流程.md`
3. `需求/总体原则.md`
4. `需求/概念模型与证据分层.md`
5. `外部/外部接口契约.md`
6. `内部/内部接口契约.md`
7. `内部/权限审计与安全治理.md`
8. `需求/验证方案与验收标准.md`
9. `需求/负面清单与反模式.md`

第三轮读运行期模块：

1. `架构/总体架构.md`
2. `架构/四个Agent设计.md`
3. `架构/现有架构差距与实施方向.md`
4. `架构/Runtime接入设计与实施计划.md`
5. `子模块/Scoped_Context与受控Memory.md`
6. `内部/LLM支持点与全路径Eval方案.md`
7. `子模块/Query_Expansion_Management.md`
8. `子模块/Current_Window_Context_Path_Design.md`
9. `子模块/Question_Frame_Relation.md`

## 验收条目处理原则

不再为每个阶段保留独立清单文档。后续验收条目只放在对应子模块文档内，形式保持为
“边界 / 子能力 / 验收”，避免把 docs 重新拆成大量历史进展文件。
