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
    let is_builtin_read_only = matches!(
        call.name.as_str(),
        "read_file" | "code_search" | "code_review" | "list_directory"
    );
    if !is_builtin_read_only && !is_read_only_shell_tool_call(call) {
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


pub(crate) fn is_read_only_shell_tool_call(call: &ToolCallRequest) -> bool {
    matches!(
        call.name.as_str(),
        "shell" | "shell_command" | "exec_command"
    ) && shell_command_from_args(&call.arguments)
        .as_deref()
        .is_some_and(is_read_only_shell_command)
}


pub(crate) fn blocked_read_only_shell_repeat_should_stop(repeat_count: &mut u32) -> bool {
    *repeat_count = repeat_count.saturating_add(1);
    *repeat_count >= MAX_CONSECUTIVE_BLOCKED_READ_ONLY_SHELL_CALLS
}


pub(crate) fn is_read_only_shell_command(command: &str) -> bool {
    let Some(segments) = split_read_only_shell_segments(command) else {
        return false;
    };
    !segments.is_empty()
        && segments
            .iter()
            .all(|segment| is_read_only_shell_segment(segment))
}


fn split_read_only_shell_segments(command: &str) -> Option<Vec<String>> {
    let chars = command.chars().collect::<Vec<_>>();
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        match quote {
            Some(marker) => {
                if ch == marker {
                    quote = None;
                }
                if marker == '"'
                    && ((ch == '$' || ch == '@') && chars.get(index + 1) == Some(&'(')
                        || ch == '`')
                {
                    return None;
                }
                current.push(ch);
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    current.push(ch);
                } else if matches!(ch, '>' | '<' | '&' | '`' | '{' | '}' | '#')
                    || ((ch == '$' || ch == '@') && chars.get(index + 1) == Some(&'('))
                {
                    return None;
                } else if matches!(ch, ';' | '|' | '\n' | '\r') {
                    let segment = current.trim();
                    if segment.is_empty() {
                        return None;
                    }
                    segments.push(segment.to_string());
                    current.clear();
                } else {
                    current.push(ch);
                }
            }
        }
        index += 1;
    }

    if quote.is_some() {
        return None;
    }
    let tail = current.trim();
    if tail.is_empty() {
        return None;
    }
    segments.push(tail.to_string());
    Some(segments)
}


fn is_read_only_shell_segment(segment: &str) -> bool {
    let tokens = shell_command_tokens(segment);
    let Some(program) = tokens.first().map(|value| value.to_ascii_lowercase()) else {
        return false;
    };

    match program.as_str() {
        "git" | "git.exe" => is_read_only_git_invocation(&tokens[1..]),
        "echo" | "printf" | "write-output" | "head" | "tail" | "cut" | "wc"
        | "more" | "findstr" | "select-object" | "format-table" | "format-list"
        | "format-wide" | "format-custom" | "out-string" | "measure-object" => true,
        _ => false,
    }
}


fn is_read_only_git_invocation(arguments: &[String]) -> bool {
    let Some((subcommand, rest)) = arguments.split_first() else {
        return false;
    };
    let subcommand = subcommand.to_ascii_lowercase();
    if git_arguments_can_write_or_execute(rest) {
        return false;
    }

    match subcommand.as_str() {
        "status" | "log" | "diff" | "show" | "rev-parse" | "ls-files" | "ls-tree"
        | "shortlog" | "describe" | "name-rev" | "merge-base" | "diff-tree"
        | "diff-index" | "diff-files" | "for-each-ref" | "count-objects" => true,
        "branch" => is_read_only_git_branch(rest),
        "tag" => is_read_only_git_tag(rest),
        "remote" => is_read_only_git_remote(rest),
        "stash" => rest.first().is_some_and(|value| {
            matches!(value.to_ascii_lowercase().as_str(), "list" | "show")
        }),
        "reflog" => rest.first().map_or(true, |value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "show" | "list" | "exists"
            )
        }),
        "worktree" => rest
            .first()
            .is_some_and(|value| value.eq_ignore_ascii_case("list")),
        _ => false,
    }
}


fn git_arguments_can_write_or_execute(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        let lower = argument.to_ascii_lowercase();
        matches!(lower.as_str(), "--output" | "--ext-diff" | "--textconv")
            || lower.starts_with("--output=")
            || lower.starts_with("--exec=")
    })
}


fn is_read_only_git_branch(arguments: &[String]) -> bool {
    arguments.is_empty()
        || arguments.iter().all(|argument| {
            let lower = argument.to_ascii_lowercase();
            matches!(
                lower.as_str(),
                "--list"
                    | "--all"
                    | "-a"
                    | "--remotes"
                    | "-r"
                    | "--show-current"
                    | "--verbose"
                    | "-v"
                    | "-vv"
                    | "--column"
                    | "--no-column"
                    | "--color"
                    | "--no-color"
                    | "--ignore-case"
                    | "-i"
            ) || [
                "--format=",
                "--sort=",
                "--contains=",
                "--no-contains=",
                "--merged=",
                "--no-merged=",
                "--points-at=",
                "--color=",
                "--column=",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        })
}


fn is_read_only_git_tag(arguments: &[String]) -> bool {
    arguments.is_empty()
        || arguments.iter().all(|argument| {
            let lower = argument.to_ascii_lowercase();
            matches!(lower.as_str(), "--list" | "-l" | "--ignore-case" | "-i")
                || lower.starts_with("-n")
                || [
                    "--format=",
                    "--sort=",
                    "--contains=",
                    "--no-contains=",
                    "--merged=",
                    "--no-merged=",
                    "--points-at=",
                    "--color=",
                    "--column=",
                ]
                .iter()
                .any(|prefix| lower.starts_with(prefix))
        })
}


fn is_read_only_git_remote(arguments: &[String]) -> bool {
    let Some((operation, rest)) = arguments.split_first() else {
        return true;
    };
    let operation = operation.to_ascii_lowercase();
    if matches!(operation.as_str(), "-v" | "--verbose") {
        return rest.is_empty();
    }
    matches!(operation.as_str(), "get-url" | "show")
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


