//! `meta` crate (L11 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §3 `TimestampOracle` / `RegionRouter`；docs/03 L11。

use lattice_common::Result;
use std::sync::atomic::{AtomicU64, Ordering};

pub type Timestamp = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionMeta {
    pub region_id: u64,
    pub leader: String,
}

/// 全局时间戳 trait（docs/02 §3）。
pub trait TimestampOracle {
    fn get_ts(&self, n: u32) -> Result<Timestamp>;
}

/// 单调 TSO 的 L0 内存实现（批量分配，docs/03 L11）。
#[derive(Debug, Default)]
pub struct MemoryTso {
    next: AtomicU64,
}

impl MemoryTso {
    pub fn new(start: Timestamp) -> Self {
        Self {
            next: AtomicU64::new(start),
        }
    }
}

impl TimestampOracle for MemoryTso {
    fn get_ts(&self, n: u32) -> Result<Timestamp> {
        let count = u64::from(n.max(1));
        Ok(self.next.fetch_add(count, Ordering::SeqCst))
    }
}

/// Region 路由 trait（docs/02 §3）。
pub trait RegionRouter {
    fn locate(&self, key: &[u8]) -> Result<RegionMeta>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tso_batch_allocation_is_monotonic() {
        let tso = MemoryTso::new(100);
        assert_eq!(tso.get_ts(1).unwrap(), 100);
        assert_eq!(tso.get_ts(3).unwrap(), 101);
        assert_eq!(tso.get_ts(1).unwrap(), 104);
    }
}
