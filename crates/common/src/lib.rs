//! `common` crate (L0).
//!
//! 全项目共享的基础设施：统一错误类型 [`Error`] / [`Result`]、配置系统
//! [`Config`] / [`ConfigLoader`]、以及 [`Block`] 基础工具（列式批骨架）。
//! 契约来源：docs/02-high-level-design.md §2.1 / §11 / §12 / §13 / §14。

pub mod block;
pub mod config;
pub mod error;
pub mod observability;
pub mod seed;

pub use block::{Block as BlockBase, BlockBuilder, Buffer, SelectionVector, Validity};
pub use config::{
    BenchConfig, Config, ConfigLoader, CostWeights, OptimizerConfig, ServerConfig, StorageConfig,
};
pub use error::{internal, unsupported, DiagnosticContext, Error, ErrorKind, Result};
pub use observability::{MetricValue, MetricsRegistry};
pub use seed::{ExperimentMetadata, SeedPolicy, DEFAULT_SEED};
