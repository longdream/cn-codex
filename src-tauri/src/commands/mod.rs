pub mod app_state;
pub mod approval;
pub mod file_review;
pub mod git;
pub mod hook;
pub mod mobile;
pub mod plugin;
pub mod recording;
pub mod robot;
pub mod rules;
pub mod skill;
pub mod skill_lab;
pub mod usage;
pub mod window;

use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::protocol::ThreadArchiveParams;
use crate::state::AppState;

pub use app_state::*;
pub use approval::*;
pub use file_review::*;
pub use git::*;
pub use hook::*;
pub use mobile::*;
pub use plugin::*;
pub use recording::*;
pub use robot::*;
pub use rules::*;
pub use skill::*;
pub use skill_lab::*;
pub use usage::*;
pub use window::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    /// standalone 模式始终为 true
    pub initialized: bool,
    pub current_thread_id: Option<String>,
    pub cwd: String,
    pub locale: String,
    pub config_dir: String,
    pub config_path: String,
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("CN-Codex ready for {name}.")
}

/// 接收前端错误日志，写入后端 tracing 日志文件。
#[tauri::command]
pub fn frontend_log(level: String, message: String) {
    match level.as_str() {
        "error" => tracing::error!("[frontend] {message}"),
        "warn" => tracing::warn!("[frontend] {message}"),
        _ => tracing::info!("[frontend] {message}"),
    }
}

/// 返回日志文件目录路径，供前端展示和打开。
#[tauri::command]
pub fn get_log_dir() -> String {
    normalize_windows_verbatim_prefix(&crate::resolve_log_dir().to_string_lossy())
}

pub(crate) fn normalize_windows_verbatim_prefix(raw: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
        return trimmed.to_string();
    }

    #[cfg(not(target_os = "windows"))]
    {
        raw.trim().to_string()
    }
}

#[tauri::command]
pub async fn thread_archive(
    state: State<'_, AppState>,
    params: ThreadArchiveParams,
) -> AppResult<serde_json::Value> {
    state.thread_store.delete_thread(&params.thread_id).await?;
    Ok(serde_json::json!({ "status": "ok" }))
}

#[tauri::command]
pub async fn get_server_status(state: State<'_, AppState>) -> AppResult<ServerStatus> {
    let cwd = state.cwd.read().await.clone();
    Ok(ServerStatus {
        initialized: true,
        current_thread_id: state.current_thread_id.read().await.clone(),
        // 将 Windows 扩展前缀路径转换为常规显示路径，避免前端直接看到 `\\?\`。
        cwd: normalize_windows_verbatim_prefix(&cwd),
        locale: state.locale.read().await.clone(),
        config_dir: normalize_windows_verbatim_prefix(
            &state.workspace_config_dir.to_string_lossy(),
        ),
        config_path: normalize_windows_verbatim_prefix(&state.config_path.to_string_lossy()),
    })
}
