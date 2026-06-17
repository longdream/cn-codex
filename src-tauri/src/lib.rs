pub mod adapter;
pub mod agent;
pub mod browser_automation;
pub mod commands;
pub mod compaction;
pub mod config_system;
pub mod conversation_logger;
pub mod document_parser;
pub mod error;
pub mod experience;
pub mod smartbrain;
pub mod file_review;
pub mod git_service;
pub mod hook_runtime;
pub mod mobile_server;
pub mod plugin_loader;
pub mod protocol;
pub mod relay_client;
pub mod robot_loader;
pub mod robot_orchestrator;
pub mod standalone;
pub mod state;
pub mod terminal;
pub mod thread_store;
pub mod tool_executor;
pub mod usage;

use state::AppState;
use tauri::Manager;

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

            // 启动后台 SmartBrain 流水线：经验提取/合并 + 知识扫描 + BM25 索引。
            {
                let sb_state = app.state::<AppState>();
                let workspace_config_dir = sb_state.workspace_config_dir.clone();
                let config_manager = sb_state.config_manager.clone();
                let thread_store = sb_state.thread_store.clone();
                tauri::async_runtime::spawn(async move {
                    thread_store.preload_threads().await;

                    let config = match config_manager.read() {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let sb_config = config.smartbrain_config();
                    if !sb_config.is_active() {
                        return;
                    }

                    let experiences_dir = smartbrain::experiences_dir(&workspace_config_dir);
                    let knowledge_dir = smartbrain::knowledge_dir(&workspace_config_dir);
                    let bm25_path = smartbrain::bm25_index_path(&workspace_config_dir);
                    let _ = std::fs::create_dir_all(experiences_dir.join("raw"));
                    let _ = std::fs::create_dir_all(knowledge_dir.join("sources"));
                    let _ = std::fs::create_dir_all(knowledge_dir.join("docs"));

                    let http = reqwest::Client::builder()
                        .connect_timeout(Duration::from_secs(30))
                        .read_timeout(Duration::from_secs(300))
                        .build()
                        .unwrap_or_default();

                    smartbrain::extractor::run_extraction(
                        &http,
                        &config,
                        &thread_store,
                        &experiences_dir,
                    )
                    .await;

                    smartbrain::consolidator::run_consolidation(
                        &http,
                        &config,
                        &experiences_dir,
                    )
                    .await;

                    smartbrain::knowledge::scan_and_ingest_new(
                        &http,
                        &config,
                        &knowledge_dir,
                        &bm25_path,
                    )
                    .await;

                    smartbrain::search::rebuild_index(&workspace_config_dir, &bm25_path);

                    info!("[startup][rust] smartbrain pipeline completed");
                });
            }

            // 兜底保护：若前端未及时发送"显示主窗口"请求，8 秒后强制显示一次，
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
            commands::window_open_document_detail,
            commands::window_close_document_detail,
            commands::window_get_document_detail_path,
            commands::window_open_runsummary_diff,
            commands::window_close_runsummary_diff,
            commands::window_get_runsummary_diff_payload,
            commands::document_detail_insert_snippet,
            commands::reveal_in_explorer,
            commands::window_toggle_devtools,
            commands::get_user_home_dir,
            commands::read_directory,
            commands::read_file_for_attach,
            commands::read_text_file_preview,
            commands::write_text_file_preview,
            // Git panel commands
            commands::git_status,
            commands::git_diff,
            commands::git_log,
            commands::git_branch_list,
            commands::git_stage,
            commands::git_unstage,
            commands::git_commit,
            commands::git_checkout,
            commands::git_pull,
            commands::git_push,
            commands::git_reset,
            commands::git_revert,
            commands::git_cherry_pick,
            // File review (pre-apply gate)
            commands::file_review_get,
            commands::file_review_update,
            commands::file_review_apply,
            commands::file_review_cancel,
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
            // SmartBrain
            smartbrain::commands::smartbrain_list_experiences,
            smartbrain::commands::smartbrain_read_experience,
            smartbrain::commands::smartbrain_delete_experience,
            smartbrain::commands::smartbrain_list_knowledge,
            smartbrain::commands::smartbrain_read_knowledge,
            smartbrain::commands::smartbrain_delete_knowledge,
            smartbrain::commands::smartbrain_upload_knowledge,
            smartbrain::commands::smartbrain_search,
            smartbrain::commands::smartbrain_rebuild_index,
            // WPS server
        ])
        .run(tauri::generate_context!())
        .expect("error while running CN-Codex");
}
