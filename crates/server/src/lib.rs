//! `server` crate (L5/L9 契约骨架，L0 空壳)。
//! 契约来源：docs/01 §3.1（MySQL wire / TCP 4000）、docs/02 §11；docs/03 L5。

use lattice_common::{Config, Result};
use std::sync::Arc;

/// 会话占位（L5 填充 Prepared Statement / 变量 / 权限）。
#[derive(Debug, Clone, Default)]
pub struct Session;

/// 服务端骨架：L0 仅验证分层装配与配置注入，不监听端口。
#[derive(Debug, Clone)]
pub struct Server {
    pub config: Arc<Config>,
    pub catalog: Arc<lattice_catalog::MemoryCatalog>,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            catalog: Arc::new(lattice_catalog::MemoryCatalog::new()),
        }
    }

    /// 监听地址来自配置（docs/02 §12：禁止硬编码）。
    pub fn listen_addr(&self) -> &str {
        &self.config.server.listen_addr
    }

    /// L0 装配自检：所有下层 crate 经 trait 注入可达。
    pub fn assemble(&self) -> Result<Assembly> {
        Ok(Assembly {
            listen_addr: self.listen_addr().to_string(),
            kv: Arc::new(lattice_storage::MemoryKV::new()),
            tso: Arc::new(lattice_meta::MemoryTso::new(1)),
            parser: Arc::new(lattice_parser::PlaceholderParser),
            rbo: Arc::new(lattice_optimizer::IdentityRbo),
            executor: Arc::new(lattice_executor::TableDualExecutor::default()),
        })
    }
}

/// 分层装配结果（依赖方向：server → executor → optimizer → planner → expr → types → common，
/// storage/meta 经 trait 注入到 executor）。
#[derive(Debug, Clone)]
pub struct Assembly {
    pub listen_addr: String,
    pub kv: Arc<lattice_storage::MemoryKV>,
    pub tso: Arc<lattice_meta::MemoryTso>,
    pub parser: Arc<lattice_parser::PlaceholderParser>,
    pub rbo: Arc<lattice_optimizer::IdentityRbo>,
    pub executor: Arc<lattice_executor::TableDualExecutor>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_meta::TimestampOracle as _;
    use lattice_parser::Parser as _;
    use lattice_storage::KvEngine as _;

    #[test]
    fn server_uses_injected_config() {
        let mut cfg = Config::default();
        cfg.server.listen_addr = "0.0.0.0:5000".into();
        let server = Server::new(cfg);
        assert_eq!(server.listen_addr(), "0.0.0.0:5000");
        assert_eq!(server.assemble().unwrap().listen_addr, "0.0.0.0:5000");
    }

    #[test]
    fn assembly_crosses_all_layers() {
        let assembly = Server::new(Config::default()).assemble().unwrap();
        let stmts = assembly.parser.parse("select 1").unwrap();
        assert_eq!(stmts.len(), 1);
        assembly.tso.get_ts(1).unwrap();
        assembly.kv.put(b"k".to_vec(), b"v".to_vec(), 1).unwrap();
    }
}
