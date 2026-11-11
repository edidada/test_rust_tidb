//! Binder（docs/02 §3、docs/04 F-SQL-007/008、AC-SQL-004、AC-TYP-004）。
//! 名称解析（库/表/列/别名/歧义检查）+ 类型推导 + 隐式转换。

use crate::{CatalogProvider, MemoryCatalog, QualifiedName, SessionCtx};
use lattice_common::{Error, ErrorKind, Result};
use lattice_expr::{ColumnRef, Expr, FuncSig};
use lattice_parser::{AstExpr, BinaryOp, Literal, Statement, UnaryOp};
use lattice_planner::{ColumnMeta, LogicalPlan, LogicalPlanNode, PlanNodeId, Schema};
use lattice_types::{DataType, Scalar};
use std::sync::Arc;

/// 绑定结果：逻辑计划骨架 + 绑定后的表达式（供 L2 逻辑计划使用）。
#[derive(Debug, Clone, PartialEq)]
pub struct BindOutput {
    pub plan: LogicalPlanNode,
    pub projection: Vec<BoundItem>,
    pub selection: Option<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    pub limit: Option<u64>,
    pub statement: Statement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundItem {
    pub expr: Expr,
    pub alias: Option<String>,
    pub data_type: DataType,
}

/// 作用域：FROM 子句可见的列（含表名限定）。
#[derive(Debug, Clone)]
struct ScopeColumn {
    table: String,
    name: String,
    data_type: DataType,
    index: usize,
    id: u64,
}

/// 默认 Binder（docs/02 §3 `trait Binder` 的实现）。
#[derive(Debug)]
pub struct DefaultBinder<C: CatalogProvider = MemoryCatalog> {
    pub catalog: Arc<C>,
    pub default_database: String,
}

impl DefaultBinder<MemoryCatalog> {
    pub fn new() -> Self {
        Self {
            catalog: Arc::new(MemoryCatalog::new()),
            default_database: "test".to_string(),
        }
    }
}

impl Default for DefaultBinder<MemoryCatalog> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: CatalogProvider> DefaultBinder<C> {
    pub fn with_catalog(catalog: C, default_database: impl Into<String>) -> Self {
        Self {
            catalog: Arc::new(catalog),
            default_database: default_database.into(),
        }
    }

    /// 解析 `db.table` / `table` 为限定名。
    fn resolve_table_name(&self, parts: &[String]) -> Result<QualifiedName> {
        match parts.len() {
            1 => Ok(QualifiedName {
                database: self.default_database.clone(),
                table: parts[0].clone(),
            }),
            2 => Ok(QualifiedName {
                database: parts[0].clone(),
                table: parts[1].clone(),
            }),
            _ => Err(Error::new(
                ErrorKind::Bind,
                format!("非法表名 {:?}（仅支持 db.table）", parts),
            )),
        }
    }

    /// 构建可见作用域（docs/04 F-SQL-007：歧义检查）。
    fn build_scope(
        &self,
        from: &[lattice_parser::TableRef],
    ) -> Result<(Vec<ScopeColumn>, Vec<QualifiedName>)> {
        let mut scope = Vec::new();
        let mut tables = Vec::new();
        for (t_idx, table_ref) in from.iter().enumerate() {
            let qname = self.resolve_table_name(&table_ref.name)?;
            let table = self.catalog.table(&qname)?;
            let label = table_ref
                .alias
                .clone()
                .unwrap_or_else(|| qname.table.clone());
            for (index, (name, ty)) in table.columns.iter().enumerate() {
                scope.push(ScopeColumn {
                    table: label.clone(),
                    name: name.clone(),
                    data_type: *ty,
                    index,
                    // 稳定 id：表序号 * 1000 + 列序号（Binder 分配，F-TYP-016）。
                    id: (t_idx as u64) * 1000 + index as u64 + 1,
                });
            }
            tables.push(qname);
        }
        Ok((scope, tables))
    }

    fn resolve_column(&self, scope: &[ScopeColumn], parts: &[String]) -> Result<ScopeColumn> {
        let (qualifier, name) = match parts.len() {
            1 => (None, parts[0].as_str()),
            2 => (Some(parts[0].as_str()), parts[1].as_str()),
            _ => return Err(Error::new(ErrorKind::Bind, format!("非法列名 {:?}", parts))),
        };
        let candidates: Vec<&ScopeColumn> = scope
            .iter()
            .filter(|c| c.name == name && qualifier.map(|q| q == c.table).unwrap_or(true))
            .collect();
        match candidates.len() {
            0 => Err(Error::new(ErrorKind::Bind, format!("未知列 {parts:?}"))),
            1 => Ok(candidates[0].clone()),
            _ => Err(Error::new(
                ErrorKind::Bind,
                format!("列歧义 {parts:?}：匹配 {} 个表", candidates.len()),
            )),
        }
    }

    /// 绑定表达式：AstExpr → lattice_expr::Expr，同时推导结果类型。
    fn bind_expr(&self, expr: &AstExpr, scope: &[ScopeColumn]) -> Result<(Expr, DataType)> {
        Ok(match expr {
            AstExpr::Column(parts) => {
                let col = self.resolve_column(scope, parts)?;
                (
                    Expr::Column(ColumnRef {
                        index: col.index,
                        id: col.id,
                    }),
                    col.data_type,
                )
            }
            AstExpr::Literal(lit) => {
                let (scalar, ty) = match lit {
                    Literal::Null => (Scalar::Null, DataType::Null),
                    Literal::Bool(b) => (Scalar::Bool(*b), DataType::Bool),
                    Literal::Int(i) => (Scalar::Int(*i), DataType::Int64),
                    Literal::Float(f) => (Scalar::Float(*f), DataType::Float64),
                    Literal::Str(s) => (Scalar::Str(s.clone()), DataType::VarChar(s.len() as u32)),
                };
                (Expr::Literal(scalar, ty), ty)
            }
            AstExpr::Param(n) => (Expr::Param(*n), DataType::Null),
            AstExpr::Star => {
                return Err(Error::new(
                    ErrorKind::Bind,
                    "SELECT * 需在投影展开阶段处理（L2）",
                ))
            }
            AstExpr::Unary { op, expr } => {
                let (inner, inner_ty) = self.bind_expr(expr, scope)?;
                match op {
                    UnaryOp::Neg => (Expr::unary(FuncSig::Neg, inner, inner_ty), inner_ty),
                    UnaryOp::Not => (
                        Expr::unary(FuncSig::Not, inner, DataType::Bool),
                        DataType::Bool,
                    ),
                }
            }
            AstExpr::IsNull { expr, negated } => {
                let (inner, _) = self.bind_expr(expr, scope)?;
                let sig = if *negated {
                    FuncSig::IsNotNull
                } else {
                    FuncSig::IsNull
                };
                (Expr::unary(sig, inner, DataType::Bool), DataType::Bool)
            }
            AstExpr::Binary { op, lhs, rhs } => {
                let (l, lt) = self.bind_expr(lhs, scope)?;
                let (r, rt) = self.bind_expr(rhs, scope)?;
                let (sig, ret_ty) = match op {
                    BinaryOp::Add => (FuncSig::Add, promote_numeric(lt, rt)),
                    BinaryOp::Sub => (FuncSig::Sub, promote_numeric(lt, rt)),
                    BinaryOp::Mul => (FuncSig::Mul, promote_numeric(lt, rt)),
                    BinaryOp::Div => (FuncSig::Div, promote_numeric(lt, rt)),
                    BinaryOp::Mod => (FuncSig::Mod, promote_numeric(lt, rt)),
                    BinaryOp::Eq => (FuncSig::Eq, DataType::Bool),
                    BinaryOp::Ne => (FuncSig::Ne, DataType::Bool),
                    BinaryOp::Lt => (FuncSig::Lt, DataType::Bool),
                    BinaryOp::Le => (FuncSig::Le, DataType::Bool),
                    BinaryOp::Gt => (FuncSig::Gt, DataType::Bool),
                    BinaryOp::Ge => (FuncSig::Ge, DataType::Bool),
                    BinaryOp::And => (FuncSig::And, DataType::Bool),
                    BinaryOp::Or => (FuncSig::Or, DataType::Bool),
                };
                // 隐式转换（docs/04 F-SQL-008）：数值二元运算两侧类型对齐。
                let (l, r) = coerce_pair(l, lt, r, rt, ret_ty)?;
                (Expr::binary(sig, l, r, ret_ty), ret_ty)
            }
        })
    }

    /// 绑定 SELECT 语句。
    pub fn bind_select(&self, stmt: &Statement, ctx: &SessionCtx) -> Result<BindOutput> {
        let _ = ctx; // L5 填充权限/变量
        let Statement::Select {
            items,
            from,
            selection,
            order_by,
            limit,
        } = stmt
        else {
            return Err(Error::new(ErrorKind::Bind, "仅支持 SELECT 绑定"));
        };
        let (scope, tables) = self.build_scope(from)?;

        // 投影：SELECT * 展开为所有列。
        let mut projection = Vec::new();
        for item in items {
            if matches!(item.expr, AstExpr::Star) {
                for col in &scope {
                    projection.push(BoundItem {
                        expr: Expr::Column(ColumnRef {
                            index: col.index,
                            id: col.id,
                        }),
                        alias: Some(col.name.clone()),
                        data_type: col.data_type,
                    });
                }
            } else {
                let (expr, ty) = self.bind_expr(&item.expr, &scope)?;
                projection.push(BoundItem {
                    expr,
                    alias: item.alias.clone(),
                    data_type: ty,
                });
            }
        }

        let bound_selection = match selection {
            Some(e) => {
                let (expr, ty) = self.bind_expr(e, &scope)?;
                if ty != DataType::Bool && ty != DataType::Null {
                    return Err(Error::new(
                        ErrorKind::TypeMismatch,
                        "WHERE 必须是布尔表达式",
                    ));
                }
                Some(expr)
            }
            None => None,
        };

        let mut bound_order = Vec::new();
        for o in order_by {
            let (expr, _) = self.bind_expr(&o.expr, &scope)?;
            bound_order.push((expr, o.desc));
        }

        // 逻辑计划骨架：无 FROM → TableDual；有 FROM → DataSource（L2 补全树形结构）。
        let plan = if tables.is_empty() {
            LogicalPlanNode::table_dual(PlanNodeId(1))
        } else {
            let qname = &tables[0];
            let table = self.catalog.table(qname)?;
            LogicalPlanNode {
                id: PlanNodeId(1),
                plan: LogicalPlan::DataSource {
                    table: format!("{}.{}", qname.database, qname.table),
                },
                children: Vec::new(),
                schema: Schema {
                    columns: table
                        .columns
                        .iter()
                        .map(|(name, ty)| ColumnMeta {
                            name: name.clone(),
                            data_type: *ty,
                            nullable: true,
                        })
                        .collect(),
                },
                rule_trace: Vec::new(),
            }
        };

        Ok(BindOutput {
            plan,
            projection,
            selection: bound_selection,
            order_by: bound_order,
            limit: *limit,
            statement: stmt.clone(),
        })
    }
}

impl<C: CatalogProvider> crate::Binder for DefaultBinder<C> {
    fn bind(
        &self,
        stmt: &lattice_parser::AstStatement,
        ctx: &SessionCtx,
    ) -> Result<LogicalPlanNode> {
        let statement = stmt
            .statement()
            .ok_or_else(|| Error::new(ErrorKind::Bind, "占位 AST 无语句内容，无法绑定"))?;
        // EXPLAIN 绑定其内层语句。
        let inner = match statement {
            Statement::Explain { inner } => inner.as_ref(),
            other => other,
        };
        Ok(self.bind_select(inner, ctx)?.plan)
    }
}

/// 数值类型提升（docs/04 F-SQL-008 隐式转换矩阵）。
fn promote_numeric(lhs: DataType, rhs: DataType) -> DataType {
    use DataType::*;
    match (lhs, rhs) {
        (Float64, _) | (_, Float64) => Float64,
        (Float32, _) | (_, Float32) => Float32,
        (Decimal { .. }, _) | (_, Decimal { .. }) => Decimal {
            precision: 38,
            scale: 6,
        },
        (Null, t) | (t, Null) => t,
        _ => Int64,
    }
}

/// 两侧类型对齐：需要时插入 CAST（docs/02 §2.2 `Cast`）。
fn coerce_pair(
    lhs: Expr,
    lt: DataType,
    rhs: Expr,
    rt: DataType,
    target: DataType,
) -> Result<(Expr, Expr)> {
    let wrap = |e: Expr, from: DataType| -> Expr {
        if from == target || from == DataType::Null {
            e
        } else {
            Expr::Cast {
                expr: Box::new(e),
                from,
                to: target,
                in_union: false,
            }
        }
    };
    Ok((wrap(lhs, lt), wrap(rhs, rt)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Table, TableId};
    use lattice_parser::Parser as _;
    use lattice_parser::SqlParser;

    fn catalog_with_tables() -> MemoryCatalog {
        let mut cat = MemoryCatalog::new();
        cat.add_table(Table {
            id: TableId(1),
            name: QualifiedName {
                database: "test".into(),
                table: "t".into(),
            },
            columns: vec![
                ("a".into(), DataType::Int64),
                ("b".into(), DataType::VarChar(32)),
            ],
        });
        cat.add_table(Table {
            id: TableId(2),
            name: QualifiedName {
                database: "test".into(),
                table: "u".into(),
            },
            columns: vec![("a".into(), DataType::Int64)],
        });
        cat
    }

    fn bind(sql: &str, cat: MemoryCatalog) -> Result<BindOutput> {
        let parser = SqlParser;
        let stmts = parser.parse(sql)?;
        let binder = DefaultBinder::with_catalog(cat, "test");
        binder.bind_select(stmts[0].statement().unwrap(), &SessionCtx)
    }

    #[test]
    fn resolves_columns_and_types() {
        let out = bind("SELECT a, b FROM t WHERE a > 10", catalog_with_tables()).unwrap();
        assert_eq!(out.projection.len(), 2);
        assert_eq!(out.projection[0].data_type, DataType::Int64);
        assert_eq!(out.projection[1].data_type, DataType::VarChar(32));
        assert!(out.selection.is_some());
        assert!(matches!(out.plan.plan, LogicalPlan::DataSource { .. }));
    }

    #[test]
    fn select_star_expands_all_columns() {
        let out = bind("SELECT * FROM t", catalog_with_tables()).unwrap();
        assert_eq!(out.projection.len(), 2);
        assert_eq!(out.projection[0].alias.as_deref(), Some("a"));
    }

    #[test]
    fn qualified_column_and_alias() {
        let out = bind("SELECT t.a + 1 AS x FROM test.t", catalog_with_tables()).unwrap();
        assert_eq!(out.projection[0].alias.as_deref(), Some("x"));
        assert_eq!(out.projection[0].data_type, DataType::Int64);
    }

    #[test]
    fn unknown_column_is_bind_error() {
        let err = bind("SELECT nope FROM t", catalog_with_tables()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Bind);
    }

    #[test]
    fn ambiguous_column_is_bind_error() {
        let err = bind("SELECT a FROM t, u", catalog_with_tables()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Bind);
        assert!(err.msg.contains("歧义"));
    }

    #[test]
    fn implicit_cast_for_mixed_numeric_types() {
        let out = bind("SELECT b + 1.5 FROM t", catalog_with_tables()).unwrap();
        // VarChar + Float 提升为 Float64，左侧插入 CAST。
        assert_eq!(out.projection[0].data_type, DataType::Float64);
        assert!(matches!(out.projection[0].expr, Expr::Function { .. }));
    }

    #[test]
    fn where_must_be_boolean() {
        let err = bind("SELECT * FROM t WHERE a + 1", catalog_with_tables()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TypeMismatch);
    }

    #[test]
    fn no_from_produces_table_dual() {
        let out = bind("SELECT 1", MemoryCatalog::new()).unwrap();
        assert!(matches!(out.plan.plan, LogicalPlan::TableDual));
        assert_eq!(out.projection[0].data_type, DataType::Int64);
    }

    #[test]
    fn trait_binder_is_implemented() {
        let parser = SqlParser;
        let stmts = parser.parse("SELECT a FROM t").unwrap();
        let binder = DefaultBinder::with_catalog(catalog_with_tables(), "test");
        let plan = crate::Binder::bind(&binder, &stmts[0], &SessionCtx).unwrap();
        assert!(matches!(plan.plan, LogicalPlan::DataSource { .. }));
    }
}
