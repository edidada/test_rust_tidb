//! W10 一元算子：Projection / Filter / Limit（docs/03 L3 W10、docs/04 §4.2）。

use crate::context::ExecStats;
use crate::row::{block_take, block_to_rows, rows_to_block, RowSchema};
use crate::{ExecCtx, Executor, Property};
use lattice_common::{Error, ErrorKind, Result, SelectionVector};
use lattice_expr::{DefaultScalarEvaluator, Expr, ScalarEvaluator as _};
use lattice_types::{Block, DataType, Scalar};

/// OP-P-PROJECTION：对子块的每行求值表达式列表。
#[derive(Debug)]
pub struct ProjectionExecutor {
    child: Box<dyn Executor>,
    exprs: Vec<Expr>,
    output_schema: RowSchema,
    stats: ExecStats,
    closed: bool,
}

impl ProjectionExecutor {
    pub fn new(child: Box<dyn Executor>, exprs: Vec<Expr>, aliases: Vec<String>) -> Result<Self> {
        if exprs.len() != aliases.len() {
            return Err(Error::new(
                ErrorKind::Internal,
                "投影表达式数与别名数不一致",
            ));
        }
        let output_schema = RowSchema::new(
            exprs
                .iter()
                .zip(&aliases)
                .map(|(e, a)| (a.clone(), e.data_type()))
                .collect(),
        );
        Ok(Self {
            child,
            exprs,
            output_schema,
            stats: ExecStats::default(),
            closed: false,
        })
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.output_schema
    }
}

impl Executor for ProjectionExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        let Some(block) = self.child.next_chunk()? else {
            return Ok(None);
        };
        let evaluator = DefaultScalarEvaluator;
        let sel = SelectionVector::identity(block.num_rows);
        let mut columns = Vec::with_capacity(self.exprs.len());
        for expr in &self.exprs {
            columns.push(evaluator.eval(expr, &block, &sel)?);
        }
        self.stats.record_rows(block.num_rows);
        Ok(Some(lattice_types::block_from_columns(columns)?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            let child_stats = self.child.close()?;
            self.stats.merge(child_stats);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        self.child.required_property()
    }
}

/// OP-P-SELECTION：谓词三值逻辑，仅保留 Bool3::True（docs/02 §2.1）。
#[derive(Debug)]
pub struct FilterExecutor {
    child: Box<dyn Executor>,
    predicate: Expr,
    stats: ExecStats,
    closed: bool,
}

impl FilterExecutor {
    pub fn new(child: Box<dyn Executor>, predicate: Expr) -> Self {
        Self {
            child,
            predicate,
            stats: ExecStats::default(),
            closed: false,
        }
    }
}

impl Executor for FilterExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        loop {
            let Some(block) = self.child.next_chunk()? else {
                return Ok(None);
            };
            if block.num_rows == 0 {
                continue;
            }
            let evaluator = DefaultScalarEvaluator;
            let sel = SelectionVector::identity(block.num_rows);
            let pred_col = evaluator.eval(&self.predicate, &block, &sel)?;
            if pred_col.data_type != DataType::Bool && pred_col.data_type != DataType::Null {
                return Err(Error::new(
                    ErrorKind::TypeMismatch,
                    "Filter 谓词结果必须是 BOOL",
                ));
            }
            let mut kept = Vec::new();
            for row in 0..block.num_rows {
                match pred_col.get(row)? {
                    Scalar::Bool(true) => kept.push(row as u32),
                    Scalar::Bool(false) | Scalar::Null => {}
                    other => {
                        return Err(Error::new(
                            ErrorKind::TypeMismatch,
                            format!("谓词返回非布尔值 {other:?}"),
                        ))
                    }
                }
            }
            if kept.is_empty() {
                // 全 Block 被过滤：继续拉下一个 chunk，不返回空块。
                continue;
            }
            self.stats.record_rows(kept.len());
            return Ok(Some(block_take(&block, &SelectionVector(kept))?));
        }
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            let child_stats = self.child.close()?;
            self.stats.merge(child_stats);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        self.child.required_property()
    }
}

/// OP-P-LIMIT：OFFSET / LIMIT，提前终止（docs/04 OP-P-LIMIT）。
#[derive(Debug)]
pub struct LimitExecutor {
    child: Box<dyn Executor>,
    offset: u64,
    limit: u64,
    seen: u64,
    emitted: u64,
    finished: bool,
    stats: ExecStats,
    closed: bool,
}

impl LimitExecutor {
    pub fn new(child: Box<dyn Executor>, offset: u64, limit: u64) -> Self {
        Self {
            child,
            offset,
            limit,
            seen: 0,
            emitted: 0,
            finished: false,
            stats: ExecStats::default(),
            closed: false,
        }
    }
}

impl Executor for LimitExecutor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.seen = 0;
        self.emitted = 0;
        self.finished = false;
        self.child.open(ctx)
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.finished {
            return Ok(None);
        }
        loop {
            let Some(block) = self.child.next_chunk()? else {
                self.finished = true;
                return Ok(None);
            };
            let mut kept = Vec::new();
            for row in 0..block.num_rows {
                let global = self.seen + row as u64;
                if global >= self.offset && (self.emitted + kept.len() as u64) < self.limit {
                    kept.push(row as u32);
                }
            }
            self.seen += block.num_rows as u64;
            if kept.is_empty() {
                if self.emitted >= self.limit {
                    self.finished = true;
                    return Ok(None);
                }
                continue;
            }
            let kept_count = kept.len() as u64;
            let out = block_take(&block, &SelectionVector(kept))?;
            self.emitted += kept_count;
            self.stats.record_rows(kept_count as usize);
            if self.emitted >= self.limit {
                self.finished = true;
            }
            return Ok(Some(out));
        }
    }

    fn close(&mut self) -> Result<ExecStats> {
        if !self.closed {
            let child_stats = self.child.close()?;
            self.stats.merge(child_stats);
            self.closed = true;
        }
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        self.child.required_property()
    }
}

/// 便捷：把执行结果按行读出（差分测试辅助）。
pub fn collect_rows(
    exec: &mut dyn Executor,
    schema: &RowSchema,
    ctx: &mut ExecCtx,
) -> Result<Vec<crate::row::Row>> {
    exec.open(ctx)?;
    let mut out = Vec::new();
    while let Some(block) = exec.next_chunk()? {
        out.extend(block_to_rows(&block, schema)?);
    }
    exec.close()?;
    Ok(out)
}

/// 便捷：从行向量构造一个一次性 Block 执行器（差分测试对照基线）。
pub fn rows_executor(rows: Vec<crate::row::Row>, schema: RowSchema) -> Result<Box<dyn Executor>> {
    Ok(Box::new(crate::scan::TableScanExecutor::new(rows, schema)))
}

/// 便捷：把行向量转 Block（测试构造用）。
pub fn block_of(rows: &[crate::row::Row], schema: &RowSchema) -> Result<Block> {
    rows_to_block(rows, schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;
    use crate::scan::TableScanExecutor;
    use lattice_expr::FuncSig;
    use lattice_types::Scalar;

    fn scan() -> (Box<dyn Executor>, RowSchema) {
        let schema = RowSchema::new(vec![
            ("a".into(), DataType::Int64),
            ("b".into(), DataType::Int64),
        ]);
        let rows: Vec<Row> = (1..=5)
            .map(|i| vec![Scalar::Int(i), Scalar::Int(i * 10)])
            .collect();
        (
            Box::new(TableScanExecutor::new(rows, schema.clone())),
            schema,
        )
    }

    fn col(index: usize, id: u64) -> Expr {
        Expr::column(index, id)
    }

    #[test]
    fn projection_evaluates_expressions() {
        let (child, _schema) = scan();
        let expr = Expr::binary(
            FuncSig::Add,
            col(0, 1),
            Expr::Literal(Scalar::Int(100), DataType::Int64),
            DataType::Int64,
        );
        let mut exec = ProjectionExecutor::new(child, vec![expr], vec!["x".into()]).unwrap();
        let mut ctx = ExecCtx::default();
        let rows = collect_rows(
            &mut exec,
            &RowSchema::new(vec![("x".into(), DataType::Int64)]),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0][0], Scalar::Int(101));
        assert_eq!(rows[4][0], Scalar::Int(105));
    }

    #[test]
    fn projection_arity_check() {
        let (child, _) = scan();
        assert!(ProjectionExecutor::new(child, vec![col(0, 1)], vec![]).is_err());
    }

    #[test]
    fn filter_keeps_only_true_three_valued() {
        let (child, _) = scan();
        let pred = Expr::binary(
            FuncSig::Gt,
            col(0, 1),
            Expr::Literal(Scalar::Int(2), DataType::Int64),
            DataType::Bool,
        );
        let mut exec = FilterExecutor::new(child, pred);
        let mut ctx = ExecCtx::default();
        let blocks = crate::scan::collect(&mut exec, &mut ctx).unwrap();
        let total: usize = blocks.iter().map(|b| b.num_rows).sum();
        assert_eq!(total, 3);
        assert_eq!(blocks[0].columns[0].get(0).unwrap(), Scalar::Int(3));
    }

    #[test]
    fn filter_null_predicate_row_is_dropped() {
        let schema = RowSchema::new(vec![("a".into(), DataType::Int64)]);
        let rows = vec![
            vec![Scalar::Null],
            vec![Scalar::Int(5)],
            vec![Scalar::Int(1)],
        ];
        let child = Box::new(TableScanExecutor::new(rows, schema));
        let pred = Expr::binary(
            FuncSig::Gt,
            col(0, 1),
            Expr::Literal(Scalar::Int(2), DataType::Int64),
            DataType::Bool,
        );
        let mut exec = FilterExecutor::new(child, pred);
        let mut ctx = ExecCtx::default();
        let blocks = crate::scan::collect(&mut exec, &mut ctx).unwrap();
        assert_eq!(blocks[0].num_rows, 1);
        assert_eq!(blocks[0].columns[0].get(0).unwrap(), Scalar::Int(5));
    }

    #[test]
    fn limit_offset_and_early_stop() {
        let (child, _) = scan();
        let mut exec = LimitExecutor::new(child, 1, 2);
        let mut ctx = ExecCtx::default();
        let blocks = crate::scan::collect(&mut exec, &mut ctx).unwrap();
        let total: usize = blocks.iter().map(|b| b.num_rows).sum();
        assert_eq!(total, 2);
        assert_eq!(blocks[0].columns[0].get(0).unwrap(), Scalar::Int(2));
    }

    #[test]
    fn limit_zero_returns_nothing() {
        let (child, _) = scan();
        let mut exec = LimitExecutor::new(child, 0, 0);
        let mut ctx = ExecCtx::default();
        assert!(crate::scan::collect(&mut exec, &mut ctx)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn stats_accumulate_actual_rows() {
        let (child, _) = scan();
        let mut exec = LimitExecutor::new(child, 0, 3);
        let mut ctx = ExecCtx::default();
        crate::scan::collect(&mut exec, &mut ctx).unwrap();
        assert_eq!(exec.close().unwrap().actual_rows, 3 + 5); // 输出 + 子节点扫描
    }
}
