use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::error::AppError;
use crate::jsonrpc_client::{JsonRpcClient, ServerEvent};
use crate::protocol::notification_event_name;
use crate::state::{AppState, ApprovalAction};

pub async fn initialize_app_server(
    app_handle: AppHandle,
    state: &AppState,
) -> Result<(), AppError> {
    let codex_exe = std::env::var("CN_CODEX_EXE").ok();
    let shared_client = Arc::new(
        JsonRpcClient::start(
            codex_exe,
            state.project_root.clone(),
            state.workspace_config_dir.clone(),
        )
        .await?,
    );

    {
        let mut client_lock = state.client.write().await;
        *client_lock = Some(shared_client.clone());
    }

    let approval_rx = {
        let mut rx_lock = state.approval_rx.write().await;
        rx_lock.take()
    };

    info!("App server initialized successfully");

    spawn_event_loop(app_handle, shared_client.clone(), shared_client, approval_rx);

    Ok(())
}

fn spawn_event_loop(
    app_handle: AppHandle,
    event_client: Arc<JsonRpcClient>,
    request_client: Arc<JsonRpcClient>,
    approval_rx: Option<mpsc::Receiver<ApprovalAction>>,
) {
    tokio::spawn(async move {
        let mut approval_rx = approval_rx;

        loop {
            tokio::select! {
                event = event_client.next_event() => {
                    match event {
                        Some(ev) => dispatch_server_event(&app_handle, ev),
                        None => {
                            info!("App server event stream ended");
                            break;
                        }
                    }
                }

                action = async {
                    if let Some(rx) = approval_rx.as_mut() {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match action {
                        Some(ApprovalAction::Resolve { request_id, result }) => {
                            if let Err(e) = request_client.resolve_server_request(request_id, result).await {
                                error!("Failed to resolve approval: {e}");
                            }
                        }
                        Some(ApprovalAction::Reject { request_id, error }) => {
                            if let Err(e) = request_client.reject_server_request(request_id, error).await {
                                error!("Failed to reject approval: {e}");
                            }
                        }
                        None => {
                            approval_rx = None;
                        }
                    }
                }
            }
        }
    });
}

fn dispatch_server_event(app_handle: &AppHandle, event: ServerEvent) {
    match event {
        ServerEvent::Notification { method, params } => {
            let event_name = notification_event_name(&method);
            if let Some(payload) = params {
                let _ = app_handle.emit(event_name, payload);
            }
        }
        ServerEvent::ServerRequest { id, method, params } => {
            let payload = serde_json::json!({
                "id": id,
                "method": method,
                "params": params,
            });
            let _ = app_handle.emit("server-request", payload);
        }
    }
}
