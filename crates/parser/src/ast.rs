//! SQL AST（docs/04 F-SQL-005）。Binder 负责把 AstExpr 绑定为 `lattice_expr::Expr`。

use crate::lexer::Keyword;

/// 未绑定的表达式（名称尚未解析）。
#[derive(Debug, Clone, PartialEq)]
pub enum AstExpr {
    Column(Vec<String>),
    Literal(Literal),
    Param(u32),
    Unary {
        op: UnaryOp,
        expr: Box<AstExpr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<AstExpr>,
        rhs: Box<AstExpr>,
    },
    IsNull {
        expr: Box<AstExpr>,
        negated: bool,
    },
    Star,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// SELECT 投影项。
#[derive(Debug, Clone, PartialEq)]
pub struct SelectItem {
    pub expr: AstExpr,
    pub alias: Option<String>,
}

/// 表引用（FROM 子句）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRef {
    pub name: Vec<String>,
    pub alias: Option<String>,
}

/// ORDER BY 项。
#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    pub expr: AstExpr,
    pub desc: bool,
}

/// 语句 AST。
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Select {
        items: Vec<SelectItem>,
        from: Vec<TableRef>,
        selection: Option<AstExpr>,
        order_by: Vec<OrderItem>,
        limit: Option<u64>,
    },
    Insert {
        table: Vec<String>,
        columns: Vec<String>,
        values: Vec<Vec<AstExpr>>,
    },
    CreateTable {
        name: Vec<String>,
        columns: Vec<(String, String)>,
    },
    Explain {
        inner: Box<Statement>,
    },
}

impl Statement {
    pub fn keyword(&self) -> Keyword {
        match self {
            Statement::Select { .. } => Keyword::Select,
            Statement::Insert { .. } => Keyword::Insert,
            Statement::CreateTable { .. } => Keyword::Create,
            Statement::Explain { .. } => Keyword::Explain,
        }
    }
}
