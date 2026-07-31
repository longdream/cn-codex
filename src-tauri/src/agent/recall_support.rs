use super::*;

pub(crate) fn read_okf_body_lines_for_recall(path: &Path) -> Option<Vec<String>> {
    let raw_content = std::fs::read_to_string(path).ok()?;
    let content = crate::smartbrain::okf::extract_body(&raw_content);
    Some(content.lines().map(|line| line.to_string()).collect())
}


pub(crate) fn safe_recall_path(root: &Path, relative_path: &str) -> Option<std::path::PathBuf> {
    let root = root.canonicalize().ok()?;
    let candidate = root.join(relative_path);
    let candidate = candidate.canonicalize().ok()?;
    candidate.starts_with(&root).then_some(candidate)
}


pub(crate) fn take_first_lines_for_recall(lines: &[String], count: usize) -> Vec<String> {
    lines.iter().take(count).cloned().collect()
}


pub(crate) fn take_last_lines_for_recall(lines: &[String], count: usize) -> Vec<String> {
    if lines.len() <= count {
        return lines.to_vec();
    }
    lines[lines.len() - count..].to_vec()
}


pub(crate) fn build_smartbrain_recall_context(
    memories_dir: &Path,
    result: &crate::smartbrain::search::SmartBrainSearchResult,
    overlap_lines: usize,
    max_chars: usize,
) -> Option<String> {
    let overlap_lines = overlap_lines.max(30);
    let doc_path = safe_recall_path(memories_dir, &result.file_path)?;
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
        safe_recall_path(&docs_dir, &previous_file)
            .and_then(|path| read_okf_body_lines_for_recall(&path))
    } else {
        None
    };
    let next_lines = if chunk_index < chunk_total {
        let next_file =
            crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index + 1);
        safe_recall_path(&docs_dir, &next_file)
            .and_then(|path| read_okf_body_lines_for_recall(&path))
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


pub(crate) fn tool_spec_names(tools: &[serde_json::Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|spec| {
            spec.pointer("/function/name")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect()
}


pub(crate) fn repeated_read_only_tool_call(
    last_signature: &mut Option<String>,
    call: &ToolCallRequest,
) -> bool {
    if !matches!(
        call.name.as_str(),
        "read_file" | "code_search" | "list_directory"
    ) {
        *last_signature = None;
        return false;
    }

    let normalized_arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .ok()
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_else(|| call.arguments.trim().to_string());
    let signature = format!("{}:{normalized_arguments}", call.name);
    let repeated = last_signature.as_deref() == Some(signature.as_str());
    *last_signature = Some(signature);
    repeated
}


pub(crate) fn assistant_is_waiting_for_user(text: &str) -> bool {
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
pub(crate) fn turn_budget_limited(goal_budget_tokens: Option<u64>, usage: &TurnUsage) -> bool {
    goal_budget_tokens.is_some_and(|budget| budget > 0 && usage.total_tokens >= budget)
}


pub(crate) fn goal_budget_limited_after(goal: Option<&ThreadGoal>, usage: &TurnUsage) -> bool {
    let Some(goal) = goal else {
        return false;
    };
    goal.token_budget.is_some_and(|budget| {
        budget > 0 && goal.tokens_used.saturating_add(usage.total_tokens) >= budget
    })
}


