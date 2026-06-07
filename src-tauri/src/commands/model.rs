use tauri::State;

use crate::error::AppResult;
use crate::protocol::*;
use crate::state::AppState;

use super::thread::get_client;

#[tauri::command]
pub async fn model_list(
    state: State<'_, AppState>,
    params: ModelListParams,
) -> AppResult<ModelListResponse> {
    let client = get_client(&state).await?;
    let rpc = model_list_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}
