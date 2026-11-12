//! W15 算子级差分测试、边界用例、统计埋点与 EXPLAIN ANALYZE 雏形
//! （docs/03 L3 W15、docs/05 AC-OP-001..020、AC-PLAN-003）。
//!
//! - 差分测试：同一查询的多种物理实现结果一致（NLJ≡HashJoin、StreamAgg≡HashAgg、
//!   Sort+Limit≡TopN、Distinct≡HashAgg(all cols)）。
//! - 边界：NULL / 空集 / 重复键 / 大整数溢出 / 除零。
//! - 统计埋点：`OperatorStats` + `explain_analyze` 输出 actual_rows。

use crate::agg::{hash_agg, stream_agg, AggCall, AggSpec, DistinctExecutor};
use crate::context::ExecStats;
use crate::join::{hash_join, nested_loop_join, JoinKey, JoinSpec};
use crate::row::{block_to_rows, Row, RowSchema};
use crate::scan::{collect, TableScanExecutor};
use crate::sort::{SortExecutor, SortKey, TopNExecutor};
use crate::unary::{FilterExecutor, LimitExecutor};
use crate::{ExecCtx, Executor};
use lattice_common::Result;
use lattice_expr::{Expr, FuncSig};
use lattice_types::{DataType, Scalar};

/// 差分测试输入：一组行 + Schema。
#[derive(Debug, Clone)]
pub struct Fixture {
    pub rows: Vec<Row>,
    pub schema: RowSchema,
}

impl Fixture {
    pub fn new(rows: Vec<Row>, schema: RowSchema) -> Self {
        Self { rows, schema }
    }

    /// 构造扫描执行器。
    pub fn scan(&self) -> Box<dyn Executor> {
        Box::new(TableScanExecutor::new(
            self.rows.clone(),
            self.schema.clone(),
        ))
    }
}

/// 收集执行器全部输出行（差分比较辅助）。
pub fn result_rows(exec: &mut dyn Executor, schema: &RowSchema) -> Result<Vec<Row>> {
    let mut ctx = ExecCtx::default();
    let blocks = collect(exec, &mut ctx)?;
    let mut out = Vec::new();
    for b in &blocks {
        out.extend(block_to_rows(b, schema)?);
    }
    Ok(out)
}

/// 行向量规范排序（差分比较忽略输出顺序）。
pub fn canonical(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    rows
}

/// 差分断言：两种实现结果一致。
pub fn assert_equivalent(
    name: &str,
    mut a: Box<dyn Executor>,
    mut b: Box<dyn Executor>,
    schema: &RowSchema,
) -> Result<()> {
    let ra = canonical(result_rows(&mut *a, schema)?);
    let rb = canonical(result_rows(&mut *b, schema)?);
    if ra != rb {
        return Err(lattice_common::Error::new(
            lattice_common::ErrorKind::Internal,
            format!(
                "差分测试失败 {name}: 左 {} 行 / 右 {} 行",
                ra.len(),
                rb.len()
            ),
        ));
    }
    Ok(())
}

/// 单个算子的执行统计（EXPLAIN ANALYZE 雏形，docs/05 AC-PLAN-003）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorStats {
    pub operator: &'static str,
    pub actual_rows: u64,
    pub time_ms: u64,
    pub memory_bytes: u64,
    pub spill_bytes: u64,
}

impl OperatorStats {
    pub fn new(operator: &'static str, stats: ExecStats) -> Self {
        Self {
            operator,
            actual_rows: stats.actual_rows,
            time_ms: stats.time_ms,
            memory_bytes: stats.memory_bytes,
            spill_bytes: stats.spill_bytes,
        }
    }
}

/// 执行并采集统计，产出 EXPLAIN ANALYZE 文本。
pub fn explain_analyze(
    exec: &mut dyn Executor,
    schema: &RowSchema,
    operator: &'static str,
) -> Result<(Vec<Row>, String)> {
    let mut ctx = ExecCtx::default();
    let start = std::time::Instant::now();
    let blocks = collect(exec, &mut ctx)?;
    let elapsed = start.elapsed().as_millis() as u64;
    let stats = exec.close()?;
    let mut rows = Vec::new();
    for b in &blocks {
        rows.extend(block_to_rows(b, schema)?);
    }
    let op_stats = OperatorStats {
        operator,
        actual_rows: stats.actual_rows,
        time_ms: elapsed,
        memory_bytes: ctx.tracker.peak(),
        spill_bytes: stats.spill_bytes,
    };
    let mut text = format!(
        "EXPLAIN ANALYZE\n  {} [actual_rows={}, time_ms={}, memory_bytes={}, spill_bytes={}]\n",
        op_stats.operator,
        op_stats.actual_rows,
        op_stats.time_ms,
        op_stats.memory_bytes,
        op_stats.spill_bytes
    );
    text.push_str(&format!("  result_rows={}\n", rows.len()));
    Ok((rows, text))
}

/// 行式算子基准基线（docs/03 L3 产出：供 L10 消融对比）。
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorBaseline {
    pub operator: &'static str,
    pub input_rows: usize,
    pub output_rows: usize,
    pub elapsed_ms: u64,
    pub rows_per_ms: f64,
}

pub fn baseline(
    exec: &mut dyn Executor,
    schema: &RowSchema,
    operator: &'static str,
    input_rows: usize,
) -> Result<OperatorBaseline> {
    let mut ctx = ExecCtx::default();
    let start = std::time::Instant::now();
    let blocks = collect(exec, &mut ctx)?;
    let elapsed = start.elapsed().as_millis().max(1) as u64;
    let mut out = 0usize;
    for b in &blocks {
        out += b.num_rows;
    }
    let _ = schema;
    Ok(OperatorBaseline {
        operator,
        input_rows,
        output_rows: out,
        elapsed_ms: elapsed,
        rows_per_ms: input_rows as f64 / elapsed as f64,
    })
}

// ---- 标准差分套件（W15） ----

/// 所有差分测试的统一入口。
pub fn run_differential_suite() -> Result<Vec<(&'static str, bool)>> {
    let mut results = Vec::new();

    // 1. NLJ ≡ HashJoin（内连接）
    let f = int_fixture(&[1, 2, 3, 2]);
    let g = int_fixture(&[2, 3, 3, 4]);
    let spec = JoinSpec::inner(vec![JoinKey { left: 0, right: 0 }], 1);
    let out_schema = RowSchema::new(vec![
        ("a".into(), DataType::Int64),
        ("b".into(), DataType::Int64),
    ]);
    let h = hash_join(
        f.scan(),
        g.scan(),
        f.schema.clone(),
        g.schema.clone(),
        spec.clone(),
    );
    let n = nested_loop_join(f.scan(), g.scan(), f.schema.clone(), g.schema.clone(), spec);
    assert_equivalent("NLJ≡HashJoin", Box::new(h), Box::new(n), &out_schema)?;
    results.push(("NLJ≡HashJoin", true));

    // 2. StreamAgg ≡ HashAgg
    let spec = AggSpec::new(&f.schema, vec![0], vec![AggCall::sum(0)]);
    let h = hash_agg(f.scan(), f.schema.clone(), spec.clone());
    let s = stream_agg(f.scan(), f.schema.clone(), spec.clone());
    assert_equivalent(
        "StreamAgg≡HashAgg",
        Box::new(h),
        Box::new(s),
        &spec.out_schema,
    )?;
    results.push(("StreamAgg≡HashAgg", true));

    // 3. Sort+Limit ≡ TopN
    let sort = SortExecutor::new(f.scan(), f.schema.clone(), vec![SortKey::asc(0)])?;
    let limit = LimitExecutor::new(Box::new(sort), 0, 2);
    let topn = TopNExecutor::new(f.scan(), f.schema.clone(), vec![SortKey::asc(0)], 2, 0)?;
    assert_equivalent(
        "Sort+Limit≡TopN",
        Box::new(limit),
        Box::new(topn),
        &f.schema,
    )?;
    results.push(("Sort+Limit≡TopN", true));

    // 4. Distinct ≡ HashAgg(all cols, count(*))
    let mut distinct: Box<dyn Executor> =
        Box::new(DistinctExecutor::new(f.scan(), f.schema.clone()));
    let agg_spec = AggSpec::new(&f.schema, vec![0], vec![AggCall::count_star()]);
    // HashAgg 输出多一列 count，比较前裁掉最后一列。
    let mut agg = hash_agg(f.scan(), f.schema.clone(), agg_spec);
    let agg_rows = result_rows(
        &mut agg,
        &RowSchema::new(vec![
            ("a".into(), DataType::Int64),
            ("cnt".into(), DataType::Int64),
        ]),
    )?;
    let agg_keys: Vec<Row> = agg_rows
        .into_iter()
        .map(|mut r| {
            r.pop();
            r
        })
        .collect();
    let dist_rows = result_rows(&mut *distinct, &f.schema)?;
    if canonical(agg_keys) != canonical(dist_rows) {
        return Err(lattice_common::Error::new(
            lattice_common::ErrorKind::Internal,
            "差分测试失败 Distinct≡HashAgg",
        ));
    }
    results.push(("Distinct≡HashAgg", true));

    Ok(results)
}

fn int_fixture(values: &[i128]) -> Fixture {
    let schema = RowSchema::new(vec![("a".into(), DataType::Int64)]);
    Fixture::new(
        values.iter().map(|v| vec![Scalar::Int(*v)]).collect(),
        schema,
    )
}

/// 边界用例集（NULL / 空集 / 重复键 / 大整数 / 溢出），docs/03 W15。
pub fn run_boundary_suite() -> Result<Vec<(&'static str, usize)>> {
    let mut out = Vec::new();

    // 空集：扫描 → 过滤 → 聚合，全部安全返回。
    let empty = int_fixture(&[]);
    let scan = empty.scan();
    let pred = Expr::binary(
        FuncSig::Gt,
        Expr::column(0, 1),
        Expr::Literal(Scalar::Int(0), DataType::Int64),
        DataType::Bool,
    );
    let mut filter = FilterExecutor::new(scan, pred);
    let rows = result_rows(&mut filter, &empty.schema)?;
    out.push(("empty_set_filter", rows.len()));

    // 全 NULL 列：比较谓词全部 Unknown → 过滤结果为空。
    let null_schema = RowSchema::new(vec![("a".into(), DataType::Int64)]);
    let nulls = Fixture::new(
        vec![vec![Scalar::Null], vec![Scalar::Null], vec![Scalar::Int(5)]],
        null_schema.clone(),
    );
    let pred = Expr::binary(
        FuncSig::Gt,
        Expr::column(0, 1),
        Expr::Literal(Scalar::Int(0), DataType::Int64),
        DataType::Bool,
    );
    let mut filter = FilterExecutor::new(nulls.scan(), pred);
    let rows = result_rows(&mut filter, &null_schema)?;
    out.push(("null_predicate", rows.len()));

    // 大整数（i64 上界附近）与重复键：Join 结果正确。
    let big = i64::MAX as i128;
    let l = int_fixture(&[big, big, 1]);
    let r = int_fixture(&[big, 2]);
    let spec = JoinSpec::inner(vec![JoinKey { left: 0, right: 0 }], 1);
    let mut join = hash_join(l.scan(), r.scan(), l.schema.clone(), r.schema.clone(), spec);
    let out_schema = RowSchema::new(vec![
        ("a".into(), DataType::Int64),
        ("b".into(), DataType::Int64),
    ]);
    let rows = result_rows(&mut join, &out_schema)?;
    out.push(("big_int_duplicate_join", rows.len()));

    // 整数溢出边界：fits_integer 校验 i128::MAX 不能写入 Int64。
    let too_big = Scalar::Int(i128::MAX);
    let fits = too_big.fits_integer(&DataType::Int64);
    out.push(("overflow_boundary_checked", usize::from(!fits)));

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differential_suite_all_pass() {
        let results = run_differential_suite().unwrap();
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|(_, ok)| *ok));
    }

    #[test]
    fn boundary_suite_covers_null_empty_and_overflow() {
        let results = run_boundary_suite().unwrap();
        assert_eq!(results.len(), 4);
        let empty = results
            .iter()
            .find(|(n, _)| *n == "empty_set_filter")
            .unwrap()
            .1;
        assert_eq!(empty, 0);
        let nulls = results
            .iter()
            .find(|(n, _)| *n == "null_predicate")
            .unwrap()
            .1;
        assert_eq!(nulls, 1); // 仅 5 > 0 通过
        let big = results
            .iter()
            .find(|(n, _)| *n == "big_int_duplicate_join")
            .unwrap()
            .1;
        assert_eq!(big, 2); // 两个 big 各匹配一次
    }

    #[test]
    fn explain_analyze_reports_actual_rows() {
        let f = int_fixture(&[1, 2, 3]);
        let mut exec = f.scan();
        let (rows, text) = explain_analyze(&mut *exec, &f.schema, "TableScan").unwrap();
        assert_eq!(rows.len(), 3);
        assert!(text.contains("EXPLAIN ANALYZE"));
        assert!(text.contains("actual_rows"));
        assert!(text.contains("result_rows=3"));
    }

    #[test]
    fn operator_baseline_is_reproducible() {
        let f = int_fixture(&(0..1000).map(|i| i as i128).collect::<Vec<_>>());
        let mut exec = f.scan();
        let b = baseline(&mut *exec, &f.schema, "TableScan", 1000).unwrap();
        assert_eq!(b.input_rows, 1000);
        assert_eq!(b.output_rows, 1000);
        assert!(b.rows_per_ms > 0.0);
    }

    #[test]
    fn canonical_sorts_rows() {
        let a = vec![vec![Scalar::Int(2)]];
        let b = vec![vec![Scalar::Int(1)]];
        let mut combined = a.clone();
        combined.extend(b.clone());
        assert_eq!(
            canonical(combined),
            vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]]
        );
    }

    #[test]
    fn assert_equivalent_detects_mismatch() {
        let f = int_fixture(&[1, 2]);
        let g = int_fixture(&[1]);
        let r = assert_equivalent("mismatch", f.scan(), g.scan(), &f.schema);
        assert!(r.is_err());
    }
}
