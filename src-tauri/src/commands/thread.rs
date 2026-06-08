use tauri::State;

use crate::error::{AppError, AppResult};
use crate::protocol::*;
use crate::state::AppState;

/// 这些命令是旧 codex 桥接模式的遗留接口。
/// 在 standalone 模式下，前端使用 standalone_* 命令，这些命令不会被调用。
/// 保留注册以防前端有残留调用时给出明确错误提示。
fn not_supported() -> AppError {
    AppError::Custom("This command is not available in standalone mode".to_string())
}

#[tauri::command]
pub async fn thread_start(
    state: State<'_, AppState>,
    params: ThreadStartParams,
) -> AppResult<ThreadStartResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_resume(
    state: State<'_, AppState>,
    params: ThreadResumeParams,
) -> AppResult<ThreadResumeResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    params: ThreadListParams,
) -> AppResult<ThreadListResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_read(
    state: State<'_, AppState>,
    params: ThreadReadParams,
) -> AppResult<ThreadReadResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_archive(
    state: State<'_, AppState>,
    params: ThreadArchiveParams,
) -> AppResult<ThreadArchiveResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_unarchive(
    state: State<'_, AppState>,
    params: ThreadUnarchiveParams,
) -> AppResult<ThreadUnarchiveResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_set_name(
    state: State<'_, AppState>,
    params: ThreadSetNameParams,
) -> AppResult<ThreadSetNameResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_rollback(
    state: State<'_, AppState>,
    params: ThreadRollbackParams,
) -> AppResult<ThreadRollbackResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn thread_unsubscribe(
    state: State<'_, AppState>,
    params: ThreadUnsubscribeParams,
) -> AppResult<ThreadUnsubscribeResponse> {
    let _ = (state, params);
    Err(not_supported())
}
