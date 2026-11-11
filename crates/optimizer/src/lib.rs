//! `optimizer` crate (L6/L7/L8 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §3、§11；docs/03 L6–L8。

use lattice_common::Result;
use lattice_planner::{LogicalPlanNode, OptCtx, Pattern, Rule};

/// 统计信息（docs/02 §2.4 `Statistics` 的 L0 简化版）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Statistics {
    pub row_count: f64,
    pub row_width: f64,
}

impl Statistics {
    pub fn new(row_count: f64, row_width: f64) -> Self {
        Self {
            row_count,
            row_width,
        }
    }
}

/// 代价分解（docs/02 §2.3 `Cost`：cpu/io/net/mem）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cost {
    pub cpu: f64,
    pub io: f64,
    pub net: f64,
    pub mem: f64,
}

impl Cost {
    pub fn total(&self, weights: &lattice_common::CostWeights) -> f64 {
        self.cpu * weights.cpu
            + self.io * weights.io
            + self.net * weights.net
            + self.mem * weights.mem
    }
}

/// RBO 优化器 trait（docs/02 §3）。
pub trait RboOptimizer {
    fn optimize(&self, plan: LogicalPlanNode) -> Result<LogicalPlanNode>;
}

/// 统计提供者 trait（docs/02 §3）。
pub trait StatisticsProvider {
    fn selectivity(&self, predicate: &str, input: &Statistics) -> Result<f64>;
}

/// 基数估计器 trait（docs/02 §3）。
pub trait CardinalityEstimator {
    fn estimate(&self, node: &LogicalPlanNode) -> Result<Statistics>;
}

/// 代价模型 trait（docs/02 §3）。
pub trait CostModel {
    fn cost(&self, op: &str, input: &[Statistics]) -> Result<Cost>;
}

/// CBO 优化器 trait（docs/02 §3）。
pub trait CboOptimizer {
    fn optimize(&self, plan: LogicalPlanNode) -> Result<lattice_planner::LogicalPlanNode>;
}

/// 默认选择性提供者：所有代价常数经 `OptimizerConfig` 注入（docs/02 §14，
/// 禁止硬编码），此处仅用于单测 mock。
#[derive(Debug, Clone)]
pub struct ConstantSelectivity {
    pub value: f64,
}

impl StatisticsProvider for ConstantSelectivity {
    fn selectivity(&self, _predicate: &str, _input: &Statistics) -> Result<f64> {
        Ok(self.value)
    }
}

/// 恒等 RBO（L0 占位：规则引擎在 L6 实现）。
#[derive(Debug, Default)]
pub struct IdentityRbo;

impl RboOptimizer for IdentityRbo {
    fn optimize(&self, plan: LogicalPlanNode) -> Result<LogicalPlanNode> {
        Ok(plan)
    }
}

/// 规则接口实现示例（供依赖方向与 trait mock 检查）。
#[derive(Debug)]
pub struct NoopRule;

impl Rule for NoopRule {
    fn name(&self) -> &str {
        "R-NOOP"
    }

    fn pattern(&self) -> Pattern {
        Pattern {
            op: "TableDual".into(),
        }
    }

    fn apply(&self, _plan: &LogicalPlanNode, _ctx: &OptCtx) -> Result<Option<LogicalPlanNode>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_total_uses_injected_weights() {
        let weights = lattice_common::CostWeights {
            cpu: 1.0,
            io: 10.0,
            net: 5.0,
            mem: 2.0,
        };
        let cost = Cost {
            cpu: 2.0,
            io: 3.0,
            net: 4.0,
            mem: 1.0,
        };
        assert_eq!(cost.total(&weights), 2.0 + 30.0 + 20.0 + 2.0);
    }

    #[test]
    fn identity_rbo_and_noop_rule_are_mocks() {
        let plan = LogicalPlanNode::table_dual(lattice_planner::PlanNodeId(1));
        assert_eq!(IdentityRbo.optimize(plan.clone()).unwrap(), plan);
        assert_eq!(NoopRule.name(), "R-NOOP");
        assert!(NoopRule.apply(&plan, &OptCtx).unwrap().is_none());
    }

    #[test]
    fn constant_selectivity_provider_is_injectable() {
        let provider = ConstantSelectivity { value: 0.25 };
        assert_eq!(
            provider
                .selectivity("a=1", &Statistics::new(100.0, 8.0))
                .unwrap(),
            0.25
        );
    }
}
