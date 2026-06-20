use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::file_review::{
    PendingPatchReview, ReviewApplyRequest, apply_review_to_workspace, pending_review_key,
    update_review_file,
};
use crate::state::AppState;
use crate::thread_store::FileChange;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReviewActionResponse {
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub changed_files: Vec<FileChange>,
}

fn emit_review_updated(app: &AppHandle, payload: serde_json::Value) {
    app.emit("file-review-updated", payload.clone()).ok();
    crate::mobile_server::broadcast("file-review-updated", payload);
}

#[tauri::command]
pub async fn file_review_get(
    state: State<'_, AppState>,
    thread_id: String,
    call_id: String,
) -> AppResult<PendingPatchReview> {
    let key = pending_review_key(&thread_id, &call_id);
    let sessions = state.file_review_sessions.read().await;
    let Some(review) = sessions.get(&key) else {
        return Err(AppError::Custom(format!(
            "pending review not found for thread={} call={}",
            thread_id, call_id
        )));
    };
    Ok(review.clone())
}

#[tauri::command]
pub async fn file_review_update(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    call_id: String,
    path: String,
    keep: Option<bool>,
    edited_content: Option<String>,
) -> AppResult<PendingPatchReview> {
    let key = pending_review_key(&thread_id, &call_id);
    let updated = {
        let mut sessions = state.file_review_sessions.write().await;
        let Some(review) = sessions.get_mut(&key) else {
            return Err(AppError::Custom(format!(
                "pending review not found for thread={} call={}",
                thread_id, call_id
            )));
        };
        update_review_file(review, &path, keep, edited_content).map_err(AppError::Custom)?;
        review.clone()
    };

    emit_review_updated(
        &app_handle,
        serde_json::json!({
            "threadId": thread_id,
            "callId": call_id,
            "status": "updated",
        }),
    );
    Ok(updated)
}

#[tauri::command]
pub async fn file_review_apply(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    call_id: String,
    keep_all: Option<bool>,
    keep_paths: Option<Vec<String>>,
) -> AppResult<FileReviewActionResponse> {
    let key = pending_review_key(&thread_id, &call_id);
    let review = {
        let sessions = state.file_review_sessions.read().await;
        let Some(review) = sessions.get(&key) else {
            return Err(AppError::Custom(format!(
                "pending review not found for thread={} call={}",
                thread_id, call_id
            )));
        };
        review.clone()
    };

    // 使用当前工作目录作为 apply 目标根目录，保证命令层与工具执行层行为一致。
    let cwd = PathBuf::from(state.cwd.read().await.clone());
    let apply_result = apply_review_to_workspace(
        &cwd,
        &review,
        &ReviewApplyRequest {
            keep_all: keep_all.unwrap_or(false),
            keep_paths: keep_paths.unwrap_or_default(),
        },
    )
    .map_err(AppError::Custom)?;

    {
        let mut sessions = state.file_review_sessions.write().await;
        sessions.remove(&key);
    }

    emit_review_updated(
        &app_handle,
        serde_json::json!({
            "threadId": thread_id,
            "callId": call_id,
            "status": "applied",
            "changedFiles": apply_result.changed_files,
        }),
    );

    Ok(FileReviewActionResponse {
        ok: true,
        message: apply_result.message,
        changed_files: apply_result.changed_files,
    })
}

#[tauri::command]
pub async fn file_review_cancel(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    call_id: String,
) -> AppResult<FileReviewActionResponse> {
    let key = pending_review_key(&thread_id, &call_id);
    let removed = {
        let mut sessions = state.file_review_sessions.write().await;
        sessions.remove(&key)
    };
    if removed.is_none() {
        return Err(AppError::Custom(format!(
            "pending review not found for thread={} call={}",
            thread_id, call_id
        )));
    }

    emit_review_updated(
        &app_handle,
        serde_json::json!({
            "threadId": thread_id,
            "callId": call_id,
            "status": "cancelled",
        }),
    );

    Ok(FileReviewActionResponse {
        ok: true,
        message: "Review cancelled.".to_string(),
        changed_files: Vec::new(),
    })
}
