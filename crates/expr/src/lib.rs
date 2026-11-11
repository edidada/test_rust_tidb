//! `expr` crate (L1 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §2.2、§3（ScalarEvaluator / FuncRegistry）。

use lattice_common::{BlockBase, SelectionVector};
use lattice_types::{Bool3, Column, DataType};

/// 表达式 AST（docs/02 §2.2 的 L0 简化版；L1 补全 Cast/Case/Subquery/Agg/Window/Param）。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Column { index: usize, data_type: DataType },
    Literal(lattice_types::Scalar, DataType),
}

/// 函数签名元数据（docs/02 §2.2：deterministic / 可下推 / SIMD / 代价）。
#[derive(Debug, Clone, PartialEq)]
pub struct FuncMeta {
    pub name: &'static str,
    pub deterministic: bool,
    pub pushdownable: bool,
    pub simd: bool,
}

/// 标量求值器 trait（docs/02 §2.2）。
pub trait ScalarEvaluator {
    fn eval(
        &self,
        expr: &Expr,
        block: &BlockBase<Column>,
        sel: &SelectionVector,
    ) -> lattice_common::Result<Column>;
}

/// 函数注册表（L0 仅提供注册/查询骨架，L1 填充内置函数）。
#[derive(Debug, Default)]
pub struct FuncRegistry {
    funcs: Vec<FuncMeta>,
}

impl FuncRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, meta: FuncMeta) {
        self.funcs.push(meta);
    }

    pub fn lookup(&self, name: &str) -> Option<&FuncMeta> {
        self.funcs.iter().find(|f| f.name == name)
    }

    pub fn pushdownable(&self, name: &str) -> bool {
        self.lookup(name).map(|f| f.pushdownable).unwrap_or(false)
    }
}

/// 三值逻辑谓词求值的统一入口（docs/02 §2.1：Filter 仅保留 True）。
pub fn filter_keeps(value: Bool3) -> bool {
    matches!(value, Bool3::True)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tracks_pushdown_metadata() {
        let mut reg = FuncRegistry::new();
        reg.register(FuncMeta {
            name: "plus",
            deterministic: true,
            pushdownable: true,
            simd: false,
        });
        assert!(reg.pushdownable("plus"));
        assert!(!reg.pushdownable("missing"));
    }

    #[test]
    fn filter_keeps_only_true() {
        assert!(filter_keeps(Bool3::True));
        assert!(!filter_keeps(Bool3::False));
        assert!(!filter_keeps(Bool3::Unknown));
    }
}
