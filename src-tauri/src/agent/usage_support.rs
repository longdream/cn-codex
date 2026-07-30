use super::*;

pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}


pub(crate) fn add_turn_usage(total: &mut TurnUsage, usage: &UsageInfo) {
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


pub(crate) fn nonzero_turn_usage(usage: &TurnUsage) -> Option<TurnUsage> {
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


pub(crate) fn emit_turn_usage_updated(
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


pub(crate) fn resolve_model_context_window_tokens(config: &ConfigToml) -> u64 {
    let configured = config.model_context_window.unwrap_or(128_000);
    if configured > 0 {
        configured as u64
    } else {
        128_000
    }
}


/// 基于字符串内容估算 token 数（中英文混合约 2-4 chars/token，取 3 折中）
pub(crate) fn estimate_tokens(text: &str) -> u64 {
    let char_count = text.chars().count() as u64;
    estimate_tokens_from_char_count(char_count)
}


pub(crate) fn estimate_tokens_from_char_count(char_count: u64) -> u64 {
    (char_count / 3).max(1)
}


pub(crate) fn truncate_chars_with_marker(value: &str, max_chars: usize) -> String {
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


