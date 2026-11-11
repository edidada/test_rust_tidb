//! 随机数种子策略与实验记录模板（docs/03 L0、docs/04 F-TEST-003 / F-OBS-012、
//! docs/05 §7.2 / AC-BENCH-010：固定种子 + 可复现实验元数据）。

use serde::{Deserialize, Serialize};

/// 全项目默认种子（W01 起点 2026-11-09）。
pub const DEFAULT_SEED: u64 = 20261109;

/// 从主种子派生独立子流种子，避免不同实验组件相互干扰。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedPolicy {
    pub master: u64,
}

impl SeedPolicy {
    pub fn new(master: u64) -> Self {
        Self { master }
    }

    /// SplitMix64 风格的确定性派生：同名组件 + 序号得到稳定子种子。
    pub fn derive(&self, component: &str, index: u64) -> u64 {
        let mut x = self.master
            ^ component.bytes().fold(0x9e3779b97f4a7c15u64, |acc, b| {
                (acc ^ (b as u64)).wrapping_mul(0x100000001b3)
            })
            ^ index.wrapping_mul(0x9e3779b97f4a7c15);
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        x ^ (x >> 31)
    }

    /// 简单 LCG，供无 rand 依赖的 L0 基准脚本模板使用。
    pub fn next_random(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state >> 33
    }
}

impl Default for SeedPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_SEED)
    }
}

/// 实验元数据记录（docs/05 §7.3 可复现规范 / AC-BENCH-010）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentMetadata {
    pub name: String,
    pub seed: u64,
    pub repeat: usize,
    pub git_commit: String,
    pub rustc_version: String,
    pub config_digest: String,
    pub dataset: String,
    pub baseline: String,
}

impl ExperimentMetadata {
    pub fn new(name: impl Into<String>, seed: u64, repeat: usize) -> Self {
        Self {
            name: name.into(),
            seed,
            repeat,
            git_commit: String::new(),
            rustc_version: String::new(),
            config_digest: String::new(),
            dataset: String::new(),
            baseline: String::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_seeds_are_deterministic_and_distinct() {
        let policy = SeedPolicy::new(DEFAULT_SEED);
        assert_eq!(policy.derive("scan", 0), policy.derive("scan", 0));
        assert_ne!(policy.derive("scan", 0), policy.derive("join", 0));
        assert_ne!(policy.derive("scan", 0), policy.derive("scan", 1));
    }

    #[test]
    fn lcg_is_reproducible() {
        let mut a = 42u64;
        let mut b = 42u64;
        let seq_a: Vec<u64> = (0..8).map(|_| SeedPolicy::next_random(&mut a)).collect();
        let seq_b: Vec<u64> = (0..8).map(|_| SeedPolicy::next_random(&mut b)).collect();
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn metadata_serializes_to_json() {
        let meta = ExperimentMetadata::new("scan-bench", DEFAULT_SEED, 5);
        let json = meta.to_json();
        assert!(json.contains("\"seed\": 20261109"));
        assert!(json.contains("\"repeat\": 5"));
    }
}
