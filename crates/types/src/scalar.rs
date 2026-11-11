//! `Scalar` 与类型检查（docs/02 §2.1、docs/03 L1：类型/表达式/隐式转换基础）。

use crate::datatype::{DataType, TypeClass};
use serde::{Deserialize, Serialize};

/// 标量值（docs/02 §2.1 `Scalar`；Decimal/Date/Time/Json 用规范化表示）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Scalar {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    /// 定点数：未缩放的整数值 + scale（规范化，避免 f64 精度损失）。
    Decimal {
        unscaled: i128,
        scale: u8,
    },
    Bytes(Vec<u8>),
    Str(String),
    /// 日期/时间统一用「自纪元起的规范化整数」，具体历法在 L1 后续补全。
    Temporal(i64),
    Json(String),
}

impl Scalar {
    /// 推断字面量的自然类型（Binder 隐式转换前的第一步）。
    pub fn inferred_type(&self) -> DataType {
        match self {
            Scalar::Null => DataType::Null,
            Scalar::Bool(_) => DataType::Bool,
            Scalar::Int(_) => DataType::Int64,
            Scalar::Float(_) => DataType::Float64,
            Scalar::Decimal { scale, .. } => DataType::Decimal {
                precision: 38,
                scale: *scale,
            },
            Scalar::Bytes(_) => DataType::VarBinary(u32::MAX),
            Scalar::Str(_) => DataType::VarChar(u32::MAX),
            Scalar::Temporal(_) => DataType::Timestamp,
            Scalar::Json(_) => DataType::Json,
        }
    }

    /// 是否可无损/可转换地赋给目标类型（docs/05 AC-TYP-001 隐式转换矩阵）。
    pub fn castable_to(&self, target: &DataType) -> bool {
        let source = self.inferred_type();
        if source == *target || matches!(self, Scalar::Null) {
            return true;
        }
        matches!(
            (source.class(), target.class()),
            (
                TypeClass::SignedInt | TypeClass::UnsignedInt,
                TypeClass::SignedInt
            ) | (
                TypeClass::SignedInt | TypeClass::UnsignedInt,
                TypeClass::Float
            ) | (TypeClass::Float, TypeClass::Float | TypeClass::Decimal)
                | (TypeClass::Decimal, TypeClass::Decimal | TypeClass::Float)
                | (
                    TypeClass::String | TypeClass::Binary,
                    TypeClass::String | TypeClass::Binary
                )
                | (TypeClass::Temporal, TypeClass::Temporal)
        )
    }

    /// 整数溢出检查（docs/05 AC-TYP-003：溢出矩阵）。
    pub fn fits_integer(&self, target: &DataType) -> bool {
        let Scalar::Int(v) = self else {
            return false;
        };
        let (min, max): (i128, i128) = match target {
            DataType::Int8 => (i8::MIN as i128, i8::MAX as i128),
            DataType::Int16 => (i16::MIN as i128, i16::MAX as i128),
            DataType::Int32 => (i32::MIN as i128, i32::MAX as i128),
            DataType::Int64 => (i64::MIN as i128, i64::MAX as i128),
            DataType::UInt8 => (0, u8::MAX as i128),
            DataType::UInt16 => (0, u16::MAX as i128),
            DataType::UInt32 => (0, u32::MAX as i128),
            DataType::UInt64 => (0, u64::MAX as i128),
            _ => return false,
        };
        (min..=max).contains(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_type_inference() {
        assert_eq!(Scalar::Int(1).inferred_type(), DataType::Int64);
        assert_eq!(Scalar::Bool(true).inferred_type(), DataType::Bool);
        assert_eq!(Scalar::Null.inferred_type(), DataType::Null);
        assert_eq!(
            Scalar::Str("x".into()).inferred_type(),
            DataType::VarChar(u32::MAX)
        );
    }

    #[test]
    fn implicit_cast_matrix() {
        assert!(Scalar::Int(1).castable_to(&DataType::Float64));
        assert!(Scalar::Int(1).castable_to(&DataType::Int32));
        assert!(Scalar::Null.castable_to(&DataType::VarChar(10)));
        assert!(!Scalar::Bool(true).castable_to(&DataType::Int64));
    }

    #[test]
    fn integer_overflow_bounds() {
        assert!(Scalar::Int(127).fits_integer(&DataType::Int8));
        assert!(!Scalar::Int(128).fits_integer(&DataType::Int8));
        assert!(!Scalar::Int(-1).fits_integer(&DataType::UInt8));
        assert!(Scalar::Int(255).fits_integer(&DataType::UInt8));
    }

    #[test]
    fn decimal_is_normalized_integer() {
        let d = Scalar::Decimal {
            unscaled: 12345,
            scale: 2,
        };
        assert_eq!(
            d.inferred_type(),
            DataType::Decimal {
                precision: 38,
                scale: 2
            }
        );
    }
}
