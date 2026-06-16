use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::robot_loader::{self, RobotDetail, RobotSummary};
use crate::state::AppState;

#[tauri::command]
pub async fn robot_list(state: State<'_, AppState>) -> AppResult<Vec<RobotSummary>> {
    Ok(robot_loader::list_robots(&state.workspace_config_dir))
}

#[tauri::command]
pub async fn robot_read(
    state: State<'_, AppState>,
    robot_id: String,
) -> AppResult<RobotDetail> {
    robot_loader::read_robot(&state.workspace_config_dir, &robot_id)
        .ok_or_else(|| AppError::Custom(format!("Robot not found: {robot_id}")))
}

#[tauri::command]
pub async fn robot_delete(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    robot_id: String,
) -> AppResult<()> {
    robot_loader::delete_robot(&state.workspace_config_dir, &robot_id)
        .map_err(AppError::Custom)?;
    app_handle
        .emit(
            "robot-deleted",
            serde_json::json!({
                "robotId": robot_id,
            }),
        )
        .ok();
    Ok(())
}
