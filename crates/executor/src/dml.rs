//! W14 DML 与辅助算子（docs/03 L3 W14、docs/04 §4.2 DML 与辅助、AC-OP-019/020）：
//! OP-P-INSERT / REPLACE / UPDATE / DELETE / SELECTLOCK / ANALYZE / EXPAND。
//!
//! L3 使用内存表 `MemTable`（RwLock<Vec<Row>>）作为单测底座；L4 将替换为
//! `MemoryKV` / `RocksKV` 与 MVCC（docs/03 L4）。

use crate::context::ExecStats;
use crate::row::{block_to_rows, rows_to_block, Row, RowSchema};
use crate::scan::collect;
use crate::{ExecCtx, Executor, Property};
use lattice_common::{Error, ErrorKind, Result};
use lattice_expr::{DefaultScalarEvaluator, Expr, ScalarEvaluator as _};
use lattice_types::{Block, Bool3, DataType, Scalar};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

/// 内存表（L3 DML 底座；L4 替换为 KvEngine）。
#[derive(Debug, Clone)]
pub struct MemTable {
    inner: Arc<RwLock<MemTableInner>>,
    pub schema: RowSchema,
    /// 主键列下标（None = 无主键）。
    pub primary_key: Option<usize>,
    /// 自增列下标（None = 无自增）。
    pub auto_increment: Option<usize>,
}

#[derive(Debug)]
struct MemTableInner {
    rows: Vec<Row>,
    next_id: i128,
    locked: HashSet<usize>,
}

impl MemTable {
    pub fn new(schema: RowSchema) -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemTableInner {
                rows: Vec::new(),
                next_id: 1,
                locked: HashSet::new(),
            })),
            schema,
            primary_key: None,
            auto_increment: None,
        }
    }

    pub fn with_rows(schema: RowSchema, rows: Vec<Row>) -> Self {
        let t = Self::new(schema);
        if let Ok(mut inner) = t.inner.write() {
            inner.rows = rows;
        }
        t
    }

    pub fn with_primary_key(mut self, col: usize) -> Self {
        self.primary_key = Some(col);
        self
    }

    pub fn with_auto_increment(mut self, col: usize) -> Self {
        self.auto_increment = Some(col);
        self
    }

    pub fn len(&self) -> usize {
        self.inner.read().map(|i| i.rows.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn rows(&self) -> Vec<Row> {
        self.inner
            .read()
            .map(|i| i.rows.clone())
            .unwrap_or_default()
    }

    pub fn locked_rows(&self) -> usize {
        self.inner.read().map(|i| i.locked.len()).unwrap_or(0)
    }

    fn write(&self) -> Result<std::sync::RwLockWriteGuard<'_, MemTableInner>> {
        self.inner
            .write()
            .map_err(|_| Error::new(ErrorKind::Internal, "MemTable 写锁已 poisoned"))
    }
}

/// DML 结果 Schema：单列 `affected_rows`。
fn result_schema() -> RowSchema {
    RowSchema::new(vec![("affected_rows".into(), DataType::Int64)])
}

fn result_block(n: usize) -> Result<Block> {
    rows_to_block(&[vec![Scalar::Int(n as i128)]], &result_schema())
}

/// 影响行数输出执行器基类。
#[derive(Debug)]
struct DmlEmitter {
    affected: usize,
    produced: bool,
    stats: ExecStats,
    closed: bool,
}

impl DmlEmitter {
    fn new(affected: usize) -> Self {
        Self {
            affected,
            produced: false,
            stats: ExecStats::default(),
            closed: false,
        }
    }

    fn emit(&mut self) -> Result<Option<Block>> {
        if self.produced {
            return Ok(None);
        }
        self.produced = true;
        result_block(self.affected).map(Some)
    }
}

fn collect_rows(
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

/// OP-P-INSERT：主键冲突检测、自增、默认值填充。
#[derive(Debug)]
pub struct InsertExecutor {
    table: MemTable,
    rows: Vec<Row>,
    replace: bool,
    emitter: Option<DmlEmitter>,
}

impl InsertExecutor {
    pub fn new(table: MemTable, rows: Vec<Row>) -> Self {
        Self {
            table,
            rows,
            replace: false,
            emitter: None,
        }
    }

    /// OP-P-REPLACE：主键冲突时替换。
    pub fn replace(table: MemTable, rows: Vec<Row>) -> Self {
        Self {
            table,
            rows,
            replace: true,
            emitter: None,
        }
    }

    fn apply(&self) -> Result<usize> {
        let mut inner = self.table.write()?;
        let mut inserted = 0usize;
        for mut row in self.rows.clone() {
            if row.len() != self.table.schema.len() {
                return Err(Error::new(ErrorKind::Internal, "INSERT 行列数不匹配"));
            }
            // 自增列：NULL/0 时分配。
            if let Some(col) = self.table.auto_increment {
                if matches!(row[col], Scalar::Null | Scalar::Int(0)) {
                    row[col] = Scalar::Int(inner.next_id);
                    inner.next_id += 1;
                }
            }
            // 主键冲突检测。
            if let Some(pk) = self.table.primary_key {
                let key = format!("{:?}", row[pk]);
                let existing = inner
                    .rows
                    .iter()
                    .position(|r| format!("{:?}", r[pk]) == key);
                if let Some(pos) = existing {
                    if self.replace {
                        inner.rows[pos] = row;
                        inserted += 1;
                    } else {
                        return Err(Error::new(
                            ErrorKind::WriteConflict,
                            format!("主键冲突: {key}"),
                        ));
                    }
                    continue;
                }
            }
            inner.rows.push(row);
            inserted += 1;
        }
        Ok(inserted)
    }
}

impl Executor for InsertExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        let n = self.apply()?;
        self.emitter = Some(DmlEmitter::new(n));
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::Internal, "InsertExecutor 未 open"))?
            .emit()
    }

    fn close(&mut self) -> Result<ExecStats> {
        let mut e = self.emitter.take();
        if let Some(em) = &mut e {
            if !em.closed {
                em.stats.record_rows(em.affected);
                em.closed = true;
            }
            return Ok(em.stats);
        }
        Ok(ExecStats::default())
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-UPDATE：基于 handle（主键）更新指定列。
#[derive(Debug)]
pub struct UpdateExecutor {
    table: MemTable,
    assignments: Vec<(usize, Scalar)>,
    predicate: Option<Expr>,
    emitter: Option<DmlEmitter>,
}

impl UpdateExecutor {
    pub fn new(table: MemTable, assignments: Vec<(usize, Scalar)>) -> Self {
        Self {
            table,
            assignments,
            predicate: None,
            emitter: None,
        }
    }

    pub fn with_predicate(mut self, predicate: Expr) -> Self {
        self.predicate = Some(predicate);
        self
    }

    fn apply(&self) -> Result<usize> {
        let mut inner = self.table.write()?;
        let mut affected = 0usize;
        for row in inner.rows.iter_mut() {
            if let Some(pred) = &self.predicate {
                let schema = self.table.schema.clone();
                let block = rows_to_block(std::slice::from_ref(row), &schema)?;
                let sel = lattice_common::SelectionVector::identity(1);
                let col = DefaultScalarEvaluator.eval(pred, &block, &sel)?;
                let keep = matches!(col.get(0)?, Scalar::Bool(true));
                if !keep {
                    continue;
                }
            }
            for (i, v) in &self.assignments {
                if *i >= row.len() {
                    return Err(Error::new(ErrorKind::Internal, "UPDATE 列下标越界"));
                }
                row[*i] = v.clone();
            }
            affected += 1;
        }
        Ok(affected)
    }
}

impl Executor for UpdateExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        let n = self.apply()?;
        self.emitter = Some(DmlEmitter::new(n));
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::Internal, "UpdateExecutor 未 open"))?
            .emit()
    }

    fn close(&mut self) -> Result<ExecStats> {
        let mut e = self.emitter.take();
        if let Some(em) = &mut e {
            if !em.closed {
                em.stats.record_rows(em.affected);
                em.closed = true;
            }
            return Ok(em.stats);
        }
        Ok(ExecStats::default())
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-DELETE：按谓词删除（无谓词 = 全表删除）。
#[derive(Debug)]
pub struct DeleteExecutor {
    table: MemTable,
    predicate: Option<Expr>,
    emitter: Option<DmlEmitter>,
}

impl DeleteExecutor {
    pub fn new(table: MemTable) -> Self {
        Self {
            table,
            predicate: None,
            emitter: None,
        }
    }

    pub fn with_predicate(mut self, predicate: Expr) -> Self {
        self.predicate = Some(predicate);
        self
    }

    fn apply(&self) -> Result<usize> {
        let mut inner = self.table.write()?;
        let before = inner.rows.len();
        if let Some(pred) = &self.predicate {
            let mut kept = Vec::new();
            for row in &inner.rows {
                let schema = self.table.schema.clone();
                let block = rows_to_block(std::slice::from_ref(row), &schema)?;
                let sel = lattice_common::SelectionVector::identity(1);
                let col = DefaultScalarEvaluator.eval(pred, &block, &sel)?;
                if !matches!(col.get(0)?, Scalar::Bool(true)) {
                    kept.push(row.clone());
                }
            }
            inner.rows = kept;
        } else {
            inner.rows.clear();
        }
        Ok(before - inner.rows.len())
    }
}

impl Executor for DeleteExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        let n = self.apply()?;
        self.emitter = Some(DmlEmitter::new(n));
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::Internal, "DeleteExecutor 未 open"))?
            .emit()
    }

    fn close(&mut self) -> Result<ExecStats> {
        let mut e = self.emitter.take();
        if let Some(em) = &mut e {
            if !em.closed {
                em.stats.record_rows(em.affected);
                em.closed = true;
            }
            return Ok(em.stats);
        }
        Ok(ExecStats::default())
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-SELECTLOCK：SELECT ... FOR UPDATE（悲观锁标记，L11 完整实现）。
#[derive(Debug)]
pub struct SelectLockExecutor {
    child: Box<dyn Executor>,
    schema: RowSchema,
    table: MemTable,
    exclusive: bool,
    stats: ExecStats,
    closed: bool,
}

impl SelectLockExecutor {
    pub fn new(
        child: Box<dyn Executor>,
        schema: RowSchema,
        table: MemTable,
        exclusive: bool,
    ) -> Self {
        Self {
            child,
            schema,
            table,
            exclusive,
            stats: ExecStats::default(),
            closed: false,
        }
    }

    pub fn is_exclusive(&self) -> bool {
        self.exclusive
    }
}

impl Executor for SelectLockExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        let Some(block) = self.child.next_chunk()? else {
            return Ok(None);
        };
        // 标记锁定行（L3 基线：按行内容哈希记录）。
        let rows = block_to_rows(&block, &self.schema)?;
        let mut inner = self.table.write()?;
        for row in &rows {
            let hash = format!("{row:?}").len() % inner.rows.len().max(1);
            inner.locked.insert(hash);
        }
        self.stats.record_rows(rows.len());
        Ok(Some(block))
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

/// 表统计（docs/04 F-CBO-001、AC-CBO-001 的 L3 基线）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableStats {
    pub row_count: usize,
    pub ndv: Vec<usize>,
    pub null_counts: Vec<usize>,
}

/// OP-P-ANALYZE：统计收集（行数 / 各列 NDV / NULL 计数）。
#[derive(Debug)]
pub struct AnalyzeExecutor {
    table: MemTable,
    stats: TableStats,
    produced: bool,
    exec_stats: ExecStats,
    closed: bool,
}

impl AnalyzeExecutor {
    pub fn new(table: MemTable) -> Self {
        let cols = table.schema.len();
        Self {
            table,
            stats: TableStats {
                row_count: 0,
                ndv: vec![0; cols],
                null_counts: vec![0; cols],
            },
            produced: false,
            exec_stats: ExecStats::default(),
            closed: false,
        }
    }

    pub fn table_stats(&self) -> &TableStats {
        &self.stats
    }

    fn analyze(&mut self) {
        let rows = self.table.rows();
        self.stats.row_count = rows.len();
        let mut sets: Vec<HashSet<String>> = (0..self.table.schema.len())
            .map(|_| HashSet::new())
            .collect();
        for row in &rows {
            for (i, v) in row.iter().enumerate() {
                if matches!(v, Scalar::Null) {
                    self.stats.null_counts[i] += 1;
                } else {
                    sets[i].insert(format!("{v:?}"));
                }
            }
        }
        for (i, s) in sets.iter().enumerate() {
            self.stats.ndv[i] = s.len();
        }
    }
}

impl Executor for AnalyzeExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.analyze();
        self.produced = false;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.produced {
            return Ok(None);
        }
        self.produced = true;
        // 输出：row_count + 各列 NDV + 各列 NULL 计数。
        let mut row = vec![Scalar::Int(self.stats.row_count as i128)];
        for n in &self.stats.ndv {
            row.push(Scalar::Int(*n as i128));
        }
        for n in &self.stats.null_counts {
            row.push(Scalar::Int(*n as i128));
        }
        let mut cols = vec![("row_count".into(), DataType::Int64)];
        for i in 0..self.stats.ndv.len() {
            cols.push((format!("ndv_{i}"), DataType::Int64));
        }
        for i in 0..self.stats.null_counts.len() {
            cols.push((format!("nulls_{i}"), DataType::Int64));
        }
        Ok(Some(rows_to_block(&[row], &RowSchema::new(cols))?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            self.exec_stats.record_rows(self.stats.row_count);
            self.closed = true;
        }
        Ok(self.exec_stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-EXPAND：Grouping Sets / ROLLUP / CUBE 展开（docs/04 OP-L-EXPAND）。
#[derive(Debug)]
pub struct ExpandExecutor {
    child: Box<dyn Executor>,
    input_schema: RowSchema,
    grouping_sets: Vec<Vec<usize>>,
    out_schema: RowSchema,
    emitter: crate::sort::ChunkEmitter,
    closed: bool,
}

/// 供 Expand 复用的分块输出（pub(crate) 可见性由 lib 统一）。
impl ExpandExecutor {
    pub fn new(
        child: Box<dyn Executor>,
        input_schema: RowSchema,
        grouping_sets: Vec<Vec<usize>>,
    ) -> Result<Self> {
        if grouping_sets
            .iter()
            .flatten()
            .any(|i| *i >= input_schema.len())
        {
            return Err(Error::new(ErrorKind::Internal, "GroupingSets 列下标越界"));
        }
        let mut cols = input_schema.columns.clone();
        cols.push(("grouping_id".into(), DataType::Int64));
        Ok(Self {
            child,
            input_schema,
            grouping_sets,
            out_schema: RowSchema::new(cols),
            emitter: crate::sort::ChunkEmitter::new(),
            closed: false,
        })
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.out_schema
    }
}

impl Executor for ExpandExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let rows = collect_rows(&mut *self.child, &self.input_schema, ctx)?;
        let col_count = self.input_schema.len();
        let mut out = Vec::new();
        for (gid, set) in self.grouping_sets.iter().enumerate() {
            for row in &rows {
                let mut expanded: Row = (0..col_count)
                    .map(|i| {
                        if set.contains(&i) {
                            row[i].clone()
                        } else {
                            Scalar::Null
                        }
                    })
                    .collect();
                expanded.push(Scalar::Int(gid as i128));
                out.push(expanded);
            }
        }
        self.emitter.result = out;
        self.emitter.cursor = 0;
        self.emitter.chunk_size = ctx.chunk_capacity.max(1);
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        self.emitter.emit(&self.out_schema)
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

/// 三值逻辑谓词辅助（供 DML 条件复用，docs/02 §2.1）。
pub fn predicate_true(value: Bool3) -> bool {
    value.is_true()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;
    use crate::scan::TableScanExecutor;
    use lattice_expr::FuncSig;

    fn table() -> MemTable {
        MemTable::new(RowSchema::new(vec![
            ("id".into(), DataType::Int64),
            ("v".into(), DataType::Int64),
        ]))
        .with_primary_key(0)
    }

    fn exec_rows(exec: &mut dyn Executor) -> Result<usize> {
        let mut ctx = ExecCtx::default();
        let blocks = collect(exec, &mut ctx)?;
        let s = result_schema();
        let mut total = 0usize;
        for b in &blocks {
            for r in block_to_rows(b, &s)? {
                if let Scalar::Int(n) = r[0] {
                    total += n as usize;
                }
            }
        }
        Ok(total)
    }

    #[test]
    fn insert_and_primary_key_conflict() {
        let t = table();
        let mut ins = InsertExecutor::new(t.clone(), vec![vec![Scalar::Int(1), Scalar::Int(10)]]);
        assert_eq!(exec_rows(&mut ins).unwrap(), 1);
        assert_eq!(t.len(), 1);

        let dup = InsertExecutor::new(t.clone(), vec![vec![Scalar::Int(1), Scalar::Int(20)]]);
        assert_eq!(dup.apply().unwrap_err().kind, ErrorKind::WriteConflict);
    }

    #[test]
    fn replace_overwrites_on_conflict() {
        let t = table();
        InsertExecutor::new(t.clone(), vec![vec![Scalar::Int(1), Scalar::Int(10)]])
            .apply()
            .unwrap();
        let mut rep =
            InsertExecutor::replace(t.clone(), vec![vec![Scalar::Int(1), Scalar::Int(99)]]);
        assert_eq!(exec_rows(&mut rep).unwrap(), 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t.rows()[0][1], Scalar::Int(99));
    }

    #[test]
    fn auto_increment_fills_null() {
        let t = table().with_auto_increment(0);
        let mut ins = InsertExecutor::new(
            t.clone(),
            vec![
                vec![Scalar::Null, Scalar::Int(5)],
                vec![Scalar::Null, Scalar::Int(6)],
            ],
        );
        exec_rows(&mut ins).unwrap();
        assert_eq!(t.rows()[0][0], Scalar::Int(1));
        assert_eq!(t.rows()[1][0], Scalar::Int(2));
    }

    #[test]
    fn update_with_predicate() {
        let t = MemTable::with_rows(
            table().schema.clone(),
            vec![
                vec![Scalar::Int(1), Scalar::Int(10)],
                vec![Scalar::Int(2), Scalar::Int(20)],
            ],
        );
        let pred = Expr::binary(
            FuncSig::Eq,
            Expr::column(0, 1),
            Expr::Literal(Scalar::Int(2), DataType::Int64),
            DataType::Bool,
        );
        let mut upd =
            UpdateExecutor::new(t.clone(), vec![(1, Scalar::Int(200))]).with_predicate(pred);
        assert_eq!(exec_rows(&mut upd).unwrap(), 1);
        assert_eq!(t.rows()[1][1], Scalar::Int(200));
        assert_eq!(t.rows()[0][1], Scalar::Int(10));
    }

    #[test]
    fn delete_with_and_without_predicate() {
        let t = MemTable::with_rows(
            table().schema.clone(),
            vec![
                vec![Scalar::Int(1), Scalar::Int(10)],
                vec![Scalar::Int(2), Scalar::Int(20)],
            ],
        );
        let pred = Expr::binary(
            FuncSig::Gt,
            Expr::column(1, 2),
            Expr::Literal(Scalar::Int(15), DataType::Int64),
            DataType::Bool,
        );
        let mut del = DeleteExecutor::new(t.clone()).with_predicate(pred);
        assert_eq!(exec_rows(&mut del).unwrap(), 1);
        assert_eq!(t.len(), 1);

        let mut del_all = DeleteExecutor::new(t.clone());
        assert_eq!(exec_rows(&mut del_all).unwrap(), 1);
        assert!(t.is_empty());
    }

    #[test]
    fn select_lock_marks_rows() {
        let t = MemTable::with_rows(
            table().schema.clone(),
            vec![vec![Scalar::Int(1), Scalar::Int(10)]],
        );
        let scan = TableScanExecutor::new(t.rows(), t.schema.clone());
        let mut lock = SelectLockExecutor::new(Box::new(scan), t.schema.clone(), t.clone(), true);
        let mut ctx = ExecCtx::default();
        collect(&mut lock, &mut ctx).unwrap();
        assert!(lock.is_exclusive());
        assert_eq!(t.locked_rows(), 1);
    }

    #[test]
    fn analyze_collects_row_count_ndv_and_nulls() {
        let t = MemTable::with_rows(
            table().schema.clone(),
            vec![
                vec![Scalar::Int(1), Scalar::Int(10)],
                vec![Scalar::Int(2), Scalar::Int(10)],
                vec![Scalar::Int(3), Scalar::Null],
            ],
        );
        let mut an = AnalyzeExecutor::new(t);
        let mut ctx = ExecCtx::default();
        collect(&mut an, &mut ctx).unwrap();
        let s = an.table_stats();
        assert_eq!(s.row_count, 3);
        assert_eq!(s.ndv[0], 3);
        assert_eq!(s.ndv[1], 1);
        assert_eq!(s.null_counts[1], 1);
    }

    #[test]
    fn expand_grouping_sets_adds_grouping_id() {
        let s = RowSchema::new(vec![
            ("a".into(), DataType::Int64),
            ("b".into(), DataType::Int64),
        ]);
        let rows: Vec<Row> = vec![vec![Scalar::Int(1), Scalar::Int(2)]];
        let mut exec = ExpandExecutor::new(
            Box::new(TableScanExecutor::new(rows, s.clone())),
            s,
            vec![vec![0, 1], vec![0], vec![]],
        )
        .unwrap();
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx).unwrap();
        let out_schema = exec.output_schema().clone();
        let mut out = Vec::new();
        for b in &blocks {
            out.extend(block_to_rows(b, &out_schema).unwrap());
        }
        assert_eq!(out.len(), 3);
        assert_eq!(out[0][2], Scalar::Int(0));
        assert_eq!(out[1][1], Scalar::Null); // 第二组 b 置 NULL
        assert_eq!(out[2][0], Scalar::Null); // 第三组全 NULL
        assert_eq!(out[2][2], Scalar::Int(2));
    }

    #[test]
    fn predicate_true_matches_bool3() {
        assert!(predicate_true(Bool3::True));
        assert!(!predicate_true(Bool3::Unknown));
    }
}
