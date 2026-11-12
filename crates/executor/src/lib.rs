//! `executor` crate (L3 物理算子全集)。
//! 契约来源：docs/02 §3 `Executor` / `ExecutorBuilder`、§11；docs/03 L3；docs/04 §4.2。
//!
//! 行式火山模型：`Executor::next_chunk` 拉取列式 `Block`，算子内部按行处理，
//! 输出仍为 `Block`（对齐 Arrow 语义）。分周实现：
//! W10 扫描/投影/过滤/Dual/Limit；W11 Join；W12 聚合/集合；W13 排序/窗口；
//! W14 DML；W15 差分测试与统计埋点。

pub mod agg;
pub mod context;
pub mod dml;
pub mod join;
pub mod row;
pub mod scan;
pub mod sort;
pub mod unary;
pub mod verify;

pub use agg::{
    check_set_schema, hash_agg, scalar_agg, stream_agg, AggCall, AggFunc, AggSpec,
    AggregateExecutor, DistinctExecutor, SetOp, SetOpExecutor,
};
pub use context::{CancelToken, ExecCtx, ExecStats, MemoryTracker};
pub use dml::{
    predicate_true, AnalyzeExecutor, DeleteExecutor, ExpandExecutor, InsertExecutor, MemTable,
    SelectLockExecutor, TableStats, UpdateExecutor,
};
pub use join::{
    hash_join, index_join, merge_join, nested_loop_join, null_aware_anti_join, JoinExecutor,
    JoinKey, JoinSpec, MatchStrategy, PhysicalJoinType, CARTESIAN_MAX_ROWS,
};
pub use row::{block_take, block_to_rows, empty_block, rows_to_block, Row, RowSchema};
pub use scan::{collect, CteScanExecutor, TableDualExecutor, TableScanExecutor};
pub use sort::{
    Frame, MaterializeExecutor, SortExecutor, SortKey, TopNExecutor, WindowExecutor, WindowFunc,
    WindowSpec,
};
pub use unary::{
    block_of, collect_rows, rows_executor, FilterExecutor, LimitExecutor, ProjectionExecutor,
};
pub use verify::{
    assert_equivalent, baseline, canonical, explain_analyze, result_rows, run_boundary_suite,
    run_differential_suite, Fixture, OperatorBaseline, OperatorStats,
};

use lattice_common::Result;
use lattice_types::Block;

/// 物理属性占位（docs/02 §2.3 `Property`；L8 填充 ordering/distribution）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Property;

/// 执行器 trait：火山 + 向量化 Chunk（docs/02 §3）。
pub trait Executor: std::fmt::Debug {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn next_chunk(&mut self) -> Result<Option<Block>>;
    fn close(&mut self) -> Result<ExecStats>;
    fn required_property(&self) -> Property;
}

/// 执行器构建器 trait（docs/02 §3）。
pub trait ExecutorBuilder {
    fn build(&self) -> Result<Box<dyn Executor>>;
}

/// 空结果执行器（L0 占位实现，保留向后兼容）。
#[derive(Debug, Default)]
pub struct EmptyExecutor {
    stats: ExecStats,
}

impl Executor for EmptyExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        Ok(None)
    }

    fn close(&mut self) -> Result<ExecStats> {
        Ok(self.stats)
    }

    fn required_property(&self) -> Property {
        Property
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_executor_returns_no_chunks() {
        let mut exec = EmptyExecutor::default();
        let mut ctx = ExecCtx::default();
        exec.open(&mut ctx).unwrap();
        assert!(exec.next_chunk().unwrap().is_none());
        assert_eq!(exec.close().unwrap().actual_rows, 0);
    }

    #[test]
    fn table_dual_emits_one_row() {
        let mut exec = TableDualExecutor::default();
        let mut ctx = ExecCtx::default();
        exec.open(&mut ctx).unwrap();
        let block = exec.next_chunk().unwrap().unwrap();
        assert_eq!(block.num_rows, 1);
        assert!(exec.next_chunk().unwrap().is_none());
        assert_eq!(exec.close().unwrap().actual_rows, 1);
    }

    #[test]
    fn w10_pipeline_scan_filter_projection_limit() {
        let schema = RowSchema::new(vec![
            ("a".into(), lattice_types::DataType::Int64),
            ("b".into(), lattice_types::DataType::Int64),
        ]);
        let rows: Vec<Row> = (1..=6)
            .map(|i| {
                vec![
                    lattice_types::Scalar::Int(i),
                    lattice_types::Scalar::Int(i * 10),
                ]
            })
            .collect();
        let scan = TableScanExecutor::new(rows, schema.clone());
        let pred = lattice_expr::Expr::binary(
            lattice_expr::FuncSig::Gt,
            lattice_expr::Expr::column(0, 1),
            lattice_expr::Expr::Literal(
                lattice_types::Scalar::Int(2),
                lattice_types::DataType::Int64,
            ),
            lattice_types::DataType::Bool,
        );
        let filter = FilterExecutor::new(Box::new(scan), pred);
        let proj = ProjectionExecutor::new(
            Box::new(filter),
            vec![lattice_expr::Expr::column(1, 2)],
            vec!["b".into()],
        )
        .unwrap();
        let mut limit = LimitExecutor::new(Box::new(proj), 0, 2);
        let mut ctx = ExecCtx::default();
        let blocks = collect(&mut limit, &mut ctx).unwrap();
        let total: usize = blocks.iter().map(|b| b.num_rows).sum();
        assert_eq!(total, 2);
        assert_eq!(
            blocks[0].columns[0].get(0).unwrap(),
            lattice_types::Scalar::Int(30)
        );
    }
}
