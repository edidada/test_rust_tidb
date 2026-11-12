//! W12 聚合 / 集合算子（docs/03 L3 W12、docs/04 §4.2、AC-OP-014/016）：
//! OP-P-HASHAGG / STREAMAGG / SCALARAGG / DISTINCTAGG /
//! UNIONALL / UNIONDISTINCT / INTERSECT / EXCEPT / DISTINCT。

use crate::context::ExecStats;
use crate::row::{block_to_rows, rows_to_block, Row, RowSchema};
use crate::scan::collect;
use crate::{ExecCtx, Executor, Property};
use lattice_common::{Error, ErrorKind, Result};
use lattice_types::{Block, DataType, Scalar};
use std::collections::{HashMap, HashSet};

/// 聚合函数种类（docs/02 §2.2 `AggKind` 的物理实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggFunc {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// 聚合调用：`arg = None` 表示 `COUNT(*)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AggCall {
    pub func: AggFunc,
    pub arg: Option<usize>,
    pub distinct: bool,
}

impl AggCall {
    pub fn count_star() -> Self {
        Self {
            func: AggFunc::Count,
            arg: None,
            distinct: false,
        }
    }

    pub fn count(col: usize, distinct: bool) -> Self {
        Self {
            func: AggFunc::Count,
            arg: Some(col),
            distinct,
        }
    }

    pub fn sum(col: usize) -> Self {
        Self {
            func: AggFunc::Sum,
            arg: Some(col),
            distinct: false,
        }
    }
}

/// 分组键（L3 基线：按 Scalar 的规范字符串编码，NULL 独立）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GroupKey(Vec<String>);

fn group_key(row: &Row, cols: &[usize]) -> GroupKey {
    GroupKey(
        cols.iter()
            .map(|i| match &row[*i] {
                Scalar::Null => "NULL".to_string(),
                other => format!("{other:?}"),
            })
            .collect(),
    )
}

fn scalar_num(v: &Scalar) -> Option<f64> {
    match v {
        Scalar::Int(i) => Some(*i as f64),
        Scalar::Float(f) => Some(*f),
        Scalar::Decimal { unscaled, scale } => Some(*unscaled as f64 / 10f64.powi(*scale as i32)),
        _ => None,
    }
}

/// 单个聚合的状态。
#[derive(Debug, Clone, Default)]
struct AggState {
    count: i128,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
    seen: HashSet<String>,
}

impl AggState {
    fn accumulate(&mut self, call: &AggCall, row: &Row) {
        let value = call.arg.map(|i| &row[i]);
        if call.distinct {
            let key = format!("{value:?}");
            if !self.seen.insert(key) {
                return;
            }
        }
        match call.func {
            AggFunc::Count => {
                if call.arg.is_none() || !matches!(value, Some(Scalar::Null)) {
                    self.count += 1;
                }
            }
            AggFunc::Sum | AggFunc::Avg => {
                if let Some(v) = value {
                    if let Some(n) = scalar_num(v) {
                        self.count += 1;
                        self.sum += n;
                    }
                }
            }
            AggFunc::Min | AggFunc::Max => {
                if let Some(n) = value.and_then(scalar_num) {
                    self.count += 1;
                    self.min = Some(self.min.map_or(n, |m| m.min(n)));
                    self.max = Some(self.max.map_or(n, |m| m.max(n)));
                }
            }
        }
    }

    fn finalize(&self, call: &AggCall) -> Scalar {
        match call.func {
            AggFunc::Count => Scalar::Int(self.count),
            AggFunc::Sum => {
                if self.count == 0 {
                    Scalar::Null
                } else {
                    Scalar::Float(self.sum)
                }
            }
            AggFunc::Avg => {
                if self.count == 0 {
                    Scalar::Null
                } else {
                    Scalar::Float(self.sum / self.count as f64)
                }
            }
            AggFunc::Min => self.min.map_or(Scalar::Null, Scalar::Float),
            AggFunc::Max => self.max.map_or(Scalar::Null, Scalar::Float),
        }
    }
}

/// 聚合规格：分组列 + 聚合调用 + 输出 Schema。
#[derive(Debug, Clone)]
pub struct AggSpec {
    pub group_by: Vec<usize>,
    pub calls: Vec<AggCall>,
    pub out_schema: RowSchema,
}

impl AggSpec {
    pub fn new(input_schema: &RowSchema, group_by: Vec<usize>, calls: Vec<AggCall>) -> Self {
        let mut cols: Vec<(String, DataType)> = group_by
            .iter()
            .map(|i| input_schema.columns[*i].clone())
            .collect();
        for (i, c) in calls.iter().enumerate() {
            let ty = match c.func {
                AggFunc::Count => DataType::Int64,
                AggFunc::Avg => DataType::Float64,
                AggFunc::Sum | AggFunc::Min | AggFunc::Max => DataType::Float64,
            };
            cols.push((format!("agg_{i}"), ty));
        }
        Self {
            group_by,
            calls,
            out_schema: RowSchema::new(cols),
        }
    }

    /// 标量聚合（无 GROUP BY，docs/04 OP-P-SCALARAGG）。
    pub fn scalar(calls: Vec<AggCall>) -> Self {
        let cols = calls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let ty = match c.func {
                    AggFunc::Count => DataType::Int64,
                    _ => DataType::Float64,
                };
                (format!("agg_{i}"), ty)
            })
            .collect();
        Self {
            group_by: Vec::new(),
            calls,
            out_schema: RowSchema::new(cols),
        }
    }
}

/// 一次性计算聚合（HashAgg / ScalarAgg 共用）。
fn compute_agg(rows: &[Row], spec: &AggSpec, _streaming: bool) -> Result<Vec<Row>> {
    // 分组：key → (首行分组值, 各聚合状态)。
    let mut groups: HashMap<GroupKey, (Row, Vec<AggState>)> = HashMap::new();
    for row in rows {
        let key = group_key(row, &spec.group_by);
        let entry = groups.entry(key.clone()).or_insert_with(|| {
            let group_values: Row = spec.group_by.iter().map(|i| row[*i].clone()).collect();
            let states = spec.calls.iter().map(|_| AggState::default()).collect();
            (group_values, states)
        });
        for (call, state) in spec.calls.iter().zip(entry.1.iter_mut()) {
            state.accumulate(call, row);
        }
    }

    // 空集标量聚合：COUNT=0，其余 NULL（docs/04 OP-P-SCALARAGG 语义）。
    if groups.is_empty() && spec.group_by.is_empty() {
        let states: Vec<AggState> = spec.calls.iter().map(|_| AggState::default()).collect();
        let mut out = Row::new();
        for (call, state) in spec.calls.iter().zip(&states) {
            out.push(state.finalize(call));
        }
        return Ok(vec![out]);
    }

    let mut out = Vec::with_capacity(groups.len());
    for (_key, (group_values, states)) in groups {
        let mut row = group_values;
        for (call, state) in spec.calls.iter().zip(&states) {
            row.push(state.finalize(call));
        }
        out.push(row);
    }
    Ok(out)
}

/// 通用聚合执行器（HashAgg / StreamAgg / ScalarAgg 共用；open 物化，分块输出）。
#[derive(Debug)]
pub struct AggregateExecutor {
    child: Box<dyn Executor>,
    input_schema: RowSchema,
    spec: AggSpec,
    /// 是否按有序输入流式处理（StreamAgg 语义标记）。
    streaming: bool,
    result: Vec<Row>,
    cursor: usize,
    chunk_size: usize,
    stats: ExecStats,
    closed: bool,
}

impl AggregateExecutor {
    pub fn new(child: Box<dyn Executor>, input_schema: RowSchema, spec: AggSpec) -> Self {
        Self {
            child,
            input_schema,
            spec,
            streaming: false,
            result: Vec::new(),
            cursor: 0,
            chunk_size: 1024,
            stats: ExecStats::default(),
            closed: false,
        }
    }

    pub fn stream_agg(mut self) -> Self {
        self.streaming = true;
        self
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.spec.out_schema
    }
}

impl Executor for AggregateExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let mut rows = Vec::new();
        for block in collect(&mut *self.child, ctx)? {
            rows.extend(block_to_rows(&block, &self.input_schema)?);
        }
        self.chunk_size = ctx.chunk_capacity.max(1);
        self.result = compute_agg(&rows, &self.spec, self.streaming)?;
        self.cursor = 0;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.cursor >= self.result.len() {
            return Ok(None);
        }
        let end = (self.cursor + self.chunk_size).min(self.result.len());
        let chunk = &self.result[self.cursor..end];
        self.cursor = end;
        self.stats.record_rows(chunk.len());
        Ok(Some(rows_to_block(chunk, &self.spec.out_schema)?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.stats.merge(self.child.close()?);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-HASHAGG。
pub fn hash_agg(
    child: Box<dyn Executor>,
    input_schema: RowSchema,
    spec: AggSpec,
) -> AggregateExecutor {
    AggregateExecutor::new(child, input_schema, spec)
}

/// OP-P-STREAMAGG（输入需按 group key 有序）。
pub fn stream_agg(
    child: Box<dyn Executor>,
    input_schema: RowSchema,
    spec: AggSpec,
) -> AggregateExecutor {
    AggregateExecutor::new(child, input_schema, spec).stream_agg()
}

/// OP-P-SCALARAGG。
pub fn scalar_agg(child: Box<dyn Executor>, calls: Vec<AggCall>) -> AggregateExecutor {
    AggregateExecutor::new(child, RowSchema::new(vec![]), AggSpec::scalar(calls))
}

/// OP-P-DISTINCT：全行去重。
#[derive(Debug)]
pub struct DistinctExecutor {
    child: Box<dyn Executor>,
    schema: RowSchema,
    result: Vec<Row>,
    cursor: usize,
    chunk_size: usize,
    stats: ExecStats,
    closed: bool,
}

impl DistinctExecutor {
    pub fn new(child: Box<dyn Executor>, schema: RowSchema) -> Self {
        Self {
            child,
            schema,
            result: Vec::new(),
            cursor: 0,
            chunk_size: 1024,
            stats: ExecStats::default(),
            closed: false,
        }
    }
}

impl Executor for DistinctExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let mut rows = Vec::new();
        for block in collect(&mut *self.child, ctx)? {
            rows.extend(block_to_rows(&block, &self.schema)?);
        }
        let mut seen = HashSet::new();
        self.result = rows
            .into_iter()
            .filter(|r| seen.insert(format!("{r:?}")))
            .collect();
        self.cursor = 0;
        self.chunk_size = ctx.chunk_capacity.max(1);
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.cursor >= self.result.len() {
            return Ok(None);
        }
        let end = (self.cursor + self.chunk_size).min(self.result.len());
        let chunk = &self.result[self.cursor..end];
        self.cursor = end;
        self.stats.record_rows(chunk.len());
        Ok(Some(rows_to_block(chunk, &self.schema)?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.stats.merge(self.child.close()?);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// 集合操作类型（docs/04 OP-L-UNION/INTERSECT/EXCEPT）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOp {
    UnionAll,
    UnionDistinct,
    Intersect,
    IntersectAll,
    Except,
    ExceptAll,
}

/// OP-P-UNIONALL / UNIONDISTINCT / INTERSECT / EXCEPT。
#[derive(Debug)]
pub struct SetOpExecutor {
    left: Box<dyn Executor>,
    right: Box<dyn Executor>,
    schema: RowSchema,
    op: SetOp,
    result: Vec<Row>,
    cursor: usize,
    chunk_size: usize,
    stats: ExecStats,
    closed: bool,
}

impl SetOpExecutor {
    pub fn new(
        left: Box<dyn Executor>,
        right: Box<dyn Executor>,
        schema: RowSchema,
        op: SetOp,
    ) -> Self {
        Self {
            left,
            right,
            schema,
            op,
            result: Vec::new(),
            cursor: 0,
            chunk_size: 1024,
            stats: ExecStats::default(),
            closed: false,
        }
    }

    fn row_key(r: &Row) -> String {
        format!("{r:?}")
    }
}

impl Executor for SetOpExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;
        let mut lr = Vec::new();
        for b in collect(&mut *self.left, ctx)? {
            lr.extend(block_to_rows(&b, &self.schema)?);
        }
        let mut rr = Vec::new();
        for b in collect(&mut *self.right, ctx)? {
            rr.extend(block_to_rows(&b, &self.schema)?);
        }
        self.chunk_size = ctx.chunk_capacity.max(1);
        self.result = match self.op {
            SetOp::UnionAll => {
                lr.extend(rr);
                lr
            }
            SetOp::UnionDistinct => {
                let mut seen = HashSet::new();
                lr.into_iter()
                    .chain(rr)
                    .filter(|r| seen.insert(Self::row_key(r)))
                    .collect()
            }
            SetOp::Intersect | SetOp::IntersectAll => {
                let rhs: HashSet<String> = rr.iter().map(Self::row_key).collect();
                let mut seen = HashSet::new();
                lr.into_iter()
                    .filter(|r| {
                        rhs.contains(&Self::row_key(r))
                            && (self.op == SetOp::IntersectAll || seen.insert(Self::row_key(r)))
                    })
                    .collect()
            }
            SetOp::Except | SetOp::ExceptAll => {
                let rhs: HashSet<String> = rr.iter().map(Self::row_key).collect();
                let mut seen = HashSet::new();
                lr.into_iter()
                    .filter(|r| {
                        !rhs.contains(&Self::row_key(r))
                            && (self.op == SetOp::ExceptAll || seen.insert(Self::row_key(r)))
                    })
                    .collect()
            }
        };
        self.cursor = 0;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.cursor >= self.result.len() {
            return Ok(None);
        }
        let end = (self.cursor + self.chunk_size).min(self.result.len());
        let chunk = &self.result[self.cursor..end];
        self.cursor = end;
        self.stats.record_rows(chunk.len());
        Ok(Some(rows_to_block(chunk, &self.schema)?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.stats.merge(self.left.close()?);
            self.stats.merge(self.right.close()?);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// 校验两个输入 Schema 一致（集合运算前置，docs/04 AC-OP-016）。
pub fn check_set_schema(left: &RowSchema, right: &RowSchema) -> Result<()> {
    if left.columns.len() != right.columns.len()
        || left
            .columns
            .iter()
            .zip(&right.columns)
            .any(|(a, b)| a.1 != b.1)
    {
        return Err(Error::new(
            ErrorKind::TypeMismatch,
            "集合运算两侧列数/类型不一致",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;
    use crate::scan::TableScanExecutor;

    fn data() -> (Vec<Row>, RowSchema) {
        let s = RowSchema::new(vec![
            ("g".into(), DataType::Int64),
            ("v".into(), DataType::Int64),
        ]);
        let rows = vec![
            vec![Scalar::Int(1), Scalar::Int(10)],
            vec![Scalar::Int(1), Scalar::Int(20)],
            vec![Scalar::Int(2), Scalar::Int(30)],
            vec![Scalar::Int(2), Scalar::Null],
        ];
        (rows, s)
    }

    fn run(exec: &mut AggregateExecutor) -> Result<Vec<Row>> {
        let mut ctx = ExecCtx::default();
        let blocks = collect(exec, &mut ctx)?;
        let schema = exec.output_schema().clone();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &schema)?);
        }
        Ok(out)
    }

    #[test]
    fn hash_agg_group_sum_count() {
        let (rows, s) = data();
        let spec = AggSpec::new(
            &s,
            vec![0],
            vec![
                AggCall::count_star(),
                AggCall::sum(1),
                AggCall {
                    func: AggFunc::Avg,
                    arg: Some(1),
                    distinct: false,
                },
            ],
        );
        let mut exec = hash_agg(Box::new(TableScanExecutor::new(rows, s.clone())), s, spec);
        let out = run(&mut exec).unwrap();
        assert_eq!(out.len(), 2);
        // 组 1: count=2 sum=30 avg=15；组 2: count=2 sum=30 avg=30（NULL 跳过）
        let g1 = out.iter().find(|r| r[0] == Scalar::Int(1)).unwrap();
        assert_eq!(g1[1], Scalar::Int(2));
        assert_eq!(g1[2], Scalar::Float(30.0));
        assert_eq!(g1[3], Scalar::Float(15.0));
    }

    #[test]
    fn scalar_agg_empty_input_semantics() {
        let mut exec = scalar_agg(
            Box::new(TableScanExecutor::new(vec![], RowSchema::new(vec![]))),
            vec![AggCall::count_star(), AggCall::sum(0)],
        );
        let out = run(&mut exec).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][0], Scalar::Int(0)); // COUNT(*) 空集 = 0
        assert_eq!(out[0][1], Scalar::Null); // SUM 空集 = NULL
    }

    #[test]
    fn distinct_agg_deduplicates_values() {
        let s = RowSchema::new(vec![("v".into(), DataType::Int64)]);
        let rows = vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(1)],
            vec![Scalar::Int(2)],
            vec![Scalar::Null],
        ];
        let spec = AggSpec::scalar(vec![AggCall::count(0, true)]);
        let mut exec = hash_agg(Box::new(TableScanExecutor::new(rows, s.clone())), s, spec);
        let out = run(&mut exec).unwrap();
        assert_eq!(out[0][0], Scalar::Int(2)); // 1, 2（NULL 不计）
    }

    #[test]
    fn stream_agg_matches_hash_agg() {
        let (rows, s) = data();
        let spec = AggSpec::new(&s, vec![0], vec![AggCall::sum(1)]);
        let mut h = hash_agg(
            Box::new(TableScanExecutor::new(rows.clone(), s.clone())),
            s.clone(),
            spec.clone(),
        );
        let mut st = stream_agg(Box::new(TableScanExecutor::new(rows, s.clone())), s, spec);
        let a = run(&mut h).unwrap();
        let b = run(&mut st).unwrap();
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn min_max_agg() {
        let (rows, s) = data();
        let spec = AggSpec::new(
            &s,
            vec![0],
            vec![
                AggCall {
                    func: AggFunc::Min,
                    arg: Some(1),
                    distinct: false,
                },
                AggCall {
                    func: AggFunc::Max,
                    arg: Some(1),
                    distinct: false,
                },
            ],
        );
        let mut exec = hash_agg(Box::new(TableScanExecutor::new(rows, s.clone())), s, spec);
        let out = run(&mut exec).unwrap();
        let g1 = out.iter().find(|r| r[0] == Scalar::Int(1)).unwrap();
        assert_eq!(g1[1], Scalar::Float(10.0));
        assert_eq!(g1[2], Scalar::Float(20.0));
    }

    #[test]
    fn distinct_executor_deduplicates_rows() {
        let s = RowSchema::new(vec![("a".into(), DataType::Int64)]);
        let rows = vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(1)],
            vec![Scalar::Int(2)],
        ];
        let mut exec =
            DistinctExecutor::new(Box::new(TableScanExecutor::new(rows, s.clone())), s.clone());
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx).unwrap();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &s).unwrap());
        }
        assert_eq!(out.len(), 2);
    }

    fn set_rows(values: &[i128]) -> (Vec<Row>, RowSchema) {
        let s = RowSchema::new(vec![("a".into(), DataType::Int64)]);
        (values.iter().map(|v| vec![Scalar::Int(*v)]).collect(), s)
    }

    fn run_set(op: SetOp, l: &[i128], r: &[i128]) -> Result<Vec<i128>> {
        let (lr, ls) = set_rows(l);
        let (rr, rs) = set_rows(r);
        check_set_schema(&ls, &rs)?;
        let mut exec = SetOpExecutor::new(
            Box::new(TableScanExecutor::new(lr, ls.clone())),
            Box::new(TableScanExecutor::new(rr, rs)),
            ls.clone(),
            op,
        );
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx)?;
        let mut out = Vec::new();
        for b in &blocks {
            for row in block_to_rows(b, &ls)? {
                if let Scalar::Int(v) = row[0] {
                    out.push(v);
                }
            }
        }
        Ok(out)
    }

    #[test]
    fn union_all_and_distinct() {
        assert_eq!(
            run_set(SetOp::UnionAll, &[1, 2], &[2, 3]).unwrap(),
            vec![1, 2, 2, 3]
        );
        assert_eq!(
            run_set(SetOp::UnionDistinct, &[1, 2], &[2, 3]).unwrap(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn intersect_and_except() {
        assert_eq!(
            run_set(SetOp::Intersect, &[1, 2, 2, 3], &[2, 3]).unwrap(),
            vec![2, 3]
        );
        assert_eq!(
            run_set(SetOp::Except, &[1, 2, 2, 3], &[2]).unwrap(),
            vec![1, 3]
        );
    }

    #[test]
    fn set_schema_mismatch_is_rejected() {
        let (_, a) = set_rows(&[]);
        let b = RowSchema::new(vec![("x".into(), DataType::VarChar(4))]);
        assert!(check_set_schema(&a, &b).is_err());
    }
}
