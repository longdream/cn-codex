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
use crate::git_service::GitService;
use crate::plugin_loader;
use crate::protocol::RequestId;
use crate::state::{AppState, ApprovalAction};

mod code_review_support;
mod code_search_support;
pub(crate) mod memory_support;
mod patch_support;
mod browser_support;
mod file_op_support;
mod image_gen_support;
mod interaction_support;
mod mcp_support;
mod plugin_manage_support;
mod shell_support;
mod subagent_support;
mod tool_search_support;
mod web_search_support;
mod smartbrain_support;
mod tool_specs_support;
use memory_support::{read_okf_body_lines, resolve_memory_path};
use patch_support::ApplyPatchProgressChange;
pub(crate) use interaction_support::*;
pub(crate) use mcp_support::*;
pub(crate) use shell_support::*;
pub(crate) use subagent_support::*;
pub(crate) use tool_search_support::*;
pub(crate) use web_search_support::*;

/// Provider configuration for internal subagents (set by parent agent before each turn).
#[derive(Debug, Clone, Default)]
pub struct SubagentProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub wire_api: String,
    pub system_prompt_prefix: String,
    pub max_output_tokens: Option<i64>,
    pub reasoning_effort: Option<String>,
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
    mcp_sse_sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSseSession>>>>>,
    web_search_enabled: bool,
    /// Request-scoped SmartBrain override. `None` falls back to workspace config.
    smartbrain_enabled_override: Option<bool>,
    subagent_enabled_override: Option<bool>,
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
pub(crate) struct McpToolAlias {
    server: String,
    tool: String,
    connector: McpConnectorMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct McpConnectorMetadata {
    connector_id: Option<String>,
    connector_name: Option<String>,
    namespace_description: Option<String>,
}

pub(crate) struct McpSession {
    server: McpServerConfig,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    child: Child,
    stderr: Arc<Mutex<String>>,
    next_request_id: i64,
    request_count: u64,
    initialized_at_ms: i64,
}

pub(crate) struct McpHttpSession {
    server: McpServerConfig,
    session_id: Option<String>,
    next_request_id: i64,
    request_count: u64,
    initialized_at_ms: i64,
}

pub(crate) struct McpSseSession {
    server: McpServerConfig,
    endpoint_url: String,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<Result<serde_json::Value, String>>,
    worker: tokio::task::JoinHandle<()>,
    next_request_id: i64,
    request_count: u64,
    initialized_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum McpRequestError {
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
pub(crate) struct ShellArgs {
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
const TOOL_OUTPUT_READ_FILE_MAX_CHARS: usize = 32_000;
const TOOL_OUTPUT_BROWSER_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_SEARCH_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_MEMORY_MAX_CHARS: usize = 8_000;
const TOOL_OUTPUT_SMARTBRAIN_MAX_CHARS: usize = 8_000;

/// Default pagination window for `read_file` when no range is requested.
const READ_FILE_DEFAULT_MAX_LINES: usize = 400;
const READ_FILE_MAX_LINES_HARD_CAP: usize = 2_000;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum ShellCommandArg {
    Script(String),
    Argv(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ExecCommandArgs {
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
pub(crate) struct WriteStdinArgs {
    session_id: u64,
    #[serde(default)]
    chars: Option<String>,
    #[serde(default)]
    yield_time_ms: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct CloseExecSessionArgs {
    session_id: u64,
}

#[derive(Clone)]
pub(crate) struct ExecSessionRecord {
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
pub(crate) struct ActiveToolProcess {
    thread_id: String,
    call_id: String,
    tool_name: String,
    child: Arc<Mutex<Child>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlanUpdateArgs {
    #[serde(default)]
    explanation: Option<String>,
    plan: Vec<PlanItemArg>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PlanItemArg {
    #[serde(default, alias = "content", alias = "text", alias = "title")]
    step: String,
    #[serde(default = "default_plan_item_status")]
    status: String,
}

fn default_plan_item_status() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RequestUserInputArgs {
    questions: Vec<RequestUserInputQuestion>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RequestUserInputQuestion {
    id: String,
    header: String,
    question: String,
    #[serde(default)]
    options: Vec<RequestUserInputQuestionOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RequestUserInputQuestionOption {
    label: String,
    description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub(crate) struct RequestPermissionsArgs {
    #[serde(default, rename = "environment_id", alias = "environmentId")]
    environment_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    permissions: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolSearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolSearchEntry {
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
pub(crate) struct AppsListArgs {
    #[serde(default)]
    connector_id: Option<String>,
    #[serde(default)]
    include_tools: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ListAvailablePluginsArgs {
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
pub(crate) struct RequestPluginInstallArgs {
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
pub(crate) struct PluginManageArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    plugin_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct McpManageArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default, alias = "transport")]
    r#type: Option<String>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    config: Option<serde_json::Value>,
    #[serde(default)]
    overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct SkillManageArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    skill_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    overwrite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginInstallCandidate {
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
pub(crate) struct AppConnectorListEntry {
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
pub(crate) struct AppConnectorPluginSource {
    plugin_id: String,
    plugin_display_name: String,
    app_key: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppConnectorToolEntry {
    name: String,
    server: String,
    tool: String,
    connector_name: Option<String>,
    namespace_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub(crate) struct SpawnAgentArgs {
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
pub(crate) struct WaitAgentArgs {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    agent_ids: Option<Vec<String>>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct SendInputArgs {
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
pub(crate) struct ResumeAgentArgs {
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
pub(crate) struct ListAgentsArgs {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct CloseAgentArgs {
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct ImageGenerateArgs {
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
pub(crate) struct ImageGenerationSettingsResolved {
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
pub(crate) struct EchartsReportArgs {
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
pub(crate) struct GeneratedImageOutput {
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
pub(crate) struct ImageGenerationResponse {
    data: Vec<ImageGenerationItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImageGenerationItem {
    #[serde(default)]
    b64_json: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    revised_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImageGenerationErrorResponse {
    error: Option<ImageGenerationErrorDetail>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImageGenerationErrorDetail {
    message: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubagentRecord {
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
pub(crate) struct SubagentInputRecord {
    submission_id: String,
    message: String,
    submitted_at_ms: i64,
    interrupt: bool,
    delivered_to_stdin: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SubagentWaitResult {
    output: String,
    has_missing: bool,
    has_failed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubagentCloseResult {
    target: String,
    closed: bool,
    previous_status: String,
    message: String,
    agent: Option<SubagentRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendInputResult {
    target: String,
    submission_id: String,
    status: String,
    delivered_to_stdin: bool,
    queued: bool,
    note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResumeAgentResult {
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
            mcp_sse_sessions: Arc::new(Mutex::new(HashMap::new())),
            web_search_enabled: false,
            smartbrain_enabled_override: None,
            subagent_enabled_override: None,
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

    /// 为独立对话线程创建隔离 executor：复用 http / codey 配置根，运行时状态全新空表。
    /// 不共享 MCP / subagent / 进程表，避免跨项目互串。
    pub fn spawn_isolated(&self) -> Self {
        Self {
            cwd: self.cwd.clone(),
            http: self.http.clone(),
            workspace_config_dir: self.workspace_config_dir.clone(),
            mcp_servers: HashMap::new(),
            mcp_tool_aliases: HashMap::new(),
            mcp_tool_specs: HashMap::new(),
            mcp_direct_tools_discovered: false,
            mcp_discovery_retry_after: None,
            activated_tools_by_thread: Arc::new(Mutex::new(HashMap::new())),
            mcp_sessions: Arc::new(Mutex::new(HashMap::new())),
            mcp_http_sessions: Arc::new(Mutex::new(HashMap::new())),
            mcp_sse_sessions: Arc::new(Mutex::new(HashMap::new())),
            web_search_enabled: false,
            smartbrain_enabled_override: None,
            subagent_enabled_override: None,
            // 运行时空表：跨 thread 不共享、不预载磁盘记录，避免 list_agents 串台。
            subagents: Arc::new(Mutex::new(HashMap::new())),
            subagent_handles: Arc::new(Mutex::new(HashMap::new())),
            exec_sessions: Arc::new(Mutex::new(HashMap::new())),
            next_exec_session_id: Arc::new(AtomicU64::new(1)),
            permission_grants: Arc::new(Mutex::new(Vec::new())),
            active_tool_processes: Arc::new(Mutex::new(HashMap::new())),
            active_browser_cancellations: Arc::new(Mutex::new(HashMap::new())),
            subagent_provider_config: Arc::new(Mutex::new(SubagentProviderConfig::default())),
        }
    }

    /// 释放本 executor 的 MCP / 工具进程 / subagent，供 thread 删除时调用。
    pub async fn shutdown(&self) {
        let _ = self.interrupt_all_active_tools().await;
        {
            let handles = {
                let mut map = self.subagent_handles.lock().await;
                map.drain().map(|(_, handle)| handle).collect::<Vec<_>>()
            };
            for handle in handles {
                handle.cancel_flag.store(true, Ordering::SeqCst);
            }
        }
        self.subagents.lock().await.clear();
        self.exec_sessions.lock().await.clear();
        self.permission_grants.lock().await.clear();
        self.activated_tools_by_thread.lock().await.clear();
        clear_mcp_sessions_async(self.mcp_sessions.clone());
        clear_mcp_http_sessions_async(self.mcp_http_sessions.clone());
        clear_mcp_sse_sessions_async(self.mcp_sse_sessions.clone());
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

    pub fn set_subagent_enabled_override(&mut self, enabled: Option<bool>) {
        self.subagent_enabled_override = enabled;
    }

    fn is_subagent_tool_name(name: &str) -> bool {
        matches!(
            name,
            "spawn_agent"
                | "wait_agent"
                | "send_input"
                | "resume_agent"
                | "list_agents"
                | "close_agent"
        )
    }

    fn subagent_tools_enabled(&self) -> bool {
        self.subagent_enabled_override.unwrap_or(false)
    }

    fn subagent_tools_allowed(&self, name: &str) -> bool {
        !Self::is_subagent_tool_name(name) || self.subagent_tools_enabled()
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
                | "smartbrain_ssh_exec"
                | "browser_run"
                // Optional web tools are only present in `tool_specs` when enabled.
                | "web_search"
                | "web_fetch"
                // Install/manage MCP servers and local skills so Settings UI can show them.
                | "mcp_manage"
                | "skill_manage"
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
        clear_mcp_sse_sessions_async(self.mcp_sse_sessions.clone());
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

    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> AppResult<String> {
        // Dialog-level gate: subagent tools must not run when the composer switch is off,
        // even if an older tool_search activation still lists them for the thread.
        if Self::is_subagent_tool_name(tool_name) && !self.subagent_tools_enabled() {
            let msg = "Subagents are disabled for this chat. Enable 子智能体 in the composer to use spawn_agent / wait_agent / send_input / resume_agent / list_agents / close_agent.".to_string();
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, "disabled");
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

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
            "mcp_manage" => {
                self.exec_mcp_manage(arguments, call_id, app_handle, thread_id)
                    .await
            }
            "skill_manage" => {
                self.exec_skill_manage(arguments, call_id, app_handle, thread_id)
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
            "smartbrain_ssh_exec" => {
                self.exec_smartbrain_ssh_exec(arguments, call_id, app_handle, thread_id)
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

    fn emit_subagent_status(app_handle: &AppHandle, thread_id: &str, record: &SubagentRecord) {
        let payload = serde_json::json!({
            "threadId": thread_id,
            "id": record.id,
            "role": record.role,
            "status": record.status,
            "prompt": record.prompt,
            "durationMs": record.duration_ms,
            "output": record.output,
            "error": record.error,
            "updatedAt": now_millis(),
        });
        app_handle.emit("subagent-status", payload.clone()).ok();
        crate::mobile_server::broadcast("subagent-status", payload);
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

    fn memories_dir(&self) -> PathBuf {
        self.workspace_config_dir.join("memories")
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

pub(crate) enum WaitChildResult {
    Exited(std::process::ExitStatus),
    TimedOut,
    Failed(String),
}

const MAX_SKILL_MANAGE_CONTENT_CHARS: usize = 400_000;

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageInfo {
    format: &'static str,
    mime: &'static str,
    width: u32,
    height: u32,
}

const POWERSHELL_UTF8_PREFIX_MARKER: &str = "# cn-codex-utf8";
const POWERSHELL_UTF8_OUTPUT_PREFIX: &str = "# cn-codex-utf8\n\
$utf8NoBom = [System.Text.UTF8Encoding]::new($false);\n\
[Console]::InputEncoding = $utf8NoBom;\n\
[Console]::OutputEncoding = $utf8NoBom;\n\
$OutputEncoding = $utf8NoBom;\n\
$PSDefaultParameterValues['*:Encoding'] = 'utf8';\n";

#[derive(Debug, Deserialize, Default)]
pub(crate) struct DuckDuckGoResponse {
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
pub(crate) struct DuckDuckGoTopic {
    #[serde(default, rename = "Text")]
    text: String,
    #[serde(default, rename = "FirstURL")]
    first_url: String,
    #[serde(default, rename = "Topics")]
    topics: Vec<DuckDuckGoTopic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebSearchProvider {
    BingBrowser,
    DuckDuckGoApi,
    DuckDuckGoBrowser,
}

const SMARTBRAIN_CONTEXT_OVERLAP_LINES: usize = 30;
const SMARTBRAIN_CONTEXT_RESULT_LIMIT: usize = 3;

// Windows-1252 在 0x80-0x9F 区间定义了“智能引号/破折号”等符号。
// 在某些短输出中，chardetng 可能把这段字节误判成 IBM866（会显示成西里尔字符），
// 因此这里保留与 Codex 同步的最小兜底逻辑：仅在“ASCII + 该符号字节”形态下强制回退 CP1252。
const WINDOWS_1252_PUNCT_BYTES: [u8; 8] = [0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x99];


#[cfg(test)]
#[path = "tool_executor_tests.rs"]
mod tests;
