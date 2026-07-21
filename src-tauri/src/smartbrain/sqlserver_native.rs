//! Built-in SQL Server client for SmartBrain SQL queries.
//!
//! Uses `tiberius` (TDS) so agents never depend on a local `sqlcmd` CLI.

use std::time::Duration;

use futures_util::stream::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

#[derive(Debug, Clone)]
pub struct SqlServerQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

pub async fn execute_sqlserver_query(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    sql: &str,
    timeout_sec: u64,
    row_limit: usize,
) -> Result<SqlServerQueryResult, String> {
    let timeout = Duration::from_secs(timeout_sec.max(1));
    let host = host.trim();
    let username = username.trim();
    if host.is_empty() {
        return Err("SQL Server host 未配置。".to_string());
    }
    if username.is_empty() {
        return Err("SQL Server username 未配置。".to_string());
    }

    let mut config = Config::new();
    config.host(host);
    config.port(port);
    config.authentication(AuthMethod::sql_server(username, password));
    if !database.trim().is_empty() {
        config.database(database.trim());
    }
    // Many on-prem / docker SQL Server deployments use self-signed certs.
    config.trust_cert();

    let addr = config.get_addr();
    let tcp = tokio::time::timeout(timeout, TcpStream::connect(addr.as_str()))
        .await
        .map_err(|_| format!("连接 SQL Server `{addr}` 超时"))?
        .map_err(|error| format!("连接 SQL Server `{addr}` 失败: {error}"))?;
    tcp.set_nodelay(true)
        .map_err(|error| format!("设置 SQL Server TCP_NODELAY 失败: {error}"))?;

    let connect_future = Client::connect(config, tcp.compat_write());
    let mut client = tokio::time::timeout(timeout, connect_future)
        .await
        .map_err(|_| format!("SQL Server 握手超时 (`{addr}`)"))?
        .map_err(|error| format!("SQL Server 登录失败 (`{addr}`): {error}"))?;

    let query_future = client.query(sql, &[]);
    let mut stream = tokio::time::timeout(timeout, query_future)
        .await
        .map_err(|_| "执行 SQL Server 查询超时".to_string())?
        .map_err(|error| format!("执行 SQL Server 查询失败: {error}"))?;

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;

    loop {
        let next = tokio::time::timeout(timeout, stream.try_next())
            .await
            .map_err(|_| "读取 SQL Server 结果超时".to_string())?
            .map_err(|error| format!("读取 SQL Server 结果失败: {error}"))?;
        let Some(item) = next else {
            break;
        };
        match item {
            QueryItem::Metadata(meta) => {
                columns = meta
                    .columns()
                    .iter()
                    .map(|col| col.name().to_string())
                    .collect();
            }
            QueryItem::Row(row) => {
                if rows.len() >= row_limit {
                    truncated = true;
                    continue;
                }
                let mut values = Vec::with_capacity(row.len());
                for data in row.into_iter() {
                    values.push(render_column_data(data));
                }
                rows.push(values);
            }
        }
    }

    Ok(SqlServerQueryResult {
        columns,
        rows,
        truncated,
    })
}

pub async fn list_sqlserver_databases(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    timeout_sec: u64,
) -> Result<Vec<String>, String> {
    let result = execute_sqlserver_query(
        host,
        port,
        username,
        password,
        "",
        "SET NOCOUNT ON; SELECT name FROM sys.databases ORDER BY name;",
        timeout_sec,
        1000,
    )
    .await?;
    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| row.into_iter().next())
        .filter(|name| !name.trim().is_empty() && name != "NULL")
        .collect())
}

fn render_column_data(data: ColumnData<'_>) -> String {
    match data {
        ColumnData::U8(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::I16(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::I32(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::I64(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::F32(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::F64(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::Bit(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::String(v) => v
            .map(|x| truncate_cell(x.as_ref()))
            .unwrap_or_else(|| "NULL".into()),
        ColumnData::Guid(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::Binary(v) => v
            .map(|bytes| format!("<blob {} bytes>", bytes.len()))
            .unwrap_or_else(|| "NULL".into()),
        ColumnData::Numeric(v) => v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into()),
        ColumnData::Xml(v) => v
            .map(|x| truncate_cell(&x.to_string()))
            .unwrap_or_else(|| "NULL".into()),
        ColumnData::DateTime(v) => v.map(|x| format!("{x:?}")).unwrap_or_else(|| "NULL".into()),
        ColumnData::SmallDateTime(v) => {
            v.map(|x| format!("{x:?}")).unwrap_or_else(|| "NULL".into())
        }
        ColumnData::Time(v) => v.map(|x| format!("{x:?}")).unwrap_or_else(|| "NULL".into()),
        ColumnData::Date(v) => v.map(|x| format!("{x:?}")).unwrap_or_else(|| "NULL".into()),
        ColumnData::DateTime2(v) => {
            v.map(|x| format!("{x:?}")).unwrap_or_else(|| "NULL".into())
        }
        ColumnData::DateTimeOffset(v) => {
            v.map(|x| format!("{x:?}")).unwrap_or_else(|| "NULL".into())
        }
    }
}

fn truncate_cell(value: &str) -> String {
    const MAX_CELL_CHARS: usize = 500;
    if value.chars().count() <= MAX_CELL_CHARS {
        return value.to_string();
    }
    let truncated: String = value.chars().take(MAX_CELL_CHARS).collect();
    format!("{truncated}…")
}
