use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::AppResult;
use crate::recording::{RecordingStatus, TraceFile, TraceListEntry};
use crate::replay::{ReplayReadResult, ReplayRunResult, ReplayScriptMeta};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchBrowserResult {
    pub cdp_endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStartResult {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatusResult {
    pub status: RecordingStatus,
    pub browser_running: bool,
}

/// Launch an external Chrome/Edge browser with CDP enabled.
#[tauri::command]
pub async fn launch_browser(
    state: State<'_, AppState>,
    browser_path: Option<String>,
    cdp_port: Option<u16>,
) -> AppResult<LaunchBrowserResult> {
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let cdp_endpoint = state
        .external_browser
        .launch(&http, browser_path.as_deref(), cdp_port)
        .await
        .map_err(|e| crate::error::AppError::Custom(e))?;

    Ok(LaunchBrowserResult { cdp_endpoint })
}

/// Close the external browser.
#[tauri::command]
pub async fn close_external_browser(state: State<'_, AppState>) -> AppResult<()> {
    state.external_browser.shutdown().await;
    Ok(())
}

/// Start recording user actions in the external browser.
#[tauri::command]
pub async fn recording_start(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    session_name: Option<String>,
) -> AppResult<RecordingStartResult> {
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let session_id = state
        .recorder
        .start_recording(
            session_name.as_deref().unwrap_or(""),
            &state.external_browser,
            &http,
        )
        .await
        .map_err(|e| crate::error::AppError::Custom(e))?;

    app_handle.emit("recording-started", &session_id).ok();

    Ok(RecordingStartResult { session_id })
}

/// Stop recording and save the trace file.
#[tauri::command]
pub async fn recording_stop(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> AppResult<TraceFile> {
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let recordings_dir = state.workspace_config_dir.join("recordings");

    let trace = state
        .recorder
        .stop_recording(&http, &recordings_dir)
        .await
        .map_err(|e| crate::error::AppError::Custom(e))?;

    // Automatically turn the recording into a Playwright Python script so the
    // user can replay it right away. Best-effort: a generation failure must not
    // break the recording flow itself.
    if let Err(e) = crate::replay::generate_and_save(&recordings_dir, &trace).await {
        tracing::warn!("Failed to generate replay script for {}: {e}", trace.session_id);
    }

    app_handle.emit("recording-completed", &trace).ok();

    Ok(trace)
}

/// Get current recording status.
#[tauri::command]
pub async fn recording_status(state: State<'_, AppState>) -> AppResult<RecordingStatusResult> {
    let status = state.recorder.get_status().await;

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_default();
    let browser_running = state.external_browser.is_running(&http).await;

    Ok(RecordingStatusResult {
        status,
        browser_running,
    })
}

/// Show or hide the recording toggle in the frontend.
#[tauri::command]
pub async fn recording_show_toggle(app_handle: AppHandle, visible: bool) -> AppResult<()> {
    app_handle.emit("recording-toggle-visibility", visible).ok();
    Ok(())
}

/// List saved recording traces.
#[tauri::command]
pub async fn recording_list_traces(state: State<'_, AppState>) -> AppResult<Vec<TraceListEntry>> {
    let recordings_dir = state.workspace_config_dir.join("recordings");
    if !recordings_dir.exists() {
        return Ok(Vec::new());
    }
    crate::recording::Recorder::list_traces(&recordings_dir)
        .await
        .map_err(|e| crate::error::AppError::Custom(e))
}

/// Read a specific trace file by session ID.
#[tauri::command]
pub async fn recording_read_trace(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<TraceFile> {
    let trace_path = state
        .workspace_config_dir
        .join("recordings")
        .join(format!("{session_id}.trace.json"));

    let content = tokio::fs::read_to_string(&trace_path)
        .await
        .map_err(|e| crate::error::AppError::Custom(format!("Failed to read trace: {e}")))?;

    serde_json::from_str(&content)
        .map_err(|e| crate::error::AppError::Custom(format!("Failed to parse trace: {e}")))
}

/// Regenerate a Playwright Python script from an existing recording trace.
#[tauri::command]
pub async fn replay_generate_script(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<ReplayScriptMeta> {
    let recordings_dir = state.workspace_config_dir.join("recordings");
    let trace_path = recordings_dir.join(format!("{session_id}.trace.json"));
    let content = tokio::fs::read_to_string(&trace_path)
        .await
        .map_err(|e| crate::error::AppError::Custom(format!("Failed to read trace: {e}")))?;
    let trace: TraceFile = serde_json::from_str(&content)
        .map_err(|e| crate::error::AppError::Custom(format!("Failed to parse trace: {e}")))?;

    crate::replay::generate_and_save(&recordings_dir, &trace)
        .await
        .map_err(crate::error::AppError::Custom)
}

/// List all saved replay scripts.
#[tauri::command]
pub async fn replay_list_scripts(state: State<'_, AppState>) -> AppResult<Vec<ReplayScriptMeta>> {
    let recordings_dir = state.workspace_config_dir.join("recordings");
    crate::replay::list_scripts(&recordings_dir)
        .await
        .map_err(crate::error::AppError::Custom)
}

/// Read a saved replay script by id.
#[tauri::command]
pub async fn replay_read_script(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<ReplayReadResult> {
    let recordings_dir = state.workspace_config_dir.join("recordings");
    crate::replay::read_script(&recordings_dir, &id)
        .await
        .map_err(crate::error::AppError::Custom)
}

/// Run a saved replay script and return structured output.
#[tauri::command]
pub async fn replay_run_script(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<ReplayRunResult> {
    let recordings_dir = state.workspace_config_dir.join("recordings");
    crate::replay::run_script(&recordings_dir, &id)
        .await
        .map_err(crate::error::AppError::Custom)
}

/// Return the directory where replay scripts are stored (used to bind the
/// dedicated repair conversation's working directory).
#[tauri::command]
pub async fn replay_get_dir(state: State<'_, AppState>) -> AppResult<String> {
    let dir = crate::replay::scripts_dir(&state.workspace_config_dir.join("recordings"));
    let _ = tokio::fs::create_dir_all(&dir).await;
    Ok(crate::commands::normalize_windows_verbatim_prefix(
        &dir.to_string_lossy(),
    ))
}
