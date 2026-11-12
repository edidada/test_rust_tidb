# 06 分布式功能测试计划（Distributed Feature Test Plan）

> 状态：`[设计]` ｜ 版本：v0.1 ｜ 上游：01 架构、02 概要设计、03 排期、04 功能点列表、05 验收规划
> 下游：L11 / L12 / L13 / L14 实现与 M5 / M6 / M7 门禁；本文是分布式功能测试的**执行清单与排期依据**，不改动 01–05 的既有编号。
>
> 核心原则：**每个分布式功能点有测试项、每个测试项有量化通过标准、每个结论有可复现实验**（与 05 §1 一致）。本文测试项统一编号为 `DT-<域>-NNN`，测试阶段编号为 `DTP-1..7`，全部引用既有 `AC-* / F-* / L* / M* / V*` 形成追溯链。

---

## 1. 范围与目标

### 1.1 覆盖范围

| 维度 | 范围 | 引用 |
| --- | --- | --- |
| 阶段 | P3 执行与分布式（L10–L13）、P4 实验与论文（L14–L15） | 03 §2 |
| 里程碑 | **M5**（W49，分布式与 HTAP 可用）、**M6**（W53，全量验收）、**M7**（W60，结题） | 03 §4 |
| 验收等级 | **V3**（分布式与一致性）、**V4**（性能/扩展）、**V5**（可复现/长稳） | 05 §2 |
| 功能域 | `F-STO-011..024`、`F-TXN-009..016`、`F-META-*`、`F-DDL-*`、`F-EXEC-010..022`、`OP-E-*`、`F-HTAP-*`、`F-BENCH-*`、`F-TEST-011..020` | 04 §8 |
| 验收基线 | 05 §8 分布式/事务/一致性验收矩阵、AC-TXN / AC-STO / AC-EXEC / AC-META / AC-HTAP / AC-BENCH | 05 §4、§8 |

### 1.2 非目标（明确裁剪）

- 不做跨地域多活 / 异地容灾的测试（01 §1.2 非目标）。
- 不测试生产级多租户、计费、云管控（01 §1.2）。
- 不将单节点 V0–V2 门禁内容重复纳入（05 §2 已覆盖，CI 每次提交执行）。
- 跨系统裸比禁止作为性能结论（05 §9.2）；TiDB / CockroachDB / DataFusion 仅作机制对照。

### 1.3 测试出口标准（Definition of Done）

1. 全部 `DT-*` 测试项状态为「已通过」，且至少一条 `AC-*` 可追溯。
2. M5 门禁三项（分布式正确性 / 一致性 / 性能）达标：RTO ≤ 10s、RPO = 0、线性一致性违例 0、4 节点 MPP 加速 ≥ 2.8×、下推开/关结果一致。
3. 所有注入类测试（故障 / 分区 / 时钟 / 磁盘）可由确定性模拟器按种子精确重放（F-TEST-016）。
4. 全部 V5 结论按 05 §9 归档：固定种子、≥5 次重复、95% 置信区间、第二人独立重跑偏差 ≤ 5%。

---

## 2. 测试环境与拓扑（环境固化，05 §9.1 / AC-BENCH-008）

| 项 | 规格 | 固化方式 |
| --- | --- | --- |
| 集群形态 | `cluster`，3–8 节点（默认 4：3 Store + 1 SQL/Meta），可扩缩至 8 | `bench/env.toml@<sha>` |
| 节点 | ≥16C / 32G / NVMe，网络 10Gbps 等价 | Dockerfile + 容器镜像 |
| OS / 工具链 | Linux（RK-3：RocksDB/raft-rs 统一容器），Rust 版本锁定 | 03 §6 RK-3、05 §7 |
| 依赖版本 | raft-rs / RocksDB binding / tonic 版本入档 | `Cargo.lock` + 实验元数据 |
| 种子 | 主种子 `20261109`（common `SeedPolicy`），子流按组件派生 | AC-BENCH-010 |
| 数据集 | TPC-H / TPC-DS / TPC-C / sysbench / YCSB + skew 集 | 04 F-BENCH-* |
| 观测 | Metrics / Tracing / Slow Log / `EXPLAIN ANALYZE` / `OPTIMIZE_TRACE` / 混沌日志 | F-OBS-* |

> 所有测试必须记录 `commit / seed / repeat / env / config / dataset / baseline`（05 §9.1）。任一 `DT-*` 失败即冻结相关里程碑（05 §10.3）。

---

## 3. 测试阶段 `DTP-*`（与 03 排期对齐）

| 阶段 | 周次 | 依赖 Lab | 主题 | 出口 |
| --- | --- | --- | --- | --- |
| **DTP-1** | W39–W42 | L11（并行 L10） | 分布式存储基础：Region / Raft / TSO / 2PC 单元与集成 | 单 Region / 多 Region 功能测试通过 |
| **DTP-2** | W43–W46 | L12 | Cop 下推 / MPP / Exchange / AQE 功能与结果一致性 | 下推开/关结果一致、MPP 正确性 |
| **DTP-3** | W47–W49 | L13 | 列存 HTAP / 行列自动选择 / 一致性读 | **M5 门禁** |
| **DTP-4** | W49–W50 | L11–L13 | 故障注入 / 混沌 / 一致性（V3） | 05 §8 矩阵 100% |
| **DTP-5** | W50–W52 | L14 | 基准 / 消融 / 差分模糊 / 性能扩展（V4） | 量化阈值达标 + 置信区间 |
| **DTP-6** | W52–W53 | L14 | 72h 长稳 / 可复现 artifact / 全矩阵（V5） | **M6 门禁** |
| **DTP-7** | W54–W60 | L15 | 研究拓展 RQ / 论文图表复核 | **M7 门禁** |

---

## 4. 分布式功能测试矩阵 `DT-*`

> 字段：**依赖**为前置 Lab / 测试阶段；**方法**为注入或验证手段；**通过标准**为量化阈值；**追溯**为 `AC-* / F-* / V*`。
> 编号按域分组，新增项从各域最大序号 +1 开始（与 05 §12 一致，不重排既有编号）。

### 4.1 DT-STO · 分布式存储与 Region（DTP-1）

| 编号 | 测试项 | 依赖 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- | --- |
| DT-STO-001 | Region 定义 / epoch / 边界路由 | L11 | 构造边界 key + epoch 变更 | 路由 100% 正确 | AC-STO-005, F-STO-011, V3 |
| DT-STO-002 | Raft Group 多副本日志复制 | L11 | 3 副本写入 + 日志比对 | 副本一致，无丢失 | AC-STO-006, F-STO-012, V3 |
| DT-STO-003 | Leader Lease / ReadIndex 线性读 | L11 | 并发读 + lease 边界 | 线性一致性违例 0 | AC-STO-006, F-STO-013, V3 |
| DT-STO-004 | Region Split / Merge | L11 | 并发事务下 Split + 随机 kill | 数据一致，恢复后无损坏 | AC-STO-007, F-STO-014, V3 |
| DT-STO-005 | Snapshot / 日志压缩 / Apply 状态机 | L11 | 触发快照 + 重启 | 状态机重放一致 | AC-STO-007, F-STO-015, V3 |
| DT-STO-006 | Store 心跳与状态上报 | L11 | 模拟心跳丢失 | 调度器正确感知 | AC-META-004, F-STO-016, V3 |
| DT-STO-007 | 副本校验 / checksum / 静默损坏 | L14 | 注入 bit flip + 主从比对 | 不一致检出率 100% | AC-STO-008, F-STO-017, V3 |
| DT-STO-008 | Coprocessor gRPC / DagRequest 协议 | L12 | 协议 fuzz + 上下层对拍 | 与上层 `expr` 结果 100% 一致 | AC-STO-009, F-STO-018/019, V3 |
| DT-STO-009 | 存储侧向量化求值 / Selection / Agg / TopN | L12 | 复用 `expr` 差分 | 上下层结果一致 | AC-STO-009, F-STO-020/021, V3 |
| DT-STO-010 | Range 按 Region 切分与部分结果重试 | L12 | 跨 Region range + 注入重试 | 无重复 / 无遗漏 | AC-EXEC-016, F-STO-022, V3 |
| DT-STO-011 | SST 批量导入 / 存储指标 | L14 | 长稳 + 批量导入 | 版本清理安全、指标齐全 | AC-STO-010, F-STO-023/024, V3 |
| DT-STO-012 | RocksDB 重启与 CF 隔离 | L4/L11 | 写盘 → 重启 → 读 | 数据不丢、CF 隔离正确 | AC-STO-002, F-STO-003, V2 |

### 4.2 DT-TXN · 分布式事务与一致性（DTP-1 / DTP-4）

| 编号 | 测试项 | 依赖 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- | --- |
| DT-TXN-001 | 分布式 2PC + primary/secondary 原子性 | L11 | 跨 Region 事务 1000 例 + 中间态 kill | 原子性 100% 通过 | AC-TXN-004, F-TXN-010, V3 |
| DT-TXN-002 | 1PC 单 Region 优化 / 只读优化 | L11 | 单 Region 事务 + 只读事务 | 原子性与优化路径正确 | AC-TXN-004, F-TXN-009/018, V3 |
| DT-TXN-003 | 悲观事务 / `SELECT … FOR UPDATE` | L11 | 锁竞争 + 死锁注入 | 死锁 100% 检出并回滚 | AC-TXN-003/005, F-TXN-006/017, V3 |
| DT-TXN-004 | 写写冲突检测与自动重试 | L11 | 并发冲突注入 | 无活锁，冲突 100% 检出 | AC-TXN-005, F-TXN-011, V3 |
| DT-TXN-005 | ResolveLock / TTL / 大事务分批 | L11 | 遗留锁 + 100 万行大事务 | 锁遗留 0，大事务成功 | AC-TXN-006, F-TXN-012/013/014, V3 |
| DT-TXN-006 | GC safe point 与活跃快照 | L11 | 长事务 + GC 并发 | 不删除任何活跃快照所需版本 | AC-TXN-007, F-TXN-015/F-STO-010, V3 |
| DT-TXN-007 | 线性一致性（Porcupine / Jepsen 风格） | L11/L14 | 历史记录检查器 | 违例 0 | AC-TXN-008, F-TEST-018, V5 |
| DT-TXN-008 | SI / SSI 异常注入（写偏斜） | L11/L15 | TLP / NoREC / PQS + 写偏斜 | 检出率 100%，SSI 违例 0 | AC-TXN-002/008, F-TXN-019, V5 |
| DT-TXN-009 | Async Commit / 1PC 延迟优化 | L15 | 提交延迟对比 | 无正确性回归，延迟可量化 | AC-TXN-008, F-TXN-020, V5 |

### 4.3 DT-META · 元数据与调度（DTP-1 / DTP-4）

| 编号 | 测试项 | 依赖 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- | --- |
| DT-META-001 | TSO 批量分配与 HA 主备切换 | L11 | 100 万次 GetTS + 切换 | 严格单调，切换后不回退 | AC-META-001, F-META-001/002, V2 |
| DT-META-002 | Region 路由缓存与重试 | L11 | EpochNotMatch / NotLeader 注入 | 重试后 100% 成功 | AC-META-002, F-META-003/004, V3 |
| DT-META-003 | 元数据自身 Raft HA | L11 | Meta 节点 kill | 无脑裂，服务连续 | AC-META-003, F-META-005, V3 |
| DT-META-004 | 副本 / Leader 均衡、热点打散、补齐下线 | L11 | 热点注入 + 节点下线 | 调度收敛，无数据丢失 | AC-META-004, F-META-006/007/008, V3 |
| DT-META-005 | Split 决策与 Operator 队列 | L11 | 高压写入 + 调度限流 | 队列无堆积雪崩 | AC-META-005, F-META-009, V3 |

### 4.4 DT-DDL · 在线 DDL 与 Schema（DTP-1）

| 编号 | 测试项 | 依赖 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- | --- |
| DT-DDL-001 | CREATE/DROP DATABASE/TABLE、TRUNCATE/RENAME | L11 | DDL 并发 DML | 元数据一致，无悬挂 | AC-DDL-001, F-DDL-001/009, V2 |
| DT-DDL-002 | Online DDL 状态机（WriteOnly→…→Public） | L11 | 各状态注入 kill | schema 版本与 lease 正确 | AC-DDL-002, F-DDL-003/004/005, V3 |
| DT-DDL-003 | DDL Job 队列 / ADD INDEX 回填 | L11 | 幂等 / 回滚 / 重放 | 回填无丢失，增量一致 | AC-DDL-003, F-DDL-006/007, V3 |
| DT-DDL-004 | 元数据锁与 DDL/DML 并发 | L11 | 并发交错注入 | 无死锁，约束 100% 正确 | AC-DDL-004, F-DDL-008/010, V3 |
| DT-DDL-005 | information_schema / SHOW / 元数据升级 | L5/L11 | 版本升级回放 | 兼容，查询正确 | AC-DDL-005, F-DDL-011/015, V2 |

### 4.5 DT-EXEC · Cop 下推 / MPP / Exchange（DTP-2 / DTP-3）

| 编号 | 测试项 | 依赖 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- | --- |
| DT-EXEC-001 | 下推决策与「下推开/关结果一致」 | L12 | 同查询开关对拍 | 100% 一致 | AC-PLAN-005, F-PLAN-011, V3 |
| DT-EXEC-002 | ExchangeSender/Receiver + Hash/Range/Broadcast Shuffle | L12 | 4 节点数据重分布 | 无重复 / 无遗漏 | AC-EXEC-010.., F-EXEC-010.., V3 |
| DT-EXEC-003 | MPP Task 调度与 4 节点加速 | L12 | TPC-H SF=100 全量 | 加速 ≥ 2.8× | AC-EXEC-*, F-EXEC-*, V4 |
| DT-EXEC-004 | Runtime Filter / Bloom / Min-Max | L12 | 构建端下推 + 对拍 | 结果一致，收益可量化 | AC-OP-022, F-EXEC-*, V4 |
| DT-EXEC-005 | Adaptive Join / 倾斜处理 AQE 消融 | L12 | skew 数据集 + 单变量消融 | 无灾难性倾斜，收益可复现 | RQ-4, 05 §5.5, V4 |
| DT-EXEC-006 | Results 流式编码与背压 / 查询取消 | L5/L12 | 大结果集 + 客户端断连 | 无泄漏，取消即时生效 | AC-EXEC-015, F-EXEC-*, V3 |
| DT-EXEC-007 | 读己之写（UnionScan）与快照合并 | L4/L12 | 事务内缓冲 + 快照读 | 结果正确 | AC-STO-003, F-STO-*, V3 |

### 4.6 DT-HTAP · 列存 HTAP 与行列选择（DTP-3）

| 编号 | 测试项 | 依赖 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- | --- |
| DT-HTAP-001 | Raft Learner 异步复制 / `resolved_ts` 一致性读 | L13 | 副本延迟注入 + 历史读 | 一致性读无违例 | AC-HTAP-001, F-HTAP-001/002, V3 |
| DT-HTAP-002 | 列式段存 + Delta / 列存扫描 / ZoneMap | L13 | 行列结果对拍 | 100% 一致 | AC-HTAP-002, F-HTAP-003/004, V4 |
| DT-HTAP-003 | 行列代价统一建模 / `READ_FROM_STORAGE` | L13 | 混合计划 + 开关对比 | 行列混合计划正确 | AC-HTAP-*, F-HTAP-*, V4 |
| DT-HTAP-004 | 列存副本一致性校验 / 扩缩容 / 回放 | L13 | checksum 注入 + 扩缩容 | 不一致检出 100% | AC-HTAP-*, F-HTAP-008/011, V3 |
| DT-HTAP-005 | freshness–latency 曲线 / OLTP 隔离 | L13 | RQ-5 消融 + 提交延迟监控 | OLTP 提交延迟不受列存同步影响 | RQ-5, 05 §5.6, V4 |

### 4.7 DT-CHAOS · 故障注入与混沌（DTP-4，对齐 05 §8）

| 编号 | 测试项 | 注入 | 检查 | 追溯 |
| --- | --- | --- | --- | --- |
| DT-CHAOS-001 | Leader 故障 | 随机 kill leader（含 Prewrite/Commit 中间态） | RTO ≤ 10s、RPO = 0、无脑裂 | 05 §8, AC-STO-006, V3 |
| DT-CHAOS-002 | 网络分区 | 少数派/多数派隔离、抖动 | 线性一致性违例 0 | 05 §8, AC-TXN-008, V3 |
| DT-CHAOS-003 | 时钟漂移 | TSO 节点 ±偏移 | 时间戳单调、快照读正确 | 05 §8, AC-META-001, V3 |
| DT-CHAOS-004 | 磁盘满 / IO 卡顿 | Store 注入 | 降级可恢复、无数据损坏 | 05 §8, AC-STO-004, V3 |
| DT-CHAOS-005 | Region Split 并发事务 | Split 与 2PC 交错 | 原子性与路由重试正确 | 05 §8, AC-STO-007, V3 |
| DT-CHAOS-006 | 副本校验 | 静默数据损坏注入 | checksum 100% 检出 | 05 §8, AC-STO-008, V3 |
| DT-CHAOS-007 | GC 安全 | 长事务 + GC 并发 | 活跃快照不失效 | 05 §8, AC-TXN-007, V3 |
| DT-CHAOS-008 | 72h 长稳 | 混合 OLTP/OLAP + 周期故障 | 数据错乱 0、panic 0 | 05 §8, V5 |
| DT-CHAOS-009 | 确定性模拟器重放 | 消息乱序/延迟/丢失/超时编码为种子化事件 | 任一失败 100% 可精确重放 | F-TEST-016, 05 §8, V3 |

### 4.8 DT-BENCH · 基准 / 消融 / 可复现（DTP-5 / DTP-6，V4/V5）

| 编号 | 测试项 | 方法 | 通过标准 | 追溯 |
| --- | --- | --- | --- | --- |
| DT-BENCH-001 | TPC-H / TPC-DS 全量 22/99 查询 | 固化环境 + 结果校验 | 结果正确，性能达 05 §3 阈值 | AC-BENCH-001/002, V4 |
| DT-BENCH-002 | TPC-C / sysbench / YCSB | 标准跑分 | tpmC 线性度、P99 记录完整 | AC-BENCH-002, V4 |
| DT-BENCH-003 | 算子微基准 / 向量化消融 | criterion + 逐层消融 | 列式 vs 行式 ≥ 3× | AC-BENCH-003, AC-OP-023..026, V4 |
| DT-BENCH-004 | SQLancer TLP / NoREC / PQS + 随机 SQL + Fuzz | 24h 连续运行 | 未修复错误 0 | AC-TEST-005, F-TEST-011/012/020, V4 |
| DT-BENCH-005 | 差分测试（MySQL/PG/DuckDB/TiDB 形态对照） | 结果对拍 | 100% 一致（跨系统只做机制讨论） | AC-TEST-004, F-TEST-008, V4 |
| DT-BENCH-006 | 计划回归 / plan hash diff | CI plan diff | 回归 0 | AC-PLAN-009, AC-TEST-009, V4 |
| DT-BENCH-007 | 可复现 artifact | 固定种子 + ≥5 次重复 + 95% CI + 第二人重跑 | 相对偏差 ≤ 5% | AC-BENCH-009/010, 05 §9, V5 |
| DT-BENCH-008 | 全量验收矩阵自动校验 | 脚本逐条核验 `AC-*` | 100% 覆盖，追溯缺口 0 | AC-TEST-012, 05 §10, V5 |

---

## 5. 一致性验证方法（V3/V5 专项）

1. **线性一致性**：Porcupine / Jepsen 风格历史记录检查器，输入为并发客户端的 `invoke/return` 序列，输出任何违例即冻结 M5（AC-TXN-008、DT-TXN-007、DT-CHAOS-002）。
2. **快照隔离（SI）**：异常注入脏读 / 不可重复读 / 幻读 / 写偏斜，检出率 100%（AC-TXN-002、DT-TXN-008）；SSI 在 L15 补齐写偏斜阻止（AC-TXN-008）。
3. **事务原子性**：跨 Region 2PC 在 Prewrite / Commit 各中间态注入 kill，检查 primary/secondary 最终一致（DT-TXN-001）。
4. **确定性模拟器**：所有分布式事件编码为种子化事件序列，任一失败可按种子精确重放（F-TEST-016、DT-CHAOS-009），是全部 V3 用例的前置工具。
5. **副本一致性**：静默损坏注入 + checksum 比对（DT-STO-007、DT-CHAOS-006）。

---

## 6. 性能与消融（V4，单变量原则，05 §9.2）

| 实验 | 变量 | 对照组 | 指标 | 归因 |
| --- | --- | --- | --- | --- |
| MPP 扩展 | 节点数 1/2/4/8 | 单节点 | 加速比 ≥ 2.8×（4 节点） | perf / flamegraph / roofline |
| 下推收益 | Cop 开 / 关 | 关闭下推 | 下推开/关结果一致 + 延迟收益 | `OPTIMIZE_TRACE` / `EXPLAIN ANALYZE` |
| 向量化 | 行式 / 列式 | 行式基线 | 列式 ≥ 3× | 逐算子消融 |
| AQE / Runtime Filter | 开 / 关 | 关闭 | 倾斜收益上界（RQ-4） | 单变量消融 + 交互项 |
| 行列选择 | 行存 / 列存 / 混合 | 行存 | freshness–latency 曲线（RQ-5） | 05 §5.6 |
| 优化器 Pareto | DP / Cascades Memo | RBO 基线 | TPC-H 几何平均加速 ≥ 2.0×（RQ-1） | 双引擎对比 |

> 禁止跨系统裸比；所有结论附环境、种子、数据集、基线、置信区间（05 §9.2、01 §14 有效性声明）。

---

## 7. CI / 门禁映射（05 §1、§10）

| 门禁 | 自动化内容 | 触发 | 阻断条件 |
| --- | --- | --- | --- |
| 每次提交（V0–V2） | fmt / clippy / 单测 / 依赖方向 / 追溯 / 单节点端到端 | `push` / `PR` | 任一失败阻断合并 |
| M5（W49） | DT-STO / DT-TXN / DT-META / DT-DDL / DT-EXEC / DT-HTAP / DT-CHAOS 全量 | 里程碑评审 | 05 §10.2 中 AC-STO-005..010、AC-TXN-002/004..007、AC-EXEC-*、AC-META-*、AC-HTAP-* 未全绿 |
| M6（W53） | DT-BENCH 全量 + 72h 长稳 + 可复现复核 | 里程碑评审 | 05 §10.2 全矩阵 100%、重跑偏差 ≤ 5% |
| M7（W60） | 论文图表一键重跑 + 文档/代码/实验版本一致 | 结题评审 | 版本不一致或结论不可复现 |

**证据归档**：每条 `DT-*` 的测试报告 / 基准 JSON / plan diff / trace / 混沌日志落在 `artifacts/<M*>/<AC-*>/`，由 F-OBS-012 自动附带环境、种子、版本、配置（05 §1、§9.3）。

---

## 8. 追溯矩阵（DT-* → AC-* → F-* → L* → M* → V*）

| 测试组 | 主 Lab | 里程碑 | 主验收项 | 等级 |
| --- | --- | --- | --- | --- |
| DT-STO-* | L11 / L12 / L14 | M5 / M6 | AC-STO-005..010, AC-EXEC-016 | V3 |
| DT-TXN-* | L11 / L14 / L15 | M5 / M6 / M7 | AC-TXN-002/004..008 | V3 / V5 |
| DT-META-* | L11 | M5 | AC-META-001..005 | V2 / V3 |
| DT-DDL-* | L5 / L11 | M5 | AC-DDL-001..005 | V2 / V3 |
| DT-EXEC-* | L5 / L12 | M5 | AC-EXEC-*, AC-PLAN-005/011, AC-OP-022 | V3 / V4 |
| DT-HTAP-* | L13 | M5 | AC-HTAP-*, RQ-5 | V3 / V4 |
| DT-CHAOS-* | L11 / L14 | M5 / M6 | 05 §8 矩阵, AC-TXN-008, AC-STO-006..008 | V3 / V5 |
| DT-BENCH-* | L14 / L15 | M6 / M7 | AC-BENCH-*, AC-TEST-*, AC-PLAN-009 | V4 / V5 |

**完整性检查**（由 `scripts/check_dependency_direction.py --traceability` 扩展或人工评审）：
1. 每条 `DT-*` 至少引用 1 条 `AC-*`；
2. 每条 05 §8 场景至少被 1 条 `DT-CHAOS-*` 覆盖；
3. 每个 M5/M6/M7 门禁的 `AC-*` 组至少被 1 条 `DT-*` 覆盖；
4. 新增 `DT-*` 仅追加、不重排既有编号（05 §12 规则）。

---

## 9. 风险与缓冲（03 §6）

| # | 风险 | 缓解 | 缓冲 |
| --- | --- | --- | --- |
| DRK-1 | raft-rs / RocksDB binding 在 Windows 编译问题 | 统一 Linux 容器做 cluster 实验，CI 用 Linux runner | W42（RK-3） |
| DRK-2 | 分布式正确性（2PC / Region epoch）调试周期超预期 | L11 先做确定性模拟器再上真网络；DT-CHAOS-009 前置 | W49 前 1 周 |
| DRK-3 | 性能实验环境波动导致结论不可复现 | 环境固化 + 固定种子 + ≥5 次重复 + 置信区间 | W53 前 1 周 |
| DRK-4 | 混沌 72h 长稳暴露偶发 bug | 故障注入常态化（DTP-4 起每周回归） | W53 |
| DRK-5 | 论文写作挤压实现 | M3 即产出第一篇数据，L15 预留 2 周纯写作 | W59–W60 |

---

## 10. 勘误与一致性说明

- 本文为 01–05 的下游测试计划，**不新增** `F-* / AC-* / OP-* / L* / M*` 编号，仅新增 `DT-* / DTP-*` 测试编号；如后续需将测试项升级为正式验收项，按 05 §12 从对应 `AC-*` 域最大序号 +1 追加。
- 03 §2 中 `W01…W48` 为早期占位，本文周次以 03 §3 正文与 §9 映射表（W39–W60）为准。
- 01 §3.1 的 `cluster` 形态（3–8 节点）为本文唯一分布式部署形态；单机 / 本地形态的测试由 05 §2 V0–V2 覆盖，不在本文重复。
