//! 行式火山模型的 `Row` / Block 转换工具（docs/02 §3、docs/03 L3 W10）。
//! L3 采用行式（row-at-a-time）语义，输出仍为列式 `Block`（对齐 Arrow 语义）。

use lattice_common::{Error, ErrorKind, Result, SelectionVector};
use lattice_types::{Block, Column, DataType, Scalar};

/// 一行数据（L3 行式执行中间表示）。
pub type Row = Vec<Scalar>;

/// 输入 Schema（列名 + 类型 + nullable）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSchema {
    pub columns: Vec<(String, DataType)>,
}

impl RowSchema {
    pub fn new(columns: Vec<(String, DataType)>) -> Self {
        Self { columns }
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
}

/// 由若干行构造列式 Block（每列用 `Column::push` 逐行追加）。
pub fn rows_to_block(rows: &[Row], schema: &RowSchema) -> Result<Block> {
    if schema.is_empty() {
        return lattice_types::block_from_columns(Vec::new());
    }
    let mut columns: Vec<Column> = schema
        .columns
        .iter()
        .map(|(_, ty)| Column::empty(*ty))
        .collect();
    for row in rows {
        if row.len() != schema.len() {
            return Err(Error::new(
                ErrorKind::Internal,
                format!("行列数不匹配: {} vs {}", row.len(), schema.len()),
            ));
        }
        for (i, value) in row.iter().enumerate() {
            columns[i].push(value)?;
        }
    }
    lattice_types::block_from_columns(columns)
}

/// 从 Block 逐行读出 `Row`（用于行式算子处理）。
pub fn block_to_rows(block: &Block, schema: &RowSchema) -> Result<Vec<Row>> {
    if block.col_count() != schema.len() {
        return Err(Error::new(
            ErrorKind::Internal,
            format!(
                "Block 列数不匹配: {} vs {}",
                block.col_count(),
                schema.len()
            ),
        ));
    }
    let mut out = Vec::with_capacity(block.num_rows);
    for r in 0..block.num_rows {
        let mut row = Vec::with_capacity(schema.len());
        for (c, col) in block.columns.iter().enumerate() {
            let value = col.get(r)?;
            // 列类型与标量形状一致性由 Column::push 保证；此处仅保留原始值。
            let _ = c;
            row.push(value);
        }
        out.push(row);
    }
    Ok(out)
}

/// 按选择向量过滤 Block 的所有列（延迟物化，docs/02 §2.1）。
pub fn block_take(block: &Block, selection: &SelectionVector) -> Result<Block> {
    let mut columns = Vec::with_capacity(block.col_count());
    for col in &block.columns {
        columns.push(col.take(selection)?);
    }
    lattice_types::block_from_columns(columns)
}

/// 空 Block（0 行，保留列类型）。
pub fn empty_block(schema: &RowSchema) -> Result<Block> {
    rows_to_block(&[], schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> RowSchema {
        RowSchema::new(vec![
            ("a".into(), DataType::Int64),
            ("b".into(), DataType::VarChar(32)),
        ])
    }

    #[test]
    fn rows_block_roundtrip() {
        let s = schema();
        let rows = vec![
            vec![Scalar::Int(1), Scalar::Str("x".into())],
            vec![Scalar::Int(2), Scalar::Null],
            vec![Scalar::Null, Scalar::Str("z".into())],
        ];
        let block = rows_to_block(&rows, &s).unwrap();
        assert_eq!(block.num_rows, 3);
        assert_eq!(block.columns[1].null_count(), 1);
        let back = block_to_rows(&block, &s).unwrap();
        assert_eq!(back, rows);
    }

    #[test]
    fn ragged_row_is_rejected() {
        let s = schema();
        let rows = vec![vec![Scalar::Int(1)]];
        assert_eq!(
            rows_to_block(&rows, &s).unwrap_err().kind,
            ErrorKind::Internal
        );
    }

    #[test]
    fn empty_block_keeps_types() {
        let b = empty_block(&schema()).unwrap();
        assert_eq!(b.num_rows, 0);
        assert_eq!(b.columns[0].data_type, DataType::Int64);
    }

    #[test]
    fn block_take_projects_all_columns() {
        let s = schema();
        let rows = vec![
            vec![Scalar::Int(10), Scalar::Str("a".into())],
            vec![Scalar::Int(20), Scalar::Str("b".into())],
            vec![Scalar::Int(30), Scalar::Str("c".into())],
        ];
        let block = rows_to_block(&rows, &s).unwrap();
        let taken = block_take(&block, &SelectionVector(vec![0, 2])).unwrap();
        let back = block_to_rows(&taken, &s).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1][0], Scalar::Int(30));
    }
}
