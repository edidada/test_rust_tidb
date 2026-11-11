# 04 功能点列表（Feature List / Requirements Baseline）

> 状态：`[设计]` ｜ 版本：v0.1 ｜ 上游：01/02/03 ｜ 下游：05 验收规划
>
> 本文是**需求基线**：凡未在此登记的能力不进入排期；凡登记项必须被 05 的至少一条 `AC-*` 覆盖（追溯规则见 §9）。**算子、执行计划、RBO、CBO 四块单独给出全量枚举（§4–§7）**，其余按 19 个功能域枚举（§8）。

---

## 1. 编号与映射约定

| 编号族 | 含义 | 枚举位置 |
| --- | --- | --- |
| `OP-L-*` / `OP-P-*` / `OP-E-*` / `OP-X-*` | 逻辑算子 / 物理算子 / 交换与分布式算子 / 表达式与向量化原语 | §4 |
| `R-*` | RBO 规则 | §5 |
| `CB-*` | CBO 能力点 | §6 |
| `PL-*` | 执行计划能力点 | §7 |
| `F-<域>-NNN` | 功能点，`域` 见 §2 | §8 |
| `AC-*` | 验收项 | 05 |
| `L*` / `M*` / `W*` | Lab / 里程碑 / 周次 | 03 |

**算子双重编号**：每个 `OP-*` 在 §4 表格末列标注其归属的 `F-OP-NNN` 组；验收按 `AC-OP-*` 逐类覆盖。`F-OP-*` 只登记算子**框架与横切能力**，不重复枚举单个算子。

---

## 2. 功能域总览（19 域）

| 域 | 名称 | 功能点数（§8） | 优先级主轴 | 主 Lab |
| --- | --- | --- | --- | --- |
| SQL | 协议、会话、解析、绑定 | 18 | P0 | L1, L5 |
| TYP | 类型系统与表达式 | 18 | P0 | L1 |
| DDL | DDL / Catalog / Schema 变更 | 15 | P0/P1 | L11 |
| TXN | 事务与并发控制 | 20 | P0 | L4, L11 |
| OP | 算子框架与横切能力 | 26 | P0 | L2, L3, L10 |
| RBO | 规则优化（框架项，逐规则见 §5） | 12 | P0 | L6 |
| CBO | 代价优化（框架项，逐能力见 §6） | 14 | P0 | L7, L8 |
| PLAN | 执行计划治理（框架项，逐能力见 §7） | 12 | P0 | L2, L9 |
| EXEC | 执行引擎与分布式执行 | 22 | P0/P1 | L10, L12 |
| STO | 存储层（KV/MVCC/Raft/Cop） | 24 | P0 | L4, L11 |
| META | 元数据与调度 | 13 | P0/P1 | L11 |
| IDX | 索引 | 15 | P0/P1 | L2, L9 |
| PART | 分区 | 11 | P1 | L6, L9 |
| HTAP | 列存与行列混合 | 11 | P1 | L13 |
| OBS | 可观测性与诊断 | 15 | P0/P1 | L0, L9 |
| TEST | 测试与正确性验证 | 21 | P0 | 全阶段 |
| BENCH | 基准与性能工程 | 16 | P0/P1 | L14 |
| ECO | 生态与兼容 | 15 | P1/P2 | L5+ |
| RSH | 研究向拓展 | 14 | P2 / `[研究向]` | L15 |

**合计**：§8 共 **312** 项功能点；§4–§7 另含算子/规则/能力点 **232** 项（`OP-*` 115、`R-*` 46、`CB-*` 47、`PL-*` 24）。全表合计约 540 个可追溯条目。

**优先级**：`P0` 核心必做（M1–M5 门禁依赖）｜`P1` 重要（M5/M6 依赖）｜`P2` 拓展 / `[研究向]`（不进主关键路径，进入排期需评审，见 03 RK-6）。

---

## 3. 优先级与状态字段定义

每条功能点形如：`| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |`

- **依赖**：前置编号（`—` 表示无）。**验收**：05 中 `AC-*` 编号或 `V*` 等级。
- **状态**：全部为 `[设计]`；实现阶段在本表追加「实现状态」列（`未开始/进行中/已验收`），不改变编号。

---

## 4. 算子体系全量清单（★核心 ①）

### 4.1 逻辑算子 `OP-L-*`（28 项，归属 `F-OP-001..006`）

| 编号 | 逻辑算子 | 说明 / 变体 | 优先级 | Lab | 验收 | F-OP 组 |
| --- | --- | --- | --- | --- | --- | --- |
| OP-L-DATASOURCE | 数据源 / 逻辑表扫描 | 表、视图、CTE 引用、分区展开前形态 | P0 | L2 | AC-OP-001 | 001 |
| OP-L-PROJECTION | 投影 | 表达式列表、列引用、别名 | P0 | L2 | AC-OP-001 | 001 |
| OP-L-SELECTION | 选择 | 谓词（Bool3 语义） | P0 | L2 | AC-OP-001 | 001 |
| OP-L-JOIN-INNER | 内连接 | 等值 / 非等值 / 自然连接 | P0 | L2 | AC-OP-002 | 002 |
| OP-L-JOIN-OUTER | 外连接 | LEFT / RIGHT / FULL | P0 | L2 | AC-OP-002 | 002 |
| OP-L-JOIN-SEMI | 半连接 / 反半连接 | Semi / AntiSemi / LeftOuterSemi / AntiLeftOuterSemi | P0 | L2 | AC-OP-002 | 002 |
| OP-L-JOIN-CROSS | 交叉连接 | 笛卡尔积（需显式上界保护） | P0 | L2 | AC-OP-002 | 002 |
| OP-L-APPLY | Apply（相关子查询） | 逐行绑定，去关联的输入形态 | P0 | L2 | AC-OP-003 | 003 |
| OP-L-LATERAL | Lateral / 横向派生表 | P2 | P2 | L6 | AC-OP-003 | 003 |
| OP-L-AGG | 聚合 | GROUP BY / 标量聚合 / DISTINCT 聚合 / FILTER 子句 | P0 | L2 | AC-OP-004 | 004 |
| OP-L-EXPAND | 分组集展开 | GROUPING SETS / ROLLUP / CUBE | P1 | L2 | AC-OP-004 | 004 |
| OP-L-WINDOW | 窗口 | 分区 / 排序 / frame（ROWS/RANGE/GROUPS） | P1 | L2 | AC-OP-005 | 005 |
| OP-L-SORT | 排序 | 多键、NULLS FIRST/LAST、Collation | P0 | L2 | AC-OP-005 | 005 |
| OP-L-TOPN | TopN | ORDER BY + LIMIT 融合 | P0 | L2 | AC-OP-005 | 005 |
| OP-L-LIMIT | 限制 | OFFSET / LIMIT，参数化 | P0 | L2 | AC-OP-005 | 005 |
| OP-L-DISTINCT | 去重 | DISTINCT / DISTINCT ON(P2) | P0 | L2 | AC-OP-004 | 004 |
| OP-L-UNION | 集合并 | UNION / UNION ALL | P0 | L2 | AC-OP-006 | 006 |
| OP-L-INTERSECT | 集合交 | INTERSECT [ALL] | P1 | L2 | AC-OP-006 | 006 |
| OP-L-EXCEPT | 集合差 | EXCEPT [ALL] | P1 | L2 | AC-OP-006 | 006 |
| OP-L-CTE | CTE | 非递归 / 递归 WITH、内联或物化决策 | P1 | L2 | AC-OP-006 | 006 |
| OP-L-PARTUNION | 分区并 | 分区表裁剪后的逻辑并 | P1 | L6 | AC-OP-006 | 006 |
| OP-L-SEQUENCE | 序列 / 多语句计划 | DML 依赖顺序 | P1 | L2 | AC-OP-006 | 006 |
| OP-L-DUAL | 虚表 | 无 FROM 查询、常量查询 | P0 | L2 | AC-OP-001 | 001 |
| OP-L-MAXONEROW | 标量子查询护栏 | 结果 >1 行报错 | P0 | L2 | AC-OP-003 | 003 |
| OP-L-LOCK | 行锁逻辑 | SELECT … FOR UPDATE / SHARE | P0 | L3 | AC-OP-007 | 007 |
| OP-L-DML | DML 计划 | INSERT / UPDATE / DELETE / REPLACE / ON DUPLICATE | P0 | L3 | AC-OP-007 | 007 |
| OP-L-DDL / OP-L-ANALYZE / OP-L-EXPLAIN | 管理类逻辑算子 | DDL、统计收集、EXPLAIN | P1 | L5 | AC-OP-007 | 007 |
| OP-L-MEMOGROUP | Memo 组引用 | 仅 CBO 内部（等价类占位） | P0 | L8 | AC-CBO-009 | 008 |

### 4.2 物理算子 `OP-P-*`（51 项，归属 `F-OP-008..020`）

**扫描类**

| 编号 | 算子 | 关键点 | 优先级 | Lab | 验收 | F-OP |
| --- | --- | --- | --- | --- | --- | --- |
| OP-P-TABLESCAN | 全表扫描 | range 构造、列裁剪、Cop 可下推 | P0 | L3 | AC-OP-008 | 008 |
| OP-P-ROWIDSCAN | RowID/Handle 回表扫描 | BatchGet 合并、随机 IO 建模 | P0 | L3 | AC-OP-008 | 008 |
| OP-P-INDEXSCAN | 索引范围扫描 | 最左前缀、range 切分、ICP | P0 | L3 | AC-OP-008 | 008 |
| OP-P-INDEXONLY | 覆盖索引扫描 | 无需回表 | P0 | L3 | AC-OP-008 | 008 |
| OP-P-INDEXLOOKUP | 索引回表 | IndexScan + RowIDScan 双读 | P0 | L3 | AC-OP-008 | 008 |
| OP-P-INDEXMERGE | 索引合并 | 多索引交/并 + 回表 | P1 | L9 | AC-OP-008 | 008 |
| OP-P-POINTGET | 点查 | 主键/唯一键单 Key，Fast Plan | P0 | L5 | AC-OP-009 | 009 |
| OP-P-BATCHPOINTGET | 批量点查 | 去重、排序、按 Region 分组 | P0 | L5 | AC-OP-009 | 009 |
| OP-P-TABLESAMPLE | 采样扫描 | Reservoir / SYSTEM 采样，固定种子 | P1 | L7 | AC-OP-009 | 009 |
| OP-P-COLUMNARSCAN | 列存扫描 | ZoneMap 过滤、延迟物化 | P1 | L13 | AC-OP-009 | 009 |
| OP-P-CTESCAN / OP-P-MEMTABLE | CTE / 内存表扫描 | 物化复用、information_schema | P1 | L3 | AC-OP-009 | 009 |
| OP-P-UNIONSCAN | 读己之写 | 事务内缓冲与快照合并 | P0 | L4 | AC-OP-009 | 009 |

**连接类**

| 编号 | 算子 | 关键点 | 优先级 | Lab | 验收 | F-OP |
| --- | --- | --- | --- | --- | --- | --- |
| OP-P-HASHJOIN | Hash Join | Build/Probe、选择率驱动 Build 侧、哈希冲突 | P0 | L3 | AC-OP-010 | 010 |
| OP-P-GRACEHJ | Grace / 可溢写 Hash Join | 动态分区、递归分区、Spill IO 计量 | P0 | L10 | AC-OP-021 | 021 |
| OP-P-MERGEJOIN | Sort Merge Join | 复用 interesting order、多键、NULL | P0 | L3 | AC-OP-010 | 010 |
| OP-P-NLJ | Nested Loop Join | 非等值 / 小表 | P0 | L3 | AC-OP-010 | 010 |
| OP-P-BLOCKNLJ | Block NLJ | 块化减少内层扫描 | P1 | L10 | AC-OP-010 | 010 |
| OP-P-INDEXJOIN | Index Join | 外表驱动索引点查、回表代价 | P0 | L3 | AC-OP-011 | 011 |
| OP-P-INDEXHASHJOIN | Index Hash Join | 批量 key 预取 | P1 | L10 | AC-OP-011 | 011 |
| OP-P-BROADCASTHJ | 广播 Hash Join | 小表广播、阈值与网络代价 | P0 | L12 | AC-OP-012 | 012 |
| OP-P-SHUFFLEHJ | Shuffle Hash Join | 双大表按 join key 重分区 | P0 | L12 | AC-OP-012 | 012 |
| OP-P-CARTESIAN | 笛卡尔积 | 显式上界保护、资源限额 | P0 | L3 | AC-OP-010 | 010 |
| OP-P-SEMIJOIN | Semi / Anti Semi Join | 去重语义、提前终止 | P0 | L3 | AC-OP-013 | 013 |
| OP-P-NAAJ | Null-Aware Anti Join | `NOT IN` + NULL 三值正确性 | P0 | L3 | AC-OP-013 | 013 |
| OP-P-OUTERSEMI | Left Outer Semi Join | 子查询产生 NULL 列 | P1 | L3 | AC-OP-013 | 013 |
| OP-P-APPLY | Apply 物理执行 | 未去关联时的逐行执行（正确性兜底） | P0 | L3 | AC-OP-003 | 003 |
| OP-P-RUNTIMEFILTER | Runtime Filter | Bloom / In / Min-Max，构建端下推到 Scan | P1 | L12 | AC-OP-022 | 022 |
| OP-P-ADAPTIVEJOIN | 自适应连接 | 运行时按实际基数切换实现 | P2 | L12 | AC-OP-022 | 022 |

**聚合 / 排序 / 集合 / 窗口**

| 编号 | 算子 | 关键点 | 优先级 | Lab | 验收 | F-OP |
| --- | --- | --- | --- | --- | --- | --- |
| OP-P-HASHAGG | Hash 聚合 | 分组哈希表、内存跟踪 | P0 | L3 | AC-OP-014 | 014 |
| OP-P-STREAMAGG | 流式聚合 | 需有序输入、低内存 | P0 | L3 | AC-OP-014 | 014 |
| OP-P-TWOPHASEAGG | 两阶段聚合 | Partial(Cop/局部) + Final，分布式必备 | P0 | L12 | AC-OP-014 | 014 |
| OP-P-SPILLAGG | 可溢写聚合 | 分区溢写、降级路径 | P0 | L10 | AC-OP-021 | 021 |
| OP-P-SCALARAGG | 标量聚合 | 空集返回 `NULL`/`0` 语义 | P0 | L3 | AC-OP-014 | 014 |
| OP-P-DISTINCTAGG | DISTINCT 聚合 | 多 distinct 兼容 | P1 | L3 | AC-OP-014 | 014 |
| OP-P-APPROXDISTINCT | 近似去重 | HLL/APPROX_COUNT_DISTINCT | P2 | L7 | AC-OP-014 | 014 |
| OP-P-SORT | 内存排序 | 多键、Collation、NULL 序 | P0 | L3 | AC-OP-015 | 015 |
| OP-P-EXTERNALSORT | 外部归并排序 | 归并路数、Spill | P0 | L10 | AC-OP-021 | 021 |
| OP-P-TOPN | TopN 堆 | 堆大小 = LIMIT，避免全排 | P0 | L3 | AC-OP-015 | 015 |
| OP-P-LIMIT | Limit / Offset | 提前终止、下推 | P0 | L3 | AC-OP-015 | 015 |
| OP-P-MERGESORT | 归并 | 多路有序输入（Exchange 后） | P0 | L12 | AC-OP-015 | 015 |
| OP-P-UNIONALL / OP-P-UNIONDISTINCT | 集合并 | 并发 gather / 去重 | P0 | L3 | AC-OP-016 | 016 |
| OP-P-INTERSECT / OP-P-EXCEPT | 集合交差 | 基于哈希/排序实现 | P1 | L3 | AC-OP-016 | 016 |
| OP-P-WINDOW | 窗口 | 分区 + 排序 + frame 流式计算 | P1 | L3 | AC-OP-017 | 017 |
| OP-P-MATERIALIZE | 物化 | Chunk 缓冲、Spill、复用 | P0 | L3 | AC-OP-018 | 018 |

**DML 与辅助**

| 编号 | 算子 | 关键点 | 优先级 | Lab | 验收 | F-OP |
| --- | --- | --- | --- | --- | --- | --- |
| OP-P-INSERT / OP-P-REPLACE | 插入 | 主键冲突、自增、默认值 | P0 | L3 | AC-OP-019 | 019 |
| OP-P-UPDATE / OP-P-DELETE | 更新删除 | 基于 handle、索引维护 | P0 | L3 | AC-OP-019 | 019 |
| OP-P-SELECTLOCK | 加锁 | 悲观锁、`FOR UPDATE` | P0 | L11 | AC-OP-019 | 019 |
| OP-P-TABLEDUAL / OP-P-MAXONEROW | 辅助 | 常量 / 标量护栏 | P0 | L2 | AC-OP-001 | 001 |
| OP-P-ANALYZE / OP-P-CHECKTABLE / OP-P-SPLITREGION | 管理算子 | 统计、校验、Region 运维 | P1 | L7/L11 | AC-OP-020 | 020 |
| OP-P-SHOW / OP-P-TRACE / OP-P-EXPLAIN | 诊断算子 | 元数据查询与计划输出 | P1 | L5/L9 | AC-OP-020 | 020 |
| OP-P-LOADDATA | 批量导入 | 流式解析 + 批量写入 | P2 | L14 | AC-OP-020 | 020 |

### 4.3 交换与分布式算子 `OP-E-*`（18 项，归属 `F-EXEC-010..020`）

| 编号 | 算子 | 说明 | 优先级 | Lab | 验收 |
| --- | --- | --- | --- | --- | --- |
| OP-E-SENDER / OP-E-RECEIVER | 交换收发 | 序列化 Block 流、有界队列 + 背压 | P0 | L12 | AC-EXEC-010 |
| OP-E-HASHSHUFFLE | 哈希重分区 | 按 key 一致性哈希，倾斜感知 | P0 | L12 | AC-EXEC-011 |
| OP-E-RANGESHUFFLE | 范围重分区 | 有序性保持、边界切分 | P1 | L12 | AC-EXEC-011 |
| OP-E-ROUNDROBIN | 轮询重分区 | 无 key 负载均衡 | P1 | L12 | AC-EXEC-011 |
| OP-E-BROADCAST | 广播 | 阈值控制、副本数 | P0 | L12 | AC-EXEC-011 |
| OP-E-GATHER | 汇聚 | 多 Fragment → 单点 | P0 | L12 | AC-EXEC-010 |
| OP-E-MERGESORTEX | 有序汇聚 | 归并维持 ordering property | P0 | L12 | AC-EXEC-011 |
| OP-E-PASSTHROUGH | 直通 | 同节点 Fragment 免网络 | P1 | L12 | AC-EXEC-010 |
| OP-E-REPARTITION | 重分区 | 运行时并行度调整 | P2 | L12 | AC-EXEC-011 |
| OP-E-TABLEREADER | 表读取封装 | Cop Task → SQL 层 | P0 | L9 | AC-PLAN-005 |
| OP-E-INDEXREADER | 索引读取封装 | 覆盖索引场景 | P0 | L9 | AC-PLAN-005 |
| OP-E-INDEXLOOKUPREADER | 回表读取封装 | 两阶段 Cop Task | P0 | L9 | AC-PLAN-005 |
| OP-E-COPTASK | Coprocessor Task | 按 Region 切分、epoch 绑定、重试幂等 | P0 | L12 | AC-EXEC-012 |
| OP-E-MPPTASK | MPP Task | 列存/行存分布式片段 | P1 | L12 | AC-EXEC-013 |
| OP-E-DISPATCHER | 任务调度 | Fragment → 节点放置、失败重发 | P0 | L12 | AC-EXEC-013 |
| OP-E-FRAGMENT | 计划片段 | 并行度、资源组、超时 | P0 | L9 | AC-PLAN-005 |
| OP-E-PIPELINE | 流水线 | 算子间推送式 Chunk 流（可选） | P2 | L10 | AC-EXEC-014 |
| OP-E-BACKPRESSURE | 背压与取消 | CancellationToken 贯穿 RPC | P0 | L12 | AC-EXEC-014 |

### 4.4 表达式与向量化原语 `OP-X-*`（18 项，归属 `F-OP-022..026`）

| 编号 | 原语 | 说明 | 优先级 | Lab | 验收 |
| --- | --- | --- | --- | --- | --- |
| OP-X-BLOCK | Chunk / Block | 列式批 + 容量可配 | P0 | L1/L10 | AC-OP-023 |
| OP-X-VALIDITY | NULL 位图 | 三值逻辑向量化表示 | P0 | L1 | AC-OP-023 |
| OP-X-SELVECTOR | SelectionVector | 延迟物化、链式过滤 | P0 | L10 | AC-OP-023 |
| OP-X-VECEVAL | 向量化表达式求值 | 按列批量、无虚调用热循环 | P0 | L10 | AC-OP-023 |
| OP-X-SIMD | SIMD 谓词 | 比较 / 算术 / LIKE 前缀 | P1 | L10 | AC-OP-024 |
| OP-X-JIT | 表达式 JIT | Cranelift 编译（可开关） | P2 | L10 | AC-OP-024 |
| OP-X-CONSTFOLD | 常量折叠 | 绑定期完成 | P0 | L6 | AC-RBO-001 |
| OP-X-CSE | 公共子表达式消除 | 表达式级去重 | P1 | L6 | AC-RBO-001 |
| OP-X-REGISTRY | 函数注册表 | 确定性 / 可下推 / 代价 / SIMD 元数据 | P0 | L1 | AC-TYP-006 |
| OP-X-AGGFUNC | 聚合函数集 | SUM/COUNT/AVG/MIN/MAX/STDDEV/GROUP_CONCAT… | P0 | L3 | AC-OP-014 |
| OP-X-WINFUNC | 窗口函数集 | ROW_NUMBER/RANK/DENSE_RANK/LEAD/LAG/NTILE/聚合 OVER | P1 | L3 | AC-OP-017 |
| OP-X-DECIMAL | Decimal 内核 | 精度/溢出/除零语义 | P0 | L1 | AC-TYP-003 |
| OP-X-COLLATION | 排序规则 | 比较与排序一致性（上下层对齐） | P1 | L1 | AC-TYP-007 |
| OP-X-DICT | 字典编码 | 低基数列压缩、聚合加速 | P2 | L10 | AC-OP-024 |
| OP-X-ARROW | Arrow 互操作 | 导入导出 / 基线对比 | P2 | L14 | AC-BENCH-008 |
| OP-X-ZEROCOPY | 零拷贝切片 | `Bytes` / buffer slice 免复制 | P0 | L10 | AC-OP-023 |
| OP-X-EXPRCACHE | 表达式结果缓存 | 同一 Block 内复用 | P2 | L10 | AC-OP-024 |
| OP-X-CODEGENFALLBACK | JIT 回退 | 不支持表达式回落解释执行 | P2 | L10 | AC-OP-024 |

---

## 5. RBO 规则集全量清单 `R-*`（★核心 ②，46 项，归属 `F-RBO-*` 与 `F-OP-022`）

> 每条规则必须附「关系代数等价 + bag 语义 + NULL 三值逻辑」证明或反例测试（05 AC-RBO-001）。全部规则可独立开关并留痕（ADR-6，`F-RBO-003`）。

| 编号 | 规则 | 类别 | 关键约束 / 边界 | 优先级 | Lab | 验收 |
| --- | --- | --- | --- | --- | --- | --- |
| R-CF | 常量折叠 | 化简 | 非确定性函数（`NOW/RAND`）需先特化 | P0 | L6 | AC-RBO-001 |
| R-CP | 常量传播 / 等值传递闭包 | 化简 | `a=1 AND a=b → b=1` | P0 | L6 | AC-RBO-001 |
| R-SIMPLIFY | 表达式化简 | 化简 | 恒真/恒假谓词、冗余 CAST、代数化简 | P0 | L6 | AC-RBO-001 |
| R-PRED-DEDUCE | 谓词推导 | 化简 | 由比较推导范围闭包，用于分区裁剪 | P1 | L6 | AC-RBO-002 |
| R-COLPRUNE | 列裁剪 | 下推 | 穿透 Projection/Agg/Join/SetOp，保留排序键 | P0 | L6 | AC-RBO-003 |
| R-PPD | 谓词下推 | 下推 | 穿透 Scan/Join/SetOp；**外连接 ON vs WHERE 边界** | P0 | L6 | AC-RBO-004 |
| R-PPD-OUTER | 外连接谓词下推 | 下推 | 空值拒绝（null-rejecting）判定 | P0 | L6 | AC-RBO-004 |
| R-PPD-AGG | 聚合下方谓词下推 | 下推 | 仅可下推分组键相关谓词 | P1 | L6 | AC-RBO-004 |
| R-PPD-WINDOW | 窗口下方谓词下推 | 下推 | 仅 partition key 谓词 | P1 | L6 | AC-RBO-004 |
| R-PPD-SETOP | 集合运算谓词下推 | 下推 | 两侧等价映射 | P1 | L6 | AC-RBO-004 |
| R-LIMIT-PD | Limit 下推 | 下推 | 穿透 Projection/Selection/UnionAll/Join 单边 | P0 | L6 | AC-RBO-005 |
| R-TOPN-PD | TopN 下推 | 下推 | 需局部 TopN + 全局归并改写配合 | P0 | L6 | AC-RBO-005 |
| R-AGG-PD | 聚合下推 | 下推 | 改写为 Partial/Final 两阶段 | P1 | L6 | AC-RBO-005 |
| R-AGG-SPLIT | 分布式聚合拆分 | 下推 | 为 Cop/MPP 生成两阶段 | P0 | L12 | AC-EXEC-012 |
| R-PART-PRUNE | 静态分区裁剪 | 下推 | Range/Hash/List 字面量谓词 | P1 | L6 | AC-RBO-006 |
| R-DYN-PRUNE | 动态分区裁剪 | 下推 | Runtime Filter 驱动（与 L12 联动） | P1 | L12 | AC-RBO-006 |
| R-ICP | 索引条件下推 | 下推 | 白名单表达式下推到 IndexScan | P0 | L6 | AC-RBO-006 |
| R-PROJ-ELIM | 冗余投影消除 | 消除 | 恒等投影、相邻投影合并 | P0 | L6 | AC-RBO-007 |
| R-SORT-ELIM | 冗余排序消除 | 消除 | 子节点已满足 ordering / 上层无有序需求 | P0 | L6 | AC-RBO-007 |
| R-LIMIT-MERGE | 相邻 Limit 合并 | 消除 | 取 min | P0 | L6 | AC-RBO-007 |
| R-DISTINCT-ELIM | 去重消除 | 消除 | 唯一键/主键/已聚合输出推导 | P1 | L6 | AC-RBO-007 |
| R-GROUPBY-ELIM | 分组消除 | 消除 | 唯一约束下 GROUP BY 冗余 | P1 | L6 | AC-RBO-007 |
| R-REDUC-ELIM | 冗余算子消除（通用） | 消除 | Noop Selection / Cast / PassThrough | P1 | L6 | AC-RBO-007 |
| R-MAXMIN | MAX/MIN 特化 | 特化 | 走有序索引 TopN(1) | P1 | L6 | AC-RBO-008 |
| R-COUNTSTAR | COUNT(*) 特化 | 特化 | 元数据行数 / 索引快速计数 | P1 | L6 | AC-RBO-008 |
| R-EXISTS-LIMIT1 | EXISTS → Limit1 + Semi | 特化 | 提前终止 | P1 | L6 | AC-RBO-008 |
| R-COUNTDISTINCT | COUNT(DISTINCT) 改写 | 特化 | 去重 + 计数 | P2 | L6 | AC-RBO-008 |
| R-JOIN-ELIM | Join 消除 | 重写 | PK/FK/唯一约束 + 无引用列 | P1 | L6 | AC-RBO-009 |
| R-OUTER2INNER | 外连接转内连接 | 重写 | WHERE 侧 null-rejecting 谓词 | P0 | L6 | AC-RBO-009 |
| R-OUTER-ELIM | 外连接消除 | 重写 | 无 NULL 扩展需求 | P1 | L6 | AC-RBO-009 |
| R-DECORRELATE | 子查询去关联 | 重写 | **Apply → Join/Agg 的核心变换** | P0 | L6 | AC-RBO-010 |
| R-IN-SEMI | `IN` → SemiJoin | 重写 | 含 NULL 语义 | P0 | L6 | AC-RBO-010 |
| R-EXISTS-SEMI | `EXISTS` → SemiJoin | 重写 | 去重与提前终止 | P0 | L6 | AC-RBO-010 |
| R-NOTIN-NAAJ | `NOT IN` → Null-Aware Anti Join | 重写 | **三值逻辑正确性重点** | P0 | L6 | AC-RBO-010 |
| R-SCALAR-SUBQ | 标量子查询 → MaxOneRow + Join | 重写 | 结果 >1 行报错 | P0 | L6 | AC-RBO-010 |
| R-SUBQ-UNNEST | 子查询展开 | 重写 | 派生表合并、视图内联 | P1 | L6 | AC-RBO-010 |
| R-OR-TO-UNION | OR 谓词 → UNION ALL | 重写 | 索引可利用性 + 去重（需 UNION DISTINCT） | P1 | L6 | AC-RBO-011 |
| R-UNION-MERGE | 相邻 UNION 合并 | 重写 | 结合律 | P2 | L6 | AC-RBO-011 |
| R-CTE-INLINE | CTE 内联决策 | 重写 | 单次引用内联 | P1 | L6 | AC-RBO-011 |
| R-CTE-MATERIALIZE | CTE 物化决策 | 重写 | 多次引用物化，代价判定 | P1 | L6 | AC-RBO-011 |
| R-EXPAND-REWRITE | GroupingSets 改写 | 重写 | Expand + 多聚合 | P1 | L6 | AC-RBO-011 |
| R-WINDOW-OPT | 窗口优化 | 重写 | 同 partition/order 窗口合并、frame 化简 | P2 | L6 | AC-RBO-011 |
| R-INDEXMERGE | IndexMerge 改写 | 重写 | OR/AND 多索引 → 交并 + 回表 | P1 | L9 | AC-RBO-011 |
| R-COMMONSUBPLAN | 公共子计划消除 | 重写 | 结构等价子树复用 | P2 | L15 | AC-RBO-012 |
| R-CONSTRAINT-PROP | 约束传播 | 化简 | NOT NULL / 唯一 / 检查约束参与推导 | P2 | L15 | AC-RBO-012 |

---

## 6. CBO 能力点全量清单 `CB-*`（★核心 ③，47 项，归属 `F-CBO-*`）

| 编号 | 能力点 | 说明 / 公式要点 | 优先级 | Lab | 验收 |
| --- | --- | --- | --- | --- | --- |
| CB-ANALYZE | `ANALYZE TABLE` 全量/采样 | 可控采样率、固定种子 | P0 | L7 | AC-CBO-001 |
| CB-SAMPLE-RESERVOIR | Reservoir 采样 | 行级等概率 | P0 | L7 | AC-CBO-001 |
| CB-SAMPLE-BERNOULLI | Bernoulli 采样 | 块级、IO 友好 | P1 | L7 | AC-CBO-001 |
| CB-NDV | NDV 估计 | 精确去重 / HyperLogLog | P0 | L7 | AC-CBO-001 |
| CB-HIST-EQUIHEIGHT | 等高直方图 | 桶内均匀分布插值 | P0 | L7 | AC-CBO-002 |
| CB-HIST-EQUIWIDTH | 等宽直方图 | 高偏斜补充 | P1 | L7 | AC-CBO-002 |
| CB-HIST-HYBRID | 混合/单例直方图 | 高频值单例桶 | P1 | L7 | AC-CBO-002 |
| CB-TOPN | TopN 高频值 | 与直方图互补 | P1 | L7 | AC-CBO-002 |
| CB-CMSKETCH | Count-Min Sketch | 等值/点查频率估计 | P1 | L7 | AC-CBO-002 |
| CB-MULTICOL | 多列组合统计 / 函数依赖 | 缓解独立性假设误差 | P1 | L7 | AC-CBO-002 |
| CB-CORRELATION | 列相关性系数 | 顺序相关（order dependency） | P2 | L7 | AC-CBO-002 |
| CB-IDXSTATS | 索引统计 | 索引前缀 NDV / 行数 | P0 | L7 | AC-CBO-002 |
| CB-AUTOANALYZE | 自动收集 | modify_count 触发、后台限流 | P1 | L7 | AC-CBO-003 |
| CB-STALE | 统计过期判定 | `modify/row_count > stale_ratio` | P1 | L7 | AC-CBO-003 |
| CB-INCREMENTAL | 统计增量更新 | 分区级增量合并 | P2 | L15 | AC-CBO-003 |
| CB-STATSVERSION | `stats_version` 与缓存失效 | 计划缓存 key 组成 | P0 | L9 | AC-PLAN-006 |
| CB-FEEDBACK | 估计反馈闭环 | `EXPLAIN ANALYZE` 实际行数反哺 | P2 | L15 | AC-CBO-004 |
| CB-STATS-INJECT | 统计扰动注入（实验） | 测 CE 鲁棒性（RQ-2） | P1 | L14 | AC-CBO-004 |
| CB-SEL-BASIC | 单表选择性 | `=` / 范围 / `IN` / `LIKE` / `IS NULL` | P0 | L7 | AC-CBO-005 |
| CB-SEL-BOOL | 复合谓词选择性 | `AND/OR/NOT` 独立性假设与修正 | P0 | L7 | AC-CBO-005 |
| CB-SEL-JOIN | Join 选择性 | `1/max(NDV_l, NDV_r)`，多键/非等值 | P0 | L7 | AC-CBO-005 |
| CB-SEL-INTERP | 直方图桶内插值 | 范围选择性精细估计 | P0 | L7 | AC-CBO-005 |
| CB-ERR-PROP | 误差传播建模 | est_rows 沿树传播与放大 | P1 | L7 | AC-CBO-005 |
| CB-BOUND | 估计上下界裁剪 | 不超过输入基数/物理上限 | P0 | L7 | AC-CBO-005 |
| CB-QERROR | q-error 监控 | `max(est/act, act/est)` 分位数看板 | P0 | L7 | AC-CBO-006 |
| CB-SKEW-DETECT | 数据倾斜检测 | 高频值 / 分布偏斜 | P1 | L12 | AC-CBO-006 |
| CB-COST-OPS | 全算子代价公式 | Scan/IndexLookUp/HashJoin/Sort/Agg/Exchange… | P0 | L7 | AC-CBO-007 |
| CB-COST-WEIGHTS | 代价权重与标定 | `w_cpu/w_io/w_net/w_mem`，回归标定 | P0 | L7 | AC-CBO-007 |
| CB-COST-NET | 网络与分布式代价 | 字节数、节点数、倾斜惩罚 | P0 | L8 | AC-CBO-007 |
| CB-COST-SPILL | 内存/Spill 代价 | 超过配额的 IO 惩罚 | P1 | L10 | AC-CBO-007 |
| CB-COST-BREAKDOWN | 代价分解输出 | 每算子 cpu/io/net/mem 可观测 | P0 | L9 | AC-PLAN-004 |
| CB-DP | System R 自底向上 DP | left-deep / bushy / interesting orders | P0 | L8 | AC-CBO-008 |
| CB-MEMO | Cascades Memo | Group / GroupExpression / Winner / 双实现对比 | P0 | L8 | AC-CBO-009 |
| CB-PROPERTY | 物理属性 | Ordering / Distribution / Partitioning | P0 | L8 | AC-CBO-010 |
| CB-ENFORCER | Enforcer | Sort / Exchange 插入并计入代价 | P0 | L8 | AC-CBO-010 |
| CB-PRUNE | B&B / 支配剪枝 | 上界、同 property 支配、剪枝率统计 | P0 | L8 | AC-CBO-009 |
| CB-JOINORDER | Join Reorder 枚举 | DPsize / DPsub / **DPccp** / Greedy / GEQO | P0 | L8 | AC-CBO-011 |
| CB-JOINCONSTRAINT | 重排约束 | 外连接 / Semi / Anti / Apply 的顺序约束 | P0 | L8 | AC-CBO-011 |
| CB-CARTESIAN-AVOID | 笛卡尔积避免 | 连通子图枚举 | P0 | L8 | AC-CBO-011 |
| CB-TIMEOUT | 搜索预算与回退 | `cbo_timeout` → DP/Greedy/RBO 回退并留痕 | P0 | L8 | AC-CBO-012 |
| CB-COPD | Cop 下推代价决策 | 收益 = 减少返回行数×网络单价 − 存储 CPU | P0 | L9 | AC-PLAN-005 |
| CB-BROADCAST | Broadcast vs Shuffle 选择 | 阈值 + 网络代价 + 倾斜 | P0 | L8 | AC-CBO-013 |
| CB-EXCHANGE | Exchange 插入点选择 | 满足 distribution property 的最小代价插入 | P0 | L9 | AC-CBO-013 |
| CB-STORAGE | 行/列/混合存储选择 | 统一代价空间，新鲜度约束 | P1 | L13 | AC-HTAP-004 |
| CB-PLANDIFF | 计划质量基线与 diff | TPC-H/DS plan hash、回归检测 | P0 | L8/L14 | AC-PLAN-009 |
| CB-OBS | Memo Dump / Prune Stat / ChosenReason | 优化过程白盒输出 | P0 | L9 | AC-PLAN-004 |

---

## 7. 执行计划能力点全量清单 `PL-*`（★核心 ④，24 项，归属 `F-PLAN-*`）

| 编号 | 能力点 | 说明 | 优先级 | Lab | 验收 |
| --- | --- | --- | --- | --- | --- |
| PL-FASTPLAN | Fast Plan | PointGet / BatchPointGet / 简单 INSERT 绕过优化器 | P0 | L5 | AC-PLAN-001 |
| PL-EXPLAIN | `EXPLAIN` 基础输出 | Logical / Physical / Distributed 三态 | P0 | L2 | AC-PLAN-002 |
| PL-FORMAT | 输出格式 | `brief / verbose / tree / json / dot` | P1 | L9 | AC-PLAN-002 |
| PL-ANALYZE | `EXPLAIN ANALYZE` | actual_rows / 耗时 / 内存 / Spill / 网络 / 回流 | P0 | L9 | AC-PLAN-003 |
| PL-FOR-CONN | `EXPLAIN FOR CONNECTION` | 在线运行查询计划抓取 | P2 | L9 | AC-PLAN-003 |
| PL-TRACE | `OPTIMIZE_TRACE` | 规则命中、前后计划 diff、耗时 | P0 | L6/L9 | AC-PLAN-004 |
| PL-MEMO-DUMP | Memo / 搜索过程输出 | Group、Winner、剪枝统计 | P1 | L9 | AC-PLAN-004 |
| PL-COST-BREAKDOWN | 代价分解 | 逐算子 cpu/io/net/mem + 选中理由 | P0 | L9 | AC-PLAN-004 |
| PL-COP-DECISION | Cop 可下推判定 | 白名单 + 代价决策 + 回退日志 | P0 | L9 | AC-PLAN-005 |
| PL-EXCHANGE-INSERT | Exchange 插入 | 满足 property，最小代价位置 | P0 | L9 | AC-PLAN-005 |
| PL-FRAGMENT | Fragment 切分与并行度 | Task 生成、资源组、超时 | P0 | L9 | AC-PLAN-005 |
| PL-SERIALIZE | 计划序列化 | protobuf / JSON，跨节点与持久化 | P0 | L9 | AC-PLAN-005 |
| PL-CACHE | Plan Cache | 参数化、key、命中/失效 | P0 | L9 | AC-PLAN-006 |
| PL-PREPARED | Prepared Statement | 参数绑定与缓存联动 | P0 | L5 | AC-PLAN-006 |
| PL-INVALIDATE | 缓存失效 | schema_version / stats_version / session var | P0 | L9 | AC-PLAN-006 |
| PL-HINT | Hint 体系 | 见下表逐条 | P0 | L9 | AC-PLAN-007 |
| PL-SPM | SPM / Baseline | 计划绑定、演化、防回归 | P1 | L9 | AC-PLAN-008 |
| PL-REPLAYER | Plan Replayer | 现场（SQL+stats+schema+plan）打包回放 | P1 | L9 | AC-PLAN-008 |
| PL-DIFF-CI | 计划回归门禁 | CI 中 plan hash diff 阻断 | P0 | L14 | AC-PLAN-009 |
| PL-PLANHASH | 计划稳定性统计 | plan hash / 变更趋势看板 | P1 | L9 | AC-PLAN-009 |
| PL-AQE | 自适应重优化 | 运行时基数偏差超阈值触发局部重规划 | P2 | L12 | AC-PLAN-010 |
| PL-REOPT-TRACE | 重优化留痕 | AQE 前后计划对比 | P2 | L12 | AC-PLAN-010 |
| PL-STABILITY | 计划稳定性 SLA | 同 SQL 在无数据变化下计划不变 | P1 | L9 | AC-PLAN-009 |
| PL-COST-VIZ | 代价可视化数据源 | 论文图表导出（CSV/JSON） | P2 | L14 | AC-PLAN-010 |

**PL-HINT 子项**（全部 P1，验收 AC-PLAN-007）：`USE_INDEX / FORCE_INDEX / IGNORE_INDEX`、`HASH_JOIN / MERGE_JOIN / INL_JOIN / INL_HASH_JOIN / HASH_AGG / STREAM_AGG`、`LEADING`（join order）、`AGG_TO_COP / LIMIT_TO_COP / IGNORE_PLAN_CACHE`、`READ_FROM_STORAGE(TIFLASH/TIKV)`、`BROADCAST_JOIN / SHUFFLE_JOIN`、`MAX_EXECUTION_TIME`、`MEMORY_QUOTA`、`SET_VAR`、`NO_DECORRELATE`、`USE_TOJA`。

---

## 8. 功能点总表（19 域 / 312 项）

> 表头：`编号 | 功能点 | 优先级 | Lab | 依赖 | 验收`。依赖列 `—` 表示无前置。

### 8.1 F-SQL · 协议、会话、解析、绑定（18）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-SQL-001 | MySQL wire 握手与认证（mysql_native_password 子集） | P0 | L5 | — | AC-SQL-001 |
| F-SQL-002 | Text / Binary 协议与结果集流式编码 | P0 | L5 | 001 | AC-SQL-001 |
| F-SQL-003 | Session 上下文与系统变量（影响计划的变量纳入 cache key） | P0 | L5 | 001 | AC-SQL-002 |
| F-SQL-004 | Prepared Statement / 参数占位符 | P0 | L5 | 001 | AC-PLAN-006 |
| F-SQL-005 | 词法/语法分析器与 AST | P0 | L1 | — | AC-SQL-003 |
| F-SQL-006 | 语法错误恢复与位置诊断 | P1 | L1 | 005 | AC-SQL-003 |
| F-SQL-007 | 名称解析（库/表/列/别名/歧义检查） | P0 | L1 | 005 | AC-SQL-004 |
| F-SQL-008 | 类型推导与隐式转换 | P0 | L1 | F-TYP-001 | AC-TYP-004 |
| F-SQL-009 | 权限检查（库/表/列级） | P1 | L5 | 007 | AC-SQL-004 |
| F-SQL-010 | 多语句 / 多结果集 | P1 | L5 | 002 | AC-SQL-001 |
| F-SQL-011 | Hint 语法解析（`/*+ ... */`） | P0 | L9 | 005 | AC-PLAN-007 |
| F-SQL-012 | Charset / Collation 会话语义 | P1 | L1 | F-TYP-007 | AC-TYP-007 |
| F-SQL-013 | SQL Mode / 注释与优化器注释兼容 | P2 | L15 | 005 | AC-SQL-003 |
| F-SQL-014 | 连接限流、空闲超时、查询取消 | P1 | L5 | 001 | AC-EXEC-015 |
| F-SQL-015 | 慢查询日志与 SQL 归一化摘要 | P0 | L5 | 002 | AC-OBS-003 |
| F-SQL-016 | 错误码对齐 MySQL（客户端可读） | P1 | L5 | 001 | AC-SQL-001 |
| F-SQL-017 | 时区与字符集转换 | P1 | L1 | F-TYP-004 | AC-TYP-004 |
| F-SQL-018 | 语句级资源组 / 标签 | P2 | L12 | 003 | AC-EXEC-015 |

### 8.2 F-TYP · 类型系统与表达式（18）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-TYP-001 | DataType 全集（数值/字符串/时间/Decimal/JSON…） | P0 | L1 | — | AC-TYP-001 |
| F-TYP-002 | Scalar 值与 Block/Column 列式表示 | P0 | L1 | 001 | AC-TYP-002 |
| F-TYP-003 | Decimal 精度、溢出、除零语义 | P0 | L1 | 001 | AC-TYP-003 |
| F-TYP-004 | Date/Time/Timestamp/Duration 与时区 | P0 | L1 | 001 | AC-TYP-004 |
| F-TYP-005 | NULL 与三值逻辑（Bool3） | P0 | L1 | 001 | AC-TYP-005 |
| F-TYP-006 | FuncRegistry（确定性/可下推/代价/NULL 传播元数据） | P0 | L1 | 001 | AC-TYP-006 |
| F-TYP-007 | Collation 比较与排序一致性（上下层同实现） | P1 | L1 | 001 | AC-TYP-007 |
| F-TYP-008 | CAST 矩阵与溢出/截断策略 | P0 | L1 | 001 | AC-TYP-004 |
| F-TYP-009 | 标量函数集（数值/字符串/日期/条件/JSON/哈希） | P0 | L1 | 006 | AC-TYP-006 |
| F-TYP-010 | 聚合函数集 | P0 | L3 | 006 | AC-OP-014 |
| F-TYP-011 | 窗口函数集 | P1 | L3 | 006 | AC-OP-017 |
| F-TYP-012 | Validity bitmap / SelectionVector | P0 | L1 | 002 | AC-OP-023 |
| F-TYP-013 | 字符串/二进制保序编码基础 | P0 | L4 | 001 | AC-STO-001 |
| F-TYP-014 | 向量（Vector）类型与距离函数 | P2 | L15 | 001 | AC-TYP-008 |
| F-TYP-015 | UDF 框架（WASM / 动态库） | P2 | L15 | 006 | AC-TYP-008 |
| F-TYP-016 | 表达式等价比较器（计划 diff 依赖） | P0 | L2 | 006 | AC-RBO-012 |
| F-TYP-017 | 常量折叠与表达式 CSE | P0 | L6 | 006 | AC-RBO-001 |
| F-TYP-018 | 向量化 / SIMD / JIT 求值（见 OP-X-*） | P0 | L10 | 006 | AC-OP-024 |

### 8.3 F-DDL · DDL / Catalog / Schema 变更（15）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-DDL-001 | CREATE/DROP DATABASE/TABLE | P0 | L11 | F-STO-001 | AC-DDL-001 |
| F-DDL-002 | ADD/DROP/RENAME INDEX | P0 | L11 | 001 | AC-DDL-001 |
| F-DDL-003 | ADD/DROP/MODIFY COLUMN（含 instant 变体） | P1 | L11 | 001 | AC-DDL-002 |
| F-DDL-004 | Online DDL 状态机（WriteOnly→WriteReorg→DeleteOnly→Public） | P0 | L11 | 001 | AC-DDL-002 |
| F-DDL-005 | schema_version 与 lease 同步协议 | P0 | L11 | 004 | AC-DDL-002 |
| F-DDL-006 | DDL Job 队列（持久化/幂等/回滚/重放） | P1 | L11 | 004 | AC-DDL-003 |
| F-DDL-007 | ADD INDEX 回填 + 增量同步 | P1 | L11 | 002,004 | AC-DDL-003 |
| F-DDL-008 | 元数据锁与 DDL/DML 并发正确性 | P0 | L11 | 004 | AC-DDL-004 |
| F-DDL-009 | TRUNCATE / RENAME TABLE | P1 | L11 | 001 | AC-DDL-001 |
| F-DDL-010 | 主键/唯一/外键/检查约束元数据 | P1 | L11 | 001 | AC-DDL-004 |
| F-DDL-011 | information_schema / SHOW 语句 | P1 | L5 | 001 | AC-SQL-004 |
| F-DDL-012 | 视图定义与查询内联 | P2 | L15 | 001 | AC-RBO-011 |
| F-DDL-013 | Catalog 快照与多版本读取 | P0 | L11 | 005 | AC-DDL-002 |
| F-DDL-014 | 统计/索引元数据持久化 | P0 | L7 | 001 | AC-CBO-001 |
| F-DDL-015 | 元数据升级与向后兼容 | P2 | L15 | 005 | AC-DDL-005 |

### 8.4 F-TXN · 事务与并发控制（20）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-TXN-001 | BEGIN / COMMIT / ROLLBACK / 自动提交 | P0 | L4 | — | AC-TXN-001 |
| F-TXN-002 | TSO 获取 start_ts / commit_ts | P0 | L4 | F-META-001 | AC-TXN-001 |
| F-TXN-003 | SI 快照读（按 start_ts 的 MVCC 可见性） | P0 | L4 | F-STO-004 | AC-TXN-002 |
| F-TXN-004 | RC 隔离级别（语句级新快照） | P1 | L11 | 003 | AC-TXN-002 |
| F-TXN-005 | 乐观事务（Prewrite 冲突检测） | P0 | L4 | 003 | AC-TXN-003 |
| F-TXN-006 | 悲观锁 / `SELECT … FOR UPDATE` | P0 | L11 | 005 | AC-TXN-003 |
| F-TXN-007 | Prewrite（写 lock + default，冲突检查） | P0 | L4 | 005 | AC-TXN-003 |
| F-TXN-008 | Commit / Rollback（写 write、删 lock） | P0 | L4 | 007 | AC-TXN-003 |
| F-TXN-009 | 1PC 单 Region 优化 | P1 | L11 | 008 | AC-TXN-004 |
| F-TXN-010 | 分布式 2PC + primary/secondary | P0 | L11 | 008 | AC-TXN-004 |
| F-TXN-011 | 写写冲突检测与自动重试 | P0 | L4 | 007 | AC-TXN-005 |
| F-TXN-012 | ResolveLock（读遇锁，查 primary 补/回滚） | P0 | L4 | 007 | AC-TXN-006 |
| F-TXN-013 | Lock TTL / 大事务续期（keep_alive） | P1 | L11 | 007 | AC-TXN-006 |
| F-TXN-014 | 大事务分批次 Prewrite（txn_size 上限） | P1 | L11 | 007 | AC-TXN-006 |
| F-TXN-015 | GC safe point 与版本清理 | P0 | L11 | 003 | AC-TXN-007 |
| F-TXN-016 | 读己之写（UnionScan / 事务缓冲） | P0 | L4 | 003 | AC-OP-009 |
| F-TXN-017 | 悲观死锁检测与回滚 | P1 | L11 | 006 | AC-TXN-005 |
| F-TXN-018 | 只读事务优化（免 2PC） | P1 | L11 | 003 | AC-TXN-004 |
| F-TXN-019 | SSI / 可串行化（写偏斜检测） | P2 | L15 | 003 | AC-TXN-008 |
| F-TXN-020 | Async Commit / 1PC 提交延迟优化 / 保存点 | P2 | L15 | 010 | AC-TXN-008 |

### 8.5 F-OP · 算子框架与横切能力（26）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-OP-001 | LogicalPlan 节点体系与 Schema 推导（覆盖 §4.1 扫描/投影/选择组） | P0 | L2 | — | AC-OP-001 |
| F-OP-002 | Join 逻辑算子族 | P0 | L2 | 001 | AC-OP-002 |
| F-OP-003 | Apply / 相关子查询 / MaxOneRow | P0 | L2 | 001 | AC-OP-003 |
| F-OP-004 | Agg / Distinct / Expand | P0 | L2 | 001 | AC-OP-004 |
| F-OP-005 | Window / Sort / TopN / Limit | P0 | L2 | 001 | AC-OP-005 |
| F-OP-006 | SetOp / CTE / PartitionUnion / Sequence | P0 | L2 | 001 | AC-OP-006 |
| F-OP-007 | DML / Lock / DDL 逻辑算子 | P0 | L3 | 001 | AC-OP-007 |
| F-OP-008 | 扫描类物理算子（Table/Index/LookUp/Merge） | P0 | L3 | 001 | AC-OP-008 |
| F-OP-009 | 点查 / 采样 / 列存 / CTE / UnionScan 扫描 | P0 | L5 | 008 | AC-OP-009 |
| F-OP-010 | HashJoin / MergeJoin / NLJ / Cartesian | P0 | L3 | 002 | AC-OP-010 |
| F-OP-011 | IndexJoin / IndexHashJoin | P0 | L3 | 008 | AC-OP-011 |
| F-OP-012 | 分布式 Join（Broadcast / Shuffle） | P0 | L12 | 010 | AC-OP-012 |
| F-OP-013 | Semi / Anti / Null-Aware Anti / OuterSemi | P0 | L3 | 002 | AC-OP-013 |
| F-OP-014 | HashAgg / StreamAgg / 两阶段 / Distinct / 标量 / 近似 | P0 | L3 | 004 | AC-OP-014 |
| F-OP-015 | Sort / ExternalSort / TopN / Limit / MergeSort | P0 | L3 | 005 | AC-OP-015 |
| F-OP-016 | Union / Intersect / Except | P0 | L3 | 006 | AC-OP-016 |
| F-OP-017 | Window 物理算子与 frame | P1 | L3 | 005 | AC-OP-017 |
| F-OP-018 | Materialize / HashTable / Chunk 缓冲 | P0 | L3 | 010 | AC-OP-018 |
| F-OP-019 | DML 物理算子 / SelectLock / LoadData | P0 | L3 | 007 | AC-OP-019 |
| F-OP-020 | 管理/诊断算子（Analyze/CheckTable/SplitRegion/SHOW/Trace） | P1 | L7/L9 | 001 | AC-OP-020 |
| F-OP-021 | **Spill / Grace Hash Join / 外排 / 可溢写聚合（内存降级）** | P0 | L10 | 010,014,015 | AC-OP-021 |
| F-OP-022 | Runtime Filter / Bloom / Adaptive Join | P1 | L12 | 010 | AC-OP-022 |
| F-OP-023 | 向量化 Block / SelVector / 零拷贝 / VecEval | P0 | L10 | F-TYP-002 | AC-OP-023 |
| F-OP-024 | SIMD / JIT / 字典编码 / 表达式缓存 | P1 | L10 | 023 | AC-OP-024 |
| F-OP-025 | 算子统一观测（actual_rows/耗时/内存/spill 埋点） | P0 | L3 | 001 | AC-OP-025 |
| F-OP-026 | 算子边界语义（NULL/空集/溢出/大数/倾斜）测试矩阵 | P0 | L3/L10 | 001 | AC-OP-026 |

### 8.6 F-RBO · 规则优化框架（12；逐规则见 §5）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-RBO-001 | Rule / Pattern / Matcher 引擎 | P0 | L6 | F-OP-001 | AC-RBO-001 |
| F-RBO-002 | RuleBatch + FixedPoint + 收敛/循环检测 | P0 | L6 | 001 | AC-RBO-012 |
| F-RBO-003 | 规则独立开关 + 命中留痕（ADR-6） | P0 | L6 | 001 | AC-RBO-012 |
| F-RBO-004 | 化简类规则集（§5 化简/消除） | P0 | L6 | 001 | AC-RBO-001 |
| F-RBO-005 | 下推类规则集（§5 下推） | P0 | L6 | 001 | AC-RBO-003..006 |
| F-RBO-006 | 谓词下推外连接/聚合/窗口边界护栏 | P0 | L6 | 005 | AC-RBO-004 |
| F-RBO-007 | 子查询去关联（R-DECORRELATE 专项） | P0 | L6 | F-OP-003 | AC-RBO-010 |
| F-RBO-008 | `IN/EXISTS/NOT IN/标量子查询` 重写族 | P0 | L6 | 007 | AC-RBO-010 |
| F-RBO-009 | Join / 外连接消除与转换 | P0 | L6 | F-OP-002 | AC-RBO-009 |
| F-RBO-010 | 分区裁剪 / ICP / IndexMerge 改写 | P1 | L6/L9 | 005 | AC-RBO-006 |
| F-RBO-011 | 规则等价性验证（证明 + 反例 + 差分） | P0 | L6 | 001 | AC-RBO-001 |
| F-RBO-012 | 规则最小充分集消融数据（RQ-7） | P1 | L6/L14 | 003 | AC-RBO-012 |

### 8.7 F-CBO · 代价优化框架（14；逐能力见 §6）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-CBO-001 | 统计信息采集与存储（CB-ANALYZE..CB-IDXSTATS） | P0 | L7 | F-DDL-014 | AC-CBO-001 |
| F-CBO-002 | 直方图 / TopN / CMSketch / 多列统计 | P0 | L7 | 001 | AC-CBO-002 |
| F-CBO-003 | 自动收集与过期判定 | P1 | L7 | 001 | AC-CBO-003 |
| F-CBO-004 | 估计反馈闭环与统计扰动注入 | P1 | L14 | 001 | AC-CBO-004 |
| F-CBO-005 | 基数估计器（选择性公式 + 插值 + 误差传播） | P0 | L7 | 002 | AC-CBO-005 |
| F-CBO-006 | q-error 监控与倾斜检测 | P0 | L7 | 005 | AC-CBO-006 |
| F-CBO-007 | 全算子代价模型 + 权重标定 | P0 | L7 | F-OP-025 | AC-CBO-007 |
| F-CBO-008 | System R 自底向上 DP | P0 | L8 | 005,007 | AC-CBO-008 |
| F-CBO-009 | Cascades Memo（Group/Winner/剪枝） | P0 | L8 | 005,007 | AC-CBO-009 |
| F-CBO-010 | 物理属性与 Enforcer | P0 | L8 | 009 | AC-CBO-010 |
| F-CBO-011 | Join Reorder 枚举族（DPsize/DPsub/DPccp/Greedy/GEQO） | P0 | L8 | 008,009 | AC-CBO-011 |
| F-CBO-012 | 搜索预算、超时与回退 | P0 | L8 | 009 | AC-CBO-012 |
| F-CBO-013 | 分布式代价（Cop/Broadcast/Exchange/行列） | P0 | L8/L9/L13 | 007 | AC-CBO-013 |
| F-CBO-014 | 双引擎对比实验与计划质量基线（RQ-1） | P1 | L8/L14 | 008,009 | AC-CBO-014 / AC-CBO-015 |

### 8.8 F-PLAN · 执行计划治理框架（12；逐能力见 §7）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-PLAN-001 | Fast Plan 快路径 | P0 | L5 | F-OP-009 | AC-PLAN-001 |
| F-PLAN-002 | EXPLAIN 三态输出 + 多格式 | P0 | L2/L9 | F-OP-001 | AC-PLAN-002 |
| F-PLAN-003 | EXPLAIN ANALYZE + FOR CONNECTION | P0 | L9 | 002 | AC-PLAN-003 |
| F-PLAN-004 | OPTIMIZE_TRACE / Memo Dump / 代价分解 / ChosenReason | P0 | L9 | F-RBO-003 | AC-PLAN-004 |
| F-PLAN-005 | Cop 下推判定 + Exchange 插入 + Fragment 切分 + 计划序列化 | P0 | L9 | F-CBO-013 | AC-PLAN-005 |
| F-PLAN-006 | Plan Cache / Prepared / 失效 | P0 | L9 | 005 | AC-PLAN-006 |
| F-PLAN-007 | Hint 体系（PL-HINT 全项） | P0 | L9 | F-SQL-011 | AC-PLAN-007 |
| F-PLAN-008 | SPM / Baseline / Plan Replayer | P1 | L9 | 006 | AC-PLAN-008 |
| F-PLAN-009 | 计划回归门禁 / plan hash / 稳定性 SLA | P0 | L9/L14 | 002 | AC-PLAN-009 / AC-TEST-009 |
| F-PLAN-010 | AQE 自适应重优化与留痕 / 代价可视化导出 | P2 | L12/L14 | 003 | AC-PLAN-010 |
| F-PLAN-011 | 下推开/关一致性契约（差分测试） | P0 | L9/L12 | 005 | AC-PLAN-005 |
| F-PLAN-012 | 计划与统计/schema 版本绑定的可回放性 | P1 | L9 | F-CBO-001 | AC-PLAN-008 |

### 8.9 F-EXEC · 执行引擎与分布式执行（22）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-EXEC-001 | 火山模型 `Executor` trait 与 ExecutorBuilder | P0 | L3 | F-OP-001 | AC-EXEC-001 |
| F-EXEC-002 | 列式 Chunk 执行路径（与行式并存、可开关） | P0 | L10 | F-OP-023 | AC-EXEC-002 |
| F-EXEC-003 | Morsel-driven 并行与线程池 | P1 | L10 | 002 | AC-EXEC-003 |
| F-EXEC-004 | MemoryTracker / 查询级内存配额 | P0 | L10 | 002 | AC-EXEC-004 |
| F-EXEC-005 | Spill 框架（临时文件、IO 计量、清理） | P0 | L10 | 004 | AC-OP-021 |
| F-EXEC-006 | 背压与 CancellationToken（贯穿 RPC） | P0 | L12 | 001 | AC-EXEC-015 |
| F-EXEC-007 | 结果流式编码与客户端回传 | P0 | L5 | 001 | AC-SQL-001 |
| F-EXEC-008 | 执行期 trace span 与统计聚合 | P0 | L9 | F-OP-025 | AC-OBS-002 |
| F-EXEC-009 | 分类重试（epoch/超时/写冲突）与幂等 | P0 | L11 | F-META-003 | AC-EXEC-016 |
| F-EXEC-010 | Exchange 收发 / Gather / PassThrough（OP-E 基础） | P0 | L12 | F-PLAN-005 | AC-EXEC-010 |
| F-EXEC-011 | Shuffle（Hash/Range/RoundRobin/Broadcast/MergeSort） | P0 | L12 | 010 | AC-EXEC-011 |
| F-EXEC-012 | Cop Task 切分、epoch 绑定、局部聚合/TopN | P0 | L12 | F-STO-020 | AC-EXEC-012 |
| F-EXEC-013 | MPP Task / Dispatcher / 节点放置 | P1 | L12 | 010 | AC-EXEC-013 |
| F-EXEC-014 | Pipeline 推送执行与背压 | P2 | L10 | 002 | AC-EXEC-014 |
| F-EXEC-015 | 资源组 / 限流 / 超时 / 取消 | P1 | L12 | 006 | AC-EXEC-015 |
| F-EXEC-016 | 节点故障的任务重发与结果去重 | P1 | L12 | 013 | AC-EXEC-016 |
| F-EXEC-017 | 数据倾斜处理（加盐、动态重分区） | P1 | L12 | 011 | AC-EXEC-011 |
| F-EXEC-018 | Runtime Filter 传递协议 | P1 | L12 | 010 | AC-OP-022 |
| F-EXEC-019 | 两阶段聚合/Join 的分布式正确性 | P0 | L12 | F-OP-014 | AC-EXEC-012 |
| F-EXEC-020 | 并行度决策与启动开销建模 | P1 | L9 | F-CBO-013 | AC-EXEC-013 |
| F-EXEC-021 | 算子融合 / Pipeline Breaker 识别 | P2 | L10 | 002 | AC-EXEC-014 |
| F-EXEC-022 | 查询结果一致性 checksum（调试模式） | P1 | L14 | 001 | AC-TEST-014 |

### 8.10 F-STO · 存储层（24）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-STO-001 | 保序（memcomparable）编码 | P0 | L4 | F-TYP-013 | AC-STO-001 |
| F-STO-002 | Key 布局（表/索引/元数据/TS 后缀） | P0 | L4 | 001 | AC-STO-001 |
| F-STO-003 | RocksDB 封装与 CF 配置 | P0 | L4 | 002 | AC-STO-002 |
| F-STO-004 | 三 CF MVCC 与快照读 | P0 | L4 | 003 | AC-TXN-002 |
| F-STO-005 | Get / BatchGet / Scan KV API | P0 | L4 | 003 | AC-STO-003 |
| F-STO-006 | Prewrite / Commit / Rollback / Raw KV API | P0 | L4 | 005 | AC-TXN-003 |
| F-STO-007 | MemoryKV 实现（单测底座） | P0 | L4 | 005 | AC-STO-003 |
| F-STO-008 | Block Cache / Compaction 调优 | P1 | L14 | 003 | AC-BENCH-006 |
| F-STO-009 | 写入限流与背压 | P1 | L11 | 006 | AC-STO-004 |
| F-STO-010 | GC 版本清理与 safe point | P0 | L11 | 004 | AC-TXN-007 |
| F-STO-011 | Region 定义 / epoch / 元数据 | P0 | L11 | 002 | AC-STO-005 |
| F-STO-012 | Raft Group（raft-rs）/ 多副本日志 | P0 | L11 | 011 | AC-STO-006 |
| F-STO-013 | Leader Lease / ReadIndex 线性读 | P0 | L11 | 012 | AC-STO-006 |
| F-STO-014 | Region Split / Merge | P1 | L11 | 011 | AC-STO-007 |
| F-STO-015 | Snapshot / 日志压缩 / Apply 状态机 | P1 | L11 | 012 | AC-STO-007 |
| F-STO-016 | Store 心跳与状态上报 | P0 | L11 | 012 | AC-META-004 |
| F-STO-017 | 副本校验 / checksum / 一致性比对 | P1 | L14 | 012 | AC-TEST-014 / AC-STO-008 |
| F-STO-018 | Coprocessor gRPC API | P0 | L12 | 005 | AC-EXEC-012 / AC-STO-009 |
| F-STO-019 | DagRequest 协议（算子 DAG + ranges + epoch） | P0 | L12 | 018 | AC-EXEC-012 / AC-STO-009 |
| F-STO-020 | 存储侧向量化表达式求值（复用 expr） | P0 | L12 | 019 | AC-PLAN-011 / AC-STO-009 |
| F-STO-021 | 存储侧 Selection / Agg / TopN / Limit | P0 | L12 | 020 | AC-EXEC-012 / AC-STO-009 |
| F-STO-022 | Range 按 Region 切分与部分结果重试 | P0 | L12 | 011,019 | AC-EXEC-016 |
| F-STO-023 | SST 批量导入 | P2 | L14 | 003 | AC-BENCH-007 / AC-STO-010 |
| F-STO-024 | 存储层指标与慢日志 | P1 | L9 | 005 | AC-OBS-004 / AC-STO-010 |

### 8.11 F-META · 元数据与调度（13）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-META-001 | TSO 批量分配与 HA 主备切换 | P0 | L11 | — | AC-META-001 |
| F-META-002 | TSO 单调性保证（跨切换） | P0 | L11 | 001 | AC-META-001 |
| F-META-003 | Region 路由与客户端 cache | P0 | L11 | F-STO-011 | AC-META-002 |
| F-META-004 | EpochNotMatch / NotLeader 重试 | P0 | L11 | 003 | AC-EXEC-016 |
| F-META-005 | 元数据自身 Raft HA | P0 | L11 | — | AC-META-003 |
| F-META-006 | 副本/Leader 均衡调度 | P1 | L11 | 004 | AC-META-004 |
| F-META-007 | 热点检测与打散 | P1 | L11 | 004 | AC-META-004 |
| F-META-008 | 副本补齐 / 故障下线处理 | P1 | L11 | 004 | AC-META-004 |
| F-META-009 | Split 决策与 Operator 队列/限流 | P1 | L11 | F-STO-014 | AC-META-005 |
| F-META-010 | Store 拓扑与版本管理 | P1 | L11 | 004 | AC-META-004 |
| F-META-011 | 故障检测（心跳超时） | P0 | L11 | 004 | AC-META-004 |
| F-META-012 | Placement 规则（标签/亲和性） | P2 | L15 | 006 | AC-META-005 |
| F-META-013 | 集群配置热加载 | P2 | L15 | 005 | AC-ECO-006 |

### 8.12 F-IDX · 索引（15）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-IDX-001 | 主键 / int handle | P0 | L2 | F-STO-002 | AC-IDX-001 |
| F-IDX-002 | 聚簇索引 / common handle | P1 | L11 | 001 | AC-IDX-001 |
| F-IDX-003 | 唯一索引（含 NULL 语义） | P0 | L3 | 001 | AC-IDX-001 |
| F-IDX-004 | 二级索引 / 联合索引最左前缀 | P0 | L3 | 001 | AC-IDX-002 |
| F-IDX-005 | 覆盖索引与 IndexOnlyScan | P0 | L3 | 004 | AC-IDX-002 |
| F-IDX-006 | IndexLookUp 回表与 IndexMerge 交并 | P0 | L9 | 004 | AC-IDX-002 |
| F-IDX-007 | Index Range 构造（等值/范围/IN/前缀 LIKE） | P0 | L3 | 004 | AC-IDX-003 |
| F-IDX-008 | ICP / 索引谓词残留处理 | P0 | L6 | 007 | AC-RBO-006 |
| F-IDX-009 | 不可见索引 / 强制索引 Hint | P1 | L9 | 004 | AC-PLAN-007 |
| F-IDX-010 | 表达式索引 / 前缀索引 | P2 | L15 | 004 | AC-IDX-004 |
| F-IDX-011 | 索引唯一性约束推导（供 Join 消除） | P1 | L6 | 003 | AC-RBO-009 |
| F-IDX-012 | 索引统计与选择率 | P0 | L7 | 004 | AC-CBO-002 |
| F-IDX-013 | DML 的索引维护（增/删/改） | P0 | L3 | 004 | AC-OP-019 |
| F-IDX-014 | ADD INDEX 在线回填 | P1 | L11 | 004 | AC-DDL-003 |
| F-IDX-015 | 索引可见性 / 约束元数据 | P1 | L11 | 004 | AC-IDX-004 |

### 8.13 F-PART · 分区（11）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-PART-001 | Range / Range Columns 分区 | P1 | L6 | F-DDL-001 | AC-PART-001 |
| F-PART-002 | Hash / Key 分区 | P1 | L6 | 001 | AC-PART-001 |
| F-PART-003 | List / List Columns 分区 | P1 | L6 | 001 | AC-PART-001 |
| F-PART-004 | 静态分区裁剪（R-PART-PRUNE） | P1 | L6 | 001 | AC-RBO-006 |
| F-PART-005 | 动态分区裁剪（runtime） | P1 | L12 | 004 | AC-RBO-006 |
| F-PART-006 | PartitionUnion 逻辑算子 | P1 | L2 | 001 | AC-OP-006 |
| F-PART-007 | 分区级统计与 CE | P1 | L7 | 001 | AC-CBO-002 |
| F-PART-008 | Partition-wise Join / Agg | P2 | L12 | 006 | AC-EXEC-011 |
| F-PART-009 | 分区 DDL（ADD/DROP/TRUNCATE/EXCHANGE） | P1 | L11 | 001 | AC-DDL-001 |
| F-PART-010 | 分区路由与写入定位 | P1 | L11 | 001 | AC-PART-002 |
| F-PART-011 | 分区索引与全局索引元数据 | P2 | L15 | 001 | AC-PART-002 |

### 8.14 F-HTAP · 列存与行列混合（11）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-HTAP-001 | Raft Learner 异步日志复制 | P1 | L13 | F-STO-012 | AC-HTAP-001 |
| F-HTAP-002 | 行→列转换与 `resolved_ts` 一致性读 | P1 | L13 | 001 | AC-HTAP-002 |
| F-HTAP-003 | 列式段存 + Delta / ZoneMap 粗过滤 | P1 | L13 | 002 | AC-HTAP-003 |
| F-HTAP-004 | 行列统一代价建模与自动选择 | P1 | L13 | F-CBO-013 | AC-HTAP-004 |
| F-HTAP-005 | `READ_FROM_STORAGE` Hint | P1 | L13 | 004 | AC-PLAN-007 |
| F-HTAP-006 | 列存向量化扫描 / 延迟物化 | P1 | L13 | 003 | AC-OP-009 |
| F-HTAP-007 | 列存 MPP（Exchange / 两阶段 Join&Agg） | P1 | L13 | F-EXEC-013 | AC-HTAP-005 |
| F-HTAP-008 | 列存副本一致性校验读 | P1 | L13 | 002 | AC-HTAP-002 |
| F-HTAP-009 | 新鲜度（freshness）监控与权衡曲线（RQ-5） | P1 | L14 | 002 | AC-HTAP-006 |
| F-HTAP-010 | OLTP 提交延迟隔离（列存不影响写入） | P0 | L13 | 001 | AC-HTAP-006 |
| F-HTAP-011 | 列存副本扩缩容与回放 | P2 | L15 | 001 | AC-HTAP-001 |

### 8.15 F-OBS · 可观测性与诊断（15）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-OBS-001 | Metrics 体系（查询/算子/存储/调度/事务） | P0 | L0 | — | AC-OBS-001 |
| F-OBS-002 | Tracing（OpenTelemetry span，贯穿 RPC/算子） | P0 | L0/L9 | — | AC-OBS-002 |
| F-OBS-003 | Slow Log / SQL 摘要与归一化 | P0 | L5 | — | AC-OBS-003 |
| F-OBS-004 | 存储/调度层指标 | P1 | L9 | — | AC-OBS-004 |
| F-OBS-005 | EXPLAIN ANALYZE 数据聚合与展示 | P0 | L9 | F-PLAN-003 | AC-PLAN-003 |
| F-OBS-006 | OPTIMIZE_TRACE / Memo / 代价可视化数据 | P0 | L9 | F-PLAN-004 | AC-PLAN-004 |
| F-OBS-007 | q-error 看板与告警 | P1 | L7 | F-CBO-006 | AC-CBO-006 |
| F-OBS-008 | plan hash / 计划稳定性看板 | P1 | L9 | F-PLAN-009 | AC-PLAN-009 |
| F-OBS-009 | 语句摘要 / TopSQL | P2 | L15 | 003 | AC-OBS-003 |
| F-OBS-010 | 诊断页（活跃查询/锁等待/Region 热点） | P2 | L15 | 001 | AC-OBS-005 |
| F-OBS-011 | 审计日志 | P2 | L15 | F-SQL-009 | AC-ECO-005 |
| F-OBS-012 | 实验元数据自动记录（配置/种子/环境/版本） | P0 | L0/L14 | — | AC-BENCH-010 |
| F-OBS-013 | 连续 profiling（perf / flamegraph） | P2 | L14 | 001 | AC-BENCH-009 |
| F-OBS-014 | 错误诊断上下文（sql_id/plan_id/region_id/ts） | P0 | L9 | 002 | AC-EXEC-016 |
| F-OBS-015 | 告警规则与阈值 | P2 | L15 | 001 | AC-OBS-005 |

### 8.16 F-TEST · 测试与正确性验证（21）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-TEST-001 | 单元测试框架与 crate 级 mock | P0 | L0 | — | AC-TEST-001 |
| F-TEST-002 | CI 门禁（fmt/clippy/test/覆盖率/依赖方向） | P0 | L0 | 001 | AC-TEST-002 |
| F-TEST-003 | 固定种子与实验记录模板 | P0 | L0 | — | AC-BENCH-010 |
| F-TEST-004 | 回归基线管理与 diff 工具 | P0 | L0 | 001 | AC-TEST-002 |
| F-TEST-005 | 类型/表达式用例集（含边界） | P0 | L1 | — | AC-TYP-001..008 |
| F-TEST-006 | 算子级正确性用例（逐 OP-P-*） | P0 | L3 | F-OP-001 | AC-OP-001..020 |
| F-TEST-007 | sqllogictest 集成 | P0 | L5 | 001 | AC-TEST-003 |
| F-TEST-008 | 多参考库差分测试（MySQL/PostgreSQL/DuckDB） | P0 | L3/L14 | 007 | AC-TEST-004 |
| F-TEST-009 | 计划等价比较器 | P0 | L2 | F-TYP-016 | AC-RBO-012 |
| F-TEST-010 | 下推开/关结果对拍 | P0 | L9 | F-PLAN-011 | AC-PLAN-005 |
| F-TEST-011 | SQLancer 风格 TLP / NoREC / PQS | P1 | L14 | 007 | AC-TEST-005 |
| F-TEST-012 | 随机 SQL 生成器（语法/类型感知） | P1 | L14 | 007 | AC-TEST-005 |
| F-TEST-013 | Metamorphic / 关系代数等价测试 | P1 | L6 | 009 | AC-RBO-001 / AC-TEST-013 |
| F-TEST-014 | 一致性 checksum / 副本比对 | P1 | L14 | F-STO-017 | AC-TEST-014 |
| F-TEST-015 | 崩溃恢复与重启测试 | P1 | L11 | F-STO-015 | AC-STO-007 / AC-TEST-010 |
| F-TEST-016 | 确定性分布式模拟器 | P1 | L11 | — | AC-TEST-006 |
| F-TEST-017 | 混沌工程（kill/分区/时钟漂移/磁盘满） | P1 | L14 | 016 | AC-TEST-007 |
| F-TEST-018 | 线性一致性检查（Porcupine / Jepsen 风格） | P1 | L14 | 016 | AC-TXN-008 |
| F-TEST-019 | 长稳 72h 压测 | P1 | L14 | 017 | AC-TEST-008 |
| F-TEST-020 | Fuzz（parser / protobuf / 编码） | P2 | L14 | 001 | AC-TEST-005 |
| F-TEST-021 | 覆盖率门禁与验收矩阵自动校验 | P0 | L14 | 002 | AC-TEST-002 / AC-TEST-011 / AC-TEST-012 |

### 8.17 F-BENCH · 基准与性能工程（16）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-BENCH-001 | TPC-H（22 查询 + SF 扩展 + 计划基线） | P0 | L8/L14 | F-PLAN-002 | AC-BENCH-001 |
| F-BENCH-002 | TPC-DS（99 查询 / 大查询搜索预算） | P1 | L14 | 001 | AC-BENCH-001 |
| F-BENCH-003 | TPC-C（事务吞吐与线性度） | P1 | L14 | F-TXN-010 | AC-BENCH-002 |
| F-BENCH-004 | sysbench OLTP / YCSB KV | P1 | L14 | F-SQL-001 | AC-BENCH-002 |
| F-BENCH-005 | criterion 算子级微基准 | P0 | L3/L10 | — | AC-BENCH-003 |
| F-BENCH-006 | 倾斜数据集（Zipf）生成 | P1 | L14 | — | AC-BENCH-004 |
| F-BENCH-007 | CE 误差基准（q-error 集） | P0 | L7 | F-CBO-006 | AC-CBO-006 |
| F-BENCH-008 | 规划时间 / Memo 规模基准 | P1 | L8 | F-CBO-014 | AC-CBO-014 |
| F-BENCH-009 | 消融实验框架（规则/算子/代价项/加速层） | P0 | L14 | — | AC-BENCH-005 |
| F-BENCH-010 | 扩张性测试（节点数 / SF / 并行度） | P1 | L14 | — | AC-BENCH-006 |
| F-BENCH-011 | 性能归因（perf / flamegraph / roofline） | P1 | L10/L14 | — | AC-BENCH-007 |
| F-BENCH-012 | 环境固化 `bench/env.toml` + 容器镜像 | P0 | L0/L14 | — | AC-BENCH-008 |
| F-BENCH-013 | 结果归档、置信区间与一键重跑 | P0 | L14 | 012 | AC-BENCH-009 |
| F-BENCH-014 | 基线对比（TiDB / DuckDB / PostgreSQL / DataFusion） | P1 | L14 | 012 | AC-BENCH-010 |
| F-BENCH-015 | 代价常数自动标定流程 | P0 | L7 | F-CBO-007 | AC-CBO-007 |
| F-BENCH-016 | 回归性能看板（按 commit） | P1 | L14 | 013 | AC-BENCH-010 |

### 8.18 F-ECO · 生态与兼容（15）

| 编号 | 功能点 | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- |
| F-ECO-001 | MySQL 协议兼容子集（mysql CLI / JDBC 可连） | P0 | L5 | F-SQL-001 | AC-SQL-001 |
| F-ECO-002 | CSV / Parquet 导入导出 | P1 | L14 | — | AC-ECO-001 |
| F-ECO-003 | `LOAD DATA` 批量导入 | P2 | L14 | 002 | AC-OP-020 |
| F-ECO-004 | 逻辑备份 / 恢复（BR-like） | P2 | L15 | — | AC-ECO-002 |
| F-ECO-005 | CDC 变更捕获（TiCDC-like） | P2 | L15 | F-STO-012 | AC-ECO-003 |
| F-ECO-006 | 用户 / 角色 / 权限 / 审计 | P1 | L5/L15 | F-SQL-009 | AC-ECO-005 |
| F-ECO-007 | 配置热加载 | P2 | L15 | — | AC-ECO-006 |
| F-ECO-008 | TLS 连接 | P2 | L15 | F-SQL-001 | AC-ECO-004 |
| F-ECO-009 | 错误码 / 元数据兼容（information_schema） | P1 | L5 | F-DDL-011 | AC-SQL-004 |
| F-ECO-010 | 多语言客户端示例（Rust/Python/Java） | P2 | L15 | 001 | AC-ECO-004 |
| F-ECO-011 | 版本升级与元数据兼容 | P2 | L15 | F-DDL-015 | AC-ECO-006 |
| F-ECO-012 | 时区 / 字符集配置体系 | P1 | L5 | F-TYP-004 | AC-TYP-004 |
| F-ECO-013 | 运维命令（ADMIN / SPLIT / REGION 查看） | P1 | L11 | F-META-003 | AC-META-005 |
| F-ECO-014 | 集群部署脚本（docker-compose / k8s manifest） | P1 | L11 | — | AC-ECO-006 |
| F-ECO-015 | 文档 / 示例 / 教程仓库 | P1 | 全阶段 | — | AC-ECO-007 |

### 8.19 F-RSH · 研究向拓展（14，P2 / `[研究向]`）

| 编号 | 功能点 | 对应 RQ | 优先级 | Lab | 依赖 | 验收 |
| --- | --- | --- | --- | --- | --- | --- |
| F-RSH-001 | DP vs Cascades Memo 的 Pareto 实验 | RQ-1 | P2 | L8/L15 | F-CBO-014 | AC-RSH-001 |
| F-RSH-002 | 基数估计误差传播与次优计划概率模型 | RQ-2 | P2 | L14/L15 | F-CBO-006 | AC-RSH-002 |
| F-RSH-003 | 学习式基数估计（轻量模型） | RQ-3 | P2 | L15 | F-CBO-005 | AC-RSH-003 |
| F-RSH-004 | 学习式代价 / RL Join Order | RQ-3 | P2 | L15 | F-CBO-011 | AC-RSH-003 |
| F-RSH-005 | 分布漂移下 CE 鲁棒性评估 | RQ-3 | P2 | L15 | 003 | AC-RSH-003 |
| F-RSH-006 | Runtime Filter / AQE 倾斜收益上界 | RQ-4 | P2 | L12/L14 | F-OP-022 | AC-RSH-004 |
| F-RSH-007 | 行列混合 freshness–latency 权衡 | RQ-5 | P2 | L13/L14 | F-HTAP-009 | AC-RSH-005 |
| F-RSH-008 | 向量化/SIMD/JIT 边际收益与 roofline | RQ-6 | P2 | L10/L14 | F-OP-024 | AC-RSH-006 |
| F-RSH-009 | 优化器规则最小充分集 | RQ-7 | P2 | L6/L14 | F-RBO-012 | AC-RSH-007 |
| F-RSH-010 | 存算分离 / 弹性缓存层 | — | P2 | L15 | F-STO-003 | AC-RSH-008 |
| F-RSH-011 | RDMA / 新硬件（NVMe-oF、CXL）路径 | — | P2 | L15 | F-EXEC-011 | AC-RSH-008 |
| F-RSH-012 | 关系代数等价的形式化验证 | — | P2 | L15 | F-TEST-013 | AC-RSH-009 |
| F-RSH-013 | GPU / 异构执行算子 | — | P2 | L15 | F-OP-023 | AC-RSH-008 |
| F-RSH-014 | 查询级能耗建模与测量 | — | P2 | L15 | F-BENCH-011 | AC-RSH-010 |

---

## 9. 覆盖与追溯检查（需求基线门禁）

1. **双向覆盖**：每个 `F-*` 至少被一条 `AC-*` 覆盖；每条 `AC-*` 至少回溯到一个 `F-*`。检查脚本 `tools/check_traceability.py`（L0 交付）在 CI 中运行，缺失即失败。
2. **优先级门禁**：所有 `P0` 必须在 M1–M5 内完成；`P1` 在 M6 前完成；`P2` 不进关键路径，进入排期需评审（03 RK-6）。
3. **重点域占比**：`OP / RBO / CBO / PLAN` 四域合计 64 个框架项 + §4–§7 的 232 个枚举项，占全部条目的 ~55%，与「算子、执行计划、CBO/RBO 为重点」一致。
4. **变更流程**：新增功能点 → 分配编号与优先级 → 补 `L*` / `M*` 映射 → 补 `AC-*` → 更新 03 排期与 05 矩阵 → 通过追溯检查后方可实现。
5. **状态同步**：实现阶段在本表追加「实现状态」列；M0–M7 评审时以本表与 05 验收矩阵为唯一需求依据。
