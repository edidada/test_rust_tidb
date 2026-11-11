//! 执行上下文与统计埋点（docs/02 §2.3/§10、docs/03 L3 W15、docs/04 F-OBS）。

use lattice_common::{Error, ErrorKind, Result};
use std::sync::{Arc, Mutex};

/// 执行期统计（docs/02 §2.3：actual_rows / time / memory / spill）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecStats {
    pub actual_rows: u64,
    pub time_ms: u64,
    pub memory_bytes: u64,
    pub spill_bytes: u64,
}

impl ExecStats {
    pub fn record_rows(&mut self, n: usize) {
        self.actual_rows += n as u64;
    }

    pub fn merge(&mut self, other: ExecStats) {
        self.actual_rows += other.actual_rows;
        self.time_ms += other.time_ms;
        self.memory_bytes = self.memory_bytes.max(other.memory_bytes);
        self.spill_bytes += other.spill_bytes;
    }
}

/// 内存跟踪器（docs/02 §10：全局 MemoryTracker + 查询级配额）。
#[derive(Debug, Clone)]
pub struct MemoryTracker {
    inner: Arc<Mutex<TrackerInner>>,
}

#[derive(Debug, Default)]
struct TrackerInner {
    used: u64,
    quota: u64,
    peak: u64,
}

impl MemoryTracker {
    pub fn new(quota: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TrackerInner {
                used: 0,
                quota,
                peak: 0,
            })),
        }
    }

    pub fn try_alloc(&self, bytes: u64) -> Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| Error::new(ErrorKind::Internal, "MemoryTracker 锁已 poisoned"))?;
        let next = inner.used.saturating_add(bytes);
        if next > inner.quota {
            return Err(Error::new(
                ErrorKind::OutOfMemory,
                format!("内存配额 {} 超限（申请 {}）", inner.quota, next),
            ));
        }
        inner.used = next;
        inner.peak = inner.peak.max(next);
        Ok(())
    }

    pub fn release(&self, bytes: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.used = inner.used.saturating_sub(bytes);
        }
    }

    pub fn used(&self) -> u64 {
        self.inner.lock().map(|i| i.used).unwrap_or(0)
    }

    pub fn peak(&self) -> u64 {
        self.inner.lock().map(|i| i.peak).unwrap_or(0)
    }
}

/// 取消标记（docs/02 §10：CancellationToken 贯穿算子）。
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    cancelled: Arc<Mutex<bool>>,
}

impl CancelToken {
    pub fn cancel(&self) {
        if let Ok(mut c) = self.cancelled.lock() {
            *c = true;
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.lock().map(|c| *c).unwrap_or(false)
    }

    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::new(ErrorKind::Internal, "查询已取消"))
        } else {
            Ok(())
        }
    }
}

/// 执行上下文（docs/02 §3 `ExecCtx`；L3 填充 MemoryTracker / CancelToken / 统计）。
#[derive(Debug, Clone)]
pub struct ExecCtx {
    pub tracker: MemoryTracker,
    pub cancel: CancelToken,
    pub chunk_capacity: usize,
}

impl Default for ExecCtx {
    fn default() -> Self {
        Self {
            tracker: MemoryTracker::new(512 * 1024 * 1024),
            cancel: CancelToken::default(),
            chunk_capacity: 1024,
        }
    }
}

impl ExecCtx {
    pub fn new(memory_quota: u64, chunk_capacity: usize) -> Self {
        Self {
            tracker: MemoryTracker::new(memory_quota),
            cancel: CancelToken::default(),
            chunk_capacity,
        }
    }

    pub fn check(&self) -> Result<()> {
        self.cancel.check()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_enforces_quota() {
        let t = MemoryTracker::new(100);
        assert!(t.try_alloc(60).is_ok());
        assert_eq!(t.try_alloc(50).unwrap_err().kind, ErrorKind::OutOfMemory);
        assert_eq!(t.used(), 60);
        t.release(20);
        assert_eq!(t.used(), 40);
        assert_eq!(t.peak(), 60);
    }

    #[test]
    fn cancel_token_propagates() {
        let token = CancelToken::default();
        assert!(token.check().is_ok());
        token.cancel();
        assert!(token.is_cancelled());
        assert!(token.check().is_err());
    }

    #[test]
    fn exec_stats_merge() {
        let mut a = ExecStats {
            actual_rows: 3,
            time_ms: 5,
            ..Default::default()
        };
        a.merge(ExecStats {
            actual_rows: 2,
            time_ms: 7,
            memory_bytes: 10,
            spill_bytes: 4,
        });
        assert_eq!(a.actual_rows, 5);
        assert_eq!(a.time_ms, 12);
        assert_eq!(a.spill_bytes, 4);
    }

    #[test]
    fn default_ctx_is_usable() {
        let ctx = ExecCtx::default();
        assert!(ctx.check().is_ok());
        assert_eq!(ctx.chunk_capacity, 1024);
    }
}
