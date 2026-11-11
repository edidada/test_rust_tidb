//! `planner` crate (L2 逻辑计划与算子框架)。
//! 契约来源：docs/02 §2.3/§3、docs/04 §4.1 OP-L-*、docs/03 L2。
//!
//! 模块：`logical`（28 个逻辑算子）、`plan_node`（Schema 推导/遍历/等价比较）、
//! `explain`（EXPLAIN 逻辑计划输出）。计划构建后不可变（`Arc`）。

pub mod explain;
pub mod logical;
pub mod plan_node;

pub use explain::{explain, ExplainFormat};
pub use logical::{
    data_source, inner_join, limit, literal_expr, projection, selection, Admin, AdminKind,
    Aggregation, Apply, Arity, Cte, DataSource, Dml, DmlKind, Expand, Join, JoinType, Limit,
    LogicalPlan, MemoGroup, Projection, SelectLock, Selection, SetOp, SetOpKind, Sort, TopN,
    Window,
};
pub use plan_node::{LogicalPlanNode, PlanNodeId, Statistics};

use lattice_common::Result;
use lattice_types::DataType;
use serde::{Deserialize, Serialize};

/// 输出列 Schema（列裁剪后收缩，docs/02 §2.3）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<ColumnMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}

/// 规则命中记录（ADR-6 / docs/02 §2.3 `rule_trace`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleHit {
    pub rule: String,
    pub before: PlanNodeId,
    pub after: PlanNodeId,
}

/// 规则 pattern（docs/02 §3 `Rule::pattern`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub op: String,
}

/// 优化上下文占位（L6 填充）。
#[derive(Debug, Clone, Default)]
pub struct OptCtx;

/// 规则 trait（docs/02 §3）。
pub trait Rule {
    fn name(&self) -> &str;
    fn pattern(&self) -> Pattern;
    fn apply(&self, plan: &LogicalPlanNode, ctx: &OptCtx) -> Result<Option<LogicalPlanNode>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l0_contract_still_compiles() {
        let node = LogicalPlanNode::table_dual(PlanNodeId(1));
        let json = node.to_json().unwrap();
        assert!(json.contains("TableDual"));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn rule_trace_is_part_of_plan() {
        let mut node = LogicalPlanNode::table_dual(PlanNodeId(7));
        node.record_rule_hit("R-PPD", PlanNodeId(1));
        assert_eq!(node.rule_trace[0].rule, "R-PPD");
        assert_eq!(node.rule_trace[0].after, PlanNodeId(7));
    }

    #[test]
    fn all_op_l_variants_have_ids() {
        assert!(LogicalPlan::TableDual.op_id().starts_with("OP-L-"));
        assert!(logical::inner_join(None).op_id().starts_with("OP-L-"));
    }
}
