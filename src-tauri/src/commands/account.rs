use tauri::State;

use crate::error::AppResult;
use crate::protocol::*;
use crate::state::AppState;

use super::thread::get_client;

#[tauri::command]
pub async fn account_read(
    state: State<'_, AppState>,
) -> AppResult<GetAccountResponse> {
    let client = get_client(&state).await?;
    let params = GetAccountParams { refresh_token: false };
    let rpc = get_account_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn account_login(
    state: State<'_, AppState>,
    params: LoginAccountParams,
) -> AppResult<LoginAccountResponse> {
    let client = get_client(&state).await?;
    let rpc = login_account_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn account_login_cancel(
    state: State<'_, AppState>,
    params: CancelLoginAccountParams,
) -> AppResult<CancelLoginAccountResponse> {
    let client = get_client(&state).await?;
    let rpc = cancel_login_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn account_logout(
    state: State<'_, AppState>,
) -> AppResult<LogoutAccountResponse> {
    let client = get_client(&state).await?;
    let rpc = logout_account_rpc();
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn account_rate_limits(
    state: State<'_, AppState>,
) -> AppResult<GetAccountRateLimitsResponse> {
    let client = get_client(&state).await?;
    let rpc = get_rate_limits_rpc();
    client.request_typed(rpc.method, rpc.params).await
}
