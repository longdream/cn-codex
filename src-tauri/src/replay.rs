//! Record & Replay: list, read, run and delete Playwright replay scripts.
//!
//! Script generation is delegated to the main pipeline (the AI agent), which
//! reads the recorded trace and writes a Python script with Chinese comments and
//! per-step descriptions. This module only manages the script files on disk.

use std::collections::{BTreeMap, HashMap, HashSet};
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
    #[serde(default)]
    pub report_count: usize,
    #[serde(default)]
    pub last_report_at: Option<i64>,
    /// Whether a `<id>.input.json` sidecar exists (show the JSON badge).
    #[serde(default)]
    pub has_input_document: bool,
    /// Number of fields inside the input document, when present.
    #[serde(default)]
    pub input_field_count: Option<usize>,
    /// True for CSV-imported documents without a generated `.py` sibling;
    /// the frontend renders these as standalone input-document cards.
    #[serde(default)]
    pub imported_only: bool,
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
    /// Most recent generated test report (also written to disk).
    #[serde(default)]
    pub report: Option<ReplayReportMeta>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReadResult {
    pub id: String,
    pub path: String,
    pub content: String,
}

/// Input document for a replay script.
///
/// The generated script and the main pipeline read this file to know which
/// concrete values to type/select/upload, so the same recording can be replayed
/// with different data by editing (or re-importing) this JSON only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayInputDocument {
    /// Matches the script / trace session id.
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Ordered field list derived from recorded fill/select actions or CSV rows.
    #[serde(default)]
    pub fields: Vec<ReplayInputField>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

/// One input slot: `page.fill(selectorCandidates[0], value)` etc. Actions map
/// 1:1 onto the recorded event types the generator understands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayInputField {
    /// Recorded event type: type | select | upload | key | click | navigate.
    #[serde(rename = "type", default)]
    pub action_type: String,
    /// Step number of the corresponding replay step.
    #[serde(default)]
    pub step: u32,
    /// Short Chinese description shown on cards.
    #[serde(default)]
    pub label: String,
    /// Primary selector (may be empty; then use selectorCandidates).
    #[serde(default)]
    pub selector: String,
    /// Fallback selectors captured with the event.
    #[serde(default)]
    pub selector_candidates: Vec<String>,
    /// Final stable value to fill / option to select / key to press.
    #[serde(default)]
    pub value: String,
}

/// Result of importing a CSV as an input document.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayCsvImportResult {
    /// Suggested display name (first non-empty value) for the card.
    #[serde(default)]
    pub suggested_name: Option<String>,
    /// Converted field count.
    #[serde(default)]
    pub field_count: usize,
    /// Fields in document order; saved via save_input_document afterwards.
    pub fields: Vec<ReplayInputField>,
    /// True when the first CSV row was detected as a header and skipped.
    pub header_detected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayCsvSaveResult {
    /// Script id matching `<imported-stem>` (`<stem>.input.json`).
    pub id: String,
    /// Path of the written `.input.json` file.
    pub path: String,
    /// Field count written into the input document.
    pub field_count: usize,
}

/// Best-effort header-cell normalizer (case folding only).
fn normalize_header_cell(cell: &str) -> String {
    cell.trim().trim_start_matches('\u{feff}').to_ascii_lowercase()
}

const HEADER_FIRST_COLUMN_KEYS: &[&str] = &[
    "selector",
    "selectors",
    "target",
    "page",
    "url",
    "field",
    "选择器",
    "定位",
    "页面",
];

const HEADER_SECOND_COLUMN_KEYS: &[&str] = &[
    "value",
    "text",
    "data",
    "password",
    "content",
    "input",
    "值",
    "内容",
    "输入",
    "数据",
];

/// Build one field from a recorded event; returns None for irrelevant actions
/// (deletions inside a typing sequence, auto redirects, …).
fn build_input_field_from_event(event: &RecordingEvent) -> Option<ReplayInputField> {
    let action_type = match event.event_type.as_str() {
        "type" => {
            if event
                .input_type
                .as_deref()
                .is_some_and(|value| value.starts_with("delete"))
            {
                return None;
            }
            "type"
        }
        "select" | "upload" | "key" | "click" | "navigate" => event.event_type.as_str(),
        _ => return None,
    };

    // navigate：只保留用户主动打开的起始页。
    if action_type == "navigate" && !matches!(event.cause.as_deref(), Some("user")) {
        return None;
    }

    // 首选 selector 排最前，与录制语义一致；其余候选按录制顺序去重追加。
    let mut candidates = Vec::new();
    if !event.selector.trim().is_empty() {
        push_unique_candidate(&mut candidates, event.selector.clone());
    }
    for candidate in &event.selector_candidates {
        push_unique_candidate(&mut candidates, candidate.clone());
    }
    if !event.tag_name.is_empty() {
        push_unique_candidate(&mut candidates, format!("css={}", event.tag_name.to_lowercase()));
    }

    let raw_value = match action_type {
        "select" | "type" | "key" => event.value.clone().unwrap_or_default(),
        "navigate" => event.url.clone(),
        _ => String::new(),
    };
    let value = raw_value.trim().to_string();

    if matches!(action_type, "type" | "select" | "navigate") && value.is_empty() {
        return None;
    }

    let label = match action_type {
        "type" => format!("输入 {value}"),
        "select" => format!("选择 {value}"),
        "key" => format!("按键 {value}"),
        "click" => {
            let target = if event.selector.is_empty() {
                event.tag_name.as_str()
            } else {
                event.selector.as_str()
            };
            if target.is_empty() {
                "点击元素".to_string()
            } else {
                format!("点击 {target}")
            }
        }
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
        _ => format!("打开 {value}"),
    };

    Some(ReplayInputField {
        action_type: action_type.to_string(),
        step: 0,
        label,
        selector: event.selector.clone(),
        selector_candidates: candidates,
        value,
    })
}

/// Heuristic: whether the first CSV row looks like a header instead of data.
/// Only common aliases are detected; a plain `account,password` style CSV can
/// simply put data on every row without headers.
fn csv_row_is_header(row: &[String]) -> bool {
    if row.len() < 2 {
        return false;
    }
    let first = normalize_header_cell(&row[0]);
    let second = normalize_header_cell(&row[1]);
    HEADER_FIRST_COLUMN_KEYS.contains(&first.as_str())
        || HEADER_SECOND_COLUMN_KEYS.contains(&second.as_str())
}

/// Parse RFC4180-ish CSV text into rows. Handles quoted cells with embedded
/// commas/newlines and escaped double quotes (`""`).
fn parse_csv_rows(content: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut field = String::new();
    let mut row: Vec<String> = Vec::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => in_quotes = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(ch),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(std::mem::take(&mut field));
        rows.push(row);
    }
    rows.retain(|current| current.iter().any(|cell| !cell.trim().is_empty()));
    rows
}

/// Convert an imported file stem into a safe script id. Non-ASCII characters
/// (e.g. Chinese file names) are preserved so distinct files keep distinct ids;
/// filesystem-hostile characters and whitespace collapse into `-`.
fn sanitize_import_script_id(raw: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in raw.trim().chars() {
        // 文件系统非法字符与空白折叠为 `-`；其余字符原样保留并小写化。
        if ch.is_whitespace() || ch.is_control() || "\\/:*?\"<>|".contains(ch) {
            pending_dash = true;
        } else {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        format!("csv-import-{}", now_millis())
    } else {
        trimmed.chars().take(80).collect()
    }
}

pub fn input_document_path(scripts_dir_path: &Path, id: &str) -> PathBuf {
    scripts_dir_path.join(format!("{id}.input.json"))
}

/// Fields converted from one CSV data row. `headers` may be empty (plain data).
///
/// Row shape (2 columns minimum):
/// - col 0: selector (optional; may be empty)
/// - col 1: value (typed / selected / pressed…)
/// - col 2+: optional second/third value reused as select options or keys.
fn csv_row_to_input_fields(row: &[String], index: usize) -> Vec<ReplayInputField> {
    if row.len() < 2 {
        return Vec::new();
    }
    let selector = row[0].trim().to_string();
    let mut fields = Vec::new();
    for cell in &row[1..] {
        let value = cell.trim().to_string();
        if value.is_empty() {
            continue;
        }
        fields.push(ReplayInputField {
            action_type: "type".to_string(),
            step: (index + 1) as u32,
            label: format!("输入 {value}"),
            selector: selector.clone(),
            selector_candidates: if selector.is_empty() {
                Vec::new()
            } else {
                vec![selector.clone()]
            },
            value,
        });
    }
    fields
}

/// Derive an input document from a recorded trace. Fields keep recording order;
/// step numbers are assigned by document position (`步骤 N`).
pub fn derive_input_document_from_trace(trace: &TraceFile) -> ReplayInputDocument {
    let mut fields = Vec::new();
    for event in &trace.events {
        // 同一输入框连续的 type/key 事件合并为最终稳定值（与生成提示一致）。
        if matches!(event.event_type.as_str(), "type" | "key")
            && !fields.is_empty()
            && fields.last().is_some_and(|last: &ReplayInputField| {
                last.action_type == "type"
                    && !last.selector.trim().is_empty()
                    && last.selector == event.selector
            })
            && event.value.as_deref().is_some_and(|value| !value.trim().is_empty())
        {
            let last = fields.last_mut().expect("checked non-empty");
            last.value = event.value.clone().unwrap_or_default().trim().to_string();
            last.label = format!("输入 {}", last.value);
            continue;
        }
        if let Some(mut field) = build_input_field_from_event(event) {
            field.step = (fields.len() + 1) as u32;
            fields.push(field);
        }
    }

    ReplayInputDocument {
        id: trace.session_id.clone(),
        name: trace.session_name.clone(),
        created_at: now_millis(),
        updated_at: now_millis(),
        fields,
    }
    .with_renumbered_steps()
}

impl ReplayInputDocument {
    /// Keep only meaningful fields and renumber steps 1..N in order.
    fn with_renumbered_steps(mut self) -> Self {
        self.fields.retain(|field| {
            !(matches!(field.action_type.as_str(), "type" | "select" | "navigate")
                && field.value.trim().is_empty()
                && field.action_type != "upload")
        });
        for (index, field) in self.fields.iter_mut().enumerate() {
            field.step = (index + 1) as u32;
        }
        self
    }
}

/// Ensure the script's input document exists; derive it from the trace when the
/// script was generated before this feature (or the file got deleted).
async fn ensure_input_document(
    recordings_dir: &Path,
    dir: &Path,
    id: &str,
) -> Result<(), String> {
    let path = input_document_path(dir, id);
    if tokio::fs::try_exists(&path)
        .await
        .map_err(|e| format!("Failed to inspect input document {id}: {e}"))?
    {
        return Ok(());
    }
    let trace_path = recordings_dir.join(format!("{id}.trace.json"));
    let content = tokio::fs::read_to_string(&trace_path).await.map_err(|e| {
        format!(
            "Missing both {id}.input.json and its recording trace: {e}"
        )
    })?;
    let trace = serde_json::from_str::<TraceFile>(&content)
        .map_err(|e| format!("Failed to parse trace {id}: {e}"))?;
    // 旧脚本目录可能还没建 scripts 子目录，写入前确保存在。
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create scripts dir: {e}"))?;
    }
    write_input_document(&path, &derive_input_document_from_trace(&trace))
        .await
        .map(|_| ())
}

pub async fn write_input_document(
    path: &Path,
    doc: &ReplayInputDocument,
) -> Result<String, String> {
    let encoded = serde_json::to_string_pretty(doc)
        .map_err(|e| format!("Failed to serialize input document: {e}"))?;
    tokio::fs::write(path, encoded)
        .await
        .map(|_| normalize_display_path(path))
        .map_err(|e| format!("Failed to write input document {}: {e}", path.display()))
}

/// Write or replace a script's input document.
pub async fn save_input_document(
    recordings_dir: &Path,
    id: &str,
    mut doc: ReplayInputDocument,
) -> Result<String, String> {
    validate_script_id(id)?;
    let dir = scripts_dir(recordings_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create scripts dir: {e}"))?;
    doc.id = id.to_string();
    doc.updated_at = now_millis();
    if doc.created_at <= 0 {
        doc.created_at = doc.updated_at;
    }
    write_input_document(&input_document_path(&dir, id), &doc).await
}

/// Read an existing `.input.json`; returns Ok(None) when the file is absent so
/// callers can fall back to deriving it from the trace.
pub async fn load_input_document(
    recordings_dir: &Path,
    id: &str,
) -> Result<Option<ReplayInputDocument>, String> {
    validate_script_id(id)?;
    let dir = scripts_dir(recordings_dir);
    let path = input_document_path(&dir, id);
    if !tokio::fs::try_exists(&path)
        .await
        .map_err(|e| format!("Failed to inspect input document {id}: {e}"))?
    {
        return Ok(None);
    }
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read input document {id}: {e}"))?;
    match serde_json::from_str::<ReplayInputDocument>(&content) {
        Ok(mut doc) => {
            doc.id = id.to_string();
            Ok(Some(doc))
        }
        Err(e) => Err(format!("Failed to parse input document {id}: {e}")),
    }
}

/// Read an input document for display; derives one from the trace on demand.
pub async fn read_input_document(
    recordings_dir: &Path,
    id: &str,
) -> Result<ReplayReadResult, String> {
    ensure_input_document(recordings_dir, &scripts_dir(recordings_dir), id).await?;
    let path = input_document_path(&scripts_dir(recordings_dir), id);
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read input document {id}: {e}"))?;
    Ok(ReplayReadResult {
        id: id.to_string(),
        path: normalize_display_path(&path),
        content,
    })
}

/// Delete the input document sidecar together with its script.
fn cleanup_input_document(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(input_document_path(dir, id));
}

/// Parse CSV text into input-document fields without touching disk.
///
/// CSV shape (2 columns minimum; extra columns become additional steps):
/// `selector,value[,value2…]` per row — selector may be empty for values the
/// generator must locate from context or candidates.
pub fn parse_csv_input_document(csv_content: &str) -> Result<ReplayCsvImportResult, String> {
    let rows = parse_csv_rows(csv_content);
    if rows.is_empty() {
        return Err("CSV 文件为空或没有可用的数据行".to_string());
    }

    let mut data_rows: &[Vec<String>] = &rows;
    if csv_row_is_header(&rows[0]) {
        data_rows = &rows[1..];
        if data_rows.is_empty() {
            return Err("CSV 只有表头，没有数据行".to_string());
        }
    }

    let mut fields = Vec::new();
    for (index, row) in data_rows.iter().enumerate() {
        fields.extend(csv_row_to_input_fields(row, index));
    }
    if fields.is_empty() {
        return Err("CSV 数据行里没有可用的输入值（每行至少 2 列：选择器,值）".to_string());
    }

    // 名字取第一个非空值，作为卡片标题的默认建议；id 由前端传入的文件名决定。
    let first_value = fields[0].value.trim().to_string();

    Ok(ReplayCsvImportResult {
        suggested_name: (!first_value.is_empty()).then_some(first_value),
        field_count: fields.len(),
        fields,
        header_detected: rows.len() != data_rows.len(),
    })
}

/// Validate then persist an uploaded CSV document as `<stem>.input.json` next
/// to the replay scripts, where it appears as its own card in the panel.
pub async fn save_csv_input_document(
    recordings_dir: &Path,
    stem: &str,
    csv_content: &str,
) -> Result<ReplayCsvSaveResult, String> {
    let id = sanitize_import_script_id(stem);
    let dir = scripts_dir(recordings_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create scripts dir: {e}"))?;

    let parsed = parse_csv_input_document(csv_content)?;
    let mut doc = ReplayInputDocument {
        id: String::new(),
        name: parsed
            .suggested_name
            .clone()
            .unwrap_or_else(|| stem.to_string()),
        created_at: now_millis(),
        updated_at: now_millis(),
        fields: parsed.fields,
    };
    doc.id = id.clone();
    let output_path = input_document_path(&dir, &id);
    write_input_document(&output_path, &doc).await?;

    Ok(ReplayCsvSaveResult {
        id,
        path: normalize_display_path(&output_path),
        field_count: doc.fields.len(),
    })
}

/// Metadata for one persisted replay test report (`*.md` + sidecar json).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReportMeta {
    /// Report file stem (creation timestamp in milliseconds).
    pub id: String,
    pub script_id: String,
    pub path: String,
    pub created_at: i64,
    pub ok: bool,
    pub summary: String,
}

/// How a single replay step ended during the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepStatus {
    Passed,
    Failed,
    Skipped,
    Unknown,
}

impl StepStatus {
    fn label(self) -> &'static str {
        match self {
            StepStatus::Passed => "通过",
            StepStatus::Failed => "失败",
            StepStatus::Skipped => "未执行",
            StepStatus::Unknown => "未知",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            StepStatus::Passed => "✅",
            StepStatus::Failed => "❌",
            StepStatus::Skipped => "⏭️",
            StepStatus::Unknown => "❓",
        }
    }
}

/// One expected step and how it went during the last run.
struct StepAnalysis {
    num: u32,
    title: String,
    status: StepStatus,
    note: Option<String>,
}

/// Minimal aggregate counters derived from `analyze_step_results`.
struct StepTotals {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    unknown: usize,
}

/// One numbered-step reference observed in the run output, in output order.
struct StepRecord {
    num: u32,
    /// The same output line (or a closely following line) confirmed completion.
    marked_done: bool,
}

/// What the parser learned about progress from the replay process output.
struct OutputScan {
    records: Vec<StepRecord>,
    /// An explicit whole-run success marker such as `[OK]` / `回放成功` appeared.
    final_success: bool,
}

impl OutputScan {
    /// Distinct step numbers seen in the output, in first-seen order.
    #[allow(dead_code)]
    fn started_nums(&self) -> Vec<u32> {
        let mut nums = Vec::new();
        for record in &self.records {
            if !nums.contains(&record.num) {
                nums.push(record.num);
            }
        }
        nums
    }

    /// Whether the given step emitted an explicit completion marker.
    fn contains_done(&self, num: u32) -> bool {
        self.records
            .iter()
            .any(|record| record.num == num && record.marked_done)
    }

    /// Whether every observed step completed explicitly.
    fn all_completed(&self) -> bool {
        self.records.iter().all(|record| record.marked_done)
    }

    /// Last observed-but-incomplete step; the most plausible failure point.
    fn last_unfinished_num(&self) -> Option<u32> {
        self.records
            .iter()
            .rev()
            .find(|record| !record.marked_done)
            .map(|record| record.num)
    }
}

/// Parse `"步骤 N：标题"` entries produced by `extract_script_steps`.
fn parse_expected_step_label(label: &str) -> Option<(u32, String)> {
    let rest = label.trim().strip_prefix("步骤")?.trim_start();
    let mut digits = String::new();
    let mut chars = rest.chars();
    for ch in chars.by_ref() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            break;
        }
    }
    let num = digits.parse().ok()?;
    let remainder = chars.as_str().trim_start();
    let title = remainder.strip_prefix('：').unwrap_or(remainder).trim();
    Some((num, title.to_string()))
}

/// Locate a `步骤 N` token anywhere in the line and return `(num, rest-of-line)`.
fn find_step_token(line: &str) -> Option<(u32, &str)> {
    const STEP_TOKEN: &str = "步骤";
    const STEP_TOKEN_LEN: usize = STEP_TOKEN.len();
    let mut search_from = 0usize;
    while let Some(rel_pos) = line[search_from..].find(STEP_TOKEN) {
        let token_start = search_from + rel_pos;
        let after = line[token_start + STEP_TOKEN_LEN..]
            .trim_start_matches(|ch: char| ch == ' ')
            .trim_start_matches('#');
        let digit_len = after
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .map(|ch: char| ch.len_utf8())
            .sum::<usize>();
        if digit_len > 0 {
            if let Ok(num) = after[..digit_len].parse::<u32>() {
                return Some((num, &after[digit_len..]));
            }
        }
        search_from = token_start + STEP_TOKEN_LEN;
    }
    None
}

/// Scan raw run output for per-step progress and overall success markers.
///
/// Generated scripts print progress like `步骤 2：输入账号……` before acting and
/// `步骤 2：输入账号 —— 完成` afterwards; they finish with `[OK]` / `回放成功`
/// (or a `REPLAY_RESULT` JSON handled separately). Any progress line printed
/// after the flow finished is attributed to real progress only via `seen`,
/// so late teardown echoes cannot fake completion.
fn scan_output_steps(stdout: &str) -> OutputScan {
    let mut records: Vec<StepRecord> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut final_success = false;

    for raw_line in stdout.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((num, rest)) = find_step_token(line) {
            let rest_lower = rest.to_ascii_lowercase();
            let line_lower = line.to_ascii_lowercase();
            let marked_done = rest_lower.contains("完成")
                || rest_lower.contains("[ok]")
                || line_lower.contains("[ok]")
                || line.contains('✔');
            if seen.insert(num) {
                records.push(StepRecord {
                    num,
                    marked_done,
                });
            } else if marked_done {
                for record in records.iter_mut() {
                    if record.num == num {
                        record.marked_done = true;
                    }
                }
            }
            continue;
        }

        let lowered = line.to_ascii_lowercase();
        if lowered.contains("[ok]") || lowered.contains("回放成功") {
            final_success = true;
        }
        // A trailing bare completion line confirms the most recent step.
        if line.contains("完成") {
            if let Some(record) = records.iter_mut().rev().find(|r| !r.marked_done) {
                record.marked_done = true;
            }
        }
    }

    if final_success {
        for record in records.iter_mut() {
            record.marked_done = true;
        }
    }

    OutputScan {
        records,
        final_success,
    }
}

/// Numeric `step` reported by the script's `REPLAY_RESULT` JSON, if any.
fn resolved_failed_step_from_structured(
    parsed: Option<&serde_json::Value>,
) -> Option<u32> {
    let step_value = parsed?.get("step")?;
    match step_value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .filter(|value| *value > 0 && *value <= u32::MAX as u64)
            .map(|value| value as u32),
        _ => None,
    }
}

/// Combine every signal into one concrete failure attribution.
fn combine_status_markers(
    structured_step: Option<u32>,
    scan: &OutputScan,
) -> Option<u32> {
    if structured_step.is_some() {
        return structured_step;
    }
    let output_suggests_failure =
        !scan.all_completed() || scan.records.is_empty();
    if !output_suggests_failure || scan.final_success {
        return None;
    }
    scan.last_unfinished_num()
}

fn analyze_step_results(
    expected_labels: &[String],
    scan: &OutputScan,
    structured_error: Option<&serde_json::Value>,
    overall_ok: bool,
) -> Vec<StepAnalysis> {
    let expected_pairs = expected_labels
        .iter()
        .filter_map(|label| parse_expected_step_label(label))
        .collect::<Vec<_>>();
    let structured_failed_step = resolved_failed_step_from_structured(structured_error);
    let failure_num = if overall_ok {
        None
    } else {
        combine_status_markers(structured_failed_step, scan)
    };

    let mut analyses = Vec::new();
    for (num, title) in &expected_pairs {
        let status = if scan.contains_done(*num) {
            StepStatus::Passed
        } else if scan.final_success {
            StepStatus::Passed
        } else if failure_num == Some(*num) {
            StepStatus::Failed
        } else if let Some(failed_num) = failure_num {
            if *num < failed_num {
                StepStatus::Unknown
            } else {
                StepStatus::Skipped
            }
        } else {
            StepStatus::Unknown
        };
        let note = if status == StepStatus::Failed {
            structured_error
                .and_then(|value| value.get("error"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        } else {
            None
        };
        analyses.push(StepAnalysis {
            num: *num,
            title: title.clone(),
            status,
            note,
        });
    }

    let expected_nums = expected_pairs
        .iter()
        .map(|(num, _)| *num)
        .collect::<HashSet<_>>();
    for record in &scan.records {
        if expected_nums.contains(&record.num) {
            continue;
        }
        analyses.push(StepAnalysis {
            num: record.num,
            title: "(脚本内出现但未登记)".to_string(),
            status: if record.marked_done {
                StepStatus::Passed
            } else {
                StepStatus::Unknown
            },
            note: None,
        });
    }

    analyses.sort_by_key(|analysis| analysis.num);
    analyses
}

fn count_step_statuses(analyses: &[StepAnalysis]) -> StepTotals {
    let mut totals = StepTotals {
        total: analyses.len(),
        passed: 0,
        failed: 0,
        skipped: 0,
        unknown: 0,
    };
    for analysis in analyses {
        match analysis.status {
            StepStatus::Passed => totals.passed += 1,
            StepStatus::Failed => totals.failed += 1,
            StepStatus::Skipped => totals.skipped += 1,
            StepStatus::Unknown => totals.unknown += 1,
        }
    }
    totals
}

fn build_report_summary(
    _analyses: &[StepAnalysis],
    totals: &StepTotals,
    overall_ok: bool,
) -> String {
    if totals.total == 0 {
        return if overall_ok {
            "回放完成（脚本未提供可解析的步骤清单）".to_string()
        } else {
            "回放失败（脚本未提供可解析的步骤清单）".to_string()
        };
    }
    if overall_ok && totals.failed == 0 && totals.unknown == 0 {
        return format!("{} 个步骤：全部通过", totals.total);
    }
    let mut parts = vec![format!("通过 {}", totals.passed)];
    if totals.failed > 0 {
        parts.push(format!("失败 {}", totals.failed));
    }
    if totals.skipped > 0 {
        parts.push(format!("未执行 {}", totals.skipped));
    }
    if totals.unknown > 0 {
        parts.push(format!("未知 {}", totals.unknown));
    }
    format!("{} 个步骤：{}", totals.total, parts.join(" · "))
}

/// Escape a cell value for a GitHub-flavored Markdown table.
fn escape_md_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\r', " ").replace('\n', " ")
}

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 60_000 {
        let minutes = duration_ms / 60_000;
        let seconds = (duration_ms % 60_000) / 1000;
        format!("{minutes} 分 {seconds} 秒")
    } else if duration_ms >= 1_000 {
        format!("{:.1} 秒", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms} ms")
    }
}

/// Best-effort `(session_name, start_url)` from the original recording trace.
async fn load_trace_info(recordings_dir: &Path, id: &str) -> (String, String) {
    let Ok(content) = tokio::fs::read_to_string(recordings_dir.join(format!("{id}.trace.json")))
        .await
    else {
        return (String::new(), String::new());
    };
    let Ok(trace) = serde_json::from_str::<TraceFile>(&content) else {
        return (String::new(), String::new());
    };
    (
        trace.session_name.clone(),
        effective_start_url(&trace),
    )
}

fn trim_run_output(output: &str, max_chars: usize) -> String {
    if output.chars().count() <= max_chars {
        return output.to_string();
    }
    let kept: String = output.chars().skip(output.chars().count() - max_chars).collect();
    format!("…（已截断，仅保留末尾内容）\n{kept}")
}

fn render_report_markdown(context: &ReportBuildContext) -> String {
    let ReportBuildContext {
        id,
        script_name,
        start_url,
        script_path,
        ok,
        stopped_by_user,
        duration_label,
        timestamp_label,
        analyses,
        totals: _totals,
        summary,
        result,
    } = context;
    let status_icon = if *ok { "✅" } else { "❌" };
    let status_label = if *stopped_by_user {
        "已由用户停止"
    } else if *ok {
        "成功"
    } else {
        "失败"
    };
    let display_name = if script_name.trim().is_empty() {
        id
    } else {
        script_name
    };

    let mut md = String::new();
    md.push_str("# 回放测试报告\n\n");
    md.push_str(&format!(
        "- **脚本名称**：{}\n",
        escape_md_cell(display_name)
    ));
    md.push_str(&format!("- **脚本 ID**：`{}`\n", escape_md_cell(id)));
    if !start_url.is_empty() {
        md.push_str(&format!(
            "- **起始页面**：{}\n",
            escape_md_cell(start_url)
        ));
    }
    if !script_path.is_empty() {
        md.push_str(&format!(
            "- **脚本路径**：`{}`\n",
            escape_md_cell(script_path)
        ));
    }
    md.push_str(&format!("- **运行时间**：{timestamp_label}\n"));
    md.push_str(&format!("- **总耗时**：{duration_label}\n"));
    md.push_str(&format!(
        "- **整体结果**：{status_icon} {status_label}\n"
    ));
    md.push_str(&format!("- **步骤统计**：{summary}\n"));
    if *stopped_by_user {
        md.push_str("\n> 本次回放在结束前被用户手动停止。\n");
    }

    if !analyses.is_empty() {
        md.push_str("\n## 步骤检查结果\n\n");
        md.push_str("| 序号 | 步骤 | 状态 | 说明 |\n|---|---|---|---|\n");
        for analysis in analyses.iter() {
            let note = analysis.note.as_deref().unwrap_or("");
            md.push_str(&format!(
                "| {} | {} | {} {} | {} |\n",
                analysis.num,
                escape_md_cell(&analysis.title),
                analysis.status.icon(),
                analysis.status.label(),
                escape_md_cell(note)
            ));
        }
    } else {
        md.push_str("\n## 步骤检查结果\n\n未能从脚本中解析出带编号的步骤清单，请参考下方原始输出确认执行情况。\n");
    }

    md.push_str("\n## 原始输出\n\n");
    if !result.stderr.trim().is_empty() {
        md.push_str("### stderr\n\n```text\n");
        md.push_str(&trim_run_output(result.stderr.trim(), 6000));
        md.push_str("\n```\n");
    }
    if !result.stdout.trim().is_empty() {
        md.push_str("### stdout\n\n```text\n");
        md.push_str(&trim_run_output(result.stdout.trim(), 8000));
        md.push_str("\n```\n");
    }
    if let Some(error) = result.error.as_deref() {
        if !error.trim().is_empty() {
            md.push_str(&format!(
                "\n### 错误摘要\n\n```text\n{}\n```\n",
                error.trim()
            ));
        }
    }
    md
}

/// Everything needed to render one replay test report.
struct ReportBuildContext<'a> {
    id: &'a str,
    script_name: &'a str,
    start_url: &'a str,
    script_path: &'a str,
    ok: bool,
    stopped_by_user: bool,
    duration_label: String,
    timestamp_label: String,
    analyses: &'a [StepAnalysis],
    totals: &'a StepTotals,
    summary: &'a str,
    result: &'a ReplayRunResult,
}

/// Directory holding per-script Markdown test reports.
pub fn reports_root(scripts_dir_path: &Path) -> PathBuf {
    scripts_dir_path.join("reports")
}

fn script_reports_dir(scripts_dir_path: &Path, id: &str) -> PathBuf {
    reports_root(scripts_dir_path).join(id)
}

fn format_timestamp_label(millis: i64) -> String {
    chrono::DateTime::<chrono::Local>::from(
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis.max(0) as u64),
    )
    .format("%Y-%m-%d %H:%M:%S")
    .to_string()
}

async fn list_reports_in(dir: &Path) -> Vec<ReplayReportMeta> {
    let mut metas = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return metas;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let created_at = stem.parse::<i64>().unwrap_or_else(|_| {
            tokio::task::block_in_place(|| {
                std::fs::metadata(&path)
            })
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0)
        });
        // The sidecar json is optional; fall back to scanning the markdown.
        let mut ok = false;
        let mut summary = String::new();
        if let Ok(content) =
            tokio::fs::read_to_string(path.with_extension("json")).await
        {
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
                ok = meta
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                summary = meta
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
        }
        if summary.is_empty() {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                for line in content.lines() {
                    if let Some(rest) = line.trim().strip_prefix("- **步骤统计**：") {
                        summary = rest.trim().to_string();
                    }
                    let trimmed = line.trim_start_matches('-').trim_start();
                    if trimmed.starts_with("**整体结果**") {
                        ok = trimmed.contains("成功");
                    }
                }
            }
        }
        metas.push(ReplayReportMeta {
            id: stem.to_string(),
            script_id: dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string(),
            path: normalize_display_path(&path),
            created_at,
            ok,
            summary,
        });
    }
    metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    metas
}

fn normalize_display_path(path: &Path) -> String {
    let lossy = path.to_string_lossy().replace("\\\\?\\", "");
    lossy
}

/// List persisted reports for one script, newest first.
pub async fn list_reports(
    recordings_dir: &Path,
    id: &str,
) -> Result<Vec<ReplayReportMeta>, String> {
    validate_script_id(id)?;
    let dir = script_reports_dir(&scripts_dir(recordings_dir), id);
    Ok(list_reports_in(&dir).await)
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
            .all(|ch| {
                // 除 ASCII 字母数字/`-`/`_` 外，允许非 ASCII 可见字符
                //（如中文文件名导入的输入文档 id）；路径分隔符、控制字符
                // 与空白仍然拒绝，防止目录穿越。
                ch.is_ascii_alphanumeric()
                    || ch == '-'
                    || ch == '_'
                    || (!ch.is_ascii() && !ch.is_control() && !ch.is_whitespace())
            })
        || id.contains(['\\', '/'])
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
    let collected: String = chars.collect();
    // 截断 print 调用尾巴（如 `……", flush=True)` / `……")`）：在首个引号处切分。
    let cleaned = collected
        .split('"')
        .next()
        .and_then(|part| part.split('\'').next())
        .unwrap_or_default();
    let title = strip_step_title_noise(cleaned);
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
        // Input document presence + step preview for the panel card.
        let input_doc_path = input_document_path(&dir, &id);
        let has_input_document = tokio::fs::try_exists(&input_doc_path).await.unwrap_or(false);
        let mut input_field_count: Option<usize> = None;
        if let Ok(content) = tokio::fs::read_to_string(&input_doc_path).await {
            if let Ok(doc) = serde_json::from_str::<ReplayInputDocument>(&content) {
                input_field_count = Some(doc.fields.len());
            }
        }

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

        let reports = list_reports_in(&script_reports_dir(&dir, &id)).await;
        let report_count = reports.len();
        let last_report_at = reports.first().map(|report| report.created_at);

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
            report_count,
            last_report_at,
            has_input_document,
            input_field_count,
            imported_only: false,
        });
    }

    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    // CSV 导入的独立输入文档（无同名 .py）：作为 `imported_only` 伪条目返回，
    // 让前端可以像脚本卡片一样展示、打开详情与删除。
    let mut all_metas = metas;
    let mut seen_ids: HashSet<String> = all_metas.iter().map(|meta| meta.id.clone()).collect();
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read scripts dir: {e}"))?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json")
            || !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.ends_with(".input.json"))
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = stem.strip_suffix(".input").unwrap_or(stem);
        if id.is_empty() || seen_ids.contains(id) {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<ReplayInputDocument>(&content) else {
            continue;
        };
        seen_ids.insert(id.to_string());
        let modified = tokio::fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(now_millis);
        all_metas.push(ReplayScriptMeta {
            id: id.to_string(),
            name: if doc.name.trim().is_empty() {
                id.to_string()
            } else {
                doc.name.trim().to_string()
            },
            path: path.to_string_lossy().to_string(),
            trace_session_id: String::new(),
            created_at: doc.created_at,
            updated_at: modified,
            step_count: 0,
            steps: Vec::new(),
            start_url: String::new(),
            last_status: None,
            last_error: None,
            last_run_at: None,
            report_count: 0,
            last_report_at: None,
            has_input_document: true,
            input_field_count: Some(doc.fields.len()),
            imported_only: true,
        });
    }

    all_metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(all_metas)
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

/// Read one persisted Markdown report by script and report id.
pub async fn read_report(
    recordings_dir: &Path,
    id: &str,
    report_id: &str,
) -> Result<ReplayReadResult, String> {
    validate_script_id(id)?;
    if report_id.is_empty()
        || !report_id
            .chars()
            .all(|ch| ch.is_ascii_digit())
    {
        return Err("Invalid report id".to_string());
    }
    let dir = script_reports_dir(&scripts_dir(recordings_dir), id);
    let path = dir.join(format!("{report_id}.md"));
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read replay report {id}/{report_id}: {e}"))?;
    Ok(ReplayReadResult {
        id: format!("{id}/{report_id}"),
        path: normalize_display_path(&path),
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
    let reports_path = script_reports_dir(&dir, id);

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
    if reports_path.exists() {
        let _ = tokio::fs::remove_dir_all(&reports_path).await;
    }
    cleanup_input_document(&dir, &id);
    Ok(())
}

/// Delete an imported input document card (`.input.json` sidecar only).
pub async fn delete_input_document(recordings_dir: &Path, id: &str) -> Result<(), String> {
    validate_script_id(id)?;
    let dir = scripts_dir(recordings_dir);
    let path = input_document_path(&dir, id);
    if !tokio::fs::try_exists(&path)
        .await
        .map_err(|e| format!("Failed to inspect input document {id}: {e}"))?
    {
        return Err(format!("Input document not found: {id}"));
    }
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("Failed to delete input document {id}: {e}"))?;
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

    // 运行前确保输入文档存在（脚本启动时会按文件名加载它作为输入数据），
    // 老脚本也能自动补建；以当前磁盘上的 JSON 为准，改动后无需重新生成脚本。
    ensure_input_document(recordings_dir, &dir, id).await?;

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
            // 输入文档路径：脚本统一从该环境变量读取输入数据。
            .env("REPLAY_INPUT_FILE", input_document_path(&dir, id))
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
                    let result = persist_last_run(&dir, id, result).await;
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
            let result = persist_last_run(&dir, id, result).await;
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
                let result = persist_last_run(&dir, id, result).await;
                return Ok(result);
            }
            Ok(Err(e)) => {
                let stop_requested = finish_replay(id).await;
                let result = if stop_requested {
                    stopped_result(started, String::new(), e.to_string(), None)
                } else {
                    failed_result(started, String::new(), e.to_string(), None, e.to_string())
                };
                let result = persist_last_run(&dir, id, result).await;
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
                        let result = persist_last_run(&dir, id, result).await;
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
                    report: None,
                };

                let result = persist_last_run(&dir, id, result).await;
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
    let result = persist_last_run(&dir, id, result).await;
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
        report: None,
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
        report: None,
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

async fn persist_last_run(dir: &Path, id: &str, mut result: ReplayRunResult) -> ReplayRunResult {
    // Every run outcome gets a Markdown test report with per-step checks.
    if let Some(report) = persist_replay_report(
        dir.parent().unwrap_or(dir),
        dir,
        id,
        &result,
    )
    .await
    {
        result.report = Some(report);
    }

    let payload = json!({
        "last_status": if result.ok { "success" } else { "failed" },
        "last_error": result.error.clone(),
        "last_run_at": now_millis(),
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = tokio::fs::write(last_run_path(dir, id), json).await;
    }
    result
}

/// Build the per-step check table, render the Markdown report and persist it.
async fn persist_replay_report(
    recordings_dir: &Path,
    dir: &Path,
    id: &str,
    result: &ReplayRunResult,
) -> Option<ReplayReportMeta> {
    let report_dir = script_reports_dir(dir, id);
    if let Err(e) = tokio::fs::create_dir_all(&report_dir).await {
        eprintln!("Failed to create replay reports dir for {id}: {e}");
        return None;
    }

    let (script_name, start_url) = load_trace_info(recordings_dir, id).await;
    let script_path_lossy = dir.join(format!("{id}.py")).to_string_lossy().to_string();
    let parsed = parse_structured_output(&result.stdout);
    let overall_ok = result.ok;
    // `stopped_by_user` is already signalled through fixable=false + error text;
    // keep the nuance in the summary line rather than parsing internals again.
    let stopped_by_user = !result.ok
        && !result.fixable
        && result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("stopped by user"));
    let scan = scan_output_steps(&result.stdout);
    let expected_labels = extract_script_steps(
        &tokio::fs::read_to_string(dir.join(format!("{id}.py")))
            .await
            .unwrap_or_default(),
    );
    let analyses = analyze_step_results(
        &expected_labels,
        &scan,
        parsed.as_ref().filter(|_| !overall_ok),
        overall_ok,
    );
    let totals = count_step_statuses(&analyses);
    let summary = build_report_summary(&analyses, &totals, overall_ok);

    let created_at = now_millis();
    let context = ReportBuildContext {
        id,
        script_name: &script_name,
        start_url: &start_url,
        script_path: &script_path_lossy,
        ok: overall_ok,
        stopped_by_user,
        duration_label: format_duration_ms(result.duration_ms),
        timestamp_label: format_timestamp_label(created_at),
        analyses: &analyses,
        totals: &totals,
        summary: &summary,
        result,
    };
    let markdown = render_report_markdown(&context);
    let md_path = report_dir.join(format!("{created_at}.md"));
    if let Err(e) = tokio::fs::write(&md_path, markdown.as_bytes()).await {
        eprintln!("Failed to write replay report for {id}: {e}");
        return None;
    }
    let sidecar = json!({
        "ok": overall_ok,
        "summary": summary,
        "createdAt": created_at,
        "durationMs": result.duration_ms,
        "stepsTotal": totals.total,
        "stepsPassed": totals.passed,
        "stepsFailed": totals.failed,
        "stepsSkipped": totals.skipped,
        "stepsUnknown": totals.unknown,
    });
    if let Ok(encoded) = serde_json::to_string_pretty(&sidecar) {
        let _ = tokio::fs::write(md_path.with_extension("json"), encoded).await;
    }

    Some(ReplayReportMeta {
        id: created_at.to_string(),
        script_id: id.to_string(),
        path: normalize_display_path(&md_path),
        created_at,
        ok: overall_ok,
        summary,
    })
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
        // 非 ASCII id（中文 CSV 文件名）应被接受；路径片段仍被拒绝。
        assert!(validate_script_id("测试-用户").is_ok());
        assert!(validate_script_id("a/b").is_err());
        assert!(validate_script_id("a\\b").is_err());
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

    #[test]
    fn scan_output_steps_marks_done_and_reports_success_marker() {
        let stdout = "步骤 1：打开页面……\n步骤 1：打开页面 —— 完成\n步骤 2：点击登录\n[OK] 回放成功\n";
        let scan = scan_output_steps(stdout);
        assert_eq!(scan.started_nums(), vec![1, 2]);
        assert!(scan.contains_done(1));
        // final success marker promotes the trailing unfinished step too
        assert!(scan.all_completed());
        assert!(scan.final_success);
    }

    #[test]
    fn analyze_step_results_attributes_failure_to_unfinished_step() {
        let labels = vec![
            "步骤 1：打开入口页面".to_string(),
            "步骤 2：输入账号".to_string(),
            "步骤 3：勾选协议".to_string(),
        ];
        let stdout = "步骤 1：打开入口页面 —— 完成\n步骤 2：输入账号……\n";
        let parsed = serde_json::json!({"ok": false, "step": 2, "error": "未找到账号输入框"});
        let scan = scan_output_steps(stdout);
        let analyses = analyze_step_results(&labels, &scan, Some(&parsed), false);
        assert_eq!(analyses[0].status, StepStatus::Passed);
        assert_eq!(analyses[1].status, StepStatus::Failed);
        assert_eq!(analyses[2].status, StepStatus::Skipped);
    }

    #[test]
    fn analyze_step_results_all_pass_on_success_marker() {
        let labels = vec!["步骤 1：打开入口页面".to_string(), "步骤 2：提交表单".to_string()];
        let stdout = "步骤 1：打开入口页面 —— 完成\n步骤 2：提交表单 —— 完成\n[OK] 回放成功：已完成全部流程\n";
        let scan = scan_output_steps(stdout);
        let analyses = analyze_step_results(&labels, &scan, None, true);
        assert!(analyses.iter().all(|a| a.status == StepStatus::Passed));
    }

    #[test]
    fn parse_expected_step_label_handles_spaces_and_colon_variants() {
        assert_eq!(
            parse_expected_step_label("步骤 3: 勾选协议"),
            Some((3, "勾选协议".to_string()))
        );
        assert_eq!(
            parse_expected_step_label("步骤12：填写验证码"),
            Some((12, "填写验证码".to_string()))
        );
        assert_eq!(parse_expected_step_label("其他内容"), None);
    }

    fn sample_trace() -> TraceFile {
        serde_json::from_str::<TraceFile>(
            r##"{
                "sessionId": "sess-1",
                "sessionName": "登录门户",
                "startUrl": "https://example.test",
                "startedAt": "2024-01-01T00:00:00Z",
                "stoppedAt": "2024-01-01T00:01:00Z",
                "events": [
                    {
                        "type": "navigate",
                        "timestamp": 1,
                        "url": "https://example.test",
                        "cause": "user",
                        "selector": "",
                        "selectorCandidates": [],
                        "tagName": ""
                    },
                    {
                        "type": "type",
                        "timestamp": 2,
                        "url": "https://example.test/login",
                        "selector": "#account",
                        "selectorCandidates": ["#account", "input[name=user]"],
                        "tagName": "input",
                        "value": "ju",
                        "previousValue": "",
                        "inputType": "insertText"
                    },
                    {
                        "type": "type",
                        "timestamp": 3,
                        "url": "https://example.test/login",
                        "selector": "#account",
                        "selectorCandidates": ["#account"],
                        "tagName": "input",
                        "value": "junlong@example.com",
                        "previousValue": "ju",
                        "inputType": "insertText"
                    },
                    {
                        "type": "select",
                        "timestamp": 4,
                        "url": "https://example.test/login",
                        "selector": "#role",
                        "selectorCandidates": ["#role"],
                        "tagName": "select",
                        "value": "admin"
                    },
                    {
                        "type": "click",
                        "timestamp": 5,
                        "url": "https://example.test/login",
                        "selector": "#submit",
                        "selectorCandidates": ["#submit"],
                        "tagName": "button"
                    }
                ]
            }"##,
        )
        .expect("sample trace should parse")
    }

    #[test]
    fn derive_input_document_merges_typing_and_renumbers_steps() {
        let doc = derive_input_document_from_trace(&sample_trace());
        assert_eq!(doc.id, "sess-1");
        // navigate(user) + 合并后的最终输入 + select + click = 4 个字段。
        assert_eq!(doc.fields.len(), 4);
        assert_eq!(doc.fields[0].action_type, "navigate");
        assert_eq!(doc.fields[0].value, "https://example.test");
        assert_eq!(
            doc.fields[1].action_type,
            "type"
        );
        assert_eq!(doc.fields[1].value, "junlong@example.com");
        assert_eq!(doc.fields[1].selector, "#account");
        assert_eq!(doc.fields[1].step, 2);
        assert_eq!(doc.fields[2].action_type, "select");
        assert_eq!(doc.fields[2].value, "admin");
        assert_eq!(doc.fields[3].action_type, "click");
    }

    #[test]
    fn parse_csv_input_document_skips_header_and_builds_fields() {
        let parsed = parse_csv_input_document("#comment not real csv\n").unwrap_err();
        assert!(parsed.contains("为空") || parsed.contains("数据行"));

        let result = parse_csv_input_document("selector,value\n#user,alice\n,secret123\n")
            .expect("csv should parse");
        assert!(result.header_detected);
        assert_eq!(result.field_count, 2);
        assert_eq!(result.fields[0].selector, "#user");
        assert_eq!(result.fields[0].value, "alice");
        assert_eq!(result.fields[0].step, 1);
        assert_eq!(result.fields[1].selector, "");
        assert_eq!(result.fields[1].value, "secret123");
        assert_eq!(result.suggested_name.as_deref(), Some("alice"));
    }

    #[test]
    fn parse_csv_rows_supports_quoted_commas_and_crlf() {
        let rows = parse_csv_rows("a,\"b,c\"\r\n\"say \"\"hi\"\"\",d\r\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a".to_string(), "b,c".to_string()]);
        assert_eq!(rows[1], vec!["say \"hi\"".to_string(), "d".to_string()]);
    }

    #[test]
    fn sanitize_import_script_id_keeps_safe_characters_only() {
        assert_eq!(sanitize_import_script_id("测试 用户"), "测试-用户");
        assert_eq!(sanitize_import_script_id("2025-08 Login"), "2025-08-login");
        let fallback = sanitize_import_script_id("***");
        assert!(fallback.starts_with("csv-import-"));
    }
}
