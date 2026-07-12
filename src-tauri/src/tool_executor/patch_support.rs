use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct ApplyPatchReport {
    pub(crate) changes: Vec<ApplyPatchReportChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct ApplyPatchReportChange {
    pub(crate) path: String,
    pub(crate) action: &'static str,
    pub(crate) move_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyPatchProgressChange {
    pub(crate) path: String,
    pub(crate) action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) move_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParsedPatchAction {
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
pub(crate) struct PatchHunk {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

enum PreparedPatchAction {
    Add {
        path: String,
        target: PathBuf,
        content: String,
    },
    Update {
        path: String,
        source: PathBuf,
        target: PathBuf,
        updated: String,
        move_to: Option<String>,
    },
    Delete {
        path: String,
        target: PathBuf,
    },
}

pub(crate) fn extract_patch_argument(arguments: &str) -> Result<String, String> {
    let trimmed = arguments.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(patch) = extract_embedded_patch_block(trimmed)
    {
        return Ok(patch);
    }

    let value: serde_json::Value =
        serde_json::from_str(arguments).map_err(|e| format!("Invalid apply_patch args: {e}"))?;
    let patch = value
        .get("patch")
        .or_else(|| value.get("command"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "Invalid apply_patch args: expected raw patch text or a string field named 'patch' or 'command'".to_string()
        })?;

    extract_embedded_patch_block(patch)
}

pub(crate) fn patch_display_label(patch: &str) -> String {
    parse_patch_actions(patch)
        .ok()
        .and_then(|actions| actions.first().map(action_display_path))
        .unwrap_or_else(|| "apply_patch".to_string())
}

fn action_display_path(action: &ParsedPatchAction) -> String {
    match action {
        ParsedPatchAction::Add { path, .. }
        | ParsedPatchAction::Update { path, .. }
        | ParsedPatchAction::Delete { path } => path.clone(),
    }
}

pub(crate) fn apply_patch_progress_changes(
    actions: &[ParsedPatchAction],
) -> Vec<ApplyPatchProgressChange> {
    actions
        .iter()
        .map(|action| match action {
            ParsedPatchAction::Add { path, .. } => ApplyPatchProgressChange {
                path: normalize_patch_display_path(path),
                action: "created",
                move_to: None,
            },
            ParsedPatchAction::Update { path, move_to, .. } => ApplyPatchProgressChange {
                path: normalize_patch_display_path(path),
                action: if move_to.is_some() {
                    "renamed"
                } else {
                    "modified"
                },
                move_to: move_to
                    .as_ref()
                    .map(|dest| normalize_patch_display_path(dest)),
            },
            ParsedPatchAction::Delete { path } => ApplyPatchProgressChange {
                path: normalize_patch_display_path(path),
                action: "deleted",
                move_to: None,
            },
        })
        .collect()
}

#[allow(dead_code)]
pub(crate) fn apply_patch_to_workspace(
    root: &Path,
    patch: &str,
) -> Result<ApplyPatchReport, String> {
    let actions = parse_patch_actions(patch)?;
    if actions.is_empty() {
        return Err("patch contains no file changes".to_string());
    }

    // Resolve every path and apply every hunk in memory before touching disk. This
    // prevents a malformed later action from leaving an earlier file modified.
    let prepared = prepare_patch_actions(root, actions)?;

    let mut report = ApplyPatchReport {
        changes: Vec::new(),
    };
    for action in prepared {
        match action {
            PreparedPatchAction::Add {
                path,
                target,
                content,
            } => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("failed to create parent for {path}: {e}"))?;
                }
                std::fs::write(&target, content)
                    .map_err(|e| format!("failed to write {path}: {e}"))?;
                report.changes.push(ApplyPatchReportChange {
                    path: normalize_patch_display_path(&path),
                    action: "created",
                    move_to: None,
                });
            }
            PreparedPatchAction::Update {
                path,
                source,
                target,
                updated,
                move_to,
            } => {
                if let Some(dest) = &move_to {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("failed to create parent for {dest}: {e}"))?;
                    }
                    std::fs::write(&target, updated)
                        .map_err(|e| format!("failed to write {dest}: {e}"))?;
                    if target != source {
                        std::fs::remove_file(&source)
                            .map_err(|e| format!("failed to remove {path}: {e}"))?;
                    }
                    report.changes.push(ApplyPatchReportChange {
                        path: normalize_patch_display_path(&path),
                        action: "renamed",
                        move_to: Some(normalize_patch_display_path(dest)),
                    });
                } else {
                    std::fs::write(&source, updated)
                        .map_err(|e| format!("failed to write {path}: {e}"))?;
                    report.changes.push(ApplyPatchReportChange {
                        path: normalize_patch_display_path(&path),
                        action: "modified",
                        move_to: None,
                    });
                }
            }
            PreparedPatchAction::Delete { path, target } => {
                std::fs::remove_file(&target)
                    .map_err(|e| format!("failed to delete {path}: {e}"))?;
                report.changes.push(ApplyPatchReportChange {
                    path: normalize_patch_display_path(&path),
                    action: "deleted",
                    move_to: None,
                });
            }
        }
    }

    Ok(report)
}

fn prepare_patch_actions(
    root: &Path,
    actions: Vec<ParsedPatchAction>,
) -> Result<Vec<PreparedPatchAction>, String> {
    let mut claimed_paths = HashSet::new();
    let mut prepared = Vec::with_capacity(actions.len());

    for action in actions {
        match action {
            ParsedPatchAction::Add { path, lines } => {
                let target = resolve_patch_path(root, &path)?;
                claim_patch_path(&mut claimed_paths, &target, &path)?;
                if target.exists() {
                    return Err(format!("cannot add {path}: file already exists"));
                }
                prepared.push(PreparedPatchAction::Add {
                    path,
                    target,
                    content: join_file_lines(&lines, "\n", !lines.is_empty()),
                });
            }
            ParsedPatchAction::Update {
                path,
                move_to,
                hunks,
            } => {
                let source = resolve_patch_path(root, &path)?;
                claim_patch_path(&mut claimed_paths, &source, &path)?;
                if !source.is_file() {
                    return Err(format!("cannot update {path}: file does not exist"));
                }

                let target = if let Some(dest) = &move_to {
                    let target = resolve_patch_path(root, dest)?;
                    if target != source {
                        claim_patch_path(&mut claimed_paths, &target, dest)?;
                        if target.exists() {
                            return Err(format!(
                                "cannot move {path} to {dest}: destination exists"
                            ));
                        }
                    }
                    target
                } else {
                    source.clone()
                };

                let original = std::fs::read_to_string(&source)
                    .map_err(|e| format!("failed to read {path}: {e}"))?;
                let eol = detect_eol(&original);
                let (mut lines, final_newline) = split_file_lines(&original);
                apply_update_hunks(&mut lines, &hunks, &path)?;
                prepared.push(PreparedPatchAction::Update {
                    path,
                    source,
                    target,
                    updated: join_file_lines(&lines, eol, final_newline),
                    move_to,
                });
            }
            ParsedPatchAction::Delete { path } => {
                let target = resolve_patch_path(root, &path)?;
                claim_patch_path(&mut claimed_paths, &target, &path)?;
                if !target.is_file() {
                    return Err(format!("cannot delete {path}: file does not exist"));
                }
                prepared.push(PreparedPatchAction::Delete { path, target });
            }
        }
    }

    Ok(prepared)
}

fn claim_patch_path(
    claimed_paths: &mut HashSet<PathBuf>,
    path: &Path,
    display_path: &str,
) -> Result<(), String> {
    if claimed_paths.insert(path.to_path_buf()) {
        Ok(())
    } else {
        Err(format!(
            "patch contains multiple actions for the same path: {display_path}"
        ))
    }
}

pub(crate) fn parse_patch_actions(patch: &str) -> Result<Vec<ParsedPatchAction>, String> {
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

                // Some providers wrap a standard unified diff inside a Codex
                // Update File section. These headers identify the same file and
                // are metadata, not deleted/added source lines.
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
    matches!(normalize_patch_directive_line(line).as_str(), "*** Begin Patch")
}

fn is_patch_end_marker(line: &str) -> bool {
    matches!(normalize_patch_directive_line(line).as_str(), "*** End Patch")
}

fn normalize_patch_directive_line(line: &str) -> String {
    let trimmed = line.trim();
    if !trimmed.starts_with("*** ") {
        return line.to_string();
    }
    trimmed.strip_suffix(" ***").unwrap_or(trimmed).to_string()
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        .join("\\n");
    if preview.chars().count() > MAX_PREVIEW_CHARS {
        preview = preview.chars().take(MAX_PREVIEW_CHARS).collect();
        preview.push_str("...");
    } else if old_lines.len() > MAX_PREVIEW_LINES {
        preview.push_str("\\n...");
    }

    format!(
        "failed to match hunk in {path} ({} expected lines). The file content has changed or the patch context is stale. Re-read the current file and retry apply_patch with a smaller hunk containing only the changed lines and a few current context lines. Preview: {preview}",
        old_lines.len()
    )
}

#[allow(dead_code)]
fn resolve_patch_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("patch path must not be empty".to_string());
    }

    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        let target = normalize_patch_path(raw, input)?;
        let absolute_root = if root.is_absolute() {
            normalize_patch_path(root, &root.display().to_string())?
        } else {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("failed to resolve workspace path: {e}"))?;
            normalize_patch_path(&cwd.join(root), &root.display().to_string())?
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

fn normalize_patch_path(path: &Path, input: &str) -> Result<PathBuf, String> {
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

#[allow(dead_code)]
fn detect_eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn join_file_lines(lines: &[String], eol: &str, final_newline: bool) -> String {
    let mut content = lines.join(eol);
    if final_newline {
        content.push_str(eol);
    }
    content
}

#[allow(dead_code)]
pub(crate) fn format_apply_patch_report(report: &ApplyPatchReport) -> String {
    let mut output = "Success. Applied patch.".to_string();
    for change in &report.changes {
        match (&change.action, &change.move_to) {
            (&"renamed", Some(dest)) => {
                output.push_str(&format!("\n- renamed {} -> {dest}", change.path));
            }
            _ => {
                output.push_str(&format!("\n- {} {}", change.action, change.path));
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        ParsedPatchAction, PatchHunk, apply_update_hunks, extract_patch_argument,
        parse_patch_actions,
    };

    #[test]
    fn extract_patch_argument_accepts_wrapped_raw_patch_block() {
        let raw = r#"D:\rustwork\cn-codex-lite-rs\src\App.tsx ***
*** Begin Patch ***
*** Update File: D:\rustwork\cn-codex-lite-rs\src\App.tsx ***
@@ -1 +1 @@
-old
+new
*** End Patch ***
output
Error applying patch: patch must start with *** Begin Patch"#;

        let patch = extract_patch_argument(raw).unwrap();
        assert!(patch.starts_with("*** Begin Patch\n"));
        assert!(patch.contains("*** Update File: D:\\rustwork\\cn-codex-lite-rs\\src\\App.tsx"));
        assert!(patch.ends_with("*** End Patch"));
    }

    #[test]
    fn parse_patch_actions_accepts_trailing_stars_and_wrappers() {
        let raw = r#"src/App.tsx ***
*** Begin Patch ***
*** Update File: src/App.tsx ***
@@ -1 +1 @@
-old
+new
*** End Patch ***
Applied patch src/App.tsx ***
done"#;

        let actions = parse_patch_actions(raw).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ParsedPatchAction::Update { path, .. } => assert_eq!(path, "src/App.tsx"),
            other => panic!("expected update action, got {other:?}"),
        }
    }

    #[test]
    fn parse_patch_actions_ignores_desc_metadata_lines() {
        let raw = r#"*** Begin Patch
*** Update File: src/style.css
*** Desc: Remove UTF-8 BOM (U+FEFF) from the first line
--- a/src/style.css
+++ b/src/style.css
@@ -1 +1 @@
-old
+new
*** End Patch"#;

        let actions = parse_patch_actions(raw).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ParsedPatchAction::Update { path, .. } => assert_eq!(path, "src/style.css"),
            other => panic!("expected update action, got {other:?}"),
        }
    }

    #[test]
    fn apply_update_hunks_accepts_unique_trailing_whitespace_and_bom_differences() {
        let mut lines = vec![
            "\u{feff}function parseChart() {   ".to_string(),
            "  return old;\t".to_string(),
            "}".to_string(),
        ];
        let hunks = vec![PatchHunk {
            old_lines: vec![
                "function parseChart() {".to_string(),
                "  return old;".to_string(),
                "}".to_string(),
            ],
            new_lines: vec![
                "function parseChart() {".to_string(),
                "  return updated;".to_string(),
                "}".to_string(),
            ],
        }];

        apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap();
        assert_eq!(lines[1], "  return updated;");
    }

    #[test]
    fn apply_update_hunks_rejects_ambiguous_relaxed_matches() {
        let mut lines = vec!["same ".to_string(), "same\t".to_string()];
        let hunks = vec![PatchHunk {
            old_lines: vec!["same".to_string()],
            new_lines: vec!["changed".to_string()],
        }];

        let error = apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap_err();
        assert!(error.contains("Re-read the current file"));
        assert!(error.contains("smaller hunk"));
    }
}
