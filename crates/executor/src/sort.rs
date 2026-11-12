//! W13 排序 / TopN / 窗口 / 物化算子（docs/03 L3 W13、docs/04 §4.2、AC-OP-015/017/018）：
//! OP-P-SORT / TOPN / WINDOW / MATERIALIZE。

use crate::context::ExecStats;
use crate::row::{block_to_rows, rows_to_block, Row, RowSchema};
use crate::scan::collect;
use crate::{ExecCtx, Executor, Property};
use lattice_common::{Error, ErrorKind, Result};
use lattice_types::{Block, DataType, Scalar};
use std::cmp::Ordering;

/// 排序键（docs/02 §2.2 SortItem 的物理形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    pub column: usize,
    pub desc: bool,
    pub nulls_first: bool,
}

impl SortKey {
    pub fn asc(column: usize) -> Self {
        Self {
            column,
            desc: false,
            nulls_first: false,
        }
    }

    pub fn desc(column: usize) -> Self {
        Self {
            column,
            desc: true,
            nulls_first: false,
        }
    }
}

/// 可比较的排序值。
#[derive(Debug, Clone, PartialEq)]
enum SortValue {
    Null,
    Num(f64),
    Str(String),
    Bool(bool),
}

fn sort_value(v: &Scalar) -> SortValue {
    match v {
        Scalar::Null => SortValue::Null,
        Scalar::Int(i) => SortValue::Num(*i as f64),
        Scalar::Float(f) => SortValue::Num(*f),
        Scalar::Decimal { unscaled, scale } => {
            SortValue::Num(*unscaled as f64 / 10f64.powi(*scale as i32))
        }
        Scalar::Str(s) => SortValue::Str(s.clone()),
        Scalar::Bytes(b) => SortValue::Str(String::from_utf8_lossy(b).into_owned()),
        Scalar::Bool(b) => SortValue::Bool(*b),
        Scalar::Temporal(t) => SortValue::Num(*t as f64),
        Scalar::Json(s) => SortValue::Str(s.clone()),
    }
}

fn cmp_values(a: &SortValue, b: &SortValue, nulls_first: bool) -> Ordering {
    match (a, b) {
        (SortValue::Null, SortValue::Null) => Ordering::Equal,
        (SortValue::Null, _) => {
            if nulls_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (_, SortValue::Null) => {
            if nulls_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (SortValue::Num(x), SortValue::Num(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (SortValue::Str(x), SortValue::Str(y)) => x.cmp(y),
        (SortValue::Bool(x), SortValue::Bool(y)) => x.cmp(y),
        // 类型不一致时按规范字符串比较（Collation 基线，F-TYP-007 后续细化）。
        _ => format!("{a:?}").cmp(&format!("{b:?}")),
    }
}

/// 多键比较（依次应用，desc 反转，NULL 序按各键配置）。
fn compare_rows(a: &Row, b: &Row, keys: &[SortKey]) -> Ordering {
    for key in keys {
        let av = &a[key.column];
        let bv = &b[key.column];
        let ord = cmp_values(&sort_value(av), &sort_value(bv), key.nulls_first);
        let ord = if key.desc { ord.reverse() } else { ord };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// 物化子执行器全部行。
fn materialize(
    child: &mut dyn Executor,
    schema: &RowSchema,
    ctx: &mut ExecCtx,
) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    for block in collect(child, ctx)? {
        out.extend(block_to_rows(&block, schema)?);
    }
    Ok(out)
}

/// 分块输出执行器基类（Sort/TopN/Window/Materialize 共用）。
#[derive(Debug)]
pub(crate) struct ChunkEmitter {
    pub(crate) result: Vec<Row>,
    pub(crate) cursor: usize,
    pub(crate) chunk_size: usize,
    pub(crate) stats: ExecStats,
}

impl ChunkEmitter {
    pub(crate) fn new() -> Self {
        Self {
            result: Vec::new(),
            cursor: 0,
            chunk_size: 1024,
            stats: ExecStats::default(),
        }
    }

    pub(crate) fn emit(&mut self, schema: &RowSchema) -> Result<Option<Block>> {
        if self.cursor >= self.result.len() {
            return Ok(None);
        }
        let end = (self.cursor + self.chunk_size).min(self.result.len());
        let chunk = &self.result[self.cursor..end];
        self.cursor = end;
        self.stats.record_rows(chunk.len());
        Ok(Some(rows_to_block(chunk, schema)?))
    }
}

/// OP-P-SORT：内存排序（多键、NULL 序、Collation 基线）。
#[derive(Debug)]
pub struct SortExecutor {
    child: Box<dyn Executor>,
    schema: RowSchema,
    keys: Vec<SortKey>,
    emitter: ChunkEmitter,
    closed: bool,
}

impl SortExecutor {
    pub fn new(child: Box<dyn Executor>, schema: RowSchema, keys: Vec<SortKey>) -> Result<Self> {
        if keys.iter().any(|k| k.column >= schema.len()) {
            return Err(Error::new(ErrorKind::Internal, "排序键列下标越界"));
        }
        Ok(Self {
            child,
            schema,
            keys,
            emitter: ChunkEmitter::new(),
            closed: false,
        })
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.schema
    }
}

impl Executor for SortExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let mut rows = materialize(&mut *self.child, &self.schema, ctx)?;
        rows.sort_by(|a, b| compare_rows(a, b, &self.keys));
        self.emitter.result = rows;
        self.emitter.cursor = 0;
        self.emitter.chunk_size = ctx.chunk_capacity.max(1);
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter.emit(&self.schema)
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.emitter.stats.merge(self.child.close()?);
            self.closed = true;
        }
        Ok(self.emitter.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-TOPN：ORDER BY + LIMIT 融合（L3 基线：排序后截断；L10 用堆优化）。
#[derive(Debug)]
pub struct TopNExecutor {
    child: Box<dyn Executor>,
    schema: RowSchema,
    keys: Vec<SortKey>,
    limit: u64,
    offset: u64,
    emitter: ChunkEmitter,
    closed: bool,
}

impl TopNExecutor {
    pub fn new(
        child: Box<dyn Executor>,
        schema: RowSchema,
        keys: Vec<SortKey>,
        limit: u64,
        offset: u64,
    ) -> Result<Self> {
        if keys.iter().any(|k| k.column >= schema.len()) {
            return Err(Error::new(ErrorKind::Internal, "TopN 排序键列下标越界"));
        }
        Ok(Self {
            child,
            schema,
            keys,
            limit,
            offset,
            emitter: ChunkEmitter::new(),
            closed: false,
        })
    }
}

impl Executor for TopNExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let mut rows = materialize(&mut *self.child, &self.schema, ctx)?;
        rows.sort_by(|a, b| compare_rows(a, b, &self.keys));
        let start = (self.offset as usize).min(rows.len());
        let end = (start + self.limit as usize).min(rows.len());
        self.emitter.result = rows[start..end].to_vec();
        self.emitter.cursor = 0;
        self.emitter.chunk_size = ctx.chunk_capacity.max(1);
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter.emit(&self.schema)
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.emitter.stats.merge(self.child.close()?);
            self.closed = true;
        }
        Ok(self.emitter.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-MATERIALIZE：物化全部 Chunk，可重复消费（docs/04 OP-P-MATERIALIZE）。
#[derive(Debug)]
pub struct MaterializeExecutor {
    child: Box<dyn Executor>,
    schema: RowSchema,
    emitter: ChunkEmitter,
    /// 物化字节数（供 MemoryTracker / 统计，docs/02 §10）。
    bytes: u64,
    closed: bool,
}

impl MaterializeExecutor {
    pub fn new(child: Box<dyn Executor>, schema: RowSchema) -> Self {
        Self {
            child,
            schema,
            emitter: ChunkEmitter::new(),
            bytes: 0,
            closed: false,
        }
    }

    pub fn materialized_bytes(&self) -> u64 {
        self.bytes
    }
}

impl Executor for MaterializeExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let rows = materialize(&mut *self.child, &self.schema, ctx)?;
        self.bytes = rows.iter().map(|r| r.len() as u64 * 16).sum::<u64>();
        ctx.tracker.try_alloc(self.bytes)?;
        self.emitter.result = rows;
        self.emitter.cursor = 0;
        self.emitter.chunk_size = ctx.chunk_capacity.max(1);
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter.emit(&self.schema)
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.emitter.stats.merge(self.child.close()?);
            self.closed = true;
        }
        Ok(self.emitter.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// 窗口函数种类（docs/02 §2.2 `WindowKind` 的物理实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowFunc {
    RowNumber,
    Rank,
    DenseRank,
    Sum,
    Avg,
}

/// 窗口 frame（docs/02 §2.2 `Frame` 的 L3 简化：ROWS 相对偏移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// 相对当前行的起始偏移（负数=之前行，i64::MIN = UNBOUNDED）。
    pub start: i64,
    /// 相对当前行的结束偏移（i64::MAX = UNBOUNDED）。
    pub end: i64,
}

impl Frame {
    pub fn unbounded_preceding_to_current() -> Self {
        Self {
            start: i64::MIN,
            end: 0,
        }
    }

    pub fn entire_partition() -> Self {
        Self {
            start: i64::MIN,
            end: i64::MAX,
        }
    }

    pub fn rows_between(start: i64, end: i64) -> Self {
        Self { start, end }
    }
}

/// 窗口规格：分区列 / 排序键 / frame / 函数列表（每个函数带参数列）。
#[derive(Debug, Clone)]
pub struct WindowSpec {
    pub partition_by: Vec<usize>,
    pub order_by: Vec<SortKey>,
    pub frame: Frame,
    pub functions: Vec<(WindowFunc, Option<usize>)>,
    pub out_schema: RowSchema,
}

impl WindowSpec {
    pub fn new(
        input_schema: &RowSchema,
        partition_by: Vec<usize>,
        order_by: Vec<SortKey>,
        frame: Frame,
        functions: Vec<(WindowFunc, Option<usize>)>,
    ) -> Result<Self> {
        if partition_by
            .iter()
            .chain(order_by.iter().map(|k| &k.column))
            .any(|i| *i >= input_schema.len())
        {
            return Err(Error::new(ErrorKind::Internal, "窗口分区/排序列下标越界"));
        }
        let mut cols = input_schema.columns.clone();
        for (i, (f, _)) in functions.iter().enumerate() {
            let ty = match f {
                WindowFunc::RowNumber | WindowFunc::Rank | WindowFunc::DenseRank => DataType::Int64,
                WindowFunc::Sum | WindowFunc::Avg => DataType::Float64,
            };
            cols.push((format!("win_{i}"), ty));
        }
        Ok(Self {
            partition_by,
            order_by,
            frame,
            functions,
            out_schema: RowSchema::new(cols),
        })
    }
}

fn partition_key(row: &Row, cols: &[usize]) -> String {
    cols.iter()
        .map(|i| format!("{:?}", row[*i]))
        .collect::<Vec<_>>()
        .join("\x1f")
}

/// OP-P-WINDOW：分区 + 排序 + frame 流式计算（docs/04 OP-P-WINDOW）。
#[derive(Debug)]
pub struct WindowExecutor {
    child: Box<dyn Executor>,
    input_schema: RowSchema,
    spec: WindowSpec,
    emitter: ChunkEmitter,
    closed: bool,
}

impl WindowExecutor {
    pub fn new(child: Box<dyn Executor>, input_schema: RowSchema, spec: WindowSpec) -> Self {
        Self {
            child,
            input_schema,
            spec,
            emitter: ChunkEmitter::new(),
            closed: false,
        }
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.spec.out_schema
    }
}

impl Executor for WindowExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let rows = materialize(&mut *self.child, &self.input_schema, ctx)?;
        self.emitter.chunk_size = ctx.chunk_capacity.max(1);

        // 分区：保持原始输入顺序，分区内按 order_by 排序。
        let mut partitions: Vec<Vec<Row>> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for row in rows {
            let key = partition_key(&row, &self.spec.partition_by);
            let idx = *index.entry(key.clone()).or_insert_with(|| {
                partitions.push(Vec::new());
                partitions.len() - 1
            });
            partitions[idx].push(row);
        }
        for part in &mut partitions {
            part.sort_by(|a, b| compare_rows(a, b, &self.spec.order_by));
        }

        let mut out = Vec::new();
        for part in partitions {
            let n = part.len();
            for (pos, row) in part.iter().enumerate() {
                let mut extended = row.clone();
                // 当前分区内 frame 行范围。
                let start = if self.spec.frame.start == i64::MIN {
                    0
                } else {
                    (pos as i64 + self.spec.frame.start).max(0) as usize
                };
                let end = if self.spec.frame.end == i64::MAX {
                    n - 1
                } else {
                    (pos as i64 + self.spec.frame.end).min(n as i64 - 1).max(0) as usize
                };
                for (func, arg) in &self.spec.functions {
                    let value = match func {
                        WindowFunc::RowNumber => Scalar::Int((pos + 1) as i128),
                        WindowFunc::Rank => {
                            // 同排序键的行共享 rank（L3 基线：按首行位置）。
                            let rank = part[..pos]
                                .iter()
                                .filter(|r| {
                                    compare_rows(r, row, &self.spec.order_by) == Ordering::Equal
                                })
                                .count();
                            Scalar::Int((pos + 1 - rank) as i128)
                        }
                        WindowFunc::DenseRank => {
                            let mut distinct = std::collections::HashSet::new();
                            for r in &part[..=pos] {
                                distinct.insert(
                                    self.spec
                                        .order_by
                                        .iter()
                                        .map(|k| format!("{:?}", r[k.column]))
                                        .collect::<Vec<_>>()
                                        .join("\x1f"),
                                );
                            }
                            Scalar::Int(distinct.len() as i128)
                        }
                        WindowFunc::Sum | WindowFunc::Avg => {
                            let mut sum = 0.0f64;
                            let mut count = 0i128;
                            if let Some(col) = arg {
                                for r in &part[start..=end] {
                                    if let Some(v) = scalar_num(&r[*col]) {
                                        sum += v;
                                        count += 1;
                                    }
                                }
                            }
                            if *func == WindowFunc::Sum {
                                if count == 0 {
                                    Scalar::Null
                                } else {
                                    Scalar::Float(sum)
                                }
                            } else if count == 0 {
                                Scalar::Null
                            } else {
                                Scalar::Float(sum / count as f64)
                            }
                        }
                    };
                    extended.push(value);
                }
                out.push(extended);
            }
        }
        self.emitter.result = out;
        self.emitter.cursor = 0;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter.emit(&self.spec.out_schema)
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.emitter.stats.merge(self.child.close()?);
            self.closed = true;
        }
        Ok(self.emitter.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

fn scalar_num(v: &Scalar) -> Option<f64> {
    match v {
        Scalar::Int(i) => Some(*i as f64),
        Scalar::Float(f) => Some(*f),
        Scalar::Decimal { unscaled, scale } => Some(*unscaled as f64 / 10f64.powi(*scale as i32)),
        _ => None,
    }
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
            vec![Scalar::Int(1), Scalar::Int(30)],
            vec![Scalar::Int(1), Scalar::Int(10)],
            vec![Scalar::Int(1), Scalar::Int(20)],
            vec![Scalar::Int(2), Scalar::Int(5)],
        ];
        (rows, s)
    }

    fn rows_of(exec: &mut dyn Executor, schema: &RowSchema) -> Result<Vec<Row>> {
        let mut ctx = ExecCtx::default();
        let blocks = collect(exec, &mut ctx)?;
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, schema)?);
        }
        Ok(out)
    }

    #[test]
    fn sort_ascending_and_descending() {
        let (rows, s) = data();
        let mut asc = SortExecutor::new(
            Box::new(TableScanExecutor::new(rows.clone(), s.clone())),
            s.clone(),
            vec![SortKey::asc(1)],
        )
        .unwrap();
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut asc, &mut ctx).unwrap();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &s).unwrap());
        }
        let vals: Vec<i128> = out
            .iter()
            .map(|r| if let Scalar::Int(v) = r[1] { v } else { 0 })
            .collect();
        assert_eq!(vals, vec![5, 10, 20, 30]);

        let mut desc = SortExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s.clone(),
            vec![SortKey::desc(1)],
        )
        .unwrap();
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut desc, &mut ctx).unwrap();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &s).unwrap());
        }
        let vals: Vec<i128> = out
            .iter()
            .map(|r| if let Scalar::Int(v) = r[1] { v } else { 0 })
            .collect();
        assert_eq!(vals, vec![30, 20, 10, 5]);
    }

    #[test]
    fn sort_nulls_first_and_last() {
        let s = RowSchema::new(vec![("a".into(), DataType::Int64)]);
        let rows = vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Null],
            vec![Scalar::Int(2)],
        ];
        let mut first = SortExecutor::new(
            Box::new(TableScanExecutor::new(rows.clone(), s.clone())),
            s.clone(),
            vec![SortKey {
                column: 0,
                desc: false,
                nulls_first: true,
            }],
        )
        .unwrap();
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut first, &mut ctx).unwrap();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &s).unwrap());
        }
        assert_eq!(out[0][0], Scalar::Null);

        let mut last = SortExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s.clone(),
            vec![SortKey {
                column: 0,
                desc: false,
                nulls_first: false,
            }],
        )
        .unwrap();
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut last, &mut ctx).unwrap();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &s).unwrap());
        }
        assert_eq!(out[2][0], Scalar::Null);
    }

    #[test]
    fn invalid_sort_key_is_rejected() {
        let (rows, s) = data();
        assert!(SortExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s,
            vec![SortKey::asc(9)],
        )
        .is_err());
    }

    #[test]
    fn topn_truncates_after_sort() {
        let (rows, s) = data();
        let mut exec = TopNExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s.clone(),
            vec![SortKey::asc(1)],
            2,
            1,
        )
        .unwrap();
        let out = rows_of(&mut exec, &s).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0][1], Scalar::Int(10));
        assert_eq!(out[1][1], Scalar::Int(20));
    }

    #[test]
    fn materialize_records_bytes() {
        let (rows, s) = data();
        let mut exec =
            MaterializeExecutor::new(Box::new(TableScanExecutor::new(rows, s.clone())), s.clone());
        let out = rows_of(&mut exec, &s).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(exec.materialized_bytes(), 4 * 2 * 16);
    }

    #[test]
    fn window_row_number_and_partition_sum() {
        let (rows, s) = data();
        let spec = WindowSpec::new(
            &s,
            vec![0],
            vec![SortKey::asc(1)],
            Frame::entire_partition(),
            vec![
                (WindowFunc::RowNumber, None),
                (WindowFunc::Sum, Some(1)),
                (WindowFunc::Avg, Some(1)),
            ],
        )
        .unwrap();
        let mut exec = WindowExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s,
            spec.clone(),
        );
        let out_schema = spec.out_schema.clone();
        let out = rows_of(&mut exec, &out_schema).unwrap();
        assert_eq!(out.len(), 4);
        // 分区 g=1 的 3 行：row_number 1,2,3；sum=60；avg=20
        let g1: Vec<&Row> = out.iter().filter(|r| r[0] == Scalar::Int(1)).collect();
        assert_eq!(g1.len(), 3);
        assert_eq!(g1[0][2], Scalar::Int(1));
        assert_eq!(g1[0][3], Scalar::Float(60.0));
        assert_eq!(g1[0][4], Scalar::Float(20.0));
    }

    #[test]
    fn window_frame_rows_between() {
        let s = RowSchema::new(vec![("v".into(), DataType::Int64)]);
        let rows = vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(2)],
            vec![Scalar::Int(3)],
            vec![Scalar::Int(4)],
        ];
        let spec = WindowSpec::new(
            &s,
            vec![],
            vec![SortKey::asc(0)],
            Frame::rows_between(-1, 0),
            vec![(WindowFunc::Sum, Some(0))],
        )
        .unwrap();
        let mut exec = WindowExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s,
            spec.clone(),
        );
        let out_schema = spec.out_schema.clone();
        let out = rows_of(&mut exec, &out_schema).unwrap();
        assert_eq!(out[0][1], Scalar::Float(1.0)); // 1
        assert_eq!(out[1][1], Scalar::Float(3.0)); // 1+2
        assert_eq!(out[2][1], Scalar::Float(5.0)); // 2+3
    }

    #[test]
    fn window_rank_handles_ties() {
        let s = RowSchema::new(vec![("v".into(), DataType::Int64)]);
        let rows = vec![
            vec![Scalar::Int(10)],
            vec![Scalar::Int(10)],
            vec![Scalar::Int(20)],
        ];
        let spec = WindowSpec::new(
            &s,
            vec![],
            vec![SortKey::asc(0)],
            Frame::entire_partition(),
            vec![(WindowFunc::Rank, None), (WindowFunc::DenseRank, None)],
        )
        .unwrap();
        let mut exec = WindowExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s,
            spec.clone(),
        );
        let out = rows_of(&mut exec, &spec.out_schema).unwrap();
        assert_eq!(out[0][1], Scalar::Int(1));
        assert_eq!(out[1][1], Scalar::Int(1)); // tie → 同 rank
        assert_eq!(out[2][1], Scalar::Int(3));
        assert_eq!(out[2][2], Scalar::Int(2)); // dense rank
    }
}
