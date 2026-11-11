//! `EXPLAIN` 逻辑计划输出骨架（docs/03 L2 DoD、AC-PLAN-002）。
//! L2 先埋观测点，L9 补齐 EXPLAIN ANALYZE / 代价分解。

use crate::logical::LogicalPlan;
use crate::plan_node::LogicalPlanNode;
use std::fmt::Write;

/// EXPLAIN 输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplainFormat {
    /// 缩进树形（默认）。
    Tree,
    /// 单行 JSON（供 CI plan diff / Replayer）。
    Json,
}

/// 生成可读的逻辑计划输出。
pub fn explain(node: &LogicalPlanNode, format: ExplainFormat) -> crate::Result<String> {
    match format {
        ExplainFormat::Json => node.to_json(),
        ExplainFormat::Tree => {
            let mut out = String::new();
            out.push_str("LogicalPlan\n");
            write_tree(node, 1, &mut out)?;
            Ok(out)
        }
    }
}

fn write_tree(node: &LogicalPlanNode, depth: usize, out: &mut String) -> crate::Result<()> {
    let indent = "  ".repeat(depth);
    let detail = describe(&node.plan);
    writeln!(
        out,
        "{indent}{} [id={}] {}",
        node.plan.op_id(),
        node.id.0,
        detail
    )
    .map_err(|e| lattice_common::internal(e.to_string()))?;
    if !node.schema.columns.is_empty() {
        let cols: Vec<String> = node
            .schema
            .columns
            .iter()
            .map(|c| format!("{}:{:?}", c.name, c.data_type))
            .collect();
        writeln!(out, "{indent}  schema = [{}]", cols.join(", "))
            .map_err(|e| lattice_common::internal(e.to_string()))?;
    }
    if let Some(stats) = node.stats {
        writeln!(out, "{indent}  est_rows = {:.1}", stats.row_count)
            .map_err(|e| lattice_common::internal(e.to_string()))?;
    }
    for hit in &node.rule_trace {
        writeln!(
            out,
            "{indent}  rule_trace = {} ({}→{})",
            hit.rule, hit.before.0, hit.after.0
        )
        .map_err(|e| lattice_common::internal(e.to_string()))?;
    }
    for child in &node.children {
        write_tree(child, depth + 1, out)?;
    }
    Ok(())
}

fn describe(plan: &LogicalPlan) -> String {
    match plan {
        LogicalPlan::DataSource(ds) => format!("table={} cols={}", ds.table, ds.columns.len()),
        LogicalPlan::Projection(p) => format!("exprs={}", p.exprs.len()),
        LogicalPlan::Selection(_) => "predicate".into(),
        LogicalPlan::TableDual => "rows=1".into(),
        LogicalPlan::Join(j) => format!("type={:?} cond={}", j.join_type, j.condition.is_some()),
        LogicalPlan::Apply(_) => "correlated".into(),
        LogicalPlan::Lateral => String::new(),
        LogicalPlan::MaxOneRow => "guard".into(),
        LogicalPlan::Aggregation(a) => {
            format!("groups={} aggs={}", a.group_by.len(), a.agg_calls.len())
        }
        LogicalPlan::Expand(e) => format!("sets={}", e.grouping_sets.len()),
        LogicalPlan::Distinct => String::new(),
        LogicalPlan::Window(w) => format!("funcs={}", w.functions.len()),
        LogicalPlan::Sort(s) => format!("keys={}", s.items.len()),
        LogicalPlan::TopN(t) => format!("limit={} offset={}", t.limit, t.offset),
        LogicalPlan::Limit(l) => format!("limit={} offset={}", l.limit, l.offset),
        LogicalPlan::SetOp(s) => format!("kind={:?}", s.kind),
        LogicalPlan::Cte(c) => format!("name={} materialized={}", c.name, c.materialized),
        LogicalPlan::PartitionUnion => String::new(),
        LogicalPlan::Sequence => String::new(),
        LogicalPlan::Lock(l) => format!("exclusive={}", l.exclusive),
        LogicalPlan::Dml(d) => format!("kind={:?} target={}", d.kind, d.target),
        LogicalPlan::Admin(a) => format!("kind={:?} target={}", a.kind, a.target),
        LogicalPlan::MemoGroup(g) => format!("group={}", g.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logical::{limit, projection, selection};
    use crate::plan_node::{LogicalPlanNode, PlanNodeId};
    use lattice_expr::{Expr, FuncSig};
    use lattice_types::{DataType, Scalar};
    use std::sync::Arc;

    fn sample() -> LogicalPlanNode {
        let scan = Arc::new(
            LogicalPlanNode::data_source(PlanNodeId(1), "test.t", &[("a", DataType::Int64)])
                .unwrap(),
        );
        let sel = Arc::new(
            LogicalPlanNode::new(
                PlanNodeId(2),
                selection(Expr::binary(
                    FuncSig::Gt,
                    Expr::column(0, 1),
                    Expr::Literal(Scalar::Int(1), DataType::Int64),
                    DataType::Bool,
                )),
                vec![scan],
            )
            .unwrap(),
        );
        LogicalPlanNode::new(
            PlanNodeId(3),
            projection(vec![Expr::column(0, 1)], vec![Some("a".into())]),
            vec![sel],
        )
        .unwrap()
    }

    #[test]
    fn tree_explain_is_readable() {
        let text = explain(&sample(), ExplainFormat::Tree).unwrap();
        assert!(text.contains("OP-L-PROJECTION"));
        assert!(text.contains("OP-L-SELECTION"));
        assert!(text.contains("OP-L-DATASOURCE"));
        assert!(text.contains("schema ="));
        assert!(text.contains("table=test.t"));
        // 缩进层级
        assert!(text.contains("  OP-L-PROJECTION"));
    }

    #[test]
    fn json_explain_roundtrips() {
        let s = sample();
        let text = explain(&s, ExplainFormat::Json).unwrap();
        let back = LogicalPlanNode::from_json(&text).unwrap();
        assert!(s.equivalent_to(&back));
    }

    #[test]
    fn limit_appears_in_explain() {
        let scan = Arc::new(LogicalPlanNode::table_dual(PlanNodeId(1)));
        let lim = LogicalPlanNode::new(PlanNodeId(2), limit(5, 1), vec![scan]).unwrap();
        let text = explain(&lim, ExplainFormat::Tree).unwrap();
        assert!(text.contains("limit=5 offset=1"));
    }
}
