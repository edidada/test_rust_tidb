//! 向量化标量求值器（docs/02 §2.2 `ScalarEvaluator`、docs/04 F-TYP-005/008/009）。
//!
//! `DefaultScalarEvaluator` 按 `SelectionVector` 对输入 `Block` 的逻辑行逐行求值，
//! 输出一列 `Column`；NULL 与三值逻辑语义与 docs/02 §2.1 一致。

use crate::ast::{Expr, FuncSig};
use lattice_common::{BlockBase, Error, ErrorKind, Result, SelectionVector};
use lattice_types::{Bool3, Column, DataType, Scalar};
use std::ops::Not;

/// 标量求值器 trait（docs/02 §2.2）。
pub trait ScalarEvaluator {
    fn eval(&self, expr: &Expr, block: &BlockBase<Column>, sel: &SelectionVector)
        -> Result<Column>;
}

/// 默认向量化求值器（下推与上层共用同一实现，docs/02 §14 检查清单）。
#[derive(Debug, Clone, Default)]
pub struct DefaultScalarEvaluator;

impl DefaultScalarEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// 求值单个逻辑行，返回 (值, 三值逻辑状态)。
    fn eval_row(
        &self,
        expr: &Expr,
        block: &BlockBase<Column>,
        row: u32,
    ) -> Result<(Scalar, Bool3)> {
        match expr {
            Expr::Column(cr) => {
                let col = block.columns.get(cr.index).ok_or_else(|| {
                    Error::new(
                        ErrorKind::Internal,
                        format!("表达式引用越界列 {}", cr.index),
                    )
                })?;
                let scalar = col.get(row as usize)?;
                let tri = match &scalar {
                    Scalar::Null => Bool3::Unknown,
                    Scalar::Bool(b) => Bool3::from(Some(*b)),
                    _ => Bool3::True,
                };
                Ok((scalar, tri))
            }
            Expr::Literal(v, _) => {
                let tri = match v {
                    Scalar::Null => Bool3::Unknown,
                    Scalar::Bool(b) => Bool3::from(Some(*b)),
                    _ => Bool3::True,
                };
                Ok((v.clone(), tri))
            }
            Expr::Bool3(b) => Ok((Scalar::Bool(b.is_true()), *b)),
            Expr::Param(idx) => Err(Error::new(
                ErrorKind::Unsupported,
                format!("参数 ${idx} 需在 Prepared 执行期绑定"),
            )),
            Expr::Subquery(_) => Err(Error::new(
                ErrorKind::Unsupported,
                "子查询需先去关联为 Join",
            )),
            Expr::AggCall { .. } | Expr::WindowFunc { .. } => Err(Error::new(
                ErrorKind::Unsupported,
                "聚合/窗口函数需专用算子求值",
            )),
            Expr::Cast {
                expr, from: _, to, ..
            } => {
                let (v, tri) = self.eval_row(expr, block, row)?;
                let casted = cast_scalar(&v, *to)?;
                Ok((casted, tri))
            }
            Expr::Case { when, then_else } => {
                for (cond, val) in when {
                    let (_, tri) = self.eval_row(cond, block, row)?;
                    if tri.is_true() {
                        return self.eval_row(val, block, row);
                    }
                }
                match then_else {
                    Some(e) => self.eval_row(e, block, row),
                    None => Ok((Scalar::Null, Bool3::Unknown)),
                }
            }
            Expr::Function {
                sig,
                args,
                ret_type,
                ..
            } => self.eval_function(*sig, args, *ret_type, block, row),
        }
    }

    fn eval_function(
        &self,
        sig: FuncSig,
        args: &[Expr],
        ret_type: DataType,
        block: &BlockBase<Column>,
        row: u32,
    ) -> Result<(Scalar, Bool3)> {
        // 结果类型提示：比较/逻辑函数必须声明 BOOL（docs/02 §2.2 契约校验）。
        if (sig.is_comparison() || sig.is_logical()) && ret_type != DataType::Bool {
            return Err(Error::new(
                ErrorKind::TypeMismatch,
                format!("{} 必须声明 BOOL 返回类型", sig.name()),
            ));
        }
        // IS NULL / IS NOT NULL 不传播 NULL；COALESCE / IF 单独处理。
        match sig {
            FuncSig::IsNull | FuncSig::IsNotNull => {
                let (v, _) = self.eval_row(&args[0], block, row)?;
                let is_null = matches!(v, Scalar::Null);
                let b = if sig == FuncSig::IsNull {
                    is_null
                } else {
                    !is_null
                };
                return Ok((Scalar::Bool(b), Bool3::from(Some(b))));
            }
            FuncSig::Coalesce => {
                for arg in args {
                    let (v, _) = self.eval_row(arg, block, row)?;
                    if !matches!(v, Scalar::Null) {
                        return Ok((v, Bool3::True));
                    }
                }
                return Ok((Scalar::Null, Bool3::Unknown));
            }
            FuncSig::If => {
                let (_, cond) = self.eval_row(&args[0], block, row)?;
                let branch = if cond.is_true() { &args[1] } else { &args[2] };
                return self.eval_row(branch, block, row);
            }
            _ => {}
        }

        let mut vals = Vec::with_capacity(args.len());
        let mut tris = Vec::with_capacity(args.len());
        for arg in args {
            let (v, t) = self.eval_row(arg, block, row)?;
            vals.push(v);
            tris.push(t);
        }

        if sig.is_logical() {
            return eval_logical(sig, &tris);
        }
        if sig.is_comparison() {
            return eval_comparison(sig, &vals, &tris);
        }
        // 算术/字符串：NULL 传播（docs/04 F-TYP-006）。
        if vals.iter().any(|v| matches!(v, Scalar::Null)) {
            return Ok((Scalar::Null, Bool3::Unknown));
        }
        let value = eval_arithmetic(sig, &vals)?;
        let tri = bool3_of(&value);
        Ok((value, tri))
    }
}

fn bool3_of(value: &Scalar) -> Bool3 {
    match value {
        Scalar::Null => Bool3::Unknown,
        Scalar::Bool(b) => Bool3::from(Some(*b)),
        _ => Bool3::True,
    }
}

fn eval_logical(sig: FuncSig, tris: &[Bool3]) -> Result<(Scalar, Bool3)> {
    let tri = match sig {
        FuncSig::And => tris[0].and(tris[1]),
        FuncSig::Or => tris[0].or(tris[1]),
        FuncSig::Not => tris[0].not(),
        _ => unreachable!("not a logical function"),
    };
    Ok((Scalar::Bool(tri.is_true()), tri))
}

fn eval_comparison(sig: FuncSig, vals: &[Scalar], tris: &[Bool3]) -> Result<(Scalar, Bool3)> {
    // SQL 三值比较：任一为 NULL → Unknown（docs/02 §2.1）。
    if tris.contains(&Bool3::Unknown) {
        return Ok((Scalar::Null, Bool3::Unknown));
    }
    let lhs = &vals[0];
    let rhs = &vals[1];
    let ord = compare_scalars(lhs, rhs)?;
    let b = match sig {
        FuncSig::Eq => ord == std::cmp::Ordering::Equal,
        FuncSig::Ne => ord != std::cmp::Ordering::Equal,
        FuncSig::Lt => ord == std::cmp::Ordering::Less,
        FuncSig::Le => ord != std::cmp::Ordering::Greater,
        FuncSig::Gt => ord == std::cmp::Ordering::Greater,
        FuncSig::Ge => ord != std::cmp::Ordering::Less,
        _ => unreachable!("not a comparison function"),
    };
    let tri = Bool3::from(Some(b));
    Ok((Scalar::Bool(b), tri))
}

/// 标量比较（数值/字符串/布尔），用于统一比较运算符。
fn compare_scalars(lhs: &Scalar, rhs: &Scalar) -> Result<std::cmp::Ordering> {
    let numeric = |v: &Scalar| -> Option<f64> {
        match v {
            Scalar::Int(i) => Some(*i as f64),
            Scalar::Float(f) => Some(*f),
            Scalar::Decimal { unscaled, scale } => {
                Some(*unscaled as f64 / 10f64.powi(*scale as i32))
            }
            _ => None,
        }
    };
    if let (Some(a), Some(b)) = (numeric(lhs), numeric(rhs)) {
        return a
            .partial_cmp(&b)
            .ok_or_else(|| Error::new(ErrorKind::TypeMismatch, "NaN 无法比较"));
    }
    match (lhs, rhs) {
        (Scalar::Str(a), Scalar::Str(b)) => Ok(a.cmp(b)),
        (Scalar::Bool(a), Scalar::Bool(b)) => Ok(a.cmp(b)),
        (Scalar::Bytes(a), Scalar::Bytes(b)) => Ok(a.cmp(b)),
        _ => Err(Error::new(
            ErrorKind::TypeMismatch,
            format!("{lhs:?} 与 {rhs:?} 类型不可比较"),
        )),
    }
}

fn eval_arithmetic(sig: FuncSig, vals: &[Scalar]) -> Result<Scalar> {
    let as_int = |v: &Scalar| -> Result<i128> {
        match v {
            Scalar::Int(i) => Ok(*i),
            _ => Err(Error::new(ErrorKind::TypeMismatch, "算术运算需要整数")),
        }
    };
    match sig {
        FuncSig::Add => Ok(Scalar::Int(as_int(&vals[0])? + as_int(&vals[1])?)),
        FuncSig::Sub => Ok(Scalar::Int(as_int(&vals[0])? - as_int(&vals[1])?)),
        FuncSig::Mul => Ok(Scalar::Int(as_int(&vals[0])? * as_int(&vals[1])?)),
        FuncSig::Div => {
            let d = as_int(&vals[1])?;
            if d == 0 {
                // docs/05 AC-TYP-003：除零语义返回 NULL（MySQL 默认）。
                return Ok(Scalar::Null);
            }
            Ok(Scalar::Int(as_int(&vals[0])? / d))
        }
        FuncSig::Mod => {
            let d = as_int(&vals[1])?;
            if d == 0 {
                return Ok(Scalar::Null);
            }
            Ok(Scalar::Int(as_int(&vals[0])? % d))
        }
        FuncSig::Neg => Ok(Scalar::Int(-as_int(&vals[0])?)),
        FuncSig::Abs => Ok(Scalar::Int(as_int(&vals[0])?.abs())),
        FuncSig::Concat => {
            let mut out = String::new();
            for v in vals {
                match v {
                    Scalar::Str(s) => out.push_str(s),
                    Scalar::Int(i) => out.push_str(&i.to_string()),
                    Scalar::Bool(b) => out.push_str(if *b { "1" } else { "0" }),
                    _ => {
                        return Err(Error::new(
                            ErrorKind::TypeMismatch,
                            "concat 需要字符串/数值参数",
                        ))
                    }
                }
            }
            Ok(Scalar::Str(out))
        }
        FuncSig::Length => match &vals[0] {
            Scalar::Str(s) => Ok(Scalar::Int(s.chars().count() as i128)),
            Scalar::Bytes(b) => Ok(Scalar::Int(b.len() as i128)),
            _ => Err(Error::new(ErrorKind::TypeMismatch, "length 需要字符串")),
        },
        FuncSig::Upper => match &vals[0] {
            Scalar::Str(s) => Ok(Scalar::Str(s.to_uppercase())),
            _ => Err(Error::new(ErrorKind::TypeMismatch, "upper 需要字符串")),
        },
        FuncSig::Lower => match &vals[0] {
            Scalar::Str(s) => Ok(Scalar::Str(s.to_lowercase())),
            _ => Err(Error::new(ErrorKind::TypeMismatch, "lower 需要字符串")),
        },
        _ => Err(Error::new(
            ErrorKind::Unsupported,
            format!("{} 不是标量算术函数", sig.name()),
        )),
    }
}

/// CAST 矩阵（docs/04 F-TYP-008：溢出/截断策略）。
fn cast_scalar(value: &Scalar, to: DataType) -> Result<Scalar> {
    if matches!(value, Scalar::Null) {
        return Ok(Scalar::Null);
    }
    let as_int = |v: &Scalar| -> Result<i128> {
        match v {
            Scalar::Int(i) => Ok(*i),
            Scalar::Bool(b) => Ok(i128::from(*b)),
            Scalar::Str(s) => s
                .parse::<i128>()
                .map_err(|_| Error::new(ErrorKind::TypeMismatch, format!("无法将 '{s}' 转为整数"))),
            _ => Err(Error::new(ErrorKind::TypeMismatch, "不支持的 CAST 源类型")),
        }
    };
    Ok(match to {
        DataType::Bool => match value {
            Scalar::Bool(b) => Scalar::Bool(*b),
            Scalar::Int(i) => Scalar::Bool(*i != 0),
            _ => return Err(Error::new(ErrorKind::TypeMismatch, "无法转为 BOOL")),
        },
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => {
            let i = as_int(value)?;
            if !value.fits_integer(&to) {
                return Err(Error::new(
                    ErrorKind::TypeMismatch,
                    format!("CAST 溢出: {i} 无法写入 {to:?}"),
                ));
            }
            Scalar::Int(i)
        }
        DataType::Float32 | DataType::Float64 => match value {
            Scalar::Int(i) => Scalar::Float(*i as f64),
            Scalar::Float(f) => Scalar::Float(*f),
            _ => return Err(Error::new(ErrorKind::TypeMismatch, "无法转为浮点")),
        },
        DataType::Char(_) | DataType::VarChar(_) => match value {
            Scalar::Str(s) => Scalar::Str(s.clone()),
            Scalar::Int(i) => Scalar::Str(i.to_string()),
            Scalar::Bool(b) => Scalar::Str(if *b { "1" } else { "0" }.into()),
            _ => return Err(Error::new(ErrorKind::TypeMismatch, "无法转为字符串")),
        },
        _ => {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("CAST → {to:?} 将在后续 Lab 实现"),
            ))
        }
    })
}

/// 推断输出列类型：`Expr::Column` 无独立类型信息，取自输入 Block（Binder 已保证）。
fn output_data_type(expr: &Expr, block: &BlockBase<Column>) -> Result<DataType> {
    let declared = expr.data_type();
    if declared != DataType::Null {
        return Ok(declared);
    }
    match expr {
        Expr::Column(cr) => block
            .columns
            .get(cr.index)
            .map(|c| c.data_type)
            .ok_or_else(|| Error::new(ErrorKind::Internal, "表达式引用越界列")),
        Expr::Case { when, then_else } => {
            if let Some((_, t)) = when.first() {
                let ty = output_data_type(t, block)?;
                if ty != DataType::Null {
                    return Ok(ty);
                }
            }
            match then_else {
                Some(e) => output_data_type(e, block),
                None => Ok(DataType::Null),
            }
        }
        Expr::Subquery(_) | Expr::Param(_) => Ok(DataType::Null),
        _ => Ok(DataType::Null),
    }
}

impl ScalarEvaluator for DefaultScalarEvaluator {
    fn eval(
        &self,
        expr: &Expr,
        block: &BlockBase<Column>,
        sel: &SelectionVector,
    ) -> Result<Column> {
        let mut out = Column::empty(output_data_type(expr, block)?);
        for logical in 0..sel.len() {
            let row = sel
                .physical_row(logical)
                .ok_or_else(|| Error::new(ErrorKind::Internal, "selection vector 缺行"))?;
            let (value, _) = self.eval_row(expr, block, row)?;
            out.push(&value)?;
        }
        Ok(out)
    }
}

/// 三值逻辑谓词求值的统一入口（docs/02 §2.1：Filter 仅保留 True）。
pub fn filter_keeps(value: Bool3) -> bool {
    value.is_true()
}

/// 便捷函数：对 Block 全量行求值（selection = identity）。
pub fn eval_all(
    evaluator: &impl ScalarEvaluator,
    expr: &Expr,
    block: &BlockBase<Column>,
) -> Result<Column> {
    let sel = SelectionVector::identity(block.num_rows);
    evaluator.eval(expr, block, &sel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_common::BlockBuilder;
    use lattice_types::{Block, BlockBuilder as TypesBlockBuilder, Column, Scalar};

    fn block_with_ints(values: &[i128], nulls: &[usize]) -> Block {
        let mut col = Column::empty(DataType::Int64);
        for (i, v) in values.iter().enumerate() {
            if nulls.contains(&i) {
                col.push(&Scalar::Null).unwrap();
            } else {
                col.push(&Scalar::Int(*v)).unwrap();
            }
        }
        TypesBlockBuilder::new(values.len())
            .column(col)
            .rows(values.len())
            .build()
            .unwrap()
    }

    fn eval_ints(expr: &Expr, block: &Block) -> Vec<Option<i128>> {
        let ev = DefaultScalarEvaluator;
        let col = eval_all(&ev, expr, block).unwrap();
        (0..col.len())
            .map(|i| match col.get(i).unwrap() {
                Scalar::Int(v) => Some(v),
                Scalar::Null => None,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[test]
    fn arithmetic_with_null_propagation() {
        let block = block_with_ints(&[10, 20, 30], &[1]);
        let expr = Expr::binary(
            FuncSig::Add,
            Expr::column(0, 1),
            Expr::literal(Scalar::Int(5), DataType::Int64),
            DataType::Int64,
        );
        assert_eq!(eval_ints(&expr, &block), vec![Some(15), None, Some(35)]);
    }

    #[test]
    fn division_by_zero_yields_null() {
        let block = block_with_ints(&[10, 20], &[]);
        let expr = Expr::binary(
            FuncSig::Div,
            Expr::column(0, 1),
            Expr::literal(Scalar::Int(0), DataType::Int64),
            DataType::Int64,
        );
        let col = eval_all(&DefaultScalarEvaluator, &expr, &block).unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Null);
    }

    #[test]
    fn comparison_is_three_valued() {
        let block = block_with_ints(&[1, 2, 3], &[1]);
        let expr = Expr::binary(
            FuncSig::Gt,
            Expr::column(0, 1),
            Expr::literal(Scalar::Int(1), DataType::Int64),
            DataType::Bool,
        );
        let col = eval_all(&DefaultScalarEvaluator, &expr, &block).unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Bool(false));
        assert_eq!(col.get(1).unwrap(), Scalar::Null); // NULL → Unknown
        assert_eq!(col.get(2).unwrap(), Scalar::Bool(true));
        assert_eq!(col.null_count(), 1);
    }

    #[test]
    fn logical_and_or_not_truth_tables() {
        let block = BlockBuilder::<Column>::new(1).rows(1).build().unwrap();
        let mk = |sig, a, b| Expr::Function {
            sig,
            args: vec![Expr::Bool3(a), Expr::Bool3(b)],
            ret_type: DataType::Bool,
            nullable: true,
        };
        let col = eval_all(
            &DefaultScalarEvaluator,
            &mk(FuncSig::And, Bool3::Unknown, Bool3::False),
            &block,
        )
        .unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Bool(false));
    }

    #[test]
    fn is_null_does_not_propagate() {
        let block = block_with_ints(&[1, 2], &[1]);
        let expr = Expr::unary(FuncSig::IsNull, Expr::column(0, 1), DataType::Bool);
        let col = eval_all(&DefaultScalarEvaluator, &expr, &block).unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Bool(false));
        assert_eq!(col.get(1).unwrap(), Scalar::Bool(true));
    }

    #[test]
    fn cast_matrix_and_overflow() {
        let block = BlockBuilder::<Column>::new(1).rows(1).build().unwrap();
        let ok = Expr::Cast {
            expr: Box::new(Expr::literal(Scalar::Int(127), DataType::Int64)),
            from: DataType::Int64,
            to: DataType::Int8,
            in_union: false,
        };
        let col = eval_all(&DefaultScalarEvaluator, &ok, &block).unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Int(127));

        let overflow = Expr::Cast {
            expr: Box::new(Expr::literal(Scalar::Int(999), DataType::Int64)),
            from: DataType::Int64,
            to: DataType::Int8,
            in_union: false,
        };
        assert_eq!(
            eval_all(&DefaultScalarEvaluator, &overflow, &block)
                .unwrap_err()
                .kind,
            ErrorKind::TypeMismatch
        );
    }

    #[test]
    fn case_and_coalesce() {
        let block = block_with_ints(&[5, 0], &[]);
        let case = Expr::Case {
            when: vec![(
                Expr::binary(
                    FuncSig::Eq,
                    Expr::column(0, 1),
                    Expr::literal(Scalar::Int(0), DataType::Int64),
                    DataType::Bool,
                ),
                Expr::literal(Scalar::Int(100), DataType::Int64),
            )],
            then_else: Some(Box::new(Expr::column(0, 1))),
        };
        assert_eq!(eval_ints(&case, &block), vec![Some(5), Some(100)]);

        let coalesce = Expr::Function {
            sig: FuncSig::Coalesce,
            args: vec![
                Expr::column(0, 1),
                Expr::literal(Scalar::Int(-1), DataType::Int64),
            ],
            ret_type: DataType::Int64,
            nullable: false,
        };
        let null_block = block_with_ints(&[7], &[0]);
        assert_eq!(eval_ints(&coalesce, &null_block), vec![Some(-1)]);
    }

    #[test]
    fn selection_vector_restricts_rows() {
        let block = block_with_ints(&[1, 2, 3, 4], &[]);
        let expr = Expr::column(0, 1);
        let sel = SelectionVector(vec![0, 2]);
        let col = DefaultScalarEvaluator.eval(&expr, &block, &sel).unwrap();
        assert_eq!(col.len(), 2);
        assert_eq!(col.get(0).unwrap(), Scalar::Int(1));
        assert_eq!(col.get(1).unwrap(), Scalar::Int(3));
    }

    #[test]
    fn string_functions() {
        let block = BlockBuilder::<Column>::new(1).rows(1).build().unwrap();
        let upper = Expr::unary(
            FuncSig::Upper,
            Expr::literal(Scalar::Str("abc".into()), DataType::VarChar(3)),
            DataType::VarChar(3),
        );
        let col = eval_all(&DefaultScalarEvaluator, &upper, &block).unwrap();
        assert_eq!(col.get(0).unwrap(), Scalar::Str("ABC".into()));
    }

    #[test]
    fn filter_keeps_only_true() {
        assert!(filter_keeps(Bool3::True));
        assert!(!filter_keeps(Bool3::False));
        assert!(!filter_keeps(Bool3::Unknown));
    }
}
