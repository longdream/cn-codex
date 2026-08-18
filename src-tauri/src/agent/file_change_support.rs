use super::*;

pub(crate) fn normalize_change_path(path: &str) -> String {
    let trimmed = path.trim();
    trimmed
        .strip_suffix(" ***")
        .unwrap_or(trimmed)
        .trim_end()
        .replace('\\', "/")
}


pub(crate) fn upsert_file_snapshot_entry<'a>(
    snapshot_map: &'a mut BTreeMap<String, FileChangeSnapshot>,
    change: &FileChange,
) -> &'a mut FileChangeSnapshot {
    let path = normalize_change_path(&change.path);
    snapshot_map
        .entry(path.clone())
        .or_insert_with(|| FileChangeSnapshot {
            path,
            action: change.action.clone(),
            before_content: None,
            after_content: None,
        })
}


pub(crate) fn capture_before_file_snapshots(
    snapshot_map: &mut BTreeMap<String, FileChangeSnapshot>,
    changes: &[FileChange],
    cwd: &Path,
) {
    for change in changes {
        let entry = upsert_file_snapshot_entry(snapshot_map, change);
        entry.action = change.action.clone();
        // before 只采集第一次，确保“本轮起始基线”稳定，不被后续同文件多次修改覆盖。
        if entry.before_content.is_none() {
            entry.before_content = read_text_file_snapshot(cwd, &entry.path);
        }
    }
}


pub(crate) fn capture_after_file_snapshots(
    snapshot_map: &mut BTreeMap<String, FileChangeSnapshot>,
    changes: &[FileChange],
    cwd: &Path,
) {
    for change in changes {
        let entry = upsert_file_snapshot_entry(snapshot_map, change);
        entry.action = change.action.clone();
        // deleted 文件在执行后不应再读取磁盘，after 显式置空。
        entry.after_content = if change.action == "deleted" {
            None
        } else {
            read_text_file_snapshot(cwd, &entry.path)
        };
    }
}


pub(crate) fn build_changed_file_snapshots(
    changed_files: &[FileChange],
    snapshot_map: &BTreeMap<String, FileChangeSnapshot>,
    cwd: &Path,
) -> Vec<FileChangeSnapshot> {
    changed_files
        .iter()
        .map(|change| {
            let normalized_path = normalize_change_path(&change.path);
            if let Some(snapshot) = snapshot_map.get(&normalized_path) {
                let mut next = snapshot.clone();
                next.action = change.action.clone();
                next.path = normalized_path;
                return next;
            }

            // 兜底：如果某条 changedFiles 没有命令级快照（例如仅由 git merge 补入），
            // 仍给前端一份最小 after 预览，避免 Diff 按钮完全无数据。
            FileChangeSnapshot {
                path: normalized_path.clone(),
                action: change.action.clone(),
                before_content: None,
                after_content: if change.action == "deleted" {
                    None
                } else {
                    read_text_file_snapshot(cwd, &normalized_path)
                },
            }
        })
        .collect()
}


pub(crate) fn read_text_file_snapshot(cwd: &Path, path: &str) -> Option<String> {
    let raw = path.trim();
    if raw.is_empty() {
        return None;
    }

    let resolved = {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            cwd.join(candidate)
        }
    };

    if !resolved.is_file() {
        return None;
    }

    let bytes = std::fs::read(&resolved).ok()?;
    let slice = if bytes.len() > MAX_CHANGED_FILE_SNAPSHOT_BYTES {
        &bytes[..MAX_CHANGED_FILE_SNAPSHOT_BYTES]
    } else {
        &bytes[..]
    };
    // 简单二进制过滤：包含 NUL 字节时视为不可读文本。
    if slice.contains(&0) {
        return None;
    }
    let safe_slice = match std::str::from_utf8(slice) {
        Ok(_) => slice,
        Err(error) if error.error_len().is_none() => &slice[..error.valid_up_to()],
        Err(_) => slice,
    };
    Some(String::from_utf8_lossy(safe_slice).to_string())
}


pub(crate) async fn handle_update_goal(
    thread_store: &ThreadStore,
    app_handle: &AppHandle,
    thread_id: &str,
    arguments: &str,
) -> Result<GoalUpdateOutcome, String> {
    let args: serde_json::Value =
        serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;
    let status_str = args
        .get("status")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'status' parameter".to_string())?;
    let goal_status = match status_str {
        "complete" => ThreadGoalStatus::Complete,
        "blocked" => ThreadGoalStatus::Blocked,
        other => return Err(format!("unknown status: {other}")),
    };
    let goal = thread_store
        .set_thread_goal_status(thread_id, goal_status)
        .await
        .map_err(|e| format!("failed to update goal status: {e}"))?;
    emit_and_broadcast(
        app_handle,
        "thread-goal-updated",
        serde_json::json!({
            "threadId": thread_id,
            "goal": goal.clone(),
        }),
    );
    Ok(GoalUpdateOutcome {
        message: format!("Goal status updated to '{status_str}'."),
        goal,
    })
}


pub(crate) fn file_changes_from_tool_call(call: &ToolCallRequest) -> Vec<FileChange> {
    match call.name.as_str() {
        "write_file" => write_file_change_from_args(&call.arguments)
            .into_iter()
            .collect(),
        "apply_patch" => apply_patch_changes_from_args(&call.arguments),
        // shell 类工具在非 git 工作区或跨目录写盘时，git 快照可能拿不到变更；
        // 这里补一层“命令参数级”识别，尽量恢复 RunSummary changedFiles 的可见性。
        "shell" | "shell_command" | "exec_command" => shell_changes_from_args(&call.arguments),
        _ => Vec::new(),
    }
}


pub(crate) fn write_file_change_from_args(arguments: &str) -> Option<FileChange> {
    let parsed: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let path = parsed.get("path")?.as_str()?.trim();
    if path.is_empty() {
        return None;
    }

    Some(FileChange {
        path: path.replace('\\', "/"),
        action: "modified".to_string(),
    })
}


pub(crate) fn apply_patch_changes_from_args(arguments: &str) -> Vec<FileChange> {
    let Some(patch) = patch_body_from_tool_args(arguments) else {
        return Vec::new();
    };

    let mut changes = Vec::new();
    let mut pending_update: Option<usize> = None;
    for line in patch.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            changes.push(FileChange {
                path: normalize_change_path(path),
                action: "created".to_string(),
            });
            pending_update = None;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            changes.push(FileChange {
                path: normalize_change_path(path),
                action: "modified".to_string(),
            });
            pending_update = Some(changes.len() - 1);
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            changes.push(FileChange {
                path: normalize_change_path(path),
                action: "deleted".to_string(),
            });
            pending_update = None;
        } else if let Some(dest) = line.strip_prefix("*** Move to: ") {
            if let Some(idx) = pending_update {
                changes[idx].path = normalize_change_path(dest);
                changes[idx].action = "renamed".to_string();
            }
        }
    }

    changes
}


pub(crate) fn shell_changes_from_args(arguments: &str) -> Vec<FileChange> {
    let Some(command) = shell_command_from_args(arguments) else {
        return Vec::new();
    };
    let vars = shell_extract_variable_assignments(&command);
    let tokens = shell_command_tokens(&command);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut changes = Vec::new();
    for (idx, token) in tokens.iter().enumerate() {
        let lowered = token.to_ascii_lowercase();
        match lowered.as_str() {
            // 明确写盘命令：默认按 modified 上报。
            "set-content" | "add-content" | "out-file" => {
                if let Some(path) = shell_flag_value_with_vars(
                    &tokens,
                    idx,
                    &["-path", "-literalpath", "-filepath"],
                    &vars,
                ) {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
            // 删除命令：标记 deleted。
            "remove-item" | "del" | "erase" | "rm" => {
                if let Some(path) =
                    shell_flag_value_with_vars(&tokens, idx, &["-path", "-literalpath"], &vars)
                {
                    push_shell_change(&mut changes, &path, "deleted");
                }
            }
            // 移动命令：目标路径视为 renamed。
            "move-item" | "mv" | "move" => {
                if let Some(path) = shell_flag_value_with_vars(
                    &tokens,
                    idx,
                    &["-destination", "-dest", "-path"],
                    &vars,
                ) {
                    push_shell_change(&mut changes, &path, "renamed");
                }
            }
            // 复制命令：目标路径按 modified 处理（新建/覆盖都可归并为可见变更）。
            "copy-item" | "copy" | "cp" => {
                if let Some(path) = shell_flag_value_with_vars(
                    &tokens,
                    idx,
                    &["-destination", "-dest", "-path"],
                    &vars,
                ) {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
            // 处理显式重定向：`>` / `>>` / `1>` / `1>>` / `2>` / `2>>`。
            ">" | ">>" | "1>" | "1>>" | "2>" | "2>>" => {
                if let Some(path) =
                    shell_path_candidate_with_vars(tokens.get(idx + 1).map(String::as_str), &vars)
                {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
            _ => {
                // 处理无空格写法：例如 `>D:\a.txt` 或 `1>>out.log`。
                if let Some(path) = shell_redirection_target(token) {
                    push_shell_change(&mut changes, &path, "modified");
                }
            }
        }
    }

    changes
}


pub(crate) fn shell_command_from_args(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }

    let parsed: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    if let Some(command) = parsed.get("command") {
        if let Some(value) = command.as_str() {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        if let Some(array) = command.as_array() {
            let merged = array
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            if !merged.trim().is_empty() {
                return Some(merged);
            }
        }
    }

    parsed
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}


pub(crate) fn shell_command_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in command.chars() {
        match quote {
            Some(marker) => {
                if ch == marker {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    continue;
                }
                if ch.is_whitespace() {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}


pub(crate) fn shell_redirection_target(token: &str) -> Option<String> {
    const PREFIXES: [&str; 6] = ["1>>", "1>", "2>>", "2>", ">>", ">"];
    for prefix in PREFIXES {
        if let Some(rest) = token.strip_prefix(prefix) {
            return shell_path_candidate(Some(rest));
        }
    }
    None
}


pub(crate) fn shell_path_candidate(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    // 过滤变量/参数位占位，避免把 `$path`、`-Force` 这类值当成文件。
    if raw.starts_with('$') || raw.starts_with('-') || raw.starts_with('&') {
        return None;
    }

    let trimmed = raw
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim_end_matches(|c: char| matches!(c, ';' | ',' | ')' | '('))
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.replace('\\', "/"))
}


/// 与 `shell_path_candidate` 相同逻辑，但当遇到 `$var` 时尝试从变量表中解析。
pub(crate) fn shell_path_candidate_with_vars(raw: Option<&str>, vars: &[(String, String)]) -> Option<String> {
    if let Some(result) = shell_path_candidate(raw) {
        return Some(result);
    }
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    // 尝试变量解析：`$varName` → 查找变量表
    if let Some(var_name) = raw.strip_prefix('$') {
        let var_name_lower = var_name.to_ascii_lowercase();
        for (name, value) in vars {
            if name.to_ascii_lowercase() == var_name_lower {
                return shell_path_candidate(Some(value.as_str()));
            }
        }
    }
    None
}


/// 与 `shell_flag_value` 相同，但使用 `shell_path_candidate_with_vars` 做路径解析。
pub(crate) fn shell_flag_value_with_vars(
    tokens: &[String],
    start_idx: usize,
    flags: &[&str],
    vars: &[(String, String)],
) -> Option<String> {
    let mut idx = start_idx + 1;
    while idx < tokens.len() {
        let lowered = tokens[idx].to_ascii_lowercase();
        if lowered == "|" || lowered == ";" {
            break;
        }

        if flags.iter().any(|flag| *flag == lowered.as_str()) {
            return shell_path_candidate_with_vars(tokens.get(idx + 1).map(String::as_str), vars);
        }

        if let Some((flag, value)) = tokens[idx].split_once('=') {
            let lowered_flag = flag.to_ascii_lowercase();
            if flags.iter().any(|item| *item == lowered_flag.as_str()) {
                return shell_path_candidate_with_vars(Some(value), vars);
            }
        }
        idx += 1;
    }
    None
}


pub(crate) fn push_shell_change(changes: &mut Vec<FileChange>, path: &str, action: &str) {
    if path.trim().is_empty() {
        return;
    }

    if let Some(existing) = changes
        .iter_mut()
        .find(|item| paths_match(&item.path, path))
    {
        existing.action = action.to_string();
        return;
    }

    changes.push(FileChange {
        path: path.to_string(),
        action: action.to_string(),
    });
}


pub(crate) fn patch_body_from_tool_args(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.starts_with("*** Begin Patch") {
        return Some(trimmed.to_string());
    }

    let parsed: serde_json::Value = serde_json::from_str(arguments).ok()?;
    parsed
        .get("patch")
        .or_else(|| parsed.get("command"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}


pub(crate) fn apply_patch_fingerprint(arguments: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    patch_body_from_tool_args(arguments)
        .unwrap_or_else(|| arguments.trim().to_string())
        .hash(&mut hasher);
    hasher.finish()
}


pub(crate) fn apply_patch_failure_requires_refresh(result: &str) -> bool {
    result.contains("failed to match hunk")
}


pub(crate) fn repeated_failed_patch_limit_reached(
    duplicate_count: &mut u32,
    duplicate_failed_patch: bool,
    max_duplicates: u32,
) -> bool {
    if !duplicate_failed_patch {
        *duplicate_count = 0;
        return false;
    }

    *duplicate_count = duplicate_count.saturating_add(1);
    *duplicate_count >= max_duplicates.max(1)
}


pub(crate) fn tool_result_success(tool_name: &str, output: &str) -> bool {
    match tool_name {
        "apply_patch" => output.starts_with("Success. Applied patch."),
        "write_file" => output.starts_with("Successfully wrote "),
        _ => true,
    }
}


pub(crate) fn truncate_log_message(message: &str) -> String {
    const MAX_CHARS: usize = 500;
    let mut chars = message.chars();
    let prefix: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{prefix}... [truncated]")
    } else {
        prefix
    }
}


pub(crate) fn final_text_with_failed_file_edit_status(text: &str) -> String {
    const STATUS: &str = "File modification status: failed. No apply_patch/write_file call wrote a file successfully in this turn.";
    if text.trim().is_empty() {
        STATUS.to_string()
    } else {
        format!("{STATUS}\n\n{text}")
    }
}


pub(crate) fn should_block_write_file_after_patch_failure(
    tool_name: &str,
    apply_patch_failed_in_turn: bool,
    changes: &[FileChange],
    cwd: &Path,
) -> bool {
    tool_name == "write_file"
        && apply_patch_failed_in_turn
        && changes.iter().any(|change| {
            let path = PathBuf::from(&change.path);
            let resolved = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            resolved.is_file()
        })
}


pub(crate) fn patch_paths_requiring_refresh_for(
    changes: &[FileChange],
    paths_requiring_refresh: &HashSet<String>,
) -> Vec<String> {
    let mut stale_paths: Vec<String> = Vec::new();
    for change in changes {
        let path = normalize_change_path(&change.path);
        if paths_requiring_refresh
            .iter()
            .any(|changed_path| paths_match(changed_path, &path))
            && !stale_paths.iter().any(|known| paths_match(known, &path))
        {
            stale_paths.push(path);
        }
    }
    stale_paths
}


pub(crate) fn read_file_path_from_tool_args(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("path")?
        .as_str()
        .map(normalize_change_path)
        .filter(|path| !path.is_empty())
}


pub(crate) fn paths_match(a: &str, b: &str) -> bool {
    let na = normalize_change_path(a);
    let nb = normalize_change_path(b);
    if na == nb {
        return true;
    }
    let sa = na.trim_start_matches('/');
    let sb = nb.trim_start_matches('/');
    sa.ends_with(sb) || sb.ends_with(sa)
}


pub(crate) fn build_goal_continuation_prompt(goal: &ThreadGoal) -> String {
    let budget_info = if let Some(budget) = goal.token_budget {
        let remaining = budget.saturating_sub(goal.tokens_used);
        format!(
            "Tokens used: {}, budget: {}, remaining: {}.",
            goal.tokens_used, budget, remaining
        )
    } else {
        format!("Tokens used: {}.", goal.tokens_used)
    };
    format!(
        "Continue working toward the active thread goal.\n\n\
         <objective>\n{}\n</objective>\n\n\
         {budget_info}\n\n\
         Keep working through the available tools until the objective is genuinely handled.\n\
         Do not repeat the previous final answer. If it described a code or file change that has not \
         been made by a successful apply_patch/write_file call, call the editing tool now.\n\
         If the objective is achieved and no required work remains, call update_goal with \
         status \"complete\".\n\
         If the same blocking condition has repeated for at least three consecutive goal turns \
         and you cannot make progress, call update_goal with status \"blocked\".\n\
         Do not call update_goal unless the goal is truly complete or the strict blocked \
         threshold above is satisfied.",
        goal.objective,
    )
}


pub(crate) fn is_internal_runtime_file(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let p = normalized.trim_start_matches('/');
    const PATTERNS: &[&str] = &[
        "codey/usage",
        "codey/sessions/",
        "codey/config.toml",
        "codey/memories/",
        ".cn-codex/robot-workflows.json",
    ];
    PATTERNS.iter().any(|pat| p.contains(pat))
}


pub(crate) fn push_file_change(changes: &mut Vec<FileChange>, mut change: FileChange) {
    change.path = normalize_change_path(&change.path);
    if change.path.trim().is_empty() || is_internal_runtime_file(&change.path) {
        return;
    }

    if let Some(existing) = changes
        .iter_mut()
        .find(|item| paths_match(&item.path, &change.path))
    {
        existing.action = change.action;
        return;
    }

    changes.push(change);
}


