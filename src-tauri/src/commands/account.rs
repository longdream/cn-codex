use tauri::State;

use crate::error::{AppError, AppResult};
use crate::protocol::*;
use crate::state::AppState;

/// 旧 codex 桥接模式的遗留接口，standalone 模式下不可用。
fn not_supported() -> AppError {
    AppError::Custom("This command is not available in standalone mode".to_string())
}

#[tauri::command]
pub async fn account_read(
    state: State<'_, AppState>,
) -> AppResult<GetAccountResponse> {
    let _ = state;
    Err(not_supported())
}

#[tauri::command]
pub async fn account_login(
    state: State<'_, AppState>,
    params: LoginAccountParams,
) -> AppResult<LoginAccountResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn account_login_cancel(
    state: State<'_, AppState>,
    params: CancelLoginAccountParams,
) -> AppResult<CancelLoginAccountResponse> {
    let _ = (state, params);
    Err(not_supported())
}

#[tauri::command]
pub async fn account_logout(
    state: State<'_, AppState>,
) -> AppResult<LogoutAccountResponse> {
    let _ = state;
    Err(not_supported())
}

#[tauri::command]
pub async fn account_rate_limits(
    state: State<'_, AppState>,
) -> AppResult<GetAccountRateLimitsResponse> {
    let _ = state;
    Err(not_supported())
}
