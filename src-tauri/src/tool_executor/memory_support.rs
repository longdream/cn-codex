use super::*;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::{condense_whitespace, truncate_output};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryOutputFormat {
    Text,
    Json,
}

impl MemoryOutputFormat {
    pub(crate) fn from_arg(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("json") => Self::Json,
            _ => Self::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MemoryListEntry {
    pub(crate) name: String,
    pub(crate) path: String,
    #[serde(rename = "isDirectory")]
    pub(crate) is_dir: bool,
}

impl Ord for MemoryListEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.path.cmp(&other.path)
    }
}

impl PartialOrd for MemoryListEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MemorySearchMatch {
    pub(crate) path: String,
    pub(crate) line_number: usize,
    pub(crate) line: String,
    pub(crate) before: Vec<String>,
    pub(crate) after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemorySearchResult {
    pub(crate) matches: Vec<MemorySearchMatch>,
    pub(crate) total_matches: usize,
    pub(crate) next_cursor: Option<String>,
}

pub(crate) fn read_okf_body_lines(path: &Path) -> Option<Vec<String>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let body = crate::smartbrain::okf::extract_body(&raw);
    Some(body.lines().map(|line| line.to_string()).collect())
}

pub(crate) fn resolve_memory_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim().replace('\\', "/");
    if trimmed.contains(':') {
        return Err(
            "Error: memory paths must be relative and must not contain a drive prefix".to_string(),
        );
    }

    let mut path = root.to_path_buf();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(path);
    }

    let raw = Path::new(&trimmed);
    if raw.is_absolute() {
        return Err("Error: memory paths must be relative".to_string());
    }

    for component in raw.components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("Error: memory paths must not contain '..'".to_string());
            }
            _ => {
                return Err("Error: invalid memory path component".to_string());
            }
        }
    }

    Ok(path)
}

pub(crate) fn parse_memory_cursor(cursor: Option<&str>) -> Result<usize, String> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    cursor
        .parse::<usize>()
        .map_err(|_| "Error: invalid memory cursor".to_string())
}

pub(crate) fn format_memory_list_output(
    path: &str,
    entries: Vec<MemoryListEntry>,
    total: usize,
    next_cursor: Option<String>,
    format: MemoryOutputFormat,
) -> String {
    match format {
        MemoryOutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "path": path,
            "total": total,
            "nextCursor": next_cursor,
            "entries": entries,
        }))
        .unwrap_or_default(),
        MemoryOutputFormat::Text => {
            if total == 0 {
                return format!("No memories found under {path}");
            }
            let mut output = format!("Memory entries under {path} ({}/{total}):\n", entries.len());
            for entry in entries {
                output.push_str("- ");
                output.push_str(&entry.path);
                if entry.is_dir {
                    output.push('/');
                }
                output.push('\n');
            }
            if let Some(cursor) = next_cursor {
                output.push_str(&format!("Next cursor: {cursor}\n"));
            }
            output
        }
    }
}

pub(crate) fn format_memory_search_output(
    query: &str,
    result: &MemorySearchResult,
    format: MemoryOutputFormat,
) -> String {
    match format {
        MemoryOutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "query": query,
            "totalMatches": result.total_matches,
            "nextCursor": result.next_cursor,
            "matches": result.matches,
        }))
        .unwrap_or_default(),
        MemoryOutputFormat::Text => {
            if result.matches.is_empty() {
                return format!("No memory matches found for: {query}");
            }
            let mut output = format!(
                "Memory search results for \"{query}\" ({}/{}):\n",
                result.matches.len(),
                result.total_matches
            );
            for item in &result.matches {
                output.push_str(&format!(
                    "\n{}:{}: {}",
                    item.path, item.line_number, item.line
                ));
                if !item.before.is_empty() {
                    output.push_str("\n  before:");
                    for line in &item.before {
                        output.push_str("\n    ");
                        output.push_str(line);
                    }
                }
                if !item.after.is_empty() {
                    output.push_str("\n  after:");
                    for line in &item.after {
                        output.push_str("\n    ");
                        output.push_str(line);
                    }
                }
            }
            if let Some(cursor) = &result.next_cursor {
                output.push_str(&format!("\n\nNext cursor: {cursor}"));
            }
            output
        }
    }
}

pub(crate) fn search_memory_files(
    memories_root: &Path,
    search_root: &Path,
    query: &str,
    case_sensitive: bool,
    context_lines: usize,
    cursor: usize,
    max_results: usize,
) -> Result<MemorySearchResult, String> {
    if !search_root.exists() {
        return Ok(MemorySearchResult {
            matches: Vec::new(),
            total_matches: 0,
            next_cursor: None,
        });
    }

    let mut files = Vec::new();
    collect_memory_files(search_root, &mut files)
        .map_err(|e| format!("Error reading memories: {e}"))?;
    files.sort();

    let query_cmp = if case_sensitive {
        query.to_string()
    } else {
        query.to_ascii_lowercase()
    };
    let mut all_matches = Vec::new();

    for file in files {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        let rel = relative_display_path(memories_root, &file);
        let lines = content.lines().collect::<Vec<_>>();
        for (idx, line) in lines.iter().enumerate() {
            let line_cmp = if case_sensitive {
                (*line).to_string()
            } else {
                line.to_ascii_lowercase()
            };
            if line_cmp.contains(&query_cmp) {
                let before_start = idx.saturating_sub(context_lines);
                let before = lines[before_start..idx]
                    .iter()
                    .map(|line| truncate_output(&condense_whitespace(line), 400))
                    .collect::<Vec<_>>();
                let after_end = (idx + 1 + context_lines).min(lines.len());
                let after = lines[idx + 1..after_end]
                    .iter()
                    .map(|line| truncate_output(&condense_whitespace(line), 400))
                    .collect::<Vec<_>>();
                all_matches.push(MemorySearchMatch {
                    path: rel.clone(),
                    line_number: idx + 1,
                    line: truncate_output(&condense_whitespace(line), 400),
                    before,
                    after,
                });
            }
        }
    }

    let total_matches = all_matches.len();
    let matches = all_matches
        .into_iter()
        .skip(cursor)
        .take(max_results)
        .collect::<Vec<_>>();
    let next_cursor = if cursor + matches.len() < total_matches {
        Some((cursor + matches.len()).to_string())
    } else {
        None
    };

    Ok(MemorySearchResult {
        matches,
        total_matches,
        next_cursor,
    })
}

fn collect_memory_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_file() {
        if is_text_memory_file(path) {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    if !path.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_memory_files(&path, out)?;
        } else if is_text_memory_file(&path) {
            out.push(path);
        }
    }

    Ok(())
}

fn is_text_memory_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "txt" | "json" | "toml" | "yaml" | "yml"
            )
        })
        .unwrap_or(false)
}

fn relative_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

impl ToolExecutor {
    pub(crate) async fn exec_memory_list(
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
            #[serde(default)]
            max_entries: Option<usize>,
            #[serde(default)]
            cursor: Option<String>,
            #[serde(default)]
            format: Option<String>,
        }

        let args: ListArgs = serde_json::from_str(arguments).unwrap_or_default();
        let display_path = args.path.as_deref().unwrap_or(".");
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_list", display_path);

        let dir = match self.resolve_memory_path(args.path.as_deref().unwrap_or("")) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
                return Ok(msg);
            }
        };

        if let Err(e) = tokio::fs::create_dir_all(self.memories_dir()).await {
            let msg = format!("Error creating memories directory: {e}");
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
            return Ok(msg);
        }

        let max_entries = args.max_entries.unwrap_or(100).clamp(1, 200);
        let cursor = match parse_memory_cursor(args.cursor.as_deref()) {
            Ok(cursor) => cursor,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
                return Ok(msg);
            }
        };
        let format = MemoryOutputFormat::from_arg(args.format.as_deref());

        let mut entries = Vec::new();
        match tokio::fs::read_dir(&dir).await {
            Ok(mut reader) => {
                while let Ok(Some(entry)) = reader.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    let path = if display_path == "." || display_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", display_path.trim_end_matches('/'), name)
                    };
                    entries.push(MemoryListEntry { name, path, is_dir });
                }
                entries.sort();
                let total = entries.len();
                let page = entries
                    .into_iter()
                    .skip(cursor)
                    .take(max_entries)
                    .collect::<Vec<_>>();
                let next_cursor = if cursor + page.len() < total {
                    Some((cursor + page.len()).to_string())
                } else {
                    None
                };
                let output =
                    format_memory_list_output(display_path, page, total, next_cursor, format);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", 0, &output);
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Error listing memory path {display_path}: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_list", -1, &msg);
                Ok(msg)
            }
        }
    }


    pub(crate) async fn exec_memory_read(
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
            format: Option<String>,
        }

        let args: ReadArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_read args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_read", &args.path);

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_read", -1, &msg);
                return Ok(msg);
            }
        };

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => {
                let msg = format!("Error reading memory {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_read", -1, &msg);
                return Ok(msg);
            }
        };

        let line_offset = args.line_offset.unwrap_or(1).max(1);
        let max_lines = args.max_lines.unwrap_or(200).clamp(1, 500);
        let lines = content.lines().collect::<Vec<_>>();
        let selected_lines = lines
            .iter()
            .skip(line_offset.saturating_sub(1))
            .take(max_lines)
            .copied()
            .collect::<Vec<_>>();
        let selected = selected_lines.join("\n");
        let next_line_offset = if line_offset.saturating_sub(1) + selected_lines.len() < lines.len()
        {
            Some(line_offset + selected_lines.len())
        } else {
            None
        };
        let format = MemoryOutputFormat::from_arg(args.format.as_deref());
        let output_body = match format {
            MemoryOutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
                "path": args.path,
                "lineOffset": line_offset,
                "maxLines": max_lines,
                "totalLines": lines.len(),
                "nextLineOffset": next_line_offset,
                "content": selected,
            }))
            .unwrap_or_default(),
            MemoryOutputFormat::Text => {
                let mut text = format!(
                    "Memory: {}\nLines: {}-{}\n",
                    args.path,
                    line_offset,
                    line_offset + selected.lines().count().saturating_sub(1)
                );
                if let Some(next) = next_line_offset {
                    text.push_str(&format!("Next line_offset: {next}\n"));
                }
                text.push('\n');
                text.push_str(&selected);
                text
            }
        };
        let output = truncate_output(&output_body, TOOL_OUTPUT_MEMORY_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "memory_read", 0, &output);

        self.track_experience_usage(&args.path);

        Ok(output)
    }


    pub(crate) async fn exec_memory_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct SearchArgs {
            query: String,
            #[serde(default)]
            path: Option<String>,
            #[serde(default)]
            case_sensitive: Option<bool>,
            #[serde(default)]
            max_results: Option<usize>,
            #[serde(default)]
            context_lines: Option<usize>,
            #[serde(default)]
            cursor: Option<String>,
            #[serde(default)]
            format: Option<String>,
        }

        let args: SearchArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_search args: {e}"))
        })?;
        let query = args.query.trim();
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_search", query);

        if query.is_empty() {
            let msg = "Error: empty memory search query".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
            return Ok(msg);
        }

        let root = match self.resolve_memory_path(args.path.as_deref().unwrap_or("")) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
                return Ok(msg);
            }
        };

        let max_results = args.max_results.unwrap_or(20).clamp(1, 50);
        let context_lines = args.context_lines.unwrap_or(0).clamp(0, 5);
        let cursor = match parse_memory_cursor(args.cursor.as_deref()) {
            Ok(cursor) => cursor,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
                return Ok(msg);
            }
        };
        let case_sensitive = args.case_sensitive.unwrap_or(false);
        let result = search_memory_files(
            &self.memories_dir(),
            &root,
            query,
            case_sensitive,
            context_lines,
            cursor,
            max_results,
        );

        let output = match result {
            Ok(result) => format_memory_search_output(
                query,
                &result,
                MemoryOutputFormat::from_arg(args.format.as_deref()),
            ),
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", -1, &msg);
                return Ok(msg);
            }
        };

        let output = truncate_output(&output, TOOL_OUTPUT_MEMORY_MAX_CHARS);
        self.emit_tool_end(app_handle, thread_id, call_id, "memory_search", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_memory_write(
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
            append: Option<bool>,
        }

        let args: WriteArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_write args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_write", &args.path);

        if args.content.trim().is_empty() {
            let msg = "Error: empty memory content".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
            return Ok(msg);
        }

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
                return Ok(msg);
            }
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                let msg = format!("Error creating memory parent directory: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
                return Ok(msg);
            }
        }

        let append = args.append.unwrap_or(false);
        let result = if append {
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(args.content.as_bytes()).await {
                        Err(e)
                    } else if !args.content.ends_with('\n') {
                        file.write_all(b"\n").await
                    } else {
                        Ok(())
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            tokio::fs::write(&path, &args.content).await
        };

        match result {
            Ok(()) => {
                let msg = format!(
                    "{} memory {} ({} bytes)",
                    if append { "Appended" } else { "Wrote" },
                    args.path,
                    args.content.len()
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", 0, &msg);
                Ok(msg)
            }
            Err(e) => {
                let msg = format!("Error writing memory {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_write", -1, &msg);
                Ok(msg)
            }
        }
    }


    pub(crate) async fn exec_memory_update(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct UpdateArgs {
            path: String,
            old_text: String,
            new_text: String,
            #[serde(default)]
            replace_all: Option<bool>,
        }

        let args: UpdateArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_update args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_update", &args.path);

        if args.old_text.is_empty() {
            let msg = "Error: memory_update old_text must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
            return Ok(msg);
        }

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
                return Ok(msg);
            }
        };

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => {
                let msg = format!("Error reading memory {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
                return Ok(msg);
            }
        };

        if !content.contains(&args.old_text) {
            let msg = format!("No exact memory text match found in {}", args.path);
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
            return Ok(msg);
        }

        let replace_all = args.replace_all.unwrap_or(false);
        let count = content.matches(&args.old_text).count();
        let updated = if replace_all {
            content.replace(&args.old_text, &args.new_text)
        } else {
            content.replacen(&args.old_text, &args.new_text, 1)
        };
        let replaced = if replace_all { count } else { 1 };

        if let Err(e) = tokio::fs::write(&path, updated).await {
            let msg = format!("Error updating memory {}: {e}", args.path);
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", -1, &msg);
            return Ok(msg);
        }

        let msg = format!("Updated memory {} ({} replacement(s))", args.path, replaced);
        self.emit_tool_end(app_handle, thread_id, call_id, "memory_update", 0, &msg);
        Ok(msg)
    }


    pub(crate) async fn exec_memory_forget(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ForgetArgs {
            path: String,
            #[serde(default)]
            match_text: Option<String>,
            #[serde(default)]
            recursive: Option<bool>,
        }

        let args: ForgetArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid memory_forget args: {e}"))
        })?;
        self.emit_tool_start(app_handle, thread_id, call_id, "memory_forget", &args.path);

        let path = match self.resolve_memory_path(&args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
        };

        if let Some(match_text) = args
            .match_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(content) => content,
                Err(e) => {
                    let msg = format!("Error reading memory {}: {e}", args.path);
                    self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                    return Ok(msg);
                }
            };
            let mut removed = 0usize;
            let kept = content
                .lines()
                .filter(|line| {
                    let keep = !line.contains(match_text);
                    if !keep {
                        removed += 1;
                    }
                    keep
                })
                .collect::<Vec<_>>();
            if removed == 0 {
                let msg = format!("No matching memory lines found in {}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
            let mut updated = kept.join("\n");
            if !updated.is_empty() {
                updated.push('\n');
            }
            if let Err(e) = tokio::fs::write(&path, updated).await {
                let msg = format!("Error forgetting memory lines in {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
            let msg = format!(
                "Forgot {} matching line(s) from memory {}",
                removed, args.path
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", 0, &msg);
            return Ok(msg);
        }

        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(e) => {
                let msg = format!("Error reading memory path {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                return Ok(msg);
            }
        };

        let result = if metadata.is_dir() {
            if args.recursive.unwrap_or(false) {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_dir(&path).await
            }
        } else {
            tokio::fs::remove_file(&path).await
        };

        match result {
            Ok(()) => {
                let msg = format!("Forgot memory path {}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", 0, &msg);
                Ok(msg)
            }
            Err(e) => {
                let msg = format!("Error forgetting memory path {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "memory_forget", -1, &msg);
                Ok(msg)
            }
        }
    }

}
