pub mod adapter;
pub mod agent;
pub mod browser_automation;
pub mod commands;
pub mod config_system;
pub mod document_parser;
pub mod error;
pub mod hook_runtime;
pub mod plugin_loader;
pub mod protocol;
pub mod standalone;
pub mod state;
pub mod thread_store;
pub mod tool_executor;
pub mod usage;

use state::AppState;

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
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                use tauri::Manager;
                if let Some(webview) = _app.get_webview_window("main") {
                    webview.open_devtools();
                }
            }
            Ok(())
        })
        .manage(AppState::new())
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
            commands::window_open_browser,
            commands::window_resize_browser,
            commands::window_navigate_browser,
            commands::window_close_browser,
            commands::reveal_in_explorer,
            commands::window_toggle_devtools,
            commands::get_user_home_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CN-Codex");
}
