use tauri::State;

use crate::error::{AppError, AppResult};
use crate::protocol::{JSONRPCErrorError, RequestId};
use crate::state::{AppState, ApprovalAction};

#[tauri::command]
pub async fn resolve_approval(
    state: State<'_, AppState>,
    request_id: RequestId,
    result: serde_json::Value,
) -> AppResult<()> {
    state
        .approval_tx
        .send(ApprovalAction::Resolve { request_id, result })
        .await
        .map_err(|e| AppError::Custom(format!("Approval channel closed: {e}")))
}

#[tauri::command]
pub async fn reject_approval(
    state: State<'_, AppState>,
    request_id: RequestId,
    code: i64,
    message: String,
) -> AppResult<()> {
    let error = JSONRPCErrorError {
        code,
        message,
        data: None,
    };
    state
        .approval_tx
        .send(ApprovalAction::Reject { request_id, error })
        .await
        .map_err(|e| AppError::Custom(format!("Approval channel closed: {e}")))
}
