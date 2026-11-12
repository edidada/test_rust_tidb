# LatticeDB 设计文档集

> **项目定位**：一个用 Rust 实现、贴近工业界架构、面向博士研究/高阶实验的分布式 HTAP 关系数据库实验平台（下文简称 **LatticeDB**，代号可替换）。整体对标 [TiDB](https://github.com/pingcap/tidb) / CockroachDB 的分层设计，采用「计算层（SQL）— 分布式 KV 存储层 — 元数据调度层」三层解耦，并以 **Lab 实验** 形式逐阶段构建。
>
> **当前状态**：仅文档阶段（Design-only）。本目录不包含任何实现代码；所有设计均为后续实现的契约与验收依据。

---

## 1. 为什么是这样一个项目

| 诉求 | 本设计的回应 |
| --- | --- |
| 贴近工业界 | 组件划分、协议、编码、事务模型、优化器结构均对齐真实系统（TiDB/TiKV/PD、Volcano/Cascades、Percolator、Raft、Arrow/DataFusion），而非玩具实现 |
| 面向博士 | 每个关键子系统给出**问题定义、形式化模型、算法族、可证伪假设、消融实验与量化验收指标**，可直接支撑论文/技术报告 |
| 类似 Lab | 全项目拆为 `L0 … L14` 递进实验，每个 Lab 有前置依赖、交付物、Definition of Done 与可复现实验脚本 |
| 重点：算子 / 执行计划 / CBO / RBO | 这四块拥有独立的编号体系、完整枚举清单、专项验收方法与基准，是本项目的**核心贡献区** |

## 2. 文档目录

| 文档 | 内容 | 主要读者 |
| --- | --- | --- |
| [01-architecture.md](01-architecture.md) | 架构设计：分层架构、组件职责、数据流、技术选型、部署形态、非功能目标、研究问题地图 | 全体 |
| [02-high-level-design.md](02-high-level-design.md) | 概要设计：核心数据结构、模块接口、SQL 全生命周期、KV 编码、事务/下推/DDL 关键机制、Crate 划分 | 开发者 |
| [03-roadmap.md](03-roadmap.md) | 项目排期：阶段、Lab 列表、周次计划、里程碑 M0–M7、关键路径、风险缓冲、论文产出点 | 导师 / 项目负责人 |
| [04-feature-list.md](04-feature-list.md) | 功能点列表：按 19 个功能域枚举全部功能点（含优先级、Lab 映射、依赖、验收引用）；算子/RBO/CBO/计划四块单独全量枚举 | 全体（需求基线） |
| [05-acceptance-plan.md](05-acceptance-plan.md) | 功能验收规划：V0–V5 验收分级、各域验收标准与量化阈值、差分/模糊/混沌测试、可复现实验规范、追溯矩阵 | 测试 / 评审 |
| [06-distributed-test-plan.md](06-distributed-test-plan.md) | 分布式功能测试计划：DTP-1..7 阶段、DT-* 测试矩阵（存储/事务/元数据/DDL/MPP/HTAP/混沌/基准）、一致性验证方法、CI 门禁映射与追溯 | 测试 / 分布式开发 |

## 3. 统一编号体系（跨文档追溯）

所有文档共享以下 ID，改动时必须同步更新引用。

| 前缀 | 含义 | 示例 | 定义位置 |
| --- | --- | --- | --- |
| `C-` | 架构组件（Component） | `C-OPT-CBO` | 01 §3 |
| `OP-L-` / `OP-P-` / `OP-E-` / `OP-X-` | 逻辑算子 / 物理算子 / 分布式交换算子 / 表达式与向量化原语 | `OP-P-HASHJOIN` | 04 §4、01 §5 |
| `R-` | RBO 规则 | `R-PPD` | 04 §5 |
| `CB-` | CBO 能力点 | `CB-HIST-EQUIHEIGHT` | 04 §6 |
| `PL-` | 执行计划能力点 | `PL-ANALYZE` | 04 §7 |
| `F-<域>-` | 功能点，`域` 见 04 §2 | `F-OP-012` | 04 |
| `L0 … L14` | Lab 实验编号 | `L7` | 03 §3 |
| `M0 … M7` | 里程碑 | `M4` | 03 §4 |
| `W01 … W48` | 排期周次 | `W23` | 03 §2 |
| `AC-` | 验收项（Acceptance Criterion） | `AC-CBO-003` | 05 |
| `V0 … V5` | 验收等级（Level） | `V3` | 05 §2 |
| `P0 / P1 / P2` | 功能优先级：核心必做 / 重要 / 研究拓展 | `P0` | 04 §2 |
| `DT-<域>-` | 分布式功能测试项（Distributed Test） | `DT-TXN-001` | 06 §4 |
| `DTP-` | 分布式测试阶段（Test Phase） | `DTP-4` | 06 §3 |

**追溯链**：`功能点 F-*` → `所属 Lab L*` → `里程碑 M*` → `验收项 AC-*` → `验收等级 V*`。任何功能点必须能被至少一条 `AC-` 覆盖；任何 `AC-` 必须能回溯到至少一个 `F-`。完整性检查见 05 §9。

## 4. 术语与参考系统

| 缩写 | 全称 / 含义 |
| --- | --- |
| RBO / CBO | Rule-Based Optimizer / Cost-Based Optimizer |
| Memo / Group | Cascades 优化器中保存逻辑等价类与物理实现的搜索结构 |
| Cop / Coprocessor | 下推到存储节点执行的算子与表达式任务 |
| MPP | Massively Parallel Processing，本库指跨节点的并行执行片段 |
| Region / Raft Group | 一段连续 Key 范围及其多副本一致性组（对标 TiKV Region） |
| TSO | Timestamp Oracle，全局单调递增时间戳 |
| 2PC / Percolator | 两阶段提交 / Google 提出的分布式事务模型 |
| MVCC / SI / SSI | 多版本并发控制 / 快照隔离 / 可串行化快照隔离 |
| Chunk / Block | 向量化执行中列式的、定长批的数据单元（对标 Arrow RecordBatch） |
| Property / Enforcer | 物理属性（排序、分布、分区）与强制满足属性的算子（Sort / Exchange） |
| q-error | 基数估计误差度量：`max(est/act, act/est)` |
| NDV / CMSketch | Number of Distinct Values / Count-Min Sketch |
| HTAP | Hybrid Transactional / Analytical Processing |

**主要参考**：System R (Selinger et al., 1979)、Volcano (Graefe & McKenna, 1993)、Cascades (Graefe, 1995)、DPccp (Moerkotte & Neumann, 2006)、Percolator (Peng & Dabek, 2010)、Raft (Ongaro & Ousterhout, 2014)、Orca (Soliman et al., 2014)、Calcite、CockroachDB Optgen、TiDB Planner/Core、DuckDB、Apache DataFusion、HyPer、MonetDB/X100、SQLancer、TPC-H/TPC-DS/TPC-C、Jepsen。

## 5. 约定

- **语言与风格**：中文正文 + 英文术语首次出现括注；面向可验证性，避免无度量口径的形容词。
- **实现语言**：Rust（与仓库名一致），异步运行时 tokio，RPC 用 tonic/gRPC，单机引擎 RocksDB binding，列式内存格式自研 `Block`（对齐 Arrow 语义），不使用现成查询引擎作为内核（`DataFusion` 仅作对照基线，见 05）。
- **文档状态标记**：`[设计]` 已定稿、`[待标定]` 需要实验确定参数、`[研究向]` 开放问题 / 论文选题。
- **变更规则**：架构或接口变更需同时更新 01/02 与受影响功能点的追溯链；新增功能点必须补齐 `AC-` 覆盖后再进入排期。
