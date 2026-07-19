use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

#[cfg(windows)]
use std::io::Write;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::truncate_output;

const CODE_SEARCH_TIMEOUT_DEFAULT_MS: u64 = 30_000;
const CODE_SEARCH_TIMEOUT_MIN_MS: u64 = 1_000;
const CODE_SEARCH_TIMEOUT_MAX_MS: u64 = 120_000;
const CODE_SEARCH_HEAD_LIMIT_DEFAULT: usize = 50;
const CODE_SEARCH_HEAD_LIMIT_MAX: usize = 200;
const CODE_SEARCH_CONTEXT_MAX: usize = 5;
const CODE_SEARCH_OUTPUT_MAX_CHARS: usize = 8_000;

#[cfg(windows)]
const RG_BINARY_NAME: &str = "rg.exe";
#[cfg(windows)]
const EMBEDDED_RG_BYTES: &[u8] = include_bytes!("../../resources/rg/rg.exe");

static EMBEDDED_RG_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct CodeSearchArgs {
    pub(crate) pattern: String,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) glob: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) case_sensitive: Option<bool>,
    #[serde(default)]
    pub(crate) fixed_strings: Option<bool>,
    #[serde(default)]
    pub(crate) context: Option<usize>,
    #[serde(default)]
    pub(crate) head_limit: Option<usize>,
    #[serde(default)]
    pub(crate) timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeSearchCommand {
    pub(crate) rg_path: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) search_root: PathBuf,
    pub(crate) timeout_ms: u64,
    pub(crate) head_limit: usize,
}

pub(crate) fn resolve_rg_binary(_project_root: &Path) -> Result<PathBuf, String> {
    EMBEDDED_RG_PATH.get_or_init(extract_embedded_rg).clone()
}

#[cfg(windows)]
fn extract_embedded_rg() -> Result<PathBuf, String> {
    let digest = Sha256::digest(EMBEDDED_RG_BYTES);
    let digest_hex = hex::encode(digest);
    let cache_root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("CN-Codex")
        .join("bin")
        .join(&digest_hex[..16]);
    let target = cache_root.join(RG_BINARY_NAME);

    if embedded_rg_file_matches(&target, &digest_hex)? {
        return Ok(target);
    }

    std::fs::create_dir_all(&cache_root)
        .map_err(|e| format!("Failed to create embedded rg cache directory: {e}"))?;
    let temp = cache_root.join(format!(
        "{RG_BINARY_NAME}.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    if let Err(error) = write_embedded_rg_temp(&temp) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    if embedded_rg_file_matches(&target, &digest_hex)? {
        let _ = std::fs::remove_file(&temp);
        return Ok(target);
    }

    if target.exists() {
        match std::fs::remove_file(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                return Err(format!("Failed to remove stale embedded rg: {error}"));
            }
        }
    }

    match std::fs::rename(&temp, &target) {
        Ok(()) => Ok(target),
        Err(_) if embedded_rg_file_matches(&target, &digest_hex)? => {
            let _ = std::fs::remove_file(&temp);
            Ok(target)
        }
        Err(rename_error) => {
            let _ = std::fs::remove_file(&temp);
            Err(format!(
                "Failed to finalize embedded rg extraction: {rename_error}"
            ))
        }
    }
}

#[cfg(windows)]
fn write_embedded_rg_temp(path: &Path) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Failed to create embedded rg temporary file: {e}"))?;
    file.write_all(EMBEDDED_RG_BYTES)
        .map_err(|e| format!("Failed to extract embedded rg: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Failed to flush embedded rg: {e}"))
}

#[cfg(windows)]
fn embedded_rg_file_matches(path: &Path, expected_digest: &str) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let bytes =
        std::fs::read(path).map_err(|e| format!("Failed to inspect cached embedded rg: {e}"))?;
    Ok(hex::encode(Sha256::digest(bytes)) == expected_digest)
}

#[cfg(not(windows))]
fn extract_embedded_rg() -> Result<PathBuf, String> {
    Err("Embedded code_search is currently available only on Windows x64 builds.".to_string())
}

pub(crate) fn resolve_search_path(
    workspace_root: &Path,
    input: Option<&str>,
) -> Result<PathBuf, String> {
    let trimmed = input.map(str::trim).unwrap_or("").replace('\\', "/");
    if trimmed.is_empty() || trimmed == "." {
        return Ok(workspace_root.to_path_buf());
    }

    let raw = Path::new(&trimmed);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        let mut path = workspace_root.to_path_buf();
        for component in raw.components() {
            match component {
                Component::Normal(part) => path.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err("Error: code_search path must not contain '..'".to_string());
                }
                _ => {
                    return Err("Error: invalid code_search path component".to_string());
                }
            }
        }
        path
    };

    let workspace_canon = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let candidate_canon = candidate
        .canonicalize()
        .map_err(|e| format!("Error: code_search path does not exist: {trimmed} ({e})"))?;

    if !candidate_canon.starts_with(&workspace_canon) {
        return Err(
            "Error: code_search path must resolve inside the current workspace".to_string(),
        );
    }

    Ok(candidate_canon)
}

pub(crate) fn build_code_search_command(
    workspace_root: &Path,
    args: &CodeSearchArgs,
) -> Result<CodeSearchCommand, String> {
    let pattern = args.pattern.trim();
    if pattern.is_empty() {
        return Err("Error: code_search pattern must not be empty".to_string());
    }
    if pattern.len() > 2_000 {
        return Err("Error: code_search pattern is too long (max 2000 chars)".to_string());
    }

    let rg_path = resolve_rg_binary(workspace_root)?;
    let search_root = resolve_search_path(workspace_root, args.path.as_deref())?;
    let head_limit = args
        .head_limit
        .unwrap_or(CODE_SEARCH_HEAD_LIMIT_DEFAULT)
        .clamp(1, CODE_SEARCH_HEAD_LIMIT_MAX);
    let context = args.context.unwrap_or(0).min(CODE_SEARCH_CONTEXT_MAX);
    let timeout_ms = args
        .timeout_ms
        .unwrap_or(CODE_SEARCH_TIMEOUT_DEFAULT_MS)
        .clamp(CODE_SEARCH_TIMEOUT_MIN_MS, CODE_SEARCH_TIMEOUT_MAX_MS);
    let case_sensitive = args.case_sensitive.unwrap_or(false);
    let fixed_strings = args.fixed_strings.unwrap_or(false);

    let mut rg_args = vec![
        "--line-number".to_string(),
        "--with-filename".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "--no-heading".to_string(),
        "--hidden".to_string(),
        "--glob".to_string(),
        "!.git/**".to_string(),
        "--glob".to_string(),
        "!**/node_modules/**".to_string(),
        "--glob".to_string(),
        "!**/target/**".to_string(),
        "--glob".to_string(),
        "!**/dist/**".to_string(),
        "--max-columns".to_string(),
        "400".to_string(),
        "--max-columns-preview".to_string(),
    ];

    if !case_sensitive {
        rg_args.push("-i".to_string());
    }
    if fixed_strings {
        rg_args.push("-F".to_string());
    }
    if context > 0 {
        rg_args.push("-C".to_string());
        rg_args.push(context.to_string());
    }

    if let Some(globs) = &args.glob {
        for glob in globs {
            let trimmed = glob.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed
                .chars()
                .any(|ch| matches!(ch, ';' | '|' | '&' | '`' | '\n' | '\r'))
            {
                return Err(format!(
                    "Error: invalid code_search glob (contains shell metacharacters): {trimmed}"
                ));
            }
            rg_args.push("--glob".to_string());
            rg_args.push(trimmed.to_string());
        }
    }

    // Keep head_limit as a soft cap after execution; still pass a generous rg limit.
    rg_args.push("--max-count".to_string());
    rg_args.push(head_limit.to_string());
    rg_args.push("--".to_string());
    rg_args.push(pattern.to_string());
    rg_args.push(search_root.to_string_lossy().to_string());

    Ok(CodeSearchCommand {
        rg_path,
        args: rg_args,
        search_root,
        timeout_ms,
        head_limit,
    })
}

pub(crate) fn format_code_search_output(
    pattern: &str,
    search_root: &Path,
    workspace_root: &Path,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    head_limit: usize,
) -> String {
    let root_display = relative_display_path(workspace_root, search_root);
    let mut lines = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        lines.push(rewrite_match_line(workspace_root, line));
        if lines.len() >= head_limit {
            break;
        }
    }

    if exit_code == 1 && lines.is_empty() {
        let mut output = format!(
            "No matches for pattern `{pattern}` under {root_display} (searched with bundled rg)."
        );
        if !stderr.trim().is_empty() {
            output.push_str("\n\nrg stderr:\n");
            output.push_str(stderr.trim());
        }
        return truncate_output(&output, CODE_SEARCH_OUTPUT_MAX_CHARS);
    }

    if exit_code > 1 && lines.is_empty() {
        let mut output = format!(
            "code_search failed (exit {exit_code}) for pattern `{pattern}` under {root_display}."
        );
        if !stderr.trim().is_empty() {
            output.push_str("\n\nrg stderr:\n");
            output.push_str(stderr.trim());
        } else if !stdout.trim().is_empty() {
            output.push_str("\n\nrg output:\n");
            output.push_str(stdout.trim());
        }
        return truncate_output(&output, CODE_SEARCH_OUTPUT_MAX_CHARS);
    }

    let mut output = format!(
        "code_search results for `{pattern}` under {root_display} (showing {} match line(s), head_limit={head_limit}):\n",
        lines.len()
    );
    for line in &lines {
        output.push_str(line);
        output.push('\n');
    }
    if stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        > lines.len()
    {
        output.push_str(&format!(
            "\n... additional matches truncated by head_limit={head_limit}\n"
        ));
    }
    if !stderr.trim().is_empty() && exit_code > 1 {
        output.push_str("\nrg stderr:\n");
        output.push_str(stderr.trim());
        output.push('\n');
    }

    truncate_output(&output, CODE_SEARCH_OUTPUT_MAX_CHARS)
}

fn rewrite_match_line(workspace_root: &Path, line: &str) -> String {
    // rg --no-heading format: path:line:content  or path-line-content for context
    let separators = [':', '-'];
    for sep in separators {
        if let Some((path_part, rest)) = split_once_path_prefix(line, sep) {
            let rewritten = relative_display_path(workspace_root, Path::new(path_part));
            return format!("{rewritten}{sep}{rest}");
        }
    }
    line.to_string()
}

fn split_once_path_prefix(line: &str, sep: char) -> Option<(&str, &str)> {
    // Prefer Windows drive paths (C:\...) then generic split.
    if line.len() >= 3 {
        let bytes = line.as_bytes();
        if bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
            // path starts with X:/ or X:\
            let after_drive = &line[2..];
            if let Some(rel_idx) = after_drive.find(sep) {
                let idx = 2 + rel_idx;
                return Some((&line[..idx], &line[idx + 1..]));
            }
            return None;
        }
    }
    line.split_once(sep)
}

fn relative_display_path(root: &Path, path: &Path) -> String {
    let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path_canon
        .strip_prefix(&root_canon)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
