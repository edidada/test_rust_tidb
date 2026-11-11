//! 三值逻辑（docs/02 §2.1：Predicate 求值为 Bool3，Filter 仅保留 True）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bool3 {
    True,
    False,
    Unknown,
}

impl Bool3 {
    /// SQL AND 真值表（FALSE 支配，UNKNOWN 次之）。
    pub fn and(self, rhs: Bool3) -> Bool3 {
        use Bool3::*;
        match (self, rhs) {
            (False, _) | (_, False) => False,
            (True, True) => True,
            _ => Unknown,
        }
    }

    /// SQL OR 真值表（TRUE 支配，UNKNOWN 次之）。
    pub fn or(self, rhs: Bool3) -> Bool3 {
        use Bool3::*;
        match (self, rhs) {
            (True, _) | (_, True) => True,
            (False, False) => False,
            _ => Unknown,
        }
    }

    /// NULL 传播的等值比较（=）：任一为 NULL 则 Unknown。
    pub fn eq_opt(lhs: Option<bool>) -> Bool3 {
        match lhs {
            Some(true) => Bool3::True,
            Some(false) => Bool3::False,
            None => Bool3::Unknown,
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, Bool3::True)
    }
}

impl std::ops::Not for Bool3 {
    type Output = Bool3;

    fn not(self) -> Bool3 {
        match self {
            Bool3::True => Bool3::False,
            Bool3::False => Bool3::True,
            Bool3::Unknown => Bool3::Unknown,
        }
    }
}

/// 从 `Option<bool>` 构造，供表达式求值与 Filter 共用。
impl From<Option<bool>> for Bool3 {
    fn from(value: Option<bool>) -> Self {
        match value {
            Some(true) => Bool3::True,
            Some(false) => Bool3::False,
            None => Bool3::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Not;

    #[test]
    fn sql_and_or_not_truth_tables() {
        use Bool3::*;
        assert_eq!(True.and(Unknown), Unknown);
        assert_eq!(False.and(Unknown), False);
        assert_eq!(True.or(Unknown), True);
        assert_eq!(False.or(Unknown), Unknown);
        assert_eq!(Unknown.not(), Unknown);
        assert_eq!(True.not(), False);
        assert_eq!(False.not(), True);
    }

    #[test]
    fn null_propagation() {
        assert_eq!(Bool3::eq_opt(None), Bool3::Unknown);
        assert_eq!(Bool3::from(Some(true)), Bool3::True);
        assert_eq!(Bool3::eq_opt(Some(false)), Bool3::False);
    }

    #[test]
    fn only_true_survives_filter() {
        assert!(Bool3::True.is_true());
        assert!(!Bool3::False.is_true());
        assert!(!Bool3::Unknown.is_true());
    }
}
