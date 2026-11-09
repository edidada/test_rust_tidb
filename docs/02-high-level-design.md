# 02 概要设计（High-Level Design）

> 状态：`[设计]` ｜ 版本：v0.1 ｜ 上游：01 架构设计 ｜ 下游：04 功能点列表、05 验收规划
>
> 本文定义**模块边界、核心数据结构、接口契约与关键机制**，是后续每个 Lab 的编码契约。所有签名为 Rust 伪代码（省略生命周期与错误细节），以实现时的 `common::Result<T>` 为统一错误类型。

---

## 1. 设计原则

1. **接口先于实现**：每个子系统以 `trait` 定义边界，允许 `MemoryKV` / `RocksKV`、`DPOptimizer` / `MemoOptimizer`、`RowExec` / `VectorizedExec` 互换，用于消融实验与单测 mock。
2. **不可变计划 + 可变状态分离**：`LogicalPlan` / `PhysicalPlan` 构建后不可变（`Arc`），执行期状态（hash table、spill 文件、runtime filter）放在算子实例内。
3. **所有代价与统计量可注入**：统计、代价常数、选择性一律经 `OptimizerConfig` / `StatisticsProvider` 获取，禁止硬编码（ADR-5）。
4. **计划可序列化**：`PhysicalPlan` 与 `DistributedPlan` 可编码为 protobuf/JSON，服务 Plan Cache、Replayer、跨节点 Task 分发。
5. **观测点内建**：计划节点自带 `id / est_rows / cost / rule_trace`，执行节点自带 `actual_rows / time / memory / spill`。

---

## 2. 核心数据结构

### 2.1 类型与值（`types` crate）

```rust
enum DataType {
    Null, Bool,
    Int8, Int16, Int32, Int64, UInt8, UInt16, UInt32, UInt64,
    Float32, Float64, Decimal { precision: u8, scale: u8 },
    Date, Time, DateTime, Timestamp, Duration, Year,
    Char(u32), VarChar(u32), Binary(u32), VarBinary(u32),
    Json, Enum, Set, Bit, Vector(u32),             // P2
}

/// 列式批：对齐 Arrow 语义，列存 + 可选 selection vector
struct Block {
    columns: Vec<Column>,        // 每列：type + validity bitmap + values buffer
    selection: Option<Vec<u32>>, // 延迟物化的行号索引
    num_rows: usize,
    capacity: usize,             // 默认 1024 / 可配，见 05 §5.4 标定
}

struct Column { data_type: DataType, validity: BitVec, buffer: Buffer }
enum Scalar { Null, Bool(bool), Int(i128), Float(f64), Decimal(Decimal),
              Bytes(Bytes), Date(Date), Time(Time), Json(Json), ... }
```

**NULL / 三值逻辑**：`Predicate` 求值返回 `Bool3 { True, False, Unknown }`；`Filter` 仅保留 `True`；`NOT IN` 的 Null-Aware 语义由专门算子承载（04 `OP-P-NAAJ`）。

### 2.2 表达式（`expr` crate）

```rust
enum Expr {
    Column(ColumnRef),                 // 由 Binder 绑定为唯一 id
    Literal(Scalar, DataType),
    Function { sig: FuncSig, args: Vec<Expr>, ret_type: DataType, nullable: bool },
    Cast { expr: Box<Expr>, from: DataType, to: DataType, in_union: bool },
    Case { when: Vec<(Expr, Expr)>, then_else: Option<Box<Expr>> },
    Subquery(QueryRef),                // 去关联后应被消除
    AggCall { func: AggKind, args: Vec<Expr>, distinct: bool, filter: Option<Box<Expr>> },
    WindowFunc { func: WindowKind, partition_by: Vec<Expr>, order_by: Vec<SortItem>, frame: Frame },
    Param(u32),                        // Prepared 占位符
}

trait ScalarEvaluator {
    fn eval(&self, expr: &Expr, block: &Block, sel: &SelectionVector) -> Result<Column>;
}
```

表达式注册表（`FuncRegistry`）集中定义：`deterministic / nullable-propagation / 可下推标记 / SIMD 实现 / JIT 实现 / 代价`，下推判定依赖该元数据（§6）。

### 2.3 计划节点（`planner` / `optimizer` crate）

```rust
struct PlanNodeId(u64);

/// 逻辑算子（04 §4.1 OP-L-*），关系代数语义，无算法与顺序
enum LogicalPlan {
    DataSource(DataSource), Projection(Projection), Selection(Selection),
    Join(Join { join_type: JoinType, condition: Expr, ... }), Apply(Apply),
    Aggregation(Aggregation), Window(Window), Sort(Sort), TopN(TopN), Limit(Limit),
    Distinct(Distinct), Union(SetOp), Intersect(SetOp), Except(SetOp),
    Cte { producer: Arc<LogicalPlan>, consumer: CteRef, materialized: bool },
    Dml(Dml { kind: Insert|Update|Delete|Replace, target: TableRef }),
    Ddl(Ddl), Lock(SelectLock), TableDual, MaxOneRow, Expand(GroupingSets),
    MemoGroup(GroupId),                       // 仅 CBO 内部
}

struct LogicalPlanNode {
    id: PlanNodeId,
    plan: LogicalPlan,
    children: Vec<Arc<LogicalPlanNode>>,
    schema: Schema,                           // 输出列（列裁剪后收缩）
    stats: Option<Statistics>,                // 估计行数/宽度
    rule_trace: Vec<RuleHit>,                 // ADR-6
}

/// 物理算子（04 §4.2 OP-P-*） + 代价与属性
struct PhysicalPlanNode {
    id: PlanNodeId,
    op: PhysicalOp,
    children: Vec<Arc<PhysicalPlanNode>>,
    schema: Schema,
    stats: Statistics,                        // est_rows / est_width / NDV
    cost: Cost,                               // 分解：cpu/io/net/mem
    required_property: Property,
    delivered_property: Property,
    chosen_reason: ChosenReason,              // 该 Winner 为何胜出（可观测）
}

struct Property {
    ordering: Vec<SortItem>,                  // interesting order
    distribution: Distribution,              // Single / Hash(keys) / Range / Broadcast / Any
    partitioning: Option<PartitionSpec>,
}
```

### 2.4 统计信息与代价（`optimizer::stats`）

```rust
struct Statistics { row_count: f64, row_width: f64, col_stats: Vec<ColStats>, ... }
struct ColStats {
    ndv: f64, null_fraction: f64, avg_width: f64,
    histogram: Option<Histogram>,     // EquiHeight / EquiWidth / Singleton / Hybrid
    top_n: Vec<(Scalar, u64)>, cmsketch: Option<CMSketch>, correlation: f64,
}
struct Histogram { buckets: Vec<Bucket{lower, upper, count, ndv}>, kind: HistKind, ... }

struct Cost { cpu: f64, io: f64, net: f64, mem: f64,
              fn total(&self, w: &CostWeights) -> f64 { ... } }
struct CostWeights { cpu: f64, io: f64, net: f64, mem: f64 }   // 标定值，见 05 §5.3
```

`CostWeights`、算子代价常数、`broadcast_threshold`、`chunk_capacity`、`spill_threshold` 全部经 `OptimizerConfig` 注入，支持「同一查询 × 不同配置」的消融矩阵。

### 2.5 Memo 结构（Cascades 实现）

```rust
struct Memo {
    groups: Vec<Group>,                    // 逻辑等价类
    exprs: Vec<GroupExpression>,          // 组内表达式（逻辑或物理）
    root: GroupId,
}
struct Group {
    id: GroupId,
    logical_exprs: Vec<ExprId>,
    physical_exprs: Vec<ExprId>,
    winner: Option<Winner>,               // 按 (property, upper_bound) 缓存
    stats: Statistics, lower_bound: f64,  // B&B 剪枝
}
struct Winner { expr: ExprId, cost: Cost, property: Property }
```

`ImplementationRule` 产出物理表达式，`EnforcerRule` 插入 `Sort/Exchange`；搜索用 `Branch-and-Bound` + 支配剪枝（同 property 下 cost 更劣者剪除）。

### 2.6 分布式计划（`executor` / `meta`）

```rust
struct DistributedPlan {
    fragments: Vec<PlanFragment>,
    exchanges: Vec<ExchangeEdge>,
}
struct PlanFragment {
    id: FragmentId,
    root: Arc<PhysicalPlanNode>,
    execution_mode: ExecMode,      // Local | CopTask | MppTask
    parallelism: u32,
    target: Placement,             // Store / Sql / Column / Any
}
enum ExchangeSpec {
    Gather, Broadcast, HashShuffle { keys: Vec<Expr>, partitions: u32 },
    RangeShuffle { keys: Vec<Expr> }, MergeSort { keys: Vec<SortItem> }, PassThrough,
}
struct Task { fragment_id: FragmentId, encoded_plan: Bytes, region_epochs: Vec<RegionEpoch>,
              ts: Timestamp, resource_group: ResourceGroup, deadline: Duration }
```

---

## 3. 模块接口契约（trait 边界）

```rust
// ---- 解析与绑定 ----
trait Parser { fn parse(&self, sql: &str) -> Result<Vec<AstStatement>>; }
trait Binder {
    fn bind(&self, stmt: &AstStatement, ctx: &SessionCtx) -> Result<LogicalPlanNode>;
}
trait CatalogProvider {
    fn table(&self, name: &QualifiedName, schema_version: SchemaVersion) -> Result<Arc<Table>>;
    fn index(&self, table: TableId, idx: IndexId) -> Result<Arc<Index>>;
    fn stats(&self, table: TableId) -> Result<Arc<TableStats>>;
}

// ---- 优化 ----
trait Rule { fn name(&self) -> &str; fn pattern(&self) -> Pattern;
             fn apply(&self, p: &LogicalPlanNode, ctx: &OptCtx) -> Result<Option<LogicalPlanNode>>; }
trait RboOptimizer { fn optimize(&self, plan: LogicalPlanNode) -> Result<LogicalPlanNode>; }
trait StatisticsProvider { fn selectivity(&self, expr: &Expr, input: &Statistics) -> Result<f64>; }
trait CardinalityEstimator { fn estimate(&self, node: &LogicalPlanNode) -> Result<Statistics>; }
trait CostModel { fn cost(&self, op: &PhysicalOp, input: &[Statistics]) -> Result<Cost>; }
trait CboOptimizer { fn optimize(&self, plan: LogicalPlanNode) -> Result<PhysicalPlanNode>; }
trait PlanPostOptimizer {  // Cop 下推 / Exchange 插入 / 并行度
    fn distribute(&self, p: PhysicalPlanNode) -> Result<DistributedPlan>;
}

// ---- 执行 ----
trait Executor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn next_chunk(&mut self) -> Result<Option<Block>>;   // 火山 + 向量化
    fn close(&mut self) -> Result<ExecStats>;
    fn required_property(&self) -> Property;
}
trait ExecutorBuilder { fn build(&self, p: &PhysicalPlanNode, ctx: &ExecCtx) -> Result<Box<dyn Executor>>; }

// ---- 存储与事务 ----
trait KvEngine {
    fn get(&self, key: &[u8], ts: Timestamp) -> Result<Option<Bytes>>;
    fn scan(&self, range: Range, ts: Timestamp, limit: usize) -> Result<BlockStream>;
    fn prewrite(&self, mutations: &[Mutation], primary: &[u8], start_ts: Timestamp) -> Result<()>;
    fn commit(&self, keys: &[&[u8]], start_ts: Timestamp, commit_ts: Timestamp) -> Result<()>;
    fn coprocessor(&self, req: DagRequest) -> Result<BlockStream>;
}
trait TimestampOracle { fn get_ts(&self, n: u32) -> Result<Timestamp>; }
trait RegionRouter { fn locate(&self, key: &[u8]) -> Result<RegionMeta>; }
trait Transaction { fn begin(&mut self, mode: TxnMode) -> Result<Timestamp>;
                    fn commit(&mut self) -> Result<Timestamp>; fn rollback(&mut self) -> Result<()>; }
```

**依赖方向（单向，CI 静态检查）**：
`server → executor → optimizer → planner → expr → types → common`；`storage/meta` 经 `trait` 注入到 `executor`，禁止反向引用。

---

## 4. KV 编码规范

### 4.1 保序编码（memcomparable）

| 值类型 | 编码 | 性质 |
| --- | --- | --- |
| `Int64` | 符号位翻转 + 8B 大端 | 字节序 == 数值序 |
| `Uint64` | 8B 大端 | 同上 |
| `VarChar/Bytes` | 分组 `8B 数据 + 1B 标记(0xFF 表示满组，否则 = 有效长度)`，末尾 `0x00` 补位 | 前缀可比、可拼接 |
| `Decimal` | 归一化指数 + 保序尾数 | 精度无损 |
| `Float` | 符号翻转 + 指数/尾数编码 | 近似保序 |
| `NULL` | 前缀 `0x00`，非 NULL `0x01` | NULL 排前 |

### 4.2 Key 布局

```
TablePrefix  = 't' | EncodeInt64(table_id)
RecordKey    = TablePrefix | "_r" | Encode(handle)                       // int handle / common handle
IndexKey     = TablePrefix | "_i" | EncodeInt64(index_id)
               | Encode(idx_cols...) | [Encode(handle)]                   // 非唯一索引必带 handle
MetaKey      = 'm' | ... (schema / ddl job / stats / region meta)
TxnKey(user) = user_key | EncodeDesc(start_ts)                            // 降序：最新版本优先
```

`IndexLookUp` 的回表：由 `IndexKey` 解析出 `handle` → 构造 `RecordKey` → `BatchGet`；`IndexMerge` 对多个索引结果做交/并后再回表（04 `F-IDX-006`）。

### 4.3 MVCC 三 CF

| CF | Key | Value | 用途 |
| --- | --- | --- | --- |
| `default` | `user_key @ start_ts` | 原始值 | 大值分离，减少 write CF 放大 |
| `lock` | `user_key` | `{ primary, start_ts, ttl, txn_size, lock_type, for_update_ts }` | 未决事务 |
| `write` | `user_key @ commit_ts` | `{ write_type: Put/Delete/Lock/Rollback, start_ts }` | 可见版本索引 |

**读流程**：`seek(user_key @ start_ts)` 找到 ≤ start_ts 的最新 `write` → 取 `start_ts` → 读 `default`；若遇 `lock` 且 `lock.ts < start_ts` → `ResolveLock`（查 primary 状态，提交/回滚）。**GC safe point** 以下版本由 `C-TXN` 定期清理，且必须保证不删除任何活跃快照所需版本（05 V3 一致性用例）。

---

## 5. SQL 全生命周期：分阶段契约

| 阶段 | 输入 | 输出 | 失败语义 | 可观测 |
| --- | --- | --- | --- | --- |
| S1 Parse | SQL 文本 | `AstStatement` | 语法错 + 位置 | `AST_DUMP` |
| S2 Bind | AST + Catalog(schema_version) | `LogicalPlanNode` + `Schema` | 列/表不存在、类型不兼容、权限拒绝 | `BINDED_PLAN` |
| S3 FastPlan 判定 | LogicalPlan | 可选 `PointGet/BatchPointGet` | — | `FAST_PLAN_HIT` |
| S4 RBO | LogicalPlan | 规范化 LogicalPlan（不动点） | 规则不收敛 → 超轮数报错 | `OPTIMIZE_TRACE` 规则命中/diff |
| S5 统计加载 | 表名/列 | `Statistics` | 缺失则退化为默认选择性（记录告警） | `STATS_MISS` |
| S6 CE | LogicalPlan + Stats | 带 `est_rows` 的计划 | — | `ESTIMATE_LOG` |
| S7 CBO 搜索 | LogicalPlan + Stats | `PhysicalPlanNode`（Winner） | 超时 → 回退 DP/greedy（记录） | `MEMO_DUMP / COST_BREAKDOWN / PRUNE_STAT` |
| S8 Post-Opt | PhysicalPlan | `DistributedPlan`（Cop/Exchange/Fragment） | 下推失败 → 回退上层执行 | `PUSHDOWN_LOG` |
| S9 PlanCache | 参数化 SQL | 命中或插入 | 参数敏感 → 跳过缓存 | `CACHE_HIT/MISS` |
| S10 Build | DistributedPlan | `Box<dyn Executor>` | — | — |
| S11 Txn | — | `start_ts` / `commit_ts` | 写冲突 → 重试/报错 | `RETRY_LOG / LOCK_RESOLVE` |
| S12 Execute | Chunk 流 | 结果集 | 算子错误、OOM→Spill、RPC 重试 | `EXPLAIN ANALYZE`、`tracing span` |
| S13 Finish | 结果 | 客户端 / slow log / metrics | — | `SLOW_QUERY / QPS / P99` |

**参数化与缓存键**：`cache_key = normalize(sql_literal) + schema_version + session_vars(影响计划的) + stats_version`；任一变化即失效（04 `F-PLAN-006`）。

---

## 6. Coprocessor 下推契约

### 6.1 可下推矩阵

| 算子 / 表达式 | 可下推 | 条件 / 边界 |
| --- | --- | --- |
| `Selection` | ✅ | 表达式全部在 `FuncRegistry::pushdown_allowed` 白名单；不含子查询、非确定性函数（`RAND/NOW` 需折叠后判定）、UDF |
| `Aggregation` | ✅（partial） | 支持 `COUNT/SUM/MIN/MAX/AVG/APPROX_COUNT_DISTINCT` 的 partial + 上层 final 两阶段；`DISTINCT` 需上层去重 |
| `TopN / Limit` | ✅ | Region 局部 TopN + 上层 MergeSort/TopN 归并；必须处理并列与全局正确性 |
| `Projection` / 表达式 | ✅ | 纯标量表达式；`CAST/DECIMAL/COLLATION` 语义与上层一致（差分测试重点，05 AC-OP） |
| `TableScan / IndexScan` | ✅ | 带 key range、列裁剪、谓词 ICP |
| `Join / Window / SetOp / Sort(global)` | ❌ | 跨 Region 状态，留在 SQL 层 / MPP |
| `SELECT … FOR UPDATE` | ❌ | 锁需在事务协调层 |
| 列存副本 | ✅（MPP） | 走 `C-COL-MPP` 而非 Cop |

### 6.2 协议与一致性

```protobuf
message DagRequest {
  uint64 start_ts = 1;
  repeated ExecutorSpec executors = 2;   // 逆波兰式算子 DAG（TiDB 风格）
  repeated ColumnInfo output_columns = 3;
  repeated KeyRange ranges = 4;          // 按 Region 切分
  uint64 schema_version = 5;
  bool    analyze_mode = 6;              // EXPLAIN ANALYZE 实际行数回传
}
```

- 每个 `KeyRange` 绑定 `RegionEpoch`；`EpochNotMatch` → 刷新路由并重发该 range（幂等）。
- 下推与上层**共用同一 `Block` 格式与表达式求值实现**（`coprocessor` crate 复用 `expr`），避免语义漂移；差分测试专门覆盖「同查询 下推开/关」结果一致（05 AC-PLAN-005）。
- 下推决策由 CBO 用代价判定（收益 = 减少返回行数 × 网络单位代价 − 存储 CPU 代价），而非硬编码规则；`AGG_TO_COP` / `READ_FROM_STORAGE` Hint 可覆盖（ADR-5/6）。

---

## 7. 事务执行概要（2PC / Percolator）

```
Client                SQL/Txn               Meta(TSO)        Store(Leader)
  |-- BEGIN ------------>|                     |                 |
  |                      |-- GetTS ---------->|                 |
  |                      |<-- start_ts -------|                 |
  |-- DML -------------->|  (缓冲 mutations, 选 primary)         |
  |-- COMMIT ----------->|                                       |
  |                      |-- Prewrite(primary+secondaries) ----->|
  |                      |  检查: write@[start_ts,∞] 冲突 / lock  |
  |                      |<-- OK --------------------------------|
  |                      |-- GetTS ---------->|                 |
  |                      |<-- commit_ts ------|                 |
  |                      |-- Commit(写 write / 删 lock) -------->|
  |                      |<-- OK --------------------------------|
  |<-- OK(commit_ts) ----|  (primary 提交即成功；secondary 异步/读时 Resolve) |
```

| 机制 | 概要 |
| --- | --- |
| 乐观事务 | 写写冲突在 Prewrite 检测；适合低冲突，冲突代价 = 回滚 |
| 悲观事务 | `PessimisticLock` 在 DML 阶段加锁，`for_update_ts` 保证读到最新已提交 |
| 锁解析 | 读遇锁 → 查 primary write：已提交则补写 secondary 并继续；未提交/TTL 过期则 Rollback |
| 1PC 优化 | 单 Region 单批次事务合并为一次 RPC（04 `F-TXN-009`） |
| 大事务 | `txn_size` 限制 + 分批次 Prewrite + TTL 续期（`keep_alive`） |
| 隔离 | 默认 SI；RC（每条语句新 start_ts）；SSI 冲突图检测为 `[研究向]` |
| GC | `safe_point = min(active_start_ts) − gc_lifetime`，按 Region 并发清理 |

---

## 8. DDL 与 Schema 版本（Online Schema Change）

- **状态机**：`None → WriteOnly → WriteReorganization → DeleteOnly → Public`（对标 TiDB / F1 在线变更协议），保证任意相邻两个版本共存时读写均正确。
- **Schema Lease**：所有节点持有带 lease 的 `schema_version`；DDL 每个状态推进需等待 ≥ 2×lease（或主动同步确认）以确保旧版本退出。
- **DDL Job 队列**：持久化、可回滚、幂等重放；`ADD INDEX` 通过 `WriteReorganization` 回填 + 增量同步。
- **计划缓存失效**：`schema_version` 变化使所有相关 `cache_key` 失效（§5）。

---

## 9. 统计信息收集（C-OPT-STAT）

| 项 | 概要 |
| --- | --- |
| 触发 | `ANALYZE TABLE`、自动后台（modify_count 阈值）、首次导入后 |
| 采样 | Reservoir（行级）/ Bernoulli（块级），固定 `seed` 保证复现 |
| 构建 | 全量或采样算 NDV（HyperLogLog/精确去重）、等高频 TopN、等高直方图、CMSketch |
| 存储 | 元数据 KV，带 `stats_version` 与 `modify_count`，供缓存失效 |
| 过期判定 | `modify_count / row_count > threshold`（默认 `[待标定]` 0.1）触发重收集 |
| 反馈闭环 | `EXPLAIN ANALYZE` 的 `actual_rows` 与 `est_rows` 落库，用于 q-error 监控与选择性修正 `[研究向]` |
| 实验支持 | 支持注入「人为扰动统计」以测 CE 鲁棒性（05 RQ-2） |

---

## 10. 并发与资源模型

| 层级 | 机制 |
| --- | --- |
| 连接 | tokio task per connection，Session 状态隔离 |
| 查询 | 一个查询 = 一棵算子树的 `Executor`；`Morsel-driven` 把 Chunk 分派到 `rayon`/tokio 线程池，按算子设并行度 |
| 内存 | 全局 `MemoryTracker` + 查询级配额；HashJoin/Agg/Sort 达阈值 → Spill 到临时文件（带 IO 计量），超额 → 按策略终止并报错 |
| 网络 | gRPC streaming 传 Chunk；Exchange 用有界队列 + 背压；Broadcast 限制 `broadcast_threshold` |
| 重试 | Region 路由错误、RPC 超时、写冲突 → 分类退避重试（次数/延迟可配），全部计入 metrics |
| 取消 | `CancellationToken` 贯穿算子与 RPC，客户端断连即释放资源 |

---

## 11. Cargo Workspace 与 Lab 落点

| Crate | 主要 Lab | 关键 trait / 产物 |
| --- | --- | --- |
| `common` | L0 | `Result`, config, tracing, metrics, `Block` 基础工具 |
| `types` `expr` | L1, L10 | `DataType`, `Block`, `FuncRegistry`, `ScalarEvaluator` |
| `parser` `catalog` | L1, L5 | `Parser`, `Binder`, `CatalogProvider` |
| `planner` | L2 | `LogicalPlanNode`, `Schema`, `Rule` |
| `optimizer` (rbo) | L6 | `RboOptimizer`, `Rule` 集合, `OPTIMIZE_TRACE` |
| `optimizer` (stats/cbo) | L7, L8 | `StatisticsProvider`, `CardinalityEstimator`, `CostModel`, `Memo`, `CboOptimizer` |
| `executor` | L3, L10, L12 | `Executor`, 全部 `OP-P-*`, `OP-E-*`, `ExecutorBuilder` |
| `coprocessor` | L9, L12 | `DagRequest`, 下推求值（复用 `expr`） |
| `storage` `raftstore` | L4, L11 | `KvEngine`, MVCC, Region/Raft |
| `meta` | L11 | `TimestampOracle`, `RegionRouter`, scheduler |
| `server` | L5, L9 | MySQL wire, Session, `EXPLAIN`, PlanCache |
| `columnar` | L13 | Learner, 列存, MPP |
| `bench` `tests` | 全阶段 | sqllogictest, TPC, SQLancer, 混沌, 可复现脚本 |

---

## 12. 配置与实验注入点（`[待标定]` 汇总）

以下参数在 05 §5 中通过标定实验确定默认值，全部可经环境变量 / `bench/env.toml` / Session 变量覆盖：

| 参数 | 含义 | 影响 | 标定实验 |
| --- | --- | --- | --- |
| `chunk_capacity` | 向量化批大小 | CPU cache / 函数调用摊销 | 05 §5.4 |
| `cost_weights.{cpu,io,net,mem}` | 代价权重 | 计划选择 | 05 §5.3 回归标定 |
| `broadcast_threshold` | 广播上限字节 | Join 策略 | 05 §5.5 |
| `spill_threshold` / `memory_quota` | 降级阈值 | HashJoin/Agg/Sort | 05 AC-OP-021 |
| `default_selectivity` | 统计缺失默认选择性 | 计划质量 | 05 §5.2 |
| `stats_stale_ratio` | 统计过期阈值 | 自动 Analyze | 05 AC-CBO |
| `max_rbo_iterations` | RBO 不动点上限 | 规划耗时/收敛 | 05 AC-RBO |
| `cbo_timeout` / `join_reorder_limit` | 搜索预算 | 大查询规划时间 | 05 RQ-1 |
| `parallelism_default` | 默认 Fragment 并行度 | MPP 扩展 | 05 V4 |

---

## 13. 错误模型与可诊断性

```rust
enum ErrorKind {
    Parse, Bind, TypeMismatch, Permission, PlanRegression, OptimizerTimeout,
    RegionEpochNotMatch, NotLeader, ServerIsBusy, KeyIsLocked, WriteConflict,
    CommitTsExpired, TxnTooLarge, OutOfMemory, SpillFailed, RpcTimeout,
    SchemaChanged, StatsStale, Internal,
}
struct Error { kind: ErrorKind, msg: String, backtrace: Backtrace,
               context: DiagnosticContext /* sql_id, plan_id, region_id, ts, retry_count */ }
```

- 所有可重试错误带 `retryable` 标记与分类退避策略；`KeyIsLocked` 走 `ResolveLock` 而非直接失败。
- 任一错误可关联到 `sql_id / plan_id / EXPLAIN / trace`，满足 Plan Replayer 复现（04 `F-PLAN-009`）。

---

## 14. 概要设计检查清单（进入实现前逐项确认）

- [ ] 所有 `trait` 有 `mock` 实现，单测不依赖真实 RocksDB / 网络（05 V0）
- [ ] `LogicalPlan` / `PhysicalPlan` / `DistributedPlan` 均可 JSON/protobuf 序列化
- [ ] 每个物理算子声明 `required/delivered_property` 与 `cost` 公式
- [ ] 每条 RBO 规则带等价性证明或反例测试编号
- [ ] 代价常数、统计、选择性无任何硬编码（静态 grep 检查）
- [ ] 下推与上层共用 `expr` 求值实现，差分测试覆盖「下推开/关」
- [ ] 所有实验配置经 `OptimizerConfig` 注入且记录到实验结果元数据
- [ ] 追溯链 `F-* → L* → M* → AC-*` 在 04/05 中闭合
