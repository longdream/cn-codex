//! Record & Replay: list, read, run and delete Playwright replay scripts.
//!
//! Script generation is delegated to the main pipeline (the AI agent), which
//! reads the recorded trace and writes a Python script with Chinese comments and
//! per-step descriptions. This module only manages the script files on disk.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::recording::TraceFile;

/// How long a single replay script run is allowed to take before it is killed.
const RUN_TIMEOUT_SECS: u64 = 180;

/// In-memory registry for replay processes. The UI can request a stop while
/// `wait_with_output` is awaiting the child, so the process ID must live
/// outside that future and be independently addressable.
#[derive(Debug, Clone, Copy)]
struct ActiveReplay {
    pid: Option<u32>,
    stop_requested: bool,
}

fn active_replays() -> &'static Mutex<HashMap<String, ActiveReplay>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, ActiveReplay>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn begin_replay(id: &str) -> Result<(), String> {
    let mut registry = active_replays().lock().await;
    if registry.contains_key(id) {
        return Err(format!("Replay script is already running: {id}"));
    }
    registry.insert(
        id.to_string(),
        ActiveReplay {
            pid: None,
            stop_requested: false,
        },
    );
    Ok(())
}

/// Register a spawned child and return whether a stop request raced with
/// process startup.
async fn set_replay_pid(id: &str, pid: u32) -> bool {
    let mut registry = active_replays().lock().await;
    let entry = registry.entry(id.to_string()).or_insert(ActiveReplay {
        pid: Some(pid),
        stop_requested: false,
    });
    entry.pid = Some(pid);
    entry.stop_requested
}

async fn finish_replay(id: &str) -> bool {
    active_replays()
        .lock()
        .await
        .remove(id)
        .map(|run| run.stop_requested)
        .unwrap_or(false)
}

async fn replay_stop_requested(id: &str) -> bool {
    active_replays()
        .lock()
        .await
        .get(id)
        .map(|run| run.stop_requested)
        .unwrap_or(false)
}

async fn clear_replay_pid(id: &str) -> bool {
    active_replays()
        .lock()
        .await
        .get_mut(id)
        .map(|run| {
            run.pid = None;
            run.stop_requested
        })
        .unwrap_or(false)
}

/// Kill a replay process and its browser children. `taskkill /T` is needed on
/// Windows because Playwright may leave a browser child behind otherwise.
async fn terminate_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .await;
    }

    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .await;
    }
}

/// Request cancellation of a running script. Returning `false` means the
/// script had already finished (or was never running), which is still a
/// successful stop operation from the UI's perspective.
pub async fn stop_script(id: &str) -> Result<bool, String> {
    let pid = {
        let mut registry = active_replays().lock().await;
        let Some(run) = registry.get_mut(id) else {
            return Ok(false);
        };
        run.stop_requested = true;
        run.pid
    };

    if let Some(pid) = pid {
        terminate_process_tree(pid).await;
    }
    Ok(true)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayScriptMeta {
    pub id: String,
    pub name: String,
    pub path: String,
    pub trace_session_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub step_count: usize,
    pub start_url: String,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_run_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRunResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    /// Human-readable failure summary extracted from the script output.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReadResult {
    pub id: String,
    pub path: String,
    pub content: String,
}

pub fn scripts_dir(recordings_dir: &Path) -> PathBuf {
    recordings_dir.join("scripts")
}

/// A sidecar JSON file that persists the last run result for a script so the
/// list command can surface status/error after a reload.
fn last_run_path(scripts_dir: &Path, id: &str) -> PathBuf {
    scripts_dir.join(format!("{id}.last.json"))
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Pick the first meaningful start page for a recorded session.
fn effective_start_url(trace: &TraceFile) -> String {
    if is_http_url(&trace.start_url) {
        return trace.start_url.clone();
    }
    trace
        .events
        .iter()
        .find_map(|e| is_http_url(&e.url).then(|| e.url.clone()))
        .unwrap_or_default()
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: impl Into<String>) {
    let candidate = candidate.into();
    if !candidate.trim().is_empty() && !candidates.iter().any(|item| item == &candidate) {
        candidates.push(candidate);
    }
}

fn add_python_install_candidate(
    preferred: &mut Vec<String>,
    fallback: &mut Vec<String>,
    path: PathBuf,
) {
    if !path.is_file() {
        return;
    }
    let value = path.to_string_lossy().to_string();
    let has_playwright = path
        .parent()
        .map(|root| root.join("Lib/site-packages/playwright").is_dir())
        .unwrap_or(false);
    if has_playwright {
        push_unique_candidate(preferred, value);
    } else {
        push_unique_candidate(fallback, value);
    }
}

#[cfg(windows)]
fn discover_python_installations(
    preferred: &mut Vec<String>,
    fallback: &mut Vec<String>,
    root: &Path,
) {
    if root.join("python.exe").is_file() {
        add_python_install_candidate(preferred, fallback, root.join("python.exe"));
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    for dir in dirs {
        add_python_install_candidate(preferred, fallback, dir.join("python.exe"));
    }
}

fn python_candidates(recordings_dir: &Path) -> Vec<String> {
    let mut preferred = Vec::new();
    let mut fallback = Vec::new();

    for key in ["CN_CODEX_PYTHON", "PYTHON", "PYTHON3"] {
        if let Some(value) = env::var_os(key) {
            push_unique_candidate(&mut preferred, value.to_string_lossy().to_string());
        }
    }

    let workspace_dir = recordings_dir.parent().unwrap_or(recordings_dir);
    for relative in [
        ".venv/Scripts/python.exe",
        "venv/Scripts/python.exe",
        "python/python.exe",
        "runtime/python.exe",
    ] {
        add_python_install_candidate(&mut preferred, &mut fallback, workspace_dir.join(relative));
    }

    #[cfg(windows)]
    {
        for key in [
            "LOCALAPPDATA",
            "USERPROFILE",
            "PROGRAMFILES",
            "PROGRAMFILES(X86)",
        ] {
            if let Some(root) = env::var_os(key) {
                let root = PathBuf::from(root);
                discover_python_installations(
                    &mut preferred,
                    &mut fallback,
                    &root.join("Programs/Python"),
                );
                discover_python_installations(&mut preferred, &mut fallback, &root.join("Python"));
            }
        }
    }

    #[cfg(windows)]
    let commands = ["python", "py", "python3"];
    #[cfg(not(windows))]
    let commands = ["python3", "python"];
    fallback.extend(commands.into_iter().map(str::to_string));

    preferred.extend(fallback);
    preferred
}

fn interpreter_unavailable(stdout: &str, stderr: &str) -> bool {
    let output = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    output.contains("python was not found")
        || output.contains("no module named 'playwright'")
        || output.contains("no module named \"playwright\"")
        || output.contains("no module named playwright")
}

/// List all saved replay scripts, newest first. Metadata (name / start URL /
/// step count) is derived from the original recording trace when available.
pub async fn list_scripts(recordings_dir: &Path) -> Result<Vec<ReplayScriptMeta>, String> {
    let dir = scripts_dir(recordings_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut metas = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read scripts dir: {e}"))?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("py") {
            continue;
        }
        let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = file_stem.to_string();

        let mut name = id.clone();
        let mut trace_session_id = id.clone();
        let mut step_count = 0usize;
        let mut start_url = String::new();

        // Derive human-friendly metadata from the original recording trace.
        if let Ok(trace_content) =
            tokio::fs::read_to_string(recordings_dir.join(format!("{id}.trace.json"))).await
        {
            if let Ok(trace) = serde_json::from_str::<TraceFile>(&trace_content) {
                if !trace.session_name.trim().is_empty() {
                    name = trace.session_name.clone();
                }
                trace_session_id = trace.session_id.clone();
                step_count = trace.events.len();
                start_url = effective_start_url(&trace);
            }
        }

        let (mut last_status, mut last_error, mut last_run_at) = (None, None, None);
        if let Ok(content) = tokio::fs::read_to_string(last_run_path(&dir, &id)).await {
            if let Ok(last) = serde_json::from_str::<serde_json::Value>(&content) {
                last_status = last
                    .get("last_status")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                last_error = last
                    .get("last_error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                last_run_at = last.get("last_run_at").and_then(serde_json::Value::as_i64);
            }
        }

        let modified = tokio::fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(now_millis);

        metas.push(ReplayScriptMeta {
            id,
            name,
            path: path.to_string_lossy().to_string(),
            trace_session_id,
            created_at: modified,
            updated_at: modified,
            step_count,
            start_url,
            last_status,
            last_error,
            last_run_at,
        });
    }

    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(metas)
}

/// Read a script by id, returning its path and content.
pub async fn read_script(recordings_dir: &Path, id: &str) -> Result<ReplayReadResult, String> {
    let dir = scripts_dir(recordings_dir);
    let path = dir.join(format!("{id}.py"));
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read script {id}: {e}"))?;
    Ok(ReplayReadResult {
        id: id.to_string(),
        path: path.to_string_lossy().to_string(),
        content,
    })
}

/// Delete a script (and its last-run sidecar) by id.
pub async fn delete_script(recordings_dir: &Path, id: &str) -> Result<(), String> {
    let dir = scripts_dir(recordings_dir);
    let py_path = dir.join(format!("{id}.py"));
    let last_path = last_run_path(&dir, id);

    if py_path.exists() {
        tokio::fs::remove_file(&py_path)
            .await
            .map_err(|e| format!("Failed to delete script {id}: {e}"))?;
    }
    if last_path.exists() {
        let _ = tokio::fs::remove_file(&last_path).await;
    }
    Ok(())
}

/// Run a saved replay script via a Python interpreter, capturing stdout/stderr.
pub async fn run_script(recordings_dir: &Path, id: &str) -> Result<ReplayRunResult, String> {
    let dir = scripts_dir(recordings_dir);
    let path = dir.join(format!("{id}.py"));
    if !path.exists() {
        return Err(format!("Replay script not found: {id}"));
    }

    begin_replay(id).await?;

    let candidates = python_candidates(recordings_dir);

    let started = std::time::Instant::now();
    let mut last_spawn_error: Option<String> = None;

    for exe in candidates {
        let script_path = path.clone();
        let child = match Command::new(&exe)
            .kill_on_drop(true)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .arg(&script_path)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                last_spawn_error = Some(format!("Failed to launch {exe}: {e}"));
                if replay_stop_requested(id).await {
                    let _ = finish_replay(id).await;
                    let result = stopped_result(started, String::new(), String::new(), None);
                    persist_last_run(&dir, id, &result).await;
                    return Ok(result);
                }
                continue;
            }
        };

        let Some(pid) = child.id() else {
            let stop_requested = finish_replay(id).await;
            let result = if stop_requested {
                stopped_result(started, String::new(), String::new(), None)
            } else {
                failed_result(
                    started,
                    String::new(),
                    String::new(),
                    None,
                    format!("Failed to get process ID for {exe}"),
                )
            };
            persist_last_run(&dir, id, &result).await;
            return Ok(result);
        };

        let stop_raced_with_start = set_replay_pid(id, pid).await;
        if stop_raced_with_start {
            terminate_process_tree(pid).await;
        }

        let wait_result = tokio::time::timeout(
            Duration::from_secs(RUN_TIMEOUT_SECS),
            child.wait_with_output(),
        )
        .await;

        if matches!(wait_result, Err(_)) {
            // Dropping the timed-out future kills the child because
            // `kill_on_drop(true)` is set; taskkill also handles descendants.
            terminate_process_tree(pid).await;
        }

        match wait_result {
            Err(_) => {
                let stop_requested = finish_replay(id).await;
                let result = if stop_requested {
                    stopped_result(started, String::new(), String::new(), None)
                } else {
                    failed_result(
                        started,
                        String::new(),
                        String::new(),
                        None,
                        format!("Replay script timed out after {RUN_TIMEOUT_SECS}s"),
                    )
                };
                persist_last_run(&dir, id, &result).await;
                return Ok(result);
            }
            Ok(Err(e)) => {
                let stop_requested = finish_replay(id).await;
                let result = if stop_requested {
                    stopped_result(started, String::new(), e.to_string(), None)
                } else {
                    failed_result(started, String::new(), e.to_string(), None, e.to_string())
                };
                persist_last_run(&dir, id, &result).await;
                return Ok(result);
            }
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let stop_requested_before_fallback = replay_stop_requested(id).await;

                // Windows App Execution Aliases can spawn a `python.exe` that
                // only prints "Python was not found". Likewise, a valid
                // interpreter may not have Playwright installed. Try the next
                // discovered interpreter before reporting that as the final
                // replay error.
                if !stop_requested_before_fallback
                    && !output.status.success()
                    && interpreter_unavailable(&stdout, &stderr)
                {
                    let detail = if stderr.trim().is_empty() {
                        stdout.trim().to_string()
                    } else {
                        stderr.trim().to_string()
                    };
                    last_spawn_error = Some(detail);
                    if clear_replay_pid(id).await {
                        let _ = finish_replay(id).await;
                        let result = stopped_result(started, stdout, stderr, output.status.code());
                        persist_last_run(&dir, id, &result).await;
                        return Ok(result);
                    }
                    continue;
                }

                let stop_requested = finish_replay(id).await;
                let exit_code = output.status.code();
                let duration_ms = started.elapsed().as_millis() as u64;

                let parsed = parse_structured_output(&stdout);
                let ok = !stop_requested
                    && parsed
                        .as_ref()
                        .and_then(|v| v.get("ok").and_then(serde_json::Value::as_bool))
                        .unwrap_or_else(|| output.status.success());
                let error = if stop_requested {
                    Some("Replay script stopped by user".to_string())
                } else {
                    extract_error(parsed.as_ref(), &stdout, &stderr, !ok)
                };

                let result = ReplayRunResult {
                    ok,
                    exit_code,
                    stdout,
                    stderr,
                    duration_ms,
                    error,
                };

                persist_last_run(&dir, id, &result).await;
                return Ok(result);
            }
        }
    }

    let stop_requested = finish_replay(id).await;
    let error = last_spawn_error.unwrap_or_else(|| "No Python interpreter found".to_string());
    let result = if stop_requested {
        stopped_result(started, String::new(), String::new(), None)
    } else {
        failed_result(started, String::new(), String::new(), None, error)
    };
    persist_last_run(&dir, id, &result).await;
    Ok(result)
}

fn stopped_result(
    started: std::time::Instant,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
) -> ReplayRunResult {
    ReplayRunResult {
        ok: false,
        exit_code,
        stdout,
        stderr,
        duration_ms: started.elapsed().as_millis() as u64,
        error: Some("Replay script stopped by user".to_string()),
    }
}

fn failed_result(
    started: std::time::Instant,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    error: String,
) -> ReplayRunResult {
    ReplayRunResult {
        ok: false,
        exit_code,
        stdout,
        stderr,
        duration_ms: started.elapsed().as_millis() as u64,
        error: Some(error),
    }
}

/// Parse the last JSON line emitted by the script (best-effort). The script is
/// generated by the main pipeline, so this only enriches the result when the
/// agent chose to emit a structured error object.
fn parse_structured_output(stdout: &str) -> Option<serde_json::Value> {
    // Generated scripts often use pretty-printed JSON, so parsing individual
    // lines misses the object entirely. Try each possible opening brace from
    // the end; the outermost candidate is the first one that parses.
    stdout.match_indices('{').rev().find_map(|(index, _)| {
        serde_json::from_str::<serde_json::Value>(stdout[index..].trim()).ok()
    })
}

fn extract_error(
    parsed: Option<&serde_json::Value>,
    stdout: &str,
    stderr: &str,
    failed: bool,
) -> Option<String> {
    if let Some(parsed) = parsed {
        if let Some(error) = parsed.get("error").and_then(serde_json::Value::as_str) {
            let mut parts = vec![error.to_string()];
            if let Some(step) = parsed.get("step").and_then(serde_json::Value::as_i64) {
                parts.push(format!("step={step}"));
            }
            if let Some(kind) = parsed.get("kind").and_then(serde_json::Value::as_str) {
                parts.push(format!("kind={kind}"));
            }
            if let Some(url) = parsed.get("url").and_then(serde_json::Value::as_str) {
                if !url.is_empty() {
                    parts.push(format!("url={url}"));
                }
            }
            if let Some(shot) = parsed.get("screenshot").and_then(serde_json::Value::as_str) {
                if !shot.is_empty() {
                    parts.push(format!("screenshot={shot}"));
                }
            }
            return Some(parts.join(" | "));
        }
    }
    if failed {
        let trimmed_err = stderr.trim();
        if !trimmed_err.is_empty() {
            return Some(trimmed_err.chars().take(2000).collect());
        }
        let tail: String = stdout
            .trim_end()
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");
        if !tail.is_empty() {
            return Some(tail.chars().take(2000).collect());
        }
    }
    None
}

async fn persist_last_run(dir: &Path, id: &str, result: &ReplayRunResult) {
    let payload = json!({
        "last_status": if result.ok { "success" } else { "failed" },
        "last_error": result.error.clone(),
        "last_run_at": now_millis(),
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = tokio::fs::write(last_run_path(dir, id), json).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_structured_output_accepts_pretty_printed_json() {
        let stdout = r#"步骤 1：开始
❌ 回放失败
{
  "step": 2,
  "url": "https://example.test/login",
  "title": "登录",
  "error": "等待跳转超时"
}
"#;
        let parsed = parse_structured_output(stdout).expect("structured JSON should parse");
        assert_eq!(parsed["step"], 2);
        assert_eq!(parsed["error"], "等待跳转超时");
    }

    #[test]
    fn extract_error_includes_context_from_structured_output() {
        let parsed = serde_json::json!({
            "step": 4,
            "url": "https://example.test/login",
            "error": "元素未出现"
        });
        let error = extract_error(Some(&parsed), "", "", true).expect("error should be extracted");
        assert!(error.contains("元素未出现"));
        assert!(error.contains("step=4"));
        assert!(error.contains("url=https://example.test/login"));
    }

    #[test]
    fn interpreter_unavailable_detects_windows_alias_and_missing_playwright() {
        assert!(interpreter_unavailable("Python was not found", ""));
        assert!(interpreter_unavailable(
            "",
            "ModuleNotFoundError: No module named 'playwright'"
        ));
        assert!(!interpreter_unavailable("", "Timeout waiting for selector"));
    }
}
