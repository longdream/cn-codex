use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmToolDef {
    pub id: &'static str,
    pub package: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub check_command: &'static str,
    pub requires_node: &'static str,
    pub extra_deps: &'static [&'static str],
    pub homepage: &'static str,
}

const TOOL_REGISTRY: &[NpmToolDef] = &[NpmToolDef {
    id: "hyperframes",
    package: "hyperframes",
    display_name: "HyperFrames",
    description: "HTML-to-Video rendering framework",
    check_command: "hyperframes",
    requires_node: "22",
    extra_deps: &["ffmpeg"],
    homepage: "https://github.com/heygen-com/hyperframes",
}];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmToolInfo {
    pub id: String,
    pub package: String,
    pub display_name: String,
    pub description: String,
    pub homepage: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub node_available: bool,
    pub node_version: Option<String>,
    pub missing_deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmToolResult {
    pub success: bool,
    pub output: String,
}

async fn run_command_output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program).args(args).output().await.ok()?;
    if result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
        if stdout.is_empty() {
            None
        } else {
            Some(stdout)
        }
    } else {
        None
    }
}

async fn get_node_version() -> Option<String> {
    let raw = run_command_output("node", &["--version"]).await?;
    Some(raw.trim_start_matches('v').to_string())
}

fn parse_major_version(version: &str) -> Option<u32> {
    version.split('.').next()?.parse().ok()
}

async fn check_command_exists(name: &str) -> bool {
    let program = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    run_command_output(program, &[name]).await.is_some()
}

async fn get_npm_package_version(package: &str) -> Option<String> {
    let args = ["list", "-g", package, "--depth=0", "--json"];
    let result = Command::new("npm").args(&args).output().await.ok()?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).ok()?;
    json.get("dependencies")?
        .get(package)?
        .get("version")?
        .as_str()
        .map(ToString::to_string)
}

async fn check_single_tool(def: &NpmToolDef) -> NpmToolInfo {
    let node_version = get_node_version().await;
    let node_available = node_version
        .as_ref()
        .and_then(|v| parse_major_version(v))
        .is_some_and(|major| major >= def.requires_node.parse::<u32>().unwrap_or(0));

    let mut missing_deps = Vec::new();
    if !node_available {
        missing_deps.push(format!("Node.js >= {}", def.requires_node));
    }
    for dep in def.extra_deps {
        if !check_command_exists(dep).await {
            missing_deps.push((*dep).to_string());
        }
    }

    let installed_version = get_npm_package_version(def.package).await;
    let installed = installed_version.is_some()
        || check_command_exists(def.check_command).await;

    NpmToolInfo {
        id: def.id.to_string(),
        package: def.package.to_string(),
        display_name: def.display_name.to_string(),
        description: def.description.to_string(),
        homepage: def.homepage.to_string(),
        installed,
        installed_version,
        node_available,
        node_version,
        missing_deps,
    }
}

#[tauri::command]
pub async fn npm_tool_list() -> AppResult<Vec<NpmToolInfo>> {
    let mut results = Vec::new();
    for def in TOOL_REGISTRY {
        results.push(check_single_tool(def).await);
    }
    Ok(results)
}

#[tauri::command]
pub async fn npm_tool_check(tool_id: String) -> AppResult<NpmToolInfo> {
    let def = TOOL_REGISTRY
        .iter()
        .find(|d| d.id == tool_id)
        .ok_or_else(|| AppError::Custom(format!("Unknown npm tool: {tool_id}")))?;
    Ok(check_single_tool(def).await)
}

#[tauri::command]
pub async fn npm_tool_install(tool_id: String) -> AppResult<NpmToolResult> {
    let def = TOOL_REGISTRY
        .iter()
        .find(|d| d.id == tool_id)
        .ok_or_else(|| AppError::Custom(format!("Unknown npm tool: {tool_id}")))?;

    let node_version = get_node_version().await;
    let node_ok = node_version
        .as_ref()
        .and_then(|v| parse_major_version(v))
        .is_some_and(|major| major >= def.requires_node.parse::<u32>().unwrap_or(0));

    if !node_ok {
        return Ok(NpmToolResult {
            success: false,
            output: format!(
                "Node.js >= {} is required but {}",
                def.requires_node,
                match node_version {
                    Some(v) => format!("found v{v}"),
                    None => "not found".to_string(),
                }
            ),
        });
    }

    let result = Command::new("npm")
        .args(["install", "-g", def.package])
        .output()
        .await;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            };
            Ok(NpmToolResult {
                success: output.status.success(),
                output: combined,
            })
        }
        Err(e) => Ok(NpmToolResult {
            success: false,
            output: format!("Failed to run npm: {e}"),
        }),
    }
}

#[tauri::command]
pub async fn npm_tool_uninstall(tool_id: String) -> AppResult<NpmToolResult> {
    let def = TOOL_REGISTRY
        .iter()
        .find(|d| d.id == tool_id)
        .ok_or_else(|| AppError::Custom(format!("Unknown npm tool: {tool_id}")))?;

    let result = Command::new("npm")
        .args(["uninstall", "-g", def.package])
        .output()
        .await;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            };
            Ok(NpmToolResult {
                success: output.status.success(),
                output: combined,
            })
        }
        Err(e) => Ok(NpmToolResult {
            success: false,
            output: format!("Failed to run npm: {e}"),
        }),
    }
}
