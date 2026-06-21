use tauri::State;

use crate::state::AppState;

use super::{WorkflowDef, WorkflowSummary};

/// Extract a workflow from a thread via LLM.
#[tauri::command]
pub async fn workflow_extract(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<WorkflowDef, String> {
    let config = state.config_manager.read().map_err(|e| e.to_string())?;
    let thread_store = state.thread_store.clone();
    let http = reqwest::Client::new();

    let def =
        super::extractor::extract_workflow_from_thread(&http, &config, &thread_store, &thread_id)
            .await?;

    Ok(def)
}

/// Save a workflow definition (after user confirmation/editing).
#[tauri::command]
pub async fn workflow_save(
    state: State<'_, AppState>,
    workflow: WorkflowDef,
) -> Result<String, String> {
    let workspace_config_dir = &state.workspace_config_dir;
    let path = super::save_workflow(workspace_config_dir, &workflow)?;
    Ok(path.to_string_lossy().to_string())
}

/// List all saved workflows.
#[tauri::command]
pub async fn workflow_list(state: State<'_, AppState>) -> Result<Vec<WorkflowSummary>, String> {
    let workspace_config_dir = &state.workspace_config_dir;
    Ok(super::list_workflows(workspace_config_dir))
}

/// Read a specific workflow by name.
#[tauri::command]
pub async fn workflow_read(
    state: State<'_, AppState>,
    name: String,
) -> Result<WorkflowDef, String> {
    let workspace_config_dir = &state.workspace_config_dir;
    let dir = super::workflows_dir(workspace_config_dir).join(&name);
    super::load_workflow(&dir).ok_or_else(|| format!("Workflow '{name}' not found"))
}

/// Delete a workflow by name.
#[tauri::command]
pub async fn workflow_delete(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let workspace_config_dir = &state.workspace_config_dir;
    super::delete_workflow(workspace_config_dir, &name)
}
