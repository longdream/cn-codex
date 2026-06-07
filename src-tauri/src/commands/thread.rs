use std::sync::Arc;

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::jsonrpc_client::JsonRpcClient;
use crate::protocol::*;
use crate::state::AppState;

pub(super) async fn get_client(state: &AppState) -> AppResult<Arc<JsonRpcClient>> {
    state
        .client
        .read()
        .await
        .clone()
        .ok_or(AppError::NotInitialized)
}

#[tauri::command]
pub async fn thread_start(
    state: State<'_, AppState>,
    params: ThreadStartParams,
) -> AppResult<ThreadStartResponse> {
    let mut params = params;
    if params.cwd.as_deref().map(str::trim).unwrap_or("").is_empty() {
        params.cwd = Some(state.cwd.read().await.clone());
    }

    let client = get_client(&state).await?;
    let rpc = thread_start_rpc(&params);
    let resp: ThreadStartResponse = client.request_typed(rpc.method, rpc.params).await?;

    {
        let mut tid = state.current_thread_id.write().await;
        *tid = Some(resp.thread.id.clone());
    }

    Ok(resp)
}

#[tauri::command]
pub async fn thread_resume(
    state: State<'_, AppState>,
    params: ThreadResumeParams,
) -> AppResult<ThreadResumeResponse> {
    let client = get_client(&state).await?;
    let thread_id = params.thread_id.clone();
    let rpc = thread_resume_rpc(&params);
    let resp: ThreadResumeResponse = client.request_typed(rpc.method, rpc.params).await?;

    {
        let mut current = state.current_thread_id.write().await;
        *current = Some(thread_id);
    }

    Ok(resp)
}

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    params: ThreadListParams,
) -> AppResult<ThreadListResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_list_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn thread_read(
    state: State<'_, AppState>,
    params: ThreadReadParams,
) -> AppResult<ThreadReadResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_read_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn thread_archive(
    state: State<'_, AppState>,
    params: ThreadArchiveParams,
) -> AppResult<ThreadArchiveResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_archive_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn thread_unarchive(
    state: State<'_, AppState>,
    params: ThreadUnarchiveParams,
) -> AppResult<ThreadUnarchiveResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_unarchive_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn thread_set_name(
    state: State<'_, AppState>,
    params: ThreadSetNameParams,
) -> AppResult<ThreadSetNameResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_set_name_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn thread_rollback(
    state: State<'_, AppState>,
    params: ThreadRollbackParams,
) -> AppResult<ThreadRollbackResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_rollback_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}

#[tauri::command]
pub async fn thread_unsubscribe(
    state: State<'_, AppState>,
    params: ThreadUnsubscribeParams,
) -> AppResult<ThreadUnsubscribeResponse> {
    let client = get_client(&state).await?;
    let rpc = thread_unsubscribe_rpc(&params);
    client.request_typed(rpc.method, rpc.params).await
}
