//! `Block` 基础工具（docs/02 §2.1、§11）。
//!
//! `common` 提供与具体类型无关的**列式批骨架**：字节缓冲区、有效性位图、
//! 延迟物化的选择向量，以及泛型 `Block<C>` / `BlockBuilder<C>`。
//! `types` crate 在此基础上定义带 `DataType` 的具体 `Column` 并别名出 `Block`，
//! 从而保证 `common` 不反向依赖 `types`（依赖方向检查）。

/// 原始值缓冲区（对齐 Arrow Buffer 语义）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buffer {
    pub bytes: Vec<u8>,
}

impl Buffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// NULL 有效性位图：`true` 表示非 NULL（valid），`false` 表示 NULL。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validity {
    bits: Vec<bool>,
}

impl Validity {
    pub fn new(len: usize, all_valid: bool) -> Self {
        Self {
            bits: vec![all_valid; len],
        }
    }

    pub fn from_bits(bits: Vec<bool>) -> Self {
        Self { bits }
    }

    pub fn len(&self) -> usize {
        self.bits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    pub fn is_valid(&self, row: usize) -> bool {
        self.bits.get(row).copied().unwrap_or(false)
    }

    pub fn set(&mut self, row: usize, valid: bool) {
        if let Some(bit) = self.bits.get_mut(row) {
            *bit = valid;
        }
    }

    /// NULL 行数（docs/02 §2.1：NULL 排前语义由上层编码处理）。
    pub fn null_count(&self) -> usize {
        self.bits.iter().filter(|v| !**v).count()
    }

    /// 追加一行有效性位（供 `Column::push` 递增构造）。
    pub fn push(&mut self, valid: bool) {
        self.bits.push(valid);
    }

    /// 暴露底层位集合（供 `Column::take` 重建有效性位图）。
    pub fn bits_vec(&self) -> Vec<bool> {
        self.bits.clone()
    }
}

/// 延迟物化的选择向量：逻辑行号到物理行号的映射。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionVector(pub Vec<u32>);

impl SelectionVector {
    pub fn identity(len: usize) -> Self {
        Self((0..len as u32).collect())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 逻辑第 `i` 行对应的物理行号。
    pub fn physical_row(&self, i: usize) -> Option<u32> {
        self.0.get(i).copied()
    }

    /// 过滤出满足谓词的逻辑行（docs/02 §2.1：Filter 仅保留 True）。
    pub fn select(&self, predicate: impl Fn(u32) -> bool) -> Self {
        Self(self.0.iter().copied().filter(|r| predicate(*r)).collect())
    }
}

/// 泛型列式批：每列由上层 `C` 提供类型与数据，本结构只负责批的形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block<C> {
    pub columns: Vec<C>,
    /// 延迟物化的行号索引；`None` 表示全量物化。
    pub selection: Option<SelectionVector>,
    pub num_rows: usize,
    pub capacity: usize,
}

impl<C> Block<C> {
    pub fn new(capacity: usize) -> Self {
        Self {
            columns: Vec::new(),
            selection: None,
            num_rows: 0,
            capacity,
        }
    }

    pub fn empty() -> Self {
        Self::new(0)
    }

    pub fn is_empty(&self) -> bool {
        self.num_rows == 0
    }

    /// 逻辑行数：有选择向量时等于其长度，否则等于物理行数。
    pub fn logical_rows(&self) -> usize {
        match &self.selection {
            Some(sel) => sel.len(),
            None => self.num_rows,
        }
    }

    pub fn col_count(&self) -> usize {
        self.columns.len()
    }
}

/// 构建器：保证 `num_rows` / `capacity` / 列数一致，避免手工构造出错。
#[derive(Debug, Clone)]
pub struct BlockBuilder<C> {
    capacity: usize,
    columns: Vec<C>,
    num_rows: Option<usize>,
    selection: Option<SelectionVector>,
}

impl<C> BlockBuilder<C> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            columns: Vec::new(),
            num_rows: None,
            selection: None,
        }
    }

    pub fn column(mut self, col: C) -> Self {
        self.columns.push(col);
        self
    }

    pub fn rows(mut self, n: usize) -> Self {
        self.num_rows = Some(n);
        self
    }

    pub fn selection(mut self, sel: SelectionVector) -> Self {
        self.selection = Some(sel);
        self
    }

    /// 构建并按 `capacity` 与行数做一致性校验。
    pub fn build(self) -> crate::Result<Block<C>> {
        let num_rows = self.num_rows.unwrap_or(0);
        if num_rows > self.capacity {
            return Err(crate::Error::new(
                crate::ErrorKind::Internal,
                format!("num_rows ({num_rows}) 超过 capacity ({})", self.capacity),
            ));
        }
        if let Some(sel) = &self.selection {
            if sel.0.iter().any(|r| *r as usize >= num_rows) {
                return Err(crate::Error::new(
                    crate::ErrorKind::Internal,
                    "selection vector 含越界物理行号",
                ));
            }
        }
        Ok(Block {
            columns: self.columns,
            selection: self.selection,
            num_rows,
            capacity: self.capacity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_counts_nulls() {
        let mut v = Validity::new(3, true);
        v.set(1, false);
        assert_eq!(v.null_count(), 1);
        assert!(!v.is_valid(1));
        assert!(v.is_valid(2));
        assert!(!v.is_valid(99));
    }

    #[test]
    fn selection_vector_filters_logical_rows() {
        let sel = SelectionVector::identity(5).select(|r| r % 2 == 0);
        assert_eq!(sel.0, vec![0, 2, 4]);
        assert_eq!(sel.physical_row(1), Some(2));
    }

    #[test]
    fn builder_validates_capacity_and_selection() {
        let block = BlockBuilder::<i32>::new(4)
            .column(1)
            .column(2)
            .rows(3)
            .build()
            .unwrap();
        assert_eq!(block.num_rows, 3);
        assert_eq!(block.logical_rows(), 3);

        let err = BlockBuilder::<i32>::new(2).rows(3).build().unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::Internal);

        let err = BlockBuilder::<i32>::new(4)
            .rows(2)
            .selection(SelectionVector(vec![0, 5]))
            .build()
            .unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::Internal);
    }
}
