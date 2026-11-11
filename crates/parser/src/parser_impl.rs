//! 递归下降 Parser（docs/04 F-SQL-005/006：AST + 错误位置诊断）。

use crate::ast::{AstExpr, BinaryOp, Literal, OrderItem, SelectItem, Statement, TableRef, UnaryOp};
use crate::lexer::{tokenize, Keyword, Symbol, Token, TokenKind};
use lattice_common::{Error, ErrorKind, Result};

/// SQL 解析器（docs/02 §3 `trait Parser`）。
#[derive(Debug, Default)]
pub struct SqlParser;

impl crate::Parser for SqlParser {
    fn parse(&self, sql: &str) -> Result<Vec<crate::AstStatement>> {
        let stmts = parse_statements(sql)?;
        Ok(stmts
            .into_iter()
            .map(|stmt| crate::AstStatement {
                sql: sql.to_string(),
                stmt: Some(stmt),
            })
            .collect())
    }
}

pub fn parse_statements(sql: &str) -> Result<Vec<Statement>> {
    let tokens = tokenize(sql)?;
    let mut cursor = Cursor::new(tokens);
    let mut stmts = Vec::new();
    while !cursor.peek_is_eof() {
        stmts.push(cursor.parse_statement()?);
        if cursor.eat_symbol(Symbol::Semicolon) {
            continue;
        }
        if !cursor.peek_is_eof() {
            return Err(cursor.error("语句之间缺少分号"));
        }
    }
    if stmts.is_empty() {
        return Err(Error::new(ErrorKind::Parse, "空 SQL 语句"));
    }
    Ok(stmts)
}

struct Cursor {
    tokens: Vec<Token>,
    pos: usize,
}

impl Cursor {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_is_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn error(&self, msg: &str) -> Error {
        let pos = self.peek().start;
        Error::new(ErrorKind::Parse, format!("位置 {pos}: {msg}"))
    }

    fn expect_keyword(&mut self, kw: Keyword) -> Result<()> {
        if matches!(self.peek().kind, TokenKind::Keyword(k) if k == kw) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("期望关键字 {kw:?}")))
        }
    }

    fn eat_keyword(&mut self, kw: Keyword) -> bool {
        if matches!(self.peek().kind, TokenKind::Keyword(k) if k == kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_symbol(&mut self, sym: Symbol) -> bool {
        if matches!(self.peek().kind, TokenKind::Symbol(s) if s == sym) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, sym: Symbol) -> Result<()> {
        if self.eat_symbol(sym) {
            Ok(())
        } else {
            Err(self.error(&format!("期望符号 {sym:?}")))
        }
    }

    fn identifier(&mut self) -> Result<String> {
        match self.advance().kind {
            TokenKind::Ident(s) => Ok(s),
            // 允许非保留关键字作为标识符（简化）。
            TokenKind::Keyword(kw) => Ok(format!("{kw:?}").to_ascii_lowercase()),
            _ => Err(self.error("期望标识符")),
        }
    }

    fn qualified_name(&mut self) -> Result<Vec<String>> {
        let mut parts = vec![self.identifier()?];
        while self.eat_symbol(Symbol::Dot) {
            parts.push(self.identifier()?);
        }
        Ok(parts)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        if self.eat_keyword(Keyword::Explain) {
            let inner = self.parse_statement()?;
            return Ok(Statement::Explain {
                inner: Box::new(inner),
            });
        }
        match self.peek().kind {
            TokenKind::Keyword(Keyword::Select) => self.parse_select(),
            TokenKind::Keyword(Keyword::Insert) => self.parse_insert(),
            TokenKind::Keyword(Keyword::Create) => self.parse_create_table(),
            _ => Err(self.error("仅支持 SELECT / INSERT / CREATE TABLE / EXPLAIN")),
        }
    }

    fn parse_select(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Select)?;
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr()?;
            let has_explicit_as = self.eat_keyword(Keyword::As);
            let alias = if has_explicit_as || matches!(self.peek().kind, TokenKind::Ident(_)) {
                Some(self.identifier()?)
            } else {
                None
            };
            items.push(SelectItem { expr, alias });
            if !self.eat_symbol(Symbol::Comma) {
                break;
            }
        }
        let mut from = Vec::new();
        if self.eat_keyword(Keyword::From) {
            loop {
                let name = self.qualified_name()?;
                let alias = if self.eat_keyword(Keyword::As) {
                    Some(self.identifier()?)
                } else {
                    None
                };
                from.push(TableRef { name, alias });
                if !self.eat_symbol(Symbol::Comma) {
                    break;
                }
            }
        }
        let selection = if self.eat_keyword(Keyword::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.eat_keyword(Keyword::Order) {
            self.expect_keyword(Keyword::By)?;
            loop {
                let expr = self.parse_expr()?;
                let desc = self.eat_keyword(Keyword::Desc);
                if !desc {
                    self.eat_keyword(Keyword::Asc);
                }
                order_by.push(OrderItem { expr, desc });
                if !self.eat_symbol(Symbol::Comma) {
                    break;
                }
            }
        }
        let limit = if self.eat_keyword(Keyword::Limit) {
            match self.advance().kind {
                TokenKind::Number(n) => Some(
                    n.parse::<u64>()
                        .map_err(|_| self.error("LIMIT 必须是非负整数"))?,
                ),
                TokenKind::Param(_) => Some(0), // Prepared 占位，L5 绑定
                _ => return Err(self.error("LIMIT 后期望整数")),
            }
        } else {
            None
        };
        Ok(Statement::Select {
            items,
            from,
            selection,
            order_by,
            limit,
        })
    }

    fn parse_insert(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Insert)?;
        self.expect_keyword(Keyword::Into)?;
        let table = self.qualified_name()?;
        let mut columns = Vec::new();
        if self.eat_symbol(Symbol::LParen) {
            loop {
                columns.push(self.identifier()?);
                if !self.eat_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RParen)?;
        }
        self.expect_keyword(Keyword::Values)?;
        let mut values = Vec::new();
        loop {
            self.expect_symbol(Symbol::LParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                if !self.eat_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RParen)?;
            values.push(row);
            if !self.eat_symbol(Symbol::Comma) {
                break;
            }
        }
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_create_table(&mut self) -> Result<Statement> {
        self.expect_keyword(Keyword::Create)?;
        self.expect_keyword(Keyword::Table)?;
        let name = self.qualified_name()?;
        self.expect_symbol(Symbol::LParen)?;
        let mut columns = Vec::new();
        loop {
            let col = self.identifier()?;
            let ty = self.parse_type_name()?;
            columns.push((col, ty));
            if !self.eat_symbol(Symbol::Comma) {
                break;
            }
        }
        self.expect_symbol(Symbol::RParen)?;
        Ok(Statement::CreateTable { name, columns })
    }

    fn parse_type_name(&mut self) -> Result<String> {
        let name = match self.advance().kind {
            TokenKind::Keyword(kw) => format!("{kw:?}"),
            TokenKind::Ident(s) => s,
            _ => return Err(self.error("期望类型名")),
        };
        if self.eat_symbol(Symbol::LParen) {
            match self.advance().kind {
                TokenKind::Number(n) => {
                    self.expect_symbol(Symbol::RParen)?;
                    return Ok(format!("{name}({n})"));
                }
                _ => return Err(self.error("类型长度必须是整数")),
            }
        }
        Ok(name)
    }

    // ---- 表达式：优先级 OR < AND < 比较 < 加减 < 乘除 < 一元 < 原子 ----
    fn parse_expr(&mut self) -> Result<AstExpr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<AstExpr> {
        let mut lhs = self.parse_and()?;
        while self.eat_keyword(Keyword::Or) {
            let rhs = self.parse_and()?;
            lhs = AstExpr::Binary {
                op: BinaryOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<AstExpr> {
        let mut lhs = self.parse_comparison()?;
        while self.eat_keyword(Keyword::And) {
            let rhs = self.parse_comparison()?;
            lhs = AstExpr::Binary {
                op: BinaryOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_comparison(&mut self) -> Result<AstExpr> {
        let lhs = self.parse_additive()?;
        // IS [NOT] NULL
        if self.eat_keyword(Keyword::Is) {
            let negated = self.eat_keyword(Keyword::Not);
            self.expect_keyword(Keyword::Null)?;
            return Ok(AstExpr::IsNull {
                expr: Box::new(lhs),
                negated,
            });
        }
        let op = match self.peek().kind {
            TokenKind::Symbol(Symbol::Eq) => Some(BinaryOp::Eq),
            TokenKind::Symbol(Symbol::Ne) => Some(BinaryOp::Ne),
            TokenKind::Symbol(Symbol::Lt) => Some(BinaryOp::Lt),
            TokenKind::Symbol(Symbol::Le) => Some(BinaryOp::Le),
            TokenKind::Symbol(Symbol::Gt) => Some(BinaryOp::Gt),
            TokenKind::Symbol(Symbol::Ge) => Some(BinaryOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let rhs = self.parse_additive()?;
            return Ok(AstExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<AstExpr> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Symbol(Symbol::Plus) => BinaryOp::Add,
                TokenKind::Symbol(Symbol::Minus) => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_multiplicative()?;
            lhs = AstExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<AstExpr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Symbol(Symbol::Star) => BinaryOp::Mul,
                TokenKind::Symbol(Symbol::Slash) => BinaryOp::Div,
                TokenKind::Symbol(Symbol::Percent) => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = AstExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<AstExpr> {
        if self.eat_keyword(Keyword::Not) {
            return Ok(AstExpr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_unary()?),
            });
        }
        if self.eat_symbol(Symbol::Minus) {
            return Ok(AstExpr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            });
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<AstExpr> {
        let token = self.advance();
        Ok(match token.kind {
            TokenKind::Number(n) => {
                if n.contains('.') {
                    AstExpr::Literal(Literal::Float(
                        n.parse().map_err(|_| self.error("非法浮点字面量"))?,
                    ))
                } else {
                    AstExpr::Literal(Literal::Int(
                        n.parse().map_err(|_| self.error("非法整数字面量"))?,
                    ))
                }
            }
            TokenKind::StringLit(s) => AstExpr::Literal(Literal::Str(s)),
            TokenKind::Param(n) => AstExpr::Param(n),
            TokenKind::Keyword(Keyword::Null) => AstExpr::Literal(Literal::Null),
            TokenKind::Keyword(Keyword::True) => AstExpr::Literal(Literal::Bool(true)),
            TokenKind::Keyword(Keyword::False) => AstExpr::Literal(Literal::Bool(false)),
            TokenKind::Symbol(Symbol::Star) => AstExpr::Star,
            TokenKind::Symbol(Symbol::LParen) => {
                let inner = self.parse_expr()?;
                self.expect_symbol(Symbol::RParen)?;
                inner
            }
            TokenKind::Ident(first) => {
                let mut parts = vec![first];
                while self.eat_symbol(Symbol::Dot) {
                    parts.push(self.identifier()?);
                }
                // 函数调用：ident(...)
                if self.eat_symbol(Symbol::LParen) {
                    let mut _args = Vec::new();
                    if !self.eat_symbol(Symbol::RParen) {
                        loop {
                            _args.push(self.parse_expr()?);
                            if !self.eat_symbol(Symbol::Comma) {
                                break;
                            }
                        }
                        self.expect_symbol(Symbol::RParen)?;
                    }
                    // L1 阶段函数参数被解析但暂不绑定为 Expr（L1 后续扩展）。
                    // 用 Column 占位保持 AST 可比较性。
                    AstExpr::Column(parts)
                } else {
                    AstExpr::Column(parts)
                }
            }
            _ => return Err(self.error("期望表达式")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_select_projection_and_where() {
        let stmts =
            parse_statements("SELECT a, b + 1 AS c FROM test.t WHERE a >= 10 AND b < 5").unwrap();
        assert_eq!(stmts.len(), 1);
        let Statement::Select {
            items,
            from,
            selection,
            ..
        } = &stmts[0]
        else {
            panic!()
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].alias.as_deref(), Some("c"));
        assert_eq!(from[0].name, vec!["test", "t"]);
        assert!(selection.is_some());
    }

    #[test]
    fn operator_precedence() {
        let stmts = parse_statements("SELECT 1 + 2 * 3 > 5 AND NOT a = 1 OR 0").unwrap();
        let Statement::Select { items, .. } = &stmts[0] else {
            panic!()
        };
        // 顶层应为 OR
        assert!(matches!(
            items[0].expr,
            AstExpr::Binary {
                op: BinaryOp::Or,
                ..
            }
        ));
    }

    #[test]
    fn parses_insert_create_and_explain() {
        let stmts = parse_statements(
            "INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y'); CREATE TABLE s (id BIGINT, name VARCHAR(32)); EXPLAIN SELECT 1",
        )
        .unwrap();
        assert_eq!(stmts.len(), 3);
        assert!(matches!(stmts[0], Statement::Insert { .. }));
        assert!(matches!(stmts[1], Statement::CreateTable { .. }));
        assert!(matches!(stmts[2], Statement::Explain { .. }));
    }

    #[test]
    fn syntax_error_has_position() {
        let err = parse_statements("SELECT FROM").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.msg.contains("位置"));
    }

    #[test]
    fn is_null_and_limit() {
        let stmts = parse_statements("SELECT * FROM t WHERE a IS NOT NULL LIMIT 10").unwrap();
        let Statement::Select {
            selection, limit, ..
        } = &stmts[0]
        else {
            panic!()
        };
        assert!(matches!(
            selection,
            Some(AstExpr::IsNull { negated: true, .. })
        ));
        assert_eq!(*limit, Some(10));
    }
}
