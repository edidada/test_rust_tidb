//! Metrics 骨架（docs/04 F-OBS-001、AC-OBS-001；docs/03 L0：tracing/metrics）。
//!
//! L0 只提供**无外部依赖**的指标注册与快照接口；tracing span 由各 crate 通过
//! `tracing` facade 埋点（L0 要求先埋观测点，docs/03 §1 原则 3）。

use std::collections::BTreeMap;

/// 支持的指标值类型（counter / gauge / histogram 的简化统一表示）。
#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    /// 直方图桶的累计计数。
    Histogram(Vec<(f64, u64)>),
}

/// 分层指标注册表：键形如 `executor.rows_scanned`。
#[derive(Debug, Clone, Default)]
pub struct MetricsRegistry {
    values: BTreeMap<String, MetricValue>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn counter_inc(&mut self, name: impl Into<String>, delta: u64) {
        let entry = self
            .values
            .entry(name.into())
            .or_insert(MetricValue::Counter(0));
        if let MetricValue::Counter(v) = entry {
            *v += delta;
        }
    }

    pub fn gauge_set(&mut self, name: impl Into<String>, value: f64) {
        self.values.insert(name.into(), MetricValue::Gauge(value));
    }

    pub fn histogram_observe(&mut self, name: impl Into<String>, value: f64, bounds: &[f64]) {
        let entry = self
            .values
            .entry(name.into())
            .or_insert_with(|| MetricValue::Histogram(bounds.iter().map(|b| (*b, 0)).collect()));
        if let MetricValue::Histogram(hist) = entry {
            for (bound, count) in hist.iter_mut() {
                if value <= *bound {
                    *count += 1;
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&MetricValue> {
        self.values.get(name)
    }

    pub fn snapshot(&self) -> &BTreeMap<String, MetricValue> {
        &self.values
    }

    /// 各层指标是否齐全（AC-OBS-001 的最小检查：5 层均有指标）。
    pub fn layers_covered(&self, layers: &[&str]) -> bool {
        layers.iter().all(|layer| {
            self.values
                .keys()
                .any(|k| k.starts_with(&format!("{layer}.")))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_and_gauge_work() {
        let mut reg = MetricsRegistry::new();
        reg.counter_inc("executor.rows_scanned", 10);
        reg.counter_inc("executor.rows_scanned", 5);
        assert_eq!(
            reg.get("executor.rows_scanned"),
            Some(&MetricValue::Counter(15))
        );
        reg.gauge_set("storage.mem_bytes", 1024.0);
        assert_eq!(
            reg.get("storage.mem_bytes"),
            Some(&MetricValue::Gauge(1024.0))
        );
    }

    #[test]
    fn histogram_accumulates_cumulative_buckets() {
        let mut reg = MetricsRegistry::new();
        let bounds = [1.0, 10.0, 100.0];
        reg.histogram_observe("query.latency_ms", 5.0, &bounds);
        reg.histogram_observe("query.latency_ms", 50.0, &bounds);
        let MetricValue::Histogram(hist) = reg.get("query.latency_ms").unwrap() else {
            panic!("expected histogram");
        };
        assert_eq!(hist[0].1, 0);
        assert_eq!(hist[1].1, 1);
        assert_eq!(hist[2].1, 2);
    }

    #[test]
    fn layer_coverage_check() {
        let mut reg = MetricsRegistry::new();
        for layer in ["server", "executor", "optimizer", "storage", "meta"] {
            reg.counter_inc(format!("{layer}.ops"), 1);
        }
        assert!(reg.layers_covered(&["server", "executor", "optimizer", "storage", "meta"]));
        assert!(!reg.layers_covered(&["server", "nonexistent"]));
    }
}
