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
const COMPACTION_TOOL_RESULT_LIMIT: usize = 100;
const COMPACTION_TOOL_RESULT_MAX_CHARS: usize = 1_600;
const COMPACTION_TOOL_CALL_MAX_CHARS: usize = 1_200;
const COMPACTION_REQUEST_INPUT_PERCENT: usize = 70;
const COMPACTION_TARGET_HISTORY_PERCENT: usize = 50;
const COMPACTION_TARGET_THRESHOLD_PERCENT: usize = 60;
const COMPACTION_MAX_OUTPUT_TOKENS: usize = 8_192;
const COMPACTION_MIN_CONTEXT_WINDOW: usize = 4_096;
const DEFAULT_CONTEXT_WINDOW: i64 = 128_000;
const COMPACT_THRESHOLD_PERCENT: i64 = 80;

fn approx_token_count(text: &str) -> usize {
    text.len().div_ceil(3)
}

fn context_window_tokens(config: &ConfigToml) -> usize {
    config
        .model_context_window
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
        .max(COMPACTION_MIN_CONTEXT_WINDOW as i64) as usize
}

fn percent_of(value: usize, percent: usize) -> usize {
    value.saturating_mul(percent) / 100
}

fn compaction_request_input_budget(config: &ConfigToml) -> usize {
    percent_of(
        context_window_tokens(config),
        COMPACTION_REQUEST_INPUT_PERCENT,
    )
}

fn compacted_history_budget(config: &ConfigToml) -> usize {
    let context_budget = percent_of(
        context_window_tokens(config),
        COMPACTION_TARGET_HISTORY_PERCENT,
    );
    let trigger_budget = percent_of(
        compact_threshold(config).min(usize::MAX as u64) as usize,
        COMPACTION_TARGET_THRESHOLD_PERCENT,
    );
    context_budget.min(trigger_budget).max(1)
}

fn compaction_output_limit(config: &ConfigToml) -> i64 {
    let context_limit = percent_of(context_window_tokens(config), 15).max(512);
    let configured_limit = config
        .max_output_tokens
        .filter(|limit| *limit > 0)
        .map(|limit| limit as usize)
        .unwrap_or(COMPACTION_MAX_OUTPUT_TOKENS);
    configured_limit
        .min(COMPACTION_MAX_OUTPUT_TOKENS)
        .min(context_limit) as i64
}

fn truncate_to_token_budget(text: &str, max_tokens: usize) -> String {
    let max_bytes = max_tokens.saturating_mul(3);
    if text.len() <= max_bytes {
        return text.to_string();
    }
    if max_bytes == 0 {
        return String::new();
    }

    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn compact_threshold(config: &ConfigToml) -> u64 {
    let context_window = context_window_tokens(config) as u64;
    let percentage_threshold =
        context_window.saturating_mul(COMPACT_THRESHOLD_PERCENT as u64) / 100;
    if let Some(limit) = config.model_auto_compact_token_limit {
        if limit > 0 {
            // An absolute override may request earlier compaction, but it must never
            // move the trigger past the context-window percentage safety line.
            return (limit as u64).min(percentage_threshold).max(1);
        }
    }
    percentage_threshold.max(1)
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

fn internal_message_tokens(message: &InternalMessage) -> usize {
    message
        .content
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .map(approx_token_count)
        .unwrap_or_default()
}

fn build_compaction_messages(history: &[ThreadMessage], max_tokens: usize) -> Vec<InternalMessage> {
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

    let system = messages.remove(0);
    let mut remaining = max_tokens.saturating_sub(internal_message_tokens(&system));
    let mut selected = Vec::new();
    for mut message in messages.into_iter().rev() {
        let tokens = internal_message_tokens(&message);
        if tokens <= remaining {
            remaining = remaining.saturating_sub(tokens);
            selected.push(message);
            continue;
        }

        if remaining > 0
            && let Some(content) = message.content.as_ref().and_then(serde_json::Value::as_str)
        {
            let truncated = truncate_to_token_budget(content, remaining);
            if !truncated.is_empty() {
                message.content = text_content(truncated);
                selected.push(message);
            }
        }
        break;
    }
    selected.reverse();

    let mut fitted = Vec::with_capacity(selected.len() + 1);
    fitted.push(system);
    fitted.extend(selected);
    fitted
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
    max_tokens: usize,
) -> Vec<ThreadMessage> {
    let summary_prefix = format!("{SUMMARY_PREFIX}\n");
    let summary_body_budget = max_tokens.saturating_sub(approx_token_count(&summary_prefix));
    let summary_body_source = if summary_text.is_empty() {
        "(no summary available)"
    } else {
        summary_text
    };
    let summary_body = truncate_to_token_budget(summary_body_source, summary_body_budget);
    let final_summary = format!("{summary_prefix}{summary_body}");
    let summary_tokens = approx_token_count(&final_summary);
    let remaining = max_tokens.saturating_sub(summary_tokens);

    let user_budget = COMPACT_USER_MESSAGE_MAX_TOKENS.min(remaining / 2);
    let selected_users = select_recent_texts(user_messages, user_budget);
    let selected_user_tokens = selected_users
        .iter()
        .map(|message| approx_token_count(message))
        .sum::<usize>();
    let tool_budget = remaining.saturating_sub(selected_user_tokens);
    let selected_tool_context = select_recent_texts(recent_tool_context, tool_budget);

    let mut messages: Vec<ThreadMessage> = Vec::new();

    for text in &selected_users {
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

    for context in &selected_tool_context {
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

fn select_recent_texts(items: &[String], max_tokens: usize) -> Vec<String> {
    let mut selected = Vec::new();
    let mut remaining = max_tokens;
    for item in items.iter().rev() {
        if remaining == 0 {
            break;
        }
        let tokens = approx_token_count(item);
        if tokens <= remaining {
            selected.push(item.clone());
            remaining = remaining.saturating_sub(tokens);
        } else {
            let truncated = truncate_to_token_budget(item, remaining);
            if !truncated.is_empty() {
                selected.push(truncated);
            }
            break;
        }
    }
    selected.reverse();
    selected
}

fn estimated_history_tokens(history: &[ThreadMessage]) -> usize {
    history
        .iter()
        .map(|message| approx_token_count(&message.content))
        .sum()
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

    let history = thread_store.get_model_history(thread_id).await;
    if history.is_empty() {
        return Ok(());
    }

    let request_input_budget = compaction_request_input_budget(config);
    let summarization_prompt_tokens = approx_token_count(SUMMARIZATION_PROMPT);
    let history_input_budget = request_input_budget.saturating_sub(summarization_prompt_tokens);
    let mut messages = build_compaction_messages(&history, history_input_budget);

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
    let body = adapter.build_body(
        model,
        &messages,
        None,
        Some(compaction_output_limit(config)),
    );

    let estimated_input_tokens = messages.iter().map(internal_message_tokens).sum::<usize>();
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
    let recent_tool_context = collect_recent_tool_context(&history, COMPACTION_TOOL_RESULT_LIMIT);
    let history_budget = compacted_history_budget(config);
    let new_history = build_compacted_history(
        &user_messages,
        &summary_text,
        &recent_tool_context,
        history_budget,
    );
    let compacted_prompt_tokens = estimated_history_tokens(&new_history) as u64;

    let backup_path = thread_store
        .backup_thread_before_compaction(thread_id)
        .await?;
    info!(
        "Created pre-compaction rollout backup for thread {thread_id}: {}",
        backup_path.display()
    );

    thread_store
        .replace_model_history(thread_id, new_history, compacted_prompt_tokens)
        .await?;

    // compaction 后立即回推一份 token usage，确保前端在 idle/turn 间隙也能同步到新占用。
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

        let messages = build_compaction_messages(&[assistant, tool], 10_000);

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
        assert_eq!(compacted_history_budget(&config), 48_000);
    }

    #[test]
    fn compacted_history_budget_stays_below_custom_trigger() {
        let mut config = ConfigToml::default();
        config.model_context_window = Some(128_000);
        config.model_auto_compact_token_limit = Some(20_000);

        assert_eq!(compacted_history_budget(&config), 12_000);
    }

    #[test]
    fn absolute_trigger_cannot_exceed_context_percentage() {
        let mut config = ConfigToml::default();
        config.model_context_window = Some(65_535);
        config.model_auto_compact_token_limit = Some(80_000);

        assert_eq!(compact_threshold(&config), 52_428);
        assert!(!should_compact(52_427, &config));
        assert!(should_compact(52_428, &config));
    }

    #[test]
    fn smaller_absolute_trigger_can_request_earlier_compaction() {
        let mut config = ConfigToml::default();
        config.model_context_window = Some(100_000);
        config.model_auto_compact_token_limit = Some(40_000);

        assert_eq!(compact_threshold(&config), 40_000);
    }

    #[test]
    fn compaction_respects_dynamic_budgets_and_keeps_latest_results() {
        let history = (0..180)
            .map(|index| {
                let mut tool = message("tool", &format!("RESULT_{index}_{}", "x".repeat(3_000)));
                tool.tool_call_id = Some(format!("call-{index}"));
                tool.tool_name = Some("shell".to_string());
                tool
            })
            .collect::<Vec<_>>();

        let request_budget = 5_000;
        let messages = build_compaction_messages(&history, request_budget);
        let retained_results = messages
            .iter()
            .filter(|message| message_text(message).starts_with("[Command result:"))
            .count();

        assert!(retained_results > 0);
        assert!(retained_results < COMPACTION_TOOL_RESULT_LIMIT);
        assert!(messages.iter().map(internal_message_tokens).sum::<usize>() <= request_budget);
        assert!(
            messages
                .iter()
                .any(|message| { message_text(message).contains("RESULT_179_") })
        );

        let recent_context = collect_recent_tool_context(&history, COMPACTION_TOOL_RESULT_LIMIT);
        assert_eq!(recent_context.len(), COMPACTION_TOOL_RESULT_LIMIT);
        assert!(recent_context.last().unwrap().contains("RESULT_179_"));

        let history_budget = 10_000;
        let compacted = build_compacted_history(
            &["keep the long task running".to_string()],
            "handoff summary",
            &recent_context,
            history_budget,
        );
        let retained_contexts = compacted
            .iter()
            .filter(|message| message.content.starts_with("[Recent command context]"))
            .count();
        assert!(retained_contexts > 0);
        assert!(retained_contexts < COMPACTION_TOOL_RESULT_LIMIT);
        assert!(estimated_history_tokens(&compacted) <= history_budget);
        assert!(
            compacted
                .iter()
                .any(|message| message.content.contains("RESULT_179_"))
        );
        assert!(is_summary_message(&compacted.last().unwrap().content));
    }

    #[test]
    fn token_budget_truncation_stays_on_utf8_boundaries() {
        let text = "中文内容".repeat(1_000);
        let truncated = truncate_to_token_budget(&text, 100);

        assert!(approx_token_count(&truncated) <= 100);
        assert!(text.starts_with(&truncated));
    }
}
