//! `executor` crate (L3/L10/L12 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §3 `Executor` / `ExecutorBuilder`；docs/02 §11。

use lattice_common::Result;
use lattice_types::Block;

/// 执行期统计（docs/02 §2.3：actual_rows / time / memory / spill）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ExecStats {
    pub actual_rows: u64,
    pub time_ms: u64,
    pub memory_bytes: u64,
    pub spill_bytes: u64,
}

/// 物理属性占位（docs/02 §2.3 `Property`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Property;

/// 执行上下文占位（L3 填充 MemoryTracker / CancellationToken）。
#[derive(Debug, Clone, Default)]
pub struct ExecCtx;

/// 执行器 trait：火山 + 向量化 Chunk（docs/02 §3）。
pub trait Executor {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn next_chunk(&mut self) -> Result<Option<Block>>;
    fn close(&mut self) -> Result<ExecStats>;
    fn required_property(&self) -> Property;
}

/// 执行器构建器 trait（docs/02 §3）。
pub trait ExecutorBuilder {
    fn build(&self) -> Result<Box<dyn Executor>>;
}

/// 空结果执行器（L0 占位实现，用于贯通编译与单测）。
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

/// TableDual 执行器：产生一行空列（docs/04 OP-P-TABLEDUAL 的 L0 占位）。
#[derive(Debug, Default)]
pub struct TableDualExecutor {
    produced: bool,
    stats: ExecStats,
}

impl Executor for TableDualExecutor {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }

    fn next_chunk(&mut self) -> Result<Option<Block>> {
        if self.produced {
            return Ok(None);
        }
        self.produced = true;
        self.stats.actual_rows = 1;
        Ok(Some(lattice_types::BlockBuilder::new(1).rows(1).build()?))
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
        let mut ctx = ExecCtx;
        exec.open(&mut ctx).unwrap();
        assert!(exec.next_chunk().unwrap().is_none());
        assert_eq!(exec.close().unwrap().actual_rows, 0);
    }

    #[test]
    fn table_dual_emits_one_row() {
        let mut exec = TableDualExecutor::default();
        let mut ctx = ExecCtx;
        exec.open(&mut ctx).unwrap();
        let block = exec.next_chunk().unwrap().unwrap();
        assert_eq!(block.num_rows, 1);
        assert!(exec.next_chunk().unwrap().is_none());
        assert_eq!(exec.close().unwrap().actual_rows, 1);
    }
}
