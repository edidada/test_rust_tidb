//! 表达式 AST（docs/02 §2.2 全量形状、docs/04 F-TYP-016 等价比较器）。

use lattice_types::{Bool3, DataType, Scalar};
use serde::{Deserialize, Serialize};

/// 列引用：绑定后为唯一 id（Binder 解析）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnRef {
    /// 输入 Block 中的物理列下标。
    pub index: usize,
    /// 稳定唯一 id（Binder 分配，跨计划等价比较用）。
    pub id: u64,
}

/// 函数签名（docs/02 §2.2 `FuncSig`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FuncSig {
    // 算术
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Abs,
    // 比较（返回 Bool3）
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    IsNull,
    IsNotNull,
    // 逻辑（Bool3 三值）
    And,
    Or,
    Not,
    // 字符串
    Concat,
    Length,
    Upper,
    Lower,
    // 条件
    Coalesce,
    If,
}

impl FuncSig {
    pub fn name(self) -> &'static str {
        match self {
            FuncSig::Add => "+",
            FuncSig::Sub => "-",
            FuncSig::Mul => "*",
            FuncSig::Div => "/",
            FuncSig::Mod => "%",
            FuncSig::Neg => "neg",
            FuncSig::Abs => "abs",
            FuncSig::Eq => "=",
            FuncSig::Ne => "<>",
            FuncSig::Lt => "<",
            FuncSig::Le => "<=",
            FuncSig::Gt => ">",
            FuncSig::Ge => ">=",
            FuncSig::IsNull => "isnull",
            FuncSig::IsNotNull => "isnotnull",
            FuncSig::And => "and",
            FuncSig::Or => "or",
            FuncSig::Not => "not",
            FuncSig::Concat => "concat",
            FuncSig::Length => "length",
            FuncSig::Upper => "upper",
            FuncSig::Lower => "lower",
            FuncSig::Coalesce => "coalesce",
            FuncSig::If => "if",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            FuncSig::Eq
                | FuncSig::Ne
                | FuncSig::Lt
                | FuncSig::Le
                | FuncSig::Gt
                | FuncSig::Ge
                | FuncSig::IsNull
                | FuncSig::IsNotNull
        )
    }

    pub fn is_logical(self) -> bool {
        matches!(self, FuncSig::And | FuncSig::Or | FuncSig::Not)
    }
}

/// 聚合函数种类（docs/02 §2.2 `AggKind`；L3 实现求值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AggKind {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// 窗口函数种类（docs/02 §2.2 `WindowKind`；L3 实现求值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowKind {
    RowNumber,
    Rank,
    DenseRank,
    Sum,
    Avg,
}

/// 排序项（ORDER BY / 窗口排序，docs/02 §2.2 `SortItem`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortItem {
    pub expr: Box<Expr>,
    pub desc: bool,
    pub nulls_first: bool,
}

/// 窗口 frame（docs/02 §2.2 `Frame`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Frame {
    Rows { start: i64, end: i64 },
    Range { start: i64, end: i64 },
    Groups { start: i64, end: i64 },
}

/// 子查询引用（去关联后应被消除，docs/02 §2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QueryRef(pub u64);

/// 表达式 AST（docs/02 §2.2）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Column(ColumnRef),
    Literal(Scalar, DataType),
    Function {
        sig: FuncSig,
        args: Vec<Expr>,
        ret_type: DataType,
        nullable: bool,
    },
    Cast {
        expr: Box<Expr>,
        from: DataType,
        to: DataType,
        in_union: bool,
    },
    Case {
        when: Vec<(Expr, Expr)>,
        then_else: Option<Box<Expr>>,
    },
    Subquery(QueryRef),
    AggCall {
        func: AggKind,
        args: Vec<Expr>,
        distinct: bool,
        filter: Option<Box<Expr>>,
    },
    WindowFunc {
        func: WindowKind,
        args: Vec<Expr>,
        partition_by: Vec<Expr>,
        order_by: Vec<SortItem>,
        frame: Frame,
    },
    Param(u32),
    /// 三值逻辑字面量（Bool3），用于谓词结果。
    Bool3(Bool3),
}

impl Expr {
    pub fn column(index: usize, id: u64) -> Self {
        Expr::Column(ColumnRef { index, id })
    }

    pub fn literal(value: Scalar, data_type: DataType) -> Self {
        Expr::Literal(value, data_type)
    }

    pub fn binary(sig: FuncSig, lhs: Expr, rhs: Expr, ret_type: DataType) -> Self {
        Expr::Function {
            sig,
            args: vec![lhs, rhs],
            ret_type,
            nullable: true,
        }
    }

    pub fn unary(sig: FuncSig, arg: Expr, ret_type: DataType) -> Self {
        Expr::Function {
            sig,
            args: vec![arg],
            ret_type,
            nullable: true,
        }
    }

    /// 表达式结果类型（Binder 类型推导，docs/02 §2.2）。
    pub fn data_type(&self) -> DataType {
        match self {
            Expr::Literal(_, ty) => *ty,
            Expr::Function { ret_type, .. } => *ret_type,
            Expr::Cast { to, .. } => *to,
            Expr::Bool3(_) => DataType::Bool,
            Expr::Case { when, then_else } => when
                .first()
                .map(|(_, t)| t.data_type())
                .or_else(|| then_else.as_ref().map(|e| e.data_type()))
                .unwrap_or(DataType::Null),
            Expr::Param(_) => DataType::Null,
            Expr::AggCall { .. } => DataType::Int64,
            Expr::WindowFunc { .. } => DataType::Int64,
            Expr::Subquery(_) => DataType::Null,
            Expr::Column(_) => DataType::Null,
        }
    }

    /// 结构等价比较（docs/04 F-TYP-016：计划 diff 依赖）。
    /// 忽略 `nullable` 等派生属性，只比较语义结构。
    pub fn structurally_eq(&self, other: &Expr) -> bool {
        match (self, other) {
            (Expr::Column(a), Expr::Column(b)) => a.id == b.id,
            (Expr::Literal(a, ta), Expr::Literal(b, tb)) => a == b && ta == tb,
            (
                Expr::Function {
                    sig: sa, args: aa, ..
                },
                Expr::Function {
                    sig: sb, args: ab, ..
                },
            ) => sa == sb && exprs_eq(aa, ab),
            (
                Expr::Cast {
                    expr: ea, to: ta, ..
                },
                Expr::Cast {
                    expr: eb, to: tb, ..
                },
            ) => ta == tb && ea.structurally_eq(eb),
            (
                Expr::Case {
                    when: wa,
                    then_else: ea,
                },
                Expr::Case {
                    when: wb,
                    then_else: eb,
                },
            ) => {
                wa.len() == wb.len()
                    && wa.iter().zip(wb).all(|((ca, va), (cb, vb))| {
                        ca.structurally_eq(cb) && va.structurally_eq(vb)
                    })
                    && match (ea, eb) {
                        (Some(a), Some(b)) => a.structurally_eq(b),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (Expr::Subquery(a), Expr::Subquery(b)) => a == b,
            (Expr::Param(a), Expr::Param(b)) => a == b,
            (Expr::Bool3(a), Expr::Bool3(b)) => a == b,
            (
                Expr::AggCall {
                    func: fa,
                    args: aa,
                    distinct: da,
                    filter: la,
                },
                Expr::AggCall {
                    func: fb,
                    args: ab,
                    distinct: db,
                    filter: lb,
                },
            ) => {
                fa == fb
                    && da == db
                    && exprs_eq(aa, ab)
                    && match (la, lb) {
                        (Some(a), Some(b)) => a.structurally_eq(b),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

fn exprs_eq(a: &[Expr], b: &[Expr]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.structurally_eq(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_and_logical_classification() {
        assert!(FuncSig::Eq.is_comparison());
        assert!(FuncSig::And.is_logical());
        assert!(!FuncSig::Add.is_logical());
    }

    #[test]
    fn structural_equality_ignores_derived_flags() {
        let a = Expr::binary(
            FuncSig::Add,
            Expr::column(0, 1),
            Expr::literal(Scalar::Int(1), DataType::Int64),
            DataType::Int64,
        );
        let b = Expr::Function {
            sig: FuncSig::Add,
            args: vec![
                Expr::column(3, 1), // index 不同但 id 相同
                Expr::literal(Scalar::Int(1), DataType::Int64),
            ],
            ret_type: DataType::Int64,
            nullable: false, // 派生属性不同
        };
        assert!(a.structurally_eq(&b));
    }

    #[test]
    fn different_column_id_breaks_equality() {
        let a = Expr::column(0, 1);
        let b = Expr::column(0, 2);
        assert!(!a.structurally_eq(&b));
    }

    #[test]
    fn json_roundtrip_for_plan_serialization() {
        let e = Expr::binary(
            FuncSig::Eq,
            Expr::column(0, 7),
            Expr::literal(Scalar::Int(42), DataType::Int64),
            DataType::Bool,
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: Expr = serde_json::from_str(&json).unwrap();
        assert!(e.structurally_eq(&back));
    }
}
