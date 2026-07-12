use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::adapter::{self, types::*};
use crate::agent::ToolCallRequest;
use crate::error::{AppError, AppResult};
use crate::tool_executor::ToolExecutor;

use tauri::AppHandle;
use tokio::sync::RwLock;

/// Configuration for an internal subagent.
#[derive(Debug, Clone)]
pub struct SubagentConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub wire_api: String,
    pub system_prompt: String,
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub max_iterations: usize,
    pub max_output_tokens: Option<i64>,
}

/// Status update sent from the subagent task to the registry.
#[derive(Debug, Clone)]
pub enum SubagentStatus {
    Running,
    Completed { output: String },
    Failed { error: String },
    TimedOut,
    Cancelled,
}

/// Handle returned after spawning a subagent, used to communicate with the task.
#[derive(Clone)]
pub struct SubagentHandle {
    pub cancel_flag: Arc<AtomicBool>,
    pub input_tx: mpsc::Sender<String>,
}

/// Result of a completed subagent run.
#[derive(Debug, Clone)]
pub struct SubagentResult {
    pub status: SubagentStatus,
    pub duration_ms: u64,
}

/// Message in the subagent's in-memory conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubagentMessage {
    role: String,
    content: String,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_calls: Option<Vec<ToolCallInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallInfo {
    id: String,
    name: String,
    arguments: String,
}

enum CompletionResult {
    Message {
        text: String,
    },
    ToolCalls {
        calls: Vec<ToolCallRequest>,
        preceding_text: String,
    },
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

/// Spawn an internal subagent as a tokio task.
///
/// Returns a handle for communication and a oneshot receiver for the final result.
pub fn spawn_subagent(
    config: SubagentConfig,
    prompt: String,
    tool_executor: Arc<RwLock<ToolExecutor>>,
    app_handle: AppHandle,
    thread_id: String,
    subagent_id: String,
) -> (
    SubagentHandle,
    tokio::sync::oneshot::Receiver<SubagentResult>,
) {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let (input_tx, input_rx) = mpsc::channel::<String>(16);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    let handle = SubagentHandle {
        cancel_flag: cancel_flag.clone(),
        input_tx,
    };

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    tokio::spawn(async move {
        let result = run_subagent_loop(
            http,
            config,
            prompt,
            tool_executor,
            &app_handle,
            &thread_id,
            &subagent_id,
            cancel_flag,
            input_rx,
        )
        .await;
        let _ = result_tx.send(result);
    });

    (handle, result_rx)
}

async fn run_subagent_loop(
    http: reqwest::Client,
    config: SubagentConfig,
    prompt: String,
    tool_executor: Arc<RwLock<ToolExecutor>>,
    app_handle: &AppHandle,
    thread_id: &str,
    subagent_id: &str,
    cancel_flag: Arc<AtomicBool>,
    mut input_rx: mpsc::Receiver<String>,
) -> SubagentResult {
    let started = Instant::now();
    let timeout = std::time::Duration::from_millis(config.timeout_ms);
    let mut messages: Vec<SubagentMessage> = Vec::new();

    messages.push(SubagentMessage {
        role: "system".to_string(),
        content: config.system_prompt.clone(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    });

    messages.push(SubagentMessage {
        role: "user".to_string(),
        content: prompt,
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    });

    let mut last_assistant_text = String::new();

    for iteration in 0..config.max_iterations {
        if cancel_flag.load(Ordering::SeqCst) {
            return SubagentResult {
                status: SubagentStatus::Cancelled,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }

        if started.elapsed() > timeout {
            return SubagentResult {
                status: SubagentStatus::TimedOut,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }

        // Check for incoming send_input messages (non-blocking)
        while let Ok(input) = input_rx.try_recv() {
            messages.push(SubagentMessage {
                role: "user".to_string(),
                content: input,
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            });
        }

        let internal_messages = build_internal_messages(&messages);
        let tools = {
            let executor = tool_executor.write().await;
            executor
                .tool_specs(false)
                .into_iter()
                .filter(|spec| {
                    let name = spec
                        .pointer("/function/name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    // Subagents cannot spawn further subagents (depth limit)
                    name != "spawn_agent"
                        && name != "wait_agent"
                        && name != "send_input"
                        && name != "resume_agent"
                        && name != "list_agents"
                        && name != "close_agent"
                })
                .collect::<Vec<_>>()
        };

        let result = stream_completion_internal(
            &http,
            &config,
            internal_messages,
            if tools.is_empty() { None } else { Some(tools) },
            &cancel_flag,
        )
        .await;

        match result {
            Ok(CompletionResult::Message { text }) => {
                info!(
                    "[subagent:{subagent_id}] iteration {iteration}: message ({} chars)",
                    text.len()
                );
                last_assistant_text = text.clone();
                messages.push(SubagentMessage {
                    role: "assistant".to_string(),
                    content: text,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
                // Message without tool calls means the subagent is done
                break;
            }
            Ok(CompletionResult::ToolCalls {
                calls,
                preceding_text,
            }) => {
                info!(
                    "[subagent:{subagent_id}] iteration {iteration}: {} tool calls",
                    calls.len()
                );

                if !preceding_text.is_empty() {
                    last_assistant_text = preceding_text.clone();
                }

                let tc_infos: Vec<ToolCallInfo> = calls
                    .iter()
                    .map(|c| ToolCallInfo {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    })
                    .collect();
                messages.push(SubagentMessage {
                    role: "assistant".to_string(),
                    content: preceding_text,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: Some(tc_infos),
                });

                for call in calls {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return SubagentResult {
                            status: SubagentStatus::Cancelled,
                            duration_ms: started.elapsed().as_millis() as u64,
                        };
                    }
                    if started.elapsed() > timeout {
                        return SubagentResult {
                            status: SubagentStatus::TimedOut,
                            duration_ms: started.elapsed().as_millis() as u64,
                        };
                    }

                    let tool_result = tool_executor
                        .read()
                        .await
                        .execute(&call.name, &call.arguments, &call.id, app_handle, thread_id)
                        .await;

                    let result_content = match tool_result {
                        Ok(output) => output,
                        Err(e) => format!("Tool execution error: {e}"),
                    };

                    messages.push(SubagentMessage {
                        role: "tool".to_string(),
                        content: result_content,
                        tool_call_id: Some(call.id),
                        tool_name: Some(call.name),
                        tool_calls: None,
                    });
                }
            }
            Err(e) => {
                warn!("[subagent:{subagent_id}] iteration {iteration} error: {e}");
                return SubagentResult {
                    status: SubagentStatus::Failed {
                        error: e.to_string(),
                    },
                    duration_ms: started.elapsed().as_millis() as u64,
                };
            }
        }
    }

    let output = if last_assistant_text.is_empty() {
        "(subagent completed with no output)".to_string()
    } else {
        last_assistant_text
    };

    SubagentResult {
        status: SubagentStatus::Completed { output },
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

fn build_internal_messages(messages: &[SubagentMessage]) -> Vec<InternalMessage> {
    messages
        .iter()
        .map(|msg| {
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
            } else {
                text_content(msg.content.clone())
            };

            InternalMessage {
                role: msg.role.clone(),
                content,
                tool_calls: internal_tool_calls,
                tool_call_id: msg.tool_call_id.clone(),
                name: msg.tool_name.clone(),
            }
        })
        .collect()
}

/// Stripped-down LLM streaming call without logging, events, or conversation logger.
async fn stream_completion_internal(
    http: &reqwest::Client,
    config: &SubagentConfig,
    messages: Vec<InternalMessage>,
    tools: Option<Vec<serde_json::Value>>,
    cancel_flag: &AtomicBool,
) -> AppResult<CompletionResult> {
    let adapter = adapter::get_adapter(&config.wire_api);
    let url = adapter.build_url(&config.base_url, &config.model);
    let headers = adapter.build_headers(&config.api_key);
    let tools_slice = tools.as_deref();
    let body = adapter.build_body(
        &config.model,
        &messages,
        tools_slice,
        config.max_output_tokens,
    );

    let response = http
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

    if content_type.contains("application/json") && !content_type.contains("stream") {
        return parse_non_streaming_response(&response.text().await.unwrap_or_default());
    }

    let mut full_text = String::new();
    let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
    let mut finish_reason: Option<String> = None;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut utf8_decoder = crate::utf8_stream::Utf8StreamDecoder::new();

    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::SeqCst) {
            finish_reason = Some("interrupted".to_string());
            break;
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                if !full_text.is_empty() || !tool_calls.is_empty() {
                    break;
                }
                return Err(AppError::Custom(format!("Stream read error: {e}")));
            }
        };
        utf8_decoder.push(&mut buffer, &chunk);

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if adapter.is_stream_done(&line) {
                if finish_reason.is_none() {
                    finish_reason = Some("stop".to_string());
                }
                continue;
            }

            let events = adapter.parse_stream_line(&line);
            for event in events {
                match event {
                    StreamEvent::TextDelta(text) => {
                        full_text.push_str(&text);
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
                    StreamEvent::Usage(_) => {}
                }
            }
        }
    }

    let valid_tool_calls: Vec<ToolCallRequest> = tool_calls
        .into_iter()
        .filter(|tc| !tc.name.is_empty())
        .map(|tc| ToolCallRequest {
            id: if tc.id.is_empty() {
                format!("call_{}", uuid::Uuid::new_v4().simple())
            } else {
                tc.id
            },
            name: tc.name,
            arguments: tc.arguments,
        })
        .collect();

    if full_text.is_empty() && valid_tool_calls.is_empty() && finish_reason.is_none() {
        return Err(AppError::Custom(
            "LLM returned empty stream for subagent".to_string(),
        ));
    }

    if !valid_tool_calls.is_empty() {
        Ok(CompletionResult::ToolCalls {
            calls: valid_tool_calls,
            preceding_text: full_text,
        })
    } else {
        Ok(CompletionResult::Message { text: full_text })
    }
}

fn parse_non_streaming_response(body: &str) -> AppResult<CompletionResult> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| AppError::Custom(format!("Failed to parse non-streaming response: {e}")))?;

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

    let tool_calls: Vec<ToolCallRequest> = message
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
        .unwrap_or_default();

    if !tool_calls.is_empty() {
        Ok(CompletionResult::ToolCalls {
            calls: tool_calls,
            preceding_text: text,
        })
    } else {
        Ok(CompletionResult::Message { text })
    }
}
