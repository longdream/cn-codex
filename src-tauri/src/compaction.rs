use std::collections::HashMap;
use std::time::Instant;
use tracing::info;

use crate::adapter::{
    self,
    types::{InternalMessage, StreamEvent, text_content},
};
use crate::config_system::ConfigToml;
use crate::error::{AppError, AppResult};
use crate::thread_store::{ThreadMessage, ThreadStore};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

pub const SUMMARIZATION_PROMPT: &str = "\
You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that will resume the task.

Include:
- Current progress and key decisions made
- Important context, constraints, or user preferences
- What remains to be done (clear next steps)
- Any critical data, examples, or references needed to continue

Be concise, structured, and focused on helping the next LLM seamlessly continue the work.";

pub const SUMMARY_PREFIX: &str = "\
Another language model started to solve this problem and produced a summary of its thinking process. \
You also have access to the state of the tools that were used by that language model. \
Use this to build on the work that has already been done and avoid duplicating work. \
Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:";

const COMPACT_USER_MESSAGE_MAX_TOKENS: usize = 20_000;
const COMPACTION_MIN_TOOL_RESULTS: usize = 100;
const COMPACTION_TOOL_RESULT_MAX_CHARS: usize = 1_600;
const COMPACTION_TOOL_CALL_MAX_CHARS: usize = 1_200;
const COMPACTION_MAX_INPUT_CHARS: usize = 240_000;
const DEFAULT_CONTEXT_WINDOW: i64 = 128_000;
const COMPACT_THRESHOLD_PERCENT: i64 = 80;

fn approx_token_count(text: &str) -> usize {
    text.len() / 3
}

pub fn compact_threshold(config: &ConfigToml) -> u64 {
    if let Some(limit) = config.model_auto_compact_token_limit {
        if limit > 0 {
            return limit as u64;
        }
    }
    let context_window = config
        .model_context_window
        .unwrap_or(DEFAULT_CONTEXT_WINDOW);
    ((context_window * COMPACT_THRESHOLD_PERCENT) / 100) as u64
}

pub fn should_compact(prompt_tokens: u64, config: &ConfigToml) -> bool {
    prompt_tokens >= compact_threshold(config)
}

pub fn is_summary_message(content: &str) -> bool {
    content.starts_with(SUMMARY_PREFIX)
}

fn collect_user_messages(history: &[ThreadMessage]) -> Vec<String> {
    history
        .iter()
        .filter(|m| m.role == "user")
        .filter(|m| !is_summary_message(&m.content))
        .map(|m| m.content.clone())
        .collect()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn truncate_compaction_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let head_chars = max_chars.saturating_mul(2) / 3;
    let tail_chars = max_chars.saturating_sub(head_chars);
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}\n...[compaction truncation]...\n{tail}")
}

fn build_compaction_messages(history: &[ThreadMessage]) -> Vec<InternalMessage> {
    let mut messages = Vec::with_capacity(history.len() + 1);
    messages.push(InternalMessage {
        role: "system".to_string(),
        content: text_content(
            "You are a helpful assistant. Summarize the conversation history, including command requests, command results, file paths, and failures."
                .to_string(),
        ),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    for msg in history {
        if msg.role == "tool" {
            let tool_name = msg.tool_name.as_deref().unwrap_or("tool");
            let result = truncate_compaction_text(&msg.content, COMPACTION_TOOL_RESULT_MAX_CHARS);
            messages.push(InternalMessage {
                // Keep the compaction transcript protocol-neutral. A raw `tool` role
                // would require a matching assistant tool-call message in the summary API.
                role: "user".to_string(),
                content: text_content(format!("[Command result: {tool_name}]\n{result}")),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
            continue;
        }

        let mut content = msg.content.clone();
        if let Some(tool_calls) = msg.tool_calls.as_ref().filter(|calls| !calls.is_empty()) {
            let calls = tool_calls
                .iter()
                .map(|call| {
                    format!(
                        "- {} ({}) {}",
                        call.name,
                        call.id,
                        truncate_compaction_text(&call.arguments, COMPACTION_TOOL_CALL_MAX_CHARS)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !content.trim().is_empty() {
                content.push_str("\n\n");
            }
            content.push_str("[Assistant command requests]\n");
            content.push_str(&calls);
        }

        if content.trim().is_empty() {
            continue;
        }
        messages.push(InternalMessage {
            role: msg.role.clone(),
            content: text_content(content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    // Keep the summarization request bounded even for very long robot/agent turns.
    // Drop the oldest command transcripts first; user and assistant decisions remain.
    while messages
        .iter()
        .map(|message| {
            message
                .content
                .as_ref()
                .map(|value| value.to_string().len())
                .unwrap_or(0)
        })
        .sum::<usize>()
        > COMPACTION_MAX_INPUT_CHARS
    {
        let tool_result_count = messages
            .iter()
            .filter(|message| {
                message.content.as_ref().is_some_and(|content| {
                    content
                        .as_str()
                        .is_some_and(|text| text.starts_with("[Command result:"))
                })
            })
            .count();
        if tool_result_count <= COMPACTION_MIN_TOOL_RESULTS {
            break;
        }
        let Some(index) = messages.iter().position(|message| {
            message.role == "user"
                && message.content.as_ref().is_some_and(|content| {
                    content
                        .as_str()
                        .is_some_and(|text| text.starts_with("[Command result:"))
                })
        }) else {
            break;
        };
        messages.remove(index);
    }

    messages
}

fn collect_recent_tool_context(history: &[ThreadMessage], limit: usize) -> Vec<String> {
    let calls = history
        .iter()
        .filter_map(|message| message.tool_calls.as_ref())
        .flatten()
        .map(|call| (call.id.clone(), (call.name.clone(), call.arguments.clone())))
        .collect::<HashMap<_, _>>();

    let mut contexts = history
        .iter()
        .rev()
        .filter(|message| message.role == "tool")
        .take(limit)
        .map(|message| {
            let call = message
                .tool_call_id
                .as_ref()
                .and_then(|call_id| calls.get(call_id));
            let tool_name = call
                .map(|(name, _)| name.as_str())
                .or(message.tool_name.as_deref())
                .unwrap_or("tool");
            let arguments = call
                .map(|(_, arguments)| {
                    truncate_compaction_text(arguments, COMPACTION_TOOL_CALL_MAX_CHARS)
                })
                .unwrap_or_else(|| "(arguments unavailable)".to_string());
            let result =
                truncate_compaction_text(&message.content, COMPACTION_TOOL_RESULT_MAX_CHARS);
            format!(
                "[Recent command context]\ncommand: {tool_name}\narguments: {arguments}\nresult:\n{result}"
            )
        })
        .collect::<Vec<_>>();
    contexts.reverse();
    contexts
}

pub fn build_compacted_history(
    user_messages: &[String],
    summary_text: &str,
    recent_tool_context: &[String],
) -> Vec<ThreadMessage> {
    let mut selected: Vec<String> = Vec::new();
    let mut remaining = COMPACT_USER_MESSAGE_MAX_TOKENS;

    for message in user_messages.iter().rev() {
        if remaining == 0 {
            break;
        }
        let tokens = approx_token_count(message);
        if tokens <= remaining {
            selected.push(message.clone());
            remaining = remaining.saturating_sub(tokens);
        } else {
            let chars_budget = remaining * 3;
            let truncated: String = message.chars().take(chars_budget).collect();
            selected.push(truncated);
            break;
        }
    }
    selected.reverse();

    let mut messages: Vec<ThreadMessage> = Vec::new();

    for text in &selected {
        messages.push(ThreadMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: "user".to_string(),
            content: text.clone(),
            timestamp: now_secs(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        });
    }

    for context in recent_tool_context {
        messages.push(ThreadMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: "user".to_string(),
            content: context.clone(),
            timestamp: now_secs(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        });
    }

    let final_summary = if summary_text.is_empty() {
        format!("{SUMMARY_PREFIX}\n(no summary available)")
    } else {
        format!("{SUMMARY_PREFIX}\n{summary_text}")
    };

    messages.push(ThreadMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: final_summary,
        timestamp: now_secs(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        attachments: Vec::new(),
    });

    messages
}

pub async fn run_compaction(
    http: &reqwest::Client,
    app_handle: &AppHandle,
    config: &ConfigToml,
    thread_store: &Arc<ThreadStore>,
    thread_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
    cancel_flag: Option<&Arc<AtomicBool>>,
    query_params: Option<&std::collections::HashMap<String, String>>,
    extra_headers: Option<&std::collections::HashMap<String, String>>,
) -> AppResult<()> {
    let compaction_start = Instant::now();
    info!("Starting context compaction for thread {thread_id}");

    let payload = serde_json::json!({ "threadId": thread_id });
    app_handle.emit("compaction-started", payload.clone()).ok();
    crate::mobile_server::broadcast("compaction-started", payload);

    let history = thread_store.get_thread_messages(thread_id).await;
    if history.is_empty() {
        return Ok(());
    }

    let mut messages = build_compaction_messages(&history);

    messages.push(InternalMessage {
        role: "user".to_string(),
        content: text_content(SUMMARIZATION_PROMPT.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let adapter = adapter::get_adapter(wire_api);
    let url = adapter.build_url(base_url, model);
    let headers = adapter.build_headers(api_key);
    let (url, headers) =
        adapter::apply_request_overrides(url, headers, query_params, extra_headers)
            .map_err(AppError::Custom)?;
    let body = adapter.build_body(model, &messages, None, config.max_output_tokens);

    let input_chars: usize = messages
        .iter()
        .map(|m| m.content.as_ref().map(|c| c.to_string().len()).unwrap_or(0))
        .sum();
    let estimated_input_tokens = input_chars / 3;
    info!(
        "Compaction LLM request: url={url}, model={model}, history_msgs={}, estimated_input_tokens={estimated_input_tokens}",
        history.len()
    );

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("Compaction request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(AppError::Custom(format!(
            "Compaction LLM returned {status}: {body_text}"
        )));
    }

    let mut summary_text = String::new();
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    let mut buffer = String::new();
    let mut utf8_decoder = crate::utf8_stream::Utf8StreamDecoder::new();
    let cancelled = |flag: Option<&Arc<AtomicBool>>| flag.is_some_and(|f| f.load(Ordering::SeqCst));

    while let Some(chunk) = stream.next().await {
        if cancelled(cancel_flag) {
            info!("Compaction cancelled by user during streaming");
            return Err(AppError::Custom("Compaction cancelled".to_string()));
        }
        let chunk = chunk.map_err(|e| AppError::Custom(format!("Stream error: {e}")))?;
        utf8_decoder.push(&mut buffer, &chunk);

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if adapter.is_stream_done(&line) {
                break;
            }

            for event in adapter.parse_stream_line(&line) {
                if let StreamEvent::TextDelta(delta) = event {
                    summary_text.push_str(&delta);
                }
            }
        }
    }

    let summary_text = summary_text.trim().to_string();
    if summary_text.is_empty() {
        return Err(AppError::Custom(
            "Compaction returned an empty summary; original history was preserved.".to_string(),
        ));
    }
    info!(
        "Compaction complete in {:.1}s. Summary: {} chars (~{} tokens). Input: ~{estimated_input_tokens} tokens",
        compaction_start.elapsed().as_secs_f64(),
        summary_text.len(),
        summary_text.len() / 3
    );

    let user_messages = collect_user_messages(&history);
    let recent_tool_context = collect_recent_tool_context(&history, COMPACTION_MIN_TOOL_RESULTS);
    let new_history = build_compacted_history(&user_messages, &summary_text, &recent_tool_context);

    let backup_path = thread_store
        .backup_thread_before_compaction(thread_id)
        .await?;
    info!(
        "Created pre-compaction rollout backup for thread {thread_id}: {}",
        backup_path.display()
    );

    thread_store
        .replace_messages(thread_id, new_history)
        .await?;

    // compaction 后立即回推一份 token usage，确保前端在 idle/turn 间隙也能同步到新占用。
    let compacted_prompt_tokens = thread_store.get_thread_total_tokens(thread_id).await;
    let model_context_window = config
        .model_context_window
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
        .max(1) as u64;
    let usage_payload = serde_json::json!({
        "threadId": thread_id,
        "usage": {
            "promptTokens": compacted_prompt_tokens,
            "completionTokens": 0_u64,
            "totalTokens": compacted_prompt_tokens,
            "cachedTokens": 0_u64,
            "cacheCreationTokens": 0_u64,
            "reasoningTokens": 0_u64,
            "callCount": 0_u64,
            "lastSinglePromptTokens": compacted_prompt_tokens,
            "contextWindowTokens": model_context_window,
        },
        "inputTokens": compacted_prompt_tokens,
        "outputTokens": 0_u64,
        "totalTokens": compacted_prompt_tokens,
        "callCount": 0_u64,
        "lastSinglePromptTokens": compacted_prompt_tokens,
        "cachedTokens": 0_u64,
        "cacheCreationTokens": 0_u64,
        "reasoningTokens": 0_u64,
        "contextPromptTokens": compacted_prompt_tokens,
        "modelContextWindow": model_context_window,
    });
    app_handle
        .emit("thread-token-usage-updated", usage_payload.clone())
        .ok();
    crate::mobile_server::broadcast("thread-token-usage-updated", usage_payload);

    let compacted_payload = serde_json::json!({
        "threadId": thread_id,
        "summaryLength": summary_text.len(),
        "contextPromptTokens": compacted_prompt_tokens,
        "modelContextWindow": model_context_window,
    });
    app_handle
        .emit("context-compacted", compacted_payload.clone())
        .ok();
    crate::mobile_server::broadcast("context-compacted", compacted_payload);

    info!("Context compaction applied for thread {thread_id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread_store::ToolCallInfo;

    fn message(role: &str, content: &str) -> ThreadMessage {
        ThreadMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: 1,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        }
    }

    fn message_text(message: &InternalMessage) -> &str {
        message
            .content
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }

    #[test]
    fn compaction_transcript_keeps_command_requests_and_results() {
        let mut assistant = message("assistant", "checking the file");
        assistant.tool_calls = Some(vec![ToolCallInfo {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
        }]);
        let mut tool = message("tool", "src/main.rs:42 error: stale context");
        tool.tool_call_id = Some("call-1".to_string());
        tool.tool_name = Some("read_file".to_string());

        let messages = build_compaction_messages(&[assistant, tool]);

        assert!(messages.iter().any(|message| {
            message_text(message).contains("[Assistant command requests]")
                && message_text(message).contains("src/main.rs")
        }));
        assert!(messages.iter().any(|message| {
            message_text(message).contains("[Command result: read_file]")
                && message_text(message).contains("stale context")
        }));
    }

    #[test]
    fn default_compaction_threshold_leaves_twenty_percent_headroom() {
        let mut config = ConfigToml::default();
        config.model_auto_compact_token_limit = None;
        config.model_context_window = Some(100_000);

        assert_eq!(compact_threshold(&config), 80_000);
    }

    #[test]
    fn compaction_transcript_preserves_at_least_one_hundred_recent_results() {
        let history = (0..180)
            .map(|index| {
                let mut tool = message("tool", &format!("RESULT_{index}_{}", "x".repeat(3_000)));
                tool.tool_call_id = Some(format!("call-{index}"));
                tool.tool_name = Some("shell".to_string());
                tool
            })
            .collect::<Vec<_>>();

        let messages = build_compaction_messages(&history);
        let retained_results = messages
            .iter()
            .filter(|message| message_text(message).starts_with("[Command result:"))
            .count();

        assert!(retained_results >= COMPACTION_MIN_TOOL_RESULTS);
        assert!(
            messages
                .iter()
                .any(|message| { message_text(message).contains("RESULT_179_") })
        );

        let recent_context = collect_recent_tool_context(&history, COMPACTION_MIN_TOOL_RESULTS);
        assert_eq!(recent_context.len(), COMPACTION_MIN_TOOL_RESULTS);
        assert!(recent_context.last().unwrap().contains("RESULT_179_"));

        let compacted = build_compacted_history(
            &["keep the long task running".to_string()],
            "handoff summary",
            &recent_context,
        );
        assert_eq!(
            compacted
                .iter()
                .filter(|message| message.content.starts_with("[Recent command context]"))
                .count(),
            COMPACTION_MIN_TOOL_RESULTS
        );
    }
}
