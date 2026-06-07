use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};

use crate::agent::AgentEngine;
use crate::config_system::ConfigManager;
use crate::jsonrpc_client::JsonRpcClient;
use crate::llm_tiers::LlmTiersManager;
use crate::protocol::{JSONRPCErrorError, RequestId};
use crate::standalone::StandaloneState;
use crate::thread_store::ThreadStore;
use crate::tool_executor::ToolExecutor;

const WORKSPACE_CONFIG_DIR: &str = "codey";

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
    pub llm_tiers: RwLock<LlmTiersManager>,
    pub client: RwLock<Option<Arc<JsonRpcClient>>>,
    pub current_thread_id: RwLock<Option<String>>,
    pub project_root: PathBuf,
    pub workspace_config_dir: PathBuf,
    pub config_path: PathBuf,
    pub cwd: RwLock<String>,
    pub approval_tx: mpsc::Sender<ApprovalAction>,
    pub approval_rx: RwLock<Option<mpsc::Receiver<ApprovalAction>>>,
    pub standalone: StandaloneState,
    pub config_manager: ConfigManager,
    pub thread_store: Arc<ThreadStore>,
    pub agent_engine: AgentEngine,
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
    workspace_dir
}

impl AppState {
    pub fn new() -> Self {
        let project_root = resolve_project_root();
        let workspace_config_dir = prepare_workspace_config_dir(&project_root);
        let config_path = workspace_config_dir.join("config.toml");
        let cwd = project_root.to_string_lossy().to_string();

        let (approval_tx, approval_rx) = mpsc::channel(64);

        let config_manager = ConfigManager::new(config_path.clone());
        let thread_store = Arc::new(ThreadStore::new(&workspace_config_dir));
        let tool_executor = ToolExecutor::new(project_root.clone());
        let agent_engine = AgentEngine::new(thread_store.clone(), tool_executor, project_root.clone())
            .expect("failed to create agent engine");

        Self {
            locale: RwLock::new("zh-CN".to_string()),
            llm_tiers: RwLock::new(LlmTiersManager::default()),
            client: RwLock::new(None),
            current_thread_id: RwLock::new(None),
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
        }
    }
}
