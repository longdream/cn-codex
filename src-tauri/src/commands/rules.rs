use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

const USER_RULES_FILE: &str = "user-rules.md";
const PROJECT_RULES_FILE: &str = ".rule.md";

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

#[tauri::command]
pub async fn rules_read_project(project_path: String) -> AppResult<String> {
    let path = std::path::Path::new(&project_path).join(PROJECT_RULES_FILE);
    Ok(std::fs::read_to_string(&path).unwrap_or_default())
}

#[tauri::command]
pub async fn rules_write_project(project_path: String, content: String) -> AppResult<()> {
    let path = std::path::Path::new(&project_path).join(PROJECT_RULES_FILE);
    std::fs::write(&path, content)?;
    Ok(())
}
