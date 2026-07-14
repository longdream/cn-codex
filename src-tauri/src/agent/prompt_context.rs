use std::path::Path;

use serde::Deserialize;

use crate::config_system::SmartBrainConfig;

use super::truncate_utf8_by_bytes;

pub(super) const SMARTBRAIN_DB_SOURCES_STATE_KEY: &str = "smartbrain.db.sources";
pub(super) const SMARTBRAIN_DB_SETTINGS_STATE_KEY: &str = "smartbrain.db.settings";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SmartbrainPromptDbPermissions {
    #[serde(default)]
    read_schema: bool,
    #[serde(default)]
    read_data: bool,
    #[serde(default)]
    write_data: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SmartbrainPromptDbSource {
    #[serde(default)]
    name: String,
    #[serde(default)]
    db_type: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    database_name: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    permissions: SmartbrainPromptDbPermissions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmartbrainPromptDbSettings {
    #[serde(default = "default_smartbrain_db_row_limit")]
    default_row_limit: usize,
    #[serde(default = "default_smartbrain_db_timeout")]
    default_timeout_sec: usize,
    #[serde(default = "default_true")]
    require_readonly_reminder: bool,
    #[serde(default = "default_true")]
    skip_when_no_permission: bool,
    #[serde(default)]
    rules_markdown: String,
}

impl Default for SmartbrainPromptDbSettings {
    fn default() -> Self {
        Self {
            default_row_limit: default_smartbrain_db_row_limit(),
            default_timeout_sec: default_smartbrain_db_timeout(),
            require_readonly_reminder: true,
            skip_when_no_permission: true,
            rules_markdown: String::new(),
        }
    }
}

fn default_smartbrain_db_row_limit() -> usize {
    200
}

fn default_smartbrain_db_timeout() -> usize {
    15
}

fn default_true() -> bool {
    true
}

fn smartbrain_source_has_any_permission(source: &SmartbrainPromptDbSource) -> bool {
    source.permissions.read_schema || source.permissions.read_data || source.permissions.write_data
}

fn smartbrain_source_is_effectively_enabled(
    source: &SmartbrainPromptDbSource,
    settings: &SmartbrainPromptDbSettings,
) -> bool {
    if !source.enabled {
        return false;
    }
    if settings.skip_when_no_permission && !smartbrain_source_has_any_permission(source) {
        return false;
    }
    true
}

fn load_workspace_state_value(workspace_config_dir: &Path, key: &str) -> Option<String> {
    let usage_db_path = workspace_config_dir.join("usage.db");
    if !usage_db_path.exists() {
        return None;
    }
    let usage_db = crate::usage::UsageDb::open(&usage_db_path).ok()?;
    usage_db.state_get(key).ok().flatten()
}

fn smartbrain_source_display_name(source: &SmartbrainPromptDbSource) -> String {
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
    let trimmed_host = source.host.trim();
    if !trimmed_host.is_empty() {
        return trimmed_host.to_string();
    }
    "未命名数据库".to_string()
}

fn smartbrain_source_target(source: &SmartbrainPromptDbSource) -> String {
    if source.db_type.trim().eq_ignore_ascii_case("sqlite") {
        return if source.file_path.trim().is_empty() {
            "SQLite 文件路径未填写".to_string()
        } else {
            source.file_path.trim().to_string()
        };
    }

    let mut target = source.host.trim().to_string();
    if let Some(port) = source.port {
        if !target.is_empty() {
            target.push(':');
            target.push_str(&port.to_string());
        }
    }
    if !source.database_name.trim().is_empty() {
        if !target.is_empty() {
            target.push('/');
        }
        target.push_str(source.database_name.trim());
    }
    if target.is_empty() {
        "连接目标未填写".to_string()
    } else {
        target
    }
}

fn smartbrain_permission_labels(permissions: &SmartbrainPromptDbPermissions) -> String {
    let mut labels = Vec::new();
    if permissions.read_schema {
        labels.push("readSchema");
    }
    if permissions.read_data {
        labels.push("readData");
    }
    if permissions.write_data {
        labels.push("writeData");
    }
    if labels.is_empty() {
        "none".to_string()
    } else {
        labels.join(", ")
    }
}

fn render_smartbrain_database_prompt_for_config_dir(workspace_config_dir: &Path) -> String {
    let raw_sources =
        load_workspace_state_value(workspace_config_dir, SMARTBRAIN_DB_SOURCES_STATE_KEY);
    let raw_settings =
        load_workspace_state_value(workspace_config_dir, SMARTBRAIN_DB_SETTINGS_STATE_KEY);

    let sources: Vec<SmartbrainPromptDbSource> = raw_sources
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();
    if sources.is_empty() {
        return String::new();
    }

    let settings: SmartbrainPromptDbSettings = raw_settings
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();

    let mut active_sources = Vec::new();
    let mut inactive_sources = Vec::new();
    for source in &sources {
        let display_name = smartbrain_source_display_name(source);
        let target = smartbrain_source_target(source);
        let username = if source.username.trim().is_empty() {
            "未设置".to_string()
        } else {
            source.username.trim().to_string()
        };
        let permissions = smartbrain_permission_labels(&source.permissions);
        let alias = if !source.database_name.trim().is_empty()
            && source.database_name.trim() != display_name
        {
            format!("；物理库名=`{}`", source.database_name.trim())
        } else {
            String::new()
        };
        let line = format!(
            "- `{display_name}`: 类型=`{db_type}`；目标=`{target}`{alias}；用户=`{username}`；权限=`{permissions}`",
            db_type = source.db_type.trim(),
        );
        if smartbrain_source_is_effectively_enabled(source, &settings) {
            active_sources.push(line);
        } else {
            inactive_sources.push(line);
        }
    }

    let mut sections = Vec::new();
    sections.push(
        "你已经有一组通过智脑配置好的数据库连接。它们属于“智脑”上下文的一部分，不要把它们当作缺失信息。"
            .to_string(),
    );
    if !active_sources.is_empty() {
        sections.push(format!(
            "### 已配置且当前可用的数据库\n{}",
            active_sources.join("\n")
        ));
    }
    if !inactive_sources.is_empty() {
        sections.push(format!(
            "### 已配置但当前应跳过的数据库\n{}",
            inactive_sources.join("\n")
        ));
    }
    sections.push(
        "名称映射规则：如果用户提到数据库的显示名称、物理库名，或它们的明显别名，应自动匹配到对应配置。例：提到“合同数据库”或“psa_crm_pact_test”时，都应视为同一个已配置数据库。"
            .to_string(),
    );
    sections.push(
        "除非配置缺失关键字段，或实际连接/查询动作已经失败，否则不要再次向用户索要主机、端口、用户名、密码或完整连接串。"
            .to_string(),
    );
    sections.push(
        "数据库查询必须使用内置工具 `smartbrain_sql_query`（参数：database, sql, 可选 row_limit/timeout_sec）。\
         不要用 Python/shell 手写连接脚本执行 SQL，也不要让用户再次提供密码；密码已保存在智脑数据库配置中。"
            .to_string(),
    );
    sections.push(
        "如果用户要求查库、看表、统计行数或验证数据，应直接调用 `smartbrain_sql_query` 并输出查询结果；\
         不要改写为编写 Python 脚本、安装驱动或索要连接密码。"
            .to_string(),
    );
    sections.push(format!(
        "默认查询限制：单次最多 {} 行，超时 {} 秒。",
        settings.default_row_limit, settings.default_timeout_sec
    ));
    if settings.require_readonly_reminder {
        sections.push(
            "连接数据库时优先使用只读账号；密码已在配置中单独保存，但不会在此提示词中回显。"
                .to_string(),
        );
    }
    if !settings.rules_markdown.trim().is_empty() {
        sections.push(format!(
            "数据库规则：\n{}",
            truncate_utf8_by_bytes(settings.rules_markdown.trim(), 2500)
        ));
    }

    sections.join("\n\n")
}

pub(crate) fn render_smartbrain_runtime_prompt(
    workspace_config_dir: &Path,
    smartbrain_config: &SmartBrainConfig,
) -> String {
    let mut parts = Vec::new();

    if smartbrain_config.inject_summary {
        if let Some(summary) = crate::smartbrain::load_summary(workspace_config_dir) {
            parts.push(format!(
                "You have accumulated experience from previous sessions. Here is a summary:\n\n\
                 {summary}\n\n\
                 For detailed experience notes, use `memory_read` to read `experiences/experience_handbook.md`."
            ));
        }

        if let Some(hierarchy) = crate::smartbrain::load_hierarchy(workspace_config_dir) {
            let hier_text = hierarchy.summary_text();
            if !hier_text.is_empty() {
                parts.push(format!(
                    "You also have access to a knowledge base with these categories:\n{hier_text}\n\n\
                     Use `smartbrain_search` to find relevant knowledge first, then read with continuity: \
                     always include previous/next sections around chunk hits and keep at least 30 lines overlap \
                     to avoid cut-off context. If available, follow the `smartbrain-context-read` skill."
                ));
            }
        }
    }

    if smartbrain_config.is_active() {
        parts.push(
            "Knowledge retrieval policy: prefer `smartbrain_search` for large or structured knowledge queries. \
             Use `memory_read` pagination (`line_offset`, `max_lines`) for targeted reading with >=30-line overlap windows, and avoid \
             broad `memory_search` or shell/python scans over large documents unless explicitly required."
                .to_string(),
        );

        let database_prompt =
            render_smartbrain_database_prompt_for_config_dir(workspace_config_dir);
        if !database_prompt.trim().is_empty() {
            parts.push(database_prompt);
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## SmartBrain (智脑)\n\n{}\n\n\
             When you apply knowledge from SmartBrain, note which experience, knowledge, or database configuration helped.",
            parts.join("\n\n")
        )
    }
}

pub(crate) fn render_robot_runtime_prompt(
    workspace_config_dir: &Path,
    robot_id: Option<&str>,
) -> String {
    let Some(robot_id) = robot_id else {
        return String::new();
    };
    let Some(detail) = crate::robot_loader::read_robot(workspace_config_dir, robot_id) else {
        return String::new();
    };
    let trimmed_prompt = detail.config.system_prompt.trim();
    if trimmed_prompt.is_empty() {
        return String::new();
    }
    format!(
        "\n\n## Active Robot Identity ({robot_id})\n\n\
         The following robot system prompt is user-configured and must be treated as active instructions for this run:\n\n\
         {}",
        truncate_utf8_by_bytes(trimmed_prompt, 12_000)
    )
}
