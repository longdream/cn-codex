use super::*;

pub(crate) fn validate_existing_file_rewrite(existing: &str, replacement: &str) -> Result<(), String> {
    let normalized = replacement.to_lowercase();
    let contains_ellipsis = normalized.contains("...") || normalized.contains('…');
    let contains_omission_marker = [
        "保留原有内容",
        "省略原有内容",
        "其余内容不变",
        "剩余内容不变",
        "content omitted",
        "keep existing content",
        "existing content unchanged",
        "rest of the file",
        "rest unchanged",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));

    if contains_ellipsis && contains_omission_marker {
        return Err(
            "replacement contains an omission placeholder and is not complete file content. Use apply_patch instead."
                .to_string(),
        );
    }

    const LARGE_EXISTING_FILE_BYTES: usize = 4 * 1024;
    const MAX_DESTRUCTIVE_SHRINK_FACTOR: usize = 4;
    if existing.len() >= LARGE_EXISTING_FILE_BYTES
        && replacement
            .len()
            .saturating_mul(MAX_DESTRUCTIVE_SHRINK_FACTOR)
            < existing.len()
    {
        return Err(format!(
            "replacement would shrink an existing {}-byte file to {} bytes. Use apply_patch so omitted content cannot be lost.",
            existing.len(),
            replacement.len()
        ));
    }

    Ok(())
}

/// Format a workspace file read, optionally as a numbered line window.
///
/// Design goals:
/// - replace shell snippets that dump `start..=end` line ranges
/// - always page large files with a stable default window
/// - return pagination metadata so the model can continue with `line_offset`
pub(crate) fn format_read_file_output(
    path: &str,
    content: &str,
    line_offset: Option<usize>,
    max_lines: Option<usize>,
    end_line: Option<usize>,
    show_line_numbers: Option<bool>,
) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let total_lines = lines.len();
    if total_lines == 0 {
        return format!("File: {path}\nLines: 0-0 / 0\n\n");
    }

    let start = line_offset.unwrap_or(1).max(1);
    if start > total_lines {
        return format!(
            "File: {path}\nError: line_offset {start} is beyond end of file ({total_lines} lines)."
        );
    }

    let requested_count = if let Some(end) = end_line {
        if end < start {
            return format!("File: {path}\nError: end_line {end} must be >= line_offset {start}.");
        }
        end.saturating_sub(start).saturating_add(1)
    } else {
        max_lines.unwrap_or(READ_FILE_DEFAULT_MAX_LINES)
    };
    let count = requested_count.clamp(1, READ_FILE_MAX_LINES_HARD_CAP);
    let selected = lines
        .iter()
        .skip(start.saturating_sub(1))
        .take(count)
        .copied()
        .collect::<Vec<_>>();
    let end = start.saturating_add(selected.len().saturating_sub(1));
    let has_more = end < total_lines;
    // Default to numbered pages so models can resume with exact line_offset values.
    let with_numbers = show_line_numbers.unwrap_or(true);

    if with_numbers {
        render_numbered_file_slice(path, &selected, start, total_lines, has_more)
    } else {
        let mut text = format!("File: {path}\nLines: {start}-{end} / {total_lines}\n");
        if has_more {
            text.push_str(&format!("Next line_offset: {}\n", end + 1));
        }
        text.push('\n');
        text.push_str(&selected.join("\n"));
        text
    }
}


pub(crate) fn render_numbered_file_slice(
    path: &str,
    selected_lines: &[&str],
    start_line: usize,
    total_lines: usize,
    has_more: bool,
) -> String {
    if selected_lines.is_empty() {
        return format!("File: {path}\nLines: 0-0 / {total_lines}\n\n");
    }

    let end_line = start_line.saturating_add(selected_lines.len().saturating_sub(1));
    let width = ((start_line.max(end_line)).max(1).ilog10() as usize) + 1;
    let mut text = format!("File: {path}\nLines: {start_line}-{end_line} / {total_lines}\n");
    if has_more {
        text.push_str(&format!("Next line_offset: {}\n", end_line + 1));
    }
    text.push('\n');
    for (idx, line) in selected_lines.iter().enumerate() {
        let number = start_line + idx;
        text.push_str(&format!("{number:>width$}|{line}\n"));
    }
    // Keep a trailing newline only when there is content; trim the final extra newline.
    text.pop();
    text
}

impl ToolExecutor {
    pub(crate) async fn exec_read_file(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ReadArgs {
            path: String,
            #[serde(default)]
            line_offset: Option<usize>,
            #[serde(default)]
            max_lines: Option<usize>,
            #[serde(default)]
            end_line: Option<usize>,
            #[serde(default)]
            show_line_numbers: Option<bool>,
        }

        let args: ReadArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid read_file args: {e}")))?;

        let full_path = self.cwd.join(&args.path);
        info!("Reading file: {}", full_path.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "read_file", &args.path);

        let result = match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                let formatted = format_read_file_output(
                    &args.path,
                    &content,
                    args.line_offset,
                    args.max_lines,
                    args.end_line,
                    args.show_line_numbers,
                );
                let truncated = truncate_output(&formatted, TOOL_OUTPUT_READ_FILE_MAX_CHARS);
                self.emit_tool_end(app_handle, thread_id, call_id, "read_file", 0, &truncated);
                truncated
            }
            Err(e) => {
                let msg = format!("Error reading {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "read_file", -1, &msg);
                msg
            }
        };
        Ok(result)
    }


    pub(crate) async fn exec_write_file(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct WriteArgs {
            path: String,
            content: String,
            #[serde(default)]
            overwrite: bool,
        }

        let args: WriteArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid write_file args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "write_file",
                    "invalid arguments",
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                return Err(crate::error::AppError::Custom(msg));
            }
        };

        let full_path = self.cwd.join(&args.path);
        info!("Writing file: {}", full_path.display());

        self.emit_tool_start(app_handle, thread_id, call_id, "write_file", &args.path);

        if full_path.is_file() {
            if !args.overwrite {
                let msg = format!(
                    "Error writing {}: the file already exists. Use apply_patch for edits, or set overwrite=true only when the user explicitly requested a complete rewrite.",
                    args.path
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                return Err(crate::error::AppError::Custom(msg));
            }

            let existing = match tokio::fs::read_to_string(&full_path).await {
                Ok(existing) => existing,
                Err(e) => {
                    let msg = format!(
                        "Error writing {}: failed to inspect existing UTF-8 file before overwrite: {e}",
                        args.path
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                    return Err(crate::error::AppError::Custom(msg));
                }
            };
            if let Err(reason) = validate_existing_file_rewrite(&existing, &args.content) {
                let msg = format!("Error writing {}: {reason}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                return Err(crate::error::AppError::Custom(msg));
            }
        }

        if let Some(parent) = full_path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            let msg = format!(
                "Error writing {}: failed to create parent directory: {e}",
                args.path
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
            return Err(crate::error::AppError::Custom(msg));
        }

        match tokio::fs::write(&full_path, &args.content).await {
            Ok(()) => {
                let msg = format!(
                    "Successfully wrote {} bytes to {}",
                    args.content.len(),
                    args.path
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", 0, &msg);
                Ok(msg)
            }
            Err(e) => {
                let msg = format!("Error writing {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_file", -1, &msg);
                Err(crate::error::AppError::Custom(msg))
            }
        }
    }


    pub(crate) async fn exec_list_dir(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize, Default)]
        struct ListArgs {
            #[serde(default)]
            path: Option<String>,
        }

        let args: ListArgs = serde_json::from_str(arguments).unwrap_or_default();
        let dir = match args.path {
            Some(ref p) if !p.is_empty() => self.cwd.join(p),
            _ => self.cwd.clone(),
        };

        let display_path = args.path.as_deref().unwrap_or(".");
        info!("Listing directory: {}", dir.display());

        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "list_directory",
            display_path,
        );

        let mut entries = Vec::new();
        match tokio::fs::read_dir(&dir).await {
            Ok(mut reader) => {
                while let Ok(Some(entry)) = reader.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    entries.push(if is_dir { format!("{name}/") } else { name });
                }
                entries.sort();
                let output = entries.join("\n");
                self.emit_tool_end(app_handle, thread_id, call_id, "list_directory", 0, &output);
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Error listing {}: {e}", dir.display());
                self.emit_tool_end(app_handle, thread_id, call_id, "list_directory", -1, &msg);
                Ok(msg)
            }
        }
    }

}
