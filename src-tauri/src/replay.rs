//! Record & Replay: list, read, run and delete Playwright replay scripts.
//!
//! Script generation is delegated to the main pipeline (the AI agent), which
//! reads the recorded trace and writes a Python script with Chinese comments and
//! per-step descriptions. This module only manages the script files on disk.

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::recording::{RecordingEvent, TraceFile};

#[cfg(windows)]
trait CommandNoConsole {
    fn no_console(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl CommandNoConsole for Command {
    fn no_console(&mut self) -> &mut Self {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

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
    #[serde(default)]
    pub steps: Vec<String>,
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
    /// When false, the UI must not send this result to the main pipeline for auto-fix
    /// (user stop, empty output after closing the browser, teardown noise).
    #[serde(default)]
    pub fixable: bool,
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

fn meta_path(scripts_dir: &Path, id: &str) -> PathBuf {
    scripts_dir.join(format!("{id}.meta.json"))
}

fn validate_script_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("Invalid script id".to_string());
    }
    Ok(())
}

fn sanitize_script_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Script name cannot be empty".to_string());
    }
    if trimmed.chars().count() > 80 {
        return Err("Script name is too long".to_string());
    }
    if trimmed.chars().any(|ch| ch.is_control()) {
        return Err("Script name contains invalid characters".to_string());
    }
    Ok(trimmed.to_string())
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn strip_step_title_noise(title: &str) -> String {
    title
        .trim()
        .trim_end_matches("……")
        .trim_end_matches("...")
        .trim_end_matches(" —— 完成")
        .trim_end_matches("——完成")
        .trim_end_matches('"')
        .trim_end_matches('\'')
        .trim_end_matches(')')
        .trim()
        .to_string()
}

fn parse_step_heading(line: &str) -> Option<(u32, String)> {
    let line = line.trim().trim_start_matches('#').trim();
    let line = line
        .trim_start_matches("print(")
        .trim_start_matches("print (")
        .trim_start_matches('f')
        .trim_start_matches(['"', '\'']);
    let rest = line.strip_prefix("步骤")?.trim_start();
    let mut digits = String::new();
    let mut chars = rest.chars();
    for ch in chars.by_ref() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch == '：' || ch == ':' {
            break;
        } else {
            return None;
        }
    }
    let num = digits.parse().ok()?;
    let title = strip_step_title_noise(&chars.collect::<String>());
    if title.is_empty() {
        return None;
    }
    Some((num, title))
}

fn parse_run_step_call(line: &str) -> Option<(u32, String)> {
    let rest = line.trim().strip_prefix("run_step(")?.trim_start();
    let mut digits = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    let num = digits.parse().ok()?;
    let rest: String = chars.collect();
    let rest = rest.trim().trim_start_matches(',').trim();
    let quote = rest.chars().next().filter(|ch| *ch == '"' || *ch == '\'')?;
    let title = rest[quote.len_utf8()..].split(quote).next()?.trim();
    if title.is_empty() {
        return None;
    }
    Some((num, title.to_string()))
}

fn extract_script_steps(content: &str) -> Vec<String> {
    let mut by_num = BTreeMap::new();
    for raw in content.lines() {
        if let Some((num, title)) = parse_step_heading(raw).or_else(|| parse_run_step_call(raw)) {
            by_num.entry(num).or_insert(title);
        }
    }
    by_num
        .into_iter()
        .map(|(num, title)| format!("步骤 {num}：{title}"))
        .collect()
}

fn summarize_trace_steps(events: &[RecordingEvent]) -> Vec<String> {
    let mut steps = Vec::new();
    for event in events {
        let label = match event.event_type.as_str() {
            "navigate" => {
                if matches!(
                    event.cause.as_deref(),
                    Some("redirect" | "link" | "form")
                ) {
                    continue;
                }
                let url = event.value.as_deref().unwrap_or(event.url.as_str());
                if url.is_empty() {
                    continue;
                }
                format!("打开 {url}")
            }
            "click" => {
                let target = if event.selector.is_empty() {
                    event.tag_name.as_str()
                } else {
                    event.selector.as_str()
                };
                if target.is_empty() {
                    continue;
                }
                format!("点击 {target}")
            }
            "type" => {
                if event
                    .input_type
                    .as_deref()
                    .is_some_and(|value| value.starts_with("delete"))
                {
                    continue;
                }
                let value = event.value.as_deref().unwrap_or("").trim();
                if value.is_empty() {
                    continue;
                }
                format!("输入 {value}")
            }
            "key" => {
                let key = event.key.as_deref().unwrap_or("").trim();
                if key.is_empty() {
                    continue;
                }
                format!("按键 {key}")
            }
            "select" => format!("选择 {}", event.value.as_deref().unwrap_or("")),
            "upload" => {
                let names = event
                    .files
                    .iter()
                    .map(|file| file.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                if names.is_empty() {
                    "选择上传文件".to_string()
                } else {
                    format!("上传 {names}")
                }
            }
            "submit" => "提交表单".to_string(),
            _ => continue,
        };
        if !steps.iter().any(|existing| existing == &label) {
            steps.push(label);
        }
    }
    steps
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
        let mut start_url = String::new();
        let mut steps = Vec::new();

        // Derive human-friendly metadata from the original recording trace.
        if let Ok(trace_content) =
            tokio::fs::read_to_string(recordings_dir.join(format!("{id}.trace.json"))).await
        {
            if let Ok(trace) = serde_json::from_str::<TraceFile>(&trace_content) {
                if !trace.session_name.trim().is_empty() {
                    name = trace.session_name.clone();
                }
                trace_session_id = trace.session_id.clone();
                start_url = effective_start_url(&trace);
                steps = summarize_trace_steps(&trace.events);
            }
        }

        if let Ok(script_content) = tokio::fs::read_to_string(&path).await {
            let script_steps = extract_script_steps(&script_content);
            if !script_steps.is_empty() {
                steps = script_steps;
            }
        }

        if let Ok(content) = tokio::fs::read_to_string(meta_path(&dir, &id)).await {
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(custom_name) = meta
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    name = custom_name.to_string();
                }
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

        let step_count = if steps.is_empty() { 0 } else { steps.len() };

        metas.push(ReplayScriptMeta {
            id,
            name,
            path: path.to_string_lossy().to_string(),
            trace_session_id,
            created_at: modified,
            updated_at: modified,
            step_count,
            steps,
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
    validate_script_id(id)?;
    let dir = scripts_dir(recordings_dir);
    let py_path = dir.join(format!("{id}.py"));
    let last_path = last_run_path(&dir, id);
    let custom_meta_path = meta_path(&dir, id);

    if py_path.exists() {
        tokio::fs::remove_file(&py_path)
            .await
            .map_err(|e| format!("Failed to delete script {id}: {e}"))?;
    }
    if last_path.exists() {
        let _ = tokio::fs::remove_file(&last_path).await;
    }
    if custom_meta_path.exists() {
        let _ = tokio::fs::remove_file(&custom_meta_path).await;
    }
    Ok(())
}

/// Persist a user-visible display name for a replay script card.
pub async fn rename_script(
    recordings_dir: &Path,
    id: &str,
    name: &str,
) -> Result<String, String> {
    validate_script_id(id)?;
    let name = sanitize_script_name(name)?;
    let dir = scripts_dir(recordings_dir);
    let py_path = dir.join(format!("{id}.py"));
    if !py_path.exists() {
        return Err(format!("Replay script not found: {id}"));
    }

    let custom_meta_path = meta_path(&dir, id);
    let mut meta = if let Ok(content) = tokio::fs::read_to_string(&custom_meta_path).await {
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    meta["name"] = json!(name);
    let encoded = serde_json::to_string_pretty(&meta)
        .map_err(|e| format!("Failed to serialize script name: {e}"))?;
    tokio::fs::write(&custom_meta_path, encoded)
        .await
        .map_err(|e| format!("Failed to save script name: {e}"))?;

    let trace_path = recordings_dir.join(format!("{id}.trace.json"));
    if let Ok(content) = tokio::fs::read_to_string(&trace_path).await {
        if let Ok(mut trace) = serde_json::from_str::<TraceFile>(&content) {
            trace.session_name = name.clone();
            if let Ok(encoded) = serde_json::to_string_pretty(&trace) {
                let _ = tokio::fs::write(&trace_path, encoded).await;
            }
        }
    }

    Ok(name)
}

fn validate_browser_automation_script(content: &str) -> Result<(), String> {
    let compact = content
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if compact.contains("headless=true") {
        return Err(
            "生成的脚本启用了 headless=True，右侧回放将看不到浏览器和模拟操作。请将其改为 headless=False 后再运行。"
                .to_string(),
        );
    }

    let has_playwright =
        content.contains("sync_playwright") || content.contains("async_playwright");
    let has_browser_launch = content.contains(".chromium.launch(")
        || content.contains(".firefox.launch(")
        || content.contains(".webkit.launch(");
    let has_page = content.contains(".new_page(") || content.contains(".pages[");
    let has_browser_action = [
        ".goto(",
        ".click(",
        ".fill(",
        ".press(",
        ".hover(",
        ".select_option(",
    ]
    .iter()
    .any(|needle| content.contains(needle));

    if has_playwright && has_browser_launch && has_page && has_browser_action {
        return Ok(());
    }

    Err(
        "生成的脚本不包含完整的 Playwright 浏览器启动和页面操作，已拒绝运行，避免只打印步骤却不执行回放。请重新生成脚本。"
            .to_string(),
    )
}

/// Run a saved replay script via a Python interpreter, capturing stdout/stderr.
pub async fn run_script(recordings_dir: &Path, id: &str) -> Result<ReplayRunResult, String> {
    let dir = scripts_dir(recordings_dir);
    let path = dir.join(format!("{id}.py"));
    if !path.exists() {
        return Err(format!("Replay script not found: {id}"));
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read replay script {id}: {e}"))?;
    validate_browser_automation_script(&content)?;

    begin_replay(id).await?;

    let candidates = python_candidates(recordings_dir);

    let started = std::time::Instant::now();
    let mut last_spawn_error: Option<String> = None;

    for exe in candidates {
        let script_path = path.clone();
        let mut command = Command::new(&exe);
        command
            .kill_on_drop(true)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env("PYTHONUNBUFFERED", "1")
            // Right-panel replay must be visible. Explicitly override any
            // REPLAY_HEADLESS=1 inherited from the app/terminal environment.
            .env("REPLAY_HEADLESS", "0")
            .arg(&script_path);
        #[cfg(windows)]
        command.no_console();

        let child = match command.spawn() {
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
                let (ok, error, fixable) = evaluate_run_outcome(
                    stop_requested,
                    output.status.success(),
                    &stdout,
                    &stderr,
                );

                let result = ReplayRunResult {
                    ok,
                    exit_code,
                    stdout,
                    stderr,
                    duration_ms,
                    error,
                    fixable,
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
        fixable: false,
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
        fixable: true,
    }
}

fn looks_like_replay_payload(value: &serde_json::Value) -> bool {
    value.get("ok").is_some() || value.get("error").is_some()
}

fn parse_json_object(raw: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(raw.trim())
        .ok()
        .filter(looks_like_replay_payload)
}

/// Parse the last `REPLAY_RESULT` JSON emitted by the script (best-effort).
fn parse_structured_output(stdout: &str) -> Option<serde_json::Value> {
    for line in stdout.lines().rev() {
        let line = line.trim();
        let payload = line
            .strip_prefix("REPLAY_RESULT")
            .map(|rest| rest.trim_start_matches(':').trim())
            .unwrap_or(line);
        if payload.starts_with('{') {
            if let Some(parsed) = parse_json_object(payload) {
                return Some(parsed);
            }
        }
    }
    // Generated scripts often use pretty-printed JSON, so parsing individual
    // lines misses the object entirely. Try each possible opening brace from
    // the end; keep only objects that look like a replay result.
    stdout.match_indices('{').rev().find_map(|(index, _)| {
        parse_json_object(&stdout[index..])
    })
}

fn stdout_indicates_success(stdout: &str) -> bool {
    stdout.lines().rev().take(20).any(|line| {
        let text = line.trim();
        text.contains("[OK]")
            || text.contains("回放成功")
            || text.contains("\"ok\": true")
            || text.contains("\"ok\":true")
    })
}

fn looks_like_browser_closed(stdout: &str, stderr: &str) -> bool {
    let blob = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    blob.contains("targetclosed")
        || blob.contains("target closed")
        || blob.contains("browser has been closed")
        || blob.contains("context has been closed")
        || blob.contains("has been closed")
        || blob.contains("connection closed")
        || blob.contains("browser closed")
}

/// Decide success / error / whether the UI should auto-fix.
///
/// Closing the Playwright window after the flow finished often kills Python
/// with an empty pipe or a teardown exception. That is not a script bug.
fn evaluate_run_outcome(
    stop_requested: bool,
    exit_success: bool,
    stdout: &str,
    stderr: &str,
) -> (bool, Option<String>, bool) {
    if stop_requested {
        return (
            false,
            Some("Replay script stopped by user".to_string()),
            false,
        );
    }

    let parsed = parse_structured_output(stdout);
    let structured_ok = parsed
        .as_ref()
        .and_then(|value| value.get("ok").and_then(serde_json::Value::as_bool));
    let success_text = stdout_indicates_success(stdout);
    let browser_closed = looks_like_browser_closed(stdout, stderr);

    let ok = match structured_ok {
        Some(true) => true,
        Some(false) => false,
        None => success_text || exit_success,
    };
    if ok {
        return (true, None, false);
    }
    if success_text && browser_closed {
        return (true, None, false);
    }

    let error = extract_error(parsed.as_ref(), stdout, stderr, true);
    let has_diagnostics = error
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
        || !stdout.trim().is_empty()
        || !stderr.trim().is_empty();
    if !has_diagnostics {
        // 脚本失败但没有输出也可能是脚本本身的问题（例如空脚本、启动即崩溃），
        // 仍值得反馈给主链路分析修复，而不是直接跳过。
        return (
            false,
            Some("回放进程已结束但没有输出，脚本可能未正确执行。".to_string()),
            true,
        );
    }
    (false, error, true)
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

    #[test]
    fn parse_structured_output_prefers_replay_result_line() {
        let stdout = r#"步骤 1：打开页面
viewport={"width": 1440}
REPLAY_RESULT {"ok": true, "step": "done", "url": "https://example.test/portal", "error": null}
"#;
        let parsed = parse_structured_output(stdout).expect("REPLAY_RESULT should parse");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["step"], "done");
    }

    #[test]
    fn evaluate_run_outcome_treats_success_text_as_ok_even_if_exit_failed() {
        let stdout = "[OK] 回放成功：已进入门户\n";
        let (ok, error, fixable) = evaluate_run_outcome(false, false, stdout, "");
        assert!(ok);
        assert!(error.is_none());
        assert!(!fixable);
    }

    #[test]
    fn evaluate_run_outcome_empty_output_is_fixable() {
        let (ok, error, fixable) = evaluate_run_outcome(false, false, "", "");
        assert!(!ok);
        assert!(fixable);
        assert!(error.unwrap().contains("没有输出"));
    }

    #[test]
    fn evaluate_run_outcome_browser_closed_after_success_is_ok() {
        let stdout = "步骤 13：等待进入门户页面 —— 完成\n[OK] 回放成功：已进入 IPSA Pro 门户\n";
        let stderr = "playwright._impl._errors.TargetClosedError: Target page, context or browser has been closed";
        let (ok, error, fixable) = evaluate_run_outcome(false, false, stdout, stderr);
        assert!(ok);
        assert!(error.is_none());
        assert!(!fixable);
    }

    #[test]
    fn evaluate_run_outcome_structured_failure_is_fixable() {
        let stdout = r#"REPLAY_RESULT {"ok": false, "step": 4, "error": "未找到账号输入框"}"#;
        let (ok, error, fixable) = evaluate_run_outcome(false, false, stdout, "");
        assert!(!ok);
        assert!(fixable);
        assert!(error.unwrap().contains("未找到账号输入框"));
    }

    #[test]
    fn evaluate_run_outcome_user_stop_is_not_fixable() {
        let (ok, error, fixable) = evaluate_run_outcome(true, false, "步骤 1", "");
        assert!(!ok);
        assert!(!fixable);
        assert_eq!(error.as_deref(), Some("Replay script stopped by user"));
    }

    #[test]
    fn extract_script_steps_deduplicates_and_keeps_order() {
        let content = r#"
print("步骤 1：打开入口页面……", flush=True)
print("步骤 1：打开入口页面 —— 完成", flush=True)
# 步骤 2：输入账号
print("步骤 2：输入账号……")
run_step(3, "勾选协议", fn)
"#;
        assert_eq!(
            extract_script_steps(content),
            vec![
                "步骤 1：打开入口页面".to_string(),
                "步骤 2：输入账号".to_string(),
                "步骤 3：勾选协议".to_string(),
            ]
        );
    }

    #[test]
    fn sanitize_script_name_rejects_empty_and_keeps_trimmed() {
        assert!(sanitize_script_name("   ").is_err());
        assert_eq!(sanitize_script_name("  登录门户  ").unwrap(), "登录门户");
    }

    #[test]
    fn validate_script_id_rejects_path_fragments() {
        assert!(validate_script_id("../secret").is_err());
        assert!(validate_script_id("e708fc40-fb28-40f0-b811-5151cced293a").is_ok());
    }

    #[test]
    fn browser_automation_validation_accepts_real_playwright_script() {
        let script = r##"
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    browser = p.chromium.launch(headless=False)
    context = browser.new_context()
    page = context.new_page()
    page.goto("https://example.test")
    page.click("#login")
"##;
        assert!(validate_browser_automation_script(script).is_ok());
    }

    #[test]
    fn browser_automation_validation_rejects_print_only_script() {
        let script = r#"
print("步骤 1：打开页面")
print("步骤 2：点击登录")
print('REPLAY_RESULT {"ok": true}')
"#;
        let error = validate_browser_automation_script(script).unwrap_err();
        assert!(error.contains("不包含完整的 Playwright"));
    }

    #[test]
    fn browser_automation_validation_rejects_headless_script() {
        let script = r#"
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    context = browser.new_context()
    page = context.new_page()
    page.goto("https://example.test")
"#;
        let error = validate_browser_automation_script(script).unwrap_err();
        assert!(error.contains("headless=True"));
    }
}
