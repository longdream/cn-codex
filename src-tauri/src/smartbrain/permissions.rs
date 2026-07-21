//! Flexible Local Knowledge Base database permission policy.
//!
//! Structured policy (defaults / table overrides / rules / aiNotes) is:
//! - injected into AI prompts
//! - enforced at SQL execution time
//!
//! Legacy library-level three-boolean configs are migrated once on load.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbPermissionDefaults {
    #[serde(default = "default_true")]
    pub read_schema: bool,
    #[serde(default = "default_true")]
    pub read_data: bool,
    #[serde(default)]
    pub allow_insert: bool,
    #[serde(default)]
    pub allow_update: bool,
    #[serde(default)]
    pub allow_delete: bool,
    #[serde(default)]
    pub allow_ddl: bool,
    /// Only used when loading partially-migrated JSON; never written back.
    #[serde(default, skip_serializing)]
    pub write_data: Option<bool>,
}

impl Default for DbPermissionDefaults {
    fn default() -> Self {
        Self {
            read_schema: true,
            read_data: true,
            allow_insert: false,
            allow_update: false,
            allow_delete: false,
            allow_ddl: false,
            write_data: None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnPermission {
    #[serde(default = "default_true")]
    pub read: bool,
    #[serde(default = "default_true")]
    pub write: bool,
}

impl Default for ColumnPermission {
    fn default() -> Self {
        Self {
            read: true,
            write: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TablePermission {
    #[serde(default = "default_true")]
    pub read: bool,
    #[serde(default)]
    pub insert: bool,
    #[serde(default)]
    pub update: bool,
    #[serde(default)]
    pub delete: bool,
    #[serde(default)]
    pub columns: BTreeMap<String, ColumnPermission>,
}

impl Default for TablePermission {
    fn default() -> Self {
        Self {
            read: true,
            insert: false,
            update: false,
            delete: false,
            columns: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRuleMatch {
    #[serde(default)]
    pub sql_kinds: Vec<String>,
    #[serde(default)]
    pub tables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    #[serde(default)]
    pub id: String,
    /// "allow" | "deny"
    #[serde(default = "default_deny")]
    pub effect: String,
    #[serde(default, rename = "match")]
    pub match_spec: PermissionRuleMatch,
    #[serde(default)]
    pub message: String,
}

fn default_deny() -> String {
    "deny".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbPermissionPolicy {
    #[serde(default = "default_policy_version")]
    pub version: u32,
    #[serde(default)]
    pub defaults: DbPermissionDefaults,
    #[serde(default)]
    pub tables: BTreeMap<String, TablePermission>,
    #[serde(default)]
    pub rules: Vec<PermissionRule>,
    #[serde(default)]
    pub ai_notes: String,
}

fn default_policy_version() -> u32 {
    2
}

impl Default for DbPermissionPolicy {
    fn default() -> Self {
        Self {
            version: 2,
            defaults: DbPermissionDefaults::default(),
            tables: BTreeMap::new(),
            rules: Vec::new(),
            ai_notes: String::new(),
        }
    }
}

/// Legacy three-boolean permissions (pre-v2).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDbPermissions {
    #[serde(default)]
    pub read_schema: bool,
    #[serde(default)]
    pub read_data: bool,
    #[serde(default)]
    pub write_data: bool,
}

impl DbPermissionPolicy {
    pub fn from_legacy(legacy: &LegacyDbPermissions) -> Self {
        let write = legacy.write_data;
        Self {
            version: 2,
            defaults: DbPermissionDefaults {
                read_schema: legacy.read_schema,
                read_data: legacy.read_data,
                allow_insert: write,
                allow_update: write,
                allow_delete: write,
                allow_ddl: false,
                write_data: None,
            },
            tables: BTreeMap::new(),
            rules: Vec::new(),
            ai_notes: String::new(),
        }
    }

    pub fn normalize(mut self) -> Self {
        if let Some(write) = self.defaults.write_data.take() {
            if write {
                self.defaults.allow_insert = true;
                self.defaults.allow_update = true;
                self.defaults.allow_delete = true;
            }
        }
        if self.version == 0 {
            self.version = 2;
        }
        self
    }

    pub fn has_any_capability(&self) -> bool {
        if self.defaults.read_schema
            || self.defaults.read_data
            || self.defaults.allow_insert
            || self.defaults.allow_update
            || self.defaults.allow_delete
            || self.defaults.allow_ddl
        {
            return true;
        }
        self.tables
            .values()
            .any(|table| table.read || table.insert || table.update || table.delete)
    }

    pub fn summary_for_prompt(&self) -> String {
        let mut clean = Vec::new();
        if self.defaults.read_schema {
            clean.push("readSchema".to_string());
        }
        if self.defaults.read_data {
            clean.push("readData".to_string());
        }
        let mut writes = Vec::new();
        if self.defaults.allow_insert {
            writes.push("insert");
        }
        if self.defaults.allow_update {
            writes.push("update");
        }
        if self.defaults.allow_delete {
            writes.push("delete");
        }
        if self.defaults.allow_ddl {
            writes.push("ddl");
        }
        if !writes.is_empty() {
            clean.push(format!("write({})", writes.join("/")));
        }

        let mut summary = if clean.is_empty() {
            "none".to_string()
        } else {
            clean.join(",")
        };

        if !self.tables.is_empty() {
            let mut table_bits = Vec::new();
            for (name, table) in self.tables.iter().take(8) {
                let mut ops = Vec::new();
                if table.read {
                    ops.push("r");
                }
                if table.insert {
                    ops.push("i");
                }
                if table.update {
                    ops.push("u");
                }
                if table.delete {
                    ops.push("d");
                }
                if ops.is_empty() {
                    ops.push("-");
                }
                table_bits.push(format!("{name}:{}", ops.join("")));
            }
            if !table_bits.is_empty() {
                summary.push_str("; tables ");
                summary.push_str(&table_bits.join(", "));
                if self.tables.len() > 8 {
                    summary.push_str(", …");
                }
            }
        }

        let deny_kinds: Vec<String> = self
            .rules
            .iter()
            .filter(|rule| rule.effect.eq_ignore_ascii_case("deny"))
            .flat_map(|rule| rule.match_spec.sql_kinds.iter().cloned())
            .take(6)
            .collect();
        if !deny_kinds.is_empty() {
            summary.push_str("; deny ");
            summary.push_str(&deny_kinds.join("/"));
        }

        summary
    }
}

/// Deserialize either the new policy object or the legacy three-boolean object.
pub fn deserialize_permissions_value(value: &serde_json::Value) -> DbPermissionPolicy {
    if value.is_null() {
        return DbPermissionPolicy::default();
    }
    if value.get("version").is_some()
        || value.get("defaults").is_some()
        || value.get("tables").is_some()
        || value.get("rules").is_some()
        || value.get("aiNotes").is_some()
        || value.get("ai_notes").is_some()
    {
        return serde_json::from_value::<DbPermissionPolicy>(value.clone())
            .unwrap_or_default()
            .normalize();
    }
    if value.get("readSchema").is_some()
        || value.get("readData").is_some()
        || value.get("writeData").is_some()
        || value.get("read_schema").is_some()
    {
        let legacy: LegacyDbPermissions =
            serde_json::from_value(value.clone()).unwrap_or_default();
        return DbPermissionPolicy::from_legacy(&legacy);
    }
    serde_json::from_value::<DbPermissionPolicy>(value.clone())
        .unwrap_or_default()
        .normalize()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlOpKind {
    ReadSchema,
    ReadData,
    Insert,
    Update,
    Delete,
    Ddl,
    Unknown,
}

impl SqlOpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SqlOpKind::ReadSchema => "readSchema",
            SqlOpKind::ReadData => "readData",
            SqlOpKind::Insert => "insert",
            SqlOpKind::Update => "update",
            SqlOpKind::Delete => "delete",
            SqlOpKind::Ddl => "ddl",
            SqlOpKind::Unknown => "unknown",
        }
    }

    pub fn rule_aliases(self) -> &'static [&'static str] {
        match self {
            SqlOpKind::ReadSchema => &["readschema", "schema", "read"],
            SqlOpKind::ReadData => &["readdata", "select", "read"],
            SqlOpKind::Insert => &["insert", "write"],
            SqlOpKind::Update => &["update", "write"],
            SqlOpKind::Delete => &["delete", "write"],
            SqlOpKind::Ddl => &["ddl", "drop", "alter", "create", "truncate"],
            SqlOpKind::Unknown => &["unknown"],
        }
    }
}

#[derive(Debug, Clone)]
pub struct PermissionDenial {
    pub code: String,
    pub message: String,
}

impl PermissionDenial {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn format_error(&self) -> String {
        format!("PERMISSION_DENIED:{}: {}", self.code, self.message)
    }
}

fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            out.push(b as char);
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            out.push(b as char);
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if b == b'\'' {
            in_single = true;
            out.push('\'');
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double = true;
            out.push('"');
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
            continue;
        }
        out.push(b as char);
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

pub fn classify_sql_op(sql: &str) -> SqlOpKind {
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
                SqlOpKind::ReadSchema
            } else {
                SqlOpKind::ReadData
            }
        }
        "insert" | "replace" => SqlOpKind::Insert,
        "update" | "merge" => SqlOpKind::Update,
        "delete" => SqlOpKind::Delete,
        "call" | "execute" | "exec" => SqlOpKind::Update,
        "drop" | "truncate" | "alter" | "create" | "grant" | "revoke" | "rename" => SqlOpKind::Ddl,
        _ => SqlOpKind::Unknown,
    }
}

fn normalize_ident(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '`' || c == '"' || c == '[' || c == ']')
        .split('.')
        .last()
        .unwrap_or_default()
        .trim()
        .trim_matches(|c| c == '`' || c == '"' || c == '[' || c == ']')
        .to_string()
}

/// Best-effort extraction of referenced table names for permission checks.
pub fn extract_table_names(sql: &str) -> Vec<String> {
    let cleaned = strip_sql_comments(sql);
    let lower = cleaned.to_ascii_lowercase();
    let original_chars: Vec<char> = cleaned.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    let mut tables = Vec::new();
    let keywords = ["from", "join", "into", "update", "table", "delete from"];

    let mut i = 0usize;
    while i < lower_chars.len() {
        let mut matched: Option<&str> = None;
        for key in keywords {
            let key_chars: Vec<char> = key.chars().collect();
            if i + key_chars.len() <= lower_chars.len()
                && lower_chars[i..i + key_chars.len()] == key_chars[..]
            {
                let before_ok = i == 0
                    || (!lower_chars[i - 1].is_ascii_alphanumeric() && lower_chars[i - 1] != '_');
                let after_idx = i + key_chars.len();
                let after_ok = after_idx >= lower_chars.len()
                    || (!lower_chars[after_idx].is_ascii_alphanumeric()
                        && lower_chars[after_idx] != '_');
                if before_ok && after_ok {
                    matched = Some(key);
                    break;
                }
            }
        }
        let Some(key) = matched else {
            i += 1;
            continue;
        };
        let mut j = i + key.chars().count();
        while j < lower_chars.len() && lower_chars[j].is_whitespace() {
            j += 1;
        }
        if j >= original_chars.len() {
            break;
        }
        let mut ident = String::new();
        if matches!(original_chars[j], '`' | '"' | '[') {
            let closer = match original_chars[j] {
                '`' => '`',
                '"' => '"',
                _ => ']',
            };
            j += 1;
            while j < original_chars.len() && original_chars[j] != closer {
                ident.push(original_chars[j]);
                j += 1;
            }
        } else {
            while j < original_chars.len() {
                let ch = original_chars[j];
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '$' {
                    ident.push(ch);
                    j += 1;
                } else {
                    break;
                }
            }
        }
        let name = normalize_ident(&ident);
        if !name.is_empty()
            && !matches!(
                name.to_ascii_lowercase().as_str(),
                "select" | "where" | "set" | "values" | "as" | "on" | "only"
            )
            && !tables
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&name))
        {
            tables.push(name);
        }
        i = j;
    }
    tables
}

fn table_permission<'a>(
    policy: &'a DbPermissionPolicy,
    table: &str,
) -> Option<&'a TablePermission> {
    policy
        .tables
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(table))
        .map(|(_, perm)| perm)
}

fn defaults_allow(policy: &DbPermissionPolicy, op: SqlOpKind) -> bool {
    match op {
        SqlOpKind::ReadSchema => policy.defaults.read_schema || policy.defaults.read_data,
        SqlOpKind::ReadData => policy.defaults.read_data,
        SqlOpKind::Insert => policy.defaults.allow_insert,
        SqlOpKind::Update => policy.defaults.allow_update,
        SqlOpKind::Delete => policy.defaults.allow_delete,
        SqlOpKind::Ddl => policy.defaults.allow_ddl,
        SqlOpKind::Unknown => false,
    }
}

fn table_allows(table: &TablePermission, op: SqlOpKind) -> bool {
    match op {
        SqlOpKind::ReadSchema | SqlOpKind::ReadData => table.read,
        SqlOpKind::Insert => table.insert,
        SqlOpKind::Update => table.update,
        SqlOpKind::Delete => table.delete,
        SqlOpKind::Ddl => false,
        SqlOpKind::Unknown => false,
    }
}

fn rule_matches(rule: &PermissionRule, op: SqlOpKind, tables: &[String]) -> bool {
    let matcher = &rule.match_spec;
    let kind_ok = if matcher.sql_kinds.is_empty() {
        true
    } else {
        matcher.sql_kinds.iter().any(|kind| {
            let kind_l = kind.trim().to_ascii_lowercase();
            op.rule_aliases()
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&kind_l))
                || (kind_l == "write"
                    && matches!(
                        op,
                        SqlOpKind::Insert | SqlOpKind::Update | SqlOpKind::Delete
                    ))
        })
    };
    if !kind_ok {
        return false;
    }
    if matcher.tables.is_empty() {
        return true;
    }
    if tables.is_empty() {
        return false;
    }
    matcher.tables.iter().any(|rule_table| {
        tables
            .iter()
            .any(|t| t.eq_ignore_ascii_case(rule_table.trim()))
    })
}

pub struct GlobalSqlGuards {
    pub deny_ddl: bool,
    pub deny_drop: bool,
    pub deny_delete_without_write_permission: bool,
}

/// Validate SQL against global settings + structured permission policy.
pub fn validate_sql_policy(
    sql: &str,
    policy: &DbPermissionPolicy,
    database_label: &str,
    guards: &GlobalSqlGuards,
) -> Result<SqlOpKind, PermissionDenial> {
    let op = classify_sql_op(sql);
    if op == SqlOpKind::Unknown {
        return Err(PermissionDenial::new(
            "SQL_UNKNOWN",
            "无法识别 SQL 类型。仅支持明确的 SELECT/SHOW/DESCRIBE/INSERT/UPDATE/DELETE 等语句。",
        ));
    }

    if op == SqlOpKind::Ddl && (guards.deny_ddl || guards.deny_drop) {
        return Err(PermissionDenial::new(
            "GLOBAL_DENY_DDL",
            format!("数据库 `{database_label}` 的全局策略禁止 DDL/高危语句。"),
        ));
    }

    let tables = extract_table_names(sql);

    for rule in &policy.rules {
        if !rule.effect.eq_ignore_ascii_case("deny") {
            continue;
        }
        if rule_matches(rule, op, &tables) {
            let msg = if rule.message.trim().is_empty() {
                format!(
                    "数据库 `{database_label}` 规则 `{}` 拒绝 {} 操作。",
                    if rule.id.is_empty() {
                        "deny"
                    } else {
                        &rule.id
                    },
                    op.as_str()
                )
            } else {
                rule.message.clone()
            };
            return Err(PermissionDenial::new("RULE_DENY", msg));
        }
    }

    let mut allowed = if tables.is_empty() {
        defaults_allow(policy, op)
    } else {
        tables.iter().all(|table| {
            if let Some(tp) = table_permission(policy, table) {
                table_allows(tp, op)
            } else {
                defaults_allow(policy, op)
            }
        })
    };

    if !allowed {
        for rule in &policy.rules {
            if rule.effect.eq_ignore_ascii_case("allow") && rule_matches(rule, op, &tables) {
                allowed = true;
                break;
            }
        }
    }

    if !allowed {
        let code = match op {
            SqlOpKind::ReadSchema => "DENY_READ_SCHEMA",
            SqlOpKind::ReadData => "DENY_READ_DATA",
            SqlOpKind::Insert => "DENY_INSERT",
            SqlOpKind::Update => "DENY_UPDATE",
            SqlOpKind::Delete => "DENY_DELETE",
            SqlOpKind::Ddl => "DENY_DDL",
            SqlOpKind::Unknown => "DENY_UNKNOWN",
        };
        let table_hint = if tables.is_empty() {
            String::new()
        } else {
            format!("（表: {}）", tables.join(", "))
        };
        return Err(PermissionDenial::new(
            code,
            format!(
                "数据库 `{database_label}` 权限策略拒绝 {} 操作{table_hint}。",
                op.as_str()
            ),
        ));
    }

    let _ = guards.deny_delete_without_write_permission;
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_read_only() -> DbPermissionPolicy {
        DbPermissionPolicy {
            defaults: DbPermissionDefaults {
                read_schema: true,
                read_data: true,
                allow_insert: false,
                allow_update: false,
                allow_delete: false,
                allow_ddl: false,
                write_data: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn migrate_legacy_write_data() {
        let legacy = LegacyDbPermissions {
            read_schema: true,
            read_data: true,
            write_data: true,
        };
        let policy = DbPermissionPolicy::from_legacy(&legacy);
        assert!(policy.defaults.allow_insert);
        assert!(policy.defaults.allow_update);
        assert!(policy.defaults.allow_delete);
        assert!(!policy.defaults.allow_ddl);
    }

    #[test]
    fn deny_insert_by_default() {
        let policy = policy_read_only();
        let guards = GlobalSqlGuards {
            deny_ddl: true,
            deny_drop: true,
            deny_delete_without_write_permission: true,
        };
        let err = validate_sql_policy(
            "INSERT INTO contract(name) VALUES('a')",
            &policy,
            "合同库",
            &guards,
        )
        .unwrap_err();
        assert_eq!(err.code, "DENY_INSERT");
    }

    #[test]
    fn table_level_insert_allow() {
        let mut policy = policy_read_only();
        policy.tables.insert(
            "contract".to_string(),
            TablePermission {
                read: true,
                insert: true,
                update: false,
                delete: false,
                columns: BTreeMap::new(),
            },
        );
        let guards = GlobalSqlGuards {
            deny_ddl: true,
            deny_drop: true,
            deny_delete_without_write_permission: true,
        };
        let ok = validate_sql_policy(
            "INSERT INTO contract(name) VALUES('a')",
            &policy,
            "合同库",
            &guards,
        )
        .unwrap();
        assert_eq!(ok, SqlOpKind::Insert);

        let err = validate_sql_policy(
            "DELETE FROM contract WHERE id=1",
            &policy,
            "合同库",
            &guards,
        )
        .unwrap_err();
        assert_eq!(err.code, "DENY_DELETE");
    }

    #[test]
    fn rule_deny_ddl() {
        let mut policy = policy_read_only();
        policy.defaults.allow_ddl = true;
        policy.rules.push(PermissionRule {
            id: "no-drop".into(),
            effect: "deny".into(),
            match_spec: PermissionRuleMatch {
                sql_kinds: vec!["ddl".into(), "drop".into()],
                tables: vec![],
            },
            message: "禁止 DDL/删库删表".into(),
        });
        let guards = GlobalSqlGuards {
            deny_ddl: false,
            deny_drop: false,
            deny_delete_without_write_permission: true,
        };
        let err =
            validate_sql_policy("DROP TABLE contract", &policy, "合同库", &guards).unwrap_err();
        assert_eq!(err.code, "RULE_DENY");
        assert!(err.message.contains("禁止"));
    }

    #[test]
    fn extract_tables_from_join() {
        let tables = extract_table_names(
            "SELECT a.id FROM contract a JOIN party b ON a.party_id=b.id WHERE a.id=1",
        );
        assert!(tables.iter().any(|t| t.eq_ignore_ascii_case("contract")));
        assert!(tables.iter().any(|t| t.eq_ignore_ascii_case("party")));
    }

    #[test]
    fn deserialize_legacy_json() {
        let value = serde_json::json!({
            "readSchema": true,
            "readData": true,
            "writeData": false
        });
        let policy = deserialize_permissions_value(&value);
        assert!(policy.defaults.read_schema);
        assert!(policy.defaults.read_data);
        assert!(!policy.defaults.allow_insert);
    }
}
