//! 配置系统（docs/02 §12：全部可经环境变量 / `bench/env.toml` / Session 变量覆盖）。
//!
//! L0 提供分层配置：默认值 → TOML 文件 → `LATTICE_` 环境变量 → 运行时覆盖。
//! 代价常数、统计与选择性一律经 [`OptimizerConfig`] 注入，禁止硬编码
//! （docs/02 §14 检查清单）。

use crate::error::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 顶层配置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub optimizer: OptimizerConfig,
    pub bench: BenchConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// MySQL wire 监听地址（docs/01 §3.1，TCP 4000）。
    pub listen_addr: String,
    pub parallelism_default: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// `memory` | `rocksdb`
    pub engine: String,
    pub data_dir: PathBuf,
    pub block_cache_bytes: u64,
}

/// 优化器与执行相关的全部可调参数（docs/02 §12 `[待标定]`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizerConfig {
    pub chunk_capacity: usize,
    pub cost_weights: CostWeights,
    pub broadcast_threshold: u64,
    pub spill_threshold: u64,
    pub memory_quota: u64,
    pub default_selectivity: f64,
    pub stats_stale_ratio: f64,
    pub max_rbo_iterations: u32,
    pub cbo_timeout_ms: u64,
    pub join_reorder_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CostWeights {
    pub cpu: f64,
    pub io: f64,
    pub net: f64,
    pub mem: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BenchConfig {
    /// 实验默认种子（docs/03 L0：随机数种子策略）。
    pub seed: u64,
    /// 基准重复次数（05 §7.2：≥5 次，输出均值/方差）。
    pub repeat: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:4000".into(),
            parallelism_default: 1,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            engine: "memory".into(),
            data_dir: PathBuf::from("data"),
            block_cache_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            chunk_capacity: 1024,
            cost_weights: CostWeights::default(),
            broadcast_threshold: 64 * 1024 * 1024,
            spill_threshold: 256 * 1024 * 1024,
            memory_quota: 512 * 1024 * 1024,
            default_selectivity: 0.2,
            stats_stale_ratio: 0.1,
            max_rbo_iterations: 100,
            cbo_timeout_ms: 5_000,
            join_reorder_limit: 12,
        }
    }
}

impl Default for CostWeights {
    fn default() -> Self {
        Self {
            cpu: 1.0,
            io: 10.0,
            net: 50.0,
            mem: 2.0,
        }
    }
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            seed: crate::seed::DEFAULT_SEED,
            repeat: 5,
        }
    }
}

/// 配置加载器：默认值 → 文件 → `LATTICE_` 环境变量。
#[derive(Debug, Clone, Default)]
pub struct ConfigLoader {
    file: Option<PathBuf>,
}

impl ConfigLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(path: impl AsRef<Path>) -> Self {
        Self {
            file: Some(path.as_ref().to_path_buf()),
        }
    }

    /// 加载配置；文件不存在时使用默认值（不视为错误）。
    pub fn load(&self) -> Result<Config> {
        let mut cfg = if let Some(path) = &self.file {
            if path.exists() {
                let text = std::fs::read_to_string(path).map_err(|e| {
                    Error::new(
                        ErrorKind::Config,
                        format!("读取配置 {} 失败: {e}", path.display()),
                    )
                })?;
                toml_str_to_config(&text)?
            } else {
                Config::default()
            }
        } else {
            Config::default()
        };
        cfg.apply_env();
        cfg.validate()?;
        Ok(cfg)
    }

    /// 解析一段 TOML 配置文本（供单测与 `bench/env.toml` 使用）。
    pub fn parse_str(text: &str) -> Result<Config> {
        let cfg = toml_str_to_config(text)?;
        cfg.validate()?;
        Ok(cfg)
    }
}

/// 极简 TOML 解析：仅识别本配置树使用到的 `key = value` 与 `[section]` 语法，
/// 避免 L0 引入外部 TOML 依赖（AC-TEST-002 依赖方向检查保持简单）。
fn toml_str_to_config(text: &str) -> Result<Config> {
    let mut cfg = Config::default();
    let mut section = String::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(Error::new(
                ErrorKind::Config,
                format!("第 {} 行不是合法的 key = value: {raw}", lineno + 1),
            ));
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        apply_key(&mut cfg, &section, key, value, lineno + 1)?;
    }
    Ok(cfg)
}

fn apply_key(cfg: &mut Config, section: &str, key: &str, value: &str, line: usize) -> Result<()> {
    let parsed_f64 = || -> Result<f64> {
        value.parse::<f64>().map_err(|_| {
            Error::new(
                ErrorKind::Config,
                format!("第 {line} 行 {key} 不是数字: {value}"),
            )
        })
    };
    let parsed_u64 = || -> Result<u64> {
        value.parse::<u64>().map_err(|_| {
            Error::new(
                ErrorKind::Config,
                format!("第 {line} 行 {key} 不是正整数: {value}"),
            )
        })
    };
    let parsed_usize = || -> Result<usize> {
        value.parse::<usize>().map_err(|_| {
            Error::new(
                ErrorKind::Config,
                format!("第 {line} 行 {key} 不是正整数: {value}"),
            )
        })
    };
    match (section, key) {
        ("server", "listen_addr") => cfg.server.listen_addr = value.to_string(),
        ("server", "parallelism_default") => cfg.server.parallelism_default = parsed_usize()?,
        ("storage", "engine") => cfg.storage.engine = value.to_string(),
        ("storage", "data_dir") => cfg.storage.data_dir = PathBuf::from(value),
        ("storage", "block_cache_bytes") => cfg.storage.block_cache_bytes = parsed_u64()?,
        ("optimizer", "chunk_capacity") => cfg.optimizer.chunk_capacity = parsed_usize()?,
        ("optimizer", "broadcast_threshold") => cfg.optimizer.broadcast_threshold = parsed_u64()?,
        ("optimizer", "spill_threshold") => cfg.optimizer.spill_threshold = parsed_u64()?,
        ("optimizer", "memory_quota") => cfg.optimizer.memory_quota = parsed_u64()?,
        ("optimizer", "default_selectivity") => cfg.optimizer.default_selectivity = parsed_f64()?,
        ("optimizer", "stats_stale_ratio") => cfg.optimizer.stats_stale_ratio = parsed_f64()?,
        ("optimizer", "max_rbo_iterations") => {
            cfg.optimizer.max_rbo_iterations = parsed_u64()? as u32
        }
        ("optimizer", "cbo_timeout_ms") => cfg.optimizer.cbo_timeout_ms = parsed_u64()?,
        ("optimizer", "join_reorder_limit") => cfg.optimizer.join_reorder_limit = parsed_usize()?,
        ("optimizer.cost_weights", "cpu") => cfg.optimizer.cost_weights.cpu = parsed_f64()?,
        ("optimizer.cost_weights", "io") => cfg.optimizer.cost_weights.io = parsed_f64()?,
        ("optimizer.cost_weights", "net") => cfg.optimizer.cost_weights.net = parsed_f64()?,
        ("optimizer.cost_weights", "mem") => cfg.optimizer.cost_weights.mem = parsed_f64()?,
        ("bench", "seed") => cfg.bench.seed = parsed_u64()?,
        ("bench", "repeat") => cfg.bench.repeat = parsed_usize()?,
        _ => {
            return Err(Error::new(
                ErrorKind::Config,
                format!("第 {line} 行未知配置项: [{section}] {key}"),
            ))
        }
    }
    Ok(())
}

impl Config {
    /// 用 `LATTICE_` 前缀环境变量覆盖（点分路径用双下划线分隔）。
    pub fn apply_env(&mut self) {
        for (key, value) in std::env::vars() {
            let Some(rest) = key.strip_prefix("LATTICE_") else {
                continue;
            };
            let parts: Vec<String> = rest
                .to_lowercase()
                .split("__")
                .map(|s| s.to_string())
                .collect();
            let section = if parts.len() > 1 {
                parts[..parts.len() - 1].join(".")
            } else {
                String::new()
            };
            let leaf = parts.last().cloned().unwrap_or_default();
            // 环境变量覆盖失败不致命，忽略非法值。
            let _ = apply_key(self, &section, &leaf, &value, 0);
        }
    }

    fn validate(&self) -> Result<()> {
        if self.optimizer.chunk_capacity == 0 {
            return Err(Error::new(ErrorKind::Config, "chunk_capacity 必须 > 0"));
        }
        if !(0.0..=1.0).contains(&self.optimizer.default_selectivity) {
            return Err(Error::new(
                ErrorKind::Config,
                "default_selectivity 必须落在 [0, 1]",
            ));
        }
        if self.bench.repeat == 0 {
            return Err(Error::new(ErrorKind::Config, "bench.repeat 必须 > 0"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(Config::default().validate().is_ok());
    }

    #[test]
    fn parses_bench_env_toml_subset() {
        let text = r#"
[server]
listen_addr = "0.0.0.0:4000"

[optimizer]
chunk_capacity = 2048
default_selectivity = 0.5

[optimizer.cost_weights]
io = 20.0

[bench]
seed = 20261109
repeat = 7
"#;
        let cfg = ConfigLoader::parse_str(text).unwrap();
        assert_eq!(cfg.server.listen_addr, "0.0.0.0:4000");
        assert_eq!(cfg.optimizer.chunk_capacity, 2048);
        assert_eq!(cfg.optimizer.cost_weights.io, 20.0);
        assert_eq!(cfg.optimizer.cost_weights.cpu, 1.0);
        assert_eq!(cfg.bench.seed, 20261109);
        assert_eq!(cfg.bench.repeat, 7);
    }

    #[test]
    fn rejects_unknown_key() {
        let err = ConfigLoader::parse_str("[server]\nnope = 1").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Config);
    }

    #[test]
    fn rejects_invalid_selectivity() {
        let err = ConfigLoader::parse_str("[optimizer]\ndefault_selectivity = 2.0").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Config);
    }
}
