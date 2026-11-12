//! W19 Session / Prepared Statement / 系统变量 / 权限 / SQL 归一化
//! （docs/04 F-SQL-003/004/009/010/015、F-OBS-003、AC-SQL-002/004、AC-OBS-003）。

use crate::protocol::{MySqlErrorCode, Packet};
use lattice_common::{Error, ErrorKind, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// 会话 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u32);

/// 会话状态（docs/02 §10：tokio task per connection，Session 隔离）。
#[derive(Debug)]
pub struct Session {
    pub id: SessionId,
    pub username: String,
    pub database: Option<String>,
    pub charset: u8,
    pub autocommit: bool,
    /// 系统变量（影响计划的变量纳入 cache key，docs/02 §5）。
    pub variables: HashMap<String, String>,
    /// Prepared Statement 表（stmt_id → 参数化 SQL）。
    pub prepared: HashMap<u32, PreparedStatement>,
    /// 权限集合（库/表级，docs/04 F-SQL-009）。
    pub grants: Vec<Grant>,
    next_stmt_id: AtomicU32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStatement {
    pub id: u32,
    pub sql: String,
    pub normalized: String,
    pub num_params: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub database: String,
    pub table: String,
    pub privilege: Privilege,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privilege {
    Select,
    Insert,
    Update,
    Delete,
    All,
}

impl Session {
    pub fn new(id: SessionId, username: impl Into<String>) -> Self {
        let mut variables = HashMap::new();
        variables.insert("sql_mode".into(), "STRICT_TRANS_TABLES".into());
        variables.insert("time_zone".into(), "SYSTEM".into());
        variables.insert("character_set_client".into(), "utf8mb4".into());
        variables.insert("max_execution_time".into(), "0".into());
        Self {
            id,
            username: username.into(),
            database: None,
            charset: 45,
            autocommit: true,
            variables,
            prepared: HashMap::new(),
            grants: Vec::new(),
            next_stmt_id: AtomicU32::new(1),
        }
    }

    /// 影响计划的系统变量（纳入 PlanCache key，docs/02 §5）。
    pub fn plan_sensitive_vars(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .variables
            .iter()
            .filter(|(k, _)| matches!(k.as_str(), "sql_mode" | "time_zone" | "max_execution_time"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.sort();
        out
    }

    pub fn set_variable(&mut self, name: &str, value: &str) -> Result<()> {
        if name.is_empty() {
            return Err(Error::new(ErrorKind::Parse, "变量名不能为空"));
        }
        self.variables.insert(name.to_string(), value.to_string());
        Ok(())
    }

    pub fn get_variable(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }

    /// 注册 Prepared Statement（COM_STMT_PREPARE）。
    pub fn prepare(&mut self, sql: &str) -> Result<PreparedStatement> {
        let normalized = normalize_sql(sql);
        let num_params = count_params(&normalized) as u16;
        let id = self.next_stmt_id.fetch_add(1, Ordering::SeqCst);
        let stmt = PreparedStatement {
            id,
            sql: sql.to_string(),
            normalized: normalized.clone(),
            num_params,
        };
        self.prepared.insert(id, stmt.clone());
        Ok(stmt)
    }

    pub fn get_prepared(&self, id: u32) -> Result<&PreparedStatement> {
        self.prepared.get(&id).ok_or_else(|| {
            Error::new(
                ErrorKind::Unsupported,
                format!("未找到 Prepared Statement {id}"),
            )
        })
    }

    /// COM_STMT_CLOSE。
    pub fn close_prepared(&mut self, id: u32) {
        self.prepared.remove(&id);
    }

    pub fn grant(&mut self, grant: Grant) {
        self.grants.push(grant);
    }

    /// 权限检查（docs/04 F-SQL-009、AC-SQL-004）。
    pub fn check_privilege(&self, database: &str, table: &str, privilege: Privilege) -> Result<()> {
        // 空 grants 表示默认允许（单用户实验形态）。
        if self.grants.is_empty() {
            return Ok(());
        }
        let ok = self.grants.iter().any(|g| {
            (g.database == database || g.database == "*")
                && (g.table == table || g.table == "*")
                && (g.privilege == Privilege::All || g.privilege == privilege)
        });
        if ok {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Permission,
                format!(
                    "权限拒绝: {} {}.{}",
                    privilege_name(privilege),
                    database,
                    table
                ),
            ))
        }
    }
}

fn privilege_name(p: Privilege) -> &'static str {
    match p {
        Privilege::Select => "SELECT",
        Privilege::Insert => "INSERT",
        Privilege::Update => "UPDATE",
        Privilege::Delete => "DELETE",
        Privilege::All => "ALL",
    }
}

/// SQL 归一化（docs/04 F-OBS-003/F-SQL-015）：去注释、折叠空白、字面量替换为 `?`，
/// 用于 Slow Log 摘要与 PlanCache key。
pub fn normalize_sql(sql: &str) -> String {
    // 1. 去块注释与行注释。
    let mut cleaned = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0usize;
    let mut in_string: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_string {
            cleaned.push(b as char);
            if b == q {
                // SQL '' 转义
                if i + 1 < bytes.len() && bytes[i + 1] == q {
                    cleaned.push(b as char);
                    i += 2;
                    continue;
                }
                in_string = None;
            }
            i += 1;
            continue;
        }
        if b == b'\'' || b == b'"' {
            in_string = Some(b);
            cleaned.push(b as char);
            i += 1;
            continue;
        }
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            cleaned.push(' ');
            continue;
        }
        cleaned.push(b as char);
        i += 1;
    }

    // 2. 字面量替换为 ?，折叠空白，关键字大写（摘要稳定性）。
    let mut out = String::new();
    let mut j = 0usize;
    let cb = cleaned.as_bytes();
    let mut last_space = false;
    while j < cb.len() {
        let b = cb[j];
        if b == b'\'' || b == b'"' {
            let q = b;
            j += 1;
            while j < cb.len() {
                if cb[j] == q {
                    if j + 1 < cb.len() && cb[j + 1] == q {
                        j += 2;
                        continue;
                    }
                    j += 1;
                    break;
                }
                j += 1;
            }
            out.push('?');
            last_space = false;
            continue;
        }
        if b.is_ascii_digit() {
            while j < cb.len() && (cb[j].is_ascii_digit() || cb[j] == b'.') {
                j += 1;
            }
            out.push('?');
            last_space = false;
            continue;
        }
        if b.is_ascii_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
                last_space = true;
            }
            j += 1;
            continue;
        }
        out.push(b.to_ascii_uppercase() as char);
        last_space = false;
        j += 1;
    }
    // 3. 规整运算符/分隔符周边空白，保证字面量参数化后的摘要稳定。
    let mut compact = out.trim().to_string();
    for op in [
        " = ", " , ", "( ", " )", " + ", " - ", " * ", " / ", " < ", " > ", " <= ", " >= ", " <> ",
    ] {
        let replacement = op.replace(' ', "");
        compact = compact.replace(op, &replacement);
    }
    compact
}

/// 统计参数占位符数量（Prepared 用）。
pub fn count_params(sql: &str) -> usize {
    let mut n = 0usize;
    let bytes = sql.as_bytes();
    let mut i = 0usize;
    let mut in_string: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_string {
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => in_string = Some(b),
            b'?' => n += 1,
            b'$' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() => n += 1,
            _ => {}
        }
        i += 1;
    }
    n
}

/// 慢查询记录（docs/04 F-OBS-003）。
#[derive(Debug, Clone, PartialEq)]
pub struct SlowLogEntry {
    pub digest: String,
    pub sql: String,
    pub duration_ms: u64,
    pub rows: usize,
    pub timestamp_ms: u64,
}

/// 慢查询日志（阈值可配，docs/05 §7 环境固化）。
#[derive(Debug)]
pub struct SlowLog {
    threshold_ms: u64,
    entries: Vec<SlowLogEntry>,
}

impl SlowLog {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            threshold_ms,
            entries: Vec::new(),
        }
    }

    pub fn threshold(&self) -> u64 {
        self.threshold_ms
    }

    pub fn record(&mut self, sql: &str, duration_ms: u64, rows: usize) -> bool {
        if duration_ms < self.threshold_ms {
            return false;
        }
        let digest = sql_digest(sql);
        self.entries.push(SlowLogEntry {
            digest,
            sql: sql.to_string(),
            duration_ms,
            rows,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        });
        true
    }

    pub fn entries(&self) -> &[SlowLogEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// SQL 摘要（归一化后的短哈希，供 TopSQL 看板）。
pub fn sql_digest(sql: &str) -> String {
    let norm = normalize_sql(sql);
    // FNV-1a 64 位。
    let mut h: u64 = 0xcbf29ce484222325;
    for b in norm.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// 将内部错误转换为客户端可读 ERR 包（docs/04 F-SQL-016）。
pub fn error_to_packet(seq: u8, err: &lattice_common::Error) -> Packet {
    let code = MySqlErrorCode::from_error_kind(err.kind, &err.msg);
    crate::protocol::err_packet(seq, code, &err.msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_defaults_and_variables() {
        let mut s = Session::new(SessionId(1), "root");
        assert_eq!(s.username, "root");
        assert_eq!(s.get_variable("sql_mode"), Some("STRICT_TRANS_TABLES"));
        s.set_variable("max_execution_time", "500").unwrap();
        assert_eq!(s.get_variable("max_execution_time"), Some("500"));
        assert!(s.plan_sensitive_vars().iter().any(|(k, _)| k == "sql_mode"));
    }

    #[test]
    fn prepared_statement_lifecycle() {
        let mut s = Session::new(SessionId(2), "u");
        let p = s.prepare("SELECT * FROM t WHERE a = ? AND b = ?").unwrap();
        assert_eq!(p.num_params, 2);
        assert_eq!(
            s.get_prepared(p.id).unwrap().sql,
            "SELECT * FROM t WHERE a = ? AND b = ?"
        );
        s.close_prepared(p.id);
        assert!(s.get_prepared(p.id).is_err());
    }

    #[test]
    fn privilege_check() {
        let mut s = Session::new(SessionId(3), "u");
        assert!(s.check_privilege("test", "t", Privilege::Select).is_ok()); // 默认允许
        s.grant(Grant {
            database: "test".into(),
            table: "t".into(),
            privilege: Privilege::Select,
        });
        assert!(s.check_privilege("test", "t", Privilege::Select).is_ok());
        assert_eq!(
            s.check_privilege("test", "t", Privilege::Delete)
                .unwrap_err()
                .kind,
            ErrorKind::Permission
        );
        assert!(s
            .check_privilege("test", "other", Privilege::Select)
            .is_err());
    }

    #[test]
    fn normalize_sql_collapses_literals_and_comments() {
        let a = normalize_sql("SELECT a FROM t WHERE a = 42 AND b = 'xyz'");
        let b = normalize_sql("  select   a  from t -- comment\n where a=99 and b='abc'  ");
        assert_eq!(a, b);
        assert_eq!(a, "SELECT A FROM T WHERE A=? AND B=?");
    }

    #[test]
    fn normalize_preserves_string_escapes() {
        assert_eq!(normalize_sql("SELECT 'it''s'"), "SELECT ?");
    }

    #[test]
    fn count_params_handles_placeholders() {
        assert_eq!(count_params("SELECT ? , $1, '?'"), 2);
    }

    #[test]
    fn sql_digest_is_stable() {
        let d1 = sql_digest("SELECT 1 FROM t WHERE a = 5");
        let d2 = sql_digest("select 1 from t where a = 999");
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 16);
    }

    #[test]
    fn slow_log_respects_threshold() {
        let mut log = SlowLog::new(100);
        assert!(!log.record("SELECT 1", 10, 1));
        assert!(log.record("SELECT slow", 250, 3));
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries()[0].digest.len(), 16);
        assert_eq!(log.entries()[0].rows, 3);
    }

    #[test]
    fn error_packet_maps_kind() {
        let err = lattice_common::Error::new(ErrorKind::Parse, "bad sql");
        let p = error_to_packet(5, &err);
        assert_eq!(p.payload[0], 0xFF);
        assert_eq!(&p.payload[1..3], &1064u16.to_le_bytes());
    }
}
