use tauri::State;

use crate::error::AppResult;
use crate::protocol::*;
use crate::state::AppState;

use super::thread::get_client;

#[tauri::command]
pub async fn turn_start(
    state: State<'_, AppState>,
    params: TurnStartParams,
) -> AppResult<TurnStartResponse> {
    let client = get_client(&state).await?;
    let rpc = turn_start_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn turn_steer(
    state: State<'_, AppState>,
    params: TurnSteerParams,
) -> AppResult<TurnSteerResponse> {
    let client = get_client(&state).await?;
    let rpc = turn_steer_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn turn_interrupt(
    state: State<'_, AppState>,
    params: TurnInterruptParams,
) -> AppResult<TurnInterruptResponse> {
    let client = get_client(&state).await?;
    let rpc = turn_interrupt_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}
