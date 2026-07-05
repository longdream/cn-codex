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
const DEFAULT_CONTEXT_WINDOW: i64 = 128_000;
const COMPACT_THRESHOLD_PERCENT: i64 = 90;

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

pub fn build_compacted_history(user_messages: &[String], summary_text: &str) -> Vec<ThreadMessage> {
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

    let mut messages: Vec<InternalMessage> = Vec::new();

    messages.push(InternalMessage {
        role: "system".to_string(),
        content: text_content(
            "You are a helpful assistant. Summarize the conversation history.".to_string(),
        ),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    for msg in &history {
        if msg.role == "tool" || msg.tool_call_id.is_some() {
            continue;
        }
        messages.push(InternalMessage {
            role: msg.role.clone(),
            content: text_content(msg.content.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

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
    let cancelled = |flag: Option<&Arc<AtomicBool>>| flag.is_some_and(|f| f.load(Ordering::SeqCst));

    while let Some(chunk) = stream.next().await {
        if cancelled(cancel_flag) {
            info!("Compaction cancelled by user during streaming");
            return Err(AppError::Custom("Compaction cancelled".to_string()));
        }
        let chunk = chunk.map_err(|e| AppError::Custom(format!("Stream error: {e}")))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];
            if adapter.is_stream_done(data) {
                break;
            }

            for event in adapter.parse_stream_line(data) {
                if let StreamEvent::TextDelta(delta) = event {
                    summary_text.push_str(&delta);
                }
            }
        }
    }

    let summary_text = summary_text.trim().to_string();
    info!(
        "Compaction complete in {:.1}s. Summary: {} chars (~{} tokens). Input: ~{estimated_input_tokens} tokens",
        compaction_start.elapsed().as_secs_f64(),
        summary_text.len(),
        summary_text.len() / 3
    );

    let user_messages = collect_user_messages(&history);
    let new_history = build_compacted_history(&user_messages, &summary_text);

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
            "callCount": 0_u64,
            "lastSinglePromptTokens": compacted_prompt_tokens,
            "contextWindowTokens": model_context_window,
        },
        "inputTokens": compacted_prompt_tokens,
        "outputTokens": 0_u64,
        "totalTokens": compacted_prompt_tokens,
        "callCount": 0_u64,
        "lastSinglePromptTokens": compacted_prompt_tokens,
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
