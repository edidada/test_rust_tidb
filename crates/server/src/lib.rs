//! `server` crate (L5 Server 与端到端 SQL)。
//! 契约来源：docs/01 §3.1（MySQL wire / TCP 4000）、docs/02 §11；docs/03 L5。
//!
//! 模块：`protocol`（MySQL wire 编解码）、`session`（Session/Prepared/SlowLog）、
//! `connection`（连接状态机与命令分发、TCP 服务）。

pub mod connection;
pub mod protocol;
pub mod session;

pub use connection::{serve_one, server_capabilities, CommandResult, Connection};
pub use protocol::{
    capability, ByteReader, ColumnMeta, ColumnType, Command, MySqlErrorCode, Packet, AUTH_PLUGIN,
    DEFAULT_CHARSET, PROTOCOL_VERSION, SERVER_VERSION,
};
pub use session::{
    count_params, error_to_packet, normalize_sql, sql_digest, Grant, PreparedStatement, Privilege,
    Session, SessionId, SlowLog, SlowLogEntry,
};

use lattice_common::{Config, Result};
use std::sync::Arc;

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

    /// 注入预置表 Catalog（L1/L2 端到端贯通用）。
    pub fn with_catalog(config: Config, catalog: lattice_catalog::MemoryCatalog) -> Self {
        Self {
            config: Arc::new(config),
            catalog: Arc::new(catalog),
        }
    }

    /// 监听地址来自配置（docs/02 §12：禁止硬编码）。
    pub fn listen_addr(&self) -> &str {
        &self.config.server.listen_addr
    }

    /// L2 端到端贯通：SQL → Parser → Binder → LogicalPlanNode。
    pub fn compile(&self, sql: &str) -> Result<lattice_planner::LogicalPlanNode> {
        lattice_catalog::compile_sql(sql, Arc::clone(&self.catalog), "test")
    }

    /// L2 端到端贯通：SQL → EXPLAIN 文本。
    pub fn explain(&self, sql: &str) -> Result<String> {
        lattice_catalog::explain_sql(
            sql,
            Arc::clone(&self.catalog),
            "test",
            lattice_planner::ExplainFormat::Tree,
        )
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

    fn server_with_tables() -> Server {
        let mut catalog = lattice_catalog::MemoryCatalog::new();
        catalog.add_table(lattice_catalog::Table {
            id: lattice_catalog::TableId(1),
            name: lattice_catalog::QualifiedName {
                database: "test".into(),
                table: "t".into(),
            },
            columns: vec![
                ("a".into(), lattice_types::DataType::Int64),
                ("b".into(), lattice_types::DataType::VarChar(32)),
            ],
        });
        Server::with_catalog(Config::default(), catalog)
    }

    #[test]
    fn sql_to_logical_plan_end_to_end() {
        let server = server_with_tables();
        let plan = server
            .compile("SELECT a, b FROM t WHERE a > 10 ORDER BY a DESC LIMIT 5")
            .unwrap();
        // DataSource → Selection → Projection → TopN = 4 层
        assert_eq!(plan.node_count(), 4);
        let ops: Vec<&str> = plan.pre_order().iter().map(|n| n.plan.op_id()).collect();
        assert_eq!(
            ops,
            vec![
                "OP-L-TOPN",
                "OP-L-PROJECTION",
                "OP-L-SELECTION",
                "OP-L-DATASOURCE"
            ]
        );
        assert_eq!(plan.schema.columns.len(), 2);
    }

    #[test]
    fn explain_sql_is_readable() {
        let server = server_with_tables();
        let text = server
            .explain("EXPLAIN SELECT a FROM t WHERE a = 1")
            .unwrap();
        assert!(text.contains("OP-L-PROJECTION"));
        assert!(text.contains("OP-L-SELECTION"));
        assert!(text.contains("OP-L-DATASOURCE"));
        assert!(text.contains("table=test.t"));
    }

    #[test]
    fn compile_reports_bind_errors() {
        let server = server_with_tables();
        let err = server.compile("SELECT nope FROM t").unwrap_err();
        assert_eq!(err.kind, lattice_common::ErrorKind::Bind);
    }

    #[test]
    fn constant_query_uses_table_dual() {
        let server = Server::new(Config::default());
        let plan = server.compile("SELECT 1 + 2 AS x").unwrap();
        assert!(plan
            .pre_order()
            .iter()
            .any(|n| n.plan.op_id() == "OP-L-DUAL"));
    }

    #[test]
    fn explain_json_roundtrips_through_pipeline() {
        let server = server_with_tables();
        let plan = server.compile("SELECT a FROM t").unwrap();
        let json = lattice_catalog::explain_sql(
            "SELECT a FROM t",
            Arc::clone(&server.catalog),
            "test",
            lattice_planner::ExplainFormat::Json,
        )
        .unwrap();
        let back = lattice_planner::LogicalPlanNode::from_json(&json).unwrap();
        assert!(plan.semantically_eq(&back));
    }
}
