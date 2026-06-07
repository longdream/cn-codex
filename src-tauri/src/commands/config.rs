use tauri::State;

use crate::error::AppResult;
use crate::protocol::*;
use crate::state::AppState;

use super::thread::get_client;

#[tauri::command]
pub async fn config_read(
    state: State<'_, AppState>,
    params: ConfigReadParams,
) -> AppResult<ConfigReadResponse> {
    let mut params = params;
    if params.cwd.as_deref().map(str::trim).unwrap_or("").is_empty() {
        params.cwd = Some(state.cwd.read().await.clone());
    }

    let client = get_client(&state).await?;
    let rpc = config_read_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn config_value_write(
    state: State<'_, AppState>,
    params: ConfigValueWriteParams,
) -> AppResult<ConfigWriteResponse> {
    let mut params = params;
    if params.file_path.as_deref().map(str::trim).unwrap_or("").is_empty() {
        params.file_path = Some(state.config_path.to_string_lossy().to_string());
    }

    let client = get_client(&state).await?;
    let rpc = config_value_write_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn config_batch_write(
    state: State<'_, AppState>,
    params: ConfigBatchWriteParams,
) -> AppResult<ConfigWriteResponse> {
    let mut params = params;
    if params.file_path.as_deref().map(str::trim).unwrap_or("").is_empty() {
        params.file_path = Some(state.config_path.to_string_lossy().to_string());
    }

    let client = get_client(&state).await?;
    let rpc = config_batch_write_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}
