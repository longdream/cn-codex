//! SmartBrain entry-form tools:
//! - build form from database schema
//! - collect user input via UI
//! - validate and insert rows

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::db_query::{
    execute_smartbrain_sql_query, load_db_settings, load_db_sources, resolve_db_source,
    SmartbrainDbSource, SmartbrainSqlQueryResult,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub db_type: String,
    pub input_type: String,
    pub required: bool,
    pub readonly: bool,
    pub auto_increment: bool,
    pub primary_key: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntryForm {
    pub database: String,
    pub db_type: String,
    pub table: String,
    pub title: String,
    pub description: String,
    pub fields: Vec<FormField>,
    #[serde(default)]
    pub known_values: Map<String, Value>,
    #[serde(default)]
    pub matched_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveFormResult {
    pub ok: bool,
    pub database: String,
    pub table: String,
    pub inserted: bool,
    pub sql: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_message: Option<String>,
}

#[derive(Debug, Clone)]
struct ColumnMeta {
    name: String,
    data_type: String,
    column_type: String,
    is_nullable: bool,
    column_key: String,
    extra: String,
    column_default: Option<String>,
    comment: Option<String>,
    max_length: Option<u64>,
    numeric_precision: Option<u64>,
    numeric_scale: Option<u64>,
}

#[derive(Debug, Clone)]
struct TableMeta {
    name: String,
    comment: Option<String>,
    columns: Vec<ColumnMeta>,
}

pub async fn build_entry_form(
    workspace_config_dir: &Path,
    database: Option<&str>,
    known_values: Map<String, Value>,
    table: Option<&str>,
    intent: Option<&str>,
) -> Result<EntryForm, String> {
    let settings = load_db_settings(workspace_config_dir);
    let sources = load_db_sources(workspace_config_dir)
        .into_iter()
        .map(crate::smartbrain::db_query::enrich_source_from_connection_uri)
        .collect::<Vec<_>>();
    let source = resolve_db_source(&sources, &settings, database)?.clone();

    if !(source.permissions.read_schema || source.permissions.read_data) {
        return Err(format!(
            "数据库 `{}` 未开启 readSchema/readData 权限，无法生成入库表单。",
            display_name(&source)
        ));
    }

    let tables = load_table_metas(workspace_config_dir, &source).await?;
    if tables.is_empty() {
        return Err(format!(
            "数据库 `{}` 中没有可读表。请确认账号权限与 schema。",
            display_name(&source)
        ));
    }

    let (selected, matched_by) = select_target_table(&tables, table, intent, &known_values)?;
    let fields = selected
        .columns
        .iter()
        .map(|column| column_to_form_field(column, &known_values))
        .collect::<Vec<_>>();

    let title = selected
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("录入 {}", selected.name));

    Ok(EntryForm {
        database: display_name(&source),
        db_type: source.db_type.clone(),
        table: selected.name.clone(),
        title,
        description: format!(
            "目标库：{} · 目标表：{} · 匹配方式：{}",
            display_name(&source),
            selected.name,
            matched_by
        ),
        fields,
        known_values,
        matched_by,
    })
}

pub async fn save_form_data(
    workspace_config_dir: &Path,
    database: Option<&str>,
    table: &str,
    values: Map<String, Value>,
) -> Result<SaveFormResult, String> {
    let settings = load_db_settings(workspace_config_dir);
    let sources = load_db_sources(workspace_config_dir)
        .into_iter()
        .map(crate::smartbrain::db_query::enrich_source_from_connection_uri)
        .collect::<Vec<_>>();
    let source = resolve_db_source(&sources, &settings, database)?.clone();

    if !source.permissions.write_data {
        return Err(format!(
            "数据库 `{}` 未开启 writeData 权限，拒绝写入。",
            display_name(&source)
        ));
    }

    let table_name = table.trim();
    if table_name.is_empty() {
        return Err("table 不能为空".to_string());
    }
    validate_ident(table_name, "table")?;

    let tables = load_table_metas(workspace_config_dir, &source).await?;
    let table_meta = tables
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(table_name))
        .ok_or_else(|| format!("未找到表 `{table_name}`"))?;

    let mut insert_columns = Vec::new();
    let mut insert_values = Vec::new();
    let mut missing_required = Vec::new();

    for column in &table_meta.columns {
        let auto_inc = column.extra.to_ascii_lowercase().contains("auto_increment");
        let has_default = column.column_default.is_some()
            || column
                .extra
                .to_ascii_lowercase()
                .contains("default_generated");
        let provided = values
            .get(&column.name)
            .or_else(|| {
                values.iter().find_map(|(key, value)| {
                    if key.eq_ignore_ascii_case(&column.name) {
                        Some(value)
                    } else {
                        None
                    }
                })
            })
            .cloned();

        match provided {
            Some(value) if !is_empty_value(&value) => {
                let sql_value = value_to_sql_literal(&value, column)?;
                insert_columns.push(quote_ident(&column.name, &source.db_type));
                insert_values.push(sql_value);
            }
            _ => {
                if !column.is_nullable && !auto_inc && !has_default {
                    missing_required.push(column.name.clone());
                }
            }
        }
    }

    if !missing_required.is_empty() {
        return Err(format!(
            "缺少必填字段：{}",
            missing_required.join(", ")
        ));
    }
    if insert_columns.is_empty() {
        return Err("没有可写入的字段值".to_string());
    }

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_ident(table_name, &source.db_type),
        insert_columns.join(", "),
        insert_values.join(", ")
    );

    match execute_smartbrain_sql_query(
        workspace_config_dir,
        Some(&display_name(&source)),
        &sql,
        Some(1),
        Some(30),
    )
    .await
    {
        Ok(result) => Ok(SaveFormResult {
            ok: true,
            database: display_name(&source),
            table: table_name.to_string(),
            inserted: true,
            sql: result.sql,
            message: format!("已成功写入表 `{table_name}`"),
            error: None,
            engine_message: Some(result.message),
        }),
        Err(error) => Ok(SaveFormResult {
            ok: false,
            database: display_name(&source),
            table: table_name.to_string(),
            inserted: false,
            sql,
            message: "写入失败".to_string(),
            error: Some(error),
            engine_message: None,
        }),
    }
}

pub async fn test_database_connection_from_source(
    source: &SmartbrainDbSource,
    timeout_sec: u64,
) -> Result<String, String> {
    let db_type = source.db_type.trim().to_ascii_lowercase();
    let probe_sql = match db_type.as_str() {
        "mysql" | "postgresql" | "postgres" | "sqlite" => "SELECT 1 AS ok",
        "sqlserver" | "mssql" => "SELECT 1 AS ok",
        other => {
            return Err(format!("暂不支持的数据库类型 `{other}`"));
        }
    };

    // Reuse the in-process query path through a temporary one-shot by constructing
    // SQL execution helpers already used by smartbrain_sql_query.
    match db_type.as_str() {
        "mysql" => {
            let host = nonempty(&source.host).unwrap_or_else(|| "127.0.0.1".to_string());
            let port = source.port.unwrap_or(3306);
            let username = nonempty(&source.username).unwrap_or_else(|| "root".to_string());
            let database = nonempty(&source.database_name)
                .ok_or_else(|| "MySQL databaseName 未配置。".to_string())?;
            let result = crate::smartbrain::mysql_native::execute_mysql_query(
                &host,
                port,
                &username,
                &source.password,
                &database,
                probe_sql,
                timeout_sec.max(1),
                1,
            )?;
            Ok(format!(
                "连接成功（MySQL 内置协议）· 目标 `{host}:{port}/{database}` · 探测返回 {} 行",
                result.rows.len()
            ))
        }
        "sqlite" => {
            let path = nonempty(&source.file_path)
                .or_else(|| nonempty(&source.connection_uri))
                .ok_or_else(|| "SQLite filePath 未配置。".to_string())?;
            let conn = rusqlite::Connection::open(&path)
                .map_err(|error| format!("打开 SQLite 失败: {error}"))?;
            let value: i64 = conn
                .query_row(probe_sql, [], |row| row.get(0))
                .map_err(|error| format!("SQLite 探测失败: {error}"))?;
            Ok(format!("连接成功（SQLite）· 文件 `{path}` · 探测结果={value}"))
        }
        "postgresql" | "postgres" | "sqlserver" | "mssql" => {
            // CLI-based engines: attempt SELECT 1 via existing execute path helpers.
            // We call through execute_smartbrain_sql_query only when workspace is available.
            Err(
                "请使用 smartbrain_test_database_connection 命令（带完整连接参数）进行探测。".into(),
            )
        }
        _ => unreachable!(),
    }
}

pub async fn test_connection_with_workspace(
    workspace_config_dir: &Path,
    database: Option<&str>,
    timeout_sec: Option<u64>,
) -> Result<String, String> {
    let settings = load_db_settings(workspace_config_dir);
    let sources = load_db_sources(workspace_config_dir)
        .into_iter()
        .map(crate::smartbrain::db_query::enrich_source_from_connection_uri)
        .collect::<Vec<_>>();
    let source = resolve_db_source(&sources, &settings, database)?.clone();
    let timeout = timeout_sec.unwrap_or(10).clamp(1, 60);

    // Prefer native/simple path first for mysql/sqlite, then generic SELECT 1.
    match source.db_type.trim().to_ascii_lowercase().as_str() {
        "mysql" | "sqlite" => test_database_connection_from_source(&source, timeout).await,
        _ => {
            let result = execute_smartbrain_sql_query(
                workspace_config_dir,
                Some(&display_name(&source)),
                "SELECT 1 AS ok",
                Some(1),
                Some(timeout),
            )
            .await?;
            Ok(format!(
                "连接成功 · 数据库 `{}`（{}）· {}",
                result.database, result.db_type, result.message
            ))
        }
    }
}

async fn load_table_metas(
    workspace_config_dir: &Path,
    source: &SmartbrainDbSource,
) -> Result<Vec<TableMeta>, String> {
    let db_type = source.db_type.trim().to_ascii_lowercase();
    match db_type.as_str() {
        "mysql" => load_mysql_table_metas(workspace_config_dir, source).await,
        "postgresql" | "postgres" => load_postgres_table_metas(workspace_config_dir, source).await,
        "sqlite" => load_sqlite_table_metas(workspace_config_dir, source).await,
        "sqlserver" | "mssql" => load_sqlserver_table_metas(workspace_config_dir, source).await,
        other => Err(format!("暂不支持的数据库类型 `{other}` 生成表单")),
    }
}

async fn load_mysql_table_metas(
    workspace_config_dir: &Path,
    source: &SmartbrainDbSource,
) -> Result<Vec<TableMeta>, String> {
    let database_name = nonempty(&source.database_name)
        .ok_or_else(|| "MySQL databaseName 未配置。".to_string())?;
    let sql = format!(
        "SELECT c.TABLE_NAME AS TABLE_NAME, \
                COALESCE(t.TABLE_COMMENT, '') AS TABLE_COMMENT, \
                c.COLUMN_NAME AS COLUMN_NAME, \
                c.DATA_TYPE AS DATA_TYPE, \
                c.COLUMN_TYPE AS COLUMN_TYPE, \
                c.IS_NULLABLE AS IS_NULLABLE, \
                c.COLUMN_KEY AS COLUMN_KEY, \
                c.EXTRA AS EXTRA, \
                c.COLUMN_DEFAULT AS COLUMN_DEFAULT, \
                c.COLUMN_COMMENT AS COLUMN_COMMENT, \
                c.CHARACTER_MAXIMUM_LENGTH AS CHARACTER_MAXIMUM_LENGTH, \
                c.NUMERIC_PRECISION AS NUMERIC_PRECISION, \
                c.NUMERIC_SCALE AS NUMERIC_SCALE \
         FROM information_schema.COLUMNS c \
         LEFT JOIN information_schema.TABLES t \
           ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
         WHERE c.TABLE_SCHEMA = {} \
         ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION",
        quote_string(&database_name)
    );
    let result = execute_smartbrain_sql_query(
        workspace_config_dir,
        Some(&display_name(source)),
        &sql,
        Some(1000),
        Some(30),
    )
    .await?;
    Ok(group_columns_from_rows(&result, true))
}

async fn load_postgres_table_metas(
    workspace_config_dir: &Path,
    source: &SmartbrainDbSource,
) -> Result<Vec<TableMeta>, String> {
    let schema = nonempty(&source.schema).unwrap_or_else(|| "public".to_string());
    let sql = format!(
        "SELECT c.table_name AS table_name, \
                COALESCE(obj_description((quote_ident(c.table_schema)||'.'||quote_ident(c.table_name))::regclass), '') AS table_comment, \
                c.column_name AS column_name, \
                c.data_type AS data_type, \
                c.udt_name AS column_type, \
                c.is_nullable AS is_nullable, \
                CASE WHEN tc.constraint_type = 'PRIMARY KEY' THEN 'PRI' ELSE '' END AS column_key, \
                CASE WHEN c.is_identity = 'YES' OR c.column_default LIKE 'nextval%' THEN 'auto_increment' ELSE COALESCE(c.column_default, '') END AS extra, \
                c.column_default AS column_default, \
                COALESCE(col_description((quote_ident(c.table_schema)||'.'||quote_ident(c.table_name))::regclass, c.ordinal_position), '') AS column_comment, \
                c.character_maximum_length AS character_maximum_length, \
                c.numeric_precision AS numeric_precision, \
                c.numeric_scale AS numeric_scale \
         FROM information_schema.columns c \
         LEFT JOIN information_schema.key_column_usage kcu \
           ON c.table_schema = kcu.table_schema AND c.table_name = kcu.table_name AND c.column_name = kcu.column_name \
         LEFT JOIN information_schema.table_constraints tc \
           ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema AND tc.constraint_type = 'PRIMARY KEY' \
         WHERE c.table_schema = {} \
         ORDER BY c.table_name, c.ordinal_position",
        quote_string(&schema)
    );
    let result = execute_smartbrain_sql_query(
        workspace_config_dir,
        Some(&display_name(source)),
        &sql,
        Some(1000),
        Some(30),
    )
    .await?;
    Ok(group_columns_from_rows(&result, false))
}

async fn load_sqlite_table_metas(
    workspace_config_dir: &Path,
    source: &SmartbrainDbSource,
) -> Result<Vec<TableMeta>, String> {
    let tables_result = execute_smartbrain_sql_query(
        workspace_config_dir,
        Some(&display_name(source)),
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        Some(500),
        Some(20),
    )
    .await?;

    let mut tables = Vec::new();
    for row in tables_result.rows {
        let Some(table_name) = row.first().map(|value| value.trim().to_string()) else {
            continue;
        };
        if table_name.is_empty() {
            continue;
        }
        validate_ident(&table_name, "table")?;
        let pragma_sql = format!("PRAGMA table_info({})", quote_ident(&table_name, "sqlite"));
        let cols = execute_smartbrain_sql_query(
            workspace_config_dir,
            Some(&display_name(source)),
            &pragma_sql,
            Some(500),
            Some(20),
        )
        .await?;
        // PRAGMA table_info: cid, name, type, notnull, dflt_value, pk
        let mut columns = Vec::new();
        for col in cols.rows {
            let name = col.get(1).cloned().unwrap_or_default();
            if name.trim().is_empty() {
                continue;
            }
            let data_type = col.get(2).cloned().unwrap_or_else(|| "TEXT".to_string());
            let notnull = col.get(3).map(|value| value == "1").unwrap_or(false);
            let default_value = col.get(4).cloned().filter(|value| value != "NULL");
            let pk = col.get(5).map(|value| value == "1").unwrap_or(false);
            columns.push(ColumnMeta {
                name,
                data_type: data_type.clone(),
                column_type: data_type,
                is_nullable: !notnull,
                column_key: if pk { "PRI".to_string() } else { String::new() },
                extra: if pk {
                    "auto_increment".to_string()
                } else {
                    String::new()
                },
                column_default: default_value,
                comment: None,
                max_length: None,
                numeric_precision: None,
                numeric_scale: None,
            });
        }
        tables.push(TableMeta {
            name: table_name,
            comment: None,
            columns,
        });
    }
    Ok(tables)
}

async fn load_sqlserver_table_metas(
    workspace_config_dir: &Path,
    source: &SmartbrainDbSource,
) -> Result<Vec<TableMeta>, String> {
    let schema = nonempty(&source.schema).unwrap_or_else(|| "dbo".to_string());
    let sql = format!(
        "SELECT t.name AS table_name, \
                ISNULL(CAST(ep_table.value AS NVARCHAR(500)), '') AS table_comment, \
                c.name AS column_name, \
                ty.name AS data_type, \
                ty.name AS column_type, \
                CASE WHEN c.is_nullable = 1 THEN 'YES' ELSE 'NO' END AS is_nullable, \
                CASE WHEN pk.column_id IS NOT NULL THEN 'PRI' ELSE '' END AS column_key, \
                CASE WHEN c.is_identity = 1 THEN 'auto_increment' ELSE '' END AS extra, \
                ISNULL(dc.definition, '') AS column_default, \
                ISNULL(CAST(ep_col.value AS NVARCHAR(500)), '') AS column_comment, \
                c.max_length AS character_maximum_length, \
                c.precision AS numeric_precision, \
                c.scale AS numeric_scale \
         FROM sys.tables t \
         INNER JOIN sys.columns c ON t.object_id = c.object_id \
         INNER JOIN sys.types ty ON c.user_type_id = ty.user_type_id \
         INNER JOIN sys.schemas s ON t.schema_id = s.schema_id \
         LEFT JOIN sys.extended_properties ep_table \
           ON ep_table.major_id = t.object_id AND ep_table.minor_id = 0 AND ep_table.name = 'MS_Description' \
         LEFT JOIN sys.extended_properties ep_col \
           ON ep_col.major_id = t.object_id AND ep_col.minor_id = c.column_id AND ep_col.name = 'MS_Description' \
         LEFT JOIN sys.default_constraints dc ON c.default_object_id = dc.object_id \
         LEFT JOIN ( \
             SELECT ic.object_id, ic.column_id \
             FROM sys.index_columns ic \
             INNER JOIN sys.indexes i ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             WHERE i.is_primary_key = 1 \
         ) pk ON pk.object_id = t.object_id AND pk.column_id = c.column_id \
         WHERE s.name = {} \
         ORDER BY t.name, c.column_id",
        quote_string(&schema)
    );
    let result = execute_smartbrain_sql_query(
        workspace_config_dir,
        Some(&display_name(source)),
        &sql,
        Some(1000),
        Some(30),
    )
    .await?;
    Ok(group_columns_from_rows(&result, false))
}

fn group_columns_from_rows(result: &SmartbrainSqlQueryResult, mysql_style: bool) -> Vec<TableMeta> {
    let index = |name: &str| {
        result
            .columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case(name))
    };
    let table_name_i = index("TABLE_NAME").or_else(|| index("table_name"));
    let table_comment_i = index("TABLE_COMMENT").or_else(|| index("table_comment"));
    let column_name_i = index("COLUMN_NAME").or_else(|| index("column_name"));
    let data_type_i = index("DATA_TYPE").or_else(|| index("data_type"));
    let column_type_i = index("COLUMN_TYPE").or_else(|| index("column_type"));
    let nullable_i = index("IS_NULLABLE").or_else(|| index("is_nullable"));
    let key_i = index("COLUMN_KEY").or_else(|| index("column_key"));
    let extra_i = index("EXTRA").or_else(|| index("extra"));
    let default_i = index("COLUMN_DEFAULT").or_else(|| index("column_default"));
    let comment_i = index("COLUMN_COMMENT").or_else(|| index("column_comment"));
    let max_len_i = index("CHARACTER_MAXIMUM_LENGTH")
        .or_else(|| index("character_maximum_length"));
    let precision_i = index("NUMERIC_PRECISION").or_else(|| index("numeric_precision"));
    let scale_i = index("NUMERIC_SCALE").or_else(|| index("numeric_scale"));

    let mut ordered: BTreeMap<String, TableMeta> = BTreeMap::new();
    for row in &result.rows {
        let table_name = table_name_i
            .and_then(|i| row.get(i))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let column_name = column_name_i
            .and_then(|i| row.get(i))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let (Some(table_name), Some(column_name)) = (table_name, column_name) else {
            continue;
        };

        let entry = ordered.entry(table_name.clone()).or_insert_with(|| TableMeta {
            name: table_name,
            comment: table_comment_i
                .and_then(|i| row.get(i))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && value != "NULL"),
            columns: Vec::new(),
        });

        if entry.comment.is_none() {
            entry.comment = table_comment_i
                .and_then(|i| row.get(i))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && value != "NULL");
        }

        let data_type = data_type_i
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_else(|| "text".to_string());
        let column_type = column_type_i
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_else(|| data_type.clone());
        let is_nullable = nullable_i
            .and_then(|i| row.get(i))
            .map(|value| {
                let lower = value.trim().to_ascii_lowercase();
                lower == "yes" || lower == "1" || lower == "true"
            })
            .unwrap_or(true);
        let column_key = key_i
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let extra = extra_i
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let column_default = default_i
            .and_then(|i| row.get(i))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty() && value != "NULL");
        let comment = comment_i
            .and_then(|i| row.get(i))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty() && value != "NULL");

        entry.columns.push(ColumnMeta {
            name: column_name,
            data_type,
            column_type,
            is_nullable,
            column_key,
            extra: if mysql_style {
                extra
            } else if extra.to_ascii_lowercase().contains("auto_increment")
                || extra.to_ascii_lowercase().contains("identity")
                || extra.to_ascii_lowercase().contains("nextval")
            {
                "auto_increment".to_string()
            } else {
                extra
            },
            column_default,
            comment,
            max_length: max_len_i.and_then(|i| row.get(i)).and_then(|value| parse_u64(value)),
            numeric_precision: precision_i
                .and_then(|i| row.get(i))
                .and_then(|value| parse_u64(value)),
            numeric_scale: scale_i.and_then(|i| row.get(i)).and_then(|value| parse_u64(value)),
        });
    }

    ordered.into_values().collect()
}

fn select_target_table<'a>(
    tables: &'a [TableMeta],
    table: Option<&str>,
    intent: Option<&str>,
    known_values: &Map<String, Value>,
) -> Result<(&'a TableMeta, String), String> {
    if let Some(name) = table.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(found) = tables
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(name))
        {
            return Ok((found, "explicit_table".to_string()));
        }
        return Err(format!(
            "未找到表 `{name}`。可用表：{}",
            tables
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut scored = tables
        .iter()
        .map(|table| {
            let score = score_table(table, intent, known_values);
            (score, table)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.name.cmp(&right.1.name)));

    if let Some((score, table)) = scored.first() {
        if *score > 0 {
            return Ok((*table, format!("semantic_score={score}")));
        }
    }

    if tables.len() == 1 {
        return Ok((&tables[0], "single_table".to_string()));
    }

    Err(format!(
        "无法自动匹配目标表，请通过 table 参数指定。可用表：{}",
        tables
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn score_table(table: &TableMeta, intent: Option<&str>, known_values: &Map<String, Value>) -> i32 {
    let mut score = 0i32;
    let table_tokens = tokenize(&table.name);
    let comment_tokens = table
        .comment
        .as_deref()
        .map(tokenize)
        .unwrap_or_default();
    let intent_tokens = intent.map(tokenize).unwrap_or_default();
    let known_keys = known_values
        .keys()
        .flat_map(|key| tokenize(key))
        .collect::<Vec<_>>();

    for token in &intent_tokens {
        if table_tokens.iter().any(|item| item.contains(token) || token.contains(item)) {
            score += 8;
        }
        if comment_tokens.iter().any(|item| item.contains(token) || token.contains(item)) {
            score += 6;
        }
    }

    // Contract-oriented hints
    for hint in ["contract", "pact", "采购", "合同", "supplier", "vendor", "订单", "order"] {
        let hint = hint.to_string();
        if table_tokens.iter().any(|item| item.contains(&hint)) {
            score += 4;
        }
        if comment_tokens.iter().any(|item| item.contains(&hint)) {
            score += 3;
        }
        if intent_tokens.iter().any(|item| item.contains(&hint)) {
            score += 2;
        }
    }

    for key in &known_keys {
        if table
            .columns
            .iter()
            .any(|column| column.name.to_ascii_lowercase().contains(key) || key.contains(&column.name.to_ascii_lowercase()))
        {
            score += 5;
        }
        for column in &table.columns {
            if let Some(comment) = &column.comment {
                if comment.to_ascii_lowercase().contains(key) {
                    score += 3;
                }
            }
        }
    }

    // Prefer tables that can accept more known fields.
    let matched_fields = known_values
        .keys()
        .filter(|key| {
            table.columns.iter().any(|column| {
                column.name.eq_ignore_ascii_case(key)
                    || semantic_field_match(&column.name, column.comment.as_deref(), key)
            })
        })
        .count() as i32;
    score += matched_fields * 6;
    score
}

fn column_to_form_field(column: &ColumnMeta, known_values: &Map<String, Value>) -> FormField {
    let input_type = map_input_type(column);
    let auto_increment = column.extra.to_ascii_lowercase().contains("auto_increment");
    let primary_key = column.column_key.to_ascii_uppercase().contains("PRI");
    let required = !column.is_nullable && !auto_increment && column.column_default.is_none();
    let options = parse_enum_options(&column.column_type);
    let label = column
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(column.name.as_str())
        .to_string();

    let value = find_known_value(column, known_values)
        .or_else(|| {
            column
                .column_default
                .as_ref()
                .filter(|value| !value.eq_ignore_ascii_case("null"))
                .map(|value| Value::String(value.trim_matches('\'').to_string()))
        });

    FormField {
        name: column.name.clone(),
        label,
        db_type: column.column_type.clone(),
        input_type,
        required,
        readonly: auto_increment,
        auto_increment,
        primary_key,
        max_length: column.max_length,
        precision: column.numeric_precision,
        scale: column.numeric_scale,
        default_value: column.column_default.clone(),
        comment: column.comment.clone(),
        options,
        value,
        placeholder: column.comment.clone(),
    }
}

fn find_known_value(column: &ColumnMeta, known_values: &Map<String, Value>) -> Option<Value> {
    if let Some(value) = known_values.get(&column.name) {
        return Some(value.clone());
    }
    for (key, value) in known_values {
        if key.eq_ignore_ascii_case(&column.name)
            || semantic_field_match(&column.name, column.comment.as_deref(), key)
        {
            return Some(value.clone());
        }
    }
    None
}

fn semantic_field_match(column_name: &str, comment: Option<&str>, key: &str) -> bool {
    let aliases: HashMap<&str, &[&str]> = HashMap::from([
        ("supplier", &["supplier", "supplier_name", "vendor", "vendor_name", "供应商", "厂商"][..]),
        ("amount", &["amount", "total", "money", "price", "金额", "合同金额", "采购金额"][..]),
        ("contract", &["contract", "contract_no", "pact", "合同", "合同号", "合同编号"][..]),
        ("quantity", &["quantity", "qty", "数量"][..]),
        ("date", &["date", "signed_date", "签约日期", "日期"][..]),
        ("note", &["note", "remark", "memo", "备注", "说明"][..]),
        ("status", &["status", "state", "状态"][..]),
    ]);

    let key_norm = normalize_token(key);
    let col_norm = normalize_token(column_name);
    let comment_norm = comment.map(normalize_token).unwrap_or_default();

    if key_norm == col_norm || col_norm.contains(&key_norm) || key_norm.contains(&col_norm) {
        return true;
    }
    if !comment_norm.is_empty()
        && (comment_norm.contains(&key_norm) || key_norm.contains(&comment_norm))
    {
        return true;
    }

    for (group, words) in aliases {
        let key_in = words.iter().any(|word| key_norm.contains(&normalize_token(word)));
        let col_in = words
            .iter()
            .any(|word| col_norm.contains(&normalize_token(word)) || comment_norm.contains(&normalize_token(word)));
        if key_in && col_in {
            // Prefer group-level matches only when both sides belong to group.
            let _ = group;
            return true;
        }
    }
    false
}

fn map_input_type(column: &ColumnMeta) -> String {
    let data = column.data_type.to_ascii_lowercase();
    let full = column.column_type.to_ascii_lowercase();

    if full.starts_with("enum(") || data == "enum" {
        return "select".to_string();
    }
    if data.contains("bool") || full == "tinyint(1)" || full == "bit" {
        return "boolean".to_string();
    }
    if data.contains("datetime") || data.contains("timestamp") {
        return "datetime".to_string();
    }
    if data == "date" {
        return "date".to_string();
    }
    if data == "time" {
        return "time".to_string();
    }
    if data.contains("json") {
        return "json".to_string();
    }
    if data.contains("text") || data.contains("blob") || data.contains("binary") {
        if data.contains("blob") || data.contains("binary") {
            return "text".to_string();
        }
        return "textarea".to_string();
    }
    if data.contains("int")
        || data.contains("decimal")
        || data.contains("numeric")
        || data.contains("float")
        || data.contains("double")
        || data.contains("real")
        || data.contains("money")
        || data.contains("number")
    {
        return "number".to_string();
    }
    if column.max_length.unwrap_or(0) >= 200 {
        return "textarea".to_string();
    }
    "text".to_string()
}

fn parse_enum_options(column_type: &str) -> Vec<String> {
    let lower = column_type.trim();
    let Some(inner) = lower
        .strip_prefix("enum(")
        .or_else(|| lower.strip_prefix("ENUM("))
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(|part| part.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn value_to_sql_literal(value: &Value, column: &ColumnMeta) -> Result<String, String> {
    if value.is_null() {
        return Ok("NULL".to_string());
    }
    let input_type = map_input_type(column);
    match input_type.as_str() {
        "number" => {
            let text = value_as_string(value);
            if text.eq_ignore_ascii_case("null") || text.is_empty() {
                return Ok("NULL".to_string());
            }
            if text.parse::<f64>().is_err() {
                return Err(format!("字段 `{}` 需要数字，收到：{text}", column.name));
            }
            Ok(text)
        }
        "boolean" => {
            let text = value_as_string(value).to_ascii_lowercase();
            let bool_value = matches!(text.as_str(), "1" | "true" | "yes" | "y" | "on");
            Ok(if bool_value { "1" } else { "0" }.to_string())
        }
        "json" => {
            let text = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            // Validate JSON
            serde_json::from_str::<Value>(&text)
                .map_err(|error| format!("字段 `{}` JSON 无效: {error}", column.name))?;
            Ok(quote_string(&text))
        }
        _ => Ok(quote_string(&value_as_string(value))),
    }
}

fn value_as_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.trim().to_string(),
        other => other.to_string(),
    }
}

fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(v) => v.trim().is_empty(),
        Value::Array(v) => v.is_empty(),
        Value::Object(v) => v.is_empty(),
        _ => false,
    }
}

fn quote_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('\'', "''");
    format!("'{escaped}'")
}

fn quote_ident(value: &str, db_type: &str) -> String {
    match db_type.trim().to_ascii_lowercase().as_str() {
        "mysql" => format!("`{}`", value.replace('`', "``")),
        "postgresql" | "postgres" | "sqlite" => format!("\"{}\"", value.replace('"', "\"\"")),
        "sqlserver" | "mssql" => format!("[{}]", value.replace(']', "]]")),
        _ => format!("\"{}\"", value.replace('"', "\"\"")),
    }
}

fn validate_ident(value: &str, kind: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{kind} 不能为空"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
    {
        return Err(format!("{kind} `{value}` 包含非法字符"));
    }
    Ok(())
}

fn display_name(source: &SmartbrainDbSource) -> String {
    let trimmed_name = source.name.trim();
    if !trimmed_name.is_empty() {
        return trimmed_name.to_string();
    }
    let trimmed_db_name = source.database_name.trim();
    if !trimmed_db_name.is_empty() {
        return trimmed_db_name.to_string();
    }
    "未命名数据库".to_string()
}

fn nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .map(|part| normalize_token(part))
        .filter(|part| !part.is_empty())
        .collect()
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, data_type: &str, column_type: &str, nullable: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            data_type: data_type.to_string(),
            column_type: column_type.to_string(),
            is_nullable: nullable,
            column_key: String::new(),
            extra: String::new(),
            column_default: None,
            comment: None,
            max_length: None,
            numeric_precision: None,
            numeric_scale: None,
        }
    }

    #[test]
    fn maps_common_sql_types_to_inputs() {
        assert_eq!(map_input_type(&col("id", "bigint", "bigint", false)), "number");
        assert_eq!(map_input_type(&col("amount", "decimal", "decimal(12,2)", false)), "number");
        assert_eq!(map_input_type(&col("name", "varchar", "varchar(100)", false)), "text");
        assert_eq!(map_input_type(&col("note", "text", "text", true)), "textarea");
        assert_eq!(map_input_type(&col("paid", "tinyint", "tinyint(1)", true)), "boolean");
        assert_eq!(map_input_type(&col("day", "date", "date", true)), "date");
        assert_eq!(map_input_type(&col("ts", "datetime", "datetime", true)), "datetime");
        assert_eq!(map_input_type(&col("meta", "json", "json", true)), "json");
        assert_eq!(
            map_input_type(&col(
                "status",
                "enum",
                "enum('draft','signed','closed')",
                true
            )),
            "select"
        );
    }

    #[test]
    fn parse_enum_options_works() {
        let options = parse_enum_options("enum('draft','signed','closed')");
        assert_eq!(options, vec!["draft", "signed", "closed"]);
    }

    #[test]
    fn semantic_match_supplier_and_amount() {
        assert!(semantic_field_match("supplier_name", Some("供应商"), "supplier"));
        assert!(semantic_field_match("amount", Some("合同金额"), "amount"));
        assert!(semantic_field_match("contract_no", Some("合同编号"), "contract"));
    }

    #[test]
    fn value_to_sql_literal_quotes_and_numbers() {
        let amount = col("amount", "decimal", "decimal(12,2)", false);
        assert_eq!(
            value_to_sql_literal(&Value::from(12000), &amount).unwrap(),
            "12000"
        );
        let name = col("supplier_name", "varchar", "varchar(100)", false);
        assert_eq!(
            value_to_sql_literal(&Value::String("华为".into()), &name).unwrap(),
            "'华为'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::String("O'Reilly".into()), &name).unwrap(),
            "'O''Reilly'"
        );
    }

    #[test]
    fn score_prefers_contract_like_table() {
        let contract = TableMeta {
            name: "purchase_contract".into(),
            comment: Some("采购合同".into()),
            columns: vec![
                col("supplier_name", "varchar", "varchar(100)", false),
                col("amount", "decimal", "decimal(12,2)", false),
            ],
        };
        let other = TableMeta {
            name: "users".into(),
            comment: None,
            columns: vec![col("username", "varchar", "varchar(50)", false)],
        };
        let mut known = Map::new();
        known.insert("supplier".into(), Value::String("华为".into()));
        known.insert("amount".into(), Value::from(12000));
        let score_contract = score_table(&contract, Some("采购合同"), &known);
        let score_other = score_table(&other, Some("采购合同"), &known);
        assert!(score_contract > score_other);
    }

    #[tokio::test]
    async fn sqlite_build_and_save_entry_form_roundtrip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workspace_config_dir = temp_dir.path().join("codey");
        std::fs::create_dir_all(&workspace_config_dir).expect("create workspace config dir");

        let sqlite_path = temp_dir.path().join("entry_form_type_test.db");
        {
            let conn = rusqlite::Connection::open(&sqlite_path).expect("open sqlite");
            conn.execute_batch(
                r#"
                CREATE TABLE sb_entry_form_type_test (
                  id INTEGER PRIMARY KEY AUTOINCREMENT,
                  supplier_name TEXT NOT NULL,
                  contract_no TEXT,
                  amount REAL NOT NULL,
                  quantity INTEGER DEFAULT 1,
                  unit_price REAL,
                  is_paid INTEGER NOT NULL DEFAULT 0,
                  status TEXT NOT NULL DEFAULT 'draft',
                  signed_date TEXT,
                  remark TEXT,
                  meta_json TEXT
                );
                CREATE TABLE users (
                  id INTEGER PRIMARY KEY AUTOINCREMENT,
                  username TEXT NOT NULL
                );
                "#,
            )
            .expect("create tables");
        }

        let usage_db =
            crate::usage::UsageDb::open(&workspace_config_dir.join("usage.db")).expect("open usage");
        let source = serde_json::json!([{
            "id": "sqlite-entry-form-test",
            "name": "本地入库测试库",
            "dbType": "sqlite",
            "connectionUri": "",
            "password": "",
            "enabled": true,
            "host": "",
            "port": null,
            "databaseName": "",
            "username": "",
            "filePath": sqlite_path.to_string_lossy(),
            "schema": "",
            "permissions": {
                "readSchema": true,
                "readData": true,
                "writeData": true
            },
            "updatedAt": 1
        }]);
        usage_db
            .state_set(
                super::super::db_query::SMARTBRAIN_DB_SOURCES_STATE_KEY,
                &source.to_string(),
            )
            .expect("save db source");
        usage_db
            .state_set(
                super::super::db_query::SMARTBRAIN_DB_SETTINGS_STATE_KEY,
                &serde_json::json!({
                    "defaultRowLimit": 200,
                    "defaultTimeoutSec": 15,
                    "requireReadonlyReminder": false,
                    "skipWhenNoPermission": true,
                    "denyDdl": true,
                    "denyDrop": true,
                    "denyDeleteWithoutWritePermission": true,
                    "rulesMarkdown": ""
                })
                .to_string(),
            )
            .expect("save db settings");

        let mut known = Map::new();
        known.insert("supplier".into(), Value::String("华为".into()));
        known.insert("amount".into(), Value::from(12000));
        known.insert(
            "remark".into(),
            Value::String("今天和华为签了12000元采购合同".into()),
        );

        let form = build_entry_form(
            &workspace_config_dir,
            Some("本地入库测试库"),
            known.clone(),
            None,
            Some("采购合同入库"),
        )
        .await
        .expect("build form");

        assert_eq!(form.table, "sb_entry_form_type_test");
        assert!(
            form.fields.iter().any(|field| field.name == "supplier_name"),
            "expected supplier_name field"
        );

        let supplier_field = form
            .fields
            .iter()
            .find(|field| field.name == "supplier_name")
            .expect("supplier field");
        assert_eq!(
            supplier_field.value.as_ref().and_then(|value| value.as_str()),
            Some("华为")
        );

        let amount_field = form
            .fields
            .iter()
            .find(|field| field.name == "amount")
            .expect("amount field");
        assert_eq!(amount_field.value.as_ref().and_then(|value| value.as_f64()), Some(12000.0));

        let mut values = Map::new();
        values.insert("supplier_name".into(), Value::String("华为".into()));
        values.insert("amount".into(), Value::from(12000));
        values.insert("contract_no".into(), Value::String("HT-2026-001".into()));
        values.insert("status".into(), Value::String("signed".into()));
        values.insert(
            "remark".into(),
            Value::String("今天和华为签了12000元采购合同".into()),
        );

        let saved = save_form_data(
            &workspace_config_dir,
            Some("本地入库测试库"),
            &form.table,
            values,
        )
        .await
        .expect("save form");
        assert!(saved.ok, "save failed: {:?}", saved.error);
        assert!(saved.inserted);

        let verify = execute_smartbrain_sql_query(
            &workspace_config_dir,
            Some("本地入库测试库"),
            "SELECT supplier_name, amount, contract_no, status FROM sb_entry_form_type_test",
            Some(10),
            Some(10),
        )
        .await
        .expect("verify select");
        assert_eq!(verify.row_count, 1);
        assert_eq!(verify.rows[0][0], "华为");
        assert!(verify.rows[0][1].starts_with("12000"));
        assert_eq!(verify.rows[0][2], "HT-2026-001");
        assert_eq!(verify.rows[0][3], "signed");
    }
}
