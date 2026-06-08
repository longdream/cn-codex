use tauri::State;

use crate::error::{AppError, AppResult};
use crate::protocol::*;
use crate::state::AppState;

/// 旧 codex 桥接模式的遗留接口，standalone 模式下不可用。
fn not_supported() -> AppError {
    AppError::Custom("This command is not available in standalone mode".to_string())
}

#[tauri::command]
pub async fn config_read(
    state: State<'_, AppState>,
    params: ConfigReadParams,
) -> AppResult<ConfigReadResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn config_value_write(
    state: State<'_, AppState>,
    params: ConfigValueWriteParams,
) -> AppResult<ConfigWriteResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn config_batch_write(
    state: State<'_, AppState>,
    params: ConfigBatchWriteParams,
) -> AppResult<ConfigWriteResponse> {
    let _ = (state, params);
    Err(not_supported())
}
