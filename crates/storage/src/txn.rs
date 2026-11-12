//! W17 乐观 2PC 事务（docs/02 §4.3/§5、docs/04 F-TXN-001/002/005/007/008/011/012、
//! AC-TXN-001/003/005/006）。
//!
//! 单机版：BEGIN(start_ts) → Prewrite(写 lock+default、冲突检测) →
//! Commit(写 write、删 lock) / Rollback(删 lock、写 Rollback)。
//! 读遇锁 → ResolveLock（查 primary 状态，补提交或回滚 secondary）。

use crate::engine::{LockInfo, MvccEngine, WriteInfo, WriteType};
use crate::{Key, Timestamp, Value};
use lattice_common::{Error, ErrorKind, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 本地 TSO（L4 单机版；docs/04 F-TXN-002，L11 替换为全局 TSO HA）。
/// 与 `meta::MemoryTso` 契约一致：严格单调、支持批量分配。
#[derive(Debug, Default)]
pub struct LocalTso {
    next: AtomicU64,
}

impl LocalTso {
    pub fn new(start: Timestamp) -> Self {
        Self {
            next: AtomicU64::new(start),
        }
    }

    pub fn get_ts(&self, n: u32) -> Result<Timestamp> {
        Ok(self.next.fetch_add(u64::from(n.max(1)), Ordering::SeqCst))
    }

    pub fn current(&self) -> Timestamp {
        self.next.load(Ordering::SeqCst)
    }
}

/// 写变更：`value = None` 表示删除。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutation {
    pub key: Key,
    pub value: Option<Value>,
}

impl Mutation {
    pub fn put(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.to_vec(),
            value: Some(value.to_vec()),
        }
    }

    pub fn delete(key: &[u8]) -> Self {
        Self {
            key: key.to_vec(),
            value: None,
        }
    }
}

/// 事务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Active,
    Prewritten,
    Committed,
    RolledBack,
}

/// 乐观事务（docs/04 F-TXN-005）。
#[derive(Debug)]
pub struct OptimisticTxn {
    engine: Arc<MvccEngine>,
    pub start_ts: Timestamp,
    pub mutations: Vec<Mutation>,
    pub state: TxnState,
}

impl OptimisticTxn {
    pub fn begin(engine: Arc<MvccEngine>, start_ts: Timestamp) -> Self {
        Self {
            engine,
            start_ts,
            mutations: Vec::new(),
            state: TxnState::Active,
        }
    }

    /// 快照读（SI：按 start_ts，docs/04 F-TXN-003）。
    pub fn get(&self, key: &[u8]) -> Result<Option<Value>> {
        self.engine.get(key, self.start_ts)
    }

    /// 缓冲写变更（自动提交事务也走此路径）。
    pub fn write(&mut self, mutation: Mutation) -> Result<()> {
        if self.state != TxnState::Active {
            return Err(Error::new(ErrorKind::Internal, "事务已结束，不能再写入"));
        }
        self.mutations.push(mutation);
        Ok(())
    }

    /// Prewrite（docs/04 F-TXN-007）：写 lock + default，冲突检查。
    ///
    /// 冲突条件（AC-TXN-005 写写冲突）：
    /// 1. 已有其他事务的锁（lock.start_ts != 本事务）；
    /// 2. 存在 commit_ts > start_ts 的已提交版本（SI 写冲突）。
    pub fn prewrite(&mut self, ttl: u64) -> Result<()> {
        if self.state != TxnState::Active {
            return Err(Error::new(ErrorKind::Internal, "Prewrite 需 Active 状态"));
        }
        if self.mutations.is_empty() {
            return Ok(());
        }
        // primary = 第一个 mutation 的 key（docs/02 §4.3）。
        let primary = self.mutations[0].key.clone();
        let txn_size = self.mutations.len() as u64;

        // 阶段 1：全部冲突检查（避免部分写入）。
        for m in &self.mutations {
            if let Some(existing) = self.engine.get_lock(&m.key)? {
                if existing.start_ts != self.start_ts {
                    return Err(Error::new(
                        ErrorKind::WriteConflict,
                        format!(
                            "Prewrite 锁冲突: key={:?} 被事务 {} 锁定",
                            m.key, existing.start_ts
                        ),
                    ));
                }
            }
            if let Some(max_ts) = self.engine.max_commit_ts(&m.key)? {
                if max_ts > self.start_ts {
                    return Err(Error::new(
                        ErrorKind::WriteConflict,
                        format!(
                            "Prewrite 写冲突: key={:?} 已有更新版本 commit_ts={} > start_ts={}",
                            m.key, max_ts, self.start_ts
                        ),
                    ));
                }
            }
        }

        // 阶段 2：写 default + lock。
        for m in &self.mutations {
            if let Some(v) = &m.value {
                self.engine.raw_put(&m.key, self.start_ts, v.clone())?;
            }
            let lock = LockInfo {
                primary: primary.clone(),
                start_ts: self.start_ts,
                ttl,
                txn_size,
                lock_type: if m.value.is_some() { "Put" } else { "Delete" }.into(),
                for_update_ts: 0,
            };
            self.engine.put_lock(&m.key, &lock)?;
        }
        self.state = TxnState::Prewritten;
        Ok(())
    }

    /// Commit（docs/04 F-TXN-008）：分配 commit_ts，写 write CF，删 lock。
    pub fn commit(&mut self, commit_ts: Timestamp) -> Result<Timestamp> {
        if self.state != TxnState::Prewritten {
            return Err(Error::new(ErrorKind::Internal, "Commit 需 Prewritten 状态"));
        }
        if commit_ts <= self.start_ts {
            return Err(Error::new(
                ErrorKind::CommitTsExpired,
                "commit_ts 必须 > start_ts",
            ));
        }
        for m in &self.mutations {
            let info = if m.value.is_some() {
                WriteInfo {
                    write_type: WriteType::Put,
                    start_ts: self.start_ts,
                }
            } else {
                WriteInfo {
                    write_type: WriteType::Delete,
                    start_ts: self.start_ts,
                }
            };
            self.engine.put_write(&m.key, commit_ts, &info)?;
            self.engine.delete_lock(&m.key)?;
        }
        self.state = TxnState::Committed;
        Ok(commit_ts)
    }

    /// Rollback（docs/04 F-TXN-008）：删 lock、写 Rollback 记录防止复活。
    pub fn rollback(&mut self) -> Result<()> {
        if self.state == TxnState::Committed {
            return Err(Error::new(ErrorKind::Internal, "已提交事务不能回滚"));
        }
        for m in &self.mutations {
            self.engine.delete_lock(&m.key)?;
            self.engine.put_write(
                &m.key,
                self.start_ts,
                &WriteInfo {
                    write_type: WriteType::Rollback,
                    start_ts: self.start_ts,
                },
            )?;
            self.engine.raw_delete(&m.key, self.start_ts)?;
        }
        self.state = TxnState::RolledBack;
        Ok(())
    }
}

/// 事务管理器（BEGIN / COMMIT / ROLLBACK / 自动提交，docs/04 F-TXN-001/002）。
#[derive(Debug)]
pub struct TxnManager {
    engine: Arc<MvccEngine>,
    tso: Arc<LocalTso>,
}

impl TxnManager {
    pub fn new(engine: Arc<MvccEngine>, tso: Arc<LocalTso>) -> Self {
        Self { engine, tso }
    }

    pub fn memory() -> Self {
        Self {
            engine: Arc::new(MvccEngine::memory()),
            tso: Arc::new(LocalTso::new(1)),
        }
    }

    pub fn engine(&self) -> &Arc<MvccEngine> {
        &self.engine
    }

    pub fn begin(&self) -> Result<OptimisticTxn> {
        Ok(OptimisticTxn::begin(
            Arc::clone(&self.engine),
            self.tso.get_ts(1)?,
        ))
    }

    /// 自动提交事务（单语句）：begin → prewrite → commit。
    pub fn auto_commit(&self, mutations: Vec<Mutation>, ttl: u64) -> Result<Timestamp> {
        let mut txn = self.begin()?;
        for m in mutations {
            txn.write(m)?;
        }
        txn.prewrite(ttl)?;
        let commit_ts = self.tso.get_ts(1)?;
        txn.commit(commit_ts)
    }

    /// 快照读（自动提交只读事务，免 2PC）。
    pub fn snapshot_get(&self, key: &[u8]) -> Result<Option<Value>> {
        let ts = self.tso.get_ts(1)?;
        self.engine.get(key, ts)
    }

    /// ResolveLock（docs/04 F-TXN-012、AC-TXN-006）：
    /// 读遇锁时，查 primary 的最终状态决定 secondary 补提交或回滚。
    pub fn resolve_lock(&self, user_key: &[u8]) -> Result<ResolveOutcome> {
        let Some(lock) = self.engine.get_lock(user_key)? else {
            return Ok(ResolveOutcome::NoLock);
        };
        let primary = &lock.primary;
        // 查 primary 是否已提交：找 primary 的 write 记录中 start_ts 匹配的 Put/Delete。
        let primary_committed =
            self.engine
                .write_records(primary)?
                .into_iter()
                .find(|(_, info)| {
                    info.start_ts == lock.start_ts
                        && matches!(info.write_type, WriteType::Put | WriteType::Delete)
                });
        match primary_committed {
            Some((commit_ts, info)) => {
                // primary 已提交 → 补写 secondary 的 write 记录并删锁。
                self.engine.put_write(user_key, commit_ts, &info)?;
                self.engine.delete_lock(user_key)?;
                Ok(ResolveOutcome::Committed(commit_ts))
            }
            None => {
                // primary 未提交（回滚/超时/不存在）→ 回滚 secondary。
                self.engine.delete_lock(user_key)?;
                self.engine.put_write(
                    user_key,
                    lock.start_ts,
                    &WriteInfo {
                        write_type: WriteType::Rollback,
                        start_ts: lock.start_ts,
                    },
                )?;
                self.engine.raw_delete(user_key, lock.start_ts)?;
                Ok(ResolveOutcome::RolledBack)
            }
        }
    }

    /// 写冲突自动重试（docs/04 F-TXN-011）：最多 max_retries 次。
    pub fn run_with_retry<F>(&self, max_retries: usize, mut op: F) -> Result<Timestamp>
    where
        F: FnMut(&Self) -> Result<Timestamp>,
    {
        let mut last_err = None;
        for _ in 0..=max_retries {
            match op(self) {
                Ok(ts) => return Ok(ts),
                Err(e) if e.kind == ErrorKind::WriteConflict => last_err = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| Error::new(ErrorKind::WriteConflict, "写冲突重试耗尽")))
    }
}

/// ResolveLock 结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveOutcome {
    NoLock,
    Committed(Timestamp),
    RolledBack,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> TxnManager {
        TxnManager::memory()
    }

    #[test]
    fn tso_is_strictly_monotonic() {
        let tso = LocalTso::new(1);
        assert_eq!(tso.get_ts(1).unwrap(), 1);
        assert_eq!(tso.get_ts(3).unwrap(), 2);
        assert_eq!(tso.get_ts(1).unwrap(), 5);
    }

    #[test]
    fn auto_commit_writes_visible_at_new_snapshot() {
        let m = mgr();
        assert_eq!(m.snapshot_get(b"k").unwrap(), None);
        let ts = m
            .auto_commit(vec![Mutation::put(b"k", b"v1")], 100)
            .unwrap();
        assert_eq!(m.engine.get(b"k", ts).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(m.snapshot_get(b"k").unwrap(), Some(b"v1".to_vec()));
    }

    #[test]
    fn prewrite_commit_rollback_lifecycle() {
        let m = mgr();
        let mut t = m.begin().unwrap();
        t.write(Mutation::put(b"a", b"1")).unwrap();
        t.prewrite(100).unwrap();
        assert_eq!(t.state, TxnState::Prewritten);
        // Prewrite 阶段读会遇锁。
        assert_eq!(
            m.engine.get(b"a", t.start_ts + 1).unwrap_err().kind,
            ErrorKind::KeyIsLocked
        );
        let cts = m.tso.get_ts(1).unwrap();
        t.commit(cts).unwrap();
        assert_eq!(t.state, TxnState::Committed);
        assert_eq!(m.engine.get(b"a", cts).unwrap(), Some(b"1".to_vec()));

        let mut t2 = m.begin().unwrap();
        t2.write(Mutation::put(b"b", b"2")).unwrap();
        t2.prewrite(100).unwrap();
        t2.rollback().unwrap();
        assert_eq!(t2.state, TxnState::RolledBack);
        assert_eq!(m.engine.get(b"b", m.tso.get_ts(1).unwrap()).unwrap(), None);
    }

    #[test]
    fn write_write_conflict_detected() {
        let m = mgr();
        // T1 先开始（start_ts 小），T2 后开始但先提交 → T1 prewrite 应冲突。
        let mut t1 = m.begin().unwrap();
        let mut t2 = m.begin().unwrap();
        t1.write(Mutation::put(b"k", b"t1")).unwrap();
        t2.write(Mutation::put(b"k", b"t2")).unwrap();
        t2.prewrite(100).unwrap();
        t2.commit(m.tso.get_ts(1).unwrap()).unwrap();
        let err = t1.prewrite(100).unwrap_err();
        assert_eq!(err.kind, ErrorKind::WriteConflict);
    }

    #[test]
    fn lock_conflict_detected() {
        let m = mgr();
        let mut t1 = m.begin().unwrap();
        t1.write(Mutation::put(b"k", b"v")).unwrap();
        t1.prewrite(100).unwrap();
        let mut t2 = m.begin().unwrap();
        t2.write(Mutation::put(b"k", b"w")).unwrap();
        assert_eq!(t2.prewrite(100).unwrap_err().kind, ErrorKind::WriteConflict);
    }

    #[test]
    fn commit_ts_must_exceed_start_ts() {
        let m = mgr();
        let mut t = m.begin().unwrap();
        t.write(Mutation::put(b"k", b"v")).unwrap();
        t.prewrite(100).unwrap();
        assert_eq!(
            t.commit(t.start_ts).unwrap_err().kind,
            ErrorKind::CommitTsExpired
        );
    }

    #[test]
    fn si_snapshot_is_repeatable() {
        let m = mgr();
        m.auto_commit(vec![Mutation::put(b"k", b"v1")], 100)
            .unwrap();
        let t = m.begin().unwrap();
        assert_eq!(t.get(b"k").unwrap(), Some(b"v1".to_vec()));
        // 另一事务在 t 开始后提交新版本。
        m.auto_commit(vec![Mutation::put(b"k", b"v2")], 100)
            .unwrap();
        // t 仍读到旧版本（SI 可重复读）。
        assert_eq!(t.get(b"k").unwrap(), Some(b"v1".to_vec()));
    }

    #[test]
    fn resolve_lock_commits_secondary_when_primary_committed() {
        let m = mgr();
        let mut t = m.begin().unwrap();
        t.write(Mutation::put(b"primary", b"p")).unwrap();
        t.write(Mutation::put(b"secondary", b"s")).unwrap();
        t.prewrite(100).unwrap();
        // 模拟：primary 已提交（写 write + 删 lock），secondary 遗留锁。
        let cts = m.tso.get_ts(1).unwrap();
        m.engine
            .put_write(
                b"primary",
                cts,
                &WriteInfo {
                    write_type: WriteType::Put,
                    start_ts: t.start_ts,
                },
            )
            .unwrap();
        m.engine.delete_lock(b"primary").unwrap();
        // secondary 读遇锁。
        assert_eq!(
            m.engine.get(b"secondary", cts).unwrap_err().kind,
            ErrorKind::KeyIsLocked
        );
        let outcome = m.resolve_lock(b"secondary").unwrap();
        assert_eq!(outcome, ResolveOutcome::Committed(cts));
        assert_eq!(
            m.engine.get(b"secondary", cts).unwrap(),
            Some(b"s".to_vec())
        );
    }

    #[test]
    fn resolve_lock_rolls_back_secondary_when_primary_missing() {
        let m = mgr();
        let mut t = m.begin().unwrap();
        t.write(Mutation::put(b"primary", b"p")).unwrap();
        t.write(Mutation::put(b"secondary", b"s")).unwrap();
        t.prewrite(100).unwrap();
        // 删除 primary 锁与 write，模拟 primary 已回滚。
        m.engine.delete_lock(b"primary").unwrap();
        let outcome = m.resolve_lock(b"secondary").unwrap();
        assert_eq!(outcome, ResolveOutcome::RolledBack);
        assert!(m.engine.get_lock(b"secondary").unwrap().is_none());
    }

    #[test]
    fn resolve_lock_no_lock_is_noop() {
        let m = mgr();
        assert_eq!(m.resolve_lock(b"x").unwrap(), ResolveOutcome::NoLock);
    }

    #[test]
    fn retry_succeeds_after_transient_conflict() {
        let m = mgr();
        // 第一次 auto_commit 占用 key 并制造冲突，第二次重试开启新 start_ts 成功。
        let mut attempts = 0;
        let ts = m
            .run_with_retry(3, |mgr| {
                attempts += 1;
                if attempts == 1 {
                    mgr.auto_commit(vec![Mutation::put(b"k", b"first")], 100)?;
                    Err(Error::new(ErrorKind::WriteConflict, "模拟冲突"))
                } else {
                    mgr.auto_commit(vec![Mutation::put(b"k", b"second")], 100)
                }
            })
            .unwrap();
        assert!(attempts >= 2);
        assert_eq!(m.engine.get(b"k", ts).unwrap(), Some(b"second".to_vec()));
    }

    #[test]
    fn txn_cannot_write_after_commit() {
        let m = mgr();
        let mut t = m.begin().unwrap();
        t.write(Mutation::put(b"k", b"v")).unwrap();
        t.prewrite(100).unwrap();
        t.commit(m.tso.get_ts(1).unwrap()).unwrap();
        assert!(t.write(Mutation::put(b"j", b"w")).is_err());
    }
}
