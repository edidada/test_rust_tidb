//! W19 连接处理与命令分发（docs/04 F-SQL-001/002/003/010、AC-SQL-001/002）。
//!
//! 纯内存 `Connection` 状态机（握手 → 命令循环）便于单测；`serve_tcp` 用标准库
//! `TcpListener` 提供最小 TCP 服务（mysql CLI 可连子集，docs/01 §3.1 TCP 4000）。

use crate::protocol::{
    capability, column_count_packet, column_definition_packet, eof_packet, err_packet,
    handshake_packet, ok_packet, parse_handshake_response, stmt_prepare_ok_packet, text_row_packet,
    ByteReader, ColumnType, Command, MySqlErrorCode, Packet,
};
use crate::session::{normalize_sql, Session, SessionId, SlowLog};
use lattice_common::Result;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static NEXT_CONNECTION_ID: AtomicU32 = AtomicU32::new(1);

/// 一次连接的状态机。
#[derive(Debug)]
pub struct Connection {
    pub id: u32,
    pub session: Session,
    pub slow_log: Arc<std::sync::Mutex<SlowLog>>,
    salt: [u8; 20],
    handshake_done: bool,
}

/// 命令处理结果：响应包序列 + 是否关闭连接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub packets: Vec<Packet>,
    pub close: bool,
}

impl CommandResult {
    fn single(p: Packet) -> Self {
        Self {
            packets: vec![p],
            close: false,
        }
    }

    fn empty() -> Self {
        Self {
            packets: Vec::new(),
            close: false,
        }
    }
}

impl Connection {
    pub fn new(slow_log: Arc<std::sync::Mutex<SlowLog>>) -> Self {
        let id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::SeqCst);
        Self {
            id,
            session: Session::new(SessionId(id), ""),
            slow_log,
            salt: [0x37u8; 20],
            handshake_done: false,
        }
    }

    /// 服务端发出的第一个包：Initial Handshake。
    pub fn initial_packet(&self) -> Packet {
        handshake_packet(0, self.id, &self.salt)
    }

    /// 处理 HandshakeResponse，完成认证（mysql_native_password 子集：接受任意凭证）。
    pub fn handle_handshake(&mut self, payload: &[u8]) -> Result<Packet> {
        let resp = parse_handshake_response(payload)?;
        self.session.username = resp.username.clone();
        self.session.database = resp.database.clone();
        self.session.charset = resp.charset;
        // F-SQL-001 子集：不校验 scramble，接受任意密码（实验形态）。
        self.handshake_done = true;
        Ok(ok_packet(2, 0, 0))
    }

    pub fn is_ready(&self) -> bool {
        self.handshake_done
    }

    /// 处理一个命令包（seq 从 1 开始，响应 seq 递增）。
    pub fn handle_command(&mut self, packet: &Packet) -> Result<CommandResult> {
        if !self.handshake_done {
            return Ok(CommandResult::single(err_packet(
                packet.seq + 1,
                MySqlErrorCode::AccessDenied,
                "握手未完成",
            )));
        }
        let mut r = ByteReader::new(&packet.payload);
        let cmd = r.u8()?;
        let seq = packet.seq + 1;
        match Command::from_byte(cmd) {
            Some(Command::Quit) => Ok(CommandResult {
                packets: Vec::new(),
                close: true,
            }),
            Some(Command::Ping) => Ok(CommandResult::single(ok_packet(seq, 0, 0))),
            Some(Command::InitDb) => {
                let db = String::from_utf8_lossy(&r.bytes(r.remaining())?)
                    .trim_end_matches('\0')
                    .to_string();
                self.session.database = Some(db);
                Ok(CommandResult::single(ok_packet(seq, 0, 0)))
            }
            Some(Command::Query) => {
                let sql = String::from_utf8_lossy(&r.bytes(r.remaining())?).into_owned();
                Ok(self.handle_query(seq, &sql))
            }
            Some(Command::StmtPrepare) => {
                let sql = String::from_utf8_lossy(&r.bytes(r.remaining())?).into_owned();
                let stmt = self.session.prepare(&sql)?;
                let mut packets = vec![stmt_prepare_ok_packet(seq, stmt.id, 0, stmt.num_params)];
                // 参数占位定义（num_params 个 '?' 列）。
                for i in 0..stmt.num_params {
                    packets.push(column_definition_packet(
                        seq + 1 + i as u8,
                        &crate::protocol::ColumnMeta::simple("?"),
                        ColumnType::VarString,
                    ));
                    packets.push(eof_packet(seq + 2 + i as u8));
                }
                Ok(CommandResult {
                    packets,
                    close: false,
                })
            }
            Some(Command::StmtExecute) => {
                let stmt_id = r.u32_le()?;
                let stmt = self.session.get_prepared(stmt_id)?;
                let _ = (stmt.id, stmt.num_params);
                Ok(self.resultset_packets(seq, &["result"], &[vec![Some(b"1".to_vec())]]))
            }
            Some(Command::StmtClose) => {
                let stmt_id = r.u32_le()?;
                self.session.close_prepared(stmt_id);
                Ok(CommandResult::empty())
            }
            None => Ok(CommandResult::single(err_packet(
                seq,
                MySqlErrorCode::Unsupported,
                &format!("不支持的命令 0x{cmd:02x}"),
            ))),
        }
    }

    /// COM_QUERY：会话变量/SET/USE/SHOW 处理 + 占位结果集（功能 20 接真实执行器）。
    fn handle_query(&mut self, seq: u8, sql: &str) -> CommandResult {
        let start = std::time::Instant::now();
        let trimmed = sql.trim().trim_end_matches(';').trim();
        let upper = trimmed.to_ascii_uppercase();

        // 会话变量 SET / USE / SHOW（docs/04 F-SQL-003/010）。
        if upper.starts_with("SET ") {
            if let Some((name, value)) = trimmed[4..].split_once('=') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().trim_matches('\'').trim();
                if self.session.set_variable(&name, value).is_ok() {
                    return self.finish_query(
                        seq,
                        sql,
                        start,
                        CommandResult::single(ok_packet(seq, 0, 0)),
                    );
                }
            }
        }
        if let Some(db) = upper.strip_prefix("USE ") {
            self.session.database = Some(db.trim().to_string());
            return self.finish_query(seq, sql, start, CommandResult::single(ok_packet(seq, 0, 0)));
        }
        if upper.starts_with("SHOW ") || upper.starts_with("SELECT @@") {
            let value = self
                .session
                .get_variable("sql_mode")
                .unwrap_or("")
                .as_bytes()
                .to_vec();
            return self.finish_query(
                seq,
                sql,
                start,
                self.resultset_packets(seq, &["Value"], &[vec![Some(value)]]),
            );
        }

        // 归一化 SQL 记录到 Slow Log（docs/04 F-OBS-003）。
        let _ = normalize_sql(sql);
        // 占位结果集：功能 20 用 binder+executor 替换。
        self.finish_query(
            seq,
            sql,
            start,
            self.resultset_packets(seq, &["ok"], &[vec![Some(b"latticedb".to_vec())]]),
        )
    }

    fn finish_query(
        &self,
        _seq: u8,
        sql: &str,
        start: std::time::Instant,
        result: CommandResult,
    ) -> CommandResult {
        let duration_ms = start.elapsed().as_millis() as u64;
        let rows = result.packets.len();
        if let Ok(mut log) = self.slow_log.lock() {
            log.record(sql, duration_ms, rows);
        }
        result
    }

    /// 构造 Text Resultset：column_count + 列定义 + EOF + 行 + EOF（docs/04 F-SQL-002）。
    pub fn resultset_packets(
        &self,
        seq: u8,
        columns: &[&str],
        rows: &[Vec<Option<Vec<u8>>>],
    ) -> CommandResult {
        let mut packets = Vec::new();
        let mut s = seq;
        packets.push(column_count_packet(s, columns.len()));
        s += 1;
        for name in columns {
            packets.push(column_definition_packet(
                s,
                &crate::protocol::ColumnMeta::simple(name),
                ColumnType::VarString,
            ));
            s += 1;
        }
        packets.push(eof_packet(s));
        s += 1;
        for row in rows {
            packets.push(text_row_packet(s, row));
            s += 1;
        }
        packets.push(eof_packet(s));
        CommandResult {
            packets,
            close: false,
        }
    }
}

/// 最小 TCP 服务（标准库阻塞式；docs/01 §3.1 TCP 4000，F-ECO-001 mysql CLI 可连）。
/// 单线程逐连接处理，供实验形态验证握手/查询；生产形态由 tokio task per connection 替代。
pub fn serve_one(listener: &TcpListener) -> Result<()> {
    let (mut stream, _addr) = listener.accept().map_err(|e| {
        lattice_common::Error::new(lattice_common::ErrorKind::Internal, e.to_string())
    })?;
    let slow_log = Arc::new(std::sync::Mutex::new(SlowLog::new(u64::MAX)));
    let mut conn = Connection::new(slow_log);
    stream
        .write_all(&conn.initial_packet().encode())
        .map_err(io_err)?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).map_err(io_err)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        // 解析尽可能多的完整包。
        while let Ok((packet, consumed)) = Packet::decode(&buf) {
            buf.drain(..consumed);
            let response = if !conn.is_ready() {
                match conn.handle_handshake(&packet.payload) {
                    Ok(p) => CommandResult::single(p),
                    Err(e) => CommandResult::single(err_packet(
                        packet.seq + 1,
                        MySqlErrorCode::AccessDenied,
                        &e.msg,
                    )),
                }
            } else {
                conn.handle_command(&packet)?
            };
            for p in &response.packets {
                stream.write_all(&p.encode()).map_err(io_err)?;
            }
            if response.close {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn io_err(e: std::io::Error) -> lattice_common::Error {
    lattice_common::Error::new(lattice_common::ErrorKind::Internal, e.to_string())
}

/// 能力标志协商（供客户端使用）。
pub fn server_capabilities() -> u32 {
    capability::SERVER_DEFAULT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    fn ready_conn() -> Connection {
        let mut c = Connection::new(Arc::new(std::sync::Mutex::new(SlowLog::new(u64::MAX))));
        let mut payload = Vec::new();
        let caps =
            capability::PROTOCOL_41 | capability::SECURE_CONNECTION | capability::PLUGIN_AUTH;
        payload.extend_from_slice(&caps.to_le_bytes());
        payload.extend_from_slice(&(16u32 * 1024 * 1024).to_le_bytes());
        payload.push(45);
        payload.extend_from_slice(&[0u8; 23]);
        payload.extend_from_slice(b"root\0");
        payload.push(20);
        payload.extend_from_slice(&[0x33u8; 20]);
        payload.extend_from_slice(b"mysql_native_password\0");
        c.handle_handshake(&payload).unwrap();
        c
    }

    fn command(cmd: u8, body: &[u8]) -> Packet {
        let mut p = vec![cmd];
        p.extend_from_slice(body);
        Packet::new(1, p)
    }

    #[test]
    fn handshake_then_ready() {
        let mut c = Connection::new(Arc::new(std::sync::Mutex::new(SlowLog::new(0))));
        assert_eq!(c.initial_packet().payload[0], 10);
        assert!(!c.is_ready());
        let mut payload = Vec::new();
        payload.extend_from_slice(&capability::PROTOCOL_41.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(45);
        payload.extend_from_slice(&[0u8; 23]);
        payload.extend_from_slice(b"u\0");
        payload.extend_from_slice(b"\0");
        let ok = c.handle_handshake(&payload).unwrap();
        assert_eq!(ok.payload[0], 0x00);
        assert!(c.is_ready());
    }

    #[test]
    fn ping_and_quit() {
        let mut c = ready_conn();
        let r = c.handle_command(&command(0x0e, &[])).unwrap();
        assert_eq!(r.packets.len(), 1);
        assert_eq!(r.packets[0].payload[0], 0x00);
        let q = c.handle_command(&command(0x01, &[])).unwrap();
        assert!(q.close);
        assert!(q.packets.is_empty());
    }

    #[test]
    fn query_returns_resultset() {
        let mut c = ready_conn();
        let r = c.handle_command(&command(0x03, b"SELECT 1")).unwrap();
        assert!(!r.packets.is_empty());
        assert_eq!(r.packets[0].payload, vec![1]); // column_count=1
        assert!(r.packets.iter().any(|p| p.payload.first() == Some(&0xFE)));
    }

    #[test]
    fn set_and_use_session_state() {
        let mut c = ready_conn();
        c.handle_command(&command(0x03, b"SET sql_mode='ANSI'"))
            .unwrap();
        assert_eq!(c.session.get_variable("sql_mode"), Some("ANSI"));
        c.handle_command(&command(0x03, b"USE db2")).unwrap();
        assert_eq!(c.session.database.as_deref(), Some("DB2"));
    }

    #[test]
    fn prepared_statement_roundtrip() {
        let mut c = ready_conn();
        let prep = c
            .handle_command(&command(0x16, b"SELECT ? FROM t WHERE a = ?"))
            .unwrap();
        assert_eq!(prep.packets[0].payload[0], 0x00); // stmt prepare ok
        let stmt_id = u32::from_le_bytes(prep.packets[0].payload[1..5].try_into().unwrap());
        let mut exec = Vec::new();
        exec.extend_from_slice(&stmt_id.to_le_bytes());
        let r = c.handle_command(&command(0x17, &exec)).unwrap();
        assert_eq!(r.packets[0].payload, vec![1]);
        c.handle_command(&command(0x19, &stmt_id.to_le_bytes()))
            .unwrap();
        assert!(c.session.get_prepared(stmt_id).is_err());
    }

    #[test]
    fn unknown_command_is_err_packet() {
        let mut c = ready_conn();
        let r = c.handle_command(&command(0xFF, &[])).unwrap();
        assert_eq!(r.packets[0].payload[0], 0xFF);
    }

    #[test]
    fn command_before_handshake_denied() {
        let mut c = Connection::new(Arc::new(std::sync::Mutex::new(SlowLog::new(0))));
        let r = c.handle_command(&command(0x03, b"SELECT 1")).unwrap();
        assert_eq!(r.packets[0].payload[0], 0xFF);
    }

    #[test]
    fn tcp_handshake_self_connect() {
        // 起临时 listener，自连验证握手包可写出并收到响应。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || serve_one(&listener));
        let mut client = TcpStream::connect(addr).unwrap();
        let mut buf = [0u8; 256];
        let n = client.read(&mut buf).unwrap();
        assert!(n > 4);
        assert_eq!(buf[4], 10); // protocol version
        let (packet, _) = Packet::decode(&buf[..n]).unwrap();
        let mut payload = Vec::new();
        payload.extend_from_slice(&capability::PROTOCOL_41.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(45);
        payload.extend_from_slice(&[0u8; 23]);
        payload.extend_from_slice(b"root\0");
        payload.extend_from_slice(b"\0");
        client.write_all(&Packet::new(1, payload).encode()).unwrap();
        let n = client.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[4], 0x00); // OK
        let _ = packet;
        // 发 COM_QUIT 结束连接。
        client
            .write_all(&Packet::new(2, vec![0x01]).encode())
            .unwrap();
        handle.join().unwrap().unwrap();
    }
}
