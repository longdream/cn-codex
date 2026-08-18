use super::*;

pub(crate) fn is_retryable_rate_limit_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("rate_limited")
}


pub(crate) fn is_retryable_upstream_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    ["502", "503", "504"]
        .iter()
        .any(|status| lower.contains(status))
        || lower.contains("upstream_error")
        || lower.contains("upstream request failed")
}


pub(crate) fn is_retryable_transient_llm_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("empty response")
        || lower.contains("timed out waiting for response headers")
        || lower.contains("error sending request")
        || lower.contains("connection refused")
        || lower.contains("dns error")
        || lower.contains("failed to connect")
}


pub(crate) fn transient_llm_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(3);
    (1_000_u64 << shift).min(8_000)
}


pub(crate) fn upstream_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(3);
    (1_000_u64 << shift).min(8_000)
}


pub(crate) fn is_retryable_stream_read_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("stream read error")
        || lower.contains("error decoding response body")
        || lower.contains("connection reset")
        || lower.contains("connection closed")
        || lower.contains("unexpected eof")
        || lower.contains("incomplete message")
}

pub(crate) fn is_oversized_model_response_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("model response exceeded the") && lower.contains("safety limit")
}


pub(crate) fn stream_ended_without_terminal_marker(finish_reason: Option<&str>) -> bool {
    finish_reason.is_none()
}


pub(crate) fn stream_read_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(4);
    (1_000_u64 << shift).min(10_000)
}


/// Decide whether the outer goal loop should inject another continuation.
/// Fatal LLM failures must end the turn so the thread lock is released and the
/// user can continue in the same conversation without opening a new thread.
pub(crate) fn should_continue_goal_loop(
    turn_mode: &str,
    cancelled: bool,
    prompt_hook_blocked: bool,
    terminated_by_error: bool,
    goal_is_active: bool,
    goal_continuation_count: usize,
    max_goal_continuations: usize,
) -> bool {
    if turn_mode != "goal" || cancelled || prompt_hook_blocked || terminated_by_error {
        return false;
    }
    goal_is_active && goal_continuation_count < max_goal_continuations
}


pub(crate) fn repeated_goal_stop_response(
    last_response: &mut Option<String>,
    response: &str,
) -> bool {
    let normalized = response.trim();
    if normalized.is_empty() {
        return false;
    }

    let repeated = last_response.as_deref() == Some(normalized);
    *last_response = Some(normalized.to_string());
    repeated
}


/// Empty streams and header timeouts are retryable inside one agent loop, but once
/// the inner loop has already marked the turn as terminated they must not restart
/// Goal continuation. Otherwise the thread stays locked under `active_threads`
/// while the UI looks idle and the user cannot send another message.
#[cfg(test)]
pub(crate) fn should_end_goal_turn_after_llm_error(error_message: &str, terminated_by_error: bool) -> bool {
    terminated_by_error
        && (is_retryable_transient_llm_error(error_message)
            || error_message.to_ascii_lowercase().contains("empty response")
            || error_message
                .to_ascii_lowercase()
                .contains("timed out waiting for response headers"))
}


/// Empty response / header timeout policy for Goal turns:
/// 1. keep retrying inside the current agent loop while attempts remain;
/// 2. after retries are exhausted, end the turn and release the lock;
/// 3. never convert the exhausted failure into Goal continuation.
pub(crate) fn should_retry_transient_llm_error_before_ending_goal_turn(
    error_message: &str,
    transient_llm_retry_count: u32,
    max_transient_llm_retries: u32,
) -> bool {
    is_retryable_transient_llm_error(error_message)
        && transient_llm_retry_count < max_transient_llm_retries
}


pub(crate) fn rate_limit_backoff_ms(attempt: u32) -> u64 {
    let normalized_attempt = attempt.max(1);
    let shift = normalized_attempt.saturating_sub(1).min(20);
    let multiplier = 1_u64 << shift;
    (1_000_u64.saturating_mul(multiplier)).min(30_000)
}


pub(crate) fn text_expresses_intent(text: &str) -> bool {
    let lower = text.to_lowercase();
    let intent_patterns = [
        "let me ",
        "i'll ",
        "i will ",
        "i am going to",
        "i'm going to",
        "i'm about to",
        "going to ",
        "start implementing",
        "continue implementing",
        "continue working",
        "now implement",
        "will implement",
        "will update",
        "will modify",
        "will patch",
        "will check",
        "will read",
        "will search",
        "will fix",
        "need to check",
        "need to read",
        "need to update",
        "need to implement",
        "need to fix",
        "让我",
        "接下来",
        "我来",
        "我将",
        "我先",
        "我接着",
        "接着改",
        "接着实现",
        "接着修",
        "开始落地",
        "开始实现",
        "开始修",
        "开始改",
        "继续实现",
        "继续修",
        "继续改",
        "继续落地",
        "继续把",
        "正在修改",
        "正在实现",
        "正在批量",
        "正在改",
        "落地改动",
        "准备修改",
        "准备实现",
        "需要修改",
        "需要实现",
        "需要检查",
        "需要读取",
        "需要查看",
        "需要搜索",
        "查看一下",
        "检查一下",
        "读取一下",
        "看看",
        "分析一下",
    ];
    intent_patterns.iter().any(|p| lower.contains(p))
}


pub(crate) fn text_contains_unapplied_patch(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("*** begin patch")
        || lower.contains("*** update file:")
        || lower.contains("```diff")
        || lower.contains("```patch")
}


pub(crate) fn user_requested_patch_text_only(text: &str) -> bool {
    let lower = text.to_lowercase();
    let text_only_patterns = [
        "只给补丁",
        "只输出补丁",
        "只展示补丁",
        "不要应用",
        "不要修改文件",
        "不要改文件",
        "don't apply",
        "do not apply",
        "patch only",
        "show me the patch",
        "show the patch",
        "output the patch",
    ];
    text_only_patterns
        .iter()
        .any(|pattern| lower.contains(pattern))
}


pub(crate) fn is_length_truncated(finish_reason: Option<&str>) -> bool {
    match finish_reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(reason) => {
            let lower = reason.to_ascii_lowercase();
            matches!(
                lower.as_str(),
                "length" | "max_tokens" | "max_output_tokens" | "max_output_tokens_reached"
            )
        }
        None => false,
    }
}


pub(crate) fn looks_like_textual_tool_protocol_leak(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const PROTOCOL_MARKERS: &[&str] = &[
        "<|recipient|>",
        "<|channel|>",
        "<tool_call>",
        "assistant to=",
        "analysis to=",
        "commentary to=",
        "recipient=",
    ];
    if PROTOCOL_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }

    // Some incompatible gateways strip the control tokens but leave a runaway
    // sequence such as `shell2 shell3 ... shell225` in assistant content.
    lower
        .split_whitespace()
        .filter(|token| {
            let token = token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_');
            let Some(suffix) = token.strip_prefix("shell") else {
                return false;
            };
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
        })
        .take(8)
        .count()
        >= 8
}


pub(crate) fn tool_protocol_mismatch_error(model: &str) -> AppError {
    AppError::Custom(format!(
        "Tool protocol mismatch for model `{model}`: the provider returned textual tool-call markers in assistant content instead of structured `tool_calls`. Configure this provider with `wire_api = \"responses\"` (or use a Chat Completions endpoint that supports external function tools)."
    ))
}


pub(crate) fn extract_non_streaming_text(body_text: &str) -> Option<String> {
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


