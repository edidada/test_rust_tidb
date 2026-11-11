//! `types` crate (L1 类型系统基础)。
//! 契约来源：docs/02 §2.1、§11；docs/03 L1（类型系统、表达式、Parser、Binder）。
//!
//! 模块划分：`datatype`（类型族/精度）、`scalar`（字面量/隐式转换/溢出）、
//! `bool3`（三值逻辑）、`column`（列式批读写与延迟物化）。

pub mod bool3;
pub mod column;
pub mod datatype;
pub mod scalar;

pub use bool3::Bool3;
pub use column::{block_from_columns, Block, BlockBuilderAlias, Column};
pub use datatype::{DataType, TypeClass};
pub use scalar::Scalar;

/// 选择向量重导出，供上层 Filter/延迟物化使用。
pub use lattice_common::SelectionVector as Sel;

/// 兼容 L0 骨架的批构建器别名。
pub type BlockBuilder = crate::column::BlockBuilderAlias;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l0_contract_still_compiles() {
        // L0 骨架契约：LogicalPlanNode 等下游仍可用 TableDual 构造；
        // 此处验证 types 侧原有 API 未被破坏。
        let mut col = Column::empty(DataType::Int64);
        col.push(&Scalar::Int(1)).unwrap();
        let block = BlockBuilder::new(4).column(col).rows(1).build().unwrap();
        assert_eq!(block.logical_rows(), 1);
        assert_eq!(block.columns[0].get(0).unwrap(), Scalar::Int(1));
    }

    #[test]
    fn three_valued_logic_and_selection() {
        use std::ops::Not;
        assert_eq!(Bool3::True.and(Bool3::Unknown), Bool3::Unknown);
        assert_eq!(Bool3::Unknown.not(), Bool3::Unknown);
        let mut col = Column::empty(DataType::Int16);
        for v in [1, 2, 3] {
            col.push(&Scalar::Int(v)).unwrap();
        }
        let taken = col.take(&Sel(vec![0, 2])).unwrap();
        assert_eq!(taken.get(1).unwrap(), Scalar::Int(3));
    }
}
