use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::git_service::{GitCommandOutput, GitService};
use crate::state::AppState;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResponse {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub is_clean: bool,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub changes: Vec<GitStatusEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffResponse {
    pub text: String,
    pub is_empty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogEntry {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogResponse {
    pub entries: Vec<GitLogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchEntry {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchListResponse {
    pub current: Option<String>,
    pub branches: Vec<GitBranchEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActionResponse {
    pub ok: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

#[tauri::command]
pub async fn git_status(
    state: State<'_, AppState>,
    cwd: Option<String>,
) -> AppResult<GitStatusResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let output = service
        .run(&["status", "--porcelain=1", "-b"], DEFAULT_MAX_OUTPUT_BYTES)
        .await?;
    Ok(parse_git_status_output(&output.stdout))
}

#[tauri::command]
pub async fn git_diff(
    state: State<'_, AppState>,
    cwd: Option<String>,
    path: Option<String>,
    staged: Option<bool>,
) -> AppResult<GitDiffResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let mut args = vec!["diff".to_string(), "--no-color".to_string()];
    if staged.unwrap_or(false) {
        args.push("--cached".to_string());
    }
    if let Some(path) = path
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        args.push("--".to_string());
        args.push(path);
    }

    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    let text = output.stdout;
    Ok(GitDiffResponse {
        is_empty: text.trim().is_empty(),
        text,
    })
}

#[tauri::command]
pub async fn git_log(
    state: State<'_, AppState>,
    cwd: Option<String>,
    limit: Option<u32>,
) -> AppResult<GitLogResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let limit = limit.unwrap_or(30).clamp(1, 200);
    let args = vec![
        "log".to_string(),
        format!("-n{limit}"),
        "--date=iso-strict".to_string(),
        "--pretty=format:%H%x1f%h%x1f%an%x1f%ad%x1f%s".to_string(),
    ];
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(GitLogResponse {
        entries: parse_git_log_output(&output.stdout),
    })
}

#[tauri::command]
pub async fn git_branch_list(
    state: State<'_, AppState>,
    cwd: Option<String>,
) -> AppResult<GitBranchListResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let args = [
        "branch",
        "--format=%(refname:short)|%(HEAD)|%(upstream:short)",
    ];
    let output = service.run(&args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(parse_git_branch_list_output(&output.stdout))
}

#[tauri::command]
pub async fn git_stage(
    state: State<'_, AppState>,
    cwd: Option<String>,
    paths: Vec<String>,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let normalized_paths = normalize_paths(paths)?;
    let mut args = vec!["add".to_string(), "--".to_string()];
    args.extend(normalized_paths);
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(action_ok("Files staged successfully.", output))
}

#[tauri::command]
pub async fn git_unstage(
    state: State<'_, AppState>,
    cwd: Option<String>,
    paths: Vec<String>,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let normalized_paths = normalize_paths(paths)?;
    let mut args = vec![
        "restore".to_string(),
        "--staged".to_string(),
        "--".to_string(),
    ];
    args.extend(normalized_paths);
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(action_ok("Files unstaged successfully.", output))
}

#[tauri::command]
pub async fn git_commit(
    state: State<'_, AppState>,
    cwd: Option<String>,
    message: String,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let commit_message = message.trim();
    if commit_message.is_empty() {
        return Err(AppError::Custom(
            "Commit message cannot be empty.".to_string(),
        ));
    }

    // 先检查是否存在暂存变更，避免触发 Git 的空提交报错后再回显。
    let staged_check = service
        .run_allow_failure(&["diff", "--cached", "--quiet"], 8 * 1024)
        .await?;
    match staged_check.exit_code {
        0 => {
            return Err(AppError::Custom(
                "No staged changes found. Stage files before committing.".to_string(),
            ));
        }
        1 => {}
        _ => {
            return Err(AppError::Custom(format!(
                "Failed to inspect staged changes: {}",
                staged_check.stderr.trim()
            )));
        }
    }

    let output = service
        .run(&["commit", "-m", commit_message], DEFAULT_MAX_OUTPUT_BYTES)
        .await?;
    Ok(action_ok("Commit created successfully.", output))
}

#[tauri::command]
pub async fn git_checkout(
    state: State<'_, AppState>,
    cwd: Option<String>,
    branch: String,
    create: Option<bool>,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let branch = normalize_branch_name(&branch)?;
    let output = if create.unwrap_or(false) {
        service
            .run(&["checkout", "-b", &branch], DEFAULT_MAX_OUTPUT_BYTES)
            .await?
    } else {
        service
            .run(&["checkout", &branch], DEFAULT_MAX_OUTPUT_BYTES)
            .await?
    };
    Ok(action_ok("Branch checkout completed.", output))
}

#[tauri::command]
pub async fn git_pull(
    state: State<'_, AppState>,
    cwd: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
    rebase: Option<bool>,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let mut args = vec!["pull".to_string()];
    if rebase.unwrap_or(false) {
        args.push("--rebase".to_string());
    }
    if let Some(remote) = remote
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        args.push(remote);
    }
    if let Some(branch) = branch
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        args.push(branch);
    }
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(action_ok("Pull completed.", output))
}

#[tauri::command]
pub async fn git_push(
    state: State<'_, AppState>,
    cwd: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
    set_upstream: Option<bool>,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let mut args = vec!["push".to_string()];
    if set_upstream.unwrap_or(false) {
        args.push("--set-upstream".to_string());
    }
    if let Some(remote) = remote
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        args.push(remote);
    }
    if let Some(branch) = branch
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        args.push(branch);
    }
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(action_ok("Push completed.", output))
}

#[tauri::command]
pub async fn git_reset(
    state: State<'_, AppState>,
    cwd: Option<String>,
    mode: String,
    target: Option<String>,
    confirm_dangerous: bool,
) -> AppResult<GitActionResponse> {
    let service = git_service_from_state(&state, cwd).await?;
    let mode = normalize_reset_mode(&mode)?;
    if mode == "hard" && !confirm_dangerous {
        return Err(AppError::Custom(
            "Hard reset requires confirmDangerous=true.".to_string(),
        ));
    }
    let target_ref = target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    let output = service
        .run(
            &["reset", &format!("--{mode}"), target_ref],
            DEFAULT_MAX_OUTPUT_BYTES,
        )
        .await?;
    Ok(action_ok("Reset completed.", output))
}

#[tauri::command]
pub async fn git_revert(
    state: State<'_, AppState>,
    cwd: Option<String>,
    commit: String,
    no_edit: Option<bool>,
    confirm_dangerous: bool,
) -> AppResult<GitActionResponse> {
    if !confirm_dangerous {
        return Err(AppError::Custom(
            "Revert requires confirmDangerous=true.".to_string(),
        ));
    }

    let service = git_service_from_state(&state, cwd).await?;
    let commit = normalize_commit_ref(&commit)?;
    let mut args = vec!["revert".to_string()];
    if no_edit.unwrap_or(true) {
        args.push("--no-edit".to_string());
    }
    args.push(commit);
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(action_ok("Revert completed.", output))
}

#[tauri::command]
pub async fn git_cherry_pick(
    state: State<'_, AppState>,
    cwd: Option<String>,
    commit: String,
    no_commit: Option<bool>,
    confirm_dangerous: bool,
) -> AppResult<GitActionResponse> {
    if !confirm_dangerous {
        return Err(AppError::Custom(
            "Cherry-pick requires confirmDangerous=true.".to_string(),
        ));
    }

    let service = git_service_from_state(&state, cwd).await?;
    let commit = normalize_commit_ref(&commit)?;
    let mut args = vec!["cherry-pick".to_string()];
    if no_commit.unwrap_or(false) {
        args.push("--no-commit".to_string());
    }
    args.push(commit);
    let output = run_git_vec(&service, &args, DEFAULT_MAX_OUTPUT_BYTES).await?;
    Ok(action_ok("Cherry-pick completed.", output))
}

async fn git_service_from_state(
    state: &State<'_, AppState>,
    cwd: Option<String>,
) -> AppResult<GitService> {
    let default_cwd = state.cwd.read().await.clone();
    let resolved = GitService::resolve_cwd(Path::new(&default_cwd), cwd.as_deref())?;
    let service = GitService::new(resolved);
    service.ensure_repository().await?;
    Ok(service)
}

async fn run_git_vec(
    service: &GitService,
    args: &[String],
    max_output_bytes: usize,
) -> AppResult<GitCommandOutput> {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    service.run(&refs, max_output_bytes).await
}

fn normalize_paths(paths: Vec<String>) -> AppResult<Vec<String>> {
    let mut normalized = Vec::new();
    for path in paths {
        let value = path.trim();
        if value.is_empty() {
            continue;
        }
        if value.contains('\0') || value.contains('\n') || value.contains('\r') {
            return Err(AppError::Custom(format!("Invalid git path value: {value}")));
        }
        normalized.push(value.replace('\\', "/"));
    }
    if normalized.is_empty() {
        return Err(AppError::Custom(
            "At least one path is required for this operation.".to_string(),
        ));
    }
    Ok(normalized)
}

fn normalize_branch_name(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Custom("Branch name cannot be empty.".to_string()));
    }
    if trimmed.contains(' ') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(AppError::Custom(format!("Invalid branch name: {trimmed}")));
    }
    Ok(trimmed.to_string())
}

fn normalize_commit_ref(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Custom(
            "Commit reference cannot be empty.".to_string(),
        ));
    }
    if trimmed.contains(' ') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(AppError::Custom(format!(
            "Invalid commit reference: {trimmed}"
        )));
    }
    Ok(trimmed.to_string())
}

fn normalize_reset_mode(mode: &str) -> AppResult<String> {
    let normalized = mode.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "soft" | "mixed" | "hard" => Ok(normalized),
        _ => Err(AppError::Custom(format!(
            "Unsupported reset mode: {mode}. Use soft/mixed/hard."
        ))),
    }
}

fn action_ok(message: &str, output: GitCommandOutput) -> GitActionResponse {
    GitActionResponse {
        ok: true,
        message: message.to_string(),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

#[derive(Debug, Clone, Default)]
struct ParsedBranchHeader {
    branch: String,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
}

fn parse_git_status_output(stdout: &str) -> GitStatusResponse {
    let mut header = ParsedBranchHeader {
        branch: "HEAD".to_string(),
        ..ParsedBranchHeader::default()
    };
    let mut changes = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("## ") {
            header = parse_branch_header(trimmed);
            continue;
        }
        if let Some(entry) = parse_status_entry(trimmed) {
            changes.push(entry);
        }
    }

    let staged_count = changes.iter().filter(|entry| entry.staged).count();
    let unstaged_count = changes.iter().filter(|entry| entry.unstaged).count();
    let untracked_count = changes.iter().filter(|entry| entry.untracked).count();

    GitStatusResponse {
        branch: header.branch,
        upstream: header.upstream,
        ahead: header.ahead,
        behind: header.behind,
        is_clean: changes.is_empty(),
        staged_count,
        unstaged_count,
        untracked_count,
        changes,
    }
}

fn parse_branch_header(line: &str) -> ParsedBranchHeader {
    let mut parsed = ParsedBranchHeader {
        branch: "HEAD".to_string(),
        ..ParsedBranchHeader::default()
    };
    let raw = line.strip_prefix("## ").unwrap_or(line).trim();
    if raw.is_empty() {
        return parsed;
    }

    if let Some(branch) = raw.strip_prefix("No commits yet on ") {
        parsed.branch = branch.trim().to_string();
        return parsed;
    }

    if raw.starts_with("HEAD (") {
        parsed.branch = "HEAD".to_string();
        return parsed;
    }

    let (core, tracking_raw) = if let Some((left, right)) = raw.split_once(" [") {
        (left.trim(), Some(right.trim_end_matches(']').trim()))
    } else {
        (raw, None)
    };

    if let Some((branch, upstream)) = core.split_once("...") {
        parsed.branch = branch.trim().to_string();
        let upstream_value = upstream.trim();
        if !upstream_value.is_empty() {
            parsed.upstream = Some(upstream_value.to_string());
        }
    } else {
        parsed.branch = core.to_string();
    }

    if let Some(tracking_raw) = tracking_raw {
        for token in tracking_raw.split(',') {
            let item = token.trim();
            if let Some(value) = item.strip_prefix("ahead ") {
                parsed.ahead = value.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(value) = item.strip_prefix("behind ") {
                parsed.behind = value.trim().parse::<u32>().unwrap_or(0);
            } else if item == "gone" {
                parsed.upstream = None;
            }
        }
    }

    if parsed.branch.is_empty() {
        parsed.branch = "HEAD".to_string();
    }
    parsed
}

fn parse_status_entry(line: &str) -> Option<GitStatusEntry> {
    if line.len() < 4 {
        return None;
    }

    let bytes = line.as_bytes();
    let x = bytes.first().copied().unwrap_or(b' ') as char;
    let y = bytes.get(1).copied().unwrap_or(b' ') as char;
    let rest = line.get(3..)?.trim();
    if rest.is_empty() {
        return None;
    }

    let (old_path, path) = if let Some((old_path, new_path)) = rest.split_once(" -> ") {
        (Some(clean_git_path(old_path)), clean_git_path(new_path))
    } else {
        (None, clean_git_path(rest))
    };

    let untracked = x == '?' && y == '?';
    let staged = x != ' ' && x != '?';
    let unstaged = y != ' ' && y != '?';
    let status = status_label(x, y, untracked);

    Some(GitStatusEntry {
        path,
        old_path,
        status,
        staged,
        unstaged,
        untracked,
    })
}

fn clean_git_path(raw: &str) -> String {
    raw.trim().trim_matches('"').replace('\\', "/")
}

fn status_label(x: char, y: char, untracked: bool) -> String {
    if untracked {
        return "untracked".to_string();
    }
    if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        return "conflicted".to_string();
    }
    if x == 'R' || y == 'R' {
        return "renamed".to_string();
    }
    if x == 'D' || y == 'D' {
        return "deleted".to_string();
    }
    if x == 'A' || y == 'A' {
        return "added".to_string();
    }
    if x == 'C' || y == 'C' {
        return "copied".to_string();
    }
    "modified".to_string()
}

fn parse_git_log_output(stdout: &str) -> Vec<GitLogEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\u{001f}');
            let hash = parts.next()?.trim().to_string();
            let short_hash = parts.next()?.trim().to_string();
            let author = parts.next()?.trim().to_string();
            let date = parts.next()?.trim().to_string();
            let message = parts.next()?.trim().to_string();
            Some(GitLogEntry {
                hash,
                short_hash,
                author,
                date,
                message,
            })
        })
        .collect()
}

fn parse_git_branch_list_output(stdout: &str) -> GitBranchListResponse {
    let mut current = None;
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let raw = line.trim();
        if raw.is_empty() {
            continue;
        }
        let mut parts = raw.split('|');
        let name = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let head_marker = parts.next().unwrap_or("").trim();
        let upstream = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let is_current = head_marker == "*";
        if is_current {
            current = Some(name.clone());
        }
        branches.push(GitBranchEntry {
            name,
            current: is_current,
            upstream,
        });
    }

    GitBranchListResponse { current, branches }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_branch_header_supports_ahead_behind() {
        let parsed = parse_branch_header("## main...origin/main [ahead 2, behind 1]");
        assert_eq!(parsed.branch, "main");
        assert_eq!(parsed.upstream.as_deref(), Some("origin/main"));
        assert_eq!(parsed.ahead, 2);
        assert_eq!(parsed.behind, 1);
    }

    #[test]
    fn parse_git_status_output_extracts_rename_and_untracked() {
        let output = "## feature/demo...origin/feature/demo [ahead 1]\nR  old/name.ts -> src/new_name.ts\n?? README.tmp\n";
        let parsed = parse_git_status_output(output);
        assert_eq!(parsed.branch, "feature/demo");
        assert_eq!(parsed.ahead, 1);
        assert_eq!(parsed.changes.len(), 2);
        assert_eq!(parsed.changes[0].status, "renamed");
        assert_eq!(parsed.changes[0].old_path.as_deref(), Some("old/name.ts"));
        assert_eq!(parsed.changes[1].status, "untracked");
        assert!(parsed.changes[1].untracked);
    }

    #[test]
    fn normalize_paths_rejects_empty_input() {
        let result = normalize_paths(vec!["   ".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_git_log_output_reads_delimited_fields() {
        let output = "abc\u{001f}abc\u{001f}dev\u{001f}2026-06-17T09:00:00+08:00\u{001f}feat: test";
        let entries = parse_git_log_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hash, "abc");
        assert_eq!(entries[0].author, "dev");
        assert_eq!(entries[0].message, "feat: test");
    }

    #[test]
    fn parse_git_branch_list_marks_current_branch() {
        let output = "main|*|origin/main\nfeature/a||origin/feature/a\n";
        let parsed = parse_git_branch_list_output(output);
        assert_eq!(parsed.current.as_deref(), Some("main"));
        assert_eq!(parsed.branches.len(), 2);
        assert!(parsed.branches[0].current);
        assert!(!parsed.branches[1].current);
    }
}
