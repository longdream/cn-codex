pub mod adapter;
pub mod agent;
pub mod browser_automation;
pub mod commands;
pub mod compaction;
pub mod config_system;
pub mod conversation_logger;
pub mod document_parser;
pub mod error;
pub mod hook_runtime;
pub mod mobile_server;
pub mod relay_client;
pub mod plugin_loader;
pub mod robot_loader;
pub mod robot_orchestrator;
pub mod protocol;
pub mod standalone;
pub mod state;
pub mod terminal;
pub mod thread_store;
pub mod tool_executor;
pub mod usage;
pub mod wps_protocol;
pub mod wps_server;

use state::AppState;
use tauri::{Emitter, Manager};

use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// 移动端服务器运行时信息
pub struct MobileServerInfo {
    pub port: u16,
    pub broadcast_tx: broadcast::Sender<mobile_server::BroadcastEvent>,
}

pub static MOBILE_SERVER: std::sync::OnceLock<MobileServerInfo> = std::sync::OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cn_codex_lib=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::new())
        .manage(std::sync::Arc::new(terminal::TerminalManager::new()))
        .setup(|app| {
            // 启动后台预热：把线程索引加载放到异步任务，避免阻塞窗口出现。
            let preload_store = app.state::<AppState>().thread_store.clone();
            tauri::async_runtime::spawn(async move {
                let preload_started_at = Instant::now();
                preload_store.preload_threads().await;
                info!(
                    "[startup][rust] thread_store_preload task finished in {} ms",
                    preload_started_at.elapsed().as_millis()
                );
            });

            // 兜底保护：若前端未及时发送“显示主窗口”请求，8 秒后强制显示一次，
            // 避免极端异常导致窗口永久隐藏（可恢复性优先于完美无闪屏）。
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(8)).await;
                if let Some(main_window) = app_handle.get_webview_window("main") {
                    match main_window.is_visible() {
                        Ok(true) => {}
                        Ok(false) => {
                            if let Err(show_err) = main_window.show() {
                                warn!("[startup][rust] fallback show main window failed: {show_err}");
                            } else {
                                warn!("[startup][rust] fallback showed main window after timeout");
                            }
                        }
                        Err(visible_err) => {
                            warn!(
                                "[startup][rust] failed to query main window visibility: {visible_err}"
                            );
                        }
                    }
                }
            });

            // Spawn WPS event forwarding task.
            let wps_server = app.state::<AppState>().wps_server.clone();
            let wps_app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut rx = match wps_server.take_event_receiver().await {
                    Some(rx) => rx,
                    None => return,
                };
                while let Some(event) = rx.recv().await {
                    let (event_name, payload) = match event {
                        wps_server::WpsEvent::Connected { conn_id, addin_name } => (
                            "wps-connected",
                            serde_json::json!({ "connId": conn_id, "addinName": addin_name }),
                        ),
                        wps_server::WpsEvent::Disconnected { conn_id } => (
                            "wps-disconnected",
                            serde_json::json!({ "connId": conn_id }),
                        ),
                        wps_server::WpsEvent::DocumentChanged { conn_id, document } => (
                            "wps-document-changed",
                            serde_json::json!({ "connId": conn_id, "document": document }),
                        ),
                        wps_server::WpsEvent::Notification { conn_id, method, params } => (
                            "wps-notification",
                            serde_json::json!({ "connId": conn_id, "method": method, "params": params }),
                        ),
                    };
                    let _ = wps_app_handle.emit(event_name, payload);
                }
            });

            #[cfg(debug_assertions)]
            {
                if let Some(webview) = app.get_webview_window("main") {
                    webview.open_devtools();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_server_status,
            // Thread management
            commands::thread_start,
            commands::thread_resume,
            commands::thread_list,
            commands::thread_read,
            commands::thread_archive,
            commands::thread_unarchive,
            commands::thread_set_name,
            commands::thread_rollback,
            commands::thread_unsubscribe,
            // Turn / conversation
            commands::turn_start,
            commands::turn_steer,
            commands::turn_interrupt,
            // Config
            commands::config_read,
            commands::config_value_write,
            commands::config_batch_write,
            commands::hook_list,
            // Account
            commands::account_read,
            commands::account_login,
            commands::account_login_cancel,
            commands::account_logout,
            commands::account_rate_limits,
            // Model
            commands::model_list,
            // Approval
            commands::resolve_approval,
            commands::reject_approval,
            // Skills
            commands::skill_list,
            commands::skill_read,
            // Plugins
            commands::plugin_list,
            commands::plugin_read,
            commands::plugin_set_enabled,
            commands::plugin_uninstall,
            commands::plugin_import_codex_cache,
            // Robots
            commands::robot_list,
            commands::robot_read,
            commands::robot_delete,
            // App State (SQLite KV)
            commands::app_state_get,
            commands::app_state_set,
            commands::app_state_delete,
            commands::app_state_get_all,
            // Standalone mode
            standalone::standalone_init,
            standalone::standalone_config_read,
            standalone::standalone_config_write,
            standalone::standalone_thread_create,
            standalone::standalone_thread_list,
            standalone::standalone_thread_read,
            standalone::standalone_thread_goal_set,
            standalone::standalone_thread_goal_status,
            standalone::standalone_thread_goal_edit,
            standalone::standalone_thread_goal_clear,
            standalone::standalone_chat,
            standalone::standalone_turn_interrupt,
            // Usage tracking
            commands::usage_get_stats,
            commands::usage_get_daily,
            commands::usage_get_by_model,
            commands::usage_get_recent,
            commands::usage_set_pricing,
            commands::usage_get_pricing,
            // Window controls
            commands::window_start_dragging,
            commands::window_minimize,
            commands::window_toggle_maximize,
            commands::window_close,
            commands::window_show_main,
            commands::window_open_browser,
            commands::window_resize_browser,
            commands::window_navigate_browser,
            commands::window_close_browser,
            commands::reveal_in_explorer,
            commands::window_toggle_devtools,
            commands::get_user_home_dir,
            commands::read_directory,
            commands::read_file_for_attach,
            commands::read_text_file_preview,
            // Terminal
            terminal::terminal_create,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            // Mobile server
            commands::start_mobile_server,
            commands::stop_mobile_server,
            commands::get_mobile_server_status,
            commands::get_mobile_server_url,
            commands::get_qrcode_svg,
            // WPS server
            commands::wps_start_server,
            commands::wps_stop_server,
            commands::wps_status,
            commands::wps_execute,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CN-Codex");
}
