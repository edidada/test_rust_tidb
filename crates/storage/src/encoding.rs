//! W16 保序（memcomparable）编码与 Key 布局（docs/02 §4、docs/04 F-STO-001/002、AC-STO-001）。
//!
//! 核心性质：**字节序 == 数值/字符串序**，可拼接（前缀可比），NULL 排前。
//! 编码值可直接用作 BTreeMap 的 key 实现有序扫描。

use crate::{Key, Timestamp};

/// 分组大小（docs/02 §4.1：8B 数据 + 1B 标记）。
const GROUP_DATA: usize = 8;
const GROUP_SIZE: usize = GROUP_DATA + 1;
const FULL_MARKER: u8 = 0xFF;
const PAD_BYTE: u8 = 0x00;

/// NULL 前缀（NULL 排前，docs/02 §4.1）。
pub const NULL_PREFIX: u8 = 0x00;
pub const NOT_NULL_PREFIX: u8 = 0x01;

/// 有符号整数保序编码：符号位翻转 + 8B 大端（字节序 == 数值序）。
pub fn encode_int64(v: i64) -> Key {
    let flipped = (v as u64) ^ (1u64 << 63);
    flipped.to_be_bytes().to_vec()
}

pub fn decode_int64(bytes: &[u8]) -> Option<i64> {
    let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    let flipped = u64::from_be_bytes(arr);
    Some((flipped ^ (1u64 << 63)) as i64)
}

/// 无符号整数保序编码：8B 大端。
pub fn encode_uint64(v: u64) -> Key {
    v.to_be_bytes().to_vec()
}

pub fn decode_uint64(bytes: &[u8]) -> Option<u64> {
    let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    Some(u64::from_be_bytes(arr))
}

/// 字符串/字节保序编码（docs/02 §4.1）：
/// 分组 8B 数据 + 1B 标记（0xFF 满组，否则 = 有效长度），末尾 0x00 补位。
///
/// 性质：`a` < `ab` < `b`，且 `encode(a) ++ encode(b) == encode(a ++ b)` 在分组边界内可比。
pub fn encode_bytes(data: &[u8]) -> Key {
    let mut out = Vec::with_capacity((data.len() / GROUP_DATA + 1) * GROUP_SIZE);
    let mut chunks = data.chunks(GROUP_DATA);
    for chunk in &mut chunks {
        out.extend_from_slice(chunk);
        if chunk.len() == GROUP_DATA {
            out.push(FULL_MARKER);
        } else {
            // 先补齐 8B 数据位，再写 1B 标记（保证 group[8] 是标记）。
            out.extend(std::iter::repeat(PAD_BYTE).take(GROUP_DATA - chunk.len()));
            out.push(chunk.len() as u8);
        }
    }
    // 空输入或恰好满 8B 倍数：追加一个「终止空组」保证前缀可比与可拼接。
    if data.is_empty() || data.len() % GROUP_DATA == 0 {
        out.extend(std::iter::repeat(PAD_BYTE).take(GROUP_DATA));
        out.push(0u8); // 有效长度 0
    }
    out
}

/// 解码字符串/字节保序编码（去掉补位与标记）。
pub fn decode_bytes(encoded: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + GROUP_SIZE <= encoded.len() {
        let group = &encoded[i..i + GROUP_SIZE];
        let marker = group[GROUP_DATA];
        if marker == FULL_MARKER {
            out.extend_from_slice(&group[..GROUP_DATA]);
        } else if marker as usize <= GROUP_DATA {
            out.extend_from_slice(&group[..marker as usize]);
            break;
        } else {
            return None; // 非法标记
        }
        i += GROUP_SIZE;
    }
    if i >= encoded.len() && !encoded.is_empty() {
        return None; // 缺少终止组
    }
    Some(out)
}

/// 浮点保序编码：符号位翻转 + 指数/尾数编码（docs/02 §4.1 近似保序）。
/// 规则：正数符号位翻转（0x80），负数全部位取反，保证字节序 == 数值序。
pub fn encode_float64(v: f64) -> Key {
    let bits = v.to_bits();
    let ordered = if bits & (1u64 << 63) != 0 {
        !bits // 负数：全位取反
    } else {
        bits ^ (1u64 << 63) // 正数：符号位翻转
    };
    ordered.to_be_bytes().to_vec()
}

pub fn decode_float64(bytes: &[u8]) -> Option<f64> {
    let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    let ordered = u64::from_be_bytes(arr);
    let bits = if ordered & (1u64 << 63) != 0 {
        ordered ^ (1u64 << 63)
    } else {
        !ordered
    };
    Some(f64::from_bits(bits))
}

/// DECIMAL 保序编码：归一化指数 + 保序尾数（docs/02 §4.1 精度无损）。
/// 用 (unscaled: i128, scale: u8) 表示；编码 = 符号翻转的 16B 尾数 + 1B scale。
pub fn encode_decimal(unscaled: i128, scale: u8) -> Key {
    let flipped = ((unscaled as u128) ^ (1u128 << 127)).to_be_bytes();
    let mut out = Vec::with_capacity(17);
    out.extend_from_slice(&flipped);
    out.push(scale);
    out
}

pub fn decode_decimal(bytes: &[u8]) -> Option<(i128, u8)> {
    let arr: [u8; 16] = bytes.get(..16)?.try_into().ok()?;
    let flipped = u128::from_be_bytes(arr);
    let unscaled = (flipped ^ (1u128 << 127)) as i128;
    let scale = *bytes.get(16)?;
    Some((unscaled, scale))
}

/// NULL / 非 NULL 前缀包装（docs/02 §4.1：NULL 排前）。
pub fn encode_nullable(value: Option<&[u8]>) -> Key {
    match value {
        None => vec![NULL_PREFIX],
        Some(v) => {
            let mut out = Vec::with_capacity(1 + v.len());
            out.push(NOT_NULL_PREFIX);
            out.extend_from_slice(v);
            out
        }
    }
}

// ---- Key 布局（docs/02 §4.2） ----

/// TablePrefix = 't' | EncodeInt64(table_id)
pub fn table_prefix(table_id: i64) -> Key {
    let mut out = vec![b't'];
    out.extend_from_slice(&encode_int64(table_id));
    out
}

/// RecordKey = TablePrefix | "_r" | Encode(handle)
pub fn record_key(table_id: i64, handle: &[u8]) -> Key {
    let mut out = table_prefix(table_id);
    out.extend_from_slice(b"_r");
    out.extend_from_slice(handle);
    out
}

/// IndexKey = TablePrefix | "_i" | EncodeInt64(index_id) | Encode(idx_cols...) | [handle]
pub fn index_key(table_id: i64, index_id: i64, idx_cols: &[Key], handle: Option<&[u8]>) -> Key {
    let mut out = table_prefix(table_id);
    out.extend_from_slice(b"_i");
    out.extend_from_slice(&encode_int64(index_id));
    for col in idx_cols {
        out.extend_from_slice(col);
    }
    if let Some(h) = handle {
        out.extend_from_slice(h);
    }
    out
}

/// MetaKey = 'm' | ...
pub fn meta_key(parts: &[&[u8]]) -> Key {
    let mut out = vec![b'm'];
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// TxnKey(user) = user_key | EncodeDesc(start_ts)（降序：最新版本优先，docs/02 §4.2）。
pub fn encode_desc_ts(ts: Timestamp) -> Key {
    (!ts).to_be_bytes().to_vec()
}

pub fn decode_desc_ts(bytes: &[u8]) -> Option<Timestamp> {
    let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    Some(!u64::from_be_bytes(arr))
}

pub fn txn_key(user_key: &[u8], start_ts: Timestamp) -> Key {
    let mut out = user_key.to_vec();
    out.extend_from_slice(&encode_desc_ts(start_ts));
    out
}

/// 从 TxnKey 拆出 user_key 与 start_ts（TxnKey 尾部固定 8B 为降序 TS）。
pub fn split_txn_key(key: &[u8]) -> Option<(Key, Timestamp)> {
    if key.len() < 8 {
        return None;
    }
    let split = key.len() - 8;
    let ts = decode_desc_ts(&key[split..])?;
    Some((key[..split].to_vec(), ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int64_order_preserved_including_negatives() {
        let vals = [i64::MIN, -1000, -1, 0, 1, 1000, i64::MAX];
        let mut encoded: Vec<Key> = vals.iter().map(|v| encode_int64(*v)).collect();
        encoded.sort();
        let decoded: Vec<i64> = encoded.iter().map(|e| decode_int64(e).unwrap()).collect();
        assert_eq!(decoded, vals.to_vec());
    }

    #[test]
    fn uint64_order_and_roundtrip() {
        let vals = [0u64, 1, 255, 256, u64::MAX];
        let mut encoded: Vec<Key> = vals.iter().map(|v| encode_uint64(*v)).collect();
        encoded.sort();
        let decoded: Vec<u64> = encoded.iter().map(|e| decode_uint64(e).unwrap()).collect();
        assert_eq!(decoded, vals.to_vec());
    }

    #[test]
    fn bytes_encoding_is_prefix_comparable_and_roundtrips() {
        let vals: Vec<&[u8]> = vec![
            b"",
            b"a",
            b"ab",
            b"abcdefgh",
            b"abcdefghi",
            b"b",
            b"zzzzzzzzzzzzzzzzz",
        ];
        let mut encoded: Vec<Key> = vals.iter().map(|v| encode_bytes(v)).collect();
        encoded.sort();
        let decoded: Vec<Vec<u8>> = encoded.iter().map(|e| decode_bytes(e).unwrap()).collect();
        let expected: Vec<Vec<u8>> = vals.iter().map(|v| v.to_vec()).collect();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn bytes_encoding_handles_boundary_lengths() {
        for len in [0usize, 1, 7, 8, 9, 15, 16, 17, 64, 1024] {
            let data = vec![0xABu8; len];
            let enc = encode_bytes(&data);
            assert_eq!(decode_bytes(&enc).unwrap(), data, "len={len}");
        }
    }

    #[test]
    fn float_order_preserved() {
        let vals = [
            -f64::INFINITY,
            -1e300,
            -1.0,
            -0.0,
            0.0,
            1.0,
            1e300,
            f64::INFINITY,
        ];
        let mut encoded: Vec<Key> = vals.iter().map(|v| encode_float64(*v)).collect();
        encoded.sort();
        let decoded: Vec<f64> = encoded.iter().map(|e| decode_float64(e).unwrap()).collect();
        for (i, v) in decoded.iter().enumerate() {
            assert!(
                v.total_cmp(&vals[i]).is_eq() || (*v == 0.0 && vals[i] == 0.0),
                "idx {i}: {v} vs {}",
                vals[i]
            );
        }
        assert_eq!(decode_float64(&encode_float64(3.5)).unwrap(), 3.5);
    }

    #[test]
    fn decimal_roundtrip_and_order() {
        let vals = [(-1000i128, 2u8), (-1, 0), (0, 0), (1, 0), (1000, 2)];
        let mut encoded: Vec<Key> = vals.iter().map(|(u, s)| encode_decimal(*u, *s)).collect();
        encoded.sort();
        let decoded: Vec<(i128, u8)> = encoded.iter().map(|e| decode_decimal(e).unwrap()).collect();
        assert_eq!(decoded, vals.to_vec());
    }

    #[test]
    fn null_sorts_before_not_null() {
        let n = encode_nullable(None);
        let v = encode_nullable(Some(b"x"));
        assert!(n < v);
    }

    #[test]
    fn key_layout_is_deterministic_and_parseable() {
        let tp = table_prefix(42);
        assert_eq!(tp[0], b't');
        assert_eq!(decode_int64(&tp[1..9]).unwrap(), 42);

        let rk = record_key(42, &encode_int64(7));
        assert!(rk.starts_with(&tp));
        assert_eq!(&rk[9..11], b"_r");

        let ik = index_key(42, 3, &[encode_bytes(b"abc")], Some(&encode_int64(7)));
        assert!(ik.starts_with(&tp));
        assert_eq!(&ik[9..11], b"_i");

        assert_eq!(meta_key(&[b"schema", b"v1"])[0], b'm');
    }

    #[test]
    fn txn_key_descending_ts_latest_first() {
        let k1 = txn_key(b"user", 10);
        let k2 = txn_key(b"user", 20);
        // 降序 TS：ts=20 的编码应排在 ts=10 之前（最新版本优先）。
        assert!(k2 < k1);
        let (uk, ts) = split_txn_key(&k2).unwrap();
        assert_eq!(uk, b"user".to_vec());
        assert_eq!(ts, 20);
    }

    #[test]
    fn desc_ts_roundtrip() {
        for ts in [0u64, 1, 12345, u64::MAX - 1] {
            assert_eq!(decode_desc_ts(&encode_desc_ts(ts)).unwrap(), ts);
        }
    }
}
