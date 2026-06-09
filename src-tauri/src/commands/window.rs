use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State, Url, WebviewUrl, WebviewWindowBuilder, Window};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const BROWSER_WINDOW_LABEL: &str = "cn-browser";
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
pub async fn window_open_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: Option<String>,
) -> AppResult<BrowserWindowInfo> {
    open_browser_window(&app, &state.workspace_config_dir, url.as_deref(), true)
}

pub fn open_browser_window(
    app: &AppHandle,
    workspace_config_dir: &Path,
    url: Option<&str>,
    focus: bool,
) -> AppResult<BrowserWindowInfo> {
    let should_navigate_existing = url.is_some_and(|value| !value.trim().is_empty());
    let browser_url = normalize_browser_url(url)?;
    let url_string = browser_url.as_str().to_string();
    let cdp_endpoint = browser_cdp_endpoint();

    if let Some(browser) = app.get_webview_window(BROWSER_WINDOW_LABEL) {
        if should_navigate_existing {
            browser.navigate(browser_url)?;
        }
        browser.show()?;
        browser.unminimize()?;
        if focus {
            browser.set_focus()?;
        }
        let info = BrowserWindowInfo {
            label: BROWSER_WINDOW_LABEL.to_string(),
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

    WebviewWindowBuilder::new(app, BROWSER_WINDOW_LABEL, WebviewUrl::External(browser_url))
        .title("CN-Codex Browser")
        .inner_size(1160.0, 800.0)
        .resizable(true)
        .decorations(true)
        .focused(focus)
        .visible(true)
        .center()
        .enable_clipboard_access()
        .data_directory(browser_dir.join("webview-data"))
        .additional_browser_args(&browser_additional_args())
        .build()?;

    let info = BrowserWindowInfo {
        label: BROWSER_WINDOW_LABEL.to_string(),
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
}
