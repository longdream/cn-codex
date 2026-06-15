use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{RwLock, mpsc};
use tracing::info;

use crate::agent::AgentEngine;
use crate::config_system::ConfigManager;
use crate::protocol::{JSONRPCErrorError, RequestId};
use crate::standalone::StandaloneState;
use crate::thread_store::ThreadStore;
use crate::tool_executor::ToolExecutor;
use crate::usage::{PricingTable, UsageDb, UsageRecorder};

const WORKSPACE_CONFIG_DIR: &str = "codey";

/// 用户批准/拒绝操作（保留以兼容 approval 前端组件）
pub enum ApprovalAction {
    Resolve {
        request_id: RequestId,
        result: serde_json::Value,
    },
    Reject {
        request_id: RequestId,
        error: JSONRPCErrorError,
    },
}

pub struct AppState {
    pub locale: RwLock<String>,
    pub current_thread_id: Arc<RwLock<Option<String>>>,
    pub project_root: PathBuf,
    pub workspace_config_dir: PathBuf,
    pub config_path: PathBuf,
    pub cwd: RwLock<String>,
    pub approval_tx: mpsc::Sender<ApprovalAction>,
    pub approval_rx: RwLock<Option<mpsc::Receiver<ApprovalAction>>>,
    pub standalone: StandaloneState,
    pub config_manager: ConfigManager,
    pub thread_store: Arc<ThreadStore>,
    pub agent_engine: Arc<AgentEngine>,
    /// 用量数据库
    pub usage_db: Arc<UsageDb>,
    /// 价格表
    pub pricing_table: Arc<std::sync::RwLock<PricingTable>>,
}

fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn looks_like_project_root(path: &Path) -> bool {
    path.join(WORKSPACE_CONFIG_DIR).is_dir()
        || (path.join("package.json").is_file() && path.join("src-tauri").is_dir())
}

fn find_project_root_from(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if looks_like_project_root(ancestor) {
            return Some(canonicalize_or_keep(ancestor.to_path_buf()));
        }
    }

    None
}

fn resolve_project_root() -> PathBuf {
    if let Ok(project_root) = std::env::var("CN_CODEX_PROJECT_ROOT") {
        let path = PathBuf::from(project_root);
        if path.exists() {
            return canonicalize_or_keep(path);
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        if let Some(root) = find_project_root_from(&current_dir) {
            return root;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            if let Some(root) = find_project_root_from(exe_dir) {
                return root;
            }

            return canonicalize_or_keep(exe_dir.to_path_buf());
        }
    }

    canonicalize_or_keep(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn prepare_workspace_config_dir(project_root: &Path) -> PathBuf {
    let workspace_dir = project_root.join(WORKSPACE_CONFIG_DIR);
    let _ = std::fs::create_dir_all(&workspace_dir);
    let _ = std::fs::create_dir_all(workspace_dir.join("skills"));
    let _ = std::fs::create_dir_all(workspace_dir.join("plugins"));
    let _ = std::fs::create_dir_all(workspace_dir.join("memories"));
    workspace_dir
}

impl AppState {
    pub fn new() -> Self {
        // 启动阶段耗时打点：用于定位“每次启动都慢”的真实瓶颈。
        let startup_started_at = Instant::now();

        let phase_started_at = Instant::now();
        let project_root = resolve_project_root();
        info!(
            "[startup][rust] resolve_project_root done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        let phase_started_at = Instant::now();
        let workspace_config_dir = prepare_workspace_config_dir(&project_root);
        info!(
            "[startup][rust] prepare_workspace_config_dir done in {} ms",
            phase_started_at.elapsed().as_millis()
        );
        let config_path = workspace_config_dir.join("config.toml");
        let cwd = project_root.to_string_lossy().to_string();

        let (approval_tx, approval_rx) = mpsc::channel(64);

        let phase_started_at = Instant::now();
        let config_manager = ConfigManager::new(config_path.clone());
        info!(
            "[startup][rust] config_manager_init done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        let phase_started_at = Instant::now();
        let thread_store = Arc::new(ThreadStore::new(&workspace_config_dir));
        info!(
            "[startup][rust] thread_store_construct done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        let phase_started_at = Instant::now();
        let tool_executor = ToolExecutor::with_workspace_config_dir(
            project_root.clone(),
            workspace_config_dir.clone(),
        );
        info!(
            "[startup][rust] tool_executor_init done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        let phase_started_at = Instant::now();
        let mut agent_engine =
            AgentEngine::new(thread_store.clone(), tool_executor, project_root.clone())
                .expect("failed to create agent engine");
        info!(
            "[startup][rust] agent_engine_init done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        // 初始化用量追踪
        let phase_started_at = Instant::now();
        let db_path = workspace_config_dir.join("usage.db");
        let usage_db = Arc::new(UsageDb::open(&db_path).expect("failed to open usage database"));
        let pricing_table = Arc::new(std::sync::RwLock::new(PricingTable::new()));
        info!(
            "[startup][rust] usage_db_open done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        // 将 recorder 注入 agent engine
        let phase_started_at = Instant::now();
        let recorder_arc = Arc::new(UsageRecorder::new(usage_db.clone(), pricing_table.clone()));
        agent_engine.set_usage_recorder(recorder_arc);
        let agent_engine = Arc::new(agent_engine);
        info!(
            "[startup][rust] usage_recorder_bind done in {} ms",
            phase_started_at.elapsed().as_millis()
        );

        info!(
            "[startup][rust] AppState::new total {} ms",
            startup_started_at.elapsed().as_millis()
        );

        Self {
            locale: RwLock::new("zh-CN".to_string()),
            current_thread_id: Arc::new(RwLock::new(None)),
            project_root,
            workspace_config_dir,
            config_path,
            cwd: RwLock::new(cwd),
            approval_tx,
            approval_rx: RwLock::new(Some(approval_rx)),
            standalone: StandaloneState::new(),
            config_manager,
            thread_store,
            agent_engine,
            usage_db,
            pricing_table,
        }
    }
}
