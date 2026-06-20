use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Url, WebviewUrl,
    WebviewWindowBuilder, Window, webview::WebviewBuilder,
};

use crate::error::{AppError, AppResult};
use crate::state::{AppState, RunSummaryDiffPayload};

const BROWSER_WEBVIEW_LABEL: &str = "cn-browser";
const BROWSER_DEBUG_PORT: u16 = 9242;
const BROWSER_ENDPOINT_FILE: &str = "visible-browser.json";
const DOCUMENT_DETAIL_WINDOW_LABEL: &str = "document-detail";
const DOCUMENT_DETAIL_OPEN_EVENT: &str = "document-detail-open";
const DOCUMENT_DETAIL_INSERT_EVENT: &str = "document-detail-insert-snippet";
const RUNSUMMARY_DIFF_WINDOW_LABEL: &str = "runsummary-diff";
const RUNSUMMARY_DIFF_OPEN_EVENT: &str = "runsummary-diff-open";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWindowInfo {
    pub label: String,
    pub url: String,
    pub created: bool,
    pub debug_port: u16,
    pub cdp_endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDetailWindowInfo {
    pub label: String,
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryDiffWindowInfo {
    pub label: String,
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserEndpointMetadata {
    label: String,
    url: String,
    debug_port: u16,
    cdp_endpoint: String,
    updated_at_ms: u128,
}

#[tauri::command]
pub fn window_start_dragging(window: Window) -> AppResult<()> {
    window.start_dragging()?;
    Ok(())
}

#[tauri::command]
pub fn window_minimize(window: Window) -> AppResult<()> {
    window.minimize()?;
    Ok(())
}

#[tauri::command]
pub fn window_toggle_maximize(window: Window) -> AppResult<()> {
    if window.is_maximized()? {
        window.unmaximize()?;
    } else {
        window.maximize()?;
    }
    Ok(())
}

#[tauri::command]
pub fn window_close(window: Window) -> AppResult<()> {
    window.close()?;
    Ok(())
}

#[tauri::command]
pub fn window_show_main(app: AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("main") {
        // 仅在隐藏状态下执行 show，避免重复 show 导致额外窗口闪动。
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn window_open_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
) -> AppResult<BrowserWindowInfo> {
    let pos_x = x.unwrap_or(0.0);
    let pos_y = y.unwrap_or(0.0);
    let w = width.unwrap_or(400.0);
    let h = height.unwrap_or(600.0);
    open_browser_embedded(
        &app,
        &state.workspace_config_dir,
        url.as_deref(),
        pos_x,
        pos_y,
        w,
        h,
    )
}

#[tauri::command]
pub async fn window_resize_browser(
    app: AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> AppResult<()> {
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.set_position(LogicalPosition::new(x, y))?;
        webview.set_size(LogicalSize::new(width, height))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn window_navigate_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> AppResult<()> {
    let browser_url = normalize_browser_url(Some(&url))?;
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.navigate(browser_url.clone())?;
        let info = BrowserWindowInfo {
            label: BROWSER_WEBVIEW_LABEL.to_string(),
            url: browser_url.as_str().to_string(),
            created: false,
            debug_port: BROWSER_DEBUG_PORT,
            cdp_endpoint: browser_cdp_endpoint(),
        };
        write_browser_endpoint_metadata(&state.workspace_config_dir, &info)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn window_close_browser(app: AppHandle) -> AppResult<()> {
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.close()?;
    }
    Ok(())
}

#[tauri::command]
pub async fn window_open_document_detail(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<DocumentDetailWindowInfo> {
    let (display_path, _) = resolve_existing_file_path(&path)?;
    {
        // 先写入“当前目标路径”，保障详情窗首次启动时可通过 command 主动读取。
        let mut guard = state.document_detail_active_path.write().await;
        *guard = Some(display_path.clone());
    }

    if let Some(window) = app.get_webview_window(DOCUMENT_DETAIL_WINDOW_LABEL) {
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
        window.set_focus()?;
        let _ = app.emit_to(
            DOCUMENT_DETAIL_WINDOW_LABEL,
            DOCUMENT_DETAIL_OPEN_EVENT,
            serde_json::json!({ "path": display_path.clone() }),
        );
        return Ok(DocumentDetailWindowInfo {
            label: DOCUMENT_DETAIL_WINDOW_LABEL.to_string(),
            path: display_path,
            created: false,
        });
    }

    let detail_window = WebviewWindowBuilder::new(
        &app,
        DOCUMENT_DETAIL_WINDOW_LABEL,
        document_detail_window_url()?,
    )
    .title("文档详情")
    .inner_size(1060.0, 760.0)
    .min_inner_size(760.0, 520.0)
    .resizable(true)
    .decorations(false)
    .build()?;

    // 允许用户最小化后再次打开时回到前台，保持“单实例复用”行为一致。
    detail_window.show()?;
    detail_window.set_focus()?;

    let active_path = state.document_detail_active_path.clone();
    detail_window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let active_path = active_path.clone();
            tauri::async_runtime::spawn(async move {
                let mut guard = active_path.write().await;
                *guard = None;
            });
        }
    });

    Ok(DocumentDetailWindowInfo {
        label: DOCUMENT_DETAIL_WINDOW_LABEL.to_string(),
        path: display_path,
        created: true,
    })
}

#[tauri::command]
pub async fn window_close_document_detail(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(DOCUMENT_DETAIL_WINDOW_LABEL) {
        window.close()?;
    }
    let mut guard = state.document_detail_active_path.write().await;
    *guard = None;
    Ok(())
}

#[tauri::command]
pub async fn window_get_document_detail_path(
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    Ok(state.document_detail_active_path.read().await.clone())
}

#[tauri::command]
pub async fn window_open_runsummary_diff(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: RunSummaryDiffPayload,
) -> AppResult<RunSummaryDiffWindowInfo> {
    // 统一在命令入口完成路径与枚举值规范化，避免前端多入口造成不一致。
    let normalized_payload = normalize_runsummary_diff_payload(payload);
    {
        // 先缓存 payload，确保新窗口首帧可通过 get command 拉到最新数据。
        let mut guard = state.runsummary_diff_payload.write().await;
        *guard = Some(normalized_payload.clone());
    }

    if let Some(window) = app.get_webview_window(RUNSUMMARY_DIFF_WINDOW_LABEL) {
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
        window.set_focus()?;
        let _ = app.emit_to(
            RUNSUMMARY_DIFF_WINDOW_LABEL,
            RUNSUMMARY_DIFF_OPEN_EVENT,
            normalized_payload.clone(),
        );
        return Ok(RunSummaryDiffWindowInfo {
            label: RUNSUMMARY_DIFF_WINDOW_LABEL.to_string(),
            path: normalized_payload.path,
            created: false,
        });
    }

    let diff_window = WebviewWindowBuilder::new(
        &app,
        RUNSUMMARY_DIFF_WINDOW_LABEL,
        runsummary_diff_window_url()?,
    )
    .title("RunSummary Diff")
    .inner_size(1060.0, 760.0)
    .min_inner_size(760.0, 520.0)
    .resizable(true)
    .decorations(false)
    .build()?;

    diff_window.show()?;
    diff_window.set_focus()?;

    let active_payload = state.runsummary_diff_payload.clone();
    diff_window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let active_payload = active_payload.clone();
            tauri::async_runtime::spawn(async move {
                let mut guard = active_payload.write().await;
                *guard = None;
            });
        }
    });

    Ok(RunSummaryDiffWindowInfo {
        label: RUNSUMMARY_DIFF_WINDOW_LABEL.to_string(),
        path: normalized_payload.path,
        created: true,
    })
}

#[tauri::command]
pub async fn window_close_runsummary_diff(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(RUNSUMMARY_DIFF_WINDOW_LABEL) {
        window.close()?;
    }
    let mut guard = state.runsummary_diff_payload.write().await;
    *guard = None;
    Ok(())
}

#[tauri::command]
pub async fn window_get_runsummary_diff_payload(
    state: State<'_, AppState>,
) -> AppResult<Option<RunSummaryDiffPayload>> {
    Ok(state.runsummary_diff_payload.read().await.clone())
}

#[tauri::command]
pub fn document_detail_insert_snippet(app: AppHandle, snippet: String) -> AppResult<()> {
    let payload_text = snippet.trim();
    if payload_text.is_empty() {
        return Err(AppError::Custom(
            "Snippet content is empty, cannot insert into chat.".to_string(),
        ));
    }
    app.emit_to(
        "main",
        DOCUMENT_DETAIL_INSERT_EVENT,
        DocumentDetailInsertEvent {
            snippet: payload_text.to_string(),
        },
    )?;
    Ok(())
}

pub fn open_browser_embedded(
    app: &AppHandle,
    workspace_config_dir: &Path,
    url: Option<&str>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> AppResult<BrowserWindowInfo> {
    let should_navigate_existing = url.is_some_and(|value| !value.trim().is_empty());
    let browser_url = normalize_browser_url(url)?;
    let url_string = browser_url.as_str().to_string();
    let cdp_endpoint = browser_cdp_endpoint();

    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        if should_navigate_existing {
            webview.navigate(browser_url)?;
        }
        webview.set_position(LogicalPosition::new(x, y))?;
        webview.set_size(LogicalSize::new(width, height))?;
        let info = BrowserWindowInfo {
            label: BROWSER_WEBVIEW_LABEL.to_string(),
            url: url_string,
            created: false,
            debug_port: BROWSER_DEBUG_PORT,
            cdp_endpoint,
        };
        write_browser_endpoint_metadata(workspace_config_dir, &info)?;
        return Ok(info);
    }

    let browser_dir = workspace_config_dir.join("browser");
    fs::create_dir_all(&browser_dir)?;

    let main_window = app
        .get_window("main")
        .ok_or_else(|| AppError::Custom("Main window not found".to_string()))?;

    let webview_builder =
        WebviewBuilder::new(BROWSER_WEBVIEW_LABEL, WebviewUrl::External(browser_url))
            .enable_clipboard_access()
            .data_directory(browser_dir.join("webview-data"))
            .additional_browser_args(&browser_additional_args());

    main_window.add_child(
        webview_builder,
        LogicalPosition::new(x, y),
        LogicalSize::new(width, height),
    )?;

    let info = BrowserWindowInfo {
        label: BROWSER_WEBVIEW_LABEL.to_string(),
        url: url_string,
        created: true,
        debug_port: BROWSER_DEBUG_PORT,
        cdp_endpoint,
    };
    write_browser_endpoint_metadata(workspace_config_dir, &info)?;
    Ok(info)
}

pub fn browser_cdp_endpoint() -> String {
    format!("http://127.0.0.1:{BROWSER_DEBUG_PORT}")
}

fn browser_additional_args() -> String {
    format!("--remote-debugging-port={BROWSER_DEBUG_PORT} --remote-allow-origins=*")
}

fn write_browser_endpoint_metadata(
    workspace_config_dir: &Path,
    info: &BrowserWindowInfo,
) -> AppResult<()> {
    let browser_dir = workspace_config_dir.join("browser");
    fs::create_dir_all(&browser_dir)?;
    let updated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let metadata = BrowserEndpointMetadata {
        label: info.label.clone(),
        url: info.url.clone(),
        debug_port: info.debug_port,
        cdp_endpoint: info.cdp_endpoint.clone(),
        updated_at_ms,
    };
    fs::write(
        browser_dir.join(BROWSER_ENDPOINT_FILE),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    Ok(())
}

fn normalize_browser_url(raw: Option<&str>) -> AppResult<Url> {
    let trimmed = raw.unwrap_or("").trim();
    let candidate = if trimmed.is_empty() {
        "about:blank".to_string()
    } else if looks_like_local_dev_host(trimmed) {
        format!("http://{trimmed}")
    } else if has_explicit_scheme(trimmed) {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let parsed = Url::parse(&candidate)
        .map_err(|e| AppError::Custom(format!("Invalid browser URL '{candidate}': {e}")))?;
    match parsed.scheme() {
        "http" | "https" | "about" => Ok(parsed),
        scheme => Err(AppError::Custom(format!(
            "Unsupported browser URL scheme '{scheme}'. Use http, https, or about."
        ))),
    }
}

fn has_explicit_scheme(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("about:")
        || lower.contains("://")
        || lower.starts_with("file:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("mailto:")
        || lower.starts_with("ftp:")
        || lower.starts_with("chrome:")
        || lower.starts_with("edge:")
}

fn looks_like_local_dev_host(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower == "localhost"
        || lower.starts_with("localhost:")
        || lower.starts_with("localhost/")
        || lower == "127.0.0.1"
        || lower.starts_with("127.")
        || lower.starts_with("[::1]")
        || lower.starts_with("::1")
}

fn normalize_windows_verbatim_prefix(raw: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
        return trimmed.to_string();
    }

    #[cfg(not(target_os = "windows"))]
    {
        raw.trim().to_string()
    }
}

fn normalize_runsummary_diff_payload(mut payload: RunSummaryDiffPayload) -> RunSummaryDiffPayload {
    // 路径统一去除 Windows `\\?\` 前缀，前后端展示和写盘命令都使用同一形态。
    payload.path = normalize_windows_verbatim_prefix(&payload.path);
    payload.file_action = payload.file_action.trim().to_lowercase();
    payload.diff_source = payload.diff_source.trim().to_lowercase();
    payload
}

fn resolve_existing_file_path(path: &str) -> AppResult<(String, std::path::PathBuf)> {
    // 所有“文档详情窗相关读写”统一使用同一套路径标准化逻辑，
    // 避免 Windows `\\?\` 前缀在不同命令中的判定不一致。
    let display_path = normalize_windows_verbatim_prefix(path);
    let file_path = std::path::PathBuf::from(&display_path);
    if !file_path.is_file() {
        return Err(AppError::Custom(format!(
            "Path is not a file: {display_path}"
        )));
    }
    Ok((display_path, file_path))
}

fn document_detail_window_url() -> AppResult<WebviewUrl> {
    #[cfg(debug_assertions)]
    {
        // 开发环境下详情窗走 Vite dev server 的独立入口，
        // 这样可以和主窗分离打包逻辑并保持热更新体验。
        let dev_url =
            std::env::var("TAURI_DEV_URL").unwrap_or_else(|_| "http://localhost:1420".to_string());
        let base = dev_url.trim().trim_end_matches('/');
        let final_url = format!("{base}/detail.html");
        let parsed = Url::parse(&final_url).map_err(|e| {
            AppError::Custom(format!(
                "Invalid document detail dev url '{final_url}': {e}"
            ))
        })?;
        return Ok(WebviewUrl::External(parsed));
    }

    #[cfg(not(debug_assertions))]
    {
        // 生产构建使用打包后的 detail.html 入口，避免与主窗 UI 互相污染。
        Ok(WebviewUrl::App("detail.html".into()))
    }
}

fn runsummary_diff_window_url() -> AppResult<WebviewUrl> {
    #[cfg(debug_assertions)]
    {
        // 开发环境下独立 Diff 窗走独立入口，确保主窗与 Diff 窗构建隔离且支持热更新。
        let dev_url =
            std::env::var("TAURI_DEV_URL").unwrap_or_else(|_| "http://localhost:1420".to_string());
        let base = dev_url.trim().trim_end_matches('/');
        let final_url = format!("{base}/diff.html");
        let parsed = Url::parse(&final_url).map_err(|e| {
            AppError::Custom(format!(
                "Invalid runsummary diff dev url '{final_url}': {e}"
            ))
        })?;
        return Ok(WebviewUrl::External(parsed));
    }

    #[cfg(not(debug_assertions))]
    {
        // 生产构建使用打包后的 diff.html，避免主窗内弹层造成交互耦合。
        Ok(WebviewUrl::App("diff.html".into()))
    }
}

#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> AppResult<()> {
    // 统一移除 Windows 扩展前缀，确保 exists 校验与 explorer 打开行为一致。
    let display_path = normalize_windows_verbatim_prefix(&path);
    let p = std::path::Path::new(&display_path);
    if !p.exists() {
        return Err(AppError::Custom(format!(
            "Path does not exist: {display_path}"
        )));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let win_path = display_path.replace('/', "\\");
        if p.is_dir() {
            std::process::Command::new("explorer")
                .arg(&win_path)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open explorer: {e}")))?;
        } else {
            std::process::Command::new("explorer")
                .arg(format!("/select,{win_path}"))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open explorer: {e}")))?;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if p.is_dir() {
            std::process::Command::new("open")
                .arg(&display_path)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open finder: {e}")))?;
        } else {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&display_path)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open finder: {e}")))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let dir = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Custom(format!("Failed to open file manager: {e}")))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn window_toggle_devtools(app: AppHandle) -> AppResult<()> {
    if let Some(webview) = app.get_webview_window("main") {
        if webview.is_devtools_open() {
            webview.close_devtools();
        } else {
            webview.open_devtools();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_user_home_dir() -> AppResult<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .ok_or_else(|| AppError::Custom("Could not determine user home directory".into()))?;
    Ok(super::normalize_windows_verbatim_prefix(
        &home.to_string_lossy(),
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

const HIDDEN_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    ".next",
    ".nuxt",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    "dist",
    ".vscode",
    ".idea",
    ".DS_Store",
];

#[tauri::command]
pub async fn read_directory(path: String) -> AppResult<Vec<FileEntry>> {
    let display_path = normalize_windows_verbatim_prefix(&path);
    let dir = std::path::Path::new(&display_path);
    if !dir.is_dir() {
        return Err(AppError::Custom(format!(
            "Path is not a directory: {display_path}"
        )));
    }

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let entries = fs::read_dir(dir)
        .map_err(|e| AppError::Custom(format!("Failed to read directory: {e}")))?;

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') && HIDDEN_DIRS.contains(&file_name.as_str()) {
            continue;
        }
        if HIDDEN_DIRS.contains(&file_name.as_str()) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let entry_path = entry.path();
        let abs_path = super::normalize_windows_verbatim_prefix(&entry_path.to_string_lossy());

        let fe = FileEntry {
            name: file_name,
            path: abs_path,
            is_dir: metadata.is_dir(),
            size: metadata.len(),
        };

        if metadata.is_dir() {
            dirs.push(fe);
        } else {
            files.push(fe);
        }
    }

    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    dirs.append(&mut files);
    Ok(dirs)
}

const MAX_ATTACH_SIZE: u64 = 2 * 1024 * 1024;
const MAX_TEXT_PREVIEW_SIZE: u64 = 512 * 1024;
const MAX_TEXT_PREVIEW_WRITE_SIZE: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachResult {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
    pub size: u64,
    pub source_path: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFilePreviewResult {
    pub name: String,
    pub path: String,
    pub mime_type: String,
    pub content: String,
    pub size: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFileWriteResult {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDetailInsertEvent {
    pub snippet: String,
}

#[tauri::command]
pub async fn read_file_for_attach(path: String) -> AppResult<FileAttachResult> {
    use base64::Engine;

    let (display_path, file_path) = resolve_existing_file_path(&path)?;

    let metadata = fs::metadata(&file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file metadata: {e}")))?;
    let file_size = metadata.len();
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let ext = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" | "jsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "md" => "text/markdown",
        "txt" | "log" => "text/plain",
        "toml" => "application/toml",
        "yaml" | "yml" => "text/yaml",
        "csv" => "text/csv",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "sql" => "text/x-sql",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    };

    let truncated = file_size > MAX_ATTACH_SIZE;
    let read_size = if truncated {
        MAX_ATTACH_SIZE as usize
    } else {
        file_size as usize
    };

    let bytes =
        fs::read(&file_path).map_err(|e| AppError::Custom(format!("Failed to read file: {e}")))?;
    let actual_bytes = &bytes[..read_size.min(bytes.len())];

    let b64 = base64::engine::general_purpose::STANDARD.encode(actual_bytes);
    let data_url = format!("data:{mime};base64,{b64}");

    Ok(FileAttachResult {
        name: file_name,
        mime_type: mime.to_string(),
        data_url,
        size: file_size,
        source_path: display_path,
        truncated,
    })
}

#[tauri::command]
pub async fn read_text_file_preview(path: String) -> AppResult<TextFilePreviewResult> {
    let (display_path, file_path) = resolve_existing_file_path(&path)?;

    let metadata = fs::metadata(&file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file metadata: {e}")))?;
    let file_size = metadata.len();
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mime = match ext.as_str() {
        "md" | "mdx" => "text/markdown",
        "txt" | "log" => "text/plain",
        "json" | "jsonc" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" | "jsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "toml" => "application/toml",
        "yaml" | "yml" => "text/yaml",
        "csv" => "text/csv",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "sql" => "text/x-sql",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
        _ => "text/plain",
    };

    let truncated = file_size > MAX_TEXT_PREVIEW_SIZE;
    let read_size = if truncated {
        MAX_TEXT_PREVIEW_SIZE as usize
    } else {
        file_size as usize
    };

    let bytes =
        fs::read(&file_path).map_err(|e| AppError::Custom(format!("Failed to read file: {e}")))?;
    let actual_bytes = &bytes[..read_size.min(bytes.len())];
    let content = String::from_utf8_lossy(actual_bytes).to_string();

    Ok(TextFilePreviewResult {
        name: file_name,
        path: display_path,
        mime_type: mime.to_string(),
        content,
        size: file_size,
        truncated,
    })
}

#[tauri::command]
pub async fn write_text_file_preview(
    path: String,
    content: String,
) -> AppResult<TextFileWriteResult> {
    let (display_path, file_path) = resolve_existing_file_path(&path)?;
    let metadata = fs::metadata(&file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file metadata: {e}")))?;
    // 为防止“读取时已截断但保存时覆盖整文件”的误写，这里限制仅允许处理预览上限内的文本文件。
    if metadata.len() > MAX_TEXT_PREVIEW_SIZE {
        return Err(AppError::Custom(format!(
            "File is larger than preview limit ({} bytes), save blocked for safety.",
            MAX_TEXT_PREVIEW_SIZE
        )));
    }

    let bytes = content.as_bytes();
    if bytes.len() as u64 > MAX_TEXT_PREVIEW_WRITE_SIZE {
        return Err(AppError::Custom(format!(
            "Edited content exceeds safety limit ({} bytes).",
            MAX_TEXT_PREVIEW_WRITE_SIZE
        )));
    }

    fs::write(&file_path, bytes)
        .map_err(|e| AppError::Custom(format!("Failed to write file: {e}")))?;

    Ok(TextFileWriteResult {
        path: display_path,
        size: bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_browser_url_defaults_to_about_blank() {
        assert_eq!(normalize_browser_url(None).unwrap().as_str(), "about:blank");
        assert_eq!(
            normalize_browser_url(Some("   ")).unwrap().as_str(),
            "about:blank"
        );
    }

    #[test]
    fn normalize_browser_url_adds_expected_scheme() {
        assert_eq!(
            normalize_browser_url(Some("localhost:1420"))
                .unwrap()
                .as_str(),
            "http://localhost:1420/"
        );
        assert_eq!(
            normalize_browser_url(Some("example.com")).unwrap().as_str(),
            "https://example.com/"
        );
    }

    #[test]
    fn normalize_browser_url_rejects_unsupported_schemes() {
        let err = normalize_browser_url(Some("file:///C:/secret.txt")).unwrap_err();
        assert!(err.to_string().contains("Unsupported browser URL scheme"));
    }

    #[test]
    fn browser_cdp_endpoint_uses_loopback_debug_port() {
        assert_eq!(browser_cdp_endpoint(), "http://127.0.0.1:9242");
        assert!(browser_additional_args().contains("--remote-debugging-port=9242"));
    }

    #[test]
    fn normalize_windows_verbatim_prefix_strips_verbatim_prefix() {
        assert_eq!(
            normalize_windows_verbatim_prefix(r"\\?\E:\work\cn-codex\codey"),
            r"E:\work\cn-codex\codey"
        );
        assert_eq!(
            normalize_windows_verbatim_prefix(r"\\?\UNC\server\share\folder"),
            r"\\server\share\folder"
        );
    }

    #[test]
    fn resolve_existing_file_path_accepts_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("sample.txt");
        fs::write(&file_path, b"hello").unwrap();

        let (_, resolved) = resolve_existing_file_path(&file_path.to_string_lossy()).unwrap();
        assert!(resolved.is_file());

        fs::remove_file(&file_path).unwrap();
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_existing_file_path_rejects_directory() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let err = resolve_existing_file_path(&temp_dir.to_string_lossy()).unwrap_err();
        assert!(err.to_string().contains("Path is not a file"));

        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
