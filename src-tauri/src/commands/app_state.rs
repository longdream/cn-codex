use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateEntry {
    pub key: String,
    pub value: String,
}

#[tauri::command]
pub async fn app_state_get(state: State<'_, AppState>, key: String) -> AppResult<Option<String>> {
    state.usage_db.state_get(&key)
}

#[tauri::command]
pub async fn app_state_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> AppResult<()> {
    state.usage_db.state_set(&key, &value)
}

#[tauri::command]
pub async fn app_state_delete(state: State<'_, AppState>, key: String) -> AppResult<()> {
    state.usage_db.state_delete(&key)
}

#[tauri::command]
pub async fn app_state_get_all(state: State<'_, AppState>) -> AppResult<HashMap<String, String>> {
    let pairs = state.usage_db.state_get_all()?;
    Ok(pairs.into_iter().collect())
}
