use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose};
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, IBM866, WINDOWS_1252};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[cfg(windows)]
trait CommandNoConsole {
    fn no_console(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl CommandNoConsole for Command {
    fn no_console(&mut self) -> &mut Self {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

use crate::commands::plugin as plugin_commands;
use crate::config_system::{ConfigToml, McpServerConfig};
use crate::error::AppResult;
use crate::git_service::{GitCommandOutput, GitService};
use crate::plugin_loader;
use crate::protocol::RequestId;
use crate::state::{AppState, ApprovalAction};

mod code_review_support;
mod code_search_support;
mod memory_support;
mod patch_support;
use code_review_support::{
    CodeReviewArgs, analyze_code_review_diff, code_review_git_args, code_review_scope_label,
    code_review_untracked_paths, format_code_review_output, validate_code_review_base_ref,
    validate_code_review_paths,
};
#[cfg(test)]
use code_review_support::{CodeReviewSummary, ReviewFinding};
use code_search_support::{CodeSearchArgs, build_code_search_command, format_code_search_output};
use memory_support::{
    MemoryListEntry, MemoryOutputFormat, format_memory_list_output, format_memory_search_output,
    parse_memory_cursor, read_okf_body_lines, resolve_memory_path, search_memory_files,
};
#[cfg(test)]
use memory_support::{MemorySearchMatch, MemorySearchResult};
use patch_support::{
    ApplyPatchProgressChange, apply_patch_progress_changes, apply_patch_to_workspace,
    extract_patch_argument, format_apply_patch_report, parse_patch_actions, patch_display_label,
};
#[cfg(test)]
use patch_support::{ApplyPatchReport, ApplyPatchReportChange};

/// Provider configuration for internal subagents (set by parent agent before each turn).
#[derive(Debug, Clone, Default)]
pub struct SubagentProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub wire_api: String,
    pub system_prompt_prefix: String,
    pub max_output_tokens: Option<i64>,
}

pub struct ToolExecutor {
    cwd: PathBuf,
    http: reqwest::Client,
    workspace_config_dir: PathBuf,
    mcp_servers: HashMap<String, McpServerConfig>,
    mcp_tool_aliases: HashMap<String, McpToolAlias>,
    mcp_tool_specs: HashMap<String, serde_json::Value>,
    mcp_direct_tools_discovered: bool,
    mcp_discovery_retry_after: Option<Instant>,
    /// Per-thread tools activated via `tool_search` (or explicit activation) for layered schema loading.
    activated_tools_by_thread: Arc<Mutex<HashMap<String, BTreeSet<String>>>>,
    mcp_sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSession>>>>>,
    mcp_http_sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpHttpSession>>>>>,
    web_search_enabled: bool,
    /// Request-scoped SmartBrain override. `None` falls back to workspace config.
    smartbrain_enabled_override: Option<bool>,
    subagents: Arc<Mutex<HashMap<String, SubagentRecord>>>,
    subagent_handles: Arc<Mutex<HashMap<String, crate::subagent_engine::SubagentHandle>>>,
    exec_sessions: Arc<Mutex<HashMap<u64, ExecSessionRecord>>>,
    next_exec_session_id: Arc<AtomicU64>,
    permission_grants: Arc<Mutex<Vec<serde_json::Value>>>,
    active_tool_processes: Arc<Mutex<HashMap<String, ActiveToolProcess>>>,
    active_browser_cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    subagent_provider_config: Arc<Mutex<SubagentProviderConfig>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpToolAlias {
    server: String,
    tool: String,
    connector: McpConnectorMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct McpConnectorMetadata {
    connector_id: Option<String>,
    connector_name: Option<String>,
    namespace_description: Option<String>,
}

struct McpSession {
    server: McpServerConfig,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    child: Child,
    stderr: Arc<Mutex<String>>,
    next_request_id: i64,
    request_count: u64,
    initialized_at_ms: i64,
}

struct McpHttpSession {
    server: McpServerConfig,
    session_id: Option<String>,
    next_request_id: i64,
    request_count: u64,
    initialized_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum McpRequestError {
    Rpc(String),
    Transport(String),
}

impl McpRequestError {
    fn message(self) -> String {
        match self {
            Self::Rpc(message) | Self::Transport(message) => message,
        }
    }

    fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl McpSession {
    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        if let Err(error) = write_mcp_message(
            &mut self.stdin,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params
            }),
        )
        .await
        {
            return Err(mcp_transport_error(&self.server.name, &self.stderr, error).await);
        }

        let response = match read_mcp_response(&mut self.reader, request_id).await {
            Ok(response) => response,
            Err(error) => {
                return Err(mcp_transport_error(&self.server.name, &self.stderr, error).await);
            }
        };

        self.request_count += 1;

        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP server '{}' returned error: {}",
                self.server.name,
                format_json_value(error)
            )));
        }

        Ok(response.get("result").cloned().unwrap_or(response))
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ShellArgs {
    command: ShellCommandArg,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    block_until_ms: Option<u64>,
    #[serde(default)]
    login: Option<bool>,
    #[serde(default)]
    sandbox_permissions: Option<String>,
    #[serde(default)]
    justification: Option<String>,
    #[serde(default)]
    prefix_rule: Option<Vec<String>>,
    #[serde(default)]
    additional_permissions: Option<serde_json::Value>,
}

const SHELL_TIMEOUT_MIN_MS: u64 = 1_000;
const SHELL_TIMEOUT_MAX_MS: u64 = 3_600_000;
const SHELL_TIMEOUT_DEFAULT_MS: u64 = 30_000;

/// Unified hard caps for tool outputs returned to the model.
/// These keep prompt growth predictable without stripping critical head/tail context.
const TOOL_OUTPUT_SHELL_MAX_CHARS: usize = 6_000;
const TOOL_OUTPUT_SHELL_PARTIAL_MAX_CHARS: usize = 4_000;
const TOOL_OUTPUT_READ_FILE_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_BROWSER_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_SEARCH_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_MEMORY_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_SMARTBRAIN_MAX_CHARS: usize = 8_000;

/// Default pagination window for `read_file` when no range is requested.
const READ_FILE_DEFAULT_MAX_LINES: usize = 200;
const READ_FILE_MAX_LINES_HARD_CAP: usize = 400;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum ShellCommandArg {
    Script(String),
    Argv(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ExecCommandArgs {
    cmd: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    login: Option<bool>,
    #[serde(default)]
    yield_time_ms: Option<u64>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
    #[serde(default)]
    sandbox_permissions: Option<String>,
    #[serde(default)]
    justification: Option<String>,
    #[serde(default)]
    prefix_rule: Option<Vec<String>>,
    #[serde(default)]
    additional_permissions: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct WriteStdinArgs {
    session_id: u64,
    #[serde(default)]
    chars: Option<String>,
    #[serde(default)]
    yield_time_ms: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct CloseExecSessionArgs {
    session_id: u64,
}

#[derive(Clone)]
struct ExecSessionRecord {
    id: u64,
    process_id: Option<u32>,
    command: String,
    cwd: String,
    started_at_ms: i64,
    output: Arc<Mutex<String>>,
    cursor: Arc<Mutex<usize>>,
    exit_code: Arc<Mutex<Option<i32>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
}

/// 记录可被“停止”中断的工具子进程。
///
/// 这里按 `thread_id + call_id` 维度登记，便于：
/// 1) 用户点击停止时按线程批量 kill；
/// 2) 工具自然结束时自动反注册。
#[derive(Clone)]
struct ActiveToolProcess {
    thread_id: String,
    call_id: String,
    tool_name: String,
    child: Arc<Mutex<Child>>,
}

#[derive(Debug, Deserialize)]
struct PlanUpdateArgs {
    #[serde(default)]
    explanation: Option<String>,
    plan: Vec<PlanItemArg>,
}

#[derive(Debug, Deserialize)]
struct PlanItemArg {
    step: String,
    status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct RequestUserInputArgs {
    questions: Vec<RequestUserInputQuestion>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct RequestUserInputQuestion {
    id: String,
    header: String,
    question: String,
    #[serde(default)]
    options: Vec<RequestUserInputQuestionOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct RequestUserInputQuestionOption {
    label: String,
    description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
struct RequestPermissionsArgs {
    #[serde(default, rename = "environment_id", alias = "environmentId")]
    environment_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    permissions: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ToolSearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct ToolSearchEntry {
    kind: String,
    name: String,
    description: String,
    source: String,
    path: Option<String>,
    spec: Option<serde_json::Value>,
    metadata: BTreeMap<String, String>,
    usage: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AppsListArgs {
    #[serde(default)]
    connector_id: Option<String>,
    #[serde(default)]
    include_tools: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ListAvailablePluginsArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    source_dir: Option<String>,
    #[serde(default)]
    include_installed: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct RequestPluginInstallArgs {
    #[serde(default)]
    tool_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tool_type: Option<String>,
    #[serde(default)]
    action_type: Option<String>,
    #[serde(default)]
    suggest_reason: Option<String>,
    #[serde(default)]
    source_dir: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PluginManageArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    plugin_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PluginInstallCandidate {
    id: String,
    name: String,
    version: Option<String>,
    description: Option<String>,
    source: String,
    destination: String,
    installed: bool,
    has_skills: bool,
    mcp_server_names: Vec<String>,
    app_connector_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AppConnectorListEntry {
    connector_id: String,
    connector_name: Option<String>,
    accessible: bool,
    source: String,
    install_url: String,
    plugin_apps: Vec<AppConnectorPluginSource>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AppConnectorToolEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct AppConnectorPluginSource {
    plugin_id: String,
    plugin_display_name: String,
    app_key: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct AppConnectorToolEntry {
    name: String,
    server: String,
    tool: String,
    connector_name: Option<String>,
    namespace_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct SpawnAgentArgs {
    prompt: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    wait: Option<bool>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    sandbox: Option<String>,
    #[serde(default)]
    dangerously_bypass_approvals_and_sandbox: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct WaitAgentArgs {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    agent_ids: Option<Vec<String>>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct SendInputArgs {
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    items: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    interrupt: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ResumeAgentArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    wait: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ListAgentsArgs {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CloseAgentArgs {
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ImageGenerateArgs {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    n: Option<u32>,
    #[serde(default)]
    output_path: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
}

#[derive(Debug, Clone)]
struct ImageGenerationSettingsResolved {
    enabled: bool,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
}

impl Default for ImageGenerationSettingsResolved {
    fn default() -> Self {
        Self {
            enabled: true,
            model: None,
            base_url: None,
            api_key: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct EchartsReportArgs {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    chart_type: Option<String>,
    option: serde_json::Value,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct GeneratedImageOutput {
    index: usize,
    output_path: String,
    absolute_path: String,
    format: String,
    mime: String,
    dimensions: String,
    width: u32,
    height: u32,
    size: String,
    size_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    revised_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageGenerationResponse {
    data: Vec<ImageGenerationItem>,
}

#[derive(Debug, Deserialize)]
struct ImageGenerationItem {
    #[serde(default)]
    b64_json: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    revised_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageGenerationErrorResponse {
    error: Option<ImageGenerationErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ImageGenerationErrorDetail {
    message: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentRecord {
    id: String,
    role: String,
    status: String,
    prompt: String,
    cwd: String,
    command: String,
    #[serde(default)]
    process_id: Option<u32>,
    started_at_ms: i64,
    #[serde(default)]
    completed_at_ms: Option<i64>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    input_history: Vec<SubagentInputRecord>,
    #[serde(default)]
    last_input_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentInputRecord {
    submission_id: String,
    message: String,
    submitted_at_ms: i64,
    interrupt: bool,
    delivered_to_stdin: bool,
}

#[derive(Debug, Clone)]
struct SubagentWaitResult {
    output: String,
    has_missing: bool,
    has_failed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubagentCloseResult {
    target: String,
    closed: bool,
    previous_status: String,
    message: String,
    agent: Option<SubagentRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendInputResult {
    target: String,
    submission_id: String,
    status: String,
    delivered_to_stdin: bool,
    queued: bool,
    note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResumeAgentResult {
    id: String,
    resumed: bool,
    previous_status: String,
    status: String,
    note: String,
    agent: Option<SubagentRecord>,
}

impl ToolExecutor {
    pub fn new(cwd: PathBuf) -> Self {
        let workspace_config_dir = cwd.join("codey");
        Self::with_workspace_config_dir(cwd, workspace_config_dir)
    }

    pub fn with_workspace_config_dir(cwd: PathBuf, workspace_config_dir: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .user_agent("CN-Codex/0.1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let subagents = load_subagent_records(&workspace_config_dir);
        Self {
            cwd,
            http,
            workspace_config_dir,
            mcp_servers: HashMap::new(),
            mcp_tool_aliases: HashMap::new(),
            mcp_tool_specs: HashMap::new(),
            mcp_direct_tools_discovered: false,
            mcp_discovery_retry_after: None,
            activated_tools_by_thread: Arc::new(Mutex::new(HashMap::new())),
            mcp_sessions: Arc::new(Mutex::new(HashMap::new())),
            mcp_http_sessions: Arc::new(Mutex::new(HashMap::new())),
            web_search_enabled: false,
            smartbrain_enabled_override: None,
            subagents: Arc::new(Mutex::new(subagents)),
            subagent_handles: Arc::new(Mutex::new(HashMap::new())),
            exec_sessions: Arc::new(Mutex::new(HashMap::new())),
            next_exec_session_id: Arc::new(AtomicU64::new(1)),
            permission_grants: Arc::new(Mutex::new(Vec::new())),
            active_tool_processes: Arc::new(Mutex::new(HashMap::new())),
            active_browser_cancellations: Arc::new(Mutex::new(HashMap::new())),
            subagent_provider_config: Arc::new(Mutex::new(SubagentProviderConfig::default())),
        }
    }

    pub async fn set_subagent_provider_config(&self, config: SubagentProviderConfig) {
        *self.subagent_provider_config.lock().await = config;
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    pub fn set_smartbrain_enabled_override(&mut self, enabled: Option<bool>) {
        self.smartbrain_enabled_override = enabled;
    }

    /// Thread-scoped tools activated by `tool_search` (or explicit activation).
    pub async fn activate_tools_for_thread<I, S>(&self, thread_id: &str, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let thread_id = thread_id.trim();
        if thread_id.is_empty() {
            return;
        }
        let mut by_thread = self.activated_tools_by_thread.lock().await;
        let activated = by_thread
            .entry(thread_id.to_string())
            .or_insert_with(BTreeSet::new);
        for name in names {
            let name = name.as_ref().trim();
            if !name.is_empty() {
                activated.insert(name.to_string());
            }
        }
    }

    pub async fn activated_tool_names_for_thread(&self, thread_id: &str) -> BTreeSet<String> {
        let thread_id = thread_id.trim();
        if thread_id.is_empty() {
            return BTreeSet::new();
        }
        self.activated_tools_by_thread
            .lock()
            .await
            .get(thread_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn clear_activated_tools_for_thread(&self, thread_id: &str) {
        let thread_id = thread_id.trim();
        if thread_id.is_empty() {
            return;
        }
        self.activated_tools_by_thread
            .lock()
            .await
            .remove(thread_id);
    }

    fn tool_spec_name(spec: &serde_json::Value) -> Option<&str> {
        spec.pointer("/function/name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// Built-in tools that are always exposed to the model (small core set).
    /// Everything else — especially MCP/Playwright direct schemas — is lazy-loaded via `tool_search`.
    fn is_core_tool_name(name: &str) -> bool {
        matches!(
            name,
            "shell"
                | "shell_command"
                | "exec_command"
                | "write_stdin"
                | "close_exec_session"
                | "read_file"
                | "write_file"
                | "apply_patch"
                | "list_directory"
                | "code_search"
                | "tool_search"
                | "update_plan"
                | "request_user_input"
                | "request_permissions"
                | "view_image"
                | "code_review"
                | "smartbrain_search"
                | "build_entry_form"
                | "save_form_data"
                | "browser_run"
                // Optional web tools are only present in `tool_specs` when enabled.
                | "web_search"
                | "web_fetch"
        )
    }

    pub fn set_mcp_servers(&mut self, mcp_servers: HashMap<String, McpServerConfig>) {
        if self.mcp_servers == mcp_servers {
            return;
        }
        self.mcp_servers = mcp_servers;
        self.mcp_tool_aliases.clear();
        self.mcp_tool_specs.clear();
        self.mcp_direct_tools_discovered = false;
        self.mcp_discovery_retry_after = None;
        clear_mcp_sessions_async(self.mcp_sessions.clone());
        clear_mcp_http_sessions_async(self.mcp_http_sessions.clone());
    }

    /// 仅用于“可中断工具”登记：key = thread_id::call_id。
    fn active_process_key(thread_id: &str, call_id: &str) -> String {
        format!("{thread_id}::{call_id}")
    }

    /// 工具子进程启动后登记，供停止按钮按线程中断。
    async fn register_active_tool_process(
        &self,
        thread_id: &str,
        call_id: &str,
        tool_name: &str,
        child: Arc<Mutex<Child>>,
    ) {
        let key = Self::active_process_key(thread_id, call_id);
        let mut table = self.active_tool_processes.lock().await;
        table.insert(
            key,
            ActiveToolProcess {
                thread_id: thread_id.to_string(),
                call_id: call_id.to_string(),
                tool_name: tool_name.to_string(),
                child,
            },
        );
    }

    /// 工具结束后反注册，避免内存中残留失效句柄。
    async fn unregister_active_tool_process(&self, thread_id: &str, call_id: &str) {
        let key = Self::active_process_key(thread_id, call_id);
        self.active_tool_processes.lock().await.remove(&key);
    }

    async fn register_active_browser_cancellation(
        &self,
        thread_id: &str,
        call_id: &str,
        cancel_flag: Arc<AtomicBool>,
    ) {
        let key = Self::active_process_key(thread_id, call_id);
        self.active_browser_cancellations
            .lock()
            .await
            .insert(key, cancel_flag);
    }

    async fn unregister_active_browser_cancellation(&self, thread_id: &str, call_id: &str) {
        let key = Self::active_process_key(thread_id, call_id);
        self.active_browser_cancellations.lock().await.remove(&key);
    }

    /// 停止当前线程下所有活跃工具进程。
    ///
    /// 返回实际命中并尝试 kill 的进程数量，便于上层日志确认中断行为。
    pub async fn interrupt_active_tools(&self, thread_id: &str) -> usize {
        let mut table = self.active_tool_processes.lock().await;
        let process_keys: Vec<String> = table
            .iter()
            .filter_map(|(key, entry)| {
                if entry.thread_id == thread_id {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect();
        let processes: Vec<ActiveToolProcess> = process_keys
            .iter()
            .filter_map(|key| table.remove(key))
            .collect();
        drop(table);
        let mut cancel_table = self.active_browser_cancellations.lock().await;
        let cancel_keys: Vec<String> = cancel_table
            .keys()
            .filter_map(|key| {
                if key.starts_with(&format!("{thread_id}::")) {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect();
        let cancel_flags: Vec<Arc<AtomicBool>> = cancel_keys
            .iter()
            .filter_map(|key| cancel_table.remove(key))
            .collect();
        drop(cancel_table);

        for process in &processes {
            let mut child = process.child.lock().await;
            if let Err(error) = child.kill().await {
                info!(
                    "interrupt_active_tools: failed to kill {} call {}: {}",
                    process.tool_name, process.call_id, error
                );
            }
        }
        for cancel in cancel_flags {
            cancel.store(true, Ordering::SeqCst);
        }

        process_keys.len().max(cancel_keys.len())
    }

    /// 当线程 id 不可用时的兜底：中断所有活跃工具。
    pub async fn interrupt_all_active_tools(&self) -> usize {
        let mut table = self.active_tool_processes.lock().await;
        let processes: Vec<ActiveToolProcess> = table.drain().map(|(_, entry)| entry).collect();
        drop(table);
        let mut cancel_table = self.active_browser_cancellations.lock().await;
        let cancel_flags: Vec<Arc<AtomicBool>> =
            cancel_table.drain().map(|(_, flag)| flag).collect();
        drop(cancel_table);

        for process in &processes {
            let mut child = process.child.lock().await;
            if let Err(error) = child.kill().await {
                info!(
                    "interrupt_all_active_tools: failed to kill {} call {}: {}",
                    process.tool_name, process.call_id, error
                );
            }
        }
        for cancel in &cancel_flags {
            cancel.store(true, Ordering::SeqCst);
        }

        processes.len().max(cancel_flags.len())
    }

    async fn remember_permission_grant(&self, profile: serde_json::Value) {
        if !profile.is_object() || profile.as_object().is_some_and(|object| object.is_empty()) {
            return;
        }

        let mut grants = self.permission_grants.lock().await;
        if !grants.iter().any(|grant| grant == &profile) {
            grants.push(profile);
        }
    }

    async fn additional_permissions_preapproved(
        &self,
        sandbox_permissions: Option<&str>,
        additional_permissions: Option<&serde_json::Value>,
    ) -> bool {
        if normalize_sandbox_permissions(sandbox_permissions).as_deref()
            != Some("with_additional_permissions")
        {
            return false;
        }

        let Some(requested) = additional_permissions else {
            return false;
        };

        let grants = self.permission_grants.lock().await;
        grants
            .iter()
            .any(|granted| permission_profile_covers(granted, requested))
    }

    pub fn tool_specs(&self, web_search_enabled: bool) -> Vec<serde_json::Value> {
        let mut tools = vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell",
                    "description": "Runs one complete shell script and returns its output. On Windows the script must be valid PowerShell with every pipeline starting from an input-producing command. Supports workdir, timeout_ms (or block_until_ms), login, and sandbox permission approval fields.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "minLength": 1,
                                "description": "One complete shell script. On Windows use valid PowerShell syntax, provide an input command before every pipeline, and use $_ as the current pipeline object."
                            },
                            "workdir": {
                                "type": "string",
                                "description": "Working directory for the command. Defaults to the turn cwd."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Maximum command runtime in milliseconds. Defaults to 30000 ms. Valid range: 1000-3600000."
                            },
                            "block_until_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Compatibility alias for timeout_ms."
                            },
                            "login": {
                                "type": "boolean",
                                "description": "True runs with login/default shell semantics; false disables profile/login behavior where supported. Defaults to true."
                            },
                            "sandbox_permissions": {
                                "type": "string",
                                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                                "description": "Per-command permission override. Defaults to use_default; require_escalated or with_additional_permissions asks the user before running."
                            },
                            "justification": {
                                "type": "string",
                                "description": "User-facing approval reason for sandbox_permissions overrides."
                            },
                            "prefix_rule": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Reusable approval prefix for the command, only with sandbox_permissions: require_escalated."
                            },
                            "additional_permissions": {
                                "type": "object",
                                "description": "Sandboxed filesystem or network access for this command; only with sandbox_permissions: with_additional_permissions."
                            }
                        },
                        "required": ["command"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell_command",
                    "description": "Codex-compatible shell tool. Runs one complete PowerShell script on Windows or one shell script on Unix and returns output. Every PowerShell pipeline must start from an input-producing command, and Where-Object/ForEach-Object use $_ as the current object. Supports workdir, timeout_ms (or block_until_ms), login, sandbox_permissions, justification, prefix_rule, and additional_permissions.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "minLength": 1,
                                "description": "One complete shell script. On Windows use valid PowerShell syntax, provide an input command before every pipeline, and use $_ as the current pipeline object."
                            },
                            "workdir": {
                                "type": "string",
                                "description": "Working directory for the command. Defaults to the turn cwd."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Maximum command runtime in milliseconds. Defaults to 30000 ms. Valid range: 1000-3600000."
                            },
                            "block_until_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Compatibility alias for timeout_ms."
                            },
                            "login": {
                                "type": "boolean",
                                "description": "True runs with login/default shell semantics; false disables profile/login behavior where supported. Defaults to true."
                            },
                            "sandbox_permissions": {
                                "type": "string",
                                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                                "description": "Per-command permission override. Defaults to use_default; require_escalated or with_additional_permissions asks the user before running."
                            },
                            "justification": {
                                "type": "string",
                                "description": "User-facing approval reason for sandbox_permissions overrides."
                            },
                            "prefix_rule": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Reusable approval prefix for the command, only with sandbox_permissions: require_escalated."
                            },
                            "additional_permissions": {
                                "type": "object",
                                "description": "Sandboxed filesystem or network access for this command; only with sandbox_permissions: with_additional_permissions."
                            }
                        },
                        "required": ["command"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "exec_command",
                    "description": "Run a command as a persistent exec session. Returns output and, when the process is still running after yield_time_ms, a session_id that can be passed to write_stdin for input or polling. This is CN-Codex's lightweight equivalent of Codex unified exec sessions.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "cmd": {
                                "type": "string",
                                "minLength": 1,
                                "description": "Shell command to execute."
                            },
                            "workdir": {
                                "type": "string",
                                "description": "Working directory for the command. Defaults to the turn cwd."
                            },
                            "shell": {
                                "type": "string",
                                "description": "Optional shell binary to launch. Defaults to PowerShell on Windows and SHELL/sh on Unix."
                            },
                            "login": {
                                "type": "boolean",
                                "description": "True runs with login/default shell semantics where supported; false disables profile/login behavior. Defaults to true."
                            },
                            "yield_time_ms": {
                                "type": "integer",
                                "minimum": 250,
                                "maximum": 30000,
                                "description": "Wait before returning output. Defaults to 10000 ms."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 250,
                                "maximum": 30000,
                                "description": "Compatibility alias for yield_time_ms."
                            },
                            "max_output_tokens": {
                                "type": "integer",
                                "minimum": 100,
                                "maximum": 50000,
                                "description": "Approximate output token budget. Defaults to 10000 tokens."
                            },
                            "sandbox_permissions": {
                                "type": "string",
                                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                                "description": "Per-command permission override. Defaults to use_default; require_escalated or with_additional_permissions asks the user before running."
                            },
                            "justification": {
                                "type": "string",
                                "description": "User-facing approval reason for sandbox_permissions overrides."
                            },
                            "prefix_rule": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Reusable approval prefix for cmd, only with sandbox_permissions: require_escalated."
                            },
                            "additional_permissions": {
                                "type": "object",
                                "description": "Sandboxed filesystem or network access for this command; only with sandbox_permissions: with_additional_permissions."
                            }
                        },
                        "required": ["cmd"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_stdin",
                    "description": "Write characters to a running exec_command session, or poll recent output when chars is omitted.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "session_id": {
                                "type": "integer",
                                "description": "Identifier returned by exec_command."
                            },
                            "chars": {
                                "type": "string",
                                "description": "Characters to write to stdin. Omit or pass an empty string to poll output only."
                            },
                            "yield_time_ms": {
                                "type": "integer",
                                "minimum": 250,
                                "maximum": 300000,
                                "description": "Wait before returning output. Defaults to 250 ms after writes and 5000 ms for polling."
                            },
                            "max_output_tokens": {
                                "type": "integer",
                                "minimum": 100,
                                "maximum": 50000,
                                "description": "Approximate output token budget. Defaults to 10000 tokens."
                            }
                        },
                        "required": ["session_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "close_exec_session",
                    "description": "Terminate and remove a running exec_command session by session_id. Use this to stop long-running servers, hung commands, or sessions that are no longer needed.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "session_id": {
                                "type": "integer",
                                "description": "Identifier returned by exec_command."
                            }
                        },
                        "required": ["session_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read the contents of a file at the given path. Returns a numbered page by default (max_lines defaults to 200, hard cap 400). Use line_offset/end_line to page through large files. Prefer this over shell/python for reading source slices.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The file path to read."
                            },
                            "line_offset": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "1-indexed starting line. Defaults to 1. Use with max_lines/end_line for ranged reads."
                            },
                            "max_lines": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 400,
                                "description": "Maximum lines to return. Defaults to 200. Large files are always returned as a numbered page; use line_offset to continue."
                            },
                            "end_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Optional inclusive end line. When set, overrides max_lines as (end_line - line_offset + 1)."
                            },
                            "show_line_numbers": {
                                "type": "boolean",
                                "description": "Prefix each returned line with its 1-indexed line number. Defaults to true for ranged reads and false for full-file reads."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Create a new UTF-8 text file with the given content, or completely rewrite a file only when explicitly required. For changes to an existing text file, use apply_patch so unrelated content, encoding, and line endings are preserved.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The file path to write."
                            },
                            "content": {
                                "type": "string",
                                "description": "The content to write to the file."
                            }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "tool_search",
                    "description": "Search available CN-Codex tools, local skills, plugin skills, and discovered MCP tools, then activate matching non-core tool schemas for the next model call in this same turn (and later iterations in the thread). Default turns only expose a small core tool set; use this to lazy-load MCP/Playwright and other non-core tools before calling them.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query for tools or skills."
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 50,
                                "description": "Maximum number of matches to return. Defaults to 8."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "apps_list",
                    "description": "List plugin-declared app connectors and currently exposed trusted codex-apps MCP tools. Use this to see which imported plugin apps are installed, which connector IDs they use, and whether matching MCP tools are available.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "connector_id": {
                                "type": "string",
                                "description": "Optional connector ID to filter to one app connector."
                            },
                            "include_tools": {
                                "type": "boolean",
                                "description": "Include matching MCP tool names for each connector. Defaults to true."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_available_plugins_to_install",
                    "description": "List Codex plugin cache candidates that CN-Codex can import into this workspace. Use this before request_plugin_install when a requested plugin or connector is not yet available.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Optional search text matched against plugin name, description, source path, MCP servers, and app connector IDs."
                            },
                            "source_dir": {
                                "type": "string",
                                "description": "Optional Codex plugin cache directory. Defaults to the user's .cn-codex/plugins/cache directory."
                            },
                            "include_installed": {
                                "type": "boolean",
                                "description": "Include plugins already imported into codey/plugins. Defaults to true."
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 100,
                                "description": "Maximum candidates to return. Defaults to 50."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_plugin_install",
                    "description": "Import one plugin from the local Codex plugin cache into codey/plugins. This is CN-Codex's local equivalent of Codex plugin install suggestions; it reimports/updates the plugin if already present.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "tool_id": {
                                "type": "string",
                                "description": "Candidate id returned by list_available_plugins_to_install."
                            },
                            "name": {
                                "type": "string",
                                "description": "Optional plugin name fallback when tool_id is not known."
                            },
                            "tool_type": {
                                "type": "string",
                                "enum": ["plugin"],
                                "description": "For compatibility with Codex request_plugin_install. Only plugin is supported locally."
                            },
                            "action_type": {
                                "type": "string",
                                "enum": ["install"],
                                "description": "For compatibility with Codex request_plugin_install. Only install is supported."
                            },
                            "suggest_reason": {
                                "type": "string",
                                "description": "Short reason this plugin is needed."
                            },
                            "source_dir": {
                                "type": "string",
                                "description": "Optional Codex plugin cache directory. Defaults to the user's .cn-codex/plugins/cache directory."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "plugin_manage",
                    "description": "List, enable, disable, or uninstall local workspace plugins under codey/plugins. Disabled plugins stay on disk but are excluded from skill prompts, MCP servers, app connectors, and hooks. Use uninstall only when explicitly requested.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["list", "enable", "disable", "uninstall"],
                                "description": "Plugin management action. Defaults to list."
                            },
                            "plugin_id": {
                                "type": "string",
                                "description": "Workspace plugin id from plugin_manage list output. Required for enable, disable, and uninstall."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for plugin_id."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "robot_save",
                    "description": "Create or update a Robot configuration. A Robot is a specialized AI role that binds skills to each workflow node. Available in robot-create and robot-modify modes.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Robot directory name (lowercase, hyphens, no spaces). E.g. 'code-reviewer', 'security-auditor'."
                            },
                            "config": {
                                "type": "object",
                                "description": "Complete robot.json content.",
                                "properties": {
                                    "name": { "type": "string", "description": "Human-readable robot name." },
                                    "description": { "type": "string", "description": "What this robot does." },
                                    "icon": { "type": "string", "description": "Icon identifier for the robot." },
                                    "skills": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "Local skill IDs from codey/skills/."
                                    },
                                    "pluginSkills": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "pluginId": { "type": "string" },
                                                "skillId": { "type": "string" }
                                            },
                                            "required": ["pluginId", "skillId"]
                                        },
                                        "description": "Plugin skill references."
                                    },
                                    "workflow": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "Legacy workflow text steps for backward compatibility. Prefer workflowNodes."
                                    },
                                    "workflowNodes": {
                                        "type": "array",
                                        "description": "Structured workflow nodes with per-node skill assignment.",
                                        "minItems": 1,
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "objective": {
                                                    "type": "string",
                                                    "description": "What this node must complete before moving to the next node."
                                                },
                                                "skills": {
                                                    "type": "array",
                                                    "items": { "type": "string" },
                                                    "description": "Local skill IDs assigned to this node."
                                                },
                                                "pluginSkills": {
                                                    "type": "array",
                                                    "items": {
                                                        "type": "object",
                                                        "properties": {
                                                            "pluginId": { "type": "string" },
                                                            "skillId": { "type": "string" }
                                                        },
                                                        "required": ["pluginId", "skillId"]
                                                    },
                                                    "description": "Plugin skill references assigned to this node."
                                                }
                                            },
                                            "required": ["objective", "skills", "pluginSkills"],
                                            "anyOf": [
                                                {
                                                    "properties": {
                                                        "skills": { "minItems": 1 }
                                                    }
                                                },
                                                {
                                                    "properties": {
                                                        "pluginSkills": { "minItems": 1 }
                                                    }
                                                }
                                            ]
                                        }
                                    },
                                    "systemPrompt": { "type": "string", "description": "Role definition and behavior rules for the robot." }
                                },
                                "required": ["name", "description", "workflowNodes", "systemPrompt"]
                            }
                        },
                        "required": ["id", "config"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "code_search",
                    "description": "Search source code in the current workspace using CN-Codex's built-in search engine. The implementation is embedded in CN-Codex and does not require an external rg installation.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "minLength": 1,
                                "description": "Text or regular expression to search for."
                            },
                            "path": {
                                "type": "string",
                                "description": "Optional file or directory inside the current workspace. Defaults to the workspace root."
                            },
                            "glob": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 },
                                "description": "Optional glob filters such as '*.rs' or 'src/**'."
                            },
                            "case_sensitive": {
                                "type": "boolean",
                                "description": "Use case-sensitive matching. Defaults to false."
                            },
                            "fixed_strings": {
                                "type": "boolean",
                                "description": "Treat pattern as literal text instead of a regular expression. Defaults to false."
                            },
                            "context": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 5,
                                "description": "Context lines before and after each match."
                            },
                            "head_limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 200,
                                "description": "Maximum match lines to return. Defaults to 50."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 120000,
                                "description": "Search timeout in milliseconds. Defaults to 30000."
                            }
                        },
                        "required": ["pattern"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "code_review",
                    "description": "Review the current git diff or a diff against a base ref. It summarizes changed files, runs git diff --check, and flags obvious risks such as secret-looking additions, risky APIs, debug logging, and source changes without matching tests.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "base_ref": {
                                "type": "string",
                                "description": "Optional git ref to compare against. Defaults to HEAD, so staged and unstaged tracked changes are reviewed."
                            },
                            "paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional path filters inside the repository."
                            },
                            "max_diff_bytes": {
                                "type": "integer",
                                "minimum": 4000,
                                "maximum": 1000000,
                                "description": "Maximum diff bytes to inspect. Defaults to 200000."
                            },
                            "include_untracked": {
                                "type": "boolean",
                                "description": "Include untracked file names in the report. Defaults to true; contents are not inspected until tracked."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "apply_patch",
                    "description": "Default tool for editing existing text files, whether one file or many. Applies contextual diffs while preserving encoding and line endings, and supports add/update/delete/move in Codex apply_patch format. Read the current file before editing. Each update hunk starts with @@, optionally followed by an exact class, function, section, or other source line that anchors the search (for example: @@ function renderApp()). Every hunk body line must start with exactly one marker: '-' for removed lines, '+' for added lines, or one space for unchanged context. Do not use '|-', '+|', '||', or separate old/new blocks. Include about 3 exact context lines above and below each small change; use an anchor when the snippet repeats. Use *** End of File after a hunk that must match the file ending. If a hunk does not match, re-read that file and retry apply_patch with refreshed, smaller context. Relative paths are preferred; absolute paths must resolve inside the workspace. Prefer the raw/freeform patch body when supported; function-call providers may use JSON fields named patch or command. The patch must start with *** Begin Patch and end with *** End Patch.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "patch": {
                                "type": "string",
                                "description": "Patch body using *** Add File, *** Update File, and *** Delete File sections."
                            },
                            "command": {
                                "type": "string",
                                "description": "Compatibility alias for a raw apply_patch command or patch body."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_directory",
                    "description": "List files and directories at the given path.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The directory path to list. Defaults to current working directory."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "update_plan",
                    "description": "Update the current task plan for multi-step work. Use concise steps, and keep at most one step in_progress.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "explanation": {
                                "type": "string",
                                "description": "Optional short explanation for this plan update."
                            },
                            "plan": {
                                "type": "array",
                                "description": "Ordered plan items.",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "step": {
                                            "type": "string",
                                            "description": "A concise task step."
                                        },
                                        "status": {
                                            "type": "string",
                                            "enum": ["pending", "in_progress", "completed"],
                                            "description": "Current status for this step."
                                        }
                                    },
                                    "required": ["step", "status"]
                                }
                            }
                        },
                        "required": ["plan"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_user_input",
                    "description": "Request user input for one to three short questions and wait for the response. Use this only when progress genuinely depends on a user choice or answer.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "questions": {
                                "type": "array",
                                "description": "Questions to show the user. Prefer 1 and do not exceed 3.",
                                "minItems": 1,
                                "maxItems": 3,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": {
                                            "type": "string",
                                            "description": "Stable identifier for mapping answers, preferably snake_case."
                                        },
                                        "header": {
                                            "type": "string",
                                            "description": "Short header label shown in the UI."
                                        },
                                        "question": {
                                            "type": "string",
                                            "description": "Single-sentence prompt shown to the user."
                                        },
                                        "options": {
                                            "type": "array",
                                            "description": "Optional mutually exclusive choices. Put the recommended option first when there is one, and prefer adding '(Recommended)' to the recommended label.",
                                            "minItems": 0,
                                            "maxItems": 3,
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "label": {
                                                        "type": "string",
                                                        "description": "User-facing label."
                                                    },
                                                    "description": {
                                                        "type": "string",
                                                        "description": "Short explanation of the option."
                                                    }
                                                },
                                                "required": ["label", "description"]
                                            }
                                        }
                                    },
                                    "required": ["id", "header", "question"]
                                }
                            }
                        },
                        "required": ["questions"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_permissions",
                    "description": "Request additional filesystem or network permissions from the user and wait for the client to grant a subset of the requested permission profile. Granted additional_permissions are cached for this executor and reused by later shell/exec calls that request the same or narrower profile.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "reason": {
                                "type": "string",
                                "description": "Optional short explanation for why additional permissions are needed."
                            },
                            "environment_id": {
                                "type": "string",
                                "description": "Optional environment id. Omit to use the primary workspace environment."
                            },
                            "permissions": {
                                "type": "object",
                                "description": "Requested permission profile. Use network and/or file_system fields.",
                                "properties": {
                                    "network": {
                                        "type": "object",
                                        "description": "Requested network permissions."
                                    },
                                    "file_system": {
                                        "type": "object",
                                        "description": "Requested filesystem permissions."
                                    }
                                }
                            }
                        },
                        "required": ["permissions"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "view_image",
                    "description": "Inspect a local image file and return its format, dimensions, size, and absolute path. Use this when the user asks about an image or when visual assets need verification.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative path inside the workspace, or an absolute local image path."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "ocr_image",
                    "description": "Run bundled PP-OCRv5 mobile via ONNX Runtime on a local image and return extracted text.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative path inside the workspace, or an absolute local image path."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "image_generate",
                    "description": "Generate an image through an OpenAI Images API-compatible backend, save it as a local file, and return its path, format, dimensions, and size. Resolution order: tool args > image_generation settings > env vars > defaults.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "Detailed prompt describing the image to generate."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional image model. Defaults to image_generation.model, then CN_CODEX_IMAGE_MODEL, then gpt-image-2."
                            },
                            "size": {
                                "type": "string",
                                "description": "Image size such as 1024x1024, 1024x1536, 1536x1024, or auto. Defaults to 1024x1024."
                            },
                            "quality": {
                                "type": "string",
                                "description": "Optional provider-specific quality value, such as low, medium, high, hd, or auto."
                            },
                            "background": {
                                "type": "string",
                                "description": "Optional provider-specific background value, such as transparent, opaque, or auto."
                            },
                            "n": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 10,
                                "description": "Number of images to generate. Values are clamped to 1-10. Defaults to 1."
                            },
                            "output_path": {
                                "type": "string",
                                "description": "Optional output file path. Relative paths resolve inside the workspace and must not contain '..'. Defaults to codey/images/generated/<id>.png. When n is greater than 1, suffixed paths such as name-1.png and name-2.png are used."
                            },
                            "base_url": {
                                "type": "string",
                                "description": "Optional OpenAI-compatible base URL or full /images/generations endpoint. Defaults to image_generation.base_url, then CN_CODEX_IMAGE_BASE_URL, then https://api.openai.com/v1."
                            }
                        },
                        "required": ["prompt"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "echarts_report",
                    "description": "Prepare an interactive ECharts report configuration and return a reusable ```echarts code block for chat rendering.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Optional chart title shown in tool summaries."
                            },
                            "chart_type": {
                                "type": "string",
                                "description": "Optional chart type hint, such as line, bar, pie, scatter, radar, or heatmap."
                            },
                            "option": {
                                "type": "object",
                                "description": "ECharts option object. Must be valid JSON object syntax accepted by echarts.setOption."
                            },
                            "notes": {
                                "type": "string",
                                "description": "Optional notes or interpretation text for this report."
                            }
                        },
                        "required": ["option"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "browser_run",
                    "description": "Run a browser session for navigation, UI interaction, screenshots, rendered DOM inspection, and web app testing. Runtime uses CN-Codex Tauri WebView with Rust-side JS Injection + CDP.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "engine": {
                                "type": "string",
                                "enum": ["webview-js-injection"],
                                "description": "Compatibility field. Runtime is always mapped to webview-js-injection internally."
                            },
                            "url": {
                                "type": "string",
                                "description": "Initial URL to open. Required unless the first action is a goto."
                            },
                            "headless": {
                                "type": "boolean",
                                "description": "Compatibility field kept for old prompts. Ignored by WebView runtime."
                            },
                            "channel": {
                                "type": "string",
                                "description": "Compatibility field kept for old prompts. Ignored in WebView runtime."
                            },
                            "use_visible_browser": {
                                "type": "boolean",
                                "description": "Compatibility field kept for old prompts. WebView runtime always controls CN-Codex built-in browser."
                            },
                            "viewport": {
                                "type": "object",
                                "properties": {
                                    "width": { "type": "integer", "minimum": 320 },
                                    "height": { "type": "integer", "minimum": 240 }
                                }
                            },
                            "actions": {
                                "type": "array",
                                "description": "Ordered browser actions: goto, reload, back, forward, click, hover, fill, type, press, check, uncheck, select_option, wait_for_selector, wait_for_timeout, screenshot, set_viewport, title, url, html, snapshot, assets, bundle_assets, eval, text, list_tabs, new_tab, switch_tab, or close_tab.",
                                "items": { "type": "object" }
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 120000,
                                "description": "Overall runner timeout in milliseconds. Defaults to 60000."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "spawn_agent",
                    "description": "Start a background CN-Codex subagent for delegated investigation, review, testing, or implementation. The subagent runs on the built-in internal subagent engine.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "Complete task instructions for the subagent."
                            },
                            "role": {
                                "type": "string",
                                "description": "Short role label, such as reviewer, tester, researcher, or implementer."
                            },
                            "cwd": {
                                "type": "string",
                                "description": "Optional working directory. Relative paths resolve under the current workspace."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 1800000,
                                "description": "Maximum runtime for the subagent. Defaults to 600000."
                            },
                            "wait": {
                                "type": "boolean",
                                "description": "Wait for completion before returning. Defaults to false."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional model override for the internal subagent engine."
                            },
                            "sandbox": {
                                "type": "string",
                                "enum": ["read-only", "workspace-write", "danger-full-access"],
                                "description": "Compatibility field reserved for Codex parity. Ignored by the current internal subagent runtime."
                            },
                            "dangerously_bypass_approvals_and_sandbox": {
                                "type": "boolean",
                                "description": "Compatibility field reserved for Codex parity. Ignored by the current internal subagent runtime."
                            }
                        },
                        "required": ["prompt"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "wait_agent",
                    "description": "Wait for one or more background subagents started with spawn_agent and return their latest status and output.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "agent_id": {
                                "type": "string",
                                "description": "Single subagent id to wait for."
                            },
                            "agent_ids": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Multiple subagent ids to wait for. If omitted, waits for all running subagents."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 1800000,
                                "description": "Maximum time to wait. Defaults to 60000. Use 0 to return current status immediately."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "send_input",
                    "description": "Send a follow-up message to an existing background subagent. CN-Codex forwards the message to the running internal subagent channel when available and records the submission in subagent input history.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "target": {
                                "type": "string",
                                "description": "Subagent id to message, returned by spawn_agent or list_agents."
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            },
                            "message": {
                                "type": "string",
                                "description": "Plain-text follow-up message for the subagent."
                            },
                            "items": {
                                "type": "array",
                                "description": "Optional structured input items. Text is extracted when message is omitted.",
                                "items": { "type": "object" }
                            },
                            "interrupt": {
                                "type": "boolean",
                                "description": "Compatibility flag for Codex send_input. Currently recorded in input history only and does not force immediate turn interruption."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "resume_agent",
                    "description": "Resume a previously closed, completed, failed, or timed-out background subagent by restarting the internal subagent engine with the same task context and id.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Subagent id to resume, returned by spawn_agent or list_agents."
                            },
                            "target": {
                                "type": "string",
                                "description": "Compatibility alias for id."
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "Compatibility alias for id."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 1800000,
                                "description": "Maximum runtime for the resumed subagent run. Defaults to 600000."
                            },
                            "wait": {
                                "type": "boolean",
                                "description": "Wait for the resumed subagent run to finish before returning. Defaults to false."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_agents",
                    "description": "List background subagents and their statuses.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "status": {
                                "type": "string",
                                "enum": ["running", "completed", "failed", "timed_out", "closed", "interrupted"],
                                "description": "Optional status filter."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "close_agent",
                    "description": "Close a background subagent started with spawn_agent when it is no longer needed. If it is still running, CN-Codex signals cancellation in the internal runtime and returns the previous status.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "target": {
                                "type": "string",
                                "description": "Subagent id to close, returned by spawn_agent or list_agents."
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_list",
                    "description": "List immediate files and directories in the CN-Codex memories store.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Optional relative directory path inside the memories store. Defaults to root."
                            },
                            "max_entries": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 200,
                                "description": "Maximum entries to return. Defaults to 100."
                            },
                            "cursor": {
                                "type": "string",
                                "description": "Opaque cursor from a previous memory_list response."
                            },
                            "format": {
                                "type": "string",
                                "enum": ["text", "json"],
                                "description": "Output format. Defaults to text."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_read",
                    "description": "Read a memory file by relative path from the CN-Codex memories store.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file path."
                            },
                            "line_offset": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "1-indexed starting line. Defaults to 1."
                            },
                            "max_lines": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 500,
                                "description": "Maximum lines to return. Defaults to 200."
                            },
                            "format": {
                                "type": "string",
                                "enum": ["text", "json"],
                                "description": "Output format. Defaults to text."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_search",
                    "description": "Search memory files for a text query and return matching lines.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Text to search for."
                            },
                            "path": {
                                "type": "string",
                                "description": "Optional relative directory or file path to search within."
                            },
                            "case_sensitive": {
                                "type": "boolean",
                                "description": "Whether matching is case-sensitive. Defaults to false."
                            },
                            "max_results": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 50,
                                "description": "Maximum matches to return. Defaults to 20."
                            },
                            "context_lines": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 5,
                                "description": "Number of context lines before and after each match. Defaults to 0."
                            },
                            "cursor": {
                                "type": "string",
                                "description": "Opaque cursor from a previous memory_search response."
                            },
                            "format": {
                                "type": "string",
                                "enum": ["text", "json"],
                                "description": "Output format. Defaults to text."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_write",
                    "description": "Create, overwrite, or append a Markdown memory file. Use only when the user explicitly asks CN-Codex to remember, forget, or update durable information.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file path, usually ending in .md."
                            },
                            "content": {
                                "type": "string",
                                "description": "Markdown content to write."
                            },
                            "append": {
                                "type": "boolean",
                                "description": "Append to the file instead of replacing it. Defaults to false."
                            }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_update",
                    "description": "Replace exact text inside an existing memory file. Use only when the user explicitly asks CN-Codex to update durable memory.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file path."
                            },
                            "old_text": {
                                "type": "string",
                                "description": "Exact text to replace."
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Replacement text. Use an empty string to remove the exact text."
                            },
                            "replace_all": {
                                "type": "boolean",
                                "description": "Replace every occurrence instead of only the first. Defaults to false."
                            }
                        },
                        "required": ["path", "old_text", "new_text"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_forget",
                    "description": "Forget durable memory by deleting a memory file/directory or removing lines containing exact text from a memory file. Use only when the user explicitly asks CN-Codex to forget durable information.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file or directory path."
                            },
                            "match_text": {
                                "type": "string",
                                "description": "Optional exact text. When provided, removes lines containing this text from the file instead of deleting the whole path."
                            },
                            "recursive": {
                                "type": "boolean",
                                "description": "Allow deleting a directory recursively. Defaults to false."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_servers",
                    "description": "List configured MCP servers from codey/config.toml, excluding secret environment values.",
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_status",
                    "description": "Inspect configured MCP server status without revealing secret env values. Optionally probes tools, resources, resource templates, and prompts to report availability and counts.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            },
                            "probe": {
                                "type": "boolean",
                                "description": "Whether to probe enabled servers. Defaults to true."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_tools",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list available MCP tools.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_call_tool",
                    "description": "Call a tool exposed by a configured MCP server.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "MCP server name from mcp_list_servers."
                            },
                            "tool": {
                                "type": "string",
                                "description": "MCP tool name to call."
                            },
                            "arguments": {
                                "type": "object",
                                "description": "JSON object arguments for the MCP tool."
                            }
                        },
                        "required": ["server", "tool"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_resources",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list MCP resources.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_read_resource",
                    "description": "Read a resource by URI from a configured MCP server.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "MCP server name from mcp_list_servers."
                            },
                            "uri": {
                                "type": "string",
                                "description": "Resource URI to read."
                            }
                        },
                        "required": ["server", "uri"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_resource_templates",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list MCP resource templates.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_prompts",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list MCP prompts.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_get_prompt",
                    "description": "Get a prompt by name from a configured MCP server.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "MCP server name from mcp_list_servers."
                            },
                            "prompt": {
                                "type": "string",
                                "description": "Prompt name from mcp_list_prompts."
                            },
                            "arguments": {
                                "type": "object",
                                "description": "Optional JSON object arguments for the MCP prompt."
                            }
                        },
                        "required": ["server", "prompt"]
                    }
                }
            }),
        ];

        if !self.image_generation_is_enabled() {
            tools.retain(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    != Some("image_generate")
            });
        }

        if web_search_enabled {
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "web_search",
                    "description": "Search the web for current information and return concise result titles, snippets, and URLs.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "The search query."
                            },
                            "max_results": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 10,
                                "description": "Maximum number of search results to return. Defaults to 5."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }));
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "web_fetch",
                    "description": "Fetch a web page by URL and return a readable text excerpt with the page title when available.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "The http or https URL to fetch."
                            },
                            "max_chars": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 20000,
                                "description": "Maximum characters of extracted text to return. Defaults to 8000."
                            }
                        },
                        "required": ["url"]
                    }
                }
            }));
        }

        if self.smartbrain_is_active() {
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "smartbrain_search",
                    "description": "Search the Local Knowledge Base (本地知识库) using BM25 relevance ranking. Returns the most relevant documents (experiences and uploaded knowledge) matching the query. For chunk hits, results include bridged previous/current/next context with at least 30 lines of overlap to avoid cut-off sections.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query text."
                            },
                            "top_k": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 20,
                                "description": "Maximum number of results. Defaults to 5."
                            },
                            "domain": {
                                "type": "string",
                                "description": "Optional domain filter for Local Knowledge Base knowledge (for example: database, backend, devops)."
                            },
                            "concept_type": {
                                "type": "string",
                                "description": "Optional OKF type filter, for example Knowledge or Experience."
                            },
                            "tags": {
                                "type": "array",
                                "items": {
                                    "type": "string"
                                },
                                "description": "Optional tag filters. Any matching tag will pass."
                            },
                            "source_type": {
                                "type": "string",
                                "enum": ["knowledge", "experience"],
                                "description": "Optional source type filter."
                            },
                            "source_group": {
                                "type": "string",
                                "description": "Optional source group filter (for example folder import batch)."
                            },
                            "relative_path_prefix": {
                                "type": "string",
                                "description": "Optional relative path prefix filter under knowledge sources."
                            },
                            "source_file": {
                                "type": "string",
                                "description": "Optional exact source file filter."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }));
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "smartbrain_sql_query",
                    "description": "Execute SQL against a Local Knowledge Base-configured database using the built-in SQL runner. Uses saved connection settings (including password) and permission rules. Prefer this over Python/shell database scripts. Do not ask the user for password when the database is already configured.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "database": {
                                "type": "string",
                                "description": "Database display name, physical database name, or alias from Local Knowledge Base settings. Optional when only one database is configured."
                            },
                            "sql": {
                                "type": "string",
                                "description": "A single SQL statement to execute. Prefer SELECT / SHOW / DESCRIBE for reads."
                            },
                            "row_limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 1000,
                                "description": "Maximum rows to return. Defaults to Local Knowledge Base DB settings (usually 200)."
                            },
                            "timeout_sec": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 120,
                                "description": "Query timeout in seconds. Defaults to Local Knowledge Base DB settings (usually 15)."
                            }
                        },
                        "required": ["sql"]
                    }
                }
            }));
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "build_entry_form",
                    "description": "Build a database entry form from Local Knowledge Base schema. Reads table/column metadata, picks the best target table, maps field types to form inputs, and pre-fills known_values. The UI will open a form for the user to complete missing fields and save.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "database": {
                                "type": "string",
                                "description": "Database display name, physical name, or alias. Optional when only one DB is configured."
                            },
                            "table": {
                                "type": "string",
                                "description": "Optional explicit target table. If omitted, the tool matches by intent and known_values."
                            },
                            "intent": {
                                "type": "string",
                                "description": "Natural language intent, for example “采购合同入库” or “今天和华为签了12000元采购合同”."
                            },
                            "known_values": {
                                "type": "object",
                                "description": "Fields already extracted from user text, such as {\"supplier\":\"华为\",\"amount\":12000}. Values are auto-filled into the form.",
                                "additionalProperties": true
                            }
                        },
                        "required": []
                    }
                }
            }));
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "save_form_data",
                    "description": "Save user-confirmed form values into a Local Knowledge Base database table. Validates required fields, checks writeData permission, and executes a parameterized-safe INSERT. Prefer calling this only after build_entry_form and user confirmation.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "database": {
                                "type": "string",
                                "description": "Database display name, physical name, or alias."
                            },
                            "table": {
                                "type": "string",
                                "description": "Target table name."
                            },
                            "values": {
                                "type": "object",
                                "description": "Final field values from the form, keyed by column name.",
                                "additionalProperties": true
                            }
                        },
                        "required": ["table", "values"]
                    }
                }
            }));
        }

        // Recording tools
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "recording_control",
                "description": "Control browser recording for the Record & Replay workflow. Use action='launch_browser' to open an external Chrome, 'show_toggle' to display the recording UI so the user can start/stop recording, 'read_trace' to read a completed recording trace, or 'list_traces' to list all saved recordings.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["launch_browser", "show_toggle", "hide_toggle", "status", "read_trace", "list_traces"],
                            "description": "The recording control action to perform."
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Session ID of the trace to read (required for read_trace action)."
                        }
                    },
                    "required": ["action"]
                }
            }
        }));

        tools
    }

    pub async fn tool_specs_with_mcp(
        &mut self,
        web_search_enabled: bool,
        thread_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        self.tool_specs_for_turn(web_search_enabled, true, thread_id)
            .await
    }

    /// Layered tool schema assembly:
    /// - always include the small core built-in set
    /// - optionally discover MCP catalogs for search/activation
    /// - only attach non-core built-ins / MCP direct tools that have been activated
    pub async fn tool_specs_for_turn(
        &mut self,
        web_search_enabled: bool,
        discover_mcp: bool,
        thread_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        self.web_search_enabled = web_search_enabled;
        if discover_mcp {
            // Discover catalogs for search + activation, but do not attach all schemas yet.
            let _ = self.discover_mcp_direct_tool_specs().await;
        }

        let activated = match thread_id {
            Some(id) => self.activated_tool_names_for_thread(id).await,
            None => BTreeSet::new(),
        };
        let mut tools = Vec::new();
        let mut seen = BTreeSet::new();

        for spec in self.tool_specs(web_search_enabled) {
            let Some(name) = Self::tool_spec_name(&spec) else {
                continue;
            };
            if Self::is_core_tool_name(name) || activated.contains(name) {
                if seen.insert(name.to_string()) {
                    tools.push(spec);
                }
            }
        }

        // Activated MCP direct tools (including Playwright) are attached only after search/activation.
        let mut mcp_aliases = self.mcp_tool_specs.keys().cloned().collect::<Vec<_>>();
        mcp_aliases.sort();
        for alias in mcp_aliases {
            if !activated.contains(&alias) {
                continue;
            }
            if let Some(spec) = self.mcp_tool_specs.get(&alias).cloned() {
                if seen.insert(alias) {
                    tools.push(spec);
                }
            }
        }

        tools
    }

    fn tool_search_entries(&self) -> Vec<ToolSearchEntry> {
        let mut entries = Vec::new();
        for spec in self.tool_specs(self.web_search_enabled) {
            let Some(entry) = tool_search_entry_from_function_spec(&spec, "tool", "built-in", None)
            else {
                continue;
            };
            if entry.name == "tool_search" {
                continue;
            }
            entries.push(entry);
        }

        let mut mcp_aliases = self.mcp_tool_specs.keys().cloned().collect::<Vec<_>>();
        mcp_aliases.sort();
        for alias in mcp_aliases {
            let Some(spec) = self.mcp_tool_specs.get(&alias) else {
                continue;
            };
            let source = self
                .mcp_tool_aliases
                .get(&alias)
                .map(|entry| format!("mcp:{}", entry.server))
                .unwrap_or_else(|| "mcp".to_string());
            if let Some(mut entry) =
                tool_search_entry_from_function_spec(spec, "tool", &source, None)
            {
                if let Some(alias_info) = self.mcp_tool_aliases.get(&alias) {
                    entry
                        .metadata
                        .insert("server".to_string(), alias_info.server.clone());
                    entry
                        .metadata
                        .insert("tool".to_string(), alias_info.tool.clone());
                    if let Some(connector_id) = alias_info.connector.connector_id.clone() {
                        entry
                            .metadata
                            .insert("connectorId".to_string(), connector_id);
                    }
                    if let Some(connector_name) = alias_info.connector.connector_name.clone() {
                        entry
                            .metadata
                            .insert("connectorName".to_string(), connector_name.clone());
                        entry.usage = Some(format!(
                            "Call this function tool directly by name. This MCP tool belongs to the {connector_name} app connector."
                        ));
                    }
                    if let Some(description) = alias_info.connector.namespace_description.clone() {
                        entry
                            .metadata
                            .insert("namespaceDescription".to_string(), description);
                    }
                }
                entries.push(entry);
            }
        }

        // A configured server name is enough for tool discovery. Do not call
        // tools/list here: this offline index lets the model activate the
        // generic MCP tools through tool_search without starting any server.
        let mut configured_servers = self.enabled_mcp_servers();
        configured_servers.sort_by(|left, right| left.name.cmp(&right.name));
        for server in configured_servers {
            let mut metadata = BTreeMap::new();
            metadata.insert("server".to_string(), server.name.clone());
            metadata.insert("transport".to_string(), server.transport.clone());
            entries.push(ToolSearchEntry {
                kind: "mcp_server".to_string(),
                name: format!("MCP server: {}", server.name),
                description: format!(
                    "Configured MCP server '{}'. Activate it only when the request needs its capability; then list tools for this server.",
                    server.name
                ),
                source: format!("mcp:{}", server.name),
                path: None,
                spec: None,
                metadata,
                usage: Some(format!(
                    "The next model call can use mcp_list_tools or mcp_call_tool with server=\"{}\". This server is not connected until one of those tools is called.",
                    server.name
                )),
            });
        }

        entries.extend(local_skill_search_entries(&self.workspace_config_dir));
        entries.extend(plugin_skill_search_entries(&self.workspace_config_dir));
        entries.extend(plugin_app_search_entries(&self.workspace_config_dir));
        entries
    }

    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> AppResult<String> {
        if let Some(alias) = self.mcp_tool_aliases.get(tool_name).cloned() {
            return self
                .exec_mcp_direct_tool(
                    tool_name,
                    &alias.server,
                    &alias.tool,
                    arguments,
                    call_id,
                    app_handle,
                    thread_id,
                )
                .await;
        }

        match tool_name {
            "shell" | "shell_command" => {
                self.exec_shell(tool_name, arguments, call_id, app_handle, thread_id)
                    .await
            }
            "exec_command" => {
                self.exec_command(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "write_stdin" => {
                self.exec_write_stdin(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "close_exec_session" => {
                self.exec_close_exec_session(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "read_file" => {
                self.exec_read_file(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "write_file" => {
                self.exec_write_file(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "tool_search" => {
                self.exec_tool_search(arguments, call_id, app_handle, thread_id, turn_id)
                    .await
            }
            "apps_list" => {
                self.exec_apps_list(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "list_available_plugins_to_install" => {
                self.exec_list_available_plugins_to_install(
                    arguments, call_id, app_handle, thread_id,
                )
                .await
            }
            "request_plugin_install" => {
                self.exec_request_plugin_install(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "plugin_manage" => {
                self.exec_plugin_manage(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "robot_save" => {
                self.exec_robot_save(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "code_review" => {
                self.exec_code_review(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "code_search" => {
                self.exec_code_search(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "apply_patch" => {
                self.exec_apply_patch(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "list_directory" => {
                self.exec_list_dir(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "update_plan" => {
                self.exec_update_plan(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "request_user_input" => {
                self.exec_request_user_input(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "request_permissions" => {
                self.exec_request_permissions(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "view_image" => {
                self.exec_view_image(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "ocr_image" => {
                self.exec_ocr_image(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "image_generate" => {
                self.exec_image_generate(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "echarts_report" => {
                self.exec_echarts_report(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "browser_run" => {
                self.exec_browser_run(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "recording_control" => {
                self.exec_recording_control(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "spawn_agent" => {
                self.exec_spawn_agent(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "wait_agent" => {
                self.exec_wait_agent(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "send_input" => {
                self.exec_send_input(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "resume_agent" => {
                self.exec_resume_agent(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "list_agents" => {
                self.exec_list_agents(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "close_agent" => {
                self.exec_close_agent(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "memory_list" => {
                self.exec_memory_list(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "memory_read" => {
                self.exec_memory_read(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "memory_search" => {
                self.exec_memory_search(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "smartbrain_search" => {
                self.exec_smartbrain_search(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "smartbrain_sql_query" => {
                self.exec_smartbrain_sql_query(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "build_entry_form" => {
                self.exec_build_entry_form(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "save_form_data" => {
                self.exec_save_form_data(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "memory_write" => {
                self.exec_memory_write(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "memory_update" => {
                self.exec_memory_update(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "memory_forget" => {
                self.exec_memory_forget(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_list_servers" => {
                self.exec_mcp_list_servers(call_id, app_handle, thread_id)
                    .await
            }
            "mcp_status" => {
                self.exec_mcp_status(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_list_tools" => {
                self.exec_mcp_list_tools(arguments, call_id, app_handle, thread_id, turn_id)
                    .await
            }
            "mcp_call_tool" => {
                self.exec_mcp_call_tool(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_list_resources" => {
                self.exec_mcp_list_resources(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_read_resource" => {
                self.exec_mcp_read_resource(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_list_resource_templates" => {
                self.exec_mcp_list_resource_templates(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_list_prompts" => {
                self.exec_mcp_list_prompts(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "mcp_get_prompt" => {
                self.exec_mcp_get_prompt(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "web_search" => {
                self.exec_web_search(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "web_fetch" => {
                self.exec_web_fetch(arguments, call_id, app_handle, thread_id)
                    .await
            }
            other => Ok(format!("Unknown tool: {other}")),
        }
    }

    fn emit_tool_start(
        &self,
        app_handle: &AppHandle,
        thread_id: &str,
        call_id: &str,
        tool: &str,
        command: &str,
    ) {
        app_handle
            .emit(
                "tool-exec-start",
                serde_json::json!({
                    "threadId": thread_id,
                    "callId": call_id,
                    "tool": tool,
                    "command": command,
                }),
            )
            .ok();
    }

    fn emit_tool_end(
        &self,
        app_handle: &AppHandle,
        thread_id: &str,
        call_id: &str,
        tool: &str,
        exit_code: i32,
        output: &str,
    ) {
        let truncated_output = if output.len() > 4000 {
            let prefix: String = output.chars().take(2000).collect();
            let suffix: String = output
                .chars()
                .rev()
                .take(1500)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!(
                "{prefix}\n\n... [{} chars truncated] ...\n\n{suffix}",
                output.len() - 3500
            )
        } else {
            output.to_string()
        };
        let payload = serde_json::json!({
            "threadId": thread_id,
            "callId": call_id,
            "tool": tool,
            "exitCode": exit_code,
            "output": truncated_output,
        });
        app_handle.emit("tool-exec-end", payload.clone()).ok();
        crate::mobile_server::broadcast("tool-exec-end", payload);
    }

    fn emit_apply_patch_progress(
        &self,
        app_handle: &AppHandle,
        thread_id: &str,
        call_id: &str,
        changes: &[ApplyPatchProgressChange],
    ) {
        let first_path = changes
            .first()
            .map(|change| change.path.clone())
            .unwrap_or_else(|| "apply_patch".to_string());
        app_handle
            .emit(
                "file-change-patch-updated",
                serde_json::json!({
                    "threadId": thread_id,
                    "callId": call_id,
                    "itemId": call_id,
                    "path": first_path,
                    "changes": changes,
                }),
            )
            .ok();
    }

    async fn exec_shell(
        &self,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ShellArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid {tool_name} args: {e}"))
        })?;

        let cmd_display = shell_command_display(&args.command);
        if cmd_display.trim().is_empty() {
            return Ok("Error: empty command".to_string());
        }
        if cfg!(target_os = "windows") {
            if let Some(msg) = powershell_command_validation_error(&cmd_display) {
                self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
                return Ok(msg);
            }
        }
        if let Some(msg) = shell_file_editing_violation(&cmd_display) {
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

        let workdir = resolve_command_cwd(&self.cwd, args.workdir.as_deref());
        if !workdir.is_dir() {
            let msg = format!(
                "Error: workdir does not exist or is not a directory: {}",
                workdir.display()
            );
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

        if let Err(msg) = validate_shell_permission_args(&args) {
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

        let timeout_ms = match resolve_shell_timeout_ms(&args) {
            Ok(timeout_ms) => timeout_ms,
            Err(msg) => {
                self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
                return Ok(msg);
            }
        };

        info!("Executing shell: {cmd_display}");

        self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);

        if shell_requires_permission_approval(&args)
            && !self
                .additional_permissions_preapproved(
                    args.sandbox_permissions.as_deref(),
                    args.additional_permissions.as_ref(),
                )
                .await
        {
            let request_id = RequestId::String(format!("approval-{call_id}"));
            let reason = args
                .justification
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Approve command permission override");
            let payload = serde_json::json!({
                "requestId": request_id,
                "method": "commandExecution",
                "params": {
                    "command": cmd_display,
                    "cwd": normalize_windows_verbatim_prefix(&workdir.to_string_lossy()),
                    "reason": reason,
                    "sandbox_permissions": args.sandbox_permissions.clone(),
                    "prefix_rule": args.prefix_rule.clone(),
                    "additional_permissions": args.additional_permissions.clone(),
                }
            });
            let _ = app_handle.emit("server-request", payload);
            if let Err(msg) = wait_for_approval_result(app_handle, &request_id, 600_000).await {
                let output = format!("Command rejected: {msg}");
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &output);
                return Ok(output);
            }
        }

        let (program, cmd_args) = if cfg!(target_os = "windows") {
            shell_program_and_args_windows(&cmd_display, args.login)
        } else {
            shell_program_and_args_unix(&cmd_display, args.login)
        };

        let mut cmd = Command::new(&program);
        cmd.args(&cmd_args)
            .current_dir(&workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.no_console();
        let mut child = cmd
            .spawn()
            .map_err(|e| crate::error::AppError::Custom(format!("Failed to spawn command: {e}")))?;

        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));
        let stdout_collector = stdout_buffer.clone();
        let stderr_collector = stderr_buffer.clone();

        let mut stdout_handle = tokio::spawn(async move {
            if let Some(out) = child_stdout {
                collect_shell_stream_bytes(out, stdout_collector).await;
            }
        });
        let mut stderr_handle = tokio::spawn(async move {
            if let Some(err) = child_stderr {
                collect_shell_stream_bytes(err, stderr_collector).await;
            }
        });

        let child = Arc::new(Mutex::new(child));
        self.register_active_tool_process(thread_id, call_id, tool_name, child.clone())
            .await;

        let result = match wait_for_child_with_timeout(&child, timeout_ms).await {
            WaitChildResult::Exited(status) => {
                wait_for_shell_stream_task(&mut stdout_handle, 1_500).await;
                wait_for_shell_stream_task(&mut stderr_handle, 1_500).await;
                let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
                let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
                let exit_code = status.code().unwrap_or(-1);
                let output = format_shell_command_output(exit_code, &stdout, &stderr);
                let truncated = truncate_shell_output(&output, TOOL_OUTPUT_SHELL_MAX_CHARS);
                self.emit_tool_end(
                    app_handle, thread_id, call_id, tool_name, exit_code, &truncated,
                );
                Ok(truncated)
            }
            WaitChildResult::Failed(error) => {
                wait_for_shell_stream_task(&mut stdout_handle, 300).await;
                wait_for_shell_stream_task(&mut stderr_handle, 300).await;
                let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
                let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
                let partial = truncate_shell_output(
                    &format_shell_partial_output(&stdout, &stderr),
                    TOOL_OUTPUT_SHELL_PARTIAL_MAX_CHARS,
                );
                let msg = if partial.trim().is_empty() {
                    format!("Failed to wait for command: {error}")
                } else {
                    format!(
                        "Failed to wait for command: {error}\n\nPartial output captured before failure:\n{partial}"
                    )
                };
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
                Ok(msg)
            }
            WaitChildResult::TimedOut => {
                terminate_shell_child(&child).await;
                wait_for_shell_stream_task(&mut stdout_handle, 1_200).await;
                wait_for_shell_stream_task(&mut stderr_handle, 1_200).await;
                info!("Shell command timed out after {timeout_ms} ms: {cmd_display}");
                let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
                let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
                let partial = truncate_shell_output(
                    &format_shell_partial_output(&stdout, &stderr),
                    TOOL_OUTPUT_SHELL_PARTIAL_MAX_CHARS,
                );
                let msg = if partial.trim().is_empty() {
                    format!(
                        "Command timed out after {timeout_ms} ms.\nThe command '{cmd_display}' did not complete before the deadline and was terminated.\nNo partial output was captured."
                    )
                } else {
                    format!(
                        "Command timed out after {timeout_ms} ms.\nThe command '{cmd_display}' did not complete before the deadline and was terminated.\n\nPartial output captured before termination:\n{partial}"
                    )
                };
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, 124, &msg);
                Ok(msg)
            }
        };

        self.unregister_active_tool_process(thread_id, call_id)
            .await;
        result
    }

    async fn exec_command(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ExecCommandArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid exec_command args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
                return Ok(msg);
            }
        };

        let cmd = args.cmd.trim();
        if cmd.is_empty() {
            let msg = "Error: exec_command cmd must not be empty".to_string();
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", "empty");
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }
        let uses_powershell = cfg!(target_os = "windows")
            && args
                .shell
                .as_deref()
                .map(|shell| {
                    let name = Path::new(shell)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    name.contains("powershell") || name == "pwsh" || name == "pwsh.exe"
                })
                .unwrap_or(true);
        if uses_powershell {
            if let Some(msg) = powershell_command_validation_error(cmd) {
                self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
                return Ok(msg);
            }
        }
        if let Some(msg) = shell_file_editing_violation(cmd) {
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }

        let workdir = resolve_command_cwd(&self.cwd, args.workdir.as_deref());
        if !workdir.is_dir() {
            let msg = format!(
                "Error: workdir does not exist or is not a directory: {}",
                workdir.display()
            );
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }

        if let Err(msg) = validate_exec_permission_args(&args) {
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }

        self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);

        if exec_requires_permission_approval(&args)
            && !self
                .additional_permissions_preapproved(
                    args.sandbox_permissions.as_deref(),
                    args.additional_permissions.as_ref(),
                )
                .await
        {
            let request_id = RequestId::String(format!("approval-{call_id}"));
            let reason = args
                .justification
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Approve command permission override");
            let payload = serde_json::json!({
                "requestId": request_id,
                "method": "commandExecution",
                "params": {
                    "command": cmd,
                    "cwd": normalize_windows_verbatim_prefix(&workdir.to_string_lossy()),
                    "reason": reason,
                    "sandbox_permissions": args.sandbox_permissions.clone(),
                    "prefix_rule": args.prefix_rule.clone(),
                    "additional_permissions": args.additional_permissions.clone(),
                }
            });
            let _ = app_handle.emit("server-request", payload);
            if let Err(msg) = wait_for_approval_result(app_handle, &request_id, 600_000).await {
                let output = format!("Command rejected: {msg}");
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &output);
                return Ok(output);
            }
        }

        let (program, cmd_args) = exec_command_program_and_args(&args);
        let mut spawn_cmd = Command::new(&program);
        spawn_cmd
            .args(&cmd_args)
            .current_dir(&workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        spawn_cmd.no_console();
        let mut child = match spawn_cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let msg = format!("Failed to spawn command: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
                return Ok(msg);
            }
        };

        let session_id = self.next_exec_session_id.fetch_add(1, Ordering::Relaxed);
        let process_id = child.id();
        let output = Arc::new(Mutex::new(String::new()));
        let cursor = Arc::new(Mutex::new(0usize));
        let exit_code = Arc::new(Mutex::new(None));
        let stdin = Arc::new(Mutex::new(child.stdin.take()));

        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(collect_exec_output(stdout, output.clone(), None));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(collect_exec_output(
                stderr,
                output.clone(),
                Some("[stderr]\n"),
            ));
        }

        let exit_code_for_task = exit_code.clone();
        tokio::spawn(async move {
            let code = match child.wait().await {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            };
            let mut guard = exit_code_for_task.lock().await;
            *guard = Some(code);
        });

        let record = ExecSessionRecord {
            id: session_id,
            process_id,
            command: cmd.to_string(),
            cwd: normalize_windows_verbatim_prefix(&workdir.to_string_lossy()),
            started_at_ms: now_millis(),
            output,
            cursor,
            exit_code,
            stdin,
        };

        self.exec_sessions
            .lock()
            .await
            .insert(session_id, record.clone());

        let yield_time = exec_yield_duration(args.yield_time_ms.or(args.timeout_ms), false);
        tokio::time::sleep(yield_time).await;

        let result = exec_session_snapshot(&record, args.max_output_tokens).await;
        if result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .is_some()
        {
            self.exec_sessions.lock().await.remove(&session_id);
        }
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        let exit_code_for_event = result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value as i32)
            .unwrap_or(0);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "exec_command",
            exit_code_for_event,
            &output,
        );
        Ok(output)
    }

    async fn exec_write_stdin(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: WriteStdinArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid write_stdin args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "write_stdin", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
                return Ok(msg);
            }
        };

        let display = format!("session {}", args.session_id);
        self.emit_tool_start(app_handle, thread_id, call_id, "write_stdin", &display);
        let Some(record) = self
            .exec_sessions
            .lock()
            .await
            .get(&args.session_id)
            .cloned()
        else {
            let msg = format!("Unknown exec session: {}", args.session_id);
            self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
            return Ok(msg);
        };

        let wrote_chars = args.chars.as_deref().is_some_and(|chars| !chars.is_empty());
        if let Some(chars) = args.chars.as_deref().filter(|chars| !chars.is_empty()) {
            let mut stdin = record.stdin.lock().await;
            let Some(stdin) = stdin.as_mut() else {
                let msg = format!("Exec session {} is not accepting stdin", args.session_id);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
                return Ok(msg);
            };
            if let Err(e) = stdin.write_all(chars.as_bytes()).await {
                let msg = format!("Failed to write to exec session {}: {e}", args.session_id);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
                return Ok(msg);
            }
        }

        tokio::time::sleep(exec_yield_duration(args.yield_time_ms, wrote_chars)).await;

        let result = exec_session_snapshot(&record, args.max_output_tokens).await;
        if result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .is_some()
        {
            self.exec_sessions.lock().await.remove(&args.session_id);
        }
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        let exit_code_for_event = result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value as i32)
            .unwrap_or(0);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "write_stdin",
            exit_code_for_event,
            &output,
        );
        Ok(output)
    }

    async fn exec_close_exec_session(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: CloseExecSessionArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid close_exec_session args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "close_exec_session",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "close_exec_session",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let display = format!("session {}", args.session_id);
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "close_exec_session",
            &display,
        );
        let Some(record) = self.exec_sessions.lock().await.remove(&args.session_id) else {
            let msg = format!("Unknown exec session: {}", args.session_id);
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "close_exec_session",
                -1,
                &msg,
            );
            return Ok(msg);
        };

        let result = close_exec_session_record(record).await;
        let exit_code_for_event = if result
            .get("closed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            0
        } else {
            -1
        };
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "close_exec_session",
            exit_code_for_event,
            &output,
        );
        Ok(output)
    }

    async fn exec_read_file(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ReadArgs {
            path: String,
            #[serde(default)]
            line_offset: Option<usize>,
            #[serde(default)]
            max_lines: Option<usize>,
            #[serde(default)]
            end_line: Option<usize>,
            #[serde(default)]
            show_line_numbers: Option<bool>,
        }

        let args: ReadArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid read_file args: {e}")))?;

        let full_path = self.cwd.join(&args.path);
        info!("Reading file: {}", full_path.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "read_file", &args.path);

        let result = match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                let formatted = format_read_file_output(
                    &args.path,
                    &content,
                    args.line_offset,
                    args.max_lines,
                    args.end_line,
                    args.show_line_numbers,
                );
                let truncated = truncate_output(&formatted, TOOL_OUTPUT_READ_FILE_MAX_CHARS);
                self.emit_tool_end(app_handle, thread_id, call_id, "read_file", 0, &truncated);
                truncated
            }
            Err(e) => {
                let msg = format!("Error reading {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "read_file", -1, &msg);
                msg
            }
        };
        Ok(result)
    }

    async fn exec_write_file(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct WriteArgs {
            path: String,
            content: String,
        }

        let args: WriteArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid write_file args: {e}")))?;

        let full_path = self.cwd.join(&args.path);
        info!("Writing file: {}", full_path.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "write_file", &args.path);

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let result = match tokio::fs::write(&full_path, &args.content).await {
            Ok(()) => {
                let msg = format!(
                    "Successfully wrote {} bytes to {}",
                    args.content.len(),
                    args.path
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", 0, &msg);
                msg
            }
            Err(e) => {
                let msg = format!("Error writing {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                msg
            }
        };
        Ok(result)
    }

    async fn exec_apply_patch(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let patch = match extract_patch_argument(arguments) {
            Ok(patch) => patch,
            Err(msg) => {
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "apply_patch",
                    "invalid patch",
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "apply_patch", -1, &msg);
                return Err(crate::error::AppError::Custom(msg));
            }
        };

        let display_label = patch_display_label(&patch);
        info!("Applying patch: {}", display_label);
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "apply_patch",
            &display_label,
        );
        let progress_changes = parse_patch_actions(&patch)
            .map(|actions| apply_patch_progress_changes(&actions))
            .unwrap_or_default();

        let result = match apply_patch_to_workspace(&self.cwd, &patch) {
            Ok(report) => {
                let msg = format_apply_patch_report(&report);
                // 仅在真正写盘成功后再广播 progress，避免失败时 UI 误显示“已修改”。
                if !progress_changes.is_empty() {
                    self.emit_apply_patch_progress(
                        app_handle,
                        thread_id,
                        call_id,
                        &progress_changes,
                    );
                }
                self.emit_tool_end(app_handle, thread_id, call_id, "apply_patch", 0, &msg);
                msg
            }
            Err(err) => {
                let msg = format!(
                    "Error applying patch: {err}\nDo not fall back to Python, PowerShell, sed, or other shell-based file editing. Correct the patch path or context and retry apply_patch so text encoding is preserved."
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "apply_patch", -1, &msg);
                return Err(crate::error::AppError::Custom(msg));
            }
        };

        Ok(result)
    }

    async fn exec_list_dir(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct ListArgs {
            #[serde(default)]
            path: Option<String>,
        }

        let args: ListArgs = serde_json::from_str(arguments).unwrap_or_default();
        let dir = match args.path {
            Some(ref p) if !p.is_empty() => self.cwd.join(p),
            _ => self.cwd.clone(),
        };

        let display_path = args.path.as_deref().unwrap_or(".");
        info!("Listing directory: {}", dir.display());

        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "list_directory",
            display_path,
        );

        let mut entries = Vec::new();
        match tokio::fs::read_dir(&dir).await {
            Ok(mut reader) => {
                while let Ok(Some(entry)) = reader.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    entries.push(if is_dir { format!("{name}/") } else { name });
                }
                entries.sort();
                let output = entries.join("\n");
                self.emit_tool_end(app_handle, thread_id, call_id, "list_directory", 0, &output);
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Error listing {}: {e}", dir.display());
                self.emit_tool_end(app_handle, thread_id, call_id, "list_directory", -1, &msg);
                Ok(msg)
            }
        }
    }

    async fn exec_update_plan(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: PlanUpdateArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid update_plan args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "update_plan", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "update_plan", -1, &msg);
                return Ok(msg);
            }
        };

        let display = format!("{} steps", args.plan.len());
        self.emit_tool_start(app_handle, thread_id, call_id, "update_plan", &display);

        let result = format_plan_update(args.explanation.as_deref(), &args.plan);
        let (exit_code, output) = match result {
            Ok(output) => (0, output),
            Err(msg) => (-1, msg),
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "update_plan",
            exit_code,
            &output,
        );
        Ok(output)
    }

    async fn exec_echarts_report(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: EchartsReportArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid echarts_report args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "echarts_report", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "echarts_report", -1, &msg);
                return Ok(msg);
            }
        };

        let display = echarts_report_display(&args);
        self.emit_tool_start(app_handle, thread_id, call_id, "echarts_report", &display);

        let result = format_echarts_report(&args);
        let (exit_code, output) = match result {
            Ok(output) => (0, output),
            Err(msg) => (-1, msg),
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "echarts_report",
            exit_code,
            &output,
        );
        Ok(output)
    }

    async fn exec_request_user_input(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let mut args: RequestUserInputArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid request_user_input args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let display = format!("{} question(s)", args.questions.len());
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "request_user_input",
            &display,
        );

        if let Err(msg) = validate_request_user_input_args(&args) {
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_user_input",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        normalize_request_user_input_args(&mut args);

        let request_id = RequestId::String(call_id.to_string());
        app_handle
            .emit(
                "server-request",
                serde_json::json!({
                    "requestId": call_id,
                    "id": call_id,
                    "method": "request_user_input",
                    "params": {
                        "threadId": thread_id,
                        "callId": call_id,
                        "questions": args.questions,
                    },
                }),
            )
            .ok();

        let output = match wait_for_approval_result(app_handle, &request_id, 600_000).await {
            Ok(result) => {
                if let Some(profile) = granted_permissions_from_result(&result) {
                    self.remember_permission_grant(profile).await;
                }
                let output =
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    0,
                    &output,
                );
                output
            }
            Err(msg) => {
                let output = format!("request_user_input failed: {msg}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_user_input",
                    -1,
                    &output,
                );
                output
            }
        };

        Ok(output)
    }

    async fn exec_request_permissions(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: RequestPermissionsArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid request_permissions args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let display = args
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("permissions");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "request_permissions",
            display,
        );

        if let Err(msg) = validate_request_permissions_args(&args) {
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_permissions",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        let request_id = RequestId::String(call_id.to_string());
        app_handle
            .emit(
                "server-request",
                serde_json::json!({
                    "requestId": call_id,
                    "id": call_id,
                    "method": "request_permissions",
                    "params": {
                        "threadId": thread_id,
                        "callId": call_id,
                        "environmentId": args.environment_id,
                        "startedAtMs": now_millis(),
                        "reason": args.reason,
                        "permissions": args.permissions,
                        "cwd": self.cwd.to_string_lossy(),
                    },
                }),
            )
            .ok();

        let output = match wait_for_approval_result(app_handle, &request_id, 600_000).await {
            Ok(result) => {
                let output =
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    0,
                    &output,
                );
                output
            }
            Err(msg) => {
                let output = format!("request_permissions failed: {msg}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_permissions",
                    -1,
                    &output,
                );
                output
            }
        };

        Ok(output)
    }

    async fn exec_view_image(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ViewImageArgs {
            path: String,
        }

        let args: ViewImageArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid view_image args: {e}")))?;
        self.emit_tool_start(app_handle, thread_id, call_id, "view_image", &args.path);

        let full_path = match resolve_view_image_path(&self.cwd, &args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "view_image", -1, &msg);
                return Ok(msg);
            }
        };

        let bytes = match tokio::fs::read(&full_path).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let msg = format!("Error reading image {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "view_image", -1, &msg);
                return Ok(msg);
            }
        };

        let info = match inspect_image_bytes(&bytes) {
            Ok(info) => info,
            Err(msg) => {
                let msg = format!("Error inspecting image {}: {msg}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "view_image", -1, &msg);
                return Ok(msg);
            }
        };

        let absolute_path = full_path.canonicalize().unwrap_or(full_path);
        let output = format!(
            "Viewed image: {}\nAbsolute path: {}\nFormat: {}\nMIME: {}\nDimensions: {}x{}\nSize: {} bytes",
            args.path,
            absolute_path.display(),
            info.format,
            info.mime,
            info.width,
            info.height,
            bytes.len()
        );
        self.emit_tool_end(app_handle, thread_id, call_id, "view_image", 0, &output);
        Ok(output)
    }

    async fn exec_ocr_image(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct OcrImageArgs {
            path: String,
        }

        let args: OcrImageArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid ocr_image args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "ocr_image", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", -1, &msg);
                return Ok(msg);
            }
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "ocr_image", &args.path);

        let full_path = match resolve_view_image_path(&self.cwd, &args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", -1, &msg);
                return Ok(msg);
            }
        };

        let absolute_path = full_path.canonicalize().unwrap_or(full_path.clone());
        let output = match crate::ocr::extract_text_from_image_file(&self.cwd, &full_path) {
            Ok(text) if !text.trim().is_empty() => format!(
                "OCR image: {}\nAbsolute path: {}\nEngine: PP-OCRv5 mobile (ONNX Runtime)\nText:\n{}",
                args.path,
                absolute_path.display(),
                text.trim()
            ),
            Ok(_) => format!(
                "OCR image: {}\nAbsolute path: {}\nEngine: PP-OCRv5 mobile (ONNX Runtime)\nText:\n<empty>",
                args.path,
                absolute_path.display()
            ),
            Err(err) => {
                let msg = format!("OCR image failed for {}: {}", absolute_path.display(), err);
                self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", -1, &msg);
                return Ok(msg);
            }
        };

        self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", 0, &output);
        Ok(output)
    }

    async fn exec_image_generate(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ImageGenerateArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid image_generate args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "image_generate", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let display = image_generate_display(&args);
        self.emit_tool_start(app_handle, thread_id, call_id, "image_generate", &display);

        let prompt = args.prompt.trim();
        if prompt.is_empty() {
            let msg = "image_generate prompt must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }

        let image_defaults = self.read_image_generation_settings();
        if !image_defaults.enabled {
            let msg = "image_generate is disabled in Settings > 文生图. Enable the 文生图开关 to use this tool.".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }
        let api_key = match image_generation_api_key(image_defaults.api_key.as_deref()) {
            Some(value) => value,
            None => {
                let msg = "image_generate requires an API key. Configure it in Settings > 文生图, or set CN_CODEX_IMAGE_API_KEY / OPENAI_API_KEY.".to_string();
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let model = image_generation_model(args.model.as_deref(), image_defaults.model.as_deref());
        let api_url =
            image_generation_api_url(args.base_url.as_deref(), image_defaults.base_url.as_deref());
        let body = image_generation_request_body(
            prompt,
            &model,
            args.size.as_deref(),
            args.quality.as_deref(),
            args.background.as_deref(),
            args.n,
        );

        let response = match self
            .http
            .post(&api_url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                let msg = format!("image_generate request failed: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let msg = format_image_generation_http_error(status.as_u16(), &text);
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }

        let parsed: ImageGenerationResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(e) => {
                let msg = format!("image_generate returned invalid JSON: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let items = parsed.data;
        if items.is_empty() {
            let msg = "image_generate response did not contain any images".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }

        let image_count = items.len();
        let mut saved_images = Vec::with_capacity(image_count);

        for (index, item) in items.into_iter().enumerate() {
            let bytes = if let Some(b64) = item
                .b64_json
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                match decode_image_base64(b64) {
                    Ok(bytes) => bytes,
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "image_generate",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            } else if let Some(url) = item.url.as_deref().filter(|value| !value.trim().is_empty()) {
                match self.fetch_generated_image_url(url, &api_url).await {
                    Ok(bytes) => bytes,
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "image_generate",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            } else {
                let msg = format!(
                    "image_generate response item {} did not include b64_json or url",
                    index + 1
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            };

            let info = match inspect_image_bytes(&bytes) {
                Ok(info) => info,
                Err(msg) => {
                    let msg = format!("image_generate returned an invalid image: {msg}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                    return Ok(msg);
                }
            };

            let extension = image_extension_for_info(&info);
            let output_path = match resolve_image_generate_output_path_for_index(
                &self.cwd,
                &self.workspace_config_dir,
                args.output_path.as_deref(),
                call_id,
                prompt,
                extension,
                index,
                image_count,
            ) {
                Ok(path) => path,
                Err(msg) => {
                    self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                    return Ok(msg);
                }
            };

            if let Some(parent) = output_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    let msg = format!(
                        "Error creating image output directory {}: {e}",
                        parent.display()
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                    return Ok(msg);
                }
            }

            if let Err(e) = tokio::fs::write(&output_path, &bytes).await {
                let msg = format!(
                    "Error writing generated image {}: {e}",
                    output_path.display()
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }

            let absolute_path = output_path.canonicalize().unwrap_or(output_path.clone());
            let display_path = workspace_relative_display_path(&self.cwd, &absolute_path)
                .unwrap_or_else(|| absolute_path.to_string_lossy().to_string());
            let revised_prompt = item
                .revised_prompt
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            saved_images.push(GeneratedImageOutput {
                index: index + 1,
                output_path: display_path,
                absolute_path: absolute_path.to_string_lossy().to_string(),
                format: info.format.to_string(),
                mime: info.mime.to_string(),
                dimensions: format!("{}x{}", info.width, info.height),
                width: info.width,
                height: info.height,
                size: format!("{} bytes", bytes.len()),
                size_bytes: bytes.len(),
                revised_prompt,
            });
        }

        let first = saved_images.first().expect("checked non-empty image data");
        let mut output = format!(
            "Generated {}\nPrompt: {}\nModel: {}\nOutput path: {}\nAbsolute path: {}\nFormat: {}\nMIME: {}\nDimensions: {}\nSize: {}",
            if saved_images.len() == 1 {
                "image".to_string()
            } else {
                format!("{} images", saved_images.len())
            },
            prompt,
            model,
            first.output_path,
            first.absolute_path,
            first.format,
            first.mime,
            first.dimensions,
            first.size
        );
        if let Some(revised_prompt) = first.revised_prompt.as_deref() {
            output.push_str("\nRevised prompt: ");
            output.push_str(revised_prompt);
        }
        output.push_str("\nImages JSON:\n");
        output.push_str(
            &serde_json::to_string_pretty(&serde_json::json!({ "images": saved_images }))
                .unwrap_or_else(|_| "{\"images\":[]}".to_string()),
        );

        self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", 0, &output);
        Ok(output)
    }

    async fn fetch_generated_image_url(
        &self,
        url: &str,
        reference_endpoint: &str,
    ) -> Result<Vec<u8>, String> {
        let resolved_url = Self::resolve_generated_image_url(url, reference_endpoint)?;
        let response = self.http.get(&resolved_url).send().await.map_err(|e| {
            format!("image_generate could not fetch generated image URL {resolved_url}: {e}")
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "image_generate image URL fetch failed with HTTP {} ({resolved_url})",
                status.as_u16(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("image_generate could not read generated image bytes: {e}"))?;
        Ok(bytes.to_vec())
    }

    fn resolve_generated_image_url(url: &str, reference_endpoint: &str) -> Result<String, String> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err("image_generate response contained an empty image URL".to_string());
        }

        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Ok(trimmed.to_string());
        }

        let base = reqwest::Url::parse(reference_endpoint).map_err(|e| {
            format!(
                "image_generate could not parse API endpoint '{reference_endpoint}' while resolving image URL '{trimmed}': {e}"
            )
        })?;

        base.join(trimmed).map(|value| value.to_string()).map_err(|e| {
            format!(
                "image_generate returned relative image URL '{trimmed}' that could not be resolved from '{reference_endpoint}': {e}"
            )
        })
    }

    async fn exec_browser_run(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let mut payload: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(value) => value,
            Err(e) => {
                let msg = format!("Invalid browser_run args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "browser_run", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "browser_run", -1, &msg);
                return Ok(msg);
            }
        };

        if !payload.is_object() {
            let msg = "Invalid browser_run args: expected a JSON object".to_string();
            self.emit_tool_start(app_handle, thread_id, call_id, "browser_run", "invalid");
            self.emit_tool_end(app_handle, thread_id, call_id, "browser_run", -1, &msg);
            return Ok(msg);
        }

        apply_browser_defaults(&self.workspace_config_dir, &mut payload);

        let display = browser_run_display(&payload);
        self.emit_tool_start(app_handle, thread_id, call_id, "browser_run", &display);

        let timeout_ms = crate::browser_automation::browser_run_timeout_ms(&payload);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.register_active_browser_cancellation(thread_id, call_id, cancel_flag.clone())
            .await;

        // Try external browser first if available, fall back to embedded WebView.
        let external_endpoint = {
            let state = app_handle.state::<crate::state::AppState>();
            state.external_browser.get_cdp_endpoint().await
        };

        let outcome = if let Some(ref ext_endpoint) = external_endpoint {
            tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                crate::browser_automation::run_external_browser(
                    ext_endpoint,
                    &self.workspace_config_dir,
                    &self.cwd,
                    self.http.clone(),
                    &payload,
                    cancel_flag.clone(),
                ),
            )
            .await
        } else {
            tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                crate::browser_automation::run_webview_js_injection(
                    app_handle,
                    &self.workspace_config_dir,
                    &self.cwd,
                    self.http.clone(),
                    &payload,
                    cancel_flag.clone(),
                ),
            )
            .await
        };

        self.unregister_active_browser_cancellation(thread_id, call_id)
            .await;

        let (exit_code, output) = match outcome {
            Ok(Ok(result)) => {
                let output = serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    serde_json::json!({
                        "ok": true,
                        "browserMode": "tauri-webview-js-injection",
                        "actions": [],
                        "screenshots": [],
                        "assetBundles": [],
                        "tabs": []
                    })
                    .to_string()
                });
                (0, output)
            }
            Ok(Err(error)) => {
                let (error_code, hint) = classify_browser_run_error(&error);
                (-1, browser_run_failure_json(error_code, &error, Some(hint)))
            }
            Err(_) => (
                124,
                browser_run_failure_json(
                    "WEBVIEW_RUN_TIMEOUT",
                    &format!("WebView browser run timed out after {timeout_ms} ms"),
                    Some("请缩小动作批次、减少 wait_for_timeout，或检查页面是否卡死。"),
                ),
            ),
        };

        let truncated = truncate_output(&output, TOOL_OUTPUT_BROWSER_MAX_CHARS);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "browser_run",
            exit_code,
            &truncated,
        );
        Ok(truncated)
    }

    async fn exec_recording_control(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            session_id: Option<String>,
        }

        let args: Args = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("Invalid recording_control args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "recording_control",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "recording_control",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "recording_control",
            &args.action,
        );

        let state = app_handle.state::<crate::state::AppState>();
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let result = match args.action.as_str() {
            "launch_browser" => {
                match state.external_browser.launch(&http, None, None).await {
                    Ok(endpoint) => {
                        // Also show the recording toggle
                        app_handle.emit("recording-toggle-visibility", true).ok();
                        serde_json::json!({
                            "ok": true,
                            "cdpEndpoint": endpoint,
                            "message": "External Chrome launched. The recording toggle is now visible. Tell the user to click 'Start Recording', perform their actions in Chrome, then click 'Stop Recording'."
                        })
                        .to_string()
                    }
                    Err(e) => serde_json::json!({ "ok": false, "error": e }).to_string(),
                }
            }
            "show_toggle" => {
                app_handle.emit("recording-toggle-visibility", true).ok();
                serde_json::json!({
                    "ok": true,
                    "message": "Recording toggle is now visible in the UI."
                })
                .to_string()
            }
            "hide_toggle" => {
                app_handle.emit("recording-toggle-visibility", false).ok();
                serde_json::json!({
                    "ok": true,
                    "message": "Recording toggle hidden."
                })
                .to_string()
            }
            "status" => {
                let recording_status = state.recorder.get_status().await;
                let browser_running = state.external_browser.is_running(&http).await;
                serde_json::json!({
                    "ok": true,
                    "recordingStatus": recording_status,
                    "browserRunning": browser_running,
                })
                .to_string()
            }
            "read_trace" => {
                let session_id = match &args.session_id {
                    Some(id) => id.clone(),
                    None => {
                        let msg = "session_id is required for read_trace action";
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "recording_control",
                            -1,
                            msg,
                        );
                        return Ok(msg.to_string());
                    }
                };
                let trace_path = self
                    .workspace_config_dir
                    .join("recordings")
                    .join(format!("{session_id}.trace.json"));
                match tokio::fs::read_to_string(&trace_path).await {
                    Ok(content) => content,
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "error": format!("Failed to read trace: {e}")
                    })
                    .to_string(),
                }
            }
            "list_traces" => {
                let recordings_dir = self.workspace_config_dir.join("recordings");
                match crate::recording::Recorder::list_traces(&recordings_dir).await {
                    Ok(traces) => serde_json::json!({ "ok": true, "traces": traces }).to_string(),
                    Err(e) => serde_json::json!({ "ok": false, "error": e }).to_string(),
                }
            }
            other => serde_json::json!({
                "ok": false,
                "error": format!("Unknown recording_control action: {other}")
            })
            .to_string(),
        };

        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "recording_control",
            0,
            &result,
        );
        Ok(result)
    }

    async fn exec_spawn_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: SpawnAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid spawn_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "spawn_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "spawn_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let prompt = args.prompt.trim();
        let role = args
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("agent")
            .to_string();
        self.emit_tool_start(app_handle, thread_id, call_id, "spawn_agent", &role);

        if prompt.is_empty() {
            let msg = "spawn_agent prompt must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "spawn_agent", -1, &msg);
            return Ok(msg);
        }

        let cwd = match resolve_subagent_cwd(&self.cwd, args.cwd.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "spawn_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let timeout_ms = args.timeout_ms.unwrap_or(600_000).clamp(1_000, 1_800_000);
        let wait = args.wait.unwrap_or(false);
        let id = format!("agent-{}", uuid::Uuid::new_v4().simple());
        let started_at_ms = now_millis();

        let provider_config = self.subagent_provider_config.lock().await.clone();
        let model = args
            .model
            .as_deref()
            .unwrap_or(&provider_config.model)
            .to_string();
        let system_prompt = format!(
            "{}\n\nYou are a sub-agent with role: {}. Your working directory is: {}",
            provider_config.system_prompt_prefix,
            role,
            cwd.display()
        );

        let record = SubagentRecord {
            id: id.clone(),
            role: role.clone(),
            status: "running".to_string(),
            prompt: prompt.to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            command: format!("[internal:{}]", model),
            process_id: None,
            started_at_ms,
            completed_at_ms: None,
            duration_ms: None,
            exit_code: None,
            output: None,
            error: None,
            input_history: Vec::new(),
            last_input_at_ms: None,
        };

        {
            let mut subagents = self.subagents.lock().await;
            subagents.insert(id.clone(), record.clone());
        }
        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;

        let subagent_config = crate::subagent_engine::SubagentConfig {
            base_url: provider_config.base_url.clone(),
            api_key: provider_config.api_key.clone(),
            model,
            wire_api: provider_config.wire_api.clone(),
            system_prompt,
            cwd,
            timeout_ms,
            max_iterations: 25,
            max_output_tokens: provider_config.max_output_tokens,
        };

        let tool_executor_for_subagent = Arc::new(tokio::sync::RwLock::new(
            ToolExecutor::with_workspace_config_dir(
                subagent_config.cwd.clone(),
                self.workspace_config_dir.clone(),
            ),
        ));

        let (handle, result_rx) = crate::subagent_engine::spawn_subagent(
            subagent_config,
            prompt.to_string(),
            tool_executor_for_subagent,
            app_handle.clone(),
            thread_id.to_string(),
            id.clone(),
        );

        self.subagent_handles
            .lock()
            .await
            .insert(id.clone(), handle);

        let (output, exit_code) = if wait {
            let result =
                wait_for_subagents(self.subagents.clone(), vec![id.clone()], timeout_ms).await;
            let exit_code = if result.has_missing || result.has_failed {
                -1
            } else {
                0
            };
            (result.output, exit_code)
        } else {
            (format_subagent_records(&[record], false), 0)
        };

        // Spawn a background task to update the record when the subagent finishes
        let subagents_clone = self.subagents.clone();
        let workspace_config_dir = self.workspace_config_dir.clone();
        let subagent_handles_clone = self.subagent_handles.clone();
        let finished_id = id.clone();
        tokio::spawn(async move {
            if let Ok(result) = result_rx.await {
                let (status, output_val, error_val, exit_code) = match result.status {
                    crate::subagent_engine::SubagentStatus::Completed { output } => {
                        ("completed", Some(output), None, Some(0))
                    }
                    crate::subagent_engine::SubagentStatus::Failed { error } => {
                        ("failed", None, Some(error), Some(1))
                    }
                    crate::subagent_engine::SubagentStatus::TimedOut => (
                        "timed_out",
                        None,
                        Some("Subagent timed out".to_string()),
                        Some(124),
                    ),
                    crate::subagent_engine::SubagentStatus::Cancelled => {
                        ("closed", None, None, None)
                    }
                    crate::subagent_engine::SubagentStatus::Running => {
                        ("running", None, None, None)
                    }
                };
                let completed_at_ms = now_millis();
                {
                    let mut subagents = subagents_clone.lock().await;
                    if let Some(record) = subagents.get_mut(&finished_id) {
                        record.status = status.to_string();
                        record.completed_at_ms = Some(completed_at_ms);
                        record.duration_ms = Some(result.duration_ms as i64);
                        record.exit_code = exit_code;
                        record.output = output_val;
                        record.error = error_val;
                    }
                }
                persist_subagent_records(&workspace_config_dir, &subagents_clone).await;
                subagent_handles_clone.lock().await.remove(&finished_id);
            }
        });

        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "spawn_agent",
            exit_code,
            &output,
        );
        Ok(output)
    }

    async fn exec_wait_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: WaitAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid wait_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "wait_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "wait_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let ids = collect_wait_agent_ids(&self.subagents, args.agent_id, args.agent_ids).await;
        let display = if ids.is_empty() {
            "agents".to_string()
        } else if ids.len() == 1 {
            ids[0].clone()
        } else {
            format!("{} agents", ids.len())
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "wait_agent", &display);

        if ids.is_empty() {
            let msg = "No subagents found to wait for".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "wait_agent", -1, &msg);
            return Ok(msg);
        }

        let timeout_ms = args.timeout_ms.unwrap_or(60_000).clamp(0, 1_800_000);
        let result = wait_for_subagents(self.subagents.clone(), ids, timeout_ms).await;
        let exit_code = if result.has_missing { -1 } else { 0 };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "wait_agent",
            exit_code,
            &result.output,
        );
        Ok(result.output)
    }

    async fn exec_send_input(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: SendInputArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid send_input args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "send_input", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                return Ok(msg);
            }
        };

        let target = match send_input_target(&args) {
            Some(target) => target,
            None => {
                let msg = "send_input target must not be empty".to_string();
                self.emit_tool_start(app_handle, thread_id, call_id, "send_input", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                return Ok(msg);
            }
        };
        let message = match send_input_message(&args) {
            Ok(message) => message,
            Err(msg) => {
                self.emit_tool_start(app_handle, thread_id, call_id, "send_input", &target);
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                return Ok(msg);
            }
        };

        self.emit_tool_start(app_handle, thread_id, call_id, "send_input", &target);

        // Check if subagent exists
        let agent_status = {
            let subagents = self.subagents.lock().await;
            subagents.get(&target).map(|r| r.status.clone())
        };

        match agent_status {
            None => {
                let msg = format!("No subagent found with id: {target}");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                Ok(msg)
            }
            Some(status) if status != "running" => {
                let msg = format!("Subagent {target} is not running (status: {status})");
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", -1, &msg);
                Ok(msg)
            }
            Some(_) => {
                let delivered = {
                    let handles = self.subagent_handles.lock().await;
                    if let Some(handle) = handles.get(&target) {
                        handle.input_tx.send(message.clone()).await.is_ok()
                    } else {
                        false
                    }
                };

                // Record in input history
                let submission_id = uuid::Uuid::new_v4().to_string();
                {
                    let mut subagents = self.subagents.lock().await;
                    if let Some(record) = subagents.get_mut(&target) {
                        record.input_history.push(SubagentInputRecord {
                            submission_id: submission_id.clone(),
                            message: message.clone(),
                            submitted_at_ms: now_millis(),
                            interrupt: args.interrupt.unwrap_or(false),
                            delivered_to_stdin: delivered,
                        });
                        record.last_input_at_ms = Some(now_millis());
                    }
                }
                persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;

                let result = SendInputResult {
                    target: target.clone(),
                    submission_id,
                    status: "running".to_string(),
                    delivered_to_stdin: delivered,
                    queued: !delivered,
                    note: if delivered {
                        "Message delivered to subagent".to_string()
                    } else {
                        "Message queued (subagent channel unavailable)".to_string()
                    },
                };
                let output = serde_json::to_string_pretty(&result).unwrap_or_default();
                self.emit_tool_end(app_handle, thread_id, call_id, "send_input", 0, &output);
                Ok(output)
            }
        }
    }

    async fn exec_resume_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ResumeAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid resume_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "resume_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let target = match resume_agent_target(&args) {
            Some(target) => target,
            None => {
                let msg = "resume_agent id must not be empty".to_string();
                self.emit_tool_start(app_handle, thread_id, call_id, "resume_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", -1, &msg);
                return Ok(msg);
            }
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "resume_agent", &target);

        let timeout_ms = args.timeout_ms.unwrap_or(600_000).clamp(1_000, 1_800_000);

        // Get the original prompt and check if it can be resumed
        let (previous_status, prompt, cwd_str) = {
            let subagents = self.subagents.lock().await;
            match subagents.get(&target) {
                None => {
                    let msg = format!("No subagent found with id: {target}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", -1, &msg);
                    return Ok(msg);
                }
                Some(record) => {
                    if record.status == "running" {
                        let msg = format!("Subagent {target} is already running");
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "resume_agent",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                    (
                        record.status.clone(),
                        record.prompt.clone(),
                        record.cwd.clone(),
                    )
                }
            }
        };

        let provider_config = self.subagent_provider_config.lock().await.clone();
        let cwd = PathBuf::from(&cwd_str);
        let system_prompt = format!(
            "{}\n\nYou are a resumed sub-agent. Your working directory is: {}",
            provider_config.system_prompt_prefix,
            cwd.display()
        );

        let subagent_config = crate::subagent_engine::SubagentConfig {
            base_url: provider_config.base_url.clone(),
            api_key: provider_config.api_key.clone(),
            model: provider_config.model.clone(),
            wire_api: provider_config.wire_api.clone(),
            system_prompt,
            cwd: cwd.clone(),
            timeout_ms,
            max_iterations: 25,
            max_output_tokens: provider_config.max_output_tokens,
        };

        // Update status to running
        {
            let mut subagents = self.subagents.lock().await;
            if let Some(record) = subagents.get_mut(&target) {
                record.status = "running".to_string();
                record.started_at_ms = now_millis();
                record.completed_at_ms = None;
                record.duration_ms = None;
                record.exit_code = None;
                record.output = None;
                record.error = None;
            }
        }
        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;

        let tool_executor_for_subagent = Arc::new(tokio::sync::RwLock::new(
            ToolExecutor::with_workspace_config_dir(cwd, self.workspace_config_dir.clone()),
        ));

        let (handle, result_rx) = crate::subagent_engine::spawn_subagent(
            subagent_config,
            prompt.clone(),
            tool_executor_for_subagent,
            app_handle.clone(),
            thread_id.to_string(),
            target.clone(),
        );

        self.subagent_handles
            .lock()
            .await
            .insert(target.clone(), handle);

        // Spawn background updater
        let subagents_clone = self.subagents.clone();
        let workspace_config_dir = self.workspace_config_dir.clone();
        let subagent_handles_clone = self.subagent_handles.clone();
        let finished_id = target.clone();
        tokio::spawn(async move {
            if let Ok(result) = result_rx.await {
                let (status, output_val, error_val, exit_code) = match result.status {
                    crate::subagent_engine::SubagentStatus::Completed { output } => {
                        ("completed", Some(output), None, Some(0))
                    }
                    crate::subagent_engine::SubagentStatus::Failed { error } => {
                        ("failed", None, Some(error), Some(1))
                    }
                    crate::subagent_engine::SubagentStatus::TimedOut => (
                        "timed_out",
                        None,
                        Some("Subagent timed out".to_string()),
                        Some(124),
                    ),
                    crate::subagent_engine::SubagentStatus::Cancelled => {
                        ("closed", None, None, None)
                    }
                    crate::subagent_engine::SubagentStatus::Running => {
                        ("running", None, None, None)
                    }
                };
                let completed_at_ms = now_millis();
                {
                    let mut subagents = subagents_clone.lock().await;
                    if let Some(record) = subagents.get_mut(&finished_id) {
                        record.status = status.to_string();
                        record.completed_at_ms = Some(completed_at_ms);
                        record.duration_ms = Some(result.duration_ms as i64);
                        record.exit_code = exit_code;
                        record.output = output_val;
                        record.error = error_val;
                    }
                }
                persist_subagent_records(&workspace_config_dir, &subagents_clone).await;
                subagent_handles_clone.lock().await.remove(&finished_id);
            }
        });

        let result = ResumeAgentResult {
            id: target.clone(),
            resumed: true,
            previous_status,
            status: "running".to_string(),
            note: "Subagent resumed with internal engine".to_string(),
            agent: None,
        };

        let output = if args.wait.unwrap_or(false) {
            let wait =
                wait_for_subagents(self.subagents.clone(), vec![target.clone()], timeout_ms).await;
            wait.output
        } else {
            serde_json::to_string_pretty(&result).unwrap_or_default()
        };
        self.emit_tool_end(app_handle, thread_id, call_id, "resume_agent", 0, &output);
        Ok(output)
    }

    async fn exec_list_agents(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ListAgentsArgs = serde_json::from_str(arguments).unwrap_or_default();
        let status_filter = args
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let display = status_filter.as_deref().unwrap_or("agents");
        self.emit_tool_start(app_handle, thread_id, call_id, "list_agents", display);

        let mut records = self
            .subagents
            .lock()
            .await
            .values()
            .filter(|record| {
                status_filter
                    .as_deref()
                    .is_none_or(|status| record.status == status)
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.started_at_ms.cmp(&right.started_at_ms));
        let output = format_subagent_records(&records, false);
        self.emit_tool_end(app_handle, thread_id, call_id, "list_agents", 0, &output);
        Ok(output)
    }

    async fn exec_close_agent(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: CloseAgentArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid close_agent args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "close_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", -1, &msg);
                return Ok(msg);
            }
        };

        let target = match close_agent_target(args) {
            Some(target) => target,
            None => {
                let msg = "close_agent target must not be empty".to_string();
                self.emit_tool_start(app_handle, thread_id, call_id, "close_agent", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", -1, &msg);
                return Ok(msg);
            }
        };

        self.emit_tool_start(app_handle, thread_id, call_id, "close_agent", &target);

        let previous_status;
        let agent_snapshot;
        {
            let mut subagents = self.subagents.lock().await;
            match subagents.get_mut(&target) {
                None => {
                    let msg = format!("No subagent found with id: {target}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", -1, &msg);
                    return Ok(msg);
                }
                Some(record) => {
                    previous_status = record.status.clone();
                    if record.status == "running" {
                        record.status = "closed".to_string();
                        record.completed_at_ms = Some(now_millis());
                    }
                    agent_snapshot = Some(record.clone());
                }
            }
        }

        // Signal the subagent to cancel via its AtomicBool flag
        if previous_status == "running" {
            let handles = self.subagent_handles.lock().await;
            if let Some(handle) = handles.get(&target) {
                handle.cancel_flag.store(true, Ordering::SeqCst);
            }
        }

        persist_subagent_records(&self.workspace_config_dir, &self.subagents).await;

        let result = SubagentCloseResult {
            target: target.clone(),
            closed: previous_status == "running",
            previous_status,
            message: "Subagent closed".to_string(),
            agent: agent_snapshot,
        };
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        self.emit_tool_end(app_handle, thread_id, call_id, "close_agent", 0, &output);
        Ok(output)
    }

    async fn exec_tool_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> AppResult<String> {
        let args: ToolSearchArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid tool_search args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "tool_search", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "tool_search", -1, &msg);
                return Ok(msg);
            }
        };
        let query = args.query.trim();
        self.emit_tool_start(app_handle, thread_id, call_id, "tool_search", query);

        if query.is_empty() {
            let msg = "tool_search query must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "tool_search", -1, &msg);
            return Ok(msg);
        }

        let limit = args.limit.unwrap_or(8).clamp(1, 50);
        let matches = search_tool_entries(self.tool_search_entries(), query, limit);
        let mut activated_names = BTreeSet::new();
        for entry in &matches {
            if entry.kind == "tool" && !Self::is_core_tool_name(&entry.name) {
                activated_names.insert(entry.name.clone());
            }
            if entry.kind == "mcp_server" {
                activated_names.insert("mcp_list_tools".to_string());
                activated_names.insert("mcp_call_tool".to_string());
            }
        }
        let activated_names: Vec<String> = activated_names.into_iter().collect();
        let activated_count = activated_names.len();
        if activated_count > 0 {
            self.activate_tools_for_thread(thread_id, activated_names.iter().cloned())
                .await;
        }
        let skill_match_count = matches.iter().filter(|entry| entry.kind == "skill").count();
        let mcp_match_count = matches
            .iter()
            .filter(|entry| entry.source.starts_with("mcp:"))
            .count();
        crate::agent::emit_and_broadcast(
            app_handle,
            "turn-loading",
            serde_json::json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "callId": call_id,
                "kind": if mcp_match_count > 0 { "mcp" } else { "skill" },
                "phase": "activation",
                "status": "completed",
                "query": query,
                "skillMatchCount": skill_match_count,
                "mcpMatchCount": mcp_match_count,
                "activatedCount": activated_count,
                "activatedTools": activated_names.clone(),
            }),
        );
        let output = format_tool_search_output(query, matches);
        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        let output = if activated_count > 0 {
            format!(
                "{output}\n\n[activated {activated_count} non-core tool schema(s) for the next model call in this turn: {}]",
                activated_names.join(", ")
            )
        } else {
            output
        };
        self.emit_tool_end(app_handle, thread_id, call_id, "tool_search", 0, &output);
        Ok(output)
    }

    async fn exec_apps_list(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: AppsListArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid apps_list args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "apps_list", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "apps_list", -1, &msg);
                return Ok(msg);
            }
        };
        let connector_filter = args
            .connector_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = connector_filter.unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "apps_list", display);

        let output = format_apps_list_output(
            &self.workspace_config_dir,
            &self.mcp_tool_aliases,
            connector_filter,
            args.include_tools.unwrap_or(true),
        );
        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "apps_list", 0, &output);
        Ok(output)
    }

    async fn exec_list_available_plugins_to_install(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ListAvailablePluginsArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid list_available_plugins_to_install args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "list_available_plugins_to_install",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "list_available_plugins_to_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let display = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .unwrap_or("all");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "list_available_plugins_to_install",
            display,
        );

        let source_dir = match resolve_plugin_cache_source_dir(args.source_dir.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "list_available_plugins_to_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let candidates = plugin_install_candidates(
            &source_dir,
            &self.workspace_config_dir,
            args.query.as_deref(),
            args.include_installed.unwrap_or(true),
            args.limit.unwrap_or(50).clamp(1, 100),
        );
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "sourceDir": source_dir,
            "tools": candidates,
        }))
        .unwrap_or_default();
        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "list_available_plugins_to_install",
            0,
            &output,
        );
        Ok(output)
    }

    async fn exec_request_plugin_install(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: RequestPluginInstallArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid request_plugin_install args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let display = args
            .tool_id
            .as_deref()
            .or(args.name.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("plugin");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "request_plugin_install",
            display,
        );

        if args
            .tool_type
            .as_deref()
            .is_some_and(|tool_type| tool_type != "plugin")
        {
            let msg =
                "request_plugin_install currently supports only tool_type=\"plugin\"".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_plugin_install",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        if args
            .action_type
            .as_deref()
            .is_some_and(|action_type| action_type != "install")
        {
            let msg = "request_plugin_install currently supports only action_type=\"install\""
                .to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "request_plugin_install",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        let source_dir = match resolve_plugin_cache_source_dir(args.source_dir.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let candidates =
            plugin_install_candidates(&source_dir, &self.workspace_config_dir, None, true, 200);
        let selection = select_plugin_install_candidate(
            &candidates,
            args.tool_id.as_deref(),
            args.name.as_deref(),
        );
        let candidate = match selection {
            Ok(candidate) => candidate,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let import_result = plugin_commands::import_plugin_root(
            Path::new(&candidate.source),
            &self.workspace_config_dir.join("plugins"),
        );
        let output = match import_result {
            Ok(item) => serde_json::to_string_pretty(&serde_json::json!({
                "completed": true,
                "userConfirmed": true,
                "toolType": "plugin",
                "actionType": "install",
                "toolId": candidate.id,
                "toolName": candidate.name,
                "suggestReason": args.suggest_reason.unwrap_or_default(),
                "imported": item,
            }))
            .unwrap_or_default(),
            Err(error) => {
                let msg = format!("Failed to import plugin '{}': {error}", candidate.name);
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "request_plugin_install",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "request_plugin_install",
            0,
            &output,
        );
        Ok(output)
    }

    async fn exec_plugin_manage(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: PluginManageArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid plugin_manage args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "plugin_manage", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                return Ok(msg);
            }
        };
        let action = args
            .action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("list");
        let plugin_id = args
            .plugin_id
            .as_deref()
            .or(args.id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = plugin_id.unwrap_or(action);
        self.emit_tool_start(app_handle, thread_id, call_id, "plugin_manage", display);

        let output = match action {
            "list" => serde_json::to_string_pretty(&serde_json::json!({
                "plugins": plugin_loader::list_plugins(&self.workspace_config_dir),
            }))
            .unwrap_or_default(),
            "enable" | "disable" => {
                let Some(plugin_id) = plugin_id else {
                    let msg = format!("plugin_manage action '{action}' requires plugin_id");
                    self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                    return Ok(msg);
                };
                match plugin_loader::set_plugin_enabled(
                    &self.workspace_config_dir,
                    plugin_id,
                    action == "enable",
                ) {
                    Ok(plugin) => serde_json::to_string_pretty(&serde_json::json!({
                        "completed": true,
                        "action": action,
                        "plugin": plugin,
                    }))
                    .unwrap_or_default(),
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "plugin_manage",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            }
            "uninstall" => {
                let Some(plugin_id) = plugin_id else {
                    let msg = "plugin_manage action 'uninstall' requires plugin_id".to_string();
                    self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                    return Ok(msg);
                };
                let removed_path = self
                    .workspace_config_dir
                    .join("plugins")
                    .join(plugin_id)
                    .to_string_lossy()
                    .to_string();
                match plugin_loader::uninstall_plugin(&self.workspace_config_dir, plugin_id) {
                    Ok(()) => serde_json::to_string_pretty(&serde_json::json!({
                        "completed": true,
                        "action": "uninstall",
                        "pluginId": plugin_id,
                        "removedPath": removed_path,
                    }))
                    .unwrap_or_default(),
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "plugin_manage",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            }
            other => {
                let msg = format!(
                    "plugin_manage action must be one of list, enable, disable, or uninstall: {other}"
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", -1, &msg);
                return Ok(msg);
            }
        };

        let output = truncate_output(&output, TOOL_OUTPUT_SEARCH_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "plugin_manage", 0, &output);
        Ok(output)
    }

    async fn exec_robot_save(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct RobotSaveArgs {
            id: Option<String>,
            config: Option<serde_json::Value>,
        }

        let args: RobotSaveArgs = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("Invalid robot_save args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "robot_save", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
                return Ok(msg);
            }
        };

        let robot_id = args
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unnamed-robot");

        self.emit_tool_start(app_handle, thread_id, call_id, "robot_save", robot_id);

        let Some(config_value) = args.config else {
            let msg = "robot_save requires a config object".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
            return Ok(msg);
        };

        let mut config: crate::robot_loader::RobotConfig =
            match serde_json::from_value(config_value) {
                Ok(c) => c,
                Err(e) => {
                    let msg = format!("Invalid robot config: {e}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
                    return Ok(msg);
                }
            };

        // 强约束校验：workflow 必须可归一化为节点，且每个节点都要绑定至少一个 skill。
        // 这里做服务端兜底，即使模型未严格遵循 schema，也能及时返回清晰错误。
        let normalized_nodes = config.normalized_workflow_nodes();
        if normalized_nodes.is_empty() {
            let msg =
                "Invalid robot config: workflowNodes must contain at least one node".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
            return Ok(msg);
        }
        if let Some((idx, _)) = normalized_nodes
            .iter()
            .enumerate()
            .find(|(_, node)| node.skills.is_empty() && node.plugin_skills.is_empty())
        {
            let msg = format!(
                "Invalid robot config: workflowNodes[{idx}] must assign at least one local or plugin skill"
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
            return Ok(msg);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if config.created_at == 0 {
            config.created_at = now;
        }
        config.updated_at = now;

        match crate::robot_loader::save_robot(&self.workspace_config_dir, robot_id, &config) {
            Ok(detail) => {
                let output = serde_json::to_string_pretty(&serde_json::json!({
                    "status": "created",
                    "robotId": detail.id,
                    "name": detail.config.name,
                    "path": detail.path,
                    "skillsCount": detail.config.all_local_skills().len() + detail.config.all_plugin_skills().len(),
                    "workflowSteps": detail.config.normalized_workflow_nodes().len(),
                }))
                .unwrap_or_default();

                app_handle
                    .emit(
                        "robot-created",
                        serde_json::json!({
                            "threadId": thread_id,
                            "robotId": detail.id,
                            "name": detail.config.name,
                        }),
                    )
                    .ok();

                self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", 0, &output);
                Ok(output)
            }
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "robot_save", -1, &msg);
                Ok(msg)
            }
        }
    }

    async fn exec_code_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: CodeSearchArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid code_search args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "code_search", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "code_search", -1, &msg);
                return Ok(msg);
            }
        };

        let display = format!(
            "pattern={} path={}",
            args.pattern,
            args.path.as_deref().unwrap_or(".")
        );
        self.emit_tool_start(app_handle, thread_id, call_id, "code_search", &display);

        let command = match build_code_search_command(&self.cwd, &args) {
            Ok(command) => command,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "code_search", -1, &msg);
                return Ok(msg);
            }
        };

        let mut process = Command::new(&command.rg_path);
        process
            .args(&command.args)
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        process.no_console();

        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(e) => {
                let msg = format!("Failed to start embedded code search engine: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "code_search", -1, &msg);
                return Ok(msg);
            }
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));
        let stdout_collector = stdout_buffer.clone();
        let stderr_collector = stderr_buffer.clone();
        let mut stdout_handle = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                collect_shell_stream_bytes(stdout, stdout_collector).await;
            }
        });
        let mut stderr_handle = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                collect_shell_stream_bytes(stderr, stderr_collector).await;
            }
        });

        let child = Arc::new(Mutex::new(child));
        self.register_active_tool_process(thread_id, call_id, "code_search", child.clone())
            .await;

        let (exit_code, timed_out) =
            match wait_for_child_with_timeout(&child, command.timeout_ms).await {
                WaitChildResult::Exited(status) => (status.code().unwrap_or(-1), false),
                WaitChildResult::TimedOut => {
                    terminate_shell_child(&child).await;
                    (124, true)
                }
                WaitChildResult::Failed(error) => {
                    terminate_shell_child(&child).await;
                    wait_for_shell_stream_task(&mut stdout_handle, 300).await;
                    wait_for_shell_stream_task(&mut stderr_handle, 300).await;
                    self.unregister_active_tool_process(thread_id, call_id)
                        .await;
                    let msg = format!("Failed to wait for code_search: {error}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "code_search", -1, &msg);
                    return Ok(msg);
                }
            };

        wait_for_shell_stream_task(&mut stdout_handle, 1_500).await;
        wait_for_shell_stream_task(&mut stderr_handle, 1_500).await;
        self.unregister_active_tool_process(thread_id, call_id)
            .await;
        let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
        let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
        let output = if timed_out {
            format!(
                "code_search timed out after {} ms for pattern `{}`.",
                command.timeout_ms, args.pattern
            )
        } else {
            format_code_search_output(
                &args.pattern,
                &command.search_root,
                &self.cwd,
                &stdout,
                &stderr,
                exit_code,
                command.head_limit,
            )
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "code_search",
            exit_code,
            &output,
        );
        Ok(output)
    }

    async fn exec_code_review(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: CodeReviewArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid code_review args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "code_review", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "code_review", -1, &msg);
                return Ok(msg);
            }
        };

        let scope = code_review_scope_label(&args);
        self.emit_tool_start(app_handle, thread_id, call_id, "code_review", &scope);

        let diff_ref = match validate_code_review_base_ref(args.base_ref.as_deref()) {
            Ok(value) => value,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "code_review", -1, &msg);
                return Ok(msg);
            }
        };
        let paths = match validate_code_review_paths(args.paths.as_deref().unwrap_or(&[])) {
            Ok(paths) => paths,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "code_review", -1, &msg);
                return Ok(msg);
            }
        };
        let max_diff_bytes = args
            .max_diff_bytes
            .unwrap_or(200_000)
            .clamp(4_000, 1_000_000);

        let status = self
            .git_capture(&["status".to_string(), "--short".to_string()], 64_000)
            .await
            .unwrap_or_else(|e| GitCommandOutput {
                exit_code: -1,
                stdout: String::new(),
                stderr: e,
            });
        if status.exit_code != 0 {
            let msg = format!(
                "code_review requires a git repository. git status failed: {}",
                combine_stdout_stderr(&status.stdout, &status.stderr)
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "code_review", -1, &msg);
            return Ok(msg);
        }

        let diff_args = code_review_git_args("diff", diff_ref.as_deref(), &paths);
        let diff = match self.git_capture(&diff_args, max_diff_bytes).await {
            Ok(output) => output,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "code_review", -1, &msg);
                return Ok(msg);
            }
        };
        if diff.exit_code != 0 {
            let msg = format!(
                "git diff failed: {}",
                combine_stdout_stderr(&diff.stdout, &diff.stderr)
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "code_review", -1, &msg);
            return Ok(msg);
        }

        let numstat_args = code_review_git_args("numstat", diff_ref.as_deref(), &paths);
        let numstat = self
            .git_capture(&numstat_args, 64_000)
            .await
            .unwrap_or_else(|e| GitCommandOutput {
                exit_code: -1,
                stdout: String::new(),
                stderr: e,
            });
        let name_status_args = code_review_git_args("name-status", diff_ref.as_deref(), &paths);
        let name_status = self
            .git_capture(&name_status_args, 64_000)
            .await
            .unwrap_or_else(|e| GitCommandOutput {
                exit_code: -1,
                stdout: String::new(),
                stderr: e,
            });
        let check_args = code_review_git_args("check", diff_ref.as_deref(), &paths);
        let diff_check = self
            .git_capture(&check_args, 64_000)
            .await
            .unwrap_or_else(|e| GitCommandOutput {
                exit_code: -1,
                stdout: String::new(),
                stderr: e,
            });

        let include_untracked = args.include_untracked.unwrap_or(true);
        let untracked = if include_untracked {
            code_review_untracked_paths(&status.stdout, &paths)
        } else {
            Vec::new()
        };
        let summary = analyze_code_review_diff(
            &diff.stdout,
            &numstat.stdout,
            &diff_check,
            &untracked,
            diff.stdout.len() >= max_diff_bytes,
        );
        let output = format_code_review_output(
            &scope,
            &summary,
            &name_status.stdout,
            &status.stdout,
            &untracked,
            &diff_check,
        );
        self.emit_tool_end(app_handle, thread_id, call_id, "code_review", 0, &output);
        Ok(output)
    }

    async fn git_capture(
        &self,
        args: &[String],
        max_output_bytes: usize,
    ) -> Result<GitCommandOutput, String> {
        // 统一复用 git_service 的执行与截断逻辑，避免工具层与右侧 Git 面板重复实现。
        let service = GitService::new(self.cwd.clone());
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        service
            .run_allow_failure(&refs, max_output_bytes)
            .await
            .map_err(|err| err.to_string())
    }

    async fn exec_memory_list(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct ListArgs {
            #[serde(default)]
            path: Option<String>,
            #[serde(default)]
            max_entries: Option<usize>,
            #[serde(default)]
            cursor: Option<String>,
            #[serde(default)]
            format: Option<String>,
        }

        let args: ListArgs = serde_json::from_str(arguments).unwrap_or_default();
        let display_path = args.path.as_deref().unwrap_or(".");
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_list", display_path);

        let dir = match self.resolve_memory_path(args.path.as_deref().unwrap_or("")) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
                return Ok(msg);
            }
        };

        if let Err(e) = tokio::fs::create_dir_all(self.memories_dir()).await {
            let msg = format!("Error creating memories directory: {e}");
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
            return Ok(msg);
        }

        let max_entries = args.max_entries.unwrap_or(100).clamp(1, 200);
        let cursor = match parse_memory_cursor(args.cursor.as_deref()) {
            Ok(cursor) => cursor,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
                return Ok(msg);
            }
        };
        let format = MemoryOutputFormat::from_arg(args.format.as_deref());

        let mut entries = Vec::new();
        match tokio::fs::read_dir(&dir).await {
            Ok(mut reader) => {
                while let Ok(Some(entry)) = reader.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    let path = if display_path == "." || display_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", display_path.trim_end_matches('/'), name)
                    };
                    entries.push(MemoryListEntry { name, path, is_dir });
                }
                entries.sort();
                let total = entries.len();
                let page = entries
                    .into_iter()
                    .skip(cursor)
                    .take(max_entries)
                    .collect::<Vec<_>>();
                let next_cursor = if cursor + page.len() < total {
                    Some((cursor + page.len()).to_string())
                } else {
                    None
                };
                let output =
                    format_memory_list_output(display_path, page, total, next_cursor, format);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", 0, &output);
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Error listing memory path {display_path}: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
                Ok(msg)
            }
        }
    }

    async fn exec_memory_read(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ReadArgs {
            path: String,
            #[serde(default)]
            line_offset: Option<usize>,
            #[serde(default)]
            max_lines: Option<usize>,
            #[serde(default)]
            format: Option<String>,
        }

        let args: ReadArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_read args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_read", &args.path);

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_read", -1, &msg);
                return Ok(msg);
            }
        };

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => {
                let msg = format!("Error reading memory {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_read", -1, &msg);
                return Ok(msg);
            }
        };

        let line_offset = args.line_offset.unwrap_or(1).max(1);
        let max_lines = args.max_lines.unwrap_or(200).clamp(1, 500);
        let lines = content.lines().collect::<Vec<_>>();
        let selected_lines = lines
            .iter()
            .skip(line_offset.saturating_sub(1))
            .take(max_lines)
            .copied()
            .collect::<Vec<_>>();
        let selected = selected_lines.join("\n");
        let next_line_offset = if line_offset.saturating_sub(1) + selected_lines.len() < lines.len()
        {
            Some(line_offset + selected_lines.len())
        } else {
            None
        };
        let format = MemoryOutputFormat::from_arg(args.format.as_deref());
        let output_body = match format {
            MemoryOutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
                "path": args.path,
                "lineOffset": line_offset,
                "maxLines": max_lines,
                "totalLines": lines.len(),
                "nextLineOffset": next_line_offset,
                "content": selected,
            }))
            .unwrap_or_default(),
            MemoryOutputFormat::Text => {
                let mut text = format!(
                    "Memory: {}\nLines: {}-{}\n",
                    args.path,
                    line_offset,
                    line_offset + selected.lines().count().saturating_sub(1)
                );
                if let Some(next) = next_line_offset {
                    text.push_str(&format!("Next line_offset: {next}\n"));
                }
                text.push('\n');
                text.push_str(&selected);
                text
            }
        };
        let output = truncate_output(&output_body, TOOL_OUTPUT_MEMORY_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "memory_read", 0, &output);

        self.track_experience_usage(&args.path);

        Ok(output)
    }

    async fn exec_memory_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct SearchArgs {
            query: String,
            #[serde(default)]
            path: Option<String>,
            #[serde(default)]
            case_sensitive: Option<bool>,
            #[serde(default)]
            max_results: Option<usize>,
            #[serde(default)]
            context_lines: Option<usize>,
            #[serde(default)]
            cursor: Option<String>,
            #[serde(default)]
            format: Option<String>,
        }

        let args: SearchArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_search args: {e}"))
        })?;
        let query = args.query.trim();
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_search", query);

        if query.is_empty() {
            let msg = "Error: empty memory search query".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
            return Ok(msg);
        }

        let root = match self.resolve_memory_path(args.path.as_deref().unwrap_or("")) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
                return Ok(msg);
            }
        };

        let max_results = args.max_results.unwrap_or(20).clamp(1, 50);
        let context_lines = args.context_lines.unwrap_or(0).clamp(0, 5);
        let cursor = match parse_memory_cursor(args.cursor.as_deref()) {
            Ok(cursor) => cursor,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
                return Ok(msg);
            }
        };
        let case_sensitive = args.case_sensitive.unwrap_or(false);
        let result = search_memory_files(
            &self.memories_dir(),
            &root,
            query,
            case_sensitive,
            context_lines,
            cursor,
            max_results,
        );

        let output = match result {
            Ok(result) => format_memory_search_output(
                query,
                &result,
                MemoryOutputFormat::from_arg(args.format.as_deref()),
            ),
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
                return Ok(msg);
            }
        };

        let output = truncate_output(&output, TOOL_OUTPUT_MEMORY_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", 0, &output);
        Ok(output)
    }

    async fn exec_smartbrain_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default)]
            top_k: Option<usize>,
            #[serde(default)]
            domain: Option<String>,
            #[serde(default)]
            concept_type: Option<String>,
            #[serde(default)]
            tags: Option<Vec<String>>,
            #[serde(default)]
            source_type: Option<String>,
            #[serde(default)]
            source_group: Option<String>,
            #[serde(default)]
            relative_path_prefix: Option<String>,
            #[serde(default)]
            source_file: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid smartbrain_search args: {e}"))
        })?;
        let query = args.query.trim();
        self.emit_tool_start(app_handle, thread_id, call_id, "smartbrain_search", query);

        if query.is_empty() {
            let msg = "Error: empty search query".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        if !self.smartbrain_is_active() {
            let msg =
                "Local Knowledge Base is disabled. Enable it in Settings to use smartbrain_search."
                    .to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        let top_k = args.top_k.unwrap_or(5).clamp(1, 20);
        let bm25_path = crate::smartbrain::bm25_index_path(&self.workspace_config_dir);
        let smartbrain_policy = ConfigToml::load(&self.workspace_config_dir.join("config.toml"))
            .map(|config| config.smartbrain_config())
            .unwrap_or_default();
        if !bm25_path.exists() {
            let msg = "Local Knowledge Base index is not ready yet. Upload knowledge files or run `smartbrain_rebuild_index`, then retry `smartbrain_search`.".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        let bm25 = crate::smartbrain::bm25_index::BM25Index::load(&bm25_path);
        if bm25.document_count() == 0 {
            let msg = "Local Knowledge Base index is empty. Upload knowledge content first, then run `smartbrain_rebuild_index` if needed.".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        let domain = args
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let concept_type = args
            .concept_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let tags = args
            .tags
            .unwrap_or_default()
            .into_iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        let source_group = args
            .source_group
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let relative_path_prefix = args
            .relative_path_prefix
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let source_file = args
            .source_file
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let source_type = args
            .source_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| match value.to_ascii_lowercase().as_str() {
                "experience" => crate::smartbrain::bm25_index::SourceType::Experience,
                _ => crate::smartbrain::bm25_index::SourceType::Knowledge,
            });
        let has_filter = concept_type.is_some()
            || !tags.is_empty()
            || source_type.is_some()
            || domain.is_some()
            || source_group.is_some()
            || relative_path_prefix.is_some()
            || source_file.is_some();
        let results = if has_filter {
            crate::smartbrain::search::unified_search_with_filter_with_policy(
                &bm25_path,
                query,
                top_k,
                crate::smartbrain::bm25_index::SearchFilter {
                    concept_type,
                    tags,
                    domain,
                    source_group,
                    relative_path_prefix,
                    source_file,
                    source_type,
                    timestamp_after: None,
                    timestamp_before: None,
                },
                smartbrain_policy.search_okf_prefilter_enabled,
                Some(&smartbrain_policy.search_okf_prefilter_order),
            )
        } else {
            crate::smartbrain::search::unified_search(&bm25_path, query, top_k)
        };

        let output = if results.is_empty() {
            "No matching documents found in Local Knowledge Base.".to_string()
        } else {
            let mut lines = Vec::new();
            let mut context_blocks_added = 0usize;
            lines.push(format!(
                "Found {} results for \"{}\":\n",
                results.len(),
                query
            ));
            for (i, r) in results.iter().enumerate() {
                lines.push(format!(
                    "{}. [{}] {} (score: {:.3})\n   Path: {}\n   Use `memory_read` with path \"{}\" and small windows (for example `line_offset: 1`, `max_lines: 120`) to read relevant sections. For chunk hits, context bridge below keeps prev/current/next continuity with >=30-line overlap.",
                    i + 1, r.source_type, r.title, r.score, r.file_path, r.file_path
                ));
                if let Some(result_domain) = r.domain.as_deref().filter(|value| !value.is_empty()) {
                    lines.push(format!("   Domain: {result_domain}"));
                }
                if let Some(result_group) =
                    r.source_group.as_deref().filter(|value| !value.is_empty())
                {
                    lines.push(format!("   Source Group: {result_group}"));
                }
                if let Some(relative_path) =
                    r.relative_path.as_deref().filter(|value| !value.is_empty())
                {
                    lines.push(format!("   Relative Path: {relative_path}"));
                }
                if let Some(source_file) =
                    r.source_file.as_deref().filter(|value| !value.is_empty())
                {
                    lines.push(format!("   Source File: {source_file}"));
                }
                if r.is_chunk {
                    let chunk_index = r.chunk_index.unwrap_or(1);
                    let chunk_total = r.chunk_total.unwrap_or(chunk_index);
                    lines.push(format!("   Chunk: {chunk_index}/{chunk_total}"));
                    if let Some(parent_doc_id) =
                        r.parent_doc_id.as_deref().filter(|value| !value.is_empty())
                    {
                        lines.push(format!("   Parent Doc: {parent_doc_id}"));
                    }
                    if context_blocks_added < SMARTBRAIN_CONTEXT_RESULT_LIMIT {
                        if let Some(context_block) =
                            self.render_chunk_context_bridge(r, SMARTBRAIN_CONTEXT_OVERLAP_LINES)
                        {
                            lines.push(context_block);
                            context_blocks_added += 1;
                        }
                    }
                }
            }
            lines.join("\n")
        };

        let output = truncate_output(&output, TOOL_OUTPUT_SMARTBRAIN_MAX_CHARS);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "smartbrain_search",
            0,
            &output,
        );
        Ok(output)
    }

    async fn exec_smartbrain_sql_query(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            database: Option<String>,
            sql: String,
            #[serde(default)]
            row_limit: Option<usize>,
            #[serde(default)]
            timeout_sec: Option<u64>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid smartbrain_sql_query args: {e}"))
        })?;
        let sql = args.sql.trim();
        let database = args
            .database
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = match database {
            Some(name) => format!("{name} :: {}", truncate_output(sql, 120)),
            None => truncate_output(sql, 160),
        };
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "smartbrain_sql_query",
            &display,
        );

        if !self.smartbrain_is_active() {
            let msg = "Local Knowledge Base is disabled. Enable it in Settings to use smartbrain_sql_query."
                .to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_sql_query",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        if sql.is_empty() {
            let msg = "Error: empty SQL".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_sql_query",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        match crate::smartbrain::db_query::execute_smartbrain_sql_query(
            &self.workspace_config_dir,
            database,
            sql,
            args.row_limit,
            args.timeout_sec,
        )
        .await
        {
            Ok(result) => {
                let output = truncate_output(
                    &crate::smartbrain::db_query::format_sql_query_result(&result),
                    24_000,
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "smartbrain_sql_query",
                    0,
                    &output,
                );
                Ok(output)
            }
            Err(error) => {
                let msg = format!("smartbrain_sql_query failed: {error}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "smartbrain_sql_query",
                    -1,
                    &msg,
                );
                Ok(msg)
            }
        }
    }

    async fn exec_build_entry_form(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            database: Option<String>,
            #[serde(default)]
            table: Option<String>,
            #[serde(default)]
            intent: Option<String>,
            #[serde(default)]
            known_values: Option<serde_json::Map<String, serde_json::Value>>,
        }

        let args: Args = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid build_entry_form args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "build_entry_form", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "build_entry_form", -1, &msg);
                return Ok(msg);
            }
        };

        let database = args
            .database
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let table = args
            .table
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let intent = args
            .intent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let known_values = args.known_values.unwrap_or_default();

        let display = match (database, table) {
            (Some(db), Some(tb)) => format!("{db}.{tb}"),
            (Some(db), None) => db.to_string(),
            (None, Some(tb)) => tb.to_string(),
            (None, None) => intent.unwrap_or("entry form").to_string(),
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "build_entry_form", &display);

        if !self.smartbrain_is_active() {
            let msg = "Local Knowledge Base is disabled. Enable it in Settings to use build_entry_form."
                .to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "build_entry_form", -1, &msg);
            return Ok(msg);
        }

        let form = match crate::smartbrain::form_entry::build_entry_form(
            &self.workspace_config_dir,
            database,
            known_values,
            table,
            intent,
        )
        .await
        {
            Ok(form) => form,
            Err(error) => {
                let msg = format!("build_entry_form failed: {error}");
                self.emit_tool_end(app_handle, thread_id, call_id, "build_entry_form", -1, &msg);
                return Ok(msg);
            }
        };

        // Open interactive form UI and wait for user save/cancel.
        let request_id = RequestId::String(call_id.to_string());
        app_handle
            .emit(
                "server-request",
                serde_json::json!({
                    "requestId": call_id,
                    "id": call_id,
                    "method": "build_entry_form",
                    "params": {
                        "threadId": thread_id,
                        "callId": call_id,
                        "form": form,
                    },
                }),
            )
            .ok();

        let output = match wait_for_approval_result(app_handle, &request_id, 600_000).await {
            Ok(result) => {
                // If the UI already saved, just return that result.
                if result.get("saved").and_then(|v| v.as_bool()).unwrap_or(false) {
                    let pretty = serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string());
                    self.emit_tool_end(
                        app_handle,
                        thread_id,
                        call_id,
                        "build_entry_form",
                        0,
                        &pretty,
                    );
                    return Ok(pretty);
                }

                // Otherwise treat as form values submission and insert here.
                let values = result
                    .get("values")
                    .and_then(|value| value.as_object())
                    .cloned()
                    .unwrap_or_default();
                let cancelled = result
                    .get("cancelled")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
                    || result
                        .get("decision")
                        .and_then(|value| value.as_str())
                        .is_some_and(|decision| {
                            decision.eq_ignore_ascii_case("cancel")
                                || decision.eq_ignore_ascii_case("cancelled")
                        });
                if cancelled {
                    let msg = "用户取消了入库表单".to_string();
                    self.emit_tool_end(
                        app_handle,
                        thread_id,
                        call_id,
                        "build_entry_form",
                        -1,
                        &msg,
                    );
                    return Ok(msg);
                }

                match crate::smartbrain::form_entry::save_form_data(
                    &self.workspace_config_dir,
                    Some(&form.database),
                    &form.table,
                    values,
                )
                .await
                {
                    Ok(save_result) => {
                        let pretty = serde_json::to_string_pretty(&save_result)
                            .unwrap_or_else(|_| format!("{save_result:?}"));
                        let code = if save_result.ok { 0 } else { -1 };
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "build_entry_form",
                            code,
                            &pretty,
                        );
                        pretty
                    }
                    Err(error) => {
                        let msg = format!("save after form failed: {error}");
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "build_entry_form",
                            -1,
                            &msg,
                        );
                        msg
                    }
                }
            }
            Err(msg) => {
                let output = format!("build_entry_form cancelled/failed: {msg}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "build_entry_form",
                    -1,
                    &output,
                );
                output
            }
        };

        Ok(output)
    }

    async fn exec_save_form_data(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            database: Option<String>,
            table: String,
            #[serde(default)]
            values: serde_json::Map<String, serde_json::Value>,
        }

        let args: Args = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid save_form_data args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "save_form_data", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "save_form_data", -1, &msg);
                return Ok(msg);
            }
        };

        let database = args
            .database
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = match database {
            Some(db) => format!("{db}.{}", args.table),
            None => args.table.clone(),
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "save_form_data", &display);

        if !self.smartbrain_is_active() {
            let msg =
                "Local Knowledge Base is disabled. Enable it in Settings to use save_form_data."
                    .to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "save_form_data", -1, &msg);
            return Ok(msg);
        }

        match crate::smartbrain::form_entry::save_form_data(
            &self.workspace_config_dir,
            database,
            &args.table,
            args.values,
        )
        .await
        {
            Ok(result) => {
                let pretty = serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("{result:?}"));
                let code = if result.ok { 0 } else { -1 };
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "save_form_data",
                    code,
                    &pretty,
                );
                Ok(pretty)
            }
            Err(error) => {
                let msg = format!("save_form_data failed: {error}");
                self.emit_tool_end(app_handle, thread_id, call_id, "save_form_data", -1, &msg);
                Ok(msg)
            }
        }
    }

    fn render_chunk_context_bridge(
        &self,
        result: &crate::smartbrain::search::SmartBrainSearchResult,
        overlap_lines: usize,
    ) -> Option<String> {
        if !result.is_chunk {
            return None;
        }
        let parent_doc_id = result.parent_doc_id.as_deref()?.trim();
        if parent_doc_id.is_empty() {
            return None;
        }
        let chunk_index = result.chunk_index?;
        let chunk_total = result.chunk_total.unwrap_or(chunk_index).max(chunk_index);
        let overlap_lines = overlap_lines.max(SMARTBRAIN_CONTEXT_OVERLAP_LINES);

        let current_path = self.resolve_memory_path(&result.file_path).ok()?;
        let current_lines = read_okf_body_lines(&current_path)?;
        if current_lines.is_empty() {
            return None;
        }

        let docs_dir = self.memories_dir().join("knowledge").join("docs");
        let previous_lines = if chunk_index > 1 {
            let previous_file =
                crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index - 1);
            read_okf_body_lines(&docs_dir.join(previous_file))
        } else {
            None
        };
        let next_lines = if chunk_index < chunk_total {
            let next_file =
                crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index + 1);
            read_okf_body_lines(&docs_dir.join(next_file))
        } else {
            None
        };

        if previous_lines.is_none() && next_lines.is_none() {
            return None;
        }

        let mut lines = vec![format!(
            "   Context Bridge (上下文补齐, overlap >= {overlap_lines} lines):"
        )];
        if let Some(prev) = previous_lines {
            lines.push(format!(
                "   - Prev chunk {}/{} tail + current head overlap:",
                chunk_index.saturating_sub(1),
                chunk_total
            ));
            for line in take_last_lines(&prev, overlap_lines) {
                lines.push(format!("     {line}"));
            }
            for line in take_first_lines(&current_lines, overlap_lines) {
                lines.push(format!("     {line}"));
            }
        }
        lines.push(format!("   - Current chunk {chunk_index}/{chunk_total}:"));
        for line in &current_lines {
            lines.push(format!("     {line}"));
        }
        if let Some(next) = next_lines {
            lines.push(format!(
                "   - Current tail overlap + next chunk {}/{} head:",
                chunk_index + 1,
                chunk_total
            ));
            for line in take_last_lines(&current_lines, overlap_lines) {
                lines.push(format!("     {line}"));
            }
            for line in take_first_lines(&next, overlap_lines) {
                lines.push(format!("     {line}"));
            }
        }
        Some(lines.join("\n"))
    }

    async fn exec_memory_write(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct WriteArgs {
            path: String,
            content: String,
            #[serde(default)]
            append: Option<bool>,
        }

        let args: WriteArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_write args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_write", &args.path);

        if args.content.trim().is_empty() {
            let msg = "Error: empty memory content".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
            return Ok(msg);
        }

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
                return Ok(msg);
            }
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                let msg = format!("Error creating memory parent directory: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
                return Ok(msg);
            }
        }

        let append = args.append.unwrap_or(false);
        let result = if append {
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(args.content.as_bytes()).await {
                        Err(e)
                    } else if !args.content.ends_with('\n') {
                        file.write_all(b"\n").await
                    } else {
                        Ok(())
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            tokio::fs::write(&path, &args.content).await
        };

        match result {
            Ok(()) => {
                let msg = format!(
                    "{} memory {} ({} bytes)",
                    if append { "Appended" } else { "Wrote" },
                    args.path,
                    args.content.len()
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", 0, &msg);
                Ok(msg)
            }
            Err(e) => {
                let msg = format!("Error writing memory {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
                Ok(msg)
            }
        }
    }

    async fn exec_memory_update(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct UpdateArgs {
            path: String,
            old_text: String,
            new_text: String,
            #[serde(default)]
            replace_all: Option<bool>,
        }

        let args: UpdateArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_update args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_update", &args.path);

        if args.old_text.is_empty() {
            let msg = "Error: memory_update old_text must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
            return Ok(msg);
        }

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
                return Ok(msg);
            }
        };

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => {
                let msg = format!("Error reading memory {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
                return Ok(msg);
            }
        };

        if !content.contains(&args.old_text) {
            let msg = format!("No exact memory text match found in {}", args.path);
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
            return Ok(msg);
        }

        let replace_all = args.replace_all.unwrap_or(false);
        let count = content.matches(&args.old_text).count();
        let updated = if replace_all {
            content.replace(&args.old_text, &args.new_text)
        } else {
            content.replacen(&args.old_text, &args.new_text, 1)
        };
        let replaced = if replace_all { count } else { 1 };

        if let Err(e) = tokio::fs::write(&path, updated).await {
            let msg = format!("Error updating memory {}: {e}", args.path);
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
            return Ok(msg);
        }

        let msg = format!("Updated memory {} ({} replacement(s))", args.path, replaced);
        self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", 0, &msg);
        Ok(msg)
    }

    async fn exec_memory_forget(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ForgetArgs {
            path: String,
            #[serde(default)]
            match_text: Option<String>,
            #[serde(default)]
            recursive: Option<bool>,
        }

        let args: ForgetArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_forget args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_forget", &args.path);

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
        };

        if let Some(match_text) = args
            .match_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(content) => content,
                Err(e) => {
                    let msg = format!("Error reading memory {}: {e}", args.path);
                    self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                    return Ok(msg);
                }
            };
            let mut removed = 0usize;
            let kept = content
                .lines()
                .filter(|line| {
                    let keep = !line.contains(match_text);
                    if !keep {
                        removed += 1;
                    }
                    keep
                })
                .collect::<Vec<_>>();
            if removed == 0 {
                let msg = format!("No matching memory lines found in {}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
            let mut updated = kept.join("\n");
            if !updated.is_empty() {
                updated.push('\n');
            }
            if let Err(e) = tokio::fs::write(&path, updated).await {
                let msg = format!("Error forgetting memory lines in {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
            let msg = format!(
                "Forgot {} matching line(s) from memory {}",
                removed, args.path
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", 0, &msg);
            return Ok(msg);
        }

        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(e) => {
                let msg = format!("Error reading memory path {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
        };

        let result = if metadata.is_dir() {
            if args.recursive.unwrap_or(false) {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_dir(&path).await
            }
        } else {
            tokio::fs::remove_file(&path).await
        };

        match result {
            Ok(()) => {
                let msg = format!("Forgot memory path {}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", 0, &msg);
                Ok(msg)
            }
            Err(e) => {
                let msg = format!("Error forgetting memory path {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                Ok(msg)
            }
        }
    }

    async fn exec_mcp_list_servers(
        &self,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_list_servers", ".");

        let mut servers: Vec<_> = self
            .mcp_servers
            .values()
            .map(|server| {
                let mut env_keys = server.env.keys().cloned().collect::<Vec<_>>();
                env_keys.sort();
                let mut header_keys = server.headers.keys().cloned().collect::<Vec<_>>();
                header_keys.sort();
                serde_json::json!({
                    "name": server.name,
                    "transport": server.transport,
                    "command": server.command,
                    "args": server.args,
                    "cwd": server.cwd,
                    "url": server.url,
                    "disabled": server.disabled,
                    "envKeys": env_keys,
                    "headerKeys": header_keys,
                })
            })
            .collect();
        servers.sort_by(|a, b| {
            a.get("name")
                .and_then(serde_json::Value::as_str)
                .cmp(&b.get("name").and_then(serde_json::Value::as_str))
        });

        let output = if servers.is_empty() {
            "No MCP servers configured in codey/config.toml".to_string()
        } else {
            serde_json::to_string_pretty(&serde_json::json!({ "servers": servers }))
                .unwrap_or_default()
        };
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_servers",
            0,
            &output,
        );
        Ok(output)
    }

    async fn exec_mcp_status(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
            #[serde(default)]
            probe: Option<bool>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_status", display);

        let mut servers = if let Some(server_name) = args.server.as_deref() {
            match self.mcp_servers.get(server_name) {
                Some(server) => vec![server.clone()],
                None => {
                    let msg = format!("MCP server not configured: {server_name}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "mcp_status", -1, &msg);
                    return Ok(msg);
                }
            }
        } else {
            self.mcp_servers.values().cloned().collect::<Vec<_>>()
        };
        servers.sort_by(|a, b| a.name.cmp(&b.name));

        let probe = args.probe.unwrap_or(true);
        let mut entries = Vec::new();
        for server in servers {
            entries.push(self.mcp_status_entry(&server, probe).await);
        }
        let has_error = entries.iter().any(|entry| {
            entry
                .get("status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|status| status == "error")
        });
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "probe": probe,
            "servers": entries,
        }))
        .unwrap_or_default();
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_status",
            if has_error { -1 } else { 0 },
            &output,
        );
        Ok(output)
    }

    async fn mcp_status_entry(&self, server: &McpServerConfig, probe: bool) -> serde_json::Value {
        let mut entry = base_mcp_status_entry(server);
        entry["session"] = self.mcp_session_status(&server.name).await;
        if server.disabled {
            entry["status"] = serde_json::json!("disabled");
            return entry;
        }
        if !probe {
            entry["status"] = serde_json::json!("configured");
            return entry;
        }

        let probes = [
            ("tools", "tools/list", "tools"),
            ("resources", "resources/list", "resources"),
            (
                "resourceTemplates",
                "resources/templates/list",
                "resourceTemplates",
            ),
            ("prompts", "prompts/list", "prompts"),
        ];
        let mut probe_values = serde_json::Map::new();
        let mut has_error = false;
        for (label, method, field) in probes {
            let result = self
                .mcp_request(server, method, serde_json::json!({}))
                .await;
            match result {
                Ok(value) => {
                    probe_values.insert(
                        label.to_string(),
                        serde_json::json!({
                            "ok": true,
                            "count": mcp_result_array_len(&value, field),
                        }),
                    );
                }
                Err(error) => {
                    has_error = true;
                    probe_values.insert(
                        label.to_string(),
                        serde_json::json!({
                            "ok": false,
                            "error": error,
                        }),
                    );
                }
            }
        }
        entry["status"] = serde_json::json!(if has_error { "error" } else { "ok" });
        entry["probes"] = serde_json::Value::Object(probe_values);
        entry
    }

    async fn exec_mcp_list_tools(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_list_tools", display);
        let started = Instant::now();
        crate::agent::emit_and_broadcast(
            app_handle,
            "turn-loading",
            serde_json::json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "callId": call_id,
                "kind": "mcp",
                "phase": "catalog",
                "status": "started",
                "server": display,
            }),
        );

        let output = self
            .mcp_request_for_selection(args.server.as_deref(), "tools/list", serde_json::json!({}))
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "tools");
        crate::agent::emit_and_broadcast(
            app_handle,
            "turn-loading",
            serde_json::json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "callId": call_id,
                "kind": "mcp",
                "phase": "catalog",
                "status": if exit_code == 0 { "completed" } else { "failed" },
                "server": display,
                "durationMs": started.elapsed().as_millis() as u64,
                "error": if exit_code == 0 { serde_json::Value::Null } else { serde_json::json!(&text) },
            }),
        );
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_tools",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_call_tool(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            tool: String,
            #[serde(default)]
            arguments: Option<serde_json::Value>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid mcp_call_tool args: {e}"))
        })?;
        let display = format!("{}:{}", args.server, args.tool);
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_call_tool", &display);

        let server = match self.mcp_server(&args.server) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_call_tool", -1, &msg);
                return Ok(msg);
            }
        };
        let params = serde_json::json!({
            "name": args.tool,
            "arguments": args.arguments.unwrap_or_else(|| serde_json::json!({})),
        });
        let result = self.mcp_request(server, "tools/call", params).await;
        let (exit_code, text) = format_mcp_single_result(result);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_call_tool",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_direct_tool(
        &self,
        visible_tool_name: &str,
        server_name: &str,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let display = format!("{server_name}:{tool_name}");
        self.emit_tool_start(app_handle, thread_id, call_id, visible_tool_name, &display);

        let server = match self.mcp_server(server_name) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, visible_tool_name, -1, &msg);
                return Ok(msg);
            }
        };

        let tool_arguments = if arguments.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str::<serde_json::Value>(arguments) {
                Ok(value) if value.is_object() => value,
                Ok(_) => {
                    let msg = format!(
                        "Invalid arguments for MCP tool {visible_tool_name}: expected a JSON object"
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, visible_tool_name, -1, &msg);
                    return Ok(msg);
                }
                Err(e) => {
                    let msg = format!("Invalid arguments for MCP tool {visible_tool_name}: {e}");
                    self.emit_tool_end(app_handle, thread_id, call_id, visible_tool_name, -1, &msg);
                    return Ok(msg);
                }
            }
        };

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": tool_arguments,
        });
        let result = self.mcp_request(server, "tools/call", params).await;
        let (exit_code, text) = format_mcp_single_result(result);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            visible_tool_name,
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_list_resources(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resources",
            display,
        );

        let output = self
            .mcp_request_for_selection(
                args.server.as_deref(),
                "resources/list",
                serde_json::json!({}),
            )
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "resources");
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resources",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_read_resource(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            uri: String,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid mcp_read_resource args: {e}"))
        })?;
        let display = format!("{}:{}", args.server, args.uri);
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "mcp_read_resource",
            &display,
        );

        let server = match self.mcp_server(&args.server) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "mcp_read_resource",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };
        let result = self
            .mcp_request(
                server,
                "resources/read",
                serde_json::json!({ "uri": args.uri }),
            )
            .await;
        let (exit_code, text) = format_mcp_single_result(result);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_read_resource",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_list_resource_templates(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resource_templates",
            display,
        );

        let output = self
            .mcp_request_for_selection(
                args.server.as_deref(),
                "resources/templates/list",
                serde_json::json!({}),
            )
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "resourceTemplates");
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_resource_templates",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_list_prompts(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            server: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).unwrap_or_default();
        let display = args.server.as_deref().unwrap_or("all");
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_list_prompts", display);

        let output = self
            .mcp_request_for_selection(
                args.server.as_deref(),
                "prompts/list",
                serde_json::json!({}),
            )
            .await;
        let (exit_code, text) = format_mcp_selection_result(output, "prompts");
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_list_prompts",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn exec_mcp_get_prompt(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            prompt: String,
            #[serde(default)]
            arguments: Option<serde_json::Value>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid mcp_get_prompt args: {e}"))
        })?;
        let display = format!("{}:{}", args.server, args.prompt);
        self.emit_tool_start(app_handle, thread_id, call_id, "mcp_get_prompt", &display);

        let server = match self.mcp_server(&args.server) {
            Ok(server) => server,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "mcp_get_prompt", -1, &msg);
                return Ok(msg);
            }
        };
        let result = self
            .mcp_request(
                server,
                "prompts/get",
                serde_json::json!({
                    "name": args.prompt,
                    "arguments": args.arguments.unwrap_or_else(|| serde_json::json!({})),
                }),
            )
            .await;
        let (exit_code, text) = format_mcp_single_result(result);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "mcp_get_prompt",
            exit_code,
            &text,
        );
        Ok(text)
    }

    async fn mcp_request_for_selection(
        &self,
        server: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Vec<(String, Result<serde_json::Value, String>)> {
        if let Some(server_name) = server {
            return match self.mcp_server(server_name) {
                Ok(server) => {
                    vec![(
                        server.name.clone(),
                        self.mcp_request(server, method, params).await,
                    )]
                }
                Err(msg) => vec![(server_name.to_string(), Err(msg))],
            };
        }

        let mut servers = self.enabled_mcp_servers();
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        if servers.is_empty() {
            return vec![(
                "all".to_string(),
                Err("No enabled MCP servers configured in codey/config.toml".to_string()),
            )];
        }

        let mut results = Vec::new();
        for server in servers {
            let name = server.name.clone();
            results.push((
                name,
                self.mcp_request(&server, method, params.clone()).await,
            ));
        }
        results
    }

    async fn discover_mcp_direct_tool_specs(&mut self) -> Vec<serde_json::Value> {
        if !self.mcp_discovery_needs_refresh() {
            return sorted_mcp_tool_specs(&self.mcp_tool_specs);
        }

        self.mcp_tool_aliases.clear();
        self.mcp_tool_specs.clear();
        let mut servers = self.enabled_mcp_servers();
        servers.sort_by(|a, b| a.name.cmp(&b.name));

        let mut used_names = BTreeSet::new();
        let mut specs = Vec::new();
        let discovery_timeout = Duration::from_secs(6);
        let requests = servers.iter().map(|server| async {
            let result = tokio::time::timeout(
                discovery_timeout,
                self.mcp_request(server, "tools/list", serde_json::json!({})),
            )
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "MCP server '{}' catalog discovery timed out after {} seconds",
                    server.name,
                    discovery_timeout.as_secs()
                ))
            });
            (server.clone(), result)
        });
        let results = futures_util::future::join_all(requests).await;
        let mut failed_servers = Vec::new();

        for (server, result) in results {
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    warn!(
                        "MCP catalog discovery failed for '{}': {error}",
                        server.name
                    );
                    failed_servers.push(server.name.clone());
                    continue;
                }
            };
            let Some(tools) = result.get("tools").and_then(serde_json::Value::as_array) else {
                continue;
            };

            for tool in tools {
                let Some(tool_name) = tool.get("name").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let alias = mcp_direct_tool_name(&server.name, tool_name, &mut used_names);
                let connector = mcp_connector_metadata(&server.name, tool);
                self.mcp_tool_aliases.insert(
                    alias.clone(),
                    McpToolAlias {
                        server: server.name.clone(),
                        tool: tool_name.to_string(),
                        connector,
                    },
                );
                let spec = mcp_direct_tool_spec(&server.name, tool_name, tool, &alias);
                self.mcp_tool_specs.insert(alias, spec.clone());
                specs.push(spec);
            }
        }

        self.mcp_direct_tools_discovered = failed_servers.is_empty();
        self.mcp_discovery_retry_after = if failed_servers.is_empty() {
            None
        } else {
            warn!(
                "MCP catalog discovery failed for {}; keeping partial catalog and retrying after cooldown",
                failed_servers.join(", ")
            );
            Some(Instant::now() + Duration::from_secs(300))
        };
        specs
    }

    pub fn mcp_discovery_needs_refresh(&self) -> bool {
        if self.mcp_direct_tools_discovered {
            return false;
        }
        self.mcp_discovery_retry_after
            .is_none_or(|retry_after| Instant::now() >= retry_after)
    }

    async fn mcp_request(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if server.disabled {
            return Err(format!("MCP server '{}' is disabled", server.name));
        }

        let timeout = std::time::Duration::from_secs(20);
        tokio::time::timeout(timeout, self.mcp_request_inner(server, method, params))
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "MCP server '{}' timed out after 20 seconds",
                    server.name
                ))
            })
    }

    async fn mcp_request_inner(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if server.is_http_transport() {
            self.mcp_http_request_with_session(server, method, params)
                .await
                .map_err(McpRequestError::message)
        } else {
            self.mcp_request_with_session(server, method, params)
                .await
                .map_err(McpRequestError::message)
        }
    }

    async fn mcp_http_request_with_session(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let (session, reused) = self.mcp_http_session(server).await?;
        let first = {
            let mut session = session.lock().await;
            self.request_mcp_http_session(&mut session, method, params.clone())
                .await
        };

        match first {
            Ok(value) => Ok(value),
            Err(error) if error.is_transport() && reused => {
                self.remove_mcp_http_session(&server.name).await;
                let (session, _) = self.mcp_http_session(server).await?;
                let retry = {
                    let mut session = session.lock().await;
                    self.request_mcp_http_session(&mut session, method, params)
                        .await
                };
                if retry
                    .as_ref()
                    .err()
                    .is_some_and(McpRequestError::is_transport)
                {
                    self.remove_mcp_http_session(&server.name).await;
                }
                retry
            }
            Err(error) => {
                if error.is_transport() {
                    self.remove_mcp_http_session(&server.name).await;
                }
                Err(error)
            }
        }
    }

    async fn request_mcp_http_session(
        &self,
        session: &mut McpHttpSession,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let request_id = session.next_request_id;
        session.next_request_id += 1;
        let (response, session_id) = self
            .send_mcp_http_jsonrpc(
                &session.server,
                session.session_id.as_deref(),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "method": method,
                    "params": params,
                }),
            )
            .await?;
        if session_id.is_some() {
            session.session_id = session_id;
        }
        session.request_count += 1;

        let response = response.ok_or_else(|| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned an empty response for request id {request_id}",
                session.server.name
            ))
        })?;
        if response.get("id").and_then(serde_json::Value::as_i64) != Some(request_id) {
            return Err(McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned response for unexpected id: {}",
                session.server.name,
                format_json_value(&response)
            )));
        }
        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP HTTP server '{}' returned error: {}",
                session.server.name,
                format_json_value(error)
            )));
        }
        Ok(response.get("result").cloned().unwrap_or(response))
    }

    async fn mcp_request_with_session(
        &self,
        server: &McpServerConfig,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpRequestError> {
        let (session, reused) = self.mcp_session(server).await?;
        let first = {
            let mut session = session.lock().await;
            session.request(method, params.clone()).await
        };

        match first {
            Ok(value) => Ok(value),
            Err(error) if error.is_transport() && reused => {
                self.remove_mcp_session(&server.name).await;
                let (session, _) = self.mcp_session(server).await?;
                let retry = {
                    let mut session = session.lock().await;
                    session.request(method, params).await
                };
                if retry
                    .as_ref()
                    .err()
                    .is_some_and(McpRequestError::is_transport)
                {
                    self.remove_mcp_session(&server.name).await;
                }
                retry
            }
            Err(error) => {
                if error.is_transport() {
                    self.remove_mcp_session(&server.name).await;
                }
                Err(error)
            }
        }
    }

    async fn mcp_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<(Arc<Mutex<McpSession>>, bool), McpRequestError> {
        if let Some(existing) = self.mcp_sessions.lock().await.get(&server.name).cloned() {
            let matches_config = {
                let session = existing.lock().await;
                session.server == *server
            };
            if matches_config {
                return Ok((existing, true));
            }
            self.remove_mcp_session(&server.name).await;
        }

        let session = Arc::new(Mutex::new(self.start_mcp_session(server).await?));
        self.mcp_sessions
            .lock()
            .await
            .insert(server.name.clone(), session.clone());
        Ok((session, false))
    }

    async fn mcp_http_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<(Arc<Mutex<McpHttpSession>>, bool), McpRequestError> {
        if let Some(existing) = self
            .mcp_http_sessions
            .lock()
            .await
            .get(&server.name)
            .cloned()
        {
            let matches_config = {
                let session = existing.lock().await;
                session.server == *server
            };
            if matches_config {
                return Ok((existing, true));
            }
            self.remove_mcp_http_session(&server.name).await;
        }

        let session = Arc::new(Mutex::new(self.start_mcp_http_session(server).await?));
        self.mcp_http_sessions
            .lock()
            .await
            .insert(server.name.clone(), session.clone());
        Ok((session, false))
    }

    async fn start_mcp_http_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<McpHttpSession, McpRequestError> {
        let init_id = 1;
        let (response, session_id) = self
            .send_mcp_http_jsonrpc(
                server,
                None,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "CN-Codex",
                            "version": "0.1.0"
                        }
                    }
                }),
            )
            .await?;

        let response = response.ok_or_else(|| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned an empty initialize response",
                server.name
            ))
        })?;
        if let Some(error) = response.get("error") {
            return Err(McpRequestError::Rpc(format!(
                "MCP HTTP server '{}' initialize failed: {}",
                server.name,
                format_json_value(error)
            )));
        }

        let mut session = McpHttpSession {
            server: server.clone(),
            session_id,
            next_request_id: 2,
            request_count: 0,
            initialized_at_ms: now_millis(),
        };

        let (_, session_id) = self
            .send_mcp_http_jsonrpc(
                server,
                session.session_id.as_deref(),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                }),
            )
            .await?;
        if session_id.is_some() {
            session.session_id = session_id;
        }

        Ok(session)
    }

    async fn send_mcp_http_jsonrpc(
        &self,
        server: &McpServerConfig,
        session_id: Option<&str>,
        message: serde_json::Value,
    ) -> Result<(Option<serde_json::Value>, Option<String>), McpRequestError> {
        let Some(url) = server
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            return Err(McpRequestError::Transport(format!(
                "MCP HTTP server '{}' is missing url",
                server.name
            )));
        };

        let mut request = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            );
        if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
            request = request.header("mcp-session-id", session_id);
        }
        for (key, value) in &server.headers {
            let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP HTTP server '{}' has invalid header name '{}'",
                    server.name, key
                )));
            };
            let Ok(value) = reqwest::header::HeaderValue::from_str(value) else {
                return Err(McpRequestError::Transport(format!(
                    "MCP HTTP server '{}' has invalid value for header '{}'",
                    server.name, key
                )));
            };
            request = request.header(name, value);
        }

        let response = request.json(&message).send().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' request failed: {error}",
                server.name
            ))
        })?;
        let status = response.status();
        let response_session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response.text().await.map_err(|error| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' response read failed: {error}",
                server.name
            ))
        })?;

        if !status.is_success() {
            return Err(McpRequestError::Transport(format!(
                "MCP HTTP server '{}' returned HTTP {status}: {}",
                server.name,
                truncate_output(&body, 2000)
            )));
        }

        let parsed = parse_mcp_http_response_body(&body, &content_type).map_err(|message| {
            McpRequestError::Transport(format!(
                "MCP HTTP server '{}' response parse failed: {message}",
                server.name
            ))
        })?;
        Ok((parsed, response_session_id))
    }

    async fn start_mcp_session(
        &self,
        server: &McpServerConfig,
    ) -> Result<McpSession, McpRequestError> {
        let program = resolve_mcp_command_for_platform(&server.command);
        let mut command = Command::new(&program);
        command.envs(&server.env);
        if should_inject_bundled_node_runtime(&program) {
            if let Some(node_dir) = bundled_node_bin_dir(&self.workspace_config_dir) {
                let existing_path = server_env_path_override(server)
                    .map(std::ffi::OsString::from)
                    .or_else(|| std::env::var_os("PATH"));
                let merged_path = prepend_path_value(existing_path, &node_dir);
                command.env("PATH", &merged_path);
                #[cfg(target_os = "windows")]
                command.env("Path", merged_path);
            }
        }
        command
            .args(&server.args)
            .current_dir(resolve_command_cwd(&self.cwd, server.cwd.as_deref()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.no_console();

        let mut child = command.spawn().map_err(|e| {
            McpRequestError::Transport(format!("Failed to start MCP server '{}': {e}", server.name))
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            McpRequestError::Transport(format!("MCP server '{}' stdin unavailable", server.name))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpRequestError::Transport(format!("MCP server '{}' stdout unavailable", server.name))
        })?;
        let stderr = child.stderr.take();
        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        collect_mcp_stderr(stderr, stderr_buffer.clone());

        let mut reader = BufReader::new(stdout);

        if let Err(error) = write_mcp_message(
            &mut stdin,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "CN-Codex",
                        "version": "0.1.0"
                    }
                }
            }),
        )
        .await
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(mcp_transport_error(&server.name, &stderr_buffer, error).await);
        }
        let init_response = match read_mcp_response(&mut reader, 1).await {
            Ok(response) => response,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(mcp_transport_error(&server.name, &stderr_buffer, error).await);
            }
        };
        if let Some(error) = init_response.get("error") {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(McpRequestError::Rpc(format!(
                "MCP server '{}' initialize failed: {}",
                server.name,
                format_json_value(error)
            )));
        }

        if let Err(error) = write_mcp_message(
            &mut stdin,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }),
        )
        .await
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(mcp_transport_error(&server.name, &stderr_buffer, error).await);
        }

        Ok(McpSession {
            server: server.clone(),
            stdin,
            reader,
            child,
            stderr: stderr_buffer,
            next_request_id: 2,
            request_count: 0,
            initialized_at_ms: now_millis(),
        })
    }

    async fn remove_mcp_session(&self, server_name: &str) {
        let session = self.mcp_sessions.lock().await.remove(server_name);
        if let Some(session) = session {
            close_mcp_session(session).await;
        }
    }

    async fn remove_mcp_http_session(&self, server_name: &str) {
        self.mcp_http_sessions.lock().await.remove(server_name);
    }

    async fn mcp_session_status(&self, server_name: &str) -> serde_json::Value {
        let session = self.mcp_sessions.lock().await.get(server_name).cloned();
        if let Some(session) = session {
            let session = session.lock().await;
            return serde_json::json!({
                "connected": true,
                "transport": "stdio",
                "requestCount": session.request_count,
                "initializedAtMs": session.initialized_at_ms,
            });
        }
        let session = self
            .mcp_http_sessions
            .lock()
            .await
            .get(server_name)
            .cloned();
        if let Some(session) = session {
            let session = session.lock().await;
            return serde_json::json!({
                "connected": true,
                "transport": "http",
                "requestCount": session.request_count,
                "initializedAtMs": session.initialized_at_ms,
                "sessionIdPresent": session.session_id.is_some(),
            });
        }
        serde_json::json!({ "connected": false })
    }

    fn enabled_mcp_servers(&self) -> Vec<McpServerConfig> {
        self.mcp_servers
            .values()
            .filter(|server| !server.disabled)
            .cloned()
            .collect()
    }

    pub fn mcp_loading_snapshot(&self) -> (usize, usize, bool) {
        (
            self.enabled_mcp_servers().len(),
            self.mcp_tool_specs.len(),
            self.mcp_direct_tools_discovered,
        )
    }

    fn mcp_server(&self, name: &str) -> Result<&McpServerConfig, String> {
        self.mcp_servers
            .get(name)
            .ok_or_else(|| format!("MCP server not configured: {name}"))
            .and_then(|server| {
                if server.disabled {
                    Err(format!("MCP server '{name}' is disabled"))
                } else {
                    Ok(server)
                }
            })
    }

    async fn exec_web_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct SearchArgs {
            query: String,
            #[serde(default)]
            max_results: Option<usize>,
        }

        let args: SearchArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid web_search args: {e}")))?;
        let query = args.query.trim();
        let max_results = args.max_results.unwrap_or(5).clamp(1, 10);

        self.emit_tool_start(app_handle, thread_id, call_id, "web_search", query);

        if query.is_empty() {
            let msg = "Error: empty search query".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "web_search", -1, &msg);
            return Ok(msg);
        }

        info!("Searching web: {query}");
        let mut fallback_output =
            format!("No web search results found for: {query} (tried Bing and DuckDuckGo)");

        for provider in web_search_provider_order() {
            match provider {
                WebSearchProvider::BingBrowser => {
                    if let Some(output) = self
                        .search_bing_browser(query, max_results, call_id, app_handle, thread_id)
                        .await
                    {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "web_search",
                            0,
                            &output,
                        );
                        return Ok(output);
                    }
                    info!("Bing browser returned no results, falling back to DuckDuckGo");
                }
                WebSearchProvider::DuckDuckGoApi => {
                    let output = match self.search_duckduckgo_api(query, max_results).await {
                        Ok(output) => output,
                        Err(err) => {
                            warn!("DuckDuckGo API search failed, trying browser fallback: {err}");
                            continue;
                        }
                    };
                    if web_search_output_has_results(&output) {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "web_search",
                            0,
                            &output,
                        );
                        return Ok(output);
                    }
                    info!("DuckDuckGo API returned no results, trying browser fallback");
                }
                WebSearchProvider::DuckDuckGoBrowser => {
                    let output = self
                        .fallback_browser_search(query, max_results, call_id, app_handle, thread_id)
                        .await;
                    if web_search_output_has_results(&output) {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "web_search",
                            0,
                            &output,
                        );
                        return Ok(output);
                    }
                    fallback_output = output;
                }
            }
        }

        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "web_search",
            0,
            &fallback_output,
        );
        Ok(fallback_output)
    }

    async fn fallback_browser_search(
        &self,
        query: &str,
        max_results: usize,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> String {
        let ddg_extract_script = r#"
            (() => {
                const results = [];
                document.querySelectorAll('.result, .nrn-react-div').forEach(el => {
                    const a = el.querySelector('.result__a, a.result-link');
                    const snippet = el.querySelector('.result__snippet, .result__body');
                    if (a) {
                        results.push({
                            title: a.innerText.trim(),
                            url: a.href || '',
                            snippet: snippet ? snippet.innerText.trim() : ''
                        });
                    }
                });
                return JSON.stringify(results);
            })()
        "#;

        let ddg_url = format!(
            "https://duckduckgo.com/?q={}",
            encode_query_component(query)
        );
        let browser_args_ddg = serde_json::json!({
            "engine": "webview-js-injection",
            "url": ddg_url,
            "waitUntil": "networkidle",
            "use_visible_browser": true,
            "actions": [
                { "type": "wait_for_timeout", "ms": 3000 },
                { "type": "eval", "script": ddg_extract_script },
                { "type": "title" },
                { "type": "url" }
            ]
        });

        let ddg_output = self
            .exec_browser_run(
                &browser_args_ddg.to_string(),
                call_id,
                app_handle,
                thread_id,
            )
            .await;

        if let Ok(ref raw) = ddg_output {
            if let Some(results) = parse_browser_search_results(raw, max_results) {
                if !results.is_empty() {
                    return format_browser_search_results(query, &results);
                }
            }
        }

        format!("No web search results found for: {query} (tried Bing and DuckDuckGo)")
    }

    async fn search_bing_browser(
        &self,
        query: &str,
        max_results: usize,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> Option<String> {
        let bing_extract_script = r#"
            (() => {
                const results = [];
                document.querySelectorAll('li.b_algo').forEach(el => {
                    const a = el.querySelector('h2 a');
                    const snippet = el.querySelector('.b_caption p, .b_lineclamp2, .b_algoSlug');
                    if (a) {
                        results.push({
                            title: a.innerText.trim(),
                            url: a.href || '',
                            snippet: snippet ? snippet.innerText.trim() : ''
                        });
                    }
                });
                return JSON.stringify(results);
            })()
        "#;

        let bing_url = format!(
            "https://cn.bing.com/search?q={}",
            encode_query_component(query)
        );
        let browser_args_bing = serde_json::json!({
            "engine": "webview-js-injection",
            "url": bing_url,
            "waitUntil": "networkidle",
            "use_visible_browser": true,
            "actions": [
                { "type": "wait_for_timeout", "ms": 2500 },
                { "type": "eval", "script": bing_extract_script },
                { "type": "title" },
                { "type": "url" }
            ]
        });

        let bing_output = self
            .exec_browser_run(
                &browser_args_bing.to_string(),
                call_id,
                app_handle,
                thread_id,
            )
            .await
            .ok()?;

        let results = parse_browser_search_results(&bing_output, max_results)?;
        if results.is_empty() {
            return None;
        }
        Some(format_browser_search_results(query, &results))
    }

    async fn search_duckduckgo_api(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<String, String> {
        let search_url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            encode_query_component(query)
        );
        let response = self
            .http
            .get(search_url)
            .send()
            .await
            .map_err(|e| format!("Web search request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Web search HTTP error {status}: {}",
                truncate_output(&body, 1000)
            ));
        }

        let parsed = response
            .json::<DuckDuckGoResponse>()
            .await
            .map_err(|e| format!("Web search response parse failed: {e}"))?;
        Ok(format_duckduckgo_results(query, parsed, max_results))
    }

    async fn exec_web_fetch(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct FetchArgs {
            url: String,
            #[serde(default)]
            max_chars: Option<usize>,
        }

        let args: FetchArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid web_fetch args: {e}")))?;
        let url = args.url.trim();
        let max_chars = args.max_chars.unwrap_or(8000).clamp(1000, 20_000);

        self.emit_tool_start(app_handle, thread_id, call_id, "web_fetch", url);

        if !(url.starts_with("http://") || url.starts_with("https://")) {
            let msg = "Error: web_fetch only supports http:// and https:// URLs".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
            return Ok(msg);
        }

        info!("Fetching web page: {url}");

        let response = match self.http.get(url).send().await {
            Ok(response) => response,
            Err(e) => {
                let msg = format!("Web fetch request failed: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
                return Ok(msg);
            }
        };

        let status = response.status();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let msg = format!(
                "Web fetch HTTP error {status} for {final_url}: {}",
                truncate_output(&body, 1000)
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
            return Ok(msg);
        }

        let raw = match response.text().await {
            Ok(raw) => raw,
            Err(e) => {
                let msg = format!("Web fetch body read failed: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
                return Ok(msg);
            }
        };

        let looks_html = content_type.to_ascii_lowercase().contains("text/html")
            || raw
                .chars()
                .take(500)
                .collect::<String>()
                .to_ascii_lowercase()
                .contains("<html");
        let title = if looks_html {
            extract_html_title(&raw).unwrap_or_default()
        } else {
            String::new()
        };
        let body = if looks_html {
            html_to_text(&raw)
        } else {
            condense_whitespace(&raw)
        };
        let body = truncate_output(&body, max_chars);

        let mut output = format!("URL: {final_url}\nStatus: {status}\n");
        if !content_type.is_empty() {
            output.push_str(&format!("Content-Type: {content_type}\n"));
        }
        if !title.is_empty() {
            output.push_str(&format!("Title: {title}\n"));
        }
        output.push('\n');
        output.push_str(&body);

        self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", 0, &output);
        Ok(output)
    }

    fn memories_dir(&self) -> PathBuf {
        self.workspace_config_dir.join("memories")
    }

    fn smartbrain_is_active(&self) -> bool {
        if let Some(enabled) = self.smartbrain_enabled_override {
            return enabled;
        }
        let config_path = self.workspace_config_dir.join("config.toml");
        ConfigToml::load(&config_path)
            .map(|config| config.smartbrain_config().knowledge_is_active())
            .unwrap_or(false)
    }

    fn image_generation_is_enabled(&self) -> bool {
        self.read_image_generation_settings().enabled
    }

    fn read_image_generation_settings(&self) -> ImageGenerationSettingsResolved {
        let config_path = self.workspace_config_dir.join("config.toml");
        let config = match ConfigToml::load(&config_path) {
            Ok(config) => config,
            Err(_) => return ImageGenerationSettingsResolved::default(),
        };
        let image_generation = config.image_generation_config();
        ImageGenerationSettingsResolved {
            enabled: image_generation.is_enabled(),
            model: image_generation.model.and_then(non_empty_string),
            base_url: image_generation.base_url.and_then(non_empty_string),
            api_key: image_generation.api_key.and_then(non_empty_string),
        }
    }

    fn resolve_memory_path(&self, path: &str) -> Result<PathBuf, String> {
        resolve_memory_path(&self.memories_dir(), path)
    }

    /// Track experience usage when the model reads files under `experiences/`.
    ///
    /// Track usage of SmartBrain content (experience or knowledge) when
    /// the model reads it via memory_read.
    fn track_experience_usage(&self, memory_path: &str) {
        let normalized = memory_path.replace('\\', "/");

        if normalized.starts_with("experiences/") {
            let experiences_dir = crate::smartbrain::experiences_dir(&self.workspace_config_dir);
            let mut index = crate::smartbrain::index::ExperienceIndex::load(&experiences_dir);

            if let Some(rest) = normalized.strip_prefix("experiences/raw/") {
                if let Some(thread_id) = rest.strip_suffix(".md") {
                    index.record_usage(thread_id);
                    let _ = index.save(&experiences_dir);
                }
            } else if normalized == "experiences/experience_handbook.md"
                || normalized == "experiences/experience_summary.md"
            {
                index.record_usage_all();
                let _ = index.save(&experiences_dir);
            }
        }
    }
}

enum WaitChildResult {
    Exited(std::process::ExitStatus),
    TimedOut,
    Failed(String),
}

/// 以短轮询方式等待子进程结束。
///
/// 说明：
/// - 不直接 `child.wait().await`，避免在等待期间长时间独占 child 的可变借用；
/// - 这样 interrupt 逻辑仍可获取 child 并执行 kill。
async fn wait_for_child_with_timeout(
    child: &Arc<Mutex<Child>>,
    timeout_ms: u64,
) -> WaitChildResult {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let wait_result = {
            let mut guard = child.lock().await;
            guard.try_wait()
        };

        match wait_result {
            Ok(Some(status)) => return WaitChildResult::Exited(status),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    return WaitChildResult::TimedOut;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return WaitChildResult::Failed(error.to_string()),
        }
    }
}

async fn collect_shell_stream_bytes<R>(mut reader: R, output: Arc<Mutex<Vec<u8>>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buf = [0u8; 4096];
    loop {
        let Ok(n) = reader.read(&mut buf).await else {
            break;
        };
        if n == 0 {
            break;
        }
        output.lock().await.extend_from_slice(&buf[..n]);
    }
}

async fn wait_for_shell_stream_task(handle: &mut tokio::task::JoinHandle<()>, timeout_ms: u64) {
    if tokio::time::timeout(Duration::from_millis(timeout_ms), &mut *handle)
        .await
        .is_err()
    {
        handle.abort();
    }
}

async fn terminate_shell_child(child: &Arc<Mutex<Child>>) {
    let pid = {
        let guard = child.lock().await;
        guard.id()
    };
    if let Some(pid) = pid {
        if let Err(error) = kill_process_tree(pid).await {
            info!("Failed to kill process tree {pid}, fallback to child.kill(): {error}");
        }
    }

    let mut guard = child.lock().await;
    let _ = guard.kill().await;
}

fn apply_browser_defaults(_workspace_config_dir: &Path, payload: &mut serde_json::Value) {
    if let Some(object) = payload.as_object_mut() {
        // 兼容历史 prompt：保留传入 engine，但统一映射到新引擎。
        let requested_engine = object
            .get("engine")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("webview-js-injection")
            .to_string();
        if requested_engine != "webview-js-injection" {
            object.insert(
                "engineRequested".to_string(),
                serde_json::Value::String(requested_engine),
            );
        }
        object.insert(
            "engine".to_string(),
            serde_json::Value::String("webview-js-injection".to_string()),
        );
        object
            .entry("use_visible_browser".to_string())
            .or_insert_with(|| serde_json::Value::Bool(true));
    }
}

fn browser_run_display(payload: &serde_json::Value) -> String {
    let url = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .unwrap_or("browser");
    let action_count = payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if action_count == 0 {
        url.to_string()
    } else {
        format!("{url} ({action_count} actions)")
    }
}

#[allow(dead_code)]
fn browser_run_use_visible_browser(payload: &serde_json::Value) -> bool {
    payload
        .get("use_visible_browser")
        .or_else(|| payload.get("useVisibleBrowser"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

#[allow(dead_code)]
fn browser_run_initial_url(payload: &serde_json::Value) -> Option<String> {
    if let Some(url) = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
    {
        return Some(url.trim().to_string());
    }

    payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .and_then(|actions| {
            actions.iter().find_map(|action| {
                let action_type = action.get("type")?.as_str()?;
                if action_type != "goto" {
                    return None;
                }
                action
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .filter(|url| !url.trim().is_empty())
                    .map(|url| url.trim().to_string())
            })
        })
}

fn subagent_state_path(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("subagents").join("state.json")
}

fn load_subagent_records(workspace_config_dir: &Path) -> HashMap<String, SubagentRecord> {
    let path = subagent_state_path(workspace_config_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return HashMap::new(),
    };
    let mut records = match serde_json::from_slice::<Vec<SubagentRecord>>(&bytes) {
        Ok(records) => records,
        Err(_) => return HashMap::new(),
    };
    let loaded_at_ms = now_millis();
    for record in &mut records {
        record.process_id = None;
        if record.status == "running" {
            record.status = "interrupted".to_string();
            record.completed_at_ms.get_or_insert(loaded_at_ms);
            record
                .duration_ms
                .get_or_insert_with(|| loaded_at_ms.saturating_sub(record.started_at_ms));
            record.error.get_or_insert_with(|| {
                "Subagent was running when CN-Codex last stopped; process state could not be restored."
                    .to_string()
            });
        }
    }
    records
        .into_iter()
        .map(|record| (record.id.clone(), record))
        .collect()
}

async fn persist_subagent_records(
    workspace_config_dir: &Path,
    subagents: &Arc<Mutex<HashMap<String, SubagentRecord>>>,
) {
    let mut records = subagents.lock().await.values().cloned().collect::<Vec<_>>();
    records.sort_by(|left, right| left.started_at_ms.cmp(&right.started_at_ms));
    let path = subagent_state_path(workspace_config_dir);
    if let Some(parent) = path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return;
        }
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&records) {
        let _ = tokio::fs::write(path, bytes).await;
    }
}

fn send_input_target(args: &SendInputArgs) -> Option<String> {
    [
        args.target.as_deref(),
        args.agent_id.as_deref(),
        args.id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.trim().to_string())
    .find(|value| !value.is_empty())
}

fn send_input_message(args: &SendInputArgs) -> Result<String, String> {
    if let Some(message) = args
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(message.to_string());
    }

    let Some(items) = args.items.as_ref() else {
        return Err("send_input message or items must not be empty".to_string());
    };
    let parts = items
        .iter()
        .filter_map(send_input_item_text)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        Err("send_input message or items must not be empty".to_string())
    } else {
        Ok(parts.join("\n"))
    }
}

fn send_input_item_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => {
            for key in ["text", "content", "input_text", "message"] {
                if let Some(text) = map.get(key).and_then(serde_json::Value::as_str) {
                    return Some(text.to_string());
                }
            }
            Some(value.to_string())
        }
        _ => Some(value.to_string()),
    }
}

fn resume_agent_target(args: &ResumeAgentArgs) -> Option<String> {
    [
        args.id.as_deref(),
        args.target.as_deref(),
        args.agent_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.trim().to_string())
    .find(|value| !value.is_empty())
}

fn close_agent_target(args: CloseAgentArgs) -> Option<String> {
    [args.target, args.agent_id, args.id]
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

#[allow(dead_code)]
async fn kill_process_tree(pid: u32) -> Result<(), String> {
    let output = if cfg!(windows) {
        let pid = pid.to_string();
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", pid.as_str(), "/F", "/T"]);
        #[cfg(windows)]
        cmd.no_console();
        cmd.output().await
    } else {
        let pid = pid.to_string();
        Command::new("kill")
            .args(["-TERM", pid.as_str()])
            .output()
            .await
    }
    .map_err(|e| format!("Failed to stop process {pid}: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = decode_command_output_bytes(&output.stdout)
        .trim()
        .to_string();
    let stderr = decode_command_output_bytes(&output.stderr)
        .trim()
        .to_string();
    let details = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit code {}", output.status.code().unwrap_or(-1))
    };
    Err(format!("Failed to stop process {pid}: {details}"))
}

async fn collect_wait_agent_ids(
    subagents: &Arc<Mutex<HashMap<String, SubagentRecord>>>,
    agent_id: Option<String>,
    agent_ids: Option<Vec<String>>,
) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(id) = agent_id.map(|value| value.trim().to_string()) {
        if !id.is_empty() {
            ids.push(id);
        }
    }
    if let Some(values) = agent_ids {
        for id in values {
            let id = id.trim().to_string();
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    if !ids.is_empty() {
        return ids;
    }

    subagents
        .lock()
        .await
        .values()
        .filter(|record| record.status == "running")
        .map(|record| record.id.clone())
        .collect()
}

async fn wait_for_subagents(
    subagents: Arc<Mutex<HashMap<String, SubagentRecord>>>,
    ids: Vec<String>,
    timeout_ms: u64,
) -> SubagentWaitResult {
    let started = std::time::Instant::now();
    loop {
        let (records, missing, all_finished) = {
            let subagents = subagents.lock().await;
            let mut records = Vec::new();
            let mut missing = Vec::new();
            let mut all_finished = true;
            for id in &ids {
                match subagents.get(id) {
                    Some(record) => {
                        if record.status == "running" {
                            all_finished = false;
                        }
                        records.push(record.clone());
                    }
                    None => missing.push(id.clone()),
                }
            }
            (records, missing, all_finished)
        };

        if all_finished || started.elapsed().as_millis() >= u128::from(timeout_ms) {
            let has_missing = !missing.is_empty();
            let has_failed = records
                .iter()
                .any(|record| matches!(record.status.as_str(), "failed" | "timed_out"));
            return SubagentWaitResult {
                output: format_subagent_wait_output(&records, &missing, all_finished),
                has_missing,
                has_failed,
            };
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

fn format_subagent_wait_output(
    records: &[SubagentRecord],
    missing: &[String],
    completed: bool,
) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "completed": completed,
        "missing": missing,
        "agents": records,
    }))
    .unwrap_or_default()
}

fn format_subagent_records(records: &[SubagentRecord], completed: bool) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "completed": completed,
        "agents": records,
    }))
    .unwrap_or_default()
}

fn resolve_subagent_cwd(root: &Path, input: Option<&str>) -> Result<PathBuf, String> {
    let path = match input.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            let candidate = PathBuf::from(value);
            if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            }
        }
        None => root.to_path_buf(),
    };
    if !path.is_dir() {
        return Err(format!(
            "Subagent cwd is not a directory: {}",
            path.display()
        ));
    }
    Ok(path.canonicalize().unwrap_or(path))
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn tool_search_entry_from_function_spec(
    spec: &serde_json::Value,
    kind: &str,
    source: &str,
    path: Option<String>,
) -> Option<ToolSearchEntry> {
    let function = spec.get("function")?;
    let name = function.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let description = function
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    Some(ToolSearchEntry {
        kind: kind.to_string(),
        name: name.to_string(),
        description,
        source: source.to_string(),
        path,
        spec: Some(spec.clone()),
        metadata: BTreeMap::new(),
        usage: Some("Call this function tool directly by name.".to_string()),
    })
}

/// Prefer short relative skill paths in tool_search results.
fn skill_search_path(workspace_config_dir: &Path, skill_md: &Path) -> String {
    let workspace_root = workspace_config_dir
        .parent()
        .unwrap_or(workspace_config_dir);
    skill_md
        .strip_prefix(workspace_root)
        .or_else(|_| skill_md.strip_prefix(workspace_config_dir))
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| skill_md.to_string_lossy().replace('\\', "/"))
}

fn local_skill_search_entries(workspace_config_dir: &Path) -> Vec<ToolSearchEntry> {
    let skills_dir = workspace_config_dir.join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return Vec::new();
    };

    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let id = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string());
        let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let (name, description, tags) = parse_tool_search_skill_frontmatter(&content);
        let display_name = if name.is_empty() { id } else { name };
        let mut description_parts = Vec::new();
        if !description.is_empty() {
            description_parts.push(description);
        }
        if !tags.is_empty() {
            description_parts.push(format!("tags: {}", tags.join(", ")));
        }
        skills.push(ToolSearchEntry {
            kind: "skill".to_string(),
            name: display_name,
            description: description_parts.join(" | "),
            source: "codey/skills".to_string(),
            path: Some(skill_search_path(workspace_config_dir, &skill_md)),
            spec: None,
            metadata: BTreeMap::new(),
            usage: Some("Read this skill's SKILL.md before applying it.".to_string()),
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

fn plugin_skill_search_entries(workspace_config_dir: &Path) -> Vec<ToolSearchEntry> {
    let mut entries = plugin_loader::list_plugin_skill_prompt_entries(workspace_config_dir)
        .into_iter()
        .map(|skill| ToolSearchEntry {
            kind: "skill".to_string(),
            name: format!("{}: {}", skill.plugin_display_name, skill.skill_name),
            description: skill.description,
            source: format!("plugin:{}", skill.plugin_id),
            path: Some(skill_search_path(workspace_config_dir, &skill.path)),
            spec: None,
            metadata: BTreeMap::new(),
            usage: Some("Read this plugin skill's SKILL.md before applying it.".to_string()),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

fn plugin_app_search_entries(workspace_config_dir: &Path) -> Vec<ToolSearchEntry> {
    let mut entries = plugin_loader::list_plugin_app_prompt_entries(workspace_config_dir)
        .into_iter()
        .map(|app| {
            let mut metadata = BTreeMap::new();
            metadata.insert("pluginId".to_string(), app.plugin_id.clone());
            metadata.insert(
                "pluginDisplayName".to_string(),
                app.plugin_display_name.clone(),
            );
            metadata.insert("appKey".to_string(), app.app_key.clone());
            metadata.insert("connectorId".to_string(), app.connector_id.clone());

            ToolSearchEntry {
                kind: "app".to_string(),
                name: format!("{}: {}", app.plugin_display_name, app.app_key),
                description: format!(
                    "Plugin app connector `{}` with connector id `{}`.",
                    app.app_key, app.connector_id
                ),
                source: format!("plugin:{}", app.plugin_id),
                path: None,
                spec: None,
                metadata,
                usage: Some(
                    "Use this app connector only through matching MCP tools/resources when they are exposed; do not invent app actions or data."
                        .to_string(),
                ),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

fn resolve_plugin_cache_source_dir(raw_source_dir: Option<&str>) -> Result<PathBuf, String> {
    if let Some(source_dir) = raw_source_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(source_dir);
        if path.is_dir() {
            return Ok(path);
        }
        return Err(format!("Codex plugin cache not found: {}", path.display()));
    }

    let path = plugin_commands::default_codex_plugin_cache_dir()
        .ok_or_else(|| "Could not locate the Codex plugin cache".to_string())?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("Codex plugin cache not found: {}", path.display()))
    }
}

fn plugin_install_candidates(
    source_dir: &Path,
    workspace_config_dir: &Path,
    query: Option<&str>,
    include_installed: bool,
    limit: usize,
) -> Vec<PluginInstallCandidate> {
    let query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let mut candidates = plugin_commands::discover_plugin_roots(source_dir)
        .into_iter()
        .filter_map(|root| {
            plugin_install_candidate_from_root(source_dir, workspace_config_dir, &root)
        })
        .filter(|candidate| include_installed || !candidate.installed)
        .filter(|candidate| {
            query
                .as_deref()
                .is_none_or(|query| plugin_install_candidate_matches(candidate, query))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
            .then(left.id.cmp(&right.id))
    });
    candidates.truncate(limit);
    candidates
}

fn plugin_install_candidate_from_root(
    source_dir: &Path,
    workspace_config_dir: &Path,
    root: &Path,
) -> Option<PluginInstallCandidate> {
    let manifest_path = root.join(".codex-plugin").join("plugin.json");
    let manifest = read_json_file(&manifest_path)?;
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let description = manifest
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let destination = workspace_config_dir
        .join("plugins")
        .join(plugin_commands::sanitize_plugin_destination_name(&name));
    let id = plugin_candidate_id(source_dir, root, &name, version.as_deref());
    let mcp_server_names = plugin_candidate_mcp_server_names(root, &manifest);
    let app_connector_ids = plugin_candidate_app_connector_ids(root, &manifest);
    let has_skills = plugin_candidate_has_skills(root, &manifest);

    Some(PluginInstallCandidate {
        id,
        name,
        version,
        description,
        source: root.to_string_lossy().to_string(),
        destination: destination.to_string_lossy().to_string(),
        installed: destination.is_dir(),
        has_skills,
        mcp_server_names,
        app_connector_ids,
    })
}

fn plugin_candidate_id(
    source_dir: &Path,
    root: &Path,
    name: &str,
    version: Option<&str>,
) -> String {
    if let Ok(relative) = root.strip_prefix(source_dir)
        && let Some(relative) = relative.to_str()
    {
        let relative = relative.replace('\\', "/");
        if !relative.is_empty() {
            return format!("local-cache:{relative}");
        }
    }
    match version {
        Some(version) => format!("local-cache:{}@{}", name, version),
        None => format!("local-cache:{name}"),
    }
}

fn plugin_install_candidate_matches(candidate: &PluginInstallCandidate, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {} {} {}",
        candidate.id,
        candidate.name,
        candidate.version.as_deref().unwrap_or_default(),
        candidate.description.as_deref().unwrap_or_default(),
        candidate.source,
        candidate.destination,
        candidate.mcp_server_names.join(" "),
        candidate.app_connector_ids.join(" "),
    )
    .to_ascii_lowercase();
    haystack.contains(query)
}

fn select_plugin_install_candidate(
    candidates: &[PluginInstallCandidate],
    tool_id: Option<&str>,
    name: Option<&str>,
) -> Result<PluginInstallCandidate, String> {
    if let Some(tool_id) = tool_id.map(str::trim).filter(|value| !value.is_empty()) {
        return candidates
            .iter()
            .find(|candidate| candidate.id == tool_id || candidate.source == tool_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "tool_id must match one of the candidates returned by list_available_plugins_to_install: {tool_id}"
                )
            });
    }

    let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(
            "request_plugin_install requires tool_id or name. Call list_available_plugins_to_install first."
                .to_string(),
        );
    };
    let name_lower = name.to_ascii_lowercase();
    let mut matches = candidates
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches = candidates
            .iter()
            .filter(|candidate| candidate.name.to_ascii_lowercase().contains(&name_lower))
            .cloned()
            .collect::<Vec<_>>();
    }
    match matches.len() {
        0 => Err(format!("No plugin candidate matched name: {name}")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "Plugin name matched multiple candidates. Use tool_id from list_available_plugins_to_install: {}",
            matches
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn plugin_candidate_has_skills(root: &Path, manifest: &serde_json::Value) -> bool {
    if root.join("skills").is_dir() {
        return true;
    }
    manifest
        .get("skills")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| resolve_plugin_manifest_path(root, value).ok())
        .is_some_and(|path| path.is_dir())
}

fn plugin_candidate_mcp_server_names(root: &Path, manifest: &serde_json::Value) -> Vec<String> {
    let mut names = BTreeSet::new();
    if let Some(value) = manifest
        .get("mcpServers")
        .or_else(|| manifest.get("mcp_servers"))
    {
        collect_mcp_server_names(root, value, &mut names);
    } else {
        let default_path = root.join(".mcp.json");
        if let Some(value) = read_json_file(&default_path) {
            collect_mcp_server_names(root, &value, &mut names);
        }
    }
    names.into_iter().collect()
}

fn collect_mcp_server_names(root: &Path, value: &serde_json::Value, names: &mut BTreeSet<String>) {
    if let Some(path) = value.as_str() {
        if let Ok(path) = resolve_plugin_manifest_path(root, path)
            && let Some(value) = read_json_file(&path)
        {
            collect_mcp_server_names(root, &value, names);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        return;
    };
    let server_map = object
        .get("mcpServers")
        .or_else(|| object.get("mcp_servers"))
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    for name in server_map.keys() {
        names.insert(name.clone());
    }
}

fn plugin_candidate_app_connector_ids(root: &Path, manifest: &serde_json::Value) -> Vec<String> {
    let mut ids = BTreeSet::new();
    if let Some(value) = manifest.get("apps") {
        collect_app_connector_ids(root, value, &mut ids);
    } else {
        let default_path = root.join(".app.json");
        if let Some(value) = read_json_file(&default_path) {
            collect_app_connector_ids(root, &value, &mut ids);
        }
    }
    ids.into_iter().collect()
}

fn collect_app_connector_ids(root: &Path, value: &serde_json::Value, ids: &mut BTreeSet<String>) {
    if let Some(path) = value.as_str() {
        if let Ok(path) = resolve_plugin_manifest_path(root, path)
            && let Some(value) = read_json_file(&path)
        {
            collect_app_connector_ids(root, &value, ids);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        return;
    };
    let apps_map = object
        .get("apps")
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    for value in apps_map.values() {
        let connector_id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(connector_id) = connector_id {
            ids.insert(connector_id.to_string());
        }
    }
}

fn resolve_plugin_manifest_path(root: &Path, raw_path: &str) -> Result<PathBuf, String> {
    let Some(relative_path) = raw_path.trim().strip_prefix("./") else {
        return Err("plugin manifest path must start with ./".to_string());
    };
    if relative_path.is_empty() {
        return Err("plugin manifest path must not be empty".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("plugin manifest path must not contain '..'".to_string());
            }
            _ => return Err("plugin manifest path must stay inside plugin root".to_string()),
        }
    }
    Ok(root.join(normalized))
}

fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn format_apps_list_output(
    workspace_config_dir: &Path,
    mcp_tool_aliases: &HashMap<String, McpToolAlias>,
    connector_filter: Option<&str>,
    include_tools: bool,
) -> String {
    let apps = app_connector_list_entries(
        workspace_config_dir,
        mcp_tool_aliases,
        connector_filter,
        include_tools,
    );
    let accessible = apps.iter().filter(|app| app.accessible).count();
    let declared_by_plugins = apps
        .iter()
        .filter(|app| !app.plugin_apps.is_empty())
        .count();
    let mcp_tool_count: usize = apps.iter().map(|app| app.tools.len()).sum();
    serde_json::to_string_pretty(&serde_json::json!({
        "connectorId": connector_filter,
        "summary": {
            "total": apps.len(),
            "accessible": accessible,
            "declaredByPlugins": declared_by_plugins,
            "mcpTools": mcp_tool_count,
        },
        "apps": apps,
    }))
    .unwrap_or_default()
}

fn app_connector_list_entries(
    workspace_config_dir: &Path,
    mcp_tool_aliases: &HashMap<String, McpToolAlias>,
    connector_filter: Option<&str>,
    include_tools: bool,
) -> Vec<AppConnectorListEntry> {
    let mut entries = BTreeMap::<String, AppConnectorListEntry>::new();

    for app in plugin_loader::list_plugin_app_prompt_entries(workspace_config_dir) {
        if connector_filter.is_some_and(|filter| filter != app.connector_id) {
            continue;
        }
        let connector_id = app.connector_id.clone();
        let entry = entries
            .entry(connector_id.clone())
            .or_insert_with(|| AppConnectorListEntry {
                connector_id: connector_id.clone(),
                connector_name: None,
                accessible: false,
                source: "plugin".to_string(),
                install_url: app_connector_install_url(&app.app_key, &connector_id),
                plugin_apps: Vec::new(),
                tools: Vec::new(),
            });
        entry.plugin_apps.push(AppConnectorPluginSource {
            plugin_id: app.plugin_id,
            plugin_display_name: app.plugin_display_name,
            app_key: app.app_key,
        });
    }

    let mut aliases = mcp_tool_aliases.iter().collect::<Vec<_>>();
    aliases.sort_by(|left, right| left.0.cmp(right.0));
    for (alias, info) in aliases {
        let Some(connector_id) = info.connector.connector_id.as_deref() else {
            continue;
        };
        if connector_filter.is_some_and(|filter| filter != connector_id) {
            continue;
        }

        let entry =
            entries
                .entry(connector_id.to_string())
                .or_insert_with(|| AppConnectorListEntry {
                    connector_id: connector_id.to_string(),
                    connector_name: info.connector.connector_name.clone(),
                    accessible: false,
                    source: "mcp".to_string(),
                    install_url: app_connector_install_url(
                        info.connector
                            .connector_name
                            .as_deref()
                            .unwrap_or(connector_id),
                        connector_id,
                    ),
                    plugin_apps: Vec::new(),
                    tools: Vec::new(),
                });
        if entry.connector_name.is_none() {
            entry.connector_name = info.connector.connector_name.clone();
        }
        entry.accessible = true;
        if include_tools {
            entry.tools.push(AppConnectorToolEntry {
                name: alias.clone(),
                server: info.server.clone(),
                tool: info.tool.clone(),
                connector_name: info.connector.connector_name.clone(),
                namespace_description: info.connector.namespace_description.clone(),
            });
        }
    }

    let mut apps = entries.into_values().collect::<Vec<_>>();
    for app in &mut apps {
        app.plugin_apps.sort();
        app.plugin_apps.dedup();
        app.tools.sort();
        app.tools.dedup();
        app.source = match (app.plugin_apps.is_empty(), app.accessible) {
            (false, true) => "plugin+mcp".to_string(),
            (false, false) => "plugin".to_string(),
            (true, true) => "mcp".to_string(),
            (true, false) => "unknown".to_string(),
        };
    }
    apps.sort_by(|left, right| {
        left.accessible
            .cmp(&right.accessible)
            .reverse()
            .then(
                left.connector_name
                    .as_deref()
                    .unwrap_or("")
                    .cmp(right.connector_name.as_deref().unwrap_or("")),
            )
            .then(left.connector_id.cmp(&right.connector_id))
    });
    apps
}

fn app_connector_install_url(name: &str, connector_id: &str) -> String {
    let slug = connector_slug(name);
    format!("https://chatgpt.com/apps/{slug}/{connector_id}")
}

fn connector_slug(name: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        "app".to_string()
    } else {
        output
    }
}

fn parse_tool_search_skill_frontmatter(content: &str) -> (String, String, Vec<String>) {
    let mut name = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();

    if !content.starts_with("---") {
        return (name, description, tags);
    }
    let Some(end) = content[3..].find("---") else {
        return (name, description, tags);
    };

    let frontmatter = &content[3..3 + end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("tags:") {
            let val = val.trim();
            if val.starts_with('[') {
                tags = val
                    .trim_matches(|ch| ch == '[' || ch == ']')
                    .split(',')
                    .map(|tag| tag.trim().trim_matches('"').to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }
        }
    }

    (name, description, tags)
}

fn search_tool_entries(
    entries: Vec<ToolSearchEntry>,
    query: &str,
    limit: usize,
) -> Vec<ToolSearchEntry> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let show_all = matches!(
        query.to_ascii_lowercase().as_str(),
        "*" | "all" | "list all" | "tools"
    );

    if show_all {
        let mut entries = entries;
        entries.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.name.cmp(&right.name))
        });
        entries.truncate(limit);
        return entries;
    }

    let documents = entries
        .iter()
        .map(tool_search_document_text)
        .map(|text| tokenize_tool_search_query(&text))
        .collect::<Vec<_>>();
    let query_tokens = unique_tool_search_tokens(tokenize_tool_search_query(query));
    if query_tokens.is_empty() {
        return Vec::new();
    }
    let inverse_document_frequencies = tool_search_inverse_document_frequencies(&documents);
    let average_document_length = if documents.is_empty() {
        0.0
    } else {
        documents
            .iter()
            .map(|document| document.len() as f64)
            .sum::<f64>()
            / documents.len() as f64
    };

    let mut scored = entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let score = tool_search_score(
                query,
                &query_tokens,
                documents.get(index).map(Vec::as_slice).unwrap_or_default(),
                average_document_length,
                &inverse_document_frequencies,
                &entry,
            );
            (score > 0.0).then_some((score, entry))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });

    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}

fn tool_search_document_text(entry: &ToolSearchEntry) -> String {
    let mut parts = Vec::new();
    push_tool_search_part(&mut parts, &entry.kind);
    push_tool_search_part(&mut parts, &entry.name);
    push_tool_search_part(&mut parts, &entry.name.replace('_', " "));
    push_tool_search_part(&mut parts, &entry.description);
    push_tool_search_part(&mut parts, &entry.source);
    if let Some(path) = entry.path.as_deref() {
        push_tool_search_part(&mut parts, path);
    }
    if let Some(usage) = entry.usage.as_deref() {
        push_tool_search_part(&mut parts, usage);
    }
    for (key, value) in &entry.metadata {
        push_tool_search_part(&mut parts, key);
        push_tool_search_part(&mut parts, value);
    }
    if let Some(spec) = &entry.spec {
        append_tool_spec_search_text(spec, &mut parts);
    }
    parts.join(" ")
}

fn append_tool_spec_search_text(spec: &serde_json::Value, parts: &mut Vec<String>) {
    let Some(function) = spec.get("function") else {
        return;
    };
    if let Some(name) = function.get("name").and_then(serde_json::Value::as_str) {
        push_tool_search_part(parts, name);
        push_tool_search_part(parts, &name.replace('_', " "));
    }
    if let Some(description) = function
        .get("description")
        .and_then(serde_json::Value::as_str)
    {
        push_tool_search_part(parts, description);
    }
    if let Some(parameters) = function.get("parameters") {
        append_json_schema_search_text(parameters, parts);
    }
}

fn append_json_schema_search_text(schema: &serde_json::Value, parts: &mut Vec<String>) {
    let Some(object) = schema.as_object() else {
        return;
    };

    for key in ["title", "description", "$comment"] {
        if let Some(value) = object.get(key).and_then(serde_json::Value::as_str) {
            push_tool_search_part(parts, value);
        }
    }

    if let Some(properties) = object
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (name, property_schema) in properties {
            push_tool_search_part(parts, name);
            push_tool_search_part(parts, &name.replace('_', " "));
            append_json_schema_search_text(property_schema, parts);
        }
    }

    if let Some(required) = object.get("required").and_then(serde_json::Value::as_array) {
        for item in required {
            if let Some(name) = item.as_str() {
                push_tool_search_part(parts, name);
            }
        }
    }

    if let Some(enum_values) = object.get("enum").and_then(serde_json::Value::as_array) {
        for item in enum_values {
            if let Some(value) = item.as_str() {
                push_tool_search_part(parts, value);
            }
        }
    }

    for key in ["items", "additionalProperties"] {
        if let Some(value) = object.get(key) {
            append_json_schema_search_text(value, parts);
        }
    }

    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(variants) = object.get(key).and_then(serde_json::Value::as_array) {
            for variant in variants {
                append_json_schema_search_text(variant, parts);
            }
        }
    }
}

fn push_tool_search_part(parts: &mut Vec<String>, part: &str) {
    let part = part.trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }
}

fn tool_search_score(
    raw_query: &str,
    query_tokens: &[String],
    document_tokens: &[String],
    average_document_length: f64,
    inverse_document_frequencies: &HashMap<String, f64>,
    entry: &ToolSearchEntry,
) -> f64 {
    let mut score = tool_search_bm25_score(
        query_tokens,
        document_tokens,
        average_document_length,
        inverse_document_frequencies,
    );

    score += tool_search_exact_match_boost(raw_query, entry);
    score
}

fn tool_search_bm25_score(
    query_tokens: &[String],
    document_tokens: &[String],
    average_document_length: f64,
    inverse_document_frequencies: &HashMap<String, f64>,
) -> f64 {
    if document_tokens.is_empty() || average_document_length <= 0.0 {
        return 0.0;
    }

    let mut frequencies: HashMap<&str, usize> = HashMap::new();
    for token in document_tokens {
        *frequencies.entry(token.as_str()).or_insert(0) += 1;
    }

    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let document_length = document_tokens.len() as f64;
    let length_norm = K1 * (1.0 - B + B * document_length / average_document_length);

    query_tokens.iter().fold(0.0, |score, token| {
        let Some(term_frequency) = frequencies.get(token.as_str()).copied() else {
            return score;
        };
        let Some(idf) = inverse_document_frequencies.get(token) else {
            return score;
        };
        let term_frequency = term_frequency as f64;
        score + idf * (term_frequency * (K1 + 1.0)) / (term_frequency + length_norm)
    })
}

fn tool_search_inverse_document_frequencies(documents: &[Vec<String>]) -> HashMap<String, f64> {
    let document_count = documents.len() as f64;
    let mut document_frequencies: HashMap<String, usize> = HashMap::new();
    for document in documents {
        let mut seen = BTreeSet::new();
        for token in document {
            if seen.insert(token) {
                *document_frequencies.entry(token.clone()).or_insert(0) += 1;
            }
        }
    }

    document_frequencies
        .into_iter()
        .map(|(token, frequency)| {
            let frequency = frequency as f64;
            let idf = (1.0 + (document_count - frequency + 0.5) / (frequency + 0.5)).ln();
            (token, idf)
        })
        .collect()
}

fn tool_search_exact_match_boost(query: &str, entry: &ToolSearchEntry) -> f64 {
    let metadata_text = entry
        .metadata
        .iter()
        .map(|(key, value)| format!("{key} {value}"))
        .collect::<Vec<_>>()
        .join(" ");
    let haystack = format!(
        "{} {} {} {} {} {}",
        entry.kind,
        entry.name,
        entry.description,
        entry.source,
        entry.path.as_deref().unwrap_or_default(),
        metadata_text
    )
    .to_ascii_lowercase();
    let name = entry.name.to_ascii_lowercase();
    let query_lower = normalize_tool_search_phrase(query);
    let tokens = tokenize_tool_search_query(&query_lower);
    if tokens.is_empty() {
        return 0.0;
    }

    let mut score = 0.0;
    if name == query_lower {
        score += 8.0;
    }
    if name.contains(&query_lower) {
        score += 3.5;
    }
    if haystack.contains(&query_lower) {
        score += 2.0;
    }
    for token in tokens {
        if name.split(['_', '-', ' ', ':']).any(|part| part == token) {
            score += 1.6;
        } else if name.contains(&token) {
            score += 1.0;
        }
        if entry.description.to_ascii_lowercase().contains(&token) {
            score += 0.7;
        }
        if entry.source.to_ascii_lowercase().contains(&token) {
            score += 0.35;
        }
        if metadata_text.to_ascii_lowercase().contains(&token) {
            score += 0.45;
        }
        if entry
            .path
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(&token)
        {
            score += 0.25;
        }
    }
    score
}

fn tokenize_tool_search_query(query: &str) -> Vec<String> {
    expand_tool_search_identifiers(query)
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn unique_tool_search_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tokens
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .collect()
}

fn normalize_tool_search_phrase(value: &str) -> String {
    tokenize_tool_search_query(value).join(" ")
}

fn expand_tool_search_identifiers(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 8);
    let mut previous: Option<char> = None;
    for ch in value.chars() {
        if matches!(
            ch,
            '_' | '-' | ':' | '/' | '\\' | '.' | '(' | ')' | '[' | ']'
        ) {
            output.push(' ');
            previous = None;
            continue;
        }

        if let Some(prev) = previous {
            if (ch.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit()))
                || (ch.is_ascii_digit() && prev.is_ascii_alphabetic())
                || (ch.is_ascii_alphabetic() && prev.is_ascii_digit())
            {
                output.push(' ');
            }
        }
        output.push(ch);
        previous = Some(ch);
    }
    output
}

fn format_tool_search_output(query: &str, matches: Vec<ToolSearchEntry>) -> String {
    let mut output_matches = Vec::new();
    let mut loadable_tools = Vec::new();
    for entry in &matches {
        let mut item = BTreeMap::new();
        item.insert("type".to_string(), serde_json::json!(entry.kind.clone()));
        item.insert("name".to_string(), serde_json::json!(entry.name.clone()));
        item.insert(
            "description".to_string(),
            serde_json::json!(entry.description.clone()),
        );
        item.insert(
            "source".to_string(),
            serde_json::json!(entry.source.clone()),
        );
        if !entry.metadata.is_empty() {
            item.insert(
                "metadata".to_string(),
                serde_json::json!(entry.metadata.clone()),
            );
        }
        if let Some(usage) = entry.usage.clone() {
            item.insert("usage".to_string(), serde_json::json!(usage));
        }
        if let Some(path) = entry.path.clone() {
            item.insert("path".to_string(), serde_json::json!(path));
        }
        if let Some(spec) = entry.spec.clone() {
            item.insert("spec".to_string(), spec.clone());
            if let Some(tool) = tool_search_loadable_tool(entry, &spec) {
                loadable_tools.push(tool);
            }
        }
        output_matches.push(serde_json::Value::Object(item.into_iter().collect()));
    }

    serde_json::to_string_pretty(&serde_json::json!({
        "query": query,
        "matches": output_matches,
        "tools": coalesce_tool_search_loadable_tools(loadable_tools),
    }))
    .unwrap_or_default()
}

fn tool_search_loadable_tool(
    entry: &ToolSearchEntry,
    spec: &serde_json::Value,
) -> Option<serde_json::Value> {
    let function = spec.get("function")?;
    let full_name = function.get("name")?.as_str()?;
    let response_tool = response_tool_from_function_spec(spec)?;

    let Some((namespace, local_name)) = mcp_tool_namespace_and_name(full_name) else {
        return Some(response_tool);
    };
    let mut local_tool = response_tool;
    local_tool["name"] = serde_json::Value::String(local_name.to_string());
    local_tool["defer_loading"] = serde_json::Value::Bool(true);

    let description = entry
        .metadata
        .get("namespaceDescription")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("Tools in the {namespace} namespace."));

    Some(serde_json::json!({
        "type": "namespace",
        "name": namespace,
        "description": description,
        "tools": [local_tool],
    }))
}

fn response_tool_from_function_spec(spec: &serde_json::Value) -> Option<serde_json::Value> {
    let function = spec.get("function")?;
    let name = function.get("name")?.as_str()?;
    let mut response_tool = serde_json::json!({
        "type": "function",
        "name": name,
        "description": function
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        "strict": false,
        "parameters": function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    });
    if let Some(defer_loading) = spec
        .get("defer_loading")
        .and_then(serde_json::Value::as_bool)
    {
        response_tool["defer_loading"] = serde_json::Value::Bool(defer_loading);
    }
    Some(response_tool)
}

fn mcp_tool_namespace_and_name(full_name: &str) -> Option<(&str, &str)> {
    let rest = full_name.strip_prefix("mcp__")?;
    let split = rest.rfind("__")?;
    if split == 0 || split + 2 >= rest.len() {
        return None;
    }
    let namespace = &full_name[.."mcp__".len() + split];
    let local_name = &rest[split + 2..];
    Some((namespace, local_name))
}

fn coalesce_tool_search_loadable_tools(tools: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut output = Vec::<serde_json::Value>::new();
    for tool in tools {
        let is_namespace =
            tool.get("type").and_then(serde_json::Value::as_str) == Some("namespace");
        if !is_namespace {
            output.push(tool);
            continue;
        }

        let name = tool
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let incoming_tools = tool
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if let Some(existing) = output.iter_mut().find(|existing| {
            existing.get("type").and_then(serde_json::Value::as_str) == Some("namespace")
                && existing.get("name").and_then(serde_json::Value::as_str) == Some(name.as_str())
        }) {
            if let Some(existing_tools) = existing
                .get_mut("tools")
                .and_then(serde_json::Value::as_array_mut)
            {
                existing_tools.extend(incoming_tools);
            }
        } else {
            output.push(tool);
        }
    }
    output
}

fn mcp_direct_tool_spec(
    server_name: &str,
    tool_name: &str,
    tool: &serde_json::Value,
    alias: &str,
) -> serde_json::Value {
    let connector = mcp_connector_metadata(server_name, tool);
    let raw_description = tool
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut description_parts = Vec::new();
    if let Some(connector_name) = connector.connector_name.as_deref() {
        let connector_label = connector
            .connector_id
            .as_deref()
            .map(|connector_id| format!("{connector_name} ({connector_id})"))
            .unwrap_or_else(|| connector_name.to_string());
        description_parts.push(format!(
            "MCP app connector {connector_label} tool {server_name}:{tool_name}."
        ));
    } else {
        description_parts.push(format!("MCP tool {server_name}:{tool_name}."));
    }
    if let Some(namespace_description) = connector.namespace_description.as_deref() {
        description_parts.push(namespace_description.to_string());
    }
    if let Some(description) = raw_description {
        description_parts.push(description.to_string());
    }
    let description = description_parts.join(" ");
    let parameters = tool
        .get("inputSchema")
        .or_else(|| tool.get("input_schema"))
        .cloned()
        .filter(|value| value.is_object())
        .unwrap_or_else(default_mcp_input_schema);

    serde_json::json!({
        "type": "function",
        "function": {
            "name": alias,
            "description": description,
            "parameters": parameters
        }
    })
}

fn sorted_mcp_tool_specs(specs: &HashMap<String, serde_json::Value>) -> Vec<serde_json::Value> {
    let mut aliases = specs.keys().cloned().collect::<Vec<_>>();
    aliases.sort();
    aliases
        .into_iter()
        .filter_map(|alias| specs.get(&alias).cloned())
        .collect()
}

fn mcp_connector_metadata(server_name: &str, tool: &serde_json::Value) -> McpConnectorMetadata {
    if !trusted_codex_apps_server_name(server_name) {
        return McpConnectorMetadata::default();
    }

    McpConnectorMetadata {
        connector_id: mcp_tool_metadata_string(tool, &["connector_id", "connectorId"]),
        connector_name: mcp_tool_metadata_string(
            tool,
            &[
                "connector_name",
                "connectorName",
                "connector_display_name",
                "connectorDisplayName",
            ],
        ),
        namespace_description: mcp_tool_metadata_string(
            tool,
            &["connector_description", "connectorDescription"],
        ),
    }
}

fn trusted_codex_apps_server_name(server_name: &str) -> bool {
    matches!(server_name.trim(), "codex-apps" | "codex_apps")
}

fn mcp_tool_metadata_string(tool: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = tool.get(*key).and_then(serde_json::Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    for meta_key in ["_meta", "meta"] {
        let Some(meta) = tool.get(meta_key).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for key in keys {
            if let Some(value) = meta.get(*key).and_then(serde_json::Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

fn default_mcp_input_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

fn mcp_direct_tool_name(
    server_name: &str,
    tool_name: &str,
    used_names: &mut BTreeSet<String>,
) -> String {
    let server_part = sanitize_tool_name_part(server_name, "server");
    let tool_part = sanitize_tool_name_part(tool_name, "tool");
    let base = format!("mcp__{server_part}__{tool_part}");
    let hash = stable_hash_hex(&format!("{server_name}\0{tool_name}"));
    let mut candidate = truncate_mcp_tool_name(&base, &hash);

    let mut counter = 2usize;
    while used_names.contains(&candidate) {
        let suffix = format!("_{counter}");
        let max_len = 64usize.saturating_sub(suffix.len());
        let prefix = candidate.chars().take(max_len).collect::<String>();
        candidate = format!("{prefix}{suffix}");
        counter += 1;
    }

    used_names.insert(candidate.clone());
    candidate
}

fn truncate_mcp_tool_name(base: &str, hash: &str) -> String {
    const MAX_TOOL_NAME_LEN: usize = 64;
    if base.len() <= MAX_TOOL_NAME_LEN {
        return base.to_string();
    }
    let suffix = format!("__{}", &hash[..8]);
    let max_prefix = MAX_TOOL_NAME_LEN.saturating_sub(suffix.len());
    let prefix = base.chars().take(max_prefix).collect::<String>();
    format!("{prefix}{suffix}")
}

fn sanitize_tool_name_part(value: &str, fallback: &str) -> String {
    let mut output = String::new();
    let mut last_was_underscore = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() { ch } else { '_' };
        if next == '_' {
            if last_was_underscore {
                continue;
            }
            last_was_underscore = true;
        } else {
            last_was_underscore = false;
        }
        output.push(next);
    }
    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn combine_stdout_stderr(stdout: &str, stderr: &str) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.trim().to_string(),
        (true, false) => stderr.trim().to_string(),
        (false, false) => format!("{}\n[stderr]\n{}", stdout.trim(), stderr.trim()),
    }
}

fn image_generate_display(args: &ImageGenerateArgs) -> String {
    let prompt = condense_whitespace(&args.prompt);
    if prompt.is_empty() {
        return "image_generate".to_string();
    }
    if prompt.chars().count() > 64 {
        format!("{}...", prompt.chars().take(64).collect::<String>())
    } else {
        prompt
    }
}

fn echarts_report_display(args: &EchartsReportArgs) -> String {
    if let Some(title) = args
        .title
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty())
    {
        if title.chars().count() > 64 {
            return format!("{}...", title.chars().take(64).collect::<String>());
        }
        return title;
    }

    if let Some(chart_type) = args
        .chart_type
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty())
    {
        return chart_type;
    }

    "echarts_report".to_string()
}

fn image_generation_api_key(settings_api_key: Option<&str>) -> Option<String> {
    settings_api_key
        .and_then(|value| non_empty_string(value.to_string()))
        .or_else(|| {
            std::env::var("CN_CODEX_IMAGE_API_KEY")
                .ok()
                .and_then(non_empty_string)
        })
        .or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .and_then(non_empty_string)
        })
}

fn image_generation_model(model: Option<&str>, settings_model: Option<&str>) -> String {
    model
        .and_then(|value| non_empty_string(value.to_string()))
        .or_else(|| settings_model.and_then(|value| non_empty_string(value.to_string())))
        .or_else(|| {
            std::env::var("CN_CODEX_IMAGE_MODEL")
                .ok()
                .and_then(non_empty_string)
        })
        .unwrap_or_else(|| "gpt-image-2".to_string())
}

fn image_generation_api_url(base_url: Option<&str>, settings_base_url: Option<&str>) -> String {
    let base = base_url
        .and_then(|value| non_empty_string(value.to_string()))
        .or_else(|| settings_base_url.and_then(|value| non_empty_string(value.to_string())))
        .or_else(|| {
            std::env::var("CN_CODEX_IMAGE_BASE_URL")
                .ok()
                .and_then(non_empty_string)
        })
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/images/generations") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/images/generations")
    }
}

fn image_generation_request_body(
    prompt: &str,
    model: &str,
    size: Option<&str>,
    quality: Option<&str>,
    background: Option<&str>,
    n: Option<u32>,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert(
        "model".to_string(),
        serde_json::Value::String(model.to_string()),
    );
    body.insert(
        "prompt".to_string(),
        serde_json::Value::String(prompt.to_string()),
    );
    body.insert(
        "size".to_string(),
        serde_json::Value::String(
            size.and_then(|value| non_empty_string(value.to_string()))
                .unwrap_or_else(|| "1024x1024".to_string()),
        ),
    );
    if let Some(value) = quality.and_then(|value| non_empty_string(value.to_string())) {
        body.insert("quality".to_string(), serde_json::Value::String(value));
    }
    if let Some(value) = background.and_then(|value| non_empty_string(value.to_string())) {
        body.insert("background".to_string(), serde_json::Value::String(value));
    }
    body.insert(
        "n".to_string(),
        serde_json::Value::Number(serde_json::Number::from(image_generation_count(n))),
    );
    body.insert(
        "response_format".to_string(),
        serde_json::Value::String("b64_json".to_string()),
    );
    serde_json::Value::Object(body)
}

fn image_generation_count(n: Option<u32>) -> u32 {
    n.unwrap_or(1).clamp(1, 10)
}

fn decode_image_base64(input: &str) -> Result<Vec<u8>, String> {
    let payload = input
        .trim()
        .split_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or_else(|| input.trim());
    let compact: String = payload.chars().filter(|ch| !ch.is_whitespace()).collect();
    general_purpose::STANDARD
        .decode(compact)
        .map_err(|e| format!("image_generate returned invalid base64 image data: {e}"))
}

fn resolve_image_generate_output_path(
    root: &Path,
    workspace_config_dir: &Path,
    input: Option<&str>,
    call_id: &str,
    prompt: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    let path = if let Some(input) = input.map(str::trim).filter(|value| !value.is_empty()) {
        let raw = PathBuf::from(input);
        if raw.is_absolute() {
            raw
        } else {
            let mut resolved = root.to_path_buf();
            for component in raw.components() {
                match component {
                    Component::Normal(part) => resolved.push(part),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        return Err(
                            "Error: relative image output paths must not contain '..'".to_string()
                        );
                    }
                    _ => return Err("Error: invalid image output path component".to_string()),
                }
            }
            resolved
        }
    } else {
        let safe_call_id = sanitize_tool_name_part(call_id, "image");
        let prompt_hash = stable_hash_hex(prompt);
        workspace_config_dir
            .join("images")
            .join("generated")
            .join(format!(
                "{}-{}.{}",
                safe_call_id,
                &prompt_hash[..8],
                extension
            ))
    };

    Ok(ensure_image_output_extension(path, extension))
}

#[allow(clippy::too_many_arguments)]
fn resolve_image_generate_output_path_for_index(
    root: &Path,
    workspace_config_dir: &Path,
    input: Option<&str>,
    call_id: &str,
    prompt: &str,
    extension: &str,
    index: usize,
    count: usize,
) -> Result<PathBuf, String> {
    let path = resolve_image_generate_output_path(
        root,
        workspace_config_dir,
        input,
        call_id,
        prompt,
        extension,
    )?;
    if count <= 1 {
        return Ok(path);
    }
    Ok(add_image_output_index_suffix(path, index + 1, extension))
}

fn add_image_output_index_suffix(mut path: PathBuf, index: usize, extension: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("image");
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(extension);
    path.set_file_name(format!("{stem}-{index}.{ext}"));
    path
}

fn ensure_image_output_extension(mut path: PathBuf, extension: &str) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension(extension);
    }
    path
}

fn image_extension_for_info(info: &ImageInfo) -> &'static str {
    match info.format {
        "JPEG" => "jpg",
        "GIF" => "gif",
        "WebP" => "webp",
        _ => "png",
    }
}

fn workspace_relative_display_path(root: &Path, path: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    path.strip_prefix(root)
        .ok()
        .map(|value| value.to_string_lossy().replace('\\', "/"))
}

fn format_image_generation_http_error(status: u16, body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<ImageGenerationErrorResponse>(body) {
        if let Some(error) = parsed.error {
            if let Some(message) = error.message.map(|value| value.trim().to_string()) {
                if !message.is_empty() {
                    if let Some(code) = error.code {
                        return format!(
                            "image_generate request failed with HTTP {status}: {message} (code: {code})"
                        );
                    }
                    return format!("image_generate request failed with HTTP {status}: {message}");
                }
            }
        }
    }

    let body = truncate_output(body.trim(), 1000);
    if body.is_empty() {
        format!("image_generate request failed with HTTP {status}")
    } else {
        format!("image_generate request failed with HTTP {status}: {body}")
    }
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageInfo {
    format: &'static str,
    mime: &'static str,
    width: u32,
    height: u32,
}

fn resolve_view_image_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Error: image path must not be empty".to_string());
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Ok(path);
    }

    let mut resolved = root.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("Error: relative image paths must not contain '..'".to_string());
            }
            _ => return Err("Error: invalid image path component".to_string()),
        }
    }
    Ok(resolved)
}

fn inspect_image_bytes(bytes: &[u8]) -> Result<ImageInfo, String> {
    inspect_png(bytes)
        .or_else(|| inspect_gif(bytes))
        .or_else(|| inspect_jpeg(bytes))
        .or_else(|| inspect_webp(bytes))
        .ok_or_else(|| {
            "unsupported or invalid image format; supported formats are PNG, JPEG, GIF, and WebP"
                .to_string()
        })
}

fn inspect_png(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 24 {
        return None;
    }
    if &bytes[0..8] != b"\x89PNG\r\n\x1A\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }

    Some(ImageInfo {
        format: "PNG",
        mime: "image/png",
        width: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        height: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    })
}

fn inspect_gif(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 10 {
        return None;
    }
    if &bytes[0..6] != b"GIF87a" && &bytes[0..6] != b"GIF89a" {
        return None;
    }

    Some(ImageInfo {
        format: "GIF",
        mime: "image/gif",
        width: u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
        height: u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
    })
}

fn inspect_jpeg(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }

    let mut i = 2usize;
    while i + 1 < bytes.len() {
        while i < bytes.len() && bytes[i] != 0xFF {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let marker = bytes[i];
        i += 1;

        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 2 > bytes.len() {
            break;
        }

        let segment_len = u16::from_be_bytes(bytes[i..i + 2].try_into().ok()?) as usize;
        if segment_len < 2 {
            break;
        }
        let data_start = i + 2;
        let data_end = i + segment_len;
        if data_end > bytes.len() {
            break;
        }

        if is_jpeg_sof_marker(marker) && data_start + 5 <= data_end {
            return Some(ImageInfo {
                format: "JPEG",
                mime: "image/jpeg",
                height: u16::from_be_bytes(bytes[data_start + 1..data_start + 3].try_into().ok()?)
                    as u32,
                width: u16::from_be_bytes(bytes[data_start + 3..data_start + 5].try_into().ok()?)
                    as u32,
            });
        }

        i = data_end;
    }

    None
}

fn is_jpeg_sof_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    )
}

fn inspect_webp(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 20 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }

    let mut i = 12usize;
    while i + 8 <= bytes.len() {
        let fourcc = &bytes[i..i + 4];
        let chunk_size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        let payload = i + 8;
        let end = payload.checked_add(chunk_size)?;
        if end > bytes.len() {
            break;
        }

        if fourcc == b"VP8X" && chunk_size >= 10 {
            return Some(ImageInfo {
                format: "WebP",
                mime: "image/webp",
                width: read_u24_le(&bytes[payload + 4..payload + 7])? + 1,
                height: read_u24_le(&bytes[payload + 7..payload + 10])? + 1,
            });
        }

        if fourcc == b"VP8 "
            && chunk_size >= 10
            && &bytes[payload + 3..payload + 6] == b"\x9D\x01\x2A"
        {
            return Some(ImageInfo {
                format: "WebP",
                mime: "image/webp",
                width: (u16::from_le_bytes(bytes[payload + 6..payload + 8].try_into().ok()?)
                    & 0x3FFF) as u32,
                height: (u16::from_le_bytes(bytes[payload + 8..payload + 10].try_into().ok()?)
                    & 0x3FFF) as u32,
            });
        }

        if fourcc == b"VP8L" && chunk_size >= 5 && bytes[payload] == 0x2F {
            let b1 = bytes[payload + 1] as u32;
            let b2 = bytes[payload + 2] as u32;
            let b3 = bytes[payload + 3] as u32;
            let b4 = bytes[payload + 4] as u32;
            return Some(ImageInfo {
                format: "WebP",
                mime: "image/webp",
                width: 1 + b1 + ((b2 & 0x3F) << 8),
                height: 1 + ((b2 >> 6) | (b3 << 2) | ((b4 & 0x0F) << 10)),
            });
        }

        i = end + (chunk_size % 2);
    }

    None
}

fn read_u24_le(bytes: &[u8]) -> Option<u32> {
    if bytes.len() != 3 {
        return None;
    }
    Some((bytes[0] as u32) | ((bytes[1] as u32) << 8) | ((bytes[2] as u32) << 16))
}

async fn write_mcp_message(
    stdin: &mut tokio::process::ChildStdin,
    message: serde_json::Value,
) -> Result<(), String> {
    let mut line =
        serde_json::to_vec(&message).map_err(|e| format!("Failed to encode MCP message: {e}"))?;
    line.push(b'\n');
    stdin
        .write_all(&line)
        .await
        .map_err(|e| format!("Failed to write MCP message: {e}"))?;
    stdin
        .flush()
        .await
        .map_err(|e| format!("Failed to flush MCP message: {e}"))
}

async fn mcp_transport_error(
    server_name: &str,
    stderr: &Arc<Mutex<String>>,
    message: String,
) -> McpRequestError {
    let stderr_text = stderr.lock().await.clone();
    let message = if stderr_text.trim().is_empty() {
        message
    } else {
        format!(
            "{message}\n[stderr]\n{}",
            truncate_output(&stderr_text, 2000)
        )
    };
    McpRequestError::Transport(format!(
        "MCP server '{server_name}' transport failed: {message}"
    ))
}

fn collect_mcp_stderr(stderr: Option<tokio::process::ChildStderr>, buffer: Arc<Mutex<String>>) {
    tokio::spawn(async move {
        let Some(stderr) = stderr else {
            return;
        };
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            let Ok(bytes) = reader.read_line(&mut line).await else {
                break;
            };
            if bytes == 0 {
                break;
            }
            let mut text = buffer.lock().await;
            text.push_str(&line);
            if text.len() > 8_000 {
                let keep_from = text.len().saturating_sub(8_000);
                let tail = text[keep_from..].to_string();
                *text = tail;
            }
        }
    });
}

async fn close_mcp_session(session: Arc<Mutex<McpSession>>) {
    let mut session = session.lock().await;
    let _ = session.child.kill().await;
    let _ = session.child.wait().await;
}

fn clear_mcp_sessions_async(sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSession>>>>>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        if let Ok(mut sessions) = sessions.try_lock() {
            sessions.clear();
        }
        return;
    };
    handle.spawn(async move {
        let sessions_to_close = {
            let mut sessions = sessions.lock().await;
            sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions_to_close {
            close_mcp_session(session).await;
        }
    });
}

fn clear_mcp_http_sessions_async(
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpHttpSession>>>>>,
) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        if let Ok(mut sessions) = sessions.try_lock() {
            sessions.clear();
        }
        return;
    };
    handle.spawn(async move {
        sessions.lock().await.clear();
    });
}

fn parse_mcp_http_response_body(
    body: &str,
    content_type: &str,
) -> Result<Option<serde_json::Value>, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
        || trimmed
            .lines()
            .any(|line| line.trim_start().starts_with("data:"))
    {
        return parse_mcp_sse_response_body(trimmed).map(Some);
    }

    serde_json::from_str::<serde_json::Value>(trimmed)
        .map(Some)
        .map_err(|error| format!("invalid JSON response: {error}"))
}

fn parse_mcp_sse_response_body(body: &str) -> Result<serde_json::Value, String> {
    let mut data_lines = Vec::new();
    for line in body.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if let Some(value) = parse_mcp_sse_data_lines(&data_lines)? {
                return Ok(value);
            }
            data_lines.clear();
            continue;
        }
        if let Some(data) = line.trim_start().strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }

    if let Some(value) = parse_mcp_sse_data_lines(&data_lines)? {
        return Ok(value);
    }
    Err("SSE response did not contain a JSON data event".to_string())
}

fn parse_mcp_sse_data_lines(lines: &[String]) -> Result<Option<serde_json::Value>, String> {
    if lines.is_empty() {
        return Ok(None);
    }
    let data = lines.join("\n");
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .map(Some)
        .map_err(|error| format!("invalid SSE JSON data: {error}"))
}

async fn read_mcp_response<R>(
    reader: &mut BufReader<R>,
    expected_id: i64,
) -> Result<serde_json::Value, String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Failed to read MCP response: {e}"))?;
        if bytes == 0 {
            return Err(format!(
                "MCP server closed stdout before response id {expected_id}"
            ));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };

        if value.get("id").and_then(serde_json::Value::as_i64) == Some(expected_id) {
            return Ok(value);
        }
    }
}

fn format_mcp_selection_result(
    results: Vec<(String, Result<serde_json::Value, String>)>,
    field: &str,
) -> (i32, String) {
    let mut has_error = false;
    let servers: Vec<_> = results
        .into_iter()
        .map(|(server, result)| match result {
            Ok(value) => serde_json::json!({
                "server": server,
                field: value,
            }),
            Err(error) => {
                has_error = true;
                serde_json::json!({
                    "server": server,
                    "error": error,
                })
            }
        })
        .collect();

    let text = serde_json::to_string_pretty(&serde_json::json!({ "servers": servers }))
        .unwrap_or_default();
    (if has_error { -1 } else { 0 }, text)
}

fn base_mcp_status_entry(server: &McpServerConfig) -> serde_json::Value {
    let mut env_keys = server.env.keys().cloned().collect::<Vec<_>>();
    env_keys.sort();
    let mut header_keys = server.headers.keys().cloned().collect::<Vec<_>>();
    header_keys.sort();
    serde_json::json!({
        "name": server.name,
        "transport": server.transport,
        "command": server.command,
        "args": server.args,
        "cwd": server.cwd,
        "url": server.url,
        "disabled": server.disabled,
        "envKeys": env_keys,
        "headerKeys": header_keys,
        "status": "configured",
    })
}

fn mcp_result_array_len(value: &serde_json::Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn format_mcp_single_result(result: Result<serde_json::Value, String>) -> (i32, String) {
    match result {
        Ok(value) => (0, format_json_value(&value)),
        Err(error) => (-1, error),
    }
}

fn format_plan_update(explanation: Option<&str>, plan: &[PlanItemArg]) -> Result<String, String> {
    if plan.is_empty() {
        return Err("Error: update_plan requires at least one plan item".to_string());
    }

    let mut in_progress = 0usize;
    for item in plan {
        let step = item.step.trim();
        if step.is_empty() {
            return Err("Error: plan item step must not be empty".to_string());
        }
        match item.status.as_str() {
            "pending" | "in_progress" | "completed" => {}
            other => {
                return Err(format!(
                    "Error: invalid plan status '{other}'. Expected pending, in_progress, or completed"
                ));
            }
        }
        if item.status == "in_progress" {
            in_progress += 1;
        }
    }

    if in_progress > 1 {
        return Err("Error: only one plan item can be in_progress".to_string());
    }

    let mut output = String::from("Plan updated");
    if let Some(explanation) = explanation.map(str::trim).filter(|value| !value.is_empty()) {
        output.push_str(": ");
        output.push_str(explanation);
    }
    output.push('\n');

    for item in plan {
        output.push_str(&format!("- [{}] {}\n", item.status, item.step.trim()));
    }

    Ok(output.trim_end().to_string())
}

fn format_echarts_report(args: &EchartsReportArgs) -> Result<String, String> {
    if !args.option.is_object() {
        return Err("Error: echarts_report option must be a JSON object".to_string());
    }

    let title = args
        .title
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty());
    let chart_type = args
        .chart_type
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty());
    let notes = args
        .notes
        .as_deref()
        .map(condense_whitespace)
        .filter(|value| !value.is_empty());
    let option_json = serde_json::to_string_pretty(&args.option)
        .map_err(|e| format!("Error: echarts_report option serialization failed: {e}"))?;

    let mut output = String::from("ECharts report ready");
    if let Some(title) = title.as_deref() {
        output.push_str(": ");
        output.push_str(title);
    }
    if let Some(chart_type) = chart_type.as_deref() {
        output.push_str("\nChart type: ");
        output.push_str(chart_type);
    }
    if let Some(notes) = notes.as_deref() {
        output.push_str("\nNotes: ");
        output.push_str(notes);
    }

    output.push_str("\nOption JSON:\n");
    output.push_str(&option_json);
    output.push_str("\n\n```echarts\n");
    output.push_str(&option_json);
    output.push_str("\n```");
    Ok(output)
}

fn format_json_value(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn option_is_recommended(option: &RequestUserInputQuestionOption) -> bool {
    let label_lower = option.label.to_ascii_lowercase();
    let desc_lower = option.description.to_ascii_lowercase();
    label_lower.contains("recommended")
        || desc_lower.contains("recommended")
        || option.label.contains("推荐")
        || option.description.contains("推荐")
}

fn normalize_request_user_input_args(args: &mut RequestUserInputArgs) {
    for question in &mut args.questions {
        if question.options.len() <= 1 {
            continue;
        }
        if let Some(index) = question.options.iter().position(option_is_recommended) {
            if index > 0 {
                let option = question.options.remove(index);
                question.options.insert(0, option);
            }
        }
    }
}

fn validate_request_user_input_args(args: &RequestUserInputArgs) -> Result<(), String> {
    if args.questions.is_empty() {
        return Err("Error: request_user_input requires at least one question".to_string());
    }
    if args.questions.len() > 3 {
        return Err("Error: request_user_input supports at most three questions".to_string());
    }

    for question in &args.questions {
        if question.id.trim().is_empty() {
            return Err("Error: request_user_input question id must not be empty".to_string());
        }
        if question.header.trim().is_empty() {
            return Err(format!(
                "Error: request_user_input question '{}' header must not be empty",
                question.id
            ));
        }
        if question.question.trim().is_empty() {
            return Err(format!(
                "Error: request_user_input question '{}' prompt must not be empty",
                question.id
            ));
        }
        if question.options.len() > 3 {
            return Err(format!(
                "Error: request_user_input question '{}' supports at most three options",
                question.id
            ));
        }
        for option in &question.options {
            if option.label.trim().is_empty() {
                return Err(format!(
                    "Error: request_user_input question '{}' option label must not be empty",
                    question.id
                ));
            }
            if option.description.trim().is_empty() {
                return Err(format!(
                    "Error: request_user_input question '{}' option description must not be empty",
                    question.id
                ));
            }
        }
    }

    Ok(())
}

fn validate_request_permissions_args(args: &RequestPermissionsArgs) -> Result<(), String> {
    let Some(object) = args.permissions.as_object() else {
        return Err("Error: request_permissions permissions must be an object".to_string());
    };

    if object.is_empty() {
        return Err("Error: request_permissions requires at least one permission".to_string());
    }

    let has_known_permission = object.get("network").is_some_and(|value| !value.is_null())
        || object
            .get("file_system")
            .or_else(|| object.get("fileSystem"))
            .is_some_and(|value| !value.is_null());
    if !has_known_permission {
        return Err(
            "Error: request_permissions requires network or file_system permissions".to_string(),
        );
    }

    Ok(())
}

fn granted_permissions_from_result(result: &serde_json::Value) -> Option<serde_json::Value> {
    let permissions = result.get("permissions")?;
    if !permissions.is_object()
        || permissions
            .as_object()
            .is_some_and(|object| object.is_empty())
    {
        return None;
    }
    Some(permissions.clone())
}

fn permission_profile_covers(granted: &serde_json::Value, requested: &serde_json::Value) -> bool {
    match (granted, requested) {
        (serde_json::Value::Object(granted), serde_json::Value::Object(requested)) => {
            requested.iter().all(|(key, requested_value)| {
                granted.get(key).is_some_and(|granted_value| {
                    permission_profile_covers(granted_value, requested_value)
                })
            })
        }
        (serde_json::Value::Array(granted), serde_json::Value::Array(requested)) => {
            requested.iter().all(|requested_value| {
                granted
                    .iter()
                    .any(|granted_value| granted_value == requested_value)
            })
        }
        _ => granted == requested,
    }
}

fn request_id_matches(left: &RequestId, right: &RequestId) -> bool {
    match (left, right) {
        (RequestId::Integer(left), RequestId::Integer(right)) => left == right,
        (RequestId::String(left), RequestId::String(right)) => left == right,
        (RequestId::Integer(left), RequestId::String(right)) => left.to_string() == right.as_str(),
        (RequestId::String(left), RequestId::Integer(right)) => left.as_str() == right.to_string(),
    }
}

async fn wait_for_approval_result(
    app_handle: &AppHandle,
    request_id: &RequestId,
    timeout_ms: u64,
) -> Result<serde_json::Value, String> {
    let state = app_handle.state::<AppState>();
    let mut receiver = {
        let mut guard = state.approval_rx.write().await;
        guard
            .take()
            .ok_or_else(|| "approval response receiver is already in use".to_string())?
    };

    let wait_result = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        loop {
            let Some(action) = receiver.recv().await else {
                return Err("approval response channel closed".to_string());
            };

            match action {
                ApprovalAction::Resolve {
                    request_id: response_id,
                    result,
                } if request_id_matches(&response_id, request_id) => {
                    return Ok(result);
                }
                ApprovalAction::Reject {
                    request_id: response_id,
                    error,
                } if request_id_matches(&response_id, request_id) => {
                    return Err(error.message);
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err("timed out waiting for user input".to_string()));

    let mut guard = state.approval_rx.write().await;
    *guard = Some(receiver);

    wait_result
}

/// 供 agent.rs 等模块在 approval 通道上等待用户回复（含超时）。
pub(crate) async fn wait_for_approval_result_public(
    app_handle: &AppHandle,
    request_id: &RequestId,
    timeout_ms: u64,
) -> Result<serde_json::Value, String> {
    wait_for_approval_result(app_handle, request_id, timeout_ms).await
}

fn resolve_command_cwd(base: &Path, cwd: Option<&str>) -> PathBuf {
    let Some(cwd) = cwd.filter(|value| !value.trim().is_empty()) else {
        let base_display = normalize_windows_verbatim_prefix(&base.to_string_lossy());
        return PathBuf::from(windows_display_path_to_access_path(&base_display));
    };

    let normalized = normalize_windows_verbatim_prefix(cwd);
    let path = PathBuf::from(&normalized);
    let resolved = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    let display = normalize_windows_verbatim_prefix(&resolved.to_string_lossy());
    PathBuf::from(windows_display_path_to_access_path(&display))
}

fn resolve_mcp_command_for_platform(command: &str) -> String {
    let trimmed = command.trim();
    #[cfg(target_os = "windows")]
    {
        if trimmed.eq_ignore_ascii_case("npx") {
            return "npx.cmd".to_string();
        }
        if trimmed.eq_ignore_ascii_case("npm") {
            return "npm.cmd".to_string();
        }
    }
    trimmed.to_string()
}

fn bundled_node_bin_dir(workspace_config_dir: &Path) -> Option<PathBuf> {
    let node_dir = workspace_config_dir.join("node");
    if node_dir.is_dir() {
        Some(node_dir)
    } else {
        None
    }
}

fn prepend_path_value(
    base_path: Option<std::ffi::OsString>,
    prepend_dir: &Path,
) -> std::ffi::OsString {
    let mut path_entries = vec![prepend_dir.to_path_buf()];
    if let Some(existing) = base_path {
        path_entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(path_entries).unwrap_or_else(|_| prepend_dir.as_os_str().to_os_string())
}

fn should_inject_bundled_node_runtime(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "node" | "node.exe" | "npm" | "npm.cmd" | "npx" | "npx.cmd"
    )
}

fn server_env_path_override(server: &McpServerConfig) -> Option<&str> {
    server.env.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case("PATH") {
            Some(value.as_str())
        } else {
            None
        }
    })
}

fn shell_command_display(command: &ShellCommandArg) -> String {
    match command {
        ShellCommandArg::Script(script) => script.trim().to_string(),
        ShellCommandArg::Argv(argv) => argv.join(" ").trim().to_string(),
    }
}

fn powershell_command_validation_error(command: &str) -> Option<String> {
    for (index, line) in command.lines().enumerate() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        let checkbox_placeholder = ["[ ]", "[x]"].iter().any(|prefix| {
            lower.strip_prefix(prefix).is_some_and(|rest| {
                let rest = rest.trim_start();
                rest.is_empty() || rest.starts_with('|') || rest.starts_with('#')
            })
        });
        let broken_closing_placeholder = trimmed.strip_prefix(']').is_some_and(|rest| {
            let rest = rest.trim_start();
            rest.is_empty() || rest.starts_with('|') || rest.starts_with('#')
        });

        if checkbox_placeholder || broken_closing_placeholder || trimmed.starts_with('|') {
            return Some(format!(
                "Error: invalid PowerShell command on line {}. The command argument must be one non-empty string containing a complete executable script, and every pipeline must begin with an input-producing command.",
                index + 1
            ));
        }

        if trimmed.contains("$*.") {
            return Some(format!(
                "Error: invalid PowerShell current-object reference on line {}. Inside Where-Object or ForEach-Object, use '$_' as the current pipeline object.",
                index + 1
            ));
        }
    }

    None
}

fn shell_file_editing_violation(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    let blocked_patterns = [
        "set-content",
        "out-file",
        "add-content",
        "writealltext(",
        "writeallbytes(",
        ".write_text(",
        ".write_bytes(",
        "sed -i",
    ];
    let matched = blocked_patterns
        .iter()
        .find(|pattern| lower.contains(**pattern))?;
    Some(format!(
        "Error: shell-based file editing is disabled ({matched}). Use apply_patch for existing files or write_file for new UTF-8 files. This prevents Chinese and other non-ASCII text from being corrupted by shell encoding defaults."
    ))
}

fn resolve_shell_timeout_ms(args: &ShellArgs) -> Result<u64, String> {
    let timeout_ms = args
        .timeout_ms
        .or(args.block_until_ms)
        .unwrap_or(SHELL_TIMEOUT_DEFAULT_MS);
    if timeout_ms < SHELL_TIMEOUT_MIN_MS {
        return Err(format!(
            "Error: shell timeout must be at least {SHELL_TIMEOUT_MIN_MS} ms (received {timeout_ms})."
        ));
    }
    if timeout_ms > SHELL_TIMEOUT_MAX_MS {
        return Err(format!(
            "Error: shell timeout exceeds {SHELL_TIMEOUT_MAX_MS} ms (received {timeout_ms})."
        ));
    }
    Ok(timeout_ms)
}

const POWERSHELL_UTF8_PREFIX_MARKER: &str = "# cn-codex-utf8";
const POWERSHELL_UTF8_OUTPUT_PREFIX: &str = "# cn-codex-utf8\n\
$utf8NoBom = [System.Text.UTF8Encoding]::new($false);\n\
[Console]::InputEncoding = $utf8NoBom;\n\
[Console]::OutputEncoding = $utf8NoBom;\n\
$OutputEncoding = $utf8NoBom;\n\
$PSDefaultParameterValues['*:Encoding'] = 'utf8';\n";

fn inject_powershell_utf8_prefix(script: &str) -> String {
    if script
        .trim_start()
        .starts_with(POWERSHELL_UTF8_PREFIX_MARKER)
    {
        script.to_string()
    } else {
        format!("{POWERSHELL_UTF8_OUTPUT_PREFIX}{script}")
    }
}

fn shell_requires_permission_approval(args: &ShellArgs) -> bool {
    normalize_sandbox_permissions(args.sandbox_permissions.as_deref())
        .is_some_and(|value| value != "use_default")
}

fn validate_shell_permission_args(args: &ShellArgs) -> Result<(), String> {
    let Some(permission) = normalize_sandbox_permissions(args.sandbox_permissions.as_deref())
    else {
        return Ok(());
    };

    match permission.as_str() {
        "use_default" => {
            if args.additional_permissions.is_some() {
                return Err(
                    "Error: additional_permissions requires sandbox_permissions: with_additional_permissions"
                        .to_string(),
                );
            }
        }
        "with_additional_permissions" => {
            let Some(profile) = &args.additional_permissions else {
                return Err(
                    "Error: with_additional_permissions requires additional_permissions"
                        .to_string(),
                );
            };
            if !profile.is_object() || profile.as_object().is_some_and(|object| object.is_empty()) {
                return Err("Error: additional_permissions must be a non-empty object".to_string());
            }
        }
        "require_escalated" => {}
        other => {
            return Err(format!(
                "Error: unsupported sandbox_permissions value '{other}'"
            ));
        }
    }

    Ok(())
}

fn normalize_sandbox_permissions(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        match value {
            "useDefault" => "use_default",
            "requireEscalated" => "require_escalated",
            "withAdditionalPermissions" => "with_additional_permissions",
            other => other,
        }
        .to_string(),
    )
}

fn shell_program_and_args_windows(script: &str, login: Option<bool>) -> (String, Vec<String>) {
    let ps_script = inject_powershell_utf8_prefix(&script.replace(" && ", "; "));
    let mut args = Vec::new();
    if login == Some(false) {
        args.push("-NoProfile".to_string());
    }
    args.push("-ExecutionPolicy".to_string());
    args.push("Bypass".to_string());
    args.push("-Command".to_string());
    args.push(ps_script);
    ("powershell.exe".to_string(), args)
}

fn shell_program_and_args_unix(script: &str, login: Option<bool>) -> (String, Vec<String>) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let shell_name = Path::new(&shell)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("sh");
    let supports_login = matches!(shell_name, "bash" | "zsh" | "fish");
    let flag = if login != Some(false) && supports_login {
        "-lc"
    } else {
        "-c"
    };
    (shell, vec![flag.to_string(), script.to_string()])
}

fn exec_command_program_and_args(args: &ExecCommandArgs) -> (String, Vec<String>) {
    if cfg!(target_os = "windows") {
        let shell = args
            .shell
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("powershell.exe")
            .to_string();
        let shell_name = Path::new(&shell)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if shell_name == "cmd.exe" || shell_name == "cmd" {
            return (shell, vec!["/C".to_string(), args.cmd.clone()]);
        }
        let command_script = if shell_name.contains("powershell")
            || shell_name == "pwsh.exe"
            || shell_name == "pwsh"
        {
            inject_powershell_utf8_prefix(&args.cmd)
        } else {
            args.cmd.clone()
        };
        let mut shell_args = Vec::new();
        if args.login == Some(false) {
            shell_args.push("-NoProfile".to_string());
        }
        shell_args.push("-ExecutionPolicy".to_string());
        shell_args.push("Bypass".to_string());
        shell_args.push("-Command".to_string());
        shell_args.push(command_script);
        (shell, shell_args)
    } else {
        let shell = args
            .shell
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()));
        let shell_name = Path::new(&shell)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("sh");
        let supports_login = matches!(shell_name, "bash" | "zsh" | "fish");
        let flag = if args.login != Some(false) && supports_login {
            "-lc"
        } else {
            "-c"
        };
        (shell, vec![flag.to_string(), args.cmd.clone()])
    }
}

fn exec_requires_permission_approval(args: &ExecCommandArgs) -> bool {
    normalize_sandbox_permissions(args.sandbox_permissions.as_deref())
        .is_some_and(|value| value != "use_default")
}

fn validate_exec_permission_args(args: &ExecCommandArgs) -> Result<(), String> {
    validate_permission_override_args(
        args.sandbox_permissions.as_deref(),
        args.additional_permissions.as_ref(),
    )
}

fn validate_permission_override_args(
    sandbox_permissions: Option<&str>,
    additional_permissions: Option<&serde_json::Value>,
) -> Result<(), String> {
    let Some(permission) = normalize_sandbox_permissions(sandbox_permissions) else {
        return Ok(());
    };

    match permission.as_str() {
        "use_default" => {
            if additional_permissions.is_some() {
                return Err(
                    "Error: additional_permissions requires sandbox_permissions: with_additional_permissions"
                        .to_string(),
                );
            }
        }
        "with_additional_permissions" => {
            let Some(profile) = additional_permissions else {
                return Err(
                    "Error: with_additional_permissions requires additional_permissions"
                        .to_string(),
                );
            };
            if !profile.is_object() || profile.as_object().is_some_and(|object| object.is_empty()) {
                return Err("Error: additional_permissions must be a non-empty object".to_string());
            }
        }
        "require_escalated" => {}
        other => {
            return Err(format!(
                "Error: unsupported sandbox_permissions value '{other}'"
            ));
        }
    }

    Ok(())
}

async fn collect_exec_output<R>(
    mut reader: R,
    output: Arc<Mutex<String>>,
    prefix: Option<&'static str>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buf = [0u8; 4096];
    let mut wrote_prefix = false;
    let mut decoder = crate::utf8_stream::Utf8StreamDecoder::new();
    loop {
        let Ok(n) = reader.read(&mut buf).await else {
            break;
        };
        if n == 0 {
            break;
        }
        let mut chunk = String::new();
        decoder.push(&mut chunk, &buf[..n]);
        if chunk.is_empty() {
            continue;
        }
        let mut output = output.lock().await;
        if let Some(prefix) = prefix
            && !wrote_prefix
        {
            output.push_str(prefix);
            wrote_prefix = true;
        }
        output.push_str(&chunk);
    }

    let mut tail = String::new();
    decoder.finish(&mut tail);
    if !tail.is_empty() {
        let mut output = output.lock().await;
        if let Some(prefix) = prefix
            && !wrote_prefix
        {
            output.push_str(prefix);
        }
        output.push_str(&tail);
    }
}

async fn exec_session_snapshot(
    record: &ExecSessionRecord,
    max_output_tokens: Option<usize>,
) -> serde_json::Value {
    let exit_code = *record.exit_code.lock().await;
    let output = record.output.lock().await;
    let mut cursor = record.cursor.lock().await;
    let start = (*cursor).min(output.len());
    let new_output = output[start..].to_string();
    *cursor = output.len();
    drop(cursor);
    drop(output);

    let output_limit = max_output_tokens_to_chars(max_output_tokens);
    let truncated = truncate_output(&new_output, output_limit);
    let original_token_count = estimate_token_count(&new_output);
    let wall_time_seconds = (now_millis().saturating_sub(record.started_at_ms) as f64) / 1000.0;

    let mut value = serde_json::json!({
        "wall_time_seconds": wall_time_seconds,
        "original_token_count": original_token_count,
        "output": truncated,
        "command": record.command.clone(),
        "cwd": record.cwd.clone(),
    });

    if let Some(exit_code) = exit_code {
        value["exit_code"] = serde_json::json!(exit_code);
    } else {
        value["session_id"] = serde_json::json!(record.id);
    }
    value
}

async fn close_exec_session_record(record: ExecSessionRecord) -> serde_json::Value {
    let previous_exit_code = *record.exit_code.lock().await;
    let was_running = previous_exit_code.is_none();
    let mut close_error = None;

    {
        let mut stdin = record.stdin.lock().await;
        stdin.take();
    }

    if was_running {
        match record.process_id {
            Some(pid) => {
                if let Err(error) = kill_process_tree(pid).await {
                    close_error = Some(error);
                }
            }
            None => {
                close_error = Some("Exec session process id is unavailable".to_string());
            }
        }

        let mut exit_code = record.exit_code.lock().await;
        if exit_code.is_none() {
            *exit_code = Some(if close_error.is_none() { 130 } else { -1 });
        }
    }

    {
        let mut output = record.output.lock().await;
        let message = if was_running {
            if let Some(error) = close_error.as_deref() {
                format!("\n[session close]\nFailed to stop exec session: {error}\n")
            } else {
                "\n[session close]\nExec session closed and process tree stop was requested.\n"
                    .to_string()
            }
        } else {
            "\n[session close]\nExec session was already finished and has been removed.\n"
                .to_string()
        };
        output.push_str(&message);
    }

    let mut value = exec_session_snapshot(&record, None).await;
    value["session_id"] = serde_json::json!(record.id);
    value["closed"] = serde_json::json!(close_error.is_none());
    value["was_running"] = serde_json::json!(was_running);
    value["process_id"] = serde_json::json!(record.process_id);
    if let Some(previous_exit_code) = previous_exit_code {
        value["previous_exit_code"] = serde_json::json!(previous_exit_code);
    }
    if let Some(close_error) = close_error {
        value["error"] = serde_json::json!(close_error);
    }
    value
}

fn exec_yield_duration(value: Option<u64>, after_write: bool) -> Duration {
    let default = if after_write { 250 } else { 10_000 };
    Duration::from_millis(value.unwrap_or(default).clamp(250, 300_000))
}

fn max_output_tokens_to_chars(value: Option<usize>) -> usize {
    value.unwrap_or(10_000).clamp(100, 50_000).saturating_mul(4)
}

fn estimate_token_count(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}

#[derive(Debug, Deserialize, Default)]
struct DuckDuckGoResponse {
    #[serde(default, rename = "Heading")]
    heading: String,
    #[serde(default, rename = "AbstractText")]
    abstract_text: String,
    #[serde(default, rename = "AbstractURL")]
    abstract_url: String,
    #[serde(default, rename = "RelatedTopics")]
    related_topics: Vec<DuckDuckGoTopic>,
}

#[derive(Debug, Deserialize, Default)]
struct DuckDuckGoTopic {
    #[serde(default, rename = "Text")]
    text: String,
    #[serde(default, rename = "FirstURL")]
    first_url: String,
    #[serde(default, rename = "Topics")]
    topics: Vec<DuckDuckGoTopic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebSearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebSearchProvider {
    BingBrowser,
    DuckDuckGoApi,
    DuckDuckGoBrowser,
}

const SMARTBRAIN_CONTEXT_OVERLAP_LINES: usize = 30;
const SMARTBRAIN_CONTEXT_RESULT_LIMIT: usize = 3;

fn web_search_provider_order() -> [WebSearchProvider; 3] {
    [
        WebSearchProvider::BingBrowser,
        WebSearchProvider::DuckDuckGoApi,
        WebSearchProvider::DuckDuckGoBrowser,
    ]
}

fn web_search_output_has_results(output: &str) -> bool {
    !output.starts_with("No web search results found")
}

fn take_first_lines(lines: &[String], count: usize) -> Vec<String> {
    lines.iter().take(count).cloned().collect()
}

fn take_last_lines(lines: &[String], count: usize) -> Vec<String> {
    if lines.len() <= count {
        return lines.to_vec();
    }
    lines[lines.len() - count..].to_vec()
}

fn format_duckduckgo_results(
    query: &str,
    response: DuckDuckGoResponse,
    max_results: usize,
) -> String {
    let mut seen_urls = BTreeSet::new();
    let mut results = Vec::new();

    if !response.abstract_text.trim().is_empty() {
        let title = if response.heading.trim().is_empty() {
            query.to_string()
        } else {
            response.heading.trim().to_string()
        };
        if !response.abstract_url.trim().is_empty() {
            seen_urls.insert(response.abstract_url.clone());
        }
        results.push(WebSearchResult {
            title,
            url: response.abstract_url,
            snippet: response.abstract_text,
        });
    }

    collect_duckduckgo_topics(
        &response.related_topics,
        max_results,
        &mut seen_urls,
        &mut results,
    );

    if results.is_empty() {
        return format!("No web search results found for: {query}");
    }

    let mut output = format!("Web search results for \"{query}\":\n");
    for (idx, result) in results.into_iter().take(max_results).enumerate() {
        output.push_str(&format!("\n{}. {}", idx + 1, result.title));
        if !result.url.trim().is_empty() {
            output.push_str(&format!("\n   URL: {}", result.url.trim()));
        }
        if !result.snippet.trim().is_empty() {
            output.push_str(&format!(
                "\n   Snippet: {}",
                truncate_output(&condense_whitespace(&result.snippet), 600)
            ));
        }
        output.push('\n');
    }

    output
}

fn collect_duckduckgo_topics(
    topics: &[DuckDuckGoTopic],
    max_results: usize,
    seen_urls: &mut BTreeSet<String>,
    results: &mut Vec<WebSearchResult>,
) {
    for topic in topics {
        if results.len() >= max_results {
            return;
        }

        if !topic.topics.is_empty() {
            collect_duckduckgo_topics(&topic.topics, max_results, seen_urls, results);
            continue;
        }

        let text = topic.text.trim();
        let url = topic.first_url.trim();
        if text.is_empty() || url.is_empty() || !seen_urls.insert(url.to_string()) {
            continue;
        }

        let (title, snippet) = split_search_text(text);
        results.push(WebSearchResult {
            title,
            url: url.to_string(),
            snippet,
        });
    }
}

fn split_search_text(text: &str) -> (String, String) {
    if let Some((title, snippet)) = text.split_once(" - ") {
        (title.trim().to_string(), snippet.trim().to_string())
    } else {
        (text.trim().to_string(), String::new())
    }
}

fn parse_browser_search_results(
    browser_output: &str,
    max_results: usize,
) -> Option<Vec<WebSearchResult>> {
    let parsed: serde_json::Value = extract_browser_output_json(browser_output)?;
    let actions = parsed.get("actions").and_then(|v| v.as_array())?;

    for action in actions {
        if action.get("type").and_then(|v| v.as_str()) != Some("eval") {
            continue;
        }
        let value_str = action.get("value").and_then(|v| v.as_str()).or_else(|| {
            action
                .get("value")
                .and_then(|v| serde_json::to_string(v).ok())
                .as_deref()
                .map(|_| "")
        })?;

        let items: Vec<serde_json::Value> = serde_json::from_str(value_str).ok()?;
        let mut results = Vec::new();
        for item in items.into_iter().take(max_results) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let url = item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !title.is_empty() {
                results.push(WebSearchResult {
                    title,
                    url,
                    snippet,
                });
            }
        }
        if !results.is_empty() {
            return Some(results);
        }
    }
    None
}

fn extract_browser_output_json(browser_output: &str) -> Option<serde_json::Value> {
    if let Ok(parsed) = serde_json::from_str(browser_output) {
        return Some(parsed);
    }

    let start = browser_output.find('{')?;
    let end = browser_output.rfind('}')?;
    if end <= start {
        return None;
    }

    serde_json::from_str(&browser_output[start..=end]).ok()
}

fn format_browser_search_results(query: &str, results: &[WebSearchResult]) -> String {
    let mut output = format!("Web search results for \"{query}\" (via browser):\n");
    for (idx, result) in results.iter().enumerate() {
        output.push_str(&format!("\n{}. {}", idx + 1, result.title));
        if !result.url.is_empty() {
            output.push_str(&format!("\n   URL: {}", result.url));
        }
        if !result.snippet.is_empty() {
            output.push_str(&format!(
                "\n   Snippet: {}",
                truncate_output(&condense_whitespace(&result.snippet), 600)
            ));
        }
        output.push('\n');
    }
    output
}

fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let after_open = &html[open..];
    let tag_end = after_open.find('>')?;
    let content_start = open + tag_end + 1;
    let close_rel = lower[content_start..].find("</title>")?;
    let raw_title = &html[content_start..content_start + close_rel];
    let title = condense_whitespace(&decode_html_entities(raw_title));
    (!title.is_empty()).then_some(title)
}

fn html_to_text(html: &str) -> String {
    let without_scripts = strip_block_tag(html, "script");
    let without_styles = strip_block_tag(&without_scripts, "style");
    let without_svg = strip_block_tag(&without_styles, "svg");

    let mut output = String::new();
    let mut in_tag = false;
    let mut last_space = false;

    for ch in without_svg.chars() {
        match ch {
            '<' => {
                in_tag = true;
                push_single_space(&mut output, &mut last_space);
            }
            '>' => {
                in_tag = false;
                push_single_space(&mut output, &mut last_space);
            }
            _ if in_tag => {}
            _ if ch.is_whitespace() => push_single_space(&mut output, &mut last_space),
            _ => {
                output.push(ch);
                last_space = false;
            }
        }
    }

    condense_whitespace(&decode_html_entities(&output))
}

fn strip_block_tag(input: &str, tag: &str) -> String {
    let open_pattern = format!("<{tag}");
    let close_pattern = format!("</{tag}>");
    let mut rest = input;
    let mut output = String::new();

    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open_pattern) else {
            output.push_str(rest);
            break;
        };

        output.push_str(&rest[..start]);
        let after_start = &rest[start..];
        let after_lower = after_start.to_ascii_lowercase();
        let Some(end_rel) = after_lower.find(&close_pattern) else {
            break;
        };

        rest = &after_start[end_rel + close_pattern.len()..];
    }

    output
}

fn push_single_space(output: &mut String, last_space: &mut bool) {
    if !output.is_empty() && !*last_space {
        output.push(' ');
        *last_space = true;
    }
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn condense_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn encode_query_component(input: &str) -> String {
    let mut output = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(*byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn browser_run_failure_json(error_code: &str, message: &str, hint: Option<&str>) -> String {
    let mapped = serde_json::json!({
        "ok": false,
        "errorCode": error_code,
        "message": message,
        "hint": hint.unwrap_or("请检查页面是否可访问、CDP 端口是否可用，并重试。"),
        "browserMode": "unavailable",
        "actions": [],
        "screenshots": [],
        "assetBundles": [],
        "tabs": []
    });
    serde_json::to_string_pretty(&mapped).unwrap_or_else(|_| mapped.to_string())
}

/// 将 browser_run 运行期错误归类为更精确的错误码，避免把所有失败都显示为 CDP 失效。
fn classify_browser_run_error(error: &str) -> (&'static str, &'static str) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("webview_cdp_unavailable")
        || lower.contains("cdp connect failed")
        || lower.contains("failed to query cdp tabs")
        || lower.contains("cdp websocket closed")
    {
        return (
            "WEBVIEW_CDP_UNAVAILABLE",
            "未建立到内置浏览器的 CDP 通道。请先打开内置浏览器面板；若仍失败，关闭浏览器面板后重开，或重启应用后再试。",
        );
    }
    if lower.contains("timeout waiting for selector:") {
        return (
            "SELECTOR_TIMEOUT",
            "页面已打开，但在超时时间内未找到目标选择器。请核对 selector 是否与当前 DOM 匹配。",
        );
    }
    if lower.contains("browser run interrupted by user") {
        return (
            "BROWSER_RUN_INTERRUPTED",
            "本次浏览器任务已被中断，未继续执行剩余动作。",
        );
    }
    (
        "JS_INJECTION_FAILED",
        "请检查页面是否可访问、动作参数是否正确，并重试。",
    )
}

// Windows-1252 在 0x80-0x9F 区间定义了“智能引号/破折号”等符号。
// 在某些短输出中，chardetng 可能把这段字节误判成 IBM866（会显示成西里尔字符），
// 因此这里保留与 Codex 同步的最小兜底逻辑：仅在“ASCII + 该符号字节”形态下强制回退 CP1252。
const WINDOWS_1252_PUNCT_BYTES: [u8; 8] = [0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x99];

fn decode_command_output_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_owned();
    }

    let encoding = detect_output_encoding(bytes);
    let (decoded, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        decoded.into_owned()
    }
}

fn detect_output_encoding(bytes: &[u8]) -> &'static Encoding {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let encoding = detector.guess(None, Utf8Detection::Allow);
    if encoding == IBM866 && looks_like_windows_1252_punctuation(bytes) {
        return WINDOWS_1252;
    }
    encoding
}

fn looks_like_windows_1252_punctuation(bytes: &[u8]) -> bool {
    let mut saw_extended_punctuation = false;
    let mut saw_ascii_word = false;

    for &byte in bytes {
        if byte >= 0xA0 {
            return false;
        }
        if (0x80..=0x9F).contains(&byte) {
            if !is_windows_1252_punct(byte) {
                return false;
            }
            saw_extended_punctuation = true;
        }
        if byte.is_ascii_alphabetic() {
            saw_ascii_word = true;
        }
    }

    saw_extended_punctuation && saw_ascii_word
}

fn is_windows_1252_punct(byte: u8) -> bool {
    WINDOWS_1252_PUNCT_BYTES.contains(&byte)
}

fn normalize_windows_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r#"\\?\UNC\"#) {
        return format!(r#"\\{rest}"#);
    }
    if let Some(rest) = path.strip_prefix(r#"\\.\UNC\"#) {
        return format!(r#"\\{rest}"#);
    }
    if let Some(rest) = path.strip_prefix(r#"\\?\"#) {
        return rest.to_string();
    }
    if let Some(rest) = path.strip_prefix(r#"\\.\"#)
        && is_windows_drive_absolute_path(rest)
    {
        return rest.to_string();
    }
    path.to_string()
}

fn is_windows_drive_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn windows_display_path_to_access_path(path: &str) -> String {
    if !cfg!(windows) {
        return path.to_string();
    }

    const WINDOWS_LONG_PATH_THRESHOLD: usize = 240;

    if path.starts_with(r#"\\?\"#) || path.starts_with(r#"\\.\"#) {
        return path.to_string();
    }
    if let Some(rest) = path.strip_prefix(r#"\\"#) {
        if path.len() >= WINDOWS_LONG_PATH_THRESHOLD {
            return format!(r#"\\?\UNC\{rest}"#);
        }
        return path.to_string();
    }
    if is_windows_drive_absolute_path(path) {
        if path.len() >= WINDOWS_LONG_PATH_THRESHOLD {
            return format!(r#"\\?\{path}"#);
        }
        return path.to_string();
    }
    path.to_string()
}

fn normalize_windows_paths_in_text(text: &str) -> String {
    text.lines()
        .map(|line| line.replace(r#"\\?\UNC\"#, r#"\\"#).replace(r#"\\?\"#, ""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_shell_partial_output(stdout: &str, stderr: &str) -> String {
    let combined = combine_stdout_stderr(stdout, stderr);
    if combined.trim().is_empty() {
        return combined;
    }
    let normalized = normalize_windows_paths_in_text(&combined);
    append_shell_artifact_hints(&normalized)
}

fn format_shell_command_output(exit_code: i32, stdout: &str, stderr: &str) -> String {
    let partial = format_shell_partial_output(stdout, stderr);
    if exit_code == 0 {
        partial
    } else if partial.trim().is_empty() {
        format!("[exit code: {exit_code}]")
    } else {
        format!("[exit code: {exit_code}]\n{partial}")
    }
}

fn append_shell_artifact_hints(output: &str) -> String {
    if output.contains("[artifact hints]") {
        return output.to_string();
    }
    let hints = extract_shell_artifact_hints(output, 12);
    if hints.is_empty() {
        return output.to_string();
    }

    let mut enriched = output.to_string();
    enriched.push_str("\n\n[artifact hints]\n");
    for hint in hints {
        enriched.push_str("- ");
        enriched.push_str(&hint);
        enriched.push('\n');
    }
    enriched
}

fn extract_shell_artifact_hints(output: &str, max_lines: usize) -> Vec<String> {
    let mut hints = Vec::new();
    let mut seen = BTreeSet::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized_line = normalize_windows_paths_in_text(trimmed);
        if !looks_like_shell_priority_line(&normalized_line) {
            continue;
        }
        if seen.insert(normalized_line.clone()) {
            hints.push(normalized_line.clone());
            if hints.len() >= max_lines {
                break;
            }
        }

        // 这里额外回传 display_path -> access_path 映射，避免后续二次读取时
        // 因 `\\?\` 前缀或 UNC 长路径形态差异导致“展示可见但读取失败”。
        if let Some(path) = extract_first_path_candidate(&normalized_line) {
            let display = normalize_windows_verbatim_prefix(&path);
            let access = windows_display_path_to_access_path(&display);
            if access != display {
                let mapping = format!("path mapping: {display} -> {access}");
                if seen.insert(mapping.clone()) {
                    hints.push(mapping);
                    if hints.len() >= max_lines {
                        break;
                    }
                }
            }
        }
    }
    hints
}

fn looks_like_shell_priority_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let keywords = [
        "artifact",
        "result_extract.json",
        "prepared_payload.json",
        "source.md",
        "chunks",
        "chapter",
        "summary",
        "output_path",
        "result_path",
        "cache",
        "标书",
        "章节",
        "摘要",
        "路径",
    ];
    keywords.iter().any(|keyword| lower.contains(keyword))
        || line.contains(":\\")
        || line.contains(":/")
        || line.contains(r#"\\"#)
}

fn extract_first_path_candidate(line: &str) -> Option<String> {
    let separators = [
        '"', '\'', ',', ';', '(', ')', '[', ']', '{', '}', '<', '>', '\t', '\r', '\n',
    ];
    for raw in line.split(|ch: char| ch.is_whitespace() || separators.contains(&ch)) {
        let candidate = raw.trim_matches(|ch: char| matches!(ch, '.' | ':' | '!' | '?'));
        if candidate.is_empty() {
            continue;
        }
        let normalized = normalize_windows_verbatim_prefix(candidate);
        if normalized.contains(":\\")
            || normalized.contains(":/")
            || normalized.starts_with(r#"\\"#)
        {
            return Some(normalized);
        }
    }
    None
}

fn truncate_shell_output(output: &str, max_chars: usize) -> String {
    if output.len() <= max_chars {
        return output.to_string();
    }

    let priority_lines = extract_shell_artifact_hints(output, 12);
    if priority_lines.is_empty() {
        return truncate_output(output, max_chars);
    }

    let mut priority_block = String::from("[priority lines]\n");
    for line in priority_lines {
        priority_block.push_str("- ");
        priority_block.push_str(&line);
        priority_block.push('\n');
    }

    let reserve = priority_block.len().saturating_add(220);
    let core_budget = max_chars.saturating_sub(reserve).max(max_chars / 3);
    let head_budget = core_budget / 2;
    let tail_budget = core_budget.saturating_sub(head_budget);
    let head = take_prefix_chars(output, head_budget);
    let tail = take_suffix_chars(output, tail_budget);
    let truncated = output
        .chars()
        .count()
        .saturating_sub(head.chars().count().saturating_add(tail.chars().count()));

    let merged = format!(
        "{head}\n\n... [truncated {truncated} chars; kept priority lines] ...\n\n{priority_block}\n{tail}"
    );
    if merged.len() > max_chars.saturating_add(512) {
        truncate_output(&merged, max_chars)
    } else {
        merged
    }
}

fn take_prefix_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn take_suffix_chars(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let half = max_chars / 2;
        let start: String = s.chars().take(half).collect();
        let end: String = s
            .chars()
            .rev()
            .take(half)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!(
            "{start}\n\n... [truncated {remaining} chars] ...\n\n{end}",
            remaining = s.len() - max_chars
        )
    }
}

/// Format a workspace file read, optionally as a numbered line window.
///
/// Design goals:
/// - replace shell snippets that dump `start..=end` line ranges
/// - always page large files with a stable default window
/// - return pagination metadata so the model can continue with `line_offset`
fn format_read_file_output(
    path: &str,
    content: &str,
    line_offset: Option<usize>,
    max_lines: Option<usize>,
    end_line: Option<usize>,
    show_line_numbers: Option<bool>,
) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let total_lines = lines.len();
    if total_lines == 0 {
        return format!("File: {path}\nLines: 0-0 / 0\n\n");
    }

    let start = line_offset.unwrap_or(1).max(1);
    if start > total_lines {
        return format!(
            "File: {path}\nError: line_offset {start} is beyond end of file ({total_lines} lines)."
        );
    }

    let requested_count = if let Some(end) = end_line {
        if end < start {
            return format!("File: {path}\nError: end_line {end} must be >= line_offset {start}.");
        }
        end.saturating_sub(start).saturating_add(1)
    } else {
        max_lines.unwrap_or(READ_FILE_DEFAULT_MAX_LINES)
    };
    let count = requested_count.clamp(1, READ_FILE_MAX_LINES_HARD_CAP);
    let selected = lines
        .iter()
        .skip(start.saturating_sub(1))
        .take(count)
        .copied()
        .collect::<Vec<_>>();
    let end = start.saturating_add(selected.len().saturating_sub(1));
    let has_more = end < total_lines;
    // Default to numbered pages so models can resume with exact line_offset values.
    let with_numbers = show_line_numbers.unwrap_or(true);

    if with_numbers {
        render_numbered_file_slice(path, &selected, start, total_lines, has_more)
    } else {
        let mut text = format!("File: {path}\nLines: {start}-{end} / {total_lines}\n");
        if has_more {
            text.push_str(&format!("Next line_offset: {}\n", end + 1));
        }
        text.push('\n');
        text.push_str(&selected.join("\n"));
        text
    }
}

fn render_numbered_file_slice(
    path: &str,
    selected_lines: &[&str],
    start_line: usize,
    total_lines: usize,
    has_more: bool,
) -> String {
    if selected_lines.is_empty() {
        return format!("File: {path}\nLines: 0-0 / {total_lines}\n\n");
    }

    let end_line = start_line.saturating_add(selected_lines.len().saturating_sub(1));
    let width = ((start_line.max(end_line)).max(1).ilog10() as usize) + 1;
    let mut text = format!("File: {path}\nLines: {start_line}-{end_line} / {total_lines}\n");
    if has_more {
        text.push_str(&format!("Next line_offset: {}\n", end_line + 1));
    }
    text.push('\n');
    for (idx, line) in selected_lines.iter().enumerate() {
        let number = start_line + idx;
        text.push_str(&format!("{number:>width$}|{line}\n"));
    }
    // Keep a trailing newline only when there is content; trim the final extra newline.
    text.pop();
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_specs_include_web_tools_only_when_enabled() {
        let temp_dir = tempfile::tempdir().expect("should create temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        std::fs::create_dir_all(&config_dir).expect("should create config dir");
        let executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
        let disabled = executor.tool_specs(false);
        let enabled = executor.tool_specs(true);

        let disabled_names: Vec<_> = disabled
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();
        let enabled_names: Vec<_> = enabled
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();

        assert!(!disabled_names.contains(&"web_search"));
        assert!(!disabled_names.contains(&"web_fetch"));
        assert!(disabled_names.contains(&"memory_list"));
        assert!(disabled_names.contains(&"memory_read"));
        assert!(disabled_names.contains(&"memory_search"));
        assert!(disabled_names.contains(&"memory_write"));
        assert!(disabled_names.contains(&"memory_update"));
        assert!(disabled_names.contains(&"memory_forget"));
        assert!(disabled_names.contains(&"shell_command"));
        assert!(disabled_names.contains(&"exec_command"));
        assert!(disabled_names.contains(&"write_stdin"));
        assert!(disabled_names.contains(&"close_exec_session"));
        assert!(disabled_names.contains(&"tool_search"));
        assert!(disabled_names.contains(&"apps_list"));
        assert!(disabled_names.contains(&"list_available_plugins_to_install"));
        assert!(disabled_names.contains(&"request_plugin_install"));
        assert!(disabled_names.contains(&"plugin_manage"));
        assert!(disabled_names.contains(&"code_review"));
        assert!(disabled_names.contains(&"apply_patch"));
        assert!(disabled_names.contains(&"update_plan"));
        assert!(disabled_names.contains(&"request_user_input"));
        assert!(disabled_names.contains(&"request_permissions"));
        assert!(disabled_names.contains(&"view_image"));
        assert!(disabled_names.contains(&"ocr_image"));
        assert!(disabled_names.contains(&"image_generate"));
        assert!(disabled_names.contains(&"echarts_report"));
        assert!(disabled_names.contains(&"browser_run"));
        assert!(disabled_names.contains(&"spawn_agent"));
        assert!(disabled_names.contains(&"wait_agent"));
        assert!(disabled_names.contains(&"send_input"));
        assert!(disabled_names.contains(&"resume_agent"));
        assert!(disabled_names.contains(&"list_agents"));
        assert!(disabled_names.contains(&"close_agent"));
        assert!(disabled_names.contains(&"mcp_list_servers"));
        assert!(disabled_names.contains(&"mcp_status"));
        assert!(disabled_names.contains(&"mcp_list_tools"));
        assert!(disabled_names.contains(&"mcp_call_tool"));
        assert!(disabled_names.contains(&"mcp_list_resources"));
        assert!(disabled_names.contains(&"mcp_read_resource"));
        assert!(disabled_names.contains(&"mcp_list_resource_templates"));
        assert!(disabled_names.contains(&"mcp_list_prompts"));
        assert!(disabled_names.contains(&"mcp_get_prompt"));
        assert!(enabled_names.contains(&"web_search"));
        assert!(enabled_names.contains(&"web_fetch"));
        assert!(enabled_names.contains(&"code_review"));
        assert!(enabled_names.contains(&"apps_list"));
        assert!(enabled_names.contains(&"list_available_plugins_to_install"));
        assert!(enabled_names.contains(&"request_plugin_install"));
        assert!(enabled_names.contains(&"plugin_manage"));
        assert!(enabled_names.contains(&"request_user_input"));
        assert!(enabled_names.contains(&"request_permissions"));
        assert!(enabled_names.contains(&"memory_update"));
        assert!(enabled_names.contains(&"memory_forget"));
        assert!(enabled_names.contains(&"ocr_image"));
        assert!(enabled_names.contains(&"image_generate"));
        assert!(enabled_names.contains(&"echarts_report"));
        assert!(enabled_names.contains(&"browser_run"));
        assert!(enabled_names.contains(&"spawn_agent"));
        assert!(enabled_names.contains(&"send_input"));
        assert!(enabled_names.contains(&"resume_agent"));
        assert!(enabled_names.contains(&"close_agent"));
        assert!(enabled_names.contains(&"close_exec_session"));
        assert!(enabled_names.contains(&"mcp_status"));
    }

    #[tokio::test]
    async fn layered_tool_specs_default_to_core_only_and_activate_via_search() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        std::fs::create_dir_all(&config_dir).expect("config dir");

        let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
        executor.mcp_tool_specs.insert(
            "mcp__playwright__browser_navigate".to_string(),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp__playwright__browser_navigate",
                    "description": "Navigate Playwright browser to a URL",
                    "parameters": {
                        "type": "object",
                        "properties": { "url": { "type": "string" } },
                        "required": ["url"]
                    }
                }
            }),
        );
        executor.mcp_tool_aliases.insert(
            "mcp__playwright__browser_navigate".to_string(),
            McpToolAlias {
                server: "playwright".to_string(),
                tool: "browser_navigate".to_string(),
                connector: McpConnectorMetadata::default(),
            },
        );
        executor.mcp_direct_tools_discovered = true;

        let default_specs = executor
            .tool_specs_for_turn(true, false, Some("thread-layered"))
            .await;
        let default_names: BTreeSet<_> = default_specs
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();

        assert!(default_names.contains("shell"));
        assert!(default_names.contains("read_file"));
        assert!(default_names.contains("tool_search"));
        assert!(default_names.contains("web_search"));
        assert!(!default_names.contains("memory_list"));
        assert!(!default_names.contains("spawn_agent"));
        assert!(!default_names.contains("mcp_list_tools"));
        assert!(!default_names.contains("mcp__playwright__browser_navigate"));
        assert!(
            default_names.len() <= 22,
            "core schema set should stay small, got {}",
            default_names.len()
        );

        executor
            .activate_tools_for_thread(
                "thread-layered",
                ["mcp__playwright__browser_navigate", "memory_list"],
            )
            .await;

        let activated_specs = executor
            .tool_specs_for_turn(true, false, Some("thread-layered"))
            .await;
        let activated_names: BTreeSet<_> = activated_specs
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();
        assert!(activated_names.contains("mcp__playwright__browser_navigate"));
        assert!(activated_names.contains("memory_list"));

        // Other threads remain core-only.
        let other_specs = executor
            .tool_specs_for_turn(true, false, Some("thread-other"))
            .await;
        let other_names: BTreeSet<_> = other_specs
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();
        assert!(!other_names.contains("mcp__playwright__browser_navigate"));
        assert!(!other_names.contains("memory_list"));
    }

    #[test]
    fn core_tool_name_set_is_intentionally_small() {
        assert!(ToolExecutor::is_core_tool_name("tool_search"));
        assert!(ToolExecutor::is_core_tool_name("apply_patch"));
        assert!(!ToolExecutor::is_core_tool_name(
            "mcp__playwright__browser_click"
        ));
        assert!(!ToolExecutor::is_core_tool_name("memory_list"));
        assert!(!ToolExecutor::is_core_tool_name("spawn_agent"));
    }

    #[tokio::test]
    async fn tool_search_activates_non_core_tools_for_same_thread_only() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        std::fs::create_dir_all(&config_dir).expect("config dir");

        let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
        executor.mcp_tool_specs.insert(
            "mcp__playwright__browser_navigate".to_string(),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp__playwright__browser_navigate",
                    "description": "Navigate Playwright browser to a URL",
                    "parameters": {
                        "type": "object",
                        "properties": { "url": { "type": "string" } },
                        "required": ["url"]
                    }
                }
            }),
        );
        executor.mcp_tool_aliases.insert(
            "mcp__playwright__browser_navigate".to_string(),
            McpToolAlias {
                server: "playwright".to_string(),
                tool: "browser_navigate".to_string(),
                connector: McpConnectorMetadata::default(),
            },
        );
        executor.mcp_direct_tools_discovered = true;

        // Simulate what exec_tool_search does after ranking matches.
        let matches = search_tool_entries(executor.tool_search_entries(), "playwright navigate", 8);
        assert!(
            matches
                .iter()
                .any(|entry| entry.name == "mcp__playwright__browser_navigate"),
            "playwright tool should be discoverable via tool_search"
        );

        let activated_names: Vec<String> = matches
            .iter()
            .filter(|entry| entry.kind == "tool")
            .filter(|entry| !ToolExecutor::is_core_tool_name(&entry.name))
            .map(|entry| entry.name.clone())
            .collect();
        assert!(
            activated_names
                .iter()
                .any(|name| name == "mcp__playwright__browser_navigate")
        );
        executor
            .activate_tools_for_thread("thread-search", activated_names)
            .await;

        let activated = executor
            .tool_specs_for_turn(true, false, Some("thread-search"))
            .await;
        let activated_set: BTreeSet<_> = activated
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();
        assert!(activated_set.contains("mcp__playwright__browser_navigate"));

        let other = executor
            .tool_specs_for_turn(true, false, Some("thread-other"))
            .await;
        let other_set: BTreeSet<_> = other
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();
        assert!(!other_set.contains("mcp__playwright__browser_navigate"));
    }

    #[tokio::test]
    async fn tool_search_hot_mounts_schemas_for_same_turn_next_iteration() {
        // Mirrors the agent loop: iteration N runs tool_search (activate),
        // iteration N+1 rebuilds tool_specs_for_turn and must include the schemas.
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        std::fs::create_dir_all(&config_dir).expect("config dir");

        let mut executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
        executor.mcp_tool_specs.insert(
            "mcp__playwright__browser_navigate".to_string(),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp__playwright__browser_navigate",
                    "description": "Navigate Playwright browser to a URL",
                    "parameters": {
                        "type": "object",
                        "properties": { "url": { "type": "string" } },
                        "required": ["url"]
                    }
                }
            }),
        );
        executor.mcp_tool_aliases.insert(
            "mcp__playwright__browser_navigate".to_string(),
            McpToolAlias {
                server: "playwright".to_string(),
                tool: "browser_navigate".to_string(),
                connector: McpConnectorMetadata::default(),
            },
        );
        executor.mcp_direct_tools_discovered = true;

        let thread_id = "thread-same-turn";

        // Iteration N: default core-only schema set.
        let before = executor
            .tool_specs_for_turn(true, false, Some(thread_id))
            .await;
        let before_names: BTreeSet<_> = before
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();
        assert!(!before_names.contains("mcp__playwright__browser_navigate"));

        // Simulate tool_search activation mid-turn.
        let matches = search_tool_entries(executor.tool_search_entries(), "playwright navigate", 8);
        let activated_names: Vec<String> = matches
            .iter()
            .filter(|entry| entry.kind == "tool")
            .filter(|entry| !ToolExecutor::is_core_tool_name(&entry.name))
            .map(|entry| entry.name.clone())
            .collect();
        assert!(
            activated_names
                .iter()
                .any(|name| name == "mcp__playwright__browser_navigate")
        );
        executor
            .activate_tools_for_thread(thread_id, activated_names)
            .await;

        // Iteration N+1 (same user turn): rebuild tools before the next model call.
        let after = executor
            .tool_specs_for_turn(true, false, Some(thread_id))
            .await;
        let after_names: BTreeSet<_> = after
            .iter()
            .filter_map(ToolExecutor::tool_spec_name)
            .map(str::to_string)
            .collect();
        assert!(after_names.contains("mcp__playwright__browser_navigate"));
        assert!(after_names.contains("tool_search"));
        assert!(after_names.len() > before_names.len());
    }

    #[test]
    fn smartbrain_search_tool_is_only_exposed_when_smartbrain_is_active() {
        let temp_dir = tempfile::tempdir().expect("should create temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        let memories_dir = config_dir.join("memories");
        std::fs::create_dir_all(&memories_dir).expect("should create memories dir");
        std::fs::write(memories_dir.join("smartbrain_index.json"), "{}")
            .expect("should create bm25 index file");

        std::fs::write(
            config_dir.join("config.toml"),
            "[smartbrain]\nenabled = false\n",
        )
        .expect("should write config");

        let mut executor =
            ToolExecutor::with_workspace_config_dir(root.clone(), config_dir.clone());
        let disabled_tools = executor.tool_specs(false);
        let disabled_names: Vec<_> = disabled_tools
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();
        assert!(!disabled_names.contains(&"smartbrain_search"));

        std::fs::write(
            config_dir.join("config.toml"),
            "[smartbrain]\nenabled = true\n",
        )
        .expect("should update config");

        let enabled_tools = executor.tool_specs(false);
        let enabled_names: Vec<_> = enabled_tools
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();
        assert!(enabled_names.contains(&"smartbrain_search"));
        assert!(
            enabled_names.contains(&"smartbrain_sql_query"),
            "smartbrain_sql_query must be exposed when Local Knowledge Base is enabled"
        );
        assert!(
            enabled_names.contains(&"build_entry_form"),
            "build_entry_form must be exposed when Local Knowledge Base is enabled"
        );
        assert!(
            enabled_names.contains(&"save_form_data"),
            "save_form_data must be exposed when Local Knowledge Base is enabled"
        );

        executor.set_smartbrain_enabled_override(Some(false));
        let overridden_specs = executor.tool_specs(false);
        let overridden_names: Vec<_> = overridden_specs
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();
        assert!(!overridden_names.contains(&"smartbrain_search"));
        assert!(!overridden_names.contains(&"smartbrain_sql_query"));
        assert!(!overridden_names.contains(&"build_entry_form"));
        assert!(!overridden_names.contains(&"save_form_data"));
    }

    #[test]
    fn image_generate_tool_is_hidden_when_disabled_in_settings() {
        let temp_dir = tempfile::tempdir().expect("should create temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        std::fs::create_dir_all(&config_dir).expect("should create config dir");

        std::fs::write(
            config_dir.join("config.toml"),
            "[image_generation]\nenabled = false\n",
        )
        .expect("should write config");

        let executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir.clone());
        let disabled_tools = executor.tool_specs(false);
        let disabled_names: Vec<_> = disabled_tools
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();
        assert!(!disabled_names.contains(&"image_generate"));

        std::fs::write(
            config_dir.join("config.toml"),
            "[image_generation]\nenabled = true\n",
        )
        .expect("should update config");

        let enabled_tools = executor.tool_specs(false);
        let enabled_names: Vec<_> = enabled_tools
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .collect();
        assert!(enabled_names.contains(&"image_generate"));
    }

    #[test]
    fn browser_run_tool_spec_advertises_tab_actions() {
        let executor = ToolExecutor::new(PathBuf::from("."));
        let tools = executor.tool_specs(false);
        let browser_spec = tools
            .iter()
            .find(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some("browser_run")
            })
            .expect("missing browser_run tool spec");
        let actions_description = browser_spec
            .pointer("/function/parameters/properties/actions/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        for action in [
            "set_viewport",
            "snapshot",
            "assets",
            "bundle_assets",
            "html",
            "list_tabs",
            "new_tab",
            "switch_tab",
            "close_tab",
        ] {
            assert!(
                actions_description.contains(action),
                "browser_run actions description should include {action}"
            );
        }
    }

    #[test]
    fn tool_specs_require_non_empty_shell_script_strings() {
        let executor = ToolExecutor::new(PathBuf::from("."));
        let tools = executor.tool_specs(false);
        for name in ["shell", "shell_command"] {
            let spec = tools
                .iter()
                .find(|tool| {
                    tool.pointer("/function/name")
                        .and_then(serde_json::Value::as_str)
                        == Some(name)
                })
                .unwrap_or_else(|| panic!("missing {name} tool spec"));
            assert_eq!(
                spec.pointer("/function/parameters/properties/command/type")
                    .and_then(serde_json::Value::as_str),
                Some("string")
            );
            assert_eq!(
                spec.pointer("/function/parameters/properties/command/minLength")
                    .and_then(serde_json::Value::as_u64),
                Some(1)
            );
            assert!(
                spec.pointer("/function/parameters/properties/workdir")
                    .is_some()
            );
            assert!(
                spec.pointer("/function/parameters/properties/timeout_ms")
                    .is_some()
            );
            assert!(
                spec.pointer("/function/parameters/properties/sandbox_permissions")
                    .is_some()
            );
        }
    }

    #[test]
    fn tool_specs_include_exec_session_tools() {
        let executor = ToolExecutor::new(PathBuf::from("."));
        let tools = executor.tool_specs(false);
        let exec = tools
            .iter()
            .find(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some("exec_command")
            })
            .expect("missing exec_command tool spec");
        assert!(
            exec.pointer("/function/parameters/properties/cmd")
                .is_some()
        );
        assert!(
            exec.pointer("/function/parameters/properties/yield_time_ms")
                .is_some()
        );
        assert!(
            exec.pointer("/function/parameters/properties/sandbox_permissions")
                .is_some()
        );

        let write = tools
            .iter()
            .find(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some("write_stdin")
            })
            .expect("missing write_stdin tool spec");
        assert!(
            write
                .pointer("/function/parameters/properties/session_id")
                .is_some()
        );
        assert!(
            write
                .pointer("/function/parameters/properties/chars")
                .is_some()
        );

        let close = tools
            .iter()
            .find(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some("close_exec_session")
            })
            .expect("missing close_exec_session tool spec");
        assert!(
            close
                .pointer("/function/parameters/properties/session_id")
                .is_some()
        );
    }

    #[test]
    fn apply_patch_tool_spec_allows_freeform_compatible_wrappers() {
        let executor = ToolExecutor::new(PathBuf::from("."));
        let tools = executor.tool_specs(false);
        let apply_patch = tools
            .iter()
            .find(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some("apply_patch")
            })
            .expect("apply_patch tool spec");

        assert!(
            apply_patch
                .pointer("/function/description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .contains("raw/freeform")
        );
        let description = apply_patch
            .pointer("/function/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        assert!(description.contains("Default tool"));
        assert!(description.contains("whether one file or many"));
        assert!(
            apply_patch
                .pointer("/function/parameters/properties/patch")
                .is_some()
        );
        assert!(
            apply_patch
                .pointer("/function/parameters/properties/command")
                .is_some()
        );
        assert!(
            apply_patch
                .pointer("/function/parameters/required")
                .and_then(serde_json::Value::as_array)
                .is_none_or(Vec::is_empty)
        );
    }

    #[test]
    fn base_mcp_status_entry_exposes_env_keys_not_values() {
        let server = McpServerConfig {
            name: "docs".to_string(),
            transport: "stdio".to_string(),
            command: "node".to_string(),
            args: vec!["server.js".to_string()],
            env: HashMap::from([
                ("Z_TOKEN".to_string(), "secret-value".to_string()),
                ("A_KEY".to_string(), "also-secret".to_string()),
            ]),
            cwd: Some("tools/docs".to_string()),
            url: None,
            headers: HashMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
            disabled: false,
        };

        let entry = base_mcp_status_entry(&server);

        assert_eq!(
            entry.get("name").and_then(serde_json::Value::as_str),
            Some("docs")
        );
        assert_eq!(
            entry.get("envKeys"),
            Some(&serde_json::json!(["A_KEY", "Z_TOKEN"]))
        );
        assert_eq!(
            entry.get("headerKeys"),
            Some(&serde_json::json!(["Authorization"]))
        );
        assert!(!entry.to_string().contains("secret-value"));
        assert!(!entry.to_string().contains("also-secret"));
        assert!(!entry.to_string().contains("Bearer secret"));
    }

    #[test]
    fn mcp_result_array_len_counts_expected_array_field() {
        let value = serde_json::json!({
            "tools": [
                { "name": "read" },
                { "name": "write" }
            ],
            "resources": "not-an-array"
        });

        assert_eq!(mcp_result_array_len(&value, "tools"), 2);
        assert_eq!(mcp_result_array_len(&value, "resources"), 0);
        assert_eq!(mcp_result_array_len(&value, "missing"), 0);
    }

    #[test]
    fn apps_list_output_merges_plugin_apps_with_mcp_connector_tools() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-apps-list-test-{}", uuid::Uuid::new_v4()));
        let config_dir = root.join("codey");
        let plugin_dir = config_dir.join("plugins").join("sites");
        std::fs::create_dir_all(plugin_dir.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_dir.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"sites","interface":{"displayName":"Sites"},"apps":"./.app.json"}"#,
        )
        .unwrap();
        std::fs::write(
            plugin_dir.join(".app.json"),
            r#"{"apps":{"sites":{"id":"connector_sites"}}}"#,
        )
        .unwrap();

        let aliases = HashMap::from([(
            "mcp__codex_apps__sites_create_project".to_string(),
            McpToolAlias {
                server: "codex-apps".to_string(),
                tool: "sites_create_project".to_string(),
                connector: McpConnectorMetadata {
                    connector_id: Some("connector_sites".to_string()),
                    connector_name: Some("Sites".to_string()),
                    namespace_description: Some("Create and deploy hosted sites.".to_string()),
                },
            },
        )]);

        let output = format_apps_list_output(&config_dir, &aliases, None, true);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            parsed
                .pointer("/summary/total")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            parsed
                .pointer("/summary/accessible")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            parsed
                .pointer("/apps/0/source")
                .and_then(serde_json::Value::as_str),
            Some("plugin+mcp")
        );
        assert_eq!(
            parsed
                .pointer("/apps/0/pluginApps/0/pluginDisplayName")
                .and_then(serde_json::Value::as_str),
            Some("Sites")
        );
        assert_eq!(
            parsed
                .pointer("/apps/0/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("mcp__codex_apps__sites_create_project")
        );

        let filtered =
            format_apps_list_output(&config_dir, &aliases, Some("connector_missing"), true);
        let filtered: serde_json::Value = serde_json::from_str(&filtered).unwrap();
        assert_eq!(
            filtered
                .pointer("/summary/total")
                .and_then(serde_json::Value::as_u64),
            Some(0)
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn plugin_install_candidates_read_codex_cache_metadata() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-plugin-install-list-test-{}",
            uuid::Uuid::new_v4()
        ));
        let cache = root.join("cache");
        let config_dir = root.join("codey");
        let plugin_root = cache.join("openai-bundled").join("sites").join("1.0.0");
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{
  "name": "sites",
  "version": "1.0.0",
  "description": "Create and deploy sites",
  "skills": "./skills",
  "mcpServers": "./mcp.json",
  "apps": "./.app.json"
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("skills").join("sites-hosting")).unwrap();
        std::fs::write(
            plugin_root.join("mcp.json"),
            r#"{"mcpServers":{"sites-mcp":{"command":"node"}}}"#,
        )
        .unwrap();
        std::fs::write(
            plugin_root.join(".app.json"),
            r#"{"apps":{"sites":{"id":"connector_sites"}}}"#,
        )
        .unwrap();

        let candidates = plugin_install_candidates(&cache, &config_dir, Some("deploy"), true, 10);

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.name, "sites");
        assert_eq!(candidate.version.as_deref(), Some("1.0.0"));
        assert!(candidate.id.ends_with("openai-bundled/sites/1.0.0"));
        assert!(candidate.has_skills);
        assert_eq!(candidate.mcp_server_names, vec!["sites-mcp".to_string()]);
        assert_eq!(
            candidate.app_connector_ids,
            vec!["connector_sites".to_string()]
        );
        assert!(!candidate.installed);

        let selected = select_plugin_install_candidate(&candidates, Some(&candidate.id), None)
            .expect("select by id");
        assert_eq!(selected.source, candidate.source);

        let _ = plugin_commands::import_plugin_root(
            Path::new(&selected.source),
            &config_dir.join("plugins"),
        )
        .expect("import plugin");
        let installed = plugin_install_candidates(&cache, &config_dir, Some("sites"), false, 10);
        assert!(installed.is_empty());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn tool_search_finds_builtin_skills_and_mcp_specs() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-tool-search-test-{}",
            uuid::Uuid::new_v4()
        ));
        let config_dir = root.join("codey");
        let skill_dir = config_dir.join("skills").join("browser");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Browser\ndescription: Control pages with WebView JS Injection\ntags: [\"browser\"]\n---\n",
        )
        .unwrap();
        let plugin_dir = root.join("codey").join("plugins").join("sites");
        std::fs::create_dir_all(plugin_dir.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_dir.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"sites","version":"1.0.0","description":"Build and host sites"}"#,
        )
        .unwrap();
        std::fs::write(
            plugin_dir.join(".app.json"),
            r#"{"apps":{"sites":{"id":"connector_sites"}}}"#,
        )
        .unwrap();

        let mut executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
        executor.web_search_enabled = true;
        executor.mcp_tool_aliases.insert(
            "mcp__docs__search".to_string(),
            McpToolAlias {
                server: "docs".to_string(),
                tool: "search".to_string(),
                connector: McpConnectorMetadata::default(),
            },
        );
        executor.mcp_tool_specs.insert(
            "mcp__docs__search".to_string(),
            mcp_direct_tool_spec(
                "docs",
                "search",
                &serde_json::json!({
                    "name": "search",
                    "description": "Search project docs",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "query": { "type": "string" } },
                        "required": ["query"]
                    }
                }),
                "mcp__docs__search",
            ),
        );
        executor.mcp_tool_aliases.insert(
            "mcp__docs__read".to_string(),
            McpToolAlias {
                server: "docs".to_string(),
                tool: "read".to_string(),
                connector: McpConnectorMetadata::default(),
            },
        );
        executor.mcp_tool_specs.insert(
            "mcp__docs__read".to_string(),
            mcp_direct_tool_spec(
                "docs",
                "read",
                &serde_json::json!({
                    "name": "read",
                    "description": "Read project docs",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                }),
                "mcp__docs__read",
            ),
        );

        let browser_matches = search_tool_entries(executor.tool_search_entries(), "browser", 10);
        assert!(
            browser_matches
                .iter()
                .any(|entry| entry.name == "browser_run")
        );
        assert!(browser_matches.iter().any(|entry| {
            entry.kind == "skill" && entry.name == "Browser" && entry.path.is_some()
        }));

        let docs_matches = search_tool_entries(executor.tool_search_entries(), "project docs", 10);
        let mcp = docs_matches
            .iter()
            .find(|entry| entry.name == "mcp__docs__search")
            .expect("MCP direct tool should be searchable");
        assert_eq!(mcp.source, "mcp:docs");
        assert!(mcp.spec.is_some());
        let mcp_read = docs_matches
            .iter()
            .find(|entry| entry.name == "mcp__docs__read")
            .expect("second MCP direct tool should be searchable");

        executor.mcp_tool_aliases.insert(
            "mcp__codex_apps__sites_create_project".to_string(),
            McpToolAlias {
                server: "codex-apps".to_string(),
                tool: "sites_create_project".to_string(),
                connector: McpConnectorMetadata {
                    connector_id: Some("connector_sites".to_string()),
                    connector_name: Some("Sites".to_string()),
                    namespace_description: Some("Create and deploy hosted sites.".to_string()),
                },
            },
        );
        executor.mcp_tool_specs.insert(
            "mcp__codex_apps__sites_create_project".to_string(),
            mcp_direct_tool_spec(
                "codex-apps",
                "sites_create_project",
                &serde_json::json!({
                    "name": "sites_create_project",
                    "description": "Create a hosted site project",
                    "connector_id": "connector_sites",
                    "connector_name": "Sites",
                    "connector_description": "Create and deploy hosted sites.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    }
                }),
                "mcp__codex_apps__sites_create_project",
            ),
        );
        let app_tool_matches =
            search_tool_entries(executor.tool_search_entries(), "hosted sites", 10);
        let app_tool = app_tool_matches
            .iter()
            .find(|entry| entry.name == "mcp__codex_apps__sites_create_project")
            .expect("MCP app connector tool should be searchable");
        assert_eq!(
            app_tool.metadata.get("connectorId").map(String::as_str),
            Some("connector_sites")
        );
        assert_eq!(
            app_tool.metadata.get("connectorName").map(String::as_str),
            Some("Sites")
        );
        assert!(
            app_tool
                .usage
                .as_deref()
                .unwrap_or_default()
                .contains("Sites app connector")
        );

        let output = format_tool_search_output("project docs", vec![mcp.clone(), mcp_read.clone()]);
        assert!(output.contains("\"mcp__docs__search\""));
        assert!(output.contains("\"spec\""));
        let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            output_json
                .pointer("/tools/0/type")
                .and_then(serde_json::Value::as_str),
            Some("namespace")
        );
        assert_eq!(
            output_json
                .pointer("/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("mcp__docs")
        );
        let tool_names = output_json
            .pointer("/tools/0/tools")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(tool_names, vec!["search", "read"]);

        let app_matches = search_tool_entries(executor.tool_search_entries(), "sites hosting", 10);
        let app = app_matches
            .iter()
            .find(|entry| entry.kind == "app" && entry.name == "sites: sites")
            .expect("plugin app connector should be searchable");
        assert_eq!(
            app.metadata.get("connectorId").map(String::as_str),
            Some("connector_sites")
        );
        assert!(
            app.usage
                .as_deref()
                .unwrap_or_default()
                .contains("MCP tools")
        );

        let app_output = format_tool_search_output("sites hosting", vec![app.clone()]);
        assert!(app_output.contains("\"type\": \"app\""));
        assert!(app_output.contains("\"connectorId\": \"connector_sites\""));

        let agent_matches =
            search_tool_entries(executor.tool_search_entries(), "spawn delegated agents", 10);
        assert!(
            agent_matches
                .iter()
                .any(|entry| entry.name == "spawn_agent")
        );
        assert!(
            agent_matches
                .iter()
                .any(|entry| entry.name == "close_agent")
        );

        let send_input_matches = search_tool_entries(
            executor.tool_search_entries(),
            "send message existing agent",
            10,
        );
        assert!(
            send_input_matches
                .iter()
                .any(|entry| entry.name == "send_input")
        );

        let resume_matches =
            search_tool_entries(executor.tool_search_entries(), "resume closed agent", 10);
        assert!(
            resume_matches
                .iter()
                .any(|entry| entry.name == "resume_agent")
        );

        let image_matches =
            search_tool_entries(executor.tool_search_entries(), "generate image", 10);
        assert!(
            image_matches
                .iter()
                .any(|entry| entry.name == "image_generate")
        );

        let review_matches = search_tool_entries(executor.tool_search_entries(), "review diff", 10);
        assert!(
            review_matches
                .iter()
                .any(|entry| entry.name == "code_review")
        );

        let close_exec_matches =
            search_tool_entries(executor.tool_search_entries(), "close exec session", 10);
        assert!(
            close_exec_matches
                .iter()
                .any(|entry| entry.name == "close_exec_session")
        );

        let plugin_manage_matches =
            search_tool_entries(executor.tool_search_entries(), "disable local plugin", 10);
        assert!(
            plugin_manage_matches
                .iter()
                .any(|entry| entry.name == "plugin_manage")
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn tool_search_uses_schema_text_for_bm25_ranking() {
        let fetch_spec = serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Fetch a remote page.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "Readable page address to retrieve."
                        },
                        "max_chars": {
                            "type": "integer",
                            "description": "Maximum readable text characters."
                        }
                    },
                    "required": ["url"]
                }
            }
        });
        let review_spec = serde_json::json!({
            "type": "function",
            "function": {
                "name": "code_review",
                "description": "Review the current git diff.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "base_ref": { "type": "string" }
                    }
                }
            }
        });
        let fetch = tool_search_entry_from_function_spec(&fetch_spec, "tool", "built-in", None)
            .expect("fetch spec should become searchable");
        let review = tool_search_entry_from_function_spec(&review_spec, "tool", "built-in", None)
            .expect("review spec should become searchable");

        let matches = search_tool_entries(vec![review, fetch], "readable page address", 10);

        assert_eq!(
            matches.first().map(|entry| entry.name.as_str()),
            Some("web_fetch")
        );
    }

    #[test]
    fn set_mcp_servers_preserves_cache_when_config_is_unchanged() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-mcp-cache-test-{}", uuid::Uuid::new_v4()));
        let config_dir = root.join("codey");
        let mut executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
        let mut servers = HashMap::new();
        servers.insert(
            "docs".to_string(),
            McpServerConfig {
                name: "docs".to_string(),
                transport: "stdio".to_string(),
                command: "docs-mcp".to_string(),
                args: vec!["--stdio".to_string()],
                env: HashMap::new(),
                cwd: None,
                url: None,
                headers: HashMap::new(),
                disabled: false,
            },
        );

        executor.set_mcp_servers(servers.clone());
        executor.mcp_tool_aliases.insert(
            "mcp__docs__search".to_string(),
            McpToolAlias {
                server: "docs".to_string(),
                tool: "search".to_string(),
                connector: McpConnectorMetadata::default(),
            },
        );
        executor.mcp_tool_specs.insert(
            "mcp__docs__search".to_string(),
            mcp_direct_tool_spec(
                "docs",
                "search",
                &serde_json::json!({ "name": "search" }),
                "mcp__docs__search",
            ),
        );
        executor.mcp_direct_tools_discovered = true;

        executor.set_mcp_servers(servers.clone());

        assert!(executor.mcp_direct_tools_discovered);
        assert!(executor.mcp_tool_aliases.contains_key("mcp__docs__search"));
        assert!(executor.mcp_tool_specs.contains_key("mcp__docs__search"));

        let mut changed = servers;
        changed
            .get_mut("docs")
            .unwrap()
            .args
            .push("--changed".to_string());
        executor.set_mcp_servers(changed);

        assert!(!executor.mcp_direct_tools_discovered);
        assert!(executor.mcp_tool_aliases.is_empty());
        assert!(executor.mcp_tool_specs.is_empty());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn mcp_discovery_cooldown_skips_repeated_turn_loading() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-mcp-cooldown-test-{}",
            uuid::Uuid::new_v4()
        ));
        let mut executor =
            ToolExecutor::with_workspace_config_dir(root.clone(), root.join("codey"));

        assert!(executor.mcp_discovery_needs_refresh());

        executor.mcp_discovery_retry_after = Some(Instant::now() + Duration::from_secs(300));
        assert!(!executor.mcp_discovery_needs_refresh());

        executor.mcp_discovery_retry_after = Some(Instant::now() - Duration::from_secs(1));
        assert!(executor.mcp_discovery_needs_refresh());

        executor.mcp_direct_tools_discovered = true;
        assert!(!executor.mcp_discovery_needs_refresh());
    }

    #[tokio::test]
    async fn initial_turn_defers_mcp_connection_until_tool_search_activation() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-mcp-lazy-turn-test-{}",
            uuid::Uuid::new_v4()
        ));
        let mut executor =
            ToolExecutor::with_workspace_config_dir(root.clone(), root.join("codey"));
        executor.set_mcp_servers(HashMap::from([(
            "playwright".to_string(),
            McpServerConfig {
                name: "playwright".to_string(),
                transport: "stdio".to_string(),
                command: "this-command-must-not-run".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                cwd: None,
                url: None,
                headers: HashMap::new(),
                disabled: false,
            },
        )]));

        let tools = executor
            .tool_specs_for_turn(false, false, Some("thread-1"))
            .await;
        let tool_names = tools
            .iter()
            .filter_map(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
            })
            .collect::<Vec<_>>();

        assert!(!tool_names.contains(&"mcp_list_tools"));
        assert!(!tool_names.contains(&"mcp_call_tool"));
        assert!(executor.mcp_tool_specs.is_empty());
        assert!(!executor.mcp_direct_tools_discovered);
        assert!(executor.tool_search_entries().iter().any(|entry| {
            entry.kind == "mcp_server"
                && entry.name == "MCP server: playwright"
                && entry.source == "mcp:playwright"
        }));
    }

    #[tokio::test]
    async fn mcp_request_reuses_stdio_session_for_same_server() {
        if Command::new("node")
            .arg("--version")
            .output()
            .await
            .is_err()
        {
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "cn-codex-mcp-session-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("fake-mcp.cjs");
        let starts = root.join("starts.txt");
        std::fs::write(
            &script,
            r#"
const fs = require("fs");
const readline = require("readline");
fs.appendFileSync(process.argv[2], "start\n");
const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (line) => {
  const msg = JSON.parse(line);
  if (msg.method === "notifications/initialized") return;
  if (msg.method === "initialize") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        serverInfo: { name: "fake", version: "1.0.0" }
      }
    }));
    return;
  }
  if (msg.method === "tools/list") {
    console.log(JSON.stringify({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        tools: [
          {
            name: "ping",
            description: "Ping",
            inputSchema: { type: "object", properties: {} }
          }
        ]
      }
    }));
    return;
  }
  console.log(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { ok: true } }));
});
"#,
        )
        .unwrap();

        let config_dir = root.join("codey");
        let mut executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
        let server = McpServerConfig {
            name: "fake".to_string(),
            transport: "stdio".to_string(),
            command: "node".to_string(),
            args: vec![
                script.to_string_lossy().to_string(),
                starts.to_string_lossy().to_string(),
            ],
            env: HashMap::new(),
            cwd: Some(root.to_string_lossy().to_string()),
            url: None,
            headers: HashMap::new(),
            disabled: false,
        };
        let mut servers = HashMap::new();
        servers.insert(server.name.clone(), server.clone());
        executor.set_mcp_servers(servers);

        let first = executor
            .mcp_request(&server, "tools/list", serde_json::json!({}))
            .await
            .expect("first tools/list should succeed");
        let second = executor
            .mcp_request(&server, "tools/list", serde_json::json!({}))
            .await
            .expect("second tools/list should reuse session");
        let status = executor.mcp_session_status("fake").await;
        executor.remove_mcp_session("fake").await;

        assert_eq!(
            first
                .pointer("/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("ping")
        );
        assert_eq!(
            second
                .pointer("/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("ping")
        );
        assert_eq!(
            status.get("connected").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            status
                .get("requestCount")
                .and_then(serde_json::Value::as_u64),
            Some(2)
        );
        let start_count = std::fs::read_to_string(&starts).unwrap().lines().count();
        assert_eq!(start_count, 1);

        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn mcp_request_supports_http_jsonrpc_servers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake MCP HTTP server");
        let addr = listener.local_addr().expect("fake server addr");
        let seen = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let seen_headers = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let server_seen = seen.clone();
        let server_seen_headers = seen_headers.clone();
        let server_task = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let (request, headers) = read_test_http_json_request(&mut stream).await;
                let method = request
                    .get("method")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                server_seen.lock().await.push(request.clone());
                server_seen_headers.lock().await.push(headers);

                match method.as_str() {
                    "initialize" => {
                        write_test_http_response(
                            &mut stream,
                            200,
                            Some("session-123"),
                            Some(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": request.get("id").cloned().unwrap_or_default(),
                                "result": {
                                    "protocolVersion": "2024-11-05",
                                    "capabilities": {},
                                    "serverInfo": { "name": "fake-http", "version": "1.0.0" }
                                }
                            })),
                            false,
                        )
                        .await;
                    }
                    "notifications/initialized" => {
                        write_test_http_response(
                            &mut stream,
                            202,
                            Some("session-123"),
                            None,
                            false,
                        )
                        .await;
                    }
                    "tools/list" => {
                        write_test_http_response(
                            &mut stream,
                            200,
                            Some("session-123"),
                            Some(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": request.get("id").cloned().unwrap_or_default(),
                                "result": {
                                    "tools": [
                                        {
                                            "name": "ping",
                                            "description": "Ping over HTTP",
                                            "inputSchema": { "type": "object", "properties": {} }
                                        }
                                    ]
                                }
                            })),
                            false,
                        )
                        .await;
                    }
                    other => panic!("unexpected MCP HTTP method {other}"),
                }
            }
        });

        let root =
            std::env::temp_dir().join(format!("cn-codex-mcp-http-test-{}", uuid::Uuid::new_v4()));
        let config_dir = root.join("codey");
        let executor = ToolExecutor::with_workspace_config_dir(root.clone(), config_dir);
        let server = McpServerConfig {
            name: "remote".to_string(),
            transport: "http".to_string(),
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: Some(format!("http://{addr}/mcp")),
            headers: HashMap::from([("X-Test-Token".to_string(), "secret".to_string())]),
            disabled: false,
        };

        let result = executor
            .mcp_request(&server, "tools/list", serde_json::json!({}))
            .await
            .expect("HTTP tools/list should succeed");
        let status = executor.mcp_session_status("remote").await;

        assert_eq!(
            result
                .pointer("/tools/0/name")
                .and_then(serde_json::Value::as_str),
            Some("ping")
        );
        assert_eq!(
            status.get("connected").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            status.get("transport").and_then(serde_json::Value::as_str),
            Some("http")
        );
        assert_eq!(
            status
                .get("requestCount")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            status
                .get("sessionIdPresent")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );

        let methods = seen
            .lock()
            .await
            .iter()
            .filter_map(|request| request.get("method").and_then(serde_json::Value::as_str))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            vec![
                "initialize".to_string(),
                "notifications/initialized".to_string(),
                "tools/list".to_string()
            ]
        );
        let headers = seen_headers.lock().await;
        assert!(headers.iter().all(|header| {
            header
                .get("x-test-token")
                .and_then(serde_json::Value::as_str)
                == Some("secret")
        }));
        assert_eq!(
            headers
                .get(1)
                .and_then(|header| header.get("mcp-session-id"))
                .and_then(serde_json::Value::as_str),
            Some("session-123")
        );
        assert_eq!(
            headers
                .get(2)
                .and_then(|header| header.get("mcp-session-id"))
                .and_then(serde_json::Value::as_str),
            Some("session-123")
        );

        server_task.await.expect("fake server task");
        std::fs::remove_dir_all(root).ok();
    }

    async fn read_test_http_json_request(
        stream: &mut tokio::net::TcpStream,
    ) -> (serde_json::Value, serde_json::Value) {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).await.expect("read request");
            assert!(read > 0, "client closed connection before headers");
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_header_end(&bytes) {
                break index;
            }
        };

        let headers_text = String::from_utf8_lossy(&bytes[..header_end]).to_string();
        let content_length = headers_text
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let body_start = header_end + 4;
        while bytes.len() < body_start + content_length {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).await.expect("read body");
            assert!(read > 0, "client closed connection before body");
            bytes.extend_from_slice(&chunk[..read]);
        }

        let body = &bytes[body_start..body_start + content_length];
        let request = serde_json::from_slice::<serde_json::Value>(body).expect("JSON body");
        let mut headers = serde_json::Map::new();
        for line in headers_text.lines().skip(1) {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            headers.insert(
                key.trim().to_ascii_lowercase(),
                serde_json::Value::String(value.trim().to_string()),
            );
        }
        (request, serde_json::Value::Object(headers))
    }

    async fn write_test_http_response(
        stream: &mut tokio::net::TcpStream,
        status: u16,
        session_id: Option<&str>,
        body: Option<serde_json::Value>,
        sse: bool,
    ) {
        let body = match (body, sse) {
            (Some(body), true) => format!("event: message\ndata: {}\n\n", body),
            (Some(body), false) => serde_json::to_string(&body).expect("response JSON"),
            (None, _) => String::new(),
        };
        let reason = match status {
            200 => "OK",
            202 => "Accepted",
            _ => "Status",
        };
        let content_type = if sse {
            "text/event-stream"
        } else {
            "application/json"
        };
        let session_header = session_id
            .map(|id| format!("Mcp-Session-Id: {id}\r\n"))
            .unwrap_or_default();
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\n{session_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.as_bytes().len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    }

    fn find_header_end(bytes: &[u8]) -> Option<usize> {
        bytes.windows(4).position(|window| window == b"\r\n\r\n")
    }

    #[test]
    fn resolve_subagent_cwd_requires_existing_directory() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-subagent-cwd-test-{}",
            uuid::Uuid::new_v4()
        ));
        let child = root.join("child");
        std::fs::create_dir_all(&child).unwrap();

        assert_eq!(
            resolve_subagent_cwd(&root, None).unwrap(),
            root.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_subagent_cwd(&root, Some("child")).unwrap(),
            child.canonicalize().unwrap()
        );
        assert!(resolve_subagent_cwd(&root, Some("missing")).is_err());

        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn wait_for_subagents_reports_success_without_missing_flag() {
        let id = "agent-test".to_string();
        let mut map = HashMap::new();
        map.insert(
            id.clone(),
            SubagentRecord {
                id: id.clone(),
                role: "reviewer".to_string(),
                status: "completed".to_string(),
                prompt: "review".to_string(),
                cwd: ".".to_string(),
                command: "codex exec review".to_string(),
                process_id: None,
                started_at_ms: 10,
                completed_at_ms: Some(20),
                duration_ms: Some(10),
                exit_code: Some(0),
                output: Some("done".to_string()),
                error: None,
                input_history: Vec::new(),
                last_input_at_ms: None,
            },
        );

        let result = wait_for_subagents(Arc::new(Mutex::new(map)), vec![id], 0).await;
        assert!(!result.has_missing);
        assert!(!result.has_failed);
        assert!(result.output.contains("\"completed\": true"));
    }

    #[test]
    fn load_subagent_records_marks_running_records_interrupted() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-subagent-load-test-{}",
            uuid::Uuid::new_v4()
        ));
        let config_dir = root.join("codey");
        std::fs::create_dir_all(config_dir.join("subagents")).unwrap();
        std::fs::write(
            subagent_state_path(&config_dir),
            r#"[
  {
    "id": "agent-running",
    "role": "tester",
    "status": "running",
    "prompt": "test",
    "cwd": ".",
    "command": "codex exec test",
    "processId": 123,
    "startedAtMs": 10
  }
]"#,
        )
        .unwrap();

        let records = load_subagent_records(&config_dir);
        let record = records.get("agent-running").unwrap();
        assert_eq!(record.status, "interrupted");
        assert_eq!(record.process_id, None);
        assert!(
            record
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("running")
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn persist_subagent_records_writes_state_file() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-subagent-persist-test-{}",
            uuid::Uuid::new_v4()
        ));
        let config_dir = root.join("codey");
        let id = "agent-persist-test".to_string();
        let mut map = HashMap::new();
        map.insert(
            id.clone(),
            SubagentRecord {
                id: id.clone(),
                role: "tester".to_string(),
                status: "completed".to_string(),
                prompt: "test".to_string(),
                cwd: ".".to_string(),
                command: "codex exec test".to_string(),
                process_id: None,
                started_at_ms: 10,
                completed_at_ms: Some(20),
                duration_ms: Some(10),
                exit_code: Some(0),
                output: Some("done".to_string()),
                error: None,
                input_history: Vec::new(),
                last_input_at_ms: None,
            },
        );
        let subagents = Arc::new(Mutex::new(map));

        persist_subagent_records(&config_dir, &subagents).await;

        let state = std::fs::read_to_string(subagent_state_path(&config_dir)).unwrap();
        assert!(state.contains("agent-persist-test"));
        assert!(state.contains("completed"));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn browser_run_display_uses_url_and_action_count() {
        let payload = serde_json::json!({
            "url": "http://localhost:1420",
            "actions": [
                { "type": "screenshot" },
                { "type": "text", "selector": "body" }
            ]
        });
        assert_eq!(
            browser_run_display(&payload),
            "http://localhost:1420 (2 actions)"
        );

        let empty = serde_json::json!({});
        assert_eq!(browser_run_display(&empty), "browser");
    }

    #[test]
    fn browser_run_prefers_visible_browser_unless_disabled() {
        assert!(browser_run_use_visible_browser(&serde_json::json!({})));
        assert!(!browser_run_use_visible_browser(&serde_json::json!({
            "use_visible_browser": false
        })));
        assert!(!browser_run_use_visible_browser(&serde_json::json!({
            "useVisibleBrowser": false
        })));
    }

    #[test]
    fn browser_run_initial_url_uses_url_or_first_goto() {
        assert_eq!(
            browser_run_initial_url(&serde_json::json!({
                "url": " http://localhost:1420 "
            })),
            Some("http://localhost:1420".to_string())
        );
        assert_eq!(
            browser_run_initial_url(&serde_json::json!({
                "actions": [
                    { "type": "screenshot" },
                    { "type": "goto", "url": "https://example.com" }
                ]
            })),
            Some("https://example.com".to_string())
        );
        assert_eq!(browser_run_initial_url(&serde_json::json!({})), None);
    }

    #[test]
    fn classify_browser_run_error_maps_selector_timeout() {
        let (code, hint) = classify_browser_run_error(
            "Action 0 (wait_for_selector) failed: Timeout waiting for selector: #game-board",
        );
        assert_eq!(code, "SELECTOR_TIMEOUT");
        assert!(hint.contains("选择器"));
    }

    #[test]
    fn classify_browser_run_error_maps_cdp_unavailable() {
        let (code, hint) = classify_browser_run_error(
            "WEBVIEW_CDP_UNAVAILABLE: cannot connect to http://127.0.0.1:9242",
        );
        assert_eq!(code, "WEBVIEW_CDP_UNAVAILABLE");
        assert!(hint.contains("CDP"));
    }

    #[test]
    fn extract_browser_output_json_parses_mixed_stdout_and_stderr() {
        let mixed = "{\n  \"actions\": []\n}\n[stderr]\nwarning";
        let parsed = extract_browser_output_json(mixed).expect("expected browser json");
        assert_eq!(
            parsed
                .get("actions")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn mcp_direct_tool_name_sanitizes_and_caps_length() {
        let mut used = BTreeSet::new();
        let name = mcp_direct_tool_name("docs server", "read-file", &mut used);
        assert_eq!(name, "mcp__docs_server__read_file");

        let long = mcp_direct_tool_name(
            "server-with-a-very-very-long-name",
            "tool-with-a-very-very-long-name-and-extra-suffix",
            &mut used,
        );
        assert!(long.starts_with("mcp__server_with_a_very_very_long_name__tool_with"));
        assert!(long.len() <= 64);
    }

    #[test]
    fn mcp_direct_tool_spec_uses_input_schema() {
        let spec = mcp_direct_tool_spec(
            "docs",
            "search",
            &serde_json::json!({
                "name": "search",
                "description": "Search docs",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                }
            }),
            "mcp__docs__search",
        );

        assert_eq!(
            spec.get("function")
                .and_then(|function| function.get("name"))
                .and_then(serde_json::Value::as_str),
            Some("mcp__docs__search")
        );
        assert_eq!(
            spec.pointer("/function/parameters/required/0")
                .and_then(serde_json::Value::as_str),
            Some("query")
        );
    }

    #[test]
    fn mcp_connector_metadata_is_trusted_only_for_codex_apps() {
        let tool = serde_json::json!({
            "name": "gmail_send",
            "description": "Send a message",
            "connector_id": "connector_gmail",
            "connector_name": "Gmail",
            "connector_description": "Tools for Gmail."
        });

        let trusted = mcp_connector_metadata("codex-apps", &tool);
        assert_eq!(trusted.connector_id.as_deref(), Some("connector_gmail"));
        assert_eq!(trusted.connector_name.as_deref(), Some("Gmail"));
        assert_eq!(
            trusted.namespace_description.as_deref(),
            Some("Tools for Gmail.")
        );

        let untrusted = mcp_connector_metadata("custom-server", &tool);
        assert_eq!(untrusted, McpConnectorMetadata::default());

        let trusted_spec = mcp_direct_tool_spec("codex-apps", "gmail_send", &tool, "mcp__x");
        assert!(
            trusted_spec
                .pointer("/function/description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .contains("MCP app connector Gmail (connector_gmail)")
        );

        let untrusted_spec = mcp_direct_tool_spec("custom-server", "gmail_send", &tool, "mcp__x");
        let description = untrusted_spec
            .pointer("/function/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        assert!(description.contains("MCP tool custom-server:gmail_send."));
        assert!(!description.contains("connector_gmail"));
    }

    #[test]
    fn code_review_analyzer_flags_risks_and_missing_tests() {
        let diff = r#"diff --git a/src/app.rs b/src/app.rs
index 1111111..2222222 100644
--- a/src/app.rs
+++ b/src/app.rs
@@ -10,0 +11,3 @@
+const API_KEY: &str = "sk_live_1234567890abcdef";
+let value = maybe.unwrap();
+todo!();
diff --git a/src/ui.tsx b/src/ui.tsx
index 1111111..2222222 100644
--- a/src/ui.tsx
+++ b/src/ui.tsx
@@ -3,0 +4,2 @@
+console.log("debug");
+<div dangerouslySetInnerHTML={{ __html: html }} />
"#;
        let numstat = "3\t0\tsrc/app.rs\n2\t0\tsrc/ui.tsx\n";
        let diff_check = GitCommandOutput {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        };

        let summary = analyze_code_review_diff(diff, numstat, &diff_check, &[], false);

        assert_eq!(summary.files_changed, 2);
        assert_eq!(summary.additions, 5);
        assert!(
            summary
                .findings
                .iter()
                .any(|finding| finding.priority == "P1" && finding.title.contains("Secret"))
        );
        assert!(summary.findings.iter().any(|finding| {
            finding
                .title
                .contains("Source changed without matching test")
        }));
        assert!(
            summary
                .findings
                .iter()
                .any(|finding| finding.title.contains("Risky dynamic"))
        );
        assert!(
            summary
                .findings
                .iter()
                .any(|finding| finding.title.contains("New Rust panic"))
        );
    }

    #[test]
    fn image_generation_helpers_build_request_and_paths() {
        assert_eq!(
            image_generation_api_url(Some("https://example.test/v1/"), None),
            "https://example.test/v1/images/generations"
        );
        assert_eq!(
            image_generation_api_url(Some("https://example.test/v1/images/generations/"), None),
            "https://example.test/v1/images/generations"
        );
        assert_eq!(
            image_generation_api_url(None, Some("https://settings.test/v1/")),
            "https://settings.test/v1/images/generations"
        );
        assert_eq!(
            image_generation_model(Some("tool-override"), Some("settings-default")),
            "tool-override"
        );
        assert_eq!(
            image_generation_model(None, Some("settings-default")),
            "settings-default"
        );
        assert_eq!(
            image_generation_api_key(Some("sk-settings")),
            Some("sk-settings".to_string())
        );

        let body = image_generation_request_body(
            "draw a window",
            "gpt-image-1",
            Some("auto"),
            Some("high"),
            None,
            Some(3),
        );
        assert_eq!(
            body.pointer("/model").and_then(serde_json::Value::as_str),
            Some("gpt-image-1")
        );
        assert_eq!(
            body.pointer("/size").and_then(serde_json::Value::as_str),
            Some("auto")
        );
        assert_eq!(
            body.pointer("/quality").and_then(serde_json::Value::as_str),
            Some("high")
        );
        assert_eq!(
            body.pointer("/n").and_then(serde_json::Value::as_u64),
            Some(3)
        );
        assert_eq!(
            body.pointer("/response_format")
                .and_then(serde_json::Value::as_str),
            Some("b64_json")
        );
        assert!(body.pointer("/background").is_none());
        assert_eq!(image_generation_count(Some(0)), 1);
        assert_eq!(image_generation_count(Some(99)), 10);
        assert_eq!(
            ToolExecutor::resolve_generated_image_url(
                "/v1/files/image/example.png",
                "https://rayplus.site/v1/images/generations",
            )
            .unwrap(),
            "https://rayplus.site/v1/files/image/example.png"
        );
        assert_eq!(
            ToolExecutor::resolve_generated_image_url(
                "https://cdn.example.test/image.png",
                "https://rayplus.site/v1/images/generations",
            )
            .unwrap(),
            "https://cdn.example.test/image.png"
        );
        assert!(
            ToolExecutor::resolve_generated_image_url(
                "",
                "https://rayplus.site/v1/images/generations"
            )
            .is_err()
        );

        assert_eq!(
            decode_image_base64("data:image/png;base64, aGk=\n").unwrap(),
            b"hi"
        );

        let root = PathBuf::from("C:/workspace/app");
        let config_dir = root.join("codey");
        let default_path = resolve_image_generate_output_path(
            &root,
            &config_dir,
            None,
            "call-1",
            "draw a window",
            "png",
        )
        .unwrap();
        assert!(default_path.starts_with(config_dir.join("images").join("generated")));
        assert_eq!(
            default_path.extension().and_then(|value| value.to_str()),
            Some("png")
        );

        assert_eq!(
            resolve_image_generate_output_path(
                &root,
                &config_dir,
                Some("assets/generated/window"),
                "call-1",
                "draw",
                "webp",
            )
            .unwrap(),
            root.join("assets").join("generated").join("window.webp")
        );
        assert!(
            resolve_image_generate_output_path(
                &root,
                &config_dir,
                Some("../window.png"),
                "call-1",
                "draw",
                "png",
            )
            .is_err()
        );

        assert_eq!(
            resolve_image_generate_output_path_for_index(
                &root,
                &config_dir,
                Some("assets/generated/window"),
                "call-1",
                "draw",
                "png",
                0,
                2,
            )
            .unwrap(),
            root.join("assets").join("generated").join("window-1.png")
        );
        assert_eq!(
            resolve_image_generate_output_path_for_index(
                &root,
                &config_dir,
                Some("assets/generated/window"),
                "call-1",
                "draw",
                "png",
                1,
                2,
            )
            .unwrap(),
            root.join("assets").join("generated").join("window-2.png")
        );
    }

    #[test]
    fn inspect_image_bytes_reads_png_dimensions() {
        let mut png = b"\x89PNG\r\n\x1A\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&320u32.to_be_bytes());
        png.extend_from_slice(&180u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);

        assert_eq!(
            inspect_image_bytes(&png).unwrap(),
            ImageInfo {
                format: "PNG",
                mime: "image/png",
                width: 320,
                height: 180,
            }
        );
    }

    #[test]
    fn format_plan_update_validates_and_renders_plan() {
        let output = format_plan_update(
            Some("Working through parity gaps"),
            &[
                PlanItemArg {
                    step: "Audit missing tools".to_string(),
                    status: "completed".to_string(),
                },
                PlanItemArg {
                    step: "Add plan tool".to_string(),
                    status: "in_progress".to_string(),
                },
                PlanItemArg {
                    step: "Run tests".to_string(),
                    status: "pending".to_string(),
                },
            ],
        )
        .unwrap();

        assert!(output.contains("Plan updated: Working through parity gaps"));
        assert!(output.contains("- [completed] Audit missing tools"));
        assert!(output.contains("- [in_progress] Add plan tool"));
    }

    #[test]
    fn format_plan_update_rejects_multiple_in_progress_items() {
        let err = format_plan_update(
            None,
            &[
                PlanItemArg {
                    step: "One".to_string(),
                    status: "in_progress".to_string(),
                },
                PlanItemArg {
                    step: "Two".to_string(),
                    status: "in_progress".to_string(),
                },
            ],
        )
        .unwrap_err();

        assert!(err.contains("only one plan item"));
    }

    #[test]
    fn format_echarts_report_outputs_reusable_echarts_block() {
        let output = format_echarts_report(&EchartsReportArgs {
            title: Some("Quarterly revenue".to_string()),
            chart_type: Some("bar".to_string()),
            option: serde_json::json!({
                "xAxis": { "type": "category", "data": ["Q1", "Q2"] },
                "yAxis": { "type": "value" },
                "series": [{ "type": "bar", "data": [120, 180] }]
            }),
            notes: Some("Revenue in ten-thousands".to_string()),
        })
        .unwrap();

        assert!(output.contains("ECharts report ready: Quarterly revenue"));
        assert!(output.contains("Chart type: bar"));
        assert!(output.contains("```echarts"));
        assert!(output.contains("\"series\""));
    }

    #[test]
    fn format_echarts_report_rejects_non_object_option() {
        let err = format_echarts_report(&EchartsReportArgs {
            title: None,
            chart_type: None,
            option: serde_json::json!(["invalid"]),
            notes: None,
        })
        .unwrap_err();

        assert!(err.contains("option must be a JSON object"));
    }

    #[test]
    fn request_user_input_validation_limits_questions_and_options() {
        let valid = RequestUserInputArgs {
            questions: vec![RequestUserInputQuestion {
                id: "choice".to_string(),
                header: "Choice".to_string(),
                question: "Which path should I take?".to_string(),
                options: vec![
                    RequestUserInputQuestionOption {
                        label: "Fast".to_string(),
                        description: "Move quickly.".to_string(),
                    },
                    RequestUserInputQuestionOption {
                        label: "Careful".to_string(),
                        description: "Spend more time verifying.".to_string(),
                    },
                ],
            }],
        };
        assert!(validate_request_user_input_args(&valid).is_ok());

        let empty = RequestUserInputArgs { questions: vec![] };
        assert!(
            validate_request_user_input_args(&empty)
                .unwrap_err()
                .contains("at least one")
        );

        let too_many = RequestUserInputArgs {
            questions: vec![
                valid.questions[0].clone(),
                valid.questions[0].clone(),
                valid.questions[0].clone(),
                valid.questions[0].clone(),
            ],
        };
        assert!(
            validate_request_user_input_args(&too_many)
                .unwrap_err()
                .contains("at most three")
        );
    }

    #[test]
    fn request_permissions_validation_requires_known_non_empty_profile() {
        let valid = RequestPermissionsArgs {
            environment_id: None,
            reason: Some("Need to fetch dependencies".to_string()),
            permissions: serde_json::json!({
                "network": { "enabled": true }
            }),
        };
        assert!(validate_request_permissions_args(&valid).is_ok());

        let empty = RequestPermissionsArgs {
            environment_id: None,
            reason: None,
            permissions: serde_json::json!({}),
        };
        assert!(
            validate_request_permissions_args(&empty)
                .unwrap_err()
                .contains("at least one")
        );

        let unknown = RequestPermissionsArgs {
            environment_id: None,
            reason: None,
            permissions: serde_json::json!({ "camera": true }),
        };
        assert!(
            validate_request_permissions_args(&unknown)
                .unwrap_err()
                .contains("network or file_system")
        );
    }

    #[test]
    fn permission_profile_covers_only_granted_subsets() {
        let granted = serde_json::json!({
            "network": { "enabled": true },
            "file_system": {
                "read": ["D:/workspace", "D:/cache"],
                "write": ["D:/workspace/out"]
            }
        });

        assert!(permission_profile_covers(
            &granted,
            &serde_json::json!({
                "network": { "enabled": true },
                "file_system": { "read": ["D:/cache"] }
            })
        ));
        assert!(!permission_profile_covers(
            &granted,
            &serde_json::json!({
                "network": { "enabled": false }
            })
        ));
        assert!(!permission_profile_covers(
            &granted,
            &serde_json::json!({
                "file_system": { "write": ["D:/secret"] }
            })
        ));
    }

    #[test]
    fn granted_permissions_are_extracted_from_approval_result() {
        let result = serde_json::json!({
            "permissions": { "network": { "enabled": true } },
            "scope": "turn",
            "strict_auto_review": false
        });
        assert_eq!(
            granted_permissions_from_result(&result),
            Some(serde_json::json!({ "network": { "enabled": true } }))
        );
        assert!(
            granted_permissions_from_result(&serde_json::json!({ "permissions": {} })).is_none()
        );
    }

    #[tokio::test]
    async fn executor_reuses_granted_additional_permissions() {
        let executor = ToolExecutor::new(PathBuf::from("."));
        let granted = serde_json::json!({
            "network": { "enabled": true },
            "file_system": { "read": ["D:/workspace"] }
        });
        executor.remember_permission_grant(granted).await;

        assert!(
            executor
                .additional_permissions_preapproved(
                    Some("with_additional_permissions"),
                    Some(&serde_json::json!({
                        "network": { "enabled": true }
                    })),
                )
                .await
        );
        assert!(
            !executor
                .additional_permissions_preapproved(
                    Some("with_additional_permissions"),
                    Some(&serde_json::json!({
                        "file_system": { "write": ["D:/workspace"] }
                    })),
                )
                .await
        );
        assert!(
            !executor
                .additional_permissions_preapproved(
                    Some("require_escalated"),
                    Some(&serde_json::json!({
                        "network": { "enabled": true }
                    })),
                )
                .await
        );
    }

    #[test]
    fn inspect_image_bytes_reads_gif_jpeg_and_webp_dimensions() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&64u16.to_le_bytes());
        gif.extend_from_slice(&32u16.to_le_bytes());
        assert_eq!(inspect_image_bytes(&gif).unwrap().width, 64);
        assert_eq!(inspect_image_bytes(&gif).unwrap().height, 32);

        let jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00,
            0x0A, 0x00, 0x14, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xFF,
            0xD9,
        ];
        let jpeg_info = inspect_image_bytes(&jpeg).unwrap();
        assert_eq!(jpeg_info.format, "JPEG");
        assert_eq!(jpeg_info.width, 20);
        assert_eq!(jpeg_info.height, 10);

        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&22u32.to_le_bytes());
        webp.extend_from_slice(b"WEBPVP8X");
        webp.extend_from_slice(&10u32.to_le_bytes());
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(&[127, 2, 0]);
        webp.extend_from_slice(&[223, 0, 0]);
        let webp_info = inspect_image_bytes(&webp).unwrap();
        assert_eq!(webp_info.format, "WebP");
        assert_eq!(webp_info.width, 640);
        assert_eq!(webp_info.height, 224);
    }

    #[test]
    fn resolve_view_image_path_accepts_absolute_and_rejects_parent_relative_paths() {
        let root = PathBuf::from("C:/workspace/app");
        assert_eq!(
            resolve_view_image_path(&root, "assets/pic.png").unwrap(),
            root.join("assets").join("pic.png")
        );
        assert_eq!(
            resolve_view_image_path(&root, "D:/images/pic.png").unwrap(),
            PathBuf::from("D:/images/pic.png")
        );
        assert!(resolve_view_image_path(&root, "../pic.png").is_err());
    }

    #[test]
    fn extract_patch_argument_accepts_raw_patch_text_and_json_aliases() {
        let raw = r#"*** Begin Patch
*** Add File: src/raw.txt
+raw
*** End Patch"#;
        assert_eq!(extract_patch_argument(raw).unwrap(), raw);

        let wrapped_patch = serde_json::json!({ "patch": raw }).to_string();
        assert_eq!(extract_patch_argument(&wrapped_patch).unwrap(), raw);

        let wrapped_command = serde_json::json!({ "command": raw }).to_string();
        assert_eq!(extract_patch_argument(&wrapped_command).unwrap(), raw);

        let err = extract_patch_argument(r#"{"body":"missing"}"#).unwrap_err();
        assert!(err.contains("raw patch text"));
        assert!(err.contains("patch"));
        assert!(err.contains("command"));
    }

    #[test]
    fn apply_patch_adds_updates_and_deletes_files() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-test-{}",
            uuid::Uuid::new_v4()
        ));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.txt"), "one\ntwo\n").unwrap();
        std::fs::write(src.join("remove.txt"), "remove me\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: src/app.txt
@@
 one
-two
+three
*** Add File: src/new.txt
+hello
+world
*** Delete File: src/remove.txt
*** End Patch"#;

        let report = apply_patch_to_workspace(&root, patch).unwrap();

        assert_eq!(
            std::fs::read_to_string(src.join("app.txt")).unwrap(),
            "one\nthree\n"
        );
        assert_eq!(
            std::fs::read_to_string(src.join("new.txt")).unwrap(),
            "hello\nworld\n"
        );
        assert!(!src.join("remove.txt").exists());
        assert_eq!(
            report.changes,
            vec![
                ApplyPatchReportChange {
                    path: "src/app.txt".to_string(),
                    action: "modified",
                    move_to: None,
                },
                ApplyPatchReportChange {
                    path: "src/new.txt".to_string(),
                    action: "created",
                    move_to: None,
                },
                ApplyPatchReportChange {
                    path: "src/remove.txt".to_string(),
                    action: "deleted",
                    move_to: None,
                },
            ]
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_patch_progress_changes_describe_file_actions() {
        let patch = r#"*** Begin Patch
*** Add File: ./src/new.txt
+hello
*** Update File: src/app.txt
@@
-old
+new
*** Update File: src/old-name.txt
*** Move to: src/new-name.txt
@@
-old
+new
*** Delete File: src/remove.txt
*** End Patch"#;

        let actions = parse_patch_actions(patch).unwrap();
        assert_eq!(
            apply_patch_progress_changes(&actions),
            vec![
                ApplyPatchProgressChange {
                    path: "src/new.txt".to_string(),
                    action: "created",
                    move_to: None,
                    additions: 1,
                    deletions: 0,
                },
                ApplyPatchProgressChange {
                    path: "src/app.txt".to_string(),
                    action: "modified",
                    move_to: None,
                    additions: 1,
                    deletions: 1,
                },
                ApplyPatchProgressChange {
                    path: "src/old-name.txt".to_string(),
                    action: "renamed",
                    move_to: Some("src/new-name.txt".to_string()),
                    additions: 1,
                    deletions: 1,
                },
                ApplyPatchProgressChange {
                    path: "src/remove.txt".to_string(),
                    action: "deleted",
                    move_to: None,
                    additions: 0,
                    deletions: 0,
                },
            ]
        );
    }

    #[test]
    fn apply_patch_updates_and_moves_file() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-move-test-{}",
            uuid::Uuid::new_v4()
        ));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("old.txt"), "alpha\nbeta\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: src/old.txt
*** Move to: src/new.txt
@@
 alpha
-beta
+gamma
*** End Patch"#;

        let report = apply_patch_to_workspace(&root, patch).unwrap();

        assert!(!src.join("old.txt").exists());
        assert_eq!(
            std::fs::read_to_string(src.join("new.txt")).unwrap(),
            "alpha\ngamma\n"
        );
        assert_eq!(
            report.changes,
            vec![ApplyPatchReportChange {
                path: "src/old.txt".to_string(),
                action: "renamed",
                move_to: Some("src/new.txt".to_string()),
            }]
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_patch_rejects_unsafe_paths() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-unsafe-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let patch = r#"*** Begin Patch
*** Add File: ../secret.txt
+nope
*** End Patch"#;

        let err = apply_patch_to_workspace(&root, patch).unwrap_err();

        assert!(err.contains("must not contain '..'"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_patch_accepts_absolute_paths_inside_workspace() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-absolute-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("index.html");
        std::fs::write(&target, "<title>ABCDE</title>\n").unwrap();
        let patch = format!(
            "*** Begin Patch\n*** Update File: {}\n@@\n-<title>ABCDE</title>\n+<title>时尚代码</title>\n*** End Patch",
            target.display()
        );

        apply_patch_to_workspace(&root, &patch).unwrap();

        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "<title>时尚代码</title>\n"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_patch_rejects_absolute_paths_outside_workspace() {
        let workspace = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-workspace-test-{}",
            uuid::Uuid::new_v4()
        ));
        let outside = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-outside-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace).unwrap();
        let patch = format!(
            "*** Begin Patch\n*** Add File: {}\n+outside\n*** End Patch",
            outside.join("file.txt").display()
        );

        let err = apply_patch_to_workspace(&workspace, &patch).unwrap_err();

        assert!(err.contains("must be inside the workspace"));
        assert!(!outside.join("file.txt").exists());
        std::fs::remove_dir_all(workspace).ok();
    }

    #[test]
    fn apply_patch_accepts_unified_diff_file_headers() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-unified-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<title>ABCDE</title>\n").unwrap();
        let patch = r#"*** Begin Patch
*** Update File: index.html
--- a/index.html
+++ b/index.html
@@ -1 +1 @@
-<title>ABCDE</title>
+<title>时尚代码</title>
*** End Patch"#;

        apply_patch_to_workspace(&root, patch).unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("index.html")).unwrap(),
            "<title>时尚代码</title>\n"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_patch_preflights_all_actions_before_writing() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-apply-patch-preflight-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("first.txt"), "before\n").unwrap();
        std::fs::write(root.join("exists.txt"), "keep\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: first.txt
@@
-before
+after
*** Add File: exists.txt
+replacement
*** End Patch"#;

        let err = apply_patch_to_workspace(&root, patch).unwrap_err();

        assert!(err.contains("file already exists"));
        assert_eq!(
            std::fs::read_to_string(root.join("first.txt")).unwrap(),
            "before\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("exists.txt")).unwrap(),
            "keep\n"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn format_mcp_selection_result_preserves_partial_errors() {
        let (exit_code, output) = format_mcp_selection_result(
            vec![
                (
                    "ok".to_string(),
                    Ok(serde_json::json!({ "tools": [{ "name": "read" }] })),
                ),
                ("bad".to_string(), Err("failed".to_string())),
            ],
            "tools",
        );

        assert_eq!(exit_code, -1);
        assert!(output.contains("\"server\": \"ok\""));
        assert!(output.contains("\"server\": \"bad\""));
        assert!(output.contains("failed"));
    }

    #[test]
    fn resolve_command_cwd_handles_relative_and_absolute_paths() {
        let base = PathBuf::from("C:/workspace/app");
        assert_eq!(resolve_command_cwd(&base, None), base);
        assert_eq!(
            resolve_command_cwd(Path::new("C:/workspace/app"), Some("tools/mcp")),
            PathBuf::from("C:/workspace/app").join("tools/mcp")
        );
        assert_eq!(
            resolve_command_cwd(Path::new("C:/workspace/app"), Some("D:/mcp")),
            PathBuf::from("D:/mcp")
        );
    }

    #[test]
    fn resolve_mcp_command_for_platform_maps_windows_node_wrappers() {
        if cfg!(target_os = "windows") {
            assert_eq!(resolve_mcp_command_for_platform("npx"), "npx.cmd");
            assert_eq!(resolve_mcp_command_for_platform("npm"), "npm.cmd");
        } else {
            assert_eq!(resolve_mcp_command_for_platform("npx"), "npx");
            assert_eq!(resolve_mcp_command_for_platform("npm"), "npm");
        }
        assert_eq!(resolve_mcp_command_for_platform("python"), "python");
    }

    #[test]
    fn should_inject_bundled_node_runtime_for_node_wrappers() {
        assert!(should_inject_bundled_node_runtime("npx"));
        assert!(should_inject_bundled_node_runtime("npx.cmd"));
        assert!(should_inject_bundled_node_runtime("npm"));
        assert!(should_inject_bundled_node_runtime("C:/bin/node.exe"));
        assert!(!should_inject_bundled_node_runtime("python"));
    }

    #[test]
    fn prepend_path_value_puts_bundled_dir_first() {
        let bundled = PathBuf::from("node-runtime");
        let existing = std::env::join_paths([PathBuf::from("bin-a"), PathBuf::from("bin-b")])
            .expect("failed to build PATH fixture");
        let merged = prepend_path_value(Some(existing), &bundled);
        let entries: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(entries.first(), Some(&bundled));
    }

    #[test]
    fn shell_args_accept_codex_style_script_and_legacy_argv() {
        let script: ShellArgs = serde_json::from_str(
            r#"{
                "command": "Get-ChildItem -Force",
                "workdir": "D:/workspace/app",
                "timeout_ms": 5000,
                "login": false
            }"#,
        )
        .unwrap();
        assert_eq!(
            shell_command_display(&script.command),
            "Get-ChildItem -Force"
        );
        assert_eq!(script.workdir.as_deref(), Some("D:/workspace/app"));
        assert_eq!(script.timeout_ms, Some(5000));
        assert_eq!(script.login, Some(false));

        let argv: ShellArgs = serde_json::from_str(
            r#"{
                "command": ["pnpm", "test", "--", "--run"]
            }"#,
        )
        .unwrap();
        assert_eq!(shell_command_display(&argv.command), "pnpm test -- --run");

        let alias_timeout: ShellArgs = serde_json::from_str(
            r#"{
                "command": "echo alias",
                "block_until_ms": 45000
            }"#,
        )
        .unwrap();
        assert_eq!(alias_timeout.timeout_ms, None);
        assert_eq!(alias_timeout.block_until_ms, Some(45000));
        assert_eq!(resolve_shell_timeout_ms(&alias_timeout).unwrap(), 45000);
    }

    #[test]
    fn powershell_prefix_configures_console_pipeline_and_file_cmdlet_utf8() {
        let script =
            inject_powershell_utf8_prefix("Get-Content index.html -Raw | Set-Content index.html");

        assert!(script.starts_with(POWERSHELL_UTF8_PREFIX_MARKER));
        assert!(script.contains("[Console]::InputEncoding"));
        assert!(script.contains("[Console]::OutputEncoding"));
        assert!(script.contains("$OutputEncoding"));
        assert!(script.contains("$PSDefaultParameterValues['*:Encoding'] = 'utf8'"));
    }

    #[test]
    fn powershell_prefix_is_not_added_twice() {
        let script = inject_powershell_utf8_prefix("Write-Output '中文'");
        assert_eq!(inject_powershell_utf8_prefix(&script), script);
    }

    #[test]
    fn shell_file_editing_guard_blocks_encoding_risk_commands() {
        for command in [
            "Get-Content index.html -Raw | Set-Content index.html -Encoding UTF8",
            "$text | Out-File index.html",
            "Path('index.html').write_text(text)",
            "sed -i 's/old/new/' index.html",
        ] {
            let error = shell_file_editing_violation(command).expect(command);
            assert!(error.contains("Use apply_patch"));
            assert!(error.contains("encoding"));
        }

        assert!(shell_file_editing_violation("cargo test --lib").is_none());
        assert!(shell_file_editing_violation("Get-Content index.html -Raw").is_none());
    }

    #[test]
    fn powershell_validation_rejects_missing_pipeline_input_and_bad_current_item() {
        for command in [
            "[ ] | Select-Object Name",
            "[x] | Select-Object -First 30 LineNumber,Filename,Line",
            "[ ] # try shell",
            "] # try shell",
            "  | Where-Object { $_.Name -match 'skill' }",
        ] {
            let error = powershell_command_validation_error(command).expect(command);
            assert!(error.contains("complete executable script"));
            assert!(error.contains("input-producing command"));
        }

        let error = powershell_command_validation_error(
            "Get-ChildItem | Where-Object { $*.FullName -match 'skill' }",
        )
        .expect("bad current-item variable");
        assert!(error.contains("Use '$_'"));
    }

    #[test]
    fn powershell_validation_accepts_complete_pipelines() {
        for command in [
            "$items | Select-Object Name",
            "Get-ChildItem | Where-Object { $_.FullName -match 'skill' }",
            "cargo test --lib",
        ] {
            assert!(
                powershell_command_validation_error(command).is_none(),
                "{command}"
            );
        }
    }

    #[test]
    fn shell_timeout_contract_enforces_explicit_bounds() {
        let mut args = ShellArgs {
            command: ShellCommandArg::Script("echo ok".to_string()),
            workdir: None,
            timeout_ms: Some(999),
            block_until_ms: None,
            login: None,
            sandbox_permissions: None,
            justification: None,
            prefix_rule: None,
            additional_permissions: None,
        };
        assert!(
            resolve_shell_timeout_ms(&args)
                .unwrap_err()
                .contains("at least")
        );

        args.timeout_ms = Some(3_600_001);
        assert!(
            resolve_shell_timeout_ms(&args)
                .unwrap_err()
                .contains("exceeds")
        );

        args.timeout_ms = None;
        args.block_until_ms = Some(60_000);
        assert_eq!(resolve_shell_timeout_ms(&args).unwrap(), 60_000);
    }

    #[test]
    fn decode_command_output_bytes_supports_gbk_cp1252_and_utf8() {
        let utf8 = "路径 C:/workspace/result.json";
        assert_eq!(decode_command_output_bytes(utf8.as_bytes()), utf8);

        let (gbk_bytes, _, _) = encoding_rs::GBK.encode("中文输出");
        assert_eq!(decode_command_output_bytes(&gbk_bytes), "中文输出");

        let (cp1252_bytes, _, _) = WINDOWS_1252.encode("“quoted” test");
        let decoded = decode_command_output_bytes(&cp1252_bytes);
        assert!(decoded.contains("quoted"));
        assert!(decoded.contains('“'));
        assert!(decoded.contains('”'));
    }

    #[test]
    fn normalize_windows_path_helpers_strip_verbatim_prefix() {
        assert_eq!(
            normalize_windows_verbatim_prefix(r#"\\?\C:\work\out\result.json"#),
            r#"C:\work\out\result.json"#
        );
        assert_eq!(
            normalize_windows_verbatim_prefix(r#"\\?\UNC\server\share\result.json"#),
            r#"\\server\share\result.json"#
        );
    }

    #[test]
    fn truncate_shell_output_keeps_priority_lines() {
        let mut source = String::new();
        source.push_str(&"x".repeat(3500));
        source.push_str("\nartifact root: C:\\work\\artifact\\run-1\n");
        source.push_str("result_extract.json: C:\\work\\artifact\\run-1\\result_extract.json\n");
        source.push_str(&"y".repeat(3500));

        let truncated = truncate_shell_output(&source, 1200);
        assert!(truncated.contains("priority lines"));
        assert!(truncated.contains("result_extract.json"));
    }

    #[test]
    fn shell_permission_args_validate_additional_permissions() {
        let default_with_extra = ShellArgs {
            command: ShellCommandArg::Script("echo ok".to_string()),
            workdir: None,
            timeout_ms: None,
            block_until_ms: None,
            login: None,
            sandbox_permissions: Some("use_default".to_string()),
            justification: None,
            prefix_rule: None,
            additional_permissions: Some(serde_json::json!({ "network": { "enabled": true } })),
        };
        assert!(
            validate_shell_permission_args(&default_with_extra)
                .unwrap_err()
                .contains("with_additional_permissions")
        );

        let missing_profile = ShellArgs {
            sandbox_permissions: Some("withAdditionalPermissions".to_string()),
            additional_permissions: None,
            ..default_with_extra.clone()
        };
        assert!(
            validate_shell_permission_args(&missing_profile)
                .unwrap_err()
                .contains("requires additional_permissions")
        );

        let valid = ShellArgs {
            sandbox_permissions: Some("with_additional_permissions".to_string()),
            additional_permissions: Some(serde_json::json!({
                "file_system": { "read": ["D:/workspace/app"] }
            })),
            ..default_with_extra
        };
        assert!(validate_shell_permission_args(&valid).is_ok());
        assert!(shell_requires_permission_approval(&valid));
    }

    #[test]
    fn exec_command_args_parse_codex_style_fields() {
        let args: ExecCommandArgs = serde_json::from_str(
            r#"{
                "cmd": "pnpm test -- --run",
                "workdir": "D:/workspace/app",
                "yield_time_ms": 750,
                "max_output_tokens": 1200,
                "sandbox_permissions": "requireEscalated",
                "justification": "Need to run local tests"
            }"#,
        )
        .unwrap();

        assert_eq!(args.cmd, "pnpm test -- --run");
        assert_eq!(args.workdir.as_deref(), Some("D:/workspace/app"));
        assert_eq!(args.yield_time_ms, Some(750));
        assert_eq!(args.max_output_tokens, Some(1200));
        assert!(exec_requires_permission_approval(&args));
        assert!(validate_exec_permission_args(&args).is_ok());
    }

    #[tokio::test]
    async fn exec_session_snapshot_returns_incremental_output_and_exit_code() {
        let record = ExecSessionRecord {
            id: 7,
            process_id: None,
            command: "echo hi".to_string(),
            cwd: "D:/workspace/app".to_string(),
            started_at_ms: now_millis(),
            output: Arc::new(Mutex::new("hello\n".to_string())),
            cursor: Arc::new(Mutex::new(0)),
            exit_code: Arc::new(Mutex::new(None)),
            stdin: Arc::new(Mutex::new(None)),
        };

        let first = exec_session_snapshot(&record, Some(100)).await;
        assert_eq!(
            first.get("session_id").and_then(serde_json::Value::as_u64),
            Some(7)
        );
        assert_eq!(
            first.get("output").and_then(serde_json::Value::as_str),
            Some("hello\n")
        );

        record.output.lock().await.push_str("done\n");
        *record.exit_code.lock().await = Some(0);
        let second = exec_session_snapshot(&record, Some(100)).await;
        assert_eq!(
            second.get("exit_code").and_then(serde_json::Value::as_i64),
            Some(0)
        );
        assert_eq!(
            second.get("output").and_then(serde_json::Value::as_str),
            Some("done\n")
        );
    }

    #[tokio::test]
    async fn close_exec_session_record_marks_finished_session_closed() {
        let record = ExecSessionRecord {
            id: 9,
            process_id: None,
            command: "echo done".to_string(),
            cwd: "D:/workspace/app".to_string(),
            started_at_ms: now_millis(),
            output: Arc::new(Mutex::new("done\n".to_string())),
            cursor: Arc::new(Mutex::new(0)),
            exit_code: Arc::new(Mutex::new(Some(0))),
            stdin: Arc::new(Mutex::new(None)),
        };

        let closed = close_exec_session_record(record).await;

        assert_eq!(
            closed.get("session_id").and_then(serde_json::Value::as_u64),
            Some(9)
        );
        assert_eq!(
            closed.get("closed").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            closed
                .get("was_running")
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            closed
                .get("previous_exit_code")
                .and_then(serde_json::Value::as_i64),
            Some(0)
        );
        assert!(
            closed
                .get("output")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .contains("already finished")
        );
    }

    #[tokio::test]
    async fn close_exec_session_record_stops_running_process() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        let mut child = match command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return,
        };
        let process_id = child.id();
        let record = ExecSessionRecord {
            id: 11,
            process_id,
            command: "long-running".to_string(),
            cwd: "D:/workspace/app".to_string(),
            started_at_ms: now_millis(),
            output: Arc::new(Mutex::new(String::new())),
            cursor: Arc::new(Mutex::new(0)),
            exit_code: Arc::new(Mutex::new(None)),
            stdin: Arc::new(Mutex::new(None)),
        };

        let closed = close_exec_session_record(record).await;
        let wait_result = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;

        assert!(
            wait_result.is_ok(),
            "close_exec_session should stop the running process"
        );
        assert_eq!(
            closed.get("closed").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            closed
                .get("was_running")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            closed.get("exit_code").and_then(serde_json::Value::as_i64),
            Some(130)
        );
    }

    #[test]
    fn resolve_memory_path_rejects_escape_paths() {
        let root = PathBuf::from("C:/tmp/cn-codex-memories");

        assert_eq!(
            resolve_memory_path(&root, "project/notes.md").unwrap(),
            root.join("project").join("notes.md")
        );
        assert!(resolve_memory_path(&root, "../secret.md").is_err());
        assert!(resolve_memory_path(&root, "C:/secret.md").is_err());
        assert!(resolve_memory_path(&root, "/secret.md").is_err());
    }

    #[test]
    fn search_memory_files_finds_markdown_matches() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-memory-test-{}", uuid::Uuid::new_v4()));
        let nested = root.join("projects");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("notes.md"),
            "First line\nRemember the lighthouse project\nOther line\n",
        )
        .unwrap();
        std::fs::write(nested.join("binary.bin"), "lighthouse").unwrap();

        let result = search_memory_files(&root, &root, "LIGHTHOUSE", false, 1, 0, 10).unwrap();

        assert_eq!(
            result.matches,
            vec![MemorySearchMatch {
                path: "projects/notes.md".to_string(),
                line_number: 2,
                line: "Remember the lighthouse project".to_string(),
                before: vec!["First line".to_string()],
                after: vec!["Other line".to_string()],
            }]
        );
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.next_cursor, None);

        let json = format_memory_search_output("LIGHTHOUSE", &result, MemoryOutputFormat::Json);
        assert!(json.contains("\"totalMatches\": 1"));
        assert!(json.contains("\"before\""));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn html_to_text_strips_script_style_and_decodes_entities() {
        let html = r#"
            <html>
              <head><title>Example &amp; Docs</title><style>.x{display:none}</style></head>
              <body><h1>Hello&nbsp;world</h1><script>alert(1)</script><p>Rust &lt;3</p></body>
            </html>
        "#;

        assert_eq!(extract_html_title(html).as_deref(), Some("Example & Docs"));
        let text = html_to_text(html);
        assert!(text.contains("Hello world"));
        assert!(text.contains("Rust <3"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("display:none"));
    }

    #[test]
    fn format_duckduckgo_results_flattens_related_topics() {
        let response = DuckDuckGoResponse {
            related_topics: vec![DuckDuckGoTopic {
                topics: vec![DuckDuckGoTopic {
                    text: "CN-Codex - A local coding app".to_string(),
                    first_url: "https://example.com/cn-codex".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let formatted = format_duckduckgo_results("cn codex", response, 5);

        assert!(formatted.contains("CN-Codex"));
        assert!(formatted.contains("https://example.com/cn-codex"));
        assert!(formatted.contains("A local coding app"));
    }

    #[test]
    fn web_search_provider_order_prefers_bing_before_duckduckgo() {
        assert_eq!(
            web_search_provider_order(),
            [
                WebSearchProvider::BingBrowser,
                WebSearchProvider::DuckDuckGoApi,
                WebSearchProvider::DuckDuckGoBrowser
            ]
        );
    }

    #[test]
    fn web_search_output_has_results_detects_empty_marker() {
        assert!(web_search_output_has_results(
            "Web search results for \"cn codex\":\n1. Result"
        ));
        assert!(!web_search_output_has_results(
            "No web search results found for: cn codex (tried Bing and DuckDuckGo)"
        ));
    }

    #[test]
    fn render_chunk_context_bridge_includes_neighbor_overlap_windows() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-smartbrain-context-{}",
            uuid::Uuid::new_v4()
        ));
        let docs_dir = root
            .join("codey")
            .join("memories")
            .join("knowledge")
            .join("docs");
        std::fs::create_dir_all(&docs_dir).unwrap();

        let prev_content = (1..=40)
            .map(|idx| format!("prev-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let current_content = (1..=40)
            .map(|idx| format!("current-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let next_content = (1..=40)
            .map(|idx| format!("next-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            docs_dir.join(crate::smartbrain::knowledge::chunk_file_name("doc", 1)),
            &prev_content,
        )
        .unwrap();
        std::fs::write(
            docs_dir.join(crate::smartbrain::knowledge::chunk_file_name("doc", 2)),
            &current_content,
        )
        .unwrap();
        std::fs::write(
            docs_dir.join(crate::smartbrain::knowledge::chunk_file_name("doc", 3)),
            &next_content,
        )
        .unwrap();

        let executor = ToolExecutor::new(root.clone());
        let result = crate::smartbrain::search::SmartBrainSearchResult {
            doc_id: "know:doc::chunk:0002".to_string(),
            source_type: "knowledge".to_string(),
            file_path: "knowledge/docs/doc__chunk_0002.md".to_string(),
            title: "Doc chunk".to_string(),
            score: 1.0,
            tags: Vec::new(),
            concept_type: None,
            domain: None,
            source_group: None,
            relative_path: None,
            source_file: None,
            parent_doc_id: Some("doc".to_string()),
            chunk_index: Some(2),
            chunk_total: Some(3),
            is_chunk: true,
        };

        let block = executor
            .render_chunk_context_bridge(&result, 30)
            .expect("chunk context bridge should be generated");

        assert!(block.contains("overlap >= 30 lines"));
        assert!(block.contains("Prev chunk 1/3"));
        assert!(block.contains("Current chunk 2/3"));
        assert!(block.contains("next chunk 3/3"));
        assert!(block.contains("prev-11"));
        assert!(!block.contains("prev-10"));
        assert!(block.contains("next-30"));
        assert!(!block.contains("next-31"));
        assert!(block.contains("current-1"));
        assert!(block.contains("current-40"));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn encode_query_component_handles_spaces_and_unicode() {
        assert_eq!(encode_query_component("cn codex"), "cn%20codex");
        assert_eq!(encode_query_component("网页"), "%E7%BD%91%E9%A1%B5");
    }

    #[test]
    fn format_read_file_output_returns_full_file_without_range() {
        let content = "alpha\nbeta\ngamma\n";
        let output = format_read_file_output("src/demo.rs", content, None, None, None, None);
        assert!(output.contains("File: src/demo.rs"));
        assert!(output.contains("Lines: 1-3 / 3"));
        assert!(output.contains("1|alpha"));
        assert!(output.contains("2|beta"));
        assert!(output.contains("3|gamma"));
        assert!(!output.contains("Next line_offset:"));
    }

    #[test]
    fn format_read_file_output_supports_numbered_window_and_end_line() {
        let content = (1..=12)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let output =
            format_read_file_output("src/demo.rs", &content, Some(4), None, Some(6), Some(true));

        assert!(output.contains("File: src/demo.rs"));
        assert!(output.contains("Lines: 4-6 / 12"));
        assert!(output.contains("Next line_offset: 7"));
        assert!(output.contains("4|line-4"));
        assert!(output.contains("5|line-5"));
        assert!(output.contains("6|line-6"));
        assert!(!output.contains("3|line-3"));
        assert!(!output.contains("7|line-7"));
    }

    #[test]
    fn format_read_file_output_rejects_invalid_end_line() {
        let output =
            format_read_file_output("src/demo.rs", "a\nb\nc\n", Some(3), None, Some(1), None);
        assert!(output.contains("end_line 1 must be >= line_offset 3"));
    }

    #[test]
    fn format_read_file_output_defaults_to_paginated_window() {
        let content = (1..=250)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let output = format_read_file_output("src/demo.rs", &content, None, None, None, None);
        assert!(output.contains("Lines: 1-200 / 250"));
        assert!(output.contains("Next line_offset: 201"));
        assert!(output.contains("1|line-1"));
        assert!(output.contains("200|line-200"));
        assert!(!output.contains("201|line-201"));
    }

    #[test]
    fn format_read_file_output_hard_caps_max_lines() {
        let content = (1..=500)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let output =
            format_read_file_output("src/demo.rs", &content, Some(1), Some(1000), None, None);
        assert!(output.contains("Lines: 1-400 / 500"));
        assert!(output.contains("Next line_offset: 401"));
        assert!(!output.contains("401|line-401"));
    }

    #[test]
    fn read_file_tool_spec_exposes_range_parameters() {
        let temp_dir = tempfile::tempdir().expect("should create temp dir");
        let root = temp_dir.path().join("workspace");
        let config_dir = root.join("codey");
        std::fs::create_dir_all(&config_dir).expect("should create config dir");
        let executor = ToolExecutor::with_workspace_config_dir(root, config_dir);
        let tools = executor.tool_specs(false);
        let read_file = tools
            .iter()
            .find(|tool| {
                tool.get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(serde_json::Value::as_str)
                    == Some("read_file")
            })
            .expect("read_file tool spec");

        let properties = read_file
            .pointer("/function/parameters/properties")
            .expect("read_file properties");
        assert!(properties.get("line_offset").is_some());
        assert!(properties.get("max_lines").is_some());
        assert!(properties.get("end_line").is_some());
        assert!(properties.get("show_line_numbers").is_some());

        let description = read_file
            .pointer("/function/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        assert!(description.contains("line_offset"));
        assert!(description.contains("Prefer this over shell"));

        let max_lines = properties
            .pointer("/max_lines/maximum")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        assert_eq!(max_lines, 400);
        assert!(
            description.contains("Defaults to a numbered page")
                || description.contains("numbered page")
        );
    }
}
