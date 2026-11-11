//! `FuncRegistry`（docs/02 §2.2、docs/04 F-TYP-006/009）。
//! 集中定义确定性 / NULL 传播 / 可下推 / SIMD / 代价元数据，供下推判定与 CE 使用。

use crate::ast::FuncSig;
use std::collections::BTreeMap;

/// 函数签名元数据（docs/02 §2.2）。
#[derive(Debug, Clone, PartialEq)]
pub struct FuncMeta {
    pub sig: FuncSig,
    pub name: &'static str,
    pub deterministic: bool,
    /// NULL 传播策略：任一参数 NULL 且返回 NULL（docs/04 F-TYP-006）。
    pub null_propagating: bool,
    pub pushdownable: bool,
    pub simd: bool,
    /// 相对代价（用于 CE / Cop 下推决策，docs/02 §2.2）。
    pub cost: f64,
}

impl FuncMeta {
    fn new(sig: FuncSig, cost: f64) -> Self {
        Self {
            sig,
            name: sig.name(),
            deterministic: true,
            null_propagating: !sig.is_comparison() && !sig.is_logical(),
            pushdownable: true,
            simd: false,
            cost,
        }
    }
}

/// 函数注册表（L1 填充内置函数全集）。
#[derive(Debug, Clone)]
pub struct FuncRegistry {
    funcs: BTreeMap<String, FuncMeta>,
}

impl FuncRegistry {
    /// 构造内置函数全集（docs/04 F-TYP-009：数值/字符串/条件/比较/逻辑）。
    pub fn with_builtins() -> Self {
        let mut reg = Self {
            funcs: BTreeMap::new(),
        };
        for (sig, cost) in [
            (FuncSig::Add, 1.0),
            (FuncSig::Sub, 1.0),
            (FuncSig::Mul, 2.0),
            (FuncSig::Div, 3.0),
            (FuncSig::Mod, 2.0),
            (FuncSig::Neg, 0.5),
            (FuncSig::Abs, 1.0),
            (FuncSig::Eq, 1.0),
            (FuncSig::Ne, 1.0),
            (FuncSig::Lt, 1.0),
            (FuncSig::Le, 1.0),
            (FuncSig::Gt, 1.0),
            (FuncSig::Ge, 1.0),
            (FuncSig::IsNull, 0.5),
            (FuncSig::IsNotNull, 0.5),
            (FuncSig::And, 0.5),
            (FuncSig::Or, 0.5),
            (FuncSig::Not, 0.5),
            (FuncSig::Concat, 2.0),
            (FuncSig::Length, 1.0),
            (FuncSig::Upper, 1.5),
            (FuncSig::Lower, 1.5),
            (FuncSig::Coalesce, 1.0),
            (FuncSig::If, 1.0),
        ] {
            let mut meta = FuncMeta::new(sig, cost);
            if matches!(
                sig,
                FuncSig::IsNull | FuncSig::IsNotNull | FuncSig::Coalesce | FuncSig::If
            ) {
                meta.null_propagating = false;
            }
            if sig.is_comparison() || matches!(sig, FuncSig::Add | FuncSig::Sub) {
                meta.simd = true;
            }
            reg.funcs.insert(meta.name.to_string(), meta);
        }
        reg
    }

    pub fn new() -> Self {
        Self::with_builtins()
    }

    pub fn register(&mut self, meta: FuncMeta) {
        self.funcs.insert(meta.name.to_string(), meta);
    }

    pub fn lookup(&self, name: &str) -> Option<&FuncMeta> {
        self.funcs.get(name)
    }

    pub fn lookup_sig(&self, sig: FuncSig) -> Option<&FuncMeta> {
        self.funcs.get(sig.name())
    }

    pub fn pushdownable(&self, name: &str) -> bool {
        self.lookup(name).map(|f| f.pushdownable).unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.funcs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.funcs.is_empty()
    }
}

impl Default for FuncRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_cover_all_signatures() {
        let reg = FuncRegistry::with_builtins();
        assert_eq!(reg.len(), 24);
        assert!(reg.pushdownable("+"));
        assert!(reg.lookup("isnull").unwrap().simd);
    }

    #[test]
    fn metadata_is_correct_per_family() {
        let reg = FuncRegistry::new();
        assert!(!reg.lookup("isnull").unwrap().null_propagating);
        assert!(reg.lookup("+").unwrap().null_propagating);
        assert!(reg.lookup("+").unwrap().simd);
        assert!(!reg.lookup("concat").unwrap().simd);
        assert!(reg.lookup("=").unwrap().deterministic);
    }

    #[test]
    fn custom_registration_overrides() {
        let mut reg = FuncRegistry::with_builtins();
        reg.register(FuncMeta {
            sig: FuncSig::Add,
            name: "+",
            deterministic: false,
            null_propagating: true,
            pushdownable: false,
            simd: false,
            cost: 99.0,
        });
        assert!(!reg.pushdownable("+"));
        assert_eq!(reg.lookup("+").unwrap().cost, 99.0);
    }
}
