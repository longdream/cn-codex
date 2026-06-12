use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

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
use crate::config_system::ConfigToml;
use crate::error::{AppError, AppResult};
use crate::hook_runtime::{
    HOOK_AGENT_END, HOOK_AGENT_START, HOOK_COMMAND_EXEC, HOOK_FILE_CHANGE, HOOK_POST_TOOL_USE,
    HOOK_SUBAGENT_STOP, HOOK_USER_PROMPT_SUBMIT, HookRunResult, HookRuntime,
    first_blocking_hook_result, hook_feedback_for_model, latest_hook_updated_input,
};
use crate::plugin_loader;
use crate::thread_store::{
    FileChange, ThreadGoal, ThreadGoalStatus, ThreadMessage, ThreadStore, ToolCallInfo, TurnUsage,
};
use crate::tool_executor::ToolExecutor;
use crate::usage::UsageRecorder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

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
    cancel_flag: Arc<AtomicBool>,
}

impl AgentEngine {
    pub fn new(
        thread_store: Arc<ThreadStore>,
        tool_executor: ToolExecutor,
        cwd: PathBuf,
    ) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| AppError::Custom(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            http,
            thread_store,
            tool_executor: Arc::new(RwLock::new(tool_executor)),
            cwd,
            usage_recorder: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 设置用量记录器
    pub fn set_usage_recorder(&mut self, recorder: Arc<UsageRecorder>) {
        self.usage_recorder = Some(recorder);
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
    ) -> AppResult<()> {
        self.cancel_flag.store(false, Ordering::SeqCst);

        let turn_mode = match mode {
            Some("goal") => "goal",
            _ => "chat",
        }
        .to_string();
        let turn_started_at_ms = now_millis();
        let turn_timer = Instant::now();
        let model = config.resolve_model();
        if model.is_empty() {
            return Err(AppError::Custom(
                "未配置模型。请在设置中选择一个模型后再试。".to_string(),
            ));
        }
        let (provider_id, provider) = config.resolve_provider();
        let effective_cwd = override_cwd
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone());
        let git_status_before = git_status_snapshot(&effective_cwd).await;
        let mut changed_files: Vec<FileChange> = Vec::new();
        let mut turn_usage = TurnUsage::default();
        let goal_budget_tokens = if turn_mode == "goal" {
            goal_budget_tokens.filter(|value| *value > 0)
        } else {
            None
        };
        if turn_mode == "goal" {
            self.thread_store
                .set_thread_goal(
                    thread_id,
                    user_input.to_string(),
                    ThreadGoalStatus::Active,
                    goal_budget_tokens,
                )
                .await?;
        }
        let turn_id = self
            .thread_store
            .start_turn(thread_id, Some(turn_mode.clone()), goal_budget_tokens)
            .await?;

        let base_url = provider.resolve_base_url().ok_or_else(|| {
            AppError::Custom(format!(
                "No base URL for provider '{provider_id}'. Configure it in settings."
            ))
        })?;
        let api_key = provider.resolve_api_key().unwrap_or_default();
        // 获取 wire_api 格式（决定使用哪个 adapter）
        let wire_api = provider.wire_api.as_deref().unwrap_or("chat").to_string();

        let user_message_id = uuid::Uuid::new_v4().to_string();
        let user_msg = ThreadMessage {
            id: user_message_id.clone(),
            role: "user".to_string(),
            content: user_input.to_string(),
            timestamp: now_secs(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        };
        self.thread_store.add_message(thread_id, user_msg).await?;

        app_handle
            .emit(
                "turn-started",
                serde_json::json!({
                    "threadId": thread_id,
                    "turn": {
                        "id": &turn_id,
                        "mode": &turn_mode,
                        "startedAt": turn_started_at_ms,
                    }
                }),
            )
            .ok();

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

        let max_iterations = 25;
        let mut stop_hooks_satisfied = false;
        let mut stop_hooks_ran_for_last_stop = false;
        let mut stop_hook_continuations = 0usize;
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

        let mut intent_retries: u32 = 0;
        const MAX_INTENT_RETRIES: u32 = 2;

        if !prompt_hook_blocked {
            for iteration in 0..max_iterations {
                if self.is_cancelled() {
                    info!("Turn {turn_id} cancelled by user at iteration {iteration}");
                    break;
                }
                info!("Agent loop iteration {iteration} for turn {turn_id}");

                let history = self.thread_store.get_thread_messages(thread_id).await;
                let internal_messages = self.build_internal_messages(
                    config,
                    &history,
                    &effective_cwd,
                    &turn_mode,
                    Some(user_message_id.as_str()),
                    &attachments,
                );
                let tools = self
                    .tool_executor
                    .write()
                    .await
                    .tool_specs_with_mcp(config.web_search_enabled())
                    .await;

                let result = self
                    .stream_completion(
                        app_handle,
                        &base_url,
                        &api_key,
                        &model,
                        &wire_api,
                        internal_messages,
                        if tools.is_empty() { None } else { Some(tools) },
                    )
                    .await;

                match result {
                    Ok(CompletionResult::Message {
                        ref text,
                        ref usage,
                    }) => {
                        info!(
                            "Iteration {iteration}: Message ({} chars), usage={:?}",
                            text.len(),
                            usage
                        );
                        // 记录用量到 SQLite
                        if let Some(u) = usage {
                            add_turn_usage(&mut turn_usage, u);
                            if let Some(ref recorder) = self.usage_recorder {
                                recorder.record(&provider_id, &model, thread_id, u);
                            }
                        }
                        if text.is_empty() && iteration > 0 {
                            info!("Empty message after tool execution, sending minimal signal");
                            app_handle
                                .emit(
                                    "agent-message-delta",
                                    serde_json::json!({ "delta": "(completed)" }),
                                )
                                .ok();
                        }

                        if !text.is_empty()
                            && iteration > 0
                            && intent_retries < MAX_INTENT_RETRIES
                            && text_expresses_intent(text)
                        {
                            intent_retries += 1;
                            info!(
                                "Intent detected in text without tool calls, retry {intent_retries}/{MAX_INTENT_RETRIES}"
                            );
                            let nudge_msg = ThreadMessage {
                                id: uuid::Uuid::new_v4().to_string(),
                                role: "system".to_string(),
                                content: "You expressed intent to perform an action but did not \
                                          call any tools. Please call the appropriate tool(s) now \
                                          instead of describing what you plan to do."
                                    .to_string(),
                                timestamp: now_secs(),
                                tool_call_id: None,
                                tool_name: None,
                                tool_calls: None,
                            };
                            self.thread_store.add_message(thread_id, nudge_msg).await?;
                            continue;
                        }

                        let content = if text.is_empty() && iteration > 0 {
                            String::new()
                        } else {
                            text.clone()
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
                            };
                            self.thread_store.add_message(thread_id, msg).await?;
                        }
                        let git_status_now = git_status_snapshot(&effective_cwd).await;
                        merge_git_changes(&mut changed_files, &git_status_before, &git_status_now);
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
                                stop_hook_continuations = stop_hook_continuations.saturating_add(1);
                                let msg = ThreadMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: "system".to_string(),
                                    content: continuation,
                                    timestamp: now_secs(),
                                    tool_call_id: None,
                                    tool_name: None,
                                    tool_calls: None,
                                };
                                self.thread_store.add_message(thread_id, msg).await?;
                                continue;
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
                        info!(
                            "Iteration {iteration}: ToolCalls ({}): {:?}, preceding_text={} chars, usage={:?}",
                            calls.len(),
                            calls.iter().map(|c| &c.name).collect::<Vec<_>>(),
                            preceding_text.len(),
                            usage
                        );
                        // 记录用量到 SQLite
                        if let Some(ref u) = usage {
                            add_turn_usage(&mut turn_usage, u);
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
                        app_handle
                            .emit(
                                "tool-calls-start",
                                serde_json::json!({
                                    "threadId": thread_id,
                                    "calls": calls_json,
                                }),
                            )
                            .ok();

                        let mut results_json: Vec<serde_json::Value> = Vec::new();

                        for mut call in calls {
                            info!("Tool call: {} args={}", call.name, call.arguments);
                            // 若用户已点击停止，则跳过工具执行，并主动补发结束状态，
                            // 防止前端工具卡片一直停留在 running。
                            if self.is_cancelled() {
                                let interrupted_call_id = call.id.clone();
                                let interrupted_tool_name = call.name.clone();
                                let interrupted_output =
                                    "Tool execution skipped: interrupted by user.".to_string();
                                app_handle
                                    .emit(
                                        "tool-exec-end",
                                        serde_json::json!({
                                            "threadId": thread_id,
                                            "callId": interrupted_call_id,
                                            "tool": interrupted_tool_name,
                                            "exitCode": -1,
                                            "output": interrupted_output.clone(),
                                        }),
                                    )
                                    .ok();
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

                            let (mut result_content, success) = match tool_result {
                                Ok(output) => (output, true),
                                Err(e) => (format!("Tool execution error: {e}"), false),
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
                            };
                            self.thread_store.add_message(thread_id, tool_msg).await?;
                        }

                        app_handle
                            .emit(
                                "tool-calls-end",
                                serde_json::json!({
                                    "threadId": thread_id,
                                    "results": results_json,
                                }),
                            )
                            .ok();
                        stop_hooks_ran_for_last_stop = false;
                    }
                    Err(e) => {
                        error!("Iteration {iteration}: LLM request failed: {e}");
                        app_handle
                            .emit(
                                "server-error",
                                serde_json::json!({ "message": e.to_string() }),
                            )
                            .ok();
                        break;
                    }
                }
            }
        }

        if !stop_hooks_satisfied && !stop_hooks_ran_for_last_stop && !prompt_hook_blocked {
            info!("Agent loop ended after tool calls without summary, requesting final summary");
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
            };
            self.thread_store
                .add_message(thread_id, summary_nudge)
                .await?;

            let history = self.thread_store.get_thread_messages(thread_id).await;
            let internal_messages = self.build_internal_messages(
                config,
                &history,
                &effective_cwd,
                &turn_mode,
                Some(user_message_id.as_str()),
                &[],
            );
            let summary_result = self
                .stream_completion(
                    app_handle,
                    &base_url,
                    &api_key,
                    &model,
                    &wire_api,
                    internal_messages,
                    None,
                )
                .await;
            let summary_text = match summary_result {
                Ok(CompletionResult::Message { text, usage }) => {
                    if let Some(u) = usage {
                        add_turn_usage(&mut turn_usage, &u);
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
                    if let Some(u) = usage {
                        add_turn_usage(&mut turn_usage, &u);
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
                };
                self.thread_store.add_message(thread_id, msg).await?;
            }
        }

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
        let goal_after = if turn_mode == "goal" {
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

        let mut completed_payload = serde_json::json!({
            "threadId": thread_id,
            "turn": {
                "id": &turn_id,
                "mode": &turn_mode,
                "startedAt": turn_started_at_ms,
                "completedAt": completed_at * 1000,
                "durationMs": duration_ms,
                "changedFiles": changed_files,
                "usage": usage,
                "goalBudgetTokens": goal_budget_tokens,
                "budgetLimited": budget_limited,
            }
        });
        if turn_mode == "goal" {
            completed_payload["goal"] = serde_json::json!(goal_after);
        }
        app_handle.emit("turn-completed", completed_payload).ok();
        Ok(())
    }

    fn build_system_prompt(&self, config: &ConfigToml, effective_cwd: &Path, mode: &str) -> String {
        let cwd_str = effective_cwd.to_string_lossy();
        let os_info = std::env::consts::OS;
        let arch_info = std::env::consts::ARCH;

        let user_instructions = config
            .instructions
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("\n\nAdditional instructions from user:\n{s}"))
            .unwrap_or_default();
        let skills_instructions = self.render_available_skills_prompt();
        let apps_instructions = self.render_plugin_apps_prompt();
        let web_tool_instructions = if config.web_search_enabled() {
            "             - web_search: Search the web for current information.\n\
             - web_fetch: Fetch a web page URL and return readable text.\n"
        } else {
            ""
        };
        let mode_instructions = if mode == "goal" {
            "\n\nGoal mode is active. Treat the latest user message as a concrete objective, not a casual chat prompt. \
             Keep working through the available tools until the objective is genuinely handled or you hit a real blocker. \
             Prefer implementation and verification over proposals. Give concise progress updates as you work, and finish \
             with a short outcome summary that mentions verification and the important files changed."
        } else {
            ""
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
             - request_user_input: Ask the user one to three short structured questions and wait for their response when progress genuinely depends on user input.\n\
             - request_permissions: Ask the user for additional filesystem or network permissions and wait for their response.\n\
             - view_image: Inspect and preview local image files, returning format, dimensions, size, and path.\n\
             - image_generate: Generate an image through an OpenAI Images API-compatible backend and save it as a local file when image generation is configured.\n\
             - browser_run: Run a browser session for page navigation, UI interaction, screenshots, and web app testing. Runtime is CN-Codex built-in Tauri WebView controlled by Rust-side JS Injection + CDP. Keep action batches focused and rely on screenshots/html/snapshot for verification.\n\
             - apps_list: List imported plugin app connectors and trusted codex-apps MCP tools, including connector IDs and availability.\n\
             - list_available_plugins_to_install: List local Codex plugin cache candidates that can be imported into this CN-Codex workspace.\n\
             - request_plugin_install: Import one local Codex plugin cache candidate into codey/plugins; call list_available_plugins_to_install first when unsure of the tool_id.\n\
             - plugin_manage: List, enable, disable, or uninstall local workspace plugins under codey/plugins; disabled plugins stay on disk but are excluded from skills, MCP servers, app connectors, and hooks.\n\
             - spawn_agent: Start a background Codex subagent for delegated investigation, review, testing, or implementation.\n\
             - wait_agent: Wait for one or more spawned subagents and read their results.\n\
             - send_input: Send a follow-up message to a spawned subagent; the CLI-backed runtime records all submissions and writes to process stdin when available.\n\
             - resume_agent: Resume a stopped spawned subagent by restarting its CLI-backed process with prior task context and input history.\n\
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
             All file paths in tool calls should be relative to the working directory unless \
             the user specifies an absolute path.\n\
             \n\
             WINDOWS SHELL: This system uses PowerShell. Do NOT use '&&' to chain commands — \
             use ';' instead (e.g. 'cd mydir; npm install'). Use Set-Location or cd to change \
             directories. Alternatively, set the 'workdir' parameter in the shell tool call.{skills_instructions}{apps_instructions}{mode_instructions}{user_instructions}"
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
    /// Convert thread history into the adapter layer's unified internal message shape.
    fn build_internal_messages(
        &self,
        config: &ConfigToml,
        history: &[ThreadMessage],
        effective_cwd: &Path,
        mode: &str,
        current_user_message_id: Option<&str>,
        attachments: &[UserAttachment],
    ) -> Vec<InternalMessage> {
        let mut messages = Vec::new();

        // system prompt
        messages.push(InternalMessage {
            role: "system".to_string(),
            content: text_content(self.build_system_prompt(config, effective_cwd, mode)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        for msg in history {
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
                && !attachments.is_empty()
            {
                Some(multimodal_user_content(&msg.content, attachments))
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
        base_url: &str,
        api_key: &str,
        model: &str,
        wire_api: &str,
        messages: Vec<InternalMessage>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> AppResult<CompletionResult> {
        // 根据 wire_api 选择 adapter
        let adapter = adapter::get_adapter(wire_api);

        let url = adapter.build_url(base_url, model);
        let headers = adapter.build_headers(api_key);
        let tools_slice = tools.as_deref();
        let body = adapter.build_body(model, &messages, tools_slice);

        info!("LLM request: wire_api={wire_api}, url={url}, model={model}");
        if tracing::enabled!(tracing::Level::DEBUG) {
            let body_preview = serde_json::to_string(&body)
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect::<String>();
            tracing::debug!("LLM request body (first 500 chars): {body_preview}");
        }

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
                &body_text[..body_text.len().min(300)]
            );
            return self.parse_non_streaming_chat_response(&body_text, app_handle);
        }

        // 流式解析
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
        let mut finish_reason: Option<String> = None;
        let mut usage_info: Option<UsageInfo> = None;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            if self.is_cancelled() {
                info!("SSE stream cancelled by user");
                finish_reason = Some("interrupted".to_string());
                break;
            }
            let chunk = chunk.map_err(|e| AppError::Custom(format!("Stream read error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                tracing::trace!("SSE line: {}", &line[..line.len().min(200)]);

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
                            full_text.push_str(&text);
                            app_handle
                                .emit("agent-message-delta", serde_json::json!({ "delta": text }))
                                .ok();
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

        let valid_tool_calls: Vec<ToolCallRequest> = tool_calls
            .into_iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| ToolCallRequest {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            })
            .collect();

        info!(
            "stream_completion done: wire_api={wire_api}, finish_reason={:?}, tool_calls={}, text_len={}, usage={:?}",
            finish_reason,
            valid_tool_calls.len(),
            full_text.len(),
            usage_info,
        );

        // 如果流结束但没有任何内容也没有 finish_reason，可能是连接异常或响应格式不兼容
        if full_text.is_empty() && valid_tool_calls.is_empty() && finish_reason.is_none() {
            warn!(
                "Stream ended with no content and no finish_reason. Buffer remainder: {:?}",
                &buffer[..buffer.len().min(200)]
            );
            return Err(AppError::Custom(
                "LLM returned empty stream - the provider may not support the current request format. \
                 Try switching wire_api or check the provider's compatibility."
                    .to_string(),
            ));
        }

        if !valid_tool_calls.is_empty() {
            Ok(CompletionResult::ToolCalls {
                calls: valid_tool_calls,
                preceding_text: full_text,
                usage: usage_info,
            })
        } else {
            Ok(CompletionResult::Message {
                text: full_text,
                usage: usage_info,
            })
        }
    }

    /// 解析非流式 Chat Completions JSON 响应
    fn parse_non_streaming_chat_response(
        &self,
        body: &str,
        app_handle: &AppHandle,
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
        let text = message
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        // 提取 usage
        let usage_info = json.get("usage").map(|u| UsageInfo {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        });

        // 提取 tool calls
        let tool_calls: Vec<ToolCallRequest> = message
            .and_then(|m| m.get("tool_calls"))
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let id = tc.get("id")?.as_str()?.to_string();
                        let func = tc.get("function")?;
                        let name = func.get("name")?.as_str()?.to_string();
                        let arguments = func.get("arguments")?.as_str()?.to_string();
                        Some(ToolCallRequest {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // 发送文本增量事件
        if !text.is_empty() {
            app_handle
                .emit("agent-message-delta", serde_json::json!({ "delta": &text }))
                .ok();
        }

        if !tool_calls.is_empty() {
            Ok(CompletionResult::ToolCalls {
                calls: tool_calls,
                preceding_text: text,
                usage: usage_info,
            })
        } else {
            Ok(CompletionResult::Message {
                text,
                usage: usage_info,
            })
        }
    }
}

/// tool call 累积器（逐步拼接 SSE 中的碎片）
#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
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

/// LLM 调用完成后的结果
enum CompletionResult {
    /// 纯文本回复
    Message {
        text: String,
        usage: Option<UsageInfo>,
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
    total.total_tokens = if usage.total_tokens > 0 {
        total.total_tokens.saturating_add(usage.total_tokens)
    } else {
        total
            .total_tokens
            .saturating_add(usage.prompt_tokens.saturating_add(usage.completion_tokens))
    };
}

fn nonzero_turn_usage(usage: &TurnUsage) -> Option<TurnUsage> {
    if usage.prompt_tokens == 0 && usage.completion_tokens == 0 && usage.total_tokens == 0 {
        None
    } else {
        Some(usage.clone())
    }
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

fn multimodal_user_content(text: &str, attachments: &[UserAttachment]) -> serde_json::Value {
    let mut parts = Vec::new();
    if !text.trim().is_empty() {
        parts.push(serde_json::json!({
            "type": "text",
            "text": text
        }));
    }

    let mut non_image_notes = Vec::new();
    for attachment in attachments {
        if attachment.mime_type.starts_with("image/") && attachment.data_url.starts_with("data:") {
            parts.push(serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": attachment.data_url,
                    "detail": "high"
                }
            }));
        } else {
            non_image_notes.push(format!(
                "- {} ({}, {} bytes)",
                attachment.name, attachment.mime_type, attachment.size
            ));
        }
    }

    if !non_image_notes.is_empty() {
        parts.push(serde_json::json!({
            "type": "text",
            "text": format!("Attached non-image files:\n{}", non_image_notes.join("\n"))
        }));
    }

    if parts.is_empty() {
        serde_json::Value::String(text.to_string())
    } else {
        serde_json::Value::Array(parts)
    }
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

fn file_changes_from_tool_call(call: &ToolCallRequest) -> Vec<FileChange> {
    match call.name.as_str() {
        "write_file" => write_file_change_from_args(&call.arguments)
            .into_iter()
            .collect(),
        "apply_patch" => apply_patch_changes_from_args(&call.arguments),
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

fn push_file_change(changes: &mut Vec<FileChange>, change: FileChange) {
    if change.path.trim().is_empty() {
        return;
    }

    if let Some(existing) = changes.iter_mut().find(|item| item.path == change.path) {
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

    fn status_entry(status: &str, fingerprint: Option<u64>) -> GitStatusEntry {
        GitStatusEntry {
            status: status.to_string(),
            fingerprint,
        }
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
        assert!(content[2]["text"].as_str().unwrap().contains("notes.txt"));
    }

    #[test]
    fn turn_budget_limited_uses_total_tokens() {
        let usage = TurnUsage {
            prompt_tokens: 700,
            completion_tokens: 300,
            total_tokens: 1_000,
        };

        assert!(turn_budget_limited(Some(1_000), &usage));
        assert!(turn_budget_limited(Some(999), &usage));
        assert!(!turn_budget_limited(Some(1_001), &usage));
        assert!(!turn_budget_limited(None, &usage));
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
}
