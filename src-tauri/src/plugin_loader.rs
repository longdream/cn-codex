use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::config_system::{McpServerConfig, is_supported_mcp_http_url, normalize_mcp_transport};

const DEFAULT_SKILLS_DIR_NAME: &str = "skills";
const DEFAULT_MCP_CONFIG_FILE: &str = ".mcp.json";
const DEFAULT_APP_CONFIG_FILE: &str = ".app.json";
const CODEX_MANIFEST_RELATIVE_PATH: &str = ".codex-plugin/plugin.json";
const CLAUDE_MANIFEST_RELATIVE_PATH: &str = ".claude-plugin/plugin.json";
const DISABLED_MARKER_FILE: &str = ".cn-codex-disabled";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub path: String,
    pub manifest_path: String,
    pub skills_dir: Option<String>,
    pub skills_count: usize,
    pub skills: Vec<PluginSkillSummary>,
    pub apps_count: usize,
    pub apps: Vec<PluginAppSummary>,
    pub enabled: bool,
    pub has_mcp_servers: bool,
    pub has_apps: bool,
    pub has_hooks: bool,
    pub interface: Option<PluginInterfaceSummary>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetail {
    #[serde(flatten)]
    pub summary: PluginSummary,
    pub manifest: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAppSummary {
    pub key: String,
    pub connector_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInterfaceSummary {
    pub display_name: Option<String>,
    pub short_description: Option<String>,
    pub developer_name: Option<String>,
    pub category: Option<String>,
    pub capabilities: Vec<String>,
    pub brand_color: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginSkillPromptEntry {
    pub plugin_id: String,
    pub plugin_display_name: String,
    pub skill_name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PluginAppPromptEntry {
    pub plugin_id: String,
    pub plugin_display_name: String,
    pub app_key: String,
    pub connector_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginManifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    skills: Option<JsonValue>,
    #[serde(default, alias = "mcp_servers")]
    mcp_servers: Option<JsonValue>,
    #[serde(default)]
    apps: Option<JsonValue>,
    #[serde(default)]
    hooks: Option<JsonValue>,
    #[serde(default)]
    interface: Option<RawPluginInterface>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginInterface {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    developer_name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    brand_color: Option<String>,
}

pub fn list_plugins(workspace_config_dir: &Path) -> Vec<PluginSummary> {
    let plugins_dir = workspace_config_dir.join("plugins");
    let Ok(entries) = std::fs::read_dir(&plugins_dir) else {
        return Vec::new();
    };

    let mut plugins = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                plugin_summary_from_root(&path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    plugins
}

pub fn read_plugin(workspace_config_dir: &Path, plugin_id: &str) -> Option<PluginDetail> {
    if !is_safe_plugin_id(plugin_id) {
        return None;
    }

    let plugin_root = workspace_config_dir.join("plugins").join(plugin_id);
    let summary = plugin_summary_from_root(&plugin_root)?;
    let manifest = std::fs::read_to_string(&summary.manifest_path)
        .ok()
        .and_then(|content| serde_json::from_str::<JsonValue>(&content).ok());

    Some(PluginDetail { summary, manifest })
}

pub fn set_plugin_enabled(
    workspace_config_dir: &Path,
    plugin_id: &str,
    enabled: bool,
) -> Result<PluginSummary, String> {
    let plugin_root = plugin_root_for_id(workspace_config_dir, plugin_id)?;
    if find_plugin_manifest_path(&plugin_root).is_none() {
        return Err(format!("Plugin not found: {plugin_id}"));
    }

    let marker = plugin_root.join(DISABLED_MARKER_FILE);
    if enabled {
        if marker.exists() {
            std::fs::remove_file(&marker)
                .map_err(|error| format!("failed to enable plugin '{plugin_id}': {error}"))?;
        }
    } else {
        std::fs::write(&marker, "disabled\n")
            .map_err(|error| format!("failed to disable plugin '{plugin_id}': {error}"))?;
    }

    plugin_summary_from_root(&plugin_root).ok_or_else(|| format!("Plugin not found: {plugin_id}"))
}

pub fn uninstall_plugin(workspace_config_dir: &Path, plugin_id: &str) -> Result<(), String> {
    let plugin_root = plugin_root_for_id(workspace_config_dir, plugin_id)?;
    if find_plugin_manifest_path(&plugin_root).is_none() {
        return Err(format!("Plugin not found: {plugin_id}"));
    }
    std::fs::remove_dir_all(&plugin_root)
        .map_err(|error| format!("failed to uninstall plugin '{plugin_id}': {error}"))
}

pub fn list_plugin_skill_prompt_entries(
    workspace_config_dir: &Path,
) -> Vec<PluginSkillPromptEntry> {
    let mut entries = Vec::new();
    for plugin in list_plugins(workspace_config_dir) {
        if plugin.error.is_some() || !plugin.enabled {
            continue;
        }

        for skill in plugin.skills {
            entries.push(PluginSkillPromptEntry {
                plugin_id: plugin.id.clone(),
                plugin_display_name: plugin.display_name.clone(),
                skill_name: skill.name,
                description: skill.description,
                path: PathBuf::from(skill.path),
            });
        }
    }

    entries
}

const COMPUTER_USE_MCP_SERVER_NAME: &str = "computer-use";

/// 判断配置是否把 library client 误写成了 MCP Server 入口。
///
/// CN-Codex 的 Computer Use 必须使用 `computer-use-mcp-server.mjs`，
/// 不能直接把 `computer-use-client.mjs` 当作 stdio MCP 启动。
pub fn is_invalid_computer_use_mcp_server(server: &McpServerConfig) -> bool {
    let command = server.command.to_ascii_lowercase();
    if command.contains("computer-use-client") {
        return true;
    }
    server.args.iter().any(|arg| {
        let normalized = arg.replace('\\', "/").to_ascii_lowercase();
        normalized.contains("computer-use-client.mjs")
            || normalized.ends_with("/computer-use-client.js")
            || normalized.ends_with("computer-use-client")
    })
}

/// Computer Use 统一走 MCP 入口：即使插件被禁用，也注入其 MCP 配置。
fn computer_use_mcp_server(workspace_config_dir: &Path) -> Option<(String, McpServerConfig)> {
    let plugin_root = workspace_config_dir
        .join("plugins")
        .join(COMPUTER_USE_MCP_SERVER_NAME);
    if !plugin_root.is_dir() || find_plugin_manifest_path(&plugin_root).is_none() {
        return None;
    }

    // 优先读取插件内 .mcp.json；若缺失则回退到默认脚本入口。
    if let Some((name, server)) = plugin_mcp_servers_from_root(&plugin_root)
        .into_iter()
        .find(|(name, _)| name == COMPUTER_USE_MCP_SERVER_NAME)
    {
        return Some((name, server));
    }

    Some((
        COMPUTER_USE_MCP_SERVER_NAME.to_string(),
        McpServerConfig {
            name: COMPUTER_USE_MCP_SERVER_NAME.to_string(),
            transport: "stdio".to_string(),
            command: "node".to_string(),
            // CN-Codex 没有 node_repl，因此使用可直接调用的 MCP Server 封装。
            args: vec!["scripts/computer-use-mcp-server.mjs".to_string()],
            env: HashMap::new(),
            cwd: Some(plugin_root.to_string_lossy().to_string()),
            url: None,
            headers: HashMap::new(),
            disabled: false,
        },
    ))
}

pub fn list_plugin_mcp_servers(workspace_config_dir: &Path) -> HashMap<String, McpServerConfig> {
    let plugins_dir = workspace_config_dir.join("plugins");
    let Ok(entries) = std::fs::read_dir(&plugins_dir) else {
        return HashMap::new();
    };

    let mut plugin_roots = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| !is_plugin_disabled_root(path))
        .filter(|path| find_plugin_manifest_path(path).is_some())
        .collect::<Vec<_>>();
    plugin_roots.sort();

    let mut servers = HashMap::new();
    for plugin_root in plugin_roots {
        for (name, server) in plugin_mcp_servers_from_root(&plugin_root) {
            servers.entry(name).or_insert(server);
        }
    }

    // Computer Use 从插件能力迁移为 MCP；仍需尊重插件的启用状态，
    // 避免用户禁用后模型仍能自动调用并触发控制遮罩。
    let computer_use_root = workspace_config_dir
        .join("plugins")
        .join(COMPUTER_USE_MCP_SERVER_NAME);
    if !is_plugin_disabled_root(&computer_use_root)
        && let Some((name, server)) = computer_use_mcp_server(workspace_config_dir)
    {
        // 若已有配置但入口错误（client 库），用正确的 MCP Server 覆盖。
        if servers
            .get(&name)
            .is_some_and(is_invalid_computer_use_mcp_server)
        {
            servers.insert(name, server);
        } else {
            servers.entry(name).or_insert(server);
        }
    }

    servers
}

pub fn list_plugin_app_prompt_entries(workspace_config_dir: &Path) -> Vec<PluginAppPromptEntry> {
    let mut entries = Vec::new();
    for plugin in list_plugins(workspace_config_dir) {
        if plugin.error.is_some() || !plugin.enabled {
            continue;
        }

        for app in plugin.apps {
            entries.push(PluginAppPromptEntry {
                plugin_id: plugin.id.clone(),
                plugin_display_name: plugin.display_name.clone(),
                app_key: app.key,
                connector_id: app.connector_id,
            });
        }
    }

    entries.sort_by(|left, right| {
        left.plugin_id
            .cmp(&right.plugin_id)
            .then(left.app_key.cmp(&right.app_key))
    });
    entries
}

fn plugin_summary_from_root(plugin_root: &Path) -> Option<PluginSummary> {
    let id = plugin_root.file_name()?.to_string_lossy().to_string();
    let enabled = !is_plugin_disabled_root(plugin_root);
    let manifest_path = find_plugin_manifest_path(plugin_root)?;
    let path = plugin_root.to_string_lossy().to_string();
    let manifest_path_str = manifest_path.to_string_lossy().to_string();

    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(content) => content,
        Err(err) => {
            return Some(error_summary(
                id,
                path,
                manifest_path_str,
                enabled,
                format!("Failed to read plugin manifest: {err}"),
            ));
        }
    };

    let manifest = match serde_json::from_str::<RawPluginManifest>(&content) {
        Ok(manifest) => manifest,
        Err(err) => {
            return Some(error_summary(
                id,
                path,
                manifest_path_str,
                enabled,
                format!("Failed to parse plugin manifest: {err}"),
            ));
        }
    };

    let mut warnings = Vec::new();
    let name = if manifest.name.trim().is_empty() {
        id.clone()
    } else {
        manifest.name.trim().to_string()
    };
    let version = manifest
        .version
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty());
    let description = manifest
        .description
        .map(|description| description.trim().to_string())
        .filter(|description| !description.is_empty());
    let interface = manifest.interface.map(PluginInterfaceSummary::from);
    let display_name = interface
        .as_ref()
        .and_then(|interface| interface.display_name.as_deref())
        .filter(|display_name| !display_name.trim().is_empty())
        .unwrap_or(&name)
        .trim()
        .to_string();
    let custom_skills_dir = resolve_manifest_path_value(
        plugin_root,
        "skills",
        manifest.skills.as_ref(),
        &mut warnings,
    );
    let skill_roots = plugin_skill_roots(plugin_root, custom_skills_dir.as_ref());
    let skills_dir = custom_skills_dir
        .or_else(|| {
            let default_dir = plugin_root.join(DEFAULT_SKILLS_DIR_NAME);
            default_dir.is_dir().then_some(default_dir)
        })
        .map(|path| path.to_string_lossy().to_string());
    let skills = collect_skills(&skill_roots);
    let app_config_paths =
        plugin_app_config_paths(plugin_root, manifest.apps.as_ref(), &mut warnings);
    let mut apps = collect_apps(&app_config_paths);
    if apps.is_empty() {
        collect_inline_apps(
            manifest.apps.as_ref(),
            &format!("{manifest_path_str}#apps"),
            &mut apps,
        );
    }

    Some(PluginSummary {
        id,
        name,
        display_name,
        version,
        description,
        keywords: manifest.keywords,
        path,
        manifest_path: manifest_path_str,
        skills_dir,
        skills_count: skills.len(),
        skills,
        apps_count: apps.len(),
        has_apps: !apps.is_empty()
            || manifest.apps.is_some()
            || plugin_root.join(DEFAULT_APP_CONFIG_FILE).is_file(),
        apps,
        enabled,
        has_mcp_servers: manifest.mcp_servers.is_some() || plugin_root.join(".mcp.json").is_file(),
        has_hooks: manifest.hooks.is_some() || plugin_root.join("hooks/hooks.json").is_file(),
        interface,
        error: None,
        warnings,
    })
}

fn plugin_mcp_servers_from_root(plugin_root: &Path) -> HashMap<String, McpServerConfig> {
    let mut servers = HashMap::new();
    for config_path in plugin_mcp_config_paths(plugin_root) {
        let Ok(content) = std::fs::read_to_string(&config_path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<JsonValue>(&content) else {
            continue;
        };
        let Some(server_map) = mcp_server_map(&parsed) else {
            continue;
        };

        for (name, value) in server_map {
            if let Some(server) = parse_plugin_mcp_server(plugin_root, name, value) {
                servers.entry(name.clone()).or_insert(server);
            }
        }
    }

    servers
}

fn plugin_mcp_config_paths(plugin_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Some(manifest_path) = find_plugin_manifest_path(plugin_root) else {
        return paths;
    };
    let Ok(content) = std::fs::read_to_string(manifest_path) else {
        return paths;
    };
    let Ok(manifest) = serde_json::from_str::<RawPluginManifest>(&content) else {
        return paths;
    };

    if let Some(custom_path) = resolve_manifest_path_value(
        plugin_root,
        "mcpServers",
        manifest.mcp_servers.as_ref(),
        &mut Vec::new(),
    ) {
        if custom_path.is_file() {
            paths.push(custom_path);
        }
        return paths;
    }

    let default_path = plugin_root.join(DEFAULT_MCP_CONFIG_FILE);
    if default_path.is_file() {
        paths.push(default_path);
    }

    paths.sort();
    paths.dedup();
    paths
}

fn mcp_server_map(value: &JsonValue) -> Option<&serde_json::Map<String, JsonValue>> {
    let object = value.as_object()?;
    if let Some(map) = object.get("mcpServers").and_then(JsonValue::as_object) {
        return Some(map);
    }
    if let Some(map) = object.get("mcp_servers").and_then(JsonValue::as_object) {
        return Some(map);
    }
    Some(object)
}

fn parse_plugin_mcp_server(
    plugin_root: &Path,
    name: &str,
    value: &JsonValue,
) -> Option<McpServerConfig> {
    let object = value.as_object()?;
    let url = object
        .get("url")
        .or_else(|| object.get("server_url"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToString::to_string);
    let transport = normalize_mcp_transport(
        object
            .get("type")
            .or_else(|| object.get("transport"))
            .and_then(JsonValue::as_str),
        url.as_deref(),
    );

    let command = object
        .get("command")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if transport == "stdio" && command.is_empty() {
        return None;
    }
    if (transport == "http" || transport == "sse") && !is_supported_mcp_http_url(url.as_deref()) {
        return None;
    }

    let args = object
        .get("args")
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let headers = object
        .get("headers")
        .or_else(|| object.get("http_headers"))
        .and_then(JsonValue::as_object)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|val| (key.clone(), val.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let env = object
        .get("env")
        .and_then(JsonValue::as_object)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|val| (key.clone(), val.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let cwd = object
        .get("cwd")
        .and_then(JsonValue::as_str)
        .filter(|cwd| !cwd.trim().is_empty())
        .map(|cwd| {
            let path = Path::new(cwd);
            if path.is_absolute() {
                cwd.to_string()
            } else {
                plugin_root.join(path).to_string_lossy().to_string()
            }
        });
    let disabled = object
        .get("disabled")
        .and_then(JsonValue::as_bool)
        .or_else(|| {
            object
                .get("isActive")
                .or_else(|| object.get("is_active"))
                .or_else(|| object.get("enabled"))
                .and_then(JsonValue::as_bool)
                .map(|active| !active)
        })
        .unwrap_or(false);

    Some(McpServerConfig {
        name: name.to_string(),
        transport,
        command,
        args,
        env,
        cwd,
        url,
        headers,
        disabled,
    })
}

fn plugin_app_config_paths(
    plugin_root: &Path,
    manifest_apps: Option<&JsonValue>,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(custom_path) =
        resolve_manifest_path_value(plugin_root, "apps", manifest_apps, warnings)
    {
        if custom_path.is_file() {
            paths.push(custom_path);
        } else {
            warnings.push(format!(
                "Ignoring apps: file does not exist: {}",
                custom_path.display()
            ));
        }
        return paths;
    }

    if manifest_apps.is_some() {
        return paths;
    }

    let default_path = plugin_root.join(DEFAULT_APP_CONFIG_FILE);
    if default_path.is_file() {
        paths.push(default_path);
    }

    paths.sort();
    paths.dedup();
    paths
}

fn collect_apps(config_paths: &[PathBuf]) -> Vec<PluginAppSummary> {
    let mut apps = Vec::new();
    for config_path in config_paths {
        let Ok(content) = std::fs::read_to_string(config_path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<JsonValue>(&content) else {
            continue;
        };
        collect_apps_from_value(&parsed, &config_path.to_string_lossy(), &mut apps);
    }

    dedupe_apps(&mut apps);
    apps
}

fn collect_inline_apps(
    value: Option<&JsonValue>,
    source_path: &str,
    apps: &mut Vec<PluginAppSummary>,
) {
    let Some(value) = value else {
        return;
    };
    if value.is_object() {
        collect_apps_from_value(value, source_path, apps);
        dedupe_apps(apps);
    }
}

fn collect_apps_from_value(value: &JsonValue, source_path: &str, apps: &mut Vec<PluginAppSummary>) {
    let Some(apps_map) = app_map(value) else {
        return;
    };

    for (key, value) in apps_map {
        let connector_id = value
            .get("id")
            .and_then(JsonValue::as_str)
            .or_else(|| value.as_str())
            .unwrap_or_default()
            .trim();
        if connector_id.is_empty() {
            continue;
        }

        apps.push(PluginAppSummary {
            key: key.trim().to_string(),
            connector_id: connector_id.to_string(),
            path: source_path.to_string(),
        });
    }
}

fn app_map(value: &JsonValue) -> Option<&serde_json::Map<String, JsonValue>> {
    let object = value.as_object()?;
    if let Some(map) = object.get("apps").and_then(JsonValue::as_object) {
        return Some(map);
    }
    Some(object)
}

fn dedupe_apps(apps: &mut Vec<PluginAppSummary>) {
    let mut seen = HashSet::new();
    apps.retain(|app| seen.insert(app.connector_id.clone()));
    apps.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then(left.connector_id.cmp(&right.connector_id))
    });
}

fn error_summary(
    id: String,
    path: String,
    manifest_path: String,
    enabled: bool,
    error: String,
) -> PluginSummary {
    PluginSummary {
        display_name: id.clone(),
        name: id.clone(),
        id,
        version: None,
        description: None,
        keywords: Vec::new(),
        path,
        manifest_path,
        skills_dir: None,
        skills_count: 0,
        skills: Vec::new(),
        apps_count: 0,
        apps: Vec::new(),
        enabled,
        has_mcp_servers: false,
        has_apps: false,
        has_hooks: false,
        interface: None,
        error: Some(error),
        warnings: Vec::new(),
    }
}

fn find_plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [CODEX_MANIFEST_RELATIVE_PATH, CLAUDE_MANIFEST_RELATIVE_PATH]
        .iter()
        .map(|relative| plugin_root.join(relative))
        .find(|path| path.is_file())
}

fn resolve_manifest_path_value(
    plugin_root: &Path,
    field: &str,
    value: Option<&JsonValue>,
    warnings: &mut Vec<String>,
) -> Option<PathBuf> {
    let Some(value) = value else {
        return None;
    };

    let Some(raw_path) = value.as_str() else {
        warnings.push(format!("Ignoring {field}: expected a string path"));
        return None;
    };

    match resolve_manifest_relative_path(plugin_root, raw_path) {
        Ok(path) => Some(path),
        Err(message) => {
            warnings.push(format!("Ignoring {field}: {message}"));
            None
        }
    }
}

fn resolve_manifest_relative_path(plugin_root: &Path, raw_path: &str) -> Result<PathBuf, String> {
    let Some(relative_path) = raw_path.strip_prefix("./") else {
        return Err("path must start with `./` relative to plugin root".to_string());
    };

    if relative_path.is_empty() {
        return Err("path must not be `./`".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => return Err("path must not contain `..`".to_string()),
            _ => return Err("path must stay within the plugin root".to_string()),
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err("path must not be empty".to_string());
    }

    Ok(plugin_root.join(normalized))
}

fn plugin_skill_roots(plugin_root: &Path, custom_skills_dir: Option<&PathBuf>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let default_dir = plugin_root.join(DEFAULT_SKILLS_DIR_NAME);
    if default_dir.is_dir() {
        roots.push(default_dir);
    }

    if let Some(custom_dir) = custom_skills_dir {
        if custom_dir.is_dir() {
            roots.push(custom_dir.clone());
        }
    }

    let mut seen = HashSet::new();
    roots.retain(|root| seen.insert(root.to_string_lossy().to_string()));
    roots.sort();
    roots
}

fn collect_skills(skill_roots: &[PathBuf]) -> Vec<PluginSkillSummary> {
    let mut skills = Vec::new();
    for root in skill_roots {
        if root.join("SKILL.md").is_file() {
            if let Some(skill) = skill_from_dir(root, root) {
                skills.push(skill);
            }
        }

        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(skill) = skill_from_dir(root, &path) {
                skills.push(skill);
            }
        }
    }

    skills.sort_by(|left, right| left.id.cmp(&right.id));
    skills
}

fn skill_from_dir(root: &Path, skill_dir: &Path) -> Option<PluginSkillSummary> {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return None;
    }

    let id = skill_dir
        .strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .and_then(|relative| relative.to_str())
        .map(|relative| relative.replace('\\', "/"))
        .or_else(|| {
            skill_dir
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "skill".to_string());
    let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
    let (name, description, tags) = parse_skill_frontmatter(&content);

    Some(PluginSkillSummary {
        name: if name.is_empty() { id.clone() } else { name },
        description,
        tags,
        path: skill_md.to_string_lossy().to_string(),
        id,
    })
}

fn parse_skill_frontmatter(content: &str) -> (String, String, Vec<String>) {
    let mut name = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();

    if !content.starts_with("---") {
        return (name, description, tags);
    }

    let Some(end) = content[3..].find("---") else {
        return (name, description, tags);
    };

    let frontmatter = &content[3..3 + end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("tags:") {
            let val = val.trim();
            if val.starts_with('[') {
                tags = val
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }

    (name, description, tags)
}

fn is_safe_plugin_id(plugin_id: &str) -> bool {
    if plugin_id.trim().is_empty() {
        return false;
    }

    let mut components = Path::new(plugin_id).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

pub fn is_plugin_disabled_root(plugin_root: &Path) -> bool {
    plugin_root.join(DISABLED_MARKER_FILE).is_file()
}

fn plugin_root_for_id(workspace_config_dir: &Path, plugin_id: &str) -> Result<PathBuf, String> {
    if !is_safe_plugin_id(plugin_id) {
        return Err(format!("Invalid plugin id: {plugin_id}"));
    }
    Ok(workspace_config_dir.join("plugins").join(plugin_id))
}

impl From<RawPluginInterface> for PluginInterfaceSummary {
    fn from(value: RawPluginInterface) -> Self {
        Self {
            display_name: value.display_name,
            short_description: value.short_description,
            developer_name: value.developer_name,
            category: value.category,
            capabilities: value.capabilities,
            brand_color: value.brand_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cn-codex-plugin-test-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("file parent")).expect("create parent");
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn discovers_default_plugin_skills() {
        let root = unique_temp_dir("default-skills");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{
  "name": "demo",
  "version": " 1.0.0 ",
  "description": "Demo plugin",
  "keywords": ["demo"],
  "interface": { "displayName": "Demo Plugin" }
}"#,
        );
        write_file(
            &plugin_root.join("skills/reviewer/SKILL.md"),
            "---\nname: Reviewer\ndescription: Checks code\ntags: [\"review\"]\n---\n# Body",
        );

        let plugins = list_plugins(&root);

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].display_name, "Demo Plugin");
        assert_eq!(plugins[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(plugins[0].skills_count, 1);
        assert_eq!(plugins[0].skills[0].name, "Reviewer");
        assert!(plugins[0].error.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_custom_manifest_skills_dir() {
        let root = unique_temp_dir("custom-skills");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{
  "name": "demo",
  "skills": "./capabilities"
}"#,
        );
        write_file(
            &plugin_root.join("capabilities/maker/SKILL.md"),
            "---\nname: Maker\n---\n# Body",
        );

        let plugins = list_plugins(&root);

        assert_eq!(plugins[0].skills_count, 1);
        assert_eq!(plugins[0].skills[0].name, "Maker");
        assert!(
            plugins[0]
                .skills_dir
                .as_deref()
                .unwrap()
                .ends_with("capabilities")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_manifest_skills_path_escape() {
        let root = unique_temp_dir("path-escape");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{
  "name": "demo",
  "skills": "../outside"
}"#,
        );

        let plugins = list_plugins(&root);

        assert_eq!(plugins[0].skills_count, 0);
        assert!(!plugins[0].warnings.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn returns_malformed_manifest_as_error_summary() {
        let root = unique_temp_dir("malformed");
        let plugin_root = root.join("plugins/bad");
        write_file(&plugin_root.join(".codex-plugin/plugin.json"), "{ bad json");

        let plugins = list_plugins(&root);

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "bad");
        assert!(
            plugins[0]
                .error
                .as_deref()
                .unwrap()
                .contains("Failed to parse")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_alternate_claude_manifest() {
        let root = unique_temp_dir("alternate");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".claude-plugin/plugin.json"),
            r#"{ "name": "demo", "interface": { "displayName": "Alternate" } }"#,
        );

        let plugins = list_plugins(&root);

        assert_eq!(plugins[0].display_name, "Alternate");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_read_rejects_path_traversal() {
        let root = unique_temp_dir("safe-id");

        assert!(read_plugin(&root, "../demo").is_none());
        assert!(read_plugin(&root, "nested/demo").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_plugins_are_listed_but_not_loaded() {
        let root = unique_temp_dir("disabled");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{
  "name": "demo",
  "interface": { "displayName": "Demo Plugin" }
}"#,
        );
        write_file(
            &plugin_root.join("skills/reviewer/SKILL.md"),
            "---\nname: Reviewer\ndescription: Checks code\n---\n# Body",
        );
        write_file(
            &plugin_root.join(".mcp.json"),
            r#"{"mcpServers":{"demo":{"command":"node"}}}"#,
        );
        write_file(
            &plugin_root.join(".app.json"),
            r#"{"apps":{"demo":{"id":"connector_demo"}}}"#,
        );

        let disabled = set_plugin_enabled(&root, "demo", false).expect("disable plugin");
        assert!(!disabled.enabled);
        assert_eq!(list_plugins(&root)[0].display_name, "Demo Plugin");
        assert!(
            !list_plugin_skill_prompt_entries(&root)
                .iter()
                .any(|entry| entry.plugin_id == "demo")
        );
        assert!(list_plugin_mcp_servers(&root).is_empty());
        assert!(list_plugin_app_prompt_entries(&root).is_empty());

        let enabled = set_plugin_enabled(&root, "demo", true).expect("enable plugin");
        assert!(enabled.enabled);
        assert!(
            list_plugin_skill_prompt_entries(&root)
                .iter()
                .any(|entry| entry.plugin_id == "demo")
        );
        assert!(list_plugin_mcp_servers(&root).contains_key("demo"));
        assert!(
            list_plugin_app_prompt_entries(&root)
                .iter()
                .any(|entry| entry.connector_id == "connector_demo")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn computer_use_mcp_is_available_even_when_plugin_disabled() {
        let root = unique_temp_dir("computer-use-mcp");
        let plugin_root = root.join("plugins/computer-use");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{ "name": "computer-use", "interface": { "displayName": "Computer Use" } }"#,
        );
        write_file(
            &plugin_root.join(".mcp.json"),
            r#"{"mcpServers":{"computer-use":{"command":"node","args":["scripts/computer-use-mcp-server.mjs"]}}}"#,
        );

        set_plugin_enabled(&root, "computer-use", false).expect("disable computer-use plugin");
        let servers = list_plugin_mcp_servers(&root);
        assert!(!servers.contains_key("computer-use"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_computer_use_client_mcp_entry_is_detected() {
        let invalid = McpServerConfig {
            name: "computer-use".to_string(),
            transport: "stdio".to_string(),
            command: "node".to_string(),
            args: vec!["plugins/computer-use/scripts/computer-use-client.mjs".to_string()],
            env: HashMap::new(),
            cwd: Some("codey".to_string()),
            url: None,
            headers: HashMap::new(),
            disabled: false,
        };
        let valid = McpServerConfig {
            name: "computer-use".to_string(),
            transport: "stdio".to_string(),
            command: "node".to_string(),
            args: vec!["scripts/computer-use-mcp-server.mjs".to_string()],
            env: HashMap::new(),
            cwd: Some("codey/plugins/computer-use".to_string()),
            url: None,
            headers: HashMap::new(),
            disabled: false,
        };

        assert!(is_invalid_computer_use_mcp_server(&invalid));
        assert!(!is_invalid_computer_use_mcp_server(&valid));
    }

    #[test]
    fn uninstall_plugin_removes_only_safe_plugin_ids() {
        let root = unique_temp_dir("uninstall");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{ "name": "demo" }"#,
        );

        assert!(uninstall_plugin(&root, "../demo").is_err());
        uninstall_plugin(&root, "demo").expect("uninstall plugin");
        assert!(!plugin_root.exists());
        assert!(list_plugins(&root).is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_default_plugin_mcp_servers() {
        let root = unique_temp_dir("mcp-default");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{ "name": "demo" }"#,
        );
        write_file(
            &plugin_root.join(".mcp.json"),
            r#"{
  "mcpServers": {
    "docs": {
      "type": "stdio",
      "command": "node",
      "args": ["server.js"],
      "cwd": "mcp",
      "env": { "TOKEN": "abc" }
    },
    "remote": {
      "type": "http",
      "url": "https://example.com/mcp"
    },
    "remote_sse": {
      "type": "sse",
      "url": "http://10.0.0.1:3000/sse?id=demo",
      "isActive": true
    }
  }
}"#,
        );

        let servers = list_plugin_mcp_servers(&root);

        assert_eq!(servers.len(), 3);
        let docs = servers.get("docs").unwrap();
        assert_eq!(docs.transport, "stdio");
        assert_eq!(docs.command, "node");
        assert_eq!(docs.args, vec!["server.js"]);
        assert_eq!(docs.env.get("TOKEN").map(String::as_str), Some("abc"));
        assert_eq!(
            docs.cwd.as_deref().map(PathBuf::from),
            Some(plugin_root.join("mcp"))
        );
        let remote = servers.get("remote").unwrap();
        assert_eq!(remote.transport, "http");
        assert_eq!(remote.url.as_deref(), Some("https://example.com/mcp"));
        let remote_sse = servers.get("remote_sse").unwrap();
        assert_eq!(remote_sse.transport, "sse");
        assert_eq!(
            remote_sse.url.as_deref(),
            Some("http://10.0.0.1:3000/sse?id=demo")
        );
        assert!(!remote_sse.disabled);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_custom_manifest_mcp_servers_file() {
        let root = unique_temp_dir("mcp-custom");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{
  "name": "demo",
  "mcpServers": "./mcp/servers.json"
}"#,
        );
        write_file(
            &plugin_root.join(".mcp.json"),
            r#"{ "mcpServers": { "default": { "command": "node" } } }"#,
        );
        write_file(
            &plugin_root.join("mcp/servers.json"),
            r#"{
  "custom": {
    "command": "python",
    "args": ["server.py"],
    "enabled": false
  }
}"#,
        );

        let servers = list_plugin_mcp_servers(&root);

        assert_eq!(servers.len(), 1);
        let custom = servers.get("custom").unwrap();
        assert_eq!(custom.command, "python");
        assert!(custom.disabled);
        assert!(!servers.contains_key("default"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_default_plugin_apps() {
        let root = unique_temp_dir("apps-default");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{ "name": "demo", "interface": { "displayName": "Demo Apps" } }"#,
        );
        write_file(
            &plugin_root.join(".app.json"),
            r#"{
  "apps": {
    "calendar": { "id": "connector_calendar" },
    "mail": { "id": "connector_mail" }
  }
}"#,
        );

        let plugins = list_plugins(&root);
        let entries = list_plugin_app_prompt_entries(&root);

        assert_eq!(plugins[0].apps_count, 2);
        assert!(plugins[0].has_apps);
        assert_eq!(plugins[0].apps[0].key, "calendar");
        assert_eq!(plugins[0].apps[0].connector_id, "connector_calendar");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].plugin_display_name, "Demo Apps");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn custom_manifest_apps_file_replaces_default_apps_file() {
        let root = unique_temp_dir("apps-custom");
        let plugin_root = root.join("plugins/demo");
        write_file(
            &plugin_root.join(".codex-plugin/plugin.json"),
            r#"{
  "name": "demo",
  "apps": "./config/custom.app.json"
}"#,
        );
        write_file(
            &plugin_root.join(".app.json"),
            r#"{"apps":{"ignored":{"id":"connector_ignored"}}}"#,
        );
        write_file(
            &plugin_root.join("config/custom.app.json"),
            r#"{"apps":{"custom":{"id":"connector_custom"}}}"#,
        );

        let plugins = list_plugins(&root);

        assert_eq!(plugins[0].apps_count, 1);
        assert_eq!(plugins[0].apps[0].key, "custom");
        assert_eq!(plugins[0].apps[0].connector_id, "connector_custom");

        let _ = fs::remove_dir_all(root);
    }
}
