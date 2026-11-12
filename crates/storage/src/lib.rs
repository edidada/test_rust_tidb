//! `storage` crate (L4 单机 KV、MVCC 与事务)。
//! 契约来源：docs/02 §3 `KvEngine`、§4 KV 编码；docs/03 L4。
//!
//! 模块：`encoding`（保序编码/Key 布局）、`engine`（三 CF MVCC）、`txn`（2PC）。

pub mod encoding;
pub mod engine;

pub use encoding::{
    decode_bytes, decode_decimal, decode_desc_ts, decode_float64, decode_int64, decode_uint64,
    encode_bytes, encode_decimal, encode_desc_ts, encode_float64, encode_int64, encode_nullable,
    encode_uint64, index_key, meta_key, record_key, split_txn_key, table_prefix, txn_key,
    NOT_NULL_PREFIX, NULL_PREFIX,
};
pub use engine::{
    CfStore, EngineKind, LockInfo, MemoryCfStore, MvccEngine, RocksDbConfig, WriteInfo, WriteType,
    ALL_CFS, CF_DEFAULT, CF_LOCK, CF_WRITE,
};

use lattice_common::Result;
use std::collections::BTreeMap;
use std::sync::RwLock;

pub type Timestamp = u64;
pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

/// 简单范围（docs/02 §3 `scan` 参数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub start: Key,
    pub end: Key,
}

/// KV 引擎 trait（docs/02 §3）。
pub trait KvEngine {
    fn get(&self, key: &[u8], ts: Timestamp) -> Result<Option<Value>>;
    fn put(&self, key: Key, value: Value, ts: Timestamp) -> Result<()>;
    fn scan(&self, range: Range, ts: Timestamp) -> Result<Vec<(Key, Value)>>;
}

/// `MemoryKV`（docs/03 L4：为 L3 提供无依赖单测底座）。
/// 以 `(key, ts)` 保存多版本，快照读取取 `ts` 及之前的最新版本。
#[derive(Debug, Default)]
pub struct MemoryKV {
    versions: RwLock<BTreeMap<(Key, Timestamp), Value>>,
}

impl MemoryKV {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KvEngine for MemoryKV {
    fn get(&self, key: &[u8], ts: Timestamp) -> Result<Option<Value>> {
        let versions = self
            .versions
            .read()
            .map_err(|_| lattice_common::internal("MemoryKV 读锁已 poisoned"))?;
        Ok(versions
            .range((key.to_vec(), 0u64)..=(key.to_vec(), ts))
            .next_back()
            .map(|(_, v)| v.clone()))
    }

    fn put(&self, key: Key, value: Value, ts: Timestamp) -> Result<()> {
        let mut versions = self
            .versions
            .write()
            .map_err(|_| lattice_common::internal("MemoryKV 写锁已 poisoned"))?;
        versions.insert((key, ts), value);
        Ok(())
    }

    fn scan(&self, range: Range, ts: Timestamp) -> Result<Vec<(Key, Value)>> {
        let versions = self
            .versions
            .read()
            .map_err(|_| lattice_common::internal("MemoryKV 读锁已 poisoned"))?;
        let mut out = BTreeMap::new();
        for ((k, t), v) in versions.iter() {
            if *t <= ts && *k >= range.start && *k < range.end {
                out.insert(k.clone(), v.clone());
            }
        }
        Ok(out.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_kv_snapshot_read() {
        let kv = MemoryKV::new();
        kv.put(b"a".to_vec(), b"v1".to_vec(), 1).unwrap();
        kv.put(b"a".to_vec(), b"v2".to_vec(), 3).unwrap();
        assert_eq!(kv.get(b"a", 1).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(kv.get(b"a", 2).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(kv.get(b"a", 3).unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn memory_kv_scan_respects_range() {
        let kv = MemoryKV::new();
        kv.put(b"a".to_vec(), b"1".to_vec(), 1).unwrap();
        kv.put(b"b".to_vec(), b"2".to_vec(), 1).unwrap();
        kv.put(b"c".to_vec(), b"3".to_vec(), 1).unwrap();
        let rows = kv
            .scan(
                Range {
                    start: b"a".to_vec(),
                    end: b"c".to_vec(),
                },
                10,
            )
            .unwrap();
        let keys: Vec<Vec<u8>> = rows.into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);
    }
}
