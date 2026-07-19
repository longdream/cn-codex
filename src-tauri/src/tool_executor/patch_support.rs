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
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
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
    change_context: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    is_end_of_file: bool,
    additions: usize,
    deletions: usize,
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
            ParsedPatchAction::Add { path, lines } => ApplyPatchProgressChange {
                path: normalize_patch_display_path(path),
                action: "created",
                move_to: None,
                additions: lines.len(),
                deletions: 0,
            },
            ParsedPatchAction::Update {
                path,
                move_to,
                hunks,
            } => {
                let additions = hunks.iter().map(|hunk| hunk.additions).sum();
                let deletions = hunks.iter().map(|hunk| hunk.deletions).sum();
                ApplyPatchProgressChange {
                    path: normalize_patch_display_path(path),
                    action: if move_to.is_some() {
                        "renamed"
                    } else {
                        "modified"
                    },
                    move_to: move_to
                        .as_ref()
                        .map(|dest| normalize_patch_display_path(dest)),
                    additions,
                    deletions,
                }
            }
            ParsedPatchAction::Delete { path } => ApplyPatchProgressChange {
                path: normalize_patch_display_path(path),
                action: "deleted",
                move_to: None,
                additions: 0,
                deletions: 0,
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
            let mut implicit_context_count = 0usize;

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
                    let Some(hunk) = current.as_mut() else {
                        return Err(format!(
                            "end-of-file marker for {path} must follow an update hunk"
                        ));
                    };
                    hunk.is_end_of_file = true;
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
                        hunks.push(finalize_parsed_hunk(hunk, implicit_context_count));
                    }
                    implicit_context_count = 0;
                    current = Some(PatchHunk {
                        change_context: parse_hunk_change_context(line),
                        old_lines: Vec::new(),
                        new_lines: Vec::new(),
                        is_end_of_file: false,
                        additions: 0,
                        deletions: 0,
                    });
                    i += 1;
                    continue;
                }

                let hunk = current.get_or_insert_with(|| PatchHunk {
                    change_context: None,
                    old_lines: Vec::new(),
                    new_lines: Vec::new(),
                    is_end_of_file: false,
                    additions: 0,
                    deletions: 0,
                });

                if let Some(content) = line.strip_prefix(' ') {
                    hunk.old_lines.push(content.to_string());
                    hunk.new_lines.push(content.to_string());
                } else if line.starts_with("|-|") {
                    hunk.old_lines
                        .push(normalize_pipe_wrapped_content(&line[2..]));
                    hunk.deletions += 1;
                } else if line.starts_with("+||") {
                    hunk.new_lines
                        .push(normalize_pipe_wrapped_content(&line[2..]));
                    hunk.additions += 1;
                } else if let Some(content) = line.strip_prefix('-') {
                    hunk.old_lines.push(content.to_string());
                    hunk.deletions += 1;
                } else if let Some(content) = line.strip_prefix('+') {
                    hunk.new_lines.push(content.to_string());
                    hunk.additions += 1;
                } else if line.is_empty() {
                    hunk.old_lines.push(String::new());
                    hunk.new_lines.push(String::new());
                } else {
                    let content = if line.starts_with("||") {
                        line.strip_prefix('|').unwrap_or(line)
                    } else {
                        line
                    };
                    let content = normalize_pipe_wrapped_content(content);
                    hunk.old_lines.push(content.clone());
                    hunk.new_lines.push(content);
                    implicit_context_count += 1;
                }

                i += 1;
            }

            if let Some(hunk) = current.take() {
                hunks.push(finalize_parsed_hunk(hunk, implicit_context_count));
            }
            hunks = combine_implicit_replacement_pairs(hunks);
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

fn normalize_pipe_wrapped_content(content: &str) -> String {
    if content.starts_with('|') && content[1..].contains('│') {
        format!("│{}", &content[1..])
    } else {
        content.to_string()
    }
}

fn finalize_parsed_hunk(mut hunk: PatchHunk, implicit_context_count: usize) -> PatchHunk {
    if implicit_context_count > 0 && hunk.deletions == 0 && hunk.additions > 0 {
        let implicit_prefix = implicit_context_count.min(hunk.new_lines.len());
        hunk.new_lines.drain(..implicit_prefix);
        hunk.deletions += implicit_prefix;
    }
    hunk
}

fn combine_implicit_replacement_pairs(hunks: Vec<PatchHunk>) -> Vec<PatchHunk> {
    let mut combined = Vec::with_capacity(hunks.len());
    let mut index = 0usize;
    while index < hunks.len() {
        let first = &hunks[index];
        let paired = hunks.get(index + 1).is_some_and(|second| {
            first.change_context.is_some()
                && first.additions == 0
                && first.deletions == 0
                && first.old_lines == first.new_lines
                && second.change_context.is_none()
                && second.additions == 0
                && second.deletions == 0
                && second.old_lines == second.new_lines
        });
        if paired {
            let second = &hunks[index + 1];
            if first.old_lines != second.old_lines {
                let mut replacement = first.clone();
                replacement.new_lines = second.new_lines.clone();
                replacement.deletions = replacement.old_lines.len();
                replacement.additions = replacement.new_lines.len();
                combined.push(replacement);
            }
            index += 2;
        } else {
            combined.push(first.clone());
            index += 1;
        }
    }
    combined
}

fn parse_hunk_change_context(line: &str) -> Option<String> {
    let suffix = line.strip_prefix("@@")?;
    let suffix = suffix.strip_prefix(' ').unwrap_or(suffix);
    if suffix.trim().is_empty() || suffix.trim() == "*** End of File" {
        return None;
    }

    // Standard unified-diff ranges are positional metadata, not source text.
    if suffix.starts_with('-') {
        return suffix
            .find("@@")
            .map(|end| suffix[end + 2..].trim())
            .filter(|context| !context.is_empty())
            .map(str::to_string);
    }

    Some(suffix.to_string())
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
    let normalized = trimmed.strip_suffix(" ***").unwrap_or(trimmed);
    match normalized {
        "*** End of Patch" => "*** End Patch".to_string(),
        other => other.to_string(),
    }
}

#[allow(dead_code)]
fn apply_update_hunks(
    lines: &mut Vec<String>,
    hunks: &[PatchHunk],
    path: &str,
) -> Result<(), String> {
    let mut cursor = 0usize;
    let mut replacements: Vec<(usize, usize, Vec<String>)> = Vec::new();

    for hunk in hunks {
        if let Some(context) = &hunk.change_context {
            if let Some(line_hint) = parse_patch_line_hint(context) {
                cursor = line_hint.saturating_sub(1).min(lines.len());
            } else {
                let context_lines = std::slice::from_ref(context);
                let context_pos = seek_sequence(lines, context_lines, cursor, false)
                    .or_else(|| find_unique_line_containing(lines, context, cursor));
                if let Some(context_pos) = context_pos {
                    let repeats_context_as_first_line =
                        hunk.old_lines.first().is_some_and(|first| {
                            normalize_patch_punctuation(first)
                                == normalize_patch_punctuation(context)
                        });
                    cursor = context_pos + usize::from(!repeats_context_as_first_line);
                }
            }
        }

        if hunk.old_lines.is_empty() {
            if hunk.new_lines.is_empty() {
                continue;
            }
            replacements.push((lines.len(), 0, hunk.new_lines.clone()));
            continue;
        }

        let mut old_lines = hunk.old_lines.as_slice();
        let mut new_lines = hunk.new_lines.as_slice();
        let mut pos = seek_sequence(lines, old_lines, cursor, hunk.is_end_of_file).or_else(|| {
            hunk.change_context
                .as_ref()
                .and_then(|_| seek_sequence(lines, old_lines, 0, hunk.is_end_of_file))
        });

        // A final blank context line represents the file's trailing newline,
        // which split_file_lines stores separately from the line vector.
        if pos.is_none() && old_lines.last().is_some_and(String::is_empty) {
            old_lines = &old_lines[..old_lines.len() - 1];
            if new_lines.last().is_some_and(String::is_empty) {
                new_lines = &new_lines[..new_lines.len() - 1];
            }
            pos = seek_sequence(lines, old_lines, cursor, hunk.is_end_of_file);
        }

        if pos.is_none()
            && !hunk.is_end_of_file
            && let Some((reduced_pos, reduced_old, reduced_new)) =
                find_context_reduced_match(lines, old_lines, new_lines, cursor)
        {
            pos = Some(reduced_pos);
            old_lines = reduced_old;
            new_lines = reduced_new;
        }

        let Some(pos) = pos else {
            let applied_start = if hunk.is_end_of_file {
                lines.len().saturating_sub(new_lines.len())
            } else {
                cursor
            };
            if let Some(applied_pos) =
                find_unique_subsequence(lines, new_lines, applied_start, hunk.is_end_of_file)
            {
                cursor = applied_pos + new_lines.len();
                continue;
            }
            return Err(format_hunk_match_error(path, &hunk.old_lines, lines));
        };
        replacements.push((pos, old_lines.len(), new_lines.to_vec()));
        cursor = pos + old_lines.len();
    }

    // Positions refer to the original file. Applying from bottom to top keeps
    // earlier replacements from shifting later coordinates.
    for (pos, old_len, new_lines) in replacements.into_iter().rev() {
        lines.splice(pos..pos + old_len, new_lines);
    }

    Ok(())
}

fn find_context_reduced_match<'a>(
    lines: &[String],
    old_lines: &'a [String],
    new_lines: &'a [String],
    start: usize,
) -> Option<(usize, &'a [String], &'a [String])> {
    let common_prefix = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(old, new)| old == new)
        .count();
    let remaining_old = old_lines.len().saturating_sub(common_prefix);
    let remaining_new = new_lines.len().saturating_sub(common_prefix);
    let common_suffix = old_lines
        .iter()
        .rev()
        .take(remaining_old)
        .zip(new_lines.iter().rev().take(remaining_new))
        .take_while(|(old, new)| old == new)
        .count();

    for total_trim in 1..=common_prefix + common_suffix {
        for leading_trim in 0..=common_prefix.min(total_trim) {
            let trailing_trim = total_trim - leading_trim;
            if trailing_trim > common_suffix
                || leading_trim + trailing_trim >= old_lines.len()
                || leading_trim + trailing_trim > new_lines.len()
            {
                continue;
            }

            let old_end = old_lines.len() - trailing_trim;
            let new_end = new_lines.len() - trailing_trim;
            let reduced_old = &old_lines[leading_trim..old_end];
            let reduced_new = &new_lines[leading_trim..new_end];
            if let Some(pos) = seek_sequence(lines, reduced_old, start, false) {
                return Some((pos, reduced_old, reduced_new));
            }
        }
    }

    None
}

fn seek_sequence(
    lines: &[String],
    needle: &[String],
    start: usize,
    end_of_file: bool,
) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(lines.len()));
    }
    if needle.len() > lines.len() {
        return None;
    }

    let max_start = lines.len() - needle.len();
    let search_start = if end_of_file { max_start } else { start };
    find_subsequence(lines, needle, search_start)
        .or_else(|| {
            find_unique_matching_subsequence(lines, needle, search_start, |actual, expected| {
                patch_line_without_bom(actual).trim_end()
                    == patch_line_without_bom(expected).trim_end()
            })
        })
        .or_else(|| {
            find_unique_matching_subsequence(lines, needle, search_start, |actual, expected| {
                patch_line_without_bom(actual).trim() == patch_line_without_bom(expected).trim()
            })
        })
        .or_else(|| {
            find_unique_matching_subsequence(lines, needle, search_start, |actual, expected| {
                normalize_patch_punctuation(actual) == normalize_patch_punctuation(expected)
            })
        })
}

fn parse_patch_line_hint(context: &str) -> Option<usize> {
    context
        .trim()
        .strip_prefix("line ")
        .unwrap_or(context.trim())
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
}

fn find_unique_line_containing(lines: &[String], context: &str, start: usize) -> Option<usize> {
    let context = patch_line_without_bom(context).trim();
    if context.is_empty() || start >= lines.len() {
        return None;
    }
    let mut matches = lines
        .iter()
        .enumerate()
        .skip(start)
        .filter_map(|(index, line)| {
            patch_line_without_bom(line)
                .trim()
                .contains(context)
                .then_some(index)
        });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[allow(dead_code)]
fn find_subsequence(lines: &[String], needle: &[String], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(lines.len()));
    }
    if needle.len() > lines.len() {
        return None;
    }
    let max_start = lines.len() - needle.len();
    if start > max_start {
        return None;
    }
    (start..=max_start).find(|idx| {
        lines[*idx..*idx + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(a, b)| a == b)
    })
}

fn find_unique_subsequence(
    lines: &[String],
    needle: &[String],
    start: usize,
    end_of_file: bool,
) -> Option<usize> {
    if needle.is_empty() || needle.len() > lines.len() {
        return None;
    }

    let max_start = lines.len() - needle.len();
    let start = if end_of_file { max_start } else { start };
    if start > max_start {
        return None;
    }
    let mut matches = (start..=max_start).filter(|index| {
        lines[*index..*index + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(actual, expected)| actual == expected)
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn find_unique_matching_subsequence(
    lines: &[String],
    needle: &[String],
    start: usize,
    matches_line: impl Fn(&str, &str) -> bool,
) -> Option<usize> {
    if needle.is_empty() || needle.len() > lines.len() {
        return None;
    }

    let max_start = lines.len() - needle.len();
    if start > max_start {
        return None;
    }
    let mut matched = None;
    for index in start..=max_start {
        let is_match = lines[index..index + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(actual, expected)| matches_line(actual, expected));
        if is_match {
            if matched.is_some() {
                return None;
            }
            matched = Some(index);
        }
    }
    matched
}

fn patch_line_without_bom(line: &str) -> &str {
    line.trim_start_matches('\u{feff}')
}

fn normalize_patch_punctuation(line: &str) -> String {
    patch_line_without_bom(line)
        .trim()
        .chars()
        .map(|character| match character {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
            '\u{00a0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200a}' | '\u{202f}' | '\u{205f}'
            | '\u{3000}' => ' ',
            '\u{2502}' => '|',
            other => other,
        })
        .collect()
}

fn relaxed_patch_line(line: &str) -> &str {
    patch_line_without_bom(line).trim_end()
}

fn format_hunk_match_error(path: &str, old_lines: &[String], current_lines: &[String]) -> String {
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

    let current_context = nearest_hunk_context(current_lines, old_lines)
        .map(|context| format!("\nFresh current context:\n{context}"))
        .unwrap_or_default();

    format!(
        "failed to match hunk in {path} ({} expected lines). The file content has changed or the patch context is stale. Use read_file around the fresh context below, then build a new, smaller hunk from the current content. Expected context: {preview}{current_context}",
        old_lines.len()
    )
}

fn nearest_hunk_context(current_lines: &[String], expected_lines: &[String]) -> Option<String> {
    const CONTEXT_BEFORE: usize = 2;
    const CONTEXT_AFTER: usize = 4;
    const MAX_CONTEXT_CHARS: usize = 800;

    let anchor = expected_lines
        .iter()
        .filter(|expected| !relaxed_patch_line(expected).trim().is_empty())
        .find_map(|expected| {
            current_lines
                .iter()
                .position(|actual| relaxed_patch_line(actual) == relaxed_patch_line(expected))
        })?;
    let start = anchor.saturating_sub(CONTEXT_BEFORE);
    let end = (anchor + CONTEXT_AFTER + 1).min(current_lines.len());
    let mut context = current_lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{:>5} | {line}", start + offset + 1))
        .collect::<Vec<_>>()
        .join("\n");
    if context.chars().count() > MAX_CONTEXT_CHARS {
        context = context.chars().take(MAX_CONTEXT_CHARS).collect();
        context.push_str("...");
    }
    Some(context)
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

    fn apply_parsed_update(lines: &mut Vec<String>, patch: &str, path: &str) {
        let actions = parse_patch_actions(patch).unwrap();
        let ParsedPatchAction::Update { hunks, .. } = &actions[0] else {
            panic!("expected update action");
        };
        apply_update_hunks(lines, hunks, path).unwrap();
    }

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
    fn parse_patch_actions_preserves_context_and_end_of_file_markers() {
        let patch = r#"*** Begin Patch
*** Update File: src/App.tsx
@@ function renderApp()
-old
+new
*** End of File
*** End Patch"#;

        let actions = parse_patch_actions(patch).unwrap();
        let ParsedPatchAction::Update { hunks, .. } = &actions[0] else {
            panic!("expected update action");
        };
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].change_context.as_deref(),
            Some("function renderApp()")
        );
        assert!(hunks[0].is_end_of_file);
    }

    #[test]
    fn parse_patch_actions_does_not_treat_unified_ranges_as_source_context() {
        let patch = r#"*** Begin Patch
*** Update File: src/App.tsx
@@ -10,2 +10,2 @@
-old
+new
*** End Patch"#;

        let actions = parse_patch_actions(patch).unwrap();
        let ParsedPatchAction::Update { hunks, .. } = &actions[0] else {
            panic!("expected update action");
        };
        assert_eq!(hunks[0].change_context, None);
    }

    #[test]
    fn parse_patch_actions_accepts_provider_bare_blank_context_lines() {
        let patch = concat!(
            "*** Begin Patch ***\n",
            "*** Update File: D:\\workspace\\scripts\\MainController.gd ***\n",
            "--- a/scripts/MainController.gd\n",
            "+++ b/scripts/MainController.gd\n",
            "@@ -252,7 +252,7 @@ func _on_enemy_turn_ended() -> void:\n",
            "\n",
            " func _on_end_turn_pressed() -> void:\n",
            " \t\"\"\"end turn\"\"\"\n",
            "-\tif battle_manager and battle_manager.state == OLD_STATE:\n",
            "+\tif battle_manager and battle_manager.state == ",
            "BattleManager.BattleState.PLAYER_TURN:\n",
            " \t\tbattle_manager.end_player_turn()\n",
            " \n",
            " \n",
            "*** End Patch ***",
        );

        let actions = parse_patch_actions(patch).unwrap();
        let ParsedPatchAction::Update { hunks, .. } = &actions[0] else {
            panic!("expected update action");
        };
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].change_context.as_deref(),
            Some("func _on_enemy_turn_ended() -> void:")
        );
        assert_eq!(hunks[0].old_lines.first(), Some(&String::new()));
        assert_eq!(hunks[0].old_lines.last(), Some(&String::new()));
    }

    #[test]
    fn apply_patch_accepts_wrapped_markdown_diff_lines_and_end_marker_typo() {
        let mut lines = vec![
            "| `StatusEffect` | `(pending)` | old |".to_string(),
            "| `RewardManager` | `(pending)` | old |".to_string(),
        ];
        let patch = r#"*** Begin Patch
*** Update File: DESIGN.md
@@ status table update
|-| `StatusEffect` | `(pending)` | old |
|-| `RewardManager` | `(pending)` | old |
+|| `StatusEffect` | `StatusEffect.gd` | implemented |
+|| `EffectExecutor` | `EffectExecutor.gd` | implemented |
+|| `RewardManager` | `(pending)` | old |
*** End of Patch"#;

        apply_parsed_update(&mut lines, patch, "DESIGN.md");
        assert_eq!(
            lines,
            vec![
                "| `StatusEffect` | `StatusEffect.gd` | implemented |",
                "| `EffectExecutor` | `EffectExecutor.gd` | implemented |",
                "| `RewardManager` | `(pending)` | old |",
            ]
        );
    }

    #[test]
    fn apply_patch_accepts_line_hint_old_block_new_block_dialect() {
        let mut lines = vec![
            "header".to_string(),
            "| `StatusEffect` | `(pending)` | old |".to_string(),
            "footer".to_string(),
        ];
        let patch = r#"*** Begin Patch
*** Update File: DESIGN.md
@@ line 2
| `StatusEffect` | `(pending)` | old |
@@
| `StatusEffect` | `StatusEffect.gd` | implemented |
| `EffectExecutor` | `EffectExecutor.gd` | implemented |
@@ line 3
footer
@@
footer
*** End Patch"#;

        apply_parsed_update(&mut lines, patch, "DESIGN.md");
        assert_eq!(
            lines,
            vec![
                "header",
                "| `StatusEffect` | `StatusEffect.gd` | implemented |",
                "| `EffectExecutor` | `EffectExecutor.gd` | implemented |",
                "footer",
            ]
        );
    }

    #[test]
    fn apply_patch_unwraps_directory_tree_context_without_changing_tree_glyphs() {
        let mut lines = vec![
            "│   │   ├── StatusEffect.gd      # pending".to_string(),
            "│   │   └── RewardManager.gd     # pending".to_string(),
        ];
        let patch = r#"*** Begin Patch
*** Update File: DESIGN.md
@@ directory tree
||   │   ├── StatusEffect.gd      # pending
||   │   └── RewardManager.gd     # pending
+||   │   ├── StatusEffect.gd      # implemented
+||   │   ├── EffectExecutor.gd    # implemented
+||   │   └── RewardManager.gd     # pending
*** End Patch"#;

        apply_parsed_update(&mut lines, patch, "DESIGN.md");
        assert_eq!(
            lines,
            vec![
                "│   │   ├── StatusEffect.gd      # implemented",
                "│   │   ├── EffectExecutor.gd    # implemented",
                "│   │   └── RewardManager.gd     # pending",
            ]
        );
    }

    #[test]
    fn apply_patch_treats_a_description_context_as_advisory() {
        let mut lines = vec![
            "var effect: Node = null".to_string(),
            String::new(),
            "func get_name() -> String:".to_string(),
            "\treturn \"effect\"".to_string(),
            String::new(),
            "func setup() -> void:".to_string(),
            "\teffect = StatusEffect.new()".to_string(),
        ];
        let patch = r#"*** Begin Patch
*** Update File: test_status_effect.gd
@@ setup
 var effect: Node = null
+const StatusEffectScript = preload("StatusEffect.gd")

 func get_name() -> String:
@@ func setup() -> void:
 func setup() -> void:
-	effect = StatusEffect.new()
+	effect = StatusEffectScript.new()
*** End Patch"#;

        apply_parsed_update(&mut lines, patch, "test_status_effect.gd");
        assert!(
            lines.contains(&"const StatusEffectScript = preload(\"StatusEffect.gd\")".to_string())
        );
        assert!(lines.contains(&"\teffect = StatusEffectScript.new()".to_string()));
    }

    #[test]
    fn apply_patch_does_not_treat_end_of_file_written_in_a_hunk_header_as_source() {
        let mut lines = vec![
            "before".to_string(),
            "old value".to_string(),
            "after".to_string(),
        ];
        let patch = r#"*** Begin Patch
*** Update File: DESIGN.md
@@ *** End of File
-old value
+new value
*** End Patch"#;

        apply_parsed_update(&mut lines, patch, "DESIGN.md");
        assert_eq!(lines, vec!["before", "new value", "after"]);
    }

    #[test]
    fn apply_update_hunks_uses_context_to_select_the_correct_repeated_block() {
        let mut lines = vec![
            "function first()".to_string(),
            "target: old".to_string(),
            "function second()".to_string(),
            "target: old".to_string(),
        ];
        let hunks = vec![PatchHunk {
            change_context: Some("function second()".to_string()),
            old_lines: vec!["target: old".to_string()],
            new_lines: vec!["target: new".to_string()],
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap();
        assert_eq!(lines[1], "target: old");
        assert_eq!(lines[3], "target: new");
    }

    #[test]
    fn apply_update_hunks_accepts_an_anchor_repeated_as_the_first_context_line() {
        let mut lines = vec![
            "func _on_end_turn_pressed() -> void:".to_string(),
            "\t\"\"\"end turn\"\"\"".to_string(),
            "\tif state == OLD_STATE:".to_string(),
            "\t\tend_turn()".to_string(),
        ];
        let hunks = vec![PatchHunk {
            change_context: Some("func _on_end_turn_pressed() -> void:".to_string()),
            old_lines: vec![
                "func _on_end_turn_pressed() -> void:".to_string(),
                "\t\"\"\"end turn\"\"\"".to_string(),
                "\tif state == OLD_STATE:".to_string(),
                "\t\tend_turn()".to_string(),
            ],
            new_lines: vec![
                "func _on_end_turn_pressed() -> void:".to_string(),
                "\t\"\"\"end turn\"\"\"".to_string(),
                "\tif state == BattleState.PLAYER_TURN:".to_string(),
                "\t\tend_turn()".to_string(),
            ],
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        apply_update_hunks(&mut lines, &hunks, "MainController.gd").unwrap();
        assert_eq!(lines[2], "\tif state == BattleState.PLAYER_TURN:");
    }

    #[test]
    fn apply_update_hunks_trims_only_stale_unchanged_edge_context() {
        let mut lines = vec![
            "var max_hand_width: float = 800.0".to_string(),
            String::new(),
            "## Battle state constant".to_string(),
            "const PLAYER_TURN_STATE = 1".to_string(),
            String::new(),
            String::new(),
            "func _ready() -> void:".to_string(),
            "\t# Find BattleManager".to_string(),
            "\tbattle_manager = _find_battle_manager()".to_string(),
        ];
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec![
                "var max_hand_width: float = 800.0".to_string(),
                String::new(),
                "## Battle state constant".to_string(),
                "const PLAYER_TURN_STATE = 1".to_string(),
                String::new(),
                String::new(),
                "func _ready() -> void:".to_string(),
                "\t# Find BattleManager".to_string(),
                "\t_find_battle_manager()".to_string(),
            ],
            new_lines: vec![
                "var max_hand_width: float = 800.0".to_string(),
                String::new(),
                "func _ready() -> void:".to_string(),
                "\t# Find BattleManager".to_string(),
                "\t_find_battle_manager()".to_string(),
            ],
            is_end_of_file: false,
            additions: 0,
            deletions: 4,
        }];

        apply_update_hunks(&mut lines, &hunks, "HandUI.gd").unwrap();
        assert_eq!(
            lines,
            vec![
                "var max_hand_width: float = 800.0",
                "",
                "func _ready() -> void:",
                "\t# Find BattleManager",
                "\tbattle_manager = _find_battle_manager()",
            ]
        );
    }

    #[test]
    fn apply_update_hunks_honors_end_of_file_for_repeated_content() {
        let mut lines = vec!["tail".to_string(), "middle".to_string(), "tail".to_string()];
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec!["tail".to_string()],
            new_lines: vec!["final tail".to_string()],
            is_end_of_file: true,
            additions: 1,
            deletions: 1,
        }];

        apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap();
        assert_eq!(lines, vec!["tail", "middle", "final tail"]);
    }

    #[test]
    fn apply_update_hunks_accepts_unique_trailing_whitespace_and_bom_differences() {
        let mut lines = vec![
            "\u{feff}function parseChart() {   ".to_string(),
            "  return old;\t".to_string(),
            "}".to_string(),
        ];
        let hunks = vec![PatchHunk {
            change_context: None,
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
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap();
        assert_eq!(lines[1], "  return updated;");
    }

    #[test]
    fn apply_update_hunks_accepts_unique_unicode_punctuation_differences() {
        let mut lines = vec!["label = \"old — value\"".to_string()];
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec!["label = \"old - value\"".to_string()],
            new_lines: vec!["label = \"new - value\"".to_string()],
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap();
        assert_eq!(lines, vec!["label = \"new - value\""]);
    }

    #[test]
    fn apply_update_hunks_rejects_ambiguous_relaxed_matches() {
        let mut lines = vec!["same ".to_string(), "same\t".to_string()];
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec!["same".to_string()],
            new_lines: vec!["changed".to_string()],
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        let error = apply_update_hunks(&mut lines, &hunks, "src/App.tsx").unwrap_err();
        assert!(error.contains("build a new, smaller hunk"));
        assert!(error.contains("smaller hunk"));
    }

    #[test]
    fn apply_update_hunks_accepts_a_uniquely_already_applied_hunk() {
        let expected = vec![
            "# Design".to_string(),
            "new architecture".to_string(),
            "stable footer".to_string(),
        ];
        let mut lines = expected.clone();
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec![
                "# Design".to_string(),
                "old architecture".to_string(),
                "stable footer".to_string(),
            ],
            new_lines: expected.clone(),
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        apply_update_hunks(&mut lines, &hunks, "DESIGN.md").unwrap();
        assert_eq!(lines, expected);
    }

    #[test]
    fn apply_update_hunks_still_rejects_a_stale_unapplied_hunk() {
        let mut lines = vec!["# Design".to_string(), "user revision".to_string()];
        let original = lines.clone();
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec!["# Design".to_string(), "old architecture".to_string()],
            new_lines: vec!["# Design".to_string(), "new architecture".to_string()],
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        let error = apply_update_hunks(&mut lines, &hunks, "DESIGN.md").unwrap_err();
        assert!(error.contains("failed to match hunk"));
        assert_eq!(lines, original);
    }

    #[test]
    fn apply_update_hunks_handles_applied_and_pending_hunks_together() {
        let mut lines = vec![
            "section one: new".to_string(),
            "separator".to_string(),
            "section two: old".to_string(),
        ];
        let hunks = vec![
            PatchHunk {
                change_context: None,
                old_lines: vec!["section one: old".to_string()],
                new_lines: vec!["section one: new".to_string()],
                is_end_of_file: false,
                additions: 1,
                deletions: 1,
            },
            PatchHunk {
                change_context: None,
                old_lines: vec!["section two: old".to_string()],
                new_lines: vec!["section two: new".to_string()],
                is_end_of_file: false,
                additions: 1,
                deletions: 1,
            },
        ];

        apply_update_hunks(&mut lines, &hunks, "DESIGN.md").unwrap();
        assert_eq!(
            lines,
            vec![
                "section one: new".to_string(),
                "separator".to_string(),
                "section two: new".to_string(),
            ]
        );
    }

    #[test]
    fn hunk_match_error_includes_fresh_nearby_file_context() {
        let mut lines = vec![
            "  \"settings.provider.activate\": \"启用\",".to_string(),
            "  \"settings.provider.activateThis\": \"设置默认供应商\",".to_string(),
            "  \"settings.provider.addCustom\": \"自定义\",".to_string(),
        ];
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec![
                "  \"settings.provider.activate\": \"启用\",".to_string(),
                "  \"settings.provider.activateThis\": \"启用此供应商\",".to_string(),
                "  \"settings.provider.addCustom\": \"自定义\",".to_string(),
            ],
            new_lines: Vec::new(),
            is_end_of_file: false,
            additions: 0,
            deletions: 1,
        }];

        let error =
            apply_update_hunks(&mut lines, &hunks, "src/i18n/zh-CN/common.json").unwrap_err();

        assert!(error.contains("Expected context"));
        assert!(error.contains("Fresh current context"));
        assert!(error.contains("设置默认供应商"));
        assert!(error.contains("settings.provider.addCustom"));
    }

    #[test]
    fn hunk_match_error_does_not_use_a_blank_line_as_the_fresh_context_anchor() {
        let mut lines = vec![
            "# Design".to_string(),
            String::new(),
            "introduction".to_string(),
            String::new(),
            "## 8. Directory".to_string(),
            String::new(),
            "current tree".to_string(),
        ];
        let hunks = vec![PatchHunk {
            change_context: None,
            old_lines: vec![
                String::new(),
                "## 8. Directory".to_string(),
                String::new(),
                "stale tree".to_string(),
            ],
            new_lines: vec![
                String::new(),
                "## 8. Directory".to_string(),
                String::new(),
                "new tree".to_string(),
            ],
            is_end_of_file: false,
            additions: 1,
            deletions: 1,
        }];

        let error = apply_update_hunks(&mut lines, &hunks, "DESIGN.md").unwrap_err();
        assert!(error.contains("    5 | ## 8. Directory"));
        assert!(error.contains("    7 | current tree"));
        assert!(!error.contains("    1 | # Design"));
    }
}
