//! `Column` / `Block` 列式读写（docs/02 §2.1、docs/03 L1 `DataType/Scalar/Block/Column`）。

use crate::datatype::DataType;
use crate::scalar::Scalar;
use lattice_common::{BlockBase, BlockBuilder, Buffer, Result, SelectionVector, Validity};

/// 列：类型 + 有效性位图 + 值缓冲区（docs/02 §2.1 `Column`）。
///
/// 定长数值采用「大端定宽」编码；变长值采用「4B 长度 + 原始字节」追加。
/// NULL 值在 buffer 中占位 0（保持定宽列可直接按 row*width 寻址）。
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub data_type: DataType,
    pub validity: Validity,
    pub buffer: Buffer,
}

impl Column {
    /// 预置 `rows` 行（全部有效，值为零占位），用于骨架/占位块。
    pub fn new(data_type: DataType, rows: usize) -> Self {
        Self {
            data_type,
            validity: Validity::new(rows, true),
            buffer: Buffer::new(),
        }
    }

    /// 空列；后续用 [`Column::push`] 逐行追加。
    pub fn empty(data_type: DataType) -> Self {
        Self::new(data_type, 0)
    }

    pub fn len(&self) -> usize {
        self.validity.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn null_count(&self) -> usize {
        self.validity.null_count()
    }

    /// 追加一个标量值；类型不匹配返回 `TypeMismatch`（docs/02 §13）。
    /// 若列是 `new(type, rows)` 预置的占位列，首次 `push` 会清空占位，从空列开始。
    pub fn push(&mut self, value: &Scalar) -> Result<()> {
        if self.buffer.is_empty() && !self.validity.is_empty() {
            self.validity = Validity::new(0, true);
        }
        if !value.castable_to(&self.data_type) && !matches!(value, Scalar::Null) {
            return Err(lattice_common::Error::new(
                lattice_common::ErrorKind::TypeMismatch,
                format!("{:?} 不能写入 {:?} 列", value, self.data_type),
            ));
        }
        if matches!(value, Scalar::Null) {
            self.validity.push(false);
            if let Some(width) = self.data_type.fixed_width() {
                self.buffer.bytes.extend(std::iter::repeat(0u8).take(width));
            } else {
                self.buffer.bytes.extend_from_slice(&0u32.to_be_bytes());
            }
            return Ok(());
        }
        self.validity.push(true);
        self.encode_value(value)
    }

    fn encode_value(&mut self, value: &Scalar) -> Result<()> {
        if self.data_type == DataType::Bool {
            let Scalar::Bool(b) = value else {
                return Err(lattice_common::Error::new(
                    lattice_common::ErrorKind::TypeMismatch,
                    "BOOL 列仅支持布尔标量",
                ));
            };
            self.buffer.bytes.push(u8::from(*b));
            return Ok(());
        }
        if let Some(width) = self.data_type.fixed_width() {
            let Scalar::Int(v) = value else {
                return Err(lattice_common::Error::new(
                    lattice_common::ErrorKind::TypeMismatch,
                    "定宽列仅支持整数标量",
                ));
            };
            let bytes = v.to_be_bytes();
            self.buffer.bytes.extend_from_slice(&bytes[16 - width..]);
            return Ok(());
        }
        let raw: Vec<u8> = match value {
            Scalar::Bool(b) => vec![u8::from(*b)],
            Scalar::Float(f) => f.to_be_bytes().to_vec(),
            Scalar::Decimal { unscaled, scale } => {
                let mut out = unscaled.to_be_bytes().to_vec();
                out.push(*scale);
                out
            }
            Scalar::Bytes(b) => b.clone(),
            Scalar::Str(s) => s.as_bytes().to_vec(),
            Scalar::Temporal(t) => t.to_be_bytes().to_vec(),
            Scalar::Json(j) => j.as_bytes().to_vec(),
            Scalar::Int(i) => i.to_be_bytes().to_vec(),
            Scalar::Null => unreachable!("NULL handled by push"),
        };
        let len = u32::try_from(raw.len()).map_err(|_| {
            lattice_common::Error::new(lattice_common::ErrorKind::Internal, "变长值超过 4GiB")
        })?;
        self.buffer.bytes.extend_from_slice(&len.to_be_bytes());
        self.buffer.bytes.extend_from_slice(&raw);
        Ok(())
    }

    /// 读取第 `row` 行的值；NULL 行返回 `Scalar::Null`。
    pub fn get(&self, row: usize) -> Result<Scalar> {
        if row >= self.len() {
            return Err(lattice_common::Error::new(
                lattice_common::ErrorKind::Internal,
                format!("行号 {row} 越界 (len={})", self.len()),
            ));
        }
        if !self.validity.is_valid(row) {
            return Ok(Scalar::Null);
        }
        if self.data_type == DataType::Bool {
            return Ok(Scalar::Bool(
                self.buffer.bytes.get(row).copied().unwrap_or(0) != 0,
            ));
        }
        if let Some(width) = self.data_type.fixed_width() {
            let start = row * width;
            let slice = &self.buffer.bytes[start..start + width];
            let mut be = [0u8; 16];
            be[16 - width..].copy_from_slice(slice);
            let negative = matches!(
                self.data_type,
                DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64
            ) && (be[16 - width] & 0x80 != 0);
            if negative {
                be[..16 - width].fill(0xFF);
            }
            return Ok(Scalar::Int(i128::from_be_bytes(be)));
        }
        // 变长：顺序扫描长度前缀（L0/L1 基线实现；L10 用 offset buffer 优化）。
        let mut offset = 0usize;
        for _ in 0..row {
            let len = self.read_len(offset)?;
            offset += 4 + len;
        }
        let len = self.read_len(offset)?;
        let raw = &self.buffer.bytes[offset + 4..offset + 4 + len];
        self.decode_value(raw)
    }

    fn read_len(&self, offset: usize) -> Result<usize> {
        let bytes: [u8; 4] = self.buffer.bytes[offset..offset + 4]
            .try_into()
            .map_err(|_| lattice_common::internal("长度前缀损坏"))?;
        Ok(u32::from_be_bytes(bytes) as usize)
    }

    fn decode_value(&self, raw: &[u8]) -> Result<Scalar> {
        Ok(match self.data_type {
            DataType::Bool => Scalar::Bool(raw.first().copied().unwrap_or(0) != 0),
            DataType::Float32 | DataType::Float64 if raw.len() == 8 => {
                Scalar::Float(f64::from_be_bytes(raw.try_into().unwrap()))
            }
            DataType::Decimal { .. } if raw.len() == 17 => Scalar::Decimal {
                unscaled: i128::from_be_bytes(raw[..16].try_into().unwrap()),
                scale: raw[16],
            },
            DataType::Binary(_) | DataType::VarBinary(_) | DataType::Bit => {
                Scalar::Bytes(raw.to_vec())
            }
            DataType::Char(_) | DataType::VarChar(_) => {
                Scalar::Str(String::from_utf8_lossy(raw).into_owned())
            }
            DataType::Json => Scalar::Json(String::from_utf8_lossy(raw).into_owned()),
            DataType::Date
            | DataType::Time
            | DataType::DateTime
            | DataType::Timestamp
            | DataType::Duration
            | DataType::Year
                if raw.len() == 8 =>
            {
                Scalar::Temporal(i64::from_be_bytes(raw.try_into().unwrap()))
            }
            _ => Scalar::Bytes(raw.to_vec()),
        })
    }

    /// 按选择向量投影出新的逻辑列（延迟物化，docs/02 §2.1）。
    pub fn take(&self, selection: &SelectionVector) -> Result<Column> {
        let mut out = Column::new(self.data_type, 0);
        for logical in 0..selection.len() {
            let physical = selection
                .physical_row(logical)
                .ok_or_else(|| lattice_common::internal("selection vector 缺行"))?
                as usize;
            let value = self.get(physical)?;
            if matches!(value, Scalar::Null) {
                out.validity.push(false);
                if let Some(width) = self.data_type.fixed_width() {
                    out.buffer.bytes.extend(std::iter::repeat(0u8).take(width));
                } else {
                    out.buffer.bytes.extend_from_slice(&0u32.to_be_bytes());
                }
            } else {
                out.validity.push(true);
                out.encode_value(&value)?;
            }
        }
        Ok(out)
    }
}

/// 列式批（对齐 Arrow 语义，docs/02 §2.1 `Block`）。
pub type Block = BlockBase<Column>;

/// 批构建器（复用 `common` 的形状一致性校验）。
pub type BlockBuilderAlias = BlockBuilder<Column>;

/// 便捷构造：由若干已填好的列组装 Block。
pub fn block_from_columns(columns: Vec<Column>) -> Result<Block> {
    let rows = columns.first().map(|c| c.len()).unwrap_or(0);
    if columns.iter().any(|c| c.len() != rows) {
        return Err(lattice_common::Error::new(
            lattice_common::ErrorKind::Internal,
            "各列行数不一致",
        ));
    }
    let capacity = rows.max(1);
    let mut builder = BlockBuilder::<Column>::new(capacity);
    for col in columns {
        builder = builder.column(col);
    }
    builder.rows(rows).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_width_column_roundtrip_with_nulls() {
        let mut col = Column::new(DataType::Int32, 0);
        col.push(&Scalar::Int(42)).unwrap();
        col.push(&Scalar::Null).unwrap();
        col.push(&Scalar::Int(-7)).unwrap();
        assert_eq!(col.len(), 3);
        assert_eq!(col.null_count(), 1);
        assert_eq!(col.get(0).unwrap(), Scalar::Int(42));
        assert_eq!(col.get(1).unwrap(), Scalar::Null);
        assert_eq!(col.get(2).unwrap(), Scalar::Int(-7));
        assert!(col.get(3).is_err());
    }

    #[test]
    fn varchar_column_roundtrip() {
        let mut col = Column::new(DataType::VarChar(64), 0);
        col.push(&Scalar::Str("hello".into())).unwrap();
        col.push(&Scalar::Null).unwrap();
        col.push(&Scalar::Str("世界".into())).unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Str("hello".into()));
        assert_eq!(col.get(1).unwrap(), Scalar::Null);
        assert_eq!(col.get(2).unwrap(), Scalar::Str("世界".into()));
    }

    #[test]
    fn type_mismatch_is_rejected() {
        let mut col = Column::new(DataType::Int64, 0);
        assert_eq!(
            col.push(&Scalar::Bool(true)).unwrap_err().kind,
            lattice_common::ErrorKind::TypeMismatch
        );
    }

    #[test]
    fn take_applies_selection_vector() {
        let mut col = Column::new(DataType::Int16, 0);
        for v in [10, 20, 30, 40] {
            col.push(&Scalar::Int(v)).unwrap();
        }
        let projected = col.take(&SelectionVector(vec![0, 2, 3])).unwrap();
        assert_eq!(projected.get(0).unwrap(), Scalar::Int(10));
        assert_eq!(projected.get(1).unwrap(), Scalar::Int(30));
        assert_eq!(projected.get(2).unwrap(), Scalar::Int(40));
    }

    #[test]
    fn block_from_columns_rejects_ragged_columns() {
        let mut a = Column::new(DataType::Int8, 0);
        a.push(&Scalar::Int(1)).unwrap();
        let b = Column::new(DataType::Int8, 2);
        assert!(block_from_columns(vec![a, b]).is_err());
    }
}
