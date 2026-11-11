//! `DataType` 与类型分类（docs/02 §2.1、docs/03 L1）。

use serde::{Deserialize, Serialize};

/// SQL 数据类型（docs/02 §2.1 全量枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Null,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Decimal { precision: u8, scale: u8 },
    Date,
    Time,
    DateTime,
    Timestamp,
    Duration,
    Year,
    Char(u32),
    VarChar(u32),
    Binary(u32),
    VarBinary(u32),
    Json,
    Enum,
    Set,
    Bit,
    Vector(u32),
}

/// 类型族：Binder 隐式转换与运算符重载的基础分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    Null,
    Boolean,
    SignedInt,
    UnsignedInt,
    Float,
    Decimal,
    Temporal,
    String,
    Binary,
    SemiStructured,
}

impl DataType {
    pub fn class(&self) -> TypeClass {
        match self {
            DataType::Null => TypeClass::Null,
            DataType::Bool => TypeClass::Boolean,
            DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64 => {
                TypeClass::SignedInt
            }
            DataType::UInt8 | DataType::UInt16 | DataType::UInt32 | DataType::UInt64 => {
                TypeClass::UnsignedInt
            }
            DataType::Float32 | DataType::Float64 => TypeClass::Float,
            DataType::Decimal { .. } => TypeClass::Decimal,
            DataType::Date
            | DataType::Time
            | DataType::DateTime
            | DataType::Timestamp
            | DataType::Duration
            | DataType::Year => TypeClass::Temporal,
            DataType::Char(_) | DataType::VarChar(_) => TypeClass::String,
            DataType::Binary(_) | DataType::VarBinary(_) | DataType::Bit => TypeClass::Binary,
            DataType::Json | DataType::Enum | DataType::Set | DataType::Vector(_) => {
                TypeClass::SemiStructured
            }
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self.class(),
            TypeClass::SignedInt | TypeClass::UnsignedInt | TypeClass::Float | TypeClass::Decimal
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(self.class(), TypeClass::SignedInt | TypeClass::UnsignedInt)
    }

    /// 定长值字节宽度；变长类型返回 `None`。
    pub fn fixed_width(&self) -> Option<usize> {
        match self {
            DataType::Int8 | DataType::UInt8 => Some(1),
            DataType::Int16 | DataType::UInt16 => Some(2),
            DataType::Int32 | DataType::UInt32 | DataType::Float32 => Some(4),
            DataType::Int64 | DataType::UInt64 | DataType::Float64 => Some(8),
            DataType::Bool => Some(1),
            _ => None,
        }
    }

    /// DECIMAL 精度校验（docs/05 AC-TYP-003：精度/溢出矩阵）。
    pub fn validate_decimal(&self) -> Result<(), String> {
        match self {
            DataType::Decimal { precision, scale } => {
                if *precision == 0 || *precision > 65 {
                    return Err(format!("precision {precision} 必须落在 1..=65"));
                }
                if *scale > *precision {
                    return Err(format!("scale {scale} 不能大于 precision {precision}"));
                }
                Ok(())
            }
            other => Err(format!("{other:?} 不是 Decimal")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_covers_all_families() {
        assert_eq!(DataType::Int64.class(), TypeClass::SignedInt);
        assert_eq!(DataType::UInt32.class(), TypeClass::UnsignedInt);
        assert_eq!(DataType::Float64.class(), TypeClass::Float);
        assert_eq!(
            DataType::Decimal {
                precision: 10,
                scale: 2
            }
            .class(),
            TypeClass::Decimal
        );
        assert_eq!(DataType::VarChar(255).class(), TypeClass::String);
        assert_eq!(DataType::Json.class(), TypeClass::SemiStructured);
        assert_eq!(DataType::Timestamp.class(), TypeClass::Temporal);
    }

    #[test]
    fn numeric_and_integer_predicates() {
        assert!(DataType::Decimal {
            precision: 10,
            scale: 2
        }
        .is_numeric());
        assert!(DataType::Int16.is_integer());
        assert!(!DataType::Float32.is_integer());
        assert!(!DataType::VarChar(10).is_numeric());
    }

    #[test]
    fn fixed_width_only_for_fixed_types() {
        assert_eq!(DataType::Int32.fixed_width(), Some(4));
        assert_eq!(DataType::Float64.fixed_width(), Some(8));
        assert_eq!(DataType::VarBinary(16).fixed_width(), None);
    }

    #[test]
    fn decimal_validation() {
        assert!(DataType::Decimal {
            precision: 10,
            scale: 2
        }
        .validate_decimal()
        .is_ok());
        assert!(DataType::Decimal {
            precision: 10,
            scale: 11
        }
        .validate_decimal()
        .is_err());
        assert!(DataType::Decimal {
            precision: 0,
            scale: 0
        }
        .validate_decimal()
        .is_err());
    }
}
