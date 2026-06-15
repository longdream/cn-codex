use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, State, Url, WebviewUrl, Window,
    webview::WebviewBuilder,
};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const BROWSER_WEBVIEW_LABEL: &str = "cn-browser";
const BROWSER_DEBUG_PORT: u16 = 9242;
const BROWSER_ENDPOINT_FILE: &str = "visible-browser.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWindowInfo {
    pub label: String,
    pub url: String,
    pub created: bool,
    pub debug_port: u16,
    pub cdp_endpoint: String,
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
    Ok(super::normalize_windows_verbatim_prefix(&home.to_string_lossy()))
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

#[tauri::command]
pub async fn read_file_for_attach(path: String) -> AppResult<FileAttachResult> {
    use base64::Engine;

    let display_path = normalize_windows_verbatim_prefix(&path);
    let file_path = std::path::Path::new(&display_path);
    if !file_path.is_file() {
        return Err(AppError::Custom(format!(
            "Path is not a file: {display_path}"
        )));
    }

    let metadata = fs::metadata(file_path)
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

    let bytes = fs::read(file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file: {e}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
