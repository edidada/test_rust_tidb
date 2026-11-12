//! W19 MySQL wire 最小协议（docs/01 §3.1 TCP 4000、docs/04 F-SQL-001/002/010/016、AC-SQL-001）。
//!
//! 实现 MySQL 客户端/服务器协议子集：包帧、V10 握手、HandshakeResponse41、
//! OK/ERR/EOF 包、COM_QUERY/COM_PING/COM_QUIT/COM_STMT_*、Text Resultset 列定义与行编码。
//! 不依赖 tokio：编解码纯内存，TCP 监听由 `server` 用标准库 `TcpListener` 驱动。

use lattice_common::{Error, ErrorKind, Result};

/// MySQL 协议常量。
pub const PROTOCOL_VERSION: u8 = 10;
pub const SERVER_VERSION: &str = "8.0.36-latticedb-0.1";
pub const AUTH_PLUGIN: &str = "mysql_native_password";
pub const DEFAULT_CHARSET: u8 = 45; // utf8mb4_general_ci

/// 能力标志（docs：CLIENT_* 子集）。
pub mod capability {
    pub const LONG_PASSWORD: u32 = 0x0000_0001;
    pub const FOUND_ROWS: u32 = 0x0000_0002;
    pub const LONG_FLAG: u32 = 0x0000_0004;
    pub const CONNECT_WITH_DB: u32 = 0x0000_0008;
    pub const PROTOCOL_41: u32 = 0x0000_0200;
    pub const TRANSACTIONS: u32 = 0x0000_2000;
    pub const SECURE_CONNECTION: u32 = 0x0000_8000;
    pub const MULTI_STATEMENTS: u32 = 0x0001_0000;
    pub const MULTI_RESULTS: u32 = 0x0002_0000;
    pub const PS_MULTI_RESULTS: u32 = 0x0004_0000;
    pub const PLUGIN_AUTH: u32 = 0x0008_0000;
    pub const CONNECT_ATTRS: u32 = 0x0010_0000;
    pub const DEPRECATE_EOF: u32 = 0x0100_0000;

    pub const SERVER_DEFAULT: u32 = LONG_PASSWORD
        | FOUND_ROWS
        | LONG_FLAG
        | CONNECT_WITH_DB
        | PROTOCOL_41
        | TRANSACTIONS
        | SECURE_CONNECTION
        | MULTI_STATEMENTS
        | MULTI_RESULTS
        | PS_MULTI_RESULTS
        | PLUGIN_AUTH
        | CONNECT_ATTRS;
}

/// 命令码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    Quit = 0x01,
    InitDb = 0x02,
    Query = 0x03,
    Ping = 0x0e,
    StmtPrepare = 0x16,
    StmtExecute = 0x17,
    StmtClose = 0x19,
}

impl Command {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Command::Quit,
            0x02 => Command::InitDb,
            0x03 => Command::Query,
            0x0e => Command::Ping,
            0x16 => Command::StmtPrepare,
            0x17 => Command::StmtExecute,
            0x19 => Command::StmtClose,
            _ => return None,
        })
    }
}

/// MySQL 错误码（docs/04 F-SQL-016：客户端可读）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MySqlErrorCode {
    Unknown = 1105,
    Parse = 1064,
    NoSuchTable = 1146,
    UnknownColumn = 1054,
    DupEntry = 1062,
    AccessDenied = 1045,
    Unsupported = 1235,
}

impl MySqlErrorCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn sql_state(self) -> &'static str {
        match self {
            MySqlErrorCode::Unknown => "HY000",
            MySqlErrorCode::Parse => "42000",
            MySqlErrorCode::NoSuchTable => "42S02",
            MySqlErrorCode::UnknownColumn => "42S22",
            MySqlErrorCode::DupEntry => "23000",
            MySqlErrorCode::AccessDenied => "28000",
            MySqlErrorCode::Unsupported => "42000",
        }
    }

    /// 由内部错误分类映射（docs/02 §13 ErrorKind）。
    pub fn from_error_kind(kind: ErrorKind, msg: &str) -> Self {
        match kind {
            ErrorKind::Parse => MySqlErrorCode::Parse,
            ErrorKind::Bind => {
                if msg.contains("表") || msg.contains("不存在") {
                    MySqlErrorCode::NoSuchTable
                } else {
                    MySqlErrorCode::UnknownColumn
                }
            }
            ErrorKind::TypeMismatch => MySqlErrorCode::UnknownColumn,
            ErrorKind::Permission => MySqlErrorCode::AccessDenied,
            ErrorKind::WriteConflict | ErrorKind::KeyIsLocked => MySqlErrorCode::DupEntry,
            ErrorKind::Unsupported => MySqlErrorCode::Unsupported,
            _ => MySqlErrorCode::Unknown,
        }
    }
}

/// 一个 MySQL 包（3B 长度 + 1B 序号 + payload）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub seq: u8,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(seq: u8, payload: Vec<u8>) -> Self {
        Self { seq, payload }
    }

    pub fn encode(&self) -> Vec<u8> {
        let len = self.payload.len();
        let mut out = Vec::with_capacity(4 + len);
        out.push((len & 0xFF) as u8);
        out.push(((len >> 8) & 0xFF) as u8);
        out.push(((len >> 16) & 0xFF) as u8);
        out.push(self.seq);
        out.extend_from_slice(&self.payload);
        out
    }

    /// 从字节流解析一个包，返回 (包, 消耗字节数)。
    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        if buf.len() < 4 {
            return Err(Error::new(ErrorKind::Parse, "包头部不足 4 字节"));
        }
        let len = buf[0] as usize | ((buf[1] as usize) << 8) | ((buf[2] as usize) << 16);
        let seq = buf[3];
        if buf.len() < 4 + len {
            return Err(Error::new(
                ErrorKind::Parse,
                format!("包体不足: 需要 {len}, 实得 {}", buf.len() - 4),
            ));
        }
        Ok((Self::new(seq, buf[4..4 + len].to_vec()), 4 + len))
    }
}

/// 长度编码整数（length-encoded integer）。
pub fn encode_lenenc_int(v: u64) -> Vec<u8> {
    match v {
        0..=250 => vec![v as u8],
        251..=0xFFFF => {
            let mut o = vec![0xFC];
            o.extend_from_slice(&(v as u16).to_le_bytes());
            o
        }
        0x1_0000..=0xFF_FFFF => {
            let mut o = vec![0xFD];
            o.extend_from_slice(&(v as u32).to_le_bytes()[..3]);
            o
        }
        _ => {
            let mut o = vec![0xFE];
            o.extend_from_slice(&v.to_le_bytes());
            o
        }
    }
}

pub fn decode_lenenc_int(buf: &[u8]) -> Result<(u64, usize)> {
    let Some(&first) = buf.first() else {
        return Err(Error::new(ErrorKind::Parse, "缺少长度编码整数"));
    };
    Ok(match first {
        0..=0xFA => (first as u64, 1),
        0xFC => {
            if buf.len() < 3 {
                return Err(Error::new(ErrorKind::Parse, "lenenc 2B 不足"));
            }
            (u16::from_le_bytes([buf[1], buf[2]]) as u64, 3)
        }
        0xFD => {
            if buf.len() < 4 {
                return Err(Error::new(ErrorKind::Parse, "lenenc 3B 不足"));
            }
            (
                buf[1] as u64 | ((buf[2] as u64) << 8) | ((buf[3] as u64) << 16),
                4,
            )
        }
        0xFE => {
            if buf.len() < 9 {
                return Err(Error::new(ErrorKind::Parse, "lenenc 8B 不足"));
            }
            (u64::from_le_bytes(buf[1..9].try_into().unwrap()), 9)
        }
        0xFB => (0, 1), // NULL
        _ => return Err(Error::new(ErrorKind::Parse, "非法 lenenc 前缀")),
    })
}

/// 长度编码字符串。
pub fn encode_lenenc_str(s: &[u8]) -> Vec<u8> {
    let mut out = encode_lenenc_int(s.len() as u64);
    out.extend_from_slice(s);
    out
}

pub fn decode_lenenc_str(buf: &[u8]) -> Result<(Vec<u8>, usize)> {
    let (len, n) = decode_lenenc_int(buf)?;
    let end = n + len as usize;
    if buf.len() < end {
        return Err(Error::new(ErrorKind::Parse, "lenenc 字符串越界"));
    }
    Ok((buf[n..end].to_vec(), end))
}

/// 以 NUL 结尾字符串。
pub fn encode_nul_str(s: &str) -> Vec<u8> {
    let mut o = s.as_bytes().to_vec();
    o.push(0);
    o
}

/// 构造 Initial Handshake Packet V10。
pub fn handshake_packet(seq: u8, connection_id: u32, salt: &[u8; 20]) -> Packet {
    let mut p = Vec::new();
    p.push(PROTOCOL_VERSION);
    p.extend_from_slice(&encode_nul_str(SERVER_VERSION));
    p.extend_from_slice(&connection_id.to_le_bytes());
    p.extend_from_slice(&salt[..8]); // auth-plugin-data-part-1
    p.push(0); // filler
    p.extend_from_slice(&(capability::SERVER_DEFAULT as u16).to_le_bytes());
    p.push(DEFAULT_CHARSET);
    p.extend_from_slice(&2u16.to_le_bytes()); // status: autocommit
    p.extend_from_slice(&((capability::SERVER_DEFAULT >> 16) as u16).to_le_bytes());
    p.push(21); // auth_plugin_data_len = 8 + 13
    p.extend_from_slice(&[0u8; 10]); // reserved
    p.extend_from_slice(&salt[8..]); // auth-plugin-data-part-2 (12B) + NUL
    p.push(0);
    p.extend_from_slice(&encode_nul_str(AUTH_PLUGIN));
    Packet::new(seq, p)
}

/// 解析 HandshakeResponse41（最小子集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeResponse {
    pub capabilities: u32,
    pub max_packet_size: u32,
    pub charset: u8,
    pub username: String,
    pub auth_response: Vec<u8>,
    pub database: Option<String>,
    pub auth_plugin: Option<String>,
}

pub fn parse_handshake_response(payload: &[u8]) -> Result<HandshakeResponse> {
    let mut r = ByteReader::new(payload);
    let capabilities = r.u32_le()?;
    let max_packet_size = r.u32_le()?;
    let charset = r.u8()?;
    r.skip(23)?; // filler
    let username = String::from_utf8_lossy(&r.nul_str()?).into_owned();
    let auth_response = if capabilities & capability::SECURE_CONNECTION != 0 {
        let len = r.u8()? as usize;
        r.bytes(len)?
    } else {
        r.nul_str()?
    };
    let database = if capabilities & capability::CONNECT_WITH_DB != 0 {
        Some(String::from_utf8_lossy(&r.nul_str()?).into_owned())
    } else {
        None
    };
    let auth_plugin = if capabilities & capability::PLUGIN_AUTH != 0 {
        Some(String::from_utf8_lossy(&r.nul_str()?).into_owned())
    } else {
        None
    };
    Ok(HandshakeResponse {
        capabilities,
        max_packet_size,
        charset,
        username,
        auth_response,
        database,
        auth_plugin,
    })
}

/// OK 包（docs/04 F-SQL-002）。
pub fn ok_packet(seq: u8, affected_rows: u64, last_insert_id: u64) -> Packet {
    let mut p = vec![0x00];
    p.extend_from_slice(&encode_lenenc_int(affected_rows));
    p.extend_from_slice(&encode_lenenc_int(last_insert_id));
    p.extend_from_slice(&2u16.to_le_bytes()); // status autocommit
    p.extend_from_slice(&0u16.to_le_bytes()); // warnings
    Packet::new(seq, p)
}

/// EOF 包（docs/04 F-SQL-002）。
pub fn eof_packet(seq: u8) -> Packet {
    let mut p = vec![0xFE];
    p.extend_from_slice(&0u16.to_le_bytes()); // warnings
    p.extend_from_slice(&2u16.to_le_bytes()); // status
    Packet::new(seq, p)
}

/// ERR 包（docs/04 F-SQL-016 错误码对齐）。
pub fn err_packet(seq: u8, code: MySqlErrorCode, message: &str) -> Packet {
    let mut p = vec![0xFF];
    p.extend_from_slice(&code.code().to_le_bytes());
    p.push(b'#');
    p.extend_from_slice(code.sql_state().as_bytes());
    p.extend_from_slice(message.as_bytes());
    Packet::new(seq, p)
}

/// 列类型（MySQL 协议，docs/04 F-SQL-002 结果集编码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Decimal = 0x00,
    Tiny = 0x01,
    Short = 0x02,
    Long = 0x03,
    Float = 0x04,
    Double = 0x05,
    Null = 0x06,
    Timestamp = 0x07,
    LongLong = 0x08,
    VarString = 0xFD,
}

/// 列定义元数据（避免 `column_definition_packet` 参数过多）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    pub catalog: String,
    pub schema: String,
    pub table: String,
    pub org_table: String,
    pub name: String,
    pub org_name: String,
}

impl ColumnMeta {
    pub fn simple(name: &str) -> Self {
        Self {
            catalog: "def".into(),
            schema: "test".into(),
            table: String::new(),
            org_table: String::new(),
            name: name.into(),
            org_name: name.into(),
        }
    }
}

/// ColumnDefinition41 包。
pub fn column_definition_packet(seq: u8, meta: &ColumnMeta, col_type: ColumnType) -> Packet {
    let mut p = Vec::new();
    for s in [
        &meta.catalog,
        &meta.schema,
        &meta.table,
        &meta.org_table,
        &meta.name,
        &meta.org_name,
    ] {
        p.extend_from_slice(&encode_lenenc_str(s.as_bytes()));
    }
    p.push(0x0C); // length of fixed fields
    p.extend_from_slice(&DEFAULT_CHARSET.to_le_bytes()); // charset (2B)
    p.extend_from_slice(&1024u32.to_le_bytes()); // column_length (4B)
    p.push(col_type as u8);
    p.extend_from_slice(&0u16.to_le_bytes()); // flags
    p.push(0); // decimals
    p.extend_from_slice(&[0u8, 0u8]); // filler
    Packet::new(seq, p)
}

/// 文本结果集行（NULL 用 0xFB，其余用 lenenc string）。
pub fn text_row_packet(seq: u8, values: &[Option<Vec<u8>>]) -> Packet {
    let mut p = Vec::new();
    for v in values {
        match v {
            Some(bytes) => p.extend_from_slice(&encode_lenenc_str(bytes)),
            None => p.push(0xFB),
        }
    }
    Packet::new(seq, p)
}

/// 列数包。
pub fn column_count_packet(seq: u8, n: usize) -> Packet {
    Packet::new(seq, encode_lenenc_int(n as u64))
}

/// Prepared Statement OK（COM_STMT_PREPARE 响应）。
pub fn stmt_prepare_ok_packet(seq: u8, stmt_id: u32, num_columns: u16, num_params: u16) -> Packet {
    let mut p = vec![0x00];
    p.extend_from_slice(&stmt_id.to_le_bytes());
    p.extend_from_slice(&num_columns.to_le_bytes());
    p.extend_from_slice(&num_params.to_le_bytes());
    p.push(0); // filler
    p.extend_from_slice(&0u16.to_le_bytes()); // warnings
    Packet::new(seq, p)
}

/// 极简字节读取器。
pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn u8(&mut self) -> Result<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| Error::new(ErrorKind::Parse, "字节读取越界"))?;
        self.pos += 1;
        Ok(b)
    }

    pub fn u32_le(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        let end = self.pos + n;
        if end > self.buf.len() {
            return Err(Error::new(ErrorKind::Parse, "字节读取越界"));
        }
        let out = self.buf[self.pos..end].to_vec();
        self.pos = end;
        Ok(out)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    pub fn nul_str(&mut self) -> Result<Vec<u8>> {
        let rel = self.buf[self.pos..]
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| Error::new(ErrorKind::Parse, "缺少 NUL 结尾"))?;
        let out = self.buf[self.pos..self.pos + rel].to_vec();
        self.pos += rel + 1;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_roundtrip() {
        let p = Packet::new(7, b"hello".to_vec());
        let bytes = p.encode();
        assert_eq!(bytes[..4], [5, 0, 0, 7]);
        let (back, consumed) = Packet::decode(&bytes).unwrap();
        assert_eq!(back, p);
        assert_eq!(consumed, 9);
    }

    #[test]
    fn truncated_packet_is_parse_error() {
        assert_eq!(
            Packet::decode(&[5, 0, 0, 0, b'a']).unwrap_err().kind,
            ErrorKind::Parse
        );
    }

    #[test]
    fn lenenc_int_boundaries() {
        for v in [
            0u64,
            1,
            250,
            251,
            65535,
            65536,
            0xFF_FFFF,
            0x100_0000,
            u64::MAX,
        ] {
            let enc = encode_lenenc_int(v);
            let (back, n) = decode_lenenc_int(&enc).unwrap();
            assert_eq!(back, v, "v={v}");
            assert_eq!(n, enc.len());
        }
    }

    #[test]
    fn lenenc_str_roundtrip() {
        let enc = encode_lenenc_str(b"lattice");
        let (back, n) = decode_lenenc_str(&enc).unwrap();
        assert_eq!(back, b"lattice".to_vec());
        assert_eq!(n, enc.len());
    }

    #[test]
    fn handshake_packet_has_v10_header() {
        let salt = [0x42u8; 20];
        let p = handshake_packet(0, 123, &salt);
        assert_eq!(p.payload[0], PROTOCOL_VERSION);
        assert!(p.payload.windows(1).any(|w| w == b"8"));
        let text = String::from_utf8_lossy(&p.payload);
        assert!(text.contains("8.0.36-latticedb-0.1"));
        assert!(text.contains(AUTH_PLUGIN));
    }

    #[test]
    fn handshake_response_parsing() {
        let mut payload = Vec::new();
        let caps =
            capability::PROTOCOL_41 | capability::SECURE_CONNECTION | capability::PLUGIN_AUTH;
        payload.extend_from_slice(&caps.to_le_bytes());
        payload.extend_from_slice(&(16u32 * 1024 * 1024).to_le_bytes());
        payload.push(DEFAULT_CHARSET);
        payload.extend_from_slice(&[0u8; 23]);
        payload.extend_from_slice(b"root\0");
        payload.push(20);
        payload.extend_from_slice(&[0xAAu8; 20]);
        payload.extend_from_slice(&encode_nul_str(AUTH_PLUGIN));
        let r = parse_handshake_response(&payload).unwrap();
        assert_eq!(r.username, "root");
        assert_eq!(r.auth_response, vec![0xAAu8; 20]);
        assert_eq!(r.auth_plugin.as_deref(), Some(AUTH_PLUGIN));
    }

    #[test]
    fn ok_eof_err_packets() {
        let ok = ok_packet(1, 3, 7);
        assert_eq!(ok.payload[0], 0x00);
        assert_eq!(ok.seq, 1);
        let eof = eof_packet(2);
        assert_eq!(eof.payload[0], 0xFE);
        let err = err_packet(3, MySqlErrorCode::Parse, "syntax error");
        assert_eq!(err.payload[0], 0xFF);
        assert_eq!(&err.payload[1..3], &1064u16.to_le_bytes());
        assert!(String::from_utf8_lossy(&err.payload).contains("42000"));
    }

    #[test]
    fn error_code_mapping() {
        assert_eq!(
            MySqlErrorCode::from_error_kind(ErrorKind::Parse, ""),
            MySqlErrorCode::Parse
        );
        assert_eq!(
            MySqlErrorCode::from_error_kind(ErrorKind::Bind, "表 test.x 不存在"),
            MySqlErrorCode::NoSuchTable
        );
        assert_eq!(
            MySqlErrorCode::from_error_kind(ErrorKind::WriteConflict, ""),
            MySqlErrorCode::DupEntry
        );
    }

    #[test]
    fn resultset_encoding() {
        let col = column_definition_packet(2, &ColumnMeta::simple("a"), ColumnType::LongLong);
        assert!(col.payload.len() > 12);
        let row = text_row_packet(4, &[Some(b"42".to_vec()), None]);
        // 42 -> len 2 + "42"; NULL -> 0xFB
        assert_eq!(row.payload[0], 2);
        assert_eq!(&row.payload[1..3], b"42");
        assert_eq!(row.payload[3], 0xFB);
        let cc = column_count_packet(1, 2);
        assert_eq!(cc.payload, vec![2]);
    }

    #[test]
    fn stmt_prepare_ok_and_command_codes() {
        let p = stmt_prepare_ok_packet(1, 7, 2, 1);
        assert_eq!(p.payload[0], 0x00);
        assert_eq!(&p.payload[1..5], &7u32.to_le_bytes());
        assert_eq!(Command::from_byte(0x03), Some(Command::Query));
        assert_eq!(Command::from_byte(0x16), Some(Command::StmtPrepare));
        assert_eq!(Command::from_byte(0xFF), None);
    }
}
