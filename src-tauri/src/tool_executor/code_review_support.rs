use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::Deserialize;

use crate::git_service::GitCommandOutput;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct CodeReviewArgs {
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    #[serde(default)]
    pub(crate) paths: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) max_diff_bytes: Option<usize>,
    #[serde(default)]
    pub(crate) include_untracked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewFinding {
    pub(crate) priority: &'static str,
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) title: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CodeReviewSummary {
    pub(crate) files_changed: usize,
    pub(crate) additions: u64,
    pub(crate) deletions: u64,
    pub(crate) findings: Vec<ReviewFinding>,
}

pub(crate) fn code_review_scope_label(args: &CodeReviewArgs) -> String {
    let base = args
        .base_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    let path_count = args.paths.as_ref().map(Vec::len).unwrap_or(0);
    if path_count == 0 {
        format!("working tree vs {base}")
    } else {
        format!("working tree vs {base} ({path_count} paths)")
    }
}

pub(crate) fn validate_code_review_base_ref(input: Option<&str>) -> Result<Option<String>, String> {
    let Some(input) = input.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if input.starts_with('-') || input.chars().any(char::is_control) {
        return Err("Error: invalid code_review base_ref".to_string());
    }
    Ok(Some(input.to_string()))
}

pub(crate) fn validate_code_review_paths(paths: &[String]) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
            return Err(format!("Error: invalid code_review path: {trimmed}"));
        }
        let parsed = Path::new(trimmed);
        if parsed.is_absolute() {
            return Err(format!(
                "Error: code_review path filters must be relative: {trimmed}"
            ));
        }
        if parsed
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(format!(
                "Error: code_review path filters must not contain '..': {trimmed}"
            ));
        }
        output.push(trimmed.replace('\\', "/"));
    }
    output.sort();
    output.dedup();
    Ok(output)
}

pub(crate) fn code_review_git_args(
    mode: &str,
    base_ref: Option<&str>,
    paths: &[String],
) -> Vec<String> {
    let mut args = vec![
        "diff".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
    ];
    match mode {
        "numstat" => args.push("--numstat".to_string()),
        "name-status" => args.push("--name-status".to_string()),
        "check" => args.push("--check".to_string()),
        _ => args.push("--unified=0".to_string()),
    }
    args.push(base_ref.unwrap_or("HEAD").to_string());
    args.push("--".to_string());
    args.extend(paths.iter().cloned());
    args
}

pub(crate) fn code_review_untracked_paths(status: &str, filters: &[String]) -> Vec<String> {
    status
        .lines()
        .filter_map(|line| line.strip_prefix("?? "))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| path.replace('\\', "/"))
        .filter(|path| path_matches_filters(path, filters))
        .take(50)
        .collect()
}

pub(crate) fn analyze_code_review_diff(
    diff: &str,
    numstat: &str,
    diff_check: &GitCommandOutput,
    untracked: &[String],
    diff_truncated: bool,
) -> CodeReviewSummary {
    let mut summary = parse_code_review_numstat(numstat);
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();

    if diff_truncated {
        push_review_finding(
            &mut findings,
            &mut seen,
            ReviewFinding {
                priority: "P2",
                path: ".".to_string(),
                line: None,
                title: "Diff was truncated before review completed".to_string(),
                detail:
                    "Increase max_diff_bytes or review a narrower path set for stronger coverage."
                        .to_string(),
            },
        );
    }

    let diff_check_output = combine_stdout_stderr(&diff_check.stdout, &diff_check.stderr);
    if diff_check.exit_code != 0 && !diff_check_output.is_empty() {
        push_review_finding(
            &mut findings,
            &mut seen,
            ReviewFinding {
                priority: "P2",
                path: ".".to_string(),
                line: None,
                title: "git diff --check reported whitespace or conflict-marker issues".to_string(),
                detail: first_line(&diff_check_output)
                    .unwrap_or("git diff --check failed")
                    .to_string(),
            },
        );
    }

    if !untracked.is_empty() {
        push_review_finding(
            &mut findings,
            &mut seen,
            ReviewFinding {
                priority: "P3",
                path: untracked[0].clone(),
                line: None,
                title: "Untracked files are outside diff content review".to_string(),
                detail: format!(
                    "{} untracked path(s) were visible in git status; add them or narrow paths before relying on this review.",
                    untracked.len()
                ),
            },
        );
    }

    let changed_paths = code_review_paths_from_numstat(numstat);
    if changed_paths.iter().any(|path| is_source_path(path))
        && !changed_paths.iter().any(|path| is_test_path(path))
    {
        push_review_finding(
            &mut findings,
            &mut seen,
            ReviewFinding {
                priority: "P2",
                path: ".".to_string(),
                line: None,
                title: "Source changed without matching test changes".to_string(),
                detail: "No changed path looked like a test file or test fixture. Verify behavior with existing tests or add focused coverage.".to_string(),
            },
        );
    }

    for finding in review_findings_from_added_lines(diff) {
        push_review_finding(&mut findings, &mut seen, finding);
        if findings.len() >= 40 {
            break;
        }
    }

    findings.sort_by_key(|finding| review_priority_rank(finding.priority));
    summary.findings = findings;
    summary
}

fn parse_code_review_numstat(numstat: &str) -> CodeReviewSummary {
    let mut summary = CodeReviewSummary::default();
    for line in numstat.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 3 {
            continue;
        }
        summary.files_changed += 1;
        summary.additions += parts[0].parse::<u64>().unwrap_or(0);
        summary.deletions += parts[1].parse::<u64>().unwrap_or(0);
    }
    summary
}

fn code_review_paths_from_numstat(numstat: &str) -> Vec<String> {
    numstat
        .lines()
        .filter_map(|line| line.split('\t').next_back())
        .map(|path| path.replace('\\', "/"))
        .collect()
}

fn review_findings_from_added_lines(diff: &str) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    let mut path = String::new();
    let mut new_line = 0u32;

    for line in diff.lines() {
        if let Some(next_path) = line.strip_prefix("+++ ") {
            path = normalize_diff_path(next_path);
            continue;
        }
        if line.starts_with("@@") {
            new_line = parse_hunk_new_start(line).unwrap_or(0);
            continue;
        }
        if path.is_empty() || path == "/dev/null" {
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            let content = &line[1..];
            findings.extend(review_findings_for_added_line(&path, new_line, content));
            new_line = new_line.saturating_add(1);
        } else if line.starts_with(' ') {
            new_line = new_line.saturating_add(1);
        }
    }

    findings
}

fn normalize_diff_path(path: &str) -> String {
    let trimmed = path.trim();
    trimmed
        .strip_prefix("b/")
        .unwrap_or(trimmed)
        .replace('\\', "/")
}

fn parse_hunk_new_start(line: &str) -> Option<u32> {
    let plus = line.find(" +")? + 2;
    let segment = line[plus..].split_whitespace().next()?;
    segment
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse::<u32>()
        .ok()
}

fn review_findings_for_added_line(path: &str, line: u32, content: &str) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    let lower = content.to_ascii_lowercase();
    if contains_private_key_marker(content) || contains_secret_like_value(content) {
        findings.push(ReviewFinding {
            priority: "P1",
            path: path.to_string(),
            line: Some(line),
            title: "Secret-looking value added".to_string(),
            detail: "The added line looks like it may contain a token, password, key, or private material. Move secrets to configuration or a secret store.".to_string(),
        });
    }
    if lower.contains("dangerouslysetinnerhtml") || lower.contains("eval(") {
        findings.push(ReviewFinding {
            priority: "P2",
            path: path.to_string(),
            line: Some(line),
            title: "Risky dynamic execution or HTML injection API".to_string(),
            detail: "Review input sanitization and trust boundaries before shipping this path."
                .to_string(),
        });
    }
    if is_rust_path(path) && (content.contains(".unwrap()") || content.contains(".expect(")) {
        findings.push(ReviewFinding {
            priority: "P3",
            path: path.to_string(),
            line: Some(line),
            title: "New Rust panic path".to_string(),
            detail: "Consider returning a typed error or adding context that proves this cannot panic in normal use.".to_string(),
        });
    }
    if is_rust_path(path) && (content.contains("todo!()") || content.contains("unimplemented!()")) {
        findings.push(ReviewFinding {
            priority: "P2",
            path: path.to_string(),
            line: Some(line),
            title: "Placeholder panic macro added".to_string(),
            detail: "todo!() and unimplemented!() panic at runtime if reached.".to_string(),
        });
    }
    if is_script_path(path) && (lower.contains("console.log(") || lower.trim() == "debugger;") {
        findings.push(ReviewFinding {
            priority: "P3",
            path: path.to_string(),
            line: Some(line),
            title: "Debug statement added".to_string(),
            detail: "Remove temporary logging or gate it behind the app's logging system before release.".to_string(),
        });
    }
    if lower.contains("http://")
        && !lower.contains("http://localhost")
        && !lower.contains("http://127.0.0.1")
    {
        findings.push(ReviewFinding {
            priority: "P3",
            path: path.to_string(),
            line: Some(line),
            title: "Plain HTTP URL added".to_string(),
            detail: "Use HTTPS for external network calls unless this is intentionally local or test-only.".to_string(),
        });
    }
    findings
}

fn contains_private_key_marker(content: &str) -> bool {
    content.contains("BEGIN PRIVATE KEY")
        || content.contains("BEGIN RSA PRIVATE KEY")
        || content.contains("BEGIN OPENSSH PRIVATE KEY")
}

fn contains_secret_like_value(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_secret_name = [
        "api_key",
        "apikey",
        "secret",
        "password",
        "passwd",
        "private_key",
        "access_token",
        "refresh_token",
        "bearer",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !has_secret_name || lower.contains("example") || lower.contains("placeholder") {
        return false;
    }
    if !(content.contains('=') || content.contains(':')) {
        return false;
    }
    content
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' && ch != '.')
        .any(|token| token.len() >= 16 && token.chars().any(|ch| ch.is_ascii_digit()))
}

fn push_review_finding(
    findings: &mut Vec<ReviewFinding>,
    seen: &mut BTreeSet<String>,
    finding: ReviewFinding,
) {
    let key = format!(
        "{}\0{}\0{:?}\0{}",
        finding.priority, finding.path, finding.line, finding.title
    );
    if seen.insert(key) {
        findings.push(finding);
    }
}

fn review_priority_rank(priority: &str) -> u8 {
    match priority {
        "P1" => 0,
        "P2" => 1,
        "P3" => 2,
        _ => 3,
    }
}

fn is_source_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if is_test_path(&lower)
        || lower.starts_with("docs/")
        || lower.ends_with(".md")
        || lower.ends_with(".lock")
        || lower.contains("/generated/")
    {
        return false;
    }
    [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".kt", ".swift", ".c", ".cc",
        ".cpp", ".h", ".hpp", ".cs",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_test.go")
}

fn is_rust_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".rs")
}

fn is_script_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
}

fn path_matches_filters(path: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    filters.iter().any(|filter| {
        let filter = filter.trim_matches('/');
        path == filter || path.starts_with(&format!("{filter}/"))
    })
}

fn first_line(value: &str) -> Option<&str> {
    value.lines().find(|line| !line.trim().is_empty())
}

pub(crate) fn format_code_review_output(
    scope: &str,
    summary: &CodeReviewSummary,
    name_status: &str,
    status: &str,
    untracked: &[String],
    diff_check: &GitCommandOutput,
) -> String {
    let mut output = String::new();
    output.push_str("Code review report\n");
    output.push_str(&format!("Scope: {scope}\n"));
    output.push_str(&format!(
        "Changed files: {}\nAdditions: {}\nDeletions: {}\n",
        summary.files_changed, summary.additions, summary.deletions
    ));

    let changed_files = code_review_changed_files(name_status);
    if !changed_files.is_empty() {
        output.push_str("\nFiles:\n");
        for file in changed_files.iter().take(25) {
            output.push_str("- ");
            output.push_str(file);
            output.push('\n');
        }
        if changed_files.len() > 25 {
            output.push_str(&format!("- ... {} more\n", changed_files.len() - 25));
        }
    }

    output.push_str("\nFindings:\n");
    if summary.findings.is_empty() {
        output.push_str("- No automated findings. This does not replace a human or model-assisted review for behavioral correctness.\n");
    } else {
        for finding in &summary.findings {
            output.push_str(&format!(
                "- [{}] {}: {} - {}\n",
                finding.priority,
                review_finding_location(finding),
                finding.title,
                finding.detail
            ));
        }
    }

    let diff_check_output = combine_stdout_stderr(&diff_check.stdout, &diff_check.stderr);
    output.push_str("\nChecks:\n");
    if diff_check.exit_code == 0 {
        output.push_str("- git diff --check: passed\n");
    } else if diff_check_output.is_empty() {
        output.push_str("- git diff --check: failed\n");
    } else {
        output.push_str("- git diff --check: reported issues\n");
    }
    if !untracked.is_empty() {
        output.push_str(&format!("- untracked paths visible: {}\n", untracked.len()));
    }
    if !status.trim().is_empty() {
        output.push_str("- git status --short had entries\n");
    }

    output
}

fn code_review_changed_files(name_status: &str) -> Vec<String> {
    name_status
        .lines()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() >= 2 {
                Some(parts[1..].join(" -> "))
            } else {
                None
            }
        })
        .collect()
}

fn review_finding_location(finding: &ReviewFinding) -> String {
    match finding.line {
        Some(line) => format!("{}:{line}", finding.path),
        None => finding.path.clone(),
    }
}

fn combine_stdout_stderr(stdout: &str, stderr: &str) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.trim().to_string(),
        (true, false) => stderr.trim().to_string(),
        (false, false) => format!("{}\n[stderr]\n{}", stdout.trim(), stderr.trim()),
    }
}
