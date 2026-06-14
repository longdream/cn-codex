use std::sync::Arc;

use tauri::{AppHandle, State};
use tokio::sync::broadcast;

use crate::mobile_server;
use crate::state::AppState;
use crate::{MOBILE_SERVER, MobileServerInfo};

#[tauri::command]
pub async fn start_mobile_server(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    tracing::info!("[mobile] start_mobile_server called");

    if MOBILE_SERVER.get().is_some() {
        let ip = mobile_server::get_local_ip();
        let port = MOBILE_SERVER.get().unwrap().port;
        tracing::info!("[mobile] already running on port {port}");
        return Ok(format!("http://{ip}:{port}"));
    }

    let (broadcast_tx, _) = broadcast::channel::<mobile_server::BroadcastEvent>(256);

    let thread_store = state.thread_store.clone();
    let current_thread_id = state.current_thread_id.clone();
    let agent_engine = state.agent_engine.clone();
    let config_manager = state.config_manager.clone();
    let static_dir = state.project_root.join("mobile-dist");

    let _ = std::fs::create_dir_all(&static_dir);
    tracing::info!("[mobile] static_dir={}", static_dir.display());

    let mobile_state = Arc::new(mobile_server::MobileServerState {
        broadcast_tx: broadcast_tx.clone(),
        thread_store,
        current_thread_id,
        app_handle,
        agent_engine,
        config_manager,
    });

    tracing::info!("[mobile] spawning server...");
    let tx_for_set = broadcast_tx.clone();

    let result = tauri::async_runtime::spawn(async move {
        mobile_server::start(mobile_state, static_dir, 19527).await
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
    .map_err(|e| format!("启动失败: {e}"))?;

    tracing::info!("[mobile] server started on port {result}");

    let _ = MOBILE_SERVER.set(MobileServerInfo {
        port: result,
        broadcast_tx: tx_for_set,
    });

    let ip = mobile_server::get_local_ip();
    Ok(format!("http://{ip}:{result}"))
}

#[tauri::command]
pub fn stop_mobile_server() -> Result<(), String> {
    // OnceLock 无法 reset，服务会保持到进程退出。
    // 实际效果：标记为"已停止"即可，前端不再显示为可用。
    // 如需真正停止需要用 tokio CancellationToken，当前简化处理。
    Ok(())
}

#[tauri::command]
pub fn get_mobile_server_status() -> Result<bool, String> {
    Ok(MOBILE_SERVER.get().is_some())
}

#[tauri::command]
pub fn get_mobile_server_url() -> Result<String, String> {
    let info = MOBILE_SERVER.get().ok_or("Mobile server not started")?;
    let ip = mobile_server::get_local_ip();
    Ok(format!("http://{ip}:{}", info.port))
}

#[tauri::command]
pub fn get_qrcode_svg() -> Result<String, String> {
    let url = get_mobile_server_url()?;
    mobile_server::generate_qrcode_svg(&url)
}
