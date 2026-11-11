//! `types` crate (L1 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §2.1、docs/11 「types: DataType, Block, FuncRegistry, ScalarEvaluator」。

use lattice_common::{BlockBase, Buffer, Validity};
use serde::{Deserialize, Serialize};

/// SQL 数据类型（docs/02 §2.1 全量枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Null,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Decimal { precision: u8, scale: u8 },
    Date,
    Time,
    DateTime,
    Timestamp,
    Duration,
    Year,
    Char(u32),
    VarChar(u32),
    Binary(u32),
    VarBinary(u32),
    Json,
    Enum,
    Set,
    Bit,
    Vector(u32),
}

impl DataType {
    /// 是否为数值类型（Binder 隐式转换的基础，L1 实现）。
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            DataType::Int8
                | DataType::Int16
                | DataType::Int32
                | DataType::Int64
                | DataType::UInt8
                | DataType::UInt16
                | DataType::UInt32
                | DataType::UInt64
                | DataType::Float32
                | DataType::Float64
                | DataType::Decimal { .. }
        )
    }
}

/// 三值逻辑（docs/02 §2.1：Predicate 求值为 Bool3，Filter 仅保留 True）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bool3 {
    True,
    False,
    Unknown,
}

impl Bool3 {
    /// SQL AND 真值表。
    pub fn and(self, rhs: Bool3) -> Bool3 {
        use Bool3::*;
        match (self, rhs) {
            (False, _) | (_, False) => False,
            (True, True) => True,
            _ => Unknown,
        }
    }

    /// SQL OR 真值表。
    pub fn or(self, rhs: Bool3) -> Bool3 {
        use Bool3::*;
        match (self, rhs) {
            (True, _) | (_, True) => True,
            (False, False) => False,
            _ => Unknown,
        }
    }
}

/// 实现标准 `Not` trait（clippy should_implement_trait）。
impl std::ops::Not for Bool3 {
    type Output = Bool3;

    fn not(self) -> Bool3 {
        match self {
            Bool3::True => Bool3::False,
            Bool3::False => Bool3::True,
            Bool3::Unknown => Bool3::Unknown,
        }
    }
}

/// 标量值（docs/02 §2.1 `Scalar`，L0 用简化表示，L1 补全 Decimal/Date/Time/Json）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Scalar {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Bytes(Vec<u8>),
    Str(String),
}

/// 列：类型 + 有效性位图 + 值缓冲区（docs/02 §2.1 `Column`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub data_type: DataType,
    pub validity: Validity,
    pub buffer: Buffer,
}

impl Column {
    pub fn new(data_type: DataType, rows: usize) -> Self {
        Self {
            data_type,
            validity: Validity::new(rows, true),
            buffer: Buffer::new(),
        }
    }
}

/// 列式批（对齐 Arrow 语义，docs/02 §2.1 `Block`）。
pub type Block = BlockBase<Column>;

/// 批构建器（复用 `common` 的形状一致性校验）。
pub type BlockBuilder = lattice_common::BlockBuilder<Column>;

/// 选择向量重导出，供上层 Filter/延迟物化使用。
pub use lattice_common::SelectionVector as Sel;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Not;

    #[test]
    fn three_valued_logic_matches_sql_truth_tables() {
        assert_eq!(Bool3::True.and(Bool3::Unknown), Bool3::Unknown);
        assert_eq!(Bool3::False.and(Bool3::Unknown), Bool3::False);
        assert_eq!(Bool3::True.or(Bool3::Unknown), Bool3::True);
        assert_eq!(Bool3::False.or(Bool3::Unknown), Bool3::Unknown);
        assert_eq!(Bool3::Unknown.not(), Bool3::Unknown);
    }

    #[test]
    fn data_type_numeric_classification() {
        assert!(DataType::Int64.is_numeric());
        assert!(DataType::Float32.is_numeric());
        assert!(!DataType::VarChar(255).is_numeric());
    }

    #[test]
    fn block_builder_keeps_columns_and_selection() {
        let block = BlockBuilder::new(8)
            .column(Column::new(DataType::Int64, 3))
            .rows(3)
            .selection(lattice_common::SelectionVector(vec![0, 2]))
            .build()
            .unwrap();
        assert_eq!(block.col_count(), 1);
        assert_eq!(block.logical_rows(), 2);
    }
}
