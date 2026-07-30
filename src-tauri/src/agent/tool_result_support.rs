use super::*;

pub(crate) fn apply_tool_result_sliding_window(
    history: &mut [ThreadMessage],
    keep_full: usize,
    keep_extended: usize,
) {
    let tool_indices: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, msg)| msg.role == "tool")
        .map(|(idx, _)| idx)
        .collect();

    let total_tools = tool_indices.len();
    if total_tools == 0 {
        return;
    }

    // Default window is `keep_full` (usually 6). High-value results may retain
    // full content for a longer extended window so mid-chain evidence survives.
    let max_window = keep_full.max(keep_extended);
    if total_tools <= keep_full {
        return;
    }

    for (tool_pos, &idx) in tool_indices.iter().enumerate() {
        let msg = &mut history[idx];
        if is_already_summarized_tool_result(&msg.content) {
            continue;
        }

        let age_from_end = total_tools.saturating_sub(tool_pos + 1);
        let tool_name = msg.tool_name.as_deref().unwrap_or("tool");
        let retention =
            tool_result_retention_for(tool_name, &msg.content, keep_full, keep_extended);
        if age_from_end < retention.min(max_window) {
            continue;
        }

        msg.content =
            summarize_old_tool_result(tool_name, &msg.content, TOOL_RESULT_SUMMARY_MAX_CHARS);
    }
}


pub(crate) fn is_already_summarized_tool_result(content: &str) -> bool {
    content.starts_with("[older tool result summarized]")
}


pub(crate) fn tool_result_retention_for(
    tool_name: &str,
    content: &str,
    default_keep: usize,
    extended_keep: usize,
) -> usize {
    if is_high_value_tool_result(tool_name, content) {
        default_keep.max(extended_keep)
    } else {
        default_keep
    }
}


pub(crate) fn is_high_value_tool_result(tool_name: &str, content: &str) -> bool {
    matches!(
        tool_name,
        "read_file" | "code_search" | "smartbrain_search" | "web_fetch"
    ) || content_has_critical_signals(content)
}


pub(crate) fn content_has_critical_signals(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    const SIGNALS: &[&str] = &[
        "error",
        "failed",
        "failure",
        "panic",
        "exit code",
        "exit_code",
        "permission denied",
        "access is denied",
        "traceback",
        "exception",
        "assert",
        "timeout",
        "not found",
        "no such file",
        "compilation failed",
        "cargo test",
        "failed to",
    ];
    SIGNALS.iter().any(|signal| lower.contains(signal))
}


pub(crate) fn extract_critical_tool_lines(content: &str, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }

    let mut selected = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !content_has_critical_signals(trimmed) && !looks_like_path_or_location_line(trimmed) {
            continue;
        }
        let normalized = trimmed.to_string();
        if !seen.insert(normalized.clone()) {
            continue;
        }
        selected.push(normalized);
        if selected.len() >= max_lines {
            break;
        }
    }
    selected
}


pub(crate) fn looks_like_path_or_location_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    // Common path/location cues that matter for later tool reuse.
    lower.contains("src/")
        || lower.contains("src\\")
        || lower.contains(".rs")
        || lower.contains(".ts")
        || lower.contains(".tsx")
        || lower.contains(".py")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains("file:")
        || lower.contains("path:")
        || lower.contains("line ")
        || lower.contains("line_offset")
        || line.contains(":\\")
        || (line.contains(':')
            && line.chars().any(|ch| ch.is_ascii_digit())
            && (line.contains('/') || line.contains('\\')))
}


pub(crate) fn summarize_old_tool_result(tool_name: &str, content: &str, max_chars: usize) -> String {
    let original_chars = content.chars().count();
    if original_chars <= max_chars {
        return format!(
            "[older tool result summarized] tool={tool_name}; chars={original_chars}\n{content}"
        );
    }

    let critical_lines = extract_critical_tool_lines(content, TOOL_RESULT_CRITICAL_LINES_MAX);
    let critical_block = if critical_lines.is_empty() {
        String::new()
    } else {
        format!("\n...[critical lines]...\n{}", critical_lines.join("\n"))
    };
    let critical_chars = critical_block.chars().count();
    let body_budget = max_chars.saturating_sub(critical_chars).max(160);
    let head_budget = body_budget / 2;
    let tail_budget = body_budget.saturating_sub(head_budget);
    let head: String = content.chars().take(head_budget).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(tail_budget)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let omitted = original_chars.saturating_sub(head.chars().count() + tail.chars().count());

    format!(
        "[older tool result summarized] tool={tool_name}; original_chars={original_chars}; omitted_chars={omitted}\n{head}{critical_block}\n...[truncated]...\n{tail}"
    )
}


