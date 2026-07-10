use std::collections::{BTreeMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

mod plan_support;
mod prompt_context;
mod protocol_support;

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

use crate::adapter::{
    self,
    types::{
        InternalFunctionCall, InternalMessage, InternalToolCall, StreamEvent, UsageInfo,
        text_content,
    },
};

/// emit 到前端 + 同时广播到移动端 WebSocket
fn emit_and_broadcast(app_handle: &AppHandle, event: &str, payload: serde_json::Value) {
    app_handle.emit(event, payload.clone()).ok();
    crate::mobile_server::broadcast(event, payload);
}

/// 按 UTF-8 字符边界截断字符串，避免按字节切片导致 panic。
///
/// 说明：
/// - `max_bytes` 代表“最多保留多少字节”；
/// - 若该字节位置落在多字节字符中间（例如中文），会向前回退到最近合法边界；
/// - 仅用于日志/提示词截断，不改变原始字符串内容。
pub(crate) fn truncate_utf8_by_bytes(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
use crate::config_system::{ConfigToml, SmartBrainConfig};
use crate::error::{AppError, AppResult};
use crate::hook_runtime::{
    HOOK_AGENT_END, HOOK_AGENT_START, HOOK_COMMAND_EXEC, HOOK_FILE_CHANGE, HOOK_POST_TOOL_USE,
    HOOK_SUBAGENT_STOP, HOOK_USER_PROMPT_SUBMIT, HookRunResult, HookRuntime,
    first_blocking_hook_result, hook_feedback_for_model, latest_hook_updated_input,
};
use crate::ocr::{OcrImageInput, extract_text_from_data_urls};
use crate::plugin_loader;
use crate::robot_orchestrator::{
    NodeProgressResult, RobotOrchestrator, build_robot_model_history,
    build_robot_node_completion_nudge, parse_robot_node_completion,
    should_enable_robot_orchestration,
};
use crate::thread_store::{
    FileChange, ThreadGoal, ThreadGoalStatus, ThreadMessage, ThreadMessageAttachment,
    ThreadRobotState, ThreadStore, ToolCallInfo, TurnUsage,
};
use crate::tool_executor::ToolExecutor;
use crate::usage::UsageRecorder;
use plan_support::{
    build_active_plan_context_prompt, plan_contents_equivalent, read_plan_file_content,
    resolve_effective_plan_content, user_requested_new_plan_file,
};
use prompt_context::{render_robot_runtime_prompt, render_smartbrain_runtime_prompt};
use protocol_support::{
    ProtocolStreamState, ToolCallAccumulator, consume_protocol_text_delta,
    flush_protocol_stream_state, parse_dsml_tool_calls_block, parse_protocol_text,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

struct GoalUpdateOutcome {
    message: String,
    goal: ThreadGoal,
}

enum RobotGoalCompletionOutcome {
    Advanced(ThreadRobotState),
    Completed,
}

fn extract_update_goal_status(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("status")?
        .as_str()
        .map(|status| status.trim().to_string())
        .filter(|status| !status.is_empty())
}

async fn advance_robot_workflow_from_goal_completion(
    thread_store: &ThreadStore,
    robot_orchestrator: &RobotOrchestrator,
    thread_id: &str,
    progress_state: ThreadRobotState,
) -> Result<RobotGoalCompletionOutcome, String> {
    match robot_orchestrator
        .apply_node_progress(thread_store, thread_id, progress_state, true, None)
        .await
        .map_err(|e| e.to_string())?
    {
        NodeProgressResult::ContinueCurrent { .. } => Err(
            "robot workflow refused to advance after update_goal marked the node complete"
                .to_string(),
        ),
        NodeProgressResult::Advanced { state, nudge } => {
            let boundary_id = uuid::Uuid::new_v4().to_string();
            let mut advanced_state = state;
            advanced_state.current_node_start_message_id = Some(boundary_id.clone());
            thread_store
                .set_thread_robot_state(thread_id, advanced_state.clone())
                .await
                .map_err(|e| e.to_string())?;
            thread_store
                .add_message(
                    thread_id,
                    ThreadMessage {
                        id: boundary_id,
                        role: "system".to_string(),
                        content: nudge,
                        timestamp: now_secs(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        attachments: Vec::new(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(RobotGoalCompletionOutcome::Advanced(advanced_state))
        }
        NodeProgressResult::Completed => Ok(RobotGoalCompletionOutcome::Completed),
    }
}

/// 记录单个文件在当前 turn 内的“修改前/修改后”文本快照。
///
/// 说明：
/// - 用于前端 RunSummary Diff 视图在非 apply_patch 场景下也能生成可读对比；
/// - 仅采集文本内容，二进制文件或读取失败时保持 None；
/// - 字段命名使用 camelCase 以便直接透传给前端事件 payload。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileChangeSnapshot {
    pub path: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_content: Option<String>,
}

/// 单文件快照读取上限（256KB）。
///
/// 说明：
/// - 目的是避免 turn-completed 事件携带过大文本导致前端卡顿；
/// - 对超限文件只截取前缀内容用于“审阅级对比”，而非完整文件恢复。
const MAX_CHANGED_FILE_SNAPSHOT_BYTES: usize = 256 * 1024;
const VISION_FALLBACK_KIND_MULTIMODAL: &str = "multimodal";
const VISION_FALLBACK_KIND_LOCAL_OCR: &str = "local_ocr";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserAttachment {
    pub name: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    pub data_url: String,
    pub size: u64,
}

pub struct AgentEngine {
    http: reqwest::Client,
    thread_store: Arc<ThreadStore>,
    tool_executor: Arc<RwLock<ToolExecutor>>,
    cwd: PathBuf,
    usage_recorder: Option<Arc<UsageRecorder>>,
    conversation_logger: Option<Arc<crate::conversation_logger::ConversationLogger>>,
    cancel_flag: Arc<AtomicBool>,
}

impl AgentEngine {
    pub fn new(
        thread_store: Arc<ThreadStore>,
        tool_executor: ToolExecutor,
        cwd: PathBuf,
    ) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| AppError::Custom(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            http,
            thread_store,
            tool_executor: Arc::new(RwLock::new(tool_executor)),
            cwd,
            usage_recorder: None,
            conversation_logger: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 设置用量记录器
    pub fn set_usage_recorder(&mut self, recorder: Arc<UsageRecorder>) {
        self.usage_recorder = Some(recorder);
    }

    /// 设置对话轨迹日志器
    pub fn set_conversation_logger(
        &mut self,
        logger: Arc<crate::conversation_logger::ConversationLogger>,
    ) {
        self.conversation_logger = Some(logger);
    }

    /// 中断当前正在运行的 turn
    pub fn interrupt(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// 中断指定线程（或全部线程）的活跃工具子进程。
    ///
    /// 说明：
    /// - 停止按钮先设置 cancel flag，再调用此方法；
    /// - 该方法只负责“进程级中断”，不修改线程消息本身；
    /// - 返回命中的进程数量，供上层记录日志和验证。
    pub async fn interrupt_active_tools(&self, thread_id: Option<&str>) -> usize {
        let executor = self.tool_executor.read().await;
        match thread_id {
            Some(id) => executor.interrupt_active_tools(id).await,
            None => executor.interrupt_all_active_tools().await,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    pub async fn run_turn(
        &self,
        app_handle: &AppHandle,
        config: &ConfigToml,
        thread_id: &str,
        user_input: &str,
        attachments: Vec<UserAttachment>,
        override_cwd: Option<&Path>,
        mode: Option<&str>,
        goal_budget_tokens: Option<u64>,
        robot_id: Option<&str>,
    ) -> AppResult<()> {
        self.cancel_flag.store(false, Ordering::SeqCst);

        if user_input.trim() == "/compact" {
            let (provider_id, provider) = config.resolve_provider();
            let model = config.resolve_model();
            let base_url = provider.resolve_base_url().ok_or_else(|| {
                AppError::Custom(format!(
                    "No base URL for provider '{provider_id}'. Configure it in settings."
                ))
            })?;
            let api_key = provider.resolve_api_key().unwrap_or_default();
            let wire_api = provider.wire_api.as_deref().unwrap_or("chat").to_string();

            crate::compaction::run_compaction(
                &self.http,
                app_handle,
                config,
                &self.thread_store,
                thread_id,
                &base_url,
                &api_key,
                &model,
                &wire_api,
                Some(&self.cancel_flag),
                provider.query_params.as_ref(),
                provider.http_headers.as_ref(),
            )
            .await?;
            return Ok(());
        }

        let turn_mode = match mode {
            Some("goal") => "goal",
            Some("plan") => "plan",
            Some("robot-create") => "robot-create",
            Some("robot-modify") => "robot-modify",
            _ => "chat",
        }
        .to_string();
        let turn_started_at_ms = now_millis();
        let turn_timer = Instant::now();
        let mut model = config.resolve_model();
        if model.is_empty() {
            return Err(AppError::Custom(
                "未配置模型。请在设置中选择一个模型后再试。".to_string(),
            ));
        }
        let pool_default_model = model.clone();
        let (provider_id, provider) = config.resolve_provider();
        let effective_cwd = override_cwd
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone());
        let git_status_before = git_status_snapshot(&effective_cwd).await;
        let mut changed_files: Vec<FileChange> = Vec::new();
        // 本轮文件快照缓存：
        // - key 为规范化后的文件路径；
        // - before 在工具执行前采集一次；
        // - after 在工具成功后更新为最新状态。
        let mut changed_file_snapshot_map: BTreeMap<String, FileChangeSnapshot> = BTreeMap::new();
        let mut turn_usage = TurnUsage::default();
        // 统计“本轮成功模型调用次数”：
        // - 每次 stream_completion 返回 Ok（无论是 Message 还是 ToolCalls）计 1 次；
        // - 失败重试不计入；
        // - 最终写入 turn usage 的 call_count 字段供前端展示。
        let mut llm_call_count: u32 = 0;
        let goal_budget_tokens = if turn_mode == "goal" {
            goal_budget_tokens.filter(|value| *value > 0)
        } else {
            None
        };

        // 端点池支持：当 model_endpoints 非空时，用端点池覆盖 base_url
        let pool_endpoints = &config.model_endpoints;
        let is_pool = !pool_endpoints.is_empty();
        let pool_resolver = if is_pool {
            let initial_idx = config.active_endpoint_index.unwrap_or(0);
            Some(crate::local_pool::PoolResolver::with_initial_index(
                initial_idx,
            ))
        } else {
            None
        };
        let pool_key = if is_pool {
            Some(format!("{provider_id}:{pool_default_model}"))
        } else {
            None
        };

        let provider_wire_api = provider.wire_api.as_deref().unwrap_or("chat").to_string();

        let (mut base_url, mut api_key, mut wire_api, mut pool_endpoint_index) = if is_pool {
            let pk = pool_key.as_deref().unwrap_or("");
            let pr = pool_resolver.as_ref().unwrap();
            let ep = pr.resolve_endpoint(pk, pool_endpoints).ok_or_else(|| {
                AppError::Custom("资源池没有可用端点，请添加至少一个端点。".to_string())
            })?;
            info!(
                "Pool '{pk}': using endpoint {} ({})",
                ep.endpoint_index, ep.url
            );
            emit_and_broadcast(
                app_handle,
                "active-endpoint-index",
                serde_json::json!({ "index": ep.endpoint_index }),
            );
            let ep_api_key = ep.api_key.clone().unwrap_or_default();
            let ep_wire_api = ep
                .wire_api
                .clone()
                .unwrap_or_else(|| provider_wire_api.clone());
            if let Some(ep_model) = ep
                .model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                model = ep_model.to_string();
            } else {
                model = pool_default_model.clone();
            }
            (ep.url, ep_api_key, ep_wire_api, Some(ep.endpoint_index))
        } else {
            let url = provider.resolve_base_url().ok_or_else(|| {
                AppError::Custom(format!(
                    "No base URL for provider '{provider_id}'. Configure it in settings."
                ))
            })?;
            let key = provider.resolve_api_key().unwrap_or_default();
            (url, key, provider_wire_api.clone(), None)
        };
        let persisted_image_attachments: Vec<ThreadMessageAttachment> = attachments
            .iter()
            .filter(|attachment| attachment.mime_type.starts_with("image/"))
            .map(|attachment| ThreadMessageAttachment {
                name: attachment.name.clone(),
                mime_type: attachment.mime_type.clone(),
                data_url: attachment.data_url.clone(),
                size: attachment.size,
            })
            .collect();
        let (attachments, vision_fallback_context) = self
            .resolve_image_context_with_fallback(config, user_input, &model, &attachments)
            .await;

        // Pre-turn compaction: 在 start_turn 之前执行，避免 replace_messages 破坏当前 turn
        let pre_turn_tokens = self.thread_store.get_thread_total_tokens(thread_id).await;
        if crate::compaction::should_compact(pre_turn_tokens, config) {
            info!("Pre-turn compaction triggered: {pre_turn_tokens} tokens");
            let _ = crate::compaction::run_compaction(
                &self.http,
                app_handle,
                config,
                &self.thread_store,
                thread_id,
                &base_url,
                &api_key,
                &model,
                &wire_api,
                Some(&self.cancel_flag),
                provider.query_params.as_ref(),
                provider.http_headers.as_ref(),
            )
            .await;
            if self.is_cancelled() {
                return Ok(());
            }
        }

        if turn_mode == "goal" {
            let existing_goal = self
                .thread_store
                .get_thread(thread_id)
                .await
                .and_then(|t| t.goal);
            if existing_goal
                .as_ref()
                .is_some_and(|g| g.status != ThreadGoalStatus::Active)
            {
                // Resume existing paused/blocked goal instead of creating a new one
                self.thread_store
                    .set_thread_goal_status(thread_id, ThreadGoalStatus::Active)
                    .await?;
            } else if existing_goal.is_none() {
                self.thread_store
                    .set_thread_goal(
                        thread_id,
                        user_input.to_string(),
                        ThreadGoalStatus::Active,
                        goal_budget_tokens,
                    )
                    .await?;
            }
        }
        let turn_id = self
            .thread_store
            .start_turn(thread_id, Some(turn_mode.clone()), goal_budget_tokens)
            .await?;

        let user_message_id = uuid::Uuid::new_v4().to_string();
        // 将文档附件提取文本，以及视觉后补结果，直接嵌入到用户消息中做持久化。
        let mut persisted_content = user_input.to_string();
        for attachment in &attachments {
            if !attachment.mime_type.starts_with("image/") {
                match crate::document_parser::parse_document(
                    &attachment.mime_type,
                    &attachment.data_url,
                ) {
                    Ok(extracted) => {
                        persisted_content.push_str(&format!(
                            "\n\n[Attachment: {}]\n{}",
                            attachment.name, extracted
                        ));
                    }
                    Err(e) => {
                        warn!("Document parse failed for {}: {e}", attachment.name);
                        persisted_content.push_str(&format!(
                            "\n\n[Attachment: {} - content extraction failed]",
                            attachment.name
                        ));
                    }
                }
            }
        }
        if let Some(fallback_context) = vision_fallback_context.as_deref() {
            persisted_content.push_str("\n\n[Vision Fallback Context]\n");
            persisted_content.push_str(fallback_context);
        }
        let user_msg = ThreadMessage {
            id: user_message_id.clone(),
            role: "user".to_string(),
            content: persisted_content,
            timestamp: now_secs(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: persisted_image_attachments,
        };
        self.thread_store.add_message(thread_id, user_msg).await?;

        let goal_snapshot = if turn_mode == "goal" {
            self.thread_store
                .get_thread(thread_id)
                .await
                .and_then(|t| t.goal)
        } else {
            None
        };
        let mut turn_started_payload = serde_json::json!({
            "threadId": thread_id,
            "turn": {
                "id": &turn_id,
                "mode": &turn_mode,
                "startedAt": turn_started_at_ms,
            }
        });
        if let Some(ref goal) = goal_snapshot {
            turn_started_payload["goal"] = serde_json::json!(goal);
        }
        emit_and_broadcast(app_handle, "turn-started", turn_started_payload);

        let hook_runtime = HookRuntime::load(config, &self.cwd.join("codey"));
        if let Some(cwd) = override_cwd {
            self.tool_executor.write().await.set_cwd(cwd.to_path_buf());
        }
        let mut mcp_servers = plugin_loader::list_plugin_mcp_servers(&self.cwd.join("codey"));
        for (name, server) in config.resolved_mcp_servers() {
            mcp_servers.insert(name, server);
        }
        self.tool_executor
            .write()
            .await
            .set_mcp_servers(mcp_servers);

        // Set provider config for internal subagents
        {
            let system_prompt_prefix =
                self.build_system_prompt(config, &effective_cwd, "chat", None);
            self.tool_executor
                .read()
                .await
                .set_subagent_provider_config(crate::tool_executor::SubagentProviderConfig {
                    base_url: base_url.clone(),
                    api_key: api_key.clone(),
                    model: model.clone(),
                    wire_api: wire_api.clone(),
                    system_prompt_prefix,
                    max_output_tokens: config.max_output_tokens,
                })
                .await;
        }

        // 机器人外层编排入口（低耦合）：
        // - 仅 mode=goal 且携带 robot_id 时启用；
        // - 编排细节下沉到 robot_orchestrator，agent 仅处理“启用判断 + 结果接线”；
        // - 非机器人路径保持原有 goal/chat 行为不变。
        let robot_orchestrator = RobotOrchestrator::new(&self.cwd);
        let robot_execution_enabled = should_enable_robot_orchestration(&turn_mode, robot_id);
        let mut robot_progress: Option<ThreadRobotState> = None;
        let existing_robot_state = self.thread_store.get_thread_robot_state(thread_id).await;

        if robot_execution_enabled {
            let rid = robot_id.unwrap_or_default();
            let prepared_state = robot_orchestrator
                .prepare_state(&self.thread_store, thread_id, rid, user_input)
                .await?;
            robot_progress = Some(prepared_state);
        } else if existing_robot_state.is_some() {
            let _ = self.thread_store.clear_thread_robot_state(thread_id).await;
        }

        let mut stop_hooks_satisfied = false;
        let mut stop_hooks_ran_for_last_stop = false;
        let mut stop_hook_continuations = 0usize;
        let mut goal_completed_now = false;
        let prompt_hook_results = hook_runtime
            .run_event(
                app_handle,
                thread_id,
                HOOK_USER_PROMPT_SUBMIT,
                &effective_cwd,
                user_prompt_submit_hook_context(
                    &turn_id,
                    &turn_mode,
                    &effective_cwd,
                    &model,
                    user_input,
                ),
            )
            .await;
        let prompt_hook_feedback = hook_feedback_for_model(&prompt_hook_results);
        let prompt_hook_blocked = prompt_hook_results
            .iter()
            .any(|result| matches!(result.decision.as_deref(), Some("block" | "stop")));
        if !prompt_hook_feedback.is_empty() {
            let msg = ThreadMessage {
                id: uuid::Uuid::new_v4().to_string(),
                role: if prompt_hook_blocked {
                    "assistant".to_string()
                } else {
                    "system".to_string()
                },
                content: format_user_prompt_submit_hook_feedback(
                    prompt_hook_feedback,
                    prompt_hook_blocked,
                ),
                timestamp: now_secs(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                attachments: Vec::new(),
            };
            self.thread_store.add_message(thread_id, msg).await?;
        }
        if prompt_hook_blocked {
            stop_hooks_satisfied = true;
        } else {
            hook_runtime
                .run_event(
                    app_handle,
                    thread_id,
                    HOOK_AGENT_START,
                    &effective_cwd,
                    serde_json::json!({
                        "turnId": &turn_id,
                        "mode": &turn_mode,
                        "cwd": &effective_cwd,
                        "userInput": user_input,
                        "goalBudgetTokens": goal_budget_tokens,
                        "attachments": attachments.iter().map(|attachment| {
                            serde_json::json!({
                                "name": &attachment.name,
                                "type": &attachment.mime_type,
                                "size": attachment.size,
                            })
                        }).collect::<Vec<_>>(),
                    }),
                )
                .await;
        }

        // SmartBrain pre-recall: automatically retrieve relevant knowledge before first model call
        if !prompt_hook_blocked && config.smartbrain_config().is_active() {
            let bm25_path = crate::smartbrain::bm25_index_path(&self.cwd.join("codey"));
            if bm25_path.exists() {
                let results = crate::smartbrain::search::unified_search(&bm25_path, user_input, 3);
                if !results.is_empty() {
                    let memories_dir = self.cwd.join("codey").join("memories");
                    let mut context_parts = Vec::new();
                    for r in &results {
                        if let Some(recall_text) =
                            build_smartbrain_recall_context(&memories_dir, r, 30, 2000)
                        {
                            context_parts.push(format!(
                                "### {} (score: {:.2})\n{}",
                                r.title, r.score, recall_text
                            ));
                        }
                    }
                    if !context_parts.is_empty() {
                        let sb_context = format!(
                            "## SmartBrain Knowledge Recall\n\
                             The following knowledge was automatically retrieved from SmartBrain and may be relevant:\n\n{}",
                            context_parts.join("\n\n---\n\n")
                        );
                        let sb_msg = ThreadMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            role: "system".to_string(),
                            content: sb_context,
                            timestamp: now_secs(),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                            attachments: Vec::new(),
                        };
                        self.thread_store.add_message(thread_id, sb_msg).await?;
                    }
                }
            }
        }

        let mut intent_retries: u32 = 0;
        const MAX_INTENT_RETRIES: u32 = 2;
        const MAX_RATE_LIMIT_RETRIES: u32 = 6;
        let max_goal_continuations: usize = 10;
        // 使用固定且可解释的上下文窗口来源，避免前端分母与后端运行时配置漂移。
        let model_context_window_tokens = resolve_model_context_window_tokens(config);
        let mut goal_continuation_count: usize = 0;
        // 追踪最近一次 API 调用返回的 prompt_tokens（代表当前 context 实际大小），
        // 而非累加值，用于 mid-turn compaction 判断。
        let mut last_prompt_tokens: u64 = 0;
        let mut mid_turn_compacted = false;
        let mut rate_limit_retry_count: u32 = 0;
        let mut terminated_by_error = false;
        let mut force_new_plan_on_next_emit =
            turn_mode == "plan" && user_requested_new_plan_file(user_input);
        let mut active_plan_path: Option<String> = None;
        let mut active_plan_revision: u64 = 0;
        let mut active_plan_content: Option<String> = None;
        if turn_mode == "plan" {
            if let Some(active_plan) = self.thread_store.get_thread_active_plan(thread_id).await {
                active_plan_revision = active_plan.revision.max(1);
                active_plan_content = read_plan_file_content(&active_plan.path, &self.cwd);
                active_plan_path = Some(active_plan.path);
            }
        }

        'goal_loop: loop {
            if !prompt_hook_blocked {
                // Goal continuation 时检查是否需要 compaction（首次 compaction 已在 start_turn 之前完成）
                if goal_continuation_count > 0 {
                    let pre_turn_tokens =
                        self.thread_store.get_thread_total_tokens(thread_id).await;
                    if crate::compaction::should_compact(pre_turn_tokens, config) {
                        info!("Goal continuation compaction triggered: {pre_turn_tokens} tokens");
                        let _ = crate::compaction::run_compaction(
                            &self.http,
                            app_handle,
                            config,
                            &self.thread_store,
                            thread_id,
                            &base_url,
                            &api_key,
                            &model,
                            &wire_api,
                            Some(&self.cancel_flag),
                            provider.query_params.as_ref(),
                            provider.http_headers.as_ref(),
                        )
                        .await;
                        if self.is_cancelled() {
                            break;
                        }
                    }
                }

                let mut iteration: u32 = 0;
                loop {
                    if self.is_cancelled() {
                        info!("Turn {turn_id} cancelled by user at iteration {iteration}");
                        break;
                    }
                    info!("Agent loop iteration {iteration} for turn {turn_id}");
                    iteration += 1;

                    let history = self.thread_store.get_thread_messages(thread_id).await;
                    // 机器人编排分支：把历史裁剪为“原始用户目标 + 当前节点自身消息”，
                    // 已完成上游节点的原始杂乱历史由 node_deliveries 以总结形式替代，保持上下文纯净。
                    let model_history = if let Some(state) = robot_progress.as_ref() {
                        build_robot_model_history(&history, state)
                    } else {
                        history
                    };
                    let robot_overlay_prompt = if let Some(state) = robot_progress.as_ref() {
                        Some(robot_orchestrator.build_overlay_prompt(state)?)
                    } else {
                        None
                    };
                    let active_plan_context = if turn_mode == "plan" {
                        if active_plan_content.is_none() {
                            if let Some(path) = active_plan_path.as_deref() {
                                active_plan_content = read_plan_file_content(path, &self.cwd);
                            }
                        }
                        match (active_plan_path.as_deref(), active_plan_content.as_deref()) {
                            (Some(path), Some(content)) => {
                                Some((path, active_plan_revision.max(1), content))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let internal_messages = self.build_internal_messages(
                        config,
                        &model_history,
                        &effective_cwd,
                        &turn_mode,
                        robot_id,
                        Some(user_message_id.as_str()),
                        &attachments,
                        robot_overlay_prompt.as_deref(),
                        active_plan_context,
                    );
                    let tools = if turn_mode == "robot-create" || turn_mode == "robot-modify" {
                        self.tool_executor
                            .write()
                            .await
                            .tool_specs(false)
                            .into_iter()
                            .filter(|spec| {
                                spec.pointer("/function/name").and_then(|v| v.as_str())
                                    == Some("robot_save")
                            })
                            .collect()
                    } else if turn_mode == "plan" {
                        const PLAN_READONLY_TOOLS: &[&str] = &[
                            "read_file",
                            "list_directory",
                            "tool_search",
                            "code_review",
                            "memory_list",
                            "memory_read",
                            "memory_search",
                            "smartbrain_search",
                            "view_image",
                            "ocr_image",
                            "web_search",
                            "web_fetch",
                        ];
                        self.tool_executor
                            .write()
                            .await
                            .tool_specs_with_mcp(config.web_search_enabled())
                            .await
                            .into_iter()
                            .filter(|spec| {
                                spec.pointer("/function/name")
                                    .and_then(|v| v.as_str())
                                    .is_some_and(|name| PLAN_READONLY_TOOLS.contains(&name))
                            })
                            .collect()
                    } else {
                        self.tool_executor
                            .write()
                            .await
                            .tool_specs_with_mcp(config.web_search_enabled())
                            .await
                    };

                    let mut tools = tools;
                    if turn_mode == "goal" {
                        tools.push(serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": "update_goal",
                                "description": "Update the current goal status. Call with status 'complete' only when the objective is fully achieved and no required work remains. Call with status 'blocked' only when the same blocking condition has recurred for at least three consecutive goal turns and you cannot make meaningful progress without user input.",
                                "parameters": {
                                    "type": "object",
                                    "properties": {
                                        "status": {
                                            "type": "string",
                                            "enum": ["complete", "blocked"],
                                            "description": "Set to 'complete' when objective is achieved. Set to 'blocked' when truly stuck after 3+ consecutive turns."
                                        }
                                    },
                                    "required": ["status"]
                                }
                            }
                        }));
                    }

                    let result = self
                        .stream_completion(
                            app_handle,
                            thread_id,
                            &base_url,
                            &api_key,
                            &model,
                            &wire_api,
                            internal_messages,
                            if tools.is_empty() { None } else { Some(tools) },
                            config.max_output_tokens,
                            iteration,
                            turn_mode == "plan",
                            provider.query_params.as_ref(),
                            provider.http_headers.as_ref(),
                        )
                        .await;

                    match result {
                        Ok(CompletionResult::Message {
                            ref text,
                            ref usage,
                            ref plan_text,
                        }) => {
                            rate_limit_retry_count = 0;
                            llm_call_count = llm_call_count.saturating_add(1);
                            info!(
                                "Iteration {iteration}: Message ({} chars), usage={:?}",
                                text.len(),
                                usage
                            );
                            if let Some(u) = usage {
                                add_turn_usage(&mut turn_usage, u);
                                last_prompt_tokens = u.prompt_tokens;
                                turn_usage.call_count = llm_call_count;
                                turn_usage.last_single_prompt_tokens = last_prompt_tokens;
                                emit_turn_usage_updated(
                                    app_handle,
                                    thread_id,
                                    &turn_usage,
                                    model_context_window_tokens,
                                );
                                if let Some(ref recorder) = self.usage_recorder {
                                    recorder.record(&provider_id, &model, thread_id, u);
                                }
                            }
                            let (cleaned_text, node_done_signal, node_delivery_summary) =
                                if robot_progress.is_some() {
                                    let (cleaned, done, summary) =
                                        parse_robot_node_completion(text);
                                    (cleaned, done, summary)
                                } else {
                                    (text.clone(), false, None)
                                };

                            if cleaned_text.is_empty() && iteration > 0 {
                                info!("Empty message after tool execution, sending minimal signal");
                                emit_and_broadcast(
                                    app_handle,
                                    "agent-message-delta",
                                    serde_json::json!({ "threadId": thread_id, "delta": "(completed)" }),
                                );
                            }

                            if turn_mode != "plan"
                                && !cleaned_text.is_empty()
                                && iteration > 0
                                && intent_retries < MAX_INTENT_RETRIES
                                && text_expresses_intent(&cleaned_text)
                            {
                                intent_retries += 1;
                                info!(
                                    "Intent detected in text without tool calls, retry {intent_retries}/{MAX_INTENT_RETRIES}"
                                );
                                let nudge_msg = ThreadMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: "system".to_string(),
                                    content:
                                        "You expressed intent to perform an action but did not \
                                          call any tools. Please call the appropriate tool(s) now \
                                          instead of describing what you plan to do."
                                            .to_string(),
                                    timestamp: now_secs(),
                                    tool_call_id: None,
                                    tool_name: None,
                                    tool_calls: None,
                                    attachments: Vec::new(),
                                };
                                self.thread_store.add_message(thread_id, nudge_msg).await?;
                                continue;
                            }

                            let content = if cleaned_text.is_empty() && iteration > 0 {
                                String::new()
                            } else {
                                cleaned_text.clone()
                            };
                            if !content.is_empty() || iteration == 0 {
                                let msg = ThreadMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: "assistant".to_string(),
                                    content: content.clone(),
                                    timestamp: now_secs(),
                                    tool_call_id: None,
                                    tool_name: None,
                                    tool_calls: None,
                                    attachments: Vec::new(),
                                };
                                self.thread_store.add_message(thread_id, msg).await?;
                            }

                            let effective_plan = resolve_effective_plan_content(
                                &turn_mode,
                                plan_text.as_deref(),
                                text,
                                &cleaned_text,
                            );
                            if let Some(ref plan_content) = effective_plan {
                                let unchanged_active_plan = turn_mode == "plan"
                                    && active_plan_content.as_deref().is_some_and(|current| {
                                        plan_contents_equivalent(current, plan_content)
                                    });
                                if unchanged_active_plan {
                                    info!(
                                        "Skipping plan update for thread {thread_id} because content is unchanged"
                                    );
                                } else {
                                    let reuse_existing_plan = turn_mode == "plan"
                                        && !force_new_plan_on_next_emit
                                        && active_plan_path.is_some();
                                    let (plan_path, plan_updated, next_revision) =
                                        if reuse_existing_plan {
                                            let existing_path = resolve_plan_storage_path(
                                                active_plan_path.as_deref().unwrap_or_default(),
                                                &self.cwd,
                                            );
                                            (
                                                existing_path,
                                                true,
                                                active_plan_revision.saturating_add(1).max(1),
                                            )
                                        } else {
                                            force_new_plan_on_next_emit = false;
                                            let plans_dir = self.cwd.join("codey").join("plans");
                                            let _ = std::fs::create_dir_all(&plans_dir);
                                            let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                                            let short_hash = &uuid::Uuid::new_v4().to_string()[..8];
                                            let file_name = format!("{ts}-{short_hash}.pmd");
                                            (plans_dir.join(&file_name), false, 1_u64)
                                        };
                                    if let Some(parent) = plan_path.parent() {
                                        let _ = std::fs::create_dir_all(parent);
                                    }
                                    if let Err(err) = std::fs::write(&plan_path, plan_content) {
                                        warn!(
                                            "Failed to write plan file {}: {err}",
                                            plan_path.display()
                                        );
                                    } else {
                                        let plan_path_string =
                                            plan_path.to_string_lossy().to_string();
                                        if turn_mode == "plan" {
                                            active_plan_path = Some(plan_path_string.clone());
                                            active_plan_revision = next_revision;
                                            active_plan_content = Some(plan_content.clone());
                                            if let Err(err) = self
                                                .thread_store
                                                .set_thread_active_plan(
                                                    thread_id,
                                                    plan_path_string.clone(),
                                                    next_revision,
                                                )
                                                .await
                                            {
                                                warn!(
                                                    "Failed to persist active plan metadata for thread {thread_id}: {err}"
                                                );
                                            }
                                        }

                                        let revision_payload = if turn_mode == "plan" {
                                            serde_json::Value::from(next_revision)
                                        } else {
                                            serde_json::Value::Null
                                        };
                                        info!(
                                            "Plan file written: {}, updated={}, revision={}",
                                            plan_path.display(),
                                            plan_updated,
                                            next_revision
                                        );
                                        emit_and_broadcast(
                                            app_handle,
                                            "plan-generated",
                                            serde_json::json!({
                                                "threadId": thread_id,
                                                "path": plan_path_string,
                                                "content": plan_content,
                                                "updated": plan_updated,
                                                "revision": revision_payload,
                                            }),
                                        );
                                    }
                                }
                            }

                            let waiting_for_user_requirements = robot_progress.is_some()
                                && !node_done_signal
                                && assistant_is_waiting_for_user(&cleaned_text);
                            if waiting_for_user_requirements {
                                // 倒计时等待：给用户 30 秒补充信息，超时后自动按已有信息继续。
                                const ROBOT_WAIT_TIMEOUT_MS: u64 = 30_000;
                                let wait_call_id = format!("robot-wait-{}", uuid::Uuid::new_v4());
                                info!(
                                    "Robot workflow waiting for user input (timeout={ROBOT_WAIT_TIMEOUT_MS}ms, callId={wait_call_id})"
                                );

                                // 发送等待事件，让前端展示倒计时 UI
                                emit_and_broadcast(
                                    app_handle,
                                    "robot-waiting-for-input",
                                    serde_json::json!({
                                        "threadId": thread_id,
                                        "callId": &wait_call_id,
                                        "countdownMs": ROBOT_WAIT_TIMEOUT_MS,
                                        "assistantText": &cleaned_text,
                                    }),
                                );

                                // 通过 approval 通道等待用户回复或超时
                                let request_id =
                                    crate::protocol::RequestId::String(wait_call_id.clone());
                                app_handle
                                    .emit(
                                        "server-request",
                                        serde_json::json!({
                                            "requestId": &wait_call_id,
                                            "id": &wait_call_id,
                                            "method": "robot_waiting_for_input",
                                            "params": {
                                                "threadId": thread_id,
                                                "callId": &wait_call_id,
                                                "countdownMs": ROBOT_WAIT_TIMEOUT_MS,
                                                "assistantText": &cleaned_text,
                                            },
                                        }),
                                    )
                                    .ok();

                                let wait_result =
                                    crate::tool_executor::wait_for_approval_result_public(
                                        app_handle,
                                        &request_id,
                                        ROBOT_WAIT_TIMEOUT_MS,
                                    )
                                    .await;

                                // 通知前端倒计时结束
                                emit_and_broadcast(
                                    app_handle,
                                    "robot-wait-resolved",
                                    serde_json::json!({
                                        "threadId": thread_id,
                                        "callId": &wait_call_id,
                                    }),
                                );

                                match wait_result {
                                    Ok(user_reply) => {
                                        // 用户在倒计时内回复了补充信息（前端传 { "userReply": "..." }）
                                        let reply_text = user_reply
                                            .get("userReply")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .trim()
                                            .to_string();

                                        if reply_text.is_empty() {
                                            // resolve 但无有效文本，等同于跳过
                                            info!(
                                                "Robot wait resolved with empty reply, auto-continuing"
                                            );
                                            let auto_msg = ThreadMessage {
                                                id: uuid::Uuid::new_v4().to_string(),
                                                role: "system".to_string(),
                                                content: "用户未在限定时间内补充需求，请根据已有信息和上下文继续执行当前节点任务。".to_string(),
                                                timestamp: now_secs(),
                                                tool_call_id: None,
                                                tool_name: None,
                                                tool_calls: None,
                                                attachments: Vec::new(),
                                            };
                                            self.thread_store
                                                .add_message(thread_id, auto_msg)
                                                .await?;
                                            continue;
                                        }

                                        info!(
                                            "User replied during robot wait: {} chars",
                                            reply_text.len()
                                        );
                                        let user_msg = ThreadMessage {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            role: "user".to_string(),
                                            content: reply_text,
                                            timestamp: now_secs(),
                                            tool_call_id: None,
                                            tool_name: None,
                                            tool_calls: None,
                                            attachments: Vec::new(),
                                        };
                                        self.thread_store.add_message(thread_id, user_msg).await?;
                                        continue;
                                    }
                                    Err(_timeout_or_reject) => {
                                        // 超时无回复，注入系统消息自动继续
                                        info!(
                                            "Robot wait timed out, auto-continuing with existing info"
                                        );
                                        let auto_continue_msg = ThreadMessage {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            role: "system".to_string(),
                                            content:
                                                "用户未在限定时间内补充需求，请根据已有信息和上下文继续执行当前节点任务。"
                                                    .to_string(),
                                            timestamp: now_secs(),
                                            tool_call_id: None,
                                            tool_name: None,
                                            tool_calls: None,
                                            attachments: Vec::new(),
                                        };
                                        self.thread_store
                                            .add_message(thread_id, auto_continue_msg)
                                            .await?;
                                        // 超时后继续 agent 循环
                                        continue;
                                    }
                                }
                            }
                            let git_status_now = git_status_snapshot(&effective_cwd).await;
                            merge_git_changes(
                                &mut changed_files,
                                &git_status_before,
                                &git_status_now,
                            );
                            let budget_limited_now = if turn_mode == "goal" {
                                let current_goal = self
                                    .thread_store
                                    .get_thread(thread_id)
                                    .await
                                    .and_then(|thread| thread.goal);
                                goal_budget_limited_after(current_goal.as_ref(), &turn_usage)
                            } else {
                                false
                            };
                            let stop_hook_results = hook_runtime
                                .run_event(
                                    app_handle,
                                    thread_id,
                                    HOOK_AGENT_END,
                                    &effective_cwd,
                                    stop_hook_context(
                                        &turn_id,
                                        &turn_mode,
                                        &effective_cwd,
                                        turn_timer.elapsed().as_millis().min(u128::from(u64::MAX))
                                            as u64,
                                        &changed_files,
                                        &turn_usage,
                                        goal_budget_tokens,
                                        budget_limited_now,
                                        &model,
                                        stop_hook_continuations > 0,
                                        content.as_str(),
                                    ),
                                )
                                .await;
                            stop_hooks_ran_for_last_stop = true;
                            if first_blocking_hook_result(&stop_hook_results).is_some() {
                                if let Some(continuation) =
                                    stop_hook_continuation_message(&stop_hook_results)
                                {
                                    stop_hook_continuations =
                                        stop_hook_continuations.saturating_add(1);
                                    let msg = ThreadMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: "system".to_string(),
                                        content: continuation,
                                        timestamp: now_secs(),
                                        tool_call_id: None,
                                        tool_name: None,
                                        tool_calls: None,
                                        attachments: Vec::new(),
                                    };
                                    self.thread_store.add_message(thread_id, msg).await?;
                                    continue;
                                }
                            }

                            if let Some(progress_snapshot) = robot_progress.clone() {
                                match robot_orchestrator
                                    .apply_node_progress(
                                        &self.thread_store,
                                        thread_id,
                                        progress_snapshot,
                                        node_done_signal,
                                        node_delivery_summary,
                                    )
                                    .await?
                                {
                                    NodeProgressResult::ContinueCurrent { state, nudge } => {
                                        robot_progress = Some(state);
                                        let msg = ThreadMessage {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            role: "system".to_string(),
                                            content: nudge,
                                            timestamp: now_secs(),
                                            tool_call_id: None,
                                            tool_name: None,
                                            tool_calls: None,
                                            attachments: Vec::new(),
                                        };
                                        self.thread_store.add_message(thread_id, msg).await?;
                                        continue;
                                    }
                                    NodeProgressResult::Advanced { state, nudge } => {
                                        // 将推进后的 nudge 消息 id 记为“当前节点起点边界”，
                                        // 用于后续把上游节点的原始杂乱历史从模型上下文裁剪掉，
                                        // 仅保留原始用户目标 + 当前节点自身消息（含其交付总结）。
                                        let boundary_id = uuid::Uuid::new_v4().to_string();
                                        let mut advanced_state = state;
                                        advanced_state.current_node_start_message_id =
                                            Some(boundary_id.clone());
                                        self.thread_store
                                            .set_thread_robot_state(
                                                thread_id,
                                                advanced_state.clone(),
                                            )
                                            .await?;
                                        robot_progress = Some(advanced_state);
                                        let msg = ThreadMessage {
                                            id: boundary_id,
                                            role: "system".to_string(),
                                            content: nudge,
                                            timestamp: now_secs(),
                                            tool_call_id: None,
                                            tool_name: None,
                                            tool_calls: None,
                                            attachments: Vec::new(),
                                        };
                                        self.thread_store.add_message(thread_id, msg).await?;
                                        continue;
                                    }
                                    NodeProgressResult::Completed => {
                                        robot_progress = None;
                                        if let Some(goal) = self
                                            .thread_store
                                            .get_thread(thread_id)
                                            .await
                                            .and_then(|thread| thread.goal)
                                        {
                                            emit_and_broadcast(
                                                app_handle,
                                                "thread-goal-updated",
                                                serde_json::json!({
                                                    "threadId": thread_id,
                                                    "goal": goal,
                                                }),
                                            );
                                        }
                                        stop_hooks_satisfied = true;
                                        break;
                                    }
                                }
                            }

                            stop_hooks_satisfied = true;
                            break;
                        }
                        Ok(CompletionResult::ToolCalls {
                            calls,
                            preceding_text,
                            usage,
                        }) => {
                            rate_limit_retry_count = 0;
                            llm_call_count = llm_call_count.saturating_add(1);
                            info!(
                                "Iteration {iteration}: ToolCalls ({}): {:?}, preceding_text={} chars, usage={:?}",
                                calls.len(),
                                calls.iter().map(|c| &c.name).collect::<Vec<_>>(),
                                preceding_text.len(),
                                usage
                            );
                            if let Some(ref u) = usage {
                                add_turn_usage(&mut turn_usage, u);
                                last_prompt_tokens = u.prompt_tokens;
                                turn_usage.call_count = llm_call_count;
                                turn_usage.last_single_prompt_tokens = last_prompt_tokens;
                                emit_turn_usage_updated(
                                    app_handle,
                                    thread_id,
                                    &turn_usage,
                                    model_context_window_tokens,
                                );
                                if let Some(ref recorder) = self.usage_recorder {
                                    recorder.record(&provider_id, &model, thread_id, u);
                                }
                            }

                            if !preceding_text.is_empty() {
                                let text_msg = ThreadMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: "assistant".to_string(),
                                    content: preceding_text,
                                    timestamp: now_secs(),
                                    tool_call_id: None,
                                    tool_name: None,
                                    tool_calls: None,
                                    attachments: Vec::new(),
                                };
                                self.thread_store.add_message(thread_id, text_msg).await?;
                            }

                            let tc_infos: Vec<ToolCallInfo> = calls
                                .iter()
                                .map(|c| ToolCallInfo {
                                    id: c.id.clone(),
                                    name: c.name.clone(),
                                    arguments: c.arguments.clone(),
                                })
                                .collect();
                            let assistant_tc_msg = ThreadMessage {
                                id: uuid::Uuid::new_v4().to_string(),
                                role: "assistant".to_string(),
                                content: String::new(),
                                timestamp: now_secs(),
                                tool_call_id: None,
                                tool_name: None,
                                tool_calls: Some(tc_infos),
                                attachments: Vec::new(),
                            };
                            self.thread_store
                                .add_message(thread_id, assistant_tc_msg)
                                .await?;

                            let calls_json: Vec<serde_json::Value> = calls
                                .iter()
                                .map(|c| {
                                    serde_json::json!({
                                        "id": c.id,
                                        "name": c.name,
                                        "arguments": c.arguments,
                                    })
                                })
                                .collect();
                            emit_and_broadcast(
                                app_handle,
                                "tool-calls-start",
                                serde_json::json!({
                                    "threadId": thread_id,
                                    "calls": calls_json,
                                }),
                            );

                            let mut results_json: Vec<serde_json::Value> = Vec::new();
                            let mut robot_node_advanced_now = false;

                            for mut call in calls {
                                info!("Tool call: {} args={}", call.name, call.arguments);
                                if goal_completed_now {
                                    let skipped_call_id = call.id.clone();
                                    let skipped_tool_name = call.name.clone();
                                    let skipped_output =
                                        "Tool execution skipped: goal already marked complete."
                                            .to_string();
                                    emit_and_broadcast(
                                        app_handle,
                                        "tool-exec-end",
                                        serde_json::json!({
                                            "threadId": thread_id,
                                            "callId": skipped_call_id,
                                            "tool": skipped_tool_name,
                                            "exitCode": -1,
                                            "output": skipped_output.clone(),
                                        }),
                                    );
                                    results_json.push(serde_json::json!({
                                        "id": call.id.clone(),
                                        "tool": call.name.clone(),
                                        "success": false,
                                        "skippedAfterGoalComplete": true,
                                    }));
                                    let tool_msg = ThreadMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: "tool".to_string(),
                                        content: skipped_output,
                                        timestamp: now_secs(),
                                        tool_call_id: Some(call.id.clone()),
                                        tool_name: Some(call.name.clone()),
                                        tool_calls: None,
                                        attachments: Vec::new(),
                                    };
                                    self.thread_store.add_message(thread_id, tool_msg).await?;
                                    continue;
                                }
                                if robot_node_advanced_now {
                                    let skipped_call_id = call.id.clone();
                                    let skipped_tool_name = call.name.clone();
                                    let skipped_output =
                                        "Tool execution skipped: workflow node already advanced."
                                            .to_string();
                                    emit_and_broadcast(
                                        app_handle,
                                        "tool-exec-end",
                                        serde_json::json!({
                                            "threadId": thread_id,
                                            "callId": skipped_call_id,
                                            "tool": skipped_tool_name,
                                            "exitCode": -1,
                                            "output": skipped_output.clone(),
                                        }),
                                    );
                                    results_json.push(serde_json::json!({
                                        "id": call.id.clone(),
                                        "tool": call.name.clone(),
                                        "success": false,
                                        "skippedAfterRobotNodeAdvance": true,
                                    }));
                                    let tool_msg = ThreadMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: "tool".to_string(),
                                        content: skipped_output,
                                        timestamp: now_secs(),
                                        tool_call_id: Some(call.id.clone()),
                                        tool_name: Some(call.name.clone()),
                                        tool_calls: None,
                                        attachments: Vec::new(),
                                    };
                                    self.thread_store.add_message(thread_id, tool_msg).await?;
                                    continue;
                                }
                                // 若用户已点击停止，则跳过工具执行，并主动补发结束状态，
                                // 防止前端工具卡片一直停留在 running。
                                if self.is_cancelled() {
                                    let interrupted_call_id = call.id.clone();
                                    let interrupted_tool_name = call.name.clone();
                                    let interrupted_output =
                                        "Tool execution skipped: interrupted by user.".to_string();
                                    emit_and_broadcast(
                                        app_handle,
                                        "tool-exec-end",
                                        serde_json::json!({
                                            "threadId": thread_id,
                                            "callId": interrupted_call_id,
                                            "tool": interrupted_tool_name,
                                            "exitCode": -1,
                                            "output": interrupted_output.clone(),
                                        }),
                                    );
                                    results_json.push(serde_json::json!({
                                        "id": call.id.clone(),
                                        "tool": call.name.clone(),
                                        "success": false,
                                        "interrupted": true,
                                    }));
                                    let tool_msg = ThreadMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: "tool".to_string(),
                                        content: interrupted_output,
                                        timestamp: now_secs(),
                                        tool_call_id: Some(call.id.clone()),
                                        tool_name: Some(call.name.clone()),
                                        tool_calls: None,
                                        attachments: Vec::new(),
                                    };
                                    self.thread_store.add_message(thread_id, tool_msg).await?;
                                    continue;
                                }
                                let pre_tool_hook_results = hook_runtime
                                    .run_event(
                                        app_handle,
                                        thread_id,
                                        HOOK_COMMAND_EXEC,
                                        &effective_cwd,
                                        tool_call_hook_context(&turn_id, &call),
                                    )
                                    .await;

                                if let Some(blocking_hook) =
                                    first_blocking_hook_result(&pre_tool_hook_results)
                                {
                                    let result_content =
                                        blocked_tool_call_output(&call.name, blocking_hook);
                                    results_json.push(serde_json::json!({
                                        "id": call.id,
                                        "tool": call.name,
                                        "success": false,
                                        "blockedByHook": true,
                                    }));

                                    let tool_msg = ThreadMessage {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        role: "tool".to_string(),
                                        content: result_content,
                                        timestamp: now_secs(),
                                        tool_call_id: Some(call.id.clone()),
                                        tool_name: Some(call.name.clone()),
                                        tool_calls: None,
                                        attachments: Vec::new(),
                                    };
                                    self.thread_store.add_message(thread_id, tool_msg).await?;
                                    continue;
                                }

                                if let Some(updated_input) =
                                    latest_hook_updated_input(&pre_tool_hook_results)
                                {
                                    call.arguments = serde_json::to_string(&updated_input)
                                        .unwrap_or_else(|_| call.arguments.clone());
                                }

                                let requested_file_changes = file_changes_from_tool_call(&call);
                                if !requested_file_changes.is_empty() {
                                    capture_before_file_snapshots(
                                        &mut changed_file_snapshot_map,
                                        &requested_file_changes,
                                        &effective_cwd,
                                    );
                                }

                                let (mut result_content, success) = if call.name == "update_goal" {
                                    let requested_status =
                                        extract_update_goal_status(&call.arguments);
                                    if requested_status.as_deref() == Some("complete") {
                                        if let Some(progress_snapshot) = robot_progress.clone() {
                                            match advance_robot_workflow_from_goal_completion(
                                                &self.thread_store,
                                                &robot_orchestrator,
                                                thread_id,
                                                progress_snapshot,
                                            )
                                            .await
                                            {
                                                Ok(RobotGoalCompletionOutcome::Advanced(state)) => {
                                                    let next_node =
                                                        state.current_node_index.saturating_add(1);
                                                    let total_nodes =
                                                        state.runtime_nodes.len().max(1);
                                                    robot_progress = Some(state);
                                                    robot_node_advanced_now = true;
                                                    (
                                                        format!(
                                                            "Current workflow node marked complete; advanced to node {next_node}/{total_nodes}."
                                                        ),
                                                        true,
                                                    )
                                                }
                                                Ok(RobotGoalCompletionOutcome::Completed) => {
                                                    robot_progress = None;
                                                    if let Some(goal) = self
                                                        .thread_store
                                                        .get_thread(thread_id)
                                                        .await
                                                        .and_then(|thread| thread.goal)
                                                    {
                                                        emit_and_broadcast(
                                                            app_handle,
                                                            "thread-goal-updated",
                                                            serde_json::json!({
                                                                "threadId": thread_id,
                                                                "goal": goal,
                                                            }),
                                                        );
                                                    }
                                                    goal_completed_now = true;
                                                    (
                                                        "Final workflow node marked complete; robot workflow finished."
                                                            .to_string(),
                                                        true,
                                                    )
                                                }
                                                Err(e) => {
                                                    (format!("update_goal error: {e}"), false)
                                                }
                                            }
                                        } else {
                                            match handle_update_goal(
                                                &self.thread_store,
                                                app_handle,
                                                thread_id,
                                                &call.arguments,
                                            )
                                            .await
                                            {
                                                Ok(outcome) => {
                                                    if outcome.goal.status
                                                        == ThreadGoalStatus::Complete
                                                    {
                                                        goal_completed_now = true;
                                                    }
                                                    (outcome.message, true)
                                                }
                                                Err(e) => {
                                                    (format!("update_goal error: {e}"), false)
                                                }
                                            }
                                        }
                                    } else {
                                        match handle_update_goal(
                                            &self.thread_store,
                                            app_handle,
                                            thread_id,
                                            &call.arguments,
                                        )
                                        .await
                                        {
                                            Ok(outcome) => {
                                                if outcome.goal.status == ThreadGoalStatus::Complete
                                                {
                                                    goal_completed_now = true;
                                                }
                                                (outcome.message, true)
                                            }
                                            Err(e) => (format!("update_goal error: {e}"), false),
                                        }
                                    }
                                } else {
                                    let tool_result = self
                                        .tool_executor
                                        .read()
                                        .await
                                        .execute(
                                            &call.name,
                                            &call.arguments,
                                            &call.id,
                                            app_handle,
                                            thread_id,
                                        )
                                        .await;
                                    match tool_result {
                                        Ok(output) => (output, true),
                                        Err(e) => (format!("Tool execution error: {e}"), false),
                                    }
                                };
                                let mut has_subagent_stop_feedback = false;
                                if success && call.name == "close_agent" {
                                    let subagent_stop_hook_results = hook_runtime
                                        .run_event(
                                            app_handle,
                                            thread_id,
                                            HOOK_SUBAGENT_STOP,
                                            &effective_cwd,
                                            subagent_stop_hook_context(
                                                &turn_id,
                                                &turn_mode,
                                                &effective_cwd,
                                                &model,
                                                &call,
                                                &result_content,
                                            ),
                                        )
                                        .await;
                                    let subagent_stop_feedback =
                                        hook_feedback_for_model(&subagent_stop_hook_results);
                                    has_subagent_stop_feedback = !subagent_stop_feedback.is_empty();
                                    if !subagent_stop_feedback.is_empty() {
                                        result_content = append_subagent_stop_hook_feedback(
                                            result_content,
                                            subagent_stop_feedback,
                                        );
                                    }
                                }
                                let post_tool_hook_results = hook_runtime
                                    .run_event(
                                        app_handle,
                                        thread_id,
                                        HOOK_POST_TOOL_USE,
                                        &effective_cwd,
                                        tool_result_hook_context(
                                            &turn_id,
                                            &call,
                                            &result_content,
                                            success,
                                        ),
                                    )
                                    .await;
                                let post_hook_feedback =
                                    hook_feedback_for_model(&post_tool_hook_results);
                                let has_post_hook_feedback = !post_hook_feedback.is_empty();
                                if !post_hook_feedback.is_empty() {
                                    result_content = append_post_tool_hook_feedback(
                                        result_content,
                                        post_hook_feedback,
                                    );
                                }

                                if success {
                                    capture_after_file_snapshots(
                                        &mut changed_file_snapshot_map,
                                        &requested_file_changes,
                                        &effective_cwd,
                                    );
                                    for change in requested_file_changes {
                                        push_file_change(&mut changed_files, change);
                                    }
                                }

                                results_json.push(serde_json::json!({
                                    "id": call.id,
                                    "tool": call.name,
                                    "success": success,
                                        "postHookFeedback": has_post_hook_feedback,
                                        "subagentStopHookFeedback": has_subagent_stop_feedback,
                                }));

                                let tool_msg = ThreadMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: "tool".to_string(),
                                    content: result_content,
                                    timestamp: now_secs(),
                                    tool_call_id: Some(call.id.clone()),
                                    tool_name: Some(call.name.clone()),
                                    tool_calls: None,
                                    attachments: Vec::new(),
                                };
                                self.thread_store.add_message(thread_id, tool_msg).await?;
                            }

                            emit_and_broadcast(
                                app_handle,
                                "tool-calls-end",
                                serde_json::json!({
                                    "threadId": thread_id,
                                    "results": results_json,
                                }),
                            );
                            stop_hooks_ran_for_last_stop = false;

                            if goal_completed_now {
                                stop_hooks_satisfied = true;
                                break;
                            }
                            if robot_node_advanced_now {
                                continue;
                            }

                            if !mid_turn_compacted
                                && crate::compaction::should_compact(last_prompt_tokens, config)
                            {
                                info!(
                                    "Mid-turn compaction triggered: {last_prompt_tokens} prompt tokens (single API call)"
                                );
                                let compaction_start = Instant::now();
                                let _ = crate::compaction::run_compaction(
                                    &self.http,
                                    app_handle,
                                    config,
                                    &self.thread_store,
                                    thread_id,
                                    &base_url,
                                    &api_key,
                                    &model,
                                    &wire_api,
                                    Some(&self.cancel_flag),
                                    provider.query_params.as_ref(),
                                    provider.http_headers.as_ref(),
                                )
                                .await;
                                info!(
                                    "Mid-turn compaction completed in {:.1}s",
                                    compaction_start.elapsed().as_secs_f64()
                                );
                                last_prompt_tokens = 0;
                                mid_turn_compacted = true;
                            }
                        }
                        Err(e) => {
                            // 资源池故障转移：标记当前端点失败，尝试切换到下一个
                            if let (Some(pk), Some(pr), Some(ep_idx)) =
                                (&pool_key, &pool_resolver, pool_endpoint_index)
                            {
                                pr.mark_failed(pk, ep_idx);
                                if let Some(next_ep) = pr.resolve_endpoint(pk, pool_endpoints) {
                                    warn!(
                                        "Pool '{pk}': endpoint {ep_idx} failed ({e}), switching to endpoint {} ({})",
                                        next_ep.endpoint_index, next_ep.url
                                    );
                                    base_url = next_ep.url;
                                    api_key = next_ep.api_key.clone().unwrap_or_default();
                                    wire_api = next_ep
                                        .wire_api
                                        .clone()
                                        .unwrap_or_else(|| provider_wire_api.clone());
                                    if let Some(next_model) = next_ep
                                        .model
                                        .as_deref()
                                        .map(str::trim)
                                        .filter(|value| !value.is_empty())
                                    {
                                        model = next_model.to_string();
                                    } else {
                                        model = pool_default_model.clone();
                                    }
                                    pool_endpoint_index = Some(next_ep.endpoint_index);
                                    emit_and_broadcast(
                                        app_handle,
                                        "pool-endpoint-switched",
                                        serde_json::json!({
                                            "threadId": thread_id,
                                            "poolKey": pk,
                                            "failedEndpoint": ep_idx,
                                            "newEndpoint": next_ep.endpoint_index,
                                            "activeEndpointIndex": next_ep.endpoint_index,
                                            "reason": e.to_string(),
                                        }),
                                    );
                                    emit_and_broadcast(
                                        app_handle,
                                        "active-endpoint-index",
                                        serde_json::json!({ "index": next_ep.endpoint_index }),
                                    );
                                    continue;
                                }
                            }
                            let error_message = e.to_string();
                            if is_retryable_rate_limit_error(&error_message)
                                && rate_limit_retry_count < MAX_RATE_LIMIT_RETRIES
                            {
                                rate_limit_retry_count = rate_limit_retry_count.saturating_add(1);
                                let retry_in_ms = rate_limit_backoff_ms(rate_limit_retry_count);
                                warn!(
                                    "Iteration {iteration}: rate limited, retrying in {retry_in_ms} ms ({rate_limit_retry_count}/{MAX_RATE_LIMIT_RETRIES}): {error_message}"
                                );
                                emit_and_broadcast(
                                    app_handle,
                                    "server-error",
                                    serde_json::json!({
                                        "threadId": thread_id,
                                        "message": error_message,
                                        "retryable": true,
                                        "retryInMs": retry_in_ms,
                                        "attempt": rate_limit_retry_count,
                                        "maxAttempts": MAX_RATE_LIMIT_RETRIES,
                                    }),
                                );
                                tokio::time::sleep(Duration::from_millis(retry_in_ms)).await;
                                if self.is_cancelled() {
                                    terminated_by_error = true;
                                    break;
                                }
                                continue;
                            }
                            error!("Iteration {iteration}: LLM request failed: {e}");
                            emit_and_broadcast(
                                app_handle,
                                "server-error",
                                serde_json::json!({
                                    "threadId": thread_id,
                                    "message": error_message,
                                    "retryable": false,
                                }),
                            );
                            terminated_by_error = true;
                            break;
                        }
                    }
                }
            }

            if !stop_hooks_satisfied
                && !stop_hooks_ran_for_last_stop
                && !prompt_hook_blocked
                && !terminated_by_error
                && !goal_completed_now
            {
                if let Some(progress) = robot_progress.as_ref() {
                    // 机器人强约束模式下，如果节点未完成，不允许退化为“直接总结”。
                    // 这里显式写回提示，保留下次 turn 继续当前节点的状态。
                    let pending_msg = ThreadMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: "system".to_string(),
                        content: build_robot_node_completion_nudge(
                            progress.current_node_index,
                            progress.runtime_nodes.len(),
                        ),
                        timestamp: now_secs(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        attachments: Vec::new(),
                    };
                    self.thread_store
                        .add_message(thread_id, pending_msg)
                        .await?;
                } else {
                    info!(
                        "Agent loop ended after tool calls without summary, requesting final summary"
                    );
                    let summary_nudge = ThreadMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: "system".to_string(),
                    content: "All tool executions have completed. You MUST now provide a brief \
                              summary: what was done, any errors encountered, and suggested next steps. \
                              Do NOT call any more tools."
                        .to_string(),
                    timestamp: now_secs(),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                    attachments: Vec::new(),
                };
                    self.thread_store
                        .add_message(thread_id, summary_nudge)
                        .await?;

                    let history = self.thread_store.get_thread_messages(thread_id).await;
                    let model_history = if let Some(state) = robot_progress.as_ref() {
                        build_robot_model_history(&history, state)
                    } else {
                        history
                    };
                    let robot_overlay_prompt = if let Some(state) = robot_progress.as_ref() {
                        Some(robot_orchestrator.build_overlay_prompt(state)?)
                    } else {
                        None
                    };
                    let summary_plan_context = if turn_mode == "plan" {
                        if active_plan_content.is_none() {
                            if let Some(path) = active_plan_path.as_deref() {
                                active_plan_content = read_plan_file_content(path, &self.cwd);
                            }
                        }
                        match (active_plan_path.as_deref(), active_plan_content.as_deref()) {
                            (Some(path), Some(content)) => {
                                Some((path, active_plan_revision.max(1), content))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let internal_messages = self.build_internal_messages(
                        config,
                        &model_history,
                        &effective_cwd,
                        &turn_mode,
                        robot_id,
                        Some(user_message_id.as_str()),
                        &[],
                        robot_overlay_prompt.as_deref(),
                        summary_plan_context,
                    );
                    let summary_result = self
                        .stream_completion(
                            app_handle,
                            thread_id,
                            &base_url,
                            &api_key,
                            &model,
                            &wire_api,
                            internal_messages,
                            None,
                            config.max_output_tokens,
                            u32::MAX,
                            false,
                            provider.query_params.as_ref(),
                            provider.http_headers.as_ref(),
                        )
                        .await;
                    let summary_text = match summary_result {
                        Ok(CompletionResult::Message { text, usage, .. }) => {
                            llm_call_count = llm_call_count.saturating_add(1);
                            if let Some(u) = usage {
                                add_turn_usage(&mut turn_usage, &u);
                                last_prompt_tokens = u.prompt_tokens;
                                turn_usage.call_count = llm_call_count;
                                turn_usage.last_single_prompt_tokens = last_prompt_tokens;
                                emit_turn_usage_updated(
                                    app_handle,
                                    thread_id,
                                    &turn_usage,
                                    model_context_window_tokens,
                                );
                                if let Some(ref recorder) = self.usage_recorder {
                                    recorder.record(&provider_id, &model, thread_id, &u);
                                }
                            }
                            text
                        }
                        Ok(CompletionResult::ToolCalls {
                            preceding_text,
                            usage,
                            ..
                        }) => {
                            llm_call_count = llm_call_count.saturating_add(1);
                            if let Some(u) = usage {
                                add_turn_usage(&mut turn_usage, &u);
                                last_prompt_tokens = u.prompt_tokens;
                                turn_usage.call_count = llm_call_count;
                                turn_usage.last_single_prompt_tokens = last_prompt_tokens;
                                emit_turn_usage_updated(
                                    app_handle,
                                    thread_id,
                                    &turn_usage,
                                    model_context_window_tokens,
                                );
                                if let Some(ref recorder) = self.usage_recorder {
                                    recorder.record(&provider_id, &model, thread_id, &u);
                                }
                            }
                            preceding_text
                        }
                        Err(e) => {
                            warn!("Final summary LLM call failed: {e}");
                            String::new()
                        }
                    };
                    if !summary_text.is_empty() {
                        let msg = ThreadMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            role: "assistant".to_string(),
                            content: summary_text,
                            timestamp: now_secs(),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                            attachments: Vec::new(),
                        };
                        self.thread_store.add_message(thread_id, msg).await?;
                    }
                }
            }

            // Goal continuation: if goal is still Active, inject continuation prompt
            // and restart the agent loop instead of ending the turn.
            if turn_mode != "goal" || self.is_cancelled() || prompt_hook_blocked {
                break 'goal_loop;
            }
            let continuation_goal = self
                .thread_store
                .get_thread(thread_id)
                .await
                .and_then(|t| t.goal);
            let should_continue = continuation_goal
                .as_ref()
                .is_some_and(|g| g.status == ThreadGoalStatus::Active);
            if !should_continue || goal_continuation_count >= max_goal_continuations {
                break 'goal_loop;
            }
            goal_continuation_count += 1;
            info!(
                "Goal continuation {goal_continuation_count}/{max_goal_continuations} for turn {turn_id}"
            );
            let continuation_msg = ThreadMessage {
                id: uuid::Uuid::new_v4().to_string(),
                role: "system".to_string(),
                content: build_goal_continuation_prompt(continuation_goal.as_ref().unwrap()),
                timestamp: now_secs(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                attachments: Vec::new(),
            };
            self.thread_store
                .add_message(thread_id, continuation_msg)
                .await?;
            emit_and_broadcast(
                app_handle,
                "goal-continuation",
                serde_json::json!({
                    "threadId": thread_id,
                    "continuation": goal_continuation_count,
                }),
            );
            stop_hooks_satisfied = false;
            stop_hooks_ran_for_last_stop = false;
            intent_retries = 0;
        } // end 'goal_loop

        info!("Turn {turn_id} completed for thread {thread_id}");

        let git_status_after = git_status_snapshot(&effective_cwd).await;
        merge_git_changes(&mut changed_files, &git_status_before, &git_status_after);
        if !changed_files.is_empty() {
            hook_runtime
                .run_event(
                    app_handle,
                    thread_id,
                    HOOK_FILE_CHANGE,
                    &effective_cwd,
                    serde_json::json!({
                        "turnId": &turn_id,
                        "mode": &turn_mode,
                        "cwd": &effective_cwd,
                        "changedFiles": &changed_files,
                    }),
                )
                .await;
        }
        let budget_limited = if turn_mode == "goal" {
            let current_goal = self
                .thread_store
                .get_thread(thread_id)
                .await
                .and_then(|thread| thread.goal);
            goal_budget_limited_after(current_goal.as_ref(), &turn_usage)
        } else {
            false
        };

        if !stop_hooks_satisfied && !stop_hooks_ran_for_last_stop {
            hook_runtime
                .run_event(
                    app_handle,
                    thread_id,
                    HOOK_AGENT_END,
                    &effective_cwd,
                    stop_hook_context(
                        &turn_id,
                        &turn_mode,
                        &effective_cwd,
                        turn_timer.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        &changed_files,
                        &turn_usage,
                        goal_budget_tokens,
                        budget_limited,
                        &model,
                        stop_hook_continuations > 0,
                        "",
                    ),
                )
                .await;
        }
        let git_status_after_hooks = git_status_snapshot(&effective_cwd).await;
        merge_git_changes(
            &mut changed_files,
            &git_status_after,
            &git_status_after_hooks,
        );
        let duration_ms = turn_timer.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let mut goal_after = if turn_mode == "goal" {
            self.thread_store
                .record_goal_usage(thread_id, turn_usage.total_tokens)
                .await?
        } else {
            None
        };
        let budget_limited = goal_after
            .as_ref()
            .is_some_and(|goal| goal.status == ThreadGoalStatus::BudgetLimited)
            || budget_limited;
        if let Some(ref goal) = goal_after {
            if goal.status == ThreadGoalStatus::Active {
                let paused = self
                    .thread_store
                    .set_thread_goal_status(thread_id, ThreadGoalStatus::Paused)
                    .await?;
                goal_after = Some(paused);
            }
        }
        turn_usage.call_count = llm_call_count;
        turn_usage.last_single_prompt_tokens = last_prompt_tokens;
        let completed_at = self
            .thread_store
            .end_turn(
                thread_id,
                &turn_id,
                Some(duration_ms),
                changed_files.clone(),
                nonzero_turn_usage(&turn_usage),
                budget_limited,
            )
            .await?;
        let usage = nonzero_turn_usage(&turn_usage);
        let changed_file_snapshots = build_changed_file_snapshots(
            &changed_files,
            &changed_file_snapshot_map,
            &effective_cwd,
        );

        let mut completed_payload = serde_json::json!({
            "threadId": thread_id,
            "turn": {
                "id": &turn_id,
                "mode": &turn_mode,
                "cwd": effective_cwd.to_string_lossy(),
                "startedAt": turn_started_at_ms,
                "completedAt": completed_at * 1000,
                "durationMs": duration_ms,
                "changedFiles": changed_files,
                "changedFileSnapshots": changed_file_snapshots,
                "usage": usage,
                "goalBudgetTokens": goal_budget_tokens,
                "budgetLimited": budget_limited,
            }
        });
        if turn_mode == "goal" {
            completed_payload["goal"] = serde_json::json!(goal_after);
        }
        emit_and_broadcast(app_handle, "turn-completed", completed_payload);

        // Fire-and-forget SmartBrain experience extraction for this session.
        {
            let http = self.http.clone();
            let config_clone = config.clone();
            let thread_store = self.thread_store.clone();
            let workspace_config_dir = self.cwd.join("codey");
            let thread_id_owned = thread_id.to_string();
            let app_handle_for_extraction = app_handle.clone();
            tokio::spawn(async move {
                let sb_config = config_clone.smartbrain_config();
                if !sb_config.is_active() || !sb_config.auto_extract {
                    return;
                }
                let experiences_dir = crate::smartbrain::experiences_dir(&workspace_config_dir);
                let _ = std::fs::create_dir_all(experiences_dir.join("raw"));

                let index = crate::smartbrain::index::ExperienceIndex::load(&experiences_dir);
                if index.has_entry(&thread_id_owned)
                    && !index.is_stale(
                        &thread_id_owned,
                        thread_store
                            .get_thread(&thread_id_owned)
                            .await
                            .map(|t| t.updated_at)
                            .unwrap_or(0),
                    )
                {
                    return;
                }

                crate::smartbrain::extractor::run_extraction_for_thread(
                    &http,
                    &config_clone,
                    &thread_store,
                    &experiences_dir,
                    &thread_id_owned,
                    Some(&app_handle_for_extraction),
                )
                .await;
            });
        }

        Ok(())
    }

    fn build_system_prompt(
        &self,
        config: &ConfigToml,
        effective_cwd: &Path,
        mode: &str,
        robot_id: Option<&str>,
    ) -> String {
        if mode == "robot-create" {
            return self.build_robot_create_prompt(config, effective_cwd);
        }

        if mode == "robot-modify" {
            if let Some(rid) = robot_id {
                if let Some(prompt) = self.build_robot_modify_prompt(config, effective_cwd, rid) {
                    return prompt;
                }
            }
            return self.build_robot_create_prompt(config, effective_cwd);
        }

        let cwd_str = effective_cwd.to_string_lossy();
        let os_info = std::env::consts::OS;
        let arch_info = std::env::consts::ARCH;

        let workspace_config_dir = self.cwd.join("codey");
        let user_rules_path = workspace_config_dir.join("user-rules.md");
        let user_rules_content = std::fs::read_to_string(&user_rules_path).unwrap_or_default();
        let user_instructions = if !user_rules_content.trim().is_empty() {
            let truncated = truncate_utf8_by_bytes(&user_rules_content, 4000);
            format!("\n\n## User Rules (from codey/user-rules.md)\n{truncated}")
        } else {
            config
                .instructions
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| format!("\n\n## User Rules\n{s}"))
                .unwrap_or_default()
        };

        let skills_instructions = self.render_available_skills_prompt();
        let apps_instructions = self.render_plugin_apps_prompt();
        let web_tool_instructions = if config.web_search_enabled() {
            "             - web_search: 搜索互联网获取最新信息，返回标题、URL 和摘要。\n\
             - web_fetch: 读取指定 URL 的网页内容，返回可读文本。\n\
             \n\
             ## 网页搜索使用指南\n\
             \n\
             使用策略：\n\
             - 先用 web_search 搜索关键词，从结果中选择最相关的 URL，再用 web_fetch 获取详情。\n\
             - 每次搜索后立即分析返回的摘要内容。如果摘要已经包含了足够回答用户问题的信息，直接回复用户，不要继续搜索或 fetch。\n\
             - 同一问题最多搜索 3 次（使用不同关键词）。3 次后仍不充分，就用已有信息总结回复，并告知用户信息可能不完整。\n\
             - 不要重复 fetch 返回错误或空内容的 URL。\n\
             - 中文问题使用中文关键词搜索，英文问题使用英文关键词。\n\
             - 可以在一次搜索中使用精确的关键词组合提高效率，而不是反复搜索模糊的词。\n\
             \n\
             决策边界（何时必须搜索）：\n\
             - 用户明确要求搜索、查找、验证信息时，必须搜索。\n\
             - 信息可能已变化时必须搜索：新闻、价格、法规、体育比分、软件版本、发布日期、时间表等。\n\
             - 不确定事实准确性时，偏向搜索验证而非凭记忆回答。\n\
             - 涉及高风险领域（医疗、法律、金融）时应搜索验证。\n\
             - 引用了特定页面、论文、网站但你没有其内容时，应使用 web_fetch 获取。\n\
             \n\
             引用规则：\n\
             - 回复中附上信息来源的 URL 链接。\n\
             - 不要大段逐字复制网页内容，用自己的话总结。引用原文不超过 50 字。\n"
        } else {
            ""
        };
        let mode_instructions = if mode == "goal" {
            "\n\nGoal mode is active. Treat the latest user message as a concrete objective, not a casual chat prompt. \
             Keep working through the available tools until the objective is genuinely handled or you hit a real blocker. \
             Do NOT stop after just planning or updating the plan — actually create the files, run the commands, and \
             complete the work. Prefer implementation and verification over proposals. Give concise progress updates \
             as you work, and finish with a short outcome summary that mentions verification and the important files changed."
        } else if mode == "plan" {
            "\n\nPlan mode is active. You are in planning-only mode. \
             Your task is to analyze the user's request and produce a detailed implementation plan. \
             If an active plan already exists in this thread, treat follow-up user messages as plan revisions by default. \
             Only create a brand-new plan when the user explicitly asks for a new plan/version. \
             Do NOT execute any mutating actions (no file writes, no shell commands that modify state). \
             You MAY read files, search code, and explore the codebase to understand context. \
             \n\nIMPORTANT: You MUST wrap your final plan inside <proposed_plan> tags. \
             Do NOT output the plan as plain text without the tags. The system relies on these \
             tags to extract and display the plan as a card to the user. \
             \n\nFormat: \
             \n<proposed_plan>\n(your plan in markdown format)\n</proposed_plan>\n\
             \nThe plan should include: \
             \n1. A clear title and summary of the approach \
             \n2. Step-by-step implementation steps \
             \n3. Key files to create or modify (with paths) \
             \n4. Potential risks or edge cases \
             \n5. Testing strategy \
             \nKeep the plan concise but actionable. The user will review and optionally execute it. \
             \nRemember: always use <proposed_plan>...</proposed_plan> tags around the plan content."
        } else {
            ""
        };
        let smartbrain_instructions =
            render_smartbrain_runtime_prompt(&workspace_config_dir, &config.smartbrain_config());
        let robot_runtime_instructions =
            render_robot_runtime_prompt(&workspace_config_dir, robot_id);

        let is_project_mode = effective_cwd != self.cwd;
        let file_creation_policy = if is_project_mode {
            "FILE CREATION POLICY: You are working inside a project directory. \
             Create files and folders directly in the current working directory. \
             Do NOT use `codey/workspace/` — that is only for general chat mode."
        } else {
            "FILE CREATION POLICY: You are in general chat mode (no specific project). \
             When creating new projects, folders, or generated output files \
             (e.g. video projects, web apps, scripts), always place them under the `codey/workspace/` \
             subdirectory within the current working directory. Create the `codey/workspace/` directory \
             if it does not exist. Do NOT create project folders directly in the working directory root. \
             This keeps user-generated content organized."
        };

        format!(
            "You are Codey, a coding assistant that helps users with programming tasks.\n\
             You are running in the following environment:\n\
             - Working directory: {cwd_str}\n\
             - Operating system: {os_info} ({arch_info})\n\
             \n\
             You have access to the following tools:\n\
             - shell / shell_command: Execute short shell commands to run code, install packages, build projects, etc.; shell_command supports Codex-style workdir, timeout_ms, login, and sandbox permission request fields.\n\
             - exec_command / write_stdin / close_exec_session: Start a persistent command session for long-running or interactive commands, write stdin or poll output by session id, and close sessions that are no longer needed.\n\
             - read_file: Read the contents of a file at a given path.\n\
             - write_file: Create or overwrite a file with the given content.\n\
             - tool_search: Search available CN-Codex tools, skills, plugin skills, and discovered MCP tools when you are unsure which capability to use.\n\
             - code_review: Review current git changes or a diff against a base ref, reporting changed files, diff-check issues, and obvious risk patterns.\n\
             - apply_patch: Apply Codex-style patches to add, update, delete, or move files. Prefer raw/freeform patch text when available; function-call providers may pass the same body as patch or command.\n\
             - list_directory: List files and subdirectories in a directory.\n\
             - update_plan: Update a concise multi-step task plan; keep at most one step in_progress.\n\
             - request_user_input: Ask the user one to three short structured questions and wait for their response when progress genuinely depends on user input. When providing options, always put the recommended one first.\n\
             - request_permissions: Ask the user for additional filesystem or network permissions and wait for their response.\n\
             - view_image: Inspect and preview local image files, returning format, dimensions, size, and path.\n\
             - image_generate: Generate an image through an OpenAI Images API-compatible backend and save it as a local file when image generation is configured.\n\
             - browser_run: Run a browser session for page navigation, UI interaction, screenshots, and web app testing. Runtime is CN-Codex built-in Tauri WebView controlled by Rust-side JS Injection + CDP. Keep action batches focused and rely on screenshots/html/snapshot for verification.\n\
             - apps_list: List imported plugin app connectors and trusted codex-apps MCP tools, including connector IDs and availability.\n\
             - list_available_plugins_to_install: List local Codex plugin cache candidates that can be imported into this CN-Codex workspace.\n\
             - request_plugin_install: Import one local Codex plugin cache candidate into codey/plugins; call list_available_plugins_to_install first when unsure of the tool_id.\n\
             - plugin_manage: List, enable, disable, or uninstall local workspace plugins under codey/plugins; disabled plugins stay on disk but are excluded from skills, MCP servers, app connectors, and hooks.\n\
            - spawn_agent: Start a background CN-Codex subagent on the built-in internal subagent engine for delegated investigation, review, testing, or implementation.\n\
             - wait_agent: Wait for one or more spawned subagents and read their results.\n\
            - send_input: Send a follow-up message to a spawned subagent; CN-Codex forwards it through the internal subagent channel when available and records it in input history.\n\
            - resume_agent: Resume a stopped spawned subagent by restarting the internal subagent engine with the same task context and id.\n\
             - list_agents: List spawned subagents and their current statuses.\n\
             - close_agent: Close a spawned subagent when it is no longer needed; running subagent processes are stopped when possible.\n\
             - memory_list: List durable CN-Codex memory files.\n\
             - memory_read: Read durable CN-Codex memory files.\n\
             - memory_search: Search durable CN-Codex memory files.\n\
             - memory_write: Write durable memory only when the user explicitly asks you to remember, forget, or update durable information.\n\
             - memory_update: Replace exact text in an existing durable memory only when the user explicitly asks to update durable information.\n\
             - memory_forget: Delete memory paths or remove matching memory lines only when the user explicitly asks you to forget durable information.\n\
             - mcp_list_servers: List configured MCP servers.\n\
             - mcp_status: Inspect MCP server configuration and probe tools/resources/prompts status without revealing secret env values.\n\
             - mcp_list_tools: List tools exposed by configured MCP servers.\n\
             - mcp_call_tool: Call a tool exposed by a configured MCP server.\n\
             - mcp__server__tool direct tools: When present, call the MCP tool directly with its own JSON schema instead of routing through mcp_call_tool.\n\
             - mcp_list_resources: List resources exposed by configured MCP servers.\n\
             - mcp_read_resource: Read an MCP resource by URI.\n\
             - mcp_list_resource_templates: List resource templates exposed by configured MCP servers.\n\
             - mcp_list_prompts: List prompts exposed by configured MCP servers.\n\
             - mcp_get_prompt: Get a prompt by name from a configured MCP server.\n\
             {web_tool_instructions}\
             \n\
             IMAGE TOOL RULE: When the user asks to generate/create/draw an image, call `image_generate` directly instead of only describing the image. \
             Use configured image-generation defaults unless the user explicitly asks for a different model or base URL.\n\
             \n\
             IMPORTANT: Before using any tools, always briefly explain what you are about to do and why. \
             This helps the user understand your reasoning and plan.\n\
             \n\
             IMPORTANT: When the user asks you to create files, write code, run commands, \
             modify projects, or perform any task that requires interacting with the file system \
             or running programs, you MUST use the appropriate tools. Do NOT just describe \
             what you would do — actually do it using tool calls.\n\
             \n\
             CRITICAL RULES:\n\
             1. NEVER express intent without action. If you say \"let me check...\", \"I will read...\", \
                \"let me look at...\" or similar phrases, you MUST include the corresponding tool call \
                in the SAME response. Do NOT output intent text and then stop.\n\
             2. After completing all tool calls, you MUST provide a summary that includes:\n\
                - What was done and the key results\n\
                - Any issues encountered\n\
                - Suggested next steps (if applicable)\n\
                Never end silently after tool execution.\n\
             \n\
             FILE EDITING RULES:\n\
             1. To create or overwrite files, use the `write_file` tool directly.\n\
             2. To apply multi-file edits (add/update/delete/move), use the `apply_patch` tool.\n\
             3. NEVER use shell commands (python, sed, echo, Set-Content, Out-File, etc.) to write or modify file contents. \
                Shell tools are for running programs, building, testing, and other system commands — not for file editing.\n\
             4. Do not use python scripts to read or write files. Use `read_file` and `write_file`/`apply_patch` instead.\n\
             \n\
             All file paths in tool calls should be relative to the working directory unless \
             the user specifies an absolute path.\n\
             \n\
             {file_creation_policy}\n\
             \n\
             WINDOWS SHELL: This system uses PowerShell. Do NOT use '&&' to chain commands — \
             use ';' instead (e.g. 'cd mydir; npm install'). Use Set-Location or cd to change \
             directories. Alternatively, set the 'workdir' parameter in the shell tool call.\n\
             {skills_instructions}{apps_instructions}{mode_instructions}{user_instructions}{robot_runtime_instructions}{smartbrain_instructions}"
        )
    }

    fn render_available_skills_prompt(&self) -> String {
        let skills_dir = self.cwd.join("codey").join("skills");
        let mut skills = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
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
                    .unwrap_or_else(|| "unknown".to_string());
                let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
                let (name, description) = parse_skill_prompt_frontmatter(&content);
                let display_name = if name.is_empty() { id } else { name };
                let rendered_path = skill_md.to_string_lossy().to_string();
                let line = if description.is_empty() {
                    format!("- {display_name}: (file: {rendered_path})")
                } else {
                    format!("- {display_name}: {description} (file: {rendered_path})")
                };
                skills.push(line);
            }
        }

        for plugin_skill in plugin_loader::list_plugin_skill_prompt_entries(&self.cwd.join("codey"))
        {
            let display_name = format!(
                "{}: {}",
                plugin_skill.plugin_display_name, plugin_skill.skill_name
            );
            let rendered_path = plugin_skill.path.to_string_lossy().to_string();
            let line = if plugin_skill.description.is_empty() {
                format!(
                    "- {display_name}: plugin `{}` skill (file: {rendered_path})",
                    plugin_skill.plugin_id
                )
            } else {
                format!(
                    "- {display_name}: {} (plugin `{}`, file: {rendered_path})",
                    plugin_skill.description, plugin_skill.plugin_id
                )
            };
            skills.push(line);
        }

        // Workflows (exposed as skills)
        let workflows_dir = self.cwd.join("codey").join("workflows");
        if let Ok(entries) = std::fs::read_dir(&workflows_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let skill_md = path.join("SKILL.md");
                if !skill_md.is_file() {
                    continue;
                }
                let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
                let (name, description) = parse_skill_prompt_frontmatter(&content);
                let display_name = if name.is_empty() {
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "workflow".to_string())
                } else {
                    format!("[Workflow] {name}")
                };
                let rendered_path = skill_md.to_string_lossy().to_string();
                let line = if description.is_empty() {
                    format!("- {display_name}: (file: {rendered_path})")
                } else {
                    format!("- {display_name}: {description} (file: {rendered_path})")
                };
                skills.push(line);
            }
        }

        if skills.is_empty() {
            return String::new();
        }

        skills.sort();
        let mut body = String::from(
            "\n\nAvailable skills:\n\
             Skills are local instruction packs. If the user names a skill, or the task clearly matches a skill description, read that skill's SKILL.md with read_file before acting. \
             Resolve relative files mentioned by a skill relative to that skill directory. Do not load every skill up front.\n",
        );
        let mut total_chars = body.chars().count();
        let mut omitted = 0usize;
        for line in skills {
            let next_chars = total_chars
                .saturating_add(line.chars().count())
                .saturating_add(1);
            if next_chars > 8_000 {
                omitted = omitted.saturating_add(1);
                continue;
            }
            body.push_str(&line);
            body.push('\n');
            total_chars = next_chars;
        }
        if omitted > 0 {
            body.push_str(&format!(
                "- {omitted} additional skills omitted from this bounded list.\n"
            ));
        }
        body
    }

    fn render_plugin_apps_prompt(&self) -> String {
        render_plugin_apps_prompt_for_config_dir(&self.cwd.join("codey"))
    }

    fn build_robot_create_prompt(&self, config: &ConfigToml, effective_cwd: &Path) -> String {
        let cwd_str = effective_cwd.to_string_lossy();
        let os_info = std::env::consts::OS;
        let arch_info = std::env::consts::ARCH;
        let workspace_config_dir = self.cwd.join("codey");

        let available_skills =
            crate::robot_loader::list_all_available_skills(&workspace_config_dir);
        let mut skills_list = String::new();
        let mut local_section = String::new();
        let mut plugin_section = String::new();

        for skill in &available_skills {
            let desc = if skill.description.is_empty() {
                String::new()
            } else {
                format!(": {}", skill.description)
            };
            match skill.source.as_str() {
                "local" => {
                    local_section.push_str(&format!("- {}{desc}\n", skill.id));
                }
                "plugin" => {
                    let plugin_id = skill.plugin_id.as_deref().unwrap_or("unknown");
                    plugin_section.push_str(&format!("- {plugin_id} > {}{desc}\n", skill.id));
                }
                _ => {}
            }
        }

        if !local_section.is_empty() {
            skills_list.push_str("Available Local Skills:\n");
            skills_list.push_str(&local_section);
        }
        if !plugin_section.is_empty() {
            skills_list.push_str("\nAvailable Plugin Skills:\n");
            skills_list.push_str(&plugin_section);
        }
        if skills_list.is_empty() {
            skills_list.push_str("No skills are currently available.\n");
        }

        let user_instructions = config
            .instructions
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("\n\nAdditional instructions from user:\n{s}"))
            .unwrap_or_default();

        format!(
            "You are a Robot Configuration Expert. Your job is to create a specialized AI robot \
             by analyzing available skills and composing them into an effective node-by-node workflow.\n\
             \n\
             Environment:\n\
             - Working directory: {cwd_str}\n\
             - OS: {os_info} ({arch_info})\n\
             \n\
             {skills_list}\n\
             \n\
             ## robot.json Specification\n\
             \n\
             The robot configuration must follow this JSON format:\n\
             ```json\n\
             {{\n\
               \"name\": \"Robot Name\",\n\
               \"description\": \"What this robot does\",\n\
               \"icon\": \"icon-identifier\",\n\
               \"skills\": [\"local-skill-id-1\", \"local-skill-id-2\"],\n\
               \"pluginSkills\": [\n\
                 {{ \"pluginId\": \"plugin-name\", \"skillId\": \"skill-name\" }}\n\
               ],\n\
               \"workflowNodes\": [\n\
                 {{\n\
                   \"objective\": \"Complete one concrete stage objective\",\n\
                   \"skills\": [\"local-skill-id-1\"],\n\
                   \"pluginSkills\": [{{ \"pluginId\": \"plugin-name\", \"skillId\": \"skill-name\" }}]\n\
                 }},\n\
                 {{\n\
                   \"objective\": \"Next stage objective\",\n\
                   \"skills\": [\"local-skill-id-2\"],\n\
                   \"pluginSkills\": []\n\
                 }}\n\
               ],\n\
               \"workflow\": [\n\
                 \"1. First step...\",\n\
                 \"2. Second step...\"\n\
               ],\n\
               \"systemPrompt\": \"You are an expert at... Your role is to...\"\n\
             }}\n\
             ```\n\
             \n\
             - `skills`: Local skill IDs from codey/skills/ directory.\n\
             - `pluginSkills`: Plugin skill references (pluginId + skillId from the plugin skills list above).\n\
             - `workflowNodes`: REQUIRED. Structured workflow nodes; each node must bind at least one skill (local or plugin).\n\
             - `workflow`: Legacy mirror text for compatibility; should describe the same node order as workflowNodes.\n\
             - `systemPrompt`: Role definition, behavior rules, and output format for the robot.\n\
             \n\
             ## Creation Guidelines\n\
             \n\
             1. Analyze the user's description to understand what they want the robot to do.\n\
             2. Select the most relevant 3-8 skills from the available list.\n\
             3. Design 3-8 workflow nodes, and assign skills for EACH node.\n\
             4. Ensure every node has at least one skill in `skills` or `pluginSkills`.\n\
             5. Keep `workflow` text aligned with `workflowNodes` order for compatibility.\n\
             6. Write a systemPrompt that defines the robot's identity, behavior rules, and output format.\n\
             7. Choose a concise, descriptive `id` for the robot directory (lowercase, hyphenated).\n\
             8. Call the `robot_save` tool with the id and config to create the robot.\n\
             9. After creating, briefly explain what skills were selected and why.\n\
             \n\
             You have access to the `robot_save` tool. Use it to save the robot configuration.\n\
             \n\
             IMPORTANT: Only select skills that actually exist in the available skills list above. \
             Do NOT invent skill IDs that are not listed.\n\
             IMPORTANT: `workflowNodes` is mandatory and each node must include at least one assigned skill.\n\
             \n\
             CRITICAL: You ONLY have access to the `robot_save` tool. Do NOT try to read files, \
             list directories, run shell commands, or use any other tool. Your sole task is to \
             analyze the user's description and compose a robot configuration based on the \
             available skills listed above, then call `robot_save`.{user_instructions}"
        )
    }

    fn build_robot_modify_prompt(
        &self,
        config: &ConfigToml,
        effective_cwd: &Path,
        robot_id: &str,
    ) -> Option<String> {
        let workspace_config_dir = self.cwd.join("codey");
        let detail = crate::robot_loader::read_robot(&workspace_config_dir, robot_id)?;

        let current_config_json = serde_json::to_string_pretty(&detail.config).unwrap_or_default();

        let cwd_str = effective_cwd.to_string_lossy();
        let os_info = std::env::consts::OS;
        let arch_info = std::env::consts::ARCH;

        let available_skills =
            crate::robot_loader::list_all_available_skills(&workspace_config_dir);
        let mut skills_list = String::new();
        let mut local_section = String::new();
        let mut plugin_section = String::new();

        for skill in &available_skills {
            let desc = if skill.description.is_empty() {
                String::new()
            } else {
                format!(": {}", skill.description)
            };
            match skill.source.as_str() {
                "local" => {
                    local_section.push_str(&format!("- {}{desc}\n", skill.id));
                }
                "plugin" => {
                    let plugin_id = skill.plugin_id.as_deref().unwrap_or("unknown");
                    plugin_section.push_str(&format!("- {plugin_id} > {}{desc}\n", skill.id));
                }
                _ => {}
            }
        }
        if !local_section.is_empty() {
            skills_list.push_str("Available Local Skills:\n");
            skills_list.push_str(&local_section);
        }
        if !plugin_section.is_empty() {
            skills_list.push_str("\nAvailable Plugin Skills:\n");
            skills_list.push_str(&plugin_section);
        }

        let user_instructions = config
            .instructions
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("\n\nAdditional instructions from user:\n{s}"))
            .unwrap_or_default();

        Some(format!(
            "You are a Robot Configuration Expert. Your job is to MODIFY an existing robot \
             configuration based on the user's instructions.\n\
             \n\
             Environment:\n\
             - Working directory: {cwd_str}\n\
             - OS: {os_info} ({arch_info})\n\
             \n\
             ## Current Robot Configuration (id: {robot_id})\n\
             \n\
             ```json\n\
             {current_config_json}\n\
             ```\n\
             \n\
             {skills_list}\n\
             \n\
             ## Modification Guidelines\n\
             \n\
             1. Read the user's modification request carefully.\n\
             2. Modify ONLY the parts the user mentions. Keep everything else unchanged.\n\
             3. If the user wants to add/remove skills, update the `skills` and `pluginSkills` arrays.\n\
             4. If the user wants to change workflow, update `workflowNodes` and keep `workflow` text aligned.\n\
             5. Every workflow node MUST include at least one assigned skill in `skills` or `pluginSkills`.\n\
             6. If the user wants to change behavior, update the `systemPrompt`.\n\
             7. Use the SAME `id` (\"{robot_id}\") when calling `robot_save` to overwrite the config.\n\
             8. After modifying, briefly explain what was changed.\n\
             \n\
             Only select skills that actually exist in the available skills list above.\n\
             `workflowNodes` is mandatory in the saved config.\n\
             \n\
             CRITICAL: You ONLY have access to the `robot_save` tool. Do NOT try to read files, \
             list directories, run shell commands, or use any other tool. Analyze the user's \
             request, modify the configuration above, and call `robot_save`.{user_instructions}"
        ))
    }
}

fn render_plugin_apps_prompt_for_config_dir(config_dir: &Path) -> String {
    let mut apps = plugin_loader::list_plugin_app_prompt_entries(config_dir)
        .into_iter()
        .map(|app| {
            format!(
                "- {}: app `{}` connector `{}` (plugin `{}`)",
                app.plugin_display_name, app.app_key, app.connector_id, app.plugin_id
            )
        })
        .collect::<Vec<_>>();

    if apps.is_empty() {
        return String::new();
    }

    apps.sort();
    apps.dedup();
    let mut body = String::from(
        "\n\n## Apps (Connectors)\n\
         Apps (Connectors) can be explicitly triggered in user messages in the format `[$app-name](app://{connector_id})`. Apps can also be implicitly triggered when the context suggests using an available app.\n\
         An app is equivalent to a set of MCP tools within the `codex-apps` MCP server.\n\
         Installed app tools may already be visible as `mcp__...` function tools, or they may need to be lazy-loaded through `tool_search`. Use `apps_list` to inspect installed connector IDs and currently exposed trusted codex-apps MCP tools.\n\
         For apps, prefer `tool_search` and the matching MCP tools; do not additionally call `mcp_list_resources` or `mcp_list_resource_templates` to discover app capabilities, and do not invent app data or actions that are not exposed by tools.\n\
         Available plugin app connectors:\n",
    );
    let mut total_chars = body.chars().count();
    let mut omitted = 0usize;
    for line in apps {
        let next_chars = total_chars
            .saturating_add(line.chars().count())
            .saturating_add(1);
        if next_chars > 4_000 {
            omitted = omitted.saturating_add(1);
            continue;
        }
        body.push_str(&line);
        body.push('\n');
        total_chars = next_chars;
    }
    if omitted > 0 {
        body.push_str(&format!(
            "- {omitted} additional app connectors omitted from this bounded list.\n"
        ));
    }
    body
}

impl AgentEngine {
    async fn resolve_image_context_with_fallback(
        &self,
        config: &ConfigToml,
        user_input: &str,
        active_model: &str,
        attachments: &[UserAttachment],
    ) -> (Vec<UserAttachment>, Option<String>) {
        let has_image = attachments
            .iter()
            .any(|attachment| attachment.mime_type.starts_with("image/"));
        if !has_image {
            return (attachments.to_vec(), None);
        }

        let model_supports_vision = config.model_supports_vision.unwrap_or(true);
        if model_supports_vision {
            return (attachments.to_vec(), None);
        }

        let image_names = attachments
            .iter()
            .filter(|attachment| attachment.mime_type.starts_with("image/"))
            .map(|attachment| attachment.name.clone())
            .collect::<Vec<_>>();
        let kept_attachments = attachments
            .iter()
            .filter(|attachment| !attachment.mime_type.starts_with("image/"))
            .cloned()
            .collect::<Vec<_>>();

        let fallback_provider_id = config
            .vision_fallback_provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let fallback_model = config
            .vision_fallback_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let fallback_kind = config
            .vision_fallback_kind
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if fallback_provider_id.is_some() && fallback_model.is_some() {
                    Some(VISION_FALLBACK_KIND_MULTIMODAL)
                } else {
                    None
                }
            });

        let image_list = image_names.join(", ");
        let no_fallback_hint = format!(
            "当前模型（{active_model}）不支持视觉，且未配置可用的视觉后补（本地 OCR 或多模态模型）。已忽略图片附件：{image_list}。"
        );

        let image_attachments = attachments
            .iter()
            .filter(|attachment| attachment.mime_type.starts_with("image/"))
            .cloned()
            .collect::<Vec<_>>();

        if matches!(fallback_kind, Some(VISION_FALLBACK_KIND_LOCAL_OCR)) {
            let ocr_inputs = image_attachments
                .iter()
                .map(|attachment| OcrImageInput {
                    name: attachment.name.clone(),
                    mime_type: attachment.mime_type.clone(),
                    data_url: attachment.data_url.clone(),
                })
                .collect::<Vec<_>>();
            match extract_text_from_data_urls(&self.cwd, &ocr_inputs) {
                Ok(results) if !results.is_empty() => {
                    let rendered = results
                        .iter()
                        .map(|result| format!("{}:\n{}", result.name, result.text.trim()))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let context = format!("本地 OCR（PP-OCRv5 mobile）识图结果：\n{rendered}");
                    return (kept_attachments, Some(context));
                }
                Ok(_) => {
                    let hint = format!(
                        "本地 OCR（PP-OCRv5 mobile）未提取到可用文本。已忽略图片附件：{image_list}。"
                    );
                    return (kept_attachments, Some(hint));
                }
                Err(err) => {
                    warn!("Local OCR fallback failed: {err}");
                    let hint = format!(
                        "本地 OCR（PP-OCRv5 mobile）调用失败（{err}）。已忽略图片附件：{image_list}。"
                    );
                    return (kept_attachments, Some(hint));
                }
            }
        }

        let (Some(fallback_provider_id), Some(fallback_model)) =
            (fallback_provider_id, fallback_model)
        else {
            return (kept_attachments, Some(no_fallback_hint));
        };

        let fallback_provider = config.resolve_provider_by_id(fallback_provider_id);
        let Some(fallback_base_url) = fallback_provider.resolve_base_url() else {
            warn!("Vision fallback skipped: provider '{fallback_provider_id}' has no base_url");
            return (kept_attachments, Some(no_fallback_hint));
        };
        let fallback_api_key = fallback_provider.resolve_api_key().unwrap_or_default();
        if fallback_api_key.is_empty() {
            warn!("Vision fallback skipped: provider '{fallback_provider_id}' has no API key");
            return (kept_attachments, Some(no_fallback_hint));
        }
        let fallback_wire_api = fallback_provider
            .wire_api
            .as_deref()
            .unwrap_or("chat")
            .to_string();

        match self
            .describe_images_with_fallback_model(
                &fallback_base_url,
                &fallback_api_key,
                fallback_model,
                &fallback_wire_api,
                user_input,
                active_model,
                &image_attachments,
                config.max_output_tokens,
                fallback_provider.query_params.as_ref(),
                fallback_provider.http_headers.as_ref(),
            )
            .await
        {
            Ok(text) if !text.trim().is_empty() => {
                let context = format!(
                    "视觉后补模型 {fallback_provider_id}/{fallback_model} 识图结果：\n{}",
                    text.trim()
                );
                (kept_attachments, Some(context))
            }
            Ok(_) => {
                warn!(
                    "Vision fallback returned empty text: provider={fallback_provider_id}, model={fallback_model}"
                );
                let hint = format!(
                    "视觉后补模型 {fallback_provider_id}/{fallback_model} 未返回可用识图结果。已忽略图片附件：{image_list}。"
                );
                (kept_attachments, Some(hint))
            }
            Err(err) => {
                warn!(
                    "Vision fallback call failed: provider={fallback_provider_id}, model={fallback_model}, error={err}"
                );
                let hint = format!(
                    "视觉后补模型 {fallback_provider_id}/{fallback_model} 调用失败（{err}）。已忽略图片附件：{image_list}。"
                );
                (kept_attachments, Some(hint))
            }
        }
    }

    async fn describe_images_with_fallback_model(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        wire_api: &str,
        user_input: &str,
        active_model: &str,
        image_attachments: &[UserAttachment],
        max_tokens: Option<i64>,
        query_params: Option<&std::collections::HashMap<String, String>>,
        extra_headers: Option<&std::collections::HashMap<String, String>>,
    ) -> AppResult<String> {
        let adapter = adapter::get_adapter(wire_api);
        let url = adapter.build_url(base_url, model);
        let headers = adapter.build_headers(api_key);
        let (url, headers) =
            adapter::apply_request_overrides(url, headers, query_params, extra_headers)
                .map_err(AppError::Custom)?;
        let image_list = image_attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| format!("{}. {}", index + 1, attachment.name))
            .collect::<Vec<_>>()
            .join("\n");
        let user_prompt = format!(
            "You are a vision parsing tool for a downstream coding model.\n\
             Active text model: {active_model}\n\
             User request:\n{user_input}\n\n\
             Attached images:\n{image_list}\n\n\
             Please analyze images and return plain text with these sections:\n\
             1) OCR text\n2) Key visual elements\n3) Facts relevant to the user request.\n\
             Keep it concise but actionable."
        );
        let messages = vec![
            InternalMessage {
                role: "system".to_string(),
                content: text_content(
                    "You convert images into reliable textual context for another LLM. \
                     Never call tools. Return plain text only."
                        .to_string(),
                ),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            InternalMessage {
                role: "user".to_string(),
                content: Some(multimodal_user_content(&user_prompt, image_attachments)),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];
        let body = adapter.build_body(model, &messages, None, max_tokens);

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Custom(format!("Vision fallback HTTP request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(AppError::Custom(format!(
                "Vision fallback API error ({status}): {body_text}"
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        if content_type.contains("application/json") && !content_type.contains("stream") {
            let body_text = response.text().await.unwrap_or_default();
            if let Some(parsed) = extract_non_streaming_text(&body_text) {
                return Ok(parsed);
            }
            if !body_text.trim().is_empty() {
                return Ok(body_text);
            }
            return Err(AppError::Custom(
                "Vision fallback returned empty non-streaming response".to_string(),
            ));
        }

        let mut result_text = String::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|e| AppError::Custom(format!("Vision fallback stream read error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if adapter.is_stream_done(&line) {
                    continue;
                }

                for event in adapter.parse_stream_line(&line) {
                    if let StreamEvent::TextDelta(delta) = event {
                        result_text.push_str(&delta);
                    }
                }
            }
        }

        if result_text.trim().is_empty() {
            return Err(AppError::Custom(
                "Vision fallback returned empty stream response".to_string(),
            ));
        }
        Ok(result_text)
    }

    /// Convert thread history into the adapter layer's unified internal message shape.
    fn build_internal_messages(
        &self,
        config: &ConfigToml,
        history: &[ThreadMessage],
        effective_cwd: &Path,
        mode: &str,
        robot_id: Option<&str>,
        current_user_message_id: Option<&str>,
        attachments: &[UserAttachment],
        robot_overlay_prompt: Option<&str>,
        active_plan_context: Option<(&str, u64, &str)>,
    ) -> Vec<InternalMessage> {
        let mut messages = Vec::new();
        let sanitized_history = sanitize_history_for_model(history);

        // 主 system prompt：保持 chat/goal 原语义，不在这里嵌入机器人覆盖逻辑。
        messages.push(InternalMessage {
            role: "system".to_string(),
            content: text_content(self.build_system_prompt(config, effective_cwd, mode, robot_id)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        // 机器人 overlay 作为“附加系统消息”注入，严格补充，不替换主目标模式提示词。
        if let Some(overlay_prompt) = robot_overlay_prompt {
            if !overlay_prompt.trim().is_empty() {
                messages.push(InternalMessage {
                    role: "system".to_string(),
                    content: text_content(overlay_prompt.to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }
        }

        if mode == "plan" {
            if let Some((path, revision, content)) = active_plan_context {
                messages.push(InternalMessage {
                    role: "system".to_string(),
                    content: text_content(build_active_plan_context_prompt(
                        path, revision, content,
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }
        }

        // 某些 chat 网关要求 system 消息必须位于开头，历史中的 system 统一前置。
        let reordered_history = reorder_history_system_messages_for_model(&sanitized_history);

        for msg in reordered_history {
            let internal_tool_calls = msg.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| InternalToolCall {
                        id: tc.id.clone(),
                        call_type: "function".to_string(),
                        function: InternalFunctionCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        },
                    })
                    .collect()
            });

            let content = if msg.role == "assistant"
                && internal_tool_calls.is_some()
                && msg.content.is_empty()
            {
                None
            } else if msg.role == "user"
                && current_user_message_id == Some(msg.id.as_str())
                && attachments
                    .iter()
                    .any(|a| a.mime_type.starts_with("image/"))
            {
                // 只有图片附件需要 multimodal 格式；文档文本已在 content 中持久化
                let image_attachments: Vec<_> = attachments
                    .iter()
                    .filter(|a| a.mime_type.starts_with("image/"))
                    .cloned()
                    .collect();
                Some(multimodal_user_content(&msg.content, &image_attachments))
            } else {
                text_content(msg.content.clone())
            };

            messages.push(InternalMessage {
                role: msg.role.clone(),
                content,
                tool_calls: internal_tool_calls,
                tool_call_id: msg.tool_call_id.clone(),
                name: msg.tool_name.clone(),
            });
        }

        messages
    }

    /// 使用 adapter 执行流式 LLM 调用
    /// wire_api 决定使用哪个 adapter（chat/responses/anthropic/gemini）
    async fn stream_completion(
        &self,
        app_handle: &AppHandle,
        thread_id: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        wire_api: &str,
        messages: Vec<InternalMessage>,
        tools: Option<Vec<serde_json::Value>>,
        max_tokens: Option<i64>,
        iteration: u32,
        plan_mode: bool,
        query_params: Option<&std::collections::HashMap<String, String>>,
        extra_headers: Option<&std::collections::HashMap<String, String>>,
    ) -> AppResult<CompletionResult> {
        // 根据 wire_api 选择 adapter
        let adapter = adapter::get_adapter(wire_api);

        let url = adapter.build_url(base_url, model);
        let headers = adapter.build_headers(api_key);
        let (url, headers) =
            adapter::apply_request_overrides(url, headers, query_params, extra_headers)
                .map_err(AppError::Custom)?;
        let tools_slice = tools.as_deref();
        let body = adapter.build_body(model, &messages, tools_slice, max_tokens);

        info!("LLM request: wire_api={wire_api}, url={url}, model={model}");
        if let Some(ref logger) = self.conversation_logger {
            logger.log_request(
                thread_id,
                iteration,
                model,
                wire_api,
                &messages,
                tools_slice.map(|t| t.len()).unwrap_or(0),
            );
        }
        if tracing::enabled!(tracing::Level::DEBUG) {
            let body_preview = serde_json::to_string(&body)
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect::<String>();
            tracing::debug!("LLM request body (first 500 chars): {body_preview}");
        }

        let request_start = Instant::now();
        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Custom(format!("HTTP request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(AppError::Custom(format!(
                "LLM API error ({status}): {body_text}"
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        info!("LLM response content-type: {content_type}");

        // 如果返回的是非流式 JSON（某些中转站即使请求 stream=true 也返回完整 JSON）
        if content_type.contains("application/json") && !content_type.contains("stream") {
            let body_text = response.text().await.unwrap_or_default();
            info!(
                "Non-streaming JSON response received (first 300 chars): {}",
                truncate_utf8_by_bytes(&body_text, 300)
            );
            return self.parse_non_streaming_chat_response(&body_text, app_handle, thread_id);
        }

        // 流式解析
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
        let mut finish_reason: Option<String> = None;
        let mut usage_info: Option<UsageInfo> = None;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut bytes_read: usize = 0;
        let stream_start = Instant::now();
        let mut plan_buffer = String::new();
        let mut inside_plan_block = false;
        let mut plan_line_buffer = String::new();
        let mut protocol_state = ProtocolStreamState::default();
        let mut dsml_tool_calls: Vec<ToolCallRequest> = Vec::new();

        while let Some(chunk) = stream.next().await {
            if self.is_cancelled() {
                info!("SSE stream cancelled by user");
                finish_reason = Some("interrupted".to_string());
                break;
            }
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let elapsed = stream_start.elapsed();
                    warn!(
                        "Stream read error after {bytes_read} bytes, {:.1}s elapsed: {e}",
                        elapsed.as_secs_f64()
                    );
                    if !full_text.is_empty() || !tool_calls.is_empty() {
                        warn!(
                            "Partial content available ({} chars text, {} tool calls), using as result",
                            full_text.len(),
                            tool_calls.len()
                        );
                        if finish_reason.is_none() {
                            finish_reason = Some("stream_error".to_string());
                        }
                        break;
                    }
                    return Err(AppError::Custom(format!(
                        "Stream read error after {bytes_read} bytes, {:.1}s elapsed: {e}",
                        elapsed.as_secs_f64()
                    )));
                }
            };
            bytes_read += chunk.len();
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                tracing::trace!("SSE line: {}", truncate_utf8_by_bytes(&line, 200));

                // 使用 adapter 检测是否结束
                if adapter.is_stream_done(&line) {
                    if finish_reason.is_none() {
                        finish_reason = Some("stop".to_string());
                    }
                    continue;
                }

                // 使用 adapter 解析 SSE 行
                let events = adapter.parse_stream_line(&line);
                for event in events {
                    match event {
                        StreamEvent::TextDelta(text) => {
                            let parsed = consume_protocol_text_delta(&mut protocol_state, &text);
                            for dsml_block in parsed.dsml_blocks {
                                dsml_tool_calls.extend(parse_dsml_tool_calls_block(&dsml_block));
                            }
                            if !parsed.reasoning.is_empty() {
                                emit_and_broadcast(
                                    app_handle,
                                    "reasoning-text-delta",
                                    serde_json::json!({
                                        "threadId": thread_id,
                                        "delta": parsed.reasoning,
                                    }),
                                );
                            }

                            if parsed.visible.is_empty() {
                                continue;
                            }
                            full_text.push_str(&parsed.visible);
                            if plan_mode {
                                for ch in parsed.visible.chars() {
                                    plan_line_buffer.push(ch);
                                    if ch == '\n' {
                                        let trimmed = plan_line_buffer.trim();
                                        if trimmed == "<proposed_plan>" {
                                            inside_plan_block = true;
                                            plan_line_buffer.clear();
                                            continue;
                                        }
                                        if trimmed == "</proposed_plan>" {
                                            inside_plan_block = false;
                                            plan_line_buffer.clear();
                                            continue;
                                        }
                                        if inside_plan_block {
                                            plan_buffer.push_str(&plan_line_buffer);
                                        } else {
                                            emit_and_broadcast(
                                                app_handle,
                                                "agent-message-delta",
                                                serde_json::json!({ "threadId": thread_id, "delta": &plan_line_buffer }),
                                            );
                                        }
                                        plan_line_buffer.clear();
                                    }
                                }
                            } else {
                                emit_and_broadcast(
                                    app_handle,
                                    "agent-message-delta",
                                    serde_json::json!({ "threadId": thread_id, "delta": parsed.visible }),
                                );
                            }
                        }
                        StreamEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments,
                        } => {
                            while tool_calls.len() <= index {
                                tool_calls.push(ToolCallAccumulator::default());
                            }
                            let acc = &mut tool_calls[index];
                            if let Some(id) = id {
                                acc.id = id;
                            }
                            if let Some(ref name) = name {
                                if !name.is_empty() {
                                    acc.name = name.clone();
                                }
                            }
                            if let Some(args) = arguments {
                                acc.arguments.push_str(&args);
                            }
                        }
                        StreamEvent::Done {
                            finish_reason: reason,
                        } => {
                            finish_reason = reason.or(finish_reason);
                        }
                        StreamEvent::Usage(usage) => {
                            // 累加 usage（Anthropic 分两次返回 input/output tokens）
                            if let Some(ref mut existing) = usage_info {
                                existing.prompt_tokens =
                                    existing.prompt_tokens.max(usage.prompt_tokens);
                                existing.completion_tokens =
                                    existing.completion_tokens.max(usage.completion_tokens);
                                existing.cached_tokens =
                                    existing.cached_tokens.max(usage.cached_tokens);
                                existing.cache_creation_tokens = existing
                                    .cache_creation_tokens
                                    .max(usage.cache_creation_tokens);
                                existing.reasoning_tokens =
                                    existing.reasoning_tokens.max(usage.reasoning_tokens);
                                existing.total_tokens =
                                    existing.prompt_tokens + existing.completion_tokens;
                            } else {
                                usage_info = Some(usage);
                            }
                        }
                    }
                }
            }
        }

        let tail = flush_protocol_stream_state(&mut protocol_state);
        for dsml_block in tail.dsml_blocks {
            dsml_tool_calls.extend(parse_dsml_tool_calls_block(&dsml_block));
        }
        if !tail.reasoning.is_empty() {
            emit_and_broadcast(
                app_handle,
                "reasoning-text-delta",
                serde_json::json!({
                    "threadId": thread_id,
                    "delta": tail.reasoning,
                }),
            );
        }
        if !tail.visible.is_empty() {
            full_text.push_str(&tail.visible);
            if plan_mode {
                for ch in tail.visible.chars() {
                    plan_line_buffer.push(ch);
                    if ch == '\n' {
                        let trimmed = plan_line_buffer.trim();
                        if trimmed == "<proposed_plan>" {
                            inside_plan_block = true;
                            plan_line_buffer.clear();
                            continue;
                        }
                        if trimmed == "</proposed_plan>" {
                            inside_plan_block = false;
                            plan_line_buffer.clear();
                            continue;
                        }
                        if inside_plan_block {
                            plan_buffer.push_str(&plan_line_buffer);
                        } else {
                            emit_and_broadcast(
                                app_handle,
                                "agent-message-delta",
                                serde_json::json!({ "threadId": thread_id, "delta": &plan_line_buffer }),
                            );
                        }
                        plan_line_buffer.clear();
                    }
                }
            } else {
                emit_and_broadcast(
                    app_handle,
                    "agent-message-delta",
                    serde_json::json!({ "threadId": thread_id, "delta": tail.visible }),
                );
            }
        }

        // Plan mode: flush any remaining content in plan_line_buffer
        if plan_mode && !plan_line_buffer.is_empty() {
            if inside_plan_block {
                plan_buffer.push_str(&plan_line_buffer);
            } else {
                emit_and_broadcast(
                    app_handle,
                    "agent-message-delta",
                    serde_json::json!({ "threadId": thread_id, "delta": &plan_line_buffer }),
                );
            }
        }

        let valid_tool_calls = normalize_tool_call_requests(
            tool_calls
                .into_iter()
                .filter(|tc| !tc.name.is_empty())
                .map(|tc| ToolCallRequest {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments,
                })
                .collect(),
        );
        let dsml_tool_calls = normalize_tool_call_requests(dsml_tool_calls);
        let final_tool_calls = if valid_tool_calls.is_empty() {
            dsml_tool_calls
        } else {
            valid_tool_calls
        };

        info!(
            "stream_completion done: wire_api={wire_api}, finish_reason={:?}, tool_calls={}, text_len={}, usage={:?}",
            finish_reason,
            final_tool_calls.len(),
            full_text.len(),
            usage_info,
        );

        // 如果流结束但没有任何内容也没有 finish_reason，可能是连接异常或响应格式不兼容
        if full_text.is_empty() && final_tool_calls.is_empty() && finish_reason.is_none() {
            warn!(
                "Stream ended with no content and no finish_reason. Buffer remainder: {:?}",
                truncate_utf8_by_bytes(&buffer, 200)
            );
            return Err(AppError::Custom(
                "LLM returned empty stream - the provider may not support the current request format. \
                 Try switching wire_api or check the provider's compatibility."
                    .to_string(),
            ));
        }

        // 当 API 不返回 usage 时，基于文本长度估算 token 数
        let usage_info = if usage_info.is_none()
            && (!full_text.is_empty() || !final_tool_calls.is_empty())
        {
            let completion_tokens = estimate_tokens(&full_text)
                + final_tool_calls
                    .iter()
                    .map(|tc| estimate_tokens(&tc.arguments))
                    .sum::<u64>();
            let prompt_tokens = messages
                .iter()
                .map(|m| {
                    let content_len = m
                        .content
                        .as_ref()
                        .map(|c| c.to_string().len() as u64)
                        .unwrap_or(0);
                    estimate_tokens_from_char_count(content_len)
                })
                .sum::<u64>();
            let total_tokens = prompt_tokens + completion_tokens;
            info!(
                "No usage from provider, estimated: prompt={prompt_tokens}, completion={completion_tokens}, total={total_tokens}"
            );
            Some(UsageInfo {
                prompt_tokens,
                completion_tokens,
                total_tokens,
                cached_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
            })
        } else {
            usage_info
        };

        if let Some(ref logger) = self.conversation_logger {
            let duration_ms = request_start.elapsed().as_millis() as u64;
            let log_usage = usage_info
                .as_ref()
                .map(|u| crate::conversation_logger::LogUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                });
            let tc_tuples: Vec<(String, String, String)> = final_tool_calls
                .iter()
                .map(|tc| (tc.id.clone(), tc.name.clone(), tc.arguments.clone()))
                .collect();
            logger.log_response(
                thread_id,
                iteration,
                model,
                wire_api,
                &full_text,
                &tc_tuples,
                log_usage,
                finish_reason.as_deref(),
                duration_ms,
            );
        }

        let plan_text = if plan_mode && !plan_buffer.is_empty() {
            Some(plan_buffer)
        } else {
            None
        };

        if !final_tool_calls.is_empty() {
            Ok(CompletionResult::ToolCalls {
                calls: final_tool_calls,
                preceding_text: full_text,
                usage: usage_info,
            })
        } else {
            Ok(CompletionResult::Message {
                text: full_text,
                usage: usage_info,
                plan_text,
            })
        }
    }

    /// 解析非流式 Chat Completions JSON 响应
    fn parse_non_streaming_chat_response(
        &self,
        body: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<CompletionResult> {
        let json: serde_json::Value = serde_json::from_str(body).map_err(|e| {
            AppError::Custom(format!("Failed to parse non-streaming response: {e}"))
        })?;

        // 检查是否有 error
        if let Some(err) = json.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(AppError::Custom(format!("LLM API error: {msg}")));
        }

        let choices = json.get("choices").and_then(|c| c.as_array());
        let choice = choices.and_then(|arr| arr.first());

        let message = choice.and_then(|c| c.get("message"));
        let raw_text = message
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let parsed_protocol = parse_protocol_text(raw_text);
        let text = parsed_protocol.visible;

        // 提取 usage
        let usage_info = json.get("usage").map(|u| {
            let prompt_details = u.get("prompt_tokens_details");
            let completion_details = u.get("completion_tokens_details");
            UsageInfo {
                prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                completion_tokens: u
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                cached_tokens: prompt_details
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_creation_tokens: prompt_details
                    .and_then(|d| d.get("cache_creation"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                reasoning_tokens: completion_details
                    .and_then(|d| d.get("reasoning_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            }
        });

        // 提取 tool calls
        let tool_calls = normalize_tool_call_requests(
            message
                .and_then(|m| m.get("tool_calls"))
                .and_then(|tc| tc.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|tc| {
                            let func = tc.get("function")?;
                            let name = func.get("name")?.as_str()?.to_string();
                            let arguments = func.get("arguments")?.as_str()?.to_string();
                            Some(ToolCallRequest {
                                id: tc
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                name,
                                arguments,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
        let dsml_tool_calls = normalize_tool_call_requests(
            parsed_protocol
                .dsml_blocks
                .iter()
                .flat_map(|block| parse_dsml_tool_calls_block(block))
                .collect(),
        );
        let final_tool_calls = if tool_calls.is_empty() {
            dsml_tool_calls
        } else {
            tool_calls
        };

        if !parsed_protocol.reasoning.is_empty() {
            emit_and_broadcast(
                app_handle,
                "reasoning-text-delta",
                serde_json::json!({ "threadId": thread_id, "delta": parsed_protocol.reasoning }),
            );
        }

        // 发送文本增量事件
        if !text.is_empty() {
            emit_and_broadcast(
                app_handle,
                "agent-message-delta",
                serde_json::json!({ "threadId": thread_id, "delta": &text }),
            );
        }

        if !final_tool_calls.is_empty() {
            Ok(CompletionResult::ToolCalls {
                calls: final_tool_calls,
                preceding_text: text,
                usage: usage_info,
            })
        } else {
            Ok(CompletionResult::Message {
                text,
                usage: usage_info,
                plan_text: None,
            })
        }
    }
}

fn is_retryable_rate_limit_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("rate_limited")
}

fn rate_limit_backoff_ms(attempt: u32) -> u64 {
    let normalized_attempt = attempt.max(1);
    let shift = normalized_attempt.saturating_sub(1).min(20);
    let multiplier = 1_u64 << shift;
    (1_000_u64.saturating_mul(multiplier)).min(30_000)
}

fn text_expresses_intent(text: &str) -> bool {
    let lower = text.to_lowercase();
    let intent_patterns = [
        "let me ",
        "i'll ",
        "i will ",
        "让我",
        "接下来",
        "我来",
        "我将",
        "我先",
        "查看一下",
        "检查一下",
        "读取一下",
        "看看",
        "分析一下",
    ];
    intent_patterns.iter().any(|p| lower.contains(p))
}

/// 从模型回复中提取机器人节点完成标记，并返回清洗后的文本。
/// 说明：
/// - 标记仅用于流程控制，不应展示给用户；
/// - 允许模型在任意位置输出标记，统一移除后再入库。
/// LLM 调用完成后的结果
enum CompletionResult {
    /// 纯文本回复
    Message {
        text: String,
        usage: Option<UsageInfo>,
        plan_text: Option<String>,
    },
    /// 包含 tool call 的回复
    ToolCalls {
        calls: Vec<ToolCallRequest>,
        preceding_text: String,
        usage: Option<UsageInfo>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitStatusEntry {
    status: String,
    fingerprint: Option<u64>,
}

type GitStatusSnapshot = BTreeMap<String, GitStatusEntry>;

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn add_turn_usage(total: &mut TurnUsage, usage: &UsageInfo) {
    total.prompt_tokens = total.prompt_tokens.saturating_add(usage.prompt_tokens);
    total.completion_tokens = total
        .completion_tokens
        .saturating_add(usage.completion_tokens);
    total.cached_tokens = total.cached_tokens.saturating_add(usage.cached_tokens);
    total.cache_creation_tokens = total
        .cache_creation_tokens
        .saturating_add(usage.cache_creation_tokens);
    total.reasoning_tokens = total
        .reasoning_tokens
        .saturating_add(usage.reasoning_tokens);
    total.total_tokens = if usage.total_tokens > 0 {
        total.total_tokens.saturating_add(usage.total_tokens)
    } else {
        total
            .total_tokens
            .saturating_add(usage.prompt_tokens.saturating_add(usage.completion_tokens))
    };
}

fn nonzero_turn_usage(usage: &TurnUsage) -> Option<TurnUsage> {
    if usage.prompt_tokens == 0
        && usage.completion_tokens == 0
        && usage.total_tokens == 0
        && usage.call_count == 0
    {
        None
    } else {
        Some(usage.clone())
    }
}

fn emit_turn_usage_updated(
    app_handle: &AppHandle,
    thread_id: &str,
    usage: &TurnUsage,
    model_context_window: u64,
) {
    if let Some(current) = nonzero_turn_usage(usage) {
        let prompt_tokens = current.prompt_tokens;
        let completion_tokens = current.completion_tokens;
        let total_tokens = current.total_tokens;
        let call_count = current.call_count;
        let last_single_prompt_tokens = current.last_single_prompt_tokens;
        let context_prompt_tokens = if last_single_prompt_tokens > 0 {
            last_single_prompt_tokens
        } else {
            prompt_tokens
        };
        emit_and_broadcast(
            app_handle,
            "thread-token-usage-updated",
            serde_json::json!({
                "threadId": thread_id,
                "usage": current,
                // Backward-compatible flat fields for existing consumers.
                "inputTokens": prompt_tokens,
                "outputTokens": completion_tokens,
                "totalTokens": total_tokens,
                "callCount": call_count,
                "lastSinglePromptTokens": last_single_prompt_tokens,
                "cachedTokens": current.cached_tokens,
                "cacheCreationTokens": current.cache_creation_tokens,
                "reasoningTokens": current.reasoning_tokens,
                // 稳定提供“上下文占用分子”与“上下文窗口分母”，让 UI 计算不依赖历史回退逻辑。
                "contextPromptTokens": context_prompt_tokens,
                "modelContextWindow": model_context_window,
            }),
        );
    }
}

fn resolve_model_context_window_tokens(config: &ConfigToml) -> u64 {
    let configured = config.model_context_window.unwrap_or(128_000);
    if configured > 0 {
        configured as u64
    } else {
        128_000
    }
}

/// 基于字符串内容估算 token 数（中英文混合约 2-4 chars/token，取 3 折中）
fn estimate_tokens(text: &str) -> u64 {
    let char_count = text.chars().count() as u64;
    estimate_tokens_from_char_count(char_count)
}

fn estimate_tokens_from_char_count(char_count: u64) -> u64 {
    (char_count / 3).max(1)
}

fn truncate_chars_with_marker(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let end = value
        .char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx <= max_chars)
        .last()
        .unwrap_or(0);
    format!("{}...(truncated)", &value[..end])
}

fn read_okf_body_lines_for_recall(path: &Path) -> Option<Vec<String>> {
    let raw_content = std::fs::read_to_string(path).ok()?;
    let content = crate::smartbrain::okf::extract_body(&raw_content);
    Some(content.lines().map(|line| line.to_string()).collect())
}

fn take_first_lines_for_recall(lines: &[String], count: usize) -> Vec<String> {
    lines.iter().take(count).cloned().collect()
}

fn take_last_lines_for_recall(lines: &[String], count: usize) -> Vec<String> {
    if lines.len() <= count {
        return lines.to_vec();
    }
    lines[lines.len() - count..].to_vec()
}

fn build_smartbrain_recall_context(
    memories_dir: &Path,
    result: &crate::smartbrain::search::SmartBrainSearchResult,
    overlap_lines: usize,
    max_chars: usize,
) -> Option<String> {
    let overlap_lines = overlap_lines.max(30);
    let doc_path = memories_dir.join(&result.file_path);
    let current_lines = read_okf_body_lines_for_recall(&doc_path)?;
    if current_lines.is_empty() {
        return None;
    }

    if !result.is_chunk {
        return Some(truncate_chars_with_marker(
            &current_lines.join("\n"),
            max_chars,
        ));
    }

    let parent_doc_id = result.parent_doc_id.as_deref()?.trim();
    if parent_doc_id.is_empty() {
        return Some(truncate_chars_with_marker(
            &current_lines.join("\n"),
            max_chars,
        ));
    }
    let chunk_index = result.chunk_index?;
    let chunk_total = result.chunk_total.unwrap_or(chunk_index).max(chunk_index);
    let docs_dir = memories_dir.join("knowledge").join("docs");
    let previous_lines = if chunk_index > 1 {
        let previous_file =
            crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index - 1);
        read_okf_body_lines_for_recall(&docs_dir.join(previous_file))
    } else {
        None
    };
    let next_lines = if chunk_index < chunk_total {
        let next_file =
            crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index + 1);
        read_okf_body_lines_for_recall(&docs_dir.join(next_file))
    } else {
        None
    };

    if previous_lines.is_none() && next_lines.is_none() {
        return Some(truncate_chars_with_marker(
            &current_lines.join("\n"),
            max_chars,
        ));
    }

    let mut sections = Vec::new();
    if let Some(prev) = previous_lines {
        let mut prev_bridge = Vec::new();
        prev_bridge.push(format!(
            "Previous chunk {} tail:",
            chunk_index.saturating_sub(1)
        ));
        prev_bridge.extend(take_last_lines_for_recall(&prev, overlap_lines));
        prev_bridge.push(format!(
            "Overlap with current chunk {} head ({} lines):",
            chunk_index, overlap_lines
        ));
        prev_bridge.extend(take_first_lines_for_recall(&current_lines, overlap_lines));
        sections.push(prev_bridge.join("\n"));
    }
    sections.push(format!(
        "Current chunk {chunk_index}/{chunk_total}:\n{}",
        current_lines.join("\n")
    ));
    if let Some(next) = next_lines {
        let mut next_bridge = Vec::new();
        next_bridge.push(format!(
            "Overlap with current chunk {} tail ({} lines):",
            chunk_index, overlap_lines
        ));
        next_bridge.extend(take_last_lines_for_recall(&current_lines, overlap_lines));
        next_bridge.push(format!("Next chunk {} head:", chunk_index + 1));
        next_bridge.extend(take_first_lines_for_recall(&next, overlap_lines));
        sections.push(next_bridge.join("\n"));
    }

    let merged = sections.join("\n\n---\n\n");
    Some(truncate_chars_with_marker(&merged, max_chars))
}

fn assistant_is_waiting_for_user(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    const ZH_PATTERNS: &[&str] = &[
        "告诉我需求",
        "请告诉我你的需求",
        "请提供需求",
        "补充需求",
        "你希望我",
        "还想加什么",
        "你还需要什么",
    ];
    if ZH_PATTERNS.iter().any(|pattern| trimmed.contains(pattern)) {
        return true;
    }

    let lowered = trimmed.to_lowercase();
    const EN_PATTERNS: &[&str] = &[
        "tell me your requirements",
        "share your requirements",
        "let me know your requirements",
        "what would you like",
        "please provide more details",
        "please clarify",
        "what changes do you want",
    ];
    if EN_PATTERNS.iter().any(|pattern| lowered.contains(pattern)) {
        return true;
    }

    let question_like = lowered.contains('?') || trimmed.contains('？');
    question_like
        && (lowered.contains("requirements")
            || lowered.contains("feature")
            || lowered.contains("details")
            || lowered.contains("clarify"))
}

#[cfg(test)]
fn turn_budget_limited(goal_budget_tokens: Option<u64>, usage: &TurnUsage) -> bool {
    goal_budget_tokens.is_some_and(|budget| budget > 0 && usage.total_tokens >= budget)
}

fn goal_budget_limited_after(goal: Option<&ThreadGoal>, usage: &TurnUsage) -> bool {
    let Some(goal) = goal else {
        return false;
    };
    goal.token_budget.is_some_and(|budget| {
        budget > 0 && goal.tokens_used.saturating_add(usage.total_tokens) >= budget
    })
}

fn extract_non_streaming_text(body_text: &str) -> Option<String> {
    fn value_to_text(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            serde_json::Value::Array(items) => {
                let mut parts = Vec::new();
                for item in items {
                    if let Some(text) = item
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        parts.push(text.to_string());
                    } else if let Some(text) = item
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        parts.push(text.to_string());
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            }
            _ => None,
        }
    }

    let json: serde_json::Value = serde_json::from_str(body_text).ok()?;
    let candidates = [
        "/choices/0/message/content",
        "/output_text",
        "/output/0/content/0/text",
        "/content/0/text",
        "/candidates/0/content/parts/0/text",
    ];
    for pointer in candidates {
        if let Some(value) = json.pointer(pointer) {
            if let Some(text) = value_to_text(value) {
                return Some(text);
            }
        }
    }
    None
}

fn multimodal_user_content(text: &str, attachments: &[UserAttachment]) -> serde_json::Value {
    let mut combined_text = text.to_string();
    let mut image_parts: Vec<serde_json::Value> = Vec::new();

    for attachment in attachments {
        if attachment.mime_type.starts_with("image/") && attachment.data_url.starts_with("data:") {
            image_parts.push(serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": attachment.data_url,
                    "detail": "high"
                }
            }));
        } else {
            match crate::document_parser::parse_document(
                &attachment.mime_type,
                &attachment.data_url,
            ) {
                Ok(extracted) => {
                    combined_text.push_str(&format!(
                        "\n\n[Attachment: {}]\n{}",
                        attachment.name, extracted
                    ));
                }
                Err(e) => {
                    warn!("Document parse failed for {}: {e}", attachment.name);
                    combined_text.push_str(&format!(
                        "\n\n[Attachment: {} ({}, {} bytes) - content extraction failed]",
                        attachment.name, attachment.mime_type, attachment.size
                    ));
                }
            }
        }
    }

    // 只有当存在图片时才使用 multimodal 数组格式，否则用纯文本（兼容不支持 multimodal 的 API）
    if image_parts.is_empty() {
        serde_json::Value::String(combined_text)
    } else {
        let mut parts = vec![serde_json::json!({
            "type": "text",
            "text": combined_text
        })];
        parts.extend(image_parts);
        serde_json::Value::Array(parts)
    }
}

fn normalize_tool_call_requests(calls: Vec<ToolCallRequest>) -> Vec<ToolCallRequest> {
    calls
        .into_iter()
        .enumerate()
        .filter_map(|(index, call)| {
            let name = call.name.trim().to_string();
            if name.is_empty() {
                return None;
            }

            let id = if call.id.trim().is_empty() {
                format!("call_{}_{}", sanitize_tool_name(&name), index)
            } else {
                call.id
            };

            Some(ToolCallRequest {
                id,
                name,
                arguments: call.arguments,
            })
        })
        .collect()
}

fn sanitize_tool_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}

fn reorder_history_system_messages_for_model<'a>(
    history: &'a [ThreadMessage],
) -> Vec<&'a ThreadMessage> {
    let mut system_messages = Vec::new();
    let mut non_system_messages = Vec::new();
    for msg in history {
        if msg.role == "system" {
            system_messages.push(msg);
        } else {
            non_system_messages.push(msg);
        }
    }
    system_messages.extend(non_system_messages);
    system_messages
}

fn sanitize_history_for_model(history: &[ThreadMessage]) -> Vec<ThreadMessage> {
    let mut seen_tool_call_ids: HashSet<String> = HashSet::new();
    let mut sanitized = Vec::with_capacity(history.len());

    for msg in history {
        let filtered_tool_calls = msg.tool_calls.as_ref().map(|tool_calls| {
            tool_calls
                .iter()
                .filter(|call| !call.id.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
        });

        if let Some(tool_calls) = &filtered_tool_calls {
            for call in tool_calls {
                seen_tool_call_ids.insert(call.id.clone());
            }
        }

        if msg.role == "assistant"
            && msg.content.trim().is_empty()
            && filtered_tool_calls
                .as_ref()
                .is_some_and(|tool_calls| tool_calls.is_empty())
        {
            continue;
        }

        if msg.role == "tool" {
            let Some(tool_call_id) = msg
                .tool_call_id
                .as_ref()
                .map(|id| id.trim())
                .filter(|id| !id.is_empty())
            else {
                continue;
            };

            if !seen_tool_call_ids.contains(tool_call_id) {
                continue;
            }
        }

        let mut sanitized_msg = msg.clone();
        sanitized_msg.tool_calls = filtered_tool_calls.filter(|tool_calls| !tool_calls.is_empty());
        sanitized.push(sanitized_msg);
    }

    sanitized
}

fn tool_call_hook_context(turn_id: &str, call: &ToolCallRequest) -> serde_json::Value {
    let parsed_args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    let command_display = parsed_args
        .as_ref()
        .and_then(|value| value.get("command"))
        .map(|command| {
            if let Some(items) = command.as_array() {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                command.as_str().unwrap_or_default().to_string()
            }
        })
        .filter(|command| !command.trim().is_empty());

    serde_json::json!({
        "turnId": turn_id,
        "toolCallId": &call.id,
        "toolName": &call.name,
        "arguments": parsed_args.unwrap_or_else(|| serde_json::Value::String(call.arguments.clone())),
        "command": command_display,
    })
}

fn tool_result_hook_context(
    turn_id: &str,
    call: &ToolCallRequest,
    output: &str,
    success: bool,
) -> serde_json::Value {
    let parsed_args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    let command_display = parsed_args
        .as_ref()
        .and_then(|value| value.get("command"))
        .map(|command| {
            if let Some(items) = command.as_array() {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                command.as_str().unwrap_or_default().to_string()
            }
        })
        .filter(|command| !command.trim().is_empty());

    serde_json::json!({
        "turnId": turn_id,
        "toolCallId": &call.id,
        "toolName": &call.name,
        "arguments": parsed_args.unwrap_or_else(|| serde_json::Value::String(call.arguments.clone())),
        "command": command_display,
        "success": success,
        "output": output,
    })
}

fn blocked_tool_call_output(tool_name: &str, hook: &HookRunResult) -> String {
    let reason = hook
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("blocked by hook");
    format!(
        "Tool call blocked by PreToolUse hook: {reason}. Tool: {tool_name}. Hook: {} ({})",
        hook.command, hook.source_name
    )
}

fn user_prompt_submit_hook_context(
    turn_id: &str,
    mode: &str,
    cwd: &Path,
    model: &str,
    prompt: &str,
) -> serde_json::Value {
    serde_json::json!({
        "hookEventName": "UserPromptSubmit",
        "turnId": turn_id,
        "mode": mode,
        "cwd": cwd.to_string_lossy(),
        "model": model,
        "permissionMode": "default",
        "prompt": prompt,
    })
}

fn format_user_prompt_submit_hook_feedback(feedback: Vec<String>, blocked: bool) -> String {
    let heading = if blocked {
        "[UserPromptSubmit hook blocked prompt]"
    } else {
        "[UserPromptSubmit hook context]"
    };
    format!("{heading}\n{}", feedback.join("\n"))
}

#[allow(clippy::too_many_arguments)]
fn stop_hook_context(
    turn_id: &str,
    mode: &str,
    cwd: &Path,
    duration_ms: u64,
    changed_files: &[FileChange],
    usage: &TurnUsage,
    goal_budget_tokens: Option<u64>,
    budget_limited: bool,
    model: &str,
    stop_hook_active: bool,
    last_assistant_message: &str,
) -> serde_json::Value {
    let last_assistant_message = if last_assistant_message.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(last_assistant_message.to_string())
    };

    serde_json::json!({
        "hookEventName": "Stop",
        "turnId": turn_id,
        "mode": mode,
        "cwd": cwd.to_string_lossy(),
        "durationMs": duration_ms,
        "changedFiles": changed_files,
        "usage": nonzero_turn_usage(usage),
        "goalBudgetTokens": goal_budget_tokens,
        "budgetLimited": budget_limited,
        "model": model,
        "permissionMode": "default",
        "stopHookActive": stop_hook_active,
        "lastAssistantMessage": last_assistant_message,
    })
}

fn stop_hook_continuation_message(results: &[HookRunResult]) -> Option<String> {
    let feedback = hook_feedback_for_model(results);
    if feedback.is_empty() {
        return None;
    }

    Some(format!("[Stop hook continuation]\n{}", feedback.join("\n")))
}

fn subagent_stop_hook_context(
    turn_id: &str,
    mode: &str,
    cwd: &Path,
    model: &str,
    call: &ToolCallRequest,
    close_output: &str,
) -> serde_json::Value {
    let args = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    let close_result = serde_json::from_str::<serde_json::Value>(close_output)
        .unwrap_or_else(|_| serde_json::Value::String(close_output.to_string()));
    let agent = close_result
        .get("agent")
        .and_then(serde_json::Value::as_object);

    let agent_id = agent
        .and_then(|agent| agent.get("id"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            close_result
                .get("target")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            args.as_ref()
                .and_then(|args| args.get("target"))
                .or_else(|| args.as_ref().and_then(|args| args.get("agent_id")))
                .or_else(|| args.as_ref().and_then(|args| args.get("id")))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("unknown");
    let agent_type = agent
        .and_then(|agent| agent.get("role"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("subagent");
    let last_assistant_message = agent
        .and_then(|agent| agent.get("output"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let agent_transcript_path = cwd
        .join("codey")
        .join("subagents")
        .join(agent_id)
        .join("last-message.txt");

    serde_json::json!({
        "hookEventName": "SubagentStop",
        "turnId": turn_id,
        "mode": mode,
        "cwd": cwd.to_string_lossy(),
        "model": model,
        "permissionMode": "default",
        "stopHookActive": false,
        "agentId": agent_id,
        "agent_id": agent_id,
        "agentType": agent_type,
        "agent_type": agent_type,
        "agentTranscriptPath": agent_transcript_path,
        "agent_transcript_path": agent_transcript_path,
        "lastAssistantMessage": last_assistant_message
            .map(|value| serde_json::Value::String(value.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "closeResult": close_result,
    })
}

fn append_subagent_stop_hook_feedback(output: String, feedback: Vec<String>) -> String {
    if feedback.is_empty() {
        return output;
    }

    let mut combined = output;
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str("\n[SubagentStop hook feedback]\n");
    combined.push_str(&feedback.join("\n"));
    combined
}

fn append_post_tool_hook_feedback(output: String, feedback: Vec<String>) -> String {
    if feedback.is_empty() {
        return output;
    }

    let mut combined = output;
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str("\n[PostToolUse hook feedback]\n");
    combined.push_str(&feedback.join("\n"));
    combined
}

fn normalize_change_path(path: &str) -> String {
    path.trim().replace('\\', "/")
}

fn upsert_file_snapshot_entry<'a>(
    snapshot_map: &'a mut BTreeMap<String, FileChangeSnapshot>,
    change: &FileChange,
) -> &'a mut FileChangeSnapshot {
    let path = normalize_change_path(&change.path);
    snapshot_map
        .entry(path.clone())
        .or_insert_with(|| FileChangeSnapshot {
            path,
            action: change.action.clone(),
            before_content: None,
            after_content: None,
        })
}

fn capture_before_file_snapshots(
    snapshot_map: &mut BTreeMap<String, FileChangeSnapshot>,
    changes: &[FileChange],
    cwd: &Path,
) {
    for change in changes {
        let entry = upsert_file_snapshot_entry(snapshot_map, change);
        entry.action = change.action.clone();
        // before 只采集第一次，确保“本轮起始基线”稳定，不被后续同文件多次修改覆盖。
        if entry.before_content.is_none() {
            entry.before_content = read_text_file_snapshot(cwd, &entry.path);
        }
    }
}

fn capture_after_file_snapshots(
    snapshot_map: &mut BTreeMap<String, FileChangeSnapshot>,
    changes: &[FileChange],
    cwd: &Path,
) {
    for change in changes {
        let entry = upsert_file_snapshot_entry(snapshot_map, change);
        entry.action = change.action.clone();
        // deleted 文件在执行后不应再读取磁盘，after 显式置空。
        entry.after_content = if change.action == "deleted" {
            None
        } else {
            read_text_file_snapshot(cwd, &entry.path)
        };
    }
}

fn build_changed_file_snapshots(
    changed_files: &[FileChange],
    snapshot_map: &BTreeMap<String, FileChangeSnapshot>,
    cwd: &Path,
) -> Vec<FileChangeSnapshot> {
    changed_files
        .iter()
        .map(|change| {
            let normalized_path = normalize_change_path(&change.path);
            if let Some(snapshot) = snapshot_map.get(&normalized_path) {
                let mut next = snapshot.clone();
                next.action = change.action.clone();
                next.path = normalized_path;
                return next;
            }

            // 兜底：如果某条 changedFiles 没有命令级快照（例如仅由 git merge 补入），
            // 仍给前端一份最小 after 预览，避免 Diff 按钮完全无数据。
            FileChangeSnapshot {
                path: normalized_path.clone(),
                action: change.action.clone(),
                before_content: None,
                after_content: if change.action == "deleted" {
                    None
                } else {
                    read_text_file_snapshot(cwd, &normalized_path)
                },
            }
        })
        .collect()
}

fn read_text_file_snapshot(cwd: &Path, path: &str) -> Option<String> {
    let raw = path.trim();
    if raw.is_empty() {
        return None;
    }

    let resolved = {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            cwd.join(candidate)
        }
    };

    if !resolved.is_file() {
        return None;
    }

    let bytes = std::fs::read(&resolved).ok()?;
    let slice = if bytes.len() > MAX_CHANGED_FILE_SNAPSHOT_BYTES {
        &bytes[..MAX_CHANGED_FILE_SNAPSHOT_BYTES]
    } else {
        &bytes[..]
    };
    // 简单二进制过滤：包含 NUL 字节时视为不可读文本。
    if slice.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(slice).to_string())
}

async fn handle_update_goal(
    thread_store: &ThreadStore,
    app_handle: &AppHandle,
    thread_id: &str,
    arguments: &str,
) -> Result<GoalUpdateOutcome, String> {
    let args: serde_json::Value =
        serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;
    let status_str = args
        .get("status")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'status' parameter".to_string())?;
    let goal_status = match status_str {
        "complete" => ThreadGoalStatus::Complete,
        "blocked" => ThreadGoalStatus::Blocked,
        other => return Err(format!("unknown status: {other}")),
    };
    let goal = thread_store
        .set_thread_goal_status(thread_id, goal_status)
        .await
        .map_err(|e| format!("failed to update goal status: {e}"))?;
    emit_and_broadcast(
        app_handle,
        "thread-goal-updated",
        serde_json::json!({
            "threadId": thread_id,
            "goal": goal.clone(),
        }),
    );
    Ok(GoalUpdateOutcome {
        message: format!("Goal status updated to '{status_str}'."),
        goal,
    })
}

fn file_changes_from_tool_call(call: &ToolCallRequest) -> Vec<FileChange> {
    match call.name.as_str() {
        "write_file" => write_file_change_from_args(&call.arguments)
            .into_iter()
            .collect(),
        "apply_patch" => apply_patch_changes_from_args(&call.arguments),
        // shell 类工具在非 git 工作区或跨目录写盘时，git 快照可能拿不到变更；
        // 这里补一层“命令参数级”识别，尽量恢复 RunSummary changedFiles 的可见性。
        "shell" | "shell_command" | "exec_command" => shell_changes_from_args(&call.arguments),
        _ => Vec::new(),
    }
}

fn write_file_change_from_args(arguments: &str) -> Option<FileChange> {
    let parsed: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let path = parsed.get("path")?.as_str()?.trim();
    if path.is_empty() {
        return None;
    }

    Some(FileChange {
        path: path.replace('\\', "/"),
        action: "modified".to_string(),
    })
}

fn apply_patch_changes_from_args(arguments: &str) -> Vec<FileChange> {
    let Some(patch) = patch_body_from_tool_args(arguments) else {
        return Vec::new();
    };

    let mut changes = Vec::new();
    let mut pending_update: Option<usize> = None;
    for line in patch.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            changes.push(FileChange {
                path: path.trim().replace('\\', "/"),
                action: "created".to_string(),
            });
            pending_update = None;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            changes.push(FileChange {
                path: path.trim().replace('\\', "/"),
                action: "modified".to_string(),
            });
            pending_update = Some(changes.len() - 1);
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            changes.push(FileChange {
                path: path.trim().replace('\\', "/"),
                action: "deleted".to_string(),
            });
            pending_update = None;
        } else if let Some(dest) = line.strip_prefix("*** Move to: ") {
            if let Some(idx) = pending_update {
                changes[idx].path = dest.trim().replace('\\', "/");
                changes[idx].action = "renamed".to_string();
            }
        }
    }

    changes
}

fn shell_changes_from_args(arguments: &str) -> Vec<FileChange> {
    let Some(command) = shell_command_from_args(arguments) else {
        return Vec::new();
    };
    let vars = shell_extract_variable_assignments(&command);
    let tokens = shell_command_tokens(&command);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut changes = Vec::new();
    for (idx, token) in tokens.iter().enumerate() {
        let lowered = token.to_ascii_lowercase();
        match lowered.as_str() {
            // 明确写盘命令：默认按 modified 上报。
            "set-content" | "add-content" | "out-file" => {
                if let Some(path) = shell_flag_value_with_vars(
                    &tokens,
                    idx,
                    &["-path", "-literalpath", "-filepath"],
                    &vars,
                ) {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
            // 删除命令：标记 deleted。
            "remove-item" | "del" | "erase" | "rm" => {
                if let Some(path) =
                    shell_flag_value_with_vars(&tokens, idx, &["-path", "-literalpath"], &vars)
                {
                    push_shell_change(&mut changes, &path, "deleted");
                }
            }
            // 移动命令：目标路径视为 renamed。
            "move-item" | "mv" | "move" => {
                if let Some(path) = shell_flag_value_with_vars(
                    &tokens,
                    idx,
                    &["-destination", "-dest", "-path"],
                    &vars,
                ) {
                    push_shell_change(&mut changes, &path, "renamed");
                }
            }
            // 复制命令：目标路径按 modified 处理（新建/覆盖都可归并为可见变更）。
            "copy-item" | "copy" | "cp" => {
                if let Some(path) = shell_flag_value_with_vars(
                    &tokens,
                    idx,
                    &["-destination", "-dest", "-path"],
                    &vars,
                ) {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
            // 处理显式重定向：`>` / `>>` / `1>` / `1>>` / `2>` / `2>>`。
            ">" | ">>" | "1>" | "1>>" | "2>" | "2>>" => {
                if let Some(path) =
                    shell_path_candidate_with_vars(tokens.get(idx + 1).map(String::as_str), &vars)
                {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
            _ => {
                // 处理无空格写法：例如 `>D:\a.txt` 或 `1>>out.log`。
                if let Some(path) = shell_redirection_target(token) {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
        }
    }

    changes
}

fn shell_command_from_args(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }

    let parsed: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    if let Some(command) = parsed.get("command") {
        if let Some(value) = command.as_str() {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        if let Some(array) = command.as_array() {
            let merged = array
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            if !merged.trim().is_empty() {
                return Some(merged);
            }
        }
    }

    parsed
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn shell_command_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in command.chars() {
        match quote {
            Some(marker) => {
                if ch == marker {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    continue;
                }
                if ch.is_whitespace() {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn shell_redirection_target(token: &str) -> Option<String> {
    const PREFIXES: [&str; 6] = ["1>>", "1>", "2>>", "2>", ">>", ">"];
    for prefix in PREFIXES {
        if let Some(rest) = token.strip_prefix(prefix) {
            return shell_path_candidate(Some(rest));
        }
    }
    None
}

fn shell_path_candidate(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    // 过滤变量/参数位占位，避免把 `$path`、`-Force` 这类值当成文件。
    if raw.starts_with('$') || raw.starts_with('-') || raw.starts_with('&') {
        return None;
    }

    let trimmed = raw
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim_end_matches(|c: char| matches!(c, ';' | ',' | ')' | '('))
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.replace('\\', "/"))
}

/// 与 `shell_path_candidate` 相同逻辑，但当遇到 `$var` 时尝试从变量表中解析。
fn shell_path_candidate_with_vars(raw: Option<&str>, vars: &[(String, String)]) -> Option<String> {
    if let Some(result) = shell_path_candidate(raw) {
        return Some(result);
    }
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    // 尝试变量解析：`$varName` → 查找变量表
    if let Some(var_name) = raw.strip_prefix('$') {
        let var_name_lower = var_name.to_ascii_lowercase();
        for (name, value) in vars {
            if name.to_ascii_lowercase() == var_name_lower {
                return shell_path_candidate(Some(value.as_str()));
            }
        }
    }
    None
}

/// 与 `shell_flag_value` 相同，但使用 `shell_path_candidate_with_vars` 做路径解析。
fn shell_flag_value_with_vars(
    tokens: &[String],
    start_idx: usize,
    flags: &[&str],
    vars: &[(String, String)],
) -> Option<String> {
    let mut idx = start_idx + 1;
    while idx < tokens.len() {
        let lowered = tokens[idx].to_ascii_lowercase();
        if lowered == "|" || lowered == ";" {
            break;
        }

        if flags.iter().any(|flag| *flag == lowered.as_str()) {
            return shell_path_candidate_with_vars(tokens.get(idx + 1).map(String::as_str), vars);
        }

        if let Some((flag, value)) = tokens[idx].split_once('=') {
            let lowered_flag = flag.to_ascii_lowercase();
            if flags.iter().any(|item| *item == lowered_flag.as_str()) {
                return shell_path_candidate_with_vars(Some(value), vars);
            }
        }
        idx += 1;
    }
    None
}

/// 从多行 shell 命令文本中提取简单的 PowerShell 变量赋值。
/// 识别形如 `$varName = "value"` 或 `$varName = 'value'` 的模式。
fn shell_extract_variable_assignments(command: &str) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    for line in command.lines() {
        let trimmed = line.trim();
        // 匹配 $name = "..." 或 $name = '...'
        if let Some(rest) = trimmed.strip_prefix('$') {
            if let Some(eq_pos) = rest.find('=') {
                let var_name = rest[..eq_pos].trim().to_string();
                if var_name.is_empty() || var_name.contains(' ') || var_name.contains('(') {
                    continue;
                }
                let value_raw = rest[eq_pos + 1..].trim();
                let value = value_raw
                    .trim_matches(|c| c == '"' || c == '\'')
                    .trim()
                    .to_string();
                if !value.is_empty() {
                    vars.push((var_name, value));
                }
            }
        }
    }
    vars
}

fn push_shell_change(changes: &mut Vec<FileChange>, path: &str, action: &str) {
    if path.trim().is_empty() {
        return;
    }

    if let Some(existing) = changes
        .iter_mut()
        .find(|item| paths_match(&item.path, path))
    {
        existing.action = action.to_string();
        return;
    }

    changes.push(FileChange {
        path: path.to_string(),
        action: action.to_string(),
    });
}

fn patch_body_from_tool_args(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.starts_with("*** Begin Patch") {
        return Some(trimmed.to_string());
    }

    let parsed: serde_json::Value = serde_json::from_str(arguments).ok()?;
    parsed
        .get("patch")
        .or_else(|| parsed.get("command"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn paths_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let na = a.replace('\\', "/");
    let nb = b.replace('\\', "/");
    if na == nb {
        return true;
    }
    let sa = na.trim_start_matches('/');
    let sb = nb.trim_start_matches('/');
    sa.ends_with(sb) || sb.ends_with(sa)
}

fn build_goal_continuation_prompt(goal: &ThreadGoal) -> String {
    let budget_info = if let Some(budget) = goal.token_budget {
        let remaining = budget.saturating_sub(goal.tokens_used);
        format!(
            "Tokens used: {}, budget: {}, remaining: {}.",
            goal.tokens_used, budget, remaining
        )
    } else {
        format!("Tokens used: {}.", goal.tokens_used)
    };
    format!(
        "Continue working toward the active thread goal.\n\n\
         <objective>\n{}\n</objective>\n\n\
         {budget_info}\n\n\
         Keep working through the available tools until the objective is genuinely handled.\n\
         If the objective is achieved and no required work remains, call update_goal with \
         status \"complete\".\n\
         If the same blocking condition has repeated for at least three consecutive goal turns \
         and you cannot make progress, call update_goal with status \"blocked\".\n\
         Do not call update_goal unless the goal is truly complete or the strict blocked \
         threshold above is satisfied.",
        goal.objective,
    )
}

fn is_internal_runtime_file(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let p = normalized.trim_start_matches('/');
    const PATTERNS: &[&str] = &[
        "codey/usage",
        "codey/sessions/",
        "codey/config.toml",
        "codey/memories/",
    ];
    PATTERNS.iter().any(|pat| p.contains(pat))
}

fn push_file_change(changes: &mut Vec<FileChange>, change: FileChange) {
    if change.path.trim().is_empty() || is_internal_runtime_file(&change.path) {
        return;
    }

    if let Some(existing) = changes
        .iter_mut()
        .find(|item| paths_match(&item.path, &change.path))
    {
        existing.action = change.action;
        return;
    }

    changes.push(change);
}

fn merge_git_changes(
    changes: &mut Vec<FileChange>,
    before: &GitStatusSnapshot,
    after: &GitStatusSnapshot,
) {
    for (path, entry) in after {
        if before.get(path) == Some(entry) {
            continue;
        }

        push_file_change(
            changes,
            FileChange {
                path: path.clone(),
                action: git_status_to_action(&entry.status),
            },
        );
    }

    for (path, entry) in before {
        if after.contains_key(path) {
            continue;
        }

        push_file_change(
            changes,
            FileChange {
                path: path.clone(),
                action: git_status_disappeared_to_action(&entry.status),
            },
        );
    }
}

fn git_status_to_action(status: &str) -> String {
    if status.contains('D') {
        "deleted".to_string()
    } else if status.contains('A') || status == "??" {
        "created".to_string()
    } else if status.contains('R') {
        "renamed".to_string()
    } else {
        "modified".to_string()
    }
}

fn git_status_disappeared_to_action(status: &str) -> String {
    if status == "??" || status.contains('A') {
        "deleted".to_string()
    } else if status.contains('D') {
        "created".to_string()
    } else {
        "modified".to_string()
    }
}

async fn git_status_snapshot(cwd: &Path) -> GitStatusSnapshot {
    let mut cmd = Command::new("git");
    cmd.args(["status", "--porcelain"]).current_dir(cwd);
    #[cfg(windows)]
    cmd.no_console();
    let Ok(output) = cmd.output().await else {
        return GitStatusSnapshot::new();
    };

    if !output.status.success() {
        return GitStatusSnapshot::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(parse_git_status_line)
        .map(|(path, status)| {
            let fingerprint = file_fingerprint(cwd, &path);
            (
                path,
                GitStatusEntry {
                    status,
                    fingerprint,
                },
            )
        })
        .collect()
}

fn file_fingerprint(cwd: &Path, path: &str) -> Option<u64> {
    let path = cwd.join(path);
    if !path.is_file() {
        return None;
    }

    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

fn parse_git_status_line(line: &str) -> Option<(String, String)> {
    if line.len() < 4 {
        return None;
    }

    let status = line.get(0..2)?.trim().to_string();
    let raw_path = line.get(3..)?.trim();
    if raw_path.is_empty() {
        return None;
    }

    let path = raw_path
        .rsplit(" -> ")
        .next()
        .unwrap_or(raw_path)
        .trim_matches('"')
        .replace('\\', "/");

    Some((path, status))
}

fn parse_skill_prompt_frontmatter(content: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();

    if !content.starts_with("---") {
        return (name, description);
    }

    let Some(end) = content[3..].find("---") else {
        return (name, description);
    };

    let frontmatter = &content[3..3 + end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        }
    }

    (name, description)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{thread_store::ThreadStore, tool_executor::ToolExecutor};

    fn status_entry(status: &str, fingerprint: Option<u64>) -> GitStatusEntry {
        GitStatusEntry {
            status: status.to_string(),
            fingerprint,
        }
    }

    fn test_thread_message(id: &str, role: &str, content: &str) -> ThreadMessage {
        ThreadMessage {
            id: id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: 0,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn extract_proposed_plan_supports_inline_tags() {
        let text = "intro<proposed_plan>\n# Plan\n- step 1\n</proposed_plan>tail";
        assert_eq!(
            extract_proposed_plan(text),
            Some("# Plan\n- step 1".to_string())
        );
    }

    #[test]
    fn resolve_effective_plan_content_prefers_stream_plan_text() {
        let resolved = resolve_effective_plan_content(
            "plan",
            Some("  from stream  "),
            "<proposed_plan>from tag</proposed_plan>",
            "fallback",
        );
        assert_eq!(resolved, Some("from stream".to_string()));
    }

    #[test]
    fn resolve_effective_plan_content_uses_tagged_content() {
        let resolved = resolve_effective_plan_content(
            "plan",
            None,
            "prefix\n<proposed_plan>\n## Title\n1. one\n</proposed_plan>\nsuffix",
            "fallback",
        );
        assert_eq!(resolved, Some("## Title\n1. one".to_string()));
    }

    #[test]
    fn resolve_effective_plan_content_falls_back_to_cleaned_text_for_plan_mode() {
        let resolved =
            resolve_effective_plan_content("plan", None, "No tags here", "  plain markdown  ");
        assert_eq!(resolved, Some("plain markdown".to_string()));
    }

    #[test]
    fn resolve_effective_plan_content_keeps_non_plan_behavior() {
        let resolved = resolve_effective_plan_content(
            "chat",
            None,
            "<proposed_plan>ignored</proposed_plan>",
            "should-not-be-plan",
        );
        assert_eq!(resolved, None);
    }

    #[test]
    fn rate_limit_backoff_ms_grows_exponentially_and_caps() {
        assert_eq!(rate_limit_backoff_ms(1), 1_000);
        assert_eq!(rate_limit_backoff_ms(2), 2_000);
        assert_eq!(rate_limit_backoff_ms(3), 4_000);
        assert_eq!(rate_limit_backoff_ms(4), 8_000);
        assert_eq!(rate_limit_backoff_ms(5), 16_000);
        assert_eq!(rate_limit_backoff_ms(6), 30_000);
        assert_eq!(rate_limit_backoff_ms(10), 30_000);
    }

    #[test]
    fn is_retryable_rate_limit_error_detects_429_and_rate_limit_text() {
        assert!(is_retryable_rate_limit_error(
            "LLM API error (429 Too Many Requests): overload"
        ));
        assert!(is_retryable_rate_limit_error("rate limit exceeded"));
        assert!(!is_retryable_rate_limit_error(
            "LLM API error (500): internal"
        ));
    }

    #[test]
    fn parse_protocol_text_extracts_think_blocks() {
        let parsed = parse_protocol_text("前文<think>推理过程</think>后文");
        assert_eq!(parsed.visible, "前文后文");
        assert_eq!(parsed.reasoning, "推理过程");
        assert!(parsed.dsml_blocks.is_empty());
    }

    #[test]
    fn parse_protocol_text_strips_dsml_block() {
        let parsed = parse_protocol_text(
            "before<｜｜DSML｜｜tool_calls><｜｜DSML｜｜invoke name=\"request_user_input\"></｜｜DSML｜｜invoke></｜｜DSML｜｜tool_calls>after",
        );
        assert_eq!(parsed.visible, "beforeafter");
        assert_eq!(parsed.dsml_blocks.len(), 1);
    }

    #[test]
    fn parse_dsml_tool_calls_block_parses_parameters() {
        let block = "<｜｜DSML｜｜invoke name=\"request_user_input\">\n<｜｜DSML｜｜parameter name=\"questions\" string=\"false\">[{\"id\":\"q1\",\"prompt\":\"继续吗?\",\"options\":[{\"id\":\"yes\",\"label\":\"继续\"}]}]</｜｜DSML｜｜parameter>\n</｜｜DSML｜｜invoke>";
        let calls = parse_dsml_tool_calls_block(block);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "request_user_input");
        let arguments: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(arguments["questions"][0]["id"], "q1");
    }

    #[test]
    fn consume_protocol_text_delta_supports_fragmented_think_tags() {
        let mut state = ProtocolStreamState::default();
        let mut visible = String::new();
        let mut reasoning = String::new();
        for chunk in ["前文<th", "ink>思", "考</thi", "nk>后文"] {
            let parsed = consume_protocol_text_delta(&mut state, chunk);
            visible.push_str(&parsed.visible);
            reasoning.push_str(&parsed.reasoning);
        }
        let tail = flush_protocol_stream_state(&mut state);
        visible.push_str(&tail.visible);
        reasoning.push_str(&tail.reasoning);
        assert_eq!(visible, "前文后文");
        assert_eq!(reasoning, "思考");
    }

    #[test]
    fn merge_git_changes_detects_content_change_for_existing_dirty_file() {
        let mut changes = Vec::new();
        let before = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(10)))]);
        let after = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(11)))]);

        merge_git_changes(&mut changes, &before, &after);

        assert_eq!(
            changes,
            vec![FileChange {
                path: "src/app.ts".to_string(),
                action: "modified".to_string(),
            }]
        );
    }

    #[test]
    fn merge_git_changes_ignores_unchanged_dirty_file() {
        let mut changes = Vec::new();
        let before = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(10)))]);
        let after = BTreeMap::from([("src/app.ts".to_string(), status_entry("M", Some(10)))]);

        merge_git_changes(&mut changes, &before, &after);

        assert!(changes.is_empty());
    }

    #[test]
    fn file_changes_from_apply_patch_tool_call_detects_changed_files() {
        let call = ToolCallRequest {
            id: "call-1".to_string(),
            name: "apply_patch".to_string(),
            arguments: serde_json::json!({
                "patch": "*** Begin Patch\n*** Add File: src/new.ts\n+hello\n*** Update File: src/old.ts\n@@\n-old\n+new\n*** Delete File: src/gone.ts\n*** End Patch"
            })
            .to_string(),
        };

        assert_eq!(
            file_changes_from_tool_call(&call),
            vec![
                FileChange {
                    path: "src/new.ts".to_string(),
                    action: "created".to_string(),
                },
                FileChange {
                    path: "src/old.ts".to_string(),
                    action: "modified".to_string(),
                },
                FileChange {
                    path: "src/gone.ts".to_string(),
                    action: "deleted".to_string(),
                },
            ]
        );
    }

    #[test]
    fn file_changes_from_apply_patch_tool_call_accepts_raw_patch_text() {
        let call = ToolCallRequest {
            id: "call-raw".to_string(),
            name: "apply_patch".to_string(),
            arguments: "*** Begin Patch\n*** Add File: src/raw.ts\n+hello\n*** End Patch"
                .to_string(),
        };

        assert_eq!(
            file_changes_from_tool_call(&call),
            vec![FileChange {
                path: "src/raw.ts".to_string(),
                action: "created".to_string(),
            }]
        );
    }

    #[test]
    fn file_changes_from_shell_tool_call_detects_set_content_write() {
        let call = ToolCallRequest {
            id: "call-shell-write".to_string(),
            name: "shell".to_string(),
            arguments: serde_json::json!({
                "command": "Set-Content -Path 'D:\\cncodetest\\index.html' -Value '<title>BBB</title>'"
            })
            .to_string(),
        };

        assert_eq!(
            file_changes_from_tool_call(&call),
            vec![FileChange {
                path: "D:/cncodetest/index.html".to_string(),
                action: "modified".to_string(),
            }]
        );
    }

    #[test]
    fn file_changes_from_shell_tool_call_detects_redirection_target() {
        let call = ToolCallRequest {
            id: "call-shell-redirect".to_string(),
            name: "shell".to_string(),
            arguments: serde_json::json!({
                "command": "echo hello > D:\\cncodetest\\output.txt"
            })
            .to_string(),
        };

        assert_eq!(
            file_changes_from_tool_call(&call),
            vec![FileChange {
                path: "D:/cncodetest/output.txt".to_string(),
                action: "modified".to_string(),
            }]
        );
    }

    #[test]
    fn file_changes_from_shell_tool_call_detects_variable_path_out_file() {
        let call = ToolCallRequest {
            id: "call-shell-var".to_string(),
            name: "shell_command".to_string(),
            arguments: serde_json::json!({
                "command": "$path = \"D:\\cncodetest\\cn-codex-site\\index.html\"\n$content = [System.IO.File]::ReadAllText($path)\n$content -replace '<title>AA</title>', '<title>DD</title>' | Out-File -FilePath $path -Encoding UTF8"
            })
            .to_string(),
        };

        assert_eq!(
            file_changes_from_tool_call(&call),
            vec![FileChange {
                path: "D:/cncodetest/cn-codex-site/index.html".to_string(),
                action: "modified".to_string(),
            }]
        );
    }

    #[test]
    fn shell_extract_variable_assignments_parses_simple_assignments() {
        let cmd = "$path = \"D:\\cncodetest\\index.html\"\n$content = [System.IO.File]::ReadAllText($path)\n$content | Out-File -FilePath $path";
        let vars = shell_extract_variable_assignments(cmd);
        assert!(vars.len() >= 1);
        assert_eq!(vars[0].0, "path");
        assert_eq!(vars[0].1, "D:\\cncodetest\\index.html");
    }

    #[test]
    fn file_change_snapshots_capture_before_and_after_content() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-file-snapshot-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("index.html");
        std::fs::write(&target, "<title>AAA</title>").unwrap();

        let changes = vec![FileChange {
            path: target.to_string_lossy().to_string(),
            action: "modified".to_string(),
        }];
        let mut snapshot_map = BTreeMap::new();
        capture_before_file_snapshots(&mut snapshot_map, &changes, &root);
        std::fs::write(&target, "<title>BBB</title>").unwrap();
        capture_after_file_snapshots(&mut snapshot_map, &changes, &root);
        let snapshots = build_changed_file_snapshots(&changes, &snapshot_map, &root);

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].before_content.as_deref(),
            Some("<title>AAA</title>")
        );
        assert_eq!(
            snapshots[0].after_content.as_deref(),
            Some("<title>BBB</title>")
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn file_change_snapshots_keep_before_when_file_deleted() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-file-snapshot-delete-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("remove-me.txt");
        std::fs::write(&target, "to be deleted").unwrap();

        let changes = vec![FileChange {
            path: target.to_string_lossy().to_string(),
            action: "deleted".to_string(),
        }];
        let mut snapshot_map = BTreeMap::new();
        capture_before_file_snapshots(&mut snapshot_map, &changes, &root);
        std::fs::remove_file(&target).unwrap();
        capture_after_file_snapshots(&mut snapshot_map, &changes, &root);
        let snapshots = build_changed_file_snapshots(&changes, &snapshot_map, &root);

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].before_content.as_deref(),
            Some("to be deleted")
        );
        assert_eq!(snapshots[0].after_content, None);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn multimodal_user_content_keeps_images_as_data_urls() {
        let content = multimodal_user_content(
            "What is in this image?",
            &[
                UserAttachment {
                    name: "screen.png".to_string(),
                    mime_type: "image/png".to_string(),
                    data_url: "data:image/png;base64,abc123".to_string(),
                    size: 42,
                },
                UserAttachment {
                    name: "notes.txt".to_string(),
                    mime_type: "text/plain".to_string(),
                    data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
                    size: 5,
                },
            ],
        );

        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,abc123"
        );
        assert!(content[0]["text"].as_str().unwrap().contains("notes.txt"));
    }

    #[tokio::test]
    async fn resolve_image_context_with_fallback_keeps_images_when_model_supports_vision() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cwd = temp_dir.path().to_path_buf();
        let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
        let tool_executor = ToolExecutor::new(cwd.clone());
        let engine = AgentEngine::new(thread_store, tool_executor, cwd).unwrap();

        let config = ConfigToml {
            model_supports_vision: Some(true),
            ..Default::default()
        };
        let attachments = vec![UserAttachment {
            name: "image.png".to_string(),
            mime_type: "image/png".to_string(),
            data_url: "data:image/png;base64,abc123".to_string(),
            size: 12,
        }];

        let (processed, fallback_context) = engine
            .resolve_image_context_with_fallback(
                &config,
                "describe image",
                "text-model",
                &attachments,
            )
            .await;

        assert_eq!(processed.len(), 1);
        assert!(processed[0].mime_type.starts_with("image/"));
        assert!(fallback_context.is_none());
    }

    #[tokio::test]
    async fn resolve_image_context_with_fallback_drops_images_without_fallback_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cwd = temp_dir.path().to_path_buf();
        let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
        let tool_executor = ToolExecutor::new(cwd.clone());
        let engine = AgentEngine::new(thread_store, tool_executor, cwd).unwrap();

        let config = ConfigToml {
            model_supports_vision: Some(false),
            ..Default::default()
        };
        let attachments = vec![
            UserAttachment {
                name: "image.png".to_string(),
                mime_type: "image/png".to_string(),
                data_url: "data:image/png;base64,abc123".to_string(),
                size: 12,
            },
            UserAttachment {
                name: "readme.txt".to_string(),
                mime_type: "text/plain".to_string(),
                data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
                size: 5,
            },
        ];

        let (processed, fallback_context) = engine
            .resolve_image_context_with_fallback(
                &config,
                "describe image",
                "text-model",
                &attachments,
            )
            .await;

        assert_eq!(processed.len(), 1);
        assert!(!processed[0].mime_type.starts_with("image/"));
        assert!(fallback_context.is_some());
        assert!(
            fallback_context
                .as_deref()
                .unwrap_or_default()
                .contains("未配置可用的视觉后补")
        );
    }

    #[tokio::test]
    async fn resolve_image_context_with_local_ocr_fallback_keeps_non_image_attachments() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cwd = temp_dir.path().to_path_buf();
        let thread_store = Arc::new(ThreadStore::new(&cwd.join("codey")));
        let tool_executor = ToolExecutor::new(cwd.clone());
        let engine = AgentEngine::new(thread_store, tool_executor, cwd).unwrap();

        let config = ConfigToml {
            model_supports_vision: Some(false),
            vision_fallback_kind: Some("local_ocr".to_string()),
            ..Default::default()
        };
        let attachments = vec![
            UserAttachment {
                name: "image.png".to_string(),
                mime_type: "image/png".to_string(),
                data_url: "data:image/png;base64,abc123".to_string(),
                size: 12,
            },
            UserAttachment {
                name: "notes.txt".to_string(),
                mime_type: "text/plain".to_string(),
                data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
                size: 5,
            },
        ];

        let (processed, fallback_context) = engine
            .resolve_image_context_with_fallback(
                &config,
                "extract text",
                "text-model",
                &attachments,
            )
            .await;

        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].mime_type, "text/plain");
        assert!(fallback_context.is_some());
        assert!(
            fallback_context
                .as_deref()
                .unwrap_or_default()
                .contains("本地 OCR")
        );
    }

    #[test]
    fn turn_budget_limited_uses_total_tokens() {
        let usage = TurnUsage {
            prompt_tokens: 700,
            completion_tokens: 300,
            total_tokens: 1_000,
            ..Default::default()
        };

        assert!(turn_budget_limited(Some(1_000), &usage));
        assert!(turn_budget_limited(Some(999), &usage));
        assert!(!turn_budget_limited(Some(1_001), &usage));
        assert!(!turn_budget_limited(None, &usage));
    }

    #[test]
    fn nonzero_turn_usage_keeps_call_count_when_tokens_are_zero() {
        let usage = TurnUsage {
            call_count: 2,
            ..Default::default()
        };
        let normalized =
            nonzero_turn_usage(&usage).expect("call_count should keep usage non-empty");
        assert_eq!(normalized.call_count, 2);
        assert_eq!(normalized.total_tokens, 0);
    }

    #[test]
    fn normalize_tool_call_requests_fills_missing_ids() {
        let calls = normalize_tool_call_requests(vec![
            ToolCallRequest {
                id: String::new(),
                name: "apply_patch".to_string(),
                arguments: "*** Begin Patch\n*** End Patch".to_string(),
            },
            ToolCallRequest {
                id: "call-real".to_string(),
                name: "shell".to_string(),
                arguments: "{}".to_string(),
            },
        ]);

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_apply_patch_0");
        assert_eq!(calls[1].id, "call-real");
    }

    #[test]
    fn reorder_history_system_messages_for_model_moves_system_to_front() {
        let history = vec![
            test_thread_message("u1", "user", "hi"),
            test_thread_message("s1", "system", "note-a"),
            test_thread_message("a1", "assistant", "hello"),
            test_thread_message("s2", "system", "note-b"),
            test_thread_message("t1", "tool", "ok"),
        ];

        let reordered = reorder_history_system_messages_for_model(&history);
        let roles: Vec<&str> = reordered.iter().map(|msg| msg.role.as_str()).collect();
        assert_eq!(roles, vec!["system", "system", "user", "assistant", "tool"]);
        assert_eq!(reordered[0].id, "s1");
        assert_eq!(reordered[1].id, "s2");
    }

    #[test]
    fn build_internal_messages_keeps_system_messages_before_non_system_roles() {
        let workspace_dir =
            std::env::temp_dir().join(format!("cn-codex-agent-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_dir).expect("create temp workspace");

        let thread_store = Arc::new(ThreadStore::new(&workspace_dir.join("codey")));
        let tool_executor = ToolExecutor::new(workspace_dir.clone());
        let engine =
            AgentEngine::new(thread_store, tool_executor, workspace_dir.clone()).expect("engine");
        let config = ConfigToml::default();
        let history = vec![
            test_thread_message("u1", "user", "hello"),
            test_thread_message("s1", "system", "runtime note"),
            test_thread_message("a1", "assistant", "reply"),
        ];

        let messages = engine.build_internal_messages(
            &config,
            &history,
            &workspace_dir,
            "chat",
            None,
            None,
            &[],
            None,
            None,
        );

        let first_non_system = messages
            .iter()
            .position(|msg| msg.role != "system")
            .expect("should contain non-system messages");
        assert!(
            messages[first_non_system..]
                .iter()
                .all(|msg| msg.role != "system")
        );
        assert!(messages.iter().any(|msg| {
            msg.role == "system"
                && matches!(
                    msg.content.as_ref(),
                    Some(serde_json::Value::String(content)) if content == "runtime note"
                )
        }));
    }

    #[test]
    fn sanitize_history_for_model_skips_orphan_tool_messages() {
        let history = vec![
            ThreadMessage {
                id: "tool-only".to_string(),
                role: "tool".to_string(),
                content: "result".to_string(),
                timestamp: 1,
                tool_call_id: Some("call-missing".to_string()),
                tool_name: Some("shell".to_string()),
                tool_calls: None,
                attachments: Vec::new(),
            },
            ThreadMessage {
                id: "assistant-call".to_string(),
                role: "assistant".to_string(),
                content: String::new(),
                timestamp: 2,
                tool_call_id: None,
                tool_name: None,
                tool_calls: Some(vec![ToolCallInfo {
                    id: "call-ok".to_string(),
                    name: "shell".to_string(),
                    arguments: "{}".to_string(),
                }]),
                attachments: Vec::new(),
            },
            ThreadMessage {
                id: "tool-ok".to_string(),
                role: "tool".to_string(),
                content: "done".to_string(),
                timestamp: 3,
                tool_call_id: Some("call-ok".to_string()),
                tool_name: Some("shell".to_string()),
                tool_calls: None,
                attachments: Vec::new(),
            },
        ];

        let sanitized = sanitize_history_for_model(&history);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0].id, "assistant-call");
        assert_eq!(sanitized[1].id, "tool-ok");
    }

    #[test]
    fn sanitize_history_for_model_removes_empty_tool_call_ids_from_assistant_and_tool() {
        let history = vec![
            ThreadMessage {
                id: "assistant-bad".to_string(),
                role: "assistant".to_string(),
                content: String::new(),
                timestamp: 1,
                tool_call_id: None,
                tool_name: None,
                tool_calls: Some(vec![ToolCallInfo {
                    id: String::new(),
                    name: "list_directory".to_string(),
                    arguments: "{}".to_string(),
                }]),
                attachments: Vec::new(),
            },
            ThreadMessage {
                id: "tool-bad".to_string(),
                role: "tool".to_string(),
                content: "files".to_string(),
                timestamp: 2,
                tool_call_id: Some(String::new()),
                tool_name: Some("list_directory".to_string()),
                tool_calls: None,
                attachments: Vec::new(),
            },
            ThreadMessage {
                id: "assistant-ok".to_string(),
                role: "assistant".to_string(),
                content: String::new(),
                timestamp: 3,
                tool_call_id: None,
                tool_name: None,
                tool_calls: Some(vec![ToolCallInfo {
                    id: "call-ok".to_string(),
                    name: "list_directory".to_string(),
                    arguments: "{}".to_string(),
                }]),
                attachments: Vec::new(),
            },
            ThreadMessage {
                id: "tool-ok".to_string(),
                role: "tool".to_string(),
                content: "files".to_string(),
                timestamp: 4,
                tool_call_id: Some("call-ok".to_string()),
                tool_name: Some("list_directory".to_string()),
                tool_calls: None,
                attachments: Vec::new(),
            },
        ];

        let sanitized = sanitize_history_for_model(&history);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0].id, "assistant-ok");
        assert_eq!(sanitized[1].id, "tool-ok");
    }

    #[test]
    fn blocked_tool_call_output_surfaces_hook_reason_and_source() {
        let hook = HookRunResult {
            id: "run".to_string(),
            event: HOOK_COMMAND_EXEC.to_string(),
            source_type: "plugin".to_string(),
            source_name: "Safety".to_string(),
            command: "python hook.py".to_string(),
            status: "blocked".to_string(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            duration_ms: 1,
            decision: Some("block".to_string()),
            reason: Some("do not run that".to_string()),
            additional_context: None,
            updated_input: None,
            invalid_output: None,
        };

        assert_eq!(
            blocked_tool_call_output("shell", &hook),
            "Tool call blocked by PreToolUse hook: do not run that. Tool: shell. Hook: python hook.py (Safety)"
        );
    }

    #[test]
    fn append_post_tool_hook_feedback_adds_visible_section() {
        assert_eq!(
            append_post_tool_hook_feedback(
                "tool output".to_string(),
                vec![
                    "Additional context from hook `a`: remember this".to_string(),
                    "Feedback from hook `b`: review that".to_string(),
                ],
            ),
            "tool output\n\n[PostToolUse hook feedback]\nAdditional context from hook `a`: remember this\nFeedback from hook `b`: review that"
        );
    }

    #[test]
    fn user_prompt_submit_hook_context_matches_codex_shape() {
        let context = user_prompt_submit_hook_context(
            "turn-1",
            "goal",
            Path::new("D:/work"),
            "gpt-test",
            "please continue",
        );

        assert_eq!(context["hookEventName"], "UserPromptSubmit");
        assert_eq!(context["turnId"], "turn-1");
        assert_eq!(context["mode"], "goal");
        assert_eq!(context["cwd"], "D:/work");
        assert_eq!(context["model"], "gpt-test");
        assert_eq!(context["permissionMode"], "default");
        assert_eq!(context["prompt"], "please continue");
    }

    #[test]
    fn format_user_prompt_submit_hook_feedback_uses_blocked_or_context_heading() {
        assert_eq!(
            format_user_prompt_submit_hook_feedback(
                vec!["Feedback from hook `gate`: blocked".to_string()],
                true,
            ),
            "[UserPromptSubmit hook blocked prompt]\nFeedback from hook `gate`: blocked"
        );
        assert_eq!(
            format_user_prompt_submit_hook_feedback(
                vec!["Additional context from hook `ctx`: remember this".to_string()],
                false,
            ),
            "[UserPromptSubmit hook context]\nAdditional context from hook `ctx`: remember this"
        );
    }

    #[test]
    fn stop_hook_continuation_message_uses_model_visible_feedback() {
        let hook = HookRunResult {
            id: "run".to_string(),
            event: HOOK_AGENT_END.to_string(),
            source_type: "config".to_string(),
            source_name: "config".to_string(),
            command: "python stop.py".to_string(),
            status: "blocked".to_string(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            duration_ms: 1,
            decision: Some("block".to_string()),
            reason: Some("run tests before stopping".to_string()),
            additional_context: Some("quality gate".to_string()),
            updated_input: None,
            invalid_output: None,
        };

        assert_eq!(
            stop_hook_continuation_message(&[hook]).as_deref(),
            Some(
                "[Stop hook continuation]\nAdditional context from hook `python stop.py`: quality gate\nFeedback from hook `python stop.py`: run tests before stopping"
            )
        );
    }

    #[test]
    fn stop_hook_context_matches_codex_stop_shape() {
        let context = stop_hook_context(
            "turn-1",
            "goal",
            Path::new("D:/work"),
            42,
            &[FileChange {
                path: "src/main.rs".to_string(),
                action: "modified".to_string(),
            }],
            &TurnUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            },
            Some(100),
            false,
            "gpt-test",
            true,
            "done",
        );

        assert_eq!(context["hookEventName"], "Stop");
        assert_eq!(context["turnId"], "turn-1");
        assert_eq!(context["mode"], "goal");
        assert_eq!(context["model"], "gpt-test");
        assert_eq!(context["stopHookActive"], true);
        assert_eq!(context["lastAssistantMessage"], "done");
        assert_eq!(context["changedFiles"][0]["path"], "src/main.rs");
        assert_eq!(context["usage"]["totalTokens"], 15);
    }

    #[test]
    fn subagent_stop_hook_context_extracts_close_agent_result() {
        let call = ToolCallRequest {
            id: "call-1".to_string(),
            name: "close_agent".to_string(),
            arguments: r#"{"target":"agent-1"}"#.to_string(),
        };
        let context = subagent_stop_hook_context(
            "turn-1",
            "goal",
            Path::new("D:/work"),
            "gpt-test",
            &call,
            r#"{
              "target": "agent-1",
              "closed": true,
              "agent": {
                "id": "agent-1",
                "role": "tester",
                "status": "closed",
                "output": "child done"
              }
            }"#,
        );

        assert_eq!(context["hookEventName"], "SubagentStop");
        assert_eq!(context["turnId"], "turn-1");
        assert_eq!(context["mode"], "goal");
        assert_eq!(context["model"], "gpt-test");
        assert_eq!(context["agentId"], "agent-1");
        assert_eq!(context["agent_id"], "agent-1");
        assert_eq!(context["agentType"], "tester");
        assert_eq!(context["agent_type"], "tester");
        assert_eq!(context["lastAssistantMessage"], "child done");
        assert_eq!(context["closeResult"]["closed"], true);
    }

    #[test]
    fn append_subagent_stop_hook_feedback_adds_visible_section() {
        assert_eq!(
            append_subagent_stop_hook_feedback(
                "close output".to_string(),
                vec!["Feedback from hook `child gate`: review child output".to_string()],
            ),
            "close output\n\n[SubagentStop hook feedback]\nFeedback from hook `child gate`: review child output"
        );
    }

    #[test]
    fn plugin_apps_prompt_includes_codex_connector_guidance() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-agent-apps-prompt-test-{}",
            uuid::Uuid::new_v4()
        ));
        let plugin_root = root.join("plugins").join("demo");
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{ "name": "demo", "interface": { "displayName": "Demo Apps" } }"#,
        )
        .unwrap();
        std::fs::write(
            plugin_root.join(".app.json"),
            r#"{"apps":{"calendar":{"id":"connector_calendar"}}}"#,
        )
        .unwrap();

        let prompt = render_plugin_apps_prompt_for_config_dir(&root);

        assert!(prompt.contains("## Apps (Connectors)"));
        assert!(prompt.contains("app://{connector_id}"));
        assert!(prompt.contains("`codex-apps` MCP server"));
        assert!(prompt.contains("lazy-loaded through `tool_search`"));
        assert!(prompt.contains("Use `apps_list`"));
        assert!(prompt.contains("do not additionally call `mcp_list_resources`"));
        assert!(prompt.contains("mcp_list_resource_templates"));
        assert!(prompt.contains("connector `connector_calendar`"));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn smartbrain_runtime_prompt_includes_database_names_without_summary_injection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_config_dir = temp_dir.path().join("codey");
        let usage_db = crate::usage::UsageDb::open(&workspace_config_dir.join("usage.db")).unwrap();
        usage_db
            .state_set(
                SMARTBRAIN_DB_SOURCES_STATE_KEY,
                r#"[{
                  "name": "合同数据库",
                  "dbType": "mysql",
                  "enabled": true,
                  "host": "10.136.0.134",
                  "port": 3306,
                  "databaseName": "psa_crm_pact_test",
                  "username": "root",
                  "password": "top-secret",
                  "permissions": {
                    "readSchema": true,
                    "readData": true,
                    "writeData": false
                  }
                }]"#,
            )
            .unwrap();
        usage_db
            .state_set(
                SMARTBRAIN_DB_SETTINGS_STATE_KEY,
                r##"{
                  "defaultRowLimit": 200,
                  "defaultTimeoutSec": 15,
                  "requireReadonlyReminder": true,
                  "skipWhenNoPermission": true,
                  "rulesMarkdown": "# 数据库安全规则\n- 只读优先"
                }"##,
            )
            .unwrap();

        let prompt = render_smartbrain_runtime_prompt(
            &workspace_config_dir,
            &SmartBrainConfig {
                enabled: true,
                inject_summary: false,
                ..SmartBrainConfig::default()
            },
        );

        assert!(prompt.contains("合同数据库"));
        assert!(prompt.contains("psa_crm_pact_test"));
        assert!(prompt.contains("不要再次向用户索要主机、端口、用户名、密码或完整连接串"));
        assert!(prompt.contains("密码已在配置中单独保存"));
        assert!(!prompt.contains("top-secret"));
    }

    #[test]
    fn robot_runtime_prompt_includes_saved_robot_system_prompt() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_config_dir = temp_dir.path().join("codey");
        let config = crate::robot_loader::RobotConfig {
            name: "数据库机器人".to_string(),
            description: String::new(),
            icon: String::new(),
            skills: Vec::new(),
            plugin_skills: Vec::new(),
            workflow: vec!["查询合同数据库".to_string()],
            workflow_nodes: vec![crate::robot_loader::WorkflowNode {
                objective: "查询合同数据库".to_string(),
                skills: vec!["smartbrain-context-read".to_string()],
                plugin_skills: Vec::new(),
            }],
            system_prompt: "优先使用合同数据库回答问题。".to_string(),
            created_at: 0,
            updated_at: 0,
        };

        crate::robot_loader::save_robot(&workspace_config_dir, "db-bot", &config).unwrap();

        let prompt = render_robot_runtime_prompt(&workspace_config_dir, Some("db-bot"));
        assert!(prompt.contains("db-bot"));
        assert!(prompt.contains("优先使用合同数据库回答问题。"));
    }

    #[test]
    fn file_changes_from_apply_patch_tool_call_marks_move_destination() {
        let call = ToolCallRequest {
            id: "call-1".to_string(),
            name: "apply_patch".to_string(),
            arguments: serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/old.ts\n*** Move to: src/new.ts\n@@\n-old\n+new\n*** End Patch"
            })
            .to_string(),
        };

        assert_eq!(
            file_changes_from_tool_call(&call),
            vec![FileChange {
                path: "src/new.ts".to_string(),
                action: "renamed".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn advance_robot_workflow_from_goal_completion_moves_to_next_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_dir = temp_dir.path().to_path_buf();
        let thread_store = ThreadStore::new(&workspace_dir.join("codey"));
        let thread = thread_store.create_thread(None).await.unwrap();
        thread_store
            .start_turn(&thread.id, Some("goal".to_string()), None)
            .await
            .unwrap();
        thread_store
            .set_thread_goal(
                &thread.id,
                "阶段 1：需求分析".to_string(),
                ThreadGoalStatus::Active,
                None,
            )
            .await
            .unwrap();
        let state = ThreadRobotState {
            robot_id: "bot".to_string(),
            current_node_index: 0,
            root_objective: "实现 lite 版".to_string(),
            runtime_nodes: vec![
                "阶段 1：需求分析".to_string(),
                "阶段 2：架构设计".to_string(),
            ],
            node_deliveries: Vec::new(),
            current_node_start_message_id: None,
        };
        thread_store
            .set_thread_robot_state(&thread.id, state.clone())
            .await
            .unwrap();

        let orchestrator = RobotOrchestrator::new(&workspace_dir);
        let outcome = advance_robot_workflow_from_goal_completion(
            &thread_store,
            &orchestrator,
            &thread.id,
            state,
        )
        .await
        .unwrap();

        let advanced_state = match outcome {
            RobotGoalCompletionOutcome::Advanced(state) => state,
            RobotGoalCompletionOutcome::Completed => panic!("expected workflow to advance"),
        };
        assert_eq!(advanced_state.current_node_index, 1);
        assert!(advanced_state.current_node_start_message_id.is_some());

        let stored_thread = thread_store.get_thread(&thread.id).await.unwrap();
        let stored_goal = stored_thread.goal.unwrap();
        let stored_robot = stored_thread.robot_state.unwrap();
        assert_eq!(stored_goal.objective, "阶段 2：架构设计");
        assert_eq!(stored_goal.status, ThreadGoalStatus::Active);
        assert_eq!(stored_robot.current_node_index, 1);
        assert_eq!(stored_robot.node_deliveries.len(), 1);
        assert!(
            stored_robot.node_deliveries[0]
                .contains("Node 1 completed (no explicit delivery summary provided).")
        );
        assert!(stored_robot.current_node_start_message_id.is_some());

        let messages = thread_store.get_thread_messages(&thread.id).await;
        assert!(messages.last().is_some_and(|message| {
            message.role == "system"
                && message
                    .content
                    .contains("Workflow node completed. Continue with node 2/2.")
        }));
    }

    #[tokio::test]
    async fn advance_robot_workflow_from_goal_completion_finishes_last_node() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_dir = temp_dir.path().to_path_buf();
        let thread_store = ThreadStore::new(&workspace_dir.join("codey"));
        let thread = thread_store.create_thread(None).await.unwrap();
        thread_store
            .start_turn(&thread.id, Some("goal".to_string()), None)
            .await
            .unwrap();
        thread_store
            .set_thread_goal(
                &thread.id,
                "阶段 1：收尾".to_string(),
                ThreadGoalStatus::Active,
                None,
            )
            .await
            .unwrap();
        let state = ThreadRobotState {
            robot_id: "bot".to_string(),
            current_node_index: 0,
            root_objective: "完成整个工作流".to_string(),
            runtime_nodes: vec!["阶段 1：收尾".to_string()],
            node_deliveries: Vec::new(),
            current_node_start_message_id: None,
        };
        thread_store
            .set_thread_robot_state(&thread.id, state.clone())
            .await
            .unwrap();

        let orchestrator = RobotOrchestrator::new(&workspace_dir);
        let outcome = advance_robot_workflow_from_goal_completion(
            &thread_store,
            &orchestrator,
            &thread.id,
            state,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, RobotGoalCompletionOutcome::Completed));
        let stored_thread = thread_store.get_thread(&thread.id).await.unwrap();
        let stored_goal = stored_thread.goal.unwrap();
        assert!(stored_thread.robot_state.is_none());
        assert_eq!(stored_goal.status, ThreadGoalStatus::Complete);
    }

    #[test]
    fn strip_robot_node_done_marker_removes_control_token() {
        let (cleaned, done) = crate::robot_orchestrator::strip_robot_node_done_marker(
            "Node completed. <workflow_node_done/> Moving to next stage.",
        );
        assert!(done);
        assert_eq!(cleaned, "Node completed.  Moving to next stage.");
    }

    #[test]
    fn robot_node_prompts_include_progress_and_done_marker() {
        let nudge = crate::robot_orchestrator::build_robot_node_completion_nudge(1, 4);
        assert!(nudge.contains("2/4"));
        assert!(nudge.contains(crate::robot_orchestrator::ROBOT_NODE_DONE_SENTINEL));

        let advance = crate::robot_orchestrator::build_robot_node_advance_prompt(2, 4);
        assert!(advance.contains("3/4"));
        assert!(advance.contains(crate::robot_orchestrator::ROBOT_NODE_DONE_SENTINEL));
    }
}
