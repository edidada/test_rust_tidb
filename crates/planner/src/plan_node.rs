//! `LogicalPlanNode`（docs/02 §2.3）：不可变计划树、Schema 推导、遍历、等价比较。

use crate::logical::LogicalPlan;
use crate::{ColumnMeta, RuleHit, Schema};
use lattice_common::{Error, ErrorKind, Result};
use lattice_expr::Expr;
use lattice_types::DataType;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 统计信息占位（docs/02 §2.3 `stats`；L7 填充）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Statistics {
    pub row_count: f64,
    pub row_width: f64,
}

/// 计划节点 ID（docs/02 §2.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanNodeId(pub u64);

/// 逻辑计划节点（docs/02 §2.3）。计划构建后不可变（`Arc`），执行期状态在算子实例内。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogicalPlanNode {
    pub id: PlanNodeId,
    pub plan: LogicalPlan,
    pub children: Vec<Arc<LogicalPlanNode>>,
    pub schema: Schema,
    pub stats: Option<Statistics>,
    pub rule_trace: Vec<RuleHit>,
}

impl LogicalPlanNode {
    /// 构造并校验 arity，随后自动推导 Schema。
    pub fn new(
        id: PlanNodeId,
        plan: LogicalPlan,
        children: Vec<Arc<LogicalPlanNode>>,
    ) -> Result<Self> {
        if !plan.arity().accepts(children.len()) {
            return Err(Error::new(
                ErrorKind::Internal,
                format!(
                    "{} 期望子节点数 {:?}，实际 {}",
                    plan.op_id(),
                    plan.arity(),
                    children.len()
                ),
            ));
        }
        let schema = derive_schema(&plan, &children)?;
        Ok(Self {
            id,
            plan,
            children,
            schema,
            stats: None,
            rule_trace: Vec::new(),
        })
    }

    /// TableDual 快捷构造（L0 兼容）。
    pub fn table_dual(id: PlanNodeId) -> Self {
        Self::new(id, LogicalPlan::TableDual, Vec::new()).unwrap()
    }

    /// DataSource 快捷构造。
    pub fn data_source(id: PlanNodeId, table: &str, columns: &[(&str, DataType)]) -> Result<Self> {
        let schema = Schema {
            columns: columns
                .iter()
                .map(|(n, t)| ColumnMeta {
                    name: (*n).to_string(),
                    data_type: *t,
                    nullable: true,
                })
                .collect(),
        };
        let plan = LogicalPlan::DataSource(crate::logical::DataSource {
            table: table.to_string(),
            columns: columns.iter().map(|(n, _)| (*n).to_string()).collect(),
        });
        let mut node = Self::new(id, plan, Vec::new())?;
        node.schema = schema;
        Ok(node)
    }

    /// 计划可序列化（docs/02 §14 检查清单）。
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
    }

    /// 从 JSON 反序列化（Plan Cache / Replayer）。
    pub fn from_json(text: &str) -> Result<Self> {
        serde_json::from_str(text).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
    }

    /// 前序遍历（docs/02 §2.3 关系代数遍历）。
    pub fn pre_order(&self) -> Vec<&LogicalPlanNode> {
        let mut out = Vec::new();
        self.collect_pre_order(&mut out);
        out
    }

    fn collect_pre_order<'a>(&'a self, out: &mut Vec<&'a LogicalPlanNode>) {
        out.push(self);
        for child in &self.children {
            child.collect_pre_order(out);
        }
    }

    /// 统计节点数与算子分布（供 EXPLAIN / 差分测试）。
    pub fn node_count(&self) -> usize {
        self.pre_order().len()
    }

    /// 查找满足谓词的节点（规则匹配器基础）。
    pub fn find(&self, predicate: impl Fn(&LogicalPlanNode) -> bool) -> Vec<&LogicalPlanNode> {
        self.pre_order()
            .into_iter()
            .filter(|n| predicate(n))
            .collect()
    }

    /// 计划等价比较（docs/04 F-TEST-009、AC-RBO-012）：结构 + Schema + 表达式语义。
    pub fn equivalent_to(&self, other: &LogicalPlanNode) -> bool {
        if self.plan != other.plan || self.schema != other.schema {
            return false;
        }
        if self.children.len() != other.children.len() {
            return false;
        }
        self.children
            .iter()
            .zip(&other.children)
            .all(|(a, b)| a.equivalent_to(b))
    }

    /// 带表达式语义比较（忽略 ColumnRef.index 差异，用 id 判定）。
    pub fn semantically_eq(&self, other: &LogicalPlanNode) -> bool {
        if self.plan.op_id() != other.plan.op_id() || self.schema != other.schema {
            return false;
        }
        if !plan_semantic_eq(&self.plan, &other.plan) {
            return false;
        }
        self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(a, b)| a.semantically_eq(b))
    }

    /// 记录规则命中（ADR-6 / docs/02 §2.3 rule_trace）。
    pub fn record_rule_hit(&mut self, rule: &str, before: PlanNodeId) {
        self.rule_trace.push(RuleHit {
            rule: rule.to_string(),
            before,
            after: self.id,
        });
    }

    /// 重建一棵子树（规则应用后，保留 rule_trace）。
    pub fn with_plan(&self, plan: LogicalPlan) -> Result<Self> {
        Self::new(self.id, plan, self.children.clone())
    }
}

/// 表达式级语义比较（忽略派生属性）。
fn plan_semantic_eq(a: &LogicalPlan, b: &LogicalPlan) -> bool {
    match (a, b) {
        (LogicalPlan::Projection(x), LogicalPlan::Projection(y)) => {
            exprs_semantic_eq(&x.exprs, &y.exprs)
        }
        (LogicalPlan::Selection(x), LogicalPlan::Selection(y)) => {
            x.predicate.structurally_eq(&y.predicate)
        }
        (LogicalPlan::Join(x), LogicalPlan::Join(y)) => {
            x.join_type == y.join_type
                && match (&x.condition, &y.condition) {
                    (Some(p), Some(q)) => p.structurally_eq(q),
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => a == b,
    }
}

fn exprs_semantic_eq(a: &[Expr], b: &[Expr]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.structurally_eq(y))
}

/// Schema 推导（docs/02 §2.3：列裁剪后收缩）。
fn derive_schema(plan: &LogicalPlan, children: &[Arc<LogicalPlanNode>]) -> Result<Schema> {
    let input = || -> Schema {
        children
            .first()
            .map(|c| c.schema.clone())
            .unwrap_or_default()
    };
    let input_arity = children
        .first()
        .map(|c| c.schema.columns.len())
        .unwrap_or(0);
    Ok(match plan {
        LogicalPlan::TableDual | LogicalPlan::MemoGroup(_) => Schema::default(),
        LogicalPlan::DataSource(ds) => Schema {
            columns: ds
                .columns
                .iter()
                .map(|n| ColumnMeta {
                    name: n.clone(),
                    data_type: DataType::Null,
                    nullable: true,
                })
                .collect(),
        },
        LogicalPlan::Projection(p) => Schema {
            columns: p
                .exprs
                .iter()
                .enumerate()
                .map(|(i, e)| ColumnMeta {
                    name: p
                        .aliases
                        .get(i)
                        .and_then(|a| a.clone())
                        .unwrap_or_else(|| format!("expr_{i}")),
                    data_type: e.data_type(),
                    nullable: true,
                })
                .collect(),
        },
        LogicalPlan::Join(j) => {
            let mut cols = children
                .first()
                .map(|c| c.schema.columns.clone())
                .unwrap_or_default();
            if let Some(rhs) = children.get(1) {
                cols.extend(rhs.schema.columns.iter().cloned());
            }
            // Semi/Anti 只输出左表列。
            if matches!(
                j.join_type,
                crate::logical::JoinType::Semi | crate::logical::JoinType::AntiSemi
            ) {
                return Ok(Schema {
                    columns: children
                        .first()
                        .map(|c| c.schema.columns.clone())
                        .unwrap_or_default(),
                });
            }
            Schema { columns: cols }
        }
        LogicalPlan::Aggregation(agg) => {
            let mut cols: Vec<ColumnMeta> = agg
                .group_by
                .iter()
                .enumerate()
                .map(|(i, e)| ColumnMeta {
                    name: format!("group_{i}"),
                    data_type: e.data_type(),
                    nullable: true,
                })
                .collect();
            for (i, (kind, _, _)) in agg.agg_calls.iter().enumerate() {
                cols.push(ColumnMeta {
                    name: format!("{kind:?}_{i}"),
                    data_type: DataType::Int64,
                    nullable: true,
                });
            }
            Schema { columns: cols }
        }
        LogicalPlan::Expand(e) => {
            let mut cols = input().columns;
            cols.push(ColumnMeta {
                name: "grouping_id".into(),
                data_type: DataType::Int64,
                nullable: false,
            });
            let _ = e;
            Schema { columns: cols }
        }
        LogicalPlan::Window(w) => {
            let mut cols = input().columns;
            for (i, (kind, _)) in w.functions.iter().enumerate() {
                cols.push(ColumnMeta {
                    name: format!("{kind:?}_{i}"),
                    data_type: DataType::Int64,
                    nullable: true,
                });
            }
            Schema { columns: cols }
        }
        LogicalPlan::SetOp(_) | LogicalPlan::PartitionUnion => input(),
        LogicalPlan::Cte(_) => input(),
        LogicalPlan::Sequence | LogicalPlan::Dml(_) | LogicalPlan::Admin(_) => Schema::default(),
        // 其余一元算子透传输入 Schema。
        _ => {
            let _ = input_arity;
            input()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logical::{data_source, inner_join, limit, projection, selection, Arity};
    use lattice_expr::{Expr, FuncSig};
    use lattice_types::Scalar;

    fn lit(v: i128) -> Expr {
        Expr::Literal(Scalar::Int(v), DataType::Int64)
    }

    fn tree() -> Arc<LogicalPlanNode> {
        let scan = Arc::new(
            LogicalPlanNode::data_source(
                PlanNodeId(1),
                "test.t",
                &[("a", DataType::Int64), ("b", DataType::VarChar(32))],
            )
            .unwrap(),
        );
        let sel = Arc::new(
            LogicalPlanNode::new(
                PlanNodeId(2),
                selection(Expr::binary(
                    FuncSig::Gt,
                    Expr::column(0, 1),
                    lit(10),
                    DataType::Bool,
                )),
                vec![scan],
            )
            .unwrap(),
        );
        Arc::new(
            LogicalPlanNode::new(
                PlanNodeId(3),
                projection(vec![Expr::column(0, 1)], vec![Some("a".into())]),
                vec![sel],
            )
            .unwrap(),
        )
    }

    #[test]
    fn new_validates_arity() {
        assert!(
            LogicalPlanNode::new(PlanNodeId(1), data_source("t", vec![]), vec![tree()]).is_err()
        );
        assert!(LogicalPlanNode::new(PlanNodeId(1), inner_join(None), vec![tree()]).is_err());
        assert!(
            LogicalPlanNode::new(PlanNodeId(1), inner_join(None), vec![tree(), tree()]).is_ok()
        );
    }

    #[test]
    fn schema_derivation_across_tree() {
        let t = tree();
        assert_eq!(t.schema.columns.len(), 1); // Projection
        assert_eq!(t.schema.columns[0].name, "a");
        assert_eq!(t.children[0].schema.columns.len(), 2); // Selection 透传 DataSource
    }

    #[test]
    fn pre_order_and_find() {
        let t = tree();
        assert_eq!(t.node_count(), 3);
        let duals = t.find(|n| matches!(n.plan, LogicalPlan::TableDual));
        assert!(duals.is_empty());
        let selections = t.find(|n| matches!(n.plan, LogicalPlan::Selection(_)));
        assert_eq!(selections.len(), 1);
    }

    #[test]
    fn json_roundtrip_preserves_equivalence() {
        let t = tree();
        let json = t.to_json().unwrap();
        let back = LogicalPlanNode::from_json(&json).unwrap();
        assert!(t.equivalent_to(&back));
        assert!(t.semantically_eq(&back));
    }

    #[test]
    fn semantic_eq_ignores_column_index_derived_flags() {
        let a = LogicalPlanNode::new(
            PlanNodeId(1),
            projection(vec![Expr::column(0, 7)], vec![None]),
            vec![Arc::new(LogicalPlanNode::table_dual(PlanNodeId(0)))],
        )
        .unwrap();
        let b = LogicalPlanNode::new(
            PlanNodeId(1),
            projection(vec![Expr::column(5, 7)], vec![None]),
            vec![Arc::new(LogicalPlanNode::table_dual(PlanNodeId(0)))],
        )
        .unwrap();
        assert!(!a.equivalent_to(&b)); // 结构比较区分 index
        assert!(a.semantically_eq(&b)); // 语义比较只看 id
    }

    #[test]
    fn rule_trace_records_hits() {
        let mut t = tree();
        let n = Arc::make_mut(&mut t);
        n.record_rule_hit("R-PPD", PlanNodeId(2));
        assert_eq!(n.rule_trace[0].rule, "R-PPD");
        assert_eq!(n.rule_trace[0].after, PlanNodeId(3));
    }

    #[test]
    fn join_schema_concatenates_but_semi_does_not() {
        let left = Arc::new(
            LogicalPlanNode::data_source(PlanNodeId(1), "l", &[("a", DataType::Int64)]).unwrap(),
        );
        let right = Arc::new(
            LogicalPlanNode::data_source(PlanNodeId(2), "r", &[("b", DataType::Int64)]).unwrap(),
        );
        let inner = LogicalPlanNode::new(
            PlanNodeId(3),
            inner_join(None),
            vec![left.clone(), right.clone()],
        )
        .unwrap();
        assert_eq!(inner.schema.columns.len(), 2);

        let semi = LogicalPlanNode::new(
            PlanNodeId(4),
            LogicalPlan::Join(crate::logical::Join {
                join_type: crate::logical::JoinType::Semi,
                condition: None,
                using_columns: vec![],
            }),
            vec![left, right],
        )
        .unwrap();
        assert_eq!(semi.schema.columns.len(), 1);
    }

    #[test]
    fn limit_transparently_passes_schema() {
        let scan = Arc::new(
            LogicalPlanNode::data_source(PlanNodeId(1), "t", &[("a", DataType::Int64)]).unwrap(),
        );
        let lim = LogicalPlanNode::new(PlanNodeId(2), limit(5, 0), vec![scan]).unwrap();
        assert_eq!(lim.schema.columns.len(), 1);
    }

    #[test]
    fn arity_enum_accepts() {
        assert!(Arity::Variadic.accepts(0));
        assert!(Arity::AtLeast(2).accepts(3));
        assert!(!Arity::Exactly(0).accepts(1));
    }
}
