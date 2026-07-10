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
