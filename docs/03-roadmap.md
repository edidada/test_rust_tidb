# 03 项目排期（Roadmap & Lab Plan）

> 状态：`[设计]` ｜ 版本：v0.1 ｜ 上游：01/02 ｜ 下游：04 功能点列表、05 验收规划
>
> 排期以 **Lab（实验）** 为基本推进单位，全部 `L0 … L15` 共 16 个 Lab、8 个里程碑 `M0 … M7`，总跨度 **W01–W60**（约 3 个学期，含缓冲与评审）。以 `2026-11-09` 作为 W01 起点示例，整体可平移。

---

## 1. 排期假设与原则

| 项 | 假设 |
| --- | --- |
| 人力 | 1 名博士（主）+ 0–2 名课程 Lab 学生（辅助测试/基准） |
| 周有效工时 | 25–30 h（扣除课程、会议、论文写作） |
| 起点 | 空仓库，仅本文档集；Rust 工具链已就绪 |
| 硬件 | 开发机 ≥16C/32G/NVMe；`cluster` 形态 3–8 节点（W42 起） |
| 外部依赖 | RocksDB / raft-rs / tonic 可用性风险已评估，见 §6 |

**四条原则**

1. **优化器与算子优先**：`L3 / L6 / L7 / L8 / L9 / L10` 共 26 周，占核心工期 ~50%，对应本项目主要贡献区（算子、执行计划、CBO/RBO）。
2. **每个 Lab 可独立验收**：DoD 明确到 05 的验收等级 `V0–V5` 与 `AC-*` 编号，未通过不进入下一 Lab（门禁见 §5）。
3. **正确性先于性能，可观测先于优化**：`EXPLAIN` / `OPTIMIZE_TRACE` / `EXPLAIN ANALYZE` 骨架在 L2 就埋点，L9 补齐，保证后续实验有数据可采。
4. **论文产出前置**：M3（W34）即达到第一个 *paper-ready* 节点，避免把写作压到最后（§7）。

---

## 2. 阶段总览

| Phase | 周次 | 主题 | 包含 Lab | 出口里程碑 |
| --- | --- | --- | --- | --- |
| P0 准备 | W01–W02 | 骨架、CI、实验设施、文档基线 | L0 | **M0** 设计冻结、CI 就绪 |
| P1 单机内核 | W03–W18 | 类型/解析/逻辑计划/物理算子/单机 KV | L1–L4 | **M1** 单机可跑 SQL，正确性基线 |
| P2 端到端与优化器 | W19–W37 | Server、RBO、统计/CE/代价、CBO、计划治理 | L5–L9 | **M2 / M3 / M4** |
| P3 执行与分布式 | W38–W51 | 向量化/Spill/并行、Raft/2PC、Cop/MPP、HTAP | L10–L13 | **M5** 分布式与 HTAP 可用 |
| P4 实验与论文 | W52–W60 | 基准、消融、混沌、可复现 artifact、研究拓展 | L14–L15 | **M6 / M7** |

```mermaid
gantt
    title LatticeDB 60 周排期（W01 = 2026-11-09，可平移）
    dateFormat YYYY-MM-DD
    axisFormat %y/%m
    tickInterval 3month
    excludes weekends

    section P0 准备
    L0 骨架/实验设施            :l0, 2026-11-09, 2w
    M0 设计冻结                 :milestone, m0, after l0, 0d

    section P1 单机内核
    L1 类型/表达式/Parser/Binder :l1, after l0, 4w
    L2 LogicalPlan/算子框架      :l2, after l1, 3w
    L3 物理算子全集(重点)        :crit, l3, after l2, 6w
    L4 单机KV/MVCC/事务          :l4, after l3, 3w
    M1 单机正确性基线            :milestone, m1, after l4, 0d

    section P2 端到端与优化器
    L5 Server/端到端SQL          :l5, after l4, 2w
    M2 端到端OLTP                :milestone, m2, after l5, 0d
    L6 RBO全规则集(重点)         :crit, l6, after l5, 4w
    L7 统计/CE/代价模型(重点)    :crit, l7, after l6, 4w
    L8 CBO: DP+Memo(核心)        :crit, l8, after l7, 6w
    M3 优化器完整(paper-ready)   :milestone, m3, after l8, 0d
    L9 执行计划治理(重点)        :crit, l9, after l8, 3w
    M4 计划可观测完整            :milestone, m4, after l9, 0d
    缓冲/评审                    :buf1, after l9, 1w

    section P3 执行与分布式
    L10 向量化/SIMD/JIT/Spill    :l10, after buf1, 4w
    L11 Region/Raft/TSO/2PC      :l11, after buf1, 4w
    L12 Cop下推/MPP/Exchange/AQE :l12, after l10 l11, 4w
    L13 列存HTAP(P1)             :l13, after l11, 3w
    M5 分布式与HTAP              :milestone, m5, after l12 l13, 0d

    section P4 实验与论文
    L14 基准/消融/混沌/可复现    :crit, l14, after m5, 4w
    M6 全量验收                  :milestone, m6, after l14, 0d
    L15 研究拓展(RQ)/论文收尾    :l15, after m6, 4w
    M7 结题                      :milestone, m7, after l15, 0d
```

> 图中 `crit` 为**关键路径**任务；`L10` 与 `L11` 并行、`L13` 与 `L12` 部分并行，`L4` 在 §5 中还可与 `L2/L3` 部分重叠以压缩工期——图为相对依赖示意，**精确周次以 §3 正文与 §9 映射表为准**。

---

## 3. Lab 详细清单（L0–L15）

> 字段说明：**依赖** 为前置 Lab；**覆盖** 为 04 功能域；**DoD** 引用 05 验收等级/验收项；**产出** 为可提交的 artifact。

### P0 准备

#### L0 · 项目骨架与实验设施（W01–W02，2 周） → M0
- **目标**：Cargo workspace、CI（fmt/clippy/test/coverage/静态依赖方向检查）、`Block` 基础工具、配置系统、tracing/metrics、`bench/env.toml` 规范、实验脚本模板与随机数种子策略。
- **依赖**：无 ｜ **覆盖**：`F-OBS-*`、`F-TEST-001..004`
- **交付物**：全部 crate 空壳 + `trait` 定义（02 §3）、CI 流水线、实验记录模板、本文档集 v0.1。
- **DoD**：`cargo test` 全绿；依赖方向检查生效；任一基准可重复跑 5 次并输出均值/方差（05 §7.2）。
- **产出**：M0 评审材料（设计冻结）。

### P1 单机内核（M1）

#### L1 · 类型系统、表达式、Parser、Binder（W03–W06，4 周）
- **目标**：`DataType/Scalar/Block/Column`、三值逻辑、`FuncRegistry`（含可下推/确定性/SIMD 元数据）、`Parser`（AST + 错误恢复）、`Binder`（名称解析/类型推导/隐式转换/权限）、内存 `Catalog`。
- **覆盖**：`F-TYP-*`、`F-SQL-001..006`、`F-TEST-005`
- **DoD**：`V1` 类型/表达式单元测试 100%；与 MySQL 对拍 500 条字面量与隐式转换用例（05 AC-TYP）。
- **风险**：Decimal/时间类型/Collation 语义是隐性工作量，预留 W06 缓冲。

#### L2 · 逻辑计划与算子框架（W07–W09，3 周）
- **目标**：`LogicalPlanNode` 全部 `OP-L-*` 节点、`Schema`、关系代数构造/遍历/等价比较、`EXPLAIN` 逻辑计划输出骨架、`PlanNodeId` 与 `rule_trace` 埋点（ADR-6）。
- **覆盖**：`OP-L-*`（04 §4.1）、`F-PLAN-002(部分)`
- **DoD**：`V1` 每个逻辑算子有构造/等价/`Schema` 推导测试；`EXPLAIN` 可读输出。
- **说明**：**先埋观测点**，避免 L6/L8 返工。

#### L3 · 物理算子全集（W10–W15，6 周）★重点
- **目标**：行式火山模型 + `Executor` trait，实现全部 `OP-P-*`：
  - W10 扫描/投影/过滤/TableDual/Limit；
  - W11 Join 四族（HashJoin / MergeJoin / IndexJoin / NLJ）+ Semi/Anti/Null-Aware Anti；
  - W12 HashAgg / StreamAgg / 两阶段 / Distinct / Union / Intersect / Except；
  - W13 Sort / TopN / 外排 / Window（含 frame）/ Materialize；
  - W14 DML 物理算子（Insert/Update/Delete/Replace）/ SelectLock / Analyze / Expand(GroupingSets)；
  - W15 算子级差分测试、NULL/空集/溢出/大数边界、算子统计埋点。
- **覆盖**：`OP-P-*`（04 §4.2）、`F-OP-008..020`、`F-TEST-006..010`
- **DoD**：`V2` 每算子正确性用例 100%；与 DuckDB/MySQL 结果对拍；`EXPLAIN ANALYZE` 雏形可出 actual_rows（05 AC-OP-001..020）。
- **产出**：算子基准基线与「行式」性能基线（供 L10 消融对比）。

#### L4 · 单机 KV、MVCC 与事务（W16–W18，3 周；与 L2/L3 部分重叠）
- **目标**：保序编码（02 §4）、`KvEngine` 的 `MemoryKV` 与 `RocksKV` 双实现、三 CF MVCC、快照读、ResolveLock、乐观 2PC 单机版、GC safe point。
- **覆盖**：`F-STO-001..010`、`F-TXN-001..008`
- **DoD**：`V2` MVCC 快照读/写冲突/锁解析用例；`V3` 并发写冲突注入（05 AC-TXN）。
- **说明**：`MemoryKV` 为 L3 提供无依赖单测底座。

**M1（W18）门禁**：单机形态可跑通 `SELECT/JOIN/AGG/INSERT/UPDATE/DELETE` 子集，sqllogictest 基础集 100%，`EXPLAIN` 可用。

### P2 端到端与优化器（核心）

#### L5 · Server 与端到端 SQL（W19–W20，2 周） → M2
- **目标**：MySQL wire 最小协议、Session/Prepared Statement、Fast Plan（PointGet/BatchPointGet）、结果流式返回、sqllogictest 完整集成、Slow Log。
- **覆盖**：`F-SQL-007..012`、`F-PLAN-001`、`F-OBS-*`
- **DoD**：`V2` 端到端 100 条 SQL；`mysql` 命令行可连；OLTP 点查 P99 基线记录（05 V2）。

#### L6 · RBO 规则引擎与全规则集（W21–W24，4 周）★重点
- **目标**：`Rule/Pattern/Matcher/RuleBatch/FixedPoint` 引擎 + 04 §5 全部 `R-*` 规则；重点：谓词下推边界、子查询去关联（Apply→Join）、外连接消除、Join 消除、分区裁剪、规则等价性测试与 `OPTIMIZE_TRACE`。
- **覆盖**：`F-RBO-*`、`R-*`
- **DoD**：`V2` 每条规则有等价性证明或反例测试；`V4` 规则可独立开关并留痕；「最小充分集」消融数据初版（05 AC-RBO，RQ-7）。
- **风险**：去关联与外连接语义是 bug 高发区，W24 留作回归周。

#### L7 · 统计信息、基数估计与代价模型（W25–W28，4 周）★重点
- **目标**：`ANALYZE` 采样、NDV/TopN/等高直方图/CMSketch/多列统计、`CardinalityEstimator` 选择性公式与误差传播、`CostModel` 全部算子代价公式、**代价常数标定方法**（05 §5.3）、q-error 监控看板。
- **覆盖**：`F-CBO-001..012`、`CB-*`
- **DoD**：`V4` q-error p95 ≤ 10（TPC-H）；代价与实际时间 Spearman 相关 ≥ 0.7（05 AC-CBO-001..006）。
- **说明**：本 Lab 是 CBO 质量的决定因素，先于 L8 且产出可直接写论文的 CE 误差分析（RQ-2 数据基础）。

#### L8 · CBO：搜索、Join Reorder 与物理属性（W29–W34，6 周）★★核心
- **目标**：
  - W29–W30 `System R` 自底向上 DP（left-deep / bushy / interesting orders）；
  - W31–W32 `Cascades Memo`（Group/GroupExpression/Winner、Implementation/Enforcer Rule、B&B 与支配剪枝）；
  - W33 `Join Reorder` 枚举（DPsize / DPsub / DPccp / Greedy / GEQO 兜底）+ 外连接/Semi/Anti 重排约束；
  - W34 分布式代价（Cop 收益、Broadcast/Shuffle 网络、倾斜惩罚）、双引擎对比实验、TPC-H 计划质量基线。
- **覆盖**：`F-CBO-008..014`、`CB-*`（04 §6）、`F-PLAN-005`
- **DoD**：`V4` TPC-H 22 查询几何平均加速 ≥ 2.0×（vs RBO 基线）；无灾难性计划（定义见 05 §5.1）；双引擎 Pareto 数据（RQ-1）。
- **产出**：**M3 第一个 paper-ready 节点**（优化器完整）。

#### L9 · 执行计划治理与可观测（W35–W37，3 周）★重点 → M4
- **目标**：`EXPLAIN` 全格式、`EXPLAIN ANALYZE`（actual_rows/耗时/内存/Spill/回流）、`OPTIMIZE_TRACE` / Memo Dump / 代价分解、Cop 可下推代价决策、`Exchange` 插入与 Fragment 切分、Plan Cache（参数化+失效）、Hint 体系、SPM/Baseline、Plan Replayer。
- **覆盖**：`F-PLAN-*` 全量、`F-SQL-013`
- **DoD**：`V4` 计划可回放、下推开/关结果一致、缓存命中正确性、SPM 防回归（05 AC-PLAN-*）。
- **缓冲**：W38 为 P2 评审/缓冲周（图 `buf1`）。

**M3（W34）/ M4（W37）门禁**：优化器与计划治理完整，具备完整实验观测能力；未达标则 P3 并行任务先行、优化器继续冲刺。

### P3 执行与分布式（M5）

#### L10 · 向量化、SIMD、JIT、Spill 与并行（W39–W42，4 周）★算子强化
- **目标**：`Block/Chunk` 列式执行路径、`SelectionVector` 延迟物化、SIMD 表达式、可选 Cranelift JIT、HashJoin/Agg/Sort 的 Spill 与 Grace Hash Join、`MemoryTracker`、Morsel-driven 并行。
- **覆盖**：`OP-X-*`（04 §4.4）、`F-OP-021..026`、`F-EXEC-*`
- **DoD**：`V4` 列式 vs 行式 ≥ 3×；OOM 降级无错无泄漏；逐层消融 + roofline 归因（RQ-6，05 §5.4）。

#### L11 · 分布式存储：Region / Raft / TSO / 2PC（W39–W42，4 周，与 L10 并行）
- **目标**：raft-rs 集成、Region 分裂/合并、Leader Lease/ReadIndex、TSO 批量分配与 HA、Region 路由缓存与重试、分布式 2PC、悲观事务、1PC 优化、DDL schema 版本与 lease。
- **覆盖**：`F-STO-011..024`、`F-TXN-009..016`、`F-META-*`、`F-DDL-*`
- **DoD**：`V3` Leader 切换 RTO ≤ 10s / RPO = 0；`V3` 2PC 异常注入全覆盖；`V5` 线性一致性（Porcupine）（05 AC-TXN / AC-STO）。

#### L12 · Coprocessor 下推、MPP 与自适应执行（W43–W46，4 周）
- **目标**：`DagRequest` 协议与存储侧向量化求值（复用 `expr`）、下推决策落地、`ExchangeSender/Receiver`、Hash/Range/Broadcast Shuffle、MPP Task 调度、Runtime Filter / Bloom、Adaptive Join、倾斜处理。
- **覆盖**：`F-EXEC-010..022`、`OP-E-*`（04 §4.3）、`F-PLAN-011`
- **DoD**：`V3` 下推开/关结果一致；`V4` 4 节点 MPP 加速 ≥ 2.8×；倾斜数据集 AQE 消融（RQ-4，05 §5.5）。

#### L13 · 列存 HTAP 与行列自动选择（W47–W49，3 周，P1）
- **目标**：Raft Learner 异步复制、`resolved_ts` 一致性读、列式段存 + Delta、列存扫描、行列代价统一建模与 `READ_FROM_STORAGE`、列存 MPP。
- **覆盖**：`F-HTAP-*`
- **DoD**：`V4` 行列混合计划正确；freshness–latency 曲线（RQ-5）；OLTP 提交延迟不受列存同步影响（05 §5.6）。

**M5（W49）门禁**：`cluster` 形态可用，分布式正确性/一致性/性能三项达标。

### P4 实验与论文（M6 / M7）

#### L14 · 基准体系、消融、混沌与可复现（W50–W53，4 周）
- **目标**：TPC-H/TPC-DS/TPC-C/sysbench/YCSB 全套脚本；SQLancer 风格差分/模糊测试（TLP/NoREC/PQS）；Jepsen 风格故障注入（kill/分区/时钟漂移/磁盘满）；72h 稳定性；**可复现 artifact**（容器镜像、固定种子、≥5 次重复、置信区间、基线对比、实验元数据归档）。
- **覆盖**：`F-BENCH-*`、`F-TEST-011..020`、`F-OBS-*`
- **DoD**：`V5` 全量验收矩阵 100%；所有论文图表可一键重跑且偏差 ≤ 5%（05 §7）。

#### L15 · 研究拓展与论文收尾（W54–W60，7 周，含 2 周缓冲） → M7
- **目标**：从 RQ-1..7 选 1–2 个深入（建议 RQ-1/RQ-2 为主，RQ-3 学习式估计为拓展）；补充实验、论文图表、技术报告、结题评审与文档定稿。
- **覆盖**：`F-RSH-*`
- **DoD**：`V5` 研究假设被证实或证伪并有量化结论；文档、代码、实验三者版本一致。

---

## 4. 里程碑与门禁（M0–M7）

| 里程碑 | 周次 | 名称 | 门禁（必须全部满足） | 关联验收 |
| --- | --- | --- | --- | --- |
| **M0** | W02 | 设计冻结 | 文档 v0.1 评审通过；CI/依赖方向/实验模板就绪 | 05 §9 追溯矩阵闭合 |
| **M1** | W18 | 单机正确性基线 | sqllogictest 基础集 100%；全算子单测 100%；MVCC 用例通过 | V1–V3 |
| **M2** | W20 | 端到端 OLTP | mysql 客户端可连；Prepared/Fast Plan 可用；OLTP P99 基线入档 | V2 |
| **M3** | W34 | 优化器完整（**paper-ready #1**） | RBO 全规则 + CBO 双引擎；q-error 达标；TPC-H 加速 ≥ 2× | V4 / AC-CBO / AC-RBO |
| **M4** | W37 | 计划可观测完整 | EXPLAIN ANALYZE / Trace / Replayer / SPM / Plan Cache 全通 | V4 / AC-PLAN |
| **M5** | W49 | 分布式与 HTAP | 一致性/故障注入/MPP 加速/行列选择达标 | V3–V4 |
| **M6** | W53 | 全量验收 | 05 验收矩阵 100%，可复现 artifact 评审通过 | V5 |
| **M7** | W60 | 结题 | 论文/技术报告 + 文档/代码/实验版本一致 | V5 |

---

## 5. 关键路径与并行策略

**关键路径（crit）**：
`L0 → L1 → L2 → L3 → L5 → L6 → L7 → L8 → L9 → L14 → L15`

任何一环延期 1 周，整体延期 1 周（缓冲见 §6）。其中 **L3 → L6 → L7 → L8 → L9** 是「算子 → 规则 → 统计/代价 → 搜索 → 计划治理」的主链，不可压缩。

**可并行/可压缩**：
- `L4` 与 `L2/L3` 重叠（`MemoryKV` 先交付给 L3 做单测底座）。
- `L10`（向量化）与 `L11`（分布式存储）完全并行，二者无接口依赖，汇合于 `L12`。
- `L13`（HTAP）依赖 `L11`，可与 `L12` 部分重叠。
- `L14` 的基准脚本可从 `L5` 起**增量编写**（每个 Lab 顺手补用例），避免 W50 集中爆发。

**最短可验收路径（若只有 1 学期 / 16 周）**：`L0 → L1 → L2 → L3(裁剪) → L5 → L6(核心规则) → L7(默认统计) → L8(仅 DP)` → 交付 M1 + 优化器最小可跑版本，其余 Lab 顺延。

---

## 6. 风险登记与缓冲

| # | 风险 | 概率 | 影响 | 缓解 | 缓冲 |
| --- | --- | --- | --- | --- | --- |
| RK-1 | RBO 去关联 / 外连接语义缺陷导致正确性长期回归 | 高 | 高 | L2 先建计划等价比较器；L6 每周回归；去关联单列 W24 | W24 + W38 |
| RK-2 | 代价常数标定不稳，CBO 出现灾难性计划 | 中 | 高 | L7 先做相关性验证；SPM(L9) 兜底；保留 RBO 回退开关 | W38 |
| RK-3 | raft-rs / RocksDB binding 在 Windows 开发环境编译问题 | 中 | 中 | W16 起统一用 Linux 容器做 `cluster` 实验；CI 用 Linux runner | W42 |
| RK-4 | 分布式正确性（2PC/Region epoch）调试周期超预期 | 高 | 高 | L11 先做确定性模拟（deterministic simulator）再上真网络 | W49 前 1 周 |
| RK-5 | 性能实验环境波动导致结论不可复现 | 中 | 高 | 环境固化 `bench/env.toml` + 固定种子 + ≥5 次重复 + 置信区间（05 §7） | W53 前 1 周 |
| RK-6 | 范围蔓延（P2 功能挤占核心研究） | 中 | 中 | 04 的 P2 项默认不做，进入排期需评审；核心链优先 | 全程 |
| RK-7 | 论文写作挤压实现 | 中 | 中 | M3 即产出第一篇实验数据；L15 预留 2 周纯写作 | W59–W60 |

**缓冲总量**：W24、W38、W47（隐含）、W59–W60 共约 6 周，分布在关键路径高风险段之后。

---

## 7. 研究/论文产出点（博士）

| 时点 | 可产出成果 | 数据来自 | 对应 RQ |
| --- | --- | --- | --- |
| M3（W34） | 「分布式代价下 DP vs Cascades Memo 的规划时间–计划质量 Pareto」技术报告 / 短文 | L8 双引擎对比 | RQ-1 |
| M4（W37） | 「优化器规则最小充分集」消融研究 | L6 逐规则开关 | RQ-7 |
| M5（W49） | 「Runtime Filter / AQE 对倾斜的收益上界」 | L12 倾斜消融 | RQ-4 |
| M6（W53） | 「基数估计误差传播与次优计划概率」 | L7 + L14 q-error 全量 | RQ-2 |
| M7（W60） | 主论文 + 可复现 artifact（镜像/脚本/数据集/元数据） | L14/L15 | RQ-1/2/3 |

**实验准入**：任何进入论文的结论必须满足 05 §7 的可复现规范（同基线、同数据集、同环境、置信区间、消融对照），禁止跨系统裸比。

---

## 8. 评审与节奏

| 节奏 | 频率 | 内容 |
| --- | --- | --- |
| 周 | 每周 | Lab 进度、CI 状态、失败用例清单、下周目标 |
| 双周 | 每 2 周 | 优化器计划质量回归（TPC-H plan diff）、q-error 趋势 |
| 里程碑 | M0–M7 | 门禁评审：DoD 逐条核验 + 05 追溯矩阵抽查 + 风险复评 |
| 季度 | 每 ~13 周 | 范围裁剪评审（P2 是否进入）、论文进度、排期重估 |

---

## 9. Lab ↔ 功能域 ↔ 验收 映射总表

| Lab | 周次 | 主功能域（04） | 主验收项（05） | 里程碑 |
| --- | --- | --- | --- | --- |
| L0 | W01–02 | OBS / TEST | AC-TEST-001, §7 可复现规范 | M0 |
| L1 | W03–06 | TYP / SQL | AC-TYP-* | — |
| L2 | W07–09 | OP-L / PLAN | AC-OP-001..006, AC-PLAN-002 | M1 |
| L3 | W10–15 | OP-P / TEST | AC-OP-001..020 | M1 |
| L4 | W16–18 | STO / TXN | AC-STO-001..010, AC-TXN-001..008 | M1 |
| L5 | W19–20 | SQL / PLAN / OBS | AC-SQL-*, AC-PLAN-001 | M2 |
| L6 | W21–24 | RBO | AC-RBO-* | — |
| L7 | W25–28 | CBO(STAT/CE/COST) | AC-CBO-001..006 | — |
| L8 | W29–34 | CBO(SEARCH/JOIN/PROP) | AC-CBO-007..015 | M3 |
| L9 | W35–37 | PLAN 全量 | AC-PLAN-002..010 | M4 |
| L10 | W39–42 | OP-X / EXEC | AC-OP-021..026, AC-EXEC-* | — |
| L11 | W39–42 | STO / TXN / META / DDL | AC-STO-*, AC-TXN-* | — |
| L12 | W43–46 | EXEC(分布式) | AC-EXEC-*, AC-PLAN-011 | — |
| L13 | W47–49 | HTAP | AC-HTAP-* | M5 |
| L14 | W50–53 | BENCH / TEST / OBS | V5 全矩阵 | M6 |
| L15 | W54–60 | RSH | RQ-* 结论 | M7 |
