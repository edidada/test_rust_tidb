//! Lexer（docs/04 F-SQL-005/006：词法分析与位置诊断）。

use lattice_common::{Error, ErrorKind, Result};

/// 词法单元（带起止字节位置，供错误恢复与诊断）。
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    StringLit(String),
    /// 参数占位符 `?` / `$1`（Prepared，F-SQL-004 骨架）。
    Param(u32),
    Keyword(Keyword),
    Symbol(Symbol),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Select,
    From,
    Where,
    As,
    And,
    Or,
    Not,
    Null,
    Is,
    True,
    False,
    Insert,
    Into,
    Values,
    Create,
    Table,
    Int,
    BigInt,
    VarChar,
    Bool,
    Double,
    Limit,
    Order,
    By,
    Asc,
    Desc,
    Explain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    LParen,
    RParen,
    Comma,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Semicolon,
    Dot,
}

/// 词法分析：将 SQL 文本切分为 Token 流。
pub fn tokenize(sql: &str) -> Result<Vec<Token>> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // 行注释 `-- ...` 与块注释 `/* ... */`（F-SQL-013 骨架）。
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err(parse_error(i, "未闭合的块注释"));
            }
            i += 2;
            continue;
        }
        let start = i;
        match b {
            b'\'' | b'"' => {
                let quote = b;
                i += 1;
                let mut s = String::new();
                while i < bytes.len() {
                    // SQL 标准：'' 表示字面量中的单引号（需在闭合判断之前处理）。
                    if quote == b'\''
                        && bytes[i] == b'\''
                        && i + 1 < bytes.len()
                        && bytes[i + 1] == b'\''
                    {
                        s.push('\'');
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        break;
                    }
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                        let esc = bytes[i];
                        s.push(match esc {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            other => other as char,
                        });
                    } else {
                        s.push(bytes[i] as char);
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(parse_error(start, "未闭合的字符串字面量"));
                }
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::StringLit(s),
                    start,
                    end: i,
                });
            }
            b'0'..=b'9' => {
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                let text = sql[start..i].to_string();
                tokens.push(Token {
                    kind: TokenKind::Number(text),
                    start,
                    end: i,
                });
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let text = &sql[start..i];
                let upper = text.to_ascii_uppercase();
                let kind = match lookup_keyword(&upper) {
                    Some(kw) => TokenKind::Keyword(kw),
                    None => TokenKind::Ident(text.to_string()),
                };
                tokens.push(Token {
                    kind,
                    start,
                    end: i,
                });
            }
            b'?' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Param(0),
                    start,
                    end: i,
                });
            }
            b'$' => {
                i += 1;
                let nstart = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if nstart == i {
                    return Err(parse_error(start, "$ 后缺少参数序号"));
                }
                let n = sql[nstart..i].parse::<u32>().unwrap_or(0);
                tokens.push(Token {
                    kind: TokenKind::Param(n),
                    start,
                    end: i,
                });
            }
            _ => {
                let (sym, width) = match_symbol(bytes, i)?;
                i += width;
                tokens.push(Token {
                    kind: TokenKind::Symbol(sym),
                    start,
                    end: i,
                });
            }
        }
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        start: bytes.len(),
        end: bytes.len(),
    });
    Ok(tokens)
}

fn lookup_keyword(s: &str) -> Option<Keyword> {
    Some(match s {
        "SELECT" => Keyword::Select,
        "FROM" => Keyword::From,
        "WHERE" => Keyword::Where,
        "AS" => Keyword::As,
        "AND" => Keyword::And,
        "OR" => Keyword::Or,
        "NOT" => Keyword::Not,
        "NULL" => Keyword::Null,
        "IS" => Keyword::Is,
        "TRUE" => Keyword::True,
        "FALSE" => Keyword::False,
        "INSERT" => Keyword::Insert,
        "INTO" => Keyword::Into,
        "VALUES" => Keyword::Values,
        "CREATE" => Keyword::Create,
        "TABLE" => Keyword::Table,
        "INT" | "INTEGER" => Keyword::Int,
        "BIGINT" => Keyword::BigInt,
        "VARCHAR" => Keyword::VarChar,
        "BOOL" | "BOOLEAN" => Keyword::Bool,
        "DOUBLE" => Keyword::Double,
        "LIMIT" => Keyword::Limit,
        "ORDER" => Keyword::Order,
        "BY" => Keyword::By,
        "ASC" => Keyword::Asc,
        "DESC" => Keyword::Desc,
        "EXPLAIN" => Keyword::Explain,
        _ => return None,
    })
}

fn match_symbol(bytes: &[u8], i: usize) -> Result<(Symbol, usize)> {
    let two = if i + 1 < bytes.len() {
        &bytes[i..i + 2]
    } else {
        &[]
    };
    Ok(match two {
        b"<>" | b"!=" => (Symbol::Ne, 2),
        b"<=" => (Symbol::Le, 2),
        b">=" => (Symbol::Ge, 2),
        _ => match bytes[i] {
            b'(' => (Symbol::LParen, 1),
            b')' => (Symbol::RParen, 1),
            b',' => (Symbol::Comma, 1),
            b'*' => (Symbol::Star, 1),
            b'+' => (Symbol::Plus, 1),
            b'-' => (Symbol::Minus, 1),
            b'/' => (Symbol::Slash, 1),
            b'%' => (Symbol::Percent, 1),
            b'=' => (Symbol::Eq, 1),
            b'<' => (Symbol::Lt, 1),
            b'>' => (Symbol::Gt, 1),
            b';' => (Symbol::Semicolon, 1),
            b'.' => (Symbol::Dot, 1),
            other => {
                return Err(parse_error(
                    i,
                    &format!("无法识别的字符 '{}'", other as char),
                ))
            }
        },
    })
}

pub fn parse_error(pos: usize, msg: &str) -> Error {
    Error::new(ErrorKind::Parse, format!("位置 {pos}: {msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(sql: &str) -> Vec<TokenKind> {
        tokenize(sql).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn tokenizes_select_with_expressions() {
        let ks = kinds("SELECT a + 1, b FROM t WHERE a >= 10");
        assert!(matches!(ks[0], TokenKind::Keyword(Keyword::Select)));
        assert!(matches!(&ks[1], TokenKind::Ident(s) if s == "a"));
        assert_eq!(ks[2], TokenKind::Symbol(Symbol::Plus));
        assert_eq!(ks[4], TokenKind::Symbol(Symbol::Comma));
        assert_eq!(ks[10], TokenKind::Symbol(Symbol::Ge));
    }

    #[test]
    fn string_and_number_literals() {
        let ks = kinds("'it''s' 3.14 \"x\"");
        assert!(matches!(&ks[0], TokenKind::StringLit(s) if s == "it's"));
        assert!(matches!(&ks[1], TokenKind::Number(s) if s == "3.14"));
        assert!(matches!(&ks[2], TokenKind::StringLit(s) if s == "x"));
    }

    #[test]
    fn comments_and_params() {
        let ks = kinds("SELECT ? -- line\n/* block */ $2");
        assert_eq!(ks[1], TokenKind::Param(0));
        assert_eq!(ks[2], TokenKind::Param(2));
    }

    #[test]
    fn unterminated_string_reports_position() {
        let err = tokenize("SELECT 'abc").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.msg.contains("位置"));
    }
}
