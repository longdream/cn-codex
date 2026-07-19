use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::thread_store::FileChange;

/// 编辑后的单文件内容上限（2MB）。
///
/// 说明：
/// - 审阅面板允许用户直接在前端编辑候选内容；
/// - 这里增加上限可防止误粘贴超大内容导致内存抖动或 IPC 卡顿。
const MAX_REVIEW_EDIT_BYTES: usize = 2 * 1024 * 1024;

/// 组合 `thread_id + call_id` 生成唯一 key，避免跨线程冲突。
pub fn pending_review_key(thread_id: &str, call_id: &str) -> String {
    format!("{thread_id}:{call_id}")
}

/// `apply_patch` 进入“写盘前审阅”后的完整会话快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPatchReview {
    pub thread_id: String,
    pub call_id: String,
    pub raw_patch: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub files: Vec<PendingPatchReviewFile>,
}

/// 单文件审阅条目。
///
/// 字段语义：
/// - `base_content`: 审阅创建时的磁盘基线，用于 apply 阶段冲突检测；
/// - `candidate_content`: 补丁推导出的目标内容（默认建议值）；
/// - `edited_content`: 用户在审阅面板编辑后的内容（可为空表示未编辑）；
/// - `keep`: 默认 true，表示该文件默认会被应用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPatchReviewFile {
    pub path: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_content: Option<String>,
    #[serde(default)]
    pub keep: bool,
}

/// 审阅应用请求：支持 keepAll 与 keepPaths 两种入口。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewApplyRequest {
    #[serde(default)]
    pub keep_all: bool,
    #[serde(default)]
    pub keep_paths: Vec<String>,
}

/// 审阅应用结果，供前端更新 UI 与状态提示。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewApplyResult {
    pub message: String,
    pub changed_files: Vec<FileChange>,
}

pub fn build_pending_patch_review(
    root: &Path,
    thread_id: &str,
    call_id: &str,
    patch: &str,
) -> Result<PendingPatchReview, String> {
    let actions = parse_patch_actions(patch)?;
    if actions.is_empty() {
        return Err("patch contains no file changes".to_string());
    }

    let mut files = Vec::with_capacity(actions.len());
    for action in actions {
        files.push(build_review_file(root, action)?);
    }

    let now = now_millis();
    Ok(PendingPatchReview {
        thread_id: thread_id.to_string(),
        call_id: call_id.to_string(),
        raw_patch: patch.to_string(),
        created_at_ms: now,
        updated_at_ms: now,
        files,
    })
}

pub fn update_review_file(
    review: &mut PendingPatchReview,
    path: &str,
    keep: Option<bool>,
    edited_content: Option<String>,
) -> Result<(), String> {
    if keep.is_none() && edited_content.is_none() {
        return Err("update request is empty: expected keep or editedContent".to_string());
    }

    let target_path = normalize_patch_display_path(path);
    let Some(file) = review
        .files
        .iter_mut()
        .find(|item| item.path == target_path)
    else {
        return Err(format!("review file not found: {path}"));
    };

    if let Some(keep) = keep {
        file.keep = keep;
    }

    if let Some(content) = edited_content {
        if file.action == "deleted" {
            return Err(format!(
                "cannot edit deleted file content in review: {}",
                file.path
            ));
        }
        if content.len() > MAX_REVIEW_EDIT_BYTES {
            return Err(format!(
                "edited content for {} exceeds limit ({} > {})",
                file.path,
                content.len(),
                MAX_REVIEW_EDIT_BYTES
            ));
        }
        if file.candidate_content.as_deref() == Some(content.as_str()) {
            file.edited_content = None;
        } else {
            file.edited_content = Some(content);
        }
    }

    review.updated_at_ms = now_millis();
    Ok(())
}

pub fn apply_review_to_workspace(
    root: &Path,
    review: &PendingPatchReview,
    request: &ReviewApplyRequest,
) -> Result<ReviewApplyResult, String> {
    let selected_paths = resolve_selected_paths(review, request)?;
    if selected_paths.is_empty() {
        return Err("no files selected. choose at least one file to Keep".to_string());
    }

    let mut changed_files = Vec::new();
    for file in &review.files {
        if !selected_paths.contains(&file.path) {
            continue;
        }
        apply_review_file(root, file, &mut changed_files)?;
    }

    let message = format!(
        "Applied {} reviewed file(s) from call {}.",
        changed_files.len(),
        review.call_id
    );
    Ok(ReviewApplyResult {
        message,
        changed_files,
    })
}

fn resolve_selected_paths(
    review: &PendingPatchReview,
    request: &ReviewApplyRequest,
) -> Result<HashSet<String>, String> {
    if request.keep_all {
        return Ok(review.files.iter().map(|file| file.path.clone()).collect());
    }

    if !request.keep_paths.is_empty() {
        let available: HashSet<String> =
            review.files.iter().map(|file| file.path.clone()).collect();
        let mut selected = HashSet::new();
        for path in &request.keep_paths {
            let normalized = normalize_patch_display_path(path);
            if !available.contains(&normalized) {
                return Err(format!(
                    "keep path does not exist in review session: {}",
                    path
                ));
            }
            selected.insert(normalized);
        }
        return Ok(selected);
    }

    Ok(review
        .files
        .iter()
        .filter(|file| file.keep)
        .map(|file| file.path.clone())
        .collect())
}

fn apply_review_file(
    root: &Path,
    file: &PendingPatchReviewFile,
    changed_files: &mut Vec<FileChange>,
) -> Result<(), String> {
    match file.action.as_str() {
        "created" => {
            let target = resolve_patch_path(root, &file.path)?;
            if target.exists() {
                return Err(format!(
                    "conflict: cannot create {}, file already exists",
                    file.path
                ));
            }
            let Some(content) = effective_content(file) else {
                return Err(format!("missing candidate content for {}", file.path));
            };
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent for {}: {e}", file.path))?;
            }
            std::fs::write(&target, content)
                .map_err(|e| format!("failed to write {}: {e}", file.path))?;
            changed_files.push(FileChange {
                path: file.path.clone(),
                action: "created".to_string(),
            });
            Ok(())
        }
        "modified" => {
            let source = resolve_patch_path(root, &file.path)?;
            if !source.is_file() {
                return Err(format!(
                    "conflict: cannot modify {}, source file does not exist",
                    file.path
                ));
            }
            verify_review_baseline(file, &source)?;
            let Some(content) = effective_content(file) else {
                return Err(format!("missing candidate content for {}", file.path));
            };
            std::fs::write(&source, content)
                .map_err(|e| format!("failed to write {}: {e}", file.path))?;
            changed_files.push(FileChange {
                path: file.path.clone(),
                action: "modified".to_string(),
            });
            Ok(())
        }
        "deleted" => {
            let source = resolve_patch_path(root, &file.path)?;
            if !source.is_file() {
                return Err(format!(
                    "conflict: cannot delete {}, source file does not exist",
                    file.path
                ));
            }
            verify_review_baseline(file, &source)?;
            std::fs::remove_file(&source)
                .map_err(|e| format!("failed to delete {}: {e}", file.path))?;
            changed_files.push(FileChange {
                path: file.path.clone(),
                action: "deleted".to_string(),
            });
            Ok(())
        }
        "renamed" => {
            let Some(dest_path) = file.move_to.as_ref() else {
                return Err(format!(
                    "missing move target for renamed file {}",
                    file.path
                ));
            };
            let source = resolve_patch_path(root, &file.path)?;
            let target = resolve_patch_path(root, dest_path)?;
            if !source.is_file() {
                return Err(format!(
                    "conflict: cannot rename {}, source file does not exist",
                    file.path
                ));
            }
            verify_review_baseline(file, &source)?;
            if target != source && target.exists() {
                return Err(format!(
                    "conflict: cannot rename {} to {}, target already exists",
                    file.path, dest_path
                ));
            }
            let Some(content) = effective_content(file) else {
                return Err(format!("missing candidate content for {}", file.path));
            };
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent for {}: {e}", dest_path))?;
            }
            std::fs::write(&target, content)
                .map_err(|e| format!("failed to write {}: {e}", dest_path))?;
            if target != source {
                std::fs::remove_file(&source)
                    .map_err(|e| format!("failed to remove {}: {e}", file.path))?;
            }
            changed_files.push(FileChange {
                path: file.path.clone(),
                action: "renamed".to_string(),
            });
            Ok(())
        }
        other => Err(format!("unsupported review action: {other}")),
    }
}

fn verify_review_baseline(file: &PendingPatchReviewFile, source: &Path) -> Result<(), String> {
    let Some(expected) = file.base_content.as_ref() else {
        return Err(format!(
            "missing baseline content for review file {}",
            file.path
        ));
    };
    let current = std::fs::read_to_string(source)
        .map_err(|e| format!("failed to read {} for conflict check: {e}", file.path))?;
    if current != *expected {
        return Err(format!(
            "conflict: {} has changed since review was created. please regenerate patch review.",
            file.path
        ));
    }
    Ok(())
}

fn effective_content(file: &PendingPatchReviewFile) -> Option<&str> {
    file.edited_content
        .as_deref()
        .or(file.candidate_content.as_deref())
}

fn build_review_file(
    root: &Path,
    action: ParsedPatchAction,
) -> Result<PendingPatchReviewFile, String> {
    match action {
        ParsedPatchAction::Add { path, lines } => {
            let target = resolve_patch_path(root, &path)?;
            if target.exists() {
                return Err(format!("cannot add {path}: file already exists"));
            }
            let content = join_file_lines(&lines, "\n", !lines.is_empty());
            Ok(PendingPatchReviewFile {
                path: normalize_patch_display_path(&path),
                action: "created".to_string(),
                move_to: None,
                base_content: None,
                candidate_content: Some(content),
                edited_content: None,
                keep: true,
            })
        }
        ParsedPatchAction::Update {
            path,
            move_to,
            hunks,
        } => {
            let source = resolve_patch_path(root, &path)?;
            if !source.is_file() {
                return Err(format!("cannot update {path}: file does not exist"));
            }

            let original = std::fs::read_to_string(&source)
                .map_err(|e| format!("failed to read {path}: {e}"))?;
            let eol = detect_eol(&original);
            let (mut lines, final_newline) = split_file_lines(&original);
            apply_update_hunks(&mut lines, &hunks, &path)?;
            let updated = join_file_lines(&lines, eol, final_newline);
            let normalized_path = normalize_patch_display_path(&path);

            if let Some(dest) = move_to {
                let target = resolve_patch_path(root, &dest)?;
                if target != source && target.exists() {
                    return Err(format!("cannot move {path} to {dest}: destination exists"));
                }
                Ok(PendingPatchReviewFile {
                    path: normalized_path,
                    action: "renamed".to_string(),
                    move_to: Some(normalize_patch_display_path(&dest)),
                    base_content: Some(original),
                    candidate_content: Some(updated),
                    edited_content: None,
                    keep: true,
                })
            } else {
                Ok(PendingPatchReviewFile {
                    path: normalized_path,
                    action: "modified".to_string(),
                    move_to: None,
                    base_content: Some(original),
                    candidate_content: Some(updated),
                    edited_content: None,
                    keep: true,
                })
            }
        }
        ParsedPatchAction::Delete { path } => {
            let target = resolve_patch_path(root, &path)?;
            if !target.is_file() {
                return Err(format!("cannot delete {path}: file does not exist"));
            }
            let original = std::fs::read_to_string(&target)
                .map_err(|e| format!("failed to read {path}: {e}"))?;
            Ok(PendingPatchReviewFile {
                path: normalize_patch_display_path(&path),
                action: "deleted".to_string(),
                move_to: None,
                base_content: Some(original),
                candidate_content: None,
                edited_content: None,
                keep: true,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedPatchAction {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<PatchHunk>,
    },
    Delete {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PatchHunk {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

fn parse_patch_actions(patch: &str) -> Result<Vec<ParsedPatchAction>, String> {
    let normalized = extract_embedded_patch_block(patch)?;
    let lines: Vec<&str> = normalized.lines().collect();

    let mut actions = Vec::new();
    let mut i = 1;
    while i < lines.len() {
        let line = lines[i];
        if line == "*** End Patch" {
            return Ok(actions);
        }

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            i += 1;
            let mut added = Vec::new();
            while i < lines.len() && !is_patch_section_boundary(lines[i]) {
                let Some(content) = lines[i].strip_prefix('+') else {
                    return Err(format!("invalid add-file line for {path}: expected '+'"));
                };
                added.push(content.to_string());
                i += 1;
            }
            actions.push(ParsedPatchAction::Add {
                path: path.trim().to_string(),
                lines: added,
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            actions.push(ParsedPatchAction::Delete {
                path: path.trim().to_string(),
            });
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Update File: ") {
            i += 1;
            let mut move_to = None;
            let mut hunks = Vec::new();
            let mut current: Option<PatchHunk> = None;

            while i < lines.len() && !is_patch_section_boundary(lines[i]) {
                let line = lines[i];
                if let Some(dest) = line.strip_prefix("*** Move to: ") {
                    if move_to.is_some() {
                        return Err(format!("multiple move destinations for {path}"));
                    }
                    move_to = Some(dest.trim().to_string());
                    i += 1;
                    continue;
                }

                if line.starts_with("*** Desc: ") {
                    i += 1;
                    continue;
                }

                if line == "*** End of File" {
                    i += 1;
                    continue;
                }

                if current.is_none()
                    && line.starts_with("--- ")
                    && lines
                        .get(i + 1)
                        .is_some_and(|next| next.starts_with("+++ "))
                {
                    i += 2;
                    continue;
                }

                if line.starts_with("@@") {
                    if let Some(hunk) = current.take() {
                        hunks.push(hunk);
                    }
                    current = Some(PatchHunk {
                        old_lines: Vec::new(),
                        new_lines: Vec::new(),
                    });
                    i += 1;
                    continue;
                }

                let hunk = current.get_or_insert_with(|| PatchHunk {
                    old_lines: Vec::new(),
                    new_lines: Vec::new(),
                });

                if let Some(content) = line.strip_prefix(' ') {
                    hunk.old_lines.push(content.to_string());
                    hunk.new_lines.push(content.to_string());
                } else if let Some(content) = line.strip_prefix('-') {
                    hunk.old_lines.push(content.to_string());
                } else if let Some(content) = line.strip_prefix('+') {
                    hunk.new_lines.push(content.to_string());
                } else {
                    return Err(format!("invalid update line for {path}: {line}"));
                }

                i += 1;
            }

            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            if hunks.is_empty() && move_to.is_none() {
                return Err(format!("update for {path} contains no changes"));
            }
            actions.push(ParsedPatchAction::Update {
                path: path.trim().to_string(),
                move_to,
                hunks,
            });
            continue;
        }

        return Err(format!("unexpected patch line: {line}"));
    }

    Err("patch must end with *** End Patch".to_string())
}

fn is_patch_section_boundary(line: &str) -> bool {
    line == "*** End Patch"
        || line.starts_with("*** Add File: ")
        || line.starts_with("*** Update File: ")
        || line.starts_with("*** Delete File: ")
}

fn extract_embedded_patch_block(input: &str) -> Result<String, String> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let Some(begin) = lines.iter().position(|line| is_patch_begin_marker(line)) else {
        return Err("patch must start with *** Begin Patch".to_string());
    };
    let Some(end) = lines.iter().rposition(|line| is_patch_end_marker(line)) else {
        return Err("patch must end with *** End Patch".to_string());
    };
    if end < begin {
        return Err("patch must end with *** End Patch".to_string());
    }

    Ok(lines[begin..=end]
        .iter()
        .map(|line| normalize_patch_directive_line(line))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn is_patch_begin_marker(line: &str) -> bool {
    matches!(
        normalize_patch_directive_line(line).as_str(),
        "*** Begin Patch"
    )
}

fn is_patch_end_marker(line: &str) -> bool {
    matches!(
        normalize_patch_directive_line(line).as_str(),
        "*** End Patch"
    )
}

fn normalize_patch_directive_line(line: &str) -> String {
    let trimmed = line.trim();
    if !trimmed.starts_with("*** ") {
        return line.to_string();
    }
    trimmed.strip_suffix(" ***").unwrap_or(trimmed).to_string()
}

fn apply_update_hunks(
    lines: &mut Vec<String>,
    hunks: &[PatchHunk],
    path: &str,
) -> Result<(), String> {
    let mut cursor = 0usize;

    for hunk in hunks {
        if hunk.old_lines.is_empty() {
            lines.splice(cursor..cursor, hunk.new_lines.clone());
            cursor += hunk.new_lines.len();
            continue;
        }

        let pos = find_subsequence(lines, &hunk.old_lines, cursor)
            .or_else(|| find_subsequence(lines, &hunk.old_lines, 0))
            .or_else(|| find_unique_relaxed_subsequence(lines, &hunk.old_lines))
            .ok_or_else(|| format_hunk_match_error(path, &hunk.old_lines))?;
        let end = pos + hunk.old_lines.len();
        lines.splice(pos..end, hunk.new_lines.clone());
        cursor = pos + hunk.new_lines.len();
    }

    Ok(())
}

fn find_subsequence(lines: &[String], needle: &[String], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(lines.len()));
    }
    if needle.len() > lines.len() {
        return None;
    }
    let max_start = lines.len().saturating_sub(needle.len());
    let start = start.min(max_start);
    (start..=max_start).find(|idx| {
        lines[*idx..*idx + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(a, b)| a == b)
    })
}

fn resolve_patch_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("patch path must not be empty".to_string());
    }

    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        let target = normalize_absolute_patch_path(raw, input)?;
        let absolute_root = if root.is_absolute() {
            normalize_absolute_patch_path(root, &root.display().to_string())?
        } else {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("failed to resolve workspace path: {e}"))?;
            normalize_absolute_patch_path(&cwd.join(root), &root.display().to_string())?
        };
        if !path_is_within(&absolute_root, &target) {
            return Err(format!(
                "absolute patch path must be inside the workspace {}: {input}",
                absolute_root.display()
            ));
        }
        return Ok(target);
    }

    let portable = trimmed.replace('\\', "/");
    if portable.contains(':') {
        return Err(format!("invalid patch path: {input}"));
    }

    let mut path = root.to_path_buf();
    for component in Path::new(&portable).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("patch path must not contain '..': {input}"));
            }
            _ => {
                return Err(format!("invalid patch path component: {input}"));
            }
        }
    }

    Ok(path)
}

fn find_unique_relaxed_subsequence(lines: &[String], needle: &[String]) -> Option<usize> {
    if needle.is_empty() || needle.len() > lines.len() {
        return None;
    }

    let mut matched = None;
    for index in 0..=lines.len() - needle.len() {
        let is_match = lines[index..index + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(actual, expected)| relaxed_patch_line(actual) == relaxed_patch_line(expected));
        if is_match {
            if matched.is_some() {
                return None;
            }
            matched = Some(index);
        }
    }
    matched
}

fn relaxed_patch_line(line: &str) -> &str {
    line.trim_start_matches('\u{feff}').trim_end()
}

fn format_hunk_match_error(path: &str, old_lines: &[String]) -> String {
    const MAX_PREVIEW_LINES: usize = 8;
    const MAX_PREVIEW_CHARS: usize = 600;

    let mut preview = old_lines
        .iter()
        .take(MAX_PREVIEW_LINES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    if preview.chars().count() > MAX_PREVIEW_CHARS {
        preview = preview.chars().take(MAX_PREVIEW_CHARS).collect();
        preview.push_str("...");
    } else if old_lines.len() > MAX_PREVIEW_LINES {
        preview.push_str("\n...");
    }

    format!(
        "failed to match hunk in {path} ({} expected lines). The file content has changed or the patch context is stale. Re-read the current file and retry apply_patch with a smaller hunk containing only the changed lines and a few current context lines. Preview: {preview}",
        old_lines.len()
    )
}

fn normalize_absolute_patch_path(path: &Path, input: &str) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("patch path must not contain '..': {input}"));
            }
        }
    }
    Ok(normalized)
}

fn path_is_within(root: &Path, target: &Path) -> bool {
    let root_components = comparable_path_components(root);
    let target_components = comparable_path_components(target);
    target_components.starts_with(&root_components)
}

fn comparable_path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let value = component.as_os_str().to_string_lossy().into_owned();
            if cfg!(windows) {
                value.to_lowercase()
            } else {
                value
            }
        })
        .collect()
}

fn normalize_patch_display_path(input: &str) -> String {
    input
        .trim()
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn detect_eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn split_file_lines(content: &str) -> (Vec<String>, bool) {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let final_newline = normalized.ends_with('\n');
    let body = if final_newline {
        &normalized[..normalized.len().saturating_sub(1)]
    } else {
        normalized.as_str()
    };

    if body.is_empty() {
        (Vec::new(), final_newline)
    } else {
        (
            body.split('\n').map(|line| line.to_string()).collect(),
            final_newline,
        )
    }
}

fn join_file_lines(lines: &[String], eol: &str, final_newline: bool) -> String {
    let mut content = lines.join(eol);
    if final_newline {
        content.push_str(eol);
    }
    content
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_review_session_keeps_workspace_unchanged() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-review-build-{}", uuid::Uuid::new_v4()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.txt"), "old\nline\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: src/app.txt
@@
-old
+new
*** Add File: src/new.txt
+hello
*** End Patch"#;

        let review = build_pending_patch_review(&root, "thread-1", "call-1", patch).unwrap();
        assert_eq!(review.files.len(), 2);
        assert_eq!(
            std::fs::read_to_string(src.join("app.txt")).unwrap(),
            "old\nline\n"
        );
        assert!(!src.join("new.txt").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn build_review_accepts_workspace_absolute_path_and_unified_headers() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-review-absolute-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("index.html");
        std::fs::write(&target, "<title>ABCDE</title>\n").unwrap();
        let patch = format!(
            "*** Begin Patch\n*** Update File: {}\n--- a/index.html\n+++ b/index.html\n@@ -1 +1 @@\n-<title>ABCDE</title>\n+<title>时尚代码</title>\n*** End Patch",
            target.display()
        );

        let review = build_pending_patch_review(&root, "thread-1", "call-1", &patch).unwrap();

        assert_eq!(review.files.len(), 1);
        assert_eq!(
            review.files[0].candidate_content.as_deref(),
            Some("<title>时尚代码</title>\n")
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "<title>ABCDE</title>\n"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn build_review_accepts_wrapped_patch_block_with_trailing_stars() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-review-wrapper-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("index.html");
        std::fs::write(&target, "<title>ABCDE</title>\n").unwrap();
        let patch = format!(
            r#"D:\rustwork\cn-codex-lite-rs\src\App.tsx ***
*** Begin Patch ***
*** Update File: {} ***
@@ -1 +1 @@
-<title>ABCDE</title>
+<title>鏃跺皻浠ｇ爜</title>
*** End Patch ***
Applied patch src/App.tsx ***
done"#,
            target.display()
        );

        let review = build_pending_patch_review(&root, "thread-1", "call-1", &patch).unwrap();

        assert_eq!(review.files.len(), 1);
        assert_eq!(
            review.files[0].candidate_content.as_deref(),
            Some("<title>鏃跺皻浠ｇ爜</title>\n")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn build_review_accepts_desc_metadata_lines() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-review-desc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("style.css");
        std::fs::write(&target, "old\n").unwrap();
        let patch = format!(
            "*** Begin Patch\n*** Update File: {}\n*** Desc: Remove UTF-8 BOM (U+FEFF) from the first line\n--- a/src/style.css\n+++ b/src/style.css\n@@ -1 +1 @@\n-old\n+new\n*** End Patch",
            target.display()
        );

        let review = build_pending_patch_review(&root, "thread-1", "call-1", &patch).unwrap();

        assert_eq!(review.files.len(), 1);
        assert_eq!(review.files[0].candidate_content.as_deref(), Some("new\n"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn build_review_accepts_unique_trailing_whitespace_context_difference() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-review-relaxed-context-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("App.tsx");
        std::fs::write(&target, "function parseChart() {   \n  return old;\t\n}\n").unwrap();
        let patch = r#"*** Begin Patch
*** Update File: App.tsx
@@
 function parseChart() {
-  return old;
+  return updated;
 }
*** End Patch"#;

        let review = build_pending_patch_review(&root, "thread-1", "call-1", patch).unwrap();
        assert_eq!(
            review.files[0].candidate_content.as_deref(),
            Some("function parseChart() {\n  return updated;\n}\n")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_review_respects_keep_selection() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-review-apply-{}", uuid::Uuid::new_v4()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.txt"), "old\n").unwrap();
        std::fs::write(src.join("remove.txt"), "remove\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: src/app.txt
@@
-old
+new
*** Add File: src/new.txt
+hello
*** Delete File: src/remove.txt
*** End Patch"#;

        let review = build_pending_patch_review(&root, "thread-1", "call-1", patch).unwrap();
        let result = apply_review_to_workspace(
            &root,
            &review,
            &ReviewApplyRequest {
                keep_all: false,
                keep_paths: vec!["src/new.txt".to_string()],
            },
        )
        .unwrap();

        assert_eq!(
            result.changed_files,
            vec![FileChange {
                path: "src/new.txt".to_string(),
                action: "created".to_string(),
            }]
        );
        assert_eq!(
            std::fs::read_to_string(src.join("app.txt")).unwrap(),
            "old\n"
        );
        assert_eq!(
            std::fs::read_to_string(src.join("new.txt")).unwrap(),
            "hello\n"
        );
        assert!(src.join("remove.txt").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_review_detects_conflict_when_source_changed() {
        let root =
            std::env::temp_dir().join(format!("cn-codex-review-conflict-{}", uuid::Uuid::new_v4()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.txt"), "old\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: src/app.txt
@@
-old
+new
*** End Patch"#;

        let review = build_pending_patch_review(&root, "thread-1", "call-1", patch).unwrap();
        std::fs::write(src.join("app.txt"), "external change\n").unwrap();
        let err =
            apply_review_to_workspace(&root, &review, &ReviewApplyRequest::default()).unwrap_err();
        assert!(err.contains("has changed since review was created"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_review_rejects_empty_keep_selection() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-review-empty-selection-{}",
            uuid::Uuid::new_v4()
        ));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.txt"), "old\n").unwrap();

        let patch = r#"*** Begin Patch
*** Update File: src/app.txt
@@
-old
+new
*** End Patch"#;

        let mut review = build_pending_patch_review(&root, "thread-1", "call-1", patch).unwrap();
        for file in &mut review.files {
            file.keep = false;
        }
        let err =
            apply_review_to_workspace(&root, &review, &ReviewApplyRequest::default()).unwrap_err();
        assert!(err.contains("no files selected"));
        std::fs::remove_dir_all(root).ok();
    }
}
