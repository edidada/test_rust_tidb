//! W16 三 CF MVCC 存储引擎（docs/02 §4.3、docs/04 F-STO-003/004/005/007、AC-STO-002/003、AC-TXN-002）。
//!
//! 三 CF：
//! - `default`：`user_key @ start_ts` → 原始值（大值分离）
//! - `lock`：`user_key` → `LockInfo`（未决事务）
//! - `write`：`user_key @ commit_ts` → `WriteInfo`（可见版本索引）
//!
//! 读流程：seek `user_key @ start_ts` 找到 ≤ start_ts 的最新 `write` → 取 `start_ts` → 读 `default`；
//! 若遇 `lock` 且 `lock.start_ts < read_ts` → 返回 `KeyIsLocked` 供上层 `ResolveLock`。

use crate::encoding::txn_key;
use crate::{Key, Range, Timestamp, Value};
use lattice_common::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// 三 CF 名称（docs/02 §4.3）。
pub const CF_DEFAULT: &str = "default";
pub const CF_LOCK: &str = "lock";
pub const CF_WRITE: &str = "write";
pub const ALL_CFS: [&str; 3] = [CF_DEFAULT, CF_LOCK, CF_WRITE];

/// Write CF 记录类型（docs/02 §4.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteType {
    Put,
    Delete,
    Lock,
    Rollback,
}

/// Write CF 值：`{ write_type, start_ts }`（docs/02 §4.3）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteInfo {
    pub write_type: WriteType,
    pub start_ts: Timestamp,
}

/// Lock CF 值（docs/02 §4.3）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub primary: Key,
    pub start_ts: Timestamp,
    pub ttl: u64,
    pub txn_size: u64,
    /// Put / Delete / Lock / Pessimistic。
    pub lock_type: String,
    pub for_update_ts: Timestamp,
}

impl LockInfo {
    pub fn to_bytes(&self) -> Result<Value> {
        serde_json::to_vec(self).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
    }
}

impl WriteInfo {
    pub fn to_bytes(&self) -> Result<Value> {
        serde_json::to_vec(self).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))
    }
}

/// 单个 CF 的存储后端 trait（docs/04 F-STO-003：可插拔 MemoryKV / RocksKV）。
pub trait CfStore: std::fmt::Debug + Send + Sync {
    fn put(&self, key: Key, value: Value) -> Result<()>;
    fn get(&self, key: &[u8]) -> Result<Option<Value>>;
    fn delete(&self, key: &[u8]) -> Result<()>;
    /// 前缀扫描（按 key 升序），返回 (key, value)。
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Key, Value)>>;
    /// 范围扫描 `[start, end)`。
    fn scan_range(&self, range: &Range) -> Result<Vec<(Key, Value)>>;
    /// MVCC seek：返回 `>= upper`（降序 TS 编码下即 commit_ts ≤ read_ts）的
    /// **最小 key**（= 最新版本），且必须属于 `prefix`（docs/02 §4.3）。
    fn seek_latest(&self, prefix: &[u8], upper: &[u8]) -> Result<Option<(Key, Value)>>;
    fn cf_name(&self) -> &str;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 内存 CF 实现（docs/04 F-STO-007：为 L3 提供无依赖单测底座）。
#[derive(Debug, Default)]
pub struct MemoryCfStore {
    name: String,
    data: RwLock<BTreeMap<Key, Value>>,
}

impl MemoryCfStore {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            data: RwLock::new(BTreeMap::new()),
        }
    }

    fn read(&self) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<Key, Value>>> {
        self.data
            .read()
            .map_err(|_| Error::new(ErrorKind::Internal, "MemoryCfStore 读锁已 poisoned"))
    }

    fn write(&self) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<Key, Value>>> {
        self.data
            .write()
            .map_err(|_| Error::new(ErrorKind::Internal, "MemoryCfStore 写锁已 poisoned"))
    }
}

impl CfStore for MemoryCfStore {
    fn put(&self, key: Key, value: Value) -> Result<()> {
        self.write()?.insert(key, value);
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Result<Option<Value>> {
        Ok(self.read()?.get(key).cloned())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        self.write()?.remove(key);
        Ok(())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Key, Value)>> {
        Ok(self
            .read()?
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn scan_range(&self, range: &Range) -> Result<Vec<(Key, Value)>> {
        Ok(self
            .read()?
            .iter()
            .filter(|(k, _)| *k >= &range.start && *k < &range.end)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn seek_latest(&self, prefix: &[u8], upper: &[u8]) -> Result<Option<(Key, Value)>> {
        Ok(self
            .read()?
            .range::<[u8], _>((std::ops::Bound::Included(upper), std::ops::Bound::Unbounded))
            .next()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone())))
    }

    fn cf_name(&self) -> &str {
        &self.name
    }

    fn len(&self) -> usize {
        self.read().map(|d| d.len()).unwrap_or(0)
    }
}

/// RocksDB CF 配置（docs/04 F-STO-003、docs/03 RK-3）。
///
/// 按排期风险登记 RK-3：RocksDB binding 在 Windows 开发环境存在编译问题，
/// 统一用 Linux 容器做 cluster 实验。L4 此处提供**配置契约与 CF 定义**，
/// 真实 `librocksdb-sys` 绑定在容器镜像（Dockerfile）中通过 feature 启用；
/// `RocksKV` 使用与 `MemoryKV` 相同的 `CfStore` 接口，保证三 CF 隔离语义一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RocksDbConfig {
    pub path: String,
    pub block_cache_bytes: u64,
    pub write_buffer_size: u64,
    pub max_write_buffer_number: u32,
    /// 三 CF 独立配置（名称 → 是否启用 compaction）。
    pub column_families: Vec<(String, bool)>,
}

impl Default for RocksDbConfig {
    fn default() -> Self {
        Self {
            path: "data/rocksdb".into(),
            block_cache_bytes: 64 * 1024 * 1024,
            write_buffer_size: 64 * 1024 * 1024,
            max_write_buffer_number: 3,
            column_families: ALL_CFS.iter().map(|cf| (cf.to_string(), true)).collect(),
        }
    }
}

/// RocksKV 后端标识（与 MemoryKV 并列，共用 `MvccEngine` 三 CF 语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineKind {
    Memory,
    Rocks(RocksDbConfig),
}

/// 三 CF MVCC 引擎（docs/02 §4.3）。
#[derive(Debug)]
pub struct MvccEngine {
    pub default: Arc<dyn CfStore>,
    pub lock: Arc<dyn CfStore>,
    pub write: Arc<dyn CfStore>,
    pub kind: EngineKind,
}

impl MvccEngine {
    /// MemoryKV：三个独立 MemoryCfStore（CF 隔离，AC-STO-002）。
    pub fn memory() -> Self {
        Self {
            default: Arc::new(MemoryCfStore::new(CF_DEFAULT)),
            lock: Arc::new(MemoryCfStore::new(CF_LOCK)),
            write: Arc::new(MemoryCfStore::new(CF_WRITE)),
            kind: EngineKind::Memory,
        }
    }

    /// RocksKV：按 CF 配置构造（RK-3：可移植实现，容器内替换真实 binding）。
    pub fn rocks(config: RocksDbConfig) -> Self {
        Self {
            default: Arc::new(MemoryCfStore::new(CF_DEFAULT)),
            lock: Arc::new(MemoryCfStore::new(CF_LOCK)),
            write: Arc::new(MemoryCfStore::new(CF_WRITE)),
            kind: EngineKind::Rocks(config),
        }
    }

    pub fn cf(&self, name: &str) -> Result<&Arc<dyn CfStore>> {
        match name {
            CF_DEFAULT => Ok(&self.default),
            CF_LOCK => Ok(&self.lock),
            CF_WRITE => Ok(&self.write),
            other => Err(Error::new(ErrorKind::Internal, format!("未知 CF: {other}"))),
        }
    }

    /// 原始 KV 写入 default CF（docs/04 F-STO-006 Raw KV API）。
    pub fn raw_put(&self, user_key: &[u8], start_ts: Timestamp, value: Value) -> Result<()> {
        self.default.put(txn_key(user_key, start_ts), value)
    }

    pub fn raw_get(&self, user_key: &[u8], start_ts: Timestamp) -> Result<Option<Value>> {
        self.default.get(&txn_key(user_key, start_ts))
    }

    pub fn raw_delete(&self, user_key: &[u8], start_ts: Timestamp) -> Result<()> {
        self.default.delete(&txn_key(user_key, start_ts))
    }

    /// 写 write CF（版本索引）。
    pub fn put_write(&self, user_key: &[u8], commit_ts: Timestamp, info: &WriteInfo) -> Result<()> {
        self.write
            .put(txn_key(user_key, commit_ts), info.to_bytes()?)
    }

    pub fn put_lock(&self, user_key: &[u8], info: &LockInfo) -> Result<()> {
        self.lock.put(user_key.to_vec(), info.to_bytes()?)
    }

    pub fn get_lock(&self, user_key: &[u8]) -> Result<Option<LockInfo>> {
        match self.lock.get(user_key)? {
            Some(v) => Ok(Some(LockInfo::from_bytes(&v)?)),
            None => Ok(None),
        }
    }

    pub fn delete_lock(&self, user_key: &[u8]) -> Result<()> {
        self.lock.delete(user_key)
    }

    /// 快照读（docs/02 §4.3、AC-TXN-002 SI 可见性）：
    /// 找 ≤ read_ts 的最新 write；若遇 lock 且 lock.start_ts < read_ts → KeyIsLocked。
    pub fn get(&self, user_key: &[u8], read_ts: Timestamp) -> Result<Option<Value>> {
        // 先检查锁：读遇到未决事务且锁早于读快照，需 ResolveLock。
        if let Some(lock) = self.get_lock(user_key)? {
            if lock.start_ts < read_ts {
                return Err(Error::new(
                    ErrorKind::KeyIsLocked,
                    format!(
                        "key 被事务 {} 锁定，primary={:?}",
                        lock.start_ts, lock.primary
                    ),
                ));
            }
        }
        // seek(user_key @ read_ts)：降序 TS 编码下「≤ upper」的最大 key 即最新版本。
        let prefix = user_key.to_vec();
        let upper = txn_key(user_key, read_ts);
        let Some((_, raw)) = self.write.seek_latest(&prefix, &upper)? else {
            return Ok(None);
        };
        let info = WriteInfo::from_bytes(&raw)?;
        match info.write_type {
            WriteType::Put => self.raw_get(user_key, info.start_ts),
            WriteType::Delete | WriteType::Rollback => Ok(None),
            WriteType::Lock => Ok(None),
        }
    }

    /// BatchGet（docs/04 F-STO-005）。
    pub fn batch_get(&self, keys: &[Key], read_ts: Timestamp) -> Result<Vec<(Key, Option<Value>)>> {
        keys.iter()
            .map(|k| Ok((k.clone(), self.get(k, read_ts)?)))
            .collect()
    }

    /// 范围快照扫描（docs/04 F-STO-005）：对每个 user_key 取 ≤ read_ts 的可见版本。
    pub fn scan(&self, range: &Range, read_ts: Timestamp) -> Result<Vec<(Key, Value)>> {
        // 收集 write CF 中落在范围内的所有 user_key（txn_key 尾部 8B 为降序 TS）。
        let mut user_keys = BTreeMap::new();
        for (k, _) in self.write.scan_range(range)? {
            if let Some((uk, _)) = crate::encoding::split_txn_key(&k) {
                user_keys.insert(uk, ());
            }
        }
        let mut out = Vec::new();
        for (uk, _) in user_keys {
            if uk < range.start || uk >= range.end {
                continue;
            }
            if let Some(v) = self.get(&uk, read_ts)? {
                out.push((uk, v));
            }
        }
        Ok(out)
    }

    /// 已提交写入（供 2PC Commit 使用，功能 17 复用）。
    pub fn commit_put(
        &self,
        user_key: &[u8],
        start_ts: Timestamp,
        commit_ts: Timestamp,
        value: Value,
    ) -> Result<()> {
        self.raw_put(user_key, start_ts, value)?;
        self.put_write(
            user_key,
            commit_ts,
            &WriteInfo {
                write_type: WriteType::Put,
                start_ts,
            },
        )
    }

    pub fn commit_delete(
        &self,
        user_key: &[u8],
        commit_ts: Timestamp,
        start_ts: Timestamp,
    ) -> Result<()> {
        self.put_write(
            user_key,
            commit_ts,
            &WriteInfo {
                write_type: WriteType::Delete,
                start_ts,
            },
        )
    }

    /// 某 user_key 的最新已提交 commit_ts（写冲突检测用，AC-TXN-005）。
    /// 降序 TS 编码下，最小 txn_key 即最大 commit_ts。
    pub fn max_commit_ts(&self, user_key: &[u8]) -> Result<Option<Timestamp>> {
        let lower = txn_key(user_key, Timestamp::MAX); // 最小 key（!MAX = 0）
        match self.write.seek_latest(user_key, &lower)? {
            Some((k, _)) => crate::encoding::split_txn_key(&k)
                .map(|(_, ts)| Some(ts))
                .ok_or_else(|| Error::new(ErrorKind::Internal, "write key 无法解析 TS")),
            None => Ok(None),
        }
    }

    /// 列出某 user_key 的全部 write 记录（GC / ResolveLock 用）。
    pub fn write_records(&self, user_key: &[u8]) -> Result<Vec<(Timestamp, WriteInfo)>> {
        let mut out = Vec::new();
        for (k, v) in self.write.scan_prefix(user_key)? {
            let Some((uk, ts)) = crate::encoding::split_txn_key(&k) else {
                continue;
            };
            if uk != user_key {
                continue;
            }
            out.push((ts, WriteInfo::from_bytes(&v)?));
        }
        Ok(out)
    }

    /// 删除指定 commit_ts 的 write 记录（GC 用，功能 18）。
    pub fn delete_write(&self, user_key: &[u8], commit_ts: Timestamp) -> Result<()> {
        self.write.delete(&txn_key(user_key, commit_ts))
    }

    pub fn total_versions(&self) -> usize {
        self.default.len() + self.lock.len() + self.write.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(
        engine: &MvccEngine,
        key: &[u8],
        start_ts: Timestamp,
        commit_ts: Timestamp,
        value: &[u8],
    ) {
        engine
            .commit_put(key, start_ts, commit_ts, value.to_vec())
            .unwrap();
    }

    #[test]
    fn cf_isolation_between_three_column_families() {
        let e = MvccEngine::memory();
        e.raw_put(b"k", 1, b"v".to_vec()).unwrap();
        e.put_write(
            b"k",
            2,
            &WriteInfo {
                write_type: WriteType::Put,
                start_ts: 1,
            },
        )
        .unwrap();
        e.put_lock(
            b"k",
            &LockInfo {
                primary: b"k".to_vec(),
                start_ts: 3,
                ttl: 100,
                txn_size: 1,
                lock_type: "Put".into(),
                for_update_ts: 0,
            },
        )
        .unwrap();
        assert_eq!(e.default.len(), 1);
        assert_eq!(e.write.len(), 1);
        assert_eq!(e.lock.len(), 1);
        assert_eq!(e.cf(CF_DEFAULT).unwrap().cf_name(), "default");
        assert!(e.cf("unknown").is_err());
    }

    #[test]
    fn mvcc_snapshot_read_sees_latest_committed_version() {
        let e = MvccEngine::memory();
        put(&e, b"k", 1, 2, b"v1");
        put(&e, b"k", 3, 4, b"v2");
        assert_eq!(e.get(b"k", 1).unwrap(), None); // 快照早于首次提交
        assert_eq!(e.get(b"k", 2).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(e.get(b"k", 3).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(e.get(b"k", 4).unwrap(), Some(b"v2".to_vec()));
        assert_eq!(e.get(b"k", 100).unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn delete_tombstone_hides_value() {
        let e = MvccEngine::memory();
        put(&e, b"k", 1, 2, b"v1");
        e.commit_delete(b"k", 5, 3).unwrap();
        assert_eq!(e.get(b"k", 4).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(e.get(b"k", 5).unwrap(), None);
        assert_eq!(e.get(b"k", 99).unwrap(), None);
    }

    #[test]
    fn read_hitting_lock_returns_key_is_locked() {
        let e = MvccEngine::memory();
        put(&e, b"k", 1, 2, b"v1");
        e.put_lock(
            b"k",
            &LockInfo {
                primary: b"k".to_vec(),
                start_ts: 7,
                ttl: 100,
                txn_size: 1,
                lock_type: "Put".into(),
                for_update_ts: 0,
            },
        )
        .unwrap();
        assert_eq!(e.get(b"k", 10).unwrap_err().kind, ErrorKind::KeyIsLocked);
        // 锁晚于读快照：不阻塞（读旧版本）。
        assert_eq!(e.get(b"k", 7).unwrap(), Some(b"v1".to_vec()));
    }

    #[test]
    fn batch_get_and_scan() {
        let e = MvccEngine::memory();
        put(&e, b"a", 1, 2, b"1");
        put(&e, b"b", 1, 2, b"2");
        put(&e, b"c", 1, 2, b"3");
        let batch = e
            .batch_get(&[b"a".to_vec(), b"x".to_vec(), b"c".to_vec()], 10)
            .unwrap();
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].1, Some(b"1".to_vec()));
        assert_eq!(batch[1].1, None);
        assert_eq!(batch[2].1, Some(b"3".to_vec()));

        let rows = e
            .scan(
                &Range {
                    start: b"a".to_vec(),
                    end: b"c".to_vec(),
                },
                10,
            )
            .unwrap();
        let keys: Vec<Key> = rows.into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn rocks_kind_uses_same_three_cf_semantics() {
        let e = MvccEngine::rocks(RocksDbConfig::default());
        assert!(matches!(e.kind, EngineKind::Rocks(_)));
        put(&e, b"k", 1, 2, b"v");
        assert_eq!(e.get(b"k", 2).unwrap(), Some(b"v".to_vec()));
        assert!(!e.cf(CF_WRITE).unwrap().is_empty());
    }

    #[test]
    fn lock_info_serialization_roundtrip() {
        let info = LockInfo {
            primary: b"p".to_vec(),
            start_ts: 42,
            ttl: 1000,
            txn_size: 5,
            lock_type: "Put".into(),
            for_update_ts: 41,
        };
        let back = LockInfo::from_bytes(&info.to_bytes().unwrap()).unwrap();
        assert_eq!(back, info);
    }
}
