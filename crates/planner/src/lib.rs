//! `planner` crate (L2 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §2.3、§3；docs/02 §11：`LogicalPlanNode, Schema, Rule`。

use lattice_common::Result;
use lattice_types::DataType;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 计划节点 ID（docs/02 §2.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanNodeId(pub u64);

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

/// 逻辑算子枚举（04 §4.1 的 L0 最小骨架；L2 补全全部 `OP-L-*`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogicalPlan {
    TableDual,
    DataSource { table: String },
}

/// 逻辑计划节点（docs/02 §2.3）。计划构建后不可变（`Arc`），状态在算子实例内。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogicalPlanNode {
    pub id: PlanNodeId,
    pub plan: LogicalPlan,
    pub children: Vec<Arc<LogicalPlanNode>>,
    pub schema: Schema,
    pub rule_trace: Vec<RuleHit>,
}

impl LogicalPlanNode {
    pub fn table_dual(id: PlanNodeId) -> Self {
        Self {
            id,
            plan: LogicalPlan::TableDual,
            children: Vec::new(),
            schema: Schema::default(),
            rule_trace: Vec::new(),
        }
    }

    /// 计划可序列化（docs/02 §14 检查清单）。
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| {
            lattice_common::Error::new(lattice_common::ErrorKind::Internal, e.to_string())
        })
    }
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
    fn logical_plan_serializes_to_json() {
        let node = LogicalPlanNode::table_dual(PlanNodeId(1));
        let json = node.to_json().unwrap();
        assert!(json.contains("TableDual"));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn rule_trace_is_part_of_plan() {
        let mut node = LogicalPlanNode::table_dual(PlanNodeId(7));
        node.rule_trace.push(RuleHit {
            rule: "R-PPD".into(),
            before: PlanNodeId(1),
            after: PlanNodeId(7),
        });
        assert_eq!(node.rule_trace[0].rule, "R-PPD");
    }
}
