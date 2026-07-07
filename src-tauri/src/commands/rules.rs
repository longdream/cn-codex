use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

const USER_RULES_FILE: &str = "user-rules.md";

#[tauri::command]
pub async fn rules_read(state: State<'_, AppState>) -> AppResult<String> {
    let path = state.workspace_config_dir.join(USER_RULES_FILE);
    Ok(std::fs::read_to_string(&path).unwrap_or_default())
}

#[tauri::command]
pub async fn rules_write(state: State<'_, AppState>, content: String) -> AppResult<()> {
    let path = state.workspace_config_dir.join(USER_RULES_FILE);
    std::fs::write(&path, content)?;
    Ok(())
}
