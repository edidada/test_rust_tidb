//! `catalog` crate (L1 Catalog 与 Binder)。
//! 契约来源：docs/02 §3、docs/04 F-SQL-007/008。

pub mod binder;

pub use binder::{BindOutput, BoundItem, DefaultBinder};

use lattice_common::Result;
use lattice_planner::LogicalPlanNode;
use lattice_types::DataType;

/// 限定名（db.table）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedName {
    pub database: String,
    pub table: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub id: TableId,
    pub name: QualifiedName,
    pub columns: Vec<(String, DataType)>,
}

/// 目录提供者 trait（docs/02 §3）。
pub trait CatalogProvider {
    fn table(&self, name: &QualifiedName) -> Result<Table>;
    fn index(&self, table: TableId, idx: IndexId) -> Result<String>;
}

/// 会话上下文占位（L5 填充权限、变量、schema 版本）。
#[derive(Debug, Clone, Default)]
pub struct SessionCtx;

/// Binder trait（docs/02 §3；L1 实现名称解析/类型推导/隐式转换）。
pub trait Binder {
    fn bind(
        &self,
        stmt: &lattice_parser::AstStatement,
        ctx: &SessionCtx,
    ) -> Result<LogicalPlanNode>;
}

/// 内存 Catalog（L0 空实现，L1 填充元数据）。
#[derive(Debug, Default)]
pub struct MemoryCatalog {
    tables: Vec<Table>,
}

impl MemoryCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_table(&mut self, table: Table) {
        self.tables.push(table);
    }
}

impl CatalogProvider for MemoryCatalog {
    fn table(&self, name: &QualifiedName) -> Result<Table> {
        self.tables
            .iter()
            .find(|t| t.name == *name)
            .cloned()
            .ok_or_else(|| {
                lattice_common::Error::new(
                    lattice_common::ErrorKind::Bind,
                    format!("表 {}.{} 不存在", name.database, name.table),
                )
            })
    }

    fn index(&self, _table: TableId, _idx: IndexId) -> Result<String> {
        Err(lattice_common::unsupported("索引解析将在 L1 实现"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_catalog_resolves_known_table() {
        let mut cat = MemoryCatalog::new();
        cat.add_table(Table {
            id: TableId(1),
            name: QualifiedName {
                database: "test".into(),
                table: "t".into(),
            },
            columns: vec![("a".into(), DataType::Int64)],
        });
        let t = cat
            .table(&QualifiedName {
                database: "test".into(),
                table: "t".into(),
            })
            .unwrap();
        assert_eq!(t.id, TableId(1));
    }

    #[test]
    fn missing_table_is_bind_error() {
        let cat = MemoryCatalog::new();
        let err = cat
            .table(&QualifiedName {
                database: "test".into(),
                table: "x".into(),
            })
            .unwrap_err();
        assert_eq!(err.kind, lattice_common::ErrorKind::Bind);
    }
}
