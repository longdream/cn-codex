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
pub mod external_browser;
pub mod file_review;
pub mod git_service;
pub mod hook_runtime;
pub mod lan_collab;
pub mod local_pool;
pub mod mobile_server;
pub mod ocr;
pub mod plugin_loader;
pub mod protocol;
pub mod recording;
pub mod relay_client;
mod request_control;
pub mod robot_loader;
pub mod robot_orchestrator;
pub mod smartbrain;
pub mod standalone;
pub mod state;
pub mod subagent_engine;
pub mod terminal;
pub mod thread_store;
pub mod tool_executor;
pub mod usage;
pub mod utf8_stream;
pub mod workflow;

use state::AppState;
use tauri::Manager;

use std::time::{Duration, Instant};
#[cfg(all(target_os = "windows", not(debug_assertions)))]
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// 移动端服务器运行时信息
pub struct MobileServerInfo {
    pub port: u16,
    pub broadcast_tx: broadcast::Sender<mobile_server::BroadcastEvent>,
}

pub static MOBILE_SERVER: std::sync::OnceLock<MobileServerInfo> = std::sync::OnceLock::new();

#[cfg(all(target_os = "windows", not(debug_assertions)))]
const WEBVIEW2_RUNTIME_VERSION: &str = include_str!("../../release/webview2-runtime.version");

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn parse_webview2_runtime_version() -> Result<String, String> {
    let version = WEBVIEW2_RUNTIME_VERSION.trim();
    if version.is_empty() {
        return Err(
            "release/webview2-runtime.version is empty; cannot resolve bundled WebView2 runtime."
                .to_string(),
        );
    }
    Ok(version.to_string())
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn resolve_fixed_webview2_runtime_dir(exe_dir: &Path, version: &str) -> Option<PathBuf> {
    let runtime_root = exe_dir.join("webview2-fixed-runtime");
    let candidate_dirs = [
        runtime_root.join(version),
        runtime_root.join(format!(
            "Microsoft.WebView2.FixedVersionRuntime.{version}.x64"
        )),
    ];
    candidate_dirs
        .into_iter()
        .find(|candidate| candidate.is_dir() && candidate.join("msedgewebview2.exe").is_file())
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn fixed_runtime_acl_marker_path(runtime_dir: &Path, version: &str) -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let mut hasher = DefaultHasher::new();
    runtime_dir.to_string_lossy().hash(&mut hasher);
    let runtime_hash = hasher.finish();
    Some(
        PathBuf::from(local_app_data)
            .join("CN-Codex")
            .join("webview2-acl")
            .join(format!("{version}-{runtime_hash:016x}.ok")),
    )
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn ensure_fixed_runtime_acl(runtime_dir: &Path, version: &str) -> Result<(), String> {
    let major = version
        .split('.')
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    if major < 120 {
        return Ok(());
    }

    let marker_path = fixed_runtime_acl_marker_path(runtime_dir, version);
    if let Some(marker_path) = marker_path.as_ref() {
        if marker_path.is_file() {
            info!(
                "[startup][rust] fixed WebView2 ACL already ensured, skipping icacls ({})",
                marker_path.display()
            );
            return Ok(());
        }
    }

    let acl_started_at = Instant::now();
    for sid in ["*S-1-15-2-2", "*S-1-15-2-1"] {
        let grant = format!("{sid}:(OI)(CI)(RX)");
        let output = Command::new("icacls")
            .arg(runtime_dir)
            .arg("/grant")
            .arg(&grant)
            .output()
            .map_err(|error| {
                format!("Failed to run icacls for fixed WebView2 runtime ACL setup: {error}")
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(format!(
                "icacls failed while granting '{grant}' on '{}'. stdout: {stdout}; stderr: {stderr}",
                runtime_dir.to_string_lossy()
            ));
        }
    }

    if let Some(marker_path) = marker_path.as_ref() {
        if let Some(parent) = marker_path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            warn!(
                "[startup][rust] failed to create WebView2 ACL marker directory '{}': {}",
                parent.display(),
                error
            );
        }
        if let Err(error) = std::fs::write(
            marker_path,
            format!(
                "version={version}\nruntime_dir={}\n",
                runtime_dir.to_string_lossy()
            ),
        ) {
            warn!(
                "[startup][rust] failed to write WebView2 ACL marker '{}': {}",
                marker_path.display(),
                error
            );
        }
    }

    info!(
        "[startup][rust] fixed WebView2 ACL ensured in {} ms",
        acl_started_at.elapsed().as_millis()
    );
    Ok(())
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn configure_bundled_webview2_runtime() -> Result<bool, String> {
    let configured_started_at = Instant::now();
    let version = parse_webview2_runtime_version()?;
    let exe_path = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    let exe_dir = exe_path.parent().ok_or_else(|| {
        format!(
            "Failed to resolve executable parent directory: {}",
            exe_path.display()
        )
    })?;
    let Some(runtime_dir) = resolve_fixed_webview2_runtime_dir(exe_dir, &version) else {
        info!(
            "[startup][rust] bundled fixed WebView2 runtime not found (version {}, exe: {}), fallback to system runtime",
            version,
            exe_path.display()
        );
        info!(
            "[startup][rust] bundled WebView2 runtime configure finished in {} ms",
            configured_started_at.elapsed().as_millis()
        );
        return Ok(false);
    };

    ensure_fixed_runtime_acl(&runtime_dir, &version)?;
    // SAFETY: process-wide environment is set before any webview is created.
    unsafe {
        std::env::set_var(
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            runtime_dir.as_os_str(),
        );
    }
    info!(
        "[startup][rust] using bundled WebView2 runtime {} at {}",
        version,
        runtime_dir.display()
    );
    info!(
        "[startup][rust] bundled WebView2 runtime configure finished in {} ms",
        configured_started_at.elapsed().as_millis()
    );
    Ok(true)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 初始化日志系统。
/// - debug 模式：输出到 stderr
/// - release 模式：同时输出到 stderr 和日志文件（可执行文件同级 logs/ 目录，按天滚动）
///
/// 返回 guard（持有文件 writer），调用方必须在 main 中 hold 住直到程序退出。
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "cn_codex_lib=info,warn".into());

    // release 模式下同时写日志文件
    if cfg!(not(debug_assertions)) {
        let log_dir = resolve_log_dir();
        let _ = std::fs::create_dir_all(&log_dir);
        let file_appender = tracing_appender::rolling::daily(&log_dir, "cn-codex.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(non_blocking),
            )
            .init();

        // 写一条启动标记方便定位
        tracing::info!(
            "=== CN-Codex started (release) === log_dir={}",
            log_dir.display()
        );
        return Some(guard);
    }

    // debug 模式只输出到 stderr
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    None
}

/// 日志目录：可执行文件同级的 logs/，或 fallback 到 TEMP/cn-codex-logs/
pub fn resolve_log_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("logs");
        }
    }
    std::env::temp_dir().join("cn-codex-logs")
}

pub fn run() {
    let _log_guard = init_tracing();

    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    let webview2_config_started_at = Instant::now();

    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    match configure_bundled_webview2_runtime() {
        Ok(true) => {}
        Ok(false) => {
            info!("[startup][rust] using system WebView2 runtime");
        }
        Err(error) => {
            warn!(
                "[startup][rust] failed to configure bundled WebView2 runtime: {}; fallback to system runtime",
                error
            );
        }
    }
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    info!(
        "[startup][rust] webview2_runtime_config done in {} ms",
        webview2_config_started_at.elapsed().as_millis()
    );

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
                let app_handle = app.handle().clone();
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
                    let knowledge_sources_dir =
                        smartbrain::knowledge_sources_dir(&workspace_config_dir);
                    let bm25_path = smartbrain::bm25_index_path(&workspace_config_dir);
                    let _ = std::fs::create_dir_all(experiences_dir.join("raw"));
                    let _ = std::fs::create_dir_all(knowledge_sources_dir);
                    let _ = std::fs::create_dir_all(knowledge_dir.join("docs"));

                    let http = reqwest::Client::builder()
                        .connect_timeout(Duration::from_secs(30))
                        .read_timeout(Duration::from_secs(300))
                        .build()
                        .unwrap_or_default();

                    smartbrain::extractor::run_extraction_backfill(
                        &http,
                        &config,
                        &thread_store,
                        &experiences_dir,
                        Some(&app_handle),
                    )
                    .await;

                    smartbrain::consolidator::run_consolidation(
                        &http,
                        &config,
                        &experiences_dir,
                    )
                    .await;

                    // Auto-summarize: when experiences exceed the configured threshold,
                    // categorize and merge them into fewer consolidated entries.
                    if sb_config.auto_summarize_enabled {
                        let exp_index = smartbrain::index::ExperienceIndex::load(&experiences_dir);
                        let exp_count = exp_index.entries.len();
                        if exp_count >= sb_config.auto_summarize_threshold {
                            info!(
                                "[startup][rust] auto-summarize triggered: {} experiences >= threshold {}",
                                exp_count, sb_config.auto_summarize_threshold
                            );
                            smartbrain::summarizer::run_summarize_merge(
                                &http,
                                &config,
                                &experiences_dir,
                                Some(&bm25_path),
                            )
                            .await;
                        }
                    }


                    smartbrain::knowledge::scan_and_ingest_new(
                        &http,
                        &config,
                        &knowledge_dir,
                        &bm25_path,
                    )
                    .await;

                    smartbrain::search::rebuild_index(&workspace_config_dir, &bm25_path);
                    smartbrain::regenerate_root_index_md(&workspace_config_dir);

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
            commands::get_log_dir,
            commands::frontend_log,
            // Thread archive (standalone)
            commands::thread_archive,
            commands::hook_list,
            // Approval
            commands::resolve_approval,
            commands::reject_approval,
            // Skills
            commands::skill_list,
            commands::skill_categories_read,
            commands::skill_read,
            // Skill Lab
            commands::skill_lab_list,
            commands::skill_lab_read,
            commands::skill_lab_save,
            commands::skill_lab_check_python_env,
            commands::skill_lab_generate_from_goal,
            commands::skill_lab_update_result,
            commands::skill_lab_deploy,
            commands::skill_lab_delete,
            commands::skill_lab_run_test,
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
            // Rules
            commands::rules_read,
            commands::rules_write,
            // App State (SQLite KV)
            commands::app_state_get,
            commands::app_state_set,
            commands::app_state_delete,
            commands::app_state_get_all,
            // Standalone mode
            standalone::standalone_init,
            standalone::standalone_config_read,
            standalone::standalone_config_write,
            standalone::standalone_mcp_enable_playwright,
            standalone::standalone_smartbrain_enable,
            standalone::standalone_thread_create,
            standalone::standalone_thread_list,
            standalone::standalone_thread_peek_goal,
            standalone::standalone_thread_read,
            standalone::fortune_llm_call,
            standalone::test_model_connection,
            standalone::probe_model_capabilities,
            standalone::fetch_provider_models,
            standalone::fortune_detail_stream_start,
            standalone::standalone_thread_goal_set,
            standalone::standalone_thread_goal_status,
            standalone::standalone_thread_goal_edit,
            standalone::standalone_thread_goal_clear,
            standalone::standalone_thread_truncate_before,
            standalone::standalone_chat,
            standalone::standalone_turn_interrupt,
            standalone::standalone_plan_open,
            // Usage tracking
            commands::usage_get_stats,
            commands::usage_get_daily,
            commands::usage_get_by_model,
            commands::usage_get_recent,
            commands::usage_set_pricing,
            commands::usage_get_pricing,
            // Auto update
            commands::update_check,
            commands::update_start,
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
            commands::window_detach_browser,
            commands::window_attach_browser,
            commands::browser_get_edit_context,
            commands::browser_start_pick_mode,
            commands::browser_stop_pick_mode,
            commands::browser_poll_picked_element,
            commands::browser_apply_dom_edit,
            commands::browser_refresh_preview,
            commands::browser_get_navigation_state,
            commands::browser_go_back,
            commands::browser_go_forward,
            commands::browser_navigate_home,
            commands::window_open_document_detail,
            commands::window_close_document_detail,
            commands::window_get_document_detail_path,
            commands::window_get_document_detail_line,
            commands::window_open_runsummary_diff,
            commands::window_close_runsummary_diff,
            commands::window_get_runsummary_diff_payload,
            commands::window_set_computer_use_overlay,
            commands::document_detail_insert_snippet,
            commands::reveal_in_explorer,
            commands::window_toggle_devtools,
            commands::get_user_home_dir,
            commands::read_directory,
            commands::search_workspace_files,
            commands::delete_path,
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
            // LAN collab (weak-center owner + P2P data plane)
            lan_collab::lan_collab_status,
            lan_collab::lan_collab_set_enabled,
            lan_collab::lan_collab_set_display_name,
            lan_collab::lan_collab_list_peers,
            lan_collab::lan_collab_connect_peer,
            lan_collab::lan_collab_refresh_scan,
            lan_collab::lan_collab_create_group,
            lan_collab::lan_collab_join_group,
            lan_collab::lan_collab_list_groups,
            lan_collab::lan_collab_send_message,
            lan_collab::lan_collab_list_messages,
            lan_collab::lan_collab_share_model,
            lan_collab::lan_collab_unshare_model,
            lan_collab::lan_collab_list_local_shared_models,
            lan_collab::lan_collab_list_remote_shared_models,
            lan_collab::lan_collab_share_knowledge,
            lan_collab::lan_collab_unshare_knowledge,
            lan_collab::lan_collab_list_local_shared_knowledge,
            lan_collab::lan_collab_list_remote_shared_knowledge,
            lan_collab::lan_collab_list_shareable_knowledge_docs,
            lan_collab::lan_collab_search_remote_knowledge,
            lan_collab::lan_collab_fetch_remote_knowledge,
            lan_collab::lan_collab_share_skill,
            lan_collab::lan_collab_unshare_skill,
            lan_collab::lan_collab_list_local_shared_skills,
            lan_collab::lan_collab_list_remote_shared_skills,
            lan_collab::lan_collab_install_remote_skill,
            lan_collab::lan_collab_share_workflow,
            lan_collab::lan_collab_unshare_workflow,
            lan_collab::lan_collab_list_local_shared_workflows,
            lan_collab::lan_collab_list_remote_shared_workflows,
            lan_collab::lan_collab_install_remote_workflow,
            lan_collab::lan_collab_list_workflow_share_origins,
            // Recording & External Browser
            commands::launch_browser,
            commands::close_external_browser,
            commands::recording_start,
            commands::recording_stop,
            commands::recording_status,
            commands::recording_show_toggle,
            commands::recording_list_traces,
            commands::recording_read_trace,
            // SmartBrain
            smartbrain::commands::smartbrain_list_experiences,
            smartbrain::commands::smartbrain_read_experience,
            smartbrain::commands::smartbrain_delete_experience,
            smartbrain::commands::smartbrain_delete_experiences,
            smartbrain::commands::smartbrain_summarize_experiences,
            smartbrain::commands::smartbrain_list_knowledge,
            smartbrain::commands::smartbrain_read_knowledge,
            smartbrain::commands::smartbrain_delete_knowledge,
            smartbrain::commands::smartbrain_update_knowledge,
            smartbrain::commands::smartbrain_upload_knowledge,
            smartbrain::commands::smartbrain_upload_knowledge_folder,
            smartbrain::commands::smartbrain_search,
            smartbrain::commands::smartbrain_parse_database_connection,
            smartbrain::commands::smartbrain_list_databases,
            smartbrain::commands::smartbrain_rebuild_index,
            smartbrain::commands::smartbrain_migrate_to_okf,
            // Workflow
            workflow::commands::workflow_extract,
            workflow::commands::workflow_save,
            workflow::commands::workflow_list,
            workflow::commands::workflow_read,
            workflow::commands::workflow_delete,
            // WPS server
        ])
        .run(tauri::generate_context!())
        .expect("error while running CN-Codex");
}
