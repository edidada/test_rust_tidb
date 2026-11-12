//! W18 GC safe point 与版本清理、并发冲突注入、SI 异常注入
//! （docs/02 §4.3、docs/04 F-STO-010/F-TXN-015/F-TXN-011、AC-TXN-007/002/005）。
//!
//! 约束（docs/05 AC-TXN-007）：**不删除任何活跃快照所需版本**。
//! 策略：对每个 user_key，保留「≤ safe_point 的最新 write 记录」以及
//! 所有 commit_ts > safe_point 的记录；其余历史版本与对应 default 值回收。
//! 若存在活跃快照 ts < safe_point，则实际清理上界收紧到活跃快照下界。

use crate::engine::MvccEngine;
use crate::{Key, Timestamp};
use lattice_common::{Error, ErrorKind, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// GC 安全点（docs/04 F-STO-010 / F-TXN-015）。
#[derive(Debug)]
pub struct GcSafePoint {
    ts: AtomicU64,
}

impl GcSafePoint {
    pub fn new(ts: Timestamp) -> Self {
        Self {
            ts: AtomicU64::new(ts),
        }
    }

    pub fn get(&self) -> Timestamp {
        self.ts.load(Ordering::SeqCst)
    }

    /// 推进 safe point（只增不减）。
    pub fn advance(&self, new_ts: Timestamp) -> Result<Timestamp> {
        let mut cur = self.ts.load(Ordering::SeqCst);
        loop {
            if new_ts <= cur {
                return Err(Error::new(
                    ErrorKind::Internal,
                    format!("safe point 只能推进: {new_ts} <= {cur}"),
                ));
            }
            match self
                .ts
                .compare_exchange(cur, new_ts, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Ok(new_ts),
                Err(actual) => cur = actual,
            }
        }
    }
}

/// 活跃快照登记（docs/02 §4.3：GC 必须保护活跃快照）。
#[derive(Debug, Default)]
pub struct ActiveSnapshots {
    inner: RwLock<BTreeSet<Timestamp>>,
}

impl ActiveSnapshots {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, ts: Timestamp) -> Result<()> {
        self.inner
            .write()
            .map_err(|_| Error::new(ErrorKind::Internal, "ActiveSnapshots 锁已 poisoned"))?
            .insert(ts);
        Ok(())
    }

    pub fn unregister(&self, ts: Timestamp) {
        if let Ok(mut s) = self.inner.write() {
            s.remove(&ts);
        }
    }

    /// 最小活跃快照（None = 无活跃快照）。
    pub fn min(&self) -> Option<Timestamp> {
        self.inner
            .read()
            .ok()
            .and_then(|s| s.iter().next().copied())
    }

    pub fn len(&self) -> usize {
        self.inner.read().map(|s| s.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// GC 结果统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GcStats {
    pub scanned_keys: usize,
    pub deleted_versions: usize,
    pub deleted_values: usize,
    pub effective_upper: Timestamp,
}

/// GC 执行器。
#[derive(Debug)]
pub struct Gc {
    engine: Arc<MvccEngine>,
    safe_point: Arc<GcSafePoint>,
    active: Arc<ActiveSnapshots>,
}

impl Gc {
    pub fn new(
        engine: Arc<MvccEngine>,
        safe_point: Arc<GcSafePoint>,
        active: Arc<ActiveSnapshots>,
    ) -> Self {
        Self {
            engine,
            safe_point,
            active,
        }
    }

    pub fn safe_point(&self) -> &Arc<GcSafePoint> {
        &self.safe_point
    }

    pub fn snapshots(&self) -> &Arc<ActiveSnapshots> {
        &self.active
    }

    /// 执行一次 GC（docs/04 F-STO-010）。
    ///
    /// 实际清理上界 = min(safe_point, min_active_snapshot)，
    /// 保证活跃快照所需版本不被删除（AC-TXN-007）。
    pub fn collect(&self) -> Result<GcStats> {
        let requested = self.safe_point.get();
        let effective_upper = match self.active.min() {
            Some(min_ts) => min_ts.min(requested),
            None => requested,
        };
        let mut stats = GcStats {
            effective_upper,
            ..Default::default()
        };

        // 收集所有 user_key（write CF 的 key = user_key ++ 降序 TS）。
        let mut by_user: BTreeMap<Key, Vec<(Timestamp, crate::engine::WriteInfo)>> =
            BTreeMap::new();
        for (k, v) in self.engine.write.scan_prefix(&[])? {
            let Some((uk, ts)) = crate::encoding::split_txn_key(&k) else {
                continue;
            };
            let info = crate::engine::WriteInfo::from_bytes(&v)?;
            by_user.entry(uk).or_default().push((ts, info));
        }

        for (user_key, mut records) in by_user {
            stats.scanned_keys += 1;
            records.sort_by_key(|r| r.0);
            // ≤ effective_upper 的最新一条必须保留（历史读基线）。
            let keep_ts = records
                .iter()
                .filter(|(ts, _)| *ts <= effective_upper)
                .map(|(ts, _)| *ts)
                .max();
            for (ts, info) in records {
                if ts > effective_upper {
                    continue; // 安全点之后的版本一律保留
                }
                if Some(ts) == keep_ts {
                    continue; // 每个 key 至少保留一份可见历史
                }
                // 删除该 write 记录及对应 default 值。
                self.engine.delete_write(&user_key, ts)?;
                stats.deleted_versions += 1;
                if matches!(info.write_type, crate::engine::WriteType::Put) {
                    self.engine.raw_delete(&user_key, info.start_ts)?;
                    stats.deleted_values += 1;
                }
            }
        }
        Ok(stats)
    }
}

/// 并发写冲突注入（docs/05 AC-TXN-005、docs/03 L4 DoD V3）：
/// 多线程竞争同一 key，统计冲突检出与成功提交数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictInjectionResult {
    pub threads: usize,
    pub committed: usize,
    pub conflicts: usize,
    pub other_errors: usize,
}

pub fn inject_concurrent_writes(threads: usize, rounds: usize) -> Result<ConflictInjectionResult> {
    use crate::txn::{Mutation, TxnManager};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    let mgr = Arc::new(TxnManager::memory());
    let committed = Arc::new(AtomicUsize::new(0));
    let conflicts = Arc::new(AtomicUsize::new(0));
    let other = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for t in 0..threads {
        let mgr = Arc::clone(&mgr);
        let committed = Arc::clone(&committed);
        let conflicts = Arc::clone(&conflicts);
        let other = Arc::clone(&other);
        handles.push(std::thread::spawn(move || {
            for r in 0..rounds {
                let value = format!("t{t}-r{r}");
                match mgr.auto_commit(vec![Mutation::put(b"hot", value.as_bytes())], 100) {
                    Ok(_) => {
                        committed.fetch_add(1, AtomicOrdering::SeqCst);
                    }
                    Err(e) if e.kind == ErrorKind::WriteConflict => {
                        conflicts.fetch_add(1, AtomicOrdering::SeqCst);
                    }
                    Err(_) => {
                        other.fetch_add(1, AtomicOrdering::SeqCst);
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join()
            .map_err(|_| Error::new(ErrorKind::Internal, "并发注入线程 panic"))?;
    }
    Ok(ConflictInjectionResult {
        threads,
        committed: committed.load(AtomicOrdering::SeqCst),
        conflicts: conflicts.load(AtomicOrdering::SeqCst),
        other_errors: other.load(AtomicOrdering::SeqCst),
    })
}

/// SI 异常注入套件（docs/05 AC-TXN-002：脏读/不可重复读/幻读/写偏斜检出率 100%）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiAnomalyResult {
    pub dirty_read_prevented: bool,
    pub non_repeatable_read_prevented: bool,
    pub phantom_read_prevented: bool,
    pub write_skew_detected: bool,
}

pub fn inject_si_anomalies() -> Result<SiAnomalyResult> {
    use crate::txn::{Mutation, TxnManager};

    let mgr = TxnManager::memory();

    // 1. 脏读：Prewrite 未提交时，读遇锁（KeyIsLocked）→ 不返回未提交值。
    mgr.auto_commit(vec![Mutation::put(b"dirty", b"v0")], 100)?;
    let mut pending = mgr.begin()?;
    pending.write(Mutation::put(b"dirty", b"uncommitted"))?;
    pending.prewrite(100)?;
    let read_ts = mgr.engine().get(b"dirty", 1000).is_err(); // 遇锁
    let dirty_read_prevented = read_ts;
    pending.rollback()?;

    // 2. 不可重复读：事务内两次读同一快照，值不变。
    let t = mgr.begin()?;
    let first = t.get(b"nr")?;
    mgr.auto_commit(vec![Mutation::put(b"nr", b"v2")], 100)?;
    let second = t.get(b"nr")?;
    let non_repeatable_read_prevented = first == second;

    // 3. 幻读：事务快照内 scan 范围，新插入行对同一快照不可见。
    mgr.auto_commit(vec![Mutation::put(b"ph_a", b"1")], 100)?;
    let snapshot_ts = mgr.begin()?.start_ts;
    let range = crate::Range {
        start: b"ph_".to_vec(),
        end: b"ph_z".to_vec(),
    };
    let before = mgr.engine().scan(&range, snapshot_ts)?;
    mgr.auto_commit(vec![Mutation::put(b"ph_b", b"2")], 100)?;
    let after = mgr.engine().scan(&range, snapshot_ts)?;
    let phantom_read_prevented = before == after;

    // 4. 写偏斜：两个事务各自读同一 key 后写不同 key，SI 下均提交 → 检出（需 SSI 才能阻止）。
    let mut t1 = mgr.begin()?;
    let mut t2 = mgr.begin()?;
    let _ = t1.get(b"guard")?;
    let _ = t2.get(b"guard")?;
    t1.write(Mutation::put(b"skew_x", b"1"))?;
    t2.write(Mutation::put(b"skew_y", b"1"))?;
    t1.prewrite(100)?;
    t2.prewrite(100)?;
    let c1 = mgr.tso_commit_ts();
    let c2 = mgr.tso_commit_ts();
    t1.commit(c1)?;
    t2.commit(c2)?;
    // 两事务都成功 → SI 写偏斜存在（被本套件检出；SSI 在 L15 实现）。
    let write_skew_detected =
        mgr.engine().get(b"skew_x", c2)?.is_some() && mgr.engine().get(b"skew_y", c2)?.is_some();

    Ok(SiAnomalyResult {
        dirty_read_prevented,
        non_repeatable_read_prevented,
        phantom_read_prevented,
        write_skew_detected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::txn::{Mutation, TxnManager};

    fn setup_versions() -> (Arc<MvccEngine>, TxnManager) {
        let mgr = TxnManager::memory();
        // 对同一 key 连续写 5 个版本。
        for i in 0..5 {
            mgr.auto_commit(vec![Mutation::put(b"k", format!("v{i}").as_bytes())], 100)
                .unwrap();
        }
        (Arc::clone(mgr.engine()), mgr)
    }

    #[test]
    fn safe_point_only_advances() {
        let sp = GcSafePoint::new(10);
        assert_eq!(sp.get(), 10);
        assert_eq!(sp.advance(20).unwrap(), 20);
        assert!(sp.advance(15).is_err());
        assert_eq!(sp.get(), 20);
    }

    #[test]
    fn active_snapshots_tracking() {
        let a = ActiveSnapshots::new();
        assert!(a.is_empty());
        a.register(5).unwrap();
        a.register(9).unwrap();
        assert_eq!(a.min(), Some(5));
        assert_eq!(a.len(), 2);
        a.unregister(5);
        assert_eq!(a.min(), Some(9));
    }

    #[test]
    fn gc_keeps_latest_below_safe_point_and_deletes_older() {
        let (engine, mgr) = setup_versions();
        let before = engine.write_records(b"k").unwrap().len();
        assert_eq!(before, 5);
        let sp = Arc::new(GcSafePoint::new(8));
        let active = Arc::new(ActiveSnapshots::new());
        let gc = Gc::new(Arc::clone(&engine), sp, active);
        let stats = gc.collect().unwrap();
        assert_eq!(stats.scanned_keys, 1);
        assert_eq!(stats.effective_upper, 8);
        assert_eq!(stats.deleted_versions, 3); // 删 ts=2,4,6，保留 8/10
        let after = engine.write_records(b"k").unwrap();
        // ≤8 的最新一条(8)保留，>8 的(10)保留。
        assert_eq!(after.len(), 2);
        assert!(after.iter().all(|(ts, _)| *ts == 8 || *ts == 10));
        // 历史读仍可用（保留 ≤8 的最新版本）。
        assert!(engine.get(b"k", 8).unwrap().is_some());
        assert_eq!(engine.get(b"k", 1000).unwrap(), Some(b"v4".to_vec()));
        let _ = mgr;
    }

    #[test]
    fn gc_respects_active_snapshot() {
        let (engine, _mgr) = setup_versions();
        let sp = Arc::new(GcSafePoint::new(100));
        let active = Arc::new(ActiveSnapshots::new());
        active.register(2).unwrap(); // 活跃快照 ts=2，清理上界收紧到 2
        let gc = Gc::new(Arc::clone(&engine), sp, Arc::clone(&active));
        let stats = gc.collect().unwrap();
        assert_eq!(stats.effective_upper, 2);
        // ts=2 的活跃快照仍能读到它可见的版本。
        assert!(engine.get(b"k", 2).unwrap().is_some());
    }

    #[test]
    fn gc_never_deletes_versions_above_safe_point() {
        let (engine, _mgr) = setup_versions();
        let sp = Arc::new(GcSafePoint::new(1));
        let gc = Gc::new(Arc::clone(&engine), sp, Arc::new(ActiveSnapshots::new()));
        gc.collect().unwrap();
        let remaining = engine.write_records(b"k").unwrap();
        assert!(remaining.iter().any(|(ts, _)| *ts > 1));
        assert_eq!(engine.get(b"k", 1000).unwrap(), Some(b"v4".to_vec()));
    }

    #[test]
    fn concurrent_writes_detect_conflicts() {
        let r = inject_concurrent_writes(4, 5).unwrap();
        assert_eq!(r.threads, 4);
        assert_eq!(r.other_errors, 0, "不应出现非冲突类错误");
        assert!(r.committed > 0, "至少要有成功提交");
        // 冲突是否出现取决于线程调度（CI 与本地可能不同）；核心不变量是
        // 每个操作要么成功提交、要么被正确分类为写冲突，不丢数据、无其它错误。
        assert_eq!(r.committed + r.conflicts, 20);
    }

    #[test]
    fn deterministic_lock_conflict_injection() {
        // 确定性注入：长事务持锁 → 并发 auto_commit 必然检出 WriteConflict。
        use crate::txn::TxnManager;
        let mgr = TxnManager::memory();
        let mut holder = mgr.begin().unwrap();
        holder.write(Mutation::put(b"hot", b"hold")).unwrap();
        holder.prewrite(100).unwrap();
        let err = mgr
            .auto_commit(vec![Mutation::put(b"hot", b"other")], 100)
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::WriteConflict);
        holder.rollback().unwrap();
    }

    #[test]
    fn si_anomaly_suite_detects_all() {
        let r = inject_si_anomalies().unwrap();
        assert!(r.dirty_read_prevented);
        assert!(r.non_repeatable_read_prevented);
        assert!(r.phantom_read_prevented);
        assert!(r.write_skew_detected);
    }
}
