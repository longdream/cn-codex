use std::sync::Arc;

use tauri::{AppHandle, State};
use tokio::sync::broadcast;

use crate::mobile_server;
use crate::state::AppState;
use crate::{MOBILE_SERVER, MobileServerInfo};

/// relay 模式的运行时信息
static RELAY_INFO: std::sync::LazyLock<std::sync::Mutex<Option<RelayInfo>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

struct RelayInfo {
    relay_url: String,
    room_id: String,
    relay_abort: tokio::task::AbortHandle,
}

/// 统一规范 relay 基础地址，避免配置里末尾 `/` 导致 `//m/...`、`//pc/...` 这类路径错误。
/// 返回 None 代表输入为空或仅包含空白，调用方可按“未配置 relay”处理。
fn normalize_relay_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches('/');
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

/// 统一拼装手机端访问地址，避免各处手写 `format!("{}/m/{}")` 再次引入双斜杠问题。
fn build_relay_mobile_url(relay_base_url: &str, room_id: &str) -> String {
    format!("{relay_base_url}/m/{room_id}")
}

#[tauri::command]
pub async fn start_mobile_server(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    tracing::info!("[mobile] start_mobile_server called");

    // 如果已经有 relay 信息，直接返回 relay URL
    if let Ok(guard) = RELAY_INFO.lock() {
        if let Some(relay_info) = guard.as_ref() {
            return Ok(build_relay_mobile_url(
                &relay_info.relay_url,
                &relay_info.room_id,
            ));
        }
    }

    if let Ok(guard) = MOBILE_SERVER.lock() {
        if let Some(info) = guard.as_ref() {
            // 已启动本地服务
            let ip = mobile_server::get_local_ip();
            let port = info.port;
            tracing::info!("[mobile] already running on port {port}");
            return Ok(format!("http://{ip}:{port}"));
        }
    }

    let (broadcast_tx, _) = broadcast::channel::<mobile_server::BroadcastEvent>(256);

    let thread_store = state.thread_store.clone();
    let current_thread_id = state.current_thread_id.clone();
    let agent_engine = state.agent_engine.clone();
    let config_manager = state.config_manager.clone();
    let static_dir = state.project_root.join("mobile-dist");
    tracing::info!("[mobile] static_dir={}", static_dir.display());

    // 检查 mobile-dist/index.html 是否存在，不存在则报错而非静默 404
    if !static_dir.join("index.html").is_file() {
        let msg = format!(
            "mobile-dist/index.html 不存在: {}。请确保发布包中包含 mobile-dist 目录。",
            static_dir.display()
        );
        tracing::error!("[mobile] {msg}");
        return Err(msg);
    }

    let mobile_state = Arc::new(mobile_server::MobileServerState {
        broadcast_tx: broadcast_tx.clone(),
        thread_store,
        current_thread_id,
        app_handle: app_handle.clone(),
        agent_engine,
        config_manager: config_manager.clone(),
    });

    tracing::info!("[mobile] spawning server...");
    let tx_for_set = broadcast_tx.clone();

    let (result, server_abort) = tauri::async_runtime::spawn(async move {
        mobile_server::start(mobile_state.clone(), static_dir, 19527).await
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
    .map_err(|e| format!("启动失败: {e}"))?;

    tracing::info!("[mobile] server started on port {result}");

    if let Ok(mut guard) = MOBILE_SERVER.lock() {
        *guard = Some(MobileServerInfo {
            port: result,
            broadcast_tx: tx_for_set,
            server_abort,
        });
    }

    // 检查是否配置了 relay server
    let relay_url = config_manager
        .read()
        .ok()
        .and_then(|c| c.relay_server_url.clone())
        .and_then(|u| normalize_relay_base_url(&u));

    if let Some(relay_url) = relay_url {
        let room_id = uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string();
        tracing::info!("[mobile] relay mode: url={relay_url}, room_id={room_id}");

        // 获取 mobile_state 再启动 relay client
        let mobile_info = MOBILE_SERVER
            .lock()
            .map_err(|_| "Mobile server state lock poisoned".to_string())?;
        let mobile_info = mobile_info
            .as_ref()
            .ok_or_else(|| "Mobile server not started".to_string())?;
        let relay_mobile_state = Arc::new(mobile_server::MobileServerState {
            broadcast_tx: mobile_info.broadcast_tx.clone(),
            thread_store: state.thread_store.clone(),
            current_thread_id: state.current_thread_id.clone(),
            app_handle,
            agent_engine: state.agent_engine.clone(),
            config_manager,
        });

        let relay_abort = crate::relay_client::start_relay_client(
            relay_url.clone(),
            room_id.clone(),
            relay_mobile_state,
        );

        if let Ok(mut guard) = RELAY_INFO.lock() {
            *guard = Some(RelayInfo {
                relay_url: relay_url.clone(),
                room_id: room_id.clone(),
                relay_abort,
            });
        }

        return Ok(build_relay_mobile_url(&relay_url, &room_id));
    }

    let ip = mobile_server::get_local_ip();
    Ok(format!("http://{ip}:{result}"))
}

#[tauri::command]
pub fn stop_mobile_server() -> Result<(), String> {
    if let Ok(mut guard) = RELAY_INFO.lock() {
        if let Some(relay_info) = guard.take() {
            relay_info.relay_abort.abort();
        }
    }
    if let Ok(mut guard) = MOBILE_SERVER.lock() {
        if let Some(server_info) = guard.take() {
            server_info.server_abort.abort();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_mobile_server_status() -> Result<bool, String> {
    Ok(MOBILE_SERVER
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false))
}

#[tauri::command]
pub fn get_mobile_server_url() -> Result<String, String> {
    // 优先返回 relay URL
    if let Ok(guard) = RELAY_INFO.lock() {
        if let Some(relay_info) = guard.as_ref() {
            return Ok(build_relay_mobile_url(
                &relay_info.relay_url,
                &relay_info.room_id,
            ));
        }
    }

    let guard = MOBILE_SERVER
        .lock()
        .map_err(|_| "Mobile server state lock poisoned")?;
    let info = guard.as_ref().ok_or("Mobile server not started")?;
    let ip = mobile_server::get_local_ip();
    Ok(format!("http://{ip}:{}", info.port))
}

#[tauri::command]
pub fn get_qrcode_svg() -> Result<String, String> {
    let url = get_mobile_server_url()?;
    mobile_server::generate_qrcode_svg(&url)
}
