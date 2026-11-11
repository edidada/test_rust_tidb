//! `parser` crate (L1 契约骨架，L0 空壳)。
//! 契约来源：docs/02 §3：`trait Parser { fn parse(&self, sql: &str) -> Result<Vec<AstStatement>>; }`

use lattice_common::Result;

/// AST 语句占位（L1 实现完整 AST + 错误恢复）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstStatement {
    pub sql: String,
}

/// 解析器 trait（docs/02 §3）。
pub trait Parser {
    fn parse(&self, sql: &str) -> Result<Vec<AstStatement>>;
}

/// L0 占位实现：保留原始 SQL，供后续 crate 贯通编译与单测。
#[derive(Debug, Default)]
pub struct PlaceholderParser;

impl Parser for PlaceholderParser {
    fn parse(&self, sql: &str) -> Result<Vec<AstStatement>> {
        let text = sql.trim();
        if text.is_empty() {
            return Err(lattice_common::Error::new(
                lattice_common::ErrorKind::Parse,
                "空 SQL 语句",
            ));
        }
        Ok(vec![AstStatement {
            sql: text.to_string(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_parser_preserves_sql() {
        let stmts = PlaceholderParser.parse("select 1").unwrap();
        assert_eq!(stmts[0].sql, "select 1");
    }

    #[test]
    fn placeholder_parser_rejects_empty() {
        assert_eq!(
            PlaceholderParser.parse("  ").unwrap_err().kind,
            lattice_common::ErrorKind::Parse
        );
    }
}
