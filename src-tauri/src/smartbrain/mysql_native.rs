//! Minimal MySQL wire-protocol client for SmartBrain SQL queries.
//!
//! Avoids depending on a local `mysql` CLI binary. Supports:
//! - `mysql_native_password`
//! - `caching_sha2_password` (including full-auth cleartext over non-SSL when server allows)

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

use sha1::{Digest, Sha1};
use sha2::Sha256;

const CLIENT_PROTOCOL_41: u32 = 0x0000_0200;
const CLIENT_SECURE_CONNECTION: u32 = 0x0000_8000;
const CLIENT_PLUGIN_AUTH: u32 = 0x0008_0000;
const CLIENT_CONNECT_WITH_DB: u32 = 0x0000_0008;
const CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA: u32 = 0x0020_0000;

const COM_QUERY: u8 = 0x03;

#[derive(Debug, Clone)]
pub struct MysqlQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

pub fn execute_mysql_query(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    sql: &str,
    timeout_sec: u64,
    row_limit: usize,
) -> Result<MysqlQueryResult, String> {
    let timeout = Duration::from_secs(timeout_sec.max(1));
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|error| format!("无效的 MySQL 地址 `{addr}`: {error}"))?,
        timeout,
    )
    .map_err(|error| format!("连接 MySQL `{addr}` 失败: {error}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("设置读超时失败: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("设置写超时失败: {error}"))?;

    let mut seq: u8 = 0;
    let greeting = read_packet(&mut stream, &mut seq)?;
    let handshake = parse_handshake(&greeting)?;

    // 使用经典结果集协议（不启用 DEPRECATE_EOF），便于稳定识别行结束 EOF。
    let mut client_caps = CLIENT_PROTOCOL_41
        | CLIENT_SECURE_CONNECTION
        | CLIENT_PLUGIN_AUTH
        | CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA;
    if !database.trim().is_empty() {
        client_caps |= CLIENT_CONNECT_WITH_DB;
    }
    client_caps &= handshake.capability_flags;

    let plugin = if handshake.auth_plugin_name.is_empty() {
        "mysql_native_password".to_string()
    } else {
        handshake.auth_plugin_name.clone()
    };
    let auth_response = build_auth_response(password, &handshake.auth_plugin_data, &plugin)?;
    let auth_packet = build_handshake_response(
        client_caps,
        username,
        &auth_response,
        database,
        &plugin,
        handshake.character_set,
    );
    write_packet(&mut stream, &mut seq, &auth_packet)?;

    let mut response = read_packet(&mut stream, &mut seq)?;
    response = complete_authentication(
        &mut stream,
        &mut seq,
        response,
        password,
        &handshake.auth_plugin_data,
    )?;
    ensure_ok_packet(&response)?;

    // Reset sequence for command phase.
    seq = 0;
    let mut query_payload = Vec::with_capacity(sql.len() + 1);
    query_payload.push(COM_QUERY);
    query_payload.extend_from_slice(sql.as_bytes());
    write_packet(&mut stream, &mut seq, &query_payload)?;

    let result = read_query_result(&mut stream, &mut seq, row_limit)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(result)
}

#[derive(Debug)]
struct Handshake {
    capability_flags: u32,
    character_set: u8,
    auth_plugin_data: Vec<u8>,
    auth_plugin_name: String,
}

fn parse_handshake(packet: &[u8]) -> Result<Handshake, String> {
    if packet.is_empty() {
        return Err("空的 MySQL handshake 包".to_string());
    }
    if packet[0] == 0xff {
        return Err(format_err_packet(packet));
    }
    let mut offset = 0usize;
    let protocol = read_u8(packet, &mut offset)?;
    if protocol != 10 {
        return Err(format!("不支持的 MySQL protocol version: {protocol}"));
    }
    let _version = read_null_terminated_string(packet, &mut offset)?;
    let _connection_id = read_u32_le(packet, &mut offset)?;
    let mut auth_plugin_data = read_fixed(packet, &mut offset, 8)?.to_vec();
    let _filler = read_u8(packet, &mut offset)?;
    let capability_lower = read_u16_le(packet, &mut offset)? as u32;
    let character_set = if offset < packet.len() {
        read_u8(packet, &mut offset)?
    } else {
        33
    };
    let _status = if offset + 2 <= packet.len() {
        read_u16_le(packet, &mut offset)?
    } else {
        0
    };
    let capability_upper = if offset + 2 <= packet.len() {
        read_u16_le(packet, &mut offset)? as u32
    } else {
        0
    };
    let capability_flags = capability_lower | (capability_upper << 16);
    let auth_data_len = if offset < packet.len() {
        read_u8(packet, &mut offset)? as usize
    } else {
        0
    };
    if offset + 10 <= packet.len() {
        offset += 10; // reserved
    }
    let plugin_data_part2_len = if auth_data_len > 8 {
        auth_data_len.saturating_sub(8)
    } else {
        13
    };
    if offset < packet.len() {
        let remaining = packet.len() - offset;
        let take = plugin_data_part2_len.min(remaining);
        let part2 = read_fixed(packet, &mut offset, take)?;
        // Trim trailing NUL if present.
        let trimmed = part2
            .iter()
            .copied()
            .take_while(|b| *b != 0)
            .collect::<Vec<_>>();
        auth_plugin_data.extend_from_slice(&trimmed);
    }
    let auth_plugin_name = if (capability_flags & CLIENT_PLUGIN_AUTH) != 0 && offset < packet.len() {
        read_null_terminated_string(packet, &mut offset).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(Handshake {
        capability_flags,
        character_set,
        auth_plugin_data,
        auth_plugin_name,
    })
}

fn build_handshake_response(
    capability_flags: u32,
    username: &str,
    auth_response: &[u8],
    database: &str,
    auth_plugin: &str,
    character_set: u8,
) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.extend_from_slice(&capability_flags.to_le_bytes());
    packet.extend_from_slice(&0x0100_0000u32.to_le_bytes()); // max packet size
    packet.push(character_set);
    packet.extend_from_slice(&[0u8; 23]);
    packet.extend_from_slice(username.as_bytes());
    packet.push(0);
    // length-encoded auth response
    write_lenenc_int(&mut packet, auth_response.len() as u64);
    packet.extend_from_slice(auth_response);
    if (capability_flags & CLIENT_CONNECT_WITH_DB) != 0 {
        packet.extend_from_slice(database.as_bytes());
        packet.push(0);
    }
    if (capability_flags & CLIENT_PLUGIN_AUTH) != 0 {
        packet.extend_from_slice(auth_plugin.as_bytes());
        packet.push(0);
    }
    packet
}

fn complete_authentication(
    stream: &mut TcpStream,
    seq: &mut u8,
    mut response: Vec<u8>,
    password: &str,
    scramble: &[u8],
) -> Result<Vec<u8>, String> {
    // Auth switch request: 0xfe
    if !response.is_empty() && response[0] == 0xfe {
        let mut offset = 1usize;
        let plugin = read_null_terminated_string(&response, &mut offset).unwrap_or_default();
        let plugin_data = if offset < response.len() {
            let data = &response[offset..];
            data.iter()
                .copied()
                .take_while(|b| *b != 0)
                .collect::<Vec<_>>()
        } else {
            scramble.to_vec()
        };
        let auth_response = build_auth_response(password, &plugin_data, &plugin)?;
        write_packet(stream, seq, &auth_response)?;
        response = read_packet(stream, seq)?;
    }

    // caching_sha2_password fast auth / full auth request: 0x01 <status>
    if response.len() >= 2 && response[0] == 0x01 {
        match response[1] {
            0x03 => {
                // fast auth success, next packet should be OK
                response = read_packet(stream, seq)?;
            }
            0x04 => {
                // full authentication: send cleartext password (NUL terminated)
                let mut clear = password.as_bytes().to_vec();
                clear.push(0);
                write_packet(stream, seq, &clear)?;
                response = read_packet(stream, seq)?;
            }
            other => {
                return Err(format!("不支持的 caching_sha2_password 状态: 0x{other:02x}"));
            }
        }
    }

    Ok(response)
}

fn ensure_ok_packet(packet: &[u8]) -> Result<(), String> {
    if packet.is_empty() {
        return Err("MySQL 返回空认证响应".to_string());
    }
    match packet[0] {
        0x00 | 0xfe => Ok(()),
        0xff => Err(format_err_packet(packet)),
        other => Err(format!("MySQL 认证失败，未知响应头: 0x{other:02x}")),
    }
}

fn read_query_result(
    stream: &mut TcpStream,
    seq: &mut u8,
    row_limit: usize,
) -> Result<MysqlQueryResult, String> {
    let first = read_packet(stream, seq)?;
    if first.is_empty() {
        return Err("MySQL 查询返回空响应".to_string());
    }
    if first[0] == 0xff {
        return Err(format_err_packet(&first));
    }
    if first[0] == 0x00 {
        // OK packet (non-resultset statement)
        return Ok(MysqlQueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            truncated: false,
        });
    }

    let mut offset = 0usize;
    let column_count = read_lenenc_int(&first, &mut offset)? as usize;
    let mut columns = Vec::with_capacity(column_count);
    for _ in 0..column_count {
        let packet = read_packet(stream, seq)?;
        columns.push(parse_column_name(&packet)?);
    }

    // Classic protocol: EOF after column definitions.
    let after_columns = read_packet(stream, seq)?;
    if after_columns.is_empty() {
        return Err("MySQL 列定义后响应为空".to_string());
    }
    if after_columns[0] == 0xff {
        return Err(format_err_packet(&after_columns));
    }
    if !is_eof_packet(&after_columns) {
        return Err("MySQL 协议异常：列定义后未收到 EOF".to_string());
    }

    let mut rows = Vec::new();
    let mut truncated = false;
    loop {
        let packet = read_packet(stream, seq)?;
        if packet.is_empty() {
            break;
        }
        if packet[0] == 0xff {
            return Err(format_err_packet(&packet));
        }
        if is_eof_packet(&packet) {
            break;
        }
        if rows.len() < row_limit {
            rows.push(parse_text_row(&packet, column_count)?);
        } else {
            truncated = true;
        }
        // 超出 row_limit 时继续排空结果集，避免污染连接状态。
    }
    Ok(MysqlQueryResult {
        columns,
        rows,
        truncated,
    })
}

fn is_eof_packet(packet: &[u8]) -> bool {
    // Classic EOF: 0xfe + 警告数/状态（总长 < 9）
    !packet.is_empty() && packet[0] == 0xfe && packet.len() < 9
}

fn parse_column_name(packet: &[u8]) -> Result<String, String> {
    // catalog, schema, table, org_table, name, org_name...
    let mut offset = 0usize;
    let _catalog = read_lenenc_string(packet, &mut offset)?;
    let _schema = read_lenenc_string(packet, &mut offset)?;
    let _table = read_lenenc_string(packet, &mut offset)?;
    let _org_table = read_lenenc_string(packet, &mut offset)?;
    let name = read_lenenc_string(packet, &mut offset)?;
    Ok(name)
}

fn parse_text_row(packet: &[u8], column_count: usize) -> Result<Vec<String>, String> {
    let mut offset = 0usize;
    let mut values = Vec::with_capacity(column_count);
    for _ in 0..column_count {
        if offset >= packet.len() {
            values.push(String::new());
            continue;
        }
        if packet[offset] == 0xfb {
            offset += 1;
            values.push("NULL".to_string());
            continue;
        }
        values.push(read_lenenc_string(packet, &mut offset)?);
    }
    Ok(values)
}

fn build_auth_response(password: &str, scramble: &[u8], plugin: &str) -> Result<Vec<u8>, String> {
    if password.is_empty() {
        return Ok(Vec::new());
    }
    match plugin {
        "mysql_native_password" => Ok(mysql_native_password(password, scramble)),
        "caching_sha2_password" => Ok(caching_sha2_password(password, scramble)),
        "mysql_clear_password" => {
            let mut data = password.as_bytes().to_vec();
            data.push(0);
            Ok(data)
        }
        other => Err(format!(
            "暂不支持的 MySQL 认证插件 `{other}`。请改用 mysql_native_password / caching_sha2_password，或在服务器侧调整账号认证插件。"
        )),
    }
}

fn mysql_native_password(password: &str, scramble: &[u8]) -> Vec<u8> {
    let mut sha1 = Sha1::new();
    sha1.update(password.as_bytes());
    let stage1 = sha1.finalize();

    let mut sha1 = Sha1::new();
    sha1.update(stage1);
    let stage2 = sha1.finalize();

    let mut sha1 = Sha1::new();
    sha1.update(scramble);
    sha1.update(stage2);
    let stage3 = sha1.finalize();

    stage1
        .iter()
        .zip(stage3.iter())
        .map(|(a, b)| a ^ b)
        .collect()
}

fn caching_sha2_password(password: &str, scramble: &[u8]) -> Vec<u8> {
    let mut sha = Sha256::new();
    sha.update(password.as_bytes());
    let stage1 = sha.finalize();

    let mut sha = Sha256::new();
    sha.update(stage1);
    let stage2 = sha.finalize();

    let mut sha = Sha256::new();
    sha.update(stage2);
    sha.update(scramble);
    let stage3 = sha.finalize();

    stage1
        .iter()
        .zip(stage3.iter())
        .map(|(a, b)| a ^ b)
        .collect()
}

fn format_err_packet(packet: &[u8]) -> String {
    if packet.len() < 3 || packet[0] != 0xff {
        return "MySQL 返回未知错误包".to_string();
    }
    let code = u16::from_le_bytes([packet[1], packet[2]]);
    let message = if packet.len() > 9 && packet[3] == b'#' {
        String::from_utf8_lossy(&packet[9..]).trim().to_string()
    } else if packet.len() > 3 {
        String::from_utf8_lossy(&packet[3..]).trim().to_string()
    } else {
        String::new()
    };
    if message.is_empty() {
        format!("MySQL 错误 {code}")
    } else {
        format!("MySQL 错误 {code}: {message}")
    }
}

fn write_packet(stream: &mut TcpStream, seq: &mut u8, payload: &[u8]) -> Result<(), String> {
    let len = payload.len();
    if len > 0x00ff_ffff {
        return Err("MySQL 数据包过大".to_string());
    }
    let header = [
        (len & 0xff) as u8,
        ((len >> 8) & 0xff) as u8,
        ((len >> 16) & 0xff) as u8,
        *seq,
    ];
    *seq = seq.wrapping_add(1);
    stream
        .write_all(&header)
        .and_then(|_| stream.write_all(payload))
        .map_err(|error| format!("写入 MySQL 数据包失败: {error}"))
}

fn read_packet(stream: &mut TcpStream, seq: &mut u8) -> Result<Vec<u8>, String> {
    let mut header = [0u8; 4];
    stream
        .read_exact(&mut header)
        .map_err(|error| format!("读取 MySQL 包头失败: {error}"))?;
    let len = (header[0] as usize) | ((header[1] as usize) << 8) | ((header[2] as usize) << 16);
    *seq = header[3].wrapping_add(1);
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream
            .read_exact(&mut payload)
            .map_err(|error| format!("读取 MySQL 包体失败: {error}"))?;
    }
    Ok(payload)
}

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, String> {
    if *offset >= data.len() {
        return Err("MySQL 包截断 (u8)".to_string());
    }
    let value = data[*offset];
    *offset += 1;
    Ok(value)
}

fn read_u16_le(data: &[u8], offset: &mut usize) -> Result<u16, String> {
    if *offset + 2 > data.len() {
        return Err("MySQL 包截断 (u16)".to_string());
    }
    let value = u16::from_le_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;
    Ok(value)
}

fn read_u32_le(data: &[u8], offset: &mut usize) -> Result<u32, String> {
    if *offset + 4 > data.len() {
        return Err("MySQL 包截断 (u32)".to_string());
    }
    let value = u32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    Ok(value)
}

fn read_fixed<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> Result<&'a [u8], String> {
    if *offset + len > data.len() {
        return Err("MySQL 包截断 (fixed)".to_string());
    }
    let slice = &data[*offset..*offset + len];
    *offset += len;
    Ok(slice)
}

fn read_null_terminated_string(data: &[u8], offset: &mut usize) -> Result<String, String> {
    if *offset >= data.len() {
        return Ok(String::new());
    }
    if let Some(rel) = data[*offset..].iter().position(|b| *b == 0) {
        let value = String::from_utf8_lossy(&data[*offset..*offset + rel]).into_owned();
        *offset += rel + 1;
        Ok(value)
    } else {
        let value = String::from_utf8_lossy(&data[*offset..]).into_owned();
        *offset = data.len();
        Ok(value)
    }
}

fn write_lenenc_int(out: &mut Vec<u8>, value: u64) {
    if value < 251 {
        out.push(value as u8);
    } else if value < 65_536 {
        out.push(0xfc);
        out.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value < 16_777_216 {
        out.push(0xfd);
        out.push((value & 0xff) as u8);
        out.push(((value >> 8) & 0xff) as u8);
        out.push(((value >> 16) & 0xff) as u8);
    } else {
        out.push(0xfe);
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn read_lenenc_int(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let first = read_u8(data, offset)?;
    match first {
        0xfb => Ok(0), // NULL treated as 0 for count contexts
        0xfc => {
            let value = read_u16_le(data, offset)? as u64;
            Ok(value)
        }
        0xfd => {
            if *offset + 3 > data.len() {
                return Err("MySQL 包截断 (lenenc3)".to_string());
            }
            let value = (data[*offset] as u64)
                | ((data[*offset + 1] as u64) << 8)
                | ((data[*offset + 2] as u64) << 16);
            *offset += 3;
            Ok(value)
        }
        0xfe => {
            if *offset + 8 > data.len() {
                return Err("MySQL 包截断 (lenenc8)".to_string());
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&data[*offset..*offset + 8]);
            *offset += 8;
            Ok(u64::from_le_bytes(bytes))
        }
        other => Ok(other as u64),
    }
}

fn read_lenenc_string(data: &[u8], offset: &mut usize) -> Result<String, String> {
    if *offset >= data.len() {
        return Ok(String::new());
    }
    if data[*offset] == 0xfb {
        *offset += 1;
        return Ok("NULL".to_string());
    }
    let len = read_lenenc_int(data, offset)? as usize;
    let bytes = read_fixed(data, offset, len)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_password_scramble_is_20_bytes() {
        let scramble = b"01234567890123456789";
        let out = mysql_native_password("secret", scramble);
        assert_eq!(out.len(), 20);
    }

    #[test]
    fn caching_sha2_password_scramble_is_32_bytes() {
        let scramble = b"01234567890123456789012345678901";
        let out = caching_sha2_password("secret", scramble);
        assert_eq!(out.len(), 32);
    }
}
