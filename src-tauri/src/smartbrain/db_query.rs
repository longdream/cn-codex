//! Built-in Local Knowledge Base database SQL execution.
//!
//! Uses configured Local Knowledge Base DB sources (host/user/password already stored)
//! and executes SQL through an in-process MySQL protocol client when possible,
//! falling back to local database CLIs / rusqlite. This keeps the agent from
//! inventing Python connection scripts or asking for passwords.

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SMARTBRAIN_DB_SOURCES_STATE_KEY: &str = "smartbrain.db.sources";
pub const SMARTBRAIN_DB_SETTINGS_STATE_KEY: &str = "smartbrain.db.settings";

const DEFAULT_ROW_LIMIT: usize = 200;
const DEFAULT_TIMEOUT_SEC: u64 = 15;
const MAX_CELL_CHARS: usize = 500;
const MAX_OUTPUT_CHARS: usize = 60_000;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmartbrainDbPermissions {
    #[serde(default)]
    pub read_schema: bool,
    #[serde(default)]
    pub read_data: bool,
    #[serde(default)]
    pub write_data: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmartbrainDbSource {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub db_type: String,
    #[serde(default)]
    pub connection_uri: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub permissions: SmartbrainDbPermissions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartbrainDbSettings {
    #[serde(default = "default_row_limit")]
    pub default_row_limit: usize,
    #[serde(default = "default_timeout_sec")]
    pub default_timeout_sec: u64,
    #[serde(default = "default_true")]
    pub require_readonly_reminder: bool,
    #[serde(default = "default_true")]
    pub skip_when_no_permission: bool,
    #[serde(default = "default_true")]
    pub deny_ddl: bool,
    #[serde(default = "default_true")]
    pub deny_drop: bool,
    #[serde(default = "default_true")]
    pub deny_delete_without_write_permission: bool,
    #[serde(default)]
    pub rules_markdown: String,
}

impl Default for SmartbrainDbSettings {
    fn default() -> Self {
        Self {
            default_row_limit: default_row_limit(),
            default_timeout_sec: default_timeout_sec(),
            require_readonly_reminder: true,
            skip_when_no_permission: true,
            deny_ddl: true,
            deny_drop: true,
            deny_delete_without_write_permission: true,
            rules_markdown: String::new(),
        }
    }
}

fn default_row_limit() -> usize {
    DEFAULT_ROW_LIMIT
}

fn default_timeout_sec() -> u64 {
    DEFAULT_TIMEOUT_SEC
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlKind {
    ReadSchema,
    ReadData,
    WriteData,
    DangerousDdl,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartbrainSqlQueryResult {
    pub ok: bool,
    pub database: String,
    pub db_type: String,
    pub sql: String,
    pub row_count: usize,
    pub truncated: bool,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn load_workspace_state_value(workspace_config_dir: &Path, key: &str) -> Option<String> {
    let usage_db_path = workspace_config_dir.join("usage.db");
    if !usage_db_path.exists() {
        return None;
    }
    let usage_db = crate::usage::UsageDb::open(&usage_db_path).ok()?;
    usage_db.state_get(key).ok().flatten()
}

pub fn load_db_sources(workspace_config_dir: &Path) -> Vec<SmartbrainDbSource> {
    load_workspace_state_value(workspace_config_dir, SMARTBRAIN_DB_SOURCES_STATE_KEY)
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

pub fn load_db_settings(workspace_config_dir: &Path) -> SmartbrainDbSettings {
    load_workspace_state_value(workspace_config_dir, SMARTBRAIN_DB_SETTINGS_STATE_KEY)
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

fn source_has_any_permission(source: &SmartbrainDbSource) -> bool {
    source.permissions.read_schema || source.permissions.read_data || source.permissions.write_data
}

fn source_is_effectively_enabled(
    source: &SmartbrainDbSource,
    settings: &SmartbrainDbSettings,
) -> bool {
    if !source.enabled {
        return false;
    }
    if settings.skip_when_no_permission && !source_has_any_permission(source) {
        return false;
    }
    true
}

fn source_display_name(source: &SmartbrainDbSource) -> String {
    let trimmed_name = source.name.trim();
    if !trimmed_name.is_empty() {
        return trimmed_name.to_string();
    }
    let trimmed_db_name = source.database_name.trim();
    if !trimmed_db_name.is_empty() {
        return trimmed_db_name.to_string();
    }
    let trimmed_file_path = source.file_path.trim();
    if !trimmed_file_path.is_empty() {
        return Path::new(trimmed_file_path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| trimmed_file_path.to_string());
    }
    "未命名数据库".to_string()
}

fn normalize_match_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '`' && *ch != '"' && *ch != '\'')
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn source_aliases(source: &SmartbrainDbSource) -> Vec<String> {
    let mut aliases = Vec::new();
    for candidate in [
        source.name.as_str(),
        source.database_name.as_str(),
        source.id.as_str(),
        source.host.as_str(),
        source.file_path.as_str(),
    ] {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            continue;
        }
        aliases.push(normalize_match_key(trimmed));
        if let Some(file_name) = Path::new(trimmed)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
        {
            let key = normalize_match_key(&file_name);
            if !key.is_empty() {
                aliases.push(key);
            }
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

pub fn resolve_db_source<'a>(
    sources: &'a [SmartbrainDbSource],
    settings: &SmartbrainDbSettings,
    database: Option<&str>,
) -> Result<&'a SmartbrainDbSource, String> {
    let active: Vec<&SmartbrainDbSource> = sources
        .iter()
        .filter(|source| source_is_effectively_enabled(source, settings))
        .collect();

    if active.is_empty() {
        return Err(
            "没有可用的本地知识库数据库配置。请先在设置 → 本地知识库 → 数据库中配置并启用至少一个数据源。"
                .to_string(),
        );
    }

    let Some(raw_name) = database.map(str::trim).filter(|value| !value.is_empty()) else {
        if active.len() == 1 {
            return Ok(active[0]);
        }
        let names = active
            .iter()
            .map(|source| format!("`{}`", source_display_name(source)))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "存在多个可用数据库，请通过 database 参数指定其中一个：{names}"
        ));
    };

    let needle = normalize_match_key(raw_name);
    let mut exact = Vec::new();
    let mut partial = Vec::new();
    for source in &active {
        let aliases = source_aliases(source);
        if aliases.iter().any(|alias| alias == &needle) {
            exact.push(*source);
            continue;
        }
        if aliases
            .iter()
            .any(|alias| alias.contains(&needle) || needle.contains(alias))
        {
            partial.push(*source);
        }
    }

    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    if exact.len() > 1 {
        let names = exact
            .iter()
            .map(|source| format!("`{}`", source_display_name(source)))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("数据库名称 `{raw_name}` 匹配到多个配置：{names}"));
    }
    if partial.len() == 1 {
        return Ok(partial[0]);
    }
    if partial.len() > 1 {
        let names = partial
            .iter()
            .map(|source| format!("`{}`", source_display_name(source)))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("数据库名称 `{raw_name}` 匹配到多个配置：{names}"));
    }

    let names = active
        .iter()
        .map(|source| format!("`{}`", source_display_name(source)))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "未找到名为 `{raw_name}` 的本地知识库数据库配置。可用数据库：{names}"
    ))
}

fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        let next = bytes.get(i + 1).map(|b| *b as char);

        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                out.push(ch);
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if ch == '*' && next == Some('/') {
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if !in_single && !in_double {
            if ch == '-' && next == Some('-') {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if ch == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }
        if ch == '\'' && !in_double {
            // Handle escaped single quotes ''
            if in_single && next == Some('\'') {
                out.push(ch);
                out.push('\'');
                i += 2;
                continue;
            }
            in_single = !in_single;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            out.push(ch);
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn first_sql_keyword(sql: &str) -> String {
    let cleaned = strip_sql_comments(sql);
    cleaned
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .to_ascii_lowercase()
}

fn classify_sql(sql: &str) -> SqlKind {
    let keyword = first_sql_keyword(sql);
    match keyword.as_str() {
        "select" | "with" | "values" | "table" | "explain" | "analyze" | "show" | "describe"
        | "desc" | "pragma" => {
            let lower = strip_sql_comments(sql).to_ascii_lowercase();
            if lower.contains(" information_schema")
                || lower.contains(" pg_catalog")
                || lower.contains(" sys.")
                || lower.contains(" sqlite_master")
                || lower.contains(" show tables")
                || lower.contains(" show columns")
                || lower.contains(" describe ")
                || lower.contains(" desc ")
                || keyword == "show"
                || keyword == "describe"
                || keyword == "desc"
                || keyword == "pragma"
            {
                SqlKind::ReadSchema
            } else {
                SqlKind::ReadData
            }
        }
        "insert" | "update" | "delete" | "replace" | "merge" | "call" | "execute" | "exec" => {
            SqlKind::WriteData
        }
        "drop" | "truncate" | "alter" | "create" | "grant" | "revoke" | "rename" => {
            SqlKind::DangerousDdl
        }
        _ => SqlKind::Unknown,
    }
}

fn validate_sql_against_permissions(
    sql: &str,
    source: &SmartbrainDbSource,
    settings: &SmartbrainDbSettings,
) -> Result<SqlKind, String> {
    let kind = classify_sql(sql);
    match kind {
        SqlKind::ReadSchema => {
            if source.permissions.read_schema || source.permissions.read_data {
                Ok(kind)
            } else {
                Err(format!(
                    "数据库 `{}` 未开启 readSchema/readData 权限，拒绝执行结构查询。",
                    source_display_name(source)
                ))
            }
        }
        SqlKind::ReadData => {
            if source.permissions.read_data {
                Ok(kind)
            } else {
                Err(format!(
                    "数据库 `{}` 未开启 readData 权限，拒绝执行数据查询。",
                    source_display_name(source)
                ))
            }
        }
        SqlKind::WriteData => {
            if !source.permissions.write_data {
                return Err(format!(
                    "数据库 `{}` 未开启 writeData 权限，拒绝执行写操作。",
                    source_display_name(source)
                ));
            }
            let keyword = first_sql_keyword(sql);
            if settings.deny_delete_without_write_permission && keyword == "delete" {
                // write permission already checked; keep setting for future finer control.
            }
            Ok(kind)
        }
        SqlKind::DangerousDdl => {
            if settings.deny_ddl || settings.deny_drop || !source.permissions.write_data {
                return Err(format!(
                    "数据库 `{}` 禁止执行 DDL/高危语句（DROP/TRUNCATE/ALTER/CREATE 等）。",
                    source_display_name(source)
                ));
            }
            Ok(kind)
        }
        SqlKind::Unknown => Err(
            "无法识别 SQL 类型。仅支持明确的 SELECT/SHOW/DESCRIBE/INSERT/UPDATE/DELETE 等语句。"
                .to_string(),
        ),
    }
}

fn first_non_empty(values: &[&str]) -> Option<String> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn default_port_for_db_type(db_type: &str) -> Option<u16> {
    match db_type {
        "postgresql" | "postgres" => Some(5432),
        "mysql" => Some(3306),
        "sqlserver" | "mssql" => Some(1433),
        _ => None,
    }
}

fn normalize_db_protocol(db_type: &str, uri: &str) -> String {
    if uri.contains("://") {
        return uri.to_string();
    }
    match db_type {
        "postgresql" | "postgres" => format!("postgresql://{uri}"),
        "mysql" => format!("mysql://{uri}"),
        "sqlserver" | "mssql" => format!("sqlserver://{uri}"),
        _ => uri.to_string(),
    }
}

fn percent_decode_loose(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &value[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn parse_server_host_and_port(db_type: &str, raw_value: &str) -> (String, Option<u16>) {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    if db_type == "sqlserver" || db_type == "mssql" {
        if let Some((host, port)) = trimmed.rsplit_once(',') {
            if let Ok(port_value) = port.trim().parse::<u16>() {
                return (host.trim().to_string(), Some(port_value));
            }
        }
    }
    if let Some((host, port)) = trimmed.rsplit_once(':') {
        if let Ok(port_value) = port.trim().parse::<u16>() {
            return (host.trim().to_string(), Some(port_value));
        }
    }
    (trimmed.to_string(), None)
}

fn parse_connection_uri_fields(
    db_type: &str,
    connection_uri: &str,
) -> Option<(String, Option<u16>, String, String, String, String)> {
    let trimmed = connection_uri.trim();
    if trimmed.is_empty() {
        return None;
    }

    let db_type = db_type.trim().to_ascii_lowercase();
    if db_type == "sqlite" {
        let without_scheme = trimmed
            .strip_prefix("sqlite://")
            .or_else(|| trimmed.strip_prefix("file:"))
            .unwrap_or(trimmed);
        let without_query = without_scheme.split('?').next().unwrap_or(without_scheme);
        let file_path = percent_decode_loose(without_query);
        return Some((
            String::new(),
            None,
            Path::new(&file_path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| file_path.clone()),
            String::new(),
            String::new(),
            file_path,
        ));
    }

    let normalized = normalize_db_protocol(&db_type, trimmed);
    let without_scheme = normalized
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(normalized.as_str());
    let (authority_and_path, _query) = without_scheme
        .split_once('?')
        .map(|(left, right)| (left, Some(right)))
        .unwrap_or((without_scheme, None));

    let (credentials, host_path) = if let Some(at_index) = authority_and_path.rfind('@') {
        (
            Some(&authority_and_path[..at_index]),
            &authority_and_path[at_index + 1..],
        )
    } else {
        (None, authority_and_path)
    };

    let (username, password) = if let Some(credentials) = credentials {
        if let Some((user, pass)) = credentials.split_once(':') {
            (percent_decode_loose(user), percent_decode_loose(pass))
        } else {
            (percent_decode_loose(credentials), String::new())
        }
    } else {
        (String::new(), String::new())
    };

    let (host_port, path) = host_path
        .split_once('/')
        .map(|(host_port, path)| (host_port, path))
        .unwrap_or((host_path, ""));
    let (host, port) = parse_server_host_and_port(&db_type, host_port);
    let port = port.or_else(|| default_port_for_db_type(&db_type));
    let database_name = percent_decode_loose(path.trim_matches('/'));

    Some((host, port, database_name, username, password, String::new()))
}

fn enrich_source_from_connection_uri(mut source: SmartbrainDbSource) -> SmartbrainDbSource {
    if source.connection_uri.trim().is_empty() {
        return source;
    }
    let Some((host, port, database_name, username, password, file_path)) =
        parse_connection_uri_fields(&source.db_type, &source.connection_uri)
    else {
        return source;
    };
    if source.host.trim().is_empty() {
        source.host = host;
    }
    if source.port.is_none() {
        source.port = port;
    }
    if source.database_name.trim().is_empty() {
        source.database_name = database_name;
    }
    if source.username.trim().is_empty() {
        source.username = username;
    }
    if source.password.trim().is_empty() {
        source.password = password;
    }
    if source.file_path.trim().is_empty() {
        source.file_path = file_path;
    }
    source
}

fn ensure_single_statement(sql: &str) -> Result<String, String> {
    let cleaned = strip_sql_comments(sql).trim().to_string();
    if cleaned.is_empty() {
        return Err("SQL 不能为空。".to_string());
    }

    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in cleaned.chars() {
        if ch == '\'' && !in_double {
            in_single = !in_single;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            current.push(ch);
            continue;
        }
        if ch == ';' && !in_single && !in_double {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                statements.push(trimmed.to_string());
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }
    let trailing = current.trim();
    if !trailing.is_empty() {
        statements.push(trailing.to_string());
    }

    if statements.len() != 1 {
        return Err("一次只允许执行一条 SQL 语句。".to_string());
    }
    Ok(statements.remove(0))
}

fn truncate_cell(value: &str) -> String {
    let trimmed = value.replace('\r', " ").replace('\n', "\\n");
    if trimmed.chars().count() <= MAX_CELL_CHARS {
        return trimmed;
    }
    let clipped: String = trimmed.chars().take(MAX_CELL_CHARS).collect();
    format!("{clipped}…")
}

fn parse_tsv_table(stdout: &str, row_limit: usize) -> (Vec<String>, Vec<Vec<String>>, bool) {
    let mut lines = stdout
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return (Vec::new(), Vec::new(), false);
    }

    // Drop common CLI banners.
    lines.retain(|line| {
        let lower = line.to_ascii_lowercase();
        !(lower.starts_with("mysql:")
            || lower.starts_with("psql:")
            || lower.starts_with("sqlcmd")
            || lower.contains("rows affected")
            || lower.starts_with("(") && lower.contains("row"))
    });
    if lines.is_empty() {
        return (Vec::new(), Vec::new(), false);
    }

    let header = lines[0]
        .split('\t')
        .map(|part| part.trim().to_string())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut truncated = false;
    for line in lines.into_iter().skip(1) {
        if rows.len() >= row_limit {
            truncated = true;
            break;
        }
        let cols = line
            .split('\t')
            .map(|part| truncate_cell(part.trim()))
            .collect::<Vec<_>>();
        rows.push(cols);
    }
    (header, rows, truncated)
}

async fn run_process_capture(
    program: &str,
    args: &[String],
    env_vars: &[(&str, String)],
    timeout_sec: u64,
) -> Result<String, String> {
    use tokio::process::Command;

    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in env_vars {
        command.env(key, value);
    }

    let child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to execute {program}: {error}"))?;

    let output = tokio::time::timeout(
        Duration::from_secs(timeout_sec.max(1)),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| format!("{program} timed out after {timeout_sec}s"))?
    .map_err(|error| format!("Failed to wait for {program}: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("{program} exited with status {}", output.status)
        };
        return Err(detail);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn apply_row_limit_hint(sql: &str, db_type: &str, row_limit: usize) -> String {
    let keyword = first_sql_keyword(sql);
    if !matches!(keyword.as_str(), "select" | "with") {
        return sql.to_string();
    }
    let lower = sql.to_ascii_lowercase();
    if lower.contains(" limit ") || lower.contains(" fetch next ") || lower.contains(" top ") {
        return sql.to_string();
    }
    match db_type {
        "sqlserver" | "mssql" => {
            // Avoid rewriting complex SQL Server queries; rely on client-side truncation.
            sql.to_string()
        }
        _ => format!("{sql}\nLIMIT {row_limit}"),
    }
}

async fn execute_mysql(
    source: &SmartbrainDbSource,
    sql: &str,
    row_limit: usize,
    timeout_sec: u64,
) -> Result<(Vec<String>, Vec<Vec<String>>, bool, String), String> {
    let host = first_non_empty(&[&source.host]).unwrap_or_else(|| "127.0.0.1".to_string());
    let port = source.port.unwrap_or(3306);
    let username = first_non_empty(&[&source.username]).unwrap_or_else(|| "root".to_string());
    let database = first_non_empty(&[&source.database_name])
        .ok_or_else(|| "MySQL databaseName 未配置。".to_string())?;
    let password = source.password.clone();
    let limited_sql = apply_row_limit_hint(sql, "mysql", row_limit);

    // Prefer in-process wire protocol so agents never need Python/shell or a local mysql CLI.
    let native_result = {
        let host = host.clone();
        let username = username.clone();
        let password = password.clone();
        let database = database.clone();
        let limited_sql = limited_sql.clone();
        tokio::task::spawn_blocking(move || {
            crate::smartbrain::mysql_native::execute_mysql_query(
                &host,
                port,
                &username,
                &password,
                &database,
                &limited_sql,
                timeout_sec,
                row_limit,
            )
        })
        .await
        .map_err(|error| format!("MySQL 内置执行任务失败: {error}"))?
    };

    match native_result {
        Ok(result) => Ok((
            result.columns,
            result.rows,
            result.truncated,
            "Executed via built-in MySQL protocol client".to_string(),
        )),
        Err(native_error) => {
            // Optional CLI fallback when protocol auth is unsupported or blocked.
            let args = vec![
                format!("-h{host}"),
                format!("-P{port}"),
                format!("-u{username}"),
                format!("-D{database}"),
                "--batch".to_string(),
                "--raw".to_string(),
                "--default-character-set=utf8mb4".to_string(),
                "-e".to_string(),
                limited_sql,
            ];
            let env_vars = vec![("MYSQL_PWD", password)];
            match run_process_capture("mysql", &args, &env_vars, timeout_sec).await {
                Ok(stdout) => {
                    let (columns, rows, truncated) = parse_tsv_table(&stdout, row_limit);
                    Ok((
                        columns,
                        rows,
                        truncated,
                        format!("Executed via mysql CLI fallback (native client: {native_error})"),
                    ))
                }
                Err(cli_error) => Err(format!(
                    "通过内置 MySQL 协议客户端执行失败，且本机 mysql CLI 不可用。\
                     请确认本地知识库数据库配置完整（host/port/database/username/password）且网络可达。\
                     Native: {native_error}; CLI: {cli_error}"
                )),
            }
        }
    }
}

async fn execute_postgres(
    source: &SmartbrainDbSource,
    sql: &str,
    row_limit: usize,
    timeout_sec: u64,
) -> Result<(Vec<String>, Vec<Vec<String>>, bool, String), String> {
    let host = first_non_empty(&[&source.host]).unwrap_or_else(|| "127.0.0.1".to_string());
    let port = source.port.unwrap_or(5432);
    let username = first_non_empty(&[&source.username]).unwrap_or_else(|| "postgres".to_string());
    let database = first_non_empty(&[&source.database_name])
        .ok_or_else(|| "PostgreSQL databaseName 未配置。".to_string())?;
    let password = source.password.clone();
    let limited_sql = apply_row_limit_hint(sql, "postgresql", row_limit);

    let args = vec![
        "-h".to_string(),
        host,
        "-p".to_string(),
        port.to_string(),
        "-U".to_string(),
        username,
        "-d".to_string(),
        database,
        "-A".to_string(),
        "-F".to_string(),
        "\t".to_string(),
        "-c".to_string(),
        limited_sql,
    ];
    let env_vars = vec![("PGPASSWORD", password)];
    let stdout = run_process_capture("psql", &args, &env_vars, timeout_sec)
        .await
        .map_err(|error| {
            format!(
                "通过内置 PostgreSQL 客户端执行失败。请确认本机已安装 `psql` CLI 且网络可达。Detail: {error}"
            )
        })?;
    let (columns, rows, truncated) = parse_tsv_table(&stdout, row_limit);
    Ok((
        columns,
        rows,
        truncated,
        "Executed via built-in psql CLI".to_string(),
    ))
}

async fn execute_sqlserver(
    source: &SmartbrainDbSource,
    sql: &str,
    row_limit: usize,
    timeout_sec: u64,
) -> Result<(Vec<String>, Vec<Vec<String>>, bool, String), String> {
    let host =
        first_non_empty(&[&source.host]).ok_or_else(|| "SQL Server host 未配置。".to_string())?;
    let port = source.port.unwrap_or(1433);
    let username = first_non_empty(&[&source.username])
        .ok_or_else(|| "SQL Server username 未配置。".to_string())?;
    let password = source.password.clone();
    let database = first_non_empty(&[&source.database_name]).unwrap_or_default();
    let server = format!("{host},{port}");
    let limited_sql = apply_row_limit_hint(sql, "sqlserver", row_limit);

    let mut args = vec![
        "-S".to_string(),
        server,
        "-U".to_string(),
        username,
        "-P".to_string(),
        password,
        "-s".to_string(),
        "\t".to_string(),
        "-W".to_string(),
        "-Q".to_string(),
        format!("SET NOCOUNT ON; {limited_sql}"),
    ];
    if !database.is_empty() {
        args.splice(6..6, ["-d".to_string(), database]);
    }

    let stdout = run_process_capture("sqlcmd", &args, &[], timeout_sec)
        .await
        .map_err(|error| {
            format!(
                "通过内置 SQL Server 客户端执行失败。请确认本机已安装 `sqlcmd` 且网络可达。Detail: {error}"
            )
        })?;
    let (columns, rows, truncated) = parse_tsv_table(&stdout, row_limit);
    Ok((
        columns,
        rows,
        truncated,
        "Executed via built-in sqlcmd CLI".to_string(),
    ))
}

fn execute_sqlite(
    source: &SmartbrainDbSource,
    sql: &str,
    row_limit: usize,
) -> Result<(Vec<String>, Vec<Vec<String>>, bool, String), String> {
    let path = first_non_empty(&[
        &source.file_path,
        &source.database_name,
        &source.connection_uri,
    ])
    .ok_or_else(|| "SQLite filePath 未配置。".to_string())?;
    let conn = Connection::open(&path).map_err(|error| format!("打开 SQLite 失败: {error}"))?;
    let mut stmt = conn
        .prepare(sql)
        .map_err(|error| format!("准备 SQL 失败: {error}"))?;

    let columns = stmt
        .column_names()
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();

    let column_count = columns.len();
    let mut rows = Vec::new();
    let mut truncated = false;
    let mut rows_iter = stmt
        .query([])
        .map_err(|error| format!("执行 SQL 失败: {error}"))?;

    while let Some(row) = rows_iter
        .next()
        .map_err(|error| format!("读取结果失败: {error}"))?
    {
        if rows.len() >= row_limit {
            truncated = true;
            break;
        }
        let mut values = Vec::with_capacity(column_count);
        for idx in 0..column_count {
            let value: Value = match row.get_ref(idx) {
                Ok(rusqlite::types::ValueRef::Null) => Value::Null,
                Ok(rusqlite::types::ValueRef::Integer(v)) => Value::from(v),
                Ok(rusqlite::types::ValueRef::Real(v)) => Value::from(v),
                Ok(rusqlite::types::ValueRef::Text(v)) => {
                    Value::String(String::from_utf8_lossy(v).to_string())
                }
                Ok(rusqlite::types::ValueRef::Blob(v)) => {
                    Value::String(format!("<blob {} bytes>", v.len()))
                }
                Err(error) => Value::String(format!("<error: {error}>")),
            };
            let rendered = match value {
                Value::Null => "NULL".to_string(),
                other => other.to_string().trim_matches('"').to_string(),
            };
            values.push(truncate_cell(&rendered));
        }
        rows.push(values);
    }

    Ok((
        columns,
        rows,
        truncated,
        "Executed via built-in rusqlite".to_string(),
    ))
}

pub async fn execute_smartbrain_sql_query(
    workspace_config_dir: &Path,
    database: Option<&str>,
    sql: &str,
    row_limit: Option<usize>,
    timeout_sec: Option<u64>,
) -> Result<SmartbrainSqlQueryResult, String> {
    let settings = load_db_settings(workspace_config_dir);
    let sources = load_db_sources(workspace_config_dir)
        .into_iter()
        .map(enrich_source_from_connection_uri)
        .collect::<Vec<_>>();
    let source = resolve_db_source(&sources, &settings, database)?.clone();
    let sql = ensure_single_statement(sql)?;
    let _kind = validate_sql_against_permissions(&sql, &source, &settings)?;

    let row_limit = row_limit
        .unwrap_or(settings.default_row_limit)
        .clamp(1, 1000);
    let timeout_sec = timeout_sec
        .unwrap_or(settings.default_timeout_sec)
        .clamp(1, 120);
    let db_type = source.db_type.trim().to_ascii_lowercase();
    let display_name = source_display_name(&source);

    let (columns, rows, truncated, engine_message) = match db_type.as_str() {
        "mysql" => execute_mysql(&source, &sql, row_limit, timeout_sec).await?,
        "postgresql" | "postgres" => {
            execute_postgres(&source, &sql, row_limit, timeout_sec).await?
        }
        "sqlserver" | "mssql" => execute_sqlserver(&source, &sql, row_limit, timeout_sec).await?,
        "sqlite" => execute_sqlite(&source, &sql, row_limit)?,
        other => {
            return Err(format!(
                "暂不支持的数据库类型 `{other}`。当前内置支持 mysql / postgresql / sqlserver / sqlite。"
            ));
        }
    };

    Ok(SmartbrainSqlQueryResult {
        ok: true,
        database: display_name,
        db_type,
        sql,
        row_count: rows.len(),
        truncated,
        columns,
        rows,
        message: engine_message,
        error: None,
    })
}

pub fn format_sql_query_result(result: &SmartbrainSqlQueryResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "本地知识库 SQL 查询成功\n数据库: {}\n类型: {}\n返回行数: {}{}\nSQL:\n{}\n\n",
        result.database,
        result.db_type,
        result.row_count,
        if result.truncated {
            "（已截断）"
        } else {
            ""
        },
        result.sql
    ));

    if result.columns.is_empty() && result.rows.is_empty() {
        out.push_str("（无结果集，语句已执行）\n");
        return out;
    }

    out.push_str(&result.columns.join(" | "));
    out.push('\n');
    out.push_str(&"-".repeat(result.columns.join(" | ").chars().count().max(8)));
    out.push('\n');
    for row in &result.rows {
        let mut cells = Vec::with_capacity(result.columns.len().max(row.len()));
        for idx in 0..result.columns.len().max(row.len()) {
            cells.push(row.get(idx).cloned().unwrap_or_default());
        }
        out.push_str(&cells.join(" | "));
        out.push('\n');
        if out.len() >= MAX_OUTPUT_CHARS {
            out.push_str("…(output truncated)\n");
            break;
        }
    }
    out.push('\n');
    out.push_str(&result.message);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_select_as_read_data() {
        assert_eq!(
            classify_sql("SELECT * FROM pact_main LIMIT 10"),
            SqlKind::ReadData
        );
    }

    #[test]
    fn classify_show_tables_as_schema() {
        assert_eq!(classify_sql("SHOW TABLES"), SqlKind::ReadSchema);
    }

    #[test]
    fn reject_multi_statement() {
        let err = ensure_single_statement("SELECT 1; SELECT 2;").unwrap_err();
        assert!(err.contains("一条"));
    }

    #[test]
    fn resolve_by_display_name_or_physical_name() {
        let settings = SmartbrainDbSettings::default();
        let sources = vec![SmartbrainDbSource {
            name: "合同数据库".to_string(),
            db_type: "mysql".to_string(),
            enabled: true,
            host: "10.0.0.1".to_string(),
            database_name: "psa_crm_pact_test".to_string(),
            permissions: SmartbrainDbPermissions {
                read_schema: true,
                read_data: true,
                write_data: false,
            },
            ..Default::default()
        }];
        let by_name = resolve_db_source(&sources, &settings, Some("合同数据库")).unwrap();
        assert_eq!(by_name.database_name, "psa_crm_pact_test");
        let by_db = resolve_db_source(&sources, &settings, Some("psa_crm_pact_test")).unwrap();
        assert_eq!(by_db.name, "合同数据库");
    }
}
