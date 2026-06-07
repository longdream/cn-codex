pub mod error;
pub mod state;
pub mod commands;
pub mod llm_tiers;
pub mod protocol;
pub mod jsonrpc_client;
pub mod standalone;
pub mod config_system;
pub mod thread_store;
pub mod agent;
pub mod tool_executor;
mod server_bridge;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;
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
        .setup(|app| {
            #[cfg(debug_assertions)]
            if let Some(webview) = app.get_webview_window("main") {
                webview.open_devtools();
            }
            Ok(())
        })
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::initialize_server,
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
            // LLM Tiers
            commands::llm_get_tiers,
            commands::llm_set_tier,
            commands::llm_resolve_tier,
            commands::llm_get_usage,
            commands::llm_record_usage,
            commands::llm_bind_scene,
            // Skills
            commands::skill_list,
            commands::skill_read,
            // Standalone mode
            standalone::standalone_init,
            standalone::standalone_config_read,
            standalone::standalone_config_write,
            standalone::standalone_thread_create,
            standalone::standalone_thread_list,
            standalone::standalone_thread_read,
            standalone::standalone_chat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CN-Codex");
}
