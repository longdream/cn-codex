use tauri::{AppHandle, State};
use tokio::sync::RwLock;
use tracing::info;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub struct StandaloneState {
    pub active: RwLock<bool>,
}

impl StandaloneState {
    pub fn new() -> Self {
        Self {
            active: RwLock::new(false),
        }
    }
}

#[tauri::command]
pub async fn standalone_init(state: State<'_, AppState>) -> AppResult<String> {
    let mut active = state.standalone.active.write().await;
    *active = true;
    info!("Standalone mode activated");
    Ok("standalone mode initialized".to_string())
}

#[tauri::command]
pub async fn standalone_config_read(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    Ok(serde_json::json!({
        "config": config.to_json(),
        "filePath": state.config_path.to_string_lossy(),
    }))
}

#[tauri::command]
pub async fn standalone_config_write(
    state: State<'_, AppState>,
    edits: Vec<serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let edit_pairs: Vec<(String, serde_json::Value)> = edits
        .iter()
        .filter_map(|edit| {
            let key = edit.get("keyPath")?.as_str()?.to_string();
            let value = edit.get("value")?.clone();
            Some((key, value))
        })
        .collect();

    state.config_manager.write(&edit_pairs)?;

    Ok(serde_json::json!({
        "status": "ok",
        "filePath": state.config_path.to_string_lossy(),
    }))
}

#[tauri::command]
pub async fn standalone_thread_create(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let thread = state
        .thread_store
        .create_thread(config.model.clone())
        .await?;

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
        }
    }))
}

#[tauri::command]
pub async fn standalone_thread_list(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let threads = state.thread_store.list_threads().await;
    let list: Vec<serde_json::Value> = threads
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "preview": t.preview(),
                "updatedAt": t.updated_at,
                "archived": false,
            })
        })
        .collect();

    Ok(serde_json::json!({ "data": list }))
}

#[tauri::command]
pub async fn standalone_thread_read(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    let thread = state
        .thread_store
        .get_thread(&thread_id)
        .await
        .ok_or_else(|| AppError::Custom(format!("Thread not found: {thread_id}")))?;

    let turns: Vec<serde_json::Value> = thread
        .turns
        .iter()
        .map(|turn| {
            let items: Vec<serde_json::Value> = turn
                .messages
                .iter()
                .filter_map(|m| {
                    match m.role.as_str() {
                        "user" => Some(serde_json::json!({
                            "type": "userMessage",
                            "id": m.id,
                            "text": m.content,
                            "content": [{ "type": "text", "text": m.content }],
                        })),
                        "assistant" if m.tool_calls.is_some() => {
                            let tcs = m.tool_calls.as_ref().unwrap();
                            Some(serde_json::json!({
                                "type": "toolUse",
                                "id": m.id,
                                "calls": tcs.iter().map(|tc| serde_json::json!({
                                    "id": tc.id,
                                    "name": tc.name,
                                    "arguments": tc.arguments,
                                })).collect::<Vec<_>>(),
                            }))
                        }
                        "assistant" => Some(serde_json::json!({
                            "type": "agentMessage",
                            "id": m.id,
                            "text": m.content,
                            "content": [{ "type": "text", "text": m.content }],
                        })),
                        "tool" => Some(serde_json::json!({
                            "type": "toolResult",
                            "id": m.id,
                            "text": m.content,
                            "toolName": m.tool_name,
                            "toolCallId": m.tool_call_id,
                        })),
                        _ => Some(serde_json::json!({
                            "type": "systemMessage",
                            "id": m.id,
                            "text": m.content,
                        })),
                    }
                })
                .collect();

            serde_json::json!({
                "id": turn.turn_id,
                "items": items,
                "startedAt": turn.started_at,
                "completedAt": turn.completed_at,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
            "name": thread.name,
            "turns": turns,
        }
    }))
}

#[tauri::command]
pub async fn standalone_chat(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    message: String,
    cwd: Option<String>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let override_cwd = cwd.map(std::path::PathBuf::from);

    state
        .agent_engine
        .run_turn(
            &app_handle,
            &config,
            &thread_id,
            &message,
            override_cwd.as_deref(),
        )
        .await?;

    Ok(serde_json::json!({ "status": "ok" }))
}
