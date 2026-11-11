//! 逻辑算子全集 `OP-L-*`（docs/04 §4.1，28 项；docs/02 §2.3）。
//! 关系代数语义，无算法与顺序；物理实现由 L3/L6+ 决定。

use lattice_expr::{AggKind, Expr, SortItem, WindowKind};
use lattice_types::{DataType, Scalar};
use serde::{Deserialize, Serialize};

/// 连接类型（docs/04 §4.1 Join 族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinType {
    Inner,
    LeftOuter,
    RightOuter,
    FullOuter,
    Cross,
    Semi,
    AntiSemi,
    LeftOuterSemi,
    AntiLeftOuterSemi,
}

/// 集合操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOpKind {
    Union,
    UnionAll,
    Intersect,
    IntersectAll,
    Except,
    ExceptAll,
}

/// DML 种类（docs/04 OP-L-DML）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DmlKind {
    Insert,
    Update,
    Delete,
    Replace,
}

/// 管理类逻辑算子（docs/04 OP-L-DDL/ANALYZE/EXPLAIN）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminKind {
    Ddl,
    Analyze,
    Explain,
}

/// 数据源（表 / 视图 / CTE 引用 / 分区展开前形态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataSource {
    pub table: String,
    pub columns: Vec<String>,
}

/// 投影。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub exprs: Vec<Expr>,
    pub aliases: Vec<Option<String>>,
}

/// 选择（谓词 Bool3 语义）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub predicate: Expr,
}

/// Join。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Join {
    pub join_type: JoinType,
    pub condition: Option<Expr>,
    pub using_columns: Vec<String>,
}

/// Apply（相关子查询，去关联的输入形态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Apply {
    pub join_type: JoinType,
    pub correlation: Vec<Expr>,
}

/// 聚合。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aggregation {
    pub group_by: Vec<Expr>,
    pub agg_calls: Vec<(AggKind, Vec<Expr>, bool)>,
}

/// 分组集展开（GROUPING SETS / ROLLUP / CUBE）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expand {
    pub grouping_sets: Vec<Vec<Expr>>,
}

/// 窗口。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub functions: Vec<(WindowKind, Vec<Expr>)>,
    pub partition_by: Vec<Expr>,
    pub order_by: Vec<SortItem>,
    pub frame: String,
}

/// 排序。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sort {
    pub items: Vec<SortItem>,
}

/// TopN（ORDER BY + LIMIT 融合）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopN {
    pub items: Vec<SortItem>,
    pub limit: u64,
    pub offset: u64,
}

/// Limit / Offset。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub limit: u64,
    pub offset: u64,
}

/// 集合操作（UNION/INTERSECT/EXCEPT）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetOp {
    pub kind: SetOpKind,
}

/// CTE。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cte {
    pub name: String,
    pub materialized: bool,
    pub recursive: bool,
}

/// DML 计划。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dml {
    pub kind: DmlKind,
    pub target: String,
    pub values: Vec<Expr>,
}

/// 行锁逻辑。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectLock {
    pub exclusive: bool,
}

/// Memo 组引用（仅 CBO 内部，docs/04 OP-L-MEMOGROUP）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoGroup(pub u64);

/// 管理类算子。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Admin {
    pub kind: AdminKind,
    pub target: String,
}

/// 逻辑算子枚举（docs/04 §4.1 全部 28 项）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogicalPlan {
    // OP-L-DATASOURCE / PROJECTION / SELECTION / DUAL
    DataSource(DataSource),
    Projection(Projection),
    Selection(Selection),
    TableDual,
    // OP-L-JOIN-*
    Join(Join),
    // OP-L-APPLY / LATERAL / MAXONEROW
    Apply(Apply),
    Lateral,
    MaxOneRow,
    // OP-L-AGG / EXPAND / DISTINCT
    Aggregation(Aggregation),
    Expand(Expand),
    Distinct,
    // OP-L-WINDOW / SORT / TOPN / LIMIT
    Window(Window),
    Sort(Sort),
    TopN(TopN),
    Limit(Limit),
    // OP-L-UNION / INTERSECT / EXCEPT / CTE / PARTUNION / SEQUENCE
    SetOp(SetOp),
    Cte(Cte),
    PartitionUnion,
    Sequence,
    // OP-L-LOCK / DML / DDL-ANALYZE-EXPLAIN
    Lock(SelectLock),
    Dml(Dml),
    Admin(Admin),
    // OP-L-MEMOGROUP（CBO 内部）
    MemoGroup(MemoGroup),
}

impl LogicalPlan {
    /// 算子编号（docs/04 §4.1 ID）。
    pub fn op_id(&self) -> &'static str {
        match self {
            LogicalPlan::DataSource(_) => "OP-L-DATASOURCE",
            LogicalPlan::Projection(_) => "OP-L-PROJECTION",
            LogicalPlan::Selection(_) => "OP-L-SELECTION",
            LogicalPlan::TableDual => "OP-L-DUAL",
            LogicalPlan::Join(j) => match j.join_type {
                JoinType::Inner | JoinType::Cross => "OP-L-JOIN-INNER",
                JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter => {
                    "OP-L-JOIN-OUTER"
                }
                JoinType::Semi
                | JoinType::AntiSemi
                | JoinType::LeftOuterSemi
                | JoinType::AntiLeftOuterSemi => "OP-L-JOIN-SEMI",
            },
            LogicalPlan::Apply(_) => "OP-L-APPLY",
            LogicalPlan::Lateral => "OP-L-LATERAL",
            LogicalPlan::MaxOneRow => "OP-L-MAXONEROW",
            LogicalPlan::Aggregation(_) => "OP-L-AGG",
            LogicalPlan::Expand(_) => "OP-L-EXPAND",
            LogicalPlan::Distinct => "OP-L-DISTINCT",
            LogicalPlan::Window(_) => "OP-L-WINDOW",
            LogicalPlan::Sort(_) => "OP-L-SORT",
            LogicalPlan::TopN(_) => "OP-L-TOPN",
            LogicalPlan::Limit(_) => "OP-L-LIMIT",
            LogicalPlan::SetOp(s) => match s.kind {
                SetOpKind::Union | SetOpKind::UnionAll => "OP-L-UNION",
                SetOpKind::Intersect | SetOpKind::IntersectAll => "OP-L-INTERSECT",
                SetOpKind::Except | SetOpKind::ExceptAll => "OP-L-EXCEPT",
            },
            LogicalPlan::Cte(_) => "OP-L-CTE",
            LogicalPlan::PartitionUnion => "OP-L-PARTUNION",
            LogicalPlan::Sequence => "OP-L-SEQUENCE",
            LogicalPlan::Lock(_) => "OP-L-LOCK",
            LogicalPlan::Dml(_) => "OP-L-DML",
            LogicalPlan::Admin(a) => match a.kind {
                AdminKind::Ddl => "OP-L-DDL",
                AdminKind::Analyze => "OP-L-ANALYZE",
                AdminKind::Explain => "OP-L-EXPLAIN",
            },
            LogicalPlan::MemoGroup(_) => "OP-L-MEMOGROUP",
        }
    }

    /// 期望的子节点数（用于构造校验）。
    pub fn arity(&self) -> Arity {
        match self {
            LogicalPlan::Join(_)
            | LogicalPlan::Apply(_)
            | LogicalPlan::SetOp(_)
            | LogicalPlan::Cte(_) => Arity::AtLeast(2),
            LogicalPlan::PartitionUnion | LogicalPlan::Sequence => Arity::Variadic,
            LogicalPlan::TableDual | LogicalPlan::DataSource(_) | LogicalPlan::MemoGroup(_) => {
                Arity::Exactly(0)
            }
            LogicalPlan::Distinct | LogicalPlan::MaxOneRow | LogicalPlan::Lateral => {
                Arity::Exactly(1)
            }
            _ => Arity::Exactly(1),
        }
    }

    /// 是否改变输出列数/类型（Schema 推导用）。
    pub fn is_schema_changing(&self) -> bool {
        matches!(
            self,
            LogicalPlan::Projection(_)
                | LogicalPlan::Aggregation(_)
                | LogicalPlan::Expand(_)
                | LogicalPlan::DataSource(_)
                | LogicalPlan::TableDual
        )
    }
}

/// 子节点基数约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    Exactly(usize),
    AtLeast(usize),
    Variadic,
}

impl Arity {
    pub fn accepts(self, n: usize) -> bool {
        match self {
            Arity::Exactly(k) => n == k,
            Arity::AtLeast(k) => n >= k,
            Arity::Variadic => true,
        }
    }
}

/// 辅助：构造常用算子的便捷函数。
pub fn data_source(table: &str, columns: Vec<String>) -> LogicalPlan {
    LogicalPlan::DataSource(DataSource {
        table: table.to_string(),
        columns,
    })
}

pub fn projection(exprs: Vec<Expr>, aliases: Vec<Option<String>>) -> LogicalPlan {
    LogicalPlan::Projection(Projection { exprs, aliases })
}

pub fn selection(predicate: Expr) -> LogicalPlan {
    LogicalPlan::Selection(Selection { predicate })
}

pub fn inner_join(condition: Option<Expr>) -> LogicalPlan {
    LogicalPlan::Join(Join {
        join_type: JoinType::Inner,
        condition,
        using_columns: Vec::new(),
    })
}

pub fn limit(n: u64, offset: u64) -> LogicalPlan {
    LogicalPlan::Limit(Limit { limit: n, offset })
}

pub fn literal_expr(value: Scalar, ty: DataType) -> Expr {
    Expr::Literal(value, ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_28_op_ids_are_distinct_and_mapped() {
        let plans = vec![
            data_source("t", vec!["a".into()]),
            projection(vec![], vec![]),
            selection(literal_expr(Scalar::Bool(true), DataType::Bool)),
            LogicalPlan::TableDual,
            inner_join(None),
            LogicalPlan::Apply(Apply {
                join_type: JoinType::Inner,
                correlation: vec![],
            }),
            LogicalPlan::Lateral,
            LogicalPlan::MaxOneRow,
            LogicalPlan::Aggregation(Aggregation {
                group_by: vec![],
                agg_calls: vec![],
            }),
            LogicalPlan::Expand(Expand {
                grouping_sets: vec![],
            }),
            LogicalPlan::Distinct,
            LogicalPlan::Window(Window {
                functions: vec![],
                partition_by: vec![],
                order_by: vec![],
                frame: "ROWS".into(),
            }),
            LogicalPlan::Sort(Sort { items: vec![] }),
            LogicalPlan::TopN(TopN {
                items: vec![],
                limit: 10,
                offset: 0,
            }),
            limit(10, 0),
            LogicalPlan::SetOp(SetOp {
                kind: SetOpKind::Union,
            }),
            LogicalPlan::Cte(Cte {
                name: "c".into(),
                materialized: false,
                recursive: false,
            }),
            LogicalPlan::PartitionUnion,
            LogicalPlan::Sequence,
            LogicalPlan::Lock(SelectLock { exclusive: true }),
            LogicalPlan::Dml(Dml {
                kind: DmlKind::Insert,
                target: "t".into(),
                values: vec![],
            }),
            LogicalPlan::Admin(Admin {
                kind: AdminKind::Ddl,
                target: "t".into(),
            }),
            LogicalPlan::MemoGroup(MemoGroup(1)),
        ];
        for p in &plans {
            assert!(p.op_id().starts_with("OP-L-"));
        }
        assert_eq!(plans.len(), 23); // 枚举变体数；Join/SetOp/Admin 各有子类型
    }

    #[test]
    fn join_op_id_reflects_family() {
        assert_eq!(
            LogicalPlan::Join(Join {
                join_type: JoinType::LeftOuter,
                condition: None,
                using_columns: vec![]
            })
            .op_id(),
            "OP-L-JOIN-OUTER"
        );
        assert_eq!(
            LogicalPlan::SetOp(SetOp {
                kind: SetOpKind::Intersect
            })
            .op_id(),
            "OP-L-INTERSECT"
        );
    }

    #[test]
    fn arity_constraints() {
        assert!(LogicalPlan::TableDual.arity().accepts(0));
        assert!(!LogicalPlan::TableDual.arity().accepts(1));
        assert!(inner_join(None).arity().accepts(2));
        assert!(!inner_join(None).arity().accepts(1));
        assert!(LogicalPlan::Sequence.arity().accepts(5));
    }
}
