use tauri::State;

use crate::error::AppResult;
use crate::hook_runtime::HookListItem;
use crate::state::AppState;

#[tauri::command]
pub async fn hook_list(state: State<'_, AppState>) -> AppResult<Vec<HookListItem>> {
    let config = state.config_manager.read()?;
    Ok(crate::hook_runtime::list_hooks(
        &config,
        &state.workspace_config_dir,
    ))
}
