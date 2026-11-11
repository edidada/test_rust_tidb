//! `parser` crate (L1 词法/语法分析器)。
//! 契约来源：docs/02 §3 `trait Parser`、docs/04 F-SQL-005/006。

pub mod ast;
pub mod lexer;
pub mod parser_impl;

pub use ast::{AstExpr, BinaryOp, Literal, OrderItem, SelectItem, Statement, TableRef, UnaryOp};
pub use lexer::{tokenize, Keyword, Symbol, Token, TokenKind};
pub use parser_impl::{parse_statements, SqlParser};

use lattice_common::Result;

/// AST 语句（docs/02 §3 `AstStatement`）。
#[derive(Debug, Clone, PartialEq)]
pub struct AstStatement {
    /// 原始 SQL 文本（保留供 Slow Log / Replayer 使用）。
    pub sql: String,
    /// 解析后的语句；`None` 表示占位解析（兼容 L0 骨架）。
    pub stmt: Option<Statement>,
}

impl AstStatement {
    pub fn statement(&self) -> Option<&Statement> {
        self.stmt.as_ref()
    }
}

/// 解析器 trait（docs/02 §3）。
pub trait Parser {
    fn parse(&self, sql: &str) -> Result<Vec<AstStatement>>;
}

/// L0 占位实现：保留原始 SQL，不做语法分析（供装配自检与向后兼容）。
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
            stmt: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_parser_produces_ast() {
        let stmts = SqlParser.parse("SELECT a + 1 FROM t WHERE a > 0").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(
            stmts[0].statement(),
            Some(Statement::Select { .. })
        ));
    }

    #[test]
    fn placeholder_parser_preserves_sql() {
        let stmts = PlaceholderParser.parse("select 1").unwrap();
        assert_eq!(stmts[0].sql, "select 1");
        assert!(stmts[0].statement().is_none());
    }

    #[test]
    fn placeholder_parser_rejects_empty() {
        assert_eq!(
            PlaceholderParser.parse("  ").unwrap_err().kind,
            lattice_common::ErrorKind::Parse
        );
    }

    #[test]
    fn sql_parser_reports_syntax_errors() {
        assert_eq!(
            SqlParser.parse("SELECT FROM").unwrap_err().kind,
            lattice_common::ErrorKind::Parse
        );
    }
}
