use tauri::{AppHandle, State};
use tokio::sync::RwLock;
use tracing::info;

use crate::agent::UserAttachment;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::thread_store::ThreadGoalStatus;

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
pub async fn standalone_config_read(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
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
pub async fn standalone_thread_create(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let thread = state
        .thread_store
        .create_thread(config.model.clone())
        .await?;
    *state.current_thread_id.write().await = Some(thread.id.clone());
    crate::mobile_server::broadcast(
        "active-thread-changed",
        serde_json::json!({ "threadId": thread.id }),
    );

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
        }
    }))
}

#[tauri::command]
pub async fn standalone_thread_list(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let threads = state.thread_store.list_threads().await;
    let list: Vec<serde_json::Value> = threads
        .iter()
        .filter(|t| !t.turns.is_empty())
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "preview": t.preview(),
                "updatedAt": t.updated_at,
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
    *state.current_thread_id.write().await = Some(thread_id.clone());
    crate::mobile_server::broadcast(
        "active-thread-changed",
        serde_json::json!({ "threadId": thread_id }),
    );

    let turns: Vec<serde_json::Value> = thread
        .turns
        .iter()
        .map(|turn| {
            let items: Vec<serde_json::Value> = turn
                .messages
                .iter()
                .filter_map(|m| match m.role.as_str() {
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
                })
                .collect();

            serde_json::json!({
                "id": turn.turn_id,
                "items": items,
                "startedAt": turn.started_at,
                "completedAt": turn.completed_at,
                "mode": turn.mode.clone(),
                "durationMs": turn.duration_ms,
                "changedFiles": turn.changed_files.clone(),
                "usage": turn.usage.clone(),
                "goalBudgetTokens": turn.goal_budget_tokens,
                "budgetLimited": turn.budget_limited,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "thread": {
            "id": thread.id,
            "name": thread.name,
            "goal": thread.goal,
            "turns": turns,
        }
    }))
}

#[tauri::command]
pub async fn standalone_thread_goal_set(
    state: State<'_, AppState>,
    thread_id: String,
    objective: String,
    status: Option<String>,
    goal_budget_tokens: Option<u64>,
) -> AppResult<serde_json::Value> {
    let status = parse_goal_status(status.as_deref())?.unwrap_or(ThreadGoalStatus::Active);
    let goal = state
        .thread_store
        .set_thread_goal(&thread_id, objective, status, goal_budget_tokens)
        .await?;

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_status(
    state: State<'_, AppState>,
    thread_id: String,
    status: String,
) -> AppResult<serde_json::Value> {
    let status = parse_goal_status(Some(status.as_str()))?
        .ok_or_else(|| AppError::Custom("Goal status is required".to_string()))?;
    let goal = state
        .thread_store
        .set_thread_goal_status(&thread_id, status)
        .await?;

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_edit(
    state: State<'_, AppState>,
    thread_id: String,
    objective: String,
    goal_budget_tokens: Option<u64>,
) -> AppResult<serde_json::Value> {
    let goal = state
        .thread_store
        .edit_thread_goal(&thread_id, objective, goal_budget_tokens)
        .await?;

    Ok(serde_json::json!({ "goal": goal }))
}

#[tauri::command]
pub async fn standalone_thread_goal_clear(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    state.thread_store.clear_thread_goal(&thread_id).await?;
    Ok(serde_json::json!({ "goal": serde_json::Value::Null }))
}

#[tauri::command]
pub async fn standalone_chat(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    message: String,
    attachments: Option<Vec<UserAttachment>>,
    cwd: Option<String>,
    mode: Option<String>,
    goal_budget_tokens: Option<u64>,
    robot_id: Option<String>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let override_cwd = cwd.map(std::path::PathBuf::from);
    let is_goal_mode = mode.as_deref() == Some("goal");
    let mode = mode.as_deref();
    // 接口层防护：仅在 goal 模式下向 agent 传递 robot_id，
    // 避免普通 chat 路径受到机器人编排逻辑影响。
    let robot_id_for_turn = resolve_robot_id_for_run_turn(mode, robot_id.as_deref());
    *state.current_thread_id.write().await = Some(thread_id.clone());

    let result = state
        .agent_engine
        .run_turn(
            &app_handle,
            &config,
            &thread_id,
            &message,
            attachments.unwrap_or_default(),
            override_cwd.as_deref(),
            mode,
            goal_budget_tokens,
            robot_id_for_turn,
        )
        .await;

    if let Err(ref err) = result {
        // Goal 模式下出错时将 goal 回退为 paused，避免前端状态卡死
        if is_goal_mode {
            info!("standalone_chat error in goal mode, reverting goal to paused: {err}");
            let _ = state
                .thread_store
                .set_thread_goal_status(&thread_id, ThreadGoalStatus::Paused)
                .await;
        }
    }

    result?;
    Ok(serde_json::json!({ "status": "ok" }))
}

fn resolve_robot_id_for_run_turn<'a>(
    mode: Option<&str>,
    robot_id: Option<&'a str>,
) -> Option<&'a str> {
    if matches!(mode, Some("goal" | "robot-modify")) {
        robot_id
    } else {
        None
    }
}

#[tauri::command]
pub async fn standalone_turn_interrupt(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    info!("Turn interrupt requested by user");
    state.agent_engine.interrupt();
    let current_thread_id = state.current_thread_id.read().await.clone();
    let interrupted_tools = state
        .agent_engine
        .interrupt_active_tools(current_thread_id.as_deref())
        .await;
    info!(
        "Turn interrupt completed: thread={:?}, interrupted_tools={interrupted_tools}",
        current_thread_id
    );
    Ok(serde_json::json!({ "status": "interrupted" }))
}

fn parse_goal_status(value: Option<&str>) -> AppResult<Option<ThreadGoalStatus>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let status = match value.to_ascii_lowercase().as_str() {
        "active" => ThreadGoalStatus::Active,
        "paused" | "pause" => ThreadGoalStatus::Paused,
        "blocked" => ThreadGoalStatus::Blocked,
        "usage_limited" | "usage-limited" | "usagelimited" => ThreadGoalStatus::UsageLimited,
        "budget_limited" | "budget-limited" | "budgetlimited" => ThreadGoalStatus::BudgetLimited,
        "complete" | "completed" => ThreadGoalStatus::Complete,
        other => {
            return Err(AppError::Custom(format!(
                "Unsupported goal status '{other}'"
            )));
        }
    };

    Ok(Some(status))
}

#[cfg(test)]
mod tests {
    use super::resolve_robot_id_for_run_turn;

    #[test]
    fn resolve_robot_id_for_run_turn_enables_in_goal_and_robot_modify_modes() {
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("chat"), Some("robot-a")),
            None
        );
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("robot-create"), Some("robot-a")),
            None
        );
        assert_eq!(resolve_robot_id_for_run_turn(Some("goal"), None), None);
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("goal"), Some("robot-a")),
            Some("robot-a")
        );
        assert_eq!(
            resolve_robot_id_for_run_turn(Some("robot-modify"), Some("robot-a")),
            Some("robot-a")
        );
    }
}
