use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::adapter::{self, types::{InternalMessage, InternalToolCall, InternalFunctionCall, StreamEvent, UsageInfo}};
use crate::config_system::ConfigToml;
use crate::error::{AppError, AppResult};
use crate::thread_store::{ThreadMessage, ThreadStore, ToolCallInfo};
use crate::tool_executor::ToolExecutor;
use crate::usage::UsageRecorder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

pub struct AgentEngine {
    http: reqwest::Client,
    thread_store: Arc<ThreadStore>,
    tool_executor: Arc<RwLock<ToolExecutor>>,
    cwd: PathBuf,
    /// 用量记录器（每次 LLM 调用后自动记录）
    usage_recorder: Option<Arc<UsageRecorder>>,
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
        })
    }

    /// 设置用量记录器
    pub fn set_usage_recorder(&mut self, recorder: Arc<UsageRecorder>) {
        self.usage_recorder = Some(recorder);
    }

    pub async fn run_turn(
        &self,
        app_handle: &AppHandle,
        config: &ConfigToml,
        thread_id: &str,
        user_input: &str,
        override_cwd: Option<&Path>,
    ) -> AppResult<()> {
        let turn_id = self.thread_store.start_turn(thread_id).await?;
        let model = config.resolve_model();
        let (provider_id, provider) = config.resolve_provider();

        let base_url = provider.resolve_base_url().ok_or_else(|| {
            AppError::Custom(format!(
                "No base URL for provider '{provider_id}'. Configure it in settings."
            ))
        })?;
        let api_key = provider.resolve_api_key().unwrap_or_default();
        // 获取 wire_api 格式（决定使用哪个 adapter）
        let wire_api = provider.wire_api.as_deref().unwrap_or("chat").to_string();

        let user_msg = ThreadMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: "user".to_string(),
            content: user_input.to_string(),
            timestamp: now_secs(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        };
        self.thread_store
            .add_message(thread_id, user_msg)
            .await?;

        app_handle
            .emit(
                "turn-started",
                serde_json::json!({
                    "threadId": thread_id,
                    "turn": { "id": &turn_id }
                }),
            )
            .ok();

        let effective_cwd = override_cwd
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone());

        if let Some(cwd) = override_cwd {
            self.tool_executor.write().await.set_cwd(cwd.to_path_buf());
        }

        let max_iterations = 10;
        for iteration in 0..max_iterations {
            info!("Agent loop iteration {iteration} for turn {turn_id}");

            let history = self.thread_store.get_thread_messages(thread_id).await;
            let internal_messages = self.build_internal_messages(config, &history, &effective_cwd);
            let tools = self.tool_executor.read().await.tool_specs();

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
                Ok(CompletionResult::Message { ref text, ref usage }) => {
                    info!("Iteration {iteration}: Message ({} chars), usage={:?}", text.len(), usage);
                    // 记录用量到 SQLite
                    if let Some(u) = usage {
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
                    let content = if text.is_empty() && iteration > 0 {
                        String::new()
                    } else {
                        text.clone()
                    };
                    if !content.is_empty() || iteration == 0 {
                        let msg = ThreadMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            role: "assistant".to_string(),
                            content,
                            timestamp: now_secs(),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                        };
                        self.thread_store.add_message(thread_id, msg).await?;
                    }
                    break;
                }
                Ok(CompletionResult::ToolCalls { calls, preceding_text, usage }) => {
                    info!("Iteration {iteration}: ToolCalls ({}): {:?}, preceding_text={} chars, usage={:?}", calls.len(), calls.iter().map(|c| &c.name).collect::<Vec<_>>(), preceding_text.len(), usage);
                    // 记录用量到 SQLite
                    if let Some(ref u) = usage {
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

                    for call in calls {
                        info!("Tool call: {} args={}", call.name, call.arguments);

                        let tool_result = self
                            .tool_executor
                            .read()
                            .await
                            .execute(&call.name, &call.arguments, &call.id, app_handle, thread_id)
                            .await;

                        let (result_content, success) = match tool_result {
                            Ok(output) => (output, true),
                            Err(e) => (format!("Tool execution error: {e}"), false),
                        };

                        results_json.push(serde_json::json!({
                            "id": call.id,
                            "tool": call.name,
                            "success": success,
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

        info!("Turn {turn_id} completed for thread {thread_id}");

        app_handle
            .emit(
                "turn-completed",
                serde_json::json!({
                    "threadId": thread_id,
                    "turn": { "id": &turn_id }
                }),
            )
            .ok();

        self.thread_store.end_turn(thread_id, &turn_id).await?;
        Ok(())
    }

    fn build_system_prompt(&self, config: &ConfigToml, effective_cwd: &Path) -> String {
        let cwd_str = effective_cwd.to_string_lossy();
        let os_info = std::env::consts::OS;
        let arch_info = std::env::consts::ARCH;

        let user_instructions = config
            .instructions
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("\n\nAdditional instructions from user:\n{s}"))
            .unwrap_or_default();

        format!(
            "You are Codey, a coding assistant that helps users with programming tasks.\n\
             You are running in the following environment:\n\
             - Working directory: {cwd_str}\n\
             - Operating system: {os_info} ({arch_info})\n\
             \n\
             You have access to the following tools:\n\
             - shell: Execute shell commands to run code, install packages, build projects, etc.\n\
             - read_file: Read the contents of a file at a given path.\n\
             - write_file: Create or overwrite a file with the given content.\n\
             - list_directory: List files and subdirectories in a directory.\n\
             \n\
             IMPORTANT: Before using any tools, always briefly explain what you are about to do and why. \
             This helps the user understand your reasoning and plan.\n\
             \n\
             IMPORTANT: When the user asks you to create files, write code, run commands, \
             modify projects, or perform any task that requires interacting with the file system \
             or running programs, you MUST use the appropriate tools. Do NOT just describe \
             what you would do — actually do it using tool calls.\n\
             \n\
             All file paths in tool calls should be relative to the working directory unless \
             the user specifies an absolute path.{user_instructions}"
        )
    }

    /// 将线程历史消息转换为 adapter 层统一的 InternalMessage 格式
    fn build_internal_messages(
        &self,
        config: &ConfigToml,
        history: &[ThreadMessage],
        effective_cwd: &Path,
    ) -> Vec<InternalMessage> {
        let mut messages = Vec::new();

        // system prompt
        messages.push(InternalMessage {
            role: "system".to_string(),
            content: Some(self.build_system_prompt(config, effective_cwd)),
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

            let content = if msg.role == "assistant" && internal_tool_calls.is_some() && msg.content.is_empty() {
                None
            } else {
                Some(msg.content.clone())
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
            return Err(AppError::Custom(format!("LLM API error ({status}): {body_text}")));
        }

        // 流式解析
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
        let mut finish_reason: Option<String> = None;
        let mut usage_info: Option<UsageInfo> = None;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| AppError::Custom(format!("Stream read error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

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
                                .emit(
                                    "agent-message-delta",
                                    serde_json::json!({ "delta": text }),
                                )
                                .ok();
                        }
                        StreamEvent::ToolCallDelta { index, id, name, arguments } => {
                            while tool_calls.len() <= index {
                                tool_calls.push(ToolCallAccumulator::default());
                            }
                            let acc = &mut tool_calls[index];
                            if let Some(id) = id {
                                acc.id = id;
                            }
                            if let Some(name) = name {
                                acc.name = name;
                            }
                            if let Some(args) = arguments {
                                acc.arguments.push_str(&args);
                            }
                        }
                        StreamEvent::Done { finish_reason: reason } => {
                            finish_reason = reason.or(finish_reason);
                        }
                        StreamEvent::Usage(usage) => {
                            // 累加 usage（Anthropic 分两次返回 input/output tokens）
                            if let Some(ref mut existing) = usage_info {
                                existing.prompt_tokens = existing.prompt_tokens.max(usage.prompt_tokens);
                                existing.completion_tokens = existing.completion_tokens.max(usage.completion_tokens);
                                existing.total_tokens = existing.prompt_tokens + existing.completion_tokens;
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
}

/// tool call 累积器（逐步拼接 SSE 中的碎片）
#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
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

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
