//! `expr` crate (L1 表达式系统)。
//! 契约来源：docs/02 §2.2/§3、docs/04 F-TYP-006/009/016。
//!
//! 模块：`ast`（Expr AST 与等价比较器）、`registry`（FuncRegistry 元数据）、
//! `eval`（向量化 ScalarEvaluator，下推与上层共用同一实现）。

pub mod ast;
pub mod eval;
pub mod registry;

pub use ast::{AggKind, ColumnRef, Expr, Frame, FuncSig, QueryRef, SortItem, WindowKind};
pub use eval::{eval_all, filter_keeps, DefaultScalarEvaluator, ScalarEvaluator};
pub use registry::{FuncMeta, FuncRegistry};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l1_expression_system_is_wired() {
        let reg = FuncRegistry::with_builtins();
        assert!(reg.lookup("+").unwrap().pushdownable);
        let expr = Expr::binary(
            FuncSig::Add,
            Expr::column(0, 1),
            Expr::literal(
                lattice_types::Scalar::Int(1),
                lattice_types::DataType::Int64,
            ),
            lattice_types::DataType::Int64,
        );
        assert_eq!(expr.data_type(), lattice_types::DataType::Int64);
        assert!(expr.structurally_eq(&expr.clone()));
    }
}
