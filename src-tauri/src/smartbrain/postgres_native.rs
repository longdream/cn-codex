//! Built-in PostgreSQL client for SmartBrain SQL queries.
//!
//! Uses `tokio-postgres` so agents never depend on a local `psql` CLI.

use std::time::Duration;

use tokio_postgres::NoTls;

#[derive(Debug, Clone)]
pub struct PostgresQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

pub async fn execute_postgres_query(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    sql: &str,
    timeout_sec: u64,
    row_limit: usize,
) -> Result<PostgresQueryResult, String> {
    let timeout = Duration::from_secs(timeout_sec.max(1));
    let host = host.trim();
    let username = username.trim();
    let database = database.trim();
    if host.is_empty() {
        return Err("PostgreSQL host 未配置。".to_string());
    }
    if database.is_empty() {
        return Err("PostgreSQL databaseName 未配置。".to_string());
    }

    let mut config = tokio_postgres::Config::new();
    config.host(host);
    config.port(port);
    config.user(if username.is_empty() {
        "postgres"
    } else {
        username
    });
    if !password.is_empty() {
        config.password(password);
    }
    config.dbname(database);
    config.connect_timeout(timeout);
    config.application_name("cn-codex-smartbrain");

    let connect_future = config.connect(NoTls);
    let (client, connection) = tokio::time::timeout(timeout, connect_future)
        .await
        .map_err(|_| format!("连接 PostgreSQL `{host}:{port}` 超时"))?
        .map_err(|error| format!("连接 PostgreSQL `{host}:{port}/{database}` 失败: {error}"))?;

    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::warn!("PostgreSQL connection closed with error: {error}");
        }
    });

    let query_future = client.simple_query(sql);
    let messages = tokio::time::timeout(timeout, query_future)
        .await
        .map_err(|_| "执行 PostgreSQL 查询超时".to_string())?
        .map_err(|error| format!("执行 PostgreSQL 查询失败: {error}"))?;

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;

    for message in messages {
        match message {
            tokio_postgres::SimpleQueryMessage::Row(row) => {
                if columns.is_empty() {
                    columns = (0..row.len())
                        .map(|idx| {
                            row.columns()
                                .get(idx)
                                .map(|col| col.name().to_string())
                                .unwrap_or_else(|| format!("col{idx}"))
                        })
                        .collect();
                }
                if rows.len() >= row_limit {
                    truncated = true;
                    continue;
                }
                let mut values = Vec::with_capacity(row.len());
                for idx in 0..row.len() {
                    values.push(match row.get(idx) {
                        Some(value) => truncate_cell(value),
                        None => "NULL".to_string(),
                    });
                }
                rows.push(values);
            }
            tokio_postgres::SimpleQueryMessage::CommandComplete(_) => {}
            // Keep forward-compat with future message variants.
            _ => {}
        }
    }

    // simple_query may return only command complete for DML; expose empty result set.
    Ok(PostgresQueryResult {
        columns,
        rows,
        truncated,
    })
}

pub async fn list_postgres_databases(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    timeout_sec: u64,
) -> Result<Vec<String>, String> {
    let result = execute_postgres_query(
        host,
        port,
        username,
        password,
        if database.trim().is_empty() {
            "postgres"
        } else {
            database
        },
        "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname;",
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

fn truncate_cell(value: &str) -> String {
    const MAX_CELL_CHARS: usize = 500;
    if value.chars().count() <= MAX_CELL_CHARS {
        return value.to_string();
    }
    let truncated: String = value.chars().take(MAX_CELL_CHARS).collect();
    format!("{truncated}…")
}
