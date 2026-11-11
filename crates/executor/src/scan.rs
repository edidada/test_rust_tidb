//! W10 扫描类算子（docs/03 L3 W10、docs/04 §4.2）：
//! OP-P-TABLESCAN / OP-P-CTESCAN / OP-P-MEMTABLE / OP-P-TABLEDUAL。

use crate::context::ExecStats;
use crate::row::{rows_to_block, Row, RowSchema};
use crate::{ExecCtx, Executor, Property};
use lattice_common::{Error, ErrorKind, Result};
use lattice_types::Block;

/// OP-P-TABLESCAN / MEMTABLE：内存表全表扫描（行式火山模型）。
///
/// 列裁剪由 `projection` 指定（None = 全列）；按 `ctx.chunk_capacity` 分批输出 Block。
#[derive(Debug)]
pub struct TableScanExecutor {
    rows: Vec<Row>,
    schema: RowSchema,
    projection: Option<Vec<usize>>,
    cursor: usize,
    output_schema: RowSchema,
    stats: ExecStats,
}

impl TableScanExecutor {
    pub fn new(rows: Vec<Row>, schema: RowSchema) -> Self {
        let output_schema = schema.clone();
        Self {
            rows,
            schema,
            projection: None,
            cursor: 0,
            output_schema,
            stats: ExecStats::default(),
        }
    }

    /// 列裁剪（docs/04 AC-RBO-003 前置：扫描只输出所需列）。
    pub fn with_projection(mut self, projection: Vec<usize>) -> Result<Self> {
        if projection.iter().any(|i| *i >= self.schema.len()) {
            return Err(Error::new(ErrorKind::Internal, "列裁剪下标越界"));
        }
        self.output_schema = RowSchema::new(
            projection
                .iter()
                .map(|i| self.schema.columns[*i].clone())
                .collect(),
        );
        self.projection = Some(projection);
        Ok(self)
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.output_schema
    }
}

impl Executor for TableScanExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.cursor = 0;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let end = (self.cursor + 1024).min(self.rows.len());
        let mut chunk: Vec<Row> = Vec::with_capacity(end - self.cursor);
        for row in &self.rows[self.cursor..end] {
            match &self.projection {
                Some(p) => chunk.push(p.iter().map(|i| row[*i].clone()).collect()),
                None => chunk.push(row.clone()),
            }
        }
        self.cursor = end;
        self.stats.record_rows(chunk.len());
        Ok(Some(rows_to_block(&chunk, &self.output_schema)?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-CTESCAN：物化一次、多次复用（L3 简化：持有已物化的行向量）。
#[derive(Debug, Clone)]
pub struct CteScanExecutor {
    state: std::sync::Arc<std::sync::Mutex<CteState>>,
    schema: RowSchema,
}

#[derive(Debug)]
struct CteState {
    rows: Vec<Row>,
    draining: bool,
}

impl CteScanExecutor {
    /// 由子执行器物化（CTE producer 只执行一次）。
    pub fn materialize(
        child: &mut dyn Executor,
        schema: RowSchema,
        ctx: &mut ExecCtx,
    ) -> Result<Self> {
        child.open(ctx)?;
        let mut rows = Vec::new();
        while let Some(block) = child.next_chunk()? {
            rows.extend(crate::row::block_to_rows(&block, &schema)?);
        }
        child.close()?;
        Ok(Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(CteState {
                rows,
                draining: false,
            })),
            schema,
        })
    }

    pub fn output_schema(&self) -> &RowSchema {
        &self.schema
    }
}

impl Executor for CteScanExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "CTE 状态锁已 poisoned"))?;
        state.draining = false;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "CTE 状态锁已 poisoned"))?;
        if state.draining {
            return Ok(None);
        }
        state.draining = true;
        rows_to_block(&state.rows, &self.schema).map(Some)
    }

    fn close(&mut self) -> Result<ExecStats> {
        let rows = self.state.lock().map(|s| s.rows.len()).unwrap_or(0);
        Ok(ExecStats {
            actual_rows: rows as u64,
            ..Default::default()
        })
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// OP-P-TABLEDUAL：产生 1 行 0 列（docs/04 AC-OP-001）。
#[derive(Debug, Default)]
pub struct TableDualExecutor {
    produced: bool,
    stats: ExecStats,
}

impl Executor for TableDualExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.produced = false;
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.produced {
            return Ok(None);
        }
        self.produced = true;
        self.stats.record_rows(1);
        // 0 列但 1 行（docs/04 OP-P-TABLEDUAL）。
        Ok(Some(lattice_types::BlockBuilder::new(1).rows(1).build()?))
    }

    fn close(&mut self) -> Result<ExecStats> {
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

/// 收集执行器的全部 Block（差分测试与算子单测辅助）。
pub fn collect(exec: &mut dyn Executor, ctx: &mut ExecCtx) -> Result<Vec<Block>> {
    exec.open(ctx)?;
    let mut out = Vec::new();
    while let Some(block) = exec.next_chunk()? {
        out.push(block);
    }
    exec.close()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_types::{DataType, Scalar};

    fn data() -> (Vec<Row>, RowSchema) {
        let schema = RowSchema::new(vec![
            ("a".into(), DataType::Int64),
            ("b".into(), DataType::VarChar(32)),
        ]);
        let rows = vec![
            vec![Scalar::Int(1), Scalar::Str("x".into())],
            vec![Scalar::Int(2), Scalar::Str("y".into())],
            vec![Scalar::Int(3), Scalar::Null],
        ];
        (rows, schema)
    }

    #[test]
    fn table_scan_emits_all_rows() {
        let (rows, schema) = data();
        let mut exec = TableScanExecutor::new(rows, schema.clone());
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx).unwrap();
        let total: usize = blocks.iter().map(|b| b.num_rows).sum();
        assert_eq!(total, 3);
        assert_eq!(blocks[0].columns[0].get(2).unwrap(), Scalar::Int(3));
        assert_eq!(blocks[0].columns[1].null_count(), 1);
    }

    #[test]
    fn column_pruning_reduces_output_columns() {
        let (rows, schema) = data();
        let exec = TableScanExecutor::new(rows, schema)
            .with_projection(vec![0])
            .unwrap();
        assert_eq!(exec.output_schema().len(), 1);
        let mut ctx = ExecCtx::default();
        let mut e = exec;
        let blocks = collect(&mut e, &mut ctx).unwrap();
        assert_eq!(blocks[0].col_count(), 1);
    }

    #[test]
    fn invalid_pruning_index_is_rejected() {
        let (rows, schema) = data();
        assert!(TableScanExecutor::new(rows, schema)
            .with_projection(vec![9])
            .is_err());
    }

    #[test]
    fn table_dual_emits_one_empty_row() {
        let mut exec = TableDualExecutor::default();
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut exec, &mut ctx).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].num_rows, 1);
        assert_eq!(blocks[0].col_count(), 0);
    }

    #[test]
    fn cte_materializes_once_and_replays() {
        let (rows, schema) = data();
        let mut scan = TableScanExecutor::new(rows, schema.clone());
        let mut ctx = ExecCtx::default();
        let mut cte = CteScanExecutor::materialize(&mut scan, schema.clone(), &mut ctx).unwrap();
        let first = collect(&mut cte, &mut ctx).unwrap();
        let second = collect(&mut cte, &mut ctx).unwrap();
        assert_eq!(first[0].num_rows, 3);
        assert_eq!(second[0].num_rows, 3);
        assert_eq!(
            first[0].columns[0].get(0).unwrap(),
            second[0].columns[0].get(0).unwrap()
        );
    }

    #[test]
    fn empty_scan_returns_no_chunks() {
        let mut exec =
            TableScanExecutor::new(vec![], RowSchema::new(vec![("a".into(), DataType::Int64)]));
        let mut ctx = ExecCtx::default();
        assert!(collect(&mut exec, &mut ctx).unwrap().is_empty());
    }
}
