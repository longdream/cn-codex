//! Local MiniApp (MCP + Web UI) registry and runtime.
//!
//! MiniApps live under `codey/miniapps/<slug>/` and are launched with the
//! bundled `codey/node` runtime. Phase 2 focuses on scaffold, registry,
//! process lifecycle, and frontend list/control APIs.

pub mod commands;
pub mod runtime;
pub mod scaffold;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config_system::McpServerConfig;
use std::collections::HashMap;

pub const MINIAPPS_DIR_NAME: &str = "miniapps";
pub const REGISTRY_FILE_NAME: &str = "registry.json";
pub const MANIFEST_FILE_NAME: &str = "miniapp.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MiniAppStatus {
    Draft,
    Generated,
    Running,
    Stopped,
    Error,
}

impl Default for MiniAppStatus {
    fn default() -> Self {
        Self::Draft
    }
}

impl MiniAppStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MiniAppStatus::Draft => "draft",
            MiniAppStatus::Generated => "generated",
            MiniAppStatus::Running => "running",
            MiniAppStatus::Stopped => "stopped",
            MiniAppStatus::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MiniAppPage {
    pub id: String,
    pub title: String,
    pub path: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MiniAppToolMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MiniAppMcpConfig {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniAppRecord {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub database_id: String,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub status: MiniAppStatus,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub root_path: String,
    #[serde(default)]
    pub mcp: MiniAppMcpConfig,
    #[serde(default)]
    pub pages: Vec<MiniAppPage>,
    #[serde(default)]
    pub tools: Vec<MiniAppToolMeta>,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct MiniAppRegistryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    apps: Vec<MiniAppRecord>,
}

pub fn miniapps_root(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join(MINIAPPS_DIR_NAME)
}

pub fn registry_path(workspace_config_dir: &Path) -> PathBuf {
    miniapps_root(workspace_config_dir).join(REGISTRY_FILE_NAME)
}

pub fn app_dir(workspace_config_dir: &Path, slug: &str) -> PathBuf {
    miniapps_root(workspace_config_dir).join(slug)
}

pub fn ensure_miniapps_dir(workspace_config_dir: &Path) -> Result<PathBuf, String> {
    let root = miniapps_root(workspace_config_dir);
    fs::create_dir_all(&root).map_err(|e| format!("创建 miniapps 目录失败: {e}"))?;
    Ok(root)
}

pub fn load_registry(workspace_config_dir: &Path) -> Result<Vec<MiniAppRecord>, String> {
    ensure_miniapps_dir(workspace_config_dir)?;
    let path = registry_path(workspace_config_dir);
    if !path.exists() {
        // Fall back to scanning directories that already contain miniapp.json.
        return Ok(scan_miniapps_from_disk(workspace_config_dir)?);
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取小程序注册表失败: {e}"))?;
    let mut file: MiniAppRegistryFile =
        serde_json::from_str(&raw).map_err(|e| format!("解析小程序注册表失败: {e}"))?;
    // The manifest is the main-chain Agent's output contract. Merge it on every
    // load so packages created or updated directly in codey/miniapps are
    // discoverable without a separate registry command.
    for disk_app in scan_miniapps_from_disk(workspace_config_dir)? {
        if let Some(registered) = file
            .apps
            .iter_mut()
            .find(|app| app.id == disk_app.id || app.slug == disk_app.slug)
        {
            merge_generated_manifest(registered, disk_app);
        } else {
            file.apps.push(disk_app);
        }
    }
    // Reconcile running flags against in-memory process table.
    for app in &mut file.apps {
        if runtime::is_running(&app.slug) {
            app.status = MiniAppStatus::Running;
            if let Some(port) = runtime::running_port(&app.slug) {
                app.port = Some(port);
            }
        } else if app.status == MiniAppStatus::Running {
            app.status = MiniAppStatus::Stopped;
        }
    }
    file.apps.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(file.apps)
}

fn merge_generated_manifest(registered: &mut MiniAppRecord, generated: MiniAppRecord) {
    if !generated.name.trim().is_empty() {
        registered.name = generated.name;
    }
    if !generated.description.trim().is_empty() {
        registered.description = generated.description;
    }
    if registered.database_id.trim().is_empty() && !generated.database_id.trim().is_empty() {
        registered.database_id = generated.database_id;
        registered.database_name = generated.database_name;
    }
    if !generated.root_path.trim().is_empty() {
        registered.root_path = generated.root_path;
    }
    if !generated.mcp.command.trim().is_empty() || !generated.mcp.args.is_empty() {
        registered.mcp = generated.mcp;
    }
    if !generated.pages.is_empty() {
        registered.pages = generated.pages;
    }
    if !generated.tools.is_empty() {
        registered.tools = generated.tools;
    }
    registered.updated_at = registered.updated_at.max(generated.updated_at);
}

pub fn save_registry(workspace_config_dir: &Path, apps: &[MiniAppRecord]) -> Result<(), String> {
    ensure_miniapps_dir(workspace_config_dir)?;
    let path = registry_path(workspace_config_dir);
    let file = MiniAppRegistryFile {
        version: 1,
        apps: apps.to_vec(),
    };
    let raw =
        serde_json::to_string_pretty(&file).map_err(|e| format!("序列化小程序注册表失败: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("写入小程序注册表失败: {e}"))
}

pub fn find_app<'a>(apps: &'a [MiniAppRecord], id_or_slug: &str) -> Option<&'a MiniAppRecord> {
    apps.iter()
        .find(|app| app.id == id_or_slug || app.slug == id_or_slug)
}

pub fn find_app_mut<'a>(
    apps: &'a mut [MiniAppRecord],
    id_or_slug: &str,
) -> Option<&'a mut MiniAppRecord> {
    apps.iter_mut()
        .find(|app| app.id == id_or_slug || app.slug == id_or_slug)
}

pub fn validate_slug(slug: &str) -> Result<(), String> {
    let trimmed = slug.trim();
    if trimmed.is_empty() {
        return Err("英文名称不能为空".into());
    }
    let valid = trimmed.chars().enumerate().all(|(idx, ch)| match ch {
        'a'..='z' => true,
        '0'..='9' | '_' | '-' if idx > 0 => true,
        _ => false,
    });
    if !valid {
        return Err("英文名称需匹配 [a-z][a-z0-9_-]*".into());
    }
    Ok(())
}

pub fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn scan_miniapps_from_disk(workspace_config_dir: &Path) -> Result<Vec<MiniAppRecord>, String> {
    let root = miniapps_root(workspace_config_dir);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut apps = Vec::new();
    let entries = fs::read_dir(&root).map_err(|e| format!("扫描 miniapps 失败: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join(MANIFEST_FILE_NAME);
        if !manifest.exists() {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&manifest) {
            if let Ok(mut app) = serde_json::from_str::<MiniAppRecord>(&raw) {
                // The containing directory is authoritative. Generated or
                // copied manifests may contain an empty or stale absolute path.
                app.root_path = path.to_string_lossy().to_string();
                apps.push(app);
            }
        }
    }
    apps.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(apps)
}

pub fn write_manifest(app: &MiniAppRecord) -> Result<(), String> {
    let root = PathBuf::from(&app.root_path);
    fs::create_dir_all(&root).map_err(|e| format!("创建小程序目录失败: {e}"))?;
    let path = root.join(MANIFEST_FILE_NAME);
    let raw =
        serde_json::to_string_pretty(app).map_err(|e| format!("序列化 miniapp.json 失败: {e}"))?;
    fs::write(path, raw).map_err(|e| format!("写入 miniapp.json 失败: {e}"))
}

pub fn read_manifest(root: &Path) -> Result<MiniAppRecord, String> {
    let path = root.join(MANIFEST_FILE_NAME);
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取 miniapp.json 失败: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析 miniapp.json 失败: {e}"))
}

pub fn resolve_bundled_node(workspace_config_dir: &Path) -> Result<PathBuf, String> {
    let candidates = [
        workspace_config_dir.join("node").join(node_bin_name()),
        workspace_config_dir
            .parent()
            .unwrap_or(workspace_config_dir)
            .join("codey")
            .join("node")
            .join(node_bin_name()),
    ];
    for path in candidates {
        if path.exists() {
            return Ok(path);
        }
    }
    // Fall back to PATH `node` so local development still works.
    Ok(PathBuf::from(if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }))
}

fn node_bin_name() -> &'static str {
    if cfg!(windows) { "node.exe" } else { "node" }
}

pub fn open_page_url(app: &MiniAppRecord, page_id: Option<&str>) -> Option<String> {
    let port = app.port?;
    let page = page_id
        .and_then(|id| {
            app.pages
                .iter()
                .find(|p| p.id == id || p.path.trim_start_matches('/') == id)
        })
        .or_else(|| app.pages.first());
    let path = page
        .map(|p| {
            if p.path.starts_with('/') {
                p.path.clone()
            } else {
                format!("/{}", p.path.trim_start_matches('/'))
            }
        })
        .unwrap_or_else(|| "/".to_string());
    Some(format!("http://127.0.0.1:{port}{path}"))
}

/// Convert a MiniApp record into an MCP stdio server config for the main chain.
pub fn app_to_mcp_server(
    app: &MiniAppRecord,
    workspace_config_dir: Option<&Path>,
) -> Option<McpServerConfig> {
    if app.root_path.trim().is_empty() {
        return None;
    }
    let root = PathBuf::from(&app.root_path);
    if !root.is_dir() {
        return None;
    }

    let command = if !app.mcp.command.trim().is_empty() {
        app.mcp.command.clone()
    } else {
        // Prefer bundled node under workspace `codey/`.
        // root is typically `.../codey/miniapps/<slug>`.
        let workspace = root
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(root.as_path());
        resolve_bundled_node(workspace)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    "node.exe".into()
                } else {
                    "node".into()
                }
            })
    };

    let args = if app.mcp.args.is_empty() {
        vec!["server/index.mjs".to_string()]
    } else {
        app.mcp.args.clone()
    };

    let cwd = app
        .mcp
        .cwd
        .clone()
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| app.root_path.clone());

    let mut env = HashMap::new();
    if !app.database_id.is_empty() {
        env.insert("MINIAPP_DATABASE_ID".into(), app.database_id.clone());
    }
    if !app.name.is_empty() {
        env.insert("MINIAPP_NAME".into(), app.name.clone());
    }
    env.insert("MINIAPP_SLUG".into(), app.slug.clone());

    // Port policy for MCP spawn (may run while host miniapp is already up):
    // 1) Host process already running → reuse its HTTP port (skip re-bind in Node).
    // 2) Preferred port free → bind it.
    // 3) Preferred port busy → omit MINIAPP_PORT so Node picks a free port.
    if let Some(port) = runtime::running_port(&app.slug).filter(|p| *p > 0) {
        env.insert("MINIAPP_PORT".into(), port.to_string());
        env.insert("PORT".into(), port.to_string());
        env.insert("MINIAPP_REUSE_HTTP".into(), "1".into());
    } else if let Some(port) = app.port.filter(|p| *p > 0) {
        if runtime::is_port_available(port) {
            env.insert("MINIAPP_PORT".into(), port.to_string());
            env.insert("PORT".into(), port.to_string());
        }
        // busy preferred port: leave MINIAPP_PORT unset → Node listen(0)
    }

    // Align MCP spawn env with host miniapp_start so DB-backed tools work.
    if let Some(dir) = workspace_config_dir {
        if let Some((host, db_port, user, password, name)) =
            runtime::resolve_miniapp_db_env(dir, &app.database_id)
        {
            env.insert("MINIAPP_DB_HOST".into(), host);
            env.insert("MINIAPP_DB_PORT".into(), db_port);
            env.insert("MINIAPP_DB_USER".into(), user);
            env.insert("MINIAPP_DB_PASSWORD".into(), password);
            env.insert("MINIAPP_DB_NAME".into(), name);
        }
    }

    // MCP server name uses slug so prompts can say mcp_call_tool(server=<slug>).
    Some(McpServerConfig {
        name: app.slug.clone(),
        transport: "stdio".to_string(),
        command,
        args,
        env,
        cwd: Some(cwd),
        url: None,
        headers: HashMap::new(),
        disabled: false,
    })
}

/// Register all MiniApps under `codey/miniapps` as MCP servers.
/// Existing keys with the same name are not overwritten (plugins/config win).
pub fn list_miniapp_mcp_servers(workspace_config_dir: &Path) -> HashMap<String, McpServerConfig> {
    let Ok(apps) = load_registry(workspace_config_dir) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for app in apps {
        if let Some(server) = app_to_mcp_server(&app, Some(workspace_config_dir)) {
            map.entry(server.name.clone()).or_insert(server);
        }
    }
    map
}

/// Short runtime prompt block describing available MiniApps for the agent.
pub fn render_miniapp_runtime_prompt(workspace_config_dir: &Path) -> String {
    let Ok(apps) = load_registry(workspace_config_dir) else {
        return String::new();
    };

    let mut lines = Vec::new();
    lines.push(
        "## Local MiniApps (小程序)\n\
         MiniApps are general-purpose local Node.js applications with a Web UI and an MCP server.\n\
         They may implement any user-requested function; a database is optional and must never be assumed.\n\
         When the user asks to create or generate a MiniApp, build it under `codey/miniapps/<slug>`.\n\
         Every generated MiniApp must include a usable `web/` interface, `server/index.mjs`, `miniapp.json`, `.mcp.json`, `package.json`, and at least one page in `pages`. API-only or CLI-only output is incomplete.\n\
         Use database features only when a `databaseId` was explicitly selected. Never hardcode credentials.\n\
         Call via MCP: `mcp_call_tool` / `mcp_list_tools` with `server=<slug>`.\n\
         Required tools usually include: `list_pages`, `get_status`, `open_page` (+ business tools).\n\
         For `open_page`, host opens the returned URL in the browser panel when `ui.action=open_page`.\n\
         The host discovers valid manifests from disk and can start them from the MiniApps panel."
            .to_string(),
    );
    lines.push("Available MiniApps:".to_string());
    if apps.is_empty() {
        lines.push("- None yet. You may generate one when requested.".to_string());
    }
    for app in apps.iter().take(20) {
        let tools = if app.tools.is_empty() {
            "list_pages,get_status,open_page".to_string()
        } else {
            app.tools
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        let status = app.status.as_str();
        let db = if app.database_id.is_empty() {
            "none"
        } else if app.database_name.is_empty() {
            app.database_id.as_str()
        } else {
            app.database_name.as_str()
        };
        lines.push(format!(
            "- `{slug}` ({name}) status={status} db={db} tools=[{tools}]",
            slug = app.slug,
            name = app.name,
            status = status,
            db = db,
            tools = tools
        ));
    }
    if apps.len() > 20 {
        lines.push(format!("- ... and {} more", apps.len() - 20));
    }
    format!("\n\n{}\n", lines.join("\n"))
}

pub fn json_ok(data: Value) -> Value {
    serde_json::json!({
        "ok": true,
        "code": "OK",
        "data": data,
    })
}

pub fn json_err(code: &str, message: impl Into<String>) -> Value {
    serde_json::json!({
        "ok": false,
        "code": code,
        "message": message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::miniapp::scaffold::{ScaffoldRequest, scaffold_miniapp};

    #[test]
    fn slug_validation() {
        assert!(validate_slug("contract-app").is_ok());
        assert!(validate_slug("a1_b").is_ok());
        assert!(validate_slug("1bad").is_err());
        assert!(validate_slug("Bad").is_err());
        assert!(validate_slug("").is_err());
    }

    #[test]
    fn scaffold_creates_package() {
        let temp = tempfile::tempdir().unwrap();
        let codey = temp.path().join("codey");
        std::fs::create_dir_all(&codey).unwrap();
        let app = scaffold_miniapp(
            &codey,
            ScaffoldRequest {
                name: "合同录入".into(),
                slug: "contract-app".into(),
                description: "demo".into(),
                database_id: "db-1".into(),
                database_name: "合同库".into(),
            },
        )
        .unwrap();
        assert_eq!(app.slug, "contract-app");
        assert!(
            PathBuf::from(&app.root_path)
                .join("server/index.mjs")
                .exists()
        );
        assert!(
            PathBuf::from(&app.root_path)
                .join("web/index.html")
                .exists()
        );
        assert!(PathBuf::from(&app.root_path).join("miniapp.json").exists());
    }

    #[test]
    fn scaffold_supports_standalone_ui_without_database() {
        let temp = tempfile::tempdir().unwrap();
        let codey = temp.path().join("codey");
        std::fs::create_dir_all(&codey).unwrap();
        let app = scaffold_miniapp(
            &codey,
            ScaffoldRequest {
                name: "计时器".into(),
                slug: "focus-timer".into(),
                description: "带界面的专注计时器".into(),
                database_id: String::new(),
                database_name: String::new(),
            },
        )
        .unwrap();

        assert!(app.database_id.is_empty());
        let root = PathBuf::from(&app.root_path);
        let mcp = std::fs::read_to_string(root.join(".mcp.json")).unwrap();
        let html = std::fs::read_to_string(root.join("web/index.html")).unwrap();
        assert!(!mcp.contains("MINIAPP_DATABASE_ID"));
        assert!(html.contains("独立运行，不依赖数据库"));
    }

    #[test]
    fn registry_discovers_main_chain_generated_packages() {
        let temp = tempfile::tempdir().unwrap();
        let codey = temp.path().join("codey");
        std::fs::create_dir_all(&codey).unwrap();

        let registered = scaffold_miniapp(
            &codey,
            ScaffoldRequest {
                name: "已登记应用".into(),
                slug: "registered-app".into(),
                description: "existing".into(),
                database_id: String::new(),
                database_name: String::new(),
            },
        )
        .unwrap();
        save_registry(&codey, &[registered]).unwrap();

        scaffold_miniapp(
            &codey,
            ScaffoldRequest {
                name: "主链路生成应用".into(),
                slug: "agent-created-app".into(),
                description: "created directly on disk".into(),
                database_id: String::new(),
                database_name: String::new(),
            },
        )
        .unwrap();

        let apps = load_registry(&codey).unwrap();
        assert_eq!(apps.len(), 2);
        assert!(apps.iter().any(|app| app.slug == "agent-created-app"));
    }
}
