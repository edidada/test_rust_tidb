//! 统一错误模型（docs/02 §13）。
//!
//! 所有 crate 使用 `common::Result<T>` 作为错误类型；`ErrorKind` 覆盖从解析、
//! 绑定到事务、存储与内部错误的全部分类，并带可诊断上下文。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 错误分类。可重试错误由 [`Error::retryable`] 判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorKind {
    Parse,
    Bind,
    TypeMismatch,
    Permission,
    PlanRegression,
    OptimizerTimeout,
    RegionEpochNotMatch,
    NotLeader,
    ServerIsBusy,
    KeyIsLocked,
    WriteConflict,
    CommitTsExpired,
    TxnTooLarge,
    OutOfMemory,
    SpillFailed,
    RpcTimeout,
    SchemaChanged,
    StatsStale,
    Internal,
    Config,
    Unsupported,
}

impl ErrorKind {
    /// 该分类是否属于「可重试」错误（docs/02 §13：分类退避重试）。
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::RegionEpochNotMatch
                | ErrorKind::NotLeader
                | ErrorKind::ServerIsBusy
                | ErrorKind::KeyIsLocked
                | ErrorKind::WriteConflict
                | ErrorKind::CommitTsExpired
                | ErrorKind::RpcTimeout
                | ErrorKind::StatsStale
                | ErrorKind::SchemaChanged
        )
    }
}

/// 错误诊断上下文，满足 Plan Replayer 复现（docs/02 §13、04 F-PLAN-009）。
/// 采用 `Box<str>` 让整体尺寸保持精简，避免 `Result<T, Error>` 触发
/// clippy 的 `result_large_err`（CI 以 `-D warnings` 运行）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticContext {
    pub sql_id: Option<Box<str>>,
    pub plan_id: Option<Box<str>>,
    pub region_id: Option<u64>,
    pub ts: Option<u64>,
    pub retry_count: u32,
}

impl DiagnosticContext {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 统一错误类型。消息与 backtrace 均装箱，控制 `Err` 变体大小。
#[derive(Debug, Clone, thiserror::Error)]
#[error("[{kind:?}] {msg}")]
pub struct Error {
    pub kind: ErrorKind,
    pub msg: Box<str>,
    pub context: DiagnosticContext,
    /// 静态字符串形式的 backtrace 占位，避免引入额外依赖。
    pub backtrace: Option<Box<str>>,
}

impl Error {
    pub fn new(kind: ErrorKind, msg: impl Into<String>) -> Self {
        Self {
            kind,
            msg: msg.into().into_boxed_str(),
            context: DiagnosticContext::default(),
            backtrace: Some(
                std::backtrace::Backtrace::capture()
                    .to_string()
                    .into_boxed_str(),
            ),
        }
    }

    pub fn with_context(mut self, context: DiagnosticContext) -> Self {
        self.context = context;
        self
    }

    /// 是否可重试（docs/02 §13）。
    pub fn retryable(&self) -> bool {
        self.kind.is_retryable()
    }

    /// 供错误诊断输出的结构化字段。
    pub fn diagnostic_fields(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind,
            "msg": self.msg.as_ref(),
            "sql_id": self.context.sql_id.as_deref(),
            "plan_id": self.context.plan_id.as_deref(),
            "region_id": self.context.region_id,
            "ts": self.context.ts,
            "retry_count": self.context.retry_count,
            "retryable": self.retryable(),
        })
    }
}

/// 便捷构造函数。
pub fn internal(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::Internal, msg)
}

pub fn unsupported(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::Unsupported, msg)
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification_matches_design() {
        assert!(ErrorKind::KeyIsLocked.is_retryable());
        assert!(ErrorKind::WriteConflict.is_retryable());
        assert!(ErrorKind::RpcTimeout.is_retryable());
        assert!(!ErrorKind::Parse.is_retryable());
        assert!(!ErrorKind::Internal.is_retryable());
    }

    #[test]
    fn diagnostic_context_is_serializable() {
        let ctx = DiagnosticContext {
            sql_id: Some("sql-1".into()),
            plan_id: Some("plan-1".into()),
            region_id: Some(42),
            ts: Some(7),
            retry_count: 3,
        };
        let err = Error::new(ErrorKind::NotLeader, "not leader").with_context(ctx);
        let v = err.diagnostic_fields();
        assert_eq!(v["sql_id"], "sql-1");
        assert_eq!(v["retryable"], true);
    }
}
