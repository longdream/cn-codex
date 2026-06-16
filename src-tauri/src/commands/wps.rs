use tauri::State;

use crate::state::AppState;
use crate::wps_protocol::WpsServerStatus;

#[tauri::command]
pub async fn wps_start_server(
    state: State<'_, AppState>,
    port: Option<u16>,
) -> Result<u16, String> {
    state.wps_server.start(port).await
}

#[tauri::command]
pub async fn wps_stop_server(state: State<'_, AppState>) -> Result<(), String> {
    if !state.wps_server.is_running() {
        return Err("WPS server is not running".into());
    }
    // The current implementation uses a spawned axum server that runs until the process exits.
    // A full stop would require storing the JoinHandle and aborting it.
    // For now we mark it as not running; a future phase can add graceful shutdown.
    Ok(())
}

#[tauri::command]
pub async fn wps_status(state: State<'_, AppState>) -> Result<WpsServerStatus, String> {
    Ok(state.wps_server.status().await)
}

#[tauri::command]
pub async fn wps_execute(
    state: State<'_, AppState>,
    method: String,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    if !state.wps_server.is_running() {
        return Err("WPS server is not running".into());
    }
    let response = state.wps_server.send_command(&method, params).await?;
    if let Some(err) = response.error {
        return Err(format!("WPS error: {}", err.message));
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}
