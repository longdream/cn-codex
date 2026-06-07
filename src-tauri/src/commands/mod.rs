pub mod account;
pub mod approval;
pub mod config;
pub mod llm;
pub mod model;
pub mod skill;
pub mod thread;
pub mod turn;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::server_bridge::initialize_app_server;
use crate::state::AppState;

pub use account::*;
pub use approval::*;
pub use config::*;
pub use llm::*;
pub use model::*;
pub use skill::*;
pub use thread::*;
pub use turn::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
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

#[tauri::command]
pub async fn initialize_server(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<String> {
    if state.client.read().await.is_some() {
        return Ok("server already initialized".to_string());
    }

    initialize_app_server(app_handle, state.inner()).await?;
    Ok("server initialized".to_string())
}

#[tauri::command]
pub async fn get_server_status(state: State<'_, AppState>) -> AppResult<ServerStatus> {
    Ok(ServerStatus {
        initialized: state.client.read().await.is_some(),
        current_thread_id: state.current_thread_id.read().await.clone(),
        cwd: state.cwd.read().await.clone(),
        locale: state.locale.read().await.clone(),
        config_dir: state.workspace_config_dir.to_string_lossy().to_string(),
        config_path: state.config_path.to_string_lossy().to_string(),
    })
}
